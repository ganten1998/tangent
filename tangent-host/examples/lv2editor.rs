//! Open a real LV2 plugin's own editor, through `tangent_host::lv2ui`.
//!
//!   cargo run -p tangent-host --example lv2editor -- <plugin-uri> [seconds]
//!
//! Exists because "the UI embeds" is not something a unit test can answer: it
//! needs an X server, a real plugin and a window to be a child of. This is the
//! smallest thing that puts one on screen.
#[cfg(any(target_os = "linux", target_os = "freebsd"))]
fn main() {
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::*;

    let uri = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "https://github.com/michaelwillis/dragonfly-reverb".into());
    let secs: u64 = std::env::args().nth(2).and_then(|s| s.parse().ok()).unwrap_or(8);

    let (conn, screen_num) = x11rb::connect(None).expect("no X display");
    let screen = &conn.setup().roots[screen_num];
    let win = conn.generate_id().expect("window id");
    conn.create_window(
        x11rb::COPY_DEPTH_FROM_PARENT, win, screen.root, 80, 80, 940, 400, 0,
        WindowClass::INPUT_OUTPUT, screen.root_visual,
        &CreateWindowAux::new().background_pixel(screen.black_pixel)
            .event_mask(EventMask::EXPOSURE | EventMask::STRUCTURE_NOTIFY),
    ).expect("create window");
    conn.map_window(win).expect("map");
    conn.flush().expect("flush");

    let mut ui = match tangent_host::lv2ui::Ui::open(&uri, win, 48_000.0) {
        Ok(Some(ui)) => ui,
        Ok(None) => { println!("{uri} has no X11 UI to embed"); return; }
        Err(e) => { println!("could not open the editor: {e}"); return; }
    };
    println!("editor open. plugin window 0x{:x} inside host window 0x{win:x}", ui.window());

    if ui.window() != 0 {
        let _ = conn.map_window(ui.window());
        let _ = conn.flush();
    }
    let start = std::time::Instant::now();
    while start.elapsed().as_secs() < secs {
        while let Ok(Some(_)) = conn.poll_for_event() {}
        if !ui.idle() { println!("the UI asked to close"); break; }
        for e in ui.drain_edits() {
            println!("  UI moved port {} to {}", e.port, e.value);
        }
        let _ = conn.flush();
        std::thread::sleep(std::time::Duration::from_millis(16));
    }
    drop(ui);
    println!("editor closed cleanly");
}

#[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
fn main() {
    println!("LV2 editors are hosted only where LV2 is.");
}
