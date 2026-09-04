//! A WAV writer, by hand, because the app has no WAV crate and must not gain
//! one for a bench. 24-bit PCM: the extra eight bits over 16 matter here, since
//! the tail of a release is where a model is judged and 16-bit dither
//! would sit right on top of it.

use std::fs::File;
use std::io::{BufWriter, Result, Write};
use std::path::Path;

fn le32(w: &mut impl Write, v: u32) -> Result<()> {
    w.write_all(&v.to_le_bytes())
}

fn le16(w: &mut impl Write, v: u16) -> Result<()> {
    w.write_all(&v.to_le_bytes())
}

/// Interleaved f32 in, 24-bit PCM out. `channels` is the interleave stride.
pub fn write24(path: impl AsRef<Path>, rate: u32, channels: u16, samples: &[f32]) -> Result<()> {
    let frames = samples.len() / usize::from(channels).max(1);
    let bytes = frames * usize::from(channels) * 3;
    let mut w = BufWriter::new(File::create(path)?);

    w.write_all(b"RIFF")?;
    le32(&mut w, (36 + bytes) as u32)?;
    w.write_all(b"WAVEfmt ")?;
    le32(&mut w, 16)?;
    le16(&mut w, 1)?; // PCM
    le16(&mut w, channels)?;
    le32(&mut w, rate)?;
    le32(&mut w, rate * u32::from(channels) * 3)?;
    le16(&mut w, channels * 3)?;
    le16(&mut w, 24)?;
    w.write_all(b"data")?;
    le32(&mut w, bytes as u32)?;

    // No dither and no soft clip: the bench must show exactly what the model
    // produced, including any overload. A clipped render is a finding.
    for &s in samples {
        let v = (s.clamp(-1.0, 1.0) * 8_388_607.0) as i32;
        let b = v.to_le_bytes();
        w.write_all(&b[0..3])?;
    }
    w.flush()
}
