#!/usr/bin/env bash
# build-stamp.sh — which tree built which file in dist/, so a tester build is
# named after the commit that is really in it.
#
#   scripts/build-stamp.sh tree                # the tree a build starting now is built from
#   scripts/build-stamp.sh write TREE FILE...  # FILE, these exact bytes, was built from TREE
#   scripts/build-stamp.sh read FILE           # "<tree> <head>" of FILE's build; exit 1
#                                              # when it has no stamp, 3 when its stamp
#                                              # describes other bytes
#
# **Why.** Between releases every build is X.Y.Z-dev, so nothing in a file's
# name says which commit made it, and nothing in the binaries does either.
# publish-dropbox.sh named the tester folder after whatever HEAD was when IT
# ran and copied every same-version file in dist/: on 2026-09-22 an ARCH=host
# arm64 zip and dmg from four days earlier went out as c027116, 84 commits
# older than their label. The build scripts stamp each artifact as they
# finish it, and publish-dropbox.sh takes only the files whose stamp is this
# tree's.
#
# **The TREE, not the commit or the clock.** `tree` is the git tree of the
# working state: tracked files as they are on disk, plus untracked ones git
# does not ignore. It is computed in a scratch copy of the index, so the real
# index and the stash are never touched, and blobs are hashed, not written
# (only the small tree objects are). A clean checkout gives HEAD's own tree,
# so a build committed AFTER it was made, with exactly what was built,
# matches the new commit; a build of uncommitted work matches only that
# work, and goes out as <HEAD>-dirty. A timestamp could do neither: 21567be's
# Windows zip was built a minute before its commit existed.
#
# A stamp is dist/.stamps/<the artifact's file name>, outside every
# `*-<version>-*` glob, so no checksum or upload ever sweeps one up. It holds
# the file's sha256, so a file replaced by anything that does not stamp (a
# failed run's leftovers, a copy from another machine) no longer matches.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
STAMPS="$ROOT/dist/.stamps"

usage() { sed -n '5,10p' "$0" | sed 's/^# //' >&2; exit 2; }

sha256_of() {  # shasum is Perl's and is not on every minimal Linux install
  if command -v shasum >/dev/null 2>&1; then shasum -a 256 "$1" | awk '{print $1}'
  elif command -v sha256sum >/dev/null 2>&1; then sha256sum "$1" | awk '{print $1}'
  else sha256 -q "$1"; fi
}

case "${1:-}" in
  tree)
    [ $# -eq 1 ] || usage
    # From the top, because ls-files lists only the directory it runs in.
    # The copied index keeps the stat cache, so only what changed is hashed;
    # with no index to copy, everything is (correct, just slower).
    cd "$ROOT"
    idx="$(mktemp)"
    cp "$(git rev-parse --git-path index)" "$idx" 2>/dev/null || rm -f "$idx"
    rc=0
    {
      GIT_INDEX_FILE="$idx" git ls-files -z -m -o --exclude-standard \
        | GIT_INDEX_FILE="$idx" git update-index -z --add --remove --info-only --stdin \
        && GIT_INDEX_FILE="$idx" git write-tree --missing-ok
    } || rc=$?
    rm -f "$idx"
    exit "$rc"
    ;;
  write)
    [ $# -ge 3 ] || usage
    tree="$2"
    shift 2
    printf '%s' "$tree" | grep -Eq '^[0-9a-f]{40}([0-9a-f]{24})?$' \
      || { echo "build-stamp: '$tree' is not a tree id" >&2; exit 2; }
    head="$(git -C "$ROOT" rev-parse --short HEAD 2>/dev/null || echo unknown)"
    mkdir -p "$STAMPS"
    for f in "$@"; do
      [ -f "$f" ] || { echo "build-stamp: no such file: $f" >&2; exit 1; }
      s="$STAMPS/$(basename "$f")"
      {
        echo "tree $tree"
        echo "sha256 $(sha256_of "$f")"
        echo "head $head"
        echo "built $(date -u +%Y-%m-%dT%H:%M:%SZ)"
      } > "$s.new"
      mv -f "$s.new" "$s"
    done
    # A stamp whose file is gone (purged, or never rebuilt) goes with it.
    for s in "$STAMPS"/*; do
      [ -e "$s" ] || continue
      [ -e "$ROOT/dist/$(basename "$s")" ] || rm -f "$s"
    done
    ;;
  read)
    [ $# -eq 2 ] || usage
    s="$STAMPS/$(basename "$2")"
    [ -f "$s" ] || exit 1
    [ -f "$2" ] && [ "$(sed -n 's/^sha256 //p' "$s")" = "$(sha256_of "$2")" ] || exit 3
    printf '%s %s\n' "$(sed -n 's/^tree //p' "$s")" "$(sed -n 's/^head //p' "$s")"
    ;;
  *) usage ;;
esac
