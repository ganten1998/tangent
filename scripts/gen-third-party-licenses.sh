#!/usr/bin/env bash
# Generate THIRD-PARTY-LICENSES at the repo root from cargo metadata.
#
#   scripts/gen-third-party-licenses.sh [OUTPUT]    # default: THIRD-PARTY-LICENSES
#
# ONE generator path, deliberately. This used to prefer cargo-license when
# installed and fall back to `cargo metadata` + python3 otherwise — two
# different output formats, so the same Cargo.lock produced different bytes
# depending on which machine ran it. That was survivable while regeneration
# was a documented step in docs/RELEASE.md, and fatal the day release.sh
# started enforcing it: the release gate regenerates into a temp file and
# BYTE-DIFFS it against the tracked one, so the generator must be a pure
# function of Cargo.lock. `cargo metadata --locked` is on every machine that
# can build Tangent at all, works offline once the deps are cached, and its
# packages are sorted here before printing.
#
# The OUTPUT argument exists for that same gate — release.sh writes to a temp
# path and compares, without touching the tracked file.
#
# Rerun after any dependency change and commit the result; the build scripts
# bundle the tracked file into every release artifact, and release.sh refuses
# to build one while it disagrees with what this script generates.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

# Homebrew rust shadows rustup's cargo on PATH; prefer the rustup toolchain.
TOOLCHAIN_BIN="$(rustup which cargo 2>/dev/null | xargs dirname || true)"
[ -n "$TOOLCHAIN_BIN" ] && export PATH="$TOOLCHAIN_BIN:$HOME/.cargo/bin:$PATH"

OUT="${1:-THIRD-PARTY-LICENSES}"

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
# Assembled in the temp dir and moved into place only when complete. The build
# scripts bundle this file on a bare existence check, so a generator that dies
# half-way (guard failure, cargo refusing a stale lockfile) must not leave a
# fonts-only truncation sitting at the real name looking bundleable.
WIP="$TMP/out"

# The font section is hand-maintained prose, and hand-maintained is how it
# went wrong: the file said SIX fonts while the binary embedded ten, because
# JetBrains Mono was added to tangent-ui/src/fonts.rs and nobody was reminded
# this text exists. The guard after the heredoc scans fonts.rs for every
# embedded font file and fails generation when one is missing from the text
# below — so the next font cannot ship unlisted, only undescribed loudly.
cat > "$WIP" <<'EOF'
THIRD-PARTY LICENSES: Tangent
============================

Fonts
-----
Tangent's executable statically embeds TEN font files: six bundled by Tangent
itself and four that arrive with its UI toolkit. All ten are free/libre, none
is modified or subset, and none is ever sold standalone. Tangent does not
bundle or redistribute Courier New. The full license text for every font ships
in every Tangent distribution, at the paths given below.

The six Tangent bundles (embedded by tangent-ui/src/fonts.rs):

  Courier Prime Regular + Bold  --  UI typeface
      (CourierPrime-Regular.ttf, CourierPrime-Bold.ttf)
      Copyright 2015 The Courier Prime Project Authors
      (https://github.com/quoteunquoteapps/CourierPrime)
      SIL Open Font License 1.1  ->  OFL.txt

  Terminess Nerd Font Mono Regular + Bold  --  UI typeface, the default
      (TerminessNerdFontMono-Regular.ttf, TerminessNerdFontMono-Bold.ttf)
      Copyright (C) 2020 Dimitar Toshkov Zhekov
      Copyright (C) 2023 Tilman Blumenbach
      SIL Open Font License 1.1  ->  font-licenses/Terminess-OFL-1.1.txt

  JetBrains Mono Regular + Bold  --  UI typeface
      (JetBrainsMono-Regular.ttf, JetBrainsMono-Bold.ttf)
      Copyright 2020 The JetBrains Mono Project Authors
      (https://github.com/JetBrains/JetBrainsMono)
      SIL Open Font License 1.1  ->  font-licenses/JetBrainsMono-OFL-1.1.txt

The remaining four arrive with eframe/egui's `default_fonts` feature (crate
epaint_default_fonts; its exact version is in the crate list below) and act
as glyph fallback. Their license texts ship in the font-licenses/ folder of
every artifact:

  Ubuntu Light
      Copyright 2011 Canonical Ltd.
      Ubuntu Font Licence 1.0  ->  font-licenses/Ubuntu-Font-Licence-1.0.txt

  Noto Emoji Regular
      Copyright 2013 Google Inc. All Rights Reserved.
      SIL Open Font License 1.1  ->  font-licenses/NotoEmoji-OFL-1.1.txt

  Hack Regular
      Copyright (c) 2018 Source Foundry Authors  --  MIT License
      Copyright (c) 2003 Bitstream, Inc.  --  Bitstream Vera License
      ->  font-licenses/Hack-LICENSE.txt

  emoji-icon-font
      Copyright (c) 2014 John Slegers  --  MIT License
      ->  font-licenses/emoji-icon-font-MIT.txt

Tangent's own MIT grant (LICENSE) covers Tangent's source and compiled code only.
The fonts above are not relicensed by it and remain under their own terms.

Bundled video encoder (Linux and Windows artifacts)
---------------------------------------------------
The Linux and Windows artifacts ship an unmodified ffmpeg build as
`tangent-ffmpeg` (`tangent-ffmpeg.exe`), invoked by Tangent as a separate
process to encode a take's video. In the Windows release artifacts it
carries Tangent's Authenticode signature, which appends a certificate table
and updates the PE checksum and certificate-table entry; the program code is
still BtbN's build byte for byte. ffmpeg is a trademark of Fabrice Bellard;
these builds are GPL-licensed and their full licence text plus a provenance
note (exact upstream URL and checksum) ship beside the binary in the
ffmpeg-licenses/ folder of those artifacts. Source code for ffmpeg is
available from https://ffmpeg.org/. Because it runs as a separate program,
its GPL terms apply to that binary and do not extend to Tangent itself
(see LICENSING.md for the same reasoning applied to the VST3 plugin).
The macOS artifact bundles no encoder: it uses AVFoundation, part of macOS.

Rust crates
-----------
The Rust crates statically linked into Tangent are listed below with their
declared licenses and authors, as resolved from Cargo.lock (all platforms).
Generated by scripts/gen-third-party-licenses.sh: regenerate after any
dependency change.

EOF

# **The guard that would have caught JetBrains Mono.** Every font file
# tangent-ui/src/fonts.rs embeds via include_bytes! must be named, verbatim by
# file name, in the Fonts section above. The count matters too: the prose
# says TEN (six embedded + four from epaint_default_fonts), and a seventh
# embedded font must update both the text and the expected count here — this
# check cannot know what the new font's licence is, only that somebody has to
# say.
FONTS_RS="tangent-ui/src/fonts.rs"
EMBEDDED="$(grep -oE 'assets/fonts/[A-Za-z0-9._-]+\.(ttf|otf)' "$FONTS_RS" | sed 's#.*/##' | sort -u)"
GUARD_FAIL=0
for f in $EMBEDDED; do
  if ! grep -qF "$f" "$WIP"; then
    echo "!! $FONTS_RS embeds $f but the Fonts section in this script's heredoc" >&2
    echo "   never mentions it — add the font's copyright, licence and licence" >&2
    echo "   file, then rerun. A font that ships unlisted is a licence" >&2
    echo "   violation nobody can see." >&2
    GUARD_FAIL=1
  fi
done
EMBED_N="$(printf '%s\n' "$EMBEDDED" | grep -c . || true)"
if [ "$EMBED_N" != 6 ]; then
  echo "!! $FONTS_RS embeds $EMBED_N font files but the Fonts section says six" >&2
  echo "   (plus four from epaint_default_fonts = ten). Update the prose and" >&2
  echo "   the expected count in this check together." >&2
  GUARD_FAIL=1
fi
[ "$GUARD_FAIL" = 0 ] || exit 1

# --all-features, because the artifacts do not ship default features: the ASIO
# backend (asio-sys and its whole bindgen build chain) rides into the Windows
# build whenever the Steinberg SDK is present, and a plain resolve leaves all
# sixteen of those crates out of this file. Cargo.lock is the union across
# every feature and platform, and that union is what this file claims to list.
cargo metadata --format-version 1 --locked --all-features > "$TMP/metadata.json"
python3 - "$TMP/metadata.json" >> "$WIP" <<'PY'
import json, sys
with open(sys.argv[1]) as f:
    meta = json.load(f)


def first_party(p):
    # Ours = lives in this repo and is NOT a vendored copy. Filtering on
    # cargo's workspace_members instead sounds cleaner and is wrong: the
    # patched vendor/ copies of third-party crates are still third-party
    # code under third-party licences (cargo even counts vendored linuxvideo
    # as a member, which silently dropped it from this file), while anything
    # first-party that is ever added to the workspace is picked up by this
    # rule with no list to maintain.
    return p["id"].startswith("path+file://") and "/vendor/" not in p["id"]


pkgs = sorted((p for p in meta["packages"] if not first_party(p)),
              key=lambda p: (p["name"].lower(), p["version"]))
for p in pkgs:
    lic = p.get("license") or (
        f'see {p["license_file"]}' if p.get("license_file") else "unknown")
    authors = ", ".join(p.get("authors") or []) or "unknown"
    print(f'{p["name"]} {p["version"]}')
    print(f'    license: {lic}')
    print(f'    authors: {authors}')
    if p.get("repository"):
        print(f'    repository: {p["repository"]}')
    print()
print(f"({len(pkgs)} crates)")
PY

mv -f "$WIP" "$OUT"
echo "==> $OUT ($(grep -c 'license:' "$OUT") crates listed)"
