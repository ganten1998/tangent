#!/bin/sh
# Install Tangent on FreeBSD.
#
#   ./install.sh                 install for everyone (asks for root)
#   ./install.sh --home          install for just you, no root, no pkg
#   ./install.sh --no-tune       skip the USB audio latency tunable
#   ./install.sh --uninstall     take it back out
#   ./install.sh --dry-run       print what would happen and do nothing
#   ./install.sh --help
#
# WHY THIS IS A WRAPPER AND NOT THE INSTALLER
#
# The real installer is `pkg`, and this script's job is to call it correctly and
# to explain what happened. That is the opposite of the Linux script beside it,
# which IS the installer because Linux has no one package manager to defer to.
# FreeBSD has exactly one, every derivative uses it, and a package the system
# tracks beats a script the user has to keep — `pkg delete tangent` is then a
# complete and honest uninstall, and `pkg info tangent` answers "what did this
# put on my disk".
#
# The one thing pkg cannot do is a no-root install: PKG_DBDIR must be owned by
# root:wheel and there is no override, so an unprivileged user cannot register a
# package at all. --home is that path, and it is a plain file copy.
#
# POSIX sh. FreeBSD's /bin/sh is not bash and a bashism here would fail on the
# only operating system this script will ever run on.
set -eu

SELF_DIR="$(cd "$(dirname "$0")" && pwd)"
NAME="Tangent"
PKGNAME="tangent"

usage() {
    cat <<'USAGE'
Install Tangent on FreeBSD.

  ./install.sh                 install for everyone (asks for root)
  ./install.sh --home          install for just you, no root, no pkg
  ./install.sh --no-tune       skip the USB audio latency tunable
  ./install.sh --uninstall     take it back out
  ./install.sh --dry-run       print what would happen and do nothing
  ./install.sh --help          this

The default installs the package with pkg, which puts Tangent in
/usr/local/bin, adds it to your application menu, and pulls in anything it
needs that you do not already have. Every dependency is in this folder, so it
works with no network connection.

--home needs no root. It copies the program to ~/.local/bin and the menu entry
to ~/.local/share, which is where a FreeBSD desktop already looks. Nothing is
registered with pkg, so --home also owns its own uninstall.

A system-wide install also lowers hw.usb.uaudio.buffer_ms from 4 to 1, which is
the single biggest latency win available here: measured on a USB interface it
takes the round trip from 36.7 ms to 27.6 ms, and it is what makes JACK's
low-latency settings possible at all. It costs nothing measurable in stability
and it is one clearly marked, reversible line in /etc/sysctl.conf. --no-tune
skips it; --uninstall removes it.
USAGE
}

MODE=install
DRY=0
TUNE=1
for arg in "$@"; do
    case "$arg" in
        --home)      MODE=home ;;
        --uninstall) MODE=uninstall ;;
        --no-tune)   TUNE=0 ;;
        --dry-run)   DRY=1 ;;
        -h|--help)   usage; exit 0 ;;
        *) echo "unknown option: $arg" >&2; echo >&2; usage >&2; exit 2 ;;
    esac
done

# The marker is what makes this reversible. Everything between these two lines
# belongs to Tangent and is removed on uninstall; anything the user has written
# around them is untouched.
TUNE_BEGIN="# --- Tangent audio latency tuning (installer) ---"
TUNE_END="# --- end Tangent audio latency tuning ---"
SYSCTL_CONF=/etc/sysctl.conf

say()  { printf '%s\n' "$*"; }
step() { printf '==> %s\n' "$*"; }
run()  {
    if [ "$DRY" = 1 ]; then printf '    would run: %s\n' "$*"; return 0; fi
    "$@"
}

# The application package in this directory. Globbed rather than named with a
# version so the script does not have to be regenerated for every release.
find_app_pkg() {
    for f in "$SELF_DIR"/${PKGNAME}-*.pkg; do
        [ -f "$f" ] && { printf '%s\n' "$f"; return 0; }
    done
    return 1
}

# ── Root ─────────────────────────────────────────────────────────────────────
#
# Asked for rather than assumed, and re-executed rather than sprinkling sudo
# through the script, so there is exactly one privileged process and the user
# sees one password prompt.
ensure_root() {
    [ "$(id -u)" = 0 ] && return 0
    if [ "$DRY" = 1 ]; then
        say "    (would re-run as root)"
        return 0
    fi
    for esc in doas sudo; do
        if command -v "$esc" >/dev/null 2>&1; then
            step "Installing system-wide needs root; re-running with $esc"
            exec "$esc" /bin/sh "$0" "$@"
        fi
    done
    cat >&2 <<EOF
This installs for every user on the machine, which needs root, and neither
doas nor sudo is installed. Either run it as root:

    su -m root -c '/bin/sh $0'

or install just for yourself, which needs no root at all:

    $0 --home
EOF
    exit 1
}

# **The single biggest latency win available on FreeBSD, and it is one line.**
#
# `hw.usb.uaudio.buffer_ms` is the USB audio driver's buffering delay, and it
# ships at 4 with a documented range of 1 to 8. Measured on this port with
# `jack_iodelay` against a Focusrite Scarlett Solo, changing it to 1:
#
#   out of the box (ALSA-over-OSS)   36.7 ms  ->  27.6 ms
#   JACK at a 256-frame period       32.9 ms  ->  20.9 ms
#   JACK at its best stable setting       --  ->  10.9 ms
#
# It also lowers the driver's minimum block from 192 frames to 48, which is
# what makes the small JACK periods legal at all — so it is not merely a
# saving, it is what unlocks the low-latency configurations entirely.
#
# Applied by default because it is free: a 45-second stress test with four CPU
# hogs and the app playing ran 68,884 interrupts with ZERO underruns. It is
# reverted by --uninstall and skipped by --no-tune, and it is one clearly
# marked line in a file the user can read.
#
# The setting takes effect when a USB audio device ATTACHES, so it is applied
# now (for anything plugged in later) and written to sysctl.conf (for the next
# boot). A device already plugged in keeps its old buffering until re-plugged,
# which the message below says.
tune_audio() {
    [ "$TUNE" = 1 ] || { say "Skipping the audio tunable (--no-tune)."; return 0; }
    if grep -q "^hw.usb.uaudio.buffer_ms" "$SYSCTL_CONF" 2>/dev/null; then
        say "A hw.usb.uaudio.buffer_ms setting is already present; leaving it alone."
        return 0
    fi
    step "Lowering USB audio latency (hw.usb.uaudio.buffer_ms: 4 -> 1)"
    if [ "$DRY" = 1 ]; then
        say "    would append a marked block to $SYSCTL_CONF and apply it now"
        return 0
    fi
    {
        printf '%s\n' "$TUNE_BEGIN"
        printf '%s\n' "# USB audio buffering delay in ms (range 1-8, default 4). At 1 the"
        printf '%s\n' "# round trip through this machine's audio drops by about a quarter."
        printf '%s\n' "# Remove this block to restore the default."
        printf '%s\n' "hw.usb.uaudio.buffer_ms=1"
        printf '%s\n' "$TUNE_END"
    } >> "$SYSCTL_CONF"
    sysctl hw.usb.uaudio.buffer_ms=1 >/dev/null 2>&1 || true
    say "    applied, and written to $SYSCTL_CONF for next boot"
}

untune_audio() {
    grep -q "^$TUNE_BEGIN$" "$SYSCTL_CONF" 2>/dev/null || return 0
    step "Removing the audio tunable from $SYSCTL_CONF"
    [ "$DRY" = 1 ] && { say "    would remove the marked block"; return 0; }
    # Only the marked block, so a user's own edits above and below survive.
    awk -v b="$TUNE_BEGIN" -v e="$TUNE_END" '
        $0 == b { skip = 1 } skip == 0 { print } $0 == e { skip = 0 }
    ' "$SYSCTL_CONF" > "$SYSCTL_CONF.tangent-new" &&
        mv "$SYSCTL_CONF.tangent-new" "$SYSCTL_CONF"
    sysctl hw.usb.uaudio.buffer_ms=4 >/dev/null 2>&1 || true
}

case "$MODE" in

install)
    APP_PKG="$(find_app_pkg)" || {
        echo "no ${PKGNAME}-*.pkg beside this script — is the folder complete?" >&2
        exit 1
    }
    command -v pkg >/dev/null 2>&1 || {
        echo "pkg is not installed. That is unusual on FreeBSD; run 'pkg bootstrap'." >&2
        exit 1
    }
    ensure_root "$@"

    step "Installing $NAME and everything it needs"
    # `pkg add` and NOT `pkg install`, and the difference is the whole point of
    # the bundle: pkg add never contacts a repository, and when the named
    # package has unmet dependencies it searches THE DIRECTORY THE FILE IS IN
    # for archives that satisfy them (pkg-add(8)). So this works with the
    # network unplugged. `pkg install ./file.pkg` would instead go to the
    # remote repo, which is the right command for someone who has one and the
    # wrong one for a bundle that already carries its dependencies.
    #
    # Every .pkg is named rather than just the app, because pkg skips the ones
    # already installed and it makes the "nothing is downloaded" claim obvious
    # in the output.
    run pkg add "$SELF_DIR"/*.pkg

    say ""
    tune_audio

    say ""
    step "Done"
    say "Run it from the Multimedia menu, or type: tangent"
    if [ "$TUNE" = 1 ]; then
        say ""
        say "If a USB audio interface is plugged in right now, unplug and replug"
        say "it once — the lower-latency buffering applies when a device attaches."
    fi
    say "To remove it later:  pkg delete $PKGNAME"
    ;;

home)
    APP_PKG="$(find_app_pkg)" || {
        echo "no ${PKGNAME}-*.pkg beside this script — is the folder complete?" >&2
        exit 1
    }
    BIN="$HOME/.local/bin"
    SHARE="$HOME/.local/share"
    step "Installing $NAME into $HOME/.local (no root, not registered with pkg)"

    # The .pkg is a tar archive, so the binary can be lifted straight out of it
    # without pkg and without a second copy of the program in the bundle.
    TMP="$(mktemp -d)"
    trap 'rm -rf "$TMP"' EXIT INT TERM
    if [ "$DRY" = 1 ]; then
        say "    would extract $APP_PKG and copy into $BIN and $SHARE"
    else
        # Everything, rather than naming members. A .pkg stores its paths
        # ABSOLUTE (`/usr/local/bin/tangent`), so asking tar for `usr/local`
        # or `./usr/local` matches nothing and the extraction silently yields
        # an empty directory. tar strips the leading slash itself — the
        # "Removing leading '/'" note on stderr is that working, not failing.
        tar -xf "$APP_PKG" -C "$TMP" 2>/dev/null \
            || { echo "could not read $APP_PKG" >&2; exit 1; }
        SRC="$TMP/usr/local"
        [ -x "$SRC/bin/tangent" ] || {
            echo "no tangent binary inside $APP_PKG" >&2; exit 1; }
        mkdir -p "$BIN" "$SHARE/applications" "$SHARE/icons/hicolor/128x128/apps"
        cp "$SRC/bin/tangent" "$BIN/tangent"
        chmod 755 "$BIN/tangent"
        cp "$SRC/share/applications/tangent.desktop" "$SHARE/applications/"
        cp "$SRC/share/icons/hicolor/128x128/apps/tangent.png" \
           "$SHARE/icons/hicolor/128x128/apps/"
        # The system triggers that do this for a real package do not run for a
        # home install, so it is done by hand. Failure is not fatal: the menu
        # entry appears at the next login either way.
        command -v update-desktop-database >/dev/null 2>&1 &&
            update-desktop-database "$SHARE/applications" 2>/dev/null || true
    fi

    say ""
    step "Done"
    case ":$PATH:" in
        *":$BIN:"*) say "Run it with: tangent" ;;
        *)
            say "$BIN is not on your PATH. Either run it in full:"
            say "    $BIN/tangent"
            say "or add this to ~/.profile or ~/.zshrc:"
            say "    export PATH=\"\$HOME/.local/bin:\$PATH\""
            ;;
    esac
    say ""
    say "A home install cannot pull in dependencies — pkg is what does that, and"
    say "it needs root. If Tangent will not start, the one thing it cannot do"
    say "without is ALSA:"
    say "    pkg install alsa-lib alsa-plugins"
    say ""
    say "To remove it:  $0 --uninstall"
    ;;

uninstall)
    REMOVED=0
    # The home copy first, because it needs no privileges and a user who
    # installed both should lose both.
    for f in "$HOME/.local/bin/tangent" \
             "$HOME/.local/share/applications/tangent.desktop" \
             "$HOME/.local/share/icons/hicolor/128x128/apps/tangent.png"; do
        if [ -e "$f" ]; then
            step "Removing $f"
            run rm -f "$f"
            REMOVED=1
        fi
    done

    if pkg info "$PKGNAME" >/dev/null 2>&1; then
        ensure_root "$@"
        step "Removing the $PKGNAME package"
        # Dependencies are left alone deliberately. pkg knows which of them
        # nothing else wants — `pkg autoremove` is the user's call to make, and
        # taking mesa off a machine because they uninstalled one app would be a
        # remarkable thing for an uninstaller to do unasked.
        run pkg delete -y "$PKGNAME"
        untune_audio
        REMOVED=1
        say ""
        say "Dependencies were left installed. To remove any that nothing else"
        say "needs:  pkg autoremove"
    fi

    [ "$REMOVED" = 0 ] && say "$NAME does not appear to be installed."
    # Settings and takes are the user's, not the installer's. Said rather than
    # deleted, and said rather than silently left, so nobody is surprised later.
    say ""
    say "Your settings and recordings were kept:"
    say "    ~/.config/tangent"
    ;;
esac
