#!/usr/bin/env bash
# Build the ffmpeg Tangent bundles on Linux, from pinned sources, with only
# what Tangent asks of it.
#
#   scripts/build-ffmpeg-linux.sh x86_64 [dest]    -> $WORK/out/x86_64/
#   scripts/build-ffmpeg-linux.sh aarch64 [dest]   -> $WORK/out/aarch64/
#   scripts/build-ffmpeg-linux.sh all
#   scripts/build-ffmpeg-linux.sh source           stage the source release only
#
# Each out/<arch>/ holds `tangent-ffmpeg` and `ffmpeg-licenses/` (the licence
# texts), and is copied to [dest] when one is given: fetch-ffmpeg.sh passes
# dist/vendor/linux(-arm64) and adds SOURCE.txt. A binary already built by
# this exact script (same bytes, same zig) is reused, FORCE=1 rebuilds.
# $WORK defaults to $THIRD_PARTY_SRC/build-ffmpeg-linux, $THIRD_PARTY_SRC to
# ~/Library/Caches/tangent-dist/third-party-src on macOS and
# ~/.cache/tangent-dist/third-party-src elsewhere.
#
# FROM THE SOURCE RELEASE ALONE. The copy of this script in the release
# folder works there by itself: run from a folder holding SOURCE.txt and the
# FFmpeg tarball, it takes every source from that folder.
#
# WHY WE BUILD IT. The bundled ffmpeg is GPL, and the downloads are offered
# under GPLv3 section 6(d), which wants the COMPLETE corresponding source of
# the exact binaries on a server we keep up (LICENSING.md, "The source it is
# owed"). A third-party static build whose library sources and build scripts
# are not published (the johnvansickle.com 7.0.2 builds this replaced) can
# never meet that. A build made by this script can: the pinned tarballs below
# plus this script ARE the complete corresponding source, and every run
# stages exactly that as the release folder
# $THIRD_PARTY_SRC/third-party-src-ffmpeg-<ver>-tangent-linux/, which
# scripts/publish-third-party-src.sh uploads.
#
# WHAT TANGENT ASKS OF IT (grep for `ffmpeg::program()`):
#   * take video, encode/ffmpeg.rs `Encoder::create`: BGRA rawvideo on
#     pipe:0 -> libx264 -preset veryfast, yuv420p, mp4 +faststart.
#   * take mux, encode/ffmpeg.rs `Encoder::mux`: that mp4 + raw f32le
#     samples -> -c:v copy, native AAC 192k, mp4 +faststart.
#   * MP3 export, encode.rs `encode_mp3_via_ffmpeg`: a float WAV ->
#     libmp3lame CBR 320k, .mp3.
#   * import, decode.rs `read_wav`: any audio file (the pickers offer wav,
#     aiff, mp3, m4a, aac, flac, ogg, opus, caf, wma; a video file dropped in
#     works too, `-map a:0`) -> f32 stereo WAV on stdout at the device rate.
# So: libx264 (GPLv2+), LAME (LGPLv2+), FFmpeg's own AAC encoder, FFmpeg's
# native decoders for those import formats, and NOTHING else: no network, no
# hardware acceleration (no libva, which is what made BtbN's linux64 build
# abort on old distros; fetch-ffmpeg.sh tells that story), no X11, no zlib.
# --enable-version3 so the licence stays the GPLv3 the docs and the
# installer's checks already name (all three parts allow it: "or later").
#
# HOW. Cross-compiled with zig as the C compiler, against glibc 2.30, the
# floor every Linux artifact keeps (build-cross.sh, GLIBC=). x264, LAME and
# FFmpeg's libraries are linked statically; the program needs only glibc
# (libc, libm, libpthread and the loader), checked at the end with
# scripts/check-linux-deps.sh. The x86_64 assembly needs nasm, which is
# built here from its own pinned tarball for the build machine (a build
# tool, not part of the binary). Runs on macOS or Linux; needs zig, make,
# pkg-config, git, gpg, curl, xz and a host C compiler (for nasm).
#
# PINNED. Every source is fetched into its own new directory under
# $WORK/dl/ and checked against the sha256 recorded here (FFmpeg's tarball
# also against its detached signature, x264's checkout against its commit
# id) before anything reads it. Nothing downloaded is executed before that
# check. A source an upstream server no longer serves is taken from our own
# published source release instead (SRC_MIRROR), under the same checks.
# Bumping a pin is one edit here, on purpose, and owes a new source release:
# the release name carries the FFmpeg version, so change the name too when
# anything else moves.
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
SELF="$HERE/$(basename "$0")"
ROOT="$(cd "$HERE/.." && pwd)"   # a Tangent checkout, when run from scripts/
if [ "$(uname -s)" = Darwin ]; then CACHE="$HOME/Library/Caches"; else CACHE="${XDG_CACHE_HOME:-$HOME/.cache}"; fi
SRC_ROOT="${THIRD_PARTY_SRC:-$CACHE/tangent-dist/third-party-src}"
WORK="${FFMPEG_LINUX_WORK:-$SRC_ROOT/build-ffmpeg-linux}"
GLIBC="2.30"   # keep equal to build-cross.sh GLIBC
# The compiler the published binaries were made with. Another zig builds a
# working program, not the same bytes, so a different one is said out loud.
ZIG_VER="0.16.0"

FFMPEG_VER="8.1.3"
FFMPEG_FILE="ffmpeg-$FFMPEG_VER.tar.xz"
FFMPEG_URL="https://ffmpeg.org/releases/$FFMPEG_FILE"
FFMPEG_SHA="7138d28c96d9d3e3af4ee3d8cad72741f8ffb40da90c1112235dea3ecd3178a3"
# The FFmpeg release signing key (ffmpeg.org/download.html).
FFMPEG_KEY_URL="https://ffmpeg.org/ffmpeg-devel.asc"
FFMPEG_KEY_FPR="FCF986EA15E6E293A5644F10B4322F04D67658D8"

# x264 has no release tarballs; its `stable` branch head, by commit (r3222,
# the same commit Homebrew's x264 builds). The archive is `git archive` of
# that commit, gzipped. The commit id is checked on every fresh checkout,
# and the pin below is the sha256 of the UNCOMPRESSED tar: git writes the
# same tar on macOS and Linux (checked, git 2.5x), but Apple's gzip and GNU
# gzip compress it to different bytes, so the .tar.gz itself cannot be the
# pin. SHA256SUMS.txt in the source release records the .tar.gz we used.
X264_COMMIT="b35605ace3ddf7c1a5d67a2eb553f034aef41d55"
X264_REPO="https://code.videolan.org/videolan/x264.git"
X264_FILE="x264-$X264_COMMIT.tar.gz"
X264_TAR_SHA="df62afece4eecef8ddf196f115dfec6e936a21b04ec2117a0f4e412de29b20e5"

# LAME 3.100, the release every distribution builds (upstream publishes no
# signature; the hash is the one Debian, Arch and Homebrew pin).
LAME_VER="3.100"
LAME_FILE="lame-$LAME_VER.tar.gz"
LAME_URL="https://downloads.sourceforge.net/project/lame/lame/$LAME_VER/$LAME_FILE"
LAME_SHA="ddfe36cab873794038ae2c1210557ad34857a4b6bdc515785d1da9e175b1da1e"

# nasm, the assembler for x86_64's hand-written code (x264 and FFmpeg). A
# build tool, but pinned and shipped in the source release all the same.
# No upstream signature; the hash matches Homebrew's nasm 3.02.
NASM_VER="3.02"
NASM_FILE="nasm-$NASM_VER.tar.xz"
NASM_URL="https://www.nasm.us/pub/nasm/releasebuilds/$NASM_VER/$NASM_FILE"
NASM_SHA="87336eba53b4acfe917424ab5d500d2b0054d9f5148d35c2273ccf2cfb712f0d"

RELEASE="third-party-src-ffmpeg-$FFMPEG_VER-tangent-linux"
SRC_MIRROR="https://github.com/ganten1998/tangent/releases/download/$RELEASE"
RELDIR="$SRC_ROOT/$RELEASE"
# Run from inside the source release itself: that folder is the source.
if [ -f "$HERE/SOURCE.txt" ] && [ -f "$HERE/$FFMPEG_FILE" ]; then RELDIR="$HERE"; fi

say() { echo "==> $*"; }
die() { echo "FAIL: $*" >&2; exit 1; }
sha() {
    if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1" | awk '{print $1}'
    else shasum -a 256 "$1" | awk '{print $1}'; fi
}
jobs_() { getconf _NPROCESSORS_ONLN 2>/dev/null || sysctl -n hw.ncpu 2>/dev/null || echo 4; }
JOBS="${JOBS:-$(jobs_)}"

# The release folder is also the source cache: a file already staged there
# and matching its pin is used as it is.
cached() { # cached <file> <sha>: echo the path of a verified copy, if any
    local f
    for f in "$RELDIR/$1" "$WORK/src/$1"; do
        if [ -f "$f" ] && [ "$(sha "$f")" = "$2" ]; then echo "$f"; return 0; fi
    done
    return 1
}

# fetch <file> <sha> <url>: into $WORK/src/<file>, verified. Each download
# lands in a fresh directory of its own, is checked there, and only then
# moves next to the others.
fetch() {
    local file="$1" want="$2" url="$3" have dl
    mkdir -p "$WORK/src"
    if have="$(cached "$file" "$want")"; then
        [ "$have" = "$WORK/src/$file" ] || cp "$have" "$WORK/src/$file"
        return 0
    fi
    for u in "$url" "$SRC_MIRROR/$file"; do
        dl="$(mktemp -d "$WORK/dl.XXXXXX")"
        say "fetching $u"
        if curl -fsSL --retry 3 -o "$dl/$file" "$u" && [ "$(sha "$dl/$file")" = "$want" ]; then
            mv "$dl/$file" "$WORK/src/$file"; rm -rf "$dl"; return 0
        fi
        echo "    $u: missing or not the pinned sha256" >&2
        rm -rf "$dl"
    done
    die "$file: no copy matches sha256 $want"
}

# The FFmpeg tarball's signature, in a keyring of its own (never the user's).
verify_ffmpeg_sig() {
    local gh dl
    dl="$(mktemp -d "$WORK/dl.XXXXXX")"
    if [ -f "$RELDIR/$FFMPEG_FILE.asc" ]; then cp "$RELDIR/$FFMPEG_FILE.asc" "$dl/"
    else curl -fsSL --retry 3 -o "$dl/$FFMPEG_FILE.asc" "$FFMPEG_URL.asc" \
        || curl -fsSL --retry 3 -o "$dl/$FFMPEG_FILE.asc" "$SRC_MIRROR/$FFMPEG_FILE.asc"; fi
    gh="$(mktemp -d "$WORK/gpg.XXXXXX")"
    curl -fsSL --retry 3 -o "$gh/key.asc" "$FFMPEG_KEY_URL"
    # The import can exit 2 for want of a gpg-agent it does not need; the
    # VALIDSIG line below is the check.
    GNUPGHOME="$gh" gpg --batch --quiet --import "$gh/key.asc" 2>/dev/null || true
    if GNUPGHOME="$gh" gpg --batch --status-fd 1 --verify "$dl/$FFMPEG_FILE.asc" "$WORK/src/$FFMPEG_FILE" 2>/dev/null \
        | grep -q "VALIDSIG $FFMPEG_KEY_FPR"; then
        say "$FFMPEG_FILE: good signature from $FFMPEG_KEY_FPR"
        mv "$dl/$FFMPEG_FILE.asc" "$WORK/src/$FFMPEG_FILE.asc"
    else
        GNUPGHOME="$gh" gpgconf --kill all 2>/dev/null || true
        rm -rf "$gh" "$dl"; die "$FFMPEG_FILE does not verify against $FFMPEG_KEY_FPR"
    fi
    GNUPGHOME="$gh" gpgconf --kill all 2>/dev/null || true
    rm -rf "$gh" "$dl"
}

# x264: the archive if we have it; otherwise a checkout of the pinned commit,
# checked by id, archived with fixed names and times.
x264_ok() { [ -f "$1" ] && [ "$(gzip -dc "$1" | sha /dev/stdin)" = "$X264_TAR_SHA" ]; }
fetch_x264() {
    local w got f
    mkdir -p "$WORK/src"
    for f in "$WORK/src/$X264_FILE" "$RELDIR/$X264_FILE"; do
        if x264_ok "$f"; then
            [ "$f" = "$WORK/src/$X264_FILE" ] || cp "$f" "$WORK/src/$X264_FILE"
            return 0
        fi
    done
    w="$(mktemp -d "$WORK/dl.XXXXXX")"
    if curl -fsSL --retry 3 -o "$w/$X264_FILE" "$SRC_MIRROR/$X264_FILE" 2>/dev/null \
        && x264_ok "$w/$X264_FILE"; then
        mv "$w/$X264_FILE" "$WORK/src/$X264_FILE"; rm -rf "$w"; return 0
    fi
    rm -rf "$w"; w="$(mktemp -d "$WORK/dl.XXXXXX")"
    say "fetching x264 $X264_COMMIT from $X264_REPO"
    git -C "$w" init -q x264
    git -C "$w/x264" fetch -q --depth 1 "$X264_REPO" "$X264_COMMIT"
    got="$(git -C "$w/x264" rev-parse FETCH_HEAD)"
    [ "$got" = "$X264_COMMIT" ] || { rm -rf "$w"; die "x264 fetched $got, not $X264_COMMIT"; }
    git -C "$w/x264" archive --format=tar --prefix="x264-$X264_COMMIT/" "$X264_COMMIT" \
        | gzip -n -9 > "$w/$X264_FILE"
    x264_ok "$w/$X264_FILE" || { rm -rf "$w"; die "x264 archive of $X264_COMMIT is not the pinned tar ($X264_TAR_SHA)"; }
    mv "$w/$X264_FILE" "$WORK/src/$X264_FILE"; rm -rf "$w"
}

fetch_all() {
    fetch "$FFMPEG_FILE" "$FFMPEG_SHA" "$FFMPEG_URL"
    verify_ffmpeg_sig
    fetch_x264
    fetch "$LAME_FILE" "$LAME_SHA" "$LAME_URL"
    fetch "$NASM_FILE" "$NASM_SHA" "$NASM_URL"
}

# ---------------------------------------------------------------- nasm (host)
build_nasm() {
    local t="$WORK/tools"
    if [ -x "$t/bin/nasm" ] && "$t/bin/nasm" -v 2>/dev/null | grep -q "version $NASM_VER "; then return 0; fi
    say "nasm $NASM_VER for the build machine"
    rm -rf "$t/nasm-$NASM_VER"; mkdir -p "$t"
    tar -xJf "$WORK/src/$NASM_FILE" -C "$t"
    mkdir -p "$t/bin"
    ( cd "$t/nasm-$NASM_VER" && ./configure -q CC="${HOST_CC:-cc}" >/dev/null \
        && make -s -j"$JOBS" nasm >/dev/null ) || die "nasm did not build"
    install -m 755 "$t/nasm-$NASM_VER/nasm" "$t/bin/nasm"
    "$t/bin/nasm" -v
}

# ------------------------------------------------------------------- one arch
# tools <arch>: the cross tools as plain commands, so no path lands in the
# configure line FFmpeg embeds in its binary (`ffmpeg -buildconf`).
tools() {
    local arch="$1" bin="$WORK/build/$1/bin" pfx="$WORK/build/$1/prefix"
    mkdir -p "$bin"
    cat > "$bin/tangent-cc" <<EOF
#!/bin/sh
# zig as a cross compiler for $arch-linux-gnu, glibc $GLIBC. The prefix holds
# x264 and LAME; the path map keeps build paths out of the binary.
exec zig cc -target $arch-linux-gnu.$GLIBC -I$pfx/include -L$pfx/lib \\
    -ffile-prefix-map=$WORK/build/$arch/=./ "\$@"
EOF
    printf '#!/bin/sh\nexec zig ar "$@"\n' > "$bin/tangent-ar"
    printf '#!/bin/sh\nexec zig ranlib "$@"\n' > "$bin/tangent-ranlib"
    chmod 755 "$bin"/tangent-*
    [ -x "$WORK/tools/bin/nasm" ] && ln -sf "$WORK/tools/bin/nasm" "$bin/nasm"
    return 0
}

build_arch() {
    local arch="$1" b="$WORK/build/$1" pfx host
    pfx="$b/prefix"
    case "$arch" in
        x86_64)  host="x86_64-linux-gnu"; build_nasm ;;
        aarch64) host="aarch64-linux-gnu" ;;
        *) die "unknown arch $arch" ;;
    esac
    rm -rf "$b"; mkdir -p "$b" "$pfx"
    tools "$arch"
    export PATH="$b/bin:$PATH"
    export ZIG_GLOBAL_CACHE_DIR="$WORK/zig-cache" ZIG_LOCAL_CACHE_DIR="$WORK/zig-cache"
    export SOURCE_DATE_EPOCH=1767225600   # 2026-01-01, for anything that stamps
    export PKG_CONFIG_LIBDIR="$pfx/lib/pkgconfig" PKG_CONFIG_PATH=""
    local build_triple
    build_triple="$(uname -m | sed 's/arm64/aarch64/')-$( [ "$(uname -s)" = Darwin ] && echo apple-darwin || echo pc-linux-gnu)"

    say "$arch: x264 ${X264_COMMIT:0:12}"
    tar -xzf "$WORK/src/$X264_FILE" -C "$b"
    ( cd "$b/x264-$X264_COMMIT"
      CC=tangent-cc AR=tangent-ar RANLIB=tangent-ranlib \
      AS="$( [ "$arch" = x86_64 ] && echo nasm || echo tangent-cc )" \
      ./configure --host="$host" --prefix="$pfx" \
        --enable-static --enable-pic --disable-cli --disable-opencl \
        --disable-avs --disable-swscale --disable-lavf --disable-ffms \
        --disable-gpac --disable-lsmash --bit-depth=8 --chroma-format=420 >"$b/x264.log" 2>&1 \
        && make -j"$JOBS" >>"$b/x264.log" 2>&1 && make install-lib-static >>"$b/x264.log" 2>&1 ) \
        || { tail -30 "$b/x264.log" >&2; die "$arch: x264 did not build ($b/x264.log)"; }

    say "$arch: LAME $LAME_VER"
    tar -xzf "$WORK/src/$LAME_FILE" -C "$b"
    ( cd "$b/lame-$LAME_VER"
      CC=tangent-cc AR=tangent-ar RANLIB=tangent-ranlib \
      ./configure --host="$host" --build="$build_triple" --prefix="$pfx" \
        --disable-shared --enable-static --disable-frontend --disable-decoder \
        --disable-gtktest --disable-dependency-tracking >"$b/lame.log" 2>&1 \
        && make -j"$JOBS" >>"$b/lame.log" 2>&1 && make install >>"$b/lame.log" 2>&1 ) \
        || { tail -30 "$b/lame.log" >&2; die "$arch: LAME did not build ($b/lame.log)"; }

    say "$arch: FFmpeg $FFMPEG_VER"
    tar -xJf "$WORK/src/$FFMPEG_FILE" -C "$b"
    local asm=()
    [ "$arch" = x86_64 ] && asm=(--x86asmexe=nasm)
    ( cd "$b/ffmpeg-$FFMPEG_VER"
      ./configure \
        --enable-cross-compile --arch="$arch" --target-os=linux \
        --cc=tangent-cc --ar=tangent-ar --ranlib=tangent-ranlib \
        --host-cc="${HOST_CC:-cc}" --pkg-config=pkg-config --pkg-config-flags=--static \
        ${asm[@]+"${asm[@]}"} \
        --prefix=/usr/local --disable-shared --enable-static \
        --enable-gpl --enable-version3 --enable-libx264 --enable-libmp3lame \
        --disable-autodetect --enable-pthreads \
        --disable-doc --disable-debug --disable-ffplay --disable-ffprobe \
        --disable-network --disable-avdevice \
        --disable-everything \
        --enable-protocol=file,pipe \
        --enable-demuxer=rawvideo,pcm_f32le,wav,w64,aiff,caf,mp3,flac,ogg,mov,matroska,aac,asf \
        --enable-decoder=rawvideo,pcm_s8,pcm_u8,pcm_s16le,pcm_s16be,pcm_s24le,pcm_s24be,pcm_s32le,pcm_s32be,pcm_f32le,pcm_f32be,pcm_f64le,pcm_f64be,pcm_alaw,pcm_mulaw,adpcm_ima_qt,adpcm_ima_wav,adpcm_ms,mp1,mp1float,mp2,mp2float,mp3,mp3float,aac,aac_fixed,alac,flac,vorbis,opus,wmav1,wmav2,wmapro,wmalossless,ac3,eac3 \
        --enable-parser=aac,ac3,flac,mpegaudio,opus,vorbis,h264 \
        --enable-encoder=libx264,aac,libmp3lame,pcm_f32le \
        --enable-muxer=mp4,mp3,wav \
        --enable-filter=scale,aresample,aformat,anull,format,null \
        --enable-swscale --enable-swresample \
        --disable-stripping --extra-ldflags=-s \
        --extra-version=tangent >"$b/ffmpeg.log" 2>&1 \
        || { tail -40 ffbuild/config.log >&2; exit 1; }
      # configure finds sysctl() by LINKING (check_func, no header): zig's
      # glibc 2.30 stubs still export it, but zig ships the headers of a
      # newer glibc, which dropped <sys/sysctl.h> in 2.32. On Linux FFmpeg
      # only calls it in BSD-only branches, so off it goes.
      sed -i.orig 's/^#define HAVE_SYSCTL 1$/#define HAVE_SYSCTL 0/' config.h
      make -j"$JOBS" ffmpeg >>"$b/ffmpeg.log" 2>&1 \
        || { grep -B2 -A6 -E 'error|Error' "$b/ffmpeg.log" | tail -40 >&2; exit 1; } ) \
        || die "$arch: FFmpeg did not build ($b/ffmpeg.log)"

    local out="$WORK/out/$arch"
    rm -rf "$out"; mkdir -p "$out/ffmpeg-licenses"
    install -m 755 "$b/ffmpeg-$FFMPEG_VER/ffmpeg" "$out/tangent-ffmpeg"
    cp "$b/ffmpeg-$FFMPEG_VER/COPYING.GPLv3" "$out/ffmpeg-licenses/GPLv3.txt"
    cp "$b/ffmpeg-$FFMPEG_VER/LICENSE.md" "$out/ffmpeg-licenses/FFmpeg-LICENSE.md"
    cp "$b/x264-$X264_COMMIT/COPYING" "$out/ffmpeg-licenses/x264-COPYING.txt"
    cp "$b/lame-$LAME_VER/COPYING" "$out/ffmpeg-licenses/LAME-COPYING.txt"
    # What the loader will demand on a user's machine: glibc and nothing else,
    # no symbol newer than the floor.
    if [ -x "$ROOT/scripts/check-linux-deps.sh" ]; then
        GLIBC_FLOOR="$GLIBC" "$ROOT/scripts/check-linux-deps.sh" "$out/tangent-ffmpeg"
    else
        echo "    (no Tangent checkout beside this script: DT_NEEDED and glibc floor not checked)"
    fi
    stamp > "$out/.built-by"
    say "$out/tangent-ffmpeg ($(wc -c < "$out/tangent-ffmpeg" | tr -d ' ') bytes)"
}

# ------------------------------------------------------------- source release
stage_source() {
    mkdir -p "$RELDIR"
    local f
    for f in "$FFMPEG_FILE" "$FFMPEG_FILE.asc" "$X264_FILE" "$LAME_FILE" "$NASM_FILE"; do
        if [ ! -f "$RELDIR/$f" ] || [ "$(sha "$RELDIR/$f")" != "$(sha "$WORK/src/$f")" ]; then
            cp "$WORK/src/$f" "$RELDIR/$f"
        fi
    done
    [ "$SELF" = "$RELDIR/build-ffmpeg-linux.sh" ] || cp "$SELF" "$RELDIR/build-ffmpeg-linux.sh"
    if [ -f "$ROOT/scripts/third-party-src/$RELEASE.SOURCE.txt" ]; then
        cp "$ROOT/scripts/third-party-src/$RELEASE.SOURCE.txt" "$RELDIR/SOURCE.txt"
    fi
    (
        cd "$RELDIR"
        for f in "$FFMPEG_FILE" "$FFMPEG_FILE.asc" "$X264_FILE" "$LAME_FILE" "$NASM_FILE" build-ffmpeg-linux.sh; do
            printf '%s  %s\n' "$(sha "$f")" "$f"
        done
    ) > "$RELDIR/SHA256SUMS.txt"
    say "source release staged: $RELDIR"
}

# What made a binary: this script's bytes and the compiler's version.
stamp() { echo "$(sha "$SELF") zig $(zig version)"; }
# up_to_date <arch>: the cached binary was made by this exact script and zig.
up_to_date() {
    [ -x "$WORK/out/$1/tangent-ffmpeg" ] && [ -f "$WORK/out/$1/.built-by" ] \
        && [ "$(cat "$WORK/out/$1/.built-by")" = "$(stamp)" ]
}
# deliver <arch> <dest>: the binary and its licence texts, copied out.
deliver() {
    [ -n "$2" ] || return 0
    mkdir -p "$2"
    install -m 755 "$WORK/out/$1/tangent-ffmpeg" "$2/tangent-ffmpeg"
    rm -rf "$2/ffmpeg-licenses"; cp -R "$WORK/out/$1/ffmpeg-licenses" "$2/ffmpeg-licenses"
}

command -v zig >/dev/null || die "zig is not installed (brew install zig); it is the cross compiler"
if [ "$(zig version)" != "$ZIG_VER" ]; then
    echo "warning: zig $(zig version), the published binaries were built with $ZIG_VER;" >&2
    echo "         this builds a working ffmpeg, not a byte-identical one." >&2
fi
mkdir -p "$WORK"
case "${1:-}" in
    x86_64|aarch64)
        if [ "${FORCE:-0}" != 1 ] && up_to_date "$1" && [ -f "$RELDIR/SHA256SUMS.txt" ]; then
            say "$1: $WORK/out/$1/tangent-ffmpeg is current"
        else
            fetch_all; stage_source; build_arch "$1"
        fi
        deliver "$1" "${2:-}" ;;
    all)
        fetch_all; stage_source
        for a in x86_64 aarch64; do
            if [ "${FORCE:-0}" = 1 ] || ! up_to_date "$a"; then build_arch "$a"; fi
        done ;;
    source) fetch_all; stage_source ;;
    *) echo "usage: scripts/build-ffmpeg-linux.sh <x86_64|aarch64|all|source> [dest]" >&2; exit 2 ;;
esac
