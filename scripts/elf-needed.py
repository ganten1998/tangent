#!/usr/bin/env python3
"""Print an ELF binary's DT_NEEDED list, one library per line.

**Why this exists rather than `readelf -d`.** The Linux artifacts are packed on
a macOS host, where no binutils that understands a Linux ELF is guaranteed to
be installed. `build-cross.sh` needs the answer on every run — a dependency
that appears without anyone noticing is exactly how `liblilv-0.so.0` came to
be in every 5.0 beta tarball, undeclared and unshipped, and the tarball started
nowhere it was not already installed. A guard that only runs where binutils
happens to exist is a guard that is off on the machine that cuts releases.

Prints DT_NEEDED entries to stdout. `--runpath` prints DT_RUNPATH/DT_RPATH
instead, empty when there is none. Exits non-zero, with a reason on stderr, if
the file is not an ELF64 it can read.
"""

import struct
import sys

DT_NULL, DT_NEEDED, DT_RPATH, DT_STRTAB, DT_RUNPATH = 0, 1, 15, 5, 29
PT_LOAD, PT_DYNAMIC = 1, 2


def read(path):
    with open(path, "rb") as f:
        d = f.read()
    if d[:4] != b"\x7fELF":
        raise SystemExit(f"{path}: not an ELF file")
    if d[4] != 2:
        raise SystemExit(f"{path}: not ELF64; nothing we ship is 32-bit")
    e = "<" if d[5] == 1 else ">"
    (phoff,) = struct.unpack_from(e + "Q", d, 0x20)
    phentsize, phnum = struct.unpack_from(e + "HH", d, 0x36)

    def segments():
        for i in range(phnum):
            o = phoff + i * phentsize
            (kind,) = struct.unpack_from(e + "I", d, o)
            offset, vaddr = struct.unpack_from(e + "QQ", d, o + 0x08)
            (filesz,) = struct.unpack_from(e + "Q", d, o + 0x20)
            yield kind, offset, vaddr, filesz

    dyn = next(((o, sz) for k, o, _, sz in segments() if k == PT_DYNAMIC), None)
    if dyn is None:
        return [], ""
    dyn_off, dyn_sz = dyn

    entries, strtab = [], None
    for k in range(dyn_sz // 16):
        tag, val = struct.unpack_from(e + "qQ", d, dyn_off + k * 16)
        if tag == DT_NULL:
            break
        entries.append((tag, val))
        if tag == DT_STRTAB:
            strtab = val
    if strtab is None:
        raise SystemExit(f"{path}: a dynamic section with no string table")

    # DT_STRTAB is a virtual address. Map it back through the PT_LOADs, which
    # is the only way to find it in the file: a stripped binary has no section
    # headers to look it up in, and everything we ship is stripped.
    base = next(
        (
            o + (strtab - v)
            for k, o, v, sz in segments()
            if k == PT_LOAD and v <= strtab < v + sz
        ),
        None,
    )
    if base is None:
        raise SystemExit(f"{path}: the string table is in no loadable segment")

    def string(off):
        return d[base + off : d.index(b"\0", base + off)].decode()

    needed = [string(v) for t, v in entries if t == DT_NEEDED]
    # DT_RUNPATH is the modern spelling; DT_RPATH is what older linkers wrote,
    # and the difference matters — RPATH is inherited by a dependency's own
    # lookups and RUNPATH is not. Report whichever is present.
    path_tag = next((string(v) for t, v in entries if t in (DT_RUNPATH, DT_RPATH)), "")
    return needed, path_tag


def main(argv):
    want_runpath = "--runpath" in argv
    files = [a for a in argv[1:] if not a.startswith("-")]
    if not files:
        raise SystemExit("usage: elf-needed.py [--runpath] BINARY...")
    for path in files:
        needed, runpath = read(path)
        if want_runpath:
            print(runpath)
        else:
            for lib in needed:
                print(lib)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
