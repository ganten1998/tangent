//! Writing arrangement notes out as a Standard MIDI File.
//!
//! The mirror of [`crate::smf_read`], and hand-rolled for the same reason its
//! header gives: this crate is firewalled from `tangent-record` and from
//! everything else with native ambitions, so the hundred lines of SMF
//! emitting live here rather than pulling a writer through the wall.
//!
//! # Why this is not `tangent_record::smf`
//!
//! There IS already a Standard MIDI File writer in this repo — 863 lines of
//! it, with ten named failure modes and a test each. It is not reused, and
//! that is a decision rather than an oversight. Three reasons, in the order
//! they bite:
//!
//! 1. **It is built around raw wire bytes on a wall clock.** Its input is
//!    `Captured { t: Nanos, bytes }` stamped into a `clock::Timeline` — the
//!    literal bytes a keyboard sent, so that half-pedalling, release
//!    velocity, pitch bend and program changes all survive a take. An
//!    arrangement clip has none of that. It has [`ArrNote`], which is four
//!    numbers, and no wire ever carried it. Feeding one to the other would
//!    mean synthesising the bytes a keyboard would have sent, which is the
//!    long way round to the three-byte messages this file writes directly.
//!
//! 2. **It writes ONE tempo and ONE meter for the whole file**, because a
//!    take is recorded against the click and the click had one setting. A
//!    SONG has a [`TempoMap`] and a [`MeterMap`]: export a project with a
//!    ritardando or a 4/4→3/4 through a constant-tempo writer and every bar
//!    line after the change lands somewhere else in the importing DAW.
//!
//! 3. **`scripts/check-firewall.sh` forbids `tangent-ui` from depending on
//!    `tangent-record` by name**, so the call could not be written even if
//!    the first two objections went away. Nor may `midly` be pulled in
//!    directly — see that script, where it is now named too.
//!
//! # What this writer does NOT re-learn
//!
//! `tangent_record::smf`'s ten failure modes are real and were paid for.
//! Three of them are reachable from here, and each is answered in the code
//! below at the point it would bite:
//!
//! - **Rule 1, no `EndOfTrack`.** A file without one opens in most readers
//!   and breaks on somebody else's machine. Both tracks get one, and it is
//!   placed at the material's real end rather than at the last note, so
//!   trailing silence survives the round trip.
//! - **Rule 6, rounding each delta instead of the absolute tick.** Delta
//!   rounding accumulates monotonically, so the error is DRIFT rather than
//!   jitter — which is precisely what a listener hears. Every tick here is
//!   quantised from absolute song time and the deltas are differenced
//!   afterwards.
//! - **Offs before ons at a shared tick.** A note that ends exactly where
//!   the next one of the same pitch begins must not be closed by the wrong
//!   note-off. The engine's `mix_channels` already emits offs first, and
//!   [`crate::smf_read`] already sorts for it; this writer emits in that
//!   order so all three agree.
//!
//! # What a round trip through `smf_read` cannot preserve
//!
//! Two notes of the SAME PITCH overlapping in one clip. The reader pairs a
//! note-off to the FIRST open note of that key, so the two come back with
//! their durations swapped. The FILE is correct — every other DAW pairs the
//! same way, because the format gives it nothing else to pair on — and the
//! reader is the lossy half. Worth knowing before reading too much into the
//! round-trip test at the bottom of this file.

use crate::arrangement::{ArrNote, ArrPedal};
use crate::tempo::{MeterMap, TempoMap};

/// Ticks per quarter note.
///
/// **The same 960 `tangent_record::smf::PPQ` is, for exactly the same
/// reasons**, restated here because the firewall makes sharing the constant
/// impossible and two different resolutions in one repo would mean a take
/// and an export quantising differently in the same DAW. The argument, from
/// there: 960 is Logic's native resolution and exact in Cubase, REAPER and
/// Live, so nothing requantises on import; it divides by 2, 3, 4, 5, 6, 8,
/// 10, 12, 15, 16, 20, 24 and 32, so every tuplet lands on an integer tick;
/// and at 120 BPM one tick is 520 µs, below the time MIDI takes to transmit
/// a three-byte message, so the quantisation is not the limiting factor.
///
/// If one of these two numbers ever changes, both must.
pub const PPQ: u16 = 960;

/// How many stepped tempo events one RAMPED segment may be sampled into.
///
/// See [`push_tempo_map`] for why a ramp is sampled at all. The cap is not
/// about file size — a tempo event is seven bytes, so even this many is a
/// rounding error next to the notes — but about a document somebody wrote by
/// hand with a ramp spanning ten thousand quarters. Past the cap the GRID
/// widens rather than the ramp being truncated, so the whole slide is still
/// represented, just more coarsely; a truncated ramp would leave the rest of
/// the song at the wrong tempo, which is the failure worth avoiding.
const RAMP_STEPS_MAX: usize = 512;

/// Emit `notes` — already in SONG seconds, measured from 0:00 — as a
/// two-track SMF: a conductor track carrying the song's tempo and meter, and
/// one performance track named `track_name`.
///
/// `end_s` is where the MATERIAL ends, which is not where the last note
/// ends: a track whose clips run to bar 9 but whose last note is in bar 7
/// has two bars of silence, and that silence is part of the arrangement. It
/// becomes the `EndOfTrack` position. A note that runs past it pushes it
/// out, because an `EndOfTrack` before an event it is supposed to terminate
/// is a malformed file.
///
/// **Format 1 and not format 0.** The tempo map has to live on its own track
/// for a DAW to adopt it as the project's tempo rather than as one track's
/// data, and a named performance track is what makes several exported files
/// identifiable once they are all dragged into the same session.
pub fn write(
    track_name: &str,
    notes: &[ArrNote],
    pedal: &[ArrPedal],
    end_s: f64,
    tempo: &TempoMap,
    meter: &MeterMap,
) -> Vec<u8> {
    // **Rule 6 lives in this closure and nowhere else.** Absolute song
    // seconds to an absolute tick, rounded once. Nothing below is allowed to
    // round a duration or a delta on its own; every tick comes from here and
    // the deltas are the differences of two of them.
    let tick = |s: f64| -> u64 {
        let beat = tempo.beat_at_seconds(if s.is_finite() { s.max(0.0) } else { 0.0 });
        let ticks = (beat * f64::from(PPQ)).round();
        if ticks.is_finite() && ticks > 0.0 {
            ticks as u64
        } else {
            0
        }
    };

    let performance = performance_track(track_name, notes, pedal, &tick, tick(end_s));
    // The conductor runs to the same end as the performance, so a DAW that
    // takes its song length from track 0 agrees with the notes.
    let conductor = conductor_track(tempo, meter, performance.end);

    let mut out = Vec::with_capacity(conductor.body.len() + performance.body.len() + 32);
    out.extend_from_slice(b"MThd");
    out.extend_from_slice(&6u32.to_be_bytes());
    out.extend_from_slice(&1u16.to_be_bytes()); // format 1
    out.extend_from_slice(&2u16.to_be_bytes()); // conductor + performance
    out.extend_from_slice(&PPQ.to_be_bytes());
    push_chunk(&mut out, &conductor.body);
    push_chunk(&mut out, &performance.body);
    out
}

/// A finished track chunk body, and the tick its `EndOfTrack` sits at.
struct Track {
    body: Vec<u8>,
    end: u64,
}

/// Track 0: the song's tempo and meter, and nothing else.
fn conductor_track(tempo: &TempoMap, meter: &MeterMap, end: u64) -> Track {
    // `(tick, order, payload)`. Order breaks ties at a shared tick: the
    // SIGNATURE first, because it is the coarser statement — a reader that
    // draws bar lines wants to know it is in 3/4 before it is told how fast.
    let mut evs: Vec<(u64, u8, Vec<u8>)> = Vec::new();
    push_meter_map(&mut evs, meter);
    push_tempo_map(&mut evs, tempo);
    evs.sort_by_key(|(at, order, _)| (*at, *order));

    let mut body = Vec::new();
    let mut prev = 0u64;
    let mut last = 0u64;
    for (at, _, payload) in &evs {
        push_varlen(&mut body, at.saturating_sub(prev));
        body.extend_from_slice(payload);
        prev = *at;
        last = last.max(*at);
    }
    let end = end.max(last);
    push_varlen(&mut body, end.saturating_sub(prev));
    body.extend_from_slice(&[0xFF, 0x2F, 0x00]);
    Track { body, end }
}

/// Every meter change as an SMF time-signature meta event.
///
/// A [`MeterPoint`](crate::tempo::MeterPoint) is addressed by BAR and the
/// file is addressed by TICK, so [`MeterMap::quarter_at_bar`] is the bridge —
/// which is also why a signature change can only land on a bar line, the
/// argument that type already makes.
fn push_meter_map(evs: &mut Vec<(u64, u8, Vec<u8>)>, meter: &MeterMap) {
    for p in meter.points() {
        let quarter = meter.quarter_at_bar(p.at_bar);
        let at = (quarter * f64::from(PPQ)).round().max(0.0) as u64;
        let unit = p.sig.unit.max(1);
        // The file stores the denominator as a POWER of two — 4 is 2, 8 is
        // 3 — because that is what a note value is. `TimeSignature::is_valid`
        // already restricts the unit to 1|2|4|8|16|32, so the count of
        // trailing zeros is exactly that power for every signature the app
        // can hold, and clamping covers a hand-edited document that is not
        // one of those.
        let power = unit.trailing_zeros().min(5) as u8;
        evs.push((
            at,
            0,
            vec![
                0xFF,
                0x58,
                0x04,
                p.sig.beats.clamp(1, 32),
                power,
                clocks_per_click(power),
                // Thirty-second notes per MIDI quarter. Always 8; nothing
                // has ever meant anything else by it, and a reader that
                // finds a different number here will do something surprising.
                8,
            ],
        ));
    }
}

/// MIDI clocks per metronome click, which is 24 per QUARTER note.
///
/// So a quarter-note beat is 24 and an eighth is 12 — the same arithmetic
/// `tangent_record::smf::Meter::clocks_per_click` does, and it is here for
/// the same reason: without it a 6/8 export makes the importing DAW's own
/// click play quarters against music counted in eighths.
fn clocks_per_click(unit_power: u8) -> u8 {
    let unit = 1u32 << u32::from(unit_power.min(5));
    ((24 * 4) / unit.max(1)).clamp(1, 255) as u8
}

/// Every tempo change as an SMF tempo meta event — with the ramps sampled.
///
/// **A ritardando has no SMF spelling.** The format's tempo event is a step
/// change and nothing else, so a segment that slides continuously from 120 to
/// 60 has to be either flattened or sampled, and the choice changes what the
/// importing DAW plays. The alternatives, both of which were considered:
///
/// - **Flatten to the segment's starting tempo.** The notes still land on
///   the right BEATS, because their ticks are computed from the real map, so
///   the bar lines are right and the notation is right. But the seconds are
///   wrong for the whole segment and the slowing-down — which is the thing
///   somebody drew a ramp to hear — is simply gone. Rejected: an export that
///   silently drops a written ritardando is an export that changes the
///   performance.
/// - **Sample it into a staircase**, which is what happens here. The grid is
///   ONE QUARTER NOTE, chosen because the quarter is the unit the tempo event
///   itself is denominated in and because a bar of 4/4 slowing from 120 to 60
///   is then four steps. The cost is seven bytes per quarter of ramp, against
///   a file whose notes are three bytes each.
///
/// **Each step carries the ramp's AVERAGE tempo over that step, not the
/// tempo at its start**, and that distinction is worth the two extra lines
/// it costs. Holding each step at its starting tempo is the rectangle rule,
/// and on a slowing ramp every rectangle is too fast, so the error does not
/// cancel — it accumulates in one direction, which is drift. Measured on the
/// test below: a 120→60 ramp over four quarters really takes 2.773 s, and a
/// start-tempo staircase gets through it in 2.538 s — a note landing after
/// the ramp is 235 ms early, and every note after that in the song is 235 ms
/// early too. The average tempo for a step is `beats·60 / (the seconds the
/// real map takes over those beats)`, which is exact by construction, so the
/// staircase and the ramp agree EXACTLY at every step boundary and the only
/// error left is inside a step and does not accumulate. The first version of
/// this used `bpm_at_beat` and the ramp test caught it.
///
/// The last sample of a ramped segment is deliberately NOT emitted at the
/// next point's beat: that point emits its own event, so the staircase lands
/// exactly on the tempo the map says it should reach.
fn push_tempo_map(evs: &mut Vec<(u64, u8, Vec<u8>)>, tempo: &TempoMap) {
    let points = tempo.points();
    for (i, p) in points.iter().enumerate() {
        let ramps_to = points.get(i + 1).filter(|next| {
            p.ramp && next.at_beat > p.at_beat && (next.bpm - p.bpm).abs() > 1.0e-9
        });
        let Some(next) = ramps_to else {
            evs.push((beat_tick(p.at_beat), 1, tempo_meta(p.bpm)));
            continue;
        };
        let span = next.at_beat - p.at_beat;
        // One step per quarter, but never more than the cap: past it the
        // GRID widens rather than the ramp being cut short. See
        // [`RAMP_STEPS_MAX`].
        let steps = (span.ceil().max(1.0) as usize).clamp(1, RAMP_STEPS_MAX);
        let step = span / steps as f64;
        for k in 0..steps {
            let beat = p.at_beat + step * k as f64;
            let took = tempo.seconds_at_beat(beat + step) - tempo.seconds_at_beat(beat);
            // A step that takes no time is a degenerate map, not a ramp;
            // the point's own tempo is the only honest thing to write.
            let bpm = if took > 1.0e-12 { step * 60.0 / took } else { p.bpm };
            evs.push((beat_tick(beat), 1, tempo_meta(bpm)));
        }
    }
}

fn beat_tick(beat: f64) -> u64 {
    let t = (beat * f64::from(PPQ)).round();
    if t.is_finite() && t > 0.0 { t as u64 } else { 0 }
}

/// A tempo meta event: microseconds per quarter note, three bytes.
///
/// Clamped into the range those three bytes can hold. `TempoMap` already
/// keeps every bpm inside `MIN_BPM..=MAX_BPM`, so the clamp is a guard
/// against arithmetic rather than against the document.
fn tempo_meta(bpm: f64) -> Vec<u8> {
    let us = if bpm.is_finite() && bpm > 0.0 {
        (60_000_000.0 / bpm).round().clamp(1.0, 0x00FF_FFFF as f64) as u32
    } else {
        500_000
    };
    let b = us.to_be_bytes();
    vec![0xFF, 0x51, 0x03, b[1], b[2], b[3]]
}

/// Track 1: the track's name, then its notes on channel 0.
///
/// **Channel 0 for everything**, deliberately. The arrangement has no notion
/// of a MIDI channel — a lane's sound is a plugin loaded in bay 1, not a
/// channel number — so any other channel would be an invention, and a file
/// that arrives on channel 7 for no reason is one somebody has to go and
/// re-channel by hand.
fn performance_track(
    name: &str,
    notes: &[ArrNote],
    pedal: &[ArrPedal],
    tick: &dyn Fn(f64) -> u64,
    end_s_tick: u64,
) -> Track {
    let mut body = Vec::new();
    push_varlen(&mut body, 0);
    push_track_name(&mut body, name);

    // `(tick, order, status, key, data)`. Order 0 is a note-OFF, 1 the
    // PEDAL and 2 a note-ON.
    //
    // Offs before ons is the old half of that rule: at a shared tick the sort
    // puts every off first, so a note that ends exactly where the next one of
    // the same pitch begins is closed by its own off and not by the new
    // note's.
    //
    // The pedal between them is the new half, and it is `tangent_record`'s
    // rule 3 read from the other end. A pedal-UP emitted BEFORE the offs at
    // its tick makes any reader that models sustain re-release the notes that
    // were meant to ring on — the invisible failure, where the piano roll
    // looks right and the sound is dry. And a pedal-DOWN emitted AFTER the
    // ons at its tick reaches a sampler a beat late for the attack it was
    // pressed under. One slot, between the two, answers both.
    let mut evs: Vec<(u64, u8, u8, u8, u8)> = Vec::with_capacity(notes.len() * 2);
    for n in notes {
        let key = n.key.min(127);
        let on = tick(n.at_s);
        // The END minus the START, both rounded once from absolute time —
        // rule 6 again, and the reason `dur_s` is never rounded on its own.
        // At least one tick long, because a zero-length note is a note-on
        // and a note-off at the same instant, which readers disagree about.
        let off = tick(n.at_s + n.dur_s.max(0.0)).max(on + 1);
        // **Velocity 0 is a note-OFF on the wire.** A note recorded or drawn
        // at zero would vanish as a note and leave a stray off behind it, so
        // the floor is 1 — the quietest thing MIDI can say.
        evs.push((on, 2, 0x90, key, n.vel.clamp(1, 127)));
        evs.push((off, 0, 0x80, key, 0x40));
    }
    // The pedal takes the middle slot. Its `key` byte is the controller
    // number, which is what makes the sort stable across the three of them.
    for e in pedal {
        if !matches!(e.cc, 64 | 66 | 67) {
            continue;
        }
        evs.push((tick(e.at_s), 1, 0xB0, e.cc, e.value.min(127)));
    }
    evs.sort_by_key(|(at, order, _, key, _)| (*at, *order, *key));

    let mut prev = 0u64;
    let mut last = 0u64;
    for (at, _, status, key, data) in &evs {
        push_varlen(&mut body, at.saturating_sub(prev));
        // **No running status.** It would save one byte per event on a
        // monophonic line and nothing at all on a chord, and every reader
        // that has ever mishandled a file has mishandled running status.
        // The saving is not worth being the interesting file.
        body.extend_from_slice(&[*status, *key, *data]);
        prev = *at;
        last = last.max(*at);
    }
    // Rule 1: `EndOfTrack`, at the MATERIAL's end. Pushed out past the last
    // note when the notes run longer, because an `EndOfTrack` before an event
    // it terminates is a malformed file.
    let end = end_s_tick.max(last);
    push_varlen(&mut body, end.saturating_sub(prev));
    body.extend_from_slice(&[0xFF, 0x2F, 0x00]);
    Track { body, end }
}

/// A `TrackName` meta event.
///
/// The bytes are UTF-8, which is what every reader written this century
/// assumes and what the app's own strings already are. Capped at 127 bytes so
/// the length is a single varlen byte and so a pathological track name cannot
/// dwarf the music, and cut on a CHAR boundary — a name truncated mid-codepoint
/// is not text any more, and this is the one field in the file made of
/// somebody's typing.
fn push_track_name(out: &mut Vec<u8>, name: &str) {
    let mut cut = name.len().min(127);
    while cut > 0 && !name.is_char_boundary(cut) {
        cut -= 1;
    }
    let bytes = &name.as_bytes()[..cut];
    out.extend_from_slice(&[0xFF, 0x03]);
    push_varlen(out, bytes.len() as u64);
    out.extend_from_slice(bytes);
}

/// An `MTrk` chunk around a finished body.
fn push_chunk(out: &mut Vec<u8>, body: &[u8]) {
    out.extend_from_slice(b"MTrk");
    out.extend_from_slice(&(u32::try_from(body.len()).unwrap_or(u32::MAX)).to_be_bytes());
    out.extend_from_slice(body);
}

/// A variable-length quantity: seven bits a byte, high bit set on all but the
/// last.
///
/// Saturating at the format's 28-bit maximum rather than wrapping. `midly`'s
/// own `u28::from` masks silently, which turns an overlong delta into roughly
/// 268 million ticks and puts the rest of the file 38 hours in the future —
/// `tangent_record::smf`'s rule 7, and worth not re-learning. Saturating gives
/// a visible gap instead of an invisible wrap, and the gap is unreachable
/// anyway: 28 bits is 78 hours of quarter notes at this resolution.
fn push_varlen(out: &mut Vec<u8>, v: u64) {
    let mut x = v.min(0x0FFF_FFFF) as u32;
    let mut stack = [0u8; 4];
    let mut n = 0usize;
    loop {
        stack[n] = (x & 0x7F) as u8;
        n += 1;
        x >>= 7;
        if x == 0 {
            break;
        }
    }
    for i in (0..n).rev() {
        out.push(stack[i] | if i == 0 { 0x00 } else { 0x80 });
    }
}

/// What a walk of a written file found.
#[cfg(test)]
pub(crate) struct Walked {
    /// Every `EndOfTrack` tick, one per chunk.
    pub ends: Vec<u64>,
    /// Every channel message, `(tick, status, key, data)`, **in the order
    /// the file lists them** — which is the whole reason this exists.
    pub events: Vec<(u64, u8, u8, u8)>,
}

/// Walk a written file's chunks: the raw events, in file order, and the
/// `EndOfTrack` ticks.
///
/// **A test-only reader, and deliberately not [`crate::smf_read`]'s job**,
/// for two separate reasons that each cost a green test to learn.
///
/// The first: that reader ignores every meta event but tempo, so its `len_s`
/// is the last thing that made a SOUND — exactly the number that cannot
/// answer "did the trailing silence survive".
///
/// The second is sharper, and is why `events` is here. `smf_read` SORTS what
/// it finds, offs before ons at a shared tick, before it pairs anything. So a
/// round trip through it cannot see the order this writer put the bytes in:
/// emit the on before the off and the reader silently repairs it, and the
/// ordering test passes against a writer that does the wrong thing. The first
/// version of `an_off_and_an_on_at_one_tick_go_off_first` was written that
/// way and stayed green when the ordering was deliberately reversed. Only the
/// bytes can answer a question about the bytes.
///
/// It lives out here rather than in this file's own `mod tests` because the
/// arrangement export's tests in `app.rs` need the same answers, and a second
/// hand-rolled chunk walker over there would be a second thing to keep right.
#[cfg(test)]
pub(crate) fn walk(bytes: &[u8]) -> Walked {
    let mut ends = Vec::new();
    let mut events = Vec::new();
    let mut at = 14usize; // past MThd and its six bytes
    while at + 8 <= bytes.len() {
        let len = u32::from_be_bytes([
            bytes[at + 4],
            bytes[at + 5],
            bytes[at + 6],
            bytes[at + 7],
        ]) as usize;
        let (mut i, end) = (at + 8, at + 8 + len);
        let mut tick = 0u64;
        let mut status = 0u8;
        while i < end {
            // delta
            let mut v = 0u64;
            loop {
                let b = bytes[i];
                i += 1;
                v = (v << 7) | u64::from(b & 0x7F);
                if b & 0x80 == 0 {
                    break;
                }
            }
            tick += v;
            let first = bytes[i];
            if first & 0x80 != 0 {
                status = first;
                i += 1;
            }
            match status {
                0xFF => {
                    let kind = bytes[i];
                    i += 1;
                    let mut n = 0usize;
                    loop {
                        let b = bytes[i];
                        i += 1;
                        n = (n << 7) | usize::from(b & 0x7F);
                        if b & 0x80 == 0 {
                            break;
                        }
                    }
                    if kind == 0x2F {
                        ends.push(tick);
                    }
                    i += n;
                }
                0x80 | 0x90 => {
                    events.push((tick, status, bytes[i], bytes[i + 1]));
                    i += 2;
                }
                other => panic!("unexpected status {other:#04x}"),
            }
        }
        at = end;
    }
    Walked { ends, events }
}

/// Just the `EndOfTrack` ticks, for a caller that wants only those.
#[cfg(test)]
pub(crate) fn end_of_track_ticks(bytes: &[u8]) -> Vec<u64> {
    walk(bytes).ends
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recorder::TimeSignature;
    use crate::tempo::{MeterPoint, TempoPoint};

    fn note(at_s: f64, dur_s: f64, key: u8, vel: u8) -> ArrNote {
        ArrNote { at_s, dur_s, key, vel, muted: false }
    }

    /// **The bytes a reader gets back are the notes that went in.**
    ///
    /// Written against `smf_read` rather than against this file's own
    /// arithmetic on purpose: an assertion about ticks would pass whatever
    /// tick math this writer used, including a wrong one, because it would be
    /// the same math on both sides. Going out through the file and back
    /// through an independently-written parser is what makes the number mean
    /// something.
    ///
    /// The tempo map has two points at two different tempos, so the walk from
    /// seconds to ticks and back has to survive a change of segment — which
    /// is precisely the case a constant-tempo writer would get wrong and a
    /// constant-tempo test would never notice.
    #[test]
    fn smf_write_round_trips_through_smf_read() {
        let tempo = TempoMap::sanitised(vec![
            TempoPoint { at_beat: 0.0, bpm: 120.0, ramp: false },
            // Half speed from bar 3 (beat 8), which at 120 bpm is 4.0 s.
            TempoPoint { at_beat: 8.0, bpm: 60.0, ramp: false },
        ]);
        let meter = MeterMap::constant(TimeSignature { beats: 4, unit: 4 });
        let notes = vec![
            note(0.0, 0.5, 60, 100),
            note(0.5, 0.25, 64, 40),
            note(2.0, 1.0, 67, 127),
            // Past the tempo change: 4.0 s is beat 8, and a quarter now
            // lasts a whole second.
            note(4.0, 1.0, 72, 90),
            note(6.0, 2.0, 48, 1),
        ];
        let bytes = write("Bass", &notes, &[], 10.0, &tempo, &meter);
        let got = crate::smf_read::read(&bytes).expect("the writer's own file was refused");
        assert_eq!(got.notes.len(), notes.len(), "{:?}", got.notes);
        for (want, back) in notes.iter().zip(&got.notes) {
            // One tick at the SLOWEST tempo in the map is the coarsest this
            // can be: 60 bpm over 960 ticks is 1.04 ms.
            assert!(
                (want.at_s - back.at_s).abs() < 2.0e-3,
                "{} came back at {}",
                want.at_s,
                back.at_s
            );
            assert!(
                (want.dur_s - back.dur_s).abs() < 2.0e-3,
                "a {}s note came back {}s long",
                want.dur_s,
                back.dur_s
            );
            assert_eq!(want.key, back.key);
            assert_eq!(want.vel, back.vel);
        }
    }

    /// **`EndOfTrack` sits at the material's end, not at the last note.**
    ///
    /// Rule 1's second half. A file that stops at the last note loses every
    /// bar of trailing silence, which is how an exported four-bar phrase with
    /// a two-bar tail comes back as a two-bar file.
    #[test]
    fn the_file_ends_where_the_material_does_and_not_at_the_last_note() {
        let tempo = TempoMap::constant(120.0);
        let meter = MeterMap::constant(TimeSignature { beats: 4, unit: 4 });
        // One note in the first second; the material runs to three.
        let bytes = write("T", &[note(0.0, 0.5, 60, 100)], &[], 3.0, &tempo, &meter);
        // 3 s at 120 bpm is six quarters.
        let want = 6 * u64::from(PPQ);
        let ends = end_of_track_ticks(&bytes);
        assert_eq!(ends.len(), 2, "both tracks need an EndOfTrack: {ends:?}");
        assert!(
            ends.iter().all(|e| *e == want),
            "the trailing silence was trimmed: {ends:?} should both be {want}"
        );

        // And the other direction: a note that outruns `end_s` pushes the
        // end out, because an EndOfTrack before an event is a broken file.
        let bytes = write("T", &[note(0.0, 4.0, 60, 100)], &[], 1.0, &tempo, &meter);
        let ends = end_of_track_ticks(&bytes);
        assert!(
            ends.iter().all(|e| *e == 8 * u64::from(PPQ)),
            "the EndOfTrack landed before the note it terminates: {ends:?}"
        );
    }

    /// **A note that ends exactly where the next begins is not closed by the
    /// wrong off**, because every off at a tick is written before every on.
    ///
    /// This is the case that sounds like choking: two notes of the same pitch
    /// back to back, where an on-before-off ordering switches the second one
    /// off the instant it starts. The engine's `mix_channels` already emits
    /// offs first for exactly this reason; a file that does not is a file
    /// that plays differently in somebody else's DAW than it does here.
    ///
    /// **Asserted against the BYTES, not against a round trip**, and that is
    /// the point of the test rather than an implementation detail.
    /// `smf_read` sorts offs before ons at a shared tick before it pairs
    /// anything, so it repairs a file written in the wrong order and reports
    /// two clean half-second notes either way. The first version of this test
    /// went through `read` and stayed GREEN when the ordering was
    /// deliberately reversed — it was measuring the reader's sort, not the
    /// writer's output. Only the bytes can answer a question about the bytes.
    ///
    /// **The notes are handed over LATEST FIRST, deliberately.** The sort in
    /// the writer is stable, so a fixture in time order gets offs before ons
    /// at the shared tick whether or not the ordering rule is applied at all
    /// — the push order alone happens to produce it — and deleting the rule
    /// from the sort key leaves this test green. Reversed, the push order
    /// works AGAINST the rule and only the rule can produce the right bytes.
    /// The writer must not depend on its caller having sorted anything, and
    /// this is what says so.
    #[test]
    fn an_off_and_an_on_at_one_tick_go_off_first() {
        let tempo = TempoMap::constant(120.0);
        let meter = MeterMap::constant(TimeSignature { beats: 4, unit: 4 });
        let notes = vec![note(0.5, 0.5, 60, 100), note(0.0, 0.5, 60, 100)];
        let bytes = write("T", &notes, &[], 1.0, &tempo, &meter);

        // 0.5 s at 120 bpm is one quarter: the shared tick.
        let shared = u64::from(PPQ);
        let at_shared: Vec<u8> = walk(&bytes)
            .events
            .into_iter()
            .filter(|(tick, ..)| *tick == shared)
            .map(|(_, status, ..)| status)
            .collect();
        assert_eq!(
            at_shared,
            vec![0x80, 0x90],
            "the note-on was written before the note-off that shares its tick"
        );

        // And the file still says what it should once read back.
        let got = crate::smf_read::read(&bytes).expect("refused its own file");
        assert_eq!(got.notes.len(), 2, "{:?}", got.notes);
        assert!(
            got.notes.iter().all(|n| (n.dur_s - 0.5).abs() < 2.0e-3),
            "a note was closed by the other one's off: {:?}",
            got.notes
        );
    }

    /// **A ritardando survives as a staircase**, rather than being flattened
    /// into a tempo the music never had.
    ///
    /// The measurement is the SECONDS a reader lands the notes at, because
    /// that is what the sampling exists to protect: the ticks would be right
    /// under a flattened tempo track too. A ramp from 120 to 60 over four
    /// quarters takes 60·4/(60−120)·ln(60/120) = 2.7726 s, against 2.0 s if
    /// the segment were held at 120 and 4.0 s if it were held at 60 — so a
    /// flattened export lands the note after it three quarters of a second
    /// out either way, and the assertion below fails.
    ///
    /// **The tolerance is five milliseconds, and it is that tight
    /// deliberately.** A staircase whose steps carry the ramp's average
    /// tempo is exact at every step boundary, and this note sits on one; a
    /// staircase carrying each step's STARTING tempo — the obvious version,
    /// which is what this first shipped as — arrives 235 ms early, and a
    /// loose tolerance would have let it through.
    #[test]
    fn a_ramped_tempo_is_sampled_rather_than_flattened() {
        let tempo = TempoMap::sanitised(vec![
            TempoPoint { at_beat: 0.0, bpm: 120.0, ramp: true },
            TempoPoint { at_beat: 4.0, bpm: 60.0, ramp: false },
        ]);
        let meter = MeterMap::constant(TimeSignature { beats: 4, unit: 4 });
        let at = tempo.seconds_at_beat(4.0);
        assert!((at - 2.7726).abs() < 1.0e-3, "the fixture moved: {at}");
        let bytes = write("T", &[note(at, 1.0, 60, 100)], &[], at + 1.0, &tempo, &meter);
        let got = crate::smf_read::read(&bytes).expect("refused its own file");
        assert!(
            (got.notes[0].at_s - at).abs() < 5.0e-3,
            "the ramp did not survive: the note landed at {} instead of {at}",
            got.notes[0].at_s
        );
    }

    /// **The meter reaches the file, in the units it is stored in.**
    ///
    /// 6/8 rather than 4/4 because it exercises both halves that get written
    /// wrong: the denominator as a POWER of two, and the clocks-per-click
    /// that stops the importing DAW counting the bar in quarters.
    #[test]
    fn the_time_signature_is_written_as_the_file_spells_it() {
        let tempo = TempoMap::constant(120.0);
        let meter = MeterMap::sanitised(vec![
            MeterPoint { at_bar: 0, sig: TimeSignature { beats: 6, unit: 8 } },
            MeterPoint { at_bar: 2, sig: TimeSignature { beats: 3, unit: 4 } },
        ]);
        let bytes = write("T", &[note(0.0, 0.5, 60, 100)], &[], 8.0, &tempo, &meter);
        // 6/8 → beats 6, power 3, 12 clocks per click, 8 thirty-seconds.
        let six_eight = [0xFFu8, 0x58, 0x04, 6, 3, 12, 8];
        let three_four = [0xFFu8, 0x58, 0x04, 3, 2, 24, 8];
        assert!(
            bytes.windows(7).any(|w| w == six_eight),
            "6/8 never reached the file"
        );
        assert!(
            bytes.windows(7).any(|w| w == three_four),
            "the signature change never reached the file"
        );
    }

    /// A varlen is the format's own encoding, checked against the two
    /// examples the spec itself gives.
    #[test]
    fn the_delta_encoding_is_the_formats() {
        let mut b = Vec::new();
        push_varlen(&mut b, 0);
        assert_eq!(b, vec![0x00]);
        b.clear();
        push_varlen(&mut b, 127);
        assert_eq!(b, vec![0x7F]);
        b.clear();
        push_varlen(&mut b, 128);
        assert_eq!(b, vec![0x81, 0x00]);
        b.clear();
        push_varlen(&mut b, 0x0800_0000);
        assert_eq!(b, vec![0xC0, 0x80, 0x80, 0x00]);
        // Past the format's 28 bits it SATURATES rather than wrapping to a
        // small number — rule 7.
        b.clear();
        push_varlen(&mut b, u64::MAX);
        assert_eq!(b, vec![0xFF, 0xFF, 0xFF, 0x7F]);
    }
}

#[cfg(test)]
mod pedal_tests {
    use super::*;
    use crate::recorder::TimeSignature;

    fn note(at: f64, dur: f64, key: u8, vel: u8) -> ArrNote {
        ArrNote { at_s: at, dur_s: dur, key, vel, muted: false }
    }

    /// **The pedal makes the round trip through a real file**, at its own
    /// value rather than as a switch — a half-pedalled 40 must not come back
    /// as 0 or as 127.
    #[test]
    fn the_pedal_survives_a_file_and_keeps_its_value() {
        let tempo = TempoMap::constant(120.0);
        let meter = MeterMap::constant(TimeSignature { beats: 4, unit: 4 });
        let notes = vec![note(0.0, 0.5, 60, 100), note(1.0, 0.5, 64, 90)];
        let pedal = vec![
            ArrPedal { at_s: 0.0, cc: 64, value: 127 },
            ArrPedal { at_s: 0.75, cc: 64, value: 40 },
            ArrPedal { at_s: 1.5, cc: 64, value: 0 },
            ArrPedal { at_s: 2.0, cc: 66, value: 100 },
            ArrPedal { at_s: 2.5, cc: 67, value: 64 },
        ];
        let bytes = write("Keys", &notes, &pedal, 4.0, &tempo, &meter);
        let got = crate::smf_read::read(&bytes).expect("the writer's own file was refused");
        assert_eq!(got.pedal.len(), pedal.len(), "{:?}", got.pedal);
        for (want, back) in pedal.iter().zip(&got.pedal) {
            assert!(
                (want.at_s - back.at_s).abs() < 2.0e-3,
                "{} came back at {}",
                want.at_s,
                back.at_s
            );
            assert_eq!(want.cc, back.cc);
            assert_eq!(
                want.value, back.value,
                "the pedal came back as a switch rather than at its value"
            );
        }
        // And the notes are untouched by the company they now keep.
        assert_eq!(got.notes.len(), notes.len());
    }

    /// **A pedal-UP is written at or AFTER the note-offs at its tick, and a
    /// pedal-DOWN before the note-ons.**
    ///
    /// `tangent_record::smf`'s rule 3, from the other end. An up emitted
    /// first makes any reader that models sustain re-release the notes that
    /// were meant to ring on — the invisible failure, where the piano roll
    /// looks right and the sound is dry.
    ///
    /// The bytes are read directly rather than through `smf_read`, because
    /// the claim IS the order and a reader that re-sorts would hide it.
    #[test]
    fn the_pedal_sits_between_the_offs_and_the_ons_at_a_shared_tick() {
        let tempo = TempoMap::constant(120.0);
        let meter = MeterMap::constant(TimeSignature { beats: 4, unit: 4 });
        // A note ending at 1.0 and another starting at 1.0, with the pedal
        // lifting on the same instant: all three land on one tick.
        let notes = vec![note(0.0, 1.0, 60, 100), note(1.0, 1.0, 67, 100)];
        let pedal = vec![ArrPedal { at_s: 1.0, cc: 64, value: 0 }];
        let bytes = write("K", &notes, &pedal, 3.0, &tempo, &meter);
        // Every three-byte channel message in the performance track, in
        // order. The file has no running status by design, so a scan for the
        // status bytes is exact.
        let mut seen: Vec<u8> = Vec::new();
        let mut i = 0;
        while i + 2 < bytes.len() {
            match bytes[i] {
                s @ (0x80 | 0x90 | 0xB0) => {
                    seen.push(s);
                    i += 3;
                }
                _ => i += 1,
            }
        }
        // off(60) … then the pedal … then on(67). The first on(60) precedes
        // all of them.
        let tail: Vec<u8> = seen.into_iter().skip(1).collect();
        assert_eq!(
            tail,
            vec![0x80, 0xB0, 0x90, 0x80],
            "the pedal is not between the offs and the ons at its tick"
        );
    }
}
