"""Photorealistic Cairo + PangoCairo hardware primitives for all authentic Quansheng chassis families."""

from __future__ import annotations

import math
from typing import Any

from ..paint import clamp1, round_rect

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


def draw_pango_text(
    cr: Any,
    text: str,
    x: float,
    y: float,
    *,
    font_desc: str,
    rgba: tuple[float, float, float, float],
    halign: str = "left",
    valign: str = "center",
) -> tuple[float, float]:
    """Crisp PangoCairo text renderer with clean Cairo fallback for headless environments."""
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


def draw_quansheng_logo(cr: Any, cx: float, cy: float, scale: float = 1.0, *, dark: bool = False) -> None:
    """Exact Quansheng dual-RF-arc + heavy italic geometric wordmark (`inspo/Quansheng-Logo.png`)."""
    import cairo

    cr.save()
    cr.translate(cx, cy)
    cr.scale(scale, scale)
    rgba = (0.08, 0.09, 0.11, 0.96) if dark else (0.95, 0.96, 0.98, 0.98)

    font_str = "Adwaita Sans Heavy Italic 12.0"
    if _ensure_pango():
        layout = _PANGOCAIRO_MOD.create_layout(cr)
        layout.set_font_description(_PANGO_MOD.FontDescription.from_string(font_str))
        layout.set_text("QUANSHENG", -1)
        tw, th = layout.get_pixel_size()
        w_scale = 1.08
        icon_w = 18.5
        total_w = icon_w + tw * w_scale
        x0 = -total_w * 0.5
        tx = x0 + icon_w

        cr.set_source_rgba(*rgba)
        cr.set_line_cap(cairo.LINE_CAP_BUTT)
        ox, oy = tx + 4.8, 5.2
        cr.new_path()
        cr.set_line_width(3.0)
        cr.arc(ox, oy, 11.0, math.pi * 1.07, math.pi * 1.43)
        cr.stroke()
        cr.new_path()
        cr.set_line_width(3.5)
        cr.arc(ox, oy, 16.2, math.pi * 1.02, math.pi * 1.46)
        cr.stroke()

        cr.save()
        cr.translate(tx, -th * 0.47)
        cr.scale(w_scale, 1.0)
        cr.set_source_rgba(*rgba)
        _PANGOCAIRO_MOD.show_layout(cr, layout)
        cr.restore()
    else:
        cr.set_source_rgba(*rgba)
        cr.select_font_face("Sans", cairo.FONT_SLANT_ITALIC, cairo.FONT_WEIGHT_BOLD)
        cr.set_font_size(14.0)
        ext = cr.text_extents("QUANSHENG")
        tx = -(ext.width + 18.0) * 0.5 + 18.0
        cr.set_line_cap(cairo.LINE_CAP_BUTT)
        cr.new_path()
        cr.set_line_width(2.9)
        cr.arc(tx + 5.0, 5.0, 10.8, math.pi * 1.07, math.pi * 1.43)
        cr.stroke()
        cr.new_path()
        cr.set_line_width(3.4)
        cr.arc(tx + 5.0, 5.0, 15.8, math.pi * 1.02, math.pi * 1.46)
        cr.stroke()
        cr.move_to(tx, ext.height * 0.36)
        cr.show_text("QUANSHENG")
    cr.restore()


def draw_phillips_screw(cr: Any, cx: float, cy: float, r: float = 7.0) -> None:
    """Machined silver Phillips pan-head PCB mounting screw with crisp cross-slot."""
    import cairo

    cr.new_path()
    cr.arc(cx, cy + 0.8, r + 1.2, 0, 2 * math.pi)
    cr.set_source_rgba(0.0, 0.0, 0.0, 0.46)
    cr.fill()

    pat = cairo.RadialGradient(cx - r * 0.32, cy - r * 0.32, 0.6, cx, cy, r)
    pat.add_color_stop_rgb(0.0, 0.98, 0.98, 0.99)
    pat.add_color_stop_rgb(0.42, 0.78, 0.80, 0.82)
    pat.add_color_stop_rgb(0.80, 0.50, 0.52, 0.54)
    pat.add_color_stop_rgb(1.0, 0.30, 0.32, 0.34)
    cr.new_path()
    cr.arc(cx, cy, r, 0, 2 * math.pi)
    cr.set_source(pat)
    cr.fill_preserve()
    cr.set_source_rgba(0.10, 0.12, 0.14, 0.60)
    cr.set_line_width(0.75)
    cr.stroke()

    arm = r * 0.58
    cr.set_source_rgb(0.16, 0.17, 0.19)
    cr.set_line_width(max(1.5, r * 0.25))
    cr.set_line_cap(cairo.LINE_CAP_ROUND)
    cr.new_path()
    cr.move_to(cx - arm, cy)
    cr.line_to(cx + arm, cy)
    cr.move_to(cx, cy - arm)
    cr.line_to(cx, cy + arm)
    cr.stroke()


def draw_hex_torx_bolt(cr: Any, cx: float, cy: float, r: float = 5.8) -> None:
    """3D countersunk machined stainless hex-socket bolt with concentric chamfer collar."""
    import cairo

    cr.set_source_rgba(0.03, 0.035, 0.045, 0.96)
    cr.arc(cx, cy, r + 2.2, 0, 2 * math.pi)
    cr.fill()

    cr.set_source_rgba(1.0, 0.99, 0.96, 0.26)
    cr.set_line_width(0.85)
    cr.arc(cx, cy, r + 2.3, math.pi * 0.12, math.pi * 0.88)
    cr.stroke()

    pat = cairo.RadialGradient(cx - r * 0.34, cy - r * 0.34, 0.5, cx, cy, r)
    pat.add_color_stop_rgb(0.0, 0.97, 0.96, 0.94)
    pat.add_color_stop_rgb(0.45, 0.78, 0.77, 0.74)
    pat.add_color_stop_rgb(0.82, 0.44, 0.43, 0.41)
    pat.add_color_stop_rgb(1.0, 0.24, 0.23, 0.22)
    cr.set_source(pat)
    cr.arc(cx, cy, r, 0, 2 * math.pi)
    cr.fill()

    hr = r * 0.48
    cr.new_sub_path()
    for i in range(6):
        ang = i * (math.pi / 3.0) + math.pi / 6.0
        px, py = cx + hr * math.cos(ang), cy + hr * math.sin(ang)
        cr.move_to(px, py) if i == 0 else cr.line_to(px, py)
    cr.close_path()
    cr.set_source_rgb(0.13, 0.13, 0.14)
    cr.fill()


def draw_frosted_led(cr: Any, x: float, y: float, w: float, h: float, state: str, *, rectangular: bool = True) -> None:
    """Translucent frosted polycarbonate light-pipe LED driven by BK4829 GPIO REG_0x33."""
    import cairo

    round_rect(cr, x - 1.5, y - 1.0, w + 3.0, h + 2.2, 3.0 if rectangular else 3.6)
    cr.set_source_rgb(0.06, 0.07, 0.08)
    cr.fill()

    round_rect(cr, x, y + 0.3, w, h - 0.6, 2.2)
    cx, cy = x + w * 0.5, y + h * 0.52
    if state in ("rx", "green"):
        core, mid, outer = (0.84, 1.0, 0.88), (0.12, 0.94, 0.42), (0.04, 0.52, 0.20)
    elif state in ("tx", "red"):
        core, mid, outer = (1.0, 0.84, 0.82), (0.98, 0.20, 0.20), (0.62, 0.06, 0.06)
    elif state in ("dfu", "orange"):
        core, mid, outer = (1.0, 0.95, 0.74), (0.98, 0.62, 0.08), (0.68, 0.34, 0.02)
    else:
        core, mid, outer = (0.90, 0.92, 0.94), (0.74, 0.76, 0.79), (0.50, 0.53, 0.56)

    pat = cairo.RadialGradient(cx, cy - 0.8, 0.8, cx, cy, w * 0.68)
    pat.add_color_stop_rgb(0.0, *core)
    pat.add_color_stop_rgb(0.55, *mid)
    pat.add_color_stop_rgb(1.0, *outer)
    cr.set_source(pat)
    cr.fill_preserve()
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.40)
    cr.set_line_width(0.8)
    cr.stroke()

    cr.new_path()
    cr.move_to(x + 2.5, y + 1.8)
    cr.line_to(x + w - 2.5, y + 1.8)
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.64 if state != "off" else 0.34)
    cr.set_line_width(1.0)
    cr.stroke()

    if state in ("rx", "green", "tx", "red", "dfu", "orange"):
        bloom = cairo.RadialGradient(cx, cy, 1.5, cx, cy, 24.0)
        bloom.add_color_stop_rgba(0.0, mid[0], mid[1], mid[2], 0.62)
        bloom.add_color_stop_rgba(0.42, mid[0], mid[1], mid[2], 0.22)
        bloom.add_color_stop_rgba(1.0, mid[0], mid[1], mid[2], 0.0)
        cr.set_source(bloom)
        cr.new_path()
        cr.arc(cx, cy, 24.0, 0, 2 * math.pi)
        cr.fill()


def draw_top_flashlight(
    cr: Any, tx: float, by: float, torch_on: bool, *, compact: bool = False, bare_led: bool = False
) -> None:
    """3D top flashlight turret housing (or bare 5mm glass LED beam on PCB view) with realistic directional beam."""
    import cairo

    hw = 13.5 if compact else 16.0
    hh = 12.5 if compact else 15.0
    ty = by - (10.0 if compact else 12.0)

    if torch_on:
        cr.save()
        cr.new_path()
        cr.move_to(tx - hw * 0.60, ty + 2)
        cr.line_to(tx - hw * 2.5, 0)
        cr.line_to(tx + hw * 2.5, 0)
        cr.line_to(tx + hw * 0.60, ty + 2)
        cr.close_path()
        beam = cairo.LinearGradient(tx, ty + 2, tx, 0)
        beam.add_color_stop_rgba(0.0, 1.0, 0.99, 0.90, 0.88)
        beam.add_color_stop_rgba(0.45, 1.0, 0.97, 0.82, 0.42)
        beam.add_color_stop_rgba(1.0, 1.0, 0.95, 0.78, 0.0)
        cr.set_source(beam)
        cr.fill()
        tg = cairo.RadialGradient(tx, ty + 2, 1.5, tx, ty + 2, 34.0)
        tg.add_color_stop_rgba(0.0, 1.0, 1.0, 0.96, 1.0)
        tg.add_color_stop_rgba(0.32, 1.0, 0.98, 0.84, 0.60)
        tg.add_color_stop_rgba(1.0, 1.0, 0.95, 0.75, 0.0)
        cr.set_source(tg)
        cr.new_path()
        cr.arc(tx, ty + 2, 34.0, 0, 2 * math.pi)
        cr.fill()
        cr.restore()

    if bare_led:
        return

    pat_fl = cairo.LinearGradient(tx - hw, ty, tx + hw, ty + hh)
    pat_fl.add_color_stop_rgb(0.0, 0.25, 0.26, 0.29)
    pat_fl.add_color_stop_rgb(1.0, 0.08, 0.09, 0.11)
    cr.set_source(pat_fl)
    round_rect(cr, tx - hw, ty, hw * 2.0, hh, 5.0)
    cr.fill()

    round_rect(cr, tx - hw * 0.56, ty + 1.5, hw * 1.12, 4.5, 2.2)
    if torch_on:
        cr.set_source_rgba(1.0, 1.0, 0.96, 0.98)
    else:
        cr.set_source_rgba(0.66, 0.70, 0.76, 0.40)
    cr.fill()


def _draw_key_glyph(cr: Any, cx: float, cy: float, w: float = 10.6, h: float = 5.4) -> None:
    """Compact horizontal padlock-key glyph matching factory UV-K1(8) silkscreen (ring right, teeth left)."""
    import cairo

    cr.save()
    cr.set_source_rgba(0.93, 0.95, 0.98, 0.96)
    cr.set_line_width(1.15)
    cr.set_line_cap(cairo.LINE_CAP_ROUND)
    cr.set_line_join(cairo.LINE_JOIN_ROUND)
    x0 = cx - w * 0.5
    r = h * 0.36
    ring_cx = x0 + w - r - 0.5
    cr.new_path()
    cr.arc(ring_cx, cy, r, 0, 2 * math.pi)
    cr.stroke()
    cr.new_path()
    cr.move_to(ring_cx - r, cy)
    cr.line_to(x0 + 0.5, cy)
    cr.move_to(x0 + 1.5, cy)
    cr.line_to(x0 + 1.5, cy + h * 0.44)
    cr.move_to(x0 + 3.6, cy)
    cr.line_to(x0 + 3.6, cy + h * 0.34)
    cr.stroke()
    cr.restore()


def _draw_boxed_badge(cr: Any, letter: str, cx: float, cy: float, size: float = 9.0) -> None:
    round_rect(cr, cx - size * 0.5, cy - size * 0.5, size, size, 2.0)
    cr.set_source_rgba(0.94, 0.95, 0.97, 0.95)
    cr.fill()
    draw_pango_text(
        cr, letter, cx, cy, font_desc="Adwaita Sans Bold 6.0", rgba=(0.08, 0.08, 0.09, 0.98), halign="center", valign="center"
    )


def draw_k1_keycap(
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
    is_nav: bool = False,
    homing_pip: bool = False,
    accent: tuple[float, float, float] = (0.18, 0.56, 1.0),
) -> None:
    """
    Authentic Quansheng UV-K1 flush-milled satin-rubber keycap (`K1_VIEW_E_FRONT_BACK.png`):
      • Tight machined armor aperture (crisp 3.8px radius, NO bloated cushion frames!)
      • Flat-satin dark charcoal silicone top with subtle upper-edge specular catchlight
      • Crisp high-contrast silkscreen matching factory K1 proportions (wide < > chevrons,
        large left-aligned geometric digits, neat 2-line right legends, subtle homing pip on 5)
    """
    import cairo

    p = clamp1(press_t)
    hv = clamp1(hover_t)
    rad = 4.0

    # Deep dark aperture pocket milled cleanly through the metallic armor plate
    round_rect(cr, x - 0.8, y - 0.8, w + 1.6, h + 1.6, rad + 0.8)
    cr.set_source_rgb(0.03, 0.032, 0.038)
    cr.fill()

    # Subtle bottom-edge metallic chamfer reflection beneath aperture
    cr.set_source_rgba(1.0, 0.98, 0.94, 0.18)
    cr.set_line_width(0.75)
    cr.move_to(x + rad, y + h + 1.1)
    cr.line_to(x + w - rad, y + h + 1.1)
    cr.stroke()

    if p > 0.02:
        round_rect(cr, x - 1.3, y - 1.3, w + 2.6, h + 2.6, rad + 1.3)
        cr.set_source_rgba(accent[0], accent[1], accent[2], 0.36 * p)
        cr.fill()

    # Tight silicone dome keytop sitting almost flush inside the metallic window
    inset = 1.3
    dy = 1.5 * p
    ix = x + inset + 0.25 * p
    iy = y + inset + dy - 0.35 * (1.0 - p)
    iw = w - inset * 2.0 - 0.5 * p
    ih = h - inset * 2.0 - 0.3 * p
    ir = max(2.6, rad - 0.8)

    if p < 0.95:
        round_rect(cr, ix, iy + 1.6 * (1.0 - p), iw, ih, ir)
        cr.set_source_rgba(0.0, 0.0, 0.0, 0.68 * (1.0 - p))
        cr.fill()

    round_rect(cr, ix, iy, iw, ih, ir)
    top_v = 0.175 + 0.055 * hv - 0.06 * p
    mid_v = 0.135 + 0.040 * hv - 0.04 * p
    bot_v = 0.105 + 0.025 * hv - 0.02 * p
    pat = cairo.LinearGradient(ix, iy, ix, iy + ih)
    pat.add_color_stop_rgb(0.0, top_v, top_v, top_v + 0.008)
    pat.add_color_stop_rgb(0.45, mid_v, mid_v, mid_v + 0.008)
    pat.add_color_stop_rgb(1.0, bot_v, bot_v, bot_v + 0.008)
    cr.set_source(pat)
    cr.fill_preserve()

    if p > 0.02:
        cr.set_source_rgba(accent[0], accent[1], accent[2], 0.92 * p)
        cr.set_line_width(1.3)
    else:
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.13 + 0.10 * hv)
        cr.set_line_width(0.8)
    cr.stroke()

    # Authentic tiny molded tactile homing pip at bottom-center edge of key 5 (zero overlap!)
    if homing_pip:
        cr.arc(ix + iw * 0.5, iy + ih - 2.4, 1.15, 0, 2 * math.pi)
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.32)
        cr.fill()

    ink = (0.96, 0.97, 0.99, 0.98)
    sub1_ink = (0.91, 0.93, 0.96, 0.94)
    sub2_ink = (0.78, 0.81, 0.86, 0.86)

    if primary in ("_K1_LT_", "_K1_GT_"):
        cr.save()
        cr.set_source_rgba(*ink)
        cr.set_line_width(2.0)
        cr.set_line_cap(cairo.LINE_CAP_ROUND)
        cr.set_line_join(cairo.LINE_JOIN_ROUND)
        cx, cy = ix + iw * 0.5, iy + ih * 0.5
        if primary == "_K1_LT_":
            cr.move_to(cx + 4.8, cy - 6.8)
            cr.line_to(cx - 4.4, cy)
            cr.line_to(cx + 4.8, cy + 6.8)
        else:
            cr.move_to(cx - 4.8, cy - 6.8)
            cr.line_to(cx + 4.4, cy)
            cr.line_to(cx - 4.8, cy + 6.8)
        cr.stroke()
        cr.restore()
    elif is_nav or (not sub1 and not sub2):
        draw_pango_text(
            cr, primary, ix + iw * 0.5, iy + ih * 0.5,
            font_desc="Adwaita Sans Bold 8.8", rgba=ink, halign="center", valign="center",
        )
    else:
        # Exact K1(8) silkscreen proportions (`K1_VIEW_D_FRONT.png`):
        # Crisp left-aligned primary digit/symbol + cleanly aligned right legend column
        p_size = "15.2" if primary == "*" else "13.6"
        p_x = ix + 4.8
        p_y = iy + ih * (0.55 if primary == "*" else 0.50)
        draw_pango_text(
            cr, primary, p_x, p_y, font_desc=f"Adwaita Sans Medium {p_size}", rgba=ink, halign="left", valign="center"
        )
        sx = ix + iw * 0.44
        rx_mid = ix + iw * 0.70

        if sub1 and sub2:
            if sub1 == "_KEY_":
                # Factory K1 '#' key: compact key glyph centered directly above 'F'
                _draw_key_glyph(cr, rx_mid, iy + ih * 0.31, 10.6, 5.4)
                draw_pango_text(
                    cr, sub2, rx_mid, iy + ih * 0.69,
                    font_desc="Adwaita Sans Medium 6.8", rgba=sub1_ink, halign="center", valign="center",
                )
            elif sub2 == "_RET_":
                # Factory K1 '*' key: 'Scan' above compact return/reverse arrow
                draw_pango_text(cr, sub1, rx_mid, iy + ih * 0.31, font_desc="Adwaita Sans Medium 6.4", rgba=sub1_ink, halign="center", valign="center")
                cr.save()
                cr.set_source_rgba(*sub1_ink)
                cr.set_line_width(1.15)
                cr.set_line_cap(cairo.LINE_CAP_ROUND)
                cr.set_line_join(cairo.LINE_JOIN_ROUND)
                ry = iy + ih * 0.70
                ax0, ax1 = rx_mid - 5.2, rx_mid + 5.2
                cr.new_path()
                cr.move_to(ax1, ry - 2.6)
                cr.line_to(ax1, ry)
                cr.line_to(ax0, ry)
                cr.move_to(ax0 + 2.6, ry - 1.9)
                cr.line_to(ax0, ry)
                cr.line_to(ax0 + 2.6, ry + 1.9)
                cr.stroke()
                cr.restore()
            else:
                draw_pango_text(cr, sub1, sx, iy + ih * 0.33, font_desc="Adwaita Sans Medium 6.5", rgba=sub1_ink, halign="left", valign="center")
                draw_pango_text(cr, sub2, sx, iy + ih * 0.68, font_desc="Adwaita Sans 6.0", rgba=sub2_ink, halign="left", valign="center")
        elif sub1:
            draw_pango_text(cr, sub1, sx, iy + ih * 0.50, font_desc="Adwaita Sans Medium 7.2", rgba=sub1_ink, halign="left", valign="center")


def draw_3d_keycap(
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
    radius: float = 10.0,
    is_nav: bool = False,
    homing_nub: bool = False,
    accent: tuple[float, float, float] = (0.98, 0.46, 0.06),
) -> None:
    """Pillowed oval/lozenge rubber keycap for UV-K5 / UV-K5v3 / UV-K6 / UV-5R Plus."""
    import cairo

    p = clamp1(press_t)
    hv = clamp1(hover_t)

    round_rect(cr, x - 0.9, y + 0.9, w + 1.8, h + 1.1, radius + 1.2)
    cr.set_source_rgba(1.0, 0.99, 0.96, 0.18)
    cr.fill()

    round_rect(cr, x - 1.0, y - 1.0, w + 2.0, h + 2.0, radius + 1.0)
    cr.set_source_rgb(0.035, 0.04, 0.048)
    cr.fill()

    if p > 0.02:
        round_rect(cr, x - 1.5, y - 1.5, w + 3.0, h + 3.0, radius + 1.5)
        cr.set_source_rgba(accent[0], accent[1], accent[2], 0.34 * p)
        cr.fill()

    gasket = 2.1
    dy = 1.6 * p
    ix = x + gasket + 0.3 * p
    iy = y + gasket + dy - 0.4 * (1.0 - p)
    iw = w - gasket * 2.0 - 0.6 * p
    ih = h - gasket * 2.0 - 0.35 * p
    ir = max(3.0, radius - 1.4)

    if p < 0.95:
        round_rect(cr, ix, iy + 1.8 * (1.0 - p), iw, ih, ir)
        cr.set_source_rgba(0.0, 0.0, 0.0, 0.72 * (1.0 - p))
        cr.fill()

    round_rect(cr, ix, iy, iw, ih, ir)
    t_l = 0.23 + 0.06 * hv - 0.09 * p
    m_l = 0.17 + 0.04 * hv - 0.05 * p
    b_l = 0.12 + 0.025 * hv - 0.02 * p
    pat = cairo.LinearGradient(ix, iy, ix, iy + ih)
    pat.add_color_stop_rgb(0.0, t_l, t_l, t_l + 0.01)
    pat.add_color_stop_rgb(0.45, m_l, m_l, m_l + 0.01)
    pat.add_color_stop_rgb(1.0, b_l, b_l, b_l + 0.01)
    cr.set_source(pat)
    cr.fill_preserve()

    if p > 0.02:
        cr.set_source_rgba(accent[0], accent[1], accent[2], 0.92 * p)
        cr.set_line_width(1.35)
    else:
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.15 + 0.10 * hv)
        cr.set_line_width(0.9)
    cr.stroke()

    if homing_nub:
        cr.arc(ix + iw * 0.5, iy + ih - 2.3, 1.1, 0, 2 * math.pi)
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.28)
        cr.fill()

    ink = (0.95, 0.96, 0.98, 0.98)
    sub_ink = (0.90, 0.92, 0.95, 0.92)
    sub2_ink = (0.78, 0.81, 0.86, 0.85)

    if primary in ("_CHEV_UP_", "_CHEV_DN_"):
        cr.save()
        cr.set_source_rgba(*ink)
        cr.set_line_width(2.2)
        cr.set_line_cap(cairo.LINE_CAP_ROUND)
        cr.set_line_join(cairo.LINE_JOIN_ROUND)
        cx = ix + iw * 0.5
        if primary == "_CHEV_UP_":
            cy2 = iy + ih * (0.40 if sub1 else 0.50)
            cr.move_to(cx - 8.0, cy2 + 3.5)
            cr.line_to(cx, cy2 - 4.3)
            cr.line_to(cx + 8.0, cy2 + 3.5)
        else:
            cy2 = iy + ih * (0.60 if sub1 else 0.50)
            cr.move_to(cx - 8.0, cy2 - 3.5)
            cr.line_to(cx, cy2 + 4.3)
            cr.line_to(cx + 8.0, cy2 - 3.5)
        cr.stroke()
        cr.restore()
        if sub1 and sub1.startswith("[") and sub1.endswith("]") and len(sub1) == 3:
            _draw_boxed_badge(cr, sub1[1], ix + iw * 0.5, iy + ih * (0.75 if primary == "_CHEV_UP_" else 0.25), 8.6)
    elif is_nav or (not sub1 and not sub2):
        if sub1.startswith("[") and sub1.endswith("]") and len(sub1) == 3:
            is_exit = primary == "EXIT"
            draw_pango_text(
                cr, primary, ix + iw * (0.34 if is_exit else 0.36), iy + ih * 0.50,
                font_desc="Adwaita Sans Bold Italic 8.1" if is_exit else "Adwaita Sans Bold Italic 13.0",
                rgba=ink, halign="center", valign="center",
            )
            _draw_boxed_badge(cr, sub1[1], ix + iw * (0.80 if is_exit else 0.76), iy + ih * 0.50, 8.3 if is_exit else 9.0)
        else:
            draw_pango_text(cr, primary, ix + iw * 0.5, iy + ih * 0.5, font_desc="Adwaita Sans Bold 9.2", rgba=ink, halign="center", valign="center")
    else:
        is_compact = iw < 55.0
        p_size = ("12.4" if is_compact else "13.8") if primary != "*" else ("14.0" if is_compact else "15.2")
        p_x = ix + (4.8 if is_compact else 5.8)
        p_y = iy + ih * (0.55 if primary == "*" else 0.49)
        pw, _ = draw_pango_text(cr, primary, p_x, p_y, font_desc=f"Adwaita Sans Medium {p_size}", rgba=ink, halign="left", valign="center")
        sx = p_x + max(pw, 9.0) + 2.6

        if sub1 and sub2:
            if sub1 == "_KEY_":
                _draw_key_glyph(cr, sx, iy + ih * 0.14, 13.2, 8.0)
            else:
                draw_pango_text(cr, sub1, sx, iy + ih * 0.29, font_desc="Adwaita Sans Medium 6.0", rgba=sub_ink, halign="left", valign="center")
            if sub2 == "_KEY_":
                _draw_key_glyph(cr, sx, iy + ih * 0.56, 13.0, 7.8)
            else:
                draw_pango_text(cr, sub2, sx, iy + ih * 0.70, font_desc="Adwaita Sans 5.8", rgba=sub2_ink, halign="left", valign="center")
        elif sub1:
            if sub1 == "_KEY_":
                _draw_key_glyph(cr, sx + 8.5, iy + ih * 0.50, 13.0, 7.6)
            else:
                draw_pango_text(cr, sub1, sx, iy + ih * 0.50, font_desc="Adwaita Sans Medium 6.4", rgba=sub_ink, halign="left", valign="center")


def draw_k1_side_shroud(cr: Any, bx: float, by: float, bh: float, side_specs: list[tuple[str, float, float, float, float, bool, int]]) -> None:
    """Compact left polymer flank & sculpted side-button wells for UV-K1."""
    import cairo

    sh_x = bx - 14.5
    top_y = by + 10.0
    bot_y = by + bh - 10.0

    cr.new_path()
    cr.move_to(bx + 4, top_y - 3)
    cr.curve_to(bx - 10, top_y - 1, sh_x - 2, top_y + 10, sh_x - 2, top_y + 24)
    cr.line_to(sh_x - 2, bot_y - 22)
    cr.curve_to(sh_x - 2, bot_y - 8, bx - 10, bot_y + 4, bx + 4, bot_y + 6)
    cr.close_path()
    cr.set_source_rgba(0.0, 0.0, 0.0, 0.40)
    cr.fill()

    cr.new_path()
    cr.move_to(bx + 6, top_y - 4)
    cr.curve_to(bx - 8, top_y - 3, sh_x, top_y + 8, sh_x, top_y + 22)
    cr.line_to(sh_x, bot_y - 22)
    cr.curve_to(sh_x, bot_y - 8, bx - 8, bot_y + 3, bx + 6, bot_y + 4)
    cr.close_path()
    pat_cheek = cairo.LinearGradient(sh_x, 0, bx + 4, 0)
    pat_cheek.add_color_stop_rgb(0.0, 0.055, 0.06, 0.07)
    pat_cheek.add_color_stop_rgb(0.32, 0.145, 0.155, 0.175)
    pat_cheek.add_color_stop_rgb(0.72, 0.085, 0.09, 0.105)
    pat_cheek.add_color_stop_rgb(1.0, 0.035, 0.04, 0.045)
    cr.set_source(pat_cheek)
    cr.fill_preserve()
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.09)
    cr.set_line_width(0.9)
    cr.stroke()

    for _kn, sx, sy, sw, sh, is_ptt, _dots in side_specs:
        round_rect(cr, sx + 4.0, sy - 2.4, sw - 2.0, sh + 4.8, 8.0 if is_ptt else 6.0)
        cr.set_source_rgba(0.015, 0.015, 0.02, 0.90)
        cr.fill()


def draw_k5_side_shroud(cr: Any, bx: float, by: float, bh: float, side_specs: list[tuple[str, float, float, float, float, bool, int]]) -> None:
    """Sculpted UV-K5 / UV-K5v3 / UV-K6 matte polymer left side-guard & rear battery flank (`UVK6.3.png`)."""
    import cairo

    rear_x = bx - 7.5
    guard_x = bx - 14.5
    top_y = by + 16.0
    guard_top = by + 38.0
    waist_y = by + 298.0
    bot_y = by + bh - 12.0

    cr.new_path()
    cr.move_to(bx + 4, top_y)
    cr.curve_to(bx - 5, top_y + 2, rear_x - 2, guard_top - 4, rear_x - 2, guard_top + 6)
    cr.curve_to(guard_x - 2, guard_top + 12, guard_x - 2, guard_top + 20, guard_x - 2, guard_top + 28)
    cr.line_to(guard_x - 2, waist_y - 14)
    cr.curve_to(guard_x - 2, waist_y + 2, rear_x - 2, waist_y + 8, rear_x - 2, waist_y + 18)
    cr.line_to(rear_x - 2, bot_y - 18)
    cr.curve_to(rear_x - 2, bot_y - 4, bx - 5, bot_y + 4, bx + 4, bot_y + 6)
    cr.close_path()
    cr.set_source_rgba(0.0, 0.0, 0.0, 0.40)
    cr.fill()

    cr.new_path()
    cr.move_to(bx + 6, top_y - 2)
    cr.curve_to(bx - 4, top_y, rear_x, top_y + 14, rear_x, top_y + 28)
    cr.line_to(rear_x, bot_y - 20)
    cr.curve_to(rear_x, bot_y - 6, bx - 4, bot_y + 2, bx + 6, bot_y + 4)
    cr.close_path()
    pat_rear = cairo.LinearGradient(rear_x, 0, bx + 4, 0)
    pat_rear.add_color_stop_rgb(0.0, 0.07, 0.075, 0.085)
    pat_rear.add_color_stop_rgb(0.45, 0.145, 0.155, 0.17)
    pat_rear.add_color_stop_rgb(1.0, 0.04, 0.045, 0.055)
    cr.set_source(pat_rear)
    cr.fill_preserve()
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.08)
    cr.set_line_width(0.85)
    cr.stroke()

    cr.new_path()
    cr.move_to(bx + 4, guard_top - 4)
    cr.curve_to(bx - 6, guard_top - 2, guard_x, guard_top + 8, guard_x, guard_top + 20)
    cr.line_to(guard_x, waist_y - 14)
    cr.curve_to(guard_x, waist_y - 2, bx - 6, waist_y + 8, bx + 4, waist_y + 10)
    cr.close_path()
    pat_g = cairo.LinearGradient(guard_x, 0, bx + 3, 0)
    pat_g.add_color_stop_rgb(0.0, 0.06, 0.065, 0.075)
    pat_g.add_color_stop_rgb(0.26, 0.18, 0.19, 0.21)
    pat_g.add_color_stop_rgb(0.68, 0.11, 0.115, 0.13)
    pat_g.add_color_stop_rgb(1.0, 0.04, 0.045, 0.05)
    cr.set_source(pat_g)
    cr.fill_preserve()
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.12)
    cr.set_line_width(0.9)
    cr.stroke()

    for _kn, sx, sy, sw, sh, is_ptt, _dots in side_specs:
        round_rect(cr, sx + 1.5, sy - 4.0, sw + 1.0, sh + 8.0, 8.5 if is_ptt else 6.5)
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.08)
        cr.fill()
        round_rect(cr, sx + 3.0, sy - 2.6, sw - 0.5, sh + 5.2, 7.5 if is_ptt else 5.5)
        cr.set_source_rgba(0.015, 0.015, 0.02, 0.92)
        cr.fill()


def draw_snap_dome(
    cr: Any,
    cx: float,
    cy: float,
    radius: float,
    label: str = "",
    *,
    press_t: float = 0.0,
    hover_t: float = 0.0,
    accent: tuple[float, float, float] = (0.18, 0.56, 1.0),
    white_ring: bool = True,
) -> None:
    """Photorealistic brushed stainless-steel PCB snap-dome contact (for Open Shell & Bare PCB views)."""
    import cairo

    p = clamp1(press_t)
    hv = clamp1(hover_t)

    # White PCB silkscreen / Kapton ring around contact pad
    if white_ring:
        cr.new_path()
        cr.arc(cx, cy, radius + 3.0, 0, 2 * math.pi)
        cr.set_source_rgba(0.94, 0.96, 0.94, 0.88)
        cr.set_line_width(1.6)
        cr.stroke()

    # Outer copper/ENIG pad collar
    cr.new_path()
    cr.arc(cx, cy, radius + 1.1, 0, 2 * math.pi)
    cr.set_source_rgb(0.42, 0.36, 0.20)
    cr.fill()

    if p > 0.02 or hv > 0.02:
        cr.new_path()
        cr.arc(cx, cy, radius + 2.6, 0, 2 * math.pi)
        cr.set_source_rgba(accent[0], accent[1], accent[2], 0.42 * p + 0.16 * hv)
        cr.fill()

    # Metallic snap-dome disc with spun conical highlight
    r = radius * (1.0 - 0.06 * p)
    ox = -r * (0.28 - 0.18 * p)
    oy = -r * (0.30 - 0.22 * p)
    pat = cairo.RadialGradient(cx + ox, cy + oy, r * 0.08, cx, cy, r)
    if p > 0.3:
        pat.add_color_stop_rgb(0.0, 0.72, 0.74, 0.76)
        pat.add_color_stop_rgb(0.55, 0.48, 0.49, 0.50)
        pat.add_color_stop_rgb(1.0, 0.26, 0.26, 0.28)
    else:
        pat.add_color_stop_rgb(0.0, 0.96 + 0.04 * hv, 0.95 + 0.04 * hv, 0.92 + 0.04 * hv)
        pat.add_color_stop_rgb(0.45, 0.76, 0.75, 0.72)
        pat.add_color_stop_rgb(0.82, 0.50, 0.49, 0.47)
        pat.add_color_stop_rgb(1.0, 0.32, 0.31, 0.30)
    cr.new_path()
    cr.arc(cx, cy, r, 0, 2 * math.pi)
    cr.set_source(pat)
    cr.fill_preserve()
    cr.set_source_rgba(0.08, 0.09, 0.10, 0.65)
    cr.set_line_width(0.85)
    cr.stroke()

    # Inner dimple ring on dome crown
    cr.new_path()
    cr.arc(cx, cy, r * 0.46, 0, 2 * math.pi)
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.26 * (1.0 - 0.6 * p))
    cr.set_line_width(0.7)
    cr.stroke()

    if label:
        draw_pango_text(
            cr,
            label,
            cx,
            cy,
            font_desc="Adwaita Sans Bold 6.5",
            rgba=(0.06, 0.08, 0.10, 0.52 + 0.38 * hv),
            halign="center",
            valign="center",
        )
