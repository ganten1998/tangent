//! The harp: the strings nobody struck, ringing with the ones somebody did,
//! and a little of the one somebody did.
//!
//! **What this file is for.** Lift the sustain pedal's dampers and every
//! string in the case is free to answer whatever is being played; a bass
//! note struck under the pedal leaves the octave, the twelfth and the
//! double octave ringing after its own damper has fallen, and a pedalled
//! chord sits in a wash that is the rest of the harp humming along. That
//! is sympathetic resonance, and it is the reason a piano with the pedal
//! down sounds like a different instrument rather than a longer one. The
//! measured reference showed it does more than that with the pedal UP: a
//! held note's own strings ring sympathetically under the note at about
//! 30 dB below it, and when the key comes up that ring is what is left
//! once the damper has taken the note down — its whole post-release tail.
//! Switch the reference's sympathetic bank off and a released C4's
//! partials fall from -46..-61 dB at half a second to -64..-92 dB.
//!
//! **Strings, not a matrix.** Eighty-eight string loops, one per key, each
//! fed the bridge sum and each summing back into the bridge. A loop is a
//! resonator at exactly its string's partials, so it takes energy from the
//! drive only where the drive has a partial that lands on one of its own,
//! and gives it back slowly at its own decay — which is the whole of the
//! effect, with no table of which note excites which. Which strings an
//! instrument has, where their partials sit and how long they ring is a
//! [`StringSet`]: the modal grand strings the harp from its own scale and
//! stretch (`StringSet::grand`, voiced per instrument by a [`GrandHarp`]),
//! and an instrument with other strings builds its own set. A set is
//! designed into coefficients once, off the audio thread ([`Strung`]), and
//! loading it is a copy (`Harp::load`).
//!
//! **Its own loop, and a lossless one.** A loop is a line sized to its own
//! pitch out of one arena, a one-pole loss with a gain in front, ONE
//! all-pass for the stiffness, and the delay's fraction read through a
//! first-order Thiran all-pass rather than linear interpolation. The
//! interpolator was a low-pass inside the feedback path: `|1 - f + f
//! e^{-jw}|` per pass, so every partial lost energy the loss design never
//! saw and the amount moved with the fraction from key to key and from one
//! sample rate to the next (`every_string_rings_at_its_designed_t60`). The
//! Thiran is an all-pass — magnitude one at every frequency — and its
//! phase is counted in the loop's tuning, so a string rings at its designed
//! T60 at every key and every rate.
//!
//! **Tuned to the instrument's strings.** A sympathetic string answers a
//! partial within its own bandwidth of it, a fraction of a cent for a
//! held C4. The modal grand is stretch-tuned (A0 -6, C7 +16 cents) and its
//! partials sit on its own B, so the harp takes both from the grand's
//! scale (`grand::tuning`): the loop's fundamental at the stretched pitch
//! and its all-pass fitted so its low partials land where the grand's do.
//! STRETCH, which scales both on the voices, re-strings them as it moves
//! (`Strung::follow`), a ringing string gliding to its new tuning.
//!
//! **Fed flat, and shaped for the strings nobody struck.** A loop is a comb
//! with a resonance at DC as well as at every partial, and fed the bridge
//! sum as it stands a short treble loop answers a bass note as that DC
//! resonance. The first answer to that fed every loop the drive's first
//! difference, normalised at its own fundamental: n times as much at
//! partial n. That made every loop a treble amplifier — the bench's stress
//! score (sixty-four pedalled keys) came back with the concert grand's harp
//! 9.7 dB OVER its voices at 2-4 kHz and +7.1 dBFS at its peak, where the
//! reference's whole bank adds a fifth of a decibel. So each loop now hears
//! the drive high-passed under its own fundamental and flat above it
//! (`Feed`: one filter per octave band of fundamentals, shared by every loop
//! in the band, a few multiplies a sample rather than a few a loop-sample),
//! its coupling normalised so its fundamental takes exactly its coupling. A
//! string being played — its key held, or let go while its voice sounds —
//! hears that flat feed: its halo is flat across its partials, as the
//! reference's is. A string nobody struck hears it shaped (`TILT_SAT`,
//! `BASS_BANDS`): above the bass, a little more of its upper partials than
//! of its fundamental, the way the reference's held strings answer a bass
//! note more above their fundamentals than at them; in the bass, its first
//! partials and little above, so the bass strings answer a pedalled
//! passage's bass without filling the floor between a bass note's partials.
//! A string crossing from one feed to the other crossfades (`Loop::cis`).
//!
//! **The struck string's own ring.** Key k's loop and key k's voice are the
//! same physical string. Strung at the voice's own pitch, coupled as
//! strongly as a free string and left to ring at the string's own decay,
//! the loop rings sympathetically with its own voice — an exact resonance,
//! every partial — and the note swells a decibel and a half over two
//! seconds, which is not a piano. The reference's ring under a held note
//! is 28 to 35 dB below it, reaches that within 50 to 100 ms, and then
//! holds its ratio to the note to the key-up: a FORCED response, the answer
//! of a resonator whose decay outruns its driver's, which settles onto the
//! drive at once and follows it. So while its key is held a string runs the
//! short `HALO_T60` loss, coupled at its `own` strength by key; every loop
//! also sits `DETUNE_CENTS` sharp of the string it shadows
//! (`a_held_note_carries_its_own_halo_and_does_not_swell`). **Let go, it is
//! the harp's again**: the halo's broad loss answers every neighbour's
//! partials as readily as its own, and kept for a released key under the
//! pedal it put a pedalled passage's harp within 2 dB of its voices from 500
//! Hz to 4 kHz; a released string rings free under the pedal at `AFTER` of
//! its free coupling, building on its own voice as the reference's bank
//! builds under a pedalled chord (from 22 to 12 dB under the voices over
//! three seconds), and rings out damped without it.
//!
//! **Dampers are a loss, not a gate, and a felt bites before it rests.** A
//! damper felt on a string adds loss; it does not delete the string. When
//! a key comes up (pedal up) or the pedal lifts, the loop stops being fed
//! and its loss moves to the damped one: the reference's residual decays
//! at 1.2 s T60 at 65 Hz rising to 2-3 s above 400 Hz, and a string rings
//! out at that rate instead of being faded in 55 ms. A string that was
//! ringing FREE when the felt lands — a sympathetic string under a lifted
//! pedal, a released key's string, a silently held key let go — first takes
//! the felt's bite, the `LAND_T60` loss for `LAND_SECS`, which is the
//! damper's own rate on the strings it lands on (about 200 dB/s), and only
//! then the residual: so the pedal's wash falls with the voices instead of
//! ringing on 15 to 45 dB over the reference. A string leaving its halo
//! (its key let go, pedal up) instead eases from the halo's loss to the
//! damped one over `TAIL_SECS`, which is the drop the reference shows between
//! the ring and the tail. What is left is heard as it sounds (`HEARD`), and
//! continues the note rather than cancelling it where the two cross. A key
//! above the instrument's last damper has no felt at all and is never
//! damped.
//!
//! **The pedal is a level.** The grand's dampers follow the sustain pedal
//! part of the way (half pedalling), and so do the harp's (`set_sustain`):
//! the felt's pressure closes a released string's coupling and blends its
//! loss toward the felt's in proportion, with the travel the grand's own
//! dampers have (`PEDAL_LIFT`). A pedal brought to half way in one message
//! or by steps lands the strings where letting a key go at half pedal does.
//!
//! **Aliquots.** A grand strung with aliquots carries a fourth, unstruck,
//! undamped string over each treble course. They ring with every treble
//! note and are why the intimate reference's top octave keeps a sustain
//! its struck strings do not have. `GrandHarp::INTIMATE` adds one per key
//! from F#6 up.
//!
//! **Most of the harp is silent most of the time, so most of it does not
//! run.** A loop ticks only while it is worth ticking. A strike wakes every
//! fed loop for a probation of ten of its own periods (at least 30 ms),
//! because the only way to learn whether a string answers a note is to let
//! it try; after that a loop whose contribution has stayed under the
//! audible floor — absolute, or far under the drive — for longer than one
//! of its own periods is put to sleep and CLEARED, so a loop that starts
//! ticking always starts from zero and opening one can never pop. Pedal
//! down and nothing struck is zero ticks.
//!
//! **One way.** The harp hears the bridge; the bridge does not hear the
//! harp taking energy out of the struck string, because the voices take no
//! input. What that costs is that a string driven at exact resonance for
//! longer than its own rise time can build toward `couple² / (1 − g)` of
//! the driving partial where a real string would have drained its driver
//! first. Staccato and ordinary pedalling never get near it, and
//! `a_held_note_never_out_rings_its_own_partial` holds a fortissimo C2 for
//! four seconds under the pedal: its partials move under 3 dB, and the
//! harp's C3, G3 and C4 never build past what drove them.

use super::grand::{self, GrandBase};
use super::string::{Ap, Lp, PianoBase};

/// One loop per key of the keyboard, A0 to C8.
pub const KEYS: usize = 88;
const FIRST_KEY: u8 = 21;

/// At most one aliquot string per key from F#6 to C8.
pub const ALIQUOTS: usize = 19;
const ALIQUOT_FIRST: u8 = 90;
const LOOPS: usize = KEYS + ALIQUOTS;

/// How often the gate looks. 256 samples is 5 ms at 48 kHz: a loop that
/// has gone silent runs at most that long for nothing, and the scan over
/// the loops costs once per frame what one loop costs per sample.
const FRAME: usize = 256;

/// A freshly woken loop runs this many of its own periods, and at least
/// `PROBATION_MIN_SECS`, before the gate judges it. A string on resonance
/// builds like a coherent sum: after m periods it stands about m times over
/// its forced response, so ten periods put an answering string 20 dB clear
/// of one that is not. In periods rather than seconds because a C7 string
/// has declared itself in five milliseconds and an A0 string needs a third
/// of a second; a flat 150 ms kept every treble loop on probation through
/// any passage faster than seven notes a second.
const PROBATION_PERIODS: f32 = 10.0;
const PROBATION_MIN_SECS: f32 = 0.030;

/// The felt's travel: how long a coupling takes to move when a damper
/// lands or lifts, the same 55 ms as the voices' own dampers. A string's
/// coupling that switched in a sample would write a step into the line.
const DAMP_SECS: f32 = 0.055;

/// How fast a ringing string re-tuned for a moved knob glides to its new
/// design (`Harp::retune`): the time constant, seconds, of its line's
/// length, its all-pass's delay and its loss pole, so that most of a pitch
/// move is made in the first 60 ms; and how near they come (samples, and
/// the pole as a fraction) before the design's own coefficients are set. An
/// `f32` glide stops moving a few ten-thousandths of a sample short, so the
/// last step is a few thousandths of a sample's delay.
const GLIDE_SECS: f32 = 0.02;
const GLIDE_DONE: f32 = 2.0e-3;

/// The most a gliding string's line length moves in a sample, samples:
/// half a sample a frame, the longest run, so the line steps at most once a
/// run (`Glide::step_line`). The bass's widest glide over the knob's whole
/// travel, A0's 19 samples, takes a fifth of a second.
const SPAN_STEP: f32 = 0.5 / FRAME as f32;

/// How long a line step's crossfade is, samples (`Glide::step_line`): the
/// two reads differ only near Nyquist, so in samples, not seconds.
const FADE: u16 = 64;

/// A first-order all-pass `(a + z^-1) / (1 + a z^-1)`'s coefficient from its
/// DC delay, and its delay from its coefficient: the one map, both ways.
fn allpass_delay(x: f32) -> f32 {
    (1.0 - x) / (1.0 + x)
}

/// How much audio pays for re-stringing one string as the knob that tunes
/// it moves (`Strung::follow`), seconds: 64 frames at 48 kHz, the pace of the
/// grand's table, which re-solves a key for every 64 (`grand::GrandDesigns`).
/// A string is a Newton solve and a few copies, 5 to 10 µs, so following
/// takes under 1 % of the audio's own time, and a set's 107 strings follow a
/// moved knob in 0.14 s of audio, where the table's 88 keys take 0.12.
/// STRETCH dragged under a pedalled eight-note chord (a new value every 800
/// frames) runs the frame at 15.5 to 20 times real time, the table and the
/// harp following it, against 20 to 28 with the table alone and 29 to 47
/// unmoved; at 16 frames a string it ran at 12 to 14 where 64 ran at 18.5 to
/// 19.4 (128-frame calls, both grands).
pub const RESTRING_SECS: f32 = 64.0 / 48_000.0;

/// How long a string's LOSS takes to move to a new value, seconds — slower
/// than its couplings, on purpose. When a sounding key comes up its loop
/// leaves the short `HALO_T60` loss for the damped one, and for this long
/// it is still losing fast while the note under it falls: the drop the
/// reference shows between the ring under a held note and the tail it
/// leaves (C4 n1 from -32 dB under the note to -59 dB under its key-off
/// level at +0.5 s, about 20 dB of it in the first third of a second).
const TAIL_SECS: f32 = 0.07;

/// The felt's bite on a string that was ringing free: the loss a damper
/// gives the string it lands on, as a T60 at the fundamental, and how long
/// it bites before the string is left to its damped residual. The voices'
/// own dampers take C4's fundamental down at 190-210 dB/s over the first
/// quarter second (the reference: -188..-214), 0.3 s of T60; a fifth of a
/// second of it is 40 dB, and the residual the reference leaves is 50 to
/// 60 dB under where the note stood. Without it a sympathetic string
/// under a lifted pedal went straight to the residual's 2 s and the wash
/// rang on: a pedalled cluster's level half a second after the lift was
/// -28 dB re the moment before it on the concert grand, against the
/// reference's -60 (verifier finding 2).
const LAND_T60: f32 = 0.3;
const LAND_SECS: f32 = 0.2;

/// The sustain pedal's travel against the harp's dampers, from CC 64 as
/// 0..1: under `[0]` they rest on the strings with their full weight, over
/// `[1]` they are clear of them, and between they bear with a pressure
/// `x^[2]` of their full one, `x` the travel left to clear. The modal
/// grand's own dampers' travel (`grand::GrandBase::pedal_lift` on the
/// grand lane, the same for both instruments), so a half-pedalled string
/// and its voice are damped by the same felt.
const PEDAL_LIFT: [f32; 3] = [0.12, 0.76, 2.3];

/// Below this RMS a loop's contribution is put to sleep and cleared:
/// -146 dBFS per string. Clearing a loop that quiet is not a step anyone
/// can hear, which is the only condition under which a ringing thing may be
/// cleared (`pickup::Transducer::clear`). It is this low for what the tail
/// is measured against, not for what can be heard: a released C4 held two
/// seconds at MIDI 80 is at -51 dBFS when the key comes up, the reference
/// leaves its tail 50 to 65 dB under that at +0.5 s and falling 30 dB a
/// second, and the bench reads the fall over 0.3-1.5 s, which ends near
/// -146. The mode bank's own cull line, -120 (`modal::CULL_FLOOR`), cut the
/// window at one second and read -70 dB/s for -30.
const FLOOR_RMS: f32 = 5.0e-8;

/// A loop contributing this far under what the bridge is driving it with
/// (power, -70 dB) is put to sleep too: it is either a non-resonant
/// string's forced copy of the drive, which sits at the drive's own
/// frequencies and is masked by it, or a string rung out seventy decibels
/// under the playing. At -80 the twenty undamped treble strings and the
/// aliquots, fed by every note, stayed awake on their forced copies through
/// any passage (`print_loops_running`).
const REL_SLEEP: f32 = 1.0e-7;

/// The largest drive sample the harp takes, absolute: two hundred times
/// full scale, far over anything a voice sends, and small enough that no
/// sum in a loop whose gain is under one can overflow `f32`.
const DRIVE_MAX: f32 = 200.0;

/// A drive whose RMS is under this cannot build anything audible through
/// the couplings below, so a loop under the floor with a drive under this
/// has nothing to wait for.
const DRIVE_QUIET_RMS: f32 = 1.0e-4;

/// The largest delay the stiffness all-pass is allowed, seconds: the
/// piano's 19 samples at 48 kHz, as a TIME, so the harp's bass is the same
/// string at every rate. As a sample count it bound on twice the keys at
/// 96 kHz (C2's eighth partial 3.3 cents flat there against none at 48);
/// as a time the section's |a| reaches 0.95 at 96 kHz and 0.97 at 192,
/// where the recurrence's rounding grows by 1/(1 - |a|), forty times
/// `f32`'s 6e-8. It binds only below about G1, where one section cannot
/// carry the bass's stretch and the eighth partial of A0 lands 4.4 cents
/// flat of the grand's (`the_harps_partials_sit_on_the_grands`).
const AP_TAU_MAX_SECS: f64 = 19.0 / 48_000.0;

/// Where the loop's all-pass is fitted, Hz: at the partial nearest this
/// frequency, between the third and the eighth — or at the second for a
/// string over 1 kHz. A bass string answers the middle of the keyboard
/// with its upper partials and a treble string with its first two or
/// three, and one section places the low partials of a stiff string to the
/// leading n² term of its law (`f_n ≈ nF(1 + B n²/2)`), so the partial it
/// is fitted at and every one below land within a cent and a half from C1
/// to C6 (`the_harps_partials_sit_on_the_grands`).
const MATCH_HZ: f64 = 600.0;

// ── the feed ──────────────────────────────────────────────────────────────

/// The feed's octave bands of fundamentals: band b holds the loops whose
/// fundamental is in `[BAND_LO 2^b, BAND_LO 2^(b+1))` (the last band holds
/// everything above), and its high-pass corner is `HP_RATIO` of the band's
/// lower edge, so a loop's own fundamental sits 2.5 to 5 times over its
/// corner. Two first-order sections: a short treble loop's DC resonance
/// answers a note far under it in proportion to `f0 / (2 pi f)`, and the
/// feed's `(f / fc)²` under the corner takes that to nothing; one section
/// left a C2 under the nineteen aliquots 10 dB over where the first
/// difference had it.
const BANDS: usize = 8;
const BAND_LO: f64 = 25.0;
const HP_RATIO: f64 = 0.4;

/// Where the feed rolls off, Hz: one first-order low-pass on the whole
/// drive, ahead of the bands — the bridge's coupling falling above the
/// board's mass-controlled region. Each loop's coupling is normalised at its
/// own fundamental, so the roll-off shapes only what a string answers above
/// it. Measured on the bench's stress score (sixty-four pedalled keys), the
/// harp against its voices from 4 to 8 kHz: at 2.5 kHz -24.1 dB on the
/// intimate grand, at 6 kHz -21.0, with no roll-off -19.3 (its reference's
/// bank: -13.7); kept for the electric grand's top octave, whose harp a
/// steal reads.
const ROLL_HZ: f64 = 6_000.0;

/// **Who hears which feed.** Every loop hears the drive high-passed in
/// its band (`x`, flat above its fundamental). A string being played — its
/// key held (the halo) or let go while its voice still sounds — hears it as
/// it is: its partials and its voice's coincide, and the reference's halo
/// is flat across them (C2's, partial by partial, -24..-40 dB). A string
/// nobody struck hears it shaped (`xs`), because what a sympathetic string
/// gives back depends on which of its partials the drive lands on:
///
/// - **Above the bass** (bands from 200 Hz up) it takes the bridge's motion
///   in proportion to its slope there, `n pi / L` for mode n — the first
///   answer's feed — but only to `TILT_SAT` times its band's lower edge, a
///   string's second or third partial, flat above; and its two high-pass
///   sections sit at `TILT_HP` times the edge, about the band's
///   fundamentals, so a string's fundamental is the partial it takes least
///   of. Each string's coupling is
///   normalised at its own fundamental, so the shape lifts its upper
///   partials over it; `GrandHarp::couple` sets the fundamental. Measured on
///   the bench's symp score (a silently held C4-E4-G4 answering a staccato
///   C2, re C2's fundamental, L/R power, the merged grand): fed flat, the held
///   C4's fundamental -37.7 dB and its second partial (C5) -43.5, the
///   reference's -57.7 and -35.9 — the strings answered as loudly at their
///   fundamentals as above them, where the reference's answer 15 to 20 dB
///   more strongly above; the slope saturated at three times the edge,
///   -37.7 and -39.5; with the corners at 1.4 times the edge and the middle's
///   coupling 2 dB lower, -41.4 and -39.3 (the brief's: C4 under -40, C5 -35
///   ± 5), G4's second -42.4 (the reference -43.3). Unsaturated, the slope
///   put C5 at -37.5 and a pedalled cluster's harp 26.5 dB OVER its voices
///   from 4 to 8 kHz on the concert grand: every free string's upper
///   partials answering every note's.
/// - **In the bass** (`BASS_BANDS`, fundamentals under 200 Hz) it takes its
///   first few partials flat and little above them, two poles at
///   `BASS_ROLL` times its band's edge — a choice, set by the pedal's wash:
///   a C2 held under the pedal lights the strings between its partials, and
///   with the bass strings fed flat to the top of their spectra their upper
///   partials filled that floor to -50.0 dB re C2's fundamental (the
///   reference's -56.7: a rise of +28 dB over the pedal-up floor where the
///   brief asks +17 ± 3); rolled off (and the middle's coupling as above),
///   -61.2 and +18.2.
const TILT_SAT: f64 = 3.0;
const TILT_HP: f64 = 1.4;
const BASS_BANDS: usize = 3;
const BASS_ROLL: f64 = 4.0;

/// The feed filters' coefficients at a sample rate: the roll-off's one
/// pole, each band's high-pass pole, and each band's shaping pole (the
/// tilt's saturation, or the bass's roll-off).
/// The tilted feed's high-pass poles at a sample rate (`TILT_HP`).
fn tilt_hp_coefs(rate: f64) -> [f64; BANDS] {
    std::array::from_fn(|b| (-std::f64::consts::TAU * TILT_HP * BAND_LO * 2.0_f64.powi(b as i32) / rate).exp())
}

fn feed_coefs(rate: f64) -> (f64, [f64; BANDS], [f64; BANDS]) {
    use std::f64::consts::TAU;
    let roll = 1.0 - (-TAU * ROLL_HZ / rate).exp();
    let edge = |b: usize| BAND_LO * 2.0_f64.powi(b as i32);
    let hp = std::array::from_fn(|b| (-TAU * HP_RATIO * edge(b) / rate).exp());
    let shape = std::array::from_fn(|b| {
        let at = if b < BASS_BANDS { BASS_ROLL } else { TILT_SAT };
        1.0 - (-TAU * at * edge(b) / rate).exp()
    });
    (roll, hp, shape)
}

/// The band a loop with this fundamental is fed from.
fn band_of(f0: f64) -> usize {
    ((f0 / BAND_LO).max(1.0).log2().floor() as usize).min(BANDS - 1)
}

/// What band `b`'s feeds pass at `f` Hz, magnitude: the flat one (the
/// roll-off and two high-pass sections) and the shaped one.
fn feed_gain(f: f64, b: usize, rate: f64) -> (f64, f64) {
    let (roll, hp, shape) = feed_coefs(rate);
    let w = std::f64::consts::TAU * f / rate;
    let (s, c) = w.sin_cos();
    let mag = |re: f64, im: f64| (re * re + im * im).sqrt();
    // A one-pole low-pass r / (1 - (1 - r) e^-jw).
    let lp = |r: f64| r / mag(1.0 - (1.0 - r) * c, (1.0 - r) * s);
    // A high-pass section a (1 - e^-jw) / (1 - a e^-jw).
    let hs = hp[b] * mag(1.0 - c, s) / mag(1.0 - hp[b] * c, hp[b] * s);
    let flat = lp(roll) * hs * hs;
    let shaped = if b < BASS_BANDS {
        flat * lp(shape[b]) * lp(shape[b])
    } else {
        // The first difference, |1 - e^-jw| = 2 sin(w / 2), then the pole.
        let a = tilt_hp_coefs(rate)[b];
        let h2 = a * mag(1.0 - c, s) / mag(1.0 - a * c, a * s);
        lp(roll) * h2 * h2 * 2.0 * (0.5 * w).sin() * lp(shape[b])
    };
    (flat, shaped)
}

/// The drive as the loops hear it, a part-frame at a time: rolled off,
/// high-passed per band (`x`), and shaped for the strings nobody struck
/// (`xs`). The filters run whenever there is drive, loop or no loop, so a
/// loop that wakes reads a filter that has been listening all along rather
/// than one starting from a stale state.
struct Feed {
    roll: f32,
    roll_z: f32,
    /// The rolled drive's last sample and its last difference, which the
    /// bands' first sections difference against.
    prev: f32,
    dprev: f32,
    hp: [f32; BANDS],
    /// The tilted feed's own high-pass poles (`TILT_HP`).
    hps: [f32; BANDS],
    shape: [f32; BANDS],
    /// Each band's sections: the flat feed's two, the slope's two, the
    /// shaping poles.
    y1: [f32; BANDS],
    y2: [f32; BANDS],
    q1: [f32; BANDS],
    q2: [f32; BANDS],
    s1: [f32; BANDS],
    s2: [f32; BANDS],
    /// This part-frame's drive, per band, flat and shaped.
    x: [[f32; FRAME]; BANDS],
    xs: [[f32; FRAME]; BANDS],
    /// Every state is zero and nothing came in: the filters can rest.
    quiet: bool,
}

impl Feed {
    fn new(rate: f32) -> Feed {
        let (roll, hp, shape) = feed_coefs(f64::from(rate));
        Feed {
            roll: roll as f32,
            roll_z: 0.0,
            prev: 0.0,
            dprev: 0.0,
            hp: hp.map(|a| a as f32),
            hps: tilt_hp_coefs(f64::from(rate)).map(|a| a as f32),
            shape: shape.map(|a| a as f32),
            y1: [0.0; BANDS],
            y2: [0.0; BANDS],
            q1: [0.0; BANDS],
            q2: [0.0; BANDS],
            s1: [0.0; BANDS],
            s2: [0.0; BANDS],
            x: [[0.0; FRAME]; BANDS],
            xs: [[0.0; FRAME]; BANDS],
            quiet: true,
        }
    }

    /// Every state to rest. The part-frame buffers are not touched: the
    /// next `run` writes what it reads.
    fn clear(&mut self) {
        (self.roll_z, self.prev, self.dprev) = (0.0, 0.0, 0.0);
        self.y1 = [0.0; BANDS];
        self.y2 = [0.0; BANDS];
        self.q1 = [0.0; BANDS];
        self.q2 = [0.0; BANDS];
        self.s1 = [0.0; BANDS];
        self.s2 = [0.0; BANDS];
        self.quiet = true;
    }

    /// Rest once everything has rung down (flushing what would go denormal
    /// on the way). Asked once a frame, from the gate, so a harp rendered a
    /// block at a time and one ticked a sample at a time rest at the same
    /// sample (`a_block_is_its_samples`).
    fn rest_if_quiet(&mut self) {
        let tiny = |v: f32| v.abs() < 1.0e-20;
        let all = |a: &[f32; BANDS]| a.iter().all(|&v| tiny(v));
        if !self.quiet
            && tiny(self.roll_z)
            && tiny(self.prev)
            && tiny(self.dprev)
            && all(&self.y1)
            && all(&self.y2)
            && all(&self.q1)
            && all(&self.q2)
            && all(&self.s1)
            && all(&self.s2)
        {
            self.clear();
        }
    }

    /// Filter `u` (the sent drive, at most a frame) into every band.
    fn run(&mut self, u: &[f32]) {
        let m = u.len();
        if self.quiet && u.iter().all(|&v| v == 0.0) {
            for x in self.x.iter_mut().chain(self.xs.iter_mut()) {
                x[..m].fill(0.0);
            }
            return;
        }
        self.quiet = false;
        let (roll, hp, hps, shape) = (self.roll, self.hp, self.hps, self.shape);
        let (mut z, mut prev, mut dprev) = (self.roll_z, self.prev, self.dprev);
        let (mut y1, mut y2, mut q1, mut q2, mut s1, mut s2) = (self.y1, self.y2, self.q1, self.q2, self.s1, self.s2);
        for (k, &v) in u.iter().enumerate() {
            z += roll * (v - z);
            let du = z - prev;
            prev = z;
            let ddu = du - dprev;
            dprev = du;
            for b in 0..BANDS {
                // Flat: two high-pass sections on the rolled drive.
                let t = hp[b] * (y1[b] + du);
                let d1 = t - y1[b];
                y1[b] = t;
                y2[b] = hp[b] * (y2[b] + d1);
                self.x[b][k] = y2[b];
                if b < BASS_BANDS {
                    // The bass: the flat feed through two poles.
                    s1[b] += shape[b] * (y2[b] - s1[b]);
                    s2[b] += shape[b] * (s1[b] - s2[b]);
                    self.xs[b][k] = s2[b];
                } else {
                    // The slope: the same sections on its first
                    // difference, saturated by one pole.
                    let t = hps[b] * (q1[b] + ddu);
                    let e1 = t - q1[b];
                    q1[b] = t;
                    q2[b] = hps[b] * (q2[b] + e1);
                    s1[b] += shape[b] * (q2[b] - s1[b]);
                    self.xs[b][k] = s1[b];
                }
            }
        }
        (self.roll_z, self.prev, self.dprev) = (z, prev, dprev);
        (self.y1, self.y2, self.q1, self.q2, self.s1, self.s2) = (y1, y2, q1, q2, s1, s2);
    }
}

// ── the strings of an instrument ──────────────────────────────────────────

/// How far every loop sits sharp of the string it shadows, cents. The
/// struck string's own loop answers its voice as a forced, detuned
/// resonator (see the module doc); a sympathetic string answering another
/// key's partial is a fraction of a cent from it whatever the detune,
/// because a staccato or decaying drive is a few hertz wide.
const DETUNE_CENTS: f32 = 1.5;

/// A string whose key has come up while its voice still sounds is coupled
/// at this fraction of its free coupling. Under the pedal it rings free with
/// its own voice — an exact resonance, detuned `DETUNE_CENTS` — so at its
/// full coupling it built toward 15 dB under the note and a pedalled passage's
/// harp stood 10 to 13 dB under its voices from 62 Hz to 1 kHz on the intimate
/// grand (its reference's bank: 12 to 22 under); at 0.6, 18 to 22 under from
/// 250 Hz to 2 kHz on the merged grand.
const AFTER: f32 = 0.6;

/// The T60 a string runs while its own key's voice sounds, seconds: what
/// sets how fast its ring settles onto the note. The reference's reaches
/// its steady ratio 50 to 100 ms after the strike (C4 -41, -35, -32 dB over
/// the first 5-20, 20-50 and 50-100 ms, then -32 to -33 to the key-up; C2
/// the same), which is a resonator whose decay outruns the note's by about
/// 20 nepers a second. See `Loop::settle`.
const HALO_T60: f32 = 0.3;

/// Where the second point of the free loss is set, Hz (and never above
/// 0.4 fs): the reference's early law falls 0.039 decades an octave below
/// 2 kHz and 0.191 above it (`hf_ratio`).
const F_HI: f32 = 8_000.0;

/// The free T60 at `f_hi` over the free T60 at `f0`, from the reference's
/// fitted early law (`log10 T60 = ... - 0.039 log2(f/1k) - 0.152 max(0,
/// log2(f/2k))`, intimate reference, 429 partials).
fn hf_ratio(f0: f32, f_hi: f32) -> f32 {
    let knee = |f: f32| (f / 2000.0).log2().max(0.0);
    10.0_f32.powf(-0.039 * (f_hi / f0).log2() - 0.152 * (knee(f_hi) - knee(f0)))
}

/// Log-linear interpolation of an anchor table, holding past the ends.
fn curve(anchors: &[(f32, f32)], key: f32) -> f32 {
    let (first, last) = (anchors[0], anchors[anchors.len() - 1]);
    if key <= first.0 {
        return first.1;
    }
    if key >= last.0 {
        return last.1;
    }
    for w in anchors.windows(2) {
        let ((k0, v0), (k1, v1)) = (w[0], w[1]);
        if key <= k1 {
            let t = (key - k0) / (k1 - k0);
            return (v0.ln() * (1.0 - t) + v1.ln() * t).exp();
        }
    }
    last.1
}

fn f_equal(key: u8) -> f32 {
    440.0 * 2.0_f32.powf((f32::from(key) - 69.0) / 12.0)
}

/// How a modal grand's harp is voiced: the couplings of its free strings
/// and of the struck string's own, how its strings ring free and damped,
/// and whether it carries aliquots. Two instruments, two voicings, each
/// set against its own reference.
#[derive(Clone, Copy, Debug)]
pub struct GrandHarp {
    /// The bridge coupling of a free string nobody is playing, by key, at
    /// its fundamental (the shaped feed lifts what is above it): the fraction
    /// of the bridge sum it takes in, and the same fraction of what it
    /// carries that it gives back — the same admittance both ways, so the
    /// return goes as the square. A string let go while its voice sounds
    /// is coupled at `AFTER` of this.
    couple: &'static [(f32, f32)],
    /// The coupling of a string whose own key is held, by key: `own` for a
    /// string with a damper, `own_free` for one above the last damper (the
    /// treble's halo is wider). Both run the short `HALO_T60` loss while
    /// held (`Loop::settle`).
    own: &'static [(f32, f32)],
    own_free: &'static [(f32, f32)],
    /// A free string's T60 at its fundamental, seconds, by key.
    t60_free: &'static [(f32, f32)],
    /// The damped T60 at a string's fundamental: `[0]` seconds at 65.4 Hz,
    /// rising as `(f0 / 65.4)^[1]`, held between `[2]` and `[3]`.
    damped: [f32; 4],
    /// An aliquot over every course from F#6.
    aliquots: bool,
}

impl GrandHarp {
    /// The intimate grand, against its reference (sympathetic resonance
    /// 0.40, aliquots). Every figure below is the bench's (tbench renders
    /// through the frame, with the harp and without, L and R power-averaged)
    /// on the grand merged from the grand lane, base grand in brackets.
    ///
    /// **The free strings' coupling** (`couple`) is flat through the bass —
    /// the bass strings' answers are carried by their first partials
    /// (`BASS_BANDS`), and the reference's bank under a pedalled cluster
    /// is as loud as the voices below 250 Hz — and falls toward the treble
    /// from the middle. In the middle it is set by the silent keys and the
    /// wash together (symp score): the held C4's fundamental answering a
    /// staccato C2 at -41.4 dB re C2's fundamental (-41.2), its second
    /// partial -39.3 (-40.0), G4's second -42.4 (-44.6), against the brief's
    /// under -40 and -35 ± 5 and the reference's -43.3 at G5; the pedal's
    /// wash between C2's partials -61.2 dB re it, a rise of +18.2 (+19.2)
    /// over the pedal-up floor (the brief's +17 ± 3). **The own coupling**
    /// puts the struck string's halo where the reference's sits re the note
    /// (exact subtraction of a render with its bank off, the release score):
    /// C2 -27.7, C4 -33.3 to -33.8, C5 -35.0; here -30.2, -33.8, -34.4, flat
    /// across C2's partials (-30 dB from n1 to n12); and the undamped
    /// treble's: G6 and C7 with their aliquots -22.5 and -22.1 (the brief's
    /// -12..-22). **The damped law** is the reference's post-release
    /// residual, organised by absolute frequency: 1.2 s at 65 Hz rising as
    /// `(f / 65 Hz)^0.4` to 2.1 s at 262 Hz and held at 3 s from about 1 kHz
    /// (C2 n1 -51.5 dB/s, C4 n1 -42, 1-2 kHz -19 to -25); the release
    /// table's slopes 0.3-1.5 s here C4 -25.7..-27.4, C2 -48.2..-48.8, C5
    /// -21.6 (the reference's -27.8..-31.4, -43.7..-54.4, -20.9).
    pub const INTIMATE: GrandHarp = GrandHarp {
        couple: &[(21.0, 0.0378), (47.0, 0.0378), (55.0, 0.032), (62.0, 0.030), (74.0, 0.0227), (84.0, 0.014), (96.0, 0.0078), (108.0, 0.0049)],
        own: &[(21.0, 0.16), (36.0, 0.1475), (60.0, 0.0627), (72.0, 0.0434), (84.0, 0.036), (96.0, 0.032), (108.0, 0.032)],
        own_free: &[(84.0, 0.066), (91.0, 0.065), (96.0, 0.0615), (108.0, 0.0615)],
        t60_free: &[(21.0, 10.0), (36.0, 8.0), (48.0, 6.0), (60.0, 5.0), (72.0, 3.5), (84.0, 2.2), (96.0, 1.6), (108.0, 1.2)],
        damped: [1.2, 0.4, 0.8, 3.0],
        aliquots: true,
    };

    /// The concert grand, against its reference: its bank answers wider than
    /// the intimate's while a key is down — the halo 8 to 11 dB over it (C2
    /// -19.7, C4 -23.3 to -25.5, C5 -23.9, C7 -19.8 dB re the note, exact
    /// subtraction; here -21.7, -24.0..-26.2, -23.9, -19.7) — and leaves far
    /// less after the key comes up: C4's fundamental 68 to 76 dB under its
    /// key-off level at +0.5 s, its release table 0.3-1.5 s -73 to -97 dB/s,
    /// against the intimate's -30. So its damped strings are short: the
    /// release table here -71.7..-89.9 (C4), -88.9..-92.8 (C2), -87.7
    /// (C5), the reference's -73..-97, -86..-108, -87. Its wash -53.0 dB re
    /// C2's fundamental, rise +30.6 (the reference's -47.5 and +35.6).
    pub const CONCERT: GrandHarp = GrandHarp {
        couple: &[(21.0, 0.0378), (47.0, 0.0378), (55.0, 0.032), (62.0, 0.030), (74.0, 0.0227), (84.0, 0.014), (96.0, 0.0078), (108.0, 0.0049)],
        own: &[(21.0, 0.165), (36.0, 0.153), (60.0, 0.0718), (72.0, 0.0524), (84.0, 0.03), (96.0, 0.022), (108.0, 0.022)],
        own_free: &[(84.0, 0.042), (91.0, 0.038), (96.0, 0.035), (108.0, 0.035)],
        t60_free: &[(21.0, 10.0), (36.0, 8.0), (48.0, 6.0), (60.0, 5.0), (72.0, 3.5), (84.0, 2.2), (96.0, 1.6), (108.0, 1.2)],
        damped: [0.8, 0.0, 0.5, 1.2],
        aliquots: false,
    };

    /// The damped T60 at a fundamental `f0`, seconds.
    fn damped_t60(&self, f0: f32) -> f32 {
        let [t, e, lo, hi] = self.damped;
        (t * (f0 / 65.4).powf(e)).clamp(lo, hi)
    }
}

/// The aliquot strings' coupling, how far sharp of their course they sit,
/// cents, and how much longer than the course they ring: an unstruck
/// string carried on its own bridge, and the reference's C7 aliquot line
/// decays at 12.9 dB/s against its note's 18. **Eight cents sharp, not
/// three**, because an aliquot is a high-Q string (a hertz wide at G6) and a
/// few cents decide how hard it answers: at +3 the merged grand's C4, whose
/// sixth partial its stretch puts 1.7 cents from the G6 aliquot, carried an
/// aliquot ring of -31.6 dB re the note that grew to -24.3 over a two-second
/// hold (the reference's aliquots in the middle: -38 to -41), and at -3 the
/// treble's own aliquots read -11 to -17 against -26; at +8 the answer is a
/// forced response a few cents either way does not move (C4 -44.7, G6
/// -28.5), and the coupling is set for G6 and C7 (here -24.8 and -26.9 re
/// the note; the reference's -25.6 and -26.5).
const ALIQUOT_COUPLE: f32 = 0.048;
const ALIQUOT_CENTS: f32 = 8.0;
const ALIQUOT_T60: f32 = 1.4;

/// One string of the harp, as the instrument it belongs to strings it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HarpString {
    /// The fundamental, Hz, as the harp tunes it (stretch and detune in).
    pub f0: f32,
    /// Inharmonicity: partial n at `n F sqrt(1 + B n²)`, `F = f0 / sqrt(1 + B)`.
    pub b: f32,
    /// Free decay (no felt on it): T60 at the fundamental and at `f_hi`, s.
    pub t60: f32,
    pub t60_hi: f32,
    pub f_hi: f32,
    /// Damped decay (felt down): T60 at the fundamental, s, the same
    /// extra loss on every partial.
    pub t60_damped: f32,
    /// The bridge coupling while free and unplayed, and while its own key
    /// sounds. Zero is a string the harp does not have.
    pub couple: f32,
    pub own: f32,
    /// Whether a damper sits on it.
    pub damper: bool,
}

impl HarpString {
    const NONE: HarpString = HarpString {
        f0: 440.0,
        b: 0.0,
        t60: 1.0,
        t60_hi: 1.0,
        f_hi: 4_000.0,
        t60_damped: 0.5,
        couple: 0.0,
        own: 0.0,
        damper: true,
    };

    /// The string as the harp will design it: a string whose pitch or
    /// decay is not a number (or not positive) is one the harp does not
    /// have, and every other field that is not a number takes a value that
    /// is. `design` clamps with these as bounds, and `f64::clamp` panics on
    /// a NaN bound — on the audio thread, through `set_patch` (verifier
    /// finding 7: a NaN `f0` panicked `Harp::string`).
    fn sane(&self) -> HarpString {
        let ok = |v: f32| v.is_finite() && v > 0.0;
        if !ok(self.f0) || !ok(self.t60) {
            return HarpString::NONE;
        }
        let pick = |v: f32, or: f32| if ok(v) { v } else { or };
        let gain = |v: f32| if v.is_finite() { v.max(0.0) } else { 0.0 };
        HarpString {
            f0: self.f0,
            b: if self.b.is_finite() { self.b.max(0.0) } else { 0.0 },
            t60: self.t60,
            t60_hi: pick(self.t60_hi, self.t60),
            f_hi: pick(self.f_hi, F_HI),
            t60_damped: pick(self.t60_damped, self.t60),
            couple: gain(self.couple),
            own: gain(self.own),
            damper: self.damper,
        }
    }
}

/// Everything an instrument tells the harp: its strings, its aliquots, and
/// how loudly the whole bank answers (the reference's "sympathetic
/// resonance" amount: 0.40 on the intimate grand, 1.00 on the concert
/// grands).
#[derive(Clone, Copy, Debug)]
pub struct StringSet {
    pub keys: [HarpString; KEYS],
    pub aliquots: [HarpString; ALIQUOTS],
    /// A gain on what the bridge drives the harp with.
    pub send: f32,
}

impl StringSet {
    /// The modal grand's strings: stretched as its scale stretches them,
    /// on its own B, `DETUNE_CENTS` sharp, dampers up to its last damper,
    /// voiced by `voice`, and — for a grand strung with them — an aliquot
    /// over every course from F#6.
    pub fn grand(base: &GrandBase, send: f32, voice: &GrandHarp) -> StringSet {
        let tune = grand::tuning(base);
        let keys = std::array::from_fn(|i| grand_string(base, voice, &tune, i, 1.0));
        let aliquots = std::array::from_fn(|j| grand_string(base, voice, &tune, KEYS + j, 1.0));
        StringSet { keys, aliquots, send }
    }

    /// The waveguide piano's strings, as the harp was first strung: equal
    /// temperament, the waveguide's B schedule and loss law, no halo (its
    /// voice already carries the struck string), a damper on every key.
    /// What `Harp::set_base` strings.
    pub fn piano(base: &PianoBase) -> StringSet {
        let voice = GrandHarp::INTIMATE;
        let keys = std::array::from_fn(|i| {
            let key = FIRST_KEY + i as u8;
            let k = f32::from(key);
            let f0 = f_equal(key);
            let b = if k < 48.0 {
                2.0e-4 * 2.0_f32.powf((48.0 - k) / 9.0)
            } else {
                2.0e-4 * 2.0_f32.powf((k - 48.0) / 22.0)
            };
            let t60 = ((18.0 * (-(k - 21.0) / 30.0).exp()).clamp(1.0, 18.0) * base.t60).clamp(0.12, 60.0);
            HarpString {
                f0,
                b: (b * base.stiff).clamp(1.0e-6, 8.0e-3),
                t60,
                t60_hi: (t60 * base.brill.clamp(0.05, 0.9)).max(0.05).min(t60),
                f_hi: 6.0 * f0,
                t60_damped: voice.damped_t60(f0),
                couple: curve(voice.couple, k),
                own: 0.0,
                damper: true,
            }
        });
        StringSet { keys, aliquots: [HarpString::NONE; ALIQUOTS], send: 1.0 }
    }
}

/// **Loop `i`'s string on a modal grand at STRETCH `stretch`** (the knob's
/// multiplier): under `KEYS`, key `i`'s from A0, tuned where its voice is
/// (`grand::GrandVoice::build`: its stretched tuning `tune.0[i]` cents times
/// the stretch and its B `tune.1[i]` times it, `grand::tuning`)
/// `DETUNE_CENTS` sharp and voiced by `voice`; above, the aliquot over key
/// `ALIQUOT_FIRST + i - KEYS`, for a grand strung with them. At 1 the
/// string `StringSet::grand` strings, to the bit; what `Strung::follow`
/// re-strings a string with as the knob moves.
pub fn grand_string(base: &GrandBase, voice: &GrandHarp, tune: &([f32; KEYS], [f32; KEYS]), i: usize, stretch: f32) -> HarpString {
    if i >= KEYS {
        if !voice.aliquots || i >= LOOPS {
            return HarpString::NONE;
        }
        let key = ALIQUOT_FIRST + (i - KEYS) as u8;
        let s = grand_string(base, voice, tune, usize::from(key - FIRST_KEY), stretch);
        return HarpString {
            f0: s.f0 * 2.0_f32.powf(ALIQUOT_CENTS / 1200.0),
            t60: s.t60 * ALIQUOT_T60,
            t60_hi: s.t60_hi * ALIQUOT_T60,
            couple: ALIQUOT_COUPLE,
            own: ALIQUOT_COUPLE,
            damper: false,
            ..s
        };
    }
    let key = FIRST_KEY + i as u8;
    let k = f32::from(key);
    let f0 = f_equal(key) * 2.0_f32.powf((tune.0[i] * stretch + DETUNE_CENTS) / 1200.0);
    let t60 = curve(voice.t60_free, k);
    HarpString {
        f0,
        b: tune.1[i] * stretch,
        t60,
        t60_hi: t60 * hf_ratio(f0, F_HI),
        f_hi: F_HI,
        t60_damped: voice.damped_t60(f0),
        couple: curve(voice.couple, k),
        own: if key <= base.last_damper { curve(voice.own, k) } else { curve(voice.own_free, k) },
        damper: key <= base.last_damper,
    }
}

// ── the loop's design ─────────────────────────────────────────────────────

/// A first-order all-pass `(a + z^-1) / (1 + a z^-1)`'s phase delay at `w`,
/// samples. It is also the Thiran fraction's, which is the same section.
fn ap_delay(w: f64, a: f64) -> f64 {
    let (sw, cw) = w.sin_cos();
    let ph = (-sw).atan2(a + cw) - (-a * sw).atan2(1.0 + a * cw);
    -ph / w.max(1e-9)
}

/// The loss one-pole `g (1 - p) / (1 - p z^-1)`'s phase delay at `w`, samples.
fn lp_delay(w: f64, p: f64) -> f64 {
    let (sw, cw) = w.sin_cos();
    (p * sw).atan2(1.0 - p * cw) / w.max(1e-9)
}

/// The all-pass coefficient whose DC delay is `t` samples.
fn a_of(t: f64) -> f64 {
    (1.0 - t) / (1.0 + t)
}

/// The largest loop gain the loss may be given: below one, because the DC
/// gain of a one-pole with a gain in front IS that gain, and a loop whose
/// DC gain reaches one holds whatever offset it is handed for ever.
const G_MAX: f64 = 1.0 - 1.0e-6;

/// Solve the loop's loss so the per-pass magnitude is `rho1` at `w1` and
/// `rho2` at `w2`: the two-point design of `string::design_loss`, in f64
/// and without its pole floor (0.02) and gain ceiling (0.9999), which were
/// set for a loop whose linear read hid them behind its own loss. A flat or
/// rising request is a flat loss. The pole is held where the gain it asks
/// for stays under `G_MAX`: a very short loop asked for a steep top would
/// otherwise get a DC gain over one.
fn loss_design(w1: f64, w2: f64, rho1: f64, rho2: f64) -> (f64, f64) {
    let (c1, c2) = (w1.cos(), w2.cos());
    let r = (rho1 / rho2) * (rho1 / rho2);
    let mut p = 0.0;
    if r > 1.0 + 1.0e-12 {
        let q = (c2 - r * c1) / (1.0 - r);
        if q > 1.0 {
            p = q - (q * q - 1.0).sqrt();
        }
    }
    let p = p.clamp(0.0, 0.95);
    let s2 = (0.5 * w1).sin().powi(2);
    let gain = |p: f64| rho1 * ((1.0 - p) * (1.0 - p) + 4.0 * p * s2).sqrt() / (1.0 - p);
    if gain(p) <= G_MAX {
        return (p, gain(p));
    }
    // g(p) = G_MAX: k (1 - p)² = 4 s² p with k = (G_MAX / rho1)² - 1, the
    // smaller root.
    let k = (G_MAX / rho1) * (G_MAX / rho1) - 1.0;
    let bq = 2.0 * k + 4.0 * s2;
    let p = ((bq - (bq * bq - 4.0 * k * k).max(0.0).sqrt()) / (2.0 * k)).clamp(0.0, p);
    (p, gain(p).min(G_MAX))
}

/// What a loop's coefficients are, worked out from its string.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Design {
    back: u32,
    frac: f32,
    loss: f32,
    disp: f32,
    gain_free: f32,
    gain_damped: f32,
    gain_halo: f32,
    gain_land: f32,
}

/// Design one loop: its (sane) string, at `rate`, in a line of `n` samples.
///
/// The loop's delay at frequency w is `back + thiran(w) + loss(w) +
/// allpass(w)`. The fundamental needs `fs / f0` exactly; the match partial
/// m needs `m fs / f_m`, less by what the stiffness shortens it. The loss
/// pole is fixed by the decay, so the line, the Thiran and the all-pass
/// are solved together for both (see below). Everything in f64: the
/// shortening of a C2 string at its eighth partial is two samples in seven
/// hundred.
fn design(s: &HarpString, rate: f32, n: u32) -> Design {
    use std::f64::consts::{PI, TAU};
    let fs = f64::from(rate);
    let f0 = f64::from(s.f0).clamp(10.0, 0.25 * fs);
    let total = fs / f0;
    let w1 = TAU * f0 / fs;

    let t60 = f64::from(s.t60).clamp(0.05, 120.0);
    let t60_hi = f64::from(s.t60_hi).clamp(0.02, t60);
    let f_hi = f64::from(s.f_hi).clamp(1.5 * f0, 0.40 * fs);
    let rho1 = 10.0_f64.powf(-3.0 / (t60 * f0));
    let rho2 = 10.0_f64.powf(-3.0 / (t60_hi * f0));
    let (p, g) = loss_design(w1, TAU * f_hi / fs, rho1, rho2);
    // The felt: the same extra loss per pass on every partial, so the
    // fundamental's T60 becomes the damped one; its bite on landing, and
    // the halo's loss, the same way.
    let extra = |t: f64| 10.0_f64.powf(-3.0 / f0 * (1.0 / t.min(t60) - 1.0 / t60));
    let td = f64::from(s.t60_damped).clamp(0.05, t60);
    let felt = extra(td);
    let halo = extra(f64::from(HALO_T60).min(td));
    let land = extra(f64::from(LAND_T60).min(td));

    let b = f64::from(s.b).clamp(0.0, 0.05);
    let big_f = f0 / (1.0 + b).sqrt();
    let partial = |m: f64| m * big_f * (1.0 + b * m * m).sqrt();
    let mut m = if f0 > 1000.0 { 2.0 } else { (MATCH_HZ / f0).round().clamp(3.0, 8.0) };
    while m > 2.0 && TAU * partial(m) / fs > 0.8 * PI {
        m -= 1.0;
    }
    let wm = (TAU * partial(m) / fs).min(0.8 * PI);
    let need = total - m * fs / partial(m);
    let tau_cap = (AP_TAU_MAX_SECS * fs).min(0.4 * total).max(1.0);
    let max_back = f64::from(n.saturating_sub(3)).max(1.0);

    // The line's whole samples, the Thiran's DC delay `d` and the
    // all-pass's `t`: the fundamental needs `total` samples of loop and the
    // match partial `m fs / f_m`. With the line fixed those are two
    // equations in (d, t), solved together by Newton:
    //   back + thiran(w1, d) + loss(w1) + allpass(w1, t) = total
    //   back + thiran(wm, d) + loss(wm) + allpass(wm, t) = m fs / f_m
    // **Together, because apart they fight.** The first design solved the
    // all-pass for the stretch, then the Thiran for the pitch, then again:
    // at a ten-sample loop (C8 at 44.1 kHz) more Thiran delay means more
    // Thiran dispersion, which asks the all-pass for less, which leaves the
    // Thiran more delay — the passes ran both to their bounds and left C8
    // 13 cents sharp, and choosing the line again each pass flipped it
    // between two lengths and left C7's second partial 3.6 cents flat.
    // Each line length near the first guess is tried, and the solution
    // whose Thiran is nearest one sample (least dispersion of its own) kept;
    // a line for which no solution stays in bounds keeps the pitch exact and
    // lets the stretch go.
    let r1 = |back: f64| total - back - lp_delay(w1, p);
    let r2 = |back: f64| m * fs / partial(m) - back - lp_delay(wm, p);
    let solve = |back: f64, t_start: f64| -> Option<(f64, f64)> {
        let resid = |d: f64, t: f64| {
            (
                ap_delay(w1, a_of(d)) + ap_delay(w1, a_of(t)) - r1(back),
                ap_delay(wm, a_of(d)) + ap_delay(wm, a_of(t)) - r2(back),
            )
        };
        let (mut d, mut t) = (1.0_f64, t_start);
        for _ in 0..24 {
            let (f1, f2) = resid(d, t);
            if f1.abs() < 1.0e-9 && f2.abs() < 1.0e-9 {
                return Some((d, t));
            }
            let h = 1.0e-6;
            let j11 = (ap_delay(w1, a_of(d + h)) - ap_delay(w1, a_of(d - h))) / (2.0 * h);
            let j21 = (ap_delay(wm, a_of(d + h)) - ap_delay(wm, a_of(d - h))) / (2.0 * h);
            let j12 = (ap_delay(w1, a_of(t + h)) - ap_delay(w1, a_of(t - h))) / (2.0 * h);
            let j22 = (ap_delay(wm, a_of(t + h)) - ap_delay(wm, a_of(t - h))) / (2.0 * h);
            let det = j11 * j22 - j12 * j21;
            if !det.is_finite() || det.abs() < 1.0e-12 {
                return None;
            }
            // A damped step, held inside the bounds: a full Newton step
            // from one sample can leave them at a ten-sample loop.
            let (sd, st) = ((f1 * j22 - f2 * j12) / det, (j11 * f2 - j21 * f1) / det);
            let k = (0.5 / sd.abs().max(st.abs()).max(1.0e-30)).min(1.0);
            d = (d - k * sd).clamp(0.05, 4.0);
            t = (t - k * st).clamp(0.25, tau_cap);
        }
        let (f1, f2) = resid(d, t);
        (f1.abs() < 1.0e-6 && f2.abs() < 1.0e-6).then_some((d, t))
    };
    // The guess: the all-pass alone fitted to the stretch, the Thiran one
    // sample.
    let spread = |t: f64| ap_delay(w1, a_of(t)) - ap_delay(wm, a_of(t));
    let need = need - (lp_delay(w1, p) - lp_delay(wm, p));
    let (mut lo, mut hi) = (0.25_f64, tau_cap);
    for _ in 0..30 {
        let mid = 0.5 * (lo + hi);
        if spread(mid) < need {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    let t0 = 0.5 * (lo + hi);
    let guess = (total - lp_delay(w1, p) - ap_delay(w1, a_of(t0)) - 1.0).round();
    let mut best: Option<(f64, f64, f64)> = None;
    for back in [guess, guess - 1.0, guess + 1.0, guess - 2.0, guess + 2.0] {
        if back < 1.0 || back > max_back {
            continue;
        }
        if let Some((d, t)) = solve(back, t0) {
            if best.is_none_or(|(_, bd, _)| (d - 1.0).abs() < (bd - 1.0).abs()) {
                best = Some((back, d, t));
            }
            // Within half a sample of one is as good as it gets, and the
            // first line tried usually is.
            if (d - 1.0).abs() <= 0.5 {
                break;
            }
        }
    }
    let (back, d, t) = best.unwrap_or_else(|| {
        // No line puts both in bounds: the all-pass at its fit, the line
        // and the Thiran for the pitch alone.
        let back = guess.clamp(1.0, max_back);
        let want = r1(back) - ap_delay(w1, a_of(t0));
        let (mut lo, mut hi) = (0.05_f64, 4.0_f64);
        for _ in 0..40 {
            let mid = 0.5 * (lo + hi);
            if ap_delay(w1, a_of(mid)) < want {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        (back, 0.5 * (lo + hi), t0)
    });
    let (a_th, a_d) = (a_of(d), a_of(t));
    Design {
        back: back as u32,
        frac: a_th as f32,
        loss: (1.0 - p) as f32,
        disp: a_d as f32,
        gain_free: g as f32,
        gain_damped: (g * felt) as f32,
        gain_halo: (g * halo) as f32,
        gain_land: (g * land) as f32,
    }
}

/// The key a loop shadows: its own for the 88, its course's for an aliquot.
fn key_of(i: usize) -> u8 {
    if i < KEYS {
        FIRST_KEY + i as u8
    } else {
        ALIQUOT_FIRST + (i - KEYS) as u8
    }
}

/// How long loop `i`'s line is at `rate`: its own key's pitch a whole tone
/// flat — room for any stretch and detune a string set asks for — plus the
/// read behind it.
fn line_len(i: usize, rate: f32) -> u32 {
    (rate / (f_equal(key_of(i)) * 2.0_f32.powf(-200.0 / 1200.0))).ceil() as u32 + 6
}

/// One loop's coefficients, designed.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Coef {
    d: Design,
    couple: f32,
    own: f32,
    after: f32,
    /// What its band's feed passes at its fundamental, inverted, so its
    /// fundamental takes exactly its coupling.
    norm: f32,
    /// The same at the slope's feed.
    norm_s: f32,
    band: u8,
    damper: bool,
    probation_frames: u8,
    /// How many frames under the floor before the gate may sleep it: more
    /// than one of its own periods.
    quiet_frames: u8,
}

impl Coef {
    /// A (sane) string's coefficients at `rate`, designed as `d` and fed
    /// from feed band `band`.
    fn with(s: &HarpString, rate: f32, d: Design, band: usize) -> Coef {
        let fs = f64::from(rate);
        let f0 = f64::from(s.f0).clamp(10.0, 0.25 * fs);
        let (flat, shaped) = feed_gain(f0, band, fs);
        let periods = PROBATION_PERIODS / s.f0;
        Coef {
            d,
            couple: s.couple,
            own: s.own,
            after: s.couple * AFTER,
            norm: (1.0 / flat.max(1.0e-9)) as f32,
            norm_s: (1.0 / shaped.max(1.0e-9)) as f32,
            band: band as u8,
            damper: s.damper,
            probation_frames: ((periods.max(PROBATION_MIN_SECS) * rate / FRAME as f32).ceil() as u32).clamp(1, 255) as u8,
            quiet_frames: ((fs / f0 / FRAME as f64).ceil() as u32 + 1).clamp(2, 255) as u8,
        }
    }

    /// Loop `i`'s coefficients for string `s` at `rate`: on the line
    /// `design` chooses, fed from the band of its fundamental.
    fn of(s: &HarpString, rate: f32, i: usize) -> Coef {
        let s = s.sane();
        let band = band_of(f64::from(s.f0).clamp(10.0, 0.25 * f64::from(rate)));
        Coef::with(&s, rate, design(&s, rate, line_len(i, rate)), band)
    }
}

/// A string set designed into the harp's coefficients for one sample rate:
/// everything `Harp::load` copies. Designing is the dear part (a Newton
/// solve per string, about half a millisecond for the set), so an
/// instrument builds its sets once, where it may allocate and take its
/// time (`Keys::new`), and a patch change copies one (verifier finding 8:
/// stringing at `set_patch` cost up to 656 us of a 64-frame callback's
/// 1451). Allocation-free itself.
///
/// **And it follows the knob that tunes the strings it shadows**
/// (`follow`). A set strung at STRETCH centre missed its voices at either
/// end of the knob: their fundamentals by up to 84 cents (C8, the knob at 1)
/// and 30 (C8, at 0), where the design sits 1.5 sharp; their fourth
/// partials by up to 225 (C7); and it moved the undamped treble's own ring
/// by 4 to 43 dB, the intimate's aliquots' by 2 to 8 and a pedalled C6's
/// ring-off by 6 to 16 (the `stretch_and_the_harp` table). So each set knows
/// what each string is strung for, and is re-strung a string at a time as
/// the knob moves.
#[derive(Clone, Copy, Debug)]
pub struct Strung {
    coef: [Coef; LOOPS],
    send: f32,
    rate: f32,
    /// The stretch each string is designed for (`f32` bits), when every
    /// one is, and where the next re-stringing looks first. `new` takes the
    /// set it is given as strung at STRETCH centre, which is how the frame
    /// builds its sets.
    strung: [u32; LOOPS],
    whole: Option<u32>,
    cursor: usize,
}

impl Strung {
    /// Design `set` at `rate`.
    pub fn new(set: &StringSet, rate: f32) -> Strung {
        let rate = if rate.is_finite() && rate > 0.0 { rate } else { 48_000.0 };
        let coef = std::array::from_fn(|i| Coef::of(if i < KEYS { &set.keys[i] } else { &set.aliquots[i - KEYS] }, rate, i));
        let send = if set.send.is_finite() { set.send.max(0.0) } else { 0.0 };
        let centre = 1.0_f32.to_bits();
        Strung { coef, send, rate, strung: [centre; LOOPS], whole: Some(centre), cursor: 0 }
    }

    /// Bring the strings toward STRETCH `stretch` (the knob's multiplier, as
    /// the voices take it), the next one not strung for it at a time, each
    /// paid `per` frames out of `credit`: `string_at(i, s)` is loop `i`'s
    /// string as the instrument strings it at stretch `s` (at 1, the string
    /// the set was designed from, to the bit). Re-designed here, and
    /// re-strung in `harp` (`Harp::retune`), which has this set loaded. A
    /// set whole for the knob is a compare, and banks nothing. Real-time: no
    /// allocation, and `credit / per + 1` strings at most, each a Newton
    /// solve and a few copies, 5 to 10 µs.
    pub fn follow(&mut self, stretch: f32, credit: &mut usize, per: usize, mut harp: Option<&mut Harp>, string_at: impl Fn(usize, f32) -> HarpString) {
        let want = stretch.to_bits();
        let per = per.max(1);
        while self.whole != Some(want) && *credit >= per {
            match (0..LOOPS).map(|d| (self.cursor + d) % LOOPS).find(|&i| self.strung[i] != want) {
                Some(i) => {
                    // No longer whole for whatever it was, so a knob put
                    // back before every string followed it is followed back
                    // (the table's rule, `grand::GrandDesigns::follow`).
                    self.whole = None;
                    self.strung[i] = want;
                    self.cursor = (i + 1) % LOOPS;
                    // A loop with no coupling is a string the set does not
                    // have (the concert grand's aliquots): nothing to tune.
                    if self.coef[i].couple == 0.0 && self.coef[i].own == 0.0 {
                        continue;
                    }
                    let s = string_at(i, stretch);
                    self.coef[i] = Coef::of(&s, self.rate, i);
                    if let Some(h) = harp.as_deref_mut() {
                        h.retune(i, &s, &self.coef[i]);
                    }
                    *credit -= per;
                }
                None => self.whole = Some(want),
            }
        }
        if self.whole == Some(want) {
            *credit = 0;
        }
    }
}

// ── the harp ──────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum State {
    /// Not ticking, and CLEARED — the invariant every wake relies on.
    Sleeping,
    /// Running: fed, or ringing out, or both.
    Ticking,
}

/// One string.
struct Loop {
    /// Where its line lives in the arena, and how long it is.
    off: u32,
    n: u32,
    write: u32,
    /// The whole samples of its delay, read behind the head; the fraction
    /// is `frac`, a first-order Thiran all-pass.
    back: u32,
    frac: Ap,
    loss: Lp,
    disp: Ap,
    /// The loop gain, where it is heading, and the ones it moves between:
    /// the free string's, the damped one's, the felt's bite on landing.
    gain: f32,
    gain_to: f32,
    gain_free: f32,
    gain_damped: f32,
    gain_land: f32,
    /// And the one it runs while its own voice sounds.
    gain_halo: f32,
    /// What it is fed with and heard through, now and heading: its input
    /// from the flat feed (`cin`, a coupling times `norm`) and from the
    /// shaped one (`cis`, times `norm_s`) — one of the two at rest, both
    /// while it crosses from one to the other, so a string never jumps from
    /// one feed to the other under a ringing line (`Feed`). The input shuts
    /// when a damper lands; the output never does — a damped string rings out
    /// heard as it was.
    cin: f32,
    cin_to: f32,
    cis: f32,
    cis_to: f32,
    cout: f32,
    cout_to: f32,
    /// Something above is still moving.
    ramping: bool,
    couple: f32,
    own: f32,
    after: f32,
    norm: f32,
    norm_s: f32,
    /// The feed band it reads.
    band: u8,
    /// Energy this frame, for the gate, and how many frames in a row it
    /// has been under the floor.
    acc: f32,
    quiet: u8,
    quiet_frames: u8,
    state: State,
    probation: u8,
    probation_frames: u8,
    /// Frames of the felt's bite left (`LAND_SECS`): zero once the string
    /// is on its damped residual, or free.
    land: u16,
    /// Its key is down (always false for an aliquot).
    held: bool,
    /// Its key's voice is sounding: the string is being played, and
    /// answers its voice only as its halo.
    voiced: bool,
    /// Its key has come up and its voice is still sounding (until
    /// `voice_done`): free under the pedal, it rings with its own voice.
    rung: bool,
    damper: bool,
}

impl Loop {
    fn silent(off: u32, n: u32) -> Self {
        Loop {
            off,
            n,
            write: 0,
            back: 2,
            frac: Ap::default(),
            loss: Lp::default(),
            disp: Ap::default(),
            gain: 0.0,
            gain_to: 0.0,
            gain_free: 0.0,
            gain_damped: 0.0,
            gain_land: 0.0,
            gain_halo: 0.0,
            cin: 0.0,
            cin_to: 0.0,
            cis: 0.0,
            cis_to: 0.0,
            cout: 0.0,
            cout_to: 0.0,
            ramping: false,
            couple: 0.0,
            own: 0.0,
            after: 0.0,
            norm: 1.0,
            norm_s: 1.0,
            band: 0,
            acc: 0.0,
            quiet: 0,
            quiet_frames: 2,
            state: State::Sleeping,
            probation: 0,
            probation_frames: 1,
            land: 0,
            held: false,
            voiced: false,
            rung: false,
            damper: true,
        }
    }

    /// Point the coupling and the loss where the flags and the dampers'
    /// pressure `p` (0 clear of the strings, 1 resting) say. Ramped: the
    /// tick moves the present values over the felt's travel.
    ///
    /// A held key, or a string with no damper, is clear of its felt
    /// whatever the pedal does. Otherwise the felt's pressure closes the
    /// string's coupling in proportion and moves its loss from the open
    /// one toward the felt's — its bite while `land` lasts, its damped
    /// residual after — geometrically, so the decay RATE blends linearly
    /// (half pressure, half the felt's extra nepers a second).
    ///
    /// **A sounding string's own loop runs a short loss, `HALO_T60`.** The
    /// reference's ring under a held note reaches its level within 50 to
    /// 100 ms and then holds a steady ratio to the note (C4 -32 dB from
    /// 0.05 s to 1 s, C2 -28), and after the key-up it decays at the damped
    /// rate, starting about 5 dB under where it stood. A resonator settles
    /// onto a decaying drive in about `1 / |sigma_loop - sigma_note + j dw|`:
    /// with the free loss (the note's own decay) and a cent of detune that
    /// is three seconds, and a first draft's halo swelled 16 dB over a
    /// two-second C4; with the damped loss (2 s at C4) it still took 1.5 s
    /// and rose 7 dB. With a 0.3 s loss it settles in tens of milliseconds
    /// and the ring tracks the note. A key with no damper has no felt to
    /// land: its own loop rings free, as the reference's treble does (its
    /// C7 ring outlasts the note).
    fn settle(&mut self, p: f32) {
        let p = if self.held || !self.damper { 0.0 } else { p.clamp(0.0, 1.0) };
        let c = if self.voiced {
            self.own
        } else if self.rung {
            self.after
        } else {
            self.couple
        };
        let open = if self.voiced { self.gain_halo } else { self.gain_free };
        let closed = if self.land > 0 { self.gain_land } else { self.gain_damped };
        let k = c * (1.0 - p);
        (self.cin_to, self.cis_to) = if self.voiced || self.rung { (k * self.norm, 0.0) } else { (0.0, k * self.norm_s) };
        self.cout_to = c;
        self.gain_to = if p <= 0.0 {
            open
        } else if p >= 1.0 {
            closed
        } else {
            open.powf(1.0 - p) * closed.powf(p)
        };
        self.ramping = true;
    }

    /// Fed, with something to feed.
    fn fed(&self) -> bool {
        self.cin_to > 0.0 || self.cis_to > 0.0
    }

    /// At rest on the shaped feed: what a settled run reads.
    fn shaped(&self) -> bool {
        self.cis_to > 0.0
    }

    /// Every coupling and the gain at their targets at once (a cleared
    /// loop, which has nothing to ramp under).
    fn snap(&mut self) {
        self.cin = self.cin_to;
        self.cis = self.cis_to;
        self.cout = self.cout_to;
        self.gain = self.gain_to;
        self.ramping = false;
    }

    /// Take a designed loop's coefficients. Coefficients only: the state
    /// circulating in the line is untouched.
    fn take(&mut self, c: &Coef) {
        self.back = c.d.back;
        self.frac.a = c.d.frac;
        self.loss.a = c.d.loss;
        self.disp.a = c.d.disp;
        self.gain_free = c.d.gain_free;
        self.gain_damped = c.d.gain_damped;
        self.gain_halo = c.d.gain_halo;
        self.gain_land = c.d.gain_land;
        self.couple = c.couple;
        self.own = c.own;
        self.after = c.after;
        self.norm = c.norm;
        self.norm_s = c.norm_s;
        self.band = c.band;
        self.damper = c.damper;
        self.probation_frames = c.probation_frames;
        self.quiet_frames = c.quiet_frames;
    }

    /// The state a run works on, out of the struct and into registers. A
    /// settled loop reads one feed, its own (`shaped`), through `ci`; a
    /// ramping one reads both, `ci` the flat and `cs` the shaped.
    fn hot(&self) -> Hot {
        let (ci, ci_to, cs, cs_to) = if !self.ramping && self.shaped() { (self.cis, self.cis_to, 0.0, 0.0) } else { (self.cin, self.cin_to, self.cis, self.cis_to) };
        Hot {
            fa: self.frac.a,
            fx1: self.frac.x1,
            fy1: self.frac.y1,
            la: self.loss.a,
            lb: 1.0 - self.loss.a,
            lz: self.loss.z,
            da: self.disp.a,
            dx1: self.disp.x1,
            dy1: self.disp.y1,
            g: self.gain,
            ci,
            cs,
            cout: self.cout,
            g_to: self.gain_to,
            ci_to,
            cs_to,
            cout_to: self.cout_to,
            acc: 0.0,
            w: self.write as usize,
            back: self.back as usize,
            n: self.n as usize,
        }
    }

    /// Put a run's state back, settling the ramps if they have arrived,
    /// and clearing the string if anything in it left the finite numbers
    /// (see `Hot`).
    fn cool(&mut self, h: &Hot, buf: &mut [f32]) {
        self.write = h.w as u32;
        // A settled loop's couplings and gain did not move.
        if self.ramping {
            let (mut g, mut ci, mut cs, mut cout) = (h.g, h.ci, h.cs, h.cout);
            if (ci - h.ci_to).abs() < 1.0e-9 && (cs - h.cs_to).abs() < 1.0e-9 && (cout - h.cout_to).abs() < 1.0e-9 && (g - h.g_to).abs() < 1.0e-9 {
                (g, ci, cs, cout) = (h.g_to, h.ci_to, h.cs_to, h.cout_to);
                self.ramping = false;
            }
            (self.gain, self.cin, self.cis, self.cout) = (g, ci, cs, cout);
        }
        if h.acc.is_finite() && h.fy1.is_finite() && h.lz.is_finite() && h.dy1.is_finite() {
            self.frac.x1 = h.fx1;
            self.frac.y1 = h.fy1;
            self.loss.z = h.lz;
            self.disp.x1 = h.dx1;
            self.disp.y1 = h.dy1;
            self.acc += h.acc;
        } else {
            // The same guard as the piano's loop, once a run: nothing
            // non-finite may live in a feedback path.
            self.frac.x1 = 0.0;
            self.frac.y1 = 0.0;
            self.loss.z = 0.0;
            self.disp.x1 = 0.0;
            self.disp.y1 = 0.0;
            buf.fill(0.0);
        }
    }
}

/// One string's working state for a run: what `run_one` / `run_two` keep
/// in registers.
///
/// **Shaped for speed, because every string runs it.** Under the pedal
/// with a full keyboard sounding all hundred and seven loops tick, and the
/// first form — a method per sample, its state in the struct, two index
/// wraps and two finite checks a sample — cost 5 ns a loop-sample and took
/// the 64-voice stress from 22x real time to 14x. Now the state lives in
/// locals for the run, the run is split where a read or a write wraps so
/// the inner loop has no branch, a settled loop runs without its ramps,
/// each recurrence is one multiply and one add from its next value, and
/// strings run in PAIRS: one string's sample is a chain of three
/// recurrences each waiting on its own last value, so a second string's
/// chain beside it is nearly free. The finite guard is once a run: the
/// drive is clamped before it gets here and every loop's gain is under
/// one, so nothing in a loop can leave the finite numbers, and a state that
/// ever did is cleared with its line (`Loop::cool`).
#[derive(Clone, Copy)]
struct Hot {
    fa: f32,
    fx1: f32,
    fy1: f32,
    la: f32,
    lb: f32,
    lz: f32,
    da: f32,
    dx1: f32,
    dy1: f32,
    g: f32,
    ci: f32,
    cs: f32,
    cout: f32,
    g_to: f32,
    ci_to: f32,
    cs_to: f32,
    cout_to: f32,
    acc: f32,
    w: usize,
    back: usize,
    n: usize,
}

impl Hot {
    /// Where the read is, behind the write.
    #[inline(always)]
    fn read_at(&self) -> usize {
        if self.w >= self.back {
            self.w - self.back
        } else {
            self.w + self.n - self.back
        }
    }

    /// How many samples from here neither index wraps.
    #[inline(always)]
    fn clear_run(&self, r: usize) -> usize {
        (self.n - self.w).min(self.n - r)
    }

    /// One sample at read `r`, write `w` (no wrap inside a run), fed `x`
    /// (and, ramping, `xs` through the shaped coupling): returns what the
    /// bridge hears of it, `HEARD` and all. Each recurrence is written so
    /// its state is one multiply and one add from its next value — `b z +
    /// a y`, `(a s + x1) - a y1` — the short chain without a fused
    /// multiply-add, which on a CPU without one is a library call
    /// (`f32::mul_add` on x86 without the FMA feature).
    #[inline(always)]
    #[allow(clippy::too_many_arguments)]
    fn step<const RAMP: bool>(&mut self, buf: &mut [f32], r: usize, w: usize, x: f32, xs: f32, ramp: f32, tail: f32) -> f32 {
        let s = buf[r];
        let y = (self.fa * s + self.fx1) - self.fa * self.fy1;
        self.fx1 = s;
        self.fy1 = y;
        self.lz = self.lb * self.lz + self.la * y;
        let v0 = self.lz * self.g;
        let v = (self.da * v0 + self.dx1) - self.da * self.dy1;
        self.dx1 = v0;
        self.dy1 = v;
        buf[w] = if RAMP { v + (self.ci * x + self.cs * xs) } else { v + self.ci * x };
        self.acc += y * y;
        let out = HEARD * (y * self.cout);
        if RAMP {
            self.ci += (self.ci_to - self.ci) * ramp;
            self.cs += (self.cs_to - self.cs) * ramp;
            self.cout += (self.cout_to - self.cout) * ramp;
            self.g += (self.g_to - self.g) * tail;
        }
        out
    }
}

/// One string over a run: read through the Thiran, lose, stiffen, write
/// back with the drive (`x`, and `xs` while ramping), and ADD what the
/// bridge hears of it into `out`.
///
/// Inlined into each caller, as it always was into `render_part`: with a
/// second caller there (a gliding string's part, `run_alone`) the compiler
/// called it instead, and a held pedalled chord at 16-frame calls cost 4.6 %
/// more instructions and 5 % more cycles. The same for `run_two`.
#[inline(always)]
fn run_one<const RAMP: bool>(h: &mut Hot, buf: &mut [f32], x: &[f32], xs: &[f32], out: &mut [f32], ramp: f32, tail: f32) {
    let len = x.len().min(xs.len()).min(out.len());
    let mut k = 0;
    while k < len {
        let r = h.read_at();
        let span = (len - k).min(h.clear_run(r));
        let w = h.w;
        for j in 0..span {
            let o = h.step::<RAMP>(buf, r + j, w + j, x[k + j], xs[k + j], ramp, tail);
            out[k + j] += o;
        }
        h.w = if w + span == h.n { 0 } else { w + span };
        k += span;
    }
}

/// Two strings over the same run, interleaved, each fed its own band's
/// drive: each output sample takes the first string's part, then the
/// second's — the order two `run_one` calls give it, so a pair is
/// bit-identical to its two strings in turn.
#[inline(always)]
#[allow(clippy::too_many_arguments)]
fn run_two<const RAMP: bool>(a: &mut Hot, ba: &mut [f32], xa: [&[f32]; 2], b: &mut Hot, bb: &mut [f32], xb: [&[f32]; 2], out: &mut [f32], ramp: f32, tail: f32) {
    let len = xa[0].len().min(xa[1].len()).min(xb[0].len()).min(xb[1].len()).min(out.len());
    let mut k = 0;
    while k < len {
        let (ra, rb) = (a.read_at(), b.read_at());
        let span = (len - k).min(a.clear_run(ra)).min(b.clear_run(rb));
        let (wa, wb) = (a.w, b.w);
        for j in 0..span {
            let oa = a.step::<RAMP>(ba, ra + j, wa + j, xa[0][k + j], xa[1][k + j], ramp, tail);
            let ob = b.step::<RAMP>(bb, rb + j, wb + j, xb[0][k + j], xb[1][k + j], ramp, tail);
            out[k + j] += oa;
            out[k + j] += ob;
        }
        a.w = if wa + span == a.n { 0 } else { wa + span };
        b.w = if wb + span == b.n { 0 } else { wb + span };
        k += span;
    }
}

// ── a re-tuned string, ringing ────────────────────────────────────────────

/// **A re-tuned string's glide** (`Harp::retune`): a ringing string brought
/// to its new design the way a string re-tuned under the hand moves, kept
/// beside its loop rather than in it, so that a loop not gliding is the loop
/// it was and runs the run it ran (`run_two`, `run_one`). The design it
/// glides to, and where it is on the way: its line's length past the
/// design's whole line, samples, the Thiran's fraction in it (`rel`, which
/// ends at the design's own fraction); its all-pass's delay and its loss
/// pole; each with where it is heading. Whether it glides; whether it reads
/// another feed band than its design's (`kept_band`), which it keeps while it
/// rings and gives up asleep; and a line step's crossfade (`step_line`): the
/// samples of it left, and the read it is leaving, its line and its Thiran,
/// which rings on beside the new read until the fade is through.
#[derive(Clone, Copy, Default)]
struct Glide {
    on: bool,
    kept_band: bool,
    to: Design,
    rel: f32,
    rel_to: f32,
    tau: f32,
    tau_to: f32,
    pole: f32,
    pole_to: f32,
    fade: u16,
    old_back: u32,
    old_frac: Ap,
    /// **Where the three stood at the last run's end that stepped** (`cool`):
    /// what a stall is judged against, over the whole run between two
    /// steps however many pieces the host cut it into.
    at_rel: f32,
    at_tau: f32,
    at_pole: f32,
}

impl Glide {
    /// Glide loop `l` to `c`, its new design fed from the band it reads:
    /// everything `take` takes but the line and the Thiran, the loss and the
    /// all-pass, which glide there from where they stand (`GLIDE_SECS`), the
    /// line a sample at a time (`step_line`). Taken at once, they were a step
    /// under the ringing string: a pedalled chord's harp fourth difference 4
    /// to 290 times its steady one, the electric grand's output second
    /// difference 3.4 times (`a_ringing_string_is_retuned_without_a_click`).
    fn start(&mut self, l: &mut Loop, c: &Coef) {
        let (back, fa, la, da) = (l.back, l.frac.a, l.loss.a, l.disp.a);
        // Where it stands, as delays and a pole, the line's length measured
        // from the new design's whole line.
        let (rel, tau, pole) = if self.on {
            (self.rel + self.to.back as f32 - c.d.back as f32, self.tau, self.pole)
        } else {
            (back as f32 - c.d.back as f32 + allpass_delay(fa), allpass_delay(da), 1.0 - la)
        };
        l.take(c);
        (l.back, l.frac.a, l.loss.a, l.disp.a) = (back, fa, la, da);
        self.to = c.d;
        (self.rel, self.rel_to) = (rel, allpass_delay(c.d.frac));
        (self.tau, self.tau_to) = (tau, allpass_delay(c.d.disp));
        (self.pole, self.pole_to) = (pole, 1.0 - c.d.loss);
        (self.at_rel, self.at_tau, self.at_pole) = (rel, tau, pole);
        self.on = true;
    }

    /// Its state for a run of loop `l` (`run_glide`), `k` the glide's
    /// per-sample step.
    fn hot(&self, l: &Loop, k: f32) -> GlideHot {
        GlideHot {
            rel: self.rel,
            rel_to: self.rel_to,
            lift: self.to.back as f32 - l.back as f32,
            tau: self.tau,
            tau_to: self.tau_to,
            pole: self.pole,
            pole_to: self.pole_to,
            k,
            fade: self.fade,
            ob: self.old_back as usize,
            ofa: self.old_frac.a,
            ofx1: self.old_frac.x1,
            ofy1: self.old_frac.y1,
        }
    }

    /// After loop `l`'s run (and its `Loop::cool`, which keeps the line's and
    /// the Thiran's state): the glide's own state and the coefficients it
    /// moved kept, and at a run's `end` its line stepped (`step_line`), and
    /// once it has arrived the design's own coefficients set and the glide
    /// over.
    ///
    /// **A run ends where the harp's frame does or where the keyboard's
    /// block does** (`Harp::render_piece`), never where a host's call cut
    /// the block: the line stepped and the arrival was judged at every piece,
    /// so a call cut inside a block stepped a gliding string's line once
    /// more there, and the repeats under STRETCH were -79 to -92 dB re peak
    /// off the whole take on Piano 1 at calls cut off the grid
    /// (`keys::tests::a_stretch_renders_the_same_called_whole_or_split_anywhere`).
    fn cool(&mut self, l: &mut Loop, h: &Hot, g: &GlideHot, buf: &[f32], end: bool) {
        (self.rel, self.tau, self.pole, self.fade) = (g.rel, g.tau, g.pole, g.fade);
        if g.ofx1.is_finite() && g.ofy1.is_finite() {
            (self.old_frac.x1, self.old_frac.y1) = (g.ofx1, g.ofy1);
        } else {
            // Nothing non-finite lives in a feedback path (`Loop::cool`):
            // the read it was leaving is let go.
            (self.old_frac.x1, self.old_frac.y1, self.fade) = (0.0, 0.0, 0);
        }
        (l.frac.a, l.loss.a, l.disp.a) = (h.fa, h.la, h.da);
        if !end {
            return;
        }
        // Which of the three the run left where it found them (`arrived`).
        let still = (self.rel == self.at_rel, self.tau == self.at_tau, self.pole == self.at_pole);
        (self.at_rel, self.at_tau, self.at_pole) = (self.rel, self.tau, self.pole);
        self.step_line(l, buf);
        // **Near its target, or stopped short of it.** Each sample moves a
        // glide by `k` of what is left, and once that is under half an `f32`
        // step of the value it moves nothing: a run that left a value where it
        // was has stalled there for good. The all-pass's delay sits at up to
        // its cap (`AP_TAU_MAX_SECS`), 38 samples at 96 kHz and 76 at 192, and
        // stalled 3.7 and 15 thousandths of a sample short of its design, wider
        // than `GLIDE_DONE`: those glides never ended while their strings
        // rang, and kept the harp on its dearer gliding path
        // (`a_glide_ends_at_every_rate`). At 48 kHz a value stalls only inside
        // `GLIDE_DONE`, so nothing there moves.
        let arrived = |a: f32, b: f32, still: bool| still || (a - b).abs() < GLIDE_DONE;
        if self.fade == 0 && l.back == self.to.back && arrived(self.rel, self.rel_to, still.0) && arrived(self.tau, self.tau_to, still.1) && arrived(self.pole, self.pole_to, still.2) {
            (l.frac.a, l.loss.a, l.disp.a) = (self.to.frac, self.to.loss, self.to.disp);
            self.on = false;
        }
    }

    /// **A gliding string's line follows its length, a sample at a time**:
    /// toward the whole line its design would read at this length (its own
    /// fraction past it), when the Thiran can take the sample back, so that
    /// the loop's delay does not move (its fraction inside `[0.05, 4]`, the
    /// design's own bounds), and the read it leaves fades out under the new
    /// one (`FADE`). Once a run, between runs, so a run's read never moves: a
    /// run is at most a frame, and the length moves at most `SPAN_STEP` a
    /// sample, half a sample a run.
    fn step_line(&mut self, l: &mut Loop, buf: &[f32]) {
        if self.fade > 0 {
            return;
        }
        let (back, n) = (i64::from(l.back), i64::from(l.n));
        let ideal = (i64::from(self.to.back) + (self.rel - self.rel_to).round() as i64).clamp(1, n - 3);
        if ideal == back {
            return;
        }
        let next = if ideal > back { back + 1 } else { back - 1 };
        let d = self.rel + self.to.back as f32 - next as f32;
        if !(0.05..=4.0).contains(&d) {
            return;
        }
        // The read it leaves rings on through its own Thiran while the new
        // one fades in (`FADE`): the two hear the line with the same delay
        // and other dispersions (three thousandths of a sample at 1 kHz, a
        // twentieth at 4 kHz, between fractions 1.5 and 0.5), and a step
        // from one to the other, the Thiran's last input carried over alone,
        // was a click under the ringing string: the harp's fourth difference
        // 1.6 and 2.9 times its steady one over STRETCH's jump on the two
        // grands, faded 1.0 and 0.65 (`a_ringing_string_is_retuned_without_a_click`).
        self.old_back = l.back;
        self.old_frac = l.frac;
        self.fade = FADE;
        l.back = next as u32;
        l.frac.a = allpass_delay(d);
        let (w, n) = (l.write as usize, n as usize);
        l.frac.x1 = buf[(w + n - next as usize - 1) % n];
    }
}

/// A gliding string's working state for a run (`run_glide`): its `Glide`'s,
/// in registers as `Hot` is, with its line's whole length from the
/// design's (`lift`) and the glide's per-sample step (`k`).
#[derive(Clone, Copy)]
struct GlideHot {
    rel: f32,
    rel_to: f32,
    lift: f32,
    tau: f32,
    tau_to: f32,
    pole: f32,
    pole_to: f32,
    k: f32,
    fade: u16,
    ob: usize,
    ofa: f32,
    ofx1: f32,
    ofy1: f32,
}

impl Hot {
    /// `step` for a string gliding to a re-tuned design (`Glide`), the same
    /// chain with the glide around it, in its own copy so that `step` stays
    /// what it was: carried through it, a glide's state made every string's
    /// loop-sample 1.57 ns where it was 1.10 (`cost_is_measured`), and the
    /// stress score 4 % dearer with nothing gliding. Across a line step the
    /// read the string leaves fades out under the new one; after the sample
    /// its line's length, its all-pass's delay and its loss pole move a
    /// glide's step toward the design's, the length's step held to
    /// `SPAN_STEP` and the Thiran's fraction to its design bounds, and the
    /// coefficients follow them.
    #[inline(always)]
    #[allow(clippy::too_many_arguments)] // `step`'s run, and the glide's state beside it
    fn step_glide<const RAMP: bool>(&mut self, g: &mut GlideHot, buf: &mut [f32], r: usize, w: usize, x: f32, xs: f32, ramp: f32, tail: f32) -> f32 {
        let s = buf[r];
        let y = (self.fa * s + self.fx1) - self.fa * self.fy1;
        self.fx1 = s;
        self.fy1 = y;
        let y = if g.fade > 0 {
            let ro = if w >= g.ob { w - g.ob } else { w + self.n - g.ob };
            let so = buf[ro];
            let yo = (g.ofa * so + g.ofx1) - g.ofa * g.ofy1;
            g.ofx1 = so;
            g.ofy1 = yo;
            let t = f32::from(g.fade) * (1.0 / f32::from(FADE));
            g.fade -= 1;
            y + (yo - y) * t
        } else {
            y
        };
        self.lz = self.lb * self.lz + self.la * y;
        let v0 = self.lz * self.g;
        let v = (self.da * v0 + self.dx1) - self.da * self.dy1;
        self.dx1 = v0;
        self.dy1 = v;
        buf[w] = if RAMP { v + (self.ci * x + self.cs * xs) } else { v + self.ci * x };
        self.acc += y * y;
        let out = HEARD * (y * self.cout);
        if RAMP {
            self.ci += (self.ci_to - self.ci) * ramp;
            self.cs += (self.cs_to - self.cs) * ramp;
            self.cout += (self.cout_to - self.cout) * ramp;
            self.g += (self.g_to - self.g) * tail;
        }
        g.rel += ((g.rel_to - g.rel) * g.k).clamp(-SPAN_STEP, SPAN_STEP);
        g.tau += (g.tau_to - g.tau) * g.k;
        g.pole += (g.pole_to - g.pole) * g.k;
        self.fa = allpass_delay((g.rel + g.lift).clamp(0.05, 4.0));
        self.da = allpass_delay(g.tau);
        self.la = 1.0 - g.pole;
        self.lb = 1.0 - self.la;
        out
    }
}

/// `run_one` for a string gliding to a re-tuned design (`step_glide`).
#[allow(clippy::too_many_arguments)] // `run_one`'s, and the glide's state beside them
fn run_glide<const RAMP: bool>(h: &mut Hot, g: &mut GlideHot, buf: &mut [f32], x: &[f32], xs: &[f32], out: &mut [f32], ramp: f32, tail: f32) {
    let len = x.len().min(xs.len()).min(out.len());
    let mut k = 0;
    while k < len {
        let r = h.read_at();
        let span = (len - k).min(h.clear_run(r));
        let w = h.w;
        for j in 0..span {
            let o = h.step_glide::<RAMP>(g, buf, r + j, w + j, x[k + j], xs[k + j], ramp, tail);
            out[k + j] += o;
        }
        h.w = if w + span == h.n { 0 } else { w + span };
        k += span;
    }
}

/// One string over a run on its own, `l` with its glide `g` and its line
/// `buf`: `run_one`, or `run_glide` while it glides to a re-tuned design.
#[allow(clippy::too_many_arguments)] // a run's pieces, borrowed apart from the harp by `render_part`
fn run_alone(l: &mut Loop, g: &mut Glide, buf: &mut [f32], feed: &Feed, m: usize, out: &mut [f32], ramp: f32, tail: f32, glide: f32, end: bool) {
    let [x, xs] = feeds(feed, l, m);
    let mut h = l.hot();
    if g.on {
        let mut gh = g.hot(l, glide);
        if l.ramping {
            run_glide::<true>(&mut h, &mut gh, buf, x, xs, out, ramp, tail);
        } else {
            run_glide::<false>(&mut h, &mut gh, buf, x, xs, out, ramp, tail);
        }
        l.cool(&h, buf);
        g.cool(l, &h, &gh, buf, end);
    } else {
        if l.ramping {
            run_one::<true>(&mut h, buf, x, xs, out, ramp, tail);
        } else {
            run_one::<false>(&mut h, buf, x, xs, out, ramp, tail);
        }
        l.cool(&h, buf);
    }
}

/// The sign the harp is heard with (`run_one`, `run_two`): as it sounds.
/// Fed the drive's first difference, a loop answered a quarter-cycle off
/// its driver and had to be heard turned for a released note's tail to
/// continue the note rather than notch it; fed flat it answers in phase,
/// heard as it is: the note with its halo is the references' +0.04 to +0.18
/// dB over the note alone (here +0.09 to +0.53; turned, -0.09 to -0.52), and
/// the released fundamental with its tail under it dips at most 0.6 dB on
/// the intimate grand where turned it dipped 1.1 (`a_released_note_joins_
/// its_tail`).
const HEARD: f32 = 1.0;

/// How long the felt bites, in frames, bearing at pressure `p`: the full
/// `LAND_SECS` resting, in proportion lighter — a half-pedalled felt bites
/// half as hard (`Loop::settle`) and half as long, so a pedal brought to
/// half way lands its strings where a pedal held there all along has them
/// (`half_pedal_lands_the_same_however_it_got_there`: bitten for the full
/// time at the light pressure they stood 4.1 dB under).
fn bite(frames: u16, p: f32) -> u16 {
    ((f32::from(frames) * p.clamp(0.0, 1.0)).round() as u16).max(1)
}

/// How hard the harp's dampers bear at a sustain pedal position (CC 64 as
/// 0..1): 1 resting with their full weight, 0 clear (`PEDAL_LIFT`). A NaN
/// is no pedal.
fn damper_pressure(sustain: f32) -> f32 {
    let s = if sustain.is_finite() { sustain.clamp(0.0, 1.0) } else { 0.0 };
    let [lo, hi, q] = PEDAL_LIFT;
    let x = ((hi - s) / (hi - lo)).clamp(0.0, 1.0);
    if x >= 1.0 {
        1.0
    } else {
        x.powf(q)
    }
}

/// What loop `l` reads this part-frame: while ramping the flat and the
/// shaped feeds of its band; settled, the one it is on, twice (`Loop::hot`).
fn feeds<'a>(feed: &'a Feed, l: &Loop, m: usize) -> [&'a [f32]; 2] {
    let b = usize::from(l.band);
    if l.ramping {
        [&feed.x[b][..m], &feed.xs[b][..m]]
    } else if l.shaped() {
        [&feed.xs[b][..m], &feed.xs[b][..m]]
    } else {
        [&feed.x[b][..m], &feed.x[b][..m]]
    }
}

/// The sympathetic strings of one instrument.
pub struct Harp {
    loops: [Loop; LOOPS],
    /// Every line, end to end. One allocation, in `new`, and never again.
    arena: Vec<f32>,
    /// The loops that are not asleep, so the tick visits five strings in
    /// five strings' time rather than in a hundred branches'.
    live: [u8; LOOPS],
    n_live: usize,
    rate: f32,
    /// How hard the dampers bear: 1 resting (pedal up), 0 clear (down).
    pressure: f32,
    send: f32,
    ramp: f32,
    tail: f32,
    /// The felt's bite, in frames.
    land_frames: u16,
    /// What the loops are fed: the drive, sanitised and sent, filtered per
    /// band; and this part-frame's sent drive before the filters.
    feed: Feed,
    u: [f32; FRAME],
    /// Samples into the current frame.
    phase: usize,
    drive_acc: f32,
    /// The last frame's mean-square drive, and whether it was anything.
    drive_ms: f32,
    drive_present: bool,
    /// Loop-ticks so far: how much of the harp has actually run.
    ticks: u64,
    /// What each loop was last designed as (`load`, `retune`): what a
    /// re-tuned string still gliding, or keeping another feed band, takes
    /// once it is asleep (`sleep`). Each loop's glide to a re-tuned design
    /// while it rings (`Glide`), whether any glides (set when one starts,
    /// recounted once a frame: what lets a pair of strings run as it always
    /// ran with one test), and the per-sample step a glide takes
    /// (`GLIDE_SECS`).
    designs: [Coef; LOOPS],
    glides: [Glide; LOOPS],
    gliding: bool,
    glide: f32,
    /// Test-only: a ringing string re-strung takes its new design at once,
    /// to measure the step the glide is there to take out; and how many
    /// strings were re-strung asleep, and ringing.
    #[cfg(test)]
    retune_at_once: bool,
    #[cfg(test)]
    retunes: [u32; 2],
}

impl Harp {
    /// Build the harp. **Allocates**, once, here.
    pub fn new(sample_rate: f32) -> Self {
        let rate = if sample_rate.is_finite() && sample_rate > 0.0 { sample_rate } else { 48_000.0 };
        // Each line sized to its own key (`line_len`); the arena is their sum.
        let sizes: [u32; LOOPS] = std::array::from_fn(|i| line_len(i, rate));
        let total: u32 = sizes.iter().sum();
        let mut off = 0u32;
        let loops = std::array::from_fn(|i| {
            let l = Loop::silent(off, sizes[i]);
            off += sizes[i];
            l
        });
        let mut harp = Harp {
            loops,
            arena: vec![0.0; total as usize],
            live: [0; LOOPS],
            n_live: 0,
            rate,
            pressure: 1.0,
            send: 1.0,
            ramp: 1.0 - (-1.0 / (DAMP_SECS * rate)).exp(),
            tail: 1.0 - (-1.0 / (TAIL_SECS * rate)).exp(),
            land_frames: ((LAND_SECS * rate / FRAME as f32).round() as u32).clamp(1, u32::from(u16::MAX)) as u16,
            feed: Feed::new(rate),
            u: [0.0; FRAME],
            phase: 0,
            drive_acc: 0.0,
            drive_ms: 0.0,
            drive_present: false,
            ticks: 0,
            designs: [Coef::default(); LOOPS],
            glides: [Glide::default(); LOOPS],
            gliding: false,
            glide: 1.0 - (-1.0 / (GLIDE_SECS * rate)).exp(),
            #[cfg(test)]
            retune_at_once: false,
            #[cfg(test)]
            retunes: [0; 2],
        };
        harp.set_base(&PianoBase::JAZZ);
        harp
    }

    /// The rate this harp was built for: what its `Strung` sets are
    /// designed at.
    #[allow(dead_code)] // The frame designs its sets at its own rate; the tests ask.
    pub fn rate(&self) -> f32 {
        self.rate
    }

    /// Design a string set for this harp (`Strung::new` at its rate).
    /// Dear: call it where time is not short, not from the callback.
    pub fn strung(&self, set: &StringSet) -> Strung {
        Strung::new(set, self.rate)
    }

    /// Load a designed string set: every loop's coefficients and the send.
    /// Coefficients only, under whatever is circulating — no state is
    /// touched, so it is safe on a ringing harp the way a restrike's
    /// re-coefficienting is safe on a ringing loop; the couplings and gains
    /// move to their new values over the felt's travel. A copy: real-time
    /// safe. A set designed at another rate than this harp's would tune
    /// every string wrong, so it is not loaded (`strung` designs at the
    /// harp's own rate; nothing else should make one).
    pub fn load(&mut self, s: &Strung) {
        debug_assert_eq!(s.rate, self.rate, "a string set designed at another rate");
        if s.rate != self.rate {
            return;
        }
        self.send = s.send;
        self.designs = s.coef;
        self.glides = [Glide::default(); LOOPS];
        let p = self.pressure;
        for (l, c) in self.loops.iter_mut().zip(&s.coef) {
            l.take(c);
            l.settle(p);
            if l.state == State::Sleeping {
                l.snap();
            }
        }
    }

    /// **Re-string loop `i` as `s`**, designed for it as `c` (`Strung::follow`).
    /// Asleep, it takes `c`, as `load` would. Ringing, it glides there
    /// (`Glide::start`): its line's length, its all-pass and its loss move
    /// to `c`'s as a string re-tuned under the hand moves, its gains and
    /// couplings as at any change of theirs, and it keeps the feed band it
    /// reads until it is asleep. Real-time safe: copies, and the band's two
    /// gains when it keeps one.
    fn retune(&mut self, i: usize, s: &HarpString, c: &Coef) {
        self.designs[i] = *c;
        let (p, rate) = (self.pressure, self.rate);
        let (l, g) = (&mut self.loops[i], &mut self.glides[i]);
        if l.state == State::Sleeping {
            #[cfg(test)]
            {
                self.retunes[0] += 1;
            }
            *g = Glide::default();
            l.take(c);
            l.settle(p);
            l.snap();
            return;
        }
        #[cfg(test)]
        {
            self.retunes[1] += 1;
            if self.retune_at_once {
                *g = Glide::default();
                l.take(c);
                l.settle(p);
                return;
            }
        }
        // Fed from the band it reads now: a switch of feed under a ringing
        // string is a step in what it hears.
        let k = if l.band == c.band { *c } else { Coef::with(&s.sane(), rate, c.d, usize::from(l.band)) };
        g.start(l, &k);
        g.kept_band = k.band != c.band;
        l.settle(p);
        self.gliding = true;
    }

    /// String the harp: design `set` and load it (`strung` + `load`). Dear
    /// (the design); an instrument that strings on the audio thread should
    /// keep its `Strung` sets and `load` them instead.
    pub fn string(&mut self, set: &StringSet) {
        let s = self.strung(set);
        self.load(&s);
    }

    /// String the harp from a waveguide piano base: `StringSet::piano`.
    pub fn set_base(&mut self, base: &PianoBase) {
        self.string(&StringSet::piano(base));
    }

    /// Bytes of string this harp carries — the arena, which is the whole
    /// of its memory beyond `size_of::<Harp>()`.
    ///
    /// A measurement, and the memory tests are what read it.
    #[allow(dead_code)]
    pub fn arena_bytes(&self) -> usize {
        self.arena.len() * size_of::<f32>()
    }

    fn index(pitch: u8) -> Option<usize> {
        let i = usize::from(pitch.checked_sub(FIRST_KEY)?);
        (i < KEYS).then_some(i)
    }

    /// Start a sleeping loop ticking. It is cleared — that is what sleeping
    /// means — so its first sample out is the first sample of drive it
    /// hears, and its couplings may stand at their targets at once.
    fn wake(&mut self, i: usize) {
        let l = &mut self.loops[i];
        debug_assert_eq!(l.state, State::Sleeping);
        l.state = State::Ticking;
        l.snap();
        l.acc = 0.0;
        l.quiet = 0;
        l.probation = l.probation_frames;
        self.live[self.n_live] = i as u8;
        self.n_live += 1;
    }

    /// Stop a loop and clear it. Only ever called on one whose output is
    /// under the floor, or from `clear`. A cleared string has nothing for
    /// a felt to bite on: its landing ends here.
    fn sleep(&mut self, i: usize) {
        let (p, c) = (self.pressure, self.designs[i]);
        let l = &mut self.loops[i];
        // A re-tuned string still gliding, or reading another band than its
        // design, takes the harp's design for it: cleared, there is nothing
        // to hear it change.
        let g = &mut self.glides[i];
        if g.on || g.kept_band {
            l.take(&c);
        }
        *g = Glide::default();
        l.state = State::Sleeping;
        l.acc = 0.0;
        l.quiet = 0;
        l.probation = 0;
        l.land = 0;
        l.write = 0;
        l.frac.x1 = 0.0;
        l.frac.y1 = 0.0;
        l.loss.z = 0.0;
        l.disp.x1 = 0.0;
        l.disp.y1 = 0.0;
        l.settle(p);
        l.snap();
        let (a, b) = (l.off as usize, (l.off + l.n) as usize);
        self.arena[a..b].fill(0.0);
    }

    /// A loop that may have something to answer: a sleeping one that is
    /// fed wakes (if anything is playing, or `force`), a ticking one starts
    /// its probation again.
    fn rouse(&mut self, i: usize, force: bool) {
        let l = &self.loops[i];
        if !l.fed() {
            return;
        }
        match l.state {
            State::Sleeping => {
                if force || self.drive_present {
                    self.wake(i);
                }
            }
            State::Ticking => {
                let l = &mut self.loops[i];
                l.probation = l.probation_frames;
            }
        }
    }

    /// The sustain pedal as a switch: all the way down, or up.
    #[allow(dead_code)] // A frame that knows the pedal as a level sends it (`set_sustain`); the switch is for one that does not, and the tests.
    pub fn set_pedal(&mut self, down: bool) {
        self.set_sustain(if down { 1.0 } else { 0.0 });
    }

    /// The sustain pedal as a level (CC 64 as 0..1). Lifting the dampers
    /// frees every string a little or all the way; letting them down closes
    /// every string whose key is not held, and each takes the felt's bite
    /// as it closes (`LAND_T60`), re-armed every time the felt bears down
    /// further.
    pub fn set_sustain(&mut self, sustain: f32) {
        let p = damper_pressure(sustain);
        if p == self.pressure {
            return;
        }
        let rising = p > self.pressure;
        self.pressure = p;
        for i in 0..KEYS {
            let l = &mut self.loops[i];
            // A string whose key is up is not its voice's halo (`key_up`),
            // so every string the felt can reach is a free one.
            if rising && !l.held && l.damper && l.state == State::Ticking {
                l.land = bite(self.land_frames, p);
            }
            l.settle(p);
            if p < 1.0 {
                self.rouse(i, false);
            }
        }
    }

    /// A key goes down, which is a strike: that string is the voice's now
    /// and answers it only as its halo, and every fed string is woken to
    /// find out whether it answers the note.
    pub fn key_down(&mut self, pitch: u8) {
        let Some(k) = Self::index(pitch) else {
            return;
        };
        let p = self.pressure;
        let l = &mut self.loops[k];
        l.held = true;
        l.voiced = true;
        l.rung = false;
        l.land = 0;
        l.settle(p);
        self.drive_present = true;
        for i in 0..LOOPS {
            self.rouse(i, true);
        }
    }

    /// The key comes up, and its string is no longer its voice's halo:
    /// the halo is the ring of a string the hammer's key holds, and a
    /// string let go is one of the harp's. With the dampers resting its felt
    /// lands (if it has one): a string leaving its halo eases to its damped
    /// tail (`TAIL_SECS`), one that was ringing free (a silently held key)
    /// takes the felt's bite first. Under the pedal it is a free string,
    /// fed at its `couple` and ringing at its free decay, which builds on
    /// its own voice and on everything else — the reference's bank under a
    /// pedalled cluster grows from 22 to 12 dB under the voices over three
    /// seconds — where a released key's halo kept as the voice's follower,
    /// broad by design, answered every neighbour's partials and put a
    /// pedalled passage's harp 0 to 2 dB under its voices from 500 Hz to
    /// 4 kHz on the concert grand (the references' banks: 9 to 23 dB under).
    ///
    /// A string the harp does not hold has no key to come up: a stray or
    /// doubled note-off re-armed its felt and took its ring back (review).
    /// And one the sostenuto held (`key_up_held`) left its halo when its key
    /// came up, and is still its voice's here.
    pub fn key_up(&mut self, pitch: u8) {
        let Some(k) = Self::index(pitch) else {
            return;
        };
        let p = self.pressure;
        let lf = self.land_frames;
        let l = &mut self.loops[k];
        if !l.held {
            return;
        }
        l.held = false;
        if l.damper && p > 0.0 && !l.voiced && l.state == State::Ticking {
            l.land = bite(lf, p);
        }
        l.rung = l.rung || l.voiced;
        l.voiced = false;
        l.settle(p);
    }

    /// The key comes up while something else holds its damper off (the
    /// sostenuto, which caught it): its string leaves its halo as at
    /// `key_up`, the harp's own and free, ringing with its voice, and stays
    /// clear of its felt as while its key was down, until `key_up` lets it
    /// go when the hold ends. Kept as the voice's halo instead, a caught
    /// key's string answered every neighbour's partials as a held one does
    /// for as long as the pedal stayed down (review).
    pub fn key_up_held(&mut self, pitch: u8) {
        let Some(k) = Self::index(pitch) else {
            return;
        };
        let p = self.pressure;
        let l = &mut self.loops[k];
        if !l.held {
            return;
        }
        l.rung = l.rung || l.voiced;
        l.voiced = false;
        l.settle(p);
    }

    /// The frame reports that this key's voice has gone silent. The string
    /// is the harp's again: free, it is fed as a sympathetic string and, if
    /// something is playing, starts listening.
    pub fn voice_done(&mut self, pitch: u8) {
        let Some(k) = Self::index(pitch) else {
            return;
        };
        let p = self.pressure;
        self.loops[k].voiced = false;
        self.loops[k].rung = false;
        self.loops[k].settle(p);
        self.rouse(k, false);
    }

    /// Anything ticking — sounding, or ringing toward silence.
    pub fn active(&self) -> bool {
        self.n_live > 0
    }

    /// How many loops are running right now.
    ///
    /// The wake/sleep tests are what read it: `active()` is the question the
    /// instrument asks, this is the number the gate is judged on.
    #[allow(dead_code)]
    pub fn ticking(&self) -> usize {
        self.n_live
    }

    /// Loop-ticks so far — the gate's account of itself.
    ///
    /// Read by the tests that pin what "a loop ticks only while it is worth
    /// ticking" actually costs. Nothing in the instrument asks.
    #[allow(dead_code)]
    pub fn ticks(&self) -> u64 {
        self.ticks
    }

    /// `pitch`'s string as the keys have left it: its flags (held, voiced,
    /// rung) and where its couplings and loop gain are heading (`cin_to`,
    /// `cis_to`, `cout_to`, `gain_to`). The frame's tests of what a key-up
    /// tells its string read it.
    #[cfg(test)]
    pub(super) fn string_state(&self, pitch: u8) -> Option<([bool; 3], [f32; 4])> {
        let l = &self.loops[Self::index(pitch)?];
        Some(([l.held, l.voiced, l.rung], [l.cin_to, l.cis_to, l.cout_to, l.gain_to]))
    }

    /// Silence, now, everything: strings cleared, keys up, the pedal as it
    /// was. What a patch change wants: `release_all` and then `hush`.
    pub fn clear(&mut self) {
        self.release_all();
        self.hush();
    }

    /// Every key up and every string the harp's again, whatever is still
    /// sounding: what the frame's all notes off tells the strings, whose
    /// sound it then fades out and clears with `hush`.
    pub fn release_all(&mut self) {
        let p = self.pressure;
        for l in self.loops.iter_mut() {
            l.held = false;
            l.voiced = false;
            l.rung = false;
            l.settle(p);
        }
    }

    /// The sound cleared, the keys kept: every string asleep and silent, as
    /// `clear` leaves them, but a key that is down is still down, and if one
    /// is, every fed string listens again as at its strike. The frame fades
    /// the harp out before this (all notes off), and a key struck during the
    /// fade would otherwise have lost its strings until the next strike.
    pub fn hush(&mut self) {
        for i in 0..LOOPS {
            self.sleep(i);
        }
        self.n_live = 0;
        self.phase = 0;
        self.feed.clear();
        self.drive_acc = 0.0;
        self.drive_ms = 0.0;
        self.drive_present = false;
        if self.loops[..KEYS].iter().any(|l| l.held) {
            self.drive_present = true;
            for i in 0..LOOPS {
                self.rouse(i, true);
            }
        }
    }

    /// One sample: feed the bridge sum to every fed string and take back
    /// what they carry. Exactly one sample of `render`.
    #[allow(dead_code)] // the frame renders the harp a block at a time (`render`); the tests tick it
    pub fn tick(&mut self, drive: f32) -> f32 {
        let mut out = [0.0_f32];
        self.render(&[drive], &mut out);
        out[0]
    }

    /// A block: feed `drive` to every fed string and ADD what they carry
    /// into `out`. String by string rather than sample by sample, so each
    /// loop's state stays in registers for the block; the sum each output
    /// sample collects is in the same order either way, so a block is
    /// bit-identical to its samples ticked one at a time
    /// (`a_block_is_its_samples`). Costs the feed's filters and a sum of
    /// squares while nothing ticks.
    pub fn render(&mut self, drive: &[f32], out: &mut [f32]) {
        self.render_piece(drive, out, true);
    }

    /// **`render` for a piece of a block**, which `ends` its block or is
    /// one a host's call cut short: a gliding string's line steps, and its
    /// arrival is judged, where a run ends (`Glide::cool`), which is the
    /// harp's frame's end or the block's, and not a cut. `render` is a
    /// piece that ends its block.
    pub fn render_piece(&mut self, drive: &[f32], out: &mut [f32], ends: bool) {
        if self.gliding {
            self.render_gliding(drive, out, ends);
            return;
        }
        let n = drive.len().min(out.len());
        let mut at = 0;
        while at < n {
            let m = (FRAME - self.phase).min(n - at);
            self.render_part(&drive[at..at + m], &mut out[at..at + m]);
            self.phase += m;
            at += m;
            if self.phase == FRAME {
                self.phase = 0;
                self.frame();
            }
        }
    }

    /// `render` while any string glides to a re-tuned design: the same block,
    /// a part at a time through `render_part_gliding`. Apart, and tested once
    /// a block, because a test in `render`'s loop or `render_part`'s, never
    /// taken, compiled them into 5 to 7 % more instructions and cycles (a
    /// held pedalled chord at 16-frame calls, and the stress score).
    #[inline(never)]
    #[cold]
    fn render_gliding(&mut self, drive: &[f32], out: &mut [f32], ends: bool) {
        let n = drive.len().min(out.len());
        let mut at = 0;
        while at < n {
            let m = (FRAME - self.phase).min(n - at);
            let end = self.phase + m == FRAME || (at + m == n && ends);
            self.render_part_gliding(&drive[at..at + m], &mut out[at..at + m], end);
            self.phase += m;
            at += m;
            if self.phase == FRAME {
                self.phase = 0;
                self.frame();
            }
        }
    }

    /// At most the rest of one frame.
    fn render_part(&mut self, drive: &[f32], out: &mut [f32]) {
        let m = drive.len();
        let send = self.send;
        for (u, &d) in self.u.iter_mut().zip(drive) {
            // Finite, and bounded far over anything a voice puts out, so
            // nothing in a stable loop can overflow (`Hot`).
            let d = if d.is_finite() { d.clamp(-DRIVE_MAX, DRIVE_MAX) } else { 0.0 };
            self.drive_acc += d * d;
            *u = d * send;
        }
        self.feed.run(&self.u[..m]);
        if self.n_live == 0 {
            return;
        }
        self.ticks += (self.n_live * m) as u64;
        let Harp { loops, arena, live, n_live, ramp, tail, feed, .. } = self;
        let (ramp, tail) = (*ramp, *tail);
        let mut i = 0;
        while i + 1 < *n_live {
            let (ia, ib) = (usize::from(live[i]), usize::from(live[i + 1]));
            // Two loops and their two lines, disjoint: split each pair of
            // borrows at the later one.
            let (la, lb) = if ia < ib {
                let (lo, hi) = loops.split_at_mut(ib);
                (&mut lo[ia], &mut hi[0])
            } else {
                let (lo, hi) = loops.split_at_mut(ia);
                (&mut hi[0], &mut lo[ib])
            };
            let (oa, na, ob, nb) = (la.off as usize, la.n as usize, lb.off as usize, lb.n as usize);
            let (ba, bb) = if oa < ob {
                let (lo, hi) = arena.split_at_mut(ob);
                (&mut lo[oa..oa + na], &mut hi[..nb])
            } else {
                let (lo, hi) = arena.split_at_mut(oa);
                (&mut hi[..na], &mut lo[ob..ob + nb])
            };
            let (xa, xb) = (feeds(feed, la, m), feeds(feed, lb, m));
            let (mut ha, mut hb) = (la.hot(), lb.hot());
            if la.ramping || lb.ramping {
                run_two::<true>(&mut ha, ba, xa, &mut hb, bb, xb, out, ramp, tail);
            } else {
                run_two::<false>(&mut ha, ba, xa, &mut hb, bb, xb, out, ramp, tail);
            }
            la.cool(&ha, ba);
            lb.cool(&hb, bb);
            i += 2;
        }
        if i < *n_live {
            let l = &mut loops[usize::from(live[i])];
            let buf = &mut arena[l.off as usize..(l.off + l.n) as usize];
            let [x, xs] = feeds(feed, l, m);
            let mut h = l.hot();
            if l.ramping {
                run_one::<true>(&mut h, buf, x, xs, out, ramp, tail);
            } else {
                run_one::<false>(&mut h, buf, x, xs, out, ramp, tail);
            }
            l.cool(&h, buf);
        }
    }

    /// `render_part` while any string glides to a re-tuned design
    /// (`render_gliding`): the same part, its pairs as there, and a string
    /// that glides on its own (`run_alone`), so that a pair is the pair it
    /// always was (`Glide`).
    fn render_part_gliding(&mut self, drive: &[f32], out: &mut [f32], end: bool) {
        let m = drive.len();
        let send = self.send;
        for (u, &d) in self.u.iter_mut().zip(drive) {
            // As `render_part`'s.
            let d = if d.is_finite() { d.clamp(-DRIVE_MAX, DRIVE_MAX) } else { 0.0 };
            self.drive_acc += d * d;
            *u = d * send;
        }
        self.feed.run(&self.u[..m]);
        if self.n_live == 0 {
            return;
        }
        self.ticks += (self.n_live * m) as u64;
        let Harp { loops, arena, live, n_live, ramp, tail, glide, glides, feed, .. } = self;
        let (ramp, tail, glide) = (*ramp, *tail, *glide);
        let mut i = 0;
        while i + 1 < *n_live {
            let (ia, ib) = (usize::from(live[i]), usize::from(live[i + 1]));
            if glides[ia].on || glides[ib].on {
                for j in [ia, ib] {
                    let l = &mut loops[j];
                    let buf = &mut arena[l.off as usize..(l.off + l.n) as usize];
                    run_alone(l, &mut glides[j], buf, feed, m, out, ramp, tail, glide, end);
                }
                i += 2;
                continue;
            }
            // Two loops and their two lines, disjoint: split each pair of
            // borrows at the later one.
            let (la, lb) = if ia < ib {
                let (lo, hi) = loops.split_at_mut(ib);
                (&mut lo[ia], &mut hi[0])
            } else {
                let (lo, hi) = loops.split_at_mut(ia);
                (&mut hi[0], &mut lo[ib])
            };
            let (oa, na, ob, nb) = (la.off as usize, la.n as usize, lb.off as usize, lb.n as usize);
            let (ba, bb) = if oa < ob {
                let (lo, hi) = arena.split_at_mut(ob);
                (&mut lo[oa..oa + na], &mut hi[..nb])
            } else {
                let (lo, hi) = arena.split_at_mut(oa);
                (&mut hi[..na], &mut lo[ob..ob + nb])
            };
            let (xa, xb) = (feeds(feed, la, m), feeds(feed, lb, m));
            let (mut ha, mut hb) = (la.hot(), lb.hot());
            if la.ramping || lb.ramping {
                run_two::<true>(&mut ha, ba, xa, &mut hb, bb, xb, out, ramp, tail);
            } else {
                run_two::<false>(&mut ha, ba, xa, &mut hb, bb, xb, out, ramp, tail);
            }
            la.cool(&ha, ba);
            lb.cool(&hb, bb);
            i += 2;
        }
        if i < *n_live {
            let j = usize::from(live[i]);
            let l = &mut loops[j];
            let buf = &mut arena[l.off as usize..(l.off + l.n) as usize];
            run_alone(l, &mut glides[j], buf, feed, m, out, ramp, tail, glide, end);
        }
    }

    /// The gate, once a frame. Decides who sleeps, ends the felt's bite
    /// where it has run its time, and rebuilds the live list. Inlined into
    /// `render` as it always was (`run_one` says why it is asked for now).
    #[inline(always)]
    fn frame(&mut self) {
        if self.gliding {
            self.gliding = self.glides.iter().any(|g| g.on);
        }
        self.feed.rest_if_quiet();
        let inv = 1.0 / FRAME as f32;
        self.drive_ms = self.drive_acc * inv;
        self.drive_acc = 0.0;
        self.drive_present = self.drive_ms > DRIVE_QUIET_RMS * DRIVE_QUIET_RMS;
        if self.n_live == 0 {
            // Nothing to judge: pedal up and nothing ringing is a counter.
            return;
        }
        let floor = (FLOOR_RMS * FLOOR_RMS).max(REL_SLEEP * self.drive_ms * self.send * self.send);
        let p = self.pressure;
        let mut n_live = 0usize;
        for i in 0..LOOPS {
            let l = &mut self.loops[i];
            if l.state == State::Sleeping {
                continue;
            }
            if l.probation > 0 {
                l.probation -= 1;
            }
            if l.land > 0 {
                l.land -= 1;
                if l.land == 0 {
                    l.settle(p);
                }
            }
            let ms = l.acc * inv;
            l.acc = 0.0;
            let contribution = ms * l.cout * l.cout;
            // **Judged over more than a period.** A string's energy passes
            // its read once a period, and a low string's period is longer
            // than a frame (A0's 1750 samples at 48 kHz are seven): judged
            // on one frame, an impulse-rung A0 read silent between its
            // arrivals and was cleared mid-ring (verifier finding 11).
            l.quiet = if contribution < floor { l.quiet.saturating_add(1) } else { 0 };
            let shut = !l.fed() && l.cin < 1.0e-9 && l.cis < 1.0e-9;
            if l.quiet >= l.quiet_frames && (l.probation == 0 || shut) {
                // Rung out, or never answering: clearing it is clearing
                // silence.
                self.sleep(i);
                continue;
            }
            self.live[n_live] = i as u8;
            n_live += 1;
        }
        self.n_live = n_live;
    }
}

#[cfg(test)]
mod tests {
    use super::super::grand::GrandVoice;
    use super::super::modal::BLOCK;
    use super::super::string::loop_phase_delay;
    use super::super::PianoKnobs;
    use super::*;

    const RATE: f32 = 48_000.0;

    /// Hann-windowed Goertzel magnitude — `analyze::goertzel_hann` from
    /// the bench, which is read-only, copied here because a rectangular
    /// window lets the fundamental leak a floor under every partial.
    fn goertzel_hann(x: &[f32], freq: f32, rate: f32) -> f32 {
        if x.is_empty() || freq <= 0.0 || freq >= rate * 0.5 {
            return 0.0;
        }
        // In f64, unlike the bench's: the recurrence's 2 cos w sits at
        // 1.99998 for a 32 Hz string and f32 loses three cents to it.
        let w = f64::from(std::f32::consts::TAU) * f64::from(freq) / f64::from(rate);
        let c = 2.0 * w.cos();
        let n = x.len() as f64;
        let step = std::f64::consts::TAU / n;
        let (mut s1, mut s2) = (0.0_f64, 0.0_f64);
        for (i, &v) in x.iter().enumerate() {
            let win = 0.5 - 0.5 * (step * i as f64).cos();
            let s0 = f64::from(v) * win + c * s1 - s2;
            s2 = s1;
            s1 = s0;
        }
        ((s1 * s1 + s2 * s2 - c * s1 * s2).max(0.0).sqrt() / (n * 0.25)) as f32
    }

    fn db(v: f32) -> f32 {
        20.0 * v.max(1e-12).log10()
    }

    fn rms(x: &[f32]) -> f32 {
        if x.is_empty() {
            return 0.0;
        }
        let e: f64 = x.iter().map(|s| f64::from(*s) * f64::from(*s)).sum();
        (e / x.len() as f64).sqrt() as f32
    }

    /// The strongest magnitude within `span` (fraction) of `around`, and
    /// where it was: a partial is searched for, never pinned, because
    /// dispersion moves it.
    fn search_peak(x: &[f32], around: f32, span: f32, rate: f32) -> (f32, f32) {
        let mut best = (0.0_f32, around);
        let mut f = around * (1.0 - span);
        let step = around * span * 0.01;
        while f < around * (1.0 + span) {
            let m = goertzel_hann(x, f, rate);
            if m > best.0 {
                best = (m, f);
            }
            f += step;
        }
        best
    }

    /// Search wide then fine.
    fn locate(x: &[f32], around: f32, rate: f32) -> (f32, f32) {
        let (_, f) = search_peak(x, around, 0.01, rate);
        let (_, f) = search_peak(x, f, 0.0008, rate);
        search_peak(x, f, 0.00008, rate)
    }

    fn cents(f: f32, re: f32) -> f32 {
        1200.0 * (f / re).log2()
    }

    fn secs(s: f32) -> usize {
        (s * RATE) as usize
    }

    fn intimate() -> StringSet {
        StringSet::grand(&GrandBase::INTIMATE, 1.0, &GrandHarp::INTIMATE)
    }

    /// The two grands' sets at their sends, as the frame strings them.
    fn shipped() -> [(&'static str, GrandBase, StringSet); 2] {
        [
            ("intimate", GrandBase::INTIMATE, StringSet::grand(&GrandBase::INTIMATE, 0.4, &GrandHarp::INTIMATE)),
            ("concert", GrandBase::CONCERT, StringSet::grand(&GrandBase::CONCERT, 1.0, &GrandHarp::CONCERT)),
        ]
    }

    /// Ring one loop on its own: a DC-free doublet into its line, then
    /// `samples` of its raw signal through the real `step`, free loss or
    /// damped. The gate and the coupling are bypassed — this is the
    /// string, not the harp.
    fn ring_loop(h: &mut Harp, i: usize, samples: usize, damped: bool) -> Vec<f32> {
        let (off, n) = (h.loops[i].off as usize, h.loops[i].n as usize);
        h.sleep(i);
        {
            let l = &mut h.loops[i];
            l.gain = if damped { l.gain_damped } else { l.gain_free };
            l.gain_to = l.gain;
            l.cin = 0.0;
            l.cin_to = 0.0;
            l.cis = 0.0;
            l.cis_to = 0.0;
            l.ramping = false;
        }
        // A hundred, not one: a doublet's spectrum rises with frequency
        // from a zero at DC, and at unity a bass string's second partial
        // would sit under the floor the partial tables measure against.
        h.arena[off] = 100.0;
        h.arena[off + 1] = -100.0;
        h.loops[i].write = 2;
        // Heard through -1 so `out` is the loop's own signal (`HEARD`).
        h.loops[i].cout = -1.0;
        h.loops[i].cout_to = -1.0;
        let x = vec![0.0_f32; samples];
        let mut out = vec![0.0_f32; samples];
        let (l, buf) = (&mut h.loops[i], &mut h.arena[off..off + n]);
        let mut hot = l.hot();
        run_one::<false>(&mut hot, buf, &x, &x, &mut out, 0.0, 0.0);
        l.cool(&hot, buf);
        h.sleep(i);
        out
    }

    /// The decay of the component near `f` in `x`, T60 in seconds, from
    /// its level in two windows `gap` apart.
    fn t60_between(x: &[f32], f: f32, rate: f32, a: f32, gap: f32, win: f32) -> f32 {
        let i = |t: f32| (t * rate) as usize;
        let (m1, f1) = locate(&x[i(a)..i(a + win)], f, rate);
        let (m2, _) = search_peak(&x[i(a + gap)..i(a + gap + win)], f1, 0.0005, rate);
        let drop = db(m1) - db(m2);
        60.0 * gap / drop.max(1e-6)
    }

    /// Play the grand into a harp, block by block: keys (pitch, velocity
    /// MIDI, strike s, release s), the pedal's transitions, `total` s.
    /// Returns the voices' mono output, the voices' drive and the harp's
    /// return. A velocity under the grand's let-off is a silent press, and
    /// the harp hears its `voice_done` at the next block, as the frame
    /// reports it.
    fn play(set: &StringSet, base: &GrandBase, keys: &[(u8, u8, f32, f32)], pedal: &[(f32, bool)], total: f32) -> (Vec<f32>, Vec<f32>, Vec<f32>) {
        let (l, r, drive, harp) = play_lr(set, base, keys, pedal, total);
        (l.iter().zip(&r).map(|(a, b)| 0.5 * (a + b)).collect(), drive, harp)
    }

    /// `play`, with the two microphones apart: (left, right, drive, harp).
    fn play_lr(set: &StringSet, base: &GrandBase, keys: &[(u8, u8, f32, f32)], pedal: &[(f32, bool)], total: f32) -> (Vec<f32>, Vec<f32>, Vec<f32>, Vec<f32>) {
        let mut h = Harp::new(RATE);
        h.string(set);
        let mut voices: Vec<(GrandVoice, u8, bool)> = keys.iter().map(|k| (GrandVoice::new(RATE), k.0, false)).collect();
        let knobs = PianoKnobs::default();
        let n = secs(total);
        let (mut left, mut right, mut drive, mut harp) = (vec![0.0; n], vec![0.0; n], vec![0.0; n], vec![0.0; n]);
        let (mut l, mut r, mut d) = ([0.0_f32; BLOCK], [0.0_f32; BLOCK], [0.0_f32; BLOCK]);
        let mut at = 0;
        let mut down = false;
        while at < n {
            let m = BLOCK.min(n - at);
            let t0 = at as f32 / RATE;
            let t1 = (at + m) as f32 / RATE;
            for &(when, p) in pedal {
                if when >= t0 && when < t1 && p != down {
                    down = p;
                    h.set_pedal(p);
                    if !p {
                        for (v, k, _) in voices.iter_mut() {
                            let held = keys.iter().any(|q| q.0 == *k && q.2 <= t0 && q.3 > t0);
                            if !held {
                                v.damp();
                            }
                        }
                    }
                }
            }
            for (j, &(key, vel, on, off)) in keys.iter().enumerate() {
                if on >= t0 && on < t1 {
                    voices[j].0.strike(key, f32::from(vel) / 127.0, &knobs, base);
                    voices[j].2 = true;
                    h.key_down(key);
                }
                if off >= t0 && off < t1 {
                    voices[j].0.release(down);
                    h.key_up(key);
                }
            }
            l.fill(0.0);
            r.fill(0.0);
            d.fill(0.0);
            for (v, _, _) in voices.iter_mut() {
                v.render(&mut l[..m], &mut r[..m], &mut d[..m]);
            }
            let mut hb = [0.0_f32; BLOCK];
            h.render(&d[..m], &mut hb[..m]);
            left[at..at + m].copy_from_slice(&l[..m]);
            right[at..at + m].copy_from_slice(&r[..m]);
            drive[at..at + m].copy_from_slice(&d[..m]);
            harp[at..at + m].copy_from_slice(&hb[..m]);
            // The frame's recount: a voice that finished hands its string
            // back — once the key has no other voice sounding (the frame
            // keeps one voice per key; this harness one per strike).
            recount(&mut voices, &mut h);
            at += m;
        }
        (left, right, drive, harp)
    }

    /// Hand a finished voice's string back to the harp, as the frame's
    /// recount does: only when no other voice of that key still sounds.
    fn recount(voices: &mut [(GrandVoice, u8, bool)], h: &mut Harp) {
        for j in 0..voices.len() {
            if voices[j].2 && voices[j].0.finished() {
                voices[j].2 = false;
                let key = voices[j].1;
                if !voices.iter().any(|(v, k, live)| *live && *k == key && !v.finished()) {
                    h.voice_done(key);
                }
            }
        }
    }

    /// The note, the harp, and the note's level over a window, mono.
    fn level(x: &[f32], a: f32, b: f32) -> f32 {
        rms(&x[secs(a)..secs(b)])
    }

    /// Power in octave bands from 62.5 Hz to 8 kHz over `[a, b)` s: a
    /// Hann-windowed FFT summed per band (an in-test radix-2 FFT; the
    /// bench's analysis tools are Python).
    fn octave_bands(x: &[f32], a: f32, b: f32) -> [f64; 7] {
        let seg = &x[secs(a)..secs(b)];
        let n = seg.len().next_power_of_two();
        let mut re: Vec<f64> = vec![0.0; n];
        let mut im: Vec<f64> = vec![0.0; n];
        for (i, &v) in seg.iter().enumerate() {
            let w = 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / seg.len() as f64).cos();
            re[i] = f64::from(v) * w;
        }
        // Bit-reversed order, then the butterflies.
        let bits = n.trailing_zeros();
        for i in 0..n {
            let j = i.reverse_bits() >> (usize::BITS - bits);
            if j > i {
                re.swap(i, j);
                im.swap(i, j);
            }
        }
        let mut len = 2;
        while len <= n {
            let ang = -std::f64::consts::TAU / len as f64;
            for start in (0..n).step_by(len) {
                for k in 0..len / 2 {
                    let (s, c) = (ang * k as f64).sin_cos();
                    let (i, j) = (start + k, start + k + len / 2);
                    let (tr, ti) = (re[j] * c - im[j] * s, re[j] * s + im[j] * c);
                    re[j] = re[i] - tr;
                    im[j] = im[i] - ti;
                    re[i] += tr;
                    im[i] += ti;
                }
            }
            len *= 2;
        }
        let mut out = [0.0_f64; 7];
        for (k, (r, i)) in re.iter().zip(&im).enumerate().take(n / 2) {
            let f = k as f64 * f64::from(RATE) / n as f64;
            let band = (f / 62.5).log2().floor();
            if (0.0..7.0).contains(&band) {
                out[band as usize] += r * r + i * i;
            }
        }
        out
    }

    /// **The phase formulas are the frozen ones.** The loop is tuned from
    /// phase delays this file computes in f64; at f32 resolution they are
    /// `string::loop_phase_delay`'s, section for section.
    #[test]
    fn the_phase_delays_are_the_frozen_formulas() {
        for &(w, p, a) in &[(0.034_f32, 0.3_f32, -0.5_f32), (0.2, 0.6, -0.8), (0.9, 0.1, -0.2), (0.5, 0.5, 0.0)] {
            let (wd, pd, ad) = (f64::from(w), f64::from(p), f64::from(a));
            let mine = lp_delay(wd, pd) + 4.0 * ap_delay(wd, ad);
            let theirs = f64::from(loop_phase_delay(w, p, a));
            assert!((mine - theirs).abs() < 2e-4 * theirs.abs().max(1.0), "w {w} p {p} a {a}: {mine} against {theirs}");
        }
    }

    /// **Every string is in tune**, at the pitch its string set asks for:
    /// the stretched grand's, a cent sharp. Rung alone and measured, every
    /// third key A0 to C8, at 44.1, 48 and 96 kHz.
    #[test]
    fn every_string_is_in_tune() {
        let set = intimate();
        for rate in [44_100.0_f32, 48_000.0, 96_000.0] {
            let mut h = Harp::new(rate);
            h.string(&set);
            let mut worst = (0.0_f32, 0u8);
            for key in (21..=108).step_by(3) {
                let i = usize::from(key - FIRST_KEY);
                let f0 = set.keys[i].f0;
                let x = ring_loop(&mut h, i, (0.6 * rate) as usize, false);
                let (_, f) = locate(&x, f0, rate);
                let c = cents(f, f0);
                if c.abs() > worst.0.abs() {
                    worst = (c, key);
                }
                assert!(c.abs() < 0.5, "key {key} at {rate} Hz: fundamental {f:.3} Hz is {c:+.2} cents off {f0:.3}");
            }
            println!("tuning at {rate} Hz, keys 21..108 by 3: worst {:+.3} cents at key {}", worst.0, worst.1);
        }
    }

    /// **Every string rings at its designed T60**, free and damped, at
    /// every key and every rate. What the Thiran bought: the linear read's
    /// loss per pass at frequency w with fraction f is
    /// `sqrt(1 - 2 f (1 - f)(1 - cos w))`, and it is printed beside the
    /// measurement for the same loop, so the fix is shown and not assumed.
    #[test]
    fn every_string_rings_at_its_designed_t60() {
        for (name, set) in [("intimate", intimate()), ("concert", StringSet::grand(&GrandBase::CONCERT, 1.0, &GrandHarp::CONCERT))] {
            for rate in [44_100.0_f32, 48_000.0, 96_000.0] {
                let mut h = Harp::new(rate);
                h.string(&set);
                let mut worst = 0.0_f32;
                let mut line = String::new();
                for key in (21..=108).step_by(4) {
                    let i = usize::from(key - FIRST_KEY);
                    let s = set.keys[i];
                    for damped in [false, true] {
                        // A felt never makes a string ring longer: above
                        // about E5 the reference's damped decay is at or
                        // over the free one, and the design holds it there.
                        let want = if damped { s.t60_damped.min(s.t60) } else { s.t60 };
                        // Two windows a third of the T60 apart (20 dB of
                        // decay), each long enough to resolve the fundamental.
                        let win = (8.0 / s.f0).max(0.02);
                        let gap = want / 3.0;
                        let x = ring_loop(&mut h, i, ((0.05 + gap + win) * rate) as usize + 16, damped);
                        let got = t60_between(&x, s.f0, rate, 0.05, gap, win);
                        let err = got / want - 1.0;
                        worst = worst.max(err.abs());
                        if !damped && (key - 21) % 12 == 0 {
                            // The old read, at this loop's own fraction: its
                            // loss alone, as a T60 at the fundamental.
                            let w = std::f32::consts::TAU * s.f0 / rate;
                            let total = rate / s.f0;
                            let f = total - total.floor();
                            let lin = (1.0 - 2.0 * f * (1.0 - f) * (1.0 - w.cos())).sqrt();
                            let lin_t60 = if lin < 1.0 { -3.0 / (s.f0 * lin.log10()) } else { f32::INFINITY };
                            line.push_str(&format!(" {key}:{got:.2}/{want:.2}s (lin read alone {lin_t60:.1}s)"));
                        }
                        assert!(err.abs() < 0.08, "{name} key {key} at {rate} Hz, {}: T60 {got:.3} s against {want:.3} s", if damped { "damped" } else { "free" });
                    }
                }
                println!("{name} T60 at {rate} Hz, measured/designed:{line}; worst error {:.1} %", 100.0 * worst);
            }
        }
    }

    /// **The harp's partials sit on the grand's.** Each loop's partials,
    /// rung and located, against `n F sqrt(1 + B n²)` for the grand's own
    /// B and stretched pitch (the detune is the string's own, so it cancels).
    #[test]
    fn the_harps_partials_sit_on_the_grands() {
        let set = intimate();
        for rate in [44_100.0_f32, 48_000.0, 96_000.0] {
            let mut h = Harp::new(rate);
            h.string(&set);
            println!("{rate} Hz: key   n2    n3    n4    n6    n8   (harp - grand series, cents)");
            for key in [21u8, 24, 28, 36, 48, 60, 72, 84, 96] {
                let i = usize::from(key - FIRST_KEY);
                let s = set.keys[i];
                let x = ring_loop(&mut h, i, (0.8 * rate) as usize, false);
                let big_f = s.f0 / (1.0 + s.b).sqrt();
                let mut line = format!("{key:3} ");
                for n in [2.0_f32, 3.0, 4.0, 6.0, 8.0] {
                    let fnn = n * big_f * (1.0 + s.b * n * n).sqrt();
                    if fnn > 0.35 * rate {
                        line.push_str("    - ");
                        continue;
                    }
                    let (m, f) = locate(&x, fnn, rate);
                    let c = cents(f, fnn);
                    line.push_str(&format!(" {c:+5.1}"));
                    // The partials a sympathetic string is lit and heard at,
                    // within a cent and a half wherever they are carried: the
                    // second and third up to G5, the second up to C7 (fitted
                    // there; C6's third is -1.7). A0's one section is capped
                    // (`AP_TAU_MAX_SECS`) and its eighth lands 4 cents flat.
                    let judged = (n <= 3.0 && key <= 79) || (n == 2.0 && key <= 96);
                    if db(m) > -60.0 && judged && key >= 24 {
                        assert!(c.abs() < 1.5, "key {key} partial {n} at {rate} Hz: {c:+.2} cents off the grand's series");
                    }
                    // And the eighth of C2 and C3, which answer the middle of
                    // the keyboard: within a cent and a half at every rate.
                    if n == 8.0 && (key == 36 || key == 48) {
                        assert!(c.abs() < 1.5, "key {key} partial 8 at {rate} Hz: {c:+.2} cents");
                    }
                }
                println!("{line}");
            }
        }
    }

    /// **A block is its samples.** The same drive rendered a block at a
    /// time and ticked a sample at a time gives the same output to the
    /// bit, through strikes, the pedal (switched and half way) and the gate.
    #[test]
    fn a_block_is_its_samples() {
        let set = intimate();
        let mut a = Harp::new(RATE);
        let mut b = Harp::new(RATE);
        a.string(&set);
        b.string(&set);
        let f0 = f_equal(60);
        let drive = |i: usize| {
            let t = i as f32 / RATE;
            (1..=6).map(|n| (std::f32::consts::TAU * f0 * n as f32 * t).sin() / n as f32).sum::<f32>() * 0.1
        };
        let n = secs(1.5);
        let mut out_a = vec![0.0_f32; n];
        let mut out_b = vec![0.0_f32; n];
        let mut at = 0;
        let mut evs = 0;
        while at < n {
            let m = BLOCK.min(n - at);
            for h in [&mut a, &mut b] {
                match evs {
                    0 => h.set_pedal(true),
                    3 => h.key_down(60),
                    90 => h.key_up(60),
                    150 => h.set_sustain(0.4),
                    200 => h.set_pedal(false),
                    300 => h.key_down(48),
                    _ => {}
                }
            }
            evs += 1;
            let d: Vec<f32> = (at..at + m).map(drive).collect();
            a.render(&d, &mut out_a[at..at + m]);
            for (k, &dk) in d.iter().enumerate() {
                out_b[at + k] = b.tick(dk);
            }
            at += m;
        }
        let same = out_a.iter().zip(&out_b).all(|(x, y)| x.to_bits() == y.to_bits());
        assert!(same, "a block and its samples differ");
        assert!(out_a.iter().any(|v| v.abs() > 1e-6), "the harp said nothing");
    }

    /// **A designed set loaded is the set strung.** `Strung::new` once and
    /// `Harp::load` (the frame's path, a copy) gives the same harp to the
    /// bit as `Harp::string` designing it on the spot, through a pedalled
    /// passage; and loading takes a small fraction of stringing.
    #[test]
    fn a_loaded_set_is_the_set_strung() {
        let set = StringSet::grand(&GrandBase::CONCERT, 1.0, &GrandHarp::CONCERT);
        let mut a = Harp::new(RATE);
        let mut b = Harp::new(RATE);
        a.string(&set);
        let s = b.strung(&set);
        b.load(&s);
        let f0 = f_equal(55);
        let n = secs(1.0);
        let mut ya = vec![0.0_f32; n];
        let mut yb = vec![0.0_f32; n];
        for h in [&mut a, &mut b] {
            h.set_pedal(true);
            h.key_down(55);
        }
        let d: Vec<f32> = (0..n).map(|i| (std::f32::consts::TAU * f0 * i as f32 / RATE).sin() * 0.1 * (-(i as f32) / RATE).exp()).collect();
        for (k, c) in d.chunks(BLOCK).enumerate() {
            a.render(c, &mut ya[k * BLOCK..k * BLOCK + c.len()]);
            b.render(c, &mut yb[k * BLOCK..k * BLOCK + c.len()]);
        }
        assert!(ya.iter().zip(&yb).all(|(x, y)| x.to_bits() == y.to_bits()), "a loaded set rang differently from the set strung");
        assert!(ya.iter().any(|v| v.abs() > 1e-6));
    }

    /// **A string that is not a number is a string the harp does not
    /// have.** NaN and infinity in each of a string's fields, one string at
    /// a time and all of them in one set: stringing does not panic (it did,
    /// on the audio thread, for a NaN `f0` or `t60`: `f64::clamp` with a NaN
    /// bound; verifier finding 7), every coefficient designed is finite, a
    /// string whose pitch or decay is not a number never ticks, and the harp
    /// rings finite.
    #[test]
    fn a_string_that_is_not_a_number_is_not_strung() {
        let mut set = intimate();
        let bad = [f32::NAN, f32::INFINITY, f32::NEG_INFINITY];
        for (j, s) in set.keys.iter_mut().enumerate().take(36) {
            let v = bad[j % 3];
            match j / 4 {
                0 => s.f0 = v,
                1 => s.t60 = v,
                2 => s.b = v,
                3 => s.t60_hi = v,
                4 => s.f_hi = v,
                5 => s.t60_damped = v,
                6 => s.couple = v,
                7 => s.own = v,
                _ => {
                    s.f0 = v;
                    s.t60 = v;
                    s.couple = v;
                }
            }
        }
        set.aliquots[3].f0 = f32::NAN;
        let mut h = Harp::new(RATE);
        h.string(&set);
        for l in &h.loops {
            for v in [l.gain_free, l.gain_damped, l.gain_halo, l.gain_land, l.frac.a, l.disp.a, l.loss.a, l.norm, l.couple, l.own] {
                assert!(v.is_finite(), "a coefficient designed non-finite");
            }
        }
        h.set_pedal(true);
        h.key_down(30);
        for i in [0usize, 1, 2, 3, 4, 5, 6, 7, 32, 33, 34, 35, KEYS + 3] {
            assert_eq!(h.loops[i].state, State::Sleeping, "string {i}, not a number, was woken");
        }
        let mut out = vec![0.0_f32; secs(0.5)];
        let d: Vec<f32> = (0..out.len()).map(|i| (i as f32 * 0.37).sin() * 0.1).collect();
        h.render(&d, &mut out);
        assert!(out.iter().all(|v| v.is_finite()), "the harp went non-finite");
        assert!(out.iter().any(|v| v.abs() > 1e-7), "the harp said nothing");
    }

    /// **The struck string's halo (item 3), flat across its partials.** A
    /// held note, pedal up, on each grand at its own send: the harp under
    /// it, against the note, over the hold; its first half second and its
    /// last; the note with it against the note; and, on the C2, partial by
    /// partial. The references' (exact subtraction of a render with the
    /// bank off, L/R power average, the release score): intimate C2 -27.7,
    /// C4 -33.3 to -33.8, C5 -35.0 dB; concert C2 -19.7, C4 -23.3 to
    /// -25.5, C5 -23.9; partial by partial at C2 (1.0-2.5 s) the intimate's
    /// n1 -24, n2 -26, n4 -35, n8 -35, n12 -28, n16 -40, the concert's -15,
    /// -16, -23, -18, -13, -28: no climb with the partial number. The
    /// first-difference feed's halo climbed from n1 -44 to n48 -15 dB and
    /// crept up 7 dB over a two-second hold on a grand whose upper partials
    /// ring longer (verifier findings 1 and 3). The note with its halo: the
    /// references' +0.04 to +0.18 dB over the note alone; here +0.1 to +0.2
    /// on the intimate and +0.4 to +0.5 at the concert's C2, whose halo is
    /// 8 dB wider and adds nearly in phase. Measured: printed.
    #[test]
    fn a_held_note_carries_its_own_halo_and_does_not_swell() {
        for (name, base, set) in shipped() {
            let wants: [(u8, f32, f32); 3] = if name == "intimate" { [(36, 3.0, -27.7), (60, 2.0, -33.5), (72, 1.0, -35.0)] } else { [(36, 3.0, -19.7), (60, 2.0, -23.5), (72, 1.0, -23.9)] };
            for (key, held, want) in wants {
                let (mono, _, harp) = play(&set, &base, &[(key, 80, 0.05, 0.05 + held)], &[], 0.05 + held + 0.5);
                let end = 0.05 + held;
                let halo = db(level(&harp, 0.07, end) / level(&mono, 0.07, end));
                let first = db(level(&harp, 0.07, 0.57) / level(&mono, 0.07, 0.57));
                let last = db(level(&harp, end - 0.5, end) / level(&mono, end - 0.5, end));
                let with: Vec<f32> = mono.iter().zip(&harp).map(|(a, b)| a + b).collect();
                let swell = db(level(&with, end - 0.5, end) / level(&mono, end - 0.5, end));
                let mut parts = String::new();
                if key == 36 {
                    let (a, b) = (secs(1.05), secs(2.55));
                    let f0 = set.keys[usize::from(key - FIRST_KEY)].f0;
                    let bb = set.keys[usize::from(key - FIRST_KEY)].b;
                    let mut lo = Vec::new();
                    let mut hi = Vec::new();
                    for n in [1.0_f32, 2.0, 4.0, 8.0, 12.0, 16.0] {
                        let fnn = n * f0 / (1.0 + bb).sqrt() * (1.0 + bb * n * n).sqrt();
                        let (vm, fv) = search_peak(&mono[a..b], fnn, 0.004, RATE);
                        let hm = goertzel_hann(&harp[a..b], fv, RATE);
                        let rel = db(hm) - db(vm);
                        parts.push_str(&format!(" n{n}:{rel:+.0}"));
                        if n <= 2.0 {
                            lo.push(rel);
                        } else if n >= 8.0 {
                            hi.push(rel);
                        }
                        assert!((-52.0..=-10.0).contains(&rel), "{name} C2 partial {n}: the halo sits {rel:+.1} dB re the partial");
                    }
                    let climb = hi.iter().sum::<f32>() / hi.len() as f32 - lo.iter().sum::<f32>() / lo.len() as f32;
                    parts.push_str(&format!(" (n8-n16 over n1-n2 {climb:+.1} dB)"));
                    assert!(climb < 6.0, "{name}: the C2 halo climbs {climb:+.1} dB from its first partials to n8-n16");
                }
                println!("{name} key {key} held {held} s: halo {halo:+.1} dB re note (reference {want:+.1}); first 0.5 s {first:+.1}, last 0.5 s {last:+.1}; note+halo over note at the end {swell:+.2} dB;{parts}");
                assert!((halo - want).abs() < 2.5, "{name} key {key}: halo {halo:+.1} dB against the reference's {want:+.1}");
                assert!((last - first).abs() < 2.0, "{name} key {key}: the halo moved {:+.1} dB over the hold", last - first);
                assert!((-0.2..0.75).contains(&swell), "{name} key {key}: the note with its halo is {swell:+.2} dB off the note");
            }
        }
    }

    /// **A released note leaves its reference's tail, and the tail joins
    /// it (item 4).** C4 held half a second and two seconds: the
    /// fundamental (searched, note and harp, mono) at +0.5 s under its own
    /// level at key-off, and its fall over the next half second. The intimate
    /// reference's C4 fundamental stands 60 to 63 dB under its key-off level
    /// at +0.5 s and falls 30 to 32 dB/s (its partials 2 to 5 at -47..-63,
    /// -17..-28 dB/s; the brief: 50 to 65 down, -30 ± 8); the concert
    /// reference's 68 to 71 dB down, falling 121 to 122 dB/s (its release
    /// table 0.3-1.5 s: -73 to -97). And the note's own fundamental, with the
    /// tail under it, never dips more than two decibels (three on the
    /// concert, whose halo is 10 dB wider) under the note alone as they
    /// cross: heard as it sounds (`HEARD`) the tail continues the note.
    /// Measured: printed.
    #[test]
    fn a_released_note_joins_its_tail() {
        for (name, base, set) in shipped() {
            let f0 = set.keys[usize::from(60 - FIRST_KEY)].f0;
            for held in [0.5_f32, 2.0] {
                let off = 0.05 + held;
                let (mono, _, harp) = play(&set, &base, &[(60, 80, 0.05, off)], &[], off + 1.6);
                let with: Vec<f32> = mono.iter().zip(&harp).map(|(a, b)| a + b).collect();
                let (_, f1) = search_peak(&with[secs(0.1)..secs(off - 0.02)], f0, 0.004, RATE);
                let n1 = |t: f32| db(goertzel_hann(&with[secs(off + t - 0.05)..secs(off + t + 0.05)], f1, RATE));
                let keyoff = db(goertzel_hann(&with[secs(off - 0.1)..secs(off)], f1, RATE));
                // The concert's tail is under the gate's floor by +1 s, so
                // its fall is read from +0.3 s to +0.6 s.
                let (ta, tb) = if name == "intimate" { (0.5, 1.0) } else { (0.3, 0.6) };
                let t05 = n1(0.5) - keyoff;
                let rate = (n1(tb) - n1(ta)) / (tb - ta);
                let mut worst = 0.0_f32;
                let mut t = off + 0.02;
                while t < off + 0.4 {
                    let w = 0.04;
                    let alone = goertzel_hann(&mono[secs(t)..secs(t + w)], f0, RATE);
                    let both = goertzel_hann(&with[secs(t)..secs(t + w)], f0, RATE);
                    worst = worst.min(db(both) - db(alone));
                    t += 0.02;
                }
                println!("{name} C4 held {held} s: fundamental {t05:+.1} dB at +0.5 s re key-off, falling {rate:+.1} dB/s ({ta}-{tb} s); deepest dip of n1 with the tail against without, first 0.4 s: {worst:+.1} dB");
                if name == "intimate" {
                    assert!((-65.0..=-50.0).contains(&t05), "{name}: tail at +0.5 s {t05:+.1} dB");
                    assert!((-38.0..=-20.0).contains(&rate), "{name}: tail falls at {rate:+.1} dB/s");
                    assert!(worst > -2.0, "{name}: the tail cancels the note it continues by {worst:+.1} dB");
                } else {
                    assert!(t05 <= -60.0, "{name}: tail at +0.5 s {t05:+.1} dB");
                    assert!(rate <= -55.0, "{name}: tail falls at {rate:+.1} dB/s");
                    assert!(worst > -3.5, "{name}: the tail cancels the note it continues by {worst:+.1} dB");
                }
            }
        }
    }

    /// **A lifted pedal lets the wash fall (verifier finding 2).** A
    /// fortissimo ten-note cluster under the pedal, its keys up after 1.5 s,
    /// the pedal lifted at 3 s: the harp after the lift against the voices
    /// in the fifth of a second before it. The damped strings take the
    /// felt's bite (`LAND_T60` for `LAND_SECS`) before their residual: by
    /// +0.5 s at least 40 dB under where they stood; without it they fell
    /// 25 dB, and the whole wash rang on. With the undamped treble and the
    /// aliquots, which have no felt and ring on as they would, the harp is
    /// under -50 dB re the voices before the lift by +0.5 s: the reference's
    /// whole output there is -52.5 on the intimate and -59.8 on the concert,
    /// with its bank and without it within a decibel on the intimate.
    /// Measured: printed.
    #[test]
    fn a_lifted_pedal_lets_the_wash_fall() {
        let cluster = [36u8, 43, 48, 52, 55, 60, 64, 67, 72, 76];
        for (name, base, set) in shipped() {
            let keys: Vec<(u8, u8, f32, f32)> = cluster.iter().enumerate().map(|(i, &k)| (k, 120u8, 0.1 + 0.009 * i as f32, 1.6)).collect();
            let mut damped = set;
            damped.aliquots = [HarpString::NONE; ALIQUOTS];
            for s in damped.keys.iter_mut().filter(|s| !s.damper) {
                s.couple = 0.0;
                s.own = 0.0;
            }
            let (mono, _, harp) = play(&set, &base, &keys, &[(0.0, true), (3.0, false)], 4.2);
            let (_, _, dh) = play(&damped, &base, &keys, &[(0.0, true), (3.0, false)], 4.2);
            let voices = level(&mono, 2.8, 3.0);
            let at = |x: &[f32], t: f32, re: f32| db(level(x, 3.0 + t, 3.05 + t) / re);
            let pre = level(&dh, 2.8, 3.0);
            println!(
                "{name}: harp before the lift {:+.1} dB re the voices; after, re the voices before: harp +0.1 {:+.1} +0.3 {:+.1} +0.5 {:+.1} +1.0 {:+.1}, voices +0.1 {:+.1} +0.3 {:+.1} +0.5 {:+.1}; the damped strings re themselves +0.3 {:+.1} +0.5 {:+.1}",
                db(level(&harp, 2.8, 3.0) / voices),
                at(&harp, 0.1, voices),
                at(&harp, 0.3, voices),
                at(&harp, 0.5, voices),
                at(&harp, 1.0, voices),
                at(&mono, 0.1, voices),
                at(&mono, 0.3, voices),
                at(&mono, 0.5, voices),
                at(&dh, 0.3, pre),
                at(&dh, 0.5, pre)
            );
            assert!(at(&dh, 0.5, pre) < -40.0, "{name}: the damped strings are {:+.1} dB half a second after the lift", at(&dh, 0.5, pre));
            assert!(at(&harp, 0.5, voices) < -50.0, "{name}: the harp is {:+.1} dB re the voices half a second after the lift", at(&harp, 0.5, voices));
        }
    }

    /// **Half pedal lands the same however the pedal got there.** The same
    /// drive into two harps: one with the pedal at CC 40 throughout and C3
    /// and C4 let go at 1 s, the other with the pedal down until 2 s and
    /// then brought to CC 40 in one message. Once the felt's bite is over,
    /// every string of both bears the same pressure with the same
    /// couplings and losses heading to the same values — the dampers are
    /// where the level puts them whatever the path; the second harp's
    /// strings, free for two seconds and ringing with everything, keep the
    /// energy they gathered and lose it at the same rate as the first's (a
    /// real string does: the felt bears, it does not reset). At C4 the two
    /// stand within 3 dB by a third of a second after the message; at C3,
    /// a slower string, the second still carries what it gathered (printed).
    #[test]
    fn half_pedal_lands_the_same_however_it_got_there() {
        let set = intimate();
        let n = secs(2.6);
        let f0 = f_equal(60);
        // A held C4's partials, decaying as under a pedal, and a C3 below.
        let drive: Vec<f32> = (0..n)
            .map(|i| {
                let t = i as f32 / RATE;
                if t < 0.1 {
                    return 0.0;
                }
                let e = (-(t - 0.1) * 1.2).exp();
                (1..=8).map(|k| (std::f32::consts::TAU * f0 * k as f32 * t).sin() / k as f32).sum::<f32>() * 0.05 * e
                    + (std::f32::consts::TAU * f0 * 0.5 * t).sin() * 0.03 * e
            })
            .collect();
        let run = |late: bool| {
            let mut h = Harp::new(RATE);
            h.string(&set);
            h.set_sustain(if late { 1.0 } else { 40.0 / 127.0 });
            let mut out = vec![0.0_f32; n];
            for (k, c) in drive.chunks(BLOCK).enumerate() {
                let t = (k * BLOCK) as f32 / RATE;
                if k == secs(0.1) / BLOCK {
                    h.key_down(60);
                    h.key_down(48);
                }
                if k == secs(1.0) / BLOCK {
                    h.key_up(60);
                    h.key_up(48);
                }
                if late && (t - 2.0).abs() < 0.5 * BLOCK as f32 / RATE {
                    h.set_sustain(40.0 / 127.0);
                }
                h.render(c, &mut out[k * BLOCK..k * BLOCK + c.len()]);
            }
            (out, h)
        };
        let ((a, ha), (b, hb)) = (run(false), run(true));
        assert_eq!(ha.pressure, hb.pressure, "the dampers bear differently");
        for (i, (la, lb)) in ha.loops.iter().zip(&hb.loops).enumerate() {
            assert_eq!((la.gain_to, la.cin_to, la.cis_to, la.cout_to), (lb.gain_to, lb.cin_to, lb.cis_to, lb.cout_to), "loop {i} heads elsewhere");
        }
        let (w0, w1) = (secs(2.3), secs(2.55));
        for f in [f0, 0.5 * f0] {
            let (ma, fa) = search_peak(&a[w0..w1], f, 0.004, RATE);
            let mb = goertzel_hann(&b[w0..w1], fa, RATE);
            println!("{f:.1} Hz, 0.3-0.55 s after the message: pedal at 40 all along {:+.1} dB, brought there {:+.1} dB", db(ma), db(mb));
            if f == f0 {
                assert!((db(mb) - db(ma)).abs() < 3.0, "{f:.1} Hz: {:+.1} dB apart", db(mb) - db(ma));
            }
        }
    }

    /// **No felt above the last damper (item 4).** The intimate's last
    /// damper is MIDI 88 and the concert's 92 (`GrandBase::last_damper`):
    /// a key above it that comes up keeps its free loss and its coupling;
    /// a key at it switches to the damped loss and stops being fed.
    #[test]
    fn keys_above_the_last_damper_never_damp() {
        for (base, last) in [(GrandBase::INTIMATE, 88u8), (GrandBase::CONCERT, 92)] {
            let mut h = Harp::new(RATE);
            h.string(&StringSet::grand(&base, 1.0, &GrandHarp::INTIMATE));
            for (key, damped) in [(last, true), (last + 1, false), (108, false), (60, true)] {
                h.key_down(key);
                h.key_up(key);
                let l = &h.loops[usize::from(key - FIRST_KEY)];
                assert_eq!(l.damper, damped, "base {} key {key}: damper {}", base.id, l.damper);
                if damped {
                    assert_eq!(l.gain_to, l.gain_damped, "base {} key {key} did not take the damped loss", base.id);
                    assert!(!l.fed(), "base {} key {key} is still fed", base.id);
                } else {
                    assert!(l.gain_to == l.gain_free || l.gain_to == l.gain_halo, "base {} key {key} took a damped loss", base.id);
                    assert!(l.fed(), "base {} key {key} stopped being fed", base.id);
                }
            }
        }
    }

    /// **A key comes up once, and a held one gives up its halo.** C4 down
    /// and up on the intimate's harp, then up again: the second `key_up`
    /// leaves the string exactly as the first left it (it re-armed the
    /// felt and took the string's ring back; review). Down and let go with
    /// `key_up_held`, the sostenuto's: rung and still held, heading for
    /// what the same key let go with the dampers off heads for; and its
    /// `key_up` when the hold ends lands it exactly as lifting that pedal
    /// lands the other.
    #[test]
    fn a_key_comes_up_once_and_a_held_one_gives_up_its_halo() {
        let set = StringSet::grand(&GrandBase::INTIMATE, 0.4, &GrandHarp::INTIMATE);
        let fresh = || {
            let mut h = Harp::new(RATE);
            h.string(&set);
            h
        };
        let state = |h: &Harp| {
            let l = &h.loops[usize::from(60 - FIRST_KEY)];
            (l.held, l.voiced, l.rung, l.land, [l.cin_to, l.cis_to, l.cout_to, l.gain_to])
        };
        let mut h = fresh();
        h.key_down(60);
        h.key_up(60);
        let once = state(&h);
        h.key_up(60);
        assert_eq!(state(&h), once, "a second key_up moved the string");
        // Held by the sostenuto, against let go under the sustain pedal.
        let (mut caught, mut pedalled) = (fresh(), fresh());
        pedalled.set_sustain(1.0);
        for h in [&mut caught, &mut pedalled] {
            h.key_down(60);
        }
        caught.key_up_held(60);
        pedalled.key_up(60);
        let (c, p) = (state(&caught), state(&pedalled));
        assert_eq!((c.0, c.1, c.2), (true, false, true), "the caught string (held, voiced, rung)");
        assert_eq!(c.4, p.4, "the caught string heads elsewhere than the pedalled one");
        caught.key_up(60);
        pedalled.set_sustain(0.0);
        assert_eq!(state(&caught), state(&pedalled), "the hold's end landed the string differently from the pedal's");
    }

    /// The silently held C4-E4-G4 answering a staccato C2, re C2's own
    /// fundamental while it rang, L and R power-averaged on both sides:
    /// (C4, E4, G4, C5, G5) in dB.
    fn silent_answers(set: &StringSet, base: &GrandBase) -> [f32; 5] {
        let keys = [(60u8, 1u8, 0.05_f32, 6.0_f32), (64, 1, 0.05, 6.0), (67, 1, 0.05, 6.0), (36, 100, 1.5, 1.65)];
        let (l, r, _, harp) = play_lr(set, base, &keys, &[], 4.5);
        let (al, ar, _, _) = play_lr(set, base, &[(36, 100, 0.05, 1.05)], &[], 1.5);
        let pw = |a: &[f32], b: &[f32], f: f32, span: f32| {
            let (ma, _) = search_peak(a, f, span, RATE);
            let (mb, _) = search_peak(b, f, span, RATE);
            10.0 * (0.5 * (ma * ma + mb * mb)).max(1e-30).log10()
        };
        let c2 = pw(&al[secs(0.15)..secs(0.95)], &ar[secs(0.15)..secs(0.95)], f_equal(36), 0.003);
        let wl: Vec<f32> = l.iter().zip(&harp).map(|(a, b)| a + b).collect();
        let wr: Vec<f32> = r.iter().zip(&harp).map(|(a, b)| a + b).collect();
        let (a, b) = (secs(2.0), secs(4.0));
        [60u8, 64, 67, 72, 79].map(|key| pw(&wl[a..b], &wr[a..b], f_equal(key), 0.004) - c2)
    }

    /// **A silently held chord answers a staccato bass note (item 5).**
    /// C4, E4 and G4 pressed at MIDI 1 — under the grand's let-off, so no
    /// hammer reaches the strings and their dampers simply lift — then C2
    /// at MIDI 100 for 0.15 s. The held strings' answers 0.5-2.5 s later,
    /// against C2's own fundamental while it rang: the intimate reference's
    /// C5 (the held C4's second partial, lit by C2's eighth) -35.9 dB, its
    /// C4 -57.7, G4 -43.9, G5 -43.3. Here the intimate's C5 is held to the
    /// brief's -35 ± 5 and its C4, the held string's fundamental, under
    /// -40. Measured: printed.
    #[test]
    fn a_silent_chord_answers_a_staccato_c2() {
        for (name, base, set) in shipped() {
            let [c4, e4, g4, c5, g5] = silent_answers(&set, &base);
            println!("{name}: silent C4-E4-G4 under a staccato C2, 0.5-2.5 s after, re C2 n1 (L/R power): C4 {c4:+.1} E4 {e4:+.1} G4 {g4:+.1} C5 {c5:+.1} G5 {g5:+.1}");
            if name == "intimate" {
                assert!((-40.0..=-30.0).contains(&c5), "{name}: the held C4's second partial answers at {c5:+.1} dB");
                assert!(c4 < -40.0, "{name}: the held C4's fundamental answers at {c4:+.1} dB");
            }
        }
    }

    /// **The pedal's wash between a C2's partials (item 5).** C2 at MIDI
    /// 100 held a second, pedal down and pedal up: the level midway between
    /// its partials 1..11, 0.35-1.0 s in, against its fundamental, L and R
    /// power-averaged. In absolute terms the intimate reference's pedal-down
    /// floor sits at -56.7 dB re n1 (median of the eleven) and its pedal-up
    /// floor at -71.4; ours is cleaner pedal-up (no key, damper or room
    /// noise under it), so the rise reads high. Pinned: the absolute
    /// pedal-down wash within six decibels of the reference's, and a rise of
    /// at least 12. Measured: printed.
    #[test]
    fn the_pedal_fills_between_a_c2s_partials() {
        let base = GrandBase::INTIMATE;
        let set = StringSet::grand(&base, 0.4, &GrandHarp::INTIMATE);
        let c2 = f_equal(36) * 2.0_f32.powf(-2.8 / 1200.0);
        let mut med = [0.0_f32; 2];
        for (j, pedal) in [false, true].into_iter().enumerate() {
            let ped: Vec<(f32, bool)> = if pedal { vec![(0.0, true)] } else { vec![] };
            let (l, r, _, harp) = play_lr(&set, &base, &[(36, 100, 0.05, 1.05)], &ped, 1.1);
            let (a, b) = (secs(0.35), secs(1.0));
            let wl: Vec<f32> = l[a..b].iter().zip(&harp[a..b]).map(|(x, h)| x + h).collect();
            let wr: Vec<f32> = r[a..b].iter().zip(&harp[a..b]).map(|(x, h)| x + h).collect();
            let pw = |f: f32, span: f32| {
                let (ma, _) = search_peak(&wl, f, span, RATE);
                let (mb, _) = search_peak(&wr, f, span, RATE);
                10.0 * (0.5 * (ma * ma + mb * mb)).max(1e-30).log10()
            };
            let n1 = pw(c2, 0.003);
            let mut lv: Vec<f32> = (1..12).map(|k| pw((k as f32 + 0.5) * c2 * 1.001, 0.02) - n1).collect();
            lv.sort_by(|a, b| a.partial_cmp(b).unwrap());
            med[j] = lv[5];
        }
        println!("between C2's partials re n1 (L/R power), median: pedal up {:+.1}, pedal down {:+.1} dB (rise {:+.1}); reference -71.4 / -56.7", med[0], med[1], med[1] - med[0]);
        assert!((med[1] + 56.7).abs() < 6.0, "the pedal's wash sits at {:+.1} dB re n1", med[1]);
        assert!(med[1] - med[0] > 12.0, "the pedal lifts the floor only {:+.1} dB", med[1] - med[0]);
    }

    /// **Pedal down, a staccato C2 leaves its octave, twelfth and double
    /// octave ringing**: the C3, G3 and C4 strings, 0.5-1.5 s after the
    /// damper falls, at least 12 dB over the same with the pedal up.
    #[test]
    fn pedal_down_c2_lights_c3_g3_c4() {
        let base = GrandBase::INTIMATE;
        let set = StringSet::grand(&base, 0.4, &GrandHarp::INTIMATE);
        let run = |pedal: bool| {
            let ped: Vec<(f32, bool)> = if pedal { vec![(0.0, true)] } else { vec![] };
            let (mono, _, harp) = play(&set, &base, &[(36, 96, 0.05, 0.25)], &ped, 1.9);
            let (a, b) = (secs(0.25 + 0.5), secs(0.25 + 1.5));
            mono[a..b].iter().zip(&harp[a..b]).map(|(x, y)| x + y).collect::<Vec<f32>>()
        };
        let down = run(true);
        let up = run(false);
        for (name, key) in [("C3", 48u8), ("G3", 55), ("C4", 60)] {
            let f = set.keys[usize::from(key - FIRST_KEY)].f0;
            let (d, fd) = search_peak(&down, f, 0.01, RATE);
            let (u, _) = search_peak(&up, f, 0.01, RATE);
            println!("  {name} {fd:7.2} Hz: pedal down {:+.1} dB, up {:+.1} dB, delta {:+.1}", db(d), db(u), db(d) - db(u));
            assert!(db(d) - db(u) >= 12.0, "{name}: pedal down only {:+.1} dB over pedal up", db(d) - db(u));
        }
    }

    /// **A dense pedalled passage leaves the harp under its voices, octave
    /// by octave (verifier finding 1).** The bench's stress score at the
    /// voice — sixty-four keys from F#1, 20 ms apart, MIDI 96, the pedal
    /// down — the harp's power against the voices' in octave bands from
    /// 62 Hz to 8 kHz over 1.0-2.0 s. The references' banks there: the
    /// intimate's 13 to 23 dB under its voices from 62 Hz to 4 kHz, the
    /// concert's 5 to 16. The first-difference feed put the concert's harp
    /// 9.7 dB OVER its voices at 2-4 kHz and the intimate's within a
    /// decibel; pinned: every band at least 6 dB under on both grands.
    /// Measured: printed.
    #[test]
    fn a_dense_pedalled_passage_stays_under_its_voices() {
        let stress: Vec<(u8, u8, f32, f32)> = (30u8..94).enumerate().map(|(i, p)| (p, 96, 0.05 + 0.02 * i as f32, 0.15 + 0.02 * i as f32)).collect();
        for (name, base, set) in shipped() {
            let (mono, _, harp) = play(&set, &base, &stress, &[(0.0, true)], 2.0);
            let (h, v) = (octave_bands(&harp, 1.0, 2.0), octave_bands(&mono, 1.0, 2.0));
            let rel: Vec<f32> = h.iter().zip(&v).map(|(a, b)| (10.0 * (a / b.max(1e-30)).log10()) as f32).collect();
            let line: Vec<String> = rel.iter().map(|r| format!("{r:+.1}")).collect();
            let peak = |x: &[f32]| x.iter().fold(0.0_f32, |m, s| m.max(s.abs()));
            let with: Vec<f32> = mono.iter().zip(&harp).map(|(a, b)| a + b).collect();
            println!("{name}: harp re voices, octaves from 62.5 Hz, 1.0-2.0 s: {}; peak {:+.2} dBFS with the harp, {:+.2} without", line.join(" "), db(peak(&with)), db(peak(&mono)));
            for (b, r) in rel.iter().enumerate() {
                assert!(*r < -6.0, "{name}: the {} Hz octave's harp is {r:+.1} dB re its voices", 62.5 * 2.0_f32.powi(b as i32));
            }
        }
    }

    /// **A bass string rung once is not cleared mid-ring (verifier finding
    /// 11).** Every string free, every loop woken, one sample of drive: an
    /// A0 string's energy passes its read once a period (seven frames at
    /// 48 kHz), and the gate judged one frame, so it cleared the bass strings
    /// at the end of their probation between two arrivals. Judged over more
    /// than a period, A0 to C2 are all still ringing a second later.
    #[test]
    fn a_bass_string_rung_once_is_not_cleared_mid_ring() {
        let mut h = Harp::new(RATE);
        h.string(&intimate());
        h.set_pedal(true);
        h.key_down(100);
        let mut out = vec![0.0_f32; BLOCK];
        let mut drive = vec![0.0_f32; BLOCK];
        drive[0] = 1.0;
        h.render(&drive, &mut out);
        drive[0] = 0.0;
        for _ in 0..secs(1.0) / BLOCK {
            h.render(&drive, &mut out);
        }
        for key in 21..=36u8 {
            let l = &h.loops[usize::from(key - FIRST_KEY)];
            assert_eq!(l.state, State::Ticking, "key {key} was cleared a second after one sample of drive");
        }
    }

    /// **The treble rings with its undamped strings and its aliquots (item
    /// 6).** G6 and C7 held 3 s on the intimate: the harp under each note
    /// over the hold, without its aliquots and with them. The reference
    /// (L/R power average, exact subtraction): sympathetic resonance -20.6
    /// and -20.0 dB, the aliquots -26.0 and -24.3, the two together about
    /// -19.5 and -18.6; the brief's combined band -12..-22. And the
    /// aliquots stay out of the middle: under -38 dB at C4 and C5 (the
    /// reference's -41 at C5).
    #[test]
    fn the_treble_rings_with_its_aliquots() {
        let base = GrandBase::INTIMATE;
        let with_ali = StringSet::grand(&base, 0.4, &GrandHarp::INTIMATE);
        let mut without = with_ali;
        without.aliquots = [HarpString::NONE; ALIQUOTS];
        for (key, held) in [(91u8, 3.0_f32), (96, 3.0), (60, 2.0), (72, 1.0)] {
            let ev = [(key, 80u8, 0.05_f32, 0.05 + held)];
            let (mono, _, a) = play(&with_ali, &base, &ev, &[], 0.05 + held);
            let (_, _, b) = play(&without, &base, &ev, &[], 0.05 + held);
            let end = 0.05 + held;
            let note = level(&mono, 0.07, end);
            let sr = db(level(&b, 0.07, end) / note);
            let ali: Vec<f32> = a.iter().zip(&b).map(|(x, y)| x - y).collect();
            let al = db(level(&ali, 0.07, end) / note);
            let both = db(level(&a, 0.07, end) / note);
            println!("key {key}: strings {sr:+.1}, aliquots {al:+.1}, together {both:+.1} dB re note");
            if key > 88 {
                assert!((-22.0..=-12.0).contains(&both), "key {key}: the treble rings at {both:+.1} dB");
                assert!((-24.5..=-17.0).contains(&sr), "key {key}: its strings ring at {sr:+.1} dB");
            } else {
                assert!(al < -38.0, "key {key}: the aliquots ring at {al:+.1} dB in the middle");
            }
        }
    }

    /// **The send is a gain (item 7).** The same notes through the same
    /// strings at 0.4 and 1.0: the harp's whole output is 20 log 2.5 =
    /// 7.96 dB apart, to rounding, because the send scales what the bridge
    /// drives the harp with and the harp is linear.
    #[test]
    fn the_send_is_one_gain_on_the_harp() {
        let base = GrandBase::INTIMATE;
        let ev = [(48u8, 90u8, 0.05_f32, 1.0_f32), (60, 70, 0.3, 1.2)];
        let (_, _, a) = play(&StringSet::grand(&base, 0.4, &GrandHarp::INTIMATE), &base, &ev, &[(0.0, true)], 2.0);
        let (_, _, b) = play(&StringSet::grand(&base, 1.0, &GrandHarp::INTIMATE), &base, &ev, &[(0.0, true)], 2.0);
        let ratio = db(rms(&b) / rms(&a));
        println!("send 1.0 over send 0.4: {ratio:+.3} dB");
        assert!((ratio - 7.959).abs() < 0.05, "the send moved the harp {ratio:+.3} dB");
    }

    /// **The grand strings the harp with its own tuning (item 2).** Every
    /// key's string sits at the grand's stretched pitch (`grand::tuning`)
    /// plus the harp's detune, on the grand's B.
    #[test]
    fn the_grand_strings_the_harp_with_its_own_tuning() {
        for (base, voice) in [(GrandBase::INTIMATE, GrandHarp::INTIMATE), (GrandBase::CONCERT, GrandHarp::CONCERT)] {
            let set = StringSet::grand(&base, 1.0, &voice);
            let (cents, b) = grand::tuning(&base);
            for i in 0..KEYS {
                let key = FIRST_KEY + i as u8;
                let want = f_equal(key) * 2.0_f32.powf((cents[i] + DETUNE_CENTS) / 1200.0);
                assert!((set.keys[i].f0 / want - 1.0).abs() < 1e-6, "key {key}");
                assert_eq!(set.keys[i].b, b[i]);
                assert_eq!(set.keys[i].damper, key <= base.last_damper);
            }
            println!("base {}: A0 {:+.1} c, C4 {:+.1} c, C7 {:+.1} c from equal temperament (detune included)", base.id, cents[0] + DETUNE_CENTS, cents[39] + DETUNE_CENTS, cents[75] + DETUNE_CENTS);
        }
    }

    /// **Any instrument can string it (item 8).** A set of six strings an
    /// instrument chose — one on an arbitrary pitch 23 cents off its key,
    /// as an electric grand's bank might be — and nothing else: those six
    /// wake, nothing else ever ticks, and each rings where it was put.
    #[test]
    fn any_instrument_can_string_the_harp() {
        let mut set = StringSet::grand(&GrandBase::INTIMATE, 1.0, &GrandHarp::CONCERT);
        for s in set.keys.iter_mut() {
            s.couple = 0.0;
            s.own = 0.0;
        }
        let strings = [(40u8, 82.41_f32), (45, 110.0), (50, 146.83), (55, 196.0 * 2.0_f32.powf(23.0 / 1200.0)), (59, 246.94), (64, 329.63)];
        for &(key, f) in &strings {
            let s = &mut set.keys[usize::from(key - FIRST_KEY)];
            s.f0 = f;
            s.couple = 0.03;
            s.damper = false;
        }
        let mut h = Harp::new(RATE);
        h.string(&set);
        h.key_down(100);
        assert_eq!(h.ticking(), strings.len(), "{} loops woke for a six-string set", h.ticking());
        for &(key, f) in &strings {
            let i = usize::from(key - FIRST_KEY);
            let x = ring_loop(&mut h, i, secs(0.6), false);
            let (_, got) = locate(&x, f, RATE);
            println!("string at key {key}: asked {f:.3} Hz, rings at {got:.3} Hz ({:+.2} c)", cents(got, f));
            assert!(cents(got, f).abs() < 0.5);
        }
    }

    /// **Every loop any shipped set designs is stable**, at every rate the
    /// app runs: free, damped, halo and landing gains under one (the DC
    /// gain of a one-pole with a gain in front IS the gain, and its
    /// magnitude falls from there), every coefficient finite, the Thiran
    /// and the all-pass inside the unit circle.
    #[test]
    fn every_designed_loop_is_stable() {
        let sets = [
            StringSet::grand(&GrandBase::INTIMATE, 0.4, &GrandHarp::INTIMATE),
            StringSet::grand(&GrandBase::CONCERT, 1.0, &GrandHarp::CONCERT),
            StringSet::piano(&PianoBase::JAZZ),
            StringSet::piano(&PianoBase::CONCERT),
        ];
        for rate in [44_100.0_f32, 48_000.0, 88_200.0, 96_000.0, 192_000.0] {
            let mut h = Harp::new(rate);
            for set in &sets {
                h.string(set);
                for (i, l) in h.loops.iter().enumerate() {
                    for g in [l.gain_free, l.gain_damped, l.gain_halo, l.gain_land] {
                        assert!(g.is_finite() && g > 0.0 && g < 1.0, "loop {i} at {rate}: gain {g}");
                    }
                    assert!(l.frac.a.abs() < 1.0 && l.disp.a.abs() < 1.0 && l.loss.a > 0.0 && l.loss.a <= 1.0, "loop {i} at {rate}");
                    assert!(l.back >= 1 && l.back + 2 <= l.n, "loop {i} at {rate}: back {} of {}", l.back, l.n);
                    assert!(l.norm.is_finite() && l.norm >= 1.0 && l.norm < 4.0, "loop {i} at {rate}: norm {}", l.norm);
                }
            }
        }
    }

    /// **Thirty seconds of loud pedalled chords stay bounded**, on both
    /// grands at their sends and at five times the concert's (the
    /// reference's control runs to 5): every sample finite; no build-up —
    /// the loudest second of the same nine seconds of chords played twice,
    /// 3-12 s and 21-30 s, within 3 dB (the score cycles three voicings in
    /// two octaves; comparing 2-12 s with 20-30 s compared other chords and
    /// read +4.6 dB); and once the pedal lifts, two seconds later, 20 dB down.
    /// At the shipped sends the harp also stays under the voices feeding it,
    /// peak and loudest second.
    #[test]
    fn thirty_seconds_of_loud_pedalled_chords_stay_bounded() {
        let chords = [[36u8, 43, 48, 52, 55, 60, 64, 67], [41, 48, 53, 57, 60, 65, 69, 72], [31, 38, 43, 47, 50, 55, 59, 62]];
        let mut keys = Vec::new();
        for c in 0..20 {
            for &k in &chords[c % 3] {
                keys.push((k + (c / 3) as u8 % 2 * 12, 127u8, 0.05 + 1.5 * c as f32, 0.05 + 1.5 * c as f32 + 1.2));
            }
        }
        for (base, send, voice, shipped) in [(GrandBase::INTIMATE, 0.4_f32, GrandHarp::INTIMATE, true), (GrandBase::CONCERT, 1.0, GrandHarp::CONCERT, true), (GrandBase::CONCERT, 5.0, GrandHarp::CONCERT, false)] {
            let set = StringSet::grand(&base, send, &voice);
            let (mono, drive, harp) = play(&set, &base, &keys, &[(0.0, true), (30.5, false)], 33.0);
            assert!(harp.iter().all(|v| v.is_finite()), "send {send}: a non-finite sample");
            let peak = |x: &[f32]| x.iter().fold(0.0_f32, |m, v| m.max(v.abs()));
            let per_s = |x: &[f32]| (0..33).map(|s| level(x, s as f32, s as f32 + 1.0)).collect::<Vec<f32>>();
            let (hs, vs) = (per_s(&harp), per_s(&mono));
            let loudest_voice = vs.iter().cloned().fold(0.0_f32, f32::max);
            let loudest_harp = hs.iter().cloned().fold(0.0_f32, f32::max);
            let fall = db(level(&harp, 32.5, 33.0) / level(&harp, 29.5, 30.5));
            let early = hs[3..12].iter().cloned().fold(0.0_f32, f32::max);
            let late = hs[21..30].iter().cloned().fold(0.0_f32, f32::max);
            let line: Vec<String> = hs.iter().zip(&vs).map(|(h, v)| format!("{:+.0}", db(*h) - db(*v))).collect();
            println!("base {} send {send}: harp re voices second by second: {}", base.id, line.join(" "));
            println!(
                "base {} send {send}: voices peak {:+.1} dBFS, drive {:+.1}, harp {:+.1}; loudest second voices {:+.1}, harp {:+.1} dBFS; harp's loudest second 3-12 s {:+.1}, 21-30 s {:+.1}; 2 s after the pedal lifts {fall:+.1} dB",
                base.id,
                db(peak(&mono)),
                db(peak(&drive)),
                db(peak(&harp)),
                db(loudest_voice),
                db(loudest_harp),
                db(early),
                db(late)
            );
            if shipped {
                assert!(peak(&harp) < peak(&mono), "send {send}: the harp out-peaks its voices");
                assert!(loudest_harp < loudest_voice, "send {send}: a second of harp louder than any of the voices");
            }
            assert!(db(late) - db(early) < 3.0, "send {send}: the harp built up {:+.1} dB over the repeats", db(late) - db(early));
            assert!(fall < -20.0, "send {send}: the harp has not fallen after the pedal lifted ({fall:+.1} dB)");
        }
    }

    /// **The gate**: pedal down and nothing struck is not one loop-tick;
    /// a strike wakes every fed string; after probation only strings that
    /// ring above the floor stay, every sleeping loop is cleared, and a
    /// staccato passage with the pedal up runs a handful of strings, not a
    /// hundred.
    #[test]
    fn the_gate_skips_silent_loops() {
        let set = intimate();
        let mut h = Harp::new(RATE);
        h.string(&set);
        h.set_pedal(true);
        for _ in 0..secs(1.0) {
            h.tick(0.0);
        }
        assert_eq!(h.ticks(), 0, "pedal down and nothing struck ticked {} loop-samples", h.ticks());
        assert!(!h.active());
        h.set_pedal(false);
        h.key_down(60);
        // Pedal up: the struck string, the twenty undamped treble strings
        // and the nineteen aliquots are fed.
        assert_eq!(h.ticking(), 1 + 20 + ALIQUOTS, "a pedal-up strike woke {}", h.ticking());
        let sleeping_clear = |h: &Harp| h.loops.iter().all(|l| l.state != State::Sleeping || h.arena[l.off as usize..(l.off + l.n) as usize].iter().all(|&v| v == 0.0));
        assert!(sleeping_clear(&h));
        let (_, drive, _) = play(&set, &GrandBase::INTIMATE, &[(60, 80, 0.0, 0.3)], &[], 3.0);
        let mut counts = Vec::new();
        for (i, chunk) in drive.chunks(BLOCK).enumerate() {
            let mut out = [0.0_f32; BLOCK];
            h.render(chunk, &mut out[..chunk.len()]);
            if i == 10 {
                h.key_up(60);
            }
            if i % 75 == 0 {
                counts.push(h.ticking());
                assert!(sleeping_clear(&h));
            }
        }
        println!("pedal-up C4: loops ticking every 0.2 s: {counts:?}");
        assert!(counts[4] <= 10, "1 s after a staccato C4 the harp runs {} loops", counts[4]);
    }

    /// **Nothing pops.** The harp's return under a steady drive through
    /// every transition it has — keys down and up, pedal down, up and half
    /// way, a voice handed back, the gate retiring and waking — has no first
    /// difference larger than the signal's own steady slope allows.
    #[test]
    fn opening_and_closing_never_step() {
        let set = intimate();
        let mut h = Harp::new(RATE);
        h.string(&set);
        let f0 = f_equal(60);
        let drive = |i: usize| {
            let t = i as f32 / RATE;
            (1..=6).map(|n| (std::f32::consts::TAU * f0 * n as f32 * t).sin() / n as f32).sum::<f32>() * 0.1
        };
        h.set_pedal(true);
        h.key_down(60);
        type Event = (f32, fn(&mut Harp));
        let events: &[Event] = &[
            (0.5, |h| h.key_down(67)),
            (0.8, |h| h.key_up(67)),
            (1.0, |h| h.set_pedal(false)),
            (1.05, |h| h.set_pedal(true)),
            (1.3, |h| h.key_down(48)),
            (1.6, |h| h.voice_done(60)),
            (1.75, |h| h.set_sustain(0.35)),
            (1.9, |h| h.key_up(48)),
            (2.05, |h| h.set_sustain(0.9)),
            (2.2, |h| h.key_up(60)),
            (2.5, |h| h.set_pedal(false)),
        ];
        let n = secs(3.5);
        let mut out = Vec::with_capacity(n);
        let mut e = 0;
        for i in 0..n {
            while e < events.len() && i == secs(events[e].0) {
                (events[e].1)(&mut h);
                e += 1;
            }
            out.push(h.tick(drive(i)));
        }
        assert_eq!(e, events.len());
        let mut r = Harp::new(RATE);
        r.string(&set);
        r.set_pedal(true);
        r.key_down(60);
        let reference: Vec<f32> = (0..n).map(|i| r.tick(drive(i))).collect();
        let steepest = |x: &[f32]| -> (usize, f32) {
            let mut worst = (0usize, 0.0_f32);
            for (k, fr) in x.chunks(FRAME).enumerate() {
                let peak = fr.iter().fold(0.0_f32, |m, v| m.max(v.abs()));
                if peak < 1.0e-6 {
                    continue;
                }
                let step = fr.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0_f32, f32::max);
                if step / peak > worst.1 {
                    worst = (k * FRAME, step / peak);
                }
            }
            worst
        };
        let (_, steady) = steepest(&reference);
        let (at, worst) = steepest(&out);
        println!("reference max step/peak {steady:.3}, worst with every transition {worst:.3} at {:.3} s", at as f32 / RATE);
        assert!(worst < 1.25 * steady, "a step/peak of {worst:.3} at {:.3} s against the signal's own {steady:.3}", at as f32 / RATE);
        assert!(out.iter().all(|v| v.is_finite()));
    }

    /// **A NaN on the bridge** — a poisoned voice, a bad host buffer — must
    /// not live in a hundred feedback paths for the rest of the session.
    #[test]
    fn a_nan_in_the_drive_does_not_poison_the_harp() {
        let mut h = Harp::new(RATE);
        h.string(&intimate());
        let f0 = f_equal(60);
        h.set_pedal(true);
        h.key_down(48);
        let mut bad = 0usize;
        let mut after = 0.0_f32;
        for i in 0..secs(2.0) {
            let x = if i == secs(0.5) {
                f32::NAN
            } else if i == secs(0.6) {
                f32::INFINITY
            } else if i == secs(0.7) {
                1.0e30
            } else {
                (std::f32::consts::TAU * f0 * i as f32 / RATE).sin() * 0.1
            };
            let y = h.tick(x);
            if !y.is_finite() {
                bad += 1;
            } else if i > secs(1.5) {
                after = after.max(y.abs());
            }
        }
        assert_eq!(bad, 0, "{bad} non-finite samples came out of the harp");
        assert!(after > 1.0e-6, "the harp went silent after the NaN, peak {after:.2e}");
    }

    /// **Small**: one arena, sized per pitch, well under the first harp's
    /// 1.4 MB budget at every rate the app runs.
    #[test]
    fn the_harp_is_small() {
        for rate in [44_100.0_f32, 48_000.0, 96_000.0, 192_000.0] {
            let h = Harp::new(rate);
            let total = size_of::<Harp>() + h.arena_bytes();
            println!("{rate} Hz: {} bytes struct + {} bytes arena = {} KB; a designed set {} bytes", size_of::<Harp>(), h.arena_bytes(), total / 1024, size_of::<Strung>());
            assert!(total < 1_400_000, "{total} bytes at {rate} Hz");
        }
    }

    /// **The one-way coupling's known flaw, bounded**: a fortissimo C2
    /// HELD under the pedal at the concert's send drives the C3, G3 and C4
    /// strings at resonance for four seconds with nothing draining the
    /// driver. Early in the hold the pedal moves the note's own partials 2,
    /// 3 and 4 by at most 3 dB (the reference's pedal moves a held C2's
    /// partials by -2.3 to +3.4 dB, its symp table, 0.3-0.95 s); and four
    /// seconds in, the free strings carry less than the note's partials did
    /// at one to two seconds — they outlast a partial that decays faster than
    /// they do, as a free string does, but never build past what drove them.
    #[test]
    fn a_held_note_never_out_rings_its_own_partial() {
        let base = GrandBase::CONCERT;
        let set = StringSet::grand(&base, 1.0, &GrandHarp::CONCERT);
        let (mono, _, harp) = play(&set, &base, &[(36, 127, 0.05, 4.05)], &[(0.0, true)], 4.05);
        let (upm, _, uph) = play(&set, &base, &[(36, 127, 0.05, 4.05)], &[], 1.05);
        let c2 = set.keys[usize::from(36 - FIRST_KEY)].f0;
        let with: Vec<f32> = mono.iter().zip(&harp).map(|(a, b)| a + b).collect();
        let up: Vec<f32> = upm.iter().zip(&uph).map(|(a, b)| a + b).collect();
        let (e0, e1) = (secs(0.35), secs(1.0));
        for (name, key, part) in [("C3", 48u8, 2.0_f32), ("G3", 55, 3.0), ("C4", 60, 4.0)] {
            let (d, f) = search_peak(&with[e0..e1], part * c2, 0.02, RATE);
            let u = goertzel_hann(&up[e0..e1], f, RATE);
            let (hm, _) = search_peak(&harp[secs(3.0)..secs(4.0)], set.keys[usize::from(key - FIRST_KEY)].f0, 0.01, RATE);
            let (vm, _) = search_peak(&mono[secs(1.0)..secs(2.0)], part * c2, 0.02, RATE);
            println!("held C2: partial {part} pedal down over up 0.35-1.0 s {:+.1} dB; harp {name} at 3-4 s {:+.1} dB re the partial at 1-2 s", db(d) - db(u), db(hm) - db(vm));
            assert!(db(d) - db(u) < 3.0, "{name}: the pedal lifts the note's partial {part} {:+.1} dB", db(d) - db(u));
            assert!(hm < vm, "{name}: the harp builds past the note's own partial");
        }
    }

    /// **Re-stringing a ringing harp does not step**: the concert's strings
    /// land under the intimate's circulating wave, coefficients only, and
    /// the output stays continuous.
    #[test]
    fn restringing_under_a_ringing_harp_does_not_step() {
        let mut h = Harp::new(RATE);
        h.string(&intimate());
        let concert = h.strung(&StringSet::grand(&GrandBase::CONCERT, 1.0, &GrandHarp::CONCERT));
        let f0 = f_equal(60);
        let drive = |i: usize| (std::f32::consts::TAU * f0 * i as f32 / RATE).sin() * 0.1;
        h.set_pedal(true);
        h.key_down(48);
        let mut out = Vec::new();
        for i in 0..secs(2.0) {
            if i == secs(1.0) {
                h.load(&concert);
            }
            out.push(h.tick(drive(i)));
        }
        let diffs: Vec<f32> = out.windows(2).map(|w| (w[1] - w[0]).abs()).collect();
        let before = diffs[secs(0.8)..secs(1.0)].iter().cloned().fold(0.0_f32, f32::max);
        let at = diffs[secs(1.0) - 2..secs(1.0) + 2].iter().cloned().fold(0.0_f32, f32::max);
        println!("max step before {before:.2e}, at the re-string {at:.2e}");
        assert!(at < 2.0 * before.max(1e-9));
    }

    /// Diagnostic: the numbers the harp is voiced on, at the voice, for both
    /// grands — halo, tail, silent keys, wash, the dense passage's bands, the
    /// lift — in one printout.
    #[test]
    #[ignore = "a printout for voicing the harp"]
    fn print_voicing() {
        for (name, base, set) in shipped() {
            for (key, held) in [(36u8, 3.0_f32), (60, 2.0), (60, 0.5), (72, 1.0), (91, 3.0), (96, 3.0)] {
                let (mono, _, harp) = play(&set, &base, &[(key, 80, 0.05, 0.05 + held)], &[], 0.05 + held + 1.6);
                let end = 0.05 + held;
                let halo = db(level(&harp, 0.07, end) / level(&mono, 0.07, end));
                let first = db(level(&harp, 0.07, 0.57) / level(&mono, 0.07, 0.57));
                let last = db(level(&harp, end - 0.5, end) / level(&mono, end - 0.5, end));
                let keyoff = level(&mono, end - 0.3, end);
                let t05 = db(level(&harp, end + 0.45, end + 0.55) / keyoff);
                let t10 = db(level(&harp, end + 0.95, end + 1.05) / keyoff);
                println!("{name} key {key} held {held}: halo {halo:+.1} (first {first:+.1} last {last:+.1}); tail +0.5 {t05:+.1} +1.0 {t10:+.1} ({:+.1} dB/s)", (t10 - t05) / 0.5);
            }
            let [c4, e4, g4, c5, g5] = silent_answers(&set, &base);
            println!("{name} silent: C4 {c4:+.1} E4 {e4:+.1} G4 {g4:+.1} C5 {c5:+.1} G5 {g5:+.1}");
        }
    }


    /// Diagnostic: where a dense pedalled passage's harp comes from — the
    /// struck strings' own loops (`own`), the free strings (`couple`), the
    /// aliquots — octave by octave against the voices.
    #[test]
    #[ignore = "a printout for voicing the harp"]
    fn print_dense_shares() {
        let stress: Vec<(u8, u8, f32, f32)> = (30u8..94).enumerate().map(|(i, p)| (p, 96, 0.05 + 0.02 * i as f32, 0.15 + 0.02 * i as f32)).collect();
        for (name, base, set) in shipped() {
            let mut only_own = set;
            let mut only_free = set;
            for s in only_own.keys.iter_mut() {
                s.couple = 0.0;
            }
            for s in only_free.keys.iter_mut() {
                s.own = 0.0;
            }
            only_own.aliquots = [HarpString::NONE; ALIQUOTS];
            only_free.aliquots = [HarpString::NONE; ALIQUOTS];
            for (part, st) in [("all", set), ("own only", only_own), ("free only", only_free)] {
                let (mono, _, harp) = play(&st, &base, &stress, &[(0.0, true)], 2.0);
                let (h, v) = (octave_bands(&harp, 1.0, 2.0), octave_bands(&mono, 1.0, 2.0));
                let rel: Vec<String> = h.iter().zip(&v).map(|(a, b)| format!("{:+.1}", 10.0 * (a / b.max(1e-30)).log10())).collect();
                println!("{name} {part}: harp re voices, octaves from 62.5 Hz, 1.0-2.0 s: {}", rel.join(" "));
            }
        }
    }


    /// Diagnostic: what rings on after a pedalled cluster's pedal lifts —
    /// the damped strings, the undamped treble, the aliquots.
    #[test]
    #[ignore = "a printout for voicing the harp"]
    fn print_lift_shares() {
        let cluster = [36u8, 43, 48, 52, 55, 60, 64, 67, 72, 76];
        for (name, base, set) in shipped() {
            let keys: Vec<(u8, u8, f32, f32)> = cluster.iter().enumerate().map(|(i, &k)| (k, 120u8, 0.1 + 0.009 * i as f32, 1.6)).collect();
            let mut damped_only = set;
            damped_only.aliquots = [HarpString::NONE; ALIQUOTS];
            for s in damped_only.keys.iter_mut() {
                if !s.damper {
                    s.couple = 0.0;
                    s.own = 0.0;
                }
            }
            let mut undamped_only = set;
            for s in undamped_only.keys.iter_mut() {
                if s.damper {
                    s.couple = 0.0;
                    s.own = 0.0;
                }
            }
            for (part, st) in [("all", set), ("damped strings", damped_only), ("undamped + aliquots", undamped_only)] {
                let (mono, _, harp) = play(&st, &base, &keys, &[(0.0, true), (3.0, false)], 4.2);
                let pre = level(&harp, 2.8, 3.0);
                let at = |t: f32| db(level(&harp, 3.0 + t, 3.05 + t) / level(&mono, 2.8, 3.0));
                println!("{name} {part}: harp before the lift {:+.1} dB re voices; after, re the voices before: +0.1 {:+.1} +0.3 {:+.1} +0.5 {:+.1} +1.0 {:+.1}", db(pre / level(&mono, 2.8, 3.0)), at(0.1), at(0.3), at(0.5), at(1.0));
                let v = |t: f32| db(level(&mono, 3.0 + t, 3.05 + t) / level(&mono, 2.8, 3.0));
                println!("{name} voices: +0.1 {:+.1} +0.3 {:+.1} +0.5 {:+.1} +1.0 {:+.1}", v(0.1), v(0.3), v(0.5), v(1.0));
            }
        }
    }


    /// Diagnostic: which strings carry a held note's halo and its tail —
    /// its own loop, the other strings, the aliquots.
    #[test]
    #[ignore = "a printout for voicing the harp"]
    fn print_halo_shares() {
        for (name, base, set) in shipped() {
            for (key, held) in [(60u8, 2.0_f32), (91, 3.0), (96, 3.0)] {
                let mut own = set;
                own.aliquots = [HarpString::NONE; ALIQUOTS];
                for (i, s) in own.keys.iter_mut().enumerate() {
                    if FIRST_KEY + i as u8 != key {
                        s.couple = 0.0;
                        s.own = 0.0;
                    }
                }
                let mut others = set;
                others.aliquots = [HarpString::NONE; ALIQUOTS];
                others.keys[usize::from(key - FIRST_KEY)].own = 0.0;
                others.keys[usize::from(key - FIRST_KEY)].couple = 0.0;
                let mut ali = set;
                for s in ali.keys.iter_mut() {
                    s.couple = 0.0;
                    s.own = 0.0;
                }
                for (part, st) in [("all", set), ("own loop", own), ("other strings", others), ("aliquots", ali)] {
                    let (mono, _, harp) = play(&st, &base, &[(key, 80, 0.05, 0.05 + held)], &[], 0.05 + held + 1.2);
                    let end = 0.05 + held;
                    let halo = db(level(&harp, 0.07, end) / level(&mono, 0.07, end));
                    let first = db(level(&harp, 0.07, 0.57) / level(&mono, 0.07, 0.57));
                    let last = db(level(&harp, end - 0.5, end) / level(&mono, end - 0.5, end));
                    let keyoff = level(&mono, end - 0.3, end);
                    let t05 = db(level(&harp, end + 0.45, end + 0.55) / keyoff);
                    println!("{name} key {key} {part}: halo {halo:+.1} (first {first:+.1} last {last:+.1}); tail +0.5 {t05:+.1}");
                }
            }
        }
    }


    /// Diagnostic: which strings make the pedal's wash between a C2's
    /// partials (L/R power, the bench's way), and which make a dense
    /// passage's bass.
    #[test]
    #[ignore = "a printout for voicing the harp"]
    fn print_wash_by_register() {
        let c2 = f_equal(36) * 2.0_f32.powf(-2.8 / 1200.0);
        let stress: Vec<(u8, u8, f32, f32)> = (30u8..94).enumerate().map(|(i, p)| (p, 96, 0.05 + 0.02 * i as f32, 0.15 + 0.02 * i as f32)).collect();
        for (name, base, full) in shipped() {
            for (lo, hi) in [(21u8, 108u8), (21, 35), (36, 47), (48, 59), (60, 71), (72, 108)] {
                let mut set = full;
                for (i, s) in set.keys.iter_mut().enumerate() {
                    let k = FIRST_KEY + i as u8;
                    if k < lo || k > hi {
                        s.couple = 0.0;
                    }
                }
                set.aliquots = [HarpString::NONE; ALIQUOTS];
                let (l, r, _, harp) = play_lr(&set, &base, &[(36, 100, 0.05, 1.05)], &[(0.0, true)], 1.1);
                let (a, b) = (secs(0.35), secs(1.0));
                let wl: Vec<f32> = l[a..b].iter().zip(&harp[a..b]).map(|(x, h)| x + h).collect();
                let wr: Vec<f32> = r[a..b].iter().zip(&harp[a..b]).map(|(x, h)| x + h).collect();
                let pw = |f: f32, span: f32| {
                    let (ma, _) = search_peak(&wl, f, span, RATE);
                    let (mb, _) = search_peak(&wr, f, span, RATE);
                    10.0 * (0.5 * (ma * ma + mb * mb)).max(1e-30).log10()
                };
                let n1 = pw(c2, 0.003);
                let mut lv: Vec<f32> = (1..12).map(|k| pw((k as f32 + 0.5) * c2 * 1.001, 0.02) - n1).collect();
                lv.sort_by(|a, b| a.partial_cmp(b).unwrap());
                let (mono, _, h2) = play(&set, &base, &stress, &[(0.0, true)], 2.0);
                let (hb, vb) = (octave_bands(&h2, 1.0, 2.0), octave_bands(&mono, 1.0, 2.0));
                let bass: Vec<String> = hb.iter().zip(&vb).take(3).map(|(x, y)| format!("{:+.1}", 10.0 * (x / y.max(1e-30)).log10())).collect();
                println!("{name} free strings {lo}..{hi}: wash median {:+.1} dB re n1; stress bands 62/125/250 Hz {}", lv[5], bass.join(" "));
            }
        }
    }


    /// Diagnostic: which loops carry the harp through the thirty seconds of
    /// pedalled chords, second by second (the loudest few by energy).
    #[test]
    #[ignore = "a printout for voicing the harp"]
    fn print_storm_loops() {
        let chords = [[36u8, 43, 48, 52, 55, 60, 64, 67], [41, 48, 53, 57, 60, 65, 69, 72], [31, 38, 43, 47, 50, 55, 59, 62]];
        let mut keys = Vec::new();
        for c in 0..20 {
            for &k in &chords[c % 3] {
                keys.push((k + (c / 3) as u8 % 2 * 12, 127u8, 0.05 + 1.5 * c as f32, 0.05 + 1.5 * c as f32 + 1.2));
            }
        }
        let base = GrandBase::INTIMATE;
        let set = StringSet::grand(&base, 0.4, &GrandHarp::INTIMATE);
        let mut h = Harp::new(RATE);
        h.string(&set);
        h.set_pedal(true);
        let mut voices: Vec<(GrandVoice, u8, bool)> = keys.iter().map(|k| (GrandVoice::new(RATE), k.0, false)).collect();
        let knobs = PianoKnobs::default();
        let n = secs(30.0);
        let (mut l, mut r, mut d) = ([0.0_f32; BLOCK], [0.0_f32; BLOCK], [0.0_f32; BLOCK]);
        let mut at = 0;
        let mut energy = vec![0.0_f64; LOOPS];
        while at < n {
            let m = BLOCK.min(n - at);
            let (t0, t1) = (at as f32 / RATE, (at + m) as f32 / RATE);
            for (j, &(key, vel, on, off)) in keys.iter().enumerate() {
                if on >= t0 && on < t1 {
                    voices[j].0.strike(key, f32::from(vel) / 127.0, &knobs, &base);
                    voices[j].2 = true;
                    h.key_down(key);
                }
                if off >= t0 && off < t1 {
                    voices[j].0.release(true);
                    h.key_up(key);
                }
            }
            l.fill(0.0);
            r.fill(0.0);
            d.fill(0.0);
            for (v, _, _) in voices.iter_mut() {
                v.render(&mut l[..m], &mut r[..m], &mut d[..m]);
            }
            let mut hb = [0.0_f32; BLOCK];
            h.render(&d[..m], &mut hb[..m]);
            recount(&mut voices, &mut h);
            for (i, lp) in h.loops.iter().enumerate() {
                if lp.state == State::Ticking {
                    let a = lp.off as usize;
                    let e: f64 = h.arena[a..a + lp.n as usize].iter().map(|v| f64::from(*v).powi(2)).sum::<f64>() / f64::from(lp.n);
                    energy[i] += e * f64::from(lp.cout).powi(2);
                }
            }
            at += m;
            if at % secs(1.0) < m {
                let mut idx: Vec<usize> = (0..LOOPS).collect();
                idx.sort_by(|&a, &b| energy[b].partial_cmp(&energy[a]).unwrap());
                let total: f64 = energy.iter().sum();
                let top: Vec<String> = idx[..4].iter().map(|&i| format!("{}{}{}:{:+.0}", key_of(i), if i >= KEYS { "a" } else { "" }, if h.loops[i].rung { "r" } else if h.loops[i].voiced { "v" } else { "" }, 10.0 * (energy[i] / total.max(1e-30)).log10())).collect();
                println!("second {:2}: harp energy {:+.1} dB; top loops {}", at / secs(1.0), 10.0 * total.max(1e-30).log10(), top.join(" "));
                energy.iter_mut().for_each(|e| *e = 0.0);
            }
        }
    }

    /// Diagnostic: how many loops run through the bench's stress score —
    /// sixty-four keys 20 ms apart under the pedal — and through a pedal-up
    /// staccato passage.
    #[test]
    #[ignore = "a printout for the harp's cost"]
    fn print_loops_running() {
        let base = GrandBase::INTIMATE;
        let set = StringSet::grand(&base, 0.4, &GrandHarp::INTIMATE);
        let stress: Vec<(u8, u8, f32, f32)> = (30u8..94).enumerate().map(|(i, p)| (p, 96, 0.05 + 0.02 * i as f32, 0.15 + 0.02 * i as f32)).collect();
        let passage: Vec<(u8, u8, f32, f32)> = (0..40).map(|i| (48 + ((i * 7) % 24) as u8, 70, 0.05 + 0.15 * i as f32, 0.15 + 0.15 * i as f32)).collect();
        for (name, keys, pedal, total) in [("stress", &stress, vec![(0.0_f32, true)], 7.4_f32), ("staccato", &passage, vec![], 7.0)] {
            let mut h = Harp::new(RATE);
            h.string(&set);
            let mut voices: Vec<(GrandVoice, u8, bool)> = keys.iter().map(|k| (GrandVoice::new(RATE), k.0, false)).collect();
            let knobs = PianoKnobs::default();
            let n = secs(total);
            let (mut l, mut r, mut d) = ([0.0_f32; BLOCK], [0.0_f32; BLOCK], [0.0_f32; BLOCK]);
            let mut at = 0;
            let mut samples = Vec::new();
            for &(_, p) in &pedal {
                h.set_pedal(p);
            }
            let down = !pedal.is_empty();
            while at < n {
                let m = BLOCK.min(n - at);
                let (t0, t1) = (at as f32 / RATE, (at + m) as f32 / RATE);
                for (j, &(key, vel, on, off)) in keys.iter().enumerate() {
                    if on >= t0 && on < t1 {
                        voices[j].0.strike(key, f32::from(vel) / 127.0, &knobs, &base);
                        voices[j].2 = true;
                        h.key_down(key);
                    }
                    if off >= t0 && off < t1 {
                        voices[j].0.release(down);
                        h.key_up(key);
                    }
                }
                l.fill(0.0);
                r.fill(0.0);
                d.fill(0.0);
                for (v, _, _) in voices.iter_mut() {
                    v.render(&mut l[..m], &mut r[..m], &mut d[..m]);
                }
                let mut hb = [0.0_f32; BLOCK];
                h.render(&d[..m], &mut hb[..m]);
                recount(&mut voices, &mut h);
                if at % secs(0.5) < m {
                    samples.push(h.ticking());
                }
                at += m;
            }
            println!("{name}: {:.1} loops per sample on average; every 0.5 s: {:?}", h.ticks() as f64 / n as f64, samples);
        }
    }

    /// **Cost, measured.** Nanoseconds per sample: pedal up and nothing
    /// held; all hundred and seven forced ringing; what designing a set
    /// costs (`Strung::new`, done where time is not short) and what loading
    /// one costs `set_patch` on the audio thread.
    ///
    /// **Ignored because it is a wall clock, not a computation**: the suite
    /// runs its tests in parallel and every other test is a competing
    /// thread. Run it alone on an idle machine — `cargo test --release --
    /// --ignored cost_is_measured --nocapture`. The instrument's own figure
    /// is `keysrig <patch> stress`.
    #[test]
    #[ignore = "a wall-clock measurement; the parallel suite is its own load. Run it alone on an idle machine."]
    fn cost_is_measured() {
        let set = intimate();
        let n = secs(4.0);
        let mut seed = 0x5EED_u32;
        let mut noise = || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            (seed as f32 / u32::MAX as f32 - 0.5) * 0.2
        };
        let input: Vec<f32> = (0..n).map(|_| noise()).collect();
        let time = |h: &mut Harp| {
            let mut out = vec![0.0_f32; BLOCK];
            let t = std::time::Instant::now();
            for c in input.chunks(BLOCK) {
                h.render(c, &mut out[..c.len()]);
            }
            std::hint::black_box(&out);
            t.elapsed().as_nanos() as f64 / n as f64
        };
        let mut h = Harp::new(RATE);
        h.string(&set);
        let idle = time(&mut h);
        let mut h = Harp::new(RATE);
        h.string(&set);
        h.set_pedal(true);
        for k in 21..=108u8 {
            h.key_down(k);
            h.voice_done(k);
        }
        let ticking = h.ticking();
        let all = time(&mut h);
        let sets = [StringSet::grand(&GrandBase::INTIMATE, 0.4, &GrandHarp::INTIMATE), StringSet::grand(&GrandBase::CONCERT, 1.0, &GrandHarp::CONCERT)];
        let t = std::time::Instant::now();
        let mut strung = Vec::new();
        for i in 0..20 {
            strung.push(h.strung(&sets[i % 2]));
        }
        let design_us = t.elapsed().as_nanos() as f64 / 20.0 / 1000.0;
        let t = std::time::Instant::now();
        for i in 0..1000 {
            h.load(&strung[i % 2]);
        }
        let load_us = t.elapsed().as_nanos() as f64 / 1000.0 / 1000.0;
        println!("harp: idle {idle:.1} ns/sample; {ticking} loops ringing {all:.1} ns/sample = {:.2} ns per loop; designing a set {design_us:.0} us, loading one {load_us:.1} us (a 128-sample block is 2667 us)", all / ticking as f64);
        assert!(idle < 10.0);
        assert!(load_us < 50.0, "loading a set takes {load_us:.1} us");
    }
    /// **A cleared harp is a new one, and a hushed one keeps its keys and
    /// nothing else.** On both grands' sets, twelve histories: keys struck
    /// and let go at random (some let go held, some reported done), the
    /// sustain at random levels, the bridge driven at a random level for a
    /// random time. Then either `clear`, against a new harp given the same
    /// set and the same pedal (the pressure is the foot's and is kept on
    /// purpose); or the frame's all notes off (`release_all`), a key or two
    /// struck while the hush fades, rendered, then `hush`, against a new
    /// harp given the same pedal and the same key events (no audio) and
    /// hushed too. From there the same keys, pedal and drive render the two
    /// to the bit, through 160 blocks.
    #[test]
    fn a_cleared_harp_is_a_new_one() {
        let mut seed = 0x51ab_2c3d_u32;
        let mut rnd = move || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed as f32 / u32::MAX as f32
        };
        // A bridge sum with something for every band: three keys' partials
        // and a little broadband, at `level`.
        let drive = |i: usize, level: f32, noise: f32| {
            let t = i as f32 / RATE;
            let tone: f32 = [36u8, 60, 79].iter().map(|&k| (1..=5).map(|n| (std::f32::consts::TAU * f_equal(k) * n as f32 * t).sin() / n as f32).sum::<f32>()).sum();
            level * (0.1 * tone + noise)
        };
        for (name, _, set) in shipped() {
            let strung = Harp::new(RATE).strung(&set);
            for trial in 0..12 {
                let mut used = Harp::new(RATE);
                used.load(&strung);
                let mut sustain = 0.0_f32;
                let level = 10.0_f32.powf(-2.0 * rnd());
                let mut at = 0;
                let mut out = [0.0_f32; BLOCK];
                for _ in 0..(20 + (rnd() * 300.0) as usize) {
                    let key = 30 + (rnd() * 70.0) as u8;
                    match (rnd() * 8.0) as u32 {
                        0 | 1 => used.key_down(key),
                        2 => used.key_up(key),
                        3 => used.key_up_held(key),
                        4 => used.voice_done(key),
                        5 => {
                            sustain = [0.0, 0.3, 0.6, 1.0][(rnd() * 4.0) as usize % 4];
                            used.set_sustain(sustain);
                        }
                        _ => {}
                    }
                    let d: Vec<f32> = (at..at + BLOCK).map(|i| drive(i, level, 0.05 * (2.0 * rnd() - 1.0))).collect();
                    used.render(&d, &mut out);
                    at += BLOCK;
                }
                let mut fresh = Harp::new(RATE);
                fresh.load(&strung);
                fresh.set_sustain(sustain);
                let hushing = trial % 2 == 1;
                if hushing {
                    // All notes off, and keys struck while it fades: one held,
                    // one let go, as the frame tells them to the strings.
                    used.release_all();
                    let fade: [(u8, bool); 3] = [(72, true), (67, true), (67, false)];
                    for (k, down) in fade {
                        for h in [&mut used, &mut fresh] {
                            if down {
                                h.key_down(k);
                            } else {
                                h.key_up(k);
                            }
                        }
                        let d: Vec<f32> = (at..at + BLOCK).map(|i| drive(i, level, 0.0)).collect();
                        used.render(&d, &mut out);
                        at += BLOCK;
                    }
                    used.hush();
                    fresh.hush();
                } else {
                    used.clear();
                }
                let (mut oa, mut ob) = ([0.0_f32; BLOCK], [0.0_f32; BLOCK]);
                for b in 0..160 {
                    for h in [&mut used, &mut fresh] {
                        match b {
                            3 => h.key_down(48),
                            9 => h.set_sustain(1.0),
                            40 => h.key_up(48),
                            70 => h.key_up(72),
                            100 => h.set_sustain(0.4),
                            _ => {}
                        }
                    }
                    let d: Vec<f32> = (0..BLOCK).map(|i| drive(i + b * BLOCK, 0.5, 0.0)).collect();
                    oa.fill(0.0);
                    ob.fill(0.0);
                    used.render(&d, &mut oa);
                    fresh.render(&d, &mut ob);
                    for i in 0..BLOCK {
                        assert!(oa[i].to_bits() == ob[i].to_bits(), "{name}, trial {trial} ({}): the harp differs from a new one at block {b}, sample {i} ({:e} against {:e})", if hushing { "hushed" } else { "cleared" }, oa[i], ob[i]);
                    }
                }
            }
        }
    }

    // ── STRETCH and the harp: the measurement behind its following ────────

    /// A voice of either kind the harp is strung for: the modal grands', or
    /// the electric grand's.
    enum Struck {
        Grand(GrandVoice),
        Electric(Box<super::super::egrand::EGrandVoice>),
    }

    /// The patch's own centre set, at its own send: what `Keys::new` strings.
    fn centre_set(patch: super::super::Patch) -> StringSet {
        use super::super::Patch;
        match patch {
            Patch::Piano1 => StringSet::grand(&GrandBase::INTIMATE, super::super::HARP_SEND_INTIMATE, &GrandHarp::INTIMATE),
            Patch::Piano2 => StringSet::grand(&GrandBase::CONCERT, super::super::HARP_SEND_CONCERT, &GrandHarp::CONCERT),
            _ => super::super::egrand_strings(),
        }
    }

    /// The patch's set as the frame strings it at stretch `s`, string by
    /// string (`grand_string`, `egrand::harp_line`): what `Strung::follow`
    /// brings the loaded harp to.
    fn frame_set(patch: super::super::Patch, s: f32) -> StringSet {
        use super::super::{egrand, Patch};
        let grand = |base: &GrandBase, voice: &GrandHarp, send: f32| {
            let tune = grand::tuning(base);
            StringSet {
                keys: std::array::from_fn(|i| grand_string(base, voice, &tune, i, s)),
                aliquots: std::array::from_fn(|j| grand_string(base, voice, &tune, KEYS + j, s)),
                send,
            }
        };
        match patch {
            Patch::Piano1 => grand(&GrandBase::INTIMATE, &GrandHarp::INTIMATE, super::super::HARP_SEND_INTIMATE),
            Patch::Piano2 => grand(&GrandBase::CONCERT, &GrandHarp::CONCERT, super::super::HARP_SEND_CONCERT),
            _ => {
                let tune = egrand::harp_tune();
                let keys: [HarpString; KEYS] = std::array::from_fn(|i| super::super::egrand_string(&egrand::harp_line(&tune, i, s)));
                StringSet { keys, aliquots: [HarpString { couple: 0.0, own: 0.0, ..keys[0] }; ALIQUOTS], send: super::super::HARP_SEND_ELECTRIC }
            }
        }
    }

    /// The patch's stretched tuning at STRETCH centre, cents from equal
    /// temperament, and its B, per key from A0.
    fn centre_tuning(patch: super::super::Patch) -> ([f32; KEYS], [f32; KEYS]) {
        use super::super::Patch;
        match patch {
            Patch::Piano1 => grand::tuning(&GrandBase::INTIMATE),
            Patch::Piano2 => grand::tuning(&GrandBase::CONCERT),
            _ => super::super::egrand::tuning(),
        }
    }

    /// `keys` (pitch, MIDI velocity, strike s, release s) on `patch` with
    /// STRETCH at `knob`, its voices into a harp strung with `set`, the
    /// sustain pedal's transitions as `play_lr` takes them: the voices' mono
    /// sum and the harp's return, each on its own.
    fn stretched_play(patch: super::super::Patch, knob: f32, set: &StringSet, keys: &[(u8, u8, f32, f32)], pedal: &[(f32, bool)], total: f32) -> (Vec<f32>, Vec<f32>) {
        use super::super::egrand::{EGrandBase, EGrandKnobs, EGrandVoice};
        use super::super::Patch;
        let mut p = patch.defaults();
        p[5] = knob;
        let (piano, electric) = (PianoKnobs::from_params(&p), EGrandKnobs::from_params(&p));
        let base = if patch == Patch::Piano2 { GrandBase::CONCERT } else { GrandBase::INTIMATE };
        let mut h = Harp::new(RATE);
        h.string(set);
        let mut voices: Vec<(Struck, u8, bool)> = keys
            .iter()
            .map(|k| {
                let v = if patch == Patch::ElectricGrand { Struck::Electric(Box::new(EGrandVoice::new(RATE))) } else { Struck::Grand(GrandVoice::new(RATE)) };
                (v, k.0, false)
            })
            .collect();
        let n = secs(total);
        let (mut mono, mut harp) = (vec![0.0; n], vec![0.0; n]);
        let (mut l, mut r, mut d) = ([0.0_f32; BLOCK], [0.0_f32; BLOCK], [0.0_f32; BLOCK]);
        let mut at = 0;
        let mut down = false;
        let finished = |v: &Struck| match v {
            Struck::Grand(g) => g.finished(),
            Struck::Electric(e) => e.finished(),
        };
        while at < n {
            let m = BLOCK.min(n - at);
            let (t0, t1) = (at as f32 / RATE, (at + m) as f32 / RATE);
            for &(when, dn) in pedal {
                if when >= t0 && when < t1 && dn != down {
                    down = dn;
                    h.set_pedal(dn);
                    if !dn {
                        for (v, k, _) in voices.iter_mut() {
                            if !keys.iter().any(|q| q.0 == *k && q.2 <= t0 && q.3 > t0) {
                                match v {
                                    Struck::Grand(g) => g.damp(),
                                    Struck::Electric(e) => e.damp(),
                                }
                            }
                        }
                    }
                }
            }
            for (j, &(key, vel, on, off)) in keys.iter().enumerate() {
                let v = f32::from(vel) / 127.0;
                if on >= t0 && on < t1 {
                    match &mut voices[j].0 {
                        Struck::Grand(g) => g.strike(key, v, &piano, &base),
                        Struck::Electric(e) => e.strike(key, v, &electric, &EGrandBase::ELECTRIC),
                    }
                    voices[j].2 = true;
                    h.key_down(key);
                }
                if off >= t0 && off < t1 {
                    match &mut voices[j].0 {
                        Struck::Grand(g) => g.release(down),
                        Struck::Electric(e) => e.release(down),
                    }
                    h.key_up(key);
                }
            }
            l.fill(0.0);
            r.fill(0.0);
            d.fill(0.0);
            for (v, _, _) in voices.iter_mut() {
                match v {
                    Struck::Grand(g) => g.render(&mut l[..m], &mut r[..m], &mut d[..m]),
                    Struck::Electric(e) => e.render(&mut l[..m], &mut r[..m], &mut d[..m]),
                }
            }
            let mut hb = [0.0_f32; BLOCK];
            h.render(&d[..m], &mut hb[..m]);
            for i in 0..m {
                mono[at + i] = 0.5 * (l[i] + r[i]);
            }
            harp[at..at + m].copy_from_slice(&hb[..m]);
            for j in 0..voices.len() {
                if voices[j].2 && finished(&voices[j].0) {
                    voices[j].2 = false;
                    let key = voices[j].1;
                    if !voices.iter().any(|(v, k, live)| *live && *k == key && !finished(v)) {
                        h.voice_done(key);
                    }
                }
            }
            at += m;
        }
        (mono, harp)
    }

    /// **What the harp does at either end of STRETCH, strung at its centre
    /// and strung where the frame strings it.** STRETCH scales a grand's B
    /// and its stretched tuning together (`grand::GrandVoice::build`, and
    /// the electric grand's), from 0.33 at the knob's bottom to 3.03 at its
    /// top; its harp was strung once, at 1. Printed, per shipped piano and
    /// the knob at 0, 0.5 and 1: how far each harp string's own partials,
    /// rung and measured, sit from its voice's (the voice's stretched series
    /// on its own B, which the frame's STRETCH tests hold the voices to);
    /// and what that does to the sound, with the harp strung at the centre
    /// (as it was before it followed the knob) beside the harp the frame
    /// strings at the knob (`frame_set`): the held note's
    /// halo, its tail after the key-up with the pedal up and its ring-off
    /// under the pedal, the undamped treble's own strings, the pedalled
    /// wash a staccato bass note leaves, and the intimate's aliquots. Each
    /// is the harp's return against the voices' in the same window, dB.
    /// Read against a criterion fixed before it was first run: audible if a
    /// harp partial within 20 dB of the harp's strongest stands more than
    /// 10 cents from its voice's, or any of these measures moves more than
    /// 3 dB from the stringing that follows the voices. Strung at the
    /// centre, all three pianos failed both at either end of the knob.
    #[test]
    #[ignore = "a measurement: run it to print the table"]
    fn stretch_and_the_harp() {
        use super::super::{bipolar, Patch};
        let db_of = |a: &[f32], b: &[f32]| 20.0 * (rms(a) / rms(b).max(1e-12)).max(1e-12).log10();
        let w = |x: &[f32], a: f32, b: f32| -> Vec<f32> { x[secs(a)..secs(b)].to_vec() };
        for patch in [Patch::Piano1, Patch::Piano2, Patch::ElectricGrand] {
            let (tune, b0) = centre_tuning(patch);
            for knob in [0.0_f32, 0.5, 1.0] {
                let s = bipolar(knob, 1.6);
                // The strings, rung: each key's fundamental and partials 2,
                // 4 and 8 against its voice's, for both stringings.
                for (name, set) in [("centre", centre_set(patch)), ("frame", frame_set(patch, s))] {
                    let mut h = Harp::new(RATE);
                    h.string(&set);
                    let mut line = format!("DETUNE {patch:?} knob {knob} (stretch {s:.3}) {name}:");
                    let mut worst = (0.0_f32, 0u8, 0usize);
                    for key in 21..=108u8 {
                        let i = usize::from(key - FIRST_KEY);
                        let fe = 440.0 * 2.0_f32.powf((f32::from(key) - 69.0) / 12.0);
                        let bv = b0[i] * s;
                        let fv = fe * 2.0_f32.powf(tune[i] * s / 1200.0);
                        let voice_n = |n: f32| n * fv / (1.0 + bv).sqrt() * (1.0 + bv * n * n).sqrt();
                        let x = ring_loop(&mut h, i, secs(0.6), false);
                        let (bh, fh) = (set.keys[i].b, set.keys[i].f0);
                        let harp_n = |n: f32| n * fh / (1.0 + bh).sqrt() * (1.0 + bh * n * n).sqrt();
                        let mut row = String::new();
                        for n in [1.0_f32, 2.0, 4.0, 8.0] {
                            if voice_n(n) > 0.4 * RATE || harp_n(n) > 0.4 * RATE {
                                continue;
                            }
                            let (_, f) = search_peak(&x, harp_n(n), 0.01, RATE);
                            let (_, f) = search_peak(&x, f, 0.0008, RATE);
                            let (_, f) = search_peak(&x, f, 0.00008, RATE);
                            let c = cents(f, voice_n(n));
                            if c.abs() > worst.0.abs() {
                                worst = (c, key, n as usize);
                            }
                            row.push_str(&format!(" n{n}:{c:+.1}"));
                        }
                        if [21u8, 24, 36, 48, 60, 72, 84, 96, 105, 108].contains(&key) {
                            line.push_str(&format!(" | {key}{row}"));
                        }
                    }
                    println!("{line}");
                    println!("DETUNE {patch:?} knob {knob} {name}: worst over 88 keys and n = 1, 2, 4, 8: {:+.1} cents (key {} n{})", worst.0, worst.1, worst.2);
                    if patch == Patch::Piano1 {
                        let mut row = format!("DETUNE {patch:?} knob {knob} {name} aliquots, cents from the voice's fundamental:");
                        for key in [90u8, 91, 96, 102, 108] {
                            let i = usize::from(key - FIRST_KEY);
                            let j = KEYS + usize::from(key - ALIQUOT_FIRST);
                            let fe = 440.0 * 2.0_f32.powf((f32::from(key) - 69.0) / 12.0);
                            let fv = fe * 2.0_f32.powf(tune[i] * s / 1200.0);
                            let x = ring_loop(&mut h, j, secs(0.6), false);
                            let (_, f) = locate(&x, set.aliquots[j - KEYS].f0, RATE);
                            row.push_str(&format!(" {key}:{:+.1}", cents(f, fv)));
                        }
                        println!("{row}");
                    }
                }
                // The sound, both stringings.
                for (name, set) in [("centre", centre_set(patch)), ("frame", frame_set(patch, s))] {
                    let mut row = format!("SOUND {patch:?} knob {knob} {name}:");
                    for key in [36u8, 60, 84] {
                        // Held 1.2 s, pedal up, let go: the halo, then the tail.
                        let (v, hp) = stretched_play(patch, knob, &set, &[(key, 80, 0.0, 1.2)], &[], 1.9);
                        let all: Vec<f32> = v.iter().zip(&hp).map(|(a, b)| a + b).collect();
                        let halo = db_of(&w(&hp, 0.1, 1.0), &w(&v, 0.1, 1.0));
                        let tail = db_of(&w(&all, 1.3, 1.8), &w(&all, 0.6, 1.1));
                        let share = db_of(&w(&hp, 1.3, 1.8), &w(&v, 1.3, 1.8));
                        // Under the pedal, let go at 0.4 s: the ring-off.
                        let (v, hp) = stretched_play(patch, knob, &set, &[(key, 80, 0.0, 0.4)], &[(0.0, true)], 2.0);
                        let ring = db_of(&w(&hp, 1.0, 2.0), &w(&v, 1.0, 2.0));
                        row.push_str(&format!(" | {key} halo {halo:+.1} tail {tail:+.1} (harp {share:+.1}) pedalled ring {ring:+.1}"));
                    }
                    // The undamped treble: struck, let go, its own string.
                    for key in [96u8, 105, 108] {
                        let (v, hp) = stretched_play(patch, knob, &set, &[(key, 80, 0.0, 0.3)], &[], 1.0);
                        let own = db_of(&w(&hp, 0.35, 0.95), &w(&v, 0.35, 0.95));
                        let i = usize::from(key - FIRST_KEY);
                        let fe = 440.0 * 2.0_f32.powf((f32::from(key) - 69.0) / 12.0);
                        let fv = fe * 2.0_f32.powf(tune[i] * s / 1200.0);
                        let seg = w(&hp, 0.35, 0.95);
                        let (_, f) = search_peak(&seg, fv, 0.07, RATE);
                        let (_, f) = search_peak(&seg, f, 0.002, RATE);
                        row.push_str(&format!(" | {key} own {own:+.1} at {:+.1} c", cents(f, fv)));
                    }
                    // The wash a pedalled staccato C2 leaves in the harp.
                    let (v, hp) = stretched_play(patch, knob, &set, &[(36, 100, 0.0, 0.1)], &[(0.0, true)], 1.6);
                    row.push_str(&format!(" | wash {:+.1}", db_of(&w(&hp, 0.5, 1.5), &w(&v, 0.5, 1.5))));
                    // The aliquots: the harp with them against without.
                    if patch == Patch::Piano1 {
                        let bare = StringSet { aliquots: [HarpString::NONE; ALIQUOTS], ..set };
                        for key in [91u8, 96, 102] {
                            let (v, with) = stretched_play(patch, knob, &set, &[(key, 80, 0.0, 1.0)], &[], 1.0);
                            let (_, without) = stretched_play(patch, knob, &bare, &[(key, 80, 0.0, 1.0)], &[], 1.0);
                            let aliq: Vec<f32> = with.iter().zip(&without).map(|(a, b)| a - b).collect();
                            row.push_str(&format!(" | {key} aliquot {:+.1}", db_of(&w(&aliq, 0.1, 0.9), &w(&v, 0.1, 0.9))));
                        }
                    }
                    println!("{row}");
                }
            }
        }
    }

    /// The frame as the host drives it, for the STRETCH tests: `set_params`
    /// with the block's frames, then `render` while anything sounds and
    /// `idle` while nothing does; `blocks` of 128 frames, the knobs `p`.
    fn host_blocks(k: &mut super::super::Keys, patch: super::super::Patch, p: &[f32; super::super::MAX_PARAMS], cc: [u8; 3], blocks: usize, out: &mut Vec<f32>) {
        let mut buf = [0.0_f32; 2 * BLOCK];
        for _ in 0..blocks {
            k.set_patch(patch);
            k.set_params(p, BLOCK);
            k.set_pedals(cc);
            buf.fill(0.0);
            if k.active() {
                k.render(&mut buf, BLOCK, 2);
            } else {
                k.idle(BLOCK);
            }
            out.extend(buf.chunks(2).map(|c| c[0]));
        }
    }

    /// **The harp is strung where STRETCH tunes the voices.** Each patch with
    /// a harp, the knob to 0, to 1 and back to its centre, the frame driven
    /// as the host drives it: after a sixth of a second of audio every string
    /// of the loaded harp, and of the set the patch loads (`Keys::harp_sets`,
    /// which a patch change and back must find where it was left), is
    /// designed exactly as the instrument strings it at the knob
    /// (`grand_string`, `egrand::harp_line`); back at the centre, exactly as
    /// `Keys::new` strung it. Strung at the centre whatever the knob, its
    /// strings missed their voices by up to 84 cents at the fundamental and
    /// hundreds above (`stretch_and_the_harp`).
    #[test]
    fn the_harp_follows_the_stretch_knob() {
        use super::super::{bipolar, Keys, Patch};
        for (set, patch) in [(0usize, Patch::Piano1), (1, Patch::Piano2), (2, Patch::ElectricGrand)] {
            let mut k = Keys::new(RATE);
            let mut out = Vec::new();
            for knob in [0.0_f32, 1.0, 0.5] {
                let mut p = patch.defaults();
                p[5] = knob;
                host_blocks(&mut k, patch, &p, [0; 3], 60, &mut out);
                let want = Strung::new(&frame_set(patch, bipolar(knob, 1.6)), RATE);
                let centre = Strung::new(&centre_set(patch), RATE);
                let mut wrong = Vec::new();
                for i in 0..LOOPS {
                    // A loop with no coupling is a string the set does not
                    // have, and is never re-strung.
                    if centre.coef[i].couple == 0.0 && centre.coef[i].own == 0.0 {
                        continue;
                    }
                    let l = &k.harp.loops[i];
                    let live = (l.back, l.frac.a, l.loss.a, l.disp.a, l.gain_free, l.norm, l.norm_s, l.band);
                    let c = want.coef[i];
                    let designed = (c.d.back, c.d.frac, c.d.loss, c.d.disp, c.d.gain_free, c.norm, c.norm_s, c.band);
                    if k.harp.designs[i] != c || k.harp_sets[set].coef[i] != c || live != designed {
                        wrong.push(i);
                    }
                }
                println!("{patch:?} STRETCH {knob}: {} strings not where the frame strings them", wrong.len());
                assert!(wrong.is_empty(), "{patch:?} at STRETCH {knob}: loops {wrong:?} are not strung for the knob");
                if knob == 0.5 {
                    assert!(k.harp_sets[set].coef == centre.coef, "{patch:?}: back at the centre the set is not the one `Keys::new` strung");
                }
            }
            // A patch change and back loads the set where its knob left it.
            let mut p = patch.defaults();
            p[5] = 0.8;
            host_blocks(&mut k, patch, &p, [0; 3], 60, &mut out);
            let other = if patch == Patch::Piano1 { Patch::Piano2 } else { Patch::Piano1 };
            host_blocks(&mut k, other, &other.defaults(), [0; 3], 4, &mut out);
            k.set_patch(patch);
            let want = Strung::new(&frame_set(patch, bipolar(0.8, 1.6)), RATE);
            let stale = (0..LOOPS).filter(|&i| want.coef[i].couple + want.coef[i].own > 0.0 && k.harp.designs[i] != want.coef[i]).count();
            assert_eq!(stale, 0, "{patch:?}: a patch change and back loaded {stale} strings not as its knob left them");
        }
    }

    /// **A ringing string is re-tuned without a click.** A pedalled chord
    /// ringing on each patch with a harp, STRETCH jumped from its centre to
    /// 1, put back (a double-click), then dragged to 0 over half a second,
    /// the frame driven as the host drives it, its harp's part of the output
    /// read against a twin whose harp is silent. Over the 100 ms after each
    /// move, the harp's fourth difference against the 100 ms before it, and
    /// the output's second: as the same chord's unmoved vary (0.24 to 1.16
    /// times, 0.71 to 1.03), where re-stringing the ringing strings at once
    /// was a step (the harp's 4.3 to 290 times, the electric grand's output
    /// 3.4). Measured, gliding: 0.65 to 1.01, and 0.68 to 1.03.
    #[test]
    fn a_ringing_string_is_retuned_without_a_click() {
        use super::super::{Keys, Patch};
        let d2 = |x: &[f32]| x.windows(3).map(|w| (w[2] - 2.0 * w[1] + w[0]).abs()).fold(0.0_f32, f32::max);
        let d4 = |x: &[f32]| x.windows(5).map(|w| (w[4] - 4.0 * w[3] + 6.0 * w[2] - 4.0 * w[1] + w[0]).abs()).fold(0.0_f32, f32::max);
        // Per move, (the harp's fourth difference, the output's second)
        // after against before; and how many strings were re-strung
        // ringing.
        let run = |patch: Patch, at_once: bool| -> (Vec<(f32, f32)>, u32) {
            let mut with = Keys::new(RATE);
            let mut bare = Keys::new(RATE);
            for k in [&mut with, &mut bare] {
                k.set_patch(patch);
                k.harp.retune_at_once = at_once;
                for key in [36u8, 48, 55, 60, 64, 67, 72, 84, 96, 103] {
                    k.note_on(key, 0.7);
                }
            }
            let silent = bare.harp.strung(&StringSet { send: 0.0, ..centre_set(patch) });
            bare.harp.load(&silent);
            let (mut a, mut b) = (Vec::new(), Vec::new());
            let mut p = patch.defaults();
            let mut moves = Vec::new();
            for block in 0..(2.3 * RATE) as usize / BLOCK {
                let t = (block * BLOCK) as f32 / RATE;
                let seg = [0.5_f32, 1.0, 1.5].iter().filter(|&&m| t >= m).count();
                if seg > moves.len() {
                    moves.push(block * BLOCK);
                }
                p[5] = match seg {
                    0 | 2 => 0.5,
                    1 => 1.0,
                    _ => (0.5 - (t - 1.5)).max(0.0),
                };
                host_blocks(&mut with, patch, &p, [127, 0, 0], 1, &mut a);
                host_blocks(&mut bare, patch, &p, [127, 0, 0], 1, &mut b);
            }
            let harp: Vec<f32> = a.iter().zip(&b).map(|(x, y)| x - y).collect();
            let ratios = moves
                .iter()
                .map(|&at| {
                    let (pre, post) = (at - secs(0.1)..at, at - 4..at + secs(0.1));
                    (d4(&harp[post.clone()]) / d4(&harp[pre.clone()]), d2(&a[post]) / d2(&a[pre]))
                })
                .collect();
            (ratios, with.harp.retunes[1])
        };
        for patch in [Patch::Piano1, Patch::Piano2, Patch::ElectricGrand] {
            let ((glide, ringing), (once, _)) = (run(patch, false), run(patch, true));
            let show = |v: &[(f32, f32)]| v.iter().map(|(h, o)| format!("{h:.2}x/{o:.2}x")).collect::<Vec<_>>().join(" ");
            println!("{patch:?}, {ringing} ringing strings re-strung; harp d4 / output d2 over the move, jump, back, drag: gliding {}; at once {}", show(&glide), show(&once));
            assert!(ringing > 100, "{patch:?}: only {ringing} ringing strings re-strung; the test proves nothing");
            for (&(h, o), &(h1, _)) in glide.iter().zip(&once) {
                assert!(h <= 1.5 && o <= 1.3, "{patch:?}: a move glided under the chord clicked: harp {h:.2}x, output {o:.2}x");
                assert!(h1 >= 3.0, "{patch:?}: re-strung at once the move was no step ({h1:.2}x); the measure sees nothing");
            }
        }
    }

    /// **A glide ends, at every rate.** A pedalled chord over the bass on
    /// each patch with a harp, STRETCH jumped to 1, the frame driven as the
    /// host drives it: a second on, no ringing string still glides. A glide
    /// moves by a fraction of what is left each sample, and in `f32` that
    /// stops once the step is under half a unit in the last place: the
    /// bass's all-pass delay, at its cap (`AP_TAU_MAX_SECS`, 38 samples at
    /// 96 kHz, 76 at 192), stopped 3.7 thousandths of a sample short of its
    /// design (15 at 192 kHz), wider than `GLIDE_DONE`, and the glide never
    /// ended while its string rang: 14, 11 and 9 strings of the three
    /// patches at 96 kHz, 54, 48 and 40 at 192, the harp 3 and 14 % dearer
    /// for it (verifier). 48 kHz is the control, where nothing stalled; 192
    /// is left out for the time it takes unoptimised.
    #[test]
    fn a_glide_ends_at_every_rate() {
        use super::super::{Keys, Patch};
        for rate in [48_000.0_f32, 96_000.0] {
            for patch in [Patch::Piano1, Patch::Piano2, Patch::ElectricGrand] {
                let mut k = Keys::new(rate);
                let mut out = Vec::new();
                let mut p = patch.defaults();
                k.set_patch(patch);
                for key in [21u8, 24, 26, 28, 31, 33, 36, 40, 45, 60] {
                    k.note_on(key, 0.8);
                }
                let blocks = |s: f32| (s * rate) as usize / BLOCK;
                host_blocks(&mut k, patch, &p, [127, 0, 0], blocks(0.2), &mut out);
                p[5] = 1.0;
                host_blocks(&mut k, patch, &p, [127, 0, 0], blocks(1.0), &mut out);
                let ringing = (0..LOOPS).filter(|&i| k.harp.loops[i].state == State::Ticking).count();
                let gliding = (0..LOOPS).filter(|&i| k.harp.loops[i].state == State::Ticking && k.harp.glides[i].on).count();
                println!("{patch:?} at {rate} Hz: {ringing} strings ringing a second after STRETCH moved, {gliding} still gliding");
                assert!(ringing > 20, "{patch:?} at {rate} Hz: only {ringing} strings ringing; the test proves nothing");
                assert_eq!(gliding, 0, "{patch:?} at {rate} Hz: glides that never ended while their strings rang");
            }
        }
    }

    /// **A STRETCH put back before every string followed it leaves none
    /// stale, asleep or ringing.** The knob to either end for 1 to 55 blocks
    /// (some of a set's 107 strings re-strung, or all), then back exactly on
    /// its centre (a double-click, an undo), the frame driven as the host
    /// drives it, nothing sounding and under a ringing pedalled chord: the
    /// set, the harp's designs and every string are `Keys::new`'s to the bit,
    /// and no glide is left. The table's rule for the harp's strings
    /// (`grand::GrandDesigns::follow`, `a_knob_put_back_leaves_no_key_stale`);
    /// nothing else in the suite failed with `Strung::follow`'s `whole =
    /// None` taken out, a ringing string never set gliding, or a string put
    /// to sleep keeping its glide's coefficients (verifier).
    #[test]
    fn a_stretch_put_back_leaves_no_string_stale() {
        use super::super::{Keys, Patch};
        let mut bad = Vec::new();
        for (set, patch) in [(0usize, Patch::Piano1), (1, Patch::Piano2), (2, Patch::ElectricGrand)] {
            let centre = Strung::new(&centre_set(patch), RATE);
            for ringing in [false, true] {
                for to in [0.0_f32, 1.0] {
                    for depth in [1usize, 3, 30, 55] {
                        let mut k = Keys::new(RATE);
                        let mut out = Vec::new();
                        let cc = if ringing { [127, 0, 0] } else { [0; 3] };
                        let mut p = patch.defaults();
                        k.set_patch(patch);
                        if ringing {
                            for key in [36u8, 48, 55, 60, 64, 67, 72, 84, 96] {
                                k.note_on(key, 0.7);
                            }
                            host_blocks(&mut k, patch, &p, cc, 40, &mut out);
                        }
                        p[5] = to;
                        host_blocks(&mut k, patch, &p, cc, depth, &mut out);
                        p[5] = 0.5;
                        host_blocks(&mut k, patch, &p, cc, 300, &mut out);
                        let s = &k.harp_sets[set];
                        let mut off = 0;
                        for i in 0..LOOPS {
                            let c = centre.coef[i];
                            // A loop with no coupling is a string the set does
                            // not have, and is never re-strung.
                            if c.couple == 0.0 && c.own == 0.0 {
                                continue;
                            }
                            let (l, g) = (&k.harp.loops[i], &k.harp.glides[i]);
                            let live = (l.back, l.frac.a, l.loss.a, l.disp.a);
                            if s.strung[i] != 1.0_f32.to_bits() || s.coef[i] != c || k.harp.designs[i] != c || g.on || live != (c.d.back, c.d.frac, c.d.loss, c.d.disp) {
                                off += 1;
                            }
                        }
                        if off > 0 || s.whole != Some(1.0_f32.to_bits()) {
                            bad.push(format!("{patch:?}, ringing {ringing}, STRETCH {to} for {depth} blocks: {off} strings off the centre"));
                        }
                    }
                }
            }
        }
        assert!(bad.is_empty(), "{bad:?}");
    }

    /// **A patch change part way through following loads the set its patch
    /// left.** Piano 1, Piano 2 and the electric grand each part way to its
    /// own moved STRETCH, changed between in the middle (and through a patch
    /// with no harp), chords struck at each: every set the harp loads is the
    /// one its patch left (`Harp::designs` with it, which a string put to
    /// sleep takes), and each ends strung where its knob stands. Nothing else
    /// in the suite failed with `Harp::load` keeping the last set's designs
    /// (verifier).
    #[test]
    fn a_patch_change_mid_follow_loads_the_set_its_patch_left() {
        use super::super::{bipolar, Keys, Patch};
        let set_of = |p: Patch| match p {
            Patch::Piano1 => 0,
            Patch::Piano2 => 1,
            _ => 2,
        };
        let mut k = Keys::new(RATE);
        let mut out = Vec::new();
        let mut knobs = [Patch::Piano1.defaults(), Patch::Piano2.defaults(), Patch::ElectricGrand.defaults()];
        (knobs[0][5], knobs[1][5], knobs[2][5]) = (1.0, 0.2, 0.9);
        let order = [(Patch::Piano1, 10usize), (Patch::Piano2, 7), (Patch::ElectricGrand, 3), (Patch::Piano1, 5), (Patch::EpMk1, 4), (Patch::Piano2, 30), (Patch::Piano1, 200), (Patch::ElectricGrand, 200), (Patch::Piano2, 200)];
        for (step, &(patch, blocks)) in order.iter().enumerate() {
            let p = if patch == Patch::EpMk1 { patch.defaults() } else { knobs[set_of(patch)] };
            k.set_patch(patch);
            if patch != Patch::EpMk1 {
                let set = set_of(patch);
                let loaded = (0..LOOPS).filter(|&i| k.harp.designs[i] != k.harp_sets[set].coef[i]).count();
                assert_eq!(loaded, 0, "step {step}, {patch:?}: the loaded harp's designs differ from its set at {loaded} loops");
            }
            for key in [48u8, 60, 67] {
                k.note_on(key, 0.6);
            }
            host_blocks(&mut k, patch, &p, [127, 0, 0], blocks, &mut out);
        }
        for patch in [Patch::Piano1, Patch::Piano2, Patch::ElectricGrand] {
            let set = set_of(patch);
            let want = Strung::new(&frame_set(patch, bipolar(knobs[set][5], 1.6)), RATE);
            let wrong = (0..LOOPS).filter(|&i| (want.coef[i].couple != 0.0 || want.coef[i].own != 0.0) && k.harp_sets[set].coef[i] != want.coef[i]).count();
            assert_eq!(wrong, 0, "{patch:?}: {wrong} strings not strung where its knob stands");
        }
    }

}
