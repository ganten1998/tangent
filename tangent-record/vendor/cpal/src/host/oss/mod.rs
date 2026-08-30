//! The Open Sound System host — FreeBSD's native audio interface.
//!
//! # Why this host exists
//!
//! cpal already runs on FreeBSD: it selects the ALSA host, which works because
//! FreeBSD's `audio/alsa-lib` port implements the ALSA API on top of OSS. That
//! is enough to make sound and it is what the platform default should stay.
//!
//! What it is not is fast, and the reason is worth stating because it is not a
//! guess. Measured on FreeBSD 15 with `jack_iodelay` and a USB interface, round
//! trip through the alsa-lib shim is about 27.6 ms and does not move: the
//! requested buffer size does nothing between 128 and 1024 frames, the device's
//! native rate is worth 0.4 ms, `hw.snd.latency`/`usefrags`/`bitperfect`/vchans
//! are worth under a millisecond between them, and opening the raw `oss` PCM
//! instead of `default` — which removes alsa-lib's `plug` conversion layer
//! entirely — lands on the *same* number with worse jitter. The same hardware
//! through JACK, which talks to `/dev/dsp` directly, reaches 11.2 ms.
//!
//! So the cost is alsa-lib's own buffering, it exposes no knob for it, and the
//! only way past it is to speak OSS directly. That is this file.
//!
//! # What it does better than the shim
//!
//! * **The buffer size means something.** `BufferSize::Fixed(n)` becomes a real
//!   `SNDCTL_DSP_SETFRAGMENT`, and the driver reports back what it actually
//!   granted. Through the shim the same request changes nothing at all.
//! * **The device list is real.** The shim exposes no ALSA *cards*, so cpal's
//!   ALSA enumeration — which iterates cards — can only ever return `"default"`
//!   and `"oss"`. Here each `/dev/dsp*` is a device, named from the kernel's own
//!   `dev.pcm.N.%desc`, so a picker can say "Focusrite Scarlett Solo USB".
//! * **The reported ranges are honest.** The shim claims a buffer range of
//!   `1..=4294967295` and sample rates to 4.29 GHz. These are probed.
//!
//! # What it deliberately does not do
//!
//! It does not become the platform default. `default_host()` stays ALSA, so a
//! machine with no OSS device, or a user relying on the shim's software mixing,
//! is unaffected. This host is opt-in, exactly as JACK is.

use std::os::unix::ffi::OsStrExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::traits::{DeviceTrait, HostTrait, StreamTrait};
use crate::{
    BackendSpecificError, BufferSize, BuildStreamError, Data, DefaultStreamConfigError,
    DeviceNameError, DevicesError, InputCallbackInfo, OutputCallbackInfo, PauseStreamError,
    PlayStreamError, SampleFormat, SampleRate, StreamConfig, StreamError, StreamInstant,
    SupportedBufferSize, SupportedStreamConfig, SupportedStreamConfigRange,
    SupportedStreamConfigsError,
};

// ── The ioctl numbers, derived rather than copied ────────────────────────────
//
// FreeBSD's `_IOC` macro, in const fn form. Writing the constants out by hand
// is how a number ends up right on one architecture and wrong on the next; this
// is the same arithmetic <sys/ioccom.h> does.
const IOC_VOID: u64 = 0x2000_0000;
const IOC_OUT: u64 = 0x4000_0000;
const IOC_IN: u64 = 0x8000_0000;
const IOCPARM_MASK: u64 = 0x1fff;

const fn ioc(inout: u64, group: u64, num: u64, len: u64) -> libc::c_ulong {
    (inout | ((len & IOCPARM_MASK) << 16) | (group << 8) | num) as libc::c_ulong
}
const fn iow(group: u64, num: u64, len: u64) -> libc::c_ulong {
    ioc(IOC_IN, group, num, len)
}
const fn ior(group: u64, num: u64, len: u64) -> libc::c_ulong {
    ioc(IOC_OUT, group, num, len)
}
const fn iowr(group: u64, num: u64, len: u64) -> libc::c_ulong {
    ioc(IOC_IN | IOC_OUT, group, num, len)
}

const P: u64 = b'P' as u64;
const INT: u64 = std::mem::size_of::<libc::c_int>() as u64;

const SNDCTL_DSP_RESET: libc::c_ulong = ioc(IOC_VOID, P, 0, 0);
const SNDCTL_DSP_SPEED: libc::c_ulong = iowr(P, 2, INT);
const SNDCTL_DSP_SETFMT: libc::c_ulong = iowr(P, 5, INT);
const SNDCTL_DSP_CHANNELS: libc::c_ulong = iowr(P, 6, INT);
const SNDCTL_DSP_SETFRAGMENT: libc::c_ulong = iowr(P, 10, INT);
const SNDCTL_DSP_GETFMTS: libc::c_ulong = ior(P, 11, INT);
const SNDCTL_DSP_GETOSPACE: libc::c_ulong =
    ior(P, 12, std::mem::size_of::<AudioBufInfo>() as u64);
const SNDCTL_DSP_GETODELAY: libc::c_ulong = ior(P, 23, INT);

const AFMT_S16_LE: libc::c_int = 0x0000_0010;
const AFMT_S32_LE: libc::c_int = 0x0000_1000;

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct AudioBufInfo {
    fragments: libc::c_int,
    fragstotal: libc::c_int,
    fragsize: libc::c_int,
    bytes: libc::c_int,
}

/// Rates worth offering. OSS will resample anything, so the point of probing is
/// to find which ones the hardware takes *without* resampling — those are the
/// ones a low-latency host should be advertising.
const CANDIDATE_RATES: [u32; 6] = [44_100, 48_000, 88_200, 96_000, 176_400, 192_000];

/// How long the read/write loop waits before re-checking the stop flag when the
/// device has gone quiet. Only reached on a wedged device; the normal path
/// blocks in `read`/`write` and never gets here.
const POLL_MS: libc::c_int = 100;

fn last_os_error() -> BackendSpecificError {
    BackendSpecificError {
        description: std::io::Error::last_os_error().to_string(),
    }
}

/// One `ioctl` taking an `int` in and out.
fn ictl(fd: libc::c_int, req: libc::c_ulong, val: libc::c_int) -> Result<libc::c_int, BackendSpecificError> {
    let mut v = val;
    // SAFETY: `req` is an int-sized ioctl and `v` is an int.
    if unsafe { libc::ioctl(fd, req, &mut v as *mut libc::c_int) } < 0 {
        return Err(last_os_error());
    }
    Ok(v)
}

// ── Host ─────────────────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct Host;

impl Host {
    pub fn new() -> Result<Self, crate::HostUnavailable> {
        if Self::is_available() {
            Ok(Host)
        } else {
            Err(crate::HostUnavailable)
        }
    }
}

impl Default for Host {
    fn default() -> Self {
        Host
    }
}

impl HostTrait for Host {
    type Devices = Devices;
    type Device = Device;

    fn is_available() -> bool {
        // The presence of a DSP node, not a successful open: another program
        // may hold the device, and "somebody else is using the sound card" is
        // not the same as "this machine has no OSS".
        std::path::Path::new("/dev/dsp0").exists() || std::path::Path::new("/dev/dsp").exists()
    }

    fn devices(&self) -> Result<Self::Devices, DevicesError> {
        Devices::new()
    }

    fn default_input_device(&self) -> Option<Device> {
        Device::default_device()
    }

    fn default_output_device(&self) -> Option<Device> {
        Device::default_device()
    }
}

// ── Devices ──────────────────────────────────────────────────────────────────

pub struct Devices(std::vec::IntoIter<Device>);

impl Devices {
    pub fn new() -> Result<Self, DevicesError> {
        let mut found = Vec::new();
        // `/dev/dspN` where N is the pcm unit. Scanning a bounded range rather
        // than reading the directory keeps the order stable and the unit number
        // — which is what `dev.pcm.N.%desc` is keyed on — in hand.
        for unit in 0..32u32 {
            let path = PathBuf::from(format!("/dev/dsp{unit}"));
            if path.exists() {
                found.push(Device {
                    path,
                    name: unit_name(unit),
                });
            }
        }
        Ok(Devices(found.into_iter()))
    }
}

impl Iterator for Devices {
    type Item = Device;
    fn next(&mut self) -> Option<Device> {
        self.0.next()
    }
}

/// The kernel's own description of a pcm unit, e.g. "Focusrite Scarlett Solo
/// USB". Falling back to the node name rather than inventing something, because
/// a name a user cannot match to their hardware is worse than a bare number.
fn unit_name(unit: u32) -> String {
    sysctl_string(&format!("dev.pcm.{unit}.%desc")).unwrap_or_else(|| format!("pcm{unit}"))
}

/// An integer sysctl, e.g. `hw.snd.default_unit`.
fn sysctl_int(oid: &str) -> Option<u32> {
    let name = std::ffi::CString::new(oid).ok()?;
    let mut val: libc::c_int = 0;
    let mut len = std::mem::size_of::<libc::c_int>() as libc::size_t;
    // SAFETY: `val` is an int and `len` says so.
    if unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            (&mut val as *mut libc::c_int).cast(),
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    } != 0
    {
        return None;
    }
    u32::try_from(val).ok()
}

fn sysctl_string(oid: &str) -> Option<String> {
    let name = std::ffi::CString::new(oid).ok()?;
    let mut len: libc::size_t = 0;
    // SAFETY: a null buffer with a valid length pointer asks for the size.
    if unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            std::ptr::null_mut(),
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    } != 0
        || len == 0
    {
        return None;
    }
    let mut buf = vec![0u8; len];
    // SAFETY: buf is `len` bytes, which is what the kernel just asked for.
    if unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            buf.as_mut_ptr().cast(),
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    } != 0
    {
        return None;
    }
    buf.truncate(len);
    while buf.last() == Some(&0) {
        buf.pop();
    }
    String::from_utf8(buf).ok()
}

// ── Device ───────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Device {
    path: PathBuf,
    name: String,
}

impl Device {
    /// The unit the system considers default, which is what `/dev/dsp` follows
    /// and what changes when an interface is plugged in.
    fn default_device() -> Option<Device> {
        // **An integer sysctl, read as an integer.** `hw.snd.default_unit` is
        // an int, not a string, so `sysctlbyname` hands back four raw bytes;
        // reading it as text yields control characters, the parse fails, and
        // the fallback silently picks unit 0. That is how this returned the
        // built-in codec on a machine whose default was the USB interface.
        let unit = sysctl_int("hw.snd.default_unit").unwrap_or(0);
        let path = PathBuf::from(format!("/dev/dsp{unit}"));
        if path.exists() {
            return Some(Device {
                path,
                name: unit_name(unit),
            });
        }
        Devices::new().ok()?.next()
    }

    fn open(&self, flags: libc::c_int) -> Result<Fd, BackendSpecificError> {
        let c = std::ffi::CString::new(self.path.as_os_str().as_bytes())
            .map_err(|_| BackendSpecificError {
                description: "device path contains a NUL".to_owned(),
            })?;
        // SAFETY: `c` is a valid NUL-terminated path for the call's duration.
        let fd = unsafe { libc::open(c.as_ptr(), flags) };
        if fd < 0 {
            return Err(last_os_error());
        }
        Ok(Fd(fd))
    }

    /// Ask the hardware what it takes, rather than asserting a range.
    ///
    /// Every rate is offered to the driver and the answer read back: OSS returns
    /// the rate it actually set, so a mismatch means "not natively supported"
    /// and the rate is dropped. That is the difference between this list and
    /// the shim's, which claims everything up to 4.29 GHz.
    fn probe(&self, input: bool) -> Result<Vec<SupportedStreamConfigRange>, BackendSpecificError> {
        let fd = self.open(if input { libc::O_RDONLY } else { libc::O_WRONLY })?;
        let fmts = ictl(fd.0, SNDCTL_DSP_GETFMTS, 0).unwrap_or(AFMT_S16_LE);

        let mut formats = Vec::new();
        if fmts & AFMT_S32_LE != 0 {
            formats.push((AFMT_S32_LE, SampleFormat::I32));
        }
        if fmts & AFMT_S16_LE != 0 {
            formats.push((AFMT_S16_LE, SampleFormat::I16));
        }
        if formats.is_empty() {
            formats.push((AFMT_S16_LE, SampleFormat::I16));
        }

        // The buffer range the driver will actually grant, asked for rather
        // than assumed: the smallest fragment it accepts and the largest.
        let (min_frames, max_frames) = probe_buffer_range(&fd);

        let mut out = Vec::new();
        for (afmt, sample_format) in formats {
            let fd = self.open(if input { libc::O_RDONLY } else { libc::O_WRONLY })?;
            let _ = ictl(fd.0, SNDCTL_DSP_SETFMT, afmt);
            for channels in [2u16, 1] {
                let got = ictl(fd.0, SNDCTL_DSP_CHANNELS, channels as libc::c_int);
                if got.map(|c| c as u16) != Ok(channels) {
                    continue;
                }
                for rate in CANDIDATE_RATES {
                    let fd = self.open(if input { libc::O_RDONLY } else { libc::O_WRONLY })?;
                    let _ = ictl(fd.0, SNDCTL_DSP_SETFMT, afmt);
                    let _ = ictl(fd.0, SNDCTL_DSP_CHANNELS, channels as libc::c_int);
                    let Ok(got) = ictl(fd.0, SNDCTL_DSP_SPEED, rate as libc::c_int) else {
                        continue;
                    };
                    // OSS answers with what it set. Within a few Hz is the same
                    // rate; anything else is a resample we do not want to
                    // advertise as native.
                    if (got as i64 - rate as i64).abs() > 100 {
                        continue;
                    }
                    out.push(SupportedStreamConfigRange {
                        channels,
                        min_sample_rate: SampleRate(rate),
                        max_sample_rate: SampleRate(rate),
                        buffer_size: SupportedBufferSize::Range {
                            min: min_frames,
                            max: max_frames,
                        },
                        sample_format,
                    });
                }
            }
        }
        Ok(out)
    }

    fn default_config(&self, input: bool) -> Result<SupportedStreamConfig, DefaultStreamConfigError> {
        let mut configs = self
            .probe(input)
            .map_err(|err| DefaultStreamConfigError::BackendSpecific { err })?;
        if configs.is_empty() {
            return Err(DefaultStreamConfigError::StreamTypeNotSupported);
        }
        // 48 kHz stereo where it exists: the rate every interface of the last
        // twenty years runs natively, and the one FreeBSD's own vchans default
        // to, so it is the choice least likely to insert a converter.
        configs.sort_by_key(|c| {
            let rate_rank = match c.min_sample_rate.0 {
                48_000 => 0,
                44_100 => 1,
                r => 2 + (r / 1000) as u64,
            };
            (2u64.saturating_sub(c.channels as u64), rate_rank)
        });
        let best = configs.remove(0);
        Ok(best.with_sample_rate(best.min_sample_rate))
    }
}

/// The smallest and largest fragment the driver grants, in frames.
///
/// Asked as bytes for a stereo 16-bit stream, which is what the returned range
/// is then expressed in. Both ends are read back from `GETOSPACE` rather than
/// taken from what we asked for, because the two differ and the honest number
/// is the one the driver chose.
fn probe_buffer_range(fd: &Fd) -> (u32, u32) {
    let frame_bytes = 4u32; // 2 channels, 16 bit — the probing shape
    let mut min = 64u32;
    let mut max = 16384u32;
    // Smallest: ask for something absurdly small and see what comes back.
    if ictl(fd.0, SNDCTL_DSP_SETFRAGMENT, (4 << 16) | 6).is_ok() {
        let mut info = AudioBufInfo::default();
        // SAFETY: `info` is the struct this ioctl fills.
        if unsafe { libc::ioctl(fd.0, SNDCTL_DSP_GETOSPACE, &mut info as *mut AudioBufInfo) } >= 0
            && info.fragsize > 0
        {
            min = (info.fragsize as u32 / frame_bytes).max(1);
        }
    }
    if ictl(fd.0, SNDCTL_DSP_SETFRAGMENT, (4 << 16) | 16).is_ok() {
        let mut info = AudioBufInfo::default();
        // SAFETY: as above.
        if unsafe { libc::ioctl(fd.0, SNDCTL_DSP_GETOSPACE, &mut info as *mut AudioBufInfo) } >= 0
            && info.fragsize > 0
        {
            max = (info.fragsize as u32 / frame_bytes).max(min);
        }
    }
    (min, max)
}

impl DeviceTrait for Device {
    type SupportedInputConfigs = SupportedInputConfigs;
    type SupportedOutputConfigs = SupportedOutputConfigs;
    type Stream = Stream;

    fn name(&self) -> Result<String, DeviceNameError> {
        Ok(self.name.clone())
    }

    fn supported_input_configs(
        &self,
    ) -> Result<SupportedInputConfigs, SupportedStreamConfigsError> {
        Ok(SupportedInputConfigs(
            self.probe(true)
                .map_err(|err| SupportedStreamConfigsError::BackendSpecific { err })?
                .into_iter(),
        ))
    }

    fn supported_output_configs(
        &self,
    ) -> Result<SupportedOutputConfigs, SupportedStreamConfigsError> {
        Ok(SupportedOutputConfigs(
            self.probe(false)
                .map_err(|err| SupportedStreamConfigsError::BackendSpecific { err })?
                .into_iter(),
        ))
    }

    fn default_input_config(&self) -> Result<SupportedStreamConfig, DefaultStreamConfigError> {
        self.default_config(true)
    }

    fn default_output_config(&self) -> Result<SupportedStreamConfig, DefaultStreamConfigError> {
        self.default_config(false)
    }

    fn build_input_stream_raw<D, E>(
        &self,
        config: &StreamConfig,
        sample_format: SampleFormat,
        data_callback: D,
        error_callback: E,
        _timeout: Option<Duration>,
    ) -> Result<Self::Stream, BuildStreamError>
    where
        D: FnMut(&Data, &InputCallbackInfo) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
    {
        Stream::new(self, config, sample_format, true, data_callback, error_callback)
    }

    fn build_output_stream_raw<D, E>(
        &self,
        config: &StreamConfig,
        sample_format: SampleFormat,
        data_callback: D,
        error_callback: E,
        _timeout: Option<Duration>,
    ) -> Result<Self::Stream, BuildStreamError>
    where
        D: FnMut(&mut Data, &OutputCallbackInfo) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
    {
        Stream::new_output(self, config, sample_format, data_callback, error_callback)
    }
}

pub struct SupportedInputConfigs(std::vec::IntoIter<SupportedStreamConfigRange>);
pub struct SupportedOutputConfigs(std::vec::IntoIter<SupportedStreamConfigRange>);

impl Iterator for SupportedInputConfigs {
    type Item = SupportedStreamConfigRange;
    fn next(&mut self) -> Option<SupportedStreamConfigRange> {
        self.0.next()
    }
}
impl Iterator for SupportedOutputConfigs {
    type Item = SupportedStreamConfigRange;
    fn next(&mut self) -> Option<SupportedStreamConfigRange> {
        self.0.next()
    }
}

// ── Stream ───────────────────────────────────────────────────────────────────

/// Owns a descriptor and closes it exactly once.
struct Fd(libc::c_int);

impl Drop for Fd {
    fn drop(&mut self) {
        // SAFETY: this type owns the descriptor and runs once.
        unsafe { libc::close(self.0) };
    }
}

// The descriptor is moved into the I/O thread and touched nowhere else.
unsafe impl Send for Fd {}

pub struct Stream {
    playing: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

fn afmt_for(f: SampleFormat) -> Option<libc::c_int> {
    match f {
        SampleFormat::I16 => Some(AFMT_S16_LE),
        SampleFormat::I32 => Some(AFMT_S32_LE),
        _ => None,
    }
}

/// Set up a freshly opened device for this config.
///
/// **Order matters and is not arbitrary.** The fragment must be requested
/// BEFORE format, channels and rate: OSS sizes its buffers when the stream
/// parameters are set, and a `SETFRAGMENT` afterwards is silently ignored. That
/// single ordering rule is the difference between honouring `BufferSize::Fixed`
/// and quietly getting the driver's default, which is the whole reason this
/// host exists.
fn configure(
    fd: &Fd,
    config: &StreamConfig,
    sample_format: SampleFormat,
) -> Result<u32, BuildStreamError> {
    let afmt = afmt_for(sample_format).ok_or(BuildStreamError::StreamConfigNotSupported)?;
    let bytes_per_frame =
        sample_format.sample_size() as u32 * config.channels.max(1) as u32;

    if let BufferSize::Fixed(frames) = config.buffer_size {
        let want_bytes = (frames * bytes_per_frame).max(16);
        // `(max_fragments << 16) | log2(fragment_bytes)`.
        //
        // **The fragment COUNT is the latency multiplier, and it is the number
        // worth understanding here.** Total buffering is count x size, so with
        // a 64-frame request four fragments is 256 frames of depth and two is
        // 128 — measured app-to-app, 11.0 ms against 5.9 ms round trip. Two is
        // the shallowest a ring can be and still be double-buffered: the
        // driver drains one while we fill the other.
        //
        // Two is the default, and the reason is what happens to a caller who
        // does not change anything. cpal's usual caller asks for 256 frames;
        // at three fragments that is 31 ms round trip, WORSE than the
        // alsa-lib shim this host exists to beat, and the user would have had
        // to also know to lower their buffer to see any gain. At two it is
        // 20.7 ms — better than the shim before they touch anything — and
        // lowering the buffer then does what lowering a buffer should:
        //
        //     256 frames -> 20.7 ms     64 frames -> 8.1 ms
        //     128 frames -> 12.5 ms     32 frames -> 8.1 ms (hardware floor)
        //
        // `CPAL_OSS_FRAGMENTS` raises it for a machine that crackles, which is
        // the direction a fix should go: the default should be fast and the
        // escape hatch should be safe, not the other way round.
        let frags: u32 = std::env::var("CPAL_OSS_FRAGMENTS")
            .ok()
            .and_then(|v| v.parse().ok())
            .filter(|n| (2..=16).contains(n))
            .unwrap_or(2);
        let log2 = 31 - want_bytes.leading_zeros();
        let _ = ictl(
            fd.0,
            SNDCTL_DSP_SETFRAGMENT,
            ((frags << 16) | log2) as libc::c_int,
        );
    }

    ictl(fd.0, SNDCTL_DSP_SETFMT, afmt).map_err(|err| BuildStreamError::BackendSpecific { err })?;
    ictl(fd.0, SNDCTL_DSP_CHANNELS, config.channels as libc::c_int)
        .map_err(|err| BuildStreamError::BackendSpecific { err })?;
    let got = ictl(fd.0, SNDCTL_DSP_SPEED, config.sample_rate.0 as libc::c_int)
        .map_err(|err| BuildStreamError::BackendSpecific { err })?;
    if (got as i64 - config.sample_rate.0 as i64).abs() > 100 {
        return Err(BuildStreamError::StreamConfigNotSupported);
    }

    // What the driver actually granted, which is what the loop must use.
    let mut info = AudioBufInfo::default();
    // SAFETY: `info` is the struct this ioctl fills.
    let frames = if unsafe { libc::ioctl(fd.0, SNDCTL_DSP_GETOSPACE, &mut info as *mut AudioBufInfo) }
        >= 0
        && info.fragsize > 0
    {
        (info.fragsize as u32 / bytes_per_frame).max(1)
    } else {
        match config.buffer_size {
            BufferSize::Fixed(f) => f,
            BufferSize::Default => 1024,
        }
    };
    Ok(frames)
}

/// Nanoseconds since the stream started, as a `StreamInstant`.
fn instant(started: Instant) -> StreamInstant {
    let n = started.elapsed().as_nanos();
    StreamInstant::from_nanos_i128(n as i128).unwrap_or_else(|| StreamInstant::from_nanos(0))
}

impl Stream {
    fn new<D, E>(
        device: &Device,
        config: &StreamConfig,
        sample_format: SampleFormat,
        _input: bool,
        mut data_callback: D,
        mut error_callback: E,
    ) -> Result<Self, BuildStreamError>
    where
        D: FnMut(&Data, &InputCallbackInfo) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
    {
        let fd = device
            .open(libc::O_RDONLY)
            .map_err(|err| BuildStreamError::BackendSpecific { err })?;
        let frames = configure(&fd, config, sample_format)?;
        let channels = config.channels as usize;
        let rate = config.sample_rate.0;
        let bytes = frames as usize * channels * sample_format.sample_size();

        let playing = Arc::new(AtomicBool::new(false));
        let stop = Arc::new(AtomicBool::new(false));
        let (p, s) = (playing.clone(), stop.clone());

        let thread = std::thread::Builder::new()
            .name("cpal-oss-in".into())
            .spawn(move || {
                let fd = fd;
                let started = Instant::now();
                let mut buf = vec![0u8; bytes];
                while !s.load(Ordering::Relaxed) {
                    // SAFETY: buf is valid for its own length.
                    let n = unsafe { libc::read(fd.0, buf.as_mut_ptr().cast(), buf.len()) };
                    if n < 0 {
                        let e = std::io::Error::last_os_error();
                        if e.kind() == std::io::ErrorKind::Interrupted {
                            continue;
                        }
                        error_callback(StreamError::BackendSpecific { err: last_os_error() });
                        break;
                    }
                    if n == 0 {
                        error_callback(StreamError::DeviceNotAvailable);
                        break;
                    }
                    if !p.load(Ordering::Relaxed) {
                        continue;
                    }
                    let len = n as usize / sample_format.sample_size();
                    // SAFETY: `buf` holds `len` samples of `sample_format`,
                    // which is what the driver was configured to deliver.
                    let data = unsafe { Data::from_parts(buf.as_mut_ptr().cast(), len, sample_format) };
                    let callback = instant(started);
                    // The samples were captured one buffer BEFORE the read
                    // returned, which is the honest thing to report.
                    let ago = Duration::from_secs_f64(frames as f64 / rate as f64);
                    let capture = callback
                        .sub(ago)
                        .unwrap_or(callback);
                    data_callback(
                        &data,
                        &InputCallbackInfo {
                            timestamp: crate::InputStreamTimestamp { callback, capture },
                        },
                    );
                }
            })
            .map_err(|e| {
                BuildStreamError::BackendSpecific {
                    err: BackendSpecificError { description: e.to_string() },
                }
            })?;

        Ok(Stream {
            playing,
            stop,
            thread: Some(thread),
        })
    }

    fn new_output<D, E>(
        device: &Device,
        config: &StreamConfig,
        sample_format: SampleFormat,
        mut data_callback: D,
        mut error_callback: E,
    ) -> Result<Self, BuildStreamError>
    where
        D: FnMut(&mut Data, &OutputCallbackInfo) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
    {
        let fd = device
            .open(libc::O_WRONLY)
            .map_err(|err| BuildStreamError::BackendSpecific { err })?;
        let frames = configure(&fd, config, sample_format)?;
        let channels = config.channels as usize;
        let rate = config.sample_rate.0;
        let bytes = frames as usize * channels * sample_format.sample_size();

        let playing = Arc::new(AtomicBool::new(false));
        let stop = Arc::new(AtomicBool::new(false));
        let (p, s) = (playing.clone(), stop.clone());

        let thread = std::thread::Builder::new()
            .name("cpal-oss-out".into())
            .spawn(move || {
                let fd = fd;
                let started = Instant::now();
                let mut buf = vec![0u8; bytes];
                let len = bytes / sample_format.sample_size();
                while !s.load(Ordering::Relaxed) {
                    if p.load(Ordering::Relaxed) {
                        // SAFETY: `buf` is `len` samples of `sample_format`.
                        let mut data =
                            unsafe { Data::from_parts(buf.as_mut_ptr().cast(), len, sample_format) };
                        let callback = instant(started);
                        // How far ahead the write will actually be heard, from
                        // the driver's own queue depth rather than an estimate.
                        let queued = ictl(fd.0, SNDCTL_DSP_GETODELAY, 0).unwrap_or(0).max(0) as f64;
                        let ahead = Duration::from_secs_f64(
                            queued / (sample_format.sample_size() * channels) as f64 / rate as f64,
                        );
                        let playback = callback.add(ahead).unwrap_or(callback);
                        data_callback(
                            &mut data,
                            &OutputCallbackInfo {
                                timestamp: crate::OutputStreamTimestamp { callback, playback },
                            },
                        );
                    } else {
                        // Paused still has to feed the device: a DSP starved of
                        // writes underruns, and an underrun is audible in a way
                        // silence is not.
                        buf.iter_mut().for_each(|b| *b = 0);
                    }
                    // SAFETY: buf is valid for its own length.
                    let n = unsafe { libc::write(fd.0, buf.as_ptr().cast(), buf.len()) };
                    if n < 0 {
                        let e = std::io::Error::last_os_error();
                        if e.kind() == std::io::ErrorKind::Interrupted {
                            continue;
                        }
                        error_callback(StreamError::BackendSpecific { err: last_os_error() });
                        break;
                    }
                }
                // Let whatever is queued finish rather than cutting it off.
                // SAFETY: the fd is still ours here.
                unsafe { libc::ioctl(fd.0, SNDCTL_DSP_RESET) };
            })
            .map_err(|e| {
                BuildStreamError::BackendSpecific {
                    err: BackendSpecificError { description: e.to_string() },
                }
            })?;

        Ok(Stream {
            playing,
            stop,
            thread: Some(thread),
        })
    }
}

impl StreamTrait for Stream {
    fn play(&self) -> Result<(), PlayStreamError> {
        self.playing.store(true, Ordering::Relaxed);
        Ok(())
    }

    fn pause(&self) -> Result<(), PauseStreamError> {
        self.playing.store(false, Ordering::Relaxed);
        Ok(())
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        // Joined rather than detached: the thread owns the callback, and a
        // callback outliving the stream that created it is how a use-after-free
        // gets into an audio path.
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

#[allow(dead_code)]
const _: () = {
    // The loop wakes on the device, not on a timer; POLL_MS is kept for the
    // wedged-device path and named so it is not mistaken for dead code.
    let _ = POLL_MS;
};
