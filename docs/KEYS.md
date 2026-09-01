# Tangent Keys — a piano and an electric piano, computed

Built 2026-09-01 against `main` at "Pertinent entries, taken slowly"
(`ea77eeb`, 5.0.0-dev). Written for whoever changes the physics next.

## What it is

A third built-in instrument beside Tangent DX7 and Tangent Drums, on the
sentinel `@tangent-keys`. It contains **no samples**. Every note is computed
from a model of the mechanism, which is why it ships two convincing
instruments in a few hundred lines and no data at all:

- **Piano 1** — a jazz grand, voiced for solo playing.
- **EP Mk.1** — a tine electric piano, taken dry. Bell-like when it is played
  lightly and it barks when it is not.

Load it in any MIDI channel's first bay, exactly as the DX7 loads — in as
many bays as you like, because every lane owns a full instance of the
instrument: a piano track and an EP track in one session, or eight of either,
each with its own settings. Clicking the bay opens a window with the two
sounds and two knobs, which is the whole interface. The first cut had no
knobs at all, on the argument that every control on a physical model is a way
to make the instrument worse; the two that ship are the two that survive that
argument. **Voicing** scales the hammer's contact the way felt does — needle
it mellow or lacquer it bright, with the centre being exactly the tuning the
bench signed off. **Width** is where the strings sit in the stereo field,
from a point source to a little wider than the stock seating. Both are per
strip, saved with the session, and neither is a filter over the output.

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

It renders in 128-frame blocks, the same size the audio callback uses, so a
bug that only happens at a block boundary happens on the bench too.

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
synth nor drop 3 MB of delay lines. A lane with no instrument costs a `None`.

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

CC 64 reaches the model. A modelled piano without a damper pedal loses its
pedalled sustain and its sympathetic wash, which is half of what makes a jazz
voicing hang together.

## Naming

The row's name is Tangent's and the reference goes in the description — the
same split `dialogs::FX_UNITS` has made since the phaser, and LICENSING.md
says why. "Rhodes", "Fender Rhodes" and "Mark I" are live trademarks and
appear nowhere in the product, the patch names, or the UI. "EP Mk.1" is
Tangent's name for a Tangent instrument; what it is modelled on is a fact
about the model and belongs in prose like this.
