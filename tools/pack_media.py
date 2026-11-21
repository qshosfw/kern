#!/usr/bin/env python3
"""Kern Media & Localization Pack Compiler & Inspector (`.icn`, `.fnt`, `.lng`).

Zero external dependencies (Python stdlib only). Produces fixed-stride power-of-two
binary packs ready to deploy onto KernFS (`/sys/icons/*.icn`, `/sys/fonts/*.fnt`, `/sys/lang/*.lng`).

Usage:
    python3 tools/pack_media.py pack-icons assets/icons/kern_kit.txt assets/media/kern_kit.icn
    python3 tools/pack_media.py pack-bdf assets/fonts/Tiny5.bdf assets/media/tiny5.fnt
    python3 tools/pack_media.py pack-lang assets/lang/ja.toml assets/media/lang/ja.lng
    python3 tools/pack_media.py inspect assets/media/lang/ja.lng
"""
from __future__ import annotations

import argparse
import struct
import sys
import tomllib
import unicodedata
from pathlib import Path

FILE_ID_ICON_PACK = b"ICN1"
FILE_ID_FONT_PACK = b"FNT1"
FILE_ID_LANG_PACK = b"LNG1"


def fnv1a_32(data: bytes) -> int:
    h = 0x811C9DC5
    for b in data:
        h ^= b
        h = (h * 0x01000193) & 0xFFFFFFFF
    return h


def fnv1a_32_short(data: bytes) -> int:
    return ((fnv1a_32(data) ^ 0x1F) * 0x01000193) & 0xFFFFFFFF


def crc16_ccitt(data: bytes) -> int:
    crc = 0xFFFF
    for b in data:
        crc ^= b << 8
        for _ in range(8):
            if crc & 0x8000:
                crc = ((crc << 1) ^ 0x1021) & 0xFFFF
            else:
                crc = (crc << 1) & 0xFFFF
    return crc


def pack_rows_msb(rows: list[str], width: int) -> bytes:
    row_bytes = (width + 7) // 8
    out = bytearray()
    for r in rows:
        if len(r) != width:
            raise ValueError(f"row '{r}' has width {len(r)}, expected {width}")
        for byte_i in range(row_bytes):
            val = 0
            for bit in range(8):
                col = byte_i * 8 + bit
                if col < width and r[col] == "#":
                    val |= 0x80 >> bit
            out.append(val)
    return bytes(out)


def parse_icon_catalog(path: Path):
    """Parse readable ASCII icon catalog into metadata + [(id, name, category, rows)]."""
    meta = {"cell_w": 8, "cell_h": 8, "stride": 8, "base_id": 0, "var_width": False}
    icons: list[tuple[int, str, str, list[str]]] = []
    cur_cat = "General"
    cur_id = None
    cur_name = None
    cur_rows: list[str] = []

    def flush():
        nonlocal cur_id, cur_name, cur_rows
        if cur_name is not None:
            if len(cur_rows) != meta["cell_h"]:
                raise ValueError(
                    f"Icon {cur_name} (#{cur_id}) has {len(cur_rows)} rows, expected {meta['cell_h']}"
                )
            icons.append((cur_id, cur_name, cur_cat, cur_rows))
        cur_id, cur_name, cur_rows = None, None, []

    for lineno, raw in enumerate(path.read_text().splitlines(), 1):
        line = raw.strip()
        if not line or line.startswith("//"):
            continue
        if line.startswith("# "):
            flush()
            cur_cat = line[2:].strip()
            continue
        if line.startswith("@"):
            flush()
            k, _, v = line[1:].partition("=")
            k, v = k.strip(), v.strip()
            if k == "var_width":
                meta[k] = v.lower() in ("1", "true", "yes")
            else:
                meta[k] = int(v, 0)
            continue
        if line.startswith("[") and line.endswith("]"):
            flush()
            inner = line[1:-1].strip()
            parts = inner.split(None, 1)
            if len(parts) == 2 and parts[0].isdigit():
                cur_id = int(parts[0])
                cur_name = parts[1].strip()
            else:
                cur_id = meta["base_id"] + len(icons)
                cur_name = inner
            continue
        if all(ch in "#." for ch in line):
            cur_rows.append(line)
            continue
        raise ValueError(f"{path}:{lineno}: unexpected line: {raw!r}")

    flush()
    return meta, icons


def compile_icon_pack(src: Path, dst: Path) -> None:
    meta, icons = parse_icon_catalog(src)
    cell_w, cell_h, stride = meta["cell_w"], meta["cell_h"], meta["stride"]
    base_id = meta["base_id"]
    var_width = meta["var_width"]
    if stride not in (8, 16, 32):
        raise ValueError(f"Record stride must be 8, 16, or 32; got {stride}")

    payload = bytearray()
    for idx, (gid, name, _cat, rows) in enumerate(icons):
        expected_id = base_id + idx
        if gid != expected_id:
            raise ValueError(f"Icon {name} has id {gid}, expected contiguous {expected_id}")
        w = len(rows[0])
        if w > cell_w:
            raise ValueError(f"Icon {name} width {w} exceeds cell_w {cell_w}")
        rec = bytearray(stride)
        packed = pack_rows_msb(rows, w)
        if var_width:
            rec[0] = w
            rec[1 : 1 + len(packed)] = packed
        else:
            rec[0 : len(packed)] = packed
        payload.extend(rec)

    flags = 1 if var_width else 0
    crc = crc16_ccitt(bytes(payload))
    header = struct.pack(
        "<4sHHBBBBHH",
        FILE_ID_ICON_PACK,
        0x0100,
        len(icons),
        cell_w,
        cell_h,
        stride,
        flags,
        base_id,
        crc,
    )
    assert len(header) == 16
    dst.parent.mkdir(parents=True, exist_ok=True)
    dst.write_bytes(header + payload)
    print(f"Packed {len(icons)} icons ({cell_w}x{cell_h}, stride {stride} B) -> {dst} ({len(header)+len(payload)} bytes)")


def pack_xy_offset(xo: int, yo: int) -> int:
    return ((xo & 0x0F) << 4) | (yo & 0x0F)


# Crisp 5px-tall Tiny5-inspired glyph kit covering ASCII + accented Latin-1 + GSM-7 + BBS UI symbols.
TINY5_GLYPHS: dict[int, list[str]] = {
    0x20: [],  # space
    0x21: ["#", "#", "#", ".", "#"],
    0x22: ["#.#", "#.#", "...", "...", "..."],
    0x23: [".#.#", "####", ".#.#", "####", ".#.#"],
    0x24: [".##.", "#...", ".##.", "...#", ".##."],
    0x25: ["#..#", "..#.", ".#..", "#..#", "...."],
    0x26: [".#..", "#.#.", ".##.", "#.#.", ".#.#"],
    0x27: ["#", "#", ".", ".", "."],
    0x28: [".#", "#.", "#.", "#.", ".#"],
    0x29: ["#.", ".#", ".#", ".#", "#."],
    0x2A: [".#.", "###", ".#.", "#.#", "..."],
    0x2B: ["...", ".#.", "###", ".#.", "..."],
    0x2C: ["..", "..", "..", ".#", "#."],
    0x2D: ["...", "...", "###", "...", "..."],
    0x2E: [".", ".", ".", ".", "#"],
    0x2F: ["..#", "..#", ".#.", "#..", "#.."],
    0x30: [".##.", "#..#", "#..#", "#..#", ".##."],
    0x31: [".#.", "##.", ".#.", ".#.", "###"],
    0x32: ["###.", "...#", ".##.", "#...", "####"],
    0x33: ["###.", "...#", ".##.", "...#", "###."],
    0x34: ["#..#", "#..#", "####", "...#", "...#"],
    0x35: ["####", "#...", "###.", "...#", "###."],
    0x36: [".###", "#...", "###.", "#..#", ".##."],
    0x37: ["####", "...#", "..#.", ".#..", ".#.."],
    0x38: [".##.", "#..#", ".##.", "#..#", ".##."],
    0x39: [".##.", "#..#", ".###", "...#", "###."],
    0x3A: [".", "#", ".", "#", "."],
    0x3B: ["..", ".#", "..", ".#", "#."],
    0x3C: ["..#", ".#.", "#..", ".#.", "..#"],
    0x3D: ["...", "###", "...", "###", "..."],
    0x3E: ["#..", ".#.", "..#", ".#.", "#.."],
    0x3F: ["###.", "...#", ".##.", "....", ".#.."],
    0x40: [".##.", "#.##", "#.##", "#...", ".###"],
    0x41: [".##.", "#..#", "####", "#..#", "#..#"],
    0x42: ["###.", "#..#", "###.", "#..#", "###."],
    0x43: [".###", "#...", "#...", "#...", ".###"],
    0x44: ["###.", "#..#", "#..#", "#..#", "###."],
    0x45: ["####", "#...", "###.", "#...", "####"],
    0x46: ["####", "#...", "###.", "#...", "#..."],
    0x47: [".###", "#...", "#.##", "#..#", ".###"],
    0x48: ["#..#", "#..#", "####", "#..#", "#..#"],
    0x49: ["###", ".#.", ".#.", ".#.", "###"],
    0x4A: ["..##", "...#", "...#", "#..#", ".##."],
    0x4B: ["#..#", "#.#.", "##..", "#.#.", "#..#"],
    0x4C: ["#...", "#...", "#...", "#...", "####"],
    0x4D: ["#...#", "##.##", "#.#.#", "#...#", "#...#"],
    0x4E: ["#..#", "##.#", "#.##", "#..#", "#..#"],
    0x4F: [".##.", "#..#", "#..#", "#..#", ".##."],
    0x50: ["###.", "#..#", "###.", "#...", "#..."],
    0x51: [".##.", "#..#", "#..#", "#.##", ".###"],
    0x52: ["###.", "#..#", "###.", "#.#.", "#..#"],
    0x53: [".###", "#...", ".##.", "...#", "###."],
    0x54: ["###", ".#.", ".#.", ".#.", ".#."],
    0x55: ["#..#", "#..#", "#..#", "#..#", ".##."],
    0x56: ["#.#", "#.#", "#.#", "#.#", ".#."],
    0x57: ["#...#", "#...#", "#.#.#", "##.##", "#...#"],
    0x58: ["#..#", "#..#", ".##.", "#..#", "#..#"],
    0x59: ["#.#", "#.#", ".#.", ".#.", ".#."],
    0x5A: ["####", "..#.", ".#..", "#...", "####"],
    0x5B: ["##", "#.", "#.", "#.", "##"],
    0x5C: ["#..", "#..", ".#.", "..#", "..#"],
    0x5D: ["##", ".#", ".#", ".#", "##"],
    0x5E: [".#.", "#.#", "...", "...", "..."],
    0x5F: ["....", "....", "....", "....", "####"],
    0x60: ["#.", ".#", "..", "..", ".."],
    0x61: ["....", ".###", "#..#", "#..#", ".###"],
    0x62: ["#...", "###.", "#..#", "#..#", "###."],
    0x63: ["....", ".###", "#...", "#...", ".###"],
    0x64: ["...#", ".###", "#..#", "#..#", ".###"],
    0x65: ["....", ".##.", "####", "#...", ".###"],
    0x66: [".##", "#..", "###", "#..", "#.."],
    0x67: [".###", "#..#", ".###", "...#", "###."],
    0x68: ["#...", "###.", "#..#", "#..#", "#..#"],
    0x69: ["#", ".", "#", "#", "#"],
    0x6A: [".#", "..", ".#", ".#", "#."],
    0x6B: ["#..", "#.#", "##.", "#.#", "#.#"],
    0x6C: ["#", "#", "#", "#", "#"],
    0x6D: [".....", "##.#.", "#.#.#", "#.#.#", "#...#"],
    0x6E: ["....", "###.", "#..#", "#..#", "#..#"],
    0x6F: ["....", ".##.", "#..#", "#..#", ".##."],
    0x70: ["###.", "#..#", "###.", "#...", "#..."],
    0x71: [".###", "#..#", ".###", "...#", "...#"],
    0x72: ["...", ".##", "#..", "#..", "#.."],
    0x73: ["....", ".###", "##..", "..##", "###."],
    0x74: [".#.", "###", ".#.", ".#.", "..#"],
    0x75: ["....", "#..#", "#..#", "#..#", ".###"],
    0x76: ["...", "#.#", "#.#", "#.#", ".#."],
    0x77: [".....", "#...#", "#.#.#", "##.##", ".#.#."],
    0x78: ["...", "#.#", ".#.", "#.#", "..."],
    0x79: ["#.#", "#.#", ".##", "..#", "##."],
    0x7A: ["...", "###", "..#", "#..", "###"],
    0x7B: [".##", ".#.", "#..", ".#.", ".##"],
    0x7C: ["#", "#", "#", "#", "#"],
    0x7D: ["##.", ".#.", "..#", ".#.", "##."],
    0x7E: ["....", ".#.#", "#.#.", "....", "...."],
    # GSM-7 & European Latin-1 essentials
    0x00A1: ["#", ".", "#", "#", "#"],           # ¡
    0x00A3: [".##.", "#...", "###.", "#...", "####"],  # £
    0x00A5: ["#.#", ".#.", "###", ".#.", "###"],  # ¥
    0x00A7: [".##", "#..", ".#.", "..#", "##."],  # §
    0x00B0: ["##", "##", "..", "..", ".."],       # °
    0x00BF: [".#..", "....", ".##.", "#...", ".###"],  # ¿
    0x00C4: ["#.#", "...", ".#.", "###", "#.#"],  # Ä
    0x00C9: [".#..", "####", "#...", "###.", "####"],  # É
    0x00D1: [".##.", "....", "#..#", "##.#", "#.##"],  # Ñ
    0x00D6: ["#.#", "...", "###", "#.#", "###"],  # Ö
    0x00DC: ["#.#", "...", "#.#", "#.#", "###"],  # Ü
    0x00DF: [".##.", "#..#", "###.", "#..#", "#.#."],  # ß
    0x00E0: ["#...", ".###", "#..#", "#..#", ".###"],  # à
    0x00E1: ["..#.", ".###", "#..#", "#..#", ".###"],  # á
    0x00E4: ["#.#.", ".###", "#..#", "#..#", ".###"],  # ä
    0x00E7: [".###", "#...", ".###", "..#.", ".#.."],  # ç
    0x00E8: ["#...", ".##.", "####", "#...", ".###"],  # è
    0x00E9: ["..#.", ".##.", "####", "#...", ".###"],  # é
    0x00F1: [".##.", "....", "###.", "#..#", "#..#"],  # ñ
    0x00F6: ["#.#.", "....", ".##.", "#..#", ".##."],  # ö
    0x00FC: ["#.#.", "....", "#..#", "#..#", ".###"],  # ü
    # Greek GSM-7 uppercase
    0x0394: ["..#..", ".#.#.", ".#.#.", "#...#", "#####"],  # Δ
    0x0398: [".###.", "#...#", "#####", "#...#", ".###."],  # Θ
    0x03A9: [".###.", "#...#", "#...#", ".#.#.", "##.##"],  # Ω
    0x20AC: [".###", "#...", "###.", "#...", ".###"],       # €
    # BBS & Remote UI Symbols (arrows, checks, box-drawing, bullets)
    0x2190: ["..#.", ".#..", "####", ".#..", "..#."],  # ←
    0x2191: ["..#..", ".###.", "#.#.#", "..#..", "..#.."],  # ↑
    0x2192: [".#..", "..#.", "####", "..#.", ".#.."],  # →
    0x2193: ["..#..", "..#..", "#.#.#", ".###.", "..#.."],  # ↓
    0x2500: ["....", "....", "####", "....", "...."],  # ─
    0x2502: [".#..", ".#..", ".#..", ".#..", ".#.."],  # │
    0x250C: ["....", "....", ".###", ".#..", ".#.."],  # ┌
    0x2510: ["....", "....", "##..", ".#..", ".#.."],  # ┐
    0x2514: [".#..", ".#..", ".###", "....", "...."],  # └
    0x2518: [".#..", ".#..", "##..", "....", "...."],  # ┘
    0x251C: [".#..", ".#..", ".###", ".#..", ".#.."],  # ├
    0x2524: [".#..", ".#..", "##..", ".#..", ".#.."],  # ┤
    0x252C: ["....", "....", "####", ".#..", ".#.."],  # ┬
    0x2534: [".#..", ".#..", "####", "....", "...."],  # ┴
    0x253C: [".#..", ".#..", "####", ".#..", ".#.."],  # ┼
    0x25A0: ["####", "####", "####", "####", "...."],  # ■
    0x25A1: ["####", "#..#", "#..#", "####", "...."],  # □
    0x25B2: ["..#..", ".###.", "#####", ".....", "....."],  # ▲
    0x25B6: ["#..", "##.", "###", "##.", "#.."],       # ▶
    0x25BC: [".....", ".....", "#####", ".###.", "..#.."],  # ▼
    0x25C0: ["..#", ".##", "###", ".##", "..#"],       # ◀
    0x25C6: ["..#..", ".###.", "#####", ".###.", "..#.."],  # ◆
    0x25CF: [".##.", "####", "####", ".##.", "...."],  # ●
    0x2605: ["..#..", "#####", ".###.", "#.#.#", "....."],  # ★
    0x2665: [".#.#.", "#####", "#####", ".###.", "..#.."],  # ♥
    0x2713: ["....#", "...#.", "#.#..", ".#...", "....."],  # ✓
    0x2715: ["#...#", ".#.#.", "..#..", ".#.#.", "#...#"],  # ✕
}


def pack_font_entry(cp: int, bmp_off: int, w: int, h: int, adv: int, xo: int, yo: int) -> bytes:
    """Pack an 8-byte FNT1 v1.1 glyph entry (23-bit offset up to 8 MB, 1..=16 dimensions)."""
    if bmp_off >= (1 << 23):
        raise ValueError(f"Bitmap pool overflowed 23-bit (8 MB) limit at U+{cp:04X}")
    if w == 0 or h == 0:
        hi = 0x80
        dim_wh = 0
        lo = 0
    else:
        if not (1 <= w <= 16 and 1 <= h <= 16):
            raise ValueError(f"Glyph U+{cp:04X} size {w}x{h} outside 1..=16")
        lo = bmp_off & 0xFFFF
        hi = (bmp_off >> 16) & 0x7F
        dim_wh = ((w - 1) << 4) | (h - 1)
    return struct.pack("<HHBBBB", cp, lo, hi, dim_wh, adv, pack_xy_offset(xo, yo))


def build_tiny5_font(dst: Path) -> None:
    entries = bytearray()
    bitmaps = bytearray()
    cps = sorted(TINY5_GLYPHS.keys())
    for cp in cps:
        rows = TINY5_GLYPHS[cp]
        if not rows:
            entries.extend(pack_font_entry(cp, 0, 0, 0, 3, 0, 0))
        else:
            w, h = len(rows[0]), len(rows)
            bmp_off = len(bitmaps)
            bitmaps.extend(pack_rows_msb(rows, w))
            entries.extend(pack_font_entry(cp, bmp_off, w, h, w + 1, 0, 0))

    crc = crc16_ccitt(bytes(entries) + bytes(bitmaps))
    header = struct.pack(
        "<4sHHBBBBHH",
        FILE_ID_FONT_PACK,
        0x0101,
        len(cps),
        6,  # line_height
        5,  # baseline
        3,  # Unicode BMP
        1,  # has_tofu_fallback
        crc,
        0,
    )
    assert len(header) == 16
    dst.parent.mkdir(parents=True, exist_ok=True)
    dst.write_bytes(header + entries + bitmaps)
    print(f"Packed {len(cps)} glyphs -> {dst} ({len(header)+len(entries)+len(bitmaps)} bytes)")


RANGE_PRESETS: dict[str, list[tuple[int, int]]] = {
    "all": [(0x0000, 0xFFFF)],
    "cyrillic": [(0x0400, 0x052F)],
    "kana": [(0x3000, 0x30FF), (0xFF00, 0xFFEF)],
    "ja": [(0x3000, 0x30FF), (0x4E00, 0x9FFF), (0xFF00, 0xFFEF)],
    "zh": [(0x3000, 0x303F), (0x4E00, 0x9FFF), (0xFF00, 0xFFEF)],
    "cjk": [(0x2E80, 0x33FF), (0x4E00, 0x9FFF), (0xAC00, 0xD7AF), (0xF900, 0xFAFF), (0xFF00, 0xFFEF)],
}


def parse_ranges(spec: str) -> list[tuple[int, int]]:
    if spec in RANGE_PRESETS:
        return RANGE_PRESETS[spec]
    out = []
    for part in spec.split(","):
        part = part.strip()
        if not part:
            continue
        if "-" in part:
            lo, hi = part.split("-", 1)
            out.append((int(lo, 0), int(hi, 0)))
        else:
            v = int(part, 0)
            out.append((v, v))
    return out


def compile_bdf_font(src: Path, dst: Path, range_spec: str = "all") -> None:
    """Compile a BDF 2.1 bitmap font (Tiny5, Fusion Pixel 8/10/12px, Ark Pixel, Misaki, Unifont)
    into an FNT1 v1.1 binary pack supporting up to 65,535 BMP glyphs and 8 MB of bitmaps.
    """
    ranges = parse_ranges(range_spec)
    ascent = 7
    descent = 2
    glyphs: dict[int, tuple[int, int, int, int, int, bytes]] = {}
    cur: dict | None = None
    in_bmp = False

    def allowed(cp: int) -> bool:
        return any(lo <= cp <= hi for lo, hi in ranges)

    for raw in src.read_text(encoding="utf-8", errors="replace").splitlines():
        line = raw.strip()
        if line.startswith("FONT_ASCENT "):
            ascent = int(line.split()[1])
        elif line.startswith("FONT_DESCENT "):
            descent = int(line.split()[1])
        elif line.startswith("STARTCHAR"):
            cur = {"cp": -1, "adv": 0, "w": 0, "h": 0, "xo": 0, "yo": 0, "rows": []}
            in_bmp = False
        elif cur is not None:
            if line.startswith("ENCODING "):
                cur["cp"] = int(line.split()[1])
            elif line.startswith("DWIDTH "):
                cur["adv"] = int(line.split()[1])
            elif line.startswith("BBX "):
                w, h, xo, yo = map(int, line.split()[1:5])
                cur["w"], cur["h"], cur["xo"], cur["yo"] = w, h, xo, yo
            elif line == "BITMAP":
                in_bmp = True
            elif line == "ENDCHAR":
                cp = cur["cp"]
                if 0 <= cp <= 0xFFFF and allowed(cp) and cur["w"] <= 16 and cur["h"] <= 16:
                    rb = (cur["w"] + 7) // 8 if cur["w"] > 0 else 0
                    bmp = bytearray()
                    for hex_row in cur["rows"][: cur["h"]]:
                        raw_b = bytes.fromhex(hex_row.ljust(rb * 2, "0")[: rb * 2])
                        bmp.extend(raw_b)
                    xo = max(-8, min(7, cur["xo"]))
                    yo = max(-8, min(7, cur["yo"]))
                    adv = max(0, min(255, cur["adv"]))
                    glyphs[cp] = (cur["w"], cur["h"], adv, xo, yo, bytes(bmp))
                cur = None
                in_bmp = False
            elif in_bmp and line:
                cur["rows"].append(line)

    # Fill in essential BBS box-drawing glyphs (`─│┌┐└┘├┤┬┴┼`) when packing full fonts
    if range_spec == "all":
        for cp in range(0x2500, 0x2540):
            if cp in TINY5_GLYPHS and cp not in glyphs:
                rows = TINY5_GLYPHS[cp]
                w, h = len(rows[0]), len(rows)
                glyphs[cp] = (w, h, w, 0, 0, pack_rows_msb(rows, w))

    cps = sorted(glyphs.keys())
    entries = bytearray()
    bitmaps = bytearray()
    for cp in cps:
        w, h, adv, xo, yo, bmp = glyphs[cp]
        bmp_off = len(bitmaps) if (w > 0 and h > 0) else 0
        if w > 0 and h > 0:
            bitmaps.extend(bmp)
        entries.extend(pack_font_entry(cp, bmp_off, w, h, adv, xo, yo))

    line_height = max(1, min(255, ascent + descent))
    baseline = max(1, min(255, ascent))
    crc = crc16_ccitt(bytes(entries) + bytes(bitmaps))
    header = struct.pack(
        "<4sHHBBBBHH",
        FILE_ID_FONT_PACK,
        0x0101,
        len(cps),
        line_height,
        baseline,
        3,  # Unicode BMP
        1,  # has_tofu_fallback
        crc,
        0,
    )
    assert len(header) == 16
    dst.parent.mkdir(parents=True, exist_ok=True)
    dst.write_bytes(header + entries + bitmaps)
    print(
        f"Packed {len(cps)} BDF glyphs (line_h={line_height}, baseline={baseline}, range={range_spec}) -> "
        f"{dst} ({len(header)+len(entries)+len(bitmaps)} bytes)"
    )


def load_font_advances(fnt_path: Path) -> dict[int, int]:
    """Read `(codepoint -> x_advance)` map from an FNT1 pack for translation pixel-width linting."""
    data = fnt_path.read_bytes()
    if len(data) < 16 or data[:4] != FILE_ID_FONT_PACK:
        raise ValueError(f"Not an FNT1 pack: {fnt_path}")
    _, _ver, count, *_ = struct.unpack("<4sHHBBBBHH", data[:16])
    adv: dict[int, int] = {}
    for i in range(count):
        off = 16 + (i << 3)
        cp, _lo, _hi, _dim, x_adv, _xy = struct.unpack("<HHBBBB", data[off : off + 8])
        adv[cp] = x_adv
    return adv


def measure_text_px(text: str, font_advs: list[dict[int, int]], cjk_fallback_px: int = 9) -> int:
    total = 0
    for ch in text:
        cp = ord(ch)
        w = None
        for table in font_advs:
            if cp in table:
                w = table[cp]
                break
        if w is None:
            w = cjk_fallback_px if cp >= 0x2E80 else 6
        total += w
    return total


def compile_lang_pack(
    src: Path,
    dst: Path,
    lang_code: str | None = None,
    font_hint: int | None = None,
    lint_fonts: list[Path] | None = None,
    max_px: int = 96,
) -> None:
    """Compile a TOML (`[meta]` + `[strings]`, supporting plain strings or `{ full, short }`)
    localization catalog into a binary LNG1 pack with 8-byte sorted FNV-1a index entries.

    Guarantees NFC normalization and zero 32-bit FNV-1a key hash collisions at build time.
    """
    raw_full: dict[str, str] = {}
    raw_short: dict[str, str] = {}

    if src.suffix.lower() == ".toml":
        doc = tomllib.loads(src.read_text(encoding="utf-8"))
        meta = doc.get("meta", {})
        if lang_code is None:
            lang_code = str(meta.get("code", src.stem))
        if font_hint is None:
            font_hint = int(meta.get("font_hint", 1))
        if "name" in meta:
            raw_full["__LANG_NAME__"] = unicodedata.normalize("NFC", str(meta["name"]).strip())
        if "ascii" in meta:
            raw_full["__LANG_ASCII__"] = unicodedata.normalize("NFC", str(meta["ascii"]).strip())
        for k, v in doc.get("strings", {}).items():
            kn = unicodedata.normalize("NFC", str(k).strip())
            if not kn:
                continue
            if isinstance(v, dict):
                full_s = unicodedata.normalize("NFC", str(v.get("full", "")).strip())
                short_s = unicodedata.normalize("NFC", str(v.get("short", "")).strip())
                if full_s:
                    raw_full[kn] = full_s
                if short_s and short_s != full_s:
                    raw_short[kn] = short_s
            else:
                val = unicodedata.normalize("NFC", str(v).strip())
                if val:
                    raw_full[kn] = val
    else:
        if lang_code is None:
            lang_code = src.stem
        if font_hint is None:
            font_hint = 1
        for lineno, raw in enumerate(src.read_text(encoding="utf-8").splitlines(), 1):
            line = raw.strip()
            if not line or line.startswith("#"):
                continue
            if "=" not in line:
                raise ValueError(f"{src}:{lineno}: expected 'English Key = Translation'")
            k, v = line.split("=", 1)
            k = unicodedata.normalize("NFC", k.strip())
            v = unicodedata.normalize("NFC", v.strip())
            if k and v:
                raw_full[k] = v

    code_bytes = lang_code.encode("ascii")[:4].ljust(4, b"\x00")
    hashed: dict[int, tuple[str, bytes]] = {}

    def add_entry(label: str, h: int, val: str) -> None:
        if h in hashed:
            raise ValueError(f"FNV-1a 32-bit hash collision: {hashed[h][0]!r} vs {label!r} (0x{h:08X})")
        vb = val.encode("utf-8")
        if len(vb) > 255:
            raise ValueError(f"Translation for {label!r} exceeds 255 UTF-8 bytes ({len(vb)} B)")
        hashed[h] = (label, vb)

    for k, v in raw_full.items():
        add_entry(k, fnv1a_32(k.encode("utf-8")), v)
    for k, v in raw_short.items():
        add_entry(f"{k} [short]", fnv1a_32_short(k.encode("utf-8")), v)

    entries = bytearray()
    pool = bytearray()
    for h in sorted(hashed.keys()):
        _k, vb = hashed[h]
        off = len(pool)
        if off >= (1 << 24):
            raise ValueError("LNG1 string pool exceeded 24-bit (16 MB) limit")
        pool.extend(vb)
        entries.extend(struct.pack("<IHBB", h, off & 0xFFFF, (off >> 16) & 0xFF, len(vb)))

    flags = (1 if "__LANG_NAME__" in raw_full else 0) | (2 if raw_short else 0)
    crc = crc16_ccitt(bytes(entries) + bytes(pool))
    header = struct.pack(
        "<4sHH4sBBH",
        FILE_ID_LANG_PACK,
        0x0100,
        len(hashed),
        code_bytes,
        font_hint & 0xFF,
        flags,
        crc,
    )
    assert len(header) == 16
    dst.parent.mkdir(parents=True, exist_ok=True)
    dst.write_bytes(header + entries + pool)

    wide_labels = []
    if lint_fonts:
        advs = [load_font_advances(p) for p in lint_fonts if p.exists()]
        for k, v in raw_full.items():
            if k.startswith("__"):
                continue
            px = measure_text_px(v, advs)
            if px > max_px:
                wide_labels.append((k, v, px))

    print(
        f"Packed {len(raw_full)} strings + {len(raw_short)} short variants "
        f"(code={lang_code!r}, font_hint={font_hint}, 0 collisions) -> "
        f"{dst} ({len(header)+len(entries)+len(pool)} bytes)"
    )
    if wide_labels:
        print(f"  [lint] {len(wide_labels)} label(s) > {max_px}px (will auto-marquee when focused):")
        for k, v, px in wide_labels[:5]:
            print(f"    - {k!r} -> {v!r} ({px}px)")


def inspect_pack(path: Path) -> None:
    data = path.read_bytes()
    if len(data) < 16:
        raise ValueError("File smaller than 16-byte table header")
    ident = data[:4]
    if ident == FILE_ID_ICON_PACK:
        _, ver, count, cw, ch, stride, flags, base_id, crc = struct.unpack("<4sHHBBBBHH", data[:16])
        calc = crc16_ccitt(data[16:])
        print(f"ICN1 v{ver>>8}.{ver&0xFF}: {count} icons ({cw}x{ch}, stride={stride} B, base_id={base_id}, flags={flags}, crc=0x{crc:04X} {'OK' if crc==calc else 'MISMATCH'})")
    elif ident == FILE_ID_FONT_PACK:
        _, ver, count, lh, base, enc, flags, crc, _res = struct.unpack("<4sHHBBBBHH", data[:16])
        calc = crc16_ccitt(data[16:])
        bmp_off = 16 + (count << 3)
        print(f"FNT1 v{ver>>8}.{ver&0xFF}: {count} glyphs (line_h={lh}, baseline={base}, enc={enc}, bmp_off={bmp_off}, crc=0x{crc:04X} {'OK' if crc==calc else 'MISMATCH'})")
    elif ident == FILE_ID_LANG_PACK:
        _, ver, count, code_b, fhint, flags, crc = struct.unpack("<4sHH4sBBH", data[:16])
        calc = crc16_ccitt(data[16:])
        code = code_b.rstrip(b"\x00").decode("ascii", errors="replace")
        pool_off = 16 + (count << 3)
        print(f"LNG1 v{ver>>8}.{ver&0xFF}: {count} entries (code={code!r}, font_hint=0x{fhint:02X}, flags={flags}, pool_off={pool_off}, crc=0x{crc:04X} {'OK' if crc==calc else 'MISMATCH'})")
    else:
        raise ValueError(f"Unknown FourCC: {ident!r}")


def main() -> int:
    ap = argparse.ArgumentParser(description="Kern .icn / .fnt / .lng binary pack compiler")
    sub = ap.add_subparsers(dest="cmd", required=True)
    p_ic = sub.add_parser("pack-icons", help="Compile ASCII icon catalog into .icn pack")
    p_ic.add_argument("src", type=Path)
    p_ic.add_argument("dst", type=Path)
    p_fn = sub.add_parser("build-tiny5", help="Build compact Tiny5-extended .fnt pack")
    p_fn.add_argument("dst", type=Path)
    p_bdf = sub.add_parser("pack-bdf", help="Compile BDF 2.1 font (Tiny5, Fusion Pixel CJK, Ark Pixel, Misaki) into .fnt")
    p_bdf.add_argument("src", type=Path)
    p_bdf.add_argument("dst", type=Path)
    p_bdf.add_argument(
        "--range",
        default="all",
        help="Preset (all, cyrillic, kana, ja, zh, cjk) or hex ranges (e.g. 0x3000-0x30FF,0x4E00-0x9FFF)",
    )
    p_lng = sub.add_parser("pack-lang", help="Compile TOML localization catalog into binary .lng pack")
    p_lng.add_argument("src", type=Path)
    p_lng.add_argument("dst", type=Path)
    p_lng.add_argument("--code", default=None, help="Override 2-4 char language code (defaults to [meta].code)")
    p_lng.add_argument("--font-hint", type=int, default=None, help="Override companion font bitmask (1=tiny5, 2=cjk_ja, 4=cjk_zh)")
    p_lng.add_argument("--font", type=Path, action="append", default=[], help="Optional .fnt pack(s) for pixel-width linting")
    p_lng.add_argument("--max-px", type=int, default=96, help="Lint threshold in pixels for menu row labels")
    p_in = sub.add_parser("inspect", help="Inspect and verify CRC of .icn, .fnt, or .lng pack")
    p_in.add_argument("file", type=Path)
    args = ap.parse_args()
    if args.cmd == "pack-icons":
        compile_icon_pack(args.src, args.dst)
    elif args.cmd == "build-tiny5":
        build_tiny5_font(args.dst)
    elif args.cmd == "pack-bdf":
        compile_bdf_font(args.src, args.dst, args.range)
    elif args.cmd == "pack-lang":
        compile_lang_pack(args.src, args.dst, args.code, args.font_hint, args.font, args.max_px)
    elif args.cmd == "inspect":
        inspect_pack(args.file)
    return 0


if __name__ == "__main__":
    sys.exit(main())
