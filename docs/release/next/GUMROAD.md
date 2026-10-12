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

- The 4.4 page said the key gates nothing. From 5.0 the free app holds 32
  tracks (MIDI and audio tracks and folders count) and a key takes that limit
  away. Keys already sold do the same, with nothing to do.
- Buses, returns, sends and routes have no limit for anyone and are never
  counted.
- A project past 32 tracks still opens, plays, edits, exports and saves whole
  on a free install; only adding the next track asks for a key.
- The DAW paragraph names what 5.0 added.
- Linux is glibc 2.30+ (it said 2.32), and FreeBSD is listed.
- The app also mentions "a certain code" beside the key. This page does not
  (owner call 9 in `docs/design/unlimited-tracks.md`).

Every surface says the same facts: this page, the post-purchase page and the
key email (`docs/STORE-CONTENT.md`, `tools/tangent-fulfil/src/main.rs`), the
README, and the app's own Supporter Key dialog.

---

## Title

**Tangent supporter key**

## Subtitle / tagline

Play. Tangent writes it down, explains it, and hands you the video. Free with
32 tracks; a supporter key takes the limit away.

## Price

$12, "pay what you want", minimum $12.

---

## Description

> Tangent is free, with up to 32 tracks. A supporter key takes the track limit
> away, and it is how Tangent keeps being made.

### What Tangent is for

Plug in a MIDI keyboard and play. Tangent names the chord, engraves it on a
staff, shows you why it works on three theory diagrams and where it sits on a
guitar neck, all in real time. Then it records the whole thing: one press
writes a `.wav`, a `.mid` and an `.mp4` of the window itself, with your camera
already placed in it.

Around that is a recording DAW: a mixer, an arrangement view, a MIDI editor,
a drum machine, automation that reaches plugins, folders that are real buses
(each counts as a track), and VST3 plugin hosting on every platform (LV2 too
on Linux and FreeBSD).

### New in 5.0

- **Buses and returns**, as many as you want, with sends at their own level,
  pre or post fader.
- **Tangent Insert**: your outboard gear in an insert bay, with a PING that
  measures the loop and keeps the session in time with it.
- **Layered instruments**: several instruments on one track, all playing.
- **Plugins follow the song**: tempo, meter, position and transport reach
  every VST3 and LV2 plugin, so drum machines and synced delays lock in.
- **Export what you want**: pick tracks or stems, and the whole song, the
  loop or a custom range. Stems are named after their tracks.
- **Send to Device**: the master on a virtual cable too, for OBS or Discord.
- **Tangent Bitcrush**, audio quantize, track freezing, rendering on every
  core, and thirty-two themes.

### What is free, and what the key does

**Free, for everyone:**

- up to 32 tracks, folders included;
- every feature: recording, video, the theory panels, the sheet music, the
  guitar neck, plugins, hardware inserts, automation and export;
- buses, returns, sends and routes, with no limit;
- no trial, no time limit, no watermark, no account and no nag.

**With a supporter key:**

- **no track limit.** Add as many tracks as your machine can carry;
- the supporter heart in the recorder band, in your theme's palette and
  moving through it;
- the mail that goes out when a release lands, written by the person who made
  it.

**If you already bought a key**, it already does all of this. Keys have no
expiry and cover every version, past and future.

**A project never gets smaller.** Open a 40-track project on a free install
and it opens, plays, edits and saves whole. Only adding a new track while the
project already has 32 or more asks for a key.

### And it stays out of the way

**It is offline.** No account, no telemetry, no phoning home. A key is checked
on your own machine with a signature and is never sent anywhere.

macOS 11+ (Apple Silicon and Intel, signed and notarized), Windows 10+ (every
program signed), Linux x86_64 and aarch64 (glibc 2.30+, ALSA, Wayland or X11),
and FreeBSD 15.

**Download:** <https://github.com/ganten1998/tangent/releases/latest>

---

## Delivery / receipt content

The post-purchase page is `docs/STORE-CONTENT.md`'s "Post-purchase page"
block, and the key itself arrives in the email `email_body()` writes. Both
say what this page says; paste that block, not a copy of it here. Its three
stale lines (the "5.0.0 itself is not" signing sentence, "glibc 2.32", and the
FFmpeg source sentence) are fixed; the FFmpeg source lines still must not be
pasted until `scripts/publish-third-party-src.sh` has created both source
releases and `scripts/check-store-links.sh` finds them.

## Tags

`music education` `piano teaching` `midi` `music theory` `chord detection`
`sheet music` `piano` `screen recording` `vst3` `daw` `content creation`
`freebsd`
