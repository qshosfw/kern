#!/usr/bin/env python3
"""XOR-obfuscate a raw firmware binary for the stock Quansheng bootloader."""

import sys

OBFUS = bytes([
    0x16, 0x6c, 0x14, 0xe6, 0x2e, 0x91, 0x0d, 0x40,
    0x21, 0x35, 0xd5, 0x40, 0x13, 0x03, 0xe9, 0x80,
])


def pack(fin: str, fout: str) -> None:
    with open(fin, "rb") as f:
        data = bytearray(f.read())

    # Pad to 16-byte boundary with 0xFF (erased flash value)
    rem = len(data) % 16
    if rem:
        data.extend(b"\xff" * (16 - rem))

    for i in range(len(data)):
        data[i] ^= OBFUS[i % 16]

    with open(fout, "wb") as f:
        f.write(data)


def main() -> int:
    if len(sys.argv) != 3:
        print(f"Usage: {sys.argv[0]} <input.bin> <output_packed.bin>", file=sys.stderr)
        return 2
    pack(sys.argv[1], sys.argv[2])
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
