#!/usr/bin/env python3
"""Say whether a Windows PE carries an Authenticode signature.

**Why this exists rather than `osslsigncode verify`.** The Windows artifacts
are built AND gated on a macOS host, where no tool that understands
Authenticode is guaranteed to be installed — and a signature gate that only
runs where a tool happens to exist is a gate that is off on the machine that
cuts releases. Same reasoning, same shape as `elf-needed.py` next door.

Presence, not validity. The Attribute Certificate Table (data directory
entry 4 of the optional header) either has bytes in it or it does not — which
is the same depth of check the .pkg branch gets from `pkgutil
--check-signature | grep 'Status: signed'`. Chain validity and the SmartScreen
verdict are only ever proven by a first launch on a real Windows machine.

Exit 0 (and print the table's size) when a signature is present, exit 1 when
the PE is clean but carries none, exit 2 with a reason on stderr when the file
cannot be read as a PE at all. Callers must treat 2 as suspicious, not as
unsigned — a truncated download would land there.
"""

import struct
import sys

# Index of the Attribute Certificate Table in the optional header's data
# directory, per the PE/COFF spec. Unlike every other entry its first field is
# a FILE offset rather than an RVA — irrelevant here, where only Size is read.
CERT_TABLE_INDEX = 4


def die(msg):
    # SystemExit with a string exits 1, which the caller reads as "unsigned" —
    # precisely the confusion the exit-code contract above forbids.
    print(msg, file=sys.stderr)
    sys.exit(2)


def cert_table_size(path):
    try:
        with open(path, "rb") as f:
            d = f.read()
    except OSError as e:
        die(f"{path}: {e}")
    if d[:2] != b"MZ":
        die(f"{path}: not a PE (no MZ header)")
    try:
        (pe_off,) = struct.unpack_from("<I", d, 0x3C)
        if d[pe_off : pe_off + 4] != b"PE\0\0":
            die(f"{path}: not a PE (no PE\\0\\0 at e_lfanew)")
        opt = pe_off + 24
        (magic,) = struct.unpack_from("<H", d, opt)
    except struct.error:
        die(f"{path}: truncated before the PE headers")
    # 0x10b = PE32, 0x20b = PE32+ (64-bit). Everything shipped is PE32+, but
    # the check costs nothing and a 32-bit stray should be reported, not lied
    # about.
    if magic == 0x10B:
        num_off, dir_off = opt + 92, opt + 96
    elif magic == 0x20B:
        num_off, dir_off = opt + 108, opt + 112
    else:
        die(f"{path}: unknown optional-header magic {magic:#x}")
    try:
        (num_dirs,) = struct.unpack_from("<I", d, num_off)
        if num_dirs <= CERT_TABLE_INDEX:
            return 0
        _, size = struct.unpack_from("<II", d, dir_off + CERT_TABLE_INDEX * 8)
    except struct.error:
        die(f"{path}: truncated inside the optional header")
    return size


def main(argv):
    if len(argv) != 2:
        die("usage: pe-signed.py FILE.exe")
    size = cert_table_size(argv[1])
    if size == 0:
        print("unsigned")
        return 1
    print(f"signed ({size} bytes of certificate table)")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
