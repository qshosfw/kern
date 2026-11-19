#!/usr/bin/env python3
"""
Kern Dock — Modern GTK4 + Libadwaita Radio Companion (Linux / macOS / Windows).

Renders an authentic Quansheng UV-K1 or UV-K5 v3 handheld radio chassis with:
  • Crisp 128x64 ST7565 dot-matrix LCD (Cairo hardware-backed rendering, 5
    backlight themes, optional STN pixel grid, realistic glass bezel & glow)
  • Switchable UV-K1 (gunmetal metallic faceplate, cobalt-blue knob/PTT accent,
    MENU / < / > / EXIT) and UV-K5 v3 (matte black chassis, speaker louvers,
    tactical orange knob accent, M / ▲ / ▼ / EXIT) hardware skins
  • Authentic 4x4 physical keypad matrix + protruding PTT / SK1 / SK2 flank
    matching `src/driver/src/keyboard/keys.rs` (press/hold/release, right-click
    long-press, PC keyboard shortcuts with live button depression animation)
  • Clickable top LED flashlight dome, live telemetry strip, pure-stdlib PNG
    screenshot export, KRX package sideloading, and integrated hardened KernFS
    WebDAV mounting ("Quansheng UV-K1 XXXX") over the shared RigWire link.

Works over:
  • Real radio USB-C CDC / UART serial (/dev/ttyACM0, COM3, /dev/cu.usbmodem*)
  • QEMU hardware emulator (tcp://127.0.0.1:4446)
  • Built-in offline preview mode (--demo)
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
from typing import Any

TOOLS_DIR = Path(__file__).resolve().parent
if str(TOOLS_DIR) not in sys.path:
    sys.path.insert(0, str(TOOLS_DIR))

import rigwire  # noqa: E402
import kernfs_mount  # noqa: E402

# =============================================================================
# 1. ST7565 128x64 LCD RASTERIZER, THEMES, CHASSIS PROFILES & PNG ENCODER
# =============================================================================

LCD_W = 128
LCD_H = 64

# theme_name -> (bg_rgb, fg_rgb, grid_rgb, glow_css_rgba)
LCD_THEMES: dict[str, tuple[tuple[int, int, int], tuple[int, int, int], tuple[int, int, int], str]] = {
    "Ice Blue (UV-K1)": ((198, 226, 248), (15, 30, 48),   (180, 210, 234), "rgba(110, 185, 255, 0.30)"),
    "Amber (UV-K5)":    ((240, 166, 56),  (28, 18, 6),    (222, 148, 42),  "rgba(245, 166, 52, 0.30)"),
    "STN Green":        ((164, 196, 134), (22, 38, 18),   (148, 180, 120), "rgba(154, 198, 120, 0.24)"),
    "Inverted OLED":    ((12, 17, 24),    (118, 222, 255),(18, 25, 36),    "rgba(88, 200, 255, 0.26)"),
    "Pure Mono":        ((244, 246, 248), (10, 12, 16),   (232, 236, 240), "rgba(220, 230, 245, 0.16)"),
}

# Exact 4x4 physical matrix matching `src/driver/src/keyboard/keys.rs` & K1 hardware photos:
# Each entry: (rigwire_key_name, primary_label, sub_top, sub_bot)
KEYPAD_ROWS_NUMERIC: list[list[tuple[str, str, str, str]]] = [
    [("1", "1", "Band", ".,?"), ("2", "2", "A/B", "abc"), ("3", "3", "V/M", "def"), ("star", "*", "Scan", "↵")],
    [("4", "4", "FC",   "ghi"), ("5", "5", "WX",  "jkl"), ("6", "6", "H/L", "mno"), ("0",    "0", "FM",   "")],
    [("7", "7", "VOX",  "pqrs"),("8", "8", "R",   "tuv"), ("9", "9", "Call","wxyz"),("f",    "#", "LOCK", "F")],
]

CHASSIS_PROFILES: dict[str, dict[str, Any]] = {
    "k1": {
        "name": "UV-K1",
        "title": "Quansheng UV-K1",
        "default_theme": "Ice Blue (UV-K1)",
        "accent_rgb": (0.176, 0.549, 1.000),  # UV-K1 signature cobalt blue (#2D8CFF)
        "nav_row": [
            ("menu", "MENU", "", ""),
            ("up",   "<",    "", ""),
            ("down", ">",    "", ""),
            ("exit", "EXIT", "", ""),
        ],
    },
    "k5": {
        "name": "UV-K5 v3",
        "title": "Quansheng UV-K5 v3",
        "default_theme": "Amber (UV-K5)",
        "accent_rgb": (0.976, 0.451, 0.086),  # UV-K5 signature tactical orange (#F97316)
        "nav_row": [
            ("menu", "M",    "", ""),
            ("up",   "▲",    "", ""),
            ("down", "▼",    "", ""),
            ("exit", "EXIT", "", ""),
        ],
    },
}


def render_st7565_rgb(
    fb: bytes,
    scale: int = 3,
    theme_name: str = "Ice Blue (UV-K1)",
    pixel_grid: bool = True,
) -> tuple[int, int, bytes]:
    """Converts a 1024-byte ST7565 vertical-page framebuffer into scaled RGB24 bytes."""
    if len(fb) < 1024:
        fb = fb.ljust(1024, b"\x00")
    bg, fg, grid, _ = LCD_THEMES.get(theme_name, LCD_THEMES["Ice Blue (UV-K1)"])
    bg_b = bytes(bg)
    fg_b = bytes(fg)
    grid_b = bytes(grid)
    fg_edge_b = bytes((
        (fg[0] * 3 + bg[0]) >> 2,
        (fg[1] * 3 + bg[1]) >> 2,
        (fg[2] * 3 + bg[2]) >> 2,
    ))

    out_w = LCD_W * scale
    out_h = LCD_H * scale
    use_grid = pixel_grid and scale >= 3 and grid != bg

    out = bytearray(out_w * out_h * 3)
    row_stride = out_w * 3

    on_cell = (fg_b * (scale - 1) + fg_edge_b) if use_grid else (fg_b * scale)
    off_cell = (bg_b * (scale - 1) + grid_b) if use_grid else (bg_b * scale)
    on_grid = fg_edge_b * scale
    off_grid = grid_b * scale
    cell_bytes = scale * 3

    for y in range(LCD_H):
        page_off = (y >> 3) * 128
        bit_mask = 1 << (y & 7)
        line_main = bytearray(row_stride)
        line_grid = bytearray(row_stride) if use_grid else None

        pos = 0
        for x in range(LCD_W):
            on = (fb[page_off + x] & bit_mask) != 0
            line_main[pos : pos + cell_bytes] = on_cell if on else off_cell
            if line_grid is not None:
                line_grid[pos : pos + cell_bytes] = on_grid if on else off_grid
            pos += cell_bytes

        y0 = y * scale
        for sy in range(scale):
            dst = (y0 + sy) * row_stride
            if line_grid is not None and sy == scale - 1:
                out[dst : dst + row_stride] = line_grid
            else:
                out[dst : dst + row_stride] = line_main

    return out_w, out_h, bytes(out)


def render_st7565_bgra(
    fb: bytes,
    scale: int = 3,
    theme_name: str = "Ice Blue (UV-K1)",
    pixel_grid: bool = True,
) -> tuple[int, int, bytearray]:
    """Renders 1024-byte ST7565 framebuffer directly into Cairo FORMAT_RGB24 (BGRX) bytes."""
    if len(fb) < 1024:
        fb = fb.ljust(1024, b"\x00")
    bg, fg, grid, _ = LCD_THEMES.get(theme_name, LCD_THEMES["Ice Blue (UV-K1)"])

    def _px(c: tuple[int, int, int]) -> bytes:
        return bytes((c[2], c[1], c[0], 0xFF))

    bg_p = _px(bg)
    fg_p = _px(fg)
    grid_p = _px(grid)
    fg_edge_p = _px((
        (fg[0] * 3 + bg[0]) >> 2,
        (fg[1] * 3 + bg[1]) >> 2,
        (fg[2] * 3 + bg[2]) >> 2,
    ))

    out_w = LCD_W * scale
    out_h = LCD_H * scale
    use_grid = pixel_grid and scale >= 3 and grid != bg

    out = bytearray(out_w * out_h * 4)
    row_stride = out_w * 4
    cell_bytes = scale * 4

    on_cell = (fg_p * (scale - 1) + fg_edge_p) if use_grid else (fg_p * scale)
    off_cell = (bg_p * (scale - 1) + grid_p) if use_grid else (bg_p * scale)
    on_grid = fg_edge_p * scale
    off_grid = grid_p * scale

    for y in range(LCD_H):
        page_off = (y >> 3) * 128
        bit_mask = 1 << (y & 7)
        line_main = bytearray(row_stride)
        line_grid = bytearray(row_stride) if use_grid else None

        pos = 0
        for x in range(LCD_W):
            on = (fb[page_off + x] & bit_mask) != 0
            line_main[pos : pos + cell_bytes] = on_cell if on else off_cell
            if line_grid is not None:
                line_grid[pos : pos + cell_bytes] = on_grid if on else off_grid
            pos += cell_bytes

        y0 = y * scale
        for sy in range(scale):
            dst = (y0 + sy) * row_stride
            if line_grid is not None and sy == scale - 1:
                out[dst : dst + row_stride] = line_grid
            else:
                out[dst : dst + row_stride] = line_main

    return out_w, out_h, out


def rgb_to_ppm(w: int, h: int, rgb: bytes) -> bytes:
    """Wraps RGB24 bytes into a binary P6 PPM image."""
    return f"P6\n{w} {h}\n255\n".encode("ascii") + rgb


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


# Compact 5x7 bitmap font for offline --demo mode framebuffer generation
_DEMO_FONT: dict[str, tuple[int, ...]] = {
    " ": (0, 0, 0, 0, 0),
    ".": (0, 0, 0x60, 0x60, 0),
    ":": (0, 0x36, 0x36, 0, 0),
    "-": (0x08, 0x08, 0x08, 0x08, 0x08),
    "%": (0x23, 0x13, 0x08, 0x64, 0x62),
    "0": (0x3E, 0x51, 0x49, 0x45, 0x3E),
    "1": (0x00, 0x42, 0x7F, 0x40, 0x00),
    "2": (0x42, 0x61, 0x51, 0x49, 0x46),
    "3": (0x21, 0x41, 0x45, 0x4B, 0x31),
    "4": (0x18, 0x14, 0x12, 0x7F, 0x10),
    "5": (0x27, 0x45, 0x45, 0x45, 0x39),
    "6": (0x3C, 0x4A, 0x49, 0x49, 0x30),
    "7": (0x01, 0x71, 0x09, 0x05, 0x03),
    "8": (0x36, 0x49, 0x49, 0x49, 0x36),
    "9": (0x06, 0x49, 0x49, 0x29, 0x1E),
    "A": (0x7E, 0x11, 0x11, 0x11, 0x7E),
    "B": (0x7F, 0x49, 0x49, 0x49, 0x36),
    "C": (0x3E, 0x41, 0x41, 0x41, 0x22),
    "D": (0x7F, 0x41, 0x41, 0x22, 0x1C),
    "E": (0x7F, 0x49, 0x49, 0x49, 0x41),
    "F": (0x7F, 0x09, 0x09, 0x09, 0x01),
    "G": (0x3E, 0x41, 0x49, 0x49, 0x7A),
    "H": (0x7F, 0x08, 0x08, 0x08, 0x7F),
    "I": (0x00, 0x41, 0x7F, 0x41, 0x00),
    "K": (0x7F, 0x08, 0x14, 0x22, 0x41),
    "L": (0x7F, 0x40, 0x40, 0x40, 0x40),
    "M": (0x7F, 0x02, 0x0C, 0x02, 0x7F),
    "N": (0x7F, 0x04, 0x08, 0x10, 0x7F),
    "O": (0x3E, 0x41, 0x41, 0x41, 0x3E),
    "P": (0x7F, 0x09, 0x09, 0x09, 0x06),
    "R": (0x7F, 0x09, 0x19, 0x29, 0x46),
    "S": (0x46, 0x49, 0x49, 0x49, 0x31),
    "T": (0x01, 0x01, 0x7F, 0x01, 0x01),
    "U": (0x3F, 0x40, 0x40, 0x40, 0x3F),
    "V": (0x1F, 0x20, 0x40, 0x20, 0x1F),
    "W": (0x3F, 0x40, 0x38, 0x40, 0x3F),
    "X": (0x63, 0x14, 0x08, 0x14, 0x63),
    "Y": (0x07, 0x08, 0x70, 0x08, 0x07),
    "Z": (0x61, 0x51, 0x49, 0x45, 0x43),
}


def make_demo_framebuffer(freq_a: str = "433.500", freq_b: str = "145.500", status: str = "DEMO") -> bytes:
    """Generates a realistic 1024-byte ST7565 preview frame when no radio is connected."""
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
    draw_text(58, 1, status[:6], 1, invert=True)
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
# 2. SERIALIZED RIGWIRE WORKER (Non-Blocking Poller + Priority Key Queue)
# =============================================================================

class DockLink:
    """Thread-safe RigWire bridge shared by the GUI poller, key injector, and WebDAV mounter."""

    def __init__(self) -> None:
        self.lock = threading.RLock()
        self.client: rigwire.RigWireClient | None = None
        self.port: str | None = None
        self.volume_name: str = "Quansheng UV-K1"
        self.detected_model: str = "k1"
        self.demo_mode: bool = False
        self.demo_freq_khz: int = 433500
        self.demo_status: str = "DEMO"
        self.torch_on: bool = False
        self.last_fb: bytes = make_demo_framebuffer()
        self.last_telemetry: dict[str, Any] = {}
        self.fps: float = 0.0
        self.connected: bool = False
        self.last_error: str = ""
        self._cmd_q: queue.Queue[tuple[str, Any]] = queue.Queue()
        self._stop = threading.Event()
        self._thread: threading.Thread | None = None
        self.dav_server: kernfs_mount.KernWebDAVServer | None = None
        self.dav_port: int = 8765
        self.dav_open_target: str | None = None
        self.dav_unmount_cmd: list[str] | None = None

    def connect(self, port: str, baud: int = 115200) -> None:
        self.disconnect()
        if port.lower() == "demo":
            self.demo_mode = True
            self.port = "demo"
            self.volume_name = "Quansheng UV-K1 DEMO"
            self.detected_model = "k1"
            self.connected = True
            self.last_error = ""
            self.last_telemetry = {"battery_mv": 8040, "battery_pct": 92}
            self.last_fb = make_demo_framebuffer()
            return
        with self.lock:
            cli = rigwire.RigWireClient(port, baudrate=baud, timeout=2.0)
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
            self.port = port
            self.last_telemetry = telem
            self.volume_name = kernfs_mount.resolve_volume_name(port, hello=hello, telemetry=telem)
            self.detected_model = "k5" if "k5" in self.volume_name.lower() else "k1"
            self.demo_mode = False
            self.connected = True
            self.last_error = ""
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
        self.connected = False
        self.demo_mode = False
        self.torch_on = False
        self.volume_name = "Quansheng UV-K1"
        self.last_telemetry = {}
        self.fps = 0.0

    def send_key(self, key_name: str, state_name: str) -> None:
        if self.demo_mode:
            if state_name == "press":
                if key_name == "up":
                    self.demo_freq_khz += 25
                elif key_name == "down":
                    self.demo_freq_khz = max(18000, self.demo_freq_khz - 25)
                self.demo_status = key_name.upper()[:6]
                fa = f"{self.demo_freq_khz // 1000}.{self.demo_freq_khz % 1000:03d}"
                self.last_fb = make_demo_framebuffer(freq_a=fa, status=self.demo_status)
            elif state_name == "hold":
                self.demo_status = f"H:{key_name.upper()[:4]}"
                fa = f"{self.demo_freq_khz // 1000}.{self.demo_freq_khz % 1000:03d}"
                self.last_fb = make_demo_framebuffer(freq_a=fa, status=self.demo_status)
            return
        code = rigwire.KEY_CODES[key_name]
        state = rigwire.KEY_STATES[state_name]
        self._cmd_q.put(("key", (code, state)))

    def toggle_flashlight(self) -> None:
        self.torch_on = not self.torch_on
        if self.demo_mode:
            self.demo_status = "TORCH" if self.torch_on else "DEMO"
            fa = f"{self.demo_freq_khz // 1000}.{self.demo_freq_khz % 1000:03d}"
            self.last_fb = make_demo_framebuffer(freq_a=fa, status=self.demo_status)
            return
        if self.connected:
            self._cmd_q.put(("torch", None))

    def start_dav(self, http_port: int = 8765, read_only: bool = False) -> str:
        if self.dav_server is not None:
            return f"http://127.0.0.1:{self.dav_port}/"
        if self.demo_mode:
            be: kernfs_mount.FsBackend = kernfs_mount.MemoryFsBackend()
            be.files["/sys/calib.dat"] = b"\xAA" * 512
        else:
            if not self.client:
                raise RuntimeError("Connect to a radio first")
            be = kernfs_mount.RigWireFsBackend(self.client, cache_ttl=3.0, shared_lock=self.lock)
        pol = kernfs_mount.SafetyPolicy(read_only=read_only)
        srv = kernfs_mount.KernWebDAVServer(("127.0.0.1", http_port), be, pol, volume_name=self.volume_name)
        self.dav_server = srv
        self.dav_port = srv.server_address[1]
        threading.Thread(target=srv.serve_forever, daemon=True).start()
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

    def _poll_loop(self) -> None:
        frames = 0
        t_fps = time.monotonic()
        last_telem = 0.0
        while not self._stop.is_set() and self.connected:
            while not self._cmd_q.empty():
                try:
                    kind, payload = self._cmd_q.get_nowait()
                    with self.lock:
                        if not self.client:
                            break
                        if kind == "key":
                            self.client.inject_key(payload[0], payload[1])
                        elif kind == "torch":
                            self.client.set_flashlight(2)
                except Exception as e:
                    self.last_error = str(e)

            try:
                with self.lock:
                    if not self.client:
                        break
                    fb = self.client.screencap()
                if len(fb) == 1024:
                    self.last_fb = fb
                    frames += 1
                    self.last_error = ""
            except Exception as e:
                self.last_error = str(e)
                time.sleep(0.25)
                continue

            now = time.monotonic()
            if now - t_fps >= 1.0:
                self.fps = frames / (now - t_fps)
                frames = 0
                t_fps = now

            if now - last_telem >= 2.5:
                last_telem = now
                try:
                    with self.lock:
                        if self.client:
                            self.last_telemetry = self.client.get_system_telemetry()
                except Exception:
                    pass

            time.sleep(0.04)


def list_available_ports() -> list[tuple[str, str]]:
    """Returns list of (port_id, human_label) candidates."""
    out: list[tuple[str, str]] = []
    try:
        from serial.tools import list_ports
        for p in list_ports.comports():
            desc = (p.product or p.description or "").strip()
            lbl = f"{p.device}  ({desc})" if desc and desc.lower() != "n/a" else p.device
            out.append((p.device, lbl))
    except Exception:
        pass
    out.append(("tcp://127.0.0.1:4446", "QEMU (127.0.0.1:4446)"))
    out.append(("demo", "Offline Demo"))
    return out


# =============================================================================
# 3. MODERN GTK4 + LIBADWAITA INDUSTRIAL DESIGN CSS
# =============================================================================

GTK_CSS = """
window.kern-dock-window {
    background: radial-gradient(circle at 50% 24%, #1e232c 0%, #13161c 62%, #0e1015 100%);
}

headerbar.kern-header {
    background: rgba(20, 23, 29, 0.92);
    border-bottom: 1px solid rgba(255, 255, 255, 0.06);
}

.dock-stage {
    padding: 14px 28px 18px 28px;
}

/* --- Outer Matte Polymer Body --- */
.radio-shell {
    background: linear-gradient(175deg, #25282e 0%, #191b20 55%, #131519 100%);
    border-radius: 26px 26px 32px 32px;
    padding: 8px;
    border: 1px solid rgba(255, 255, 255, 0.09);
    box-shadow:
        0 26px 54px rgba(0, 0, 0, 0.68),
        0 6px 16px rgba(0, 0, 0, 0.45),
        inset 0 1px 0 rgba(255, 255, 255, 0.12);
}

/* --- UV-K1 Gunmetal Metallic Faceplate (Matches K1_VIEW_D_FRONT.png) --- */
.faceplate-k1 {
    background: linear-gradient(162deg, #52575e 0%, #3d4147 34%, #31353b 72%, #292c31 100%);
    border-radius: 20px 20px 24px 24px;
    padding: 12px 15px 15px 15px;
    border: 1px solid rgba(255, 255, 255, 0.14);
    box-shadow:
        inset 0 1px 0 rgba(255, 255, 255, 0.22),
        inset 0 -2px 4px rgba(0, 0, 0, 0.45);
}

/* --- UV-K5 v3 Industrial Matte Charcoal Faceplate --- */
.faceplate-k5 {
    background: linear-gradient(170deg, #2f333a 0%, #22252b 48%, #1a1c21 100%);
    border-radius: 20px 20px 24px 24px;
    padding: 12px 15px 15px 15px;
    border: 1px solid rgba(255, 255, 255, 0.08);
    box-shadow:
        inset 0 1px 0 rgba(255, 255, 255, 0.10),
        inset 0 -2px 5px rgba(0, 0, 0, 0.55);
}

/* Recessed Torx corner bosses */
.torx-boss {
    min-width: 9px;
    min-height: 9px;
    border-radius: 99px;
    background: radial-gradient(circle at 35% 35%, #5b616a 0%, #1c1f24 65%, #0e1013 100%);
    border: 1px solid rgba(0, 0, 0, 0.55);
    box-shadow: inset 0 1px 1px rgba(255, 255, 255, 0.18);
}

/* Glossy black top nameplate pill (UV-K1) */
.nameplate-k1 {
    background: linear-gradient(180deg, #171a1f 0%, #090b0e 52%, #111419 100%);
    border-radius: 8px;
    padding: 4px 16px;
    border: 1px solid rgba(255, 255, 255, 0.08);
    box-shadow:
        inset 0 1px 0 rgba(255, 255, 255, 0.14),
        0 2px 4px rgba(0, 0, 0, 0.45);
}

/* Matte nameplate with subtle orange rule (UV-K5) */
.nameplate-k5 {
    background: linear-gradient(180deg, #1b1e23 0%, #14161a 100%);
    border-radius: 6px;
    padding: 4px 14px;
    border-bottom: 2px solid #f97316;
    box-shadow: 0 1px 3px rgba(0, 0, 0, 0.40);
}

.brand-wordmark {
    color: #eef2f7;
    font-weight: 800;
    font-style: italic;
    letter-spacing: 1.8px;
    font-size: 13px;
}

.brand-model-tag {
    color: rgba(255, 255, 255, 0.52);
    font-weight: 700;
    font-size: 9px;
    letter-spacing: 0.9px;
}

/* UV-K5 speaker grille slats */
.speaker-slot {
    min-height: 3px;
    border-radius: 99px;
    background: #0d0f12;
    box-shadow: inset 0 1px 1px rgba(0, 0, 0, 0.9), 0 1px 0 rgba(255, 255, 255, 0.07);
}

/* Status LED lens */
.status-led {
    min-width: 11px;
    min-height: 11px;
    border-radius: 99px;
    background: radial-gradient(circle at 35% 35%, #555c66 0%, #262a30 70%, #15181c 100%);
    border: 1px solid rgba(0, 0, 0, 0.65);
    transition: all 140ms ease;
}
.status-led.led-rx {
    background: radial-gradient(circle at 35% 35%, #86efac 0%, #22c55e 55%, #15803d 100%);
    box-shadow: 0 0 10px rgba(34, 197, 94, 0.78);
}
.status-led.led-tx {
    background: radial-gradient(circle at 35% 35%, #fca5a5 0%, #ef4444 55%, #b91c1c 100%);
    box-shadow: 0 0 12px rgba(239, 68, 68, 0.88);
}

/* Deep recessed LCD glass bezel */
.lcd-bezel {
    background: linear-gradient(180deg, #13161b 0%, #0a0c0f 100%);
    border-radius: 13px;
    padding: 9px;
    border: 1px solid rgba(0, 0, 0, 0.85);
    box-shadow:
        inset 0 3px 8px rgba(0, 0, 0, 0.85),
        inset 0 -1px 0 rgba(255, 255, 255, 0.12),
        0 1px 0 rgba(255, 255, 255, 0.12);
}

.lcd-frame {
    border-radius: 6px;
    padding: 4px;
    background: #080a0d;
    border: 1px solid rgba(255, 255, 255, 0.06);
}

/* Etched bezel subtitle + mic slot below screen */
.bezel-etch {
    color: rgba(255, 255, 255, 0.42);
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 0.5px;
}

.mic-slot {
    min-width: 24px;
    min-height: 5px;
    border-radius: 99px;
    background: #0b0d10;
    box-shadow: inset 0 1px 2px rgba(0, 0, 0, 0.95), 0 1px 0 rgba(255, 255, 255, 0.12);
}

/* Recessed precision 4x4 keypad well */
.keypad-well {
    background: #101216;
    border-radius: 13px;
    padding: 8px;
    border: 1px solid rgba(0, 0, 0, 0.82);
    box-shadow:
        inset 0 2px 6px rgba(0, 0, 0, 0.85),
        0 1px 0 rgba(255, 255, 255, 0.12);
}

/* Rubberized satin physical radio keys */
button.hw-key {
    background: linear-gradient(180deg, #2d323a 0%, #20242a 55%, #191c22 100%);
    color: #f3f6fa;
    border-radius: 8px;
    border: 1px solid rgba(255, 255, 255, 0.08);
    padding: 6px 7px;
    min-height: 40px;
    min-width: 66px;
    box-shadow:
        0 3px 5px rgba(0, 0, 0, 0.55),
        inset 0 1px 0 rgba(255, 255, 255, 0.14);
    transition: all 70ms ease;
}

button.hw-key:hover {
    background: linear-gradient(180deg, #353b45 0%, #262b33 55%, #1e2229 100%);
    border-color: rgba(255, 255, 255, 0.16);
}

button.hw-key:active,
button.hw-key.key-pressed {
    background: linear-gradient(180deg, #171a1f 0%, #22262e 100%);
    box-shadow:
        inset 0 2px 5px rgba(0, 0, 0, 0.85),
        0 0 0 1px rgba(45, 140, 255, 0.55);
    border-color: rgba(45, 140, 255, 0.65);
}

button.hw-key.hw-nav {
    min-height: 36px;
    background: linear-gradient(180deg, #2e333b 0%, #20242a 58%, #191c22 100%);
}

button.hw-key.hw-menu {
    background: linear-gradient(180deg, #313740 0%, #22262d 58%, #1a1e24 100%);
}
button.hw-key.hw-exit {
    background: linear-gradient(180deg, #313740 0%, #22262d 58%, #1a1e24 100%);
}

button.dock-connect {
    background: #2563eb;
    color: #ffffff;
    font-weight: 700;
    padding: 4px 14px;
    border-radius: 99px;
}
button.dock-connect:hover {
    background: #3b82f6;
}
button.dock-disconnect {
    background: rgba(239, 68, 68, 0.22);
    color: #fca5a5;
    border: 1px solid rgba(239, 68, 68, 0.42);
    font-weight: 700;
    padding: 4px 14px;
    border-radius: 99px;
}
button.dock-disconnect:hover {
    background: rgba(239, 68, 68, 0.34);
    color: #ffffff;
}

.key-primary {
    font-size: 15px;
    font-weight: 800;
    color: #f5f7fa;
}
.key-nav-label {
    font-size: 11px;
    font-weight: 800;
    letter-spacing: 0.6px;
    color: #f5f7fa;
}
.key-nav-arrow {
    font-size: 16px;
    font-weight: 800;
    color: #f5f7fa;
}
.key-sub-top {
    font-size: 8.5px;
    font-weight: 700;
    color: rgba(226, 234, 244, 0.78);
}
.key-sub-bot {
    font-size: 8px;
    font-weight: 500;
    color: rgba(168, 179, 194, 0.62);
}

/* --- Left Flank Protruding Hardware Keys (PTT, SK1, SK2) --- */
button.side-ptt {
    background: linear-gradient(90deg, #1b1e23 0%, #2a2f37 55%, #1d2026 100%);
    color: #eef2f7;
    border-radius: 10px 2px 2px 10px;
    border: 1px solid rgba(255, 255, 255, 0.09);
    border-right: none;
    min-width: 34px;
    min-height: 104px;
    padding: 4px 3px;
    box-shadow: -4px 4px 10px rgba(0, 0, 0, 0.48), inset 2px 0 0 rgba(45, 140, 255, 0.90);
    font-size: 10px;
    font-weight: 800;
    letter-spacing: 0.8px;
    transition: all 70ms ease;
}
button.side-ptt.ptt-k5 {
    box-shadow: -4px 4px 10px rgba(0, 0, 0, 0.48), inset 2px 0 0 rgba(249, 115, 22, 0.90);
}
button.side-ptt:hover {
    background: linear-gradient(90deg, #252a32 0%, #343b45 60%, #23272e 100%);
}
button.side-ptt:active,
button.side-ptt.key-pressed {
    background: linear-gradient(90deg, #7f1d1d 0%, #b91c1c 100%);
    box-shadow: -2px 2px 12px rgba(239, 68, 68, 0.55), inset 2px 0 0 #fca5a5;
    color: #ffffff;
}

button.side-key {
    background: linear-gradient(90deg, #1c1f25 0%, #2b3038 65%, #1d2026 100%);
    color: rgba(230, 236, 245, 0.84);
    border-radius: 8px 2px 2px 8px;
    border: 1px solid rgba(255, 255, 255, 0.08);
    border-right: none;
    min-width: 30px;
    min-height: 40px;
    padding: 2px;
    box-shadow: -3px 3px 8px rgba(0, 0, 0, 0.45);
    font-size: 9px;
    font-weight: 700;
}
button.side-key:hover {
    background: linear-gradient(90deg, #262b33 0%, #353c46 65%, #23272e 100%);
}
button.side-key:active,
button.side-key.key-pressed {
    background: #14171c;
    box-shadow: inset 1px 1px 4px rgba(0, 0, 0, 0.85), inset 2px 0 0 rgba(45, 140, 255, 0.8);
}

/* Right-side subtle Kenwood/USB-C rubber flap detail */
.right-flap {
    background: linear-gradient(270deg, #181a1f 0%, #24282f 100%);
    border-radius: 2px 6px 6px 2px;
    border: 1px solid rgba(255, 255, 255, 0.06);
    border-left: none;
    min-width: 9px;
    min-height: 86px;
    box-shadow: 3px 3px 8px rgba(0, 0, 0, 0.35);
}

/* --- Bottom Compact Studio Telemetry Pill --- */
.telemetry-bar {
    background: rgba(24, 28, 36, 0.84);
    border-radius: 12px;
    border: 1px solid rgba(255, 255, 255, 0.07);
    padding: 7px 14px;
    margin-top: 12px;
}

.telem-label {
    font-size: 11px;
    font-weight: 600;
    color: rgba(232, 238, 246, 0.88);
}
.telem-muted {
    font-size: 10.5px;
    color: rgba(164, 174, 188, 0.68);
}
.telem-pill {
    background: rgba(255, 255, 255, 0.06);
    border-radius: 99px;
    padding: 2px 9px;
    font-size: 10.5px;
    font-weight: 700;
    color: #dbe4f0;
}
.telem-pill-ok {
    background: rgba(34, 197, 94, 0.16);
    color: #86efac;
}
"""


# =============================================================================
# 4. GTK4 + LIBADWAITA APPLICATION
# =============================================================================

def launch_gtk4(
    initial_port: str | None = None,
    baud: int = 115200,
    scale: int = 3,
    theme: str | None = None,
    model: str = "k1",
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
            self.model_id = model if model in CHASSIS_PROFILES else "k1"
            self.user_picked_theme = theme is not None
            self.theme_name = (
                theme if (theme and theme in LCD_THEMES)
                else CHASSIS_PROFILES[self.model_id]["default_theme"]
            )
            self.lcd_scale = scale
            self.pixel_grid = True
            self.mount_read_only = False
            self._hold_timers: dict[str, int] = {}
            self._key_widgets: dict[str, Any] = {}
            self._pressed_keys: set[str] = set()
            self._lcd_buf: bytearray | None = None
            self._lcd_surface: Any = None
            self._last_render_key: Any = None
            self._ports: list[tuple[str, str]] = []
            self._glow_provider = Gtk.CssProvider()

        def do_activate(self) -> None:
            if has_adw:
                Adw.StyleManager.get_default().set_color_scheme(Adw.ColorScheme.PREFER_DARK)

            display = Gdk.Display.get_default()
            css = Gtk.CssProvider()
            css.load_from_string(GTK_CSS)
            Gtk.StyleContext.add_provider_for_display(
                display, css, Gtk.STYLE_PROVIDER_PRIORITY_APPLICATION
            )
            Gtk.StyleContext.add_provider_for_display(
                display, self._glow_provider, Gtk.STYLE_PROVIDER_PRIORITY_APPLICATION + 1
            )

            self.win = base_win_cls(application=self)
            self.win.add_css_class("kern-dock-window")
            self.win.set_resizable(False)
            self.win.set_title("Kern Dock")

            # --- Build HeaderBar ---
            header = Adw.HeaderBar() if has_adw else Gtk.HeaderBar()
            header.add_css_class("kern-header")

            # Left: Port selector + Refresh + Connect
            port_box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=6)
            self.port_strings = Gtk.StringList()
            self.port_dd = Gtk.DropDown(model=self.port_strings)
            self.port_dd.set_size_request(148, -1)
            self.port_dd.set_tooltip_text("Select serial port, QEMU TCP bridge, or Offline Demo")
            self._refresh_ports(select_port=initial_port)
            port_box.append(self.port_dd)

            refresh_btn = Gtk.Button.new_from_icon_name("view-refresh-symbolic")
            refresh_btn.set_tooltip_text("Rescan USB serial ports")
            refresh_btn.connect("clicked", lambda _b: self._refresh_ports())
            port_box.append(refresh_btn)

            self.conn_btn = Gtk.Button(label="Connect")
            self.conn_btn.add_css_class("dock-connect")
            self.conn_btn.connect("clicked", self._on_connect_toggle)
            port_box.append(self.conn_btn)
            header.pack_start(port_box)

            # Center: Window title
            if has_adw:
                self.win_title = Adw.WindowTitle(title="Kern Dock", subtitle="Disconnected")
                header.set_title_widget(self.win_title)
            else:
                self.win_title = None

            # Right: Skin switcher + Screenshot + Mount FS + Open FS + Sideload + Settings
            right_box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=6)

            skin_grp = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=0)
            skin_grp.add_css_class("linked")
            self.btn_k1 = Gtk.ToggleButton(label="K1")
            self.btn_k5 = Gtk.ToggleButton(label="K5")
            self.btn_k5.set_group(self.btn_k1)
            self.btn_k1.set_active(self.model_id == "k1")
            self.btn_k5.set_active(self.model_id == "k5")
            self.btn_k1.set_tooltip_text("Quansheng UV-K1 Gunmetal Chassis")
            self.btn_k5.set_tooltip_text("Quansheng UV-K5 v3 Matte Chassis")
            self.btn_k1.connect("toggled", lambda b: self._set_model("k1") if b.get_active() else None)
            self.btn_k5.connect("toggled", lambda b: self._set_model("k5") if b.get_active() else None)
            skin_grp.append(self.btn_k1)
            skin_grp.append(self.btn_k5)
            right_box.append(skin_grp)

            shot_btn = Gtk.Button.new_from_icon_name("camera-photo-symbolic")
            shot_btn.set_tooltip_text("Save Crisp 4x PNG Screenshot")
            shot_btn.connect("clicked", self._on_save_screenshot)
            right_box.append(shot_btn)

            self.mount_btn = Gtk.ToggleButton()
            self.mount_btn.set_icon_name("drive-harddisk-symbolic")
            self.mount_btn.set_tooltip_text("Mount KernFS over RigWire (WebDAV)")
            self.mount_btn.connect("toggled", self._on_mount_toggled)
            right_box.append(self.mount_btn)

            self.open_fs_rev = Gtk.Revealer()
            self.open_fs_rev.set_transition_type(Gtk.RevealerTransitionType.SLIDE_RIGHT)
            open_fs_btn = Gtk.Button.new_from_icon_name("folder-open-symbolic")
            open_fs_btn.set_tooltip_text("Open Mounted Radio Filesystem in File Manager")
            open_fs_btn.connect("clicked", self._on_open_fs)
            self.open_fs_rev.set_child(open_fs_btn)
            right_box.append(self.open_fs_rev)

            sideload_btn = Gtk.Button.new_from_icon_name("document-send-symbolic")
            sideload_btn.set_tooltip_text("Sideload .krx / .mod Package to Radio")
            sideload_btn.connect("clicked", self._on_sideload)
            right_box.append(sideload_btn)

            # Settings Popover Menu
            menu_btn = Gtk.MenuButton(icon_name="open-menu-symbolic")
            menu_btn.set_tooltip_text("Display, Backlight & Mount Settings")
            menu_btn.set_popover(self._build_settings_popover())
            right_box.append(menu_btn)

            header.pack_end(right_box)

            # --- Main Stage ---
            stage = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=0)
            stage.add_css_class("dock-stage")
            stage.set_halign(Gtk.Align.CENTER)

            # 1. Top Hardware Silhouette (Antenna + Clickable Flashlight + Cobalt/Orange Volume Knob)
            self.top_cap = Gtk.DrawingArea()
            self.top_cap.set_content_width(428)
            self.top_cap.set_content_height(48)
            self.top_cap.set_halign(Gtk.Align.CENTER)
            self.top_cap.set_margin_start(22)
            self.top_cap.set_tooltip_text("Click the center LED lens to toggle the radio flashlight")
            self.top_cap.set_draw_func(self._draw_top_cap)
            cap_click = Gtk.GestureClick()
            cap_click.connect("pressed", self._on_top_cap_click)
            self.top_cap.add_controller(cap_click)
            stage.append(self.top_cap)

            # 2. Horizontal Radio Assembly: Left Side Keys | Main Chassis | Right Port Flap
            radio_row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=0)
            radio_row.set_halign(Gtk.Align.CENTER)

            # Left Flank (PTT + SK1 + SK2)
            flank = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=8)
            flank.set_valign(Gtk.Align.START)
            flank.set_margin_top(62)

            self.ptt_btn = self._make_side_button("P\nT\nT", "ptt", is_ptt=True)
            self.sk1_btn = self._make_side_button("SK1", "side1")
            self.sk2_btn = self._make_side_button("SK2", "side2")
            flank.append(self.ptt_btn)
            flank.append(self.sk1_btn)
            flank.append(self.sk2_btn)
            radio_row.append(flank)

            # Main Radio Outer Polymer Shell
            shell = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=0)
            shell.add_css_class("radio-shell")

            self.faceplate = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=10)
            self.faceplate.add_css_class("faceplate-k1" if self.model_id == "k1" else "faceplate-k5")

            # Top nameplate bar with Torx bosses + LED + Brand pill
            top_bar = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
            top_bar.set_valign(Gtk.Align.CENTER)

            left_corner = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=6)
            left_corner.set_valign(Gtk.Align.CENTER)
            boss_tl = Gtk.Box()
            boss_tl.add_css_class("torx-boss")
            self.led_dot = Gtk.Box()
            self.led_dot.add_css_class("status-led")
            self.led_dot.set_tooltip_text("Status LED (Green = Connected/RX, Red = PTT TX)")
            left_corner.append(boss_tl)
            left_corner.append(self.led_dot)
            top_bar.append(left_corner)

            self.nameplate = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=6)
            self.nameplate.set_hexpand(True)
            self.nameplate.set_halign(Gtk.Align.CENTER)
            self.nameplate.add_css_class("nameplate-k1" if self.model_id == "k1" else "nameplate-k5")
            rf_mark = Gtk.DrawingArea()
            rf_mark.set_content_width(14)
            rf_mark.set_content_height(14)
            rf_mark.set_valign(Gtk.Align.CENTER)
            rf_mark.set_draw_func(self._draw_rf_arcs)
            self.nameplate.append(rf_mark)
            brand_lbl = Gtk.Label(label="QUANSHENG")
            brand_lbl.add_css_class("brand-wordmark")
            self.model_tag_lbl = Gtk.Label(label=CHASSIS_PROFILES[self.model_id]["name"])
            self.model_tag_lbl.add_css_class("brand-model-tag")
            self.nameplate.append(brand_lbl)
            self.nameplate.append(self.model_tag_lbl)
            top_bar.append(self.nameplate)

            boss_tr = Gtk.Box()
            boss_tr.add_css_class("torx-boss")
            boss_tr.set_valign(Gtk.Align.CENTER)
            top_bar.append(boss_tr)
            self.faceplate.append(top_bar)

            # UV-K5 Speaker Grille Louvers (revealed only in K5 skin)
            self.speaker_rev = Gtk.Revealer()
            self.speaker_rev.set_transition_type(Gtk.RevealerTransitionType.SLIDE_DOWN)
            grille = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=4)
            grille.set_margin_start(14)
            grille.set_margin_end(14)
            for _ in range(3):
                slot = Gtk.Box()
                slot.add_css_class("speaker-slot")
                grille.append(slot)
            self.speaker_rev.set_child(grille)
            self.speaker_rev.set_reveal_child(self.model_id == "k5")
            self.faceplate.append(self.speaker_rev)

            # Recessed Glass LCD Bezel
            bezel = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=0)
            bezel.add_css_class("lcd-bezel")
            self.lcd_frame = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=0)
            self.lcd_frame.add_css_class("lcd-frame")
            self.lcd_frame.add_css_class("lcd-glow-active")

            self.lcd_area = Gtk.DrawingArea()
            self.lcd_area.set_content_width(LCD_W * self.lcd_scale)
            self.lcd_area.set_content_height(LCD_H * self.lcd_scale)
            self.lcd_area.set_draw_func(self._draw_lcd)
            self.lcd_frame.append(self.lcd_area)
            bezel.append(self.lcd_frame)
            self.faceplate.append(bezel)

            # Etched lower bezel legend + microphone cutout (matches K1_VIEW_D_FRONT.png)
            etch_row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
            etch_row.set_margin_start(4)
            etch_row.set_margin_end(4)
            self.etch_device_lbl = Gtk.Label(label="KERN OS")
            self.etch_device_lbl.add_css_class("bezel-etch")
            self.etch_device_lbl.set_halign(Gtk.Align.START)
            etch_row.append(self.etch_device_lbl)

            spacer = Gtk.Box()
            spacer.set_hexpand(True)
            etch_row.append(spacer)

            transceiver_lbl = Gtk.Label(label="UHF/VHF Transceiver")
            transceiver_lbl.add_css_class("bezel-etch")
            etch_row.append(transceiver_lbl)

            mic_slot = Gtk.Box()
            mic_slot.add_css_class("mic-slot")
            mic_slot.set_valign(Gtk.Align.CENTER)
            etch_row.append(mic_slot)
            self.faceplate.append(etch_row)

            # Recessed 4x4 Tactile Keypad Matrix
            keypad_well = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=0)
            keypad_well.add_css_class("keypad-well")

            self.keypad_grid = Gtk.Grid(row_spacing=6, column_spacing=6)
            self.keypad_grid.set_row_homogeneous(False)
            self.keypad_grid.set_column_homogeneous(True)

            # Row 0: Navigation keys (labels switch dynamically between K1 and K5)
            self._nav_labels: dict[str, Gtk.Label] = {}
            for col, (kname, primary, _, _) in enumerate(CHASSIS_PROFILES[self.model_id]["nav_row"]):
                btn, lbl = self._make_nav_button(kname, primary)
                self._nav_labels[kname] = lbl
                self.keypad_grid.attach(btn, col, 0, 1, 1)

            # Rows 1-3: Numeric + Star/0/#/F keys (exact K1/K5 physical 4x4 layout)
            for r_idx, row in enumerate(KEYPAD_ROWS_NUMERIC, start=1):
                for c_idx, (kname, primary, sub_t, sub_b) in enumerate(row):
                    btn = self._make_numeric_button(kname, primary, sub_t, sub_b)
                    self.keypad_grid.attach(btn, c_idx, r_idx, 1, 1)

            keypad_well.append(self.keypad_grid)
            self.faceplate.append(keypad_well)

            # Lower corner Torx bosses
            bot_bosses = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL)
            b_bl = Gtk.Box()
            b_bl.add_css_class("torx-boss")
            b_sp = Gtk.Box()
            b_sp.set_hexpand(True)
            b_br = Gtk.Box()
            b_br.add_css_class("torx-boss")
            bot_bosses.append(b_bl)
            bot_bosses.append(b_sp)
            bot_bosses.append(b_br)
            self.faceplate.append(bot_bosses)

            shell.append(self.faceplate)
            radio_row.append(shell)

            # Right Flank (subtle Kenwood / USB-C rubber port cover)
            r_flank = Gtk.Box(orientation=Gtk.Orientation.VERTICAL)
            r_flank.set_valign(Gtk.Align.START)
            r_flank.set_margin_top(92)
            flap = Gtk.Box()
            flap.add_css_class("right-flap")
            flap.set_tooltip_text("USB-C / Accessory Port")
            r_flank.append(flap)
            radio_row.append(r_flank)

            stage.append(radio_row)

            # 3. Bottom Studio Telemetry Strip
            telem_bar = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=10)
            telem_bar.add_css_class("telemetry-bar")

            self.telem_status = Gtk.Label(label="Disconnected")
            self.telem_status.add_css_class("telem-label")
            self.telem_status.set_halign(Gtk.Align.START)
            telem_bar.append(self.telem_status)

            self.telem_fps = Gtk.Label(label="0.0 FPS")
            self.telem_fps.add_css_class("telem-pill")
            telem_bar.append(self.telem_fps)

            self.telem_batt = Gtk.Label(label="— V")
            self.telem_batt.add_css_class("telem-pill")
            telem_bar.append(self.telem_batt)

            self.telem_mount = Gtk.Label(label="")
            self.telem_mount.add_css_class("telem-pill")
            self.telem_mount.add_css_class("telem-pill-ok")
            self.telem_mount.set_visible(False)
            telem_bar.append(self.telem_mount)

            telem_spacer = Gtk.Box()
            telem_spacer.set_hexpand(True)
            telem_bar.append(telem_spacer)

            hint = Gtk.Label(label="Right-click key = Hold")
            hint.add_css_class("telem-muted")
            telem_bar.append(hint)
            stage.append(telem_bar)

            # Wrap in ToastOverlay + ToolbarView (Adw) or VBox (plain GTK4)
            if has_adw:
                self.toast_overlay = Adw.ToastOverlay()
                self.toast_overlay.set_child(stage)
                tb_view = Adw.ToolbarView()
                tb_view.add_top_bar(header)
                tb_view.set_content(self.toast_overlay)
                self.win.set_content(tb_view)
            else:
                self.toast_overlay = None
                outer = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=0)
                outer.append(header)
                outer.append(stage)
                self.win.set_child(outer)

            # Keyboard shortcuts controller
            key_ctrl = Gtk.EventControllerKey()
            key_ctrl.connect("key-pressed", self._on_pc_key_pressed)
            key_ctrl.connect("key-released", self._on_pc_key_released)
            self.win.add_controller(key_ctrl)

            self.win.connect("close-request", self._on_close_request)
            self._apply_theme_glow()
            self._apply_skin_classes()

            GLib.timeout_add(33, self._tick_ui)
            if initial_port:
                GLib.timeout_add(120, self._auto_connect_once)

            self.win.present()

        # --- Toast helper ---
        def _notify(self, msg: str) -> None:
            if self.toast_overlay is not None and has_adw:
                t = Adw.Toast.new(msg)
                t.set_timeout(3)
                self.toast_overlay.add_toast(t)
            else:
                self.telem_status.set_text(msg)

        # --- Settings Popover ---
        def _build_settings_popover(self) -> Any:
            pop = Gtk.Popover()
            box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=10)
            box.set_margin_top(12)
            box.set_margin_bottom(12)
            box.set_margin_start(14)
            box.set_margin_end(14)
            box.set_size_request(250, -1)

            # LCD Backlight Theme
            t_lbl = Gtk.Label(label="LCD Backlight Theme", xalign=0)
            t_lbl.add_css_class("heading")
            box.append(t_lbl)

            theme_names = list(LCD_THEMES.keys())
            self.theme_dd = Gtk.DropDown.new_from_strings(theme_names)
            self.theme_dd.set_selected(theme_names.index(self.theme_name))
            self.theme_dd.connect("notify::selected", self._on_theme_changed)
            box.append(self.theme_dd)

            # LCD Scale
            s_row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
            s_row.append(Gtk.Label(label="LCD Scale", xalign=0, hexpand=True))
            scale_grp = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=0)
            scale_grp.add_css_class("linked")
            first_btn = None
            for sc in (2, 3, 4):
                b = Gtk.ToggleButton(label=f"{sc}×")
                if first_btn is None:
                    first_btn = b
                else:
                    b.set_group(first_btn)
                b.set_active(self.lcd_scale == sc)
                b.connect("toggled", lambda btn, v=sc: self._set_scale(v) if btn.get_active() else None)
                scale_grp.append(b)
            s_row.append(scale_grp)
            box.append(s_row)

            # Pixel Grid toggle
            g_row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
            g_row.append(Gtk.Label(label="STN Dot-Matrix Grid", xalign=0, hexpand=True))
            g_sw = Gtk.Switch(active=self.pixel_grid)
            g_sw.connect("notify::active", lambda sw, _p: self._set_grid(sw.get_active()))
            g_row.append(g_sw)
            box.append(g_row)

            # Read-Only Mount toggle
            ro_row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
            ro_row.append(Gtk.Label(label="Read-Only FS Mount", xalign=0, hexpand=True))
            ro_sw = Gtk.Switch(active=self.mount_read_only)
            ro_sw.connect("notify::active", lambda sw, _p: setattr(self, "mount_read_only", sw.get_active()))
            ro_row.append(ro_sw)
            box.append(ro_row)

            box.append(Gtk.Separator())
            kb = Gtk.Label(
                label="Shortcuts:\n  Enter = MENU   Esc = EXIT\n  Arrows = Up/Down   Space = PTT\n  0–9, *, # / F = Keypad   F1/F2 = SK1/SK2",
                xalign=0,
            )
            kb.add_css_class("telem-muted")
            box.append(kb)

            pop.set_child(box)
            return pop

        # --- Quansheng Curved RF Wave Emblem (Cairo) ---
        def _draw_rf_arcs(self, _area: Any, cr: Any, w: int, h: int) -> None:
            cr.set_source_rgba(0.92, 0.95, 0.98, 0.85)
            cr.set_line_width(1.5)
            cr.set_line_cap(cairo.LINE_CAP_ROUND)
            cx, cy = w - 2.0, h * 0.58
            for r in (4.5, 8.2):
                cr.arc(cx, cy, r, math.pi * 0.82, math.pi * 1.48)
                cr.stroke()

        # --- Hardware Top Cap Cairo Art (Antenna + Flashlight + Rotary Knob) ---
        def _draw_top_cap(self, _area: Any, cr: Any, w: int, h: int) -> None:
            prof = CHASSIS_PROFILES[self.model_id]
            ar, ag, ab = prof["accent_rgb"]

            # 1. Left: Stubby SMA Antenna base + ribbed stub
            ant_x = 66
            # Brass/gunmetal SMA collar
            pat_collar = cairo.LinearGradient(ant_x - 16, 0, ant_x + 16, 0)
            pat_collar.add_color_stop_rgb(0.0, 0.14, 0.16, 0.18)
            pat_collar.add_color_stop_rgb(0.45, 0.32, 0.35, 0.40)
            pat_collar.add_color_stop_rgb(1.0, 0.10, 0.11, 0.13)
            cr.set_source(pat_collar)
            cr.rectangle(ant_x - 16, h - 10, 32, 10)
            cr.fill()

            # Stubby matte-black antenna barrel
            pat_ant = cairo.LinearGradient(ant_x - 13, 0, ant_x + 13, 0)
            pat_ant.add_color_stop_rgb(0.0, 0.10, 0.11, 0.13)
            pat_ant.add_color_stop_rgb(0.35, 0.24, 0.26, 0.30)
            pat_ant.add_color_stop_rgb(1.0, 0.08, 0.09, 0.11)
            cr.set_source(pat_ant)
            cr.move_to(ant_x - 13, h - 10)
            cr.line_to(ant_x - 11, 6)
            cr.arc(ant_x, 8, 11, math.pi, 0)
            cr.line_to(ant_x + 13, h - 10)
            cr.close_path()
            cr.fill()
            # Subtle horizontal grip rings
            cr.set_source_rgba(0, 0, 0, 0.45)
            cr.set_line_width(1.2)
            for ry in (15, 22, 29):
                cr.move_to(ant_x - 11, ry)
                cr.line_to(ant_x + 11, ry)
                cr.stroke()

            # 2. Center: Recessed LED Flashlight Dome (clickable!)
            led_x = w / 2.0
            if self.link.torch_on:
                glow = cairo.RadialGradient(led_x, h - 14, 2, led_x, h - 14, 34)
                glow.add_color_stop_rgba(0.0, 1.0, 0.97, 0.80, 0.92)
                glow.add_color_stop_rgba(0.45, 1.0, 0.93, 0.62, 0.42)
                glow.add_color_stop_rgba(1.0, 1.0, 0.90, 0.50, 0.0)
                cr.set_source(glow)
                cr.arc(led_x, h - 14, 34, 0, 2 * math.pi)
                cr.fill()

            # Bezel ring
            cr.set_source_rgb(0.16, 0.18, 0.21)
            cr.rectangle(led_x - 14, h - 12, 28, 12)
            cr.fill()
            # Glass bulb dome
            bulb = cairo.RadialGradient(led_x - 2, h - 16, 1, led_x, h - 12, 11)
            if self.link.torch_on:
                bulb.add_color_stop_rgb(0.0, 1.0, 1.0, 0.95)
                bulb.add_color_stop_rgb(1.0, 0.98, 0.85, 0.45)
            else:
                bulb.add_color_stop_rgb(0.0, 0.72, 0.78, 0.84)
                bulb.add_color_stop_rgb(0.65, 0.34, 0.38, 0.44)
                bulb.add_color_stop_rgb(1.0, 0.14, 0.16, 0.20)
            cr.set_source(bulb)
            cr.arc(led_x, h - 11, 9, math.pi, 0)
            cr.close_path()
            cr.fill()

            # 3. Right: Knurled Volume/Power Knob with UV-K1 Cobalt Blue or UV-K5 Orange Cap!
            knob_x = w - 68
            pat_knob = cairo.LinearGradient(knob_x - 22, 0, knob_x + 22, 0)
            pat_knob.add_color_stop_rgb(0.0, 0.09, 0.10, 0.12)
            pat_knob.add_color_stop_rgb(0.32, 0.27, 0.29, 0.33)
            pat_knob.add_color_stop_rgb(0.75, 0.16, 0.18, 0.21)
            pat_knob.add_color_stop_rgb(1.0, 0.07, 0.08, 0.10)
            cr.set_source(pat_knob)
            cr.move_to(knob_x - 22, h)
            cr.line_to(knob_x - 19, 12)
            cr.line_to(knob_x + 19, 12)
            cr.line_to(knob_x + 22, h)
            cr.close_path()
            cr.fill()

            # Vertical knurl grooves
            cr.set_source_rgba(0, 0, 0, 0.42)
            cr.set_line_width(1.6)
            for dx in (-13, -7, 0, 7, 13):
                cr.move_to(knob_x + dx, 15)
                cr.line_to(knob_x + dx * 1.08, h - 2)
                cr.stroke()

            # Signature colored top accent stripe & cap (Blue for UV-K1, Orange for UV-K5)
            cr.set_source_rgb(ar, ag, ab)
            cr.rectangle(knob_x - 15, 9, 30, 4)
            cr.fill()
            # Center pointer notch down the front of the knob
            cr.rectangle(knob_x - 2.2, 9, 4.4, 15)
            cr.fill()
            # Top specular gleam
            cr.set_source_rgba(1, 1, 1, 0.35)
            cr.rectangle(knob_x - 13, 9.5, 26, 1.2)
            cr.fill()

        def _on_top_cap_click(self, _gesture: Any, _n: int, x: float, _y: float) -> None:
            w = self.top_cap.get_width()
            if abs(x - w / 2.0) <= 36:
                self.link.toggle_flashlight()
                self.top_cap.queue_draw()
                self._notify("Flashlight toggled")

        # --- Hardware-Backed Crisp ST7565 LCD Drawing ---
        def _draw_lcd(self, _area: Any, cr: Any, w: int, h: int) -> None:
            if self._lcd_surface is None:
                self._rebuild_lcd_surface()
            if self._lcd_surface is not None:
                cr.save()
                sx = w / float(self._lcd_surface.get_width())
                sy = h / float(self._lcd_surface.get_height())
                cr.scale(sx, sy)
                cr.set_source_surface(self._lcd_surface, 0, 0)
                cr.get_source().set_filter(cairo.FILTER_NEAREST)
                cr.paint()
                cr.restore()

            # Subtle realistic anti-glare LCD glass sheen across top-left diagonal
            sheen = cairo.LinearGradient(0, 0, w * 0.65, h * 0.85)
            sheen.add_color_stop_rgba(0.0, 1.0, 1.0, 1.0, 0.08)
            sheen.add_color_stop_rgba(0.38, 1.0, 1.0, 1.0, 0.02)
            sheen.add_color_stop_rgba(0.39, 1.0, 1.0, 1.0, 0.0)
            sheen.add_color_stop_rgba(1.0, 0.0, 0.0, 0.0, 0.06)
            cr.set_source(sheen)
            cr.rectangle(0, 0, w, h)
            cr.fill()

        def _rebuild_lcd_surface(self) -> None:
            w, h, buf = render_st7565_bgra(
                self.link.last_fb,
                scale=self.lcd_scale,
                theme_name=self.theme_name,
                pixel_grid=self.pixel_grid,
            )
            self._lcd_buf = buf
            self._lcd_surface = cairo.ImageSurface.create_for_data(
                buf, cairo.FORMAT_RGB24, w, h, w * 4
            )

        # --- Key Widget Factories & Exact Press/Hold/Release Controllers ---
        def _attach_key_gestures(self, widget: Any, kname: str) -> None:
            self._key_widgets[kname] = widget
            g_left = Gtk.GestureClick()
            g_left.set_button(1)
            g_left.connect("pressed", lambda _g, _n, _x, _y, k=kname: self._press_key(k))
            g_left.connect("released", lambda _g, _n, _x, _y, k=kname: self._release_key(k))
            g_left.connect("stopped", lambda _g, k=kname: self._release_key(k))
            widget.add_controller(g_left)

            g_right = Gtk.GestureClick()
            g_right.set_button(3)
            g_right.connect("pressed", lambda _g, _n, _x, _y, k=kname: self._long_click_key(k))
            widget.add_controller(g_right)

        def _make_nav_button(self, kname: str, label: str) -> tuple[Any, Any]:
            btn = Gtk.Button()
            btn.set_focusable(False)
            btn.add_css_class("hw-key")
            btn.add_css_class("hw-nav")
            if kname == "menu":
                btn.add_css_class("hw-menu")
            elif kname == "exit":
                btn.add_css_class("hw-exit")
            lbl = Gtk.Label(label=label)
            lbl.add_css_class("key-nav-arrow" if kname in ("up", "down") else "key-nav-label")
            btn.set_child(lbl)
            self._attach_key_gestures(btn, kname)
            return btn, lbl

        def _make_numeric_button(self, kname: str, primary: str, sub_t: str, sub_b: str) -> Any:
            btn = Gtk.Button()
            btn.set_focusable(False)
            btn.add_css_class("hw-key")

            row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=5)
            row.set_halign(Gtk.Align.CENTER)
            row.set_valign(Gtk.Align.CENTER)

            p_lbl = Gtk.Label(label=primary)
            p_lbl.add_css_class("key-primary")
            row.append(p_lbl)

            if sub_t or sub_b:
                sub_col = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=0)
                sub_col.set_valign(Gtk.Align.CENTER)
                if sub_t:
                    st = Gtk.Label(label=sub_t, xalign=0)
                    st.add_css_class("key-sub-top")
                    sub_col.append(st)
                if sub_b:
                    sb = Gtk.Label(label=sub_b, xalign=0)
                    sb.add_css_class("key-sub-bot")
                    sub_col.append(sb)
                row.append(sub_col)

            btn.set_child(row)
            self._attach_key_gestures(btn, kname)
            return btn

        def _make_side_button(self, label: str, kname: str, is_ptt: bool = False) -> Any:
            btn = Gtk.Button(label=label)
            btn.set_focusable(False)
            btn.add_css_class("side-ptt" if is_ptt else "side-key")
            self._attach_key_gestures(btn, kname)
            return btn

        # --- Key Event Dispatch ---
        def _press_key(self, kname: str) -> None:
            if kname in self._pressed_keys:
                return
            self._pressed_keys.add(kname)
            w = self._key_widgets.get(kname)
            if w is not None:
                w.add_css_class("key-pressed")
            if kname == "ptt" and self.link.connected:
                self.led_dot.remove_css_class("led-rx")
                self.led_dot.add_css_class("led-tx")
            if not self.link.connected:
                return
            self.link.send_key(kname, "press")
            if kname in self._hold_timers:
                GLib.source_remove(self._hold_timers.pop(kname))

            def _fire_hold(k: str = kname) -> bool:
                self._hold_timers.pop(k, None)
                if k in self._pressed_keys and self.link.connected:
                    self.link.send_key(k, "hold")
                return False

            self._hold_timers[kname] = GLib.timeout_add(450, _fire_hold)

        def _release_key(self, kname: str) -> None:
            was_down = kname in self._pressed_keys
            self._pressed_keys.discard(kname)
            w = self._key_widgets.get(kname)
            if w is not None:
                w.remove_css_class("key-pressed")
            if kname in self._hold_timers:
                GLib.source_remove(self._hold_timers.pop(kname))
            if kname == "ptt":
                self.led_dot.remove_css_class("led-tx")
                if self.link.connected:
                    self.led_dot.add_css_class("led-rx")
            if was_down and self.link.connected:
                self.link.send_key(kname, "release")

        def _long_click_key(self, kname: str) -> None:
            if not self.link.connected:
                return
            w = self._key_widgets.get(kname)
            if w is not None:
                w.add_css_class("key-pressed")
            self.link.send_key(kname, "press")

            def _step_hold() -> bool:
                if self.link.connected:
                    self.link.send_key(kname, "hold")
                return False

            def _step_rel() -> bool:
                if w is not None:
                    w.remove_css_class("key-pressed")
                if self.link.connected:
                    self.link.send_key(kname, "release")
                return False

            GLib.timeout_add(120, _step_hold)
            GLib.timeout_add(270, _step_rel)

        # --- PC Keyboard Mapping ---
        _KEYVAL_MAP = {
            Gdk.KEY_Return: "menu", Gdk.KEY_KP_Enter: "menu",
            Gdk.KEY_Escape: "exit", Gdk.KEY_BackSpace: "exit",
            Gdk.KEY_Up: "up", Gdk.KEY_Left: "up",
            Gdk.KEY_Down: "down", Gdk.KEY_Right: "down",
            Gdk.KEY_f: "f", Gdk.KEY_F: "f", Gdk.KEY_numbersign: "f",
            Gdk.KEY_asterisk: "star", Gdk.KEY_KP_Multiply: "star",
            Gdk.KEY_F1: "side1", Gdk.KEY_F2: "side2",
            Gdk.KEY_space: "ptt",
            **{getattr(Gdk, f"KEY_{d}"): str(d) for d in range(10)},
            **{getattr(Gdk, f"KEY_KP_{d}"): str(d) for d in range(10)},
        }

        def _on_pc_key_pressed(self, _ctrl: Any, keyval: int, _kc: int, _state: Any) -> bool:
            kn = self._KEYVAL_MAP.get(keyval)
            if kn:
                self._press_key(kn)
                return True
            return False

        def _on_pc_key_released(self, _ctrl: Any, keyval: int, _kc: int, _state: Any) -> None:
            kn = self._KEYVAL_MAP.get(keyval)
            if kn:
                self._release_key(kn)

        # --- Skin, Theme, Scale & Port Controls ---
        def _apply_theme_glow(self) -> None:
            _, _, _, glow = LCD_THEMES.get(self.theme_name, LCD_THEMES["Ice Blue (UV-K1)"])
            self._glow_provider.load_from_string(
                f".lcd-glow-active {{ box-shadow: 0 0 18px {glow}, inset 0 0 6px {glow}; }}"
            )

        def _apply_skin_classes(self) -> None:
            prof = CHASSIS_PROFILES[self.model_id]
            self.faceplate.remove_css_class("faceplate-k1")
            self.faceplate.remove_css_class("faceplate-k5")
            self.faceplate.add_css_class(f"faceplate-{self.model_id}")

            self.nameplate.remove_css_class("nameplate-k1")
            self.nameplate.remove_css_class("nameplate-k5")
            self.nameplate.add_css_class(f"nameplate-{self.model_id}")

            if self.model_id == "k5":
                self.ptt_btn.add_css_class("ptt-k5")
            else:
                self.ptt_btn.remove_css_class("ptt-k5")

            self.model_tag_lbl.set_text(prof["name"])
            self.speaker_rev.set_reveal_child(self.model_id == "k5")
            for kname, primary, _, _ in prof["nav_row"]:
                if kname in self._nav_labels:
                    self._nav_labels[kname].set_text(primary)
            self.top_cap.queue_draw()

        def _set_model(self, model_id: str) -> None:
            if model_id not in CHASSIS_PROFILES:
                return
            self.model_id = model_id
            self._apply_skin_classes()
            if self.link.connected and self.link.demo_mode:
                self.link.volume_name = f"{CHASSIS_PROFILES[model_id]['title']} DEMO"
                self.etch_device_lbl.set_text(self.link.volume_name.upper())
                if self.win_title is not None:
                    self.win_title.set_subtitle(self.link.volume_name)
                self.win.set_title(f"Kern Dock — {self.link.volume_name}")
            if not self.user_picked_theme:
                default_th = CHASSIS_PROFILES[model_id]["default_theme"]
                self.theme_name = default_th
                self._apply_theme_glow()
                if hasattr(self, "theme_dd"):
                    names = list(LCD_THEMES.keys())
                    self.theme_dd.set_selected(names.index(default_th))
                self._last_render_key = None

        def _on_theme_changed(self, dd: Any, _pspec: Any) -> None:
            names = list(LCD_THEMES.keys())
            idx = dd.get_selected()
            if 0 <= idx < len(names):
                self.theme_name = names[idx]
                self.user_picked_theme = True
                self._apply_theme_glow()
                self._last_render_key = None

        def _set_scale(self, sc: int) -> None:
            self.lcd_scale = sc
            self.lcd_area.set_content_width(LCD_W * sc)
            self.lcd_area.set_content_height(LCD_H * sc)
            self._last_render_key = None

        def _set_grid(self, enabled: bool) -> None:
            self.pixel_grid = enabled
            self._last_render_key = None

        def _refresh_ports(self, select_port: str | None = None) -> None:
            self._ports = list_available_ports()
            if select_port and all(p[0] != select_port for p in self._ports):
                self._ports.insert(0, (select_port, select_port))
            while self.port_strings.get_n_items() > 0:
                self.port_strings.remove(0)
            for _pid, lbl in self._ports:
                self.port_strings.append(lbl)
            target_idx = 0
            if select_port:
                for i, (pid, _lbl) in enumerate(self._ports):
                    if pid == select_port:
                        target_idx = i
                        break
            self.port_dd.set_selected(target_idx)

        def _selected_port(self) -> str:
            idx = self.port_dd.get_selected()
            if 0 <= idx < len(self._ports):
                return self._ports[idx][0]
            return "demo"

        def _auto_connect_once(self) -> bool:
            if not self.link.connected:
                self._on_connect_toggle(self.conn_btn)
            return False

        def _on_connect_toggle(self, _btn: Any) -> None:
            if self.link.connected:
                self.link.disconnect()
                self.conn_btn.set_label("Connect")
                self.conn_btn.remove_css_class("dock-disconnect")
                self.conn_btn.add_css_class("dock-connect")
                self.mount_btn.set_active(False)
                self.open_fs_rev.set_reveal_child(False)
                self.telem_mount.set_visible(False)
                self.led_dot.remove_css_class("led-rx")
                self.led_dot.remove_css_class("led-tx")
                self.etch_device_lbl.set_text("KERN OS")
                if self.win_title is not None:
                    self.win_title.set_subtitle("Disconnected")
                self.win.set_title("Kern Dock")
                self.telem_status.set_text("Disconnected")
                self.telem_fps.set_text("0.0 FPS")
                self.telem_batt.set_text("— V")
                self.top_cap.queue_draw()
                return

            port = self._selected_port()
            try:
                self.link.connect(port, baud=baud)
            except Exception as e:
                self._notify(f"Connection failed: {e}")
                return

            if self.link.demo_mode:
                self.link.volume_name = f"{CHASSIS_PROFILES[self.model_id]['title']} DEMO"
            self.conn_btn.set_label("Disconnect")
            self.conn_btn.remove_css_class("dock-connect")
            self.conn_btn.add_css_class("dock-disconnect")
            self.led_dot.add_css_class("led-rx")
            self.etch_device_lbl.set_text(self.link.volume_name.upper())
            if self.win_title is not None:
                self.win_title.set_subtitle(self.link.volume_name)
            self.win.set_title(f"Kern Dock — {self.link.volume_name}")

            # Auto-align chassis skin to connected live device (preserve user choice in Demo mode)
            if not self.link.demo_mode:
                if self.link.detected_model == "k5" and self.model_id != "k5":
                    self.btn_k5.set_active(True)
                elif self.link.detected_model == "k1" and self.model_id != "k1":
                    self.btn_k1.set_active(True)

            self._last_render_key = None
            self._notify(f"Connected to {self.link.volume_name}")

        def _on_mount_toggled(self, btn: Any) -> None:
            want = btn.get_active()
            if not want:
                if self.link.dav_server is not None:
                    self.link.stop_dav()
                    self.open_fs_rev.set_reveal_child(False)
                    self.telem_mount.set_visible(False)
                    self._notify("Unmounted radio filesystem")
                return
            if not self.link.connected:
                btn.set_active(False)
                self._notify("Connect to a radio (or Demo) first")
                return
            try:
                url = self.link.start_dav(http_port=8765, read_only=self.mount_read_only)
                open_target, unmount_cmd = kernfs_mount.try_auto_mount(url, self.link.volume_name)
                self.link.dav_open_target = open_target
                self.link.dav_unmount_cmd = unmount_cmd
                self.open_fs_rev.set_reveal_child(True)
                self.telem_mount.set_text(f"FS :{self.link.dav_port}")
                self.telem_mount.set_visible(True)
                self._notify(f"Mounted {self.link.volume_name} ({url})")
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

        def _on_save_screenshot(self, _btn: Any) -> None:
            dialog = Gtk.FileDialog()
            dialog.set_title("Save ST7565 LCD Screenshot")
            dialog.set_initial_name(f"kern_lcd_{time.strftime('%Y%m%d_%H%M%S')}.png")
            f_png = Gtk.FileFilter()
            f_png.set_name("PNG Images")
            f_png.add_pattern("*.png")
            filters = Gio.ListStore.new(Gtk.FileFilter)
            filters.append(f_png)
            dialog.set_filters(filters)

            def _done(dlg: Any, res: Any) -> None:
                try:
                    gfile = dlg.save_finish(res)
                    fpath = gfile.get_path()
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

            dialog.save(self.win, None, _done)

        def _on_sideload(self, _btn: Any) -> None:
            if not self.link.connected or self.link.demo_mode or not self.link.client:
                self._notify("Connect to a live radio (or QEMU) to sideload packages")
                return
            dialog = Gtk.FileDialog()
            dialog.set_title("Sideload KRX Package / Module")
            f_krx = Gtk.FileFilter()
            f_krx.set_name("KRX Packages (*.krx, *.mod, *.app)")
            for pat in ("*.krx", "*.mod", "*.app", "*.svc", "*.dmn", "*.drv"):
                f_krx.add_pattern(pat)
            filters = Gio.ListStore.new(Gtk.FileFilter)
            filters.append(f_krx)
            dialog.set_filters(filters)

            def _done(dlg: Any, res: Any) -> None:
                try:
                    gfile = dlg.open_finish(res)
                    fpath = gfile.get_path()
                    if fpath:
                        with self.link.lock:
                            info = self.link.client.sideload(fpath, verify=True, force=True)  # type: ignore[union-attr]
                        self._notify(f"Installed {info['remote_path']}")
                except GLib.Error:
                    pass
                except Exception as e:
                    self._notify(f"Sideload failed: {e}")

            dialog.open(self.win, None, _done)

        def _tick_ui(self) -> bool:
            rkey = (self.link.last_fb, self.theme_name, self.pixel_grid, self.lcd_scale)
            if rkey != self._last_render_key:
                self._rebuild_lcd_surface()
                self.lcd_area.queue_draw()
                self._last_render_key = rkey

            if self.link.connected:
                if self.link.demo_mode:
                    self.telem_status.set_text("Offline Demo")
                    self.telem_fps.set_text("DEMO")
                else:
                    err = f" · {self.link.last_error}" if self.link.last_error else ""
                    self.telem_status.set_text(f"{self.link.port}{err}")
                    self.telem_fps.set_text(f"{self.link.fps:.1f} FPS")
                t = self.link.last_telemetry
                if "battery_mv" in t:
                    pct = t.get("battery_pct", 0)
                    self.telem_batt.set_text(f"{t['battery_mv']/1000:.2f} V · {pct}%")
            return True

        def _on_close_request(self, _win: Any) -> bool:
            self.link.disconnect()
            return False

    app = KernDockApp()
    return int(app.run([]))


# =============================================================================
# 5. HEADLESS SELFTEST (Zero Display Server Required — Safe for CI)
# =============================================================================

def run_selftest() -> int:
    # 1. Verify every physical key in both UV-K1 and UV-K5 v3 chassis maps to a valid RigWire key
    for pid, prof in CHASSIS_PROFILES.items():
        all_keys = [k[0] for k in prof["nav_row"]] + [k[0] for row in KEYPAD_ROWS_NUMERIC for k in row]
        assert len(all_keys) == 16, f"Profile {pid} must define exact 4x4 (16-key) matrix"
        for kname in all_keys:
            assert kname in rigwire.KEY_CODES, f"Unknown RigWire key '{kname}' in profile {pid}"

    # 2. Verify ST7565 rasterizers (RGB24 + Cairo BGRX) across all 5 LCD themes + pure-stdlib PNG
    fb = make_demo_framebuffer("433.500", "145.500", "TEST")
    assert len(fb) == 1024
    for theme_name in LCD_THEMES:
        w, h, rgb = render_st7565_rgb(fb, scale=3, theme_name=theme_name, pixel_grid=True)
        assert w == 384 and h == 192 and len(rgb) == 384 * 192 * 3
        w2, h2, bgra = render_st7565_bgra(fb, scale=3, theme_name=theme_name, pixel_grid=True)
        assert w2 == 384 and h2 == 192 and len(bgra) == 384 * 192 * 4
        assert rgb_to_ppm(w, h, rgb).startswith(b"P6\n384 192\n255\n")
    w4, h4, rgb4 = render_st7565_rgb(fb, scale=4, theme_name="Ice Blue (UV-K1)", pixel_grid=True)
    png = encode_png_rgb(w4, h4, rgb4)
    assert png.startswith(b"\x89PNG\r\n\x1a\n") and png.endswith(b"IEND\xaeB`\x82")

    # 3. Verify DockLink demo mode, key injection, flashlight, and WebDAV volume identity propagation
    link = DockLink()
    link.connect("demo")
    assert link.connected and link.demo_mode and link.volume_name == "Quansheng UV-K1 DEMO"
    link.send_key("up", "press")
    assert link.demo_freq_khz == 433525
    link.toggle_flashlight()
    assert link.torch_on
    url = link.start_dav(http_port=0, read_only=True)
    assert url.startswith("http://127.0.0.1:")
    conn = http.client.HTTPConnection("127.0.0.1", link.dav_port, timeout=5)
    try:
        conn.request("PROPFIND", "/", headers={"Depth": "1"})
        resp = conn.getresponse()
        xml_body = resp.read()
        assert resp.status == 207 and b"Quansheng UV-K1 DEMO" in xml_body
    finally:
        conn.close()
    link.disconnect()
    assert not link.connected and link.dav_server is None

    kernfs_mount.log_ok(
        "All Kern Dock headless selftests passed (UV-K1/UV-K5 4x4 matrices, ST7565 RGB24+BGRX 5-theme rasterizer, PNG, DockLink & WebDAV volume identity)."
    )
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description="Kern Dock — Modern GTK4 + Libadwaita radio companion")
    ap.add_argument("-p", "--port", help="Serial port (/dev/ttyACM0, COM3, tcp://127.0.0.1:4446, or 'demo')")
    ap.add_argument("-b", "--baud", type=int, default=115200, help="Serial baud rate (default: 115200)")
    ap.add_argument("--model", default="k1", choices=["k1", "k5"], help="Initial radio chassis skin (k1 or k5, default: k1)")
    ap.add_argument("--scale", type=int, default=3, choices=[2, 3, 4], help="LCD integer pixel scale (default: 3)")
    ap.add_argument("--theme", choices=list(LCD_THEMES.keys()), help="Override initial LCD backlight theme")
    ap.add_argument("--demo", action="store_true", help="Launch immediately in offline demo preview mode")
    ap.add_argument("--selftest", action="store_true", help="Run headless rasterizer/PNG/link selftest")
    args = ap.parse_args()

    if args.selftest:
        return run_selftest()

    port = "demo" if args.demo else args.port
    kernfs_mount.print_header("Kern Dock — GTK4 Radio Companion")
    kernfs_mount.log_info(f"Launching GTK4 UI (model={args.model.upper()}, scale={args.scale}x, port={port or 'auto'})")
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
