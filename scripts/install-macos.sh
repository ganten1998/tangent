#!/usr/bin/env bash
#
# Install the built Tangent.app into /Applications.
#
# ONE short command, and that is the whole point of it:
#
#     cd ~/Dropbox/Projects/Apps/tangent && ./scripts/install-macos.sh
#
# The three-line copy-paste this replaces was
#
#     sudo rm -rf /Applications/Tangent.app
#     sudo ditto dist/Tangent.app /Applications/Tangent.app
#     sudo chown -R root:wheel /Applications/Tangent.app
#
# and a terminal that wraps long pastes eventually joined the first two lines,
# so `rm -rf` ran with `dist/Tangent.app` among its arguments and deleted the
# build as well as the installation. A destructive command must never share a
# line with anything, and the reliable way to guarantee that is not to hand
# anybody a destructive command at all.
#
# No `sudo`. /Applications is drwxrwxr-x root:admin, so an admin user has
# always been able to write to it; the sudo was cargo-culted and it is what
# made a mistyped paste able to reach outside /Applications.

set -euo pipefail

cd "$(dirname "$0")/.."

APP="dist/Tangent.app"
DEST="/Applications/Tangent.app"

# **The source is checked BEFORE the destination is touched.** This is the
# whole safety property: the failure that prompted this script left the machine
# with no built app AND no installed app, because the removal happened first.
if [ ! -d "$APP" ]; then
  echo "==> $APP is missing; looking for a packaged build to restore it from"
  ZIP=$(ls -t dist/Tangent-*-macos-*.zip 2>/dev/null | head -1 || true)
  if [ -z "$ZIP" ]; then
    echo "error: no $APP and no dist/Tangent-*-macos-*.zip to restore from." >&2
    echo "       Run ./scripts/build-macos.sh first." >&2
    exit 1
  fi
  echo "    restoring from $ZIP"
  TMP=$(mktemp -d)
  trap 'rm -rf "$TMP"' EXIT
  ditto -xk "$ZIP" "$TMP"
  INNER=$(find "$TMP" -maxdepth 2 -name Tangent.app -type d | head -1)
  if [ -z "$INNER" ]; then
    echo "error: $ZIP does not contain a Tangent.app." >&2
    exit 1
  fi
  ditto "$INNER" "$APP"
fi

# Signed, and readable. A bundle that fails this is one that will be killed on
# launch by AMFI with nothing on screen to say why, which is a far worse
# afternoon than a failed install.
if ! codesign --verify --strict "$APP" >/dev/null 2>&1; then
  echo "error: $APP is not validly signed. Rebuild rather than installing it." >&2
  exit 1
fi

# The FULL version, pre-release suffix and all. `TangentVersion` is written
# by build-macos.sh precisely because CFBundleShortVersionString cannot hold a
# suffix; fall back to it for apps built before that key existed.
app_version() {
  defaults read "$1/Contents/Info.plist" TangentVersion 2>/dev/null \
    || defaults read "$1/Contents/Info.plist" CFBundleShortVersionString 2>/dev/null \
    || echo "?"
}
VERSION=$(app_version "$PWD/$APP")

# Replace, rather than copy over the top. `ditto` merges, so a file that
# existed in the old version and not in the new one would survive into the
# installed bundle and break its signature.
if [ -d "$DEST" ]; then
  OLD=$(app_version "$DEST")
  # The retired plugin, wherever an old dev-install or pkg left it. The
  # user-level copy goes quietly; a root-owned /Library copy is named so the
  # owner can remove it with sudo.
  rm -rf "$HOME/Library/Audio/Plug-Ins/VST3/Tangent.vst3" 2>/dev/null || true
  if [ -d "/Library/Audio/Plug-Ins/VST3/Tangent.vst3" ] && [ ! -w "/Library/Audio/Plug-Ins/VST3/Tangent.vst3" ]; then
    echo "NOTE: retired plugin at /Library/Audio/Plug-Ins/VST3/Tangent.vst3 (root-owned)"
    echo "      remove it with:  sudo rm -rf /Library/Audio/Plug-Ins/VST3/Tangent.vst3"
  fi
  echo "==> Replacing $OLD with $VERSION"
  rm -rf "$DEST"
else
  echo "==> Installing $VERSION"
fi

ditto "$APP" "$DEST"

# **A reinstall silently drops this app out of SoundSource's exclusions.**
#
# Replacing the bundle gives /Applications/Tangent.app a new inode, and
# SoundSource's per-app audio exclusion stores an ALIAS beside the bundle id —
# a reference to the bundle that was there when the rule was made. The new copy
# is not that bundle, so the exclusion stops matching, and SoundSource captures
# a live DAW's output through ARK until it is restarted and re-resolves the
# alias by path.
#
# That cost a week. It read as Tangent's own monitoring getting slower, it
# survived two sessions of measuring the software path (which finds nothing,
# because an app-level shim is invisible from inside the process), and six
# installs in one morning re-broke it six times.
#
# Updating the bundle IN PLACE would avoid it, and does not work: macOS refuses
# writes into a running app's bundle, which is exactly when a dev install
# happens. So the honest answer is to say so, every time, rather than let it
# recur in silence.
if pgrep -xq SoundSource 2>/dev/null; then
  echo
  echo "NOTE: SoundSource is running. This install replaced the bundle, so its"
  echo "      per-app exclusion for Tangent no longer matches and SoundSource"
  echo "      will process (and delay) Tangent's audio."
  echo "      Restart SoundSource to re-arm the exclusion."
fi

# Notarization is a fact about the build, not about this copy, and it is worth
# saying out loud: a signed-but-unnotarized bundle runs perfectly well here
# (nothing downloaded it, so it carries no quarantine attribute) and is refused
# on every machine that DID download it.
if xcrun stapler validate "$DEST" >/dev/null 2>&1; then
  echo "    notarized and stapled"
else
  echo "    signed but NOT notarized — fine on this machine, refused on any other"
fi

echo "==> $DEST is $(app_version "$DEST")"
