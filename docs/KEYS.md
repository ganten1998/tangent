# Tangent Keys: ten instruments, computed

Built 2026-09-01 against `main` at "Pertinent entries, taken slowly"
(`6688495`, 5.0.0-dev). Written for whoever changes the physics next.

Since then the instrument has moved three times. It grew to eight sounds,
two of each mechanism; on 2026-09-04/05 its physics was reworked against a
brief of its own (`tangent-keys-handoff/KEYS-REWORK-BRIEF.md`); and on
2026-10-16/17 every sound was re-voiced against measured renders of a
reference instrument and two sounds were added. Everything below
still describes how the thing is built and why, but **"The physics rework"
says what changed in September and "The reference study (2026-10)" what
changed in October**, and where two sections disagree the later one is the
current instrument.

## What it is

A third built-in instrument beside Tangent DX7 and Tangent Drums, on the
sentinel `@tangent-keys`. It contains **no samples**. Every note is computed
from a model of the mechanism, which is why it ships ten instruments and no
data at all:

- **Intimate Grand**: a small room's grand heard close, soft felt, wood and
  the action.
- **EP Mk.1**: a tine electric piano, taken dry. Bell-like when it is played
  lightly and it barks when it is not.
- **Concert Grand**: the hall instrument, a longer ring, a deeper bass and a
  wider swing.
- **EP Suitcase**: the same tines through their own amp, with its stereo
  throb.
- **Clav**: struck-and-held strings read by two coils, taken dry.
- **Clav Wah**: the same strings through a filter that chases the touch.
- **Reed 200**: a reed piano, dry: hollow when soft, growling when hard.
- **Reed Amp**: the reed piano through its amp, its slap, cabinet, throb and
  room.
- **Electric Grand**: a stage grand, short stiff strings heard through
  pickups and no soundboard.
- **Nylon Guitar**: six nylon strings under a fingertip, the open ones
  ringing along. Every key plays: one under the low E or past the
  fingerboard is a string of its own, as a harp guitar's extra strings are.

Load it in any MIDI channel's first bay, exactly as the DX7 loads, in as
many bays as you like, because every lane owns a full instance of the
instrument: a piano track and an EP track in one session, or eight of either,
each with its own settings. Clicking the bay opens a window with the ten
sounds and, under whichever is chosen, that sound's own rack of eight.

The first cut had no knobs at all, on the argument that every control on a
physical model is a way to make the instrument worse. What answers that
argument is not "users want knobs" but *what the knob is allowed to be*:
every one of these moves a quantity the physics already had — a hammer's
contact width, a loss filter's target decay, the cents between the strings of
a course, how far a tine swings against its magnet — and there is not a filter
or a saturator among them. Turning one is voicing an instrument, not
processing a signal.

The lists differ per sound, because the instruments do. The piano has
**Voicing** (the hammer's felt, needled mellow or lacquered bright), **Width**
(where the strings sit across the field), **Decay**, **Brilliance** (how long
the upper partials outlast the fundamental), **Unison** (the detune inside a
course, which is where the beating comes from), **Stretch** (string stiffness,
which pulls the partials sharp), **Release** (the damper's speed) and
**Soundboard**. The electric piano has Voicing, Width, Decay, **Bell** (the
chime on the attack), **Bark** (how hard the tine leans on the pickup),
**Chime** (where the bell partial sits above the note), Release and
**Tonebar**; the suitcase trades Chime for its amp's **Tremolo**. The clavs
have **Pickup** (where the coil sits under the string), **Tone** or **Wah**,
**Snap** and **Mute** (the sliding damper pad); the reed pianos **Reed**,
**Growl** (where the reed rests in the plate's field), **Tremolo** and
**Thump**; the electric grand the piano's string knobs with a **Pickup**
(where the pickups read the strings) in place of the soundboard; the guitar
**Position** (where the finger plucks), **Body** (the hollow body's air),
**Pick** (how long the finger holds the string before letting go) and
**Strings** (the open strings that ring in sympathy). Showing one fixed rack
with half of it inert would be a lie about every one of these instruments.

**Every knob's default is the bench's own number.** The mappings are written
so that the centre lands exactly on the constant this instrument was tuned
with — decay ×1.000, brilliance 0.28, unison ×1.000 — which is what makes an
untouched strip, a freshly initialised sound and the two-knob build that
shipped first all identical. A test asserts it, because a knob whose middle
multiplies by 1.02 is a silent retune of both patches that no one would ever
catch by ear.

Double-click a knob to put it back; Initialise puts the whole sound back and
leaves the other sound alone. Values are per strip **and per sound**, so a
piano track and a Rhodes track keep their own and switching a strip between
them finds each as it was left.

## Why a model and not a recording

The same argument the DX7 makes, one size larger. A convincing sampled piano
is gigabytes, and it is gigabytes because a recording is a still photograph of
one hammer hitting one string at one speed: every velocity needs its own take,
every take needs a loop point, and the seams between them are what the ear
finds first. A model has no seams. Hit the string harder and the felt
compresses further, the contact gets shorter, and the tone brightens — because
that is what the arithmetic does, not because a louder take was selected.

## The two mechanisms are not one mechanism

This is the load-bearing decision in the module, and it is why `string.rs` and
`tine.rs` share nothing but the frame.

A piano string is flexible and under enormous tension. Its partials are very
nearly whole-number multiples of the fundamental, so a **digital waveguide** —
a delay line with a filter in the feedback loop — models it almost exactly.
The delay's length is the pitch; the filter is the decay; an all-pass in the
loop makes high partials travel faster than low ones, which is stiffness,
which is why a piano's partials are stretched sharp.

A Rhodes tine is a stiff steel rod clamped at one end, and a stiff bar's modes
are nowhere near harmonic — the textbook clamped-free ratios run
1 : 6.27 : 17.55. A waveguide **cannot** produce those, because a delay line
makes harmonics by construction. So the electric piano is **modal**: a bank of
decaying sinusoids, each free to sit wherever the metal actually puts it.

Modelling both with one algorithm and different numbers is the mistake that
makes every electric piano preset sound like a detuned piano.

## Where the electric piano's velocity response comes from

Three separate mechanisms, and all three have to be present:

1. **Contact time.** A harder strike is shorter and therefore wider-band, so
   more energy lands in the high modes. Each mode takes the amount of energy
   the contact pulse actually contains at its frequency — a first-order
   amplitude rolloff at `1/contact`, contact running 2.2 ms feathered to
   0.7 ms hard.
2. **The bell's own weight**, because a stiffer, more compressed neoprene tip
   also drives the tine's high mode harder.
3. **The pickup.** Flux through the coil goes as the inverse square of the
   gap, so the transfer is `out = x / (1 - x/d)^3`. Well inside the gap this
   is indistinguishable from a wire; approaching it the positive half grows
   and the negative half compresses, which is even-harmonic distortion — a
   lopsided waveform, and lopsided is what a bark *is*.

The third one has to live in the model rather than in an overdrive after the
fact, because it is a function of **displacement**: it arrives when the player
hits hard and it is simply not there when they do not. A waveshaper on the
output would dirty the quiet notes too, and quiet is where this instrument is
supposed to be at its cleanest.

Measured across MIDI velocity 8 to 127 on E4, bell-to-fundamental runs
**−48.1 dB to −18.1 dB** and waveform asymmetry runs **0.017 to 0.382**, both
monotonically. That is the brief — light is bell, hard is bark — as a number.

## The physics rework

Four mechanisms, four things that were shaped by hand and are now shaped by
the thing they model. The whole of it was gated on measurement: every
acceptance below is a named test, and every one was checked by breaking the
mechanism it guards and watching that test go red.

**The piano's hammer is a mass in contact with the string.** It used to pour
in a Gaussian whose width was a fitted function of velocity. It is now a felt
spring the string pushes back on, so the contact duration comes out of the
argument rather than being posted into it: 2.08 ms at the softest note falling
monotonically to 1.00 ms at the hardest, within 4 % at every sample rate the
engine runs. The partials at C4 mezzo moved by at most 1.5 dB, which is the
proof that this was a re-derivation and not a re-voicing. VOICING stopped
being a width and became the felt's stiffness, which is what the panel always
called it. A hammer meeting a string already moving toward it now takes energy
out honestly, where the modal banks fake it with a floor.

**Its soundboard grew a plate.** Sixty-four resonators at the board's own
modes, designed from four numbers per instrument, beside the feedback-delay
network that stays for the diffuse tail. Under a held C4 four modes below
300 Hz stand more than 10 dB above the same measurement with the plate
bypassed, and the body's total level at the default knob moved by a hundredth
of a decibel.

**The strings nobody struck now answer.** Eighty-eight undamped loops, sized
per pitch, opened by the pedal and by every key that goes down, gated so a
silent one is not ticked. An idle harp costs 3.3 ns a sample and a pedalled
chord 47, against 190 for all eighty-eight forced at once; the whole bank is
125 KB. Pedal down, a struck C2 lights the C3, G3 and C4 strings, and breaking
the opening makes that test say so.

**Three transducers replaced three waveshapers**, and this is the change that
buys the most. Every pickup here is now the same shape — a static
characteristic, a rest offset, and a derivative — because that is what a
pickup is. The Wurlitzer's reed swings through the plane of its plate and the
Rhodes' tine across the magnet's axis, so both curves are non-monotonic and
both put out two events per mechanical cycle, which is what the measurements
of the real instruments show and what no saturator can do. The clavinet's
string never crosses its magnet, so that one is monotonic and its dirt stays a
gentler colour. Three knobs stopped scaling an abstraction and became the
adjustment a technician makes with a screwdriver: **Bark** is the tine-height
screw, **Growl** is the reed's regulation against the plate, **Snap** is how
much of the clavinet's non-speaking length rejoins the string at release.

Two traps came with that, both of which this module had already hit once.
A rest offset of zero makes the transfer even and the note plays an octave up,
so it is clamped away from the axis. And the normalisation has to be computed
from the transfer over the swing the strike actually reaches, or the knob
becomes an inverse volume control — which it had been, twice.

**Two schedules ran the wrong way and now come off a beam.** The Wurlitzer's
overtone ratios fell toward the bass where a solder blob at the tip raises
them; they now come from a clamped-free beam with a taper and a tip mass, and
the ratio is highest in the bass. The clavinet's inharmonicity rose with
pitch where eight measured tones say it falls sixfold across the keyboard; it
is now a least-squares fit to those tones, residuals within 10 %.

**Two constants had to be re-derived rather than re-tuned**, and both are
written down where they live with what they were measured against. The
restrike floor moved from 6 dB to 8: the four instruments that grew a pickup
now cancel 5 to 7 dB when a strike lands against the motion, because
harmonics made out of the fundamental cancel *with* it where independent
oscillators used to fill the hole — and the two instruments that did not grow
one stayed inside the old six. The wah's envelope follower went from 5.5 to
35 because it is a mean-of-absolute detector and the clavinet's new pickup
raised the crest factor, so the same instrument read as a softer one and left
the filter shut on the first chop of a run.

Cost at sixty-four voices with the pedal down, measured on one machine before
and after: the jazz piano 25x to 23x, the concert 22x to 19x, the Rhodes 37x
to 33x, the clavinet 108x to 61x, the Wurlitzer 69x to 62x. The floor is 25x
and the machine was under load in every one of those readings; the brief's own
idle baseline for the piano was 78x.

## The reference study (2026-10)

On 2026-10-16 to 18 every sound was re-voiced against measured renders of a
reference instrument, and two sounds were
added. The method was measurement, not copying: the reference was only ever
driven offline (MIDI in, WAV out, no audio device), and everything taken
from it is a number read off its output: a partial's decay, a velocity law,
a halo's level, a release slope. No code, data or preset of the reference is
in this module, and the mechanisms are this module's own. Where a fit and a
physical argument disagreed, the argument won and the difference is written
down where the number lives. The study itself, with every table it
measured, is in the owner's handoff folder (`tangent-keys-handoff/`).

Each sound was built by its own lane against a written brief, checked by an
adversarial verifier that re-measured it, and fixed. The integrated frame
then had two reviews and eight more lanes of the same kind, which fixed its
pedals and edges, solved the grands' strings ahead and paid for them by the
frames that go by, carried a landing's motion onto its new modes, and made
the host's buffer size no part of the sound. The acceptance tables compare
each sound with its reference row by row (a row is one measured quantity on
one key at one velocity, with a tolerance the brief set before anything was
fitted; the counts are as each sound's own lane last measured them):

| slot | sound | mechanism | voiced against | rows passing |
|---|---|---|---|---|
| 0 | Intimate Grand | `grand` (modal) + `symp` | an intimate, close-miked grand | 107 of 138 (55 before) |
| 1 | EP Mk.1 | `tine` | a dry tine piano | 15 of 16 |
| 2 | Concert Grand | `grand` (modal) + `symp` | a concert grand | 80 of 110 (37 before) |
| 3 | EP Suitcase | `tine` | a tine piano through its amp | 10 of 11 |
| 4 | Clav | `clav` | a clavinet, dry | 25 of 30, with the wah's rows |
| 5 | Clav Wah | `clav` + `clav_wah` | the same through an envelope wah | (above) |
| 6 | Reed 200 | `reed` | a reed piano, dry | 63 of 65 |
| 7 | Reed Amp | `reed` + `reed_amp` | a reed piano through its amp | 65 of 68; the amp chain all |
| 8 | Electric Grand | `egrand` + `symp` | a stage electric grand | 32 of 35 |
| 9 | Nylon Guitar | `guitar` | a classical guitar | 22 of 27 |

Slot 7 kept its index when it became Reed Amp, so a saved strip there keeps
its knobs (its Tremolo now means the amp's throb in dB, 0.5 the reference's
3 dB). Slots 8 and 9 are appended; `Patch::ALL` is still append-only. A rack
saved before the two new sounds existed gives them their shipped knobs, and
the legacy eight-sound arrays are still read at their own stride
(`dialogs::V32_KEYS_PATCHES`), whatever the live rack's count becomes.

### What changed in the engine

- **The grands are mode banks.** The Intimate and the Concert Grand play
  `grand::GrandVoice` on `modal::Bank` (768 modes), each partial a two-pole
  resonator placed on the stiff-string series with a per-key B and a stretched
  tuning, its loss read off a bridge admittance with board bumps, and three
  strings per course coupled at the bridge, which is where the two-stage decay
  comes from. The waveguide piano (`string.rs`) no longer plays any sound and
  has left the voice pool; its code stays for its tests and the bench, and
  `Body` is still both grands' board.
- **Their strings are solved before anyone strikes them**
  (`grand::GrandDesigns`): every key of both grands at the knobs' centres in
  `new`, then brought to the knobs as they move, a key for every 64 frames
  that go by. The host says what went by: a render pays its own frames, and
  a lane with nothing to play is told `Keys::idle`, so no call pays more than
  its own length (after a 512-frame grain a 16-frame visit had paid 322 to
  348 us). A knob put back where it was leaves no key stale. A fresh strike
  copies its key's strings instead of solving them; under the una corda it
  takes what the soft pedal does not move and solves the rest.
- **A landing never gains.** When new modes land under a ringing voice (a
  restrike, the una corda moving the hammer), each voice's ledger (each
  string's place and each mode's root, per partial) projects the strings'
  motion onto the new modes. It used to hand each mode's motion to the new
  mode of the same index, which under the una corda is another mode: the
  Intimate Grand's E5 at a tiny UNISON rang 13 times its first strike.
- **The hammer is graded by key and velocity** (`hammer.rs`, a midpoint
  contact with a felt core that hardens with rate), with level and
  brightness set apart, so the level law follows the reference (v20/41/70/98
  against v127: about -23/-17/-11/-5 dB) while fortissimo stays where the
  old piano had it.
- **The action speaks** (`grand_noise.rs`): the knock, the key's release,
  and the trapwork's thump when the dampers are thrown and land.
- **The pedals are levels.** `Keys::set_pedals` takes CC 64, 66 and 67 as
  0..127 every block: the grand's dampers and the harp follow the sustain
  part way (half pedalling), the sostenuto catches the keys that are down
  (sounding or not) and forgets them at a patch change, the soft pedal
  shifts the action. Every other sound, the electric grand included, hears
  the sustain as a switch at 64.
- **The sympathetic harp was rebuilt** (`symp.rs`): fed flat above each
  string's fundamental (shaped for the strings nobody struck), the struck
  string's halo only while its key is held, a released key's string ringing
  free under the pedal, the felt's bite when a damper lands, aliquots over
  the intimate's top courses, and a set designed per instrument in
  `Keys::new` and loaded at `set_patch` (both grands and the electric grand).
  The harp is strung where STRETCH tunes the voices it shadows (at the
  knob's top C8's string had sat 84 cents from its voice), and it is
  re-strung on a 128-frame grid of the timeline as the knob moves; a ringing
  string glides to its new tuning rather than jumping.
- **Every family is block-rendered and owns its stereo image.** The frame
  no longer pans voices and has no per-sample path; each voice renders up to
  `modal::BLOCK` frames into the accumulators with its own microphones or
  image.
- **New mechanisms:** `egrand` (short, stiff strings read by pickups, no
  board), `reed_amp` (the reed piano's slap, cabinet, throb and room), and
  `guitar` (six strings on a modal bank, a fingertip's pluck, and a shared
  `guitar::Fretboard` that picks each note's string and holds the open
  strings and the body's air).
- **A host's call size is not part of the sound.** The control a voice runs
  once a block (a damper's fall, a contact solve, a cull, a tremolo's phase,
  the wah's follower) ran once a host CALL, so the same performance at
  64-frame buffers and at 512 were two performances: the reed's strikes
  were -4.2 dB re peak apart at 1-frame calls, the grands -30 to -45. Every
  voice now keeps its own 128-frame control block counted from its last
  event (each family's `Ctl`, the fretboard's `BoardCtl`), a grand's
  contact is solved for its whole block across calls, and the harp is paid
  on its own grid. At calls of 1 to 1024 frames against 128, EP Mk.1, EP
  Suitcase, Clav, Reed 200 and Nylon Guitar render the same to the bit;
  Clav Wah is within -100.8 dB re peak, Reed Amp -87.1 and the electric
  grand -91.1. The grand's voice is the same to the bit; through the frame
  the grands read -74 dB, which is the harp's glide and the frame's work at
  the end of a call (below).
- **The frame's edges**, from the reviews of the integrated frame:
  `all_notes_off` fades the shared side (harp, board, open strings, body
  air) over 10 ms instead of cutting it, fades it back in after its clear,
  chokes the guitar's strings, and stays off when the pedal moves after it.
  The host repeats it every block of a quiet lane, and a repeat with nothing
  struck since costs nothing more than the first. After it the harp's and
  the guitar's open strings hear nothing until a key goes down, and each
  sleeps once its output stays under -146 dBFS RMS, so under the pedal the
  guitar is asleep half a second after a stop where it rang on for 7 to
  10 s. Every reset the frame makes renders as a new component would, after
  any history; a knob moved in silence is heard at its new value from the
  next note; the three harp patches drop keys outside A0..C8; a NaN
  velocity is a silent touch; and
  eight corpses fade a chord that steals eight voices in one block.

### Cost

keysrig's `stress` score: 64 keys struck 20 ms apart under the pedal, 7.4 s of
audio. Render time, the median of seven rounds interleaved with main's build
in the same minute, on the owner's machine (load 1.8):

| slot | sound | main | now |
|---|---|---|---|
| 0 | Intimate Grand | 308 ms, 24x real time | 413 ms, 18x |
| 1 | EP Mk.1 | 214 ms, 35x | 187 ms, 40x |
| 2 | Concert Grand | 370 ms, 20x | 373 ms, 20x |
| 3 | EP Suitcase | 214 ms, 35x | 222 ms, 33x |
| 4 | Clav | 122 ms, 61x | 205 ms, 36x |
| 5 | Clav Wah | 129 ms, 57x | 209 ms, 35x |
| 6 | Reed 200 | 119 ms, 62x | 169 ms, 44x |
| 7 | Reed Amp (Reed 140 on main) | 118 ms, 63x | 178 ms, 42x |
| 8 | Electric Grand | new | 292 ms, 25x |
| 9 | Nylon Guitar | new | 111 ms, 67x |

The Intimate Grand is the one under the 25x floor above, and under the 20x
the modal brief accepted: its harp carries the intimate's nineteen aliquots
over the 88 strings, and each voice is a 768-mode bank. At 18x a full
pedalled texture of 64 voices takes 5.5 % of one core. The guitar takes the
score's keys on the fingerboard on six strings, a new note on a string
stopping the one there, and every key off it on a string of its own: 35 ms
(213x) when it dropped those, 111 ms (67x) since it plays every key
(5.1.0, three rounds at load 6 to 7, so if anything high).

`Keys::new` takes 22.5 MB of heap (22,481,524 bytes; main's took 5.7 MB), in
11 to 13 ms on the UI thread (main's 1 ms), and nothing after it: no patch
allocates on the audio thread (`no_keys_patch_allocates_after_new`). Most
of it is mode banks: every voice slot carries a full 768-mode bank for each
of the three modal families though only one plays at a time (gap 10), and
both grands' strings are solved ahead of any strike.

### Levels

The two grands, the electric grand and the guitar were each set by their
loudest playing to peak a few dB under full scale, where the tine and reed
pianos and the clavs had been voiced up to it, so at forte the four sat 7 to
8 dB under the other six and the ten spanned 15.7 dB on the pedalled
voicings. For 5.1.0 the frame lifts those four by 7 dB at its output
(`Patch::lift`), after everything the instrument does, so that nothing it
measures of itself (its culls, its sleep, the harp's) moves; the other six
are untouched, to the bit. Against the reference, at the same velocities,
on its two listening scores (loudness the power mean over the 300 ms
windows within 40 dB of a take's loudest, both channels, so the gaps
between phrases do not count):

| slot | sound | against the reference, evans / comp (dB) |
|---|---|---|
| 0 | Intimate Grand | -4.0 / -3.6 |
| 1 | EP Mk.1 | -1.8 / -0.3 |
| 2 | Concert Grand | -1.2 / -1.5 |
| 3 | EP Suitcase | -5.5 / -4.0 |
| 4 | Clav | -1.8 / -3.2 |
| 5 | Clav Wah | -1.7 / -2.9 |
| 6 | Reed 200 | -1.3 / -1.4 |
| 7 | Reed Amp | -5.7 / -3.2 |
| 8 | Electric Grand | -4.4 / -5.8 |
| 9 | Nylon Guitar | -0.3 / +0.1 |

From the loudest of the ten to the quietest is now 4.6 dB on the comping
(`comp`; EP Mk.1 to Clav Wah) where the reference spreads 5.7, and 8.7 on
the pedalled voicings (`evans`; EP Mk.1 to the Concert Grand) against its
11.8. A six-note chord at MIDI 100 lands all ten within 2.7 dB
(`the_ten_sounds_sit_together_at_forte`). Under forte the grands and the
guitar still fall away faster than the electric pianos, as the reference's
do (gap 1).

What it costs in headroom: the comping take peaks at -2.6 dBFS on the
Intimate Grand, -1.9 on the Concert and the electric grand, -2.5 on the
guitar, where EP Mk.1's peaks at -1.1 and the suitcase's at +0.2. Ten keys
at MIDI 127 from C3 to C6 under the pedal pass full scale on all ten: +1.9
dBFS on the Intimate Grand, +2.9 on
the Concert, +1.6 on the electric grand and the guitar, against +4.2 on EP
Mk.1, +3.5 on the suitcase, +2.1 on the clav, +1.7 on the wah, +4.8 on Reed
200 and +5.6 on Reed Amp (main's peaked at +5.5 to +9.2 on that chord). The
reference's own comping renders stop at 0 dBFS. Against main, the Intimate
Grand plays 1.7 and 3.7 dB louder (evans / comp) and 7 at fortissimo (a
fortissimo C4's first 300 ms -17.6 dBFS, where the waveguide piano's was
-24.6); the Concert Grand 0.7 quieter on `evans`, 2.2 louder on `comp` and
7 at fortissimo; the suitcase is 4.3 and 2.3 dB quieter, the clavs 3.4 to
8.7 louder, and EP Mk.1 and the reed pianos within 2.5 dB either way. The
study's listening pairs are loudness-matched, so what they compare is tone.

### Known gaps, most audible first

Each is measured against the reference on the same score, by the same code;
the lane reports in the study folder have the tables.

1. **Levels under forte.** The lift puts the ten within 4.6 dB on the
   comping, but the grands and the guitar fall away faster with velocity
   than the electric pianos: on the pedalled voicings they span 8.7 dB, the
   Concert Grand the quietest (the reference's own ten span 11.8).
2. **The Intimate Grand's tone across velocity.** Its pianissimo is still 3 to
   11 dB too bright at C5, and its bass and tenor fortissimo 8 to 16 dB too
   dark at 4 to 8 kHz. Its C7 fortissimo tail stands 15 to 20 dB over the
   reference's at 1.2 to 1.4 s, and its C8 holds its tone where the
   reference's drops 19 dB in its first 60 ms (the top row fails at -26.2
   against -32.5).
3. **The grands' half pedal and una corda.** CC 64 at 64 misses its row at
   +1 s, and the una corda's C4 hold and partial changes still fail; the
   concert's C4 unison and aftersound, its treble image (median IPD 34
   degrees) and its bass n2-n1 at E1 and E2 do too.
4. **The harp.** Under a dense pedalled bass its 31 to 250 Hz octaves sit 23
   to 30 dB under the voices, where the reference's sit 4 to 18 dB under; a
   bass string damped by a pedal-up key-off has almost no tail (C2's needs a
   shelf in the damped loop, about one multiply-add per loop sample); the
   concert's silent E4, G4 and G5 answer 11 to 15 dB over; the intimate's
   aliquots in the middle are 5 to 14 dB under. On the electric grand the
   mid and treble rings are 12 to 19 dB under from C4 to C5, because the
   reference's ring grows through the hold and this one holds steady.
5. **The guitar's first half second.** With the open strings ringing, 13 of
   19 fundamentals decay within 30 % of the reference's (E3 9.3 s against
   23.0, B3 5.0 against 9.7): a one-way drive cannot slow a played partial.
   The 222 to 249 Hz band rings 5.0 s against 10.4; three of five phantoms
   on E2 at MIDI 127 sit in their band; there is no damper-noise burst.
6. **The reed's E4 to G4 tail** is 8.7 to 10.4 dB louder than the
   reference's at 2 to 3 s, where the reference's decay speeds up and then
   beats; a sum of two decays cannot make that. The amp's room is 22 to 53
   % short above 1 kHz, and the reed piano has no sympathetic resonance.
7. **The clav's attack.** The upper register lacks early top end (C4 v127
   in its first 12 ms: -28.5 against -19.3 dB), C2's above 2.8 kHz is
   under, and above 9 kHz it is over; C4 H3 at v70 is 5.5 dB hot and the
   anti-phase pickup's n7 19.9 dB over. G1 still blooms 2 dB at 1 s. The
   wah's late centres match on 20 to 22 of 27 notes.
8. **The tine pianos.** Mk.1's upper harmonics are a structural limit of the
   pickup curve (a new pickup model and a refit of both patches); the
   suitcase's C4 H6 falls into a null near v110 to v115 (-52 against -36
   dB); the knock runs up to 3.4 dB hot from v40 to v90; the deep bass (F1
   and below) is about 9 dB short on H3.
9. **The electric grand's spectrum.** C4 at MIDI 70 (n3) and its odd
   partials n7 to n11 are about 10 dB short; C2's n16 to n36 are 10 to 20
   dB short and its fortissimo above 3 kHz 14 dB; the reference's onset
   glide is not built.
10. **Memory.** Every voice slot carries a full 768-mode bank for each of
    the three modal families though only one plays at a time (above).
11. **What a host's call size still moves.** Through the frame the two
    grands are -74 dB re peak off the 128-frame take at small calls, three
    places where a step lands at the end of a call rather than on a grid:
    the harp's glide steps once a run, the frame falls silent at the end of
    a call, and a finished voice's harp string is handed back there. With
    STRETCH moving under a pedalled chord the grands read -61 dB, and
    silent presses under a pedalled C2 -44 at 1024-frame calls (no app path
    sends a velocity-0 note-on). The electric grand's velocity ladder reads
    -68 dB at 4096-frame calls, and its velocity and knob extremes -60 at
    1024. Each is a step landing up to a call later in an otherwise whole
    take, not a sound of its own.
12. **What the reworked frame costs.** A STRETCH drag under a pedalled chord
    costs more than it did (the Intimate Grand at 128 frames 73-78 us a block
    to 111-114; the electric grand 23 to 43-46), as does the first landing of
    a copied voice after the string table has moved off its knobs (1.7 to 2.3
    times the old one). A lane that has never rendered pays 424 us a block
    (4 %) at 512-frame buffers for about 11 blocks, where it paid 82 us for 44
    or more; on a project load with many unarmed pianos that adds up across
    lanes. The clavs cost 2 to 6 % more than before the frame's grids.
13. **The harp keeps the Intimate Grand awake after a pedal lift.** Its damped
    tail runs down to the -146 dBFS floor, 2.7 s after the lift, and the lane
    is not idle until it does (CPU only; nothing over the floor is heard).
14. **STRETCH's top.** At the knob's 1 C8 sits 128 cents over equal
    temperament, and the bass's 8th partial on the harp 10.5 to 12.7 cents
    off its voice's (one all-pass per string is the limit).
15. **By decision:** a key struck in the 10 ms of the stop's fade keeps
    some but not all of its shared side (its step is held at 1.2 times a
    fresh strike's); after a second stop in a row the board and the harp
    stay muted until the choked voices finish (up to 1.7 s on the electric
    grand), until a key is struck; and keys outside A0..C8 are silent on
    the two pianos and the electric grand.

## The bench

`tangent/src/keys/` is pure `std` and depends on nothing, exactly as
`tangent/src/dx7/` does. That is not an accident of style: a new crate in the
app's lock file costs a THIRD-PARTY-LICENSES byte-diff at release, so the DSP
pays for itself by importing nothing.

It also means the module can be compiled **on its own**, which is the only way
anyone has ever tuned an instrument model. A bench crate includes
`tangent/src/keys/mod.rs` by `#[path]` — one copy of the source, no chance of
the bench and the app drifting — and builds in under a second instead of the
minutes the app takes:

    cargo run --release -- piano evans out.wav    # pedalled voicings
    cargo run --release -- ep    velramp v.wav    # 12 strikes, vel 8..127
    cargo run --release -- ep    report           # the measurement tables
    cargo run --release -- guitar stress /dev/null # the CPU check

The ten sounds are `piano ep piano2 suitcase clav clavwah reed reedamp egrand
guitar`, in slot order. It renders in 128-frame blocks, the same size the
audio callback uses, so a bug that only happens at a block boundary happens
on the bench too.

### Measure, do not opine

`report` prints per-strike **peak level, bell-to-fundamental, HF ratio and
waveform asymmetry**, and per-partial frequency, level and T60.

Two of those choices are load-bearing:

- **HF ratio, not spectral centroid.** The centroid is the obvious choice and
  it is useless: the fundamental carries so much of the power that deleting a
  whole upper partial moves it a few percent. This is the same lesson the DX7
  bank learned in `docs/DX7-BANK-01.md`.
- **Bell-to-fundamental, not HF ratio, for the velocity question.** Even the
  HF ratio is dominated by the fundamental. Measuring the bright partials
  *relative to* the fundamental, by Goertzel at each predicted frequency, is
  what shows a 30 dB swing as 30 dB.

Three real defects were found by that table and by nothing else: a resonator
bank with 10^5 of gain at high Q (+90 dBFS output), a compensated pickup gain
that **inverted** once its clamp engaged, so full-velocity notes came out
quieter than velocity 107, and an over-driven nonlinearity reaching an
asymmetry of 0.995 — a one-sided spike, which reads to the ear as clipping
rather than as an instrument being leant on. None of the three is visible in
the source; all three are obvious in one column of numbers.

## Integration

The sentinel rides a bay through the picker, the settings and the saved
session without any of those layers learning what it is — the trick
`BUILTIN_PATH` documents, and `is_builtin()` is one `starts_with('@')`.

The engine holds the keyboards **per lane**: every strip whose first bay
names the sentinel owns a full `Keys` instance, the way every drum lane owns
its kit. That is the second half of an argument that started with "a session
can hold a DX7 track *and* a piano track" — it now holds a piano track and an
EP track, or several of each, because the amount-limit on the in-house
instruments died. Instances are built and freed on the UI thread and travel
by the kit's swap-ring protocol, since the callback may neither allocate a
synth nor drop 22 MB of mode banks. A lane with no instrument costs a
`None`.

Two things that must stay true:

- **`Shared::keys_strips` is a mask from the first line, not an index.** The
  DX7 and the drumpad both shipped as a single `.find()` and both had to be
  widened after a duplicated track took MIDI and made no sound; the keyboard
  went one further and gave every masked lane its own instance, because a
  shared instrument behind a mask made the second track silent by design and
  a per-track patch inexpressible.
- **The export walk sets it too.** An instrument wired into the live engine
  and forgotten in the bounce is the worst kind of export bug: everything
  sounds right while it is being played and the rendered file is missing a
  track.

CC 64, 66 and 67 reach the model, as levels, every block. A modelled piano
without a damper pedal loses its pedalled sustain and its sympathetic wash,
which is half of what makes a jazz voicing hang together; the grands also
half-pedal, hold the sostenuto's keys and shift for the una corda.

## Naming

The row's name is Tangent's and the reference goes in the description — the
same split `dialogs::FX_UNITS` has made since the phaser, and LICENSING.md
says why. "Rhodes", "Fender Rhodes" and "Mark I" are live trademarks and
appear nowhere in the product, the patch names, or the UI. "EP Mk.1" is
Tangent's name for a Tangent instrument; what it is modelled on is a fact
about the model and belongs in prose like this.

The same holds for every sound since: "Intimate Grand", "Concert Grand",
"Clav", "Reed 200", "Reed Amp", "Electric Grand" and "Nylon Guitar" are
Tangent's names (the first two were "Piano 1" and "Piano 2" until 5.1.0; a
saved strip keeps its sound, since the slot is stored by number), and the
instruments they were voiced against are named in no patch, description or
comment. The code and its comments call the measured instrument "the
reference".
