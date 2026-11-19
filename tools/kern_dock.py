#!/usr/bin/env python3
"""
Kern Dock — Native GTK4 + Libadwaita Hardware Companion for Kern OS.

Features:
  • 3D-sculpted hardware models with PangoCairo typography (Adwaita Sans)
    modeled from official Quansheng orthographic renders & auto-detected via
    GUdev USB descriptors + RigWire HELLO:
      - Quansheng UV-K1(8): brushed champagne-titanium 3D armor, hexagonal Torx
        bolts, glossy Quansheng RF-wave logo bar, stubby A1 antenna, cobalt-blue
        rotary knob & 3D contoured PTT paddle, frosted light-pipe status LED,
        12 inter-row speaker grille slots, raised rubber island 4x4 keypad
      - Quansheng UV-K5 v3 / UV-K6: matte polycarbonate shell, brushed silver
        upper armor, vivid orange knob, vertical left M / ▲ / ▼ / EXIT column
        beside the 3-rib speaker dish, and lower 4x3 keypad
  • Blazing-fast cached Cairo + NumPy ST7565 LCD engine (~600 FPS CPU cap)
    with crisp tight unpixelated glass margin, 1.3x taller-than-wide pixels,
    asymmetric STN ghosting, contrast & backlight curves, rear-polarizer shadow
  • Automatic DFU / Stock Bootloader / CPS Flasher recognition:
      - Detects Kern OS (RigWire), Stock Quansheng Bootloader / CPS (36b7:0001
        or 38400-baud 0x0518 beacon), and Moto UF2 Bootloader (36b7:ffff)
      - Native Adw.Banner alerts + in-app .kfw / .bin flasher with live progress
      - One-click Factory Calibration & Full KernFS .kfw Backup / Safe Restore
  • Full GTK4 + Libadwaita workspace:
      - Device (battery LevelBar, identity, link FPS/RTT, hardware controls)
      - Files  (searchable KernFS browser + anti-brick shields + WebDAV OS mount)
      - Flash  (DFU/CPS firmware flasher + verified .kfw calibration & FS backups)
      - Wire   (interactive RigWire command console)
      - Capture (PNG snapshot + non-destructive GIF recorder with trim/cut/splice)
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
import kfw  # noqa: E402

try:
    import flasher as _flasher  # noqa: E402
except Exception:
    _flasher = None  # type: ignore[assignment]

try:
    import numpy as _np
except ImportError:
    _np = None  # type: ignore[assignment]

# =============================================================================
# 1. BLAZING-FAST ST7565 LCD ENGINE (Vectorized NumPy + Cached Cairo Layers)
# =============================================================================

LCD_W = 128
LCD_H = 64

LCD_PALETTES: dict[str, dict[str, Any]] = {
    "UV-K1":         {"bg": (0xC9, 0xF7, 0xEC), "fg": (0x26, 0x20, 0x83), "dark": False},
    "UV-K5":         {"bg": (0xFF, 0x99, 0x00), "fg": (0x0E, 0x0E, 0x0E), "dark": False},
    "Classic Green": {"bg": (0x9C, 0xA9, 0x88), "fg": (0x2A, 0x2A, 0x2A), "dark": False},
    "UV-5R Blue":    {"bg": (0x39, 0xAE, 0xF1), "fg": (0x10, 0x10, 0x10), "dark": False},
    "OLED White":    {"bg": (0x10, 0x10, 0x10), "fg": (0xF0, 0xF0, 0xF0), "dark": True},
}

STN_ATTACK = 0.72
STN_DECAY = 0.24


@dataclasses.dataclass
class LcdSettings:
    palette: str = "UV-K1"
    pixel_aspect: float = 1.30
    pixel_grid: bool = True
    invert_lcd: bool = False
    ghosting: bool = True
    contrast: int = 13
    backlight: int = 8
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
        bg[0] + (fg_raw[0] - bg[0]) * 0.065,
        bg[1] + (fg_raw[1] - bg[1]) * 0.065,
        bg[2] + (fg_raw[2] - bg[2]) * 0.065,
    )
    return bg, fg, grid, bool(pal["dark"])


class FastLcdState:
    """Vectorized ST7565 pixel state emitting a fresh GSK-safe 128x64 Cairo ARGB32 surface."""

    def __init__(self) -> None:
        if _np is not None:
            self._ghost_np = _np.zeros((LCD_H, LCD_W), dtype=_np.float32)
            self._shifts = _np.arange(8, dtype=_np.uint8).reshape(8, 1)
            self._bgra_np = _np.zeros((LCD_H, LCD_W, 4), dtype=_np.uint8)
        else:
            self._ghost_py: list[float] = [0.0] * (LCD_W * LCD_H)

    def step(self, fb: bytes, st: LcdSettings) -> bool:
        if len(fb) < 1024:
            fb = fb.ljust(1024, b"\x00")
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

        inv, use_g, settling, g = st.invert_lcd, st.ghosting, False, self._ghost_py
        idx = 0
        for y in range(LCD_H):
            po, m = (y >> 3) * 128, 1 << (y & 7)
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
        for i, a in enumerate(self._ghost_py):
            if a > 0.01:
                p = i * 4
                buf[p] = int(fb_c * a)
                buf[p + 1] = int(fg_c * a)
                buf[p + 2] = int(fr * a)
                buf[p + 3] = int(255.0 * a)
        return cairo.ImageSurface.create_for_data(buf, cairo.FORMAT_ARGB32, LCD_W, LCD_H, LCD_W * 4)


def _compute_lcd_active_rect(w: float, h: float, aspect: float) -> tuple[float, float, float, float, float, float]:
    """Tight, authentic ST7565 unpixelated glass margin (~3.6% X, ~4.6% Y — never oversized)."""
    pad_x = max(4.0, w * 0.036)
    pad_y = max(4.0, h * 0.046)
    avail_w = max(16.0, w - pad_x * 2.0)
    avail_h = max(16.0, h - pad_y * 2.0)
    px_w = min(avail_w / LCD_W, avail_h / (LCD_H * aspect))
    px_h = px_w * aspect
    mat_w = LCD_W * px_w
    mat_h = LCD_H * px_h
    return (w - mat_w) * 0.5, (h - mat_h) * 0.5, mat_w, mat_h, px_w, px_h


class LcdLayerCache:
    """Caches static backlit LCD glass backdrop + hairline subpixel grid / inner bezel shadow / glass sheen."""

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
        bg, _fg, grid, is_dark = _lcd_colors_norm(st)
        mx, my, mat_w, mat_h, px_w, px_h = _compute_lcd_active_rect(float(w), float(h), st.pixel_aspect)
        self.metrics = (mx, my, mat_w, mat_h, px_w, px_h)

        # Layer A: Unpixelated Illuminated Glass + LED Diffuser Gradient
        bg_s = cairo.ImageSurface(cairo.FORMAT_ARGB32, w, h)
        cr = cairo.Context(bg_s)
        _round_rect(cr, 0, 0, w, h, 4.0)
        cr.clip()

        pat = cairo.LinearGradient(0, 0, 0, h)
        if is_dark:
            pat.add_color_stop_rgb(0.0, bg[0], bg[1], bg[2])
            pat.add_color_stop_rgb(1.0, _clamp1(bg[0] * 1.08), _clamp1(bg[1] * 1.08), _clamp1(bg[2] * 1.08))
        else:
            pat.add_color_stop_rgb(0.0, bg[0] * 0.94, bg[1] * 0.95, bg[2] * 0.96)
            pat.add_color_stop_rgb(0.55, bg[0], bg[1], bg[2])
            pat.add_color_stop_rgb(1.0, _clamp1(bg[0] * 1.04), _clamp1(bg[1] * 1.04), _clamp1(bg[2] * 1.03))
        cr.set_source(pat)
        cr.rectangle(0, 0, w, h)
        cr.fill()

        if not is_dark:
            cr.set_source_rgba(grid[0], grid[1], grid[2], 0.45 if st.pixel_grid else 0.18)
            cr.rectangle(mx, my, mat_w, mat_h)
            cr.fill()
        self.bg_surf = bg_s

        # Layer B: Crisp Hairline Subpixel Grid + Recessed Bezel Shadow + Glass Sheen
        fg_s = cairo.ImageSurface(cairo.FORMAT_ARGB32, w, h)
        fcr = cairo.Context(fg_s)
        _round_rect(fcr, 0, 0, w, h, 4.0)
        fcr.clip()

        if st.pixel_grid and px_w >= 1.5:
            fcr.set_source_rgba(bg[0], bg[1], bg[2], 0.68 if not is_dark else 0.62)
            fcr.set_line_width(max(0.30, px_w * 0.10))
            for col in range(1, LCD_W):
                lx = mx + col * px_w
                fcr.move_to(lx, my)
                fcr.line_to(lx, my + mat_h)
            fcr.stroke()
            fcr.set_line_width(max(0.30, px_h * 0.10))
            for row in range(1, LCD_H):
                ly = my + row * px_h
                fcr.move_to(mx, ly)
                fcr.line_to(mx + mat_w, ly)
            fcr.stroke()

        sd = max(5.0, min(w, h) * 0.048)
        for (x0, y0, x1, y1, rx, ry, rw, rh) in (
            (0, 0, 0, sd, 0, 0, w, sd),
            (0, h, 0, h - sd * 0.65, 0, h - sd * 0.65, w, sd * 0.65),
            (0, 0, sd * 0.8, 0, 0, 0, sd * 0.8, h),
            (w, 0, w - sd * 0.8, 0, w - sd * 0.8, 0, sd * 0.8, h),
        ):
            sg = cairo.LinearGradient(x0, y0, x1, y1)
            sg.add_color_stop_rgba(0.0, 0.0, 0.0, 0.0, 0.28)
            sg.add_color_stop_rgba(1.0, 0.0, 0.0, 0.0, 0.0)
            fcr.set_source(sg)
            fcr.rectangle(rx, ry, rw, rh)
            fcr.fill()

        sheen = cairo.LinearGradient(0, 0, w * 0.55, h * 0.72)
        sheen.add_color_stop_rgba(0.0, 1.0, 1.0, 1.0, 0.075)
        sheen.add_color_stop_rgba(0.40, 1.0, 1.0, 1.0, 0.015)
        sheen.add_color_stop_rgba(0.41, 1.0, 1.0, 1.0, 0.0)
        fcr.set_source(sheen)
        fcr.rectangle(0, 0, w, h)
        fcr.fill()
        self.fg_surf = fg_s


def paint_fast_lcd(cr: Any, x: float, y: float, w: int, h: int, lcd_state: FastLcdState, st: LcdSettings, cache: LcdLayerCache) -> None:
    import cairo
    cache.ensure(w, h, st)
    mx, my, mat_w, mat_h, px_w, px_h = cache.metrics
    _, _, _, is_dark = _lcd_colors_norm(st)
    px_surf = lcd_state.get_cairo_pixel_surface(st)

    cr.save()
    cr.translate(x, y)
    cr.set_source_surface(cache.bg_surf, 0, 0)
    cr.paint()

    if not is_dark:
        cr.save()
        cr.translate(mx + max(0.55, px_w * 0.24), my + max(0.65, px_h * 0.24))
        cr.scale(mat_w / float(LCD_W), mat_h / float(LCD_H))
        cr.set_source_surface(px_surf, 0, 0)
        cr.get_source().set_filter(cairo.FILTER_NEAREST)
        cr.paint_with_alpha(0.12)
        cr.restore()

    cr.save()
    cr.translate(mx, my)
    cr.scale(mat_w / float(LCD_W), mat_h / float(LCD_H))
    cr.set_source_surface(px_surf, 0, 0)
    cr.get_source().set_filter(cairo.FILTER_NEAREST)
    cr.paint()
    cr.restore()

    cr.set_source_surface(cache.fg_surf, 0, 0)
    cr.paint()
    cr.restore()


# =============================================================================
# 2. 3D HARDWARE MODELS + PANGOCAIRO TYPOGRAPHY + AUTHENTIC QUANSHENG LOGO
# =============================================================================

K1_MATRIX: list[list[tuple[str, str, str, str]]] = [
    [("menu", "MENU", "", ""), ("up", "_K1_LT_", "", ""), ("down", "_K1_GT_", "", ""), ("exit", "EXIT", "", "")],
    [("1", "1", "Band", ".,?"), ("2", "2", "A/B", "abc"), ("3", "3", "V/M", "def"), ("star", "*", "Scan", "_RET_")],
    [("4", "4", "FC",   "ghi"), ("5", "5", "WX",  "jkl"), ("6", "6", "H/L", "mno"), ("0",    "0", "FM",   "")],
    [("7", "7", "VOX",  "pqrs"),("8", "8", "R",   "tuv"), ("9", "9", "Call","wxyz"),("f",    "#", "_KEY_", "F")],
]

K5_BOTTOM_MATRIX: list[list[tuple[str, str, str]]] = [
    [("1", "1", "Band"), ("2", "2", "A/B"),  ("3", "3", "VFO/MR"), ("star", "*", "Scan")],
    [("4", "4", "FC"),   ("5", "5", "NOAA"), ("6", "6", "H/M/L"),  ("0",    "0", "FM")],
    [("7", "7", "VOX"),  ("8", "8", "R"),    ("9", "9", "Call"),   ("f",    "F", "_KEY_")],
]

_PANGO_READY: bool | None = None
_PANGO_MOD: Any = None
_PANGOCAIRO_MOD: Any = None


def _ensure_pango() -> bool:
    global _PANGO_READY, _PANGO_MOD, _PANGOCAIRO_MOD
    if _PANGO_READY is not None:
        return _PANGO_READY
    try:
        import gi
        gi.require_version("Pango", "1.0")
        gi.require_version("PangoCairo", "1.0")
        from gi.repository import Pango, PangoCairo
        _PANGO_MOD = Pango
        _PANGOCAIRO_MOD = PangoCairo
        _PANGO_READY = True
    except Exception:
        _PANGO_READY = False
    return _PANGO_READY


def _draw_pango_text(
    cr: Any,
    text: str,
    x: float,
    y: float,
    *,
    font_desc: str,
    rgba: tuple[float, float, float, float],
    halign: str = "left",    # "left" | "center" | "right"
    valign: str = "center",  # "top" | "center" | "bottom"
) -> tuple[float, float]:
    """Crisp PangoCairo text renderer (with automatic Cairo fallback in minimal headless environments)."""
    cr.set_source_rgba(*rgba)
    if _ensure_pango():
        layout = _PANGOCAIRO_MOD.create_layout(cr)
        layout.set_font_description(_PANGO_MOD.FontDescription.from_string(font_desc))
        layout.set_text(text, -1)
        pw, ph = layout.get_pixel_size()
        dx = x if halign == "left" else (x - pw * 0.5 if halign == "center" else x - pw)
        dy = y if valign == "top" else (y - ph * 0.5 if valign == "center" else y - ph)
        cr.move_to(dx, dy)
        _PANGOCAIRO_MOD.show_layout(cr, layout)
        return float(pw), float(ph)
    # Fallback
    import cairo
    cr.select_font_face("Sans", cairo.FONT_SLANT_NORMAL, cairo.FONT_WEIGHT_BOLD if "Bold" in font_desc else cairo.FONT_WEIGHT_NORMAL)
    sz = 12.0
    for tok in font_desc.split():
        try:
            sz = float(tok)
        except ValueError:
            pass
    cr.set_font_size(sz * 1.25)
    ext = cr.text_extents(text)
    dx = x if halign == "left" else (x - ext.width * 0.5 - ext.x_bearing if halign == "center" else x - ext.width)
    dy = y + ext.height * 0.38 if valign == "center" else y
    cr.move_to(dx, dy)
    cr.show_text(text)
    return float(ext.width), float(ext.height)


def _round_rect(cr: Any, x: float, y: float, w: float, h: float, r: float) -> None:
    r = min(r, w * 0.5, h * 0.5)
    cr.new_sub_path()
    cr.arc(x + w - r, y + r, r, -math.pi / 2, 0)
    cr.arc(x + w - r, y + h - r, r, 0, math.pi / 2)
    cr.arc(x + r, y + h - r, r, math.pi / 2, math.pi)
    cr.arc(x + r, y + r, r, math.pi, 3 * math.pi / 2)
    cr.close_path()


def _draw_quansheng_logo(cr: Any, cx: float, cy: float, scale: float = 1.0) -> None:
    """
    Authentic Quansheng corporate emblem modeled directly after `inspo/Quansheng-Logo.png`
    and `ref_k1_body_crop.png`: slanted curved dual RF wave crescents hugging the upper-left
    shoulder of wide, heavy italic geometric `QUANSHENG` lettering.
    """
    import cairo
    cr.save()
    cr.translate(cx, cy)
    cr.scale(scale, scale)

    # Measure wordmark in crisp heavy italic geometric sans matching Quansheng-Logo.png
    font_str = "Adwaita Sans Heavy Italic 12.2"
    if _ensure_pango():
        layout = _PANGOCAIRO_MOD.create_layout(cr)
        layout.set_font_description(_PANGO_MOD.FontDescription.from_string(font_str))
        layout.set_text("QUANSHENG", -1)
        tw, th = layout.get_pixel_size()
        w_scale = 1.08
        icon_w = 19.0
        total_w = icon_w + tw * w_scale
        x0 = -total_w * 0.5
        tx = x0 + icon_w

        # Two bold filled curved RF signal bars hugging top-left of 'Q' (matches Quansheng-Logo.png)
        cr.set_source_rgba(0.94, 0.95, 0.97, 0.98)
        cr.set_line_cap(cairo.LINE_CAP_BUTT)
        ox, oy = tx + 5.0, 5.0
        # Inner bold bar
        cr.new_path()
        cr.set_line_width(3.1)
        cr.arc(ox, oy, 11.2, math.pi * 1.07, math.pi * 1.43)
        cr.stroke()
        # Outer wider bold bar
        cr.new_path()
        cr.set_line_width(3.6)
        cr.arc(ox, oy, 16.5, math.pi * 1.02, math.pi * 1.46)
        cr.stroke()

        cr.save()
        cr.translate(tx, -th * 0.46)
        cr.scale(w_scale, 1.0)
        cr.set_source_rgba(0.94, 0.95, 0.97, 0.98)
        _PANGOCAIRO_MOD.show_layout(cr, layout)
        cr.restore()
    else:
        cr.set_source_rgba(0.94, 0.95, 0.97, 0.98)
        cr.select_font_face("Sans", cairo.FONT_SLANT_ITALIC, cairo.FONT_WEIGHT_BOLD)
        cr.set_font_size(14.5)
        ext = cr.text_extents("QUANSHENG")
        tx = -(ext.width + 18.0) * 0.5 + 18.0
        cr.set_line_cap(cairo.LINE_CAP_BUTT)
        cr.new_path()
        cr.set_line_width(3.0)
        cr.arc(tx + 5.0, 5.0, 11.0, math.pi * 1.07, math.pi * 1.43)
        cr.stroke()
        cr.new_path()
        cr.set_line_width(3.5)
        cr.arc(tx + 5.0, 5.0, 16.2, math.pi * 1.02, math.pi * 1.46)
        cr.stroke()
        cr.move_to(tx, ext.height * 0.36)
        cr.show_text("QUANSHENG")
    cr.restore()


def _draw_hex_torx_bolt(cr: Any, cx: float, cy: float, r: float = 6.2) -> None:
    """3D countersunk stainless hex-socket bolt matching `ref_k1_body_crop.png`."""
    import cairo
    # Deep countersunk circular socket shadow
    cr.set_source_rgba(0.06, 0.06, 0.06, 0.95)
    cr.arc(cx, cy, r + 1.8, 0, 2 * math.pi)
    cr.fill()
    # Chamfered rim highlight
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.22)
    cr.set_line_width(0.8)
    cr.arc(cx, cy, r + 1.9, math.pi * 0.15, math.pi * 0.85)
    cr.stroke()
    # Brushed stainless conical washer head
    pat = cairo.RadialGradient(cx - r * 0.32, cy - r * 0.32, 0.6, cx, cy, r)
    pat.add_color_stop_rgb(0.0, 0.95, 0.94, 0.90)
    pat.add_color_stop_rgb(0.55, 0.72, 0.70, 0.65)
    pat.add_color_stop_rgb(1.0, 0.36, 0.35, 0.32)
    cr.set_source(pat)
    cr.arc(cx, cy, r, 0, 2 * math.pi)
    cr.fill()
    # Real 6-sided hex drive recess
    hr = r * 0.50
    cr.new_sub_path()
    for i in range(6):
        ang = i * (math.pi / 3.0) + math.pi / 6.0
        px, py = cx + hr * math.cos(ang), cy + hr * math.sin(ang)
        cr.move_to(px, py) if i == 0 else cr.line_to(px, py)
    cr.close_path()
    cr.set_source_rgb(0.16, 0.15, 0.14)
    cr.fill()


def _draw_frosted_led(cr: Any, x: float, y: float, w: float, h: float, state: str) -> None:
    """
    3D beveled frosted polycarbonate light-pipe status LED matching `ref_k1_body_crop.png`:
    off = milky frosted glass; rx = emerald core + bloom; tx = ruby crimson; dfu = warm amber.
    """
    import cairo
    # Black recessed mounting collar
    cr.new_sub_path()
    cr.move_to(x + 4, y)
    cr.line_to(x + w - 4, y)
    cr.line_to(x + w + 2, y + h)
    cr.line_to(x - 2, y + h)
    cr.close_path()
    cr.set_source_rgb(0.09, 0.09, 0.10)
    cr.fill()

    # Frosted trapezoid lens body
    cr.new_sub_path()
    cr.move_to(x + 5, y + 1.5)
    cr.line_to(x + w - 5, y + 1.5)
    cr.line_to(x + w - 1, y + h - 1.5)
    cr.line_to(x + 1, y + h - 1.5)
    cr.close_path()

    cx, cy = x + w * 0.5, y + h * 0.55
    if state == "rx":
        core, mid, outer = (0.78, 1.0, 0.86), (0.14, 0.90, 0.42), (0.06, 0.55, 0.24)
    elif state == "tx":
        core, mid, outer = (1.0, 0.82, 0.80), (0.98, 0.20, 0.20), (0.62, 0.06, 0.06)
    elif state == "dfu":
        core, mid, outer = (1.0, 0.94, 0.72), (0.98, 0.62, 0.08), (0.68, 0.34, 0.02)
    else:
        core, mid, outer = (0.92, 0.93, 0.95), (0.76, 0.78, 0.80), (0.54, 0.56, 0.58)

    pat = cairo.RadialGradient(cx, cy - 1.0, 1.0, cx, cy, w * 0.65)
    pat.add_color_stop_rgb(0.0, *core)
    pat.add_color_stop_rgb(0.55, *mid)
    pat.add_color_stop_rgb(1.0, *outer)
    cr.set_source(pat)
    cr.fill_preserve()
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.40)
    cr.set_line_width(0.8)
    cr.stroke()

    # Translucent polycarbonate upper specular facet edge
    cr.new_path()
    cr.move_to(x + 6.5, y + 2.4)
    cr.line_to(x + w - 6.5, y + 2.4)
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.58 if state != "off" else 0.35)
    cr.set_line_width(1.1)
    cr.stroke()

    # Realistic multi-layered ambient bloom spilling onto neighboring bezel & antenna collar
    if state in ("rx", "tx", "dfu"):
        bloom = cairo.RadialGradient(cx, cy, 2.0, cx, cy, 28.0)
        bloom.add_color_stop_rgba(0.0, mid[0], mid[1], mid[2], 0.64)
        bloom.add_color_stop_rgba(0.45, mid[0], mid[1], mid[2], 0.24)
        bloom.add_color_stop_rgba(1.0, mid[0], mid[1], mid[2], 0.0)
        cr.set_source(bloom)
        cr.new_path()
        cr.arc(cx, cy, 28.0, 0, 2 * math.pi)
        cr.fill()


def _draw_key_glyph(cr: Any, x: float, y: float, w: float, h: float) -> None:
    """Crisp vector padlock/key icon matching UV-K1 `#` / UV-K5 `F` silkscreen."""
    import cairo
    cr.save()
    cr.set_source_rgba(0.92, 0.94, 0.97, 0.94)
    cr.set_line_width(1.35)
    cr.set_line_cap(cairo.LINE_CAP_ROUND)
    # Key bow (ring) on right + shaft + teeth on left (exact UV-K1 key icon!)
    ry = y + h * 0.5
    cr.new_path()
    cr.arc(x + w - 3.2, ry, 2.6, 0, 2 * math.pi)
    cr.stroke()
    cr.new_path()
    cr.move_to(x + w - 5.8, ry)
    cr.line_to(x + 1.0, ry)
    cr.move_to(x + 2.5, ry)
    cr.line_to(x + 2.5, ry + 2.5)
    cr.move_to(x + 5.2, ry)
    cr.line_to(x + 5.2, ry + 2.0)
    cr.stroke()
    cr.restore()


def _draw_3d_keycap(
    cr: Any,
    x: float,
    y: float,
    w: float,
    h: float,
    primary: str,
    sub1: str,
    sub2: str,
    *,
    press_t: float,
    hover_t: float,
    radius: float = 6.0,
    is_nav: bool = False,
    homing_nub: bool = False,
    accent: tuple[float, float, float] = (0.18, 0.56, 1.0),
) -> None:
    """
    3D-sculpted raised island rubber keycap matching `ref_k1_body_crop.png`:
      • Outer milled metallic chamfer + thick matte-black rubber surround gasket
      • Raised pillowed dark charcoal rubber island with crisp upper rim specular
      • Real PangoCairo typography (Adwaita Sans) with large clear digits & crisp legends
      • Smooth 60 FPS spring plunge animation
    """
    import cairo
    p = _clamp1(press_t)
    hv = _clamp1(hover_t)

    # 1. Metallic cutout lower chamfer highlight (catches top-down studio light!)
    _round_rect(cr, x - 1.0, y + 0.8, w + 2.0, h + 1.2, radius + 1.4)
    cr.set_source_rgba(1.0, 0.99, 0.96, 0.26)
    cr.fill()

    # 2. Thick glossy-black rubber surround bezel (the signature UV-K1 deep button well!)
    _round_rect(cr, x - 1.0, y - 1.0, w + 2.0, h + 2.0, radius + 1.2)
    cr.set_source_rgb(0.05, 0.05, 0.06)
    cr.fill()

    if p > 0.02:
        _round_rect(cr, x - 1.4, y - 1.4, w + 2.8, h + 2.8, radius + 1.6)
        cr.set_source_rgba(accent[0], accent[1], accent[2], 0.30 * p)
        cr.fill()

    # 3. Raised 3D rubber keytop island inside the black gasket
    gasket = 2.6
    dy = 1.8 * p
    ix = x + gasket + 0.35 * p
    iy = y + gasket + dy - 0.5 * (1.0 - p)
    iw = w - gasket * 2.0 - 0.7 * p
    ih = h - gasket * 2.0 - 0.4 * p
    ir = max(3.0, radius - 1.8)

    # 3D bottom skirt shadow under raised keytop
    if p < 0.95:
        _round_rect(cr, ix, iy + 2.0 * (1.0 - p), iw, ih, ir)
        cr.set_source_rgba(0.0, 0.0, 0.0, 0.72 * (1.0 - p))
        cr.fill()

    # Pillowed 3D rubber keytop surface
    _round_rect(cr, ix, iy, iw, ih, ir)
    t_l = 0.25 + 0.06 * hv - 0.11 * p
    m_l = 0.19 + 0.04 * hv - 0.06 * p
    b_l = 0.14 + 0.03 * hv - 0.02 * p
    pat = cairo.LinearGradient(ix, iy, ix, iy + ih)
    pat.add_color_stop_rgb(0.0, t_l, t_l, t_l + 0.01)
    pat.add_color_stop_rgb(0.42, m_l, m_l, m_l + 0.01)
    pat.add_color_stop_rgb(1.0, b_l, b_l, b_l + 0.01)
    cr.set_source(pat)
    cr.fill_preserve()

    # Crisp 3D top specular highlight rim (or accent ring on press)
    if p > 0.02:
        cr.set_source_rgba(accent[0], accent[1], accent[2], 0.90 * p)
        cr.set_line_width(1.4)
    else:
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.16 + 0.10 * hv)
        cr.set_line_width(0.95)
    cr.stroke()

    # Subtle tactile homing bar on key `5` (bottom-centered so it never crowds legends!)
    if homing_nub:
        _round_rect(cr, ix + iw * 0.5 - 4.5, iy + ih - 4.0, 9.0, 1.8, 0.9)
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.28)
        cr.fill()

    # 4. Crisp PangoCairo Legends & Bold Geometric Vector Icons
    ink = (0.95, 0.96, 0.98, 0.98)
    sub_ink = (0.90, 0.92, 0.95, 0.92)
    sub2_ink = (0.80, 0.83, 0.88, 0.84)

    if primary in ("_K1_LT_", "_K1_GT_", "_CHEV_UP_", "_CHEV_DN_"):
        cr.save()
        cr.set_source_rgba(*ink)
        cr.set_line_width(2.8 if primary.startswith("_K1_") else 2.2)
        cr.set_line_cap(cairo.LINE_CAP_ROUND)
        cr.set_line_join(cairo.LINE_JOIN_ROUND)
        cx, cy = ix + iw * 0.5, iy + ih * 0.5
        if primary == "_K1_LT_":
            cr.move_to(cx + 4.5, cy - 8.0)
            cr.line_to(cx - 5.5, cy)
            cr.line_to(cx + 4.5, cy + 8.0)
        elif primary == "_K1_GT_":
            cr.move_to(cx - 4.5, cy - 8.0)
            cr.line_to(cx + 5.5, cy)
            cr.line_to(cx - 4.5, cy + 8.0)
        elif primary == "_CHEV_UP_":
            cy2 = iy + ih * 0.42
            cr.move_to(cx - 8.5, cy2 + 3.8)
            cr.line_to(cx, cy2 - 4.6)
            cr.line_to(cx + 8.5, cy2 + 3.8)
        else:
            cy2 = iy + ih * 0.58
            cr.move_to(cx - 8.5, cy2 - 3.8)
            cr.line_to(cx, cy2 + 4.6)
            cr.line_to(cx + 8.5, cy2 - 3.8)
        cr.stroke()
        cr.restore()
        if sub1:
            _draw_pango_text(
                cr, sub1, ix + iw * 0.5, iy + ih * (0.80 if primary == "_CHEV_UP_" else 0.28),
                font_desc="Adwaita Sans Bold 6.5", rgba=sub2_ink, halign="center", valign="center",
            )
    elif is_nav or (not sub1 and not sub2):
        _draw_pango_text(
            cr, primary, ix + iw * 0.5, iy + ih * 0.5,
            font_desc="Adwaita Sans SemiBold 10.5", rgba=ink, halign="center", valign="center",
        )
    else:
        # Large, crisp geometric primary digit on left (matches real K1 70%-height digits!)
        is_long_sub = bool(sub1 and not sub2 and sub1 != "_KEY_" and len(sub1) >= 5)
        p_size = "14.5" if is_long_sub else ("15.5" if primary != "*" else "17")
        p_x = ix + (5.0 if is_long_sub else 6.5)
        p_y = iy + ih * (0.56 if primary == "*" else 0.50)
        pw, _ = _draw_pango_text(
            cr, primary, p_x, p_y,
            font_desc=f"Adwaita Sans Medium {p_size}", rgba=ink, halign="left", valign="center",
        )
        sx = p_x + max(pw, 10.0 if is_long_sub else 11.5) + (2.8 if is_long_sub else 4.0)

        if sub1 and sub2:
            if sub1 == "_KEY_":
                _draw_key_glyph(cr, sx, iy + ih * 0.14, 15.0, 9.0)
            else:
                _draw_pango_text(cr, sub1, sx, iy + ih * 0.30, font_desc="Adwaita Sans Medium 7.6", rgba=sub_ink, halign="left", valign="center")

            if sub2 == "_RET_":
                cr.save()
                cr.set_source_rgba(*sub_ink)
                cr.set_line_width(1.5)
                cr.set_line_cap(cairo.LINE_CAP_ROUND)
                ry = iy + ih * 0.72
                cr.new_path()
                cr.move_to(sx + 13.0, ry - 4.0)
                cr.line_to(sx + 13.0, ry)
                cr.line_to(sx + 1.0, ry)
                cr.move_to(sx + 4.2, ry - 2.6)
                cr.line_to(sx + 1.0, ry)
                cr.line_to(sx + 4.2, ry + 2.6)
                cr.stroke()
                cr.restore()
            else:
                _draw_pango_text(cr, sub2, sx, iy + ih * 0.72, font_desc="Adwaita Sans 7.2", rgba=sub2_ink, halign="left", valign="center")
        elif sub1:
            if sub1 == "_KEY_":
                _draw_key_glyph(cr, sx, iy + ih * 0.34, 15.0, 9.0)
            else:
                s_sz = "6.8" if len(sub1) >= 5 else "8.0"
                _draw_pango_text(cr, sub1, sx, iy + ih * 0.50, font_desc=f"Adwaita Sans Medium {s_sz}", rgba=sub_ink, halign="left", valign="center")


def _draw_side_profile_shroud(cr: Any, bx: float, by: float, bh: float, side_specs: list[tuple[str, float, float, float, float, bool, int]]) -> None:
    """
    Full-height 3D shadowed left back-shell plastic flank extending along the entire left side
    of the radio body so PTT, SK1, and SK2 emerge naturally from the device's side profile.
    """
    import cairo
    sh_x = bx - 15.0
    top_y = by + 10.0
    bot_y = by + bh - 10.0

    # 1. Soft drop shadow beneath the full-height left back-plastic flank
    cr.new_path()
    cr.move_to(bx + 4, top_y - 3)
    cr.curve_to(bx - 10, top_y - 1, sh_x - 2, top_y + 10, sh_x - 2, top_y + 26)
    cr.line_to(sh_x - 2, bot_y - 24)
    cr.curve_to(sh_x - 2, bot_y - 8, bx - 10, bot_y + 4, bx + 4, bot_y + 6)
    cr.close_path()
    cr.set_source_rgba(0.0, 0.0, 0.0, 0.42)
    cr.fill()

    # 2. Full-height matte dark polymer rear/side shell plastic wrapping the entire left side
    cr.new_path()
    cr.move_to(bx + 6, top_y - 4)
    cr.curve_to(bx - 8, top_y - 3, sh_x, top_y + 8, sh_x, top_y + 24)
    cr.line_to(sh_x, bot_y - 24)
    cr.curve_to(sh_x, bot_y - 8, bx - 8, bot_y + 3, bx + 6, bot_y + 4)
    cr.close_path()
    pat_cheek = cairo.LinearGradient(sh_x, 0, bx + 4, 0)
    pat_cheek.add_color_stop_rgb(0.0, 0.055, 0.06, 0.07)
    pat_cheek.add_color_stop_rgb(0.32, 0.14, 0.15, 0.17)
    pat_cheek.add_color_stop_rgb(0.72, 0.085, 0.09, 0.105)
    pat_cheek.add_color_stop_rgb(1.0, 0.035, 0.04, 0.045)
    cr.set_source(pat_cheek)
    cr.fill_preserve()
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.09)
    cr.set_line_width(0.9)
    cr.stroke()

    # 3. Subtle horizontal grip contour ridge on lower left side-shell (below side buttons)
    cr.save()
    cr.set_source_rgba(0.0, 0.0, 0.0, 0.32)
    cr.set_line_width(1.2)
    cr.new_path()
    cr.move_to(sh_x + 2.0, by + 292.0)
    cr.line_to(bx - 1.0, by + 292.0)
    cr.stroke()
    cr.restore()

    # 4. Deep ambient-occlusion vertical seam shadow where left side-shell meets front chassis
    seam = cairo.LinearGradient(bx - 9.0, 0, bx + 2.0, 0)
    seam.add_color_stop_rgba(0.0, 0.0, 0.0, 0.0, 0.0)
    seam.add_color_stop_rgba(0.78, 0.0, 0.0, 0.0, 0.65)
    seam.add_color_stop_rgba(1.0, 0.0, 0.0, 0.0, 0.0)
    cr.set_source(seam)
    cr.rectangle(bx - 9.0, top_y, 11.0, bot_y - top_y)
    cr.fill()

    # 5. Recessed dark polymer button-well pockets cradling PTT, SK1, SK2
    for _kn, sx, sy, sw, sh, is_ptt, _dots in side_specs:
        _round_rect(cr, sx + 4.0, sy - 2.5, sw - 2.0, sh + 5.0, 8.5 if is_ptt else 6.5)
        cr.set_source_rgba(0.015, 0.015, 0.02, 0.88)
        cr.fill()


def _draw_top_flashlight(cr: Any, tx: float, by: float, torch_on: bool, *, compact: bool = False) -> None:
    """3D top flashlight lens housing with realistic directional LED beam & page-flash strobe."""
    import cairo
    hw = 15.0 if compact else 18.0
    hh = 14.0 if compact else 17.0
    ty = by - (11.0 if compact else 13.0)

    if torch_on:
        # Upward directional beam cone (simulates real white/warm-white SMD torch LED!)
        cr.save()
        cr.new_path()
        cr.move_to(tx - hw * 0.65, ty + 2)
        cr.line_to(tx - hw * 2.5, 0)
        cr.line_to(tx + hw * 2.5, 0)
        cr.line_to(tx + hw * 0.65, ty + 2)
        cr.close_path()
        beam = cairo.LinearGradient(tx, ty + 2, tx, 0)
        beam.add_color_stop_rgba(0.0, 1.0, 0.99, 0.90, 0.90)
        beam.add_color_stop_rgba(0.45, 1.0, 0.97, 0.82, 0.44)
        beam.add_color_stop_rgba(1.0, 1.0, 0.95, 0.78, 0.0)
        cr.set_source(beam)
        cr.fill()
        # Intense radial LED core bloom spilling onto top bezel
        tg = cairo.RadialGradient(tx, ty + 2, 1.5, tx, ty + 2, 34.0)
        tg.add_color_stop_rgba(0.0, 1.0, 1.0, 0.96, 1.0)
        tg.add_color_stop_rgba(0.32, 1.0, 0.98, 0.84, 0.62)
        tg.add_color_stop_rgba(1.0, 1.0, 0.95, 0.75, 0.0)
        cr.set_source(tg)
        cr.new_path()
        cr.arc(tx, ty + 2, 34.0, 0, 2 * math.pi)
        cr.fill()
        cr.restore()

    # Low-profile lens bezel housing
    pat_fl = cairo.LinearGradient(tx - hw, ty, tx + hw, ty + hh)
    pat_fl.add_color_stop_rgb(0.0, 0.24, 0.25, 0.27)
    pat_fl.add_color_stop_rgb(1.0, 0.09, 0.10, 0.11)
    cr.set_source(pat_fl)
    _round_rect(cr, tx - hw, ty, hw * 2.0, hh, 4.0)
    cr.fill()
    # Recessed clear acrylic lens dome (glows bright white when LED is lit)
    _round_rect(cr, tx - hw * 0.56, ty + 1.5, hw * 1.12, 4.5, 2.0)
    if torch_on:
        cr.set_source_rgba(1.0, 1.0, 0.96, 0.98)
    else:
        cr.set_source_rgba(0.62, 0.65, 0.70, 0.35)
    cr.fill()


def compute_handset_geometry(model: str) -> tuple[int, int]:
    return (356, 614) if model == "k5" else (364, 594)


class HandsetRenderer:
    """3D-sculpted cached Cairo handset renderer (~0.7 ms/frame animated!)."""

    def __init__(self) -> None:
        self._chassis_key: Any = None
        self._back_surf: Any = None
        self._chassis_surf: Any = None
        self._body_bx: float = 30.0
        self._lcd_rect: tuple[int, int, int, int] = (0, 0, 100, 60)
        self._torch_anchor: tuple[float, float, bool] = (180.0, 60.0, False)
        self._key_specs: list[tuple[str, float, float, float, float, str, str, str, float, bool, bool]] = []
        self._side_specs: list[tuple[str, float, float, float, float, bool, int]] = []
        self.hitboxes: dict[str, tuple[float, float, float, float]] = {}
        self.lcd_cache = LcdLayerCache()

    def _ensure_chassis(self, model: str, led_state: str, lcd_st: LcdSettings) -> None:
        import cairo
        key = (model, led_state, lcd_st.palette, lcd_st.backlight, lcd_st.backlight_glow)
        if key == self._chassis_key and self._chassis_surf is not None and self._back_surf is not None:
            return
        self._chassis_key = key
        W, H = compute_handset_geometry(model)
        back_surf = cairo.ImageSurface(cairo.FORMAT_ARGB32, W, H)
        back_cr = cairo.Context(back_surf)
        surf = cairo.ImageSurface(cairo.FORMAT_ARGB32, W, H)
        cr = cairo.Context(surf)
        self.hitboxes.clear()
        self._key_specs.clear()
        self._side_specs.clear()

        if model == "k5":
            self._build_k5_chassis(back_cr, cr, W, H, led_state, lcd_st)
        else:
            self._build_k1_chassis(back_cr, cr, W, H, led_state, lcd_st)
        # Bake idle/unpressed front keycaps onto the cached chassis surface so steady LCD
        # streaming skips 16 PangoCairo text layouts per frame (>1,000 FPS render cap!).
        accent = (0.98, 0.48, 0.08) if model == "k5" else (0.18, 0.56, 1.0)
        for _kn, kx, ky, kw, kh, pri, s1, s2, rad, is_nav, nub in self._key_specs:
            _draw_3d_keycap(
                cr, kx, ky, kw, kh, pri, s1, s2,
                press_t=0.0, hover_t=0.0, radius=rad, is_nav=is_nav, homing_nub=nub, accent=accent,
            )
        self._back_surf = back_surf
        self._chassis_surf = surf

    def _build_k1_chassis(self, back_cr: Any, cr: Any, W: int, H: int, led_state: str, lcd_st: LcdSettings) -> None:
        """Photorealistic 3D Quansheng UV-K1(8) modeled directly from `ref_k1_body_crop.png` & `左视图.png`."""
        import cairo
        bx, by, bw, bh = 30.0, 60.0, W - 44.0, H - 74.0
        self._body_bx = bx

        # Register 3D side key specs first (tucked cleanly into left back-shell behind front bezel lip!)
        self._side_specs = [
            ("ptt",   8.0, by + 56.0, 26.0, 122.0, True, 0),
            ("side1", 11.0, by + 188.0, 23.0, 38.0, False, 2),
            ("side2", 11.0, by + 234.0, 23.0, 38.0, False, 1),
        ]
        for kn, sx, sy, sw, sh, _, _ in self._side_specs:
            self.hitboxes[kn] = (sx - 3.0, sy, sw, sh)

        # Back layer: ambient drop shadow + full-height left back-plastic shell + recessed button wells
        _round_rect(back_cr, bx + 4, by + 10, bw, bh, 18)
        back_cr.set_source_rgba(0.0, 0.0, 0.0, 0.36)
        back_cr.fill()
        _draw_side_profile_shroud(back_cr, bx, by, bh, self._side_specs)

        # --- 1. Top Hardware Assembly (3D Stubby A1 Antenna + Flashlight + Cobalt-Blue Rotary Knob) ---
        ax = bx + 62.0
        # 3D cylindrical ribbed rubber antenna base
        pat_ant = cairo.LinearGradient(ax - 28, 0, ax + 28, 0)
        pat_ant.add_color_stop_rgb(0.0, 0.08, 0.09, 0.10)
        pat_ant.add_color_stop_rgb(0.28, 0.28, 0.29, 0.31)
        pat_ant.add_color_stop_rgb(0.52, 0.20, 0.21, 0.22)
        pat_ant.add_color_stop_rgb(0.82, 0.11, 0.12, 0.13)
        pat_ant.add_color_stop_rgb(1.0, 0.06, 0.07, 0.08)
        cr.set_source(pat_ant)
        _round_rect(cr, ax - 24, 2, 48, by + 6, 5)
        cr.fill()
        # Stepped lower SMA skirt
        cr.set_source(pat_ant)
        _round_rect(cr, ax - 27, by - 22, 54, 26, 4)
        cr.fill()
        # Recessed matte label panel "A1"
        _round_rect(cr, ax - 10, 4, 20, 26, 4)
        cr.set_source_rgba(0.05, 0.05, 0.06, 0.55)
        cr.fill()
        _draw_pango_text(cr, "A1", ax, 16, font_desc="Adwaita Sans Bold 9", rgba=(0.70, 0.72, 0.75, 0.50), halign="center")

        # Center low-profile LED flashlight anchor (drawn dynamically in paint() for 60 FPS page-flash strobe!)
        tx = bx + bw * 0.49
        self._torch_anchor = (tx, by, False)
        self.hitboxes["_torch"] = (tx - 20, by - 18, 40, 22)

        # 3D Sculpted Knurled Rotary Knob with Signature Vertical Cobalt-Blue Stripe
        kx = bx + bw - 62.0
        pat_k = cairo.LinearGradient(kx - 34, 0, kx + 34, 0)
        pat_k.add_color_stop_rgb(0.0, 0.07, 0.08, 0.09)
        pat_k.add_color_stop_rgb(0.26, 0.30, 0.32, 0.35)
        pat_k.add_color_stop_rgb(0.52, 0.18, 0.19, 0.21)
        pat_k.add_color_stop_rgb(0.80, 0.22, 0.24, 0.26)
        pat_k.add_color_stop_rgb(1.0, 0.06, 0.07, 0.08)
        cr.set_source(pat_k)
        _round_rect(cr, kx - 33, by - 42, 66, 46, 7)
        cr.fill()
        # Realistic recessed vertical grip scoops
        for dx in (-22, -13, 13, 22):
            _round_rect(cr, kx + dx - 3.2, by - 38, 6.4, 30, 3.0)
            cr.set_source_rgba(0.03, 0.03, 0.04, 0.65)
            cr.fill()
        # Signature electric cobalt-blue center pointer stripe
        pat_bl = cairo.LinearGradient(kx - 4, by - 42, kx + 4, by - 42)
        pat_bl.add_color_stop_rgb(0.0, 0.12, 0.42, 0.96)
        pat_bl.add_color_stop_rgb(0.5, 0.28, 0.62, 1.00)
        pat_bl.add_color_stop_rgb(1.0, 0.10, 0.36, 0.88)
        cr.set_source(pat_bl)
        cr.rectangle(kx - 3.6, by - 42, 7.2, 40)
        cr.fill()

        # --- 2. 3D Outer Textured Black Polymer Chassis & Curved Side Cheeks ---
        _round_rect(cr, bx, by, bw, bh, 15)
        pat_sh = cairo.LinearGradient(bx, by, bx + bw, by)
        pat_sh.add_color_stop_rgb(0.0, 0.24, 0.25, 0.27)   # Left 3D curved rubber edge highlight
        pat_sh.add_color_stop_rgb(0.05, 0.11, 0.12, 0.13)
        pat_sh.add_color_stop_rgb(0.95, 0.11, 0.12, 0.13)
        pat_sh.add_color_stop_rgb(1.0, 0.26, 0.27, 0.29)   # Right 3D curved rubber edge highlight
        cr.set_source(pat_sh)
        cr.fill_preserve()
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.14)
        cr.set_line_width(1.0)
        cr.stroke()

        # Frosted Trapezoid Status LED Light-Pipe sitting right on top-left shoulder
        _draw_frosted_led(cr, bx + 46.0, by - 10.0, 30.0, 13.0, led_state)

        # --- 3. 3D Brushed Champagne-Titanium Metallic Armor Faceplate (ref_k1_body_crop.png!) ---
        fx_l, fx_r = bx + 20.0, bx + bw - 20.0
        fy_t, fy_b = by + 5.0, by + bh - 8.0

        def _armor_path() -> None:
            cr.new_sub_path()
            cr.move_to(bx + 12, fy_t)
            cr.line_to(bx + bw - 12, fy_t)
            cr.line_to(bx + bw - 4, fy_t + 8)
            cr.line_to(bx + bw - 4, fy_t + 44)
            cr.line_to(fx_r, fy_t + 58)
            cr.line_to(fx_r, fy_b - 92)
            cr.line_to(bx + bw - 4, fy_b - 78)
            cr.line_to(bx + bw - 4, fy_b - 9)
            cr.line_to(bx + bw - 12, fy_b)
            cr.line_to(bx + 12, fy_b)
            cr.line_to(bx + 4, fy_b - 9)
            cr.line_to(bx + 4, fy_b - 78)
            cr.line_to(fx_l, fy_b - 92)
            cr.line_to(fx_l, fy_t + 58)
            cr.line_to(bx + 4, fy_t + 44)
            cr.line_to(bx + 4, fy_t + 8)
            cr.close_path()

        # Outer metallic bevel skirt
        _armor_path()
        pat_m = cairo.LinearGradient(bx, fy_t, bx + bw, fy_b)
        pat_m.add_color_stop_rgb(0.0, 0.72, 0.69, 0.64)    # Warm champagne-titanium bright highlight
        pat_m.add_color_stop_rgb(0.28, 0.59, 0.56, 0.52)
        pat_m.add_color_stop_rgb(0.65, 0.49, 0.47, 0.43)
        pat_m.add_color_stop_rgb(1.0, 0.63, 0.60, 0.55)
        cr.set_source(pat_m)
        cr.fill_preserve()
        cr.set_source_rgba(1.0, 0.99, 0.96, 0.34)
        cr.set_line_width(1.3)
        cr.stroke()

        # 3D sculpted side cheek chamfers flanking the main central deck
        cr.save()
        _armor_path()
        cr.clip()
        # Left bevel reflection strip
        lb = cairo.LinearGradient(fx_l, 0, fx_l + 14, 0)
        lb.add_color_stop_rgba(0.0, 0.0, 0.0, 0.0, 0.32)
        lb.add_color_stop_rgba(0.6, 1.0, 0.98, 0.95, 0.14)
        lb.add_color_stop_rgba(1.0, 0.0, 0.0, 0.0, 0.0)
        cr.set_source(lb)
        cr.rectangle(fx_l, fy_t + 44, 14, fy_b - fy_t - 44)
        cr.fill()
        # Right bevel reflection strip
        rb = cairo.LinearGradient(fx_r - 14, 0, fx_r, 0)
        rb.add_color_stop_rgba(0.0, 0.0, 0.0, 0.0, 0.0)
        rb.add_color_stop_rgba(0.4, 1.0, 0.98, 0.95, 0.14)
        rb.add_color_stop_rgba(1.0, 0.0, 0.0, 0.0, 0.34)
        cr.set_source(rb)
        cr.rectangle(fx_r - 14, fy_t + 44, 14, fy_b - fy_t - 44)
        cr.fill()
        cr.restore()

        # 4 Real Machined Hex-Socket Torx Bolts
        _draw_hex_torx_bolt(cr, bx + 22.0, fy_t + 24.0, 6.2)
        _draw_hex_torx_bolt(cr, bx + bw - 22.0, fy_t + 24.0, 6.2)
        _draw_hex_torx_bolt(cr, bx + 22.0, fy_b - 44.0, 6.2)
        _draw_hex_torx_bolt(cr, bx + bw - 22.0, fy_b - 44.0, 6.2)

        # --- 4. Glossy Piano-Black Top Nameplate with Diagonal Glass Specular & Exact Quansheng Logo ---
        np_x, np_y, np_w, np_h = bx + 46.0, fy_t + 8.0, bw - 92.0, 32.0
        _round_rect(cr, np_x, np_y, np_w, np_h, 5)
        cr.set_source_rgb(0.06, 0.06, 0.07)
        cr.fill_preserve()
        # Crisp glossy diagonal reflection across left half (exact ref_k1_body_crop.png!)
        cr.save()
        cr.clip_preserve()
        cr.new_sub_path()
        cr.move_to(np_x, np_y)
        cr.line_to(np_x + np_w * 0.56, np_y)
        cr.line_to(np_x + np_w * 0.42, np_y + np_h)
        cr.line_to(np_x, np_y + np_h)
        cr.close_path()
        gl = cairo.LinearGradient(np_x, np_y, np_x + np_w * 0.5, np_y + np_h)
        gl.add_color_stop_rgba(0.0, 1.0, 1.0, 1.0, 0.20)
        gl.add_color_stop_rgba(1.0, 1.0, 1.0, 1.0, 0.05)
        cr.set_source(gl)
        cr.fill()
        cr.restore()
        cr.set_source_rgba(0.0, 0.0, 0.0, 0.75)
        cr.set_line_width(1.2)
        cr.stroke()
        _draw_quansheng_logo(cr, np_x + np_w * 0.5, np_y + np_h * 0.52, scale=1.02)

        # --- 5. Deep 3D Recessed LCD Window Cutout ---
        lz_x, lz_y, lz_w, lz_h = int(fx_l + 14), int(fy_t + 54), int((fx_r - fx_l) - 28), 166
        # Upper dark shadow casting down from nameplate ridge
        _round_rect(cr, lz_x - 2, lz_y - 2, lz_w + 4, lz_h + 4, 10)
        cr.set_source_rgba(0.0, 0.0, 0.0, 0.35)
        cr.fill()
        # Thick glossy black inner bezel
        _round_rect(cr, lz_x, lz_y, lz_w, lz_h, 9)
        cr.set_source_rgb(0.07, 0.07, 0.08)
        cr.fill_preserve()
        cr.set_source_rgba(1.0, 0.99, 0.96, 0.22)
        cr.set_line_width(1.0)
        cr.stroke()
        if lcd_st.backlight_glow and lcd_st.backlight > 0:
            bg_c, _, _, _ = _lcd_colors_norm(lcd_st)
            cr.set_source_rgba(bg_c[0], bg_c[1], bg_c[2], 0.16)
            _round_rect(cr, lz_x + 5, lz_y + 5, lz_w - 10, lz_h - 10, 6)
            cr.set_line_width(2.5)
            cr.stroke()
        self._lcd_rect = (lz_x + 7, lz_y + 7, lz_w - 14, lz_h - 14)

        # --- 6. Sculpted 3D Mid-Bar ("UHF/VHF Transceiver") + Deep Shadow Step + Dual Mic Cutouts ---
        bar_y = lz_y + lz_h + 6.0
        # Raised champagne bar strip
        pat_bar = cairo.LinearGradient(fx_l, bar_y, fx_l, bar_y + 20.0)
        pat_bar.add_color_stop_rgb(0.0, 0.72, 0.70, 0.66)
        pat_bar.add_color_stop_rgb(0.85, 0.58, 0.56, 0.52)
        pat_bar.add_color_stop_rgb(1.0, 0.30, 0.29, 0.27)
        cr.set_source(pat_bar)
        cr.rectangle(fx_l + 6, bar_y, (fx_r - fx_l) - 12, 20.0)
        cr.fill()
        _draw_pango_text(
            cr, "UHF/VHF Transceiver", fx_r - 12.0, bar_y + 9.5,
            font_desc="Adwaita Sans Medium 8.5", rgba=(0.08, 0.08, 0.08, 0.85), halign="right", valign="center",
        )
        # Deep 3D recessed shadow sloping onto the keypad deck (exact ref_k1_body_crop.png!)
        step_sh = cairo.LinearGradient(0, bar_y + 20.0, 0, bar_y + 30.0)
        step_sh.add_color_stop_rgba(0.0, 0.0, 0.0, 0.0, 0.45)
        step_sh.add_color_stop_rgba(1.0, 0.0, 0.0, 0.0, 0.0)
        cr.set_source(step_sh)
        cr.rectangle(fx_l + 6, bar_y + 20.0, (fx_r - fx_l) - 12, 10.0)
        cr.fill()

        # Dual recessed indicator/mic port right above EXIT
        _round_rect(cr, fx_r - 44.0, bar_y + 24.0, 28.0, 6.5, 3.0)
        cr.set_source_rgb(0.06, 0.06, 0.07)
        cr.fill()
        cr.set_source_rgba(0.75, 0.77, 0.80, 0.45)
        _round_rect(cr, fx_r - 41.0, bar_y + 25.5, 8.0, 3.5, 1.5)
        _round_rect(cr, fx_r - 27.0, bar_y + 25.5, 8.0, 3.5, 1.5)
        cr.fill()

        # --- 7. Exact 4x4 Keypad with 12 Inter-Row Speaker Slots! ---
        kp_x, kp_y, kp_w = fx_l + 8.0, bar_y + 36.0, (fx_r - fx_l) - 16.0
        gap_c = 7.0
        gap_r = 12.0  # Space for real horizontal speaker slots between rows!
        kw = (kp_w - gap_c * 3.0) / 4.0
        kh_nav, kh_num = 42.0, 36.0

        for r_i, row_keys in enumerate(K1_MATRIX):
            ky = kp_y + (0.0 if r_i == 0 else (kh_nav + gap_r + (r_i - 1) * (kh_num + gap_r)))
            kh = kh_nav if r_i == 0 else kh_num
            for c_i, (kn, pri, s1, s2) in enumerate(row_keys):
                kx = kp_x + c_i * (kw + gap_c)
                self.hitboxes[kn] = (kx, ky, kw, kh)
                self._key_specs.append((kn, kx, ky, kw, kh, pri, s1, s2, 6.0, r_i == 0, kn == "5"))
                # Draw the real horizontal speaker vent slot right under rows 0, 1, 2 (exact ref_k1_body_crop.png!)
                if r_i < 3:
                    sl_w = kw * 0.62
                    sl_x = kx + (kw - sl_w) * 0.5
                    sl_y = ky + kh + 4.2
                    _round_rect(cr, sl_x, sl_y, sl_w, 4.2, 2.0)
                    cr.set_source_rgb(0.06, 0.06, 0.07)
                    cr.fill()
                    cr.set_source_rgba(1.0, 0.99, 0.95, 0.25)
                    cr.set_line_width(0.7)
                    cr.move_to(sl_x + 2, sl_y + 4.6)
                    cr.line_to(sl_x + sl_w - 2, sl_y + 4.6)
                    cr.stroke()

    def _build_k5_chassis(self, back_cr: Any, cr: Any, W: int, H: int, led_state: str, lcd_st: LcdSettings) -> None:
        """Photorealistic 3D Quansheng UV-K5 v3 / UV-K6 modeled directly from `UVK6.1.png`."""
        import cairo
        bx, by, bw, bh = 30.0, 58.0, W - 44.0, H - 72.0
        self._body_bx = bx

        self._side_specs = [
            ("ptt",   8.0, by + 64.0, 26.0, 114.0, True, 0),
            ("side1", 11.0, by + 188.0, 23.0, 36.0, False, 0),
            ("side2", 11.0, by + 232.0, 23.0, 36.0, False, 0),
        ]
        for kn, sx, sy, sw, sh, _, _ in self._side_specs:
            self.hitboxes[kn] = (sx - 3.0, sy, sw, sh)

        _round_rect(back_cr, bx + 4, by + 10, bw, bh, 18)
        back_cr.set_source_rgba(0.0, 0.0, 0.0, 0.36)
        back_cr.fill()

        # Shadowed 3D full-height left side-profile back plastic behind PTT / SK1 / SK2
        _draw_side_profile_shroud(back_cr, bx, by, bh, self._side_specs)

        # Tapered antenna base
        ax = bx + 56.0
        pat_ant = cairo.LinearGradient(ax - 24, 0, ax + 24, 0)
        pat_ant.add_color_stop_rgb(0.0, 0.09, 0.10, 0.11)
        pat_ant.add_color_stop_rgb(0.35, 0.28, 0.29, 0.31)
        pat_ant.add_color_stop_rgb(1.0, 0.07, 0.08, 0.09)
        cr.set_source(pat_ant)
        cr.move_to(ax - 22, by + 5)
        cr.line_to(ax - 16, 2)
        cr.line_to(ax + 16, 2)
        cr.line_to(ax + 22, by + 5)
        cr.close_path()
        cr.fill()

        # Center LED flashlight anchor (drawn dynamically in paint() for 60 FPS page-flash strobe!)
        tx = bx + bw * 0.48
        self._torch_anchor = (tx, by, True)
        self.hitboxes["_torch"] = (tx - 18, by - 16, 36, 20)

        # 3D Orange-Capped Volume Knob + Guard Ear
        kx = bx + bw - 62.0
        pat_kb = cairo.LinearGradient(kx - 28, 0, kx + 28, 0)
        pat_kb.add_color_stop_rgb(0.0, 0.08, 0.09, 0.10)
        pat_kb.add_color_stop_rgb(0.35, 0.24, 0.25, 0.27)
        pat_kb.add_color_stop_rgb(1.0, 0.07, 0.08, 0.09)
        cr.set_source(pat_kb)
        _round_rect(cr, kx - 27, by - 28, 54, 32, 5)
        cr.fill()
        _round_rect(cr, kx - 27, by - 40, 54, 14, 5)
        pat_o = cairo.LinearGradient(kx - 27, by - 40, kx + 27, by - 26)
        pat_o.add_color_stop_rgb(0.0, 0.92, 0.36, 0.04)
        pat_o.add_color_stop_rgb(0.40, 1.00, 0.56, 0.14)
        pat_o.add_color_stop_rgb(1.0, 0.80, 0.28, 0.03)
        cr.set_source(pat_o)
        cr.fill()
        _round_rect(cr, bx + bw - 22, by - 28, 15, 32, 4)
        cr.set_source_rgb(0.12, 0.13, 0.15)
        cr.fill()

        # Frosted Status LED
        _draw_frosted_led(cr, bx + 44.0, by - 9.0, 26.0, 11.0, led_state)

        # 3D Matte Polycarbonate Lower Body
        _round_rect(cr, bx, by, bw, bh, 18)
        pat_b = cairo.LinearGradient(bx, by, bx + bw, by)
        pat_b.add_color_stop_rgb(0.0, 0.24, 0.25, 0.27)
        pat_b.add_color_stop_rgb(0.08, 0.14, 0.15, 0.17)
        pat_b.add_color_stop_rgb(0.92, 0.13, 0.14, 0.16)
        pat_b.add_color_stop_rgb(1.0, 0.22, 0.23, 0.25)
        cr.set_source(pat_b)
        cr.fill_preserve()
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.10)
        cr.set_line_width(1.0)
        cr.stroke()

        # 3D Brushed Titanium Upper LCD Armor Frame
        ux, uy, uw, uh = bx + 6.0, by + 6.0, bw - 12.0, 220.0
        _round_rect(cr, ux, uy, uw, uh, 14)
        pat_u = cairo.LinearGradient(ux, uy, ux + uw, uy + uh)
        pat_u.add_color_stop_rgb(0.0, 0.72, 0.70, 0.66)
        pat_u.add_color_stop_rgb(0.45, 0.54, 0.53, 0.50)
        pat_u.add_color_stop_rgb(1.0, 0.64, 0.62, 0.58)
        cr.set_source(pat_u)
        cr.fill_preserve()
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.28)
        cr.set_line_width(1.1)
        cr.stroke()

        _draw_hex_torx_bolt(cr, ux + 15, uy + 15, 4.4)
        _draw_hex_torx_bolt(cr, ux + uw - 15, uy + 15, 4.4)
        _draw_hex_torx_bolt(cr, ux + 15, uy + uh - 54, 4.4)
        _draw_hex_torx_bolt(cr, ux + uw - 15, uy + uh - 22, 4.4)

        # Recessed LCD Window
        lz_x, lz_y, lz_w, lz_h = int(ux + 18), int(uy + 12), int(uw - 36), 154
        _round_rect(cr, lz_x, lz_y, lz_w, lz_h, 8)
        cr.set_source_rgb(0.07, 0.07, 0.08)
        cr.fill()
        self._lcd_rect = (lz_x + 6, lz_y + 6, lz_w - 12, lz_h - 12)

        # Glossy QUANSHENG Badge right of [M A]
        m_x, m_y, m_w, m_h = ux + 16.0, lz_y + lz_h + 10.0, 72.0, 34.0
        np_x, np_y, np_w, np_h = m_x + m_w + 10.0, m_y + 2.0, uw - (m_w + 56.0), 30.0
        _round_rect(cr, np_x, np_y, np_w, np_h, 7)
        cr.set_source_rgb(0.06, 0.06, 0.07)
        cr.fill()
        _draw_quansheng_logo(cr, np_x + np_w * 0.5, np_y + np_h * 0.52, scale=0.86)

        # 3D Speaker Grille Dish (right of vertical ▲ / ▼)
        mid_y = uy + uh + 12.0
        col_x, col_w = bx + 18.0, 60.0
        gx = col_x + col_w + 14.0
        gw = (bx + bw - 18.0) - gx
        _round_rect(cr, gx - 4, mid_y, gw + 8, 88.0, 8.0)
        cr.set_source_rgba(0.04, 0.04, 0.05, 0.62)
        cr.fill()
        for i in range(3):
            gy = mid_y + 8.0 + i * 26.0
            _round_rect(cr, gx, gy, gw, 16.0, 5.0)
            cr.set_source_rgb(0.04, 0.04, 0.05)
            cr.fill()
            _round_rect(cr, gx + 3, gy + 4, gw - 6, 8.0, 3.0)
            pat_sl = cairo.LinearGradient(0, gy + 4, 0, gy + 12)
            pat_sl.add_color_stop_rgb(0.0, 0.25, 0.26, 0.28)
            pat_sl.add_color_stop_rgb(1.0, 0.14, 0.15, 0.17)
            cr.set_source(pat_sl)
            cr.fill()

        # Front Key Specs (side specs already registered above before shroud)
        nav_specs = [
            ("menu", m_x, m_y, m_w, m_h, "M", "A", "", 6.0, False, False),
            ("up",   col_x, mid_y, col_w, 36.0, "_CHEV_UP_", "B", "", 7.0, True, False),
            ("down", col_x, mid_y + 44.0, col_w, 36.0, "_CHEV_DN_", "C", "", 7.0, True, False),
            ("exit", col_x, mid_y + 94.0, col_w + 14.0, 30.0, "EXIT", "", "", 6.0, True, False),
        ]
        for sp in nav_specs:
            self.hitboxes[sp[0]] = (sp[1], sp[2], sp[3], sp[4])
            self._key_specs.append(sp)

        kp_x, kp_y, kp_w = bx + 16.0, mid_y + 132.0, bw - 32.0
        gap_c, gap_r, kh = 6.0, 7.0, 34.0
        kw = (kp_w - gap_c * 3.0) / 4.0
        for r_i, row_keys in enumerate(K5_BOTTOM_MATRIX):
            ky = kp_y + r_i * (kh + gap_r)
            for c_i, (kn, pri, s1) in enumerate(row_keys):
                kx = kp_x + c_i * (kw + gap_c)
                self.hitboxes[kn] = (kx, ky, kw, kh)
                self._key_specs.append((kn, kx, ky, kw, kh, pri, s1, "", 6.0, False, kn == "5"))

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
        import cairo
        self._ensure_chassis(model, led_state, lcd_st)
        accent = (0.98, 0.48, 0.08) if model == "k5" else (0.18, 0.56, 1.0)
        bx = self._body_bx

        # 1. Cached full-height shadowed left back-shell plastic flank + drop shadow
        cr.set_source_surface(self._back_surf, 0, 0)
        cr.paint()

        # 2. 3D Sculpted Protruding Side Keys (drawn BEHIND front chassis lip so they slide
        #    physically INTO the side housing instead of looking slapped on top!)
        for kn, sx, sy, sw, sh, is_ptt, dots in self._side_specs:
            p = _clamp1(press_anim.get(kn, 0.0))
            hv = _clamp1(hover_anim.get(kn, 0.0))
            dx = 3.2 * p
            # Soft contact shadow cast left/down onto back-shell plastic
            _round_rect(cr, sx + dx - 1.4, sy + 2.2, sw, sh, 8 if is_ptt else 6)
            cr.set_source_rgba(0.0, 0.0, 0.0, 0.52 * (1.0 - 0.5 * p))
            cr.fill()

            # 3D rounded cylindrical side paddle body
            _round_rect(cr, sx + dx, sy, sw, sh, 8 if is_ptt else 6)
            pat_s = cairo.LinearGradient(sx + dx, sy, sx + dx + sw, sy)
            if is_ptt:
                pat_s.add_color_stop_rgb(0.0, 0.25 + 0.38 * p + 0.06 * hv, 0.26 - 0.10 * p + 0.06 * hv, 0.29 - 0.12 * p + 0.06 * hv)
                pat_s.add_color_stop_rgb(0.45, 0.15 + 0.42 * p, 0.16 - 0.04 * p, 0.18 - 0.05 * p)
                pat_s.add_color_stop_rgb(0.85, 0.07 + 0.25 * p, 0.075, 0.085)
                pat_s.add_color_stop_rgb(1.0, 0.03, 0.03, 0.04)
            else:
                v = 0.23 + 0.06 * hv - 0.07 * p
                pat_s.add_color_stop_rgb(0.0, v, v + 0.01, v + 0.02)
                pat_s.add_color_stop_rgb(0.55, v * 0.65, v * 0.67, v * 0.71)
                pat_s.add_color_stop_rgb(1.0, v * 0.30, v * 0.31, v * 0.34)
            cr.set_source(pat_s)
            cr.fill_preserve()

            if is_ptt:
                # 3D beveled anodized collar (cobalt-blue on UV-K1, burnt-orange on UV-K5, crimson on TX)
                c_r = accent[0] * (1.0 - p) + 0.98 * p
                c_g = accent[1] * (1.0 - p) + 0.30 * p
                c_b = accent[2] * (1.0 - p) + 0.28 * p
                pat_c = cairo.LinearGradient(sx + dx, sy, sx + dx, sy + sh)
                pat_c.add_color_stop_rgb(0.0, min(1.0, c_r * 1.15), min(1.0, c_g * 1.15), min(1.0, c_b * 1.15))
                pat_c.add_color_stop_rgb(0.5, c_r * 0.88, c_g * 0.88, c_b * 0.88)
                pat_c.add_color_stop_rgb(1.0, c_r * 0.56, c_g * 0.56, c_b * 0.56)
                cr.set_source(pat_c)
                cr.set_line_width(2.2)
                cr.stroke()
                if model == "k1":
                    for ry in (sy + 20, sy + 44, sy + 68, sy + 92):
                        _round_rect(cr, sx + 6.4 + dx, ry - 0.8, 5.2, 14.6, 2.4)
                        cr.set_source_rgba(0.02, 0.02, 0.03, 0.75)
                        cr.fill()
                        _round_rect(cr, sx + 7.2 + dx, ry, 3.6, 13.0, 1.7)
                        cr.set_source_rgb(
                            accent[0] * (1.0 - p) + 1.0 * p,
                            accent[1] * (1.0 - p) + 0.72 * p,
                            accent[2] * (1.0 - p) + 0.72 * p,
                        )
                        cr.fill()
            else:
                cr.set_source_rgba(accent[0], accent[1], accent[2], 0.88 * p) if p > 0.05 else cr.set_source_rgba(1, 1, 1, 0.14 + 0.08 * hv)
                cr.set_line_width(1.0)
                cr.stroke()
                if dots > 0:
                    cr.set_source_rgba(0.84, 0.87, 0.90, 0.72)
                    if dots == 2:
                        cr.new_path()
                        cr.arc(sx + 8.0 + dx, sy + sh * 0.37, 1.9, 0, 2 * math.pi)
                        cr.fill()
                        cr.new_path()
                        cr.arc(sx + 8.0 + dx, sy + sh * 0.63, 1.9, 0, 2 * math.pi)
                        cr.fill()
                    else:
                        cr.new_path()
                        cr.arc(sx + 8.0 + dx, sy + sh * 0.50, 1.9, 0, 2 * math.pi)
                        cr.fill()

            # Overhanging front-chassis shadow cast onto right root of the side button!
            sh_ov = cairo.LinearGradient(bx - 7.5, 0, bx + 1.0, 0)
            sh_ov.add_color_stop_rgba(0.0, 0.0, 0.0, 0.0, 0.0)
            sh_ov.add_color_stop_rgba(1.0, 0.0, 0.0, 0.0, 0.78)
            cr.set_source(sh_ov)
            cr.rectangle(bx - 7.5, sy - 1.0, 8.5, sh + 2.0)
            cr.fill()

        # 3. Cached Front 3D Radio Chassis ON TOP (front bezel cleanly overhangs side button roots!)
        cr.set_source_surface(self._chassis_surf, 0, 0)
        cr.paint()

        # 3b. Dynamic top flashlight LED (zero-cost strobe during firmware page flashing!)
        tx, ty_base, is_compact = self._torch_anchor
        _draw_top_flashlight(cr, tx, ty_base, torch_on, compact=is_compact)

        # 4. Fast layered ST7565 LCD
        lx, ly, lw, lh = self._lcd_rect
        paint_fast_lcd(cr, lx, ly, lw, lh, lcd_state, lcd_st, self.lcd_cache)

        # 5. Dynamic 3D Front Keycap Overlays (only animating/hovered keys redraw live!)
        for kn, kx, ky, kw, kh, pri, s1, s2, rad, is_nav, nub in self._key_specs:
            pt = press_anim.get(kn, 0.0)
            ht = hover_anim.get(kn, 0.0)
            if pt > 0.005 or ht > 0.005:
                _draw_3d_keycap(
                    cr, kx, ky, kw, kh, pri, s1, s2,
                    press_t=pt, hover_t=ht, radius=rad, is_nav=is_nav, homing_nub=nub, accent=accent,
                )


# =============================================================================
# 3. PNG & GIF RECORDER / TIMELINE SPLICE EXPORTER
# =============================================================================

def _bgra_to_rgba(raw: bytes) -> bytes:
    if _np is not None:
        return _np.frombuffer(raw, dtype=_np.uint8).reshape(-1, 4)[:, [2, 1, 0, 3]].tobytes()
    out = bytearray(len(raw))
    out[0::4], out[1::4], out[2::4], out[3::4] = raw[2::4], raw[1::4], raw[0::4], raw[3::4]
    return bytes(out)


def render_lcd_to_rgba(fb: bytes, lcd_st: LcdSettings, width: int = 440, height: int = 304) -> tuple[int, int, bytes]:
    import cairo
    surf = cairo.ImageSurface(cairo.FORMAT_ARGB32, width, height)
    cr = cairo.Context(surf)
    st_s = dataclasses.replace(lcd_st, ghosting=False)
    fs = FastLcdState()
    fs.step(fb, st_s)
    paint_fast_lcd(cr, 0, 0, width, height, fs, st_s, LcdLayerCache())
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
    st_s = dataclasses.replace(lcd_st, ghosting=False)
    fs = FastLcdState()
    fs.step(fb, st_s)
    rend = renderer or HandsetRenderer()
    rend.paint(
        cr, model, fs, st_s,
        press_anim={k: 1.0 for k in pressed_keys},
        hover_anim={}, led_state=led_state, torch_on=torch_on,
    )
    surf.flush()
    return w, h, _bgra_to_rgba(bytes(surf.get_data()))


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
    def __init__(self) -> None:
        self.frames: list[RecordedFrame] = []
        self.recording: bool = False
        self.t0_mono: float = 0.0
        self.trim_in: int = 0
        self.trim_out: int = 0
        self.cuts: list[tuple[int, int]] = []

    def start(self, initial_fb: bytes, pressed: frozenset[str], led: str, torch: bool) -> None:
        self.frames = [RecordedFrame(0, bytes(initial_fb), frozenset(pressed), led, torch)]
        self.trim_in = self.trim_out = 0
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
        self.frames.append(RecordedFrame(int((time.monotonic() - self.t0_mono) * 1000.0), bytes(fb), frozenset(pressed), led, torch))
        self.trim_out = len(self.frames) - 1

    def stop(self) -> None:
        if self.recording:
            self.recording = False
            self.trim_out = max(0, len(self.frames) - 1)

    def add_cut(self, start_idx: int, end_idx: int) -> None:
        a, b = max(self.trim_in, min(start_idx, end_idx)), min(self.trim_out, max(start_idx, end_idx))
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
        pil_frames, delays = [], []
        for pos, (fi, dt_ms) in enumerate(seq):
            rf = self.frames[fi]
            if mode == "body":
                w, h, rgba = render_handset_to_rgba(model, rf.fb, lcd_st, pressed_keys=rf.pressed, led_state=rf.led, torch_on=rf.torch, scale=1.0, renderer=rend)
            else:
                w, h, rgba = render_lcd_to_rgba(rf.fb, lcd_st, width=440, height=304)
            img = Image.frombytes("RGBA", (w, h), rgba).convert("RGB")
            pil_frames.append(img.quantize(colors=64 if mode == "lcd" else 128, method=Image.Quantize.MEDIANCUT, dither=Image.Dither.NONE))
            delays.append(dt_ms)
            if progress_cb:
                progress_cb((pos + 1) / float(len(seq)))
        dest = Path(out_path)
        pil_frames[0].save(dest, format="GIF", save_all=True, append_images=pil_frames[1:], duration=delays, loop=0, optimize=True, disposal=2)
        return dest.stat().st_size


# =============================================================================
# 4. GUDEV HOTPLUG + DFU / BOOTLOADER / CPS RECOGNITION + BACKUPS & FLASHING
# =============================================================================

VID_PID_STOCK_BL = "36b7:0001"
VID_PID_KERN_CDC = "36b7:ffff"


@dataclasses.dataclass
class DetectedPort:
    device: str
    pretty_name: str
    vendor: str = ""
    model: str = ""
    serial: str = ""
    vid: str = ""
    pid: str = ""
    mode_hint: str = "kern_os"  # "kern_os" | "stock_bl"
    is_quansheng: bool = False


def classify_usb_mode(vid: str, pid: str, pretty_name: str) -> tuple[str, bool]:
    """
    Classifies USB serial devices:
      • 36b7:ffff -> Kern OS native USB-C CDC (NEVER Moto UF2, which is USB MSC block storage!)
      • 36b7:0001 -> Quansheng K1/K5v3 CPS or Stock Bootloader (live-probed on connect!)
    """
    vp = f"{vid}:{pid}".lower() if (vid and pid) else ""
    low = pretty_name.lower()
    if vp == VID_PID_KERN_CDC:
        return "kern_os", True
    if vp == VID_PID_STOCK_BL:
        return "stock_bl", True
    is_qs = any(k in low for k in ("quansheng", "uv-k", "kern"))
    return "kern_os", is_qs


class PortMonitor:
    """Real-time USB hotplug monitor recognizing Kern OS CDC and Quansheng CPS/Bootloader ports."""

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
                v_c, m_c = vendor.replace("_", " ").strip(), model.replace("_", " ").strip()
                pretty = m_c if m_c.lower().startswith("quansheng") else f"{v_c} {m_c}".strip() or dev_file
                mode_hint, is_qs = classify_usb_mode(vid, pid, pretty)
                found.append(DetectedPort(dev_file, pretty, v_c, m_c, ser, vid, pid, mode_hint, is_qs))
        else:
            try:
                from serial.tools import list_ports
                for p in list_ports.comports():
                    desc = (p.product or p.description or "").strip()
                    vid = f"{p.vid:04x}" if p.vid is not None else ""
                    pid = f"{p.pid:04x}" if p.pid is not None else ""
                    mode_hint, is_qs = classify_usb_mode(vid, pid, desc)
                    found.append(DetectedPort(p.device, desc or p.device, model=desc, serial=p.serial_number or "", vid=vid, pid=pid, mode_hint=mode_hint, is_quansheng=is_qs))
            except Exception:
                pass

        found.sort(key=lambda d: (0 if d.is_quansheng else 1, d.device))
        changed = [(p.device, p.mode_hint) for p in found] != [(p.device, p.mode_hint) for p in self.ports]
        self.ports = found
        if changed and self.on_change is not None:
            self.on_change(found)
        return found


def probe_bootloader_beacon(port: str, duration_s: float = 0.85) -> dict[str, str] | None:
    """Listens briefly at 38400 baud for Quansheng stock bootloader / CPS 0x0518 beacon packets."""
    if _flasher is None or port.startswith("tcp://") or port == "demo":
        return None
    try:
        import serial
        ser = serial.Serial(port, baudrate=_flasher.BAUDRATE, timeout=0.0001)
    except Exception:
        return None
    try:
        ser.reset_input_buffer()
        proto = _flasher.Protocol(port)
        proto.ser = ser
        deadline = time.monotonic() + duration_s
        while time.monotonic() < deadline:
            msg = proto.fetch_message()
            if msg and msg[0] == _flasher.MSG_NOTIFY_DEV_INFO:
                data = msg[1]
                uid = data[:16].hex()
                ver = data[16:].split(b"\x00")[0].decode("utf-8", errors="ignore").strip()
                return {"version": ver or "Stock BL", "uid": uid}
            time.sleep(0.01)
    finally:
        try:
            ser.close()
        except Exception:
            pass
    return None


def inspect_firmware_file(fpath: str | Path) -> dict[str, Any]:
    """Inspects and pre-flight validates a `.kfw` container or raw `.bin` firmware image."""
    p = Path(fpath)
    data = p.read_bytes()
    if data[:8] == kfw.FILE_ID_KFW:
        parsed = kfw.parse_kfw(data, verify_root=True)
        pts = kfw.extract_all_parts(parsed)
        fw_parts = [pt for pt in parsed.parts if pt["kind"] == kfw.PART_FIRMWARE]
        targets = [t.slug for t in parsed.targets]
        if not fw_parts:
            raise ValueError("Container has no firmware image part (may be a backup/module pack)")
        raw_fw = pts[fw_parts[0]["index"]]
        return {
            "format": "KFW v1.0",
            "version": kfw.format_semver(parsed.fw_version),
            "targets": targets,
            "fw_bytes": raw_fw,
            "size": len(raw_fw),
            "summary": f"KFW v{kfw.format_semver(parsed.fw_version)} · {', '.join(targets) or 'generic'} · {len(raw_fw):,} B · Ascon OK",
        }
    if len(data) < 256 or len(data) > 128 * 1024:
        raise ValueError(f"Unexpected firmware size: {len(data):,} B")
    return {
        "format": "Raw BIN",
        "version": p.stem,
        "targets": ["raw"],
        "fw_bytes": data,
        "size": len(data),
        "summary": f"Raw binary · {p.name} · {len(data):,} B",
    }


def flash_via_stock_bootloader(
    port: str,
    fw_bytes: bytes,
    progress_cb: Callable[[float, str], None] | None = None,
    page_cb: Callable[[int, int], None] | None = None,
) -> dict[str, Any]:
    """Flashes raw firmware bytes via Quansheng stock bootloader / CPS / Kern dfu.mod protocol."""
    if _flasher is None:
        raise RuntimeError("tools/flasher.py unavailable")
    import serial

    ser = serial.Serial(port, baudrate=_flasher.BAUDRATE, timeout=0.0001, write_timeout=2.0)
    proto = _flasher.Protocol(port)
    proto.ser = ser
    try:
        ser.reset_input_buffer()
        if progress_cb:
            progress_cb(0.02, "Waiting for bootloader beacon…")

        # 1. Beacon sync (5 consistent 0x0518 frames)
        t0 = time.monotonic()
        last_ts = 0.0
        acc = 0
        dev_info = None
        while time.monotonic() - t0 < 12.0:
            msg = proto.fetch_message()
            if not msg:
                time.sleep(0.008)
                continue
            if msg[0] == _flasher.MSG_NOTIFY_DEV_INFO:
                now = time.monotonic()
                dt = (now - last_ts) * 1000.0
                last_ts = now
                if 5.0 <= dt <= 1000.0:
                    acc += 1
                    if acc >= 4:
                        uid = msg[1][:16].hex()
                        ver = msg[1][16:].split(b"\x00")[0].decode("utf-8", errors="ignore").strip()
                        dev_info = (uid, ver or "7.00")
                        break
                else:
                    acc = 0
        if dev_info is None:
            raise TimeoutError("Bootloader beacon timeout — ensure radio is in DFU / PTT+Power-On mode")

        uid, ver = dev_info
        if progress_cb:
            progress_cb(0.08, f"Bootloader {ver} · Handshaking…")

        # 2. Handshake reply
        sent = 0
        for _ in range(25):
            time.sleep(0.04)
            msg = proto.fetch_message()
            if msg and msg[0] == _flasher.MSG_NOTIFY_DEV_INFO:
                if sent == 0:
                    proto.send(_flasher.MSG_NOTIFY_BL_VER, ver[:4].encode("utf-8"))
                sent += 1
                if sent >= 3:
                    break
        time.sleep(0.18)
        while proto.fetch_message():
            pass

        # 3. Page-by-page flash
        page_sz = 256
        total_pages = math.ceil(len(fw_bytes) / page_sz)
        ts = int(time.time()) & 0xFFFFFFFF
        t_start = time.monotonic()

        for page in range(total_pages):
            chunk = fw_bytes[page * page_sz : (page + 1) * page_sz]
            payload = bytearray(268)
            struct.pack_into("<IHH", payload, 0, ts, page, total_pages)
            payload[12 : 12 + len(chunk)] = chunk

            ok = False
            for _attempt in range(3):
                proto.send(_flasher.MSG_PROG_FW, payload)
                wait_end = time.monotonic() + 3.0
                while time.monotonic() < wait_end:
                    resp = proto.fetch_message()
                    if not resp:
                        time.sleep(0.004)
                        continue
                    if resp[0] == _flasher.MSG_PROG_FW_RESP and len(resp[1]) >= 8:
                        r_page, r_err = struct.unpack_from("<HH", resp[1], 4)
                        if r_page == page and r_err == 0:
                            ok = True
                            break
                if ok:
                    break
            if not ok:
                raise RuntimeError(f"Flash page {page + 1}/{total_pages} failed")
            if page_cb:
                page_cb(page + 1, total_pages)
            if progress_cb:
                frac = 0.10 + 0.88 * ((page + 1) / float(total_pages))
                progress_cb(frac, f"Flashing page {page + 1}/{total_pages}")

        try:
            proto.send(_flasher.MSG_REBOOT)
        except Exception:
            pass
        if progress_cb:
            progress_cb(1.0, "Complete — rebooting radio")
        return {"bootloader": ver, "uid": uid, "pages": total_pages, "elapsed_s": time.monotonic() - t_start}
    finally:
        try:
            ser.close()
        except Exception:
            pass


def build_kfw_backup_from_backend(
    be: kernfs_mount.FsBackend,
    *,
    volume_name: str,
    calib_only: bool = False,
    progress_cb: Callable[[float, str], None] | None = None,
) -> bytes:
    """Packs verified factory calibration (or full KernFS snapshot) into an Ascon-Hash256 `.kfw` backup container."""
    files_to_pack: list[tuple[str, bytes]] = []
    if calib_only:
        paths = ["/sys/calib.dat", "/sys/calib.sig"]
    else:
        paths = []
        for root_dir in ("/sys", "/pref", "/user", "/apps"):
            try:
                for entry in be.ls(root_dir):
                    if not entry["is_dir"]:
                        paths.append(f"{root_dir}/{entry['name']}")
            except Exception:
                pass

    total = max(1, len(paths))
    for i, path in enumerate(paths):
        if progress_cb:
            progress_cb(i / float(total), f"Reading {path}…")
        try:
            st = be.stat(path)
            if st.get("exists") and not st.get("is_dir"):
                files_to_pack.append((path, be.read_bytes(path)))
        except Exception:
            continue

    if not files_to_pack:
        raise RuntimeError("No readable files found to back up")

    parts: list[kfw.PartInput] = []
    for path, data in files_to_pack:
        pkind = kfw.PART_CALIBRATION if path == "/sys/calib.dat" else kfw.PART_KERNFS_FILE
        parts.append(kfw.PartInput(kind=pkind, data=data, dest_path=path, in_solid=True))

    t0, bp0, prs0 = kfw._make_target_entries("uv-k1", 0)
    if progress_cb:
        progress_cb(0.95, "Sealing Ascon-Hash256 .kfw backup…")
    blob = kfw.build_kfw(
        kind=kfw.KIND_BACKUP,
        fw_version=kfw.pack_semver("1.0.0"),
        targets=[t0],
        boot_policies=[bp0],
        protect_ranges=prs0,
        parts=parts,
        manifest={
            "name": f"{volume_name} Backup",
            "scope": "calibration" if calib_only else "full-kernfs",
            "timestamp": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
            "file_count": len(files_to_pack),
        },
    )
    if progress_cb:
        progress_cb(1.0, f"Backed up {len(files_to_pack)} file(s)")
    return blob


def restore_kfw_backup_to_backend(
    kfw_path: str | Path,
    be: kernfs_mount.FsBackend,
    policy: kernfs_mount.SafetyPolicy,
    progress_cb: Callable[[float, str], None] | None = None,
) -> tuple[int, int]:
    """Verifies Ascon digests & restores user/pref/app files from a `.kfw` backup while strictly shielding `/sys/*`."""
    parsed = kfw.parse_kfw(Path(kfw_path).read_bytes(), verify_root=True)
    pts = kfw.extract_all_parts(parsed)
    restored = skipped = 0
    file_parts = [
        pt for pt in parsed.parts
        if pt["index"] in parsed.part_paths or pt["kind"] == kfw.PART_CALIBRATION
    ]
    total = max(1, len(file_parts))
    for i, pt in enumerate(file_parts):
        if pt["kind"] == kfw.PART_CALIBRATION:
            skipped += 1
            continue
        path = parsed.part_paths[pt["index"]]["path"]
        data = pts[pt["index"]]
        if progress_cb:
            progress_cb(i / float(total), f"Restoring {path}…")
        if path.startswith("/sys/"):
            skipped += 1
            continue
        policy.check_write(path, data)
        be.write_bytes(path, data)
        restored += 1
    if progress_cb:
        progress_cb(1.0, f"Restored {restored} file(s) ({skipped} protected /sys skipped)")
    return restored, skipped


# =============================================================================
# 5. DEMO FRAMEBUFFERS & LOW-LATENCY RIGWIRE ENGINE
# =============================================================================

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


def make_demo_framebuffer(freq_a: str = "433.500", freq_b: str = "145.500", status: str = "VFO") -> bytes:
    fb = bytearray(1024)
    for x in range(128):
        for y in range(9):
            fb[(y >> 3) * 128 + x] |= 1 << (y & 7)
    _fb_draw_text(fb, 3, 1, "KERN OS", 1, invert=True)
    _fb_draw_text(fb, 60, 1, status[:5], 1, invert=True)
    _fb_draw_text(fb, 104, 1, "92%", 1, invert=True)

    for x in range(2, 18):
        for y in range(13, 23):
            fb[(y >> 3) * 128 + x] |= 1 << (y & 7)
    _fb_draw_text(fb, 4, 15, "V1", 1, invert=True)
    _fb_draw_text(fb, 22, 13, freq_a, 2)
    _fb_draw_text(fb, 112, 13, "FM", 1)

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


def _make_demo_krx(name: str, payload_size: int = 64) -> bytes:
    """Build a minimal valid KRX v1 container for demo virtual KernFS."""
    name_bytes = name.encode("utf-8") + b"\x00"
    sec1_body = bytearray(40)
    sec2_body = bytearray(28)
    # Place name string right after the two sections inside the header.
    header_sections_len = 4 + len(sec1_body) + 4 + len(sec2_body)
    name_off = 24 + header_sections_len
    struct.pack_into("<HBBB", sec1_body, 0, name_off, 1, 0, 0)
    # Execution section: min_stack=256, entry_point=0, exec_payload_size=payload_size
    struct.pack_into("<HHHBBII", sec2_body, 0, 256, 0, 0, 0, 0, 0, payload_size)
    sec_blob = (
        struct.pack("<HH", 0x0001, 4 + len(sec1_body)) + bytes(sec1_body)
        + struct.pack("<HH", 0x0002, 4 + len(sec2_body)) + bytes(sec2_body)
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
        0,  # Resident
        1,  # version 1
        2,  # section_count
        header_len,
        0,
        pay_crc,
        hdr_crc,
        0,
    )
    return preamble + sec_blob + payload


class DockLink:
    """Pipelined low-latency RigWire & Bootloader/DFU link."""

    def __init__(self) -> None:
        self.lock = threading.RLock()
        self.client: rigwire.RigWireClient | None = None
        self.fs_backend: kernfs_mount.FsBackend | None = None
        self.policy = kernfs_mount.SafetyPolicy(read_only=False)
        self.port: str | None = None
        self.baud: int = 115200
        self.link_mode: str = "disconnected"  # "disconnected" | "kern_os" | "stock_bl" | "moto_uf2"
        self.bootloader_info: dict[str, str] = {}
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
        self._wake_ev = threading.Event()
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

        # 1. Offline Demo
        if port.lower() == "demo":
            self.demo_mode = True
            self.link_mode = "kern_os"
            self.port = "demo"
            self.detected_model = model_override if model_override in ("k1", "k5") else "k1"
            self.volume_name = "Quansheng UV-K5 v3 DEMO" if self.detected_model == "k5" else "Quansheng UV-K1 DEMO"
            self.hello_info = {"minor_version": 1, "accepted_max_payload": 128}
            self.last_telemetry = {"serial_str": "DEMO0001K1", "mac_addr": "02:4B:45:52:4E:01", "battery_mv": 8040, "battery_pct": 92}
            mem = kernfs_mount.MemoryFsBackend()
            mem.files["/sys/calib.dat"] = b"\xAA" * 512
            mem.files["/sys/calib.sig"] = b"\xBB" * 32
            mem.files["/apps/dfu.mod"] = _make_demo_krx("dfu", 128)
            mem.files["/apps/sysinfo.krx"] = _make_demo_krx("sysinfo", 256)
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

        # 2. Always attempt RigWire (Kern OS) first on every serial/TCP port (36b7:ffff is Kern OS CDC!).
        # Only fall back to Stock Bootloader mode if an authentic 0x0518 beacon frame is actually received.
        with self.lock:
            last_err: Exception | None = None
            cli: rigwire.RigWireClient | None = None
            hello: dict[str, Any] = {}
            telem: dict[str, Any] = {}
            for attempt in range(2):
                cand = rigwire.RigWireClient(port, baudrate=baud, timeout=0.75)
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
                        time.sleep(0.08)

            if cli is None:
                # Verify real 38400-baud Stock Bootloader 0x0518 beacon (NEVER assume bootloader without a real beacon!)
                bl = probe_bootloader_beacon(port, duration_s=0.85)
                if bl is not None:
                    self.port = port
                    self.link_mode = "stock_bl"
                    self.bootloader_info = bl
                    self.volume_name = f"Bootloader {bl['version']}"
                    self.detected_model = model_override or detect_hardware_model(
                        port_info.pretty_name if port_info else "", port_info=port_info
                    )
                    self.connected = True
                    # Authentic Stock Bootloader hardware state: LCD is unlit/off and top flashlight LED is ON!
                    self.last_fb = bytes(1024)
                    self.torch_on = True
                    self._emit_log(f"Bootloader {bl['version']} beacon verified on {port}")
                    if self.on_frame:
                        self.on_frame()
                    if self.on_telemetry:
                        self.on_telemetry()
                    return
                raise last_err or RuntimeError(f"No RigWire or Bootloader response on {port}")

            self.client = cli
            self.client.timeout = 0.45  # Fast interactive recovery (never 2.5s freeze!)
            self.fs_backend = kernfs_mount.RigWireFsBackend(cli, cache_ttl=2.0, shared_lock=self.lock)
            self.port = port
            self.link_mode = "kern_os"
            self.hello_info = hello
            self.last_telemetry = telem
            self.torch_on = bool(telem.get("flashlight", False))
            self.keypad_locked = bool(telem.get("keypad_locked", False))
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
        self.link_mode = "disconnected"
        self.bootloader_info = {}
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
        if self.link_mode != "kern_os":
            return
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
        self._wake_ev.set()

    def set_flashlight(self, enabled: bool) -> None:
        self.torch_on = enabled
        if self.demo_mode:
            self._emit_log(f"Flashlight {'ON' if enabled else 'OFF'}")
            return
        if self.connected and self.link_mode == "kern_os":
            self._cmd_q.put(("torch", 1 if enabled else 0))
            self._wake_ev.set()

    def set_keypad_lock(self, locked: bool) -> None:
        self.keypad_locked = locked
        if self.demo_mode:
            self._emit_log(f"Keypad lock {'ON' if locked else 'OFF'}")
            return
        if self.connected and self.link_mode == "kern_os":
            self._cmd_q.put(("lock", locked))
            self._wake_ev.set()

    def toggle_vfo(self) -> None:
        if self.demo_mode:
            self._emit_log("Swapped active VFO")
            return
        if self.connected and self.link_mode == "kern_os":
            self._cmd_q.put(("vfo", None))
            self._wake_ev.set()

    def run_wire_command(self, cmd_line: str) -> str:
        parts = cmd_line.strip().split()
        if not parts:
            return ""
        op = parts[0].lower()
        if op == "help":
            return "Commands: ping, hello, ls [dir], stat <path>, key <name> [press|hold|release], vfo, freq <MHz>, torch <0|1>, lock <0|1>, clear"
        if not self.connected or self.link_mode != "kern_os":
            return f"RigWire unavailable (mode={self.link_mode})."
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
            raise RuntimeError("Connect to a radio running Kern OS first")
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
            for attempt in range(2):
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
                    break
                except Exception:
                    if attempt == 0:
                        try:
                            self.client.resync()
                            continue
                        except Exception:
                            pass
                    self.error_count += 1

    def _poll_loop(self) -> None:
        # Adaptive 45 FPS pacing (22 ms budget) with single-in-flight Delta+RLE frames
        # (pipeline=1 avoids CDC_RX_BUF_SIZE overflow when keys are injected) + instant
        # auto-resync via HELLO on any dropped packet (prevents StaleSequence freezes!).
        target_dt = 1.0 / 45.0
        frames = 0
        t_fps = time.monotonic()
        last_telem = time.monotonic()
        while not self._stop.is_set() and self.connected:
            t_frame = time.monotonic()
            try:
                with self.lock:
                    if not self.client:
                        break
                    self._drain_priority_cmds()
                    fb = self.client.screencap(on_between_chunks=self._drain_priority_cmds, pipeline=1, delta=True)
                    self.rtt_ms = self.client.last_rtt_ms
                if len(fb) == 1024:
                    changed = fb != self.last_fb
                    self.last_fb = fb
                    frames += 1
                    if changed and self.on_frame is not None:
                        self.on_frame()
            except Exception:
                # Auto-resync RigWire sequence=1 immediately so one dropped USB frame NEVER freezes the stream!
                recovered = False
                try:
                    with self.lock:
                        if self.client:
                            self.client.resync()
                            recovered = True
                except Exception:
                    pass
                if not recovered:
                    self.error_count += 1
                    self._wake_ev.wait(0.08)
                    self._wake_ev.clear()
                continue

            now = time.monotonic()
            if now - t_fps >= 1.0:
                self.fps = frames / (now - t_fps)
                frames = 0
                t_fps = now
                if self.on_telemetry is not None:
                    self.on_telemetry()

            if now - last_telem >= 3.0:
                last_telem = now
                try:
                    with self.lock:
                        if self.client:
                            met = self.client.handle_get(rigwire.DOMAIN_SYSTEM, 0, rigwire.FACET_SYS_METRICS)
                            if len(met) >= 14:
                                self.last_telemetry["uptime_ms"] = struct.unpack("<I", met[:4])[0]
                                self.last_telemetry["sysclk_hz"] = struct.unpack("<I", met[4:8])[0]
                                self.last_telemetry["battery_mv"] = struct.unpack("<H", met[8:10])[0]
                                self.last_telemetry["battery_pct"] = met[10]
                                self.last_telemetry["active_app"] = met[11]
                                self.keypad_locked = bool(met[12])
                                self.torch_on = bool(met[13])
                    if self.on_telemetry is not None:
                        self.on_telemetry()
                except Exception:
                    pass

            rem = target_dt - (time.monotonic() - t_frame)
            if rem > 0.001 and self._cmd_q.empty():
                self._wake_ev.wait(rem)
            self._wake_ev.clear()


# =============================================================================
# 6. GNOME HIG NATIVE GTK4 + LIBADWAITA APPLICATION
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
.dfu-dot {
    min-width: 8px;
    min-height: 8px;
    border-radius: 99px;
    background: #f59e0b;
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


_RADIO_ICON_SVG = b"""<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 16 16">
  <rect x="4" y="1" width="1.8" height="4" rx="0.9" fill="#e5e7eb"/>
  <rect x="10.2" y="2.8" width="2.2" height="2.2" rx="0.6" fill="#e5e7eb"/>
  <path d="M4 5h8a1.5 1.5 0 0 1 1.5 1.5v7A1.5 1.5 0 0 1 12 15H4a1.5 1.5 0 0 1-1.5-1.5v-7A1.5 1.5 0 0 1 4 5zm.5 2v3h7V7h-7zm.5 4.2a.7.7 0 1 0 0 1.4.7.7 0 0 0 0-1.4zm3 0a.7.7 0 1 0 0 1.4.7.7 0 0 0 0-1.4zm3 0a.7.7 0 1 0 0 1.4.7.7 0 0 0 0-1.4z" fill="#e5e7eb" fill-rule="evenodd"/>
</svg>"""


def _make_radio_icon_widget(Gtk_mod: Any, Gdk_mod: Any, GLib_mod: Any, size: int = 16) -> Any:
    """Crisp 16x16 handheld transceiver symbolic icon for radio rows & connection pill."""
    try:
        tex = Gdk_mod.Texture.new_from_bytes(GLib_mod.Bytes.new(_RADIO_ICON_SVG))
        img = Gtk_mod.Image.new_from_paintable(tex)
        img.set_pixel_size(size)
        return img
    except Exception:
        img = Gtk_mod.Image.new_from_icon_name("network-wireless-symbolic")
        img.set_pixel_size(size)
        return img


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
            self.allow_usb_tx = False  # Hard RF RFI safety lock: PTT blocked over USB unless explicitly armed!
            self._flashing_active = False
            self._pressed_keys: set[str] = set()
            self._hovered_key: str | None = None
            self._press_anim: dict[str, float] = {}
            self._hover_anim: dict[str, float] = {}
            self._anim_timer: int | None = None
            self._hold_timers: dict[str, int] = {}
            self._frame_pending = False
            self._updating_switches = False
            self.current_dir = "/"
            self._cached_dir_entries: list[dict[str, Any]] = []
            self._selected_fw: dict[str, Any] | None = None

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

            # --- Ultra-Clean GNOME HIG HeaderBar (NO tab spam!) ---
            header = Adw.HeaderBar() if has_adw else Gtk.HeaderBar()

            # Start: Compact Radio & Connection Popover Pill
            self.conn_menu_btn = Gtk.MenuButton()
            self.conn_menu_btn.set_tooltip_text("Radio connection, battery & hardware controls")
            st_box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=6)
            self.hdr_link_icon = _make_radio_icon_widget(Gtk, Gdk, GLib, 16)
            self.hdr_batt_icon = Gtk.Image(icon_name="battery-missing-symbolic", visible=False)
            self.hdr_dev_lbl = Gtk.Label(label="Connect")
            self.hdr_rec_dot = Gtk.Box(valign=Gtk.Align.CENTER, visible=False)
            self.hdr_rec_dot.add_css_class("rec-dot")
            self.hdr_dfu_dot = Gtk.Box(valign=Gtk.Align.CENTER, visible=False)
            self.hdr_dfu_dot.add_css_class("dfu-dot")
            for w in (self.hdr_link_icon, self.hdr_dev_lbl, self.hdr_batt_icon, self.hdr_dfu_dot, self.hdr_rec_dot):
                st_box.append(w)
            self.conn_menu_btn.set_child(st_box)
            self.conn_popover = self._build_connection_popover()
            self.conn_menu_btn.set_popover(self.conn_popover)
            header.pack_start(self.conn_menu_btn)

            # Center: Clean Adw.WindowTitle (NO random tabs in top window bar!)
            if has_adw:
                self.win_title = Adw.WindowTitle(title="Kern Dock", subtitle="Not connected")
                header.set_title_widget(self.win_title)
            else:
                self.win_title = None

            # End: Dock Panel Toggle + Primary Menu (Display Optics & Shortcuts)
            end_box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=4)
            self.pane_toggle = Gtk.ToggleButton(icon_name="sidebar-show-right-symbolic", active=True)
            self.pane_toggle.set_tooltip_text("Show/hide workspace sidebar (Ctrl+B)")
            end_box.append(self.pane_toggle)

            menu_btn = Gtk.MenuButton(icon_name="open-menu-symbolic")
            menu_btn.set_tooltip_text("Display optics & keyboard shortcuts")
            menu_btn.set_popover(self._build_primary_menu_popover())
            end_box.append(menu_btn)
            header.pack_end(end_box)

            # --- Context-Aware Auto Bootloader Banner (ONLY shown on verified 0x0518 beacon!) ---
            self.dfu_banner = Adw.Banner(revealed=False, button_label="Flash Firmware…") if has_adw else None
            if self.dfu_banner is not None:
                self.dfu_banner.connect("button-clicked", lambda _b: self._on_flash_firmware_dialog(None))

            # --- Left Pane: 3D Interactive Cairo Handset ---
            self.handset_area = Gtk.DrawingArea(halign=Gtk.Align.CENTER, valign=Gtk.Align.CENTER)
            self.handset_area.set_margin_start(14)
            self.handset_area.set_margin_end(14)
            self.handset_area.set_margin_top(10)
            self.handset_area.set_margin_bottom(12)
            self.handset_area.set_draw_func(self._draw_handset)

            # Natural press-and-hold gesture (never cancels on tiny pointer jitter!)
            g_l = Gtk.GestureClick(button=1)
            g_l.set_exclusive(True)
            g_l.connect("pressed", self._on_handset_press)
            g_l.connect("released", self._on_handset_release)
            g_l.connect("unpaired-release", lambda _g, _x, _y, _b, _s: self._release_all_pointer_keys())
            g_l.connect("cancel", lambda _g, _s: self._release_all_pointer_keys())
            self.handset_area.add_controller(g_l)

            g_r = Gtk.GestureClick(button=3)
            g_r.connect("pressed", self._on_handset_right_click)
            self.handset_area.add_controller(g_r)

            g_m = Gtk.EventControllerMotion()
            g_m.connect("motion", self._on_handset_motion)
            g_m.connect("leave", lambda _c: self._set_hovered_key(None))
            self.handset_area.add_controller(g_m)

            # --- Right Workspace Pane: Clean 3-View Switcher (Files · Capture · Console) ---
            self.dock_stack = Adw.ViewStack() if has_adw else Gtk.Stack()
            files_page = self._build_files_page()
            cap_page = self._build_capture_page()
            wire_page = self._build_wire_page()

            if has_adw:
                self.dock_stack.add_titled_with_icon(files_page, "files", "Files", "folder-symbolic")
                self.dock_stack.add_titled_with_icon(cap_page, "capture", "Capture", "camera-video-symbolic")
                self.dock_stack.add_titled_with_icon(wire_page, "wire", "Console", "utilities-terminal-symbolic")
            else:
                for w, nm, lbl in ((files_page, "files", "Files"), (cap_page, "capture", "Capture"), (wire_page, "wire", "Console")):
                    self.dock_stack.add_titled(w, nm, lbl)
            self.dock_stack.set_hexpand(True)
            self.dock_stack.set_vexpand(True)

            pane_col = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, hexpand=True, vexpand=True)
            # Compact segmented switcher cleanly nested inside the workspace pane
            sw_bar = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, halign=Gtk.Align.CENTER, margin_top=10, margin_bottom=4)
            sw_bar.add_css_class("linked")
            self._pane_btns: dict[str, Any] = {}
            first_pb = None
            for nm, lbl, icon in (
                ("files", "Files", "folder-symbolic"),
                ("capture", "Capture", "camera-video-symbolic"),
                ("wire", "Console", "utilities-terminal-symbolic"),
            ):
                tb_btn = Gtk.ToggleButton(active=(nm == "files"))
                if first_pb is None:
                    first_pb = tb_btn
                else:
                    tb_btn.set_group(first_pb)
                bi = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=6)
                bi.append(Gtk.Image.new_from_icon_name(icon))
                bi.append(Gtk.Label(label=lbl))
                tb_btn.set_child(bi)
                tb_btn.connect("toggled", lambda b, page_name=nm: self.dock_stack.set_visible_child_name(page_name) if b.get_active() else None)
                sw_bar.append(tb_btn)
                self._pane_btns[nm] = tb_btn
            pane_col.append(sw_bar)

            # Shared inline progress bar for flashing / backup / restore (hidden when idle)
            self.op_prog = Gtk.ProgressBar(show_text=True, visible=False, margin_start=12, margin_end=12, margin_top=4)
            pane_col.append(self.op_prog)
            pane_col.append(self.dock_stack)

            self.right_pane = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, hexpand=True)
            self.right_pane.set_size_request(448, -1)
            self.right_pane.append(Gtk.Separator(orientation=Gtk.Orientation.VERTICAL))
            self.right_pane.append(pane_col)
            self.pane_toggle.connect("toggled", self._on_dock_pane_toggled)

            split = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL)
            split.append(self.handset_area)
            split.append(self.right_pane)

            content_vbox = Gtk.Box(orientation=Gtk.Orientation.VERTICAL)
            if self.dfu_banner is not None:
                content_vbox.append(self.dfu_banner)
            content_vbox.append(split)

            if has_adw:
                self.toast_overlay = Adw.ToastOverlay(child=content_vbox)
                tb = Adw.ToolbarView()
                tb.add_top_bar(header)
                tb.set_content(self.toast_overlay)
                self.win.set_content(tb)
            else:
                self.toast_overlay = None
                outer = Gtk.Box(orientation=Gtk.Orientation.VERTICAL)
                outer.append(header)
                outer.append(content_vbox)
                self.win.set_child(outer)

            kc = Gtk.EventControllerKey()
            kc.connect("key-pressed", self._on_pc_key_pressed)
            kc.connect("key-released", self._on_pc_key_released)
            self.win.add_controller(kc)
            self.win.connect("notify::is-active", lambda w, _p: None if w.is_active() else self._release_all_keys())

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

        def _sync_window_size(self) -> None:
            lw, lh = compute_handset_geometry(self.active_model)
            self.handset_area.set_content_width(lw)
            self.handset_area.set_content_height(lh)
            expanded = self.pane_toggle.get_active()
            target_w = (lw + 32 + 456) if expanded else (lw + 32)
            target_h = max(lh + 76, 686 if expanded else lh + 76)
            self.win.set_default_size(target_w, target_h)

        def _on_dock_pane_toggled(self, btn: Any) -> None:
            self.right_pane.set_visible(btn.get_active())
            self._sync_window_size()

        # --- Compact GNOME HIG Radio & Connection Popover ---
        def _build_connection_popover(self) -> Any:
            pop = Gtk.Popover()
            box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=8, margin_top=10, margin_bottom=10, margin_start=10, margin_end=10)
            box.set_size_request(280, -1)

            # Compact Active Radio Section (visible when connected)
            self.conn_card = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=7, visible=False)
            top_line = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
            info_col = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=1, hexpand=True)
            self.conn_card_title = Gtk.Label(xalign=0, ellipsize=3)
            self.conn_card_title.add_css_class("heading")
            self.conn_card_sub = Gtk.Label(xalign=0, ellipsize=3)
            self.conn_card_sub.add_css_class("caption")
            self.conn_card_sub.add_css_class("dim-label")
            self.conn_card_id = Gtk.Label(xalign=0, ellipsize=3)
            self.conn_card_id.add_css_class("caption")
            self.conn_card_id.add_css_class("dim-label")
            info_col.append(self.conn_card_title)
            info_col.append(self.conn_card_sub)
            info_col.append(self.conn_card_id)
            top_line.append(info_col)
            disc_btn = Gtk.Button(icon_name="media-eject-symbolic", valign=Gtk.Align.CENTER)
            disc_btn.set_tooltip_text("Disconnect")
            disc_btn.add_css_class("flat")
            disc_btn.connect("clicked", lambda _b: (self._disconnect(), self.conn_popover.popdown()))
            top_line.append(disc_btn)
            self.conn_card.append(top_line)

            # Single-line Battery Meter
            batt_row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=6)
            self.batt_lbl = Gtk.Label(label="—", xalign=0)
            self.batt_lbl.add_css_class("caption")
            self.batt_lbl.add_css_class("numeric")
            self.batt_bar = Gtk.LevelBar(min_value=0.0, max_value=100.0, hexpand=True, valign=Gtk.Align.CENTER)
            self.batt_bar.add_offset_value("low", 20.0)
            self.batt_bar.add_offset_value("high", 50.0)
            self.batt_bar.add_offset_value("full", 85.0)
            self.batt_bar.set_size_request(-1, 6)
            batt_row.append(self.batt_lbl)
            batt_row.append(self.batt_bar)
            self.conn_card.append(batt_row)

            # Ultra-compact 4-action hardware pill bar (Torch · Lock · VFO · USB TX Safety)
            ctl_row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, homogeneous=True)
            ctl_row.add_css_class("linked")
            self.btn_torch = Gtk.ToggleButton(label="Torch")
            self.btn_torch.set_tooltip_text("Toggle top flashlight LED")
            self.btn_torch.connect("toggled", lambda b: None if self._updating_switches else (self.link.set_flashlight(b.get_active()), self.handset_area.queue_draw()))
            self.btn_lock = Gtk.ToggleButton(label="Lock")
            self.btn_lock.set_tooltip_text("Lock front keypad")
            self.btn_lock.connect("toggled", lambda b: None if self._updating_switches else self.link.set_keypad_lock(b.get_active()))
            btn_vfo = Gtk.Button(label="VFO")
            btn_vfo.set_tooltip_text("Swap primary VFO A/B")
            btn_vfo.connect("clicked", lambda _b: self.link.toggle_vfo())
            self.sw_allow_tx = Gtk.ToggleButton(label="TX")
            self.sw_allow_tx.set_tooltip_text("Allow TX over USB (OFF by default to protect PC from RF interference)")
            self.sw_allow_tx.connect("toggled", lambda b: setattr(self, "allow_usb_tx", b.get_active()))
            for w in (self.btn_torch, self.btn_lock, btn_vfo, self.sw_allow_tx):
                ctl_row.append(w)
            self.conn_card.append(ctl_row)

            self.conn_card.append(Gtk.Separator())
            box.append(self.conn_card)

            hdr = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=6)
            lbl = Gtk.Label(label="Radios", xalign=0, hexpand=True)
            lbl.add_css_class("caption-heading")
            lbl.add_css_class("dim-label")
            hdr.append(lbl)
            ref = Gtk.Button.new_from_icon_name("view-refresh-symbolic")
            ref.add_css_class("flat")
            ref.set_tooltip_text("Scan USB ports")
            ref.connect("clicked", lambda _b: (self.port_monitor.scan(), self._rebuild_connection_list()))
            hdr.append(ref)
            box.append(hdr)

            self.usb_list = Gtk.ListBox(selection_mode=Gtk.SelectionMode.NONE)
            self.usb_list.add_css_class("boxed-list")
            box.append(self.usb_list)

            # Compact footer row: QEMU · Demo · Auto-connect
            foot = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=6)
            vgrp = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL)
            vgrp.add_css_class("linked")
            for pid, title in (("tcp://127.0.0.1:4446", "QEMU"), ("demo", "Demo")):
                vb = Gtk.Button(label=title)
                vb.connect("clicked", lambda _b, p=pid: (self.conn_popover.popdown(), self._connect_to(p)))
                vgrp.append(vb)
            foot.append(vgrp)
            foot.append(Gtk.Box(hexpand=True))
            ac_lbl = Gtk.Label(label="Auto")
            ac_lbl.add_css_class("caption")
            ac_lbl.add_css_class("dim-label")
            ac_sw = Gtk.Switch(active=self.auto_connect_usb, valign=Gtk.Align.CENTER)
            ac_sw.set_tooltip_text("Auto-connect USB radio")
            ac_sw.connect("notify::active", lambda w, _p: setattr(self, "auto_connect_usb", w.get_active()))
            foot.append(ac_lbl)
            foot.append(ac_sw)
            box.append(foot)

            pop.set_child(box)
            return pop

        def _rebuild_connection_list(self) -> None:
            c = self.usb_list.get_first_child()
            while c is not None:
                n = c.get_next_sibling()
                self.usb_list.remove(c)
                c = n
            if not self.port_monitor.ports:
                if has_adw:
                    empty = Adw.ActionRow(title="No USB radio detected", subtitle="Plug in via USB-C or 2-pin cable")
                    empty.set_sensitive(False)
                    self.usb_list.append(empty)
                return
            for p in self.port_monitor.ports:
                if has_adw:
                    row = Adw.ActionRow(
                        title=GLib.markup_escape_text(p.pretty_name),
                        subtitle=GLib.markup_escape_text(p.device),
                        activatable=True,
                    )
                    if p.is_quansheng:
                        row.add_prefix(_make_radio_icon_widget(Gtk, Gdk, GLib, 16))
                    else:
                        row.add_prefix(Gtk.Image.new_from_icon_name("network-wired-symbolic"))
                    row.add_suffix(Gtk.Image.new_from_icon_name(
                        "object-select-symbolic" if (self.link.connected and self.link.port == p.device) else "go-next-symbolic"
                    ))
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

        # --- Primary Menu Popover (Display Optics & Shortcuts) ---
        def _build_primary_menu_popover(self) -> Any:
            pop = Gtk.Popover()
            box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=10, margin_top=12, margin_bottom=12, margin_start=14, margin_end=14)
            box.set_size_request(255, -1)

            lbl = Gtk.Label(label="Display Optics", xalign=0)
            lbl.add_css_class("heading")
            box.append(lbl)

            names = list(LCD_PALETTES.keys())
            self.pal_dd = Gtk.DropDown.new_from_strings(names)
            self.pal_dd.set_selected(names.index(self.lcd_st.palette))
            self.pal_dd.connect("notify::selected", self._on_palette_changed)
            box.append(self.pal_dd)

            for title, lo, hi, attr in (("Backlight", 0, 10, "backlight"), ("Contrast", 0, 15, "contrast")):
                r = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=2)
                r.append(Gtk.Label(label=title, xalign=0))
                sc = Gtk.Scale.new_with_range(Gtk.Orientation.HORIZONTAL, lo, hi, 1)
                sc.set_draw_value(True)
                sc.set_digits(0)
                sc.set_value(getattr(self.lcd_st, attr))
                sc.connect("value-changed", lambda s, a=attr: (setattr(self.lcd_st, a, int(s.get_value())), self.handset_area.queue_draw()))
                r.append(sc)
                box.append(r)

            for title, attr in (("STN Ghosting", "ghosting"), ("Pixel Grid", "pixel_grid"), ("Invert LCD", "invert_lcd")):
                r = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
                r.append(Gtk.Label(label=title, xalign=0, hexpand=True))
                sw = Gtk.Switch(active=getattr(self.lcd_st, attr))
                sw.connect("notify::active", lambda w, _p, a=attr: self._toggle_lcd_flag(a, w.get_active()))
                r.append(sw)
                box.append(r)

            box.append(Gtk.Separator())
            kb = Gtk.Label(label="Enter=MENU · Esc=EXIT · Arrows=Nav · Space=PTT\nCtrl+S PNG · Ctrl+R Record · Ctrl+B Dock", xalign=0)
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

        # --- Unified 60 FPS Animation Loop ---
        def _ensure_anim_loop(self) -> None:
            if self._anim_timer is None:
                self._anim_timer = GLib.timeout_add(16, self._tick_animations)

        def _tick_animations(self) -> bool:
            active = False
            fb, pressed, _led, _torch = self._current_display_state()
            if self.lcd_state.step(fb, self.lcd_st):
                active = True

            for kn in set(self._press_anim.keys()) | set(pressed):
                tgt = 1.0 if kn in pressed else 0.0
                cur = self._press_anim.get(kn, 0.0)
                nxt = cur + (tgt - cur) * (0.58 if tgt > cur else 0.30)
                if abs(tgt - nxt) < 0.015:
                    self._press_anim.pop(kn, None) if tgt == 0.0 else self._press_anim.__setitem__(kn, 1.0)
                else:
                    self._press_anim[kn] = nxt
                    active = True

            for kn in set(self._hover_anim.keys()) | ({self._hovered_key} if self._hovered_key else set()):
                tgt = 1.0 if kn == self._hovered_key else 0.0
                cur = self._hover_anim.get(kn, 0.0)
                nxt = cur + (tgt - cur) * 0.35
                if abs(tgt - nxt) < 0.015:
                    self._hover_anim.pop(kn, None) if tgt == 0.0 else self._hover_anim.__setitem__(kn, 1.0)
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
            if self._flashing_active or self.link.link_mode == "stock_bl":
                # Authentic Stock Bootloader hardware state: LCD is OFF (blank), top flashlight LED shows activity!
                return bytes(1024), frozenset(self._pressed_keys), ("tx" if self._flashing_active else "off"), self.link.torch_on
            if "ptt" in self._pressed_keys and self.link.connected and (self.allow_usb_tx or self.link.demo_mode):
                led = "tx"
            elif self.link.connected:
                led = "rx"
            else:
                led = "off"
            return self.link.last_fb, frozenset(self._pressed_keys), led, self.link.torch_on

        def _current_display_fb(self) -> bytes:
            return self._current_display_state()[0]

        def _draw_handset(self, _area: Any, cr: Any, w: int, h: int) -> None:
            lw, lh = compute_handset_geometry(self.active_model)
            scale = min(w / float(lw), h / float(lh))
            ox, oy = (w - lw * scale) * 0.5, (h - lh * scale) * 0.5
            cr.save()
            cr.translate(ox, oy)
            cr.scale(scale, scale)
            _fb, _pr, led, torch = self._current_display_state()
            eff_st = (
                dataclasses.replace(self.lcd_st, backlight=0)
                if (self._flashing_active or self.link.link_mode == "stock_bl")
                else self.lcd_st
            )
            self.handset_renderer.paint(
                cr, self.active_model, self.lcd_state, eff_st,
                press_anim=self._press_anim, hover_anim=self._hover_anim,
                led_state=led, torch_on=torch,
            )
            cr.restore()

        def _hit_test(self, x: float, y: float) -> str | None:
            lw, lh = compute_handset_geometry(self.active_model)
            w, h = self.handset_area.get_width(), self.handset_area.get_height()
            scale = min(w / float(lw), h / float(lh))
            lx = (x - (w - lw * scale) * 0.5) / scale
            ly = (y - (h - lh * scale) * 0.5) / scale
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
                _fb, pr, led, torch = self._current_display_state()
                self.recorder.push(self.link.last_fb, pr, led, torch)
                self._update_rec_label()

        def _release_all_keys(self) -> None:
            self._pointer_active_key = None
            for kn in list(self._pressed_keys):
                self._release_key(kn)

        def _press_key(self, kname: str) -> None:
            if kname in self._pressed_keys:
                return
            # Hard RF RFI protection: block live PTT over USB unless explicitly armed!
            if kname == "ptt" and self.link.connected and not self.link.demo_mode and not self.allow_usb_tx:
                self._notify("TX locked over USB (RF safety) — arm 'Allow TX over USB' in Radio menu")
                return
            self._pressed_keys.add(kname)
            self._capture_rec_event()
            self._ensure_anim_loop()
            if not self.link.connected or self.link.link_mode != "kern_os":
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
            if was and self.link.connected and self.link.link_mode == "kern_os":
                self.link.send_key(kname, "release")

        def _long_click_key(self, kname: str) -> None:
            if not self.link.connected or self.link.link_mode != "kern_os":
                return
            if kname == "ptt" and not self.link.demo_mode and not self.allow_usb_tx:
                self._notify("TX locked over USB (RF safety) — arm 'Allow TX over USB' in Radio menu")
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

        # --- PC Keyboard Shortcuts ---
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
            return foc is not None and isinstance(foc, (Gtk.Entry, Gtk.SearchEntry, Gtk.Text))

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

        # --- Dock Pages (Files · Capture · Console) ---
        def _build_files_page(self) -> Any:
            box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=8, margin_top=8, margin_bottom=10, margin_start=12, margin_end=12)

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
            ref.set_tooltip_text("Refresh directory")
            ref.connect("clicked", lambda _b: self._browse_dir(self.current_dir))
            bar.append(ref)

            up = Gtk.Button.new_from_icon_name("document-send-symbolic")
            up.set_tooltip_text("Upload file to current folder")
            up.connect("clicked", self._on_upload_file)
            bar.append(up)

            # Maintenance & Firmware Popover (replaces the cluttered Flash tab & manual Probe button!)
            maint_btn = Gtk.MenuButton(icon_name="view-more-symbolic")
            maint_btn.set_tooltip_text("Firmware flasher, KRX sideload & .kfw backups")
            maint_btn.set_popover(self._build_maintenance_popover())
            bar.append(maint_btn)
            box.append(bar)

            mbar = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
            self.file_search = Gtk.SearchEntry(placeholder_text="Filter files…", hexpand=True)
            self.file_search.connect("search-changed", lambda _e: self._render_filtered_files())
            mbar.append(self.file_search)
            self.mount_btn = Gtk.ToggleButton(label="Mount Drive")
            self.mount_btn.connect("toggled", self._on_mount_toggled)
            mbar.append(self.mount_btn)
            self.open_fs_btn = Gtk.Button(icon_name="folder-open-symbolic", visible=False)
            self.open_fs_btn.connect("clicked", self._on_open_fs)
            mbar.append(self.open_fs_btn)
            box.append(mbar)

            self.path_lbl = Gtk.Label(label="/", xalign=0)
            self.path_lbl.add_css_class("caption-heading")
            self.path_lbl.add_css_class("dim-label")
            box.append(self.path_lbl)

            scroll = Gtk.ScrolledWindow(vexpand=True)
            self.file_list = Gtk.ListBox(selection_mode=Gtk.SelectionMode.NONE)
            self.file_list.add_css_class("boxed-list")
            scroll.set_child(self.file_list)
            box.append(scroll)
            return box

        def _build_maintenance_popover(self) -> Any:
            pop = Gtk.Popover()
            box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=6, margin_top=10, margin_bottom=10, margin_start=10, margin_end=10)
            box.set_size_request(290, -1)

            def _add_list_section(title: str, items: list[tuple[str, str, str, Callable[[], None]]]) -> None:
                lbl = Gtk.Label(label=title, xalign=0)
                lbl.add_css_class("caption-heading")
                lbl.add_css_class("dim-label")
                box.append(lbl)
                lb = Gtk.ListBox(selection_mode=Gtk.SelectionMode.NONE)
                lb.add_css_class("boxed-list")
                for icon, t, sub, fn in items:
                    if has_adw:
                        row = Adw.ActionRow(title=t, subtitle=sub, activatable=True)
                        row.add_prefix(Gtk.Image.new_from_icon_name(icon))
                        row.connect("activated", lambda _r, f=fn: (pop.popdown(), f()))
                        lb.append(row)
                box.append(lb)

            _add_list_section("Firmware &amp; Packages", [
                ("software-update-available-symbolic", "Flash Firmware…", "Write verified .kfw or .bin to radio", lambda: self._on_flash_firmware_dialog(None)),
                ("application-x-executable-symbolic", "Install App / Module…", "Sideload .krx / .mod to /apps", lambda: self._on_sideload(None)),
            ])
            _add_list_section("Ascon-Verified .kfw Backups", [
                ("security-high-symbolic", "Backup Calibration…", "Save /sys/calib.dat &amp; /sys/calib.sig", lambda: self._on_run_backup(calib_only=True)),
                ("drive-harddisk-symbolic", "Full KernFS Snapshot…", "Archive /sys, /pref, /user &amp; /apps", lambda: self._on_run_backup(calib_only=False)),
                ("document-revert-symbolic", "Restore User Backup…", "Restore /user, /pref &amp; /apps (/sys shielded)", lambda: self._on_restore_backup(None)),
            ])
            pop.set_child(box)
            return pop

        def _build_wire_page(self) -> Any:
            box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=8, margin_top=10, margin_bottom=10, margin_start=12, margin_end=12)
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
            box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=12, margin_top=12, margin_bottom=12, margin_start=14, margin_end=14)
            top_row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
            self.rec_btn = Gtk.ToggleButton()
            rec_in = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=6)
            rec_in.append(Gtk.Image.new_from_icon_name("media-record-symbolic"))
            self.rec_lbl = Gtk.Label(label="Record GIF")
            rec_in.append(self.rec_lbl)
            self.rec_btn.set_child(rec_in)
            self.rec_btn.connect("toggled", self._on_record_toggled)
            top_row.append(self.rec_btn)

            shot_btn = Gtk.Button()
            sh_in = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=6)
            sh_in.append(Gtk.Image.new_from_icon_name("camera-photo-symbolic"))
            sh_in.append(Gtk.Label(label="Snapshot PNG"))
            shot_btn.set_child(sh_in)
            shot_btn.connect("clicked", self._on_save_screenshot)
            top_row.append(shot_btn)
            box.append(top_row)

            self.gif_summary_lbl = Gtk.Label(label="Press Record GIF to capture radio interactions.", xalign=0, wrap=True)
            self.gif_summary_lbl.add_css_class("dim-label")
            box.append(self.gif_summary_lbl)

            self.timeline_area = Gtk.DrawingArea(content_height=48, hexpand=True)
            self.timeline_area.set_draw_func(self._draw_timeline)
            tl_c = Gtk.GestureClick()
            tl_c.connect("pressed", self._on_timeline_click)
            self.timeline_area.add_controller(tl_c)
            tl_d = Gtk.GestureDrag()
            tl_d.connect("drag-update", self._on_timeline_drag)
            self.timeline_area.add_controller(tl_d)
            box.append(self.timeline_area)

            tr = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=6)
            self.pb_btn = Gtk.Button.new_from_icon_name("media-playback-start-symbolic")
            self.pb_btn.connect("clicked", self._on_pb_toggle)
            tr.append(self.pb_btn)
            for icon, d in (("go-previous-symbolic", -1), ("go-next-symbolic", 1)):
                b = Gtk.Button.new_from_icon_name(icon)
                b.connect("clicked", lambda _b, delta=d: self._step_pb(delta))
                tr.append(b)
            tr.append(Gtk.Separator(orientation=Gtk.Orientation.VERTICAL))
            b_in = Gtk.Button(label="[ In")
            b_in.connect("clicked", lambda _b: self._set_trim_in())
            tr.append(b_in)
            b_out = Gtk.Button(label="Out ]")
            b_out.connect("clicked", lambda _b: self._set_trim_out())
            tr.append(b_out)
            self.cut_btn = Gtk.Button(label="Mark Cut")
            self.cut_btn.connect("clicked", self._on_cut_splice)
            tr.append(self.cut_btn)
            undo_b = Gtk.Button.new_from_icon_name("edit-undo-symbolic")
            undo_b.connect("clicked", lambda _b: (self.recorder.undo_cut(), self._refresh_gif_ui()))
            tr.append(undo_b)
            box.append(tr)

            if has_adw:
                opt = Adw.PreferencesGroup(title="Export")
                r_mode = Adw.ActionRow(title="Capture Scope", subtitle="LCD glass only or full 3D radio with key presses")
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

        # --- Automatic Flasher (Real Bootloader + Authentic Flashlight Page Strobe) & Backup Handlers ---
        def _on_flash_page_pulse(self, page: int, _total: int) -> bool:
            """Toggle top flashlight LED per flashed 256 B page exactly like real Quansheng bootloaders."""
            self.link.torch_on = (page % 2 == 1)
            self.handset_area.queue_draw()
            return False

        def _on_flash_firmware_dialog(self, _btn: Any) -> None:
            dlg = Gtk.FileDialog(title="Select Firmware Image (.kfw or .bin)")

            def _picked(d: Any, res: Any) -> None:
                try:
                    fpath = d.open_finish(res).get_path()
                except GLib.Error:
                    return
                if not fpath:
                    return
                try:
                    info = inspect_firmware_file(fpath)
                except Exception as e:
                    self._notify(f"Invalid firmware: {e}")
                    return
                self._start_firmware_flash(info)

            dlg.open(self.win, None, _picked)

        def _start_firmware_flash(self, info: dict[str, Any]) -> None:
            if not self.link.connected:
                self._notify("Connect a radio in Bootloader mode (PTT + Power On) first")
                return
            fw_bytes = info["fw_bytes"]
            total_pages = max(1, math.ceil(len(fw_bytes) / 256.0))
            is_demo = self.link.demo_mode
            if not is_demo and self.link.link_mode != "stock_bl":
                self._notify("Put radio into Bootloader mode (PTT + Power On) to flash")
                return

            port = self.link.port or ""
            self._flashing_active = True
            self.link.torch_on = True
            self.lcd_state.step(bytes(1024), dataclasses.replace(self.lcd_st, ghosting=False, backlight=0))
            self.op_prog.set_visible(True)
            self.op_prog.set_fraction(0.0)
            self.op_prog.set_text(f"Flashing {info['version']} (0/{total_pages} pages)…")
            self.handset_area.queue_draw()

            def _prog(f: float, msg: str) -> None:
                GLib.idle_add(self.op_prog.set_fraction, f)
                GLib.idle_add(self.op_prog.set_text, msg)

            def _page(p: int, tot: int) -> None:
                GLib.idle_add(self._on_flash_page_pulse, p, tot)

            def _w() -> None:
                try:
                    if is_demo:
                        # Authentic bootloader simulation: unlit blank LCD + flashlight strobing per page!
                        t0 = time.monotonic()
                        sim_pages = min(total_pages, 64)
                        for pg in range(1, sim_pages + 1):
                            _page(pg, sim_pages)
                            _prog(pg / float(sim_pages), f"Demo flash · Page {pg}/{sim_pages}")
                            time.sleep(0.032)
                        GLib.idle_add(self._notify, f"Simulated {sim_pages} pages in {time.monotonic() - t0:.1f}s")
                    else:
                        res = flash_via_stock_bootloader(port, fw_bytes, progress_cb=_prog, page_cb=_page)
                        GLib.idle_add(self._notify, f"Flashed {res['pages']} pages in {res['elapsed_s']:.1f}s")
                except Exception as e:
                    GLib.idle_add(self._notify, f"Flash failed: {e}")
                finally:
                    def _finish() -> bool:
                        self._flashing_active = False
                        self.link.torch_on = False
                        self.op_prog.set_visible(False)
                        self.lcd_state.step(self._current_display_fb(), self.lcd_st)
                        self.handset_area.queue_draw()
                        return False
                    GLib.idle_add(_finish)

            threading.Thread(target=_w, daemon=True).start()

        def _on_run_backup(self, *, calib_only: bool) -> None:
            if not self.link.connected or self.link.fs_backend is None:
                self._notify("Connect to a radio running Kern OS first")
                return
            tag = "calib" if calib_only else "kernfs"
            slug = self.link.volume_name.lower().replace(" ", "_")
            dlg = Gtk.FileDialog(title="Save Verified .kfw Backup")
            dlg.set_initial_name(f"{slug}_{tag}_{time.strftime('%Y%m%d_%H%M%S')}.kfw")
            be = self.link.fs_backend
            vname = self.link.volume_name

            def _picked(d: Any, res: Any) -> None:
                try:
                    dest = d.save_finish(res).get_path()
                except GLib.Error:
                    return
                if not dest:
                    return
                self.op_prog.set_visible(True)
                self.op_prog.set_fraction(0.0)

                def _prog(f: float, msg: str) -> None:
                    GLib.idle_add(self.op_prog.set_fraction, f)
                    GLib.idle_add(self.op_prog.set_text, msg)

                def _w() -> None:
                    try:
                        blob = build_kfw_backup_from_backend(be, volume_name=vname, calib_only=calib_only, progress_cb=_prog)
                        Path(dest).write_bytes(blob)
                        GLib.idle_add(self._notify, f"Saved {Path(dest).name} ({len(blob):,} B)")
                    except Exception as e:
                        GLib.idle_add(self._notify, f"Backup failed: {e}")
                    finally:
                        GLib.idle_add(self.op_prog.set_visible, False)

                threading.Thread(target=_w, daemon=True).start()

            dlg.save(self.win, None, _picked)

        def _on_restore_backup(self, _btn: Any) -> None:
            if not self.link.connected or self.link.fs_backend is None:
                self._notify("Connect to a radio running Kern OS first")
                return
            dlg = Gtk.FileDialog(title="Select .kfw Backup to Restore")
            be = self.link.fs_backend

            def _picked(d: Any, res: Any) -> None:
                try:
                    src = d.open_finish(res).get_path()
                except GLib.Error:
                    return
                if not src:
                    return
                self.op_prog.set_visible(True)
                self.op_prog.set_fraction(0.0)

                def _prog(f: float, msg: str) -> None:
                    GLib.idle_add(self.op_prog.set_fraction, f)
                    GLib.idle_add(self.op_prog.set_text, msg)

                def _w() -> None:
                    try:
                        rest, skip = restore_kfw_backup_to_backend(src, be, self.link.policy, progress_cb=_prog)
                        GLib.idle_add(self._notify, f"Restored {rest} file(s) ({skip} /sys shielded)")
                        GLib.idle_add(self._browse_dir, self.current_dir)
                    except Exception as e:
                        GLib.idle_add(self._notify, f"Restore failed: {e}")
                    finally:
                        GLib.idle_add(self.op_prog.set_visible, False)

                threading.Thread(target=_w, daemon=True).start()

            dlg.open(self.win, None, _picked)

        # --- Recording & Timeline Handlers ---
        def _update_rec_label(self) -> None:
            if self.recorder.recording:
                dt = int(time.monotonic() - self.recorder.t0_mono)
                self.rec_lbl.set_text(f"Stop ({dt // 60:02d}:{dt % 60:02d} · {len(self.recorder.frames)}f)")
            else:
                self.rec_lbl.set_text("Record GIF")

        def _on_record_toggled(self, btn: Any) -> None:
            if btn.get_active():
                self._stop_playback()
                _fb, pr, led, torch = self._current_display_state()
                self.recorder.start(self.link.last_fb, pr, led, torch)
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
                cr.set_source_rgba(0.92, 0.24, 0.24, 0.65)
                cr.rectangle(fx(a), 8, max(3.0, fx(b) - fx(a)), h - 16)
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
            if n > 1:
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
                self.gif_summary_lbl.set_text("Press Record GIF to capture radio interactions.")
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

        # --- Connect / Disconnect / Telemetry / Searchable Files / Wire ---
        def _connect_to(self, port: str, port_info: DetectedPort | None = None) -> None:
            try:
                self.link.connect(port, baud=baud, model_override=self.forced_model, port_info=port_info)
            except Exception as e:
                self._notify(f"Connection failed: {e}")
                return

            # Always re-arm USB TX safety lock on every new radio connection!
            self.allow_usb_tx = False
            if hasattr(self, "sw_allow_tx"):
                self.sw_allow_tx.set_active(False)

            is_bl = self.link.link_mode == "stock_bl"
            self.hdr_batt_icon.set_visible(not is_bl)
            self.hdr_dfu_dot.set_visible(is_bl)
            short_name = self.link.volume_name.removeprefix("Quansheng ")
            self.hdr_dev_lbl.set_text(short_name)
            self.conn_card.set_visible(True)
            self.conn_card_title.set_text(self.link.volume_name)
            self.conn_card_sub.set_text(f"Port: {self.link.port}")
            self.win.set_title(f"Kern Dock — {self.link.volume_name}")
            if self.win_title is not None:
                self.win_title.set_title("Kern Dock")
                self.win_title.set_subtitle("Bootloader ready" if is_bl else "Connected")

            if self.dfu_banner is not None:
                if is_bl:
                    ver = self.link.bootloader_info.get("version", "Stock BL")
                    self.dfu_banner.set_title(f"Stock Bootloader ({ver}) detected on {self.link.port}")
                    self.dfu_banner.set_revealed(True)
                else:
                    self.dfu_banner.set_revealed(False)

            if self.link.detected_model != self.active_model:
                self.active_model = self.link.detected_model
                if not self.user_picked_palette:
                    self.lcd_st.palette = "UV-K5" if self.active_model == "k5" else "UV-K1"
                    self.pal_dd.set_selected(list(LCD_PALETTES.keys()).index(self.lcd_st.palette))
                self._sync_window_size()

            self._rebuild_connection_list()
            if is_bl:
                self.lcd_state.step(bytes(1024), dataclasses.replace(self.lcd_st, ghosting=False, backlight=0))
            else:
                self.lcd_state.step(self._current_display_fb(), self.lcd_st)
            self.handset_area.queue_draw()
            self._refresh_telemetry_ui()
            if self.link.link_mode == "kern_os":
                self._browse_dir("/")

        def _disconnect(self) -> None:
            self._release_all_keys()
            self.link.disconnect()
            self.allow_usb_tx = False
            if hasattr(self, "sw_allow_tx"):
                self.sw_allow_tx.set_active(False)
            self.hdr_batt_icon.set_visible(False)
            self.hdr_dfu_dot.set_visible(False)
            self.hdr_dev_lbl.set_text("Connect")
            self.conn_card.set_visible(False)
            if self.dfu_banner is not None:
                self.dfu_banner.set_revealed(False)
            self.mount_btn.set_active(False)
            self.open_fs_btn.set_visible(False)
            self.win.set_title("Kern Dock")
            if self.win_title is not None:
                self.win_title.set_title("Kern Dock")
                self.win_title.set_subtitle("Not connected")
            self._rebuild_connection_list()
            self._cached_dir_entries = []
            self._render_filtered_files()
            self._refresh_telemetry_ui()
            self.handset_area.queue_draw()

        def _refresh_telemetry_ui(self) -> bool:
            if not self.link.connected:
                self.batt_lbl.set_text("Battery —")
                self.batt_bar.set_value(0.0)
                return False

            if self.link.link_mode != "kern_os":
                ver = self.link.bootloader_info.get("version", "Stock BL")
                self.conn_card_sub.set_text(f"{self.link.port} · Bootloader {ver}")
                self.conn_card_id.set_text("LCD off · Flashlight indicates bootloader activity")
                if self.win_title is not None:
                    self.win_title.set_subtitle(f"Bootloader {ver} · {self.link.port}")
                return False

            t = self.link.last_telemetry
            mv, pct = t.get("battery_mv"), t.get("battery_pct")
            if mv is not None:
                self.batt_lbl.set_text(f"{mv / 1000.0:.2f} V · {pct}%")
                self.batt_bar.set_value(float(max(0, min(100, pct or 0))))
                self.hdr_batt_icon.set_from_icon_name(_battery_icon_name(pct))
                self.hdr_batt_icon.set_tooltip_text(f"Battery: {pct}% ({mv / 1000.0:.2f} V)")

            fps_s = "Demo" if self.link.demo_mode else f"{self.link.fps:.0f} FPS"
            err_s = f" · {self.link.error_count} err" if self.link.error_count else ""
            self.conn_card_sub.set_text(f"{self.link.port} · {fps_s} · {self.link.rtt_ms:.1f} ms{err_s}")
            self.conn_card_id.set_text(f"{t.get('serial_str', '—')}  ·  {t.get('mac_addr', '—')}")
            if self.win_title is not None:
                self.win_title.set_subtitle(f"{fps_s} · {self.link.rtt_ms:.1f} ms")

            self._updating_switches = True
            self.btn_torch.set_active(self.link.torch_on)
            self.btn_lock.set_active(self.link.keypad_locked)
            self._updating_switches = False
            return False

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
                GLib.idle_add(self._set_dir_entries, entries)

            threading.Thread(target=_w, daemon=True).start()

        def _set_dir_entries(self, entries: list[dict[str, Any]]) -> bool:
            self._cached_dir_entries = entries
            self._render_filtered_files()
            return False

        def _render_filtered_files(self) -> None:
            c = self.file_list.get_first_child()
            while c is not None:
                n = c.get_next_sibling()
                self.file_list.remove(c)
                c = n
            q = self.file_search.get_text().strip().lower() if hasattr(self, "file_search") else ""
            shown = [e for e in self._cached_dir_entries if (not q or q in e["name"].lower())]
            if not shown:
                if has_adw:
                    msg = "Connect a Kern OS radio to browse files" if not self.link.connected else ("No matching files" if q else "Empty directory")
                    st = Adw.StatusPage(icon_name="folder-symbolic", title=msg)
                    st.add_css_class("compact")
                    self.file_list.append(st)
                return
            dir_path = self.current_dir
            for e in shown:
                name, is_dir, size = e["name"], bool(e["is_dir"]), int(e["size"])
                full = f"/{name}" if dir_path == "/" else f"{dir_path.rstrip('/')}/{name}"
                protected = full.startswith("/sys/") or full in kernfs_mount.PROTECTED_RECOVERY_MODS
                if has_adw:
                    row = Adw.ActionRow(
                        title=GLib.markup_escape_text(name),
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
                self._notify("Connect to a Kern OS radio first")
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
            if not self.link.connected or self.link.fs_backend is None:
                btn.set_active(False)
                self._notify("Connect to a Kern OS radio first")
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
                        fb, pr, led, torch = self._current_display_state()
                        if self._export_mode == "body":
                            w, h, rgba = render_handset_to_rgba(self.active_model, fb, self.lcd_st, pressed_keys=pr, led_state=led, torch_on=torch, scale=2.0)
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
# 7. HEADLESS SELFTEST & BENCHMARK (CI-Safe, Zero Display Required)
# =============================================================================

def run_selftest() -> int:
    import tempfile

    k1_keys = [k[0] for row in K1_MATRIX for k in row]
    k5_keys = ["menu", "up", "down", "exit"] + [k[0] for row in K5_BOTTOM_MATRIX for k in row]
    assert len(k1_keys) == 16 and len(k5_keys) == 16
    for kn in set(k1_keys + k5_keys):
        assert kn in rigwire.KEY_CODES, f"Invalid RigWire key '{kn}'"

    # Verify USB CDC (36b7:ffff = Kern OS!) & Stock BL (36b7:0001) & model auto-classification
    assert detect_hardware_model("Quansheng UV-K1 1D9E") == "k1"
    assert detect_hardware_model("Quansheng UV-K5 v3 ABCD") == "k5"
    assert classify_usb_mode("36b7", "0001", "QUANSHENG K1 CPS") == ("stock_bl", True)
    assert classify_usb_mode("36b7", "ffff", "Quansheng UV-K1 1D9E") == ("kern_os", True)
    assert classify_usb_mode("1a86", "7523", "Quansheng UV-K1 1D9E") == ("kern_os", True)

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

    # Verify .kfw calibration & full KernFS backup + safe restore (shields /sys/calib.dat!)
    link = DockLink()
    link.connect("demo")
    assert link.connected and link.fs_backend is not None
    with tempfile.TemporaryDirectory() as td:
        bk_cal = Path(td) / "calib.kfw"
        bk_full = Path(td) / "full.kfw"
        bk_cal.write_bytes(build_kfw_backup_from_backend(link.fs_backend, volume_name=link.volume_name, calib_only=True))
        bk_full.write_bytes(build_kfw_backup_from_backend(link.fs_backend, volume_name=link.volume_name, calib_only=False))
        parsed_cal = kfw.parse_kfw(bk_cal.read_bytes(), verify_root=True)
        assert parsed_cal.kind == kfw.KIND_BACKUP
        rest, skip = restore_kfw_backup_to_backend(bk_full, link.fs_backend, link.policy)
        assert rest >= 3 and skip >= 2  # /sys/calib.dat & /sys/calib.sig strictly shielded!

    bench_str = ""
    try:
        import cairo
        has_cairo = True
    except ImportError:
        has_cairo = False

    if has_cairo:
        rend = HandsetRenderer()
        lw, lh = compute_handset_geometry("k1")
        surf = cairo.ImageSurface(cairo.FORMAT_ARGB32, lw, lh)
        cr = cairo.Context(surf)
        rend.paint(cr, "k1", fs, st_k1, press_anim={"up": 0.5}, hover_anim={}, led_state="rx")
        t0 = time.monotonic()
        for _ in range(40):
            fs.step(fb2, st_k1)
            rend.paint(cr, "k1", fs, st_k1, press_anim={"up": 0.5}, hover_anim={}, led_state="rx")
        dt_ms = (time.monotonic() - t0) * 1000.0 / 40.0
        bench_str = f", {dt_ms:.2f} ms/frame (~{1000.0/max(dt_ms, 0.01):.0f} FPS cap)"

        w, h, rgba = render_lcd_to_rgba(fb1, st_k1, width=440, height=304)
        png = encode_png_rgba(w, h, rgba)
        assert png.startswith(b"\x89PNG\r\n\x1a\n") and png.endswith(b"IEND\xaeB`\x82")
        for mdl in ("k1", "k5"):
            bw, bh, brgba = render_handset_to_rgba(mdl, fb2, st_k1, pressed_keys=frozenset({"up", "ptt"}), led_state="dfu")
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

    assert "PONG" in link.run_wire_command("ping")
    assert "dfu.mod" in link.run_wire_command("ls /apps")
    url = link.start_dav(http_port=0, read_only=True)
    conn = http.client.HTTPConnection("127.0.0.1", link.dav_port, timeout=5)
    try:
        conn.request("PROPFIND", "/", headers={"Depth": "1"})
        resp = conn.getresponse()
        assert resp.status == 207 and b"Quansheng UV-K1 DEMO" in resp.read()
    finally:
        conn.close()
    link.disconnect()

    kernfs_mount.log_ok(
        f"All Kern Dock selftests passed (3D UV-K1/UV-K5 models, fast LCD{bench_str}, DFU/CPS detect, .kfw backups/restore, GIF splice, WebDAV)."
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
