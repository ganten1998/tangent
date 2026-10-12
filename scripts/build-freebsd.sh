#!/bin/sh
# build-freebsd.sh — the FreeBSD package and the two offline bundles.
#
#   scripts/build-freebsd.sh              build everything
#   scripts/build-freebsd.sh --no-build   package a binary that is already built
#   scripts/build-freebsd.sh --pkg-only    just the .pkg, no bundles, no fetching
#
# WHAT THIS PRODUCES, AND WHY THERE ARE TWO BUNDLES
#
#   dist/tangent-<v>-freebsd-<abi>.pkg              the package on its own
#   dist/tangent-<v>-freebsd-<abi>-desktop.tar.gz   + deps a desktop lacks
#   dist/tangent-<v>-freebsd-<abi>-complete.tar.gz  + every dependency at all
#
# The bare .pkg is for anyone with a network: `pkg install ./tangent-*.pkg`
# fetches what it needs from the FreeBSD repo and is the idiomatic answer.
#
# The bundles are for machines with no network, and there are two because the
# honest gap depends on what is already there. On a box that runs a desktop,
# Xorg and mesa are present and the only things genuinely missing are ALSA and
# LV2 — about sixty megabytes, nearly all of it LV2's. lilv depends on
# libsndfile, which drags in FLAC, Vorbis, Opus and mpg123 whether this host
# decodes those formats or not, and on the `lv2` port, which carries the spec
# bundles the plugins genuinely need at runtime and a Python toolchain they do
# not. Both are pkg's graph rather than a choice available here. On a box where
# somebody installed FreeBSD,
# added Xorg and nothing else, the closure is the whole graphics stack and runs
# to hundreds.
# Shipping only the large one punishes every normal user with a download they
# do not need; shipping only the small one fails the person this port exists
# for. Both, then, from one .pkg — they differ ONLY in which dependency
# archives ride along beside it.
#
# ABI: pkg derives it from this host and enforces it at install time, so this
# script builds for the FreeBSD major it runs on and says so in the filename. A
# FreeBSD 14 artifact needs a FreeBSD 14 build host; see docs/RELEASE.md.
#
# POSIX sh, like the installer it ships.
set -eu

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

DO_BUILD=1
BUNDLES=1
for arg in "$@"; do
    case "$arg" in
        --no-build)  DO_BUILD=0 ;;
        --pkg-only)  BUNDLES=0 ;;
        *) echo "usage: $0 [--no-build] [--pkg-only]" >&2; exit 2 ;;
    esac
done

step() { printf '\n==> %s\n' "$*"; }
die()  { printf 'error: %s\n' "$*" >&2; exit 1; }

# `du` reports blocks ALLOCATED, and this repository is built on ZFS with
# compression on — so du called it a 5.7M package that `ls` called 7.9M, and a
# freshly-written tarball "512B" because nothing had been flushed yet. What a
# user downloads is the logical size, so ask for that.
size() {
    b="$(stat -f %z "$1" 2>/dev/null || echo 0)"
    awk -v b="$b" 'BEGIN{
        split("B KB MB GB", u, " ");
        i = 1; while (b >= 1024 && i < 4) { b /= 1024; i++ }
        printf (i == 1 ? "%d%s" : "%.1f%s"), b, u[i]
    }'
}

[ "$(uname -s)" = FreeBSD ] || die "this builds a FreeBSD package and must run on FreeBSD"

# ── Version ──────────────────────────────────────────────────────────────────
#
# Cargo says 5.0.0-beta.9; pkg cannot. A hyphen is what pkg uses to separate a
# package NAME from its VERSION in a filename, so a hyphen inside the version
# makes `tangent-5.0.0-beta.9.pkg` ambiguous and pkg reads the name as
# "tangent-5.0.0". FreeBSD's own convention for a prerelease is a `.bN` suffix,
# which also sorts BEFORE the release the way a beta should: 5.0.0.b9 < 5.0.0.
CARGO_VERSION="$(grep -m1 '^version' Cargo.toml | sed -E 's/.*"([^"]+)".*/\1/')"
PKG_VERSION="$(printf '%s' "$CARGO_VERSION" \
    | sed -E 's/-beta\.?/.b/; s/-alpha\.?/.a/; s/-rc\.?/.r/; s/-/./g')"
ABI="$(pkg config abi 2>/dev/null || echo unknown)"
ABI_TAG="$(printf '%s' "$ABI" | tr ':' '-')"

step "Tangent $CARGO_VERSION -> pkg version $PKG_VERSION, abi $ABI"

WORK="$ROOT/target/freebsd-pkg"
# The staging tree is a fake filesystem ROOT, not a prefix: pkg joins the
# manifest's `prefix` to each plist path and looks for the result under -r. So
# `bin/tangent` in the plist is `$STAGE/usr/local/bin/tangent` on disk, and a
# tree staged at the prefix instead produces three "Unable to access file"
# lines and nothing else.
STAGE="$WORK/stage"
PREFIXDIR="$STAGE/usr/local"
META="$WORK/meta"
DIST="$ROOT/dist"
# **Removed with the same privilege the bundles were fetched with.** `pkg fetch`
# needs root even with `-o`, so the previous run's fetch directories are owned by
# root and a plain `rm -rf` fails on them — which made running this script twice
# in a row an error rather than a rebuild. Only escalating when there is
# something root-owned to remove keeps the ordinary --pkg-only path from ever
# asking for a password.
if [ -d "$WORK" ] && ! rm -rf "$WORK" 2>/dev/null; then
    if [ "$(id -u)" = 0 ]; then rm -rf "$WORK"
    elif command -v doas >/dev/null 2>&1; then doas rm -rf "$WORK"
    elif command -v sudo >/dev/null 2>&1; then sudo rm -rf "$WORK"
    fi
    [ -d "$WORK" ] && {
        echo "could not remove $WORK" >&2
        echo "It holds root-owned files from an earlier bundle build (pkg fetch" >&2
        echo "needs root). Remove it as root and run this again:" >&2
        echo "    sudo rm -rf $WORK" >&2
        exit 1
    }
fi
mkdir -p "$PREFIXDIR/bin" \
         "$PREFIXDIR/share/applications" \
         "$PREFIXDIR/share/icons/hicolor/128x128/apps" \
         "$META" "$DIST"

# ── The binary ───────────────────────────────────────────────────────────────
if [ "$DO_BUILD" = 1 ]; then
    step "Building the release binary"
    cargo build --release -p tangent --features recorder
fi
BIN="$ROOT/target/release/tangent"
[ -x "$BIN" ] || die "no $BIN — run without --no-build"

# A debug build in a release package is a 400 MB surprise. The release profile
# already sets strip=true; this catches a binary built some other way.
case "$(file -b "$BIN")" in
    *"not stripped"*) step "Stripping $BIN"; strip "$BIN" ;;
esac

step "Staging"
install -m 0755 "$BIN" "$PREFIXDIR/bin/tangent"
# The low-latency path in one command. Everything measured about periods,
# block sizes and the uaudio tunable lives inside it, so the knowledge ships
# with the program rather than in a document.
install -m 0755 "$ROOT/installer/freebsd/tangent-jack" "$PREFIXDIR/bin/tangent-jack"
install -m 0644 "$ROOT/assets/tangent.desktop" \
                "$PREFIXDIR/share/applications/tangent.desktop"
# **The 128x128 directory gets a file that is actually 128x128.** It used to get
# `assets/tangent.png`, which was 128 px when that line was written and became a
# 2486x2476 wordmark when the artwork was swapped. hicolor takes the DIRECTORY as
# the truth: a desktop indexes it as 128, hands the theme a six-megapixel PNG for
# a taskbar slot, and the hairlines in it scale down to nothing anyway.
install -m 0644 "$ROOT/assets/tangent-128.png" \
                "$PREFIXDIR/share/icons/hicolor/128x128/apps/tangent.png"
# **The licence texts accompany every copy**, as every other artifact's
# already do (docs/RELEASE.md): the OFL requires it of the fonts embedded in
# the binary, and THIRD-PARTY-LICENSES is this project's own rule. pkg's
# convention for them is share/licenses/<name>/, and release.sh refuses a
# .pkg without them — this package shipped two releases with only LICENSE.
LIC="$PREFIXDIR/share/licenses/tangent"
mkdir -p "$LIC/font-licenses"
install -m 0644 "$ROOT/LICENSE"                "$LIC/LICENSE"
install -m 0644 "$ROOT/THIRD-PARTY-LICENSES"   "$LIC/THIRD-PARTY-LICENSES"
install -m 0644 "$ROOT/assets/fonts/OFL.txt"   "$LIC/OFL.txt"
for f in "$ROOT"/assets/font-licenses/*.txt; do
    install -m 0644 "$f" "$LIC/font-licenses/"
done
# LAME is statically linked here (the default `mp3` feature), so its LGPL
# texts and our notice go wherever the binary goes (LICENSING.md, LAME).
mkdir -p "$LIC/lame-licenses"
for f in "$ROOT"/assets/lame-licenses/*.txt; do
    install -m 0644 "$f" "$LIC/lame-licenses/"
done

# ── Manifest ─────────────────────────────────────────────────────────────────
sed "s/@@VERSION@@/$PKG_VERSION/" \
    "$ROOT/installer/freebsd/MANIFEST.in" > "$META/+MANIFEST"
cp "$ROOT/installer/freebsd/pkg-plist" "$META/pkg-plist"

step "Building the package"
# The -m/-p form, not -M: the plist takes paths relative to prefix, inherits
# modes from the staging tree and defaults ownership to root:wheel, where -M
# would mean enumerating every file with its attributes inside UCL.
#
# shlibs_required is NOT declared anywhere — pkg create scans the ELF binaries
# in the staging tree and computes it. Hand-maintaining that list is how it goes
# stale.
pkg create -m "$META" -p "$META/pkg-plist" -r "$STAGE" -o "$DIST" \
    || die "pkg create failed"

BUILT="$DIST/tangent-$PKG_VERSION.pkg"
[ -f "$BUILT" ] || die "pkg create did not produce $BUILT"
FINAL="$DIST/tangent-$PKG_VERSION-freebsd-$ABI_TAG.pkg"
mv "$BUILT" "$FINAL"
step "Package: $FINAL ($(size "$FINAL"))"

# A package that does not declare libasound is a package that will not start on
# the machine this whole port exists for, and it is the kind of thing that only
# shows up on a clean box. Assert it here instead.
step "Verifying the package declares what the binary needs"
MANIFEST_JSON="$(tar -xOf "$FINAL" +COMPACT_MANIFEST 2>/dev/null || true)"
# lilv joins them for the same reason and with the same consequence: the binary
# links liblilv-0.so outright, so a package that forgets it starts nowhere.
for want in alsa-lib alsa-plugins lilv; do
    case "$MANIFEST_JSON" in
        *"\"$want\""*) printf '    %-14s declared   ok\n' "$want" ;;
        *) die "the manifest does not declare $want" ;;
    esac
done
for lib in libasound.so.2 liblilv-0.so.0; do
    case "$MANIFEST_JSON" in
        *"$lib"*) printf '    %-14s detected   ok\n' "$lib" ;;
        *) die "pkg create did not detect $lib — is this the right binary?" ;;
    esac
done
# The build host is GhostBSD, which packages the base system. A dependency on
# one of those would produce something that installs only on GhostBSD.
case "$MANIFEST_JSON" in
    *GhostBSD-*) die "the manifest depends on a GhostBSD base package; it would not install on plain FreeBSD" ;;
    *) printf '    %-14s absent     ok\n' "GhostBSD deps" ;;
esac

[ "$BUNDLES" = 0 ] && { step "Done (--pkg-only)"; exit 0; }

# ── The dependency archives ──────────────────────────────────────────────────
#
# `pkg fetch -d` resolves the closure itself, which is the only way to keep
# these lists honest as ports move underneath us.
#
# DESKTOP is what a machine running a desktop does not already have. COMPLETE is
# every declared dependency, closure and all, for a machine that has only ever
# had Xorg installed.
#
# lilv is in BOTH, and in the desktop one for the same reason alsa-lib is: the
# binary links liblilv-0.so outright, so a machine without it does not start
# Tangent at all, and a desktop is no more likely to have an LV2 host library
# than a bare Xorg install is.
DESKTOP_DEPS="alsa-lib alsa-plugins lilv"
COMPLETE_DEPS="alsa-lib alsa-plugins lilv mesa-dri libglvnd libX11 libXcursor \
               libXi libXrandr libxkbcommon zenity xdg-utils desktop-file-utils \
               hicolor-icon-theme ffmpeg"

# pkg fetch writes to the package cache and wants root for it, even with -o.
FETCH="pkg"
if [ "$(id -u)" != 0 ]; then
    if command -v doas >/dev/null 2>&1; then FETCH="doas pkg"
    elif command -v sudo >/dev/null 2>&1; then FETCH="sudo pkg"
    else
        step "SKIPPING the bundles: fetching dependencies needs root"
        echo "    Re-run as root, or with doas/sudo available, to build them."
        exit 0
    fi
fi

bundle() {
    kind="$1"; shift
    deps="$*"
    dir="$WORK/bundle-$kind/tangent-$PKG_VERSION-freebsd-$ABI_TAG-$kind"
    step "Bundle '$kind': fetching dependencies"
    mkdir -p "$dir"
    # -d pulls the whole dependency closure, -y answers the prompt, -o puts the
    # archives somewhere we own instead of the shared cache.
    # shellcheck disable=SC2086
    $FETCH fetch -y -d -o "$WORK/fetch-$kind" $deps >/dev/null \
        || die "pkg fetch failed for the $kind bundle"
    # fetch lays them out under All/; the bundle wants them flat, beside the
    # app, because that is where `pkg add` looks for unmet dependencies.
    find "$WORK/fetch-$kind" -name '*.pkg' -exec cp {} "$dir/" \;
    cp "$FINAL" "$dir/"
    cp "$ROOT/installer/freebsd/install.sh" "$dir/install.sh"
    chmod 755 "$dir/install.sh"
    # The same licence set every other archive carries — see the staging
    # above for why, and release.sh for what refuses a bundle without them.
    cp "$ROOT/LICENSE" "$dir/LICENSE"
    cp "$ROOT/THIRD-PARTY-LICENSES" "$dir/THIRD-PARTY-LICENSES"
    cp "$ROOT/assets/fonts/OFL.txt" "$dir/OFL.txt"
    mkdir -p "$dir/font-licenses"
    cp "$ROOT"/assets/font-licenses/*.txt "$dir/font-licenses/"
    mkdir -p "$dir/lame-licenses"
    cp "$ROOT"/assets/lame-licenses/*.txt "$dir/lame-licenses/"
    write_readme "$kind" > "$dir/README.txt"

    out="$DIST/tangent-$PKG_VERSION-freebsd-$ABI_TAG-$kind.tar.gz"
    ( cd "$(dirname "$dir")" && tar -czf "$out" "$(basename "$dir")" )
    n="$(find "$dir" -name '*.pkg' | wc -l | tr -d ' ')"
    step "Bundle '$kind': $out ($(size "$out"), $n packages)"
}

write_readme() {
    cat <<EOF
Tangent $CARGO_VERSION for FreeBSD ($ABI)
=========================================

Tangent names what you play on a MIDI keyboard in real time, and records,
mixes and arranges it too.

INSTALL

    sh install.sh

That installs Tangent for everyone on the machine and asks for root when it
needs it. Everything Tangent depends on is in this folder, so it works with no
network connection.

To install for just yourself, with no root at all:

    sh install.sh --home

To remove it:

    sh install.sh --uninstall

WHAT IS IN HERE

    tangent-*.pkg     Tangent itself
    *.pkg             its dependencies -- $( [ "$1" = desktop ] &&
        printf 'the pieces a FreeBSD desktop does not\n                      usually have. If Tangent will not start on a machine\n                      with no Xorg or mesa, use the "complete" download.' ||
        printf 'every dependency, so this works on a\n                      machine that has never had a desktop package installed.' )
    install.sh        the installer
    README.txt        this
    LICENSE, OFL.txt, THIRD-PARTY-LICENSES, font-licenses/, lame-licenses/
                      the licences: Tangent's, the embedded fonts', LAME's
                      (the MP3 encoder linked into it), and everything else
                      the binary carries

AFTER INSTALLING

Sound plays through FreeBSD's own OSS by way of the alsa-lib shim, so there is
no sound server to start. If you hear nothing, check the mixer:

    mixer vol 80 pcm 90

Plug in a USB MIDI keyboard and Tangent finds it -- MIDI is read straight from
/dev/umidi, with no daemon in between.

Free, no telemetry, no account. MIT licensed. Tangent makes no network
connections except Stream to OBS, which is off until you turn it on and then
sends the master's audio over UDP to the host you give it (127.0.0.1, this
machine, unless you change it). A plugin you load runs inside Tangent but is
its maker's program, so what it connects to is up to its maker.
https://ganten.neocities.org/projects
EOF
}

bundle desktop $DESKTOP_DEPS
bundle complete $COMPLETE_DEPS

step "Done"
ls -lh "$DIST"/tangent-"$PKG_VERSION"-freebsd-* | awk '{printf "    %-8s %s\n", $5, $NF}'
