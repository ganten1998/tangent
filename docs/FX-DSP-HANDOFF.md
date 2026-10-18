# The plate, the tape echo and the pedals: what is done, and what is owed

Branch `fx-dsp`, off `46b54c5`. Two files changed, `tangent/src/effects.rs` and
`tangent-ui/src/dialogs.rs`, chosen because they were the only two clean in the
other session's tree. Five commits, +2053/−364.

**Workspace green: 23 suites, 1455 tests, 0 failures. `effects.rs` and the code
added to `dialogs.rs` build without a warning.**

This document is the other half. Everything below lives in files this branch
deliberately did not touch.

---

## 0. The three rules this branch was built under

Worth stating first, because they are why the merge should be uneventful.

1. **`Sends` is frozen.** Not one field added, removed or renamed. The two
   sites in `desktop.rs` that construct it compile untouched. The knobs keep
   meaning what they meant — `reverb_mix` is the plate's send, `delay_mix` is
   the echo's. The DSP behind them changed; the interface did not.

2. **`Params` changes are additive.** Every existing field is still there with
   its old name. Three new ones sit beside them. So `effect_params_from()` and
   `effect_defaults()` compile unchanged, and the new parameters sit at their
   defaults until their keys are wired. **The merge is green on arrival**; the
   feature lights up incrementally rather than in one step that has to land
   whole.

3. **`enum Fx` is untouched.** No variant, no arm, no row. §3 proposes rows;
   it does not write them, because this branch cannot see the ten-variant
   `Fx`/`GRID` layout that exists only in the other tree.

---

## 1. `Params`: three new fields, three changed defaults

### New — additive, currently unreachable from the UI

| Field | Type | Default | Range | What it is |
|---|---|---|---|---|
| `plate_dispersion` | `f32` | `0.75` | 0..=1 | How much of the plate's chirp is in circuit. 1.0 is the sheet of steel; 0 is a tank with no plate physics in it. |
| `plate_predelay` | `f32` | `0.12` | 0..=1 across `PLATE_PREDELAY_MS` (0–150 ms) | Silence in front of the plate. The real one has none; this is the tape machine every studio patched in. 0.12 is 18 ms. |
| `echo_mode` | `EchoMode` | `EchoMode::One` | 12 named positions | Which playback heads are live and whether the spring is in. |

### Reused — same name, and say so out loud

These were **not** silently reinterpreted; each is the same knob doing the same
job under a different machine, and the defaults moved because the machine did.

| Field | Was | Is now | Default was → is |
|---|---|---|---|
| `reverb_size` | comb feedback | plate **Decay**, in seconds across `PLATE_DECAY_S` (0.5–5.5 s) | `0.62` → `0.34` (= 2.2 s) |
| `reverb_damp` | comb damping | plate **Tone** — moves both the corner and how much shorter the top decays | `0.35` → `0.55` |
| `reverb_width` | stereo delay offset | plate **Width** — a mono fold, no longer a buffer length | `0.70` → `0.80` |
| `delay_feedback` | repeats | tape **Intensity**. Ceiling raised `0.72` → `0.90` | `0.42`, unchanged |
| `delay_tone` | loop lowpass | tape playback loss corner | `0.55`, unchanged |
| `delay_width` | right-channel ratio | how far the three heads spread across the pair; 0 is the mono machine | `0.60`, unchanged |
| `delay_division` | repeat time | **head one's** time; heads two and three follow by 1 : 1.94 : 2.85 | unchanged |
| `chorus_depth` | symmetric swing | how far the delay opens **above a fixed 3 ms floor** | unchanged |

Everything else — `chorus_rate`, `chorus_width`, `chorus_tone`, all four
`phaser_*`, both filters, both limiter fields — is untouched in name, meaning
and default.

---

## 2. `desktop.rs`: two functions, and a `use`

Both crossings of the firewall. Missing the second is the silent failure this
codebase already warns about: the knob moves and the audio does not.

### 2.1 `effect_defaults()` — three additions

In the `values` table, beside the other `d.*` rows:

```rust
("plate_dispersion", d.plate_dispersion),
("plate_predelay", d.plate_predelay),
```

In `choices`, beside the `delay_division` entry:

```rust
tangent_ui::ports::ChoiceParam {
    key: "echo_mode".to_owned(),
    options: EchoMode::ALL
        .into_iter()
        .map(|x| (x.key().to_owned(), x.label().to_owned()))
        .collect(),
    default: d.echo_mode.key().to_owned(),
},
```

and widen the existing import to `use crate::effects::{Division, EchoMode,
Params, Sends, Stages};`.

No `units` entry is needed: neither new parameter is a knob, and the eight mix
knobs' units are unchanged.

### 2.2 `effect_params_from()` — three additions

In the `(key, dst)` array:

```rust
("plate_dispersion", &mut p.plate_dispersion),
("plate_predelay", &mut p.plate_predelay),
```

And beside the `delay_division` block:

```rust
if let Some(m) = map
    .get("echo_mode")
    .and_then(serde_json::Value::as_str)
    .and_then(crate::effects::EchoMode::from_key)
{
    p.echo_mode = m;
}
```

**Doing this removes the `#[allow(dead_code)]` on `EchoMode` and its impl.**
That attribute carries a comment naming these two functions; please delete it
rather than leave it, or it will outlive its reason.

---

## 3. `recorder_panel.rs`: subtitles, rows, and one decision

### 3.1 Subtitles — the exact strings

```rust
Fx::Reverb => "legendary plate, after the EMT-140",
Fx::Delay  => "warbling saturation, after the RE-201 Space Echo",
```

Leave the chorus and phaser subtitles alone; they were already right.

### 3.2 Titles — your call

`"REVERB"` and `"DELAY"` still describe what the knobs do, and changing them to
`"PLATE"` and `"ECHO"` would name the effects rather than the category. Either
is defensible. **What must not happen is the mark moving into the title** — see
§5.

### 3.3 Rows — both units want five and `rows()` holds four

```rust
// The plate. Decay is the EMT's own dial, marked in seconds.
Fx::Reverb => [
    slide("reverb_size", "Decay"),
    slide("reverb_damp", "Tone"),
    slide("plate_predelay", "Predelay"),
    slide("reverb_width", "Width"),
],
// The tape echo. Mode is the twelve-way selector and it is the instrument.
Fx::Delay => [
    step("echo_mode", "Mode"),
    step("delay_division", "Time"),
    slide("delay_feedback", "Intensity"),
    slide("delay_tone", "Tone"),
],
```

That fits, and it leaves two parameters at their defaults and unreachable:
`plate_dispersion` and `delay_width`. Both ship at good values, so nothing
sounds wrong — but the plate's dispersion is the single most characteristic
thing about it, and a person who wants less of it currently cannot have less.

Three ways out, in the order I would take them:

1. **Widen `FxRow`'s array to six.** Touches the panel layout and its overlap
   test. Both units then get everything, and the filters and limiter simply
   carry more `FxRow::NONE`.
2. **Drop `Width` from the plate** and give the row to `plate_dispersion`. The
   figure-of-eight tank decorrelates on its own, so width at its default is
   already wide; dispersion has no such fallback, and it is the single most
   characteristic thing about the unit.
3. **Leave it.** Defensible, and it should then be said in a comment so the
   next person does not think the parameters were forgotten.

**Updated after `Fx::A`/`Fx::B` were retired:** `enum Fx` is back to the eight
at `46b54c5`, so the grid is eight cells in two rows and nothing is competing
for the space. That removes the reason I hedged here. With a plain eight-dial
grid I would take (1) — six rows costs one array width and one layout test, and
it is the only option that does not trade a control away. (2) is the right
fallback if the panel geometry fights back.

---

## 4. `is_builtin()`: two sites, and only two

`dialogs::is_builtin(path)` exists and the seven sites inside `dialogs.rs` are
converted. Outside it there are **two** places where the two-way test is now
wrong, both in `desktop.rs`. **Named by function and by the line they contain,
not by line number** — line numbers taken against `46b54c5` will not resolve
against a tree that is moving.

Both sites are the identical two lines:

```rust
let engine_want = want.as_deref().filter(|p| {
    *p != tangent_ui::dialogs::BUILTIN_PATH && *p != tangent_ui::dialogs::DRUMPAD_PATH
});
```

and both want to become:

```rust
let engine_want = want.as_deref().filter(|p| !tangent_ui::dialogs::is_builtin(p));
```

| Function | Where in it | Why it matters |
|---|---|---|
| **`after_frame`** | the `inserts_loaded` walk — right under the comment beginning *"The built-in is not a bundle"*, just before `e.load_insert(i / inserts, i % inserts, path, …)` | **The critical one.** This is the live rack walk across every seat and bay, and it is the path an `@tangent-plate` in an insert bay travels to the VST3 loader. |
| **`start_offline_bounce`** | under the comment `// ── every configured rack seat, saved state included ──`, just before the second `e.load_insert(` | The export mirror of the same walk. |

**Both, or neither.** `start_offline_bounce` carries a comment in the tree
saying it "furnishes a second engine BY HAND and duplicates four things the
live path also does" — converting only the live one would mean an effect you
could hear and could not export, which is precisely the class of quiet
wrongness that comment is warning about.

**Everything else that mentions a sentinel is fine and should be left alone.**
The other eleven ask "is this *the DX7*" or "is this *the drumpad*" — a
different question with a correct answer already. They are the
`set_builtin_strip` / `set_drum_strip` lookups in `after_frame` and
`start_offline_bounce`, the drumpad's bay-1 rule in `pump_midi`'s neighbourhood,
and in `app.rs` the `bay0(*n) == BUILTIN_PATH` searches, the drumpad-live
checks, and the fresh-install assertion. Search for `== …BUILTIN_PATH` and
`== …DRUMPAD_PATH` rather than for line numbers: **equality against one
specific sentinel is always correct; only the two-way `!=` pair is wrong.**

I would still ship the conversion as **its own commit that changes no
behaviour**, then the sentinels on top: the failure mode is silent, and a
bisect that can land on the refactor alone is worth the extra commit.

---

## 5. The line that must not move

Every effect's **name** is Tangent's. The hardware it is voiced after is named
in the line underneath. That is a reference to somebody else's product rather
than a claim about this one, and it is the whole legal position in one
sentence.

`dialogs::FX_UNITS` holds `(path, name, description)` on exactly that split,
and `no_effect_is_named_after_hardware_it_only_refers_to` fails the build if a
name ever picks up `emt`, `roland`, `space echo`, `re-201`, `boss`, `ce-1`,
`mxr`, `phase 90` or `140`. It is a test rather than a comment because a
comment is one careless edit from being gone.

Two things that follow:

- Do not let a mark into `Fx::title()`, a bundle id, a filename, an installer
  string, or a store keyword.
- The subtitles in §3.1 are fine as written. `"after the RE-201 Space Echo"`
  leans on a live Roland trademark harder than the EMT one does; **"after the
  Roland RE-201"** is the more conservative form if that is ever wanted. It is
  a judgement call, not a correction.

Nothing here is sampled from, recorded off, or derived from anybody's code. The
one tempting reference — the Politecnico Wave Digital model of the Phase 90 —
is **CC BY-NC 4.0** and cannot be used in an MIT product. It was read and not
copied.

---

## 6. Two decisions I made that are reversible, and one I did not make

**Feedback ceiling, `TAPE_FEEDBACK` = 1.25. Ruled on: match the hardware.**
This file argued the other way twice — it shipped at 0.72, then at 0.90, both
chosen so every setting kept decaying, on the reasoning that a send across a
master bus is not a place for an oscillator. Ganten's instruction was to match
the hardware, so it does: an RE-201's Intensity reaches self-oscillation and
that howl is on the records rather than being an abuse of the control.

Three things had to move with it, and they are the interesting part:

- **The invariant changed, not just the constant.**
  `the_delay_does_not_run_away_at_any_setting` demanded that the tail decay at
  *every* Intensity, which is right for a delay and forbids a Space Echo. What
  tape actually guarantees is not decay but **boundedness** — it compresses
  rather than diverging. It is `the_echo_stays_bounded_at_every_setting` now:
  finite, under a generous ceiling, in the worst case of all three heads live;
  plus a decay assertion at two thirds of the knob, so it still behaves like a
  delay where somebody looking for repeats would put it.
  `the_intensity_reaches_self_oscillation` covers the capability the old
  assertion used to forbid.
- **1.25, not a hair over 1.0.** Small-signal and large-signal loop gain are
  different numbers: `tanh(1.6a)/1.6a` is already down to 0.97 once the loop
  carries a fifth of full scale, so 1.06 self-limits almost immediately and
  reads as a very long delay. At 1.25 the loop settles where
  `1.25 · tanh(1.6a)/1.6a = 1`, near 0.68 on the tape — loud, squashed and
  stable.
- **The feedback tap is now the head MIX, not the head sum.** Only started
  mattering once Intensity could pass unity: three heads summing raw would give
  mode 11 three times mode 1's loop gain, so one knob setting would be a gentle
  wash on one position of the selector and an instant howl on another. The wet
  output still carries the full sum — three heads *should* be louder, and that
  is a level rather than a loop.

**What bounds it is the `tanh`, not the ceiling on the knob.** Worth knowing
because the master limiter ships bypassed: at the top of the travel this is
genuinely loud, in the same way the gain dial next to it is documented as one
that *can* clip. It cannot diverge.

**Tempo sync survives.** `Division` still sets head one's time. The alternative
— a free-running Repeat Rate, as on the hardware — would have deleted the one
opinion this file states about itself. What is no longer compromised is the
transport: the length slews, so audio already on the tape slides in pitch while
the speed changes, which is what the manual describes.

**One I did not make: the chorus still has separate Rate and Depth.** A real
CE-1's chorus mode has one knob, Intensity, that moves both. Two knobs are more
useful and are what is already wired, so the modulation shape was corrected and
the control layout was left alone. Collapsing them is a panel decision.

---

## 7. What is not here

- **Vibrato mode.** Half the CE-1's name. It needs a choice parameter and a
  row, and the rows are yours.
- **FX actually loadable into a bay.** The sentinels exist and are filtered
  correctly; nothing consumes them yet. That is the per-`(seat, bay)` table in
  `instrument.rs`, load-time allocation on the UI thread, and handover through
  a ring — all in files this branch stayed out of. `PluginBox` does **not**
  need to become an enum: the drumpad's precedent is a sentinel that rides the
  bay while the engine does the work elsewhere, and 444 bays at ~1 MB is why
  allocation must happen on load rather than up front.
- **Per-bay parameters.** `effect_params` is still one global map, so every
  instance of the plate would read the same `reverb_size`. Fine for a first
  cut; the key format wants choosing now so per-bay is a migration and not a
  rewrite.

---

## 8. Three bugs the tests found, kept here because they will be re-derived otherwise

1. **`Plate::feedback` had a factor of two.** `g` is applied per *branch* and a
   full trip round the figure of eight crosses two, so the dial asked for
   2.20 s and the plate rang for 1.09. The exponent is 1.5, not 3.
2. **`Effects::process` only flushed on a call that *began* at rest.** A single
   long call spanning the whole fade-out reached zero after the loop and left
   the tail standing, so the promise in that comment — nothing rings on from an
   hour ago — was only kept when the knob happened to reach zero between calls.
   Invisible with the old short room; a plate comes back under the next note.
3. **The chorus gate's envelope and its slew were one constant.** The envelope
   had to fall seven time constants past the threshold before the gate began to
   move — nearly two seconds of hiss after the last note.

And one test bug worth the same treatment: `the_tape_warbles` first measured an
envelope. Moving a delay line moves *pitch* and leaves amplitude alone, so it
read 0.41497 against 0.41498 and declared the tape perfect. It times zero
crossings against a regular grid now.
