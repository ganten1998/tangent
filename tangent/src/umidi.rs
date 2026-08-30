//! MIDI input on FreeBSD, straight off `/dev/umidi*`.
//!
//! **Why this file exists at all.** midir 0.11 declares a backend for
//! windows/macos/ios/linux/wasm/android and nothing else. On FreeBSD the crate
//! does not compile, and the only arm in its table not keyed to an OS is the
//! JACK one — so the port's first working MIDI came from `midir/jack`, which
//! means a JACK server has to be running before a keyboard exists. For a DAW
//! that is a reasonable ask. For *this* app it is not: Tangent is a MIDI
//! keyboard monitor first, so "plug in a keyboard and it works" is the product,
//! and "install JACK, start the server, then plug in a keyboard" is a different,
//! worse product.
//!
//! FreeBSD's native MIDI is a character device per port. No Rust crate wraps
//! it, which is the real reason midir has no arm here — not that BSD is exotic,
//! but that nobody has written the twenty lines of `read(2)` it needs.
//!
//! This module is that, shaped so `midi.rs` cannot tell the difference: the
//! same five operations with the same signatures midir offers, so the call site
//! swaps one `use` and nothing else changes. Everything below is deliberately
//! the SAME SEMANTICS as midir rather than the nicest available API, because a
//! second MIDI abstraction that behaves subtly differently on one platform is
//! how a bug gets to be platform-specific and unreproducible.
//!
//! # What the device actually hands over, verified rather than assumed
//!
//! Captured from a Kawai MP6 on GhostBSD 26.1 / FreeBSD 15.0 — 2628 bytes,
//! 876 messages, kept in `tests/fixtures/kawai-mp6-umidi.raw` and parsed by the
//! tests at the bottom of this file:
//!
//! ```text
//! b0 40 11  b0 40 7f  90 47 33  80 47 40  90 4a 3d  ...
//! ```
//!
//! That is a **raw MIDI byte stream** — status byte, data bytes, exactly as it
//! travels down a DIN cable. It is emphatically NOT the USB-MIDI 4-byte event
//! packet format (which would put a cable/CIN byte in front of every message,
//! `09 90 47 33`). FreeBSD's `uaudio`/`umidi` driver unpacks the USB layer for
//! us, so what is needed here is an ordinary MIDI stream parser and not a USB
//! packet decoder. Getting this backwards would have produced a parser that
//! silently dropped every message on real hardware, so it was measured first.
//!
//! Two more things that capture proves, and that the parser has to survive:
//! the MP6 sends note-off as a real `0x80` with velocity 64 rather than
//! note-on-with-velocity-0, and it sends CONTINUOUS controller traffic for
//! half-pedalling — `b0 40 11`, `b0 40 2a`, `b0 40 49` — so the common case is
//! a controller stream with notes threaded through it, not a tidy note-on/off
//! alternation.
//!
//! # The one place this is honestly worse than midir
//!
//! midir hands the callback a DEVICE timestamp, and `midi.rs` says at length
//! why that matters: arrival time carries OS delivery latency and its jitter.
//! **FreeBSD's MIDI device does not offer one.** `read(2)` returns bytes; there
//! is no timestamp anywhere in the interface to return. So the stamp here is
//! taken at the moment the read completes, which is arrival time, and every
//! message parsed out of one read shares it.
//!
//! The error that introduces is bounded and small — a USB MIDI endpoint is
//! polled at 1 kHz, and the read loop wakes on `poll(2)` with no work between
//! the wake and the stamp — but it is not zero, and it is a real difference
//! from every other platform. It is written down here rather than hidden
//! because the recorder timestamps takes with these numbers.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Instant;

/// How long `poll(2)` waits before the read loop re-checks the stop flag.
///
/// The only cost of a shorter wait is wakeups on an idle device; the only cost
/// of a longer one is how long `drop` takes to return. A tenth of a second is
/// imperceptible when closing a port and is 10 wakeups a second at worst.
const POLL_MS: libc::c_int = 100;

/// One `read(2)`. A MIDI stream is a trickle — the busiest thing here is a
/// pedal sweep at a few hundred bytes a second — so this only has to be big
/// enough that a burst never needs two syscalls.
const READ_BUF: usize = 1024;

/// A SysEx that never ends must not grow until the machine dies. A DX7 bank —
/// the largest thing this app has a reason to receive — is 4104 bytes, so this
/// is sixty times the real worst case and still nothing.
const MAX_SYSEX: usize = 256 * 1024;

/// What to drop before the callback ever sees it.
///
/// Mirrors `midir::Ignore` variant for variant, including the combinations,
/// so the call site's `Ignore::TimeAndActiveSense` means the same thing on
/// every platform. The filtering itself matches midir's ALSA backend exactly:
/// `Time` drops MTC quarter-frame (`0xF1`), timing clock (`0xF8`) and tick
/// (`0xF9`); `ActiveSense` drops `0xFE`; `Sysex` drops `0xF0`-`0xF7`. Note what
/// `Time` does NOT drop — Start/Continue/Stop (`0xFA`-`0xFC`) and Reset
/// (`0xFF`) are transport, not timing, and midir passes them through.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Ignore {
    #[default]
    None,
    Sysex,
    Time,
    SysexAndTime,
    ActiveSense,
    SysexAndActiveSense,
    TimeAndActiveSense,
    All,
}

impl Ignore {
    fn sysex(self) -> bool {
        matches!(
            self,
            Ignore::Sysex | Ignore::SysexAndTime | Ignore::SysexAndActiveSense | Ignore::All
        )
    }
    fn time(self) -> bool {
        matches!(
            self,
            Ignore::Time | Ignore::SysexAndTime | Ignore::TimeAndActiveSense | Ignore::All
        )
    }
    fn active_sense(self) -> bool {
        matches!(
            self,
            Ignore::ActiveSense
                | Ignore::SysexAndActiveSense
                | Ignore::TimeAndActiveSense
                | Ignore::All
        )
    }

    /// Whether a complete message survives this filter. `msg` is never empty.
    fn keeps(self, msg: &[u8]) -> bool {
        match msg[0] {
            0xF1 | 0xF8 | 0xF9 => !self.time(),
            0xFE => !self.active_sense(),
            0xF0 => !self.sysex(),
            _ => true,
        }
    }
}

/// Why a MIDI operation failed.
///
/// One type for all three of midir's error positions (`InitError`,
/// `PortInfoError`, `ConnectError`) because the call site only ever turns them
/// into strings or throws them away. `PartialEq` is load-bearing: `midi.rs`
/// compares `port_name(p).as_deref() == Ok(name)` to find a port.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error(String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

/// A MIDI input port: one `/dev/umidi<unit>.<sub>` node and the name to show.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MidiInputPort {
    path: PathBuf,
    name: String,
}

/// The handle ports are enumerated and opened through.
pub struct MidiInput {
    ignore: Ignore,
}

impl MidiInput {
    /// The client name is what a MIDI *server* would show other clients. There
    /// is no server here — this opens a device node directly — so it is
    /// accepted and dropped, which keeps the call site identical.
    pub fn new(_client_name: &str) -> Result<Self, Error> {
        Ok(Self {
            ignore: Ignore::None,
        })
    }

    pub fn ignore(&mut self, flags: Ignore) {
        self.ignore = flags;
    }

    /// Every readable MIDI input node, in device order.
    ///
    /// Sorted, because `/dev` enumeration order is not defined and the port
    /// list is shown to a person and remembered in settings; an order that
    /// shuffles between runs would move the entry under their cursor.
    pub fn ports(&self) -> Vec<MidiInputPort> {
        let mut found: Vec<(u32, u32, PathBuf)> = Vec::new();
        let Ok(entries) = std::fs::read_dir("/dev") else {
            return Vec::new();
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            let Some((unit, sub)) = parse_node_name(name) else {
                continue;
            };
            found.push((unit, sub, entry.path()));
        }
        found.sort();

        // A device with several cables gets several nodes under one unit. Only
        // then is a bare product name ambiguous, so only then is it qualified —
        // a single-port keyboard should read "MP6 Stage Piano" and not
        // "MP6 Stage Piano port 1".
        let mut ports = Vec::with_capacity(found.len());
        for (unit, sub, path) in &found {
            let multi = found.iter().filter(|(u, _, _)| u == unit).count() > 1;
            let base = device_name(*unit)
                .unwrap_or_else(|| format!("MIDI {}", path.to_string_lossy()));
            let name = if multi {
                format!("{base} port {}", sub + 1)
            } else {
                base
            };
            ports.push(MidiInputPort {
                path: path.clone(),
                name,
            });
        }

        // Two identical keyboards produce two identical names, and the app
        // reconnects BY NAME — so an unqualified duplicate would be a port that
        // silently opens the wrong instrument. The node disambiguates.
        let mut out = ports.clone();
        for (i, port) in ports.iter().enumerate() {
            if ports.iter().filter(|p| p.name == port.name).count() > 1 {
                out[i].name = format!(
                    "{} ({})",
                    port.name,
                    port.path.file_name().unwrap_or_default().to_string_lossy()
                );
            }
        }
        out
    }

    pub fn port_count(&self) -> usize {
        self.ports().len()
    }

    pub fn port_name(&self, port: &MidiInputPort) -> Result<String, Error> {
        Ok(port.name.clone())
    }

    /// Open the port and start delivering messages to `callback`.
    ///
    /// The callback runs on a thread of its own — midir's contract, and what
    /// `midi.rs` is written against. It is NOT an audio callback, but it is on
    /// the path to one (`MidiFastLane::play`), so this loop does no allocation
    /// per message beyond the SysEx buffer, which only grows for SysEx.
    pub fn connect<F, T>(
        self,
        port: &MidiInputPort,
        _port_name: &str,
        mut callback: F,
        mut data: T,
    ) -> Result<MidiInputConnection<T>, Error>
    where
        F: FnMut(u64, &[u8], &mut T) + Send + 'static,
        T: Send + 'static,
    {
        let fd = open_nonblocking(&port.path)?;
        let stop = Arc::new(AtomicBool::new(false));
        let ignore = self.ignore;

        let thread_stop = stop.clone();
        let thread = std::thread::Builder::new()
            .name("tangent-umidi".into())
            .spawn(move || {
                // Owns the fd for the thread's whole life; closed on the way
                // out whichever way the loop ends.
                let guard = FdGuard(fd);
                let started = Instant::now();
                let mut parser = Parser::new();
                let mut buf = [0u8; READ_BUF];

                while !thread_stop.load(Ordering::Relaxed) {
                    let mut pfd = libc::pollfd {
                        fd: guard.0,
                        events: libc::POLLIN,
                        revents: 0,
                    };
                    // SAFETY: one initialised pollfd, count matches.
                    let r = unsafe { libc::poll(&mut pfd, 1, POLL_MS) };
                    if r < 0 {
                        // EINTR is a signal, not a failure; anything else is.
                        if std::io::Error::last_os_error().kind()
                            == std::io::ErrorKind::Interrupted
                        {
                            continue;
                        }
                        break;
                    }
                    if r == 0 {
                        continue;
                    }
                    // The keyboard was unplugged. There is nothing to recover
                    // here — the node is gone — so the thread ends and the app
                    // sees a dead connection, exactly as it would if a midir
                    // port vanished.
                    if pfd.revents & (libc::POLLHUP | libc::POLLERR | libc::POLLNVAL) != 0 {
                        break;
                    }
                    if pfd.revents & libc::POLLIN == 0 {
                        continue;
                    }

                    // SAFETY: buf is valid for its own length.
                    let n = unsafe {
                        libc::read(guard.0, buf.as_mut_ptr().cast(), buf.len())
                    };
                    if n < 0 {
                        let e = std::io::Error::last_os_error();
                        match e.kind() {
                            std::io::ErrorKind::WouldBlock
                            | std::io::ErrorKind::Interrupted => continue,
                            _ => break,
                        }
                    }
                    if n == 0 {
                        break; // EOF: device detached.
                    }

                    // ONE stamp for the whole read. See the module note: this
                    // is arrival time because the device offers nothing else,
                    // and pretending to sub-byte precision we do not have
                    // would be worse than saying so.
                    let stamp_us = started.elapsed().as_micros() as u64;
                    for &byte in &buf[..n as usize] {
                        parser.feed(byte, |msg| {
                            if ignore.keeps(msg) {
                                callback(stamp_us, msg, &mut data);
                            }
                        });
                    }
                }
                data
            })
            .map_err(|e| Error(format!("could not start the MIDI thread: {e}")))?;

        Ok(MidiInputConnection {
            stop,
            thread: Some(thread),
        })
    }
}

/// Holds the port open. Dropping it stops the thread and closes the device.
pub struct MidiInputConnection<T> {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<T>>,
}

impl<T> MidiInputConnection<T> {
    /// midir's `close` returns the input and the user data. Nothing in this
    /// app calls it; it exists so the shim is a drop-in rather than a
    /// near-miss.
    pub fn close(mut self) -> (MidiInput, T) {
        let data = self.shutdown().expect("connection joined twice");
        (
            MidiInput {
                ignore: Ignore::None,
            },
            data,
        )
    }

    fn shutdown(&mut self) -> Option<T> {
        self.stop.store(true, Ordering::Relaxed);
        self.thread.take()?.join().ok()
    }
}

impl<T> Drop for MidiInputConnection<T> {
    fn drop(&mut self) {
        // Joined rather than detached: the thread owns the callback, which owns
        // an `Arc<MidiFastLane>` and an `mpsc::Sender`. Letting it run past the
        // connection's life is how a port switch ends up with two threads
        // pushing into the same fast lane.
        let _ = self.shutdown();
    }
}

/// Closes the descriptor however the read loop ends.
struct FdGuard(libc::c_int);

impl Drop for FdGuard {
    fn drop(&mut self) {
        // SAFETY: this type owns the descriptor and runs once.
        unsafe { libc::close(self.0) };
    }
}

fn open_nonblocking(path: &Path) -> Result<libc::c_int, Error> {
    let c = std::ffi::CString::new(path.as_os_str().as_encoded_bytes())
        .map_err(|_| Error("device path contains a NUL".into()))?;
    // Non-blocking so the loop is driven by `poll` and can notice the stop flag
    // even on a keyboard nobody is touching. A blocking read would sit in the
    // kernel until the next note, which can be never, and `drop` would hang
    // waiting for it.
    // SAFETY: c is a valid NUL-terminated path for the duration of the call.
    let fd = unsafe { libc::open(c.as_ptr(), libc::O_RDONLY | libc::O_NONBLOCK) };
    if fd < 0 {
        let e = std::io::Error::last_os_error();
        // **A MIDI node takes ONE reader, and this is the error people will
        // actually hit.** Observed on the test box: a second Tangent, or any
        // other program holding the port, makes `open` fail with EBUSY. The
        // bare errno reads "Device busy", which tells a musician nothing about
        // what to do — so name the cause and the fix instead. Everything else
        // keeps the OS wording, which is the right level of detail for the
        // failures nobody can predict.
        if e.raw_os_error() == Some(libc::EBUSY) {
            return Err(Error(format!(
                "{} is open in another program — MIDI ports allow one at a \
                 time, so close whatever else is using it and reconnect",
                path.display()
            )));
        }
        return Err(Error(format!("cannot open {}: {e}", path.display())));
    }
    Ok(fd)
}

/// `umidi<unit>.<sub>` and nothing else. Returns the two numbers.
fn parse_node_name(name: &str) -> Option<(u32, u32)> {
    let rest = name.strip_prefix("umidi")?;
    let (unit, sub) = rest.split_once('.')?;
    Some((unit.parse().ok()?, sub.parse().ok()?))
}

/// A human-readable name for the device behind `/dev/umidi<unit>.*`.
///
/// **The device's own USB strings are not good enough, and the box proves it.**
/// The Kawai MP6 reports `iManufacturer = <no string>` and `iProduct =
/// "USB-MIDI"`. A port list saying "USB-MIDI" is barely better than saying
/// "umidi0.0", and would be actively wrong with two keyboards attached.
///
/// What `usbconfig list` shows instead — "MP6 Stage Piano, Kawai Musical
/// Instruments" — does not come from the device at all. It comes from the USB
/// ID database that ships with the system, looked up by vendor and product ID.
/// So this does the same thing: take the IDs from the driver's `%pnpinfo`
/// sysctl, which needs no USB ioctls and no libusb, and resolve them against
/// that database.
fn device_name(unit: u32) -> Option<String> {
    let (vid, pid) = device_ids(unit)?;
    if let Some(name) = lookup_usb_ids(vid, pid) {
        return Some(name);
    }
    // No database entry. The IDs themselves still beat the node name, and they
    // are the thing somebody would search for.
    Some(format!("USB MIDI {vid:04x}:{pid:04x}"))
}

/// Vendor and product ID from the driver that created the node.
///
/// Two driver names because either can own a MIDI endpoint: `uaudio` claims
/// USB audio-class devices (which is what a class-compliant keyboard is, even
/// with no audio streams — this box logs "uaudio0: No playback. No recording.
/// MIDI sequencer."), while `umidi` claims MIDI-only ones. The unit number in
/// the node name matches the driver unit.
fn device_ids(unit: u32) -> Option<(u16, u16)> {
    for driver in ["uaudio", "umidi"] {
        let oid = format!("dev.{driver}.{unit}.%pnpinfo");
        let Some(info) = sysctl_string(&oid) else {
            continue;
        };
        let vid = field_hex(&info, "vendor=")?;
        let pid = field_hex(&info, "product=")?;
        return Some((vid, pid));
    }
    None
}

/// `vendor=0x0f54 product=0x0101 ...` — pull one hex field out.
fn field_hex(info: &str, key: &str) -> Option<u16> {
    let rest = info.split(key).nth(1)?;
    let tok = rest.split_whitespace().next()?;
    let tok = tok.strip_prefix("0x").unwrap_or(tok);
    u16::from_str_radix(tok, 16).ok()
}

fn sysctl_string(oid: &str) -> Option<String> {
    let name = std::ffi::CString::new(oid).ok()?;
    let mut len: libc::size_t = 0;
    // SAFETY: null buffer with a valid length pointer asks for the size only.
    let r = unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            std::ptr::null_mut(),
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    if r != 0 || len == 0 {
        return None;
    }
    let mut buf = vec![0u8; len];
    // SAFETY: buf is len bytes and len is what the kernel just asked for.
    let r = unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            buf.as_mut_ptr().cast(),
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    if r != 0 {
        return None;
    }
    buf.truncate(len);
    while buf.last() == Some(&0) {
        buf.pop();
    }
    String::from_utf8(buf).ok()
}

/// Where the system keeps its USB ID database.
///
/// `usb_vendors` is FreeBSD/GhostBSD's own copy; `usb.ids` is the one
/// `hwdata` installs and what most of the world calls it. Both use the same
/// format, so both are read the same way and the first that answers wins.
const USB_ID_FILES: [&str; 3] = [
    "/usr/share/misc/usb_vendors",
    "/usr/local/share/hwdata/usb.ids",
    "/usr/share/misc/usb.ids",
];

fn lookup_usb_ids(vid: u16, pid: u16) -> Option<String> {
    for path in USB_ID_FILES {
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        if let Some(found) = lookup_in_usb_ids(&text, vid, pid) {
            return Some(found);
        }
    }
    None
}

/// The `usb.ids` format, which is two levels of indentation and nothing else:
///
/// ```text
/// 0f54  Kawai Musical Instruments Mfg. Co., Ltd
/// <TAB>0101  MP6 Stage Piano
/// ```
///
/// Split out from the file reading so it can be tested against the real lines
/// without depending on the host having the database installed.
fn lookup_in_usb_ids(text: &str, vid: u16, pid: u16) -> Option<String> {
    let want_vendor = format!("{vid:04x}");
    let want_product = format!("{pid:04x}");
    let mut in_vendor = false;
    let mut vendor_name = String::new();

    for line in text.lines() {
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        if !line.starts_with('\t') {
            // A new vendor ends the one we were reading. Leaving early matters:
            // this file is three quarters of a megabyte.
            if in_vendor {
                return Some(vendor_name);
            }
            in_vendor = line
                .get(..4)
                .is_some_and(|id| id.eq_ignore_ascii_case(&want_vendor));
            if in_vendor {
                vendor_name = line[4..].trim().to_owned();
            }
            continue;
        }
        if !in_vendor {
            continue;
        }
        let entry = line.trim_start_matches('\t');
        // Nested deeper than one tab is an interface, not a product.
        if entry.starts_with('\t') {
            continue;
        }
        if entry
            .get(..4)
            .is_some_and(|id| id.eq_ignore_ascii_case(&want_product))
        {
            return Some(entry[4..].trim().to_owned());
        }
    }
    in_vendor.then_some(vendor_name)
}

/// A MIDI byte stream, one byte at a time, into whole messages.
///
/// The three rules that make this more than a `match`, all of which real
/// hardware relies on:
///
/// * **Running status.** A status byte may be omitted when it repeats, so
///   `90 40 7f 43 7f` is two note-ons. The captured MP6 never does this, which
///   is exactly why it is tested separately — a parser that only ever met that
///   keyboard would look correct and fail on the next one.
/// * **System real-time can arrive ANYWHERE**, including between a status byte
///   and its data bytes. `90 40 f8 7f` is a note-on with a clock tick wedged
///   inside it, and both must come out whole.
/// * **System common cancels running status**, where system real-time does not.
struct Parser {
    /// Current running status, or 0 when there is none.
    status: u8,
    data: [u8; 2],
    have: usize,
    sysex: Vec<u8>,
    in_sysex: bool,
    /// Reused so an ordinary channel message never allocates.
    scratch: [u8; 3],
}

impl Parser {
    fn new() -> Self {
        Self {
            status: 0,
            data: [0; 2],
            have: 0,
            sysex: Vec::new(),
            in_sysex: false,
            scratch: [0; 3],
        }
    }

    /// How many data bytes a status byte takes, or `None` if it is not one we
    /// collect data for.
    fn expected(status: u8) -> Option<usize> {
        match status & 0xF0 {
            0x80 | 0x90 | 0xA0 | 0xB0 | 0xE0 => Some(2),
            0xC0 | 0xD0 => Some(1),
            0xF0 => match status {
                0xF1 | 0xF3 => Some(1),
                0xF2 => Some(2),
                0xF6 => Some(0),
                _ => None,
            },
            _ => None,
        }
    }

    fn feed<F: FnMut(&[u8])>(&mut self, byte: u8, mut emit: F) {
        // System real-time: one byte, always complete, never disturbs whatever
        // message is half-read around it. Checked FIRST for that reason.
        if byte >= 0xF8 {
            emit(&[byte]);
            return;
        }

        if byte >= 0x80 {
            // End of SysEx is the only status byte that is not a beginning.
            if byte == 0xF7 {
                if self.in_sysex {
                    self.sysex.push(0xF7);
                    emit(&self.sysex);
                    self.sysex.clear();
                    self.in_sysex = false;
                }
                return;
            }
            // Any other status byte while a SysEx is open means the SysEx was
            // truncated — a real thing on a cable pulled mid-dump. Drop the
            // fragment rather than emitting a malformed message.
            if self.in_sysex {
                self.in_sysex = false;
                self.sysex.clear();
            }
            if byte == 0xF0 {
                self.in_sysex = true;
                self.sysex.clear();
                self.sysex.push(0xF0);
                self.status = 0; // SysEx cancels running status.
                return;
            }
            self.have = 0;
            match Self::expected(byte) {
                Some(0) => {
                    emit(&[byte]);
                    self.status = 0; // Tune Request is system common.
                }
                Some(_) => {
                    self.status = byte;
                }
                // 0xF4 / 0xF5 are undefined. Consume them and clear running
                // status, which is what every sane parser does with a reserved
                // status byte.
                None => self.status = 0,
            }
            return;
        }

        // A data byte.
        if self.in_sysex {
            if self.sysex.len() < MAX_SYSEX {
                self.sysex.push(byte);
            } else {
                // Runaway. Abandon it; the next status byte starts afresh.
                self.in_sysex = false;
                self.sysex.clear();
            }
            return;
        }
        if self.status == 0 {
            return; // Data with no status: nothing can be made of it.
        }
        self.data[self.have] = byte;
        self.have += 1;
        let Some(want) = Self::expected(self.status) else {
            self.status = 0;
            self.have = 0;
            return;
        };
        if self.have < want {
            return;
        }
        self.scratch[0] = self.status;
        self.scratch[1] = self.data[0];
        self.scratch[2] = self.data[1];
        emit(&self.scratch[..=want]);
        self.have = 0;
        // Running status applies to channel messages only; system common
        // (0xF1-0xF3) is one-shot.
        if self.status >= 0xF0 {
            self.status = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Feed a whole slice and collect the messages that come out.
    fn parse(bytes: &[u8], ignore: Ignore) -> Vec<Vec<u8>> {
        let mut p = Parser::new();
        let mut out = Vec::new();
        for &b in bytes {
            p.feed(b, |m| {
                if ignore.keeps(m) {
                    out.push(m.to_vec());
                }
            });
        }
        out
    }

    #[test]
    fn a_note_on_and_off_come_out_whole() {
        let msgs = parse(&[0x90, 0x40, 0x7F, 0x80, 0x40, 0x40], Ignore::None);
        assert_eq!(msgs, vec![vec![0x90, 0x40, 0x7F], vec![0x80, 0x40, 0x40]]);
    }

    #[test]
    fn running_status_repeats_the_last_status() {
        // Three note-ons, one status byte between them.
        let msgs = parse(&[0x90, 0x40, 0x7F, 0x43, 0x7F, 0x47, 0x60], Ignore::None);
        assert_eq!(
            msgs,
            vec![
                vec![0x90, 0x40, 0x7F],
                vec![0x90, 0x43, 0x7F],
                vec![0x90, 0x47, 0x60],
            ]
        );
    }

    #[test]
    fn real_time_interleaves_inside_a_message_without_breaking_it() {
        // A clock tick lands between the note number and the velocity.
        let msgs = parse(&[0x90, 0x40, 0xF8, 0x7F], Ignore::None);
        assert_eq!(msgs, vec![vec![0xF8], vec![0x90, 0x40, 0x7F]]);
    }

    #[test]
    fn real_time_does_not_cancel_running_status() {
        let msgs = parse(&[0x90, 0x40, 0x7F, 0xFE, 0x43, 0x7F], Ignore::None);
        assert_eq!(
            msgs,
            vec![
                vec![0x90, 0x40, 0x7F],
                vec![0xFE],
                vec![0x90, 0x43, 0x7F],
            ]
        );
    }

    #[test]
    fn system_common_does_cancel_running_status() {
        // Song Select, then two bytes that must NOT become a note-on.
        let msgs = parse(&[0x90, 0x40, 0x7F, 0xF3, 0x02, 0x43, 0x7F], Ignore::None);
        assert_eq!(
            msgs,
            vec![vec![0x90, 0x40, 0x7F], vec![0xF3, 0x02]],
            "data after system common must be discarded, not run on"
        );
    }

    #[test]
    fn sysex_is_collected_whole() {
        let msgs = parse(&[0xF0, 0x43, 0x00, 0x01, 0xF7], Ignore::None);
        assert_eq!(msgs, vec![vec![0xF0, 0x43, 0x00, 0x01, 0xF7]]);
    }

    #[test]
    fn real_time_inside_sysex_comes_out_without_corrupting_it() {
        let msgs = parse(&[0xF0, 0x43, 0xF8, 0x00, 0xF7], Ignore::None);
        assert_eq!(msgs, vec![vec![0xF8], vec![0xF0, 0x43, 0x00, 0xF7]]);
    }

    #[test]
    fn a_truncated_sysex_is_dropped_rather_than_emitted_malformed() {
        // Cable pulled mid-dump: a new status byte arrives instead of 0xF7.
        let msgs = parse(&[0xF0, 0x43, 0x00, 0x90, 0x40, 0x7F], Ignore::None);
        assert_eq!(msgs, vec![vec![0x90, 0x40, 0x7F]]);
    }

    #[test]
    fn data_with_no_status_is_ignored() {
        assert!(parse(&[0x40, 0x7F, 0x00], Ignore::None).is_empty());
    }

    #[test]
    fn one_byte_channel_messages_take_one_data_byte() {
        // Program change and channel pressure.
        let msgs = parse(&[0xC0, 0x05, 0xD0, 0x40], Ignore::None);
        assert_eq!(msgs, vec![vec![0xC0, 0x05], vec![0xD0, 0x40]]);
    }

    #[test]
    fn tune_request_has_no_data_and_ends_running_status() {
        let msgs = parse(&[0x90, 0x40, 0x7F, 0xF6, 0x43, 0x7F], Ignore::None);
        assert_eq!(msgs, vec![vec![0x90, 0x40, 0x7F], vec![0xF6]]);
    }

    #[test]
    fn the_ignore_filter_matches_midirs_alsa_backend() {
        // Time drops quarter-frame, clock and tick. ActiveSense drops 0xFE.
        // Neither drops transport (0xFA/0xFB/0xFC) or reset (0xFF).
        let stream = [
            0xF1, 0x00, // MTC quarter frame
            0xF8, // clock
            0xF9, // tick
            0xFE, // active sensing
            0xFA, // start
            0xFF, // reset
            0x90, 0x40, 0x7F,
        ];
        let kept = parse(&stream, Ignore::TimeAndActiveSense);
        assert_eq!(
            kept,
            vec![vec![0xFA], vec![0xFF], vec![0x90, 0x40, 0x7F]],
            "transport and reset must survive a Time+ActiveSense filter"
        );

        // And with no filter, everything comes through.
        assert_eq!(parse(&stream, Ignore::None).len(), 7);
    }

    #[test]
    fn sysex_survives_time_and_active_sense_because_the_recorder_wants_it() {
        let msgs = parse(&[0xF0, 0x43, 0xF7], Ignore::TimeAndActiveSense);
        assert_eq!(msgs, vec![vec![0xF0, 0x43, 0xF7]]);
        assert!(parse(&[0xF0, 0x43, 0xF7], Ignore::Sysex).is_empty());
    }

    #[test]
    fn a_runaway_sysex_cannot_grow_without_bound() {
        let mut p = Parser::new();
        let mut emitted = 0;
        p.feed(0xF0, |_| emitted += 1);
        for _ in 0..(MAX_SYSEX + 1024) {
            p.feed(0x00, |_| emitted += 1);
        }
        assert_eq!(emitted, 0, "an unterminated SysEx must never be emitted");
        assert!(
            p.sysex.len() <= MAX_SYSEX,
            "buffer grew past the cap: {}",
            p.sysex.len()
        );
    }

    // ── The real device ──────────────────────────────────────────────────────

    /// 2628 bytes captured from a Kawai MP6 Stage Piano on FreeBSD 15.0,
    /// straight off `/dev/umidi0.0`. Scales, chords and pedal.
    const KAWAI: &[u8] = include_bytes!("../../tests/fixtures/kawai-mp6-umidi.raw");

    #[test]
    fn the_captured_keyboard_parses_completely() {
        let msgs = parse(KAWAI, Ignore::None);
        assert_eq!(msgs.len(), 876, "message count changed");

        // Every message is well formed: a status byte, then only data bytes.
        for m in &msgs {
            assert!(!m.is_empty());
            assert!(m[0] >= 0x80, "message does not start with a status byte");
            assert!(
                m[1..].iter().all(|b| *b < 0x80),
                "a data byte has its high bit set: {m:02x?}"
            );
        }

        // Consumed to the last byte: a parser that resynchronised badly would
        // leave a partial message behind.
        let consumed: usize = msgs.iter().map(|m| m.len()).sum();
        assert_eq!(consumed, KAWAI.len(), "bytes were dropped or duplicated");
    }

    #[test]
    fn the_captured_keyboard_is_notes_and_a_sustain_pedal() {
        let msgs = parse(KAWAI, Ignore::None);
        let note_on = msgs.iter().filter(|m| m[0] & 0xF0 == 0x90).count();
        let note_off = msgs.iter().filter(|m| m[0] & 0xF0 == 0x80).count();
        let sustain = msgs
            .iter()
            .filter(|m| m[0] & 0xF0 == 0xB0 && m[1] == 64)
            .count();

        assert!(note_on > 300, "expected a lot of notes, got {note_on}");
        // This keyboard sends real note-offs rather than note-on velocity 0,
        // which is the case the chord detector has to handle.
        assert!(note_off > 300, "expected real note-offs, got {note_off}");
        assert!(sustain > 0, "the capture includes pedal movement");

        // Half-pedalling: the pedal is a continuous controller, not a switch.
        let levels: std::collections::BTreeSet<u8> = msgs
            .iter()
            .filter(|m| m[0] & 0xF0 == 0xB0 && m[1] == 64)
            .map(|m| m[2])
            .collect();
        assert!(
            levels.len() > 2,
            "expected continuous pedal values, saw {levels:?}"
        );
    }

    #[test]
    fn the_capture_parses_the_same_split_across_arbitrary_reads() {
        // The read loop hands the parser whatever `read(2)` returned, so a
        // message WILL be split across two buffers. Same bytes, same messages,
        // however they are chopped.
        let whole = parse(KAWAI, Ignore::None);
        for chunk in [1usize, 2, 3, 7, 64, 1023] {
            let mut p = Parser::new();
            let mut out = Vec::new();
            for part in KAWAI.chunks(chunk) {
                for &b in part {
                    p.feed(b, |m| out.push(m.to_vec()));
                }
            }
            assert_eq!(out, whole, "chunking at {chunk} changed the result");
        }
    }

    // ── Naming ───────────────────────────────────────────────────────────────

    #[test]
    fn node_names_parse_and_others_are_refused() {
        assert_eq!(parse_node_name("umidi0.0"), Some((0, 0)));
        assert_eq!(parse_node_name("umidi3.1"), Some((3, 1)));
        assert_eq!(parse_node_name("umidi10.2"), Some((10, 2)));
        assert_eq!(parse_node_name("umidi0"), None);
        assert_eq!(parse_node_name("midi0.0"), None);
        assert_eq!(parse_node_name("dsp0"), None);
        assert_eq!(parse_node_name("umidi.x"), None);
    }

    #[test]
    fn pnpinfo_fields_are_read_the_way_the_kernel_writes_them() {
        // Verbatim from `sysctl dev.uaudio.0.%pnpinfo` on the test box.
        let info = "vendor=0x0f54 product=0x0101 devclass=0x00 devsubclass=0x00 \
                    devproto=0x00 sernum=\"\" release=0x0100 mode=host \
                    intclass=0x01 intsubclass=0x03 intprotocol=0x00";
        assert_eq!(field_hex(info, "vendor="), Some(0x0f54));
        assert_eq!(field_hex(info, "product="), Some(0x0101));
        assert_eq!(field_hex(info, "nothing="), None);
    }

    #[test]
    fn the_usb_id_database_resolves_the_test_keyboard() {
        // The real shape of /usr/share/misc/usb_vendors around this entry.
        let db = "\
0f53  Analog Devices\n\
\t0001  Something Else\n\
0f54  Kawai Musical Instruments Mfg. Co., Ltd\n\
\t0101  MP6 Stage Piano\n\
\t0102  Another Piano\n\
0f55  AmbiCom, Inc.\n";

        assert_eq!(
            lookup_in_usb_ids(db, 0x0f54, 0x0101).as_deref(),
            Some("MP6 Stage Piano")
        );
        assert_eq!(
            lookup_in_usb_ids(db, 0x0f54, 0x0102).as_deref(),
            Some("Another Piano")
        );
        // Known vendor, unknown product: the vendor name is still worth having.
        assert_eq!(
            lookup_in_usb_ids(db, 0x0f54, 0x9999).as_deref(),
            Some("Kawai Musical Instruments Mfg. Co., Ltd")
        );
        // Unknown vendor entirely.
        assert_eq!(lookup_in_usb_ids(db, 0xABCD, 0x0001), None);
    }

    #[test]
    fn nested_interface_lines_are_not_mistaken_for_products() {
        // usb.ids indents interfaces one tab deeper than products.
        let db = "\
0f54  Kawai\n\
\t0101  MP6 Stage Piano\n\
\t\t0101  Some Interface\n\
0f55  Next\n";
        assert_eq!(
            lookup_in_usb_ids(db, 0x0f54, 0x0101).as_deref(),
            Some("MP6 Stage Piano")
        );
    }

    #[test]
    fn the_connection_is_send_because_the_app_requires_it() {
        // `MidiPorts: Send` (tangent-ui/src/ports.rs) and `DeviceMidi` holds
        // the connection inside a `Box<dyn MidiPorts>`, so a connection that
        // is not `Send` fails to compile at the call site with an error
        // pointing at desktop.rs rather than here. Assert it where the type
        // lives.
        fn assert_send<T: Send>() {}
        assert_send::<MidiInputConnection<()>>();
        assert_send::<MidiInputPort>();
    }

    #[test]
    fn enumeration_does_not_panic_without_hardware() {
        // The test box has no keyboard attached most of the time. This must be
        // an empty list, never a panic.
        let input = MidiInput::new("test").expect("MidiInput::new cannot fail");
        let _ = input.ports();
    }
}
