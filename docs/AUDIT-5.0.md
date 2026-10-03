# Tangent 5.0 Readiness Audit
2026-08-25 · repo at ef2ab01 (v4.92.0) · 14-agent audit (5 repo sweeps, 7 research, 2 synthesis)
· companion to the /code-review ultra run on the v4.82.0..HEAD diff

> **Read as of its date.** This is a snapshot of the repo at `ef2ab01`, not a live
> reference, and the gesture vocabulary has moved since. Notably: drawing a MIDI
> item on a lane is CMD-drag (it was shift, and shift is now unbound there), the
> exclusive arm is ALT rather than shift, and clips carry a loop with a draggable
> length. `docs/ARRANGEMENT.md` and the in-app H cards are the live answer. The
> body below is left as it was written — an audit that is quietly edited to match
> a later repo stops being evidence of anything.

> Owner's questions: (1) target audience · (2) excels/lacks · (3) category wins vs competitors
> · (4) marketing & release plan · (5) proprietary candidates · (6) full feature draft
> · (7) pace — answered separately in-session.

## Executive summary

**The product is real, the wedge is narrow and defensible, the licensing is clear, and the
store plumbing has six concrete blockers — all cheap to fix.** Verdicts in one screen:

- **Audience:** lead with SELF-LEARNERS (the theory-curious bedroom producer) as the beachhead;
  creators/streamers are the amplifier (the only category where Tangent beats all five majors);
  hobbyists are the free-tier funnel; TEACHERS are phase 2 — do not market 5.0 to them
  (no accessibility, no Chromebook story, no curriculum = procurement blockers).
- **The wedge:** "the free, 17 MB, theory-teaching, Linux-friendly DAW that is the only one
  that records screen+webcam+audio+MIDI as one honest take and pipes straight into OBS."
  In that framing there is no direct competitor. In traditional DAW categories (automation,
  comping, PDC, time-stretch, extensibility) Tangent is last or near-last, and saying so is
  part of the credibility.
- **Category wins (strict):** video takes — beats all five; OBS/streamer path — first equal
  (ties REAPER); live chord detection + correctable detector — beats all five; price — tied
  first with Ardour; footprint — second to REAPER by a hair; onboarding mechanic (H-overlay)
  — first. Composition: fifth of six. Traditional recording/editing/extensibility: last.
- **Pricing:** FREE 5.0 + a "Tangent Supporter" Gumroad listing (PWYW $5 floor, $15 suggested),
  Gumroad license key redeemed at the Fly service for a real offline Ed25519 key. The
  "free, stays free" welcome promise is load-bearing; 5.0 must extend it, never revise it.
- **Licensing: all clear, with homework.** VST3 hosting needs NO Steinberg agreement (bindings
  are MIT; trademark-hygiene checklist only). Bundled ffmpeg is a GPL sidecar — legally fine
  as a separate spawned program, but 5.0 must host the corresponding source and link it from
  the Gumroad page; LAME attribution is MISSING from THIRD-PARTY-LICENSES on mac/linux (fix).
  Trademark risk on "Tangent": LOW for C&D exposure (do a proper clearance + register before
  the brand hardens; TRADEMARK.md is the moat move).
- **Ship-blockers found in release machinery (fix before 5.0):**
  1. Versionless Windows zip permalinks 404 on next publish (README + old supporter emails).
  2. The native-path Linux tarball ships WITHOUT install.sh and tangent-ffmpeg — the README
     inside it tells buyers to run a script that is not there (the 4.92 tarball on the void
     Desktop has this defect — verify during that test).
  3. Tangent-macos.pkg — the primary mac buyer link — is built by no script in the release path.
  4. The setup.exe a Windows buyer runs is never Authenticode-signed (SmartScreen wall).
  5. Installer UI + supporter emails + store copy still sell the retired VST3 plugin.
  6. release.sh's artifact checks cannot catch #2 (one-line fix).
- **FreeBSD:** three hard build-breakers (midir backend, two scan.rs compile errors), plus
  BUFFER_PERIODS/dialogs/camera wrong-behavior arms — the port is real work but bounded;
  full worklist in the portability section. Keep it OFF the 5.0 critical path.
- **Proprietary path (MIT-forever core):** trademark + official signed binaries; content
  (DX7 banks now, curricula later); the supporter/fulfilment rail (back up the Ed25519 seeds
  — currently un-backed-up); an OBS chord-overlay companion later. Do NOT close the chord
  engine — it is the identity feature and the marketing story.

## Table of contents
- Product Verdicts — Audience, Excels/Lacks, Category Wins, Proprietary Candidates (Q1, Q2, Q3, Q5)
- Marketing & Release Plan (Q4)
- Competitive Analysis — REAPER, Ardour, Ableton, Pro Tools, Logic (Q3 evidence)
- Market & Audience Research (Q1/Q4 evidence)
- Feature Catalog — from the changelog (Q6, release-notes material)
- UI Surface Catalog — views, dialogs, every shortcut (Q6, manual material)
- UX Review — strengths, friction, verified gap list
- Licensing: VST3 Hosting
- Licensing: Bundled ffmpeg Compliance
- Naming: 'Tangent' Trademark Risk
- Gumroad Mechanics & Store Setup
- Windows Code Signing
- FreeBSD Portability Catalog
- Release Machinery Findings

---


# Product Verdicts — Audience, Excels/Lacks, Category Wins, Proprietary Candidates (Q1, Q2, Q3, Q5)

# Product Strategy: 5.0-Readiness Audit (Tangent 4.92.0)

## 1. Target Audience Verdict

The owner's proposed set (teachers, self-learners, creators, hobbyists, "wide acceptable") is directionally right but wrong as a flat list. Ranked for the product **as it exists today**:

### Ranking

**1. Self-learners (the theory-curious bedroom producer) — the beachhead.**
This is the only segment where Tangent's strongest existing feature (live chord detection with naming, runners-up, and a correctable detector no other DAW has) meets a large, reachable, paying audience (r/WATMM 3.8M, r/musictheory 709k; proven $40-150 one-time purchases: Scaler, Synthesia, Hookpad lifetime). The discovery channels (YouTube theory educators, Reddit, KVR, BPB) are exactly the ones a solo dev can access.
**What's missing:** Tangent *names* theory but does not yet *teach* it. The market gap the research identifies is "a DAW that IS the theory curriculum" (chords explained as placed, progression analysis in the arrangement); Tangent today is the DAW that is the theory *display*. Also missing for this segment specifically: the discoverability cliff (hold-H taught exactly once in the Welcome card; undo/redo completely unadvertised, wired outside the BINDINGS table at app.rs:15602), the dead first minute for mouse-only users (`keytoggle_enabled` defaults false, settings.rs:1124), and no MIDI CC lanes (a pianist's sustain-pedal data records but cannot be edited).

**2. Creators/streamers — the amplifier, not the market.**
Twitch Music averages only ~7,600-9,000 concurrent viewers; too small to be a revenue segment. But this is the only category where Tangent beats all five majors outright (native composited video takes; native OBS sender, interop-validated 2026-08-25), DMCA pressure pushes streamers toward original composition, and every on-stream use is free marketing to exactly the beachhead audience.
**What's missing:** the chrome that would be visible on camera undermines the pitch. The window title is always "Tangent" (no project name), and the About dialog still reads "Simple MIDI Keyboard Monitor with Advanced Chord Detection" (dialogs.rs:4653), which is the first thing a curious stream viewer will see opened. Video is 1080p30 only (no 60fps, no vertical/short-form export for TikTok clips), and capture is window-only.

**3. Hobbyists — the funnel, not a revenue segment.**
Anchored at $0 by BandLab (100M users) and GarageBand. Tangent's genuinely-free, no-account, 17 MB posture is the correct funnel shape (the Vital/Waveform pattern).
**What's missing:** sound in the first minute (same keytoggle default), content depth (one synth, one factory bank; a hobbyist compares against Logic's ~72 GB), no browser/Chromebook story at all, and no sharing/community mechanics.

**4. Teachers — defer to phase 2. Do not market to them for 5.0.**
Everything structural is missing: no web/Chromebook build (Chromebook fleets dominate US classrooms), no SSO/LMS integration, no per-student licensing or purchase-order motion, no notation *editing*, no curriculum materials, and — decisively — **zero accessibility** (no AccessKit anywhere in Cargo.lock, fixed-size window, mouse-only faders). For schools that is not a UX nit; it is a Section 508/procurement blocker. Teacher discovery also runs on PD ecosystems and school-year calendars that take a year of relationship-building. Enter later with free-teacher-account + $3-6/student/yr, on the back of self-learner testimonials.

**Verdict:** lead with self-learners, ship and market the creator features as part of the same product (not a separate SKU), treat hobbyists as free-tier volume, and explicitly *decline* the teacher segment until a web-or-accessible build and curriculum exist. "Wide acceptable" is how a 141k-line solo project dies; the wedge below (section 3) is narrow and real.

---

## 2. Excels / Lacks

### Genuinely excellent (strongest evidence for each)

1. **One-gesture audio+MIDI+video takes with an honest manifest.** WAV sample 0, MIDI tick 0, and video frame 0 are the same instant; `take.json` records every source, the sync report, and encoder-side dropped/padded frames (4.88) so the manifest cannot disagree with the live counter. No competitor of the five can capture video at all.
2. **The theory stack's teachability.** Live weighted-scoring detection (95 patterns × 12 roots), runners-up named ("C6, or Am7"), and user-teachable overrides with a learning re-ranker. Nobody else ships a *correctable* detector; Logic's Chord Track is offline-per-region, Ableton names nothing.
3. **The keybind system's architectural honesty.** One table (`keys.rs` BINDINGS), the H-card rendered *from* it, `available()` gating both `pressed()` and `draw_help()` — "listed but dead" and "live but unlisted" are the same impossible thing.
4. **Error-surfacing as a designed chain.** Worst-news-first status composition (desktop.rs:1207-1276, with the yank test that proved the old order backwards recorded in a comment); the VST2 scan message that tells the truth ("N .dlls seen are VST2, which Tangent does not host"); dead-control prevention as a lived rule (`has_editor` greys the row instead of apologizing after the click).
5. **Device robustness the majors do not advertise.** Yank-proof stall watchdog plus background Reprobe that reopens audio the frame hardware returns (4.90/4.92); ASIO single-client duplex; the Kontakt-safe `_exit(0)` teardown that stops a plugin from wedging the machine's audio (4.76).
6. **Footprint.** 16-18 MB installers *including* an FM synth validated against 11,756 real cartridges, a 128-pad drum machine, and video capture — within rounding error of REAPER, 1-3 orders of magnitude under everyone else.
7. **Real cross-platform, including per-platform camera backends** (AVFoundation / V4L2 + VA-API / DirectShow) and bundled checksum-pinned ffmpeg so filming needs nothing installed. Beats Ableton, Pro Tools, and Logic on Linux existence alone.
8. **The offline/free/no-telemetry posture with offline Ed25519 supporter keys.** Post-Waves-revolt, "no account, no subscription, activates offline forever" is itself a marketing asset in this community.
9. **Consistency machinery.** One dial grammar app-wide; pure-painter panels with hit-tests derived from the same Layout (picture and click targets cannot disagree — which is also what lets the band render into the video composite); settings v19 with migrations against frozen historical constants.

### Lacks (strongest evidence for each)

1. **No automation lanes and no MIDI CC lanes** (velocity lane only, midi_editor.rs:687; zero hits for automation anywhere). Disqualifying for serious mixing; every competitor has it.
2. **No plugin-delay compensation** (tangent-host never queries plugin latency; one passing comment at instance.rs:760). Any lookahead plugin silently smears the mix.
3. **No comping and no audio time-stretch** (MIDI clips do stretch; audio does not). All five competitors have both.
4. **Worst-in-field extensibility.** VST3-only, `INSERTS: usize = 3` fixed (recorder.rs:491), one send to one bus, no groups, no scripting, no ARA.
5. **No tempo track or time-signature changes** — one global BPM (20-300), one signature per project. A theory-teaching app that cannot represent a ritardando or a 4/4→3/4 change undercuts its own curriculum ambition.
6. **Accessibility is literally zero.** No AccessKit in the dependency tree; every main surface is a raw Painter a screen reader perceives as nothing; fixed-size window (`with_resizable(false)`, main.rs:280).
7. **The discoverability cliff.** The entire command surface hangs on two facts (hold H, right-click) taught once; undo — the feature that makes learners bold — appears on no card and in no menu; chorded bindings live as hand-written strings that can drift from their card rows.
8. **Identity drift.** About describes a MIDI monitor, not a 16-channel DAW with video takes; main.rs:318 claims plugin editors are mac-only when editor.rs supports all three platforms (and that stale rationale silently underpins the glow-renderer choice).
9. **One-click catastrophe:** "Reset Settings to Default" with no confirmation (app.rs:12928), sitting under About in every context menu, destroying desk gains, devices, and record folder.
10. **Ecosystem/longevity.** One developer, four weeks, 391 commits, no third-party tutorials or community. Reviewers will weight this heavily and they will be right to.

---

## 3. Category Verdicts (strict)

| Category | Verdict |
|---|---|
| **Video capture (screen+webcam takes)** | **First. Beats all five.** No DAW among REAPER/Ardour/Live/PT/Logic captures video at all; they import only. |
| **Streamer/OBS audio path** | **First equal.** Beats Ardour, Live, PT, Logic (who need loopback drivers); ties REAPER, which ships ReaStream itself. |
| **Live theory detection + correctable detector** | **First on detection and teachability, beats all five.** Loses "acting on theory" (generation/transformation) to Logic and Ableton. |
| **Price/licensing** | **Tied first with Ardour.** Beats REAPER, Live, PT Artist/Studio, Logic on sticker; beats PT Intro on limits. Caveat every reviewer will state: free-from-a-four-week-old-solo-project carries longevity risk. |
| **Footprint** | **Second to REAPER by a hair; beats the other four by 1-3 orders of magnitude.** |
| **Onboarding mechanic** | **First (H-overlay), with the honest footnote that a smaller surface does part of the work.** Among full DAWs, Logic and Ableton lead. |
| **Platforms** | **Third of six** (REAPER, then Ardour); beats Live, PT, Logic on Linux existence; ALSA-only is thinner than their JACK/PipeWire citizenship. |
| **Composition (MIDI + instruments)** | **Fifth of six.** Beats only Ardour (DX7 + drum machine vs a GM SoundFont player). Loses to Logic (content), Ableton (workflow), REAPER (editor depth), PT. |
| **Traditional recording/mixing** | **Last of six** on comping/punch/PDC/automation criteria. Arguably **first** for "capture the moment" — no one else records the take as audio+MIDI+video in one gesture. |
| **Audio editing** | **Last.** No audio stretch, no comping; all five have both. |
| **Extensibility** | **Last, worst in field.** VST3-only + fixed inserts is more restrictive than even AAX-only or AU-only, which at least have deep ecosystems. |
| **Notation** | Loses to Logic and PT (both edit; Tangent displays). |
| **Ecosystem/longevity** | **Last, decisively.** |

**Bottom line (unchanged from the competitive analysis, and correct):** Tangent must not be positioned as a REAPER/Logic replacement. The defensible wedge: *the free, 17 MB, theory-teaching, Linux-friendly DAW that is the only one that records your screen, webcam, audio, and MIDI as a single honest take and pipes straight into OBS.* In that framing it has no direct competitor; in most traditional categories it is last or near-last, and honesty about that is part of the credibility.

---

## 4. Proprietary Candidates

**The governing fact, verified on disk:** the entire repo is MIT (LICENSE, © 2025-2026 Ganten) — including the chord engine (`tangent-core/src/detector.rs`, `patterns.rs`, `overrides.rs`), the golden corpus (`tests/golden/corpus.json`), the acceptance contract (`tangent-core/tests/acceptance.rs`, `examples/diffcorpus.rs`), the keygen/fulfilment tools (`tools/tangent-keygen`, `tools/tangent-fulfil`), and Factory Bank 01 (`ivory/assets/Tangent-01-Tines-and-Boxes.syx`). MIT is irrevocable for everything already shipped. "Becoming proprietary" can therefore only mean: (a) *future* work kept out of the repo, (b) assets that never enter it, (c) non-copyright moats (trademark, services, private keys). `LICENSING.md`'s trademark section covers only Steinberg's VST mark; there is **no Tangent trademark claim anywhere**.

Candidates, ranked by value-per-goodwill:

1. **Name + visual identity (trademark) — do first, before 5.0.** MIT licenses code, not the mark. What it takes: a clearance search (note real collision risk — "Tangent" is an existing audio-hardware brand in the color-grading/controller space), a registration (~$250-350/class DIY), and a TRADEMARK.md stating that forks must rename. Goodwill cost: zero — this is the Ardour/Firefox norm, universally accepted in FOSS. This is the single move that makes "official binaries + supporter keys" a durable business even though the source is free, and it is the Ardour model the competitive analysis already ranks as Tangent's licensing peer. **Worth it, unambiguously.**

2. **Supporter-key/fulfilment infrastructure — already proprietary-in-practice.** The tools are public MIT, but the moat was never the code: it is the Ed25519 private seeds (in `~/.tangent-signing`, currently unbacked-up — an operational risk worth more attention than any licensing question) and the live Fly fulfilment service. Goodwill cost: zero; nobody expects payment plumbing either open or closed. **Worth it (it already is), and it is the payment rail every other candidate rides on.**

3. **DX7 patch banks — the cleanest paid-content candidate.** Bank 01 is already MIT-shipped; Banks 02+ simply never enter the repo. The authoring methodology exists (originality measured against 42,291 corpus voices, DX7-BANK-01.md) and is itself a private asset. Goodwill cost: near zero — free-core + paid-content is the community-validated model (Vital). Honest caveat: free DX7 cartridges number in the thousands, so banks sell on curation and originality; expect supporter-tier revenue, not a product line. **Worth it as supporter content, not as a standalone SKU.**

4. **Curricula/lesson content — the phase-2 asset, born proprietary.** Does not exist yet. The market research's structural gap (drill tools teach without making music; DAWs make music without teaching; only Hookpad occupies the middle, and it is a browser sketchpad with no audio engine) is Tangent's to take, and lesson content carries zero open-source expectation. Hookpad's $149-lifetime decade proves the price band. What it takes: genuine course design, and first closing the gap between *naming* theory and *teaching* it in-app (progression analysis, explanations at placement time). **Worth it, sequenced after the self-learner beachhead validates; it is also the only credible teacher-segment monetization.**

5. **An OBS companion (overlay/receiver) — proprietary only if it grows real value.** The plain ReaStream path must stay free (ReaStream itself is free; charging for parity buys resentment). But a companion that renders Tangent's *live chord names on stream* as an OBS overlay would be a unique, on-wedge, paid asset no one else can offer, because no one else has the detector. Goodwill cost: low if the audio path stays free. **Worth it only in that enriched form; skip the plain-receiver version.**

6. **Chord engine + corpus — keep MIT; do not attempt to close.** It is already published, it is the identity feature, and "free theory teaching" is the entire marketing story (the shareable moment the demand-signal tests are built around). Closing future engine work would fork the project against itself for negligible protection — the durable advantage is the iteration discipline (acceptance-as-contract, diffcorpus loop) and release velocity, neither of which a forker inherits. **Not worth it; the goodwill cost lands exactly on the beachhead segment.**

7. **Cloud services — highest goodwill risk; do last, and only optional.** "Fully offline, no account, no telemetry" is load-bearing positioning in a market that made Waves reverse a subscription in five days. Optional services (share links, a TheoryTab-style community chord database, classroom SSO/management) are proprietary by nature and fine *if* the desktop app never needs them. Classroom management is the one with a documented buyer expectation (teachers expect SSO/LMS), so it belongs to the phase-2 EDU motion. **Worth it only as strictly-optional add-ons, and only when a segment demonstrably pulls.**

**Summary:** the realistic moat stack for a permanently-MIT core is (a) trademark + official signed binaries, (b) content (banks now, curricula later), (c) the fulfilment rail and optional services, (d) development velocity. That is Ardour's model plus Vital's, both proven in this exact market — and steps (a) and the seed-backup fix in (b)/(2) are the only ones that belong on the 5.0 critical path.

---


# Marketing & Release Plan (Q4)

# Marketing & Release Plan: Tangent 5.0

This section assumes the verdicts of the three companion reports: the market research (lead with self-learners, amplify through creators, defer teachers, treat hobbyists as funnel), the Gumroad mechanics report (Option A: free app + paid Supporter listing, redeem-pull key fulfilment), and the competitive analysis (the only defensible wedge is theory + one-gesture capture + OBS, not "DAW replacement"). Windows signing is treated as a launch blocker, not a nice-to-have.

---

## 1. Positioning

### 1.1 Positioning statement (internal, drives all copy)

> Tangent is the free, 17 MB DAW that teaches you music theory while you play: it names your chords live, lets you correct it when it's wrong, and records your audio, MIDI, screen, and webcam as one honest take that streams straight into OBS. It is for people learning to write music and people who want to show that process on camera. It is not, and does not claim to be, a mixing or mastering environment.

Three pillars, in priority order (matches audience priority):

1. **Learn as you play.** Live chord detection and naming, user-teachable overrides (no other DAW has a correctable detector), theory panel, fretboard, staff view, 128-key strip.
2. **Capture everything in one gesture.** One key records audio + MIDI + composited 1080p30 screen-and-webcam video with an honest `take.json` manifest. No other DAW records video at all.
3. **Made for showing.** Native "Stream to OBS" (ReaStream-compatible sender), hold-H keybind overlay that makes every stream a tutorial, 16-18 MB installers, no account, no subscription.

### 1.2 One-liner options (pick one for the hero, reuse others per channel)

1. "The DAW that names your chords while you play them." (theory-led; best for r/musictheory, YouTube educators)
2. "Learn music theory by making music, not flashcards." (self-learner-led; best for landing page A/B test vs #1)
3. "One key records everything: your audio, your MIDI, and video of you playing it." (capture-led; best for creators)
4. "The only DAW with a Stream to OBS button." (streamer-led; best for Twitch/OBS communities)
5. "A whole DAW in 17 MB. Free. No subscription. No account." (hobbyist/anti-bloat; best for itch.io, HN, KVR)

Recommendation: run #1 vs #2 as the landing-page message test (research signal 1), and keep #5 as the permanent subhead since the Waves reversal showed "no subscription" is itself a marketing asset in this community.

### 1.3 Positioning rules

- Never position against REAPER/Logic/Live as a replacement. The honest matrix says Tangent is last or near-last in every traditional category except footprint, theory display, and price. Reviewers will run that matrix; get ahead of it.
- Publish a **"What Tangent is not (yet)"** section on the site: no automation lanes, no PDC, no comping, no time-stretch, 3 inserts per strip, VST3 only, notation display not editing. (Pre-launch task: verify this gap list against the 5.0 code before writing it; the audit capsule flags these as believed, not confirmed.) Honesty here converts skeptics and defuses the one predictable negative-review angle.
- The "built in 4 weeks" origin story is press-worthy for developer audiences (HN, r/rust) and poison for musician audiences (it reads as "immature"). Use it only in dev-channel posts; never on the product page.
- Copy style: plain language, short sentences, no em dashes anywhere in published text, no AI-flavored filler.

---

## 2. Pricing

### 2.1 The constraint that overrides everything

4.x shipped with a "free, stays free" welcome note and an offline Ed25519 supporter-key system. That is a promise with witnesses. The 5.0 pricing story must be an *extension* of that promise, never a revision. The Waves incident shows this community punishes perceived bait-and-switch within days; a solo dev with no goodwill reserve would not survive it.

### 2.2 Recommended: Free 5.0 + "Tangent Supporter" on Gumroad

- **Listing 1: Tangent 5.0** on Gumroad at `0+` (suggested $0), all three platform files attached (18/45/11 MB fits the 250 MB free-file cap). Purpose: email capture for update blasts, Discover visibility, a canonical download URL. Mirror the free download on the website and itch.io.
- **Listing 2: Tangent Supporter** at PWYW `5+`, **suggested $15**, optional $39 "name in the credits" variant. Sub-$5 floors are mostly fee ($0.50 + processing), and PWYW without a strong suggested anchor collapses average price, so both numbers matter.
- Enable Gumroad license-key generation; buyer redeems the Gumroad key at the Fly service, which verifies via `licenses/verify` and mints the real offline Ed25519 supporter key (redeem-pull as primary, Ping as accelerator, per the Gumroad report). This fixes the Reply-To black hole: the key appears on the redeem page and in a service-sent email, no reply required.
- **30-day money-back**, set explicitly (do not inherit Gumroad's silent default unknowingly). Refunded buyers keep working offline keys; at $15 that is an accepted cost and it suppresses chargebacks.
- What supporters get, stated plainly: the warm glow, the credits listing, the supporter badge/flair in-app if one exists, and the knowledge that the free promise is funded. Do not gate features. Expected math: ~$12.30 net on a $15 sale.
- Advertise the model in one sentence everywhere: **"Tangent is free and stays free. If it earns money for you or teaches you something, the Supporter key is how you say thanks."**

### 2.3 Alternative A: Free core + paid content (the Vital pattern)

Everything functional stays free; sell **content**: "Tangent Banks Vol. 1" ($19-25): additional DX7-style patch banks, drum-machine kits, templates, and 3-5 produced demo projects with commentary. Keeps the promise intact by the letter and the spirit (the 4.x app never bundled this content). More revenue per buyer than tips, and the DX7 patch-authoring pipeline already exists. Cost: real content-production time competing with dev time, and it starts a content treadmill. Viable as a **phase-2 addition on top of the Supporter model** rather than a true alternative.

### 2.4 Alternative B: Paid 5.0 ($19 fixed), 4.x stays free. Not recommended.

Cleanest revenue story, and Gumroad's paid bucket removes all hosting constraints. But it breaks "free, stays free" unless every existing user is grandfathered, and even a perfectly executed grandfather clause changes the product's identity from "free instrument" to "software with a paywall," forfeits the hobbyist funnel against BandLab's 100M-user $0 anchor, and makes every future major version a re-purchase argument. Choose this only if 5.0 were a discontinuity rather than a continuation, which it is not. Documented here so the audit shows it was considered and rejected on the promise, not overlooked.

---

## 3. Pre-launch checklist

### 3.1 Infrastructure (start immediately; these have the longest lead times)

| # | Item | Why it cannot wait |
|---|---|---|
| 1 | **Back up `~/.tangent-signing`** (encrypted, two locations) before touching fulfilment | The private seeds are currently unbacked-up; everything downstream depends on them |
| 2 | **Azure Artifact Signing, Basic** ($9.99/mo): create paid Azure sub (Individual billing matching government ID), AU10TIX identity validation, cert profile, wire `signtool` + dlib into the Windows build (sign inner exe, `!uninstfinalize`, `!finalize`; signing must run on the Windows box or a CI Windows job since makensis on macOS cannot invoke signtool) | Identity validation takes days; **SmartScreen reputation takes weeks of clean installs and accrues on the certificate**. Ship a **signed 4.93 point release 3-4 weeks before 5.0** so the cert walks into launch with reputation instead of starting the "unrecognized app" clock on launch day |
| 3 | **Gumroad account:** complete identity verification early (drops payout threshold from $100 to $10), set the 30-day refund policy explicitly, fill Discover category (Music & Sound Design → Plugins) and tags (DAW, VST, chord, MIDI, music theory) | Verification and payout plumbing should never be debugged during launch week |
| 4 | **Fulfilment service:** `/redeem` page + `licenses/verify` client + `sale_id` table + refund/dispute resource subscriptions on the Fly app; Ping endpoint as accelerator; decide seed placement (dedicated Gumroad-channel seed on the server for instant fulfilment, per the Gumroad report) | Mind the known `license` feature-flag trap when 5.0 wires key checking |
| 5 | **Name collision check:** one afternoon of trademark/search diligence on "Tangent" in audio (e.g. Tangent Wave control surfaces) before printing it on a paid listing | Cheap now, expensive after launch |
| 6 | **Verify the gap list** (automation, PDC, inserts, time-stretch, comping, notation editing) against the actual 5.0 build | The "honest limits" page and all review responses depend on it being accurate |

### 3.2 Assets

**Trailer (60-90 seconds, the single most important asset).** Shot list:
1. Cold open, no logo: hands on keys, chord plays, Tangent names it on screen ("Cmaj9"). Second chord, name updates live. (5-8 s)
2. Player corrects the detector (user-teachable override). Caption: "Disagree? Teach it." (5 s)
3. Hold H: full keybind overlay blooms. Tab cycles Band → Mixer → Arrange → MIDI. (8 s)
4. One keypress: count-in, take records; split-screen shows audio + MIDI + webcam video landing as one take, `take.json` briefly visible. Caption: "One key. Audio, MIDI, and video." (15 s)
5. "Stream to OBS" clicked; cut to an OBS scene receiving the master, meter moving. (8 s)
6. 20 seconds of an actual short piece being built (drum machine banks, DX7 patch, loop zone) so it sounds like music, not a feature list.
7. End card: "Tangent 5.0. Free. No subscription. 17 MB. macOS, Windows, Linux." URL.

Record the trailer *in* Tangent using its own video-take capture wherever possible; that fact belongs in the YouTube description.

**Supporting video:** one 6-8 minute "first session" walkthrough (zero to recorded take), and three 30-60 s vertical clips cut from the trailer material for TikTok/Shorts/Reddit (the chord-naming moment, the one-key take, the OBS button). The chord-naming clip is the shareability test from the market research (signal 3).

**Screenshots (8, per-platform where the chrome differs):** band strip with live chord name; theory panel + fretboard; MIDI editor; mixer with VST3 inserts; drum machine; H-overlay in full glory; a take with video thumbnail in Arrange; the OBS pairing. Light and dark if both exist.

**Demo projects (2), bundled in-app and downloadable:** one finished short song (uses DX7 banks + drum machine + built-in effects only, so it sounds identical for everyone), one "theory tour" project whose track names walk the user through a progression with the theory panel open.

**Docs:** a 10-minute quickstart (install → first take → export); a printable one-page keybind card (the H-overlay as PDF); an **"OBS in 5 minutes" guide** with screenshots of adding the ReaStream VST2 receiver (this doubles as SEO content: "how to stream your DAW to OBS" is a searched pain point); a plain-English FAQ covering the free promise, what supporter keys are, the expected first-weeks SmartScreen prompt on Windows ("it shows my verified name; here's why"), refunds, and the honest limits list.

**Website/landing page copy outline:**
1. Hero: chosen one-liner + trailer embed + three download buttons (auto-detect OS).
2. Three pillars, one screenshot + two sentences each.
3. "What Tangent is not (yet)": the gap list, linked to a public roadmap.
4. Supporter section: the one-sentence pitch, Gumroad link, credits wall.
5. Comparison strip, framed as *use case* not feature war ("Mixing an album? Get REAPER, it's excellent. Learning to write and showing it? That's Tangent.") Naming and praising the competitor buys credibility.
6. FAQ + press kit link (logo, factsheet with the 391-commits/141k-lines/16 MB numbers, screenshots, GIFs, contact).
7. itch.io page mirroring the above, with devlog enabled: itch is the discovery surface, Gumroad the checkout, per the platform research.

---

## 4. Launch sequence (solo-dev realistic: 6 weeks)

| Week | Theme | Actions | Gate to proceed |
|---|---|---|---|
| **1** | Infrastructure | All of §3.1: seed backup, Azure signing setup + validation, Gumroad verification, fulfilment endpoints, name check, gap verification. Start trailer script. | Signing validated; redeem flow works end-to-end in test mode |
| **2** | Signed soft release | Ship **signed 4.93** through existing channels (starts SmartScreen reputation). Put the free build + devlog on itch.io quietly. Build landing page with the two-framing A/B (signal 1). Record trailer footage. | 4.93 installs cleanly signed on a stock Win11 box incl. Smart App Control |
| **3** | Assets + seeding | Finish trailer, walkthrough, screenshots, demo projects, quickstart, OBS guide. **Outreach wave 1:** 15-20 mid-tier theory/production YouTubers (5k-100k subs) with keys + the 90-second trailer + a personal two-line note (signal 2). **Seed 5-10 Twitch Music streamers** with the OBS build and the 5-minute guide (signal 5). Email existing supporters/users via `announce` + in-app note: "5.0 is coming, still free, here's the trailer first." | ≥3 educator/streamer commitments, or one more outreach week |
| **4** | Pre-launch beta | "5.0 RC" to itch + email list as a public beta. Post the 60 s chord-naming clip to r/WeAreTheMusicMakers (weekly promo thread, the sanctioned path) and r/musictheory; measure purchase-intent comments and click-through (signal 3). Fix what the beta breaks. Tip Bedroom Producers Blog with the press kit (BPB drove the framing of every comparable free launch). | No release-blocking bugs for 5 days; landing conversion ≥5% on the winning framing |
| **5** | **Launch week** | Day 1: flip Gumroad listings live, publish site, email existing users first (they hear it from you, not Reddit). Day 1-2: itch devlog post, KVR announce thread, WATMM promo-thread post, trailer on YouTube. Day 3: dev-audience post ("Show HN: a 16 MB DAW in Rust that teaches music theory" + r/rust); this is where the 4-week/141k-line story is an asset and where the FreeBSD tester likely finds you. Day 4-5: coordinated educator/streamer coverage goes live (give partners the date in week 3). Reply to every comment for 72 hours; that presence is the marketing. | n/a |
| **6** | Consolidate | Round-up post ("launch week by the numbers", itch devlog + email). 5.0.1 bugfix release with the Gumroad Updates-tab email as the first demonstration of the update cadence. Log every "does it do X" question as roadmap input. Begin measuring signal 4 (itch free downloads ≥1,000/month, ≥2% supporter conversion). | n/a |

Total: five weeks of work and a launch on week 5, with signing lead time absorbed in weeks 1-2. Nothing in this plan requires a second person.

---

## 5. Channel plan per audience

### Self-learners (primary; the "theory-curious bedroom producer")
- **YouTube via mid-tier educators**, not cold-pitching Beato/Huang: 5k-100k-sub theory and beginner-production channels answer email; one honest adopter beats a sponsored placement (KVR threads are unambiguous on this). Offer keys, the trailer, and a no-strings "use it in your teaching content" invitation; an educator building lessons *inside* Tangent is the endgame.
- **Reddit:** r/WeAreTheMusicMakers (3.8M, weekly promo thread), r/musictheory (709k, the chord-naming clip framed as "I made the detector correctable, tell me where it's wrong", which invites the exact engagement that community enjoys), later r/piano for the 128-key strip + falling-notes adjacency.
- **KVR:** announce thread at launch, honest dev participation thereafter; enter the **next KVR Developer Challenge** with something Tangent-adjacent (the free app itself, or a free standalone chord-naming utility that funnels to Tangent). Winners get years of residual recommendations.
- **Bedroom Producers Blog:** the highest-leverage single pitch in this niche; tip in week 4 with the press kit.

### Creators/streamers (amplifier, not market)
- Direct seeding of Twitch Music streamers (§4 week 3); success metric is *repeat unprompted on-stream use*, not a shoutout.
- The DMCA angle is the pitch: original composition is the only strike-proof music content, and Tangent is the shortest path from "no theory" to "original music on stream". Write one blog post exactly on that framing for r/Twitch and streamer Discords.
- OBS ecosystem: the "OBS in 5 minutes" guide posted to OBS forums and r/obs answers a preexisting question ("how do I get DAW audio into OBS without Loopback/virtual cables") where Tangent is legitimately the easiest answer.
- TikTok/Shorts: the three vertical clips; beat-making and learning-piano-live content already circulates there.

### Hobbyists (free-tier funnel)
- itch.io is the actual browsing audience for approachable music tools (the Bosca Ceoil lineage); maintain the devlog as the project's public diary. Gumroad Discover with good tags is found money at 30%; never a strategy.
- No paid acquisition. Hobbyists convert to supporters through years of updates, not launch-week persuasion.

### Developers (bonus channel this plan adds)
- HN "Show HN" + r/rust in launch week. Musicians won't come from here, but testers, goodwill, the FreeBSD community, and press do. This is the only channel where the build-speed story leads.

### Teachers (explicitly phase 2; do not market to them at launch)
- The research is blunt: PD-driven discovery, per-student annual pricing with seat minimums, purchase orders, Chromebook fleets that a native desktop app cannot reach, and $0 incumbents. A launch-week teacher push would burn the one first impression.
- Phase-2 trigger (day 90): run 5 discovery calls recruited via the Midnight Music / Music Tech Teacher community; the go signal is an unprompted site-license request. Then: free teacher accounts + roughly $3-6/student/yr or a flat $150-300 lab license, pitched with self-learner testimonials, timed to the school-year buying window. Tangent's tiny Windows installer is genuinely lab-friendly where labs are PCs; Chromebook districts are out of reach and should be told so honestly.

---

## 6. The OBS/streaming angle, worked as a differentiator

This is the one category where the competitive analysis ranks Tangent first against all five majors, so it gets its own motion:

1. **Product truth:** no mainstream DAW captures video at all, and none has a native OBS path; REAPER ties only on the audio-send half. "The only DAW with a Stream to OBS button" is a checkable, defensible claim; keep it in exactly that falsifiable form.
2. **The flywheel:** every streamer who uses it displays it, on camera, to an audience of aspiring producers (precisely the primary segment), with the H-overlay turning every stream into an involuntary tutorial. Marketing spend here is free product, not money.
3. **The honest manifest as content:** `take.json` makes "I played this live in one take" *provable*. Pitch that to performance-oriented creators as an anti-fake-performance receipt; it is a story angle no competitor can copy quickly and it fuses the capture and streaming pillars.
4. **DMCA-safe by construction:** the segment's defining constraint (99% of the 2020 strike wave was background music) pushes streamers toward original composition; a DAW that teaches composition is the safe-content machine. Lead partner outreach with this sentence.
5. **Content commitments:** the OBS-in-5-minutes guide (SEO + forums), one "stream your first beat" video with a seeded partner, and a standing offer of keys to any Music-category streamer who asks.

---

## 7. Post-launch cadence

- **Releases:** a signed point release every 3-4 weeks for the first quarter (bugfixes + one visible improvement each), announced via the Gumroad Updates tab, `announce`, and an itch devlog entry. Buyers' Gumroad links always serve the newest files, so there is no stale-version support burden; compose update emails manually (the one-click notify button is known-flaky post-open-sourcing). The cadence itself is the answer to the predictable "solo dev, 4-week-old project, will it be abandoned?" objection; say so out loud: "release every month, changelog public."
- **Feedback loops:** one canonical public channel (a small Discord or a forum, not both), a "report a problem" link inside the H-overlay, itch comments, and a weekly one-hour triage that feeds a public roadmap page. Answer "when automation/PDC/comping?" with roadmap positions, never dates.
- **The FreeBSD tester as community story:** FreeBSD support is planned and there is one committed, high-value tester. Make that the project's flagship community narrative: name them (with consent) as the port champion, run the port as a public devlog series ("porting a 141k-line Rust DAW to FreeBSD"), and when it ships, announce in the FreeBSD Forums, r/freebsd, and the BSD podcast circuit. The audience is small; the story ("the DAW so light and native it runs on FreeBSD, because one user asked") is not. It reinforces the anti-bloat identity for every platform's audience, it is dev-channel catnip, and it models the behavior you want: users who show up get engraved into the project. Do not gate any release on it.
- **Quarterly:** re-run the numbers against §9, decide the teacher phase-2 go/no-go at day 90, consider the content pack (§2.3) once monthly Supporter revenue plateaus, and enter the next KVR DC.

---

## 8. Risks and what not to do

1. **Do not touch the free promise.** No paywalled features, no "free tier" language (Tangent is not a tier, it is the product), no launch discount mechanics that make early supporters feel like suckers. The Waves reversal is the case study in how fast this community turns.
2. **Do not launch Windows unsigned.** The do-nothing path is an Edge interstitial plus a full-screen "Windows protected your PC" block with the escape hidden behind "More info", repeated from zero on every release, and a hard no-run on Smart App Control machines. For a paid-adjacent product that reads as malware. $10/month ends it; the signed-4.93-first sequencing (§4) is what makes launch week clean.
3. **Do not posture as a REAPER/Logic alternative.** Every traditional-category comparison is a losing fight invited voluntarily. The wedge framing wins by refusing the war.
4. **Do not let reviewers discover the gaps.** Publish them first. "No automation lanes" found by a reviewer is a scandal; on your own limits page it is scoping.
5. **Do not cold-pitch mega-YouTubers or buy placements.** Documented waste (KVR threads); the budget is product keys and personal notes to people who answer email.
6. **Do not build the plan on Gumroad Discover** (flat 30%, no selective opt-out for public products) **or on Product Hunt** (tech-generalist audience; a good day there is not product-market fit).
7. **Do not set a paid floor below $5** (the two fixed fees eat it) **or run PWYW without a suggested price** (average collapses without the anchor).
8. **Do not promise dates for FreeBSD, automation, or PDC.** Roadmap positions only. A solo dev's credibility is a non-renewable resource.
9. **Do not skip the trademark check** on "Tangent" before money changes hands under that name.
10. **Do not let launch-week support drown development:** the FAQ, the SmartScreen explainer, and the 30-day no-questions refund policy exist to keep the inbox survivable; friendliness is also the cheapest chargeback suppression.
11. **Do not ship the launch with the signing seeds unbacked-up.** This is the single highest-consequence, lowest-effort item in the plan.

---

## 9. Success metrics (from the validated demand signals)

| Signal | Threshold | Decision it drives |
|---|---|---|
| Landing-page A/B conversion | ≥5% visitor→email on either framing | Winning framing becomes launch copy |
| Micro-educator commitments | ≥3 unprompted, or any educator teaching *with* it | Launch-week coverage locked; else extend outreach one week |
| Theory-clip shareability | Unsolicited purchase-intent comments + cross-posts on r/WATMM / r/musictheory | Confirms the chord-naming moment as the hero asset |
| itch free funnel | ≥1,000 downloads month one, ≥2% supporter conversion | Validates the Vital-style funnel at this price point |
| Streamer retention | Second unprompted on-stream session; viewers asking "what is that" | Doubles down on creator seeding vs reallocating the time |
| Teacher pull (day 90) | An unprompted site-license request from discovery calls | Go/no-go on the phase-2 EDU motion |

Bottom line: 5.0 launches as the same promise 4.x made, wearing better clothes: free and staying free, now signed on Windows, sold nothing except gratitude on Gumroad, and marketed on the three things no competitor can say: it names your chords, it records the whole moment in one keypress, and it streams itself.

---


# Competitive Analysis — REAPER, Ardour, Ableton, Pro Tools, Logic (Q3 evidence)

# Tangent vs. the Field: REAPER, Ardour, Ableton Live, Pro Tools, Logic Pro

Research date: 2026-08-25. Prices verified against vendor pages and current 2026 coverage (sources at bottom). Tangent capsule facts taken as given; competitor claims verified via web research. Ardour 9.8 shipped literally today (2026-08-25) and its new "scale tracking" feature is factored in.

---

## 1. The Matrix

| Category | **Tangent 4.92** | **REAPER 7.79** | **Ardour 9.8** | **Ableton Live 12** | **Pro Tools 2026** | **Logic Pro 11** |
|---|---|---|---|---|---|---|
| **Price / licensing** | Free; optional offline Ed25519 supporter keys | $60 discounted / $225 commercial, one-time, honor-system 60-day full trial; license spans 2 major versions | Pay-what-you-can from $1 for official binaries (GPL source free); optional support subs up to $120/yr | Intro $99, Standard $349, Suite $749, one-time perpetual; Suite rent-to-own $31.21/mo; free Live Lite via hardware bundles | Intro free (8 audio/8 inst/8 MIDI tracks); Artist $9.99/mo or $99/yr, perpetual $199; Studio $34.99/mo or $299/yr, perpetual $599 | $199.99 one-time (Mac App Store), 90-day trial; or Apple Creator Studio bundle sub $12.99/mo / $129/yr (edu $2.99/mo) |
| **Platforms** | macOS, Windows, Linux (ALSA); FreeBSD planned | macOS, Windows, Linux, all official | Linux (first-class), macOS, Windows | macOS, Windows only | macOS, Windows only | macOS only (+ iPad variant) |
| **Learning curve** | Shallow; keyboard-first, hold-H keybind overlay, Tab view cycling, small surface | Steep; utilitarian UI, huge customization burden, no content | Moderate-steep; engineer-oriented | Moderate; two-view model, built-in interactive lessons, free Learning Music / Learning Synths web apps | Steep; modal editing, industry conventions | Moderate; GarageBand on-ramp, deep beyond |
| **Theory / learning aids** | Live chord detection + naming, user-teachable chord overrides, theory panel, fretboard view, staff view (display), 128-key strip | None native (third-party scripts only) | NEW in 9.8: per-track key/scale, out-of-scale highlighting, force-to-scale; no chord naming | Global key/scale awareness across devices, MIDI Transform/Generate tools (Strum, Ornament, Connect, Recombine), scale-aware Chord/Arp; no live chord-name readout | Essentially none (Score editor exists but no chord/scale intelligence) | Chord Track (detects chords from MIDI regions, editable), Session Players follow chords, scale quantize, full notation EDITING in Score editor |
| **Composition (MIDI + instruments)** | 16 ch + drum track, 16-pad drum machine w/ banks, DX7-style FM synth + patch banks, MIDI editor, .mid takes | Excellent MIDI editor; only utility instruments (ReaSynth etc.) | Capable MIDI, new arpeggiator tools in 9.8; near-zero bundled instruments (GM SoundFont player) | Best-in-class clip/MIDI workflow; Suite: 17+ instruments incl. Operator FM, Wavetable, Meld, Max for Live | Decent MIDI (improving); modest bundled synths (SynthCell, GrooveCell, Xpand!2 lineage) | Massive: Alchemy, Sampler, Sculpture, Drummer + AI Session Players (Bass/Keys), step sequencer, Live Loops, up to ~72 GB sounds |
| **Recording workflow** | Always-visible band strip, one-key takes (audio+MIDI+video) with take.json manifest, loop zones, count-in, click, ASIO single-client duplex + device auto-recovery; no comping, no PDC | Takes + comping, fixed lanes (v7), autopunch, input FX, full PDC | Takes via playlists, punch, hardened count-in + crash recovery (max 5 s loss, 9.8), full PDC | Comping (since 11), session capture, warping, full PDC | Gold standard: playlist comping, QuickPunch/TrackPunch, Beat Detective, Elastic Audio, full PDC | Take folders + quick-swipe comping, Flex Time, low-latency mode, full PDC |
| **Video capture / creator-streamer integration** | Composited 1080p30 screen+webcam video takes recorded natively, honest manifest; built-in ReaStream-compatible network sender to OBS | Video import/playback/edit/render, video processor FX; ships ReaStream VST; NO camera/screen capture | Video timeline import + soundtrack export (xjadeo); no capture | Video import for scoring, video export; no capture, no native OBS path | Video track/engine for post sync; no capture | Movie track import, export audio to movie; no capture |
| **Extensibility (plugin formats)** | VST3 only; fixed 3 inserts/strip + master rack | VST2/VST3, CLAP, LV2, AU (mac), JSFX, ReaScript (Lua/EEL/Python), ARA2 | VST2/VST3, AU (mac), LV2, Lua scripting; CLAP still roadmap-only | VST2/VST3, AU (mac), Max for Live | AAX only (+ ARA2) | AU only |
| **Footprint / install size** | ~16-18 MB installers | ~15.5 MB installer, decades-tuned runtime | Hundreds of MB | Multi-GB base; Suite library ~tens of GB (up to ~76 GB) | Multi-GB app + content | ~1 GB app; 6 GB min, ~72 GB full library |

---

## 2. Per-Category Verdicts

**Price & licensing — WINNER: Tangent and Ardour (tie), asterisk for Pro Tools Intro.**
Tangent is unconditionally free with no track limits; supporter keys are optional and offline (no account, no phone-home). Ardour matches at $1+ and adds GPL source. Pro Tools Intro is free but crippled at 8 tracks. Among paid full DAWs, REAPER's $60 honor-system license is the value benchmark. Tangent places: first, tied with Ardour, with the honest caveat that "free from a 4-week-old solo project" carries longevity risk a reviewer would flag.

**Platforms — WINNER: REAPER.**
REAPER and Ardour both cover macOS/Windows/Linux officially; REAPER wins on polish parity across all three plus a decade of Linux-audio community tooling. Tangent places: third, genuinely cross-platform including Linux (ALSA), which already beats Ableton (no Linux), Pro Tools (no Linux), and Logic (macOS only). ALSA-only is thinner than Ardour's JACK/PipeWire-native citizenship, and FreeBSD is still vapor until shipped.

**Learning curve & onboarding — WINNER: Tangent (with a caveat), runner-up Logic/Ableton.**
The hold-H full-keybind overlay is a legitimately excellent onboarding mechanic none of the five majors have; Tab view cycling plus a small feature surface means a beginner is recording in minutes. The caveat a reviewer would state: it is easy partly because it does less. Among full DAWs, Logic (GarageBand on-ramp) and Ableton (built-in interactive lessons) lead; REAPER and Pro Tools trail badly.

**Music theory / learning aids — WINNER: Tangent for detection and teaching; Logic/Ableton for acting on theory.**
This is Tangent's lineage and it shows: live chord detection with naming as you play, user-teachable overrides (no other DAW lets you correct the detector), theory panel, fretboard, staff display, 128-key strip. Nobody else does real-time chord naming natively: Ableton's scale awareness colors notes and constrains devices but never names what you played; Logic's Chord Track detects chords from recorded MIDI regions (offline, not live) and its Session Players then follow them; Ardour just gained basic scale highlighting today (9.8); REAPER and Pro Tools have nothing. Where Tangent loses inside this category: Logic and Ableton can generate and transform music from theory (Session Players, MIDI Transform/Generate), and Logic and Pro Tools have actual notation editing where Tangent only displays.

**Composition workflow — WINNER: Logic Pro (value) / Ableton Suite (workflow).**
Logic's instrument+content library at $199.99 is untouchable; Ableton's clip-based writing flow and MIDI generators are the producer favorite. Tangent places: fifth of six, ahead of Ardour. One DX7-style FM synth with banks plus a 16-pad multi-bank drum machine is more musical out-of-box than Ardour's GM SoundFont player or REAPER's utility synths, but REAPER's MIDI editor depth still outclasses a 4-week-old editor.

**Recording workflow — WINNER: Pro Tools.**
Playlist comping, punch modes, and editing conventions remain the studio standard. Logic and REAPER follow, then Ableton, then Ardour (whose 9.8 crash-recovery guarantee is a notable reliability point). Tangent places: last on classic multitrack criteria (no comping, no PDC), BUT its capture concept is unique: one gesture records audio + MIDI + composited video with a manifest, and its ASIO single-client duplex + device auto-recovery is engineering the majors do not advertise. For "capture the moment" use, it is arguably first; for "produce an album," last.

**Video capture & creator/streaming integration — WINNER: Tangent, decisively.**
No DAW among the five records webcam or screen video, period; they all only import video for scoring/post. Tangent natively records composited 1080p30 screen+webcam video takes synced to audio and MIDI. On OBS interop, REAPER ties (it ships ReaStream, the very protocol Tangent speaks), and Tangent's sender is interop-validated against OBS's ReaStream VST2 receiver. Ableton/Logic/Pro Tools/Ardour require third-party loopback audio drivers to feed OBS at all. This is the single category where a fair reviewer would rank Tangent first against all five.

**Extensibility — WINNER: REAPER.**
VST2/3 + CLAP + LV2 + AU + JSFX + ReaScript + ARA2 is the broadest surface in the industry. Tangent places: last. VST3-only with a fixed 3 inserts per strip is more restrictive than even Pro Tools' AAX-only (which has a deep pro ecosystem) or Logic's AU-only (ditto). No CLAP, no scripting, no ARA.

**Resource footprint & install size — WINNER: REAPER, by a hair.**
15.5 MB and 20 years of runtime tuning edges Tangent's 16-18 MB. Tangent places: second, and both are an order of magnitude (or three) below everyone else. Being within rounding error of REAPER's famously tiny installer while bundling an FM synth, drum machine, and video capture is a genuine engineering flex.

---

## 3. Where Tangent BEATS specific competitors TODAY (strict)

1. **Integrated screen+webcam video takes: beats all five.** None of them can capture video at all; this is not close.
2. **Native OBS/streamer audio path: beats Ardour, Ableton, Pro Tools, Logic; ties REAPER.** Built-in ReaStream-compatible sender vs. their nothing; REAPER ties because it ships ReaStream itself.
3. **Live chord detection and naming (theory display): beats REAPER, Ardour, and Pro Tools outright; beats Ableton and Logic on real-time detection specifically.** User-teachable chord overrides beat all five (nobody else has a correctable detector). Ableton and Logic still win the broader "do something with theory" fight.
4. **Price: beats REAPER, Ableton, Pro Tools Artist/Studio, Logic.** Ties Ardour and Pro Tools Intro on sticker, beats PT Intro on limits (no 8-track cap).
5. **Linux support: beats Ableton, Pro Tools, and Logic** (which have zero); loses to REAPER/Ardour on Linux maturity.
6. **Install size: beats Ardour, Ableton, Pro Tools, Logic** by 1-3 orders of magnitude; ties REAPER.
7. **Onboarding discoverability (H-overlay of every keybind): beats all five as a mechanic**, with the honest footnote that the app's smaller surface does part of the work.
8. **Bundled instruments vs. Ardour specifically: beats Ardour.** A DX7-style FM synth with patch banks plus a multi-bank drum machine vs. a GM SoundFont player.

## 4. Where Tangent clearly LOSES, and to whom

- **Extensibility: loses to all five.** VST3-only, fixed 3 inserts/strip, no AU/VST2/CLAP/LV2, no scripting, no ARA. Worst-in-field.
- **Mixing/production depth: loses to all five.** No automation lanes and no plugin-delay compensation are disqualifying for serious mixing in 2026; every competitor has both.
- **Audio editing: loses to all five.** No time-stretch (REAPER, Ardour, Live's Warp, PT Elastic Audio, Logic Flex all have it), no comping (all five have it).
- **Composition content/instruments: loses to Logic, Ableton Suite, and REAPER's editor depth**; Pro Tools too on bundled-synth breadth.
- **Recording (traditional multitrack): loses to Pro Tools, Logic, REAPER, Ableton, Ardour** on comping/punch/editing conventions.
- **Notation: loses to Logic and Pro Tools** (both EDIT notation; Tangent displays only).
- **Track/architecture ceiling: loses to everything above Pro Tools Intro.** 16 channels + drum track vs. 32-2048 track architectures.
- **Ecosystem/longevity: loses to all five.** 391 commits, one developer, four weeks old; no third-party tutorials, templates, courses, or hiring market. A reviewer would weight this heavily.

### Bottom line
Tangent cannot be sold as a REAPER/Logic replacement and should not try. Its defensible, reviewer-conceded wedge today: **the free, 17 MB, theory-teaching, Linux-friendly DAW that is the only one on the market that records your screen, webcam, audio, and MIDI as a single honest take and pipes straight into OBS.** In that framing it has no direct competitor; in every traditional DAW category except footprint, theory display, and price, it is currently last or near-last.

---

## Sources

- [REAPER purchase page](https://www.reaper.fm/purchase.php) ($60/$225 one-time); [TechSpot REAPER 7.79 download](https://www.techspot.com/downloads/2642-reaper.html) (~15 MB installer)
- [Ableton Live 12 shop](https://www.ableton.com/en/shop/live/) (Intro $99 / Standard $349 / Suite $749, rent-to-own $31.21/mo); [Ableton Live 12 MIDI Tools FAQ](https://help.ableton.com/hc/en-us/articles/11535349458588-MIDI-Tools-and-Device-Updates-in-Live-12-FAQ); [Keys and Scales in Live 12 FAQ](https://help.ableton.com/hc/en-us/articles/11425083250972-Keys-and-Scales-in-Live-12-FAQ)
- [Sound On Sound: Pro Tools perpetual vs subscription](https://www.soundonsound.com/techniques/pro-tools-perpetual-licence-or-subscription-plan) (Artist $99/yr, $199 perpetual; Studio $299/yr, $599 perpetual); [Avid Pro Tools](https://www.avid.com/pro-tools); [Production Expert: Which Pro Tools in 2026](https://www.production-expert.com/production-expert-1/avid-pro-tools-which-version-is-the-right-one-for-me-in-2026) (tier/track limits)
- [Logic Pro on the Mac App Store](https://apps.apple.com/us/app/logic-pro/id634148309) ($199.99); [Logic Pro Cost in 2026](https://logicpros.net/how-much-does-logic-pro-cost/) (Apple Creator Studio $12.99/mo / $129/yr); [Apple Newsroom: Logic Pro 11](https://www.apple.com/newsroom/2024/05/logic-pro-takes-music-making-to-the-next-level-with-new-ai-features/) (Session Players, Chord Track, Stem Splitter)
- [Ardour subscription/download page](https://community.ardour.org/subscribe) (pay-what-you-can from $1, GPL); [Ardour 9.8 release coverage](https://www.linuxcompatible.org/story/ardour-98-released-scale-tracking-crash-recovery-and-clip-overhaul) (scale tracking, crash recovery, released 2026-08-25); [Ardour roadmap](https://ardour.org/future.html) (CLAP still future)
- Video-capture absence across DAWs: [Gearspace: webcam + DAW](https://gearspace.com/board/music-computers/581834-using-webcam-amp-daw-simultaneously.html), [The Gear Forum: DAW with audio and video](https://thegearforum.com/threads/daw-with-audio-and-video.1882/) (import-for-scoring only; no major DAW captures camera/screen)

---


# Market & Audience Research (Q1/Q4 evidence)

# Market Research: Beginner-Friendly, Theory-Teaching, Creator-Oriented DAW at Indie Pricing

Audiences assessed: **music teachers**, **self-learners**, **creators/streamers**, **hobbyists**. Research conducted via web search, August 2026.

---

## 1. Where these audiences discover tools

### Self-learners and hobbyists: YouTube first, Reddit second

- **YouTube theory educators are the discovery engine.** The channels repeatedly cited as where producers actually learn theory: David Bennett Piano (song-analysis format), Michael New, Andrew Huang, Rick Beato, Adam Neely, and production-side channels like Produce Like A Pro ([Hello Music Theory's roundup](https://hellomusictheory.com/learn/best-music-theory-youtube-channels/), [School of Composition's 60+ channel list](https://www.schoolofcomposition.com/music-theory-composition-youtube-channels/)). On the production side, beginner-DAW channels like In The Mix (200k+ subscribers at last count, likely far larger now; the go-to FL Studio beginner channel) are where absolute beginners pick their first DAW ([Your Best Channel](https://yourbestchannel.com/fl-studio-software-tutorial-youtube-channels/)). Notably, [Porter Robinson said an xQc video inspired his FL Studio tutorials](https://www.sportskeeda.com/esports/news-porter-robinson-reveals-xqc-s-video-inspired-make-fl-studio-tutorials), which shows how much the streamer/producer worlds already cross-pollinate.
- **Reddit sizes:** [r/WeAreTheMusicMakers has ~3.8M members](https://gummysearch.com/r/WeAreTheMusicMakers/) and "frequently recommends/reviews DAWs and music software"; [r/musictheory ~709k](https://gummysearch.com/r/musictheory/); [r/piano ~1.3M](https://gummysearch.com/r/piano/). WATMM has weekly promo threads, which is the sanctioned path for an indie launch post.
- **KVR Audio:** still the credibility hub for audio software. Developer threads there conclude that community-first beats reach-first for small devs: "an honest post from someone with 2,000 followers who genuinely uses the plugin daily outperforms a sponsored video from someone with 200k followers," and that YouTubers mostly ignore cold outreach ([KVR marketing thread](https://www.kvraudio.com/forum/viewtopic.php?t=623713), [DSP forum thread](https://www.kvraudio.com/forum/viewtopic.php?p=9209869)). The [KVR Developer Challenge](https://www.kvraudio.com/kvr-developer-challenge/) (56 free entries in 2026, a record) is a proven zero-cost exposure event; winners get years of residual recommendations ([BPB coverage](https://bedroomproducersblog.com/2026/07/08/kvr-developer-challenge-26/)).
- **Bedroom Producers Blog** deserves explicit mention: it drove the framing of nearly every free/indie launch found in this research (Vital, KVR DC, etc.).
- **Product Hunt** works for browser/AI-flavored music tools (Mozart AI took Product of the Day #2) but the audience is tech-generalist, not musicians, and PH itself warns a #1 ranking "means you resonated on a specific day, not product-market fit" ([PH traction metrics](https://www.producthunt.com/stories/8-traction-metrics-from-successful-product-hunt-launches), [Mozart AI](https://www.producthunt.com/products/mozart-ai)). Useful for the "creator tool" framing, weak for the teacher/musician framings.
- **itch.io vs Gumroad:** itch has an actual browsing audience for music tools ([233+ VST-tagged tools](https://itch.io/tools/tag-vst)) plus community/devlog mechanics and organic discovery; Gumroad is a transactional storefront with simpler payments and no built-in audience ([itch community discussion](https://itch.io/t/2760445/itch-vs-gumroad), [fluffy's platform comparison](https://beesbuzz.biz/blog/4910-On-bandcamp-gumroad-and-itch.io)). Some devs have migrated off itch over payout-withholding incidents ([one dev's account](https://soundfontguy.itch.io/guyengine/devlog/1031123/a-frustrating-announcement)). Practical read: itch page for discovery + devlogs, own-site or Gumroad for the actual paid checkout. Bosca Ceoil-style tools are exactly what itch's audience browses for.

### Teachers discover tools completely differently

Teachers do not read KVR. They discover software through **PD ecosystems**: the [Midnight Music / Music Tech Teacher podcast](https://midnightmusic.com/music-tech-teacher-podcast/) (Katie Argyle, 100+ conferences, "a professional development gold mine" per listeners, publishes an annual guide of 350+ free tech resources), teacher blogs comparing tools head-to-head ([Soundtrap vs BandLab for schools](https://midnightmusic.com/2026/03/music-tech-soundtrap-vs-bandlab/)), conference circuits (NAMM/TMEA/state MEA), Teachers Pay Teachers (where Hooktheory-related classroom materials circulate), and vendor EDU programs. Reaching them is a separate marketing motion with a school-year calendar.

---

## 2. Comparable indie audio launches and observable outcomes

| Product | Model | Observable outcome |
|---|---|---|
| **Vital** (Matt Tytel, solo dev, 2020) | Free full-engine tier + $25/$80 content tiers + $5/mo sub | "Biggest freeware VST release of the year," instant must-have status, genuinely competes with $189 Serum; the free tier is the whole engine, paid tiers are presets/wavetables ([BPB](https://bedroomproducersblog.com/2020/11/24/vital/), [MusicTech](https://musictech.com/news/matt-tytel-vital-free-synth/)). The canonical proof that free-core/paid-content works in this exact market. |
| **Scaler 2** (Plugin Boutique) | ~$49-59 one-time | "A huge success for Plugin Boutique"; users include Jean-Michel Jarre and Carl Cox ([MusicTech](https://musictech.com/news/plugin-boutiques-scaler-2/)). Proof that **theory-assistance is a purchasable category at ~$50**, even for pros. |
| **Hooktheory / Hookpad** | $4.99/mo, $49/yr, or $149 lifetime; Aria AI add-on $14.99/mo | Running since 2013; TheoryTab community database of 13,000+ analyzed songs; testimonials like "grasped more in a week than 8 years of classical training" ([Hookpad pricing](https://www.hooktheory.com/hookpad/pricing), [Produce Like A Pro review](https://producelikeapro.com/blog/hooktheory-hookpad-review/)). The closest existing "theory-teaching sketchpad," and it sustains a small company. |
| **Bosca Ceoil / Bosca Ceoil Blue** (Terry Cavanagh / Yuri Sizov) | Free, open source, itch + Steam | The reference beginner music tool on itch; the Blue port added Steam distribution ([itch page](https://yurisizov.itch.io/boscaceoil-blue), [Steam](https://store.steampowered.com/app/3570850/Bosca_Ceoil_The_Blue_Album/)). Shows itch/Steam are viable channels for approachable music tools, and that "beginner-friendly music making" has durable pull at $0. |
| **Tracktion Waveform Free** | Free unlimited DAW → $119 Pro + à-la-carte feature expansions from $30 | The established free-DAW-as-funnel play ([Tracktion](https://www.tracktion.com/products/waveform-free), [Dubspot](https://blog.dubspot.com/tracktion-waveform-free-14-2026)). |
| **REAPER** | $60 discounted license, enormous version coverage | The benchmark for "fair one-time indie DAW pricing"; repeatedly cited as the value option ([DJ.Studio price guide](https://dj.studio/blog/daw-price-guide-subscriptions-student-deals)). |
| **Synthesia** (piano game) | ~$40 one-time unlock | 10M+ Android installs; the standard self-learner "falling notes" tool ([Google Play](https://play.google.com/store/apps/details?id=com.synthesia.synthesia), [Synthesia site](https://synthesiagame.com/)). Proof self-learners pay ~$40 once for learning software. |
| **Melodics** | $9.99/mo or ~$99/yr subscription | Sustained gamified-practice business across keys/pads/drums ([ProSoundPicks review](https://prosoundpicks.com/melodics-review/)). Shows subscription works when content (lessons/songs) keeps flowing. |
| **BandLab** (the competitive anchor) | 100% free | [100M+ users as of March 2024](https://www.billboard.com/business/tech/bandlab-100-million-users-report-1235637853/), ~30% US. The free ceiling any paid beginner DAW must position against. |

**Pattern:** the successful indie plays are either (a) free core + paid content/features (Vital, Waveform), or (b) a sharply-scoped one-time purchase in the $40-150 band (Scaler, Synthesia, Hookpad lifetime, REAPER). Nobody small wins with a mandatory subscription against BandLab/GarageBand at $0.

---

## 3. Price sensitivity by audience

**Self-learners:** strongly prefer perpetual licenses. The 2023 Waves incident is the clearest market evidence: Waves went subscription-only and reversed within ~5 days after community revolt ([MusicTech](https://musictech.com/news/industry/waves-subscription-only-u-turn-licenses/), [RA](https://ra.co/news/78778), [Waves' own climb-down](https://www.waves.com/news/perpetual-waves-licenses-are-back)). Observed willing-to-pay anchors: $40 (Synthesia), $49-59 (Scaler), $60 (REAPER), $99 (FL Studio entry, with the beloved "lifetime free updates" promise), $149 (Hookpad lifetime). A $29-79 one-time price with free updates sits squarely in the accepted band.

**Teachers:** budget exists but flows through per-student annual licensing with seat minimums and purchase orders. Norms: [Soundtrap EDU ~$8-10/student/yr with a 50-student minimum](https://midnightmusic.com/2026/03/music-tech-soundtrap-vs-bandlab/); [Auralia/Musition cloud priced per student, 10-seat minimum, annual](https://www.bhphotovideo.com/c/product/1276231-REG/rising_software_aucel_bun_auralia_5_cloud_bundle.html); [uTheory $3-10/student/yr with free teacher accounts](https://info.utheory.com/pricing-changes-for-utheory/); [Flat for Education $99/teacher + $6/student](https://blog.flat.io/best-music-software-k12-music-classrooms/). Free anchors are brutal here too: BandLab EDU and GarageBand are $0, and Chromebook fleets dominate US classrooms, which disadvantages any native desktop app ([Midnight Music comparison](https://midnightmusic.com/2022/06/should-i-use-garageband-soundtrap-or-bandlab-with-my-students/)). Teachers also expect Google Classroom/SSO integration. A realistic EDU model: free teacher account + ~$3-6/student/yr or a flat ~$150-300 lab/site license, but only if the product runs on school hardware.

**Creators/streamers:** most tolerant of paying because tools are business inputs, but the segment is small: Twitch's Music category averages only ~7,600-9,000 concurrent viewers across ~575 channels ([TwitchTracker](https://twitchtracker.com/games/26936)). They'll pay $50-100 one-time for something that visibly improves streams.

**Hobbyists:** anchored at $0 by BandLab (100M users), GarageBand, Waveform Free, Cakewalk, and Vital's free tier. They convert only through a generous free tier plus an impulse-priced unlock (≤$30) or via sales events (Plugin Boutique/KVR deal culture).

---

## 4. The music-education software landscape, and where a theory-DAW fits

Current teacher stack for theory:

- **Drill/assessment tools:** [Musition + Auralia](https://www.risingsoftware.com/shop/education) (the institutional standard, per-seat cloud licensing), [musictheory.net](https://www.musictheory.net/products/tenuto) (free web lessons/exercises; $4.99 Tenuto app; teachers assign via custom exercise links), Teoria (free web), [uTheory](https://utheory.com/teach) (lessons + video + adaptive practice, free for teachers).
- **Creation-adjacent theory:** [Hooktheory Classroom](https://www.hooktheory.com/education) (Hookpad + theory book + assessment kit; course codes, under-13 accommodations, but limited: one teacher account per course).
- **Classroom DAWs (no theory instruction):** [Soundtrap EDU](https://edu.soundtrap.com/finding-the-best-daw-software-for-students/) (paid, LMS/SSO), BandLab EDU (free), GarageBand (free, Apple-only).
- **Free theory-through-play:** [Ableton Learning Music](https://learningmusic.ableton.com/) (browser, exports to Live) and Chrome Music Lab, both widely used as free classroom warm-ups.

**The structural gap is real:** drill tools (Musition, Tenuto, uTheory) teach theory without making music; classroom DAWs make music without teaching theory; teachers currently bridge this manually. Hookpad is the only serious occupant of the middle and it is a browser sketchpad, not a DAW (no real audio engine, no recording, no plugins). Ableton's Learning Music proves the pedagogy-inside-a-production-tool concept but deliberately dead-ends into a €349+ product. A DAW that *is* the theory curriculum (chords explained as you place them, keys/scales enforced-but-explained, progression analysis in the arrangement itself) has no direct incumbent at indie pricing.

---

## 5. Content-creator crossover (the OBS angle)

- Music streaming on Twitch is a modest but real niche (~9k average concurrent viewers, 76M+ hours watched YTD 2025, weekend-skewed, [TwitchTracker](https://twitchtracker.com/games/26936), [SullyGnome](https://sullygnome.com/game/music)).
- **DMCA is the niche's defining constraint:** 99% of Twitch's 2020 DMCA wave was background music; playing covers or Spotify on stream risks strikes, so *original composition on stream is the safe content* ([ecopyright guide](https://www.ecopyright.io/blog/twitch-livestream-creator-copyright/), [Meld Studio guide](https://meldstudio.co/blog/live-stream-music-without-getting-a-dcma-strike/)). A DAW that teaches you to write your own music is inherently DMCA-safe streaming content.
- **Audio routing into OBS is a known pain point** solved today with Loopback, virtual cables, or hardware kludges ([Ableton forum thread](https://forum.ableton.com/viewtopic.php?f=1&t=214537), [Hypebot's beginner guide to streaming production](https://www.hypebot.com/hypebot/2021/02/beginners-guide-for-streaming-music-production-on-twitch.html), [OBS audio routing guide](https://blog.b2fab.com/2021/10/understanding-audio-mixing-and-routing-in-obs/)). A native "Stream to OBS" button is a genuine differentiator no mainstream DAW ships, and it doubles as a *marketing flywheel*: every streamer using it shows the product on camera. Beat-making and "learning piano live" content already exists across Twitch/TikTok ([TikTok FL-streaming discovery pages](https://www.tiktok.com/discover/how-to-stream-fl-studio-on-twitch)).

The creator segment is too small to be the market, but it is the highest-leverage *distribution* channel: creators are self-learners who broadcast their tool choices.

---

## 6. Audience prioritization recommendation

**Lead with self-learners (the "theory-curious bedroom producer"), amplified through creators. Defer teachers to a phase-2 EDU motion. Treat hobbyists as the free-tier funnel, not a revenue segment.**

Reasoning:

1. **Self-learners are the largest reachable audience with proven willingness to pay this exact price band.** 3.8M in r/WATMM + 709k in r/musictheory, discoverable through the same YouTube/Reddit/KVR/BPB channels an indie dev can actually access, with demonstrated $40-150 one-time purchases for theory/learning tools (Scaler's "huge success," Synthesia's 10M installs, Hookpad's decade of business). The pain ("I make beats but don't know theory") is the single most-served topic across the theory-YouTube ecosystem, which means the content channels to market into already exist and are hungry for material.
2. **Creators multiply the self-learner motion for free.** The OBS tie-in gives streamers a reason to show the product on stream, where the audience is precisely other aspiring producers, and DMCA pressure pushes streamers toward original-music content. Small segment, outsized distribution value. Ship the creator features at launch; market them as part of the same product, not a separate SKU.
3. **Teachers are attractive but structurally slow:** per-student annual pricing with seat minimums, purchase orders, school-year buying windows, Chromebook fleets favoring web apps, LMS/SSO expectations, and PD-based discovery channels that take a year of relationship-building (Midnight Music, conferences). Also, classroom incumbents at $0 (BandLab EDU, GarageBand). Enter later with a free-teacher-account + per-student model once the product has self-learner testimonials, because teacher marketing runs on peer proof.
4. **Hobbyists convert through a free tier, not marketing.** Vital and Waveform Free show the pattern: give away a genuinely useful core, charge for depth/content. Against BandLab's 100M-user free anchor, a paid-only beginner product has no hobbyist story at all.

**Pricing implication:** free tier (real, not crippled) + one-time perpetual license in the $39-79 range with free updates, explicitly advertising "no subscription." The Waves reversal shows this positioning is itself a marketing asset in this community.

---

## 7. Five concrete demand signals to validate before/at launch

1. **Message-tested waitlist conversion.** Run one landing page with two framings ("a DAW that teaches you theory as you write" vs "the easiest way to start making music") against traffic from a WATMM weekly-promo post and one KVR thread. Signal: ≥5% visitor→email conversion on either framing, and which framing wins decides the launch copy. (KVR devs report forum posts still out-convert paid reach for small tools.)
2. **Micro-educator commitments before launch.** Pitch 15-20 mid-tier theory/production YouTubers (5k-100k subs, the tier that answers email) with keys and a 90-second demo. Signal: ≥3 unprompted commitments to cover it, or any educator asking to use it *in* their teaching content. Per the KVR marketing threads, one honest small creator who adopts it daily is worth more than a paid placement.
3. **"Where do I buy" density on a demo clip.** Post a 60-90s clip of the theory-teaching interaction (chord placed → explanation appears) to r/WeAreTheMusicMakers and r/musictheory. Signal: unsolicited purchase-intent comments and cross-posting by others, not raw upvotes; also measure click-through to the page. This tests whether the *theory-in-the-DAW* moment itself is shareable.
4. **Free-tier→paid conversion in a soft launch.** Put a free tier on itch.io (where beginner music tools like Bosca Ceoil demonstrably find an audience) 4-8 weeks pre-launch. Signal: ≥1,000 downloads in the first month without paid promotion, and ≥2% conversion to a paid pre-order/founder license, which validates the Vital-style funnel at your price point.
5. **Streamer retention + teacher pull (leading indicators for phase 2).** Seed the OBS integration to 5-10 music-category streamers: signal is *repeat* on-stream use (second session unprompted) and viewers asking what the tool is. In parallel, run 5 discovery calls with music-tech teachers recruited via the Midnight Music / Music Tech Teacher community: signal is a teacher asking for a site-license quote unprompted, plus hard data on their Chromebook constraint and per-student price ceiling (expect $3-10/student/yr based on uTheory/Soundtrap norms). If teachers pull, build the EDU motion; if not, the self-learner focus is confirmed.

---

### Sources

Discovery channels: [Hello Music Theory](https://hellomusictheory.com/learn/best-music-theory-youtube-channels/) · [School of Composition](https://www.schoolofcomposition.com/music-theory-composition-youtube-channels/) · [Splice blog](https://splice.com/blog/10-youtube-music-education-channels/) · [GummySearch r/WATMM](https://gummysearch.com/r/WeAreTheMusicMakers/) · [GummySearch r/musictheory](https://gummysearch.com/r/musictheory/) · [GummySearch r/piano](https://gummysearch.com/r/piano/) · [KVR marketing thread](https://www.kvraudio.com/forum/viewtopic.php?t=623713) · [KVR DSP forum](https://www.kvraudio.com/forum/viewtopic.php?p=9209869) · [KVR Developer Challenge](https://www.kvraudio.com/kvr-developer-challenge/) · [BPB on KVR DC 2026](https://bedroomproducersblog.com/2026/07/08/kvr-developer-challenge-26/) · [itch vs Gumroad discussion](https://itch.io/t/2760445/itch-vs-gumroad) · [itch VST tools](https://itch.io/tools/tag-vst) · [fluffy on platforms](https://beesbuzz.biz/blog/4910-On-bandcamp-gumroad-and-itch.io) · [Product Hunt traction metrics](https://www.producthunt.com/stories/8-traction-metrics-from-successful-product-hunt-launches) · [Mozart AI on PH](https://www.producthunt.com/products/mozart-ai)

Comparable launches: [BPB on Vital](https://bedroomproducersblog.com/2020/11/24/vital/) · [MusicTech on Vital](https://musictech.com/news/matt-tytel-vital-free-synth/) · [MusicTech on Scaler 2](https://musictech.com/news/plugin-boutiques-scaler-2/) · [Hookpad pricing](https://www.hooktheory.com/hookpad/pricing) · [Produce Like A Pro Hookpad review](https://producelikeapro.com/blog/hooktheory-hookpad-review/) · [Bosca Ceoil Blue (itch)](https://yurisizov.itch.io/boscaceoil-blue) · [Bosca Ceoil Blue (Steam)](https://store.steampowered.com/app/3570850/Bosca_Ceoil_The_Blue_Album/) · [Tracktion Waveform Free](https://www.tracktion.com/products/waveform-free) · [DJ.Studio DAW price guide](https://dj.studio/blog/daw-price-guide-subscriptions-student-deals) · [Synthesia on Google Play](https://play.google.com/store/apps/details?id=com.synthesia.synthesia) · [ProSoundPicks Melodics review](https://prosoundpicks.com/melodics-review/) · [Billboard on BandLab 100M](https://www.billboard.com/business/tech/bandlab-100-million-users-report-1235637853/)

Pricing/EDU: [MusicTech on Waves U-turn](https://musictech.com/news/industry/waves-subscription-only-u-turn-licenses/) · [RA on Waves](https://ra.co/news/78778) · [Waves statement](https://www.waves.com/news/perpetual-waves-licenses-are-back) · [Midnight Music Soundtrap vs BandLab](https://midnightmusic.com/2026/03/music-tech-soundtrap-vs-bandlab/) · [Midnight Music GarageBand/Soundtrap/BandLab](https://midnightmusic.com/2022/06/should-i-use-garageband-soundtrap-or-bandlab-with-my-students/) · [uTheory pricing](https://info.utheory.com/pricing-changes-for-utheory/) · [Flat.io K-12 software guide](https://blog.flat.io/best-music-software-k12-music-classrooms/) · [Rising Software education shop](https://www.risingsoftware.com/shop/education) · [B&H Auralia/Musition cloud bundle](https://www.bhphotovideo.com/c/product/1276231-REG/rising_software_aucel_bun_auralia_5_cloud_bundle.html) · [musictheory.net Tenuto](https://www.musictheory.net/products/tenuto) · [Hooktheory Classroom](https://www.hooktheory.com/education) · [Ableton Learning Music](https://learningmusic.ableton.com/) · [Music Tech Teacher podcast](https://midnightmusic.com/music-tech-teacher-podcast/)

Creator crossover: [TwitchTracker Music category](https://twitchtracker.com/games/26936) · [SullyGnome Music stats](https://sullygnome.com/game/music) · [ecopyright livestream copyright guide](https://www.ecopyright.io/blog/twitch-livestream-creator-copyright/) · [Meld Studio DMCA guide](https://meldstudio.co/blog/live-stream-music-without-getting-a-dcma-strike/) · [Hypebot streaming-production guide](https://www.hypebot.com/hypebot/2021/02/beginners-guide-for-streaming-music-production-on-twitch.html) · [Ableton forum OBS thread](https://forum.ableton.com/viewtopic.php?f=1&t=214537) · [Sportskeeda on Porter Robinson/xQc](https://www.sportskeeda.com/esports/news-porter-robinson-reveals-xqc-s-video-inspired-make-fl-studio-tutorials)

---


# Feature Catalog — from the changelog (Q6, release-notes material)

# Tangent — exhaustive user-facing feature catalog (through 4.92.0)

Compiled from a read-only sweep of `/Users/ganten/Library/CloudStorage/Dropbox/Projects/Apps/tangent/docs/` — primarily ARRANGEMENT.md (per-release log 4.43–4.92), HANDOFF.md (2.0–4.29), CHANGELOG.md (1.0–4.5), release/4.2 and release/4.4 notes, DRUMPAD.md, DX7-BANK-01.md, DESIGN.md, STORE-CONTENT.md, RECORDER-PLAN.md, plus release-commit messages for the 4.30–4.42 gap (which no doc covers). Versions cited where the docs state them.

---

## Recording & takes (Capture)

- **One-press whole takes**: a single Capture press writes one self-contained take folder with a `.wav` (48 kHz/24-bit BWF), a real `.mid` (SMF format 1, 960 PPQ), and an `.mp4` of the window — WAV sample 0, MIDI tick 0 and video frame 0 are the same instant (4.2, hardened through 4.4).
- **Capture is its own control**: a tape-amber cassette button beside the transport runs the whole-take machinery completely independently of arrangement recording — you can film a take over a playing or even recording arrangement (4.46).
- **The take is the desk**: what you hear is what gets recorded — mute is the one control that decides what stays out of a take, and monitored/unmonitored inputs both land in the file correctly (4.27).
- **Missing-source warning**: while a take is rolling, the band names any muted source that has signal in it, before the take is lost (4.27).
- **Count-in** in any time signature including 6/8, kept out of the file or recorded in; the cassette pressed during the count cancels it; the arrangement's red button honours it too, and a take and an arrangement record started together share one count (4.2-era; arrangement count-in 4.57; shared count 5.0).
- **Take summary**: a take reports its duration and any problem (a silent take counts as one) in a dismissable dialog with a "don't show again" for the happy path (4.18).
- **Honest take manifest**: `take.json` records every source, the sync report, and now the encoder-side dropped/padded frame counts, so the manifest can never disagree with the band's live counter (4.88).
- **Space is the transport's**: it stops an arrangement record or the rolling transport and, on the desk, plays from the edit cursor - the same under a take as without one. Only the cassette starts or ends a capture (5.0).
- **Clip lamp**: a latching, clickable clip indicator on the meters that clears when pressed and warms up past converter glitches so it never lies (4.15–4.20).
- **Crash-safe, never-overwriting output**: every take gets a fresh timestamped directory; nothing is ever written outside it.

## Video capture & camera

- **The window is the take**: the video is the app's own display — same panels, same places, camera inset where the window already puts it, 16:9, no scene to build (4.4).
- **Video without a camera**: a machine with no webcam still records the window (4.12).
- **Camera on all three platforms**: macOS via AVFoundation, Linux via a V4L2 backend with pure-Rust MJPEG decode (4.4.1) plus hardware VA-API JPEG decode where available (4.32), and Windows via the bundled ffmpeg's DirectShow input, honouring each webcam's mjpeg/raw mode split (4.87).
- **Click the preview to pick a camera**: the preview pane is the camera picker, and says plainly when no camera exists or a privacy setting blocks one (4.18; Windows wording 4.85/4.87).
- **Frame-accurate video**: every frame lands at the slot its own timestamp names, stalls are padded with the previous picture, and dropped frames can no longer silently halve a take's length (4.29).
- **Bundled encoder**: Windows and Linux ship a checksum-pinned static `tangent-ffmpeg` beside the binary, so filming needs nothing installed; macOS encodes natively (4.4.1).
- **Adaptive preview**: the camera preview costs only what the machine can afford — full rate for a take, throttled for the preview, nothing when hidden (4.20-era potato pass, 4.24).

## Mixer (the desk)

- **A real mixer view** behind Tab: 16 channels (grown from 8 in 4.82), of which 8 show at a time — plain wheel scrolls the desk, Shift+wheel zooms how many channels are visible (4.82, 4.84).
- **Channels are MIDI or AUDIO by what you plug in**: the input IS the kind, REAPER-style — binding an input makes the track, instruments bind MIDI, and there is no separate kind ceremony (4.52).
- **M / S / R on every strip**: mute, solo, and a record-arm that is also the live switch — an armed MIDI channel sounds the keyboard, an armed audio channel monitors its input, unarmed channels only play back (4.43).
- **Arm gestures**: a plain press toggles, Shift+arm means "this lane alone", and adding or binding a track never touches anyone's standing arms (4.64–4.67).
- **Three insert bays per channel** for VST3 effects, on the mixer strips, the arrange headers and the drum pads alike; Alt+click empties a bay, right-click offers Delete/Replace FX (4.34, 4.60, 4.68, 4.77).
- **Per-channel sends** into the effects bus, with new sends resting at zero so only the shipped DX7 arrives wired to the reverb (4.70).
- **Pan everywhere**: equal-power pan knobs on every mixer strip and every arrangement header (4.50).
- **In-app input menu**: the chip under a channel opens a floating hover tree — None / every exposed Mono and Stereo pick of the chosen interface / MIDI (All + each port) — with no system dialogs (4.50–4.52).
- **Master strip**: split stereo ladders with mirrored dB scales, gain-reduction readable against the scale, and a master fader in red — the one colour that can undo the rest (4.13–4.16, 4.26).
- **Track menu on right-click**: remove track, colour swatches, a five-palette auto-colour system (per-slot, manual paint overrides), and a 4-slot icon picker (4.50, 4.51, 4.59).
- **Tracks reorder by drag**, carrying everything — kind, input, colour, fader, arm, FX bays with their saved state, and every clip (4.59).
- **Add a track**: a circled plus in both views (or Ctrl/Cmd+T) adds a bare armed track whose input you bind later (4.54).
- **Rename by double-click**, and typing into a name field never fires shortcuts (4.52, 4.85).
- **Delete removes the selected track** in mixer and arrange when no clip is selected (4.68).
- **Honest meters**: stereo pairs on stereo sources, one bar on mono, instant-rise/timed-fall ballistics, and program-mix VU that ignores the click and auditions by design (4.60, 4.61, 4.66).
- **One dial standard**: every dial reads value on top and label below, click-to-type opens prefilled, double-click resets, Shift gives a true tenth-speed fine drag that engages cleanly mid-gesture (4.50, 4.71).
- **Typed values everywhere**: dB into faders, Hz with `k` suffix into filters, BPM inline into the tempo dial (4.26, 4.51, 4.60).

## Arrange view

- **A full arrangement timeline** over the piano area — Tab cycles THEORY → ARRANGE → MIXER, Shift+1/2/3 jump straight there (4.45; navigation rework 4.61).
- **Lane = channel**: each lane shares name, colour, kind, arm, mute and solo with its mixer strip, with REAPER-style headers (number, arm, name, volume knob, meter, M/S, input chip, three FX bays) (4.45, 4.60, 4.61).
- **All clip operations by gesture**: drag to move (across lanes too, since 4.58), drag ends to trim, corner-drag fades, `s` splits at the playhead, Delete removes (4.45).
- **Clip right-click menu**: Delete take / Copy take (4.73).
- **Import by drop**: drag audio from the file manager onto a lane, and `.mid`/`.midi`/`.smf` files drop in as MIDI clips (4.45; MIDI import 4.58).
- **Zoom three-plus ways**: wheel pan, Cmd/Ctrl+wheel zoom, ruler-drag zoom, `-`/`=` steps, `W` fits, and the wheel zooms at the playhead (4.45, 4.71).
- **Snap & grid**: magnet toggle (`g`) and a gridbox that clicks finer, right-clicks coarser, and drags as a stepper; snap holds the playhead to the grid too (4.45-era, 4.47, 4.62, 4.68).
- **Time selection is a loop zone**: drag on the ruler for an amber span that loops playback seamlessly and scopes exports; Escape clears it (4.53; looping 4.80; MIDI survives the wrap 4.81).
- **Arrangement recording**: the red transport button records armed lanes as clips — MIDI lanes collect the note stream (from hardware AND the app's own piano, key rows and pads since 4.62), audio lanes tee their raw inputs to WAVs in the project (4.46).
- **Destructive punch**: a new take carves out what it overlaps, and the recording lane's old clips fall silent from the record start so you never hear the overdub against what it replaces (4.50, 4.52).
- **The take is visible while it happens**: a growing red-rimmed ghost clip with its notes so far, and the window zooms out on its own when a take crosses the right edge (4.47, 4.74).
- **Undo/redo**: one shared timeline across editor, arrange and mixer (Ctrl/Cmd+Z, Shift for redo), 100 deep (4.60).
- **Copy/paste** for clips and notes through the real system clipboard — cross-track groups paste keeping their lane spread, landing at the playhead on the selected channel (4.60, 4.61, 4.80).
- **Marquee select** with right-drag, over clips and notes (4.59).
- **The band is one LCD**: bars|beats|thousandths plus min:sec, Start/End/Length of the timeline as draggable fields, and the playhead timecode drags by part (4.50, 4.54, 4.56).
- **Built-in oscilloscope**: the LCD's right half shows the live output mix (click excluded) on LCD glass (4.52).
- **Corner navigator**: an outer-song/inner-viewport widget you drag to zoom and scroll (4.49).
- **Transport**: Rec → Play → Stop → Pause; stop returns to the edit cursor so play-from-9-stop-at-12 puts you back at 9 (4.50, 4.53).
- **Tempo follows the document**: changing tempo rescales every MIDI clip and re-seats audio clips by bar, live while you drag; a TAP chip taps the tempo in (4.66).
- **Time-stretch**: shift-drag a MIDI clip's (or note selection's) edge to scale it around the fixed far end, notes, trims and fades riding along (4.66).
- **Per-view dark mode**: arrange and editor share a light/dark wardrobe toggled with D, independent of the theory view (4.53, 4.60).

## MIDI editor

- **Piano-roll on double-click** of a MIDI clip: fills the whole view under the band, with a pop-out button to float it (4.47; full-view + floating 4.56).
- **Note editing by gesture**: click draws one grid step, body-drag moves in time and pitch, either edge resizes snapped, right-click or double-click erases, and draw previews audition through the live path (4.47, 4.56, 4.59).
- **Velocity lane**: drag to set the nearest note's or the whole selection's velocity (4.50).
- **Multi-select**: shift-click, marquee, Cmd+A; move/resize the set as one; Q quantises, Cmd+D duplicates a grid step ahead, arrows nudge (Alt = fine), Z fits the clip (4.50).
- **Swing with a number**: the SWG chip opens a menu — any percentage plus named presets from Straight through Quintuplet, Golden Ratio, Shuffle and Dotted — and the percentage IS the grid: lines, snapping, note lengths and nudges all follow it; `p` gives triplet grids (4.47, 4.57).
- **Bar ruler with beat decimals** inside the roll (4.50, 4.59).
- **The keys column plays**: clicking the keys auditions through the live instrument, and on a drum lane the keys wear the pad names and strike the pads (4.47, 4.60).
- **Own zoom grammar**: wheel zooms at the pointer, Ctrl+wheel walks the roll, Shift+wheel zooms the visible note range, Alt+wheel pans — with the editor's own grid and snap settings kept separate from the arrangement's (4.47, 4.84).

## Drum machine

- **A built-in drum sampler** (`@tangent-drums`) shipped loaded on a desk track (channel 2 since 4.84), so a MIDI clip on its lane plays the kit and the piano-roll edits it (4.57).
- **128 pads covering the whole keyboard**: pad n is note n, shown one 4×4 page at a time with a bank strip that lights each page's loaded pads (4.77, 4.81).
- **Load by dropping**: drop audio files onto pads (N files land on N pads), or drag rows straight from the built-in media explorer, which browses the filesystem, follows symlinks, auditions on click, and remembers a default folder (4.49 merge, 4.50, 4.59, 4.61).
- **Per pad**: gain, pan, tune (±12 st), choke groups A–D, three FX insert bays, a waveform start/end trimmer, obey-note-offs, an editable name, and drag-to-swap between pads with FX riding along (4.50, 4.59, 4.80).
- **Plays like an instrument**: MIDI channel 10 addresses the pads, live notes 36–51 mirror onto the kit while the panel is up, retriggers self-choke through a click-free fade, and pad FX racks let tails ring (DRUMPAD.md, 4.81).
- **The kit is a desk citizen**: it renders through its lane's own strip — fader, pan, mute, solo, send, meter and stem (4.59).

## Instruments / synth

- **A built-in DX7**: a faithful six-operator FM engine validated against 11,756 real cartridges, loaded from the instrument picker with no plugin scan involved (4.5/4.6).
- **Reads real DX7 cartridges** (.syx bulk dumps), with a patch picker where selecting IS auditioning, and a Factory button back to the shipped bank (4.6, 4.24).
- **Factory bank "Tangent Bank 01 — Tines and Boxes"**: 32 original voices — sixteen electric pianos and sixteen jazz guitars — measured against 42,291 unique corpus voices to prove originality (DX7-BANK-01.md).
- **Real damper behaviour**: the sustain pedal reuses held voices per pitch, so rapid repeats never exhaust the voice pool (4.86).
- **A MIDI fast lane**: hardware notes go straight from the MIDI callback into the audio callback, cutting 3–10 ms of UI-frame latency for every instrument, pitch bend included (4.65).

## Effects

- **Six mix knobs on the band**, Tascam-styled: reverb, tempo-synced dotted-eighth delay, and chorus as bus sends; high-pass, low-pass and limiter as a master insert (4.12; bus/insert split 4.24).
- **A true-peak limiter** with 1 ms lookahead, automatic makeup gain, and a -48…0 dB threshold dial — output brickwalls at 0 dBTP (4.12, 4.15).
- **Filters read and type in hertz** (accepting `1.2k`), and the LPF runs ascending with 20 kHz fully clockwise so "open" is the resting position — double-click rests it open (4.13, 4.51, 4.90).
- **A user VST3 across the effects bus**: load any effect plugin as the bus insert, with correct dry-signal subtraction so sends never comb-filter (4.26).
- **One knob grammar**: drag sets, right-click types, double-click resets to "nothing applied" (4.14).
- **Master gain** last in the chain, reaching both the speakers and the take (4.13).

## Plugins / VST3

- **VST3 instrument and effect hosting** with per-seat editor windows, per-slot faders, and plugin state that persists per seat and survives track reorders (4.2-era onward; state-safe reorder 4.60).
- **Instrument vs effect awareness**: the app knows which kind a plugin is and refuses the wrong slot instead of playing silence (4.24, 4.26).
- **Recursive plugin scanning** with user-added folders and rescan without restart (4.4).
- **A picker organised by vendor**: real vendor folders (161/161 named on the owner's disk), collapsed by default except the loaded plugin's, with a filter that searches names and vendors and overrides the collapse (4.73–4.75).
- **Windows scanning hardened**: extra registry/path roots, dependency-carrying DLL loads, and a scan that says out loud when it found VST2s a VST3 host cannot load (4.63).
- **Safe teardown**: quitting with a plugin like Kontakt loaded can no longer wedge the machine's audio — the app exits without running foreign destructors (4.76).
- *(Note for release-notes writers: Tangent's own DAW-plugin VST3, shipped at 3.0, was deliberately retired at 4.77 — Tangent is a DAW and no longer loads inside another.)*

## Music-theory tools

- **The 88-key piano** lighting every key live from any MIDI keyboard, with sustain-held notes recoloured (1.0/2.0).
- **The chord engine**: weighted scoring, not lookup — 95 patterns against all 12 roots covering triads, sevenths, extensions, altered dominants, sus/add chords, slash chords, rootless jazz voicings, plus 28 scales and modes, with intervals named for two-note input (2.0).
- **Runners-up named**: the readout says "C6, or Am7" because a chord that is two things is a fact about harmony (4.2).
- **Teach it your own names**: hold a voicing, press N (or use the menu), and name it — literally or in every key (2.0; one-keystroke naming 3.0).
- **Chord Learning**: an optional re-ranker that learns your preferred readings from corrections, fully resettable, with taught names always winning (2.1).
- **Sharps or flats**, your choice, everywhere at once.
- **Transpose the readout** without moving your hands — now across the whole MIDI range, a chord stepping until a note would leave MIDI (early; full-range 4.79).
- **Keytoggle**: click the piano, the neck or the diagrams to place notes and work things out with no instrument attached.
- **The guitar neck**: an exhaustive, deterministic voicing solver picks the shape a hand would actually use (span, opens, barres weighed) and holds it steady as you add notes; seven tunings plus your own, a capo, three fingerboard woods, a per-string interval readout, and the neck works backwards — click frets, drag a barre, read the chord off the piano (2.3–3.0; interval strip 4.25).
- **The theory band**: circle of fifths with every key shaded by how much of your chord belongs to it, a Tonnetz where triads are triangles and shared notes are shared edges, and I–IV–V harmonic triangles anchored on your key signature — all clickable as inputs, with optional Follow MIDI (3.0, 4.2, 4.20).
- **Sheet music, engraved properly**: six clefs (including the octave-down treble and bass guitarists and bassists actually read), all fifteen key signatures, accidentals spelled for the key, 8va/8vb, and letter names inside the noteheads; the stave is now an exclusive pick with Grand Staff the default (4.2; single-stave pick 4.73).
- **Rearrange the board**: keys 1–4 toggle the four theory panels and re-order them mid-lesson; all off collapses the band into the keyboard (4.2).
- **The selection ring**: a keytoggle click's audition rings for 1.5 s instead of dying with the mouse button, so damped instruments are actually heard (4.79).

## Streaming / ReaStream → OBS

- **Stream to OBS**: a native ReaStream sender pushes Tangent's master output over localhost UDP to OBS running the genuine ReaStream VST2 receiver — no plugin needed on Tangent's side, no audio-thread sockets (4.90).
- **Its own door**: a STREAM row in the settings cog's AUDIO tier reads out on/off and opens the dialog, which always renders in the light theme (4.92).

## Projects & templates

- **A project is a directory**: the arrangement doc plus a `media/` folder into which all imported and recorded media is auto-copied — move it, zip it, hand it over, it opens (4.44).
- **Reopens where you left off**, but with every arm off except the default DX7 channel, so no project ever opens blasting (4.44).
- **Sessions carry the desk**: kinds, names, colours, inserts with their plugin state, sends, gains, pans, inputs, tempo, metronome and master travel inside the project (4.50).
- **Project menu on the transport**: New / Open / Save / Save As / Save As Template / Export, with templates in a Templates folder and a `default` template that New clones (4.50).
- **Untitled scratch sessions**: new sessions land on "Untitled" and are reclaimed unless saved on purpose; the project window's title is a type-and-save rename field (4.57; truly-empty New 4.81).
- **v12 migration**: an old backing track becomes an audio clip at 0:00 in a fresh default project — nobody's track vanishes (4.44).

## Export

- **One export dialog**: master, per-lane stems, or both; sample rate; bit depth (24-bit default, 32-float, 16-bit); stereo/mono; WAV plus optional MP3 (320k); destination and name; and the time selection as an optional range (4.52, 4.53, 4.56).
- **Full-speed offline rendering** by default on a deviceless engine — same racks, clips, pads and desk — with realtime as the option (4.52).
- **Stems in one pass**: every used lane renders to its own WAV simultaneously, the drum kit through its lane, and silent lanes are skipped (4.50, 4.59).
- **MP3 on Windows too**, routed through the bundled ffmpeg (4.85).
- **Verified decode**: imported audio is checked against its container's real rate and layout and resampled to match, so a lying decoder can't warp an export (4.80).

## Audio systems & devices

- **Backends per platform**: CoreAudio, WASAPI and ASIO (compiled in, a deliberate choice with WASAPI the default), and ALSA/PipeWire-aware Linux (ASIO 4.63; WASAPI-default 4.80; ASIO buffer contract 4.91).
- **Yank-proof**: a dead device is declared loudly by a stall watchdog, and a background reprobe reopens audio by itself the moment the hardware returns — no freeze, no manual nudge (4.90, 4.92).
- **Per-channel input exposure**: any interface offers each mono input and every adjacent stereo pair as tick-box picks, and a channel's menu lists exactly the chosen device's inputs (4.23, 4.48).
- **Multiple simultaneous inputs** from one interface, sample-aligned on one clock, one strip each (4.28).
- **Input monitoring on by default** with a feedback guard riding the arm, and monitoring state that never survives a relaunch into your speakers (4.52; never-persisted rule from 4.20-era).
- **Low-latency macOS duplex**: the input runs at a quarter of the chosen buffer, putting 128-sample round trips under Ableton Live's (4.64).
- **Buffer size, sample rate and host system** chooseable in settings, applied safely on an edge — including the CoreAudio ordering that prevents deadlocks (4.20, 4.24-era fix).
- **Wide format support**: I24, F64, I64 outputs for picky drivers, denormal flushing, and a Windows double-buffer so small buffers don't crackle (4.63, 4.80).
- **PipeWire-native input listing** on Linux, so the interface you plugged in is the one recorded (4.20-era).
- **MIDI everywhere**: auto-connect priority chain, a port picker, per-port or all-ports binding per channel, and channel-10 drum routing.

## Settings & UX

- **The settings cog**: a two-tier AUDIO/TAKE panel holding device, buffer, count-in and time signature, with PROJECT, PLUGINS and STREAM doors on their own rows (4.51, 4.57, 4.68, 4.92).
- **Everything is in the right-click menu**: context menus everywhere, with the arrange view and MIDI editor opening a global cut of the app menu at the pointer (4.68, 4.69).
- **Hold H for help**: a per-view shortcut card that lists only the keys that exist on that page (4.49, 4.59).
- **View navigation**: Tab cycles theory/arrange/mixer; Shift+1/2/3 jump; per-view dark mode on D (4.61).
- **Fully offline and free**: no account, no server, no telemetry, no trial, no locked features — MIT licensed.
- **Supporter key**: pay-what-you-want; the key activates offline (Ed25519), never expires, works on every machine, and unlocks nothing but a pixel heart and a thanks card (2.x onward).
- **Every colour is yours**: full colour customization, dark mode, three bundled typefaces cycled with F, plus a custom font path.
- **A launch splash** that spells TANGENT on the app's own Tonnetz (4.4).
- **Installers on all three platforms**: signed + notarized macOS .pkg/.dmg, a Windows NSIS setup.exe wearing the app's own icon with a statically-linked runtime (no VC++ redist needed, 4.83), and a Linux tarball with a rootless `install.sh` (3.0 installers; 4.83 hardening).
- **Windows behaves**: no console-window flashes from subprocesses, hidden-then-shown child windows, native paths and drive views (4.23, 4.63).
- **Tiling-WM friendly**: i3/AeroSpace-safe geometry, floating dialog hints, borderless mode with correct repaint (4.4.1, 4.79).
- **Single instance** with a clear "already running" dialog, and a panic-hook error dialog instead of a silent death.

## Retired on purpose (don't cite as current)

- The standalone **backing-track player/strip** — replaced by "an audio clip on a lane" (retired 4.43–4.47).
- The **overview scrubber** — replaced by the LCD cluster and navigator (retired 4.50).
- The **drum-bus mixer column** — the kit lives on its lane's strip (retired 4.59).
- **Detachable panels** (chord window, neck) — retired 4.20.
- The **Tangent VST3 plugin** for other DAWs — retired 4.77.

## Gap note

Versions **4.30–4.42** have no doc-level changelog; from release-commit messages they cover: channel name fields and interface aliases (4.30), open-string drawing (4.31), Linux hardware MJPEG decode (4.32), monitoring latency accuracy (4.33), the three-inserts-per-channel desk (4.34), insert windows and readable rack (4.35–4.36), the GL compositor (4.37), camera starvation reporting (4.38), V4L2 queue depth and neck interval-from-bass (4.39), the transport/play/seek foundation and the arrangement file (4.40), the MIDI/AUDIO channel flip (4.41), and pause plus cross-platform plugin windows (4.42).

---


# UI Surface Catalog — views, dialogs, every shortcut (Q6, manual material)

# Tangent (Ivory) UI Catalog - every view, control, dialog, shortcut, and mouse gesture

Sources: `tangent-ui/src/` - `keys.rs`, `recorder_panel.rs`, `mixer_panel.rs`, `arrangement_panel.rs`, `midi_editor.rs`, `drumpad_panel.rs`, `dialogs.rs`, `settings.rs`, `chord_strip.rs`, `theory_panel.rs`, `fretboard_panel.rs`, `staff.rs`, `piano.rs`, `menu.rs`, plus gesture handlers in `app.rs`. All paths relative to `/Users/ganten/Library/CloudStorage/Dropbox/Projects/Apps/tangent/`.

The app has **no menu bar and no preferences window**. The entire chrome is: the right-click context menu, the recorder band's buttons/cog, dialogs, and single-key shortcuts. Toggle rows in menus rename themselves (no checkmarks); the current choice in a list is marked with a bullet.

---

## 1. Views

The main window is a stack of horizontal bands (top to bottom, each present only when enabled): **Recorder band**, **Theory band row** (diagrams + optional camera pane), **Chord strip** (legacy, off by default), **Piano**, **Fretboard**. The middle of the window can instead be swapped to the **Arrangement** or the **Mixer** (Tab cycles theory/arrange/mixer; Shift+1/2/3 jump directly). The **MIDI editor** and the **Drumpad** are popups floating over whichever view is open. Four surfaces can detach into their own OS windows: the chord readout, the fretboard, the theory band, and the recorder band.

### 1.1 Recorder band (`recorder_panel.rs`)

Two layouts switched by transport state: **idle** (framing and destination) and **rolling** (readable from the piano bench: destination controls vanish, the clock triples, dead controls are removed rather than greyed).

Left to right:

- **Camera preview** - letterboxed (never cropped) picture of the selected camera. Clicking it opens the **camera picker** (disabled while rolling). Same size in both layouts.
- **LCD cluster** (black glass, green digits):
  - Big musical position readout `bar| beat| thousandths` with an h:mm:ss clock under it. The playhead's three number parts are each **draggable** (drag up/down changes by that unit) - so are all the others on this glass.
  - **Start / End / Length** rows: the arrangement viewport's own numbers, three draggable part-cells each (bar, beat, thousandths). Start/End count from one; Length from zero.
  - Right half of the glass is the **oscilloscope** (the approved band-LCD scope widget; speakers' mix without the click).
- **Transport row** (under the LCD; identical geometry idle and rolling):
  - **Tempo knob** at the head. Drag turns it (vertical, relative); the number cap above it is a click-to-type target; double-click resets to default BPM; a **TAP** chip sits in the cell's slack - each press is one beat and the tempo follows. It stays through a take (held still during a count-in, and absent only while an export renders); the take's elapsed time is on the LCD glass, suppressed entirely if "Hide Elapsed Time" is on.
  - **Record** (red) - record armed channels into the arrangement. The same button, in the same place, while a take runs.
  - **Play** (green) - audition from the playhead without recording. Live while a take runs.
  - **Stop** - stops the transport and returns the playhead to the edit cursor. Same pixel position in both layouts; it does not end a take.
  - **Pause** - hold the playhead where it stands (Stop is the one that goes home). Live while a take runs.
  - **Cassette (Capture)** - start/stop a capture of the whole session (video, audio, MIDI) apart from the arrangement. The only control that reaches a take: it glows amber while one runs and is the button that ends it. A take does not start or stop the transport.
  - **Folder (Project)** - opens the Project dialog (new, open, save, save as, template, export).
  - **Cog (Setup)** - opens the take-settings popup (below). Idle only.
- **VU meter** - what is being recorded (input side). The **whole meter face is a button: clicking it clears the clip lamp** (the clip latch is shown on the face itself).
- **Effect knobs, two rows of three**: top row sends - **REVERB, DELAY, CHORUS** (blue caps); bottom row output shapers - **HPF, LPF** (violet caps), **LIMITER** (green/red cap). Each cell: word/value cap on top (single click = type a number), dial below (vertical relative drag; Shift = fine 0.1x). Double-click resets: sends to zero, HPF to zero, **LPF and Limiter to fully open/1.0** ("not applied"). **Right-click a knob opens its parameter panel** (see 1.3). Right-click on a fader/other control does nothing or falls to the app menu.
- **Master column** (right edge): stereo LED ladder meter with dB scales on **both** sides, gain-reduction bar hanging from the top (limiter), and the **master knob** at the foot (same size/gestures as an effect knob; double-click = shipped default; cap = type dB).
- **Status line** (bottom sliver, overlay): one line of status/warning text with a permanent **x** at its right end to dismiss the message.
- **Supporter heart** (top-right corner, only with a valid supporter key + Show Heart on): click cycles through 7 colors; hovering raises the **thanks card** (names of supporters).
- Right-click anywhere on the band opens the app context menu with the Recorder's categories moved to the top.

### 1.2 Take Settings popup (the cog) (`recorder_panel.rs` SetupLayout)

Anchored under the cog; click away dismisses; **Done** closes. Nothing here can be changed mid-take. Two tiers:

- **AUDIO tier**: "Setup..." row opening the **Audio Setup** dialog; **Hear the live input** tick (input monitor, session-only, never persisted); **Count-in** (cycles 0 / 4 / 8 beats); **Time signature** (click to type, e.g. "6/8"); **Stream to OBS...** (ReaStream dialog); **Project** row (Project dialog); **Add a VST3 Folder...** (plugin folder picker + rescan).
- **TAKE tier**: **Take name** field; **Choose folder** (destination) + **Reveal** (show in file manager); folder/naming preview line + disk-time-remaining (read-only); **Count-in** and **Time signature** (take copies of the same settings); **Export...** (per-take export dialog); four ticks: **Show the take when it is finished**, **Record the click into takes**, **Record the count-in into the take**, **Hide the elapsed time**.

### 1.3 Effect parameter panels (`recorder_panel.rs` FxLayout)

Opened by right-clicking an effect knob. A titled panel with subtitle, up to four parameter rows, an **x** close and a right-aligned **Reset**:

- Reverb: Size, Damping, Width (sliders).
- Delay: Time (steps through named tempo divisions), Repeats, Tone, Width.
- Chorus: Rate, Depth, Width, Tone.
- HPF / LPF: Slope (steps: dB/oct choices), Resonance.
- Limiter: Release, Knee.

Sliders drag along the row (drag can continue outside the panel; the value pins at the ends). Stepped rows advance on click. A press on panel chrome is swallowed, not passed through.

### 1.4 Piano (`piano.rs`)

88 keys, A0-C8. Key colors (white/black idle, active, sustain) are user-settable. Optional note names on keys (key `U` / menu). Clicking a key: with **keytoggle off**, sounds the note while the button is down; with **keytoggle on** (key `K`), places/removes the note (a held selection click rings briefly). Arrow Up/Down transpose all held/placed notes by a semitone.

### 1.5 Chord strip (legacy) + detached chord window (`chord_strip.rs`)

Black strip showing the detected chord name (user-settable color, auto font-fit, two-line wrap for long scale names). Off by default since the staff took over chord display; re-enabled via menu (Chords category, "Enable Chord Strip (legacy)").

- **Transpose chevrons** top-left (when `show_transpose`): up/down a semitone; a non-zero offset is printed beside them.
- **Supporter heart** top-right (when no recorder band is showing): click cycles color; hover raises the thanks card.
- **Detached chord window** ("Detach Chord Window" in menu): resizable, min 220x80, remembers size/position, always-on-top while the main window is focused; right-click anywhere opens the app menu; in borderless mode click-drag anywhere moves it; closing it re-attaches.

### 1.6 Theory band (`theory_panel.rs`)

Up to four elements side by side, in user order: **Circle of Fifths (1)**, **Tonnetz (2)**, **Harmonic Triangles (3)**, **Sheet Music (4)**. Number keys toggle each element; a re-added element appends at the right (that is the reordering mechanism). `T` cycles arrangements; the menu's "Show All Theory" restores everything.

- Clicking a **note name on the circle** or a **node on the Tonnetz** sounds that pitch class (and places it when keytoggle is on).
- Clicking a **vertex of the harmonic triangles** sounds the whole triad (I/IV/V relative to the current Key setting); with keytoggle on it places/removes the triad.
- The **staff cell** answers clicks with real pitches (see 1.7).
- **Camera pane** (menu: Show Camera) can sit beside the diagrams (left or right); it is a picture only - no click targets.
- Detachable into its own window (menu).

### 1.7 Sheet music / staff (`staff.rs`)

Grand staff by default; presets: Grand, Treble, Bass, Alto, Tenor, Treble 8vb, Bass 8vb, plus a user "custom" stack. Key signatures from 7 flats to 7 sharps (menu Key list). Shows held notes engraved in the current key; optional letter names inside noteheads.

- **Chord readout**: the winning chord name beside/above the staff plus up to two next-best alternate readings (the chord strip auto-hides while the staff is up).
- **Clicking a line or space places/sounds that pitch**, spelled by the key signature (F line in D major gives F#). Reach extends 3 ledger lines beyond each staff.
- Right-click on the staff opens the app menu with **Clef / Key / Stave** categories moved to the front.

### 1.8 Fretboard / guitar view (`fretboard_panel.rs`)

Toggled with `G`. Height scales with string count (6 to 12).

- **Click a string at a fret**: sounds the note; with keytoggle on places it (pinned to that string/fret). The zone left of the nut is the open-string zone.
- **Barre gesture** (keytoggle on): hold and drag along one fret; every string crossed is fretted there. This is the only way to enter a barre.
- **Capo**: shown at the fret chosen in the menu (No Capo / Fret 1-9); **clicking the capo cycles its material** (black / silver / wood).
- Optional note names on the neck and an interval column beside it (menu toggles).
- Fingerboard **Wood**: Rosewood / Maple / Ebony (menu). Tuning: every preset in `fretboard::TUNINGS` plus **Custom Tuning...** editor.
- Detachable into its own window (default 880x190); remembers geometry.

### 1.9 Mixer (`mixer_panel.rs`)

Reached with Tab or Shift+3. A fixed desk: **8 general channel strips** (scrollable window of them), then pinned at the right the **CLICK** strip and the **MASTER**. Shift+wheel changes how many channel columns show (1-8+); plain wheel over the desk scrolls the channel window.

Each used channel strip, top to bottom:

- **Name plate** - double-click to rename (typed inline; on an interface input it renames the interface, so all its inputs change at once). Single press on the name (or bare strip paint) arms a **carry**: drag the strip to another column to reorder channels.
- **Icon** (under the name) - shows the channel kind (keyboard = MIDI instrument, microphone = audio, waveform = clip lane) or a chosen badge (drum / guitar / bass from the Track dialog). Clicking it opens the input/instrument picker (`Hit::Add`); on an empty slot the circled **+** is the whole button and adds a bare track.
- **Input chip** ("NO INPUT" or the input's name) - press opens the channel input menu.
- **Insert rack** - three full-width stacked bays. Click an empty bay (+) to open the plugin picker; click a loaded bay to open the plugin's window (the built-in DX7 opens the patch picker; the drum kit opens the drumpad). **Alt+click a loaded bay empties it.** **Right-click a loaded bay** opens a two-row modal menu: **DELETE FX / REPLACE FX** (anywhere else dismisses and does nothing).
- **Send knob** (left) and **Pan knob** (right) - vertical relative drag, 420 pt full travel, Shift = fine; double-click resets; readout above each.
- **Meter** (beside the fader): one bar for mono sources, two for stereo; ticks at 0/-12/-24/-48; gain-reduction strip from the top.
- **Fader** - ivory cap in a black slot, vertical relative drag (420 pt travel = 0.1 dB per point), Shift fine, double-click resets to shipped level.
- **dB readout** - click to type an exact figure (Enter commits, Esc abandons, clicking elsewhere commits).
- **M / S / R switches** - mute (amber), solo (crimson; soloing dims/silences the others), arm (red; record-ready and live). **Shift+click R arms this lane exclusively** (every other arm stands down). The click strip has M/S only; the master has neither.

Master: full dB scale (6 to -48), stereo ladder, fader, dB entry; cannot be muted/soloed/armed. Empty slots draw as an outline with a circled **+** (press = add bare track; the rest of the panel is quiet ground where right-click opens the app menu).

Other mixer gestures: **right-click anywhere on a used strip** opens the **Track dialog** (remove / icon / colour / palette); **Delete/Backspace** removes the selected channel; **Ctrl/Cmd+T** adds a new track; any press on a channel selects it (faint rim).

### 1.10 Arrangement (`arrangement_panel.rs`)

Reached with Tab or Shift+2. Eight-lane timeline (window of lanes scrolls; Shift+wheel over headers changes visible lane count). Layout: corner toolbar + bar ruler on top; track headers left; lanes right; a draggable **header grip** seam between them (hover-lit; drag resizes the header column, a motionless click returns the playhead to the start).

**Corner toolbar**: **metronome** toggle, **magnet** (snap) toggle, **grid box** (label like "8", "1", "1/16"; click-drag up = finer, right-click = one coarser, wheel over it turns it; Ctrl+Alt+wheel anywhere is the same control), and the **navigator** (song = outer box, viewport = lit inner box; drag horizontally scrolls, vertically zooms, both at once).

**Ruler**: click = locate (snapped); drag sideways = **time selection** (drives the loop and export span; Esc clears); drag vertically = zoom around the pressed second (down zooms in). Bars count from 1; a clock row (h:mm:ss) runs along the foot.

**Track headers** (per used lane): number cell (drag = reorder track; bare header ground works too), arm dot, name (double-click = rename), input chip (press = input menu), three insert bays (same gestures as mixer, incl. Alt+click empty and right-click Delete/Replace), PAN / VOL / SND dials with readouts (vertical drag, Shift fine, double-click reset), stereo/mono meter, M/S switches. Unused lanes: circled **+** (add bare track) and a NO INPUT chip (bind an input directly). Right-click a used header = Track dialog; empty header = app menu.

**Clips**: body = select + move along/between lanes (grabbing a member of the selection moves the whole set); left/right edges = trim; top corners = **fade handles** (drag in for fade in/out wedges); **Shift-drag a MIDI clip's edge stretches it, notes and all** (audio refuses); **double-click a MIDI clip opens the MIDI editor**; audio clips draw their waveform, MIDI clips a note miniature (empty MIDI shows a dashed line); a clip being recorded is red-rimmed and takes no gestures. **Right-click a clip**: two-row menu **Delete / Copy** (acts on the whole selection when the clip is in it). **Right-drag on empty lane space draws a marquee** selecting every clip it crosses (Shift keeps the existing selection). **Shift-drag on a lane draws a new MIDI item**; **Alt-drag pans the viewport**; a plain click on empty lane space locates the playhead (snapped) and clears the selection.

**Wheel**: plain = zoom at the playhead (sideways component pans); Alt = pan; Ctrl+Alt = grid finer/coarser; over the headers = scroll lanes (Shift = change lane count).

**Keys in this view**: `S` split clip at the playhead, `G` snap toggle, `M` metronome, `W` fit the biggest clip (or shipped zoom when empty), `-`/`=` zoom out/in around the playhead, Delete = delete selected clips (or, with none, the selected track), Cmd/Ctrl+C / +V copy/paste clips, Ctrl/Cmd+T new track, Esc clears the time selection. Files dropped from the OS land on the lane under the cursor (audio or `.mid`).

### 1.11 MIDI editor (`midi_editor.rs`)

A popup piano-roll over the arrangement (open via double-click on a MIDI clip). Top strip: clip name; then right-aligned chips **SWG** (opens the swing menu: a percent field plus six presets - Straight 1:1, Septuplet 4:3, Quintuplet 3:2, Golden Ratio, Triplet (Shuffle) 2:1, Dotted (Hard) 3:1), **TRP** (triplet grid), **SNAP** (magnet), **grid box** (word like "1/8T", "1/8 sw"; click-drag up = finer, right-click = coarser, wheel turns it), **detach** (float the editor / full view), **x** (close). Under the strip: bar **ruler** (click = locate, snapped) with beat decimals; left **piano key column** (click auditions; on a drum lane the pads' sample names are printed on the keys); the **roll**; and a **velocity lane** along the foot (click/drag sets the nearest note's velocity; selected notes' bars are lit).

Roll gestures: click empty cell = **draw a note** (lands in the cell under the hand, floor-snapped); note body = select + move (selection moves together, auditions while dragging, view crawls at the edges); note edges = resize head/tail; **Shift-drag an edge stretches the selection to the grid**; **Shift-click adds to the selection**; **Cmd-drag empty room = rubber-band select**; **right-click a note erases it** (double-click also removes); **right-drag empty room draws the marquee** (Shift keeps); dragging is snapped by the magnet through the same math the grid lines are drawn with (straight/swing/triplet).

Keys: `Q` quantise selection, `Cmd/Ctrl+A` select all, `Cmd/Ctrl+D` duplicate one grid step later, arrows nudge (left/right a step, up/down a semitone), Delete = delete selection, `-`/`=` zoom, `W`/`Z` fit the clip, `G` snap, `P` triplet, `O` swing on/off, Esc close. Wheel: zoom at playhead; Alt = pan; Ctrl = walk the roll up/down; Ctrl+Alt = grid size; Shift+wheel zooms the number of visible keys.

### 1.12 Drumpad (`drumpad_panel.rs`)

A popup floating over any view (opened by clicking a drum kit's insert bay - the ToggleDrumpad action currently has no key binding). Layout: title strip (drag empty strip run = move the panel; **DEF** chip = remember the current media folder as default; **MIDI** chip = mirror live MIDI notes onto the pads; **x** = close), a **bank band** (8 stacked page cells, each a 4x4 miniature showing which of the 128 keys hold samples; click = show that page), the **4x4 pad grid** (pad 0 bottom-left, MPC order; pads light from the engine's audio peak), an **inspector**, and a **media explorer** on the right half.

- **Pads**: click = strike + select (empty pad only selects); press-and-really-move = **carry the pad to another pad** (samples, knobs and FX bays swap); **right-click an occupied pad** opens the **pad menu**: editable name, waveform **trimmer** (draggable start/end handles), and an **obey note-offs** toggle. Pads show note name (C2 etc.), sample name (red if the file is missing), waveform ghost, choke-group letter (amber), and a mini level meter.
- **Inspector** (for the selected pad): **GAIN / PAN / TUNE** horizontal-travel knobs (drag right = more; Shift fine; click the number = type, Enter lands it; double-click the track = default), **CHK** chip (cycles choke group -, A, B, C, D), **CLEAR** (red; empties the pad), and the pad's **three FX bays** (same gestures as every bay: click = picker/window, right-click = clear).
- **Media explorer**: ".." chip = up a directory (above a root: the drives list); folder rows walk in; **clicking a file row auditions it once and arms it as a drag source - drag it onto a pad to load**; wheel scrolls; a "+N" count marks overflow. OS file drops onto pads also load them.

### 1.13 Help card (`keys.rs`)

Hold `H` (or F1): a card slides down listing shortcuts, generated from the live binding table so it can never advertise a dead key. It is per-view (piano/mixer/arrange/MIDI editor), lists a GENERAL section plus the view's own section, and includes the gesture rows quoted in section 3 below. Esc also closes it. It sizes itself into 1-5 columns to fit the window.

---

## 2. Dialogs (`dialogs.rs` unless noted)

Each dialog is its own decorated viewport (or an in-canvas layer in a plugin).

| Dialog | What it does |
|---|---|
| **Project** | Session name field (Save keeps the typed name) + rows: New (from default template), Open..., Save, Save As... (copy whole session and open the copy), Save As Template... (Projects/Templates; name it `default` to replace the default), Export..., Done. |
| **Export (project)** | SOURCE Master mix/Stems/Both; TIME Entire project/Time selection; SAMPLE RATE 44.1/48/88.2/96k; CHANNELS Stereo/Mono; DEPTH 16/24/32-float (with advice line); RENDER Full speed/Realtime; PRIMARY wav/mp3; SECONDARY off/wav/mp3; FILE NAME; DIRECTORY + Browse; Export / Cancel. |
| **Track** | Right-click on a mixer strip or lane header. Remove Track; ICON picker (auto/drum/guitar/bass); COLOUR 27-swatch grid (hues x tones; slot 0 = auto/wood); AUTO-PALETTE choice (IVORY, EMBER, TIDE, MOSS, NEON) with swatch previews; Done. |
| **MIDI picker** | Select MIDI input port (list, current marked). |
| **Device picker** | One dialog for camera or audio input; rows by stable UID; a real "None" row (close the camera / record audio-only). |
| **Input Channels** | Which of an interface's inputs the mic picker offers: MONO tick per input, STEREO tick per adjacent pair; any number at once; each becomes a picker row. |
| **Audio Setup** | Live status of the audio path plus controls: audio system, buffer frames, sample rate (both streams), device chooser, exposed input channels, interface alias (short name). |
| **Plugin picker** | Directory listing of installed VST3 bundles grouped in vendor folders (collapsed at open except the loaded one), free-text filter (primary interaction), Load/None. Loading is the one action that runs third-party code and says so. |
| **Patch picker (DX7)** | Cartridge name, 32+ voice list; **clicking a patch auditions it immediately** (no Load button, only Done); filter field; Load Cartridge... (.syx), Factory bank restore; checksum warning shown but never refused. |
| **Patch editor (DX7)** | Every DX7 parameter by operator page (OP1-6 + patch page), algorithm diagram, name field, Save into the user bank, note line for the result. |
| **File browser** | Tangent's own fallback browser (for portal-less Linux): title, directory listing, error line. |
| **Take summary** | What a take produced; "don't show again" tick (problem takes are unsuppressable). |
| **NoMidiInput / MidiError** | Info/error boxes. |
| **About** | Themed about box (key `A`). |
| **Welcome** | Shown at startup until dismissed with its checkbox; support ask, gates nothing. |
| **ReaStream** | Stream master to OBS/REAPER: enabled tick, identifier, host. |
| **Supporter key** | Paste a key; stays open on failure with a message; shows the installed name. |
| **Color pick** | egui color picker for: white/black idle keys, active key, sustain, chord text, recorder background. |
| **Teach Chord** (`N`) | Names the held voicing; shows held notes, current reading, editable name, "apply in all keys" tick. |
| **Manage Taught Chords** (`M`) | List of overrides with delete; footer shows learned re-ranker status/weights and Forget. |
| **Correct Chord** (`E`) | Pick the reading the re-ranker should learn toward, from real candidates. |
| **Learn result** | Report of what a correction achieved. |
| **Custom tuning editor** | Seeded from the live tuning; per-string note-name and MIDI-number fields (kept in step), Add/Remove low string (grows downward in fourths), name field with clash detection; refusals are explained (non-ascending strings etc.); confirm gated on validity. |
| **Export (take)** | Per-take capture spec: sources/resolution/fps/display composition; tempo text field; "use these settings for every take" tick; post-take mode greys camera controls. |

**Overlay menus** (drawn in-canvas, modal): mixer/arrangement insert-bay menu (DELETE FX / REPLACE FX), arrangement clip menu (Delete / Copy), drumpad pad menu (name / trimmer / obey note-offs), MIDI editor swing menu (percent + six presets), channel input menu (the interface inputs and MIDI sources for a lane).

---

## 3. Complete keyboard shortcut table

Bare keys fire only with **no modifier held** and never while a text field has focus. One table drives both the behavior and the help card (`keys.rs`).

### Global (every view)

| Key | Action |
|---|---|
| `H` (hold), `F1` | Show the shortcut card (slides away on release) |
| `D` | Dark mode |
| `B` | Window border (borderless toggle) |
| `F` | Cycle the typeface |
| `A` | About |
| `Z`, `F11` | Fullscreen (only when the host owns its window) |
| `Tab` | Cycle theory / arrange / mixer |
| `Shift+1` | Theory (home) view |
| `Shift+2` | Arrangement |
| `Shift+3` | Mixer |
| `Enter` | Record the arrangement (only while the recorder band is open) |
| `Space` | Play / stop (to cursor) (only while the recorder band is open) |
| `Ctrl/Cmd+Z` | Undo |
| `Ctrl/Cmd+Shift+Z` | Redo |
| `Esc` | Close the help card / MIDI editor / clear the time selection (innermost claimant first) |

### Piano view (the home; keeps every binding)

| Key | Action |
|---|---|
| `K` | Keytoggle: click the piano or the neck to place notes |
| `R` | Clear every note you placed |
| `G` | Guitar view |
| `U` | Note names |
| `I` | Cycle clef |
| `1`-`4` | Toggle a theory element (Circle / Tonnetz / Triangles / Staff); re-adding appends at the right |
| `T` | Cycle the theory band |
| `C` | Chord detection |
| `S` | Enter a supporter key |
| `N` | Teach: name the chord you are holding |
| `E` | Correct what it reads |
| `M` | Chords you have taught |
| `L` | Chord learning on/off |
| `P` | Sharps or flats |
| `Up` / `Down` | Transpose up/down a semitone |

### Mixer view

| Key | Action |
|---|---|
| `S` | Supporter key card |
| `Delete`/`Backspace` | Remove the selected channel |
| `Ctrl/Cmd+T` | New track |

### Arrangement view

| Key | Action |
|---|---|
| `S` | Split the clip at the playhead |
| `G` | Snap to grid (the magnet) |
| `M` | Metronome on/off |
| `W` | Fit the biggest clip (empty: the shipped zoom) |
| `-` / `=` | Zoom out / in, in steps, around the playhead |
| `Delete`/`Backspace` | Delete selected clips; with none, the selected track |
| `Cmd/Ctrl+C` / `+V` | Copy / paste clips |
| `Ctrl/Cmd+T` | New track |
| `Esc` | Clear the time selection (and the loop) |

### MIDI editor

| Key | Action |
|---|---|
| `G` | Snap to grid |
| `P` | Triplet grid on/off |
| `O` | Swing grid on/off |
| `Q` | Quantise the selection |
| `Cmd/Ctrl+A` | Select every note |
| `Cmd/Ctrl+D` | Duplicate one grid step later |
| Arrows | Nudge: left/right a grid step, up/down a semitone |
| `Delete` | Delete the selection |
| `-` / `=` | Zoom out / in |
| `W`, `Z` | Fit the clip |
| `Esc` | Close the editor |

### Text/number fields (band name field, dB fields, tempo, time signature, renames)

Enter commits; Esc abandons; clicking anywhere else **commits** (never discards); Backspace edits. While any field is focused, single-key shortcuts are suppressed.

---

## 4. Mouse conventions

**Dragging is relative everywhere.** Pressing a fader or knob never makes the handle jump; the value follows how far the hand moves, and the hand may leave the control (and the window) and keep going. Travel budgets: band knobs 260 pt, band/mixer faders 720/420 pt (0.1 dB per point), header dials 420 pt, drumpad knobs 150 pt horizontal.

- **Shift while dragging = fine (0.1x)**, applied per frame (order of click-and-shift never re-prices distance already moved).
- **Double-click a knob/fader/dial = reset to its shipped default** (sends to zero; LPF/limiter to fully open; levels to unity or their shipped dB; tempo to default BPM). In the mixer, double-click a **name** = rename; in the arrangement, double-click a **MIDI clip** = open its editor, double-click a **note** = delete it.
- **Click the number to type it.** Every draggable control's readout (knob caps, dB readouts, drumpad values, LCD signature) is a click-to-type target; Enter lands it.
- **Right-click**:
  - empty room / piano / band chrome = the app context menu (recorder-first on the band, staff-first on the sheet music, global-only cut on the arrangement and MIDI editor);
  - a band effect knob = its parameter panel; a fader = nothing (deliberately);
  - a loaded insert bay (mixer, header, drumpad) = DELETE FX / REPLACE FX menu; an occupied drum pad = pad menu; a MIDI note = erase;
  - a mixer strip = Track menu; a lane header = Track menu; a clip = Delete/Copy menu;
  - the grid box (arrange or editor) = one step coarser;
  - empty lanes/roll = right-drag marquee selection (Shift keeps existing);
  - on macOS, Ctrl+click is right-click (Qt convention).
- **Alt**: Alt+click a loaded insert bay empties it (both views and pad chips); Alt+wheel pans the viewport; Alt+drag empty arrangement room pans.
- **Wheel**: plain = zoom at the playhead (arrange/editor) or scroll channels (mixer); Shift+wheel = how many lanes/channels/keys show; Ctrl+Alt+wheel = grid finer/coarser; Ctrl+wheel (editor) = walk the piano roll; over the grid box = turn the grid.
- **Modal overlays swallow the press that dismisses them** - a press outside a menu/popup closes it and does not also land underneath.
- Drag-and-drop: OS files onto arrangement lanes (audio/MIDI import) and onto drum pads; media-explorer file rows drag onto pads; clicking a media file row auditions it.
- The capo, the heart, and the transpose chevrons sit on top of the surfaces beneath them and take the click first.

## Notable persisted settings (`settings.rs`)

All of the above choices persist in one settings file: theme, key/chord/recorder colors, note-name and view toggles, window size percent (50-200) and positions for all detached windows, tuning/capo/wood/capo style, theory order + follow-MIDI, staff set/key/custom stack, plugin folders, take destination/name/devices/count-in/signature, per-channel gains/pans/kinds/icons/names/inserts/sends, effect mixes and parameters, drumpad kit/page/media default folder, grid/snap/swing per view, and metronome state. "Reset Settings to Default" (menu) restores everything. Arm states are deliberately session-only (a project must never open live), as is input monitoring.

---


# UX Review — strengths, friction, verified gap list

# Tangent UX review (read-only, from code)

Scope: `tangent-ui/src` (all 29 modules) plus `tangent/src/desktop.rs` and supporting spots in `tangent/src/main.rs`, `tangent-host/src/editor.rs`. Version reviewed: 4.92.0 working tree.

## Strengths (specific, with evidence)

1. **The keybind system is architecturally honest.** All single-key bindings live in ONE table (`keys.rs` `BINDINGS`), the H-card is rendered FROM that table, and `available()` gates both `pressed()` and `draw_help()` - so "listed but dead" and "live but unlisted" are the same impossible thing (keys.rs:120-132). The card is per-view (`HelpView::{Piano,Mixer,Arrange,MidiEdit}`), re-describes stolen keys in their local meaning ("S: split the clip at the playhead" on Arrange), and appends gesture rows per surface. The hold-to-show interaction (press H, read, release) has no stuck state, animates so it doesn't read as a glitch, and the layout algorithm picks the BIGGEST legible text across 1-5 columns rather than the first that fits (keys.rs `layout`), with the key column measured, not assumed.

2. **Error surfacing is a designed chain, not an afterthought.** The band's one status line composes worst-news-first (desktop.rs:1207-1276): camera-opening notice > `engine_error` (a dead output silences the whole app, and the comment records that the yank test proved the old order backwards) > input error > camera error > camera-open-but-black (with per-platform fix text - macOS names the exact Privacy pane) > camera denied > "this take will be missing X" (shown only while rolling, explicitly to avoid training users to ignore the line) > last take's note. The take report itself was promoted out of the strip into a `TakeSummary` dialog, whose "don't show again" suppresses only ordinary reports - "the disk filled" stays unsuppressable (dialogs.rs:180-197). A silent take gets a line, not a modal, with an actionable message ("check the input, and the mute buttons in the mixer").

3. **Failure recovery is proactive.** When the engine dies with retries spent, a background `Reprobe` watches for the device's return and reopens the audio path the frame it is seen (desktop.rs:1142-1161). Plugin load failures name both the plugin and the channel ("TAL-Reverb did not load on Channel 3: ..."). A VST3 rescan that finds nothing distinguishes "0 found" from "never ran" and tells the truth about VST2: "N .dll files seen are VST2, which Tangent does not host" (desktop.rs:1797-1808) - replacing an earlier message that sent users hunting a ReaPlugs VST3 build that does not exist.

4. **Dead-control prevention is a lived rule.** `EditorHandle::has_editor` exists because a Windows tester found a live "open window" button that could only fail; the fix greys the row instead of apologizing after the click (editor.rs:205-227). The export dialog reserves footer height for refusals so the button row never walks under the cursor (dialogs.rs `EXPORT_FOOTER_H`). Tempo is edited as text so "9" on the way to "92" is never clamped to 20 mid-keystroke. The Linux `FileBrowser` dialog exists because rfd on a portal-less box returns `None` indistinguishably from Cancel - a button that silently did nothing became a real file browser (dialogs.rs:154-179).

5. **First-run is deliberately shaped.** First launch opens with everything on (`Settings::first_launch`, settings.rs:1458-1477) so features are seen rather than hidden, with the mic-permission cost documented as a chosen trade. The Welcome card waits for the splash, is fit-tested against its own text with explicit slack after a real overflow incident (dialogs.rs:1016-1033), teaches the three interaction pillars (hold H, Tab, right-click), and frames the donation ask as explicitly non-gating.

6. **One consistency machine per concern.** Every dialog goes through `show_dialog_viewport` (27 call sites) - modal, positioned, X11-typed as Dialog so tiling WMs don't bury it invisibly (dialogs.rs:2374-2429). Every panel (`recorder_panel`, `mixer_panel`, `arrangement_panel`) is a pure painter with `draw` and `hit_test` derived from one `Layout`, so picture and click targets cannot disagree - and that same purity is what lets the band render into the recorded video composite. Settings carry a version stamp (v19) with migrations written against frozen historical constants, after the owner personally hit the "a default is a one-shot" trap four times (settings.rs:22-58).

7. **The band is designed for a pianist's actual body.** Idle vs rolling are two layouts, not one that dims: rolling collapses the preview and triples the timecode for readability from a bench two metres away, destination controls vanish mid-take, and the monitor fader is pinned to the same rectangle in both layouts because it is the one control you still reach for (recorder_panel.rs:1-46). The limiter knob is red because it is the one effect that alters a take whether or not anyone is listening (recorder_panel.rs `Fx::cap`).

8. **Menu craft.** Two levels maximum by construction, categories name subjects and hovers carry verbs, toggles rename themselves instead of using checkmarks, "Select MIDI Input..." was promoted to the top row because it is the first thing a new user needs, a right-click ON the recorder band reorders (never filters) its categories to the front, and separators are joined between blocks so gaps never move as bands open and close (menu.rs:714-1210).

9. **Undo is real and safe.** Whole-document snapshots with the desk baked in, 100 deep; fader release commits a moment; refused mid-take because the recorder owns those frames; applying an undo invalidates every selection that might name a dead clip (app.rs:6260-6354).

## Weaknesses / friction

1. **The discoverability cliff after Welcome.** "Hold H" is taught exactly once, in the Welcome card (dialogs.rs:988), which many users will dismiss with "don't show again" on day one. There is no persistent "?" affordance, and the right-click menu has no Help/Shortcuts row (explicitly considered and rejected as a hover invented to hold one row, menu.rs:1136). A user who forgets H has no path back to the entire keyboard vocabulary.

2. **Undo/redo is completely unadvertised.** Ctrl+Z / Ctrl+Shift+Z are wired directly in app.rs:15602-15615, outside the `BINDINGS` table - so they appear on no H card, in no menu, nowhere. The app's single most confidence-giving feature for a learner ("you cannot break anything") is invisible unless the user guesses.

3. **Two keybinding systems, one guarantee.** The one-table promise covers bare letters only. Every chorded binding (Ctrl+T new track, Q quantise, W fit, -/= zoom, Del, Shift+1/2/3 views, Ctrl+Z) lives as hand-written strings in `card_rows` (keys.rs:400-463) with handlers elsewhere in app.rs. A few are pinned by tests; most can silently drift from what the card claims.

4. **Per-view letter overloading is documented but arbitrary.** S = supporter key (piano) but split-clip (arrange); M = manage chords vs metronome; G = guitar view vs snap magnet. And bare letters fire real settings changes: a new user exploratorily typing flips borderless (B), dark mode (D), typeface (F) - none confirmable, none undoable (settings are outside the undo history).

5. **Accessibility is near zero.** No AccessKit anywhere in the dependency tree (absent from Cargo.lock), and every main surface is a raw `Painter` - a screen reader perceives nothing at all. Faders and knobs are mouse-drag only (Shift for fine); there is no keyboard nudge. The help card's font floor is 6pt. The window is fixed-size (`with_resizable(false)`, main.rs:280) with seven percentage presets as the only scaling. Good instincts exist (band ink follows background brightness, mono sources draw one meter bar "because two identical bars is a lie"), but they are visual-design instincts, not accessibility ones.

6. **Identity drift.** The About dialog still reads "Simple MIDI Keyboard Monitor with Advanced Chord Detection" (dialogs.rs:4653) - materially wrong for a 16-channel DAW with video takes, and the first thing a curious user opens. The window title is always "Tangent"; the open project's name appears only inside the Project dialog. main.rs:318 still claims Windows/Linux plugin editors are unimplemented; editor.rs supports all three platforms (the stale rationale also silently underpins the glow-renderer choice on those platforms).

7. **"Reset Settings to Default" is a one-click catastrophe.** No confirmation (app.rs:12928-12940), present in every context menu's tail directly under About, and it restores `first_launch()` wholesale - including desk gains, insert choices, chosen devices, and record folder, which all live in settings.

8. **Theme fragmentation.** One key (D) toggles one of three stored dark flags depending on what is open (me_dark / arr_dark / global, app.rs:12504-12514) - a user who "turned on dark mode" will find other views unchanged and no UI ever shows the three switches side by side. The ReaStream dialog is hard-coded light in either theme. Dialog affirmatives are split OK(9) / Done(5) / Close(4).

9. **Modal absolutism.** Every dialog drops all main-window input including H itself, and only one dialog can exist. Fine individually; combined with no menu bar and right-click as the sole command surface, a stuck or off-screen dialog is an app that has "frozen" (the code knows this - it is why the X11 Dialog type exists - but the desktop path relies on the OS never misplacing the window).

10. **Mouse-only learners hit a dead piano.** `keytoggle_enabled` defaults false (settings.rs:1124), so clicking the piano on first run does nothing; the K toggle is discoverable only via the H card. For the no-MIDI-hardware hobbyist this is a silent first-minute failure.

11. **Gesture-only timeline vocabulary.** Shift-drag to draw a MIDI item, shift-edge to stretch, alt-click to empty a bay, corner-drag to fade - all H-card-only. The empty lane's circled-plus is a good affordance, but an empty arrangement shows no ghost text teaching the one gesture (shift-drag) that creates music from nothing.

## Corrected gap list (verified against code)

| Capsule claim | Verdict | Evidence |
|---|---|---|
| No automation lanes | **Confirmed** | Zero hits for automation anywhere; no CC/controller lanes either - the MIDI editor has a velocity lane only (midi_editor.rs:687) |
| No plugin-delay compensation | **Confirmed** | tangent-host never queries or applies plugin latency (one passing comment, instance.rs:760) |
| Fixed 3 inserts/strip | **Confirmed** | `pub const INSERTS: usize = 3` (recorder.rs:491) |
| No time-stretch | **Needs correction** | Audio time-stretch: absent. But MIDI clips DO stretch (shift-drag edge scales notes and all, in arrange and editor, app.rs:800-803) |
| No comping | **Confirmed** | Takes land in timestamped folders; no take lanes/comping infrastructure |
| No surround | **Confirmed** | Stereo throughout; pan is -1..1 (mixer_panel.rs) |
| VST3-only | **Confirmed** | Scan counts VST2 .dlls only to explain them; CLAP/AUv3 mentioned as future aspiration (host.rs:15) |
| No notation editing | **Confirmed** | staff.rs has no edit interactions; the theory band's staff hit-test returns None (theory_panel.rs:523) |

**Additions the capsule missed:**
- No MIDI CC lanes (sustain/mod recording plays through, but is not editable; velocity only).
- No tempo track or time-signature changes: one global BPM (20-300) and one signature per project.
- Plugin editors work on macOS, Windows AND Linux (main.rs comment claiming otherwise is stale).
- Undo covers arrangement+desk only; settings, colors, and recorder configuration are not undoable.
- No screen-reader support at all (no AccessKit); fixed-size window, preset scaling only.
- One send per strip to a single FX bus; no groups or flexible routing.

## Top 10 UX improvements, ranked for the teacher / self-learner / hobbyist-creator audience

1. **A persistent help affordance.** A small "H" glyph in a corner of every view, plus a "Keyboard Shortcuts" menu row that toggles the existing card. The card is already excellent; the only failure is that its front door is taught once and forgotten. Highest value-per-line-of-code in the app.

2. **Advertise undo/redo.** Put Ctrl+Z / Ctrl+Shift+Z on every H card's GENERAL section and as menu rows. For learners, knowing mistakes are reversible changes how boldly they explore; right now the app's safety net is a secret.

3. **Sound in the first minute for mouse-only users.** Default `keytoggle_enabled` to true on first launch (or make a bare click audition the note with a one-time hint naming K). The no-hardware hobbyist currently meets a piano that ignores clicks.

4. **Confirm "Reset Settings to Default".** One dialog ("This resets the desk, devices, and colors - taught chords are kept") or make it undoable. It currently sits one slip below "About" and destroys a configured desk silently.

5. **Fold the chorded bindings into the guaranteed table.** Extend `BINDINGS` (or add a parallel chord table with the same available/describe/test machinery) so Ctrl+T, Q, W, Del, Shift+1/2/3 cannot drift from their card rows. This protects the app's single best UX invariant as the surface grows.

6. **Project identity in the chrome.** Title the window "Tangent - Nocturne", and show the project name somewhere always visible on Arrange/Mixer. Teachers juggling one project per student currently have no way to tell which is open without opening a dialog.

7. **Basic accessibility floor.** Enable eframe's AccessKit feature so dialogs and labels reach screen readers, and add keyboard nudge (arrows = 1 dB / fine) to whichever fader was last touched. The pure-painter surfaces make full semantics expensive, but the current answer to assistive tech is literally nothing.

8. **Unify dark mode** into one visible setting with optional per-view overrides shown together (a row in the cog), and settle OK/Done/Close to one affirmative per dialog type. The three hidden dark flags produce "dark mode is broken" reports from users who did nothing wrong.

9. **Empty-state ghost text on the arrangement.** One dim line in an empty timeline - "shift-drag to draw a MIDI item · double-click to edit · hold H for everything" - vanishing on first clip. It teaches the creation gesture at the exact moment of need, matching the circled-plus pattern already there.

10. **Refresh the About identity and stale platform notes.** New tagline reflecting what Tangent is now, and correct the main.rs editor comment (then re-evaluate the glow renderer on Windows/Linux, since the GL-context-conflict reasoning that keeps them on glow was written when those platforms had no plugin editors). Small, but About is the app introducing itself, and it introduces the wrong app.

**Overall:** this is an unusually self-aware codebase - failure modes are named, trades are documented with their reasons, and the error-message culture (say the true thing, name the fix, never offer a dead control) is better than most commercial DAWs. The dominant UX risk is concentration: the entire command surface hangs off two discoverable-once facts (hold H, right-click), and the app's rapidly growing chorded vocabulary has outgrown the machinery that kept the original letter keys honest.

---


# Licensing: VST3 Hosting

# VST3 Hosting Licensing for Tangent (commercial, MIT-licensed, sold on Gumroad)

## 1. Repo fact-check

**`/Users/ganten/Dropbox/Projects/Apps/tangent/tangent-host/Cargo.toml`** uses exactly one VST3-related crate:

- `vst3 = "0.3"` (resolves to `vst3 0.3.0` in `Cargo.lock`), the coupler-rs binding (`MIT OR Apache-2.0`, repo `https://github.com/coupler-rs/vst3-rs`). The Cargo.toml comment is explicit that it was chosen because it ships a pre-generated `bindings.rs` (~670 KB), so there is no build-time libclang, no SDK download, no C toolchain, which keeps `cargo xwin` cross-builds working. It is NOT Steinberg's C++ SDK and NOT the GPLv3 `vst3-sys` crate (that one is only reached by the separate `Tangent.vst3` plugin via NIH-plug, not by the host).
- The Cargo.toml header comment already states: "MIT, and that is newly possible: Steinberg relicensed the VST3 SDK from GPLv3-or-proprietary to MIT on 2025-10-29 (VST 3.8). See LICENSING.md."

**`/Users/ganten/Dropbox/Projects/Apps/tangent/LICENSING.md`** documents the split: standalone app MIT, `Tangent.vst3` plugin GPLv3 (because of `vst3-sys` via NIH-plug), the GPLv3 section 5 "aggregate" analysis for shipping both in one installer, and a correction dated 2026-08-15 recording the Steinberg MIT relicensing, verified against `steinbergmedia/vst3sdk` and `steinbergmedia/vst3_pluginterfaces` LICENSE files. It also carries a trademark section ("VST" must not appear in the product name) and a "not legal advice" note suggesting counsel before the first paid release.

**`/Users/ganten/Dropbox/Projects/Apps/tangent/THIRD-PARTY-LICENSES`** lists `vst3 0.3.0` (license: MIT OR Apache-2.0) and `com-scrape-types 0.1.1`, both from `coupler-rs/vst3-rs`. **Gap found: there is no Steinberg entry anywhere in the file** (no "Steinberg" match at all). See checklist item 2 below.

I independently verified LICENSING.md's claims during this research; they are accurate.

## 2. The headline: the dual license is dead, VST3 is MIT since 2025-10-29

With the VST 3.8 SDK (SDK tagged 2025-10-20, announced 2025-10-29), Steinberg replaced the old dual-licensing model (GPLv3 OR proprietary "Steinberg VST 3 License Agreement") with the plain **MIT License**. Verified at source: `steinbergmedia/vst3sdk` `LICENSE.txt` is standard MIT, "Copyright (c) 2026, Steinberg Media Technologies GmbH". Steinberg's press release: "The MIT License of the VST 3.8 SDK permits free use, modification, and distribution of software, as long as the original copyright and license text are retained... allowing developers to freely use VST technology in both commercial and non-commercial software products." The developer portal's licensing page: "Since version 3.8, VST 3 is licensed under MIT license" and "Licensing under GPLv3 and the Steinberg proprietary license is no longer available." Steinberg's FAQ confirms commercial closed distribution: you "may distribute or sell your plug-in/host in binary form as freeware, shareware, or a paid version", and the MIT license "does not require you to disclose your source code." Previously signed proprietary agreements remain valid for existing products, but new products simply use the MIT SDK; there is nothing to sign anymore.

**Consequence: a commercial, closed OR open host needs no agreement with Steinberg at all.** The only copyright obligation is MIT notice retention; the only other consideration is trademark (section 4).

## 3. The old dual license (historical context, and what "signing" involved)

Until 2025-10-29 the SDK was dual-licensed, and the choice applied explicitly to "your VST 3 plug-in/host" (hosts were covered by the same agreement as plug-ins):

- **Proprietary Steinberg VST 3 license**: allowed binary-only (closed) distribution. Per the archived developer portal: "You need written permission from Steinberg Media Technologies GmbH in order to distribute your VST 3 plug-in/host (Steinberg will send you the countersigned License agreement included in the SDK that you signed and sent to us)." You filled in the agreement PDF from the SDK package, signed it, sent it by mail, fax, or email to reception@steinberg.de, and Steinberg countersigned and returned it.
- **Cost: free, confirmed.** The agreement text itself (§4 Fees and Royalties): "This license is non-royalty bearing and the Licensee shall not obligated to pay to Steinberg any fees or royalties with respect to the VST PlugIn Interface technology." But it had real terms: no redistribution of the SDK, no reverse-engineering of VST products, mandatory Steinberg credit in documentation and about box, VST logo on packaging and promotional material, and compliance with the Steinberg VST usage guidelines. The grant explicitly covered "the development of VST PlugIns and/or... an application that can host VST PlugIns."
- **GPLv3**: the alternative for open-source plug-ins/hosts; your host's source (the parts linking SDK code) had to be GPLv3-compatible and published. Usage guidelines applied under this path too.

This is why, historically, every closed commercial host (Ableton Live, REAPER, FL Studio, Bitwig, etc.) had a signed Steinberg agreement, and GPL hosts (Ardour, OBS's VST filter, Carla) used the GPLv3 leg. The visible residue is the trademark credit: e.g. Ableton's legal page still carries "VST is a trademark of Steinberg Media Technologies GmbH." Under MIT, new hosts (and JUCE/CLAP-world developers generally, per community discussion) no longer sign anything; the standard practice that remains is the about-box/docs trademark credit when the product advertises VST compatibility.

## 4. Trademark: separate from copyright, and now optional

"VST" is a registered trademark of Steinberg (e.g. US Reg. No. 2499337). Trademark law is independent of the SDK's copyright license, so the MIT relicensing did not release the mark. Steinberg's current position (developer portal): **"Trademark usage (e.g. 'VST' name or logo) is optional under MIT license, but if used, must comply with Steinberg's official trademark rules."** Non-compliance with logo guidelines does not affect your MIT rights, but may be a trademark violation. The current usage guidelines, if you use the mark:

- First use of "VST" in product-related material carries the ® symbol; credits/documentation must include: **"VST is a registered trademark of Steinberg Media Technologies GmbH."**
- The VST Compatible Logo should appear on webpages/packaging/about box referencing the product's VST support.
- Product naming: "VST" only as plain text in a "for VST" construction (e.g. "MyCoolEngine for VST"), never stylized or merged into the brand. **"VST" in a company name is strictly prohibited.** LICENSING.md's rule ("Tangent" is fine, never "Tangent VST") remains correct.

So: no, trademark usage does not require signing any agreement (there is nothing left to sign); it requires following the published guidelines whenever you choose to say "VST" in marketing or UI.

## 5. Does an independent Rust binding change the obligations?

- **Before MIT it was the contested question.** Steinberg's position was that the interface definitions themselves were licensed subject matter, which is why RustAudio's `vst3-sys` declared itself GPL-3.0 (bindings translated from the dual-licensed headers) and why NIH-plug warns that VST3 plugins built with it must be GPLv3-compliant. An "independent" binding never clearly escaped the obligation, because the bindings are derived from Steinberg's headers.
- **After MIT it is moot, and Tangent's chain is clean.** I verified the provenance of the exact crate the host ships: `vst3 0.3.0`'s pre-generated bindings were produced by com-scrape from the `coupler-rs/vst3_pluginterfaces` submodule pinned at commit `31d6eeba` titled **"VST SDK 3.8.0"** (2025-10-20), i.e. the MIT-licensed headers. The crate itself is `MIT OR Apache-2.0`. So the host's entire VST3 surface descends from MIT-licensed material.
- **One residual nit:** the published `vst3 0.3.0` crate contains no Steinberg copyright notice (I grepped the vendored source at `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/vst3-0.3.0/`; only the crate's own MIT/Apache texts are present, and `bindings.rs` derives from Steinberg's headers). MIT requires the copyright notice to accompany "copies or substantial portions of the Software." Whether machine-generated bindings are a "substantial portion" of the headers is a judgment call, but the conservative fix costs one paragraph: add a Steinberg MIT entry to THIRD-PARTY-LICENSES.

## 6. Concrete obligations for Tangent (selling on Gumroad, MIT-licensed)

Selling the standalone host commercially requires **no permission, no agreement, no fee, and no source disclosure** (the repo is public anyway). The complete set of real obligations:

1. **MIT notice retention (Steinberg).** Ship Steinberg's MIT copyright line with the app, since the `vst3` crate's bindings derive from the SDK 3.8 headers.
2. **MIT notice retention (crates).** Already handled by THIRD-PARTY-LICENSES for `vst3`/`com-scrape-types`; keep shipping that file.
3. **Trademark hygiene, only where "VST" appears.** Product name stays "Tangent" (never "Tangent VST"); if the Gumroad page, README, or about box says "VST"/"VST3", use "VST®" on first use and add the credit line "VST is a registered trademark of Steinberg Media Technologies GmbH." The Compatible Logo is part of the guidelines if you lean on the mark in marketing; the minimal-risk alternative is to describe the feature as "hosts VST3 plug-ins" with the credit line and skip the logo.
4. **Nothing from the plugin side leaks in.** `Tangent.vst3`'s GPLv3 (via `vst3-sys`) is confined to the plugin binary; the LICENSING.md aggregate rules (optional install, never link, each carries its own license) keep the paid standalone MIT. Unchanged by anything above.

### Action checklist

- [ ] Add a Steinberg entry to `/Users/ganten/Dropbox/Projects/Apps/tangent/THIRD-PARTY-LICENSES`: "VST 3 API / pluginterfaces, Copyright (c) Steinberg Media Technologies GmbH, MIT License (bindings in the `vst3` crate are generated from the VST 3.8 SDK headers)", with the MIT text or a pointer to it.
- [ ] Audit every place "VST" appears in shipped/published text (about box, README, CHANGELOG, Gumroad listing, website): first use gets ®, and add the credit line "VST is a registered trademark of Steinberg Media Technologies GmbH" to the about box and docs.
- [ ] Confirm the Gumroad listing never uses "VST" in the product title or any stylized branding; "hosts VST3 plug-ins" in the body copy is the safe pattern.
- [ ] Decide on the VST Compatible Logo: either adopt it per the guidelines (about box + product webpage) or deliberately don't use the logo at all; don't half-use it.
- [ ] Keep `scripts/build-plugin.sh` shipping GPLv3 text + pinned NIH-plug revision inside `Tangent.vst3` (existing LICENSING.md obligation; unrelated to hosting but part of the same release).
- [ ] Optional, cheap insurance: pin/record in LICENSING.md that `vst3 0.3.0`'s bindings derive from the SDK 3.8.0 (MIT) header commit (`coupler-rs/vst3_pluginterfaces@31d6eeba`, "VST SDK 3.8.0"), so the provenance argument is written down.
- [ ] Per LICENSING.md's own note: the first paid release is the right moment for a one-off legal review; the questions to bring are the aggregate analysis and the trademark usage, not SDK licensing (which is now trivially clean).

## Sources

- Steinberg press release, "VST 3 Now Available Under MIT License" (2025-10-29): https://www.steinberg.net/press/2025/vst-3-8/ (PDF: https://ocl-steinberg-live.steinberg.net/_storage/asset/819253/storage/master/Press%20Release%20-%202025-10-29%20-%20VST%203.8%20-%20EN.pdf)
- VST3 SDK license file (MIT): https://github.com/steinbergmedia/vst3sdk/blob/master/LICENSE.txt
- VST 3 Developer Portal, Licensing: https://steinbergmedia.github.io/vst3_dev_portal/pages/VST+3+Licensing/Index.html
- VST 3 Developer Portal, Licensing FAQ (sell in binary form, no source disclosure, old agreements remain valid): https://steinbergmedia.github.io/vst3_dev_portal/pages/FAQ/Licensing.html
- VST 3 Developer Portal, Steinberg VST usage guidelines (trademark rules, optional under MIT): https://steinbergmedia.github.io/vst3_dev_portal/pages/VST+3+Licensing/Usage+guidelines.html
- Steinberg forum, "VST 3.8.0 SDK Released": https://forums.steinberg.net/t/vst-3-8-0-sdk-released/1011988
- Archived pre-MIT licensing options page (countersigned agreement, plug-in/host wording): https://web.archive.org/web/2024/https://steinbergmedia.github.io/vst3_dev_portal/pages/VST+3+Licensing/What+are+the+licensing+options.html
- Old Steinberg VST Plug-In SDK Licensing Agreement PDF (§2 host grant, §4 no fees/royalties): https://web.archive.org/web/2023id_/https://www.steinberg.net/fileadmin/redaktion/PDF/VST_SDKs/VST3_License_Agreement.pdf
- KVR news, "Steinberg Moves VST 3 SDK to MIT Open Source License, ASIO Now GPLv3": https://www.kvraudio.com/news/steinberg-moves-vst-3-sdk-to-mit-open-source-license-asio-now-gplv3-65179
- Sound On Sound, "Steinberg adopt MIT License for VST3": https://www.soundonsound.com/news/steinberg-adopt-mit-license-vst3
- CDM, "Open Steinberg: VST3 and ASIO SDKs now have open source licenses": https://cdm.link/open-steinberg-vst3-and-asio/
- coupler-rs vst3 binding (MIT OR Apache-2.0): https://github.com/coupler-rs/vst3-rs and its header source pin https://github.com/coupler-rs/vst3_pluginterfaces (commit "VST SDK 3.8.0", 2025-10-20)
- KVR forum thread, developer reaction to the MIT change: https://www.kvraudio.com/forum/viewtopic.php?t=624544
- Ableton trademark acknowledgment example: https://www.ableton.com/en/copyright-and-licensing/
- "VST" US trademark registration: https://trademarks.justia.com/755/84/vst-75584165.html

*Not legal advice; this is a research summary consistent with the repo's own LICENSING.md analysis.*

---


# Licensing: Bundled ffmpeg Compliance

# Tangent's bundled ffmpeg: what ships, whose build it is, and what the licenses require

## Part 1: Repo facts

### Whose ffmpeg build is shipped (not an own compile; two third-party GPL builds)

`/Users/ganten/Library/CloudStorage/Dropbox/Projects/Apps/tangent/scripts/fetch-ffmpeg.sh` downloads pinned, sha256-verified upstream builds into `dist/vendor/`, renames the binary to `tangent-ffmpeg` / `tangent-ffmpeg.exe`, and stages a `ffmpeg-licenses/` folder (upstream license text plus a generated `SOURCE.txt` with the exact URL and checksum) beside it:

| Platform | Build | Source | License |
|---|---|---|---|
| Windows x64 | **BtbN autobuild** `ffmpeg-n8.1.3-win64-gpl-8.1.zip`, dated tag `autobuild-2026-09-22-13-18` (BtbN prunes dated tags after a few weeks - the previous pin, `autobuild-2026-08-17-13-05`, was gone by 2026-09-22; `scripts/fetch-ffmpeg.sh` keeps the verified archive in the gitignored `vendor/ffmpeg/`) | github.com/BtbN/FFmpeg-Builds | **GPL** ("gpl" variant; `--enable-gpl --enable-version3` so effectively **GPLv3**) |
| Linux x86_64 / arm64 | **John Van Sickle static build** ffmpeg 7.0.2 | johnvansickle.com/ffmpeg | **GPLv3** (his page says so explicitly; tarball ships `GPLv3.txt`) |
| macOS | **none**: video via AVFoundation/`AVAssetWriter` (`tangent-record/src/encode/macos.rs`), no ffmpeg in the artifact | n/a | n/a |

I extracted the configure lines from the binaries actually cached in `dist/vendor/`:

- Linux (`dist/vendor/linux/tangent-ffmpeg`): `--enable-gpl --enable-version3 --enable-static ... --enable-libmp3lame ... --enable-libx264 --enable-libx265 ...` (no `--enable-vaapi`, a known cost documented in fetch-ffmpeg.sh).
- Windows (`dist/vendor/windows/tangent-ffmpeg.exe`): `--enable-gpl --enable-version3 ... --enable-libmp3lame ... --enable-libopenh264 ... --enable-amf --enable-ffnvcodec --enable-libvpl --enable-vaapi ... --enable-libx264 --enable-libx265 ...`

So both bundled binaries are GPL builds, and because `--enable-version3` is set, the effective license of each whole binary is **GPLv3**, matching the `GPLv3.txt` / `LICENSE.txt` shipped beside them.

### Packaging

- `scripts/build-cross.sh` (lines 305-319, 423-427): copies `tangent-ffmpeg(.exe)` and `ffmpeg-licenses/` into the Linux stage and the Windows stage (`WINSTAGE`).
- `installer/windows/tangent.nsi` Section `SEC_APP` does `File /r "${SRCDIR}/app/*.*"` into `$PROGRAMFILES64\Tangent`, so `tangent-ffmpeg.exe` and `ffmpeg-licenses/` land in the install dir next to `tangent.exe`. The click-through license page shows Tangent's MIT `LICENSE.txt` only.
- `scripts/build-installer.sh` (lines 265-269, 307-308) stages the Linux sidecar and asserts `ffmpeg-licenses/GPLv3.txt` is in the artifact.
- `THIRD-PARTY-LICENSES` (generated by `scripts/gen-third-party-licenses.sh`) has a "Bundled video encoder" section naming the GPL builds, the trademark, the provenance note, and the separate-process reasoning.

### How it is invoked (`tangent-record/src/encode/ffmpeg.rs`, `encode.rs`, `proc.rs`, `camera/windows.rs`)

Everything is a spawned subprocess via `std::process::Command` (wrapped by `crate::proc::command`, which only adds `CREATE_NO_WINDOW` on Windows). Resolution order in `program()`: `TANGENT_FFMPEG` env var, then exe-adjacent `tangent-ffmpeg(.exe)`, then bare `ffmpeg` on PATH.

1. **Video takes** (`encode/ffmpeg.rs::Encoder::create`, line 230): raw BGRA frames piped to stdin, **`-c:v libx264 -preset veryfast -pix_fmt yuv420p -movflags +faststart`**. **libx264 is hardcoded as the H.264 encoder** on Windows and Linux. The mux pass (line 550) uses `-c:v copy -c:a aac -b:a 192k`: AAC comes from FFmpeg's **built-in native AAC encoder** (part of LGPL libavcodec, no external lib).
2. **MP3 export** (`encode.rs::encode_mp3_via_ffmpeg`, line 266): `-c:a libmp3lame -b:a 320k`. This path is compiled off macOS and used where the in-process encoder is absent, which in shipping builds is **Windows only** (see LAME finding below).
3. **Windows camera** (`camera/windows.rs`): `-list_devices`/`-list_options` with `-f dshow`, then a capture process decoding mjpeg to rgba over stdout. The `dshow` indevice and mjpeg decoder are LGPL parts of ffmpeg, but they ride inside the GPL binary.

Communication is exec + command-line args + byte pipes (raw BGRA in, rgba out). No shared address space, no complex internal data structures exchanged. This is squarely the "separate programs" side of the FSF's line.

### Side finding: the app itself statically links LGPL LAME on macOS and Linux

`tangent/Cargo.toml`: `default = ["recorder", "mp3"]`, `mp3 = ["dep:mp3lame-encoder"]`. The Windows build passes `--no-default-features` (build-cross.sh line 406) so it has no in-process LAME, but the **macOS and Linux builds compile `mp3lame-sys` 0.1.11, which vendors and statically links `lame-3.100`**. The crates declare `LGPL-3.0`; the vendored LAME itself is LGPL with an explicit "commercial use is fine under LGPL restrictions, acknowledge LAME" note. **`THIRD-PARTY-LICENSES` currently has no LAME/mp3lame entry at all** (grep confirms), so the MIT app binaries on two platforms embed LGPL code that the attribution file does not mention. This is a real, fixable gap (see checklist).

### Two more repo facts that matter

- The `ivory` repo **is public on GitHub** (`github.com/ganten1998/tangent` returns 200 unauthenticated; Codeberg copy is private). That makes LGPL relink/source obligations for the app satisfiable by pointing at the repo.
- `docs/RECORDER-PLAN.md` (lines 1091-1140) contains an earlier analysis that *rejected* bundling GPL ffmpeg and analyzed openh264 (including why Cisco's patent grant does not cover a self-compiled openh264). The shipped 4.x implementation went the other way (GPL sidecar); the two documents now disagree and 5.0 should reconcile them.

## Part 2: What the licenses actually require

### The core question: does GPL ffmpeg infect the paid app? No.

- **Spawning a GPL program as a subprocess does not make the caller GPL.** The FSF's own FAQ: "pipes, sockets and command-line arguments are communication mechanisms normally used between two separate programs... when they are used for communication, the modules normally are separate programs" (only "exchanging complex internal data structures" would change that). Raw pixel bytes over a pipe are not intimate semantics. ([GPL FAQ, MereAggregation](https://www.gnu.org/licenses/gpl-faq.en.html#MereAggregation))
- **Distributing GPL software alongside a proprietary/MIT product is expressly permitted** if the programs "communicate at arms length" and the free status of the GPL part is made clear. ([GPL FAQ, GPLInProprietarySystem](https://www.gnu.org/licenses/gpl-faq.en.html#GPLInProprietarySystem))
- **Charging money changes nothing.** "The right to sell copies is part of the definition of free software"; you may charge any fee, including for an aggregate containing GPL parts. ([GPL FAQ, DoesTheGPLAllowMoney](https://www.gnu.org/licenses/gpl-faq.en.html#DoesTheGPLAllowMoney))

So Tangent stays MIT and can be sold on Gumroad with the GPL sidecar. The existing reasoning in `LICENSING.md` and `fetch-ffmpeg.sh` is correct. **But** conveying the ffmpeg binary makes *you* a GPL distributor *of that binary*, and those obligations are real and currently only half met.

### Obligations for conveying the GPLv3 ffmpeg binary (both bundles are effectively GPLv3)

1. **License text with the binary.** Done: `ffmpeg-licenses/GPLv3.txt` / `LICENSE.txt` ship beside it.
2. **No further restrictions on it.** The Gumroad terms/EULA and installer click-through must not purport to restrict copying of `tangent-ffmpeg`. `THIRD-PARTY-LICENSES` already says the GPL terms apply to that binary; keep the MIT click-through clearly scoped to Tangent.
3. **Corresponding Source, GPLv3 section 6.** For a download product the operative option is **6(d)**: "offer equivalent access to the Corresponding Source in the same way through the same place at no further charge." The source may live on a different server, "operated by you or a third party," provided there are "clear directions next to the object code" and **"you remain obligated to ensure that it is available for as long as needed"** (GPLv3 section 6(d), text in the repo's own `LICENSE-GPL-3.0`; confirmed by [GPL FAQ, SourceAndBinaryOnDifferentSites](https://www.gnu.org/licenses/gpl-faq.en.html#SourceAndBinaryOnDifferentSites)). Note: the "3-year written offer" (6b) is for physical products, and the pass-along-the-offer option (6c) is "only occasionally and noncommercially," so **neither is available to a paid download**; 6(d) is the path.
4. **Corresponding Source means the whole static binary's source**, "all the source code needed to generate, install, and run" it, including the statically linked third-party libraries and the scripts controlling compilation. A bare pointer to ffmpeg.org is not the corresponding source of a static build with forty bundled libraries.

**Where Tangent stands today:** `SOURCE.txt` points at ffmpeg.org and the build's download page. That is a good-faith direction but strictly insufficient:

- The **BtbN release page hosts binaries only** (I listed the `autobuild-2026-08-17-13-05` assets via the GitHub API: 49 archives, zero source). The corresponding source is reconstructible only via the FFmpeg-Builds repo at that tag (which pins each library's commit in `scripts.d/`), and BtbN is under no obligation to keep any of it up for your benefit.
- **John Van Sickle does publish sources** ("All static builds available here are licensed under the GNU General Public License version 3" with `git-source` / `release-source` directories on [johnvansickle.com/ffmpeg](https://johnvansickle.com/ffmpeg/)), but his site is one hobbyist server and old releases rotate.

The robust fix is cheap: mirror the corresponding source archives yourself and link them from the same place the binaries are offered.

### The libx264 question, answered directly

**Yes, libx264 is the H.264 encoder Tangent relies on** on Windows and Linux (hardcoded at `encode/ffmpeg.rs:230`), and x264 is **GPLv2+** with a commercial-license alternative from x264 LLC ([x264.org/licensing](https://x264.org/licensing/), [VideoLAN x264 page](https://www.videolan.org/developers/x264.html)). **But it does not "matter enormously" in the feared way.** It matters for *which ffmpeg build variant you must ship* (a "gpl" build, which you already do, with the whole binary conveyed under GPL), not for Tangent's own license, because the arm's-length subprocess boundary holds. The FFmpeg legal page's warnings about avoiding libx264 ([ffmpeg.org/legal.html](https://www.ffmpeg.org/legal.html)) are aimed at apps that *link* the FFmpeg libraries and want LGPL treatment; Tangent is not in that situation.

The separate thing x264 genuinely carries is **patents**: distributing an AVC encoder in a paid product sits under the Via LA (ex-MPEG LA) AVC pool, which is **royalty-free for the first 100,000 units per year** ([Via LA AVC/H.264 program](https://www.via-la.com/licensing-programs/avc-h-264/)); `docs/RECORDER-PLAN.md:1132` already adopts exactly this argument. At Tangent's volume this is the sound position, and note x264's own licensing page says a patent license question exists "whether under the open source GPL or a commercial license," so switching encoders does not remove it (openh264 compiled from source has the same exposure; only Cisco's *separately downloaded binary* carries Cisco's grant, per `openh264.org/BINARY_LICENSE.txt` as analyzed in RECORDER-PLAN.md:1120-1130).

### LGPL components

- **libmp3lame inside the sidecar** (LGPL) and the **native AAC encoder** (LGPL part of libavcodec): inside a `--enable-gpl` build these are conveyed as part of a GPLv3 whole; there are no additional separate obligations beyond the binary's GPL ones.
- **LAME statically linked into the app** (macOS/Linux): LGPL for a *statically linked* library requires letting users modify LAME and relink it with the app (LGPLv3 section 4(d), LGPL-2.1 section 6). Because Tangent's full source is MIT and public, this is satisfied in substance (anyone can rebuild the app against a modified LAME), but LGPL also requires **prominent notice and the LGPL text**, which is currently missing from `THIRD-PARTY-LICENSES`.

## Part 3: Compliance checklist for Tangent 5.0 on Gumroad

Ship-blocking (do before 5.0):

1. **Host the Corresponding Source and link it "next to the object code."** Create a `tangent-ffmpeg source` location you control (GitHub release on `ganten1998/tangent`, or a Codeberg release; the repo is already public) containing, per bundled build: (a) the JVS `release-source` bundle for ffmpeg 7.0.2 as fetched from johnvansickle.com, and (b) for EACH BtbN build that has shipped: n8.1.3 (current, `autobuild-2026-09-22-13-18`, ffmpeg release tag `n8.1.3`) and n8.1.2 (every Windows build from e5ae43b up to d6547f6, that is 4.4.1 through 5.0.0-beta.13 and the tester builds of that span: the FFmpeg-Builds commit behind the now-deleted `autobuild-2026-08-17-13-05`, ffmpeg at `7c533d0f86`), each with the FFmpeg-Builds repo archived at that point (the tag's `scripts.d/` pins every library commit; archiving the fetched sources too is the gold-plated version). Then put the URL: on the **Gumroad product page** (the "place" the object code is offered), in **`SOURCE.txt`** inside `ffmpeg-licenses/`, and in **`THIRD-PARTY-LICENSES`**. Keep it up as long as 5.0 is sold plus a safety margin.
2. **Pin the archives forever.** `dist/vendor/*.tar.xz|zip` are already cached and sha256-pinned; also copy them (and the source bundles) somewhere durable outside Dropbox sync accidents, since you must be *able* to produce the source for as long as you distribute the binaries.
3. **Add LAME to `THIRD-PARTY-LICENSES`** (and the LGPL text to the artifact license folder) for the macOS and Linux apps: "This application incorporates the LAME MP3 encoder (lame-3.100, LGPL), statically linked; complete source including build scripts at github.com/ganten1998/tangent" plus the acknowledgment/link LAME's own LICENSE asks for. Investigate why `gen-third-party-licenses.sh` (cargo-license over Cargo.lock) missed `mp3lame-encoder`/`mp3lame-sys` and regenerate.
4. **Scope the EULA/click-through.** One sentence in the Gumroad description and installer text: the bundled `tangent-ffmpeg` is a separate GPLv3 program, not covered by Tangent's terms, license and source in `ffmpeg-licenses/`. (This is also exactly the "form" the FSF asks for in GPLInProprietarySystem: make the free program's status clear.)
5. **Keep the three aggregate-preserving rules in `LICENSING.md`** true for the sidecar as well: the app must keep degrading gracefully without `tangent-ffmpeg` (it does: audio+MIDI still record), no linking, license text beside each part.

Already done (verify, don't rebuild):

6. License texts ship beside the binary (`GPLv3.txt` / BtbN `LICENSE.txt`), with provenance `SOURCE.txt`. Good.
7. Unmodified upstream builds, renamed only (renaming is fine; FFmpeg's "do not rename to obscure" checklist item concerns hiding FFmpeg's presence, and `SOURCE.txt`/`THIRD-PARTY-LICENSES` name it plainly). Trademark acknowledgment for FFmpeg present. Good.
8. FFmpeg attribution in an about box / download page, per FFmpeg's own checklist ([ffmpeg.org/legal.html](https://www.ffmpeg.org/legal.html)): "This software uses code of FFmpeg licensed under the GPLv3 and its source can be downloaded [link]". Worth adding to the Gumroad page while editing it anyway.

Posture items:

9. **Patents:** rely on the Via LA AVC threshold (royalty-free under 100,000 units/year) and write that position down in LICENSING.md, as RECORDER-PLAN.md already does for the openh264 case. No action at current volume; revisit if Tangent ever approaches six figures of yearly units.
10. **`LICENSING.md`'s "first release that takes money" note:** 5.0 on Gumroad is precisely that moment; a one-hour review by an actual lawyer of the aggregate + 6(d) setup would close the file.

## Part 4: Must we switch encoders? No.

**No switch is required.** The GPL permits selling an aggregate containing a GPL binary; the obligations are the paperwork above, all cheap. Switching would buy freedom *from the source-hosting obligation*, at real cost:

- **LGPL-only ffmpeg build** (BtbN publishes `win64-lgpl` in the same releases; its scripts confirm x264 is excluded from lgpl variants while libmp3lame and openh264 remain): you would lose `libx264` and have to move `-c:v` to `libopenh264` (Constrained Baseline only, roughly 1.5x bitrate for equal quality per your own RECORDER-PLAN numbers, and no Cisco patent shelter for a static build) or to hardware encoders (`h264_amf`/`h264_nvenc`/`h264_qsv`/`h264_vaapi`), which are absent or broken on exactly the low-end machines the encoder path was hardened for (see the libva SIGABRT war story in fetch-ffmpeg.sh). And an LGPL *static* binary still carries source-availability obligations, so the hosting work does not even disappear.
- **Keeping GPL x264** costs: one hosted source bundle and a few lines of text, while keeping the best software H.264 encoder, working MP3 via libmp3lame on Windows, and dshow capture, all in the one binary already validated on canary.

Recommendation: keep the current GPL sidecar for 5.0, execute checklist items 1-5, and note in `RECORDER-PLAN.md` that its "no GPL ffmpeg" rejection was superseded by the shipped 4.x design plus this compliance work.

## Sources

- Repo: `/Users/ganten/Library/CloudStorage/Dropbox/Projects/Apps/tangent/scripts/fetch-ffmpeg.sh`, `scripts/build-cross.sh`, `scripts/build-installer.sh`, `scripts/gen-third-party-licenses.sh`, `installer/windows/tangent.nsi`, `tangent-record/src/encode/ffmpeg.rs`, `tangent-record/src/encode.rs`, `tangent-record/src/proc.rs`, `tangent-record/src/camera/windows.rs`, `LICENSING.md`, `THIRD-PARTY-LICENSES`, `docs/RECORDER-PLAN.md`, `dist/vendor/*` (configure lines extracted from the binaries), `tangent/Cargo.toml`, `~/.cargo/registry/.../mp3lame-sys-0.1.11`
- [FFmpeg License and Legal Considerations](https://www.ffmpeg.org/legal.html)
- [GNU GPL FAQ: MereAggregation](https://www.gnu.org/licenses/gpl-faq.en.html#MereAggregation), [GPLInProprietarySystem](https://www.gnu.org/licenses/gpl-faq.en.html#GPLInProprietarySystem), [DoesTheGPLAllowMoney](https://www.gnu.org/licenses/gpl-faq.en.html#DoesTheGPLAllowMoney), [UnchangedJustBinary](https://www.gnu.org/licenses/gpl-faq.en.html#UnchangedJustBinary), [SourceAndBinaryOnDifferentSites](https://www.gnu.org/licenses/gpl-faq.en.html#SourceAndBinaryOnDifferentSites), [DoesTheGPLAllowDownloadFee](https://www.gnu.org/licenses/gpl-faq.en.html#DoesTheGPLAllowDownloadFee)
- GPLv3 section 6 text (repo copy at `LICENSE-GPL-3.0`; canonical at [gnu.org/licenses/gpl-3.0](https://www.gnu.org/licenses/gpl-3.0.en.html))
- [BtbN FFmpeg-Builds](https://github.com/BtbN/FFmpeg-Builds) (README variant descriptions; release `autobuild-2026-08-17-13-05` asset list via GitHub API showing binaries only; `scripts.d/50-x264.sh` lgpl gating, `50-openh264.sh`, `50-libmp3lame.sh`)
- [John Van Sickle FFmpeg Static Builds](https://johnvansickle.com/ffmpeg/) (GPLv3 statement, source directories)
- [x264 licensing (x264 LLC)](https://x264.org/licensing/), [VideoLAN x264](https://www.videolan.org/developers/x264.html)
- [Via LA AVC/H.264 licensing program](https://www.via-la.com/licensing-programs/avc-h-264/) (100,000 units/year royalty-free threshold)

Not legal advice; a written-down reading of the licenses, consistent with the repo's existing `LICENSING.md` posture.

---


# Naming: 'Tangent' Trademark Risk

# "Tangent" name clearance research — music production app (DAW) on Gumroad

**Scope note:** This is a knockout search from public sources, not a professional clearance search. I verified US records live via USPTO TSDR; EUIPO via a secondary database; UKIPO's search is captcha-gated so UK registrations could not be directly enumerated (flagged below).

## Risk rating: **LOW** for cease-and-desist exposure as an indie on Gumroad; **MEDIUM** on two narrower axes (EU sales at scale, and your own ability to register/enforce the mark)

---

## 1. Collisions found

### A. Tangent Wave Ltd ("Tangent") — color-grading control surfaces (UK) — *the one you asked about*
- [tangentwave.co.uk](https://www.tangentwave.co.uk/) brands itself simply as **"Tangent"**: control surfaces (Wave, Wave2, Element, Ripple) for **color grading, editing and video post-production**. Well known in film/post circles; integrates with Resolve, Premiere, etc. ([products](https://www.tangentwave.co.uk/products/), [applications](https://www.tangentwave.co.uk/applications/))
- UK company [TANGENT WAVE LIMITED, no. 06787617](https://find-and-update.company-information.service.gov.uk/company/06787617).
- **No US trademark registration for them surfaced** in any search; they appear to rely on the "Tangent Wave" trade name and any UK/common-law rights. (UKIPO not directly searchable from here; caveat.)
- Overlap analysis: creative-pro tools, yes, but **hardware for video** vs **software for music**. Different goods, different buyers, different sales channels (broadcast/post resellers vs Gumroad). Classic multi-factor confusion analysis (similarity of goods, channels, purchaser sophistication) favors coexistence. Their customers are colorists who buy $1k–$4k panels; confusion with a music DAW is implausible. Realistic C&D likelihood: **low**. They are also a small UK firm, not a litigious brand-policing operation.

### B. Tangent A/S — Danish consumer hi-fi (speakers, amps, radios) — *the strongest formal collision*
- [tangent-audio.com](https://www.tangent-audio.com/), Danish brand est. 1996, sells speakers, amplifiers, CD players, radios ([Gear Patrol coverage](https://www.gearpatrol.com/audio/tangent-spectrum-ii-x5-active-bt/)).
- Holds **live EUTM 003710373 "TANGENT"** (word), **Nice class 9**, filed 2004, registered 2008, **renewed 2014 and 2024**, covering "electric and electronic apparatus and equipment for … processing, reproduction, control, amplification … of sound or image signals" ([TrademarkElite record](https://www.trademarkelite.com/europe/trademark/trademark-detail/003710373/TANGENT)).
- This is an identical word mark, in class 9, in the audio field, live in the EU. Mitigating factors: the goods are **apparatus/hardware**, not downloadable software; they are consumer hi-fi, not pro audio production; EU practice does not treat all of class 9 as similar goods. But if you sell meaningfully into the EU or ever attempt an EU registration of "TANGENT" for audio software, this mark is the likely citation/objection. This is the main reason the rating is not flat-low.
- Historical UK cousin: "TANGENT" / "TANGENT UK" marks of [Tangent Acoustics UK Limited](https://trademark.justia.com/owners/tangent-acoustics-uk-limited-249281) (1981, hi-fi speakers; the vintage British speaker brand). Almost certainly lapsed, but shows the name has deep audio-hardware history.

### C. Music-software space: effectively clear
- **No DAW, plugin, or music app named plain "Tangent" currently exists** in any marketplace I checked: nothing on [KVR](https://www.kvraudio.com/), nothing music-related named Tangent on [Gumroad](https://gumroad.com/music-and-sound-design/sound-design/plugins), nothing on the iOS App Store, and **no Eurorack module named Tangent on ModularGrid** (the rumored Eurorack collision does not exist; the nearest is Instruo [tanh[3]](https://www.instruomodular.com/wp-content/uploads/2019/09/tanh3-Manual.pdf), a hyperbolic-tangent waveshaper).
- Near names, all small and non-blocking:
  - [Tangent Edge Instruments](https://www.kvraudio.com/developer/tangent-edge-instruments) — one-person Dutch sample-library developer on KVR (since 2014). Different name ("Tangent Edge"), tiny, no registration found. Low.
  - "Tangent 1.0" — a 1990s generative-MIDI shareware program, long dead ([legacy download page](https://softwiki.net/Tangent.html), [retro Facebook post](https://www.facebook.com/groups/891767077528360/posts/3306043412767369/)).
  - [Tangent Notes](https://www.tangentnotes.com/Download) — free open-source note-taking app; different category.
- Music-culture uses (goods too different to matter, listed for completeness): prog band **The Tangent**; Dutch electronic duo [Tangent](https://tangentnl.bandcamp.com/); assorted small "Tangent" record labels. Also a nice fact for you: **"tangent" is the generic name of the striking blade in a clavichord / tangent piano**, which both makes the name apt and makes it hard for anyone to monopolize the word in a music context.

### D. US register snapshot (verified live on TSDR, 2026-08-25)
| Mark | Serial/Reg | Owner | Goods | Status |
|---|---|---|---|---|
| TANGENT | SN 86037774 / Reg 4501552 | Pixite Inc. (creative photo app) | Class 9, photo-editing mobile app | **DEAD** — cancelled Feb 2021 (Section 8) |
| TANGENT | SN 73109551 / Reg 1123948 | Tangent Systems, Inc. | Class 9 (1970s) | **DEAD** — cancelled 1986 |
| TANGENT | SN 88833810 / Reg 6163427 | (medical-computing company) | Class 9, hardware+software for **medical patient monitoring** | **LIVE** (reg. Sep 2020) |

Source pages: [TSDR sn86037774](https://tsdr.uspto.gov/statusview/sn86037774), [TSDR sn88833810](https://tsdr.uspto.gov/statusview/sn88833810), [TSDR sn73109551](https://tsdr.uspto.gov/statusview/sn73109551); discovery via [uspto.report](https://uspto.report/TM/86037774) and [Justia Trademarks](https://trademarks.justia.com/). Peripheral live marks in other fields: TANGENT LAW ([legal services](https://trademarks.justia.com/878/04/tangent-87804554.html)), Tangent Medical's NOVACATH, DARK TANGENT ([DEF CON](https://www.trademarkia.com/dark-tangent-77769149)), plus cosmetics/apparel owners. "Tangent" is a **crowded, dilute field** across industries, which weakens everyone's ability to assert it broadly.

---

## 2. Realistic assessment for an indie

1. **Likelihood of confusion, honestly:** A music DAW named Tangent vs a color-grading panel maker and a Danish hi-fi brand. Goods, channels, and price points all differ. No live US registration covers anything close to "downloadable music production software." The only identical live US class 9 mark is for medical patient monitoring. A confusion claim from any of these owners would be weak.
2. **C&D probability:** Low. The parties most likely to even notice are Tangent Wave (if you exhibit at NAB-type events or market "control surface support" hard) and Tangent A/S (if you sell hardware or push into EU consumer retail). A Gumroad-distributed indie DAW is far below both radars, and both would have a hard case.
3. **Is "Tangent" + distinct logo/category defensible?** Yes, in the US: the Pixite precedent shows "TANGENT" was registrable for a creative app as recently as 2014, and that registration is now dead. An application for "downloadable software for music production/recording" has a decent shot; the medical-monitoring registration is the only live citation risk and the goods are distinguishable. In the **EU**, the Tangent A/S mark makes registration and large-scale sales riskier; that is the medium-risk pocket.
4. **The flip side of a common-word mark:** you will also have weak offensive rights. If someone else ships "Tangent"-anything in music later, you'll be relying on your own registration and first-use evidence, so paper them now (see mitigations).

## 3. Low-cost mitigations (do these, no rename needed)

- **Always pair the name with the category** in commerce and metadata: "Tangent — music production app", store listings, site `<title>`, About box. "Tangent DAW" as the informal handle is fine and cheap insurance. (This is exactly the "distinct logo + category" play, and it works here because no one owns the word in your category.)
- **Do NOT brand as "Tangent Audio."** That is effectively the Danish company's identity (tangent-audio.com). This is the one sub-name to avoid.
- **File a US TEAS application** for TANGENT, class 9, "downloadable software for music production/recording/editing" (~$350 for one class, self-filed). First use is already in commerce (Gumroad sales), so a use-based filing works. This converts your squishy common-law position into a blocking registration before anyone else in music grabs it.
- **Keep first-use evidence**: dated Gumroad listing, release announcements, invoices. Costs nothing.
- **Stay software.** The moment you ship Tangent-branded *hardware* (a controller, an audio interface), you walk into both Tangent A/S's EUTM goods description and Tangent Wave's turf. Software keeps clean water between you and every live mark found.
- Optional, later: if EU revenue becomes real, get a one-hour opinion from an EU trademark attorney about coexistence with EUTM 003710373 before filing anything there, rather than filing blind.

## 4. Alternative-name directions (only relevant to the medium-risk pockets; no rename is required for the current Gumroad model)

If you ever decide to rename before the brand hardens (e.g., ahead of EU retail or a hardware companion), directions that keep the flavor:

1. **Coined tangent-derivatives** that are ownable as marks: *Tangently, Tangint, Tanjent* (coined spellings register and enforce far better than dictionary words).
2. **Two-word lockups with Tangent kept as the hero word**: *Tangent Line, Tangent Lab, Tangent Tracks* (avoid "Tangent Audio" and "Tangent Wave" specifically).
3. **Sibling geometry terms with no live audio presence** (verify each before use): *Secant, Radian, Osculate/Oscule*. Skip *Chord* (Chord Electronics, UK hi-fi) and *Vertex* (crowded).
4. **The clavichord angle**: names from tangent-piano history (*Tangentenflügel* territory), e.g. *Tangenta*: nods to the mechanism that literally strikes strings, on-brand for a music app and highly distinctive.

**Sources:** [Tangent Wave](https://www.tangentwave.co.uk/) ([products](https://www.tangentwave.co.uk/products/), [Companies House](https://find-and-update.company-information.service.gov.uk/company/06787617)) · [Tangent A/S / tangent-audio.com](https://www.tangent-audio.com/) ([Gear Patrol](https://www.gearpatrol.com/audio/tangent-spectrum-ii-x5-active-bt/)) · [EUTM 003710373 via TrademarkElite](https://www.trademarkelite.com/europe/trademark/trademark-detail/003710373/TANGENT) · USPTO TSDR live records: [86037774](https://tsdr.uspto.gov/statusview/sn86037774), [88833810](https://tsdr.uspto.gov/statusview/sn88833810), [73109551](https://tsdr.uspto.gov/statusview/sn73109551) · [Justia Trademarks](https://trademarks.justia.com/) ([Tangent Acoustics UK owner page](https://trademark.justia.com/owners/tangent-acoustics-uk-limited-249281), [TANGENT LAW](https://trademarks.justia.com/878/04/tangent-87804554.html)) · [uspto.report Pixite record](https://uspto.report/TM/86037774) · [Trademarkia DARK TANGENT](https://www.trademarkia.com/dark-tangent-77769149) · [KVR: Tangent Edge Instruments](https://www.kvraudio.com/developer/tangent-edge-instruments) · [ModularGrid module browser](https://modulargrid.net/e/modules/browser?SearchName=tangent) · [Instruo tanh[3] manual](https://www.instruomodular.com/wp-content/uploads/2019/09/tanh3-Manual.pdf) · [Tangent 1.0 legacy page](https://softwiki.net/Tangent.html) · [Tangent (NL duo) Bandcamp](https://tangentnl.bandcamp.com/) · [Tangent Notes](https://www.tangentnotes.com/Download)

*Caveats: UKIPO's register is captcha-gated and could not be enumerated (Tangent Wave may hold UK registrations not visible here); USPTO's new search UI is WAF-protected, so US coverage came from TSDR lookups of marks surfaced by secondary indexes rather than an exhaustive register sweep. Not legal advice; for a real clearance opinion before heavy brand investment, a single-class attorney search runs a few hundred dollars.*

---


# Gumroad Mechanics & Store Setup

# Selling Tangent on Gumroad in 2026 — Research Report

## 1. File hosting limits and delivering updates

**Size limits.** Paid products (priced above ~$0.99) allow files up to **16 GB each**, with no cap on the number of files per product; **free products are limited to 250 MB** per file ([Gumroad Help: file size limits](https://gumroad.com/help/article/289-file-size-limits-on-gumroad)). Gumroad has in the past tightened free/$0+ limits abruptly (a 2021 episode cut free items to 25 MB before the current 250 MB policy, [CGPress](https://cgpress.org/archives/gumroad-restricts-file-sizes-of-free-and-0-items-to-25mb.html)), so a $0-floor PWYW product should be treated as living under the "free" bucket. Tangent's payloads (18 MB dmg, 45 MB exe, 11 MB tarball) fit comfortably under either limit. The "Download all" button only appears if total content is under 500 MB.

**Updates to past buyers.** The flow is exactly "re-upload + optional email": edit the product, add/replace files, save; buyers' library links always serve the **current** files, and every past buyer can re-download free ([Gumroad Help: updating a product](https://gumroad.helpjuice.com/11169-Creator-FAQ/how-do-i-update-my-product)). You then optionally send an email blast from the **Updates/Emails tab** to everyone who bought ([Gumroad Help: send an update](https://gumroad.com/help/article/169-how-to-send-an-update)), or automate onboarding emails with [Workflows](https://help.gumroad.com/article/131-using-workflows-to-send-automated-updates). Caveat: the one-click "notify customers of new files" prompt has been flaky since the open-source transition; there's an open issue about the button being missing, so plan on composing the update email manually ([antiwork/gumroad #9](https://github.com/antiwork/gumroad/issues/9)).

**"Versions"** on Gumroad are *variants* of one product (each variant can carry its own files and price), not release versioning ([Gumroad Help: setting up versions](https://help.gumroad.com/article/126-setting-up-versions-on-a-digital-product)). Useful for offering macOS/Windows/Linux as selectable variants of one listing, or a "Supporter" variant — not for keeping 4.x and 5.0 archives. If you want old versions to stay downloadable, just leave the old files attached alongside the new ones (files are additive; unlimited count).

## 2. License Key API and how purchases would mint *your* keys

Gumroad has a built-in per-product toggle ("generate a unique license key per sale"). Keys are Gumroad-format strings shown on the receipt, and you verify them **online** via `POST https://api.gumroad.com/v2/licenses/verify` with `product_id`, `license_key`, and optional `increment_uses_count` (default true); the response includes `success`, `uses`, and a `purchase` object with `test`, `refunded`, `chargebacked`, `email`, `sale_timestamp` ([Gumroad Help: license keys](https://gumroad.com/help/article/76-license-keys); worked example in [Šević's dev.to writeup](https://dev.to/zsevic/license-key-verification-with-gumroad-api-58f9)). Important: this is a server-verified system with no offline story and no licensing events (activation/deactivation webhooks don't exist — [LicenseSeat's critique](https://licenseseat.com/alternative-to-gumroad)). It is **not a replacement** for your offline Ed25519 supporter keys; it's best used as a *proof-of-purchase token*.

**Purchase notifications** come two ways:

- **Ping**: a form-encoded POST to a URL you set in advanced settings, fired on every sale, carrying `sale_id`, `product_id`, `product_permalink`, `email`, `full_name`, `price`, `variants`, `test`, `custom_fields`, and — when key generation is on — the `license_key` ([Gumroad Ping](https://gumroad.com/ping); field usage documented in [Botble's Gumroad integration docs](https://docs.botble.com/license-manager/gumroad-integration.html)).
- **Resource subscriptions**: `PUT https://api.gumroad.com/v2/resource_subscriptions` with an OAuth `access_token`, a `resource_name`, and a `post_url`; valid resources include `sale`, `refund`, `dispute`, `dispute_won`, `cancellation` ([gumroad_elixir client docs](https://hexdocs.pm/gumroad_elixir/Gumroad.Client.html), [Gumroad API](https://gumroad.com/api)).

Neither webhook is cryptographically signed, so the fulfilment service must treat inbound pings as hints and confirm authoritatively — either by calling `licenses/verify` with the ping's `license_key`, or the Sales API with the `sale_id` — before minting anything.

## 3. Pricing models: PWYW vs fixed

Enter `0+` as the price for pay-what-you-want with a true $0 floor; buyers can take it free, but **if they pay anything, the minimum charge is $0.99**. Any `N+` sets a paid floor, and a **suggested price** field anchors what people actually pay ([Gumroad Help: PWYW pricing](https://help.gumroad.com/article/133-pay-what-you-want-pricing)). Third-party analysis of ~146k Gumroad products found PWYW products get ~8% more sales but that average paid amounts collapse without a strong suggested-price anchor ([InsightRaider](https://insightraider.com/en/answers/does-gumroad-let-you-offer-pay-what-you-want)). Note the interaction with hosting: a `0+` product likely sits under the 250 MB free-file limit — irrelevant at Tangent's sizes, but a reason some sellers split "free download" and "supporter purchase" into two listings.

## 4. Gumroad's cut in 2026

- **Direct sales** (your link/profile): **10% + $0.50**, plus payment processing (~2.9% + $0.30 Stripe card; PayPal ~3.49% + $0.49) — effectively ~13-14% on a $20-30 sale, ~21% on a $10 sale ([gumroad.com/pricing](https://gumroad.com/pricing), [Swell breakdown](https://www.swell.is/content/gumroad-pricing), [Checkoutpage](https://checkoutpage.com/blog/gumroad-fees)).
- **Discover-attributed sales**: **flat 30%**, processing included ([gumroad.com/pricing](https://gumroad.com/pricing)).
- No monthly fee, no listing fee; the old volume sliding scale is gone since 2023 ([WeAreFounders](https://www.wearefounders.uk/gumroad-fees-2026-what-sellers-actually-pay-per-sale/)).
- New in March 2026: **unverified accounts must accrue $100 before first payout**; completing identity verification drops the threshold back to $10 ([roo.beehiiv fees guide](https://roo.beehiiv.com/p/gumroad-fees-2026)).
- Fee math at supporter-tier prices: a $5 payment nets roughly $3.55 (~29% total friction) because of the two fixed fees; $15 nets ~$12.30. Floors below ~$5 are punishing.

## 5. VAT / sales tax — merchant of record

Since **January 1, 2025, Gumroad is the Merchant of Record on every transaction**: it calculates, collects, and remits EU/UK VAT, US sales tax, and GST worldwide at no extra fee, with no registrations or configuration on your side ([Gumroad announcement](https://gumroad.gumroad.com/p/gumroad-is-becoming-a-merchant-of-record-more-updates), [Gumroad Help: VAT](https://gumroad.com/help/article/10-dealing-with-vat), [LegalClarity](https://legalclarity.org/how-does-gumroad-handle-sales-tax-for-sellers/)). This is the single strongest argument for Gumroad over rolling checkout on the existing Fly.io service: an EU sale via your own Stripe would otherwise make *you* the VAT-liable seller.

## 6. Refund norms for software

Gumroad lets each creator set a per-product refund policy — from 30-day money-back to firm no-refunds ([Gumroad: specify a refund policy](https://gumroad.gumroad.com/p/specify-a-refund-policy-for-your-products), [custom refund policy](https://gumroad.com/help/article/335-custom-refund-policy)). In January 2025 Gumroad silently defaulted every store to a 30-day money-back guarantee; you can switch it back in settings ([seller PSA on X](https://x.com/sardamit/status/1877255869521346951)). Norms for indie software are 14-30 days no-questions-asked; a no-refunds policy doesn't stop chargebacks, and excessive disputes risk account suspension ([Gumroad Help: refund policy](https://gumroad.com/help/article/51-what-is-gumroads-refund-policy)). Since your Ed25519 keys verify offline and can't be remotely revoked, a refunded buyer keeps a working key — at supporter-tier prices that's an acceptable cost of a friendly 14- or 30-day policy, and friendliness suppresses chargebacks.

## 7. Discover / marketplace visibility

Discover is Gumroad's search-and-recommendation marketplace; public products are surfaced there and Discover-attributed purchases carry the flat 30% fee — you can't selectively opt a public product out ([Checkoutpage](https://checkoutpage.com/blog/how-gumroad-pricing-works-and-a-cheaper-alternative)). There are established categories for exactly this niche — [Music & Sound Design → Plugins](https://gumroad.com/music-and-sound-design/sound-design/plugins) and [Software & Plugins](https://gumroad.com/software-development/software-and-plugins) — with healthy VST/audio-tool listings, so tagging well ("VST", "synth", "chord", "MIDI") gets real incidental traffic. Treat 30% Discover sales as found money, not the main channel.

## 8. Policy issues for audio tools

None specific to audio software. The [prohibited products list](https://gumroad.com/prohibited) bars unauthorized copies of copyrighted software/media (including OEM/bundled software) — original audio tools are squarely allowed and widely sold. The only audio-adjacent trap is shipping uncleared copyrighted sample content inside the product; Tangent's DX7-style patches and synth engine are original, so no exposure. The list can change abruptly with card-network rules, so it's worth a re-skim before launch.

---

## Recommended Gumroad setup for Tangent 5.0

**Recommendation: keep Tangent free, sell the supporter tier on Gumroad (Option A).** It preserves the existing identity (free instrument + supporter keys), reuses the Ed25519/Fly.io machinery you've already validated, offloads VAT/MoR entirely, and fixes the current fulfilment pain points (manual `announce`, the Reply-To black hole) by letting Gumroad own receipts and buyer email.

**Option A — free app + paid "Tangent Supporter" product (recommended)**
- Listing 1: *Tangent 5.0* — price `0+` (or plain $0), suggested $0, all three platform files attached (18/45/11 MB — fine under the 250 MB free cap). This exists to capture emails for update blasts and to be visible on Discover.
- Listing 2: *Tangent Supporter* — PWYW with a paid floor, `5+` suggested **$15** (the fixed $0.50 + $0.30 fees make sub-$5 floors mostly fee). Enable "generate a unique license key per sale." Deliverable: a short thank-you PDF/readme plus the key-redemption instructions. 14- or 30-day money-back policy.
- Updates: re-upload files on the free listing per release, send an Updates-tab email to both audiences. Buyers' links always fetch the newest build, so no stale-version support burden.

**Option B — paid 5.0, free 4.x**
- *Tangent 5* at a fixed $19-29 (paid bucket, 16 GB/file so no hosting concerns), 4.x left as a free listing or a plain download on your site. Cleaner revenue per user and a simpler story ("buy the app, get updates"), but it forfeits the goodwill/reach of a free instrument, makes every future major version a re-purchase decision, and puts your whole audience behind a paywall for a first-time indie product. Only pick this if 5.0 is a clearly bigger product than 4.x rather than a continuation.

Either way: verify the account identity early (avoids the $100 payout threshold), set the refund policy explicitly (don't inherit the silent 30-day default unknowingly), and fill in Discover category/tags.

## Integration sketch: Gumroad purchase → supporter key

Two viable patterns; **redeem-pull is the primary path**, with the Ping as an optional accelerator:

**Primary — redemption (pull), authoritative and unspoofable:**
1. Enable license key generation on the Supporter product. Buyer's receipt shows a Gumroad key.
2. Buyer visits `https://<fly-app>/redeem`, pastes the Gumroad key (+ the email/name to bind, matching however your Ed25519 payload is structured).
3. Fly service calls `POST https://api.gumroad.com/v2/licenses/verify` with `product_id` + the key. Accept only if `success && !purchase.refunded && !purchase.chargebacked && !purchase.test`; use `uses` (or your own DB keyed on `sale_id`) to cap redemptions to 1.
4. Service mints the Ed25519 supporter key with the signing seed and shows it on the page **and** emails it — no dependence on the buyer replying to anything, which sidesteps the Reply-To black hole.
5. On later `refund`/`dispute` resource-subscription events, mark the sale revoked in the service DB so the Gumroad key can't be re-redeemed (the already-minted offline key survives; accepted loss).

**Optional accelerator — Ping (push):** point Gumroad's Ping at `https://<fly-app>/gumroad-ping`; on receipt, *don't trust the payload* (it's unsigned) — take its `license_key`/`sale_id`, run the same `licenses/verify` confirmation, then mint and email the supporter key proactively so most buyers never need the redeem page. Idempotency on `sale_id` covers Ping retries/duplicates.

**Operational notes:** the Fly service needs a signing key — either move a *dedicated Gumroad-channel* Ed25519 seed there (and finally back up `~/.tangent-signing`, which is currently unbacked-up), or keep seeds off the server and have the service queue requests for a local `announce`-style batch signer; the former is the only way fulfilment is instant. Mind the existing `license` feature-flag trap in the codebase when 5.0 wires in key checking. Total new surface: one webhook route, one redeem route, one `licenses/verify` client, one table keyed on `sale_id`.

## Sources

- [Gumroad pricing](https://gumroad.com/pricing) · [Swell: Gumroad pricing 2026](https://www.swell.is/content/gumroad-pricing) · [Checkoutpage: Gumroad fees](https://checkoutpage.com/blog/gumroad-fees) · [WeAreFounders: fees per sale](https://www.wearefounders.uk/gumroad-fees-2026-what-sellers-actually-pay-per-sale/) · [roo.beehiiv: 2026 fees + $100 payout catch](https://roo.beehiiv.com/p/gumroad-fees-2026)
- [Help: file size limits](https://gumroad.com/help/article/289-file-size-limits-on-gumroad) · [CGPress: free-item limit episode](https://cgpress.org/archives/gumroad-restricts-file-sizes-of-free-and-0-items-to-25mb.html)
- [Help: updating a product](https://gumroad.helpjuice.com/11169-Creator-FAQ/how-do-i-update-my-product) · [Help: send an update](https://gumroad.com/help/article/169-how-to-send-an-update) · [Help: workflows](https://help.gumroad.com/article/131-using-workflows-to-send-automated-updates) · [Help: versions](https://help.gumroad.com/article/126-setting-up-versions-on-a-digital-product) · [antiwork/gumroad #9](https://github.com/antiwork/gumroad/issues/9)
- [Help: license keys](https://gumroad.com/help/article/76-license-keys) · [Šević: license verification walkthrough](https://dev.to/zsevic/license-key-verification-with-gumroad-api-58f9) · [LicenseSeat on Gumroad licensing gaps](https://licenseseat.com/alternative-to-gumroad) · [Gumroad Ping](https://gumroad.com/ping) · [Gumroad API](https://gumroad.com/api) · [Botble Gumroad integration](https://docs.botble.com/license-manager/gumroad-integration.html) · [gumroad_elixir resource subscriptions](https://hexdocs.pm/gumroad_elixir/Gumroad.Client.html)
- [Help: PWYW pricing](https://help.gumroad.com/article/133-pay-what-you-want-pricing) · [InsightRaider: PWYW data](https://insightraider.com/en/answers/does-gumroad-let-you-offer-pay-what-you-want)
- [Gumroad: becoming Merchant of Record](https://gumroad.gumroad.com/p/gumroad-is-becoming-a-merchant-of-record-more-updates) · [Help: dealing with VAT](https://gumroad.com/help/article/10-dealing-with-vat) · [LegalClarity: Gumroad sales tax](https://legalclarity.org/how-does-gumroad-handle-sales-tax-for-sellers/)
- [Help: Gumroad refund policy](https://gumroad.com/help/article/51-what-is-gumroads-refund-policy) · [Help: custom refund policy](https://gumroad.com/help/article/335-custom-refund-policy) · [Gumroad: specify a refund policy](https://gumroad.gumroad.com/p/specify-a-refund-policy-for-your-products) · [PSA: 30-day default switch](https://x.com/sardamit/status/1877255869521346951)
- [Checkoutpage: Discover fee mechanics](https://checkoutpage.com/blog/how-gumroad-pricing-works-and-a-cheaper-alternative) · [Gumroad prohibited products](https://gumroad.com/prohibited) · [Discover: audio plugins category](https://gumroad.com/music-and-sound-design/sound-design/plugins) · [Discover: software & plugins](https://gumroad.com/software-development/software-and-plugins)

---


# Windows Code Signing

# Windows Code Signing for an Indie Paid App (Outside the Microsoft Store), August 2026

## 1. What an unsigned installer actually looks like to a buyer

Two gates stand between a Gumroad buyer and your NSIS installer:

1. **The browser.** Edge (and Chrome, less aggressively) flags uncommon unsigned .exe downloads with "isn't commonly downloaded" / "make sure you trust it" interstitials that require extra clicks to keep the file.
2. **SmartScreen at launch.** For an unsigned or unknown file, the buyer gets the full-screen blue **"Windows protected your PC"** dialog. The only visible button is "Don't run." They must notice the small **"More info"** link, click it, then click **"Run anyway."** Publisher shows as "Unknown publisher." The Register memorably counted the full gauntlet at ~7 steps for an unsigned app ([The Register](https://www.theregister.com/2020/06/05/windows_10_microsoft_defender_smartscreen/)).

Microsoft's current official behavior table ([SmartScreen reputation for Windows app developers](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/smartscreen-reputation), updated May 2026):

| Signing | First-download behavior |
| --- | --- |
| Microsoft Store | No warning ever |
| Valid OV/EV cert | Warning ("unrecognized") until reputation accrues, but your **verified publisher name** is shown |
| Unsigned / self-signed | "Windows protected your PC" block; enterprise policy can prevent the bypass entirely |

Two facts that matter most for a solo dev:

- **Unsigned files rebuild reputation from zero on every release** (reputation is per file hash and "cannot transfer from previous versions unless both were signed using the same publisher identity"). Signed files accumulate reputation on the *certificate*, which carries across updates.
- **Smart App Control** (Windows 11, enabled at OOBE on many new consumer machines) "will block execution of unsigned files unless the file has a positive reputation," and it checks **all executables, not just downloaded ones**. For those users an unsigned app can be a hard no-run, not a warning.

## 2. OV vs EV: EV's SmartScreen advantage is dead

- Historically EV certs got instant SmartScreen reputation. **Microsoft removed that around August 2024.** Current Microsoft docs state it flatly: *"EV certificates no longer bypass SmartScreen... Paying a premium for EV solely to avoid SmartScreen warnings is no longer justified."* ([Microsoft Learn](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/smartscreen-reputation), also noted by [Advanced Installer](https://www.advancedinstaller.com/prevent-screen-from-appearing.html))
- OV and EV now behave identically for SmartScreen. EV only matters for kernel drivers (Partner Center) and some enterprise checklists. **For a $0-50 desktop app, EV is wasted money.**
- Azure Artifact Signing explicitly does not and will not issue EV certs ([Artifact Signing FAQ](https://learn.microsoft.com/en-us/azure/artifact-signing/faq)).

## 3. Azure Artifact Signing (formerly "Trusted Signing"): current state, verified

Renamed from Trusted Signing to **Azure Artifact Signing** and reached **GA in January 2026**; no functional change ([devclass, Jan 2026](https://www.devclass.com/security/2026/01/14/code-signing-windows-apps-may-be-easier-and-more-secure-with-new-azure-artifact-service/4079554), [product page](https://azure.microsoft.com/en-us/products/artifact-signing)).

**Pricing** ([Azure pricing page](https://azure.microsoft.com/en-us/pricing/details/artifact-signing/)):
- **Basic: $9.99/month, 5,000 signatures/month** (then $0.005/sig). One cert profile of each type.
- Premium: $99.99/month, 100,000 sigs. Irrelevant for you.
- Requires a **paid** Azure subscription (pay-as-you-go is fine; free/trial/sponsored subscriptions are rejected, per the FAQ). Not pro-rated: full month billed regardless of start date.

**Eligibility, verified current (Aug 2026)** ([Quickstart prerequisites](https://learn.microsoft.com/en-us/azure/artifact-signing/quickstart), doc dated 2026-05-21):
- **Organizations:** US, Canada, EU, UK, Australia, NZ, Japan, South Korea, Singapore, Switzerland, Norway, Israel.
- **Individual developers: US or Canada only.** Individual onboarding is open at GA.
- **The 3-year-history requirement is GONE.** It existed during the 2024-2025 preview; a Microsoft employee confirmed on 2026-08-17 that under GA there are "country/region onboarding pre-requisites only, no minimum organization age restrictions" ([Microsoft Q&A](https://learn.microsoft.com/en-us/answers/questions/5977141/azure-artifact-signing-trusted-signing-is-a-us-llc)). So a brand-new individual or LLC qualifies.

**Individual identity validation process** (from the [Quickstart](https://learn.microsoft.com/en-us/azure/artifact-signing/quickstart)): your name/address are auto-sourced from your **Azure billing account** (must be Account Type "Individual" and must exactly match your government ID), then you verify via **AU10TIX**: email PIN, phone, government ID photo + face check on your phone, Microsoft Authenticator Verified ID. Org validation takes 1-20 business days; individual VID is usually much faster.

**How it signs:** certificates are short-lived (rotated automatically, ~72-hour validity) and never leave Microsoft's FIPS 140-3 Level 3 HSMs; you sign via `signtool` + the Artifact Signing dlib, the GitHub Action, or the SDK, and every signature is timestamped (`timestamp.acs.microsoft.com`), which is what keeps signatures valid after the short cert expires. The cert subject is your validated legal name ("John Q. Ganten, Washington, US" style); no custom CN allowed. Because the certificate identity is stable, **SmartScreen reputation accrues on it across releases** just like a traditional cert. Practical writeups: [Melatonin/JUCE guide](https://melatonin.dev/blog/code-signing-on-windows-with-azure-trusted-signing/), [Tim Anderson's setup notes](https://www.itwriting.com/blog/12462-getting-started-with-azure-artifact-signing.html).

Caveat: no instant reputation here either. Signed-but-new apps can still show the softer "unrecognized app" prompt until downloads accumulate; the difference is your real name shows, the prompt dies off permanently, and it never comes back for updates once the cert has reputation.

## 4. Traditional CA price points (2026)

Since June 2023 all OV/IV certs ship on hardware tokens or in cloud HSMs (no more .pfx files). Also note: **from Feb 2026 CA/B rules cap code-signing cert lifetime at ~1 year** (Certum states max 459 days from Feb 27, 2026; DigiCert now sells 1-year only) ([Certum shop](https://shop.certum.eu/code-signing.html), [SSL Dragon](https://www.ssldragon.com/ssl-certificates/code-signing/), [comparison guide](https://ivansingleton.dev/the-cheapest-code-signing-certificate-for-business-central-appsource-in-2026-a-complete-comparison-guide/)).

| Option | Cost | Notes |
| --- | --- | --- |
| **Azure Artifact Signing Basic** | **~$120/yr** ($9.99/mo) | US/CA individuals OK; no token; cloud signing; 5,000 sigs/mo |
| **Certum cloud OV (SimplySign)** | ~$116-130/yr | Cheapest traditional OV; Polish CA; cloud or smartcard ([SSLmentor](https://www.sslmentor.com/certum/certumcodecloud)) |
| Certum "Open Source" IV | ~$50-69 | **Open-source projects only; not valid for a paid app** ([sslcertshop](https://sslcertshop.com/certum-open-source-code-signing), [piers.rocks writeup](https://piers.rocks/2025/10/30/certum-open-source-code-sign.html)) |
| SSL.com eSigner (IV/OV cloud) | from ~$180/yr + tiered signature counts, extra credentials $20/mo | ([eSigner pricing](https://www.ssl.com/guide/esigner-pricing-for-code-signing/)) |
| DigiCert OV | ~$400-550/yr (+$120 token) | Premium brand, zero SmartScreen advantage over the others ([ssl2buy](https://www.ssl2buy.com/digicert-ov-code-signing-certificate.php), [signmycode](https://signmycode.com/digicert-code-signing)) |
| Any EV | ~$250-700/yr | No SmartScreen benefit since Aug 2024; skip |

## 5. Reputation-building timeline

From Microsoft's own doc: no published threshold, no consumer submission process; reputation builds "organically through download volume" and can take **several weeks and hundreds of clean installs from a wide audience**. Signed apps: the warning phase happens once per certificate, then future releases inherit trust. Unsigned apps: the clock resets every single release. If prompts persist on a signed file, you can submit it to [Microsoft Security Intelligence](https://www.microsoft.com/wdsi/filesubmission) for review (the FAQ suggests this). Consistency matters: keep the same signing identity forever, and never sign anything remotely PUA-ish or the cert accrues *negative* reputation.

## 6. Sign the inner .exe AND the installer? Yes, both (and the uninstaller)

- **The installer** carries the Mark-of-the-Web and is what SmartScreen judges at download/run time. Must be signed.
- **The inner app .exe** must also be signed: Microsoft's guidance is "sign every release / all of your files"; Smart App Control checks **all** executables regardless of download origin; unsigned inner binaries also trip AV heuristics and look wrong in Task Manager/firewall prompts. ([SmartScreen doc](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/smartscreen-reputation))
- **The NSIS uninstaller** is a third binary, generated at build time. NSIS 3.08+ handles all of this cleanly: sign the app exe first, then use `!uninstfinalize 'signtool ... "%1"'` for the uninstaller and `!finalize 'signtool ... "%1"'` for the installer itself ([NSIS wiki: Signing an Uninstaller](https://nsis.sourceforge.io/Signing_an_Uninstaller), [external signing variant](https://nsis.sourceforge.io/Signing_an_Uninstaller_externally)). Order: sign inner exe -> makensis (uninstfinalize fires) -> sign outer installer.
- Practical note for your cross-build: Artifact Signing's supported signing paths run on Windows (signtool + dlib) or CI (GitHub Actions `azure/trusted-signing-action`). Since Ivory's Windows exe is cross-built on the Mac, the realistic flow is either a signing step on the Windows canary box (sign exe, rebuild/sign installer there) or a small GitHub Actions Windows job that signs the three binaries. `!finalize`/`!uninstfinalize` only work where signtool can run, so if makensis runs on macOS, plan to assemble/sign on Windows instead.

## 7. macOS: yes, settled

Developer ID signing + notarization + stapling is the complete, final story for direct distribution on macOS in 2026. Nothing new is required; Gatekeeper accepts notarized+stapled apps without warnings. You already do this for Ivory/Tangent. No action.

## 8. Recommendation

**Path: Azure Artifact Signing, Basic tier, as an individual developer** (assuming you're in the US; if not US/Canada, fall back to Certum cloud OV at ~$116/yr).

**Cost: $9.99/month (~$120/yr)** plus a $0 pay-as-you-go Azure subscription. Cheapest legitimate option, no USB token, no annual revalidation scramble (identity validation renews with reminders), Microsoft-run HSM, and the certificate identity is stable so reputation compounds.

**Setup steps (roughly one afternoon plus validation wait):**
1. Create a **paid** (pay-as-you-go) Azure subscription with an **Individual** billing account. Make the billing name/address exactly match your government ID first (fixable via `az billing account update`).
2. Register the `Microsoft.CodeSigning` resource provider on the subscription.
3. Create an **Artifact Signing account**, Basic SKU, in a supported region (e.g. East US).
4. Assign yourself the **Artifact Signing Identity Verifier** role, then create an **Individual / Public** identity validation. Complete the AU10TIX flow: email PIN, phone, photo of government ID, face check, Verified ID in Microsoft Authenticator.
5. Create a **Public Trust certificate profile** bound to that identity.
6. Assign the **Artifact Signing Certificate Profile Signer** role to whatever identity signs (you, or a service principal for CI).
7. Wire signing into the Windows-side build: sign `ivory.exe` (signtool + Artifact Signing dlib + `metadata.json`, or `azure/trusted-signing-action` in CI), build the NSIS installer with `!uninstfinalize` and `!finalize` invoking the same signing command, upload the signed installer to Gumroad.
8. Expect the "unrecognized app" prompt for the first weeks of sales; tell early buyers it's expected and shows your verified name. It fades and never returns for later releases.

**The do-nothing consequence, plainly:** every buyer who pays you $0-50 hits an Edge download warning followed by a full-screen "Windows protected your PC" block with "Unknown publisher," where the run-anyway path is deliberately hidden behind "More info." A meaningful fraction of non-technical buyers will conclude your paid software is malware, refund, or email support instead; buyers on Smart App Control machines or locked-down PCs may be unable to run it at all; and because unsigned reputation is per-file-hash, this repeats from zero on **every update you ship, forever**. For ~$10/month that entire failure mode goes away.

### Sources
- [SmartScreen reputation for Windows app developers - Microsoft Learn](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/smartscreen-reputation)
- [Quickstart: Set up Artifact Signing - Microsoft Learn](https://learn.microsoft.com/en-us/azure/artifact-signing/quickstart)
- [Artifact Signing FAQ - Microsoft Learn](https://learn.microsoft.com/en-us/azure/artifact-signing/faq)
- [Artifact Signing pricing - Azure](https://azure.microsoft.com/en-us/pricing/details/artifact-signing/)
- [Azure Artifact Signing product page](https://azure.microsoft.com/en-us/products/artifact-signing)
- [Microsoft Q&A: US LLC under 3 years eligible under GA (official MSFT answer)](https://learn.microsoft.com/en-us/answers/questions/5977141/azure-artifact-signing-trusted-signing-is-a-us-llc)
- [devclass: Artifact Signing GA (Jan 2026)](https://www.devclass.com/security/2026/01/14/code-signing-windows-apps-may-be-easier-and-more-secure-with-new-azure-artifact-service/4079554)
- [Trusted Signing opened to individual developers - Microsoft Community Hub](https://techcommunity.microsoft.com/blog/microsoft-security-blog/trusted-signing-is-now-open-for-individual-developers-to-sign-up-in-public-previ/4273554)
- [Melatonin: Code signing on Windows with Azure Artifact Signing](https://melatonin.dev/blog/code-signing-on-windows-with-azure-trusted-signing/)
- [Tim Anderson: Getting started with Azure Artifact Signing](https://www.itwriting.com/blog/12462-getting-started-with-azure-artifact-signing.html)
- [The Register: 7 steps to run an unsigned app](https://www.theregister.com/2020/06/05/windows_10_microsoft_defender_smartscreen/)
- [Advanced Installer: avoiding SmartScreen warnings (EV change)](https://www.advancedinstaller.com/prevent-smartscreen-from-appearing.html)
- [NSIS wiki: Signing an Uninstaller (!finalize / !uninstfinalize)](https://nsis.sourceforge.io/Signing_an_Uninstaller)
- [NSIS wiki: Signing an Uninstaller externally](https://nsis.sourceforge.io/Signing_an_Uninstaller_externally)
- [Certum shop: code signing (459-day max from Feb 2026)](https://shop.certum.eu/code-signing.html)
- [SSLmentor: Certum Cloud Code Signing pricing](https://www.sslmentor.com/certum/certumcodecloud)
- [SSL.com eSigner pricing](https://www.ssl.com/guide/esigner-pricing-for-code-signing/)
- [SSL Dragon: OV/EV code signing pricing overview](https://www.ssldragon.com/ssl-certificates/code-signing/)
- [ssl2buy: DigiCert OV pricing](https://www.ssl2buy.com/digicert-ov-code-signing-certificate.php)
- [signmycode: DigiCert pricing](https://signmycode.com/digicert-code-signing)
- [Ivan Singleton: cheapest code signing comparison 2026](https://ivansingleton.dev/the-cheapest-code-signing-certificate-for-business-central-appsource-in-2026-a-complete-comparison-guide/)
- [piers.rocks: Certum Open Source cert experience](https://piers.rocks/2025/10/30/certum-open-source-code-sign.html)
- [sslcertshop: Certum Open Source cert](https://sslcertshop.com/certum-open-source-code-signing)

---


# FreeBSD Portability Catalog

# FreeBSD-port worklist — platform-seam sweep of the ivory workspace

Repo root: `/Users/ganten/Library/CloudStorage/Dropbox/Projects/Apps/tangent` (all paths below are absolute). Assumed FreeBSD runtime: cpal's ALSA host via the alsa-lib-over-OSS shim, X11 (no Wayland), V4L2 via webcamd, ffmpeg from ports. Verified against the locked crate versions in `Cargo.lock` (cpal 0.16.0, midir 0.11.0, rfd 0.15.4, winit 0.30.13).

Two upstream facts frame everything:

- **cpal 0.16.0 explicitly supports FreeBSD** and selects the ALSA host there (`~/.cargo/registry/.../cpal-0.16.0/src/platform/mod.rs:595-597, 721-723` gate on `linux|dragonfly|freebsd|netbsd`). So every `#[cfg(target_os = "linux")]` in this repo that really means "the cpal ALSA backend" is a trap: FreeBSD gets ALSA's *semantics* but falls into the *else* arm written for macOS.
- **midir 0.11.0 has NO FreeBSD backend at all** — its backend table (`midir-0.11.0/src/backend/mod.rs`) covers windows/macos/ios/linux/wasm/android plus a `jack` feature; on FreeBSD without `--features jack` no backend module is declared and the crate **fails to compile**. This alone stops the whole `ivory` binary from building.

---

## A. Hard build breaks (the port does not compile until these are fixed)

### A1. midir has no FreeBSD backend — `ivory` crate fails to build
- `/Users/ganten/Library/CloudStorage/Dropbox/Projects/Apps/tangent/tangent/Cargo.toml:93` — `midir = "0.11"`, unconditional.
- On FreeBSD: no cfg arm in midir's backend table matches → missing types/modules → compile error in midir itself.
- Options: (a) build midir with its `jack` feature (needs libjack from ports; JACK is healthy on FreeBSD); (b) vendor-patch midir to widen its ALSA arm to FreeBSD — **but** note the linux backend uses the ALSA *sequencer* API, and FreeBSD's alsa-lib shim sits over OSS; whether `snd_seq_*` works through it needs a runtime probe before committing to that route.
- **Risk: BREAKS (build).**

### A2. `CONTENTS_SUBDIR` has no FreeBSD arm — `tangent-host` fails to build
- `/Users/ganten/Library/CloudStorage/Dropbox/Projects/Apps/tangent/tangent-host/src/scan.rs:48-57` — the VST3 bundle binary subdir const is defined only for macos / windows(x86_64,aarch64) / linux(x86_64,aarch64).
- On FreeBSD: no arm matches, the const does not exist, every use is E0425. This is the exact "4th OS" trap class named in the brief, in its hardest form.
- Port needs an arm (plugins built on FreeBSD conventionally use `x86_64-linux`-style naming or a `x86_64-freebsd` dir — decide with whatever FreeBSD VST3 builds actually ship, e.g. from audio/ ports).
- **Risk: BREAKS (build).**

### A3. `Library::open`'s init-symbol name is only defined for windows/linux
- `/Users/ganten/Library/CloudStorage/Dropbox/Projects/Apps/tangent/tangent-host/src/scan.rs:667-683` — inside the `#[cfg(not(target_os = "macos"))]` block, `let name: &[u8]` exists only under `#[cfg(target_os = "windows")]` (`InitDll`) and `#[cfg(target_os = "linux")]` (`ModuleEntry`); line 677 then uses `name`.
- On FreeBSD: `name` is undeclared → compile error. Fix: widen the linux arm (FreeBSD VST3s use `ModuleEntry`, same ELF convention).
- The rest of the loader is fine on FreeBSD: `load()` at scan.rs:732-771 is `#[cfg(not(windows))]` dlopen with `RTLD_NOW=2`/`RTLD_LOCAL=0` (correct values on FreeBSD), `find_symbol` at :831-839 is dlsym.
- **Risk: BREAKS (build).**

*(Minimal build (`--no-default-features`) sidesteps A2/A3 but not A1 — midir is unconditional.)*

---

## B. Known-trap class: else-arm written for another platform, FreeBSD runs ALSA/X11

### B1. `BUFFER_PERIODS` — the named trap, and it is real
- `/Users/ganten/Library/CloudStorage/Dropbox/Projects/Apps/tangent/tangent/src/instrument.rs:6044-6056` — linux=4, windows=2, **else=1**.
- FreeBSD falls into the else arm (1), but FreeBSD runs cpal's **ALSA host**, which reads `Fixed(v)` as the whole ring and carves it into 4 periods (the very behavior the linux=4 arm compensates for). Result: asking for 256 produces 64-frame callbacks at 4x the intended rate on a non-RT thread — the measured 2012-MBA config that produced 6 underruns/30s *with* the fix absent. Also `pick_buffer` (:6111-6116) will then report the ring as the period to the Audio Status panel.
- Fix: the linux arm must become "any OS where cpal picks the ALSA host" (linux|freebsd|dragonfly|netbsd).
- **Risk: wrong-behavior (severe: audio underruns/breakup, the app's core function).**
- Related and consistent: the input-side arm `/Users/ganten/Library/CloudStorage/Dropbox/Projects/Apps/tangent/tangent-record/src/audio.rs:2246-2268` (`not(macos)`) already gives FreeBSD the Linux/ALSA mapping — fine as-is once B1 is fixed.

### B2. `native_dialogs_work()` — the silent dead Save/Load/Export buttons come back
- `/Users/ganten/Library/CloudStorage/Dropbox/Projects/Apps/tangent/tangent/src/desktop.rs:2759-2825` (linux probe: portal bus / zenity), `:2827-2830` else-arm returns `true`.
- FreeBSD hits the else arm: the app assumes a native rfd dialog will appear. On a FreeBSD box without xdg-desktop-portal or zenity, `rfd::pick_file` returns `None` — indistinguishable from Cancel — so file buttons silently do nothing. This is exactly the diagnosed Void/XFCE bug (docs/LINUX-4.16-FINDINGS.md) reintroduced. The fallback probe even checks `/usr/local/share/xdg-desktop-portal/portals` (:2811), which is the FreeBSD prefix — the *probe* would work there; only the gate excludes it.
- **Risk: wrong-behavior (high: core UI actions silently no-op).**

### B3. `picker_needs_windowed()` — fullscreen file-picker freeze under tiling WMs
- `/Users/ganten/Library/CloudStorage/Dropbox/Projects/Apps/tangent/tangent/src/desktop.rs:2682-2690` — linux: leave fullscreen before a blocking picker; else-arm: `false`.
- FreeBSD X11 with i3/sway-class WMs has the identical `_NET_WM_STATE_FULLSCREEN` problem → modal opens under the fullscreen window, app appears frozen, force-quit territory.
- **Risk: wrong-behavior (app-freeze UX under tiling WMs).**

### B4. Compositor: owned-GL accelerator and all its guards are linux-only
- `/Users/ganten/Library/CloudStorage/Dropbox/Projects/Apps/tangent/tangent/src/composite.rs:50` (`mod glctx`), `:98-99` (gl field), `:109-113` (Drop guard), `:177-180` (`owned_gl_is_hardware` probe), `:315-319` (`on_owned_gl` first), `:380-443`, `:524`, `:597`, `:752` (make-current guards).
- On FreeBSD none of this exists; `standalone()` (:310-368) goes straight to wgpu, **refuses any GL adapter wgpu found itself** (:357-364, correctly — the eglMakeCurrent-unbind hazard is not linux-specific), so video compositing works **only with a Vulkan driver** (hardware or lavapipe). A GL-only FreeBSD box gets "no video" where an identical Linux box gets hardware GL. Also the error hint at `:337-343` is `cfg!(target_os = "linux")`-gated, so FreeBSD gets the error *without* the "install lavapipe" advice; `renders_on_the_cpu` (:172-205) skips the owned-GL probe and may wrongly lower defaults to 720p/15 on a machine with good GL but software Vulkan.
- Note the wgpu dependency itself is gated correctly: `/Users/ganten/Library/CloudStorage/Dropbox/Projects/Apps/tangent/tangent/Cargo.toml:150-152` uses `cfg(all(unix, not(target_os = "macos")))` — FreeBSD **is** included. The Cargo.toml got the seam right; composite.rs narrowed it to linux.
- **Risk: wrong-behavior (no video / degraded defaults on non-Vulkan machines; audio+MIDI still record).**

### B5. Camera: FreeBSD gets the stub, though webcamd speaks V4L2
- `/Users/ganten/Library/CloudStorage/Dropbox/Projects/Apps/tangent/tangent-record/src/camera.rs:113-116` — `not(macos|linux|windows)` → `mod stub` → every open returns `CameraError::Unsupported`.
- Linux backend: `/Users/ganten/Library/CloudStorage/Dropbox/Projects/Apps/tangent/tangent-record/src/camera/linux.rs` (enumerates `/dev/video{0..3}` at :156-167 — webcamd creates exactly these via cuse) over the **vendored** `linuxvideo` crate, gated at `/Users/ganten/Library/CloudStorage/Dropbox/Projects/Apps/tangent/tangent-record/Cargo.toml:172-187` (`target_os = "linux"`, plus `zune-jpeg` for MJPEG).
- Porting caveat: `linuxvideo` is pure-Rust ioctl wrappers encoding **Linux** `_IOC` numbers; FreeBSD's ioctl bit layout differs (3-bit dir/13-bit size vs Linux 2/14), and webcamd+v4l_compat expects FreeBSD-encoded numbers. The vendored crate likely needs its ioctl constants re-derived for FreeBSD — verify against `multimedia/v4l_compat` headers before assuming the linux backend "just works". The EACCES message at linux.rs:168-173 ("add your user to the 'video' group") also needs FreeBSD wording (`webcamd`/`operator` group / devfs rules).
- Also `camera/vaapi.rs` (dlopen'd libva, `/dev/dri/renderD128` default at :688) is linux-only; FreeBSD has libva in ports — optional follow-on, not required (it is strictly an accelerator).
- **Risk: wrong-behavior (whole camera feature absent; UI at least says so honestly).**

### B6. VST3 search paths: FreeBSD searches nothing
- `/Users/ganten/Library/CloudStorage/Dropbox/Projects/Apps/tangent/tangent-host/src/scan.rs:104-111` — the `~/.vst3`, `/usr/lib/vst3`, `/usr/local/lib/vst3` list is `target_os = "linux"`. On FreeBSD only `VST3_PATH` (:69-71) contributes → plugin picker is always empty. FreeBSD wants `~/.vst3` + `/usr/local/lib/vst3` (ports prefix).
- Compounding it, the empty-list help text `/Users/ganten/Library/CloudStorage/Dropbox/Projects/Apps/tangent/tangent-ui/src/dialogs.rs:1857-1871` else-arm tells the user "Linux keeps them in ~/.vst3 and /usr/lib/vst3" — folders the scanner never looked in on FreeBSD.
- **Risk: wrong-behavior (plugin hosting unusable without env var; help text lies).**

### B7. Plugin editor window: stub despite a working X11 backend
- `/Users/ganten/Library/CloudStorage/Dropbox/Projects/Apps/tangent/tangent-host/src/editor.rs:88-91` (`x11::Window` is linux; FreeBSD → `stub::Window`), `:98-102` (`EDITORS_SUPPORTED` = false on FreeBSD), `:325-328` (`pump` no-op), `:1161` (x11 module), `:1649` (stub module).
- The x11 backend is `x11rb` + `libc::poll` (`/Users/ganten/Library/CloudStorage/Dropbox/Projects/Apps/tangent/tangent-host/Cargo.toml:82-84`, also linux-gated) — both fully portable to FreeBSD; widening the gates should be enough.
- **Risk: wrong-behavior (feature degraded to "Unsupported" needlessly).**

### B8. PipeWire enumeration/binding — else-arm is acceptable, with a footnote
- `/Users/ganten/Library/CloudStorage/Dropbox/Projects/Apps/tangent/tangent-record/src/audio.rs:419-471` (`pw-dump` subprocess, linux) / `:474-477` (else: empty) and `:498-508` (`PIPEWIRE_NODE` env wrap) / `:510-513` (else: passthrough).
- FreeBSD else-arms fall back to plain cpal-ALSA enumeration — correct for the OSS-shim setup. Footnote: PipeWire does exist on FreeBSD; if a user runs it, they get the degraded single-`pipewire`-device behavior the linux path was built to fix.
- **Risk: fine (minor wrong-behavior only under PipeWire-on-FreeBSD).**

### B9. Misleading per-OS user text (else-arms name the wrong OS)
- `/Users/ganten/Library/CloudStorage/Dropbox/Projects/Apps/tangent/tangent-ui/src/dialogs.rs:1543-1561` — `nothing_found` else-arm is the **macOS** text: FreeBSD users are told to check "System Settings > Privacy & Security" for camera/mic access. (linux arm :1524-1541 is the right template — its `/dev/video*`/'video' group wording needs FreeBSD-ization per B5.)
- `/Users/ganten/Library/CloudStorage/Dropbox/Projects/Apps/tangent/tangent-record/src/encode/ffmpeg.rs:127-136` — `how_to_install` else-arm suggests `sudo apt install ffmpeg`; FreeBSD wants `pkg install ffmpeg`.
- `/Users/ganten/Library/CloudStorage/Dropbox/Projects/Apps/tangent/tangent/src/desktop.rs:1234-1244` — camera-silent hint: FreeBSD gets the generic arm; fine.
- **Risk: wrong-behavior (cosmetic/support-load).**

---

## C. External binaries spawned (and how each is located)

| Binary | Where spawned | Located how | FreeBSD status |
|---|---|---|---|
| `tangent-ffmpeg` / `ffmpeg` | `/Users/ganten/Library/CloudStorage/Dropbox/Projects/Apps/tangent/tangent-record/src/encode/ffmpeg.rs:107-120` (`program()`), used for encode (ffmpeg.rs:587-…), decode/import (`decode.rs:256`), mux | 1) `TANGENT_FFMPEG` env; 2) `tangent-ffmpeg` beside the exe (BUNDLED const :97-101, non-windows name); 3) bare `ffmpeg` on PATH | **fine** — port should depend on ports ffmpeg (PATH fallback) or install an exe-adjacent `tangent-ffmpeg`; fix the apt hint (B9). Release currently bundles it via `scripts/fetch-ffmpeg.sh` (build-cross.sh:305-319) — a native port skips that. |
| `/usr/bin/afconvert` | `/Users/ganten/Library/CloudStorage/Dropbox/Projects/Apps/tangent/tangent-record/src/decode.rs:341` | absolute path, **macOS-only cfg** | not compiled on FreeBSD — fine |
| `pw-dump` | `/Users/ganten/Library/CloudStorage/Dropbox/Projects/Apps/tangent/tangent-record/src/audio.rs:421` | PATH, linux-only cfg, absence = clean fallback | not compiled — fine (B8) |
| `dbus-send` | `/Users/ganten/Library/CloudStorage/Dropbox/Projects/Apps/tangent/tangent/src/desktop.rs:2783` | PATH, linux-only cfg | not compiled on FreeBSD — which is B2's bug: the probe that *should* run there doesn't |
| `open` / `explorer` / `xdg-open` | `/Users/ganten/Library/CloudStorage/Dropbox/Projects/Apps/tangent/tangent/src/desktop.rs:5323-5341` | PATH; FreeBSD correctly takes the `all(unix, not(macos))` xdg-open arm | fine (port should depend on xdg-utils) |
| `ffprobe` | encode.rs:402,549; ffmpeg.rs:720; composite.rs:1271 | PATH — **test code only** (inside `#[cfg(test)]`) | fine |
| resource compilers (`llvm-rc` etc.) | `/Users/ganten/Library/CloudStorage/Dropbox/Projects/Apps/tangent/tangent/build.rs:116-117,157` | hardcoded Homebrew paths + PATH — **only when `CARGO_CFG_TARGET_OS == "windows"`** (:33-35) | no-op for a native FreeBSD build — fine |

All runtime spawns go through `crate::proc::command` (`/Users/ganten/Library/CloudStorage/Dropbox/Projects/Apps/tangent/tangent-record/src/proc.rs:23-33`, `CREATE_NO_WINDOW` under `#[cfg(windows)]` only) — fine on FreeBSD.

## D. Paths & path assumptions

- Config: literal `~/.config/tangent/` on every platform, deliberately (settings.rs:1339-1369, overrides.rs:153-156, license.rs:324-327, main.rs:216 lock file, desktop.rs:2353). **fine** on FreeBSD.
- Take destination: `/Users/ganten/Library/CloudStorage/Dropbox/Projects/Apps/tangent/tangent-record/src/take.rs:453-484` — FreeBSD takes the `not(any(windows, macos))` arm: `$XDG_VIDEOS_DIR`/`~/.config/user-dirs.dirs`/`~/Videos`. **fine**.
- Disk-free: `/Users/ganten/Library/CloudStorage/Dropbox/Projects/Apps/tangent/tangent/src/record.rs:2227-2247` `cfg(unix)` statvfs (f_frsize handling correct for FreeBSD). **fine**.
- Clean exit: `/Users/ganten/Library/CloudStorage/Dropbox/Projects/Apps/tangent/tangent/src/main.rs:373-376` `cfg(unix)` `libc::_exit(0)`. **fine** (and wanted — same plugin-atexit hazard applies).
- App icon: main.rs:295-296 `not(macos)` full-bleed PNG. **fine**. Renderer: main.rs:328-329 `not(macos)` → Glow. **fine** on X11/mesa.
- Portal dirs fallback desktop.rs:2810-2811 includes `/usr/local/share/...` — already FreeBSD-friendly once B2's gate is widened.
- `/proc/self/stat` reads exist only in test/example code (composite.rs:1437 test, tangent-record examples) — dev-only; would return 0s on FreeBSD without procfs. **fine**.
- `cpu_seconds` instrument.rs:6262-6278 `cfg(unix)` getrusage. **fine**.
- keygen chmod 600 tools/tangent-keygen/src/main.rs:83 `cfg(unix)`. **fine** (not shipped).
- `flush_denormals` audio.rs:944 / instrument.rs:996 — `target_arch = "x86_64"`, OS-independent. **fine** (aarch64 FreeBSD gets the same no-op as aarch64 Linux).

## E. Platform-conditional dependencies (per Cargo.toml)

| Manifest | Gate | FreeBSD effect |
|---|---|---|
| `tangent/Cargo.toml:114` windows-sys | `cfg(windows)` | skipped — fine |
| `tangent/Cargo.toml:146-148` wgpu vulkan+gles | `cfg(target_os = "windows")` | skipped |
| `tangent/Cargo.toml:150-152` wgpu vulkan+gles + egui-wgpu | `cfg(all(unix, not(macos)))` | **included — correct** (the one gate written the right way) |
| `tangent/Cargo.toml:154-193` eframe/wgpu Metal | macos | skipped — fine |
| `tangent/Cargo.toml:195-198` libc (statvfs) | `cfg(unix)` | included — fine |
| `tangent/Cargo.toml:93` **midir** | unconditional | **A1 build break** |
| `tangent/Cargo.toml:102` mp3lame-encoder (builds LAME C) | via `mp3` default feature | should compile with clang on FreeBSD; **verify**, else ship without `mp3` (WAV fallback is honest) |
| `tangent-record/Cargo.toml:70-164` objc2 stack | macos | skipped — fine |
| `tangent-record/Cargo.toml:172-187` **linuxvideo (vendored) + zune-jpeg** | linux | skipped → camera stub (B5) |
| `tangent-host/Cargo.toml:40-60` objc2-app-kit | macos | skipped |
| `tangent-host/Cargo.toml:67-73` windows-sys | windows | skipped |
| `tangent-host/Cargo.toml:82-84` **x11rb + libc** | linux | skipped → editor stub (B7); widen to freebsd |
| eframe features `tangent/Cargo.toml:82-87` include `x11` (+`wayland`) | — | winit 0.30 supports FreeBSD X11 — fine |

---

## Summary — highest-risk items, ranked

| # | Item | Location | FreeBSD else-arm result | Risk |
|---|---|---|---|---|
| 1 | midir has no FreeBSD backend | tangent/Cargo.toml:93 (midir 0.11 backend table) | crate fails to compile — no binary at all | **breaks (build)** |
| 2 | `CONTENTS_SUBDIR` undefined | tangent-host/src/scan.rs:48-57 | E0425 — tangent-host fails to compile | **breaks (build)** |
| 3 | `let name` only for win/linux in `Library::open` | tangent-host/src/scan.rs:667-683 | undefined variable — fails to compile | **breaks (build)** |
| 4 | `BUFFER_PERIODS` else=1 on an ALSA host | tangent/src/instrument.rs:6044-6056 | 4x callback rate, measured-underrun territory | **wrong-behavior (severe)** |
| 5 | `native_dialogs_work()` else=true | tangent/src/desktop.rs:2827-2830 | Save/Load/Export silently no-op without portal/zenity | **wrong-behavior (severe)** |
| 6 | Camera → stub despite webcamd V4L2 | tangent-record/src/camera.rs:113-116 (+ vendored linuxvideo ioctl encoding) | no camera; port needs gate widening **and** ioctl-encoding verification | **wrong-behavior (feature gone)** |
| 7 | VST3 search paths empty + dialog names unsearched dirs | tangent-host/src/scan.rs:104-111; tangent-ui/src/dialogs.rs:1857-1871 | plugin hosting unusable without `VST3_PATH` | **wrong-behavior** |
| 8 | Compositor: no owned-GL path, GL adapter refused | tangent/src/composite.rs:50,315-364 | video requires Vulkan; wrong hint text; possibly wrongly lowered defaults | **wrong-behavior** |
| 9 | Plugin editor → stub though x11rb backend is portable | tangent-host/src/editor.rs:88-91,1649 | editors "Unsupported" needlessly | **wrong-behavior** |
| 10 | Fullscreen picker freeze under tiling WMs | tangent/src/desktop.rs:2682-2690 | apparent hang, force-quit | **wrong-behavior** |
| 11 | Misleading OS text (macOS privacy panel, `apt install`) | tangent-ui/src/dialogs.rs:1543-1561; tangent-record/src/encode/ffmpeg.rs:127-136 | wrong instructions shown | wrong-behavior (cosmetic) |

The repeating pattern for the fix pass: nearly every `target_os = "linux"` gate in this codebase actually means either "cpal's ALSA host" (items 4, B8), "X11" (9, 10), "XDG desktop" (5, 7, take-paths), or "V4L2" (6) — all of which FreeBSD shares — while the genuinely correct seams already use `cfg(unix)` / `cfg(all(unix, not(macos)))` (exit path, statvfs, xdg-open, the wgpu dependency). The port is mostly a systematic widening of `linux` → `any(linux, freebsd)` plus the three build breaks and the two vendored/external verifications (linuxvideo ioctls, midir backend).

---


# Release Machinery Findings

# Release machinery review (read-only) — ivory/Tangent, toward a paid 5.0

Repo root: `/Users/ganten/Library/CloudStorage/Dropbox/Projects/Apps/tangent` (all `file:line` refs below are relative to it). Current `Cargo.toml` version: **4.92.0**. `dist/` currently holds the 4.92 set: macOS universal zip+dmg, Windows setup.exe, Linux x86_64 tarball, `Tangent.app` — **no .pkg, no Windows zip, no SHA256SUMS**.

## Findings (most severe first)

### 1. CONFIRMED: versionless Windows zip permalink dies on the next stable publish
- **Where:** `scripts/publish-github.sh:17-18` (header still lists `tangent-windows-x86_64.zip` as part of the "release contract"); `scripts/build-cross.sh:435-446` (the zip is now explicitly "a TRANSIENT installer input" and is `rm -f`'d the moment the NSIS installer exists); `README.md:52` (live clickable link to `releases/latest/download/tangent-windows-x86_64.zip`); `docs/STORE-CONTENT.md:34` (contract table still lists the zip); `docs/RELEASE.md:238,297` (same).
- **What:** `releases/latest/download/<name>` resolves by exact asset name **against the latest release only**. The alias-upload code in publish-github.sh is generic (it derives aliases from whatever is in `dist/`), so with no zip built, no `tangent-windows-x86_64.zip` alias and no legacy `tangent-windows-x86_64.zip` alias gets uploaded. The moment the 5.0 release becomes latest, both URLs 404 — for README visitors and for **every pre-3.0 supporter-key email already sent** (those used the `ivory-*` zip name). The script's own permalink verification (`publish-github.sh:223-239`) only iterates over names it uploaded, so it structurally cannot notice a contract name that went missing.
- **Fix direction:** either (a) drop the zip from the contract — remove README.md:52's portable link, fix STORE-CONTENT.md:34 and publish-github.sh's header, and accept that ancient ivory-zip email links die; or (b) keep shipping the zip (stop deleting it in build-cross.sh, or alias the setup.exe under the zip name is NOT acceptable — wrong file type for the name); plus (c) give publish-github.sh a hard-coded required-alias manifest checked against UPLOADS before upload, so a missing contract name fails the publish instead of the buyer.

### 2. The Linux tarball buyers get has no install.sh and no bundled ffmpeg — and the script that injects them can never succeed
- **Where:** `scripts/build-linux-native.sh:39-52` (stages binary, fonts, licenses, README — **no `install.sh`, no `tangent-ffmpeg`**); `scripts/build-linux-remote.sh:160` ("Next: scripts/release.sh --sums-only, then scripts/publish-github.sh" — skips the installer step entirely); `scripts/build-installer.sh:258` vs `:307-323` (`build_linux` deletes the retired VST3 (`rm -rf "$dir/$NAME.vst3"`) and then its own verification **requires** `Tangent.vst3/Contents/x86_64-linux/Tangent.so` to be present exactly once — so `build-installer.sh linux` now always exits 1).
- **Proof:** the actual `dist/tangent-4.92.0-linux-x86_64.tar.gz` contains neither `install.sh` nor `tangent-ffmpeg` (verified by `tar -tzf`; 19 entries, 11 MB).
- **Why it matters for paid 5.0:** the supporter email a buyer receives says "the tarball has an install.sh that needs no root" (`tools/tangent-fulfil/src/main.rs:214`, `docs/STORE-CONTENT.md:162`), and the README.txt **inside the tarball** tells them to run `./install.sh` (`docs/ARTIFACT-README.md:37`). Video recording silently falls back to distro ffmpeg, i.e. breaks on clean machines — the exact 4.11 regression the cross-build path (`build-cross.sh` `package_linux`, lines 304-345) was hardened against; the native path (the documented "recommended Linux path") regressed it.
- **Fix direction:** stage `installer/linux/install.sh`, `tangent-ffmpeg` + `ffmpeg-licenses` in build-linux-native.sh (fetch-ffmpeg.sh is portable and checksum-pinned but downloads a Linux binary — simplest is to inject on the Mac after rsync-back, i.e. fix and re-enable `build-installer.sh linux` by deleting its three VST3 assertions at 307-323); and add `install.sh` + `tangent-ffmpeg` to release.sh's Linux artifact checks (see finding 6).

### 3. The primary macOS buyer link (`Tangent-macos.pkg`) is built by nothing in the release path
- **Where:** `scripts/release.sh:85-105` (Darwin branch runs `build-macos.sh` + `build-cross.sh` only; `build-installer.sh macos` is never invoked by any script); `docs/RELEASE.md` release procedure steps 1-10 never mention `build-installer.sh` at all; `docs/STORE-CONTENT.md:157`, `tools/tangent-fulfil/src/main.rs:183`, `README.md:51` all hand out `Tangent-macos.pkg` as the **first** macOS link.
- **What:** the pkg exists only if the owner remembers a by-hand step that no checklist names. release.sh's pkg inspection (`release.sh:126-143`) runs only *if* a pkg is already in `dist/`; publish-github.sh publishes happily without one (ARTIFACTS is non-empty from the zip/dmg). `dist/` today has no 4.92 pkg. The only net that catches it is `check-store-links.sh` **after** publishing — a 404 window on the top buyer link.
- **Fix direction:** have release.sh's Darwin branch run `scripts/build-installer.sh macos` after build-macos.sh, and add the pkg to a required-artifact list; document the step in RELEASE.md. Note the ordering constraint: the pkg must exist before `release.sh --sums-only`, or publish-github.sh's sums-coverage check (`publish-github.sh:112-115`) correctly refuses it.

### 4. The file a Windows buyer actually runs (setup.exe) is never Authenticode-signed
- **Where:** `scripts/build-cross.sh:412` (`sign_windows_exe` is called on the inner `tangent.exe` only, and only when `TRUSTED_SIGNING_*` is configured — unset today, so even that ships unsigned with just a console note, `build-cross.sh:164-166`); `scripts/build-installer.sh:196-228` (`build_windows` has no signing step at all — contrast with the macOS branch's productsign/notarize path).
- **Why it matters for paid 5.0:** SmartScreen reputation attaches to the downloaded executable — the installer. A paying customer's first contact with the product is an "unknown publisher" wall, on the artifact that RELEASE.md itself says must carry the SmartScreen note. Signing the inner exe but not the setup.exe buys almost nothing.
- **Fix direction:** call `sign_windows_exe` on `$out` at the end of `build_windows()` (jsign signs any PE, the machinery already exists in build-cross.sh — hoist it into a shared helper), and for a paid release consider making unsigned-Windows a refuse-to-package gate like the macOS notarization gate at `build-macos.sh:359-378`.

### 5. Buyer-facing copy still sells the retired VST3, including inside the installer UI
- **Where:** `installer/windows/tangent.nsi:74` — the components page a buyer sees says "Tangent comes in two pieces. Install either, or both." above a list with exactly **one** component; `tangent.nsi:1-19` header comments describe two components. `tools/tangent-fulfil/src/main.rs:209` — the supporter email body: "VST3 plugin as separate choices, so you can take either or both". `docs/STORE-CONTENT.md:123,153` — same claim in the store page copy. (`main.rs:177` comment too.)
- **Why:** a paid product whose install screen and receipt email describe a component that does not exist reads as broken or bait; the plugin was retired 2026-08-24 by owner ruling (`build-installer.sh:2-4`).
- **Fix direction:** reword tangent.nsi's components text (or drop the components page for a single-section install), and update the email body + STORE-CONTENT block before the fulfil service is redeployed for 5.0. There is a test asserting on `email_body`, so the fixture updates with it.

### 6. release.sh's Linux artifact check cannot catch finding 2
- **Where:** `scripts/release.sh:178-180` — the Linux tarball check greps only for `/tangent$` (plus the shared README/LICENSE/OFL checks). `install.sh` and `tangent-ffmpeg` are not on the list, though the cross path treats their absence as a build failure.
- **Fix direction:** add `(^|/)install\.sh$` and `(^|/)tangent-ffmpeg$` (and arguably `ffmpeg-licenses/`) to the `*linux*` case. This one line would have blocked the current 4.92 tarball.

### 7. Checksums do not cover the filenames buyers actually download
- **Where:** `scripts/release.sh:184-187` writes `SHA256SUMS` over version-scoped names only; `scripts/publish-github.sh:141-144` uploads that same file bare. `docs/STORE-CONTENT.md:169` hands buyers the SHA256SUMS link.
- **What:** a buyer who downloads `Tangent-macos.pkg` (the alias every buyer link uses) finds no line for that filename in SHA256SUMS — `shasum -c` fails; they must know to match hashes against the version-scoped spelling. For a paid product advertising checksums, verification should work as-is.
- **Also:** `dist/` currently has no SHA256SUMS for 4.92 at all, and nothing ever writes the `SHA256SUMS-<version>` spelling that publish-github.sh:109 prefers (harmless — the fallback works — but the preferred path is dead code).
- **Fix direction:** when writing SHA256SUMS, append alias-named duplicate lines (same hash, alias filename), or generate the alias lines in publish-github.sh where the alias names are already computed.

### 8. Settings migrations: upgrade path is solid; downgrade-then-upgrade is destructive
- **Where:** `tangent-ui/src/settings.rs:38` (`SETTINGS_VERSION = 19`), `:2986-2989` (every save unconditionally re-stamps with the **running build's** version), `:2346` (v15 migration flips `lpf_mix = 1.0 - lpf_mix`, non-idempotent), `:2359-2367` (v18 pad-seat move: re-running takes now-empty seats and overwrites seats 48..63, wiping user-placed pad inserts).
- **What's good for 5.0:** fresh install with no file skips migration cleanly (`load()`, :1391-1398); unreadable files are renamed aside, not overwritten (:1418-1424); unknown keys survive round-trips via `extra`; the Windows uninstaller deliberately preserves `%USERPROFILE%\.config\ivory` (`tangent.nsi:147-149`) — taught chords and the supporter key survive uninstall/reinstall. Upgrade 4.x → 5.0 is well covered.
- **The risk:** a buyer who keeps an old build around (old installer on disk, second machine, portable copy) launches old-then-new: the old build re-stamps the file to its lower version, and the new build then **re-runs** migrations 15..19 against an already-migrated file — the low-pass dial audibly inverts and pad inserts get cleared. Plausible in a paid population that hoards installers.
- **Fix direction:** never lower the stamp — in `from_map` keep `max(file_stamp, SETTINGS_VERSION)` around (e.g. carry a higher-than-known stamp through `extra` or a dedicated field) so a newer file passing through an older build keeps its version. That's a change for 5.0's settings.rs, cheap now, impossible to retrofit into builds already shipped.
- **Process note:** if any default changes for 5.0, SETTINGS_VERSION must bump to 20 — the freeze rules in the file are good, just don't ship a default change without the bump.

### 9. NSIS installer upgrade/uninstall rough edges
- **Where:** `installer/windows/tangent.nsi:132-137` — `.onInit` checks only for 64-bit; no check that Tangent is running. Installing 5.0 over a running 4.92 hits file-in-use retry/abort dialogs mid-copy. `tangent.nsi:144` — `RMDir /r "$INSTDIR"` on uninstall: classic NSIS hazard if a user pointed the directory page at a non-empty folder (e.g. `C:\Program Files` itself). `tangent.nsi:140` — uninstall also deletes `$COMMONFILES64\VST3\Tangent.vst3`, a file this installer never wrote (only defensible as cleanup of pre-5.0 installs; fine, but it is undocumented behavior).
- **Fix direction:** add a running-process check (FindWindow / `tasklist` loop) in `.onInit`; delete known files/subdirs explicitly instead of `RMDir /r $INSTDIR`, or at least verify `$INSTDIR` looks like a Tangent install (tangent.exe present) before recursing.

### 10. docs/RELEASE.md misdescribes the current artifact set
- **Where:** `docs/RELEASE.md:230-239` — artifact table still says `Ivory-<v>-*` names, lists the Windows zip as a deliverable, and has no row for the .pkg or setup.exe; `:269-271` step 5 says build-cross.sh "emits the Windows zip" (it deletes it); `:283` step 6 says `open dist/Ivory.app`; `:295-298` step 9's alias examples name the zip. The procedure never mentions `build-installer.sh` (either platform).
- **Why:** this is the checklist the 5.0 release will be driven from; every stale line is a chance to reproduce findings 1-3 by following instructions.
- **Fix direction:** rewrite the table to the 5.0 deliverable set (pkg, setup.exe, tarball, macOS zip/dmg, SHA256SUMS) and add build-installer steps with the sums-ordering constraint from finding 3.

### 11. Minor
- `scripts/check-store-links.sh:45` uses `curl -IL` (HEAD) — the exact method `publish-github.sh:229-231` documents as intermittently returning false 503s from GitHub's asset CDN. The one gate that stands between buyers and dead links is the flaky-by-design variant. Fix: same ranged-GET (`-r 0-0`) trick.
- `scripts/publish-github.sh:153-163` uploads legacy `Ivory-macos.pkg` / `Ivory-windows-setup.exe` aliases that no email or page has ever linked (installers postdate the rename). Harmless bloat; could be scoped to the four names that actually circulated.
- `Cargo.toml:12` is 4.92.0 — 5.0.0 needs the bump plus a `cargo check` so Cargo.lock follows (release.sh:54-56 gates on this, good). The NSI `NUMVERSION` mechanism already handles a `5.0.0-beta.N` VIProductVersion correctly, and publish-github.sh correctly quarantines pre-releases away from the permalinks.
- `.cargo/config.toml` (static CRT for Windows): correct and valuable for a paid release — no findings.
- Verified non-issues: NSI `File /r "*.*"` does pick up extensionless `LICENSE`/`THIRD-PARTY-LICENSES` (confirmed in the 4.92 setup.exe via 7zz); 7zz does list `Uninstall.exe`, so release.sh's uninstaller check works; `install-macos.sh` is sound; `fetch-ffmpeg.sh` is checksum-pinned and cached (good supply-chain hygiene).

## What Gumroad delivery needs vs what the scripts produce

| Gumroad delivery needs | What the machinery produces today | Gap |
|---|---|---|
| `Tangent-macos.pkg` at the permalink, signed (Developer ID **Installer**) + notarized | Built only by hand-running `build-installer.sh macos`; unsigned fallback is a warning, not a gate; no script in the release path invokes it | Wire into release.sh; make unsigned-pkg a refusal for stable versions |
| `Tangent-windows-setup.exe`, Authenticode-signed | Built automatically by build-cross.sh (good); setup.exe never signed, inner exe unsigned unless `TRUSTED_SIGNING_*` set | Sign the NSIS output; decide the signing account before launch, not after SmartScreen complaints |
| Every permalink in the supporter email, store page and README resolving after publish | Aliases derived from whatever is in `dist/`; verification checks only what was uploaded | `tangent-windows-x86_64.zip` (and legacy `ivory-*.zip`) will 404; add a required-alias manifest to publish-github.sh, or retire the zip links first |
| Linux tarball containing `install.sh` + `tangent-ffmpeg` as the email promises | Native/remote path (the recommended one) ships neither; the injection script (`build-installer.sh linux`) always fails on retired-VST3 assertions | Fix staging + assertions; extend release.sh's tarball checks |
| `SHA256SUMS` a buyer can actually run `shasum -c` against | Version-scoped filenames only; alias-named downloads unverifiable; no sums generated for the current dist at all | Emit alias-named lines; run `release.sh --sums-only` after ALL artifacts (incl. pkg) exist |
| Truthful purchase-moment copy (installer UI, receipt email, store page) | Still advertises the retired VST3 "either or both" on all three surfaces | Reword before redeploying tangent-fulfil for 5.0 |
| Safe upgrade for existing users; safe uninstall | Settings migration v0..19 solid; uninstaller preserves settings; Windows upgrade over a running app fails ugly; downgrade-then-upgrade corrupts lpf/pad settings | Running-app check in NSI; version-stamp floor in settings.rs before 5.0 ships |
| A checklist that produces all of the above when followed | docs/RELEASE.md predates the rename, the installers and the zip retirement | Rewrite the artifact table + procedure for the 5.0 set |

Key files: `/Users/ganten/Library/CloudStorage/Dropbox/Projects/Apps/tangent/scripts/publish-github.sh`, `/Users/ganten/Library/CloudStorage/Dropbox/Projects/Apps/tangent/scripts/build-installer.sh`, `/Users/ganten/Library/CloudStorage/Dropbox/Projects/Apps/tangent/scripts/build-linux-native.sh`, `/Users/ganten/Library/CloudStorage/Dropbox/Projects/Apps/tangent/scripts/release.sh`, `/Users/ganten/Library/CloudStorage/Dropbox/Projects/Apps/tangent/installer/windows/tangent.nsi`, `/Users/ganten/Library/CloudStorage/Dropbox/Projects/Apps/tangent/tangent-ui/src/settings.rs`, `/Users/ganten/Library/CloudStorage/Dropbox/Projects/Apps/tangent/tools/tangent-fulfil/src/main.rs`, `/Users/ganten/Library/CloudStorage/Dropbox/Projects/Apps/tangent/docs/RELEASE.md`, `/Users/ganten/Library/CloudStorage/Dropbox/Projects/Apps/tangent/docs/STORE-CONTENT.md`.

---
