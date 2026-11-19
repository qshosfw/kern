#!/usr/bin/env python3
"""
Kern Dock — Native GTK4 + Libadwaita Hardware Companion for Kern OS.

Features:
  • Hardware-accurate Cairo handset models derived from official Quansheng
    factory orthographic views (auto-detected via GUdev USB descriptors &
    RigWire HELLO — no manual model switcher):
      - Quansheng UV-K1(8): warm gunmetal metallic faceplate, milled 4x4 key
        lattice, corner Torx bosses, stubby A1 antenna, cobalt-blue rotary
        knob stripe & PTT accent
      - Quansheng UV-K5 v3 / UV-K6: brushed titanium upper LCD armor, orange
        rotary knob cap, vertical left nav column (M / ▲ / ▼ / EXIT) alongside
        the 3-bar speaker grille, and lower 4x3 compact keypad
  • Realistic ST7565 128x64 LCD engine (taller-than-wide 1.3x pixels,
    unpixelated backlit glass margin, edge-lit diffuser gradient, asymmetric
    STN attack/decay ghosting, contrast & backlight curves, rear-polarizer
    shadow, and authentic UV-K1/UV-K5 palettes)
  • Low-latency RigWire link (GUdev hotplug detection, chunk-interleaved
    priority key dispatch, zero-sleep frame streaming, instant GLib redraws)
  • Built-in GIF Recorder & Non-Destructive Timeline Editor (trim In/Out,
    cut/splice middle segments, speed & idle-cap, export LCD-only OR Full
    Radio Body with animated physical button presses)
  • Full GTK4 Dock Workspace: Device telemetry & controls, live KernFS File
    Browser + WebDAV OS mount, interactive RigWire Console, and GIF Editor
"""

from __future__ import annotations

import argparse
import dataclasses
import http.client
import io
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
# 1. REALISTIC ST7565 LCD ENGINE (Squelch-Inspired Physics + Cairo Precision)
# =============================================================================

LCD_W = 128
LCD_H = 64

# Authentic hardware palettes (from real UV-K1 / UV-K5 measurements & squelch)
LCD_PALETTES: dict[str, dict[str, Any]] = {
    "UV-K1":         {"bg": (0xC9, 0xF7, 0xEC), "fg": (0x26, 0x20, 0x83), "dark": False},
    "UV-K5":         {"bg": (0xFF, 0x99, 0x00), "fg": (0x0E, 0x0E, 0x0E), "dark": False},
    "Classic Green": {"bg": (0x9C, 0xA9, 0x88), "fg": (0x2A, 0x2A, 0x2A), "dark": False},
    "UV-5R Blue":    {"bg": (0x39, 0xAE, 0xF1), "fg": (0x10, 0x10, 0x10), "dark": False},
    "OLED White":    {"bg": (0x10, 0x10, 0x10), "fg": (0xF0, 0xF0, 0xF0), "dark": True},
}

# Asymmetric STN liquid-crystal response (fast turn-on, smooth turn-off persistence)
STN_ATTACK = 0.72
STN_DECAY = 0.22


@dataclasses.dataclass
class LcdSettings:
    palette: str = "UV-K1"
    pixel_aspect: float = 1.30
    pixel_grid: bool = True
    invert_lcd: bool = False
    ghosting: bool = True
    contrast: int = 13         # 0..15
    backlight: int = 8         # 0..10 (0-6 dim, 7-8 normal, 9-10 vibrant)
    backlight_glow: bool = True


def _backlight_multiplier(level: int) -> float:
    if level <= 6:
        return 0.15 + (level / 6.0) * 0.75
    if level <= 8:
        return 1.0
    return 1.0 + (level - 8) * 0.14


def _clamp1(v: float) -> float:
    return 0.0 if v < 0.0 else (1.0 if v > 1.0 else v)


def _lcd_colors_norm(st: LcdSettings) -> tuple[tuple[float, float, float], tuple[float, float, float], tuple[float, float, float], bool]:
    pal = LCD_PALETTES.get(st.palette, LCD_PALETTES["UV-K1"])
    bright = _backlight_multiplier(st.backlight)
    bg = tuple(_clamp1((c / 255.0) * bright) for c in pal["bg"])
    fg_raw = tuple(_clamp1((c / 255.0) * bright) for c in pal["fg"])
    c_ratio = 0.15 + (max(0, min(15, st.contrast)) / 15.0) * 0.85
    fg = (
        bg[0] + (fg_raw[0] - bg[0]) * c_ratio,
        bg[1] + (fg_raw[1] - bg[1]) * c_ratio,
        bg[2] + (fg_raw[2] - bg[2]) * c_ratio,
    )
    grid = (
        bg[0] + (fg_raw[0] - bg[0]) * 0.075,
        bg[1] + (fg_raw[1] - bg[1]) * 0.075,
        bg[2] + (fg_raw[2] - bg[2]) * 0.075,
    )
    return bg, fg, grid, bool(pal["dark"])


def step_ghost_buffer(ghost: list[float], fb: bytes, st: LcdSettings) -> bool:
    """Advances STN ghosting persistence by one tick; returns True if pixels are still settling."""
    if len(fb) < 1024:
        fb = fb.ljust(1024, b"\x00")
    inv = st.invert_lcd
    use_ghost = st.ghosting
    settling = False
    idx = 0
    for y in range(LCD_H):
        page_off = (y >> 3) * 128
        mask = 1 << (y & 7)
        for x in range(LCD_W):
            bit = 1 if (fb[page_off + x] & mask) else 0
            target = (0.0 if bit else 1.0) if inv else (1.0 if bit else 0.0)
            if not use_ghost:
                ghost[idx] = target
            else:
                cur = ghost[idx]
                diff = target - cur
                if abs(diff) > 0.015:
                    factor = STN_ATTACK if diff > 0.0 else STN_DECAY
                    nxt = cur + diff * factor
                    if abs(target - nxt) <= 0.015:
                        ghost[idx] = target
                    else:
                        ghost[idx] = nxt
                        settling = True
                else:
                    ghost[idx] = target
            idx += 1
    return settling


def draw_st7565_cairo(
    cr: Any,
    x: float,
    y: float,
    w: float,
    h: float,
    fb: bytes,
    st: LcdSettings,
    ghost: list[float] | None = None,
) -> None:
    """
    Draws an ultra-realistic ST7565 128x64 glass LCD inside (x, y, w, h):
      • Unpixelated illuminated glass margin on all 4 sides (~6% padding)
      • Subtle edge-lit LED diffuser gradient & recessed bezel shadow
      • Active 128x64 dot matrix with taller-than-wide pixels (1.3x)
      • Crisp 10% floating-point subpixel gap (never harsh 33% checkerboard)
      • Subtle STN rear-polarizer pixel drop-shadow on reflective panels
    """
    import cairo

    bg, fg, grid, is_dark = _lcd_colors_norm(st)
    if len(fb) < 1024:
        fb = fb.ljust(1024, b"\x00")

    cr.save()
    # Rounded glass clip
    r = 4.0
    cr.new_sub_path()
    cr.arc(x + w - r, y + r, r, -math.pi / 2, 0)
    cr.arc(x + w - r, y + h - r, r, 0, math.pi / 2)
    cr.arc(x + r, y + h - r, r, math.pi / 2, math.pi)
    cr.arc(x + r, y + r, r, math.pi, 3 * math.pi / 2)
    cr.close_path()
    cr.clip()

    # 1. Unpixelated illuminated backlit glass fill + subtle edge-lit diffuser gradient
    pat = cairo.LinearGradient(x, y, x, y + h)
    if is_dark:
        pat.add_color_stop_rgb(0.0, bg[0], bg[1], bg[2])
        pat.add_color_stop_rgb(1.0, _clamp1(bg[0] * 1.06), _clamp1(bg[1] * 1.06), _clamp1(bg[2] * 1.06))
    else:
        # Subtle bottom LED diffuser boost (real ST7565 modules are bottom/side lit)
        pat.add_color_stop_rgb(0.0, bg[0] * 0.94, bg[1] * 0.95, bg[2] * 0.96)
        pat.add_color_stop_rgb(0.55, bg[0], bg[1], bg[2])
        pat.add_color_stop_rgb(1.0, _clamp1(bg[0] * 1.04), _clamp1(bg[1] * 1.04), _clamp1(bg[2] * 1.03))
    cr.set_source(pat)
    cr.rectangle(x, y, w, h)
    cr.fill()

    # 2. Compute active matrix rectangle centered inside unpixelated backlit glass margin
    pad_x = max(5.0, w * 0.048)
    pad_y = max(5.0, h * 0.068)
    avail_w = w - pad_x * 2.0
    avail_h = h - pad_y * 2.0

    # Fit 128 x (64 * aspect) strictly inside available area
    px_w = min(avail_w / LCD_W, avail_h / (LCD_H * st.pixel_aspect))
    px_h = px_w * st.pixel_aspect
    mat_w = LCD_W * px_w
    mat_h = LCD_H * px_h
    mx = x + (w - mat_w) * 0.5
    my = y + (h - mat_h) * 0.5

    # Passive matrix zone subtle tint (only inside active 128x64 region — padding stays pure glass!)
    if st.pixel_grid and not is_dark:
        cr.set_source_rgb(grid[0], grid[1], grid[2])
        cr.rectangle(mx, my, mat_w, mat_h)
        cr.fill()

    gap_x = (px_w * 0.11) if st.pixel_grid else 0.0
    gap_y = (px_h * 0.11) if st.pixel_grid else 0.0
    dw = max(0.5, px_w - gap_x)
    dh = max(0.5, px_h - gap_y)
    half_gx = gap_x * 0.5
    half_gy = gap_y * 0.5

    # 3. Pass A: STN rear-polarizer subtle shadow (+0.7px, +0.9px) on light panels
    if not is_dark:
        cr.set_source_rgba(fg[0], fg[1], fg[2], 0.11)
        sh_x = max(0.55, px_w * 0.24)
        sh_y = max(0.65, px_h * 0.24)
        idx = 0
        for row in range(LCD_H):
            py = my + row * px_h + half_gy + sh_y
            page_off = (row >> 3) * 128
            mask = 1 << (row & 7)
            for col in range(LCD_W):
                val = ghost[idx] if ghost is not None else (
                    (0.0 if (fb[page_off + col] & mask) else 1.0) if st.invert_lcd
                    else (1.0 if (fb[page_off + col] & mask) else 0.0)
                )
                if val > 0.35:
                    cr.rectangle(mx + col * px_w + half_gx + sh_x, py, dw, dh)
                idx += 1
        cr.fill()

    # 4. Pass B: Active liquid crystal dots (batched for full-ON pixels for blazing speed!)
    cr.set_source_rgb(fg[0], fg[1], fg[2])
    partials: list[tuple[float, float, float]] = []
    idx = 0
    for row in range(LCD_H):
        py = my + row * px_h + half_gy
        page_off = (row >> 3) * 128
        mask = 1 << (row & 7)
        for col in range(LCD_W):
            if ghost is not None:
                val = ghost[idx]
            else:
                bit = 1 if (fb[page_off + col] & mask) else 0
                val = (0.0 if bit else 1.0) if st.invert_lcd else (1.0 if bit else 0.0)
            if val >= 0.96:
                cr.rectangle(mx + col * px_w + half_gx, py, dw, dh)
            elif val > 0.02:
                partials.append((mx + col * px_w + half_gx, py, val))
            idx += 1
    cr.fill()

    # Draw intermediate ghosting pixels
    for px_x, px_y, val in partials:
        r_c = bg[0] + (fg[0] - bg[0]) * val
        g_c = bg[1] + (fg[1] - bg[1]) * val
        b_c = bg[2] + (fg[2] - bg[2]) * val
        cr.set_source_rgb(r_c, g_c, b_c)
        cr.rectangle(px_x, px_y, dw, dh)
        cr.fill()

    # 5. Recessed inner bezel shadow cast onto glass edges
    shadow_d = 6.0
    for (x0, y0, x1, y1, rx, ry, rw, rh) in (
        (x, y, x, y + shadow_d, x, y, w, shadow_d),
        (x, y + h, x, y + h - shadow_d * 0.7, x, y + h - shadow_d * 0.7, w, shadow_d * 0.7),
        (x, y, x + shadow_d * 0.85, y, x, y, shadow_d * 0.85, h),
        (x + w, y, x + w - shadow_d * 0.85, y, x + w - shadow_d * 0.85, y, shadow_d * 0.85, h),
    ):
        sg = cairo.LinearGradient(x0, y0, x1, y1)
        sg.add_color_stop_rgba(0.0, 0.0, 0.0, 0.0, 0.28)
        sg.add_color_stop_rgba(1.0, 0.0, 0.0, 0.0, 0.0)
        cr.set_source(sg)
        cr.rectangle(rx, ry, rw, rh)
        cr.fill()

    # 6. Subtle matte polarizer glass reflection
    sheen = cairo.LinearGradient(x, y, x + w * 0.55, y + h * 0.75)
    sheen.add_color_stop_rgba(0.0, 1.0, 1.0, 1.0, 0.08)
    sheen.add_color_stop_rgba(0.42, 1.0, 1.0, 1.0, 0.015)
    sheen.add_color_stop_rgba(0.43, 1.0, 1.0, 1.0, 0.0)
    cr.set_source(sheen)
    cr.rectangle(x, y, w, h)
    cr.fill()
    cr.restore()


# =============================================================================
# 2. ACCURATE CAIRO HARDWARE MODELS (UV-K1(8) & UV-K5 v3 / UV-K6)
# =============================================================================

# UV-K1 exact 4x4 physical matrix (matches inspo/UV-K1(8)渲染图带液晶 & keys.rs)
K1_MATRIX: list[list[tuple[str, str, str, str]]] = [
    [("menu", "MENU", "", ""), ("up", "<", "", ""), ("down", ">", "", ""), ("exit", "EXIT", "", "")],
    [("1", "1", "Band", ".,?"), ("2", "2", "A/B", "abc"), ("3", "3", "V/M", "def"), ("star", "*", "Scan", "_RET_")],
    [("4", "4", "FC",   "ghi"), ("5", "5", "WX",  "jkl"), ("6", "6", "H/L", "mno"), ("0",    "0", "FM",   "")],
    [("7", "7", "VOX",  "pqrs"),("8", "8", "R",   "tuv"), ("9", "9", "Call","wxyz"),("f",    "#", "LOCK", "F")],
]

# UV-K5 v3 / UV-K6 exact lower 4x3 matrix (matches inspo/K6图片+视频去掉3D图/UVK6.1.png)
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


def _draw_torx_bolt(cr: Any, cx: float, cy: float, r: float = 5.5) -> None:
    import cairo
    # Outer recessed socket
    cr.set_source_rgb(0.14, 0.14, 0.14)
    cr.arc(cx, cy, r + 1.4, 0, 2 * math.pi)
    cr.fill()
    # Machined stainless bolt head
    pat = cairo.RadialGradient(cx - r * 0.3, cy - r * 0.3, 0.5, cx, cy, r)
    pat.add_color_stop_rgb(0.0, 0.88, 0.87, 0.84)
    pat.add_color_stop_rgb(0.65, 0.55, 0.54, 0.51)
    pat.add_color_stop_rgb(1.0, 0.30, 0.29, 0.27)
    cr.set_source(pat)
    cr.arc(cx, cy, r, 0, 2 * math.pi)
    cr.fill()
    # Hex/Torx dark center
    cr.set_source_rgb(0.16, 0.15, 0.14)
    cr.arc(cx, cy, r * 0.42, 0, 2 * math.pi)
    cr.fill()


def _draw_key_cap(
    cr: Any,
    x: float,
    y: float,
    w: float,
    h: float,
    primary: str,
    sub1: str,
    sub2: str,
    *,
    pressed: bool,
    hovered: bool,
    radius: float = 5.0,
    is_nav: bool = False,
    accent_ring: tuple[float, float, float] | None = None,
) -> None:
    import cairo
    # Recessed milled well shadow
    _round_rect(cr, x - 1.2, y - 1.2, w + 2.4, h + 2.4, radius + 1.0)
    cr.set_source_rgba(0.04, 0.04, 0.05, 0.92)
    cr.fill()

    dy = 1.2 if pressed else 0.0
    _round_rect(cr, x, y + dy, w, h - dy * 0.4, radius)
    pat = cairo.LinearGradient(x, y + dy, x, y + h)
    if pressed:
        pat.add_color_stop_rgb(0.0, 0.08, 0.09, 0.11)
        pat.add_color_stop_rgb(1.0, 0.14, 0.16, 0.19)
    elif hovered:
        pat.add_color_stop_rgb(0.0, 0.25, 0.27, 0.30)
        pat.add_color_stop_rgb(1.0, 0.15, 0.16, 0.19)
    else:
        pat.add_color_stop_rgb(0.0, 0.19, 0.20, 0.22)
        pat.add_color_stop_rgb(0.55, 0.13, 0.14, 0.16)
        pat.add_color_stop_rgb(1.0, 0.09, 0.10, 0.12)
    cr.set_source(pat)
    cr.fill_preserve()

    # Tactile rim highlight / pressed glow
    if pressed:
        ar, ag, ab = accent_ring or (0.25, 0.62, 1.0)
        cr.set_source_rgba(ar, ag, ab, 0.92)
        cr.set_line_width(1.6)
    else:
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.14 if hovered else 0.09)
        cr.set_line_width(0.9)
    cr.stroke()

    # Crisp silkscreen typography & vector symbols (zero font-fallback tofu!)
    cr.set_source_rgba(0.95, 0.96, 0.98, 0.96 if not pressed else 1.0)
    if primary in ("_CHEV_UP_", "_CHEV_DN_"):
        cx = x + w * 0.5
        cy = y + dy + h * 0.44 if primary == "_CHEV_UP_" else y + dy + h * 0.56
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
            cr.set_source_rgba(0.82, 0.86, 0.92, 0.80)
            se = cr.text_extents(sub1)
            cr.move_to(cx - se.width * 0.5 - se.x_bearing, y + dy + (h * 0.82 if primary == "_CHEV_UP_" else h * 0.34))
            cr.show_text(sub1)
    elif is_nav or (not sub1 and not sub2):
        cr.select_font_face("Sans", cairo.FONT_SLANT_NORMAL, cairo.FONT_WEIGHT_BOLD)
        fsize = 16.0 if primary in ("<", ">") else 10.5
        cr.set_font_size(fsize)
        ext = cr.text_extents(primary)
        tx = x + (w - ext.width) * 0.5 - ext.x_bearing
        ty = y + dy + (h - ext.height) * 0.5 - ext.y_bearing
        cr.move_to(tx, ty)
        cr.show_text(primary)
    else:
        # Left bold primary digit/symbol + right stacked silkscreen secondary legends
        cr.select_font_face("Sans", cairo.FONT_SLANT_NORMAL, cairo.FONT_WEIGHT_BOLD)
        cr.set_font_size(14.5)
        pe = cr.text_extents(primary)
        px = x + 7.5
        py = y + dy + (h - pe.height) * 0.5 - pe.y_bearing
        cr.move_to(px, py)
        cr.show_text(primary)

        sx = px + max(pe.x_advance, 10.0) + 3.5
        cr.select_font_face("Sans", cairo.FONT_SLANT_NORMAL, cairo.FONT_WEIGHT_BOLD)
        cr.set_font_size(7.6)
        cr.set_source_rgba(0.90, 0.92, 0.95, 0.88)
        if sub1 and sub2:
            cr.move_to(sx, y + dy + h * 0.43)
            cr.show_text(sub1)
            if sub2 == "_RET_":
                # Crisp vector return arrow (matches UV-K1 * Scan silkscreen)
                cr.set_source_rgba(0.78, 0.82, 0.88, 0.78)
                cr.set_line_width(1.2)
                cr.set_line_cap(cairo.LINE_CAP_ROUND)
                ry = y + dy + h * 0.72
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
                cr.move_to(sx, y + dy + h * 0.78)
                cr.show_text(sub2)
        elif sub1:
            s1e = cr.text_extents(sub1)
            cr.move_to(sx, y + dy + (h - s1e.height) * 0.5 - s1e.y_bearing)
            cr.show_text(sub1)


def compute_handset_geometry(model: str) -> tuple[int, int]:
    """Canonical logical coordinate box (W, H) for each authentic radio model."""
    return (336, 598) if model == "k5" else (344, 548)


def draw_handset_cairo(
    cr: Any,
    model: str,
    fb: bytes,
    lcd_st: LcdSettings,
    *,
    ghost: list[float] | None = None,
    pressed_keys: set[str] | frozenset[str] = frozenset(),
    hovered_key: str | None = None,
    led_state: str = "off",   # "off" | "rx" | "tx"
    torch_on: bool = False,
    out_hitboxes: dict[str, tuple[float, float, float, float]] | None = None,
) -> None:
    """
    Renders an authentic, CAD-proportioned Quansheng UV-K1(8) or UV-K5 v3 / UV-K6
    handset onto any Cairo context (live GTK4 DrawingArea OR offscreen GIF surface!).
    Populates `out_hitboxes[key_name] = (x, y, w, h)` for exact pointer hit-testing.
    """
    if model == "k5":
        _draw_uvk5v3_cairo(
            cr, fb, lcd_st, ghost=ghost, pressed_keys=pressed_keys,
            hovered_key=hovered_key, led_state=led_state, torch_on=torch_on,
            out_hitboxes=out_hitboxes,
        )
    else:
        _draw_uvk1_cairo(
            cr, fb, lcd_st, ghost=ghost, pressed_keys=pressed_keys,
            hovered_key=hovered_key, led_state=led_state, torch_on=torch_on,
            out_hitboxes=out_hitboxes,
        )


def _draw_uvk1_cairo(
    cr: Any,
    fb: bytes,
    lcd_st: LcdSettings,
    *,
    ghost: list[float] | None,
    pressed_keys: set[str] | frozenset[str],
    hovered_key: str | None,
    led_state: str,
    torch_on: bool,
    out_hitboxes: dict[str, tuple[float, float, float, float]] | None,
) -> None:
    """Faithful Quansheng UV-K1(8) ("Mini Kong") modeled directly from factory front/left renders."""
    import cairo
    W, H = compute_handset_geometry("k1")
    bx, by, bw, bh = 28.0, 56.0, W - 44.0, H - 68.0
    hits = out_hitboxes if out_hitboxes is not None else {}
    hits.clear()

    # --- 1. Top Hardware Crown (Stubby A1 Antenna Base + Center LED + Cobalt-Blue Striped Knob) ---
    # Left: stubby matte-black A1 antenna base (cropped cleanly at top of frame)
    ax = bx + 54.0
    pat_ant = cairo.LinearGradient(ax - 22, 0, ax + 22, 0)
    pat_ant.add_color_stop_rgb(0.0, 0.10, 0.11, 0.12)
    pat_ant.add_color_stop_rgb(0.35, 0.25, 0.26, 0.28)
    pat_ant.add_color_stop_rgb(0.75, 0.14, 0.15, 0.16)
    pat_ant.add_color_stop_rgb(1.0, 0.08, 0.09, 0.10)
    cr.set_source(pat_ant)
    _round_rect(cr, ax - 19, 4, 38, by + 4, 5)
    cr.fill()
    # Antenna collar step
    cr.set_source_rgb(0.11, 0.12, 0.13)
    cr.rectangle(ax - 22, by - 14, 44, 16)
    cr.fill()
    cr.set_source_rgba(0.75, 0.77, 0.80, 0.26)
    cr.select_font_face("Sans", cairo.FONT_SLANT_NORMAL, cairo.FONT_WEIGHT_BOLD)
    cr.set_font_size(9.5)
    cr.move_to(ax - 6, 30)
    cr.show_text("A1")

    # Center: low-profile LED flashlight bump (clickable!)
    tx = bx + bw * 0.49
    if torch_on:
        tg = cairo.RadialGradient(tx, by - 8, 2, tx, by - 8, 30)
        tg.add_color_stop_rgba(0.0, 1.0, 0.98, 0.82, 0.90)
        tg.add_color_stop_rgba(1.0, 1.0, 0.94, 0.65, 0.0)
        cr.set_source(tg)
        cr.arc(tx, by - 8, 30, 0, 2 * math.pi)
        cr.fill()
    cr.set_source_rgb(0.15, 0.16, 0.18)
    _round_rect(cr, tx - 16, by - 12, 32, 16, 4)
    cr.fill()
    hits["_torch"] = (tx - 18, by - 18, 36, 22)

    # Right: Fluted black rotary volume/power knob with UV-K1 vertical cobalt-blue stripe!
    kx = bx + bw - 56.0
    pat_k = cairo.LinearGradient(kx - 28, 0, kx + 28, 0)
    pat_k.add_color_stop_rgb(0.0, 0.08, 0.09, 0.10)
    pat_k.add_color_stop_rgb(0.32, 0.24, 0.26, 0.29)
    pat_k.add_color_stop_rgb(0.72, 0.13, 0.14, 0.16)
    pat_k.add_color_stop_rgb(1.0, 0.07, 0.08, 0.09)
    cr.set_source(pat_k)
    _round_rect(cr, kx - 26, by - 36, 52, 40, 6)
    cr.fill()
    # Knurl flutes
    cr.set_source_rgba(0.0, 0.0, 0.0, 0.45)
    cr.set_line_width(1.6)
    for dx in (-18, -10, 10, 18):
        cr.move_to(kx + dx, by - 33)
        cr.line_to(kx + dx, by - 2)
        cr.stroke()
    # Signature vertical cobalt-blue indicator stripe (exact match to UV-K1前视图.png)
    cr.set_source_rgb(0.14, 0.50, 1.00)
    cr.rectangle(kx - 3.0, by - 36, 6.0, 34)
    cr.fill()

    # Translucent status LED bump on top-left shoulder (exact K1 position!)
    lx, ly = bx + 44.0, by - 4.0
    if led_state == "tx":
        cr.set_source_rgb(0.96, 0.24, 0.24)
    elif led_state == "rx":
        cr.set_source_rgb(0.18, 0.84, 0.42)
    else:
        cr.set_source_rgb(0.72, 0.74, 0.76)
    _round_rect(cr, lx - 10, ly - 5, 20, 8, 3)
    cr.fill()
    if led_state in ("rx", "tx"):
        lg = cairo.RadialGradient(lx, ly, 2, lx, ly, 18)
        if led_state == "tx":
            lg.add_color_stop_rgba(0.0, 1.0, 0.25, 0.25, 0.70)
            lg.add_color_stop_rgba(1.0, 1.0, 0.25, 0.25, 0.0)
        else:
            lg.add_color_stop_rgba(0.0, 0.20, 0.90, 0.45, 0.65)
            lg.add_color_stop_rgba(1.0, 0.20, 0.90, 0.45, 0.0)
        cr.set_source(lg)
        cr.arc(lx, ly, 18, 0, 2 * math.pi)
        cr.fill()

    # --- 2. Left Side Physical Keys (PTT with cobalt-blue oval ring + SK1 + SK2) ---
    ptt_rect = (4.0, by + 56.0, 26.0, 114.0)
    sk1_rect = (6.0, by + 180.0, 24.0, 36.0)
    sk2_rect = (6.0, by + 224.0, 24.0, 36.0)
    hits["ptt"] = ptt_rect
    hits["side1"] = sk1_rect
    hits["side2"] = sk2_rect

    # PTT paddle
    px, py, pw, ph = ptt_rect
    ptt_down = "ptt" in pressed_keys
    _round_rect(cr, px + (2.0 if ptt_down else 0.0), py, pw, ph, 7)
    cr.set_source_rgb(0.58, 0.12, 0.14) if ptt_down else cr.set_source_rgb(0.14, 0.15, 0.17)
    cr.fill_preserve()
    cr.set_source_rgb(0.95, 0.35, 0.35) if ptt_down else cr.set_source_rgb(0.16, 0.54, 1.0)
    cr.set_line_width(2.0)
    cr.stroke()
    # Blue vertical center ribs inside PTT (from UV-K1左视图.png)
    cr.set_source_rgb(1.0, 0.72, 0.72) if ptt_down else cr.set_source_rgb(0.18, 0.56, 1.0)
    for ry in (py + 22, py + 44, py + 66, py + 88):
        _round_rect(cr, px + 9 + (2.0 if ptt_down else 0.0), ry, 4.0, 12.0, 2)
        cr.fill()

    for kn, (sx, sy, sw, sh), dots in (("side1", sk1_rect, 2), ("side2", sk2_rect, 1)):
        down = kn in pressed_keys
        hov = hovered_key == kn
        _round_rect(cr, sx + (2.0 if down else 0.0), sy, sw, sh, 6)
        cr.set_source_rgb(0.10, 0.11, 0.13) if down else cr.set_source_rgb(0.20 if hov else 0.15, 0.21 if hov else 0.16, 0.24 if hov else 0.18)
        cr.fill_preserve()
        cr.set_source_rgba(0.25, 0.62, 1.0, 0.9) if down else cr.set_source_rgba(1, 1, 1, 0.12)
        cr.set_line_width(1.2)
        cr.stroke()
        cr.set_source_rgba(0.75, 0.78, 0.82, 0.65)
        if dots == 2:
            cr.arc(sx + 10, sy + sh * 0.38, 1.8, 0, 2 * math.pi)
            cr.arc(sx + 10, sy + sh * 0.62, 1.8, 0, 2 * math.pi)
        else:
            cr.arc(sx + 10, sy + sh * 0.5, 1.8, 0, 2 * math.pi)
        cr.fill()

    # --- 3. Outer Matte-Black Polymer Chassis Shell ---
    _round_rect(cr, bx, by, bw, bh, 14)
    pat_shell = cairo.LinearGradient(bx, by, bx + bw, by)
    pat_shell.add_color_stop_rgb(0.0, 0.14, 0.15, 0.16)
    pat_shell.add_color_stop_rgb(0.10, 0.09, 0.10, 0.11)
    pat_shell.add_color_stop_rgb(0.90, 0.09, 0.10, 0.11)
    pat_shell.add_color_stop_rgb(1.0, 0.16, 0.17, 0.18)
    cr.set_source(pat_shell)
    cr.fill_preserve()
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.10)
    cr.set_line_width(1.0)
    cr.stroke()

    # --- 4. Warm Brushed Gunmetal/Titanium Metallic Armor Faceplate (EXACT UV-K1 Silhouette!) ---
    # Flared top & bottom bolt ears, inset waist around the black polymer side grips!
    fx_l = bx + 16.0
    fx_r = bx + bw - 16.0
    fy_t = by + 6.0
    fy_b = by + bh - 8.0
    cr.new_sub_path()
    cr.move_to(bx + 10, fy_t)
    cr.line_to(bx + bw - 10, fy_t)
    cr.line_to(bx + bw - 4, fy_t + 8)
    cr.line_to(bx + bw - 4, fy_t + 40)        # Upper right ear
    cr.line_to(fx_r, fy_t + 54)
    cr.line_to(fx_r, fy_b - 92)               # Inset metallic waist
    cr.line_to(bx + bw - 4, fy_b - 78)        # Lower right ear
    cr.line_to(bx + bw - 4, fy_b - 8)
    cr.line_to(bx + bw - 12, fy_b)
    cr.line_to(bx + 12, fy_b)
    cr.line_to(bx + 4, fy_b - 8)
    cr.line_to(bx + 4, fy_b - 78)             # Lower left ear
    cr.line_to(fx_l, fy_b - 92)
    cr.line_to(fx_l, fy_t + 54)
    cr.line_to(bx + 4, fy_t + 40)             # Upper left ear
    cr.line_to(bx + 4, fy_t + 8)
    cr.close_path()

    pat_metal = cairo.LinearGradient(bx, fy_t, bx + bw, fy_b)
    pat_metal.add_color_stop_rgb(0.0, 0.56, 0.54, 0.50)   # Warm brushed titanium highlight
    pat_metal.add_color_stop_rgb(0.35, 0.44, 0.42, 0.39)
    pat_metal.add_color_stop_rgb(0.75, 0.36, 0.35, 0.32)
    pat_metal.add_color_stop_rgb(1.0, 0.46, 0.44, 0.40)
    cr.set_source(pat_metal)
    cr.fill_preserve()
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.24)
    cr.set_line_width(1.2)
    cr.stroke()

    # 4 Real Machined Silver Torx Bolts in the 4 outer ears!
    _draw_torx_bolt(cr, bx + 20.0, fy_t + 22.0, 5.4)
    _draw_torx_bolt(cr, bx + bw - 20.0, fy_t + 22.0, 5.4)
    _draw_torx_bolt(cr, bx + 20.0, fy_b - 42.0, 5.4)
    _draw_torx_bolt(cr, bx + bw - 20.0, fy_b - 42.0, 5.4)

    # --- 5. Glossy Piano-Black Top Nameplate Pill ("QUANSHENG" + RF arcs) ---
    np_x, np_y, np_w, np_h = bx + 42.0, fy_t + 7.0, bw - 84.0, 28.0
    _round_rect(cr, np_x, np_y, np_w, np_h, 5)
    pat_np = cairo.LinearGradient(np_x, np_y, np_x, np_y + np_h)
    pat_np.add_color_stop_rgb(0.0, 0.14, 0.15, 0.16)
    pat_np.add_color_stop_rgb(0.45, 0.05, 0.06, 0.07)
    pat_np.add_color_stop_rgb(1.0, 0.10, 0.11, 0.12)
    cr.set_source(pat_np)
    cr.fill_preserve()
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.12)
    cr.set_line_width(0.8)
    cr.stroke()

    # RF wave arcs + italic bold QUANSHENG
    cr.set_source_rgba(0.95, 0.96, 0.98, 0.94)
    cr.set_line_width(1.5)
    cr.set_line_cap(cairo.LINE_CAP_ROUND)
    arc_cx, arc_cy = np_x + 44.0, np_y + 18.0
    for ar in (5.0, 9.0):
        cr.arc(arc_cx, arc_cy, ar, math.pi * 0.90, math.pi * 1.48)
        cr.stroke()
    cr.select_font_face("Sans", cairo.FONT_SLANT_ITALIC, cairo.FONT_WEIGHT_BOLD)
    cr.set_font_size(13.5)
    cr.move_to(np_x + 48.0, np_y + 20.0)
    cr.show_text("QUANSHENG")

    # --- 6. Deep Beveled LCD Window (Exact UV-K1 Proportions) ---
    lz_x, lz_y, lz_w, lz_h = fx_l + 14.0, fy_t + 48.0, (fx_r - fx_l) - 28.0, 162.0
    # Outer dark stepped bezel
    _round_rect(cr, lz_x, lz_y, lz_w, lz_h, 8)
    cr.set_source_rgb(0.09, 0.09, 0.10)
    cr.fill_preserve()
    cr.set_source_rgba(0.0, 0.0, 0.0, 0.75)
    cr.set_line_width(2.0)
    cr.stroke()

    # Optional subtle backlight spill around inner LCD glass
    if lcd_st.backlight_glow and lcd_st.backlight > 0:
        bg_c, _, _, _ = _lcd_colors_norm(lcd_st)
        cr.set_source_rgba(bg_c[0], bg_c[1], bg_c[2], 0.20)
        _round_rect(cr, lz_x + 5, lz_y + 5, lz_w - 10, lz_h - 10, 6)
        cr.set_line_width(3.0)
        cr.stroke()

    # Draw authentic ST7565 LCD panel inside bezel
    draw_st7565_cairo(cr, lz_x + 8.0, lz_y + 8.0, lz_w - 16.0, lz_h - 16.0, fb, lcd_st, ghost=ghost)

    # --- 7. Horizontal Mid-Bezel Step, Engraved "UHF/VHF Transceiver", & Mic Slot ---
    div_y = lz_y + lz_h + 18.0
    cr.set_source_rgba(0.08, 0.08, 0.08, 0.72)
    cr.select_font_face("Sans", cairo.FONT_SLANT_NORMAL, cairo.FONT_WEIGHT_BOLD)
    cr.set_font_size(9.5)
    te = cr.text_extents("UHF/VHF Transceiver")
    cr.move_to(fx_r - 12.0 - te.width, div_y - 3.0)
    cr.show_text("UHF/VHF Transceiver")

    # Crisp machined horizontal groove separating screen armor from keypad armor
    cr.set_source_rgba(0.05, 0.05, 0.06, 0.65)
    cr.set_line_width(1.6)
    cr.move_to(fx_l + 6, div_y + 5.0)
    cr.line_to(fx_r - 6, div_y + 5.0)
    cr.stroke()
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.16)
    cr.set_line_width(0.8)
    cr.move_to(fx_l + 6, div_y + 6.4)
    cr.line_to(fx_r - 6, div_y + 6.4)
    cr.stroke()

    # Tiny horizontal mic cutout right above EXIT
    _round_rect(cr, fx_r - 40.0, div_y + 11.0, 26.0, 5.0, 2.5)
    cr.set_source_rgb(0.06, 0.06, 0.07)
    cr.fill()

    # --- 8. Exact 4x4 Individual Milled Metallic Lattice Keypad (NO generic black box!) ---
    kp_x = fx_l + 10.0
    kp_y = div_y + 22.0
    kp_w = (fx_r - fx_l) - 20.0
    gap_c = 7.0
    gap_r = 8.0
    kw = (kp_w - gap_c * 3.0) / 4.0
    kh_nav = 35.0
    kh_num = 34.0

    for r_idx, row_keys in enumerate(K1_MATRIX):
        ky = kp_y + (0.0 if r_idx == 0 else (kh_nav + gap_r + (r_idx - 1) * (kh_num + gap_r)))
        kh = kh_nav if r_idx == 0 else kh_num
        for c_idx, (kn, pri, s1, s2) in enumerate(row_keys):
            kx = kp_x + c_idx * (kw + gap_c)
            hits[kn] = (kx, ky, kw, kh)
            _draw_key_cap(
                cr, kx, ky, kw, kh, pri, s1, s2,
                pressed=(kn in pressed_keys),
                hovered=(hovered_key == kn),
                radius=5.0,
                is_nav=(r_idx == 0),
                accent_ring=(0.18, 0.56, 1.0),
            )


def _draw_uvk5v3_cairo(
    cr: Any,
    fb: bytes,
    lcd_st: LcdSettings,
    *,
    ghost: list[float] | None,
    pressed_keys: set[str] | frozenset[str],
    hovered_key: str | None,
    led_state: str,
    torch_on: bool,
    out_hitboxes: dict[str, tuple[float, float, float, float]] | None,
) -> None:
    """Faithful Quansheng UV-K5 v3 / UV-K6 modeled directly from factory UVK6.1.png."""
    import cairo
    W, H = compute_handset_geometry("k5")
    bx, by, bw, bh = 28.0, 56.0, W - 44.0, H - 68.0
    hits = out_hitboxes if out_hitboxes is not None else {}
    hits.clear()

    # --- 1. Top Crown (Tapered Whip Antenna Base + Orange-Capped Volume Knob + LED) ---
    ax = bx + 52.0
    pat_ant = cairo.LinearGradient(ax - 22, 0, ax + 22, 0)
    pat_ant.add_color_stop_rgb(0.0, 0.11, 0.12, 0.13)
    pat_ant.add_color_stop_rgb(0.4, 0.25, 0.26, 0.28)
    pat_ant.add_color_stop_rgb(1.0, 0.08, 0.09, 0.10)
    cr.set_source(pat_ant)
    cr.move_to(ax - 20, by + 4)
    cr.line_to(ax - 15, 4)
    cr.line_to(ax + 15, 4)
    cr.line_to(ax + 20, by + 4)
    cr.close_path()
    cr.fill()

    # Center flashlight bump (clickable)
    tx = bx + bw * 0.48
    if torch_on:
        tg = cairo.RadialGradient(tx, by - 6, 2, tx, by - 6, 28)
        tg.add_color_stop_rgba(0.0, 1.0, 0.98, 0.80, 0.9)
        tg.add_color_stop_rgba(1.0, 1.0, 0.94, 0.65, 0.0)
        cr.set_source(tg)
        cr.arc(tx, by - 6, 28, 0, 2 * math.pi)
        cr.fill()
    cr.set_source_rgb(0.15, 0.16, 0.18)
    _round_rect(cr, tx - 14, by - 10, 28, 12, 3)
    cr.fill()
    hits["_torch"] = (tx - 16, by - 16, 32, 20)

    # Right: Signature Quansheng Vivid Orange-Capped Volume Knob (exact UVK6.1.png!)
    kx = bx + bw - 56.0
    _round_rect(cr, kx - 26, by - 26, 52, 30, 5)
    cr.set_source_rgb(0.14, 0.15, 0.17)
    cr.fill()
    # Vivid orange top cap
    _round_rect(cr, kx - 26, by - 36, 52, 13, 5)
    pat_o = cairo.LinearGradient(kx - 26, by - 36, kx + 26, by - 23)
    pat_o.add_color_stop_rgb(0.0, 0.92, 0.36, 0.04)
    pat_o.add_color_stop_rgb(0.45, 1.00, 0.52, 0.10)
    pat_o.add_color_stop_rgb(1.0, 0.82, 0.30, 0.03)
    cr.set_source(pat_o)
    cr.fill()

    # Translucent status LED on top-left shoulder
    lx, ly = bx + 52.0, by - 2.0
    cr.set_source_rgb(0.95, 0.24, 0.24) if led_state == "tx" else (
        cr.set_source_rgb(0.18, 0.84, 0.42) if led_state == "rx" else cr.set_source_rgb(0.75, 0.77, 0.79)
    )
    _round_rect(cr, lx - 10, ly - 5, 20, 8, 3)
    cr.fill()

    # --- 2. Left Side PTT / SK1 / SK2 ---
    ptt_rect = (4.0, by + 64.0, 26.0, 108.0)
    sk1_rect = (6.0, by + 182.0, 24.0, 34.0)
    sk2_rect = (6.0, by + 224.0, 24.0, 34.0)
    hits["ptt"], hits["side1"], hits["side2"] = ptt_rect, sk1_rect, sk2_rect
    for kn, (sx, sy, sw, sh), is_ptt in (("ptt", ptt_rect, True), ("side1", sk1_rect, False), ("side2", sk2_rect, False)):
        down = kn in pressed_keys
        _round_rect(cr, sx + (2.0 if down else 0.0), sy, sw, sh, 6)
        cr.set_source_rgb(0.60, 0.14, 0.14) if (is_ptt and down) else cr.set_source_rgb(0.15, 0.16, 0.18)
        cr.fill_preserve()
        if is_ptt:
            cr.set_source_rgb(0.98, 0.46, 0.08)
            cr.set_line_width(1.8)
        else:
            cr.set_source_rgba(1, 1, 1, 0.12)
            cr.set_line_width(1.0)
        cr.stroke()

    # --- 3. Matte Dark-Charcoal Main Polycarbonate Body ---
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

    # --- 4. Brushed Silver-Titanium Upper LCD Armor Frame (exact UVK6.1.png!) ---
    ux, uy, uw, uh = bx + 6.0, by + 6.0, bw - 12.0, 214.0
    _round_rect(cr, ux, uy, uw, uh, 14)
    pat_u = cairo.LinearGradient(ux, uy, ux + uw, uy + uh)
    pat_u.add_color_stop_rgb(0.0, 0.62, 0.61, 0.58)
    pat_u.add_color_stop_rgb(0.45, 0.46, 0.45, 0.43)
    pat_u.add_color_stop_rgb(1.0, 0.56, 0.55, 0.52)
    cr.set_source(pat_u)
    cr.fill_preserve()
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.22)
    cr.set_line_width(1.0)
    cr.stroke()

    # Corner mini screws on upper titanium bezel
    _draw_torx_bolt(cr, ux + 15, uy + 15, 4.0)
    _draw_torx_bolt(cr, ux + uw - 15, uy + 15, 4.0)
    _draw_torx_bolt(cr, ux + 15, uy + uh - 54, 4.0)
    _draw_torx_bolt(cr, ux + uw - 15, uy + uh - 22, 4.0)

    # Recessed LCD Screen Window
    lz_x, lz_y, lz_w, lz_h = ux + 20.0, uy + 14.0, uw - 40.0, 148.0
    _round_rect(cr, lz_x, lz_y, lz_w, lz_h, 7)
    cr.set_source_rgb(0.08, 0.09, 0.10)
    cr.fill()
    draw_st7565_cairo(cr, lz_x + 6.0, lz_y + 6.0, lz_w - 12.0, lz_h - 12.0, fb, lcd_st, ghost=ghost)

    # Below LCD inside lower edge of titanium frame: [ M ] left + glossy QUANSHENG badge right
    m_x, m_y, m_w, m_h = ux + 16.0, lz_y + lz_h + 10.0, 68.0, 32.0
    hits["menu"] = (m_x, m_y, m_w, m_h)
    _draw_key_cap(
        cr, m_x, m_y, m_w, m_h, "M", "A", "",
        pressed=("menu" in pressed_keys), hovered=(hovered_key == "menu"),
        radius=6.0, accent_ring=(0.98, 0.50, 0.10),
    )

    np_x, np_y, np_w, np_h = m_x + m_w + 10.0, m_y + 2.0, uw - (m_w + 56.0), 28.0
    _round_rect(cr, np_x, np_y, np_w, np_h, 8)
    cr.set_source_rgb(0.07, 0.08, 0.09)
    cr.fill()
    cr.set_source_rgba(0.95, 0.96, 0.98, 0.92)
    cr.select_font_face("Sans", cairo.FONT_SLANT_ITALIC, cairo.FONT_WEIGHT_BOLD)
    cr.set_font_size(11.5)
    cr.move_to(np_x + 14.0, np_y + 18.0)
    cr.show_text("QUANSHENG")

    # --- 5. Middle Section: Vertical Left Column (▲ / ▼ / EXIT) + 3 Bold Speaker Grille Bars! ---
    mid_y = uy + uh + 12.0
    col_x = bx + 18.0
    col_w = 56.0

    # Stacked ▲ (B) and ▼ (C) on the left alongside the 3 speaker ribs
    up_rect = (col_x, mid_y, col_w, 34.0)
    dn_rect = (col_x, mid_y + 42.0, col_w, 34.0)
    ex_rect = (col_x, mid_y + 92.0, col_w + 12.0, 28.0)
    hits["up"], hits["down"], hits["exit"] = up_rect, dn_rect, ex_rect

    _draw_key_cap(cr, *up_rect, "_CHEV_UP_", "B", "", pressed=("up" in pressed_keys), hovered=(hovered_key == "up"), radius=7.0, is_nav=True, accent_ring=(0.98, 0.50, 0.10))
    _draw_key_cap(cr, *dn_rect, "_CHEV_DN_", "C", "", pressed=("down" in pressed_keys), hovered=(hovered_key == "down"), radius=7.0, is_nav=True, accent_ring=(0.98, 0.50, 0.10))
    _draw_key_cap(cr, *ex_rect, "EXIT", "", "", pressed=("exit" in pressed_keys), hovered=(hovered_key == "exit"), radius=6.0, is_nav=True, accent_ring=(0.98, 0.50, 0.10))

    # 3 Bold horizontal beveled speaker grille ribs to the right of ▲ / ▼ (exact UVK6.1.png!)
    gx = col_x + col_w + 14.0
    gw = (bx + bw - 18.0) - gx
    for i in range(3):
        gy = mid_y + 4.0 + i * 28.0
        _round_rect(cr, gx, gy, gw, 16.0, 5.0)
        cr.set_source_rgb(0.06, 0.06, 0.07)
        cr.fill()
        _round_rect(cr, gx + 3, gy + 4, gw - 6, 8.0, 3.0)
        cr.set_source_rgb(0.18, 0.19, 0.21)
        cr.fill()

    # --- 6. Lower Compact 4x3 Pill Keypad (below EXIT) ---
    kp_x = bx + 16.0
    kp_y = ex_rect[1] + ex_rect[3] + 10.0
    kp_w = bw - 32.0
    gap_c, gap_r = 6.0, 7.0
    kw = (kp_w - gap_c * 3.0) / 4.0
    kh = 32.0

    for r_idx, row_keys in enumerate(K5_BOTTOM_MATRIX):
        ky = kp_y + r_idx * (kh + gap_r)
        for c_idx, (kn, pri, s1) in enumerate(row_keys):
            kx = kp_x + c_idx * (kw + gap_c)
            hits[kn] = (kx, ky, kw, kh)
            _draw_key_cap(
                cr, kx, ky, kw, kh, pri, s1, "",
                pressed=(kn in pressed_keys), hovered=(hovered_key == kn),
                radius=6.0, accent_ring=(0.98, 0.50, 0.10),
            )


# =============================================================================
# 3. IMAGE & ANIMATED GIF ENCODER (Pillow Fast Path + Pure-Stdlib Fallback)
# =============================================================================

def render_lcd_to_rgba(fb: bytes, lcd_st: LcdSettings, width: int = 384, height: int = 266) -> tuple[int, int, bytes]:
    """Renders standalone realistic ST7565 LCD to RGBA bytes via Cairo."""
    import cairo
    surf = cairo.ImageSurface(cairo.FORMAT_ARGB32, width, height)
    cr = cairo.Context(surf)
    draw_st7565_cairo(cr, 0, 0, width, height, fb, lcd_st, ghost=None)
    surf.flush()
    raw = bytes(surf.get_data())
    # Cairo ARGB32 on little-endian is BGRA -> convert to RGBA
    out = bytearray(len(raw))
    out[0::4] = raw[2::4]
    out[1::4] = raw[1::4]
    out[2::4] = raw[0::4]
    out[3::4] = raw[3::4]
    return width, height, bytes(out)


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
) -> tuple[int, int, bytes]:
    """Renders the ENTIRE physical radio handset (with pressed buttons!) to RGBA bytes via Cairo."""
    import cairo
    lw, lh = compute_handset_geometry(model)
    w, h = int(round(lw * scale)), int(round(lh * scale))
    surf = cairo.ImageSurface(cairo.FORMAT_ARGB32, w, h)
    cr = cairo.Context(surf)
    cr.set_source_rgb(*bg_rgb)
    cr.rectangle(0, 0, w, h)
    cr.fill()
    cr.scale(scale, scale)
    draw_handset_cairo(
        cr, model, fb, lcd_st,
        ghost=None, pressed_keys=pressed_keys, hovered_key=None,
        led_state=led_state, torch_on=torch_on,
    )
    surf.flush()
    raw = bytes(surf.get_data())
    out = bytearray(len(raw))
    out[0::4] = raw[2::4]
    out[1::4] = raw[1::4]
    out[2::4] = raw[0::4]
    out[3::4] = raw[3::4]
    return w, h, bytes(out)


def encode_png_rgba(w: int, h: int, rgba: bytes) -> bytes:
    """Pure-stdlib RGBA PNG encoder."""
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
    """
    Event-driven non-destructive recording timeline supporting:
      • Trim In / Out bounds
      • Multiple Cut / Splice-Out middle segments with Undo
      • Playback speed multiplier & max idle pause clamping
      • Export as LCD-Only OR Full Radio Body animated GIF
    """

    def __init__(self) -> None:
        self.frames: list[RecordedFrame] = []
        self.recording: bool = False
        self.t0_mono: float = 0.0
        self.trim_in: int = 0
        self.trim_out: int = 0
        self.cuts: list[tuple[int, int]] = []  # inclusive frame ranges spliced out

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

    def clear_cuts(self) -> None:
        self.cuts.clear()

    def is_cut(self, idx: int) -> bool:
        return any(a <= idx <= b for a, b in self.cuts)

    def active_indices(self) -> list[int]:
        if not self.frames:
            return []
        lo = max(0, min(self.trim_in, len(self.frames) - 1))
        hi = max(lo, min(self.trim_out, len(self.frames) - 1))
        return [i for i in range(lo, hi + 1) if not self.is_cut(i)]

    def frame_durations_ms(self, speed: float = 1.0, max_idle_ms: int | None = None) -> list[tuple[int, int]]:
        """Returns list of (frame_index, delay_ms) for the active spliced sequence."""
        idxs = self.active_indices()
        if not idxs:
            return []
        spd = max(0.25, speed)
        out: list[tuple[int, int]] = []
        for pos, fi in enumerate(idxs):
            if pos + 1 < len(idxs):
                nxt = idxs[pos + 1]
                # If contiguous, use true elapsed dt; across a splice seam use crisp 120ms beat
                raw_dt = (self.frames[nxt].ts_ms - self.frames[fi].ts_ms) if nxt == fi + 1 else 120
            else:
                raw_dt = 350
            dt = max(30, raw_dt)
            if max_idle_ms is not None and dt > max_idle_ms:
                dt = max_idle_ms
            dt = max(20, int(round(dt / spd)))
            out.append((fi, dt))
        return out

    def export_gif(
        self,
        out_path: str | Path,
        *,
        model: str,
        lcd_st: LcdSettings,
        mode: str = "lcd",          # "lcd" | "body"
        speed: float = 1.0,
        max_idle_ms: int | None = None,
        progress_cb: Callable[[float], None] | None = None,
    ) -> int:
        """Exports the trimmed & spliced sequence as an optimized animated GIF."""
        seq = self.frame_durations_ms(speed=speed, max_idle_ms=max_idle_ms)
        if not seq:
            raise ValueError("No active frames to export (check trim & cut ranges)")

        from PIL import Image  # Pillow 11 verified present

        pil_frames: list[Any] = []
        delays: list[int] = []
        total = len(seq)
        for pos, (fi, dt_ms) in enumerate(seq):
            rf = self.frames[fi]
            if mode == "body":
                w, h, rgba = render_handset_to_rgba(
                    model, rf.fb, lcd_st,
                    pressed_keys=rf.pressed, led_state=rf.led, torch_on=rf.torch, scale=1.0,
                )
            else:
                w, h, rgba = render_lcd_to_rgba(rf.fb, lcd_st, width=410, height=284)
            img = Image.frombytes("RGBA", (w, h), rgba).convert("RGB")
            # Sharp adaptive quantization (no noisy dithering for crisp LCD pixel art!)
            pal_img = img.quantize(colors=64 if mode == "lcd" else 128, method=Image.Quantize.MEDIANCUT, dither=Image.Dither.NONE)
            pil_frames.append(pal_img)
            delays.append(dt_ms)
            if progress_cb is not None:
                progress_cb((pos + 1) / float(total))

        dest = Path(out_path)
        pil_frames[0].save(
            dest,
            format="GIF",
            save_all=True,
            append_images=pil_frames[1:],
            duration=delays,
            loop=0,
            optimize=True,
            disposal=2,
        )
        return dest.stat().st_size


# =============================================================================
# 4. GUDEV HOTPLUG AUTO-DETECTION (Inspired by Antenna/src/backbone/deviceManager.js)
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


class PortMonitor:
    """Live USB serial device detector via GUdev (Linux) with pyserial fallback."""

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
                is_usb = (dev.get_property("ID_BUS") == "usb") or ("USB" in name) or ("ACM" in name)
                if not is_usb:
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
                v_clean = vendor.replace("_", " ").strip()
                m_clean = model.replace("_", " ").strip()
                pretty = m_clean if m_clean.lower().startswith("quansheng") else f"{v_clean} {m_clean}".strip() or dev_file
                is_qs = "quansheng" in pretty.lower() or "uv-k" in pretty.lower() or "kern" in pretty.lower()
                found.append(DetectedPort(dev_file, pretty, v_clean, m_clean, ser, vid, pid, is_qs))
        else:
            try:
                from serial.tools import list_ports
                for p in list_ports.comports():
                    desc = (p.product or p.description or "").strip()
                    is_qs = "quansheng" in desc.lower() or "uv-k" in desc.lower()
                    found.append(DetectedPort(p.device, desc or p.device, model=desc, serial=p.serial_number or "", is_quansheng=is_qs))
            except Exception:
                pass

        # Quansheng radios sorted first, then other USB serial, then QEMU & Demo
        found.sort(key=lambda d: (0 if d.is_quansheng else 1, d.device))
        found.append(DetectedPort("tcp://127.0.0.1:4446", "QEMU · 127.0.0.1:4446"))
        found.append(DetectedPort("demo", "Offline Demo"))

        changed = [p.device for p in found] != [p.device for p in self.ports]
        self.ports = found
        if changed and self.on_change is not None:
            self.on_change(found)
        return found


# =============================================================================
# 5. DEMO FRAMEBUFFER & LOW-LATENCY RIGWIRE ENGINE
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
    """Auto-detects physical radio shell ('k1' vs 'k5') from USB descriptors & RigWire identity."""
    blob = " ".join([
        volume_name or "",
        (port_info.pretty_name if port_info else ""),
        (port_info.model if port_info else ""),
    ]).lower()
    if "k5" in blob or "k6" in blob:
        return "k5"
    if "k1" in blob or "k10" in blob:
        return "k1"
    if hello:
        hw_prof = int(hello.get("hardware_profile", 0) or 0)
        if (hw_prof & 0xFF) == 0x05:
            return "k5"
    return "k1"


class DockLink:
    """Serialized low-latency RigWire bridge shared by LCD streaming, keys, Files, Wire, and WebDAV."""

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
                    if changed and self.on_frame is not None:
                        self.on_frame()
            except Exception:
                self.error_count += 1
                time.sleep(0.12)
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
            time.sleep(0.002)


# =============================================================================
# 6. NATIVE GTK4 + LIBADWAITA APPLICATION
# =============================================================================

GTK_CSS = """
button.rec-active {
    background: #dc2626;
    color: #ffffff;
}
button.rec-active:hover {
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
    closest = min(steps, key=lambda s: abs(s - pct))
    return f"battery-level-{closest}-symbolic"


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
            self.forced_model = model_override if model_override in ("k1", "k5") else None
            self.active_model = self.forced_model or "k1"
            self.user_picked_palette = palette_override is not None
            self.lcd_st = LcdSettings(
                palette=(
                    palette_override if (palette_override and palette_override in LCD_PALETTES)
                    else ("UV-K5" if self.active_model == "k5" else "UV-K1")
                )
            )
            self.ghost_buf: list[float] = [0.0] * (LCD_W * LCD_H)
            self._ghost_timer: int | None = None
            self._hitboxes: dict[str, tuple[float, float, float, float]] = {}
            self._pressed_keys: set[str] = set()
            self._hovered_key: str | None = None
            self._hold_timers: dict[str, int] = {}
            self._frame_pending = False
            self._updating_switches = False
            self.current_dir = "/"

            # GIF editor state
            self._pb_playing = False
            self._pb_idx = 0
            self._pb_timer: int | None = None
            self._cut_mark_start: int | None = None
            self._export_mode = "lcd"  # "lcd" | "body"
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
            self.win.set_default_size(940, 660)

            self.link.on_frame = self._schedule_frame_update
            self.link.on_telemetry = lambda: GLib.idle_add(self._refresh_telemetry_ui)
            self.link.on_log = lambda line: GLib.idle_add(self._append_console_line, line)

            # Live GUdev hotplug port monitor
            self.port_monitor = PortMonitor(on_change=lambda _p: GLib.idle_add(self._refresh_port_dropdown))

            # --- Clean Native HeaderBar (Antenna-inspired Status Cluster + ViewSwitcher) ---
            header = Adw.HeaderBar() if has_adw else Gtk.HeaderBar()

            # Start: Record GIF toggle + Screenshot PNG
            start_box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=6)
            self.rec_btn = Gtk.ToggleButton()
            rec_in = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=5)
            rec_in.append(Gtk.Image.new_from_icon_name("media-record-symbolic"))
            self.rec_lbl = Gtk.Label(label="Record")
            rec_in.append(self.rec_lbl)
            self.rec_btn.set_child(rec_in)
            self.rec_btn.set_tooltip_text("Record radio LCD / full handset to GIF")
            self.rec_btn.connect("toggled", self._on_record_toggled)
            start_box.append(self.rec_btn)

            shot_btn = Gtk.Button.new_from_icon_name("camera-photo-symbolic")
            shot_btn.set_tooltip_text("Save PNG screenshot")
            shot_btn.connect("clicked", self._on_save_screenshot)
            start_box.append(shot_btn)
            header.pack_start(start_box)

            # Center: Native Adw.ViewSwitcher (Device · Files · Wire · GIF)
            self.dock_stack = Adw.ViewStack() if has_adw else Gtk.Stack()
            if has_adw:
                switcher = Adw.ViewSwitcher()
                switcher.set_stack(self.dock_stack)
                switcher.set_policy(Adw.ViewSwitcherPolicy.WIDE)
                header.set_title_widget(switcher)

            # End: Antenna-style live Link + Battery Status MenuButton + Display Settings + Sidebar Toggle
            end_box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=6)

            status_btn = Gtk.MenuButton()
            status_btn.set_tooltip_text("Radio connection & USB status")
            st_in = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=6)
            self.hdr_link_icon = Gtk.Image.new_from_icon_name("network-offline-symbolic")
            self.hdr_batt_icon = Gtk.Image.new_from_icon_name("battery-missing-symbolic")
            self.hdr_dev_lbl = Gtk.Label(label="Disconnected")
            st_in.append(self.hdr_link_icon)
            st_in.append(self.hdr_batt_icon)
            st_in.append(self.hdr_dev_lbl)
            status_btn.set_child(st_in)
            status_btn.set_popover(self._build_connection_popover())
            end_box.append(status_btn)

            disp_btn = Gtk.MenuButton(icon_name="preferences-desktop-display-symbolic")
            disp_btn.set_tooltip_text("LCD optical settings")
            disp_btn.set_popover(self._build_display_popover())
            end_box.append(disp_btn)

            self.pane_toggle = Gtk.ToggleButton(icon_name="sidebar-show-right-symbolic", active=True)
            self.pane_toggle.set_tooltip_text("Toggle dock workspace")
            end_box.append(self.pane_toggle)
            header.pack_end(end_box)

            # --- Left Pane: Precision Cairo Radio Handset Canvas ---
            self.handset_area = Gtk.DrawingArea()
            lw, lh = compute_handset_geometry(self.active_model)
            self.handset_area.set_content_width(lw)
            self.handset_area.set_content_height(lh)
            self.handset_area.set_halign(Gtk.Align.CENTER)
            self.handset_area.set_valign(Gtk.Align.CENTER)
            self.handset_area.set_margin_start(16)
            self.handset_area.set_margin_end(16)
            self.handset_area.set_margin_top(10)
            self.handset_area.set_margin_bottom(10)
            self.handset_area.set_draw_func(self._draw_handset)

            # Pointer hit-testing controllers on Cairo handset
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

            # --- Right Pane: Device · Files · Wire · GIF ---
            dev_page = self._build_device_page()
            files_page = self._build_files_page()
            wire_page = self._build_wire_page()
            gif_page = self._build_gif_page()

            if has_adw:
                self.dock_stack.add_titled_with_icon(dev_page, "device", "Device", "computer-symbolic")
                self.dock_stack.add_titled_with_icon(files_page, "files", "Files", "folder-symbolic")
                self.dock_stack.add_titled_with_icon(wire_page, "wire", "Wire", "utilities-terminal-symbolic")
                self.dock_stack.add_titled_with_icon(gif_page, "gif", "GIF", "video-x-generic-symbolic")
            else:
                self.dock_stack.add_titled(dev_page, "device", "Device")
                self.dock_stack.add_titled(files_page, "files", "Files")
                self.dock_stack.add_titled(wire_page, "wire", "Wire")
                self.dock_stack.add_titled(gif_page, "gif", "GIF")
            self.dock_stack.set_hexpand(True)
            self.dock_stack.set_vexpand(True)

            self.right_pane = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL)
            self.right_pane.set_hexpand(True)
            self.right_pane.set_size_request(440, -1)
            self.right_pane.append(Gtk.Separator(orientation=Gtk.Orientation.VERTICAL))
            self.right_pane.append(self.dock_stack)
            self.pane_toggle.connect("toggled", lambda b: self.right_pane.set_visible(b.get_active()))

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
            self._refresh_port_dropdown(select_port=initial_port)
            step_ghost_buffer(self.ghost_buf, self.link.last_fb, self.lcd_st)

            if initial_port:
                GLib.timeout_add(80, self._auto_connect_once)
            elif any(p.is_quansheng for p in self.port_monitor.ports):
                # Auto-connect immediately if a Quansheng USB radio is already plugged in!
                GLib.timeout_add(120, self._auto_connect_once)

            self.win.present()

        def _notify(self, msg: str) -> None:
            if self.toast_overlay is not None and has_adw:
                t = Adw.Toast.new(msg)
                t.set_timeout(2)
                self.toast_overlay.add_toast(t)

        # --- Antenna-Style Connection Popover (GUdev Auto-Detection) ---
        def _build_connection_popover(self) -> Any:
            pop = Gtk.Popover()
            box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=10)
            box.set_margin_top(12)
            box.set_margin_bottom(12)
            box.set_margin_start(12)
            box.set_margin_end(12)
            box.set_size_request(290, -1)

            hdr = Gtk.Label(label="<b>Radio Connection</b>", use_markup=True, xalign=0)
            box.append(hdr)

            prow = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=6)
            self.port_strings = Gtk.StringList()
            self.port_dd = Gtk.DropDown(model=self.port_strings)
            self.port_dd.set_hexpand(True)
            prow.append(self.port_dd)

            ref = Gtk.Button.new_from_icon_name("view-refresh-symbolic")
            ref.set_tooltip_text("Rescan USB ports")
            ref.connect("clicked", lambda _b: (self.port_monitor.scan(), self._refresh_port_dropdown()))
            prow.append(ref)
            box.append(prow)

            self.conn_btn = Gtk.Button(label="Connect")
            self.conn_btn.add_css_class("suggested-action")
            self.conn_btn.connect("clicked", self._on_connect_toggle)
            box.append(self.conn_btn)

            box.append(Gtk.Separator())
            self.pop_info_lbl = Gtk.Label(label="No radio connected", xalign=0, wrap=True)
            self.pop_info_lbl.add_css_class("dim-label")
            box.append(self.pop_info_lbl)
            pop.set_child(box)
            return pop

        # --- Display Optical Settings Popover (Squelch-Inspired Controls) ---
        def _build_display_popover(self) -> Any:
            pop = Gtk.Popover()
            box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=10)
            box.set_margin_top(12)
            box.set_margin_bottom(12)
            box.set_margin_start(14)
            box.set_margin_end(14)
            box.set_size_request(250, -1)

            box.append(Gtk.Label(label="LCD Palette", xalign=0))
            names = list(LCD_PALETTES.keys())
            self.pal_dd = Gtk.DropDown.new_from_strings(names)
            self.pal_dd.set_selected(names.index(self.lcd_st.palette))
            self.pal_dd.connect("notify::selected", self._on_palette_changed)
            box.append(self.pal_dd)

            # Backlight slider (0-10)
            box.append(Gtk.Label(label="Backlight", xalign=0))
            bl_s = Gtk.Scale.new_with_range(Gtk.Orientation.HORIZONTAL, 0, 10, 1)
            bl_s.set_draw_value(True)
            bl_s.set_digits(0)
            bl_s.set_value(self.lcd_st.backlight)
            bl_s.connect("value-changed", lambda s: (setattr(self.lcd_st, "backlight", int(s.get_value())), self.handset_area.queue_draw()))
            box.append(bl_s)

            # Contrast slider (0-15)
            box.append(Gtk.Label(label="Contrast", xalign=0))
            ct_s = Gtk.Scale.new_with_range(Gtk.Orientation.HORIZONTAL, 0, 15, 1)
            ct_s.set_draw_value(True)
            ct_s.set_digits(0)
            ct_s.set_value(self.lcd_st.contrast)
            ct_s.connect("value-changed", lambda s: (setattr(self.lcd_st, "contrast", int(s.get_value())), self.handset_area.queue_draw()))
            box.append(ct_s)

            for title, attr in (("STN Ghosting", "ghosting"), ("Pixel Grid", "pixel_grid"), ("Invert LCD", "invert_lcd")):
                r = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
                r.append(Gtk.Label(label=title, xalign=0, hexpand=True))
                sw = Gtk.Switch(active=getattr(self.lcd_st, attr))
                sw.connect("notify::active", lambda w, _p, a=attr: self._toggle_lcd_flag(a, w.get_active()))
                r.append(sw)
                box.append(r)

            pop.set_child(box)
            return pop

        def _toggle_lcd_flag(self, attr: str, val: bool) -> None:
            setattr(self.lcd_st, attr, val)
            step_ghost_buffer(self.ghost_buf, self._current_display_fb(), self.lcd_st)
            self.handset_area.queue_draw()

        def _on_palette_changed(self, dd: Any, _p: Any) -> None:
            names = list(LCD_PALETTES.keys())
            idx = dd.get_selected()
            if 0 <= idx < len(names):
                self.lcd_st.palette = names[idx]
                self.user_picked_palette = True
                self.handset_area.queue_draw()

        # --- Handset Cairo Drawing & Pointer Hit-Testing ---
        def _current_display_state(self) -> tuple[bytes, frozenset[str], str, bool]:
            # When playing back or scrubbing in GIF editor (not recording), preview recorded state on the radio!
            if (not self.recorder.recording) and self.recorder.frames and self.dock_stack.get_visible_child_name() == "gif":
                idx = max(0, min(self._pb_idx, len(self.recorder.frames) - 1))
                rf = self.recorder.frames[idx]
                return rf.fb, rf.pressed, rf.led, rf.torch
            led = "tx" if ("ptt" in self._pressed_keys and self.link.connected) else ("rx" if self.link.connected else "off")
            return self.link.last_fb, frozenset(self._pressed_keys), led, self.link.torch_on

        def _current_display_fb(self) -> bytes:
            return self._current_display_state()[0]

        def _draw_handset(self, _area: Any, cr: Any, w: int, h: int) -> None:
            lw, lh = compute_handset_geometry(self.active_model)
            sx, sy = w / float(lw), h / float(lh)
            scale = min(sx, sy)
            ox = (w - lw * scale) * 0.5
            oy = (h - lh * scale) * 0.5
            cr.save()
            cr.translate(ox, oy)
            cr.scale(scale, scale)
            fb, pressed, led, torch = self._current_display_state()
            draw_handset_cairo(
                cr, self.active_model, fb, self.lcd_st,
                ghost=self.ghost_buf, pressed_keys=pressed, hovered_key=self._hovered_key,
                led_state=led, torch_on=torch, out_hitboxes=self._hitboxes,
            )
            cr.restore()

        def _hit_test(self, x: float, y: float) -> str | None:
            lw, lh = compute_handset_geometry(self.active_model)
            w, h = self.handset_area.get_width(), self.handset_area.get_height()
            scale = min(w / float(lw), h / float(lh))
            ox = (w - lw * scale) * 0.5
            oy = (h - lh * scale) * 0.5
            lx = (x - ox) / scale
            ly = (y - oy) / scale
            for kn, (rx, ry, rw, rh) in self._hitboxes.items():
                if rx <= lx <= rx + rw and ry <= ly <= ry + rh:
                    return kn
            return None

        def _set_hovered_key(self, kn: str | None) -> None:
            if kn != self._hovered_key:
                self._hovered_key = kn
                self.handset_area.queue_draw()

        def _on_handset_motion(self, _ctrl: Any, x: float, y: float) -> None:
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

        # --- Key Press / Hold / Release + Recording Capture ---
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
            self.handset_area.queue_draw()
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
            self.handset_area.queue_draw()
            if was and self.link.connected:
                self.link.send_key(kname, "release")

        def _long_click_key(self, kname: str) -> None:
            if not self.link.connected:
                return
            self._pressed_keys.add(kname)
            self._capture_rec_event()
            self.handset_area.queue_draw()
            self.link.send_key(kname, "press")
            GLib.timeout_add(120, lambda: (self.link.send_key(kname, "hold"), False)[1] if self.link.connected else False)

            def _end() -> bool:
                self._pressed_keys.discard(kname)
                self._capture_rec_event()
                self.handset_area.queue_draw()
                if self.link.connected:
                    self.link.send_key(kname, "release")
                return False

            GLib.timeout_add(260, _end)

        # --- STN Ghosting & Instant Low-Latency Frame Update ---
        def _schedule_frame_update(self) -> None:
            if not self._frame_pending:
                self._frame_pending = True
                GLib.idle_add(self._on_new_frame_main)

        def _on_new_frame_main(self) -> bool:
            self._frame_pending = False
            self._capture_rec_event()
            settling = step_ghost_buffer(self.ghost_buf, self._current_display_fb(), self.lcd_st)
            self.handset_area.queue_draw()
            if settling and self._ghost_timer is None:
                self._ghost_timer = GLib.timeout_add(16, self._tick_ghost)
            return False

        def _tick_ghost(self) -> bool:
            settling = step_ghost_buffer(self.ghost_buf, self._current_display_fb(), self.lcd_st)
            self.handset_area.queue_draw()
            if not settling:
                self._ghost_timer = None
                return False
            return True

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
            return foc is not None and (foc is self.cmd_entry or isinstance(foc, Gtk.Text))

        def _on_pc_key_pressed(self, _c: Any, keyval: int, _kc: int, _st: Any) -> bool:
            if self._text_focused():
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

        # --- Dock Page 1: Device Telemetry & Controls ---
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

        # --- Dock Page 2: KernFS File Browser + WebDAV Mount ---
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
            sp = Gtk.Box(hexpand=True)
            bar.append(sp)
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

        # --- Dock Page 3: RigWire Console ---
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

        # --- Dock Page 4: Non-Destructive GIF Recorder & Trim/Cut/Splice Editor ---
        def _build_gif_page(self) -> Any:
            box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=12)
            box.set_margin_top(12)
            box.set_margin_bottom(12)
            box.set_margin_start(14)
            box.set_margin_end(14)

            self.gif_summary_lbl = Gtk.Label(label="Press Record in the headerbar to capture radio interaction.", xalign=0, wrap=True)
            self.gif_summary_lbl.add_css_class("heading")
            box.append(self.gif_summary_lbl)

            # Interactive Visual Timeline (shows In/Out trim, Red Spliced Cut Segments, Playhead!)
            self.timeline_area = Gtk.DrawingArea()
            self.timeline_area.set_content_height(52)
            self.timeline_area.set_hexpand(True)
            self.timeline_area.set_draw_func(self._draw_timeline)
            tl_click = Gtk.GestureClick()
            tl_click.connect("pressed", self._on_timeline_click)
            self.timeline_area.add_controller(tl_click)
            tl_drag = Gtk.GestureDrag()
            tl_drag.connect("drag-update", self._on_timeline_drag)
            self.timeline_area.add_controller(tl_drag)
            box.append(self.timeline_area)

            # Transport + Trim + Cut/Splice Controls
            tr = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=6)
            self.pb_btn = Gtk.Button.new_from_icon_name("media-playback-start-symbolic")
            self.pb_btn.set_tooltip_text("Play / Pause preview on handset")
            self.pb_btn.connect("clicked", self._on_pb_toggle)
            tr.append(self.pb_btn)

            prev_f = Gtk.Button.new_from_icon_name("go-previous-symbolic")
            prev_f.set_tooltip_text("Step back 1 frame")
            prev_f.connect("clicked", lambda _b: self._step_pb(-1))
            tr.append(prev_f)

            next_f = Gtk.Button.new_from_icon_name("go-next-symbolic")
            next_f.set_tooltip_text("Step forward 1 frame")
            next_f.connect("clicked", lambda _b: self._step_pb(1))
            tr.append(next_f)

            tr.append(Gtk.Separator(orientation=Gtk.Orientation.VERTICAL))

            btn_in = Gtk.Button(label="[ In")
            btn_in.set_tooltip_text("Set trim start to current frame")
            btn_in.connect("clicked", lambda _b: self._set_trim_in())
            tr.append(btn_in)

            btn_out = Gtk.Button(label="Out ]")
            btn_out.set_tooltip_text("Set trim end to current frame")
            btn_out.connect("clicked", lambda _b: self._set_trim_out())
            tr.append(btn_out)

            self.cut_btn = Gtk.Button(label="Mark Cut")
            self.cut_btn.set_tooltip_text("Click once to mark cut start, move playhead, click again to splice out middle segment")
            self.cut_btn.connect("clicked", self._on_cut_splice)
            tr.append(self.cut_btn)

            undo_cut = Gtk.Button.new_from_icon_name("edit-undo-symbolic")
            undo_cut.set_tooltip_text("Undo last cut segment")
            undo_cut.connect("clicked", lambda _b: (self.recorder.undo_cut(), self._refresh_gif_ui()))
            tr.append(undo_cut)
            box.append(tr)

            # Export Options
            opt_grp = Adw.PreferencesGroup(title="Export Settings") if has_adw else Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=8)
            if has_adw:
                r_mode = Adw.ActionRow(title="Capture Scope", subtitle="Export LCD screen only or entire radio with button presses")
                m_grp = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL)
                m_grp.add_css_class("linked")
                m_grp.set_valign(Gtk.Align.CENTER)
                b_lcd = Gtk.ToggleButton(label="LCD Only", active=True)
                b_body = Gtk.ToggleButton(label="Full Radio", group=b_lcd)
                b_lcd.connect("toggled", lambda b: setattr(self, "_export_mode", "lcd") if b.get_active() else None)
                b_body.connect("toggled", lambda b: setattr(self, "_export_mode", "body") if b.get_active() else None)
                m_grp.append(b_lcd)
                m_grp.append(b_body)
                r_mode.add_suffix(m_grp)
                opt_grp.add(r_mode)

                r_spd = Adw.ActionRow(title="Playback Speed", subtitle="Adjust output GIF timing")
                s_grp = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL)
                s_grp.add_css_class("linked")
                s_grp.set_valign(Gtk.Align.CENTER)
                first_s = None
                for lbl, val in (("0.5×", 0.5), ("1×", 1.0), ("1.5×", 1.5), ("2×", 2.0)):
                    sb = Gtk.ToggleButton(label=lbl, active=(val == 1.0))
                    if first_s is None:
                        first_s = sb
                    else:
                        sb.set_group(first_s)
                    sb.connect("toggled", lambda b, v=val: (setattr(self, "_export_speed", v), self._refresh_gif_ui()) if b.get_active() else None)
                    s_grp.append(sb)
                r_spd.add_suffix(s_grp)
                opt_grp.add(r_spd)

                r_cap = Adw.SwitchRow(title="Clamp Idle Pauses", subtitle="Shorten pauses longer than 0.6 s automatically", active=True)
                r_cap.connect("notify::active", lambda sw, _p: (setattr(self, "_cap_idle", sw.get_active()), self._refresh_gif_ui()))
                opt_grp.add(r_cap)
            box.append(opt_grp)

            self.gif_progress = Gtk.ProgressBar(show_text=True)
            self.gif_progress.set_visible(False)
            box.append(self.gif_progress)

            act = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
            clr_btn = Gtk.Button(label="Discard")
            clr_btn.connect("clicked", self._on_discard_recording)
            act.append(clr_btn)
            act.append(Gtk.Box(hexpand=True))
            self.export_gif_btn = Gtk.Button(label="Export GIF…")
            self.export_gif_btn.add_css_class("suggested-action")
            self.export_gif_btn.set_sensitive(False)
            self.export_gif_btn.connect("clicked", self._on_export_gif)
            act.append(self.export_gif_btn)
            box.append(act)
            return box

        # --- Recording & Timeline Editor Logic ---
        def _update_rec_label(self) -> None:
            if self.recorder.recording:
                dt = int(time.monotonic() - self.recorder.t0_mono)
                self.rec_lbl.set_text(f"{dt // 60:02d}:{dt % 60:02d} · {len(self.recorder.frames)}f")
            else:
                self.rec_lbl.set_text("Record")

        def _on_record_toggled(self, btn: Any) -> None:
            if btn.get_active():
                self._stop_playback()
                led = "tx" if ("ptt" in self._pressed_keys and self.link.connected) else ("rx" if self.link.connected else "off")
                self.recorder.start(self.link.last_fb, frozenset(self._pressed_keys), led, self.link.torch_on)
                btn.add_css_class("rec-active")
                self._rec_tick_id = GLib.timeout_add(500, self._tick_rec_clock)
                self._update_rec_label()
            else:
                self.recorder.stop()
                btn.remove_css_class("rec-active")
                self._update_rec_label()
                self._pb_idx = self.recorder.trim_in
                self._refresh_gif_ui()
                if len(self.recorder.frames) > 0:
                    self.dock_stack.set_visible_child_name("gif")
                    self._notify(f"Captured {len(self.recorder.frames)} frames — ready to trim/splice & export")

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

            def fx(i: int) -> float:
                return 8.0 + (i / float(span)) * max(1.0, w - 16.0)

            # Active trimmed window
            x_in, x_out = fx(self.recorder.trim_in), fx(self.recorder.trim_out)
            _round_rect(cr, x_in, 8, max(4.0, x_out - x_in), h - 16, 4)
            cr.set_source_rgba(0.22, 0.55, 0.96, 0.34)
            cr.fill()

            # Spliced-out cut segments in translucent crimson
            for a, b in self.recorder.cuts:
                xa, xb = fx(a), fx(b)
                cr.set_source_rgba(0.92, 0.24, 0.24, 0.65)
                cr.rectangle(xa, 8, max(3.0, xb - xa), h - 16)
                cr.fill()

            # Pending cut mark start
            if self._cut_mark_start is not None:
                xm = fx(self._cut_mark_start)
                cr.set_source_rgb(0.98, 0.72, 0.18)
                cr.set_line_width(2.0)
                cr.move_to(xm, 6)
                cr.line_to(xm, h - 6)
                cr.stroke()

            # Key-event tick marks along bottom edge
            cr.set_source_rgba(0.85, 0.90, 0.96, 0.45)
            for i, rf in enumerate(self.recorder.frames):
                if rf.pressed:
                    xt = fx(i)
                    cr.rectangle(xt - 1.0, h - 14, 2.0, 6)
            cr.fill()

            # Playhead cursor
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
            frac = max(0.0, min(1.0, (x - 8.0) / w))
            self._pb_idx = int(round(frac * (n - 1)))
            self._refresh_gif_ui()

        def _on_timeline_click(self, _g: Any, _n: int, x: float, _y: float) -> None:
            self._drag_start_x = x
            self._seek_timeline_x(x)

        def _on_timeline_drag(self, _g: Any, ox: float, _oy: float) -> None:
            self._seek_timeline_x(getattr(self, "_drag_start_x", 0.0) + ox)

        def _set_trim_in(self) -> None:
            if not self.recorder.frames:
                return
            self.recorder.trim_in = min(self._pb_idx, self.recorder.trim_out)
            self._refresh_gif_ui()

        def _set_trim_out(self) -> None:
            if not self.recorder.frames:
                return
            self.recorder.trim_out = max(self._pb_idx, self.recorder.trim_in)
            self._refresh_gif_ui()

        def _on_cut_splice(self, _btn: Any) -> None:
            if not self.recorder.frames:
                return
            if self._cut_mark_start is None:
                self._cut_mark_start = self._pb_idx
                self.cut_btn.set_label("Splice Out")
                self._refresh_gif_ui()
            else:
                self.recorder.add_cut(self._cut_mark_start, self._pb_idx)
                self._cut_mark_start = None
                self.cut_btn.set_label("Mark Cut")
                self._refresh_gif_ui()

        def _step_pb(self, delta: int) -> None:
            if not self.recorder.frames:
                return
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
            self._schedule_next_pb_frame(seq)

        def _schedule_next_pb_frame(self, seq: list[tuple[int, int]]) -> None:
            if not self._pb_playing or not seq:
                return
            fi, dt_ms = seq[self._pb_seq_pos % len(seq)]
            self._pb_idx = fi
            self._refresh_gif_ui()
            self._pb_seq_pos = (self._pb_seq_pos + 1) % len(seq)
            self._pb_timer = GLib.timeout_add(dt_ms, lambda: (self._schedule_next_pb_frame(seq), False)[1])

        def _refresh_gif_ui(self) -> None:
            n = len(self.recorder.frames)
            seq = self.recorder.frame_durations_ms(speed=self._export_speed, max_idle_ms=600 if self._cap_idle else None)
            tot_ms = sum(d for _, d in seq)
            if n == 0:
                self.gif_summary_lbl.set_text("Press Record in the headerbar to capture radio interaction.")
                self.export_gif_btn.set_sensitive(False)
            else:
                cuts_s = f" · {len(self.recorder.cuts)} cut(s)" if self.recorder.cuts else ""
                self.gif_summary_lbl.set_text(
                    f"Frame {self._pb_idx + 1}/{n} · Kept {len(seq)}f ({tot_ms / 1000.0:.2f} s){cuts_s}"
                )
                self.export_gif_btn.set_sensitive(len(seq) > 0)
            self.timeline_area.queue_draw()
            step_ghost_buffer(self.ghost_buf, self._current_display_fb(), self.lcd_st)
            self.handset_area.queue_draw()

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
                    gf = d.save_finish(res)
                    dest = gf.get_path()
                except GLib.Error:
                    return
                if not dest:
                    return
                self.gif_progress.set_visible(True)
                self.gif_progress.set_fraction(0.0)
                self.export_gif_btn.set_sensitive(False)

                def _worker() -> None:
                    try:
                        sz = self.recorder.export_gif(
                            dest,
                            model=self.active_model,
                            lcd_st=dataclasses.replace(self.lcd_st),
                            mode=self._export_mode,
                            speed=self._export_speed,
                            max_idle_ms=600 if self._cap_idle else None,
                            progress_cb=lambda f: GLib.idle_add(self.gif_progress.set_fraction, f),
                        )
                        GLib.idle_add(self._notify, f"Saved {Path(dest).name} ({sz / 1024.0:.1f} KB)")
                    except Exception as e:
                        GLib.idle_add(self._notify, f"GIF export failed: {e}")
                    finally:
                        GLib.idle_add(self.gif_progress.set_visible, False)
                        GLib.idle_add(self.export_gif_btn.set_sensitive, True)

                threading.Thread(target=_worker, daemon=True).start()

            dlg.save(self.win, None, _picked)

        # --- Connection, Telemetry, Files & Wire Methods ---
        def _refresh_port_dropdown(self, select_port: str | None = None) -> bool:
            ports = list(self.port_monitor.ports)
            if select_port and all(p.device != select_port for p in ports):
                ports.insert(0, DetectedPort(select_port, select_port))
            self._ui_ports = ports
            while self.port_strings.get_n_items() > 0:
                self.port_strings.remove(0)
            for p in ports:
                lbl = f"{p.pretty_name} ({p.device})" if p.pretty_name != p.device and not p.device.startswith("tcp://") and p.device != "demo" else p.pretty_name
                self.port_strings.append(lbl)
            idx = 0
            if select_port:
                for i, p in enumerate(ports):
                    if p.device == select_port:
                        idx = i
                        break
            self.port_dd.set_selected(idx)
            return False

        def _selected_port_obj(self) -> DetectedPort:
            idx = self.port_dd.get_selected()
            if 0 <= idx < len(self._ui_ports):
                return self._ui_ports[idx]
            return DetectedPort("demo", "Offline Demo")

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
                self.hdr_link_icon.set_from_icon_name("network-offline-symbolic")
                self.hdr_batt_icon.set_from_icon_name("battery-missing-symbolic")
                self.hdr_dev_lbl.set_text("Disconnected")
                self.pop_info_lbl.set_text("No radio connected")
                self.mount_btn.set_active(False)
                self.open_fs_btn.set_visible(False)
                self.win.set_title("Kern Dock")
                self._clear_file_list()
                self._refresh_telemetry_ui()
                self.handset_area.queue_draw()
                return

            pobj = self._selected_port_obj()
            try:
                self.link.connect(pobj.device, baud=baud, model_override=self.forced_model, port_info=pobj)
            except Exception as e:
                self._notify(f"Connection failed: {e}")
                return

            self.conn_btn.set_label("Disconnect")
            self.conn_btn.remove_css_class("suggested-action")
            self.conn_btn.add_css_class("destructive-action")
            self.hdr_link_icon.set_from_icon_name("network-transmit-receive-symbolic")
            self.hdr_dev_lbl.set_text(self.link.volume_name.removeprefix("Quansheng "))
            self.pop_info_lbl.set_text(f"{self.link.volume_name}\nPort: {self.link.port}")
            self.win.set_title(f"Kern Dock — {self.link.volume_name}")

            # Auto-adapt physical radio geometry & native LCD palette (zero manual model switcher!)
            if self.link.detected_model != self.active_model:
                self.active_model = self.link.detected_model
                lw, lh = compute_handset_geometry(self.active_model)
                self.handset_area.set_content_width(lw)
                self.handset_area.set_content_height(lh)
                if not self.user_picked_palette:
                    self.lcd_st.palette = "UV-K5" if self.active_model == "k5" else "UV-K1"
                    self.pal_dd.set_selected(list(LCD_PALETTES.keys()).index(self.lcd_st.palette))

            step_ghost_buffer(self.ghost_buf, self.link.last_fb, self.lcd_st)
            self.handset_area.queue_draw()
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
                self.hdr_batt_icon.set_from_icon_name(_battery_icon_name(pct))
                self.hdr_batt_icon.set_tooltip_text(f"Battery: {pct}% ({mv / 1000.0:.2f} V)")

            if has_adw:
                self.row_ident.set_subtitle(f"{self.link.volume_name} ({self.link.port})")
                self.row_serial.set_subtitle(f"{t.get('serial_str', '—')}  ·  {t.get('mac_addr', '—')}")
                mtu = self.link.hello_info.get("accepted_max_payload", 128)
                fps_s = "Demo" if self.link.demo_mode else f"{self.link.fps:.1f} FPS"
                err_s = f" · {self.link.error_count} err" if self.link.error_count else ""
                self.row_link.set_subtitle(f"{fps_s} · RTT {self.link.rtt_ms:.1f} ms · MTU {mtu} B{err_s}")

            self._updating_switches = True
            self.sw_torch.set_active(self.link.torch_on)
            self.sw_lock.set_active(self.link.keypad_locked)
            self._updating_switches = False
            return False

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
            dlg = Gtk.FileDialog(title="Save Screenshot (PNG)")
            dlg.set_initial_name(f"kern_{self.active_model}_{time.strftime('%Y%m%d_%H%M%S')}.png")

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
            end_it = self.console_buf.get_end_iter()
            self.console_buf.insert(end_it, line + "\n")
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
# 7. HEADLESS SELFTEST (CI-Safe, Zero Display Server Required)
# =============================================================================

def run_selftest() -> int:
    import tempfile

    # 1. Verify exact UV-K1 and UV-K5 physical keymaps match RigWire KEY_CODES
    k1_keys = [k[0] for row in K1_MATRIX for k in row]
    k5_keys = ["menu", "up", "down", "exit"] + [k[0] for row in K5_BOTTOM_MATRIX for k in row]
    assert len(k1_keys) == 16 and len(k5_keys) == 16
    for kn in set(k1_keys + k5_keys):
        assert kn in rigwire.KEY_CODES, f"Invalid RigWire key '{kn}'"

    # 2. Verify hardware auto-detection
    assert detect_hardware_model("Quansheng UV-K1 1D9E") == "k1"
    assert detect_hardware_model("Quansheng UV-K5 v3 ABCD") == "k5"
    assert detect_hardware_model("Quansheng UV-K6 99AA") == "k5"

    # 3. Verify STN ghosting physics + RecordingSession trim/cut/splice timeline
    fb1 = make_demo_framebuffer("433.500", "145.500", "VFO")
    fb2 = make_demo_framebuffer("433.525", "145.500", "UP")
    fb3 = make_demo_framebuffer("433.550", "145.500", "UP")
    st_k1 = LcdSettings(palette="UV-K1")
    ghost = [0.0] * (LCD_W * LCD_H)
    assert step_ghost_buffer(ghost, fb1, st_k1) is True

    rec = RecordingSession()
    rec.frames = [
        RecordedFrame(0, fb1, frozenset(), "rx", False),
        RecordedFrame(150, fb2, frozenset({"up"}), "rx", False),
        RecordedFrame(450, fb2, frozenset(), "rx", False),   # will be spliced out
        RecordedFrame(950, fb3, frozenset({"up"}), "rx", False),
        RecordedFrame(1100, fb3, frozenset(), "rx", False),
    ]
    rec.trim_in, rec.trim_out = 0, 4
    rec.add_cut(2, 2)
    assert rec.active_indices() == [0, 1, 3, 4]
    seq = rec.frame_durations_ms(speed=1.0, max_idle_ms=600)
    assert len(seq) == 4 and all(dt >= 20 for _, dt in seq)

    # 3b. When pycairo & Pillow are installed (desktop/dev), verify full Cairo handset & GIF exports
    try:
        import cairo as _cairo  # noqa: F401
        has_cairo = True
    except ImportError:
        has_cairo = False

    if has_cairo:
        w, h, rgba = render_lcd_to_rgba(fb1, st_k1, width=384, height=266)
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

    # 4. Verify DockLink demo + FS browser + Wire console + WebDAV volume identity
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
        "All Kern Dock selftests passed (UV-K1 & UV-K5 CAD Cairo models, ST7565 optics, auto-detect, GIF trim/splice LCD+Body export, FS/Wire/WebDAV)."
    )
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description="Kern Dock — Native GTK4 + Libadwaita radio companion")
    ap.add_argument("-p", "--port", help="Serial port (/dev/ttyACM0, COM3, tcp://127.0.0.1:4446, or 'demo')")
    ap.add_argument("-b", "--baud", type=int, default=115200, help="Serial baud rate (default: 115200)")
    ap.add_argument("--model", choices=["k1", "k5"], help="Override auto-detected radio chassis (k1 or k5)")
    ap.add_argument("--palette", choices=list(LCD_PALETTES.keys()), help="Override LCD color palette")
    ap.add_argument("--demo", action="store_true", help="Launch immediately in offline demo mode")
    ap.add_argument("--selftest", action="store_true", help="Run headless selftest")
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
