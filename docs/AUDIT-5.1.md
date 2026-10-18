# Tangent 5.1 Readiness Audit

2026-08-30 · repo at `36b1f56` (5.0.0-beta.15) · successor to `docs/AUDIT-5.0.md` (`488bbdd`, v4.92.0)
· 28-agent audit: 9 code sweeps, 9 adversarial refutations, a 4-level programmer panel, 4 judgement
passes, 2 critics

> Owner's questions: (1) how far has it come, per gap · (2) what do we do well now · (3) who do we
> appeal to now · (4) implications for marketing and version numbering · (5) is it visually
> distinct · (6) is the feature set competitive · (7) what would programmers of different levels
> think, before and after seeing the code

**Method note.** Every verdict below was checked against the code at `36b1f56`, not against the
previous audit's claims and not against commit messages. Optimistic verdicts were then attacked by
a second agent whose job was to refute them; 23 verdicts were downgraded in that pass, and those
downgrades are reflected here. Where two agents disagreed, the disagreement was resolved by hand.
Numbers that did not reproduce were corrected. Section 10 lists what to distrust.

---

## Executive summary

**The engineering since the last audit is real and the gap list is mostly closed. The release
machinery is in worse shape than the last audit found it, and the product's public face now
describes a product that no longer exists.** In one screen:

- **The distance travelled is genuine.** Of the eight losses the 5.0 audit named, five are closed
  or effectively closed: automation (four modes, plugin parameters, master, folder-aware),
  plugin delay compensation (folder-aware, both plugin formats queried), folder buses that are
  real latency-correct buses, real routing, and a tempo map with closed-form ritardando integrals
  that every consumer actually reads. Two are partly closed (audio time-stretch, notation), one is
  untouched (comping).
- **But a closed gap is not a competitive advantage.** Automation, PDC and groups are table stakes:
  they remove an objection, they do not win a category. Not one competitive rank moved. Marketing
  them as wins invites the comparison Tangent loses.
- **What is genuinely excellent is not a DAW feature.** The honest take manifest, the correctable
  chord detector, the DSP, the contrast machinery, the FreeBSD native OSS path, and the keybind
  system. Those are the wins, and none of them appears on a feature grid.
- **Three things ship broken or false right now.** A truncated or corrupt project file opens as a
  silent empty timeline that is not marked dirty, and four separate code paths then write the
  document to disk without asking, so "nothing saves itself" is false at HEAD. A plugin in the
  master's second rack is silent in playback and present in the export. Windows users are told a
  performance claim that is not true.
- **A fourth is real but one-distro.** The Linux binary hard-links `liblilv-0.so.0` and ships no
  copy. On Debian, Fedora, Arch, Void, openSUSE, Mint, Pop and every Ubuntu *flavour* that library
  arrives with the audio or media stack, so nothing is wrong. It is absent on **stock Ubuntu
  GNOME**, where the app cannot start at all. See B1 for the correction: an earlier draft of this
  audit called it a Linux-wide failure, and that was wrong.
- **5.0 cannot currently be published by the release script**, because it refuses to publish
  without release notes and the CHANGELOG stops at 4.5.0.
- **The beachhead has not moved.** Self-learners remain correct. Teachers stay closed: AccessKit is
  in the tree and the keyboard nudge is genuinely good, but macOS never detects a screen reader and
  the only door is a right-click row, so the answer to "is this Section 508 defensible today" is no.
- **Visually it is distinct where nobody has photographed it** (the recorder band, the theory band,
  the Tonnetz wordmark) and conventional where a DAW is judged (mixer, arrangement). All four
  README screenshots predate the entire 5.0 batch.
- **On the programmer question**, the panel splits, and not the way one would guess: junior 7 to 8,
  **mid 7 to 6 (the only one that falls)**, senior 5 to 7, staff 5 to 6. All four converge on one
  finding: the rigour is real and it is *unenforced*. There is no CI, no hooks, and no script runs
  the tests. The verdict is not "over-engineered"; it is that the discipline is a property of a
  person rather than of the repository, and it has already begun to rot exactly where no machine
  was watching.

---

## 1. The distance travelled, per gap

### 1.1 Where the 5.0 audit said Tangent lost

| # | The 5.0 audit's loss | Status | What the code says now |
|---|---|---|---|
| 1 | **No automation lanes.** "Disqualifying for serious mixing" | **CLOSED** | `automation.rs`, 677 lines. Parameters are Gain/Pan/Send/Mute plus `Plugin{bay,id}`, addressed by the plugin's *own* parameter id so ids survive a plugin update. Four real modes (Read/Touch/Latch/Write), deliberately never persisted, because "a lane left in Write and saved would overwrite its own curve the next time anybody pressed play". Folder-aware, master-aware, and the offline render drives the same `Renderer::render`. |
| 2 | **No plugin-delay compensation.** "Any lookahead plugin silently smears the mix" | **CLOSED** | VST3 `kLatencyChanged` and LV2 latency ports both queried. `MAX_PDC_FRAMES = 8_192`, pre-allocated whole, never resized on the audio thread. Polling is event-shaped, not a timer. **Folder-aware:** alignment targets `lane latency + folder_lat`, deliberately excluding the master. A plugin wanting more than the ceiling is *reported*, not silently half-compensated. |
| 3 | **No audio time-stretch** | **PARTLY** | `stretch.rs`, 1,046 lines, three hand-written algorithms (Hermite tape, WSOLA, phase vocoder with peak locking and its own radix-2 FFT), auto-selected by measured percussiveness. **The hole: the offline bounce ignores it.** `start_offline_bounce` never applies `clip.stretch`, so an export renders a different record from the one being listened to. |
| 4 | **No comping** | **UNTOUCHED, and deliberate** | Zero take-lane infrastructure. `ArrClip` has no take-group or alternate field. Glue is MIDI-only. **The owner states this is a design decision, not a gap** — and the rationale exists, stranded in `docs/RECORDER-PLAN.md:1996` under "Explicitly NOT in v1": *"Editing, trimming, or any timeline. The take is what was played."* The first clause is now dead (there is a timeline). The second is the position, one sentence from publishable. See §1.5. |
| 5 | **No tempo track, no time-signature changes** | **CLOSED / PARTLY** | `TempoMap` with **closed-form ramp integrals** and an exact inverse: a real ritardando, not a stepped loop. Every consumer verified one at a time: ruler, grid, snap, click, transport readout, export. `MeterMap` is equally done, **but the UI door to create the first signature change does not exist**: the meter strip is `Rect::NOTHING` while the map is constant, so it is reachable only by hand-editing JSON. MIDI file export still flattens tempo to one value at tick 0. |
| 6 | **Worst-in-field extensibility** | **PARTLY** | LV2 hosting is real (audio, MIDI atoms, parameters, state, latency, worker threads, X11 editors), **on Linux and FreeBSD only**. macOS and Windows remain VST3-only, and that is where the paying audience is. Inserts still 3 per channel. CLAP/AU/VST2/ARA/scripting: zero code, zero groundwork. |
| 7 | **One send to one bus; no groups** | **CLOSED** | A routing table (hardware outs and channel-to-channel, ordered by Kahn's, cycles degrade to one hop rather than howling), plus folder buses that are real: a folder runs its own rack over the summed children, nests, and inherits mute and solo through the tree. |
| 8 | **Accessibility is literally zero** | **PARTLY** | AccessKit is in and enabled; `a11y.rs` lays invisible focusable widgets over painted surfaces on four panels; the keyboard nudge routes through each panel's *own* hit-test so keyboard and mouse cannot drift. The window is resizable. **But macOS never auto-detects a screen reader** and the only door is a right-click row with no keybind. |
| 9 | **Notation displays, does not edit** | **PARTLY** | The staff is now playable: `hit_test` inverts clef position and spells by key signature, so clicking the F line in D major yields F sharp. That is chord entry, not notation editing; the staff still takes a set of pitch classes with no time, duration or voice. |
| 10 | **16-channel ceiling** | **UNTOUCHED, and deliberate** | Still 16, and documented as a decision at `recorder.rs:562`: *"Fixed, like everything about the desk's shape: an app whose strips appear and disappear is a desk that changes shape under your hands."* What grew is beneath a channel. Note a folder **consumes one of the 16**, and `make_folder` fails silently when none is spare — that part is a bug, not a design. |
| 11 | **No surround** | **UNTOUCHED** | Stereo throughout. |
| 12 | **Ecosystem and longevity: last, decisively** | **UNTOUCHED, and structurally worse** | 563 commits, one author, 33 days. No CI, no hooks, and no build or release script runs the tests. |

### 1.2 The 5.0 audit's "genuinely excellent" list, re-checked

Eight of nine hold with fresh citations. One regressed:

**Footprint.** "16 to 18 MB installers, within rounding error of REAPER" is no longer true off
macOS. Measured from `dist/` at HEAD: macOS dmg **21.4 MB** (zip 19.9 MB), Windows setup
**47.3 MB**, Linux x86_64 **42.1 MB** (105 MB unpacked), Linux aarch64 **37.8 MB**. Two thirds of
the Windows and Linux payload is the bundled GPL ffmpeg. The REAPER comparison survives only in the
macOS column.

One item on that list must also be **withdrawn**, and it is the one this audit was closest to
repeating uncritically. See §8.1, finding B6: "nothing saves itself" is false at HEAD.

### 1.3 The six ship-blockers

| # | Blocker | Status |
|---|---|---|
| 1 | Versionless Windows zip permalinks 404 | **OPEN, and now guaranteed.** The build *deletes* the zip as a transient installer input, so the alias is never uploaded. No beta has exercised this path; it lands on the first stable publish. |
| 2 | Linux tarball missing `install.sh` and ffmpeg | **CLOSED**, but it landed *in the audit commit itself*, not in the 170 commits since. |
| 3 | `Tangent-macos.pkg` built by no script in the release path | **OPEN.** The pkg builder exists and is good; nothing calls it. The supporter email hardcodes that link. |
| 4 | Windows `setup.exe` never Authenticode-signed | **OPEN.** The signing function exists and signs the *inner* binary; the file a buyer double-clicks is signed by nothing. |
| 5 | Installer UI, emails and store copy still sell the retired VST3 plugin | **CLOSED** 2026-10-10, by the owner's ruling "remove those mentions of the vst plugin -- that's what it means to be dead". The installers' pages, scripts and components, the Linux `install.sh`, the store copy, `LICENSING.md` and the app's own strings no longer offer, describe or license one; what stays is the cleanup that removes an old install's `Tangent.vst3`. The `plugin/` crate went too, on the owner's second ruling the same day ("yes, delete the plugin crate too"): the crate, `scripts/build-plugin.sh`, `scripts/verify-plugin.c`, `LICENSE-GPL-3.0` and every gate leg that built or checked it were removed on `lane/ul-w8-plugin-gone`. |
| 6 | `release.sh` cannot catch #2 | **CLOSED**, also in the audit commit. |

Two of the six were therefore already fixed when the audit was written, and **none of the remaining
four moved in 170 commits.**

### 1.4 Licensing homework

| Item | Status |
|---|---|
| Host the ffmpeg Corresponding Source | **OPEN.** A pointer to upstream is not the corresponding source of *that* build. The Gumroad copy contains no licence notice of any kind. |
| LAME / LGPL attribution | **OPEN, and worse than described.** `THIRD-PARTY-LICENSES` is 12 days and ~170 commits stale and omits **41 shipped crates**. Correction to the 5.0 audit: LAME is statically linked on **macOS and FreeBSD**, not Linux; the cross build drops it. Those are the artifacts carrying the obligation. |
| `TRADEMARK.md` | **OPEN.** No trademark claim anywhere, and `docs/RELEASE.md`'s hard gate still asks about the *old* name's collision. |
| Ed25519 seeds unbacked-up | **THE CLAIM WAS WRONG.** A backup exists and was verified to be the real minting key. But it lives only in Dropbox, which violates the keygen tool's own written threat model ("the repo is inside Dropbox, so anything under it is uploaded to a third party"), the folder still carries the old name, and the sale ledger has been unbacked since 11 August. The honest framing is leak exposure, not loss exposure. |

### 1.5 Deliberate constraint or unfinished work? The distinction is worth money.

The owner has confirmed that the absence of comping and the 16-channel cap are **design decisions,
not gaps**. That is right, and this audit's first draft was wrong to file them as holes to close.
But it changes less than it might appear, and it changes one thing a great deal.

**What it does not change: the capability gap.** A reviewer comparing Tangent to REAPER or Ardour
writes an X in the comping cell either way, and a buyer filtering on "must have comping" filters
Tangent out either way. Intent does not fill a cell on a feature grid, so all of these stay on the
competitive-loss list in §6 and should stay scored there.

**What it does change: the trust risk, and for an unknown beta by a solo author that is the larger
of the two effects.** One *unexplained* gap discounts everything a reader has not personally
verified: "if comping is not there, what else is unfinished, and will it drop my take?" One
*explained* gap discounts only itself. The reframing to adopt is therefore two columns rather than
one:

| Constraint | Capability gap | Trust risk | Where the rationale lives |
|---|---|---|---|
| 3 inserts per channel | yes | **removed** | `recorder.rs:596`, and it is publishable verbatim |
| 16 channels | yes | **removed** | `recorder.rs:562`, publishable with one sentence added |
| No comping | yes | **currently unmitigated** | stranded in `RECORDER-PLAN.md:1996`, half of it now false |
| No surround, no MIDI out, no scripting, no ARA | yes | unmitigated | nowhere |

The insert rationale is the model, because it names what is *gained* rather than what is missing:

> *"Three, on every channel, and the number never changes… It is also artistically limiting on
> purpose — this started as a MIDI monitor, and a rack that grows until the machine gives out is a
> different program."*

The 16-channel note explains the *fixedness* but not the *number*, so a reader still asks "why not
24 fixed channels?" One added sentence closes it.

**Comping is the one to actually write down**, and it is harder to defend in kind than the other
two: sixteen channels is a shape-of-the-desk argument, comping is a workflow argument, and workflow
arguments invite "then your workflow is wrong for me." That is precisely why it must be written
before launch rather than improvised in a public thread, because an improvised rationale for a
missing feature is indistinguishable from a rationalisation. The material already exists in the
owner's own words: *"The take is what was played."* Lift it to `recorder.rs` beside the other two
shape constraints, drop the dead clause about there being no timeline, and it is a position.

---

## 2. What Tangent does well now

The distinction that matters for 5.0 copy is between a **category win** (better than the
alternatives) and a **closed gap** (an objection removed). Most of the 170 commits bought the
second kind. That was the right work. It must not be sold as the first kind.

### 2.1 Genuine category wins

1. **One-gesture audio, MIDI and video takes with a manifest that cannot lie.** Sample 0, tick 0
   and frame 0 are the same instant, and the manifest says so in the file. It is written
   `complete: false` at take start as a crash detector, and carries the counts a sync complaint
   cannot be debugged without. Four irrecoverable fields were *added* since the last audit,
   including what the take was heard muted and soloed against. One function is deliberately the
   only writer of the clock fields, so the reported clock and the measured error cannot disagree.
   *Better because:* no competitor captures video at all.

2. **The correctable chord detector.** 95 patterns against 12 roots, runners-up named, user
   overrides with a learning re-ranker, held by an acceptance contract and a 13,133-row
   differential corpus against a frozen baseline. *Better because:* Logic's Chord Track is offline
   per region and Ableton names nothing. Nobody else ships a detector a user can correct.

3. **The DSP is genuinely good, well above hobbyist grade.** This surprised the sweep that went
   looking to disprove it. Biquads are the RBJ cookbook in transposed direct form II with correct
   normalisation; the 24 dB cascade uses the right Butterworth Qs; the one-pole uses the exact
   coefficient rather than the approximation "because this knob goes to 20 kHz where the two stop
   agreeing". Denormal flushing is systematic at every state store in every feedback path. The
   plate reverb is physics rather than a preset: a twelve-stage allpass cascade with geometrically
   staggered negative coefficients, because a real plate's group delay goes as the inverse root of
   frequency. The decay dial is solved, not tuned. Filter slopes are verified by *measuring* the
   response against the Butterworth each name promises.

4. **The FreeBSD native OSS path.** 36.7 ms out of the box becomes **8.1 ms**, against 27.5 ms
   through the alsa-lib shim and 11.2 ms for Ardour through JACK, and Ardour was *checked* rather
   than assumed. The cpal patch is 54 diff lines outside a new module, every functional line behind
   a FreeBSD cfg, so every other platform compiles byte-identical upstream code. *Better because:*
   no other DAW ships a native low-latency FreeBSD path. Small audience, unambiguous first.

5. **The keybind system's architectural honesty.** One table; the help card is rendered *from* it;
   `available()` gates both the key and its card row, so "listed but dead" and "live but unlisted"
   are the same impossible thing. It got stronger: a test now reads `app.rs` back and asserts every
   hand-written card row names a key the handler actually reads. "Prose drifts. So the prose is read
   back against the handler."

6. **The contrast machinery.** Ink is a property of the surface and is never inherited from a
   themed role; where neither ink can clear 4.5:1, the *surface* moves instead. Brute-forced over
   636,056 colours: zero outputs below 4.5:1, worst case exactly 4.500. Twelve test loops walk all
   16 themes in both modes. *Better because:* this is a WCAG floor enforced by construction across
   a fully themeable DAW, and it is simultaneously a design asset and an accessibility asset.

7. **Error-surfacing as a designed chain**, reordered on evidence (the yank test proved the old
   order backwards), written per-platform "because naming the wrong panel is worse than naming
   none". Craft, not a feature. Sell it as reliability, not as a bullet.

8. **Offline, no account, no telemetry, no subscription**, with supporter keys verified offline
   against embedded public keys. In a market that made Waves reverse a subscription in five days,
   this is itself positioning.

### 2.2 Closed gaps: real work, but table stakes

Do not market these as wins. They remove objections.

| Capability | Honest standing |
|---|---|
| Automation, four modes, plugin parameters, master | Level with Ardour; **behind** Live and Logic on curve shapes and lane workflow. |
| Plugin delay compensation | Table stakes, **except** that it is folder-aware on the first implementation, which REAPER took years to get right and which Live still resolves by giving up in some configurations. |
| Folder buses | Table stakes, with the same exception: latency-correct from the start. |
| Tempo track | Table stakes as a feature; the closed-form ritardando integral is unusually careful. |
| Routing, hardware outs, channel sends | Table stakes. |
| Audio time-stretch | **Below** table stakes until the export bug is fixed. |
| Project lifecycle | **Do not claim this at all until §8.1 B6 is fixed.** |
| Device robustness | Mostly table stakes now. The exception is the `_exit(0)` teardown that stops a plugin wedging the machine's audio. |
| Accessibility | Not yet table stakes on macOS. |

### 2.3 The strongest sentence the product can honestly say

> The only DAW that records your screen, your webcam, your audio and your MIDI as one take that
> starts on the same instant, and names the chord while you play it.

Everything in the 170 commits is what makes that sentence safe to say to somebody who then tries to
mix the record.

---

## 3. Who Tangent appeals to now

### 3.1 The beachhead has not moved

**Self-learners remain correct.** Every DAW feature added since the audit is rated table stakes by
this audit's own sweep, which means it changed the competitive picture very little and the audience
picture not at all. What did change is that the DAW half now feeds the theory half: the person who
came for chord names and stayed to make a song is better served, not diluted.

*The honest counter-argument*, which deserves to be recorded: "table stakes" is a judgement about
competitors, not about users. A self-learner does not care about parity; they care that the thing
they wanted to do now works. On that reading the beachhead has genuinely drifted toward the
graduating hobbyist who outgrew GarageBand. What would settle it: whether new users arrive asking
about chords or asking about tracks.

Do **not** re-pitch as a general DAW. There are no Audio Units anywhere, so a GarageBand graduate
on a Mac cannot bring their instruments.

### 3.2 Teachers: still closed. The answer is no.

Is Tangent Section 508 defensible in a procurement conversation today? **No, and not close.**
The architecture is right and unusually elegant (nodes derived from each panel's existing hit-test,
so nothing can steal a gesture; readouts speak human units, "-4.5 dB" rather than "0.596"). But
macOS never detects a screen reader, the only door is a right-click menu row with no keybind, and
in reader mode the card still advertises Tab for a job Tab no longer does. Fix the correctness bugs
because they are bugs. Do not open a teacher conversation on the strength of them.

### 3.3 The most damaging gap is not a missing feature

**It is that most of what landed is behind a door with no handle.** There is not one automation
entry in the keybind table, so every automation gesture is discoverable only by clicking. The tempo
lane's add, remove and ramp gestures are on no card. The help card still says the stretch gesture
is MIDI-only, which stopped being true when audio stretch landed. There is still no persistent help
affordance, no menu row for the shortcut card, and no empty-state hint on a blank timeline.

The consequence is specific and expensive: **the beachhead will use, review and describe a smaller
product than the one that exists.** The cheapest close is a discoverability pass measured in days.

---

## 4. Version numbering and marketing

### 4.1 5.0.0 is the right number. The 4.x that preceded it was the mistake.

The major bump is correct: the product changed category and the on-disk contract broke. What is
indefensible is the run before it. Versions **4.78.0 through 4.93.0 all shipped across two days**;
the changelog's last entry is **4.5.0**; six of the fifteen 5.0 betas including HEAD are untagged.
The charge is not "too many versions", it is that **a version was minted for a commit instead of
for a release**, which makes the number carry no information for a user.

*The fair defence:* nobody was watching. The user base is a tester group known personally, the app
exposes no API, and the one external integration surface (the VST3) is retired. SemVer's promise is
made to downstream consumers and this project has none. That defence expires the moment money
changes hands.

### 4.2 `beta.15` is not a shippable number for a paying audience

Not because a pre-release label is dishonest, but because two of the open blockers are the two
categories a pre-release label does not excuse: **a binary that does not execute**, and **the
destruction of a thing the customer paid for**.

### 4.3 A versioning policy small enough to actually follow

1. `X.Y.Z-dev` lives in `Cargo.toml` on main. **Only `release.sh` writes a real version.** This
   makes spending a version on a commit structurally impossible rather than merely discouraged.
2. `release.sh` refuses to proceed if `git tag` lacks `v$VERSION`.
3. `release.sh` refuses to proceed if `CHANGELOG.md` lacks a `## [$VERSION]` heading.
4. Pre-releases are `rc.N`, capped at `rc.3`. Needing `rc.4` means the freeze was not a freeze, and
   the version goes back to `-dev`.
5. One release per day, with one carve-out stated openly: a same-day PATCH is allowed for something
   that release introduced.
6. Minor versions are for user-visible change, not for build iterations.

### 4.4 Copy rules: the sentences that must never be written

Each corresponds to a specific divergence in the code, and one of them is **already shipping as
false text inside the app**.

1. *"Tangent hosts VST3 and LV2."* LV2 is **Linux and FreeBSD only**. The plugin-format story
   improved on the two platforms with the fewest users.
2. *"Tangent exports MP3."* True everywhere, by two different routes: compiled-in LAME on macOS and
   FreeBSD, the ffmpeg sidecar on Windows and Linux. Delete the sidecar and MP3 goes with it.
3. *"Video encoding costs almost nothing."* True on macOS only. **The app currently tells Windows
   users exactly this, and it is false** (see §8.1 B4).
4. *"Records screen, webcam, audio and MIDI on every platform."* **FreeBSD has no camera at all.**
5. *"A 17 MB DAW."* No longer true anywhere.

**The rule to adopt as a written standard:** any sentence with a capability verb and no platform
noun is a bug.

### 4.5 The revised wedge

The old wedge was "the free, 17 MB, theory-teaching, Linux-friendly DAW that is the only one that
records screen, webcam, audio and MIDI as one honest take and pipes straight into OBS." The 17 MB
is false and "Linux-friendly" is awkward while the Linux build does not start. Revised:

> **The free DAW that names the chord while you play it, and writes every take as audio, MIDI and
> video that all start on the same instant. No account, no telemetry, no subscription.**

With the honest footnote, which buys more credibility than it costs: *"It is 16 tracks, it hosts
VST3 (and LV2 on Linux and FreeBSD), and it does not do comping or Audio Units. It is not a REAPER
replacement and is not trying to be."*

### 4.6 The upgrade problem, which must be solved before publishing

Three things must reach every existing supporter **before** 5.0 is published:

1. Your supporter key file will be deleted. Paste it again; it still works.
2. Your taught chords, your patches, and any unsaved arrangement will be deleted.
3. On macOS you will have to re-grant camera and microphone access, because the bundle id changed.

**And there is a prior problem: the list of who those people are may not exist.** The sale ledger
lives on a single Fly volume that was created on the day of the rename, the local ledger copy
carries no email addresses, and the repo's own setup instructions still name the *old* volume, so
anyone following the disaster-recovery runbook would stand up a service whose ledger is ephemeral.
**Pull a copy of that ledger off Fly before anything else in this document.**

---

## 5. Is it visually distinct?

### 5.1 Yes, in the two surfaces nobody has photographed

Three decisions would make a Tangent screenshot recognisable at a glance:

1. **The theory band over a 128-key piano.** A circle of fifths, a Tonnetz, harmonic triangles and
   a grand staff in a row across the top of the window, live, as surfaces you can click. No other
   DAW has this. A test asserts every Tonnetz triangle is a real triad, so the lattice is music
   theory rather than decoration.
2. **The palette as one sweep.** Sixteen channels each with its own colour, generated so luminance
   climbs monotonically from track 1 to 16 while hue walks an arc, every one dark enough to carry
   ivory text at 4.5:1 and every neighbour at least 10 dE apart. Most DAWs give you eight colours
   that cycle; this reads as one instrument.
3. **The wordmark.** "TANGENT" spelled across two rows of a Tonnetz lattice, the bottom row offset
   exactly between the top, drawn as pure vector at runtime, with lines running off the crop so it
   reads as a window onto an infinite lattice. A test asserts the geometry, so a rearrangement
   cannot silently turn the logo into two lines of type. **This is the best piece of identity work
   in the project:** the mark is the product's own subject matter.

And it is measurably not an egui app: every main surface is a raw painter with zero widget calls,
and even the dialogs force square corners and paint from the derived surface. The language is
restrained skeuomorphism with a real materials vocabulary: wood, engraved ink, plates, an LCD with
green on black glass, a gridbox, a steel-plate reverb after the EMT 140.

### 5.2 The tension, and a finding that reframes it

The brief asked whether the parchment, monospace, textbook look is a wedge (the DAW that does not
look like a spaceship cockpit) or a liability (it looks like a teaching aid, not a tool you would
mix a record in).

**The premise is partly wrong, and that is the finding.** Tangent is not a parchment app. It ships
half parchment and half conventional dark, in one window, and **all sixteen factory themes have
light backgrounds.** There is no dark theme; "Dusk" is a pale lilac. The identity is front-loaded
onto the chrome and the heritage surfaces and thins out toward the mixer and arrangement, which are
the surfaces a DAW is actually judged on.

So it is a wedge currently being operated as a liability, because the app has not committed to
either side. There is a real defence of the split (staves and circles are print objects that read
better on light; waveforms read better on dark, and the theme code argues exactly this), but it is
a defence of a *deliberate* split, and what ships reads as an incomplete one.

### 5.3 Ranked visual fixes before 5.0

1. **Shoot the screenshots.** All four in `docs/img/` predate the entire 5.0 batch: they show the
   theory band, piano and fretboard and no recorder, mixer, arrangement, MIDI editor, drum machine
   or themes. The images that sell the product depict a chord monitor. The deterministic headless
   screenshot harness already exists, so this is a script, not a photo session. **Lead with the
   recorder band**, the app's most distinctive and least photographed surface.
2. **Fix the About box.** It still reads "Simple MIDI Keyboard Monitor with Advanced Chord
   Detection" directly above "Version 5.0.0-beta.15", and is the one surface where the new identity
   is absent. Draw the wordmark there.
3. **Cut a real small-size icon.** The 16 px frame is an illegible smudge; the source art is 12%
   ink at low mean luminance, so it has no silhouette.
4. **Decide the light/dark question** and add two or three genuinely dark factory themes.
5. **Theme the two panels that no theme reaches:** the drum machine (hard-coded walnut) and the
   export progress window (hard-coded ivory, on screen for minutes).
6. Put the wordmark at the top of the README, and make the theme-template writer actually run: it
   is dead code, so the "user themes" folder is empty until you save one.

### 5.4 Does the contrast machinery matter commercially?

Not as a feature; do not try to sell it. It is valuable one level removed: it is what makes sixteen
themes affordable for a solo developer, and it is a genuine accessibility asset that cannot yet be
delivered. Market the sixteen themes it makes possible, not the mathematics.

---

## 6. Is the feature set competitive?

### 6.1 The revised category table

| Category | 5.0 verdict | Now | Why |
|---|---|---|---|
| Video capture | First, beats all five | **First, unchanged** | Still nobody else captures video. |
| Streamer / OBS path | First equal | **First equal, unchanged** | |
| Live theory detection | First, beats all five | **First, unchanged** | Still the only correctable detector. |
| Price / licensing | Tied first with Ardour | **Four-way tie** | Free is table stakes in 2026, not a differentiator. |
| Footprint | Second to REAPER | **Regressed** | True on macOS only; 40 to 47 MB elsewhere. |
| Onboarding mechanic | First | **First, but undermined** | The card is excellent and most new features are not on it. |
| Platforms | Third of six | **Worse in shipped reality** | Four platforms plus a real FreeBSD port, and the Linux artifact does not start. |
| Composition | Fifth of six | **Fifth, unchanged** | |
| Traditional recording / mixing | Last of six | **Still last, but no longer disqualifying** | Automation, PDC and buses removed the two objections a reviewer would have led with. Comping is still the hole. |
| Audio editing | Last | **Still last** | Stretch landed but does not survive an export; no comping. |
| Extensibility | Last, worst in field | **Last on macOS and Windows; mid-field on Linux and FreeBSD** | The format count moved where the users are not. |
| Notation | Loses to Logic and PT | **Unchanged** | Playable staff is not notation editing. |
| Ecosystem / longevity | Last, decisively | **Last, decisively** | |
| **Stock effects quality** | *not rated* | **New: genuinely strong** | The DSP is designed, not assembled. Ahead of Ardour's stock set. |
| **FreeBSD support** | *not rated* | **New: unambiguous first** | No other DAW has a native low-latency path. |

**Eleven of thirteen ranks did not move.** That reads harsher than it should: the two objections a
reviewer would have led with are gone, and a ranking table is a lagging indicator. But it is the
honest picture, and it is why §2's distinction matters so much.

### 6.2 Against the free tier specifically

The prior audit under-weighted the real competitive set. For a free app, the competitors are
Ardour, Waveform Free, Cakewalk Sonar Free and GarageBand, not Pro Tools. On features Tangent loses
to all of them on every traditional axis, and they got stronger while this work was happening.

**That is survivable, but only if the wedge carries the whole weight.** A theory-curious bedroom
producer opening Waveform Free meets an unlimited-track professional workstation and no idea what
to do in it. Opening Tangent they get a picture of a piano that sounds when clicked, chord names
appearing as they play, and one key that records everything. That is a different product, and the
comparison a feature grid invites is the one to refuse.

### 6.3 LV2 and FreeBSD, honestly

LV2 is what makes the Linux build a usable DAW rather than a demo, and the FreeBSD port is a
genuine, checkable first. Both land on the two platforms with the fewest users, while macOS and
Windows remain VST3-only with no Audio Units. **And as shipped, LV2 is currently net-negative on
Linux, because it is the reason the app does not start.**

The defence deserves recording: FreeBSD was never a market bet. It was a bet on one committed
high-value tester, and on portability as a forcing function, and it paid in that currency. It
forced the audio-host abstraction to be honest, produced the vendored patches that made the
aarch64 Linux build work at all, and widened the X11 editor backend that LV2 UIs now use.

**The cheapest move that shifts the extensibility rank where users are: CLAP.** Permissive
pure-Rust bindings, no C library, works on all four platforms, and does not disturb the
cross-compilation path that kept LV2 off macOS and Windows.

---

## 7. What programmers would think, before and after

This was the owner's most-curious question. Four engineers were asked to judge the public face
first, then the code.

| Level | Before | After | Direction |
|---|---|---|---|
| **Junior** | 7/10 | **8/10** | up, qualified |
| **Mid** | 7/10 | **6/10** | **down**, qualified |
| **Senior** | 5/10 | **7/10** | up, qualified |
| **Staff** | 5/10 | **6/10** | up, qualified |

### 7.1 Before

The junior scored it highest before opening a file and was the only one who caught the
inconsistencies from outside: the README describing 3.0.0 against a 5.0.0-beta.15 manifest, a
changelog that stops at 4.5.0 with an empty `[Unreleased]`, store copy still leading with the
retired plugin, and no mention anywhere of a mixer or a timeline. *"Beautiful writing, clearly
loved, and out of date in a way I can see from outside."*

Senior and staff both scored 5 and for the same reason: from the outside this looks like a
one-person project with an unusually good README, which is a pattern they have both been burned by.

### 7.2 After, and why mid-level is the one that falls

**Junior (7 to 8).** Went up because the code taught them things. The effects module explains the
physics of an EMT 140 plate before showing the DSP; the 68-line safety proof is the first `unsafe`
they could follow; the keybind table is a pattern they would copy. Capped by things they could
check in ten seconds: 2,169 formatting diffs (so format-on-save would blow up the repo), a warning
count that would make their first build look broken, and a 36,060-line file with a 255-field struct
and no map.

**Mid (7 to 6), the only fall.** Not because the code is bad but because this is the level that
asks "could I ship a feature here on Monday?" The answer is no: their first change lands in
`app.rs`, in a 255-field struct inside one impl block running 20,000 lines. Their verdict is the
sharpest sentence any of the four produced: *"My actual verdict is neither word: it is unenforced
rigour, which is a habit rather than a property, and habits leave with the person."*

**Senior (5 to 7).** Went up on substance: the plugin handoff protocol, the denormal and filter
discipline, the automation invariant, the frozen migration constants. *"That is what an audio
engine costs, and the author knows which parts are load-bearing."* Pushed back on the enforcement:
eleven tests that grep their own source text are a substitute for a type, and most of those
invariants are a newtype-wrapped accessor away from being enforced by the compiler for free.

**Staff (5 to 6).** Rejects the over-engineering charge outright: no speculative abstractions, no
framework, no plugin architecture built for a second consumer, zero TODOs, 1,002 named constants.
The classic fingerprints are absent. What is present is *unteachable*, which is a different and in
some ways worse problem. *"A new hire who renames a function gets a failure reading 'the renderer's
impl moved', which is a puzzle rather than a message."*

### 7.3 The convergent finding

All four land in the same place from different directions, and it is the answer to the question:

> **The rigour is real. The enforcement is a person, not a system.**

There are 1,795 test functions, a 13,133-row differential corpus and eleven source-scanning
invariants, and **no CI, no hooks, and no build or release script that runs any of them.** The
proof that this is not theoretical is the rot in exactly the places no machine was watching: the
lint policy quotes 33 unwraps against an actual 242, claims the build stays warning-free against
hundreds, and cites a version three majors old; the formatting decision documents 171 hunks against
an actual 2,169.

And the irony worth telling the author directly: **this project invented eleven source scans to
enforce rules no runtime assertion can state, and the one rule the compiler enforces for free is
the one that went unenforced.**

### 7.4 The critic's corrections, which change the story

An adversarial critic was set on the panel and found two things that matter:

**The panel converged too neatly.** Mid, senior and staff cite the same five positives in the same
order and the same eight negatives. Four independent readers of a 200,000-line tree do not do that.
Treat the junior's verdict as the most independent, and read the other three as one analysis at
three volumes.

**The panel's unanimous prescription is wrong.** All four said "add one CI job running `cargo test`
and `cargo check`." That would have caught **none** of the five shipping defects the same panel
named: the Linux blocker is a runtime linkage failure on a clean box, and `cargo test` passes on any
machine that can build the project; the false Media Foundation string **has a test and it passes**;
the config purge **has two tests and they pass**; and the README and CHANGELOG are not code.

What would catch three of five is a **clean-box boot**: a container with no dev packages, unpack
the shipped tarball, run it, assert a window appears. The project already half-owns this discipline;
its own Linux shipping script says in its header that "a cross-build proves the binary LINKS, and
only running it on Linux proves it starts."

Also worth correcting before anyone repeats it: the suite is **green** (1,674 tests run, 0 failed).
The warning count depends on the command: roughly 670 for `cargo check --workspace`, 998 with
`--all-targets`, of which the overwhelming majority are one cosmetic lint the author himself
enabled. The debt is real but it is one lint's worth, not a collapse.

### 7.5 What each would actually do

Contribution is not currently available: there is no `CONTRIBUTING`, no `.github`, and the source of
truth is Codeberg with GitHub as a mirror. This is a repository you read, not one you join.

- **Junior:** stars it, reads the effects and keybind modules as the textbook they are, copies the
  pattern into their own work, files nothing.
- **Mid:** forks to read the DSP, ships nothing back, because any non-trivial change lands in
  `app.rs`.
- **Senior and staff:** *would hire the author* and would not contribute today. Both said so
  independently and firmly. The staff verdict: *"On a team, with a reviewer and a pipeline, this
  person is a strong senior heading toward staff."*

The thing all four said they cannot teach and rarely see: the habit of writing down the failure that
motivated a fix, in the reporter's own words. One commit documents a defect it is deliberately *not*
fixing, prices both candidate fixes, and wires a marker into a test so the next person cannot miss
it.

---

## 8. The 5.0 blocker list

### 8.1 New since the last audit, ranked by damage

**B1. Stock Ubuntu GNOME cannot start the app. One distro, silent failure, cheap fix.**

*This finding was overstated in the first draft of this audit, which called it a Linux-wide
failure. That was wrong and the correction matters, so it is recorded here rather than quietly
edited away.*

The linkage fact is solid: `liblilv-0.so.0` is in `DT_NEEDED` on both architectures with **no
`DT_RUNPATH` or `DT_RPATH`**, the tarball ships no copy, and `grep -ci lilv` returns **0** for
`README.txt`, `install.sh` and `ARTIFACT-README.md`. It is unconditional — `lilv` and `livi` are
declared as plain target dependencies, so no cargo feature or build flag produces a Linux binary
without it.

**But on most distributions this is invisible, because liblilv arrives with the audio or media
stack anyway:**

| Distro | liblilv present by default? | How it arrives |
|---|---|---|
| Debian 13 (GNOME/KDE) | **yes** | `libpipewire-0.3-0t64` → `libspa-0.2-modules` → `liblilv-0-0` |
| Fedora Workstation 43 | **yes** | comps `multimedia` group, mandatory `gstreamer1-plugin-libav` → `libavfilter-free` |
| Arch / EndeavourOS | **yes** | `pipewire-audio` hard-depends on `lilv` |
| Void (the owner's box) | **yes** | `pipewire` `run_depends` includes `liblilv` |
| Mint, Pop!\_OS, openSUSE | **yes** | libavfilter or gst-plugins-bad |
| Kubuntu, Xubuntu, Lubuntu, MATE, Budgie, Studio | **yes** | each ships a media player pulling libavfilter |
| **Ubuntu GNOME (`ubuntu-desktop`)** | **NO** | — |

Ubuntu is the sole outlier and the cause is structural rather than accidental: `liblilv-0-0` is in
**universe**, `ubuntu-desktop` is in **main**, Ubuntu policy forbids a main package depending on
universe, so Canonical builds PipeWire with `-Dlv2=disabled` (Launchpad #2054223). Verified against
Canonical's own ISO manifests for 24.04.4 and 26.04.1: zero hits for lilv, serd, sord, sratom or
any `libav*` package.

**It stays a ship-blocker anyway, for three reasons.** Ubuntu is the default answer to "which
Linux" for the self-learner beachhead; the shipped README names Debian/Ubuntu package names
explicitly; and the failure mode is the worst available for a first impression — `ld.so` refuses to
exec, so a double-click produces *nothing*: no window, no crash dialog, no log line. There is also
a nasty interaction: on Debian/Ubuntu the single command most likely to have rescued a user by
accident is `apt install ffmpeg`, and because Tangent bundles its own static ffmpeg, nobody is ever
told to run it.

**FreeBSD got this right and the tarball path never inherited it:** the FreeBSD manifest declares
lilv with the comment *"the binary links liblilv-0.so outright, so a package that forgets it starts
nowhere"*, and the build hard-fails without it.

*Close it, in order of cost:*
1. **~1 hour, do this regardless.** A `DT_NEEDED` guard in `package_linux()` asserting every entry
   is on a declared allowlist and named in `README.txt`, mirroring the FreeBSD check. Then name
   lilv beside libasound in the README and add a `have_lilv()` preflight to `install.sh` next to the
   existing `have_libva()` — but loud, because libva absence costs CPU while lilv absence costs the
   whole app.
2. **~half a day, actually fixes it.** Ship `liblilv-0.so.0` plus its serd/sord/sratom/zix closure
   with an rpath. **Implementation trap:** `install.sh` copies only the binary into `~/.local/bin`,
   so a bare `$ORIGIN` would resolve to a directory the libraries are not in. Use
   `$ORIGIN:$ORIGIN/../lib/tangent` and have `install.sh` place them accordingly. Decide this before
   writing the linker flag.

*Not recommended:* `dlopen`ing lilv. It sounds cheapest and is not — lilv and livi are vendored and
the call sites are spread through the LV2 host.

**B2. Three known testers lose their key, their chords and their patches. Housekeeping, not a blocker.**

*Also downgraded from the first draft, on the owner's data: there are five testers and two already
hold fresh keys, so the exposed population is at most three people he knows personally. It never
grows, because the purge only fires on a machine that has the pre-5.0 directory and no post-5.0
user will ever have one. This is an email, or six lines of code.*

`purge_pre_5_0()` runs `remove_dir_all` on the old config directory unconditionally at startup:
"nothing is read out of it first and nothing is kept." That directory holds `license.key`,
`overrides.json`, `my-patches.syx`, and, at 4.93, **the auto-saved arrangement** (which the
function's own inventory comment does not list, so the document naming what is destroyed omits the
user's unsaved song). On macOS the bundle id also changed, so the same launch drops camera and
microphone consent. The README currently tells readers the *opposite*: that the config path was
deliberately never changed so nobody's settings get orphaned.
The code itself is careful and the decision is argued. The failure is that it is argued **only in
source comments**.
*Close it:* copy `license.key` forward before the delete (six lines), and write the upgrade note
into the CHANGELOG, the release notes and the store page.

**B3. A corrupt project opens as a silent empty one, and four code paths then overwrite it.**
Found during this audit; it was not in scope for any of the nine sweeps. `from_json` returns a
default document on *any* parse failure and `load_from` does the same on *any* read error, so a
truncated `project.tangent` opens as an empty timeline with no message. Opening then resets undo,
so the project is not dirty and no prompt will fire.
And **"nothing saves itself" is false at HEAD.** There are six non-test writers of the document, of
which only two are explicit saves. The other four write it with no prompt: finishing a recording,
the button that empties the timeline, a passive clip-length annotation after a decode, and **every
undo and every redo**.
Chain them and you get real data loss: open a truncated project, press undo or finish a take, and
the empty document is written over what was there.
*Close it:* distinguish missing from corrupt and refuse rather than defaulting; and either make the
four implicit writers explicit or stop advertising that nothing saves itself.

**B4. Windows users are told a performance claim that is not true.**
The Take Settings panel says video encoding "runs on Media Foundation, so three 1080p30 streams
cost the CPU almost nothing." **There is no Media Foundation code in the repo.** Windows runs the
same `libx264 -preset veryfast` software encoder as Linux. The macOS and Linux arms of that message
are both true; the Windows arm is the only one promising the cost is near zero.
*Close it:* fold the Windows arm into the software-encoder branch, and fix the two doc comments.
One line, and the entire positioning rests on honesty.

**B5. A plugin in the master's second rack is silent in playback and present in the export.**
Found and confirmed during this audit by arithmetic and by reading the loop bound. The live
reconcile sizes its record as `(STRIPS + 1 + DRUM_PADS) * INSERTS` = **444 slots**, but
`MASTER_EXTRA_SEAT` **is** seat 148, an index rather than a count, so it needs slots 444 to 446:
exactly one seat past the end. The reconcile loop iterates `0..len`, so those bays are never loaded
into the live engine.
The rest of the system is correct, which is what makes it sharp: the settings writer uses the
`INSERT_SPACE` constant with a comment explicitly naming "the master's second rack", a test asserts
the bound, the engine sizes its latency array from the full seat count, and **the export loads every
seat from the app's own state.** So you mix without the plugin and it appears in the bounce, which
is the worst possible shape for a mixing bug. Per-channel drum-pad racks beyond the first channel
fall outside the same array.
*Close it:* use `INSERT_SPACE` at all four allocation sites.

**B6. 5.0 cannot be published by the release script.**
`publish-github.sh` refuses to create a release without release notes. The CHANGELOG's last entry
is 4.5.0 and `[Unreleased]` is empty, so ~170 commits of the largest change in the project's life
have no notes.
*Close it:* one `## [5.0.0]` entry covering the arc from 4.5.0, plus a line stating that 4.6
through 4.93 were internal iterations never separately released. Do not backfill ninety entries.

**B7. The fulfilment service can mint a second key for one sale.**
The ledger lookup uses `?` on a per-line parse *inside* the loop, so it returns "not found" from the
whole function on the first unparseable or blank row. The handler then falls through, the sale is
genuine, and a second key is minted, breaking the module's own headline promise that a fulfilled
sale re-sends the original key. The sibling function in the same file skips bad lines deliberately,
which is what makes this a slip rather than a design. Separately, the genuineness check has no
product-id and no refund or dispute check, and keys are perpetual with no revocation path.

### 8.2 The recommended order

1. **Pull the supporter ledger off Fly** and store a copy elsewhere. Everything else can be redone;
   the list of who paid cannot.
2. **B3** (silent data loss) and **B5** (silent master rack). These two are correctness bugs that
   reach every user on every platform, and both fail quietly.
3. **B1** (Ubuntu GNOME cannot start) and **B4** (false Windows claim). Both are one-distro or
   one-line, and both are first-impression damage.
4. **B6** (changelog), which mechanically blocks publishing at all, then **B7** (double minting).
5. **B2** (config purge) — one email to three people, plus the six-line key migration if it is
   cheaper than the email.
6. Then the four original blockers still open: the macOS pkg in the release path, Authenticode
   signing, the Windows zip alias, and the copy that still sells the retired plugin.
7. The licensing homework, which is unchanged from the last audit and still cheap.
8. The discoverability pass and the screenshots, which are what make the work of the last 170
   commits visible to the person using it.
9. Write down the comping rationale (§1.5), which costs one paragraph and removes the trust
   discount that an unexplained absence applies to everything else.

### 8.3 The one process change worth making

Not "add CI", for the reason in §7.4. **Add a clean-box boot to the release path:** a container per
platform with no dev packages, unpack the shipped artifact, run it, assert a window appears. That
catches B1, catches the class of B2 on macOS, and catches the missing-install.sh class that already
bit 4.92. CI running the tests is worth adding too, but as the second move, and specified properly:
the default test run skips 51 ignored tests including the full differential corpus, and two crates
declare their own workspaces so `--workspace` never reaches them.

---

## 9. The platform matrix

| | **macOS** (universal) | **Windows** x86_64 | **Linux** x86_64 / aarch64 | **FreeBSD** |
|---|---|---|---|---|
| VST3 | yes | yes | yes | yes (bundle dir is `amd64-linux`; the SDK hardcodes `-linux` on every Unix) |
| **LV2** | **no** | **no** | **yes** | **yes**, and the only format that matters there |
| LV2 editors | n/a | n/a | X11UI only | X11UI only |
| VST3 editors | yes | yes | yes | yes |
| **MP3 export** | compiled-in LAME | ffmpeg sidecar | ffmpeg sidecar | compiled-in LAME |
| Video encode | VideoToolbox (hardware) | `libx264` (software) | `libx264` (software) | ffmpeg (pkg dep) |
| **Camera** | AVFoundation | bundled ffmpeg dshow, **as a subprocess** | V4L2 + optional VA-API | **none**, stubbed |
| Audio host | CoreAudio | WASAPI + ASIO | ALSA only | ALSA shim, JACK, or **native OSS** |
| MIDI in | CoreMIDI | WinMM | ALSA seq | hand-written `/dev/umidi` reader |
| Download size | 21.4 MB | 47.3 MB | 42.1 / 37.8 MB | 8.5 MB pkg |
| **In the release path?** | yes | yes | yes | **no** |

Two consequences worth stating plainly. **The Windows camera silently depends on the ffmpeg
sidecar** sitting beside the binary, because it is an ffmpeg subprocess rather than a native API.
And **FreeBSD is a real port that no release ships**: the release script exits on any host that is
not macOS or Linux, and the package filename matches neither the checksum glob nor the upload
globs, so it is never checked, never checksummed and never published. Decide explicitly whether
FreeBSD ships; right now it is excellent work in a drawer.

---

## 10. Method, and what to distrust

**How this was produced.** Nine parallel code sweeps, each followed by an adversarial agent whose
only job was to refute that sweep's optimistic verdicts; 23 verdicts were downgraded and those
downgrades are what this document reports. Then a judgement phase (competitive re-rank, audience,
versioning, visual) and a four-level programmer panel, followed by two critics: one attacking the
panel, one hunting for what nobody had asked.

**Corrections made during the audit, recorded so the same errors are not repeated:**

- The Windows camera is the bundled ffmpeg's DirectShow input, **not** `IMFSourceReader`. One sweep
  misread a comment naming the rejected alternative. Resolved by reading the file.
- The test count is roughly **1,795 to 1,799** `#[test]` functions in the five workspace crates, of
  which 51 are ignored, and the suite passes. An earlier figure of 1,830 does not reproduce.
- The warning count depends on the invocation: ~670 for `cargo check --workspace`, 998 with
  `--all-targets`. The great majority is one cosmetic lint.
- Two prior "progress" claims (the retired-plugin mechanism, the ffmpeg licence pointers) were
  credited to work that **predates** the 5.0 audit, and were downgraded to untouched.
- The Ed25519 backup claim in the 5.0 audit was **wrong**; a backup exists and was verified.

**Corrections made after the owner's review, which are the two largest in this document:**

- **B1 was overstated.** "The Linux build does not start" and "liblilv is not installed by default
  on any mainstream desktop distribution" were both false. The library is present by default on
  Debian, Fedora, Arch, Void, openSUSE, Mint, Pop and every Ubuntu flavour; it is absent only on
  stock Ubuntu GNOME. The owner's report that the app has been running on his Void box for days is
  correct **and his box is representative** — Void's PipeWire pulls liblilv in, exactly as Debian's
  and Arch's do. An earlier draft suggested his machine was unrepresentative; that was wrong too.
  The blocker survives at one-distro severity because that distro is Ubuntu.
- **B2 was over-ranked.** It sat second on the assumption of an unknown installed base. With five
  testers, two already re-keyed, it is three known people and an email.
- **Comping and the 16-channel cap were mis-filed as gaps to close.** Both are deliberate; see §1.5
  for the reframing and for what still needs writing down.
- **A reasoning error worth recording even though it changes no verdict:** this audit stated that
  `libGL`/`libEGL` "degrade rather than blocking startup". That is wrong. There are two consumers —
  the compositor's own loader, which does degrade, and **glutin**, which does not: `main.rs:535`
  pins the glow renderer on every non-macOS target, so with no GL implementation the app shows a
  dialog and exits. It changes nothing in practice, because those libraries are in every graphical
  session, but it is the same class of error as B1: asserting a runtime behaviour never observed.
- **A free win found during the recheck:** the glibc floor of 2.32 is self-imposed by the build
  target, not by the code. The binary's highest required symbol version is driven by a single
  symbol and the true API floor is 2.30. Retargeting would cost nothing and would add Ubuntu 20.04,
  Debian 11 and openSUSE Leap 15.4/15.5.

**What to distrust in this document.** The competitive rankings and the audience judgement are
opinion built on verified facts, not facts. The programmer panel is a simulation, and its own critic
found that three of its four voices converged more than four independent readers would; treat §7.3
as its finding and the individual verdicts as illustration. B5 was confirmed by arithmetic and by
reading the loop bound rather than by running the app, so it deserves a runtime confirmation before
it is written into a changelog. Nothing here has been verified by running the shipped artifacts on
their target operating systems, which is precisely the gap §8.3 recommends closing — and it is how
B1 got its reason wrong: a runtime failure was asserted from the linkage without ever checking the
claim about the world that made it a Linux-wide problem rather than an Ubuntu one. Two minutes of
container time converts the whole finding from inference to demonstration:

```
podman run --rm -v "$PWD/dist/tangent-5.0.0-beta.15-linux-x86_64:/t:ro,Z" \
  docker.io/library/ubuntu:24.04 /t/tangent --version
# expect: error while loading shared libraries: liblilv-0.so.0
```

Run the same against `debian:trixie` with `libpipewire-0.3-0t64` installed and it gets past the
loader. That pair is the entire finding, reproducible, and it belongs in the record as evidence
rather than as reasoning that predicts evidence.
