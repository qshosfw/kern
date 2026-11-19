#!/usr/bin/env python3
"""
Kern Dock — Native GTK4 + Libadwaita Hardware Companion for Kern OS.

Dual-pane hardware dock over RigWire (USB-C CDC, UART, or QEMU TCP):
  • Authentic ST7565 128x64 LCD with taller-than-wide rectangular pixels,
    unpixelated backlit glass margin, edge-lit diffuser falloff, and STN
    rear-polarizer pixel shadow
  • Auto-detected hardware handset layout:
      - Quansheng UV-K1 (compact gunmetal bezel, 4x4 flush keypad, cobalt PTT)
      - Quansheng UV-K5 v3 (speaker grille below LCD, M + stacked ▲/▼ + EXIT,
        4x3 numeric block, orange PTT)
  • Low-latency RigWire streaming (chunk-interleaved priority key dispatch,
    zero-sleep frame loop, immediate GLib.idle_add presentation)
  • Live GTK4 Dock Workspace:
      - Device: Battery level bar, hardware identity, FPS/RTT link stats, controls
      - Files:  Interactive KernFS browser + anti-brick shields + WebDAV OS mount
      - Wire:   Live RigWire command & telemetry console
"""

from __future__ import annotations

import argparse
import http.client
import math
import os
import queue
import struct
import subprocess
import sys
import threading
import time
import zlib
from pathlib import Path
from typing import Any, Callable

TOOLS_DIR = Path(__file__).resolve().parent
if str(TOOLS_DIR) not in sys.path:
    sys.path.insert(0, str(TOOLS_DIR))

import rigwire  # noqa: E402
import kernfs_mount  # noqa: E402

# =============================================================================
# 1. REALISTIC ST7565 LCD RASTERIZER (Non-Square Dots + Backlit Glass Margin)
# =============================================================================

LCD_W = 128
LCD_H = 64

# Real ST7565 128x64 COG panels have taller-than-wide pixels (pitch H/W ~1.25-1.33).
# scale -> (dot_w, dot_h, pad_x, pad_y)
LCD_GEOMETRY: dict[int, tuple[int, int, int, int]] = {
    2: (2, 3, 10, 10),   # 256x192 active -> 276x212 glass
    3: (3, 4, 13, 14),   # 384x256 active -> 410x284 glass
    4: (4, 5, 16, 18),   # 512x320 active -> 544x356 glass
}

# theme_name -> (bg_rgb, fg_rgb, led_edge_boost_rgb, glow_css_rgba, dark_mode_lcd)
LCD_THEMES: dict[str, tuple[tuple[int, int, int], tuple[int, int, int], tuple[int, int, int], str, bool]] = {
    "Ice Blue":      ((192, 220, 244), (16, 30, 48),   (22, 24, 18),  "rgba(116, 186, 255, 0.22)", False),
    "Amber":         ((234, 160, 52),  (28, 18, 6),    (22, 20, 10),  "rgba(242, 162, 50, 0.22)",  False),
    "STN Green":     ((160, 190, 130), (22, 36, 18),   (16, 20, 14),  "rgba(150, 190, 118, 0.18)", False),
    "Inverted Blue": ((12, 18, 28),    (138, 214, 255),(8, 14, 22),   "rgba(96, 190, 255, 0.20)",  True),
    "Paper Mono":    ((236, 239, 242), (14, 16, 20),   (12, 12, 12),  "rgba(210, 220, 232, 0.12)", False),
}

# Exact UV-K1 4x4 physical matrix (matches `src/driver/src/keyboard/keys.rs` & K1_VIEW_D_FRONT.png)
K1_NAV_ROW: list[tuple[str, str]] = [
    ("menu", "MENU"), ("up", "<"), ("down", ">"), ("exit", "EXIT"),
]
K1_NUM_ROWS: list[list[tuple[str, str, str, str]]] = [
    [("1", "1", "Band", ".,?"), ("2", "2", "A/B", "abc"), ("3", "3", "V/M", "def"), ("star", "*", "Scan", "↵")],
    [("4", "4", "FC",   "ghi"), ("5", "5", "WX",  "jkl"), ("6", "6", "H/L", "mno"), ("0",    "0", "FM",   "")],
    [("7", "7", "VOX",  "pqrs"),("8", "8", "R",   "tuv"), ("9", "9", "Call","wxyz"),("f",    "#", "LOCK", "F")],
]

# Authentic UV-K5 v3 numeric block (below M + stacked ▲/▼ + EXIT cluster, no flat 4-key nav row)
K5_NUM_ROWS: list[list[tuple[str, str, str, str]]] = [
    [("1", "1", "BAND", ""), ("2", "2", "A/B",  ""), ("3", "3", "V/M",  "")],
    [("4", "4", "FC",   ""), ("5", "5", "NOAA", ""), ("6", "6", "H/M/L","")],
    [("7", "7", "VOX",  ""), ("8", "8", "R",    ""), ("9", "9", "CALL", "")],
    [("star", "*", "SCAN", ""), ("0", "0", "FM", ""), ("f", "F", "LOCK", "")],
]


def _clamp8(v: float) -> int:
    if v <= 0.0:
        return 0
    if v >= 255.0:
        return 255
    return int(v)


def _build_backlight_backdrop(
    total_w: int,
    total_h: int,
    bg: tuple[int, int, int],
    boost: tuple[int, int, int],
    fmt_bgrx: bool,
) -> bytearray:
    """Precomputes the unpixelated illuminated glass with LED edge diffuser + bezel vignette."""
    stride = total_w * (4 if fmt_bgrx else 3)
    buf = bytearray(total_h * stride)
    inv_w = 1.0 / max(1, total_w - 1)
    inv_h = 1.0 / max(1, total_h - 1)
    br, bgc, bb = bg
    er, eg, eb = boost

    for y in range(total_h):
        ny = y * inv_h
        # Edge-lit LED bar along bottom/left diffuser with gentle top/corner falloff
        led_y = 1.0 - 0.55 * (1.0 - ny)
        # Soft recessed bezel shadow around outer 8px perimeter
        edge_y = min(y, total_h - 1 - y)
        vig_y = 0.84 + 0.16 * min(1.0, edge_y / 8.0)
        row_off = y * stride
        for x in range(total_w):
            nx = x * inv_w
            edge_x = min(x, total_w - 1 - x)
            vig_x = 0.86 + 0.14 * min(1.0, edge_x / 8.0)
            vig = vig_x * vig_y
            diff = led_y * (0.94 + 0.06 * math.sin(nx * math.pi))
            r = _clamp8((br + er * diff) * vig)
            g = _clamp8((bgc + eg * diff) * vig)
            b = _clamp8((bb + eb * diff) * vig)
            if fmt_bgrx:
                p = row_off + x * 4
                buf[p] = b
                buf[p + 1] = g
                buf[p + 2] = r
                buf[p + 3] = 0xFF
            else:
                p = row_off + x * 3
                buf[p] = r
                buf[p + 1] = g
                buf[p + 2] = b
    return buf


_BACKDROP_CACHE: dict[tuple[int, str, bool], bytearray] = {}


def _get_backdrop(scale: int, theme_name: str, fmt_bgrx: bool) -> tuple[int, int, bytearray]:
    pw, ph, pad_x, pad_y = LCD_GEOMETRY.get(scale, LCD_GEOMETRY[3])
    total_w = LCD_W * pw + pad_x * 2
    total_h = LCD_H * ph + pad_y * 2
    key = (scale, theme_name, fmt_bgrx)
    base = _BACKDROP_CACHE.get(key)
    if base is None:
        bg, _fg, boost, _glow, _dark = LCD_THEMES.get(theme_name, LCD_THEMES["Ice Blue"])
        base = _build_backlight_backdrop(total_w, total_h, bg, boost, fmt_bgrx)
        _BACKDROP_CACHE[key] = base
    return total_w, total_h, bytearray(base)


def render_st7565(
    fb: bytes,
    *,
    scale: int = 3,
    theme_name: str = "Ice Blue",
    pixel_grid: bool = True,
    fmt_bgrx: bool = True,
) -> tuple[int, int, bytearray]:
    """
    Renders a 1024-byte ST7565 framebuffer with:
      • Unpixelated backlit glass padding on all 4 sides
      • Taller-than-wide rectangular LCD dots (pw < ph)
      • STN rear-polarizer drop shadow (+1,+1) beneath active pixels
      • Optional crisp subpixel gap grid
    """
    if len(fb) < 1024:
        fb = fb.ljust(1024, b"\x00")
    pw, ph, pad_x, pad_y = LCD_GEOMETRY.get(scale, LCD_GEOMETRY[3])
    bg, fg, _boost, _glow, is_dark = LCD_THEMES.get(theme_name, LCD_THEMES["Ice Blue"])
    total_w, total_h, buf = _get_backdrop(scale, theme_name, fmt_bgrx)

    bpp = 4 if fmt_bgrx else 3
    stride = total_w * bpp

    # Active pixel core dimensions inside each (pw x ph) dot cell
    gap = 1 if (pixel_grid and pw >= 3) else 0
    core_w = pw - gap
    core_h = ph - gap

    fr, fg_c, fb_c = fg
    if fmt_bgrx:
        fg_core = bytes((fb_c, fg_c, fr, 0xFF)) * core_w
        # Subtle edge-blended pixel rim for crispness
        fg_soft = bytes((
            (fb_c * 3 + bg[2]) >> 2,
            (fg_c * 3 + bg[1]) >> 2,
            (fr * 3 + bg[0]) >> 2,
            0xFF,
        ))
    else:
        fg_core = bytes((fr, fg_c, fb_c)) * core_w
        fg_soft = bytes((
            (fr * 3 + bg[0]) >> 2,
            (fg_c * 3 + bg[1]) >> 2,
            (fb_c * 3 + bg[2]) >> 2,
        ))

    # Very subtle off-pixel passive LC matrix ghosting inside active region
    if pixel_grid and not is_dark and pw >= 3:
        for y in range(LCD_H):
            gy = pad_y + y * ph + (ph - 1)
            row_off = gy * stride + pad_x * bpp
            end_off = row_off + LCD_W * pw * bpp
            for p in range(row_off, end_off, bpp):
                buf[p] = (buf[p] * 246) >> 8
                buf[p + 1] = (buf[p + 1] * 246) >> 8
                buf[p + 2] = (buf[p + 2] * 246) >> 8

    # Pass 1: STN rear-polarizer drop shadow (+1px, +1px) for reflective light-mode LCDs
    if not is_dark:
        for y in range(LCD_H):
            page_off = (y >> 3) * 128
            bit_mask = 1 << (y & 7)
            y0 = pad_y + y * ph
            for x in range(LCD_W):
                if (fb[page_off + x] & bit_mask) == 0:
                    continue
                x0 = pad_x + x * pw
                # Cast subtle shadow 1px right & 1px down onto rear polarizer
                sy_bot = y0 + core_h
                if sy_bot < total_h:
                    row_off = sy_bot * stride + (x0 + 1) * bpp
                    for dx in range(core_w):
                        p = row_off + dx * bpp
                        buf[p] = (buf[p] * 224) >> 8
                        buf[p + 1] = (buf[p + 1] * 224) >> 8
                        buf[p + 2] = (buf[p + 2] * 224) >> 8
                sx_rt = x0 + core_w
                if sx_rt < total_w:
                    for dy in range(1, core_h):
                        p = (y0 + dy) * stride + sx_rt * bpp
                        buf[p] = (buf[p] * 224) >> 8
                        buf[p + 1] = (buf[p + 1] * 224) >> 8
                        buf[p + 2] = (buf[p + 2] * 224) >> 8

    # Pass 2: Active liquid crystal dots
    core_bytes = core_w * bpp
    for y in range(LCD_H):
        page_off = (y >> 3) * 128
        bit_mask = 1 << (y & 7)
        y0 = pad_y + y * ph
        for x in range(LCD_W):
            if (fb[page_off + x] & bit_mask) == 0:
                continue
            x0 = pad_x + x * pw
            for dy in range(core_h):
                dst = (y0 + dy) * stride + x0 * bpp
                buf[dst : dst + core_bytes] = fg_core
                if gap == 0 and core_w >= 2 and dy == core_h - 1:
                    buf[dst : dst + bpp] = fg_soft

    return total_w, total_h, buf


def render_st7565_rgb(
    fb: bytes,
    scale: int = 3,
    theme_name: str = "Ice Blue",
    pixel_grid: bool = True,
) -> tuple[int, int, bytes]:
    w, h, buf = render_st7565(fb, scale=scale, theme_name=theme_name, pixel_grid=pixel_grid, fmt_bgrx=False)
    return w, h, bytes(buf)


def encode_png_rgb(w: int, h: int, rgb: bytes) -> bytes:
    """Pure-stdlib RGB24 PNG encoder (zero Pillow/PIL dependency)."""
    def chunk(tag: bytes, payload: bytes) -> bytes:
        crc = zlib.crc32(tag + payload) & 0xFFFFFFFF
        return struct.pack(">I", len(payload)) + tag + payload + struct.pack(">I", crc)

    stride = w * 3
    raw = bytearray()
    for y in range(h):
        raw.append(0)
        raw.extend(rgb[y * stride : (y + 1) * stride])

    ihdr = struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0)
    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", ihdr)
        + chunk(b"IDAT", zlib.compress(bytes(raw), level=6))
        + chunk(b"IEND", b"")
    )


# Compact 5x7 font for offline demo preview framebuffer
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
}


def make_demo_framebuffer(freq_a: str = "433.500", freq_b: str = "145.500", status: str = "VFO") -> bytes:
    """Generates a 1024-byte ST7565 preview frame for offline demo mode."""
    fb = bytearray(1024)

    def set_px(x: int, y: int, val: bool = True) -> None:
        if 0 <= x < 128 and 0 <= y < 64:
            idx = (y >> 3) * 128 + x
            bit = 1 << (y & 7)
            if val:
                fb[idx] |= bit
            else:
                fb[idx] &= ~bit

    def draw_text(x: int, y: int, text: str, scale: int = 1, invert: bool = False) -> None:
        cx = x
        fg_val = not invert
        for ch in text.upper():
            cols = _DEMO_FONT.get(ch, _DEMO_FONT[" "])
            for ci, col in enumerate(cols):
                for row in range(7):
                    if (col >> row) & 1:
                        for sy in range(scale):
                            for sx in range(scale):
                                set_px(cx + ci * scale + sx, y + row * scale + sy, fg_val)
            cx += 6 * scale

    for x in range(128):
        for y in range(9):
            set_px(x, y, True)
    draw_text(3, 1, "KERN OS", 1, invert=True)
    draw_text(60, 1, status[:5], 1, invert=True)
    draw_text(104, 1, "92%", 1, invert=True)

    for x in range(2, 18):
        for y in range(13, 23):
            set_px(x, y, True)
    draw_text(4, 15, "V1", 1, invert=True)
    draw_text(22, 13, freq_a, 2)
    draw_text(112, 13, "FM", 1)

    for x in range(0, 128, 2):
        set_px(x, 33, True)

    for x in range(2, 18):
        set_px(x, 37, True)
        set_px(x, 46, True)
    for y in range(37, 47):
        set_px(2, y, True)
        set_px(17, y, True)
    draw_text(4, 39, "V2", 1, invert=False)
    draw_text(22, 37, freq_b, 2)
    draw_text(112, 37, "FM", 1)

    draw_text(2, 55, "S", 1)
    for i in range(12):
        bx = 14 + i * 7
        for y in range(56, 62):
            for x in range(bx, bx + 5):
                if i < 7 or y in (56, 61) or x in (bx, bx + 4):
                    set_px(x, y, True)
    draw_text(102, 55, "RX", 1)
    return bytes(fb)


# =============================================================================
# 2. LOW-LATENCY RIGWIRE ENGINE (Chunk-Interleaved Priority Queue + Shared FS)
# =============================================================================

class DockLink:
    """
    Low-latency serialized RigWire bridge shared by:
      • Display streamer (zero artificial sleep; pushes frames immediately)
      • Priority input queue (drains between 100-byte screencap chunks for <5 ms key latency)
      • Integrated KernFS file browser & WebDAV server
      • Interactive RigWire console
    """

    def __init__(self) -> None:
        self.lock = threading.RLock()
        self.client: rigwire.RigWireClient | None = None
        self.fs_backend: kernfs_mount.FsBackend | None = None
        self.policy = kernfs_mount.SafetyPolicy(read_only=False)
        self.port: str | None = None
        self.baud: int = 115200
        self.volume_name: str = "Quansheng UV-K1"
        self.detected_model: str = "k1"
        self.hello_info: dict[str, Any] = {}
        self.last_telemetry: dict[str, Any] = {}
        self.demo_mode: bool = False
        self.demo_freq_khz: int = 433500
        self.demo_status: str = "VFO"
        self.torch_on: bool = False
        self.keypad_locked: bool = False
        self.last_fb: bytes = make_demo_framebuffer()
        self.fps: float = 0.0
        self.rtt_ms: float = 0.0
        self.error_count: int = 0
        self.connected: bool = False
        self.last_error: str = ""
        self._cmd_q: queue.Queue[tuple[str, Any]] = queue.Queue()
        self._stop = threading.Event()
        self._thread: threading.Thread | None = None
        self.dav_server: kernfs_mount.KernWebDAVServer | None = None
        self.dav_port: int = 8765
        self.dav_open_target: str | None = None
        self.dav_unmount_cmd: list[str] | None = None
        self.on_frame: Callable[[], None] | None = None
        self.on_telemetry: Callable[[], None] | None = None
        self.on_log: Callable[[str], None] | None = None

    def _emit_log(self, line: str) -> None:
        if self.on_log is not None:
            self.on_log(f"[{time.strftime('%H:%M:%S')}] {line}")

    def connect(self, port: str, baud: int = 115200, model_hint: str | None = None) -> None:
        self.disconnect()
        self.baud = baud
        if port.lower() == "demo":
            self.demo_mode = True
            self.port = "demo"
            self.detected_model = model_hint if model_hint in ("k1", "k5") else "k1"
            self.volume_name = "Quansheng UV-K5 v3 DEMO" if self.detected_model == "k5" else "Quansheng UV-K1 DEMO"
            self.hello_info = {
                "minor_version": 1,
                "accepted_max_payload": 128,
                "hardware_profile": 0x00010001,
                "firmware_profile": 0x00010000,
            }
            self.last_telemetry = {
                "serial_str": "DEMO0001K1",
                "mac_addr": "02:4B:45:52:4E:01",
                "mcu_uid": "PY32F071-DEMO",
                "battery_mv": 8040,
                "battery_pct": 92,
                "uptime_ms": 142000,
            }
            mem = kernfs_mount.MemoryFsBackend()
            mem.files["/sys/calib.dat"] = b"\xAA" * 512
            mem.files["/sys/calib.sig"] = b"\xBB" * 32
            mem.files["/apps/dfu.mod"] = b"\xC0\xDE\x01\xFE" + b"\x00" * 188
            mem.files["/apps/sysinfo.krx"] = b"\xC0\xDE\x01\xFE" + b"\x00" * 508
            mem.files["/user/channels.ch"] = b"CHAN" + b"\x00" * 12 + b"\x00" * 128
            mem.files["/user/vfo_a.vfo"] = b"VFO0" + b"\x00" * 156
            mem.files["/pref/display.cfg"] = b"DISP" + b"\x00" * 28
            self.fs_backend = mem
            self.connected = True
            self.last_error = ""
            self.rtt_ms = 0.2
            self.last_fb = make_demo_framebuffer()
            self._emit_log(f"Connected ({self.volume_name}, offline demo)")
            if self.on_frame:
                self.on_frame()
            if self.on_telemetry:
                self.on_telemetry()
            return

        with self.lock:
            cli = rigwire.RigWireClient(port, baudrate=baud, timeout=1.5)
            try:
                hello = cli.hello()
                try:
                    telem = cli.get_system_telemetry()
                except Exception:
                    telem = {}
            except Exception:
                cli.close()
                raise
            self.client = cli
            self.fs_backend = kernfs_mount.RigWireFsBackend(cli, cache_ttl=2.0, shared_lock=self.lock)
            self.port = port
            self.hello_info = hello
            self.last_telemetry = telem
            self.volume_name = kernfs_mount.resolve_volume_name(port, hello=hello, telemetry=telem)
            self.detected_model = model_hint or ("k5" if "k5" in self.volume_name.lower() else "k1")
            self.demo_mode = False
            self.connected = True
            self.last_error = ""
            self.error_count = 0
            self.rtt_ms = cli.last_rtt_ms
        self._emit_log(f"Connected {port} -> {self.volume_name} (MTU {hello.get('accepted_max_payload', 128)} B, RTT {self.rtt_ms:.1f} ms)")
        self._stop.clear()
        self._thread = threading.Thread(target=self._poll_loop, daemon=True)
        self._thread.start()

    def disconnect(self) -> None:
        self.stop_dav()
        self._stop.set()
        if self._thread and self._thread.is_alive():
            self._thread.join(timeout=1.5)
        self._thread = None
        with self.lock:
            if self.client is not None:
                try:
                    self.client.close()
                except Exception:
                    pass
                self.client = None
            self.fs_backend = None
        was_conn = self.connected
        self.connected = False
        self.demo_mode = False
        self.torch_on = False
        self.keypad_locked = False
        self.volume_name = "Quansheng UV-K1"
        self.hello_info = {}
        self.last_telemetry = {}
        self.fps = 0.0
        self.rtt_ms = 0.0
        if was_conn:
            self._emit_log("Disconnected")

    def send_key(self, key_name: str, state_name: str) -> None:
        if self.demo_mode:
            if state_name == "press":
                if key_name == "up":
                    self.demo_freq_khz += 25
                elif key_name == "down":
                    self.demo_freq_khz = max(18000, self.demo_freq_khz - 25)
                self.demo_status = key_name.upper()[:5]
                fa = f"{self.demo_freq_khz // 1000}.{self.demo_freq_khz % 1000:03d}"
                self.last_fb = make_demo_framebuffer(freq_a=fa, status=self.demo_status)
                if self.on_frame:
                    self.on_frame()
            elif state_name == "hold":
                self.demo_status = f"H:{key_name.upper()[:3]}"
                fa = f"{self.demo_freq_khz // 1000}.{self.demo_freq_khz % 1000:03d}"
                self.last_fb = make_demo_framebuffer(freq_a=fa, status=self.demo_status)
                if self.on_frame:
                    self.on_frame()
            return
        code = rigwire.KEY_CODES[key_name]
        state = rigwire.KEY_STATES[state_name]
        self._cmd_q.put(("key", (code, state)))

    def set_flashlight(self, enabled: bool) -> None:
        self.torch_on = enabled
        if self.demo_mode:
            self._emit_log(f"Flashlight -> {'ON' if enabled else 'OFF'}")
            return
        if self.connected:
            self._cmd_q.put(("torch", 1 if enabled else 0))

    def set_keypad_lock(self, locked: bool) -> None:
        self.keypad_locked = locked
        if self.demo_mode:
            self._emit_log(f"Keypad lock -> {'LOCKED' if locked else 'UNLOCKED'}")
            return
        if self.connected:
            self._cmd_q.put(("lock", locked))

    def toggle_vfo(self) -> None:
        if self.demo_mode:
            self._emit_log("Toggled active VFO (A/B)")
            return
        if self.connected:
            self._cmd_q.put(("vfo", None))

    def run_wire_command(self, cmd_line: str) -> str:
        """Executes an interactive RigWire console command and returns formatted output."""
        parts = cmd_line.strip().split()
        if not parts:
            return ""
        op = parts[0].lower()
        if op == "help":
            return "Commands: ping, hello, ls [dir], stat <path>, key <name> [press|hold|release], vfo, freq <MHz>, torch <0|1>, lock <0|1>, clear"
        if not self.connected:
            return "Not connected."

        if op == "ping":
            if self.demo_mode:
                return "PONG (demo, 0.2 ms)"
            with self.lock:
                t0 = time.monotonic()
                assert self.client is not None
                self.client.ping()
                dt = (time.monotonic() - t0) * 1000.0
                self.rtt_ms = dt
                return f"PONG ({dt:.1f} ms)"

        if op == "hello":
            if self.demo_mode:
                return str(self.hello_info)
            with self.lock:
                assert self.client is not None
                self.hello_info = self.client.hello()
                return str(self.hello_info)

        if op == "ls":
            path = parts[1] if len(parts) > 1 else "/"
            assert self.fs_backend is not None
            items = self.fs_backend.ls(path)
            if not items:
                return f"{path}: (empty)"
            return "\n".join(
                f"  {'d' if e['is_dir'] else '-'}  {e['size']:>7} B  {e['name']}"
                for e in items
            )

        if op == "stat" and len(parts) > 1:
            assert self.fs_backend is not None
            st = self.fs_backend.stat(parts[1])
            return str(st)

        if op == "key" and len(parts) > 1:
            kn = parts[1].lower()
            st_name = parts[2].lower() if len(parts) > 2 else "press"
            if kn not in rigwire.KEY_CODES or st_name not in rigwire.KEY_STATES:
                return f"Invalid key/state. Keys: {', '.join(rigwire.KEY_CODES.keys())}"
            self.send_key(kn, st_name)
            return f"Injected key {kn} ({st_name})"

        if op == "vfo":
            self.toggle_vfo()
            return "Toggled active VFO"

        if op == "freq" and len(parts) > 1:
            mhz = float(parts[1])
            hz = int(round(mhz * 1_000_000))
            if self.demo_mode:
                self.demo_freq_khz = hz // 1000
                fa = f"{self.demo_freq_khz // 1000}.{self.demo_freq_khz % 1000:03d}"
                self.last_fb = make_demo_framebuffer(freq_a=fa, status="TUNE")
                if self.on_frame:
                    self.on_frame()
                return f"Tuned demo VFO A to {mhz:.3f} MHz"
            with self.lock:
                assert self.client is not None
                self.client.set_frequency(hz)
                return f"Tuned active VFO to {hz} Hz"

        if op == "torch":
            val = (parts[1] != "0") if len(parts) > 1 else (not self.torch_on)
            self.set_flashlight(val)
            return f"Flashlight {'ON' if val else 'OFF'}"

        if op == "lock":
            val = (parts[1] != "0") if len(parts) > 1 else (not self.keypad_locked)
            self.set_keypad_lock(val)
            return f"Keypad lock {'ON' if val else 'OFF'}"

        return f"Unknown command '{op}'. Type 'help'."

    def start_dav(self, http_port: int = 8765, read_only: bool = False) -> str:
        if self.dav_server is not None:
            return f"http://127.0.0.1:{self.dav_port}/"
        if not self.fs_backend:
            raise RuntimeError("Connect to a radio first")
        self.policy.read_only = read_only
        srv = kernfs_mount.KernWebDAVServer(
            ("127.0.0.1", http_port),
            self.fs_backend,
            self.policy,
            volume_name=self.volume_name,
        )
        self.dav_server = srv
        self.dav_port = srv.server_address[1]
        threading.Thread(target=srv.serve_forever, daemon=True).start()
        self._emit_log(f"WebDAV mounted '{self.volume_name}' at http://127.0.0.1:{self.dav_port}/")
        return f"http://127.0.0.1:{self.dav_port}/"

    def stop_dav(self) -> None:
        if self.dav_unmount_cmd:
            try:
                subprocess.run(self.dav_unmount_cmd, capture_output=True, timeout=5)
            except Exception:
                pass
            self.dav_unmount_cmd = None
        self.dav_open_target = None
        if self.dav_server is not None:
            try:
                self.dav_server.shutdown()
                self.dav_server.server_close()
            except Exception:
                pass
            self.dav_server = None
            self._emit_log("WebDAV unmounted")

    def _drain_priority_cmds(self) -> None:
        """Executes queued key/control packets immediately (even mid-screencap)."""
        while not self._cmd_q.empty():
            try:
                kind, payload = self._cmd_q.get_nowait()
            except queue.Empty:
                break
            if not self.client:
                break
            try:
                if kind == "key":
                    self.client.inject_key(payload[0], payload[1])
                elif kind == "torch":
                    self.client.set_flashlight(payload)
                elif kind == "lock":
                    self.client.set_keypad_lock(payload)
                elif kind == "vfo":
                    self.client.toggle_vfo()
                self.rtt_ms = self.client.last_rtt_ms
            except Exception as e:
                self.error_count += 1
                self.last_error = str(e)

    def _poll_loop(self) -> None:
        frames = 0
        t_fps = time.monotonic()
        last_telem = 0.0
        while not self._stop.is_set() and self.connected:
            try:
                with self.lock:
                    if not self.client:
                        break
                    self._drain_priority_cmds()
                    fb = self.client.screencap(on_between_chunks=self._drain_priority_cmds)
                    self.rtt_ms = self.client.last_rtt_ms
                if len(fb) == 1024:
                    changed = fb != self.last_fb
                    self.last_fb = fb
                    frames += 1
                    self.last_error = ""
                    if changed and self.on_frame is not None:
                        self.on_frame()
            except Exception as e:
                self.error_count += 1
                self.last_error = str(e)
                time.sleep(0.15)
                continue

            now = time.monotonic()
            if now - t_fps >= 1.0:
                self.fps = frames / (now - t_fps)
                frames = 0
                t_fps = now
                if self.on_telemetry is not None:
                    self.on_telemetry()

            if now - last_telem >= 2.0:
                last_telem = now
                try:
                    with self.lock:
                        if self.client:
                            self.last_telemetry = self.client.get_system_telemetry()
                    if self.on_telemetry is not None:
                        self.on_telemetry()
                except Exception:
                    pass
            # Yield minimally so USB/TCP file ops acquire lock cleanly without capping FPS
            time.sleep(0.002)


def list_available_ports() -> list[tuple[str, str]]:
    out: list[tuple[str, str]] = []
    try:
        from serial.tools import list_ports
        for p in list_ports.comports():
            desc = (p.product or p.description or "").strip()
            lbl = f"{p.device} · {desc}" if desc and desc.lower() != "n/a" else p.device
            out.append((p.device, lbl))
    except Exception:
        pass
    out.append(("tcp://127.0.0.1:4446", "QEMU · 127.0.0.1:4446"))
    out.append(("demo", "Demo"))
    return out


# =============================================================================
# 3. CONCISE NATIVE GTK4 + LIBADWAITA CSS (Hardware Surface Only)
# =============================================================================

GTK_CSS = """
.handset-stage {
    padding: 16px 20px 20px 14px;
}

/* --- UV-K1 Compact Gunmetal Bezel (Faithful to K1_VIEW_D_FRONT.png) --- */
.handset-k1 {
    background: linear-gradient(170deg, #43474d 0%, #34383e 45%, #2b2e34 100%);
    border-radius: 18px 18px 22px 22px;
    padding: 12px 14px 14px 14px;
    border: 1px solid rgba(255, 255, 255, 0.12);
    box-shadow:
        0 14px 32px rgba(0, 0, 0, 0.48),
        inset 0 1px 0 rgba(255, 255, 255, 0.16),
        inset 0 -2px 4px rgba(0, 0, 0, 0.40);
}

/* --- UV-K5 v3 Matte Polycarbonate Body --- */
.handset-k5 {
    background: linear-gradient(172deg, #282c32 0%, #1e2126 55%, #171a1e 100%);
    border-radius: 16px 16px 20px 20px;
    padding: 12px 14px 14px 14px;
    border: 1px solid rgba(255, 255, 255, 0.08);
    box-shadow:
        0 14px 32px rgba(0, 0, 0, 0.50),
        inset 0 1px 0 rgba(255, 255, 255, 0.08);
}

/* Top brand strip */
.k1-brand-pill {
    background: #0d0f12;
    border-radius: 6px;
    padding: 3px 14px;
    border: 1px solid rgba(255, 255, 255, 0.07);
}
.brand-text {
    color: #e6ebf2;
    font-weight: 800;
    font-style: italic;
    letter-spacing: 1.5px;
    font-size: 11.5px;
}

/* Status LED */
.radio-led {
    min-width: 9px;
    min-height: 9px;
    border-radius: 99px;
    background: #23272e;
    border: 1px solid rgba(0, 0, 0, 0.7);
}
.radio-led.rx {
    background: #22c55e;
    box-shadow: 0 0 8px rgba(34, 197, 94, 0.75);
}
.radio-led.tx {
    background: #ef4444;
    box-shadow: 0 0 10px rgba(239, 68, 68, 0.85);
}

/* Deep recessed LCD opening */
.lcd-recess {
    background: #0b0d10;
    border-radius: 10px;
    padding: 6px;
    border: 1px solid rgba(0, 0, 0, 0.90);
    box-shadow:
        inset 0 2px 6px rgba(0, 0, 0, 0.85),
        0 1px 0 rgba(255, 255, 255, 0.10);
}

.lcd-glass {
    border-radius: 5px;
}

.k1-subtext {
    color: rgba(255, 255, 255, 0.36);
    font-size: 9.5px;
    font-weight: 600;
    letter-spacing: 0.4px;
}
.mic-cutout {
    min-width: 22px;
    min-height: 4px;
    border-radius: 99px;
    background: #0c0e11;
    box-shadow: inset 0 1px 1px rgba(0, 0, 0, 0.95), 0 1px 0 rgba(255, 255, 255, 0.12);
}

/* UV-K5 speaker grille (positioned between LCD and keypad) */
.k5-grille-slot {
    min-height: 3px;
    border-radius: 99px;
    background: #0b0d10;
    box-shadow: inset 0 1px 1px rgba(0, 0, 0, 0.9), 0 1px 0 rgba(255, 255, 255, 0.06);
}

/* Recessed rubber keypad well */
.keypad-well {
    background: #111317;
    border-radius: 10px;
    padding: 7px;
    border: 1px solid rgba(0, 0, 0, 0.82);
    box-shadow: inset 0 2px 5px rgba(0, 0, 0, 0.8), 0 1px 0 rgba(255, 255, 255, 0.08);
}

/* Physical rubber keys */
button.hw-key {
    background: linear-gradient(180deg, #2b3038 0%, #1e2228 60%, #181b20 100%);
    color: #f1f5f9;
    border-radius: 6px;
    border: 1px solid rgba(255, 255, 255, 0.07);
    padding: 4px 5px;
    min-height: 36px;
    min-width: 58px;
    box-shadow: 0 2px 4px rgba(0, 0, 0, 0.55), inset 0 1px 0 rgba(255, 255, 255, 0.12);
    transition: background 60ms ease, border-color 60ms ease;
}
button.hw-key:hover {
    background: linear-gradient(180deg, #343a44 0%, #252a32 60%, #1d2128 100%);
    border-color: rgba(255, 255, 255, 0.15);
}
button.hw-key:active,
button.hw-key.key-down {
    background: #14171c;
    box-shadow: inset 0 2px 4px rgba(0, 0, 0, 0.85);
    border-color: rgba(96, 165, 250, 0.65);
}

button.hw-key.k5-m {
    background: linear-gradient(180deg, #273e3c 0%, #1a2d2b 100%);
}
button.hw-key.k5-exit {
    background: linear-gradient(180deg, #472828 0%, #301a1a 100%);
}
button.hw-key.k5-arrow {
    min-height: 24px;
    padding: 1px 4px;
}

.k-main {
    font-size: 14px;
    font-weight: 800;
    color: #f4f7fb;
}
.k-nav {
    font-size: 10.5px;
    font-weight: 800;
    letter-spacing: 0.5px;
    color: #f4f7fb;
}
.k-arrow {
    font-size: 14px;
    font-weight: 800;
    color: #f4f7fb;
}
.k-sub1 {
    font-size: 8px;
    font-weight: 700;
    color: rgba(226, 232, 240, 0.78);
}
.k-sub2 {
    font-size: 7.5px;
    color: rgba(168, 179, 194, 0.60);
}

/* Flush left-side physical buttons */
button.flank-ptt {
    background: linear-gradient(90deg, #1b1e24 0%, #282d36 65%, #1d2026 100%);
    color: #e6ebf2;
    border-radius: 7px 1px 1px 7px;
    border: 1px solid rgba(255, 255, 255, 0.08);
    border-right: none;
    min-width: 26px;
    min-height: 92px;
    padding: 3px 2px;
    font-size: 9px;
    font-weight: 800;
    box-shadow: -2px 2px 6px rgba(0, 0, 0, 0.38), inset 2px 0 0 #2d8cff;
}
button.flank-ptt.k5-accent {
    box-shadow: -2px 2px 6px rgba(0, 0, 0, 0.38), inset 2px 0 0 #f97316;
}
button.flank-ptt:active,
button.flank-ptt.key-down {
    background: #991b1b;
    color: #ffffff;
    box-shadow: -2px 2px 8px rgba(239, 68, 68, 0.5), inset 2px 0 0 #fca5a5;
}

button.flank-sk {
    background: linear-gradient(90deg, #1c1f25 0%, #282d35 70%, #1d2026 100%);
    color: rgba(226, 232, 240, 0.80);
    border-radius: 6px 1px 1px 6px;
    border: 1px solid rgba(255, 255, 255, 0.07);
    border-right: none;
    min-width: 24px;
    min-height: 36px;
    padding: 2px;
    font-size: 8.5px;
    font-weight: 700;
}
button.flank-sk:active,
button.flank-sk.key-down {
    background: #13161b;
    box-shadow: inset 2px 0 0 #60a5fa;
}

/* Console monospace view */
textview.wire-console {
    font-family: monospace;
    font-size: 11.5px;
    padding: 10px;
}
"""


# =============================================================================
# 4. NATIVE GTK4 + LIBADWAITA DOCK APPLICATION
# =============================================================================

def launch_gtk4(
    initial_port: str | None = None,
    baud: int = 115200,
    scale: int = 3,
    theme: str | None = None,
    model: str | None = None,
) -> int:
    import gi
    gi.require_version("Gtk", "4.0")
    gi.require_version("Gdk", "4.0")
    from gi.repository import Gdk, Gio, GLib, Gtk
    import cairo

    has_adw = False
    try:
        gi.require_version("Adw", "1")
        from gi.repository import Adw
        has_adw = True
    except Exception:
        Adw = None  # type: ignore[assignment]

    base_app_cls = Adw.Application if has_adw else Gtk.Application
    base_win_cls = Adw.ApplicationWindow if has_adw else Gtk.ApplicationWindow

    class KernDockApp(base_app_cls):  # type: ignore[misc,valid-type]
        def __init__(self) -> None:
            super().__init__(application_id="org.kernos.Dock", flags=Gio.ApplicationFlags.NON_UNIQUE)
            self.link = DockLink()
            self.forced_model = model if model in ("k1", "k5") else None
            self.active_model = self.forced_model or "k1"
            self.user_picked_theme = theme is not None
            self.theme_name = (
                theme if (theme and theme in LCD_THEMES)
                else ("Amber" if self.active_model == "k5" else "Ice Blue")
            )
            self.lcd_scale = scale if scale in LCD_GEOMETRY else 3
            self.pixel_grid = True
            self.mount_read_only = False
            self.current_dir = "/"
            self._hold_timers: dict[str, int] = {}
            self._key_widgets: dict[str, Any] = {}
            self._pressed_keys: set[str] = set()
            self._ports: list[tuple[str, str]] = []
            self._lcd_buf: bytearray | None = None
            self._lcd_surface: Any = None
            self._last_render_key: Any = None
            self._frame_pending = False
            self._glow_provider = Gtk.CssProvider()
            self._updating_switches = False

        def do_activate(self) -> None:
            if has_adw:
                Adw.StyleManager.get_default().set_color_scheme(Adw.ColorScheme.PREFER_DARK)

            display = Gdk.Display.get_default()
            css = Gtk.CssProvider()
            css.load_from_string(GTK_CSS)
            Gtk.StyleContext.add_provider_for_display(display, css, Gtk.STYLE_PROVIDER_PRIORITY_APPLICATION)
            Gtk.StyleContext.add_provider_for_display(display, self._glow_provider, Gtk.STYLE_PROVIDER_PRIORITY_APPLICATION + 1)

            self.win = base_win_cls(application=self)
            self.win.set_title("Kern Dock")
            self.win.set_default_size(940, 620)

            # Bind instant low-latency callbacks from DockLink worker thread -> GTK main thread
            self.link.on_frame = self._schedule_frame_redraw
            self.link.on_telemetry = lambda: GLib.idle_add(self._refresh_telemetry_ui)
            self.link.on_log = lambda line: GLib.idle_add(self._append_console_line, line)

            # --- Single Clean Adw.HeaderBar ---
            header = Adw.HeaderBar() if has_adw else Gtk.HeaderBar()

            # Left: Port selector + Rescan + Connect
            conn_box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=6)
            self.port_strings = Gtk.StringList()
            self.port_dd = Gtk.DropDown(model=self.port_strings)
            self.port_dd.set_size_request(150, -1)
            self.port_dd.set_tooltip_text("Serial port, QEMU TCP, or Demo")
            self._refresh_ports(select_port=initial_port)
            conn_box.append(self.port_dd)

            rescan_btn = Gtk.Button.new_from_icon_name("view-refresh-symbolic")
            rescan_btn.set_tooltip_text("Rescan serial ports")
            rescan_btn.connect("clicked", lambda _b: self._refresh_ports())
            conn_box.append(rescan_btn)

            self.conn_btn = Gtk.Button(label="Connect")
            self.conn_btn.add_css_class("suggested-action")
            self.conn_btn.connect("clicked", self._on_connect_toggle)
            conn_box.append(self.conn_btn)
            header.pack_start(conn_box)

            # Center: Native Adw.ViewSwitcher for the Dock Workspace (Device · Files · Wire)
            self.dock_stack = Adw.ViewStack() if has_adw else Gtk.Stack()
            if has_adw:
                switcher = Adw.ViewSwitcher()
                switcher.set_stack(self.dock_stack)
                switcher.set_policy(Adw.ViewSwitcherPolicy.WIDE)
                header.set_title_widget(switcher)

            # Right: Screenshot + Display Popover + Inspector Pane Toggle
            right_box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=6)

            shot_btn = Gtk.Button.new_from_icon_name("camera-photo-symbolic")
            shot_btn.set_tooltip_text("Save LCD Screenshot (PNG)")
            shot_btn.connect("clicked", self._on_save_screenshot)
            right_box.append(shot_btn)

            disp_btn = Gtk.MenuButton(icon_name="preferences-desktop-display-symbolic")
            disp_btn.set_tooltip_text("Display settings")
            disp_btn.set_popover(self._build_display_popover())
            right_box.append(disp_btn)

            self.pane_toggle = Gtk.ToggleButton(icon_name="sidebar-show-right-symbolic", active=True)
            self.pane_toggle.set_tooltip_text("Show/hide dock workspace")
            right_box.append(self.pane_toggle)
            header.pack_end(right_box)

            # --- Build Left Handset Pane + Right Dock Workspace ---
            self.handset_holder = Gtk.Box(orientation=Gtk.Orientation.VERTICAL)
            self.handset_holder.set_valign(Gtk.Align.CENTER)
            self.handset_holder.set_halign(Gtk.Align.CENTER)
            self.handset_holder.add_css_class("handset-stage")
            self._rebuild_handset_widget()

            # Build Dock Pages
            dev_page = self._build_device_page()
            files_page = self._build_files_page()
            wire_page = self._build_wire_page()

            if has_adw:
                self.dock_stack.add_titled_with_icon(dev_page, "device", "Device", "computer-symbolic")
                self.dock_stack.add_titled_with_icon(files_page, "files", "Files", "folder-symbolic")
                self.dock_stack.add_titled_with_icon(wire_page, "wire", "Wire", "utilities-terminal-symbolic")
            else:
                self.dock_stack.add_titled(dev_page, "device", "Device")
                self.dock_stack.add_titled(files_page, "files", "Files")
                self.dock_stack.add_titled(wire_page, "wire", "Wire")

            self.dock_stack.set_hexpand(True)
            self.dock_stack.set_vexpand(True)

            # Separator + Right Dock Wrapper (collapsible via header toggle)
            self.right_pane = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=0)
            self.right_pane.set_hexpand(True)
            self.right_pane.set_size_request(420, -1)
            self.right_pane.append(Gtk.Separator(orientation=Gtk.Orientation.VERTICAL))
            self.right_pane.append(self.dock_stack)

            self.pane_toggle.connect("toggled", lambda b: self.right_pane.set_visible(b.get_active()))

            main_split = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=0)
            main_split.append(self.handset_holder)
            main_split.append(self.right_pane)

            if has_adw:
                self.toast_overlay = Adw.ToastOverlay()
                self.toast_overlay.set_child(main_split)
                tb = Adw.ToolbarView()
                tb.add_top_bar(header)
                tb.set_content(self.toast_overlay)
                self.win.set_content(tb)
            else:
                self.toast_overlay = None
                outer = Gtk.Box(orientation=Gtk.Orientation.VERTICAL)
                outer.append(header)
                outer.append(main_split)
                self.win.set_child(outer)

            # Keyboard shortcuts (skip when typing inside Wire console entry)
            key_ctrl = Gtk.EventControllerKey()
            key_ctrl.connect("key-pressed", self._on_pc_key_pressed)
            key_ctrl.connect("key-released", self._on_pc_key_released)
            self.win.add_controller(key_ctrl)

            self.win.connect("close-request", self._on_close_request)
            self._apply_lcd_glow()
            self._render_lcd_now()

            if initial_port:
                GLib.timeout_add(80, self._auto_connect_once)

            self.win.present()

        def _notify(self, msg: str) -> None:
            if self.toast_overlay is not None and has_adw:
                t = Adw.Toast.new(msg)
                t.set_timeout(2)
                self.toast_overlay.add_toast(t)

        # --- Display Settings Popover ---
        def _build_display_popover(self) -> Any:
            pop = Gtk.Popover()
            box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=10)
            box.set_margin_top(12)
            box.set_margin_bottom(12)
            box.set_margin_start(14)
            box.set_margin_end(14)
            box.set_size_request(230, -1)

            box.append(Gtk.Label(label="Backlight", xalign=0))
            names = list(LCD_THEMES.keys())
            self.theme_dd = Gtk.DropDown.new_from_strings(names)
            self.theme_dd.set_selected(names.index(self.theme_name))
            self.theme_dd.connect("notify::selected", self._on_theme_changed)
            box.append(self.theme_dd)

            s_row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
            s_row.append(Gtk.Label(label="Scale", xalign=0, hexpand=True))
            s_grp = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL)
            s_grp.add_css_class("linked")
            first = None
            for sc in (2, 3, 4):
                b = Gtk.ToggleButton(label=f"{sc}×")
                if first is None:
                    first = b
                else:
                    b.set_group(first)
                b.set_active(self.lcd_scale == sc)
                b.connect("toggled", lambda btn, v=sc: self._set_scale(v) if btn.get_active() else None)
                s_grp.append(b)
            s_row.append(s_grp)
            box.append(s_row)

            g_row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
            g_row.append(Gtk.Label(label="Pixel Grid", xalign=0, hexpand=True))
            g_sw = Gtk.Switch(active=self.pixel_grid)
            g_sw.connect("notify::active", lambda sw, _p: self._set_grid(sw.get_active()))
            g_row.append(g_sw)
            box.append(g_row)

            pop.set_child(box)
            return pop

        # --- Handset Builder (Auto-Adapts Authentic UV-K1 vs UV-K5 v3 Physical Geometry) ---
        def _rebuild_handset_widget(self) -> None:
            child = self.handset_holder.get_first_child()
            while child is not None:
                nxt = child.get_next_sibling()
                self.handset_holder.remove(child)
                child = nxt
            self._key_widgets.clear()

            is_k5 = self.active_model == "k5"
            row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=0)
            row.set_halign(Gtk.Align.CENTER)

            # Left flank: PTT + SK1 + SK2
            flank = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=6)
            flank.set_valign(Gtk.Align.START)
            flank.set_margin_top(48)
            ptt = Gtk.Button(label="P\nT\nT")
            ptt.set_focusable(False)
            ptt.add_css_class("flank-ptt")
            if is_k5:
                ptt.add_css_class("k5-accent")
            self._attach_key_gestures(ptt, "ptt")
            flank.append(ptt)

            for lbl, kn in (("SK1", "side1"), ("SK2", "side2")):
                sk = Gtk.Button(label=lbl)
                sk.set_focusable(False)
                sk.add_css_class("flank-sk")
                self._attach_key_gestures(sk, kn)
                flank.append(sk)
            row.append(flank)

            # Main body
            body = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=8)
            body.add_css_class("handset-k5" if is_k5 else "handset-k1")

            # Top header strip: LED + crisp Quansheng mark
            top = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
            top.set_valign(Gtk.Align.CENTER)
            self.led_dot = Gtk.Box()
            self.led_dot.set_valign(Gtk.Align.CENTER)
            self.led_dot.set_halign(Gtk.Align.CENTER)
            self.led_dot.add_css_class("radio-led")
            if self.link.connected:
                self.led_dot.add_css_class("rx")
            top.append(self.led_dot)

            brand_box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=5)
            brand_box.set_hexpand(True)
            brand_box.set_halign(Gtk.Align.CENTER)
            if not is_k5:
                brand_box.add_css_class("k1-brand-pill")
            brand_lbl = Gtk.Label(label="QUANSHENG")
            brand_lbl.add_css_class("brand-text")
            brand_box.append(brand_lbl)
            top.append(brand_box)

            # Balance right side visually
            bal = Gtk.Box()
            bal.set_size_request(9, 9)
            top.append(bal)
            body.append(top)

            # Recessed ST7565 LCD Window (taller-than-wide pixels + backlit glass margin)
            recess = Gtk.Box(orientation=Gtk.Orientation.VERTICAL)
            recess.add_css_class("lcd-recess")
            self.lcd_glass = Gtk.Box(orientation=Gtk.Orientation.VERTICAL)
            self.lcd_glass.add_css_class("lcd-glass")
            self.lcd_glass.add_css_class("lcd-glow-active")
            self.lcd_glass.set_overflow(Gtk.Overflow.HIDDEN)

            pw, ph, pad_x, pad_y = LCD_GEOMETRY[self.lcd_scale]
            self.lcd_area = Gtk.DrawingArea()
            self.lcd_area.set_content_width(LCD_W * pw + pad_x * 2)
            self.lcd_area.set_content_height(LCD_H * ph + pad_y * 2)
            self.lcd_area.set_draw_func(self._draw_lcd)
            self.lcd_glass.append(self.lcd_area)
            recess.append(self.lcd_glass)
            body.append(recess)

            if is_k5:
                # UV-K5 v3: Speaker grille BELOW LCD, ABOVE keypad
                grille = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=4)
                grille.set_margin_start(12)
                grille.set_margin_end(12)
                grille.set_margin_top(2)
                grille.set_margin_bottom(2)
                for _ in range(4):
                    s = Gtk.Box()
                    s.add_css_class("k5-grille-slot")
                    grille.append(s)
                body.append(grille)

                # UV-K5 v3 Keypad: M (left) + stacked ▲/▼ (center) + EXIT (right), then 4x3 grid
                well = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=6)
                well.add_css_class("keypad-well")

                nav_k5 = Gtk.Grid(row_spacing=4, column_spacing=6)
                nav_k5.set_column_homogeneous(True)
                btn_m = self._make_simple_key("menu", "M", extra_cls="k5-m")
                btn_up = self._make_simple_key("up", "▲", is_arrow=True, extra_cls="k5-arrow")
                btn_dn = self._make_simple_key("down", "▼", is_arrow=True, extra_cls="k5-arrow")
                btn_ex = self._make_simple_key("exit", "EXIT", extra_cls="k5-exit")
                nav_k5.attach(btn_m,  0, 0, 1, 2)
                nav_k5.attach(btn_up, 1, 0, 1, 1)
                nav_k5.attach(btn_dn, 1, 1, 1, 1)
                nav_k5.attach(btn_ex, 2, 0, 1, 2)
                well.append(nav_k5)

                num_k5 = Gtk.Grid(row_spacing=5, column_spacing=6)
                num_k5.set_column_homogeneous(True)
                for r_i, r_keys in enumerate(K5_NUM_ROWS):
                    for c_i, (kn, p_txt, s1, s2) in enumerate(r_keys):
                        num_k5.attach(self._make_dual_key(kn, p_txt, s1, s2), c_i, r_i, 1, 1)
                well.append(num_k5)
                body.append(well)
            else:
                # UV-K1: Subtle etched legend + mic slot directly below LCD
                etch = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
                etch.set_margin_start(4)
                etch.set_margin_end(4)
                sp = Gtk.Box()
                sp.set_hexpand(True)
                etch.append(sp)
                et_lbl = Gtk.Label(label="UHF/VHF Transceiver")
                et_lbl.add_css_class("k1-subtext")
                etch.append(et_lbl)
                mic = Gtk.Box()
                mic.add_css_class("mic-cutout")
                mic.set_valign(Gtk.Align.CENTER)
                etch.append(mic)
                body.append(etch)

                # UV-K1: Exact 4x4 flush keypad matrix
                well = Gtk.Box(orientation=Gtk.Orientation.VERTICAL)
                well.add_css_class("keypad-well")
                grid = Gtk.Grid(row_spacing=5, column_spacing=5)
                grid.set_column_homogeneous(True)
                for c_i, (kn, lbl) in enumerate(K1_NAV_ROW):
                    grid.attach(self._make_simple_key(kn, lbl, is_arrow=(kn in ("up", "down"))), c_i, 0, 1, 1)
                for r_i, r_keys in enumerate(K1_NUM_ROWS, start=1):
                    for c_i, (kn, p_txt, s1, s2) in enumerate(r_keys):
                        grid.attach(self._make_dual_key(kn, p_txt, s1, s2), c_i, r_i, 1, 1)
                well.append(grid)
                body.append(well)

            row.append(body)
            self.handset_holder.append(row)
            self._last_render_key = None

        def _make_simple_key(self, kname: str, label: str, is_arrow: bool = False, extra_cls: str = "") -> Any:
            btn = Gtk.Button()
            btn.set_focusable(False)
            btn.add_css_class("hw-key")
            if extra_cls:
                btn.add_css_class(extra_cls)
            lbl = Gtk.Label(label=label)
            lbl.add_css_class("k-arrow" if is_arrow else "k-nav")
            btn.set_child(lbl)
            self._attach_key_gestures(btn, kname)
            return btn

        def _make_dual_key(self, kname: str, primary: str, s1: str, s2: str) -> Any:
            btn = Gtk.Button()
            btn.set_focusable(False)
            btn.add_css_class("hw-key")
            box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=4)
            box.set_halign(Gtk.Align.CENTER)
            box.set_valign(Gtk.Align.CENTER)
            p_lbl = Gtk.Label(label=primary)
            p_lbl.add_css_class("k-main")
            box.append(p_lbl)
            if s1 or s2:
                col = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=0)
                col.set_valign(Gtk.Align.CENTER)
                if s1:
                    l1 = Gtk.Label(label=s1, xalign=0)
                    l1.add_css_class("k-sub1")
                    col.append(l1)
                if s2:
                    l2 = Gtk.Label(label=s2, xalign=0)
                    l2.add_css_class("k-sub2")
                    col.append(l2)
                box.append(col)
            btn.set_child(box)
            self._attach_key_gestures(btn, kname)
            return btn

        # --- Dock Page 1: Device Telemetry, Power & Hardware Controls ---
        def _build_device_page(self) -> Any:
            if has_adw:
                page = Adw.PreferencesPage()

                # Power & Battery
                g_pwr = Adw.PreferencesGroup(title="Power")
                self.row_batt = Adw.ActionRow(title="Battery", subtitle="Not connected")
                self.batt_bar = Gtk.LevelBar()
                self.batt_bar.set_min_value(0.0)
                self.batt_bar.set_max_value(100.0)
                self.batt_bar.add_offset_value("low", 20.0)
                self.batt_bar.add_offset_value("high", 50.0)
                self.batt_bar.add_offset_value("full", 85.0)
                self.batt_bar.set_size_request(110, 10)
                self.batt_bar.set_valign(Gtk.Align.CENTER)
                self.row_batt.add_suffix(self.batt_bar)
                g_pwr.add(self.row_batt)
                page.add(g_pwr)

                # Hardware & Link
                g_dev = Adw.PreferencesGroup(title="Radio &amp; Link")
                self.row_ident = Adw.ActionRow(title="Identity", subtitle="—")
                self.row_serial = Adw.ActionRow(title="Serial / MAC", subtitle="—")
                self.row_link = Adw.ActionRow(title="RigWire Link", subtitle="Disconnected")
                g_dev.add(self.row_ident)
                g_dev.add(self.row_serial)
                g_dev.add(self.row_link)
                page.add(g_dev)

                # Hardware Controls
                g_ctl = Adw.PreferencesGroup(title="Controls")
                self.sw_torch = Adw.SwitchRow(title="Flashlight", subtitle="Toggle top LED lamp")
                self.sw_torch.connect("notify::active", self._on_torch_switch)
                g_ctl.add(self.sw_torch)

                self.sw_lock = Adw.SwitchRow(title="Keypad Lock", subtitle="Lock physical front keys")
                self.sw_lock.connect("notify::active", self._on_lock_switch)
                g_ctl.add(self.sw_lock)

                row_vfo = Adw.ActionRow(title="Active VFO", subtitle="Swap primary transmit/receive VFO")
                btn_vfo = Gtk.Button(label="Swap A/B")
                btn_vfo.set_valign(Gtk.Align.CENTER)
                btn_vfo.connect("clicked", lambda _b: self.link.toggle_vfo())
                row_vfo.add_suffix(btn_vfo)
                g_ctl.add(row_vfo)

                row_side = Adw.ActionRow(title="Sideload Package", subtitle="Install .krx / .mod over RigWire")
                btn_side = Gtk.Button(label="Install…")
                btn_side.set_valign(Gtk.Align.CENTER)
                btn_side.connect("clicked", self._on_sideload)
                row_side.add_suffix(btn_side)
                g_ctl.add(row_side)
                page.add(g_ctl)
                return page

            # Minimal GTK4-only fallback if Adw absent
            box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=8)
            box.set_margin_top(12)
            box.set_margin_start(12)
            box.set_margin_end(12)
            self.row_batt = Gtk.Label(label="Battery: —", xalign=0)
            self.row_ident = Gtk.Label(label="Identity: —", xalign=0)
            self.row_serial = Gtk.Label(label="Serial: —", xalign=0)
            self.row_link = Gtk.Label(label="Link: Disconnected", xalign=0)
            self.batt_bar = Gtk.LevelBar()
            self.sw_torch = Gtk.Switch()
            self.sw_lock = Gtk.Switch()
            for w in (self.row_batt, self.batt_bar, self.row_ident, self.row_serial, self.row_link):
                box.append(w)
            return box

        # --- Dock Page 2: Live KernFS File Browser + WebDAV OS Mount ---
        def _build_files_page(self) -> Any:
            box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=8)
            box.set_margin_top(10)
            box.set_margin_bottom(10)
            box.set_margin_start(12)
            box.set_margin_end(12)

            # Top action & quick-dir bar
            bar = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=6)
            dirs_grp = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL)
            dirs_grp.add_css_class("linked")
            for d in ("/", "/user", "/pref", "/apps", "/sys"):
                b = Gtk.Button(label=d if d != "/" else "root")
                b.connect("clicked", lambda _btn, p=d: self._browse_dir(p))
                dirs_grp.append(b)
            bar.append(dirs_grp)

            sp = Gtk.Box()
            sp.set_hexpand(True)
            bar.append(sp)

            btn_ref = Gtk.Button.new_from_icon_name("view-refresh-symbolic")
            btn_ref.set_tooltip_text("Refresh directory")
            btn_ref.connect("clicked", lambda _b: self._browse_dir(self.current_dir))
            bar.append(btn_ref)

            btn_up = Gtk.Button.new_from_icon_name("document-send-symbolic")
            btn_up.set_tooltip_text("Upload file to current directory")
            btn_up.connect("clicked", self._on_upload_file)
            bar.append(btn_up)
            box.append(bar)

            # Mount bar (WebDAV native OS integration)
            mount_bar = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
            self.path_lbl = Gtk.Label(label="/", xalign=0, hexpand=True)
            self.path_lbl.add_css_class("heading")
            mount_bar.append(self.path_lbl)

            self.mount_btn = Gtk.ToggleButton(label="Mount OS Drive")
            self.mount_btn.set_tooltip_text("Mount radio KernFS via hardened localhost WebDAV")
            self.mount_btn.connect("toggled", self._on_mount_toggled)
            mount_bar.append(self.mount_btn)

            self.open_fs_btn = Gtk.Button.new_from_icon_name("folder-open-symbolic")
            self.open_fs_btn.set_tooltip_text("Open mounted drive in system file manager")
            self.open_fs_btn.set_visible(False)
            self.open_fs_btn.connect("clicked", self._on_open_fs)
            mount_bar.append(self.open_fs_btn)
            box.append(mount_bar)

            # File List
            scroll = Gtk.ScrolledWindow()
            scroll.set_vexpand(True)
            self.file_list = Gtk.ListBox()
            self.file_list.set_selection_mode(Gtk.SelectionMode.NONE)
            self.file_list.add_css_class("boxed-list")
            scroll.set_child(self.file_list)
            box.append(scroll)
            return box

        # --- Dock Page 3: Live RigWire Serial & Command Console ---
        def _build_wire_page(self) -> Any:
            box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=8)
            box.set_margin_top(10)
            box.set_margin_bottom(10)
            box.set_margin_start(12)
            box.set_margin_end(12)

            scroll = Gtk.ScrolledWindow()
            scroll.set_vexpand(True)
            self.console_buf = Gtk.TextBuffer()
            self.console_view = Gtk.TextView(buffer=self.console_buf, editable=False, monospace=True)
            self.console_view.set_wrap_mode(Gtk.WrapMode.WORD_CHAR)
            self.console_view.add_css_class("wire-console")
            scroll.set_child(self.console_view)
            box.append(scroll)

            cmd_row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=6)
            self.cmd_entry = Gtk.Entry()
            self.cmd_entry.set_hexpand(True)
            self.cmd_entry.set_placeholder_text("RigWire command (ping, ls /apps, freq 433.500, help)…")
            self.cmd_entry.connect("activate", self._on_console_submit)
            cmd_row.append(self.cmd_entry)

            send_btn = Gtk.Button(label="Run")
            send_btn.connect("clicked", self._on_console_submit)
            cmd_row.append(send_btn)
            box.append(cmd_row)
            return box

        # --- Low-Latency LCD Rendering ---
        def _apply_lcd_glow(self) -> None:
            _, _, _, glow, _ = LCD_THEMES.get(self.theme_name, LCD_THEMES["Ice Blue"])
            self._glow_provider.load_from_string(
                f".lcd-glow-active {{ box-shadow: 0 0 14px {glow}; }}"
            )

        def _schedule_frame_redraw(self) -> None:
            if not self._frame_pending:
                self._frame_pending = True
                GLib.idle_add(self._render_lcd_now)

        def _render_lcd_now(self) -> bool:
            self._frame_pending = False
            rkey = (self.link.last_fb, self.theme_name, self.pixel_grid, self.lcd_scale)
            if rkey != self._last_render_key:
                w, h, buf = render_st7565(
                    self.link.last_fb,
                    scale=self.lcd_scale,
                    theme_name=self.theme_name,
                    pixel_grid=self.pixel_grid,
                    fmt_bgrx=True,
                )
                self._lcd_buf = buf
                self._lcd_surface = cairo.ImageSurface.create_for_data(buf, cairo.FORMAT_RGB24, w, h, w * 4)
                self._last_render_key = rkey
                self.lcd_area.queue_draw()
            return False

        def _draw_lcd(self, _area: Any, cr: Any, w: int, h: int) -> None:
            if self._lcd_surface is None:
                self._render_lcd_now()
            if self._lcd_surface is not None:
                cr.save()
                cr.scale(w / float(self._lcd_surface.get_width()), h / float(self._lcd_surface.get_height()))
                cr.set_source_surface(self._lcd_surface, 0, 0)
                cr.get_source().set_filter(cairo.FILTER_NEAREST)
                cr.paint()
                cr.restore()

            # Subtle matte polarizer top-left sheen
            sheen = cairo.LinearGradient(0, 0, w * 0.55, h * 0.7)
            sheen.add_color_stop_rgba(0.0, 1.0, 1.0, 1.0, 0.05)
            sheen.add_color_stop_rgba(0.45, 1.0, 1.0, 1.0, 0.0)
            cr.set_source(sheen)
            cr.rectangle(0, 0, w, h)
            cr.fill()

        # --- Key Gestures & Instant Keyboard Dispatch ---
        def _attach_key_gestures(self, widget: Any, kname: str) -> None:
            self._key_widgets[kname] = widget
            g_l = Gtk.GestureClick()
            g_l.set_button(1)
            g_l.connect("pressed", lambda _g, _n, _x, _y, k=kname: self._press_key(k))
            g_l.connect("released", lambda _g, _n, _x, _y, k=kname: self._release_key(k))
            g_l.connect("stopped", lambda _g, k=kname: self._release_key(k))
            widget.add_controller(g_l)

            g_r = Gtk.GestureClick()
            g_r.set_button(3)
            g_r.connect("pressed", lambda _g, _n, _x, _y, k=kname: self._long_click_key(k))
            widget.add_controller(g_r)

        def _press_key(self, kname: str) -> None:
            if kname in self._pressed_keys:
                return
            self._pressed_keys.add(kname)
            w = self._key_widgets.get(kname)
            if w is not None:
                w.add_css_class("key-down")
            if kname == "ptt" and self.link.connected:
                self.led_dot.remove_css_class("rx")
                self.led_dot.add_css_class("tx")
            if not self.link.connected:
                return
            self.link.send_key(kname, "press")
            if kname in self._hold_timers:
                GLib.source_remove(self._hold_timers.pop(kname))

            def _hold(k: str = kname) -> bool:
                self._hold_timers.pop(k, None)
                if k in self._pressed_keys and self.link.connected:
                    self.link.send_key(k, "hold")
                return False

            self._hold_timers[kname] = GLib.timeout_add(450, _hold)

        def _release_key(self, kname: str) -> None:
            was = kname in self._pressed_keys
            self._pressed_keys.discard(kname)
            w = self._key_widgets.get(kname)
            if w is not None:
                w.remove_css_class("key-down")
            if kname in self._hold_timers:
                GLib.source_remove(self._hold_timers.pop(kname))
            if kname == "ptt":
                self.led_dot.remove_css_class("tx")
                if self.link.connected:
                    self.led_dot.add_css_class("rx")
            if was and self.link.connected:
                self.link.send_key(kname, "release")

        def _long_click_key(self, kname: str) -> None:
            if not self.link.connected:
                return
            w = self._key_widgets.get(kname)
            if w is not None:
                w.add_css_class("key-down")
            self.link.send_key(kname, "press")
            GLib.timeout_add(120, lambda: (self.link.send_key(kname, "hold"), False)[1] if self.link.connected else False)

            def _end() -> bool:
                if w is not None:
                    w.remove_css_class("key-down")
                if self.link.connected:
                    self.link.send_key(kname, "release")
                return False

            GLib.timeout_add(260, _end)

        _KEYVAL_MAP = {
            Gdk.KEY_Return: "menu", Gdk.KEY_KP_Enter: "menu",
            Gdk.KEY_Escape: "exit", Gdk.KEY_BackSpace: "exit",
            Gdk.KEY_Up: "up", Gdk.KEY_Left: "up",
            Gdk.KEY_Down: "down", Gdk.KEY_Right: "down",
            Gdk.KEY_f: "f", Gdk.KEY_F: "f", Gdk.KEY_numbersign: "f",
            Gdk.KEY_asterisk: "star", Gdk.KEY_KP_Multiply: "star",
            Gdk.KEY_F1: "side1", Gdk.KEY_F2: "side2", Gdk.KEY_space: "ptt",
            **{getattr(Gdk, f"KEY_{d}"): str(d) for d in range(10)},
            **{getattr(Gdk, f"KEY_KP_{d}"): str(d) for d in range(10)},
        }

        def _entry_focused(self) -> bool:
            foc = self.win.get_focus()
            return foc is not None and (foc is self.cmd_entry or isinstance(foc, Gtk.Text))

        def _on_pc_key_pressed(self, _c: Any, keyval: int, _kc: int, _st: Any) -> bool:
            if self._entry_focused():
                return False
            kn = self._KEYVAL_MAP.get(keyval)
            if kn:
                self._press_key(kn)
                return True
            return False

        def _on_pc_key_released(self, _c: Any, keyval: int, _kc: int, _st: Any) -> None:
            if self._entry_focused():
                return
            kn = self._KEYVAL_MAP.get(keyval)
            if kn:
                self._release_key(kn)

        # --- Connection, Display & Telemetry Sync ---
        def _refresh_ports(self, select_port: str | None = None) -> None:
            self._ports = list_available_ports()
            if select_port and all(p[0] != select_port for p in self._ports):
                self._ports.insert(0, (select_port, select_port))
            while self.port_strings.get_n_items() > 0:
                self.port_strings.remove(0)
            for _pid, lbl in self._ports:
                self.port_strings.append(lbl)
            idx = 0
            if select_port:
                for i, (pid, _lbl) in enumerate(self._ports):
                    if pid == select_port:
                        idx = i
                        break
            self.port_dd.set_selected(idx)

        def _selected_port(self) -> str:
            idx = self.port_dd.get_selected()
            return self._ports[idx][0] if 0 <= idx < len(self._ports) else "demo"

        def _auto_connect_once(self) -> bool:
            if not self.link.connected:
                self._on_connect_toggle(self.conn_btn)
            return False

        def _on_connect_toggle(self, _btn: Any) -> None:
            if self.link.connected:
                self.link.disconnect()
                self.conn_btn.set_label("Connect")
                self.conn_btn.remove_css_class("destructive-action")
                self.conn_btn.add_css_class("suggested-action")
                self.mount_btn.set_active(False)
                self.open_fs_btn.set_visible(False)
                self.led_dot.remove_css_class("rx")
                self.led_dot.remove_css_class("tx")
                self.win.set_title("Kern Dock")
                self._clear_file_list()
                self._refresh_telemetry_ui()
                return

            port = self._selected_port()
            try:
                self.link.connect(port, baud=baud, model_hint=self.forced_model)
            except Exception as e:
                self._notify(f"Connection failed: {e}")
                return

            self.conn_btn.set_label("Disconnect")
            self.conn_btn.remove_css_class("suggested-action")
            self.conn_btn.add_css_class("destructive-action")
            self.win.set_title(f"Kern Dock — {self.link.volume_name}")

            # Auto-adapt handset physical geometry to detected radio model (no manual switcher!)
            if self.link.detected_model != self.active_model:
                self.active_model = self.link.detected_model
                if not self.user_picked_theme:
                    self.theme_name = "Amber" if self.active_model == "k5" else "Ice Blue"
                    self.theme_dd.set_selected(list(LCD_THEMES.keys()).index(self.theme_name))
                    self._apply_lcd_glow()
                self._rebuild_handset_widget()
            else:
                self.led_dot.add_css_class("rx")

            self._render_lcd_now()
            self._refresh_telemetry_ui()
            self._browse_dir("/")

        def _refresh_telemetry_ui(self) -> bool:
            if not self.link.connected:
                if has_adw:
                    self.row_batt.set_subtitle("Not connected")
                    self.row_ident.set_subtitle("—")
                    self.row_serial.set_subtitle("—")
                    self.row_link.set_subtitle("Disconnected")
                self.batt_bar.set_value(0.0)
                return False

            t = self.link.last_telemetry
            mv = t.get("battery_mv")
            pct = t.get("battery_pct")
            if mv is not None:
                if has_adw:
                    self.row_batt.set_subtitle(f"{mv / 1000.0:.2f} V · {pct}%")
                self.batt_bar.set_value(float(max(0, min(100, pct or 0))))

            if has_adw:
                self.row_ident.set_subtitle(f"{self.link.volume_name} ({self.link.port})")
                ser = t.get("serial_str", "—")
                mac = t.get("mac_addr", "—")
                self.row_serial.set_subtitle(f"{ser}  ·  {mac}")
                mtu = self.link.hello_info.get("accepted_max_payload", 128)
                fps_s = "Demo" if self.link.demo_mode else f"{self.link.fps:.1f} FPS"
                err_s = f" · {self.link.error_count} err" if self.link.error_count else ""
                self.row_link.set_subtitle(f"{fps_s} · RTT {self.link.rtt_ms:.1f} ms · MTU {mtu} B{err_s}")

            self._updating_switches = True
            self.sw_torch.set_active(self.link.torch_on)
            self.sw_lock.set_active(self.link.keypad_locked)
            self._updating_switches = False
            return False

        def _on_torch_switch(self, sw: Any, _p: Any) -> None:
            if not self._updating_switches:
                self.link.set_flashlight(sw.get_active())

        def _on_lock_switch(self, sw: Any, _p: Any) -> None:
            if not self._updating_switches:
                self.link.set_keypad_lock(sw.get_active())

        def _on_theme_changed(self, dd: Any, _p: Any) -> None:
            names = list(LCD_THEMES.keys())
            idx = dd.get_selected()
            if 0 <= idx < len(names):
                self.theme_name = names[idx]
                self.user_picked_theme = True
                self._apply_lcd_glow()
                self._last_render_key = None
                self._render_lcd_now()

        def _set_scale(self, sc: int) -> None:
            self.lcd_scale = sc
            pw, ph, pad_x, pad_y = LCD_GEOMETRY[sc]
            self.lcd_area.set_content_width(LCD_W * pw + pad_x * 2)
            self.lcd_area.set_content_height(LCD_H * ph + pad_y * 2)
            self._last_render_key = None
            self._render_lcd_now()

        def _set_grid(self, enabled: bool) -> None:
            self.pixel_grid = enabled
            self._last_render_key = None
            self._render_lcd_now()

        # --- Files Browser Operations (Async over Shared RigWire Lock) ---
        def _clear_file_list(self) -> None:
            child = self.file_list.get_first_child()
            while child is not None:
                nxt = child.get_next_sibling()
                self.file_list.remove(child)
                child = nxt

        def _browse_dir(self, path: str) -> None:
            if not self.link.connected or self.link.fs_backend is None:
                return
            self.current_dir = path
            self.path_lbl.set_text(path)
            be = self.link.fs_backend

            def _worker() -> None:
                try:
                    entries = be.ls(path)
                except Exception as e:
                    GLib.idle_add(self._notify, f"List {path} failed: {e}")
                    return
                GLib.idle_add(self._populate_file_list, path, entries)

            threading.Thread(target=_worker, daemon=True).start()

        def _populate_file_list(self, dir_path: str, entries: list[dict[str, Any]]) -> bool:
            self._clear_file_list()
            if not entries:
                lbl = Gtk.Label(label="Empty directory")
                lbl.set_margin_top(18)
                lbl.set_margin_bottom(18)
                lbl.add_css_class("dim-label")
                self.file_list.append(lbl)
                return False

            for e in entries:
                name = e["name"]
                is_dir = bool(e["is_dir"])
                size = int(e["size"])
                full = f"/{name}" if dir_path == "/" else f"{dir_path.rstrip('/')}/{name}"
                protected = full.startswith("/sys/") or full in kernfs_mount.PROTECTED_RECOVERY_MODS

                if has_adw:
                    row = Adw.ActionRow(
                        title=name,
                        subtitle="Directory" if is_dir else f"{size:,} bytes" + (" · Protected" if protected else ""),
                    )
                    icon_name = "folder-symbolic" if is_dir else (
                        "application-x-executable-symbolic" if name.endswith(kernfs_mount.PKG_EXTS) else "text-x-generic-symbolic"
                    )
                    row.add_prefix(Gtk.Image.new_from_icon_name(icon_name))
                    if is_dir:
                        row.set_activatable(True)
                        row.add_suffix(Gtk.Image.new_from_icon_name("go-next-symbolic"))
                        row.connect("activated", lambda _r, p=full: self._browse_dir(p))
                    else:
                        dl_btn = Gtk.Button.new_from_icon_name("document-save-symbolic")
                        dl_btn.set_valign(Gtk.Align.CENTER)
                        dl_btn.add_css_class("flat")
                        dl_btn.set_tooltip_text(f"Download {full}")
                        dl_btn.connect("clicked", lambda _b, p=full, n=name: self._on_download_file(p, n))
                        row.add_suffix(dl_btn)

                        del_btn = Gtk.Button.new_from_icon_name("user-trash-symbolic")
                        del_btn.set_valign(Gtk.Align.CENTER)
                        del_btn.add_css_class("flat")
                        del_btn.set_sensitive(not protected)
                        del_btn.set_tooltip_text("Protected system file" if protected else f"Delete {full}")
                        del_btn.connect("clicked", lambda _b, p=full: self._on_delete_file(p))
                        row.add_suffix(del_btn)
                    self.file_list.append(row)
                else:
                    lbl = Gtk.Label(label=f"{'[DIR] ' if is_dir else ''}{name} ({size} B)", xalign=0)
                    self.file_list.append(lbl)
            return False

        def _on_download_file(self, remote_path: str, filename: str) -> None:
            if not self.link.fs_backend:
                return
            dlg = Gtk.FileDialog()
            dlg.set_title(f"Save {filename}")
            dlg.set_initial_name(filename)
            be = self.link.fs_backend

            def _picked(d: Any, res: Any) -> None:
                try:
                    gf = d.save_finish(res)
                    dest = gf.get_path()
                except GLib.Error:
                    return
                if not dest:
                    return

                def _worker() -> None:
                    try:
                        data = be.read_bytes(remote_path)
                        Path(dest).write_bytes(data)
                        GLib.idle_add(self._notify, f"Saved {Path(dest).name} ({len(data):,} B)")
                    except Exception as e:
                        GLib.idle_add(self._notify, f"Download failed: {e}")

                threading.Thread(target=_worker, daemon=True).start()

            dlg.save(self.win, None, _picked)

        def _on_upload_file(self, _btn: Any) -> None:
            if not self.link.connected or not self.link.fs_backend:
                self._notify("Connect to a radio first")
                return
            dlg = Gtk.FileDialog()
            dlg.set_title(f"Upload file to {self.current_dir}")
            be = self.link.fs_backend
            target_dir = self.current_dir

            def _picked(d: Any, res: Any) -> None:
                try:
                    gf = d.open_finish(res)
                    src = gf.get_path()
                except GLib.Error:
                    return
                if not src:
                    return
                p = Path(src)
                remote = f"/{p.name}" if target_dir == "/" else f"{target_dir.rstrip('/')}/{p.name}"

                def _worker() -> None:
                    try:
                        data = p.read_bytes()
                        self.link.policy.check_write(remote, data)
                        be.write_bytes(remote, data)
                        GLib.idle_add(self._notify, f"Uploaded {remote} ({len(data):,} B)")
                        GLib.idle_add(self._browse_dir, target_dir)
                    except Exception as e:
                        GLib.idle_add(self._notify, f"Upload blocked/failed: {e}")

                threading.Thread(target=_worker, daemon=True).start()

            dlg.open(self.win, None, _picked)

        def _on_delete_file(self, remote_path: str) -> None:
            if not self.link.fs_backend:
                return
            be = self.link.fs_backend

            def _worker() -> None:
                try:
                    self.link.policy.check_delete(remote_path)
                    be.remove(remote_path)
                    GLib.idle_add(self._notify, f"Deleted {remote_path}")
                    GLib.idle_add(self._browse_dir, self.current_dir)
                except Exception as e:
                    GLib.idle_add(self._notify, f"Delete blocked: {e}")

            threading.Thread(target=_worker, daemon=True).start()

        def _on_mount_toggled(self, btn: Any) -> None:
            if not btn.get_active():
                if self.link.dav_server is not None:
                    self.link.stop_dav()
                    self.open_fs_btn.set_visible(False)
                    self._notify("Unmounted radio drive")
                return
            if not self.link.connected:
                btn.set_active(False)
                self._notify("Connect to a radio first")
                return
            try:
                url = self.link.start_dav(http_port=8765, read_only=self.mount_read_only)
                open_target, unmount_cmd = kernfs_mount.try_auto_mount(url, self.link.volume_name)
                self.link.dav_open_target = open_target
                self.link.dav_unmount_cmd = unmount_cmd
                self.open_fs_btn.set_visible(True)
                self._notify(f"Mounted {self.link.volume_name}")
            except Exception as e:
                btn.set_active(False)
                self._notify(f"Mount failed: {e}")

        def _on_open_fs(self, _btn: Any) -> None:
            if self.link.dav_server is None:
                return
            target = self.link.dav_open_target or f"http://127.0.0.1:{self.link.dav_port}/"
            try:
                kernfs_mount.open_in_file_manager(target)
            except Exception as e:
                self._notify(f"Could not open file manager: {e}")

        # --- Screenshot & Sideload ---
        def _on_save_screenshot(self, _btn: Any) -> None:
            dlg = Gtk.FileDialog()
            dlg.set_title("Save ST7565 Screenshot")
            dlg.set_initial_name(f"kern_lcd_{time.strftime('%Y%m%d_%H%M%S')}.png")

            def _done(d: Any, res: Any) -> None:
                try:
                    gf = d.save_finish(res)
                    fpath = gf.get_path()
                    if fpath:
                        w, h, rgb = render_st7565_rgb(
                            self.link.last_fb,
                            scale=4,
                            theme_name=self.theme_name,
                            pixel_grid=self.pixel_grid,
                        )
                        Path(fpath).write_bytes(encode_png_rgb(w, h, rgb))
                        self._notify(f"Saved {Path(fpath).name}")
                except GLib.Error:
                    pass

            dlg.save(self.win, None, _done)

        def _on_sideload(self, _btn: Any) -> None:
            if not self.link.connected or self.link.demo_mode or not self.link.client:
                self._notify("Connect to a live radio (or QEMU) to sideload")
                return
            dlg = Gtk.FileDialog()
            dlg.set_title("Sideload KRX Package")

            def _done(d: Any, res: Any) -> None:
                try:
                    gf = d.open_finish(res)
                    fpath = gf.get_path()
                except GLib.Error:
                    return
                if not fpath:
                    return

                def _worker() -> None:
                    try:
                        with self.link.lock:
                            info = self.link.client.sideload(fpath, verify=True, force=True)  # type: ignore[union-attr]
                        GLib.idle_add(self._notify, f"Installed {info['remote_path']}")
                        GLib.idle_add(self._browse_dir, "/apps")
                    except Exception as e:
                        GLib.idle_add(self._notify, f"Sideload failed: {e}")

                threading.Thread(target=_worker, daemon=True).start()

            dlg.open(self.win, None, _done)

        # --- Wire Console ---
        def _append_console_line(self, line: str) -> bool:
            end_it = self.console_buf.get_end_iter()
            self.console_buf.insert(end_it, line + "\n")
            end_it = self.console_buf.get_end_iter()
            self.console_view.scroll_to_iter(end_it, 0.0, False, 0.0, 1.0)
            return False

        def _on_console_submit(self, _w: Any) -> None:
            cmd = self.cmd_entry.get_text().strip()
            if not cmd:
                return
            self.cmd_entry.set_text("")
            if cmd.lower() == "clear":
                self.console_buf.set_text("")
                return
            self._append_console_line(f"> {cmd}")

            def _worker() -> None:
                try:
                    out = self.link.run_wire_command(cmd)
                except Exception as e:
                    out = f"Error: {e}"
                if out:
                    GLib.idle_add(self._append_console_line, out)

            threading.Thread(target=_worker, daemon=True).start()

        def _on_close_request(self, _win: Any) -> bool:
            self.link.disconnect()
            return False

    app = KernDockApp()
    return int(app.run([]))


# =============================================================================
# 5. HEADLESS SELFTEST (CI-Safe, Zero Display Required)
# =============================================================================

def run_selftest() -> int:
    # 1. Verify both UV-K1 and UV-K5 physical keymaps map 100% to valid RigWire keys
    k1_keys = [k[0] for k in K1_NAV_ROW] + [k[0] for row in K1_NUM_ROWS for k in row]
    k5_keys = ["menu", "up", "down", "exit"] + [k[0] for row in K5_NUM_ROWS for k in row]
    assert len(k1_keys) == 16 and len(k5_keys) == 16
    for kname in set(k1_keys + k5_keys):
        assert kname in rigwire.KEY_CODES, f"Invalid RigWire key '{kname}'"

    # 2. Verify realistic non-square ST7565 renderer (taller-than-wide dots + unpixelated glass margin)
    fb = make_demo_framebuffer("433.500", "145.500", "TEST")
    assert len(fb) == 1024
    for sc, (pw, ph, pad_x, pad_y) in LCD_GEOMETRY.items():
        assert ph > pw, "ST7565 pixels must be taller than wide"
        exp_w = LCD_W * pw + pad_x * 2
        exp_h = LCD_H * ph + pad_y * 2
        w, h, bgrx = render_st7565(fb, scale=sc, theme_name="Ice Blue", pixel_grid=True, fmt_bgrx=True)
        assert w == exp_w and h == exp_h and len(bgrx) == exp_w * exp_h * 4
    w4, h4, rgb4 = render_st7565_rgb(fb, scale=4, theme_name="Amber", pixel_grid=True)
    png = encode_png_rgb(w4, h4, rgb4)
    assert png.startswith(b"\x89PNG\r\n\x1a\n") and png.endswith(b"IEND\xaeB`\x82")

    # 3. Verify DockLink demo mode, FS browser, Wire console, and WebDAV volume identity
    link = DockLink()
    link.connect("demo")
    assert link.connected and link.demo_mode and link.volume_name == "Quansheng UV-K1 DEMO"
    link.send_key("up", "press")
    assert link.demo_freq_khz == 433525
    assert "PONG" in link.run_wire_command("ping")
    assert "dfu.mod" in link.run_wire_command("ls /apps")
    url = link.start_dav(http_port=0, read_only=True)
    assert url.startswith("http://127.0.0.1:")
    conn = http.client.HTTPConnection("127.0.0.1", link.dav_port, timeout=5)
    try:
        conn.request("PROPFIND", "/", headers={"Depth": "1"})
        resp = conn.getresponse()
        assert resp.status == 207 and b"Quansheng UV-K1 DEMO" in resp.read()
    finally:
        conn.close()
    link.disconnect()
    assert not link.connected and link.dav_server is None

    kernfs_mount.log_ok(
        "All Kern Dock headless selftests passed (UV-K1/UV-K5 layouts, non-square ST7565 + backlit margin + shadow, PNG, FS browser, Wire console, WebDAV)."
    )
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description="Kern Dock — Native GTK4 + Libadwaita radio companion")
    ap.add_argument("-p", "--port", help="Serial port (/dev/ttyACM0, COM3, tcp://127.0.0.1:4446, or 'demo')")
    ap.add_argument("-b", "--baud", type=int, default=115200, help="Serial baud rate (default: 115200)")
    ap.add_argument("--model", choices=["k1", "k5"], help="Override auto-detected radio chassis (k1 or k5)")
    ap.add_argument("--scale", type=int, default=3, choices=[2, 3, 4], help="LCD pixel scale (default: 3)")
    ap.add_argument("--theme", choices=list(LCD_THEMES.keys()), help="Override LCD backlight theme")
    ap.add_argument("--demo", action="store_true", help="Launch immediately in offline demo mode")
    ap.add_argument("--selftest", action="store_true", help="Run headless selftest")
    args = ap.parse_args()

    if args.selftest:
        return run_selftest()

    port = "demo" if args.demo else args.port
    kernfs_mount.print_header("Kern Dock")
    kernfs_mount.log_info(f"Launching GTK4 dock (scale={args.scale}x, port={port or 'auto'})")
    try:
        return launch_gtk4(
            initial_port=port,
            baud=args.baud,
            scale=args.scale,
            theme=args.theme,
            model=args.model,
        )
    except ImportError as e:
        kernfs_mount.log_err(f"GTK4 / PyGObject unavailable ({e}). Install python3-gobject + gtk4 + libadwaita.")
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
