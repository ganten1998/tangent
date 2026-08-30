//! **The round trip with the signal actually in it.**
//!
//! [`rtlatency`] brackets the SOFTWARE path with CoreAudio's timestamps and is
//! honest about being a floor. This plays a click, listens for it coming back,
//! and counts the time between — so whatever the converters, the USB frame, the
//! HAL's safety offsets and anything else resident on the machine are costing
//! is IN the number, because the click went through all of it.
//!
//! Two ways to get the signal back, and it discovers which by itself:
//!
//! * **A cable** from an output into an input. The true figure, converters and
//!   all, and the one to compare against another host.
//! * **The interface's own loopback** — the Scarlett 18i20 carries `Loop 1` and
//!   `Loop 2` on inputs 9 and 10 — which returns the playback stream digitally.
//!   That misses the AD and DA and nothing else, so the gap between the two is
//!   the converters, measured rather than assumed.
//!
//! It scans EVERY input channel for the click, so it does not need to be told
//! which one is listening; it reports the channel it found and how loud.
//!
//! ```text
//! cargo run --release -p tangent-record --example loopback -- 128 64 12
//! ```
//!
//! Arguments: output buffer, input buffer, how many clicks to average. The
//! defaults mirror what Tangent actually opens on macOS — the input at a
//! quarter of the output, floor 64 — so the figure is directly comparable.
//! `DEVICE` picks the interface by substring, `OUT_CH` the output channel pair.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{BufferSize, SampleFormat, StreamConfig};

/// The click is a short alternating burst rather than one sample: full-scale
/// every other sample is the most distinctive thing that can be put on a wire,
/// it survives a converter round trip, and it is still under a millisecond so
/// its leading edge is a sharp arrival time.
const CLICK: f32 = 0.7;
const BURST: usize = 8;
/// The absolute floor, whatever the room is doing. The threshold actually used
/// is this or ten times the measured ambient, whichever is HIGHER — the first
/// run of this probe triggered on the loopback's own idle content at 0.02 and
/// reported a 69 ms mean made entirely of noise.
const FLOOR: f32 = 0.05;

struct Shared {
    /// Nanoseconds (monotonic) at which the click will LEAVE, or 0.
    sent_ns: AtomicU64,
    /// Set once the output has written a click and is waiting to hear it.
    pending: AtomicBool,
    /// Round-trip samples in nanoseconds.
    n: AtomicU64,
    sum: AtomicU64,
    min: AtomicU64,
    max: AtomicU64,
    /// Which input channel heard it, and how loud.
    channel: AtomicU64,
    peak_bits: AtomicU64,
    /// The loudest thing seen on any input while no click was in flight.
    ambient_bits: AtomicU64,
    /// Per input channel: loudest sample, and strongest burst correlation.
    peaks: Vec<AtomicU64>,
    corrs: Vec<AtomicU64>,
    missed: AtomicU64,
}

fn pick(host: &cpal::Host, want: Option<&str>, output: bool) -> Option<cpal::Device> {
    let all: Vec<_> = if output {
        host.output_devices().ok()?.collect()
    } else {
        host.input_devices().ok()?.collect()
    };
    match want {
        Some(w) => {
            let w = w.to_lowercase();
            all.into_iter()
                .find(|d| d.name().map(|n| n.to_lowercase().contains(&w)).unwrap_or(false))
        }
        None => {
            if output {
                host.default_output_device()
            } else {
                host.default_input_device()
            }
        }
    }
}

fn main() {
    let out_buf: u32 = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(128);
    let in_buf: u32 = std::env::args()
        .nth(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| (out_buf / 4).max(64));
    let clicks: usize = std::env::args().nth(3).and_then(|s| s.parse().ok()).unwrap_or(12);
    let out_ch0: usize = std::env::var("OUT_CH").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
    let want_dev = std::env::var("DEVICE").ok();

    let host = cpal::default_host();
    let (Some(out_dev), Some(in_dev)) = (
        pick(&host, want_dev.as_deref(), true),
        pick(&host, want_dev.as_deref(), false),
    ) else {
        eprintln!("could not find both an input and an output device");
        std::process::exit(1);
    };
    let out_cfg = out_dev.default_output_config().expect("output config");
    let in_cfg = in_dev.default_input_config().expect("input config");
    if out_cfg.sample_format() != SampleFormat::F32 || in_cfg.sample_format() != SampleFormat::F32 {
        eprintln!("this probe only speaks f32");
        std::process::exit(1);
    }
    let rate = out_cfg.sample_rate().0;
    let out_n = out_cfg.channels() as usize;
    let in_n = in_cfg.channels() as usize;
    println!(
        "output {:?} {} ch, asking {} frames\ninput  {:?} {} ch, asking {} frames\nclicking on output channels {}/{} at {} Hz",
        out_dev.name().unwrap_or_default(), out_n, out_buf,
        in_dev.name().unwrap_or_default(), in_n, in_buf,
        out_ch0 + 1, out_ch0 + 2, rate
    );

    let t0 = Instant::now();
    let sh = Arc::new(Shared {
        sent_ns: AtomicU64::new(0),
        pending: AtomicBool::new(false),
        n: AtomicU64::new(0),
        sum: AtomicU64::new(0),
        min: AtomicU64::new(u64::MAX),
        max: AtomicU64::new(0),
        channel: AtomicU64::new(u64::MAX),
        peak_bits: AtomicU64::new(0),
        ambient_bits: AtomicU64::new(0),
        peaks: (0..in_n).map(|_| AtomicU64::new(0)).collect(),
        corrs: (0..in_n).map(|_| AtomicU64::new(0)).collect(),
        missed: AtomicU64::new(0),
    });

    let osh = Arc::clone(&sh);
    let oc = t0;
    // Blocks to wait between clicks: long enough that the previous one has
    // certainly come back and decayed before the next goes out.
    let gap = (rate / out_buf.max(1) / 4).max(4) as u64;
    let mut block = 0u64;
    let out_stream = out_dev
        .build_output_stream(
            &StreamConfig {
                channels: out_cfg.channels(),
                sample_rate: out_cfg.sample_rate(),
                buffer_size: BufferSize::Fixed(out_buf),
            },
            move |data: &mut [f32], info: &cpal::OutputCallbackInfo| {
                data.fill(0.0);
                block += 1;
                // Warm up first: the first blocks of a fresh stream are not
                // representative of anything.
                // **A click that never came back must not stop the next one.**
                // Without this the first miss latched `pending` for the rest of
                // the run and exactly one burst was ever played, which is how
                // the first working version of this probe reported "no click"
                // off a single sample of evidence.
                if osh.pending.load(Ordering::Acquire) {
                    let sent = osh.sent_ns.load(Ordering::Relaxed);
                    let now = oc.elapsed().as_nanos() as u64;
                    if now.saturating_sub(sent) > 200_000_000 {
                        osh.missed.fetch_add(1, Ordering::Relaxed);
                        osh.pending.store(false, Ordering::Release);
                    }
                    return;
                }
                if block < 40 || block % gap != 0 {
                    return;
                }
                let ahead = info
                    .timestamp()
                    .playback
                    .duration_since(&info.timestamp().callback)
                    .map(|d| d.as_nanos() as u64)
                    .unwrap_or(0);
                // The click is the FIRST frame of this block, so the moment it
                // is heard is exactly the playback timestamp.
                for f in 0..BURST {
                    let v = if f % 2 == 0 { CLICK } else { -CLICK };
                    for c in 0..2 {
                        if let Some(s) = data.get_mut(f * out_n + out_ch0 + c) {
                            *s = v;
                        }
                    }
                }
                osh.sent_ns
                    .store(oc.elapsed().as_nanos() as u64 + ahead, Ordering::Relaxed);
                osh.pending.store(true, Ordering::Release);
            },
            |e| eprintln!("output error: {e}"),
            None,
        )
        .expect("build output");

    let ish = Arc::clone(&sh);
    let ic = t0;
    // One rolling window per input channel, newest first. Allocated here so
    // the callback never does.
    let mut hist: Vec<[f32; BURST]> = vec![[0.0; BURST]; in_n];
    let in_stream = in_dev
        .build_input_stream(
            &StreamConfig {
                channels: in_cfg.channels(),
                sample_rate: in_cfg.sample_rate(),
                buffer_size: BufferSize::Fixed(in_buf),
            },
            move |data: &[f32], info: &cpal::InputCallbackInfo| {
                let frames = data.len() / in_n.max(1);
                // **Correlation, not a threshold.** The first version of this
                // triggered on whatever the machine happened to be playing —
                // the interface's loopback carries the system mix, which
                // measured 0.14 — and reported a 69 ms mean made entirely of
                // noise. The burst is full scale at NYQUIST, alternating every
                // sample, and ordinary programme material has almost no energy
                // there; correlating against that pattern ignores music
                // completely and answers in the units of the click itself.
                for f in 0..frames {
                    for c in 0..in_n {
                        let h = &mut hist[c];
                        h.rotate_right(1);
                        h[0] = data[f * in_n + c];
                        let mut corr = 0.0f32;
                        for (k, v) in h.iter().enumerate() {
                            corr += if k % 2 == 0 { *v } else { -*v };
                        }
                        corr /= BURST as f32;
                        if let Some(a) = ish.peaks.get(c) {
                            a.fetch_max(u64::from(h[0].abs().to_bits()), Ordering::Relaxed);
                        }
                        if let Some(a) = ish.corrs.get(c) {
                            a.fetch_max(u64::from(corr.abs().to_bits()), Ordering::Relaxed);
                        }
                        if !ish.pending.load(Ordering::Acquire) {
                            continue;
                        }
                        if corr.abs() < CLICK * 0.15 {
                            continue;
                        }
                        let now = ic.elapsed().as_nanos() as u64;
                        let ago = info
                            .timestamp()
                            .callback
                            .duration_since(&info.timestamp().capture)
                            .map(|d| d.as_nanos() as u64)
                            .unwrap_or(0);
                        // The burst's LAST sample is in `h[0]`; its first
                        // arrived `BURST - 1` frames earlier, and that is the
                        // arrival time being measured.
                        let at = (f + 1).saturating_sub(BURST) as u64;
                        let cap = now.saturating_sub(ago)
                            + (at * 1_000_000_000) / u64::from(rate);
                        let sent = ish.sent_ns.load(Ordering::Relaxed);
                        if cap > sent {
                            let lat = cap - sent;
                            ish.n.fetch_add(1, Ordering::Relaxed);
                            ish.sum.fetch_add(lat, Ordering::Relaxed);
                            ish.min.fetch_min(lat, Ordering::Relaxed);
                            ish.max.fetch_max(lat, Ordering::Relaxed);
                            ish.channel.store(c as u64, Ordering::Relaxed);
                            ish.peak_bits
                                .store(u64::from(corr.abs().to_bits()), Ordering::Relaxed);
                        }
                        ish.pending.store(false, Ordering::Release);
                    }
                }
            },
            |e| eprintln!("input error: {e}"),
            None,
        )
        .expect("build input");

    out_stream.play().expect("play output");
    in_stream.play().expect("play input");
    // Enough wall time for `clicks` round trips at the chosen gap.
    let secs = (clicks as f64 * gap as f64 * f64::from(out_buf) / f64::from(rate)) + 3.0;
    std::thread::sleep(std::time::Duration::from_secs_f64(secs));
    // Dropped, never killed: force-killing a CoreAudio client wedges device
    // enumeration for the whole machine.
    drop(in_stream);
    drop(out_stream);

    println!(
        "\nbursts that never came back: {}",
        sh.missed.load(Ordering::Relaxed)
    );
    println!("per input channel:  peak / burst-correlation");
    for c in 0..in_n {
        let pk = f32::from_bits(sh.peaks[c].load(Ordering::Relaxed) as u32);
        let co = f32::from_bits(sh.corrs[c].load(Ordering::Relaxed) as u32);
        if pk > 0.0005 || co > 0.0005 {
            println!("  in {:>2}: peak {pk:.4}   corr {co:.4}", c + 1);
        }
    }
    let n = sh.n.load(Ordering::Relaxed);
    if n == 0 {
        println!(
            "\nNO CLICK CAME BACK.\n  Nothing is routed from the output back to an input. Either patch a\n  cable from an output into an input, or route the output to the\n  interface's loopback inputs in its control panel."
        );
        return;
    }
    let ms = |ns: u64| ns as f64 / 1.0e6;
    let frames = |ns: u64| ns as f64 * f64::from(rate) / 1.0e9;
    let mean = sh.sum.load(Ordering::Relaxed) / n;
    println!(
        "\nheard {n} clicks on INPUT CHANNEL {} at {:.3} peak correlation\n  \
         round trip  min {:.2} ms ({:.0} frames)   mean {:.2} ms ({:.0} frames)   max {:.2} ms\n  \
         (output {} frames, input {} frames, {} Hz)",
        sh.channel.load(Ordering::Relaxed) + 1,
        f32::from_bits(sh.peak_bits.load(Ordering::Relaxed) as u32),
        ms(sh.min.load(Ordering::Relaxed)),
        frames(sh.min.load(Ordering::Relaxed)),
        ms(mean),
        frames(mean),
        ms(sh.max.load(Ordering::Relaxed)),
        out_buf,
        in_buf,
        rate
    );
}
