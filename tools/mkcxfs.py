#!/usr/bin/env python3
"""
mkcxfs.py — build a CXFS flat archive from a list of files.

Usage:
    python3 tools/mkcxfs.py -o disk.cxfs file1 file2 ...

The output is a raw binary suitable for use as a QEMU disk image:
    qemu-system-x86_64 ... -drive file=disk.cxfs,format=raw,if=ide,index=1

CXFS format:
    [0..4]   magic "CXFS"
    [4..8]   u32 LE: file count N
    [8..]    N * 40-byte entries
      [+0..+32]  filename, null-terminated, zero-padded to 32 bytes
      [+32..+36] u32 LE: data offset from archive start
      [+36..+40] u32 LE: data size in bytes
    packed file data (no alignment padding)
"""

import argparse
import os
import struct
import sys

MAGIC      = b"CXFS"
NAME_LEN   = 32
ENTRY_SIZE = 40  # NAME_LEN + 4 + 4
SECTOR     = 512

def pack_name(name: str) -> bytes:
    b = name.encode("ascii")
    if len(b) >= NAME_LEN:
        sys.exit(f"error: filename too long (max {NAME_LEN - 1} chars): {name}")
    return b.ljust(NAME_LEN, b"\x00")

def build(files: list[str], output: str):
    entries = []
    blobs = []
    header_size = 8 + len(files) * ENTRY_SIZE
    cursor = header_size

    for path in files:
        name = os.path.basename(path)
        with open(path, "rb") as f:
            data = f.read()
        entries.append((name, cursor, len(data)))
        blobs.append(data)
        cursor += len(data)

    with open(output, "wb") as out:
        out.write(MAGIC)
        out.write(struct.pack("<I", len(files)))
        for name, offset, size in entries:
            out.write(pack_name(name))
            out.write(struct.pack("<II", offset, size))
        for data in blobs:
            out.write(data)

        # Pad to sector boundary so QEMU is happy with the raw image.
        pos = out.tell()
        remainder = pos % SECTOR
        if remainder:
            out.write(b"\x00" * (SECTOR - remainder))

    total = os.path.getsize(output)
    print(f"wrote {output}: {len(files)} file(s), {total} bytes "
          f"({total // SECTOR} sectors)")
    for name, offset, size in entries:
        print(f"  {name:30s}  offset={offset:#010x}  size={size} bytes")

def main():
    p = argparse.ArgumentParser(description="Build a CXFS archive")
    p.add_argument("-o", "--output", required=True)
    p.add_argument("files", nargs="+")
    args = p.parse_args()
    build(args.files, args.output)

if __name__ == "__main__":
    main()
