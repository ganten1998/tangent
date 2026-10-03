#!/usr/bin/env bash
# publish-dropbox.sh — copy built artifacts out of dist/ into Dropbox, so a
# machine that is not this one can fetch them.
#
#   scripts/publish-dropbox.sh                 # everything for this version
#   scripts/publish-dropbox.sh windows         # one platform
#   scripts/publish-dropbox.sh windows macos
#
# **Why this exists at all.** `dist` is meant to be a SYMLINK out of the repo:
#
#     dist -> ~/Library/Caches/tangent-dist
#
# pointed there for the same reason `target` is — Dropbox syncing build output
# is what produced the "Operation not permitted" linker failures, and
# `com.dropbox.ignored` does not protect subdirectories created later. The
# consequence is the thing this script answers: nothing in dist/ is in
# Dropbox, so nothing in dist/ appears on dropbox.com, and the owner went
# looking for it there and found an empty space where a folder should be.
#
# **And ~/Library/Caches is not storage.** macOS purges it under disk
# pressure, so a build that exists only in dist/ can vanish with no warning —
# `install-macos.sh` already assumes exactly that and carries a
# restore-from-zip path. What this copies is the durable copy.
#
# The destination carries the COMMIT, not just the version: every build
# between releases is `5.0.0-dev`, so a version-named folder would be
# overwritten by the next one and a tester could never say which build they
# were on. `tangent-windows-verification-ee6c2ce.md` already set that habit.
#
# **And it is the commit that BUILT the files, not the one checked out now.**
# This used to name the folder after HEAD at publish time and copy every
# same-version file in dist/, so `builds/c027116/` (2026-09-22) carried an
# ARCH=host arm64 zip and dmg from four days and 84 commits earlier. Every
# build script now stamps each artifact with the tree it was built from
# (scripts/build-stamp.sh), and only a file whose stamp is this tree's goes:
# HEAD's tree under `<HEAD>`, the uncommitted working state's under
# `<HEAD>-dirty`. Anything else is left out, and the run says why.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

VERSION="$(grep '^version' Cargo.toml | head -1 | sed -E 's/.*"([^"]+)".*/\1/')"
COMMIT="$(git rev-parse --short HEAD)"

# The Dropbox root, asked of the filesystem rather than assumed: the modern
# path is under CloudStorage and the old one is a plain ~/Dropbox, and which
# one a Mac has depends on when the client was installed.
DBOX=""
for c in "$HOME/Library/CloudStorage/Dropbox" "$HOME/Dropbox"; do
  [ -d "$c" ] && { DBOX="$c"; break; }
done
[ -n "$DBOX" ] || { echo "error: no Dropbox folder found (looked in ~/Library/CloudStorage/Dropbox and ~/Dropbox)." >&2; exit 1; }

want=("$@")
[ ${#want[@]} -eq 0 ] && want=(windows macos linux)

# Named by platform, so `publish-dropbox.sh windows` means something. Globs
# rather than a file list, because a hand-maintained list silently drops
# whatever was added last — the same trap build-cross.sh names about the
# readme going missing from the macOS artifacts.
# **Globbed against dist/ itself**, not against a bare name later prefixed
# with it. Written the other way first, and the wildcards then expanded in
# the REPO root where nothing matches — so bash left the pattern as a
# literal, `[ -f ]` refused it, and `publish-dropbox.sh linux` reported
# "nothing to publish" with two tarballs sitting in dist/. Windows survived
# only because its two names carry no wildcard, which is exactly the shape
# of bug that ships.
shopt -s nullglob
files=()
for p in "${want[@]}"; do
  case "$p" in
    windows) pats=(dist/"Tangent-${VERSION}-windows-setup.exe" dist/"tangent-${VERSION}-windows-x86_64.zip") ;;
    macos)   pats=(dist/"Tangent-${VERSION}-macos-"*.dmg dist/"Tangent-${VERSION}-macos-"*.zip) ;;
    linux)   pats=(dist/"tangent-${VERSION}-linux-"*.tar.gz) ;;
    *) echo "error: unknown platform '$p' (want: windows, macos, linux)" >&2; exit 1 ;;
  esac
  # `${pats[@]+"${pats[@]}"}` and not `"${pats[@]}"`: under `set -u`, bash
  # 3.2 - which is what macOS ships, and what `env bash` finds here - treats
  # an EMPTY array expansion as an unbound variable and aborts, and nullglob
  # empties this one for a platform with nothing in dist/. The default run
  # asks for all three, so one platform not built stopped it before anything
  # was copied. publish-github.sh names the same trap.
  before=${#files[@]}
  for f in ${pats[@]+"${pats[@]}"}; do
    [ -f "$f" ] && files+=("$f")
  done
  [ ${#files[@]} -gt "$before" ] || echo "    ($p: nothing in dist/)" >&2
done
shopt -u nullglob

if [ ${#files[@]} -eq 0 ]; then
  echo "error: nothing to publish — no artifacts for $VERSION in dist/." >&2
  echo "       Build first: scripts/build-macos.sh, scripts/build-cross.sh" >&2
  exit 1
fi

# **Only what this tree built** (the header says why). A file built from
# HEAD's tree is published as <HEAD> even if you have started editing since;
# one built from the working state you have now is published as
# <HEAD>-dirty; the two are never mixed in one folder, because that folder
# would describe neither. Skipped files are named, with the reason.
HEAD_TREE="$(git rev-parse 'HEAD^{tree}')"
WORK_TREE="$(scripts/build-stamp.sh tree)" \
  || { echo "error: could not read the working tree's state (scripts/build-stamp.sh tree)." >&2; exit 1; }
from_head=(); from_work=()
for f in "${files[@]}"; do
  st=0; built="$(scripts/build-stamp.sh read "$f")" || st=$?
  if [ "$st" = 0 ] && [ "${built%% *}" = "$HEAD_TREE" ]; then
    from_head+=("$f")
  elif [ "$st" = 0 ] && [ "${built%% *}" = "$WORK_TREE" ]; then
    from_work+=("$f")
  else
    case "$st" in
      0) why="built from another tree (HEAD was ${built#* } when it was stamped)" ;;
      1) why="no build stamp (built before stamps, or by hand)" ;;
      *) why="changed since it was stamped" ;;
    esac
    echo "    (skipping $(basename "$f"): $why)" >&2
  fi
done
if [ ${#from_head[@]} -gt 0 ] && [ ${#from_work[@]} -gt 0 ]; then
  echo "error: some of these were built from $COMMIT and some from your uncommitted changes:" >&2
  for f in "${from_head[@]}"; do echo "    ${f##*/}  (built from $COMMIT)" >&2; done
  for f in "${from_work[@]}"; do echo "    ${f##*/}  (built from uncommitted changes)" >&2; done
  echo "       One folder cannot be both: publish one platform at a time, or rebuild." >&2
  exit 1
fi
DIRTY=""
if [ ${#from_head[@]} -gt 0 ]; then
  files=("${from_head[@]}")
elif [ ${#from_work[@]} -gt 0 ]; then
  files=("${from_work[@]}")
  # **A dirty build is published under a name that says so**, rather than
  # refused: handing a tester a build from uncommitted work is a real thing
  # to want, and a build nobody can trace back to a commit is not.
  DIRTY="-dirty"
  echo "NOTE: built from uncommitted changes — publishing as ${COMMIT}${DIRTY}." >&2
else
  echo "error: nothing to publish — nothing in dist/ was built from this tree." >&2
  echo "       Rebuild: scripts/build-macos.sh, scripts/build-cross.sh" >&2
  exit 1
fi
DEST="$DBOX/Tangent Development/builds/${COMMIT}${DIRTY}"

# **The release's own checks, on what testers get.** release.sh refuses a
# `-dev` version before it opens a single file, so a tester build used to
# pass no gate at all: a PE without a signature (which Smart App Control
# refuses to start, with no override) or a WASAPI-only Windows build went
# straight out. scripts/check-artifacts.sh is the one copy of those checks,
# and release.sh runs it too. SKIP_CHECKS=1 publishes anyway, and must be
# typed.
if [ "${SKIP_CHECKS:-0}" = 1 ]; then
  echo "NOTE: SKIP_CHECKS=1 — publishing WITHOUT the artifact checks." >&2
elif ! scripts/check-artifacts.sh "$VERSION" "${files[@]}"; then
  echo "refusing: the build above fails the release's own checks (the !! lines)." >&2
  echo "  Fix the build, or SKIP_CHECKS=1 to publish it anyway, on purpose." >&2
  exit 1
fi

mkdir -p "$DEST"
# **The INSTALLER first**, and that is not cosmetic: Dropbox uploads in the
# order files land, and a 50 MB exe that arrives before a 67 MB zip is a
# tester unblocked several minutes sooner. The owner asked for it in that
# order for exactly that reason.
IFS=$'\n' files=($(printf '%s\n' "${files[@]}" | sort -t/ -k2 | awk '/setup\.exe$/{print;next}{o=o $0 "\n"}END{printf "%s",o}'))
unset IFS

for f in "${files[@]}"; do
  cp "$f" "$DEST/"
  # dist/ files can be mode 600 (the notarisation staple rewrites them that
  # way); a file another machine has to download should be readable.
  chmod 644 "$DEST/$(basename "$f")"
  echo "    $(basename "$f")"
done

# A checksum file, so "did the download finish" is answerable from the other
# end without asking anybody.
(cd "$DEST" && rm -f SHA256SUMS.txt && shasum -a 256 -- * > SHA256SUMS.txt)

echo "==> $DEST"
ls -lh "$DEST" | tail -n +2 | awk '{print "    " $5, $9}'
echo "    (Dropbox uploads in the background; give it a minute per 50 MB.)"
