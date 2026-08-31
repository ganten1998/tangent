//! **Where does a take's audio go?** — the app's capture drain, headless.
//!
//! Three video takes on the owner's Linux box dropped 22 %, 53 % and 47 % of
//! their audio through the ALSA path while JACK takes dropped none, every
//! manifest read `true_rate_hz` at exactly half nominal, and a writer-thread
//! trace showed the writer asleep in its own poll doing almost nothing wrong.
//! The first two runs of this instrument settled it in twenty seconds each:
//! the device delivered a steady 22,016 frames per second into a `Fixed(256)`
//! stream under a 512-frame `clock.min-quantum` — the graph hands the
//! `pipewire` PCM one quantum per cycle, the ring keeps what fits, and the
//! rest vanishes below every counter — and at `Fixed(512)` and `Fixed(1024)`
//! the same stream delivered every frame. The drops in the manifests were the
//! desk tap's ring overflowing at the halved drain pace, not the capture's;
//! the stamps were honest all along. See `ring_covering_graph` for the cure,
//! which the third run of this instrument validated at full rate.
//!
//! This instrument runs the app's OWN capture path — [`audio::open_input`],
//! [`CaptureSink::next_mark`], [`FrameCursor::plan`], [`CaptureSink::pop_frames`],
//! the same 4 ms poll — with nothing else attached: no desk, no meters, no
//! video, no UI. Once a second it reports what the device delivered, what the
//! drain consumed, what the ring holds, and how fast the stamps are moving
//! against the host clock. A fault that reproduces here is in the capture
//! machinery and can be bisected on this bench; one that does not lives in
//! what the app adds around it, and the knobs below add those back one at a
//! time.
//!
//! ```text
//! cargo run --release -p tangent-record --example takeprobe            # 15 s, buffer 256
//! cargo run --release -p tangent-record --example takeprobe -- 30 256
//! SYSTEM=JACK cargo run --release -p tangent-record --example takeprobe
//! LOAD=2   ...   # two spinning threads, standing in for the video encoder
//! STALL_MS=500 ...  # one deliberate writer stall at t=5 s, to check the
//!                   # accounting: the ring should absorb it and drain back
//! ```
//!
//! The two numbers to watch are `cap` — frames per second, which should be
//! the sample rate — and `stamp`, device nanoseconds per host nanosecond,
//! which should read 1.000. The halving fault reads `cap` at half rate with
//! `stamp` honest and every drop column zero: a stream that looks clean and
//! is not. `NODE=<pipewire node name>` opens a picked interface exactly as
//! the app does; without it the default input opens, which is a DIFFERENT
//! ALSA plugin and has already told a different truth once.
//!
//! **It opens one input and writes nothing.**

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tangent_record::audio::{self, ConfigWish, FrameCursor, InputSelection, Timebase};

/// The app's writer polls at this cadence; the probe must not poll harder, or
/// it hides a starvation the app would feel.
const POLL: Duration = Duration::from_millis(4);

fn main() {
    let mut args = std::env::args().skip(1);
    let secs: u64 = args.next().and_then(|a| a.parse().ok()).unwrap_or(15);
    let buffer: u32 = args.next().and_then(|a| a.parse().ok()).unwrap_or(256);
    let load: usize = env_num("LOAD") as usize;
    let stall_ms: u64 = env_num("STALL_MS");

    // The same host-selection door the app uses, so `SYSTEM=JACK` compares the
    // stacks and unset compares nothing.
    let system = std::env::var("SYSTEM").ok();
    audio::set_system(system.as_deref());

    // `NODE` opens a PipeWire node through the `pipewire` PCM, exactly as the
    // app opens a picked interface — which is a DIFFERENT ALSA plugin from the
    // `default` PCM the bare bench opens, with its own delivery cadence and its
    // own timestamps. The first bench run measured the wrong PCM and said
    // "healthy"; the knob exists so that cannot happen twice.
    let selection = match std::env::var("NODE") {
        Ok(n) if !n.is_empty() => InputSelection::PipewireNode(n),
        _ => InputSelection::Default,
    };

    // Spinning stand-ins for the video encoder: the take that drops audio is a
    // VIDEO take on a two-core machine, and cpu pressure is the difference
    // easiest to fake.
    let stop = Arc::new(AtomicBool::new(false));
    for _ in 0..load {
        let stop = Arc::clone(&stop);
        std::thread::spawn(move || {
            let mut x = 0u64;
            while !stop.load(Ordering::Relaxed) {
                x = x.wrapping_mul(6364136223846793005).wrapping_add(1);
            }
            std::hint::black_box(x);
        });
    }

    let timebase = Timebase::new();
    let wish = ConfigWish {
        buffer_frames: Some(buffer),
        ..ConfigWish::default()
    };
    // 3.0 seconds of ring, exactly as `Session::open_input` asks for it.
    let (input, mut sink, _monitor) =
        match audio::open_input(&selection, &wish, 3.0, timebase, None) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("open_input failed: {e}");
                std::process::exit(1);
            }
        };
    let cfg = input.config();
    println!(
        "host {} | {} - {} ch, {} Hz, {:?}, buffer {:?}",
        audio::system().unwrap_or_else(|| "default".into()),
        cfg.device,
        cfg.channels,
        cfg.sample_rate,
        cfg.sample_format,
        cfg.buffer_size,
    );
    let stats = Arc::clone(input.stats());

    // The app's take path, verbatim: marks drive the drain through the cursor,
    // and the popped audio is thrown away — in the app it is timing for the
    // desk's render; here the timing IS the experiment.
    let mut cursor = FrameCursor::new();
    let mut buf: Vec<f32> = Vec::new();

    println!(
        "{:>4} {:>5} {:>7} {:>7} {:>6} {:>7} {:>7} {:>6} {:>7}  {}",
        "t", "cb", "cap", "drop", "mkdrop", "planned", "popped", "short", "ring", "stamp"
    );

    let t0 = Instant::now();
    let mut stalled = false;
    // Per-second window accumulators, reset at each report.
    let (mut planned, mut popped, mut short): (u64, u64, u64) = (0, 0, 0);
    let mut window: Option<((u64, i64), (u64, i64))> = None; // first/last (device_ns, host_ns)
    let (mut last_cb, mut last_cap, mut last_drop, mut last_mkdrop) = (0u64, 0u64, 0u64, 0u64);
    let mut next_report = Duration::from_secs(1);

    while t0.elapsed() < Duration::from_secs(secs) {
        while let Some(mark) = sink.next_mark() {
            if let Some(d) = mark.device_ns {
                let point = (d, mark.host_ns);
                let (first, _) = window.get_or_insert((point, point));
                window = Some((*first, point));
            }
            let plan = cursor.plan(&mark);
            planned += u64::from(plan.frames);
            let got = if plan.frames > 0 {
                buf.clear();
                sink.pop_frames(plan.frames as usize, &mut buf)
            } else {
                0
            };
            popped += got as u64;
            short += u64::from(plan.frames) - got as u64;
        }

        if stall_ms > 0 && !stalled && t0.elapsed() >= Duration::from_secs(5) {
            stalled = true;
            std::thread::sleep(Duration::from_millis(stall_ms));
        }

        if t0.elapsed() >= next_report {
            let (cb, cap) = (stats.callbacks(), stats.frames_captured());
            let (drop, mkdrop) = (stats.frames_dropped(), stats.marks_dropped());
            let stamp = match window {
                Some(((d0, h0), (d1, h1))) if h1 > h0 => {
                    format!("{:.3}", (d1 - d0) as f64 / (h1 - h0) as f64)
                }
                _ => "-".into(),
            };
            println!(
                "{:>4} {:>5} {:>7} {:>7} {:>6} {:>7} {:>7} {:>6} {:>7}  {}",
                next_report.as_secs(),
                cb - last_cb,
                cap - last_cap,
                drop - last_drop,
                mkdrop - last_mkdrop,
                planned,
                popped,
                short,
                sink.frames_available(),
                stamp,
            );
            (last_cb, last_cap, last_drop, last_mkdrop) = (cb, cap, drop, mkdrop);
            (planned, popped, short) = (0, 0, 0);
            window = None;
            next_report += Duration::from_secs(1);
        }

        std::thread::sleep(POLL);
    }
    stop.store(true, Ordering::Relaxed);

    let dropped = stats.frames_dropped();
    let captured = stats.frames_captured();
    println!();
    println!(
        "  {captured} frames captured, {dropped} dropped ({:.1} %), {} marks dropped, {} stream errors",
        if captured > 0 {
            dropped as f64 * 100.0 / captured as f64
        } else {
            0.0
        },
        stats.marks_dropped(),
        stats.stream_errors(),
    );
    if let Some(f) = input.fault() {
        println!("  fault: {f}");
    }
}

/// A numeric env knob; unset or unparsable is 0, which turns the knob off.
fn env_num(name: &str) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
}
