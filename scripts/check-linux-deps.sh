#!/bin/sh
# Refuse a Linux binary that will ask the loader for something nobody declared.
#
# Usage: scripts/check-linux-deps.sh path/to/tangent
#
# **Why this is one file and not a block in each build script.** There are two
# packaging paths — `build-cross.sh`'s `package_linux()` on a macOS host, and
# `build-linux-native.sh` on a Linux one — and a list kept in both is a list
# that disagrees with itself the first time somebody edits one of them. The
# whole point of the check is that a dependency cannot arrive unnoticed; two
# copies would reintroduce exactly that.
#
# `build-freebsd.sh` has asserted the same property since the port existed, by
# reading the pkg manifest. The tarball paths never inherited it, and the
# consequence arrived precisely as that script's comment predicts: hosting LV2
# put `liblilv-0.so.0` into `DT_NEEDED`, nobody saw it, and every 5.0 beta
# tarball was a program that does not start on a machine without lilv — with a
# loader error naming a library the user has never heard of.
set -eu

BIN="${1:-}"
[ -n "$BIN" ] || { echo "usage: $0 BINARY" >&2; exit 2; }
[ -f "$BIN" ] || { echo "$0: no such file: $BIN" >&2; exit 2; }

# **The contract.** Every name here is either one `docs/ARTIFACT-README.md`
# tells the user to install, or one that no Linux system lacks. Adding a name
# without adding it to that README — and to the preflight in
# `installer/linux/install.sh` — is how the last one got through.
#
#   libasound.so.2   MIDI and audio. README declares it; every desktop has it.
#   liblilv-0.so.0   The LV2 host. Arrives with PipeWire or ffmpeg, so it is
#                    present on Debian, Fedora, Arch, Void, openSUSE, Mint, Pop
#                    and every Ubuntu *flavour* — and absent on stock Ubuntu
#                    GNOME, structurally: liblilv is in universe, ubuntu-desktop
#                    is in main, main may not depend on universe, so Canonical
#                    builds PipeWire with -Dlv2=disabled (Launchpad #2054223).
#                    README declares it and install.sh checks for it.
#   the toolchain    libm/libc/libpthread/libdl, and libgcc_s for unwinding.
#     runtime        A system without these is not a system. `libpthread` and
#                    `libdl` are folded into libc from glibc 2.34, so a modern
#                    native build names neither and an older one names both.
#
# **`libgcc_s.so.1` is here because the CROSS artifacts do not name it and the
# NATIVE ones do.** zigbuild links compiler-rt statically; the gcc toolchain on
# a Linux box links libgcc_s. The first version of this list was read off the
# cross-built binaries alone, so it would have refused every native build — the
# same mistake this whole check exists to catch, made while writing the check.
# The list has to be the UNION of what every packaging path produces, and it is
# one file precisely so that union has somewhere to live.
#
# No loader entries: the ELF interpreter lives in PT_INTERP, not DT_NEEDED, so
# `ld-linux-*` can never appear in what `elf-needed.py` reads.
ALLOWED=" \
  libasound.so.2 \
  liblilv-0.so.0 \
  libgcc_s.so.1 \
  libm.so.6 libc.so.6 libpthread.so.0 libdl.so.2 "

HERE="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
NEEDED="$(python3 "$HERE/elf-needed.py" "$BIN")" || {
  echo "!! could not read $BIN's DT_NEEDED — see scripts/elf-needed.py" >&2
  exit 1
}

for lib in $NEEDED; do
  case "$ALLOWED" in
    *" $lib "*) ;;
    *)
      echo "!! $BIN needs $lib and nothing tells the user to install it." >&2
      echo "   Either ship it in the tarball, or declare it in all three" >&2
      echo "   places: the Linux section of docs/ARTIFACT-README.md, the" >&2
      echo "   ALLOWED list in this script, and have_lilv's neighbourhood in" >&2
      echo "   installer/linux/install.sh. A new DT_NEEDED that nobody" >&2
      echo "   declares is a tarball that starts on the machine that built it" >&2
      echo "   and nowhere else." >&2
      exit 1 ;;
  esac
done

# A RUNPATH would change the answer above — the loader would find bundled
# copies first — so say whether there is one rather than leaving it implied.
RUNPATH="$(python3 "$HERE/elf-needed.py" --runpath "$BIN" || true)"
printf '    DT_NEEDED  ok (%s)%s\n' \
  "$(echo "$NEEDED" | tr '\n' ' ' | sed 's/ *$//')" \
  "${RUNPATH:+, RUNPATH=$RUNPATH}"
