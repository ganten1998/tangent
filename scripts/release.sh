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
#   FreeBSD host -> scripts/build-freebsd.sh (the .pkg always; the two offline
#                 bundles only as root or with doas/sudo, which the check
#                 below turns into a problem on the record when they are missing)
#
# Run it on each OS (and each Linux arch), copy the artifacts into one dist/,
# then `scripts/release.sh --sums-only` there so SHA256SUMS covers everything.
# It builds and checks only. Tagging comes FIRST (refusal 2); signing is not
# manual: build-macos.sh and build-installer.sh sign and notarize, and
# build-cross.sh signs every Windows PE; pushing, publishing and verifying
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
  elif command -v sha256sum >/dev/null 2>&1; then sha256sum "$@"
  # FreeBSD base has neither; its sha256(1) prints "hash file" with ONE space
  # under -r, and `sha256sum -c` wants two. `command` so this is the tool and
  # not this function calling itself.
  else command sha256 -r "$@" | sed 's/ /  /'; fi
}

# ── FreeBSD's spelling of the version ────────────────────────────────────────
# pkg(8) uses the hyphen to split a package NAME from its VERSION in a
# filename, so build-freebsd.sh writes 5.0.0-rc.1 as 5.0.0.r1 (FreeBSD's own
# prerelease convention, which also sorts before the release). The SAME sed
# expression, character for character, so this script and that one cannot
# disagree about what a FreeBSD artifact is called; scripts/check.sh holds
# the three copies (build, release, publish) byte-identical.
freebsd_version() { printf '%s' "$1" | sed -E 's/-beta\.?/.b/; s/-alpha\.?/.a/; s/-rc\.?/.r/; s/-/./g'; }

# ── Verify the version ───────────────────────────────────────────────────────
VERSION="$(grep '^version' Cargo.toml | head -1 | sed -E 's/.*"([^"]+)".*/\1/')"
if ! printf '%s' "$VERSION" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+([-+][0-9A-Za-z.-]+)?$'; then
  echo "could not read a sane version from Cargo.toml (got '$VERSION')" >&2
  exit 2
fi

PKG_VERSION="$(freebsd_version "$VERSION")"

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

# 3b. The Welcome card must greet this version. Every new version opens it
#     once, whatever its box says (`Settings::welcome_seen`, the owner's ruling
#     of 2026-10-19), so a card still greeting the last one is the first thing
#     every upgrader reads. Its first line names the major and the minor.
GREETS="${VERSION%%-*}"
GREETS="${GREETS%.*}"
if ! grep -qF "\"Welcome to Tangent ${GREETS}!\"" tangent-ui/src/dialogs.rs 2>/dev/null; then
  echo "refusing: the Welcome card does not greet ${GREETS}." >&2
  echo "  Update WELCOME_LINES in tangent-ui/src/dialogs.rs: its greeting and what is new." >&2
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
#    it was a documented gate that lived in memory (then docs/RELEASE.md gate
#    4; gate 2 now) —
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
#    are docs/RELEASE.md gate 1, the Minimal build, the crate firewall and the
#    Cargo.toml lint policy under the clippy driver, all in scripts/check.sh:
#    one script, so CI can someday call the same thing instead of restating
#    it in YAML (docs/RELEASE.md's one hard rule for when CI returns).
#    Skipped on --sums-only, which re-checks artifacts that already passed
#    this gate when they were built, and on SKIP_TESTS=1, which must be
#    TYPED, deliberately, because a warning that can be scrolled past is how
#    versions got burnt.
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
# or upload time. Only the known artifact name shapes are ever touched — and a
# FreeBSD artifact is known by pkg's spelling of the version, not Cargo's.
mkdir -p dist
if [ "${KEEP_STALE:-0}" != "1" ]; then
  shopt -s nullglob
  STALE=()
  for f in dist/Tangent-*-macos-*.zip dist/Tangent-*-macos-*.dmg \
           dist/tangent-*-linux-*.tar.gz dist/tangent-*-windows-*.zip \
           dist/Tangent-*-macos.pkg dist/Tangent-*-windows-setup.exe \
           dist/tangent-*-freebsd-*.pkg dist/tangent-*-freebsd-*.tar.gz; do
    case "$f" in *"-${VERSION}-"*|*"-${PKG_VERSION}-"*) continue ;; esac
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
        # A leg that fails (a Linux tarball, the installer) lets the others
        # run and then exits non-zero; the set check below names what is
        # missing, and a missing one is a red run.
        scripts/build-cross.sh || note "build-cross.sh exited non-zero"
      fi
      ;;
    Linux)
      scripts/build-linux-native.sh
      ;;
    FreeBSD)
      # The .pkg always. The two offline bundles need `pkg fetch`, which
      # needs root: without it the script says so and exits 0, and the
      # bundle check below is what puts that on the record as a problem.
      scripts/build-freebsd.sh
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
# **Not the PDB.** build-cross.sh keeps the Windows symbols beside the
# deliverables under the build's own name, so the glob finds them, and they
# are never uploaded: summed here, they made every user's `shasum -c
# SHA256SUMS` report a file that is not there, which reads as a failed
# integrity check on a clean download.
for f in dist/*-"${VERSION}"-*; do
  case "$f" in *.pdb) continue ;; esac
  [ -f "$f" ] && ARTIFACTS+=("$f")
done
# The FreeBSD artifacts wear pkg's spelling, which is only the same string
# for a final release; a prerelease's are found by their own name.
if [ "$PKG_VERSION" != "$VERSION" ]; then
  for f in dist/tangent-"${PKG_VERSION}"-freebsd-*; do [ -f "$f" ] && ARTIFACTS+=("$f"); done
fi
shopt -u nullglob
if [ "${#ARTIFACTS[@]}" -eq 0 ]; then
  echo "no artifacts for $VERSION in dist/ — nothing to check or checksum" >&2
  exit 1
fi
# A FreeBSD release is the .pkg AND both bundles — the small one is for the
# person this port exists for. Only judged on the host that builds them.
if [ "$(uname -s)" = FreeBSD ] && [ "$SUMS_ONLY" = 0 ]; then
  for kind in desktop complete; do
    ls dist/tangent-"${PKG_VERSION}"-freebsd-*-"$kind".tar.gz >/dev/null 2>&1 \
      || note "no $kind bundle for $PKG_VERSION (build-freebsd.sh needs root or doas to fetch it)"
  done
fi
# **And the set the Mac builds, every piece of it.** Only files that exist
# are checked below, so a leg that produced nothing used to pass unseen: a
# failed Linux leg gave a release with no Linux tarball, every check green,
# and a `releases/latest/download` link in the README and in every
# supporter email that 404ed. **The dmg is required here too.**
# build-macos.sh still treats it as best-effort, so a tester build is not
# lost to a flaky hdiutil, but README.md links Tangent-macos-universal.dmg
# and publish-github.sh refuses a stable release without an asset for every
# permanent link: a release missing it could be built "OK" and then not be
# publishable. SKIP_WINDOWS=1 waives the four the cross build makes.
if [ "$(uname -s)" = Darwin ] && [ "$SUMS_ONLY" = 0 ]; then
  case "${ARCH:-universal}" in host) MAC_ARCH="$(uname -m)" ;; *) MAC_ARCH=universal ;; esac
  WANT="Tangent-${VERSION}-macos.pkg Tangent-${VERSION}-macos-${MAC_ARCH}.zip
    Tangent-${VERSION}-macos-${MAC_ARCH}.dmg"
  [ "${SKIP_WINDOWS:-0}" = 1 ] || WANT="$WANT Tangent-${VERSION}-windows-setup.exe
    tangent-${VERSION}-windows-x86_64.zip tangent-${VERSION}-linux-x86_64.tar.gz
    tangent-${VERSION}-linux-aarch64.tar.gz"
  for w in $WANT; do
    [ -f "dist/$w" ] || note "no $w: its build failed or was skipped (see above)"
  done
fi

echo "==> Checking ${#ARTIFACTS[@]} artifact(s)"
# The checks themselves are scripts/check-artifacts.sh, which
# publish-dropbox.sh runs on every tester build as well; each problem is a
# `!!` line of its own, and any of them makes this run red.
PKG_VERSION="$PKG_VERSION" scripts/check-artifacts.sh "$VERSION" "${ARTIFACTS[@]}" || FAIL=1

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
