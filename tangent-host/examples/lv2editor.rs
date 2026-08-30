//! Open a real LV2 plugin's own editor and prove the whole round trip.
//!
//!   cargo run -p tangent-host --example lv2editor -- <plugin-uri> [seconds]
//!
//! Exists because "the UI embeds" is not something a unit test can answer: it
//! needs an X server, a real plugin and a window to be a child of. This is the
//! smallest thing that puts one on screen.
//!
//! It also closes the loop the app depends on, which is the part worth proving:
//! a knob moved in the plugin's OWN window produces an edit, that edit goes to
//! the DSP through `set_param`, and the AUDIO CHANGES. Those three are separate
//! pieces of machinery — a UI shared object, a host ring, a DSP shared object —
//! and each of them can work while the joins between them do not. Watching the
//! RMS move is the only way to see that the whole chain is connected, and it is
//! exactly what the user experiences as "the knob does something".
#[cfg(any(target_os = "linux", target_os = "freebsd"))]
fn main() {
    let uri = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "https://github.com/michaelwillis/dragonfly-reverb".into());
    let secs: u64 = std::env::args().nth(2).and_then(|s| s.parse().ok()).unwrap_or(8);
    const RATE: f64 = 48_000.0;
    const FRAMES: usize = 256;

    // The DSP, the same way the engine makes it.
    let mut inst = match tangent_host::lv2::Instance::create(&uri, RATE) {
        Ok(i) => i,
        Err(e) => {
            println!("could not instantiate {uri}: {e}");
            return;
        }
    };
    let info = tangent_host::lv2::discover()
        .into_iter()
        .find(|p| p.uri == uri);
    let (ins, outs) = info
        .as_ref()
        .map_or((2, 2), |i| (i.audio_in.max(1), i.audio_out.max(1)));

    // Through `Lv2Window`, which is the type the app uses - it owns the window
    // as well as the UI, so this is the whole door rather than half of it.
    let mut win = match tangent_host::lv2ui::Lv2Window::open(&uri, RATE as f32, "Tangent - LV2 editor")
    {
        Ok(w) => w,
        Err(e) => {
            println!("could not open the editor: {e}");
            return;
        }
    };
    println!(
        "editor open, {}x{}, window 0x{:x}",
        win.size().0,
        win.size().1,
        win.window()
    );
    println!("drag a control in the plugin's window; the RMS below is its output");

    // A quiet steady tone in, so a change in the plugin's settings shows up as
    // a change in the number. Silence would leave every reverb reading zero.
    let mut phase = 0.0f32;
    let mut input: Vec<Vec<f32>> = vec![vec![0.0; FRAMES]; ins];
    let mut output: Vec<Vec<f32>> = vec![vec![0.0; FRAMES]; outs];

    let start = std::time::Instant::now();
    let mut last_report = std::time::Instant::now();
    let mut moved = 0usize;
    while start.elapsed().as_secs() < secs {
        if win.closed() {
            println!("the window closed");
            break;
        }
        // **The join.** This is `Engine::poll_editor` in miniature: whatever the
        // user just turned, straight onto the plugin's control ports.
        for e in win.drain_edits() {
            println!("  UI moved port {} to {}", e.port, e.value);
            if let Err(err) = inst.set_param(e.port as usize, e.value) {
                println!("    the DSP refused it: {err}");
            }
            moved += 1;
        }
        for ch in input.iter_mut() {
            let mut p = phase;
            for s in ch.iter_mut() {
                *s = (p * std::f32::consts::TAU).sin() * 0.25;
                p = (p + 220.0 / RATE as f32).fract();
            }
        }
        phase = (phase + FRAMES as f32 * 220.0 / RATE as f32).fract();
        if let Err(e) = inst.process_planar(FRAMES, &input, &mut output) {
            println!("the plugin refused a block: {e}");
            break;
        }
        if last_report.elapsed().as_millis() >= 500 {
            let sum: f32 = output[0].iter().map(|s| s * s).sum();
            println!("  out RMS {:.5}", (sum / FRAMES as f32).sqrt());
            last_report = std::time::Instant::now();
        }
        std::thread::sleep(std::time::Duration::from_millis(16));
    }
    drop(win);
    println!("editor closed cleanly after {moved} edits");
}

#[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
fn main() {
    println!("LV2 editors are hosted only where LV2 is.");
}
