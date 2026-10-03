#!/usr/bin/env bash
# Cross-build Tangent release artifacts for Linux and Windows from a macOS host.
#
#   scripts/build-cross.sh        # builds + packages everything into dist/
#   scripts/build-cross.sh ico    # regenerate assets/tangent.ico only (dry-test hook)
#   scripts/build-cross.sh check-linux   # TYPE-CHECK the Linux target from macOS
#
# Requirements (all via Homebrew/cargo):
#   brew install zig
#   cargo install cargo-zigbuild cargo-xwin
#   rustup target add x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu x86_64-pc-windows-msvc
#
# Note: if Homebrew's rust is also installed, its cargo/rustc shadow rustup's
# on PATH and cross-target std libs won't be found — hence the PATH prefix.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

VERSION="$(grep '^version' Cargo.toml | head -1 | sed -E 's/.*"([^"]+)".*/\1/')"
# The glibc floor for released Linux binaries, and it is 2.30 because of ONE
# symbol: `gettid`, which glibc grew in 2.30 and std's thread plumbing calls.
# The floor LOOKED like 2.32 for every 5.0 beta — `pthread_getattr_np` showed
# up versioned GLIBC_2.32 in the shipped binaries — but that was the pin
# talking to itself: the function has existed since glibc 2.2.5 in libpthread,
# 2.32 merely moved it into libc under a new version, and targeting 2.32 makes
# zig bind the new home. Target 2.30 and it binds the ancient libpthread one
# (proven by objdump on both bindings, 2026-08-31), so the requirement
# vanishes — which buys Ubuntu 20.04, Debian 11 and openSUSE Leap 15.4/15.5,
# all glibc 2.31, all refused by one digit. Do not chase 2.29 without a real
# 2.29 box: gettid is a weak ref, but weak is not the same as optional.
# The floor is ENFORCED at package time — check-linux-deps.sh receives it as
# GLIBC_FLOOR below and refuses any binary whose version refs exceed it — so
# raising it is a deliberate act that fails loudly, not a drift. Raise the
# user-facing claims (README.md, docs/RELEASE.md, docs/ARTIFACT-README.md)
# in the same commit.
GLIBC="2.30"
ICON_SRC="assets/tangent.png"

TOOLCHAIN_BIN="$(rustup which cargo 2>/dev/null | xargs dirname || true)"
[ -n "$TOOLCHAIN_BIN" ] && export PATH="$TOOLCHAIN_BIN:$HOME/.cargo/bin:$PATH"

# check_linux — compile the Linux target from macOS WITHOUT an ALSA sysroot.
#
# Linux release binaries still cannot be cross-built here (see the note above
# package_linux), but a release build is not what is usually wanted: what is
# wanted is to know whether the Linux target still COMPILES before pushing, and
# for that the answer is yes.
#
# `cargo check` never links, so libasound.so does not have to exist — only
# `pkg-config --exists alsa` has to succeed, because that is all alsa-sys's
# build script tests. A three-line stub .pc file satisfies it.
#
# ALSA_NO_PKG_CONFIG does NOT work for this: alsa-sys panics on it deliberately
# ("Aborted because ALSA_NO_PKG_CONFIG is set", build.rs:13).
#
# **And a stub for LILV, for the same reason and the same way.** The moment
# LV2 arrived this function stopped working: `lilv 0.2.4`'s build script asks
# pkg-config for `lilv-0 >= 0.24.2` and panics when it is absent, so
# `check-linux` died in a build script before it type-checked a single line —
# the same class of silent loss the `--features` note below records, and the
# second time this one check has been broken without anybody hearing about
# it. `--version` is what the `>=` is tested against, so the stub claims a
# version that satisfies the floor; `Requires:` is deliberately EMPTY, since
# sord, sratom and lv2 only matter to a link that never happens here. SERD is
# the exception and gets a stub of its own: `lilv/build.rs` probes it as a
# SECOND package in its own right (`atleast_version("0.30.0")`), so an empty
# `Requires:` does not hide it.
#
# Without this, Rust code that is Linux-only — the `#[cfg(unix)]` statvfs
# branch, the non-macOS camera stub — is never type-checked on this machine at
# all, and the first thing that finds a mistake in it is the Linux box.
check_linux() {
  local target="${1:-x86_64-unknown-linux-gnu}" pcdir
  pcdir="$(mktemp -d)"
  cat > "$pcdir/alsa.pc" <<'PC'
prefix=/usr
libdir=${prefix}/lib
includedir=${prefix}/include

Name: alsa
Description: stub for cross TYPE-CHECKING only; never linked against
Version: 1.2.11
Libs: -L${libdir} -lasound
Cflags: -I${includedir}
PC
  cat > "$pcdir/lilv-0.pc" <<'PC'
prefix=/usr
libdir=${prefix}/lib
includedir=${prefix}/include/lilv-0

Name: lilv
Description: stub for cross TYPE-CHECKING only; never linked against
Version: 0.24.26
Libs: -L${libdir} -llilv-0
Cflags: -I${includedir}
PC
  cat > "$pcdir/serd-0.pc" <<'PC'
prefix=/usr
libdir=${prefix}/lib
includedir=${prefix}/include/serd-0

Name: serd
Description: stub for cross TYPE-CHECKING only; never linked against
Version: 0.32.4
Libs: -L${libdir} -lserd-0
Cflags: -I${includedir}
PC
  echo "==> type-checking $target (stub alsa/lilv/serd .pc, no sysroot)"
  # **The library targets only.** `--all-targets` was tried and cannot work
  # from here: it drags `alsa-sys`'s build script into a HOST build, where
  # there is no ALSA and the stub .pc above does not apply. The Linux-only
  # tests are checked on the Linux box instead.
  #
  # **`--no-default-features --features tangent/recorder`, exactly as
  # `package_linux` builds.** Without it the default `mp3` feature drags
  # `mp3lame-encoder` into a CROSS build, and LAME's autoconf decides its
  # toolchain by compiling and RUNNING test programs — which a macOS host
  # cannot do for a Linux target. `check-linux` therefore exited 101 on every
  # invocation, which means the only pre-release type check for the Linux legs
  # has not run for as long as the flag has been missing. The packaged build
  # was always right; only the check was wrong, so nothing shipped broken —
  # but nothing was being checked either.
  #
  # Nothing may go between the backslashes below. A comment there ends the
  # continuation, the environment never reaches `cargo`, and the failure is
  # alsa-sys panicking about pkg-config — which reads as a broken toolchain
  # rather than as a broken line.
  PKG_CONFIG_ALLOW_CROSS=1 \
  PKG_CONFIG_PATH="$pcdir" \
  PKG_CONFIG_SYSROOT_DIR=/ \
    cargo check --workspace --no-default-features --features tangent/recorder \
      --target "$target"
  local rc=$?
  rm -rf "$pcdir"
  return $rc
}

if [ "${1:-}" = "check-linux" ]; then
  check_linux "${2:-x86_64-unknown-linux-gnu}"
  exit $?
fi

# gen_ico — produce assets/tangent.ico from assets/tangent.png so tangent/build.rs
# (winres) can embed it in tangent.exe. ImageMagick when available (BMP frames
# for small sizes = maximum shell compatibility), else sips + a hand-packed
# ICO of PNG frames (valid on Windows Vista+).
gen_ico() {
  local out="assets/tangent.ico" tmp px
  # The square, full-bleed cut of the Tonnetz wordmark — Windows and Linux
  # want the whole tile, where macOS wants the squircle (see build-macos.sh).
  if [ "$(stat -f%z "$ICON_SRC")" -lt 4096 ]; then
    echo "NOTE: $ICON_SRC is small; frames above its own size are upscaled"
    echo "      and will look soft. Supply the artwork at 256px or more."
  fi
  # **Small frames from THICKENED strokes, large ones from the art itself.**
  #
  # Same reason the macOS iconset does it (see `make_icns`): the wordmark is a
  # hairline lattice and a hairline does not survive being resampled to 16 px
  # — a taskbar got a black square with a ghost in it. `auto-resize` renders
  # every frame from one source, so the frames are built separately and
  # combined, which is the only way to hint the small ones differently.
  # **Purpose-drawn small frames beat any amount of thickening.** The
  # committed `assets/icon-small-square-{16,32,48}.png` are the bold-dot
  # cut of the mark (letters and rays dropped), drawn once from
  # `assets/icon-small-square.svg`; the dilate fallback only runs for a
  # frame whose file is missing.
  if command -v magick >/dev/null 2>&1; then
    local w r small
    w="$(magick identify -format '%w' "$ICON_SRC" 2>/dev/null || echo 0)"
    r=$(( w / 150 )); [ "$r" -lt 1 ] && r=1
    tmp="$(mktemp -d)"
    small="$tmp/small.png"
    if magick "$ICON_SRC" -channel RGB -morphology Dilate "Disk:$r" +channel \
         -modulate 112 "$small" 2>/dev/null; then
      local frames="" fpx fsrc
      for fpx in 256 128 64 48 32 16; do
        if [ "$fpx" -le 48 ] && [ -f "assets/icon-small-square-$fpx.png" ]; then
          cp "assets/icon-small-square-$fpx.png" "$tmp/$fpx.png"
        else
          fsrc="$ICON_SRC"; [ "$fpx" -le 48 ] && fsrc="$small"
          magick "$fsrc" -resize "${fpx}x${fpx}" "$tmp/$fpx.png" || { rm -rf "$tmp"; return 1; }
        fi
        frames="$frames $tmp/$fpx.png"
      done
      # shellcheck disable=SC2086
      magick $frames "$out" || { rm -rf "$tmp"; return 1; }
      rm -rf "$tmp"
    else
      rm -rf "$tmp"
      magick "$ICON_SRC" -define icon:auto-resize=256,128,64,48,32,16 "$out"
    fi
  elif command -v convert >/dev/null 2>&1; then
    convert "$ICON_SRC" -define icon:auto-resize=256,128,64,48,32,16 "$out"
  else
    tmp="$(mktemp -d)"
    for px in 16 24 32 48 64 128 256; do
      sips -z "$px" "$px" -s format png --out "$tmp/$px.png" "$ICON_SRC" >/dev/null
    done
    /usr/bin/python3 - "$tmp" "$out" <<'PY'
import os, struct, sys
src, out = sys.argv[1], sys.argv[2]
sizes = [16, 24, 32, 48, 64, 128, 256]
entries, body = [], b''
offset = 6 + 16 * len(sizes)
for px in sizes:
    with open(os.path.join(src, f'{px}.png'), 'rb') as f:
        data = f.read()
    b = 0 if px == 256 else px  # 0 means 256 in ICONDIRENTRY
    entries.append(struct.pack('<BBBBHHII', b, b, 0, 0, 1, 32, len(data), offset))
    offset += len(data)
    body += data
with open(out, 'wb') as f:
    f.write(struct.pack('<HHH', 0, 1, len(sizes)) + b''.join(entries) + body)
print(f"  ico: {len(sizes)} frames -> {out}")
PY
    rm -rf "$tmp"
  fi
  echo "==> $out"
}

# Dry-test hook: regenerate the .ico only (exactly the shipped code path).
if [ "${1:-}" = "ico" ]; then
  gen_ico
  exit 0
fi

# **No ASIO SDK, no Windows build, decided before anything is compiled.**
# The Steinberg SDK lives in vendor/asiosdk_* (its licence forbids committing
# it), so a fresh checkout does not have it, and this used to carry on to a
# WASAPI-only tangent.exe, saying so in one line nobody re-reads, after both
# Linux legs had spent their time. release.sh failed that build at the very
# end and a tester build shipped it. ASIO is the standard on Windows and the
# testers have interfaces (the owner's ruling, below): without it the build
# is not the product. ALLOW_WASAPI_ONLY=1 builds one on purpose, and the
# artifact checks (scripts/check-artifacts.sh) still keep it from shipping.
ASIO_DIR="$(ls -d "$ROOT"/vendor/asiosdk_* 2>/dev/null | head -1 || true)"
if [ -z "$ASIO_DIR" ] && [ "${ALLOW_WASAPI_ONLY:-0}" != 1 ]; then
  echo "error: no ASIO SDK under vendor/asiosdk_*: refusing a WASAPI-only Windows build." >&2
  echo "       Unpack Steinberg's ASIO SDK into vendor/ and rerun, or set" >&2
  echo "       ALLOW_WASAPI_ONLY=1 to build one anyway (nothing will ship it)." >&2
  exit 1
fi

mkdir -p dist

# License bundle must exist before packaging.
[ -f THIRD-PARTY-LICENSES ] || scripts/gen-third-party-licenses.sh

# Windows icon must exist BEFORE cargo builds so build.rs embeds it.
gen_ico

# **The build stamp**: the tree this run builds from, taken once the icon is
# regenerated and before anything compiles, and recorded beside each
# artifact as it is finished (scripts/build-stamp.sh). It is how
# publish-dropbox.sh tells this run's files from an earlier run's under the
# same names - a failed leg keeps the previous tarball, by design.
BUILD_TREE="$(scripts/build-stamp.sh tree 2>/dev/null || true)"
[ -n "$BUILD_TREE" ] \
  || echo "NOTE: no git tree to stamp with; publish-dropbox.sh will not take these artifacts" >&2
build_stamp() {
  [ -z "$BUILD_TREE" ] || scripts/build-stamp.sh write "$BUILD_TREE" "$@" \
    || echo "warning: could not stamp $*; publish-dropbox.sh will not take it" >&2
}

# Authenticode signing lives in scripts/sign-windows.sh (jsign to Azure
# Artifact Signing; unconfigured ships unsigned and says so, partially
# configured is a hard failure). It moved out of this file the day the NSIS
# setup.exe started being signed too: SmartScreen judges the OUTER installer,
# so the inner exe and the installer must take the SAME route, and one script
# with two callers is how they are kept from drifting apart.

# ensure_alsa_sysroot — the thing that made Linux cross-builds impossible here.
#
# midir and cpal both link ALSA, and alsa-sys's build script asks pkg-config for
# it. On a macOS host there is no ALSA for a Linux target, so every Linux stage
# of this script failed for the app's whole life and the artifacts were built on
# a Linux box instead. A sysroot fixes it: headers and a `libasound.so` to link
# against, which is all the linker wants — the real library comes from the
# user's own machine at run time.
#
# **Debian BULLSEYE, not bookworm, and that is the entire trick.** Bookworm's
# libasound is built against glibc 2.36 and its .so references `lstat64@2.33`,
# `dlsym@2.34` and friends; linking it while targeting the glibc floor this
# script sets fails with a page of undefined references that look like a broken
# toolchain and are actually a version skew. What matters is not the glibc a
# distro RUNS but the newest symbol version its .so files demand: bullseye
# ships glibc 2.31, yet its libasound/lilv stack references nothing past
# GLIBC_2.29 (measured on both arches, 2026-08-31), which is under the 2.30
# floor — so the sysroot survived the floor's move from 2.32 and would survive
# 2.30 exactly until somebody swaps the suite, which is what this comment is
# for.
#
# **And LILV, since LV2 arrived.** `lilv` is the reference LV2 host library and
# the crate of that name has a build script that asks pkg-config for `lilv-0`
# — so from the moment LV2 was hosted, a Linux cross build died in that build
# script, before the linker was ever reached. (The FreeBSD session's brief
# predicted a link-time missing `.so`; that is true of `lilv-sys`, which ships
# pre-generated bindings, but the safe wrapper in the graph is `lilv 0.2.4`
# and it does run pkg-config.) `lilv-0.pc` in turn requires serd, sord,
# sratom and the LV2 headers, so all four come too — a missing `Requires:`
# fails exactly as loudly as a missing library.
#
# Cached, because it is a megabyte a time and nobody wants a download in a
# release build. Delete the directory to refresh it.
ALSA_CACHE="${ALSA_CACHE:-$HOME/.cache/tangent/alsa}"
# The runtime halves carry the `.so.N` the linker follows; the `-dev` halves
# carry the headers, the bare `.so` symlink and the `.pc` files that make
# pkg-config answer at all. Both halves of each, or the build script finds a
# library it cannot describe.
SYSROOT_PKGS="libasound2 libasound2-dev \
libserd-0-0 libserd-dev libsord-0-0 libsord-dev \
libsratom-0-0 libsratom-dev liblilv-0-0 liblilv-dev lv2-dev"
ensure_alsa_sysroot() { # $1 = debian arch (amd64|arm64)
  local darch="$1" root="$ALSA_CACHE/bullseye-$1"
  # **The stamp, not the directory.** This asked whether `usr/include` existed,
  # which an ALSA-only sysroot cached before LV2 also answers yes to — so
  # adding lilv to the list would have been ignored on every machine that had
  # ever built Linux before, and the failure would have looked like the list
  # was wrong rather than stale. The stamp names what was actually fetched.
  if [ -f "$root/.tangent-sysroot" ] \
     && [ "$(cat "$root/.tangent-sysroot")" = "$SYSROOT_PKGS" ]; then
    echo "$root"; return 0
  fi
  rm -rf "$root"
  local idx tmp fn base
  tmp="$(mktemp -d)" || return 1
  idx="$tmp/Packages.gz"
  curl -sSL --max-time 180 \
    "http://deb.debian.org/debian/dists/bullseye/main/binary-$darch/Packages.gz" \
    -o "$idx" || { rm -rf "$tmp"; return 1; }
  mkdir -p "$root" || { rm -rf "$tmp"; return 1; }
  for pkg in $SYSROOT_PKGS; do
    fn="$(gunzip -c "$idx" | awk -v p="Package: $pkg" '''$0==p{f=1} f&&/^Filename: /{print $2; exit}''')"
    [ -n "$fn" ] || { rm -rf "$tmp" "$root"; return 1; }
    base="$tmp/$(basename "$fn")"
    curl -sSL --max-time 300 "http://deb.debian.org/debian/$fn" -o "$base" \
      || { rm -rf "$tmp" "$root"; return 1; }
    ( cd "$tmp" && ar x "$base" && tar -xf data.tar.* -C "$root" ) \
      || { rm -rf "$tmp" "$root"; return 1; }
  done
  rm -rf "$tmp"
  printf '%s' "$SYSROOT_PKGS" > "$root/.tangent-sysroot"
  echo "$root"
}

package_linux() { # $1 = rust target, $2 = artifact arch name
  local target="$1" arch="$2"
  local stage="dist/tangent-${VERSION}-linux-${arch}"
  # NOTE: this function is invoked as `package_linux ... || handler`, which
  # DISABLES `set -e` inside the whole body (bash: errexit is suppressed in a
  # command that is the left operand of ||). Every failure must therefore be
  # checked by hand — otherwise a failed cargo build sails on and tar cheerfully
  # ships an archive containing licences and fonts but no program. That is
  # exactly what happened before 2.1.0.
  # **NO `mp3`, for the same reason Windows has none.** `mp3lame-sys` builds
  # LAME from C through autotools, and autoconf cross-compiles by COMPILING
  # AND RUNNING test programs — which a macOS host cannot do for a Linux
  # target. Every `build-cross.sh` run attempted it, failed, and blamed the
  # ALSA sysroot; the tarball ships `tangent-ffmpeg`, so MP3 delivery goes
  # through the sidecar exactly as it does on Windows and nothing is lost.
  #
  # Clear the STAGE, never the tarball. Removing the artifact up front meant a
  # failed run destroyed a perfectly good Linux release that had been built on
  # the Linux box and rsynced back — silently, and before the failure that made
  # it look like the run had simply not produced one. The replacement happens
  # at the end, and only on success.
  rm -rf "$stage"
  # The sysroot, and the multiarch directory name inside it — which is the
  # Debian spelling of the target, not the Rust one.
  local darch multiarch sysroot
  case "$target" in
    x86_64-unknown-linux-gnu)  darch=amd64; multiarch=x86_64-linux-gnu ;;
    aarch64-unknown-linux-gnu) darch=arm64; multiarch=aarch64-linux-gnu ;;
    *) echo "!! no ALSA sysroot mapping for $target"; return 1 ;;
  esac
  sysroot="$(ensure_alsa_sysroot "$darch")" || {
    echo "!! could not fetch the ALSA sysroot for $darch"; return 1;
  }
  PKG_CONFIG_ALLOW_CROSS=1 \
  PKG_CONFIG_SYSROOT_DIR="$sysroot" \
  PKG_CONFIG_PATH="$sysroot/usr/lib/$multiarch/pkgconfig" \
  RUSTFLAGS="-L $sysroot/usr/lib/$multiarch" \
    cargo zigbuild --release --target "${target}.${GLIBC}" -p tangent \
      --no-default-features --features recorder || return 1
  if [ ! -f "target/${target}/release/tangent" ]; then
    echo "!! no binary at target/${target}/release/tangent"
    return 1
  fi
  mkdir -p "$stage/fonts" || return 1
  cp "target/${target}/release/tangent" "$stage/" || { rm -rf "$stage"; return 1; }
  # **What the loader will demand on the user's machine, checked here.**
  # The list and the reasoning live in one place, shared with
  # `build-linux-native.sh`, because two copies of it would disagree the first
  # time somebody edited one — which is the very failure it guards against.
  # GLIBC_FLOOR makes the check also refuse a binary whose symbol-version refs
  # exceed the floor zigbuild just targeted — the number the README advertises
  # and the loader enforces (see the comment at GLIBC= above).
  GLIBC_FLOOR="$GLIBC" scripts/check-linux-deps.sh "$stage/tangent" \
    || { rm -rf "$stage"; return 1; }
  cp assets/tangent.desktop "$stage/tangent.desktop" || { rm -rf "$stage"; return 1; }
  cp assets/tangent.png "$stage/tangent.png" || { rm -rf "$stage"; return 1; }
  cp assets/fonts/CourierPrime-Regular.ttf assets/fonts/CourierPrime-Bold.ttf \
     assets/fonts/OFL.txt "$stage/fonts/" || { rm -rf "$stage"; return 1; }
  # Licences for the four fonts eframe's `default_fonts` embeds in the binary.
  # (errexit is off in this function — every command needs its own handler.)
  mkdir -p "$stage/font-licenses" || { rm -rf "$stage"; return 1; }
  cp assets/font-licenses/*.txt "$stage/font-licenses/" || { rm -rf "$stage"; return 1; }
  cp LICENSE THIRD-PARTY-LICENSES "$stage/" || { rm -rf "$stage"; return 1; }
  # **The encoder, which this path shipped without for two releases.**
  #
  # Video on Linux is an ffmpeg subprocess, resolved exe-adjacent before PATH.
  # `build-linux-remote.sh` packed it and this one never did — so a tarball
  # built here was 10 MB instead of 45, video worked on any box that happened
  # to have distro ffmpeg, and broke on every clean one. Found by a tester on
  # a clean box; see docs/LINUX-4.11-FINDINGS.md, finding 2.
  #
  # Both architectures now. The arm64 tarball shipped NO encoder for as long
  # as it existed — video silently depended on a distro ffmpeg a clean machine
  # does not have, which is the 4.11 hole all over again on the other arch.
  local vendor="linux"
  [ "$arch" = "x86_64" ] || vendor="linux-arm64"
  scripts/fetch-ffmpeg.sh "$vendor" || { rm -rf "$stage"; return 1; }
  cp "dist/vendor/$vendor/tangent-ffmpeg" "$stage/" || { rm -rf "$stage"; return 1; }
  chmod 0755 "$stage/tangent-ffmpeg" || { rm -rf "$stage"; return 1; }
  cp -R "dist/vendor/$vendor/ffmpeg-licenses" "$stage/ffmpeg-licenses" \
    || { rm -rf "$stage"; return 1; }
  # The same user-facing readme the macOS and Windows artifacts carry.
  cp docs/ARTIFACT-README.md "$stage/README.txt" \
    || { rm -rf "$stage"; return 1; }
  # **The installer the README tells them to run.**
  #
  # `build-installer.sh` copied this in and this path never did, so every
  # tarball built here shipped a readme whose Linux section says
  # `./install.sh` next to a directory that did not contain one. The script
  # handles an app-only archive: the plugin is added by the installer path,
  # and its absence here is a note rather than a failure.
  cp installer/linux/install.sh "$stage/install.sh" || { rm -rf "$stage"; return 1; }
  chmod 0755 "$stage/install.sh" || { rm -rf "$stage"; return 1; }
  # **Everything the readme promises is actually here.** The readme is one
  # file shared by three platforms, so a name in it is a claim this stage has
  # to be able to meet — and the way that breaks is silently, one release at a
  # time, on the platform nobody packs by hand.
  # tangent.desktop and tangent.png are on the list since 4.20.0: the installer
  # now ASKS whether you want desktop integration, and with neither file in the
  # stage it silently never asks. A missing feature that produces no output at
  # all is exactly the kind this loop exists to catch.
  for promised in tangent install.sh README.txt LICENSE tangent.desktop tangent.png; do
    if [ ! -e "$stage/$promised" ]; then
      echo "!! the readme promises $promised and the stage has no such file"
      rm -rf "$stage"; return 1
    fi
  done
  # **Readable by somebody other than the person who packed it.** A root
  # install of a tarball whose files are 0600 gives every other user on the
  # box an app they cannot read. Directories need the bit too, or nobody can
  # list them.
  chmod -R a+rX "$stage" || { rm -rf "$stage"; return 1; }
  # Written beside the real name and moved into place, so a tar that fails
  # half way leaves the previous artifact intact rather than a truncated one.
  # **No mac metadata in a Linux tarball.** bsdtar writes the host's extended
  # attributes as pax headers — Dropbox's, Apple's provenance, the lot — and GNU
  # tar on the far side prints a warning for every one of them. Forty lines of
  # "Ignoring unknown extended header keyword" is what somebody sees instead of
  # the file list, and it reads like the archive is damaged.
  COPYFILE_DISABLE=1 \
  tar --no-xattrs --no-mac-metadata \
      -C dist -czf "${stage}.tar.gz.new" "tangent-${VERSION}-linux-${arch}" || {
    rm -rf "$stage" "${stage}.tar.gz.new"; return 1;
  }
  mv -f "${stage}.tar.gz.new" "${stage}.tar.gz" || {
    rm -rf "$stage" "${stage}.tar.gz.new"; return 1;
  }
  rm -rf "$stage"
  build_stamp "${stage}.tar.gz"
  echo "==> ${stage}.tar.gz"
}

# **A failed Linux leg lets the others run, and then fails the run.** Linux
# has been built here since the bullseye sysroot (ensure_alsa_sysroot), so a
# failure is a real one - the glibc floor refusing a binary, a file the
# readme promises missing, a sysroot that would not download - and not "the
# ALSA cross-build" this used to blame. The next leg still runs, so one
# failure costs nothing else, and the exit status at the end says so. It
# used to exit 0: release.sh then summed a release with no Linux tarball, or
# with the previous run's (package_linux keeps the old one on failure), and
# the README's and every supporter email's Linux link 404ed.
LINUX_OK=1
echo "==> Tangent $VERSION — Linux x86_64"
package_linux x86_64-unknown-linux-gnu x86_64 || { LINUX_OK=0; echo "!! Linux x86_64 build failed: see the log above"; }

echo "==> Tangent $VERSION — Linux aarch64"
package_linux aarch64-unknown-linux-gnu aarch64 || { LINUX_OK=0; echo "!! Linux aarch64 build failed: see the log above"; }

echo "==> Tangent $VERSION — Windows x86_64"
# No mp3 on the Windows leg: LAME's autotools build cannot run under
# xwin. The app copes — the delivery falls back to WAV with a note.
#
# ASIO rides in when the Steinberg SDK is present (vendor/asiosdk_*,
# fetched separately — its licence forbids committing it). The owner's
# ruling: ASIO is the standard and the testers have interfaces. ASIO_DIR was
# found (or its absence refused) at the top, before the Linux legs.
WIN_FEATURES="recorder"
if [ -n "$ASIO_DIR" ]; then
  export CPAL_ASIO_DIR="$ASIO_DIR"
  WIN_FEATURES="recorder,asio"
  # asio-sys assumes a native Windows host and hunts for vcvarsall.bat;
  # VCINSTALLDIR set to anything makes it trust the env instead — and the
  # env IS the toolchain here: cargo-xwin's clang-cl + the xwin headers.
  export VCINSTALLDIR="${VCINSTALLDIR:-/}"
  XWIN="$HOME/Library/Caches/cargo-xwin/xwin"
  if [ -d "$XWIN" ]; then
    export BINDGEN_EXTRA_CLANG_ARGS="--target=x86_64-pc-windows-msvc -I$XWIN/crt/include -I$XWIN/sdk/include/ucrt -I$XWIN/sdk/include/um -I$XWIN/sdk/include/shared"
  fi
  echo "==> ASIO backend in (SDK at $ASIO_DIR)"
else
  echo "==> ALLOW_WASAPI_ONLY=1: no ASIO SDK, so this Windows build is WASAPI-only"
fi
cargo xwin build --release --target x86_64-pc-windows-msvc -p tangent --no-default-features --features "$WIN_FEATURES"
WINSTAGE="dist/tangent-${VERSION}-windows-x86_64"
WINZIP="${WINSTAGE}.zip"
rm -rf "$WINSTAGE" "$WINZIP"
mkdir -p "$WINSTAGE"
cp target/x86_64-pc-windows-msvc/release/tangent.exe "$WINSTAGE/"
# **The symbols, beside the deliverables and never inside them.** lld-link
# writes a PDB of public symbols even for the stripped release, and it is
# what turned a Windows Error Reporting offset into a function name in one
# command on 2026-09-05 (`llvm-symbolizer --obj=tangent.exe 0x1400c6d417`).
# Kept per build under the build's own name, because an offset is only
# meaningful against the exact binary that faulted.
cp target/x86_64-pc-windows-msvc/release/tangent.pdb "dist/tangent-${VERSION}-windows-x86_64.pdb" 2>/dev/null || true
scripts/sign-windows.sh "$WINSTAGE/tangent.exe"
# Steinberg's required notice, when the ASIO backend rode in: keyed on the
# features this build was compiled with, not on CPAL_ASIO_DIR. That is cpal's
# own variable and a shell can export it, so a WASAPI-only build made with it
# set used to carry the notice, which the artifact checks took for proof of
# ASIO. (They read the host off tangent.exe now as well.)
case ",$WIN_FEATURES," in
  *,asio,*) printf '%s\n' "ASIO is a trademark and software of Steinberg Media Technologies GmbH." > "$WINSTAGE/ASIO-NOTICE.txt" ;;
esac
cp LICENSE THIRD-PARTY-LICENSES "$WINSTAGE/"
cp assets/fonts/OFL.txt "$WINSTAGE/"
# Licences for the four fonts eframe's `default_fonts` embeds in tangent.exe.
mkdir -p "$WINSTAGE/font-licenses"
cp assets/font-licenses/*.txt "$WINSTAGE/font-licenses/"
# The bundled encoder, so video needs nothing installed — the app looks for
# `tangent-ffmpeg.exe` beside `tangent.exe` before falling back to PATH.
# Checksum-pinned and cached; offline after the first fetch.
scripts/fetch-ffmpeg.sh windows
cp dist/vendor/windows/tangent-ffmpeg.exe "$WINSTAGE/"
# Signed like tangent.exe, because Smart App Control checks every program
# each time it starts: an unsigned encoder is blocked the first time Tangent
# runs it, however the app itself is signed. The STAGED copy, so the cache in
# dist/vendor stays byte-for-byte BtbN's build.
scripts/sign-windows.sh "$WINSTAGE/tangent-ffmpeg.exe"
cp -R dist/vendor/windows/ffmpeg-licenses "$WINSTAGE/ffmpeg-licenses"
# User-facing instructions travel with the build (SmartScreen steps, connecting a
# keyboard, the teach/correct features, where settings live). Shipped as .txt, not
# .md: Windows has no default handler for .md.
cp docs/ARTIFACT-README.md "$WINSTAGE/README.txt"
# Archive whatever was staged above. A hand-maintained file list silently drops
# anything newly added — that is how the readme went missing from the macOS
# artifacts before 2.1.0.
# **The zip is a DELIVERABLE again, as well as the installer's input.** It was
# briefly demoted to a transient — deleted the moment setup.exe existed, on
# the ruling that one Windows download is one thing to support — but the
# versionless permalinks derived from it, `tangent-windows-x86_64.zip` and the
# legacy `ivory-windows-x86_64.zip`, are already printed in README.md, on the
# store page and in every supporter email sent, and `publish-github.sh` can
# only upload an alias for a file that actually exists in dist/. Deleting the
# zip here meant the FIRST stable publish would silently drop both aliases and
# every one of those links would 404 — for exactly the people who paid (see
# publish-github.sh's header: uploading the aliases is the release contract).
# `build-installer.sh` still unzips exactly this file, so the installed files
# and the portable files are one build's: a failed installer build leaves no
# setup.exe (it removes the old one first), and check-artifacts.sh compares
# the two downloads' programs byte for byte.
(cd "$WINSTAGE" && zip -qr "$ROOT/$WINZIP" . -x '.DS_Store' '._*')
rm -rf "$WINSTAGE"
build_stamp "$WINZIP"
echo "==> $WINZIP"
if command -v makensis >/dev/null 2>&1; then
  scripts/build-installer.sh windows
  # SmartScreen judges the OUTER PE, so the installer takes the same signing
  # route as the inner exe. After makensis, necessarily: NSIS writes its
  # output last, and Authenticode is appended to the finished PE (the NSIS
  # CRC lives inside it and is untouched — signing post-build is the standard
  # flow). The uninstaller inside it was signed while makensis ran. The
  # artifact checks (scripts/check-artifacts.sh: release.sh, and
  # publish-dropbox.sh for testers) refuse an unsigned setup.exe, or an
  # unsigned PE inside it or the zip.
  scripts/sign-windows.sh "dist/Tangent-${VERSION}-windows-setup.exe"
  # Stamped once signed: the signature changes the bytes the stamp records.
  build_stamp "dist/Tangent-${VERSION}-windows-setup.exe"
  echo "==> dist/Tangent-${VERSION}-windows-setup.exe"
else
  # No installer toolchain: the zip alone is a real deliverable, but the
  # setup.exe the store links is NOT produced — say so loudly.
  echo "WARNING: makensis not found — built NO installer, only the portable zip." >&2
  echo "         Install it with: brew install makensis, then rerun." >&2
fi

# A listing, not a check: `ls` exits non-zero for a name that is not there,
# and under pipefail that was the only thing that failed a run with no
# setup.exe - by accident, with no word about why.
ls -lh dist/tangent-"${VERSION}"-* dist/Tangent-"${VERSION}"-windows-setup.exe 2>/dev/null | sed 's/^/    /' || true
# **Any leg that failed fails the run**, now that every leg has had its turn.
BUILD_OK=1
[ "$LINUX_OK" = 1 ] || {
  echo "!! a Linux leg failed (above): its tarball is missing, or is still the previous run's" >&2
  BUILD_OK=0
}
[ -f "dist/Tangent-${VERSION}-windows-setup.exe" ] || {
  echo "!! no setup.exe was built" >&2
  BUILD_OK=0
}
[ "$BUILD_OK" = 1 ] || exit 1
