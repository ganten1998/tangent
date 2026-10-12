# Gumroad store content

The text a buyer sees after paying, and where each piece of it lives.

There are two surfaces, and only one of them is inside this repo:

| surface | who owns it | how it is edited |
|---|---|---|
| Post-purchase page ("View content") | Gumroad | web UI, paste the block below |
| Supporter-key email | us | `tools/tangent-fulfil/src/main.rs`, `email_body()` |

Gumroad's own receipt email is not customisable in any useful way, so the
supporter-key email is where the download links have to be. It is the one
message a buyer keeps.

## The links, and why they are shaped this way

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

**Links, not uploaded copies.** Gumroad can host the binaries itself, and that
was the alternative. Links win because the buyer always gets the current
release: uploaded copies go stale the moment a version ships and have to be
re-uploaded by hand, which is exactly the step that gets forgotten. The cost is
that downloads depend on GitHub being reachable, which is the same dependency
the public README already has.

## Product description

Gumroad → Products → Tangent → **Description**. This is the public page, the
one people read before paying, and it is separate from the Content tab below.

Kept here because it has been lost once already. If you rewrite it on the web,
paste it back into this file. The fuller 5.0 page, with its own tagline and
tags, is `docs/release/<newest>/GUMROAD.md`; this is the short form of the same
facts, for when the long one is too much.

NO EM DASHES anywhere in it. They read as machine-written and have drawn a
real accusation before.

What a key does is said the same way on every surface: the free app holds 32
tracks, a key takes that limit away (keys already sold included), and buses,
returns, sends and routes have no limit for anyone. The app also names "a
certain code" beside the key; the store, the email and the README do not (owner
call 9 in `docs/design/unlimited-tracks.md`). `copy_scan.rs` in tangent-ui
fails the build if the old wording (a key as a thanks that unlocks nothing)
comes back here, in the README or in the email.

Written from scratch for 5.0, when Tangent stopped being a keyboard monitor
that records and became a DAW. No third-party product is named in it.

---

Tangent is a DAW for macOS, Windows, Linux and FreeBSD.

Record audio and MIDI. Arrange clips on a timeline, edit notes in a piano
roll, program drums on sixteen pads, and mix on a desk with buses, returns,
sends, folders and automation that reaches plugin parameters. Export a
mixdown or stems, as WAV or MP3.

It hosts VST3 plugins on every platform and LV2 plugins on Linux and
FreeBSD, and tells them the song's tempo and transport. It also brings its
own: a modelled keyboard with eight sounds, an FM synth, an analog-style
synth, a sampler, and a shelf of effects from EQ and compression to tape,
reverb, amp and a bitcrusher. Tangent Insert puts your outboard gear in an
insert slot and keeps the session in time with it.

Some things it does that you may not expect from a DAW. It names the chord
you are playing as you play it, writes it on a staff and places it on a
guitar neck. And a take can film the window itself, your camera included,
and save the video beside the audio and the MIDI.

Tangent is free, with up to 32 tracks: MIDI tracks, audio tracks and folders
count, and buses, returns, sends and routes have no limit for anyone. There
is no trial, no time limit and no account. A supporter key removes the track
limit and lights a small heart in your theme's colours. Every key already
sold does the same.

It works offline, collects nothing, and checks your key on your own machine.

macOS 11 or later (Apple Silicon or Intel, signed and notarized), Windows 10
or later (signed), Linux x86_64 and aarch64, FreeBSD 15 (amd64).

---

## Before pasting anything, check the links resolve

Every download link resolves by EXACT asset name against whatever release is
currently latest. A name that is not on that release 404s, and the store page
and the key email are the two places a buyer meets those links.

This has now gone wrong twice: once by redeploying the key email ahead of its
assets, and once by writing the 3.0.0 installer names into this file while
2.3.0 was still the latest release. Both times the text was right and the
release was not.

So check, do not assume:

```sh
scripts/check-store-links.sh
```

It pulls every `releases/latest/download/` URL out of this file and out of
`tools/tangent-fulfil/src/main.rs` and fetches each one. Run it after publishing
and before pasting.

## The 3.0.0 swap: done

The block below hands out the **installers** rather than the archives: each
one puts Tangent where it belongs, so there is nothing for a buyer to drag
anywhere. Those assets first existed at 3.0.0, published 2026-08-14;
`tools/tangent-fulfil/src/main.rs` was swapped at the same moment and must be
redeployed for a NEW buyer to get them.

The order that keeps this honest, and the one that was got wrong twice:
**publish, check, paste, deploy.** Never edit the links in anticipation.

One more gate, same spirit: the post-purchase block below tells buyers the
source for the bundled tangent-ffmpeg is published as two releases,
`third-party-src-ffmpeg-n8.1.3-btbn-2026-09-22` and
`third-party-src-ffmpeg-8.1.3-tangent-linux`. That sentence becomes true only once
`scripts/publish-third-party-src.sh` has created both (LICENSING.md, "The
source it is owed") and `scripts/check-store-links.sh` finds them. Do not
paste it to Gumroad before then.

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

## Keeping the two in step

The email and the page say the same things about activation and first launch. If
you change one, change the other. The email version is
`email_body()` in `tools/tangent-fulfil/src/main.rs`; its tests assert the three
download links are present and unpinned, so deleting a link fails the build
rather than quietly shipping a page with nothing on it.

`cargo test -- --ignored --nocapture print_email` prints the email exactly as a
buyer receives it.
