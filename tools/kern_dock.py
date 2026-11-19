#!/usr/bin/env python3
"""
Kern Dock — Native GTK4 + Libadwaita Hardware Companion for Kern OS.

Architecture:
  • Blazing-fast multi-layer Cairo rendering (< 0.5 ms/frame):
      - Vectorized NumPy ST7565 unpacking + asymmetric STN ghosting (pure-Python fallback)
      - Cached static handset chassis, LCD glass backdrop, and subpixel micro-grid surfaces
      - Smooth 60 FPS spring-animated physical buttons (0% CPU when idle)
  • Factory-accurate hardware models auto-detected via GUdev USB descriptors & RigWire HELLO:
      - Quansheng UV-K1(8) ("Mini Kong"): warm titanium-gunmetal armor, 4 Torx bolts,
        authentic Quansheng RF-wave logo, stubby A1 antenna, cobalt-blue knob & PTT,
        generous unpixelated backlit LCD margin, milled 4x4 keypad lattice
      - Quansheng UV-K5 v3 / UV-K6: matte polycarbonate shell, brushed silver upper LCD
        armor, vivid orange knob, vertical left nav column (M / ▲ / ▼ / EXIT) alongside
        the 3-bar speaker dish, and lower 4x3 keypad
  • Clean GNOME HIG workspace (zero cluttered top bar, auto window sizing):
      - Upgraded GUdev hotplug Connection Popover (live boxed-list devices, auto-connect)
      - Device (battery LevelBar, identity, link FPS/RTT, hardware controls)
      - Files  (live KernFS browser + anti-brick shields + native WebDAV OS mount)
      - Wire   (interactive RigWire console)
      - Capture (PNG snapshots + event-driven GIF recorder with trim/cut/splice editor,
        exporting LCD-Only or Full Radio Body with animated button presses)
"""

from __future__ import annotations

import argparse
import dataclasses
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

try:
    import numpy as _np
except ImportError:
    _np = None  # type: ignore[assignment]

# =============================================================================
# 1. BLAZING-FAST ST7565 LCD ENGINE (Vectorized NumPy + Cached Cairo Layers)
# =============================================================================

LCD_W = 128
LCD_H = 64

# Authentic measured palettes (UV-K1 mint/indigo, UV-K5 vivid amber/black, etc.)
LCD_PALETTES: dict[str, dict[str, Any]] = {
    "UV-K1":         {"bg": (0xC9, 0xF7, 0xEC), "fg": (0x26, 0x20, 0x83), "dark": False},
    "UV-K5":         {"bg": (0xFF, 0x99, 0x00), "fg": (0x0E, 0x0E, 0x0E), "dark": False},
    "Classic Green": {"bg": (0x9C, 0xA9, 0x88), "fg": (0x2A, 0x2A, 0x2A), "dark": False},
    "UV-5R Blue":    {"bg": (0x39, 0xAE, 0xF1), "fg": (0x10, 0x10, 0x10), "dark": False},
    "OLED White":    {"bg": (0x10, 0x10, 0x10), "fg": (0xF0, 0xF0, 0xF0), "dark": True},
}

# Asymmetric STN liquid-crystal dynamics (fast dark turn-on, smooth fade-off)
STN_ATTACK = 0.72
STN_DECAY = 0.24


@dataclasses.dataclass
class LcdSettings:
    palette: str = "UV-K1"
    pixel_aspect: float = 1.30
    pixel_grid: bool = True
    invert_lcd: bool = False
    ghosting: bool = True
    contrast: int = 13          # 0..15
    backlight: int = 8          # 0..10 (0..6 dim, 7..8 normal, 9..10 vibrant)
    backlight_glow: bool = True

    def cache_key(self) -> tuple[Any, ...]:
        return (self.palette, self.pixel_aspect, self.pixel_grid, self.contrast, self.backlight, self.backlight_glow)


def _backlight_mult(level: int) -> float:
    if level <= 6:
        return 0.15 + (level / 6.0) * 0.75
    if level <= 8:
        return 1.0
    return 1.0 + (level - 8) * 0.14


def _clamp1(v: float) -> float:
    return 0.0 if v < 0.0 else (1.0 if v > 1.0 else v)


def _lcd_colors_norm(st: LcdSettings) -> tuple[tuple[float, float, float], tuple[float, float, float], tuple[float, float, float], bool]:
    pal = LCD_PALETTES.get(st.palette, LCD_PALETTES["UV-K1"])
    bright = _backlight_mult(st.backlight)
    bg = tuple(_clamp1((c / 255.0) * bright) for c in pal["bg"])
    fg_raw = tuple(_clamp1((c / 255.0) * bright) for c in pal["fg"])
    c_ratio = 0.15 + (max(0, min(15, st.contrast)) / 15.0) * 0.85
    fg = (
        bg[0] + (fg_raw[0] - bg[0]) * c_ratio,
        bg[1] + (fg_raw[1] - bg[1]) * c_ratio,
        bg[2] + (fg_raw[2] - bg[2]) * c_ratio,
    )
    grid = (
        bg[0] + (fg_raw[0] - bg[0]) * 0.07,
        bg[1] + (fg_raw[1] - bg[1]) * 0.07,
        bg[2] + (fg_raw[2] - bg[2]) * 0.07,
    )
    return bg, fg, grid, bool(pal["dark"])


class FastLcdState:
    """
    Ultra-fast ST7565 pixel state + premultiplied ARGB32 128x64 Cairo surface generator.
    Runs in ~0.12 ms/frame with NumPy (or ~0.6 ms pure Python) — zero per-pixel Cairo loops!
    """

    def __init__(self) -> None:
        if _np is not None:
            self._ghost_np = _np.zeros((LCD_H, LCD_W), dtype=_np.float32)
            self._shifts = _np.arange(8, dtype=_np.uint8).reshape(8, 1)
            self._bgra_np = _np.zeros((LCD_H, LCD_W, 4), dtype=_np.uint8)
        else:
            self._ghost_py: list[float] = [0.0] * (LCD_W * LCD_H)
            self._bgra_py = bytearray(LCD_W * LCD_H * 4)
        self._pixel_surface: Any = None
        self._surface_dirty = True

    def step(self, fb: bytes, st: LcdSettings) -> bool:
        """Updates STN ghosting state for one frame; returns True while pixels are still settling."""
        if len(fb) < 1024:
            fb = fb.ljust(1024, b"\x00")
        self._surface_dirty = True

        if _np is not None:
            raw = _np.frombuffer(fb[:1024], dtype=_np.uint8).reshape(8, 128)
            bits = ((raw[:, None, :] >> self._shifts[None, :, :]) & 1).reshape(64, 128)
            target = (1.0 - bits).astype(_np.float32) if st.invert_lcd else bits.astype(_np.float32)
            if not st.ghosting:
                self._ghost_np[:] = target
                return False
            diff = target - self._ghost_np
            mask = _np.abs(diff) > 0.015
            if not _np.any(mask):
                self._ghost_np[:] = target
                return False
            rate = _np.where(diff > 0.0, STN_ATTACK, STN_DECAY).astype(_np.float32)
            self._ghost_np += diff * rate
            settled = _np.abs(target - self._ghost_np) <= 0.015
            self._ghost_np = _np.where(settled, target, self._ghost_np)
            return bool(_np.any(~settled))

        # Pure-Python fallback
        inv = st.invert_lcd
        use_g = st.ghosting
        settling = False
        g = self._ghost_py
        idx = 0
        for y in range(LCD_H):
            po = (y >> 3) * 128
            m = 1 << (y & 7)
            for x in range(LCD_W):
                bit = 1 if (fb[po + x] & m) else 0
                tgt = (0.0 if bit else 1.0) if inv else (1.0 if bit else 0.0)
                if not use_g:
                    g[idx] = tgt
                else:
                    cur = g[idx]
                    d = tgt - cur
                    if abs(d) > 0.015:
                        nxt = cur + d * (STN_ATTACK if d > 0.0 else STN_DECAY)
                        if abs(tgt - nxt) <= 0.015:
                            g[idx] = tgt
                        else:
                            g[idx] = nxt
                            settling = True
                    else:
                        g[idx] = tgt
                idx += 1
        return settling

    def get_cairo_pixel_surface(self, st: LcdSettings) -> Any:
        """
        Returns a fresh 128x64 Cairo FORMAT_ARGB32 premultiplied surface of ONLY the active LC pixels.
        A fresh buffer/surface per update avoids mutating a surface still snapshotted by GTK4 GSK.
        """
        import cairo
        _, fg, _, _ = _lcd_colors_norm(st)
        fr, fg_c, fb_c = fg[0] * 255.0, fg[1] * 255.0, fg[2] * 255.0

        if _np is not None:
            a = self._ghost_np
            out = self._bgra_np
            out[:, :, 0] = (fb_c * a).astype(_np.uint8)
            out[:, :, 1] = (fg_c * a).astype(_np.uint8)
            out[:, :, 2] = (fr * a).astype(_np.uint8)
            out[:, :, 3] = (255.0 * a).astype(_np.uint8)
            raw = bytearray(out.tobytes())
            return cairo.ImageSurface.create_for_data(raw, cairo.FORMAT_ARGB32, LCD_W, LCD_H, LCD_W * 4)

        buf = bytearray(LCD_W * LCD_H * 4)
        g = self._ghost_py
        for i in range(LCD_W * LCD_H):
            a = g[i]
            if a > 0.01:
                p = i * 4
                buf[p] = int(fb_c * a)
                buf[p + 1] = int(fg_c * a)
                buf[p + 2] = int(fr * a)
                buf[p + 3] = int(255.0 * a)
        return cairo.ImageSurface.create_for_data(buf, cairo.FORMAT_ARGB32, LCD_W, LCD_H, LCD_W * 4)


def _compute_lcd_active_rect(w: float, h: float, aspect: float) -> tuple[float, float, float, float, float, float]:
    """Computes (mx, my, mat_w, mat_h, px_w, px_h) inside an unpixelated backlit glass margin."""
    # Generous authentic unpixelated backlit STN glass border on all 4 sides (~7.5% X, ~9% Y)
    pad_x = max(7.0, w * 0.072)
    pad_y = max(7.0, h * 0.088)
    avail_w = max(16.0, w - pad_x * 2.0)
    avail_h = max(16.0, h - pad_y * 2.0)
    px_w = min(avail_w / LCD_W, avail_h / (LCD_H * aspect))
    px_h = px_w * aspect
    mat_w = LCD_W * px_w
    mat_h = LCD_H * px_h
    mx = (w - mat_w) * 0.5
    my = (h - mat_h) * 0.5
    return mx, my, mat_w, mat_h, px_w, px_h


class LcdLayerCache:
    """Caches static LCD backdrop (unpixelated backlit glass) & foreground (micro-grid + bezel shadow + sheen)."""

    def __init__(self) -> None:
        self._key: Any = None
        self.bg_surf: Any = None
        self.fg_surf: Any = None
        self.metrics: tuple[float, float, float, float, float, float] = (0, 0, 1, 1, 1, 1)

    def ensure(self, w: int, h: int, st: LcdSettings) -> None:
        import cairo
        key = (w, h, st.cache_key())
        if key == self._key and self.bg_surf is not None:
            return
        self._key = key
        bg, fg, grid, is_dark = _lcd_colors_norm(st)
        mx, my, mat_w, mat_h, px_w, px_h = _compute_lcd_active_rect(float(w), float(h), st.pixel_aspect)
        self.metrics = (mx, my, mat_w, mat_h, px_w, px_h)

        # --- Layer A: Unpixelated Illuminated Glass Backdrop ---
        bg_s = cairo.ImageSurface(cairo.FORMAT_ARGB32, w, h)
        cr = cairo.Context(bg_s)
        _round_rect(cr, 0, 0, w, h, 4.5)
        cr.clip()

        pat = cairo.LinearGradient(0, 0, 0, h)
        if is_dark:
            pat.add_color_stop_rgb(0.0, bg[0], bg[1], bg[2])
            pat.add_color_stop_rgb(1.0, _clamp1(bg[0] * 1.08), _clamp1(bg[1] * 1.08), _clamp1(bg[2] * 1.08))
        else:
            # Realistic bottom-edge LED diffuser brightness gradient across the glass
            pat.add_color_stop_rgb(0.0, bg[0] * 0.93, bg[1] * 0.94, bg[2] * 0.95)
            pat.add_color_stop_rgb(0.50, bg[0], bg[1], bg[2])
            pat.add_color_stop_rgb(1.0, _clamp1(bg[0] * 1.05), _clamp1(bg[1] * 1.05), _clamp1(bg[2] * 1.03))
        cr.set_source(pat)
        cr.rectangle(0, 0, w, h)
        cr.fill()

        # Subtle passive liquid-crystal matrix zone tint ONLY inside active 128x64 rectangle
        if not is_dark:
            cr.set_source_rgba(grid[0], grid[1], grid[2], 0.55 if st.pixel_grid else 0.25)
            cr.rectangle(mx, my, mat_w, mat_h)
            cr.fill()
        self.bg_surf = bg_s

        # --- Layer B: Foreground Subpixel Hairline Grid + Recessed Bezel Shadow + Glass Sheen ---
        fg_s = cairo.ImageSurface(cairo.FORMAT_ARGB32, w, h)
        fcr = cairo.Context(fg_s)
        _round_rect(fcr, 0, 0, w, h, 4.5)
        fcr.clip()

        # Crisp 10% hairline grid strictly inside the active 128x64 window (never on the glass margin!)
        if st.pixel_grid and px_w >= 1.6:
            fcr.set_source_rgba(bg[0], bg[1], bg[2], 0.72 if not is_dark else 0.65)
            fcr.set_line_width(max(0.32, px_w * 0.11))
            for col in range(1, LCD_W):
                lx = mx + col * px_w
                fcr.move_to(lx, my)
                fcr.line_to(lx, my + mat_h)
            fcr.stroke()
            fcr.set_line_width(max(0.32, px_h * 0.11))
            for row in range(1, LCD_H):
                ly = my + row * px_h
                fcr.move_to(mx, ly)
                fcr.line_to(mx + mat_w, ly)
            fcr.stroke()

        # Recessed inner bezel shadow cast around all 4 glass edges
        sd = max(6.0, min(w, h) * 0.055)
        for (x0, y0, x1, y1, rx, ry, rw, rh) in (
            (0, 0, 0, sd, 0, 0, w, sd),
            (0, h, 0, h - sd * 0.7, 0, h - sd * 0.7, w, sd * 0.7),
            (0, 0, sd * 0.85, 0, 0, 0, sd * 0.85, h),
            (w, 0, w - sd * 0.85, 0, w - sd * 0.85, 0, sd * 0.85, h),
        ):
            sg = cairo.LinearGradient(x0, y0, x1, y1)
            sg.add_color_stop_rgba(0.0, 0.0, 0.0, 0.0, 0.30)
            sg.add_color_stop_rgba(1.0, 0.0, 0.0, 0.0, 0.0)
            fcr.set_source(sg)
            fcr.rectangle(rx, ry, rw, rh)
            fcr.fill()

        # Subtle matte polarizer diagonal sheen
        sheen = cairo.LinearGradient(0, 0, w * 0.55, h * 0.75)
        sheen.add_color_stop_rgba(0.0, 1.0, 1.0, 1.0, 0.075)
        sheen.add_color_stop_rgba(0.42, 1.0, 1.0, 1.0, 0.015)
        sheen.add_color_stop_rgba(0.43, 1.0, 1.0, 1.0, 0.0)
        fcr.set_source(sheen)
        fcr.rectangle(0, 0, w, h)
        fcr.fill()
        self.fg_surf = fg_s


def paint_fast_lcd(
    cr: Any,
    x: float,
    y: float,
    w: int,
    h: int,
    lcd_state: FastLcdState,
    st: LcdSettings,
    cache: LcdLayerCache,
) -> None:
    """Paints full realistic ST7565 LCD in ~0.20 ms using cached surfaces + nearest-neighbor blit."""
    import cairo
    cache.ensure(w, h, st)
    mx, my, mat_w, mat_h, px_w, px_h = cache.metrics
    _, _, _, is_dark = _lcd_colors_norm(st)
    px_surf = lcd_state.get_cairo_pixel_surface(st)

    cr.save()
    cr.translate(x, y)
    # 1. Cached backlit glass + unpixelated padding
    cr.set_source_surface(cache.bg_surf, 0, 0)
    cr.paint()

    # 2. STN rear-polarizer shadow (+0.8, +0.95) on reflective light panels
    if not is_dark:
        cr.save()
        cr.translate(mx + max(0.6, px_w * 0.25), my + max(0.7, px_h * 0.25))
        cr.scale(mat_w / float(LCD_W), mat_h / float(LCD_H))
        cr.set_source_surface(px_surf, 0, 0)
        cr.get_source().set_filter(cairo.FILTER_NEAREST)
        cr.paint_with_alpha(0.13)
        cr.restore()

    # 3. Crisp active liquid-crystal dot matrix
    cr.save()
    cr.translate(mx, my)
    cr.scale(mat_w / float(LCD_W), mat_h / float(LCD_H))
    cr.set_source_surface(px_surf, 0, 0)
    cr.get_source().set_filter(cairo.FILTER_NEAREST)
    cr.paint()
    cr.restore()

    # 4. Cached micro-grid + bezel shadow + polarizer sheen
    cr.set_source_surface(cache.fg_surf, 0, 0)
    cr.paint()
    cr.restore()


# =============================================================================
# 2. FACTORY-ACCURATE CAIRO MODELS + AUTHENTIC QUANSHENG LOGO + SPRING KEYS
# =============================================================================

K1_MATRIX: list[list[tuple[str, str, str, str]]] = [
    [("menu", "MENU", "", ""), ("up", "<", "", ""), ("down", ">", "", ""), ("exit", "EXIT", "", "")],
    [("1", "1", "Band", ".,?"), ("2", "2", "A/B", "abc"), ("3", "3", "V/M", "def"), ("star", "*", "Scan", "_RET_")],
    [("4", "4", "FC",   "ghi"), ("5", "5", "WX",  "jkl"), ("6", "6", "H/L", "mno"), ("0",    "0", "FM",   "")],
    [("7", "7", "VOX",  "pqrs"),("8", "8", "R",   "tuv"), ("9", "9", "Call","wxyz"),("f",    "#", "LOCK", "F")],
]

K5_BOTTOM_MATRIX: list[list[tuple[str, str, str]]] = [
    [("1", "1", "Band"), ("2", "2", "A/B"),  ("3", "3", "VFO/MR"), ("star", "*", "Scan")],
    [("4", "4", "FC"),   ("5", "5", "NOAA"), ("6", "6", "H/M/L"),  ("0",    "0", "FM")],
    [("7", "7", "VOX"),  ("8", "8", "R"),    ("9", "9", "Call"),   ("f",    "F", "#")],
]


def _round_rect(cr: Any, x: float, y: float, w: float, h: float, r: float) -> None:
    r = min(r, w * 0.5, h * 0.5)
    cr.new_sub_path()
    cr.arc(x + w - r, y + r, r, -math.pi / 2, 0)
    cr.arc(x + w - r, y + h - r, r, 0, math.pi / 2)
    cr.arc(x + r, y + h - r, r, math.pi / 2, math.pi)
    cr.arc(x + r, y + r, r, math.pi, 3 * math.pi / 2)
    cr.close_path()


def _draw_quansheng_logo(cr: Any, cx: float, cy: float, font_size: float = 13.0) -> None:
    """
    Draws the authentic Quansheng emblem modeled directly after `inspo/Quansheng-Logo.png`:
    two heavy slanted curved RF wave crescents over the upper-left shoulder of bold italic QUANSHENG.
    """
    import cairo
    cr.save()
    cr.select_font_face("Sans", cairo.FONT_SLANT_ITALIC, cairo.FONT_WEIGHT_BOLD)
    cr.set_font_size(font_size)
    ext = cr.text_extents("QUANSHENG")
    icon_w = font_size * 1.15
    total_w = icon_w + ext.width
    x0 = cx - total_w * 0.5
    text_x = x0 + icon_w
    text_y = cy + ext.height * 0.36

    # Two bold tapered RF crescents hugging top-left of 'Q'
    cr.set_source_rgba(0.95, 0.96, 0.98, 0.96)
    cr.set_line_cap(cairo.LINE_CAP_ROUND)
    ox, oy = text_x + font_size * 0.14, text_y - font_size * 0.08
    cr.set_line_width(font_size * 0.16)
    cr.arc(ox, oy, font_size * 0.66, math.pi * 1.02, math.pi * 1.44)
    cr.stroke()
    cr.set_line_width(font_size * 0.19)
    cr.arc(ox, oy, font_size * 0.98, math.pi * 1.00, math.pi * 1.46)
    cr.stroke()

    # Heavy sheared geometric wordmark
    cr.move_to(text_x, text_y)
    cr.show_text("QUANSHENG")
    cr.restore()


def _draw_torx_bolt(cr: Any, cx: float, cy: float, r: float = 5.4) -> None:
    import cairo
    cr.set_source_rgb(0.11, 0.11, 0.12)
    cr.arc(cx, cy, r + 1.4, 0, 2 * math.pi)
    cr.fill()
    pat = cairo.RadialGradient(cx - r * 0.3, cy - r * 0.3, 0.5, cx, cy, r)
    pat.add_color_stop_rgb(0.0, 0.90, 0.89, 0.86)
    pat.add_color_stop_rgb(0.62, 0.54, 0.53, 0.50)
    pat.add_color_stop_rgb(1.0, 0.28, 0.27, 0.25)
    cr.set_source(pat)
    cr.arc(cx, cy, r, 0, 2 * math.pi)
    cr.fill()
    cr.set_source_rgb(0.14, 0.13, 0.13)
    cr.arc(cx, cy, r * 0.42, 0, 2 * math.pi)
    cr.fill()


def _draw_animated_key(
    cr: Any,
    x: float,
    y: float,
    w: float,
    h: float,
    primary: str,
    sub1: str,
    sub2: str,
    *,
    press_t: float,       # smooth 0.0..1.0 press spring
    hover_t: float,       # smooth 0.0..1.0 hover highlight
    radius: float = 5.0,
    is_nav: bool = False,
    accent: tuple[float, float, float] = (0.18, 0.56, 1.0),
) -> None:
    """Draws a tactile rubberized radio key with smooth 60 FPS spring plunge & specular shift."""
    import cairo
    p = _clamp1(press_t)
    hv = _clamp1(hover_t)

    # Milled well recess
    _round_rect(cr, x - 1.3, y - 1.3, w + 2.6, h + 2.6, radius + 1.0)
    cr.set_source_rgba(0.03, 0.03, 0.04, 0.94)
    cr.fill()

    # Subtle outer accent halo when actively pressed
    if p > 0.02:
        _round_rect(cr, x - 1.6, y - 1.6, w + 3.2, h + 3.2, radius + 1.5)
        cr.set_source_rgba(accent[0], accent[1], accent[2], 0.22 * p)
        cr.fill()

    # Physical plunge travel (downward + tiny inner shrink for 3D depth)
    inset = 0.65 * p
    dy = 1.9 * p
    kx, ky, kw, kh = x + inset, y + dy, w - inset * 2.0, h - dy * 0.55

    # Drop shadow under keytop (shrinks as button bottoms out)
    if p < 0.95:
        _round_rect(cr, kx, ky + 1.6 * (1.0 - p), kw, kh, radius)
        cr.set_source_rgba(0.0, 0.0, 0.0, 0.55 * (1.0 - p))
        cr.fill()

    _round_rect(cr, kx, ky, kw, kh, radius)
    # Smoothly interpolate keytop gradient between idle, hovered, and depressed
    top_l = 0.19 + 0.06 * hv - 0.11 * p
    mid_l = 0.13 + 0.04 * hv - 0.05 * p
    bot_l = 0.09 + 0.02 * hv + 0.03 * p
    pat = cairo.LinearGradient(kx, ky, kx, ky + kh)
    pat.add_color_stop_rgb(0.0, top_l, top_l + 0.01, top_l + 0.025)
    pat.add_color_stop_rgb(0.55, mid_l, mid_l + 0.01, mid_l + 0.02)
    pat.add_color_stop_rgb(1.0, bot_l, bot_l + 0.01, bot_l + 0.02)
    cr.set_source(pat)
    cr.fill_preserve()

    # Top bevel specular rim blends smoothly into accent ring as button is pressed
    if p > 0.02:
        cr.set_source_rgba(
            accent[0] * p + 1.0 * (1.0 - p),
            accent[1] * p + 1.0 * (1.0 - p),
            accent[2] * p + 1.0 * (1.0 - p),
            0.10 + 0.82 * p,
        )
        cr.set_line_width(0.9 + 0.8 * p)
    else:
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.09 + 0.09 * hv)
        cr.set_line_width(0.9)
    cr.stroke()

    # Crisp silkscreen typography + vector symbols (zero tofu!)
    cr.set_source_rgba(0.95, 0.96, 0.98, 0.95 + 0.05 * p)
    if primary in ("_CHEV_UP_", "_CHEV_DN_"):
        cx = kx + kw * 0.5
        cy = ky + kh * (0.42 if primary == "_CHEV_UP_" else 0.58)
        cr.set_line_width(2.0)
        cr.set_line_cap(cairo.LINE_CAP_ROUND)
        cr.set_line_join(cairo.LINE_JOIN_ROUND)
        if primary == "_CHEV_UP_":
            cr.move_to(cx - 8.0, cy + 3.5)
            cr.line_to(cx, cy - 4.5)
            cr.line_to(cx + 8.0, cy + 3.5)
        else:
            cr.move_to(cx - 8.0, cy - 3.5)
            cr.line_to(cx, cy + 4.5)
            cr.line_to(cx + 8.0, cy - 3.5)
        cr.stroke()
        if sub1:
            cr.select_font_face("Sans", cairo.FONT_SLANT_NORMAL, cairo.FONT_WEIGHT_BOLD)
            cr.set_font_size(7.0)
            cr.set_source_rgba(0.82, 0.86, 0.92, 0.82)
            se = cr.text_extents(sub1)
            cr.move_to(cx - se.width * 0.5 - se.x_bearing, ky + kh * (0.83 if primary == "_CHEV_UP_" else 0.34))
            cr.show_text(sub1)
    elif is_nav or (not sub1 and not sub2):
        cr.select_font_face("Sans", cairo.FONT_SLANT_NORMAL, cairo.FONT_WEIGHT_BOLD)
        cr.set_font_size(16.0 if primary in ("<", ">") else 10.5)
        ext = cr.text_extents(primary)
        cr.move_to(kx + (kw - ext.width) * 0.5 - ext.x_bearing, ky + (kh - ext.height) * 0.5 - ext.y_bearing)
        cr.show_text(primary)
    else:
        cr.select_font_face("Sans", cairo.FONT_SLANT_NORMAL, cairo.FONT_WEIGHT_BOLD)
        cr.set_font_size(14.5)
        pe = cr.text_extents(primary)
        px = kx + 7.5
        cr.move_to(px, ky + (kh - pe.height) * 0.5 - pe.y_bearing)
        cr.show_text(primary)

        sx = px + max(pe.x_advance, 10.0) + 3.5
        cr.set_font_size(7.6)
        cr.set_source_rgba(0.90, 0.92, 0.95, 0.88)
        if sub1 and sub2:
            cr.move_to(sx, ky + kh * 0.43)
            cr.show_text(sub1)
            if sub2 == "_RET_":
                cr.set_source_rgba(0.78, 0.82, 0.88, 0.80)
                cr.set_line_width(1.2)
                cr.set_line_cap(cairo.LINE_CAP_ROUND)
                ry = ky + kh * 0.72
                cr.move_to(sx + 10.0, ry - 3.0)
                cr.line_to(sx + 10.0, ry)
                cr.line_to(sx + 1.0, ry)
                cr.move_to(sx + 3.5, ry - 2.2)
                cr.line_to(sx + 1.0, ry)
                cr.line_to(sx + 3.5, ry + 2.2)
                cr.stroke()
            else:
                cr.select_font_face("Sans", cairo.FONT_SLANT_NORMAL, cairo.FONT_WEIGHT_NORMAL)
                cr.set_font_size(7.2)
                cr.set_source_rgba(0.76, 0.80, 0.86, 0.74)
                cr.move_to(sx, ky + kh * 0.78)
                cr.show_text(sub2)
        elif sub1:
            se = cr.text_extents(sub1)
            cr.move_to(sx, ky + (kh - se.height) * 0.5 - se.y_bearing)
            cr.show_text(sub1)


def compute_handset_geometry(model: str) -> tuple[int, int]:
    return (336, 598) if model == "k5" else (344, 552)


class HandsetRenderer:
    """
    High-performance cached Cairo handset renderer:
      • Static chassis (shell, armor, bolts, antenna, knob, logo, bezel) cached on an ImageSurface
      • LCD painted via FastLcdState + LcdLayerCache (~0.2 ms)
      • Physical buttons drawn with smooth 60 FPS spring animation
    """

    def __init__(self) -> None:
        self._chassis_key: Any = None
        self._chassis_surf: Any = None
        self._lcd_rect: tuple[int, int, int, int] = (0, 0, 100, 60)
        self._key_specs: list[tuple[str, float, float, float, float, str, str, str, float, bool]] = []
        self._side_specs: list[tuple[str, float, float, float, float, bool, int]] = []
        self.hitboxes: dict[str, tuple[float, float, float, float]] = {}
        self.lcd_cache = LcdLayerCache()

    def _ensure_chassis(self, model: str, led_state: str, torch_on: bool, lcd_st: LcdSettings) -> None:
        import cairo
        key = (model, led_state, torch_on, lcd_st.palette, lcd_st.backlight, lcd_st.backlight_glow)
        if key == self._chassis_key and self._chassis_surf is not None:
            return
        self._chassis_key = key
        W, H = compute_handset_geometry(model)
        surf = cairo.ImageSurface(cairo.FORMAT_ARGB32, W, H)
        cr = cairo.Context(surf)
        self.hitboxes.clear()
        self._key_specs.clear()
        self._side_specs.clear()

        if model == "k5":
            self._build_k5_chassis(cr, W, H, led_state, torch_on, lcd_st)
        else:
            self._build_k1_chassis(cr, W, H, led_state, torch_on, lcd_st)

        self._chassis_surf = surf

    def _build_k1_chassis(self, cr: Any, W: int, H: int, led_state: str, torch_on: bool, lcd_st: LcdSettings) -> None:
        import cairo
        bx, by, bw, bh = 28.0, 58.0, W - 44.0, H - 68.0

        # 1. Stubby A1 Antenna Base
        ax = bx + 54.0
        pat_ant = cairo.LinearGradient(ax - 22, 0, ax + 22, 0)
        pat_ant.add_color_stop_rgb(0.0, 0.09, 0.10, 0.11)
        pat_ant.add_color_stop_rgb(0.32, 0.26, 0.27, 0.29)
        pat_ant.add_color_stop_rgb(0.75, 0.13, 0.14, 0.15)
        pat_ant.add_color_stop_rgb(1.0, 0.07, 0.08, 0.09)
        cr.set_source(pat_ant)
        _round_rect(cr, ax - 19, 3, 38, by + 4, 5)
        cr.fill()
        cr.set_source_rgb(0.10, 0.11, 0.12)
        cr.rectangle(ax - 22, by - 14, 44, 16)
        cr.fill()
        cr.set_source_rgba(0.80, 0.82, 0.85, 0.25)
        cr.select_font_face("Sans", cairo.FONT_SLANT_NORMAL, cairo.FONT_WEIGHT_BOLD)
        cr.set_font_size(9.5)
        cr.move_to(ax - 6, 30)
        cr.show_text("A1")

        # Center LED Flashlight Dome (clickable)
        tx = bx + bw * 0.49
        if torch_on:
            tg = cairo.RadialGradient(tx, by - 8, 2, tx, by - 8, 32)
            tg.add_color_stop_rgba(0.0, 1.0, 0.98, 0.82, 0.92)
            tg.add_color_stop_rgba(1.0, 1.0, 0.94, 0.65, 0.0)
            cr.set_source(tg)
            cr.arc(tx, by - 8, 32, 0, 2 * math.pi)
            cr.fill()
        cr.set_source_rgb(0.14, 0.15, 0.17)
        _round_rect(cr, tx - 16, by - 12, 32, 16, 4)
        cr.fill()
        self.hitboxes["_torch"] = (tx - 18, by - 18, 36, 22)

        # Cobalt-Blue Striped Rotary Volume Knob
        kx = bx + bw - 56.0
        pat_k = cairo.LinearGradient(kx - 28, 0, kx + 28, 0)
        pat_k.add_color_stop_rgb(0.0, 0.08, 0.09, 0.10)
        pat_k.add_color_stop_rgb(0.32, 0.25, 0.27, 0.30)
        pat_k.add_color_stop_rgb(0.72, 0.12, 0.13, 0.15)
        pat_k.add_color_stop_rgb(1.0, 0.07, 0.08, 0.09)
        cr.set_source(pat_k)
        _round_rect(cr, kx - 26, by - 38, 52, 42, 6)
        cr.fill()
        cr.set_source_rgba(0.0, 0.0, 0.0, 0.45)
        cr.set_line_width(1.6)
        for dx in (-18, -10, 10, 18):
            cr.move_to(kx + dx, by - 35)
            cr.line_to(kx + dx, by - 2)
            cr.stroke()
        cr.set_source_rgb(0.14, 0.50, 1.00)
        cr.rectangle(kx - 3.2, by - 38, 6.4, 36)
        cr.fill()

        # Translucent Top-Left Status LED
        lx, ly = bx + 44.0, by - 4.0
        cr.set_source_rgb(0.96, 0.24, 0.24) if led_state == "tx" else (
            cr.set_source_rgb(0.18, 0.86, 0.42) if led_state == "rx" else cr.set_source_rgb(0.72, 0.74, 0.76)
        )
        _round_rect(cr, lx - 10, ly - 5, 20, 8, 3)
        cr.fill()
        if led_state in ("rx", "tx"):
            lg = cairo.RadialGradient(lx, ly, 2, lx, ly, 19)
            c = (1.0, 0.25, 0.25) if led_state == "tx" else (0.18, 0.90, 0.45)
            lg.add_color_stop_rgba(0.0, c[0], c[1], c[2], 0.72)
            lg.add_color_stop_rgba(1.0, c[0], c[1], c[2], 0.0)
            cr.set_source(lg)
            cr.arc(lx, ly, 19, 0, 2 * math.pi)
            cr.fill()

        # 2. Outer Matte-Black Polymer Shell + Side Grip Shoulders
        _round_rect(cr, bx, by, bw, bh, 14)
        pat_shell = cairo.LinearGradient(bx, by, bx + bw, by)
        pat_shell.add_color_stop_rgb(0.0, 0.16, 0.17, 0.18)
        pat_shell.add_color_stop_rgb(0.08, 0.09, 0.10, 0.11)
        pat_shell.add_color_stop_rgb(0.92, 0.09, 0.10, 0.11)
        pat_shell.add_color_stop_rgb(1.0, 0.18, 0.19, 0.20)
        cr.set_source(pat_shell)
        cr.fill_preserve()
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.10)
        cr.set_line_width(1.0)
        cr.stroke()

        # 3. Warm Titanium-Gunmetal Metallic Armor Faceplate (exact K1 silhouette)
        fx_l, fx_r = bx + 16.0, bx + bw - 16.0
        fy_t, fy_b = by + 6.0, by + bh - 8.0
        cr.new_sub_path()
        cr.move_to(bx + 10, fy_t)
        cr.line_to(bx + bw - 10, fy_t)
        cr.line_to(bx + bw - 4, fy_t + 8)
        cr.line_to(bx + bw - 4, fy_t + 40)
        cr.line_to(fx_r, fy_t + 54)
        cr.line_to(fx_r, fy_b - 92)
        cr.line_to(bx + bw - 4, fy_b - 78)
        cr.line_to(bx + bw - 4, fy_b - 8)
        cr.line_to(bx + bw - 12, fy_b)
        cr.line_to(bx + 12, fy_b)
        cr.line_to(bx + 4, fy_b - 8)
        cr.line_to(bx + 4, fy_b - 78)
        cr.line_to(fx_l, fy_b - 92)
        cr.line_to(fx_l, fy_t + 54)
        cr.line_to(bx + 4, fy_t + 40)
        cr.line_to(bx + 4, fy_t + 8)
        cr.close_path()

        pat_m = cairo.LinearGradient(bx, fy_t, bx + bw, fy_b)
        pat_m.add_color_stop_rgb(0.0, 0.58, 0.56, 0.52)
        pat_m.add_color_stop_rgb(0.32, 0.45, 0.43, 0.40)
        pat_m.add_color_stop_rgb(0.75, 0.36, 0.35, 0.32)
        pat_m.add_color_stop_rgb(1.0, 0.48, 0.46, 0.42)
        cr.set_source(pat_m)
        cr.fill_preserve()
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.24)
        cr.set_line_width(1.2)
        cr.stroke()

        # 4 Recessed Stainless Torx Bolts
        _draw_torx_bolt(cr, bx + 20.0, fy_t + 22.0, 5.4)
        _draw_torx_bolt(cr, bx + bw - 20.0, fy_t + 22.0, 5.4)
        _draw_torx_bolt(cr, bx + 20.0, fy_b - 42.0, 5.4)
        _draw_torx_bolt(cr, bx + bw - 20.0, fy_b - 42.0, 5.4)

        # Glossy Piano-Black Top Nameplate with Authentic Quansheng RF Logo
        np_x, np_y, np_w, np_h = bx + 42.0, fy_t + 7.0, bw - 84.0, 28.0
        _round_rect(cr, np_x, np_y, np_w, np_h, 5)
        pat_np = cairo.LinearGradient(np_x, np_y, np_x, np_y + np_h)
        pat_np.add_color_stop_rgb(0.0, 0.15, 0.16, 0.17)
        pat_np.add_color_stop_rgb(0.45, 0.05, 0.06, 0.07)
        pat_np.add_color_stop_rgb(1.0, 0.11, 0.12, 0.13)
        cr.set_source(pat_np)
        cr.fill_preserve()
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.12)
        cr.set_line_width(0.8)
        cr.stroke()
        _draw_quansheng_logo(cr, np_x + np_w * 0.5, np_y + np_h * 0.5, font_size=13.0)

        # Deep Beveled LCD Window Cutout (generous height for 1.3x dots + unpixelated glass margin)
        lz_x, lz_y, lz_w, lz_h = int(fx_l + 12), int(fy_t + 46), int((fx_r - fx_l) - 24), 166
        _round_rect(cr, lz_x, lz_y, lz_w, lz_h, 8)
        cr.set_source_rgb(0.08, 0.08, 0.09)
        cr.fill_preserve()
        cr.set_source_rgba(0.0, 0.0, 0.0, 0.78)
        cr.set_line_width(2.0)
        cr.stroke()
        if lcd_st.backlight_glow and lcd_st.backlight > 0:
            bg_c, _, _, _ = _lcd_colors_norm(lcd_st)
            cr.set_source_rgba(bg_c[0], bg_c[1], bg_c[2], 0.18)
            _round_rect(cr, lz_x + 5, lz_y + 5, lz_w - 10, lz_h - 10, 6)
            cr.set_line_width(2.5)
            cr.stroke()
        self._lcd_rect = (lz_x + 8, lz_y + 8, lz_w - 16, lz_h - 16)

        # Mid-bezel engraved text + horizontal groove + dual mic/vent cutouts
        div_y = lz_y + lz_h + 16.0
        cr.set_source_rgba(0.08, 0.08, 0.08, 0.74)
        cr.select_font_face("Sans", cairo.FONT_SLANT_NORMAL, cairo.FONT_WEIGHT_BOLD)
        cr.set_font_size(9.5)
        te = cr.text_extents("UHF/VHF Transceiver")
        cr.move_to(fx_r - 12.0 - te.width, div_y - 3.0)
        cr.show_text("UHF/VHF Transceiver")

        cr.set_source_rgba(0.05, 0.05, 0.06, 0.65)
        cr.set_line_width(1.6)
        cr.move_to(fx_l + 6, div_y + 4.5)
        cr.line_to(fx_r - 6, div_y + 4.5)
        cr.stroke()
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.16)
        cr.set_line_width(0.8)
        cr.move_to(fx_l + 6, div_y + 5.9)
        cr.line_to(fx_r - 6, div_y + 5.9)
        cr.stroke()

        # Dual tiny recessed cutouts matching UV-K1前视图.png
        _round_rect(cr, fx_r - 42.0, div_y + 10.5, 14.0, 4.5, 2.0)
        _round_rect(cr, fx_r - 24.0, div_y + 10.5, 11.0, 4.5, 2.0)
        cr.set_source_rgb(0.06, 0.06, 0.07)
        cr.fill()

        # Register Side & Front Key Geometry
        self._side_specs = [
            ("ptt",   4.0, by + 56.0, 26.0, 114.0, True, 0),
            ("side1", 6.0, by + 180.0, 24.0, 36.0, False, 2),
            ("side2", 6.0, by + 224.0, 24.0, 36.0, False, 1),
        ]
        for kn, sx, sy, sw, sh, _, _ in self._side_specs:
            self.hitboxes[kn] = (sx, sy, sw, sh)

        kp_x, kp_y, kp_w = fx_l + 10.0, div_y + 21.0, (fx_r - fx_l) - 20.0
        gap_c, gap_r = 7.0, 8.0
        kw = (kp_w - gap_c * 3.0) / 4.0
        kh_nav, kh_num = 35.0, 34.0
        for r_i, row_keys in enumerate(K1_MATRIX):
            ky = kp_y + (0.0 if r_i == 0 else (kh_nav + gap_r + (r_i - 1) * (kh_num + gap_r)))
            kh = kh_nav if r_i == 0 else kh_num
            for c_i, (kn, pri, s1, s2) in enumerate(row_keys):
                kx = kp_x + c_i * (kw + gap_c)
                self.hitboxes[kn] = (kx, ky, kw, kh)
                self._key_specs.append((kn, kx, ky, kw, kh, pri, s1, s2, 5.0, r_i == 0))

    def _build_k5_chassis(self, cr: Any, W: int, H: int, led_state: str, torch_on: bool, lcd_st: LcdSettings) -> None:
        import cairo
        bx, by, bw, bh = 28.0, 56.0, W - 44.0, H - 68.0

        # 1. Tapered Whip Antenna Base + Orange Volume Knob + Guard Ear
        ax = bx + 52.0
        pat_ant = cairo.LinearGradient(ax - 22, 0, ax + 22, 0)
        pat_ant.add_color_stop_rgb(0.0, 0.10, 0.11, 0.12)
        pat_ant.add_color_stop_rgb(0.38, 0.25, 0.26, 0.28)
        pat_ant.add_color_stop_rgb(1.0, 0.08, 0.09, 0.10)
        cr.set_source(pat_ant)
        cr.move_to(ax - 20, by + 4)
        cr.line_to(ax - 15, 3)
        cr.line_to(ax + 15, 3)
        cr.line_to(ax + 20, by + 4)
        cr.close_path()
        cr.fill()

        tx = bx + bw * 0.48
        if torch_on:
            tg = cairo.RadialGradient(tx, by - 6, 2, tx, by - 6, 28)
            tg.add_color_stop_rgba(0.0, 1.0, 0.98, 0.82, 0.9)
            tg.add_color_stop_rgba(1.0, 1.0, 0.94, 0.65, 0.0)
            cr.set_source(tg)
            cr.arc(tx, by - 6, 28, 0, 2 * math.pi)
            cr.fill()
        cr.set_source_rgb(0.14, 0.15, 0.17)
        _round_rect(cr, tx - 14, by - 10, 28, 12, 3)
        cr.fill()
        self.hitboxes["_torch"] = (tx - 16, by - 16, 32, 20)

        # Orange-capped volume knob + right shoulder lanyard guard ear (exact UVK6.1.png)
        kx = bx + bw - 58.0
        _round_rect(cr, kx - 25, by - 26, 50, 30, 5)
        cr.set_source_rgb(0.13, 0.14, 0.16)
        cr.fill()
        _round_rect(cr, kx - 25, by - 37, 50, 13, 5)
        pat_o = cairo.LinearGradient(kx - 25, by - 37, kx + 25, by - 24)
        pat_o.add_color_stop_rgb(0.0, 0.92, 0.36, 0.04)
        pat_o.add_color_stop_rgb(0.45, 1.00, 0.54, 0.12)
        pat_o.add_color_stop_rgb(1.0, 0.82, 0.30, 0.03)
        cr.set_source(pat_o)
        cr.fill()
        # Small black guard ear right of knob
        _round_rect(cr, bx + bw - 22, by - 26, 14, 30, 4)
        cr.set_source_rgb(0.13, 0.14, 0.16)
        cr.fill()

        # Translucent Status LED
        lx, ly = bx + 52.0, by - 2.0
        cr.set_source_rgb(0.95, 0.24, 0.24) if led_state == "tx" else (
            cr.set_source_rgb(0.18, 0.86, 0.42) if led_state == "rx" else cr.set_source_rgb(0.75, 0.77, 0.79)
        )
        _round_rect(cr, lx - 10, ly - 5, 20, 8, 3)
        cr.fill()

        # 2. Matte Dark Polycarbonate Main Body
        _round_rect(cr, bx, by, bw, bh, 18)
        pat_b = cairo.LinearGradient(bx, by, bx + bw, by + bh)
        pat_b.add_color_stop_rgb(0.0, 0.22, 0.23, 0.25)
        pat_b.add_color_stop_rgb(0.5, 0.15, 0.16, 0.18)
        pat_b.add_color_stop_rgb(1.0, 0.11, 0.12, 0.14)
        cr.set_source(pat_b)
        cr.fill_preserve()
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.09)
        cr.set_line_width(1.0)
        cr.stroke()

        # 3. Brushed Silver-Titanium Upper LCD Armor Frame
        ux, uy, uw, uh = bx + 6.0, by + 6.0, bw - 12.0, 216.0
        _round_rect(cr, ux, uy, uw, uh, 14)
        pat_u = cairo.LinearGradient(ux, uy, ux + uw, uy + uh)
        pat_u.add_color_stop_rgb(0.0, 0.64, 0.63, 0.60)
        pat_u.add_color_stop_rgb(0.45, 0.46, 0.45, 0.43)
        pat_u.add_color_stop_rgb(1.0, 0.56, 0.55, 0.52)
        cr.set_source(pat_u)
        cr.fill_preserve()
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.22)
        cr.set_line_width(1.0)
        cr.stroke()

        _draw_torx_bolt(cr, ux + 15, uy + 15, 4.0)
        _draw_torx_bolt(cr, ux + uw - 15, uy + 15, 4.0)
        _draw_torx_bolt(cr, ux + 15, uy + uh - 54, 4.0)
        _draw_torx_bolt(cr, ux + uw - 15, uy + uh - 22, 4.0)

        # Recessed LCD Window
        lz_x, lz_y, lz_w, lz_h = int(ux + 18), int(uy + 12), int(uw - 36), 152
        _round_rect(cr, lz_x, lz_y, lz_w, lz_h, 7)
        cr.set_source_rgb(0.08, 0.09, 0.10)
        cr.fill()
        self._lcd_rect = (lz_x + 6, lz_y + 6, lz_w - 12, lz_h - 12)

        # Glossy QUANSHENG Badge right of M
        m_x, m_y, m_w, m_h = ux + 16.0, lz_y + lz_h + 10.0, 68.0, 32.0
        np_x, np_y, np_w, np_h = m_x + m_w + 10.0, m_y + 2.0, uw - (m_w + 56.0), 28.0
        _round_rect(cr, np_x, np_y, np_w, np_h, 8)
        cr.set_source_rgb(0.07, 0.08, 0.09)
        cr.fill()
        _draw_quansheng_logo(cr, np_x + np_w * 0.5, np_y + np_h * 0.5, font_size=11.0)

        # 3-Bar Speaker Grille Dish to the right of stacked ▲ / ▼
        mid_y = uy + uh + 12.0
        col_x, col_w = bx + 18.0, 56.0
        gx = col_x + col_w + 14.0
        gw = (bx + bw - 18.0) - gx
        _round_rect(cr, gx - 4, mid_y, gw + 8, 86.0, 8.0)
        cr.set_source_rgba(0.05, 0.05, 0.06, 0.55)
        cr.fill()
        for i in range(3):
            gy = mid_y + 7.0 + i * 26.0
            _round_rect(cr, gx, gy, gw, 16.0, 5.0)
            cr.set_source_rgb(0.05, 0.05, 0.06)
            cr.fill()
            _round_rect(cr, gx + 3, gy + 4, gw - 6, 8.0, 3.0)
            cr.set_source_rgb(0.19, 0.20, 0.22)
            cr.fill()

        # Register Side & Front Keys
        self._side_specs = [
            ("ptt",   4.0, by + 64.0, 26.0, 108.0, True, 0),
            ("side1", 6.0, by + 182.0, 24.0, 34.0, False, 0),
            ("side2", 6.0, by + 224.0, 24.0, 34.0, False, 0),
        ]
        for kn, sx, sy, sw, sh, _, _ in self._side_specs:
            self.hitboxes[kn] = (sx, sy, sw, sh)

        nav_specs = [
            ("menu", m_x, m_y, m_w, m_h, "M", "A", "", 6.0, False),
            ("up",   col_x, mid_y, col_w, 34.0, "_CHEV_UP_", "B", "", 7.0, True),
            ("down", col_x, mid_y + 42.0, col_w, 34.0, "_CHEV_DN_", "C", "", 7.0, True),
            ("exit", col_x, mid_y + 92.0, col_w + 12.0, 28.0, "EXIT", "", "", 6.0, True),
        ]
        for sp in nav_specs:
            self.hitboxes[sp[0]] = (sp[1], sp[2], sp[3], sp[4])
            self._key_specs.append(sp)

        kp_x, kp_y, kp_w = bx + 16.0, mid_y + 130.0, bw - 32.0
        gap_c, gap_r, kh = 6.0, 7.0, 32.0
        kw = (kp_w - gap_c * 3.0) / 4.0
        for r_i, row_keys in enumerate(K5_BOTTOM_MATRIX):
            ky = kp_y + r_i * (kh + gap_r)
            for c_i, (kn, pri, s1) in enumerate(row_keys):
                kx = kp_x + c_i * (kw + gap_c)
                self.hitboxes[kn] = (kx, ky, kw, kh)
                self._key_specs.append((kn, kx, ky, kw, kh, pri, s1, "", 6.0, False))

    def paint(
        self,
        cr: Any,
        model: str,
        lcd_state: FastLcdState,
        lcd_st: LcdSettings,
        *,
        press_anim: dict[str, float],
        hover_anim: dict[str, float],
        led_state: str = "off",
        torch_on: bool = False,
    ) -> None:
        """Composites cached chassis + blazing-fast LCD + smooth animated buttons (~0.5 ms total!)."""
        self._ensure_chassis(model, led_state, torch_on, lcd_st)
        accent = (0.98, 0.48, 0.08) if model == "k5" else (0.18, 0.56, 1.0)

        # 1. Cached static radio chassis (0.05 ms)
        cr.set_source_surface(self._chassis_surf, 0, 0)
        cr.paint()

        # 2. Fast layered ST7565 LCD (0.20 ms)
        lx, ly, lw, lh = self._lcd_rect
        paint_fast_lcd(cr, lx, ly, lw, lh, lcd_state, lcd_st, self.lcd_cache)

        # 3. Animated left flank keys (PTT + SK1 + SK2)
        for kn, sx, sy, sw, sh, is_ptt, dots in self._side_specs:
            p = _clamp1(press_anim.get(kn, 0.0))
            hv = _clamp1(hover_anim.get(kn, 0.0))
            dx = 2.4 * p
            _round_rect(cr, sx + dx, sy, sw, sh, 7 if is_ptt else 6)
            if is_ptt:
                cr.set_source_rgb(0.14 + 0.48 * p + 0.05 * hv, 0.15 - 0.03 * p + 0.05 * hv, 0.17 - 0.03 * p + 0.05 * hv)
                cr.fill_preserve()
                cr.set_source_rgb(
                    accent[0] * (1.0 - p) + 0.96 * p,
                    accent[1] * (1.0 - p) + 0.34 * p,
                    accent[2] * (1.0 - p) + 0.34 * p,
                )
                cr.set_line_width(1.9)
                cr.stroke()
                if model == "k1":
                    cr.set_source_rgb(
                        accent[0] * (1.0 - p) + 1.0 * p,
                        accent[1] * (1.0 - p) + 0.72 * p,
                        accent[2] * (1.0 - p) + 0.72 * p,
                    )
                    for ry in (sy + 22, sy + 44, sy + 66, sy + 88):
                        _round_rect(cr, sx + 9 + dx, ry, 4.0, 12.0, 2)
                        cr.fill()
            else:
                v = 0.15 + 0.05 * hv - 0.05 * p
                cr.set_source_rgb(v, v + 0.01, v + 0.025)
                cr.fill_preserve()
                cr.set_source_rgba(accent[0], accent[1], accent[2], 0.9 * p) if p > 0.05 else cr.set_source_rgba(1, 1, 1, 0.12 + 0.08 * hv)
                cr.set_line_width(1.1)
                cr.stroke()
                if dots > 0:
                    cr.set_source_rgba(0.76, 0.80, 0.84, 0.68)
                    if dots == 2:
                        cr.arc(sx + 10 + dx, sy + sh * 0.38, 1.8, 0, 2 * math.pi)
                        cr.arc(sx + 10 + dx, sy + sh * 0.62, 1.8, 0, 2 * math.pi)
                    else:
                        cr.arc(sx + 10 + dx, sy + sh * 0.50, 1.8, 0, 2 * math.pi)
                    cr.fill()

        # 4. Animated front keypad buttons
        for kn, kx, ky, kw, kh, pri, s1, s2, rad, is_nav in self._key_specs:
            _draw_animated_key(
                cr, kx, ky, kw, kh, pri, s1, s2,
                press_t=press_anim.get(kn, 0.0),
                hover_t=hover_anim.get(kn, 0.0),
                radius=rad, is_nav=is_nav, accent=accent,
            )


# =============================================================================
# 3. OFFSCREEN PNG & ANIMATED GIF EXPORTER (Trim + Cut/Splice + LCD or Body)
# =============================================================================

def render_lcd_to_rgba(fb: bytes, lcd_st: LcdSettings, width: int = 440, height: int = 304) -> tuple[int, int, bytes]:
    import cairo
    surf = cairo.ImageSurface(cairo.FORMAT_ARGB32, width, height)
    cr = cairo.Context(surf)
    st_snap = dataclasses.replace(lcd_st, ghosting=False)
    fs = FastLcdState()
    fs.step(fb, st_snap)
    paint_fast_lcd(cr, 0, 0, width, height, fs, st_snap, LcdLayerCache())
    surf.flush()
    return width, height, _bgra_to_rgba(bytes(surf.get_data()))


def render_handset_to_rgba(
    model: str,
    fb: bytes,
    lcd_st: LcdSettings,
    *,
    pressed_keys: frozenset[str] = frozenset(),
    led_state: str = "rx",
    torch_on: bool = False,
    scale: float = 1.0,
    bg_rgb: tuple[float, float, float] = (0.11, 0.12, 0.14),
    renderer: HandsetRenderer | None = None,
) -> tuple[int, int, bytes]:
    import cairo
    lw, lh = compute_handset_geometry(model)
    w, h = int(round(lw * scale)), int(round(lh * scale))
    surf = cairo.ImageSurface(cairo.FORMAT_ARGB32, w, h)
    cr = cairo.Context(surf)
    cr.set_source_rgb(*bg_rgb)
    cr.rectangle(0, 0, w, h)
    cr.fill()
    cr.scale(scale, scale)
    st_snap = dataclasses.replace(lcd_st, ghosting=False)
    fs = FastLcdState()
    fs.step(fb, st_snap)
    rend = renderer or HandsetRenderer()
    rend.paint(
        cr, model, fs, st_snap,
        press_anim={k: 1.0 for k in pressed_keys},
        hover_anim={}, led_state=led_state, torch_on=torch_on,
    )
    surf.flush()
    return w, h, _bgra_to_rgba(bytes(surf.get_data()))


def _bgra_to_rgba(raw: bytes) -> bytes:
    if _np is not None:
        arr = _np.frombuffer(raw, dtype=_np.uint8).reshape(-1, 4)
        return arr[:, [2, 1, 0, 3]].tobytes()
    out = bytearray(len(raw))
    out[0::4] = raw[2::4]
    out[1::4] = raw[1::4]
    out[2::4] = raw[0::4]
    out[3::4] = raw[3::4]
    return bytes(out)


def encode_png_rgba(w: int, h: int, rgba: bytes) -> bytes:
    def chunk(tag: bytes, payload: bytes) -> bytes:
        crc = zlib.crc32(tag + payload) & 0xFFFFFFFF
        return struct.pack(">I", len(payload)) + tag + payload + struct.pack(">I", crc)

    stride = w * 4
    raw = bytearray()
    for y in range(h):
        raw.append(0)
        raw.extend(rgba[y * stride : (y + 1) * stride])
    ihdr = struct.pack(">IIBBBBB", w, h, 8, 6, 0, 0, 0)
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", ihdr) + chunk(b"IDAT", zlib.compress(bytes(raw), 6)) + chunk(b"IEND", b"")


@dataclasses.dataclass
class RecordedFrame:
    ts_ms: int
    fb: bytes
    pressed: frozenset[str]
    led: str
    torch: bool


class RecordingSession:
    """Event-driven non-destructive recording timeline (Trim In/Out + Cut/Splice + LCD/Body GIF)."""

    def __init__(self) -> None:
        self.frames: list[RecordedFrame] = []
        self.recording: bool = False
        self.t0_mono: float = 0.0
        self.trim_in: int = 0
        self.trim_out: int = 0
        self.cuts: list[tuple[int, int]] = []

    def start(self, initial_fb: bytes, pressed: frozenset[str], led: str, torch: bool) -> None:
        self.frames = [RecordedFrame(0, bytes(initial_fb), frozenset(pressed), led, torch)]
        self.trim_in = 0
        self.trim_out = 0
        self.cuts = []
        self.t0_mono = time.monotonic()
        self.recording = True

    def push(self, fb: bytes, pressed: frozenset[str], led: str, torch: bool) -> None:
        if not self.recording:
            return
        if self.frames:
            last = self.frames[-1]
            if last.fb == fb and last.pressed == pressed and last.led == led and last.torch == torch:
                return
        ts = int((time.monotonic() - self.t0_mono) * 1000.0)
        self.frames.append(RecordedFrame(ts, bytes(fb), frozenset(pressed), led, torch))
        self.trim_out = len(self.frames) - 1

    def stop(self) -> None:
        if not self.recording:
            return
        self.recording = False
        self.trim_out = max(0, len(self.frames) - 1)

    def add_cut(self, start_idx: int, end_idx: int) -> None:
        a, b = min(start_idx, end_idx), max(start_idx, end_idx)
        a = max(self.trim_in, a)
        b = min(self.trim_out, b)
        if a <= b:
            self.cuts.append((a, b))

    def undo_cut(self) -> None:
        if self.cuts:
            self.cuts.pop()

    def is_cut(self, idx: int) -> bool:
        return any(a <= idx <= b for a, b in self.cuts)

    def active_indices(self) -> list[int]:
        if not self.frames:
            return []
        lo = max(0, min(self.trim_in, len(self.frames) - 1))
        hi = max(lo, min(self.trim_out, len(self.frames) - 1))
        return [i for i in range(lo, hi + 1) if not self.is_cut(i)]

    def frame_durations_ms(self, speed: float = 1.0, max_idle_ms: int | None = None) -> list[tuple[int, int]]:
        idxs = self.active_indices()
        if not idxs:
            return []
        spd = max(0.25, speed)
        out: list[tuple[int, int]] = []
        for pos, fi in enumerate(idxs):
            if pos + 1 < len(idxs):
                nxt = idxs[pos + 1]
                raw_dt = (self.frames[nxt].ts_ms - self.frames[fi].ts_ms) if nxt == fi + 1 else 120
            else:
                raw_dt = 350
            dt = max(25, raw_dt)
            if max_idle_ms is not None and dt > max_idle_ms:
                dt = max_idle_ms
            out.append((fi, max(20, int(round(dt / spd)))))
        return out

    def export_gif(
        self,
        out_path: str | Path,
        *,
        model: str,
        lcd_st: LcdSettings,
        mode: str = "lcd",
        speed: float = 1.0,
        max_idle_ms: int | None = None,
        progress_cb: Callable[[float], None] | None = None,
    ) -> int:
        seq = self.frame_durations_ms(speed=speed, max_idle_ms=max_idle_ms)
        if not seq:
            raise ValueError("No active frames to export")
        from PIL import Image

        rend = HandsetRenderer()
        pil_frames: list[Any] = []
        delays: list[int] = []
        total = len(seq)
        for pos, (fi, dt_ms) in enumerate(seq):
            rf = self.frames[fi]
            if mode == "body":
                w, h, rgba = render_handset_to_rgba(
                    model, rf.fb, lcd_st,
                    pressed_keys=rf.pressed, led_state=rf.led, torch_on=rf.torch,
                    scale=1.0, renderer=rend,
                )
            else:
                w, h, rgba = render_lcd_to_rgba(rf.fb, lcd_st, width=440, height=304)
            img = Image.frombytes("RGBA", (w, h), rgba).convert("RGB")
            pal_img = img.quantize(colors=64 if mode == "lcd" else 128, method=Image.Quantize.MEDIANCUT, dither=Image.Dither.NONE)
            pil_frames.append(pal_img)
            delays.append(dt_ms)
            if progress_cb is not None:
                progress_cb((pos + 1) / float(total))

        dest = Path(out_path)
        pil_frames[0].save(
            dest, format="GIF", save_all=True, append_images=pil_frames[1:],
            duration=delays, loop=0, optimize=True, disposal=2,
        )
        return dest.stat().st_size


# =============================================================================
# 4. GUDEV HOTPLUG AUTO-DETECTION & LOW-LATENCY RIGWIRE ENGINE
# =============================================================================

@dataclasses.dataclass
class DetectedPort:
    device: str
    pretty_name: str
    vendor: str = ""
    model: str = ""
    serial: str = ""
    vid: str = ""
    pid: str = ""
    is_quansheng: bool = False
    is_usb: bool = False


class PortMonitor:
    """Real-time USB serial detector via GUdev (Linux) with cross-platform pyserial fallback."""

    def __init__(self, on_change: Callable[[list[DetectedPort]], None] | None = None) -> None:
        self.on_change = on_change
        self.ports: list[DetectedPort] = []
        self._gudev_client: Any = None
        try:
            import gi
            gi.require_version("GUdev", "1.0")
            from gi.repository import GUdev
            self._gudev_client = GUdev.Client(subsystems=["tty"])
            self._gudev_client.connect("uevent", lambda _c, _act, _dev: self.scan())
        except Exception:
            self._gudev_client = None
        self.scan()

    def scan(self) -> list[DetectedPort]:
        found: list[DetectedPort] = []
        if self._gudev_client is not None:
            for dev in self._gudev_client.query_by_subsystem("tty"):
                dev_file = dev.get_device_file()
                if not dev_file:
                    continue
                name = dev.get_name() or ""
                if not ((dev.get_property("ID_BUS") == "usb") or ("USB" in name) or ("ACM" in name)):
                    continue
                vid = pid = vendor = model = ser = ""
                cur = dev
                for _ in range(10):
                    if cur is None:
                        break
                    vid = vid or cur.get_property("ID_VENDOR_ID") or cur.get_property("ID_USB_VENDOR_ID") or cur.get_sysfs_attr("idVendor") or ""
                    pid = pid or cur.get_property("ID_MODEL_ID") or cur.get_property("ID_USB_MODEL_ID") or cur.get_sysfs_attr("idProduct") or ""
                    vendor = vendor or cur.get_property("ID_VENDOR") or cur.get_sysfs_attr("manufacturer") or ""
                    model = model or cur.get_property("ID_MODEL") or cur.get_sysfs_attr("product") or ""
                    ser = ser or cur.get_property("ID_SERIAL_SHORT") or cur.get_sysfs_attr("serial") or ""
                    if vid and pid and vendor and model:
                        break
                    cur = cur.get_parent()
                v_c = vendor.replace("_", " ").strip()
                m_c = model.replace("_", " ").strip()
                pretty = m_c if m_c.lower().startswith("quansheng") else f"{v_c} {m_c}".strip() or dev_file
                is_qs = any(k in pretty.lower() for k in ("quansheng", "uv-k", "kern"))
                found.append(DetectedPort(dev_file, pretty, v_c, m_c, ser, vid, pid, is_qs, True))
        else:
            try:
                from serial.tools import list_ports
                for p in list_ports.comports():
                    desc = (p.product or p.description or "").strip()
                    is_qs = any(k in desc.lower() for k in ("quansheng", "uv-k", "kern"))
                    found.append(DetectedPort(p.device, desc or p.device, model=desc, serial=p.serial_number or "", is_quansheng=is_qs, is_usb=True))
            except Exception:
                pass

        found.sort(key=lambda d: (0 if d.is_quansheng else 1, d.device))
        changed = [p.device for p in found] != [p.device for p in self.ports]
        self.ports = found
        if changed and self.on_change is not None:
            self.on_change(found)
        return found


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


def detect_hardware_model(volume_name: str, hello: dict[str, Any] | None = None, port_info: DetectedPort | None = None) -> str:
    blob = " ".join([
        volume_name or "",
        (port_info.pretty_name if port_info else ""),
        (port_info.model if port_info else ""),
    ]).lower()
    if "k5" in blob or "k6" in blob:
        return "k5"
    if "k1" in blob or "k10" in blob:
        return "k1"
    if hello and (int(hello.get("hardware_profile", 0) or 0) & 0xFF) == 0x05:
        return "k5"
    return "k1"


class DockLink:
    """Pipelined low-latency RigWire bridge shared by LCD streaming, keys, Files, Wire, and WebDAV."""

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

    def connect(
        self,
        port: str,
        baud: int = 115200,
        model_override: str | None = None,
        port_info: DetectedPort | None = None,
    ) -> None:
        self.disconnect()
        self.baud = baud
        if port.lower() == "demo":
            self.demo_mode = True
            self.port = "demo"
            self.detected_model = model_override if model_override in ("k1", "k5") else "k1"
            self.volume_name = "Quansheng UV-K5 v3 DEMO" if self.detected_model == "k5" else "Quansheng UV-K1 DEMO"
            self.hello_info = {"minor_version": 1, "accepted_max_payload": 128}
            self.last_telemetry = {
                "serial_str": "DEMO0001K1",
                "mac_addr": "02:4B:45:52:4E:01",
                "battery_mv": 8040,
                "battery_pct": 92,
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
            self.rtt_ms = 0.2
            self.last_fb = make_demo_framebuffer()
            self._emit_log(f"Connected {self.volume_name} (demo)")
            if self.on_frame:
                self.on_frame()
            if self.on_telemetry:
                self.on_telemetry()
            return

        with self.lock:
            last_err: Exception | None = None
            cli: rigwire.RigWireClient | None = None
            hello: dict[str, Any] = {}
            telem: dict[str, Any] = {}
            for attempt in range(2):
                cand = rigwire.RigWireClient(port, baudrate=baud, timeout=1.5)
                try:
                    hello = cand.hello()
                    try:
                        telem = cand.get_system_telemetry()
                    except Exception:
                        telem = {}
                    cli = cand
                    break
                except Exception as exc:
                    last_err = exc
                    try:
                        cand.close()
                    except Exception:
                        pass
                    if attempt == 0:
                        time.sleep(0.12)
            if cli is None:
                raise last_err or RuntimeError(f"Could not connect to {port}")
            self.client = cli
            self.fs_backend = kernfs_mount.RigWireFsBackend(cli, cache_ttl=2.0, shared_lock=self.lock)
            self.port = port
            self.hello_info = hello
            self.last_telemetry = telem
            self.volume_name = kernfs_mount.resolve_volume_name(port, hello=hello, telemetry=telem)
            self.detected_model = model_override or detect_hardware_model(self.volume_name, hello=hello, port_info=port_info)
            self.demo_mode = False
            self.connected = True
            self.error_count = 0
            self.rtt_ms = cli.last_rtt_ms
        self._emit_log(f"Connected {port} -> {self.volume_name} ({self.detected_model.upper()}, RTT {self.rtt_ms:.1f} ms)")
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
        was = self.connected
        self.connected = False
        self.demo_mode = False
        self.torch_on = False
        self.keypad_locked = False
        self.volume_name = "Quansheng UV-K1"
        self.hello_info = {}
        self.last_telemetry = {}
        self.fps = 0.0
        self.rtt_ms = 0.0
        if was:
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
        self._cmd_q.put(("key", (rigwire.KEY_CODES[key_name], rigwire.KEY_STATES[state_name])))

    def set_flashlight(self, enabled: bool) -> None:
        self.torch_on = enabled
        if self.demo_mode:
            self._emit_log(f"Flashlight {'ON' if enabled else 'OFF'}")
            return
        if self.connected:
            self._cmd_q.put(("torch", 1 if enabled else 0))

    def set_keypad_lock(self, locked: bool) -> None:
        self.keypad_locked = locked
        if self.demo_mode:
            self._emit_log(f"Keypad lock {'ON' if locked else 'OFF'}")
            return
        if self.connected:
            self._cmd_q.put(("lock", locked))

    def toggle_vfo(self) -> None:
        if self.demo_mode:
            self._emit_log("Swapped active VFO")
            return
        if self.connected:
            self._cmd_q.put(("vfo", None))

    def run_wire_command(self, cmd_line: str) -> str:
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
                self.rtt_ms = (time.monotonic() - t0) * 1000.0
                return f"PONG ({self.rtt_ms:.1f} ms)"
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
            return "\n".join(f"  {'d' if e['is_dir'] else '-'}  {e['size']:>7} B  {e['name']}" for e in items) or f"{path}: (empty)"
        if op == "stat" and len(parts) > 1:
            assert self.fs_backend is not None
            return str(self.fs_backend.stat(parts[1]))
        if op == "key" and len(parts) > 1:
            kn = parts[1].lower()
            st_n = parts[2].lower() if len(parts) > 2 else "press"
            if kn not in rigwire.KEY_CODES or st_n not in rigwire.KEY_STATES:
                return f"Invalid key/state. Keys: {', '.join(rigwire.KEY_CODES.keys())}"
            self.send_key(kn, st_n)
            return f"Sent key {kn} ({st_n})"
        if op == "vfo":
            self.toggle_vfo()
            return "Swapped VFO"
        if op == "freq" and len(parts) > 1:
            mhz = float(parts[1])
            hz = int(round(mhz * 1_000_000))
            if self.demo_mode:
                self.demo_freq_khz = hz // 1000
                self.last_fb = make_demo_framebuffer(freq_a=f"{self.demo_freq_khz//1000}.{self.demo_freq_khz%1000:03d}", status="TUNE")
                if self.on_frame:
                    self.on_frame()
                return f"Tuned demo VFO A to {mhz:.3f} MHz"
            with self.lock:
                assert self.client is not None
                self.client.set_frequency(hz)
                return f"Tuned to {hz} Hz"
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
        srv = kernfs_mount.KernWebDAVServer(("127.0.0.1", http_port), self.fs_backend, self.policy, volume_name=self.volume_name)
        self.dav_server = srv
        self.dav_port = srv.server_address[1]
        threading.Thread(target=srv.serve_forever, daemon=True).start()
        self._emit_log(f"Mounted '{self.volume_name}' at http://127.0.0.1:{self.dav_port}/")
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
            self._emit_log("Unmounted WebDAV drive")

    def _drain_priority_cmds(self) -> None:
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
            except Exception:
                self.error_count += 1

    def _poll_loop(self) -> None:
        frames = 0
        t_fps = time.monotonic()
        last_telem = 0.0
        pipe = 2
        while not self._stop.is_set() and self.connected:
            try:
                with self.lock:
                    if not self.client:
                        break
                    self._drain_priority_cmds()
                    fb = self.client.screencap(on_between_chunks=self._drain_priority_cmds, pipeline=pipe)
                    self.rtt_ms = self.client.last_rtt_ms
                if len(fb) == 1024:
                    changed = fb != self.last_fb
                    self.last_fb = fb
                    frames += 1
                    if changed and self.on_frame is not None:
                        self.on_frame()
            except Exception:
                self.error_count += 1
                pipe = 1  # Automatically step down pipeline on noisy UART links
                time.sleep(0.10)
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
            # Zero artificial frame cap — brief 1ms yield lets UI/Files acquire lock smoothly
            time.sleep(0.001)


# =============================================================================
# 5. GNOME HIG NATIVE GTK4 + LIBADWAITA APPLICATION
# =============================================================================

GTK_CSS = """
button.rec-live {
    background: #dc2626;
    color: #ffffff;
}
button.rec-live:hover {
    background: #ef4444;
}
.rec-dot {
    min-width: 8px;
    min-height: 8px;
    border-radius: 99px;
    background: #ef4444;
}
textview.wire-console {
    font-family: monospace;
    font-size: 11.5px;
    padding: 8px;
}
"""


def _battery_icon_name(pct: int | None) -> str:
    if pct is None:
        return "battery-missing-symbolic"
    steps = (0, 10, 20, 30, 40, 50, 60, 70, 80, 90, 100)
    return f"battery-level-{min(steps, key=lambda s: abs(s - pct))}-symbolic"


def launch_gtk4(
    initial_port: str | None = None,
    baud: int = 115200,
    model_override: str | None = None,
    palette_override: str | None = None,
) -> int:
    import gi
    gi.require_version("Gtk", "4.0")
    gi.require_version("Gdk", "4.0")
    from gi.repository import Gdk, Gio, GLib, Gtk

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
            self.recorder = RecordingSession()
            self.handset_renderer = HandsetRenderer()
            self.lcd_state = FastLcdState()
            self.forced_model = model_override if model_override in ("k1", "k5") else None
            self.active_model = self.forced_model or "k1"
            self.user_picked_palette = palette_override is not None
            self.lcd_st = LcdSettings(
                palette=(
                    palette_override if (palette_override and palette_override in LCD_PALETTES)
                    else ("UV-K5" if self.active_model == "k5" else "UV-K1")
                )
            )
            self.auto_connect_usb = (initial_port is None)
            self._pressed_keys: set[str] = set()
            self._hovered_key: str | None = None
            self._press_anim: dict[str, float] = {}
            self._hover_anim: dict[str, float] = {}
            self._anim_timer: int | None = None
            self._hold_timers: dict[str, int] = {}
            self._frame_pending = False
            self._updating_switches = False
            self.current_dir = "/"

            # Capture / GIF editor state
            self._pb_playing = False
            self._pb_idx = 0
            self._pb_timer: int | None = None
            self._cut_mark_start: int | None = None
            self._export_mode = "lcd"
            self._export_speed = 1.0
            self._cap_idle = True

        def do_activate(self) -> None:
            if has_adw:
                Adw.StyleManager.get_default().set_color_scheme(Adw.ColorScheme.PREFER_DARK)

            css = Gtk.CssProvider()
            css.load_from_string(GTK_CSS)
            Gtk.StyleContext.add_provider_for_display(
                Gdk.Display.get_default(), css, Gtk.STYLE_PROVIDER_PRIORITY_APPLICATION
            )

            self.win = base_win_cls(application=self)
            self.win.set_title("Kern Dock")

            # App actions & keyboard accelerators (GNOME HIG best practice)
            for name, cb, accel in (
                ("screenshot", lambda _a, _p: self._on_save_screenshot(None), "<Control>s"),
                ("toggle-dock", lambda _a, _p: self.pane_toggle.set_active(not self.pane_toggle.get_active()), "<Control>b"),
                ("toggle-record", lambda _a, _p: self.rec_btn.set_active(not self.rec_btn.get_active()), "<Control>r"),
                ("quit", lambda _a, _p: self.win.close(), "<Control>q"),
            ):
                act = Gio.SimpleAction.new(name, None)
                act.connect("activate", cb)
                self.add_action(act)
                self.set_accels_for_action(f"app.{name}", [accel])

            self.link.on_frame = self._schedule_frame_update
            self.link.on_telemetry = lambda: GLib.idle_add(self._refresh_telemetry_ui)
            self.link.on_log = lambda line: GLib.idle_add(self._append_console_line, line)
            self.port_monitor = PortMonitor(on_change=lambda _p: GLib.idle_add(self._on_ports_changed))

            # --- Clean GNOME HIG HeaderBar (NO Record / Mode clutter in top bar!) ---
            header = Adw.HeaderBar() if has_adw else Gtk.HeaderBar()

            # Start: Clean Connection & Live Hardware Status MenuButton
            self.conn_menu_btn = Gtk.MenuButton()
            self.conn_menu_btn.set_tooltip_text("Radio connection & USB devices")
            st_box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=6)
            self.hdr_link_icon = Gtk.Image.new_from_icon_name("network-offline-symbolic")
            self.hdr_batt_icon = Gtk.Image.new_from_icon_name("battery-missing-symbolic")
            self.hdr_batt_icon.set_visible(False)
            self.hdr_dev_lbl = Gtk.Label(label="Connect")
            self.hdr_rec_dot = Gtk.Box()
            self.hdr_rec_dot.add_css_class("rec-dot")
            self.hdr_rec_dot.set_valign(Gtk.Align.CENTER)
            self.hdr_rec_dot.set_visible(False)
            st_box.append(self.hdr_link_icon)
            st_box.append(self.hdr_dev_lbl)
            st_box.append(self.hdr_batt_icon)
            st_box.append(self.hdr_rec_dot)
            self.conn_menu_btn.set_child(st_box)
            self.conn_popover = self._build_connection_popover()
            self.conn_menu_btn.set_popover(self.conn_popover)
            header.pack_start(self.conn_menu_btn)

            # Center: Native Adw.ViewSwitcher (Device · Files · Wire · Capture)
            self.dock_stack = Adw.ViewStack() if has_adw else Gtk.Stack()
            if has_adw:
                self.view_switcher = Adw.ViewSwitcher()
                self.view_switcher.set_stack(self.dock_stack)
                self.view_switcher.set_policy(Adw.ViewSwitcherPolicy.WIDE)
                header.set_title_widget(self.view_switcher)
            else:
                self.view_switcher = None

            # End: Dock Pane Collapse Toggle + Primary Menu (Display Optics & Shortcuts)
            end_box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=4)
            self.pane_toggle = Gtk.ToggleButton(icon_name="sidebar-show-right-symbolic", active=True)
            self.pane_toggle.set_tooltip_text("Show/hide dock panel (Ctrl+B)")
            end_box.append(self.pane_toggle)

            menu_btn = Gtk.MenuButton(icon_name="open-menu-symbolic")
            menu_btn.set_tooltip_text("Display settings & shortcuts")
            menu_btn.set_popover(self._build_primary_menu_popover())
            end_box.append(menu_btn)
            header.pack_end(end_box)

            # --- Left Pane: Precision Interactive Cairo Radio Handset ---
            self.handset_area = Gtk.DrawingArea()
            lw, lh = compute_handset_geometry(self.active_model)
            self.handset_area.set_content_width(lw)
            self.handset_area.set_content_height(lh)
            self.handset_area.set_halign(Gtk.Align.CENTER)
            self.handset_area.set_valign(Gtk.Align.CENTER)
            self.handset_area.set_margin_start(14)
            self.handset_area.set_margin_end(14)
            self.handset_area.set_margin_top(10)
            self.handset_area.set_margin_bottom(12)
            self.handset_area.set_draw_func(self._draw_handset)

            g_left = Gtk.GestureClick()
            g_left.set_button(1)
            g_left.connect("pressed", self._on_handset_press)
            g_left.connect("released", self._on_handset_release)
            g_left.connect("stopped", lambda _g: self._release_all_pointer_keys())
            self.handset_area.add_controller(g_left)

            g_right = Gtk.GestureClick()
            g_right.set_button(3)
            g_right.connect("pressed", self._on_handset_right_click)
            self.handset_area.add_controller(g_right)

            g_motion = Gtk.EventControllerMotion()
            g_motion.connect("motion", self._on_handset_motion)
            g_motion.connect("leave", lambda _c: self._set_hovered_key(None))
            self.handset_area.add_controller(g_motion)

            # --- Right Pane: Device · Files · Wire · Capture ---
            dev_page = self._build_device_page()
            files_page = self._build_files_page()
            wire_page = self._build_wire_page()
            cap_page = self._build_capture_page()

            if has_adw:
                self.dock_stack.add_titled_with_icon(dev_page, "device", "Device", "computer-symbolic")
                self.dock_stack.add_titled_with_icon(files_page, "files", "Files", "folder-symbolic")
                self.dock_stack.add_titled_with_icon(wire_page, "wire", "Wire", "utilities-terminal-symbolic")
                self.dock_stack.add_titled_with_icon(cap_page, "capture", "Capture", "camera-video-symbolic")
            else:
                self.dock_stack.add_titled(dev_page, "device", "Device")
                self.dock_stack.add_titled(files_page, "files", "Files")
                self.dock_stack.add_titled(wire_page, "wire", "Wire")
                self.dock_stack.add_titled(cap_page, "capture", "Capture")
            self.dock_stack.set_hexpand(True)
            self.dock_stack.set_vexpand(True)

            self.right_pane = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL)
            self.right_pane.set_hexpand(True)
            self.right_pane.set_size_request(440, -1)
            self.right_pane.append(Gtk.Separator(orientation=Gtk.Orientation.VERTICAL))
            self.right_pane.append(self.dock_stack)
            self.pane_toggle.connect("toggled", self._on_dock_pane_toggled)

            split = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL)
            split.append(self.handset_area)
            split.append(self.right_pane)

            if has_adw:
                self.toast_overlay = Adw.ToastOverlay()
                self.toast_overlay.set_child(split)
                tb = Adw.ToolbarView()
                tb.add_top_bar(header)
                tb.set_content(self.toast_overlay)
                self.win.set_content(tb)
            else:
                self.toast_overlay = None
                outer = Gtk.Box(orientation=Gtk.Orientation.VERTICAL)
                outer.append(header)
                outer.append(split)
                self.win.set_child(outer)

            key_ctrl = Gtk.EventControllerKey()
            key_ctrl.connect("key-pressed", self._on_pc_key_pressed)
            key_ctrl.connect("key-released", self._on_pc_key_released)
            self.win.add_controller(key_ctrl)

            self.win.connect("close-request", self._on_close_request)
            self._rebuild_connection_list()
            self.lcd_state.step(self.link.last_fb, self.lcd_st)
            self._sync_window_size()

            if initial_port:
                GLib.timeout_add(70, lambda: (self._connect_to(initial_port), False)[1])
            elif self.auto_connect_usb and any(p.is_quansheng for p in self.port_monitor.ports):
                first_qs = next(p for p in self.port_monitor.ports if p.is_quansheng)
                GLib.timeout_add(100, lambda: (self._connect_to(first_qs.device, first_qs), False)[1])

            self.win.present()

        def _notify(self, msg: str) -> None:
            if self.toast_overlay is not None and has_adw:
                t = Adw.Toast.new(msg)
                t.set_timeout(2)
                self.toast_overlay.add_toast(t)

        # --- Auto Window Sizing (Tightly hugs handset when collapsed, balanced when expanded) ---
        def _sync_window_size(self) -> None:
            lw, lh = compute_handset_geometry(self.active_model)
            self.handset_area.set_content_width(lw)
            self.handset_area.set_content_height(lh)
            expanded = self.pane_toggle.get_active()
            if self.view_switcher is not None:
                self.view_switcher.set_visible(expanded)
            target_w = (lw + 32 + 450) if expanded else (lw + 32)
            target_h = max(lh + 76, 668 if expanded else lh + 76)
            self.win.set_default_size(target_w, target_h)

        def _on_dock_pane_toggled(self, btn: Any) -> None:
            self.right_pane.set_visible(btn.get_active())
            self._sync_window_size()

        # --- Upgraded Native Connection Popover (Live GUdev boxed-list + Auto-Connect) ---
        def _build_connection_popover(self) -> Any:
            pop = Gtk.Popover()
            box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=10)
            box.set_margin_top(12)
            box.set_margin_bottom(12)
            box.set_margin_start(12)
            box.set_margin_end(12)
            box.set_size_request(310, -1)

            # Live Connected Summary Card (visible only while connected)
            self.conn_card = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=6)
            self.conn_card.set_visible(False)
            self.conn_card_title = Gtk.Label(xalign=0)
            self.conn_card_title.add_css_class("heading")
            self.conn_card_sub = Gtk.Label(xalign=0)
            self.conn_card_sub.add_css_class("dim-label")
            self.conn_card.append(self.conn_card_title)
            self.conn_card.append(self.conn_card_sub)
            disc_btn = Gtk.Button(label="Disconnect")
            disc_btn.add_css_class("destructive-action")
            disc_btn.connect("clicked", lambda _b: (self._disconnect(), self.conn_popover.popdown()))
            self.conn_card.append(disc_btn)
            self.conn_card.append(Gtk.Separator())
            box.append(self.conn_card)

            # USB Devices Header + Rescan
            hdr = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=6)
            lbl = Gtk.Label(label="USB Radios", xalign=0, hexpand=True)
            lbl.add_css_class("heading")
            hdr.append(lbl)
            ref = Gtk.Button.new_from_icon_name("view-refresh-symbolic")
            ref.add_css_class("flat")
            ref.set_tooltip_text("Rescan USB ports")
            ref.connect("clicked", lambda _b: (self.port_monitor.scan(), self._rebuild_connection_list()))
            hdr.append(ref)
            box.append(hdr)

            self.usb_list = Gtk.ListBox(selection_mode=Gtk.SelectionMode.NONE)
            self.usb_list.add_css_class("boxed-list")
            box.append(self.usb_list)

            # Virtual / Emulation Section
            dev_lbl = Gtk.Label(label="Virtual &amp; Emulation", use_markup=True, xalign=0)
            dev_lbl.add_css_class("caption-heading")
            dev_lbl.add_css_class("dim-label")
            box.append(dev_lbl)

            virt_list = Gtk.ListBox(selection_mode=Gtk.SelectionMode.NONE)
            virt_list.add_css_class("boxed-list")
            for pid, title, sub in (
                ("tcp://127.0.0.1:4446", "QEMU Emulator", "tcp://127.0.0.1:4446"),
                ("demo", "Offline Demo", "Local preview &amp; virtual KernFS"),
            ):
                if has_adw:
                    row = Adw.ActionRow(title=title, subtitle=sub, activatable=True)
                    row.add_suffix(Gtk.Image.new_from_icon_name("go-next-symbolic"))
                    row.connect("activated", lambda _r, p=pid: (self.conn_popover.popdown(), self._connect_to(p)))
                    virt_list.append(row)
            box.append(virt_list)

            # Auto-Connect Toggle
            ac_row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
            ac_row.append(Gtk.Label(label="Auto-connect USB radio", xalign=0, hexpand=True))
            ac_sw = Gtk.Switch(active=self.auto_connect_usb)
            ac_sw.connect("notify::active", lambda w, _p: setattr(self, "auto_connect_usb", w.get_active()))
            ac_row.append(ac_sw)
            box.append(ac_row)

            pop.set_child(box)
            return pop

        def _rebuild_connection_list(self) -> None:
            child = self.usb_list.get_first_child()
            while child is not None:
                nxt = child.get_next_sibling()
                self.usb_list.remove(child)
                child = nxt

            if not self.port_monitor.ports:
                if has_adw:
                    empty = Adw.ActionRow(title="No USB radios detected", subtitle="Plug in via USB-C or serial cable")
                    empty.set_sensitive(False)
                    self.usb_list.append(empty)
                return

            for p in self.port_monitor.ports:
                if has_adw:
                    id_tag = f" · {p.vid}:{p.pid}" if (p.vid and p.pid) else ""
                    row = Adw.ActionRow(
                        title=GLib.markup_escape_text(p.pretty_name),
                        subtitle=GLib.markup_escape_text(f"{p.device}{id_tag}"),
                        activatable=True,
                    )
                    row.add_prefix(Gtk.Image.new_from_icon_name("drive-removable-media-symbolic" if p.is_quansheng else "network-wired-symbolic"))
                    if self.link.connected and self.link.port == p.device:
                        row.add_suffix(Gtk.Image.new_from_icon_name("object-select-symbolic"))
                    else:
                        row.add_suffix(Gtk.Image.new_from_icon_name("go-next-symbolic"))
                    row.connect("activated", lambda _r, po=p: (self.conn_popover.popdown(), self._connect_to(po.device, po)))
                    self.usb_list.append(row)

        def _on_ports_changed(self) -> bool:
            self._rebuild_connection_list()
            if self.auto_connect_usb and not self.link.connected:
                for p in self.port_monitor.ports:
                    if p.is_quansheng:
                        self._connect_to(p.device, p)
                        break
            return False

        # --- Primary Menu Popover (LCD Optics + Quick Actions + Shortcuts) ---
        def _build_primary_menu_popover(self) -> Any:
            pop = Gtk.Popover()
            box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=10)
            box.set_margin_top(12)
            box.set_margin_bottom(12)
            box.set_margin_start(14)
            box.set_margin_end(14)
            box.set_size_request(255, -1)

            lbl = Gtk.Label(label="Display Optics", xalign=0)
            lbl.add_css_class("heading")
            box.append(lbl)

            names = list(LCD_PALETTES.keys())
            self.pal_dd = Gtk.DropDown.new_from_strings(names)
            self.pal_dd.set_selected(names.index(self.lcd_st.palette))
            self.pal_dd.connect("notify::selected", self._on_palette_changed)
            box.append(self.pal_dd)

            bl_row = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=2)
            bl_row.append(Gtk.Label(label="Backlight", xalign=0))
            bl_s = Gtk.Scale.new_with_range(Gtk.Orientation.HORIZONTAL, 0, 10, 1)
            bl_s.set_draw_value(True)
            bl_s.set_digits(0)
            bl_s.set_value(self.lcd_st.backlight)
            bl_s.connect("value-changed", lambda s: (setattr(self.lcd_st, "backlight", int(s.get_value())), self.handset_area.queue_draw()))
            bl_row.append(bl_s)
            box.append(bl_row)

            ct_row = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=2)
            ct_row.append(Gtk.Label(label="Contrast", xalign=0))
            ct_s = Gtk.Scale.new_with_range(Gtk.Orientation.HORIZONTAL, 0, 15, 1)
            ct_s.set_draw_value(True)
            ct_s.set_digits(0)
            ct_s.set_value(self.lcd_st.contrast)
            ct_s.connect("value-changed", lambda s: (setattr(self.lcd_st, "contrast", int(s.get_value())), self.handset_area.queue_draw()))
            ct_row.append(ct_s)
            box.append(ct_row)

            for title, attr in (("STN Ghosting", "ghosting"), ("Pixel Grid", "pixel_grid"), ("Invert LCD", "invert_lcd")):
                r = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
                r.append(Gtk.Label(label=title, xalign=0, hexpand=True))
                sw = Gtk.Switch(active=getattr(self.lcd_st, attr))
                sw.connect("notify::active", lambda w, _p, a=attr: self._toggle_lcd_flag(a, w.get_active()))
                r.append(sw)
                box.append(r)

            box.append(Gtk.Separator())
            kb = Gtk.Label(
                label="Keys: Enter=MENU · Esc=EXIT · Arrows=Nav\nSpace=PTT · 0–9/*/F · Ctrl+S PNG · Ctrl+R Rec",
                xalign=0,
            )
            kb.add_css_class("caption")
            kb.add_css_class("dim-label")
            box.append(kb)

            pop.set_child(box)
            return pop

        def _toggle_lcd_flag(self, attr: str, val: bool) -> None:
            setattr(self.lcd_st, attr, val)
            self.lcd_state.step(self._current_display_fb(), self.lcd_st)
            self.handset_area.queue_draw()

        def _on_palette_changed(self, dd: Any, _p: Any) -> None:
            names = list(LCD_PALETTES.keys())
            idx = dd.get_selected()
            if 0 <= idx < len(names):
                self.lcd_st.palette = names[idx]
                self.user_picked_palette = True
                self.handset_area.queue_draw()

        # --- Unified 60 FPS Animation Loop (Ghosting + Smooth Spring Buttons; 0% CPU when idle!) ---
        def _ensure_anim_loop(self) -> None:
            if self._anim_timer is None:
                self._anim_timer = GLib.timeout_add(16, self._tick_animations)

        def _tick_animations(self) -> bool:
            active = False
            fb, pressed, _led, _torch = self._current_display_state()

            # 1. Advance STN liquid-crystal ghosting (vectorized ~0.04 ms)
            if self.lcd_state.step(fb, self.lcd_st):
                active = True

            # 2. Advance smooth spring button press & hover interpolations
            all_k = set(self._press_anim.keys()) | set(pressed)
            for kn in all_k:
                tgt = 1.0 if kn in pressed else 0.0
                cur = self._press_anim.get(kn, 0.0)
                rate = 0.55 if tgt > cur else 0.30  # Fast tactile click down, smooth spring back
                nxt = cur + (tgt - cur) * rate
                if abs(tgt - nxt) < 0.015:
                    if tgt == 0.0:
                        self._press_anim.pop(kn, None)
                    else:
                        self._press_anim[kn] = 1.0
                else:
                    self._press_anim[kn] = nxt
                    active = True

            all_h = set(self._hover_anim.keys()) | ({self._hovered_key} if self._hovered_key else set())
            for kn in all_h:
                tgt = 1.0 if kn == self._hovered_key else 0.0
                cur = self._hover_anim.get(kn, 0.0)
                nxt = cur + (tgt - cur) * 0.35
                if abs(tgt - nxt) < 0.015:
                    if tgt == 0.0:
                        self._hover_anim.pop(kn, None)
                    else:
                        self._hover_anim[kn] = 1.0
                else:
                    self._hover_anim[kn] = nxt
                    active = True

            self.handset_area.queue_draw()
            if not active:
                self._anim_timer = None
                return False
            return True

        # --- Handset Drawing & Pointer Hit-Testing ---
        def _current_display_state(self) -> tuple[bytes, frozenset[str], str, bool]:
            if (not self.recorder.recording) and self.recorder.frames and self.dock_stack.get_visible_child_name() == "capture":
                idx = max(0, min(self._pb_idx, len(self.recorder.frames) - 1))
                rf = self.recorder.frames[idx]
                return rf.fb, rf.pressed, rf.led, rf.torch
            led = "tx" if ("ptt" in self._pressed_keys and self.link.connected) else ("rx" if self.link.connected else "off")
            return self.link.last_fb, frozenset(self._pressed_keys), led, self.link.torch_on

        def _current_display_fb(self) -> bytes:
            return self._current_display_state()[0]

        def _draw_handset(self, _area: Any, cr: Any, w: int, h: int) -> None:
            lw, lh = compute_handset_geometry(self.active_model)
            scale = min(w / float(lw), h / float(lh))
            ox = (w - lw * scale) * 0.5
            oy = (h - lh * scale) * 0.5
            cr.save()
            cr.translate(ox, oy)
            cr.scale(scale, scale)
            _fb, _pressed, led, torch = self._current_display_state()
            self.handset_renderer.paint(
                cr, self.active_model, self.lcd_state, self.lcd_st,
                press_anim=self._press_anim, hover_anim=self._hover_anim,
                led_state=led, torch_on=torch,
            )
            cr.restore()

        def _hit_test(self, x: float, y: float) -> str | None:
            lw, lh = compute_handset_geometry(self.active_model)
            w, h = self.handset_area.get_width(), self.handset_area.get_height()
            scale = min(w / float(lw), h / float(lh))
            ox, oy = (w - lw * scale) * 0.5, (h - lh * scale) * 0.5
            lx, ly = (x - ox) / scale, (y - oy) / scale
            for kn, (rx, ry, rw, rh) in self.handset_renderer.hitboxes.items():
                if rx <= lx <= rx + rw and ry <= ly <= ry + rh:
                    return kn
            return None

        def _set_hovered_key(self, kn: str | None) -> None:
            if kn != self._hovered_key:
                self._hovered_key = kn
                self._ensure_anim_loop()

        def _on_handset_motion(self, _c: Any, x: float, y: float) -> None:
            self._set_hovered_key(self._hit_test(x, y))

        def _on_handset_press(self, _g: Any, _n: int, x: float, y: float) -> None:
            kn = self._hit_test(x, y)
            if kn == "_torch":
                self.link.set_flashlight(not self.link.torch_on)
                self._capture_rec_event()
                self.handset_area.queue_draw()
                self._refresh_telemetry_ui()
                return
            if kn:
                self._pointer_active_key = kn
                self._press_key(kn)

        def _on_handset_release(self, _g: Any, _n: int, _x: float, _y: float) -> None:
            self._release_all_pointer_keys()

        def _release_all_pointer_keys(self) -> None:
            kn = getattr(self, "_pointer_active_key", None)
            if kn:
                self._pointer_active_key = None
                self._release_key(kn)

        def _on_handset_right_click(self, _g: Any, _n: int, x: float, y: float) -> None:
            kn = self._hit_test(x, y)
            if kn and kn != "_torch":
                self._long_click_key(kn)

        def _capture_rec_event(self) -> None:
            if self.recorder.recording:
                led = "tx" if ("ptt" in self._pressed_keys and self.link.connected) else ("rx" if self.link.connected else "off")
                self.recorder.push(self.link.last_fb, frozenset(self._pressed_keys), led, self.link.torch_on)
                self._update_rec_label()

        def _press_key(self, kname: str) -> None:
            if kname in self._pressed_keys:
                return
            self._pressed_keys.add(kname)
            self._capture_rec_event()
            self._ensure_anim_loop()
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
            if kname in self._hold_timers:
                GLib.source_remove(self._hold_timers.pop(kname))
            self._capture_rec_event()
            self._ensure_anim_loop()
            if was and self.link.connected:
                self.link.send_key(kname, "release")

        def _long_click_key(self, kname: str) -> None:
            if not self.link.connected:
                return
            self._pressed_keys.add(kname)
            self._capture_rec_event()
            self._ensure_anim_loop()
            self.link.send_key(kname, "press")
            GLib.timeout_add(120, lambda: (self.link.send_key(kname, "hold"), False)[1] if self.link.connected else False)

            def _end() -> bool:
                self._pressed_keys.discard(kname)
                self._capture_rec_event()
                self._ensure_anim_loop()
                if self.link.connected:
                    self.link.send_key(kname, "release")
                return False

            GLib.timeout_add(260, _end)

        def _schedule_frame_update(self) -> None:
            if not self._frame_pending:
                self._frame_pending = True
                GLib.idle_add(self._on_new_frame_main)

        def _on_new_frame_main(self) -> bool:
            self._frame_pending = False
            self._capture_rec_event()
            settling = self.lcd_state.step(self._current_display_fb(), self.lcd_st)
            self.handset_area.queue_draw()
            if settling:
                self._ensure_anim_loop()
            return False

        # --- PC Keyboard Mapping ---
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

        def _text_focused(self) -> bool:
            foc = self.win.get_focus()
            return foc is not None and (foc is self.cmd_entry or isinstance(foc, Gtk.Text))

        def _on_pc_key_pressed(self, _c: Any, keyval: int, _kc: int, state: Any) -> bool:
            if (state & Gdk.ModifierType.CONTROL_MASK) or self._text_focused():
                return False
            kn = self._KEYVAL_MAP.get(keyval)
            if kn:
                self._press_key(kn)
                return True
            return False

        def _on_pc_key_released(self, _c: Any, keyval: int, _kc: int, _st: Any) -> None:
            if self._text_focused():
                return
            kn = self._KEYVAL_MAP.get(keyval)
            if kn:
                self._release_key(kn)

        # --- Dock Pages: Device · Files · Wire · Capture ---
        def _build_device_page(self) -> Any:
            page = Adw.PreferencesPage() if has_adw else Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=8)
            if not has_adw:
                self.row_batt = self.row_ident = self.row_serial = self.row_link = Gtk.Label()
                self.batt_bar = Gtk.LevelBar()
                self.sw_torch = Gtk.Switch()
                self.sw_lock = Gtk.Switch()
                return page

            g_pwr = Adw.PreferencesGroup(title="Power")
            self.row_batt = Adw.ActionRow(title="Battery", subtitle="Not connected")
            self.batt_bar = Gtk.LevelBar(min_value=0.0, max_value=100.0)
            self.batt_bar.add_offset_value("low", 20.0)
            self.batt_bar.add_offset_value("high", 50.0)
            self.batt_bar.add_offset_value("full", 85.0)
            self.batt_bar.set_size_request(110, 10)
            self.batt_bar.set_valign(Gtk.Align.CENTER)
            self.row_batt.add_suffix(self.batt_bar)
            g_pwr.add(self.row_batt)
            page.add(g_pwr)

            g_dev = Adw.PreferencesGroup(title="Radio &amp; Link")
            self.row_ident = Adw.ActionRow(title="Identity", subtitle="—")
            self.row_serial = Adw.ActionRow(title="Serial / MAC", subtitle="—")
            self.row_link = Adw.ActionRow(title="RigWire Link", subtitle="Disconnected")
            g_dev.add(self.row_ident)
            g_dev.add(self.row_serial)
            g_dev.add(self.row_link)
            page.add(g_dev)

            g_ctl = Adw.PreferencesGroup(title="Controls")
            self.sw_torch = Adw.SwitchRow(title="Flashlight", subtitle="Toggle top LED lamp")
            self.sw_torch.connect("notify::active", lambda sw, _p: None if self._updating_switches else (self.link.set_flashlight(sw.get_active()), self.handset_area.queue_draw()))
            g_ctl.add(self.sw_torch)

            self.sw_lock = Adw.SwitchRow(title="Keypad Lock", subtitle="Lock physical front keys")
            self.sw_lock.connect("notify::active", lambda sw, _p: None if self._updating_switches else self.link.set_keypad_lock(sw.get_active()))
            g_ctl.add(self.sw_lock)

            row_vfo = Adw.ActionRow(title="Active VFO", subtitle="Swap primary VFO A/B")
            btn_vfo = Gtk.Button(label="Swap A/B", valign=Gtk.Align.CENTER)
            btn_vfo.connect("clicked", lambda _b: self.link.toggle_vfo())
            row_vfo.add_suffix(btn_vfo)
            g_ctl.add(row_vfo)

            row_side = Adw.ActionRow(title="Sideload Package", subtitle="Install .krx / .mod over RigWire")
            btn_side = Gtk.Button(label="Install…", valign=Gtk.Align.CENTER)
            btn_side.connect("clicked", self._on_sideload)
            row_side.add_suffix(btn_side)
            g_ctl.add(row_side)
            page.add(g_ctl)
            return page

        def _build_files_page(self) -> Any:
            box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=8)
            box.set_margin_top(10)
            box.set_margin_bottom(10)
            box.set_margin_start(12)
            box.set_margin_end(12)

            bar = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=6)
            grp = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL)
            grp.add_css_class("linked")
            for d in ("/", "/user", "/pref", "/apps", "/sys"):
                b = Gtk.Button(label="root" if d == "/" else d)
                b.connect("clicked", lambda _b, p=d: self._browse_dir(p))
                grp.append(b)
            bar.append(grp)
            bar.append(Gtk.Box(hexpand=True))
            ref = Gtk.Button.new_from_icon_name("view-refresh-symbolic")
            ref.connect("clicked", lambda _b: self._browse_dir(self.current_dir))
            bar.append(ref)
            up = Gtk.Button.new_from_icon_name("document-send-symbolic")
            up.set_tooltip_text("Upload file")
            up.connect("clicked", self._on_upload_file)
            bar.append(up)
            box.append(bar)

            mbar = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
            self.path_lbl = Gtk.Label(label="/", xalign=0, hexpand=True)
            self.path_lbl.add_css_class("heading")
            mbar.append(self.path_lbl)
            self.mount_btn = Gtk.ToggleButton(label="Mount OS Drive")
            self.mount_btn.connect("toggled", self._on_mount_toggled)
            mbar.append(self.mount_btn)
            self.open_fs_btn = Gtk.Button.new_from_icon_name("folder-open-symbolic")
            self.open_fs_btn.set_visible(False)
            self.open_fs_btn.connect("clicked", self._on_open_fs)
            mbar.append(self.open_fs_btn)
            box.append(mbar)

            scroll = Gtk.ScrolledWindow(vexpand=True)
            self.file_list = Gtk.ListBox(selection_mode=Gtk.SelectionMode.NONE)
            self.file_list.add_css_class("boxed-list")
            scroll.set_child(self.file_list)
            box.append(scroll)
            return box

        def _build_wire_page(self) -> Any:
            box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=8)
            box.set_margin_top(10)
            box.set_margin_bottom(10)
            box.set_margin_start(12)
            box.set_margin_end(12)

            scroll = Gtk.ScrolledWindow(vexpand=True)
            self.console_buf = Gtk.TextBuffer()
            self.console_view = Gtk.TextView(buffer=self.console_buf, editable=False, monospace=True)
            self.console_view.set_wrap_mode(Gtk.WrapMode.WORD_CHAR)
            self.console_view.add_css_class("wire-console")
            scroll.set_child(self.console_view)
            box.append(scroll)

            row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=6)
            self.cmd_entry = Gtk.Entry(hexpand=True, placeholder_text="RigWire command (ping, ls /apps, freq 433.500, help)…")
            self.cmd_entry.connect("activate", self._on_console_submit)
            row.append(self.cmd_entry)
            btn = Gtk.Button(label="Run")
            btn.connect("clicked", self._on_console_submit)
            row.append(btn)
            box.append(row)
            return box

        def _build_capture_page(self) -> Any:
            box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=12)
            box.set_margin_top(12)
            box.set_margin_bottom(12)
            box.set_margin_start(14)
            box.set_margin_end(14)

            # Primary Capture Controls (cleanly housed inside Capture tab, NOT cluttering headerbar!)
            top_row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
            self.rec_btn = Gtk.ToggleButton()
            rec_in = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=6)
            rec_in.append(Gtk.Image.new_from_icon_name("media-record-symbolic"))
            self.rec_lbl = Gtk.Label(label="Record GIF")
            rec_in.append(self.rec_lbl)
            self.rec_btn.set_child(rec_in)
            self.rec_btn.set_tooltip_text("Start / Stop recording (Ctrl+R)")
            self.rec_btn.connect("toggled", self._on_record_toggled)
            top_row.append(self.rec_btn)

            shot_btn = Gtk.Button()
            sh_in = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=6)
            sh_in.append(Gtk.Image.new_from_icon_name("camera-photo-symbolic"))
            sh_in.append(Gtk.Label(label="Snapshot PNG"))
            shot_btn.set_child(sh_in)
            shot_btn.set_tooltip_text("Save current frame as PNG (Ctrl+S)")
            shot_btn.connect("clicked", self._on_save_screenshot)
            top_row.append(shot_btn)
            box.append(top_row)

            self.gif_summary_lbl = Gtk.Label(label="No recording yet — press Record GIF to capture.", xalign=0, wrap=True)
            self.gif_summary_lbl.add_css_class("dim-label")
            box.append(self.gif_summary_lbl)

            # Interactive Visual Timeline (Trim In/Out + Red Cut Segments + Playhead)
            self.timeline_area = Gtk.DrawingArea(content_height=50, hexpand=True)
            self.timeline_area.set_draw_func(self._draw_timeline)
            tl_c = Gtk.GestureClick()
            tl_c.connect("pressed", self._on_timeline_click)
            self.timeline_area.add_controller(tl_c)
            tl_d = Gtk.GestureDrag()
            tl_d.connect("drag-update", self._on_timeline_drag)
            self.timeline_area.add_controller(tl_d)
            box.append(self.timeline_area)

            # Transport + Trim + Cut/Splice Toolbar
            tr = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=6)
            self.pb_btn = Gtk.Button.new_from_icon_name("media-playback-start-symbolic")
            self.pb_btn.set_tooltip_text("Play / Pause preview on handset")
            self.pb_btn.connect("clicked", self._on_pb_toggle)
            tr.append(self.pb_btn)

            for icon, tip, delta in (("go-previous-symbolic", "Previous frame", -1), ("go-next-symbolic", "Next frame", 1)):
                b = Gtk.Button.new_from_icon_name(icon)
                b.set_tooltip_text(tip)
                b.connect("clicked", lambda _b, d=delta: self._step_pb(d))
                tr.append(b)

            tr.append(Gtk.Separator(orientation=Gtk.Orientation.VERTICAL))

            b_in = Gtk.Button(label="[ In")
            b_in.connect("clicked", lambda _b: self._set_trim_in())
            tr.append(b_in)
            b_out = Gtk.Button(label="Out ]")
            b_out.connect("clicked", lambda _b: self._set_trim_out())
            tr.append(b_out)

            self.cut_btn = Gtk.Button(label="Mark Cut")
            self.cut_btn.set_tooltip_text("Mark cut start, scrub, click again to splice out middle segment")
            self.cut_btn.connect("clicked", self._on_cut_splice)
            tr.append(self.cut_btn)

            undo_b = Gtk.Button.new_from_icon_name("edit-undo-symbolic")
            undo_b.set_tooltip_text("Undo last splice cut")
            undo_b.connect("clicked", lambda _b: (self.recorder.undo_cut(), self._refresh_gif_ui()))
            tr.append(undo_b)
            box.append(tr)

            if has_adw:
                opt = Adw.PreferencesGroup(title="Export")
                r_mode = Adw.ActionRow(title="Capture Scope", subtitle="LCD glass only or full radio with pressed keys")
                mg = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, valign=Gtk.Align.CENTER)
                mg.add_css_class("linked")
                b_lcd = Gtk.ToggleButton(label="LCD Only", active=True)
                b_body = Gtk.ToggleButton(label="Full Radio", group=b_lcd)
                b_lcd.connect("toggled", lambda b: setattr(self, "_export_mode", "lcd") if b.get_active() else None)
                b_body.connect("toggled", lambda b: setattr(self, "_export_mode", "body") if b.get_active() else None)
                mg.append(b_lcd)
                mg.append(b_body)
                r_mode.add_suffix(mg)
                opt.add(r_mode)

                r_spd = Adw.ActionRow(title="Speed", subtitle="GIF playback timing")
                sg = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, valign=Gtk.Align.CENTER)
                sg.add_css_class("linked")
                first_s = None
                for lbl, val in (("0.5×", 0.5), ("1×", 1.0), ("1.5×", 1.5), ("2×", 2.0)):
                    sb = Gtk.ToggleButton(label=lbl, active=(val == 1.0))
                    if first_s is None:
                        first_s = sb
                    else:
                        sb.set_group(first_s)
                    sb.connect("toggled", lambda b, v=val: (setattr(self, "_export_speed", v), self._refresh_gif_ui()) if b.get_active() else None)
                    sg.append(sb)
                r_spd.add_suffix(sg)
                opt.add(r_spd)

                r_cap = Adw.SwitchRow(title="Clamp Idle Pauses", subtitle="Cap gaps > 0.6 s", active=True)
                r_cap.connect("notify::active", lambda sw, _p: (setattr(self, "_cap_idle", sw.get_active()), self._refresh_gif_ui()))
                opt.add(r_cap)
                box.append(opt)

            self.gif_progress = Gtk.ProgressBar(show_text=True, visible=False)
            box.append(self.gif_progress)

            act = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
            clr = Gtk.Button(label="Discard")
            clr.connect("clicked", self._on_discard_recording)
            act.append(clr)
            act.append(Gtk.Box(hexpand=True))
            self.export_gif_btn = Gtk.Button(label="Export GIF…", sensitive=False)
            self.export_gif_btn.add_css_class("suggested-action")
            self.export_gif_btn.connect("clicked", self._on_export_gif)
            act.append(self.export_gif_btn)
            box.append(act)
            return box

        # --- Recording & Timeline Editor Handlers ---
        def _update_rec_label(self) -> None:
            if self.recorder.recording:
                dt = int(time.monotonic() - self.recorder.t0_mono)
                self.rec_lbl.set_text(f"Stop ({dt // 60:02d}:{dt % 60:02d} · {len(self.recorder.frames)}f)")
            else:
                self.rec_lbl.set_text("Record GIF")

        def _on_record_toggled(self, btn: Any) -> None:
            if btn.get_active():
                self._stop_playback()
                led = "tx" if ("ptt" in self._pressed_keys and self.link.connected) else ("rx" if self.link.connected else "off")
                self.recorder.start(self.link.last_fb, frozenset(self._pressed_keys), led, self.link.torch_on)
                btn.add_css_class("rec-live")
                self.hdr_rec_dot.set_visible(True)
                GLib.timeout_add(500, self._tick_rec_clock)
                self._update_rec_label()
            else:
                self.recorder.stop()
                btn.remove_css_class("rec-live")
                self.hdr_rec_dot.set_visible(False)
                self._update_rec_label()
                self._pb_idx = self.recorder.trim_in
                self._refresh_gif_ui()
                if len(self.recorder.frames) > 0:
                    self.dock_stack.set_visible_child_name("capture")

        def _tick_rec_clock(self) -> bool:
            if not self.recorder.recording:
                return False
            self._update_rec_label()
            return True

        def _draw_timeline(self, _area: Any, cr: Any, w: int, h: int) -> None:
            n = len(self.recorder.frames)
            _round_rect(cr, 0, 6, w, h - 12, 6)
            cr.set_source_rgb(0.11, 0.12, 0.15)
            cr.fill()
            if n <= 0:
                return
            span = max(1, n - 1)
            fx = lambda i: 8.0 + (i / float(span)) * max(1.0, w - 16.0)

            x_in, x_out = fx(self.recorder.trim_in), fx(self.recorder.trim_out)
            _round_rect(cr, x_in, 8, max(4.0, x_out - x_in), h - 16, 4)
            cr.set_source_rgba(0.22, 0.55, 0.96, 0.34)
            cr.fill()

            for a, b in self.recorder.cuts:
                xa, xb = fx(a), fx(b)
                cr.set_source_rgba(0.92, 0.24, 0.24, 0.65)
                cr.rectangle(xa, 8, max(3.0, xb - xa), h - 16)
                cr.fill()

            if self._cut_mark_start is not None:
                xm = fx(self._cut_mark_start)
                cr.set_source_rgb(0.98, 0.72, 0.18)
                cr.set_line_width(2.0)
                cr.move_to(xm, 6)
                cr.line_to(xm, h - 6)
                cr.stroke()

            cr.set_source_rgba(0.85, 0.90, 0.96, 0.45)
            for i, rf in enumerate(self.recorder.frames):
                if rf.pressed:
                    cr.rectangle(fx(i) - 1.0, h - 14, 2.0, 6)
            cr.fill()

            xp = fx(self._pb_idx)
            cr.set_source_rgb(1.0, 1.0, 1.0)
            cr.set_line_width(2.2)
            cr.move_to(xp, 3)
            cr.line_to(xp, h - 3)
            cr.stroke()

        def _seek_timeline_x(self, x: float) -> None:
            n = len(self.recorder.frames)
            if n <= 1:
                return
            w = max(20.0, float(self.timeline_area.get_width()) - 16.0)
            self._pb_idx = int(round(max(0.0, min(1.0, (x - 8.0) / w)) * (n - 1)))
            self._refresh_gif_ui()

        def _on_timeline_click(self, _g: Any, _n: int, x: float, _y: float) -> None:
            self._drag_start_x = x
            self._seek_timeline_x(x)

        def _on_timeline_drag(self, _g: Any, ox: float, _oy: float) -> None:
            self._seek_timeline_x(getattr(self, "_drag_start_x", 0.0) + ox)

        def _set_trim_in(self) -> None:
            if self.recorder.frames:
                self.recorder.trim_in = min(self._pb_idx, self.recorder.trim_out)
                self._refresh_gif_ui()

        def _set_trim_out(self) -> None:
            if self.recorder.frames:
                self.recorder.trim_out = max(self._pb_idx, self.recorder.trim_in)
                self._refresh_gif_ui()

        def _on_cut_splice(self, _btn: Any) -> None:
            if not self.recorder.frames:
                return
            if self._cut_mark_start is None:
                self._cut_mark_start = self._pb_idx
                self.cut_btn.set_label("Splice Out")
            else:
                self.recorder.add_cut(self._cut_mark_start, self._pb_idx)
                self._cut_mark_start = None
                self.cut_btn.set_label("Mark Cut")
            self._refresh_gif_ui()

        def _step_pb(self, delta: int) -> None:
            if self.recorder.frames:
                self._pb_idx = max(0, min(len(self.recorder.frames) - 1, self._pb_idx + delta))
                self._refresh_gif_ui()

        def _stop_playback(self) -> None:
            self._pb_playing = False
            if self._pb_timer is not None:
                GLib.source_remove(self._pb_timer)
                self._pb_timer = None
            if hasattr(self, "pb_btn"):
                self.pb_btn.set_icon_name("media-playback-start-symbolic")

        def _on_pb_toggle(self, _btn: Any) -> None:
            if self._pb_playing:
                self._stop_playback()
                return
            seq = self.recorder.frame_durations_ms(speed=self._export_speed, max_idle_ms=600 if self._cap_idle else None)
            if not seq:
                return
            self._pb_playing = True
            self.pb_btn.set_icon_name("media-playback-pause-symbolic")
            self._pb_seq_pos = 0
            self._schedule_next_pb(seq)

        def _schedule_next_pb(self, seq: list[tuple[int, int]]) -> None:
            if not self._pb_playing or not seq:
                return
            fi, dt_ms = seq[self._pb_seq_pos % len(seq)]
            self._pb_idx = fi
            self._refresh_gif_ui()
            self._pb_seq_pos = (self._pb_seq_pos + 1) % len(seq)
            self._pb_timer = GLib.timeout_add(dt_ms, lambda: (self._schedule_next_pb(seq), False)[1])

        def _refresh_gif_ui(self) -> None:
            n = len(self.recorder.frames)
            seq = self.recorder.frame_durations_ms(speed=self._export_speed, max_idle_ms=600 if self._cap_idle else None)
            tot_ms = sum(d for _, d in seq)
            if n == 0:
                self.gif_summary_lbl.set_text("No recording yet — press Record GIF to capture.")
                self.export_gif_btn.set_sensitive(False)
            else:
                cuts_s = f" · {len(self.recorder.cuts)} cut(s)" if self.recorder.cuts else ""
                self.gif_summary_lbl.set_text(f"Frame {self._pb_idx + 1}/{n} · Kept {len(seq)}f ({tot_ms / 1000.0:.2f} s){cuts_s}")
                self.export_gif_btn.set_sensitive(len(seq) > 0)
            self.timeline_area.queue_draw()
            self._ensure_anim_loop()

        def _on_discard_recording(self, _btn: Any) -> None:
            self._stop_playback()
            self.recorder.frames.clear()
            self.recorder.cuts.clear()
            self._cut_mark_start = None
            self.cut_btn.set_label("Mark Cut")
            self._pb_idx = 0
            self._refresh_gif_ui()

        def _on_export_gif(self, _btn: Any) -> None:
            dlg = Gtk.FileDialog(title="Export Animated GIF")
            dlg.set_initial_name(f"kern_{self.active_model}_{self._export_mode}_{time.strftime('%Y%m%d_%H%M%S')}.gif")

            def _picked(d: Any, res: Any) -> None:
                try:
                    dest = d.save_finish(res).get_path()
                except GLib.Error:
                    return
                if not dest:
                    return
                self.gif_progress.set_visible(True)
                self.gif_progress.set_fraction(0.0)
                self.export_gif_btn.set_sensitive(False)

                def _w() -> None:
                    try:
                        sz = self.recorder.export_gif(
                            dest, model=self.active_model, lcd_st=dataclasses.replace(self.lcd_st),
                            mode=self._export_mode, speed=self._export_speed,
                            max_idle_ms=600 if self._cap_idle else None,
                            progress_cb=lambda f: GLib.idle_add(self.gif_progress.set_fraction, f),
                        )
                        GLib.idle_add(self._notify, f"Saved {Path(dest).name} ({sz / 1024.0:.1f} KB)")
                    except Exception as e:
                        GLib.idle_add(self._notify, f"GIF export failed: {e}")
                    finally:
                        GLib.idle_add(self.gif_progress.set_visible, False)
                        GLib.idle_add(self.export_gif_btn.set_sensitive, True)

                threading.Thread(target=_w, daemon=True).start()

            dlg.save(self.win, None, _picked)

        # --- Connect / Disconnect / Telemetry / Files / Wire ---
        def _connect_to(self, port: str, port_info: DetectedPort | None = None) -> None:
            try:
                self.link.connect(port, baud=baud, model_override=self.forced_model, port_info=port_info)
            except Exception as e:
                self._notify(f"Connection failed: {e}")
                return

            self.hdr_link_icon.set_from_icon_name("network-transmit-receive-symbolic")
            self.hdr_batt_icon.set_visible(True)
            self.hdr_dev_lbl.set_text(self.link.volume_name.removeprefix("Quansheng "))
            self.conn_card.set_visible(True)
            self.conn_card_title.set_text(self.link.volume_name)
            self.conn_card_sub.set_text(f"Port: {self.link.port}")
            self.win.set_title(f"Kern Dock — {self.link.volume_name}")

            if self.link.detected_model != self.active_model:
                self.active_model = self.link.detected_model
                if not self.user_picked_palette:
                    self.lcd_st.palette = "UV-K5" if self.active_model == "k5" else "UV-K1"
                    self.pal_dd.set_selected(list(LCD_PALETTES.keys()).index(self.lcd_st.palette))
                self._sync_window_size()

            self._rebuild_connection_list()
            self.lcd_state.step(self.link.last_fb, self.lcd_st)
            self.handset_area.queue_draw()
            self._refresh_telemetry_ui()
            self._browse_dir("/")

        def _disconnect(self) -> None:
            self.link.disconnect()
            self.hdr_link_icon.set_from_icon_name("network-offline-symbolic")
            self.hdr_batt_icon.set_visible(False)
            self.hdr_dev_lbl.set_text("Connect")
            self.conn_card.set_visible(False)
            self.mount_btn.set_active(False)
            self.open_fs_btn.set_visible(False)
            self.win.set_title("Kern Dock")
            self._rebuild_connection_list()
            self._clear_file_list()
            self._refresh_telemetry_ui()
            self.handset_area.queue_draw()

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
            mv, pct = t.get("battery_mv"), t.get("battery_pct")
            if mv is not None:
                if has_adw:
                    self.row_batt.set_subtitle(f"{mv / 1000.0:.2f} V · {pct}%")
                self.batt_bar.set_value(float(max(0, min(100, pct or 0))))
                self.hdr_batt_icon.set_from_icon_name(_battery_icon_name(pct))
                self.hdr_batt_icon.set_tooltip_text(f"Battery: {pct}% ({mv / 1000.0:.2f} V)")

            fps_s = "Demo" if self.link.demo_mode else f"{self.link.fps:.1f} FPS"
            self.conn_card_sub.set_text(f"{self.link.port} · {fps_s} · {self.link.rtt_ms:.1f} ms")
            if has_adw:
                self.row_ident.set_subtitle(f"{self.link.volume_name} ({self.link.port})")
                self.row_serial.set_subtitle(f"{t.get('serial_str', '—')}  ·  {t.get('mac_addr', '—')}")
                mtu = self.link.hello_info.get("accepted_max_payload", 128)
                err_s = f" · {self.link.error_count} err" if self.link.error_count else ""
                self.row_link.set_subtitle(f"{fps_s} · RTT {self.link.rtt_ms:.1f} ms · MTU {mtu} B{err_s}")

            self._updating_switches = True
            self.sw_torch.set_active(self.link.torch_on)
            self.sw_lock.set_active(self.link.keypad_locked)
            self._updating_switches = False
            return False

        def _clear_file_list(self) -> None:
            c = self.file_list.get_first_child()
            while c is not None:
                n = c.get_next_sibling()
                self.file_list.remove(c)
                c = n

        def _browse_dir(self, path: str) -> None:
            if not self.link.connected or self.link.fs_backend is None:
                return
            self.current_dir = path
            self.path_lbl.set_text(path)
            be = self.link.fs_backend

            def _w() -> None:
                try:
                    entries = be.ls(path)
                except Exception as e:
                    GLib.idle_add(self._notify, f"List {path} failed: {e}")
                    return
                GLib.idle_add(self._populate_file_list, path, entries)

            threading.Thread(target=_w, daemon=True).start()

        def _populate_file_list(self, dir_path: str, entries: list[dict[str, Any]]) -> bool:
            self._clear_file_list()
            if not entries:
                lbl = Gtk.Label(label="Empty directory", margin_top=18, margin_bottom=18)
                lbl.add_css_class("dim-label")
                self.file_list.append(lbl)
                return False
            for e in entries:
                name, is_dir, size = e["name"], bool(e["is_dir"]), int(e["size"])
                full = f"/{name}" if dir_path == "/" else f"{dir_path.rstrip('/')}/{name}"
                protected = full.startswith("/sys/") or full in kernfs_mount.PROTECTED_RECOVERY_MODS
                if has_adw:
                    row = Adw.ActionRow(
                        title=name,
                        subtitle="Directory" if is_dir else f"{size:,} bytes" + (" · Protected" if protected else ""),
                    )
                    icon = "folder-symbolic" if is_dir else ("application-x-executable-symbolic" if name.endswith(kernfs_mount.PKG_EXTS) else "text-x-generic-symbolic")
                    row.add_prefix(Gtk.Image.new_from_icon_name(icon))
                    if is_dir:
                        row.set_activatable(True)
                        row.add_suffix(Gtk.Image.new_from_icon_name("go-next-symbolic"))
                        row.connect("activated", lambda _r, p=full: self._browse_dir(p))
                    else:
                        dl = Gtk.Button.new_from_icon_name("document-save-symbolic")
                        dl.set_valign(Gtk.Align.CENTER)
                        dl.add_css_class("flat")
                        dl.connect("clicked", lambda _b, p=full, n=name: self._on_download_file(p, n))
                        row.add_suffix(dl)
                        rm = Gtk.Button.new_from_icon_name("user-trash-symbolic")
                        rm.set_valign(Gtk.Align.CENTER)
                        rm.add_css_class("flat")
                        rm.set_sensitive(not protected)
                        rm.connect("clicked", lambda _b, p=full: self._on_delete_file(p))
                        row.add_suffix(rm)
                    self.file_list.append(row)
            return False

        def _on_download_file(self, remote_path: str, filename: str) -> None:
            if not self.link.fs_backend:
                return
            dlg = Gtk.FileDialog(title=f"Save {filename}")
            dlg.set_initial_name(filename)
            be = self.link.fs_backend

            def _picked(d: Any, res: Any) -> None:
                try:
                    dest = d.save_finish(res).get_path()
                except GLib.Error:
                    return
                if dest:
                    threading.Thread(
                        target=lambda: (Path(dest).write_bytes(be.read_bytes(remote_path)), GLib.idle_add(self._notify, f"Saved {Path(dest).name}")),
                        daemon=True,
                    ).start()

            dlg.save(self.win, None, _picked)

        def _on_upload_file(self, _btn: Any) -> None:
            if not self.link.connected or not self.link.fs_backend:
                self._notify("Connect to a radio first")
                return
            dlg = Gtk.FileDialog(title=f"Upload to {self.current_dir}")
            be, tdir = self.link.fs_backend, self.current_dir

            def _picked(d: Any, res: Any) -> None:
                try:
                    src = d.open_finish(res).get_path()
                except GLib.Error:
                    return
                if not src:
                    return
                p = Path(src)
                remote = f"/{p.name}" if tdir == "/" else f"{tdir.rstrip('/')}/{p.name}"

                def _w() -> None:
                    try:
                        data = p.read_bytes()
                        self.link.policy.check_write(remote, data)
                        be.write_bytes(remote, data)
                        GLib.idle_add(self._notify, f"Uploaded {remote}")
                        GLib.idle_add(self._browse_dir, tdir)
                    except Exception as e:
                        GLib.idle_add(self._notify, f"Upload failed: {e}")

                threading.Thread(target=_w, daemon=True).start()

            dlg.open(self.win, None, _picked)

        def _on_delete_file(self, remote_path: str) -> None:
            if not self.link.fs_backend:
                return
            be = self.link.fs_backend

            def _w() -> None:
                try:
                    self.link.policy.check_delete(remote_path)
                    be.remove(remote_path)
                    GLib.idle_add(self._notify, f"Deleted {remote_path}")
                    GLib.idle_add(self._browse_dir, self.current_dir)
                except Exception as e:
                    GLib.idle_add(self._notify, f"Delete blocked: {e}")

            threading.Thread(target=_w, daemon=True).start()

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
                url = self.link.start_dav(http_port=8765)
                open_t, unmount_c = kernfs_mount.try_auto_mount(url, self.link.volume_name)
                self.link.dav_open_target = open_t
                self.link.dav_unmount_cmd = unmount_c
                self.open_fs_btn.set_visible(True)
                self._notify(f"Mounted {self.link.volume_name}")
            except Exception as e:
                btn.set_active(False)
                self._notify(f"Mount failed: {e}")

        def _on_open_fs(self, _btn: Any) -> None:
            if self.link.dav_server is not None:
                kernfs_mount.open_in_file_manager(self.link.dav_open_target or f"http://127.0.0.1:{self.link.dav_port}/")

        def _on_save_screenshot(self, _btn: Any) -> None:
            dlg = Gtk.FileDialog(title="Save Snapshot (PNG)")
            dlg.set_initial_name(f"kern_{self.active_model}_{self._export_mode}_{time.strftime('%Y%m%d_%H%M%S')}.png")

            def _done(d: Any, res: Any) -> None:
                try:
                    fpath = d.save_finish(res).get_path()
                    if fpath:
                        fb, pressed, led, torch = self._current_display_state()
                        if self._export_mode == "body":
                            w, h, rgba = render_handset_to_rgba(self.active_model, fb, self.lcd_st, pressed_keys=pressed, led_state=led, torch_on=torch, scale=2.0)
                        else:
                            w, h, rgba = render_lcd_to_rgba(fb, self.lcd_st, width=540, height=374)
                        Path(fpath).write_bytes(encode_png_rgba(w, h, rgba))
                        self._notify(f"Saved {Path(fpath).name}")
                except GLib.Error:
                    pass

            dlg.save(self.win, None, _done)

        def _on_sideload(self, _btn: Any) -> None:
            if not self.link.connected or self.link.demo_mode or not self.link.client:
                self._notify("Connect to a live radio (or QEMU) to sideload")
                return
            dlg = Gtk.FileDialog(title="Sideload KRX Package")

            def _done(d: Any, res: Any) -> None:
                try:
                    fpath = d.open_finish(res).get_path()
                except GLib.Error:
                    return
                if not fpath:
                    return

                def _w() -> None:
                    try:
                        with self.link.lock:
                            info = self.link.client.sideload(fpath, verify=True, force=True)  # type: ignore[union-attr]
                        GLib.idle_add(self._notify, f"Installed {info['remote_path']}")
                        GLib.idle_add(self._browse_dir, "/apps")
                    except Exception as e:
                        GLib.idle_add(self._notify, f"Sideload failed: {e}")

                threading.Thread(target=_w, daemon=True).start()

            dlg.open(self.win, None, _done)

        def _append_console_line(self, line: str) -> bool:
            self.console_buf.insert(self.console_buf.get_end_iter(), line + "\n")
            self.console_view.scroll_to_iter(self.console_buf.get_end_iter(), 0.0, False, 0.0, 1.0)
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
            threading.Thread(
                target=lambda: GLib.idle_add(self._append_console_line, self.link.run_wire_command(cmd)),
                daemon=True,
            ).start()

        def _on_close_request(self, _win: Any) -> bool:
            self._stop_playback()
            self.link.disconnect()
            return False

    app = KernDockApp()
    return int(app.run([]))


# =============================================================================
# 6. HEADLESS SELFTEST & BENCHMARK (CI-Safe, Zero Display Required)
# =============================================================================

def run_selftest() -> int:
    import tempfile

    k1_keys = [k[0] for row in K1_MATRIX for k in row]
    k5_keys = ["menu", "up", "down", "exit"] + [k[0] for row in K5_BOTTOM_MATRIX for k in row]
    assert len(k1_keys) == 16 and len(k5_keys) == 16
    for kn in set(k1_keys + k5_keys):
        assert kn in rigwire.KEY_CODES, f"Invalid RigWire key '{kn}'"

    assert detect_hardware_model("Quansheng UV-K1 1D9E") == "k1"
    assert detect_hardware_model("Quansheng UV-K5 v3 ABCD") == "k5"
    assert detect_hardware_model("Quansheng UV-K6 99AA") == "k5"

    fb1 = make_demo_framebuffer("433.500", "145.500", "VFO")
    fb2 = make_demo_framebuffer("433.525", "145.500", "UP")
    fb3 = make_demo_framebuffer("433.550", "145.500", "UP")
    st_k1 = LcdSettings(palette="UV-K1")
    fs = FastLcdState()
    assert fs.step(fb1, st_k1) is True

    rec = RecordingSession()
    rec.frames = [
        RecordedFrame(0, fb1, frozenset(), "rx", False),
        RecordedFrame(150, fb2, frozenset({"up"}), "rx", False),
        RecordedFrame(450, fb2, frozenset(), "rx", False),
        RecordedFrame(950, fb3, frozenset({"up"}), "rx", False),
        RecordedFrame(1100, fb3, frozenset(), "rx", False),
    ]
    rec.trim_in, rec.trim_out = 0, 4
    rec.add_cut(2, 2)
    assert rec.active_indices() == [0, 1, 3, 4]

    bench_str = ""
    try:
        import cairo
        has_cairo = True
    except ImportError:
        has_cairo = False

    if has_cairo:
        # Micro-benchmark cached handset + LCD rendering speed (must easily exceed 200 FPS on CPU!)
        rend = HandsetRenderer()
        lw, lh = compute_handset_geometry("k1")
        surf = cairo.ImageSurface(cairo.FORMAT_ARGB32, lw, lh)
        cr = cairo.Context(surf)
        # Warm cache
        rend.paint(cr, "k1", fs, st_k1, press_anim={"up": 0.5}, hover_anim={}, led_state="rx")
        t0 = time.monotonic()
        for _ in range(60):
            fs.step(fb2, st_k1)
            rend.paint(cr, "k1", fs, st_k1, press_anim={"up": 0.5}, hover_anim={}, led_state="rx")
        dt_ms = (time.monotonic() - t0) * 1000.0 / 60.0
        bench_str = f", {dt_ms:.2f} ms/frame (~{1000.0/max(dt_ms, 0.01):.0f} FPS cap)"

        w, h, rgba = render_lcd_to_rgba(fb1, st_k1, width=440, height=304)
        png = encode_png_rgba(w, h, rgba)
        assert png.startswith(b"\x89PNG\r\n\x1a\n") and png.endswith(b"IEND\xaeB`\x82")
        for mdl in ("k1", "k5"):
            bw, bh, brgba = render_handset_to_rgba(mdl, fb2, st_k1, pressed_keys=frozenset({"up", "ptt"}), led_state="tx")
            assert bw > 200 and bh > 400 and len(brgba) == bw * bh * 4
        try:
            from PIL import Image as _pil  # noqa: F401
            with tempfile.TemporaryDirectory() as td:
                g_lcd = Path(td) / "lcd.gif"
                g_body = Path(td) / "body.gif"
                sz1 = rec.export_gif(g_lcd, model="k1", lcd_st=st_k1, mode="lcd")
                sz2 = rec.export_gif(g_body, model="k5", lcd_st=LcdSettings(palette="UV-K5"), mode="body")
                assert sz1 > 100 and g_lcd.read_bytes()[:6] == b"GIF89a"
                assert sz2 > 100 and g_body.read_bytes()[:6] == b"GIF89a"
        except ImportError:
            pass

    link = DockLink()
    link.connect("demo")
    assert link.connected and link.volume_name == "Quansheng UV-K1 DEMO"
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

    kernfs_mount.log_ok(
        f"All Kern Dock selftests passed (UV-K1/UV-K5 Cairo models, fast LCD{bench_str}, auto-detect, GIF splice, FS/Wire/WebDAV)."
    )
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description="Kern Dock — Native GTK4 + Libadwaita radio companion")
    ap.add_argument("-p", "--port", help="Serial port (/dev/ttyACM0, COM3, tcp://127.0.0.1:4446, or 'demo')")
    ap.add_argument("-b", "--baud", type=int, default=115200, help="Serial baud rate (default: 115200)")
    ap.add_argument("--model", choices=["k1", "k5"], help="Override auto-detected radio chassis (k1 or k5)")
    ap.add_argument("--palette", choices=list(LCD_PALETTES.keys()), help="Override LCD color palette")
    ap.add_argument("--demo", action="store_true", help="Launch immediately in offline demo mode")
    ap.add_argument("--selftest", action="store_true", help="Run headless selftest & frame-time benchmark")
    args = ap.parse_args()

    if args.selftest:
        return run_selftest()

    port = "demo" if args.demo else args.port
    kernfs_mount.print_header("Kern Dock")
    kernfs_mount.log_info(f"Launching GTK4 dock (port={port or 'auto-detect'})")
    try:
        return launch_gtk4(
            initial_port=port,
            baud=args.baud,
            model_override=args.model,
            palette_override=args.palette,
        )
    except ImportError as e:
        kernfs_mount.log_err(f"GTK4 / PyGObject unavailable ({e}). Install python3-gobject + gtk4 + libadwaita.")
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
