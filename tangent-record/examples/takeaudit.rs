//! **Does the file match the manifest?** — every take on the machine, at once.
//!
//! A take writes a `.wav` and a `take.json` saying how long the take was, and
//! nothing has ever compared the two. There are exactly two ways they can
//! disagree, and this reports both:
//!
//! - **The file is SHORTER than the take** — audio that never arrived. A
//!   dropout does not do this: dropouts are padded and keep the file
//!   sample-accurate. So a short file no longer lines up with the `.mid` or the
//!   video, and anything aligning them by the manifest's duration is wrong by
//!   the whole shortfall, silently.
//! - **The file is LONGER than the take by a constant ratio** — a manifest
//!   fault rather than a file fault. 48000/44100 is 1.0884, and that ratio is
//!   the one recorded above `TakeAudio::sample_rate`: the manifest read the
//!   CAPTURE device's rate while the file was written at the desk's. The audio
//!   is fine; the duration, the rate and the ppm in the sidecar are not.
//!
//! **The second shape is why this is a tool and not a test.** That fault is
//! fixed, and fixing it did nothing for the takes already on the disk — a
//! sidecar written by a build from two months ago keeps its wrong numbers
//! forever, and no amount of correctness today will tell you which of your
//! archive is affected. A check inside the app can only speak for takes it is
//! about to write. Only something that walks a corpus can answer "is what I
//! already recorded trustworthy", and that question has an owner: the person
//! with the archive, not the program.
//!
//! It follows that a verdict here is a statement about the BUILD THAT WROTE
//! that take and not about the one running now. Ratios and dates, not blame.
//!
//! ```text
//! cargo run --release -p tangent-record --example takeaudit
//! cargo run --release -p tangent-record --example takeaudit -- ~/Videos/Tangent
//! ```
//!
//! With no argument it looks where takes actually land. Every root given is
//! searched recursively for `take.json`, so pointing it at a backup drive or an
//! archive works without teaching it anything.
//!
//! **It reads and never writes.** A tool for deciding whether a recording is
//! trustworthy must not be able to change one.

use std::path::{Path, PathBuf};

/// A take is "short" past this. The clock is a fit and the last block is a
/// partial one, so demanding exactness would flag every honest take; a quarter
/// of a second is far below anything a listener would call a hole and far above
/// the arithmetic.
const TOLERANCE_S: f64 = 0.25;

/// The ratio that means the manifest read 48 kHz over a 44.1 kHz file, and the
/// slack around it. Recognised by name because "8.8 % long" is a diagnosis
/// rather than a measurement, and the person reading this wants the diagnosis.
const RATE_MIXUP: f64 = 48_000.0 / 44_100.0;

fn main() {
    let roots: Vec<PathBuf> = {
        let given: Vec<PathBuf> = std::env::args().skip(1).map(PathBuf::from).collect();
        if given.is_empty() {
            default_roots()
        } else {
            given
        }
    };
    let mut manifests = vec![];
    for r in &roots {
        find_takes(r, &mut manifests);
    }
    manifests.sort();
    if manifests.is_empty() {
        println!("no takes found under {roots:?}");
        return;
    }

    println!(
        "{:<28}{:>10}{:>10}{:>10}   {}",
        "take", "manifest", "wav", "diff", "verdict"
    );
    let (mut short, mut long, mut unreadable) = (0usize, 0usize, 0usize);
    for m in &manifests {
        let name = m
            .parent()
            .and_then(Path::file_name)
            .map_or_else(|| "?".into(), |n| n.to_string_lossy().into_owned());
        let Some(manifest_s) = manifest_seconds(m) else {
            println!("{name:<28}{:>10}", "unreadable");
            unreadable += 1;
            continue;
        };
        let Some(wav) = wav_beside(m).and_then(|p| wav_seconds(&p)) else {
            println!("{name:<28}{manifest_s:>10.2}{:>10}", "no wav");
            continue;
        };
        // Manifest MINUS file, so a positive number is missing audio. The sign
        // is the whole diagnosis and it should not need working out.
        let diff = manifest_s - wav;
        let verdict = if diff > TOLERANCE_S {
            short += 1;
            format!("SHORT by {diff:.2} s - audio that never arrived")
        } else if -diff > TOLERANCE_S {
            long += 1;
            let ratio = wav / manifest_s;
            if (ratio - RATE_MIXUP).abs() < 0.01 {
                "long by exactly 48000/44100 - the manifest read the wrong rate".to_owned()
            } else {
                format!("long by {:.2} s", -diff)
            }
        } else {
            "ok".to_owned()
        };
        println!("{name:<28}{manifest_s:>10.2}{wav:>10.2}{diff:>+10.2}   {verdict}");
    }

    println!();
    println!(
        "  {} takes: {} short, {} long, {} unreadable",
        manifests.len(),
        short,
        long,
        unreadable
    );
    if short > 0 {
        println!(
            "  A SHORT take has lost audio. It is not a dropout — those are padded and\n  \
             keep the file sample-accurate — so the .wav no longer lines up with the\n  \
             .mid or the video, and anything aligning them by the manifest is wrong."
        );
    }
    if long > 0 {
        println!(
            "  A LONG take by the 48000/44100 ratio is a manifest fault and not a file\n  \
             fault: the audio is fine and the sidecar's duration, rate and ppm are not."
        );
    }
}

/// Where takes land when nobody has said otherwise.
fn default_roots() -> Vec<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    home.into_iter()
        .flat_map(|h| {
            [
                h.join("Documents").join("Tangent Media").join("Takes"),
                h.join("Videos").join("Tangent"),
                h.join("Movies").join("Tangent"),
            ]
        })
        .filter(|p| p.is_dir())
        .collect()
}

fn find_takes(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            find_takes(&p, out);
        } else if p.file_name().is_some_and(|n| n == "take.json") {
            out.push(p);
        }
    }
}

fn manifest_seconds(p: &Path) -> Option<f64> {
    let text = std::fs::read_to_string(p).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    v.get("duration_seconds")?.as_f64()
}

fn wav_beside(manifest: &Path) -> Option<PathBuf> {
    std::fs::read_dir(manifest.parent()?)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .find(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("wav")))
}

/// The WAV's own duration, from its chunks.
///
/// **Walked rather than assumed at a fixed offset.** A take's `.wav` is a
/// broadcast WAV: it carries a `bext` chunk before `fmt `, so the layout every
/// naive reader assumes — and the one this function was first written with —
/// reads the middle of a timestamp and reports 22081 channels at 2 GHz.
fn wav_seconds(p: &Path) -> Option<f64> {
    let b = std::fs::read(p).ok()?;
    if b.len() < 12 || &b[0..4] != b"RIFF" || &b[8..12] != b"WAVE" {
        return None;
    }
    let (mut i, mut fmt, mut data) = (12usize, None, None);
    while i + 8 <= b.len() {
        let id = &b[i..i + 4];
        let sz = u32::from_le_bytes(b[i + 4..i + 8].try_into().ok()?) as usize;
        let body = i + 8;
        if id == b"fmt " && body + 16 <= b.len() {
            let channels = u16::from_le_bytes(b[body + 2..body + 4].try_into().ok()?);
            let rate = u32::from_le_bytes(b[body + 4..body + 8].try_into().ok()?);
            let align = u16::from_le_bytes(b[body + 12..body + 14].try_into().ok()?);
            if channels > 0 && rate > 0 && align > 0 {
                fmt = Some((f64::from(rate), f64::from(align)));
            }
        } else if id == b"data" {
            // The declared length, not the file length: a take that was cut
            // off mid-write has a header claiming more than is there, and the
            // smaller of the two is the audio that actually exists.
            data = Some(sz.min(b.len().saturating_sub(body)) as f64);
        }
        // Chunks are word-aligned; an odd size carries a pad byte.
        i = body + sz + (sz & 1);
    }
    let ((rate, align), data) = (fmt?, data?);
    Some(data / align / rate)
}
