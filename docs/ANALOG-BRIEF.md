# Tangent Analog — the brief

Written 2026-09-04 against `main` at `98ea626` ("The tempo draws in the
master's lane…", 5.0.0-dev). This is the contract for building the fourth
built-in instrument. It is written for whoever builds it, and it says *why*
as often as *what*, because half of the decisions below were made once
already, elsewhere in this repo, and re-deriving them is how they get made
differently the second time.

---

## 0. What it is, in one paragraph

**Tangent Analog** is a subtractive synthesiser with three oscillators, two
filter characters, and a voice-mode switch that takes it from a single
gliding monophonic line to eight or sixteen drifting polyphonic voices. It is
modelled on two machines and pretends to be neither: the 1970 monosynth for
the mono side (three oscillators into a four-pole transistor ladder, glide,
oscillator three doing duty as the modulation source) and the 1980
eight-voice for the poly side (a two-pole state-variable filter with no bass
loss, per-voice component tolerance, an ensemble chorus behind it). One
panel, one preset format, one set of knobs. It contains no samples.

It is also the first built-in **voice**. That sentence is the largest thing
in this document and §7 is about nothing else.

---

## 1. The four decisions already made

The owner settled these on 2026-09-04. They are not open.

1. **One panel with a VOICE MODE switch**, not two patch families. A single
   architecture with `MONO / UNISON / POLY` changing voice allocation, glide
   and detune stacking. A preset can be either; the Minimoog patch and the
   OB-Xa patch are the same instrument voiced differently.
2. **Two filters, switchable**: a nonlinear four-pole ladder and a two-pole
   state-variable in the Oberheim manner. That switch *is* the difference
   between the two machines, and it is an enum in the voice — far cheaper
   than the panel work either way.
3. **The built-in voice road** (§7), not the mask road the DX7, the drumpad
   and Keys ride. Every knob automatable, presets and state for free.
4. **v1 extras: ensemble chorus, output overdrive/feedback, arpeggiator with
   hold.** A second LFO and a modulation matrix are explicitly OUT — the
   modulation is the Minimoog's own arrangement (oscillator three or noise,
   mixed, to pitch or to cutoff, scaled by the wheel) plus one dedicated LFO
   for vibrato, which a polysynth cannot do without.

---

## 2. The machine

**Oscillator bank.** Three oscillators. Each has a six-position range switch
(LO, 32′, 16′, 8′, 4′, 2′) and a six-position waveform switch (triangle,
triangle-saw, saw, square, wide pulse, narrow pulse). Oscillators two and
three have a ±7-semitone frequency knob with a centre detent; oscillator one
has none, because the master Tune knob is its tune and a second control for
the same quantity is a way to get lost. Oscillator three has a **keyboard
tracking switch**: switched off with the range on LO it stops being an
oscillator and becomes the modulation source, which is the whole trick the
mono machine is built around. Oscillator two can hard-sync oscillator one
(one switch, and it unlocks a class of leads neither reference machine's
basic panel had — the owner has not asked for it; it is cheap and it is
recommended; cut it if the panel gets tight).

**Mixer.** A level for each oscillator, a level for noise with a white/pink
switch, and **Drive**: the output patched back into the external input,
overloading the mixer stage ahead of the filter. That patch cable is most of
what "loud" meant on the original and it is a gain stage with the right
nonlinearity in the right place.

**Modifiers.** The filter, with its character switch:

- **Ladder 24** — four one-pole sections with a saturating nonlinearity and
  a feedback path. Resonance thins the bass (this is a feature and is NOT
  compensated), self-oscillates cleanly at the top of the Emphasis knob, and
  distorts *inside the loop* when driven, which is why the Drive knob lives
  in the mixer and not after the filter.
- **SEM 12** — a two-pole zero-delay-feedback state variable with no bass
  loss and a continuous **Mode** knob running lowpass → notch → highpass.
  Its resonance law is its own; it does not self-oscillate the ladder's way.

Two full ADSR envelopes, one to the filter through a **Contour** amount and a
**Keyboard** tracking knob, one to the amplifier. Velocity to amplitude and
velocity to cutoff, as separate amounts, because a synth that cannot be
played quietly is a synth nobody plays twice.

**Voice.** `MONO` (last/low/high note priority, glide, single-trigger
legato), `UNISON` (all voices on one note, detuned and spread), `POLY`
(1–16 voices, round-robin, per-voice drift). **Drift** is one knob and it
scales the whole analogue-tolerance model in §5.

**Modulation.** One LFO (rate, wave, delay) and oscillator three, mixed by
the **Modulation Mix** knob against noise, routed by two switches — to
oscillator pitch, to filter cutoff — and scaled by the mod wheel with a depth
ceiling. Channel pressure has its own small amount knob. That is the entire
modulation section and it is deliberately not a matrix.

**Ensemble.** Off / I / II / III, plus a Width knob. Three BBD-style delay
lines with independent slow LFOs, the high-frequency loss the real ones had,
and enough difference between the three settings to be worth having three.

**Arpeggiator.** Off / Up / Down / Up-Down / Random / As-Played, host-tempo
synced, one to four octaves, a gate-length knob and a **Hold** latch. The
latch is a parameter as well as a button, so it automates.

**Output.** Volume, and a stereo spread for how far the voices are thrown
across the field independently of the ensemble's width.

---

## 3. The parameter table — this is the contract

Normalised `0.0..=1.0` end to end, the same currency `builtin_fx.rs` and the
automation lanes already speak; the mapping to what the DSP wants lives in a
const beside the DSP, per the house rule in `effects.rs`. Design arithmetic
in `f64`, the audio path in `f32`.

**Ids and symbols are append-only forever.** A curve in somebody's project
says `fx:<bay>:<id>`; a preset says the symbol. Ids are laid out in blocks
with room to append inside each, which is a courtesy to the next wave and not
a promise that dense numbering matters.

| id | symbol | name | units | steps | default | notes |
|---|---|---|---|---|---|---|
| 0 | `tune` | Tune | cents | 0 | 0.5 | ±100 cents, centre detent |
| 1 | `glide` | Glide | ms | 0 | 0.0 | 0–2000 ms, logarithmic; 0 is off |
| 2 | `mod_mix` | Mod Mix | | 0 | 0.0 | oscillator 3 ↔ noise |
| 3 | `mod_depth` | Mod Depth | % | 0 | 0.30 | what the wheel reaches at full |
| 4 | `bend_range` | Bend Range | semi | 13 | 2/12 | 0–12 semitones |
| 5 | `pressure` | Pressure | % | 0 | 0.0 | channel pressure to cutoff + vibrato |
| 10 | `o1_range` | Osc 1 Range | | 6 | 0.6 | LO, 32′, 16′, **8′**, 4′, 2′ |
| 11 | `o1_wave` | Osc 1 Wave | | 6 | 0.4 | tri, tri-saw, **saw**, sq, wide, narrow |
| 12 | `o2_range` | Osc 2 Range | | 6 | 0.6 | |
| 13 | `o2_tune` | Osc 2 Tune | semi | 0 | 0.5 | ±7 semitones, centre detent |
| 14 | `o2_wave` | Osc 2 Wave | | 6 | 0.4 | |
| 15 | `o3_range` | Osc 3 Range | | 6 | 0.6 | |
| 16 | `o3_tune` | Osc 3 Tune | semi | 0 | 0.5 | ±7 semitones |
| 17 | `o3_wave` | Osc 3 Wave | | 6 | 0.0 | triangle |
| 18 | `o3_track` | Osc 3 Keyboard | | 2 | 1.0 | off = free-running modulation source |
| 19 | `sync` | Sync | | 2 | 0.0 | osc 2 hard-syncs osc 1 |
| 20 | `pwm` | PWM | % | 0 | 0.0 | LFO to pulse width; bites on the three pulse positions only |
| 21 | `osc_drift` | Osc Drift | % | 0 | 0.20 | per-oscillator wander, §5 |
| 22 | `mix_o1` | Osc 1 | | 0 | 0.75 | |
| 23 | `mix_o2` | Osc 2 | | 0 | 0.0 | |
| 24 | `mix_o3` | Osc 3 | | 0 | 0.0 | |
| 25 | `mix_noise` | Noise | | 0 | 0.0 | |
| 26 | `noise_colour` | Noise Colour | | 2 | 0.0 | white / pink |
| 27 | `drive` | Drive | | 0 | 0.0 | the feedback patch into the mixer |
| 30 | `filter_model` | Filter | | 2 | 0.0 | **Ladder 24** / SEM 12 |
| 31 | `cutoff` | Cutoff | Hz | 0 | 0.80 | 20 Hz – 20 kHz, logarithmic |
| 32 | `emphasis` | Emphasis | | 0 | 0.10 | ladder self-oscillates near 1.0 |
| 33 | `sem_mode` | Mode | | 0 | 0.0 | LP → notch → HP; SEM only, §6 |
| 34 | `contour` | Contour | % | 0 | 0.35 | filter envelope amount |
| 35 | `keytrack` | Keyboard | % | 0 | 0.33 | cutoff follows pitch |
| 36 | `f_attack` | Filter Attack | ms | 0 | 0.0 | |
| 37 | `f_decay` | Filter Decay | ms | 0 | 0.35 | |
| 38 | `f_sustain` | Filter Sustain | | 0 | 0.40 | |
| 39 | `f_release` | Filter Release | ms | 0 | 0.25 | |
| 40 | `a_attack` | Attack | ms | 0 | 0.0 | |
| 41 | `a_decay` | Decay | ms | 0 | 0.40 | |
| 42 | `a_sustain` | Sustain | | 0 | 0.80 | |
| 43 | `a_release` | Release | ms | 0 | 0.20 | |
| 44 | `vel_amp` | Velocity | % | 0 | 0.60 | |
| 45 | `vel_filter` | Vel → Filter | % | 0 | 0.30 | |
| 50 | `voice_mode` | Voice | | 3 | 1.0 | mono / unison / **poly** |
| 51 | `voices` | Voices | | 16 | 7/15 | 1–16; **8** |
| 52 | `detune` | Detune | cents | 0 | 0.20 | unison spread in cents |
| 53 | `unison_spread` | Spread | % | 0 | 0.50 | unison across the field |
| 54 | `drift` | Drift | % | 0 | 0.25 | the analogue-tolerance master, §5 |
| 55 | `priority` | Priority | | 3 | 0.5 | low / **last** / high |
| 56 | `trigger` | Trigger | | 2 | 0.0 | **legato** (single) / retrigger |
| 60 | `lfo_rate` | LFO Rate | Hz | 0 | 0.45 | 0.05–30 Hz, logarithmic |
| 61 | `lfo_wave` | LFO Wave | | 5 | 0.0 | tri, sine, square, saw, S&H |
| 62 | `lfo_delay` | LFO Delay | s | 0 | 0.0 | 0–3 s fade-in |
| 63 | `osc_mod` | Osc Mod | | 2 | 0.0 | modulation to pitch |
| 64 | `filter_mod` | Filter Mod | | 2 | 0.0 | modulation to cutoff |
| 70 | `ensemble` | Ensemble | | 4 | 0.0 | **off** / I / II / III |
| 71 | `ens_width` | Width | % | 0 | 0.60 | |
| 72 | `spread` | Spread | % | 0 | 0.30 | voices across the field |
| 73 | `volume` | Volume | | 0 | 0.70 | |
| 80 | `arp_mode` | Arp | | 6 | 0.0 | **off** / up / down / up-down / random / as played |
| 81 | `arp_rate` | Rate | | 7 | 2/6 | 1/4, 1/4T, **1/8**, 1/8T, 1/16, 1/16T, 1/32 |
| 82 | `arp_oct` | Octaves | | 4 | 0.0 | 1–4 |
| 83 | `arp_gate` | Gate | % | 0 | 0.50 | |
| 84 | `arp_hold` | Hold | | 2 | 0.0 | |

**Every default is the voicing bench's own number**, and a test asserts it —
the Keys law, and it is there because a knob whose middle multiplies by 1.02
is a silent retune that nobody catches by ear. A freshly inserted Analog is
one saw oscillator, eight voices, a soft filter with a little contour on it:
the thing a person plays first, and it must sound finished.

---

## 4. The DSP — the parts that must not be invented

**Oscillators.** Anti-aliased, and the method is named here so it is not
re-chosen: polyBLEP on the discontinuity for saw and pulse, polyBLAMP on the
slope break for triangle and triangle-saw. Free-running phase: a voice does
**not** reset oscillator phase on note-on. That is what the hardware did, it
is a real part of why two voices playing the same note do not sound like one
voice twice as loud, and it costs nothing.

**Sync.** Hard sync resets oscillator one's phase from oscillator two's
wrap. The reset is itself a discontinuity and needs its own BLEP correction
or it aliases worse than the un-synced waveform.

**Filters.** Both zero-delay-feedback, both solved rather than approximated.

- Ladder: four one-pole sections, `tanh` in the feedback path, resonance
  uncompensated. Run the filter *and the drive stage ahead of it* at 4×.
  **Reuse `units/drive.rs`** — it already has the ADAA `ln cosh`
  antiderivative, the 23-tap halfband and the 63-tap shaded sinc for exactly
  this, and a second copy of an oversampler in this repo is a second thing to
  fix.
- SEM: two-pole ZDF state variable, LP/BP/HP outputs mixed continuously by
  `sem_mode` (lowpass at 0, notch at 0.5 as LP+HP, highpass at 1). Chamberlin
  is not acceptable here; it goes unstable exactly where a resonant sweep
  lives.

**Envelopes.** Analogue-shaped, not linear: an RC charging toward a target
*beyond* the rail, so the attack curve is the real one and the decay is a
true exponential. Attack 1 ms to 10 s, decay and release 2 ms to 15 s.

**Restrike is superposition, never a reset.** This is the law from
`docs/`-adjacent history and from the keys work: a same-pitch restrike on a
sounding voice must **attack from the current envelope level and leave the
oscillators running**. Zeroing a ringing state mid-waveform *is* the pop, and
the pedal makes it loudest because the voice is always still loud. A steal
cannot superpose — the pitch changes — so the dying voice moves to a
pre-allocated corpse slot and fades over ~8 ms. The pool must be sized for
`voices` plus corpses.

**Voice allocation.** Round-robin over free voices, then oldest-quietest for
the steal. Mono priority is a real switch (low is the 1970 machine's
behaviour and the reason its basslines sit where they do). Single-trigger
legato does not restart the envelopes and does glide; retrigger does both.

**Arpeggiator.** Driven by the transport (§7's `Transport`), latched by
Hold. When the transport is stopped it free-runs at the last known tempo,
because an arpeggiator that goes silent when the song stops cannot be
auditioned.

**Ensemble.** Three delay lines, 5–30 ms, independent LFOs at incommensurate
rates, a one-pole loss around 6 kHz on the delayed paths, and the three
settings differing in rate/depth/how many lines are live — not merely in
depth.

**Output.** A gentle asymmetric saturation, a DC blocker after it, and a
level. Nothing else: the suite has a tape unit and a comp and they are one
bay away.

---

## 5. What "warm" actually means here

Warmth is not a filter setting; it is five specific inexactnesses, and each
one is a line of code with a number on it. Ship all five, all scaled by
`drift` except where noted:

1. **Per-voice component tolerance**, drawn once at construction from a
   fixed seed per voice index: cutoff offset ±3 %, envelope times ±5 %,
   output level ±0.4 dB, oscillator tune ±2 cents. This is most of what
   separates an eight-voice board from eight copies of one voice.
2. **Slow oscillator wander** (`osc_drift`): a bounded random walk, ±3 cents,
   filtered to about 0.1 Hz, independent per oscillator per voice.
3. **Free-running phase** (§4).
4. **The ladder's bass loss**, uncompensated.
5. **The drive path**, which is where a chord stops being a sum and starts
   being a signal.

**The seeds are fixed constants, not `random()`.** A bounce must sound like
the playback, twice in a row, on every machine — and a test that renders the
same notes twice and compares bit for bit is the cheapest guard this module
can have.

---

## 6. The panel

The owner's words: *"a preset selector, save buttons, and an ACTUAL UI unlike
the other plugins — because this one is special. Knobs must be after
minimoog."*

**Reuse, don't rebuild.** `faces::preset_strip` already draws the selector
and the save buttons on every suite face; the Analog face gets the same
strip, in the same place, for the same reason two panels wearing one switch
in two liveries is worse than either livery. `face::FaceStyle` resolves the
themed surface, the glass and the three ink tiers. `dialogs::FACE_SIZES`
takes a per-unit window.

**The dial standard is the house's** — `recorder_panel::draw_knob` is the
reference: the NAME above the control, the READING below it, the sweep
centred on *up* so the tick skirt arcs over the top where a hand on the knob
is not covering it, and the ticks drawn outside the body because they are a
mark on the panel the knob is mounted through. Follow the derivation, not the
numbers.

**The Moog dress, applied per-theme.** Black moulded body, inset cap, a full
screwdriver-slot pointer straight through the centre (a short pointer at one
edge is a dot at this size). What is *themed* is the panel ground, the
engraved legends, and the cap colour — one accent per section, the way the
transport's tempo knob is a darkened orange so it is not read as a fourth
send. The slot stays the house's light colour so it reads on every cap.

**Layout**, left to right, in the order the signal flows and the original
laid it out:

```
┌─ TANGENT ANALOG ───────────────────────────────────────────────  ◬  ─┐
│ CONTROLLERS │ OSCILLATOR BANK │ MIXER │ MODIFIERS      │ OUTPUT      │
│  Tune       │  1 range wave   │ Osc1  │ Filter Cutoff  │ Volume      │
│  Glide      │  2 range tune w │ Osc2  │ Emphasis Mode  │ Spread      │
│  Mod Mix    │  3 range tune w │ Osc3  │ Contour Kbd    │ Ensemble    │
│  Depth      │  [KBD] [SYNC]   │ Noise │ ADSR  ADSR     │ Width       │
│  Bend  Prs  │  PWM   Drift    │ Drive │ Vel   Vel→Flt  │             │
├──────────────────────────────────────────────────────────────────────┤
│ VOICE [MONO|UNISON|POLY]  Voices  Detune  Spread  Drift  Prio  Trig   │
│ FILTER [LADDER 24|SEM 12]   LFO rate wave delay  [OSC MOD][FLT MOD]   │
│ ARP [OFF|UP|DOWN|UP-DN|RND|PLAY]  Rate  Oct  Gate  [HOLD]            │
├──────────────────────────────────────────────────────────────────────┤
│ ‹ preset selector ›                          [ Save ]  [ Save As… ]   │
└──────────────────────────────────────────────────────────────────────┘
```

**The badge.** The app's own mark goes on the faceplate, at the right of the
header band, opposite the model name — a maker's badge at one end and the
model at the other, which is where both live on the panels this one is
after. It is **drawn, not loaded**: `assets/icon-small-square.svg` is a
triad, an equilateral triangle stroked with a filled circle at each vertex,
and on its 512 viewBox the geometry is vertices `(256,150)`, `(136,356)`,
`(376,356)`, node radius `82`, stroke width `48`. Nine numbers. Reproduce
them exactly and scale to the badge rect, so it stays the icon rather than a
thing that resembles it.

**Themed, like everything else on this panel.** The mark takes the panel's
own ink and sits on the panel's own ground. It does **not** carry the icon
file's `#191919` plate or its `#E8DCC0` ink: those are a dark icon's colours
and stamping them here is exactly the "ink inherited from somewhere else"
bug this repo has shipped seven times. Same ink as the engraved legends.

Put the painter in `face.rs`, beside the rest of the shared face vocabulary,
rather than inside `faces_analog.rs` — the next panel that wants a badge
should find one, not redraw it.

Suggested `FACE_SIZES` row: share `(0.62, 0.52)`, floor `900 × 560`, ceiling
`1400 × 840`. **The floor is a promise and a test must hold it** — the amp
face has exactly one point of headroom at its own floor on JetBrains Mono,
and `the_amp_is_whole_at_its_own_window_on_every_font` is why that is a
known fact rather than a bug report. Write the equivalent test here, on every
font, before the panel grows a row.

**The `sem_mode` knob is inert under the ladder**, and that is the one place
this panel disobeys the Keys principle that showing a rack with half of it
dead is a lie about both instruments. The mitigation is that it is *drawn*
dead: dimmed to the caption tier, ticks unpainted, drag ignored. A real panel
with a filter selector would have the same knob and the same dead half; what
would be dishonest is leaving it looking live.

**Gestures**: drag turns (relative, so the pointer can leave the knob and go
on turning); double-click resets to default — the fx-panel convention, and
the release-branch landmine noted there applies, egui delivers a second
release. No typed entry in v1.

**Theming is proven by tests, not by eye**, because dialog screenshots are
still impossible (egui viewports against the offscreen compositor). Run the
new face through the same sweep `face.rs` already has —
`ink_on_its_surface` and `a_worst_case_face` — across every shipping theme,
including Silverface. **Ink is a property of the surface**; it is never
inherited from a themed role and never picked by a brightness threshold.
Seven instances of that bug have shipped and every one was found by a
screenshot rather than a test. And where neither ink clears 4.5:1 — the dead
zone around 0.12–0.22 luminance — **the surface moves**, via
`legible_surface`, exactly as `FaceStyle::new` already walks the glass.

---

## 7. The integration contract: the first built-in VOICE

Today the sentinels fork two ways. Instruments (`@tangent-dx7`,
`@tangent-drums`, `@tangent-keys`) step *out* of the rack: a mask in
`Shared`, a branch in `mix_channels`, hand-plumbed atomics per knob, and no
automation of any of it. Effects (`@tangent-eq` and the twelve beside it) are
seated *in* the rack by `load_insert` as `Backend::Builtin` with
`voice: false`, and inherit the hand-off rings, `run_rack`, delay
compensation, reorder, export, state and automation without any of those
learning what they are.

**Tangent Analog takes the effects' road and turns the voice flag on.** The
code already names the day:

```rust
// instrument.rs:2718, in Backend::process_voice
// Unreachable by construction — the loader seats every built-in
// with `voice: false` — but an arm that renders silence would
// hide the day that changes, and an error names it.
Backend::Builtin(_) => Err("a built-in effect takes no notes".to_owned()),
```

This is that day. The changes, exactly:

1. **`UnitSpec` gains `voice: bool`** (`builtin_fx.rs:89`), `false` on all
   fourteen existing rows, `true` on the analog's.
2. **`load_insert` stops hardcoding it** (`instrument.rs:10899`, the comment
   there — "Always an effect: a built-in INSTRUMENT is a mask and a
   `mix_channels` branch, never a bay tenant" — is what this wave rewrites,
   and rewriting the comment is part of the work).
3. **`process_voice` gains a `Builtin` arm**, and gains two arguments it does
   not have today:
   - `params: &[tangent_host::ParamPoint]` — `bay_params` is already
     computed per bay at `instrument.rs:3379` and handed only to
     `process_effect`. **Without this the automation lanes point at a knob
     nothing reads**, which is the whole reason for choosing this road. Pass
     it; leave the VST3 and LV2 arms ignoring it exactly as they do now.
   - `transport: Transport` — `{ bpm: f64, playing: bool, beat: f64 }`,
     read from `Shared.bpm` (`instrument.rs:2147`, already loaded once a
     block at `:5192`) and the playhead. The arpeggiator needs it. Do **not**
     take the opportunity to fix the echo's free-time-only limitation in this
     wave; note it and move on.
4. **`is_instrument_sentinel` (`desktop.rs:1270`) does NOT gain
   `@tangent-analog`.** Its doc comment says a new instrument sentinel is a
   deliberate line; this one is deliberately not one, because it is a bay
   tenant. Adding it there would filter the analog out of the engine and the
   symptom is a synth that loads, names itself, draws its panel and is
   silent.
5. **The picker**: `dialogs.rs:3574`, index 3 under `LABS_VENDOR`, after
   Keys. Display name `Tangent Analog`. `is_builtin` needs no edit — the
   leading `@` is the whole test.
6. **Presets fall out free**: `desktop::preset_subfolder` already routes any
   `builtin_fx` spec to `spec.class`, so `Tangent Analog` becomes the folder
   under `Documents/Tangent Media/Presets/` with no new line.
7. **The face**: a `faces_analog.rs` beside `faces_amp.rs`, dispatched from
   `faces::draw` (`faces.rs:744`), plus the `FACE_SIZES` row.
8. **The bypass row is skipped.** `faces::draws_its_own_bypass` is currently
   a two-name list meaning "this panel draws its own"; a voice needs a third
   answer, "a voice has no bypass row" — bypassing bay 0 already silences the
   lane and the mixer already has mute. Do not bend the existing predicate
   into meaning both things; give the rule its own name.
9. **Latency** is whatever `unit.latency()` honestly reports. The 4×
   oversampling has group delay; report it and let PDC do its job.
10. **`SUITE_UNITS` is an effects table**; the analog is not in it. The face
    sweep tests iterate it, so they need the analog added explicitly or a
    sibling table — say which, in a comment, so the next instrument does not
    have to work it out again.

**MIDI arrives properly on this road.** `process_voice` gets `notes` with
sample offsets and `controls` carrying CC, `Control::PITCH_BEND` and
`Control::AFTERTOUCH` with offsets and channel. Note that **clips carry
notes only** (`ClipNote { at, dur, key, vel }`) — bend and wheel are
live-play only, which is precisely why knob automation had to be free.

---

## 8. What this wave does not touch

- The three existing instruments, their masks, their settings keys.
- `strip_keys_params` / `strip_keys_params8` and the settings shear generally.
  The analog stores nothing in settings; its state is the bay's state blob.
- The suite's thirteen units and their faces, except the one dispatch line.
- `STRIP_COLORS`, palette indices, the theme table. Silverface stays as it is.
- `effects.rs` and the fixed chain.
- Anything in `scripts/`. The release policy is unchanged: `main` carries
  `X.Y.Z-dev`, only `release.sh` writes a real version.

---

## 9. The bench, and the numbers it has to print

**Build `tools/analogrig` first and keep it.** Same shape as `tools/keysrig`:
`#[path]`-include the real DSP so there is one copy and no drift, zero
dependencies, a hand-written 24-bit WAV writer (a new crate costs a
THIRD-PARTY-LICENSES byte-diff), and a build measured in seconds rather than
the minutes the DAW takes.

Performances: `single`, `velramp`, `chord`, `glide`, `arp`, `unison`,
`sweep` (a filter sweep at several resonances), `restrike`, `steal`,
`report`.

Acceptance gates, each printed as a number and asserted by a `#[test]`:

| what | how it is measured | gate |
|---|---|---|
| Alias floor | saw at 8′ C7, worst non-harmonic bin against the fundamental | ≤ −60 dB |
| Ladder self-oscillation | cutoff swept, oscillation pitch read against the mapping | ≤ ±15 cents |
| Ladder bass loss | 100 Hz level at emphasis 0 vs 0.9 | measurable, monotone, documented — not "fixed" |
| SEM stability | resonance 1.0, cutoff swept 20 Hz–20 kHz, peak sample | finite, ≤ +12 dB |
| Envelope timing | 10–90 % rise, Schroeder-integrated decay | within 5 % of the mapping |
| Restrike | the first/second difference edge at the event against a fresh strike's own edge | no excess step, per the keys harness |
| Steal | corpse-fade click, same measure | no excess step |
| Drift determinism | render twice from the same seed | bit for bit |
| Tuning | every range × every note, Goertzel against equal temperament | ≤ ±1 cent, drift zeroed |
| Level | eight-voice forte chord | ≤ −6 dBFS, clip-free |
| CPU | 16 voices, ensemble on, 48 kHz, release build | ≥ 40× realtime on this machine, printed either way |

**Two measurement traps, both already paid for in this repo.** A
rectangular-window Goertzel *lies* about decay — the fundamental's 1/f
sidelobes floor every high partial and a threshold reads a phantom; use a
Hann window, Schroeder backward integration and a T30 read. And a spectral
centroid is useless when a fundamental is in the frame. A measurement you
have not validated is a confident opinion with decimal places, and one of
those has already sent a defect report out of this repo with the cause
inverted.

---

## 10. The laws

Restated because each one has been broken here at least once.

1. **No new dependencies.** `release.sh` gates on a THIRD-PARTY byte-diff.
2. **Ids and symbols append-only, forever.** So is `STRIP_COLORS`; so are
   settings keys, which go at the END of `to_map`.
3. **Ink is a property of the surface** (§6). The dead zone moves the
   surface, not the ink.
4. **`scripts/check.sh` clippy is RED already** — ~876 pre-existing
   `unused_qualifications` and `linuxvideo` on macOS. Do not chase it, do not
   "fix" it, and do not let a green-tests claim be quietly stated as a green
   check.
5. **Verify that new tests actually run.** A `#[test]` attribute sat stranded
   on the wrong function in this repo for its whole life, and the test that
   pinned the bench tuning never ran once.
6. **Do not launch the app to verify a build.** Build it, install it with
   `scripts/install-macos.sh`, and report. The launch is the owner's.
7. Commit at each wave boundary; **do not push**.

---

## 11. Waves, and what done means

**Wave 1 — the kernel and the bench.** `tangent/src/analog/{mod,osc,filter,
env,voice,ensemble,arp}.rs`, `tools/analogrig`, every gate in §9 printing.
Touches nothing outside those two trees plus one `mod analog;` line. This
wave is self-contained and testable and is the one worth handing to a single
long-running session.

**Wave 2 — the road and the panel.** The ten changes in §7, plus
`faces_analog.rs` and its geometry and theme tests. Merges green or it does
not merge.

**Wave 3 — voicing and the factory presets.** Twenty-four presets across
mono leads and basses, poly brass and pads, plucks, ensemble strings and two
arpeggios. **Evocative names, never trademarks** — the units may be described
referentially in `docs/ANALOG.md` with the usual disclaimer, and never in
branding. Then `docs/ANALOG.md`, written like `docs/KEYS.md`: what it is, why
a model, what each knob moves, and what is unfinished.

Done is: the suites green (`527/913/76` was the last full count), the gates in
§9 printing their numbers, the panel legible on all sixteen themes by test,
a preset saved from the face and reloaded in a fresh session, a filter sweep
drawn as an automation curve and heard on playback, and the same project
bounced twice byte-identical.
