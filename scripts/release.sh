#!/usr/bin/env bash
# Orchestrate one Tangent release build on THIS host: verify the version, purge
# artifacts left over from other versions, run whichever build scripts this OS
# can run, sanity-check the results, and write a version-scoped SHA256SUMS.
#
# Usage:
#   scripts/release.sh                  # build everything this host can
#   ARCH=host scripts/release.sh        # macOS: host arch only, for a quick check
#                                       # (releases are universal by default)
#   scripts/release.sh --sums-only      # re-check + re-emit SHA256SUMS only
#   SKIP_WINDOWS=1 scripts/release.sh   # macOS: skip build-cross.sh
#   KEEP_STALE=1 scripts/release.sh     # leave other versions' artifacts in dist/
#   SKIP_TESTS=1 scripts/release.sh     # skip the test/lint gate (scripts/check.sh)
#                                       # — for re-runs where this tree just passed
#
#   macOS host -> scripts/build-macos.sh + scripts/build-installer.sh macos
#                 + scripts/build-cross.sh (Linux tarballs, Windows zip + setup.exe)
#   Linux host -> scripts/build-linux-native.sh
#
# Run it on each OS (and each Linux arch), copy the artifacts into one dist/,
# then `scripts/release.sh --sums-only` there so SHA256SUMS covers everything.
# It builds and checks only — tagging, signing for distribution and uploading
# (docs/RELEASE.md steps 8-10) stay deliberately manual.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

SUMS_ONLY=0
case "${1:-}" in
  "")          ;;
  --sums-only) SUMS_ONLY=1 ;;
  *) echo "unknown argument: $1 (expected --sums-only)" >&2; exit 2 ;;
esac

FAIL=0
note() { echo "!! $*"; FAIL=1; }
sha256() {  # shasum is Perl's and is not on every minimal Linux install
  if command -v shasum >/dev/null 2>&1; then shasum -a 256 "$@"
  else sha256sum "$@"; fi
}

# ── Verify the version ───────────────────────────────────────────────────────
VERSION="$(grep '^version' Cargo.toml | head -1 | sed -E 's/.*"([^"]+)".*/\1/')"
if ! printf '%s' "$VERSION" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+([-+][0-9A-Za-z.-]+)?$'; then
  echo "could not read a sane version from Cargo.toml (got '$VERSION')" >&2
  exit 2
fi

echo "==> Tangent $VERSION  host=$(uname -s)/$(uname -m)"
case "$VERSION" in
  *-*|*+*) echo "    pre-release: artifacts say $VERSION, Info.plist says ${VERSION%%[-+]*}" ;;
esac

# Cargo.lock records the workspace version too; if it disagrees the lockfile was
# never refreshed after the bump and a --locked build would fail.
LOCK_VERSION="$(awk '/^name = "tangent"$/{getline; gsub(/[",]/,""); print $3; exit}' Cargo.lock)"
[ "$LOCK_VERSION" = "$VERSION" ] || note \
  "Cargo.lock says tangent $LOCK_VERSION, Cargo.toml says $VERSION — run 'cargo check' and commit the lockfile"
[ -z "$(git status --porcelain 2>/dev/null)" ] \
  || echo "    NOTE: working tree is dirty — these artifacts match no commit"
git rev-parse -q --verify "refs/tags/v${VERSION}" >/dev/null 2>&1 \
  && echo "    NOTE: tag v${VERSION} exists — you are rebuilding a shipped version"

# ── The refusals ─────────────────────────────────────────────────────────────
#
# **A version is spent the moment a build carries it, and nothing used to stop
# that.** Six numbers went in a single day — beta.10 through beta.15 — none
# tagged, none released, several bumped inline inside a build command. Nothing
# leaked, because the public tags stopped at beta.9; the burn was simply
# invisible until somebody counted.
#
# So `main` carries `X.Y.Z-dev` and only a deliberate release writes a real
# version. These checks are what make that structural rather than a thing to
# remember. They are `exit 2`, not `note` — a warning that can be scrolled
# past is how this happened. Ordered cheap-to-expensive, so a wrong version
# never costs a workspace build.

# 1. The placeholder is not a release. This is the whole point of `-dev`: a
#    build on main cannot claim a number, because the number is not a number.
case "$VERSION" in
  *-dev|*-dev.*)
    echo "refusing: Cargo.toml says $VERSION, which is main's placeholder." >&2
    echo "  Set a real version deliberately, commit it, tag it, then release." >&2
    exit 2 ;;
esac

# 2. The tag must already exist. A release is a thing you can go back to; an
#    artifact built from an untagged commit is a thing you cannot.
if ! git rev-parse -q --verify "refs/tags/v${VERSION}" >/dev/null 2>&1; then
  echo "refusing: no tag v${VERSION}." >&2
  echo "  git tag -a v${VERSION} -m 'Tangent ${VERSION}'   then run this again." >&2
  exit 2
fi

# 3. The changelog must name this version. `publish-github.sh` already refuses
#    to publish without release notes, and CHANGELOG.md had gone stale at 4.5.0
#    — which is why 5.0 could not be published at all. Failing HERE means
#    finding that out before the build rather than after it.
if ! grep -qE "^#+ +\[?${VERSION//./\.}\]?( |\$|	)" CHANGELOG.md 2>/dev/null; then
  echo "refusing: CHANGELOG.md has no heading for ${VERSION}." >&2
  echo "  Add '## ${VERSION} - $(date +%Y-%m-%d)' and what changed under it." >&2
  exit 2
fi

# 4. Pre-releases are rc.N, capped at rc.3. Needing rc.4 means the freeze was
#    not a freeze — the answer is to stop cutting candidates and fix the thing
#    that keeps moving.
case "$VERSION" in
  *-rc.[0-9]*)
    RC_N="${VERSION##*-rc.}"
    if [ "${RC_N%%[!0-9]*}" -gt 3 ] 2>/dev/null; then
      echo "refusing: ${VERSION} is past rc.3." >&2
      echo "  Four candidates means the freeze was not a freeze. Fix, do not cut." >&2
      exit 2
    fi ;;
  *-beta*|*-alpha*)
    echo "    NOTE: pre-releases are rc.N now; $VERSION predates that policy" ;;
esac

# 5. THIRD-PARTY-LICENSES must match Cargo.lock. The build scripts only test
#    that the file EXISTS, so a stale one is bundled forever, and regenerating
#    it was a documented gate that lived in memory (docs/RELEASE.md gate 4) —
#    which is how 5.0 shipped a licence file 400+ commits stale, with the LV2,
#    ASIO and accessibility dependency work entirely unlisted. Regenerate into
#    a temp file and byte-diff; the generator is deterministic given the same
#    Cargo.lock, which is exactly what makes a byte-diff a fair gate.
TPL_TMP="$(mktemp)"
if ! scripts/gen-third-party-licenses.sh "$TPL_TMP" >/dev/null; then
  rm -f "$TPL_TMP"
  echo "refusing: could not regenerate THIRD-PARTY-LICENSES to compare" >&2
  echo "  (scripts/gen-third-party-licenses.sh failed — its output is above)." >&2
  exit 2
fi
if ! diff -q THIRD-PARTY-LICENSES "$TPL_TMP" >/dev/null 2>&1; then
  rm -f "$TPL_TMP"
  echo "refusing: THIRD-PARTY-LICENSES is stale against Cargo.lock." >&2
  echo "  Run scripts/gen-third-party-licenses.sh and commit the result." >&2
  exit 2
fi
rm -f "$TPL_TMP"

# 6. The gate must be green before the build spends the version. The commands
#    are docs/RELEASE.md gate 3 plus the Cargo.toml lint policy under the
#    clippy driver, all in scripts/check.sh — one script, so CI can someday
#    call the same thing instead of restating it in YAML (docs/RELEASE.md's
#    one hard rule for when CI returns). Skipped on --sums-only, which
#    re-checks artifacts that already passed this gate when they were built,
#    and on SKIP_TESTS=1 — which must be TYPED, deliberately, because a
#    warning that can be scrolled past is how versions got burnt.
if [ "$SUMS_ONLY" = 0 ] && [ "${SKIP_TESTS:-0}" != "1" ]; then
  scripts/check.sh || {
    echo "refusing: the release gate is red (scripts/check.sh, above)." >&2
    echo "  Fix it, or SKIP_TESTS=1 to state on the record that you did not." >&2
    exit 2
  }
elif [ "${SKIP_TESTS:-0}" = "1" ]; then
  echo "    NOTE: SKIP_TESTS=1 — the test/lint gate was deliberately skipped"
fi

# ── Purge artifacts belonging to other versions ──────────────────────────────
# Left in place they get swept up by any `Tangent-*` / `tangent-*` glob at checksum
# or upload time. Only the four known artifact name shapes are ever touched.
mkdir -p dist
if [ "${KEEP_STALE:-0}" != "1" ]; then
  shopt -s nullglob
  STALE=()
  for f in dist/Tangent-*-macos-*.zip dist/Tangent-*-macos-*.dmg \
           dist/tangent-*-linux-*.tar.gz dist/tangent-*-windows-*.zip \
           dist/Tangent-*-macos.pkg dist/Tangent-*-windows-setup.exe; do
    case "$f" in *"-${VERSION}-"*) continue ;; esac
    STALE+=("$f")
  done
  shopt -u nullglob
  if [ "${#STALE[@]}" -gt 0 ]; then
    echo "==> Purging ${#STALE[@]} artifact(s) from other versions"
    printf '    rm %s\n' "${STALE[@]}"
    rm -f "${STALE[@]}"
  fi
fi
rm -f dist/SHA256SUMS   # always regenerated below

# ── Build ────────────────────────────────────────────────────────────────────
if [ "$SUMS_ONLY" = 0 ]; then
  case "$(uname -s)" in
    Darwin)
      scripts/build-macos.sh
      # The pkg, immediately after the .app it consumes is written. Everything
      # downstream was already wired for it — the purge glob, the artifact
      # checks, publish-github.sh's upload glob — and the supporter-key email
      # hardcodes its permalink, so a release built without this line shipped
      # every check green and a 404 to exactly the people who paid.
      scripts/build-installer.sh macos
      if [ "${SKIP_WINDOWS:-0}" = "1" ]; then
        echo "==> Skipping build-cross.sh (SKIP_WINDOWS=1)"
      else
        # Its Linux stage is expected to fail on the ALSA cross-build and says
        # so loudly; only a missing Windows zip is a real failure, caught below.
        scripts/build-cross.sh || note "build-cross.sh exited non-zero"
      fi
      ;;
    Linux)
      scripts/build-linux-native.sh
      ;;
    *)
      echo "no build path for $(uname -s); see docs/RELEASE.md" >&2
      exit 2
      ;;
  esac
fi

# ── Inspect what actually got built ──────────────────────────────────────────
# Every artifact must carry the user-facing readme and the licence texts, and a
# Linux tarball without a binary is the exact thing that shipped before 2.1.0.
shopt -s nullglob
ARTIFACTS=()
for f in dist/*-"${VERSION}"-*; do [ -f "$f" ] && ARTIFACTS+=("$f"); done
shopt -u nullglob
if [ "${#ARTIFACTS[@]}" -eq 0 ]; then
  echo "no artifacts for $VERSION in dist/ — nothing to check or checksum" >&2
  exit 1
fi

echo "==> Checking ${#ARTIFACTS[@]} artifact(s)"
for a in "${ARTIFACTS[@]}"; do
  # ── Installers, which carry BOTH payloads and are checked for both ────────
  # An installer that quietly ships only the app is the same class of failure
  # as the empty Linux tarball: it installs, it looks right, and the thing the
  # user came for is not there.
  case "$a" in
    *.pkg)
      exp="$(mktemp -d)"
      if pkgutil --expand "$a" "$exp/x" 2>/dev/null; then
        [ -d "$exp/x/Tangent-app.pkg" ]  || note "$a: no application component"
        [ ! -d "$exp/x/Tangent-vst3.pkg" ] || note "$a: carries the RETIRED VST3 component"
        grep -q 'choice.standalone' "$exp/x/Distribution" 2>/dev/null \
          || note "$a: the installer offers no standalone choice"
        # relocatable="true" would let a stray copy elsewhere on the disk
        # receive the update instead of /Applications.
        grep -q 'relocatable="false"' "$exp/x/Tangent-app.pkg/PackageInfo" 2>/dev/null \
          || note "$a: the app component is RELOCATABLE (it must not be)"
        pkgutil --check-signature "$a" 2>/dev/null | grep -q 'Status: signed' \
          || note "$a: UNSIGNED (needs a Developer ID Installer certificate)"
        # Signed-but-unnotarized passes the grep above and still warns on
        # first open; the staple is a real part of the artifact, so validate
        # it here rather than trust build-installer.sh's log line.
        xcrun stapler validate "$a" >/dev/null 2>&1 \
          || note "$a: signed but NOT stapled (notarization missing or failed)"
      else
        note "$a: could not be expanded — it is not a valid flat package"
      fi
      rm -rf "$exp"
      continue
      ;;
    *setup.exe)
      if command -v 7zz >/dev/null 2>&1; then
        wl="$(7zz l "$a" 2>/dev/null)"
        printf '%s\n' "$wl" | grep -q 'tangent\.exe' \
          || note "$a: no tangent.exe"
        printf '%s\n' "$wl" | grep -q 'VST3/Tangent\.vst3' \
          && note "$a: carries the RETIRED VST3 payload"
        printf '%s\n' "$wl" | grep -q 'Uninstall\.exe' \
          || note "$a: no uninstaller"
      else
        note "$a: not inspected (install 7-zip to check: brew install sevenzip)"
      fi
      # The same refusal the .pkg gets, because SmartScreen judges the OUTER
      # installer no matter what happened to the inner exe. A presence check
      # on the PE's certificate table (scripts/pe-signed.py) rather than a
      # verifier that may not be installed — a gate that only runs where a
      # tool happens to exist is a gate that is off on the machine that cuts
      # releases. Exit 1 is a clean-but-unsigned PE; anything else means the
      # file could not even be read as a PE, which is worse, not better.
      sig=0; python3 scripts/pe-signed.py "$a" >/dev/null 2>&1 || sig=$?
      case "$sig" in
        0) ;;
        1) note "$a: UNSIGNED (Authenticode) — configure TRUSTED_SIGNING_* and rerun (docs/RELEASE.md § Windows)" ;;
        *) note "$a: could not read the PE certificate table (pe-signed.py exit $sig)" ;;
      esac
      continue
      ;;
  esac

  case "$a" in
    *.zip)    list="$(unzip -Z1 "$a")" ;;
    *.tar.gz) list="$(tar -tzf "$a")" ;;
    *)        continue ;;   # .dmg: mounting to inspect is not worth it
  esac
  # (^|/) so an AppleDouble "._LICENSE" can never satisfy the LICENSE check.
  printf '%s\n' "$list" | grep -Eq '(^|/)README\.txt$' || note "$a: no README.txt"
  printf '%s\n' "$list" | grep -Eq '(^|/)LICENSE$'           || note "$a: no LICENSE"
  printf '%s\n' "$list" | grep -Eq '(^|/)OFL\.txt$'          || note "$a: no OFL.txt (font licence)"
  case "$a" in
    *macos*)
      printf '%s\n' "$list" | grep -Eq '^__MACOSX'           && note "$a: carries __MACOSX junk"
      printf '%s\n' "$list" | grep -Eq '/MacOS/tangent$'       || note "$a: no Tangent.app executable"
      ;;
    *windows*)
      printf '%s\n' "$list" | grep -Eq '^tangent\.exe$'        || note "$a: no tangent.exe"
      ;;
    *linux*)
      printf '%s\n' "$list" | grep -Eq '/tangent$'             || note "$a: NO BINARY (the pre-2.1.0 empty-tarball bug)"
      # The 4.92.0 tarball shipped without either of these while its own
      # README told the user to run install.sh. Named parts, not vibes.
      printf '%s\n' "$list" | grep -Eq '/install\.sh$'         || note "$a: no install.sh (README's first instruction)"
      printf '%s\n' "$list" | grep -Eq '/tangent-ffmpeg$'      || note "$a: no bundled encoder (video takes break on clean machines)"
      ;;
  esac
done

# ── Checksums ────────────────────────────────────────────────────────────────
echo "==> Writing dist/SHA256SUMS"
( cd dist && sha256 "${ARTIFACTS[@]#dist/}" > SHA256SUMS )
sed 's/^/    /' dist/SHA256SUMS
ls -lh "${ARTIFACTS[@]}" | sed 's/^/    /'

if [ "$FAIL" = 0 ]; then
  echo "==> OK. Next: docs/RELEASE.md step 6 (smoke test), 8 (tag), 9 (publish)."
else
  echo "==> FINISHED WITH PROBLEMS — see the !! lines above. Do not publish."
  exit 1
fi
