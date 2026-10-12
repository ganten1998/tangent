#!/usr/bin/env bash
# Fetch every download link the store page and the supporter email hand out.
#
#   scripts/check-store-links.sh
#   scripts/check-store-links.sh --names   # the asset names only; fetches nothing
#
# These links resolve by EXACT asset name against whatever release is currently
# latest, so a rename or a new name that has not shipped yet is a 404 in the
# two places a buyer actually meets Tangent: the post-purchase page and the one
# email they keep.
#
# It has gone wrong twice. Once by redeploying the key email ahead of its
# assets, and once by writing 3.0.0's installer names into STORE-CONTENT.md
# while 2.3.0 was still the latest release. Both times the text was right and
# the release was not, and both times a human had to notice.
set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

# What a buyer meets TODAY. Deliberately not the release-email draft: that one
# is written to be sent after publishing, so its links are supposed to 404
# until then, and it carries its own checklist.
SOURCES=(docs/STORE-CONTENT.md tools/tangent-fulfil/src/main.rs README.md)

# Fenced code blocks are skipped. STORE-CONTENT.md documents the swap to the
# 3.0.0 installer links inside a fence, and those are instructions for later
# rather than links anyone can click now — flagging them would make this script
# cry wolf, and a check that always fails is a check nobody runs.
URLS="$(
  for f in "${SOURCES[@]}"; do
    awk '/^```/ { fenced = !fenced; next } !fenced' "$f"
  done | grep -ohE 'https://github\.com/[^ ")]*/releases/latest/download/[A-Za-z0-9._-]+' | sort -u
)"

if [ -z "$URLS" ]; then
  echo "no download links found in ${SOURCES[*]}" >&2
  exit 1
fi

# `--names`: the asset names those links ask for, one per line, and nothing
# fetched. publish-github.sh refuses a stable publish that would leave one
# of them without an asset, so the list lives here, read the same way.
if [ "${1:-}" = "--names" ]; then
  printf '%s\n' "$URLS" | sed 's#.*/##' | sort -u
  exit 0
fi

# **And the source releases the GPL makes us keep up** (LICENSING.md, "The
# source it is owed"): README.md and the post-purchase page name the
# `third-party-src-*` releases, so a deleted or never-created one fails here,
# the same gate as a dead download link. Swept, never listed by --names: they
# are releases of their own, not assets publish-github.sh uploads.
TAGS="$(
  for f in "${SOURCES[@]}"; do
    awk '/^```/ { fenced = !fenced; next } !fenced' "$f"
  done | grep -ohE 'https://github\.com/[^ ")]*/releases/tag/third-party-src-[A-Za-z0-9._-]+' | sort -u
)"
[ -z "$TAGS" ] || URLS="$URLS
$TAGS"

fail=0
n=0
while IFS= read -r url; do
  [ -z "$url" ] && continue
  n=$((n + 1))
  code="$(curl -s -o /dev/null -w '%{http_code}' -IL --max-time 25 "$url")"
  name="${url##*/}"
  if [ "$code" = "200" ]; then
    printf '  ok    %-34s\n' "$name"
  else
    printf '  %-5s %-34s %s\n' "$code" "$name" "$url"
    fail=1
  fi
done <<< "$URLS"

echo
if [ "$fail" = "0" ]; then
  echo "==> all $n links resolve"
else
  echo "==> SOME LINKS 404. Do not paste this into the store or deploy the" >&2
  echo "    fulfil service until the assets exist on the latest release." >&2
  exit 1
fi
