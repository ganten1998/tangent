//! What the round trip actually costs, measured instead of assumed.
//!
//! Every number the Audio Setup panel shows is a REQUEST: a buffer size we
//! asked a driver for. What a player feels is the whole loop — the app writes a
//! sample, the ring drains, the DAC converts, the sound crosses the air, the
//! ADC converts, an input ring fills, and only then does the app see it again.
//! No part of the code knows that number, and on a new platform nobody should
//! believe an estimate of it. This measures it.
//!
//!   cargo run --release -p tangent-record --example rtlatency -- [seconds] [buffer]
//!
//! **How it measures.** The output stream emits a one-millisecond full-scale
//! click every half second and notes the wall clock at the instant it wrote it,
//! offset for where in the buffer the click sits. The input stream watches for
//! the click coming back and notes the wall clock where it found it, offset the
//! same way. The difference between those two is the round trip. Both stamps
//! are taken inside the callbacks with `Instant::now()`, on one clock, so the
//! subtraction is meaningful even though the two streams are independent.
//!
//! cpal's own `playback`/`capture` timestamps are printed beside the measured
//! figure. They are what the driver BELIEVES; a gap between the two columns is
//! itself the finding, and on a shim like FreeBSD's alsa-lib-over-OSS it is the
//! number most likely to be a polite fiction.
//!
//! **The path has to close.** With no cable, the loop is acoustic: the speakers
//! play the click and the microphone hears it, which works on a laptop and adds
//! the flight time — about 3 µs per centimetre, so under a millisecond at any
//! sane distance, and small against any buffer worth having. Turn the speakers
//! up and the mic on; if the click is never seen this prints that rather than
//! inventing a number. A physical output-to-input cable is the better path
//! where there is one, and needs no change here.
//!
//! `IVORY_AUDIO_SYSTEM` picks the host by name (`JACK`, `ALSA`), matching the
//! Audio Setup panel, so the same probe compares backends on one box.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// Full scale, one millisecond. Loud and short: loud so it clears a noise
/// floor without the mic gain having to be perfect, short so the arrival time
/// is a moment rather than a smear.
const CLICK_MS: f32 = 1.0;
/// Far enough apart that a click's own echo has died before the next one.
const CLICK_EVERY_MS: f32 = 500.0;
/// A click is full-scale; room noise and speaker rumble are not. Anything past
/// this is the click and not the room.
const DETECT: f32 = 0.25;

fn main() {
    let mut args = std::env::args().skip(1);
    let secs: f32 = args.next().and_then(|s| s.parse().ok()).unwrap_or(6.0);
    let buffer: Option<u32> = args.next().and_then(|s| s.parse().ok());

    let host = match std::env::var("IVORY_AUDIO_SYSTEM").ok().filter(|s| !s.is_empty()) {
        Some(want) => cpal::available_hosts()
            .into_iter()
            .find(|id| id.name().eq_ignore_ascii_case(&want))
            .and_then(|id| cpal::host_from_id(id).ok())
            .unwrap_or_else(|| {
                eprintln!("no host named {want:?}; using the default");
                cpal::default_host()
            }),
        None => cpal::default_host(),
    };
    println!("hosts available : {:?}", cpal::available_hosts().iter().map(|h| h.name()).collect::<Vec<_>>());
    println!("host            : {}", host.id().name());

    let Some(out_dev) = host.default_output_device() else {
        eprintln!("no output device");
        return;
    };
    let Some(in_dev) = host.default_input_device() else {
        eprintln!("no input device");
        return;
    };
    let out_base = out_dev.default_output_config().expect("no output config");
    let in_base = in_dev.default_input_config().expect("no input config");
    println!("output          : {:?}  {:?}", out_dev.name(), out_base);
    println!("input           : {:?}  {:?}", in_dev.name(), in_base);

    // One rate for both directions or the arithmetic below is comparing frames
    // of different lengths. The output's native rate is the one the speakers
    // will not resample.
    let rate = out_base.sample_rate().0;
    let mut out_cfg: cpal::StreamConfig = out_base.clone().into();
    let mut in_cfg: cpal::StreamConfig = in_base.clone().into();
    in_cfg.sample_rate = cpal::SampleRate(rate);
    if let Some(b) = buffer {
        out_cfg.buffer_size = cpal::BufferSize::Fixed(b);
        in_cfg.buffer_size = cpal::BufferSize::Fixed(b);
    }
    let out_ch = out_cfg.channels as usize;
    let in_ch = in_cfg.channels as usize;
    println!(
        "asked           : {} Hz, out {} ch, in {} ch, buffer {:?}\n",
        rate, out_ch, in_ch, out_cfg.buffer_size
    );

    let click_len = (CLICK_MS / 1000.0 * rate as f32) as u64;
    let click_period = (CLICK_EVERY_MS / 1000.0 * rate as f32) as u64;

    // When the most recent click was WRITTEN, as a wall clock instant already
    // adjusted for where in the buffer it sat. The input side reads it once and
    // clears it, so one click is matched to one arrival and a missed click
    // cannot be paired with the next one's echo.
    let sent: Arc<Mutex<Option<Instant>>> = Arc::new(Mutex::new(None));
    let armed = Arc::new(AtomicBool::new(false));
    let frame = Arc::new(AtomicU64::new(0));
    let results: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(Vec::new()));
    // cpal's own opinion, sampled once so the print below is not a race.
    let reported: Arc<Mutex<Option<f32>>> = Arc::new(Mutex::new(None));

    let out_stream = {
        let sent = sent.clone();
        let armed = armed.clone();
        let frame = frame.clone();
        out_dev
            .build_output_stream(
                &out_cfg,
                move |data: &mut [f32], info: &cpal::OutputCallbackInfo| {
                    let now = Instant::now();
                    // How far ahead of "now" this buffer will actually be
                    // heard, from the driver's own timestamps. Added to the
                    // send stamp so the measurement starts where the sound
                    // starts, not where the memcpy happened.
                    let ahead = info
                        .timestamp()
                        .playback
                        .duration_since(&info.timestamp().callback)
                        .map(|d| d.as_secs_f32())
                        .unwrap_or(0.0);
                    let start = frame.fetch_add((data.len() / out_ch) as u64, Ordering::Relaxed);
                    for (i, f) in data.chunks_mut(out_ch).enumerate() {
                        let pos = (start + i as u64) % click_period;
                        let v = if pos < click_len {
                            // Alternate sign so the click is a sharp edge
                            // rather than a tone the room can ring with.
                            if pos % 2 == 0 { 0.9 } else { -0.9 }
                        } else {
                            0.0
                        };
                        if pos == 0 {
                            let at = now
                                + std::time::Duration::from_secs_f32(
                                    ahead + i as f32 / rate as f32,
                                );
                            *sent.lock().unwrap() = Some(at);
                            armed.store(true, Ordering::Release);
                        }
                        for s in f.iter_mut() {
                            *s = v;
                        }
                    }
                },
                |e| eprintln!("output stream error: {e}"),
                None,
            )
            .expect("could not open the output stream")
    };

    let in_stream = {
        let sent = sent.clone();
        let armed = armed.clone();
        let results = results.clone();
        let reported = reported.clone();
        in_dev
            .build_input_stream(
                &in_cfg,
                move |data: &[f32], info: &cpal::InputCallbackInfo| {
                    let now = Instant::now();
                    // The mirror of the output's `ahead`: how long ago the
                    // first sample in this buffer was actually captured.
                    let behind = info
                        .timestamp()
                        .callback
                        .duration_since(&info.timestamp().capture)
                        .map(|d| d.as_secs_f32())
                        .unwrap_or(0.0);
                    if reported.lock().unwrap().is_none() {
                        *reported.lock().unwrap() = Some(behind * 1000.0);
                    }
                    if !armed.load(Ordering::Acquire) {
                        return;
                    }
                    for (i, f) in data.chunks(in_ch).enumerate() {
                        if f.iter().any(|s| s.abs() >= DETECT) {
                            let heard = now
                                - std::time::Duration::from_secs_f32(behind)
                                + std::time::Duration::from_secs_f32(i as f32 / rate as f32);
                            let mut slot = sent.lock().unwrap();
                            if let Some(at) = slot.take() {
                                armed.store(false, Ordering::Release);
                                if heard > at {
                                    let ms = (heard - at).as_secs_f32() * 1000.0;
                                    results.lock().unwrap().push(ms);
                                }
                            }
                            break;
                        }
                    }
                },
                |e| eprintln!("input stream error: {e}"),
                None,
            )
            .expect("could not open the input stream")
    };

    out_stream.play().expect("could not start output");
    in_stream.play().expect("could not start input");
    println!("listening for {secs:.0}s — the speakers must be audible to the mic\n");
    std::thread::sleep(std::time::Duration::from_secs_f32(secs));
    drop(out_stream);
    drop(in_stream);

    let mut r = results.lock().unwrap().clone();
    if let Some(ms) = *reported.lock().unwrap() {
        println!("cpal says its capture buffer is {ms:.2} ms behind the callback");
    }
    if r.is_empty() {
        println!(
            "\nNo click was ever heard, so there is no round trip to report.\n\
             That is a result about the PATH, not about the latency: check that\n\
             the output is audible and that the input is not muted (`mixer` on\n\
             FreeBSD — `mic` and `rec` both need level, and `mic` must be the\n\
             recording source). Re-run once a click is audible."
        );
        return;
    }
    r.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let n = r.len();
    let med = r[n / 2];
    let mean = r.iter().sum::<f32>() / n as f32;
    println!(
        "\nround trip over {n} clicks:\n  min {:.2} ms   median {med:.2} ms   mean {mean:.2} ms   max {:.2} ms",
        r[0],
        r[n - 1]
    );
    println!(
        "  ({:.0} frames at {rate} Hz, median; includes the acoustic flight)",
        med / 1000.0 * rate as f32
    );
}
