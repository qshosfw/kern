"""Offline demo: 5x7 framebuffer font, demo LCD frames and KernFS seed files."""

from __future__ import annotations

import struct

import rigwire

from .t9 import T9_LOWER, T9_MAX_LEN, T9_NUMBERS, T9_SYMBOLS, T9_UPPER

_DEMO_FONT: dict[str, tuple[int, ...]] = {
    " ": (0, 0, 0, 0, 0), ".": (0, 0, 0x60, 0x60, 0), ":": (0, 0x36, 0x36, 0, 0),
    "-": (0x08, 0x08, 0x08, 0x08, 0x08), "%": (0x23, 0x13, 0x08, 0x64, 0x62),
    "0": (0x3E, 0x51, 0x49, 0x45, 0x3E), "1": (0x00, 0x42, 0x7F, 0x40, 0x00),
    "2": (0x42, 0x61, 0x51, 0x49, 0x46), "3": (0x21, 0x41, 0x45, 0x4B, 0x31),
    "4": (0x18, 0x14, 0x12, 0x7F, 0x10), "5": (0x27, 0x45, 0x45, 0x45, 0x39),
    "6": (0x3C, 0x4A, 0x49, 0x49, 0x30), "7": (0x01, 0x71, 0x09, 0x05, 0x03),
    "8": (0x36, 0x49, 0x49, 0x49, 0x36), "9": (0x06, 0x49, 0x49, 0x29, 0x1E),
    "A": (0x7E, 0x11, 0x11, 0x11, 0x7E), "B": (0x7F, 0x49, 0x49, 0x49, 0x36),
    "C": (0x3E, 0x41, 0x41, 0x41, 0x22), "D": (0x7F, 0x41, 0x41, 0x22, 0x1C),
    "E": (0x7F, 0x49, 0x49, 0x49, 0x41), "F": (0x7F, 0x09, 0x09, 0x09, 0x01),
    "G": (0x3E, 0x41, 0x49, 0x49, 0x7A), "H": (0x7F, 0x08, 0x08, 0x08, 0x7F),
    "I": (0x00, 0x41, 0x7F, 0x41, 0x00), "K": (0x7F, 0x08, 0x14, 0x22, 0x41),
    "L": (0x7F, 0x40, 0x40, 0x40, 0x40), "M": (0x7F, 0x02, 0x0C, 0x02, 0x7F),
    "N": (0x7F, 0x04, 0x08, 0x10, 0x7F), "O": (0x3E, 0x41, 0x41, 0x41, 0x3E),
    "P": (0x7F, 0x09, 0x09, 0x09, 0x06), "R": (0x7F, 0x09, 0x19, 0x29, 0x46),
    "S": (0x46, 0x49, 0x49, 0x49, 0x31), "T": (0x01, 0x01, 0x7F, 0x01, 0x01),
    "U": (0x3F, 0x40, 0x40, 0x40, 0x3F), "V": (0x1F, 0x20, 0x40, 0x20, 0x1F),
    "W": (0x3F, 0x40, 0x38, 0x40, 0x3F), "X": (0x63, 0x14, 0x08, 0x14, 0x63),
    "Y": (0x07, 0x08, 0x70, 0x08, 0x07), "Z": (0x61, 0x51, 0x49, 0x45, 0x43),
    "J": (0x20, 0x40, 0x41, 0x3F, 0x01), "Q": (0x3E, 0x41, 0x51, 0x21, 0x5E),
    "!": (0x00, 0x00, 0x5F, 0x00, 0x00), '"': (0x00, 0x07, 0x00, 0x07, 0x00),
    "#": (0x14, 0x7F, 0x14, 0x7F, 0x14), "$": (0x24, 0x2A, 0x7F, 0x2A, 0x12),
    "&": (0x36, 0x49, 0x56, 0x20, 0x50), "'": (0x00, 0x05, 0x03, 0x00, 0x00),
    "(": (0x00, 0x1C, 0x22, 0x41, 0x00), ")": (0x00, 0x41, 0x22, 0x1C, 0x00),
    "*": (0x08, 0x2A, 0x1C, 0x2A, 0x08), "+": (0x08, 0x08, 0x3E, 0x08, 0x08),
    ",": (0x00, 0x50, 0x30, 0x00, 0x00), "/": (0x20, 0x10, 0x08, 0x04, 0x02),
    ";": (0x00, 0x56, 0x36, 0x00, 0x00), "<": (0x08, 0x14, 0x22, 0x41, 0x00),
    "=": (0x14, 0x14, 0x14, 0x14, 0x14), ">": (0x00, 0x41, 0x22, 0x14, 0x08),
    "?": (0x02, 0x01, 0x51, 0x09, 0x06), "@": (0x32, 0x49, 0x79, 0x41, 0x3E),
    "[": (0x00, 0x7F, 0x41, 0x41, 0x00), "]": (0x00, 0x41, 0x41, 0x7F, 0x00),
    "\\": (0x02, 0x04, 0x08, 0x10, 0x20), "_": (0x40, 0x40, 0x40, 0x40, 0x40),
    "`": (0x00, 0x01, 0x02, 0x04, 0x00), "{": (0x00, 0x08, 0x36, 0x41, 0x00),
    "|": (0x00, 0x00, 0x7F, 0x00, 0x00), "}": (0x00, 0x41, 0x36, 0x08, 0x00),
}


def _fb_draw_text(fb: bytearray, x: int, y: int, text: str, scale: int = 1, invert: bool = False) -> None:
    cx, fg_val = x, not invert
    for ch in text.upper():
        cols = _DEMO_FONT.get(ch, _DEMO_FONT[" "])
        for ci, col in enumerate(cols):
            for row in range(7):
                if (col >> row) & 1:
                    for sy in range(scale):
                        for sx in range(scale):
                            px, py = cx + ci * scale + sx, y + row * scale + sy
                            if 0 <= px < 128 and 0 <= py < 64:
                                idx, bit = (py >> 3) * 128 + px, 1 << (py & 7)
                                if fg_val:
                                    fb[idx] |= bit
                                else:
                                    fb[idx] &= ~bit
        cx += 6 * scale


def _fb_fill(fb: bytearray, x0: int, y0: int, x1: int, y1: int, on: bool = True) -> None:
    for y in range(max(0, y0), min(64, y1)):
        for x in range(max(0, x0), min(128, x1)):
            if on:
                fb[(y >> 3) * 128 + x] |= 1 << (y & 7)
            else:
                fb[(y >> 3) * 128 + x] &= ~(1 << (y & 7))


def make_text_input_framebuffer(text: str, charset: str, pending: bool) -> bytes:
    """Demo mirror of the firmware TextInput screen (title bar, 2x21 field, charset tag, hints)."""
    fb = bytearray(1024)
    _fb_fill(fb, 0, 0, 128, 9)
    _fb_draw_text(fb, 3, 1, "TEXT INPUT", 1, invert=True)
    tag = {T9_UPPER: "CAPS", T9_LOWER: "LOW", T9_NUMBERS: "123", T9_SYMBOLS: "SYM"}.get(charset, "CAPS")
    _fb_draw_text(fb, 126 - 6 * len(tag), 1, tag, 1, invert=True)
    _fb_fill(fb, 0, 13, 128, 14)
    _fb_fill(fb, 0, 40, 128, 41)
    lines = [text[i : i + 21] for i in range(0, max(len(text), 1), 21)][:2] or [""]
    for i, ln in enumerate(lines):
        _fb_draw_text(fb, 1, 18 + i * 11, ln, 1)
    cx = 1 + 6 * (len(lines[-1]) - (1 if pending and lines[-1] else 0))
    cy = 18 + (len(lines) - 1) * 11 + 8
    _fb_fill(fb, cx, cy, cx + 5, cy + 1 + (1 if pending else 0))
    _fb_draw_text(fb, 2, 45, f"{len(text):2d}/{T9_MAX_LEN}", 1)
    _fb_draw_text(fb, 2, 55, "M:OK  EXIT:DEL  *:MODE", 1)
    return bytes(fb)


def make_demo_framebuffer(
    freq_a: str = "433.500",
    freq_b: str = "145.500",
    status: str = "VFO",
    mode: str = "FM",
) -> bytes:
    fb = bytearray(1024)
    for x in range(128):
        for y in range(9):
            fb[(y >> 3) * 128 + x] |= 1 << (y & 7)
    _fb_draw_text(fb, 3, 1, "RIGWIRE", 1, invert=True)
    _fb_draw_text(fb, 60, 1, status[:5], 1, invert=True)
    _fb_draw_text(fb, 104, 1, "92%", 1, invert=True)

    for x in range(2, 18):
        for y in range(13, 23):
            fb[(y >> 3) * 128 + x] |= 1 << (y & 7)
    _fb_draw_text(fb, 4, 15, "V1", 1, invert=True)
    _fb_draw_text(fb, 22, 13, freq_a, 2)
    _fb_draw_text(fb, 128 - 6 * len(mode[:3]), 13, mode[:3], 1)

    for x in range(0, 128, 2):
        fb[(33 >> 3) * 128 + x] |= 1 << (33 & 7)

    for x in range(2, 18):
        fb[(37 >> 3) * 128 + x] |= 1 << (37 & 7)
        fb[(46 >> 3) * 128 + x] |= 1 << (46 & 7)
    for y in range(37, 47):
        fb[(y >> 3) * 128 + 2] |= 1 << (y & 7)
        fb[(y >> 3) * 128 + 17] |= 1 << (y & 7)
    _fb_draw_text(fb, 4, 39, "V2", 1)
    _fb_draw_text(fb, 22, 37, freq_b, 2)
    _fb_draw_text(fb, 112, 37, "FM", 1)

    _fb_draw_text(fb, 2, 55, "S", 1)
    for i in range(12):
        bx = 14 + i * 7
        for y in range(56, 62):
            for x in range(bx, bx + 5):
                if i < 7 or y in (56, 61) or x in (bx, bx + 4):
                    fb[(y >> 3) * 128 + x] |= 1 << (y & 7)
    _fb_draw_text(fb, 102, 55, "RX", 1)
    return bytes(fb)


def make_demo_channels_table() -> bytes:
    """Build a valid /user/channels.ch table (16 B header + 64 B records)."""
    hdr = struct.pack("<4sHHII", b"CHAN", 1, 64, 3, 0)
    recs = bytearray()
    samples = [
        ("PMR 1", 446_006_250, 0),
        ("2M CALL", 145_500_000, 0),
        ("70CM CALL", 433_500_000, 0),
    ]
    for name, rx_hz, mod in samples:
        rec = bytearray(64)
        nm = name.encode("ascii")[:16]
        rec[0 : len(nm)] = nm
        struct.pack_into("<IIBB", rec, 16, rx_hz, rx_hz, mod, 0)
        recs.extend(rec)
    return hdr + bytes(recs)


def make_demo_krx(name: str, payload_size: int = 64) -> bytes:
    """Build a minimal valid KRX v1 container for demo virtual KernFS."""
    name_bytes = name.encode("utf-8") + b"\x00"
    sec1_body = bytearray(40)
    sec2_body = bytearray(28)
    header_sections_len = 4 + len(sec1_body) + 4 + len(sec2_body)
    name_off = 24 + header_sections_len
    struct.pack_into("<HBBB", sec1_body, 0, name_off, 1, 0, 0)
    struct.pack_into("<HHHBBII", sec2_body, 0, 256, 0, 0, 0, 0, 0, payload_size)
    sec_blob = (
        struct.pack("<HH", 0x0001, 4 + len(sec1_body))
        + bytes(sec1_body)
        + struct.pack("<HH", 0x0002, 4 + len(sec2_body))
        + bytes(sec2_body)
        + name_bytes
    )
    pad = (-len(sec_blob)) & 3
    sec_blob += b"\x00" * pad
    header_len = 24 + len(sec_blob)
    payload = b"\x00" * payload_size
    hdr_crc = rigwire.crc32_ieee(sec_blob)
    pay_crc = rigwire.crc32_ieee(payload)
    preamble = struct.pack(
        "<4sBBHHHIII",
        b"\xC0\xDE\x01\xFE",
        0,
        1,
        2,
        header_len,
        0,
        pay_crc,
        hdr_crc,
        0,
    )
    return preamble + sec_blob + payload
