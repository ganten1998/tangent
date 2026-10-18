//! The clav: a string struck and HELD, and two pickups arguing about it.
//!
//! **Why this is neither the piano nor a harpsichord.** A piano's hammer
//! throws and leaves; a harpsichord's plectrum plucks and leaves. The clav's
//! tangent — a rubber tip on the back of the key — strikes the string and
//! then STAYS, pinning it against its anvil for as long as the finger is
//! down. That one fact is the whole instrument: the strike is percussive
//! like a hammer, the sustain is a fretted string like a guitarist's
//! hammer-on, and the note ends not by fading but by the tangent LIFTING.
//! While it is down the string is two strings — the speaking length between
//! anvil and bridge, and the far length wound in yarn and pinned silent —
//! and when it lifts they become one string again. The loop grows by the
//! yarn's length, the pitch falls, and the yarn chokes what is left.
//! Gabrielli et al. measured the fall at three semitones across the whole
//! keyboard, and that drop IS the release of a clavinet.
//!
//! **What the reference taught this file.** The voicing below is informed
//! by measured renders of the reference (a modelled instrument; the analysis
//! digest is the lane's input, and every number here is pinned beside its
//! measurement in a test). The sustain is long and nearly flat: the
//! fundamental's t60 is 8 s at C4 and twice that two octaves down, and the
//! eighth partial keeps seven tenths of it. That is a string whose loop
//! delay is LOSSLESS — the loop's fraction of a sample is a first-order
//! all-pass, not a linear read, because a linear read is itself a low-pass
//! that would take more out of C4's eighth partial on every pass than the
//! whole design asks for. Two point pickups sit near the anvil end; each
//! is read as the sum of the two travelling waves passing it, so it hears
//! the strike the moment the wave reaches it (the old reading of the loop's
//! output arrived a whole period late, twelve milliseconds at C2) and its
//! comb is exactly `|sin(n·π·x)|`. The rocker's in-phase and anti-phase
//! laws are the reference's measured ones (the in-phase one as its best
//! signed sum: a sum of two point readings cannot fill the opposite-sign
//! partials the reference's in-phase setting fills). The tangent's contact
//! is a raised cosine of 0.7 ms with a step at each end, the tip arriving
//! and the wire landing on the anvil, so the attack keeps the reference's
//! partials above the contact's main lobe and only they brighten with the
//! touch. A bass-weighted
//! quadratic term on the coils' output makes the bass fatten with velocity,
//! a two-pole near-copy of the fundamental makes it bloom and beat, and the
//! release is the measured two stages: a ramped choke while the pitch
//! glides down, then the rejoined string's tail forty decibels down. The
//! tangent's knock and the damper's thock are the reference's measured
//! noises. The voice owns its output stage, all-passes, so it moves no
//! partial's level: a spread that gives the attack the reference's crest
//! (a struck wire's pulse otherwise stands 17 dB over its power at C2, and
//! a chord's pulses land on one sample), then a narrow seat and two
//! "microphones" a quarter-cycle apart, which decorrelate the channels by
//! the same angle at every partial.
//!
//! **Where the funk lives.** In the PICKUPS. A point reading of a
//! travelling wave is a comb filter: everything whose half-wavelength
//! divides the coil's distance from the end is simply not heard. The rocker
//! takes one coil, the other, their sum, or their DIFFERENCE, which cancels
//! the low partials the two nearly share and leaves the thin sound. And
//! each coil is a magnet whose flux against the string's distance is an
//! exponential — monotonic, because the string never crosses the magnet's
//! axis — read through a derivative. It is voiced nearly linear here: the
//! reference's mid register keeps its timbre within half a decibel from
//! pianissimo to fortissimo, so the dirt that velocity buys is the
//! quadratic term's, and that lives in the bass.

use super::pickup::{Curve, Transducer};
use super::string::{Ap, Lp};
use super::ClavKnobs;

/// The loop's storage, a power of two so a read is a mask: A0 at 96 kHz is
/// 3491 samples, with room for the taps.
const LOOP_LEN: usize = 4096;
const LOOP_MASK: usize = LOOP_LEN - 1;

/// The non-speaking line's store. At SNAP's top the rejoin is six semitones
/// — 0.414 of the loop — and the loop is at most 4000 samples, so 1657 is
/// the most that is ever in flight.
const NS_LEN: usize = 2048;
const NS_MASK: usize = NS_LEN - 1;

/// The longest and shortest loop the design will build, in samples: below
/// eight the coils' taps and the four dispersion sections have no room.
const LOOP_MIN: f32 = 8.0;
const LOOP_MAX: f32 = (LOOP_LEN - 64) as f32;

// ── the sustain ────────────────────────────────────────────────────────

/// **The fundamental's t60 at C4, and how many keys halve it.** The
/// reference's whole-note fits (median over its two channels, the shared
/// analyzer's decay score at velocity 80): C2 19.3, C3 13.9, C4 7.7, C5 5.0,
/// C6 4.2, E6 3.1 s, which `8.06 · 2^(−(key − 60)/21.8)` fits from C2 to E6
/// within its own beat scatter. Clamped to [`T60_MIN`, `T60_MAX`] so the
/// extrapolation past the reference's F1..E6 stays an instrument.
const T60_C4: f32 = 8.06;
const T60_KEYS_PER_HALVING: f32 = 21.8;
const T60_MIN: f32 = 2.5;
const T60_MAX: f32 = 22.0;

/// The second loss point: the eighth partial, or 0.4 of the rate where
/// that is lower. The reference's relative t60 depends on ABSOLUTE
/// frequency — 0.72 below 2.1 kHz, 0.40 at 4.2 kHz, 0.21 at 8.4 kHz — so the
/// ratio is `0.75 · 2^(−0.9 · log2(f / 2100 Hz))`, clamped to 0.15..0.72:
/// C4's n8/n1 0.72, C5's 0.40, the reference's 0.70 and 0.38.
const LOSS_PARTIAL: f32 = 8.0;
const LOSS_RATE_FRAC: f32 = 0.40;
const RATIO_KNEE_HZ: f32 = 2100.0;
const RATIO_AT_KNEE: f32 = 0.75;
const RATIO_OCT_SLOPE: f32 = 0.9;
const RATIO_MIN: f32 = 0.15;
const RATIO_MAX: f32 = 0.72;

/// **The stiffness schedule, the reference's.** Its measured B (a
/// stiff-string fit, partials n ≤ 16 within 50 dB): 3e−5 to 5e−5 from G1 to
/// C5, then 9.6e−5 at G5, 2.4e−4 at C6 and 4.9e−4 at E6 — a plain treble
/// wire getting short, not the wound bass Gabrielli's schedule fell from.
/// `B = 3.5e−5 + 2.0e−4 · 2^((key − 84) / 3)` sits within ×1.4 of every
/// measured note but E2 (whose 1.75e−5 is a beat-confused fit), and the
/// cap keeps the extrapolation above E6 a string.
const B_FLOOR: f32 = 3.5e-5;
const B_TOP: f32 = 2.0e-4;
const B_TOP_KEY: f32 = 84.0;
const B_KEYS_PER_DOUBLING: f32 = 3.0;
const B_MAX: f32 = 2.0e-3;

/// The partial the dispersion is solved on: the eighth, or the highest
/// under 0.42 of the rate. Four first-order sections with one coefficient
/// cannot follow the stiff-string law exactly; pinned at the eighth, the
/// partials between land within a cent or two of it, and the shared
/// analyzer's fit (n ≤ 16) reads the schedule back within its tolerance
/// (`the_realised_stiffness_is_the_reference_schedule`).
const DISP_PARTIAL: f32 = 8.0;
const DISP_RATE_FRAC: f32 = 0.42;
/// The most delay one dispersion section may take, as a TIME: nineteen
/// samples at 48 kHz, the piano's cap, so 96 kHz keeps the same stiffness.
const DISP_CAP_SECS: f32 = 19.0 / 48_000.0;

// ── the pickups ────────────────────────────────────────────────────────

/// **Where the two coils sit**, as fractions of the speaking length from
/// the anvil end, fitted on the reference's single-pickup renders (an
/// exact point reading: a grid fit of the high/low ratio, four decimals,
/// 0.04 dB rms at C4): the lower coil at 0.200 / 0.225 / 0.2625 at C2 /
/// C4 / C5 — `0.1996 − 0.00347·u + 0.00813·u²`, `u` octaves above C2 — and
/// the upper one a constant 0.151 nearer the end. Held outside C2 − 2 oct
/// .. E6: beyond the reference's range the quadratic would walk the lower
/// coil to the middle of the string.
const X_LO_AT_C2: f32 = 0.1996;
const X_LO_PER_OCT: f32 = -0.00347;
const X_LO_PER_OCT2: f32 = 0.00813;
const COIL_SPACING: f32 = 0.151;
const GEOMETRY_OCT_MIN: f32 = -2.0;
const GEOMETRY_OCT_MAX: f32 = 13.0 / 3.0;

/// The upper coil's sensitivity against the lower's (the reference's fit:
/// 0.92 / 1.11 / 1.28 at C2 / C4 / C5), its in-phase weight on top of that
/// (the best signed sum of the reference's in-phase setting, `lo + 1.35·hi`
/// at −2.9 dB overall: 2.2 / 2.8 dB rms in magnitude at C4 / C5 against
/// 4.4 / 4.6 with equal weights), and the anti-phase law
/// `makeup · (lo − β·hi)`: β 1.10 / 0.89 / 0.79 (rms 3.1 / 0.23 / 0.28
/// dB). Interpolated in key, held outside.
///
/// **The makeup is this instrument's, not the reference's.** The
/// reference's own +0.2 / +2.5 / +6.6 dB level its anti-phase setting
/// against ITS in-phase law, which fills the cancellations a signed sum
/// cannot; over this in-phase law they left the anti-phase position
/// +3.1 / −0.9 / −3.3 dB re in-phase at C2 / C4 / C5 (0.02..2 s RMS),
/// where the reference's moves −1.6 / −1.7 / +0.8. Re-fitted here on that
/// ratio: −4.5 / +1.7 / +10.7 dB (`the_anti_phase_keeps_the_references_level`).
const SENS_KEYS: [f32; 3] = [36.0, 60.0, 72.0];
const UPPER_SENS: [f32; 3] = [0.92, 1.11, 1.28];
const IN_PHASE_UPPER: f32 = 1.35;
const IN_PHASE_DB: f32 = -2.9;
const ANTI_BETA: [f32; 3] = [1.10, 0.89, 0.79];
const ANTI_MAKEUP_DB: [f32; 3] = [-4.5, 1.7, 10.7];

/// The coil's rest offset in its curve's length — `pickup.rs`'s own
/// reference regulation, which a Transducer that has not been told
/// otherwise sits at — and the leak of the integrator that turns the
/// wave's velocity into the displacement the magnet reads. A leak well
/// under F1 (43.7 Hz) keeps the fundamental's integration within two per
/// cent of exact.
const COIL_X0: f32 = 0.5;
const INT_LEAK_HZ: f32 = 8.0;

/// **Loop amplitude to gaps**: what sets the coil's swing, and so its
/// DISTORTION, not its level (the chain is unit gain to the linear part).
/// Voiced nearly linear — the reference holds C4's H2..H6 within half a
/// decibel from velocity 10 to 127, and a contact that pushes the string
/// end by its whole travel displaces the wire at the coil by π·f0·τ of the
/// strike's area, 3.7 gaps at C6 fortissimo on the old 1.6 — and pinned by
/// the swing it produces (`the_string_swings_the_measured_gaps_at_the_coil`).
const DRIVE: f32 = 0.15;

/// The swing that DRIVE produces at E4, MIDI 76, in gaps: the test's
/// target and the only reader.
#[cfg(test)]
const SWING_MF: f32 = 0.048;

// ── the tangent's pulse ────────────────────────────────────────────────

/// **The contact.** A raised cosine of 0.70 ms at fortissimo, lengthening
/// as `v^−0.02`: the shape the reference's spectrum implies (its partials
/// re the fundamental, divided by its own two-coil comb, fall like the main
/// lobe of a 0.7 ms contact with the first null between C6's third and
/// fourth partials — the paper's polynomial ramp, fitted the same way, misses
/// C6 by up to 8 dB), and only that much velocity dependence: the
/// reference's C4 H2..H5 move by under a decibel from v10 to v127. What
/// brightens with the touch lies at and above the main lobe's edge, and
/// the contact's steps carry it ([`STEP_AREA`]). A compact pulse puts a
/// spectral null at `2/τ`, and that null must never sit on one of C4's
/// first eight partials (`the_pulse_has_no_null_on_c4s_first_eight_partials`).
const PULSE_SECS_FF: f32 = 0.70e-3;
const PULSE_VEL_EXP: f32 = 0.02;

/// **The contact never outlasts 1.2 periods of its note.** Above E6 the
/// reference has no strings, and a 0.70 ms contact on a note whose period
/// is shorter puts its own spectral nulls at 2.86 and 4.29 kHz, on the
/// fundamentals of F7..G7 and near C8: the top octave sank 15..24 dB
/// under C4 in holes a few keys wide (the clav verifier's chromatic scan).
/// At 1.2 periods the first null sits at 1.67 f0, between the fundamental
/// and its octave, at every key; it binds only above F#6
/// (`the_top_octave_has_no_holes`).
const CONTACT_MAX_PERIODS: f32 = 1.2;

/// **The contact's two steps.** The tip does not ease onto the wire and
/// off it: it arrives with a step of velocity, and the wire stops with
/// one when it lands on the anvil. Two short raised cosines, up at the
/// contact's start and down at its end, so they carry no net area and
/// leave the fundamental alone. A raised cosine by itself passes almost
/// nothing above its main lobe, where the reference keeps sidelobes that
/// hold the main lobe's nulls at the multiples of `1/τ` and brighten with
/// the touch; a step pair has exactly that spectrum. Each step's area re
/// the contact's: [`STEP_AREA`] at C4 fortissimo, as `v^0.75`, falling as
/// `(f_C4/f0)^1.6` above C4 and held at C4's below (the reference's C6 at
/// v127 carries 12 dB less above 3 kHz than its C4: −27.3 against −14.9 dB
/// re the fundamental). Fitted through this chain on the reference's attack
/// partials (C4 and C6 at nine velocities, E3..E6 at v80) and measured in
/// `the_contact_steps_carry_the_upper_partials`.
const STEP_AREA: f32 = 0.065;
const STEP_VEL_EXP: f32 = 0.75;
const STEP_KEY_EXP: f32 = 1.6;
const STEP_SECS: f32 = 0.08e-3;

/// VOICING: the tip's compound, worn soft to fresh hard. It scales the
/// contact's length by `tip^0.29` — no further, because at MIDI 1 the null
/// may not come down onto C4's eighth partial — and the pulse's low-pass
/// corner by `tip^−2`, so a worn tip is a darker strike without a hole in
/// it. `tip = 1.3^(1 − 2·voicing)`: exactly one at the knob's centre.
const TIP_TRAVEL: f32 = 1.30;
const TIP_LEN_EXP: f32 = 0.29;
const TIP_LP_EXP: f32 = 2.0;

/// The pulse's one-pole low-pass, `min(12·f0, 6 kHz)`: the click scales
/// with the note instead of outgrowing the treble tone.
const PULSE_LP_PARTIALS: f32 = 12.0;
const PULSE_LP_MAX_HZ: f32 = 6000.0;

/// **The pulse is zero-mean.** A second-order high-pass at 172 Hz: the
/// reference's C2 fundamental sits 14 dB under its second to fifth
/// partials with both its quadratic term and its sympathetic copy off
/// (H3/H1 +20.3 dB at v25, against the comb's +5.7; here +20.4 for the
/// string alone, +20.0 with the knock it carries), and a contact that
/// carries no net displacement is what leaves the loop and the coils'
/// integrators DC-free without a tap pair. Its tail is injected until it
/// has died, [`PULSE_TAIL_SECS`] after the contact.
const PULSE_HP_HZ: f32 = 172.0;
const PULSE_TAIL_SECS: f32 = 0.020;

/// The level law: amplitude as `v^1.07`, which with the contact's own
/// lengthening and its steps puts C4's 300 ms RMS span from v10 to v127
/// at 23.8 dB, the reference's 23.4 (`the_velocity_law_and_the_keyboard_balance`).
const LEVEL_VEL_EXP: f32 = 1.07;

/// **The keyboard's balance**, dB at the strike, piecewise linear in key.
/// A pulse of fixed length and amplitude in a shorter loop passes the
/// coils more often, so the loop's RMS rises as √f0 and the coils' combs
/// read the treble's low partials better: uncorrected, this instrument's
/// 300 ms RMS rose 13.9 dB from C2 to C6, where the reference's is flat
/// within 1.9 dB (decay score, v80: −25.9 / −25.3 / −24.8 / −24.2 / −24.0
/// dBFS at C2..C6). These anchors are the measured difference, re C4, at
/// G1 / C2 / C3 / C4 / C5 / C6 / E6, and above the reference's range two
/// more, C7 and C8, where the contact's cap ([`CONTACT_MAX_PERIODS`]) and
/// the pulse's low-pass let the top octave fall 5.2 dB under C4 by C8.
/// Held outside them.
const KEY_GAIN_KEYS: [f32; 9] = [31.0, 36.0, 48.0, 60.0, 72.0, 84.0, 88.0, 96.0, 108.0];
const KEY_GAIN_DB: [f32; 9] = [11.0, 9.0, 4.2, 0.0, -2.9, -3.2, -4.0, -2.4, 0.2];

/// What the whole chain weighs: the one level constant, the house rule's —
/// a fresh mezzo E4 (MIDI 76) as loud as the signed-off instrument's, whose
/// peak was −14.1 dBFS: the build before the spread peaked there at −14.6,
/// and the spread ([`SPREAD_SECTIONS`]) took 0.9 dB of crest and none of
/// the power, so the constant stayed and the peak reads −15.5. It leaves
/// the wah patch its headroom (the reference's pedal sits 2..5 dB over its
/// dry signal; at 0.30 the wah patch's C6 at MIDI 127 reached +0.5 dBFS):
/// the dry patch's C6 at MIDI 127 peaks at −12.3 dBFS (the old voice +0.24,
/// its pan law lifting the right channel), its loudest single note at −8.7
/// (`the_treble_keeps_its_headroom`), and a five-note v127 bass chord at
/// −3.5 (`a_bass_chord_keeps_its_headroom`).
const LEVEL: f32 = 0.19;

// ── the quadratic term ─────────────────────────────────────────────────

/// **`y + q·y²` on the coils' output**, the DC of `y²` taken out by a
/// one-pole at 10 Hz. Bass-weighted as `(f_C4 / f0)^QE_KEY_EXP`: the
/// reference with its sympathetic copy OFF gains +6.0 / +2.0 / −0.3 dB of
/// C2 fundamental at v127 / v70 / v25 in the attack and moves C4 by under
/// 0.3 dB. This term alone gives +10.8 dB at C2 v127 and +0.6 at C4: more
/// than the reference's alone, because this instrument's copy carries
/// none of the attack and the reference's carries +5 dB of it, so the
/// COMPOSITE is what is fitted — C2 H3/H1 +18.2 / +10.3 dB at v25 / v127
/// at 48 kHz, +19.1 / +10.8 at 44.1 (the brief's +20 ± 3 / +9 ± 3; the
/// reference +19.8 / +8.7) and a fundamental growing 32.6 dB from v10 to
/// v127 (28..34; the reference 32.9) (`the_quadratic_term_fattens_the_bass_only`).
/// The weighting is the first power of `f_C4/f0`: at the 0.8th the bass
/// took too little of it to bring v127 under +12 at 44.1 kHz.
const QE_Q: f32 = 0.050;
const QE_KEY_EXP: f32 = 1.0;
const QE_DC_HZ: f32 = 10.0;
/// **The products grow faster than the square.** The reference's grow as
/// the 2.15..2.37th power of the string's amplitude, and it shows: its
/// C2 fundamental gains nothing from them at v25 (−0.3 dB with its copy
/// off) and +6 dB at v127. A plain `q·y²` gains +2.6 dB at v25 for the
/// same +11 at v127, so `q` rides the touch as `v^0.35`, which with the
/// level law's `v^1.07` is the 2.33rd power.
const QE_VEL_EXP: f32 = 0.35;
/// How fast a landing strike's `q` takes over from the last one's: a jump
/// in `q` is a step of `Δq·y²` in the output.
const QE_GLIDE_SECS: f32 = 0.002;

// ── the sympathetic near-copy ──────────────────────────────────────────

/// A two-pole resonator at the fundamental, a few cents flat, fed from the
/// quadratic term's output and added back: the reference's copy sits
/// 2.2..5.8 c flat of the struck string, lifts C2's fundamental +0.5..0.7
/// dB in the attack and +7 dB over 0.5..2 s at QE 0, and — fed by the
/// quadratic products — +18 dB at v127; its composite C2 fundamental swells
/// +2.2 dB by 1 s. Bass-weighted as `(f_C4/f0)^2.5` down to C2 and held
/// there ([`SYMP_HOLD_HZ`]); its t60 is the string's law times
/// [`SYMP_T60_OVER_STRING`] (20 s at C2), and at the release it is choked
/// with the string. Measured here: C2 +2.5 dB at 1 s, G1 +2.2, a 0.113 Hz
/// beat at C2 at every rate (the copy rings within 0.01 c of its −3 c),
/// C4 moved 0.65 dB at most (`the_copy_blooms_the_bass_and_leaves_the_middle`,
/// `the_copy_rings_where_it_is_tuned`).
const SYMP_CENTS: f32 = -3.0;
/// The copy's growth: how many times the drive's own amplitude it would
/// reach in one second if driven on its resonance, at C4 (the send is
/// `2/rate`: the coupled form's resonance is the direct form's times
/// `r·sin ω`, so this number means the same at every pitch before the bass
/// weighting, as it did through the direct form's `2·sin ω/rate`).
const SYMP_GROWTH: f32 = 0.096;
const SYMP_KEY_EXP: f32 = 2.5;
const SYMP_T60_OVER_STRING: f32 = 1.15;
/// The weighting stops at C2 and holds below it: carried on down, it lit
/// G1's fundamental +6.4 dB over its first 1.6 s (+16.8 dB of copy at
/// 1 s) where the reference's G1 falls 1.7 dB by 1 s and 5.2 by 2 s.
const SYMP_HOLD_HZ: f32 = 65.406_39;

// ── the release ────────────────────────────────────────────────────────

/// **The choke**, measured on the reference's C4 held 0.5 s (broadband
/// level re the pre-off level at +10 / 20 / 40 / 80 / 160 / 320 ms: −1.3 /
/// −2.8 / −17.4 / −37.0 / −44.8 / −54.7 dB, L/R average). The yarn's rate
/// ramps in over [`CHOKE_RAMP_SECS`] (a raised cosine) to
/// [`CHOKE_DB_PER_S`], and what it chokes is the speaking string as the
/// coils read it: the rejoined string rings on beside it at
/// [`TAIL_FLOOR_DB`], decaying [`TAIL_DB_PER_S`] — the reference's tail
/// is the string's own (it does not move with its sympathetic copy off),
/// −45..−55 dB re pre-off, 25..36 dB/s.
const CHOKE_RAMP_SECS: f32 = 0.020;
const CHOKE_DB_PER_S: f32 = 720.0;
/// How the yarn's rate scales with pitch, octaves of rate per octave of
/// key: the reference's C5 has lost 26.7 dB by +40 ms where its C4 has
/// lost 17.4, a rate half as fast again an octave up.
const CHOKE_OCT_EXP: f32 = 0.6;
/// The copy's life once the key is up: the reference's copy lifts C2's
/// 60..200 ms release window +7.8 dB and is gone by its tail, which falls
/// 240 dB/s between +40 and +160 ms.
const COPY_RELEASE_SECS: f32 = 0.25;
/// The floor the choke leaves, dB re the coils' reading, at C2 / C4 / C5
/// and held outside: the reference's tail over 0.2..0.6 s after the off
/// falls with pitch, −48.9 / −55.2 / −60.6 dB re pre-off at C2 / C4 / C5
/// (its release score, L/R mean), where one floor for every key put this
/// one's at −51.9 / −51.5 / −52.2.
const TAIL_FLOOR_KEYS: [f32; 3] = [36.0, 60.0, 72.0];
const TAIL_FLOOR_DB: [f32; 3] = [-35.5, -40.0, -47.0];
const TAIL_DB_PER_S: f32 = 32.0;

/// The rejoin the paper measured: "the pitch decrease after release is three
/// semitones for the whole keyboard" (Gabrielli et al. 2013). SNAP at its
/// centre is exactly this; the knob scales the semitones.
const REJOIN_SEMITONES: f32 = 3.0;

/// **The lift, timed.** The key rises for [`REJOIN_DELAY_SECS`] before the
/// tip leaves the string, and the far length then joins over
/// [`REJOIN_SECS`] (a smoothstep): the reference's fundamental reads
/// 0..−30 c at +10 ms and −150..−186 c at +20 ms.
const REJOIN_DELAY_SECS: f32 = 0.008;
const REJOIN_SECS: f32 = 0.020;

/// The fastest the loop may grow, in samples per sample. A line
/// lengthening faster than one sample per sample would read samples
/// written BEFORE the tangent lifted, which is a hole.
const REJOIN_MAX_STEP: f32 = 0.5;

/// The level under which a voice is finished: −110 dBFS, so the tail
/// gets its second and a half.
const DONE_LEVEL: f32 = 3.0e-6;

/// The panic path and the patch change: a choke at pitch, no lift, no
/// floor, this fast.
const DAMP_SECS: f32 = 0.014;

/// **A tip landing on a moving wire takes part of its motion.** The rubber
/// meets a string already swinging past it and, for the instant of
/// contact, damps it like a felt would; what is left of the old vibration
/// is this fraction of it, and the new pulse adds to that. Without it a
/// sustain eight seconds long superposes eight pedalled strikes sixty
/// milliseconds apart past the frame's 4 dB stacking margin (measured
/// +4.1 dB); with it they stack to [`STACK_MEASURED`] and the shallowest
/// dip after a restrike stays inside the frame's hole bound
/// (`mod.rs` `eight_pedalled_repeats_neither_stack_nor_leave_a_hole`).
const STRIKE_CATCH: f32 = 0.80;

/// What the eight strikes measure with the catch, dB over a fresh strike:
/// the test's pin (`eight_repeats_stack_within_the_frame_margin`).
#[cfg(test)]
const STACK_MEASURED: f64 = 3.27;

// ── the mechanical noise ───────────────────────────────────────────────

/// **The attack's knock**, the reference's hammer-noise share: −11.7 /
/// −13.7 / −14.7 dB re the note's first 30 ms RMS at v10 / v40 / v127,
/// equal energy per band from 50 Hz to 3 kHz within 2.5 dB, its 3..8 kHz
/// band −26 dB under the strongest at v10 and −10 at v127, falling
/// −6 / −13 / −19 / −23 / −34 dB at 10 / 20 / 40 / 80 / 150 ms re its 5 ms
/// peak. Synthesised: seeded noise, a three-term pinking, a sixth-order
/// Butterworth low-pass whose corner rises with velocity (2.2 to 3.2
/// kHz), and two exponentials (`the_knock_and_the_thock`). The low-pass is
/// that steep because the reference's knock is flat to 3 kHz and 26 dB
/// down a little above it at v10: the two-pole it replaces had to sit at
/// 1.4 kHz to reach −22 dB at 3..8 kHz, and took the 1..3 kHz band 7 dB
/// under the rest. Measured here (32 strikes each): −11.6 / −14.1 / −14.9
/// dB re the note, bands within 2.1 dB to 3 kHz, 3..8 kHz −26.5 / −21.9 /
/// −11.0 dB.
const KNOCK_LEVEL: f32 = 1.067;
/// The knock is relatively louder the softer the touch — the reference's
/// rises 20 dB from v10 to v127 where the note rises 23 — so it rides the
/// level law times `v^−0.272`, fitted on its three measured velocities.
/// It does not ride the keyboard's balance table ([`KEY_GAIN_DB`], the
/// string's): with it, C2's knock stood 9 dB louder than C4's and filled
/// the weak C2 fundamental's bin, taking 1..3 dB off its H3/H1 at v25.
const KNOCK_VEL_TILT: f32 = 0.272;
const KNOCK_FAST_SECS: f32 = 0.008;
const KNOCK_SLOW_SECS: f32 = 0.050;
const KNOCK_SLOW_WEIGHT: f32 = 0.12;
const KNOCK_RISE_SECS: f32 = 0.004;
const KNOCK_SECS: f32 = 0.30;
const KNOCK_LP_HZ_SOFT: f32 = 2200.0;
const KNOCK_LP_HZ_HARD: f32 = 3200.0;
/// The pinking's flat term, which carries the knock above 600 Hz.
const KNOCK_FLAT: f32 = 0.55;
/// A sixth-order Butterworth as three biquads: their qualities.
const BUTTER6_Q: [f32; 3] = [0.517_638_1, std::f32::consts::FRAC_1_SQRT_2, 1.931_852];

/// **The release thock**: the damper and the key coming home, −23.5..−33
/// dB re the pre-off energy over 200 ms, mostly 150..400 Hz, gone 40 dB
/// in 200 ms; it tracks the note's ENERGY at the off (the reference's
/// −23.5 / −28.5 / −24.9 dB for holds of 0.12 / 0.5 / 2 s), so it rides
/// the output's mean square over [`THOCK_RMS_SECS`]. Its band falls past
/// 400 Hz as a felt's does: the reference's 400..1k band sits 4..5 dB
/// under its 150..400, its 3..8k 23..25 dB under, which a band-pass alone
/// (−1.2 / −10.4 dB, every key-up ticking above 8 kHz) did not do; the
/// one-pole low-pass at [`THOCK_LP_HZ`] does (`the_knock_and_the_thock`).
const THOCK_LEVEL: f32 = 4.0;
const THOCK_HZ: f32 = 260.0;
const THOCK_Q: f32 = 0.9;
const THOCK_LP_HZ: f32 = 450.0;
const THOCK_RMS_SECS: f32 = 0.10;
const THOCK_FAST_SECS: f32 = 0.018;
const THOCK_SLOW_SECS: f32 = 0.055;
const THOCK_SLOW_WEIGHT: f32 = 0.15;
const THOCK_RISE_SECS: f32 = 0.004;
const THOCK_SECS: f32 = 0.35;

// ── the image ──────────────────────────────────────────────────────────

/// **The attack's phase, spread.** A tip striking a wire sends every
/// partial off in the same instant, so the waveform the coils read is a
/// pulse a period long and its peak stands far over its power: 17 dB over
/// the first 300 ms at C2 here, against the reference's 11.5..12.8, and a
/// chord of such pulses struck together lands every one on the same sample
/// (a five-note v127 chord C2..G3 peaked at +4.5 dBFS, 5.5 dB over the
/// reference's own relative to its mezzo E4). Three first-order all-pass
/// sections at ten times the fundamental, on the voice's output, move no
/// partial's level and spread the attack's partials over a couple of
/// milliseconds, the way the reference's attack builds over its first
/// period: C2's crest comes to 12.1..12.8 dB, the onset re the 300 ms tail
/// stays inside its −12..−20 dB at C2 and C4, and because the corner
/// follows each key, the partials two struck notes share no longer arrive
/// in phase (`the_spread_keeps_every_level_and_takes_the_peak`,
/// `a_bass_chord_keeps_its_headroom`).
const SPREAD_SECTIONS: usize = 3;
const SPREAD_PARTIALS: f32 = 10.0;

/// **A mono instrument, seated, heard by two microphones.** The key's
/// place pans it by a third of the piano's spread (a balance law that never
/// lifts a side above unity, so the extremes differ by under 2 dB). The
/// two channels are then a ROTATION of one signal and its quadrature: two
/// chains of four first-order all-passes whose phases differ by 90°
/// (within 1.1° from 40 Hz to 12 kHz at 48 kHz, 6.9° at 44.1; corners
/// fitted by minimax, Hz; the A chain's 196 Hz section is also a third of
/// C2's crest — three sections a chain left it at 13.9 dB),
/// mixed as `cos θ·A ± sin θ·B`. Each channel is the mono signal with its
/// phase turned by ±θ, so every partial has the same level in either
/// channel and in their sum less a flat `cos θ`: correlation `cos 2θ`,
/// 0.87 at the stock seating, mono 0.3 dB down, at every note and every
/// partial (`the_image_is_seated_and_decorrelated`). WIDTH scales θ, and at
/// zero both channels are the A chain's output, sample for sample.
const PAN_SPREAD: f32 = 0.32;
const MIC_A_HZ: [f32; 4] = [19.0, 195.81, 1234.25, 7559.65];
const MIC_B_HZ: [f32; 4] = [74.09, 493.5, 3076.35, 17222.66];
const MIC_ANGLE: f32 = 15.0 * std::f32::consts::PI / 180.0;

/// The physical instrument under a pair of patches. The wah is an effect
/// on top, exactly as the suitcase's tremolo is; the reference's envelope
/// wah preset plays the same strings and pickups as its dry one, so the two
/// bases are the same wire.
pub struct ClavBase {
    /// Multiplier on the measured stiffness schedule: string gauge. At a
    /// fixed pitch and length B goes as the wire's diameter squared, so a
    /// heavier set is a stiffer set.
    pub gauge: f32,
}

impl ClavBase {
    /// The instrument the reference measured.
    pub const DRY: ClavBase = ClavBase { gauge: 1.0 };
    /// The wah patch's instrument: the same.
    pub const WAH: ClavBase = ClavBase { gauge: 1.0 };
}

/// The stiffness schedule — see [`B_FLOOR`] — scaled by the base's wire.
fn stiffness(key: f32, gauge: f32) -> f32 {
    ((B_FLOOR + B_TOP * 2.0_f32.powf((key - B_TOP_KEY) / B_KEYS_PER_DOUBLING)) * gauge).min(B_MAX)
}

/// The fundamental's t60 law, seconds, before the knobs.
fn t60_law(key: f32) -> f32 {
    (T60_C4 * 2.0_f32.powf(-(key - 60.0) / T60_KEYS_PER_HALVING)).clamp(T60_MIN, T60_MAX)
}

/// The second loss point's t60 as a fraction of the fundamental's.
fn t60_ratio(f_hi: f32) -> f32 {
    (RATIO_AT_KNEE * 2.0_f32.powf(-RATIO_OCT_SLOPE * (f_hi / RATIO_KNEE_HZ).log2())).clamp(RATIO_MIN, RATIO_MAX)
}

/// The cookbook low-pass biquad at `hz`, quality `q`, as
/// `[b0, b1, b2, a1, a2]` for the transposed direct form.
fn lowpass(hz: f32, q: f32, rate: f32) -> [f32; 5] {
    let w0 = std::f32::consts::TAU * hz.min(0.45 * rate) / rate;
    let (s, c) = w0.sin_cos();
    let alpha = s / (2.0 * q);
    let a0 = 1.0 + alpha;
    [(1.0 - c) * 0.5 / a0, (1.0 - c) / a0, (1.0 - c) * 0.5 / a0, -2.0 * c / a0, (1.0 - alpha) / a0]
}

/// The choke's floor for this key, dB: see [`TAIL_FLOOR_DB`].
fn tail_floor_db(key: f32) -> f32 {
    let (ks, ds) = (TAIL_FLOOR_KEYS, TAIL_FLOOR_DB);
    if key <= ks[0] {
        ds[0]
    } else if key <= ks[1] {
        ds[0] + (ds[1] - ds[0]) * (key - ks[0]) / (ks[1] - ks[0])
    } else if key <= ks[2] {
        ds[1] + (ds[2] - ds[1]) * (key - ks[1]) / (ks[2] - ks[1])
    } else {
        ds[2]
    }
}

/// The strike's key gain, linear: see [`KEY_GAIN_DB`].
fn key_gain(key: f32) -> f32 {
    let (ks, ds) = (KEY_GAIN_KEYS, KEY_GAIN_DB);
    let db = if key <= ks[0] {
        ds[0]
    } else if key >= ks[ks.len() - 1] {
        ds[ds.len() - 1]
    } else {
        let i = ks.iter().position(|&k| k > key).unwrap_or(ks.len() - 1).max(1);
        ds[i - 1] + (ds[i] - ds[i - 1]) * (key - ks[i - 1]) / (ks[i] - ks[i - 1])
    };
    10.0_f32.powf(db / 20.0)
}

/// Piecewise linear in key through three anchors, held outside them.
fn by_key(key: f32, ys: [f32; 3]) -> f32 {
    let ks = SENS_KEYS;
    if key <= ks[0] {
        ys[0]
    } else if key <= ks[1] {
        ys[0] + (ys[1] - ys[0]) * (key - ks[0]) / (ks[1] - ks[0])
    } else if key <= ks[2] {
        ys[1] + (ys[2] - ys[1]) * (key - ks[1]) / (ks[2] - ks[1])
    } else {
        ys[2]
    }
}

/// The two coils' positions, fractions of the speaking length from the
/// anvil end: `(lower, upper)`.
fn coil_positions(key: f32) -> (f32, f32) {
    let u = ((key - 36.0) / 12.0).clamp(GEOMETRY_OCT_MIN, GEOMETRY_OCT_MAX);
    let lo = X_LO_AT_C2 + X_LO_PER_OCT * u + X_LO_PER_OCT2 * u * u;
    (lo, lo - COIL_SPACING)
}

/// The rocker under the PICKUP knob, as `(lower, upper)` weights on the two
/// coils' unit-sensitivity readings. Four positions, hard-edged as the
/// switch is, in the order the knob has always stored them (a saved value
/// keeps its position): the lower coil alone (the old centre coil), the
/// upper coil alone (the old bridge coil), both in phase (the shipped
/// 0.6, the reference's preset), both in anti-phase. Read at the strike;
/// a change of position under a held note is a design change `lands`
/// refuses.
fn rocker(pickup: f32, key: f32) -> (f32, f32) {
    let p = pickup.clamp(0.0, 1.0);
    let sens = by_key(key, UPPER_SENS);
    if p < 0.25 {
        (1.0, 0.0)
    } else if p < 0.5 {
        (0.0, sens)
    } else if p < 0.75 {
        let g = 10.0_f32.powf(IN_PHASE_DB / 20.0);
        (g, g * IN_PHASE_UPPER * sens)
    } else {
        let g = 10.0_f32.powf(by_key(key, ANTI_MAKEUP_DB) / 20.0);
        (g, -g * by_key(key, ANTI_BETA) * sens)
    }
}

/// Where a key sits across the field before the WIDTH knob: the frame's
/// own `pan_for` (A0 at -0.55, rising in a straight line to +0.55 at D#6,
/// key 87, and flat above it), which this voice narrows.
fn key_pan(pitch: u8) -> f32 {
    let t = (f32::from(pitch) - 21.0) / 66.0;
    (t.clamp(0.0, 1.0) - 0.5) * 1.1
}

/// The loss filter's two-point solve — `string.rs`'s `design_loss`, the same
/// algebra, without its floor on the pole. Its `p ≥ 0.02` binds above C5 at
/// 44.1 kHz, where this sustain asks for a gentler top than that, and C#5's
/// eighth partial then dies 3 % early (`the_loop_is_lossless_and_its_fraction_an_all_pass`
/// found it; the critic's roadmap asks for that clamp to go). A flat or
/// rising request gets a flat filter. `(p, g)` for `|H| = g(1 − p) / |1 −
/// p·e^−jw|` with `ρ1` at `w1` and `ρ2` at `w2`.
fn design_loss_exact(w1: f32, w2: f32, rho1: f32, rho2: f32) -> (f32, f32) {
    let (c1, c2) = (f64::from(w1).cos(), f64::from(w2).cos());
    let r = (f64::from(rho1) / f64::from(rho2)).powi(2);
    let q = (c2 - r * c1) / (1.0 - r);
    let p = if r > 1.0 && q > 1.0 { (q - (q * q - 1.0).sqrt()).min(0.95) } else { 0.0 };
    let g = f64::from(rho1) * (1.0 - 2.0 * p * c1 + p * p).sqrt() / (1.0 - p);
    (p as f32, g.min(0.999_999) as f32)
}

/// The phase LAG of a first-order all-pass `(a + z⁻¹)/(1 + a·z⁻¹)` at `w`.
fn ap_lag(w: f64, a: f64) -> f64 {
    let (s, c) = w.sin_cos();
    w - 2.0 * (a * s).atan2(1.0 + a * c)
}

/// The phase lag of the loss one-pole `g(1 − p)/(1 − p·z⁻¹)` at `w`.
fn lp_lag(w: f64, p: f64) -> f64 {
    let (s, c) = w.sin_cos();
    (p * s).atan2(1.0 - p * c)
}

/// One of a coil's two taps: `i + f` samples behind the write head,
/// read linearly. A FIXED fraction on a feed-forward tap is a mild
/// low-pass on the coil's reading (a tenth of a decibel at C4's tenth
/// partial), not a loss on every pass of the loop, which is where it
/// mattered.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Tap {
    i: usize,
    f: f32,
}

impl Tap {
    fn at(d: f32) -> Tap {
        let d = d.max(1.0);
        let i = d.floor();
        Tap { i: i as usize, f: d - i }
    }
}

/// Everything a strike decides about the loop and the coils before it
/// touches the string. Pure: see [`ClavVoice::design`].
#[derive(Clone, Copy)]
struct Design {
    /// The loop's period at the fundamental, samples.
    total: f32,
    /// `2π f0 / rate`.
    omega: f32,
    /// The integer line.
    m: usize,
    /// The fraction's all-pass coefficient: its phase delay at the
    /// fundamental is exactly what the line lacks.
    thiran: f32,
    loss_a: f32,
    gain: f32,
    disp_a: f32,
    /// Where the tangent's pulse enters: the anvil, samples behind the head.
    anvil: usize,
    /// The coils' taps: lower near / far, upper near / far.
    taps: [Tap; 4],
    mix: (f32, f32),
    /// The fundamental's t60 after the knobs, seconds.
    t60: f32,
    /// The output stage's coefficients (states zero).
    image: Image,
}

/// The loop's design for a fundamental of `f0` Hz at `rate`, stiffness `b`,
/// t60s `t60_f` at the fundamental and `t60_h` at `f_hi`, leaving room for
/// coil taps `room` samples either side of the anvil.
///
/// **Exact by construction at the fundamental**: the dispersion is solved
/// first (a bisection on the sections' shared coefficient that puts
/// partial [`DISP_PARTIAL`] at `n·f0·√(1 + B·n²)/√(1 + B)` once the line has
/// taken up the rest of the period), then the line's integer part and its
/// fraction — the first-order all-pass whose phase delay AT THE
/// FUNDAMENTAL is the fraction exactly, `a = sin((1 − d)·w/2) /
/// sin((1 + d)·w/2)` (Thiran's `(1 − d)/(1 + d)` as `w → 0`), with `d` kept
/// in [0.5, 1.5) where that all-pass is closest to flat. Nothing in the
/// loop has a magnitude but the loss filter, so the t60s ARE the design.
fn design_loop(rate: f32, f0: f32, b: f32, t60_f: f32, t60_h: f32, f_hi: f32, room: f32) -> Loop {
    let total = (rate / f0).clamp(LOOP_MIN, LOOP_MAX);
    let w1 = std::f32::consts::TAU / total;
    let w2 = std::f32::consts::TAU * f_hi / rate;
    let f0 = rate / total;
    let rho1 = 10.0_f32.powf(-3.0 / (t60_f * f0));
    let rho2 = 10.0_f32.powf(-3.0 / (t60_h * f0));
    let (p, gain) = design_loss_exact(w1, w2, rho1, rho2);

    let (w1d, pd, bd) = (f64::from(w1), f64::from(p), f64::from(b));
    let lp1 = lp_lag(w1d, pd);
    // The dispersion's budget: what the line must keep for the coils'
    // taps either side of the anvil (`room` each way, the anvil at the
    // line's middle), and the fraction's 1.5 at most.
    let m_min = (2.0 * f64::from(room) + 1.0).ceil();
    let budget = f64::from(total) - lp1 / w1d - 1.5 - m_min;
    let tau_cap = (budget / 4.0).min(f64::from(DISP_CAP_SECS * rate));
    let a_min = if tau_cap > 1.0 { (1.0 - tau_cap) / (1.0 + tau_cap) } else { 0.0 };
    // A loop too short to give each section a whole sample — above E7 at
    // 44.1 kHz — gives each what it can: under a sample is a section with
    // a positive coefficient, a slight stretch the wrong way, on a note
    // whose second partial is already past 8 kHz; the fundamental stays in
    // tune, which is what matters there.
    let short = if tau_cap < 1.0 {
        let t = tau_cap.max(0.2);
        Some((1.0 - t) / (1.0 + t))
    } else {
        None
    };
    let n = (f64::from(DISP_PARTIAL)).min((f64::from(DISP_RATE_FRAC * rate / f0)).floor()).max(2.0);
    let wr = n * w1d * ((1.0 + bd * n * n) / (1.0 + bd)).sqrt();
    let lpr = lp_lag(wr, pd);
    let two_pi = std::f64::consts::TAU;
    // Loop phase at the target frequency, minus the 2π·n it must be,
    // once the line has made the fundamental exact. Positive: the loop's
    // n-th mode lies below the target and wants more dispersion.
    let g = |a: f64| {
        let line = (two_pi - lp1 - 4.0 * ap_lag(w1d, a)) / w1d;
        wr * line + lpr + 4.0 * ap_lag(wr, a) - two_pi * n
    };
    let disp_a = if let Some(a) = short {
        a
    } else if g(0.0) <= 0.0 {
        0.0
    } else if g(a_min) > 0.0 {
        a_min
    } else {
        let (mut lo, mut hi) = (a_min, 0.0);
        for _ in 0..40 {
            let mid = 0.5 * (lo + hi);
            if g(mid) > 0.0 {
                hi = mid;
            } else {
                lo = mid;
            }
        }
        0.5 * (lo + hi)
    };
    let d_line = (two_pi - lp1 - 4.0 * ap_lag(w1d, disp_a)) / w1d;
    let m = ((d_line - 0.5).floor().max(m_min)) as usize;
    // Only a loop too short for its own taps can push `d` out of range,
    // and there a fundamental a few cents off is the right failure: an
    // all-pass with `d` near −1 is a pole on the unit circle.
    let d = (d_line - m as f64).clamp(0.5, 1.4999);
    let thiran = ((1.0 - d) * w1d * 0.5).sin() / ((1.0 + d) * w1d * 0.5).sin();
    Loop {
        total,
        m,
        thiran: thiran as f32,
        loss_a: 1.0 - p,
        gain,
        disp_a: disp_a as f32,
        anvil: m / 2,
    }
}

/// What [`design_loop`] decides: the period, the integer line and the
/// fraction's all-pass, the loss one-pole, the dispersion, the anvil.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Loop {
    total: f32,
    m: usize,
    thiran: f32,
    loss_a: f32,
    gain: f32,
    disp_a: f32,
    anvil: usize,
}

/// One coil: the integrator that turns the wave's velocity into
/// displacement, in gaps, and the exponential flux at the rest offset plus
/// that displacement, `e^−(x₀ + x)`, read through its derivative.
///
/// **The flux is advanced, not re-evaluated.** `f(x + Δ) = f(x)·e^−Δ`, and
/// one sample's travel `Δ` is a hundredth of a gap or less, so `e^−Δ` to
/// third order is exact to float precision; the exact flux is re-taken once
/// a block from the shared transducer's curve, so rounding cannot walk it
/// and the curve stays `pickup.rs`'s. The output is `pickup.rs`'s
/// Exponential transducer exactly — normalised at the rest point, the
/// derivative referred to the note's own ω₀ — which
/// `the_coil_is_the_exponential_transducer` holds sample for sample.
#[derive(Clone, Copy, Default)]
struct Coil {
    x: f32,
    f: f32,
}

impl Coil {
    fn clear(&mut self, magnet: &Transducer) {
        self.x = 0.0;
        self.f = magnet.flux(COIL_X0);
    }

    fn resync(&mut self, magnet: &Transducer) {
        self.f = magnet.flux(COIL_X0 + self.x);
    }

    /// One sample: velocity `v` in, the flux's fall over the sample out
    /// (the voice scales it to unit gain).
    #[inline(always)]
    fn tick(&mut self, v: f32, k: f32, leak: f32) -> f32 {
        let xn = self.x * leak + v * k;
        let d = xn - self.x;
        self.x = xn;
        let fall = self.f * (d * (1.0 - d * (0.5 - d * (1.0 / 6.0))));
        self.f -= fall;
        fall
    }
}

/// Deterministic noise: a xorshift32, seeded per strike. No OS randomness.
#[derive(Clone, Copy)]
struct Rng(u32);

impl Rng {
    #[inline(always)]
    fn next(&mut self) -> f32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        // 2^−31: a signed 32-bit integer to [−1, 1).
        (x as i32) as f32 * 4.656_613e-10
    }
}

/// A burst's envelope: how long it runs, its fast and slow decay time
/// constants, the slow part's weight and the rise, seconds.
struct Shape {
    secs: f32,
    fast: f32,
    slow: f32,
    weight: f32,
    rise: f32,
}

const KNOCK: Shape = Shape {
    secs: KNOCK_SECS,
    fast: KNOCK_FAST_SECS,
    slow: KNOCK_SLOW_SECS,
    weight: KNOCK_SLOW_WEIGHT,
    rise: KNOCK_RISE_SECS,
};

const THOCK: Shape = Shape {
    secs: THOCK_SECS,
    fast: THOCK_FAST_SECS,
    slow: THOCK_SLOW_SECS,
    weight: THOCK_SLOW_WEIGHT,
    rise: THOCK_RISE_SECS,
};

/// One control block of the clav: what [`ClavVoice::begin_block`] set at
/// its first frame for its `len` frames (the choke's and the rejoin's steps
/// a frame), how far the voice has rendered into it, and its peak and
/// energy so far. A host's call may end inside a block and the next
/// goes on with the same values, so the voice's control runs on a grid of
/// its own, every [`BLOCK`] frames from the last strike or release: run once
/// a call, it made a release in 1-, 16- or 37-frame calls another release
/// (-31 to -47 dB re the note after the key-off against the 128-frame take).
#[derive(Clone, Copy, Default)]
struct Ctl {
    len: usize,
    done: usize,
    choke_step: f32,
    ns_step: f32,
    peak: f32,
    energy: f32,
}

/// The longest control block, the frame's block.
const BLOCK: usize = super::modal::BLOCK;

/// A burst of shaped noise with a two-exponential envelope and a rise:
/// the attack's knock and the release's thock share it.
#[derive(Clone, Copy, Default)]
struct Burst {
    left: u32,
    amp: f32,
    fast: f32,
    slow: f32,
    rise: f32,
    fast_k: f32,
    slow_k: f32,
    rise_k: f32,
    // the knock's pinking and low-pass; the thock's band-pass and low-pass
    p1: f32,
    p2: f32,
    p1_a: f32,
    p2_a: f32,
    lp: [f32; 6],
    lp_a: f32,
    /// The knock's three low-pass biquads, `[b0, b1, b2, a1, a2]` each.
    lpq: [[f32; 5]; 3],
    bp_s1: f32,
    bp_s2: f32,
    bp: [f32; 5],
}

impl Burst {
    fn arm(&mut self, rate: f32, amp: f32, shape: &Shape) {
        self.left = (shape.secs * rate) as u32;
        self.amp = amp;
        self.fast = 1.0;
        self.slow = shape.weight;
        self.rise = 1.0;
        self.fast_k = (-1.0 / (shape.fast * rate)).exp();
        self.slow_k = (-1.0 / (shape.slow * rate)).exp();
        self.rise_k = (-1.0 / (shape.rise * rate)).exp();
    }

    #[inline(always)]
    fn envelope(&mut self) -> f32 {
        self.left -= 1;
        let e = (self.fast + self.slow) * (1.0 - self.rise);
        self.fast *= self.fast_k;
        self.slow *= self.slow_k;
        self.rise *= self.rise_k;
        e * self.amp
    }
}

/// **The near-copy**: a two-pole resonator in the coupled form, a rotation
/// of `(x, y)` by the copy's angle each sample, shrunk by its radius. Its
/// angle at C2 is 0.0085 rad at 48 kHz, so the direct form's `2r·cos ω`
/// sits 3.6e−5 under two, where an f32 holds it to 1.2e−7: the copy's
/// pitch then moved with the key and the rate (−2.7 c at C2 and 48 kHz,
/// −4.1 at 96 kHz, −9.8 at C1, none at E1 — the clav verifier's
/// simulation). Here the angle lives in `s = r·sin ω`, whose RELATIVE
/// precision is the float's, and the coefficients are computed in f64:
/// the copy rings on its cents at every key and rate
/// (`the_copy_rings_where_it_is_tuned`). Output `y`, which is the direct
/// form's response times `r·sin ω` and a sample's delay, so the send
/// divides that back out.
#[derive(Clone, Copy, Default)]
struct NearCopy {
    c: f32,
    s: f32,
    x: f32,
    y: f32,
    send: f32,
    /// The copy's angle per sample, kept in f64 for the release's retune.
    w: f64,
}

impl NearCopy {
    /// Tune to an angle of `w` radians a sample and a t60 of `secs`.
    fn tune(&mut self, w: f64, secs: f64, rate: f64) {
        self.w = w;
        self.set_t60(secs, rate);
    }

    /// A new life at the same pitch: the radius for `secs`.
    fn set_t60(&mut self, secs: f64, rate: f64) {
        let r = 10.0_f64.powf(-3.0 / (secs.max(1.0e-3) * rate));
        let (s, c) = self.w.sin_cos();
        self.c = (r * c) as f32;
        self.s = (r * s) as f32;
    }

    #[inline(always)]
    fn tick(&mut self, u: f32) -> f32 {
        let x = self.c * self.x - self.s * self.y + self.send * u;
        let y = self.s * self.x + self.c * self.y;
        self.x = x;
        self.y = y;
        y
    }
}

/// The bilinear first-order all-pass coefficient for a corner of `hz`:
/// `(t − 1)/(t + 1)`, `t = tan(π·hz/rate)`, the corner held under 0.45 of
/// the rate.
fn ap_coef(hz: f32, rate: f32) -> f32 {
    let t = (std::f64::consts::PI * f64::from(hz.min(0.45 * rate)) / f64::from(rate)).tan();
    ((t - 1.0) / (t + 1.0)) as f32
}

/// A first-order all-pass `(a + z⁻¹)/(1 + a·z⁻¹)` in its one-multiplier
/// lattice form, `w = x − a·w₋₁`, `y = a·w + w₋₁`: one multiply-add on the
/// path a sample carries into the next, where the direct form has a
/// subtraction and a multiply-add. The output stage runs eleven of these
/// a sample in every voice, so the carried path is its cost. (Not
/// `mul_add`: off ARM it is a library call.)
#[derive(Clone, Copy, Default)]
struct Lattice {
    a: f32,
    w: f32,
}

impl Lattice {
    #[inline(always)]
    fn tick(&mut self, x: f32) -> f32 {
        let w = x - self.a * self.w;
        let y = self.a * w + self.w;
        self.w = w;
        y
    }
}

/// The voice's output stage: the spread ([`SPREAD_SECTIONS`]), then the
/// two microphones ([`MIC_A_HZ`], [`MIC_B_HZ`]) and the seat. All-passes
/// and gains: linear, time-invariant through a note.
#[derive(Clone, Copy, Default)]
struct Image {
    spread: [Lattice; SPREAD_SECTIONS],
    a: [Lattice; 4],
    b: [Lattice; 4],
    /// `L = la·A + lb·B`, `R = ra·A + rb·B`: the rotation and the seat.
    la: f32,
    lb: f32,
    ra: f32,
    rb: f32,
}

impl Image {
    /// The coefficients for this key and WIDTH, states untouched.
    fn design(&mut self, rate: f32, f0: f32, pitch: u8, width: f32) {
        let k = ap_coef(SPREAD_PARTIALS * f0, rate);
        for s in &mut self.spread {
            s.a = k;
        }
        for (s, hz) in self.a.iter_mut().zip(MIC_A_HZ) {
            s.a = ap_coef(hz, rate);
        }
        for (s, hz) in self.b.iter_mut().zip(MIC_B_HZ) {
            s.a = ap_coef(hz, rate);
        }
        let width = width.clamp(0.0, 1.5);
        let pan = key_pan(pitch) * PAN_SPREAD * width;
        let (gl, gr) = ((1.0 - pan).min(1.0), (1.0 + pan).min(1.0));
        let (s, c) = (MIC_ANGLE * width).sin_cos();
        (self.la, self.lb, self.ra, self.rb) = (gl * c, gl * s, gr * c, -gr * s);
    }

    fn clear(&mut self) {
        for s in self.spread.iter_mut().chain(self.a.iter_mut()).chain(self.b.iter_mut()) {
            s.w = 0.0;
        }
    }

    /// Whether two designs are the same stage (a strike lands only on its
    /// own image: a gain or a corner moved under a ringing string is a
    /// step in the output).
    fn same(&self, o: &Image) -> bool {
        self.spread[0].a == o.spread[0].a && (self.la, self.lb, self.ra, self.rb) == (o.la, o.lb, o.ra, o.rb)
    }

    /// One sample: the spread signal through both microphones, `(A, B)`.
    #[inline(always)]
    fn tick(&mut self, x: f32) -> (f32, f32) {
        let mut d = x;
        for s in &mut self.spread {
            d = s.tick(d);
        }
        let (mut a, mut b) = (d, d);
        for (sa, sb) in self.a.iter_mut().zip(self.b.iter_mut()) {
            a = sa.tick(a);
            b = sb.tick(b);
        }
        (a, b)
    }
}

/// One key of the clav.
pub struct ClavVoice {
    rate: f32,
    // ── the loop ──
    buf: [f32; LOOP_LEN],
    /// The slot about to be written: the bridge's outgoing sample.
    w: usize,
    m: usize,
    th: Ap,
    loss: Lp,
    gain: f32,
    disp: [Ap; 4],
    anvil: usize,
    taps: [Tap; 4],
    total: f32,
    t60: f32,
    // ── the rejoin ──
    ns: [f32; NS_LEN],
    ns_w: usize,
    ns_len: f32,
    ns_to: f32,
    ns_gain: f32,
    rejoining: bool,
    /// The SNAP knob as read at the strike: how many of the paper's three
    /// semitones the rejoin is.
    rejoin: f32,
    // ── the pulse ──
    exc_left: u32,
    pulse_i: u32,
    pulse_n: u32,
    pulse_ph: f32,
    pulse_amp: f32,
    /// The contact's two steps ([`STEP_AREA`]): each one's length in
    /// samples and phase step, where the second begins, and their height
    /// re the contact's crest.
    step_n: u32,
    step_ph: f32,
    step_at: u32,
    step_amp: f32,
    hp: [f32; 5],
    hp_s1: f32,
    hp_s2: f32,
    lp_a: f32,
    lp_z: f32,
    // ── the coils ──
    /// The magnet's curve, shared with the tine and the reed's file.
    magnet: Transducer,
    coil: [Coil; 2],
    mix: (f32, f32),
    coil_k: f32,
    coil_leak: f32,
    coil_out: f32,
    // ── the quadratic term and the copy ──
    q: f32,
    /// `q` as it stands: it glides to a landing strike's over [`QE_GLIDE_SECS`].
    q_now: f32,
    q_glide: f32,
    q_dc: f32,
    q_dc_a: f32,
    copy: NearCopy,
    // ── the tone rocker ──
    tone_a: f32,
    tone_z: f32,
    tone_z2: f32,
    // ── the image ──
    image: Image,
    // ── the release ──
    releasing: bool,
    rel_t: u32,
    /// The release knob's multiplier on the choke's time.
    rel_k: f32,
    choke: f32,
    choke_floor: f32,
    /// The yarn's rate for this key, dB/s.
    choke_rate: f32,
    damping: bool,
    damp_k: f32,
    // ── the noise ──
    rng: Rng,
    /// How many times this voice has been struck: the knock's seed takes
    /// it, so a repeated key is a new knock.
    strikes: u32,
    knock: Burst,
    thock: Burst,
    /// The key, for the release's key-dependent tail.
    pitch: u8,
    env: f32,
    /// The output's mean square over the last ~100 ms: what the thock
    /// rides (the reference's tracks the note's energy at the off).
    ms: f32,
    done: bool,
    /// The control block being rendered (see [`Ctl`]).
    ctl: Ctl,
    /// Tests measure a mechanism by difference; this keeps the release's
    /// thock out when they ask.
    #[cfg(test)]
    quiet: bool,
}

impl ClavVoice {
    pub fn new(sample_rate: f32) -> Self {
        let rate = if sample_rate > 0.0 { sample_rate } else { 48_000.0 };
        // The pulse's high-pass: a second-order Butterworth, the cookbook's
        // bilinear form, fixed for the rate.
        let w0 = std::f32::consts::TAU * PULSE_HP_HZ / rate;
        let (s, c) = w0.sin_cos();
        let alpha = s / (2.0 * std::f32::consts::FRAC_1_SQRT_2);
        let a0 = 1.0 + alpha;
        let hp = [
            (1.0 + c) * 0.5 / a0,
            -(1.0 + c) / a0,
            (1.0 + c) * 0.5 / a0,
            -2.0 * c / a0,
            (1.0 - alpha) / a0,
        ];
        let mut v = ClavVoice {
            rate,
            buf: [0.0; LOOP_LEN],
            w: 0,
            m: 100,
            th: Ap::default(),
            loss: Lp::default(),
            gain: 0.0,
            disp: [Ap::default(); 4],
            anvil: 50,
            taps: [Tap { i: 1, f: 0.0 }; 4],
            total: 100.0,
            t60: 1.0,
            ns: [0.0; NS_LEN],
            ns_w: 0,
            ns_len: 0.0,
            ns_to: 0.0,
            ns_gain: 1.0,
            rejoining: false,
            rejoin: 1.0,
            exc_left: 0,
            pulse_i: 0,
            pulse_n: 0,
            pulse_ph: 0.0,
            pulse_amp: 0.0,
            step_n: 0,
            step_ph: 0.0,
            step_at: 0,
            step_amp: 0.0,
            hp,
            hp_s1: 0.0,
            hp_s2: 0.0,
            lp_a: 1.0,
            lp_z: 0.0,
            magnet: Transducer::new(Curve::Exponential { scale: 1.0 }),
            coil: [Coil::default(); 2],
            mix: (1.0, 0.0),
            coil_k: 0.0,
            coil_leak: 1.0,
            coil_out: 0.0,
            q: 0.0,
            q_now: 0.0,
            q_glide: 1.0 - (-1.0 / (QE_GLIDE_SECS * rate)).exp(),
            q_dc: 0.0,
            q_dc_a: 1.0 - (-std::f32::consts::TAU * QE_DC_HZ / rate).exp(),
            copy: NearCopy::default(),
            tone_a: 1.0,
            tone_z: 0.0,
            tone_z2: 0.0,
            image: Image::default(),
            releasing: false,
            rel_t: 0,
            rel_k: 1.0,
            choke: 1.0,
            choke_floor: 0.0,
            choke_rate: CHOKE_DB_PER_S,
            damping: false,
            damp_k: 1.0,
            rng: Rng(0x9E37_79B9),
            strikes: 0,
            knock: Burst::default(),
            thock: Burst::default(),
            pitch: 60,
            env: 0.0,
            ms: 0.0,
            done: true,
            ctl: Ctl::default(),
            #[cfg(test)]
            quiet: false,
        };
        let magnet = v.magnet;
        v.coil[0].clear(&magnet);
        v.coil[1].clear(&magnet);
        v
    }

    /// Whether a strike of `pitch` with these knobs would land on the
    /// string ringing now rather than have to replace it. The coils' taps
    /// and the rocker: moving a tap or a weight under a ringing string
    /// jumps the output by up to the whole raw signal. And the rejoin: a
    /// string whose far length is back in the loop is a longer string than
    /// the one being asked for. The frame fades such a string out and
    /// starts the new note on a fresh voice.
    pub fn lands(&self, pitch: u8, knobs: &ClavKnobs) -> bool {
        self.lands_based(pitch, knobs, &ClavBase::DRY)
    }

    pub fn lands_based(&self, pitch: u8, knobs: &ClavKnobs, base: &ClavBase) -> bool {
        !self.done && self.fits(&self.design(pitch, knobs, base))
    }

    fn fits(&self, d: &Design) -> bool {
        !self.rejoining
            && self.m == d.m
            && self.th.a == d.thiran
            && self.taps == d.taps
            && self.mix == d.mix
            && self.anvil == d.anvil
            && self.image.same(&d.image)
    }

    /// Everything a strike decides about the loop and the coils, for this
    /// pitch, these knobs and this base. Pure: reads the rate, writes
    /// nothing.
    fn design(&self, pitch: u8, k: &ClavKnobs, base: &ClavBase) -> Design {
        let key = f32::from(pitch);
        let f0 = 440.0 * 2.0_f32.powf((key - 69.0) / 12.0);
        // Sustain: the law, then the DECAY knob, then the MUTE pad riding
        // the whole curve down to staccato.
        let t60 = (t60_law(key) * k.decay * (1.0 - 0.88 * k.mute.powf(0.8))).clamp(0.06, 60.0);
        let f_hi = (LOSS_PARTIAL * f0).min(LOSS_RATE_FRAC * self.rate);
        let t60_h = (t60 * t60_ratio(f_hi)).clamp(0.03, t60);
        let (x_lo, x_hi) = coil_positions(key);
        let total0 = (self.rate / f0).clamp(LOOP_MIN, LOOP_MAX);
        let room = x_lo * total0 * 0.5 + 1.0;
        let lp = design_loop(self.rate, f0, stiffness(key, base.gauge), t60, t60_h, f_hi, room);
        let (total, anvil) = (lp.total, lp.anvil);
        // The coils: each tap `x·T/2` either side of the anvil, so the tap
        // spacing is `x` of the whole period and the comb is |sin(nπx)|
        // exactly at the loop's modes.
        let s_lo = x_lo * total * 0.5;
        let s_hi = x_hi * total * 0.5;
        let a = anvil as f32;
        let taps = [Tap::at(a - s_lo), Tap::at(a + s_lo), Tap::at(a - s_hi), Tap::at(a + s_hi)];
        let mut image = Image::default();
        image.design(self.rate, self.rate / total, pitch, k.width);
        Design {
            total,
            omega: std::f32::consts::TAU / total,
            m: lp.m,
            thiran: lp.thiran,
            loss_a: lp.loss_a,
            gain: lp.gain,
            disp_a: lp.disp_a,
            anvil,
            taps,
            mix: rocker(k.pickup, key),
            t60,
            image,
        }
    }

    /// The dry patch: `strike_based` through [`ClavBase::DRY`]: what
    /// `mod.rs`'s partial test strikes with.
    #[allow(dead_code)]
    pub fn strike_knobs(&mut self, pitch: u8, velocity: f32, k: &ClavKnobs) {
        self.strike_based(pitch, velocity, k, &ClavBase::DRY);
    }

    pub fn strike_based(&mut self, pitch: u8, velocity: f32, k: &ClavKnobs, base: &ClavBase) {
        // The tangent lands on the string as this frame finds it: the
        // control block it falls in ends here, and the next starts with it.
        self.close_block();
        let v = velocity.clamp(1.0 / 127.0, 1.0);
        let d = self.design(pitch, k, base);
        let f0 = self.rate / d.total;

        // **A tangent landing on a string that is still moving adds to the
        // motion.** The loop, the coils and the filters keep their memory;
        // only a string of another design is cleared, and the frame keeps
        // that from being audible.
        let landing = !self.done && self.fits(&d);
        if landing {
            // The choke's take so far, made real in the string, so the
            // coils see the same displacement on the next sample, and the
            // tip's own catch on the moving wire.
            let c = self.choke * STRIKE_CATCH;
            for s in &mut self.buf {
                *s *= c;
            }
            self.th.x1 *= c;
            self.th.y1 *= c;
            self.loss.z *= c;
            for ap in &mut self.disp {
                ap.x1 *= c;
                ap.y1 *= c;
            }
        }
        if !landing {
            self.clear_state();
        }
        self.m = d.m;
        self.th.a = d.thiran;
        self.loss.a = d.loss_a;
        self.gain = d.gain;
        for ap in &mut self.disp {
            ap.a = d.disp_a;
        }
        self.anvil = d.anvil;
        self.taps = d.taps;
        self.total = d.total;
        self.t60 = d.t60;
        self.mix = d.mix;
        // The output stage: a landing strike is on the same stage (`fits`
        // asks), so its states run on; anything else starts from rest.
        if !landing {
            self.image = d.image;
        }
        self.pitch = pitch;

        // **The pulse.** A raised cosine as long as the contact lasts at
        // this velocity and this tip, through the zero-mean high-pass and
        // the note-scaled low-pass — and never longer than
        // [`CONTACT_MAX_PERIODS`] of the note.
        let tip = TIP_TRAVEL.powf(1.0 - 2.0 * k.voicing.clamp(0.0, 1.0));
        let secs = (PULSE_SECS_FF * v.powf(-PULSE_VEL_EXP) * tip.powf(TIP_LEN_EXP)).min(CONTACT_MAX_PERIODS / f0);
        let n = (secs * self.rate).max(3.0);
        self.pulse_n = n.ceil() as u32;
        self.pulse_ph = std::f32::consts::TAU / n;
        self.pulse_i = 0;
        self.exc_left = self.pulse_n + (PULSE_TAIL_SECS * self.rate) as u32;
        self.pulse_amp = v.powf(LEVEL_VEL_EXP) * key_gain(f32::from(pitch));
        // The steps, each STEP_AREA of the contact's area at C4 fortissimo:
        // the contact's area is n/2 samples of its crest, a step's ne/2.
        let ne = (STEP_SECS * self.rate).max(3.0);
        self.step_n = (ne.ceil() as u32).min(self.pulse_n / 2);
        self.step_ph = std::f32::consts::TAU / ne;
        self.step_at = self.pulse_n - self.step_n;
        let key_law = (261.625_55 / f0).min(1.0).powf(STEP_KEY_EXP);
        self.step_amp = STEP_AREA * v.powf(STEP_VEL_EXP) * key_law * n / ne;
        self.hp_s1 = 0.0;
        self.hp_s2 = 0.0;
        self.lp_z = 0.0;
        let lp_hz = (PULSE_LP_PARTIALS * f0).min(PULSE_LP_MAX_HZ) * tip.powf(-TIP_LP_EXP);
        self.lp_a = 1.0 - (-std::f32::consts::TAU * lp_hz.min(0.45 * self.rate) / self.rate).exp();

        // The coils: integrator leak and gain for this pitch, and the scale
        // that refers the flux's derivative back to the loop's level.
        self.coil_leak = (-std::f32::consts::TAU * INT_LEAK_HZ / self.rate).exp();
        self.coil_k = d.omega * DRIVE;
        self.coil_out = COIL_X0.exp() / (d.omega * DRIVE);

        // The quadratic term, bass-weighted.
        let f_c4 = 261.625_55_f32;
        self.q = QE_Q * (f_c4 / f0).powf(QE_KEY_EXP) * v.powf(QE_VEL_EXP);
        if !landing {
            self.q_now = self.q;
        }

        // The copy: a resonator a few cents flat, its life the string's,
        // its send bass-weighted down to C2 and held there.
        let rate64 = f64::from(self.rate);
        let fs = rate64 / f64::from(d.total) * 2.0_f64.powf(f64::from(SYMP_CENTS) / 1200.0);
        self.copy.tune(std::f64::consts::TAU * fs / rate64, f64::from(d.t60 * SYMP_T60_OVER_STRING), rate64);
        self.copy.send = SYMP_GROWTH * 2.0 / self.rate * (f_c4 / f0.max(SYMP_HOLD_HZ)).powf(SYMP_KEY_EXP);

        // The tone rockers: brilliant leaves the wire alone, soft is the
        // mellow switch pair down; 8010 Hz at the shipped 0.75, a
        // five-octave span around it.
        let corner = 8_010.0 * 2.0_f32.powf((k.tone.clamp(0.0, 1.0) - 0.75) * 5.0);
        self.tone_a = 1.0 - (-std::f32::consts::TAU * corner.min(0.45 * self.rate) / self.rate).exp();

        // The knock: seeded per STRIKE FROM REST (the key and how many
        // times this voice has started a note), so a repeated key is a new
        // knock as the reference's attack noise is new on every strike, and
        // a render still repeats. A voice's first strike is the key's own
        // seed, and a strike that lands on its own ringing string keeps
        // the seed it rang with: the frame holds a restrike to adding what
        // a fresh strike adds, and the wah patch's restrike through an
        // open pedal reads 1.37 of that test's 1.41 with this knock and up
        // to 1.45 with others (a brighter pedal, not a step; MERGE-ME).
        if !landing {
            self.strikes = self.strikes.wrapping_add(1);
        }
        let seed = u32::from(pitch).wrapping_mul(0x85EB_CA6B) ^ self.strikes.wrapping_sub(1).wrapping_mul(0xC2B2_AE35);
        self.rng = Rng((0x9E37_79B9 ^ seed) | 1);
        // Its level is the TOUCH's, not the string's: the key and the
        // tangent are the same mechanism at every pitch, so the knock does
        // not take the keyboard's balance table, which is the string's.
        self.knock.arm(self.rate, KNOCK_LEVEL * LEVEL * v.powf(LEVEL_VEL_EXP - KNOCK_VEL_TILT), &KNOCK);
        // A soft tangent arrives slower: the knock's rise lengthens toward
        // twice its fortissimo time at the softest touch (the reference's
        // first 5 ms sit 0.3 dB under the next 5 at v127 and 5 dB under at
        // v10).
        self.knock.rise_k = (-1.0 / (KNOCK_RISE_SECS * (2.0 - v) * self.rate)).exp();
        let knock_hz = KNOCK_LP_HZ_SOFT * (KNOCK_LP_HZ_HARD / KNOCK_LP_HZ_SOFT).powf(v);
        self.knock.lpq = BUTTER6_Q.map(|q| lowpass(knock_hz, q, self.rate));
        self.knock.p1_a = 1.0 - (-std::f32::consts::TAU * 60.0 / self.rate).exp();
        self.knock.p2_a = 1.0 - (-std::f32::consts::TAU * 600.0 / self.rate).exp();
        if !landing {
            self.knock.p1 = 0.0;
            self.knock.p2 = 0.0;
            self.knock.lp = [0.0; 6];
        }

        self.releasing = false;
        self.damping = false;
        self.choke = 1.0;
        self.choke_floor = 0.0;
        self.rel_k = k.release.clamp(0.1, 10.0);
        self.choke_rate = CHOKE_DB_PER_S * 2.0_f32.powf(CHOKE_OCT_EXP * (f32::from(pitch) - 60.0) / 12.0);
        self.rejoin = k.snap;
        // The follower keeps whichever is louder: it is the stealing
        // policy's view of the voice. Seeded with what the strike will
        // reach, so a fresh voice is never read as silent.
        let seed = self.pulse_amp * LEVEL * 0.3;
        self.env = if landing { self.env.max(seed) } else { seed };
        self.done = false;
    }

    /// The copy's radius for a t60 of `secs` at its own frequency.
    fn set_copy_t60(&mut self, secs: f32) {
        self.copy.set_t60(f64::from(secs), f64::from(self.rate));
    }

    fn clear_state(&mut self) {
        self.buf.fill(0.0);
        self.w = 0;
        self.th.x1 = 0.0;
        self.th.y1 = 0.0;
        self.loss.z = 0.0;
        for ap in &mut self.disp {
            ap.x1 = 0.0;
            ap.y1 = 0.0;
        }
        let magnet = self.magnet;
        self.coil[0].clear(&magnet);
        self.coil[1].clear(&magnet);
        self.q_dc = 0.0;
        self.copy.x = 0.0;
        self.copy.y = 0.0;
        self.tone_z = 0.0;
        self.tone_z2 = 0.0;
        self.image.clear();
        self.ms = 0.0;
        self.rejoining = false;
        self.ns_len = 0.0;
        self.ns_to = 0.0;
        self.ns_gain = 1.0;
        self.thock.left = 0;
        self.knock.left = 0;
    }

    /// Silence now: every state cleared, `finished` true — the voice a
    /// first strike can land on as if from `new`.
    pub fn silence(&mut self) {
        self.clear_state();
        self.exc_left = 0;
        self.choke = 1.0;
        self.releasing = false;
        self.damping = false;
        self.env = 0.0;
        self.done = true;
        self.ctl = Ctl::default();
    }

    /// The tangent lifts. The yarn chokes the string AND the string gets
    /// its far length back: after the key's travel the two halves rejoin
    /// and the loop grows by `2^(snap · 3 / 12)`.
    ///
    /// **How a frozen loop grows.** The loop is read, filtered and written;
    /// from here on its filters' output goes through the non-speaking line
    /// before it reaches the head. At a length of zero that is bit-for-bit
    /// what the loop was about to write itself, and then the length glides
    /// up: nothing in the loop is dropped, nothing silent is spliced in,
    /// and the output is continuous through the lift.
    pub fn release(&mut self, pedal: bool) {
        if pedal || self.done || self.releasing || self.damping {
            return;
        }
        // The lift starts from this frame (see `strike_based`).
        self.close_block();
        if self.done {
            return;
        }
        self.releasing = true;
        self.rel_t = 0;
        self.choke_floor = 10.0_f32.powf(tail_floor_db(f32::from(self.pitch)) / 20.0);
        // The copy is the struck string's own resonance: it goes with it,
        // a little behind the speaking length.
        self.set_copy_t60(COPY_RELEASE_SECS);
        // The thock tracks the note's ENERGY at the off, as the reference's
        // does, not its last peak: a peak follower reads a note's crest as
        // well as its level, and the crest falls through a hold.
        let amp = THOCK_LEVEL * self.ms.sqrt();
        #[cfg(test)]
        let amp = if self.quiet { 0.0 } else { amp };
        self.thock.arm(self.rate, amp, &THOCK);
        // A resonant band-pass for the thock: cookbook, constant 0 dB peak,
        // then a one-pole low-pass, so its top falls as a felt's does.
        let w0 = std::f32::consts::TAU * THOCK_HZ / self.rate;
        let (s, c) = w0.sin_cos();
        let alpha = s / (2.0 * THOCK_Q);
        let a0 = 1.0 + alpha;
        self.thock.bp = [alpha / a0, 0.0, -alpha / a0, -2.0 * c / a0, (1.0 - alpha) / a0];
        self.thock.bp_s1 = 0.0;
        self.thock.bp_s2 = 0.0;
        self.thock.lp = [0.0; 6];
        self.thock.lp_a = 1.0 - (-std::f32::consts::TAU * THOCK_LP_HZ / self.rate).exp();
        let ratio = 2.0_f32.powf(self.rejoin.clamp(0.0, 2.0) * REJOIN_SEMITONES / 12.0);
        let extra = (self.total * (ratio - 1.0)).min((NS_LEN - 2) as f32);
        // The tail's extra loss: what makes the string, joined or not,
        // decay [`TAIL_DB_PER_S`] once the yarn has it.
        let extra_db = (TAIL_DB_PER_S - 60.0 / self.t60).max(0.0);
        // SNAP at zero: the tangent does not lift clear — a string choked at
        // pitch, which is what a felt mute under the strings would do. The
        // yarn still has the whole wire, so the loop takes the tail's loss
        // at its own pitch; without it the floor rang on at the string's
        // full t60, eleven seconds after the off (the clav verifier).
        if extra < 0.5 {
            let f0 = self.rate / self.total;
            self.gain *= 10.0_f32.powf(-extra_db / (20.0 * f0));
            return;
        }
        self.ns_to = extra.round();
        self.ns_len = 0.0;
        // The rejoined string's tail: the extra loss per pass that makes the
        // longer loop decay [`TAIL_DB_PER_S`] at its own lower pitch.
        let f_rel = self.rate / (self.total + self.ns_to);
        self.ns_gain = 10.0_f32.powf(-extra_db / (20.0 * f_rel));
        self.ns.fill(0.0);
        self.ns_w = 0;
        self.rejoining = true;
    }

    pub fn damp(&mut self) {
        // The panic path and the patch change: choke without the lift.
        if self.done || self.damping {
            return;
        }
        self.close_block();
        if self.done {
            return;
        }
        self.damping = true;
        self.releasing = false;
        self.damp_k = (-1.0 / (DAMP_SECS * self.rate)).exp();
        self.set_copy_t60(DAMP_SECS * 6.9);
    }

    pub fn finished(&self) -> bool {
        self.done
    }

    pub fn level(&self) -> f32 {
        self.env
    }

    /// The choke's gain `t` samples after the off: the rate ramps in as a
    /// raised cosine over the ramp, then holds; the floor is the rejoined
    /// string, which the loop itself decays.
    fn choke_at(&self, t: f32) -> f32 {
        // The yarn has nothing to choke until the tip leaves the string.
        let t = (t - REJOIN_DELAY_SECS * self.rate).max(0.0);
        let ramp = CHOKE_RAMP_SECS * self.rel_k * self.rate;
        let rate = self.choke_rate / (self.rel_k * self.rate);
        let db = if t < ramp {
            rate * (0.5 * t - ramp / std::f32::consts::TAU * (std::f32::consts::PI * t / ramp).sin())
        } else {
            rate * (0.5 * ramp + (t - ramp))
        };
        self.choke_floor + (1.0 - self.choke_floor) * 10.0_f32.powf(-db.min(300.0) / 20.0)
    }

    /// One sample, mono: what the tests play, the voice through its spread
    /// and its A microphone, which is exactly either channel of the block
    /// path at WIDTH zero. The block path is [`Self::render`].
    #[allow(dead_code)] // the frame block-renders (`render`); kept for the spectrum tests
    pub fn tick(&mut self) -> f32 {
        if self.done {
            return 0.0;
        }
        let mut o = [0.0_f32; 1];
        self.close_block();
        self.begin_block(1);
        self.run(&mut o);
        let out = self.image.tick(o[0]).0;
        self.end_block();
        out
    }

    /// One sample before the output stage, its control run for that one
    /// sample (a block one frame long): `tick` without the spread, which
    /// the tests measure the spread against.
    #[cfg(test)]
    fn tick_bare(&mut self) -> f32 {
        let mut o = [0.0_f32; 1];
        self.close_block();
        self.begin_block(1);
        self.run(&mut o);
        self.end_block();
        o[0]
    }

    /// One block of this voice's stereo output, ADDED into `l` and `r`:
    /// the WAVE2 block contract. Nothing goes into `drive` — the clav has
    /// no shared body.
    pub fn render(&mut self, l: &mut [f32], r: &mut [f32], _drive: &mut [f32]) {
        if self.done {
            return;
        }
        let n = l.len().min(r.len()).min(BLOCK);
        let mut at = 0;
        while at < n && !self.done {
            if self.ctl.done == self.ctl.len {
                self.begin_block(BLOCK);
            }
            let m = (self.ctl.len - self.ctl.done).min(n - at);
            let mut o = [0.0_f32; BLOCK];
            self.run(&mut o[..m]);
            // The output stage hears each part before the block's end can
            // end the voice. It heard a block only after the voice's end of
            // it, so the voice's last block went through a stage its silence
            // had cleared, and with that block cut into parts by a host's
            // calls, how much of it did moved with the calls.
            let mut im = self.image;
            let (la, lb, ra, rb) = (im.la, im.lb, im.ra, im.rb);
            for i in 0..m {
                let (a, b) = im.tick(o[i]);
                l[at + i] += la * a + lb * b;
                r[at + i] += ra * a + rb * b;
            }
            self.image = im;
            if self.ctl.done == self.ctl.len {
                self.end_block();
            }
            at += m;
        }
    }

    /// The block rate's part of a control block of `n` frames, at its first
    /// frame: the release's choke and lift as steps a frame. Into `ctl`; the
    /// release's clock moves at the block's end (`end_block`), by the frames
    /// it ran.
    fn begin_block(&mut self, n: usize) {
        let mut choke_step = 1.0_f32;
        let mut ns_step = 0.0_f32;
        if self.releasing {
            let t0 = self.rel_t as f32;
            let g0 = self.choke_at(t0);
            let g1 = self.choke_at(t0 + n as f32);
            self.choke = g0;
            choke_step = (g1 / g0.max(1.0e-30)).powf(1.0 / n as f32);
            if self.rejoining && self.ns_len < self.ns_to {
                // The lift: a smoothstep over the rejoin after the key's
                // travel, held to the wave's own speed.
                let delay = REJOIN_DELAY_SECS * self.rate;
                let span = (REJOIN_SECS * self.rate).max(1.0);
                let s = |t: f32| {
                    let u = ((t - delay) / span).clamp(0.0, 1.0);
                    u * u * (3.0 - 2.0 * u)
                };
                let target = s(t0 + n as f32) * self.ns_to;
                ns_step = ((target - self.ns_len) / n as f32).clamp(0.0, REJOIN_MAX_STEP);
            }
        } else if self.damping {
            choke_step = self.damp_k;
        }
        self.ctl = Ctl { len: n, done: 0, choke_step, ns_step, peak: 0.0, energy: 0.0 };
    }

    /// An event lands inside a control block: the block ends where it
    /// stands, and the next one starts at the event.
    fn close_block(&mut self) {
        if self.ctl.done > 0 && self.ctl.done < self.ctl.len {
            self.end_block();
        }
    }

    /// The next `out.len()` frames of the control block, mono, before the
    /// output stage: at most what is left of it.
    fn run(&mut self, out: &mut [f32]) {
        let n = out.len();
        let Ctl { choke_step, ns_step, .. } = self.ctl;
        // The hot state in locals for the block: nothing the loop writes
        // through `self` can then alias it, and it stays in registers.
        let mut choke = self.choke;
        let (leak, ck) = (self.coil_leak, self.coil_k);
        let (mix0, mix1) = self.mix;
        let [t0, t1, t2, t3] = self.taps;
        let (m, coil_out) = (self.m, self.coil_out);
        let (th_a, mut th_x1, mut th_y1) = (self.th.a, self.th.x1, self.th.y1);
        let (loss_a, mut loss_z, gain) = (self.loss.a, self.loss.z, self.gain);
        let da = self.disp[0].a;
        let mut dx = [self.disp[0].x1, self.disp[1].x1, self.disp[2].x1, self.disp[3].x1];
        let mut dy = [self.disp[0].y1, self.disp[1].y1, self.disp[2].y1, self.disp[3].y1];
        let (mut c0, mut c1) = (self.coil[0], self.coil[1]);
        let (q, q_dc_a, mut q_dc) = (self.q, self.q_dc_a, self.q_dc);
        let (mut q_now, q_glide) = (self.q_now, self.q_glide);
        let mut copy = self.copy;
        let (tone_a, mut tone_z, mut tone_z2) = (self.tone_a, self.tone_z, self.tone_z2);
        let mut w = self.w;
        let mut peak = self.ctl.peak;
        let mut energy = self.ctl.energy;
        for slot in out.iter_mut() {
            // ── the tangent's pulse, into the loop at the anvil ──
            if self.exc_left > 0 {
                let h = if self.pulse_i < self.pulse_n {
                    let i = self.pulse_i;
                    let ph = (self.pulse_ph * i as f32).min(std::f32::consts::TAU);
                    let mut h = 0.5 - 0.5 * ph.cos();
                    // The steps: up as the tip arrives, down as the wire lands.
                    if i < self.step_n {
                        h += self.step_amp * (0.5 - 0.5 * (self.step_ph * i as f32).cos());
                    }
                    if i >= self.step_at {
                        h -= self.step_amp * (0.5 - 0.5 * (self.step_ph * (i - self.step_at) as f32).cos());
                    }
                    h
                } else {
                    0.0
                };
                self.pulse_i += 1;
                let x = h * self.pulse_amp;
                let [b0, b1, b2, a1, a2] = self.hp;
                let y = b0 * x + self.hp_s1;
                self.hp_s1 = b1 * x - a1 * y + self.hp_s2;
                self.hp_s2 = b2 * x - a2 * y;
                self.lp_z += self.lp_a * (y - self.lp_z);
                // The outgoing wave gains the tip's velocity; the line
                // stores it inverted (see `Tap` and `design`).
                let at = w.wrapping_sub(self.anvil) & LOOP_MASK;
                self.buf[at] -= self.lp_z;
                self.exc_left -= 1;
            }
            // ── the coils read the two travelling waves at their places ──
            let buf = &self.buf;
            let read = |t: Tap| {
                let a = buf[w.wrapping_sub(t.i) & LOOP_MASK];
                let b = buf[w.wrapping_sub(t.i + 1) & LOOP_MASK];
                a + (b - a) * t.f
            };
            let v_lo = read(t0) - read(t1);
            let v_hi = read(t2) - read(t3);
            // ── the loop: the bridge's reflection, all filters lumped ──
            let x = buf[w.wrapping_sub(m) & LOOP_MASK];
            let mut v = th_a * (x - th_y1) + th_x1;
            th_x1 = x;
            th_y1 = v;
            loss_z += loss_a * (v - loss_z);
            v = loss_z * gain;
            for (x1, y1) in dx.iter_mut().zip(dy.iter_mut()) {
                let y = da * (v - *y1) + *x1;
                *x1 = v;
                *y1 = y;
                v = y;
            }
            if self.rejoining {
                self.ns[self.ns_w] = v;
                let back = self.ns_len;
                let i = back.floor();
                let f = back - i;
                let i = i as usize;
                let a = self.ns[self.ns_w.wrapping_sub(i) & NS_MASK];
                let b = self.ns[self.ns_w.wrapping_sub(i + 1) & NS_MASK];
                v = (a + (b - a) * f) * self.ns_gain;
                self.ns_w = (self.ns_w + 1) & NS_MASK;
                self.ns_len = (self.ns_len + ns_step).min(self.ns_to);
            }
            self.buf[w] = v;
            w = (w + 1) & LOOP_MASK;
            // ── the magnets, the rocker, the choke on what they read ──
            let c = choke;
            choke *= choke_step;
            let o_lo = c0.tick(v_lo * c, ck, leak);
            let o_hi = c1.tick(v_hi * c, ck, leak);
            let y = (mix0 * o_lo + mix1 * o_hi) * coil_out;
            // ── the quadratic term, its DC taken out ──
            let y2 = y * y;
            q_dc += q_dc_a * (y2 - q_dc);
            q_now += q_glide * (q - q_now);
            let yq = y + q_now * (y2 - q_dc);
            // ── the near-copy ──
            let ys = yq + copy.tick(yq);
            // ── the tone rockers, both poles ──
            tone_z += tone_a * (ys - tone_z);
            tone_z2 += tone_a * (tone_z - tone_z2);
            let mut o = tone_z2 * LEVEL;
            // ── the knock and the thock ──
            if self.knock.left > 0 {
                let e = self.knock.envelope();
                let wn = self.rng.next();
                let b = &mut self.knock;
                b.p1 += b.p1_a * (wn - b.p1);
                b.p2 += b.p2_a * (wn - b.p2);
                let mut x = 4.0 * b.p1 + 1.2 * b.p2 + KNOCK_FLAT * wn;
                for (j, &[b0, b1, b2, a1, a2]) in b.lpq.iter().enumerate() {
                    let y = b0 * x + b.lp[2 * j];
                    b.lp[2 * j] = b1 * x - a1 * y + b.lp[2 * j + 1];
                    b.lp[2 * j + 1] = b2 * x - a2 * y;
                    x = y;
                }
                o += x * e;
            }
            if self.thock.left > 0 {
                let e = self.thock.envelope();
                let wn = self.rng.next();
                let b = &mut self.thock;
                let [b0, b1, b2, a1, a2] = b.bp;
                let y = b0 * wn + b.bp_s1;
                b.bp_s1 = b1 * wn - a1 * y + b.bp_s2;
                b.bp_s2 = b2 * wn - a2 * y;
                b.lp[0] += b.lp_a * (y - b.lp[0]);
                o += b.lp[0] * e;
            }
            *slot = o;
            peak = peak.max(o.abs());
            energy += o * o;
        }
        self.w = w;
        self.th.x1 = th_x1;
        self.th.y1 = th_y1;
        self.loss.z = loss_z;
        for (d, (x1, y1)) in self.disp.iter_mut().zip(dx.iter().zip(dy.iter())) {
            d.x1 = *x1;
            d.y1 = *y1;
        }
        self.coil = [c0, c1];
        self.q_dc = q_dc;
        self.q_now = q_now;
        self.copy = copy;
        self.tone_z = tone_z;
        self.tone_z2 = tone_z2;
        self.choke = choke;
        self.ctl.peak = peak;
        self.ctl.energy = energy;
        self.ctl.done += n;
        // The guard, on every part: nothing non-finite may be handed on.
        let sum = out.iter().fold(0.0_f32, |s, &v| s + v) + self.copy.y + self.coil[0].x;
        if !sum.is_finite() {
            out.fill(0.0);
            self.silence();
        }
    }

    /// The end of a control block, full or cut short by an event (after
    /// `done` of its frames): the release's clock, the coils' resync, the
    /// followers, the end.
    fn end_block(&mut self) {
        let Ctl { done: n, peak, energy, .. } = self.ctl;
        self.ctl = Ctl::default();
        if self.done || n == 0 {
            return;
        }
        if self.releasing {
            self.rel_t = self.rel_t.saturating_add(n as u32);
        }
        let magnet = self.magnet;
        self.coil[0].resync(&magnet);
        self.coil[1].resync(&magnet);
        let fall = (-(n as f32) / (0.035 * self.rate)).exp();
        self.env = (self.env * fall).max(peak);
        let k = (-(n as f32) / (THOCK_RMS_SECS * self.rate)).exp();
        self.ms = self.ms * k + (1.0 - k) * energy / n as f32;
        if self.env < DONE_LEVEL && self.exc_left == 0 && self.knock.left == 0 && self.thock.left == 0 {
            self.silence();
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::TAU;

    const RATE: f32 = 48_000.0;

    fn knobs() -> ClavKnobs {
        ClavKnobs::from_params(&super::super::Patch::Clav.defaults())
    }

    fn f_of(key: u8) -> f32 {
        440.0 * 2.0_f32.powf((f32::from(key) - 69.0) / 12.0)
    }

    fn db(x: f64) -> f64 {
        20.0 * x.max(1e-30).log10()
    }

    /// What a test may switch off, to measure one mechanism by difference.
    #[derive(Clone, Copy)]
    struct Parts {
        qe: bool,
        copy: bool,
        noise: bool,
    }

    const ALL: Parts = Parts { qe: true, copy: true, noise: true };
    const STRING: Parts = Parts { qe: false, copy: false, noise: false };

    impl ClavVoice {
        fn strip(&mut self, p: Parts) {
            if !p.qe {
                self.q = 0.0;
                self.q_now = 0.0;
            }
            if !p.copy {
                self.copy.send = 0.0;
            }
            if !p.noise {
                self.knock.left = 0;
                self.quiet = true;
            }
        }
    }

    /// One voice struck once, mono through `tick`, `secs` long.
    fn strike(pitch: u8, vel: f32, k: &ClavKnobs, p: Parts, secs: f32) -> Vec<f32> {
        let mut v = ClavVoice::new(RATE);
        v.strike_based(pitch, vel, k, &ClavBase::DRY);
        v.strip(p);
        (0..(secs * RATE) as usize).map(|_| v.tick()).collect()
    }

    /// One voice struck once and released after `held`, mono.
    fn strike_release(pitch: u8, vel: f32, k: &ClavKnobs, p: Parts, held: f32, secs: f32) -> Vec<f32> {
        let mut v = ClavVoice::new(RATE);
        v.strike_based(pitch, vel, k, &ClavBase::DRY);
        v.strip(p);
        let off = (held * RATE) as usize;
        (0..(secs * RATE) as usize)
            .map(|i| {
                if i == off {
                    v.release(false);
                }
                v.tick()
            })
            .collect()
    }

    /// One voice through the block path, stereo.
    fn strike_stereo(pitch: u8, vel: f32, k: &ClavKnobs, secs: f32) -> (Vec<f32>, Vec<f32>) {
        let mut v = ClavVoice::new(RATE);
        v.strike_based(pitch, vel, k, &ClavBase::DRY);
        let n = (secs * RATE) as usize;
        let (mut l, mut r, mut d) = (vec![0.0; n], vec![0.0; n], vec![0.0; n]);
        let mut at = 0;
        while at < n {
            let m = 128.min(n - at);
            v.render(&mut l[at..at + m], &mut r[at..at + m], &mut d[at..at + m]);
            at += m;
        }
        (l, r)
    }

    /// Blackman-Harris windowed magnitude at `f`, amplitude-normalised: the
    /// clav verifier's measuring stick, so the numbers below are the ones
    /// its scripts read on the reference.
    fn bh_at(x: &[f32], f: f64) -> f64 {
        bh_at_rate(x, f, RATE)
    }

    /// [`bh_at`] for a render at `rate`.
    fn bh_at_rate(x: &[f32], f: f64, rate: f32) -> f64 {
        let n = x.len();
        let w = TAU * f / f64::from(rate);
        let (mut re, mut im, mut ws) = (0.0_f64, 0.0_f64, 0.0_f64);
        for (i, &v) in x.iter().enumerate() {
            let t = TAU * i as f64 / (n - 1) as f64;
            let win = 0.35875 - 0.48829 * t.cos() + 0.14128 * (2.0 * t).cos() - 0.01168 * (3.0 * t).cos();
            ws += win;
            let ph = w * i as f64;
            re += f64::from(v) * win * ph.cos();
            im -= f64::from(v) * win * ph.sin();
        }
        2.0 * re.hypot(im) / ws
    }

    /// The strongest BH magnitude within `rel` of `f` (search, never pin).
    fn bh_peak(x: &[f32], f: f64, rel: f64) -> (f64, f64) {
        let mut best = (0.0, f);
        for s in 0..=40 {
            let g = f * (1.0 - rel + 2.0 * rel * f64::from(s) / 40.0);
            let m = bh_at(x, g);
            if m > best.0 {
                best = (m, g);
            }
        }
        best
    }

    /// Partials 2..=n of a window in dB re the first, and the first's dB.
    fn partials(x: &[f32], f0: f32, n: usize) -> (f64, Vec<f64>) {
        let (m1, f1) = bh_peak(x, f64::from(f0), 0.03);
        let rel = (2..=n).map(|k| db(bh_peak(x, k as f64 * f1, 0.01).0 / m1)).collect();
        (db(m1), rel)
    }

    fn window(x: &[f32], a: f32, b: f32) -> &[f32] {
        &x[(a * RATE) as usize..((b * RATE) as usize).min(x.len())]
    }

    fn rms_db(x: &[f32]) -> f64 {
        let e: f64 = x.iter().map(|&v| f64::from(v) * f64::from(v)).sum();
        10.0 * (e / x.len().max(1) as f64).max(1e-30).log10()
    }

    fn peak(x: &[f32]) -> f32 {
        x.iter().fold(0.0_f32, |m, &v| m.max(v.abs()))
    }
    /// The loop's own frequency response, every element exact: the line,
    /// the fraction's all-pass, the loss one-pole, the four dispersion
    /// sections. Returns the frequency (Hz) and t60 (s) of mode `n`, solved
    /// from the phase condition by Newton's method.
    fn loop_mode(d: &Design, rate: f64, n: usize) -> (f64, f64) {
        let p = 1.0 - f64::from(d.loss_a);
        let (th, ap) = (f64::from(d.thiran), f64::from(d.disp_a));
        let phase = |w: f64| d.m as f64 * w + ap_lag(w, th) + lp_lag(w, p) + 4.0 * ap_lag(w, ap);
        let mut w = TAU * n as f64 / f64::from(d.total);
        for _ in 0..50 {
            let h = 1e-7;
            let dv = (phase(w + h) - phase(w - h)) / (2.0 * h);
            w -= (phase(w) - TAU * n as f64) / dv;
        }
        let (sw, cw) = w.sin_cos();
        let mag = f64::from(d.gain) * (1.0 - p) / ((1.0 - p * cw).powi(2) + (p * sw).powi(2)).sqrt();
        let passes = rate * w / (TAU * n as f64);
        (w * rate / TAU, -60.0 / (20.0 * mag.log10() * passes))
    }

    /// **The loop is lossless and exact.** Solved from its own coefficients
    /// at three rates and every key from F1 to C8: the fundamental within a
    /// cent of the key's frequency (the fraction's all-pass phase is in the
    /// sum by construction), the fundamental's t60 the design's to a tenth
    /// of a per cent, the loss filter exact at its second point and the
    /// eighth partial on it to two per cent — nothing in the loop has a magnitude but the loss
    /// filter. With the linear read this replaces, C4's n8 measured 0.63 s
    /// of a designed 0.76 and C8 lived 50 ms (the verifier's loop model).
    #[test]
    fn the_loop_is_lossless_and_its_fraction_an_all_pass() {
        for rate in [44_100.0_f32, 48_000.0, 96_000.0] {
            let v = ClavVoice::new(rate);
            let mut worst = (0.0_f64, 0.0_f64, 0.0_f64);
            for key in 29u8..=108 {
                let d = v.design(key, &knobs(), &ClavBase::DRY);
                let f0 = f64::from(f_of(key));
                let (f1, t1) = loop_mode(&d, f64::from(rate), 1);
                let cents = 1200.0 * (f1 / f0).log2();
                let e1 = (t1 / f64::from(d.t60) - 1.0).abs();
                assert!(cents.abs() < 1.0, "{rate} key {key}: the fundamental sits {cents:+.2} cents off");
                assert!(e1 < 1e-3, "{rate} key {key}: n1 t60 {t1:.3} s against the design's {}", d.t60);
                worst.0 = worst.0.max(cents.abs());
                worst.1 = worst.1.max(e1);
                let f_hi = (LOSS_PARTIAL * f_of(key)).min(LOSS_RATE_FRAC * rate);
                let want = f64::from(d.t60 * t60_ratio(f_hi).clamp(0.03 / d.t60, 1.0));
                // The loss solve itself, exact at its own second point.
                let p = 1.0 - f64::from(d.loss_a);
                let w2 = TAU * f64::from(f_hi) / f64::from(rate);
                let mag = f64::from(d.gain) * (1.0 - p) / ((1.0 - p * w2.cos()).powi(2) + (p * w2.sin()).powi(2)).sqrt();
                let rho2 = 10.0_f64.powf(-3.0 / (want * f64::from(rate) / f64::from(d.total)));
                assert!((mag / rho2 - 1.0).abs() < 2e-5, "{rate} key {key}: |H| {mag} at f_hi against {rho2}");
                if f_hi == LOSS_PARTIAL * f_of(key) && key <= 88 {
                    // Mode 8 sits `√(1 + 64B)` sharp of 8·f0, where the
                    // one-pole is steeper in the treble: 2 % to C6, 5 % to
                    // E6, the top of the reference's range (above it the
                    // schedule's stretch is an extrapolation).
                    let (_, t8) = loop_mode(&d, f64::from(rate), 8);
                    let e8 = (t8 / want - 1.0).abs();
                    let tol = if key < 84 { 0.02 } else { 0.05 };
                    assert!(e8 < tol, "{rate} key {key}: n8 t60 {t8:.3} s against {want:.3}");
                    worst.2 = worst.2.max(e8);
                }
            }
            println!(
                "  {rate} Hz: worst fundamental {:.3} c, n1 t60 {:.4} %, n8 t60 {:.2} %",
                worst.0,
                worst.1 * 100.0,
                worst.2 * 100.0
            );
        }
    }

    /// The fundamental's t60 on a render — the string alone (the copy and
    /// the quadratic term off, which would beat and bloom on it) — against
    /// the reference's law, and the eighth partial against its ratio:
    /// C4 n8/n1 0.72 (the reference 0.70), C5 0.40 (0.38).
    #[test]
    fn the_sustain_is_the_reference_law() {
        let slope = |x: &[f32], f: f64| {
            // Least-squares dB slope of 0.2 s BH frames from 0.5 to 4.5 s.
            let (mut st, mut sl, mut stt, mut stl, mut n) = (0.0, 0.0, 0.0, 0.0, 0.0);
            let mut t = 0.5_f32;
            while t <= 4.5 {
                let l = db(bh_peak(window(x, t - 0.1, t + 0.1), f, 0.004).0);
                let tf = f64::from(t);
                st += tf;
                sl += l;
                stt += tf * tf;
                stl += tf * l;
                n += 1.0;
                t += 0.25;
            }
            (n * stl - st * sl) / (n * stt - st * st)
        };
        for (key, ratio_want) in [(36u8, None), (48, None), (60, Some(0.72)), (72, Some(0.40)), (84, None)] {
            let x = strike(key, 80.0 / 127.0, &knobs(), STRING, 4.8);
            let f0 = f64::from(f_of(key));
            let t1 = -60.0 / slope(&x, f0);
            let law = f64::from(t60_law(f32::from(key)));
            print!("  key {key}: n1 t60 {t1:.2} s, law {law:.2} s");
            assert!((t1 / law - 1.0).abs() < 0.05, "key {key}: n1 t60 {t1:.2} s against the law's {law:.2}");
            if let Some(r) = ratio_want {
                let b = f64::from(stiffness(f32::from(key), 1.0));
                let f8 = 8.0 * f0 * ((1.0 + b * 64.0) / (1.0 + b)).sqrt();
                let t8 = -60.0 / slope(&x, f8);
                print!(", n8 t60 {t8:.2} s, n8/n1 {:.2}", t8 / t1);
                assert!((t8 / t1 - r).abs() < 0.05, "key {key}: n8/n1 {:.2} against {r}", t8 / t1);
            }
            println!();
        }
        assert!((t60_law(60.0) - 8.06).abs() < 1e-4);
        assert!((t60_ratio(2093.0) - 0.72).abs() < 1e-3, "{}", t60_ratio(2093.0));
        assert!((t60_ratio(4186.0) - 0.40).abs() < 0.01, "{}", t60_ratio(4186.0));
    }

    /// **The stiffness is the reference's.** Partials found on a render
    /// (BH, 0.2..1.8 s, searched), `B` fitted on `(f_n/n)² = f0²(1 + B n²)`
    /// over n ≤ 12, against the schedule: within ×1.5 from G1 to E6, as the
    /// brief asks, and B > 0 at the top where the old map's clamp left C6
    /// at 3e−6.
    #[test]
    fn the_realised_stiffness_is_the_reference_schedule() {
        for key in [31u8, 36, 48, 60, 72, 76, 79, 84, 88] {
            let x = strike(key, 80.0 / 127.0, &knobs(), STRING, 1.9);
            let w = window(&x, 0.2, 1.8);
            let (_, f1) = bh_peak(w, f64::from(f_of(key)), 0.02);
            let want = f64::from(stiffness(f32::from(key), 1.0));
            let nmax = (0.4 * f64::from(RATE) / f1).floor().min(12.0) as usize;
            let (mut a, mut b, mut c, mut dd, mut e) = (0.0, 0.0, 0.0, 0.0, 0.0);
            for n in 1..=nmax {
                let nf = n as f64;
                let guess = nf * f1 * ((1.0 + want * nf * nf) / (1.0 + want)).sqrt();
                let (m, f) = bh_peak(w, guess, 0.004);
                if db(m) < -150.0 {
                    continue;
                }
                let y = (f / nf).powi(2);
                let xx = nf * nf;
                a += 1.0;
                b += xx;
                c += xx * xx;
                dd += y;
                e += xx * y;
            }
            let slope = (a * e - b * dd) / (a * c - b * b);
            let icpt = (dd - slope * b) / a;
            let got = slope / icpt;
            println!("  key {key}: B {got:.2e}, schedule {want:.2e} (x{:.2})", got / want);
            assert!((got / want) > 1.0 / 1.5 && (got / want) < 1.5, "key {key}: B {got:.2e} against {want:.2e}");
        }
    }

    /// **The coil is `pickup.rs`'s Exponential transducer.** The same
    /// displacement driven through both — the voice's advanced flux,
    /// re-taken from the transducer's curve every 128 samples, and the
    /// transducer's own derivative of its curve — at the reference swing
    /// of 0.6 gaps and at a fortissimo 1.5: the same output to a part in a
    /// thousand of its peak.
    #[test]
    fn the_coil_is_the_exponential_transducer() {
        use super::super::pickup::{Curve, Transducer};
        for swing in [0.6_f32, 1.5] {
            let omega = std::f32::consts::TAU * 220.0 / RATE;
            let mut t = Transducer::new(Curve::Exponential { scale: 1.0 });
            t.calibrate(0.0);
            let magnet = Transducer::new(Curve::Exponential { scale: 1.0 });
            let mut c = Coil::default();
            c.clear(&magnet);
            let (k, leak) = (omega * 1.0, 1.0_f32);
            let mut worst = 0.0_f32;
            let mut top = 0.0_f32;
            for i in 0..9600 {
                let v = swing * omega * (omega * i as f32).cos();
                let fall = c.tick(v, k, leak);
                let mine = fall * COIL_X0.exp() / omega;
                let theirs = -t.tick(c.x, omega);
                worst = worst.max((mine - theirs).abs());
                top = top.max(theirs.abs());
                if i % 128 == 127 {
                    c.resync(&magnet);
                }
            }
            println!("  swing {swing}: worst difference {:.2e} of a peak {top:.3}", worst);
            assert!(worst < 1e-3 * top, "swing {swing}: the coil and the transducer differ by {worst}");
        }
    }

    /// The contact's spectrum at C4's partials for a velocity and a voicing,
    /// dB re its DC: the pulse exactly as `strike_based` pours it, its two
    /// steps included, before the high-pass and the low-pass (which have no
    /// nulls).
    fn contact_spectrum(vel: f32, voicing: f32) -> Vec<f64> {
        let v = vel.max(1.0 / 127.0);
        let tip = TIP_TRAVEL.powf(1.0 - 2.0 * voicing);
        let secs = PULSE_SECS_FF * v.powf(-PULSE_VEL_EXP) * tip.powf(TIP_LEN_EXP);
        let n = (secs * RATE).max(3.0);
        let len = n.ceil() as usize;
        let ph = std::f32::consts::TAU / n;
        let ne = (STEP_SECS * RATE).max(3.0);
        let (sn, sph) = ((ne.ceil() as usize).min(len / 2), std::f32::consts::TAU / ne);
        let amp = STEP_AREA * v.powf(STEP_VEL_EXP) * n / ne;
        let bump = |j: usize| amp * (0.5 - 0.5 * (sph * j as f32).cos());
        let p: Vec<f64> = (0..len)
            .map(|i| {
                let mut h = 0.5 - 0.5 * (ph * i as f32).min(std::f32::consts::TAU).cos();
                if i < sn {
                    h += bump(i);
                }
                if i >= len - sn {
                    h -= bump(i - (len - sn));
                }
                f64::from(h)
            })
            .collect();
        let dc: f64 = p.iter().sum();
        (1..=10)
            .map(|k| {
                let w = TAU * k as f64 * f64::from(f_of(60)) / f64::from(RATE);
                let (re, im) = p.iter().enumerate().fold((0.0, 0.0), |(re, im), (i, &v)| {
                    (re + v * (w * i as f64).cos(), im - v * (w * i as f64).sin())
                });
                db(re.hypot(im) / dc)
            })
            .collect()
    }

    /// **The null table.** A compact pulse has spectral zeros near multiples
    /// of its own reciprocal length, and this module once shipped a pulse
    /// whose zero sat on C4's fifth and sixth partials at every velocity.
    /// The raised-cosine contact's first null is `2/τ`: at 0.70 ms and
    /// fortissimo that is 2.86 kHz, C4's eleventh partial, and the longest
    /// contact the knobs make — MIDI 1, the softest voicing — puts it at
    /// 2.40 kHz, past the ninth (the steps only fill nulls). A null is a
    /// partial 8 dB under both its neighbours; none on the first eight,
    /// anywhere.
    #[test]
    fn the_pulse_has_no_null_on_c4s_first_eight_partials() {
        for voicing in [0.0_f32, 0.5, 1.0] {
            for midi in [1u8, 4, 8, 16, 32, 64, 100, 127] {
                let sp = contact_spectrum(f32::from(midi) / 127.0, voicing);
                for n in 1..=8usize {
                    let here = sp[n - 1];
                    let lo = if n == 1 { 0.0 } else { sp[n - 2] };
                    let hi = sp[n];
                    assert!(
                        !(here < lo - 8.0 && here < hi - 8.0),
                        "voicing {voicing} MIDI {midi}: partial {n} at {here:.1} dB sits in a null ({lo:.1} below, {hi:.1} above)"
                    );
                }
            }
            let soft = contact_spectrum(1.0 / 127.0, voicing);
            println!(
                "  voicing {voicing} MIDI 1: {}",
                soft.iter().map(|v| format!("{v:+.1}")).collect::<Vec<_>>().join(" ")
            );
        }
    }

    /// The swing DRIVE produces: E4 at MIDI 76, the largest displacement
    /// either coil sees in the first 0.25 s (the strike's own transient),
    /// in gaps. Pinned because it sets the coil's distortion — at 0.048 the
    /// reference's velocity-invariant C4 timbre holds (H2..H5 within 1.5 dB
    /// from v10 to v127, `the_attack_is_the_references`); the old 0.6-gap
    /// axis moved them 2..6 dB and lit C6's third and fourth partials 20 dB
    /// over the reference.
    #[test]
    fn the_string_swings_the_measured_gaps_at_the_coil() {
        let swing_at = |vel: f32| {
            let mut v = ClavVoice::new(RATE);
            v.strike_based(64, vel, &knobs(), &ClavBase::DRY);
            (0..(0.25 * RATE) as usize)
                .map(|_| {
                    v.tick();
                    v.coil[0].x.abs().max(v.coil[1].x.abs())
                })
                .fold(0.0_f32, f32::max)
        };
        let mf = swing_at(76.0 / 127.0);
        let ff = swing_at(1.0);
        println!("  swing at MIDI 76: {mf:.3} gaps; MIDI 127: {ff:.3}");
        assert!((mf / SWING_MF - 1.0).abs() < 0.10, "MIDI 76 swings {mf:.3} gaps, not {SWING_MF}");
        assert!(ff > mf && ff < 2.0 * mf, "MIDI 127 swings {ff:.3} gaps");
    }

    /// Partial levels 1..=n, dB, BH-searched on the stretched series.
    fn series(x: &[f32], key: u8, n: usize) -> Vec<f64> {
        let (_, f1) = bh_peak(x, f64::from(f_of(key)), 0.02);
        let b = f64::from(stiffness(f32::from(key), 1.0));
        (1..=n)
            .map(|k| {
                let kf = k as f64;
                db(bh_peak(x, kf * f1 * ((1.0 + b * kf * kf) / (1.0 + b)).sqrt(), 0.003).0)
            })
            .collect()
    }

    /// **Each coil hears its own comb, where the reference's sit.** The
    /// lower coil alone and the upper alone on the same strike, the
    /// string alone (QE, copy, noise off), 0.1..1.9 s: the ratio of the two
    /// cancels the string's own spectrum and leaves
    /// `|sin(nπ·x_hi)| / |sin(nπ·x_lo)|`, grid-fitted exactly as the clav
    /// verifier fitted the reference's (`pick_check.py`). The lower coil's
    /// first notch `1/x_lo` within ±0.2 partial of the reference's 5.01 /
    /// 4.44 / 3.81 at C2 / C4 / C5.
    #[test]
    fn a_coil_hears_its_own_comb() {
        for (key, nmax, want) in [(36u8, 24usize, 5.01_f64), (60, 14, 4.44), (72, 10, 3.81)] {
            let mut k = knobs();
            k.pickup = 0.1;
            let lo = series(window(&strike(key, 80.0 / 127.0, &k, STRING, 2.0), 0.1, 1.9), key, nmax);
            k.pickup = 0.35;
            let hi = series(window(&strike(key, 80.0 / 127.0, &k, STRING, 2.0), 0.1, 1.9), key, nmax);
            let ratio: Vec<f64> = hi.iter().zip(&lo).map(|(h, l)| h - l).collect();
            let mut best = (f64::MAX, 0.0, 0.0);
            let mut xl = 0.12;
            while xl < 0.40 {
                let mut xh = 0.02;
                while xh < 0.16 {
                    let (mut sum, mut cnt, mut r) = (0.0, 0, Vec::new());
                    for n in 1..=nmax {
                        let sl = (n as f64 * std::f64::consts::PI * xl).sin().abs();
                        let sh = (n as f64 * std::f64::consts::PI * xh).sin().abs();
                        if db(sl) > -25.0 && db(sh) > -25.0 {
                            let e = ratio[n - 1] - (db(sh) - db(sl));
                            r.push(e);
                            sum += e;
                            cnt += 1;
                        }
                    }
                    if cnt >= 6 {
                        let mean = sum / f64::from(cnt);
                        let rms = (r.iter().map(|e| (e - mean).powi(2)).sum::<f64>() / f64::from(cnt)).sqrt();
                        if rms < best.0 {
                            best = (rms, xl, xh);
                        }
                    }
                    xh += 0.0005;
                }
                xl += 0.0005;
            }
            let (rms, xl, xh) = best;
            println!("  key {key}: fitted x_lo {xl:.4} (1/x {:.2}, reference {want}), x_hi {xh:.4} (1/x {:.2}), rms {rms:.2} dB", 1.0 / xl, 1.0 / xh);
            assert!((1.0 / xl - want).abs() <= 0.2, "key {key}: the lower coil's first notch at {:.2}, the reference's {want}", 1.0 / xl);
        }
    }

    /// **The attack is the reference's.** Partials re the fundamental over
    /// 5..205 ms, everything on, against the reference's L/R power average
    /// (its two microphones colour single partials ±5 dB, so the brief
    /// holds the mean): C4 v70 H2 / H3 / H4 / H5 / H6 / H8 +5.1 / −1.6 /
    /// +1.3 / −3.8 / −14.3 / −18.2, mean |error| ≤ 4 dB — H3 is the one the
    /// reference's own excitation dips and a contact of one shape cannot;
    /// C6 v127 H2 / H3 / H4 −6.2 / −31.7 / −46.7 each within ±5 dB; C4's
    /// H5..H10 brighten 4..6 dB from v10 to v127 (the reference 4.8) while
    /// H2..H5 move under 1.5 dB (the reference 0.7).
    #[test]
    fn the_attack_is_the_references() {
        let k = knobs();
        let at = |key: u8, vel: u8, n: usize| partials(window(&strike(key, f32::from(vel) / 127.0, &k, ALL, 0.25), 0.005, 0.205), f_of(key), n).1;
        let c4 = at(60, 70, 10);
        let want = [(2usize, 5.1), (3, -1.6), (4, 1.3), (5, -3.8), (6, -14.3), (8, -18.2)];
        let errs: Vec<f64> = want.iter().map(|&(n, w)| c4[n - 2] - w).collect();
        let mean = errs.iter().map(|e| e.abs()).sum::<f64>() / errs.len() as f64;
        println!("  C4 v70: {} (mean |error| {mean:.1} dB)", want.iter().zip(&errs).map(|(&(n, _), e)| format!("H{n} {:+.1} ({e:+.1})", c4[n - 2])).collect::<Vec<_>>().join(" "));
        assert!(mean <= 4.0, "C4 v70 attack mean error {mean:.1} dB");
        let c6 = at(84, 127, 4);
        for (n, w) in [(2usize, -6.2), (3, -31.7), (4, -46.7)] {
            println!("  C6 v127 H{n} {:+.1} (reference {w})", c6[n - 2]);
            assert!((c6[n - 2] - w).abs() <= 5.0, "C6 v127 H{n} {:+.1} against {w}", c6[n - 2]);
        }
        let soft = at(60, 10, 10);
        let hard = at(60, 127, 10);
        let mid = |v: &[f64]| v[3..9].iter().sum::<f64>() / 6.0;
        let spread = mid(&hard) - mid(&soft);
        let low: f64 = (0..4).map(|i| (hard[i] - soft[i]).abs()).fold(0.0, f64::max);
        println!("  C4 mean(H5..H10) v10 {:+.1} v127 {:+.1}: spread {spread:+.1} dB; H2..H5 moved at most {low:.1} dB", mid(&soft), mid(&hard));
        assert!((4.0..=6.0).contains(&spread), "C4 H5..H10 velocity spread {spread:.1} dB");
        assert!(low <= 1.5, "C4 H2..H5 moved {low:.1} dB with velocity");
    }

    /// **The contact's steps carry the upper partials.** C4's attack above
    /// 3 kHz (partials 12..16, 5..205 ms), their energy summed re the
    /// fundamental: the reference's −30.8 / −26.1 / −19.8 / −14.9 dB at
    /// v10 / v40 / v70 / v127 (its dry velocity ramp, L/R power average),
    /// 15.9 dB of brightening on its own fundamental. The raised cosine
    /// alone gave a flat −32.6..−31.8 dB: nothing above the main lobe, and
    /// nothing for the touch to brighten.
    #[test]
    fn the_contact_steps_carry_the_upper_partials() {
        let k = knobs();
        let hf = |vel: u8, steps: bool| {
            let mut v = ClavVoice::new(RATE);
            v.strike_based(60, f32::from(vel) / 127.0, &k, &ClavBase::DRY);
            if !steps {
                v.step_amp = 0.0;
            }
            let x: Vec<f32> = (0..(0.25 * RATE) as usize).map(|_| v.tick()).collect();
            let (_, rel) = partials(window(&x, 0.005, 0.205), f_of(60), 16);
            let e: f64 = rel
                .iter()
                .enumerate()
                .filter(|&(i, _)| (i + 2) as f32 * f_of(60) > 3000.0)
                .map(|(_, &d)| 10.0_f64.powf(d / 10.0))
                .sum();
            10.0 * e.log10()
        };
        let want = [(10u8, -30.8), (40, -26.1), (70, -19.8), (127, -14.9)];
        let got: Vec<f64> = want.iter().map(|&(v, _)| hf(v, true)).collect();
        let bare = hf(127, false);
        println!(
            "  C4 above 3 kHz re n1: {} (the raised cosine alone at v127: {bare:+.1})",
            want.iter().zip(&got).map(|(&(v, w), g)| format!("v{v} {g:+.1} ({w})")).collect::<Vec<_>>().join(" ")
        );
        for (&(v, w), &g) in want.iter().zip(&got) {
            assert!((g - w).abs() <= 4.0, "C4 v{v}: {g:+.1} dB above 3 kHz against the reference's {w}");
        }
        assert!(got[3] - got[0] >= 10.0, "the touch brightens the top by only {:.1} dB", got[3] - got[0]);
        assert!(got[3] - bare >= 10.0, "the steps add only {:.1} dB at v127", got[3] - bare);
    }

    /// **The quadratic term fattens the bass, and only the bass.** The
    /// reference with its copy off gains +6.0 dB of C2 fundamental at v127
    /// in the attack and moves C4 under 0.3 dB (the brief: under 1); this
    /// term alone gives what
    /// is printed below (more, because this instrument's copy carries
    /// none of the attack, so the term carries the reference's copy's
    /// attack share too: the composite is what the brief holds). The
    /// composite: C2 H3/H1 +20 ± 3 dB at v25 and +9 ± 3 at v127, and the
    /// C2 fundamental growing 28..34 dB from v10 to v127 (the reference
    /// 32.9).
    #[test]
    fn the_quadratic_term_fattens_the_bass_only() {
        let k = knobs();
        let n1 = |key: u8, vel: u8, p: Parts| partials(window(&strike(key, f32::from(vel) / 127.0, &k, p, 0.25), 0.005, 0.205), f_of(key), 3);
        let no_copy = Parts { copy: false, ..ALL };
        let lin = Parts { qe: false, copy: false, noise: true };
        let c2 = n1(36, 127, no_copy).0 - n1(36, 127, lin).0;
        let c4 = n1(60, 127, no_copy).0 - n1(60, 127, lin).0;
        println!("  the term alone: C2 v127 fundamental {c2:+.1} dB, C4 v127 {c4:+.2} dB");
        assert!(c2 > 5.0, "C2 v127 gains only {c2:.1} dB");
        assert!(c4.abs() < 1.0, "C4 v127 moves {c4:.2} dB");
        // The composite at 44.1 and 48 kHz: the verifier read C2 v127 at
        // +12.6 at 44.1, over the band's top.
        for rate in [44_100.0_f32, 48_000.0] {
            let at = |vel: u8| {
                let mut v = ClavVoice::new(rate);
                v.strike_based(36, f32::from(vel) / 127.0, &k, &ClavBase::DRY);
                let x: Vec<f32> = (0..(0.25 * rate) as usize).map(|_| v.tick()).collect();
                let w = &x[(0.005 * rate) as usize..(0.205 * rate) as usize];
                let peak_near = |f: f64, rel: f64| {
                    (0..=40).map(|s| f * (1.0 - rel + 2.0 * rel * f64::from(s) / 40.0)).map(|g| (bh_at_rate(w, g, rate), g)).fold((0.0, f), |b, m| if m.0 > b.0 { m } else { b })
                };
                let (m1, f1) = peak_near(f64::from(f_of(36)), 0.03);
                let (m3, _) = peak_near(3.0 * f1, 0.01);
                (db(m1), db(m3 / m1))
            };
            let (s25, h25) = at(25);
            let (s127, h127) = at(127);
            let s10 = at(10).0;
            println!("  composite C2 at {rate}: H3/H1 v25 {h25:+.1} v127 {h127:+.1}; n1 v10 -> v127 {:+.1} dB (v25 {s25:+.1})", s127 - s10);
            assert!((17.0..=23.0).contains(&h25), "{rate}: C2 v25 H3/H1 {h25:+.1}");
            assert!((6.0..=12.0).contains(&h127), "{rate}: C2 v127 H3/H1 {h127:+.1}");
            assert!((28.0..=34.0).contains(&(s127 - s10)), "{rate}: C2 n1 growth {:+.1}", s127 - s10);
        }
    }

    /// The fundamental's level over a hold, 0.2 s BH frames centred at `t`.
    fn n1_track(x: &[f32], key: u8, at: &[f32]) -> Vec<f64> {
        at.iter().map(|&t| db(bh_peak(window(x, t - 0.1, t + 0.1), f64::from(f_of(key)), 0.01).0)).collect()
    }

    /// The copy alone, struck by an impulse and left to ring `secs`: its
    /// frequency from upward zero crossings over all of it.
    fn copy_rings_at(c: &mut NearCopy, rate: f32, secs: f32) -> f64 {
        c.x = 0.0;
        c.y = 0.0;
        let mut last = c.tick(1.0);
        let (mut first, mut final_, mut count) = (f64::NAN, 0.0_f64, 0usize);
        for i in 1..(secs * rate) as usize {
            let y = c.tick(0.0);
            if last <= 0.0 && y > 0.0 {
                let t = i as f64 - 1.0 + f64::from(-last) / f64::from(y - last);
                if first.is_nan() {
                    first = t;
                } else {
                    count += 1;
                }
                final_ = t;
            }
            last = y;
        }
        count as f64 / ((final_ - first) / f64::from(rate))
    }

    /// **The copy rings where it is tuned**, at every key and rate: struck
    /// alone with an impulse, its frequency (zero crossings over 8 s)
    /// within 0.1 c of [`SYMP_CENTS`] under the string's own fundamental
    /// (the loop's mode, solved from its coefficients), so the bass's beat
    /// is the same at 44.1, 48 and 96 kHz — the direct form's f32
    /// coefficients put C2's at −2.67 c / 0.101 Hz at 48 kHz, −4.10 c /
    /// 0.155 Hz at 96, C1 at −9.81 c and E1 at none.
    #[test]
    fn the_copy_rings_where_it_is_tuned() {
        for rate in [44_100.0_f32, 48_000.0, 96_000.0] {
            let mut line = String::new();
            for key in [21u8, 24, 28, 31, 36, 48, 60] {
                let mut v = ClavVoice::new(rate);
                v.strike_based(key, 0.6, &knobs(), &ClavBase::DRY);
                let d = v.design(key, &knobs(), &ClavBase::DRY);
                let (f_string, _) = loop_mode(&d, f64::from(rate), 1);
                let mut c = v.copy;
                let f_copy = copy_rings_at(&mut c, rate, 8.0);
                let cents = 1200.0 * (f_copy / f_string).log2();
                if key == 36 || key == 24 {
                    line.push_str(&format!(" key {key} {cents:+.2} c (beat {:.3} Hz)", f_string - f_copy));
                }
                assert!((cents - f64::from(SYMP_CENTS)).abs() <= 0.1, "{rate} key {key}: the copy rings {cents:+.2} c off the string");
                if key == 36 {
                    let beat = f_string - f_copy;
                    assert!((0.1..=0.8).contains(&beat), "{rate}: the C2 beat is {beat:.3} Hz");
                }
            }
            println!("  {rate} Hz:{line}");
        }
    }

    /// **The copy blooms the bass and leaves the middle alone.** C2 at v80,
    /// everything on: the fundamental re its 0.1 s frame rises +2..+3 dB
    /// by 1 s (the reference swells +2.2, the brief's band) and its beat is
    /// the copy's measured detune (`the_copy_rings_where_it_is_tuned`);
    /// below C2 the send holds C2's weight, so G1 rises no more than C2
    /// does (carried on down as `(f_C4/f0)^2.5` it rose +6.4 dB by 1.6 s,
    /// where the reference's G1 falls); C4's fundamental with the copy
    /// against without, at 0.5 / 1 / 2 s, under 1 dB (the reference's
    /// under 0.7).
    #[test]
    fn the_copy_blooms_the_bass_and_leaves_the_middle() {
        let k = knobs();
        let at = [0.105_f32, 0.5, 1.0, 1.5, 2.0];
        let c2 = n1_track(&strike(36, 80.0 / 127.0, &k, ALL, 2.2), 36, &at);
        let rise = c2[2] - c2[0];
        println!("  C2 v80 n1 re 0.1 s at 0.5 / 1 / 1.5 / 2 s: {:+.1} {rise:+.1} {:+.1} {:+.1}", c2[1] - c2[0], c2[3] - c2[0], c2[4] - c2[0]);
        assert!((2.0..=3.0).contains(&rise), "C2 n1 rises {rise:+.1} dB over the first second");
        let g1 = n1_track(&strike(31, 80.0 / 127.0, &k, ALL, 2.2), 31, &at);
        let g1_top = (1..5).map(|i| g1[i] - g1[0]).fold(f64::MIN, f64::max);
        println!("  G1 v80 n1 re 0.1 s at 0.5 / 1 / 1.5 / 2 s: {:+.1} {:+.1} {:+.1} {:+.1}", g1[1] - g1[0], g1[2] - g1[0], g1[3] - g1[0], g1[4] - g1[0]);
        assert!(g1_top <= 3.0, "G1's fundamental blooms {g1_top:+.1} dB");
        let with = n1_track(&strike(60, 80.0 / 127.0, &k, ALL, 2.2), 60, &at);
        let without = n1_track(&strike(60, 80.0 / 127.0, &k, Parts { copy: false, ..ALL }, 2.2), 60, &at);
        let worst = (1..5).map(|i| (with[i] - without[i]).abs()).fold(0.0, f64::max);
        println!("  C4 copy effect at most {worst:.2} dB");
        assert!(worst < 1.0, "the copy moves C4 by {worst:.2} dB");
    }

    /// 5 ms RMS of `x` centred `ms` after sample `at`, dB re `pre`.
    fn frame_db(x: &[f32], at: usize, ms: f32, pre: f64) -> f64 {
        let c = at + (ms * 1e-3 * RATE) as usize;
        let h = (0.0025 * RATE) as usize;
        rms_db(&x[c - h..c + h]) - pre
    }

    /// **The release is the reference's two stages.** C4 at v80 held 0.5 s:
    /// the broadband level re the 50 ms before the off, 5 ms frames, within
    /// ±3 dB of the reference's at +10 / 20 / 40 / 80 ms (−1.3 / −2.8 /
    /// −17.4 / −37.0, its L/R average, the clav verifier's method); the tail
    /// over 0.2..0.6 s at −55 ± 5 dB falling 25..36 dB/s; and the pitch,
    /// read with the clav verifier's analytic-signal tracker, gliding
    /// monotonically down to −100..−250 c at +20 ms (the reference reads
    /// −151..−186 c with it, the analyst −110..−120 with a 16 ms window: a
    /// range, the critic says).
    #[test]
    fn the_release_is_the_references() {
        let held = 0.5_f32;
        let x = strike_release(60, 80.0 / 127.0, &knobs(), ALL, held, held + 1.2);
        // The pitch is read with the noise off: the thock sits in the very
        // band the tracker listens to.
        let quiet = strike_release(60, 80.0 / 127.0, &knobs(), Parts { noise: false, ..ALL }, held, held + 0.1);
        let off = (held * RATE) as usize;
        let pre = rms_db(&x[off - (0.06 * RATE) as usize..off - (0.01 * RATE) as usize]);
        let got: Vec<f64> = [10.0_f32, 20.0, 40.0, 80.0].iter().map(|&ms| frame_db(&x, off, ms, pre)).collect();
        let want = [-1.3, -2.8, -17.4, -37.0];
        println!("  C4 release at +10/20/40/80 ms: {} (reference {:?})", got.iter().map(|v| format!("{v:+.1}")).collect::<Vec<_>>().join(" "), want);
        for (g, w) in got.iter().zip(want) {
            assert!((g - w).abs() <= 3.0, "release {g:+.1} dB against {w}");
        }
        let ts: Vec<f64> = (0..80).map(|i| 0.2 + 0.005 * f64::from(i)).collect();
        let ls: Vec<f64> = ts.iter().map(|&t| frame_db(&x, off, (t * 1000.0) as f32, pre)).collect();
        let mean = ls.iter().sum::<f64>() / ls.len() as f64;
        let (mt, ml) = (ts.iter().sum::<f64>() / 80.0, mean);
        let slope = ts.iter().zip(&ls).map(|(t, l)| (t - mt) * (l - ml)).sum::<f64>() / ts.iter().map(|t| (t - mt).powi(2)).sum::<f64>();
        println!("  tail 0.2..0.6 s: {mean:+.1} dB, {slope:+.1} dB/s");
        assert!((-60.0..=-50.0).contains(&mean), "tail at {mean:+.1} dB");
        assert!((-36.0..=-25.0).contains(&slope), "tail slope {slope:+.1} dB/s");
        // The glide, read the clav verifier's way: the band from seven
        // semitones under the note to two and a half over it (eighth-order
        // Butterworth, run forward and back so it moves nothing in time),
        // its analytic signal from the image's own 90° pair, and the
        // instantaneous frequency weighted by the envelope's square over
        // ±2 ms.
        let f0 = f64::from(f_of(60));
        let seg = &quiet[off - (0.3 * RATE) as usize..off + (0.06 * RATE) as usize];
        let z = (0.3 * RATE) as usize;
        let biquad = |x: &mut Vec<f64>, hz: f64, q: f64, high: bool| {
            let w0 = TAU * hz / f64::from(RATE);
            let (sn, c) = w0.sin_cos();
            let al = sn / (2.0 * q);
            let a0 = 1.0 + al;
            let (b0, b1) = if high { ((1.0 + c) * 0.5, -(1.0 + c)) } else { ((1.0 - c) * 0.5, 1.0 - c) };
            let (b0, b1, b2, a1, a2) = (b0 / a0, b1 / a0, b0 / a0, -2.0 * c / a0, (1.0 - al) / a0);
            let (mut s1, mut s2) = (0.0, 0.0);
            for v in x.iter_mut() {
                let o = b0 * *v + s1;
                s1 = b1 * *v - a1 * o + s2;
                s2 = b2 * *v - a2 * o;
                *v = o;
            }
        };
        let mut bp: Vec<f64> = seg.iter().map(|&v| f64::from(v)).collect();
        for pass in 0..2 {
            for q in [0.541_196_1, 1.306_563] {
                biquad(&mut bp, f0 * 2.0_f64.powf(-7.0 / 12.0), q, true);
                biquad(&mut bp, f0 * 2.0_f64.powf(2.5 / 12.0), q, false);
            }
            bp.reverse();
            let _ = pass;
        }
        let mut im = Image::default();
        im.design(RATE, f_of(60), 60, 1.0);
        let (mut a, mut b) = (im.a, im.b);
        let mut ph_prev = 0.0_f64;
        let mut inst = Vec::with_capacity(bp.len());
        let mut amp = Vec::with_capacity(bp.len());
        for (i, &v) in bp.iter().enumerate() {
            let (mut ya, mut yb) = (v as f32, v as f32);
            for s in &mut a {
                ya = s.tick(ya);
            }
            for s in &mut b {
                yb = s.tick(yb);
            }
            let ph = f64::from(yb).atan2(f64::from(ya));
            let mut d = ph - ph_prev;
            d -= TAU * (d / TAU).round();
            ph_prev = ph;
            inst.push(if i == 0 { 0.0 } else { d.abs() * f64::from(RATE) / TAU });
            amp.push(f64::from(ya).hypot(f64::from(yb)));
        }
        let at = |c: usize, h: usize| {
            let (mut num, mut den) = (0.0, 0.0);
            for j in c - h..c + h {
                num += inst[j] * amp[j] * amp[j];
                den += amp[j] * amp[j];
            }
            num / den
        };
        let rf = at(z - (0.055 * RATE) as usize, (0.045 * RATE) as usize);
        let g: Vec<f64> = [5.0_f32, 10.0, 15.0, 20.0]
            .iter()
            .map(|&ms| 1200.0 * (at(z + (ms * 1e-3 * RATE) as usize, (0.002 * RATE) as usize) / rf).log2())
            .collect();
        println!("  glide at +5/10/15/20 ms: {} c (the held pitch {:+.1} c)", g.iter().map(|v| format!("{v:+.0}")).collect::<Vec<_>>().join(" "), 1200.0 * (rf / f0).log2());
        assert!((-250.0..=-100.0).contains(&g[3]), "the glide reads {:+.0} c at +20 ms", g[3]);
        for w in g.windows(2) {
            assert!(w[1] <= w[0] + 15.0, "the glide turns back up: {g:?}");
        }
    }

    /// **The velocity law and the keyboard's balance.** C4's 300 ms RMS
    /// spans 23.4 ± 1 dB from v10 to v127 (the reference's, the critic's
    /// figure for this instrument), and at v80 every register from G1 to E6
    /// sits within 2 dB of C4 (the reference: within 1.1).
    #[test]
    fn the_velocity_law_and_the_keyboard_balance() {
        let k = knobs();
        let rms300 = |key: u8, vel: u8| rms_db(window(&strike(key, f32::from(vel) / 127.0, &k, ALL, 0.3), 0.0, 0.3));
        let span = rms300(60, 127) - rms300(60, 10);
        println!("  C4 v10 -> v127: {span:+.1} dB");
        assert!((span - 23.4).abs() <= 1.0, "C4 velocity span {span:.1} dB");
        let c4 = rms300(60, 80);
        let mut line = String::new();
        for key in [31u8, 36, 43, 48, 55, 60, 67, 72, 79, 84, 88] {
            let d = rms300(key, 80) - c4;
            line.push_str(&format!(" {key}:{d:+.1}"));
            assert!(d.abs() <= 2.0, "key {key} sits {d:+.1} dB from C4");
        }
        println!("  rms300 re C4 at v80:{line}");
    }

    /// **The top octave has no holes.** Above the reference's E6 every key
    /// from E6 to C8 at v80 sits within 2 dB of C4 and within 1 dB of its
    /// neighbour (the uncapped contact's nulls left C7 −7.4, D#7 −15.5,
    /// F#7 −22.6 and C8 −24.3 dB under C4, neighbours 4.3 dB apart).
    #[test]
    fn the_top_octave_has_no_holes() {
        let k = knobs();
        let rms300 = |key: u8| rms_db(window(&strike(key, 80.0 / 127.0, &k, ALL, 0.3), 0.0, 0.3));
        let c4 = rms300(60);
        let mut prev = rms300(87) - c4;
        let (mut lo, mut hi, mut step) = (f64::MAX, f64::MIN, 0.0_f64);
        for key in 88u8..=108 {
            let d = rms300(key) - c4;
            assert!(d.abs() <= 2.0, "key {key} sits {d:+.1} dB from C4");
            assert!((d - prev).abs() <= 1.0, "key {key} steps {:+.1} dB from its neighbour", d - prev);
            (lo, hi, step) = (lo.min(d), hi.max(d), step.max((d - prev).abs()));
            prev = d;
        }
        println!("  E6..C8 at v80: {lo:+.1}..{hi:+.1} dB re C4, neighbours at most {step:.1} dB apart");
    }

    /// **The treble keeps its headroom.** Single-note peaks through the
    /// block path, both channels, at v127: C6 at or under −3 dBFS (the old
    /// voice clipped there at +0.24, its pan law lifting the right channel
    /// 2.3 dB), and no key on the keyboard over −4 dBFS — the wah patch's
    /// pedal adds its 2..5 dB on top (`clav_wah`'s headroom test).
    #[test]
    fn the_treble_keeps_its_headroom() {
        let k = knobs();
        let mut worst = (0u8, f64::MIN);
        for key in 21u8..=108 {
            let (l, r) = strike_stereo(key, 1.0, &k, 0.4);
            let pk = db(f64::from(peak(&l).max(peak(&r))));
            if key == 84 {
                println!("  C6 v127 peak L {:+.2} R {:+.2} dBFS", db(f64::from(peak(&l))), db(f64::from(peak(&r))));
                assert!(pk <= -3.0, "C6 v127 peaks at {pk:+.2} dBFS");
            }
            if pk > worst.1 {
                worst = (key, pk);
            }
        }
        println!("  loudest single note: key {} at {:+.2} dBFS", worst.0, worst.1);
        assert!(worst.1 <= -4.0, "key {} peaks at {:+.2} dBFS", worst.0, worst.1);
    }

    /// The loudest sample of a chord struck in one block at MIDI 127,
    /// through the frame, `patch`, both channels, over 1.5 s: dBFS.
    fn chord_peak(patch: super::super::Patch, keys: &[u8]) -> f64 {
        let mut k = super::super::Keys::new(RATE);
        k.set_patch(patch);
        for &p in keys {
            k.note_on(p, 1.0);
        }
        let mut buf = vec![0.0_f32; 256];
        let mut pk = 0.0_f32;
        for _ in 0..((1.5 * RATE) as usize / 128) {
            buf.fill(0.0);
            k.render(&mut buf, 128, 2);
            pk = buf.iter().fold(pk, |m, &v| m.max(v.abs()));
        }
        db(f64::from(pk))
    }

    /// **A bass chord keeps its headroom.** Struck in one block at MIDI
    /// 127 — the frame quantises a hand's chord to that — through both
    /// patches. The reference's own five-note C2..G3 chord reaches its full
    /// scale at its native level (0.0 dBFS with its mezzo E4 at −13.0), and
    /// its three-note bass chords 11.0..11.5 dB over its E4; this voice's,
    /// before the spread, peaked 5.5 dB over that (+4.5 dBFS: every note's
    /// pulse on the same sample). Five notes at or under −1.5 dBFS, three
    /// at or under −3.
    #[test]
    fn a_bass_chord_keeps_its_headroom() {
        use super::super::Patch;
        let chords: [(&str, &[u8]); 5] = [
            ("C2-G2-C3-E3-G3", &[36, 43, 48, 52, 55]),
            ("F1-F2-F3", &[29, 41, 53]),
            ("E2-B2-E3", &[40, 47, 52]),
            ("G1-D2-G2", &[31, 38, 43]),
            ("C2-C3", &[36, 48]),
        ];
        let mut worst = Vec::new();
        for patch in [Patch::Clav, Patch::ClavWah] {
            let mut line = String::new();
            for (name, keys) in chords {
                let pk = chord_peak(patch, keys);
                line.push_str(&format!(" {name} {pk:+.1}"));
                let ceiling = if keys.len() >= 5 { -1.5 } else { -3.0 };
                worst.push((patch, name, pk, ceiling));
            }
            println!("  {patch:?} v127 chords, dBFS:{line}");
        }
        for (patch, name, pk, ceiling) in worst {
            assert!(pk <= ceiling, "{patch:?} {name} peaks at {pk:+.1} dBFS");
        }
    }

    /// **The anti-phase position keeps the reference's level.** C2 / C4 /
    /// C5 at v80, the 0.02..2 s RMS of the anti-phase position re the
    /// in-phase one within 1 dB of the reference's −1.6 / −1.7 / +0.8 (its
    /// release score; the analysis holds the two within 2 dB), and C3 / C6,
    /// between and past the anchors, within 2 dB of in-phase.
    #[test]
    fn the_anti_phase_keeps_the_references_level() {
        let rms = |key: u8, pickup: f32, secs: f32| {
            let mut k = knobs();
            k.pickup = pickup;
            rms_db(window(&strike(key, 80.0 / 127.0, &k, ALL, secs), 0.02, secs))
        };
        let mut line = String::new();
        for (key, secs, want) in [(36u8, 2.0_f32, Some(-1.6)), (48, 2.0, None), (60, 2.0, Some(-1.7)), (72, 1.0, Some(0.8)), (84, 1.0, None)] {
            let d = rms(key, 0.9, secs) - rms(key, 0.6, secs);
            line.push_str(&format!(" {key}:{d:+.1}"));
            match want {
                Some(w) => assert!((d - w).abs() <= 1.0, "key {key}: anti-phase {d:+.1} dB re in-phase, the reference {w:+.1}"),
                None => assert!(d.abs() <= 2.0, "key {key}: anti-phase {d:+.1} dB re in-phase"),
            }
        }
        println!("  anti-phase re in-phase, 0.02..2 s RMS:{line}");
    }

    /// **The spread keeps every level and takes the peak.** The voice's
    /// output stage is all-passes: C2 / C4 / C6's attack partials (5..205
    /// ms) through it within 0.3 dB of the same strike without it, and
    /// C2's crest (peak over the first 300 ms' RMS) at v127 brought from
    /// 17.1 dB to the reference's 11.5..12.8 (13 at most here).
    #[test]
    fn the_spread_keeps_every_level_and_takes_the_peak() {
        let k = knobs();
        for (key, vel, n) in [(36u8, 127u8, 10usize), (60, 70, 10), (84, 127, 4)] {
            let mut a = ClavVoice::new(RATE);
            a.strike_based(key, f32::from(vel) / 127.0, &k, &ClavBase::DRY);
            let mut b = ClavVoice::new(RATE);
            b.strike_based(key, f32::from(vel) / 127.0, &k, &ClavBase::DRY);
            let through: Vec<f32> = (0..(0.25 * RATE) as usize).map(|_| a.tick()).collect();
            let bare: Vec<f32> = (0..(0.25 * RATE) as usize).map(|_| b.tick_bare()).collect();
            let (_, pt) = partials(window(&through, 0.005, 0.205), f_of(key), n);
            let (_, pb) = partials(window(&bare, 0.005, 0.205), f_of(key), n);
            let worst = pt.iter().zip(&pb).map(|(x, y)| (x - y).abs()).fold(0.0, f64::max);
            let crest = |x: &[f32]| db(f64::from(peak(x))) - rms_db(window(x, 0.0, 0.25));
            println!("  key {key} v{vel}: partials moved at most {worst:.2} dB; crest {:.1} dB, without the spread {:.1}", crest(&through), crest(&bare));
            assert!(worst <= 0.3, "key {key}: the spread moved a partial {worst:.2} dB");
            if key == 36 {
                assert!(crest(&through) <= 13.0, "C2's crest {:.1} dB", crest(&through));
            }
        }
    }

    /// **SNAP at zero chokes at pitch, and the string still dies.** C2 and
    /// C4 at v100 held 1 s: the tail over 0.2..0.6 s after the off within
    /// 3 dB of SNAP 1's and falling at least 25 dB/s (its fundamental at
    /// the tail's 32, the upper partials faster), and the voice finished
    /// within 2.5 s of the off (the floor once rang on at the string's full
    /// t60: −50 dB at +2 s, the last sample 11.5 s after the off).
    #[test]
    fn snap_zero_chokes_at_pitch_and_the_string_still_dies() {
        for key in [36u8, 60] {
            let mut tails = Vec::new();
            for snap in [0.0_f32, 1.0] {
                let mut k = knobs();
                k.snap = snap;
                let mut v = ClavVoice::new(RATE);
                v.strike_based(key, 100.0 / 127.0, &k, &ClavBase::DRY);
                v.strip(Parts { noise: false, ..ALL });
                let off = RATE as usize;
                let mut x = Vec::new();
                let mut end = None;
                for i in 0..(4.0 * RATE) as usize {
                    if i == off {
                        v.release(false);
                    }
                    x.push(v.tick());
                    if i > off && end.is_none() && v.finished() {
                        end = Some((i - off) as f32 / RATE);
                    }
                }
                let pre = rms_db(&x[off - (0.06 * RATE) as usize..off - (0.01 * RATE) as usize]);
                let ts: Vec<f64> = (0..80).map(|i| 0.2 + 0.005 * f64::from(i)).collect();
                let ls: Vec<f64> = ts.iter().map(|&t| frame_db(&x, off, (t * 1000.0) as f32, pre)).collect();
                let mean = ls.iter().sum::<f64>() / 80.0;
                let mt = ts.iter().sum::<f64>() / 80.0;
                let slope = ts.iter().zip(&ls).map(|(t, l)| (t - mt) * (l - mean)).sum::<f64>() / ts.iter().map(|t| (t - mt).powi(2)).sum::<f64>();
                println!("  key {key} snap {snap}: tail {mean:+.1} dB at {slope:+.1} dB/s, finished {:.2} s after the off", end.unwrap_or(f32::NAN));
                assert!(slope <= -25.0, "key {key} snap {snap}: the tail falls {slope:+.1} dB/s");
                let end = end.unwrap_or(f32::MAX);
                assert!(end <= 2.5, "key {key} snap {snap}: the voice lives {end:.2} s past the off");
                tails.push(mean);
            }
            assert!((tails[0] - tails[1]).abs() <= 3.0, "key {key}: SNAP 0's tail {:+.1} against SNAP 1's {:+.1}", tails[0], tails[1]);
        }
    }

    /// **The strike arrives at once.** The coils read the travelling waves
    /// where they sit, so the first sound is the wave reaching the upper
    /// coil: at C2, v70, the first sample over −60 dB re the note's peak
    /// within 1.5 ms (the old reading of the loop's output: 12.3 ms).
    #[test]
    fn the_onset_arrives_at_once() {
        for key in [36u8, 60, 84] {
            let x = strike(key, 70.0 / 127.0, &knobs(), ALL, 0.5);
            let pk = peak(&x);
            let first = x.iter().position(|&v| v.abs() > pk * 1e-3).unwrap_or(x.len());
            let ms = first as f32 / RATE * 1000.0;
            println!("  key {key}: first sound at {ms:.2} ms");
            assert!(ms <= 1.5, "key {key}: the onset arrives at {ms:.2} ms");
        }
    }

    /// **A mono instrument, seated.** Per note at v80 over 0.05..1 s, the
    /// block path: the channels within 2 dB of each other, correlated
    /// 0.5..0.95, and their sum losing at most 2 dB against their average
    /// (the brief's numbers); WIDTH at zero is mono, sample for sample, and
    /// `tick` is it.
    #[test]
    fn the_image_is_seated_and_decorrelated() {
        let k = knobs();
        let mut line = String::new();
        for key in [29u8, 36, 43, 48, 55, 60, 67, 72, 79, 84, 88] {
            let (l, r) = strike_stereo(key, 80.0 / 127.0, &k, 1.0);
            let (a, b) = ((0.05 * RATE) as usize, l.len());
            let (l, r) = (&l[a..b], &r[a..b]);
            let el: f64 = l.iter().map(|&v| f64::from(v).powi(2)).sum();
            let er: f64 = r.iter().map(|&v| f64::from(v).powi(2)).sum();
            let lr: f64 = l.iter().zip(r).map(|(&x, &y)| f64::from(x) * f64::from(y)).sum();
            let em: f64 = l.iter().zip(r).map(|(&x, &y)| (0.5 * f64::from(x + y)).powi(2)).sum();
            let ldiff = 10.0 * (el / er).log10();
            let corr = lr / (el * er).sqrt();
            let mono = 10.0 * (em / (0.5 * (el + er))).log10();
            line.push_str(&format!(" {key}:{ldiff:+.1}/{corr:.2}/{mono:+.1}"));
            assert!(ldiff.abs() <= 2.0, "key {key}: L-R {ldiff:+.1} dB");
            assert!((0.5..=0.95).contains(&corr), "key {key}: correlation {corr:.2}");
            assert!(mono >= -2.0, "key {key}: mono loses {mono:.1} dB");
        }
        println!("  L-R dB / correlation / mono re channels:{line}");
        // Every partial the same in either channel as in their sum, but for
        // the flat cos θ: the sustain of C3 (where the old companion lifted
        // the fundamental 4.4 dB in each channel), C2 and C4, and C4 with the
        // TONE rockers down (the companion bypassed them: partials 12 / 16 /
        // 20 were 4.5..6.8 dB hotter in each channel than in mono).
        let flat = 20.0 * f64::from(MIC_ANGLE.cos()).log10();
        let mut worst = 0.0_f64;
        for (key, tone, ns) in [(48u8, 0.75_f32, &[1usize, 2, 3, 4, 5, 6][..]), (36, 0.75, &[1, 2, 3, 4, 5, 6][..]), (60, 0.75, &[1, 2, 3, 4, 5, 6][..]), (60, 0.0, &[1, 2, 4, 8, 12, 16, 20][..])] {
            let mut k = knobs();
            k.tone = tone;
            let (l, r) = strike_stereo(key, 80.0 / 127.0, &k, 2.5);
            let m: Vec<f32> = l.iter().zip(&r).map(|(a, b)| 0.5 * (a + b)).collect();
            let (wl, wr, wm) = (window(&l, 0.5, 2.5), window(&r, 0.5, 2.5), window(&m, 0.5, 2.5));
            let (sl, sr, sm) = (series(wl, key, 20), series(wr, key, 20), series(wm, key, 20));
            for &n in ns {
                let pa = 10.0 * (0.5 * (10.0_f64.powf(sl[n - 1] / 10.0) + 10.0_f64.powf(sr[n - 1] / 10.0))).log10();
                let d = pa - sm[n - 1] + flat;
                worst = worst.max(d.abs());
                assert!(d.abs() <= 0.5, "key {key} tone {tone} n{n}: a channel sits {d:+.2} dB off the mid");
            }
        }
        println!("  every partial in either channel within {worst:.2} dB of the mid's, beside the flat {:+.2} dB", -flat);
        let mut k0 = knobs();
        k0.width = 0.0;
        let (l, r) = strike_stereo(48, 0.7, &k0, 0.2);
        assert!(l.iter().zip(&r).all(|(a, b)| a == b), "WIDTH 0 is not mono");
        // And tick() is that mono, sample for sample.
        let mut v = ClavVoice::new(RATE);
        v.strike_based(48, 0.7, &k0, &ClavBase::DRY);
        let t: Vec<f32> = (0..l.len()).map(|_| v.tick()).collect();
        // To rounding: the block path re-takes the coils' flux once a block
        // and `tick` once a sample.
        let top = peak(&l);
        let off = t.iter().zip(&l).fold(0.0_f32, |m, (a, b)| m.max((a - b).abs()));
        assert!(off <= 1e-4 * top, "tick() is {off:.2e} off the WIDTH-0 output (peak {top:.3})");
    }

    /// Band energies of `x` (Hann, a brute-force DFT on the bin grid), dB:
    /// 50-150, 150-400, 400-1k, 1-3k, 3-8k Hz, the clav verifier's bands.
    fn bands(x: &[f32]) -> [f64; 5] {
        let edges = [(50.0, 150.0), (150.0, 400.0), (400.0, 1000.0), (1000.0, 3000.0), (3000.0, 8000.0)];
        let n = x.len();
        let df = f64::from(RATE) / n as f64;
        let mut out = [0.0; 5];
        for (b, &(lo, hi)) in edges.iter().enumerate() {
            let mut e = 0.0_f64;
            let mut f = lo;
            while f < hi {
                let w = TAU * f / f64::from(RATE);
                let (mut re, mut im) = (0.0_f64, 0.0_f64);
                for (i, &v) in x.iter().enumerate() {
                    let win = 0.5 - 0.5 * (TAU * i as f64 / n as f64).cos();
                    re += f64::from(v) * win * (w * i as f64).cos();
                    im -= f64::from(v) * win * (w * i as f64).sin();
                }
                e += re * re + im * im;
                f += df;
            }
            out[b] = 10.0 * e.max(1e-30).log10();
        }
        out
    }

    /// One strike with `seed` strikes already on the voice (the knock's
    /// seed), its noise on or off, `held` then released (`None`: never),
    /// mono, `secs` long.
    fn strike_seeded(vel: f32, seed: u32, noise: bool, held: Option<f32>, secs: f32) -> Vec<f32> {
        let mut v = ClavVoice::new(RATE);
        v.strikes = seed;
        v.strike_based(60, vel, &knobs(), &ClavBase::DRY);
        v.strip(Parts { noise, ..ALL });
        let off = held.map_or(usize::MAX, |h| (h * RATE) as usize);
        (0..(secs * RATE) as usize)
            .map(|i| {
                if i == off {
                    v.release(false);
                }
                v.tick()
            })
            .collect()
    }

    /// **The knock and the thock**, by difference against the same strike
    /// with the noise off (the voice is deterministic), band energies in
    /// power averaged over 32 strikes (the knock's) and eight (the thock's)
    /// — that many seeds, because one strike's 100 ms of noise scatters its
    /// lowest band by ±3 dB and eight still by ±1. The knock (the
    /// reference's hammer noise): its 0..30 ms RMS 11.5..15.5 dB under the
    /// note's (the reference: 11.7 / 13.7 / 14.7 at v10 / v40 / v127), its
    /// bands from 50 Hz to 3 kHz within 2.5 dB of the strongest at every
    /// velocity, its 3..8 kHz band 26 ± 3 dB under at v10 and 10 ± 3 at
    /// v127. The thock: C4 held 0.12 / 0.5 / 2 s, its 200 ms energy 25..30
    /// dB under the 200 ms before the off at every hold (the reference
    /// −23.5 / −28.5 / −24.9), 150..400 Hz its strongest band, 400..1k 3..7
    /// dB under it and 3..8 kHz at least 20; and above 8 kHz, in 2 ms
    /// frames over the 30 ms after the off, the thock alone 55 dB under the
    /// note (5 dB under what the release leaves without it; the old thock
    /// ticked there at −37.5).
    #[test]
    fn the_knock_and_the_thock() {
        let mut hf = Vec::new();
        let mut levels = Vec::new();
        for vel in [10u8, 40, 127] {
            let v = f32::from(vel) / 127.0;
            let i30 = (0.03 * RATE) as usize;
            let (mut e_n, mut e_w) = (0.0_f64, 0.0_f64);
            let mut b_sum = [0.0_f64; 5];
            for seed in 0..32u32 {
                let with = strike_seeded(v, seed * 13, true, None, 0.12);
                let without = strike_seeded(v, seed * 13, false, None, 0.12);
                let n: Vec<f32> = with.iter().zip(&without).map(|(a, b)| a - b).collect();
                e_n += n[..i30].iter().map(|&x| f64::from(x).powi(2)).sum::<f64>();
                e_w += with[..i30].iter().map(|&x| f64::from(x).powi(2)).sum::<f64>();
                for (s, b) in b_sum.iter_mut().zip(bands(&n[..(0.1 * RATE) as usize])) {
                    *s += 10.0_f64.powf(b / 10.0);
                }
            }
            let re = 10.0 * (e_n / e_w).log10();
            let b = b_sum.map(|x| 10.0 * x.log10());
            let top = b.iter().fold(f64::MIN, |m, &x| m.max(x));
            println!("  knock v{vel}: {re:+.1} dB re the note's first 30 ms; bands re max {}", b.iter().map(|x| format!("{:+.1}", x - top)).collect::<Vec<_>>().join(" "));
            levels.push((vel, re));
            for (i, &x) in b[..4].iter().enumerate() {
                assert!(x - top >= -2.5, "v{vel}: the knock's band {i} sits {:+.1} dB under its strongest", x - top);
            }
            hf.push(b[4] - top);
        }
        for (vel, re) in levels {
            assert!((-15.5..=-11.5).contains(&re), "v{vel}: the knock sits {re:+.1} dB re the note");
        }
        assert!((hf[0] + 26.0).abs() <= 3.0, "the knock's 3..8 kHz at v10: {:+.1}", hf[0]);
        assert!((hf[2] + 10.0).abs() <= 3.0, "the knock's 3..8 kHz at v127: {:+.1}", hf[2]);
        let e = |x: &[f32]| 10.0 * x.iter().map(|&v| f64::from(v).powi(2)).sum::<f64>().max(1e-30).log10();
        // A fourth-order high-pass at 8 kHz, two cookbook biquads.
        let hp8 = |x: &[f32]| {
            let mut y: Vec<f64> = x.iter().map(|&v| f64::from(v)).collect();
            for q in [0.541_196_1_f64, 1.306_563] {
                let w0 = TAU * 8000.0 / f64::from(RATE);
                let (sn, c) = w0.sin_cos();
                let al = sn / (2.0 * q);
                let a0 = 1.0 + al;
                let (b0, b1, b2, a1, a2) = ((1.0 + c) * 0.5 / a0, -(1.0 + c) / a0, (1.0 + c) * 0.5 / a0, -2.0 * c / a0, (1.0 - al) / a0);
                let (mut s1, mut s2) = (0.0, 0.0);
                for v in &mut y {
                    let o = b0 * *v + s1;
                    s1 = b1 * *v - a1 * o + s2;
                    s2 = b2 * *v - a2 * o;
                    *v = o;
                }
            }
            y
        };
        for held in [0.12_f32, 0.5, 2.0] {
            let off = (held * RATE) as usize;
            let (mut e_t, mut e_p) = (0.0_f64, 0.0_f64);
            let mut b_sum = [0.0_f64; 5];
            let mut tick = f64::MIN;
            for seed in 0..8u32 {
                let with = strike_seeded(80.0 / 127.0, seed * 13, true, Some(held), held + 0.25);
                let mut v = ClavVoice::new(RATE);
                v.strikes = seed * 13;
                v.strike_based(60, 80.0 / 127.0, &knobs(), &ClavBase::DRY);
                v.quiet = true;
                let without: Vec<f32> = (0..with.len())
                    .map(|i| {
                        if i == off {
                            v.release(false);
                        }
                        v.tick()
                    })
                    .collect();
                let n: Vec<f32> = with[off..].iter().zip(&without[off..]).map(|(a, b)| a - b).collect();
                // Energies per sample: the thock's over 200 ms, the note's
                // over the 200 ms before the off or as much of it as the
                // hold has.
                let pre = (0.2 * RATE).min(off as f32 - 0.005 * RATE) as usize;
                e_t += 10.0_f64.powf(e(&n[..(0.2 * RATE) as usize]) / 10.0) / (0.2 * f64::from(RATE));
                e_p += 10.0_f64.powf(e(&with[off - pre..off]) / 10.0) / pre as f64;
                for (s, b) in b_sum.iter_mut().zip(bands(&n[..(0.2 * RATE) as usize])) {
                    *s += 10.0_f64.powf(b / 10.0);
                }
                // Above 8 kHz: the thock's loudest 2 ms frame in the 30 ms
                // after the off, re the note's broadband power over the 50 ms
                // before it (the clav verifier's measure, on the whole
                // output: the old thock −37.5 dB, the release without it
                // −50, the old voice −67).
                let thock: Vec<f32> = with.iter().zip(&without).map(|(a, b)| a - b).collect();
                let hw = hp8(&thock);
                let fr = (0.002 * RATE) as usize;
                let pre50: f64 = with[off - (0.05 * RATE) as usize..off].iter().map(|&v| f64::from(v).powi(2)).sum::<f64>() / (0.05 * f64::from(RATE));
                let mut a = off;
                while a + fr <= off + (0.03 * RATE) as usize {
                    let ew: f64 = hw[a..a + fr].iter().map(|v| v * v).sum::<f64>() / fr as f64;
                    tick = tick.max(10.0 * (ew / pre50).log10());
                    a += fr;
                }
            }
            let re = 10.0 * (e_t / e_p).log10();
            let b = b_sum.map(|x| 10.0 * x.log10());
            let top = (0..5).max_by(|&i, &j| b[i].total_cmp(&b[j])).unwrap_or(0);
            println!(
                "  thock, held {held} s: {re:+.1} dB re pre-off; bands {}; above 8 kHz {tick:+.1} dB re the note",
                b.iter().map(|x| format!("{:+.1}", x - b[top])).collect::<Vec<_>>().join(" ")
            );
            assert!((-30.0..=-25.0).contains(&re), "held {held}: the thock sits {re:+.1} dB re the pre-off energy");
            assert_eq!(top, 1, "held {held}: the thock's strongest band is not 150..400 Hz");
            assert!((-7.0..=-3.0).contains(&(b[2] - b[1])), "held {held}: the thock's 400..1k at {:+.1}", b[2] - b[1]);
            assert!(b[4] - b[1] <= -20.0, "held {held}: the thock's 3..8 kHz at {:+.1}", b[4] - b[1]);
            assert!(tick <= -55.0, "held {held}: the key-up ticks {tick:+.1} dB above 8 kHz");
        }
    }

    /// **Eight pedalled strikes sixty milliseconds apart** — the frame's
    /// scenario (`mod.rs`, `eight_pedalled_repeats_neither_stack_nor_leave_a_hole`)
    /// on a bare voice: the peak against a fresh strike's within the
    /// frame's 4 dB margin, and the quietest 4 ms in the 12 after each
    /// restrike no more than 8 dB under the 4 before it. Without the tip's
    /// catch the string alone stacked +4.1 dB here; the measured figure is
    /// [`STACK_MEASURED`].
    #[test]
    fn eight_repeats_stack_within_the_frame_margin() {
        let k = knobs();
        let block = 128usize;
        let at = |secs: f32| (secs * RATE) as usize / block * block;
        let mut v = ClavVoice::new(RATE);
        let mut x = Vec::new();
        let mut fresh = 0.0_f32;
        let onsets: Vec<usize> = (0..8).map(|s| at(s as f32 * 0.060)).collect();
        let offs: Vec<usize> = (0..8).map(|s| at(s as f32 * 0.060 + 0.030)).collect();
        for n in 0..at(0.52) {
            if onsets.contains(&n) {
                v.strike_based(60, 0.8, &k, &ClavBase::DRY);
            }
            if offs.contains(&n) {
                v.release(true);
            }
            let o = v.tick();
            if n < onsets[1] {
                fresh = fresh.max(o.abs());
            }
            x.push(o);
        }
        let stack = peak(&x);
        let ratio = 20.0 * f64::from(stack / fresh).log10();
        let ms = |m: f32| (m * 1e-3 * RATE) as usize;
        let mut deepest = 0.0_f64;
        for &o in &onsets[1..] {
            let before = rms_db(&x[o - ms(4.0)..o]);
            let mut worst = f64::MAX;
            let mut a = o;
            while a + ms(4.0) <= o + ms(12.0) {
                worst = worst.min(rms_db(&x[a..a + ms(4.0)]));
                a += ms(1.0);
            }
            deepest = deepest.min(worst - before);
        }
        println!("  stacked {ratio:+.2} dB over a fresh strike; deepest dip {deepest:+.1} dB");
        assert!((ratio - STACK_MEASURED).abs() < 0.3, "stacked {ratio:+.2} dB, pinned {STACK_MEASURED}");
        assert!(ratio <= 4.0, "eight strikes stack {ratio:+.2} dB");
        assert!(deepest >= -8.0, "a restrike leaves a {deepest:.1} dB hole");
    }

    /// The rejoin's first millisecond has no step larger than the ringing
    /// signal's own slope: the largest one-sample difference in the 1 ms
    /// after the lift against the largest over two periods before it, at
    /// three pitches and both ends of SNAP, the noise off (the thock is a
    /// burst, not a step). A lengthening line plays its history slower, so
    /// an honest rejoin can only flatten the slope.
    #[test]
    fn the_release_rejoins_without_a_step() {
        for pitch in [36u8, 60, 84] {
            for snap in [1.0_f32, 2.0] {
                let mut k = knobs();
                k.snap = snap;
                let mut v = ClavVoice::new(RATE);
                v.strike_based(pitch, 0.8, &k, &ClavBase::DRY);
                v.strip(Parts { noise: false, ..ALL });
                let hold = (0.3 * RATE) as usize;
                let mut out: Vec<f32> = (0..hold).map(|_| v.tick()).collect();
                v.release(false);
                assert!(v.rejoining, "SNAP {snap} did not splice the far length in");
                out.extend((0..(0.040 * RATE) as usize).map(|_| v.tick()));
                let d1 = |a: usize, b: usize| (a.max(1)..b).fold(0.0_f32, |m, n| m.max((out[n] - out[n - 1]).abs()));
                let span = ((2.0 * RATE / f_of(pitch)) as usize).max((0.008 * RATE) as usize);
                let before = d1(hold - span, hold);
                let after = d1(hold, hold + (0.001 * RATE) as usize);
                assert!(before > 1e-4, "key {pitch}: nothing was ringing to lift off");
                assert!(after <= before, "key {pitch} snap {snap}: the release stepped {after:.5} against {before:.5}");
            }
        }
    }

    /// The drop is the paper's three semitones at SNAP centre, none at
    /// SNAP zero, six at the top: the tail's fundamental 40..90 ms after the
    /// lift (the rejoin done), RELEASE slowed so there is a tail, in cents
    /// from the held note's.
    #[test]
    fn the_release_drops_three_semitones_at_snap_centre() {
        let f0 = f_of(60);
        for (snap, want) in [(0.0_f32, 0.0_f64), (1.0, -300.0), (2.0, -600.0)] {
            let mut k = knobs();
            k.snap = snap;
            k.release = 3.0;
            let x = strike_release(60, 0.8, &k, Parts { noise: false, ..ALL }, 0.3, 0.4);
            let w = window(&x, 0.34, 0.39);
            let mut best = (0.0_f64, 0.0_f64);
            let mut f = f64::from(f0) * 0.6;
            while f < f64::from(f0) * 1.1 {
                let m = bh_at(w, f);
                if m > best.1 {
                    best = (f, m);
                }
                f *= 1.001;
            }
            let cents = 1200.0 * (best.0 / f64::from(f0)).log2();
            println!("  snap {snap}: the tail at {cents:+.0} c");
            assert!((cents - want).abs() < 30.0, "snap {snap}: the tail sits {cents:+.0} c, wanted {want:+.0}");
        }
    }

    /// A string in its rejoin refuses to land — the frame fades it and the
    /// new note starts fresh — while a string choked at pitch (SNAP zero)
    /// lands as it always did.
    #[test]
    fn a_strike_does_not_land_on_a_rejoining_string() {
        for (snap, want) in [(1.0_f32, false), (0.0, true)] {
            let mut k = knobs();
            k.snap = snap;
            let mut v = ClavVoice::new(RATE);
            v.strike_based(60, 0.8, &k, &ClavBase::DRY);
            for _ in 0..(0.2 * RATE) as usize {
                v.tick();
            }
            assert!(v.lands(60, &k), "a held note does not land on itself");
            v.release(false);
            for _ in 0..(0.002 * RATE) as usize {
                v.tick();
            }
            assert_eq!(v.lands(60, &k), want, "snap {snap}: lands() during the release");
        }
    }

    /// Peak output rises monotonically with velocity, on both bases, over
    /// twelve strikes from MIDI 8 to 127.
    #[test]
    fn peak_output_rises_monotonically_with_velocity() {
        for base in [&ClavBase::DRY, &ClavBase::WAH] {
            let mut prev = 0.0_f32;
            for i in 0..12 {
                let midi = (8 + i * 11).min(127);
                let mut v = ClavVoice::new(RATE);
                v.strike_based(64, midi as f32 / 127.0, &knobs(), base);
                let pk = (0..(0.25 * RATE) as usize).map(|_| v.tick().abs()).fold(0.0_f32, f32::max);
                assert!(pk > prev, "peak fell to {pk} at MIDI {midi} from {prev}");
                prev = pk;
            }
        }
    }

    /// `strike_knobs` is `strike_based` through the dry base, sample for
    /// sample, so mod.rs's untouched dispatch is the dry instrument.
    #[test]
    fn strike_knobs_is_the_dry_base() {
        let mut a = ClavVoice::new(RATE);
        let mut b = ClavVoice::new(RATE);
        a.strike_knobs(60, 0.7, &knobs());
        b.strike_based(60, 0.7, &knobs(), &ClavBase::DRY);
        for _ in 0..4800 {
            assert_eq!(a.tick(), b.tick());
        }
    }

    /// **Every knob at its default is the tuned instrument.** DECAY,
    /// RELEASE and SNAP land on 1 (the law, the measured choke, the paper's
    /// three semitones); VOICING on a tip of exactly one; TONE on the 8010
    /// Hz corner; PICKUP's 0.6 on the in-phase pair, the reference's
    /// preset, with the four positions where a saved value always had
    /// them; WIDTH on the stock seating; MUTE lifted.
    #[test]
    fn every_knob_at_its_default_is_the_tuned_constant() {
        let k = knobs();
        assert_eq!(k.decay, 1.0);
        assert_eq!(k.release, 1.0);
        assert_eq!(k.snap, 1.0);
        assert_eq!(TIP_TRAVEL.powf(1.0 - 2.0 * k.voicing), 1.0);
        assert!((8_010.0 * 2.0_f32.powf((k.tone - 0.75) * 5.0) - 8_010.0).abs() < 1e-3);
        let g = 10.0_f32.powf(IN_PHASE_DB / 20.0);
        assert_eq!(k.pickup, 0.6);
        let (lo, hi) = rocker(k.pickup, 60.0);
        assert!((lo - g).abs() < 1e-6 && (hi - g * IN_PHASE_UPPER * 1.11).abs() < 1e-6, "PICKUP's default is not the in-phase pair");
        // The positions a saved value has always meant: the lower (once
        // the centre) coil, the upper (once the bridge) coil, both, anti.
        assert_eq!(rocker(0.1, 60.0), (1.0, 0.0));
        assert_eq!(rocker(0.35, 60.0).0, 0.0);
        assert!(rocker(0.9, 60.0).1 < 0.0);
        assert_eq!(k.width, 1.0);
        assert_eq!(k.mute, 0.0);
    }
}
