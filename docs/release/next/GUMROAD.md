# Gumroad product page for the unlimited desk

Paste-ready copy for <https://ganten.gumroad.com/l/ivory>, for the first
release with unlimited tracks. The folder is `next` because the version is
`release.sh`'s to write; rename it to that release's number when it is cut.
The 4.2 and 4.4 pages beside it stay as the history of what each release's
page said.

NO EM DASHES anywhere in the pasted text. They read as machine-written and
have drawn a real accusation before.

## What changed, and why this page had to

The 4.4 page said the key gates nothing: "No features. That is not modesty, it
is the specification." That stopped being true the day the free desk got a
track limit. From this release:

- **The free app holds 32 tracks.** That counts MIDI and audio tracks and
  folders. It is the only thing a key changes.
- **A supporter key takes the limit away.** Memory is the bound.
- **Buses, returns, sends and routes have no limit for anyone.** They are not
  tracks and they are never counted.
- **Keys already sold do the same.** Every key ever issued is perpetual and
  covers every version, so every existing supporter gets unlimited tracks with
  nothing to do.
- **The limit never takes anything away.** A project with more than 32 tracks
  opens, plays, edits, exports and saves whole on a free install. Only adding
  the next track asks for a key.

A page that kept the 4.4 wording would tell a buyer the key does nothing while
the app tells them it lifts a limit. Every surface now says the same four
facts: this page, the post-purchase page and the key email
(`docs/STORE-CONTENT.md`, `tools/tangent-fulfil/src/main.rs`), the README, and
the app's own Supporter Key dialog.

The app also mentions "a certain code" beside the key. This page does not
(owner call 9 in `docs/design/unlimited-tracks.md`).

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
a drum machine, automation, folders that are real buses, and VST3 plugin
hosting on every platform (LV2 too on Linux and FreeBSD).

### What is free, and what the key does

**Free, for everyone:**

- up to 32 tracks;
- every feature: recording, video, the theory panels, the sheet music, the
  guitar neck, plugins, automation and export;
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

macOS 11+ (Apple Silicon and Intel, signed and notarized), Windows 10+, Linux
(glibc 2.32+, ALSA, Wayland or X11).

**Download:** <https://github.com/ganten1998/tangent/releases/latest>

---

## Delivery / receipt content

The post-purchase page is `docs/STORE-CONTENT.md`'s "Post-purchase page"
block, and the key itself arrives in the email `email_body()` writes. Both
say what this page says; paste that block, not a copy of it here.

## Tags

`music education` `piano teaching` `midi` `music theory` `chord detection`
`sheet music` `piano` `screen recording` `vst3` `daw` `content creation`
