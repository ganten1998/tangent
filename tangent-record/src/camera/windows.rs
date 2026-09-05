//! The Windows camera backend, through the bundled ffmpeg's DirectShow input.
//!
//! Windows has no first-party capture API as small as V4L2 or AVFoundation:
//! `IMFSourceReader` is a pile of hand-managed COM on a worker thread, and it
//! is exactly the kind of unsafe, hard-to-cross-compile code that a Mac build
//! box cannot exercise. But this app already ships a statically-linked
//! `tangent-ffmpeg.exe` beside the binary for video encode, and its build
//! carries the `dshow` input device — so a camera is one subprocess away, with
//! no new native bindings and nothing extra to cross-compile.
//!
//! # Shape
//!
//! * [`cameras`] runs `ffmpeg -list_devices` and parses the friendly name and
//!   the stable `@device_pnp_…` alternative name off stderr; the alternative
//!   name is the UID (it survives a rename and a language change), the friendly
//!   name is for display. [`device_formats`] then asks `-list_options` for the
//!   sizes each device offers.
//! * [`open`] spawns one ffmpeg that decodes the chosen mode to **rgba** and
//!   writes raw frames to stdout; a reader thread hands each one to the
//!   [`FrameSlot`]. ffmpeg does the mjpeg decode and the pixel conversion in
//!   its own process, so the frame thread here only reads and stamps.
//!
//! # The mjpeg / raw split
//!
//! A webcam's highest sizes are usually mjpeg-only (720p on the test camera
//! exists as mjpeg and nowhere as raw yuyv422, which tops out at 480p). So a
//! request for a size only offered as mjpeg must pass `-vcodec mjpeg`, or dshow
//! silently falls back to a raw mode and caps the resolution. The parsed
//! options carry which codec each size needs, and [`open`] asks for the right
//! one.

use std::io::Read;
use std::process::{Child, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;

use super::{
    pick_format, CameraError, CameraInfo, CameraStats, Format, FormatWish, FrameSlot,
    PermissionStatus, VideoSource, WindowRegion, BYTES_PER_PIXEL,
};
use crate::audio::{DeviceState, Timebase};

/// Windows has a camera privacy switch, but dshow does not expose a queryable
/// permission the way AVFoundation does: with it off, enumeration and open
/// simply fail, which surfaces as an ordinary error. So there is no permission
/// model to report here.
pub(super) fn permission_status() -> PermissionStatus {
    PermissionStatus::NotApplicable
}

/// Which encoding a size is offered under. A size only listed as mjpeg must be
/// asked for as mjpeg or dshow drops to a raw mode and a lower resolution.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Codec {
    Mjpeg,
    Raw,
}

/// A parsed capture mode: its format and the codec dshow offers it under.
struct Mode {
    format: Format,
    codec: Codec,
}

/// Run the bundled ffmpeg with `args` and return its stderr, whatever the exit
/// status — the device and option listings are PRINTED to stderr and ffmpeg
/// exits non-zero on the dummy input, which is expected.
fn ffmpeg_stderr(args: &[&str]) -> Result<String, CameraError> {
    let program = crate::encode::ffmpeg::program();
    let out = crate::proc::command(&program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| CameraError::Enumeration(format!("could not run ffmpeg: {e}")))?;
    Ok(String::from_utf8_lossy(&out.stderr).into_owned())
}

/// The quoted string on a dshow listing line, e.g. `"HP TrueVision HD Camera"`.
fn quoted(line: &str) -> Option<String> {
    let a = line.find('"')?;
    let b = line[a + 1..].find('"')? + a + 1;
    Some(line[a + 1..b].to_owned())
}

pub(super) fn cameras() -> Result<Vec<CameraInfo>, CameraError> {
    let text = ffmpeg_stderr(&["-hide_banner", "-list_devices", "true", "-f", "dshow", "-i", "dummy"])?;
    let mut list: Vec<CameraInfo> = Vec::new();
    // ffmpeg prints, per device, a `"Friendly Name" (video)` line followed by
    // an `Alternative name "@device_pnp_…"` line. The friendly name is display;
    // the alternative name is the stable UID. Audio devices carry `(audio)`
    // and are skipped.
    let mut pending: Option<String> = None; // a friendly video name awaiting its alt name
    for line in text.lines() {
        if line.contains("(video)") {
            if let Some(prev) = pending.take() {
                // A video line with no alternative name before the next: the
                // friendly name is the only handle, so it is the UID too.
                push_camera(&mut list, prev.clone(), prev);
            }
            pending = quoted(line);
        } else if line.contains("Alternative name") {
            if let Some(friendly) = pending.take() {
                let uid = quoted(line).unwrap_or_else(|| friendly.clone());
                push_camera(&mut list, uid, friendly);
            }
        } else if line.contains("(audio)") {
            pending = None;
        }
    }
    if let Some(friendly) = pending.take() {
        push_camera(&mut list, friendly.clone(), friendly);
    }
    Ok(list)
}

fn push_camera(list: &mut Vec<CameraInfo>, uid: String, name: String) {
    if list.iter().any(|c| c.uid == uid) {
        return;
    }
    let formats: Vec<Format> = device_formats(&uid, &name)
        .into_iter()
        .map(|m| m.format)
        .collect();
    let is_default = list.is_empty();
    list.push(CameraInfo { uid, name, is_default, formats });
}

/// The capture modes a device offers, parsed from `ffmpeg -list_options`.
///
/// Deduplicated by `(width, height, fps)` keeping the mjpeg entry when a size
/// appears under both, because mjpeg is what reaches the higher resolutions.
fn device_formats(uid: &str, name: &str) -> Vec<Mode> {
    // Prefer the stable UID for the query; fall back to the friendly name.
    let by = |dev: &str| {
        ffmpeg_stderr(&[
            "-hide_banner",
            "-f",
            "dshow",
            "-list_options",
            "true",
            "-i",
            &format!("video={dev}"),
        ])
        .unwrap_or_default()
    };
    let mut text = by(uid);
    if !text.contains("vcodec") && !text.contains("pixel_format") {
        text = by(name);
    }
    let mut modes: Vec<Mode> = Vec::new();
    for line in text.lines() {
        let codec = if line.contains("vcodec=mjpeg") {
            Codec::Mjpeg
        } else if line.contains("pixel_format=") {
            Codec::Raw
        } else {
            continue;
        };
        let Some((w, h)) = parse_size(line) else { continue };
        let fps = parse_fps(line).unwrap_or(30.0);
        let format = Format { width: w, height: h, fps };
        // A size under both codecs: keep the mjpeg row (it reaches higher).
        if let Some(existing) = modes
            .iter_mut()
            .find(|m| m.format.width == w && m.format.height == h && (m.format.fps - fps).abs() < 0.5)
        {
            if codec == Codec::Mjpeg {
                existing.codec = Codec::Mjpeg;
            }
            continue;
        }
        modes.push(Mode { format, codec });
    }
    modes
}

/// `…min s=640x480 max s=640x480…` → `(640, 480)`. Takes the first `NxM`.
fn parse_size(line: &str) -> Option<(u32, u32)> {
    let at = line.find("s=")?;
    let rest = &line[at + 2..];
    let end = rest
        .find(|c: char| !(c.is_ascii_digit() || c == 'x'))
        .unwrap_or(rest.len());
    let dims = &rest[..end];
    let (w, h) = dims.split_once('x')?;
    Some((w.parse().ok()?, h.parse().ok()?))
}

/// `…fps=30…` → `30.0`. The test camera reports min=max, so either reads the
/// same; the last `fps=` on the line is the max.
fn parse_fps(line: &str) -> Option<f64> {
    let at = line.rfind("fps=")?;
    let rest = &line[at + 4..];
    let end = rest
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(rest.len());
    rest[..end].parse().ok()
}

pub(super) fn open(
    uid: &str,
    wish: &FormatWish,
    timebase: Timebase,
    slot: Arc<FrameSlot>,
    stats: Arc<CameraStats>,
) -> Result<Box<dyn VideoSource>, CameraError> {
    let name = cameras()
        .ok()
        .and_then(|list| list.into_iter().find(|c| c.uid == uid))
        .map(|c| c.name)
        .unwrap_or_else(|| uid.to_owned());

    let modes = device_formats(uid, &name);
    // A camera we cannot read options from still opens: offer the common HD
    // and SD modes and let dshow negotiate, rather than refusing outright.
    let fallback = [
        Mode { format: Format { width: 1280, height: 720, fps: 30.0 }, codec: Codec::Mjpeg },
        Mode { format: Format { width: 640, height: 480, fps: 30.0 }, codec: Codec::Raw },
    ];
    let modes: &[Mode] = if modes.is_empty() { &fallback } else { &modes };

    let formats: Vec<Format> = modes.iter().map(|m| m.format).collect();
    let Some(pick) = pick_format(&formats, wish) else {
        return Err(CameraError::NoUsableFormat { camera: name, offered: formats.len() });
    };
    let chosen = &modes[pick];
    let (w, h, fps) = (chosen.format.width, chosen.format.height, chosen.format.fps);

    // dshow takes the friendly name OR the `@device_pnp_…` alternative name.
    // The alternative name (our UID) avoids quoting a name with punctuation.
    let device = format!("video={uid}");
    let size = format!("{w}x{h}");
    let rate = format!("{:.0}", fps.max(1.0));
    let mut args: Vec<String> = vec![
        "-hide_banner".into(),
        "-loglevel".into(),
        "error".into(),
        "-nostdin".into(),
        "-f".into(),
        "dshow".into(),
    ];
    // The codec ask, before `-i`, so dshow selects the matching source mode.
    match chosen.codec {
        Codec::Mjpeg => args.extend(["-vcodec".into(), "mjpeg".into()]),
        Codec::Raw => args.extend(["-pixel_format".into(), "yuyv422".into()]),
    }
    args.extend([
        "-video_size".into(),
        size,
        "-framerate".into(),
        rate,
        "-i".into(),
        device,
        // Out: tightly-packed rgba, the frame slot's format, straight to stdout.
        "-pix_fmt".into(),
        "rgba".into(),
        "-f".into(),
        "rawvideo".into(),
        "-".into(),
    ]);

    let program = crate::encode::ffmpeg::program();
    let mut child = crate::proc::command(&program)
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| CameraError::Open(format!("could not start ffmpeg: {e}")))?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| CameraError::Open("ffmpeg gave no output pipe".to_owned()))?;

    let format = Format { width: w, height: h, fps };
    let stop = Arc::new(AtomicBool::new(false));
    let state = Arc::new(AtomicU8::new(state_as_u8(DeviceState::Running)));
    let thread_stop = Arc::clone(&stop);
    let thread_state = Arc::clone(&state);
    let join = std::thread::Builder::new()
        .name("ivory-camera-win".to_owned())
        .spawn(move || {
            capture_loop(stdout, w, h, timebase, &slot, &stats, &thread_stop, &thread_state);
        })
        .map_err(|e| CameraError::Open(format!("could not start the capture thread: {e}")))?;

    Ok(Box::new(WindowsCamera { format, child, stop, state, join: Some(join) }))
}

/// Read rgba frames off ffmpeg's stdout, stamp, and publish, until stopped or
/// the stream ends.
#[allow(clippy::too_many_arguments)]
fn capture_loop(
    mut stdout: std::process::ChildStdout,
    width: u32,
    height: u32,
    timebase: Timebase,
    slot: &FrameSlot,
    stats: &CameraStats,
    stop: &AtomicBool,
    state: &AtomicU8,
) {
    let frame_bytes = width as usize * height as usize * BYTES_PER_PIXEL;
    if frame_bytes == 0 {
        state.store(state_as_u8(DeviceState::Errored), Ordering::Relaxed);
        return;
    }
    // One reused buffer: a 720p rgba frame is 3.7 MB, and allocating that
    // thirty times a second would churn the heap for no reason.
    let mut buf = vec![0u8; frame_bytes];
    loop {
        if stop.load(Ordering::Relaxed) {
            return;
        }
        // Fill exactly one frame. `read_exact` would be tidier but turns a
        // clean end-of-stream into an error; this tells the two apart.
        let mut filled = 0;
        while filled < frame_bytes {
            match stdout.read(&mut buf[filled..]) {
                Ok(0) => {
                    // ffmpeg closed the pipe. A clean stop is not an error; an
                    // unexpected end (camera unplugged, privacy switched off,
                    // device grabbed by another app) is.
                    let s = if stop.load(Ordering::Relaxed) {
                        DeviceState::Running
                    } else {
                        DeviceState::Lost
                    };
                    state.store(state_as_u8(s), Ordering::Relaxed);
                    return;
                }
                Ok(n) => filled += n,
                Err(ref e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(_) => {
                    state.store(state_as_u8(DeviceState::Errored), Ordering::Relaxed);
                    return;
                }
            }
        }
        // Stamped when the frame is fully in hand — the closest this backend
        // gets to the arrival instant. The a/v sync contract lives here.
        let host_ns = timebase.now();
        let mut pixels = slot.take_spare();
        pixels.clear();
        pixels.extend_from_slice(&buf);
        slot.publish(super::Frame {
            width,
            height,
            stride: width as usize * BYTES_PER_PIXEL,
            pixels,
            host_ns,
            // ffmpeg does not hand back the device timestamp through a raw
            // pipe; the host clock is the only stamp there is.
            pts_ns: None,
        });
        stats.note_delivered();
    }
}

struct WindowsCamera {
    format: Format,
    child: Child,
    stop: Arc<AtomicBool>,
    state: Arc<AtomicU8>,
    join: Option<JoinHandle<()>>,
}

impl VideoSource for WindowsCamera {
    fn format(&self) -> Format {
        self.format
    }

    fn state(&self) -> DeviceState {
        state_from_u8(self.state.load(Ordering::Relaxed))
    }
}

impl Drop for WindowsCamera {
    fn drop(&mut self) {
        // Kill ffmpeg FIRST: that closes the pipe, so the reader's blocking
        // `read` returns 0 and the thread falls out of its loop. Killing the
        // process is also what releases the camera, so an immediate reopen
        // does not fail with the device busy.
        self.stop.store(true, Ordering::Relaxed);
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

fn state_as_u8(state: DeviceState) -> u8 {
    match state {
        DeviceState::Running => 0,
        DeviceState::Lost => 1,
        DeviceState::Errored => 2,
    }
}

fn state_from_u8(v: u8) -> DeviceState {
    match v {
        1 => DeviceState::Lost,
        2 => DeviceState::Errored,
        _ => DeviceState::Running,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoted_extracts_a_device_name() {
        assert_eq!(
            quoted(r#"[in#0] "HP TrueVision HD Camera" (video)"#).as_deref(),
            Some("HP TrueVision HD Camera")
        );
        assert_eq!(quoted("no quotes here"), None);
    }

    #[test]
    fn size_and_fps_parse_off_an_options_line() {
        let line = "  vcodec=mjpeg  min s=1280x720 fps=30 max s=1280x720 fps=30";
        assert_eq!(parse_size(line), Some((1280, 720)));
        assert_eq!(parse_fps(line), Some(30.0));
        let raw = "  pixel_format=yuyv422  min s=640x480 fps=30 max s=640x480 fps=30";
        assert_eq!(parse_size(raw), Some((640, 480)));
    }

    /// **The 720p-only-as-mjpeg case the test camera showed.** When a size is
    /// offered under both codecs, mjpeg wins; a raw-only size stays raw.
    #[test]
    fn a_size_offered_as_both_keeps_mjpeg() {
        // Two lines for 640x480 (raw then mjpeg) and one mjpeg-only 1280x720.
        let text = "\
pixel_format=yuyv422 min s=640x480 fps=30 max s=640x480 fps=30
vcodec=mjpeg min s=640x480 fps=30 max s=640x480 fps=30
vcodec=mjpeg min s=1280x720 fps=30 max s=1280x720 fps=30";
        let mut modes: Vec<Mode> = Vec::new();
        for line in text.lines() {
            let codec = if line.contains("vcodec=mjpeg") {
                Codec::Mjpeg
            } else {
                Codec::Raw
            };
            let (w, h) = parse_size(line).unwrap();
            let fps = parse_fps(line).unwrap();
            let format = Format { width: w, height: h, fps };
            if let Some(e) = modes.iter_mut().find(|m| m.format.width == w && m.format.height == h) {
                if codec == Codec::Mjpeg {
                    e.codec = Codec::Mjpeg;
                }
                continue;
            }
            modes.push(Mode { format, codec });
        }
        assert_eq!(modes.len(), 2, "640x480 should dedup to one row");
        let sd = modes.iter().find(|m| m.format.height == 480).unwrap();
        assert!(sd.codec == Codec::Mjpeg, "the both-codecs size did not prefer mjpeg");
        let hd = modes.iter().find(|m| m.format.height == 720).unwrap();
        assert!(hd.codec == Codec::Mjpeg);
    }
}

/// The window capture, which this platform does not have yet.
///
/// [`CameraError::Unsupported`] and not a panic, for the reason `open` gives:
/// the UI prints it as a sentence and the take goes on with the composited
/// video it always had.
pub(super) fn open_window(
    _region: WindowRegion,
    _fps: f64,
    _timebase: Timebase,
    _slot: Arc<FrameSlot>,
    _stats: Arc<CameraStats>,
) -> Result<Box<dyn VideoSource>, CameraError> {
    Err(CameraError::Unsupported)
}
