//! Reading an audio file in, for the backing-track player.
//!
//! **Two backends, split the same way `encode` is, and for the same reason.**
//! Neither platform needs a decoding crate per format, because both already
//! have something that reads everything:
//!
//! * **Not macOS** — `tangent-ffmpeg`, which the release already ships so that
//!   video works on a machine with nothing installed. One command does format,
//!   channel layout and sample rate together.
//! * **macOS** — `afconvert`, which is in `/usr/bin` on every Mac ever sold.
//!   The mac build has no bundled ffmpeg (video goes through AVFoundation), so
//!   reaching for one here would mean either shipping 76 MB of encoder for a
//!   feature that is not encoding, or several hundred lines of CoreAudio FFI
//!   plus a resampler of my own. `afconvert` is neither.
//!
//! What comes back either way is interleaved **stereo f32 at the device's
//! rate**, because that is the only shape the mixer wants and both tools will
//! convert to it. A mono file is duplicated across both channels, which is
//! what a mono backing track should do.

use std::path::{Path, PathBuf};
use std::process::Stdio;

/// A decoded audio file, ready to mix.
#[derive(Debug, Clone, PartialEq)]
pub struct Clip {
    /// Interleaved stereo, at [`Clip::rate`].
    pub samples: Vec<f32>,
    pub rate: u32,
    /// Where it came from, for the row's label and for reloading it.
    pub source: PathBuf,
}

impl Clip {
    pub fn frames(&self) -> usize {
        self.samples.len() / 2
    }

    pub fn seconds(&self) -> f64 {
        if self.rate == 0 {
            return 0.0;
        }
        self.frames() as f64 / f64::from(self.rate)
    }

    /// The file's name, for a row that has one line to say what is loaded.
    pub fn label(&self) -> String {
        self.source
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    /// A peak envelope, `buckets` wide, for drawing.
    ///
    /// **Peak and not average.** A waveform drawn from means is a grey sausage
    /// that says nothing about where the music is; the outline people read is
    /// the loudest sample in each column, which is also what tells them where
    /// a track actually starts when they come to trim it.
    ///
    /// Both channels fold into one outline: this is a picture for finding the
    /// ends of a take, not a stereo analyser.
    pub fn envelope(&self, buckets: usize) -> Vec<f32> {
        let frames = self.frames();
        if buckets == 0 || frames == 0 {
            return Vec::new();
        }
        let mut out = vec![0.0; buckets];
        for (i, slot) in out.iter_mut().enumerate() {
            let from = i * frames / buckets;
            let to = (((i + 1) * frames / buckets).max(from + 1)).min(frames);
            let mut peak = 0.0f32;
            for f in from..to {
                peak = peak.max(self.samples[f * 2].abs());
                peak = peak.max(self.samples[f * 2 + 1].abs());
            }
            *slot = peak.min(1.0);
        }
        out
    }
}

/// The most audio one clip may hold, in frames.
///
/// **Twenty minutes of stereo, which is 460 MB of f32.** A backing track is
/// three to six minutes; this is not a limit anybody making one will meet, and
/// it is what stops somebody who picked a two-hour podcast by mistake from
/// finding out by watching the machine swap. The error says the length, so it
/// reads as "that file is enormous" rather than as a failure.
const MAX_FRAMES: usize = 20 * 60 * 48_000;

/// Decode `path` to interleaved stereo f32 at `rate`.
///
/// The error is a sentence for a person: this is reached by picking a file in
/// a dialog, so "it did not work" has to say what to do instead.
///
/// **The decoder's word is verified, not trusted.** This used to read a bare
/// `f32le` byte stream and stamp the asked-for rate on it — whatever the
/// subprocess actually produced. The engine never resamples a clip, so any
/// decoder that ignored `-ar` or `-ac` (a slimmed-down ffmpeg build, an odd
/// codec path) came out as wrong-speed, wrong-channel garbage in every
/// export and nothing anywhere could tell. The decode goes through a WAV
/// container now, whose `fmt ` chunk states what was REALLY produced; a
/// different rate is resampled here, a different layout is folded here, and
/// a format this cannot read is a clear sentence instead of noise.
pub fn decode(path: &Path, rate: u32) -> Result<Clip, String> {
    let rate = rate.clamp(8_000, 192_000);
    let wav = read_wav(path, rate)?;
    let (samples, got) = wav_stereo_f32(&wav)?;
    if samples.len() < 2 {
        return Err("that file has no audio in it".to_owned());
    }
    // The length check speaks in OUTPUT frames, and runs before the
    // resample allocates them.
    let frames_out = (samples.len() / 2) as u64 * u64::from(rate) / u64::from(got.max(1));
    if frames_out as usize > MAX_FRAMES {
        return Err(format!(
            "that file is {:.0} minutes long. The backing track holds up to {} \
             minutes - trim it first, or pick a shorter one.",
            frames_out as f64 / f64::from(rate) / 60.0,
            MAX_FRAMES / 60 / 48_000
        ));
    }
    let samples = if got == rate {
        samples
    } else {
        resample_stereo(&samples, got, rate)
    };
    Ok(Clip {
        samples,
        rate,
        source: path.to_path_buf(),
    })
}

/// Interleaved stereo f32 out of a RIFF/WAVE byte string, plus the rate its
/// own `fmt ` chunk states.
///
/// **Walked rather than assumed at offset 44.** `afconvert` writes an `FLLR`
/// padding chunk before `data` to page-align it, and ffmpeg piping to stdout
/// writes `0xFFFFFFFF` for the sizes it could not seek back to fill in — the
/// walk tolerates both. Mono is doubled into both ears; anything wider than
/// stereo keeps its first pair (the decode asked for two, so this is a
/// decoder that did not listen); int16/24/32 are converted, because a
/// decoder that ignored the codec ask should still come out as music.
fn wav_stereo_f32(bytes: &[u8]) -> Result<(Vec<f32>, u32), String> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err("the decoder did not produce audio".to_owned());
    }
    let mut fmt: Option<(u16, u16, u32, u16)> = None; // tag, channels, rate, bits
    let mut data: Option<&[u8]> = None;
    let mut at = 12usize;
    while at + 8 <= bytes.len() {
        let id = &bytes[at..at + 4];
        let size = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().unwrap()) as usize;
        let from = at + 8;
        // Clamped: a streamed WAV's size fields lie (see above), and a
        // truncated file's run past the end — what there is is still music.
        let to = from.saturating_add(size).min(bytes.len());
        if id == b"fmt " && to - from >= 16 {
            let c = &bytes[from..to];
            let mut tag = u16::from_le_bytes([c[0], c[1]]);
            let channels = u16::from_le_bytes([c[2], c[3]]);
            let rate = u32::from_le_bytes([c[4], c[5], c[6], c[7]]);
            let bits = u16::from_le_bytes([c[14], c[15]]);
            // WAVE_FORMAT_EXTENSIBLE: the real tag leads the subformat GUID.
            if tag == 0xFFFE && c.len() >= 26 {
                tag = u16::from_le_bytes([c[24], c[25]]);
            }
            fmt = Some((tag, channels, rate, bits));
        }
        if id == b"data" {
            data = Some(&bytes[from..to]);
            // `data` is the last chunk either platform's decoder writes, and
            // a lying streamed size would walk `at` past real bytes.
            break;
        }
        // Chunks are word-aligned; an odd size carries a pad byte.
        at = from.saturating_add(size + (size & 1));
    }
    let Some((tag, channels, rate, bits)) = fmt else {
        return Err("the decoded audio does not say what format it is".to_owned());
    };
    let Some(data) = data else {
        return Err("the decoded file has no audio in it".to_owned());
    };
    if channels == 0 || rate == 0 {
        return Err("the decoded audio has an impossible format".to_owned());
    }
    let ch = channels as usize;
    let read: fn(&[u8]) -> f32 = match (tag, bits) {
        (3, 32) => |b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]),
        (1, 16) => |b| f32::from(i16::from_le_bytes([b[0], b[1]])) / 32768.0,
        (1, 24) => |b| {
            (i32::from_le_bytes([0, b[0], b[1], b[2]]) >> 8) as f32 / 8_388_608.0
        },
        (1, 32) => |b| i32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f32 / 2_147_483_648.0,
        _ => {
            return Err(format!(
                "the decoded audio came out as format {tag} at {bits} bits, \
                 which this cannot read"
            ));
        }
    };
    let step = (bits / 8) as usize;
    let frame = step * ch;
    let frames = data.len() / frame.max(1);
    let mut samples = Vec::with_capacity(frames * 2);
    for f in 0..frames {
        let row = &data[f * frame..(f + 1) * frame];
        let l = read(&row[0..step]);
        let r = if ch >= 2 { read(&row[step..2 * step]) } else { l };
        samples.push(l);
        samples.push(r);
    }
    Ok((samples, rate))
}

/// Linear resample of interleaved stereo, `from` Hz to `to` Hz.
///
/// Linear is the honest budget here: this path only runs when the decoder
/// DISOBEYED the rate it was asked for, which is a recovery, not the main
/// road — and linear at these rates is transparent next to the wrong-speed
/// playback it replaces.
fn resample_stereo(input: &[f32], from: u32, to: u32) -> Vec<f32> {
    let in_frames = input.len() / 2;
    if in_frames == 0 || from == 0 || to == 0 || from == to {
        return input.to_vec();
    }
    let out_frames = ((in_frames as u64 * u64::from(to)) / u64::from(from)) as usize;
    let mut out = Vec::with_capacity(out_frames * 2);
    let ratio = f64::from(from) / f64::from(to);
    for i in 0..out_frames {
        let pos = i as f64 * ratio;
        let i0 = (pos as usize).min(in_frames - 1);
        let i1 = (i0 + 1).min(in_frames - 1);
        let t = (pos - i0 as f64) as f32;
        for chan in 0..2 {
            let a = input[i0 * 2 + chan];
            let b = input[i1 * 2 + chan];
            out.push(a + (b - a) * t);
        }
    }
    out
}

/// Everywhere but macOS: the encoder the release already carries.
///
/// A WAV stream rather than bare `f32le`, so the container's `fmt ` chunk
/// carries what the decoder REALLY produced — `wav_stereo_f32` is where the
/// asked-for rate and layout stop being an assumption.
#[cfg(not(target_os = "macos"))]
fn read_wav(path: &Path, rate: u32) -> Result<Vec<u8>, String> {
    use std::io::Read;

    let program = crate::encode::ffmpeg::program();
    let mut child = crate::proc::command(&program)
        // `-nostdin`, or a child that decides it wants a terminal takes the
        // parent's and the app is left with no keyboard until it exits.
        .args(["-nostdin", "-v", "error", "-i"])
        .arg(path)
        .args([
            // The first AUDIO stream: somebody will import a video file, and
            // without this the default mapping picks its picture.
            "-map",
            "a:0",
            "-f",
            "wav",
            "-acodec",
            "pcm_f32le",
            "-ac",
            "2",
            "-ar",
            &rate.to_string(),
            "-",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            format!(
                "that file could not be read ({e}). Importing audio needs \
                 ffmpeg: {}",
                crate::encode::ffmpeg::how_to_install()
            )
        })?;

    let mut raw = Vec::new();
    if let Some(mut out) = child.stdout.take() {
        // Drained BEFORE waiting: a child whose pipe fills while the parent
        // waits on it is a deadlock, and a five-minute track is a great many
        // pipe buffers.
        out.read_to_end(&mut raw)
            .map_err(|e| format!("that file stopped part way through ({e})"))?;
    }
    let status = child
        .wait()
        .map_err(|e| format!("the decoder did not finish ({e})"))?;
    if !status.success() {
        let mut why = String::new();
        if let Some(mut err) = child.stderr.take() {
            let _ = err.read_to_string(&mut why);
        }
        let why = why.lines().last().unwrap_or("").trim().to_owned();
        return Err(if why.is_empty() {
            "that file is not audio this can read".to_owned()
        } else {
            format!("that file could not be read: {why}")
        });
    }
    Ok(raw)
}

/// macOS: `afconvert`, which is in `/usr/bin` on every Mac.
///
/// It writes a file rather than a stream — there is no stdout mode that works
/// across formats — so this goes via a temporary WAV and takes it away again.
/// `LEF32@rate` is little-endian 32-bit float; the whole WAV goes back to
/// `wav_stereo_f32`, whose walk is what steps over `afconvert`'s `FLLR`
/// padding chunk.
#[cfg(target_os = "macos")]
fn read_wav(path: &Path, rate: u32) -> Result<Vec<u8>, String> {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tmp = std::env::temp_dir().join(format!(
        "tangent-import-{}-{stamp}.wav",
        std::process::id()
    ));
    // Whatever happens below, the temporary file goes away.
    struct Sweep(PathBuf);
    impl Drop for Sweep {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    let _sweep = Sweep(tmp.clone());

    let out = crate::proc::command("/usr/bin/afconvert")
        .arg(path)
        .args(["-f", "WAVE", "-d"])
        .arg(format!("LEF32@{rate}"))
        .args(["-c", "2"])
        .arg(&tmp)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("that file could not be read ({e})"))?;
    if !out.status.success() {
        let why = String::from_utf8_lossy(&out.stderr);
        let why = why.lines().last().unwrap_or("").trim().to_owned();
        return Err(if why.is_empty() {
            "that file is not audio this can read".to_owned()
        } else {
            format!("that file could not be read: {why}")
        });
    }
    std::fs::read(&tmp).map_err(|e| format!("the decoded file went missing ({e})"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A hand-rolled WAV: `fmt ` then `data`, sizes as given (a streamed
    /// ffmpeg WAV writes `0xFFFFFFFF` where it could not seek back).
    fn wav_bytes(tag: u16, channels: u16, rate: u32, bits: u16, data: &[u8], lie: bool) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(b"RIFF");
        b.extend_from_slice(&if lie { u32::MAX } else { 36 + data.len() as u32 }.to_le_bytes());
        b.extend_from_slice(b"WAVE");
        b.extend_from_slice(b"fmt ");
        b.extend_from_slice(&16u32.to_le_bytes());
        b.extend_from_slice(&tag.to_le_bytes());
        b.extend_from_slice(&channels.to_le_bytes());
        b.extend_from_slice(&rate.to_le_bytes());
        let step = u32::from(bits / 8);
        b.extend_from_slice(&(rate * step * u32::from(channels)).to_le_bytes());
        b.extend_from_slice(&(step as u16 * channels).to_le_bytes());
        b.extend_from_slice(&bits.to_le_bytes());
        b.extend_from_slice(b"data");
        b.extend_from_slice(&if lie { u32::MAX } else { data.len() as u32 }.to_le_bytes());
        b.extend_from_slice(data);
        b
    }

    /// **The container is believed over the ask.** Float stereo comes
    /// through untouched; a STREAMED wav whose size fields lie (ffmpeg to a
    /// pipe) still parses; int16 MONO — a decoder that ignored both asks —
    /// comes out converted and doubled into both ears rather than as noise.
    #[test]
    fn the_wav_parse_reads_what_was_really_produced() {
        let f32s: Vec<u8> = [0.5f32, -0.5, 0.25, -0.25]
            .iter()
            .flat_map(|s| s.to_le_bytes())
            .collect();
        let (s, rate) = wav_stereo_f32(&wav_bytes(3, 2, 48_000, 32, &f32s, false)).expect("f32");
        assert_eq!(rate, 48_000);
        assert_eq!(s, vec![0.5, -0.5, 0.25, -0.25]);

        let (s, _) = wav_stereo_f32(&wav_bytes(3, 2, 48_000, 32, &f32s, true)).expect("streamed");
        assert_eq!(s.len(), 4, "the lying sizes truncated the data");

        let i16s: Vec<u8> = [16384i16, -16384]
            .iter()
            .flat_map(|s| s.to_le_bytes())
            .collect();
        let (s, rate) = wav_stereo_f32(&wav_bytes(1, 1, 44_100, 16, &i16s, false)).expect("i16");
        assert_eq!(rate, 44_100);
        assert_eq!(s.len(), 4, "mono did not double into stereo");
        assert!((s[0] - 0.5).abs() < 1e-3 && (s[1] - 0.5).abs() < 1e-3);
        assert!((s[2] + 0.5).abs() < 1e-3);

        assert!(
            wav_stereo_f32(&wav_bytes(85, 2, 48_000, 32, &f32s, false))
                .unwrap_err()
                .contains("format 85"),
            "an unknown format must name itself"
        );
    }

    /// **A disobeyed rate is resampled, not stamped.** A one-second tone
    /// declared at 24k and asked for at 48k comes out one second long at
    /// 48k — the wrong-speed export this replaces played it at half speed.
    #[test]
    fn a_disobeyed_rate_is_resampled_to_the_ask() {
        let n = 24_000usize;
        let input: Vec<f32> = (0..n * 2).map(|i| (i / 2) as f32 / n as f32).collect();
        let out = resample_stereo(&input, 24_000, 48_000);
        assert_eq!(out.len(), n * 4, "one second at 48k is 48k frames");
        // The ramp survives: halfway through is about half.
        let mid = out[out.len() / 2];
        assert!((mid - 0.5).abs() < 0.01, "halfway was {mid}");
    }

    /// **A file goes in and stereo float comes out at the rate asked for.**
    ///
    /// Run against a real file through whatever this platform decodes with,
    /// because the thing being tested is the subprocess contract — the flags,
    /// the byte order and, on macOS, the chunk walk that a fixed offset gets
    /// wrong.
    #[test]
    fn a_wav_round_trips_through_the_platform_decoder() {
        // A second of a 440 Hz tone, written with this crate's own writer.
        let dir = std::env::temp_dir().join(format!("tangent-decode-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("tone.wav");
        let rate = 48_000u32;
        let frames = rate as usize;
        let mut pcm = Vec::with_capacity(frames * 2);
        for f in 0..frames {
            let x = (std::f32::consts::TAU * 440.0 * f as f32 / rate as f32).sin() * 0.5;
            pcm.push(x);
            pcm.push(x);
        }
        let spec = crate::wav::WavSpec {
            sample_rate: rate,
            channels: 2,
            format: crate::wav::SampleFormat::Float32,
        };
        let bext = crate::wav::Bext::new(crate::wav::Wallclock::now_utc(), spec);
        match crate::wav::WavWriter::create(&path, spec, &bext) {
            Ok(mut w) => {
                w.write_interleaved(&pcm).expect("write");
                w.finish().expect("finish");
            }
            Err(e) => {
                eprintln!("no writer here ({e}); skipping");
                return;
            }
        }

        let clip = match decode(&path, rate) {
            Ok(c) => c,
            Err(e) => {
                // No decoder on this machine is a real answer, not a failure:
                // the Linux CI box may have neither ffmpeg nor a bundled one.
                eprintln!("no decoder here ({e}); skipping");
                let _ = std::fs::remove_dir_all(&dir);
                return;
            }
        };
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(clip.rate, rate);
        assert!(
            (clip.seconds() - 1.0).abs() < 0.02,
            "a one-second file decoded to {:.3}s",
            clip.seconds()
        );
        let peak = clip.samples.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(
            (peak - 0.5).abs() < 0.02,
            "a half-scale tone came back at {peak:.3} - the byte order or the \
             chunk offset is wrong"
        );
        // Not silence in the first millisecond: that is what a `data` chunk
        // read from the wrong offset looks like.
        let head = clip.samples[..96].iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(head > 0.01, "the decode started inside the header");
    }

    /// The envelope is the outline, and it is as long as it was asked for.
    #[test]
    fn the_envelope_follows_the_loudest_sample() {
        let clip = Clip {
            // Quiet, then loud, then quiet.
            samples: (0..3000)
                .flat_map(|i| {
                    let v = if (1000..2000).contains(&i) { 0.8 } else { 0.05 };
                    [v, v]
                })
                .collect(),
            rate: 48_000,
            source: PathBuf::from("x.wav"),
        };
        let env = clip.envelope(3);
        assert_eq!(env.len(), 3);
        assert!(env[0] < 0.1 && env[2] < 0.1, "{env:?}");
        assert!(env[1] > 0.7, "the loud third did not show: {env:?}");
        // Degenerate asks answer with nothing rather than dividing by zero.
        assert!(clip.envelope(0).is_empty());
    }
}

/// **decode**: what a take costs to read through the platform decoder
/// (`afconvert` on macOS, ffmpeg elsewhere) against the native road this
/// file already has — `std::fs::read` plus `wav_stereo_f32`, which parses
/// 16/24/32-bit PCM and float WAV on its own. Ignored; run in RELEASE:
///
/// ```text
/// cargo test --release -p tangent-record --lib perf_decode -- --ignored --nocapture
/// ```
///
/// The files are what the recorder writes: 24-bit PCM, stereo, 48 kHz,
/// through this crate's own `WavWriter`.
#[cfg(test)]
mod perf_probe {
    use super::*;
    use std::time::{Duration, Instant};

    fn per_call(n: u32, mut f: impl FnMut()) -> Duration {
        let t = Instant::now();
        for _ in 0..n {
            f();
        }
        t.elapsed() / n.max(1)
    }

    /// A take of `seconds`: decaying sine bursts, a note every half second,
    /// written a block at a time.
    fn write_take(path: &Path, seconds: f64, rate: u32) -> u64 {
        let spec = crate::wav::WavSpec {
            sample_rate: rate,
            channels: 2,
            format: crate::wav::SampleFormat::Int24,
        };
        let bext = crate::wav::Bext::new(crate::wav::Wallclock::now_utc(), spec);
        let mut w = crate::wav::WavWriter::create(path, spec, &bext).expect("writer");
        let frames = (seconds * f64::from(rate)) as usize;
        let mut block = vec![0.0f32; 4_800 * 2];
        let mut at = 0usize;
        while at < frames {
            let n = (frames - at).min(4_800);
            for k in 0..n {
                let t = (at + k) as f64 / f64::from(rate);
                let into = t - (t * 2.0).floor() / 2.0;
                let env = 0.7 * (-into * 5.0).exp();
                let ph = std::f64::consts::TAU * 220.0 * into;
                let v = env * (ph.sin() + 0.4 * (2.0 * ph).sin());
                block[k * 2] = v as f32;
                block[k * 2 + 1] = (v * 0.9) as f32;
            }
            w.write_interleaved(&block[..n * 2]).expect("write");
            at += n;
        }
        let bytes = w.total_bytes();
        w.finish().expect("finish");
        bytes
    }

    #[test]
    #[ignore]
    fn perf_decode() {
        let build = if cfg!(debug_assertions) { "DEBUG" } else { "release" };
        println!(
            "decode [{build}]: tangent_record::decode::decode (platform decoder) vs fs::read + wav_stereo_f32 (native), 24-bit stereo 48 kHz"
        );
        let rate = 48_000u32;
        let dir = std::env::temp_dir().join(format!("tangent-perf-decode-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        for (seconds, n) in [(5.0, 5u32), (60.0, 3), (180.0, 2)] {
            let path = dir.join(format!("take-{seconds}s.wav"));
            let bytes = write_take(&path, seconds, rate);
            let platform = per_call(n, || {
                let clip = decode(&path, rate).expect("the platform decoder");
                std::hint::black_box(clip);
            });
            let native = per_call(n, || {
                let raw = std::fs::read(&path).expect("read");
                let (samples, got) = wav_stereo_f32(&raw).expect("parse");
                assert_eq!(got, rate);
                std::hint::black_box(samples);
            });
            let read_only = per_call(n, || {
                std::hint::black_box(std::fs::read(&path).expect("read"));
            });
            // Both roads must produce the same audio, or the ratio compares
            // two different jobs.
            let a = decode(&path, rate).expect("decode");
            let raw = std::fs::read(&path).expect("read");
            let (b, _) = wav_stereo_f32(&raw).expect("parse");
            assert_eq!(a.rate, rate);
            assert_eq!(a.samples.len(), b.len(), "{seconds}s: the two roads disagree on length");
            let dev = a
                .samples
                .iter()
                .zip(&b)
                .map(|(x, y)| (x - y).abs())
                .fold(0.0f32, f32::max);
            println!(
                "decode       {seconds:>5.0} s ({:>6.1} MB): platform {platform:>10.3?} | native {native:>10.3?} (of which fs::read {read_only:>9.3?}) | {:>5.1}x | max sample deviation {dev:e}  [{build}]",
                bytes as f64 / 1_048_576.0,
                platform.as_secs_f64() / native.as_secs_f64()
            );
            assert!(dev < 1e-6, "{seconds}s: the decoders disagree by {dev}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
