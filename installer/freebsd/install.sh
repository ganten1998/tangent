#!/bin/sh
# Install Tangent on FreeBSD.
#
#   ./install.sh                 install for everyone (asks for root)
#   ./install.sh --home          install for just you, no root, no pkg
#   ./install.sh --no-tune       skip the USB audio latency tunable
#   ./install.sh --no-rt         skip the real-time scheduling grant
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
# tracks beats a script the user has to keep: `pkg delete tangent` is then a
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
  ./install.sh --no-rt         skip the real-time scheduling grant
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

A system-wide install also puts you in the 'realtime' group and loads the
mac_priority policy that makes that group mean something. Without it Tangent
cannot put its render workers on the real-time scheduler, and a worker that
cannot be scheduled does not count - so the mixer renders every track on one
core no matter how many you have. This is the same grant the Linux audio
group exists to give. It is two reversible changes, a group membership and
one marked line in /boot/loader.conf. --no-rt skips it; --uninstall takes
back whatever it granted. A new group only applies to new logins, so it asks
you to log out and back in.
USAGE
}

MODE=install
DRY=0
TUNE=1
RT=1
for arg in "$@"; do
    case "$arg" in
        --home)      MODE=home ;;
        --uninstall) MODE=uninstall ;;
        --no-tune)   TUNE=0 ;;
        --no-rt)     RT=0 ;;
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

RT_BEGIN="# --- Tangent real-time scheduling (installer) ---"
RT_END="# --- end Tangent real-time scheduling ---"
LOADER_CONF=/boot/loader.conf
RT_GROUP=realtime

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
# what makes the small JACK periods legal at all, so it is not merely a
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

# **Which human asked for this.**
#
# ensure_root re-execs the script through doas or sudo, so by the time the
# grant runs $USER is root and the person who typed the command survives only
# in what the escalation tool left behind. Both set a variable; logname(1)
# reads the login name out of the session's utmpx entry and covers `su -` and
# a root login. Putting root in an unprivileged group would be pointless, so
# root is rejected at every step rather than silently "succeeding".
target_user() {
    for tu_v in "${SUDO_USER:-}" "${DOAS_USER:-}"; do
        if [ -n "$tu_v" ] && [ "$tu_v" != root ]; then
            printf '%s\n' "$tu_v"; return 0
        fi
    done
    tu_l="$(logname 2>/dev/null || true)"
    if [ -n "$tu_l" ] && [ "$tu_l" != root ]; then
        printf '%s\n' "$tu_l"; return 0
    fi
    return 1
}

# **The grant that decides whether the mixer uses one core or all of them.**
#
# Tangent renders lanes onto a worker pool, and the pool is gated on readiness:
# `rtwork` counts the workers that actually took the real-time policy, and the
# fork takes the serial walk whenever that count is short of the pool's width.
# That gate is not defensive programming, it is a measurement: an ordinary
# timeshare thread misses whole deadlines, and a fork-join is defined by its
# slowest worker, so a pool that cannot be scheduled is SLOWER than not
# forking at all.
#
# On FreeBSD the policy is SCHED_FIFO through pthread_setschedparam(3), which
# needs PRIV_SCHED_RTPRIO. Unprivileged, that call returns EPERM, the pool
# reports zero ready, and every track renders on the callback thread: on one
# core, silently, with no error the user ever sees. That silence is the whole
# reason this step exists: nothing in the UI can tell you it happened.
#
# mac_priority(4) is FreeBSD's answer and the direct counterpart of the Linux
# audio group: it grants PRIV_SCHED_RTPRIO and PRIV_SCHED_SETPOLICY to any
# process running with gid 47, the 'realtime' group. On a stock system that
# group ALREADY EXISTS in /etc/group and the module is already on disk; the
# two things missing are the load and the membership, which is all this does.
#
# The module is loaded now AND written to loader.conf, for the same reason the
# sysctl above is applied now and written to sysctl.conf: one for this boot,
# one for the next. The granted user is recorded inside the marked block so
# that --uninstall gives back precisely what this granted and nothing more: a
# user who was already in 'realtime' before Tangent existed keeps it.
grant_realtime() {
    [ "$RT" = 1 ] || { say "Skipping the real-time scheduling grant (--no-rt)."; return 0; }

    RT_USER="$(target_user || true)"
    if [ -z "$RT_USER" ]; then
        step "Real-time scheduling"
        say "    Could not tell which user to add to the '$RT_GROUP' group, so this"
        say "    was skipped. Without it the mixer renders on one core. To do it"
        say "    by hand:  pw groupmod $RT_GROUP -m <your-username>"
        return 0
    fi

    step "Granting real-time scheduling to $RT_USER (mac_priority + '$RT_GROUP' group)"

    if [ "$DRY" = 1 ]; then
        say "    would kldload mac_priority and append a marked block to $LOADER_CONF"
        say "    would run: pw groupmod $RT_GROUP -m $RT_USER"
        return 0
    fi

    # The group is created if a stripped system lacks it. gid 47 is not
    # arbitrary: it is the number mac_priority(4) defaults to, and the policy
    # matches on the NUMBER, so a group of this name at any other gid would
    # look right in /etc/group and grant nothing.
    if ! pw groupshow "$RT_GROUP" >/dev/null 2>&1; then
        pw groupadd "$RT_GROUP" -g 47 2>/dev/null \
            || say "    could not create the '$RT_GROUP' group"
    fi

    # Load for this boot. Already-loaded is success, not failure.
    if ! kldstat -q -m mac_priority 2>/dev/null; then
        kldload mac_priority >/dev/null 2>&1 \
            || say "    could not load mac_priority now; it will load at next boot"
    fi

    # And for the next one. Marked, so --uninstall can find exactly this and
    # leave anything the user wrote around it alone.
    if grep -q "^mac_priority_load" "$LOADER_CONF" 2>/dev/null; then
        say "    a mac_priority_load setting is already present; leaving it alone"
    else
        {
            printf '%s\n' "$RT_BEGIN"
            printf '%s\n' "# Lets members of the '$RT_GROUP' group (gid 47) use the real-time"
            printf '%s\n' "# scheduler. Tangent's render workers need it; without it the"
            printf '%s\n' "# mixer renders every track on one core. See mac_priority(4)."
            printf '%s\n' "# granted-to: $RT_USER"
            printf '%s\n' 'mac_priority_load="YES"'
            printf '%s\n' "$RT_END"
        } >> "$LOADER_CONF"
        say "    written to $LOADER_CONF for next boot"
    fi

    if id -Gn "$RT_USER" 2>/dev/null | grep -qw "$RT_GROUP"; then
        say "    $RT_USER was already in '$RT_GROUP'; leaving it alone"
        # Recorded as pre-existing so --uninstall does not take away a
        # membership this script did not grant.
        sed -i '' "s|^# granted-to: $RT_USER\$|# granted-to: (pre-existing)|" \
            "$LOADER_CONF" 2>/dev/null || true
    else
        pw groupmod "$RT_GROUP" -m "$RT_USER" \
            || say "    could not add $RT_USER to '$RT_GROUP'"
    fi
}

# Gives back exactly what grant_realtime took, and nothing it found already
# there. The module is left LOADED: unloading a MAC policy on a running system
# is not worth doing for an uninstall, and the loader.conf line going away
# means it is gone at the next boot anyway.
ungrant_realtime() {
    grep -q "^$RT_BEGIN$" "$LOADER_CONF" 2>/dev/null || return 0
    step "Removing the real-time scheduling grant"
    if [ "$DRY" = 1 ]; then
        say "    would remove the marked block from $LOADER_CONF and the group membership"
        return 0
    fi

    RT_WHO="$(sed -n "s|^# granted-to: ||p" "$LOADER_CONF" 2>/dev/null | head -1)"

    awk -v b="$RT_BEGIN" -v e="$RT_END" '
        $0 == b { skip = 1 } skip == 0 { print } $0 == e { skip = 0 }
    ' "$LOADER_CONF" > "$LOADER_CONF.tangent-new" &&
        mv "$LOADER_CONF.tangent-new" "$LOADER_CONF"

    if [ -n "$RT_WHO" ] && [ "$RT_WHO" != "(pre-existing)" ]; then
        pw groupmod "$RT_GROUP" -d "$RT_WHO" 2>/dev/null \
            && say "    $RT_WHO removed from '$RT_GROUP'" \
            || say "    could not remove $RT_WHO from '$RT_GROUP'"
    else
        say "    '$RT_GROUP' membership was not this installer's to remove; kept"
    fi
}

case "$MODE" in

install)
    APP_PKG="$(find_app_pkg)" || {
        echo "no ${PKGNAME}-*.pkg beside this script; is the folder complete?" >&2
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
    grant_realtime

    say ""
    step "Done"
    say "Run it from the Multimedia menu, or type: tangent"
    if [ "$TUNE" = 1 ]; then
        say ""
        say "If a USB audio interface is plugged in right now, unplug and replug"
        say "it once: the lower-latency buffering applies when a device attaches."
    fi
    # Said last and said plainly, because it is the one step the installer
    # cannot take for the user and the one whose absence is invisible: a
    # process keeps the groups it started with, so the running desktop session
    # is still without the grant no matter what /etc/group now says.
    if [ "$RT" = 1 ]; then
        say ""
        say "Log out and back in before using Tangent. A new group only applies"
        say "to new logins, and until then the mixer renders on a single core."
    fi
    say "To remove it later:  pkg delete $PKGNAME"
    ;;

home)
    APP_PKG="$(find_app_pkg)" || {
        echo "no ${PKGNAME}-*.pkg beside this script; is the folder complete?" >&2
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
        # an empty directory. tar strips the leading slash itself; the
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
    say "A home install cannot pull in dependencies: pkg is what does that, and"
    say "it needs root. If Tangent will not start, these are the two it cannot"
    say "do without: ALSA for sound, lilv for the plugin format FreeBSD has."
    say "    pkg install alsa-lib alsa-plugins lilv"
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
        # nothing else wants; `pkg autoremove` is the user's call to make, and
        # taking mesa off a machine because they uninstalled one app would be a
        # remarkable thing for an uninstaller to do unasked.
        run pkg delete -y "$PKGNAME"
        untune_audio
        ungrant_realtime
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
