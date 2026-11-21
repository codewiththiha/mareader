#!/usr/bin/env python3
"""Strip the ad hoc signature cargo-packager puts on a .dmg.

Usage: python3 tools/unsign-dmg.py IMAGE.dmg...

macOS checks a signed disk image when it opens. An ad hoc signature
can't pass, so the image itself is refused ("Apple could not verify
...dmg"), and Open Anyway is the only way past. An unsigned image just
opens, and the app inside, still signed ad hoc, gets the one prompt the
release notes describe. With a Developer ID, sign and leave it alone.

The signature sits just before the image's trailer (the 512-byte koly
block), which records its offset and length. Unsigning drops those bytes
and zeroes both fields; the checksums cover only the image's data.
"""

import struct
import sys

KOLY = 512
SIG_OFFSET = 0x128
SIG_LENGTH = 0x130


def unsign(path: str) -> str:
    with open(path, "rb") as f:
        data = f.read()
    if len(data) < KOLY or data[-KOLY:][:4] != b"koly":
        raise SystemExit(f"{path}: not a UDIF disk image")
    koly = bytearray(data[-KOLY:])
    (offset,) = struct.unpack_from(">Q", koly, SIG_OFFSET)
    (length,) = struct.unpack_from(">Q", koly, SIG_LENGTH)
    if length == 0:
        return "already unsigned"
    end = len(data) - KOLY
    if offset + length != end:
        raise SystemExit(f"{path}: signature isn't just before the trailer")
    if data[offset : offset + 4] != b"\xfa\xde\x0c\xc0":
        raise SystemExit(f"{path}: no signature blob where the trailer says")
    struct.pack_into(">QQ", koly, SIG_OFFSET, 0, 0)
    with open(path, "wb") as f:
        f.write(data[:offset])
        f.write(koly)
    return f"dropped a {length}-byte signature"


if __name__ == "__main__":
    if len(sys.argv) < 2:
        raise SystemExit(__doc__)
    for image in sys.argv[1:]:
        print(f"{image}: {unsign(image)}")
