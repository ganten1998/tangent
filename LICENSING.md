# Licensing

Tangent ships one program, under the MIT licence, and some downloads carry a
third-party program and libraries beside it under their own terms. This file
exists so all of that is written down once, precisely, rather than
reconstructed from memory later.

| Artifact | Licence | Why |
|---|---|---|
| `Tangent.app` / `tangent` / `tangent.exe` | **MIT** | Links no GPL. The LGPL LAME static link (macOS/FreeBSD) adds duties, not a licence change; see below. |
| `tangent-core` and every other crate the app is built from | **MIT** | Source, not binaries. |
| `tangent-ffmpeg` / `tangent-ffmpeg.exe` (Linux and Windows downloads) | **GPL**, ffmpeg's own | A separate program that Tangent starts, not part of Tangent; see below. |

## Hosting VST3 plugins

The VST3 SDK is **MIT**. Steinberg relicensed it on **2025-10-29**, with
VST 3.8; until then it was dual-licensed, GPLv3 or a separate proprietary
agreement with Steinberg. Verified at source:
`steinbergmedia/vst3sdk/LICENSE.txt` and
`steinbergmedia/vst3_pluginterfaces/LICENSE.txt` are both plain MIT
("Copyright (c) 2026, Steinberg Media Technologies GmbH"), the GitHub API
reports `spdx_id: MIT` on both repositories, and the SDK README states that
"Licensing under GPLv3 and the Steinberg proprietary license is no longer
available." There is no host/plug-in distinction under MIT; the old SDK licence
had one.

So **an MIT application may host VST3 plugins.** That is what makes the
Recorder view's plugin-hosting workstream legally possible; see
`docs/RECORDER-PLAN.md` §8. Tangent's host reads the VST3 interfaces through
the `vst3` crate (coupler-rs, `MIT OR Apache-2.0`). The only surviving
obligation is trademark, and it is now optional under MIT (see the trademark
section below, which still applies).

## LAME, statically linked into the macOS and FreeBSD builds

The default `mp3` feature pulls `mp3lame-encoder` 0.2.5, whose `mp3lame-sys`
0.1.11 vendors the unmodified LAME 3.100 sources
(https://lame.sourceforge.io/) and links them **statically** into the
`tangent` binary. That is live wherever the default features build: macOS
(`build-macos.sh`) and FreeBSD (`build-freebsd.sh`). The Linux and Windows
artifacts build with `--no-default-features` and do not link LAME at all;
their MP3 export runs through the separate `tangent-ffmpeg` process.

LAME is licensed under the GNU Library General Public License, version 2 or,
at your option, any later version. The two wrapper crates declare LGPL-3.0.
The app stays MIT; a weak-copyleft static link does not relicense it, but
it does attach three obligations to the artifacts that carry the library:

1. **Notice.** Each artifact must say it contains LAME and where the licence
   texts are. The notice of record is this section plus the LAME entry in
   THIRD-PARTY-LICENSES once regenerated (see status below).
2. **Licence texts.** Ship both texts, verbatim, beside the binary: LGPL v2
   covers LAME itself, LGPL v3 covers the wrapper crates. Until the build
   scripts ship a `lame-licenses/` folder, the texts live in the vendored
   crate (`mp3lame-sys-0.1.11/lame-3.100/COPYING`) and at
   https://www.gnu.org/licenses/old-licenses/lgpl-2.0.txt and
   https://www.gnu.org/licenses/lgpl-3.0.txt.
3. **Relink route** (LGPL-2.1 section 6, LGPL-3.0 section 4(d)): a recipient
   must be able to rebuild the application against a modified LAME. That is
   available in substance: Tangent's complete source is MIT and public, and
   `Cargo.lock` pins `mp3lame-sys` 0.1.11, which contains the exact LAME
   sources that were linked. To relink: check out the release tag, place a
   modified copy of `mp3lame-sys` (carrying your LAME changes) in the tree,
   point a `[patch.crates-io]` entry at it, and run
   `cargo build --release -p tangent`.

**Status, 2026-08-31.** This file is the notice of record today. Still to do,
and tracked as W30 in the 5.1 audit worklist: add the LAME section to
`scripts/gen-third-party-licenses.sh` and regenerate THIRD-PARTY-LICENSES
(stale since before mp3lame entered Cargo.lock, because every build script
regenerates only when the file is MISSING); add `assets/lame-licenses/` with
both texts; ship that folder from `build-macos.sh` and `build-freebsd.sh`
(the FreeBSD tarball currently ships only LICENSE, not even
THIRD-PARTY-LICENSES).

## ffmpeg: GPL binaries in the Linux and Windows artifacts

`scripts/fetch-ffmpeg.sh` downloads prebuilt **GPL** ffmpeg binaries, pinned
by sha256, and the Linux tarballs and the Windows artifact ship them as
`tangent-ffmpeg(.exe)`:

* Linux x86_64 and arm64: johnvansickle.com static ffmpeg **7.0.2**.
* Windows: BtbN autobuild ffmpeg **n8.1.3** (`ffmpeg-n8.1.3-win64-gpl-8.1.zip`),
  from the dated tag `autobuild-2026-09-22-13-18` of `BtbN/FFmpeg-Builds`.
  BtbN prunes dated tags after a few weeks, so the pin says which build, not
  where it can still be fetched. Every Windows build made from e5ae43b up
  to d6547f6 (2026-08-18 to 2026-09-22), that is releases 4.4.1 through
  5.0.0-beta.13 and the 5.0.0-dev builds handed to testers in that span,
  the last of them 21567be, bundled **n8.1.2-44-g7c533d0f86** from
  `autobuild-2026-08-17-13-05` (already gone upstream).
* Once Windows signing is configured, the shipped `tangent-ffmpeg.exe`
  carries Tangent's Authenticode signature, because Smart App Control
  blocks unsigned programs. The program is still BtbN's build byte for
  byte, apart from the appended signature and the header fields signing
  updates; the cached copy in `dist/vendor` is never signed.

macOS and FreeBSD are unaffected (AVFoundation, and the `multimedia/ffmpeg`
port, respectively).

**Why it does not make the app GPL.** GPLv3 section 5 defines an
**aggregate**:

> A compilation of a covered work with other separate and independent works,
> which are not by their nature extensions of the covered work, and which are
> not combined with it such as to form a larger program, in or on a volume of a
> storage or distribution medium, is called an "aggregate" [...] Inclusion of a
> covered work in an aggregate does not cause this License to apply to the other
> parts of the aggregate.

Tangent and `tangent-ffmpeg` are separate executables. Tangent starts the
encoder as a process of its own and talks to it the way separate programs do:
command-line arguments, a pipe for a take's frames, a file for an export's
samples. Neither links to the other. Putting both in one download is
distribution on a shared medium, which is exactly what section 5 describes.

**Three rules keep that true. Do not break them casually:**

1. **The app runs without it.** Tangent looks for `tangent-ffmpeg` beside
   itself and then for an `ffmpeg` on `PATH`; with neither, a take still
   records its audio and MIDI. Video, and MP3 export on Linux and Windows,
   are what it is for.
2. **They never link.** No ffmpeg library is compiled into Tangent or loaded
   by it. If Tangent ever needs ffmpeg as a library rather than as a program
   it starts, this analysis has to be redone before shipping.
3. **Each carries its own licence.** `LICENSE` (MIT) next to the app;
   ffmpeg's licence text and a provenance note in `ffmpeg-licenses/` next to
   the encoder.

**The source it is owed.** Because the artifacts are handed out through
paid-adjacent channels (the Gumroad post-purchase page and the supporter
email), GPLv3 section 6(d) is the only applicable source-offer option, and it
requires the **complete corresponding source of those exact builds** to be
available from a designated server that WE keep up, for as long as the
binaries are offered. Pointing at ffmpeg.org, at BtbN's release page (which
hosts no source archives), or at johnvansickle.com (a rotating hobbyist
server) does not meet that.

**The hosting plan**, one mirror per binary pin, as assets on dedicated
GitHub releases in the same repo the binaries are served from:

* Tag `third-party-src-ffmpeg-7.0.2`: the archives from
  johnvansickle.com/ffmpeg/release-source/ for the 7.0.2 static build (the
  builder's GPL source drop for that build, bundled libraries included) plus
  his build readme. Fetch and hash these promptly; that server rotates old
  builds out.
* Tag `third-party-src-ffmpeg-n8.1.2-btbn-2026-08-17`, for the Windows
  builds already out (every build from e5ae43b up to d6547f6: 4.4.1 through
  5.0.0-beta.13 and the tester builds of that span): the FFmpeg source
  archive at commit `7c533d0f86`, a git archive of `BtbN/FFmpeg-Builds` at
  the COMMIT the tag `autobuild-2026-08-17-13-05` pointed to (the tag itself
  is gone upstream), and the source tarball of every bundled library that
  commit's `scripts.d/` files pin. "Reconstructible from someone else's repo"
  is not hosting; the tarballs themselves go in the release.
* Tag `third-party-src-ffmpeg-n8.1.3-btbn-2026-09-22`, for the current pin:
  the FFmpeg source at the release tag `n8.1.3`, a git archive of
  `BtbN/FFmpeg-Builds` at `autobuild-2026-09-22-13-18` (archive it now,
  before it is pruned too), and the pinned library tarballs.
* Never delete these source releases while the binaries are offered: the
  `releases/latest/download` links in old supporter emails keep old binaries
  on offer indefinitely, so availability is indefinite too.

Follow-up work that lives in scripts, tracked as W31 in the 5.1 worklist and
not yet done: a `scripts/fetch-ffmpeg-sources.sh` mirroring
`fetch-ffmpeg.sh`'s pin-by-sha256 style, so bumping the binary pin forces
bumping the source mirror in the same edit; `provenance()` in
`fetch-ffmpeg.sh` rewriting SOURCE.txt to name the hosted URL as the
licence-satisfying one (the upstream URL stays as provenance); the
THIRD-PARTY-LICENSES ffmpeg section naming the hosted URL; and the hosted
URLs joining the `check-store-links.sh` sweep so a deleted source release
fails the same gate that guards download links.

**The uploads themselves are an owner action**: they need release
credentials, and a standing decision that GitHub remains the hosting venue.
Until all three tags exist with their assets, distribution of the Linux and
Windows artifacts is out of compliance on this point, and no documentation
edit changes that.

## Trademarks, which are separate from all of the above

Steinberg's VST trademark terms are not a copyright matter and apply regardless
of which SDK licence is used. The relevant one: **"VST" must not appear in the
product name.** "Tangent" satisfies this. "VST" belongs only where Tangent
says what it hosts, as in "VST3 plugin hosting".

Tangent's own name and icon are covered by [TRADEMARK.md](TRADEMARK.md) at the
repo root. That file is the single policy; change it rather than adding
trademark prose here, so the two cannot drift apart silently.

## Not legal advice

This is a written-down reading of the licences, not an opinion from a lawyer.
The aggregation analysis above is the standard one and the structure follows the
conservative path, but the first release that takes money is a reasonable moment
to have someone qualified confirm it.
