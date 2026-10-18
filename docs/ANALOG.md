# Tangent Analog — a subtractive synthesiser, solved

Built 2026-09-04 against `main` from `d4884d0` (5.0.0-dev), to the contract
in `docs/ANALOG-BRIEF.md`. Written for whoever changes the model next, and
for whoever wonders why a knob is where it is.

## What it is

The fourth built-in instrument, on the sentinel `@tangent-analog`, and the
first built-in **voice**: where the DX7, the drumpad and the keys are rendered
by a mask in the mixer, the analog is seated in a rack bay exactly as the
suite's effects are, with one flag turned on, so it inherits the hand-off
rings, delay compensation, the state blob, presets and every automation lane
for free. Load it in any MIDI channel's bay from the picker's Tangent Labs
shelf, fourth row, and click the bay to open its panel.

It contains no samples. Three oscillators, each with the six ranges (LO, 32′,
16′, 8′, 4′, 2′) and six waves (triangle, triangle-saw, saw, square, wide and
narrow pulse); a mixer with a level for each, a noise source that is white or
pink, and Drive — the output patched back into the mixer, which is what "loud"
meant on the 1970 machine and is most of what the growl is; two filter
characters behind one switch; two contours; a voice-mode switch that runs the
whole thing as one gliding monophonic line, a unison stack of up to sixteen
detuned voices, or a polyphonic board of up to sixteen with tolerance between
them; one LFO; a three-line ensemble; an arpeggiator with hold; and a volume.
One panel, one preset format, one set of knobs.

It is modelled on two machines and pretends to be neither: the 1970
monosynth for the mono side — three oscillators into a four-pole transistor
ladder, oscillator three doing duty as the modulation source — and the 1980
eight-voice for the poly side — a two-pole state-variable with no bass loss,
per-voice component tolerance, an ensemble behind it. A preset can be either;
the two are the same instrument voiced differently.

## What each knob moves

**Tune** is ±100 cents on everything. **Glide** is a time, 5 ms to 2 s, with
zero meaning off; the pitch slews exponentially, which is what an RC on a
control voltage does. **Mod Mix** and **Depth**, **Bend Range**, **Pressure**:
see the modulation section below.

**Range** and **Wave** per oscillator are rotary switches and drawn as dials
with detents. Oscillators two and three have a **Tune** of ±7 semitones with
a centre detent; oscillator one has none, because the master Tune is its tune.
Oscillator three's **KBD** switch is the 1970 machine's whole trick: switched
off, it stops tracking the keyboard and becomes the modulation source, at LO
a vibrato and at 8′ an audio-rate modulator. **SYNC** resets oscillator one's
phase from oscillator two's wrap. **PWM** is the LFO on the three pulse
widths, and does nothing to the other waves. **Osc Drift** is the slow
wander, described under "warm".

The mixer's four levels and **Drive** feed the filter. Three oscillators at
full lean on the mixer's overload; one does not.

**Filter** chooses **Ladder 24** or **SEM 12**. **Cutoff** is 20 Hz to
20 kHz, logarithmic. **Emphasis** is the resonance: on the ladder it thins the
bass as it rises (uncompensated, on purpose — see below) and sings at the top;
on the SEM it peaks up to about eleven decibels and never oscillates, which is
that filter's own law. **Mode** is the SEM's low-pass → notch → high-pass, and
is dead under the ladder, drawn dead. **Contour** is the filter contour's
amount, up to six octaves; **Keyboard** is how much the cutoff follows pitch,
one being fully. **Vel→Flt** and **Velocity** are velocity to cutoff and to
amplitude, kept separate because a synth that cannot be played quietly is a
synth nobody plays twice.

Two **ADSRs**, filter and amplifier. Attack is 1 ms to 10 s and is the time to
the peak; decay and release are 2 ms to 15 s and are T60, the time to fall
sixty decibels, the number a room's decay is quoted in. The curves are a
capacitor's, not lines.

**VOICE** is MONO, UNISON or POLY. **Voices** is one to sixteen: the stack in
unison, the pool in poly. **Detune** spreads a unison stack up to ±25 cents
between its outermost voices and **Spread** throws it across the field.
**Drift** is the master of the analogue-tolerance model. **Priority** is low,
last or high note for the mono side — low is the 1970 machine's, and the
reason its basslines sit where they do. **Trigger** is legato (single) or
retrigger.

**LFO** rate, wave and delay: 0.05 to 30 Hz, five shapes, up to three seconds
of fade-in from the note. **OSC MOD** and **FLT MOD** route the modulation bus
to pitch and to cutoff.

**Ensemble** is off, I, II or III, and **Width** spreads its lines. **Spread**
in the output section is how far poly voices sit across the field,
independent of the ensemble. **Volume** is a quartic taper: the shipped 0.7
puts an eight-voice forte chord near −8 dBFS, and the top of the travel is
twelve decibels above that, which is where one mono voice needs to be to reach
a mix.

**ARP**: off, up, down, up-down, random, as played; **Rate** from a quarter to
a thirty-second with triplets; **Octaves** one to four; **Gate**; **HOLD**, a
latch that is also a parameter, so it automates.

## The modulation, and why it is not a matrix

One bus, two switches. The left side of Mod Mix is oscillator three when its
keyboard switch is off — the 1970 machine's arrangement, an oscillator become a
modulator — and the dedicated LFO when it is on, because a polysynth cannot
spare an oscillator and cannot do without a vibrato. The right side is noise.
The bus reaches pitch through OSC MOD and cutoff through FLT MOD, scaled by the
mod wheel up to the Depth ceiling; Depth is square-law so the vibrato range
lives in the lower half of the knob. Channel pressure adds a little LFO
vibrato and a little cutoff of its own. PWM is the LFO on the pulse edges,
unswitched. That is the entire section, and a second LFO and a matrix are
explicitly out.

## The parts that were not invented

**Oscillators** are anti-aliased by a band-limited step, tabulated. The brief
named polyBLEP and polyBLEP was written first: a four-point kernel, the
integrated cubic B-spline. It measured −47 dB on the brief's own gate, and the
shortfall is arithmetic: a cubic B-spline's transform is sinc⁴, only 21 dB
down where the thirteenth harmonic of a saw at C7 folds, and no polynomial
kernel a few samples wide is sharper near the fold. The gate (−60 dB) is what
the method was chosen to serve, so the method moved: the residual is that of a
64-sample Kaiser sinc from the same designer the suite's oversampler uses,
integrated once for the step and twice for the corner, held 64× oversampled.
It reads −107 dB. Hard sync is a step of `wave(0) − wave(p)` plus a corner of
`slope(0) − slope(p)` at the reset, through the same path, so it aliases no
worse than the unsynced wave. Phase free-runs: nothing resets on a note.

**Both filters are zero-delay-feedback and solved.** The ladder is four TPT
one-poles with the differential pair's tanh on the difference between input
and fed-back output; once the four linear stages fold into `y = G⁴·u + S` the
whole loop is one equation in `y`, monotone, and one Newton step from a linear
prediction of the last two outputs settles it at 4× — the self-oscillation and
bass-loss tests read the same numbers with three steps. Its bracket is applied
as `max().min()` and not `clamp()`, and that is worth a sentence: `clamp`
asserts its bounds are ordered, and that panic branch inside the loop body
both kept the laned form scalar and halved the scalar ladder's throughput.
Resonance thins the bass — the DC gain is `1/(1+k)` — and nothing compensates
it; the brief calls that a feature and it is. The SEM is Zavalishin's
state-variable, LP, BP and HP from one integrator pair, mixed by the Mode
knob. Chamberlin's form would go unstable exactly where a resonant sweep
lives.

Both run at 4× through the suite's own halfband pair (`units/drive.rs`'s
designer, tap counts and shaded cutoff), so there is one oversampler in the
repo. The drive path — the amplifier's output back into the mixer — closes at
4×, which is the difference between a growl and a comb filter.

**Envelopes** charge toward a target above the rail and switch at one, so
the attack is convex; decay and release are true exponentials.

**Restrike is superposition, never a reset.** A same-pitch restrike attacks
the contours from their current level and leaves the oscillators and the
filter running; a steal moves the dying voice to a corpse slot that fades over
eight milliseconds. Zeroing a live state is the pop, and the pedal makes it
loudest.

## What "warm" actually means here

Five specific inexactnesses, each a line with a number on it, and each held by
a test:

1. **Per-voice component tolerance**, drawn once from a fixed seed per voice
   index: cutoff ±3 %, envelope times ±5 %, output level ±0.4 dB, tune ±2
   cents. Those are the numbers at the Drift knob's MIDPOINT — a calibrated
   board; the top of the knob is twice that, a board that has not seen a
   trimmer in a decade, and the shipped quarter is half a calibrated board.
2. **Oscillator wander**: a filtered random walk per oscillator, ±3 cents at
   Osc Drift's midpoint, shaped by a one-pole at 0.1 Hz.
3. **Free-running phase.**
4. **The ladder's bass loss**, uncompensated.
5. **The drive path**, where a chord stops being a sum.

The seeds are constants. A bounce is the playback, twice, on every machine,
and `the_same_score_renders_bit_for_bit_twice` holds it.

## The cost, and what it is not

Sixteen voices with the ensemble render at about 24× real time on an M5 Pro:
4 % of one core, 55 ns per voice-sample. One note is 1.5 % of a core, eight
2.4 %, because the filters and the 4× pair run eight voices at a time in lanes
that follow whoever is sounding — a chord costs what it plays. The lanes are a
data-layout change and nothing more: the same scalar tick written over
`[f32; 8]` arrays, which the compiler turns into NEON on arm64 and SSE on
x86_64 by itself, so the six shipped artifacts compute the same numbers, and
two tests hold the laned and scalar roads equal bit for bit.

The brief's §9 asked for 40×. That number was written before the architecture
was measured. What remains under the lanes is the oscillators — a branch per
discontinuity, which will not lane — the contours, the overload and the glue,
and the honest ceiling of this design on this machine is about 25×. The owner
was given the arithmetic and the comparison (a sixteen-voice instance here
costs about a third of what circuit-modelled competitors take per voice) and
chose to keep the design: no eco switch, polyphony is the knob. The test that
asserts 40× is kept and ignored by name with the measured figure in its
reason; a regression floor asserts at 11×.

## The bench

`tangent/src/analog/` is pure `std`, like the keys and the DX7, because a new
crate in the lock file costs a THIRD-PARTY-LICENSES byte-diff at release. It
compiles on its own through `tools/analogrig`, which includes the module by
`#[path]` — one copy of the source — and the one oversampler the same way:

    cargo run --release -- report          every gate in the brief's §9, as numbers
    cargo run --release -- perf            what each stage costs, per voice-sample
    cargo run --release -- presets out/    every factory preset to a WAV, with its peak
    cargo run --release -- sweep s.wav     a filter sweep at four emphases
    cargo run --release -- restrike r.wav  a note restruck under the pedal

`cargo test --release` there runs the module's own suite in about a second and
a half. The gates at HEAD: alias floor −107 dB; self-oscillation within 0.1
cents; bass loss −13.8 dB at 0.9 of the dial, monotone; SEM peak +11.5 dB;
envelope timing within 1.7 %; restrike shape 1.08; steal fade onset −21 dB;
determinism bit for bit; tuning within 0.03 cents; forte chord −7.8 dBFS.

### Measure, do not opine — and revert-check the measure

Two lessons the keys already paid for are written into `measure.rs`: a
rectangular-window Goertzel lies about a decay, and a threshold on a raw
level reads beats as decay. A third was paid for here. The keys' restrike
guard compares the first and second differences of what a strike added
against a fresh strike's own, and it is BLIND for this instrument: a saw is a
band-limited step at every period, so a fresh strike already has a step's
shape and a cut looks like a strike. Both the restrike and the steal guards
passed with their mechanisms deliberately broken until the tests gave the
contours a 25 ms attack and the steal was judged by how fast the stolen note
appears in the difference: a cut is level, a fade is twenty decibels down in
its first millisecond. Every guard in this module has been reverted once and
watched go red.

## Integration

`builtin_fx::UnitSpec` carries `voice: bool`, false on the fourteen effects
and true on the analog; `prepare_builtin` takes it from the table where it
hard-coded `false`. `Backend::process_voice` takes the block's automation
points and a `Transport` the rack builds from the shared board's tempo,
rolling flag and playhead — the sample rate was added to `Shared` for the
frame-to-beat arithmetic — and its `Builtin` arm, which for a long time was
the error that named this day, hands everything to `AnalogUnit`. That wrapper
translates VST3's notes and controls (bend, pressure, CC 1, CC 64, the two
panic controllers) into the kernel's events in time order without allocating.
The built-in voice reports its latency, 68 samples, where a VST3 voice stays
exempt, because it has a route to say it.

`desktop::is_instrument_sentinel` does NOT name the analog. That list is the
three mask-rendered instruments, and adding this one would filter it out of
the engine — a synth that loads, names itself, draws its panel and is silent.

The panel lives in `tangent-ui/src/faces_analog.rs`: sixty-one controls placed
by a table, the house's dial in the Moog dress with five section caps derived
from each theme's accent and walked until the light slot reads on every one,
the app's own mark drawn from the icon's nine numbers in the panel's ink. The
floor of 900 by 560 is held on every font by test; the ink on every theme by
reading the shape stream; a real drag and a real click through egui's own
input.

## Presets

Twenty-four ship, seeded as ordinary files into `Presets/Tangent Analog/` the
first time that folder is asked for, whole (every dial by symbol) and tagged
`factory` so `head` can tell them from yours. From then on they are yours:
rename, delete, save over. A deleted file stays deleted; delete the folder and
the set comes back. Five leads, five basses, three brass, four pads, three
plucks, two string ensembles, two arpeggios; every one auditioned on the bench
and held between −18 and −1 dBFS.

## What is unfinished

- **The oscillators do not lane.** A branchless oscillator would be a
  different design, and the number does not ask for it.
- **The face has been proven, not seen.** Dialog screenshots are still
  impossible in this repo, so the panel's legibility and geometry are held by
  tests and the owner's eye.
- **Automation points land at their offsets inside a block** for the analog,
  which is more than the effects' road promises; the echo's free-time-only
  limitation was noted and not touched, per the brief.
- **Clips carry notes only.** Bend and wheel are live-play only, which is why
  the automation lanes had to be free.
- **No typed entry on the panel's dials** in this version.

## Naming

The row's name is Tangent's and the reference goes in prose like this, the
rule every unit has kept since the phaser. "Minimoog", "Moog", "Oberheim",
"SEM", "OB-Xa", "Taurus", "Solina" and "Prophet" are somebody else's marks
and appear in no preset name, panel word or picker row; "Ladder 24" and
"SEM 12" are the panel's words for two filter characters, and "Pedal Bass"
and "Ensemble Strings" are Tangent's names for Tangent sounds. What they are
modelled on is a fact about the model and belongs here.
