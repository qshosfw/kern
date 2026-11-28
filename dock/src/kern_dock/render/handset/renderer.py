"""Multi-layer cached Cairo handset renderer for supported Quansheng UV-K hardware."""

from __future__ import annotations

import math
from typing import Any

from ..lcd import FastLcdState, LcdLayerCache, LcdSettings, _lcd_colors_norm, paint_fast_lcd
from ..paint import clamp1, paint_brushed_metal_grain, round_rect
from .catalog import K1_MATRIX, K5_BOTTOM_MATRIX, compute_handset_geometry, radio_model
from .parts import (
    _draw_boxed_badge,
    draw_3d_keycap,
    draw_frosted_led,
    draw_hex_torx_bolt,
    draw_k1_keycap,
    draw_k1_side_shroud,
    draw_k5_side_shroud,
    draw_k10_keycap,
    draw_k10_logo_led,
    draw_pango_text,
    draw_phillips_screw,
    draw_quansheng_logo,
    draw_snap_dome,
    draw_top_flashlight,
)
from .spec import HandsetSpec
from .variants import build_k1_clear, build_k5_99, build_k5_black, build_k10_pro, build_uv5r_plus


class HandsetRenderer:
    """Photorealistic cached Cairo handset renderer (~0.7 ms/frame animated)."""

    def __init__(self) -> None:
        self._chassis_key: Any = None
        self._back_surf: Any = None
        self._chassis_surf: Any = None
        self._body_bx: float = 30.0
        self._lcd_rect: tuple[int, int, int, int] = (0, 0, 100, 60)
        self._torch_anchor: tuple[float, float, bool] = (180.0, 60.0, False)
        self._led_anchor: tuple[float, float, float, float, bool] = (96.0, 50.0, 22.0, 10.5, True)
        # (key_name, x, y, w, h, primary, sub1, sub2, radius, is_nav, homing, style)
        self._key_specs: list[tuple[str, float, float, float, float, str, str, str, float, bool, bool, str]] = []
        self._side_specs: list[tuple[str, float, float, float, float, bool, int]] = []
        self.hitboxes: dict[str, tuple[float, float, float, float]] = {}
        self.lcd_cache = LcdLayerCache()
        self._spec: HandsetSpec = radio_model("k1")

    def _ensure_chassis(self, model: str, lcd_st: LcdSettings) -> None:
        import cairo

        spec = radio_model(model)
        self._spec = spec
        key = (spec.id, lcd_st.palette, lcd_st.backlight, lcd_st.backlight_glow)
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

        if spec.chassis == "k5v3":
            self._build_k5v3(back_cr, cr, W, H, lcd_st, spec)
        elif spec.chassis == "k1-clear":
            build_k1_clear(self, back_cr, cr, W, H, lcd_st, spec)
        elif spec.chassis == "k5-black":
            build_k5_black(self, back_cr, cr, W, H, lcd_st, spec)
        elif spec.chassis == "k5-99":
            build_k5_99(self, back_cr, cr, W, H, lcd_st, spec)
        elif spec.chassis == "uv5r-plus":
            build_uv5r_plus(self, back_cr, cr, W, H, lcd_st, spec)
        elif spec.chassis == "k10-pro":
            build_k10_pro(self, back_cr, cr, W, H, lcd_st, spec)
        elif spec.chassis == "k1-naked":
            self._build_k1_naked(back_cr, cr, W, H, lcd_st, spec)
        elif spec.chassis == "pcb-r51":
            self._build_pcb_r51(back_cr, cr, W, H, lcd_st, spec)
        elif spec.chassis == "k1-polymer":
            build_k1_clear(self, back_cr, cr, W, H, lcd_st, spec, transparent=False)
        else:
            self._build_k1(back_cr, cr, W, H, lcd_st, spec, metallic_armor=True)

        # Bake idle keycaps onto the cached chassis surface so steady streaming skips 16 text layouts/frame.
        for _kn, kx, ky, kw, kh, pri, s1, s2, rad, is_nav, nub, kstyle in self._key_specs:
            self._paint_key(cr, kx, ky, kw, kh, pri, s1, s2, rad, is_nav, nub, kstyle, 0.0, 0.0, spec)
        self._back_surf = back_surf
        self._chassis_surf = surf

    # =========================================================================
    # 1. Quansheng UV-K1(8) (Metallic Armor) & UV-K1 (Base Polymer) — Compact Mini-Kong
    # =========================================================================
    def _build_k1(
        self, back_cr: Any, cr: Any, W: int, H: int, lcd_st: LcdSettings, spec: HandsetSpec, *, metallic_armor: bool
    ) -> None:
        import cairo

        bx, by, bw, bh = 32.0, 58.0, W - 48.0, H - 70.0
        self._body_bx = bx

        self._side_specs = [
            ("ptt",   9.0, by + 56.0, 26.0, 114.0, True, 0),
            ("side1", 12.0, by + 180.0, 23.0, 35.0, False, 2),
            ("side2", 12.0, by + 223.0, 23.0, 35.0, False, 1),
        ]
        for kn, sx, sy, sw, sh, _, _ in self._side_specs:
            self.hitboxes[kn] = (sx - 3.0, sy, sw, sh)

        # Back shadow + sculpted left flank + right accessory-port cheek
        round_rect(back_cr, bx + 3, by + 10, bw, bh, 17)
        back_cr.set_source_rgba(0.0, 0.0, 0.0, 0.42)
        back_cr.fill()
        draw_k1_side_shroud(back_cr, bx, by, bh, self._side_specs)
        round_rect(back_cr, bx + bw - 6.0, by + 48.0, 12.0, bh - 112.0, 5.5)
        pat_rc = cairo.LinearGradient(bx + bw - 6.0, 0, bx + bw + 6.0, 0)
        pat_rc.add_color_stop_rgb(0.0, 0.05, 0.055, 0.065)
        pat_rc.add_color_stop_rgb(0.65, 0.16, 0.17, 0.19)
        pat_rc.add_color_stop_rgb(1.0, 0.07, 0.075, 0.085)
        back_cr.set_source(pat_rc)
        back_cr.fill()

        # Top hardware: stubby ribbed A1 rubber antenna + frosted LED + torch + rotary knob
        ax = bx + 54.0
        pat_ant = cairo.LinearGradient(ax - 26, 0, ax + 26, 0)
        pat_ant.add_color_stop_rgb(0.0, 0.07, 0.08, 0.09)
        pat_ant.add_color_stop_rgb(0.28, 0.28, 0.29, 0.31)
        pat_ant.add_color_stop_rgb(0.55, 0.19, 0.20, 0.21)
        pat_ant.add_color_stop_rgb(1.0, 0.05, 0.06, 0.07)
        cr.set_source(pat_ant)
        round_rect(cr, ax - 21, 2, 42, by + 4, 5)
        cr.fill()
        round_rect(cr, ax - 24, by - 20, 48, 24, 4)
        cr.fill()
        cr.set_source_rgba(0.0, 0.0, 0.0, 0.35)
        cr.set_line_width(1.0)
        for ry in (by - 15, by - 9):
            cr.move_to(ax - 23, ry)
            cr.line_to(ax + 23, ry)
            cr.stroke()
        round_rect(cr, ax - 10, 5, 20, 22, 4)
        cr.set_source_rgba(0.04, 0.04, 0.05, 0.56)
        cr.fill()
        draw_pango_text(cr, spec.antenna_label, ax, 16, font_desc="Adwaita Sans Bold 8.4", rgba=(0.72, 0.74, 0.77, 0.52), halign="center")

        tx = bx + bw * 0.51
        self._torch_anchor = (tx, by, False)
        self.hitboxes["_torch"] = (tx - 19, by - 16, 38, 20)

        kx = bx + bw - 56.0
        pat_k = cairo.LinearGradient(kx - 31, 0, kx + 31, 0)
        pat_k.add_color_stop_rgb(0.0, 0.06, 0.07, 0.08)
        pat_k.add_color_stop_rgb(0.28, 0.29, 0.31, 0.34)
        pat_k.add_color_stop_rgb(0.55, 0.17, 0.18, 0.20)
        pat_k.add_color_stop_rgb(1.0, 0.05, 0.06, 0.07)
        cr.set_source(pat_k)
        round_rect(cr, kx - 30, by - 38, 60, 42, 7)
        cr.fill()
        for dx in (-20, -11, 11, 20):
            round_rect(cr, kx + dx - 2.8, by - 34, 5.6, 27, 2.5)
            cr.set_source_rgba(0.02, 0.02, 0.03, 0.68)
            cr.fill()
        if metallic_armor:
            ar, ag, ab = spec.accent
            pat_bl = cairo.LinearGradient(kx - 4, by - 38, kx + 4, by - 38)
            pat_bl.add_color_stop_rgb(0.0, ar * 0.68, ag * 0.74, ab * 0.94)
            pat_bl.add_color_stop_rgb(0.5, min(1.0, ar * 1.4), min(1.0, ag * 1.15), min(1.0, ab * 1.0))
            pat_bl.add_color_stop_rgb(1.0, ar * 0.52, ag * 0.62, ab * 0.86)
            cr.set_source(pat_bl)
            round_rect(cr, kx - 3.6, by - 38, 7.2, 36, 2.0)
            cr.fill()
        else:
            # Base polymer UV-K1 has an all-black knob with a crisp white index mark
            cr.set_source_rgba(0.86, 0.88, 0.92, 0.75)
            round_rect(cr, kx - 1.5, by - 36, 3.0, 14, 1.2)
            cr.fill()

        # Main black polymer body
        br, bg_t, bb = spec.body_tone
        round_rect(cr, bx, by, bw, bh, 15)
        pat_sh = cairo.LinearGradient(bx, by, bx + bw, by)
        pat_sh.add_color_stop_rgb(0.0, br * 2.1, bg_t * 2.1, bb * 2.1)
        pat_sh.add_color_stop_rgb(0.06, br, bg_t, bb)
        pat_sh.add_color_stop_rgb(0.94, br, bg_t, bb)
        pat_sh.add_color_stop_rgb(1.0, br * 2.2, bg_t * 2.2, bb * 2.2)
        cr.set_source(pat_sh)
        cr.fill_preserve()
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.12)
        cr.set_line_width(1.0)
        cr.stroke()

        # Frosted polycarbonate status LED tucked beside antenna base (`K1_VIEW_E_FRONT_BACK.png`)
        self._led_anchor = (bx + 84.0, by - 8.5, 22.0, 10.0, True)

        fx_l, fx_r = bx + 20.0, bx + bw - 20.0
        fy_t, fy_b = by + 5.0, by + bh - 8.0

        if metallic_armor:
            # Exposed black side polymer channel ribs
            for side_x in (bx + 4.5, bx + bw - 13.5):
                round_rect(cr, side_x, by + 62.0, 9.0, bh - 148.0, 3.0)
                cr.set_source_rgba(0.035, 0.04, 0.048, 0.88)
                cr.fill()

            # Authentic K1(8) 4-lug chamfered metallic armor plate (`K1_VIEW_E_FRONT_BACK.png`)
            def _k1_armor_path() -> None:
                cr.new_sub_path()
                cr.move_to(bx + 18, fy_t)
                cr.line_to(bx + bw - 18, fy_t)
                cr.curve_to(bx + bw - 5, fy_t, bx + bw - 2, fy_t + 8, bx + bw - 2, fy_t + 22)
                cr.curve_to(bx + bw - 2, fy_t + 34, bx + bw - 6, fy_t + 41, bx + bw - 13, fy_t + 44)
                cr.line_to(fx_r, fy_t + 53)
                cr.line_to(fx_r, fy_b - 72)
                cr.line_to(bx + bw - 9, fy_b - 63)
                cr.curve_to(bx + bw - 2, fy_b - 59, bx + bw - 2, fy_b - 50, bx + bw - 2, fy_b - 39)
                cr.curve_to(bx + bw - 2, fy_b - 26, bx + bw - 7, fy_b - 20, fx_r, fy_b - 16)
                cr.line_to(fx_r - 10, fy_b)
                cr.line_to(fx_l + 10, fy_b)
                cr.line_to(fx_l, fy_b - 16)
                cr.curve_to(bx + 7, fy_b - 20, bx + 2, fy_b - 26, bx + 2, fy_b - 39)
                cr.curve_to(bx + 2, fy_b - 50, bx + 2, fy_b - 59, bx + 9, fy_b - 63)
                cr.line_to(fx_l, fy_b - 72)
                cr.line_to(fx_l, fy_t + 53)
                cr.line_to(bx + 13, fy_t + 44)
                cr.curve_to(bx + 6, fy_t + 41, bx + 2, fy_t + 34, bx + 2, fy_t + 22)
                cr.curve_to(bx + 2, fy_t + 8, bx + 5, fy_t, bx + 18, fy_t)
                cr.close_path()

            cr.save()
            cr.translate(0, 2.0)
            _k1_armor_path()
            cr.set_source_rgba(0.0, 0.0, 0.0, 0.45)
            cr.fill()
            cr.restore()

            _k1_armor_path()
            pat_m = cairo.LinearGradient(bx, fy_t, bx + bw, fy_b)
            for stop, rgb in spec.armor_stops:
                pat_m.add_color_stop_rgb(stop, *rgb)
            cr.set_source(pat_m)
            cr.fill_preserve()

            cr.save()
            cr.clip_preserve()
            paint_brushed_metal_grain(cr, bx, fy_t, bw, fy_b - fy_t, vertical=False, alpha=0.026)
            cr.restore()

            cr.set_source_rgba(1.0, 0.98, 0.94, 0.28)
            cr.set_line_width(1.1)
            cr.stroke()

            for bx_b, by_b in (
                (bx + 18.0, fy_t + 22.0),
                (bx + bw - 18.0, fy_t + 22.0),
                (bx + 18.0, fy_b - 41.0),
                (bx + bw - 18.0, fy_b - 41.0),
            ):
                draw_hex_torx_bolt(cr, bx_b, by_b, 5.6)
        else:
            # Base UV-K1 (Polymer): clean molded front polymer fascia with subtle recessed border grooves
            round_rect(cr, bx + 10, fy_t + 2, bw - 20, fy_b - fy_t - 2, 12)
            pat_p = cairo.LinearGradient(bx + 10, fy_t, bx + bw - 10, fy_b)
            for stop, rgb in spec.armor_stops:
                pat_p.add_color_stop_rgb(stop, *rgb)
            cr.set_source(pat_p)
            cr.fill_preserve()
            cr.set_source_rgba(1.0, 1.0, 1.0, 0.09)
            cr.set_line_width(1.0)
            cr.stroke()

        # Trapezoidal glossy black QUANSHENG nameplate
        np_x, np_y, np_w, np_h = bx + 44.0, fy_t + 7.0, bw - 88.0, 29.0
        cr.new_sub_path()
        cr.move_to(np_x + 7.0, np_y)
        cr.line_to(np_x + np_w - 7.0, np_y)
        cr.line_to(np_x + np_w, np_y + np_h - 4.0)
        cr.curve_to(np_x + np_w, np_y + np_h, np_x + np_w - 2.0, np_y + np_h, np_x + np_w - 5.0, np_y + np_h)
        cr.line_to(np_x + 5.0, np_y + np_h)
        cr.curve_to(np_x + 2.0, np_y + np_h, np_x, np_y + np_h, np_x, np_y + np_h - 4.0)
        cr.close_path()
        cr.set_source_rgb(0.05, 0.055, 0.065)
        cr.fill_preserve()
        cr.save()
        cr.clip_preserve()
        cr.new_sub_path()
        cr.move_to(np_x, np_y)
        cr.line_to(np_x + np_w * 0.56, np_y)
        cr.line_to(np_x + np_w * 0.42, np_y + np_h)
        cr.line_to(np_x, np_y + np_h)
        cr.close_path()
        gl = cairo.LinearGradient(np_x, np_y, np_x + np_w * 0.5, np_y + np_h)
        gl.add_color_stop_rgba(0.0, 1.0, 1.0, 1.0, 0.18)
        gl.add_color_stop_rgba(1.0, 1.0, 1.0, 1.0, 0.03)
        cr.set_source(gl)
        cr.fill()
        cr.restore()
        cr.set_source_rgba(0.0, 0.0, 0.0, 0.78)
        cr.set_line_width(1.1)
        cr.stroke()
        draw_quansheng_logo(cr, np_x + np_w * 0.5, np_y + np_h * 0.53, scale=0.98)

        # Wide 1.44" LCD recessed deep inside broad chamfered bevel walls (`K1_VIEW_D_FRONT.png`)
        lz_w = int((fx_r - fx_l) - 28)
        lz_h = int(round(lz_w / 1.46))
        lz_x = int(fx_l + ((fx_r - fx_l) - lz_w) * 0.5)
        lz_y = int(fy_t + 48)

        # Broad 45-degree sloped metallic chamfer wall framing the screen
        round_rect(cr, lz_x - 7, lz_y - 7, lz_w + 14, lz_h + 14, 12)
        pat_ch = cairo.LinearGradient(lz_x, lz_y - 7, lz_x, lz_y + lz_h + 7)
        pat_ch.add_color_stop_rgba(0.0, 0.0, 0.0, 0.0, 0.50)
        pat_ch.add_color_stop_rgba(0.5, 0.0, 0.0, 0.0, 0.12)
        pat_ch.add_color_stop_rgba(1.0, 1.0, 0.99, 0.96, 0.24)
        cr.set_source(pat_ch)
        cr.fill()

        round_rect(cr, lz_x, lz_y, lz_w, lz_h, 8)
        cr.set_source_rgb(0.05, 0.055, 0.065)
        cr.fill_preserve()
        cr.set_source_rgba(1.0, 0.99, 0.96, 0.16)
        cr.set_line_width(0.9)
        cr.stroke()
        if lcd_st.backlight_glow and lcd_st.backlight > 0:
            bg_c, _, _, _ = _lcd_colors_norm(lcd_st)
            cr.set_source_rgba(bg_c[0], bg_c[1], bg_c[2], 0.14)
            round_rect(cr, lz_x + 4, lz_y + 4, lz_w - 8, lz_h - 8, 5)
            cr.set_line_width(2.2)
            cr.stroke()
        self._lcd_rect = (lz_x + 6, lz_y + 6, lz_w - 12, lz_h - 12)

        # Stepped mid-bar ("UHF/VHF Transceiver") + single recessed pill mic port (`K1_VIEW_E_FRONT_BACK.png`)
        bar_y = lz_y + lz_h + 9.0
        base_r, base_g, base_b = spec.armor_stops[1][1]
        pat_bar = cairo.LinearGradient(fx_l, bar_y, fx_l, bar_y + 18.0)
        pat_bar.add_color_stop_rgb(0.0, clamp1(base_r * 1.15), clamp1(base_g * 1.15), clamp1(base_b * 1.15))
        pat_bar.add_color_stop_rgb(0.84, base_r, base_g, base_b)
        pat_bar.add_color_stop_rgb(1.0, base_r * 0.55, base_g * 0.55, base_b * 0.55)
        cr.set_source(pat_bar)
        cr.rectangle(fx_l + 4, bar_y, (fx_r - fx_l) - 8, 18.0)
        cr.fill()
        tag_rgba = (0.06, 0.06, 0.07, 0.86) if metallic_armor else (0.78, 0.80, 0.84, 0.78)
        draw_pango_text(
            cr, spec.tagline, fx_r - 10.0, bar_y + 8.5,
            font_desc="Adwaita Sans Medium 8.2", rgba=tag_rgba, halign="right", valign="center",
        )
        step_sh = cairo.LinearGradient(0, bar_y + 18.0, 0, bar_y + 26.0)
        step_sh.add_color_stop_rgba(0.0, 0.0, 0.0, 0.0, 0.42)
        step_sh.add_color_stop_rgba(1.0, 0.0, 0.0, 0.0, 0.0)
        cr.set_source(step_sh)
        cr.rectangle(fx_l + 4, bar_y + 18.0, (fx_r - fx_l) - 8, 8.0)
        cr.fill()

        mic_x, mic_y, mic_w, mic_h = fx_r - 39.0, bar_y + 21.0, 26.0, 5.8
        round_rect(cr, mic_x, mic_y, mic_w, mic_h, 2.8)
        cr.set_source_rgb(0.03, 0.035, 0.042)
        cr.fill()

        # Redesigned compact K1 4×4 flush-grid keypad + 12 wide milled speaker slots (`K1_VIEW_E_FRONT_BACK.png`)
        kp_x, kp_y, kp_w = fx_l + 6.0, bar_y + 30.0, (fx_r - fx_l) - 12.0
        gap_c = 5.5
        gap_r = 10.5
        kw = (kp_w - gap_c * 3.0) / 4.0
        kh_nav, kh_num = 36.5, 33.5

        for r_i, row_keys in enumerate(K1_MATRIX):
            ky = kp_y + (0.0 if r_i == 0 else (kh_nav + gap_r + (r_i - 1) * (kh_num + gap_r)))
            kh = kh_nav if r_i == 0 else kh_num
            for c_i, (kn, pri, s1, s2) in enumerate(row_keys):
                kx = kp_x + c_i * (kw + gap_c)
                self.hitboxes[kn] = (kx, ky, kw, kh)
                self._key_specs.append((kn, kx, ky, kw, kh, pri, s1, s2, 4.0, r_i == 0, kn == "5", "k1"))
                if r_i < 3:
                    sl_w = kw * 0.74
                    sl_x = kx + (kw - sl_w) * 0.5
                    sl_y = ky + kh + 3.8
                    round_rect(cr, sl_x, sl_y, sl_w, 4.4, 1.8)
                    cr.set_source_rgb(0.03, 0.035, 0.04)
                    cr.fill()
                    cr.set_source_rgba(1.0, 0.99, 0.95, 0.18)
                    cr.set_line_width(0.65)
                    cr.move_to(sl_x + 2, sl_y + 4.9)
                    cr.line_to(sl_x + sl_w - 2, sl_y + 4.9)
                    cr.stroke()

    # =========================================================================
    # 2. Quansheng UV-K5 v3 / UV-K5(8) / UV-K6 (Brushed Metallic Upper Armor)
    # =========================================================================
    def _build_k5v3(self, back_cr: Any, cr: Any, W: int, H: int, lcd_st: LcdSettings, spec: HandsetSpec) -> None:
        import cairo

        bx, by, bw, bh = 31.0, 62.0, W - 47.0, H - 76.0
        self._body_bx = bx

        self._side_specs = [
            ("ptt",   10.0, by + 58.0, 24.0, 122.0, True, 0),
            ("side1", 12.0, by + 192.0, 22.0, 38.0, False, 1),
            ("side2", 12.0, by + 238.0, 22.0, 38.0, False, 2),
        ]
        for kn, sx, sy, sw, sh, _, _ in self._side_specs:
            self.hitboxes[kn] = (sx - 3.0, sy, sw, sh)

        round_rect(back_cr, bx + 4, by + 11, bw, bh, 20)
        back_cr.set_source_rgba(0.0, 0.0, 0.0, 0.42)
        back_cr.fill()
        draw_k5_side_shroud(back_cr, bx, by, bh, self._side_specs)

        round_rect(back_cr, bx + bw - 7.0, by + 48.0, 13.5, bh - 188.0, 5.5)
        pat_rb = cairo.LinearGradient(bx + bw - 7.0, 0, bx + bw + 6.5, 0)
        pat_rb.add_color_stop_rgb(0.0, 0.05, 0.055, 0.065)
        pat_rb.add_color_stop_rgb(0.62, 0.17, 0.18, 0.20)
        pat_rb.add_color_stop_rgb(1.0, 0.06, 0.065, 0.075)
        back_cr.set_source(pat_rb)
        back_cr.fill()

        # Tapered whip antenna + torch + iconic orange-capped V-notch knob + shoulder guard ear
        ax = bx + 62.0
        pat_ant = cairo.LinearGradient(ax - 24, 0, ax + 24, 0)
        pat_ant.add_color_stop_rgb(0.0, 0.07, 0.08, 0.09)
        pat_ant.add_color_stop_rgb(0.32, 0.29, 0.30, 0.32)
        pat_ant.add_color_stop_rgb(0.65, 0.17, 0.18, 0.19)
        pat_ant.add_color_stop_rgb(1.0, 0.05, 0.06, 0.07)
        cr.set_source(pat_ant)
        cr.new_path()
        cr.move_to(ax - 25, by + 4)
        cr.line_to(ax - 21, 2)
        cr.line_to(ax + 21, 2)
        cr.line_to(ax + 25, by + 4)
        cr.close_path()
        cr.fill()

        tx = bx + bw * 0.49
        self._torch_anchor = (tx, by, True)
        self.hitboxes["_torch"] = (tx - 18, by - 16, 36, 20)

        kx = bx + bw - 62.0
        pat_kb = cairo.LinearGradient(kx - 32, 0, kx + 32, 0)
        pat_kb.add_color_stop_rgb(0.0, 0.06, 0.07, 0.08)
        pat_kb.add_color_stop_rgb(0.32, 0.25, 0.26, 0.28)
        pat_kb.add_color_stop_rgb(1.0, 0.06, 0.07, 0.08)
        cr.set_source(pat_kb)
        round_rect(cr, kx - 31, by - 32, 62, 36, 6)
        cr.fill()

        ar, ag, ab = spec.accent
        pat_o = cairo.LinearGradient(kx - 30, by - 44, kx + 30, by - 22)
        pat_o.add_color_stop_rgb(0.0, ar * 0.90, ag * 0.68, ab * 0.50)
        pat_o.add_color_stop_rgb(0.42, min(1.0, ar * 1.04), min(1.0, ag * 1.15), min(1.0, ab * 1.45))
        pat_o.add_color_stop_rgb(1.0, ar * 0.80, ag * 0.52, ab * 0.25)
        cr.set_source(pat_o)
        round_rect(cr, kx - 30, by - 44, 60, 13, 5)
        cr.fill()
        cr.new_path()
        cr.move_to(kx - 8.0, by - 33)
        cr.line_to(kx + 8.0, by - 33)
        cr.line_to(kx + 3.5, by - 16)
        cr.line_to(kx - 3.5, by - 16)
        cr.close_path()
        cr.fill()
        round_rect(cr, kx - 26.5, by - 28, 7.0, 12.0, 2.5)
        round_rect(cr, kx + 19.5, by - 28, 7.0, 12.0, 2.5)
        cr.fill()

        br, bg_t, bb = spec.body_tone
        is_bright_body = max(br, bg_t, bb) > 0.40

        cr.new_path()
        cr.move_to(bx + bw - 22, by + 6)
        cr.line_to(bx + bw - 22, by - 14)
        cr.curve_to(bx + bw - 14, by - 16, bx + bw - 12, by - 36, bx + bw - 9, by - 40)
        cr.line_to(bx + bw - 2, by - 40)
        cr.line_to(bx + bw + 2, by + 6)
        cr.close_path()
        cr.set_source_rgb(br * 0.94, bg_t * 0.94, bb * 0.94)
        cr.fill()

        self._led_anchor = (bx + 86.0, by - 7.5, 20.0, 9.5, True)

        # Contoured waist body (`UVK6.1.png` charcoal / `media_1795271400430` safety orange)
        waist_in = 5.0
        wy_t, wy_b = by + 238.0, by + 360.0
        cr.new_sub_path()
        cr.move_to(bx + 20, by)
        cr.line_to(bx + bw - 20, by)
        cr.curve_to(bx + bw, by, bx + bw, by + 10, bx + bw, by + 22)
        cr.line_to(bx + bw, wy_t - 10)
        cr.curve_to(bx + bw, wy_t + 6, bx + bw - waist_in, wy_t + 14, bx + bw - waist_in, wy_t + 26)
        cr.line_to(bx + bw - waist_in, wy_b - 18)
        cr.curve_to(bx + bw - waist_in, wy_b, bx + bw - 1.5, wy_b + 10, bx + bw - 1.5, wy_b + 24)
        cr.line_to(bx + bw - 1.5, by + bh - 20)
        cr.curve_to(bx + bw - 1.5, by + bh, bx + bw - 16, by + bh, bx + bw - 24, by + bh)
        cr.line_to(bx + 24, by + bh)
        cr.curve_to(bx + 16, by + bh, bx + 1.5, by + bh, bx + 1.5, by + bh - 20)
        cr.line_to(bx + 1.5, wy_b + 24)
        cr.curve_to(bx + 1.5, wy_b + 10, bx + waist_in, wy_b, bx + waist_in, wy_b - 18)
        cr.line_to(bx + waist_in, wy_t + 26)
        cr.curve_to(bx + waist_in, wy_t + 14, bx, wy_t + 6, bx, wy_t - 10)
        cr.line_to(bx, by + 22)
        cr.curve_to(bx, by + 10, bx, by, bx + 20, by)
        cr.close_path()
        pat_b = cairo.LinearGradient(bx, by, bx + bw, by)
        if is_bright_body:
            pat_b.add_color_stop_rgb(0.0, clamp1(br * 1.08), clamp1(bg_t * 1.15), clamp1(bb * 1.30))
            pat_b.add_color_stop_rgb(0.10, br, bg_t, bb)
            pat_b.add_color_stop_rgb(0.90, br * 0.92, bg_t * 0.90, bb * 0.88)
            pat_b.add_color_stop_rgb(1.0, clamp1(br * 0.82), clamp1(bg_t * 0.78), clamp1(bb * 0.72))
        else:
            pat_b.add_color_stop_rgb(0.0, clamp1(br * 1.9), clamp1(bg_t * 1.9), clamp1(bb * 1.9))
            pat_b.add_color_stop_rgb(0.08, br, bg_t, bb)
            pat_b.add_color_stop_rgb(0.92, br * 0.96, bg_t * 0.96, bb * 0.96)
            pat_b.add_color_stop_rgb(1.0, clamp1(br * 1.85), clamp1(bg_t * 1.85), clamp1(bb * 1.85))
        cr.set_source(pat_b)
        cr.fill_preserve()
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.14 if is_bright_body else 0.10)
        cr.set_line_width(1.0)
        cr.stroke()

        for fi in range(3):
            round_rect(cr, bx + bw - 16.0 + fi * 3.8, wy_t + 14.0, 2.0, 96.0, 1.0)
            cr.set_source_rgba(0.0, 0.0, 0.0, 0.25 if is_bright_body else 0.45)
            cr.fill()

        cr.set_source_rgb(br * 0.78, bg_t * 0.78, bb * 0.78)
        round_rect(cr, bx + 28, by + bh - 2, 26, 6, 2)
        round_rect(cr, bx + bw - 54, by + bh - 2, 26, 6, 2)
        cr.fill()

        # Brushed metallic upper armor plate
        ux, uy, uw, uh = bx + 5.0, by + 4.0, bw - 10.0, 230.0
        cr.new_sub_path()
        cr.move_to(ux + 14, uy)
        cr.line_to(ux + uw - 14, uy)
        cr.line_to(ux + uw, uy + 14)
        cr.line_to(ux + uw, uy + uh - 26)
        cr.line_to(ux + uw - 22, uy + uh)
        cr.line_to(ux + 22, uy + uh)
        cr.line_to(ux, uy + uh - 26)
        cr.line_to(ux, uy + 14)
        cr.close_path()
        pat_u = cairo.LinearGradient(ux, uy, ux + uw, uy)
        for stop, rgb in spec.armor_stops:
            pat_u.add_color_stop_rgb(stop, *rgb)
        cr.set_source(pat_u)
        cr.fill_preserve()
        cr.save()
        cr.clip_preserve()
        paint_brushed_metal_grain(cr, ux, uy, uw, uh, vertical=True, alpha=0.025)
        cr.restore()
        cr.set_source_rgba(1.0, 0.99, 0.96, 0.34)
        cr.set_line_width(1.1)
        cr.stroke()

        lz_w = int(uw - 80)
        lz_h = int(round(lz_w / 1.46))
        lz_x = int(ux + (uw - lz_w) * 0.5)
        lz_y = int(uy + 34)
        round_rect(cr, lz_x - 4, lz_y - 4, lz_w + 8, lz_h + 8, 10)
        cr.set_source_rgba(0.0, 0.0, 0.0, 0.30)
        cr.fill()
        round_rect(cr, lz_x, lz_y, lz_w, lz_h, 8)
        cr.set_source_rgb(0.055, 0.06, 0.07)
        cr.fill_preserve()
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.24)
        cr.set_line_width(0.95)
        cr.stroke()
        if lcd_st.backlight_glow and lcd_st.backlight > 0:
            bg_c, _, _, _ = _lcd_colors_norm(lcd_st)
            cr.set_source_rgba(bg_c[0], bg_c[1], bg_c[2], 0.22)
            round_rect(cr, lz_x + 4, lz_y + 4, lz_w - 8, lz_h - 8, 6)
            cr.set_line_width(2.5)
            cr.stroke()
        self._lcd_rect = (lz_x + 6, lz_y + 6, lz_w - 12, lz_h - 12)

        m_w, m_h = 62.0, 34.0
        m_x, m_y = ux + 28.0, lz_y + lz_h + 14.0
        np_x, np_y, np_w, np_h = m_x + m_w + 14.0, m_y + 1.0, uw - (m_w + 92.0), 32.0
        round_rect(cr, np_x, np_y, np_w, np_h, 6)
        cr.set_source_rgb(0.20, 0.21, 0.23)
        cr.fill_preserve()
        cr.set_source_rgba(0.0, 0.0, 0.0, 0.50)
        cr.set_line_width(1.0)
        cr.stroke()
        draw_quansheng_logo(cr, np_x + np_w * 0.5, np_y + np_h * 0.52, scale=0.72)

        for bx_s, by_s in (
            (lz_x + 4.0, uy + 16.0),
            (lz_x + lz_w - 4.0, uy + 16.0),
            (ux + 20.0, lz_y + lz_h - 12.0),
            (ux + uw - 24.0, np_y + np_h * 0.5),
        ):
            cr.new_path()
            cr.arc(bx_s, by_s, 10.6, 0, 2 * math.pi)
            cr.set_source_rgba(0.26, 0.26, 0.27, 0.52)
            cr.fill()
            draw_hex_torx_bolt(cr, bx_s, by_s, 5.8)

        # Two deep horizontal pill speaker bars (colored to match body polymer on UV-K5(8) Safety Orange!)
        mid_y = uy + uh + 14.0
        col_x, col_w = bx + 22.0, 44.0
        gx = col_x + col_w + 18.0
        gw = (bx + bw - 26.0) - gx
        for i in range(2):
            gy = mid_y + 2.0 + i * 40.0
            round_rect(cr, gx - 4, gy, gw + 4, 28.0, 10.0)
            cr.set_source_rgba(0.035, 0.04, 0.045, 0.72 if is_bright_body else 1.0)
            cr.fill()
            round_rect(cr, gx + 1, gy + 4.5, gw - 6, 19.0, 6.5)
            pat_sl = cairo.LinearGradient(0, gy + 4.5, 0, gy + 23.5)
            if is_bright_body:
                pat_sl.add_color_stop_rgb(0.0, clamp1(br * 1.08), clamp1(bg_t * 1.18), clamp1(bb * 1.32))
                pat_sl.add_color_stop_rgb(0.55, br * 0.96, bg_t * 0.94, bb * 0.92)
                pat_sl.add_color_stop_rgb(1.0, br * 0.76, bg_t * 0.72, bb * 0.68)
            else:
                pat_sl.add_color_stop_rgb(0.0, 0.27, 0.28, 0.30)
                pat_sl.add_color_stop_rgb(0.55, 0.17, 0.18, 0.20)
                pat_sl.add_color_stop_rgb(1.0, 0.10, 0.11, 0.12)
            cr.set_source(pat_sl)
            cr.fill_preserve()
            cr.set_source_rgba(1.0, 1.0, 1.0, 0.22 if is_bright_body else 0.12)
            cr.set_line_width(0.8)
            cr.stroke()

        round_rect(cr, bx + bw - 42.0, mid_y + 92.0, 14.0, 7.0, 3.5)
        cr.set_source_rgb(0.035, 0.04, 0.045)
        cr.fill()

        self._build_k5_keypad_dish(cr, bx, by, bw, bh, mid_y, m_x, m_y, m_w, m_h, col_x, col_w, is_bright_body=is_bright_body)

    # =========================================================================
    # 3. Shared UV-K5 Keypad Dish Helper
    # =========================================================================
    def _build_k5_keypad_dish(
        self,
        cr: Any,
        bx: float,
        by: float,
        bw: float,
        bh: float,
        mid_y: float,
        m_x: float,
        m_y: float,
        m_w: float,
        m_h: float,
        col_x: float,
        col_w: float,
        *,
        is_bright_body: bool = False,
    ) -> None:
        dish_top = mid_y + 84.0
        dish_step = dish_top + 26.0
        dish_bot = by + bh - 14.0
        cr.new_path()
        cr.move_to(bx + 20, dish_top)
        cr.line_to(bx + 78, dish_top)
        cr.curve_to(bx + 94, dish_top, bx + 100, dish_step, bx + 116, dish_step)
        cr.line_to(bx + bw - 20, dish_step)
        cr.curve_to(bx + bw - 12, dish_step, bx + bw - 10, dish_step + 8, bx + bw - 10, dish_step + 16)
        cr.line_to(bx + bw - 10, dish_bot - 14)
        cr.curve_to(bx + bw - 10, dish_bot, bx + bw - 20, dish_bot, bx + bw - 30, dish_bot)
        cr.line_to(bx + 30, dish_bot)
        cr.curve_to(bx + 18, dish_bot, bx + 12, dish_bot - 6, bx + 12, dish_bot - 16)
        cr.line_to(bx + 12, dish_top + 10)
        cr.curve_to(bx + 12, dish_top, bx + 16, dish_top, bx + 20, dish_top)
        cr.close_path()
        if is_bright_body:
            cr.set_source_rgba(0.0, 0.0, 0.0, 0.09)
        else:
            cr.set_source_rgba(0.035, 0.04, 0.05, 0.68)
        cr.fill_preserve()
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.18 if is_bright_body else 0.10)
        cr.set_line_width(1.0)
        cr.stroke()

        nav_specs = [
            ("menu", m_x, m_y, m_w, m_h, "M", "[A]", "", 8.0, True, False, "k5"),
            ("up",   col_x, mid_y, col_w, 35.0, "_CHEV_UP_", "[B]", "", 9.0, True, False, "k5"),
            ("down", col_x, mid_y + 41.0, col_w, 35.0, "_CHEV_DN_", "[C]", "", 9.0, True, False, "k5"),
            ("exit", bx + 20.0, dish_top + 4.0, 62.0, 25.0, "EXIT", "[D]", "", 10.0, True, False, "k5"),
        ]
        for sp in nav_specs:
            self.hitboxes[sp[0]] = (sp[1], sp[2], sp[3], sp[4])
            self._key_specs.append(sp)

        kp_x, kp_y, kp_w = bx + 20.0, dish_step + 8.0, bw - 40.0
        gap_c, gap_r, kh = 6.0, 6.5, 29.0
        kw = (kp_w - gap_c * 3.0) / 4.0
        for r_i, row_keys in enumerate(K5_BOTTOM_MATRIX):
            ky = kp_y + r_i * (kh + gap_r)
            for c_i, (kn, pri, s1, s2) in enumerate(row_keys):
                kx = kp_x + c_i * (kw + gap_c)
                self.hitboxes[kn] = (kx, ky, kw, kh)
                self._key_specs.append((kn, kx, ky, kw, kh, pri, s1, s2, 12.0, False, kn == "5", "k5"))

    # =========================================================================
    # 3. Developer View: UV-K1 Open Shell (No Top Cover — `K1_VIEW_D_FRONT.png`)
    # =========================================================================
    def _build_k1_naked(
        self, back_cr: Any, cr: Any, W: int, H: int, lcd_st: LcdSettings, spec: HandsetSpec
    ) -> None:
        import cairo

        bx, by, bw, bh = 32.0, 58.0, W - 48.0, H - 70.0
        self._body_bx = bx
        self._side_specs = [
            ("ptt", 9.0, by + 56.0, 26.0, 114.0, True, 0),
            ("side1", 12.0, by + 180.0, 23.0, 35.0, False, 2),
            ("side2", 12.0, by + 223.0, 23.0, 35.0, False, 1),
        ]
        for kn, sx, sy, sw, sh, _p, _d in self._side_specs:
            self.hitboxes[kn] = (sx - 2.0, sy, sw + 4.0, sh)

        # Exposed stubby antenna base + top rotary shaft (no decorative outer collar)
        ant_cx = bx + 54.0
        round_rect(back_cr, ant_cx - 19.0, 4.0, 38.0, by + 8.0, 5.0)
        pat_ant = cairo.LinearGradient(ant_cx - 19.0, 0, ant_cx + 19.0, 0)
        pat_ant.add_color_stop_rgb(0.0, 0.08, 0.085, 0.095)
        pat_ant.add_color_stop_rgb(0.5, 0.19, 0.20, 0.22)
        pat_ant.add_color_stop_rgb(1.0, 0.06, 0.065, 0.075)
        back_cr.set_source(pat_ant)
        back_cr.fill()

        kn_x, kn_w = bx + bw - 82.0, 52.0
        round_rect(back_cr, kn_x, 16.0, kn_w, by - 8.0, 5.0)
        pat_kn = cairo.LinearGradient(kn_x, 0, kn_x + kn_w, 0)
        pat_kn.add_color_stop_rgb(0.0, 0.11, 0.115, 0.125)
        pat_kn.add_color_stop_rgb(0.5, 0.23, 0.24, 0.26)
        pat_kn.add_color_stop_rgb(1.0, 0.08, 0.085, 0.095)
        back_cr.set_source(pat_kn)
        back_cr.fill()

        # Left side shroud
        draw_k1_side_shroud(back_cr, bx, by, bh, self._side_specs)

        # Matte-black molded inner housing (exposed without anodized armor plate)
        round_rect(cr, bx, by, bw, bh, 18.0)
        pat_b = cairo.LinearGradient(bx, by, bx + bw, by + bh)
        pat_b.add_color_stop_rgb(0.0, 0.135, 0.14, 0.15)
        pat_b.add_color_stop_rgb(0.5, 0.095, 0.10, 0.11)
        pat_b.add_color_stop_rgb(1.0, 0.07, 0.075, 0.085)
        cr.set_source(pat_b)
        cr.fill_preserve()
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.09)
        cr.set_line_width(1.0)
        cr.stroke()

        # 4 exposed black screw bosses where the top armor plate mounts
        for sx, sy in ((bx + 18.0, by + 22.0), (bx + bw - 18.0, by + 22.0), (bx + 18.0, by + bh - 34.0), (bx + bw - 18.0, by + bh - 34.0)):
            cr.new_path()
            cr.arc(sx, sy, 6.2, 0, 2 * math.pi)
            cr.set_source_rgb(0.05, 0.055, 0.06)
            cr.fill()
            cr.new_path()
            cr.arc(sx, sy, 2.8, 0, 2 * math.pi)
            cr.set_source_rgb(0.55, 0.44, 0.22)  # Brass threaded insert
            cr.fill()

        # Exposed mic pinhole above LCD right
        cr.new_path()
        cr.arc(bx + bw - 48.0, by + 32.0, 4.5, 0, 2 * math.pi)
        cr.set_source_rgb(0.03, 0.032, 0.036)
        cr.fill()

        self._torch_anchor = (bx + bw * 0.50, by + 2.0, True)
        self.hitboxes["_torch"] = (bx + bw * 0.50 - 20.0, by - 14.0, 40.0, 26.0)
        self._led_anchor = (ant_cx + 30.0, by - 9.0, 22.0, 9.0, True)

        # Exposed ST7565 LCD plastic carrier frame (exact 1.50:1 glass ratio)
        lz_w = 228
        lz_h = int(round(lz_w / 1.46))  # 156 -> inner 216x144
        lz_x = int(bx + (bw - lz_w) * 0.5)
        lz_y = int(by + 48)
        round_rect(cr, lz_x - 6, lz_y - 6, lz_w + 12, lz_h + 12, 6.0)
        cr.set_source_rgb(0.15, 0.16, 0.175)
        cr.fill_preserve()
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.12)
        cr.set_line_width(1.0)
        cr.stroke()
        round_rect(cr, lz_x, lz_y, lz_w, lz_h, 4.0)
        cr.set_source_rgb(0.045, 0.05, 0.058)
        cr.fill()
        self._lcd_rect = (lz_x + 6, lz_y + 6, lz_w - 12, lz_h - 12)
        lcd_y, lcd_h = lz_y + 6, lz_h - 12

        # Exposed white/copper snap-dome keypad sheet (exact `K1_VIEW_D_FRONT.png` right half!)
        kp_x, kp_y, kp_w, kp_h = bx + 20.0, lcd_y + lcd_h + 24.0, bw - 40.0, bh - (lcd_y - by + lcd_h + 38.0)
        round_rect(cr, kp_x, kp_y, kp_w, kp_h, 6.0)
        cr.set_source_rgb(0.92, 0.93, 0.91)
        cr.fill_preserve()
        cr.set_source_rgba(0.20, 0.22, 0.24, 0.68)
        cr.set_line_width(1.2)
        cr.stroke()

        # Copper flex ribbon edge peeking on left + subtle copper matrix bus traces
        cr.set_source_rgb(0.72, 0.46, 0.18)
        cr.rectangle(kp_x + 2.0, kp_y + 20.0, 8.0, kp_h - 40.0)
        cr.fill()
        cr.set_source_rgba(0.76, 0.52, 0.22, 0.34)
        cr.set_line_width(1.1)
        for row_i in range(4):
            ry = kp_y + 24.0 + row_i * ((kp_h - 48.0) / 3.0)
            cr.move_to(kp_x + 12.0, ry)
            cr.line_to(kp_x + kp_w - 12.0, ry)
        cr.stroke()

        # 6 horizontal black acoustic speaker slots with subtle fine metallic mesh inside
        col_step = (kp_w - 28.0) / 4.0
        row_step = (kp_h - 24.0) / 4.0
        slot_w = (kp_w - 42.0) * 0.46
        for r_slot in range(3):
            sy = kp_y + 12.0 + (r_slot + 1) * row_step - 3.75
            for c_slot in range(2):
                sx = kp_x + 16.0 + c_slot * (slot_w + 10.0)
                round_rect(cr, sx, sy, slot_w, 7.5, 3.0)
                cr.set_source_rgb(0.05, 0.055, 0.065)
                cr.fill()
                # Subtle acoustic mesh cross-hatching
                cr.set_source_rgba(0.45, 0.48, 0.52, 0.25)
                cr.set_line_width(0.6)
                for mx in range(int(sx + 5), int(sx + slot_w - 4), 5):
                    cr.move_to(mx, sy + 1.2)
                    cr.line_to(mx, sy + 6.3)
                cr.stroke()
                # Tiny warm SMD backlight LED centered on each slot bridge
                round_rect(cr, sx + slot_w * 0.5 - 3.5, sy + 1.5, 7.0, 4.5, 1.0)
                cr.set_source_rgb(0.92, 0.78, 0.28)
                cr.fill()

        # 4×4 interactive stainless snap domes
        for r_i, row_keys in enumerate(K1_MATRIX):
            cy = kp_y + 12.0 + (r_i + 0.5) * row_step
            for c_i, (kn, pri, _s1, _s2) in enumerate(row_keys):
                cx = kp_x + 14.0 + (c_i + 0.5) * col_step
                d_sz = 33.0
                lbl = "<" if pri == "_K1_LT_" else (">" if pri == "_K1_GT_" else pri[:1] if len(pri) > 2 else pri)
                self.hitboxes[kn] = (cx - d_sz * 0.5, cy - d_sz * 0.5, d_sz, d_sz)
                self._key_specs.append((kn, cx - d_sz * 0.5, cy - d_sz * 0.5, d_sz, d_sz, lbl, "", "", 16.5, r_i == 0, False, "dome"))

    # =========================================================================
    # 4. Developer View: Bare R51-V2.1 Green PCB + Mounted ST7565 LCD (`media_1795270657071`)
    # =========================================================================
    def _build_pcb_r51(
        self, back_cr: Any, cr: Any, W: int, H: int, lcd_st: LcdSettings, spec: HandsetSpec
    ) -> None:
        import cairo

        bx, by, bw, bh = 38.0, 56.0, W - 76.0, H - 68.0
        self._body_bx = bx

        # Bare tactile side-switch micro-buttons soldered along the left PCB edge
        self._side_specs = [
            ("ptt", bx - 10.0, by + 162.0, 12.0, 26.0, True, 0),
            ("side1", bx - 10.0, by + 206.0, 12.0, 19.0, False, 0),
            ("side2", bx - 10.0, by + 236.0, 12.0, 19.0, False, 0),
        ]
        for kn, sx, sy, sw, sh, _p, _d in self._side_specs:
            self.hitboxes[kn] = (sx - 4.0, sy - 2.0, sw + 8.0, sh + 4.0)

        # Soft workbench drop shadow beneath PCB
        round_rect(back_cr, bx + 4, by + 10, bw, bh, 20.0)
        back_cr.set_source_rgba(0.0, 0.0, 0.0, 0.45)
        back_cr.fill()

        # Top-edge exposed hardware: Brass threaded SMA bulkhead + knurled nut, amber Kapton LED tab, steel encoder shaft
        sma_x = bx + 42.0
        round_rect(back_cr, sma_x - 16.0, 24.0, 32.0, by - 16.0, 3.0)
        pat_sma = cairo.LinearGradient(sma_x - 16.0, 0, sma_x + 16.0, 0)
        pat_sma.add_color_stop_rgb(0.0, 0.48, 0.38, 0.16)
        pat_sma.add_color_stop_rgb(0.32, 0.96, 0.84, 0.48)
        pat_sma.add_color_stop_rgb(0.68, 0.72, 0.58, 0.28)
        pat_sma.add_color_stop_rgb(1.0, 0.38, 0.28, 0.12)
        back_cr.set_source(pat_sma)
        back_cr.fill()
        back_cr.set_source_rgba(0.22, 0.14, 0.03, 0.68)
        back_cr.set_line_width(1.15)
        for ty in range(28, int(by - 4), 4):
            back_cr.move_to(sma_x - 15.0, ty)
            back_cr.line_to(sma_x + 15.0, ty)
        back_cr.stroke()
        round_rect(back_cr, sma_x - 19.5, by - 11.0, 39.0, 13.0, 2.0)
        back_cr.set_source(pat_sma)
        back_cr.fill()

        # Golden Kapton flex bridge + translucent 5mm clear glass dome LED at top center
        led_cx = bx + bw * 0.49
        round_rect(back_cr, led_cx - 17.0, by - 14.0, 34.0, 18.0, 3.0)
        back_cr.set_source_rgb(0.74, 0.45, 0.10)
        back_cr.fill()
        back_cr.new_path()
        back_cr.arc(led_cx, by - 25.0, 11.0, math.pi, 2 * math.pi)
        back_cr.line_to(led_cx + 11.0, by - 11.0)
        back_cr.line_to(led_cx - 11.0, by - 11.0)
        back_cr.close_path()
        pat_dome = cairo.LinearGradient(led_cx - 11.0, 0, led_cx + 11.0, 0)
        pat_dome.add_color_stop_rgba(0.0, 0.68, 0.72, 0.76, 0.78)
        pat_dome.add_color_stop_rgba(0.35, 0.96, 0.98, 1.0, 0.92)
        pat_dome.add_color_stop_rgba(0.75, 0.72, 0.76, 0.80, 0.82)
        pat_dome.add_color_stop_rgba(1.0, 0.50, 0.54, 0.58, 0.74)
        back_cr.set_source(pat_dome)
        back_cr.fill_preserve()
        back_cr.set_source_rgba(1.0, 1.0, 1.0, 0.55)
        back_cr.set_line_width(0.8)
        back_cr.stroke()
        # Internal metallic anvil + post lead frame inside clear glass LED
        back_cr.set_source_rgb(0.48, 0.52, 0.56)
        back_cr.rectangle(led_cx - 4.2, by - 23.0, 2.8, 10.0)
        back_cr.rectangle(led_cx + 1.4, by - 25.0, 2.8, 12.0)
        back_cr.fill()

        # Bare brushed stainless cylindrical rotary encoder shaft (top right)
        sh_x = bx + bw - 32.0
        round_rect(back_cr, sh_x - 9.0, 8.0, 18.0, by, 3.5)
        pat_sh = cairo.LinearGradient(sh_x - 9.0, 0, sh_x + 9.0, 0)
        pat_sh.add_color_stop_rgb(0.0, 0.38, 0.40, 0.44)
        pat_sh.add_color_stop_rgb(0.34, 0.94, 0.96, 0.98)
        pat_sh.add_color_stop_rgb(0.70, 0.60, 0.62, 0.66)
        pat_sh.add_color_stop_rgb(1.0, 0.32, 0.34, 0.36)
        back_cr.set_source(pat_sh)
        back_cr.fill()

        # Right-edge exposed chrome Kenwood 3.5mm + 2.5mm audio jacks & steel USB-C shell
        for jy, jh, is_usb in ((by + 88.0, 20.0, False), (by + 120.0, 17.0, False), (by + 150.0, 24.0, True)):
            round_rect(back_cr, bx + bw - 3.0, jy, 10.5 if not is_usb else 7.5, jh, 2.5)
            back_cr.set_source_rgb(0.76, 0.78, 0.80) if is_usb else back_cr.set_source_rgb(0.62, 0.64, 0.67)
            back_cr.fill_preserve()
            back_cr.set_source_rgba(0.12, 0.14, 0.16, 0.72)
            back_cr.set_line_width(0.8)
            back_cr.stroke()
            if not is_usb:
                round_rect(back_cr, bx + bw + 3.0, jy + 3.5, 3.8, jh - 7.0, 1.0)
                back_cr.set_source_rgb(0.11, 0.12, 0.14)
                back_cr.fill()

        # Dark translucent fiberglass FR-4 base substrate (etch-trench color)
        round_rect(cr, bx, by, bw, bh, 20.0)
        pat_fr4 = cairo.LinearGradient(bx, by, bx + bw, by + bh)
        pat_fr4.add_color_stop_rgb(0.00, 0.065, 0.285, 0.140)
        pat_fr4.add_color_stop_rgb(0.50, 0.052, 0.235, 0.115)
        pat_fr4.add_color_stop_rgb(1.00, 0.042, 0.195, 0.095)
        cr.set_source(pat_fr4)
        cr.fill_preserve()
        cr.set_source_rgba(0.52, 0.82, 0.56, 0.58)
        cr.set_line_width(1.6)
        cr.stroke()

        # Raised brighter-green copper ground-pour islands & realistic routed 45° traces under solder mask!
        cr.save()
        round_rect(cr, bx + 2.5, by + 2.5, bw - 5.0, bh - 5.0, 18.0)
        cr.clip()

        # Continuous glossy emerald-green R51 copper ground pour (NO artificial parallel sample stripes!)
        pat_cu = cairo.LinearGradient(bx, by, bx + bw, by + bh)
        pat_cu.add_color_stop_rgb(0.0, 0.155, 0.535, 0.255)
        pat_cu.add_color_stop_rgb(0.45, 0.115, 0.445, 0.215)
        pat_cu.add_color_stop_rgb(1.0, 0.085, 0.355, 0.175)
        cr.set_source(pat_cu)
        round_rect(cr, bx + 4.5, by + 4.5, bw - 9.0, bh - 9.0, 17.0)
        cr.fill()

        # Subtle glossy diagonal solder-mask specular sheen across real fiberglass board
        pat_gl = cairo.LinearGradient(bx, by + 120.0, bx + bw, by + 380.0)
        pat_gl.add_color_stop_rgba(0.00, 1.0, 1.0, 1.0, 0.0)
        pat_gl.add_color_stop_rgba(0.46, 1.0, 1.0, 1.0, 0.065)
        pat_gl.add_color_stop_rgba(0.54, 1.0, 1.0, 1.0, 0.0)
        cr.set_source(pat_gl)
        cr.rectangle(bx, by, bw, bh)
        cr.fill()

        # Real organic point-to-point copper traces (dark etch-clearance trenches + raised copper trace inside)
        real_traces = [
            # MENU dome to MCU/under-LCD bus
            [(bx + 54, by + 180), (bx + 66, by + 180), (bx + 66, by + 158)],
            # SPI flash 25Q64 pins to upper-left decoupling & MCU vias
            [(bx + 84, by + 272), (bx + 84, by + 248), (bx + 102, by + 230), (bx + 102, by + 206)],
            [(bx + 96, by + 280), (bx + 114, by + 280), (bx + 128, by + 266), (bx + 128, by + 244)],
            # Audio amp LM4890 to SP+/SP- speaker pads & electret mic
            [(bx + 174, by + 300), (bx + 202, by + 300), (bx + 216, by + 288), (bx + 240, by + 288)],
            [(bx + 174, by + 306), (bx + 214, by + 306), (bx + 226, by + 318)],
            # Right-side Kenwood audio jack routing into upper-right SMD cluster
            [(bx + 258, by + 168), (bx + 228, by + 168), (bx + 216, by + 180), (bx + 216, by + 206)],
            [(bx + 258, by + 184), (bx + 236, by + 184), (bx + 224, by + 196), (bx + 224, by + 230)],
            # Keypad matrix column bus feeding bottom domes
            [(bx + 76, by + 312), (bx + 76, by + 344), (bx + 91, by + 356)],
            [(bx + 144, by + 316), (bx + 144, by + 346)],
        ]
        for pts in real_traces:
            cr.new_path()
            cr.move_to(*pts[0])
            for px_t, py_t in pts[1:]:
                cr.line_to(px_t, py_t)
            cr.set_source_rgba(0.045, 0.20, 0.095, 0.85)
            cr.set_line_width(2.6)
            cr.stroke_preserve()
            cr.set_source_rgba(0.18, 0.60, 0.30, 0.92)
            cr.set_line_width(1.1)
            cr.stroke()

        # Left-edge exposed tin-plated ground shield castellations & solder fillets
        for cy_tab in (by + 98.0, by + 128.0, by + 188.0, by + 266.0):
            round_rect(cr, bx - 1.0, cy_tab, 8.5, 11.5, 2.0)
            pat_sn = cairo.LinearGradient(bx, cy_tab, bx + 8.0, cy_tab + 11.0)
            pat_sn.add_color_stop_rgb(0.0, 0.88, 0.90, 0.90)
            pat_sn.add_color_stop_rgb(1.0, 0.56, 0.58, 0.60)
            cr.set_source(pat_sn)
            cr.fill()

        # Asymmetric brushed ENIG gold ground plane polygon directly under LCD with drilled thermal vias!
        cr.new_path()
        cr.move_to(bx + 72.0, by + 162.0)
        cr.line_to(bx + 144.0, by + 162.0)
        cr.line_to(bx + 144.0, by + 194.0)
        cr.line_to(bx + 122.0, by + 206.0)
        cr.line_to(bx + 72.0, by + 206.0)
        cr.close_path()
        pat_au = cairo.LinearGradient(bx + 72.0, by + 162.0, bx + 144.0, by + 206.0)
        pat_au.add_color_stop_rgb(0.0, 0.88, 0.74, 0.44)
        pat_au.add_color_stop_rgb(0.55, 0.76, 0.62, 0.34)
        pat_au.add_color_stop_rgb(1.0, 0.62, 0.48, 0.25)
        cr.set_source(pat_au)
        cr.fill()

        # Stitched gold micro-vias + circular tin test points across the board
        via_pts = [
            (bx + 82, by + 171), (bx + 98, by + 171), (bx + 114, by + 171), (bx + 132, by + 171),
            (bx + 82, by + 188), (bx + 132, by + 188), (bx + 112, by + 199),
            (bx + 168, by + 216), (bx + 186, by + 228), (bx + 208, by + 246),
            (bx + 124, by + 258), (bx + 140, by + 276), (bx + 224, by + 278),
            (bx + 240, by + 240), (bx + 240, by + 256), (bx + 240, by + 274),
        ]
        for vx, vy in via_pts:
            cr.new_path()
            cr.arc(vx, vy, 2.2, 0, 2 * math.pi)
            cr.set_source_rgb(0.86, 0.72, 0.38)
            cr.fill()
            cr.new_path()
            cr.arc(vx, vy, 0.95, 0, 2 * math.pi)
            cr.set_source_rgb(0.03, 0.12, 0.06)
            cr.fill()

        # Bright circular ENIG/tin test points along right speaker/debug edge
        for tpx, tpy in ((bx + 242, by + 294), (bx + 242, by + 306), (bx + 144, by + 244), (bx + 214, by + 218)):
            cr.new_path()
            cr.arc(tpx, tpy, 3.6, 0, 2 * math.pi)
            cr.set_source_rgb(0.88, 0.76, 0.40)
            cr.fill_preserve()
            cr.set_source_rgba(0.28, 0.22, 0.08, 0.55)
            cr.set_line_width(0.6)
            cr.stroke()
        cr.restore()

        # Crisp white PCB silkscreen (`R51-V2.1`, `YL13`, `XHR 2547`, `SP`)
        draw_pango_text(cr, "R51-V2.1", bx + bw * 0.54, by + 216.0, font_desc="Monospace Bold 7.6", rgba=(0.96, 0.99, 0.96, 0.94), halign="center")
        draw_pango_text(cr, "YL13", bx + bw * 0.54, by + 227.0, font_desc="Monospace Bold 6.2", rgba=(0.92, 0.96, 0.92, 0.84), halign="center")
        draw_pango_text(cr, "XHR 2547", bx + bw * 0.62, by + 244.0, font_desc="Monospace 6.8", rgba=(0.90, 0.96, 0.90, 0.78), halign="center")
        draw_pango_text(cr, "SP", bx + bw - 22.0, by + 286.0, font_desc="Monospace Bold 6.0", rgba=(0.94, 0.98, 0.94, 0.88), halign="center")

        # Multi-pin black SMD ICs (SPI Flash 25Q64, Audio Amp LM4890, LDO) with shiny tin leads & pin-1 dot
        for ix_ic, iy_ic, iw_ic, ih_ic, n_pins, ic_lbl in (
            (bx + 72.0, by + 272.0, 23.0, 21.0, 4, "25Q"),
            (bx + 152.0, by + 294.0, 22.0, 14.5, 4, "4890"),
            (bx + 190.0, by + 206.0, 14.5, 14.5, 3, ""),
        ):
            round_rect(cr, ix_ic + 1.0, iy_ic + 1.2, iw_ic, ih_ic, 2.0)
            cr.set_source_rgba(0.0, 0.0, 0.0, 0.42)
            cr.fill()
            round_rect(cr, ix_ic, iy_ic, iw_ic, ih_ic, 2.0)
            cr.set_source_rgb(0.075, 0.08, 0.09)
            cr.fill_preserve()
            cr.set_source_rgba(1.0, 1.0, 1.0, 0.16)
            cr.set_line_width(0.6)
            cr.stroke()
            cr.set_source_rgb(0.84, 0.86, 0.88)
            for p_i in range(n_pins):
                py_pin = iy_ic + 2.5 + p_i * ((ih_ic - 5.0) / max(1, n_pins - 1))
                cr.rectangle(ix_ic - 3.0, py_pin - 0.9, 3.0, 1.8)
                cr.rectangle(ix_ic + iw_ic, py_pin - 0.9, 3.0, 1.8)
            cr.fill()
            cr.new_path()
            cr.arc(ix_ic + 3.5, iy_ic + 3.5, 1.1, 0, 2 * math.pi)
            cr.set_source_rgba(0.88, 0.90, 0.92, 0.72)
            cr.fill()
            if ic_lbl:
                draw_pango_text(cr, ic_lbl, ix_ic + iw_ic * 0.5, iy_ic + ih_ic * 0.55, font_desc="Monospace 4.4", rgba=(0.78, 0.80, 0.82, 0.68), halign="center")

        # Dense 0402 SMD ceramic capacitors (ochre/tan) & black chip resistors with realistic silver solder fillets
        smd_parts = [
            (bx + 156, by + 168, True, False), (bx + 166, by + 168, False, False), (bx + 178, by + 176, True, True),
            (bx + 198, by + 168, False, False), (bx + 212, by + 168, True, False), (bx + 158, by + 186, True, True),
            (bx + 72, by + 226, False, False), (bx + 84, by + 226, True, False), (bx + 72, by + 238, True, False),
            (bx + 84, by + 238, False, True), (bx + 98, by + 234, True, False), (bx + 110, by + 250, False, False),
            (bx + 102, by + 276, True, True), (bx + 102, by + 286, False, True), (bx + 138, by + 294, True, False),
            (bx + 138, by + 304, False, False), (bx + 182, by + 298, True, False), (bx + 194, by + 298, False, True),
            (bx + 212, by + 262, True, False), (bx + 224, by + 262, True, False), (bx + 218, by + 296, False, False),
            (bx + 116, by + 274, True, False), (bx + 126, by + 274, False, False), (bx + 164, by + 270, True, True),
            (bx + 180, by + 270, False, True), (bx + 196, by + 272, True, False), (bx + 206, by + 226, False, True),
        ]
        for px_s, py_s, is_cap, horiz in smd_parts:
            sw, sh = (7.2, 3.8) if horiz else (3.8, 7.2)
            cr.set_source_rgb(0.84, 0.86, 0.88)
            cr.rectangle(px_s, py_s, sw, sh)
            cr.fill()
            cr.set_source_rgb(0.70, 0.52, 0.26) if is_cap else cr.set_source_rgb(0.10, 0.11, 0.12)
            if horiz:
                cr.rectangle(px_s + 1.5, py_s, sw - 3.0, sh)
            else:
                cr.rectangle(px_s, py_s + 1.5, sw, sh - 3.0)
            cr.fill()

        # Electret microphone brushed silver can + dark acoustic felt mesh (lower right)
        mic_cx, mic_cy = bx + bw - 32.0, by + 322.0
        cr.new_path()
        cr.arc(mic_cx, mic_cy, 15.0, 0, 2 * math.pi)
        cr.set_source_rgb(0.74, 0.76, 0.78)
        cr.fill()
        cr.new_path()
        cr.arc(mic_cx, mic_cy, 11.8, 0, 2 * math.pi)
        cr.set_source_rgb(0.14, 0.15, 0.165)
        cr.fill()
        cr.new_path()
        cr.arc(mic_cx, mic_cy, 11.8, 0, 2 * math.pi)
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.18)
        cr.set_line_width(0.8)
        cr.stroke()

        # 3 machined silver Phillips pan-head mounting screws (matching exact R51 screw locations!)
        for scx, scy in ((bx + 90.0, by + 208.0), (bx + bw - 25.0, by + 188.0), (bx + bw * 0.50, by + bh - 14.0)):
            draw_phillips_screw(cr, scx, scy, 7.0)

        # Gold SWD Debug Test Pads (`GND`, `CLK`, `DIO`, `3V3`) along lower-right PCB edge!
        swd_x = bx + bw - 12.5
        for idx_p, (pad_lbl, is_sq) in enumerate((("GND", True), ("CLK", False), ("DIO", False), ("3V3", False))):
            py = by + bh - 108.0 + idx_p * 21.5
            cr.set_source_rgb(0.90, 0.76, 0.36)
            if is_sq:
                cr.rectangle(swd_x - 4.6, py - 4.6, 9.2, 9.2)
            else:
                cr.new_path()
                cr.arc(swd_x, py, 4.6, 0, 2 * math.pi)
            cr.fill_preserve()
            cr.set_source_rgba(0.32, 0.24, 0.08, 0.65)
            cr.set_line_width(0.6)
            cr.stroke()
            draw_pango_text(cr, pad_lbl, swd_x - 8.0, py, font_desc="Monospace Bold 5.2", rgba=(0.94, 0.98, 0.94, 0.88), halign="right", valign="center")

        # Flush top-mounted ST7565 LCD module in white polycarbonate carrier + glossy black stamped top lip!
        self._torch_anchor = (led_cx, by - 14.0, True)
        self.hitboxes["_torch"] = (led_cx - 16.0, by - 36.0, 32.0, 30.0)
        self._led_anchor = (bx + 24.0, by + 154.0, 11.0, 6.5, True)

        lcd_w, lcd_h = 208, 140  # Exact 1.485:1 physical ST7565 glass window!
        lcd_x = int(bx + (bw - lcd_w) * 0.5)
        lcd_y = int(by + 12)
        self._lcd_rect = (lcd_x, lcd_y, lcd_w, lcd_h)
        round_rect(cr, lcd_x - 10, lcd_y - 10, lcd_w + 20, lcd_h + 18, 4.0)
        cr.set_source_rgb(0.94, 0.95, 0.93)
        cr.fill_preserve()
        cr.set_source_rgba(0.12, 0.14, 0.16, 0.45)
        cr.set_line_width(0.9)
        cr.stroke()
        round_rect(cr, lcd_x - 6, lcd_y - 10, lcd_w + 12, lcd_h + 14, 2.0)
        cr.set_source_rgb(0.075, 0.08, 0.09)
        cr.fill()
        cr.set_source_rgb(0.95, 0.96, 0.94)
        cr.rectangle(lcd_x - 10, lcd_y - 2, 4, lcd_h + 6)
        cr.rectangle(lcd_x + lcd_w + 6, lcd_y - 2, 4, lcd_h + 6)
        cr.fill()

        # 4 Left-column snap domes (MENU, UP, DOWN, EXIT) with individual white circular silkscreen rings
        left_x = bx + 38.0
        for kn, pri, ly in (("menu", "M", by + 180.0), ("up", "▲", by + 230.0), ("down", "▼", by + 272.0), ("exit", "E", by + 322.0)):
            d_sz = 32.0
            self.hitboxes[kn] = (left_x - d_sz * 0.5, ly - d_sz * 0.5, d_sz, d_sz)
            self._key_specs.append((kn, left_x - d_sz * 0.5, ly - d_sz * 0.5, d_sz, d_sz, pri, "", "", 16.0, True, False, "dome"))

        # Signature R51 tight vertical white silkscreen stadium capsules wrapping each 3-dome numeric column!
        kp_x0 = bx + 38.0
        col_gap = 53.0
        row_y0 = by + 368.0
        row_gap = 42.0
        for c_i in range(4):
            cx_col = kp_x0 + c_i * col_gap
            round_rect(cr, cx_col - 19.0, row_y0 - 19.0, 38.0, row_gap * 2.0 + 38.0, 19.0)
            cr.set_source_rgba(0.96, 0.98, 0.96, 0.92)
            cr.set_line_width(2.0)
            cr.stroke()

        for r_i, row_keys in enumerate(K5_BOTTOM_MATRIX):
            cy = row_y0 + r_i * row_gap
            for c_i, (kn, pri, _s1, _s2) in enumerate(row_keys):
                cx = kp_x0 + c_i * col_gap
                d_sz = 31.0
                self.hitboxes[kn] = (cx - d_sz * 0.5, cy - d_sz * 0.5, d_sz, d_sz)
                self._key_specs.append((kn, cx - d_sz * 0.5, cy - d_sz * 0.5, d_sz, d_sz, pri, "", "", 15.5, False, kn == "5", "dome-col"))

        # Tiny glowing amber/warm-white 0603 SMD keypad LEDs beside domes
        led_alpha = 0.92 if lcd_st.backlight > 0 else 0.30
        for lx_smd, ly_smd in (
            (bx + 24.0, by + 206.0), (bx + 24.0, by + 298.0),
            (bx + 65.0, by + 388.0), (bx + 65.0, by + 430.0),
            (bx + 171.0, by + 388.0), (bx + 171.0, by + 430.0),
        ):
            round_rect(cr, lx_smd - 4.0, ly_smd - 2.5, 8.0, 5.0, 1.0)
            cr.set_source_rgb(0.90, 0.92, 0.90)
            cr.fill()
            round_rect(cr, lx_smd - 2.3, ly_smd - 1.5, 4.6, 3.0, 0.6)
            cr.set_source_rgba(0.98, 0.86, 0.34, led_alpha)
            cr.fill()

    # =========================================================================
    # Paint Pass (Cached back + animated side keys + cached chassis + dynamic LED/torch/LCD/active keys)
    # =========================================================================
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

        self._ensure_chassis(model, lcd_st)
        spec = self._spec
        accent = spec.accent
        bx = self._body_bx
        is_k1_collar = spec.chassis in ("k1-metal", "k1-clear", "k10-pro")
        is_k5_side = spec.chassis in ("k5v3", "k5-black", "k5-99", "uv5r-plus")

        cr.set_source_surface(self._back_surf, 0, 0)
        cr.paint()

        for kn, sx, sy, sw, sh, is_ptt, dots in self._side_specs:
            p = clamp1(press_anim.get(kn, 0.0))
            hv = clamp1(hover_anim.get(kn, 0.0))
            dx = 3.2 * p
            round_rect(cr, sx + dx - 1.4, sy + 2.2, sw, sh, 8 if is_ptt else 6)
            cr.set_source_rgba(0.0, 0.0, 0.0, 0.52 * (1.0 - 0.5 * p))
            cr.fill()

            round_rect(cr, sx + dx, sy, sw, sh, 8 if is_ptt else 6)
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
                if is_k1_collar:
                    c_r = accent[0] * (1.0 - p) + 0.98 * p
                    c_g = accent[1] * (1.0 - p) + 0.28 * p
                    c_b = accent[2] * (1.0 - p) + 0.26 * p
                    pat_c = cairo.LinearGradient(sx + dx, sy, sx + dx, sy + sh)
                    pat_c.add_color_stop_rgb(0.0, min(1.0, c_r * 1.15), min(1.0, c_g * 1.15), min(1.0, c_b * 1.15))
                    pat_c.add_color_stop_rgb(0.5, c_r * 0.88, c_g * 0.88, c_b * 0.88)
                    pat_c.add_color_stop_rgb(1.0, c_r * 0.56, c_g * 0.56, c_b * 0.56)
                    cr.set_source(pat_c)
                    cr.set_line_width(2.2)
                    cr.stroke()
                    for ry in (sy + 18, sy + 40, sy + 62, sy + 84):
                        round_rect(cr, sx + 6.4 + dx, ry - 0.8, 5.2, 13.6, 2.4)
                        cr.set_source_rgba(0.02, 0.02, 0.03, 0.75)
                        cr.fill()
                        round_rect(cr, sx + 7.2 + dx, ry, 3.6, 12.0, 1.7)
                        cr.set_source_rgb(
                            accent[0] * (1.0 - p) + 1.0 * p,
                            accent[1] * (1.0 - p) + 0.72 * p,
                            accent[2] * (1.0 - p) + 0.72 * p,
                        )
                        cr.fill()
                else:
                    if p > 0.05:
                        cr.set_source_rgba(0.98, 0.30, 0.28, 0.92 * p)
                        cr.set_line_width(1.7)
                    else:
                        cr.set_source_rgba(1.0, 1.0, 1.0, 0.18 + 0.10 * hv)
                        cr.set_line_width(1.05)
                    cr.stroke()
                    round_rect(cr, sx + 2.5 + dx, sy + 5.0, sw - 9.0, sh - 10.0, 4.5)
                    pat_in = cairo.LinearGradient(sx + 2.5 + dx, sy, sx + 2.5 + dx + (sw - 9.0), sy)
                    pat_in.add_color_stop_rgb(0.0, 0.27 + 0.06 * hv - 0.05 * p, 0.28 + 0.06 * hv - 0.05 * p, 0.30 + 0.06 * hv - 0.05 * p)
                    pat_in.add_color_stop_rgb(0.65, 0.16, 0.17, 0.19)
                    pat_in.add_color_stop_rgb(1.0, 0.09, 0.10, 0.11)
                    cr.set_source(pat_in)
                    cr.fill_preserve()
                    cr.set_source_rgba(1.0, 1.0, 1.0, 0.10)
                    cr.set_line_width(0.75)
                    cr.stroke()
                    gv_x, gv_y, gv_h = sx + 5.5 + dx, sy + 20.0, sh - 40.0
                    round_rect(cr, gv_x, gv_y, 3.6, gv_h, 1.8)
                    cr.set_source_rgba(0.03, 0.035, 0.04, 0.88)
                    cr.fill()
            else:
                if p > 0.05:
                    cr.set_source_rgba(accent[0], accent[1], accent[2], 0.90 * p)
                    cr.set_line_width(1.3)
                else:
                    cr.set_source_rgba(1.0, 1.0, 1.0, 0.16 + 0.10 * hv)
                    cr.set_line_width(1.0)
                cr.stroke()
                if is_k5_side:
                    round_rect(cr, sx + 2.6 + dx, sy + 4.0, sw - 9.5, sh - 8.0, 3.8)
                    cr.set_source_rgba(1.0, 1.0, 1.0, 0.07 + 0.06 * hv)
                    cr.fill_preserve()
                    cr.set_source_rgba(0.02, 0.02, 0.03, 0.55)
                    cr.set_line_width(0.8)
                    cr.stroke()
                    mx_s = sx + 5.6 + dx
                    cr.set_source_rgba(0.76, 0.79, 0.84, 0.56 if p < 0.05 else 0.92)
                    cr.set_line_width(1.05)
                    if dots == 1:
                        round_rect(cr, mx_s, sy + sh * 0.5 - 3.0, 4.4, 6.0, 1.1)
                        cr.stroke()
                    elif dots == 2:
                        round_rect(cr, mx_s, sy + sh * 0.5 - 6.8, 4.4, 5.4, 1.0)
                        cr.stroke()
                        round_rect(cr, mx_s, sy + sh * 0.5 + 1.4, 4.4, 5.4, 1.0)
                        cr.stroke()
                elif dots > 0:
                    cr.set_source_rgba(0.84, 0.87, 0.90, 0.72)
                    if dots == 2:
                        for cy_d in (sy + sh * 0.37, sy + sh * 0.63):
                            cr.new_path()
                            cr.arc(sx + 8.0 + dx, cy_d, 1.9, 0, 2 * math.pi)
                            cr.fill()
                    else:
                        cr.new_path()
                        cr.arc(sx + 8.0 + dx, sy + sh * 0.50, 1.9, 0, 2 * math.pi)
                        cr.fill()

            sh_ov = cairo.LinearGradient(bx - 7.5, 0, bx + 1.0, 0)
            sh_ov.add_color_stop_rgba(0.0, 0.0, 0.0, 0.0, 0.0)
            sh_ov.add_color_stop_rgba(1.0, 0.0, 0.0, 0.0, 0.78)
            cr.set_source(sh_ov)
            cr.rectangle(bx - 7.5, sy - 1.0, 8.5, sh + 2.0)
            cr.fill()

        cr.set_source_surface(self._chassis_surf, 0, 0)
        cr.paint()

        tx, ty_base, is_compact = self._torch_anchor
        draw_top_flashlight(cr, tx, ty_base, torch_on, compact=is_compact, bare_led=(spec.chassis in ("pcb-r51", "k10-pro")))
        lx_led, ly_led, lw_led, lh_led, rect_led = self._led_anchor
        if spec.chassis == "k10-pro":
            draw_k10_logo_led(cr, lx_led, ly_led, lw_led, led_state)
        else:
            draw_frosted_led(cr, lx_led, ly_led, lw_led, lh_led, led_state, rectangular=rect_led)

        lx, ly, lw, lh = self._lcd_rect
        paint_fast_lcd(cr, lx, ly, lw, lh, lcd_state, lcd_st, self.lcd_cache)

        for kn, kx, ky, kw, kh, pri, s1, s2, rad, is_nav, nub, kstyle in self._key_specs:
            pt = press_anim.get(kn, 0.0)
            ht = hover_anim.get(kn, 0.0)
            if pt > 0.005 or ht > 0.005:
                self._paint_key(cr, kx, ky, kw, kh, pri, s1, s2, rad, is_nav, nub, kstyle, pt, ht, spec)

    @staticmethod
    def _paint_key(
        cr: Any, kx: float, ky: float, kw: float, kh: float,
        pri: str, s1: str, s2: str, rad: float, is_nav: bool, nub: bool,
        kstyle: str, pt: float, ht: float, spec: HandsetSpec,
    ) -> None:
        if kstyle == "k1":
            draw_k1_keycap(cr, kx, ky, kw, kh, pri, s1, s2, press_t=pt, hover_t=ht, is_nav=is_nav, homing_pip=nub, accent=spec.accent)
        elif kstyle.startswith("k10"):
            corner = kstyle[4:] if len(kstyle) > 4 else ""
            draw_k10_keycap(
                cr, kx, ky, kw, kh, pri, s1, s2,
                press_t=pt, hover_t=ht, is_nav=is_nav, homing_pip=nub, corner=corner, accent=spec.accent,
            )
        elif kstyle in ("dome", "dome-col"):
            draw_snap_dome(
                cr, kx + kw * 0.5, ky + kh * 0.5, min(kw, kh) * 0.44, pri,
                press_t=pt, hover_t=ht, accent=spec.accent,
                white_ring=(kstyle == "dome" and spec.chassis == "pcb-r51"),
            )
        elif kstyle in ("k5-diag-top", "k5-diag-bot"):
            import cairo

            dy = 1.4 * pt
            cut = 16.0
            def _diag_path(ox: float, oy: float, ow: float, oh: float) -> None:
                cr.new_sub_path()
                if kstyle == "k5-diag-top":
                    cr.move_to(ox + 6.0, oy)
                    cr.line_to(ox + ow - 4.0, oy)
                    cr.line_to(ox + ow, oy + 4.0)
                    cr.line_to(ox + ow - cut, oy + oh)
                    cr.line_to(ox + 6.0, oy + oh)
                    cr.curve_to(ox, oy + oh, ox, oy + oh - 6.0, ox, oy + oh - 6.0)
                    cr.line_to(ox, oy + 6.0)
                    cr.curve_to(ox, oy, ox + 6.0, oy, ox + 6.0, oy)
                else:
                    cr.move_to(ox + 6.0, oy)
                    cr.line_to(ox + ow - cut, oy)
                    cr.line_to(ox + ow, oy + oh - 4.0)
                    cr.line_to(ox + ow - 4.0, oy + oh)
                    cr.line_to(ox + 6.0, oy + oh)
                    cr.curve_to(ox, oy + oh, ox, oy + oh - 6.0, ox, oy + oh - 6.0)
                    cr.line_to(ox, oy + 6.0)
                    cr.curve_to(ox, oy, ox + 6.0, oy, ox + 6.0, oy)
                cr.close_path()

            _diag_path(kx - 1.2, ky - 1.2, kw + 2.4, kh + 2.4)
            cr.set_source_rgb(0.03, 0.035, 0.04)
            cr.fill()
            ix, iy, iw, ih = kx + 1.8, ky + 1.8 + dy, kw - 3.6, kh - 3.6
            _diag_path(ix, iy, iw, ih)
            t_l = 0.24 + 0.06 * ht - 0.09 * pt
            b_l = 0.12 + 0.025 * ht - 0.02 * pt
            pat_d = cairo.LinearGradient(ix, iy, ix, iy + ih)
            pat_d.add_color_stop_rgb(0.0, t_l, t_l, t_l + 0.01)
            pat_d.add_color_stop_rgb(1.0, b_l, b_l, b_l + 0.01)
            cr.set_source(pat_d)
            cr.fill_preserve()
            if pt > 0.02:
                cr.set_source_rgba(spec.accent[0], spec.accent[1], spec.accent[2], 0.90 * pt)
                cr.set_line_width(1.3)
            else:
                cr.set_source_rgba(1.0, 1.0, 1.0, 0.16 + 0.10 * ht)
                cr.set_line_width(0.9)
            cr.stroke()
            is_exit = pri == "EXIT"
            draw_pango_text(
                cr, pri, ix + iw * (0.30 if is_exit else 0.30), iy + ih * 0.50,
                font_desc="Adwaita Sans Bold Italic 8.0" if is_exit else "Adwaita Sans Bold Italic 12.6",
                rgba=(0.95, 0.96, 0.98, 0.98), halign="center", valign="center",
            )
            if s1.startswith("[") and s1.endswith("]") and len(s1) == 3:
                _draw_boxed_badge(cr, s1[1], ix + iw * (0.68 if is_exit else 0.62), iy + ih * 0.50, 8.2 if is_exit else 8.8)
        elif kstyle in ("k5-orange", "k5-blue"):
            import cairo

            tint = (0.94, 0.42, 0.10) if kstyle == "k5-orange" else (0.14, 0.54, 0.96)
            dy = 1.4 * pt
            round_rect(cr, kx - 1.0, ky - 1.0, kw + 2.0, kh + 2.0, rad + 1.0)
            cr.set_source_rgb(0.035, 0.04, 0.048)
            cr.fill()
            ix, iy, iw, ih = kx + 1.8, ky + 1.8 + dy, kw - 3.6, kh - 3.6
            round_rect(cr, ix, iy, iw, ih, max(3.0, rad - 1.2))
            pat_c = cairo.LinearGradient(ix, iy, ix, iy + ih)
            pat_c.add_color_stop_rgb(0.0, clamp1(tint[0] * (1.12 + 0.08 * ht - 0.16 * pt)), clamp1(tint[1] * (1.14 + 0.08 * ht - 0.16 * pt)), clamp1(tint[2] * (1.16 + 0.08 * ht - 0.16 * pt)))
            pat_c.add_color_stop_rgb(1.0, tint[0] * (0.78 - 0.12 * pt), tint[1] * (0.76 - 0.12 * pt), tint[2] * (0.78 - 0.12 * pt))
            cr.set_source(pat_c)
            cr.fill_preserve()
            cr.set_source_rgba(1.0, 1.0, 1.0, 0.28 + 0.16 * ht)
            cr.set_line_width(0.95)
            cr.stroke()
            is_exit = pri == "EXIT"
            draw_pango_text(
                cr, pri, ix + iw * (0.34 if is_exit else 0.36), iy + ih * 0.50,
                font_desc="Adwaita Sans Bold Italic 8.1" if is_exit else "Adwaita Sans Bold Italic 12.8",
                rgba=(1.0, 1.0, 1.0, 0.98), halign="center", valign="center",
            )
            if s1.startswith("[") and s1.endswith("]") and len(s1) == 3:
                _draw_boxed_badge(cr, s1[1], ix + iw * (0.80 if is_exit else 0.76), iy + ih * 0.50, 8.3 if is_exit else 9.0)
        else:
            draw_3d_keycap(cr, kx, ky, kw, kh, pri, s1, s2, press_t=pt, hover_t=ht, radius=rad, is_nav=is_nav, homing_nub=nub, accent=spec.accent)
