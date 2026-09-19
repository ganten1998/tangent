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

# **Not while it is running.**
#
# The install replaces the bundle's contents in place, and macOS refuses
# writes into a running app's bundle — an earlier attempt at this emptied
# /Applications/Tangent.app and left nothing behind. Refusing is the whole
# safeguard: there is no half-install to recover from if it never starts.
if pgrep -xq tangent 2>/dev/null; then
  echo "error: Tangent is running. Quit it (normally — never force-kill an" >&2
  echo "       audio app) and run this again." >&2
  exit 1
fi

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
  # **Can this shell still write inside an app bundle?**
  #
  # macOS 26 brought App Management: a process may not modify another app's
  # bundle in /Applications unless the user has granted it that permission in
  # System Settings. A terminal usually has not, and the refusal is EPERM on
  # every write, including `mkdir Contents` and a bare `touch`.
  #
  # That turned the in-place replace below into a one-way trip: the contents
  # were deleted, because DELETING an app is allowed so that people can
  # uninstall one, and then the copy could not land. It left
  # /Applications/Tangent.app an empty directory and the machine with no
  # installed app, which is precisely the failure the whole ordering at the
  # top of this script exists to prevent. It is checked here rather than
  # discovered, and the probe runs BEFORE anything is removed.
  IN_PLACE=0
  if touch "$DEST/.tangent-install-probe" 2>/dev/null; then
    rm -f "$DEST/.tangent-install-probe"
    IN_PLACE=1
  fi
  # The number the whole exercise is about. Compared after the copy, so the
  # claim "the alias still resolves" is CHECKED rather than assumed.
  INODE_BEFORE=$(stat -f %i "$DEST" 2>/dev/null || echo "")
  # **The bundle DIRECTORY survives; only its contents are replaced.**
  #
  # `rm -rf "$DEST"` gave /Applications/Tangent.app a new inode on every
  # install, and that is what silently dropped the app out of SoundSource's
  # audio exclusions — the rule stores an ALIAS beside the bundle id, the
  # alias is a reference to the directory that was there when the rule was
  # made, and a fresh directory is not that one. See the note further down.
  #
  # Emptying it in place keeps the inode, so every alias anyone has made to
  # this app — SoundSource's exclusion, Loopback's source, a Dock item, a
  # login item — goes on resolving across updates.
  #
  # `find -mindepth 1 -maxdepth 1` rather than a glob, because a glob misses
  # dotfiles and `"$DEST"/*` with nothing to match expands to a literal that
  # `rm -rf` would then complain about. Everything inside goes: `ditto`
  # MERGES, so a file that existed in the old version and not in the new one
  # would otherwise survive into the installed bundle and break its
  # signature.
  if [ "$IN_PLACE" = 1 ]; then
    find "$DEST" -mindepth 1 -maxdepth 1 -exec rm -rf {} +
  else
    # **The bundle goes whole, and its inode with it.** Every alias made to
    # this app - SoundSource's exclusion, Loopback's source, a Dock item, a
    # login item - is a reference to the directory that was there when the
    # rule was made, so each of those has to be re-pointed once after this.
    # The alternative is refusing to install at all, which is worse.
    echo "NOTE: this shell cannot write inside an app bundle, so the bundle is"
    echo "      being replaced rather than refilled, and its identity changes."
    echo "      Aliases to Tangent (SoundSource exclusions, Loopback sources,"
    echo "      Dock and login items) need re-pointing once after this."
    echo "      To keep the identity across installs instead, grant this"
    echo "      terminal App Management in System Settings > Privacy &"
    echo "      Security > App Management, and run this again."
    rm -rf "$DEST"
  fi
else
  echo "==> Installing $VERSION"
fi

ditto "$APP" "$DEST"

# **Emptying a bundle in place is destructive until the copy lands.**
#
# If `ditto` failed, /Applications/Tangent.app is now an empty directory
# rather than an app, and saying so is the difference between a re-run and a
# mystery. The source is still in `dist/`, so a re-run fixes it.
if [ ! -x "$DEST/Contents/MacOS/tangent" ]; then
  echo "error: the copy did not land — $DEST has no executable." >&2
  echo "       The build is still in $APP; re-run this script." >&2
  echo "       If this says 'Operation not permitted', it is App Management:" >&2
  echo "       grant this terminal App Management in System Settings >" >&2
  echo "       Privacy & Security, or remove $DEST by hand and re-run." >&2
  exit 1
fi

# **A reinstall silently drops this app out of SoundSource's exclusions.**
#
# Replacing the bundle gives /Applications/Tangent.app a new inode, and
# SoundSource's per-app audio exclusion stores an ALIAS beside the bundle id —
# a reference to the bundle that was there when the rule was made. The new copy
# is not that bundle, so the exclusion stopped matching and SoundSource
# captured a live DAW's output through ARK.
#
# That cost a week. It read as Tangent's own monitoring getting slower, it
# survived two sessions of measuring the software path (which finds nothing,
# because an app-level shim is invisible from inside the process), and six
# installs in one morning re-broke it six times.
#
# **This install keeps the bundle's inode**, so the alias goes on resolving
# and the exclusion survives the update. See the replace above.
#
# **A restart does NOT repair one that is already broken**, which is what the
# note here used to claim: restarting re-reads the same stale alias. The
# owner found the real remedy by hand — remove the app from the exclusions
# and add it back, which mints a fresh alias — and it takes a restart to
# take effect, which is why "restart" looked like the fix. Only said when
# the exclusion cannot be seen to match, because after this change it
# usually will.
INODE_AFTER=$(stat -f %i "$DEST" 2>/dev/null || echo "")
if [ -n "${INODE_BEFORE:-}" ] && [ "$INODE_BEFORE" = "$INODE_AFTER" ]; then
  echo "    bundle kept its identity (inode $INODE_AFTER) — aliases still resolve"
fi
if pgrep -xq SoundSource 2>/dev/null \
  && [ -n "${INODE_BEFORE:-}" ] && [ "$INODE_BEFORE" != "$INODE_AFTER" ]; then
  echo
  echo "NOTE: SoundSource is running, and this install changed the bundle's"
  echo "      identity, so its audio exclusion for Tangent no longer matches"
  echo "      — SoundSource will process (and delay) Tangent's audio."
  echo "      Fix: remove Tangent from SoundSource's exclusions and add it"
  echo "      back, then restart SoundSource. A restart alone re-reads the"
  echo "      same stale reference and does not help."
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
