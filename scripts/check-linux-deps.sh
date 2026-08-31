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

# **The glibc floor, enforced where the artifact is packed — when the caller
# declares one.** The README advertises a minimum glibc, but the number the
# loader actually enforces is the highest GLIBC_x.y in the binary's own
# version-requirement table, and the two disagreed for every 5.0 beta: the
# build targeted 2.32 while the only genuinely new symbol in the binary was
# gettid@GLIBC_2.30, so the artifacts refused Ubuntu 20.04, Debian 11 and
# openSUSE Leap 15.4/15.5 for nothing. A floor that lives in a comment is a
# floor nobody measures; this turns it into the same packaging-time contract
# as the DT_NEEDED allowlist above, and catches the next crate that quietly
# pulls in a 2.31+ symbol.
#
# The floor comes from the CALLER (build-cross.sh exports GLIBC_FLOOR from the
# same variable it hands to zigbuild) rather than living here, because only
# the cross artifacts make the promise: a native build binds whatever glibc
# the build box runs and carries that box's floor, so build-linux-native.sh
# passes nothing and gets the measurement printed instead of enforced.
GLIBC_MAX="$(python3 "$HERE/elf-needed.py" --max-glibc "$BIN")" || {
  echo "!! could not read $BIN's version requirements — see scripts/elf-needed.py" >&2
  exit 1
}
if [ -n "${GLIBC_FLOOR:-}" ] && [ -n "$GLIBC_MAX" ]; then
  if [ "$(printf '%s\n%s\n' "$GLIBC_FLOOR" "$GLIBC_MAX" | sort -V | tail -1)" != "$GLIBC_FLOOR" ]; then
    echo "!! $BIN requires GLIBC_$GLIBC_MAX but the artifact promises glibc >= $GLIBC_FLOOR." >&2
    echo "   Either something new in the dependency graph binds a symbol younger" >&2
    echo "   than the floor (find it: objdump -T | grep GLIBC_$GLIBC_MAX), or the" >&2
    echo "   floor moved — then raise GLIBC in build-cross.sh AND the user-facing" >&2
    echo "   claims in README.md, docs/RELEASE.md and docs/ARTIFACT-README.md" >&2
    echo "   together, because a floor the README understates is a loader error" >&2
    echo "   on a user's machine with the README insisting it should work." >&2
    exit 1
  fi
fi

# A RUNPATH would change the answer above — the loader would find bundled
# copies first — so say whether there is one rather than leaving it implied.
RUNPATH="$(python3 "$HERE/elf-needed.py" --runpath "$BIN" || true)"
printf '    DT_NEEDED  ok (%s)%s%s\n' \
  "$(echo "$NEEDED" | tr '\n' ' ' | sed 's/ *$//')" \
  "${GLIBC_MAX:+, max glibc ref $GLIBC_MAX${GLIBC_FLOOR:+ <= floor $GLIBC_FLOOR}}" \
  "${RUNPATH:+, RUNPATH=$RUNPATH}"
