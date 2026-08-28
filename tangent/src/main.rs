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
/// Two AppKit facts winit does not surface, and the hook that lets the OS
/// keep this aspect-locked layout on its curve DURING a drag. A target gate,
/// not a feature gate — the convention against `#[cfg(feature)]` outside this
/// crate is about features, and this is a platform.
#[cfg(target_os = "macos")]
mod macwin;
#[cfg(feature = "recorder")]
mod devices;
/// Six-operator FM, and real DX7 cartridges to feed it. The instrument the app
/// has when it has no instrument, so a fresh install makes a sound. The default
/// patch is written in the source: no samples, no data, no licence questions.
#[cfg(feature = "recorder")]
mod dx7;
/// Reverb and delay, on everything the app plays.
#[cfg(feature = "recorder")]
mod effects;
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
struct StderrLogger(log::LevelFilter);

impl log::Log for StderrLogger {
    fn enabled(&self, m: &log::Metadata<'_>) -> bool {
        m.level() <= self.0
    }
    fn log(&self, r: &log::Record<'_>) {
        if self.enabled(r.metadata()) {
            eprintln!("[{}] {}: {}", r.level(), r.target(), r.args());
        }
    }
    fn flush(&self) {}
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
    let _ = log::set_logger(logger).map(|()| log::set_max_level(level));
}

/// Top-level error surface (spec §2.3): any panic shows a critical "Tangent
/// Error" box with the message and a backtrace, then exits with code 1.
fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
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
        let backtrace = std::backtrace::Backtrace::force_capture();
        let body =
            format!("Tangent encountered an error:\n\n{msg}\n  at {location}\n\n{backtrace}");
        eprintln!("{body}");
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            rfd::MessageDialog::new()
                .set_level(rfd::MessageLevel::Error)
                .set_title("Tangent Error")
                .set_description(&body)
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
    let settings = Settings::load();
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
        .with_title("Tangent")
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
        // branch in `IvoryApp::frame`.
        .with_min_inner_size(tangent_ui::settings::MIN_WINDOW)
        .with_resizable(tangent_ui::settings::RESIZABLE)
        .with_decorations(!settings.borderless_mode);

    // Reopen where it was last left. Monitors are not enumerable before the
    // window exists, so a position saved on a display that is now unplugged
    // could land off-screen; `IvoryApp` rescues that on the first frame, once
    // it can actually see the monitor.
    if let Some(pos) = settings.window_pos_for_use() {
        viewport = viewport.with_position(pos);
    }

    // macOS wants a rounded-squircle icon with padding around the art; every other
    // platform expects the plain full-bleed square. Both are the same artwork.
    #[cfg(target_os = "macos")]
    let icon_png: &[u8] = &include_bytes!("../../assets/tangent-macos.png")[..];
    #[cfg(not(target_os = "macos"))]
    let icon_png: &[u8] = &include_bytes!("../../assets/tangent.png")[..];

    match eframe::icon_data::from_png_bytes(icon_png) {
        Ok(icon) => viewport = viewport.with_icon(icon),
        Err(err) => eprintln!("could not decode assets/tangent.png as the app icon: {err}"),
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
    // Windows and Linux stay on glow: their plugin editors are not implemented
    // (`EditorError::Unsupported`), so no foreign GL context is ever created in
    // this process, and wgpu is a large dependency tree to add for a problem
    // those platforms do not have yet. When their editors land, they inherit
    // this question.
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
