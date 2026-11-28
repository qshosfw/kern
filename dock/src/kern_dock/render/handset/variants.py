"""Photorealistic GNOME-styled Cairo builders for real Quansheng handset families.

Each variant has its own sculpted body silhouette, authentic ST7565/1.44" display aspect ratio,
distinct acoustic grille, molded bevels, and accurate factory keypad legend:
  • UV-K1(8) Transparent ("Mini Kong") — clear polycarbonate shell, visible green PCB glow, blue V-notch knob
  • UV-K5 (Diagonal Grille) — angular matte-black shell, octagonal speaker island, angled M/EXIT keys
  • UV-K5(99) Charcoal & Red — flared cheek-ribbed armor, stepped narrow keypad skirt, 4-slat speaker
  • Quansheng UV-5R Plus — all-black knob, cobalt-blue LCD, orange M / blue EXIT, 6×7 dot-matrix grille
  • Quansheng UV-K10 Pro — slate-blue exoskeleton, cyan antenna collar, dual top knobs, titanium fascia, A/B V/M Band
"""

from __future__ import annotations

import math
from typing import Any

from ..lcd import LcdSettings, _lcd_colors_norm
from ..paint import clamp1, paint_brushed_metal_grain, round_rect
from .catalog import K1_MATRIX, K5_99_MATRIX, K5_BOTTOM_MATRIX, K10_PRO_MATRIX
from .parts import (
    draw_k1_side_shroud,
    draw_k5_side_shroud,
    draw_k10_logo_badge,
    draw_pango_text,
    draw_quansheng_logo,
)
from .spec import HandsetSpec


def _draw_tapered_antenna(
    cr: Any,
    ax: float,
    by: float,
    *,
    stubby: bool = False,
    label: str = "",
    cyan_collar: bool = False,
) -> None:
    import cairo

    pat_ant = cairo.LinearGradient(ax - 24, 0, ax + 24, 0)
    pat_ant.add_color_stop_rgb(0.0, 0.07, 0.075, 0.085)
    pat_ant.add_color_stop_rgb(0.30, 0.28, 0.29, 0.31)
    pat_ant.add_color_stop_rgb(0.62, 0.16, 0.17, 0.19)
    pat_ant.add_color_stop_rgb(1.0, 0.05, 0.055, 0.065)
    cr.set_source(pat_ant)
    if stubby:
        round_rect(cr, ax - 20.5, 2.0, 41.0, by + 4.0, 5.0)
        cr.fill()
        round_rect(cr, ax - 23.5, by - 20.0, 47.0, 24.0, 4.0)
        cr.fill()
        cr.set_source_rgba(0.0, 0.0, 0.0, 0.38)
        cr.set_line_width(1.0)
        for ry in (by - 15.0, by - 9.0):
            cr.move_to(ax - 22.5, ry)
            cr.line_to(ax + 22.5, ry)
            cr.stroke()
        round_rect(cr, ax - 9.5, 5.0, 19.0, 22.0, 4.0)
        cr.set_source_rgba(0.03, 0.035, 0.045, 0.56)
        cr.fill()
        if label:
            draw_pango_text(cr, label, ax, 16.0, font_desc="Adwaita Sans Bold 8.2", rgba=(0.74, 0.76, 0.80, 0.54), halign="center")
    else:
        cr.new_path()
        cr.move_to(ax - 23.5, by + 4.0)
        cr.line_to(ax - 19.5, 2.0)
        cr.line_to(ax + 19.5, 2.0)
        cr.line_to(ax + 23.5, by + 4.0)
        cr.close_path()
        cr.fill()
        if cyan_collar:
            round_rect(cr, ax - 24.5, by - 13.5, 49.0, 15.0, 3.0)
            pat_c = cairo.LinearGradient(ax - 24.5, 0, ax + 24.5, 0)
            pat_c.add_color_stop_rgb(0.0, 0.05, 0.44, 0.70)
            pat_c.add_color_stop_rgb(0.36, 0.22, 0.82, 0.99)
            pat_c.add_color_stop_rgb(0.70, 0.10, 0.60, 0.86)
            pat_c.add_color_stop_rgb(1.0, 0.04, 0.36, 0.60)
            cr.set_source(pat_c)
            cr.fill_preserve()
            cr.set_source_rgba(1.0, 1.0, 1.0, 0.24)
            cr.set_line_width(0.8)
            cr.stroke()


def _draw_v_notch_knob(
    cr: Any,
    kx: float,
    by: float,
    accent: tuple[float, float, float] | None,
    *,
    width: float = 60.0,
) -> None:
    """Authentic Quansheng rotary knob with V-notch cap (orange, blue, or all-black)."""
    import cairo

    hw = width * 0.5
    pat_kb = cairo.LinearGradient(kx - hw, 0, kx + hw, 0)
    pat_kb.add_color_stop_rgb(0.0, 0.06, 0.07, 0.08)
    pat_kb.add_color_stop_rgb(0.30, 0.26, 0.27, 0.29)
    pat_kb.add_color_stop_rgb(0.65, 0.15, 0.16, 0.18)
    pat_kb.add_color_stop_rgb(1.0, 0.05, 0.06, 0.07)
    cr.set_source(pat_kb)
    round_rect(cr, kx - hw, by - 31.0, width, 35.0, 6.0)
    cr.fill()

    if accent is not None:
        ar, ag, ab = accent
        pat_o = cairo.LinearGradient(kx - hw, by - 42.0, kx + hw, by - 22.0)
        pat_o.add_color_stop_rgb(0.0, ar * 0.86, ag * 0.74, ab * 0.70)
        pat_o.add_color_stop_rgb(0.42, clamp1(ar * 1.12), clamp1(ag * 1.14), clamp1(ab * 1.18))
        pat_o.add_color_stop_rgb(1.0, ar * 0.72, ag * 0.58, ab * 0.55)
        cr.set_source(pat_o)
        round_rect(cr, kx - hw + 1.0, by - 42.0, width - 2.0, 12.5, 5.0)
        cr.fill()
        cr.new_path()
        cr.move_to(kx - 7.5, by - 31.5)
        cr.line_to(kx + 7.5, by - 31.5)
        cr.line_to(kx + 3.2, by - 15.0)
        cr.line_to(kx - 3.2, by - 15.0)
        cr.close_path()
        cr.fill()
        round_rect(cr, kx - hw + 4.5, by - 26.5, 6.5, 11.0, 2.2)
        round_rect(cr, kx + hw - 11.0, by - 26.5, 6.5, 11.0, 2.2)
        cr.fill()
    else:
        # Classic all-black UV-5R knurled rotary knob
        round_rect(cr, kx - hw + 2.0, by - 39.0, width - 4.0, 10.0, 4.5)
        cr.set_source(pat_kb)
        cr.fill_preserve()
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.14)
        cr.set_line_width(0.8)
        cr.stroke()
        cr.set_source_rgba(0.02, 0.02, 0.03, 0.72)
        for dx in (-20.0, -12.0, -4.0, 4.0, 12.0, 20.0):
            round_rect(cr, kx + dx - 2.0, by - 28.0, 4.0, 22.0, 1.5)
            cr.fill()


def _draw_right_shoulder_ear(cr: Any, bx: float, by: float, bw: float, rgb: tuple[float, float, float]) -> None:
    import cairo

    cr.new_path()
    cr.move_to(bx + bw - 22.0, by + 8.0)
    cr.line_to(bx + bw - 22.0, by - 13.0)
    cr.curve_to(bx + bw - 13.0, by - 15.0, bx + bw - 10.0, by - 35.0, bx + bw - 6.0, by - 40.0)
    cr.line_to(bx + bw + 1.5, by - 40.0)
    cr.curve_to(bx + bw + 4.5, by - 38.0, bx + bw + 4.0, by - 14.0, bx + bw + 2.0, by + 10.0)
    cr.close_path()
    pat = cairo.LinearGradient(bx + bw - 22.0, by - 40.0, bx + bw + 4.0, by + 10.0)
    pat.add_color_stop_rgb(0.0, clamp1(rgb[0] * 1.45), clamp1(rgb[1] * 1.45), clamp1(rgb[2] * 1.45))
    pat.add_color_stop_rgb(0.55, rgb[0], rgb[1], rgb[2])
    pat.add_color_stop_rgb(1.0, rgb[0] * 0.62, rgb[1] * 0.62, rgb[2] * 0.62)
    cr.set_source(pat)
    cr.fill_preserve()
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.14)
    cr.set_line_width(0.85)
    cr.stroke()


def _draw_lcd_bezel(
    cr: Any,
    lz_x: int,
    lz_y: int,
    lz_w: int,
    lz_h: int,
    lcd_st: LcdSettings,
    *,
    radius: float = 7.0,
    chamfer: float = 5.0,
    glow_alpha: float = 0.20,
) -> tuple[int, int, int, int]:
    """Draw a recessed multi-layer GNOME-grade LCD bezel and return the inner pixel rect."""
    import cairo

    round_rect(cr, lz_x - chamfer, lz_y - chamfer, lz_w + chamfer * 2, lz_h + chamfer * 2, radius + 3.0)
    pat_ch = cairo.LinearGradient(lz_x, lz_y - chamfer, lz_x, lz_y + lz_h + chamfer)
    pat_ch.add_color_stop_rgba(0.0, 0.0, 0.0, 0.0, 0.56)
    pat_ch.add_color_stop_rgba(0.5, 0.0, 0.0, 0.0, 0.16)
    pat_ch.add_color_stop_rgba(1.0, 1.0, 1.0, 1.0, 0.18)
    cr.set_source(pat_ch)
    cr.fill()

    round_rect(cr, lz_x, lz_y, lz_w, lz_h, radius)
    cr.set_source_rgb(0.045, 0.05, 0.06)
    cr.fill_preserve()
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.18)
    cr.set_line_width(0.9)
    cr.stroke()

    if lcd_st.backlight_glow and lcd_st.backlight > 0:
        bg_c, _, _, _ = _lcd_colors_norm(lcd_st)
        cr.set_source_rgba(bg_c[0], bg_c[1], bg_c[2], glow_alpha)
        round_rect(cr, lz_x + 4, lz_y + 4, lz_w - 8, lz_h - 8, max(3.0, radius - 2.0))
        cr.set_line_width(2.2)
        cr.stroke()

    return (lz_x + 6, lz_y + 6, lz_w - 12, lz_h - 12)


# =============================================================================
# 1. Quansheng UV-K1 Mini-Kong (Transparent & Matte Black Polymer — `media_1795271390597`)
# =============================================================================
def build_k1_clear(
    rend: Any, back_cr: Any, cr: Any, W: int, H: int, lcd_st: LcdSettings, spec: HandsetSpec, *, transparent: bool = True
) -> None:
    """UV-K1 'Mini Kong' mold: either crystal-clear transparent polycarbonate or opaque matte-black polymer."""
    import cairo

    bx, by, bw, bh = 32.0, 58.0, W - 48.0, H - 70.0
    rend._body_bx = bx
    rend._side_specs = [
        ("ptt", 9.0, by + 56.0, 26.0, 114.0, True, 0),
        ("side1", 12.0, by + 180.0, 23.0, 35.0, False, 2),
        ("side2", 12.0, by + 223.0, 23.0, 35.0, False, 1),
    ]
    for kn, sx, sy, sw, sh, _p, _d in rend._side_specs:
        rend.hitboxes[kn] = (sx - 3.0, sy, sw + 4.0, sh)

    # Drop shadow + left side shroud + right port cover
    round_rect(back_cr, bx + 3, by + 10, bw, bh, 17.0)
    back_cr.set_source_rgba(0.0, 0.0, 0.0, 0.42)
    back_cr.fill()
    draw_k1_side_shroud(back_cr, bx, by, bh, rend._side_specs)

    round_rect(back_cr, bx + bw - 6.0, by + 50.0, 12.0, bh - 116.0, 5.5)
    pat_rc = cairo.LinearGradient(bx + bw - 6.0, 0, bx + bw + 6.0, 0)
    if transparent:
        pat_rc.add_color_stop_rgb(0.0, 0.44, 0.48, 0.52)
        pat_rc.add_color_stop_rgb(0.60, 0.78, 0.82, 0.85)
        pat_rc.add_color_stop_rgb(1.0, 0.38, 0.42, 0.46)
    else:
        pat_rc.add_color_stop_rgb(0.0, 0.05, 0.055, 0.065)
        pat_rc.add_color_stop_rgb(0.60, 0.16, 0.17, 0.19)
        pat_rc.add_color_stop_rgb(1.0, 0.06, 0.065, 0.075)
    back_cr.set_source(pat_rc)
    back_cr.fill()

    # Stubby ribbed A1 antenna + blue V-notch Mini Kong rotary knob
    ax = bx + 54.0
    _draw_tapered_antenna(cr, ax, by, stubby=True, label="A1")
    _draw_v_notch_knob(cr, bx + bw - 56.0, by, spec.accent, width=60.0)

    tx = bx + bw * 0.51
    rend._torch_anchor = (tx, by, False)
    rend.hitboxes["_torch"] = (tx - 19.0, by - 16.0, 38.0, 20.0)
    rend._led_anchor = (bx + 84.0, by - 8.5, 22.0, 10.0, True)

    # Molded Mini Kong body (crystal-clear polycarbonate OR opaque matte-black polymer)
    round_rect(cr, bx, by, bw, bh, 16.0)
    pat_cl = cairo.LinearGradient(bx, by, bx + bw, by)
    if transparent:
        pat_cl.add_color_stop_rgb(0.00, 0.90, 0.93, 0.95)
        pat_cl.add_color_stop_rgb(0.06, 0.66, 0.70, 0.74)
        pat_cl.add_color_stop_rgb(0.14, 0.76, 0.80, 0.83)
        pat_cl.add_color_stop_rgb(0.50, 0.60, 0.64, 0.68)
        pat_cl.add_color_stop_rgb(0.86, 0.74, 0.78, 0.82)
        pat_cl.add_color_stop_rgb(0.94, 0.64, 0.68, 0.72)
        pat_cl.add_color_stop_rgb(1.00, 0.90, 0.93, 0.95)
        cr.set_source(pat_cl)
        cr.fill_preserve()
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.62)
        cr.set_line_width(1.4)
        cr.stroke()
    else:
        pat_cl.add_color_stop_rgb(0.00, 0.22, 0.23, 0.25)
        pat_cl.add_color_stop_rgb(0.08, 0.115, 0.12, 0.135)
        pat_cl.add_color_stop_rgb(0.50, 0.095, 0.10, 0.112)
        pat_cl.add_color_stop_rgb(0.92, 0.115, 0.12, 0.135)
        pat_cl.add_color_stop_rgb(1.00, 0.21, 0.22, 0.24)
        cr.set_source(pat_cl)
        cr.fill_preserve()
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.12)
        cr.set_line_width(1.1)
        cr.stroke()

    if transparent:
        # Inner dark polycarbonate wall thickness refraction frame + visible internal ribs
        cr.save()
        round_rect(cr, bx + 5.5, by + 5.5, bw - 11.0, bh - 11.0, 12.0)
        cr.set_source_rgba(0.18, 0.22, 0.26, 0.34)
        cr.fill_preserve()
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.45)
        cr.set_line_width(1.0)
        cr.stroke()

        # Internal screw standoffs peeking through clear corners
        for cx_b, cy_b in (
            (bx + 18.0, by + 24.0),
            (bx + bw - 18.0, by + 24.0),
            (bx + 18.0, by + bh - 26.0),
            (bx + bw - 18.0, by + bh - 26.0),
        ):
            cr.new_path()
            cr.arc(cx_b, cy_b, 6.2, 0, 2 * math.pi)
            cr.set_source_rgba(0.24, 0.28, 0.32, 0.55)
            cr.fill()
            cr.new_path()
            cr.arc(cx_b, cy_b, 3.0, 0, 2 * math.pi)
            cr.set_source_rgba(0.75, 0.78, 0.82, 0.70)
            cr.fill()

        # Signature vivid emerald-green internal PCB glow radiating around LCD & top bezel!
        pat_g1 = cairo.RadialGradient(bx + bw * 0.50, by + 115.0, 38.0, bx + bw * 0.50, by + 135.0, 142.0)
        pat_g1.add_color_stop_rgba(0.0, 0.12, 0.88, 0.28, 0.78)
        pat_g1.add_color_stop_rgba(0.62, 0.10, 0.72, 0.24, 0.46)
        pat_g1.add_color_stop_rgba(1.0, 0.12, 0.48, 0.20, 0.0)
        cr.set_source(pat_g1)
        round_rect(cr, bx + 12.0, by + 8.0, bw - 24.0, 240.0, 10.0)
        cr.fill()

        round_rect(cr, bx + 26.0, by + 4.0, bw - 52.0, 12.0, 4.0)
        cr.set_source_rgba(0.14, 0.88, 0.28, 0.52)
        cr.fill()

        for rx, rw in ((bx + 9.0, 5.5), (bx + bw - 14.5, 5.5)):
            round_rect(cr, rx, by + 34.0, rw, bh - 70.0, 2.5)
            cr.set_source_rgba(1.0, 1.0, 1.0, 0.34)
            cr.fill()
        cr.restore()
    else:
        # Opaque matte-black Mini Kong molded inner fascia bevel & vertical side grip steps
        round_rect(cr, bx + 9.0, by + 8.0, bw - 18.0, bh - 16.0, 12.0)
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.05)
        cr.fill_preserve()
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.08)
        cr.set_line_width(0.9)
        cr.stroke()

    # Glossy black trapezoidal QUANSHENG nameplate
    np_x, np_y, np_w, np_h = bx + 42.0, by + 11.0, bw - 84.0, 28.0
    round_rect(cr, np_x, np_y, np_w, np_h, 4.0)
    cr.set_source_rgb(0.04, 0.045, 0.055)
    cr.fill_preserve()
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.24 if transparent else 0.14)
    cr.set_line_width(0.9)
    cr.stroke()
    draw_quansheng_logo(cr, np_x + np_w * 0.5, np_y + np_h * 0.52, scale=0.95)

    # Proper 1.50:1 ST7565 LCD window
    lz_w = 228
    lz_h = int(round(lz_w / 1.46))  # 156px -> inner 216x144 (exact 1.50:1)
    lz_x = int(bx + (bw - lz_w) * 0.5)
    lz_y = int(by + 50)
    if transparent:
        round_rect(cr, lz_x - 8, lz_y - 8, lz_w + 16, lz_h + 16, 11.0)
        cr.set_source_rgba(0.16, 0.90, 0.30, 0.34)
        cr.fill_preserve()
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.58)
        cr.set_line_width(1.2)
        cr.stroke()
    rend._lcd_rect = _draw_lcd_bezel(cr, lz_x, lz_y, lz_w, lz_h, lcd_st, radius=7.0, chamfer=3.5 if transparent else 5.5, glow_alpha=0.14)

    # Molded `Mini Kong` pill (left) + `MIC` pill (right) + pinhole
    my = lz_y + lz_h + 12.0
    for px, pw, txt, fsz in ((bx + 26.0, 134.0, "Mini Kong", 9.4), (bx + bw - 82.0, 56.0, "MIC", 8.6)):
        round_rect(cr, px, my, pw, 22.0, 6.0)
        pat_pill = cairo.LinearGradient(px, my, px, my + 22.0)
        if transparent:
            pat_pill.add_color_stop_rgba(0.0, 0.95, 0.97, 0.99, 0.78)
            pat_pill.add_color_stop_rgba(1.0, 0.76, 0.80, 0.84, 0.72)
            cr.set_source(pat_pill)
            cr.fill_preserve()
            cr.set_source_rgba(0.28, 0.34, 0.38, 0.52)
            txt_rgba = (0.30, 0.34, 0.38, 0.88)
        else:
            pat_pill.add_color_stop_rgb(0.0, 0.16, 0.17, 0.19)
            pat_pill.add_color_stop_rgb(1.0, 0.10, 0.105, 0.12)
            cr.set_source(pat_pill)
            cr.fill_preserve()
            cr.set_source_rgba(1.0, 1.0, 1.0, 0.12)
            txt_rgba = (0.64, 0.67, 0.72, 0.86)
        cr.set_line_width(0.95)
        cr.stroke()
        draw_pango_text(cr, txt, px + pw * 0.5, my + 11.0, font_desc=f"Adwaita Sans Bold {fsz}", rgba=txt_rgba, halign="center", valign="center")

    # Small recessed circular mic pinhole directly beneath MIC pill
    cr.new_path()
    cr.arc(bx + bw - 54.0, my + 28.0, 3.0, 0, 2 * math.pi)
    cr.set_source_rgb(0.04, 0.05, 0.06)
    cr.fill()

    # Recessed keypad grid + 4×4 flush K1 keys + milled acoustic speaker slots
    kp_x, kp_y, kp_w = bx + 22.0, my + 30.0, bw - 44.0
    gap_c, gap_r, kh_nav, kh_num = 6.0, 10.5, 36.0, 33.5
    kw = (kp_w - gap_c * 3.0) / 4.0
    grid_h = kh_nav + 3 * kh_num + 3 * gap_r + 12.0
    round_rect(cr, kp_x - 6.0, kp_y - 6.0, kp_w + 12.0, grid_h, 8.0)
    if transparent:
        cr.set_source_rgba(0.84, 0.88, 0.91, 0.45)
        cr.fill_preserve()
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.48)
    else:
        cr.set_source_rgba(0.04, 0.045, 0.055, 0.72)
        cr.fill_preserve()
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.10)
    cr.set_line_width(0.9)
    cr.stroke()

    for r_i, row_keys in enumerate(K1_MATRIX):
        ky = kp_y + (0.0 if r_i == 0 else (kh_nav + gap_r + (r_i - 1) * (kh_num + gap_r)))
        kh = kh_nav if r_i == 0 else kh_num
        for c_i, (kn, pri, s1, s2) in enumerate(row_keys):
            kx = kp_x + c_i * (kw + gap_c)
            rend.hitboxes[kn] = (kx, ky, kw, kh)
            rend._key_specs.append((kn, kx, ky, kw, kh, pri, s1, s2, 4.0, r_i == 0, kn == "5", "k1"))
            if r_i < 3:
                sl_w = kw * 0.72
                round_rect(cr, kx + (kw - sl_w) * 0.5, ky + kh + 3.4, sl_w, 4.2, 1.8)
                cr.set_source_rgba(0.04, 0.05, 0.06, 0.82)
                cr.fill()
                cr.set_source_rgba(1.0, 1.0, 1.0, 0.35 if transparent else 0.14)
                cr.set_line_width(0.6)
                cr.move_to(kx + (kw - sl_w) * 0.5 + 2.0, ky + kh + 8.0)
                cr.line_to(kx + (kw + sl_w) * 0.5 - 2.0, ky + kh + 8.0)
                cr.stroke()


# =============================================================================
# 2. Quansheng UV-K5 All-Black Diagonal Grille (`media_1795271264067`)
# =============================================================================
def build_k5_black(
    rend: Any, back_cr: Any, cr: Any, W: int, H: int, lcd_st: LcdSettings, spec: HandsetSpec
) -> None:
    """Sculpted matte-black UV-K5: octagonal speaker island, twin 3D diagonal louvers, angled M/EXIT & B/C rocker."""
    import cairo

    bx, by, bw, bh = 31.0, 62.0, W - 47.0, H - 76.0
    rend._body_bx = bx
    rend._side_specs = [
        ("ptt", 10.0, by + 62.0, 24.0, 120.0, True, 0),
        ("side1", 12.0, by + 194.0, 22.0, 36.0, False, 1),
        ("side2", 12.0, by + 238.0, 22.0, 36.0, False, 2),
    ]
    for kn, sx, sy, sw, sh, _p, _d in rend._side_specs:
        rend.hitboxes[kn] = (sx - 3.0, sy, sw + 4.0, sh)

    round_rect(back_cr, bx + 4, by + 11, bw, bh, 20.0)
    back_cr.set_source_rgba(0.0, 0.0, 0.0, 0.42)
    back_cr.fill()
    draw_k5_side_shroud(back_cr, bx, by, bh, rend._side_specs)

    # Right accessory port rubber flap
    round_rect(back_cr, bx + bw - 7.0, by + 48.0, 13.5, bh - 188.0, 5.5)
    pat_rb = cairo.LinearGradient(bx + bw - 7.0, 0, bx + bw + 6.5, 0)
    pat_rb.add_color_stop_rgb(0.0, 0.05, 0.055, 0.065)
    pat_rb.add_color_stop_rgb(0.62, 0.17, 0.18, 0.20)
    pat_rb.add_color_stop_rgb(1.0, 0.06, 0.065, 0.075)
    back_cr.set_source(pat_rb)
    back_cr.fill()

    ax = bx + 60.0
    _draw_tapered_antenna(cr, ax, by)
    _draw_v_notch_knob(cr, bx + bw - 62.0, by, spec.accent, width=60.0)
    _draw_right_shoulder_ear(cr, bx, by, bw, (0.11, 0.115, 0.13))

    rend._torch_anchor = (bx + bw * 0.48, by, True)
    rend.hitboxes["_torch"] = (bx + bw * 0.48 - 18.0, by - 16.0, 36.0, 20.0)
    rend._led_anchor = (bx + 54.0, by - 6.5, 22.0, 8.5, True)

    # Distinct angular contoured UV-K5 matte-black body with pinched waist & chamfered shoulders
    waist_in = 6.0
    wy_t, wy_b = by + 232.0, by + 362.0
    cr.new_sub_path()
    cr.move_to(bx + 18.0, by)
    cr.line_to(bx + bw - 18.0, by)
    cr.line_to(bx + bw, by + 18.0)
    cr.line_to(bx + bw, wy_t)
    cr.line_to(bx + bw - waist_in, wy_t + 20.0)
    cr.line_to(bx + bw - waist_in, wy_b - 14.0)
    cr.line_to(bx + bw - 1.5, wy_b + 12.0)
    cr.line_to(bx + bw - 1.5, by + bh - 18.0)
    cr.curve_to(bx + bw - 1.5, by + bh, bx + bw - 16.0, by + bh, bx + bw - 24.0, by + bh)
    cr.line_to(bx + 24.0, by + bh)
    cr.curve_to(bx + 16.0, by + bh, bx + 1.5, by + bh, bx + 1.5, by + bh - 18.0)
    cr.line_to(bx + 1.5, wy_b + 12.0)
    cr.line_to(bx + waist_in, wy_b - 14.0)
    cr.line_to(bx + waist_in, wy_t + 20.0)
    cr.line_to(bx, wy_t)
    cr.line_to(bx, by + 18.0)
    cr.close_path()
    pat_b = cairo.LinearGradient(bx, by, bx + bw, by)
    pat_b.add_color_stop_rgb(0.0, 0.22, 0.23, 0.25)
    pat_b.add_color_stop_rgb(0.09, 0.125, 0.13, 0.145)
    pat_b.add_color_stop_rgb(0.50, 0.095, 0.10, 0.112)
    pat_b.add_color_stop_rgb(0.91, 0.12, 0.125, 0.14)
    pat_b.add_color_stop_rgb(1.0, 0.20, 0.21, 0.23)
    cr.set_source(pat_b)
    cr.fill_preserve()
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.12)
    cr.set_line_width(1.0)
    cr.stroke()

    # Molded bottom feet + lower-flank grip ribs
    cr.set_source_rgb(0.06, 0.065, 0.075)
    round_rect(cr, bx + 26.0, by + bh - 2.0, 26.0, 6.0, 2.0)
    round_rect(cr, bx + bw - 52.0, by + bh - 2.0, 26.0, 6.0, 2.0)
    cr.fill()
    for fi in range(3):
        round_rect(cr, bx + bw - 14.5 + fi * 3.4, wy_b + 24.0, 1.8, 92.0, 0.9)
        cr.set_source_rgba(0.0, 0.0, 0.0, 0.48)
        cr.fill()

    # Recessed chamfered upper eyebrow with crisp white QUANSHENG silkscreen ABOVE the LCD
    round_rect(cr, bx + 18.0, by + 8.0, bw - 36.0, 30.0, 7.0)
    cr.set_source_rgba(0.0, 0.0, 0.0, 0.22)
    cr.fill()
    draw_quansheng_logo(cr, bx + bw * 0.50, by + 23.0, scale=0.88)

    # Proper 1.50:1 Ice White ST7565 LCD
    lz_w = 216
    lz_h = int(round(lz_w / 1.46))  # 148px -> inner 204x136 (1.50:1)
    lz_x = int(bx + (bw - lz_w) * 0.5)
    lz_y = int(by + 46)
    rend._lcd_rect = _draw_lcd_bezel(cr, lz_x, lz_y, lz_w, lz_h, lcd_st, radius=8.0, chamfer=5.0, glow_alpha=0.16)

    # Signature raised chamfered octagonal speaker island on middle-right!
    isl_x, isl_y = bx + 92.0, lz_y + lz_h + 14.0
    isl_w, isl_h = bw - 106.0, 152.0
    cr.new_sub_path()
    cr.move_to(isl_x + 26.0, isl_y)
    cr.line_to(isl_x + isl_w - 14.0, isl_y)
    cr.line_to(isl_x + isl_w, isl_y + 14.0)
    cr.line_to(isl_x + isl_w, isl_y + isl_h - 16.0)
    cr.line_to(isl_x + isl_w - 16.0, isl_y + isl_h)
    cr.line_to(isl_x + 26.0, isl_y + isl_h)
    cr.line_to(isl_x, isl_y + isl_h - 24.0)
    cr.line_to(isl_x, isl_y + 24.0)
    cr.close_path()
    pat_isl = cairo.LinearGradient(isl_x, isl_y, isl_x + isl_w, isl_y + isl_h)
    pat_isl.add_color_stop_rgb(0.0, 0.20, 0.21, 0.23)
    pat_isl.add_color_stop_rgb(0.45, 0.145, 0.15, 0.165)
    pat_isl.add_color_stop_rgb(1.0, 0.085, 0.09, 0.10)
    cr.set_source(pat_isl)
    cr.fill_preserve()
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.13)
    cr.set_line_width(1.1)
    cr.stroke()

    # Twin recessed oval stadium speaker cavities with 3D diagonal slanted louvers (`/`)
    for gy in (isl_y + 30.0, isl_y + 84.0):
        gx, gw, gh = isl_x + 14.0, isl_w - 28.0, 36.0
        round_rect(cr, gx - 2.0, gy - 2.0, gw + 4.0, gh + 4.0, 19.0)
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.08)
        cr.fill()
        round_rect(cr, gx, gy, gw, gh, 18.0)
        cr.set_source_rgb(0.025, 0.028, 0.032)
        cr.fill()

        cr.save()
        round_rect(cr, gx + 1.0, gy + 1.0, gw - 2.0, gh - 2.0, 17.0)
        cr.clip()
        for dx in range(-10, int(gw + 16), 20):
            # 3D diagonal slat: dark body + bright bevel highlight edge
            cr.set_source_rgb(0.18, 0.19, 0.21)
            cr.set_line_width(9.0)
            cr.move_to(gx + dx, gy + gh + 6.0)
            cr.line_to(gx + dx + 22.0, gy - 6.0)
            cr.stroke()
            cr.set_source_rgba(1.0, 1.0, 1.0, 0.15)
            cr.set_line_width(1.2)
            cr.move_to(gx + dx - 3.6, gy + gh + 6.0)
            cr.line_to(gx + dx + 18.4, gy - 6.0)
            cr.stroke()
        cr.restore()

    # Recessed vertical capsule well for unified [B]/[C] rocker on left
    rx_well, ry_well = bx + 18.0, lz_y + lz_h + 48.0
    round_rect(cr, rx_well - 3.0, ry_well - 3.0, 48.0, 90.0, 11.0)
    cr.set_source_rgb(0.03, 0.035, 0.04)
    cr.fill_preserve()
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.11)
    cr.set_line_width(0.9)
    cr.stroke()

    # Angled trapezoidal M [A], joined vertical B/C rocker, angled trapezoidal EXIT [D]
    for kn, x, y, w, h, pri, s1, style in (
        ("menu", bx + 18.0, lz_y + lz_h + 10.0, 74.0, 30.0, "M", "[A]", "k5-diag-top"),
        ("up", rx_well, ry_well, 42.0, 41.0, "_CHEV_UP_", "[B]", "k5"),
        ("down", rx_well, ry_well + 43.0, 42.0, 41.0, "_CHEV_DN_", "[C]", "k5"),
        ("exit", bx + 18.0, lz_y + lz_h + 146.0, 76.0, 28.0, "EXIT", "[D]", "k5-diag-bot"),
    ):
        rend.hitboxes[kn] = (x, y, w, h)
        rend._key_specs.append((kn, x, y, w, h, pri, s1, "", 8.0, True, False, style))

    # Recessed matte-black keypad dish + 4×3 lozenge keypad
    kp_x, kp_y, kp_w = bx + 19.0, lz_y + lz_h + 186.0, bw - 38.0
    gap_c, gap_r, kh = 6.0, 6.5, 29.0
    kw = (kp_w - gap_c * 3.0) / 4.0
    round_rect(cr, kp_x - 6.0, kp_y - 7.0, kp_w + 12.0, 3 * kh + 2 * gap_r + 14.0, 11.0)
    cr.set_source_rgba(0.03, 0.035, 0.042, 0.80)
    cr.fill_preserve()
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.10)
    cr.set_line_width(0.95)
    cr.stroke()

    for r_i, row_keys in enumerate(K5_BOTTOM_MATRIX):
        ky = kp_y + r_i * (kh + gap_r)
        for c_i, (kn, pri, s1, s2) in enumerate(row_keys):
            kx = kp_x + c_i * (kw + gap_c)
            rend.hitboxes[kn] = (kx, ky, kw, kh)
            rend._key_specs.append((kn, kx, ky, kw, kh, pri, s1, s2, 12.0, False, kn == "5", "k5"))


# =============================================================================
# 3. Quansheng UV-K5(99) Charcoal & Red (`media_1795271225079` & `media_1795271243258`)
# =============================================================================
def build_k5_99(
    rend: Any, back_cr: Any, cr: Any, W: int, H: int, lcd_st: LcdSettings, spec: HandsetSpec
) -> None:
    """UV-K5(99): wide flared cheek-ribbed armor, visibly narrower stepped lower keypad skirt & 4-slat speaker."""
    import cairo

    bx, by, bw, bh = 31.0, 62.0, W - 47.0, H - 76.0
    rend._body_bx = bx
    rend._side_specs = [
        ("ptt", 10.0, by + 62.0, 24.0, 118.0, True, 0),
        ("side1", 12.0, by + 192.0, 22.0, 36.0, False, 1),
        ("side2", 12.0, by + 236.0, 22.0, 36.0, False, 2),
    ]
    for kn, sx, sy, sw, sh, _p, _d in rend._side_specs:
        rend.hitboxes[kn] = (sx - 3.0, sy, sw + 4.0, sh)

    # Stepped lower-skirt shadow
    arm_h = 316.0
    skirt_in = 10.0
    round_rect(back_cr, bx + 4, by + 10, bw, arm_h, 16.0)
    back_cr.set_source_rgba(0.0, 0.0, 0.0, 0.40)
    back_cr.fill()
    round_rect(back_cr, bx + skirt_in + 4, by + arm_h - 10, bw - skirt_in * 2, bh - arm_h + 18, 16.0)
    back_cr.fill()
    draw_k5_side_shroud(back_cr, bx, by, bh, rend._side_specs)

    ax = bx + 60.0
    _draw_tapered_antenna(cr, ax, by)
    _draw_v_notch_knob(cr, bx + bw - 62.0, by, spec.accent, width=62.0)
    _draw_right_shoulder_ear(cr, bx, by, bw, (0.12, 0.125, 0.14))

    rend._torch_anchor = (bx + bw * 0.48, by, True)
    rend.hitboxes["_torch"] = (bx + bw * 0.48 - 18.0, by - 16.0, 36.0, 20.0)
    rend._led_anchor = (bx + 46.0, by - 6.0, 22.0, 8.5, True)

    # Narrower black lower keypad body stepping inward below the wide upper armor cheeks!
    sx_l, sx_w = bx + skirt_in, bw - skirt_in * 2.0
    round_rect(cr, sx_l, by + arm_h - 18.0, sx_w, bh - arm_h + 18.0, 18.0)
    pat_sk = cairo.LinearGradient(sx_l, 0, sx_l + sx_w, 0)
    pat_sk.add_color_stop_rgb(0.0, 0.22, 0.23, 0.25)
    pat_sk.add_color_stop_rgb(0.08, 0.08, 0.085, 0.095)
    pat_sk.add_color_stop_rgb(0.92, 0.08, 0.085, 0.095)
    pat_sk.add_color_stop_rgb(1.0, 0.20, 0.21, 0.23)
    cr.set_source(pat_sk)
    cr.fill_preserve()
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.10)
    cr.set_line_width(1.0)
    cr.stroke()

    # Recessed charcoal keypad inner frame on the stepped narrow skirt
    round_rect(cr, sx_l + 9.0, by + arm_h + 2.0, sx_w - 18.0, bh - arm_h - 14.0, 12.0)
    cr.set_source_rgb(0.14, 0.145, 0.16)
    cr.fill_preserve()
    cr.set_source_rgba(0.0, 0.0, 0.0, 0.55)
    cr.set_line_width(1.0)
    cr.stroke()

    # Molded bottom feet
    cr.set_source_rgb(0.07, 0.075, 0.085)
    round_rect(cr, sx_l + 22.0, by + bh - 2.0, 24.0, 6.0, 2.0)
    round_rect(cr, sx_l + sx_w - 46.0, by + bh - 2.0, 24.0, 6.0, 2.0)
    cr.fill()

    # Wide upper black outer frame + colored/metallic sculpted cheek armor (`k5-99` charcoal or `k5-99-red` crimson!)
    round_rect(cr, bx, by, bw, arm_h, 16.0)
    cr.set_source_rgb(0.075, 0.08, 0.09)
    cr.fill()

    # Sculpted colored armor mask with angled corners & stepped cheek wings
    ax0, ay0, aw0, ah0 = bx + 6.0, by + 6.0, bw - 12.0, arm_h - 12.0
    cr.new_sub_path()
    cr.move_to(ax0 + 14.0, ay0)
    cr.line_to(ax0 + aw0 - 14.0, ay0)
    cr.line_to(ax0 + aw0, ay0 + 16.0)
    cr.line_to(ax0 + aw0, ay0 + ah0 - 18.0)
    cr.line_to(ax0 + aw0 - 14.0, ay0 + ah0)
    cr.line_to(ax0 + 14.0, ay0 + ah0)
    cr.line_to(ax0, ay0 + ah0 - 18.0)
    cr.line_to(ax0, ay0 + 16.0)
    cr.close_path()
    pa = cairo.LinearGradient(ax0, ay0, ax0 + aw0, ay0)
    pa.add_color_stop_rgb(0.00, *spec.armor_stops[0][1])
    pa.add_color_stop_rgb(0.16, *spec.armor_stops[1][1])
    pa.add_color_stop_rgb(0.50, *spec.armor_stops[2][1])
    pa.add_color_stop_rgb(0.84, *spec.armor_stops[1][1])
    pa.add_color_stop_rgb(1.00, *spec.armor_stops[0][1])
    cr.set_source(pa)
    cr.fill_preserve()
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.22)
    cr.set_line_width(1.1)
    cr.stroke()

    # Proper 1.50:1 amber ST7565 LCD
    lz_w = 216
    lz_h = int(round(lz_w / 1.46))  # 148px -> inner 204x136
    lz_x = int(bx + (bw - lz_w) * 0.5)
    lz_y = int(by + 20)
    rend._lcd_rect = _draw_lcd_bezel(cr, lz_x, lz_y, lz_w, lz_h, lcd_st, radius=7.0, chamfer=5.0, glow_alpha=0.22)

    # Stepped mid-bar carrying M [A] (left) and glossy black QUANSHENG plaque (right)
    bar_y = lz_y + lz_h + 12.0
    np_x, np_w = bx + 98.0, bw - 118.0
    round_rect(cr, np_x, bar_y, np_w, 32.0, 6.0)
    cr.set_source_rgb(0.05, 0.055, 0.065)
    cr.fill_preserve()
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.16)
    cr.set_line_width(0.9)
    cr.stroke()
    draw_quansheng_logo(cr, np_x + np_w * 0.5, bar_y + 16.0, scale=0.74)

    # Center recessed black speaker cavity with 4 bold beveled horizontal louver slats
    sp_x, sp_y, sp_w, sp_h = bx + 86.0, bar_y + 38.0, bw - 156.0, 92.0
    round_rect(cr, sp_x - 4.0, sp_y - 4.0, sp_w + 8.0, sp_h + 8.0, 6.0)
    cr.set_source_rgb(0.035, 0.04, 0.045)
    cr.fill()
    for si in range(4):
        sy = sp_y + 2.0 + si * 22.5
        round_rect(cr, sp_x, sy, sp_w, 15.5, 3.0)
        pat_sl = cairo.LinearGradient(0, sy, 0, sy + 15.5)
        pat_sl.add_color_stop_rgb(0.0, 0.30, 0.31, 0.34)
        pat_sl.add_color_stop_rgb(0.55, 0.20, 0.21, 0.23)
        pat_sl.add_color_stop_rgb(1.0, 0.12, 0.13, 0.15)
        cr.set_source(pat_sl)
        cr.fill_preserve()
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.14)
        cr.set_line_width(0.75)
        cr.stroke()

    # Right cheek: 3 deeply recessed horizontal stadium grip grooves (signature UV-K5(99) detail!)
    rc_x = sp_x + sp_w + 14.0
    for gi in range(3):
        gy = sp_y + 10.0 + gi * 25.5
        round_rect(cr, rc_x, gy, 40.0, 12.0, 6.0)
        cr.set_source_rgba(0.0, 0.0, 0.0, 0.30)
        cr.fill_preserve()
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.22)
        cr.set_line_width(0.85)
        cr.stroke()

    # Vertical recessed pill microphone slot on lower-right apron next to EXIT [D]
    round_rect(cr, sx_l + sx_w - 28.0, by + arm_h + 12.0, 8.0, 22.0, 4.0)
    cr.set_source_rgb(0.04, 0.045, 0.05)
    cr.fill_preserve()
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.14)
    cr.set_line_width(0.75)
    cr.stroke()

    for kn, x, y, w, h, pri, s1 in (
        ("menu", bx + 20.0, bar_y, 68.0, 30.0, "M", "[A]"),
        ("up", bx + 20.0, sp_y + 6.0, 52.0, 34.0, "_CHEV_UP_", "[B]"),
        ("down", bx + 20.0, sp_y + 48.0, 52.0, 34.0, "_CHEV_DN_", "[C]"),
        ("exit", sx_l + 16.0, by + arm_h + 10.0, 64.0, 28.0, "EXIT", "[D]"),
    ):
        rend.hitboxes[kn] = (x, y, w, h)
        rend._key_specs.append((kn, x, y, w, h, pri, s1, "", 8.0, True, False, "k5"))

    # Stepped narrow 4×3 UV-K5(99) Step / Vox / TDR keypad
    kp_x, kp_y, kp_w = sx_l + 16.0, by + arm_h + 46.0, sx_w - 32.0
    gap_c, gap_r, kh = 5.5, 6.5, 30.0
    kw = (kp_w - gap_c * 3.0) / 4.0
    for r_i, row_keys in enumerate(K5_99_MATRIX):
        ky = kp_y + r_i * (kh + gap_r)
        for c_i, (kn, pri, s1, s2) in enumerate(row_keys):
            kx = kp_x + c_i * (kw + gap_c)
            rend.hitboxes[kn] = (kx, ky, kw, kh)
            rend._key_specs.append((kn, kx, ky, kw, kh, pri, s1, s2, 11.5, False, kn == "5", "k5"))


# =============================================================================
# 4. Quansheng UV-5R Plus (`media_1795271406435`)
# =============================================================================
def build_uv5r_plus(
    rend: Any, back_cr: Any, cr: Any, W: int, H: int, lcd_st: LcdSettings, spec: HandsetSpec
) -> None:
    """Quansheng UV-5R Plus: all-black knob, cobalt-blue LCD, orange M [A], blue EXIT [D], 6×7 perforated grille."""
    import cairo

    bx, by, bw, bh = 31.0, 60.0, W - 47.0, H - 72.0
    rend._body_bx = bx
    rend._side_specs = [
        ("ptt", 10.0, by + 62.0, 24.0, 114.0, True, 0),
        ("side1", 12.0, by + 188.0, 22.0, 35.0, False, 1),
        ("side2", 12.0, by + 232.0, 22.0, 35.0, False, 2),
    ]
    for kn, sx, sy, sw, sh, _p, _d in rend._side_specs:
        rend.hitboxes[kn] = (sx - 3.0, sy, sw + 4.0, sh)

    skirt_in = 7.0
    split_y = by + 314.0
    round_rect(back_cr, bx + 4, by + 10, bw, bh, 18.0)
    back_cr.set_source_rgba(0.0, 0.0, 0.0, 0.42)
    back_cr.fill()
    draw_k5_side_shroud(back_cr, bx, by, bh, rend._side_specs)

    ax = bx + 58.0
    _draw_tapered_antenna(cr, ax, by)
    _draw_v_notch_knob(cr, bx + bw - 62.0, by, None, width=58.0)  # All-black knob!
    _draw_right_shoulder_ear(cr, bx, by, bw, (0.11, 0.115, 0.13))

    rend._torch_anchor = (bx + bw * 0.47, by, True)
    rend.hitboxes["_torch"] = (bx + bw * 0.47 - 18.0, by - 16.0, 36.0, 20.0)
    rend._led_anchor = (bx + 54.0, by - 5.5, 20.0, 7.5, True)

    # Classic UV-5R contoured silhouette (wider upper body + stepped lower skirt with black bumper rails)
    cr.new_sub_path()
    cr.move_to(bx + 18.0, by)
    cr.line_to(bx + bw - 18.0, by)
    cr.curve_to(bx + bw, by, bx + bw, by + 10.0, bx + bw, by + 20.0)
    cr.line_to(bx + bw, split_y - 8.0)
    cr.line_to(bx + bw - skirt_in, split_y + 10.0)
    cr.line_to(bx + bw - skirt_in, by + bh - 18.0)
    cr.curve_to(bx + bw - skirt_in, by + bh, bx + bw - skirt_in - 16.0, by + bh, bx + bw - skirt_in - 24.0, by + bh)
    cr.line_to(bx + skirt_in + 24.0, by + bh)
    cr.curve_to(bx + skirt_in + 16.0, by + bh, bx + skirt_in, by + bh, bx + skirt_in, by + bh - 18.0)
    cr.line_to(bx + skirt_in, split_y + 10.0)
    cr.line_to(bx, split_y - 8.0)
    cr.line_to(bx, by + 20.0)
    cr.curve_to(bx, by + 10.0, bx, by, bx + 18.0, by)
    cr.close_path()
    pb = cairo.LinearGradient(bx, by, bx + bw, by)
    pb.add_color_stop_rgb(0.0, 0.22, 0.23, 0.25)
    pb.add_color_stop_rgb(0.08, 0.12, 0.125, 0.14)
    pb.add_color_stop_rgb(0.50, 0.095, 0.10, 0.115)
    pb.add_color_stop_rgb(0.92, 0.12, 0.125, 0.14)
    pb.add_color_stop_rgb(1.0, 0.21, 0.22, 0.24)
    cr.set_source(pb)
    cr.fill_preserve()
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.12)
    cr.set_line_width(1.0)
    cr.stroke()

    # Recessed upper charcoal LCD surround frame
    round_rect(cr, bx + 14.0, by + 12.0, bw - 28.0, 182.0, 12.0)
    cr.set_source_rgba(0.04, 0.045, 0.055, 0.55)
    cr.fill_preserve()
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.09)
    cr.set_line_width(0.9)
    cr.stroke()

    # Proper 1.49:1 Cobalt-blue ST7565 LCD
    lz_w = 218
    lz_h = int(round(lz_w / 1.46))  # 149px -> inner 206x137
    lz_x = int(bx + (bw - lz_w) * 0.5)
    lz_y = int(by + 24)
    rend._lcd_rect = _draw_lcd_bezel(cr, lz_x, lz_y, lz_w, lz_h, lcd_st, radius=7.0, chamfer=4.5, glow_alpha=0.26)

    # Signature UV-5R Plus 6×7 horizontal-oval perforated speaker matrix (stepped at top-left for orange M key!)
    gx, gy, gw, gh = bx + 84.0, lz_y + lz_h + 26.0, bw - 104.0, 92.0
    round_rect(cr, gx - 4.0, gy - 4.0, gw + 8.0, gh + 8.0, 8.0)
    cr.set_source_rgb(0.13, 0.135, 0.15)
    cr.fill()
    cols, rows = 7, 6
    cell_w = (gw - 12.0) / cols
    cell_h = (gh - 8.0) / rows
    for r in range(rows):
        for c in range(cols):
            # Subtle UV-5R stepped top-left cutout near orange M key
            if r == 0 and c == 0:
                continue
            px = gx + 6.0 + c * cell_w + 2.0
            py = gy + 4.0 + r * cell_h + 2.0
            round_rect(cr, px, py, cell_w - 5.0, 6.8, 3.0)
            cr.set_source_rgb(0.025, 0.028, 0.032)
            cr.fill()
            cr.set_source_rgba(1.0, 1.0, 1.0, 0.09)
            cr.set_line_width(0.6)
            cr.move_to(px + 2.0, py + 7.4)
            cr.line_to(px + cell_w - 7.0, py + 7.4)
            cr.stroke()

    # Glossy recessed QUANSHENG plaque below perforated speaker grille
    pl_x, pl_y, pl_w, pl_h = gx + 14.0, gy + gh + 8.0, gw - 16.0, 21.0
    round_rect(cr, pl_x, pl_y, pl_w, pl_h, 4.0)
    cr.set_source_rgb(0.055, 0.06, 0.07)
    cr.fill()
    draw_quansheng_logo(cr, pl_x + pl_w * 0.5, pl_y + pl_h * 0.52, scale=0.66)

    # Signature colored keys: Vibrant Orange M [A] & Sky-Blue EXIT [D]
    for kn, x, y, w, h, pri, s1, style in (
        ("menu", bx + 18.0, lz_y + lz_h + 10.0, 68.0, 30.0, "M", "[A]", "k5-orange"),
        ("up", bx + 18.0, gy + 14.0, 48.0, 35.0, "_CHEV_UP_", "[B]", "k5"),
        ("down", bx + 18.0, gy + 55.0, 48.0, 35.0, "_CHEV_DN_", "[C]", "k5"),
        ("exit", bx + skirt_in + 12.0, split_y + 8.0, 66.0, 28.0, "EXIT", "[D]", "k5-blue"),
    ):
        rend.hitboxes[kn] = (x, y, w, h)
        rend._key_specs.append((kn, x, y, w, h, pri, s1, "", 8.0, True, False, style))

    # Recessed lower keypad tray on the stepped narrow skirt
    kp_x, kp_y, kp_w = bx + skirt_in + 12.0, split_y + 44.0, bw - (skirt_in + 12.0) * 2.0
    gap_c, gap_r, kh = 5.5, 6.5, 29.0
    kw = (kp_w - gap_c * 3.0) / 4.0
    round_rect(cr, kp_x - 5.0, kp_y - 6.0, kp_w + 10.0, 3 * kh + 2 * gap_r + 12.0, 10.0)
    cr.set_source_rgba(0.03, 0.035, 0.042, 0.72)
    cr.fill_preserve()
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.09)
    cr.stroke()

    for r_i, row_keys in enumerate(K5_BOTTOM_MATRIX):
        ky = kp_y + r_i * (kh + gap_r)
        for c_i, (kn, pri, s1, s2) in enumerate(row_keys):
            kx = kp_x + c_i * (kw + gap_c)
            rend.hitboxes[kn] = (kx, ky, kw, kh)
            rend._key_specs.append((kn, kx, ky, kw, kh, pri, s1, s2, 11.5, False, kn == "5", "k5"))

    # Glossy sculpted chin nameplate (`UV-5R PLUS`)
    ch_w = 114.0
    round_rect(cr, bx + (bw - ch_w) * 0.5, by + bh - 20.0, ch_w, 15.0, 4.0)
    cr.set_source_rgba(0.03, 0.035, 0.045, 0.80)
    cr.fill()
    draw_pango_text(
        cr, "UV-5R PLUS", bx + bw * 0.50, by + bh - 12.5,
        font_desc="Adwaita Sans Bold 7.4", rgba=(0.76, 0.80, 0.85, 0.86), halign="center", valign="center",
    )


# =============================================================================
# 5. Quansheng UV-K10 Pro (Photorealistic front chassis from official hardware photos)
# =============================================================================
def build_k10_pro(
    rend: Any, back_cr: Any, cr: Any, W: int, H: int, lcd_st: LcdSettings, spec: HandsetSpec
) -> None:
    """
    Quansheng UV-K10 Pro:
      • Metallic slate-blue sculpted exoskeleton with 3 raised top turret collars
      • Anodized cyan antenna collar + matte U/V rubber whip base
      • Center knurled waterproof SMA/HF dust cap with top pull-tab + right diagonally-knurled volume knob
      • Warm brushed champagne-titanium upper U-fascia terminating cleanly above the black keypad basin
      • Contoured glossy-black QUANSHENG brow badge with live glowing dual-RF-arc RX/TX status LED
      • Deep glossy-black beveled LCD lens, tight 3-key [A|B] · V/M · Band bar, recessed slate accent bar
      • Sculpted 4×4 chamfered-corner matte-black keypad with horizontal divider ribs & cyan '0' FM radio icon
      • Left flank: cyan-collared tactical PTT, two dotless black side keys, vivid cyan lower APRS button
    """
    import cairo

    bx, by, bw, bh = 30.0, 64.0, W - 46.0, H - 76.0
    rend._body_bx = bx
    rend._side_specs = [
        ("ptt", 8.0, by + 54.0, 25.0, 102.0, True, 0),
        ("side1", 11.0, by + 170.0, 22.0, 31.0, False, 0),
        ("side2", 11.0, by + 206.0, 22.0, 31.0, False, 0),
    ]
    for kn, sx, sy, sw, sh, _p, _d in rend._side_specs:
        rend.hitboxes[kn] = (sx - 3.0, sy, sw + 4.0, sh)

    # --- Back layer: drop shadow, tailored upper-left tactical button shroud, cyan SK3 & right port cheek ---
    round_rect(back_cr, bx + 4.0, by + 10.0, bw, bh, 17.0)
    back_cr.set_source_rgba(0.0, 0.0, 0.0, 0.44)
    back_cr.fill()

    # Tailored UV-K10 Pro left side-button shroud: hugs PTT, SK1, SK2 & cyan SK3 then tucks cleanly into the lower rail
    sh_x = bx - 13.5
    sh_top = by + 34.0
    sk3_y = by + 247.0
    sh_bot = sk3_y + 39.0
    back_cr.new_path()
    back_cr.move_to(bx + 4.0, sh_top - 4.0)
    back_cr.curve_to(bx - 8.0, sh_top - 2.0, sh_x, sh_top + 8.0, sh_x, sh_top + 18.0)
    back_cr.line_to(sh_x, sh_bot - 14.0)
    back_cr.curve_to(sh_x, sh_bot - 2.0, bx - 8.0, sh_bot + 4.0, bx + 4.0, sh_bot + 6.0)
    back_cr.close_path()
    pat_sh = cairo.LinearGradient(sh_x, 0, bx + 4.0, 0)
    pat_sh.add_color_stop_rgb(0.0, 0.055, 0.065, 0.08)
    pat_sh.add_color_stop_rgb(0.35, 0.145, 0.16, 0.185)
    pat_sh.add_color_stop_rgb(0.75, 0.085, 0.095, 0.11)
    pat_sh.add_color_stop_rgb(1.0, 0.035, 0.04, 0.048)
    back_cr.set_source(pat_sh)
    back_cr.fill_preserve()
    back_cr.set_source_rgba(1.0, 1.0, 1.0, 0.09)
    back_cr.set_line_width(0.85)
    back_cr.stroke()
    for _kn, sx, sy, sw, sh, is_ptt, _dots in rend._side_specs:
        round_rect(back_cr, sx + 4.0, sy - 2.4, sw - 2.0, sh + 4.8, 8.0 if is_ptt else 6.0)
        back_cr.set_source_rgba(0.015, 0.015, 0.02, 0.90)
        back_cr.fill()

    # Vivid cyan-blue lower side key (SK3 / APRS) nestled cleanly in the lower-left side channel
    round_rect(back_cr, bx - 10.5, sk3_y - 2.0, 14.0, 34.0, 4.5)
    back_cr.set_source_rgba(0.02, 0.025, 0.035, 0.90)
    back_cr.fill()
    round_rect(back_cr, bx - 9.2, sk3_y, 12.0, 30.0, 3.6)
    pat_sk3 = cairo.LinearGradient(bx - 9.2, sk3_y, bx + 2.8, sk3_y)
    pat_sk3.add_color_stop_rgb(0.0, 0.09, 0.62, 0.88)
    pat_sk3.add_color_stop_rgb(0.45, 0.22, 0.82, 0.99)
    pat_sk3.add_color_stop_rgb(1.0, 0.06, 0.46, 0.72)
    back_cr.set_source(pat_sk3)
    back_cr.fill_preserve()
    back_cr.set_source_rgba(1.0, 1.0, 1.0, 0.28)
    back_cr.set_line_width(0.75)
    back_cr.stroke()

    # Right flank: subtle rubberized Kenwood + USB-C port flap cheek peeking along right edge
    round_rect(back_cr, bx + bw - 6.0, by + 64.0, 11.5, 198.0, 4.8)
    pat_rc = cairo.LinearGradient(bx + bw - 6.0, 0, bx + bw + 5.5, 0)
    pat_rc.add_color_stop_rgb(0.0, 0.05, 0.06, 0.075)
    pat_rc.add_color_stop_rgb(0.55, 0.16, 0.18, 0.21)
    pat_rc.add_color_stop_rgb(1.0, 0.07, 0.08, 0.095)
    back_cr.set_source(pat_rc)
    back_cr.fill()
    # Split seam between audio jack flap and USB-C flap on right flank
    back_cr.set_source_rgba(0.02, 0.02, 0.03, 0.82)
    back_cr.rectangle(bx + bw - 2.0, by + 178.0, 8.0, 5.0)
    back_cr.fill()

    # --- Top Hardware: Antenna + cyan collar, Center knurled HF/SMA cap with pull-tab, Right diagonal-knurl knob ---
    ax = bx + 54.0
    # Matte black U/V antenna lower boot (slightly tapered upward)
    pat_ant = cairo.LinearGradient(ax - 23.0, 0, ax + 23.0, 0)
    pat_ant.add_color_stop_rgb(0.00, 0.06, 0.065, 0.075)
    pat_ant.add_color_stop_rgb(0.28, 0.26, 0.27, 0.29)
    pat_ant.add_color_stop_rgb(0.58, 0.15, 0.16, 0.175)
    pat_ant.add_color_stop_rgb(1.00, 0.045, 0.05, 0.06)
    cr.set_source(pat_ant)
    cr.new_path()
    cr.move_to(ax - 23.0, by - 8.0)
    cr.line_to(ax - 21.0, 2.0)
    cr.line_to(ax + 21.0, 2.0)
    cr.line_to(ax + 23.0, by - 8.0)
    cr.close_path()
    cr.fill()

    # Subtle embossed vertical "U/V" text on antenna boot (authentic Quansheng whip detail)
    cr.save()
    cr.translate(ax, 23.0)
    cr.rotate(-math.pi * 0.5)
    draw_pango_text(cr, "U/V", 0.0, 0.0, font_desc="Adwaita Sans Bold 8.0", rgba=(0.04, 0.045, 0.055, 0.52), halign="center", valign="center")
    cr.restore()

    # Signature bright anodized cyan-turquoise antenna base collar
    col_y = by - 19.0
    round_rect(cr, ax - 23.5, col_y, 47.0, 14.5, 2.2)
    pat_col = cairo.LinearGradient(ax - 23.5, 0, ax + 23.5, 0)
    pat_col.add_color_stop_rgb(0.00, 0.04, 0.42, 0.64)
    pat_col.add_color_stop_rgb(0.28, 0.20, 0.80, 0.98)
    pat_col.add_color_stop_rgb(0.44, 0.34, 0.89, 1.00)
    pat_col.add_color_stop_rgb(0.76, 0.09, 0.58, 0.82)
    pat_col.add_color_stop_rgb(1.00, 0.03, 0.32, 0.52)
    cr.set_source(pat_col)
    cr.fill_preserve()
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.24)
    cr.set_line_width(0.75)
    cr.stroke()
    # Thin dark separation groove above and below cyan collar
    cr.set_source_rgba(0.0, 0.0, 0.0, 0.55)
    cr.set_line_width(1.0)
    cr.move_to(ax - 23.0, col_y + 0.3)
    cr.line_to(ax + 23.0, col_y + 0.3)
    cr.stroke()

    # Center turret: knurled black waterproof HF/SMA dust cap + signature top pull-tab nub
    cx_m = bx + bw * 0.505
    cap_w, cap_h = 42.0, 21.0
    cap_y = by - 29.0
    # Top pull-tab stem sticking up from center of dust cap
    pat_tab = cairo.LinearGradient(cx_m - 6.0, 0, cx_m + 6.0, 0)
    pat_tab.add_color_stop_rgb(0.0, 0.07, 0.08, 0.09)
    pat_tab.add_color_stop_rgb(0.4, 0.28, 0.30, 0.33)
    pat_tab.add_color_stop_rgb(1.0, 0.05, 0.06, 0.07)
    cr.set_source(pat_tab)
    cr.new_path()
    cr.move_to(cx_m - 6.0, cap_y + 2.0)
    cr.line_to(cx_m - 3.8, cap_y - 9.0)
    cr.line_to(cx_m + 3.8, cap_y - 9.0)
    cr.line_to(cx_m + 6.0, cap_y + 2.0)
    cr.close_path()
    cr.fill()
    # Rounded shoulder neck below dust cap
    round_rect(cr, cx_m - 17.0, cap_y + cap_h - 4.0, 34.0, 12.0, 3.0)
    cr.set_source_rgb(0.08, 0.09, 0.11)
    cr.fill()
    # Main ribbed rubber dust-cap drum
    round_rect(cr, cx_m - cap_w * 0.5, cap_y, cap_w, cap_h, 6.5)
    pat_mc = cairo.LinearGradient(cx_m - cap_w * 0.5, 0, cx_m + cap_w * 0.5, 0)
    pat_mc.add_color_stop_rgb(0.00, 0.06, 0.07, 0.08)
    pat_mc.add_color_stop_rgb(0.32, 0.27, 0.29, 0.32)
    pat_mc.add_color_stop_rgb(0.65, 0.14, 0.15, 0.17)
    pat_mc.add_color_stop_rgb(1.00, 0.05, 0.06, 0.07)
    cr.set_source(pat_mc)
    cr.fill_preserve()
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.14)
    cr.set_line_width(0.8)
    cr.stroke()
    # Vertical knurling grooves on center cap
    cr.set_source_rgba(0.02, 0.02, 0.03, 0.72)
    for dx in (-15.0, -9.0, -3.0, 3.0, 9.0, 15.0):
        round_rect(cr, cx_m + dx - 1.3, cap_y + 3.5, 2.6, cap_h - 7.0, 1.0)
        cr.fill()

    # Right turret: larger power/volume rotary knob with iconic slanted/diagonal knurling ridges (///)
    kx_r = bx + bw - 54.0
    kw_r, kh_r = 56.0, 29.0
    ky_r = by - 34.0
    round_rect(cr, kx_r - kw_r * 0.5, ky_r, kw_r, kh_r, 6.0)
    pat_rk = cairo.LinearGradient(kx_r - kw_r * 0.5, 0, kx_r + kw_r * 0.5, 0)
    pat_rk.add_color_stop_rgb(0.00, 0.06, 0.065, 0.075)
    pat_rk.add_color_stop_rgb(0.28, 0.28, 0.30, 0.33)
    pat_rk.add_color_stop_rgb(0.58, 0.15, 0.16, 0.18)
    pat_rk.add_color_stop_rgb(1.00, 0.045, 0.05, 0.06)
    cr.set_source(pat_rk)
    cr.fill_preserve()
    # Clipped diagonal knurling grooves leaning top-right to bottom-left
    cr.save()
    cr.clip_preserve()
    for dx in range(-32, 36, 8):
        cr.set_source_rgba(0.02, 0.02, 0.03, 0.72)
        cr.set_line_width(3.0)
        cr.move_to(kx_r + dx + 5.5, ky_r + 4.0)
        cr.line_to(kx_r + dx - 2.5, ky_r + kh_r - 3.0)
        cr.stroke()
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.10)
        cr.set_line_width(0.8)
        cr.move_to(kx_r + dx + 7.2, ky_r + 4.0)
        cr.line_to(kx_r + dx - 0.8, ky_r + kh_r - 3.0)
        cr.stroke()
    # Chamfered top bevel band on volume knob
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.12)
    cr.rectangle(kx_r - kw_r * 0.5, ky_r, kw_r, 3.8)
    cr.fill()
    cr.restore()
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.16)
    cr.set_line_width(0.85)
    cr.stroke()

    # Raised slate-blue cylindrical turret bosses seamlessly joining the 3 top controls to the top deck
    def _draw_slate_boss(cx_b: float, w_b: float, y_top: float, h_b: float) -> None:
        round_rect(cr, cx_b - w_b * 0.5, y_top, w_b, h_b, 8.0)
        pat_sb = cairo.LinearGradient(cx_b - w_b * 0.5, y_top, cx_b + w_b * 0.5, y_top)
        pat_sb.add_color_stop_rgb(0.00, 0.14, 0.19, 0.27)
        pat_sb.add_color_stop_rgb(0.28, 0.34, 0.42, 0.54)
        pat_sb.add_color_stop_rgb(0.55, 0.22, 0.28, 0.38)
        pat_sb.add_color_stop_rgb(1.00, 0.11, 0.15, 0.22)
        cr.set_source(pat_sb)
        cr.fill_preserve()
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.18)
        cr.set_line_width(0.85)
        cr.stroke()

    _draw_slate_boss(ax, 58.0, by - 7.0, 20.0)
    _draw_slate_boss(cx_m, 44.0, by - 9.0, 22.0)
    _draw_slate_boss(kx_r, 64.0, by - 9.0, 22.0)

    # Invisible top-center torch beam anchor (emits beam when torch_on without adding a fake grey block)
    rend._torch_anchor = (cx_m, by - 2.0, True)
    rend.hitboxes["_torch"] = (cx_m - 20.0, by - 28.0, 40.0, 26.0)

    # --- Sculpted Metallic Slate-Blue Outer Exoskeleton ---
    def _body_path() -> None:
        cr.new_sub_path()
        cr.move_to(bx + 14.0, by)
        cr.line_to(bx + bw - 14.0, by)
        cr.curve_to(bx + bw - 3.0, by, bx + bw, by + 5.0, bx + bw, by + 16.0)
        cr.line_to(bx + bw, by + bh - 14.0)
        cr.curve_to(bx + bw, by + bh - 4.0, bx + bw - 5.0, by + bh, bx + bw - 14.0, by + bh)
        cr.line_to(bx + 14.0, by + bh)
        cr.curve_to(bx + 5.0, by + bh, bx, by + bh - 4.0, bx, by + bh - 14.0)
        cr.line_to(bx, by + 16.0)
        cr.curve_to(bx, by + 5.0, bx + 3.0, by, bx + 14.0, by)
        cr.close_path()

    _body_path()
    pb = cairo.LinearGradient(bx, by, bx + bw, by)
    pb.add_color_stop_rgb(0.00, 0.18, 0.24, 0.34)
    pb.add_color_stop_rgb(0.05, 0.34, 0.42, 0.54)
    pb.add_color_stop_rgb(0.14, 0.22, 0.28, 0.38)
    pb.add_color_stop_rgb(0.50, 0.18, 0.23, 0.32)
    pb.add_color_stop_rgb(0.86, 0.22, 0.28, 0.38)
    pb.add_color_stop_rgb(0.95, 0.32, 0.40, 0.52)
    pb.add_color_stop_rgb(1.00, 0.16, 0.21, 0.30)
    cr.set_source(pb)
    cr.fill_preserve()

    # Subtle vertical sheen across upper deck & bottom chin of slate-blue shell
    cr.save()
    cr.clip_preserve()
    pat_v = cairo.LinearGradient(0, by, 0, by + 26.0)
    pat_v.add_color_stop_rgba(0.0, 0.72, 0.84, 1.0, 0.22)
    pat_v.add_color_stop_rgba(0.6, 0.50, 0.62, 0.78, 0.05)
    pat_v.add_color_stop_rgba(1.0, 0.0, 0.0, 0.0, 0.0)
    cr.set_source(pat_v)
    cr.rectangle(bx, by, bw, 26.0)
    cr.fill()

    # Subtle horizontal sculpted chin bevel step at bottom of blue exoskeleton
    pat_chin = cairo.LinearGradient(0, by + bh - 18.0, 0, by + bh)
    pat_chin.add_color_stop_rgba(0.0, 0.0, 0.0, 0.0, 0.18)
    pat_chin.add_color_stop_rgba(0.55, 0.75, 0.86, 1.0, 0.14)
    pat_chin.add_color_stop_rgba(1.0, 0.0, 0.0, 0.0, 0.36)
    cr.set_source(pat_chin)
    cr.rectangle(bx, by + bh - 18.0, bw, 18.0)
    cr.fill()
    cr.restore()

    cr.set_source_rgba(0.82, 0.90, 1.0, 0.22)
    cr.set_line_width(1.1)
    cr.stroke()

    # Recessed dark inner framing perimeter around the lower half (keypad basin rim)
    rail = 11.5
    lower_y = by + 286.0
    round_rect(cr, bx + rail - 1.5, lower_y, bw - (rail - 1.5) * 2.0, by + bh - lower_y - 12.0, 13.0)
    cr.set_source_rgb(0.045, 0.05, 0.062)
    cr.fill_preserve()
    cr.set_source_rgba(0.75, 0.85, 0.98, 0.16)
    cr.set_line_width(0.85)
    cr.stroke()

    # --- Sculpted Warm Brushed Champagne-Titanium Upper/Middle U-Fascia (stops cleanly above keypad!) ---
    fx_l, fx_r = bx + rail, bx + bw - rail
    fy_top_outer = by + 13.0
    fy_top_mid = by + 18.0
    fy_bridge_bot = by + 310.0

    def _titanium_fascia_path() -> None:
        cr.new_sub_path()
        # Winged upper brow flaring right out to the outer top shoulders like the real UV-K10 Pro
        cr.move_to(bx + 1.5, fy_top_outer + 3.0)
        cr.curve_to(bx + 1.5, fy_top_outer, bx + 8.0, fy_top_outer, fx_l + 24.0, fy_top_outer + 2.5)
        cr.line_to(bx + bw * 0.50 - 62.0, fy_top_mid)
        cr.line_to(bx + bw * 0.50 + 62.0, fy_top_mid)
        cr.line_to(fx_r - 24.0, fy_top_outer + 2.5)
        cr.curve_to(bx + bw - 8.0, fy_top_outer, bx + bw - 1.5, fy_top_outer, bx + bw - 1.5, fy_top_outer + 3.0)
        # Stepped shoulder wings tapering smoothly inward along the slate-blue side rails
        cr.line_to(bx + bw - 1.5, fy_top_outer + 19.0)
        cr.line_to(fx_r, fy_top_outer + 31.0)
        cr.line_to(fx_r, fy_bridge_bot - 16.0)
        cr.curve_to(fx_r, fy_bridge_bot - 4.0, fx_r - 5.0, fy_bridge_bot, fx_r - 16.0, fy_bridge_bot)
        cr.line_to(fx_l + 16.0, fy_bridge_bot)
        cr.curve_to(fx_l + 5.0, fy_bridge_bot, fx_l, fy_bridge_bot - 4.0, fx_l, fy_bridge_bot - 16.0)
        cr.line_to(fx_l, fy_top_outer + 31.0)
        cr.line_to(bx + 1.5, fy_top_outer + 19.0)
        cr.close_path()

    # Cast shadow of titanium armor onto the dark keypad basin below
    cr.save()
    cr.translate(0.0, 2.5)
    _titanium_fascia_path()
    cr.set_source_rgba(0.0, 0.0, 0.0, 0.55)
    cr.fill()
    cr.restore()

    _titanium_fascia_path()
    pa = cairo.LinearGradient(fx_l, fy_top_outer, fx_r, fy_top_outer)
    pa.add_color_stop_rgb(0.00, 0.30, 0.29, 0.26)
    pa.add_color_stop_rgb(0.08, 0.58, 0.56, 0.51)
    pa.add_color_stop_rgb(0.24, 0.50, 0.48, 0.44)
    pa.add_color_stop_rgb(0.50, 0.42, 0.40, 0.37)
    pa.add_color_stop_rgb(0.76, 0.50, 0.48, 0.44)
    pa.add_color_stop_rgb(0.92, 0.56, 0.54, 0.49)
    pa.add_color_stop_rgb(1.00, 0.28, 0.27, 0.25)
    cr.set_source(pa)
    cr.fill_preserve()

    cr.save()
    cr.clip_preserve()
    paint_brushed_metal_grain(cr, fx_l - 4.0, fy_top_outer, (fx_r - fx_l) + 8.0, fy_bridge_bot - fy_top_outer, vertical=False, alpha=0.030)
    # Soft top metallic brow highlight
    pat_brow = cairo.LinearGradient(0, fy_top_outer, 0, fy_top_outer + 32.0)
    pat_brow.add_color_stop_rgba(0.0, 1.0, 0.98, 0.92, 0.26)
    pat_brow.add_color_stop_rgba(1.0, 1.0, 0.98, 0.92, 0.0)
    cr.set_source(pat_brow)
    cr.rectangle(fx_l - 4.0, fy_top_outer, (fx_r - fx_l) + 8.0, 32.0)
    cr.fill()
    # Sculpted lower metallic bevel shading right above the keypad
    pat_lip = cairo.LinearGradient(0, fy_bridge_bot - 12.0, 0, fy_bridge_bot)
    pat_lip.add_color_stop_rgba(0.0, 0.0, 0.0, 0.0, 0.0)
    pat_lip.add_color_stop_rgba(0.65, 0.0, 0.0, 0.0, 0.28)
    pat_lip.add_color_stop_rgba(1.0, 1.0, 0.98, 0.92, 0.14)
    cr.set_source(pat_lip)
    cr.rectangle(fx_l, fy_bridge_bot - 12.0, fx_r - fx_l, 12.0)
    cr.fill()
    cr.restore()

    cr.set_source_rgba(1.0, 0.98, 0.92, 0.30)
    cr.set_line_width(0.95)
    cr.stroke()

    # --- Contoured Glossy-Black QUANSHENG Brow Badge + Dynamic RF-Arc LED Anchor ---
    arc_ox, arc_oy, logo_sc = draw_k10_logo_badge(cr, bx + bw * 0.51, by + 23.5, scale=0.90)
    rend._led_anchor = (arc_ox, arc_oy, logo_sc, 0.0, False)

    # --- Deep Chamfered Titanium Rim + Glossy-Black LCD Lens Window ---
    lz_w = 216
    lz_h = 154
    lz_x = int(bx + (bw - lz_w) * 0.5)
    lz_y = int(by + 48)

    # Outer 3D sloped titanium chamfer framing the screen
    round_rect(cr, lz_x - 7.5, lz_y - 7.5, lz_w + 15.0, lz_h + 15.0, 14.0)
    pat_ch = cairo.LinearGradient(lz_x, lz_y - 7.5, lz_x, lz_y + lz_h + 7.5)
    pat_ch.add_color_stop_rgba(0.0, 0.0, 0.0, 0.0, 0.56)
    pat_ch.add_color_stop_rgba(0.45, 0.0, 0.0, 0.0, 0.18)
    pat_ch.add_color_stop_rgba(0.88, 1.0, 0.98, 0.92, 0.24)
    pat_ch.add_color_stop_rgba(1.0, 1.0, 0.98, 0.92, 0.40)
    cr.set_source(pat_ch)
    cr.fill()

    # Deep glossy-black bezel lens (generous black glass border like real UV-K10 Pro photos)
    round_rect(cr, lz_x, lz_y, lz_w, lz_h, 9.5)
    pat_lens = cairo.LinearGradient(lz_x, lz_y, lz_x + lz_w * 0.6, lz_y + lz_h)
    pat_lens.add_color_stop_rgb(0.0, 0.09, 0.095, 0.11)
    pat_lens.add_color_stop_rgb(0.35, 0.04, 0.045, 0.052)
    pat_lens.add_color_stop_rgb(1.0, 0.025, 0.028, 0.035)
    cr.set_source(pat_lens)
    cr.fill_preserve()

    # Subtle diagonal glass sheen across the upper-left corner of the glossy black bezel
    cr.save()
    cr.clip_preserve()
    cr.new_sub_path()
    cr.move_to(lz_x, lz_y)
    cr.line_to(lz_x + lz_w * 0.52, lz_y)
    cr.line_to(lz_x + lz_w * 0.22, lz_y + lz_h)
    cr.line_to(lz_x, lz_y + lz_h)
    cr.close_path()
    pat_gl = cairo.LinearGradient(lz_x, lz_y, lz_x + lz_w * 0.35, lz_y + lz_h)
    pat_gl.add_color_stop_rgba(0.0, 1.0, 1.0, 1.0, 0.09)
    pat_gl.add_color_stop_rgba(0.55, 1.0, 1.0, 1.0, 0.02)
    pat_gl.add_color_stop_rgba(1.0, 1.0, 1.0, 1.0, 0.0)
    cr.set_source(pat_gl)
    cr.fill()
    cr.restore()

    cr.set_source_rgba(1.0, 0.99, 0.96, 0.20)
    cr.set_line_width(1.0)
    cr.stroke()

    # Inner recessed LCD active glass aperture (authentic thick glossy black border around active LCD)
    pad_x, pad_y = 14, 14
    act_x, act_y, act_w, act_h = lz_x + pad_x, lz_y + pad_y, lz_w - pad_x * 2, lz_h - pad_y * 2
    if lcd_st.backlight_glow and lcd_st.backlight > 0:
        bg_c, _, _, _ = _lcd_colors_norm(lcd_st)
        cr.set_source_rgba(bg_c[0], bg_c[1], bg_c[2], 0.16)
        round_rect(cr, act_x - 2.0, act_y - 2.0, act_w + 4.0, act_h + 4.0, 4.0)
        cr.set_line_width(2.4)
        cr.stroke()
    rend._lcd_rect = (act_x, act_y, act_w, act_h)

    # --- Integrated 3-Key Metallic-Rimmed Quick Bar (`[A|B]`, `V/M`, `Band`) ---
    qk_y = lz_y + lz_h + 15.0
    bar_x = fx_l + 15.0
    bar_w = (fx_r - fx_l) - 30.0
    bar_h = 31.0

    # Deeply beveled recessed pill pocket holding the 3 closely abutted quick keys
    round_rect(cr, bar_x - 3.5, qk_y - 3.5, bar_w + 7.0, bar_h + 7.0, 9.5)
    pat_qp = cairo.LinearGradient(0, qk_y - 3.5, 0, qk_y + bar_h + 3.5)
    pat_qp.add_color_stop_rgba(0.0, 0.0, 0.0, 0.0, 0.62)
    pat_qp.add_color_stop_rgba(0.85, 0.0, 0.0, 0.0, 0.25)
    pat_qp.add_color_stop_rgba(1.0, 1.0, 0.98, 0.92, 0.30)
    cr.set_source(pat_qp)
    cr.fill()

    round_rect(cr, bar_x - 1.5, qk_y - 1.5, bar_w + 3.0, bar_h + 3.0, 7.5)
    cr.set_source_rgb(0.035, 0.04, 0.048)
    cr.fill()

    q_gap = 4.0
    qk_w = (bar_w - q_gap * 2.0) / 3.0
    for qi, qtxt in enumerate(("A/B", "V/M", "Band")):
        qx = bar_x + qi * (qk_w + q_gap)
        qy = qk_y + 1.0
        qh = bar_h - 2.0
        # Outer keys curve cleanly with the pill well; inner edges stay crisp
        r_l = 6.0 if qi == 0 else 2.2
        r_r = 6.0 if qi == 2 else 2.2
        cr.new_sub_path()
        cr.move_to(qx + r_l, qy)
        cr.line_to(qx + qk_w - r_r, qy)
        cr.arc(qx + qk_w - r_r, qy + r_r, r_r, -math.pi * 0.5, 0.0)
        cr.line_to(qx + qk_w, qy + qh - r_r)
        cr.arc(qx + qk_w - r_r, qy + qh - r_r, r_r, 0.0, math.pi * 0.5)
        cr.line_to(qx + r_l, qy + qh)
        cr.arc(qx + r_l, qy + qh - r_l, r_l, math.pi * 0.5, math.pi)
        cr.line_to(qx, qy + r_l)
        cr.arc(qx + r_l, qy + r_l, r_l, math.pi, math.pi * 1.5)
        cr.close_path()

        pq = cairo.LinearGradient(qx, qy, qx, qy + qh)
        pq.add_color_stop_rgb(0.00, 0.25, 0.265, 0.295)
        pq.add_color_stop_rgb(0.32, 0.16, 0.17, 0.19)
        pq.add_color_stop_rgb(1.00, 0.095, 0.105, 0.12)
        cr.set_source(pq)
        cr.fill_preserve()
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.18)
        cr.set_line_width(0.8)
        cr.stroke()

        if qtxt == "A/B":
            # Authentic UV-K10 Pro split white [A] / black [B] badge
            bw_b, bh_b = 31.0, 16.0
            bx_b = qx + (qk_w - bw_b) * 0.5
            by_b = qy + (qh - bh_b) * 0.5
            round_rect(cr, bx_b, by_b, bw_b, bh_b, 2.5)
            cr.set_source_rgb(0.94, 0.96, 0.98)
            cr.fill_preserve()
            cr.set_source_rgba(0.94, 0.96, 0.98, 0.95)
            cr.set_line_width(1.1)
            cr.stroke()
            cr.rectangle(bx_b + bw_b * 0.5, by_b + 1.1, bw_b * 0.5 - 1.1, bh_b - 2.2)
            cr.set_source_rgb(0.09, 0.10, 0.12)
            cr.fill()
            draw_pango_text(cr, "A", bx_b + bw_b * 0.25, by_b + bh_b * 0.5, font_desc="Adwaita Sans Bold 8.0", rgba=(0.07, 0.08, 0.10, 0.98), halign="center", valign="center")
            draw_pango_text(cr, "B", bx_b + bw_b * 0.75, by_b + bh_b * 0.5, font_desc="Adwaita Sans Bold 8.0", rgba=(0.95, 0.97, 0.99, 0.98), halign="center", valign="center")
        else:
            col = (0.44, 0.80, 0.98, 0.96) if qtxt == "Band" else (0.90, 0.93, 0.97, 0.96)
            draw_pango_text(cr, qtxt, qx + qk_w * 0.5, qy + qh * 0.5, font_desc="Adwaita Sans Bold 8.4", rgba=col, halign="center", valign="center")

    # --- Sculpted Recessed Horizontal Body-Color Accent / Acoustic Bridge (exact match to real UV-K10 Pro!) ---
    acc_y = qk_y + bar_h + 12.0
    acc_x = bar_x - 1.0
    acc_w = bar_w + 2.0
    acc_h = 16.5

    # Deep recessed shadow channel around the sculpted horizontal pill bar
    round_rect(cr, acc_x - 2.0, acc_y - 2.5, acc_w + 4.0, acc_h + 5.0, 9.5)
    cr.set_source_rgb(0.035, 0.04, 0.048)
    cr.fill()
    # Bottom metallic lip catchlight under channel
    cr.set_source_rgba(1.0, 0.98, 0.92, 0.26)
    cr.set_line_width(0.8)
    cr.move_to(acc_x + 8.0, acc_y + acc_h + 3.0)
    cr.line_to(acc_x + acc_w - 8.0, acc_y + acc_h + 3.0)
    cr.stroke()

    # Raised beveled slate-blue/gunmetal horizontal bar inside the acoustic channel
    round_rect(cr, acc_x + 1.5, acc_y + 1.5, acc_w - 3.0, acc_h - 3.0, 6.5)
    pat_bar = cairo.LinearGradient(acc_x, acc_y, acc_x, acc_y + acc_h)
    pat_bar.add_color_stop_rgb(0.00, 0.34, 0.41, 0.52)
    pat_bar.add_color_stop_rgb(0.45, 0.22, 0.27, 0.36)
    pat_bar.add_color_stop_rgb(1.00, 0.13, 0.17, 0.24)
    cr.set_source(pat_bar)
    cr.fill_preserve()
    cr.set_source_rgba(0.82, 0.90, 1.0, 0.24)
    cr.set_line_width(0.8)
    cr.stroke()

    # Subtle recessed acoustic micro-slot line below the pill bar
    round_rect(cr, acc_x + 10.0, acc_y + acc_h - 0.5, acc_w - 20.0, 2.2, 1.0)
    cr.set_source_rgba(0.02, 0.025, 0.03, 0.85)
    cr.fill()

    # --- Sculpted Matte-Black 4×4 Keypad Tray (extends cleanly down to the bottom slate-blue rim!) ---
    kp_x = fx_l + 9.0
    kp_y = fy_bridge_bot + 10.0
    kp_w = (fx_r - fx_l) - 18.0
    gap_c = 4.5
    gap_r = 6.5
    kw = (kp_w - gap_c * 3.0) / 4.0
    kh_nav = 29.5
    kh_num = 36.0
    total_kp_h = kh_nav + 3.0 * kh_num + 3.0 * gap_r

    # Chamfered matte-black keypad surround tray
    round_rect(cr, kp_x - 5.5, kp_y - 5.5, kp_w + 11.0, total_kp_h + 11.0, 11.0)
    pat_tray = cairo.LinearGradient(0, kp_y - 5.5, 0, kp_y + total_kp_h + 5.5)
    pat_tray.add_color_stop_rgb(0.0, 0.075, 0.08, 0.095)
    pat_tray.add_color_stop_rgb(1.0, 0.045, 0.05, 0.06)
    cr.set_source(pat_tray)
    cr.fill_preserve()
    cr.set_source_rgba(0.80, 0.88, 1.0, 0.14)
    cr.set_line_width(0.85)
    cr.stroke()

    # Crisp horizontal metallic-charcoal divider ribs between keypad rows (signature UV-K10 Pro tactile strips!)
    for r_i in range(3):
        ry_bar = kp_y + (kh_nav if r_i == 0 else kh_nav + r_i * (kh_num + gap_r)) + 2.0
        round_rect(cr, kp_x + 2.0, ry_bar, kp_w - 4.0, 2.4, 1.0)
        pat_div = cairo.LinearGradient(kp_x, ry_bar, kp_x + kp_w, ry_bar)
        pat_div.add_color_stop_rgb(0.0, 0.12, 0.14, 0.17)
        pat_div.add_color_stop_rgb(0.5, 0.24, 0.27, 0.32)
        pat_div.add_color_stop_rgb(1.0, 0.12, 0.14, 0.17)
        cr.set_source(pat_div)
        cr.fill()

    corner_map = {(0, 0): "k10-tl", (0, 3): "k10-tr", (3, 0): "k10-bl", (3, 3): "k10-br"}
    for r_i, row_keys in enumerate(K10_PRO_MATRIX):
        ky = kp_y + (0.0 if r_i == 0 else (kh_nav + gap_r + (r_i - 1) * (kh_num + gap_r)))
        kh = kh_nav if r_i == 0 else kh_num
        for c_i, (kn, pri, s1, s2) in enumerate(row_keys):
            kx = kp_x + c_i * (kw + gap_c)
            rend.hitboxes[kn] = (kx, ky, kw, kh)
            kstyle = corner_map.get((r_i, c_i), "k10")
            rend._key_specs.append((kn, kx, ky, kw, kh, pri, s1, s2, 3.2, r_i == 0, kn == "5", kstyle))
