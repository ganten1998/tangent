# Gumroad product page for Tangent 5.0.0

Paste-ready copy for <https://ganten.gumroad.com/l/ivory>, for 5.0.0, the
first stable release with unlimited tracks. The folder is `next` because the
version is `release.sh`'s to write; rename it to `5.0.0` when the release is
cut (`copy_scan.rs` follows the rename and scans whichever folder is newest).
The 4.2 and 4.4 pages beside it stay as the history of what each release's
page said.

NO EM DASHES anywhere in the pasted text. They read as machine-written and
have drawn a real accusation before. `copy_scan.rs` scans this file in the
release gate: it refuses an em dash, the old "a key unlocks nothing" wording,
the desk described by a width, and "folders are buses" without saying they
count.

**Paste order (docs/RELEASE.md): publish, check, paste, deploy.** Paste this
only after `scripts/publish-github.sh` has published 5.0.0 and
`scripts/check-store-links.sh` is green from a machine that is NOT signed in
to GitHub (the links must resolve for a stranger).

## What changed on this page since 4.4

- **Written from scratch for 5.0** (owner, 2026-10-12: "it's a DAW, all
  content must be written from scratch for this release"). The page used to
  sell a keyboard monitor that also records. It now sells a DAW, and the
  chord naming, theory views, guitar neck and take video are features of it.
- The 4.4 page said the key gates nothing. From 5.0 the free app holds 32
  tracks (MIDI and audio tracks and folders count) and a key takes that limit
  away. Keys already sold do the same, with nothing to do.
- Buses, returns, sends and routes have no limit for anyone and are never
  counted.
- A project past 32 tracks still opens, plays, edits, exports and saves whole
  on a free install; only adding the next track asks for a key.
- No third-party product is named (no plugin, cable or streaming app), and
  Tangent is not compared with any other program.
- Linux is glibc 2.30+, and FreeBSD is listed.
- The app also mentions "a certain code" beside the key. This page does not
  (owner call 9 in `docs/design/unlimited-tracks.md`).

Every surface says the same facts: this page, the post-purchase page and the
key email (`docs/STORE-CONTENT.md`, `tools/tangent-fulfil/src/main.rs`), the
README, and the app's own Supporter Key dialog.

---

## Title

**Tangent supporter key**

## Subtitle / tagline

Record, arrange and mix on macOS, Windows, Linux and FreeBSD.

Free with up to 32 tracks; a supporter key removes the limit.

## Price

$12, "pay what you want", minimum $12.

---

## Description

> Tangent is a DAW, and it is free to use with up to 32 tracks. A supporter
> key removes the track limit. It is also how Tangent keeps being made.

### What Tangent is

Tangent is a digital audio workstation for macOS, Windows, Linux and
FreeBSD. You record audio and MIDI into it, arrange clips on a timeline,
edit notes in a piano roll, and mix on a desk with buses, returns, sends,
folders and automation. It hosts your VST3 plugins on every platform, and
LV2 plugins on Linux and FreeBSD.

It comes with its own instruments and effects, so a fresh install makes
music before you add anything to it.

### What you can do with it

- **Record.** Audio and MIDI, on every track you arm at once, with a
  metronome, a count-in and software monitoring. A take can also film the
  window itself, with your camera placed in it, and save the video beside
  the audio and the MIDI.
- **Arrange.** Clips loop, stretch from their edges and glue together.
  Quantize works on audio takes as well as MIDI. The tempo lane takes ramps
  and steps.
- **Edit MIDI.** A piano roll with step entry, a note menu and humanize, and
  a pad-based drum machine whose kits save as one file, samples included.
- **Mix.** Faders, pan, inserts, sends before or after the fader, sidechain,
  delay compensation, and automation in four modes that reaches plugin
  parameters too. Freeze a heavy track to give its CPU back.
- **Export.** A mixdown or stems, as WAV or MP3, for the whole song, the loop
  or a range you type in, with only the tracks you pick.
- **See what you play.** Tangent names the chord under your hands as you
  play it, writes it on a staff, places it on a guitar neck, and draws it on
  the circle of fifths. If it names a chord differently than you would, you
  can teach it your name.

### What comes in the box

- **Instruments:** a modelled keyboard with eight sounds (grand pianos,
  tine and reed electric pianos, clavs), an FM synth that reads DX7 patch
  banks, an analog-style synth with an arpeggiator, a sampler, and a
  sixteen-pad drum machine.
- **Effects:** EQ, compressor, tape, stereo imager, plate reverb, echo,
  chorus, tremolo, wah, overdrive, a tube amp with a pedalboard, a
  convolution unit (with a wizard that captures impulse responses from your
  own gear), a noise reducer that learns your noise floor, and a bitcrusher.
- **Tangent Insert,** which puts outboard gear in an insert slot: it sends
  out of your interface, takes the signal back in, and measures the round
  trip so the session stays in time with it.
- **Thirty-two themes,** each with its own palette of track colours.

Several instruments can share one track and all play from the same MIDI.
Plugins are told the song's tempo, meter, position and transport, so synced
delays, arpeggiators and drum machines lock to the song.

### What is free, and what the key does

**Free, for everyone:**

- up to 32 tracks (MIDI tracks, audio tracks and folders count);
- every feature on this page;
- buses, returns, sends and routes, with no limit and never counted;
- no trial, no time limit, no watermark, no account, no nag screen.

**With a supporter key:**

- **no track limit.** Add as many tracks as your machine can carry;
- the supporter heart in the recorder band, drawn in your theme's colours;
- an email from me when a new release is out.

**If you already bought a key,** it already does all of this. Keys have no
expiry and cover every version.

**A project never gets smaller.** A 40-track project opens, plays, edits,
exports and saves whole on a free install. Only adding a new track while the
project already has 32 or more asks for a key.

### Offline, and yours

Tangent needs no account and sends nothing about you anywhere: no
telemetry, no update check. Your key is checked on your own machine with a
signature, and never leaves it.

It runs on macOS 11 or later (Apple Silicon and Intel, signed and notarized),
Windows 10 or later (signed, with ASIO), Linux x86_64 and aarch64 (glibc
2.30+, ALSA or JACK, Wayland or X11), and FreeBSD 15.

**Download:** <https://github.com/ganten1998/tangent/releases/latest>

---

## Delivery / receipt content

Nothing to paste here. The post-purchase page is the "Post-purchase page"
block in `docs/STORE-CONTENT.md`, and the key arrives in the email that
`email_body()` in `tools/tangent-fulfil/src/main.rs` writes. Both were
rewritten with this page and say the same things. The FFmpeg source lines in
the post-purchase block must not be pasted until
`scripts/publish-third-party-src.sh` has created both source releases and
`scripts/check-store-links.sh` finds them.

## Tags

`daw` `music production` `recording` `mixing` `midi` `vst3` `lv2`
`audio plugins` `linux audio` `freebsd` `music theory` `free software`
