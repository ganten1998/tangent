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
instead, empty when there is none. `--max-glibc` prints the highest
GLIBC_x.y version the binary's version-requirement table (DT_VERNEED)
references, bare ("2.30"), or nothing for a binary with no glibc refs — the
number a user's too-old distro would print in its loader error, and therefore
the artifact's REAL glibc floor regardless of what any script or README
claims. `check-linux-deps.sh` compares it against the floor build-cross.sh
targets, because the two disagreed for every 5.0 beta and nothing measured it.
Exits non-zero, with a reason on stderr, if the file is not an ELF64 it can
read.
"""

import re
import struct
import sys

DT_NULL, DT_NEEDED, DT_RPATH, DT_STRTAB, DT_RUNPATH = 0, 1, 15, 5, 29
DT_VERNEED, DT_VERNEEDNUM = 0x6FFFFFFE, 0x6FFFFFFF
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
        return [], "", []
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

    # DT_STRTAB (and DT_VERNEED below) are virtual addresses. Map them back
    # through the PT_LOADs, which is the only way to find them in the file: a
    # stripped binary has no section headers to look them up in, and
    # everything we ship is stripped.
    def vaddr_to_offset(vaddr):
        return next(
            (
                o + (vaddr - v)
                for k, o, v, sz in segments()
                if k == PT_LOAD and v <= vaddr < v + sz
            ),
            None,
        )

    base = vaddr_to_offset(strtab)
    if base is None:
        raise SystemExit(f"{path}: the string table is in no loadable segment")

    def string(off):
        return d[base + off : d.index(b"\0", base + off)].decode()

    needed = [string(v) for t, v in entries if t == DT_NEEDED]
    # DT_RUNPATH is the modern spelling; DT_RPATH is what older linkers wrote,
    # and the difference matters — RPATH is inherited by a dependency's own
    # lookups and RUNPATH is not. Report whichever is present.
    path_tag = next((string(v) for t, v in entries if t in (DT_RUNPATH, DT_RPATH)), "")

    # The version-requirement table: one Verneed per DT_NEEDED library that is
    # versioned at all, each chaining Vernaux records naming the versions
    # ("GLIBC_2.30", "GLIBC_2.2.5", ...) this binary's symbols bind to. The
    # loader refuses the binary when any of them is absent, so the highest one
    # IS the floor — DT_NEEDED says which libraries, this says how new.
    versions = []
    verneed = next((v for t, v in entries if t == DT_VERNEED), None)
    verneednum = next((v for t, v in entries if t == DT_VERNEEDNUM), 0)
    if verneed is not None and verneednum:
        off = vaddr_to_offset(verneed)
        if off is None:
            raise SystemExit(f"{path}: DT_VERNEED is in no loadable segment")
        for _ in range(verneednum):
            _, vn_cnt = struct.unpack_from(e + "HH", d, off)
            _, vn_aux, vn_next = struct.unpack_from(e + "III", d, off + 4)
            aux = off + vn_aux
            for _ in range(vn_cnt):
                _, _, _, vna_name, vna_next = struct.unpack_from(e + "IHHII", d, aux)
                versions.append(string(vna_name))
                if vna_next == 0:
                    break
                aux += vna_next
            if vn_next == 0:
                break
            off += vn_next
    return needed, path_tag, versions


def max_glibc(versions):
    # Numeric GLIBC_x.y[.z] only: names like GLIBC_ABI_DT_RELR are feature
    # markers, not floors this comparison could order.
    best = None
    for name in versions:
        m = re.fullmatch(r"GLIBC_(\d+(?:\.\d+)*)", name)
        if m:
            t = tuple(int(x) for x in m.group(1).split("."))
            best = t if best is None or t > best else best
    return ".".join(str(x) for x in best) if best else ""


def main(argv):
    want_runpath = "--runpath" in argv
    want_max_glibc = "--max-glibc" in argv
    files = [a for a in argv[1:] if not a.startswith("-")]
    if not files:
        raise SystemExit("usage: elf-needed.py [--runpath|--max-glibc] BINARY...")
    for path in files:
        needed, runpath, versions = read(path)
        if want_max_glibc:
            print(max_glibc(versions))
        elif want_runpath:
            print(runpath)
        else:
            for lib in needed:
                print(lib)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
