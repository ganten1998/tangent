#!/usr/bin/env bash
# Fetch the video encoder Tangent BUNDLES, so the artifact works on a machine
# with nothing installed.
#
#   scripts/fetch-ffmpeg.sh linux     -> dist/vendor/linux/tangent-ffmpeg
#   scripts/fetch-ffmpeg.sh windows   -> dist/vendor/windows/tangent-ffmpeg.exe
#
# macOS needs nothing here: its encoder is AVFoundation, in the OS.
#
# WHY BUNDLED AT ALL. The ffmpeg backend spawns an external `ffmpeg`, and
# "install it with your package manager" is exactly the step the packaging
# requirement says a user must never need. The app looks for `tangent-ffmpeg`
# beside its own binary before falling back to PATH — see `program()` in
# tangent-record/src/encode/ffmpeg.rs, including why the name is prefixed.
#
# PINNED BY CHECKSUM, CACHED IN dist/vendor. A moving "latest" URL is a supply
# chain risk and an unreproducible build in one; these are exact versioned
# files with their sha256 recorded here. Bumping the version means changing
# the URL AND the hash in the same edit, on purpose. The downloads are cached,
# so packaging is offline after the first run.
#
# LICENSING. Both builds are GPL ffmpeg. Tangent invokes the binary as a
# separate process, which keeps the app MIT (LICENSING.md's reasoning for the
# GPL plugin applies in reverse). The build's own licence text ships beside it
# in the artifact, and gen-third-party-licenses.sh names the bundle.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

# ffmpeg 7.0.2, static, x86_64 glibc — johnvansickle.com, the long-standing
# static-build source the ffmpeg project itself links to. When this release
# rotates out, the same file appears under /ffmpeg/old-releases/.
#
# **THIS BUILD HAS NO VA-API IN IT**, and that is a known, measured cost rather
# than an oversight: no `--enable-vaapi` in its configure line, no `h264_vaapi`
# in the binary. Those builds are made for maximum portability and leave out
# anything needing a runtime library. So a Linux machine with a perfectly good
# hardware encoder encodes takes on the CPU unless the user installs a system
# ffmpeg — 25.54 s of CPU per 10 s of 720p against VA-API's 0.70, measured.
#
# **BtbN's linux64 build was tried instead and had to be reverted**, and the
# reason is worth keeping because it is invisible from any static check.
# BtbN links libva through `implib.so` trampolines rather than a plain dlopen,
# and a trampoline whose symbol will not resolve does not degrade — it runs
# `assert(0)` and the process ABORTS. Their build wants `vaMapBuffer2`, which
# arrived in libva 2.22; Ubuntu 24.04 and every derivative of it ship 2.20.
# Measured on Zorin OS 18.1: `exit=134`, SIGABRT, a zero-byte file, mid-encode.
#
# That is strictly worse than encoding on the CPU. A silent software fallback
# costs processor; an aborting encoder costs the take. Anything that replaces
# this pin has to be tested against an OLD libva on a real machine, not read.
LINUX_FILE="ffmpeg-7.0.2-amd64-static.tar.xz"
LINUX_URLS="https://johnvansickle.com/ffmpeg/releases/$LINUX_FILE
https://johnvansickle.com/ffmpeg/old-releases/$LINUX_FILE"
LINUX_SHA="abda8d77ce8309141f83ab8edf0596834087c52467f6badf376a6a2a4c87cf67"

# The same release for aarch64, because until now there was NOTHING: the arm64
# tarball shipped no encoder at all, so video silently depended on a distro
# ffmpeg that a clean machine does not have — the exact hole the x86_64 tarball
# fixed in 4.11 (docs/LINUX-4.11-FINDINGS.md, finding 2), still open on the
# other architecture. Same source, same licence, same caveats as above.
ARM64_FILE="ffmpeg-7.0.2-arm64-static.tar.xz"
ARM64_URLS="https://johnvansickle.com/ffmpeg/releases/$ARM64_FILE
https://johnvansickle.com/ffmpeg/old-releases/$ARM64_FILE"
ARM64_SHA="f4149bb2b0784e30e99bdda85471c9b5930d3402014e934a5098b41d0f7201b1"

# ffmpeg 8.1.3, win64, BtbN's autobuilds — a dated tag, never the rolling
# `latest` release, whose assets are rebuilt in place. The clean n8.1.3 tag
# rather than a git-describe build between releases.
#
# **Dated is not permanent.** BtbN prunes its dated releases after a few
# weeks: the 8.1.2 pin this replaced (autobuild-2026-08-17-13-05) 404ed on
# the TAG by 2026-09-22, and every clean Windows build died here after its
# `cargo xwin build` had already succeeded. So the archive is also kept in
# the durable cache below. **On the day you bump this pin**: change WIN_FILE,
# WIN_URLS and WIN_SHA in one edit, run `scripts/fetch-ffmpeg.sh windows` once
# (`keep` copies the verified zip into `vendor/ffmpeg/`), and archive
# `BtbN/FFmpeg-Builds` at the tag for the source release (LICENSING.md, W31).
# BtbN prunes dated tags within weeks; after that a copy of the verified
# archive (`vendor/ffmpeg/` in this Dropbox-synced folder, or `dist/vendor/`)
# is the only thing that builds Windows, and a fresh clone without one needs a
# new pin, i.e. a different encoder build.
WIN_FILE="ffmpeg-n8.1.3-win64-gpl-8.1.zip"
WIN_URLS="https://github.com/BtbN/FFmpeg-Builds/releases/download/autobuild-2026-09-22-13-18/$WIN_FILE"
WIN_SHA="ed885833e406f0f7a1ea0304dd170779117799b2fba8096abc3e2e4dd679cc20"

PLATFORM="${1:-}"
case "$PLATFORM" in
    linux|linux-arm64|windows) ;;
    *) echo "usage: scripts/fetch-ffmpeg.sh <linux|linux-arm64|windows>" >&2; exit 2 ;;
esac

# macOS (where the Windows artifact is packaged) ships `shasum`, Linux ships
# `sha256sum`; this is the one place the difference matters.
sha_ok() { # sha_ok <file> <sha256>
    local got
    if command -v sha256sum >/dev/null 2>&1; then
        got="$(sha256sum "$1" | awk '{print $1}')"
    else
        got="$(shasum -a 256 "$1" | awk '{print $1}')"
    fi
    [ "$got" = "$2" ]
}

# **The durable cache: `vendor/ffmpeg/`, beside the ASIO SDK.** `dist` is
# meant to be a symlink out of Dropbox, to `~/Library/Caches/tangent-dist` (see
# `publish-dropbox.sh`). `rm -rf dist` deletes the LINK, not the cache behind
# it, and the next `mkdir -p dist` makes an empty directory inside the
# Dropbox-synced repo; `dist/vendor` then starts empty and every archive is
# fetched again, which is the road on which the Windows leg met its pruned
# upstream on 2026-09-22. Clean with `rm -rf dist/*`, never `rm -rf dist`.
# `/vendor/` is gitignored (the ASIO licence forbids committing the SDK next
# to it), so nothing is redistributed by keeping the archives here; it is
# inside the Dropbox folder, so it is synced, and an archive Dropbox has made
# online-only is read back over the network.
CACHE="vendor/ffmpeg"

fetch() { # fetch <file> <sha256> <url...>
    local file="$1" sha="$2" url
    shift 2
    if [ -f "dist/vendor/$file" ] && sha_ok "dist/vendor/$file" "$sha"; then
        keep "$file" "$sha"
        return 0
    fi
    mkdir -p dist/vendor
    if [ -f "$CACHE/$file" ] && sha_ok "$CACHE/$file" "$sha"; then
        cp "$CACHE/$file" "dist/vendor/$file"
        echo "==> $file from $CACHE"
        return 0
    fi
    for url in "$@"; do
        echo "==> fetching $url"
        if curl -fL --retry 3 -o "dist/vendor/$file.part" "$url"; then
            mv "dist/vendor/$file.part" "dist/vendor/$file"
            break
        fi
    done
    sha_ok "dist/vendor/$file" "$sha" || {
        echo "FAIL: dist/vendor/$file does not match its pinned sha256" >&2
        echo "      (a new upstream build needs a deliberate URL+hash bump here)" >&2
        rm -f "dist/vendor/$file"
        exit 1
    }
    keep "$file" "$sha"
}

# keep <file> <sha256>: a verified archive, copied into the durable cache.
#
# Replaces a cached file of the SAME NAME that does not verify: BtbN names its
# archives by tag alone, so a later dated build of n8.1.3 is the same file
# name with different bytes, and a cache that only asked "is it there" kept
# the old one forever after a pin bump. And never fatal: the cache is a
# backup, and a build that has its archive in dist/ must not fail because the
# backup could not be written.
keep() {
    if [ -f "$CACHE/$1" ] && sha_ok "$CACHE/$1" "$2"; then
        return 0
    fi
    if mkdir -p "$CACHE" && cp "dist/vendor/$1" "$CACHE/$1.part" && mv "$CACHE/$1.part" "$CACHE/$1"; then
        return 0
    fi
    rm -f "$CACHE/$1.part"
    echo "warning: could not keep $1 in $CACHE (the build goes on without the backup)" >&2
    return 0
}

provenance() { # provenance <dir> <url> <sha>
    cat > "$1/SOURCE.txt" <<EOF
This ffmpeg build is bundled unmodified. Tangent invokes it as a separate
process; it is GPL-licensed (see the licence text beside this file) and its
source code is available from https://ffmpeg.org/ and from the build's page:

  $2
  sha256 of the downloaded archive: $3
EOF
}

case "$PLATFORM" in
linux)
    # Word-splitting the URL list is deliberate; the URLs contain no spaces.
    # shellcheck disable=SC2086
    fetch "$LINUX_FILE" "$LINUX_SHA" $LINUX_URLS
    OUT="dist/vendor/linux"
    rm -rf "$OUT"; mkdir -p "$OUT/ffmpeg-licenses"
    tar -xJf "dist/vendor/$LINUX_FILE" -C "$OUT" --strip-components=1 \
        "${LINUX_FILE%.tar.xz}/ffmpeg" "${LINUX_FILE%.tar.xz}/GPLv3.txt"
    mv "$OUT/ffmpeg" "$OUT/tangent-ffmpeg"
    chmod 755 "$OUT/tangent-ffmpeg"
    mv "$OUT/GPLv3.txt" "$OUT/ffmpeg-licenses/GPLv3.txt"
    provenance "$OUT/ffmpeg-licenses" "${LINUX_URLS%%$'\n'*}" "$LINUX_SHA"
    echo "==> $OUT/tangent-ffmpeg"
    ;;
linux-arm64)
    # shellcheck disable=SC2086
    fetch "$ARM64_FILE" "$ARM64_SHA" $ARM64_URLS
    OUT="dist/vendor/linux-arm64"
    rm -rf "$OUT"; mkdir -p "$OUT/ffmpeg-licenses"
    tar -xJf "dist/vendor/$ARM64_FILE" -C "$OUT" --strip-components=1 \
        "${ARM64_FILE%.tar.xz}/ffmpeg" "${ARM64_FILE%.tar.xz}/GPLv3.txt"
    mv "$OUT/ffmpeg" "$OUT/tangent-ffmpeg"
    chmod 755 "$OUT/tangent-ffmpeg"
    mv "$OUT/GPLv3.txt" "$OUT/ffmpeg-licenses/GPLv3.txt"
    provenance "$OUT/ffmpeg-licenses" "${ARM64_URLS%%$'\n'*}" "$ARM64_SHA"
    echo "==> $OUT/tangent-ffmpeg"
    ;;
windows)
    # shellcheck disable=SC2086
    fetch "$WIN_FILE" "$WIN_SHA" $WIN_URLS
    OUT="dist/vendor/windows"
    rm -rf "$OUT"; mkdir -p "$OUT/ffmpeg-licenses"
    unzip -j -q "dist/vendor/$WIN_FILE" \
        "${WIN_FILE%.zip}/bin/ffmpeg.exe" "${WIN_FILE%.zip}/LICENSE.txt" -d "$OUT"
    mv "$OUT/ffmpeg.exe" "$OUT/tangent-ffmpeg.exe"
    mv "$OUT/LICENSE.txt" "$OUT/ffmpeg-licenses/LICENSE.txt"
    provenance "$OUT/ffmpeg-licenses" "$WIN_URLS" "$WIN_SHA"
    echo "==> $OUT/tangent-ffmpeg.exe"
    ;;
esac
