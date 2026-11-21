"""ST7565 128x64 LCD engine: vectorized NumPy STN ghosting plus cached Cairo glass layers."""

from __future__ import annotations

import dataclasses
import math
from typing import Any

try:
    import numpy as _np
except ImportError:
    _np = None  # type: ignore[assignment]

from .paint import clamp1, round_rect

LCD_W = 128
LCD_H = 64

LCD_PALETTES: dict[str, dict[str, Any]] = {
    "UV-K1":         {"bg": (0xC9, 0xF7, 0xEC), "fg": (0x24, 0x1F, 0x7E), "dark": False},
    "Ice White":     {"bg": (0xE6, 0xF0, 0xF2), "fg": (0x14, 0x18, 0x20), "dark": False},
    "UV-K5":         {"bg": (0xFF, 0x99, 0x08), "fg": (0x12, 0x10, 0x0E), "dark": False},
    "UV-5R Blue":    {"bg": (0x1E, 0x72, 0xF8), "fg": (0x08, 0x0E, 0x1C), "dark": False},
    "K10 Warm":      {"bg": (0xF6, 0xDF, 0x76), "fg": (0x14, 0x14, 0x10), "dark": False},
    "Classic Green": {"bg": (0x9C, 0xA9, 0x88), "fg": (0x24, 0x28, 0x22), "dark": False},
    "OLED White":    {"bg": (0x10, 0x11, 0x14), "fg": (0xF2, 0xF5, 0xFA), "dark": True},
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


def _lcd_colors_norm(
    st: LcdSettings,
) -> tuple[tuple[float, float, float], tuple[float, float, float], tuple[float, float, float], bool]:
    pal = LCD_PALETTES.get(st.palette, LCD_PALETTES["UV-K1"])
    is_dark = bool(pal["dark"])
    if st.backlight <= 0 and not is_dark:
        bg = (0.125, 0.138, 0.132)
        fg_raw = (0.035, 0.040, 0.042)
    else:
        bright = _backlight_mult(st.backlight)
        if not is_dark and st.backlight <= 2:
            t = st.backlight / 2.0
            led_bg = tuple(clamp1((c / 255.0) * bright) for c in pal["bg"])
            bg = (
                0.125 * (1.0 - t) + led_bg[0] * t,
                0.138 * (1.0 - t) + led_bg[1] * t,
                0.132 * (1.0 - t) + led_bg[2] * t,
            )
        else:
            bg = tuple(clamp1((c / 255.0) * bright) for c in pal["bg"])
        fg_raw = tuple(clamp1((c / 255.0) * bright) for c in pal["fg"])
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
    return bg, fg, grid, is_dark


class FastLcdState:
    """Vectorized ST7565 pixel state emitting a fresh GSK-safe 128x64 Cairo ARGB32 surface."""

    def __init__(self) -> None:
        if _np is not None:
            self._ghost_np = _np.zeros((LCD_H, LCD_W), dtype=_np.float32)
            self._shifts = _np.arange(8, dtype=_np.uint8).reshape(8, 1)
            self._bgra_buf = bytearray(LCD_W * LCD_H * 4)
            self._bgra_np = _np.frombuffer(self._bgra_buf, dtype=_np.uint8).reshape(LCD_H, LCD_W, 4)
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
            return cairo.ImageSurface.create_for_data(self._bgra_buf, cairo.FORMAT_ARGB32, LCD_W, LCD_H, LCD_W * 4)

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
    """Supersampled (2x) cached LCD optics + moire-free STN pixel-matrix grid."""

    def __init__(self) -> None:
        self._key: Any = None
        self.bg_surf: Any = None
        self.fg_surf: Any = None
        self._px3_surf: Any = None
        self.ss: int = 2
        self.metrics: tuple[float, float, float, float, float, float] = (0, 0, 1, 1, 1, 1)

    def ensure(self, w: int, h: int, st: LcdSettings, dev_scale: float = 1.0) -> None:
        import cairo

        ds_bucket = round(max(1.0, min(3.0, dev_scale)) * 2.0) / 2.0
        key = (w, h, ds_bucket, st.cache_key())
        if key == self._key and self.bg_surf is not None:
            return
        self._key = key
        ss = 2
        self.ss = ss
        bg, fg, grid, is_dark = _lcd_colors_norm(st)
        mx, my, mat_w, mat_h, px_w, px_h = _compute_lcd_active_rect(float(w), float(h), st.pixel_aspect)
        self.metrics = (mx, my, mat_w, mat_h, px_w, px_h)

        bg_s = cairo.ImageSurface(cairo.FORMAT_ARGB32, w * ss, h * ss)
        cr = cairo.Context(bg_s)
        cr.scale(ss, ss)
        round_rect(cr, 0, 0, w, h, 4.0)
        cr.clip()

        pat = cairo.LinearGradient(0, 0, 0, h)
        if is_dark:
            pat.add_color_stop_rgb(0.0, bg[0], bg[1], bg[2])
            pat.add_color_stop_rgb(1.0, clamp1(bg[0] * 1.08), clamp1(bg[1] * 1.08), clamp1(bg[2] * 1.08))
        elif st.backlight <= 0:
            pat.add_color_stop_rgb(0.0, bg[0] * 0.90, bg[1] * 0.90, bg[2] * 0.90)
            pat.add_color_stop_rgb(0.55, bg[0], bg[1], bg[2])
            pat.add_color_stop_rgb(1.0, bg[0] * 1.06, bg[1] * 1.06, bg[2] * 1.06)
        else:
            pat.add_color_stop_rgb(0.0, bg[0] * 0.93, bg[1] * 0.94, bg[2] * 0.95)
            pat.add_color_stop_rgb(0.50, bg[0], bg[1], bg[2])
            pat.add_color_stop_rgb(1.0, clamp1(bg[0] * 1.05), clamp1(bg[1] * 1.04), clamp1(bg[2] * 1.02))
        cr.set_source(pat)
        cr.rectangle(0, 0, w, h)
        cr.fill()

        if not is_dark and st.backlight > 0:
            hdiff = cairo.LinearGradient(0, 0, w, 0)
            hdiff.add_color_stop_rgba(0.0, 1.0, 0.98, 0.92, 0.11)
            hdiff.add_color_stop_rgba(0.18, 1.0, 0.98, 0.92, 0.0)
            hdiff.add_color_stop_rgba(0.82, 1.0, 0.98, 0.92, 0.0)
            hdiff.add_color_stop_rgba(1.0, 1.0, 0.98, 0.92, 0.11)
            cr.set_source(hdiff)
            cr.rectangle(0, 0, w, h)
            cr.fill()

        if not is_dark:
            cr.set_source_rgba(grid[0], grid[1], grid[2], 0.36 if st.pixel_grid else 0.16)
            cr.rectangle(mx, my, mat_w, mat_h)
            cr.fill()
        self.bg_surf = bg_s

        fg_s = cairo.ImageSurface(cairo.FORMAT_ARGB32, w * ss, h * ss)
        fcr = cairo.Context(fg_s)
        fcr.scale(ss, ss)
        round_rect(fcr, 0, 0, w, h, 4.0)
        fcr.clip()

        eff_px_w = px_w * ds_bucket
        grid_alpha = clamp1((eff_px_w - 0.95) / 1.35)
        if st.pixel_grid and grid_alpha > 0.08:
            gs = cairo.ImageSurface(cairo.FORMAT_ARGB32, LCD_W * 6, LCD_H * 6)
            gcr = cairo.Context(gs)
            gcr.set_antialias(cairo.ANTIALIAS_NONE)
            if not is_dark and st.backlight > 0:
                gcr.set_source_rgba(fg[0], fg[1], fg[2], 0.028)
                gcr.rectangle(0, 0, LCD_W * 6, LCD_H * 6)
                gcr.fill()
            gcr.set_operator(cairo.OPERATOR_SOURCE)
            gcr.set_source_rgba(bg[0], bg[1], bg[2], 0.42 if not is_dark else 0.50)
            for col in range(1, LCD_W):
                gcr.rectangle(col * 6 - 1, 0, 1, LCD_H * 6)
            for row in range(1, LCD_H):
                gcr.rectangle(0, row * 6 - 1, LCD_W * 6, 1)
            gcr.fill()
            fcr.save()
            fcr.translate(mx, my)
            fcr.scale(mat_w / float(LCD_W * 6), mat_h / float(LCD_H * 6))
            fcr.set_source_surface(gs, 0, 0)
            fcr.get_source().set_filter(cairo.FILTER_BEST)
            fcr.paint_with_alpha(grid_alpha)
            fcr.restore()

        sd = max(5.5, min(w, h) * 0.052)
        for (x0, y0, x1, y1, rx, ry, rw, rh, a0) in (
            (0, 0, 0, sd, 0, 0, w, sd, 0.34),
            (0, h, 0, h - sd * 0.65, 0, h - sd * 0.65, w, sd * 0.65, 0.22),
            (0, 0, sd * 0.85, 0, 0, 0, sd * 0.85, h, 0.28),
            (w, 0, w - sd * 0.85, 0, w - sd * 0.85, 0, sd * 0.85, h, 0.26),
        ):
            sg = cairo.LinearGradient(x0, y0, x1, y1)
            sg.add_color_stop_rgba(0.0, 0.0, 0.0, 0.0, a0)
            sg.add_color_stop_rgba(1.0, 0.0, 0.0, 0.0, 0.0)
            fcr.set_source(sg)
            fcr.rectangle(rx, ry, rw, rh)
            fcr.fill()

        sheen = cairo.LinearGradient(0, 0, w * 0.56, h * 0.74)
        sheen.add_color_stop_rgba(0.0, 1.0, 1.0, 1.0, 0.09 if st.backlight > 0 else 0.12)
        sheen.add_color_stop_rgba(0.38, 1.0, 1.0, 1.0, 0.02)
        sheen.add_color_stop_rgba(0.40, 1.0, 1.0, 1.0, 0.0)
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
    import cairo

    ctm = cr.get_matrix()
    dev_scale = max(1.0, math.hypot(ctm.xx, ctm.yx))
    cache.ensure(w, h, st, dev_scale=dev_scale)
    mx, my, mat_w, mat_h, px_w, px_h = cache.metrics
    _, _, _, is_dark = _lcd_colors_norm(st)
    px_surf = lcd_state.get_cairo_pixel_surface(st)

    if cache._px3_surf is None:
        cache._px3_surf = cairo.ImageSurface(cairo.FORMAT_ARGB32, LCD_W * 2, LCD_H * 2)
    p2_cr = cairo.Context(cache._px3_surf)
    p2_cr.set_operator(cairo.OPERATOR_SOURCE)
    p2_cr.scale(2.0, 2.0)
    p2_cr.set_source_surface(px_surf, 0, 0)
    p2_cr.get_source().set_filter(cairo.FILTER_NEAREST)
    p2_cr.paint()

    ss_inv = 1.0 / float(cache.ss)
    cr.save()
    cr.translate(x, y)

    cr.save()
    cr.scale(ss_inv, ss_inv)
    cr.set_source_surface(cache.bg_surf, 0, 0)
    cr.get_source().set_filter(cairo.FILTER_GOOD)
    cr.paint()
    cr.restore()

    if not is_dark and st.backlight > 0:
        cr.save()
        cr.translate(mx + max(0.50, px_w * 0.26), my + max(0.60, px_h * 0.26))
        cr.scale(mat_w / float(LCD_W * 2), mat_h / float(LCD_H * 2))
        cr.set_source_surface(cache._px3_surf, 0, 0)
        cr.get_source().set_filter(cairo.FILTER_GOOD)
        cr.paint_with_alpha(0.14)
        cr.restore()

    cr.save()
    cr.translate(mx, my)
    cr.scale(mat_w / float(LCD_W * 2), mat_h / float(LCD_H * 2))
    cr.set_source_surface(cache._px3_surf, 0, 0)
    cr.get_source().set_filter(cairo.FILTER_GOOD)
    cr.paint()
    cr.restore()

    cr.save()
    cr.scale(ss_inv, ss_inv)
    cr.set_source_surface(cache.fg_surf, 0, 0)
    cr.get_source().set_filter(cairo.FILTER_GOOD)
    cr.paint()
    cr.restore()

    cr.restore()
