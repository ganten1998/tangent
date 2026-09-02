//! A WAV reader, by hand, because the app has no WAV crate and must not gain
//! one for a loader — the same law the keysrig bench's writer follows from
//! the other side. It exists for exactly one caller: the IR unit hands a
//! saved `ir_path` here and wants planar f32 back, whatever bit depth the
//! capture tool wrote.
//!
//! Formats: PCM at 16, 24 and 32 bits, and 32-bit float, both plain and
//! wrapped in WAVE_FORMAT_EXTENSIBLE. Any channel count comes back planar;
//! the CALLER decides how many it can use, because "this file is 5.1" is a
//! sentence about the caller's layout, not about whether the file is a WAV.
//!
//! Chunk-walked rather than offset-assumed: real capture tools drop `bext`,
//! `LIST` and cue chunks between `fmt ` and `data`, and a reader that
//! hard-codes offset 44 reads somebody's metadata as audio.

use std::path::Path;

/// What a WAV holds, decoded: one `Vec` per channel, all the same length.
pub struct WavData {
    /// The file's own sample rate — the caller compares it to the engine's
    /// and decides whether the mismatch is worth a sentence.
    pub rate: u32,
    pub channels: Vec<Vec<f32>>,
}

/// The two format tags this reader speaks, plus the extensible wrapper that
/// restates one of them from inside its GUID.
const FORMAT_PCM: u16 = 1;
const FORMAT_FLOAT: u16 = 3;
const FORMAT_EXTENSIBLE: u16 = 0xFFFE;

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

/// Read and decode one WAV file. Errors are sentences for the load-error
/// path, never panics — the path came out of a session file somebody may
/// have edited, moved or truncated.
pub fn read(path: &Path) -> Result<WavData, String> {
    let bytes = std::fs::read(path)
        .map_err(|e| format!("{} could not be read: {e}", path.display()))?;
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err(format!("{} is not a WAV file", path.display()));
    }

    // The chunk walk. Sizes are trusted only as far as the file actually
    // goes: a truncated size field must end the walk, not slice past the end.
    let mut format = 0u16;
    let mut channels = 0usize;
    let mut rate = 0u32;
    let mut bits = 0u16;
    let mut data: Option<&[u8]> = None;
    let mut at = 12usize;
    while at + 8 <= bytes.len() {
        let id = &bytes[at..at + 4];
        let size = u32_at(&bytes, at + 4).unwrap_or(0) as usize;
        let body = bytes.get(at + 8..(at + 8).saturating_add(size).min(bytes.len()));
        let Some(body) = body else { break };
        match id {
            b"fmt " => {
                format = u16_at(body, 0).unwrap_or(0);
                channels = u16_at(body, 2).unwrap_or(0) as usize;
                rate = u32_at(body, 4).unwrap_or(0);
                bits = u16_at(body, 14).unwrap_or(0);
                // The extensible wrapper's REAL format tag is the first two
                // bytes of the sub-format GUID, at offset 24 of the chunk.
                if format == FORMAT_EXTENSIBLE {
                    format = u16_at(body, 24).unwrap_or(0);
                }
            }
            b"data" => data = Some(body),
            _ => {}
        }
        // Chunks are word-aligned: an odd size carries one pad byte.
        at = at + 8 + size + (size & 1);
    }

    let Some(data) = data else {
        return Err(format!("{} has no data chunk", path.display()));
    };
    if channels == 0 || rate == 0 {
        return Err(format!("{} declares no channels or no rate", path.display()));
    }
    let bytes_per = usize::from(bits / 8);
    let decode: fn(&[u8]) -> f32 = match (format, bits) {
        (FORMAT_PCM, 16) => |b| f32::from(i16::from_le_bytes([b[0], b[1]])) / 32_768.0,
        (FORMAT_PCM, 24) => |b| {
            // Sign-extend three little-endian bytes through the top of an i32.
            (i32::from_le_bytes([0, b[0], b[1], b[2]]) >> 8) as f32 / 8_388_608.0
        },
        (FORMAT_PCM, 32) => |b| {
            i32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f32 / 2_147_483_648.0
        },
        (FORMAT_FLOAT, 32) => |b| {
            // A float file can hold anything, including the NaN that would
            // poison a convolution forever. Silence is the honest reading.
            let v = f32::from_le_bytes([b[0], b[1], b[2], b[3]]);
            if v.is_finite() { v } else { 0.0 }
        },
        _ => {
            return Err(format!(
                "{} is {bits}-bit format {format}, which this reader does not speak \
                 (16/24/32-bit PCM and 32-bit float do)",
                path.display()
            ))
        }
    };

    let frame = bytes_per * channels;
    let frames = if frame == 0 { 0 } else { data.len() / frame };
    // Not the `vec!` macro: cloning an empty Vec forgets its capacity.
    let mut out: Vec<Vec<f32>> = (0..channels).map(|_| Vec::with_capacity(frames)).collect();
    for f in 0..frames {
        for (c, ch) in out.iter_mut().enumerate() {
            ch.push(decode(&data[f * frame + c * bytes_per..]));
        }
    }
    Ok(WavData { rate, channels: out })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A WAV built byte by byte, so the test owns the layout it claims to
    /// read — with a junk chunk between `fmt ` and `data`, because real
    /// capture tools put one there.
    fn wav_bytes(format: u16, bits: u16, channels: u16, rate: u32, payload: &[u8]) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(b"RIFF");
        v.extend_from_slice(&0u32.to_le_bytes()); // size unchecked on purpose
        v.extend_from_slice(b"WAVEfmt ");
        v.extend_from_slice(&16u32.to_le_bytes());
        v.extend_from_slice(&format.to_le_bytes());
        v.extend_from_slice(&channels.to_le_bytes());
        v.extend_from_slice(&rate.to_le_bytes());
        v.extend_from_slice(&0u32.to_le_bytes()); // byte rate, unread
        v.extend_from_slice(&0u16.to_le_bytes()); // block align, unread
        v.extend_from_slice(&bits.to_le_bytes());
        v.extend_from_slice(b"junk");
        v.extend_from_slice(&3u32.to_le_bytes());
        v.extend_from_slice(&[9, 9, 9, 0]); // odd size + its pad byte
        v.extend_from_slice(b"data");
        v.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        v.extend_from_slice(payload);
        v
    }

    fn read_bytes(bytes: &[u8]) -> Result<WavData, String> {
        let dir = std::env::temp_dir();
        let path = dir.join(format!("tangent-wavread-{}-{}.wav", std::process::id(), bytes.len()));
        std::fs::write(&path, bytes).unwrap();
        let got = read(&path);
        let _ = std::fs::remove_file(&path);
        got
    }

    #[test]
    fn every_depth_decodes_to_the_same_stereo_samples() {
        // The same four frames of stereo — L rising, R falling — written at
        // every depth this reader claims, must come back as the same planar
        // audio to a depth-appropriate tolerance.
        let l = [0.0f32, 0.25, -0.5, 1.0 - 1.0 / 32_768.0];
        let r = [0.5f32, -0.25, 0.75, -1.0];
        let mut p16 = Vec::new();
        let mut p24 = Vec::new();
        let mut p32 = Vec::new();
        let mut pf = Vec::new();
        for f in 0..4 {
            for s in [l[f], r[f]] {
                p16.extend_from_slice(&((s * 32_768.0).clamp(-32_768.0, 32_767.0) as i16).to_le_bytes());
                let v24 = (s * 8_388_608.0).clamp(-8_388_608.0, 8_388_607.0) as i32;
                p24.extend_from_slice(&v24.to_le_bytes()[0..3]);
                let v32 = (f64::from(s) * 2_147_483_648.0).clamp(-2_147_483_648.0, 2_147_483_647.0) as i32;
                p32.extend_from_slice(&v32.to_le_bytes());
                pf.extend_from_slice(&s.to_le_bytes());
            }
        }
        for (bits, format, payload, tol) in [
            (16, FORMAT_PCM, &p16, 1.0 / 32_000.0),
            (24, FORMAT_PCM, &p24, 1.0 / 8_000_000.0),
            (32, FORMAT_PCM, &p32, 1.0 / 2.0e9),
            (32, FORMAT_FLOAT, &pf, 0.0),
        ] {
            let wav = read_bytes(&wav_bytes(format, bits, 2, 48_000, payload)).unwrap();
            assert_eq!(wav.rate, 48_000);
            assert_eq!(wav.channels.len(), 2);
            for f in 0..4 {
                assert!((wav.channels[0][f] - l[f]).abs() <= tol, "{bits}-bit L frame {f}");
                assert!((wav.channels[1][f] - r[f]).abs() <= tol, "{bits}-bit R frame {f}");
            }
        }
    }

    #[test]
    fn extensible_mono_float_and_the_refusals_all_answer_in_sentences() {
        // Extensible wrapping float: the sub-format GUID's first two bytes
        // carry the truth, at offset 24 of a 40-byte fmt chunk.
        let mut v = Vec::new();
        v.extend_from_slice(b"RIFF");
        v.extend_from_slice(&0u32.to_le_bytes());
        v.extend_from_slice(b"WAVEfmt ");
        v.extend_from_slice(&40u32.to_le_bytes());
        v.extend_from_slice(&FORMAT_EXTENSIBLE.to_le_bytes());
        v.extend_from_slice(&1u16.to_le_bytes());
        v.extend_from_slice(&44_100u32.to_le_bytes());
        v.extend_from_slice(&[0; 6]);
        v.extend_from_slice(&32u16.to_le_bytes());
        v.extend_from_slice(&22u16.to_le_bytes()); // cbSize
        v.extend_from_slice(&32u16.to_le_bytes()); // valid bits
        v.extend_from_slice(&0u32.to_le_bytes()); // channel mask
        v.extend_from_slice(&FORMAT_FLOAT.to_le_bytes()); // GUID's format tag
        v.extend_from_slice(&[0; 14]);
        v.extend_from_slice(b"data");
        v.extend_from_slice(&8u32.to_le_bytes());
        v.extend_from_slice(&1.0f32.to_le_bytes());
        v.extend_from_slice(&f32::NAN.to_le_bytes());
        let wav = read_bytes(&v).unwrap();
        assert_eq!(wav.rate, 44_100);
        assert_eq!(wav.channels.len(), 1);
        assert_eq!(wav.channels[0][0], 1.0);
        assert_eq!(wav.channels[0][1], 0.0, "a NaN sample must read as silence");

        assert!(read_bytes(b"not a wav at all").is_err());
        assert!(read_bytes(&wav_bytes(FORMAT_PCM, 8, 1, 48_000, &[1, 2, 3])).is_err());
        assert!(read(Path::new("/nowhere/tangent-missing.wav")).is_err());
    }
}
