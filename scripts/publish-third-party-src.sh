#!/usr/bin/env bash
# Publish the GPL source for the bundled ffmpeg builds as GitHub releases of
# the repository the binaries are served from (LICENSING.md, "The source it
# is owed").
#
#   DRY_RUN=1 scripts/publish-third-party-src.sh    # says what it would do
#   scripts/publish-third-party-src.sh              # creates and uploads
#   scripts/publish-third-party-src.sh <release-name> ...   # only those
#
# Reads the folders staged under $THIRD_PARTY_SRC (default
# ~/Library/Caches/tangent-dist/third-party-src), one per release:
# third-party-src-ffmpeg-8.1.3-tangent-linux (the Linux builds, staged by
# scripts/build-ffmpeg-linux.sh every time it builds) and
# third-party-src-ffmpeg-n8.1.3-btbn-2026-09-22 (the Windows pin, staged by
# scripts/fetch-ffmpeg-sources.sh). The retired johnvansickle.com 7.0.2 pin's
# folder, third-party-src-ffmpeg-7.0.2, is not in the default list: it can
# never be complete (LICENSING.md); name it to publish it anyway.
#
# **These releases must never become "Latest".** Every download link a buyer
# holds is /releases/latest/download/<name>, resolved against the latest
# release; a source release taking that place would 404 every one of them.
# So each is created as a pre-release (which GitHub never makes Latest) AND
# with --latest=false, and the script checks afterwards that Latest did not
# move.
#
# **Never delete them while the binaries are offered** (the GitHub wipe in
# docs/release/5.0.0/CHECKLIST.md keeps every third-party-src-* tag).
#
# Run it after the repository is public (it refuses a private one: the GPL
# wants the source available to whoever got the binary, and a private
# release 404s for them) and before the release that ships the binaries is
# announced. Re-running is safe: an existing release gets the missing files
# uploaded with --clobber, nothing is deleted.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
REPO="${REPO:-ganten1998/tangent}"
SRC="${THIRD_PARTY_SRC:-$HOME/Library/Caches/tangent-dist/third-party-src}"
DRY="${DRY_RUN:-0}"
RELEASES=(third-party-src-ffmpeg-8.1.3-tangent-linux third-party-src-ffmpeg-n8.1.3-btbn-2026-09-22)
[ "$#" -gt 0 ] && RELEASES=("$@")

run() {
    if [ "$DRY" = 1 ]; then printf '    would run:'; printf ' %q' "$@"; echo; else "$@"; fi
}
sha() { shasum -a 256 "$1" | awk '{print $1}'; }

command -v gh >/dev/null || { echo "gh is not installed" >&2; exit 1; }

# The files a release carries: everything the fetcher staged except its own
# per-file notes (*.commit, *.verified), whose content is in SHA256SUMS.txt.
assets() { # assets <dir>
    find "$1" -type f ! -name '*.commit' ! -name '.*' | sort
}

# Every archive must still match the manifest the fetcher wrote.
verify() { # verify <dir>
    local dir="$1" want file rest bad=0
    [ -s "$dir/SHA256SUMS.txt" ] || { echo "refusing: $dir has no SHA256SUMS.txt (run scripts/fetch-ffmpeg-sources.sh)" >&2; return 1; }
    [ -s "$dir/SOURCE.txt" ] || { echo "refusing: $dir has no SOURCE.txt" >&2; return 1; }
    while read -r want file rest; do
        [ -f "$dir/$file" ] || { echo "  missing $file" >&2; bad=1; continue; }
        [ "$(sha "$dir/$file")" = "$want" ] || { echo "  CHANGED $file" >&2; bad=1; }
    done < "$dir/SHA256SUMS.txt"
    # Asset names are flat on GitHub: two files of one name would overwrite.
    if [ -n "$(assets "$dir" | xargs -n1 basename | sort | uniq -d)" ]; then
        echo "  two files share a name in $dir" >&2; bad=1
    fi
    return "$bad"
}

if [ "$DRY" != 1 ]; then
    vis="$(gh repo view "$REPO" --json visibility -q .visibility)"
    if [ "$vis" != PUBLIC ] && [ "${ALLOW_PRIVATE:-0}" != 1 ]; then
        echo "refusing: $REPO is $vis. Source nobody can download satisfies nothing;" >&2
        echo "  make the repository public first (ALLOW_PRIVATE=1 to rehearse anyway)." >&2
        exit 1
    fi
fi
latest_before="$(gh release view --repo "$REPO" --json tagName -q .tagName 2>/dev/null || echo none)"
echo "==> $REPO, Latest is $latest_before"

for name in "${RELEASES[@]}"; do
    dir="$SRC/$name"
    [ -d "$dir" ] || { echo "refusing: no $dir (run scripts/fetch-ffmpeg-sources.sh)" >&2; exit 1; }
    echo "==> $name"
    verify "$dir" || { echo "refusing: $dir does not match its SHA256SUMS.txt" >&2; exit 1; }
    files=()
    while IFS= read -r f; do files+=("$f"); done < <(assets "$dir")
    total="$(du -ch "${files[@]}" | tail -1 | awk '{print $1}')"
    echo "    ${#files[@]} files, $total"
    title="Source for the bundled ffmpeg (${name#third-party-src-})"
    if gh release view "$name" --repo "$REPO" >/dev/null 2>&1; then
        echo "    release exists: uploading with --clobber"
    else
        run gh release create "$name" --repo "$REPO" --target main \
            --title "$title" --notes-file "$dir/SOURCE.txt" \
            --prerelease --latest=false
    fi
    # In batches, so one failed upload is retried by re-running, not lost
    # in a 900 MB single call.
    i=0
    while [ "$i" -lt "${#files[@]}" ]; do
        run gh release upload "$name" --repo "$REPO" --clobber "${files[@]:i:10}"
        i=$((i + 10))
    done
    if [ "$DRY" != 1 ]; then
        got="$(gh release view "$name" --repo "$REPO" --json assets -q '.assets | length')"
        [ "$got" -ge "${#files[@]}" ] || { echo "FAIL: $name has $got assets, expected ${#files[@]}" >&2; exit 1; }
        code="$(curl -s -o /dev/null -w '%{http_code}' -IL --max-time 25 "https://github.com/$REPO/releases/tag/$name")"
        [ "$code" = 200 ] || { echo "FAIL: https://github.com/$REPO/releases/tag/$name answers $code anonymously" >&2; exit 1; }
    fi
done

if [ "$DRY" != 1 ]; then
    latest_after="$(gh release view --repo "$REPO" --json tagName -q .tagName 2>/dev/null || echo none)"
    if [ "$latest_after" != "$latest_before" ]; then
        echo "FAIL: Latest moved from $latest_before to $latest_after. Every download link" >&2
        echo "  now resolves against it: mark $latest_before Latest again at once." >&2
        exit 1
    fi
    echo "==> OK. Latest is still $latest_after; scripts/check-store-links.sh sweeps the source links."
else
    echo "==> DRY_RUN=1, nothing created or uploaded"
fi
