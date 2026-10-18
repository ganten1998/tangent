# Tangent 5.0.0 store kit
Everything to paste, in one place. Sources: docs/release/5.0.0/GUMROAD.md (product page), docs/STORE-CONTENT.md (links + post-purchase page), WEBSITE-CARD.md, ANNOUNCE.md. Edit those, not this file; regenerate this from them.
**Order:** publish 5.0.0 on GitHub (repo public) -> `scripts/check-store-links.sh` green while signed out -> paste the product page and post-purchase page -> re-enable the product -> `fly deploy` tangent-fulfil -> website card -> announcement -> set the GitHub description (CHECKLIST.md G5).

---

# 1. Download links (all resolve against the latest GitHub release)

Every download link points at
`https://github.com/ganten1998/tangent/releases/latest/download/<name>`, which
GitHub resolves by **exact asset name** against whatever release is currently
latest. Nothing here is version-pinned, so a page written today still hands out
2.9.0 in two years without being touched.

That guarantee rests entirely on `scripts/publish-github.sh` uploading the
version-less alias assets on every release. If a future release ships only
`Tangent-2.3.0-macos-arm64.dmg`, all of these 404 at once, for buyers as well as
for the README, and nothing anywhere reports it. See `docs/RELEASE.md` step 9.

| platform | asset name |
|---|---|
| macOS installer | `Tangent-macos.pkg` |
| macOS archives | `Tangent-macos-universal.dmg`, `Tangent-macos-universal.zip` |
| Windows installer | `Tangent-windows-setup.exe` |
| Windows archive | `tangent-windows-x86_64.zip` |
| Linux | `tangent-linux-x86_64.tar.gz` |
| checksums | `SHA256SUMS` |

`Tangent-macos-arm64.{dmg,zip}` are still uploaded as aliases of the universal
build, because they are the names in every email sent before 3.0.0. They are
not what a new page should hand out.

---

# 2. Gumroad product page

## Title

**Tangent supporter key**

## Subtitle / tagline

Record, arrange and mix on macOS, Windows, Linux and FreeBSD.

Free with up to 32 tracks; a supporter key removes the limit.

## Price

"Pay what you want", minimum $9, suggested price $15. In Gumroad: set the price to $9, tick "Let customers pay what they want", and set the suggested amount to $15.

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

---

# 3. Gumroad post-purchase page (Content tab)

## Post-purchase page

Gumroad → Products → Tangent → **Content** tab → paste, then **Save changes**.

Gumroad's editor turns a bare URL into a link on paste, so this can go in as
plain text. Headings are optional; set them with the editor's own controls
rather than typing `#`, which it renders literally.

Preview it with **Preview** on the product page, not by buying: buying fires the
Ping and mints a real key.

---

Thank you for supporting Tangent.

Tangent is free, with up to 32 tracks. Your supporter key takes the track
limit away, on every machine you own. Buses, returns, sends and routes have no
limit for anyone, and every other feature is in the free app as well.

DOWNLOAD TANGENT

These links always fetch the current version, so keep them with your key.

macOS 11 or later, Apple Silicon or Intel
https://github.com/ganten1998/tangent/releases/latest/download/Tangent-macos.pkg

Windows 10 or later
https://github.com/ganten1998/tangent/releases/latest/download/Tangent-windows-setup.exe

Linux x86_64 (the tarball has an install.sh that needs no root)
https://github.com/ganten1998/tangent/releases/latest/download/tangent-linux-x86_64.tar.gz

The release page has everything else: the .dmg and .zip for macOS, a
portable .zip for Windows, the Linux aarch64 tarball and the FreeBSD 15
package:
https://github.com/ganten1998/tangent/releases/latest

Checksums:
https://github.com/ganten1998/tangent/releases/latest/download/SHA256SUMS

YOUR SUPPORTER KEY

Your key comes by email within about a minute, from keys@tangentdaw.org. If
you cannot see it, look in your spam folder for that address. If it is not
there either, reply to your Gumroad receipt and I will send it myself.

To use it, open Tangent, right-click anywhere, choose "Support Tangent...",
paste the key and press Activate. Case, spaces, dashes and line breaks do not
matter.

The key never expires, never contacts a server, and works on every machine
you own.

FIRST LAUNCH

macOS: open the .pkg and follow it. It is signed with a Developer ID
certificate and notarized by Apple, so macOS opens it without a security
prompt. If you took the .dmg, drag Tangent into your Applications folder.

Windows: run the setup.exe and follow it. Every program in it is signed
under the author's name. SmartScreen judges a program by its reputation,
which a new signature takes time to earn, so the first run may still show
"Windows protected your PC". Click "More info", then "Run anyway", and
Windows remembers the choice. If you took the .zip, unzip it anywhere and
run tangent.exe.

Linux: extract the archive and run ./install.sh, which needs no root;
./install.sh --uninstall removes it again. You can also run ./tangent
straight from the folder. Tangent needs glibc 2.30 or newer and ALSA's
libasound.so.2, which almost every desktop already has.

FreeBSD 15: take the .pkg from the release page and run pkg install on it
(pkg install ./tangent-...-freebsd-FreeBSD-15-amd64.pkg). On a machine
without a network, unpack the desktop or complete bundle instead and run
sh install.sh.

The Windows and Linux downloads include tangent-ffmpeg, a separate program
built from FFmpeg, which films takes and writes MP3s. FFmpeg is licensed
under the GPL. Its licence text ships next to it, and the source for those
builds is published here:
Windows: https://github.com/ganten1998/tangent/releases/tag/third-party-src-ffmpeg-n8.1.3-btbn-2026-09-22
Linux: https://github.com/ganten1998/tangent/releases/tag/third-party-src-ffmpeg-8.1.3-tangent-linux

GETTING STARTED

A new session opens with tracks ready to play. Plug in your audio interface
and MIDI keyboard before you start Tangent. Tab moves between the keyboard view,
the arrangement and the mixer, the right-click menu holds everything else,
and holding H shows the keyboard shortcuts for the view you are in.

Source code and release notes:
https://github.com/ganten1998/tangent

---

---

# 4. Website card

Replaces the Tangent `<article>` in
`~/Dropbox/Projects/Web/ganten-site/projects.html` (lines 131 to 178 on
2026-10-12). The live card is stale in four ways:

- it says `v3.0.0` and dates itself 2026-08-14;
- it describes Tangent "inside your DAW, as a VST3 plugin", which is retired
  (owner ruling 2026-10-10: remove every user-facing mention);
- every link uses the old slug `github.com/ganten1998/ivory`, which only works
  through GitHub's rename redirect (and a deleted or recreated repository
  loses that redirect);
- it says nothing about the 32-track free desk or what a key does.

Rules for this copy: no em dashes, no mention of the Konami code (owner call
9), no third-party product names, and the key facts said the way every other
surface says them. Written from scratch for 5.0 (owner, 2026-10-12): the card
used to describe a chord display that also records, and it now describes a
DAW.

Publish it only after the 5.0.0 release is public and
`scripts/check-store-links.sh` is green from a signed-out machine. Then
`bash upload.sh` in `ganten-site/` (Dropbox strips the exec bit). If the old
screenshots are replaced, move retired files out of `media/` first and prune
them with `bash prune-remote.sh <path>`, because Neocities uploads never
delete.

**Screenshots:** the card leads with two new shots and keeps the theory
image already on the site. Before uploading, copy
`docs/img/screenshot-arrangement.png` and `docs/img/screenshot-mixer.png`
from this repository to `ganten-site/media/apps/tangent-arrangement.png` and
`ganten-site/media/apps/tangent-mixer.png` (1600x900 PNGs). `upload.sh`
sends them with the page; the card's `src` paths are
`media/apps/tangent-arrangement.png` and `media/apps/tangent-mixer.png`.
`media/apps/tangent-dark.png` is no longer used by the card: move it out of
`media/` and prune it with `bash prune-remote.sh media/apps/tangent-dark.png`
if nothing else on the site uses it.

---

## Short version (one paragraph, for a list or a social post)

Tangent 5.0: record, arrange and mix on macOS, Windows, Linux and FreeBSD.
Tangent is now a DAW. Record audio and MIDI, arrange
and edit it, and mix with buses, returns, sends, folders and automation,
using your own VST3 or LV2 plugins or the instruments and effects it comes
with. It still names the chord you are playing. Free with up to 32 tracks;
a supporter key removes the limit.

---

## The card (paste-ready HTML)

```html
        <article class="post">
          <header class="post__head">
            <h2 class="post__title">
              <a href="https://github.com/ganten1998/tangent" target="_blank" rel="noopener">Tangent</a>
            </h2>
            <span class="post__status" data-status="live">released</span>
            <time class="post__date" datetime="2026-10-12">2026 &middot; v5.0.0</time>
          </header>
          <div class="post__body">
            <p>
              Record, arrange and mix on macOS, Windows, Linux and FreeBSD.
              Free with up to 32 tracks; a supporter key removes the limit.
            </p>
            <img src="media/apps/tangent-arrangement.png" alt="Tangent's arrangement: drum, piano and audio tracks with MIDI and audio clips and a volume automation lane, under the recorder band" loading="lazy">
            <p>
              Tangent is a DAW. It records audio and MIDI, lays clips out on
              a timeline, edits notes in a piano roll and mixes them on a desk
              with buses, returns, sends, folders and automation. It hosts VST3 plugins everywhere and
              LV2 on Linux and FreeBSD, and it ships its own instruments and
              effects: modelled keys, an FM synth, an analog-style synth, a
              sampler, a drum machine, and a shelf of effects from EQ and
              compression to tape, reverb and a tube amp.
            </p>
            <img src="media/apps/tangent-theory.png" alt="Tangent naming a C major 13, with the circle of fifths, a Tonnetz and the harmonic triangles above a piano keyboard and a guitar neck" loading="lazy">
            <p>
              It started as a program that names the chord you play, and it
              still does: as you play, it writes the chord on a staff, places
              it on a guitar neck and draws it on the circle of fifths. A take
              can film the window with your camera in it, and outboard gear
              can sit in an insert slot, measured and in time with the rest
              of the session.
            </p>
            <img src="media/apps/tangent-mixer.png" alt="Tangent's mixer: a strip per track with inserts, routing, sends and faders, a folder, a return and the master" loading="lazy">
            <p>
              Written in Rust and MIT licensed. Buses, returns, sends and
              routes have no limit for anyone, free or not. It makes no
              network connections of its own: no telemetry, no update check,
              no account.
            </p>
            <p>
              <a href="https://github.com/ganten1998/tangent/releases/latest" target="_blank" rel="noopener">Download</a><span class="sep">&middot;</span><a href="https://github.com/ganten1998/tangent" target="_blank" rel="noopener">Source</a><span class="sep">&middot;</span><a href="https://ganten.gumroad.com/l/tangent" target="_blank" rel="noopener">Support it</a>
            </p>
            <p class="post__tags">
              <span class="tag">rust</span>
              <span class="tag">egui</span>
              <span class="tag">daw</span>
              <span class="tag">audio</span>
              <span class="tag">midi</span>
            </p>
          </div>
        </article>
```

Notes:

- "no network connections of its own" is deliberate: Stream to OBS sends
  audio to the address you give it once you switch it on, and a plugin you
  load is its maker's program. The README's Privacy section says the same.
- The owner decided (2026-10-12) to make the GitHub repository public at
  publish time. Until it is, the Download and Source links are 404s for
  every visitor, so upload this card only after it is public (CHECKLIST.md,
  D1).

---

# 5. Announcement

For existing users: supporters by email, and the same text for a post.
Written from scratch for 5.0 (owner, 2026-10-12): it tells a supporter that
Tangent is now a DAW before it tells them what changed, and names no
third-party product.

How it goes out (from the supporter-licence notes; nothing here sends it):
Gumroad cannot email customers at these numbers, so the mail goes through the
fulfil service's `announce` command on the Fly machine, which dedupes by
address, is a dry run unless `--send` is given, and refuses an em dash. The
owner runs the `--send` himself. Send it only after the release is public,
`scripts/check-store-links.sh` is green from a signed-out machine, the
Gumroad page is re-enabled and the fulfil service is redeployed.

```sh
flyctl ssh console -a tangent-fulfil -C 'tangent-fulfil announce --subject "Tangent 5.0 is out" --body /tmp/body.txt'          # dry run
flyctl ssh console -a tangent-fulfil -C 'tangent-fulfil announce --subject "Tangent 5.0 is out" --body /tmp/body.txt --send'   # owner only
```

(Upload the body first by piping base64 into `base64 -d > /tmp/body.txt` on
the machine; a redeploy wipes `/tmp`, so upload after the deploy. The usage
is `tangent-fulfil announce --subject <text> --body <file> [--ledger <path>]
[--send]`, tools/tangent-fulfil/src/main.rs:454.)

"Replies come to me" holds only if the deployed service has `MAIL_REPLY_TO`
unset or pointing at a read inbox (default `tangentdaw@gmail.com`,
main.rs:352).

---

Subject: Tangent 5.0 is out

Hi,

Tangent 5.0 is out: record, arrange and mix on macOS, Windows, Linux and
FreeBSD. Free with up to 32 tracks; your key removes the limit.

You bought a key for a program that named the chords you
played and could record them. This release is the one where it becomes a
DAW.

You can record audio and MIDI into it, arrange clips on a timeline, edit
notes in a piano roll, and mix on a desk with buses, returns, sends, folders
and automation. It hosts your VST3 plugins (and LV2 on Linux and FreeBSD) and
tells them the song's tempo and transport. It also comes with its own
instruments, among them modelled pianos and electric pianos, an analog-style
synth and a sampler, and a shelf of effects from EQ and compression to tape,
reverb, a tube amp and a bitcrusher. Tangent Insert puts your outboard gear
in an insert slot and keeps the session in time with it. Exports can pick
their tracks and their range. FreeBSD is now a supported platform too.

It still names the chord you are playing, and a take can still film the
window.

What your key does now: the free app holds up to 32 tracks, and your key
removes that limit. Every key ever sold does this, so there is nothing for
you to do. Buses, returns, sends and routes have no limit for anyone.

Two things to know before you upgrade from 4.x:

1. Tangent as a VST3 plugin inside another DAW is gone. The 5.0 installers
   remove the old Tangent.vst3 on macOS and Windows; on Linux, delete
   ~/.vst3/Tangent.vst3 yourself. Tangent still hosts your plugins.

2. 5.0 keeps its settings in a new folder and deletes the old
   ~/.config/ivory folder the first time it starts. That folder held your
   settings, the chord names you taught Tangent, and your key. Your projects
   and recordings are not in it and are not touched. After upgrading, paste
   your key again (right-click anywhere, Support Tangent..., Activate). It is
   in the email your key first came in, from this same address; if you
   cannot find it, reply and I will send it again. If you taught Tangent
   chord names you want to keep, copy the ~/.config/ivory folder somewhere
   safe before you start 5.0, and the release notes say where to put it
   back.

Download (these links always fetch the current version):

  macOS 11 or later, Apple Silicon or Intel
  https://github.com/ganten1998/tangent/releases/latest/download/Tangent-macos.pkg

  Windows 10 or later
  https://github.com/ganten1998/tangent/releases/latest/download/Tangent-windows-setup.exe

  Linux x86_64
  https://github.com/ganten1998/tangent/releases/latest/download/tangent-linux-x86_64.tar.gz

  Everything else (Linux aarch64, FreeBSD, the .dmg and .zip, checksums, the
  full release notes):
  https://github.com/ganten1998/tangent/releases/latest

On Windows every program in the download is signed. SmartScreen may still say
"Windows protected your PC" the first time; click More info, then Run
anyway.

Thank you for supporting it. Replies come to me.

Ganten
