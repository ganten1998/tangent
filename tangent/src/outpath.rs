//! **Who ELSE is in the output path.**
//!
//! The panel can already say how many milliseconds the two buffers hold. It
//! could not say the thing that actually costs the time: that the "device" it
//! is playing into is not hardware at all, and that another application is
//! standing between the mix and the speakers.
//!
//! That gap cost a week. A per-app audio tool held a rule matching this app,
//! routed its output through a virtual device, and added enough delay to be
//! plainly audible — at unity gain, so there was nothing to HEAR except the
//! lag. Every measurement the app can take of itself came back clean, because
//! with the device to itself there was nothing to find. The missing sentence
//! was never a number. It was *something else is downstream of you*.
//!
//! So this asks the OS, which knows:
//!
//! * **`kAudioDevicePropertyTransportType`.** `virt` means the device is
//!   published by software. Documented, vendor-neutral, and true of every
//!   interposer rather than of one product — SoundSource, Loopback, Audio
//!   Hijack, BlackHole, a conferencing tool's noise suppressor. Naming a
//!   vendor would date; naming the transport does not.
//! * **`kAudioObjectPropertyManufacturer`**, so the line can say WHOSE, which
//!   is the difference between a warning and an instruction.
//! * **`kAudioDevicePropertyLatency` and `kAudioDevicePropertySafetyOffset`.**
//!   cpal reads NEITHER on any platform — `tangent-record/src/audio.rs` says
//!   so where the round trip is computed — so these frames are real delay the
//!   estimate has never included, whatever kind of device is open.
//!
//! **What this does NOT prove, measured on the owner's machine 2026-08-29.**
//!
//! Enumerating from a test binary, every output device was real hardware:
//!
//! ```text
//! Scarlett 18i20 USB     USB          Focusrite    28 frames
//! LG ULTRAGEAR+          HDMI         GSM         214 frames
//! MacBook Pro Speakers   built-in     Apple       108 frames
//! ```
//!
//! No virtual device was the default output *while a per-app capture rule for
//! this app was armed*. So the transport check does NOT, on that evidence,
//! catch a per-app interposer — Rogue Amoeba's ARK substitutes the device
//! list INSIDE the process it is capturing, and a test binary is not the
//! captured process. Whether the running app sees a virtual device is
//! unverified and needs eyes on the panel.
//!
//! Which is fine, because the transport check is not the only thing here and
//! is not the part that pays for itself. **The frame counts above are, and
//! they are already wrong on the panel today**: 214 frames is 4.5 ms of HDMI
//! output that the round-trip estimate has never included and cpal has never
//! read. That number is right whoever else is or is not in the path.
//!
//! **This changes nothing and repairs nothing.** It is a readout. The app does
//! not reach into another vendor's settings to switch itself out of the path:
//! that config is undocumented, versioned, cached in the other app's memory
//! and rewritten when it quits, and a DAW editing it behind the user's back
//! would be a worse bug than the one it fixed. Telling the truth on the panel
//! is the whole remedy.
//!
//! # Safety
//!
//! `AudioObjectGetPropertyData` is a blocking HAL call. It runs HERE on the UI
//! thread, once, when the setup panel opens — never from an audio callback,
//! where a HAL call is a deadlock (`coreaudio-rs`'s input proc doing exactly
//! that is a wedge this project has already paid for). Every query is
//! fallible and every failure path leaves the field at its unknown default, so
//! a machine whose HAL is wedged gets a panel with one line missing rather
//! than a hang.

use tangent_ui::ports::OutputPath;

/// Ask the OS what the monitor is really playing into.
///
/// `device` is the name the output was opened by, exactly as
/// [`crate::instrument::Engine::start`] took it — `None` for the system
/// default, which is what the app uses unless somebody has chosen otherwise.
#[cfg(not(target_os = "macos"))]
pub fn probe(_device: Option<&str>) -> OutputPath {
    // Windows and Linux have the same class of problem — Voicemeeter, a
    // PipeWire graph, WASAPI shared mode — and neither is wired up yet. An
    // `OutputPath` that says it knows nothing prints nothing, which is the
    // correct behaviour for a build that cannot check rather than a claim
    // that the path is clear.
    OutputPath::default()
}

/// **The default output's nominal rate, without building an AudioUnit.**
///
/// Off macOS there is no HAL to ask and the caller keeps its own road; see the
/// macOS twin for why that road was worth leaving. Which is also why nothing
/// calls this one: the rate follower asks the HAL only where there is a HAL,
/// and the stub is here so the module has one shape on every platform.
#[cfg(not(target_os = "macos"))]
#[allow(dead_code)]
pub fn nominal_rate(_device: Option<&str>) -> Option<u32> {
    None
}

/// The stand-in on every platform without a CoreAudio HAL: it watches
/// nothing and says so, so `count()` stays `None` and the merge that reads it
/// is byte-for-byte the arithmetic it was before this type existed.
#[cfg(not(target_os = "macos"))]
#[derive(Default)]
pub struct OverloadWatch;

#[cfg(not(target_os = "macos"))]
impl OverloadWatch {
    pub fn watch(_device: Option<&str>) -> Self {
        Self
    }
    pub fn count(&self) -> Option<u64> {
        None
    }
}

// Minimal links no audio backend, so nothing in that build asks the output
// path anything: the re-export is read only where the recorder is compiled.
#[cfg_attr(not(feature = "recorder"), allow(unused_imports))]
#[cfg(target_os = "macos")]
pub use mac::{nominal_rate, OverloadWatch};

#[cfg_attr(not(feature = "recorder"), allow(unused_imports))]
#[cfg(target_os = "macos")]
pub use mac::probe;

/// See `mac::device_id`. Off macOS there is no HAL to ask, so nothing can
/// be resolved and `rtwork` finds no workgroup, which is the same answer.
#[cfg(target_os = "macos")]
pub(crate) use mac::device_id;

#[cfg(not(target_os = "macos"))]
#[allow(dead_code)]
pub(crate) fn device_id(_device: Option<&str>) -> Option<u32> {
    None
}

#[cfg(target_os = "macos")]
mod mac {
    use super::OutputPath;
    use std::ffi::c_void;

    type OSStatus = i32;
    type AudioObjectID = u32;

    /// A four-character code, as every CoreAudio selector is written in the
    /// headers. `b"tran"` here is `'tran'` there.
    const fn fourcc(s: &[u8; 4]) -> u32 {
        u32::from_be_bytes(*s)
    }

    const SYSTEM_OBJECT: AudioObjectID = 1;

    const DEVICES: u32 = fourcc(b"dev#");
    const DEFAULT_OUTPUT: u32 = fourcc(b"dOut");
    const SCOPE_GLOBAL: u32 = fourcc(b"glob");
    const SCOPE_OUTPUT: u32 = fourcc(b"outp");
    const ELEMENT_MAIN: u32 = 0;

    const NAME: u32 = fourcc(b"lnam");
    const MANUFACTURER: u32 = fourcc(b"lmak");
    const TRANSPORT: u32 = fourcc(b"tran");
    const LATENCY: u32 = fourcc(b"ltnc");
    const SAFETY_OFFSET: u32 = fourcc(b"saft");
    const STREAMS: u32 = fourcc(b"stm#");

    /// `kAudioDevicePropertyNominalSampleRate`. A plain `Float64` read on the
    /// device object — the same number cpal returns from
    /// `default_output_config`, at none of the price. See [`nominal_rate`].
    const NOMINAL_RATE: u32 = fourcc(b"nsrt");
    /// `kAudioDeviceProcessorOverload`. Not a value: the HAL POSTS this when
    /// an IO cycle on the device missed its deadline. See [`OverloadWatch`].
    const PROCESSOR_OVERLOAD: u32 = fourcc(b"over");

    const T_VIRTUAL: u32 = fourcc(b"virt");
    const T_AGGREGATE: u32 = fourcc(b"grup");

    /// `kCFStringEncodingUTF8`.
    const UTF8: u32 = 0x0800_0100;

    #[repr(C)]
    struct PropertyAddress {
        selector: u32,
        scope: u32,
        element: u32,
    }

    impl PropertyAddress {
        const fn global(selector: u32) -> Self {
            Self { selector, scope: SCOPE_GLOBAL, element: ELEMENT_MAIN }
        }
        const fn output(selector: u32) -> Self {
            Self { selector, scope: SCOPE_OUTPUT, element: ELEMENT_MAIN }
        }
    }

    #[link(name = "CoreAudio", kind = "framework")]
    extern "C" {
        fn AudioObjectGetPropertyDataSize(
            id: AudioObjectID,
            address: *const PropertyAddress,
            qualifier_size: u32,
            qualifier: *const c_void,
            out_size: *mut u32,
        ) -> OSStatus;
        fn AudioObjectGetPropertyData(
            id: AudioObjectID,
            address: *const PropertyAddress,
            qualifier_size: u32,
            qualifier: *const c_void,
            io_size: *mut u32,
            out_data: *mut c_void,
        ) -> OSStatus;
        fn AudioObjectAddPropertyListener(
            id: AudioObjectID,
            address: *const PropertyAddress,
            listener: ListenerProc,
            client_data: *mut c_void,
        ) -> OSStatus;
        fn AudioObjectRemovePropertyListener(
            id: AudioObjectID,
            address: *const PropertyAddress,
            listener: ListenerProc,
            client_data: *mut c_void,
        ) -> OSStatus;
    }

    /// `AudioObjectPropertyListenerProc`, exactly as the header declares it.
    type ListenerProc = unsafe extern "C" fn(
        AudioObjectID,
        u32,
        *const PropertyAddress,
        *mut c_void,
    ) -> OSStatus;

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFStringGetCString(
            s: *const c_void,
            buffer: *mut u8,
            size: isize,
            encoding: u32,
        ) -> u8;
        fn CFRelease(cf: *const c_void);
    }

    /// One fixed-size property, or `None` if the HAL will not answer.
    ///
    /// Every read goes through here so that "the call failed" and "the device
    /// does not have this property" land in the same place: at an unknown
    /// field, which prints nothing.
    fn get<T: Copy + Default>(id: AudioObjectID, address: &PropertyAddress) -> Option<T> {
        let mut value = T::default();
        let mut size = size_of::<T>() as u32;
        // SAFETY: `size` describes `value` exactly, the address is a live
        // borrow for the length of the call, and the HAL writes at most
        // `size` bytes. A non-zero status leaves `value` untouched and we
        // discard it.
        let status = unsafe {
            AudioObjectGetPropertyData(
                id,
                address,
                0,
                std::ptr::null(),
                &mut size,
                (&mut value as *mut T).cast(),
            )
        };
        (status == 0 && size as usize == size_of::<T>()).then_some(value)
    }

    /// A `CFStringRef` property, copied out as a `String`.
    fn get_string(id: AudioObjectID, address: &PropertyAddress) -> Option<String> {
        let cf: *const c_void = get(id, address)?;
        if cf.is_null() {
            return None;
        }
        // A device name or a manufacturer is a short label. 512 bytes is far
        // past anything real, and a name that somehow did not fit is dropped
        // rather than truncated into a half-word.
        let mut buf = [0u8; 512];
        // SAFETY: `cf` is a CFStringRef the HAL just handed us and which we
        // own (a `Get`-rule property returns +1 here). The buffer's length is
        // passed honestly and `CFStringGetCString` NUL-terminates within it.
        let ok = unsafe { CFStringGetCString(cf, buf.as_mut_ptr(), buf.len() as isize, UTF8) };
        // SAFETY: released exactly once, and `cf` is not used again.
        unsafe { CFRelease(cf) };
        if ok == 0 {
            return None;
        }
        let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
        std::str::from_utf8(&buf[..end]).ok().map(str::to_owned)
    }

    /// Every audio object the system currently publishes.
    fn all_devices() -> Vec<AudioObjectID> {
        let address = PropertyAddress::global(DEVICES);
        let mut bytes: u32 = 0;
        // SAFETY: an out-parameter of exactly the width the API documents.
        let status = unsafe {
            AudioObjectGetPropertyDataSize(
                SYSTEM_OBJECT,
                &address,
                0,
                std::ptr::null(),
                &mut bytes,
            )
        };
        if status != 0 || bytes == 0 {
            return Vec::new();
        }
        let count = bytes as usize / size_of::<AudioObjectID>();
        let mut ids = vec![0 as AudioObjectID; count];
        let mut io = bytes;
        // SAFETY: `ids` holds exactly `io` bytes and the HAL writes at most
        // that many. On failure the vector is dropped unread.
        let status = unsafe {
            AudioObjectGetPropertyData(
                SYSTEM_OBJECT,
                &address,
                0,
                std::ptr::null(),
                &mut io,
                ids.as_mut_ptr().cast(),
            )
        };
        if status != 0 {
            return Vec::new();
        }
        ids.truncate(io as usize / size_of::<AudioObjectID>());
        ids
    }

    /// Whether the device has anything to play OUT of.
    ///
    /// The device list is one list: microphones and interfaces are on it
    /// together, and an input-only box can carry the same name as the output
    /// somebody meant. Asking for the size of its output streams is the
    /// cheapest way to tell them apart, and it is how the name match avoids
    /// answering about the wrong half of a duplex interface.
    fn has_output(id: AudioObjectID) -> bool {
        let address = PropertyAddress::output(STREAMS);
        let mut bytes: u32 = 0;
        // SAFETY: as `all_devices`.
        let status = unsafe {
            AudioObjectGetPropertyDataSize(id, &address, 0, std::ptr::null(), &mut bytes)
        };
        status == 0 && bytes > 0
    }

    /// The device the monitor is playing into, by the name it was opened with.
    /// The HAL's id for the device the engine opened, by the name it
    /// reported. Crate-visible so `rtwork` can ask the same device for its
    /// IO workgroup: one resolver, so the overload watch and the worker
    /// pool can never end up watching and joining two different boxes.
    pub(crate) fn device_id(device: Option<&str>) -> Option<AudioObjectID> {
        resolve(device)
    }

    fn resolve(device: Option<&str>) -> Option<AudioObjectID> {
        let Some(wanted) = device else {
            // No name means the system DEFAULT output device. A session may
            // run a CHOSEN device instead (`Settings::audio_output_device`),
            // so anything that watches the RUNNING output must pass the name
            // the engine opened - `OutputInfo::device`, cpal's own name for
            // it, which is the `kAudioObjectPropertyName` matched below - and
            // never `None`. The overload watch, `rtwork` and `RatePoll` all
            // do.
            let id: AudioObjectID = get(SYSTEM_OBJECT, &PropertyAddress::global(DEFAULT_OUTPUT))?;
            return (id != 0).then_some(id);
        };
        // cpal names a macOS device by `kAudioObjectPropertyName`, so the
        // string the app opened by is the string to match — no translation
        // table, and nothing to keep in step when cpal changes.
        all_devices().into_iter().find(|&id| {
            has_output(id)
                && get_string(id, &PropertyAddress::global(NAME)).as_deref() == Some(wanted)
        })
    }

    /// The transport in a word the panel can print.
    ///
    /// Only the ones worth SAYING. A built-in speaker and a USB interface are
    /// both simply hardware as far as this panel is concerned, and the line
    /// exists to flag what is not.
    fn transport_word(code: u32) -> &'static str {
        match &code.to_be_bytes() {
            b"virt" => "virtual",
            b"grup" => "aggregate",
            b"bltn" => "built-in",
            b"usb " => "USB",
            b"thun" => "Thunderbolt",
            b"1394" => "FireWire",
            b"blue" | b"blea" => "Bluetooth",
            b"hdmi" => "HDMI",
            b"dprt" => "DisplayPort",
            b"airp" => "AirPlay",
            b"eavb" => "AVB",
            b"pci " => "PCI",
            _ => "",
        }
    }

    pub fn probe(device: Option<&str>) -> OutputPath {
        let Some(id) = resolve(device) else {
            return OutputPath::default();
        };
        let transport: u32 = get(id, &PropertyAddress::global(TRANSPORT)).unwrap_or(0);
        // Latency and the safety offset are both in FRAMES and both belong to
        // the output side, so both are read in the output scope and added.
        // Either may be absent; a missing one is zero rather than a refusal,
        // because the other is still worth reporting.
        let latency: u32 = get(id, &PropertyAddress::output(LATENCY)).unwrap_or(0);
        let safety: u32 = get(id, &PropertyAddress::output(SAFETY_OFFSET)).unwrap_or(0);
        OutputPath {
            known: true,
            virtualised: transport == T_VIRTUAL,
            aggregate: transport == T_AGGREGATE,
            transport: transport_word(transport).to_owned(),
            maker: get_string(id, &PropertyAddress::global(MANUFACTURER)).unwrap_or_default(),
            extra_frames: latency.saturating_add(safety),
        }
    }

    /// **The device's nominal sample rate, for the price of a property read.**
    ///
    /// The rate follower asked this question through cpal, and cpal's macOS
    /// `default_output_config` answers it by BUILDING AN AUHAL — 
    /// `audio_unit_from_device` instantiates an `AudioUnit`, sets its current
    /// device and reads the buffer-frame range back, only for the caller to
    /// throw all of that away and keep one `Float64`. Measured on this Mac,
    /// 40 samples: 21 ms at the median, 32 ms at p95, and the follower calls
    /// it TWICE a second for the life of the process — once through
    /// `output_is_live` and once for the rate itself.
    ///
    /// This is the same number by the road the HAL means for it: resolve the
    /// device, read `kAudioDevicePropertyNominalSampleRate`, done. No
    /// component instantiated, nothing registered with the audio server,
    /// nothing for a running IO proc to contend with.
    ///
    /// `None` where the HAL will not answer, which the follower already reads
    /// as "not evidence of a move" (`RateWatch::note`) — so a machine whose
    /// HAL is unhappy gets a follower that does nothing rather than a
    /// follower that reopens the audio path on a guess.
    pub fn nominal_rate(device: Option<&str>) -> Option<u32> {
        let id = resolve(device)?;
        let hz: f64 = get(id, &PropertyAddress::global(NOMINAL_RATE))?;
        // The same sanity window `Engine::default_output_rate` applies, so the
        // two roads cannot disagree about what counts as a rate at all.
        (hz.is_finite() && (8_000.0..=768_000.0).contains(&hz)).then_some(hz as u32)
    }

    /// **CoreAudio's own answer to "did a block arrive late".**
    ///
    /// Tangent could not report a dropout on macOS at all. `Engine::xruns`
    /// counts a `BackendSpecific` error whose description starts with
    /// `"xrun"`, and that string exists only in cpal's ALSA and JACK hosts —
    /// CoreAudio never emits it. `GraphDropouts` is a PipeWire reader and is
    /// `#[cfg(target_os = "linux")]`. So `merge_dropouts` was handed `None`
    /// and `0` on every Mac, answered `None`, and the warn line that exists
    /// to be grepped after "it crackled" could never fire — while the Audio
    /// Status panel told the user, confidently, zero.
    ///
    /// `kAudioDeviceProcessorOverload` is the signal that was there the whole
    /// time: the HAL posts it on the DEVICE when an IO cycle overruns. It
    /// costs nothing when nothing overruns — there is no polling here, only a
    /// listener the HAL calls — and it sees overloads caused by anyone on the
    /// device, which is the right scope for a fault the user hears in other
    /// applications too.
    ///
    /// A watch that could not resolve its device answers `None`, not `0`:
    /// "nobody could see" and "nobody was late" are the distinction the whole
    /// merge is built on, and collapsing them here would re-tell the same lie
    /// in a new place.
    pub struct OverloadWatch {
        /// `None` when the device could not be resolved — nothing was
        /// installed and there is nothing to remove.
        device: Option<AudioObjectID>,
        counter: &'static std::sync::atomic::AtomicU64,
    }

    /// The listener the HAL calls, on its own notification thread.
    ///
    /// # Safety
    ///
    /// `client_data` is the pointer handed to `AudioObjectAddPropertyListener`
    /// below, which is always a `&'static AtomicU64`.
    unsafe extern "C" fn on_overload(
        _id: AudioObjectID,
        _count: u32,
        _addresses: *const PropertyAddress,
        client_data: *mut c_void,
    ) -> OSStatus {
        if !client_data.is_null() {
            // SAFETY: the only pointer ever installed with this listener is a
            // leaked `Box<AtomicU64>`, which outlives every watch.
            let counter = unsafe { &*client_data.cast::<std::sync::atomic::AtomicU64>() };
            counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        0
    }

    impl OverloadWatch {
        /// Start watching the device the engine actually opened, by the name
        /// it reported — the same name-is-the-identity rule [`resolve`] uses
        /// for the panel, and the same one cpal itself has on this platform.
        pub fn watch(device: Option<&str>) -> Self {
            // **Leaked on purpose, and it is eight bytes.**
            //
            // `AudioObjectRemovePropertyListener` does not promise that a
            // listener already running on the HAL's thread has returned by
            // the time it does, so freeing this on `Drop` would be a
            // use-after-free with a window nobody can close from here. A
            // watch is made once per engine open; a session that reopened the
            // path a thousand times would leak eight kilobytes.
            let counter: &'static std::sync::atomic::AtomicU64 =
                Box::leak(Box::new(std::sync::atomic::AtomicU64::new(0)));
            let Some(id) = resolve(device) else {
                return Self { device: None, counter };
            };
            let address = PropertyAddress::global(PROCESSOR_OVERLOAD);
            // SAFETY: `address` is a live borrow for the length of the call,
            // and `counter` is `'static`, so the pointer stays valid for as
            // long as the HAL can call back through it.
            let status = unsafe {
                AudioObjectAddPropertyListener(
                    id,
                    &address,
                    on_overload,
                    (counter as *const std::sync::atomic::AtomicU64 as *mut std::sync::atomic::AtomicU64).cast(),
                )
            };
            if status != 0 {
                log::debug!(
                    "the overload listener would not install on this device \
                     (status {status}); dropouts will read as unknown"
                );
                return Self { device: None, counter };
            }
            Self { device: Some(id), counter }
        }

        /// Overloads since this watch started, or `None` where nothing could
        /// be watched.
        pub fn count(&self) -> Option<u64> {
            self.device
                .map(|_| self.counter.load(std::sync::atomic::Ordering::Relaxed))
        }
    }

    impl Default for OverloadWatch {
        /// A watch over nothing, for the moment before an engine exists.
        fn default() -> Self {
            Self {
                device: None,
                counter: Box::leak(Box::new(std::sync::atomic::AtomicU64::new(0))),
            }
        }
    }

    impl Drop for OverloadWatch {
        fn drop(&mut self) {
            let Some(id) = self.device else { return };
            let address = PropertyAddress::global(PROCESSOR_OVERLOAD);
            // SAFETY: the same three arguments this watch installed with.
            // The counter is `'static` and deliberately not freed here.
            unsafe {
                AudioObjectRemovePropertyListener(
                    id,
                    &address,
                    on_overload,
                    (self.counter as *const std::sync::atomic::AtomicU64
                        as *mut std::sync::atomic::AtomicU64)
                        .cast(),
                );
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// The selectors are FOUR-CHARACTER CODES and a typo in one is a
        /// property that silently does not exist — the read fails, the field
        /// stays unknown, and the panel just never mentions it. Nothing else
        /// in the file would notice, so the codes are checked against their
        /// literal values from the CoreAudio headers.
        #[test]
        fn every_selector_spells_what_the_headers_spell() {
            for (got, want) in [
                (DEVICES, 0x6465_7623),         // 'dev#'
                (DEFAULT_OUTPUT, 0x644f_7574),  // 'dOut'
                (SCOPE_GLOBAL, 0x676c_6f62),    // 'glob'
                (SCOPE_OUTPUT, 0x6f75_7470),    // 'outp'
                (NAME, 0x6c6e_616d),            // 'lnam'
                (MANUFACTURER, 0x6c6d_616b),    // 'lmak'
                (TRANSPORT, 0x7472_616e),       // 'tran'
                (LATENCY, 0x6c74_6e63),         // 'ltnc'
                (SAFETY_OFFSET, 0x7361_6674),   // 'saft'
                (STREAMS, 0x7374_6d23),         // 'stm#'
                (T_VIRTUAL, 0x7669_7274),       // 'virt'
                (T_AGGREGATE, 0x6772_7570),     // 'grup'
                (NOMINAL_RATE, 0x6e73_7274),    // 'nsrt'
                (PROCESSOR_OVERLOAD, 0x6f76_6572), // 'over'
            ] {
                assert_eq!(got, want, "a selector does not match the header");
            }
        }

        /// **The probe answers on this machine and does not hang.**
        ///
        /// It cannot assert WHAT the answer is — a build machine has whatever
        /// audio hardware it has — but "the HAL was asked and came back" is
        /// most of what can go wrong here, and a wedged HAL would show up as
        /// this test never returning.
        #[test]
        fn the_default_output_answers_or_says_it_cannot() {
            let path = probe(None);
            if path.known {
                // A real device has SOME transport. Zero would mean the read
                // succeeded and returned nothing, which is the shape of a
                // scope mistake.
                assert!(
                    !path.transport.is_empty() || path.extra_frames > 0,
                    "the device answered with neither a transport nor a latency"
                );
            }
            // A name nothing is called resolves to nothing, and must not
            // resolve to the default by accident.
            assert!(!probe(Some("\u{1}no such device\u{1}")).known);
        }

        /// **The cheap road and the expensive road must return the same
        /// number**, or the rate follower would start reopening the audio
        /// path on a disagreement between two ways of asking one question.
        ///
        /// This is the whole no-regression case for [`nominal_rate`]: it is
        /// allowed to be faster, it is not allowed to be different. Skips
        /// where either side cannot answer — a build machine with no audio
        /// device is not a machine with a bug — and that is honest, because
        /// the follower treats `None` from either road identically.
        ///
        /// **`recorder` only, because the expensive road IS cpal.** Minimal
        /// does not link an audio backend at all (see the feature note in
        /// `Cargo.toml`), so without this gate the whole test binary refuses
        /// to compile there - `cargo test -p tangent --no-default-features`
        /// died on `use cpal::traits`, which is a build the gate never runs
        /// and so never reported.
        #[cfg(feature = "recorder")]
        #[test]
        fn the_hal_rate_agrees_with_the_audio_unit_it_replaces() {
            use cpal::traits::{DeviceTrait, HostTrait};
            let Some(device) = cpal::default_host().default_output_device() else {
                eprintln!("skipping: no default output on this machine");
                return;
            };
            let Some(via_unit) = device
                .default_output_config()
                .ok()
                .map(|c| c.sample_rate().0)
                .filter(|r| (8_000..=768_000).contains(r))
            else {
                eprintln!("skipping: the default output would not report a config");
                return;
            };
            let Some(via_hal) = nominal_rate(None) else {
                eprintln!("skipping: the HAL would not report a nominal rate");
                return;
            };
            assert_eq!(
                via_hal, via_unit,
                "the HAL read and cpal's AudioUnit disagree about the default \
                 output's rate - the rate follower would reopen the path forever"
            );
        }

        /// A watch installs, answers a number, and comes back down without
        /// taking the process with it. It cannot assert an overload HAPPENED
        /// — that needs a machine in trouble — but "installed, readable,
        /// removable" is what would break silently.
        #[test]
        fn an_overload_watch_installs_and_removes() {
            let watch = OverloadWatch::watch(None);
            match watch.count() {
                // A machine with a working default output: the listener took,
                // and a fresh watch has seen nothing yet.
                Some(n) => assert_eq!(n, 0, "a new watch starts at zero"),
                // No default output, or the HAL refused: `None` is the honest
                // answer and the merge treats it as "nobody could see".
                None => eprintln!("skipping: nothing to watch on this machine"),
            }
            drop(watch);
            // A watch over nothing must also be safe to make and drop.
            assert!(OverloadWatch::watch(Some("\u{1}no such device\u{1}")).count().is_none());
            assert!(OverloadWatch::default().count().is_none());
        }
    }
}
