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
paste it back into this file.

NO EM DASHES anywhere in it. They read as machine-written and have drawn a
real accusation before.

What a key does is said the same way on every surface: the free app holds 32
tracks, a key takes that limit away (keys already sold included), and buses,
returns, sends and routes have no limit for anyone. The app also names "a
certain code" beside the key; the store, the email and the README do not (owner
call 9 in `docs/design/unlimited-tracks.md`). `copy_scan.rs` in tangent-ui
fails the build if the old wording (a key as a thanks that unlocks nothing)
comes back here, in the README or in the email.

---

Play a chord. Tangent names it.

Tangent is a MIDI keyboard monitor for people who want to see what they are
playing. Plug in a keyboard and all 88 keys light up in real time, while the
chord engine reads what you are holding: plain triads, sevenths, extensions,
altered dominants, sus and add chords, slash chords, rootless jazz voicings,
and 28 scales and modes. 95 chord patterns against all 12 roots.

It shows the same notes two more ways.

On a guitar neck, where a player would actually put their fingers. One MIDI
note can be six places on a guitar, so Tangent picks the shape a hand would
use, weighing span, open strings and barres, and holds it steady as you add
notes instead of jumping around the neck. Seven tunings, a capo, and the neck
works backwards too: click it to build a shape and read the chord off the
piano.

As geometry, on the circle of fifths, on a Tonnetz, and as the I, IV and V
triangles. Every key shaded by how much of your chord belongs to it, so keys
that are close light up together.

Teach it your own names. If you call something a Hendrix chord, right-click,
type it, and that is what it says from then on, in every key if you want.

Tangent is free, with up to 32 tracks. There is no trial, no time limit and no
account, and buses, returns, sends and routes have no limit for anyone. It
talks to no server and collects nothing. Paying gets you a supporter key,
which takes the track limit away and sets a small pixel heart moving through
your theme's colours, and my thanks. Every key already sold does the same.

macOS 11 or later (Apple Silicon or Intel), Windows 10 or later, Linux x86_64.
Signed and notarized on macOS.

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

The block below hands out the **installers** rather than the archives. At 3.0.0
that was because each one offered the app and the plugin as separate choices and
put the plugin where the DAW already looks; the plugin was retired at 4.77.0,
and the installers carry the app alone now. Those assets first existed at
3.0.0, published 2026-08-14; `tools/tangent-fulfil/src/main.rs` was swapped at
the same moment and must be redeployed for a NEW buyer to get them.

The order that keeps this honest, and the one that was got wrong twice:
**publish, check, paste, deploy.** Never edit the links in anticipation.

One more gate, same spirit: the post-purchase block below now tells buyers the
FFmpeg source for the bundled tangent-ffmpeg is available from the releases
page. That sentence becomes true only once the `third-party-src-ffmpeg-*`
source releases described in LICENSING.md exist. Do not paste it to Gumroad
before they do.

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
limit for anyone, and every other feature is in the free app too.

DOWNLOAD TANGENT

These links always give you the current version, so they are worth keeping.

macOS 11 or later, Apple Silicon or Intel
https://github.com/ganten1998/tangent/releases/latest/download/Tangent-macos.pkg

Windows 10 or later
https://github.com/ganten1998/tangent/releases/latest/download/Tangent-windows-setup.exe

Linux x86_64 (the tarball has an install.sh that needs no root)
https://github.com/ganten1998/tangent/releases/latest/download/tangent-linux-x86_64.tar.gz

Prefer to unpack it yourself? The .dmg, .zip and .tar.gz are all on the release
page: https://github.com/ganten1998/tangent/releases/latest

Checksums:
https://github.com/ganten1998/tangent/releases/latest/download/SHA256SUMS

YOUR SUPPORTER KEY

The key is emailed to you within about a minute of your purchase, from
keys@tangentdaw.org. If it has not arrived, search your spam folder for that
address. If it is still missing, reply to your Gumroad receipt and I will send
it by hand.

To use it: open Tangent, right-click anywhere, choose "Support Tangent...", paste
the key and press Activate. Case, spaces, dashes and line breaks do not matter.

The key has no expiry, contacts no server, and works on every machine you own.

FIRST LAUNCH

macOS: open the .pkg and follow it. It is signed with a Developer ID
certificate and notarized by Apple, so there is no security prompt. If you took
the .dmg instead, drag Tangent into your Applications folder.

Windows: run the setup.exe and follow it. Every release after 5.0.0 is
code-signed under the author's name (5.0.0 itself is not), but SmartScreen goes
on reputation, which a new signature has to earn, so it may still show "Windows
protected your PC" the first time. Click "More info", then "Run anyway". Windows
remembers the choice. If you took the .zip instead, unzip it anywhere and run
tangent.exe.

Linux: extract the archive and run ./install.sh, which needs no root and takes
--app and --uninstall. Or just chmod +x tangent and ./tangent. MIDI goes
through ALSA, so libasound.so.2 needs to be present. Requires glibc 2.32 or
newer.

The Windows and Linux downloads include tangent-ffmpeg, a small helper program
built from FFmpeg and used for audio export. FFmpeg is licensed under the GPL.
Its licence text ships next to it, and the complete source code for those
exact builds is available from the project's releases page:
https://github.com/ganten1998/tangent/releases

Plug your MIDI keyboard in before you start Tangent. Everything in the app
lives in the right-click menu, and holding H shows every keyboard shortcut.

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
