//! **What the round trip actually costs, from the converter and back.**
//!
//! Tangent opens the interface as TWO cpal streams — an input and an output —
//! bridged by the monitor ring. A duplex host opens one, and its callback is
//! handed this block's input alongside the output it is about to fill. The
//! difference between those two shapes is the thing nobody had a number for,
//! and `mix_monitor`'s own comment says so:
//!
//! > This does not close the whole gap and cannot: two AudioUnits means an
//! > output callback can only ever see input that arrived before it started.
//!
//! This measures it, using CoreAudio's own timestamps rather than arithmetic:
//! the INPUT callback is told when the block it is holding was captured, the
//! OUTPUT callback is told when the block it is filling will be played, and
//! the ring in between says which captured frame this output block is about to
//! consume. Their difference is what a player hears, end to end, in software.
//!
//! It deliberately does NOT need a loopback cable. A cable would add the
//! converters (a fixed ~0.6 ms on this interface, identical for every host and
//! therefore not what the comparison is about); what is in dispute is the
//! software path, and that is exactly what the two timestamps bracket.
//!
//! ```text
//! cargo run --release -p tangent-record --example rtlatency -- 256 8
//! cargo run --release -p tangent-record --example rtlatency -- 128 8
//! ```
//!
//! Arguments are the OUTPUT buffer in frames and how many seconds to watch.
//! The input side follows Tangent's own macOS rule — a quarter of the output,
//! floor 64 — unless `IN_BUFFER` overrides it. `DEVICE` picks an interface by
//! substring; the default output and input devices are used otherwise.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{BufferSize, SampleFormat, StreamConfig};

/// Nanoseconds since the probe started, from one monotonic clock both
/// callbacks can read. `Instant` rather than a cpal `StreamInstant`, because
/// the two streams' instants are not documented to share an origin and this
/// comparison is the whole measurement.
struct Clock(Instant);
impl Clock {
    fn now_ns(&self) -> u64 {
        self.0.elapsed().as_nanos() as u64
    }
}

/// Everything the two callbacks say to each other. Atomics only: the audio
/// thread never locks here any more than it does in the app.
struct Shared {
    /// Frames the input has pushed, ever.
    pushed: AtomicU64,
    /// The frame index the most recent input block started at...
    block_start: AtomicU64,
    /// ...and when THAT frame was captured, per CoreAudio.
    block_capture_ns: AtomicU64,
    /// Round-trip samples, in nanoseconds: count, sum, min, max.
    n: AtomicU64,
    sum: AtomicU64,
    min: AtomicU64,
    max: AtomicU64,
    /// 0.25 ms buckets, so the shape is visible and not just the mean.
    hist: [AtomicU64; 128],
    /// Frames the stale-drop threw away, exactly as `mix_monitor` counts them.
    slipped: AtomicU64,
}

fn pick(host: &cpal::Host, want: Option<&str>, output: bool) -> Option<cpal::Device> {
    let all = if output {
        host.output_devices().ok()?.collect::<Vec<_>>()
    } else {
        host.input_devices().ok()?.collect::<Vec<_>>()
    };
    if let Some(w) = want {
        let w = w.to_lowercase();
        if let Some(d) = all
            .into_iter()
            .find(|d| d.name().map(|n| n.to_lowercase().contains(&w)).unwrap_or(false))
        {
            return Some(d);
        }
        return None;
    }
    if output {
        host.default_output_device()
    } else {
        host.default_input_device()
    }
}

fn main() {
    let out_buf: u32 = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(256);
    let secs: f64 = std::env::args().nth(2).and_then(|s| s.parse().ok()).unwrap_or(8.0);
    // Tangent's own macOS rule (`open_input`): the input runs at a quarter of
    // the output, floor 64, because its callback only copies into a ring.
    let in_buf: u32 = std::env::var("IN_BUFFER")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or_else(|| (out_buf / 4).max(64));
    let want_dev = std::env::var("DEVICE").ok();

    let host = cpal::default_host();
    let Some(out_dev) = pick(&host, want_dev.as_deref(), true) else {
        eprintln!("no output device");
        std::process::exit(1);
    };
    let Some(in_dev) = pick(&host, want_dev.as_deref(), false) else {
        eprintln!("no input device");
        std::process::exit(1);
    };
    let out_cfg = out_dev.default_output_config().expect("default output config");
    let in_cfg = in_dev.default_input_config().expect("default input config");
    let rate = out_cfg.sample_rate().0;
    assert_eq!(rate, in_cfg.sample_rate().0, "run both sides at one rate");
    let out_ch = out_cfg.channels() as usize;
    let in_ch = in_cfg.channels() as usize;

    println!(
        "output {:?}  {} ch @ {} Hz, asking {} frames\ninput  {:?}  {} ch, asking {} frames",
        out_dev.name().unwrap_or_default(),
        out_ch,
        rate,
        out_buf,
        in_dev.name().unwrap_or_default(),
        in_ch,
        in_buf
    );
    if out_cfg.sample_format() != SampleFormat::F32 || in_cfg.sample_format() != SampleFormat::F32 {
        eprintln!("this probe only speaks f32; the device offers something else");
        std::process::exit(1);
    }

    let clock = Arc::new(Clock(Instant::now()));
    let sh = Arc::new(Shared {
        pushed: AtomicU64::new(0),
        block_start: AtomicU64::new(0),
        block_capture_ns: AtomicU64::new(0),
        n: AtomicU64::new(0),
        sum: AtomicU64::new(0),
        min: AtomicU64::new(u64::MAX),
        max: AtomicU64::new(0),
        hist: std::array::from_fn(|_| AtomicU64::new(0)),
        slipped: AtomicU64::new(0),
    });
    // The monitor ring, sized as `open_input` sizes it: an eighth of a second.
    let (mut tx, mut rx) = rtrb::RingBuffer::<f32>::new((rate as usize / 8).max(1024));

    let ic = Arc::clone(&clock);
    let ish = Arc::clone(&sh);
    let in_stream = in_dev
        .build_input_stream(
            &StreamConfig {
                channels: in_cfg.channels(),
                sample_rate: in_cfg.sample_rate(),
                buffer_size: BufferSize::Fixed(in_buf),
            },
            move |data: &[f32], info: &cpal::InputCallbackInfo| {
                let now = ic.now_ns();
                let frames = data.len() / in_ch.max(1);
                // How long ago the FIRST frame of this block was captured,
                // per CoreAudio — not per our arithmetic.
                let ago = info
                    .timestamp()
                    .callback
                    .duration_since(&info.timestamp().capture)
                    .map(|d| d.as_nanos() as u64)
                    .unwrap_or(0);
                let start = ish.pushed.load(Ordering::Relaxed);
                ish.block_capture_ns
                    .store(now.saturating_sub(ago), Ordering::Relaxed);
                ish.block_start.store(start, Ordering::Relaxed);
                // One f32 per frame: this models the ring's DEPTH, which is
                // what the measurement is about, not its contents.
                for i in 0..frames {
                    let _ = tx.push(data[i * in_ch]);
                }
                ish.pushed.store(start + frames as u64, Ordering::Release);
            },
            |e| eprintln!("input stream error: {e}"),
            None,
        )
        .expect("build input stream");

    let oc = Arc::clone(&clock);
    let osh = Arc::clone(&sh);
    let out_stream = out_dev
        .build_output_stream(
            &StreamConfig {
                channels: out_cfg.channels(),
                sample_rate: out_cfg.sample_rate(),
                buffer_size: BufferSize::Fixed(out_buf),
            },
            move |data: &mut [f32], info: &cpal::OutputCallbackInfo| {
                let now = oc.now_ns();
                data.fill(0.0);
                let frames = data.len() / out_ch.max(1);
                // When the first frame of this block will be heard, per
                // CoreAudio. Exactly `Engine::output_delay_ns`.
                let ahead = info
                    .timestamp()
                    .playback
                    .duration_since(&info.timestamp().callback)
                    .map(|d| d.as_nanos() as u64)
                    .unwrap_or(0);

                // `mix_monitor`'s stale-drop, to the letter: what is
                // legitimately in flight is this output block plus the input
                // block the device is filling now. Anything beyond is history.
                let in_flight = frames + in_buf as usize;
                let backlog = rx.slots();
                if backlog > in_flight {
                    let stale = backlog - in_flight;
                    if let Ok(c) = rx.read_chunk(stale) {
                        c.commit_all();
                    }
                    osh.slipped.fetch_add(stale as u64, Ordering::Relaxed);
                }

                let pushed = osh.pushed.load(Ordering::Acquire);
                let backlog = rx.slots();
                let take = backlog.min(frames);
                if take > 0 {
                    // The oldest frame still in the ring is the one this block
                    // is about to play.
                    let oldest = pushed.saturating_sub(backlog as u64);
                    let bstart = osh.block_start.load(Ordering::Relaxed);
                    let bcap = osh.block_capture_ns.load(Ordering::Relaxed);
                    // Capture time is linear in frame index within a block.
                    let delta_frames = oldest as i64 - bstart as i64;
                    let cap_ns =
                        bcap as i64 + (delta_frames * 1_000_000_000) / i64::from(rate);
                    let play_ns = now as i64 + ahead as i64;
                    let lat = play_ns - cap_ns;
                    if lat > 0 {
                        let lat = lat as u64;
                        osh.n.fetch_add(1, Ordering::Relaxed);
                        osh.sum.fetch_add(lat, Ordering::Relaxed);
                        osh.min.fetch_min(lat, Ordering::Relaxed);
                        osh.max.fetch_max(lat, Ordering::Relaxed);
                        let b = ((lat / 250_000) as usize).min(127);
                        osh.hist[b].fetch_add(1, Ordering::Relaxed);
                    }
                    if let Ok(c) = rx.read_chunk(take) {
                        c.commit_all();
                    }
                }
            },
            |e| eprintln!("output stream error: {e}"),
            None,
        )
        .expect("build output stream");

    // The INPUT is started last, which is what the app does and what makes
    // cpal's device-wide buffer-size write land on the smaller number.
    out_stream.play().expect("play output");
    in_stream.play().expect("play input");
    std::thread::sleep(std::time::Duration::from_secs_f64(secs));
    // Dropped, never killed: force-killing a CoreAudio client wedges device
    // enumeration for the whole machine.
    drop(in_stream);
    drop(out_stream);

    let n = sh.n.load(Ordering::Relaxed);
    if n == 0 {
        println!("\nno measurements — did the input device deliver anything?");
        return;
    }
    let ms = |ns: u64| ns as f64 / 1.0e6;
    let mean = sh.sum.load(Ordering::Relaxed) / n;
    // The median off the histogram, which is the number to quote: the mean is
    // dragged by the first few callbacks while the ring settles.
    let mut seen = 0u64;
    let mut median = 0.0;
    for (i, b) in sh.hist.iter().enumerate() {
        seen += b.load(Ordering::Relaxed);
        if seen * 2 >= n {
            median = i as f64 * 0.25 + 0.125;
            break;
        }
    }
    println!(
        "\nround trip over {n} callbacks, capture -> playback:\n  \
         min {:.2} ms   median {:.2} ms   mean {:.2} ms   max {:.2} ms\n  \
         stale frames dropped: {}\n  \
         (output {} frames = {:.2} ms, input {} frames = {:.2} ms at {} Hz)",
        ms(sh.min.load(Ordering::Relaxed)),
        median,
        ms(mean),
        ms(sh.max.load(Ordering::Relaxed)),
        sh.slipped.load(Ordering::Relaxed),
        out_buf,
        f64::from(out_buf) * 1000.0 / f64::from(rate),
        in_buf,
        f64::from(in_buf) * 1000.0 / f64::from(rate),
        rate
    );
}
