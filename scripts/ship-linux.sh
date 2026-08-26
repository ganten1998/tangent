#!/usr/bin/env bash
# ship-linux.sh — put the Linux tarball built HERE onto a Linux box, install it,
# and prove it runs.
#
# **Everything is compiled on this machine now.** `build-cross.sh` builds the
# Linux legs with cargo-zigbuild against a Debian bullseye ALSA sysroot, which
# is both faster than the remote box and does not require it to be awake. What
# the remote box is still for is the half a cross-build cannot do: a
# cross-build proves the binary LINKS, and only running it on Linux proves it
# starts, finds ALSA at run time and opens a window.
#
# So this ships and verifies; it does not build. `build-linux-remote.sh` is
# still there for building natively when that is wanted — a toolchain
# regression, a glibc question, or the suite on Linux.
#
#   scripts/ship-linux.sh void            # x86_64, the default
#   scripts/ship-linux.sh void aarch64
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

HOST="${1:-}"
ARCH="${2:-x86_64}"
if [ -z "$HOST" ]; then
  echo "usage: scripts/ship-linux.sh <ssh-host> [x86_64|aarch64]" >&2
  exit 2
fi

VERSION="$(grep '^version' Cargo.toml | head -1 | sed -E 's/.*"([^"]+)".*/\1/')"
TARBALL="dist/tangent-${VERSION}-linux-${ARCH}.tar.gz"
[ -f "$TARBALL" ] || {
  echo "no $TARBALL — run scripts/build-cross.sh first" >&2
  exit 1
}

echo "==> $TARBALL -> $HOST"
ssh -o BatchMode=yes -o ConnectTimeout=10 "$HOST" true 2>/dev/null || {
  echo "cannot ssh to '$HOST' without a password. Try: ssh $HOST true" >&2
  exit 1
}
scp -q "$TARBALL" "$HOST:/tmp/"

# Unpacked into a scratch directory and installed to ~/.local/bin, which is
# what a person testing a build actually wants on their PATH.
#
# The `rm` of older tarballs is deliberately a `find`, not a glob: an unmatched
# glob is a hard ERROR in zsh and merely expands to itself in sh, and either
# way it breaks an `&&` chain for a reason that has nothing to do with the
# release. That has already cost one silent skip of the verify step.
ssh "$HOST" "set -e
  cd /tmp
  rm -rf tangent-ship && mkdir tangent-ship
  tar xzf /tmp/$(basename "$TARBALL") -C tangent-ship
  install -m755 tangent-ship/*/tangent \"\$HOME/.local/bin/tangent\"
  rm -rf tangent-ship
  find \"\$HOME/Desktop\" -maxdepth 1 -name 'tangent-*-linux-*.tar.gz' -delete 2>/dev/null || true
"

# **The point of the whole script.** A version string proves the right build
# landed; `--list` proves the thing actually starts on Linux and can reach the
# MIDI subsystem, which is exactly what a cross-build cannot tell you.
echo "==> verifying on $HOST"
STAMP="$(ssh "$HOST" "strings \"\$HOME/.local/bin/tangent\" | grep -m1 -o 'Tangent ${VERSION}'" || true)"
[ -n "$STAMP" ] || { echo "!! installed binary is not $VERSION" >&2; exit 1; }
ssh "$HOST" "\"\$HOME/.local/bin/tangent\" --list >/dev/null 2>&1" || {
  echo "!! $VERSION installed but will not run on $HOST" >&2
  exit 1
}
echo "==> OK  $STAMP installed and running on $HOST"
