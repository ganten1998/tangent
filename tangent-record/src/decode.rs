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
//!
//! **And a third road in front of both**: a stereo WAV already at the asked
//! rate — which is every take this app has ever written — is read and parsed
//! here without a child at all. See `read_native` for the gates; the two
//! tools above remain the answer for everything else.

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
pub const MAX_FRAMES: usize = 20 * 60 * 48_000;

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
    // **The file itself first, the child only for what the file is not.**
    // See `read_native`: a take is a plain WAV this file already parses, and
    // the child was costing nineteen milliseconds and a temp file per take
    // to produce the same bytes.
    let (samples, got) = match read_native(path, rate) {
        Some(native) => native,
        None => {
            let wav = read_wav(path, rate)?;
            wav_stereo_f32(&wav)?
        }
    };
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

/// The most bytes the native road will read into memory at all:
/// [`MAX_FRAMES`] of 32-bit stereo plus room for the chunks in front of
/// `data`. Anything larger is refused on its size alone, before a byte of it
/// is read, which is the one thing the child road could never do — it had to
/// convert the whole file to learn its length.
const NATIVE_MAX_BYTES: u64 = (MAX_FRAMES as u64) * 2 * 4 + (1 << 20);

/// How much of a file the native road reads before it decides whether to
/// read the rest: the RIFF header, a `bext` (602 bytes fixed, plus a coding
/// history), the `fmt ` chunk, and whatever small chunks a writer puts
/// between them.
///
/// **This used to be the whole file.** `read_native` did an `fs::read` of
/// everything up to [`NATIVE_MAX_BYTES`] and only then looked at byte zero —
/// so an imported MP3, AIFF or MP4 under that size was pulled through memory
/// in full, declined at its first four bytes, and handed to the child to be
/// read again from the start. Twelve bytes decide those; a WAV at another
/// rate or width is decided at its `fmt `; only a file the header does not
/// rule out is read to the end. See [`peek_declines`].
const NATIVE_PEEK_BYTES: usize = 16 * 1024;

// How many children `read_wav` has started on this thread. A test's proof
// that a take never left the process; a `Cell` rather than a `cfg(test)`
// static because `proc.rs` scans this file's runtime text up to its first
// `#[cfg(test)]`, and a gate placed above `read_wav` would hide `read_wav`
// from that scan. Read by the tests through `tests::child_spawns`.
thread_local! {
    static CHILD_SPAWNS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

fn note_child_spawn() {
    CHILD_SPAWNS.with(|c| c.set(c.get().saturating_add(1)));
}

/// **The native road: read the file and parse it here, when it is a WAV this
/// file can already read at the rate that was asked for.** `None` means
/// "not this way" — the child road follows, exactly as before.
///
/// Every media file in a real project is a take this app wrote: RIFF/WAVE,
/// `bext` first, tag-1 or float `fmt `, `data` last, at the engine's rate.
/// `wav_stereo_f32` has parsed that shape for as long as it has existed,
/// because it is the shape `afconvert` and ffmpeg hand back — so the child
/// was being spawned to convert a file into the file it already was.
/// Measured on a 60 s 24-bit take: 37 ms through `afconvert` (19 ms of it
/// fixed: spawn, a 23 MB temp file, and reading it back), 7 ms here, the
/// samples bit-identical. Forty-five takes is 1.7 s against 0.3 s.
///
/// **What still goes to the child, and why each:**
/// * a rate other than the ask — `resample_stereo` is linear, a RECOVERY for
///   a decoder that disobeyed, not a converter; the child resamples properly;
/// * a channel count other than two — the child's mono-to-stereo and
///   downmix gains are its own, and the doubling here has not been measured
///   against them;
/// * a file longer than [`NATIVE_MAX_BYTES`] on disk — refused unread, the
///   child then produces the "that file is N minutes long" sentence as it
///   always has;
/// * anything the first [`NATIVE_PEEK_BYTES`] already rule out — see
///   [`peek_declines`]: an MP3, an AIFF or an MP4 costs one small read here
///   and not a pass through memory before the child reads it again;
/// * anything the STRICT parse refuses — not RIFF (AIFF, CAF, MP3, RF64),
///   a format the parser has no arm for, and a chunk that claims more bytes
///   than the file holds. That last one is a take that was cut short: the
///   tolerant parse would return the bytes that are there as if they were
///   the whole file, and a clip silently a minute short is worse than one
///   the child either reads the same way or refuses out loud.
fn read_native(path: &Path, rate: u32) -> Option<(Vec<f32>, u32)> {
    use std::io::Read;

    let len = std::fs::metadata(path).ok()?.len();
    if len > NATIVE_MAX_BYTES {
        return None;
    }
    let mut file = std::fs::File::open(path).ok()?;
    // The header first, and only as much of it as a decision needs.
    let mut bytes = Vec::new();
    (&mut file)
        .take(NATIVE_PEEK_BYTES as u64)
        .read_to_end(&mut bytes)
        .ok()?;
    if peek_declines(&bytes, rate) {
        return None;
    }
    // The rest, behind the header already in hand. `len` sizes the one
    // allocation; a file that grew since `metadata` merely reallocates.
    bytes.reserve((len as usize).saturating_sub(bytes.len()));
    file.read_to_end(&mut bytes).ok()?;
    let (samples, got, channels) = parse_wav(&bytes, true).ok()?;
    if got != rate || channels != 2 {
        return None;
    }
    Some((samples, got))
}

/// **Whether the first bytes of a file already rule the native road out**,
/// asked of the [`NATIVE_PEEK_BYTES`] read before the rest of the file is.
///
/// `true` is a decision: not RIFF/WAVE at all (an MP3, an AIFF, an MP4, an
/// RF64), or a `fmt ` chunk inside the peek that names another rate, another
/// channel count, or a format tag the parser has no arm for. `false` is NOT
/// an admission — it also covers a `fmt ` that lies past the peek or is cut
/// by its edge, and a file with no `fmt ` before its `data` — because every
/// strict rule is still applied by [`parse_wav`] on the whole file
/// afterwards: the chunk sizes against the file, the ceiling, the sample
/// width, the extensible subformat. This only says no early; it never says
/// yes, so nothing the strict walk would refuse gets in by being short.
fn peek_declines(head: &[u8], rate: u32) -> bool {
    if head.len() < 12 || &head[0..4] != b"RIFF" || &head[8..12] != b"WAVE" {
        return true;
    }
    let mut at = 12usize;
    while at + 8 <= head.len() {
        let id = &head[at..at + 4];
        let size = u32::from_le_bytes([head[at + 4], head[at + 5], head[at + 6], head[at + 7]])
            as usize;
        let from = at + 8;
        let to = from.saturating_add(size).min(head.len());
        if id == b"fmt " {
            let c = &head[from..to];
            // Cut by the peek's edge, or too short to be a format at all:
            // the whole read decides.
            if c.len() < 16 {
                return false;
            }
            let mut tag = u16::from_le_bytes([c[0], c[1]]);
            let channels = u16::from_le_bytes([c[2], c[3]]);
            let got = u32::from_le_bytes([c[4], c[5], c[6], c[7]]);
            // WAVE_FORMAT_EXTENSIBLE: the real tag leads the subformat GUID,
            // exactly as `parse_wav` reads it. Cut short, the whole read
            // decides.
            if tag == 0xFFFE {
                if c.len() < 26 {
                    return false;
                }
                tag = u16::from_le_bytes([c[24], c[25]]);
            }
            return !(matches!(tag, 1 | 3) && channels == 2 && got == rate);
        }
        if id == b"data" {
            // Audio before any `fmt `: the strict walk refuses that itself.
            return false;
        }
        // Chunks are word-aligned; an odd size carries a pad byte.
        at = from.saturating_add(size).saturating_add(size & 1);
    }
    false
}

/// Interleaved stereo f32 out of a RIFF/WAVE byte string, plus the rate its
/// own `fmt ` chunk states — the TOLERANT read, for what a child produced.
///
/// **Walked rather than assumed at offset 44.** `afconvert` writes an `FLLR`
/// padding chunk before `data` to page-align it, and ffmpeg piping to stdout
/// writes `0xFFFFFFFF` for the sizes it could not seek back to fill in — the
/// walk tolerates both. Mono is doubled into both ears; anything wider than
/// stereo keeps its first pair (the decode asked for two, so this is a
/// decoder that did not listen); int16/24/32 are converted, because a
/// decoder that ignored the codec ask should still come out as music.
fn wav_stereo_f32(bytes: &[u8]) -> Result<(Vec<f32>, u32), String> {
    parse_wav(bytes, false).map(|(samples, rate, _)| (samples, rate))
}

/// The walk behind [`wav_stereo_f32`], with its `strict` face for
/// [`read_native`]: a chunk whose declared size runs past the end of the
/// bytes is refused rather than clamped, and a `data` chunk past
/// [`MAX_FRAMES`] is refused before a single sample is allocated. Returns
/// the source channel count beside the samples, so the native road can
/// tell a real stereo file from a doubled mono one.
fn parse_wav(bytes: &[u8], strict: bool) -> Result<(Vec<f32>, u32, u16), String> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err("the decoder did not produce audio".to_owned());
    }
    let mut fmt: Option<(u16, u16, u32, u16)> = None; // tag, channels, rate, bits
    let mut data: Option<&[u8]> = None;
    let mut at = 12usize;
    while at + 8 <= bytes.len() {
        let id = &bytes[at..at + 4];
        let size = u32::from_le_bytes([bytes[at + 4], bytes[at + 5], bytes[at + 6], bytes[at + 7]])
            as usize;
        let from = at + 8;
        let end = from.saturating_add(size);
        if strict && end > bytes.len() {
            return Err("the file is shorter than its header says it is".to_owned());
        }
        // Clamped: a streamed WAV's size fields lie (see above), and a
        // truncated file's run past the end — what there is is still music.
        let to = end.min(bytes.len());
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
        at = from.saturating_add(size).saturating_add(size & 1);
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
    if strict && data.len() / (usize::from(bits / 8).max(1) * ch) > MAX_FRAMES {
        // The child road says how long it is; this road only declines.
        return Err("longer than the native road reads".to_owned());
    }
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
    Ok((samples, rate, channels))
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

    note_child_spawn();
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

    note_child_spawn();
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

    /// Children `read_wav` has started on the calling thread.
    fn child_spawns() -> usize {
        CHILD_SPAWNS.with(std::cell::Cell::get)
    }

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

    /// A second of a 440 Hz tone at `rate`, written with this crate's own
    /// writer in `format`, under a directory of the test's own.
    fn write_tone(name: &str, rate: u32, format: crate::wav::SampleFormat) -> Option<(PathBuf, Vec<f32>)> {
        let dir = std::env::temp_dir().join(format!("tangent-decode-{}-{name}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("tone.wav");
        let frames = rate as usize;
        let mut pcm = Vec::with_capacity(frames * 2);
        for f in 0..frames {
            let x = (std::f32::consts::TAU * 440.0 * f as f32 / rate as f32).sin() * 0.5;
            pcm.push(x);
            pcm.push(x * 0.75);
        }
        let spec = crate::wav::WavSpec {
            sample_rate: rate,
            channels: 2,
            format,
        };
        let bext = crate::wav::Bext::new(crate::wav::Wallclock::now_utc(), spec);
        match crate::wav::WavWriter::create(&path, spec, &bext) {
            Ok(mut w) => {
                w.write_interleaved(&pcm).expect("write");
                w.finish().expect("finish");
                Some((path, pcm))
            }
            Err(e) => {
                eprintln!("no writer here ({e}); skipping");
                None
            }
        }
    }

    fn sweep(path: &Path) {
        if let Some(dir) = path.parent() {
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    /// **A file goes in and stereo float comes out at the rate asked for.**
    ///
    /// Run against a real file through whatever this platform decodes with,
    /// because the thing being tested is the subprocess contract — the flags,
    /// the byte order and, on macOS, the chunk walk that a fixed offset gets
    /// wrong. The file is at 44.1 k and the ask is 48 k ON PURPOSE: a file
    /// already at the asked rate never reaches the child any more (see
    /// `read_native`), and this test exists to exercise the child.
    #[test]
    fn a_wav_round_trips_through_the_platform_decoder() {
        let Some((path, _)) = write_tone("child", 44_100, crate::wav::SampleFormat::Float32) else {
            return;
        };
        let before = child_spawns();
        let rate = 48_000u32;
        let clip = match decode(&path, rate) {
            Ok(c) => c,
            Err(e) => {
                // No decoder on this machine is a real answer, not a failure:
                // the Linux CI box may have neither ffmpeg nor a bundled one.
                eprintln!("no decoder here ({e}); skipping");
                sweep(&path);
                return;
            }
        };
        sweep(&path);
        assert_eq!(child_spawns(), before + 1, "a rate mismatch has to go to the child");

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

    /// **A take as this app writes it never leaves the process.** Both
    /// shapes the app writes — the timeline's float take and the recorder's
    /// 24-bit one, each with its `bext` in front — decode with no child, and
    /// the samples are what the writer was given (to a 24-bit step for the
    /// integer one).
    #[test]
    fn a_take_as_the_app_writes_it_decodes_without_a_child() {
        for (name, format, step) in [
            ("float", crate::wav::SampleFormat::Float32, 0.0f32),
            ("int24", crate::wav::SampleFormat::Int24, 1.0 / 8_388_608.0),
        ] {
            let Some((path, pcm)) = write_tone(name, 48_000, format) else {
                return;
            };
            let before = child_spawns();
            let clip = decode(&path, 48_000).expect("the native road reads a take");
            sweep(&path);
            assert_eq!(child_spawns(), before, "{name}: a take spawned a child");
            assert_eq!(clip.rate, 48_000);
            assert_eq!(clip.samples.len(), pcm.len(), "{name}: length");
            let worst = clip
                .samples
                .iter()
                .zip(&pcm)
                .map(|(a, b)| (a - b).abs())
                .fold(0.0f32, f32::max);
            assert!(worst <= step * 1.01, "{name}: samples drifted by {worst}");
            assert_eq!(clip.source, path);
        }
    }

    /// **What the native road declines still reaches the child**, so nothing
    /// that decoded before stops decoding: bytes that are not a WAV, an RF64
    /// header, a WAV at another rate, a mono WAV, and a take cut short —
    /// whose header claims more `data` than the file holds. Each is counted
    /// as a spawn; whether the child then succeeds depends on the machine
    /// and is not this test's question.
    #[test]
    fn what_the_native_road_declines_goes_to_the_child() {
        let dir = std::env::temp_dir().join(format!("tangent-decode-decline-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let spawn_for = |name: &str, bytes: &[u8]| {
            let path = dir.join(name);
            std::fs::write(&path, bytes).expect("write");
            let before = child_spawns();
            let _ = decode(&path, 48_000);
            assert_eq!(child_spawns(), before + 1, "{name} did not reach the child");
        };
        spawn_for("garbage.bin", b"this is not a wav file at all, not even close");
        let f32s: Vec<u8> = [0.5f32, -0.5, 0.25, -0.25]
            .iter()
            .flat_map(|s| s.to_le_bytes())
            .collect();
        let mut rf64 = wav_bytes(3, 2, 48_000, 32, &f32s, false);
        rf64[0..4].copy_from_slice(b"RF64");
        spawn_for("rf64.wav", &rf64);
        spawn_for("mono.wav", &wav_bytes(3, 1, 48_000, 32, &f32s, false));
        spawn_for("other-rate.wav", &wav_bytes(3, 2, 44_100, 32, &f32s, false));
        // A streamed writer's lying sizes are the child's business too.
        spawn_for("streamed.wav", &wav_bytes(3, 2, 48_000, 32, &f32s, true));
        // The take cut short: a real header, patched to say 4 frames, with
        // only 2 on disk behind it.
        let mut short = wav_bytes(3, 2, 48_000, 32, &f32s, false);
        short.truncate(short.len() - 8);
        spawn_for("short.wav", &short);
        // ...which the TOLERANT parse still reads as far as it goes, because
        // that is what a child's own streamed output looks like.
        let (got, _) = wav_stereo_f32(&short).expect("tolerant");
        assert_eq!(got.len(), 2, "the tolerant walk keeps what is there");
        assert!(
            parse_wav(&short, true).is_err(),
            "the strict walk must not pass a short file off as whole"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **The peek says no from the first bytes, and never says yes.** An
    /// MP3's ID3 tag, an AIFF's `FORM`, an MP4's `ftyp` and an RF64 header
    /// are declined at twelve bytes; a WAV at another rate or width, or in
    /// a format the parser has no arm for, at its `fmt `. A stereo WAV at
    /// the asked rate — plain or extensible — is not declined, and neither
    /// is a header whose `fmt ` lies past the peek or is cut by its edge,
    /// because those are the whole read's to decide; a truncated `fmt ` of
    /// the app's own shape is the same. What this pins is that the road
    /// that used to read the whole file first now reads it only for a file
    /// its header does not rule out.
    #[test]
    fn the_peek_declines_early_and_admits_nothing() {
        let f32s: Vec<u8> = [0.5f32, -0.5, 0.25, -0.25]
            .iter()
            .flat_map(|s| s.to_le_bytes())
            .collect();
        assert!(peek_declines(b"ID3\x04\x00\x00\x00\x00\x00\x00\xff\xfb", 48_000), "an MP3");
        assert!(peek_declines(b"FORM\x00\x00\x00\x00AIFF", 48_000), "an AIFF");
        assert!(peek_declines(b"\x00\x00\x00\x18ftypmp42", 48_000), "an MP4");
        assert!(peek_declines(b"RIFF", 48_000), "too short to be anything");
        let mut rf64 = wav_bytes(3, 2, 48_000, 32, &f32s, false);
        rf64[0..4].copy_from_slice(b"RF64");
        assert!(peek_declines(&rf64, 48_000), "RF64");
        assert!(peek_declines(&wav_bytes(3, 2, 44_100, 32, &f32s, false), 48_000), "another rate");
        assert!(peek_declines(&wav_bytes(3, 1, 48_000, 32, &f32s, false), 48_000), "mono");
        assert!(peek_declines(&wav_bytes(85, 2, 48_000, 32, &f32s, false), 48_000), "no arm for 85");
        assert!(!peek_declines(&wav_bytes(3, 2, 48_000, 32, &f32s, false), 48_000), "float stereo");
        assert!(!peek_declines(&wav_bytes(1, 2, 48_000, 24, &f32s, false), 48_000), "int24 stereo");
        // The streamed writer's lying sizes are the strict walk's business,
        // not the peek's: its `fmt ` is honest.
        assert!(!peek_declines(&wav_bytes(3, 2, 48_000, 32, &f32s, true), 48_000), "streamed");
        // Extensible: a 40-byte `fmt ` whose real tag leads the GUID.
        let mut ext = Vec::new();
        ext.extend_from_slice(b"RIFF\xff\xff\xff\xffWAVEfmt ");
        ext.extend_from_slice(&40u32.to_le_bytes());
        ext.extend_from_slice(&0xFFFEu16.to_le_bytes());
        ext.extend_from_slice(&2u16.to_le_bytes());
        ext.extend_from_slice(&48_000u32.to_le_bytes());
        ext.extend_from_slice(&[0u8; 6]); // byte rate, block align
        ext.extend_from_slice(&32u16.to_le_bytes());
        ext.extend_from_slice(&22u16.to_le_bytes()); // cbSize
        ext.extend_from_slice(&32u16.to_le_bytes()); // valid bits
        ext.extend_from_slice(&0u32.to_le_bytes()); // channel mask
        ext.extend_from_slice(&3u16.to_le_bytes()); // the subformat's tag: float
        ext.extend_from_slice(&[0u8; 14]);
        assert!(!peek_declines(&ext, 48_000), "extensible float stereo at the rate");
        let mut ext_mono = ext.clone();
        ext_mono[22..24].copy_from_slice(&1u16.to_le_bytes());
        assert!(peek_declines(&ext_mono, 48_000), "extensible mono");
        // Cut by the peek's edge before the subformat: undecided.
        assert!(!peek_declines(&ext[..12 + 8 + 20], 48_000), "a cut extensible fmt is the whole read's");
        // A `fmt ` past the peek — a megabyte of LIST first — is undecided
        // too: the walk steps over what it cannot see.
        let mut far = Vec::new();
        far.extend_from_slice(b"RIFF\xff\xff\xff\xffWAVELIST");
        far.extend_from_slice(&(1u32 << 20).to_le_bytes());
        far.extend_from_slice(&[0u8; 64]);
        assert!(!peek_declines(&far, 48_000), "a fmt past the peek is the whole read's");
    }

    /// The strict walk refuses a `data` chunk past the ceiling before it
    /// allocates a sample for it, and the size check cannot overflow on a
    /// chunk that claims the whole 32-bit range.
    #[test]
    fn the_strict_walk_declines_the_ceiling_before_allocating() {
        // A header alone whose data chunk claims 4 GiB: strict refuses it as
        // short (the bytes are not there), tolerant reads zero frames.
        let mut b = wav_bytes(3, 2, 48_000, 32, &[], false);
        let n = b.len();
        b[n - 4..].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(parse_wav(&b, true).is_err());
        let (s, _) = wav_stereo_f32(&b).expect("tolerant");
        assert!(s.is_empty());
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

/// **decode**: what a take costs to read through `decode()` — the native
/// road for a stereo WAV at the asked rate, the platform child for the
/// rest — against the child road alone. Ignored; run in RELEASE:
///
/// ```text
/// cargo test --release -p tangent-record --lib perf_probe::perf_decode -- --ignored --nocapture
/// ```
///
/// The files are what the recorder writes: 24-bit PCM, stereo, 48 kHz,
/// through this crate's own `WavWriter`. The child road is measured by
/// asking for a rate the file is not at, which is the one gate every take
/// passes and no `afconvert` invocation can skip; its number therefore
/// includes a resample the native road never does, and reads as an upper
/// bound on what a take used to cost, not an exact one.
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
    /// written a block at a time. Answers the bytes on disk.
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
            "decode [{build}]: decode() on a 24-bit stereo 48 kHz take (native road) vs the platform child (asked at 44.1 kHz, so it resamples too)"
        );
        let rate = 48_000u32;
        let dir = std::env::temp_dir().join(format!("tangent-perf-decode-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        for (seconds, n) in [(5.0, 5u32), (60.0, 3), (180.0, 2)] {
            let path = dir.join(format!("take-{seconds}s.wav"));
            let bytes = write_take(&path, seconds, rate);
            let native = per_call(n, || {
                let clip = decode(&path, rate).expect("the native road");
                std::hint::black_box(clip);
            });
            let child = per_call(n, || {
                // No decoder on this machine: the column reads as the cost
                // of finding that out.
                let got = decode(&path, 44_100).map(|c| c.samples.len()).unwrap_or(0);
                std::hint::black_box(got);
            });
            let read_only = per_call(n, || {
                std::hint::black_box(std::fs::read(&path).expect("read"));
            });
            println!(
                "decode       {seconds:>5.0} s ({:>6.1} MB): native {native:>10.3?} (of which fs::read {read_only:>9.3?}) | child+resample {child:>10.3?} | {:>5.1}x  [{build}]",
                bytes as f64 / 1_048_576.0,
                child.as_secs_f64() / native.as_secs_f64().max(1e-9)
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
