#!/usr/bin/env python3
"""
tools/png_to_splash.py
Convert 128x64 images into:
1. Native ST7565 1024-byte page-column raw binary (.img) for KernFS (/user/splash.img)
2. Byte-run RLE compressed array for Rust ROM presets.
3. Terminal ASCII art verification.
"""

import sys
import os
from PIL import Image

def image_to_st7565_raw(im_path):
    im = Image.open(im_path).convert('1')
    if im.size != (128, 64):
        print(f"Resizing {im.size} to (128, 64)...")
        im = im.resize((128, 64))

    # ST7565: 8 pages (y // 8), 128 columns (x), bit (y % 8)
    # 1 = Black/energized pixel, 0 = White/reflective background
    raw = bytearray(1024)
    for y in range(64):
        page = y // 8
        bit = y % 8
        for x in range(128):
            pixel = im.getpixel((x, y))
            # 0 in '1' mode is black
            if pixel == 0:
                raw[page * 128 + x] |= (1 << bit)
    return bytes(raw)

def packbits_compress(data):
    """Standard Apple/TIFF PackBits byte-run RLE matching Kern boot splash engine."""
    out = bytearray()
    i = 0
    n = len(data)
    while i < n:
        run = 1
        while i + run < n and data[i + run] == data[i] and run < 128:
            run += 1

        if run >= 3:
            # Repeat run: (257 - run) as signed i8 / byte
            out.append((257 - run) & 0xFF)
            out.append(data[i])
            i += run
        else:
            # Literal run: find literal bytes until next repeat of >= 3
            lit_start = i
            while i < n and (i - lit_start) < 128:
                if i + 2 < n and data[i] == data[i + 1] == data[i + 2]:
                    break
                i += 1
            lit_len = i - lit_start
            if lit_len > 0:
                out.append(lit_len - 1)
                out.extend(data[lit_start:i])
    return bytes(out)

def rle_compress(data):
    return packbits_compress(data)

def preview_ascii(raw):
    for y in range(0, 64, 2):
        line = []
        for x in range(0, 128, 2):
            page1 = y // 8
            bit1 = y % 8
            p1 = (raw[page1 * 128 + x] >> bit1) & 1

            y2 = y + 1
            page2 = y2 // 8
            bit2 = y2 % 8
            p2 = (raw[page2 * 128 + x] >> bit2) & 1

            if p1 and p2:
                line.append('█')
            elif p1:
                line.append('▀')
            elif p2:
                line.append('▄')
            else:
                line.append(' ')
        print(''.join(line))

def main():
    args = [a for a in sys.argv[1:] if not a.startswith('--')]
    flags = [a for a in sys.argv[1:] if a.startswith('--')]

    if len(args) < 1:
        print(f"Usage: {sys.argv[0]} <image.png> [output.img|output.rle] [--invert] [--rust]")
        sys.exit(1)

    in_path = args[0]
    out_path = args[1] if len(args) >= 2 else None
    invert = '--invert' in flags
    dump_rust = '--rust' in flags

    raw = image_to_st7565_raw(in_path)
    if invert:
        raw = bytes(b ^ 0xFF for b in raw)

    rle = packbits_compress(raw)

    print(f"Input: {in_path}")
    print(f"Raw ST7565 VRAM: {len(raw)} bytes")
    print(f"PackBits RLE: {len(rle)} bytes ({len(rle)/len(raw)*100:.1f}%)")
    print("\n--- ASCII Preview ---")
    preview_ascii(raw)
    print("---------------------\n")

    if dump_rust:
        var_name = os.path.splitext(os.path.basename(in_path))[0].upper().replace('-', '_').replace(' ', '_')
        print(f"pub static {var_name}_RLE: [u8; {len(rle)}] = [")
        for chunk_idx in range(0, len(rle), 12):
            chunk = rle[chunk_idx:chunk_idx+12]
            formatted = ", ".join(f"0x{b:02X}" for b in chunk)
            print(f"    {formatted},")
        print("];\n")

    if out_path:
        with open(out_path, 'wb') as f:
            if out_path.endswith('.rle'):
                f.write(rle)
                print(f"Saved PackBits RLE image to {out_path} ({len(rle)} bytes)")
            else:
                f.write(raw)
                print(f"Saved raw ST7565 VRAM image to {out_path} ({len(raw)} bytes)")

if __name__ == '__main__':
    main()
