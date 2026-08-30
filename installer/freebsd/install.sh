#!/bin/sh
# Install Tangent on FreeBSD.
#
#   ./install.sh                 install for everyone (asks for root)
#   ./install.sh --home          install for just you, no root, no pkg
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
USAGE
}

MODE=install
DRY=0
for arg in "$@"; do
    case "$arg" in
        --home)      MODE=home ;;
        --uninstall) MODE=uninstall ;;
        --dry-run)   DRY=1 ;;
        -h|--help)   usage; exit 0 ;;
        *) echo "unknown option: $arg" >&2; echo >&2; usage >&2; exit 2 ;;
    esac
done

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
    step "Done"
    say "Run it from the Multimedia menu, or type: tangent"
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
