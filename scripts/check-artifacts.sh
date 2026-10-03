#!/usr/bin/env bash
# check-artifacts.sh — every check an artifact must pass before it leaves this
# machine, by either road: a release (release.sh) or a tester build
# (publish-dropbox.sh).
#
#   scripts/check-artifacts.sh VERSION FILE...
#
# One `!!` line per problem; exit 1 if there was any, 0 when every FILE
# passed, 2 on a usage error. VERSION is the version the files are named for,
# `-dev` included. A FreeBSD prerelease's pkg spells its version another way:
# release.sh passes that spelling as PKG_VERSION, and VERSION stands in for it
# otherwise.
#
# **Why one script with two callers.** These checks lived inside release.sh,
# and release.sh refuses a `-dev` version before it looks at a single file, so
# what testers received passed none of them: not the Smart App Control
# signatures, not the ASIO notice. A gate only one road out passes through is
# a gate the other road goes around.
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
[ $# -ge 2 ] && [ -n "$1" ] || { echo "usage: $0 VERSION FILE..." >&2; exit 2; }
VERSION="$1"
shift
PKG_VERSION="${PKG_VERSION:-$VERSION}"

FAIL=0
note() { echo "!! $*"; FAIL=1; }

# pe_sig <label> <pe>: the Authenticode presence check for every Windows PE we
# ship, in one wording. A presence check on the PE's certificate table
# (scripts/pe-signed.py) rather than a verifier that may not be installed — a
# gate that only runs where a tool happens to exist is a gate that is off on
# the machine that cuts releases. Exit 1 is a clean-but-unsigned PE; anything
# else means the file could not even be read as a PE, which is worse, not
# better.
pe_sig() {
  local sig=0
  python3 "$HERE/pe-signed.py" "$2" >/dev/null 2>&1 || sig=$?
  case "$sig" in
    0) ;;
    1) note "$1: UNSIGNED (Authenticode) — configure TRUSTED_SIGNING_* and rerun (docs/RELEASE.md § Windows)" ;;
    *) note "$1: could not read the PE certificate table (pe-signed.py exit $sig)" ;;
  esac
}

# ── The FreeBSD artifacts, checked as artifacts ──────────────────────────────
# Functions rather than case arms so they can be exercised against a file in
# dist/ without running the whole release — which is how they were tested,
# against the artifacts the build box had already produced.
#
# A FreeBSD .pkg is pkg(8)'s archive, not Installer's flat package: a tar with
# the two manifests at its head, readable with the bsdtar every host has. What
# build-freebsd.sh asserts at create time is asserted AGAIN here, on the file
# that actually reached dist/ — a copy from the build box is the thing being
# blessed, not the box's log.
check_freebsd_pkg() {
  local a="$1" list cm want
  list="$(tar -tf "$a" 2>/dev/null)" || { note "$a: not a readable pkg archive"; return; }
  for want in '^\+MANIFEST$' '^\+COMPACT_MANIFEST$' \
              '^/usr/local/bin/tangent$' '^/usr/local/bin/tangent-jack$' \
              '/share/applications/tangent\.desktop$' \
              '/icons/hicolor/128x128/apps/tangent\.png$' \
              '^/usr/local/share/licenses/tangent/LICENSE$' \
              '^/usr/local/share/licenses/tangent/OFL\.txt$' \
              '^/usr/local/share/licenses/tangent/THIRD-PARTY-LICENSES$'; do
    printf '%s\n' "$list" | grep -Eq "$want" || note "$a: missing $want"
  done
  cm="$(tar -xOf "$a" +COMPACT_MANIFEST 2>/dev/null || true)"
  [ -n "$cm" ] || { note "$a: no readable +COMPACT_MANIFEST"; return; }
  # alsa-lib decides whether the app STARTS on the machine this port exists
  # for, lilv the same; a GhostBSD base dependency installs nowhere else.
  for want in alsa-lib alsa-plugins lilv; do
    case "$cm" in *"\"$want\""*) ;; *) note "$a: manifest does not declare $want" ;; esac
  done
  case "$cm" in *GhostBSD-*) note "$a: depends on a GhostBSD base package (will not install on plain FreeBSD)" ;; esac
  printf '%s' "$cm" | grep -Eq "\"version\": ?\"${PKG_VERSION//./\\.}\"" \
    || note "$a: manifest version is not $PKG_VERSION"
}

# An offline bundle: the app's own pkg, its dependency pkgs beside it, the
# installer the README's first line names, and the same licence texts every
# other archive carries. The generic README/LICENSE/OFL checks have already
# run on the listing by the time this is called.
check_freebsd_bundle() {
  local a="$1" list="$2" dep
  printf '%s\n' "$list" | grep -Eq "/tangent-${PKG_VERSION//./\\.}-freebsd-[^/]*\.pkg$" \
    || note "$a: no tangent pkg inside"
  printf '%s\n' "$list" | grep -Eq '/install\.sh$' \
    || note "$a: no install.sh (README's first instruction)"
  printf '%s\n' "$list" | grep -Eq '/THIRD-PARTY-LICENSES$' \
    || note "$a: no THIRD-PARTY-LICENSES"
  for dep in alsa-lib alsa-plugins lilv; do
    printf '%s\n' "$list" | grep -Eq "/${dep}-[0-9][^/]*\.pkg$" \
      || note "$a: no $dep pkg (the app does not start without it)"
  done
  case "$a" in
    *-complete.tar.gz)
      # 'complete' is for a box that has only ever had Xorg: the graphics
      # stack and the encoder are exactly what it is for.
      for dep in mesa-dri ffmpeg; do
        printf '%s\n' "$list" | grep -Eq "/${dep}-[0-9][^/]*\.pkg$" \
          || note "$a: 'complete' bundle without $dep"
      done ;;
  esac
}

for a in "$@"; do
  # ── Installers, which carry BOTH payloads and are checked for both ────────
  # An installer that quietly ships only the app is the same class of failure
  # as the empty Linux tarball: it installs, it looks right, and the thing the
  # user came for is not there.
  case "$a" in
    *-freebsd-*.pkg)
      check_freebsd_pkg "$a"
      continue
      ;;
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
      # installer no matter what happened to the inner exe.
      pe_sig "$a" "$a"
      # And every PE INSIDE it, because Smart App Control judges each program
      # as it starts, with no way for the user to allow one: an unsigned
      # tangent.exe, encoder or uninstaller is blocked however signed the
      # setup was. Extracted, not listed, because a signature is in the bytes.
      if command -v 7zz >/dev/null 2>&1; then
        inner="$(mktemp -d)"
        for pe in tangent.exe tangent-ffmpeg.exe Uninstall.exe; do
          if 7zz e -y -o"$inner" "$a" "$pe" >/dev/null 2>&1 && [ -f "$inner/$pe" ]; then
            pe_sig "$a: $pe" "$inner/$pe"
          else
            note "$a: could not extract $pe to check its signature"
          fi
        done
        rm -rf "$inner"
      fi
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
      # **ASIO, or the release does not go out.**
      #
      # `build-cross.sh` writes this notice when the build's features include
      # `asio`, which it allows only with `vendor/asiosdk_*` present - the
      # SDK's licence forbids committing it, so a fresh checkout does not have
      # it. Steinberg require the notice whenever ASIO rides in.
      #
      # ASIO is the standard on Windows and the testers have interfaces: a
      # build without it is not the product, and the way that failure
      # presented was a user asking why the ASIO option FL Studio gives him
      # is missing here.
      printf '%s\n' "$list" | grep -Eq '(^|/)ASIO-NOTICE\.txt$' \
        || note "$a: NO ASIO (vendor/asiosdk_* was missing at build time)"
      # The portable build meets Smart App Control exactly as the installed
      # one does, so its programs are held to the same signatures.
      inner="$(mktemp -d)"
      for pe in tangent.exe tangent-ffmpeg.exe; do
        if unzip -q -o "$a" "$pe" -d "$inner" 2>/dev/null && [ -f "$inner/$pe" ]; then
          pe_sig "$a: $pe" "$inner/$pe"
        else
          note "$a: could not extract $pe to check its signature"
        fi
      done
      # **And the host itself, read off the program.** A text file is a
      # notice, not proof: the notice used to be keyed on CPAL_ASIO_DIR,
      # cpal's own variable, which any shell can export, so a WASAPI-only
      # build could carry it and pass. The SDK's driver list (asiolist.cpp)
      # compiles the registry key it enumerates, `software\asio`, into the
      # program; nothing of Tangent's own spells it in lower case, and a
      # build without the host has no such bytes.
      if [ -f "$inner/tangent.exe" ] && ! LC_ALL=C grep -a -q -F 'software\asio' "$inner/tangent.exe"; then
        note "$a: tangent.exe has no ASIO host in it (a WASAPI-only build)"
      fi
      rm -rf "$inner"
      ;;
    *-freebsd-*)
      check_freebsd_bundle "$a" "$list"
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

[ "$FAIL" = 0 ]
