#!/usr/bin/env bash
# Publish this version's artifacts to the GitHub release: every artifact under
# its version-scoped name, a version-less alias for each name a permanent
# download link resolves through, and one SHA256SUMS.
#
# Usage:
#   scripts/publish-github.sh --notes-file <file>   # create the release
#   scripts/publish-github.sh                       # assets only, release exists
#   DRY_RUN=1 scripts/publish-github.sh             # print, upload nothing
#
# WHY THE ALIASES EXIST — read before "tidying" them away.
# The permalink https://github.com/<repo>/releases/latest/download/<name>
# matches by EXACT asset name against whatever release is currently latest.
# The Gumroad post-purchase page, the supporter-key email and README.md all
# link through it, using the version-less names:
#
#   Tangent-macos.pkg             Tangent-windows-setup.exe
#   Tangent-macos-universal.dmg   Tangent-macos-universal.zip
#   tangent-windows-x86_64.zip   tangent-linux-x86_64.tar.gz   SHA256SUMS
#
# Ship a release carrying only `Tangent-5.1.0-macos-universal.dmg` and every
# one of those links 404s for everybody, including people who have already
# paid, with nothing in any log to say so. That is the whole reason this script
# exists: uploading the aliases is not optional polish, it is the release
# contract.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

NOTES_ARGS=()
case "${1:-}" in
  "")           ;;
  --notes-file) [ -n "${2:-}" ] || { echo "--notes-file needs a path" >&2; exit 2; }
                NOTES_ARGS=(--notes-file "$2") ;;
  *) echo "unknown argument: $1 (expected --notes-file <file>)" >&2; exit 2 ;;
esac

command -v gh >/dev/null 2>&1 || { echo "gh CLI not installed" >&2; exit 2; }

# The repo is the GitHub mirror, never `origin` (that is Codeberg).
REMOTE_URL="$(git remote get-url github 2>/dev/null || true)"
[ -n "$REMOTE_URL" ] || { echo "no 'github' remote — see docs/RELEASE.md" >&2; exit 2; }
REPO="$(printf '%s' "$REMOTE_URL" | sed -E 's#.*github\.com[:/]##; s#\.git$##')"

VERSION="$(grep '^version' Cargo.toml | head -1 | sed -E 's/.*"([^"]+)".*/\1/')"
if ! printf '%s' "$VERSION" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+([-+][0-9A-Za-z.-]+)?$'; then
  echo "could not read a sane version from Cargo.toml (got '$VERSION')" >&2
  exit 2
fi
TAG="v${VERSION}"

# A version carrying a pre-release suffix ("2.3.0-beta.1") is published as a
# GitHub PRE-RELEASE, and that is not cosmetic.
#
# `releases/latest/download/<name>` resolves against the latest NON-prerelease,
# so a beta marked correctly leaves every permanent link pointing at the last
# stable build. Publish the same beta as a normal release and every one of
# those links — the ones in every supporter-key email already sent, on the
# Gumroad post-purchase page, and in the README — silently starts handing a
# beta to people who paid for a finished app.
#
# For the same reason a pre-release uploads ONLY version-scoped names. The
# version-less aliases are the permanent-link contract, and a beta has no
# business owning them even by accident.
PRERELEASE=0
case "$VERSION" in *-*) PRERELEASE=1 ;; esac

echo "==> Tangent $VERSION -> $REPO $TAG"
[ "$PRERELEASE" = 1 ] && echo "    PRE-RELEASE: version-scoped assets only, permalinks stay on the last stable"

RELEASE_EXISTS=0
gh release view "$TAG" --repo "$REPO" >/dev/null 2>&1 && RELEASE_EXISTS=1

# Only a NEW release needs the tag to exist here first. Re-uploading assets to a
# published release does not, and `gh release create` will happily mint a tag on
# the remote that this clone has never seen — which is how v2.2.0 came to exist
# on GitHub but nowhere else. If that has happened, say so rather than fail.
if [ "$RELEASE_EXISTS" = 0 ]; then
  git rev-parse -q --verify "refs/tags/${TAG}" >/dev/null 2>&1 \
    || { echo "tag ${TAG} does not exist locally — docs/RELEASE.md step 8" >&2; exit 1; }
elif ! git rev-parse -q --verify "refs/tags/${TAG}" >/dev/null 2>&1; then
  echo "    NOTE: ${TAG} exists on GitHub but not in this clone — it was never"
  echo "          tagged locally or pushed to Codeberg (docs/RELEASE.md step 8)"
fi

# ── Collect this version's artifacts ─────────────────────────────────────────
# FreeBSD's pkg spells a prerelease its own way (5.0.0-rc.1 is 5.0.0.r1); the
# same expression release.sh and build-freebsd.sh use, so the three agree.
PKG_VERSION="$(printf '%s' "$VERSION" | sed -E 's/-beta\.?/.b/; s/-alpha\.?/.a/; s/-rc\.?/.r/; s/-/./g')"
shopt -s nullglob
ARTIFACTS=()
# Tangent-* since the 2.3.0 rename. This glob kept the pre-rename prefix long
# after build-macos.sh stopped producing that name, so the script exited "no
# artifacts in dist/" on the first release it was asked to publish.
for f in dist/Tangent-"${VERSION}"-macos-*.zip dist/Tangent-"${VERSION}"-macos-*.dmg \
         dist/tangent-"${VERSION}"-linux-*.tar.gz dist/tangent-"${VERSION}"-windows-*.zip \
         dist/Tangent-"${VERSION}"-macos.pkg dist/Tangent-"${VERSION}"-windows-setup.exe \
         dist/tangent-"${PKG_VERSION}"-freebsd-*.pkg dist/tangent-"${PKG_VERSION}"-freebsd-*.tar.gz; do
  [ -f "$f" ] && ARTIFACTS+=("$f")
done
shopt -u nullglob
if [ "${#ARTIFACTS[@]}" -eq 0 ]; then
  echo "no $VERSION artifacts in dist/ — run scripts/release.sh first" >&2
  exit 1
fi
# Pick the checksum file, then PROVE it describes this version. `dist/` keeps a
# bare SHA256SUMS that a previous release wrote, so "the file exists" means
# nothing — publishing 2.1.0's sums alongside 2.3.0's binaries is worse than
# publishing none, because it reads as a failed integrity check.
SUMS="dist/SHA256SUMS-${VERSION}"
[ -f "$SUMS" ] || SUMS="dist/SHA256SUMS"
[ -f "$SUMS" ] || { echo "no checksum file in dist/ — scripts/release.sh --sums-only" >&2; exit 1; }
for a in "${ARTIFACTS[@]}"; do
  grep -Fq "  ${a#dist/}" "$SUMS" \
    || { echo "$SUMS does not cover ${a#dist/} (stale?) — scripts/release.sh --sums-only" >&2; exit 1; }
done
# **And the other way round: every file the sums name goes up with them.** A
# user checks a download with `shasum -c SHA256SUMS`, which fails on any line
# whose file was never published; the Windows PDB rode in exactly that way,
# summed by release.sh and uploaded by nobody.
while read -r _ named; do
  [ -n "$named" ] || continue
  named="${named#\*}"   # a binary-mode sum marks the name with `*`
  hit=0
  for a in "${ARTIFACTS[@]}"; do [ "${a#dist/}" = "$named" ] && { hit=1; break; }; done
  [ "$hit" = 1 ] || {
    echo "$SUMS names $named, which this publish does not upload: a user's" >&2
    echo "  \`shasum -c\` would fail on it. Rerun scripts/release.sh --sums-only." >&2
    exit 1
  }
done < "$SUMS"
echo "==> Checksums: $SUMS"

# ── Build the alias set in a temp dir ────────────────────────────────────────
# **A version-less copy only where a permanent link asks for one.** The links
# in README.md, the store page and the supporter email resolve by exact name
# against the latest stable release (`check-store-links.sh --names` reads all
# three), so `Tangent-5.1.0-macos-universal.dmg` also goes up as
# `Tangent-macos-universal.dmg` because a link names that, and the Linux
# aarch64 tarball, the Windows zip and the FreeBSD files go up once, under
# their versioned names, because nothing links them bare. 5.1.0 first
# carried a bare copy of every artifact and of the checksums, the FreeBSD
# complete bundle's 489 MB among them, that no link reached. The checksums
# go up once, as `SHA256SUMS`: the release they sit in is the version.
STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT

NEED=""
if [ "$PRERELEASE" = 0 ]; then
  NEED="$(scripts/check-store-links.sh --names)" \
    || { echo "could not read the permanent links (scripts/check-store-links.sh --names)" >&2; exit 1; }
fi
needed() { printf '%s\n' "$NEED" | grep -Fqx -- "$1"; }

UPLOADS=()
if [ "$PRERELEASE" = 1 ]; then
  UPLOADS=("${ARTIFACTS[@]}")
  cp "$SUMS" "$STAGE/SHA256SUMS-${VERSION}"
  UPLOADS+=("$STAGE/SHA256SUMS-${VERSION}")
fi
for a in "${ARTIFACTS[@]}"; do
  [ "$PRERELEASE" = 1 ] && break
  base="${a#dist/}"
  UPLOADS+=("$a")
  alias_name="${base/-${VERSION}-/-}"
  [ "$alias_name" != "$base" ] && needed "$alias_name" || continue
  cp "$a" "$STAGE/$alias_name"
  UPLOADS+=("$STAGE/$alias_name")
done
if [ "$PRERELEASE" = 0 ]; then
  cp "$SUMS" "$STAGE/SHA256SUMS"
  UPLOADS+=("$STAGE/SHA256SUMS")
fi

# **No third spelling.** Every asset also went up under its pre-rename name,
# and the universal Mac build under the arm64 names it had before 3.0.0, so
# that links in old supporter emails kept working. The owner retired both
# sets: 5.1.0 carries neither, and those old links 404.

echo "==> ${#UPLOADS[@]} assets (${#ARTIFACTS[@]} artifacts, a version-less copy where a permanent link names one)"
for u in "${UPLOADS[@]}"; do echo "    $(basename "$u")"; done

# **Every permanent link a buyer holds gets its asset.** The links in
# README.md, the store page and the supporter email resolve by exact name
# against the latest stable release, and the check after the upload only
# asks about what WAS uploaded: a release built without its Linux tarball
# would publish "OK" and 404 tangent-linux-x86_64.tar.gz for everyone who
# had paid. check-store-links.sh reads those three sources; nothing here
# keeps a second list of the names.
if [ "$PRERELEASE" = 0 ]; then
  MISSING=""
  while IFS= read -r name; do
    [ -n "$name" ] || continue
    hit=0
    for u in "${UPLOADS[@]}"; do
      if [ "$(basename "$u")" = "$name" ]; then hit=1; break; fi
    done
    [ "$hit" = 1 ] || MISSING="$MISSING $name"
  done <<< "$NEED"
  if [ -n "$MISSING" ]; then
    echo "refusing: this release has no asset for these permanent links:" >&2
    for m in $MISSING; do echo "    https://github.com/${REPO}/releases/latest/download/$m" >&2; done
    echo "  README.md, docs/STORE-CONTENT.md or the supporter email hands them out." >&2
    echo "  Build what is missing (scripts/release.sh names it), then publish." >&2
    exit 1
  fi
fi

if [ "${DRY_RUN:-0}" = "1" ]; then
  echo "==> DRY_RUN=1, nothing uploaded"
  exit 0
fi

# ── Create or update the release ─────────────────────────────────────────────
if [ "$RELEASE_EXISTS" = 1 ]; then
  echo "==> Release $TAG exists — uploading assets (--clobber)"
  if [ "${#NOTES_ARGS[@]}" -gt 0 ]; then
    echo "    NOTE: release exists, ignoring --notes-file (edit notes in the web UI)"
  fi
else
  if [ "${#NOTES_ARGS[@]}" -eq 0 ]; then
    echo "release $TAG does not exist and no --notes-file given" >&2
    exit 2
  fi
  echo "==> Creating release $TAG"
  # `${a[@]+"${a[@]}"}` and not `"${a[@]}"`: under `set -u`, bash 3.2 — which is
  # what macOS ships — treats an EMPTY array expansion as an unbound variable
  # and aborts. That is exactly the stable-release path, where PRE_ARGS is
  # empty, so the bug could only ever fire on a real release.
  PRE_ARGS=()
  [ "$PRERELEASE" = 1 ] && PRE_ARGS=(--prerelease)
  gh release create "$TAG" --repo "$REPO" --title "Tangent ${VERSION}" \
    ${PRE_ARGS[@]+"${PRE_ARGS[@]}"} ${NOTES_ARGS[@]+"${NOTES_ARGS[@]}"}
fi

gh release upload "$TAG" --repo "$REPO" --clobber "${UPLOADS[@]}"

# ── Prove the permalinks a customer will click actually resolve ──────────────
# Checking the release's own asset list is not enough: `latest` is a separate
# pointer, and a draft or a newer release moves it.
echo "==> Verifying /releases/latest/download/ permalinks"
FAIL=0
for u in "${UPLOADS[@]}"; do
  name="$(basename "$u")"
  case "$name" in *"-${VERSION}"*) continue ;; esac   # aliases only
  url="https://github.com/${REPO}/releases/latest/download/${name}"
  # A ranged GET for one byte, NOT curl -I. The redirect lands on GitHub's
  # asset CDN, which answers HEAD with an intermittent 503 even when the asset
  # is fine — that false alarm costs an hour of chasing a non-bug.
  code="$(curl -sL -r 0-0 -o /dev/null -w '%{http_code}' "$url" || echo 000)"
  if [ "$code" = "200" ] || [ "$code" = "206" ]; then
    echo "    ok   $name"
  else
    echo "    !!   $name -> HTTP $code  ($url)"
    FAIL=1
  fi
done

if [ "$FAIL" = 0 ]; then
  echo "==> OK. Every permanent download link resolves to $VERSION."
else
  echo "==> PERMALINKS BROKEN — customers' download links are dead. Fix before announcing." >&2
  exit 1
fi
