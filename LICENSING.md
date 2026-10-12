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
   texts are: `lame-licenses/NOTICE.txt` beside the binary, and the LAME
   section of THIRD-PARTY-LICENSES.
2. **Licence texts.** Ship them verbatim beside the binary: LGPL v2 covers
   LAME itself, LGPL v3 covers the wrapper crates, and LGPL v3 is written
   as additional permissions on GPL v3, whose text it requires alongside
   (LGPL-3.0 section 4(b)).
3. **Relink route** (LGPL-2.1 section 6, LGPL-3.0 section 4(d)): a recipient
   must be able to rebuild the application against a modified LAME. That is
   available in substance: Tangent's complete source is MIT and public, and
   `Cargo.lock` pins `mp3lame-sys` 0.1.11, which contains the exact LAME
   sources that were linked. To relink: check out the release tag, place a
   modified copy of `mp3lame-sys` (carrying your LAME changes) in the tree,
   point a `[patch.crates-io]` entry at it, and run
   `cargo build --release -p tangent`.

**Status, 2026-10-12: done.** `assets/lame-licenses/` holds, verbatim:
`LAME-COPYING-LGPL-2.txt` and `LAME-LICENSE.txt` (LAME 3.100's own `COPYING`
and `LICENSE`, copied from `mp3lame-sys-0.1.11/lame-3.100/` in the cargo
registry, the exact sources Cargo.lock links); `LGPL-3.0.txt` (the
`LICENSE` both crates carry, byte-identical); `GPL-3.0.txt` (`COPYING.GPLv3`
from the signature-verified FFmpeg 8.1.3 release tarball, the FSF text);
and our `NOTICE.txt`. They ship wherever LAME is linked:

* macOS: `build-macos.sh` puts them in `Tangent.app/Contents/Resources/lame-licenses/`,
  so the zip, the dmg and the pkg (`build-installer.sh` packs that app)
  all carry them.
* FreeBSD: `build-freebsd.sh` installs them to
  `share/licenses/tangent/lame-licenses/` in the package (listed in
  `installer/freebsd/pkg-plist`) and copies them into both offline bundles.
* Linux built natively (`build-linux-native.sh`, default features): in the
  tarball.

`scripts/check-artifacts.sh` refuses a macOS zip or pkg, a FreeBSD package
or a FreeBSD bundle without all five files. The Windows download and the
cross-built Linux tarballs (`build-cross.sh`) build without the `mp3`
feature, do not contain LAME, and carry none of these; their MP3 export
runs through `tangent-ffmpeg`, whose own LAME is covered by the ffmpeg
source releases below. THIRD-PARTY-LICENSES has a LAME section
(`scripts/gen-third-party-licenses.sh`).

## ffmpeg: GPL binaries in the Linux and Windows artifacts

The Linux tarballs and the Windows artifact ship a **GPL** ffmpeg as
`tangent-ffmpeg(.exe)`, put in place by `scripts/fetch-ffmpeg.sh`:

* Linux x86_64 and arm64: ffmpeg **8.1.3 built by us**, by
  `scripts/build-ffmpeg-linux.sh`, from pinned sources: the FFmpeg release
  tarball (signature checked), x264 at commit `b35605ace3dd` (its stable
  branch), LAME 3.100, and NASM 3.02 as the x86_64 assembler. Configured
  with only what Tangent calls it for (libx264, LAME, FFmpeg's own AAC and
  the decoders of the import formats; no network, no hardware
  acceleration), cross-compiled with zig for glibc 2.30, the libraries
  linked statically. GPLv3 (`--enable-gpl --enable-version3`). Every Linux
  artifact built before this change bundled johnvansickle.com's static
  **7.0.2** instead (see the source list below for why it was
  replaced).
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

**Where the source is.** One GitHub release per binary pin, in the same
repository the binaries are served from, created by
`scripts/publish-third-party-src.sh` from the folders
`scripts/fetch-ffmpeg-sources.sh` stages (default
`~/Library/Caches/tangent-dist/third-party-src/<release>/`, each with a
`SOURCE.txt` saying what every file is and where it came from, and a
`SHA256SUMS.txt`). Each is a pre-release with `--latest=false`, so it can
never become the Latest release the download links resolve against.

* `third-party-src-ffmpeg-n8.1.3-btbn-2026-09-22`, for the Windows pin.
  Staged and complete as far as the build's own scripts define it: the
  FFmpeg 8.1.3 release tarball (signature verified) and the git tree at
  `n8.1.3`; `BtbN/FFmpeg-Builds` at commit `3e6685eda92f`, the commit the
  pruned tag `autobuild-2026-09-22-13-18` pointed to (identified from
  BtbN's workflow run 35726071479, whose release job ran 13:16 to 13:38 UTC
  that day); and the source of all 125 library pins in that commit's
  `scripts.d/`, plus what their download steps fetch beyond a checkout
  (shaderc's DEPS repositories, Opus's model data, glib's proxy-libintl,
  and the vendored crates of librsvg and rav1e). About 1.1 GB, 148 files.
  Not included, and not owed: the compilers and build tools in BtbN's
  Docker images (GPLv3 section 1 excludes general-purpose tools); the
  runtime pieces they link (libgcc, libstdc++, libatomic) are under the GCC
  Runtime Library Exception.
* `third-party-src-ffmpeg-8.1.3-tangent-linux`, for the Linux builds
  (x86_64 and aarch64). **Complete by construction**: it is staged by
  `scripts/build-ffmpeg-linux.sh` on every run, from the very files that run
  built from, and holds the FFmpeg 8.1.3 release tarball and its signature,
  the x264, LAME and NASM source tarballs, and a copy of the build script
  itself, which is the scripts used to control compilation and installation.
  The script runs from inside that folder alone (zig 0.16.0 is the only
  tool it needs that is not on an ordinary build machine; GPLv3 section 1
  excludes compilers), and the same zig rebuilds the same bytes (checked:
  two builds from different folders, one from the staged release, gave
  identical binaries on both architectures).
* `third-party-src-ffmpeg-7.0.2`, for the johnvansickle.com builds every
  Linux artifact bundled before ours (x86_64 and aarch64). Not in
  `publish-third-party-src.sh`'s list any more. **Incomplete.** It holds the FFmpeg 7.0.2 release tarball (signature
  verified) and the builder's readme, which names every library version
  the build links. It does not hold those libraries' source or the
  builder's build scripts and configure options, which GPLv3 counts as part
  of the complete corresponding source: johnvansickle.com publishes none
  for this build (its `release-source/` folder holds FFmpeg 4.1 from 2018),
  so there is nothing to mirror. A written offer (GPLv3 section 6(b)) does
  not help either, because it promises source we do not have. The owner's
  decision was the last of the three ways out: an ffmpeg built by our own
  script with only what Tangent uses, whose script and pinned tarballs are
  the source release (the entry above). What stays open is the source owed
  for 7.0.2 binaries already handed out, which only the builder can supply
  (he owes it to us as recipients) or which stops mattering when those
  artifacts stop being offered.
* The Windows builds made from e5ae43b up to d6547f6 (4.4.1 through
  5.0.0-beta.13) bundled n8.1.2-44-g7c533d0f86 from
  `autobuild-2026-08-17-13-05`. No source release is staged for it: the
  plan is to stop offering those binaries by deleting the old GitHub
  releases once 5.0.0 is Latest (docs/release/next/CHECKLIST.md, section
  J). The source is owed for as long as anyone was given those binaries
  under 6(d), so if that is judged to reach past the deletion, the same
  fetcher can stage it from BtbN's commit for that day.
* Never delete these source releases while the binaries are offered: the
  `releases/latest/download` links in old supporter emails keep binaries
  on offer indefinitely, so availability is indefinite too.

`scripts/check-store-links.sh` sweeps the source releases' links in
README.md and the post-purchase page with the download links, so a deleted
or never-created source release fails the same gate, and each artifact's
`ffmpeg-licenses/SOURCE.txt` (`provenance()` in `fetch-ffmpeg.sh`) names
the hosted release for its pin, the "clear directions next to the object
code" GPLv3 section 6(d) asks for.

**The uploads are an owner action**: they need release credentials and the
repository public (the script refuses a private one). Until both releases
exist with their assets, distribution of the Linux and Windows artifacts is
out of compliance on this point. A Linux artifact that still carries the
7.0.2 build stays out of compliance until it is withdrawn or rebuilt.

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
