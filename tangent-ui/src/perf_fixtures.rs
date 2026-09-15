//! **Documents of a known size, for measuring against.**
//!
//! Every perf probe in this crate reads the same four documents, built here
//! and nowhere else, so a number printed by one probe can be set beside a
//! number printed by another and the two are about the same song. The four
//! sizes (`S`, `M`, `L`, `XL`) step the lane count, the clips per lane, the
//! notes per MIDI clip and the number and length of the audio takes; `S` is
//! already larger than the biggest real project measured (3-13 clips, up to
//! ~3,250 notes, up to 45 takes), so the ladder is a stress ladder and not a
//! census.
//!
//! The shape is a REAL document's shape rather than a bag of notes: audio and
//! MIDI mixed, some clips looped, some trimmed, some stretched, fades on the
//! audio, a few automation curves, markers, and a desk snapshot taken from
//! `Settings::default()` — the shipped desk, two occupied seats (a real
//! document's `desk.inserts` was 13,167 entries before version 7; it is
//! the 108-seat head and a sparse tail now).
//!
//! **Test-only.** Declared `#[cfg(test)]` in `lib.rs`; nothing here ships,
//! and the WAV writer at the bottom exists only because `tangent-ui` cannot
//! reach `tangent-record`'s (the firewall in `Cargo.toml`).
//!
//! `TANGENT_PERF_OUT=/some/dir cargo test --release -p tangent-ui --lib
//! write_perf_projects -- --ignored --nocapture` writes the four as real
//! project folders (document plus `media/*.wav`) for opening in the app.
//! `TANGENT_PERF_SIZES=S,M` limits which. Budget the disk: the takes are
//! 24-bit stereo at 48 kHz, so L is 2.2 GB of WAV and XL is 5 GB.

use crate::arrangement::{
    Anchor, ArrClip, ArrMarker, ArrNote, ArrPedal, Arrangement, ClipKind, DeskSnap,
};
use crate::automation::{AutoParam, Automation, Curve, Point, Shape};
use crate::recorder::CHANNELS;
use crate::settings::Settings;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Which rung of the ladder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Size {
    S,
    M,
    L,
    XL,
}

/// What a size means, in the caller's own terms.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Spec {
    pub lanes: usize,
    pub clips_per_lane: usize,
    pub notes_per_clip: usize,
    /// How many audio clips, each on its own WAV.
    pub takes: usize,
    /// Seconds per take.
    pub take_s: f64,
}

impl Size {
    pub(crate) const ALL: [Size; 4] = [Size::S, Size::M, Size::L, Size::XL];

    pub(crate) fn tag(self) -> &'static str {
        match self {
            Size::S => "S",
            Size::M => "M",
            Size::L => "L",
            Size::XL => "XL",
        }
    }

    pub(crate) fn from_tag(t: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.tag().eq_ignore_ascii_case(t.trim()))
    }

    pub(crate) fn spec(self) -> Spec {
        match self {
            Size::S => Spec { lanes: 8, clips_per_lane: 4, notes_per_clip: 64, takes: 8, take_s: 30.0 },
            Size::M => Spec { lanes: 16, clips_per_lane: 16, notes_per_clip: 128, takes: 32, take_s: 60.0 },
            Size::L => Spec { lanes: 24, clips_per_lane: 32, notes_per_clip: 256, takes: 64, take_s: 120.0 },
            Size::XL => Spec { lanes: 32, clips_per_lane: 64, notes_per_clip: 512, takes: 96, take_s: 180.0 },
        }
    }

    /// The sizes named in `TANGENT_PERF_SIZES`, or all four.
    pub(crate) fn chosen() -> Vec<Size> {
        match std::env::var("TANGENT_PERF_SIZES") {
            Ok(list) if !list.trim().is_empty() => {
                list.split(',').filter_map(Size::from_tag).collect()
            }
            _ => Self::ALL.to_vec(),
        }
    }
}

/// The takes' sample rate. Every take Tangent records is 48 kHz.
pub(crate) const RATE: u32 = 48_000;
/// The fixture's tempo: 120 in 4/4, so a bar is exactly two seconds and the
/// "8 bars" and "1 bar" zooms the probes ask for are 16 s and 2 s.
pub(crate) const BPM: f64 = 120.0;
pub(crate) const BAR_S: f64 = 4.0 * 60.0 / BPM;

/// A MIDI clip's length for a note count: sixteen notes a second — sixteenths
/// at 120 in two voices — rounded UP to whole bars. 64 notes is two bars.
fn midi_len_s(notes: usize) -> f64 {
    let raw = notes as f64 / 16.0;
    (raw / BAR_S).ceil() * BAR_S
}

/// `media/take-NN.wav`, as the recorder names the files it writes into a
/// project folder.
pub(crate) fn take_path(i: usize) -> String {
    format!("media/take-{i:03}.wav")
}

/// The document, in memory, ids assigned.
pub(crate) fn arrangement(size: Size) -> Arrangement {
    let sp = size.spec();
    let total = sp.lanes * sp.clips_per_lane;
    // Which grid cells carry audio: `takes` of them, spread evenly.
    let mut audio_at = vec![usize::MAX; total];
    for k in 0..sp.takes.min(total) {
        audio_at[k * total / sp.takes.max(1)] = k;
    }
    let mut arr = Arrangement { named: true, ..Default::default() };
    let midi_len = midi_len_s(sp.notes_per_clip);
    let mut song_end = 0.0f64;
    for lane in 0..sp.lanes {
        let mut cursor = 0.0f64;
        for c in 0..sp.clips_per_lane {
            let k = lane * sp.clips_per_lane + c;
            let take = audio_at[k];
            let clip = if take != usize::MAX {
                let clip = audio_clip(lane, cursor, take, sp.take_s);
                cursor += (clip.len_s / BAR_S).ceil() * BAR_S;
                clip
            } else {
                let clip = midi_clip(lane, c, cursor, midi_len, sp.notes_per_clip);
                cursor += clip.len_s;
                clip
            };
            arr.add_clip(clip);
        }
        song_end = song_end.max(cursor);
    }
    arr.automation = automation(sp.lanes, song_end);
    arr.markers = markers(song_end);
    arr.desk = Some(desk());
    arr
}

/// One audio clip standing on `take`'s file. Every fourth is trimmed, every
/// third is dragged out past its file (a real loop), every fifth is stretched
/// to follow the tempo; all of them fade.
fn audio_clip(lane: usize, start_s: f64, take: usize, take_s: f64) -> ArrClip {
    let trimmed = take % 4 == 3;
    let dragged = take % 3 == 2;
    let stretched = take % 5 == 4;
    let offset_s = if trimmed { 1.25 } else { 0.0 };
    let file_left = take_s - offset_s;
    // Born looping at its own length, the owner's rule; a dragged one
    // repeats half a pass more.
    let loop_s = file_left;
    let len_s = if dragged { file_left * 1.5 } else { file_left };
    ArrClip {
        kind: ClipKind::Audio,
        lane,
        path: take_path(take),
        name: format!("Take {}", take + 1),
        start_s,
        offset_s,
        len_s,
        loop_s,
        // Binary fractions throughout: the document reader parses through
        // serde_json's default float path, which is exact only for short
        // decimals, and a fixture must be a fixed point of the reader.
        fade_in_s: 0.03125 + 0.0625 * (take % 5) as f64,
        fade_out_s: 0.125 + 0.25 * (take % 4) as f64,
        anchor: if stretched { Anchor::Beat } else { Anchor::Time },
        follow_tempo: stretched,
        stretch: if stretched { if take % 2 == 0 { 1.06 } else { 0.94 } } else { 1.0 },
        stretch_mode: if stretched {
            if take % 2 == 0 { "tonal" } else { "rhythmic" }.to_owned()
        } else {
            String::new()
        },
        gain: match take % 6 {
            0 => 0.8,
            3 => 1.2,
            _ => 1.0,
        },
        ..Default::default()
    }
}

/// One MIDI clip: `notes` notes in `len_s`, two voices, a scale walk over
/// three octaves. Every third clip is a real loop — one bar of material
/// repeated across the clip. Every other clip has a pedal under it.
fn midi_clip(lane: usize, index: usize, start_s: f64, len_s: f64, notes: usize) -> ArrClip {
    let looped = index % 3 == 2;
    // A real loop is half the clip (never under a bar): two passes of the
    // material, held once.
    let loop_s = if looped { (len_s / 2.0).max(BAR_S) } else { len_s };
    // The notes fill the LOOP span when looping (the material is held once),
    // and the whole clip otherwise.
    let span = loop_s;
    let step = span / notes as f64;
    let scale = [0u8, 2, 4, 5, 7, 9, 11];
    let mut list = Vec::with_capacity(notes);
    for n in 0..notes {
        let degree = (n * 3 + lane) % 21;
        let key = 36 + scale[degree % 7] + 12 * (degree / 7) as u8;
        list.push(ArrNote {
            at_s: n as f64 * step,
            dur_s: if n % 2 == 0 { step * 7.0 / 8.0 } else { step * 15.0 / 8.0 },
            key,
            vel: 60 + ((n * 7 + lane * 3) % 60) as u8,
        muted: false,
        });
    }
    let mut pedal = Vec::new();
    if index % 2 == 1 {
        let beats = (span / (BAR_S / 4.0)) as usize;
        for b in 0..beats.max(2) {
            pedal.push(ArrPedal {
                at_s: b as f64 * (BAR_S / 4.0),
                cc: 64,
                value: if b % 2 == 0 { 127 } else { 0 },
            });
        }
    }
    ArrClip {
        kind: ClipKind::Midi,
        lane,
        name: format!("Part {}", index + 1),
        start_s,
        len_s,
        loop_s,
        notes: list,
        pedal,
        anchor: Anchor::Beat,
        ..Default::default()
    }
}

/// A few curves, the way a mixed song has them: volume rides on the first
/// four lanes, a pan and a send, and a master fade — 64 points each.
fn automation(lanes: usize, song_s: f64) -> Automation {
    // Times on half-second lines and values in 1/1024ths: short decimals,
    // which the reader's float path parses exactly — see `audio_clip`.
    let pts = |n: usize, hold: bool| -> Vec<Point> {
        (0..n)
            .map(|i| Point {
                at_s: (song_s * i as f64 / n as f64 * 2.0).floor() / 2.0,
                value: ((0.5 + 0.4 * (i as f64 * 0.7).sin()) * 1024.0).round() / 1024.0,
                shape: if hold && i % 3 == 0 { Shape::Hold } else { Shape::Linear },
            })
            .collect()
    };
    let mut curves = Vec::new();
    for lane in 0..4.min(lanes) {
        curves.push(Curve::sanitised(lane, AutoParam::Gain, pts(64, false)));
    }
    if lanes > 1 {
        curves.push(Curve::sanitised(1, AutoParam::Pan, pts(32, true)));
    }
    if lanes > 2 {
        curves.push(Curve::sanitised(2, AutoParam::Send, pts(48, false)));
    }
    curves.push(Curve::sanitised(CHANNELS, AutoParam::Gain, pts(16, false)));
    Automation::from_curves(curves)
}

fn markers(song_s: f64) -> Vec<ArrMarker> {
    let names = [
        "Intro", "Verse 1", "Chorus", "Verse 2", "Chorus 2", "Bridge", "Solo", "Chorus 3",
        "Outro", "Tail", "Alt take", "Fix this",
    ];
    names
        .iter()
        .enumerate()
        .map(|(i, n)| ArrMarker {
            // On bar lines, like a person puts them.
            at_s: ((song_s * i as f64 / names.len() as f64) / BAR_S).floor() * BAR_S,
            name: (*n).to_owned(),
        })
        .collect()
}

/// The desk as `TangentApp::desk_snapshot_of(&Settings::default())` takes
/// it — the same fields from the same settings, spelled here because that
/// function is private to `app.rs` and the document has to carry a desk to
/// have a real document's shape.
pub(crate) fn desk() -> DeskSnap {
    desk_of(&Settings::default())
}

/// The snapshot `desk_snapshot_of` would take of ANY settings — the same
/// field list, rebuilt here because the app's is private.
pub(crate) fn desk_of(s: &Settings) -> DeskSnap {
    let sig = s.time_signature();
    DeskSnap {
        kinds: s.channel_kinds.to_vec(),
        icons: s.channel_icons.to_vec(),
        names: s.strip_names.clone(),
        colors: s.strip_colors.to_vec(),
        inserts: s
            .strip_inserts
            .iter()
            .enumerate()
            .filter(|(_, p)| !p.is_empty())
            .map(|(n, p)| (n, p.clone()))
            .collect(),
        sends: s.strip_sends.to_vec(),
        muted: s.strip_muted,
        soloed: s.strip_soloed,
        gains: s.channel_gains.to_vec(),
        pans: s.channel_pans.to_vec(),
        inputs: s.channel_inputs.clone(),
        midi_inputs: s.channel_midi_inputs.clone(),
        drums_gain: s.drums_gain,
        drums_pan: s.drums_pan,
        metronome_gain: s.metronome_gain,
        master_gain: s.master_gain,
        fx_return: s.fx_return_gain,
        tempo_bpm: f64::from(s.record_export.tempo_bpm),
        sig_beats: sig.beats,
        sig_unit: sig.unit,
        metronome_on: s.metronome_on,
        tracks_added: s.tracks_added as u32,
        is_folder: s.strip_is_folder.clone(),
        in_folder: s.strip_in_folder.clone(),
        folder_shut: s.strip_folder_shut.clone(),
        armed: s.channel_armed.unwrap_or(0),
        phase: Some(s.strip_phase),
        to_theory: Some(s.strip_to_theory.clone()),
        routes: Some(s.routes.clone()),
        insert_keys: Some(s.insert_keys.clone()),
        bypass: Some(s.insert_bypass.clone()),
        instruments: Some(crate::arrangement::Instruments {
            dx7_cartridge: s.strip_dx7_cartridge.clone(),
            dx7_patch: s.strip_dx7_patch.clone(),
            keys_patch: s.strip_keys_patch.clone(),
            keys_params: s.strip_keys_params.clone(),
        }),
        rack: Some(crate::arrangement::Rack {
            reverb: s.reverb_mix,
            delay: s.delay_mix,
            chorus: s.chorus_mix,
            phaser: s.phaser_mix,
            hpf: s.hpf_mix,
            lpf: s.lpf_mix,
            gain: s.gain_mix,
            limiter: s.limiter_mix,
            params: s.effect_params.clone(),
        }),
        track_gain: Some(s.track_gain),
        master_mono: Some(s.master_mono),
        lufs_window: Some(s.lufs_window),
        varispeed: Some(s.varispeed),
        click: Some(crate::arrangement::Click {
            div: s.metronome_div,
            in_take: s.metronome_in_take,
            sample: s.click_sample.clone(),
        }),
        transpose: Some(s.transpose),
        count_in: Some(crate::arrangement::CountIn {
            bars: s.record_count_in_bars,
            beats: s.record_count_in_beats,
            in_take: s.record_count_in_in_take,
        }),
        take_name: Some(s.record_take_name.clone().unwrap_or_default()),
        feel: Some(crate::arrangement::Feel {
            snap: s.midi_snap,
            grid_exp: s.midi_grid_exp,
            swing: s.midi_swing,
            swing_pct: s.midi_swing_pct,
            triplet: s.midi_triplet,
            humanize_timing: s.midi_humanize_timing,
            humanize_vel: s.midi_humanize_vel,
        }),
        notation: Some(crate::arrangement::Notation {
            staff_key: s.staff_key,
            prefer_flats: s.prefer_flats,
            capo: s.fretboard_capo,
            capo_style: s.capo_style.clone(),
            tuning: s.fretboard_tuning.clone(),
        }),
    }
}

/// Facts about a built document, for the printed lines.
pub(crate) struct Census {
    pub clips: usize,
    pub midi_clips: usize,
    pub audio_clips: usize,
    pub notes: usize,
    pub song_s: f64,
}

pub(crate) fn census(arr: &Arrangement) -> Census {
    let midi_clips = arr.clips.iter().filter(|c| c.kind == ClipKind::Midi).count();
    Census {
        clips: arr.clips.len(),
        midi_clips,
        audio_clips: arr.clips.len() - midi_clips,
        notes: arr.clips.iter().map(|c| c.notes.len()).sum(),
        song_s: arr.clips.iter().map(|c| c.start_s + c.len_s).fold(0.0, f64::max),
    }
}

// ── the takes ───────────────────────────────────────────────────────────────

/// `seconds` of stereo for take `i`, interleaved f32: a note every half
/// second, each a decaying sine burst with three harmonics, the right
/// channel a little behind the left, over a floor of quiet noise so nothing
/// in the file is digital silence. Generated a block at a time through `f`
/// so a 180-second take never sits whole in memory.
pub(crate) fn take_samples(i: usize, seconds: f64, mut f: impl FnMut(&[f32])) {
    let frames = (seconds * f64::from(RATE)) as usize;
    let f0 = 110.0 * 2f64.powf((i % 12) as f64 / 12.0);
    let mut block = vec![0.0f32; 4_800 * 2];
    let mut seed: u32 = 0x9E37_79B9 ^ (i as u32).wrapping_mul(2_654_435_761);
    let mut at = 0usize;
    while at < frames {
        let n = (frames - at).min(4_800);
        for k in 0..n {
            let t = (at + k) as f64 / f64::from(RATE);
            let burst = (t * 2.0).floor();
            let into = t - burst / 2.0;
            let env = 0.7 * (-into * 5.0).exp();
            let pitch = f0 * 2f64.powf(((burst as usize * 5) % 12) as f64 / 12.0);
            let tone = |phase: f64| {
                (phase).sin() + 0.4 * (2.0 * phase).sin() + 0.15 * (3.0 * phase).sin()
            };
            let ph = std::f64::consts::TAU * pitch * into;
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let noise = (f64::from(seed >> 8) / f64::from(1u32 << 24) - 0.5) * 0.002;
            block[k * 2] = (env * tone(ph) + noise) as f32;
            block[k * 2 + 1] = (env * 0.9 * tone(ph - 0.35) + noise) as f32;
        }
        f(&block[..n * 2]);
        at += n;
    }
}

/// A 24-bit PCM WAV writer of the smallest honest kind: `RIFF`, `fmt ` (tag
/// 1), `data`, sizes patched at the end. What the recorder's `WavWriter`
/// produces minus the `bext` chunk, which nothing here reads.
pub(crate) struct Wav24 {
    file: std::io::BufWriter<std::fs::File>,
    channels: u16,
    data_bytes: u32,
}

impl Wav24 {
    pub(crate) fn create(path: &Path, rate: u32, channels: u16) -> std::io::Result<Self> {
        use std::io::Write;
        let mut file = std::io::BufWriter::new(std::fs::File::create(path)?);
        let block_align = 3 * channels;
        let mut h = Vec::with_capacity(44);
        h.extend_from_slice(b"RIFF");
        h.extend_from_slice(&0u32.to_le_bytes());
        h.extend_from_slice(b"WAVE");
        h.extend_from_slice(b"fmt ");
        h.extend_from_slice(&16u32.to_le_bytes());
        h.extend_from_slice(&1u16.to_le_bytes());
        h.extend_from_slice(&channels.to_le_bytes());
        h.extend_from_slice(&rate.to_le_bytes());
        h.extend_from_slice(&(rate * u32::from(block_align)).to_le_bytes());
        h.extend_from_slice(&block_align.to_le_bytes());
        h.extend_from_slice(&24u16.to_le_bytes());
        h.extend_from_slice(b"data");
        h.extend_from_slice(&0u32.to_le_bytes());
        file.write_all(&h)?;
        Ok(Self { file, channels, data_bytes: 0 })
    }

    pub(crate) fn write(&mut self, interleaved: &[f32]) -> std::io::Result<()> {
        use std::io::Write;
        assert_eq!(interleaved.len() % usize::from(self.channels), 0);
        let mut bytes = Vec::with_capacity(interleaved.len() * 3);
        for &s in interleaved {
            let v = (f64::from(s.clamp(-1.0, 1.0)) * 8_388_607.0).round() as i32;
            let b = v.to_le_bytes();
            bytes.extend_from_slice(&b[..3]);
        }
        self.file.write_all(&bytes)?;
        self.data_bytes += bytes.len() as u32;
        Ok(())
    }

    pub(crate) fn finish(mut self) -> std::io::Result<u64> {
        use std::io::{Seek, SeekFrom, Write};
        self.file.flush()?;
        let f = self.file.get_mut();
        f.seek(SeekFrom::Start(4))?;
        f.write_all(&(36 + self.data_bytes).to_le_bytes())?;
        f.seek(SeekFrom::Start(40))?;
        f.write_all(&self.data_bytes.to_le_bytes())?;
        f.flush()?;
        Ok(44 + u64::from(self.data_bytes))
    }
}

/// Write take `i` as a WAV at `path`; bytes on disk.
pub(crate) fn write_take(path: &Path, i: usize, seconds: f64) -> std::io::Result<u64> {
    let mut w = Wav24::create(path, RATE, 2)?;
    let mut err = None;
    take_samples(i, seconds, |block| {
        if err.is_none() {
            err = w.write(block).err();
        }
    });
    if let Some(e) = err {
        return Err(e);
    }
    w.finish()
}

/// **Write a project folder**: `dir/project.tan`, and with `media` the
/// takes under `dir/media/`. Returns the document's path. The probes that
/// only parse pass `media = false`; the generator below passes `true`.
pub(crate) fn write_project(size: Size, dir: &Path, media: bool) -> PathBuf {
    let arr = arrangement(size);
    std::fs::create_dir_all(dir).expect("project dir");
    let doc = dir.join(crate::arrangement::DOC_NAME);
    arr.save_to(&doc);
    if media {
        let sp = size.spec();
        std::fs::create_dir_all(dir.join("media")).expect("media dir");
        for i in 0..sp.takes {
            write_take(&dir.join(take_path(i)), i, sp.take_s).expect("take");
        }
    }
    doc
}

/// A throwaway folder for one probe run, under the temp dir.
pub(crate) fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "tangent-perf-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch");
    dir
}

// ── the shared yardstick ────────────────────────────────────────────────────

/// The word every probe line carries, so nobody reads a debug number as a
/// shipping one.
pub(crate) fn build() -> &'static str {
    if cfg!(debug_assertions) { "DEBUG" } else { "release" }
}

pub(crate) fn per_call(n: u32, mut f: impl FnMut()) -> Duration {
    let t = Instant::now();
    for _ in 0..n {
        f();
    }
    t.elapsed() / n.max(1)
}

pub(crate) fn pct_frame(d: Duration) -> f64 {
    d.as_secs_f64() / 0.0166 * 100.0
}

/// One result line: the probe, the size, the cost, and — where the cost is
/// paid per frame — its share of a 16.6 ms frame.
pub(crate) fn say(probe: &str, size: &str, what: &str, d: Duration, per_frame: bool) {
    if per_frame {
        println!(
            "{probe:<12} {size:<3} {what:<44} {:>11.3?}  {:>8.2}% frame  [{}]",
            d,
            pct_frame(d),
            build()
        );
    } else {
        println!("{probe:<12} {size:<3} {what:<44} {:>11.3?}  [{}]", d, build());
    }
}

pub(crate) fn mb(bytes: usize) -> f64 {
    bytes as f64 / 1_048_576.0
}

// ── the probes that need nothing but the document ───────────────────────────

/// **The generator.** Writes the four (or `TANGENT_PERF_SIZES`) as real
/// project folders under `TANGENT_PERF_OUT`, takes and all.
#[test]
#[ignore]
fn write_perf_projects() {
    let Some(out) = std::env::var_os("TANGENT_PERF_OUT") else {
        println!("set TANGENT_PERF_OUT=/some/dir to write the fixtures; nothing written");
        return;
    };
    let out = PathBuf::from(out);
    for size in Size::chosen() {
        let dir = out.join(format!("perf-{}", size.tag()));
        let t = Instant::now();
        let doc = write_project(size, &dir, true);
        let took = t.elapsed();
        let arr = arrangement(size);
        let c = census(&arr);
        let doc_bytes = std::fs::metadata(&doc).map(|m| m.len()).unwrap_or(0);
        let media_bytes: u64 = std::fs::read_dir(dir.join("media"))
            .map(|d| d.flatten().filter_map(|e| e.metadata().ok()).map(|m| m.len()).sum())
            .unwrap_or(0);
        println!(
            "fixture {:<3} {} clips ({} midi / {} audio), {} notes, {:.0} s of song, doc {:.1} KB, media {:.2} GB in {} takes, written in {:.1?} -> {}",
            size.tag(),
            c.clips,
            c.midi_clips,
            c.audio_clips,
            c.notes,
            c.song_s,
            doc_bytes as f64 / 1024.0,
            media_bytes as f64 / 1_073_741_824.0,
            size.spec().takes,
            took,
            dir.display()
        );
    }
}

/// The seat the far-seat fixture fills, and with what. Bay 4149 is a pad's
/// own effect seat, far past the 108-seat head; the path is the shape a
/// real one has, so an older build that keeps it would try to load it.
pub(crate) const FAR_SEAT: usize = 4149;
pub(crate) const FAR_SEAT_PATH: &str = "/Library/Audio/Plug-Ins/VST3/Brusfri.vst3";

/// **A document with a seat past the head**, beside `perf-S`, for the
/// cross-version check. The shipped desk's two occupied seats both sit
/// inside the head, so `perf-S` writes no `inserts_tail` and a build
/// before v7 opens it whole; this one seats a plug-in in [`FAR_SEAT`] so
/// the tail is written and an OLDER reader's answer to it (keep the head,
/// drop the far seat) can be watched rather than argued. Written without
/// media: the takes are `perf-S`'s, and the document is what is under test.
#[test]
#[ignore]
fn write_far_seat_project() {
    let Some(out) = std::env::var_os("TANGENT_PERF_OUT") else {
        println!("set TANGENT_PERF_OUT=/some/dir to write the fixture; nothing written");
        return;
    };
    let dir = PathBuf::from(out).join("perf-S-far");
    let mut arr = arrangement(Size::S);
    arr.desk.as_mut().expect("the fixture carries a desk").set_bay(FAR_SEAT, FAR_SEAT_PATH);
    std::fs::create_dir_all(&dir).expect("project dir");
    let doc = dir.join(crate::arrangement::DOC_NAME);
    arr.save_to(&doc);
    let text = std::fs::read_to_string(&doc).expect("the document just written");
    assert!(text.contains("\"inserts_tail\""), "the far seat did not reach the tail");
    println!(
        "fixture S-far: seat {FAR_SEAT} = {FAR_SEAT_PATH}, doc {:.1} KB -> {}",
        text.len() as f64 / 1024.0,
        dir.display()
    );
}

/// The fixture is a fixed point of the document reader, and is the shape it
/// claims to be. Not ignored: it is what makes the sizes trustworthy, and it
/// costs a few hundred milliseconds in debug.
#[test]
fn the_fixtures_round_trip_and_have_the_promised_shape() {
    for size in [Size::S, Size::M] {
        let sp = size.spec();
        let arr = arrangement(size);
        let c = census(&arr);
        assert_eq!(c.clips, sp.lanes * sp.clips_per_lane, "{size:?} clip count");
        assert_eq!(c.audio_clips, sp.takes, "{size:?} take count");
        assert_eq!(c.notes, c.midi_clips * sp.notes_per_clip, "{size:?} notes");
        assert!(arr.clips.iter().any(|c| c.loop_s > 0.0 && c.loop_s < c.len_s), "no real loop");
        assert!(arr.clips.iter().any(|c| c.stretch != 1.0), "nothing stretched");
        assert!(arr.clips.iter().any(|c| c.fade_in_s > 0.0), "no fades");
        assert!(!arr.automation.is_empty() && !arr.markers.is_empty());
        let desk = arr.desk.as_ref().expect("a desk");
        assert_eq!(desk.bay(0), Some(crate::dialogs::DRUMPAD_PATH), "the shipped desk");
        assert!(desk.inserts.iter().all(|(_, p)| !p.is_empty()), "an empty seat was kept");
        let text = arr.to_json();
        let back = Arrangement::from_json(&text).to_json();
        if back != text {
            // Say WHERE rather than dumping two half-megabyte strings.
            let (a, b): (Vec<&str>, Vec<&str>) = (text.lines().collect(), back.lines().collect());
            let at = a.iter().zip(&b).position(|(x, y)| x != y).unwrap_or(a.len().min(b.len()));
            let from = at.saturating_sub(3);
            panic!(
                "{size:?} is not a fixed point of the reader; first difference at line {at}:\n  written: {:?}\n  re-read: {:?}",
                &a[from..(at + 3).min(a.len())],
                &b[from..(at + 3).min(b.len())]
            );
        }
    }
}

/// **doc_parse**: what the document costs to read and to write, per size.
/// Written to a temp dir first so `read_from` pays for the file too, as the
/// app does.
#[test]
#[ignore]
fn perf_doc_parse() {
    println!("doc_parse [{}]: Arrangement::read_from / to_json / bytes, per size", build());
    let dir = scratch("doc-parse");
    for size in Size::chosen() {
        let arr = arrangement(size);
        let c = census(&arr);
        let doc = write_project(size, &dir.join(size.tag()), false);
        let bytes = std::fs::metadata(&doc).map(|m| m.len() as usize).unwrap_or(0);
        let n = match size {
            Size::S | Size::M => 10,
            Size::L => 5,
            Size::XL => 3,
        };
        let read = per_call(n, || {
            let out = Arrangement::read_from(&doc);
            assert!(matches!(out, crate::arrangement::LoadOutcome::Loaded(_)));
            std::hint::black_box(out);
        });
        let write = per_call(n, || {
            std::hint::black_box(arr.to_json());
        });
        let text = arr.to_json();
        let from_str = per_call(n, || {
            std::hint::black_box(Arrangement::from_json(&text));
        });
        println!(
            "doc_parse    {:<3} {} clips / {} notes / {:.0} s: doc {:.1} KB ({:.2} MB)",
            size.tag(),
            c.clips,
            c.notes,
            c.song_s,
            bytes as f64 / 1024.0,
            mb(bytes)
        );
        say("doc_parse", size.tag(), "Arrangement::read_from (file + parse)", read, false);
        say("doc_parse", size.tag(), "Arrangement::from_json (parse only)", from_str, false);
        say("doc_parse", size.tag(), "Arrangement::to_json", write, false);
    }
    let _ = std::fs::remove_dir_all(&dir);
}

// ── envelope: the host's min/max walk against a peak pyramid ────────────────

/// `desktop::span_envelope` and `frame_range_minmax`, replicated line for
/// line on an interleaved stereo buffer: those two are private to the binary
/// crate, and this crate cannot see it. Kept identical so the comparison
/// below is against the shipping algorithm and not a paraphrase of it.
pub(crate) fn span_envelope(samples: &[f32], offset: usize, len: usize, buckets: usize) -> Vec<f32> {
    let total = samples.len() / 2;
    let start = offset.min(total);
    let end = (offset + len).min(total);
    let frames = end.saturating_sub(start);
    if frames == 0 || buckets == 0 {
        return Vec::new();
    }
    let mut out = vec![0.0f32; buckets * 2];
    for i in 0..buckets {
        let a = start + i * frames / buckets;
        let b = (start + (i + 1) * frames / buckets).max(a + 1).min(end);
        let (lo, hi) = frame_range_minmax(samples, a, b);
        out[i * 2] = lo;
        out[i * 2 + 1] = hi;
    }
    out
}

fn frame_range_minmax(samples: &[f32], a: usize, b: usize) -> (f32, f32) {
    let mut lo = f32::INFINITY;
    let mut hi = f32::NEG_INFINITY;
    for f in a..b {
        for ch in 0..2 {
            let v = samples.get(f * 2 + ch).copied().unwrap_or(0.0);
            if v.is_finite() {
                lo = lo.min(v);
                hi = hi.max(v);
            }
        }
    }
    if lo <= hi {
        (lo.max(-1.0), hi.min(1.0))
    } else {
        (0.0, 0.0)
    }
}

/// Fold frames `[from, to)` of both channels into a running min and max.
fn fold_raw(samples: &[f32], from: usize, to: usize, lo: &mut f32, hi: &mut f32) {
    for &v in &samples[from * 2..to * 2] {
        if v.is_finite() {
            *lo = lo.min(v);
            *hi = hi.max(v);
        }
    }
}

/// **A peak pyramid**: (min, max) per block of `BASE` frames, then each
/// level the pairwise fold of the one below. A range query decomposes
/// `[a, b)` into raw samples at the unaligned ends and the largest aligned
/// blocks it can in the middle, so the answer is EXACT for any range — not
/// only when a block divides the bucket — and costs O(log n) per bucket.
pub(crate) struct Pyramid {
    base: usize,
    /// `levels[k]` holds pairs over blocks of `base << k` frames.
    levels: Vec<Vec<(f32, f32)>>,
}

impl Pyramid {
    pub(crate) fn build(samples: &[f32], base: usize) -> Self {
        let frames = samples.len() / 2;
        let n0 = frames / base;
        let mut l0 = Vec::with_capacity(n0);
        for blk in 0..n0 {
            let s = &samples[blk * base * 2..(blk + 1) * base * 2];
            let mut lo = f32::INFINITY;
            let mut hi = f32::NEG_INFINITY;
            for &v in s {
                if v.is_finite() {
                    lo = lo.min(v);
                    hi = hi.max(v);
                }
            }
            l0.push((lo, hi));
        }
        let mut levels = vec![l0];
        while levels.last().is_some_and(|l| l.len() >= 2) {
            let below = levels.last().expect("a level");
            let up: Vec<(f32, f32)> = below
                .chunks_exact(2)
                .map(|p| (p[0].0.min(p[1].0), p[0].1.max(p[1].1)))
                .collect();
            levels.push(up);
        }
        Self { base, levels }
    }

    pub(crate) fn bytes(&self) -> usize {
        self.levels.iter().map(|l| l.len() * 8).sum()
    }

    /// Min and max over frames `[a, b)`, seeded at the infinities like the
    /// host's, clamped like the host's.
    fn minmax(&self, samples: &[f32], a: usize, b: usize) -> (f32, f32) {
        let mut lo = f32::INFINITY;
        let mut hi = f32::NEG_INFINITY;
        let mut cur = a;
        // The unaligned head, sample by sample.
        let head_end = ((a + self.base - 1) / self.base * self.base).min(b);
        if head_end > cur {
            fold_raw(samples, cur, head_end, &mut lo, &mut hi);
            cur = head_end;
        }
        // The middle: the biggest aligned block that fits, every step.
        while cur + self.base <= b {
            let mut k = self.levels.len() - 1;
            loop {
                let span = self.base << k;
                if cur % span == 0 && cur + span <= b && (cur / span) < self.levels[k].len() {
                    let (l, h) = self.levels[k][cur / span];
                    lo = lo.min(l);
                    hi = hi.max(h);
                    cur += span;
                    break;
                }
                k -= 1;
            }
        }
        // The unaligned tail.
        if b > cur {
            fold_raw(samples, cur, b, &mut lo, &mut hi);
        }
        if lo <= hi {
            (lo.max(-1.0), hi.min(1.0))
        } else {
            (0.0, 0.0)
        }
    }

    /// `span_envelope`'s answer, through the pyramid.
    pub(crate) fn span_envelope(&self, samples: &[f32], offset: usize, len: usize, buckets: usize) -> Vec<f32> {
        let total = samples.len() / 2;
        let start = offset.min(total);
        let end = (offset + len).min(total);
        let frames = end.saturating_sub(start);
        if frames == 0 || buckets == 0 {
            return Vec::new();
        }
        let mut out = vec![0.0f32; buckets * 2];
        for i in 0..buckets {
            let a = start + i * frames / buckets;
            let b = (start + (i + 1) * frames / buckets).max(a + 1).min(end);
            let (lo, hi) = self.minmax(samples, a, b);
            out[i * 2] = lo;
            out[i * 2 + 1] = hi;
        }
        out
    }
}

/// **envelope**: the host's per-redraw min/max walk over a 180 s stereo
/// take at 8192 buckets, against the pyramid — built once, queried per zoom.
#[test]
#[ignore]
fn perf_envelope() {
    const BUCKETS: usize = 8192;
    const BASE: usize = 64;
    println!("envelope [{}]: 180 s stereo f32 at {RATE} Hz, {BUCKETS} buckets, pyramid base {BASE}", build());
    let seconds = 180.0;
    let mut samples: Vec<f32> = Vec::with_capacity((seconds * f64::from(RATE)) as usize * 2);
    take_samples(3, seconds, |b| samples.extend_from_slice(b));
    let frames = samples.len() / 2;
    println!("envelope         buffer {} frames = {:.1} MB", frames, mb(samples.len() * 4));

    let t = Instant::now();
    let pyr = Pyramid::build(&samples, BASE);
    let built = t.elapsed();
    println!(
        "envelope         pyramid build once   {:>11.3?}  ({} levels, {:.2} MB)",
        built,
        pyr.levels.len(),
        mb(pyr.bytes())
    );

    let rate = RATE as usize;
    // The whole file, a minute, the two musical zooms, and a close-up whose
    // buckets are a few frames wide — where the pyramid can help least.
    for (what, offset_s, len_s) in [
        ("whole take (180 s)", 0.0, 180.0),
        ("60 s", 30.0, 60.0),
        ("8 bars (16 s)", 40.0, 16.0),
        ("1 bar (2 s)", 40.5, 2.0),
        ("0.25 s close-up", 41.0, 0.25),
    ] {
        let offset = (offset_s * rate as f64) as usize;
        let len = (len_s * rate as f64) as usize;
        let n = if len_s >= 60.0 { 5 } else { 20 };
        let direct = per_call(n, || {
            std::hint::black_box(span_envelope(&samples, offset, len, BUCKETS));
        });
        let via = per_call(n, || {
            std::hint::black_box(pyr.span_envelope(&samples, offset, len, BUCKETS));
        });
        let a = span_envelope(&samples, offset, len, BUCKETS);
        let b = pyr.span_envelope(&samples, offset, len, BUCKETS);
        assert_eq!(a.len(), b.len());
        let dev = a.iter().zip(&b).map(|(x, y)| (x - y).abs()).fold(0.0f32, f32::max);
        let frames_per_bucket = len as f64 / BUCKETS as f64;
        println!(
            "envelope         {what:<20} ({:>8.1} frames/bucket): walk {:>11.3?} ({:>6.1}% frame) | pyramid {:>10.3?} ({:>6.2}% frame) | {:>6.1}x | max deviation {dev:e}{}",
            frames_per_bucket,
            direct,
            pct_frame(direct),
            via,
            pct_frame(via),
            direct.as_secs_f64() / via.as_secs_f64(),
            if dev > 1e-6 { "  <-- DISAGREES" } else { "" }
        );
        assert!(dev <= 1e-6, "{what}: the pyramid disagrees with the walk by {dev}");
    }
}

/// **doc_bytes**: what a REAL-shaped document weighs on disk — the owner's
/// cr78test (13 clips, 140 notes, three occupied bays low in the desk) and
/// moscow1 (13 clips, 2,354 notes, nine bays with one at seat 4149, the
/// folder-extra rack that defeats any trailing trim) — and how much of it
/// is the desk's insert seats. Ignored by default; run with
/// `--ignored --nocapture`, before and after the seat space changed shape.
#[test]
#[ignore]
fn perf_doc_bytes() {
    use crate::arrangement::{ArrNote, ClipKind, ARRANGEMENT_VERSION};
    let low: &[(usize, &str)] = &[
        (0, crate::dialogs::DRUMPAD_PATH),
        (3, crate::dialogs::BUILTIN_PATH),
        (15, "/Library/Audio/Plug-Ins/VST3/Reverb.vst3"),
    ];
    let far: &[(usize, &str)] = &[
        (0, crate::dialogs::DRUMPAD_PATH),
        (3, crate::dialogs::BUILTIN_PATH),
        (6, "/Library/Audio/Plug-Ins/VST3/Pro-Q 3.vst3"),
        (9, "/Library/Audio/Plug-Ins/VST3/Rhodes Anthology.vst3"),
        (15, "/Library/Audio/Plug-Ins/VST3/Reverb.vst3"),
        (21, "/Library/Audio/Plug-Ins/VST3/Compressor.vst3"),
        (30, "/Library/Audio/Plug-Ins/VST3/Delay.vst3"),
        (45, "/Library/Audio/Plug-Ins/VST3/Chorus.vst3"),
        (4149 * crate::recorder::INSERTS, "/Library/Audio/Plug-Ins/VST3/Brusfri.vst3"),
    ];
    for (name, notes, bays) in [("cr78test-shaped", 140usize, low), ("moscow1-shaped", 2354, far)] {
        let mut arr = Arrangement::default();
        arr.version = ARRANGEMENT_VERSION;
        arr.named = true;
        for i in 0..12usize {
            arr.add_clip(ArrClip {
                pedal: Vec::new(),
                path: format!("media/take-{i:03}.wav"),
                name: format!("take {i}"),
                lane: i % 8,
                start_s: i as f64 * 4.0,
                len_s: 30.0,
                loop_s: 30.0,
                ..Default::default()
            });
        }
        let mut part = ArrClip {
            pedal: Vec::new(),
            kind: ClipKind::Midi,
            lane: 1,
            name: "part".into(),
            len_s: 64.0,
            loop_s: 64.0,
            ..Default::default()
        };
        for n in 0..notes {
            part.notes.push(ArrNote {
                at_s: n as f64 * 0.25,
                dur_s: 0.125,
                key: 36 + (n % 48) as u8,
                vel: 100,
            muted: false,
            });
        }
        arr.add_clip(part);
        let mut s = Settings::default();
        for (bay, path) in bays {
            if let Some(slot) = s.strip_inserts.get_mut(*bay) {
                *slot = (*path).to_owned();
            }
        }
        s.strip_names = vec!["drums".into(), "keys".into(), "gtr".into()];
        arr.desk = Some(desk_of(&s));
        let json = arr.to_json();
        // The desk's insert seats alone, by the same writer.
        let desk_only = {
            let mut bare = Arrangement::default();
            bare.desk = arr.desk.clone();
            bare.to_json().len()
        };
        let back = Arrangement::from_json(&json);
        assert_eq!(back.to_json(), json, "{name} is not a fixed point");
        println!(
            "doc_bytes    {name:<16} document {:>7} B; the desk alone {:>7} B; {} clips / {notes} notes / {} bays",
            json.len(),
            desk_only,
            arr.clips.len(),
            bays.len()
        );
    }
}
