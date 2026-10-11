#!/bin/sh
# Install Tangent.
#
#   ./install.sh                 into your home directory (no root)
#   ./install.sh --system        for every user, under /usr/local and
#                                /usr/share (needs root)
#   ./install.sh --prefix DIR    somewhere else entirely
#   ./install.sh --desktop       menu entry + icon, without being asked
#   ./install.sh --deps          install missing video-acceleration packages
#   ./install.sh --no-deps       never touch the package manager, just advise
#   ./install.sh --no-desktop    the binary only, no menu entry
#   ./install.sh --uninstall     take it all back out
#   ./install.sh --dry-run       print what would happen and do nothing
#
# WHY A SHELL SCRIPT AND NOT A .deb
#
# A .deb reaches Debian and Ubuntu. The same argument then repeats for rpm, for
# Arch, and for Void, which is the machine this build is made on. This runs on
# all of them, needs no root for the default, and can be read before it is run,
# which is what a Linux user is going to do anyway.
#
# POSIX sh, not bash: /bin/sh is dash on Debian and Ubuntu, and a bashism here
# would fail on the two distributions most people are running.
set -eu

SELF_DIR="$(cd "$(dirname "$0")" && pwd)"
NAME="Tangent"

# Written out rather than sed'd out of the header comment above. That trick
# needs a line range, the range was 2..26, and the comment grew, so --help
# printed the usage AND the first half of an essay about .deb packages.
usage() {
    cat <<'USAGE'
Install Tangent.

  ./install.sh                 into your home directory (no root)
  ./install.sh --system        for every user, under /usr/local and
                               /usr/share (needs root)
  ./install.sh --prefix DIR    somewhere else entirely
  ./install.sh --desktop       menu entry + icon, without being asked
  ./install.sh --no-desktop    the binary only, no menu entry
  ./install.sh --deps          install missing video-acceleration packages
  ./install.sh --no-deps       never touch the package manager, just advise
  ./install.sh --no-rt         skip the real-time scheduling grant
  ./install.sh --uninstall     take it all back out
  ./install.sh --dry-run       print what would happen and do nothing

The default needs no root. It puts the application in ~/.local/bin.

Desktop integration is the menu entry and the icon: it is what makes Tangent
appear in your application menu and in launchers like rofi, wofi and dmenu.
You are asked about it unless --desktop or --no-desktop says so; with no
terminal to ask in, it is installed.

Video acceleration is checked at the end. Tangent records correctly without it;
what it costs is processor: a take encoded on the CPU can use two and a half
cores where the GPU uses a twentieth of one. If something is missing you are
shown the exact command for your distribution and asked whether to run it.
--deps runs it without asking, --no-deps never touches the package manager.
Nothing here can fail the install.
USAGE
}

# Desktop integration: the .desktop entry and the icon, which is what puts
# Tangent in an application menu and in rofi/wofi/dmenu. Empty means "ask";
# see the prompt below. Not a plain 1, because "install it" and "the user has
# not said" are different states and the flags have to be able to say both.
WANT_DESKTOP=""
# Hardware video acceleration packages. Empty means "ask", for the same reason
# WANT_DESKTOP is: "install them" and "the user has not been asked" are
# different states, and a non-interactive run must be able to tell them apart.
WANT_DEPS=""
DRY=0
UNINSTALL=0
MODE=user
PREFIX=""
# **The real-time grant.** On by default for a system install, because
# without it Tangent renders every track on one core and sounds like a slow
# computer rather than like a missing permission. See `grant_realtime`.
RT=1
RT_LIMITS=/etc/security/limits.d/99-tangent-realtime.conf
RT_GROUP=audio

while [ $# -gt 0 ]; do
    case "$1" in
        # Still taken, so that a command line copied from an older page keeps
        # working. Each installs what the default installs.
        --app|--both) ;;
        --desktop)    WANT_DESKTOP=1 ;;
        --no-desktop) WANT_DESKTOP=0 ;;
        --deps)       WANT_DEPS=1 ;;
        --no-deps)    WANT_DEPS=0 ;;
        --system)    MODE=system ;;
        --user)      MODE=user ;;
        --prefix)    shift; [ $# -gt 0 ] || { echo "--prefix needs a directory" >&2; exit 2; }
                     PREFIX="$1"; MODE=prefix ;;
        --no-rt)     RT=0 ;;
        --uninstall) UNINSTALL=1 ;;
        --dry-run|-n) DRY=1 ;;
        -h|--help)   usage; exit 0 ;;
        *) echo "unknown option: $1  (try --help)" >&2; exit 2 ;;
    esac
    shift
done

# Where things go.
#
#   VST3_DIR is where an install before 4.77.0 could put a Tangent.vst3. It is
#   read only to take that leftover back out, here and by --uninstall.
case "$MODE" in
    user)
        BIN_DIR="$HOME/.local/bin"
        VST3_DIR="$HOME/.vst3"
        DESKTOP_DIR="$HOME/.local/share/applications"
        ICON_DIR="$HOME/.local/share/icons/hicolor/128x128/apps"
        ;;
    system)
        BIN_DIR="/usr/local/bin"
        VST3_DIR="/usr/lib/vst3"
        DESKTOP_DIR="/usr/share/applications"
        ICON_DIR="/usr/share/icons/hicolor/128x128/apps"
        ;;
    prefix)
        BIN_DIR="$PREFIX/bin"
        VST3_DIR="$PREFIX/lib/vst3"
        DESKTOP_DIR="$PREFIX/share/applications"
        ICON_DIR="$PREFIX/share/icons/hicolor/128x128/apps"
        ;;
esac

run() {
    if [ "$DRY" = "1" ]; then
        printf '  would: %s\n' "$*"
    else
        "$@"
    fi
}

# ── Real-time scheduling ────────────────────────────────────────────────────
#
# **Tangent's render workers ask the kernel for SCHED_FIFO, and a worker that
# cannot get it does not count.** `rtwork.rs` is explicit about the answer to
# a refused policy: stop forking and render serially. So a desktop without
# this permission does not fail, it renders every track on the callback
# thread and sounds exactly like a slow computer. Nothing in the interface
# can tell the user which one they have, which is precisely why the grant
# belongs in an installer rather than in a troubleshooting page.
#
# The mechanism is the audio group plus an `rtprio` line in `limits.d`.
# Group membership ALONE is usually not enough: on most distributions the
# group exists and nothing grants it anything, and the `limits.d` line is
# what `pam_limits` turns into an actual `RLIMIT_RTPRIO`. That is why this
# writes the file rather than only running `usermod`.
#
# `memlock` is deliberately NOT granted. Tangent never calls `mlock` (the
# source has no `mlock`, `MCL_CURRENT` or `memlock` anywhere), so asking for
# it would be taking a privilege the program does not use.
#
# 95 rather than 99: a real-time thread that outranks the kernel's own
# watchdogs is how a scheduling grant turns a glitch into a lockup. JACK and
# PipeWire both settle here for the same reason.
#
# Its own file rather than a marked block in limits.conf, because a file is
# what `limits.d` is for and removing it is the whole uninstall. The user we
# added is recorded inside it, so `--uninstall` gives back exactly what this
# granted and leaves alone anyone who was in the audio group already.

# Who to grant to. `sudo`/`doas` re-exec as root, so `$USER` is root by then.
rt_target_user() {
    for rt_v in "${SUDO_USER:-}" "${DOAS_USER:-}"; do
        if [ -n "$rt_v" ] && [ "$rt_v" != root ]; then
            printf '%s\n' "$rt_v"; return 0
        fi
    done
    rt_v=$(logname 2>/dev/null || true)
    if [ -n "$rt_v" ] && [ "$rt_v" != root ]; then
        printf '%s\n' "$rt_v"; return 0
    fi
    return 1
}

grant_realtime() {
    [ "$RT" = "1" ] || return 0
    # Only a system install can do this; a home install has no root and
    # should say so rather than look like it worked.
    if [ "$MODE" != system ]; then
        return 0
    fi
    if [ "$(id -u)" != 0 ]; then
        echo "  (not root, so the real-time grant was skipped)"
        return 0
    fi
    rt_user=$(rt_target_user || true)
    if [ -z "$rt_user" ]; then
        echo "  Could not tell which user to grant real-time scheduling to."
        echo "  Run this yourself, then log out and back in:"
        echo "      sudo usermod -aG $RT_GROUP <your-username>"
        return 0
    fi
    echo "Granting real-time scheduling to $rt_user"
    # The group first. Only add, and only record it if we DID add, so the
    # uninstall cannot take away something that was there before us.
    rt_added=0
    if getent group "$RT_GROUP" >/dev/null 2>&1; then
        if id -nG "$rt_user" 2>/dev/null | tr ' ' '\n' | grep -qx "$RT_GROUP"; then
            echo "  already in the $RT_GROUP group; leaving it alone"
        else
            run usermod -aG "$RT_GROUP" "$rt_user" && rt_added=1
        fi
    else
        echo "  no '$RT_GROUP' group on this system; writing the limit anyway"
    fi
    # And the limit, which is what actually grants it.
    if [ "$DRY" = "1" ]; then
        printf '  would: write %s\n' "$RT_LIMITS"
    else
        mkdir -p "$(dirname "$RT_LIMITS")"
        {
            echo "# --- Tangent real-time scheduling (installer) ---"
            echo "# Written by Tangent's installer. Remove with ./install.sh --uninstall"
            echo "# tangent-granted-user: $rt_user"
            echo "# tangent-added-to-group: $rt_added"
            echo "@$RT_GROUP   -   rtprio   95"
        } > "$RT_LIMITS"
        chmod 0644 "$RT_LIMITS"
    fi
    echo "  Log out and back in for this to take effect: a process keeps the"
    echo "  groups and limits it started with."
}

ungrant_realtime() {
    [ "$(id -u)" = 0 ] || return 0
    [ -f "$RT_LIMITS" ] || return 0
    grep -q "Tangent real-time scheduling (installer)" "$RT_LIMITS" 2>/dev/null || {
        echo "  $RT_LIMITS is not ours; leaving it alone"
        return 0
    }
    rt_user=$(sed -n 's/^# tangent-granted-user: //p' "$RT_LIMITS" | head -1)
    rt_added=$(sed -n 's/^# tangent-added-to-group: //p' "$RT_LIMITS" | head -1)
    run rm -f "$RT_LIMITS"
    # Only if WE put them there.
    if [ "$rt_added" = "1" ] && [ -n "$rt_user" ]; then
        if command -v gpasswd >/dev/null 2>&1; then
            run gpasswd -d "$rt_user" "$RT_GROUP" >/dev/null 2>&1 || true
        fi
    fi
    echo "  real-time scheduling grant removed"
}

# ── Hardware video acceleration ─────────────────────────────────────────────
#
# Tangent works without any of this, and that is why none of it is ever an
# error. What it buys is the difference between a take that costs a few percent
# of a core and one that costs two and a half: hardware MJPEG decode for the
# camera and hardware H.264 encode for the file. Measured on the machine this
# is built on, over 10 s of 720p, software x264 spent 25.54 s of CPU and VA-API
# spent 0.70 s.
#
# So: look for each piece, offer to install what is missing, and if that is not
# possible say the exact command. Never fail.

# The distribution family, from ID and then ID_LIKE.
#
# **ID_LIKE is the part that matters**, because it is what makes this work on
# the derivatives people actually run rather than only on the five names
# someone thought of. Zorin is `ID=zorin` with `ID_LIKE="ubuntu debian"`, Mint
# is `ID=linuxmint`, Pop is `ID=pop`: an enumerated list of names misses all
# of them silently, which is worse than not checking at all.
distro_family() {
    (
        # Overridable so the mapping can be TESTED against a real Zorin or Mint
        # os-release rather than reasoned about. Defaults to the real file.
        . "${OS_RELEASE:-/etc/os-release}" 2>/dev/null || exit 0
        for _id in "${ID:-}" ${ID_LIKE:-}; do
            case "$_id" in
                debian|ubuntu)       echo debian; exit 0 ;;
                fedora|rhel|centos)  echo fedora; exit 0 ;;
                arch)                echo arch;   exit 0 ;;
                void)                echo void;   exit 0 ;;
                suse|opensuse*|sles) echo suse;   exit 0 ;;
            esac
        done
    )
}

# **The one library whose absence stops Tangent from starting at all.**
#
# LV2 hosting links liblilv-0.so.0 outright, so a machine without it gets a
# loader error naming a library the user has never heard of, before a single
# line of the app runs. Everything else this script warns about degrades a
# feature; this one is the difference between a program and a paperweight.
#
# It arrives with PipeWire or with ffmpeg, so it is already present on Debian,
# Fedora, Arch, Void, openSUSE, Mint, Pop and every Ubuntu *flavour*. The one
# desktop that ships without it is stock Ubuntu GNOME, for a structural reason
# rather than an oversight: liblilv is in universe, `ubuntu-desktop` is in main,
# main may not depend on universe, so Canonical builds PipeWire with
# `-Dlv2=disabled` (Launchpad #2054223).
have_lilv() {
    # ldconfig's cache is the loader's own answer, which beats guessing at
    # directories on any glibc system.
    if command -v ldconfig >/dev/null 2>&1; then
        ldconfig -p 2>/dev/null | grep -q 'liblilv-0\.so\.0' && return 0
    fi
    # musl has no ldconfig cache and a minimal container may have no ldconfig
    # at all, so fall back to looking where the loader looks. `LILV_SEARCH`
    # overrides the list so this can be exercised on a machine that is not the
    # one it is written for, the same trick `OS_RELEASE` plays above.
    for _d in ${LILV_SEARCH:-/usr/lib /usr/lib64 /lib /lib64 /usr/local/lib \
              /usr/lib/x86_64-linux-gnu /usr/lib/aarch64-linux-gnu}; do
        [ -e "$_d/liblilv-0.so.0" ] && return 0
    done
    return 1
}

# Which GPU is in the render node, because the driver package differs per
# vendor and recommending the wrong one is worse than recommending nothing.
gpu_vendor() {
    for _v in /sys/class/drm/renderD*/device/vendor; do
        [ -r "$_v" ] || continue
        case "$(cat "$_v" 2>/dev/null)" in
            0x8086) echo intel;  return ;;
            0x1002) echo amd;    return ;;
            0x10de) echo nvidia; return ;;
        esac
    done
}

have_libva() {
    if command -v ldconfig >/dev/null 2>&1 &&
       ldconfig -p 2>/dev/null | grep -q "libva\.so\.2"; then
        return 0
    fi
    for _d in /usr/lib64 /usr/lib /usr/lib/x86_64-linux-gnu; do
        [ -e "$_d/libva.so.2" ] && return 0
    done
    return 1
}

# libva is only a dispatcher: it dlopens a per-vendor backend, and having the
# dispatcher without a backend is the common case on a minimal install. It is
# also the case that looks fine until something asks it to decode.
have_va_driver() {
    for _d in ${LIBVA_DRIVERS_PATH:-} /usr/lib64/dri /usr/lib/dri \
              /usr/lib/x86_64-linux-gnu/dri; do
        [ -d "$_d" ] || continue
        for _f in "$_d"/*_drv_video.so; do
            [ -e "$_f" ] && return 0
        done
    done
    return 1
}

# **Probe the ffmpeg the app will actually run, not the one on PATH.**
#
# `encode::ffmpeg::program()` resolves `$TANGENT_FFMPEG`, then the `tangent-ffmpeg`
# sitting beside the installed binary, and only then the bare name, so the
# BUNDLED copy wins over PATH. Checking `command -v ffmpeg` therefore answers a
# question nobody asked: a machine with a perfect system ffmpeg and a bundled
# copy built without VA-API would be told it was ready while every take encoded
# on the CPU. That was true of every Linux release before 4.39.
#
# `FFMPEG_TESTED` is left holding whichever binary was probed, because the
# remedy depends on which one it is and the message has to name it.
FFMPEG_TESTED=""
have_vaapi_ffmpeg() {
    FFMPEG_TESTED=""
    if [ -n "${TANGENT_FFMPEG:-}" ] && [ -x "${TANGENT_FFMPEG:-}" ]; then
        FFMPEG_TESTED="$TANGENT_FFMPEG"
    elif [ -x "$BIN_DIR/tangent-ffmpeg" ]; then
        FFMPEG_TESTED="$BIN_DIR/tangent-ffmpeg"
    elif [ -x "$SELF_DIR/tangent-ffmpeg" ]; then
        FFMPEG_TESTED="$SELF_DIR/tangent-ffmpeg"
    else
        FFMPEG_TESTED="$(command -v ffmpeg 2>/dev/null)"
    fi
    [ -n "$FFMPEG_TESTED" ] || return 1
    "$FFMPEG_TESTED" -hide_banner -hwaccels </dev/null 2>/dev/null | grep -q "^vaapi$"
}

# Is the encoder we probed the one we ship? If so, no package fixes it: the
# bundled copy takes precedence over anything the package manager installs.
tested_is_bundled() {
    case "$FFMPEG_TESTED" in
        "$BIN_DIR/tangent-ffmpeg"|"$SELF_DIR/tangent-ffmpeg") return 0 ;;
        *) return 1 ;;
    esac
}

# **Package names, and how far each one is trusted.** The Void row is verified
# on the machine this is built on. The Debian and Arch rows are the names those
# distributions actually ship and are high confidence. Fedora and openSUSE are
# from documentation rather than from a running system, which is why a failed
# install falls back to printing the command rather than pretending it worked.
#
# Fedora takes `ffmpeg-free` and not `ffmpeg`: the plain name lives in RPM
# Fusion, which a stock Fedora does not have enabled, so recommending it gives
# "No match for argument". `ffmpeg-free` is in the default repositories and is
# built with VA-API.
va_packages() {     # $1 family, $2 vendor
    case "$1:$2" in
        debian:intel) echo "i965-va-driver intel-media-va-driver vainfo" ;;
        debian:amd)   echo "mesa-va-drivers vainfo" ;;
        fedora:intel) echo "libva-intel-driver intel-media-driver libva-utils" ;;
        fedora:amd)   echo "mesa-va-drivers libva-utils" ;;
        arch:intel)   echo "libva-intel-driver intel-media-driver libva-utils" ;;
        arch:amd)     echo "libva-mesa-driver libva-utils" ;;
        void:intel)   echo "libva-intel-driver intel-media-driver libva-utils" ;;
        void:amd)     echo "mesa-vaapi libva-utils" ;;
        suse:intel)   echo "libva-intel-driver intel-media-driver libva-utils" ;;
        suse:amd)     echo "Mesa-libva libva-utils" ;;
    esac
}

install_cmd() {     # $1 family, rest packages
    _f="$1"; shift
    [ "$#" -gt 0 ] || return 1
    _sudo=""
    [ "$(id -u)" = "0" ] || _sudo="sudo "
    case "$_f" in
        debian) echo "${_sudo}apt install -y $*" ;;
        fedora) echo "${_sudo}dnf install -y $*" ;;
        arch)   echo "${_sudo}pacman -S --needed $*" ;;
        void)   echo "${_sudo}xbps-install -Sy $*" ;;
        suse)   echo "${_sudo}zypper install -y $*" ;;
        *)      return 1 ;;
    esac
}

# Look for every piece, install what is missing if allowed to, and otherwise
# print the command that fixes it. Called once, at the end of a successful
# install, and it never changes the exit status.
media_prereqs() {
    _fam="$(distro_family)"
    _ven="$(gpu_vendor)"

    # **The one prerequisite no package manager can supply.** Without a render
    # node there is nothing to accelerate against, and the reason is a driver
    # or a permission rather than a missing library.
    _node=""
    for _n in /dev/dri/renderD*; do
        [ -e "$_n" ] && { _node="$_n"; break; }
    done
    if [ -z "$_node" ]; then
        echo
        echo "  NOTE: no GPU render node (/dev/dri/renderD*) was found, so video"
        echo "        will be decoded and encoded on the CPU. Takes still work;"
        echo "        they cost a great deal more processor. This usually means"
        echo "        no graphics driver is loaded for your card."
        return 0
    fi
    if [ ! -r "$_node" ] || [ ! -w "$_node" ]; then
        echo
        echo "  NOTE: $_node exists but this account cannot open it, so video"
        echo "        will be handled on the CPU. Add yourself to its group,"
        echo "        then log out and back in - a new group does not apply to"
        echo "        a session that is already running:"
        echo "          sudo usermod -aG video,render \"${USER:-$(id -un)}\""
        return 0
    fi

    # NVIDIA can do this, but not through the VA-API path Tangent uses: it
    # needs the proprietary driver plus a translation layer, version-matched.
    # Saying so is more use than recommending a package that may not help.
    if [ "$_ven" = "nvidia" ]; then
        echo
        echo "  NOTE: NVIDIA graphics detected. Tangent's hardware video path is"
        echo "        VA-API, which NVIDIA does not provide directly, so takes"
        echo "        will encode on the CPU. Everything else works normally."
        return 0
    fi

    _miss_driver=0; _miss_ffmpeg=0
    have_libva     || _miss_driver=1
    have_va_driver || _miss_driver=1
    have_vaapi_ffmpeg || _miss_ffmpeg=1

    if [ "$_miss_driver" = "0" ] && [ "$_miss_ffmpeg" = "0" ]; then
        echo
        echo "  Hardware video acceleration: ready."
        return 0
    fi

    _pkgs="$(va_packages "$_fam" "$_ven")"
    [ "$_miss_driver" = "1" ] || _pkgs=""
    # **A bundled encoder without VA-API is not a package-manager problem.**
    # The copy beside the binary is resolved before PATH, so installing a system
    # ffmpeg would change nothing about which one runs. Flagged here, said after
    # the NOTE below, because advice before the problem reads backwards.
    _bundled_ffmpeg=0
    if [ "$_miss_ffmpeg" = "1" ]; then
        if tested_is_bundled; then
            _bundled_ffmpeg=1
        else
            case "$_fam" in
                fedora) _pkgs="$_pkgs ffmpeg-free" ;;
                *)      _pkgs="$_pkgs ffmpeg" ;;
            esac
        fi
    fi
    _pkgs="$(echo $_pkgs)"          # squeeze the spaces the two branches leave

    echo
    echo "  NOTE: hardware video acceleration is not available yet, so takes"
    if [ "$_miss_driver" = "1" ]; then
        echo "        will decode the camera on the CPU"
    fi
    if [ "$_miss_ffmpeg" = "1" ]; then
        echo "        will encode the video on the CPU"
        echo "        ($FFMPEG_TESTED has no VA-API in it)"
    fi
    echo "        Everything records correctly either way; this is about how"
    echo "        much processor a take costs."

    if [ "$_bundled_ffmpeg" = "1" ]; then
        echo
        echo "        Installing ffmpeg will NOT fix this: Tangent runs the copy"
        echo "        beside its own binary before anything on PATH, so that copy"
        echo "        is the one that decides. Use a build of 4.39.0 or later, or"
        echo "        point Tangent at an encoder that has VA-API:"
        echo "          TANGENT_FFMPEG=/usr/bin/ffmpeg tangent"
    fi

    # Nothing a package manager can supply: everything outstanding has been
    # explained above.
    [ -n "$_pkgs" ] || return 0

    _cmd="$(install_cmd "$_fam" $_pkgs)" || {
        echo "        Install your distribution's VA-API driver for your GPU,"
        echo "        and an ffmpeg built with VA-API."
        return 0
    }

    _do="$WANT_DEPS"
    if [ -z "$_do" ]; then
        # Ask only where there is somebody to answer. A piped or scripted
        # install must not stop on a question nobody can see.
        if [ -t 0 ] && [ -t 1 ]; then
            echo
            echo "        $_cmd"
            printf '  Run that now? [Y/n] '
            read -r _reply || _reply=""
            case "$_reply" in [Nn]*) _do=0 ;; *) _do=1 ;; esac
        else
            _do=0
        fi
    fi

    if [ "$_do" = "1" ]; then
        if [ "$DRY" = "1" ]; then
            printf '  would: %s\n' "$_cmd"
            return 0
        fi
        echo "  running: $_cmd"
        # Deliberately not fatal. A refused sudo, a held package or no network
        # is a reason to fall back to telling the user, not to fail an install
        # that has already succeeded.
        if sh -c "$_cmd"; then
            echo "  done. Restart Tangent if it is running."
        else
            echo
            echo "  That did not complete. You can run it yourself later:"
            echo "        $_cmd"
        fi
    else
        echo
        echo "  To enable it later:"
        echo "        $_cmd"
    fi
    return 0
}

# The nearest ancestor of $1 that exists.
#
# `--prefix /tmp/somewhere-new` is a directory that is SUPPOSED not to exist
# yet, so testing its parent for writability tested a directory that was also
# absent: `-w` said no and the script refused to install anywhere it had been
# asked to create. Walking up to something real is the question actually
# worth asking: may I create this?
nearest_existing() {
    d="$1"
    while [ ! -e "$d" ] && [ "$d" != "/" ] && [ "$d" != "." ]; do
        d="$(dirname "$d")"
    done
    printf '%s' "$d"
}

# Refuse early and clearly, rather than failing on the first copy.
if [ "$DRY" = "0" ] && [ "$MODE" != "user" ]; then
    if [ ! -w "$(nearest_existing "$BIN_DIR")" ] && [ "$(id -u)" != "0" ]; then
        echo "cannot write to $BIN_DIR: the nearest existing directory" >&2
        echo "($(nearest_existing "$BIN_DIR")) is not writable by you." >&2
        if [ "$MODE" = "system" ]; then
            echo "Re-run with sudo, or drop --system to install into your home" >&2
            echo "directory instead (no root needed)." >&2
        else
            echo "Re-run with sudo, or choose a --prefix you own." >&2
        fi
        exit 1
    fi
fi

if [ "$UNINSTALL" = "1" ]; then
    echo "Removing $NAME"
    ungrant_realtime
    run rm -f  "$BIN_DIR/tangent"
    run rm -f  "$BIN_DIR/tangent-ffmpeg"
    run rm -rf "$VST3_DIR/$NAME.vst3"
    run rm -f  "$DESKTOP_DIR/tangent.desktop"
    run rm -f  "$ICON_DIR/tangent.png"
    # And tell the desktop, or the menu keeps an entry for a binary that is
    # gone, which reads as a broken install rather than as a finished
    # uninstall. Best-effort, exactly as on the way in.
    if [ "$DRY" = "0" ]; then
        command -v update-desktop-database >/dev/null 2>&1 &&
            update-desktop-database "$DESKTOP_DIR" >/dev/null 2>&1 || true
        command -v gtk-update-icon-cache >/dev/null 2>&1 &&
            gtk-update-icon-cache -qtf "$(dirname "$(dirname "$(dirname "$ICON_DIR")")")" \
                >/dev/null 2>&1 || true
    fi
    echo
    echo "Removed. Your settings are still in ~/.config/tangent: taught chord"
    echo "names and a supporter key live there, so uninstalling does not throw"
    echo "them away. Delete that directory yourself if you want them gone."
    exit 0
fi

[ "$DRY" = "1" ] && echo "(dry run: nothing will be written)"
echo "Installing $NAME"

if [ ! -f "$SELF_DIR/tangent" ]; then
    echo "no 'tangent' binary next to this script" >&2
    exit 1
fi
# A Tangent.vst3 in VST3_DIR is a leftover of an install before 4.77.0,
# which every program that scans that folder would keep listing forever.
# Taken out here, and said out loud.
if [ -d "$VST3_DIR/$NAME.vst3" ]; then
    echo "  removing $VST3_DIR/$NAME.vst3, left by an older install"
    run rm -rf "$VST3_DIR/$NAME.vst3"
fi
echo "  application -> $BIN_DIR/tangent"
run mkdir -p "$BIN_DIR"
run cp "$SELF_DIR/tangent" "$BIN_DIR/tangent"
run chmod 755 "$BIN_DIR/tangent"

# The bundled video encoder. Prefixed `tangent-`, never plain `ffmpeg`:
# ~/.local/bin is on most users' PATH and an unprefixed copy there would
# shadow the system ffmpeg for every shell they own. Tangent looks for
# this name beside its own binary, so video works with nothing installed.
if [ -f "$SELF_DIR/tangent-ffmpeg" ]; then
    echo "  encoder     -> $BIN_DIR/tangent-ffmpeg"
    run cp "$SELF_DIR/tangent-ffmpeg" "$BIN_DIR/tangent-ffmpeg"
    run chmod 755 "$BIN_DIR/tangent-ffmpeg"
fi

# ── desktop integration ─────────────────────────────────────────────────────
#
# The .desktop entry and the icon. Between them they are what makes Tangent
# appear in an application menu and, more to the point, in the launchers
# people actually use on a tiling desktop (rofi, wofi, dmenu), all of
# which read $XDG_DATA_HOME/applications and nothing else.
#
# ASKED FOR rather than assumed. It writes three files outside the one
# directory the rest of this script touches, and somebody installing a
# binary into ~/.local/bin on a machine they keep tidy is entitled to be
# asked before their menu gains an entry. --desktop / --no-desktop skip the
# question; with no terminal to ask in, it happens, because that is what
# every release before this one did and a silent behaviour change is worse
# than a default somebody disagrees with.
if [ -f "$SELF_DIR/tangent.desktop" ]; then
    if [ -z "$WANT_DESKTOP" ]; then
        if [ -t 0 ]; then
            echo
            echo "  Desktop integration puts Tangent in your application menu"
            echo "  and in launchers like rofi, wofi and dmenu. It writes:"
            echo "      $DESKTOP_DIR/tangent.desktop"
            echo "      $ICON_DIR/tangent.png"
            printf '  Install it? [Y/n] '
            read -r reply || reply=""
            case "$reply" in
                [Nn]*) WANT_DESKTOP=0 ;;
                *)     WANT_DESKTOP=1 ;;
            esac
            echo
        else
            WANT_DESKTOP=1
        fi
    fi
else
    WANT_DESKTOP=0
fi

if [ "$WANT_DESKTOP" = "1" ]; then
    echo "  menu entry  -> $DESKTOP_DIR/tangent.desktop"
    run mkdir -p "$DESKTOP_DIR"
    # **The absolute path, not the bare name.** The shipped entry says
    # `Exec=tangent`, which needs $BIN_DIR to be on PATH, and a launcher
    # does not run your shell's PATH anyway: rofi and wofi exec through a
    # session environment that may never have sourced your profile. An
    # entry that cannot start the program is worse than no entry, because
    # it fails silently from a menu with nowhere to print an error.
    if [ "$DRY" = "1" ]; then
        printf '  would: write tangent.desktop with Exec=%s\n' "$BIN_DIR/tangent"
    else
        sed "s|^Exec=.*|Exec=$BIN_DIR/tangent|" \
            "$SELF_DIR/tangent.desktop" > "$DESKTOP_DIR/tangent.desktop"
        chmod 644 "$DESKTOP_DIR/tangent.desktop"
    fi

    if [ -f "$SELF_DIR/tangent.png" ]; then
        echo "  icon        -> $ICON_DIR/tangent.png"
        run mkdir -p "$ICON_DIR"
        run cp "$SELF_DIR/tangent.png" "$ICON_DIR/tangent.png"
        # 0644 explicitly: the build tarball has carried 0600 on this file
        # before, which is unreadable by anyone else after a --system
        # install and shows up as a menu entry with a missing icon.
        run chmod 644 "$ICON_DIR/tangent.png"
    fi

    # Tell the desktop it changed. Both are best-effort and both are
    # genuinely absent on minimal systems (a tiling-WM install often has
    # neither), so a missing one is not a failure. rofi reads the .desktop
    # files directly and will find Tangent either way; these are what make
    # a full desktop environment and the icon cache notice inside a second
    # rather than at the next login.
    if [ "$DRY" = "0" ]; then
        command -v update-desktop-database >/dev/null 2>&1 &&
            update-desktop-database "$DESKTOP_DIR" >/dev/null 2>&1 || true
        command -v gtk-update-icon-cache >/dev/null 2>&1 &&
            gtk-update-icon-cache -qtf "$(dirname "$(dirname "$(dirname "$ICON_DIR")")")" \
                >/dev/null 2>&1 || true
    fi
elif [ -f "$SELF_DIR/tangent.desktop" ]; then
    echo "  menu entry  -> skipped (--no-desktop)"
fi

# Say so rather than leaving them to find out by typing `tangent`.
ON_PATH=1
case ":${PATH}:" in
    *":$BIN_DIR:"*) ;;
    *) ON_PATH=0
       echo "  NOTE: $BIN_DIR is not on your PATH."
       echo "        Add it, or use the full path below." ;;
esac

echo
# **Before the dry-run exit, on purpose.** `--dry-run` is what a careful person
# runs first, and this is the one thing on the whole page they would want to
# learn from it: everything else here degrades a feature, and this decides
# whether the program starts. Saying it only on the real run tells them after
# the moment they were asking.
if ! have_lilv; then
    echo "  BEFORE YOU RUN IT: liblilv-0.so.0 is not installed, and Tangent"
    echo "        will not start without it - it is the LV2 plugin host, and"
    echo "        the program is linked against it. One command fixes it:"
    case "$(distro_family)" in
        debian) echo "          sudo apt install liblilv-0-0" ;;
        fedora) echo "          sudo dnf install lilv-libs" ;;
        arch)   echo "          sudo pacman -S lilv" ;;
        void)   echo "          sudo xbps-install lilv" ;;
        suse)   echo "          sudo zypper install liblilv-0-0" ;;
        *)      echo "          (install your distribution's lilv package)" ;;
    esac
    echo
fi
# **Before the dry-run exit**, so `--dry-run` reports the grant like it
# reports every other write. It was below it at first, which made the one
# option whose whole job is "tell me what you would do" silent about the
# only step that touches the system outside Tangent's own files.
#
# A grant is about the machine rather than the install, so it comes after
# the files are in place; and it prints a "log out and back in" that should
# be the last thing read rather than the first thing scrolled past.
grant_realtime
if [ "$RT" = "1" ] && [ "$MODE" != system ]; then
    echo
    echo "NOTE: Tangent spreads track rendering across cores, and the kernel"
    echo "      only allows that for users granted real-time scheduling."
    echo "      Without it every track renders on one core. A home install"
    echo "      cannot grant it; either re-run with --system, or:"
    echo "          sudo usermod -aG audio \"${USER:-$(id -un)}\""
    echo "          echo '@audio   -   rtprio   95' |"
    echo "            sudo tee /etc/security/limits.d/99-tangent-realtime.conf"
    echo "      then log out and back in."
fi
if [ "$DRY" = "1" ]; then
    echo "Nothing was written. Drop --dry-run to install."
    exit 0
fi
# The full path when `tangent` would not resolve. Printing "Run it with:
# tangent" four lines under "that directory is not on your PATH" is telling
# someone to do the thing you just told them would not work.
if [ "${ON_PATH:-1}" = "1" ]; then
    echo "Run it with:  tangent"
else
    echo "Run it with:  $BIN_DIR/tangent"
fi
# Said out loud, because the whole point of answering yes to that question
# is being able to start it without a terminal, and a launcher that has
# not rescanned yet looks exactly like an entry that was never written.
if [ "${WANT_DESKTOP:-0}" = "1" ]; then
    echo "Or from your application menu, rofi, wofi or dmenu, as \"Tangent\"."
    echo "  (a launcher that caches its list may need one restart to see it)"
fi

# Filming a take composites on the GPU through Vulkan. Everything else in
# Tangent works without it, so a missing driver is a note with the exact
# command, not an error. ICD manifests are how the loader finds drivers;
# no manifest anywhere means takes will record audio and MIDI but no video.
have_vulkan=0
for d in /usr/share/vulkan/icd.d /usr/local/share/vulkan/icd.d /etc/vulkan/icd.d; do
    if [ -d "$d" ] && [ -n "$(ls "$d"/*.json 2>/dev/null)" ]; then
        have_vulkan=1
        break
    fi
done
if [ "$have_vulkan" = "0" ]; then
    echo
    echo "  NOTE: no Vulkan driver was found, so takes will record audio and"
    echo "        MIDI but not video. Any Vulkan driver fixes it - mesa's"
    echo "        lavapipe works on every GPU:"
    # `distro_family` rather than a list of ID values: the list missed every
    # derivative that does not name itself after its parent. Zorin, Mint and
    # Pop all landed in the `*)` branch and were told to work it out
    # themselves, which is precisely who the note is for.
    case "$(distro_family)" in
        debian) echo "          sudo apt install mesa-vulkan-drivers" ;;
        fedora) echo "          sudo dnf install mesa-vulkan-drivers" ;;
        arch)   echo "          sudo pacman -S vulkan-swrast" ;;
        void)   echo "          sudo xbps-install mesa-vulkan-lavapipe" ;;
        suse)   echo "          sudo zypper install libvulkan1 Mesa-vulkan-device-select" ;;
        *)      echo "          (install your distribution's mesa Vulkan package)" ;;
    esac
fi

media_prereqs
echo "Hold H in Tangent to see every keyboard shortcut."
