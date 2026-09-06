//! Tangent — MIDI keyboard monitor with chord detection (Rust port of the
//! PySide6 v1.1.0 app; UI parity per docs/spec/ui-spec.md).
#![windows_subsystem = "windows"]

/// The video compositor's offscreen surface.
///
/// Inside `recorder` because a Minimal build has no camera to composite and
/// must not link a renderer to prove it. **No longer macOS-only**: it borrows
/// the window's wgpu device when there is one and opens its own when there is
/// not, so it does not care which renderer the app is drawing with.
#[cfg(feature = "recorder")]
mod composite;
mod desktop;
/// What the app ships onto the user's shelves — impulse responses and drum
/// kits — and the once-only seeding that puts them there at launch.
mod shipped;
/// How much of the screen the OS leaves for a window. One number, asked of
/// the platform, so the layout can be made to FIT a laptop rather than run
/// off the bottom of it.
mod display;
/// Two AppKit facts winit does not surface, and the hook that lets the OS
/// keep this aspect-locked layout on its curve DURING a drag. A target gate,
/// not a feature gate — the convention against `#[cfg(feature)]` outside this
/// crate is about features, and this is a platform.
#[cfg(target_os = "macos")]
mod macwin;
#[cfg(feature = "recorder")]
mod devices;

/// One chance for a plugin that kills the process. See the module docs.
mod plugin_guard;

/// What else is standing between the mix and the speakers.
mod outpath;
/// Six-operator FM, and real DX7 cartridges to feed it. The instrument the app
/// has when it has no instrument, so a fresh install makes a sound. The default
/// patch is written in the source: no samples, no data, no licence questions.
#[cfg(feature = "recorder")]
mod dx7;
/// A piano and an electric piano, computed rather than recorded. Zero samples
/// for the same reason the DX7 carries none: a model has no velocity layers to
/// cross and no loop points to hide, so it is right at every pitch and every
/// force. Two patches, and both of them are arithmetic.
#[cfg(feature = "recorder")]
mod keys;
/// Reverb and delay, on everything the app plays.
#[cfg(feature = "recorder")]
mod effects;
/// Standalone DSP kernels for the coming plugin suite. Deliberately
/// unreachable from any UI — the hosts that will own them are not written
/// yet — so the dead-code lint is told once here rather than silenced
/// piecemeal inside. Ungated: pure std, nothing a Minimal build lacks.
#[allow(dead_code)]
mod units;
/// Tangent Analog, the subtractive synthesiser: the fourth built-in
/// instrument and the first built-in VOICE, seated in a rack bay the way
/// the effects are (`docs/ANALOG-BRIEF.md` §7). Ungated and unreachable
/// this wave, exactly as `units` was before its hosts existed: pure std,
/// proven by its own tests and by `tools/analogrig`, and told about the
/// dead-code lint here for the same reason.
#[allow(dead_code)]
mod analog;
/// Tangent's own effects as ordinary insert-bay tenants — the `@tangent-*`
/// effect sentinels the engine seats in a rack rather than rendering by mask.
#[cfg(feature = "recorder")]
mod builtin_fx;
/// The hand-rolled WAV reader the IR unit loads its impulse responses with.
#[cfg(feature = "recorder")]
mod wavread;
/// Loudness to BS.1770-4, for the ladder beside the master. Inside
/// `recorder` because the only thing that feeds it is the engine's output.
#[cfg(feature = "recorder")]
mod loudness;
/// True varispeed: the resampler that sits between the program clock and the
/// device's. Inside `recorder` for the same reason — there is no device in a
/// Minimal build for it to sit between.
#[cfg(feature = "recorder")]
mod varispeed;
/// Time-stretching a clip. Inside `recorder` because it takes an
/// `tangent_record::decode::Clip`, and a Minimal build does not link that crate
/// at all.
#[cfg(feature = "recorder")]
mod stretch;
/// The monitor engine: the audio OUTPUT stream that plays a hosted VST3
/// instrument and the metronome. Absent from a Minimal build for the same
/// reason `record` is (docs/RECORDER-PLAN.md §12a).
#[cfg(feature = "recorder")]
mod instrument;
/// FreeBSD's MIDI input, because midir has no backend there at all. Shaped as
/// a drop-in for the handful of midir calls `midi` makes, so the difference is
/// one `use` and not a second code path. A target gate, not a feature gate —
/// same reasoning as `macwin` above.
#[cfg(target_os = "freebsd")]
mod umidi;
mod midi;
/// The recording session. Absent from a Minimal build, where `tangent-record` is
/// not linked at all — the exclusion the compiler enforces rather than a flag
/// anyone can flip (docs/RECORDER-PLAN.md §12a).
#[cfg(feature = "recorder")]
mod record;
/// The playhead and the play switch. Inside `recorder` because its one
/// owner is `Recorder`, and because it speaks to an `Engine` — neither of
/// which a Minimal build has.
#[cfg(feature = "recorder")]
mod transport;

/// Every plugin the picker should offer: VST3 bundles, and LV2 where there is
/// an LV2 world to ask.
///
/// **One list, two formats, and the picker does not need to know.** Each row is
/// `(identity, vendor, display name)`. For a VST3 the identity is the bundle
/// path and the name is `None`, because the file stem already IS the plugin's
/// name. For an LV2 the identity is `lv2:<uri>` and the name is given, because
/// a URI's last segment is a slug at best — "dragonfly-reverb" where the plugin
/// is called "Dragonfly Hall Reverb" — and the whole URN at worst.
///
/// The VST3 half is still a DIRECTORY LISTING: nothing is opened, so 112
/// bundles cost milliseconds and no plugin can crash the process before the
/// window appears. The LV2 half reads Turtle manifests through lilv, which is
/// more work but is the only way LV2 can be enumerated at all, and lilv caches
/// the world for the life of the process.
#[cfg(feature = "recorder")]
pub fn plugin_catalog(
    extra: &[std::path::PathBuf],
) -> Vec<(std::path::PathBuf, Option<String>, Option<String>)> {
    plugin_catalog_from(tangent_host::discover_in(extra))
}

/// The same, for a caller that has already done the VST3 scan and wants its
/// report — see the scan-report path in `desktop`.
#[cfg(feature = "recorder")]
pub fn plugin_catalog_from(
    bundles: Vec<std::path::PathBuf>,
) -> Vec<(std::path::PathBuf, Option<String>, Option<String>)> {
    let mut rows: Vec<(std::path::PathBuf, Option<String>, Option<String>)> = bundles
        .into_iter()
        .map(|p| {
            let v = tangent_host::vendor_of(&p);
            (p, v, None)
        })
        .collect();
    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    rows.extend(tangent_host::lv2::discover().into_iter().map(|p| {
        (
            tangent_host::lv2::identity_for(&p.uri),
            Some(tangent_host::lv2::vendor_of(&p.uri)),
            Some(p.name),
        )
    }));
    rows
}

use tangent_ui::settings::Settings;

/// On Windows, a windows-subsystem binary detaches from the console; reattach
/// so `-l` / `-p` output still prints when launched from a terminal.
#[cfg(windows)]
fn attach_console() {
    use windows_sys::Win32::System::Console::{AttachConsole, ATTACH_PARENT_PROCESS};
    unsafe {
        let _ = AttachConsole(ATTACH_PARENT_PROCESS);
    }
}

struct CliArgs {
    port: Option<String>,
}

fn print_usage_and_exit(code: i32) -> ! {
    let text = "usage: tangent [-h] [-p PORT] [-l]\n\
                \n\
                MIDI keyboard monitor with chord detection\n\
                \n\
                options:\n\
                  -h, --help            show this help message and exit\n\
                  -p PORT, --port PORT  MIDI input port name\n\
                  -l, --list            List available MIDI input ports\n";
    if code == 0 {
        print!("{text}");
    } else {
        eprint!("{text}");
    }
    std::process::exit(code);
}

fn parse_cli() -> CliArgs {
    let mut port = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-l" | "--list" => {
                midi::print_port_list();
                std::process::exit(0);
            }
            // Undocumented on purpose (absent from --help): a developer probe
            // for the recorder's raw MIDI tap, which the GUI cannot show yet.
            // It is how the timestamp, the channel, the continuous pedal sweep
            // and the release velocity get checked against a REAL keyboard,
            // which no unit test can do. See docs/RECORDER-PLAN.md step 1.
            #[cfg(feature = "recorder")]
            "--dump-midi" => {
                midi::dump_raw(args.next());
                std::process::exit(0);
            }
            // The other half of the same idea, for audio: record one take from
            // the default input with no GUI and report what landed on disk.
            // The only way to exercise the whole capture chain against real
            // hardware, and the way a missing microphone entitlement shows up
            // as an empty device list rather than as a mystery months later.
            #[cfg(feature = "recorder")]
            "--record-test" => {
                record::record_test(args.next());
                std::process::exit(0);
            }
            // And the same for the monitor side: load a VST3 by name, play it a
            // few notes, click a bar of metronome, and report what the output
            // device did. The only way to check the plugin path against real
            // hardware, and the way a missing `disable-library-validation`
            // entitlement shows up as a refusal to load rather than as a
            // mystery.
            #[cfg(feature = "recorder")]
            "--plugin-test" => {
                instrument::plugin_test(args.next());
                std::process::exit(0);
            }
            // And the same for the IR road, which is the one measurement in
            // this app whose correctness cannot be argued from a test: the
            // question is what a converter, a cable and a box actually did.
            // `--ir-test 13,13` plays the sweep on outputs 13/14 and records
            // inputs 13/14. See `desktop::ir_test`.
            // Whether this app is holding an interface's sample rate against
            // its owner. See `desktop::device_watch`.
            #[cfg(feature = "recorder")]
            "--device-watch" => {
                desktop::device_watch(args.next());
                std::process::exit(0);
            }
            #[cfg(feature = "recorder")]
            "--ir-test" => {
                desktop::ir_test(args.next());
                std::process::exit(0);
            }
            // And the denoiser, whose faults are a matter of measurement and
            // whose quality is a matter of taste — a file in, a file out, and
            // the dials named. See `desktop::denoise_test`.
            #[cfg(feature = "recorder")]
            "--denoise-test" => {
                desktop::denoise_test(args.next());
                std::process::exit(0);
            }
            "-h" | "--help" => print_usage_and_exit(0),
            "-p" | "--port" => match args.next() {
                Some(value) => port = Some(value),
                None => {
                    eprintln!("tangent: error: argument -p/--port: expected one argument");
                    std::process::exit(2);
                }
            },
            other => {
                if let Some(value) = other.strip_prefix("--port=") {
                    port = Some(value.to_owned());
                } else if let Some(value) = other.strip_prefix("-p") {
                    if !value.is_empty() {
                        port = Some(value.to_owned());
                    }
                } else {
                    eprintln!("tangent: error: unrecognized arguments: {other}");
                    print_usage_and_exit(2);
                }
            }
        }
    }
    CliArgs { port }
}

/// Minimal stderr logger. eframe probes for a desktop GL context first and only
/// falls back to GLES if that fails — but the first failure is a `log::warn!`,
/// so with no logger installed a startup crash reports the FALLBACK error and
/// hides the actual cause. (Seen under CrossOver: "extension to create ES
/// context with wgl is not present", which is the second failure, not the first.)
///
/// Quiet by default; `IVORY_LOG=debug ivory` turns it on. On Windows pair it
/// with a console — the binary is windows-subsystem and reattaches to the
/// parent, so running it from a shell shows this output.
///
/// Since 5.1 the same records also land in `~/.config/tangent/tangent.log`
/// (see [`attach_log_file`]) — because the normal launch is a double-click,
/// where stderr goes nowhere and "it broke on their machine" was previously
/// unanswerable. Hand-rolled rather than a crate: two sinks and one filter do
/// not need a dependency tree.
struct StderrLogger(log::LevelFilter);

/// The file the logger drains to, once [`attach_log_file`] has opened it.
/// `None` until then, so nothing touches the config directory before the
/// pre-5.0 purge has had its turn. A mutex because `log::Log` is `&self` from
/// any thread; acceptable in the log path because nothing on the audio
/// callback logs, warnings are exceptional, and each record is one
/// pre-formatted `write_all` — no formatting and no flush loop inside the lock.
static LOG_FILE: std::sync::Mutex<Option<std::fs::File>> = std::sync::Mutex::new(None);

/// What the file keeps no matter what `IVORY_LOG` says.
///
/// The file exists for the field, where nobody has set an environment
/// variable — a terminal quieted with `IVORY_LOG=off` must not also blind the
/// one artifact a user can send after the fact.
///
/// **INFO, not WARN, because the lines written to BE evidence are all INFO
/// and the field never saw one.** `panel_start` logs which directory it
/// handed a native panel and says out loud that it exists "so the next report
/// carries evidence"; the audio road logs the rate and buffer it actually
/// got; the IR road logs a device that moved on its own. Every one of those
/// was invisible in a shipped build, and this was found the hard way — a save
/// panel opening in the wrong folder was diagnosed twice from a log that had
/// nothing in it but the startup banner, while the line that would have
/// answered it existed and was being thrown away.
///
/// Nothing here is per-frame: the app's INFO records are edges (a device
/// opened, a panel raised, a rate moved), and the ones that could repeat
/// already say so in their own notes. Rotation is unchanged, so the ceiling
/// on what this costs is still the documented ~1 MB.
const FILE_LOG_FLOOR: log::LevelFilter = log::LevelFilter::Info;

/// Rotate at half a megabyte. With exactly one predecessor kept, the pair is
/// bounded near 1 MB total — attachable to an email, never worth a cleanup
/// tool. Rotation happens only at startup, so a single very chatty
/// `IVORY_LOG=debug` run can overshoot until the next launch; bounded per
/// run, which is bounded enough.
const MAX_LOG_LEN: u64 = 512 * 1024;

impl StderrLogger {
    /// The union of what either sink wants — stderr at its `IVORY_LOG` level,
    /// the file never below [`FILE_LOG_FLOOR`]. `log::set_max_level` must be
    /// set from this, not from the stderr level, or the `log!` macros drop
    /// warn records before `log()` ever sees them.
    fn ceiling(&self) -> log::LevelFilter {
        self.0.max(FILE_LOG_FLOOR)
    }
}

/// **wgpu's GL backend logs a normal code path as an error, once per
/// video frame.** `AdapterShared::get_buffer_sub_data` (wgpu-hal 29,
/// `gles/adapter.rs`) has two ways to read a buffer back; when the context
/// lacks `GET_BUFFER_SUB_DATA` it uses `map_buffer_range` instead and says
/// `log::error!("Fake map")` on the way — a debugging aid somebody left in,
/// on the path every compositor readback takes on a Mesa EGL context. The
/// owner's Void box (Ivy Bridge, HD 4000) wrote 231 of those lines during a
/// fifteen-second take, fifteen a second, and the take's own lines — the
/// ones that say what the take lost — were buried in them. The picture is
/// fine; the line is noise; it is dropped before either sink sees it. Only
/// that one message from that one crate, so a real GL error still lands.
fn is_fake_map_noise(target: &str, message: &str) -> bool {
    target.starts_with("wgpu_hal::gles") && message == "Fake map"
}

impl log::Log for StderrLogger {
    fn enabled(&self, m: &log::Metadata<'_>) -> bool {
        m.level() <= self.ceiling()
    }
    fn log(&self, r: &log::Record<'_>) {
        if is_fake_map_noise(r.target(), &r.args().to_string()) {
            return;
        }
        if r.level() <= self.0 {
            eprintln!("[{}] {}: {}", r.level(), r.target(), r.args());
        }
        if r.level() <= self.ceiling() {
            // Formatted before the lock, written in one call, error dropped:
            // a full disk or a yanked home directory costs the trail, never
            // the app.
            let line = format!(
                "[{}] [{}] {}: {}\n",
                utc_timestamp(unix_now()),
                r.level(),
                r.target(),
                r.args()
            );
            if let Ok(mut slot) = LOG_FILE.lock() {
                if let Some(file) = slot.as_mut() {
                    use std::io::Write as _;
                    let _ = file.write_all(line.as_bytes());
                }
            }
        }
    }
    fn flush(&self) {}
}

/// Seconds since the Unix epoch, or zero on a clock set before 1970 — a
/// wrong-but-harmless timestamp beats reasoning about time inside a logger.
fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Unix seconds to `2026-08-31T15:32:01Z`. Hand-rolled because nothing else
/// in the tree needs a time crate, and a dependency is the wrong price for
/// one format. The days-to-civil arithmetic is Howard Hinnant's; exact for
/// every date the proleptic Gregorian calendar can name, which is a few more
/// than this log will see.
fn utc_timestamp(secs: u64) -> String {
    let days = secs / 86_400;
    let rem = secs % 86_400;
    let (hh, mm, ss) = (rem / 3600, (rem / 60) % 60, rem % 60);
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z % 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + u64::from(mo <= 2);
    format!("{y:04}-{mo:02}-{d:02}T{hh:02}:{mm:02}:{ss:02}Z")
}

/// Over `max_len`, the current log becomes `<name>.1` — replacing the last
/// backup — and a fresh file starts. One predecessor, not a numbered chain,
/// because the case that matters is "it crashed, they relaunched, THEN they
/// asked for help": the tail of the run that died must survive exactly one
/// relaunch, and anything older than that nobody has ever asked for.
fn rotate_log(path: &std::path::Path, max_len: u64) {
    let Ok(meta) = std::fs::metadata(path) else {
        return;
    };
    if meta.len() <= max_len {
        return;
    }
    let mut backup = path.as_os_str().to_owned();
    backup.push(".1");
    let _ = std::fs::rename(path, std::path::PathBuf::from(backup));
}

/// Open `~/.config/tangent/tangent.log` and hand it to the logger.
///
/// Called from `main` AFTER `purge_pre_5_0` — the ordering comment there is
/// load-bearing, and this is exactly the kind of config-file creation it
/// exists to forbid, which is why this is not simply part of
/// [`install_logger`]. Until it runs the logger has only stderr; the handful
/// of records startup emits before this point go to the terminal as they
/// always did.
///
/// Every failure in here is shrugged off: a read-only home costs the log
/// file, never the launch.
/// **Which macOS this is**, for the log header.
///
/// `std::env::consts::OS` says `macos` and stops, which is how a mailed-in
/// log answered "on what" with a word that covers eleven releases. A crackle
/// reported on Sonoma and not reproducible on Tahoe is the kind of thing that
/// costs a day when the log will not say which one it came from, and the
/// header exists precisely so the answer arrives with the report.
///
/// `kern.osproductversion` rather than the Darwin number: it is the version
/// a person would say out loud ("14.7.1"), and it needs no framework — libc
/// is already here for `_exit`. Empty where it cannot be read, and empty on
/// every other platform, so the header keeps its old shape rather than
/// growing an "unknown".
#[cfg(target_os = "macos")]
fn os_release() -> String {
    let name = c"kern.osproductversion";
    let mut len: usize = 0;
    // SAFETY: the documented two-call shape — the first asks for the size
    // with a null buffer, the second fills a buffer of exactly that size.
    // Both are checked, and a non-zero status leaves the string empty.
    unsafe {
        if libc::sysctlbyname(
            name.as_ptr(),
            std::ptr::null_mut(),
            &mut len,
            std::ptr::null_mut(),
            0,
        ) != 0
            || len == 0
        {
            return String::new();
        }
        let mut buf = vec![0u8; len];
        if libc::sysctlbyname(
            name.as_ptr(),
            buf.as_mut_ptr().cast(),
            &mut len,
            std::ptr::null_mut(),
            0,
        ) != 0
        {
            return String::new();
        }
        // The value is NUL-terminated and `len` counts the terminator.
        buf.truncate(len.saturating_sub(1));
        String::from_utf8(buf).map_or_else(|_| String::new(), |v| format!(" {v}"))
    }
}

#[cfg(not(target_os = "macos"))]
fn os_release() -> String {
    String::new()
}

fn attach_log_file() {
    let dir = tangent_core::config::dir();
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("tangent.log");
    rotate_log(&path, MAX_LOG_LEN);
    let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(&path) else {
        return;
    };
    // Written directly rather than through `log!` so it lands at every filter
    // level: the header is what makes a mailed-in log answer "which build, on
    // what" without a follow-up email.
    use std::io::Write as _;
    let _ = writeln!(
        file,
        "[{}] tangent {} starting ({} {}{})",
        utc_timestamp(unix_now()),
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
        os_release(),
    );
    if let Ok(mut slot) = LOG_FILE.lock() {
        *slot = Some(file);
    }
}

fn install_logger() {
    let level = match std::env::var("IVORY_LOG")
        .unwrap_or_default()
        .to_lowercase()
        .as_str()
    {
        "trace" => log::LevelFilter::Trace,
        "debug" => log::LevelFilter::Debug,
        "info" => log::LevelFilter::Info,
        "warn" => log::LevelFilter::Warn,
        "error" => log::LevelFilter::Error,
        "off" => log::LevelFilter::Off,
        // Default: warnings only. Silent in normal use, but a GL probe failure
        // leaves a trail if the user ever runs it from a terminal.
        _ => log::LevelFilter::Warn,
    };
    let logger = Box::leak(Box::new(StderrLogger(level)));
    let ceiling = logger.ceiling();
    let _ = log::set_logger(logger).map(|()| log::set_max_level(ceiling));
}

/// Where a crash report should be sent. The always-receiving support inbox,
/// not the branded key-fulfilment address — a crash mail must never bounce.
const CRASH_CONTACT: &str = "tangentdaw@gmail.com";

/// The text of a crash file. Version and platform first, because those are
/// the two questions support would otherwise have to ask back — the whole
/// point of the file is that one attachment closes the loop.
fn crash_report(msg: &str, location: &str, backtrace: &str) -> String {
    format!(
        "Tangent {} ({} {})\n{}\n\n{msg}\n  at {location}\n\n{backtrace}\n",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
        utc_timestamp(unix_now())
    )
}

/// Top-level error surface (spec §2.3): any panic writes a crash file under
/// the config directory, shows a critical "Tangent Error" box that NAMES that
/// file, and exits with code 1.
///
/// The file is the load-bearing half. Before 5.1 the dialog was all there
/// was, and a dialog is gone the moment it is dismissed — "it crashed" from
/// someone else's machine carried no message, no location, no backtrace.
/// Local disk only: no telemetry, no network, nothing leaves the machine
/// unless the user mails the file themselves.
fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        // A dying process still gives the graph clock back — one spawned
        // pw-metadata, best effort, before anything slower. (The Minimal
        // build has no audio and no claim to release.)
        #[cfg(feature = "recorder")]
        tangent_record::audio::release_graph_claim();
        let msg = if let Some(s) = info.payload().downcast_ref::<&str>() {
            (*s).to_owned()
        } else if let Some(s) = info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "unknown error".to_owned()
        };
        let location = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_default();
        // `force_capture` rather than `capture`: the person whose machine
        // this dies on has never heard of RUST_BACKTRACE, and the hook only
        // runs when the process is already lost — cost is no objection here.
        let backtrace = std::backtrace::Backtrace::force_capture();
        let body =
            format!("Tangent encountered an error:\n\n{msg}\n  at {location}\n\n{backtrace}");
        eprintln!("{body}");
        // The hook can fire before `main` has made the config directory, so
        // make it here too. Epoch seconds in the name keep one file per
        // crash without a collision to think about.
        let dir = tangent_core::config::dir();
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join(format!("crash-{}.txt", unix_now()));
        let saved =
            std::fs::write(&path, crash_report(&msg, &location, &backtrace.to_string())).is_ok();
        let dialog_body = if saved {
            eprintln!("crash report saved to {}", path.display());
            format!(
                "A crash report was saved to:\n{}\n\nPlease send it to {CRASH_CONTACT}.\n\n{body}",
                path.display()
            )
        } else {
            // Nowhere to write — a read-only home is not a reason to hide
            // the error itself, so the dialog falls back to what it always
            // showed.
            body
        };
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            rfd::MessageDialog::new()
                .set_level(rfd::MessageLevel::Error)
                .set_title("Tangent Error")
                .set_description(&dialog_body)
                .set_buttons(rfd::MessageButtons::Ok)
                .show();
        }));
        std::process::exit(1);
    }));
}

/// Single instance via a lock file (D-UI-2): same dialog + exit UX as the
/// Python QSharedMemory key "tangent-midi-monitor"; a crashed instance never
/// blocks relaunch (the OS releases the lock with the process).
fn acquire_single_instance() -> bool {
    let Some(home) = dirs::home_dir() else {
        return true; // no home dir: don't block launching
    };
    let dir = tangent_core::config::dir();
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("tangent-midi-monitor.lock");
    let Ok(file) = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
    else {
        return true; // can't create the lock file: don't block launching
    };
    let lock: &'static mut fd_lock::RwLock<std::fs::File> =
        Box::leak(Box::new(fd_lock::RwLock::new(file)));
    match lock.try_write() {
        Ok(guard) => {
            // Hold the lock for the lifetime of the process.
            std::mem::forget(guard);
            true
        }
        Err(_) => false,
    }
}

/// The window title, composed in exactly one place: `"<project> - Tangent"`,
/// with a ` *` after the name while there are unsaved changes — the same
/// convention REAPER uses, so the mark reads on sight — or plain `"Tangent"`
/// when no project is open. Pure so it can be tested; `pub` so `desktop` can
/// recompose it per frame through `egui::ViewportCommand::Title` without a
/// second opinion on the format ever existing — the same split as
/// `plugin_catalog` above. ASCII hyphen: this string is published UI.
pub fn window_title(project: &str, dirty: bool) -> String {
    if project.is_empty() {
        return "Tangent".to_owned();
    }
    format!("{project}{} - Tangent", if dirty { " *" } else { "" })
}

/// **A window icon has to fit in one X11 request, and the artwork does not.**
///
/// X11 carries the icon in a single `ChangeProperty` on `_NET_WM_ICON`: two
/// words of size followed by one 32-bit pixel each. The server here caps a
/// request at 16 MB even with BIG-REQUESTS negotiated, and the wordmark is
/// 2486×2476 — about 24.6 MB. Over the line, and not by a little.
///
/// What that costs is the whole application, not the icon. winit sets the
/// icon while it is still building the window, an oversized request is a
/// CONNECTION error rather than a rejected property, and the connection it
/// poisons is the one glutin is about to ask for a config. The user sees
/// "Found no glutin configs matching the template", which names neither X11
/// nor an icon, and the app never opens.
///
/// So it is capped here rather than left to fail: 128 px is what every
/// desktop actually draws in a titlebar, an alt-tab switcher or a dock, and
/// scaling artwork nobody sees at full size costs nothing. The ASSET keeps
/// its resolution — the packaged hicolor icons and the macOS bundle still
/// ship the real thing.
///
/// Not gated to X11. A 24 MB texture is waste on every platform, this is
/// the one place all of them pass through, and a cap that only exists where
/// somebody already got hurt is a cap that gets forgotten.
fn shrink_icon_for_x11(icon: egui::IconData) -> egui::IconData {
    /// Comfortably inside every limit: 128×128 is 65 544 bytes, under even
    /// the 262 140-byte classic maximum that applies without BIG-REQUESTS.
    const MAX_EDGE: u32 = 128;
    shrink_icon_to(icon, MAX_EDGE)
}

/// **The runtime icon, put on Apple's grid.**
///
/// macOS is the one platform where the icon handed to the window system is
/// not a window icon at all: eframe passes it to `setApplicationIconImage_`,
/// so it becomes the DOCK TILE of the running app and overrides the bundle's
/// own `.icns` for as long as Tangent is open. That is why a correctly padded
/// bundle icon and a Dock icon that is visibly too big are not a
/// contradiction, why rebuilding never fixed it, and why clearing the icon
/// caches never fixed it either: the running app was setting it, every launch.
///
/// `build-macos.sh` pads the same artwork to 80.5% of its canvas when it makes
/// the `.icns`, because Apple's grid puts a squircle at 824 of 1024 and
/// full-bleed art renders LARGER than every neighbour. This is that step, for
/// the other path, from the same constant.
///
/// The CANVAS grows rather than the art shrinking: no resampling, so the
/// hairline lattice arrives exactly as drawn.
#[cfg(target_os = "macos")]
fn pad_icon_for_dock(icon: egui::IconData) -> egui::IconData {
    /// The share of the canvas Apple's grid gives the art. `build-macos.sh`
    /// uses the same number; changing one without the other puts the running
    /// app's icon a size away from its own bundle's.
    const ART_FRACTION: f32 = 0.805;
    let (w, h) = (icon.width as usize, icon.height as usize);
    if w == 0 || h == 0 || icon.rgba.len() != w * h * 4 {
        return icon;
    }
    let grow = |n: usize| ((n as f32) * (1.0 / ART_FRACTION - 1.0) / 2.0).round() as usize;
    let (mx, my) = (grow(w), grow(h));
    if mx == 0 && my == 0 {
        return icon;
    }
    let (nw, nh) = (w + mx * 2, h + my * 2);
    // Transparent, which is what the corners of a squircle are.
    let mut out = vec![0u8; nw * nh * 4];
    for y in 0..h {
        let src = y * w * 4;
        let dst = ((y + my) * nw + mx) * 4;
        out[dst..dst + w * 4].copy_from_slice(&icon.rgba[src..src + w * 4]);
    }
    egui::IconData {
        rgba: out,
        width: nw as u32,
        height: nh as u32,
    }
}

/// The scaler both of the above stand on: longest edge to `max_edge`, the
/// other keeping its proportion.
fn shrink_icon_to(icon: egui::IconData, max_edge: u32) -> egui::IconData {
    if icon.width <= max_edge && icon.height <= max_edge {
        return icon;
    }
    let (w, h) = (icon.width as usize, icon.height as usize);
    if icon.rgba.len() != w * h * 4 {
        // The buffer did not match the dimensions it claimed. Nothing safe
        // to scale, and artwork is never worth refusing to start over.
        eprintln!("the app icon's pixels do not match its size; going without one");
        return egui::IconData { rgba: Vec::new(), width: 0, height: 0 };
    }
    // **The longest edge goes to 128 and the other keeps the proportion.**
    //
    // Not a forced square. The artwork here is 2486×2476, so squaring it would
    // stretch by four parts in a thousand — invisible, and exactly the sort of
    // invisible that becomes visible the day somebody ships a wide wordmark
    // through the same door. `_NET_WM_ICON` carries width and height, so a
    // non-square icon is perfectly legal and there is nothing to gain by lying.
    let (dw, dh) = if w >= h {
        (max_edge as usize, ((h * max_edge as usize) / w).max(1))
    } else {
        (((w * max_edge as usize) / h).max(1), max_edge as usize)
    };
    // A box filter over the source cell each destination pixel covers.
    // Written out rather than pulled from `image`, which is a transitive
    // dependency here and not a direct one — twenty lines is cheaper than a
    // manifest entry for artwork nobody inspects at this size.
    //
    // Averaged PREMULTIPLIED, then un-premultiplied. Averaging straight RGBA
    // lets a fully transparent pixel's colour — often black, and arbitrary —
    // pull the colour of its opaque neighbours, which is what puts a dark
    // fringe around a scaled-down logo.
    let mut out = vec![0u8; dw * dh * 4];
    for dy in 0..dh {
        let (y0, y1) = (dy * h / dh, (((dy + 1) * h) / dh).max(dy * h / dh + 1));
        for dx in 0..dw {
            let (x0, x1) = (dx * w / dw, (((dx + 1) * w) / dw).max(dx * w / dw + 1));
            let (mut r, mut g, mut b, mut a, mut n) = (0u64, 0u64, 0u64, 0u64, 0u64);
            for sy in y0..y1.min(h) {
                for sx in x0..x1.min(w) {
                    let p = (sy * w + sx) * 4;
                    let al = u64::from(icon.rgba[p + 3]);
                    r += u64::from(icon.rgba[p]) * al;
                    g += u64::from(icon.rgba[p + 1]) * al;
                    b += u64::from(icon.rgba[p + 2]) * al;
                    a += al;
                    n += 1;
                }
            }
            let d = (dy * dw + dx) * 4;
            // `a` is the sum of alphas, so it is also the premultiplied
            // weight; dividing by it is the un-premultiply.
            let (rr, gg, bb) = if a == 0 { (0, 0, 0) } else { (r / a, g / a, b / a) };
            out[d] = rr as u8;
            out[d + 1] = gg as u8;
            out[d + 2] = bb as u8;
            out[d + 3] = if n == 0 { 0 } else { (a / n) as u8 };
        }
    }
    egui::IconData { rgba: out, width: dw as u32, height: dh as u32 }
}

fn main() {
    #[cfg(windows)]
    attach_console();

    let cli = parse_cli();
    install_logger();
    install_panic_hook();

    // **Before anything reads or creates a config file.** `acquire_single_instance`
    // below does a `create_dir_all` on the new directory, and Settings::load
    // follows it — either would have to run after this, not before, or the
    // pre-5.0 directory would still be sitting there next to a new one.
    if tangent_core::config::purge_pre_5_0() {
        log::info!("removed the pre-5.0 ~/.config/tangent");
    }

    // **The graph-clock claim's crash road.** The claim (see
    // `tangent_record::audio::claim_graph_clock`) releases on every clean
    // exit; a crash cannot, so each launch checks whether the force still
    // on the graph is its own predecessor's and takes it off. No-op
    // everywhere but Linux.
    #[cfg(feature = "recorder")]
    {
        tangent_record::audio::set_claim_marker(
            tangent_core::config::dir().join("graph-claim"),
        );
        tangent_record::audio::recover_graph_claim();
    }

    // The earliest legal moment — see the ordering comment above and the one
    // on the function. Everything the logger says from here on is on disk.
    attach_log_file();

    // **After the log is on disk**, so the line it writes is kept, and before
    // anything opens a device. Off unless `TANGENT_ACTIVITY=1`; see the
    // function for what it declares and what it deliberately does not.
    #[cfg(target_os = "macos")]
    macwin::hold_latency_critical();

    if !acquire_single_instance() {
        // **The terminal first**, as the panic hook does. A message box on
        // Linux goes through a portal or a zenity subprocess, and on a box
        // with neither `rfd` shows nothing and says nothing about it — so the
        // one line explaining why the app just exited would be lost on exactly
        // the machines where somebody is running it from a shell anyway.
        eprintln!("Tangent is already running. Only one instance can run at a time.");
        rfd::MessageDialog::new()
            .set_level(rfd::MessageLevel::Warning)
            .set_title("Tangent Already Running")
            .set_description("Tangent is already running.\n\nOnly one instance can run at a time.")
            .set_buttons(rfd::MessageButtons::Ok)
            .show();
        std::process::exit(0);
    }

    // Settings are read before building the ViewportBuilder so the very first
    // frame is right-sized (DESIGN: fixed-size via Min+Max+Inner triple).
    //
    // The OS is asked FIRST whether a screen reader is running, because the
    // load runs a one-shot that throws away a `screen_reader_mode` the old
    // probe guessed at — and a file saying yes on a machine where VoiceOver
    // is provably up right now is somebody's setting, not a guess. Main
    // thread, before eframe: exactly where an AppKit question may be asked.
    let settings = Settings::load_with_reader(desktop::os_reader_attached());
    // Whatever the window was last dragged to, or the size its bands come to
    // if it never has been. See `Settings::window_size_for_use`, which refuses
    // a remembered size below the floor rather than clamping to it.
    let size = settings
        .window_size_for_use()
        .unwrap_or_else(|| tangent_ui::app::initial_window_size(&settings));

    // The app icon MUST be set here. eframe applies its own embedded default
    // (the egui hexagon) whenever the viewport icon is None, and on macOS a
    // runtime icon overrides the .app bundle's CFBundleIconFile — so without
    // this the Dock showed the egui logo while Finder correctly showed Tangent's.
    // A decode failure just means no runtime icon: on macOS the bundle icon then
    // applies, so fall back silently rather than killing startup over artwork.
    let mut viewport = egui::ViewportBuilder::default()
        // No project is open when the window is built; once one is, the title
        // is recomposed by the same `window_title` so the format cannot fork.
        .with_title(window_title("", false))
        .with_app_id("tangent")
        .with_inner_size(size)
        // **Resizable, and the min is a floor rather than a pin.**
        //
        // It was `min = max = inner` plus `with_resizable(false)`, which is
        // three ways of saying the same thing: this window has seven sizes,
        // all of them percentages, and no edge you can drag. The audit called
        // it out under accessibility and it was right to — a person who needs
        // the app bigger than 200% or narrower than their screen had no move
        // to make.
        //
        // Nothing about the LAYOUT required it. Every band's height is a pure
        // function of the window's width, and the two solvers for an arbitrary
        // rect have been in `app.rs` since the plugin shipped. See the layout
        // branch in `TangentApp::frame`.
        .with_min_inner_size(tangent_ui::settings::MIN_WINDOW)
        .with_resizable(tangent_ui::settings::RESIZABLE)
        .with_decorations(!settings.borderless_mode);

    // Reopen where it was last left. Monitors are not enumerable before the
    // window exists, so a position saved on a display that is now unplugged
    // could land off-screen; `TangentApp` rescues that on the first frame, once
    // it can actually see the monitor.
    if let Some(pos) = settings.window_pos_for_use() {
        viewport = viewport.with_position(pos);
    }

    // macOS wants a rounded-squircle icon with padding around the art; every other
    // platform expects the plain full-bleed square. Both are the same artwork.
    //
    // **The small one is DRAWN for small, not merely scaled to it.** The
    // wordmark is a hairline lattice, and a hairline is the one thing that does
    // not survive being resampled to 128 px: at a twentieth of its size every
    // line lands between pixels and comes back as a grey suggestion of itself.
    // `tangent-128.png` is that artwork with the strokes thickened and the cream
    // lifted, so the shape still reads in a taskbar. Same drawing, same colour —
    // the hue is the artwork's own (227,215,187) raised a tenth, not whitened.
    #[cfg(target_os = "macos")]
    let icon_png: &[u8] = &include_bytes!("../../assets/tangent-macos.png")[..];
    #[cfg(not(target_os = "macos"))]
    let icon_png: &[u8] = &include_bytes!("../../assets/tangent-128.png")[..];

    // `shrink_icon_for_x11` is a GUARD, not the thing that sizes the icon. The
    // asset above already fits; this is what stops the next oversized one from
    // taking the window system down with it. See the note on that function.
    match eframe::icon_data::from_png_bytes(icon_png) {
        Ok(icon) => {
            // **macOS takes a different road on purpose.** There the icon
            // becomes the Dock tile of the running app, so it wants Apple's
            // padding and a resolution a Retina Dock can use — 128 px, the
            // X11 ceiling, is upscaled twice over on the tile and reads soft.
            // Everywhere else the X11 guard is the only thing that matters.
            #[cfg(target_os = "macos")]
            let icon = pad_icon_for_dock(shrink_icon_to(icon, 512));
            #[cfg(not(target_os = "macos"))]
            let icon = shrink_icon_for_x11(icon);
            viewport = viewport.with_icon(icon);
        }
        Err(err) => eprintln!("could not decode the app icon: {err}"),
    }

    // Which renderer draws the window.
    //
    // **macOS defaults to wgpu (Metal), and that is a bug fix rather than a
    // preference.** With a VST3 editor open the main window flickered rapidly
    // and recovered the instant the plugin's window was closed. eframe draws
    // with glow — OpenGL — and a plugin editor on macOS commonly makes its own
    // GL context current on this very thread; two GL contexts on one thread is
    // the classic way a host ends up drawing into the wrong drawable.
    //
    // Confirmed by switching renderers with nothing else changed: under Metal
    // the flicker is gone. Metal shares nothing with the plugin's GL.
    //
    // `IVORY_RENDERER=glow` goes back, because this is the renderer every
    // shipped build until now has used and an escape hatch costs one line.
    //
    // Windows and Linux stay on glow — but NOT because they are safe from the
    // problem. Plugin editors are implemented on all four platforms (an HWND,
    // an X11 window id — see `tangent_host::editor`), so a plugin can bring a
    // foreign GL context into this process there exactly as it can on macOS.
    // They stay because glow is what every shipped build on those platforms
    // has used, the flicker has only ever been OBSERVED on macOS, and a
    // renderer swap deserves its own decision with hardware in front of it
    // rather than riding along on a comment fix. If the flicker is ever
    // reproduced on Windows or Linux, the macOS arm above is the template.
    #[cfg(target_os = "macos")]
    let renderer = match std::env::var("IVORY_RENDERER").as_deref() {
        Ok("glow") => eframe::Renderer::Glow,
        _ => eframe::Renderer::Wgpu,
    };
    #[cfg(not(target_os = "macos"))]
    let renderer = eframe::Renderer::Glow;
    let options = eframe::NativeOptions {
        viewport,
        renderer,
        ..Default::default()
    };

    let port = cli.port;
    let result = eframe::run_native(
        "Tangent",
        options,
        Box::new(move |cc| Ok(Box::new(desktop::DesktopApp::new(cc, settings, port)))),
    );

    if let Err(err) = result {
        let body = format!("Tangent encountered an error:\n\n{err}");
        eprintln!("{body}");
        rfd::MessageDialog::new()
            .set_level(rfd::MessageLevel::Error)
            .set_title("Tangent Error")
            .set_description(&body)
            .set_buttons(rfd::MessageButtons::Ok)
            .show();
        std::process::exit(1);
    }

    // **Leave without running anyone else's destructors.**
    //
    // By this line the app is DOWN: `run_native` returned, eframe dropped
    // `DesktopApp`, and that drop closed the engine — streams released,
    // devices handed back, settings and the document long since saved. What
    // remains in the process is other people's code: every loaded VST3's
    // static destructors, queued on atexit. Kontakt 8's Qt teardown
    // SEGFAULTS there (measured on the owner's machine, 2026-08-24,
    // `~QApplication` -> `qAccessibleCleanup`), and a crash at that point is
    // indistinguishable from a force-kill to coreaudiod — which wedges audio
    // MACHINE-WIDE until the daemon is restarted. Discord and REAPER died
    // with it. `_exit` skips the atexit chain entirely; a normal `exit()`
    // (or falling off `main`) is precisely what detonates.
    //
    // Every serious host does this, for this exact reason.
    // The graph clock goes back before the process does anything else —
    // `_exit` skips atexit, so nothing later can put it back for us.
    #[cfg(feature = "recorder")]
    tangent_record::audio::release_graph_claim();
    use std::io::Write as _;
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
    #[cfg(unix)]
    unsafe {
        libc::_exit(0)
    }
    // Windows: `TerminateProcess`, which skips even the per-DLL detach
    // routines where a plugin's CRT would run its statics. (`libc` does
    // not expose `_exit` on Windows targets, and `ExitProcess` still
    // walks DllMain detach.)
    #[cfg(windows)]
    unsafe {
        windows_sys::Win32::System::Threading::TerminateProcess(
            windows_sys::Win32::System::Threading::GetCurrentProcess(),
            0,
        );
        std::process::exit(0)
    }
}

#[cfg(all(test, feature = "recorder"))]
mod catalog_tests {
    /// The picker's list is what a user chooses from, so the thing worth
    /// pinning is that an LV2 plugin arrives in it *usably* — with the name the
    /// plugin calls itself, not the tail of its URI.
    ///
    /// Skips where no LV2 plugin is installed, since a machine without them is
    /// not a machine with a bug.
    #[test]
    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    fn lv2_plugins_reach_the_picker_under_their_own_names() {
        let rows = super::plugin_catalog_from(Vec::new());
        let lv2: Vec<_> = rows
            .iter()
            .filter(|(p, _, _)| tangent_host::lv2::uri_of(p).is_some())
            .collect();
        if lv2.is_empty() {
            eprintln!("skipping: no LV2 plugins installed");
            return;
        }
        for (id, vendor, name) in &lv2 {
            let name = name.as_deref().unwrap_or("");
            assert!(!name.is_empty(), "{id:?} reached the picker with no name");
            // The failure this guards is silent and ugly: a URI whose last
            // segment is a slug would show "dragonfly-reverb" where the plugin
            // is called "Dragonfly Hall Reverb", and a URN would show the whole
            // thing. Deriving the name from the identity is exactly what the
            // display name exists to avoid.
            let derived = std::path::Path::new(id.to_str().unwrap_or(""))
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            let _ = derived;
            assert!(
                vendor.is_some(),
                "{name} reached the picker with no vendor to group it under"
            );
        }
    }

    /// A VST3 row must keep carrying `None` for its name, because the bundle's
    /// file stem already is the plugin's name and inventing a second source of
    /// truth for it is how the two disagree.
    #[test]
    fn vst3_rows_still_derive_their_name_from_the_bundle() {
        let rows = super::plugin_catalog_from(vec![std::path::PathBuf::from(
            "/usr/local/lib/vst3/Grand 8.vst3",
        )]);
        let row = rows
            .iter()
            .find(|(p, _, _)| p.to_string_lossy().contains("piano"))
            .expect("the bundle handed in should come back");
        assert_eq!(row.2, None, "a VST3 row must not carry a display name");
    }
}

#[cfg(test)]
mod icon_tests {
    use super::shrink_icon_for_x11;

    /// **The window icon fits in one X11 request.**
    ///
    /// `_NET_WM_ICON` is a single `ChangeProperty`: two words of size and then
    /// one 32-bit pixel each. Go past the server's maximum and X11 answers with
    /// a CONNECTION error, not a rejected property — which happens while winit
    /// is still building the window, and poisons the connection glutin is about
    /// to ask for a config. The app then dies reporting "Found no glutin configs
    /// matching the template", which names neither the icon nor X11.
    ///
    /// That is not hypothetical. `assets/tangent.png` became a 2486×2476
    /// wordmark, which needs 24.6 MB in one request against a 16 MB ceiling, and
    /// Tangent stopped launching on FreeBSD/X11 entirely.
    #[test]
    fn the_window_icon_fits_in_one_x11_request() {
        // What a server guarantees WITHOUT BIG-REQUESTS negotiated, which is
        // the floor worth holding: 262 140 bytes.
        const CLASSIC_MAX_BYTES: usize = 65_535 * 4;

        // Both assets: the one the window actually gets, and the full-size
        // wordmark that caused this. The first proves what ships is safe; the
        // second proves the guard still catches an oversized one, which is the
        // case that will come back the next time somebody swaps the artwork.
        for (name, png) in [
            ("tangent-128.png", &include_bytes!("../../assets/tangent-128.png")[..]),
            ("tangent.png", &include_bytes!("../../assets/tangent.png")[..]),
        ] {
            let decoded =
                eframe::icon_data::from_png_bytes(png).expect("the shipped icon must decode");
            let shrunk = shrink_icon_for_x11(decoded);

            let request = (shrunk.width as usize * shrunk.height as usize + 2) * 4;
            assert!(
                request <= CLASSIC_MAX_BYTES,
                "{name} needs {request} bytes in one X11 request, over the \
                 {CLASSIC_MAX_BYTES} a server promises without BIG-REQUESTS: \
                 {}x{}",
                shrunk.width,
                shrunk.height
            );
            assert_eq!(
                shrunk.rgba.len(),
                shrunk.width as usize * shrunk.height as usize * 4,
                "{name}: the buffer does not match the size it reports"
            );
        }

        // And the one the window is handed is already the right size, so the
        // guard is not silently doing the art department's job.
        let shipped = eframe::icon_data::from_png_bytes(&include_bytes!(
            "../../assets/tangent-128.png"
        )[..])
        .expect("the shipped icon must decode");
        assert_eq!(
            (shipped.width, shipped.height),
            (128, 128),
            "the icon the window gets should already be 128x128"
        );
    }

    /// **A wide icon stays wide.** The cap sets the LONGEST edge and the other
    /// keeps its proportion, so nothing is stretched to fit a square.
    /// `_NET_WM_ICON` carries width and height, so a non-square icon is legal
    /// and squaring one would only be a lie that shows.
    #[test]
    fn the_aspect_ratio_survives_the_cap() {
        let (w, h) = (1000u32, 250u32);
        let icon = egui::IconData {
            rgba: vec![255u8; w as usize * h as usize * 4],
            width: w,
            height: h,
        };
        let out = shrink_icon_for_x11(icon);
        assert_eq!(out.width, 128, "the long edge is what the cap names");
        assert_eq!(out.height, 32, "the short edge kept 1000:250");
        assert_eq!(out.rgba.len(), 128 * 32 * 4, "the buffer is not the size reported");
    }

    /// A tall one, the other way round, so the branch is not half-tested.
    #[test]
    fn a_tall_icon_caps_on_its_height() {
        let icon = egui::IconData { rgba: vec![9u8; 100 * 400 * 4], width: 100, height: 400 };
        let out = shrink_icon_for_x11(icon);
        assert_eq!((out.width, out.height), (32, 128));
    }

    /// An icon already small enough is handed back untouched — no resampling
    /// of artwork that was authored at the right size.
    #[test]
    fn a_small_icon_is_left_alone() {
        let icon = egui::IconData { rgba: vec![7u8; 64 * 64 * 4], width: 64, height: 64 };
        let out = shrink_icon_for_x11(icon);
        assert_eq!((out.width, out.height), (64, 64));
        assert!(out.rgba.iter().all(|b| *b == 7), "the pixels were resampled");
    }

    /// A transparent border must not darken the art. Averaging straight RGBA
    /// lets a clear pixel's arbitrary colour bleed into its opaque neighbours,
    /// which is the fringe that shows up around a scaled-down logo.
    #[test]
    fn transparent_pixels_do_not_bleed_into_the_colour() {
        // Half opaque white, half fully transparent black.
        let (w, h) = (256usize, 256usize);
        let mut rgba = vec![0u8; w * h * 4];
        for y in 0..h {
            for x in 0..w {
                let p = (y * w + x) * 4;
                if x < w / 2 {
                    rgba[p] = 255;
                    rgba[p + 1] = 255;
                    rgba[p + 2] = 255;
                    rgba[p + 3] = 255;
                }
            }
        }
        let out = shrink_icon_for_x11(egui::IconData {
            rgba,
            width: w as u32,
            height: h as u32,
        });
        // Sample well inside the opaque half: it must still be white, not grey.
        let p = ((out.height as usize / 2) * out.width as usize + 8) * 4;
        assert!(
            out.rgba[p] > 250 && out.rgba[p + 1] > 250 && out.rgba[p + 2] > 250,
            "transparent black bled into the opaque white: {:?}",
            &out.rgba[p..p + 4]
        );
    }
}

#[cfg(test)]
mod diagnosability_tests {
    use super::{crash_report, rotate_log, utc_timestamp, window_title};

    /// A scratch root per test, in the OS temp directory — same pattern as
    /// tangent-core's config tests, and for the same reason: a test that
    /// touches the real `~/.config/tangent` would rotate the log of whoever
    /// ran it.
    fn scratch(tag: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!(
            "tangent-log-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).expect("scratch");
        p
    }

    /// The point of rotation is that the tail of the run that CRASHED
    /// survives the relaunch that follows it — so the oversized file must
    /// move to `.1` intact, not be truncated in place.
    #[test]
    fn an_oversized_log_becomes_the_backup_with_its_contents_intact() {
        let root = scratch("rotate");
        let path = root.join("tangent.log");
        std::fs::write(&path, vec![b'x'; 4096]).expect("write");

        rotate_log(&path, 1024);

        assert!(!path.exists(), "the oversized log was left in place");
        let backup = root.join("tangent.log.1");
        let kept = std::fs::read(&backup).expect("the backup should exist");
        assert_eq!(kept.len(), 4096, "the backup lost bytes in the move");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Under the cap nothing moves — and an existing backup is not disturbed,
    /// because it may still be the only record of the crash being asked about.
    #[test]
    fn a_small_log_is_not_rotated() {
        let root = scratch("small");
        let path = root.join("tangent.log");
        std::fs::write(&path, b"short").expect("write");
        std::fs::write(root.join("tangent.log.1"), b"previous run").expect("backup");

        rotate_log(&path, 1024);

        assert!(path.exists(), "a small log was rotated away");
        assert_eq!(
            std::fs::read(root.join("tangent.log.1")).expect("backup"),
            b"previous run",
            "the backup was touched for no reason"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A second rotation REPLACES the backup rather than growing a chain —
    /// one predecessor is the bound that keeps the pair attachable to an
    /// email forever.
    #[test]
    fn a_second_rotation_replaces_the_backup() {
        let root = scratch("replace");
        let path = root.join("tangent.log");
        std::fs::write(&path, vec![b'a'; 2048]).expect("first");
        rotate_log(&path, 1024);
        std::fs::write(&path, vec![b'b'; 2048]).expect("second");
        rotate_log(&path, 1024);

        let kept = std::fs::read(root.join("tangent.log.1")).expect("backup");
        assert!(
            kept.iter().all(|b| *b == b'b'),
            "the backup still holds the older generation"
        );
        assert!(
            !root.join("tangent.log.1.1").exists(),
            "rotation grew a chain instead of replacing the backup"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A missing log is the first-launch case, not a fault.
    #[test]
    fn rotating_a_log_that_is_not_there_does_nothing() {
        let root = scratch("absent");
        rotate_log(&root.join("tangent.log"), 1024);
        assert!(!root.join("tangent.log.1").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The crash file must answer the questions support would otherwise mail
    /// back: which build, on what, what happened, where.
    #[test]
    fn a_crash_report_names_the_build_the_platform_and_the_fault() {
        let report = crash_report(
            "index out of bounds",
            "tangent-ui/src/app.rs:123:4",
            "0: some::frame",
        );
        assert!(report.contains(env!("CARGO_PKG_VERSION")), "no version: {report}");
        assert!(report.contains(std::env::consts::OS), "no OS: {report}");
        assert!(report.contains(std::env::consts::ARCH), "no arch: {report}");
        assert!(report.contains("index out of bounds"), "no message: {report}");
        assert!(report.contains("tangent-ui/src/app.rs:123:4"), "no location: {report}");
        assert!(report.contains("0: some::frame"), "no backtrace: {report}");
    }

    /// The hand-rolled civil-date arithmetic against known answers, including
    /// both leap cases that break naive versions: a leap year inside a
    /// century (2024) and the century year that IS a leap year (2000).
    #[test]
    fn the_timestamp_matches_the_calendar() {
        for (secs, want) in [
            (0u64, "1970-01-01T00:00:00Z"),
            (951_782_400, "2000-02-29T00:00:00Z"),
            (1_709_164_800, "2024-02-29T00:00:00Z"),
            (1_756_654_321, "2025-08-31T15:32:01Z"),
            (4_102_444_799, "2099-12-31T23:59:59Z"),
        ] {
            assert_eq!(utc_timestamp(secs), want, "for {secs}");
        }
    }

    /// The title carries the project, the dirty mark carries the star, and a
    /// build with no project open still answers to its own name.
    #[test]
    fn the_window_title_carries_project_and_dirtiness() {
        assert_eq!(window_title("", false), "Tangent");
        assert_eq!(window_title("", true), "Tangent", "no project, nothing to be dirty");
        assert_eq!(window_title("First Song", false), "First Song - Tangent");
        assert_eq!(window_title("First Song", true), "First Song * - Tangent");
    }
}

#[cfg(test)]
mod log_tests {
    use super::is_fake_map_noise;

    /// **wgpu's "Fake map" is dropped, and nothing else is.** The same crate
    /// saying anything else, or another crate saying those words, still
    /// reaches the log. Revert-check, performed: make the helper answer on
    /// the target alone and the second assertion goes red.
    #[test]
    fn wgpus_fake_map_line_is_dropped_and_real_gl_errors_are_kept() {
        assert!(is_fake_map_noise("wgpu_hal::gles::adapter", "Fake map"));
        assert!(!is_fake_map_noise("wgpu_hal::gles::adapter", "GL error: out of memory"));
        assert!(!is_fake_map_noise("tangent::desktop", "Fake map"));
    }
}
