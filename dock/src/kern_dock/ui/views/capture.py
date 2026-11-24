"""Capture Studio: native GTK4/Libadwaita media studio for snapshots, raw framebuffers & GIFs."""

from __future__ import annotations

import dataclasses
import threading
import time
from pathlib import Path
from typing import Any, Callable

import gi

gi.require_version("Gtk", "4.0")
gi.require_version("Gdk", "4.0")
gi.require_version("Adw", "1")
from gi.repository import Adw, Gdk, GLib, Gtk  # noqa: E402

from ...render.capture import encode_png_rgba, render_handset_to_rgba, render_lcd_to_rgba
from ...render.handset.parts import draw_pango_text
from ...render.lcd import _lcd_colors_norm
from ...render.paint import round_rect
from ..runtime import DockRuntime
from ..widgets.handset_stage import HandsetStage


class CaptureView:
    """Adw.ToolbarView media studio: top action bar, hero ST7565 preview canvas, bottom timeline dock."""

    def __init__(
        self,
        win: Adw.ApplicationWindow,
        runtime: DockRuntime,
        stage: HandsetStage,
        *,
        on_toast: Callable[[str], None],
        on_recording_changed: Callable[[bool], None],
    ) -> None:
        self.win = win
        self.runtime = runtime
        self.stage = stage
        self._on_toast = on_toast
        self._on_recording_changed = on_recording_changed

        self._pb_playing = False
        self._pb_idx = 0
        self._pb_timer: int | None = None
        self._pb_seq_pos = 0
        self._cut_mark_start: int | None = None
        self._export_mode = "lcd"
        self._export_speed = 1.0
        self._cap_idle = True
        self._drag_start_x = 0.0

        self.widget = self._build()
        runtime.events.subscribe("link.frame", lambda _e: self._on_live_frame(), main_thread=True)
        runtime.events.subscribe("lcd.changed", lambda _e: self.preview_area.queue_draw(), main_thread=True)

    def _build(self) -> Adw.ToolbarView:
        view = Adw.ToolbarView()

        # === TOP BAR: STUDIO ACTION TOOLBAR ===
        top_bar = Gtk.Box(
            orientation=Gtk.Orientation.HORIZONTAL,
            spacing=8,
            margin_top=8,
            margin_bottom=8,
            margin_start=12,
            margin_end=12,
        )

        # Record toggle
        self.rec_btn = Gtk.ToggleButton(tooltip_text="Record live frames (Ctrl+R)")
        rec_in = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=6)
        rec_in.append(Gtk.Image.new_from_icon_name("media-record-symbolic"))
        self.rec_lbl = Gtk.Label(label="Record")
        rec_in.append(self.rec_lbl)
        self.rec_btn.set_child(rec_in)
        self.rec_btn.connect("toggled", self._on_record_toggled)
        top_bar.append(self.rec_btn)

        # Instant capture tools (PNG / Copy / Raw 1 KB FB)
        cap_grp = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL)
        cap_grp.add_css_class("linked")
        b_png = Gtk.Button(icon_name="camera-photo-symbolic", tooltip_text="Save PNG snapshot (Ctrl+S)")
        b_png.connect("clicked", lambda _b: self.save_screenshot())
        b_copy = Gtk.Button(icon_name="edit-copy-symbolic", tooltip_text="Copy PNG to clipboard")
        b_copy.connect("clicked", lambda _b: self.copy_screenshot())
        b_raw = Gtk.Button(label="1 KB FB", tooltip_text="Save raw 1024-byte ST7565 framebuffer (.bin)")
        b_raw.connect("clicked", lambda _b: self.save_raw_framebuffer())
        for b in (b_png, b_copy, b_raw):
            cap_grp.append(b)
        top_bar.append(cap_grp)

        top_bar.append(Gtk.Box(hexpand=True))

        # Target switch (LCD vs Full Radio)
        mode_grp = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL)
        mode_grp.add_css_class("linked")
        b_lcd = Gtk.ToggleButton(label="LCD", active=True, tooltip_text="Capture 128×64 ST7565 LCD")
        b_body = Gtk.ToggleButton(label="Handset", group=b_lcd, tooltip_text="Capture full radio handset")
        b_lcd.connect("toggled", lambda b: self._set_mode("lcd") if b.get_active() else None)
        b_body.connect("toggled", lambda b: self._set_mode("body") if b.get_active() else None)
        mode_grp.append(b_lcd)
        mode_grp.append(b_body)
        top_bar.append(mode_grp)

        # Export MenuButton (popover with speed, idle clamp, frame PNG & GIF export)
        self.export_menu_btn = Gtk.MenuButton(label="Export", tooltip_text="Export GIF, Frame PNG & options")
        self.export_menu_btn.add_css_class("suggested-action")
        self.export_menu_btn.set_popover(self._build_export_popover())
        top_bar.append(self.export_menu_btn)

        top_wrap = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        top_wrap.append(top_bar)
        top_wrap.append(Gtk.Separator(orientation=Gtk.Orientation.HORIZONTAL))
        view.add_top_bar(top_wrap)

        # === CENTER STAGE: HERO PREVIEW VIEWPORT ===
        stage_box = Gtk.Box(
            orientation=Gtk.Orientation.VERTICAL,
            spacing=8,
            vexpand=True,
            hexpand=True,
            margin_top=12,
            margin_bottom=8,
            margin_start=14,
            margin_end=14,
        )
        self.preview_area = Gtk.DrawingArea(hexpand=True, vexpand=True)
        self.preview_area.set_size_request(280, 180)
        self.preview_area.set_draw_func(self._draw_stage_preview)
        stage_box.append(self.preview_area)

        meta_bar = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        self.meta_left = Gtk.Label(label="Live Preview · 128×64 ST7565", xalign=0, hexpand=True, ellipsize=3)
        self.meta_left.add_css_class("caption")
        self.meta_left.add_css_class("dim-label")
        self.meta_right = Gtk.Label(label="No clip", xalign=1)
        self.meta_right.add_css_class("caption")
        self.meta_right.add_css_class("dim-label")
        self.meta_right.add_css_class("numeric")
        meta_bar.append(self.meta_left)
        meta_bar.append(self.meta_right)
        stage_box.append(meta_bar)

        self.gif_progress = Gtk.ProgressBar(show_text=True, visible=False)
        stage_box.append(self.gif_progress)
        view.set_content(stage_box)

        # === BOTTOM BAR: TIMELINE & TRANSPORT DOCK ===
        bot_wrap = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        bot_wrap.append(Gtk.Separator(orientation=Gtk.Orientation.HORIZONTAL))

        dock = Gtk.Box(
            orientation=Gtk.Orientation.VERTICAL,
            spacing=8,
            margin_top=10,
            margin_bottom=10,
            margin_start=12,
            margin_end=12,
        )

        self.timeline_area = Gtk.DrawingArea(content_height=40, hexpand=True)
        self.timeline_area.set_draw_func(self._draw_timeline)
        tl_c = Gtk.GestureClick()
        tl_c.connect("pressed", self._on_tl_click)
        self.timeline_area.add_controller(tl_c)
        tl_d = Gtk.GestureDrag()
        tl_d.connect("drag-update", self._on_tl_drag)
        self.timeline_area.add_controller(tl_d)
        dock.append(self.timeline_area)

        tr = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        nav_g = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL)
        nav_g.add_css_class("linked")
        self.start_btn = Gtk.Button(icon_name="media-skip-backward-symbolic", tooltip_text="First frame", sensitive=False)
        self.start_btn.connect("clicked", lambda _b: self._jump_to(0))
        self.prev_btn = Gtk.Button(icon_name="go-previous-symbolic", tooltip_text="Previous frame", sensitive=False)
        self.prev_btn.connect("clicked", lambda _b: self._step_pb(-1))
        self.pb_btn = Gtk.Button(icon_name="media-playback-start-symbolic", tooltip_text="Play / Pause", sensitive=False)
        self.pb_btn.connect("clicked", lambda _b: self._toggle_pb())
        self.next_btn = Gtk.Button(icon_name="go-next-symbolic", tooltip_text="Next frame", sensitive=False)
        self.next_btn.connect("clicked", lambda _b: self._step_pb(1))
        self.end_btn = Gtk.Button(icon_name="media-skip-forward-symbolic", tooltip_text="Last frame", sensitive=False)
        self.end_btn.connect("clicked", lambda _b: self._jump_to(max(0, len(self.runtime.recorder.frames) - 1)))
        for w in (self.start_btn, self.prev_btn, self.pb_btn, self.next_btn, self.end_btn):
            nav_g.append(w)
        tr.append(nav_g)

        tr.append(Gtk.Box(hexpand=True))

        edit_g = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL)
        edit_g.add_css_class("linked")
        self.b_in = Gtk.Button(label="In", tooltip_text="Trim start", sensitive=False)
        self.b_in.connect("clicked", lambda _b: self._set_trim_in())
        self.b_out = Gtk.Button(label="Out", tooltip_text="Trim end", sensitive=False)
        self.b_out.connect("clicked", lambda _b: self._set_trim_out())
        self.b_reset_trim = Gtk.Button(label="Reset", tooltip_text="Reset trim", sensitive=False)
        self.b_reset_trim.connect("clicked", lambda _b: self._reset_trim())
        self.cut_btn = Gtk.Button(label="Cut", tooltip_text="Mark splice cut", sensitive=False)
        self.cut_btn.connect("clicked", lambda _b: self._cut_splice())
        self.undo_btn = Gtk.Button(icon_name="edit-undo-symbolic", tooltip_text="Undo cut", sensitive=False)
        self.undo_btn.connect("clicked", lambda _b: (self.runtime.recorder.undo_cut(), self._refresh_ui()))
        for w in (self.b_in, self.b_out, self.b_reset_trim, self.cut_btn, self.undo_btn):
            edit_g.append(w)
        tr.append(edit_g)

        self.clr_btn = Gtk.Button(icon_name="user-trash-symbolic", tooltip_text="Discard clip", sensitive=False)
        self.clr_btn.add_css_class("flat")
        self.clr_btn.connect("clicked", lambda _b: self._discard())
        tr.append(self.clr_btn)

        dock.append(tr)
        bot_wrap.append(dock)
        view.add_bottom_bar(bot_wrap)
        return view

    def _build_export_popover(self) -> Gtk.Popover:
        pop = Gtk.Popover()
        box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=10, margin_top=12, margin_bottom=12, margin_start=12, margin_end=12)
        box.set_size_request(250, -1)

        spd_lbl = Gtk.Label(label="Playback Speed", xalign=0)
        spd_lbl.add_css_class("caption-heading")
        spd_lbl.add_css_class("dim-label")
        box.append(spd_lbl)

        sg = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, homogeneous=True)
        sg.add_css_class("linked")
        first_s = None
        for lbl, val in (("0.5×", 0.5), ("1×", 1.0), ("1.5×", 1.5), ("2×", 2.0)):
            sb = Gtk.ToggleButton(label=lbl, active=(val == 1.0))
            if first_s is None:
                first_s = sb
            else:
                sb.set_group(first_s)
            sb.connect("toggled", lambda b, v=val: (setattr(self, "_export_speed", v), self._refresh_ui()) if b.get_active() else None)
            sg.append(sb)
        box.append(sg)

        idle_row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        idle_lbl = Gtk.Label(label="Clamp idle pauses (0.6 s)", xalign=0, hexpand=True)
        idle_sw = Gtk.Switch(active=True, valign=Gtk.Align.CENTER)
        idle_sw.connect("notify::active", lambda sw, _p: (setattr(self, "_cap_idle", sw.get_active()), self._refresh_ui()))
        idle_row.append(idle_lbl)
        idle_row.append(idle_sw)
        box.append(idle_row)

        box.append(Gtk.Separator(orientation=Gtk.Orientation.HORIZONTAL))

        self.export_frame_btn = Gtk.Button(label="Export Selected Frame PNG…", sensitive=False)
        self.export_frame_btn.connect("clicked", lambda _b: (pop.popdown(), self._export_selected_frame_png()))
        box.append(self.export_frame_btn)

        self.export_btn = Gtk.Button(label="Export Animated GIF…", sensitive=False)
        self.export_btn.add_css_class("suggested-action")
        self.export_btn.connect("clicked", lambda _b: (pop.popdown(), self._export_gif()))
        box.append(self.export_btn)

        pop.set_child(box)
        return pop

    def _set_mode(self, mode: str) -> None:
        self._export_mode = mode
        self._refresh_meta()
        self.preview_area.queue_draw()

    def _on_live_frame(self) -> None:
        if not self.runtime.recorder.frames or self.runtime.recorder.recording:
            self.preview_area.queue_draw()

    def playback_frame_if_active(self, is_capture_page: bool) -> tuple[bytes, frozenset[str], str, bool] | None:
        rec = self.runtime.recorder
        if is_capture_page and (not rec.recording) and rec.frames:
            idx = max(0, min(self._pb_idx, len(rec.frames) - 1))
            rf = rec.frames[idx]
            return rf.fb, rf.pressed, rf.led, rf.torch
        return None

    def capture_event(self) -> None:
        rec = self.runtime.recorder
        if rec.recording:
            _fb, pr, led, torch = self.stage.current_display_state()
            rec.push(self.runtime.link.last_fb, pr, led, torch)
            self._update_rec_label()
            self.preview_area.queue_draw()

    def toggle_record(self) -> None:
        self.rec_btn.set_active(not self.rec_btn.get_active())

    def stop_playback(self) -> None:
        self._pb_playing = False
        if self._pb_timer is not None:
            GLib.source_remove(self._pb_timer)
            self._pb_timer = None
        self.pb_btn.set_icon_name("media-playback-start-symbolic")

    def _update_rec_label(self) -> None:
        rec = self.runtime.recorder
        if rec.recording:
            dt = int(time.monotonic() - rec.t0_mono)
            self.rec_lbl.set_text(f"Stop {dt // 60:02d}:{dt % 60:02d} ({len(rec.frames)}f)")
        else:
            self.rec_lbl.set_text("Record")

    def _on_record_toggled(self, btn: Any) -> None:
        rec = self.runtime.recorder
        if btn.get_active():
            self.stop_playback()
            _fb, pr, led, torch = self.stage.current_display_state()
            rec.start(self.runtime.link.last_fb, pr, led, torch)
            btn.add_css_class("rec-live")
            self._on_recording_changed(True)
            GLib.timeout_add(500, self._tick_rec_clock)
            self._update_rec_label()
        else:
            rec.stop()
            btn.remove_css_class("rec-live")
            self._on_recording_changed(False)
            self._update_rec_label()
            self._pb_idx = rec.trim_in
            self._refresh_ui()

    def _tick_rec_clock(self) -> bool:
        if not self.runtime.recorder.recording:
            return False
        self._update_rec_label()
        return True

    def _draw_stage_preview(self, _area: Any, cr: Any, w: int, h: int) -> None:
        import cairo

        round_rect(cr, 0, 0, w, h, 12)
        cr.set_source_rgb(0.075, 0.085, 0.105)
        cr.fill_preserve()
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.07)
        cr.set_line_width(1.0)
        cr.stroke()

        rec = self.runtime.recorder
        if rec.frames and not rec.recording:
            rf = rec.frames[max(0, min(self._pb_idx, len(rec.frames) - 1))]
            fb = rf.fb
        else:
            fb, _, _, _ = self.stage.current_display_state()

        if len(fb) < 1024:
            return

        st = self.runtime.lcd_st
        bg, fg, grid, _dark = _lcd_colors_norm(st)
        aspect = st.pixel_aspect or 1.30
        px_h = min((h - 32.0) / 64.0, ((w - 40.0) / 128.0) * aspect)
        px_w = px_h / aspect
        lcd_w = 128.0 * px_w
        lcd_h = 64.0 * px_h
        x0 = (w - lcd_w) * 0.5
        y0 = (h - lcd_h) * 0.5

        # Subtle bezel glow when backlight is on
        if st.backlight > 0 and st.backlight_glow:
            round_rect(cr, x0 - 9, y0 - 9, lcd_w + 18, lcd_h + 18, 10)
            cr.set_source_rgba(bg[0], bg[1], bg[2], 0.14 * min(1.0, st.backlight / 8.0))
            cr.fill()

        # LCD glass plate
        round_rect(cr, x0 - 5, y0 - 5, lcd_w + 10, lcd_h + 10, 6)
        cr.set_source_rgb(*bg)
        cr.fill()

        cr.set_antialias(cairo.ANTIALIAS_NONE)
        gap = 0.25 if (st.pixel_grid and px_w >= 2.2) else 0.0
        inv = st.invert_lcd
        cr.set_source_rgb(*fg)
        for page in range(8):
            base = page * 128
            for x in range(128):
                b = fb[base + x] ^ (0xFF if inv else 0x00)
                if not b:
                    continue
                px = x0 + x * px_w
                for bit in range(8):
                    if b & (1 << bit):
                        py = y0 + (page * 8 + bit) * px_h
                        cr.rectangle(px, py, max(1.0, px_w - gap), max(1.0, px_h - gap))
        cr.fill()

    def _draw_timeline(self, _area: Any, cr: Any, w: int, h: int) -> None:
        rec = self.runtime.recorder
        n = len(rec.frames)
        round_rect(cr, 0, 4, w, h - 8, 7)
        cr.set_source_rgb(0.09, 0.10, 0.12)
        cr.fill_preserve()
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.08)
        cr.set_line_width(1.0)
        cr.stroke()
        if n <= 0:
            draw_pango_text(cr, "Press Record to capture a timeline clip", w * 0.5, h * 0.5, font_desc="Adwaita Sans 9", rgba=(0.65, 0.68, 0.74, 0.55), halign="center", valign="center")
            return
        span = max(1, n - 1)
        fx = lambda i: 10.0 + (i / float(span)) * max(1.0, w - 20.0)
        x_in, x_out = fx(rec.trim_in), fx(rec.trim_out)
        round_rect(cr, x_in, 7, max(4.0, x_out - x_in), h - 14, 4)
        cr.set_source_rgba(0.22, 0.58, 0.98, 0.30)
        cr.fill_preserve()
        cr.set_source_rgba(0.36, 0.68, 1.0, 0.55)
        cr.set_line_width(1.0)
        cr.stroke()
        for a, b in rec.cuts:
            round_rect(cr, fx(a), 7, max(3.0, fx(b) - fx(a)), h - 14, 3)
            cr.set_source_rgba(0.94, 0.26, 0.26, 0.62)
            cr.fill()
        if self._cut_mark_start is not None:
            xm = fx(self._cut_mark_start)
            cr.set_source_rgb(0.98, 0.72, 0.18)
            cr.set_line_width(2.0)
            cr.move_to(xm, 4)
            cr.line_to(xm, h - 4)
            cr.stroke()
        cr.set_source_rgba(0.88, 0.92, 0.98, 0.50)
        for i, rf in enumerate(rec.frames):
            if rf.pressed:
                cr.rectangle(fx(i) - 1.0, h - 13, 2.0, 5)
        cr.fill()
        xp = fx(self._pb_idx)
        cr.set_source_rgb(1.0, 1.0, 1.0)
        cr.set_line_width(2.0)
        cr.move_to(xp, 3)
        cr.line_to(xp, h - 3)
        cr.stroke()
        round_rect(cr, xp - 4.0, 1.5, 8.0, 5.0, 2.5)
        cr.fill()

    def _seek_x(self, x: float) -> None:
        n = len(self.runtime.recorder.frames)
        if n > 1:
            w = max(20.0, float(self.timeline_area.get_width()) - 16.0)
            self._pb_idx = int(round(max(0.0, min(1.0, (x - 8.0) / w)) * (n - 1)))
            self._refresh_ui()

    def _on_tl_click(self, _g: Any, _n: int, x: float, _y: float) -> None:
        self._drag_start_x = x
        self._seek_x(x)

    def _on_tl_drag(self, _g: Any, ox: float, _oy: float) -> None:
        self._seek_x(self._drag_start_x + ox)

    def _jump_to(self, idx: int) -> None:
        n = len(self.runtime.recorder.frames)
        if n:
            self._pb_idx = max(0, min(n - 1, idx))
            self._refresh_ui()

    def _set_trim_in(self) -> None:
        rec = self.runtime.recorder
        if rec.frames:
            rec.trim_in = min(self._pb_idx, rec.trim_out)
            self._refresh_ui()

    def _set_trim_out(self) -> None:
        rec = self.runtime.recorder
        if rec.frames:
            rec.trim_out = max(self._pb_idx, rec.trim_in)
            self._refresh_ui()

    def _reset_trim(self) -> None:
        rec = self.runtime.recorder
        if rec.frames:
            rec.trim_in = 0
            rec.trim_out = len(rec.frames) - 1
            self._refresh_ui()

    def _cut_splice(self) -> None:
        rec = self.runtime.recorder
        if not rec.frames:
            return
        if self._cut_mark_start is None:
            self._cut_mark_start = self._pb_idx
            self.cut_btn.set_label("Splice")
        else:
            rec.add_cut(self._cut_mark_start, self._pb_idx)
            self._cut_mark_start = None
            self.cut_btn.set_label("Cut")
        self._refresh_ui()

    def _step_pb(self, delta: int) -> None:
        n = len(self.runtime.recorder.frames)
        if n:
            self._pb_idx = max(0, min(n - 1, self._pb_idx + delta))
            self._refresh_ui()

    def _toggle_pb(self) -> None:
        if self._pb_playing:
            self.stop_playback()
            return
        seq = self.runtime.recorder.frame_durations_ms(speed=self._export_speed, max_idle_ms=600 if self._cap_idle else None)
        if not seq:
            return
        self._pb_playing = True
        self.pb_btn.set_icon_name("media-playback-pause-symbolic")
        self._pb_seq_pos = 0
        self._schedule_pb(seq)

    def _schedule_pb(self, seq: list[tuple[int, int]]) -> None:
        if not self._pb_playing or not seq:
            return
        fi, dt_ms = seq[self._pb_seq_pos % len(seq)]
        self._pb_idx = fi
        self._refresh_ui()
        self._pb_seq_pos = (self._pb_seq_pos + 1) % len(seq)
        self._pb_timer = GLib.timeout_add(dt_ms, lambda: (self._schedule_pb(seq), False)[1])

    def _refresh_meta(self) -> None:
        rec = self.runtime.recorder
        n = len(rec.frames)
        target_s = "Full Handset" if self._export_mode == "body" else "128×64 ST7565 (540×374)"
        if n == 0:
            self.meta_left.set_text(f"Live Preview · {target_s}")
            self.meta_right.set_text("No clip")
        else:
            seq = rec.frame_durations_ms(speed=self._export_speed, max_idle_ms=600 if self._cap_idle else None)
            tot_ms = sum(d for _, d in seq)
            rf = rec.frames[max(0, min(self._pb_idx, n - 1))]
            keys_s = f" · Keys: {', '.join(sorted(rf.pressed))}" if rf.pressed else ""
            cuts_s = f" · {len(rec.cuts)} cut" if rec.cuts else ""
            self.meta_left.set_text(f"{target_s}{keys_s}")
            self.meta_right.set_text(f"Frame {self._pb_idx + 1}/{n} · {tot_ms / 1000.0:.2f}s{cuts_s}")

    def _refresh_ui(self) -> None:
        rec = self.runtime.recorder
        n = len(rec.frames)
        seq = rec.frame_durations_ms(speed=self._export_speed, max_idle_ms=600 if self._cap_idle else None)
        has_frames = n > 0
        for w in (
            self.clr_btn,
            self.start_btn,
            self.prev_btn,
            self.pb_btn,
            self.next_btn,
            self.end_btn,
            self.b_in,
            self.b_out,
            self.b_reset_trim,
            self.cut_btn,
            self.export_frame_btn,
        ):
            w.set_sensitive(has_frames)
        self.undo_btn.set_sensitive(bool(rec.cuts))
        self.export_btn.set_sensitive(bool(has_frames and len(seq) > 0))
        self._refresh_meta()
        self.preview_area.queue_draw()
        self.timeline_area.queue_draw()
        self.stage.ensure_anim_loop()

    def _discard(self) -> None:
        self.stop_playback()
        self.runtime.recorder.frames.clear()
        self.runtime.recorder.cuts.clear()
        self._cut_mark_start = None
        self.cut_btn.set_label("Cut")
        self._pb_idx = 0
        self._refresh_ui()

    def _export_selected_frame_png(self) -> None:
        rec = self.runtime.recorder
        if not rec.frames:
            return
        rf = rec.frames[max(0, min(self._pb_idx, len(rec.frames) - 1))]
        dlg = Gtk.FileDialog(title="Export Selected Frame (PNG)")
        dlg.set_initial_name(f"kern_frame_{self._pb_idx + 1}_{self._export_mode}.png")

        def _done(d: Any, res: Any) -> None:
            try:
                fpath = d.save_finish(res).get_path()
                if fpath:
                    if self._export_mode == "body":
                        w, h, rgba = render_handset_to_rgba(self.runtime.active_model, rf.fb, self.runtime.lcd_st, pressed_keys=rf.pressed, led_state=rf.led, torch_on=rf.torch, scale=2.0)
                    else:
                        w, h, rgba = render_lcd_to_rgba(rf.fb, self.runtime.lcd_st, width=540, height=374)
                    Path(fpath).write_bytes(encode_png_rgba(w, h, rgba))
                    self._on_toast(f"Saved {Path(fpath).name}")
            except GLib.Error:
                pass

        dlg.save(self.win, None, _done)

    def _export_gif(self) -> None:
        dlg = Gtk.FileDialog(title="Export Animated GIF")
        dlg.set_initial_name(f"kern_{self.runtime.active_model}_{self._export_mode}_{time.strftime('%Y%m%d_%H%M%S')}.gif")

        def _picked(d: Any, res: Any) -> None:
            try:
                dest = d.save_finish(res).get_path()
            except GLib.Error:
                return
            if not dest:
                return
            self.gif_progress.set_visible(True)
            self.gif_progress.set_fraction(0.0)
            self.export_btn.set_sensitive(False)

            def _w() -> None:
                try:
                    sz = self.runtime.recorder.export_gif(
                        dest,
                        model=self.runtime.active_model,
                        lcd_st=dataclasses.replace(self.runtime.lcd_st),
                        mode=self._export_mode,
                        speed=self._export_speed,
                        max_idle_ms=600 if self._cap_idle else None,
                        progress_cb=lambda f: GLib.idle_add(self.gif_progress.set_fraction, f),
                    )
                    GLib.idle_add(self._on_toast, f"Saved {Path(dest).name} ({sz / 1024.0:.1f} KB)")
                except Exception as exc:
                    GLib.idle_add(self._on_toast, f"GIF export failed: {exc}")
                finally:
                    GLib.idle_add(self.gif_progress.set_visible, False)
                    GLib.idle_add(self.export_btn.set_sensitive, True)

            threading.Thread(target=_w, daemon=True).start()

        dlg.save(self.win, None, _picked)

    def save_raw_framebuffer(self) -> None:
        fb, _, _, _ = self.stage.current_display_state()
        dlg = Gtk.FileDialog(title="Save Raw 1 KB Framebuffer (.bin)")
        dlg.set_initial_name(f"kern_fb_{time.strftime('%Y%m%d_%H%M%S')}.bin")

        def _done(d: Any, res: Any) -> None:
            try:
                fpath = d.save_finish(res).get_path()
                if fpath:
                    Path(fpath).write_bytes(bytes(fb[:1024]))
                    self._on_toast(f"Saved {Path(fpath).name} (1024 B)")
            except GLib.Error:
                pass

        dlg.save(self.win, None, _done)

    def copy_screenshot(self) -> None:
        try:
            fb, pr, led, torch = self.stage.current_display_state()
            if self._export_mode == "body":
                w, h, rgba = render_handset_to_rgba(self.runtime.active_model, fb, self.runtime.lcd_st, pressed_keys=pr, led_state=led, torch_on=torch, scale=2.0)
            else:
                w, h, rgba = render_lcd_to_rgba(fb, self.runtime.lcd_st, width=540, height=374)
            png = encode_png_rgba(w, h, rgba)
            provider = Gdk.ContentProvider.new_for_bytes("image/png", GLib.Bytes.new(png))
            Gdk.Display.get_default().get_clipboard().set_content(provider)
            self._on_toast("Copied PNG snapshot to clipboard")
        except Exception as exc:
            self._on_toast(f"Clipboard copy failed: {exc}")

    def save_screenshot(self) -> None:
        dlg = Gtk.FileDialog(title="Save Snapshot (PNG)")
        dlg.set_initial_name(f"kern_{self.runtime.active_model}_{self._export_mode}_{time.strftime('%Y%m%d_%H%M%S')}.png")

        def _done(d: Any, res: Any) -> None:
            try:
                fpath = d.save_finish(res).get_path()
                if fpath:
                    fb, pr, led, torch = self.stage.current_display_state()
                    if self._export_mode == "body":
                        w, h, rgba = render_handset_to_rgba(self.runtime.active_model, fb, self.runtime.lcd_st, pressed_keys=pr, led_state=led, torch_on=torch, scale=2.0)
                    else:
                        w, h, rgba = render_lcd_to_rgba(fb, self.runtime.lcd_st, width=540, height=374)
                    Path(fpath).write_bytes(encode_png_rgba(w, h, rgba))
                    self._on_toast(f"Saved {Path(fpath).name}")
            except GLib.Error:
                pass

        dlg.save(self.win, None, _done)
