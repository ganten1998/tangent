//! The round trip measured by CLICKING, for platforms whose drivers lie.
//!
//! **Companion to `rtlatency`, not a replacement for it.** That one brackets
//! the SOFTWARE path using the driver's own capture and playback timestamps,
//! which is the right instrument when the driver is honest — on CoreAudio it
//! is, and it needs no cable to do it.
//!
//! This one assumes nothing. It emits a click, watches for it coming back, and
//! measures the wall clock between, because on FreeBSD's alsa-lib-over-OSS shim
//! the driver reports a capture buffer `0.00 ms` behind the callback, which
//! cannot be true. Where the timestamps can be trusted, prefer `rtlatency` —
//! it is the finer instrument. Where they cannot, only a real signal going out
//! and coming back settles it.
//!
//! Every number the Audio Setup panel shows is a REQUEST: a buffer size we
//! asked a driver for. What a player feels is the whole loop — the app writes a
//! sample, the ring drains, the DAC converts, the sound crosses the air, the
//! ADC converts, an input ring fills, and only then does the app see it again.
//! No part of the code knows that number, and on a new platform nobody should
//! believe an estimate of it. This measures it.
//!
//!   cargo run --release -p tangent-record --example rtclick -- [seconds] [buffer] [rate]
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
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// Full scale, one millisecond. Loud and short: loud so it clears a noise
/// floor without the mic gain having to be perfect, short so the arrival time
/// is a moment rather than a smear.
const CLICK_MS: f32 = 1.0;
/// Far enough apart that a click's own echo has died before the next one.
const CLICK_EVERY_MS: f32 = 500.0;
/// A click is full-scale; room noise, speaker rumble and an open instrument
/// input are not. Anything past this is the click and not the room.
///
/// Low enough to survive a quiet monitoring level or a conservative input gain,
/// because the failure this guards against is not a false positive — a
/// mis-detected room noise would show up instantly as an absurd round trip —
/// but a run that reports nothing and cannot say why.
const DETECT: f32 = 0.10;

fn main() {
    let mut args = std::env::args().skip(1);
    let secs: f32 = args.next().and_then(|s| s.parse().ok()).unwrap_or(6.0);
    let buffer: Option<u32> = args.next().and_then(|s| s.parse().ok());
    // The third argument is the sample rate, and it is here because the
    // DEFAULT is not always the device's own. FreeBSD's alsa-lib shim reports
    // 44100 whatever the hardware runs at, so asking for the default can put
    // a resampler in a path being measured for latency without saying so.
    let rate_arg: Option<u32> = args.next().and_then(|s| s.parse().ok());

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

    // `IVORY_AUDIO_DEVICE` picks a device by name instead of taking the
    // default, which matters more on FreeBSD than anywhere else: the shim
    // offers "default" (which asound.conf maps to `plug:oss`, adding a
    // conversion layer) and "oss" (which is the OSS plugin raw). Those are the
    // same hardware down two different paths, and the difference between them
    // is worth being able to measure.
    let want_dev = std::env::var("IVORY_AUDIO_DEVICE").ok().filter(|s| !s.is_empty());
    // **Raw mode: trust no backend's arithmetic, only the wall clock.**
    //
    // The adjusted measurement adds the driver's own `playback` lead and
    // subtracts its `capture` lag, which is the right answer WHEN THE DRIVER IS
    // HONEST. Comparing hosts that way compares their bookkeeping as much as
    // their latency — and on FreeBSD one of them reports a capture buffer
    // 0.00 ms behind the callback, which cannot be true. In raw mode the clock
    // starts when the callback that wrote the click returns and stops when the
    // callback that saw it is entered, so every host is measured by the same
    // rule: app to app, no claims involved.
    let raw = std::env::var("IVORY_RAW_TIMING").is_ok();
    let by_name = |name: &str, input: bool| -> Option<cpal::Device> {
        let iter: Vec<cpal::Device> = if input {
            host.input_devices().ok()?.collect()
        } else {
            host.output_devices().ok()?.collect()
        };
        iter.into_iter()
            .find(|d| d.name().map(|n| n == name).unwrap_or(false))
    };

    let out_dev = match &want_dev {
        Some(n) => match by_name(n, false) {
            Some(d) => d,
            None => { eprintln!("no output device named {n:?}"); return; }
        },
        None => match host.default_output_device() {
            Some(d) => d,
            None => { eprintln!("no output device"); return; }
        },
    };
    let in_dev = match &want_dev {
        Some(n) => match by_name(n, true) {
            Some(d) => d,
            None => { eprintln!("no input device named {n:?}"); return; }
        },
        None => match host.default_input_device() {
            Some(d) => d,
            None => { eprintln!("no input device"); return; }
        },
    };
    let out_base = out_dev.default_output_config().expect("no output config");
    let in_base = in_dev.default_input_config().expect("no input config");
    println!("output          : {:?}  {:?}", out_dev.name(), out_base);
    println!("input           : {:?}  {:?}", in_dev.name(), in_base);

    // One rate for both directions or the arithmetic below is comparing frames
    // of different lengths. The output's native rate is the one the speakers
    // will not resample.
    let rate = rate_arg.unwrap_or_else(|| out_base.sample_rate().0);
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
    // The loudest thing the input ever saw, kept so that "no click" can say
    // whether the input was silent (nothing patched, gain down) or merely
    // quieter than the threshold. Without it the failure is unactionable.
    let peak = Arc::new(AtomicU32::new(0));

    // **Generic over the sample format, because FreeBSD makes it matter.**
    // The shim's "default" device converts to f32 and reports a nonsense
    // buffer range; the raw "oss" device is I16 only and reports a real one.
    // Measuring the second requires speaking its format, so the click writer
    // and the detector are written once against f32 and converted at the edge.
    let out_stream = match out_base.sample_format() {
        cpal::SampleFormat::I32 => build_out::<i32>(
            &out_dev, &out_cfg, out_ch, rate, click_len, click_period,
            sent.clone(), armed.clone(), frame.clone(), raw),
        cpal::SampleFormat::I16 => build_out::<i16>(
            &out_dev, &out_cfg, out_ch, rate, click_len, click_period,
            sent.clone(), armed.clone(), frame.clone(), raw),
        _ => build_out::<f32>(
            &out_dev, &out_cfg, out_ch, rate, click_len, click_period,
            sent.clone(), armed.clone(), frame.clone(), raw),
    }
    .expect("could not open the output stream");

    let in_stream = match in_base.sample_format() {
        cpal::SampleFormat::I32 => build_in::<i32>(
            &in_dev, &in_cfg, in_ch, rate, sent.clone(), armed.clone(),
            results.clone(), reported.clone(), peak.clone(), raw),
        cpal::SampleFormat::I16 => build_in::<i16>(
            &in_dev, &in_cfg, in_ch, rate, sent.clone(), armed.clone(),
            results.clone(), reported.clone(), peak.clone(), raw),
        _ => build_in::<f32>(
            &in_dev, &in_cfg, in_ch, rate, sent.clone(), armed.clone(),
            results.clone(), reported.clone(), peak.clone(), raw),
    }
    .expect("could not open the input stream");

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
        let seen = peak.load(Ordering::Relaxed) as f32 / 1e6;
        println!(
            "\nNo click was ever heard, so there is no round trip to report.\n\
             That is a result about the PATH, not about the latency.\n\n\
             The loudest thing the input saw was {seen:.4} of full scale, \
             against a threshold of {DETECT}."
        );
        if seen < 0.001 {
            println!(
                "That is silence: nothing is reaching the input at all. Check the\n\
                 cable is in the INPUT and not a headphone socket, that the gain\n\
                 is up, and — on an instrument input — that INST is engaged."
            );
        } else {
            println!(
                "So the path works and the level is merely low: turn the input\n\
                 gain up, or the monitoring level if this is an acoustic loop."
            );
        }
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
        // No claim about WHICH loop is patched, because this cannot tell: a
        // cable and a speaker-to-microphone path both arrive here. The
        // difference is under a millisecond of flight time either way, and
        // saying which one it was is the operator's job, not the probe's.
        "  ({:.0} frames at {rate} Hz, median)",
        med / 1000.0 * rate as f32
    );
}

/// A sample this probe can write a click into.
trait OutSample: cpal::SizedSample + Send + 'static {
    fn from_f32(v: f32) -> Self;
}
impl OutSample for f32 {
    fn from_f32(v: f32) -> f32 { v }
}
impl OutSample for i16 {
    fn from_f32(v: f32) -> i16 { (v * 32767.0) as i16 }
}
impl OutSample for i32 {
    fn from_f32(v: f32) -> i32 { (v * 2_147_483_000.0) as i32 }
}

/// A sample this probe can look for a click in.
trait InSample: cpal::SizedSample + Send + Copy + 'static {
    fn to_f32(self) -> f32;
}
impl InSample for f32 {
    fn to_f32(self) -> f32 { self }
}
impl InSample for i16 {
    fn to_f32(self) -> f32 { self as f32 / 32768.0 }
}
impl InSample for i32 {
    fn to_f32(self) -> f32 { self as f32 / 2_147_483_648.0 }
}

#[allow(clippy::too_many_arguments)]
fn build_out<T: OutSample>(
    dev: &cpal::Device,
    cfg: &cpal::StreamConfig,
    out_ch: usize,
    rate: u32,
    click_len: u64,
    click_period: u64,
    sent: Arc<Mutex<Option<Instant>>>,
    armed: Arc<AtomicBool>,
    frame: Arc<AtomicU64>,
    raw: bool,
) -> Result<cpal::Stream, cpal::BuildStreamError> {
    dev.build_output_stream(
        cfg,
        move |data: &mut [T], info: &cpal::OutputCallbackInfo| {
            let now = Instant::now();
            // How far ahead of "now" this buffer will actually be heard, from
            // the driver's own timestamps. Added to the send stamp so the
            // measurement starts where the sound starts, not where the memcpy
            // happened.
            let ahead = info
                .timestamp()
                .playback
                .duration_since(&info.timestamp().callback)
                .map(|d| d.as_secs_f32())
                .unwrap_or(0.0);
            let ahead = if raw { 0.0 } else { ahead };
            let start = frame.fetch_add((data.len() / out_ch) as u64, Ordering::Relaxed);
            for (i, f) in data.chunks_mut(out_ch).enumerate() {
                let pos = (start + i as u64) % click_period;
                // Alternate sign so the click is a sharp edge rather than a
                // tone the room can ring with.
                let v = if pos < click_len {
                    if pos % 2 == 0 { 0.9 } else { -0.9 }
                } else {
                    0.0
                };
                if pos == 0 {
                    let at = now
                        + std::time::Duration::from_secs_f32(ahead + i as f32 / rate as f32);
                    *sent.lock().unwrap() = Some(at);
                    armed.store(true, Ordering::Release);
                }
                for s in f.iter_mut() {
                    *s = T::from_f32(v);
                }
            }
        },
        |e| eprintln!("output stream error: {e}"),
        None,
    )
}

#[allow(clippy::too_many_arguments)]
fn build_in<T: InSample>(
    dev: &cpal::Device,
    cfg: &cpal::StreamConfig,
    in_ch: usize,
    rate: u32,
    sent: Arc<Mutex<Option<Instant>>>,
    armed: Arc<AtomicBool>,
    results: Arc<Mutex<Vec<f32>>>,
    reported: Arc<Mutex<Option<f32>>>,
    peak: Arc<AtomicU32>,
    raw: bool,
) -> Result<cpal::Stream, cpal::BuildStreamError> {
    dev.build_input_stream(
        cfg,
        move |data: &[T], info: &cpal::InputCallbackInfo| {
            let now = Instant::now();
            let loudest = data.iter().fold(0f32, |m, s| m.max(s.to_f32().abs()));
            peak.fetch_max((loudest * 1e6) as u32, Ordering::Relaxed);
            // The mirror of the output's `ahead`: how long ago the first
            // sample in this buffer was actually captured.
            let behind = info
                .timestamp()
                .callback
                .duration_since(&info.timestamp().capture)
                .map(|d| d.as_secs_f32())
                .unwrap_or(0.0);
            let behind = if raw { 0.0 } else { behind };
            if reported.lock().unwrap().is_none() {
                *reported.lock().unwrap() = Some(behind * 1000.0);
            }
            if !armed.load(Ordering::Acquire) {
                return;
            }
            for (i, f) in data.chunks(in_ch).enumerate() {
                if f.iter().any(|s| s.to_f32().abs() >= DETECT) {
                    let heard = now - std::time::Duration::from_secs_f32(behind)
                        + std::time::Duration::from_secs_f32(i as f32 / rate as f32);
                    let mut slot = sent.lock().unwrap();
                    if let Some(at) = slot.take() {
                        armed.store(false, Ordering::Release);
                        if heard > at {
                            results.lock().unwrap().push((heard - at).as_secs_f32() * 1000.0);
                        }
                    }
                    break;
                }
            }
        },
        |e| eprintln!("input stream error: {e}"),
        None,
    )
}
