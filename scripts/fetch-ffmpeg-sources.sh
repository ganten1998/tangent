#!/usr/bin/env bash
# Stage the SOURCE for the GPL ffmpeg builds Tangent bundles, one folder per
# GitHub source release (LICENSING.md, "The source it is owed").
#
#   scripts/fetch-ffmpeg-sources.sh tangent-linux Linux (our own build, 8.1.3)
#   scripts/fetch-ffmpeg-sources.sh btbn-n8.1.3   Windows pin (BtbN n8.1.3)
#   scripts/fetch-ffmpeg-sources.sh all           both of the above
#   scripts/fetch-ffmpeg-sources.sh jvs-7.0.2     the RETIRED Linux pin
#                                                 (johnvansickle 7.0.2), whose
#                                                 source can never be complete
#
# The Linux source is staged by scripts/build-ffmpeg-linux.sh itself (every
# build stages what it built from, so the two cannot drift); `tangent-linux`
# here only asks it to.
#
# Output: $THIRD_PARTY_SRC/<release-name>/ (default
# ~/Library/Caches/tangent-dist/third-party-src), each with a SOURCE.txt that
# says what every file is and where it came from.
# scripts/publish-third-party-src.sh uploads those folders.
#
# PINNED, like fetch-ffmpeg.sh. The pins below change in the same edit as the
# binary pins there, on purpose: a new binary owes a new source release.
#
# NOTHING DOWNLOADED IS EXECUTED. BtbN's own download.sh would source every
# stage script inside their Docker image; this reads the SCRIPT_REPO /
# SCRIPT_COMMIT / SCRIPT_REV lines out of those files as text and fetches each
# pin with git or svn itself. The few stages whose download step does more
# than a checkout (submodules, a cargo vendor, a dependency sync script, a
# model download) are reproduced from the pinned files those steps read
# (btbn_extras), never by running the steps. Needs git, svn (Subversion, for
# LAME and Xvid), gpg, xz and, for the cargo vendor, the rustup cargo.
#
# Re-running is safe and cheap: an archive already staged is kept.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT_ROOT="${THIRD_PARTY_SRC:-$HOME/Library/Caches/tangent-dist/third-party-src}"
JOBS="${JOBS:-6}"

# The retired Linux pin (johnvansickle.com 7.0.2), bundled before Tangent
# built its own; kept so its partial source can still be staged.
JVS_RELEASE="third-party-src-ffmpeg-7.0.2"
JVS_FFMPEG="ffmpeg-7.0.2.tar.xz"

# Windows pin: fetch-ffmpeg.sh WIN_FILE, tag autobuild-2026-09-22-13-18.
# The tag is gone upstream (pruned by 2026-10-12). The commit is the head_sha
# of the scheduled "Build FFmpeg" run 35726071479 whose "Publish release" job
# ran 13:16:45 to 13:38:16 UTC on 2026-09-22, the minute the tag names.
BTBN_RELEASE="third-party-src-ffmpeg-n8.1.3-btbn-2026-09-22"
BTBN_REPO="https://github.com/BtbN/FFmpeg-Builds.git"
BTBN_COMMIT="3e6685eda92f9288c15ac320139622dcedca09a4"
BTBN_FFMPEG_TAG="n8.1.3"
BTBN_FFMPEG="ffmpeg-8.1.3.tar.xz"

# The FFmpeg release signing key (ffmpeg.org/download.html, "FFmpeg release
# signing key"), used only to check the tarballs' detached signatures.
FFMPEG_KEY_FPR="FCF986EA15E6E293A5644F10B4322F04D67658D8"

say() { echo "==> $*"; }
sha() { shasum -a 256 "$1" | awk '{print $1}'; }

# ffmpeg_tarball <dir> <file>: the release tarball and its .asc, verified.
ffmpeg_tarball() {
    local dir="$1" file="$2" gh
    [ -f "$dir/$file" ] || curl -fsSL --retry 3 -o "$dir/$file" "https://ffmpeg.org/releases/$file"
    [ -f "$dir/$file.asc" ] || curl -fsSL --retry 3 -o "$dir/$file.asc" "https://ffmpeg.org/releases/$file.asc"
    gh="$(mktemp -d)"
    curl -fsSL --retry 3 -o "$gh/key.asc" https://ffmpeg.org/ffmpeg-devel.asc
    GNUPGHOME="$gh" gpg --batch --quiet --import "$gh/key.asc" 2>/dev/null
    if GNUPGHOME="$gh" gpg --batch --status-fd 1 --verify "$dir/$file.asc" "$dir/$file" 2>/dev/null \
        | grep -q "VALIDSIG $FFMPEG_KEY_FPR"; then
        say "$file: good signature from $FFMPEG_KEY_FPR"
        echo "good signature, key $FFMPEG_KEY_FPR" > "$dir/.$file.verified"
    else
        echo "FAIL: $file does not verify against $FFMPEG_KEY_FPR" >&2
        rm -rf "$gh"; exit 1
    fi
    rm -rf "$gh"
}

# ---------------------------------------------------------------- jvs 7.0.2
jvs() {
    local dir="$OUT_ROOT/$JVS_RELEASE" bin
    mkdir -p "$dir"
    ffmpeg_tarball "$dir" "$JVS_FFMPEG"
    # The builder's readme (library versions) and licence, out of the binary
    # archive itself, so the release says which build it answers for.
    bin="$ROOT/vendor/ffmpeg/ffmpeg-7.0.2-amd64-static.tar.xz"
    [ -f "$bin" ] || bin="$ROOT/dist/vendor/ffmpeg-7.0.2-amd64-static.tar.xz"
    tar -xJOf "$bin" ffmpeg-7.0.2-amd64-static/readme.txt > "$dir/jvs-7.0.2-readme.txt"
    manifest "$dir"
    cp "$ROOT/scripts/third-party-src/$JVS_RELEASE.SOURCE.txt" "$dir/SOURCE.txt"
    say "$dir"
}

# ------------------------------------------------------------- btbn n8.1.3
# git_pin <dest.tar.xz> <repo> <commit-or-tag> [submodules...]
git_pin() {
    local dest="$1" repo="$2" rev="$3"; shift 3
    [ -s "$dest" ] && return 0
    local w; w="$(mktemp -d)"
    (
        cd "$w"
        git init -q src && cd src
        git remote add origin "$repo"
        # By commit or tag, shallow, where the server allows it; otherwise
        # the whole history and a checkout of the pin itself (FETCH_HEAD is
        # then a branch head, not the pin).
        if git fetch -q --depth 1 origin "$rev" 2>/dev/null; then
            git -c advice.detachedHead=false checkout -q FETCH_HEAD
        else
            git fetch -q --tags origin 2>/dev/null || git fetch -q origin
            git -c advice.detachedHead=false checkout -q "$rev"
        fi
        if [ "$#" -gt 0 ]; then
            if [ "$1" = all ]; then
                git submodule -q update --init --recursive --depth=1
            else
                git submodule -q update --init --recursive --depth=1 -- "$@"
            fi
        fi
        echo "$(git rev-parse HEAD)" > ../HEAD
    ) || { rm -rf "$w"; echo "FAILED $dest ($repo @ $rev)" >&2; return 1; }
    tar --exclude=.git -C "$w/src" -cf - . | xz -T0 -6 > "$dest.part"
    mv "$dest.part" "$dest"
    echo "$(cat "$w/HEAD")" > "$dest.commit"
    rm -rf "$w"
}

svn_pin() { # svn_pin <dest.tar.xz> <url> <rev>
    local dest="$1" w; [ -s "$dest" ] && return 0
    w="$(mktemp -d)"
    # xvid's server wants the anonymous login BtbN's stage passes.
    svn export -q --non-interactive -r "$3" "$2@$3" "$w/src" 2>/dev/null \
        || svn export -q --non-interactive --username anonymous --password '' -r "$3" "$2@$3" "$w/src" \
        || { rm -rf "$w"; echo "FAILED $dest ($2 @ r$3)" >&2; return 1; }
    tar -C "$w/src" -cf - . | xz -T0 -6 > "$dest.part"
    mv "$dest.part" "$dest"; echo "r$3" > "$dest.commit"; rm -rf "$w"
}

btbn() {
    local dir="$OUT_ROOT/$BTBN_RELEASE" scripts list
    mkdir -p "$dir/deps"
    ffmpeg_tarball "$dir" "$BTBN_FFMPEG"
    git_pin "$dir/ffmpeg-$BTBN_FFMPEG_TAG-git.tar.xz" https://git.ffmpeg.org/ffmpeg.git "$BTBN_FFMPEG_TAG"
    git_pin "$dir/FFmpeg-Builds-$BTBN_COMMIT.tar.xz" "$BTBN_REPO" "$BTBN_COMMIT"

    # The build scripts, unpacked as plain text to read the pins from.
    scripts="$(mktemp -d)"
    tar -xJf "$dir/FFmpeg-Builds-$BTBN_COMMIT.tar.xz" -C "$scripts" ./scripts.d
    list="$dir/deps/PINS.tsv"
    : > "$list"
    local f stage s repo rev
    for f in "$scripts"/scripts.d/*.sh "$scripts"/scripts.d/*/*.sh; do
        stage="${f#"$scripts"/scripts.d/}"; stage="${stage%.sh}"; stage="${stage//\//_}"
        for s in "" 2 3; do
            repo="$(sed -n "s/^SCRIPT_REPO$s=\"\(.*\)\"$/\1/p" "$f" | head -1)"
            rev="$(sed -n "s/^SCRIPT_COMMIT$s=\"\(.*\)\"$/\1/p" "$f" | head -1)"
            if [ -z "$rev" ]; then
                rev="$(sed -n "s/^SCRIPT_REV$s=\"\(.*\)\"$/\1/p" "$f" | head -1)"
                [ -n "$rev" ] && rev="svn:$rev"
            fi
            [ -n "$repo" ] && [ -n "$rev" ] || continue
            # gnulib: BtbN clones the GitHub mirror (SCRIPT_MIRROR2), which
            # answers a fetch by commit; savannah's endpoint does not.
            [ "$stage" = 20-libiconv ] && [ "$s" = 2 ] && repo="$(sed -n 's/^SCRIPT_MIRROR2="\(.*\)"$/\1/p' "$f")"
            printf '%s%s\t%s\t%s\n' "$stage" "${s:+-$s}" "$repo" "$rev" >> "$list"
        done
    done
    rm -rf "$scripts"

    # Stages whose download step also pulls submodules (BtbN's
    # ffbuild_dockerdl adds `git submodule update`): the same here.
    subs() {
        case "$1" in
            25-openssl|50-libplacebo|50-zimg|50-librist_40-mbedtls|50-librsvg_10-glib) echo all ;;
            50-libjxl_50-libjxl) echo third_party/highway ;;
        esac
    }
    export -f git_pin svn_pin
    local name short failed=0
    while IFS=$'\t' read -r name repo rev; do
        case "$rev" in
            svn:*) printf '%s\0' "svn_pin '$dir/deps/$name-r${rev#svn:}.tar.xz' '$repo' '${rev#svn:}'" ;;
            *) short="$rev"; [[ "$rev" =~ ^[0-9a-f]{40}$ ]] && short="${rev:0:12}"
               printf '%s\0' "git_pin '$dir/deps/$name-$short.tar.xz' '$repo' '$rev' $(subs "$name")" ;;
        esac
    done < "$list" | xargs -0 -n 1 -P "$JOBS" bash -c 'eval "$1"' _ || failed=1
    btbn_extras "$dir" || failed=1
    manifest "$dir"
    cp "$ROOT/scripts/third-party-src/$BTBN_RELEASE.SOURCE.txt" "$dir/SOURCE.txt"
    say "$dir ($(wc -l < "$list" | tr -d ' ') pins)"
    [ "$failed" = 0 ] || { echo "some pins FAILED (above); re-run to retry only those" >&2; exit 1; }
}

# The stages whose BtbN download step fetches more than its checkout, done
# here from the pinned files those steps read, without running the steps:
#   shaderc   utils/git-sync-deps clones the repos its DEPS file pins
#   opus      autogen.sh fetches the DNN model archive by its sha256
#   glib      `meson subprojects download proxy-libintl` (its .wrap pins it)
#   librsvg   `cargo vendor` of its Cargo.lock (BtbN's step)
#   rav1e     cargo fetches its Cargo.lock at build time (`cargo cinstall`)
btbn_extras() {
    local dir="$1" ex="$1/deps" t f ok=0 name rev
    t="$(mktemp -d)"
    # shaderc DEPS: 'name_revision': '<sha>', and the repo after Var('x_git').
    f="$(ls "$ex"/47-vulkan_50-shaderc-*.tar.xz | head -1)"
    tar -xJOf "$f" ./DEPS > "$t/DEPS"
    local base repo
    while read -r name base repo; do
        rev="$(sed -n "s/^ *'${name}_revision': *'\([0-9a-f]*\)'.*/\1/p" "$t/DEPS")"
        [ -n "$rev" ] || { echo "FAILED shaderc DEPS: no ${name}_revision" >&2; ok=1; continue; }
        git_pin "$ex/47-vulkan_50-shaderc-dep-$name-${rev:0:12}.tar.xz" "$base/$repo" "$rev" || ok=1
    done <<DEPS_LIST
abseil https://github.com/abseil abseil-cpp.git
effcee https://github.com/google effcee.git
glslang https://github.com/KhronosGroup glslang.git
googletest https://github.com/google googletest.git
re2 https://github.com/google re2.git
spirv_headers https://github.com/KhronosGroup SPIRV-Headers.git
spirv_tools https://github.com/KhronosGroup SPIRV-Tools.git
DEPS_LIST
    # opus: `dnn/download_model.sh "<sha256>"` in autogen.sh.
    f="$(ls "$ex"/50-libopus-*.tar.xz | head -1)"
    local h out
    h="$(tar -xJOf "$f" ./autogen.sh | sed -n 's/^dnn\/download_model.sh "\([0-9a-f]\{64\}\)".*/\1/p')"
    out="$ex/50-libopus-opus_data-$h.tar.gz"
    if [ -n "$h" ] && { [ -s "$out" ] || curl -fsSL --retry 3 -o "$out" "https://media.xiph.org/opus/models/opus_data-$h.tar.gz"; } \
        && [ "$(sha "$out")" = "$h" ]; then :; else
        echo "FAILED opus model ${h:-?}" >&2; rm -f "$out"; ok=1
    fi
    # glib: subprojects/proxy-libintl.wrap names the archive and its sha256.
    f="$(ls "$ex"/50-librsvg_10-glib-*.tar.xz | head -1)"
    tar -xJOf "$f" ./subprojects/proxy-libintl.wrap > "$t/wrap"
    local url fn wh
    url="$(sed -n 's/^source_url = //p' "$t/wrap")"
    fn="$(sed -n 's/^source_filename = //p' "$t/wrap")"
    wh="$(sed -n 's/^source_hash = //p' "$t/wrap")"
    out="$ex/50-librsvg_10-glib-subproject-$fn"
    if { [ -s "$out" ] || curl -fsSL --retry 3 -o "$out" "$url"; } && [ "$(sha "$out")" = "$wh" ]; then :; else
        echo "FAILED glib proxy-libintl ($url)" >&2; rm -f "$out"; ok=1
    fi
    # librsvg and rav1e: every crate their Cargo.lock pins. `cargo vendor`
    # only downloads and unpacks; it builds and runs nothing. It runs from a
    # directory of ours with an empty CARGO_HOME and the tree's own .cargo
    # removed, so no config in the unpacked tree or in ours steers it.
    for f in "$ex"/50-librsvg_99-librsvg-*.tar.xz "$ex"/50-rav1e-*.tar.xz; do
        case "$f" in *-cargo-vendor.tar.xz) continue ;; esac
        name="$(basename "$f" .tar.xz)"
        [ -s "$ex/$name-cargo-vendor.tar.xz" ] && continue
        rm -rf "$t/src" "$t/out"; mkdir -p "$t/src" "$t/out"
        tar -xJf "$f" -C "$t/src"
        rm -rf "$t/src/.cargo"
        if (cd "$t" && CARGO_HOME="$t/cargo-home" cargo vendor --locked --versioned-dirs \
                --manifest-path "$t/src/Cargo.toml" "$t/out/vendor" >/dev/null); then
            tar -C "$t/out" -cf - vendor | xz -T0 -6 > "$ex/$name-cargo-vendor.tar.xz"
            echo "cargo vendor --locked of $(basename "$f")" > "$ex/$name-cargo-vendor.tar.xz.commit"
        else
            echo "FAILED cargo vendor for $name" >&2; ok=1
        fi
    done
    rm -rf "$t"
    return "$ok"
}

# manifest <dir>: one line per file with its sha256 and the pin it answers
# for, so the per-file .commit notes stay out of the upload.
manifest() {
    local dir="$1" f
    (
        cd "$dir"
        for f in *.tar.xz *.asc deps/*.tar.xz deps/*.tar.gz deps/PINS.tsv; do
            [ -f "$f" ] || continue
            printf '%s  %s  %s\n' "$(sha "$f")" "$f" "$(cat "$f.commit" 2>/dev/null || echo -)"
        done
    ) > "$dir/SHA256SUMS.txt"
}

case "${1:-}" in
    tangent-linux) THIRD_PARTY_SRC="$OUT_ROOT" "$ROOT/scripts/build-ffmpeg-linux.sh" source ;;
    jvs-7.0.2) jvs ;;
    btbn-n8.1.3) btbn ;;
    all) THIRD_PARTY_SRC="$OUT_ROOT" "$ROOT/scripts/build-ffmpeg-linux.sh" source; btbn ;;
    *) echo "usage: scripts/fetch-ffmpeg-sources.sh <tangent-linux|btbn-n8.1.3|all|jvs-7.0.2>" >&2; exit 2 ;;
esac
