#!/usr/bin/env bash
# Package Tangent, one installer per platform.
#
#   scripts/build-installer.sh macos      # dist/Tangent-<ver>-macos.pkg
#   scripts/build-installer.sh windows    # dist/Tangent-<ver>-windows-setup.exe
#   scripts/build-installer.sh linux      # install.sh inside the Linux tarball
#   scripts/build-installer.sh            # host platform
#
# It CONSUMES artifacts, it does not build them. Run these first:
#
#   scripts/build-macos.sh                 -> dist/Tangent.app        (signed, notarized)
#   scripts/build-cross.sh                 -> dist/tangent-<ver>-windows-x86_64/
#   scripts/build-linux-remote.sh <host>   -> the Linux tarball
#
# Consuming rather than building is deliberate: the macOS app is signed AND
# notarized by its own script, and rebuilding it here would either duplicate
# that or quietly ship an unnotarized copy inside a package that looks fine.
#
# WHERE THINGS GO:
#
#   macOS    /Applications (~/Applications when installed for one user)
#   Windows  %PROGRAMFILES%\Tangent
#   Linux    ~/.local/bin (or <prefix>/bin)
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

VERSION="$(grep '^version' Cargo.toml | head -1 | sed -E 's/.*"([^"]+)".*/\1/')"
NAME="Tangent"

# Staging happens OUTSIDE the repository, and that is not tidiness.
#
# This repo lives in Dropbox, which cannot store extended attributes natively
# and writes AppleDouble sidecars instead. `ditto` into a Dropbox directory
# therefore produces `._Tangent.app`, `._Contents`, `._Info.plist` and one
# `._` file per binary — and `pkgbuild` packages every one of them, so the
# installer would put that litter into /Applications. (It is also why
# `target` is a symlink out of Dropbox; see
# docs/HANDOFF.md.) A real APFS directory stores the attributes properly and
# no sidecar is created at all.
WORK="$(mktemp -d "${TMPDIR:-/tmp}/tangent-installer.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT

PLATFORM="${1:-$(uname -s | tr '[:upper:]' '[:lower:]')}"
[ "$PLATFORM" = "darwin" ] && PLATFORM=macos

need() {
  if [ ! -e "$1" ]; then
    echo "missing: $1" >&2
    echo "         $2" >&2
    exit 1
  fi
}

# ── macOS ───────────────────────────────────────────────────────────────────
build_macos() {
  local app="dist/$NAME.app"
  need "$app"  "run scripts/build-macos.sh first"

  echo "==> $NAME $VERSION — macOS installer"
  rm -rf "$WORK/macos"
  mkdir -p "$WORK/macos/pkgs" "$WORK/macos/root-app" "$WORK/macos/resources"

  # `ditto` rather than `cp -R`, and STRIPPED rather than faithful.
  #
  # These bundles live in Dropbox, so every file carries a `com.dropbox.attrs`
  # extended attribute, and there is no reason for a user's /Applications to
  # receive this machine's sync bookkeeping.
  #
  # Stripping is safe, and that is checked below rather than assumed: a
  # bundle's signature lives in the Mach-O and in
  # `_CodeSignature/CodeResources`, and the notarization ticket is stapled into
  # `Contents/CodeResources` — all real files. Extended attributes only carry a
  # signature for single-file (non-bundle) code, which this is not.
  #
  # (`lsbom` on the built package still lists `._` entries either way. Those
  # are AppleDouble metadata records, not files: `pkgutil --expand-full`
  # extracts a tree with zero of them, which is what the installer writes.
  # Verified, because they look exactly like litter until you check.)
  ditto --norsrc --noextattr --noacl "$app"  "$WORK/macos/root-app/$NAME.app"

  find "$WORK/macos" \( -name '._*' -o -name '.DS_Store' \) -delete

  # ...and prove it, because a broken signature inside a package is invisible
  # until someone downloads it. `spctl` is the one that would catch a lost
  # staple, which `codesign` alone would not.
  codesign --verify --strict "$WORK/macos/root-app/$NAME.app"
  if ! spctl --assess --type execute "$WORK/macos/root-app/$NAME.app" >/dev/null 2>&1; then
    echo "!!  the staged app is not accepted by Gatekeeper — did build-macos.sh notarize?"
  fi

  # The component plist, and the one field that has to be changed.
  #
  # pkgbuild defaults an .app component to BundleIsRelocatable=true, which
  # means: if Spotlight finds another copy of this bundle id anywhere on the
  # disk, install OVER THAT instead of the location you chose. Someone with an
  # old copy in ~/Downloads gets an "install to /Applications" that silently
  # updates ~/Downloads and leaves /Applications empty. Turning it off is the
  # single most important line in this script.
  pkgbuild --analyze --root "$WORK/macos/root-app" "$WORK/macos/app.plist" >/dev/null
  /usr/libexec/PlistBuddy -c "Set :0:BundleIsRelocatable false" "$WORK/macos/app.plist"

  pkgbuild --root "$WORK/macos/root-app" \
    --component-plist "$WORK/macos/app.plist" \
    --identifier com.ganten.tangent.app \
    --version "$VERSION" \
    --install-location /Applications \
    --scripts installer/macos/scripts \
    "$WORK/macos/pkgs/Tangent-app.pkg" >/dev/null

  cp installer/macos/welcome.html installer/macos/conclusion.html "$WORK/macos/resources/"
  {
    cat LICENSING.md
    printf '\n\n%s\n\n' "$(printf '=%.0s' {1..70})"
    cat LICENSE
  } > "$WORK/macos/resources/LICENSE.txt"

  sed -e "s/@VERSION@/$VERSION/g" -e "s/@TITLE@/$NAME $VERSION/g" \
    installer/macos/distribution.xml.in > "$WORK/macos/distribution.xml"

  local out="dist/$NAME-$VERSION-macos.pkg"
  productbuild --distribution "$WORK/macos/distribution.xml" \
    --package-path "$WORK/macos/pkgs" \
    --resources "$WORK/macos/resources" \
    "$WORK/macos/unsigned.pkg" >/dev/null

  # Signing a .pkg needs a "Developer ID INSTALLER" certificate, which is a
  # different certificate from the "Developer ID Application" one that signs
  # the .app — same account, separate download from the developer portal.
  # `productsign` rejects the application identity outright:
  #
  #   An installer signing identity (not an application signing identity)
  #   is required for signing flat-style products.
  #
  # So this checks for the right kind and says exactly what is missing when
  # there is not one, rather than shipping something that looks signed.
  local installer_id
  # `|| true` is load-bearing under `set -e`: an assignment takes the exit
  # status of its command substitution, and `grep` with no match exits 1 — so
  # the script would die at exactly the point it is trying to detect that
  # something is absent, with no message at all.
  installer_id="${TANGENT_INSTALLER_ID:-$(security find-identity -v 2>/dev/null \
    | grep 'Developer ID Installer' | head -1 | sed -E 's/.*"(.*)"/\1/' || true)}"

  if [ -n "$installer_id" ]; then
    echo "    signing as: $installer_id"
    productsign --sign "$installer_id" "$WORK/macos/unsigned.pkg" "$out"
    pkgutil --check-signature "$out" | sed 's/^/    /'
    local profile="${TANGENT_NOTARY_PROFILE:-tangent-notary}"
    if xcrun notarytool history --keychain-profile "$profile" >/dev/null 2>&1; then
      echo "    notarizing"
      if xcrun notarytool submit "$out" --keychain-profile "$profile" --wait; then
        xcrun stapler staple "$out"
        xcrun stapler validate "$out"
      else
        echo "!!  notarization failed — the pkg is signed but will warn on first open"
      fi
    else
      echo "!!  no '$profile' keychain profile; signed but NOT notarized"
    fi
  else
    mv "$WORK/macos/unsigned.pkg" "$out"
    cat <<'WARN'
!!  UNSIGNED. This installs, but Gatekeeper will refuse it on first open and
    the user has to right-click -> Open, which is exactly the friction the
    notarized .app was built to avoid.

    The fix is a "Developer ID Installer" certificate. It is a SECOND
    certificate on the same paid account that already issued the "Developer ID
    Application" one used for Tangent.app:

      1. https://developer.apple.com/account/resources/certificates/add
      2. choose "Developer ID Installer", upload a CSR from Keychain Access
      3. download and double-click the .cer

    Then re-run this script; it picks the identity up automatically.
WARN
  fi

  echo
  echo "==> $out"
  ls -lh "$out" | sed 's/^/    /'
  # What the package offers, read back out of it: one choice, the application
  # (check-artifacts.sh refuses a package without it).
  echo "    choices offered:"
  installer -showChoicesXML -pkg "$out" -target / 2>/dev/null \
    | grep -A2 'choiceIdentifier' | grep -E 'choice\.|<integer>' \
    | sed 's/^/      /' || true
}

# ── Windows ─────────────────────────────────────────────────────────────────
build_windows() {
  # The ZIP, not a staging directory: `build-cross.sh` removes the stage once
  # it has packaged it, and the zip is also exactly what a user downloading
  # the portable build receives — so the installed files and the unzipped
  # files cannot drift apart.
  local zip="dist/tangent-$VERSION-windows-x86_64.zip"
  local out="dist/$NAME-$VERSION-windows-setup.exe"
  need "$zip"  "run scripts/build-cross.sh first"
  command -v makensis >/dev/null || {
    echo "makensis not found. Install it with: brew install makensis" >&2
    exit 1
  }
  # **The previous installer goes first.** makensis writes its output last,
  # so when it aborts an existing one can stay where it was, and anything
  # below can abort it: a plug-in that will not sign, or the uninstaller's signing,
  # which `!uninstfinalize ... = 0` turns into a failed makensis. The old
  # setup.exe then sat beside a fresh zip, two builds under one version's
  # names, and the checks passed both. Now a failed build leaves none.
  rm -f "$out"

  echo "==> $NAME $VERSION — Windows installer"
  rm -rf "$WORK/windows"
  mkdir -p "$WORK/windows/app"
  unzip -q "$zip" -d "$WORK/windows/app"
  need "$WORK/windows/app/tangent.exe" "the zip has no tangent.exe in it"
  cp LICENSING.md "$WORK/windows/LICENSE.txt"

  # The installer wears the app's logo. Same icon build.rs embeds into the
  # exe; the setup, the app, and the Add/Remove entry are one mark.
  need "$ROOT/assets/tangent.ico" "the app icon is missing"

  # **NSIS's own plug-ins are signed too.** The installer unpacks two of
  # NSIS's stock plug-in DLLs into $PLUGINSDIR and loads them: System.dll in
  # .onInit (x64.nsh's ${RunningX64} is four System::Calls) and nsDialogs.dll
  # for the finish page. Smart App Control judges DLLs as well as programs,
  # and a System.dll it refuses makes ${RunningX64} false: the signed
  # installer would stop with "Tangent needs 64-bit Windows." on a 64-bit
  # machine. makensis loads plug-ins from $NSISDIR/Plugins, and NSISDIR is
  # its documented override for its data directory, so it gets a private
  # copy of that directory whose two DLLs are signed (Homebrew's copy is
  # never touched). A plug-in the .nsi starts calling later is NOT signed
  # here, and check-artifacts.sh, which reads every PE inside the setup.exe,
  # refuses it: add it to this list.
  local nsis_data plugin
  nsis_data="$(cd "${NSISDIR:-$(dirname "$(command -v makensis)")/../share/nsis}" 2>/dev/null && pwd -P || true)"
  [ -n "$nsis_data" ] && [ -d "$nsis_data/Plugins/x86-unicode" ] || {
    echo "cannot find NSIS's data directory (Plugins, Stubs, Include); set NSISDIR" >&2
    exit 1
  }
  cp -R "$nsis_data" "$WORK/windows/nsis"
  chmod -R u+w "$WORK/windows/nsis"
  for plugin in System nsDialogs; do
    scripts/sign-windows.sh "$WORK/windows/nsis/Plugins/x86-unicode/$plugin.dll" "NSIS $plugin.dll"
  done

  # SIGNCMD signs the uninstaller while makensis builds it (the .nsi's
  # !uninstfinalize); the setup.exe itself is signed by build-cross.sh once
  # makensis has written it.
  NSISDIR="$WORK/windows/nsis" makensis -NOCD \
    "-DVERSION=$VERSION" \
    "-DNUMVERSION=${VERSION%%-*}" \
    "-DSRCDIR=$WORK/windows" \
    "-DICON=$ROOT/assets/tangent.ico" \
    "-DOUTFILE=$ROOT/$out" \
    "-DSIGNCMD=$ROOT/scripts/sign-windows.sh" \
    installer/windows/tangent.nsi | sed 's/^/    /'

  echo "==> $out"
  ls -lh "$out" | sed 's/^/    /'
}

# ── Linux ───────────────────────────────────────────────────────────────────
# There is no installer binary here on purpose. A .deb reaches Debian and
# Ubuntu and nothing else, and the same argument then repeats for rpm, for
# Arch and for Void — the project's own Linux host. A POSIX shell script inside
# the tarball works on all of them, needs no root for the default (user-local)
# install, and can be read before it is run, which is what a Linux user will
# want to do anyway.
build_linux() {
  local tarball="dist/tangent-$VERSION-linux-x86_64.tar.gz"
  need "$tarball" "run scripts/build-linux-remote.sh <host> first"

  echo "==> $NAME $VERSION — Linux tarball with an installer"
  rm -rf "$WORK/linux"
  mkdir -p "$WORK/linux"
  tar -xzf "$tarball" -C "$WORK/linux"
  local dir
  dir="$(find "$WORK/linux" -maxdepth 1 -mindepth 1 -type d | head -1)"
  [ -n "$dir" ] || { echo "the tarball has no top-level directory" >&2; exit 1; }

  cp installer/linux/install.sh "$dir/install.sh"
  chmod +x "$dir/install.sh"
  cp LICENSING.md "$dir/"

  # The bundled encoder, so video needs nothing installed. Cached and
  # checksum-pinned; offline after the first fetch.
  scripts/fetch-ffmpeg.sh linux
  cp dist/vendor/linux/tangent-ffmpeg "$dir/tangent-ffmpeg"
  chmod 755 "$dir/tangent-ffmpeg"
  rm -rf "$dir/ffmpeg-licenses"
  cp -R dist/vendor/linux/ffmpeg-licenses "$dir/ffmpeg-licenses"

  # Strip macOS extended attributes before repacking.
  #
  # This tarball is built on Linux, brought here, unpacked, added to, and
  # repacked — and the files added here come out of a Dropbox working copy, so
  # they carry `com.dropbox.attrs` and `com.apple.provenance`. bsdtar stores
  # those as PAX extended headers, and GNU tar on the far side prints
  #
  #   tar: Ignoring unknown extended header keyword 'LIBARCHIVE.xattr...'
  #
  # once PER FILE. Fifty lines of warnings is what a Linux user sees on their
  # first contact with this project, and every one of them is this machine's
  # sync bookkeeping leaking into a release.
  xattr -cr "$dir" 2>/dev/null || true

  # Written beside the real name and moved into place only on success.
  #
  # The input and the output are the SAME FILE (this unpacks the tarball, adds
  # the installer and the encoder, and repacks it), so a failure anywhere in
  # between used to destroy the artifact it was given, which took a remote
  # rebuild to get back. `package_linux` in build-cross.sh learned this the
  # same way.
  local out="dist/tangent-$VERSION-linux-x86_64.tar.gz"
  # `--no-mac-metadata` is bsdtar-only, and bsdtar is only where the Mac
  # metadata is. GNU tar (this step run ON Linux, where a Linux-only fix
  # release is assembled end to end) has neither the flag nor the problem.
  local strip_meta=""
  tar --no-mac-metadata --version >/dev/null 2>&1 && strip_meta="--no-mac-metadata"
  ( cd "$WORK/linux" && tar $strip_meta --no-xattrs -czf "$ROOT/$out.new" "$(basename "$dir")" )
  mv -f "$ROOT/$out.new" "$ROOT/$out"

  echo "==> $out"
  ls -lh "$out" | sed 's/^/    /'
  echo "    contents:"
  tar -tzf "$out" | sed 's/^/      /'
  # Same lesson as everywhere else in this repo: an archive that exists proves
  # nothing. The binary, the installer and the encoder with its licence must
  # actually be in it.
  for want in '/tangent$' '/install.sh$' '/tangent-ffmpeg$' \
              'ffmpeg-licenses/GPLv3.txt$'; do
    if [ "$(tar -tzf "$out" | grep -cE "$want")" -eq 0 ]; then
      echo "FAIL: nothing matching $want in $out" >&2
      exit 1
    fi
  done
  # And no Tangent.vst3 may come back in: no download carries one.
  if [ "$(tar -tzf "$out" | grep -c 'Tangent\.vst3')" -ne 0 ]; then
    echo "FAIL: $out contains a Tangent.vst3, which no download may carry" >&2
    exit 1
  fi
  # **The installer itself has to RUN, not merely be present.** `sh -n` catches
  # a syntax error; it does not catch a line that parses fine and does the wrong
  # thing. A stray uncommented line in the header comment block — one lost '#' —
  # is valid shell that re-invokes the script, and each invocation does it
  # again: an exponential fork bomb that only appears when someone runs it.
  # `--help` exercises argument parsing and exits, under a process cap so a
  # regression fails this build instead of the machine running it.
  _inst="$(mktemp -d)"
  tar -xzf "$out" -C "$_inst" --wildcards '*/install.sh' 2>/dev/null ||
    tar -xzf "$out" -C "$_inst"
  _sh="$(find "$_inst" -name install.sh -print -quit)"
  if ! sh -n "$_sh"; then
    echo "FAIL: install.sh is not valid POSIX sh" >&2; rm -rf "$_inst"; exit 1
  fi
  if ! ( ulimit -u 200; timeout 20 sh "$_sh" --help >/dev/null 2>&1 </dev/null ); then
    echo "FAIL: install.sh --help did not exit cleanly under a process cap" >&2
    echo "      (a self-invoking line in the header will look exactly like this)" >&2
    rm -rf "$_inst"; exit 1
  fi
  rm -rf "$_inst"

  echo "    binary, installer and encoder all present; install.sh --help runs"
}

mkdir -p dist
case "$PLATFORM" in
  macos)   build_macos ;;
  windows) build_windows ;;
  linux)   build_linux ;;
  *) echo "unknown platform: $PLATFORM (want macos, windows or linux)" >&2; exit 2 ;;
esac
