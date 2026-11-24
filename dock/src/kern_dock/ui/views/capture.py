"""Capture Studio: native GTK4/Libadwaita snapshot & GIF studio using the authentic ST7565 renderer."""

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
from gi.repository import Adw, Gdk, Gio, GLib, Gtk  # noqa: E402

from ...render.capture import encode_png_rgba, render_handset_to_rgba, render_lcd_to_rgba
from ...render.handset.catalog import compute_handset_geometry
from ...render.handset.renderer import HandsetRenderer
from ...render.lcd import FastLcdState, LcdLayerCache, paint_fast_lcd
from ...render.paint import round_rect
from ..runtime import DockRuntime
from ..widgets.handset_stage import HandsetStage


class CaptureView:
    """Native Libadwaita Capture Studio: clean toolbar, authentic ST7565 preview & revealable timeline."""

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

        self._lcd_cache = LcdLayerCache()
        self._scrub_lcd = FastLcdState()
        self._handset_renderer = HandsetRenderer()

        self.widget = self._build()
        runtime.events.subscribe("link.frame", lambda _e: self._on_live_frame(), main_thread=True)
        runtime.events.subscribe("lcd.changed", lambda _e: self.preview_area.queue_draw(), main_thread=True)

    def _build(self) -> Adw.ToolbarView:
        view = Adw.ToolbarView()

        # === TOP TOOLBAR (native Libadwaita .toolbar) ===
        top_bar = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=6)
        top_bar.add_css_class("toolbar")

        self.rec_btn = Gtk.ToggleButton(tooltip_text="Record clip (Ctrl+R)")
        rec_in = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=6)
        rec_in.append(Gtk.Image.new_from_icon_name("media-record-symbolic"))
        self.rec_lbl = Gtk.Label(label="Record")
        rec_in.append(self.rec_lbl)
        self.rec_btn.set_child(rec_in)
        self.rec_btn.connect("toggled", self._on_record_toggled)
        top_bar.append(self.rec_btn)

        # Native Adw.SplitButton for Snapshot (primary: Save PNG; menu: Copy PNG / Raw 1 KB FB)
        snap_btn = Adw.SplitButton(
            icon_name="camera-photo-symbolic",
            label="Snapshot",
            tooltip_text="Save PNG snapshot (Ctrl+S)",
        )
        snap_btn.connect("clicked", lambda _b: self.save_screenshot())
        snap_pop = Gtk.Popover()
        snap_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=2, margin_top=6, margin_bottom=6, margin_start=6, margin_end=6)
        for lbl, fn in (
            ("Copy PNG to Clipboard", self.copy_screenshot),
            ("Save Raw Framebuffer (.bin)", self.save_raw_framebuffer),
        ):
            mb = Gtk.Button(label=lbl)
            mb.add_css_class("flat")
            mb.set_halign(Gtk.Align.FILL)
            mb.connect("clicked", lambda _b, f=fn, p=snap_pop: (p.popdown(), f()))
            snap_box.append(mb)
        snap_pop.set_child(snap_box)
        snap_btn.set_popover(snap_pop)
        top_bar.append(snap_btn)

        top_bar.append(Gtk.Box(hexpand=True))

        mode_grp = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL)
        mode_grp.add_css_class("linked")
        b_lcd = Gtk.ToggleButton(label="LCD", active=True)
        b_body = Gtk.ToggleButton(label="Handset", group=b_lcd)
        b_lcd.connect("toggled", lambda b: self._set_mode("lcd") if b.get_active() else None)
        b_body.connect("toggled", lambda b: self._set_mode("body") if b.get_active() else None)
        mode_grp.append(b_lcd)
        mode_grp.append(b_body)
        top_bar.append(mode_grp)

        self.export_menu_btn = Gtk.MenuButton(label="Export", tooltip_text="Export GIF or Frame PNG")
        self.export_menu_btn.add_css_class("suggested-action")
        self.export_menu_btn.set_popover(self._build_export_popover())
        top_bar.append(self.export_menu_btn)

        top_wrap = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        top_wrap.append(top_bar)
        top_wrap.append(Gtk.Separator(orientation=Gtk.Orientation.HORIZONTAL))
        view.add_top_bar(top_wrap)

        # === CENTER STAGE: CLEAN, UNFRAMED NATIVE VIEWPORT ===
        stage_box = Gtk.Box(
            orientation=Gtk.Orientation.VERTICAL,
            spacing=10,
            vexpand=True,
            hexpand=True,
            margin_top=20,
            margin_bottom=16,
            margin_start=24,
            margin_end=24,
        )
        self.preview_area = Gtk.DrawingArea(hexpand=True, vexpand=True)
        self.preview_area.set_size_request(280, 180)
        self.preview_area.set_draw_func(self._draw_stage_preview)
        stage_box.append(self.preview_area)

        self.meta_lbl = Gtk.Label(label="Live · 128×64 ST7565", halign=Gtk.Align.CENTER, ellipsize=3)
        self.meta_lbl.add_css_class("caption")
        self.meta_lbl.add_css_class("dim-label")
        self.meta_lbl.add_css_class("numeric")
        stage_box.append(self.meta_lbl)

        self.gif_progress = Gtk.ProgressBar(show_text=True, visible=False)
        stage_box.append(self.gif_progress)
        view.set_content(stage_box)

        # === BOTTOM TIMELINE DOCK (Revealed only when a clip exists) ===
        self.timeline_revealer = Gtk.Revealer(
            transition_type=Gtk.RevealerTransitionType.SLIDE_UP,
            transition_duration=180,
            reveal_child=False,
        )
        bot_wrap = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        bot_wrap.append(Gtk.Separator(orientation=Gtk.Orientation.HORIZONTAL))

        dock = Gtk.Box(
            orientation=Gtk.Orientation.VERTICAL,
            spacing=6,
            margin_top=8,
            margin_bottom=6,
            margin_start=12,
            margin_end=12,
        )
        self.timeline_area = Gtk.DrawingArea(content_height=34, hexpand=True)
        self.timeline_area.set_draw_func(self._draw_timeline)
        tl_c = Gtk.GestureClick()
        tl_c.connect("pressed", self._on_tl_click)
        self.timeline_area.add_controller(tl_c)
        tl_d = Gtk.GestureDrag()
        tl_d.connect("drag-update", self._on_tl_drag)
        self.timeline_area.add_controller(tl_d)
        dock.append(self.timeline_area)

        tr = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=6)
        tr.add_css_class("toolbar")

        # Native flat media transport buttons
        self.prev_btn = Gtk.Button(icon_name="go-previous-symbolic", tooltip_text="Previous frame")
        self.prev_btn.add_css_class("flat")
        self.prev_btn.connect("clicked", lambda _b: self._step_pb(-1))
        self.pb_btn = Gtk.Button(icon_name="media-playback-start-symbolic", tooltip_text="Play / Pause")
        self.pb_btn.add_css_class("flat")
        self.pb_btn.connect("clicked", lambda _b: self._toggle_pb())
        self.next_btn = Gtk.Button(icon_name="go-next-symbolic", tooltip_text="Next frame")
        self.next_btn.add_css_class("flat")
        self.next_btn.connect("clicked", lambda _b: self._step_pb(1))
        for w in (self.prev_btn, self.pb_btn, self.next_btn):
            tr.append(w)

        tr.append(Gtk.Box(hexpand=True))

        edit_g = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL)
        edit_g.add_css_class("linked")
        self.b_in = Gtk.Button(label="In", tooltip_text="Trim start")
        self.b_in.connect("clicked", lambda _b: self._set_trim_in())
        self.b_out = Gtk.Button(label="Out", tooltip_text="Trim end")
        self.b_out.connect("clicked", lambda _b: self._set_trim_out())
        self.cut_btn = Gtk.Button(label="Cut", tooltip_text="Mark splice cut")
        self.cut_btn.connect("clicked", lambda _b: self._cut_splice())
        self.undo_btn = Gtk.Button(icon_name="edit-undo-symbolic", tooltip_text="Undo cut", sensitive=False)
        self.undo_btn.connect("clicked", lambda _b: (self.runtime.recorder.undo_cut(), self._refresh_ui()))
        for w in (self.b_in, self.b_out, self.cut_btn, self.undo_btn):
            edit_g.append(w)
        tr.append(edit_g)

        self.b_reset_trim = Gtk.Button(icon_name="view-refresh-symbolic", tooltip_text="Reset trim")
        self.b_reset_trim.add_css_class("flat")
        self.b_reset_trim.connect("clicked", lambda _b: self._reset_trim())
        tr.append(self.b_reset_trim)

        self.clr_btn = Gtk.Button(icon_name="user-trash-symbolic", tooltip_text="Discard clip")
        self.clr_btn.add_css_class("flat")
        self.clr_btn.connect("clicked", lambda _b: self._discard())
        tr.append(self.clr_btn)

        dock.append(tr)
        bot_wrap.append(dock)
        self.timeline_revealer.set_child(bot_wrap)
        view.add_bottom_bar(self.timeline_revealer)
        return view

    def _build_export_popover(self) -> Gtk.Popover:
        pop = Gtk.Popover()
        box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=10, margin_top=12, margin_bottom=12, margin_start=12, margin_end=12)
        box.set_size_request(240, -1)

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
            self.rec_lbl.set_text(f"Stop {dt // 60:02d}:{dt % 60:02d}")
        else:
            self.rec_lbl.set_text("Record")

    def _on_record_toggled(self, btn: Any) -> None:
        rec = self.runtime.recorder
        if btn.get_active():
            self.stop_playback()
            _fb, pr, led, torch = self.stage.current_display_state()
            rec.start(self.runtime.link.last_fb, pr, led, torch)
            btn.add_css_class("destructive-action")
            self._on_recording_changed(True)
            GLib.timeout_add(500, self._tick_rec_clock)
            self._update_rec_label()
        else:
            rec.stop()
            btn.remove_css_class("destructive-action")
            self._on_recording_changed(False)
            self._update_rec_label()
            self._pb_idx = rec.trim_in
            self._refresh_ui()

    def _tick_rec_clock(self) -> bool:
        if not self.runtime.recorder.recording:
            return False
        self._update_rec_label()
        return True

    def _draw_stage_preview(self, widget: Gtk.DrawingArea, cr: Any, w: int, h: int) -> None:
        st = self.runtime.lcd_st
        rec = self.runtime.recorder
        if rec.frames and not rec.recording:
            rf = rec.frames[max(0, min(self._pb_idx, len(rec.frames) - 1))]
            fb, pr, led, torch = rf.fb, rf.pressed, rf.led, rf.torch
        else:
            fb, pr, led, torch = self.stage.current_display_state()

        # Crisp, lag-free ST7565 frame for Capture preview
        st_clean = dataclasses.replace(st, ghosting=False)
        self._scrub_lcd.step(fb, st_clean)

        if self._export_mode == "body":
            model = self.runtime.active_model
            lw, lh = compute_handset_geometry(model)
            scale = min((w - 16.0) / max(1.0, lw), (h - 16.0) / max(1.0, lh))
            dw, dh = lw * scale, lh * scale
            cr.save()
            cr.translate((w - dw) * 0.5, (h - dh) * 0.5)
            cr.scale(scale, scale)
            self._handset_renderer.paint(
                cr,
                model,
                self._scrub_lcd,
                st_clean,
                press_anim={k: 1.0 for k in pr},
                hover_anim={},
                led_state=led,
                torch_on=torch,
            )
            cr.restore()
            return

        # Exact export LCD geometry (540×374 aspect = 1.4438) with authentic ST7565 optics
        aspect = 540.0 / 374.0
        max_w = max(64.0, w - 16.0)
        max_h = max(48.0, h - 16.0)
        lcd_w = min(max_w, max_h * aspect)
        lcd_h = lcd_w / aspect
        iw, ih = max(64, int(round(lcd_w))), max(48, int(round(lcd_h)))
        x0 = (w - iw) * 0.5
        y0 = (h - ih) * 0.5

        paint_fast_lcd(cr, x0, y0, iw, ih, self._scrub_lcd, st_clean, self._lcd_cache)
        rgba = widget.get_color()
        round_rect(cr, x0 - 0.5, y0 - 0.5, iw + 1.0, ih + 1.0, 4.5)
        cr.set_source_rgba(rgba.red, rgba.green, rgba.blue, 0.16)
        cr.set_line_width(1.0)
        cr.stroke()

    def _draw_timeline(self, widget: Gtk.DrawingArea, cr: Any, w: int, h: int) -> None:
        rec = self.runtime.recorder
        n = len(rec.frames)
        rgba = widget.get_color()
        # Native theme-aware trough
        round_rect(cr, 0, 6, w, h - 12, 6)
        cr.set_source_rgba(rgba.red, rgba.green, rgba.blue, 0.08)
        cr.fill()
        if n <= 0:
            return
        span = max(1, n - 1)
        fx = lambda i: 8.0 + (i / float(span)) * max(1.0, w - 16.0)
        x_in, x_out = fx(rec.trim_in), fx(rec.trim_out)
        # Active trim region
        round_rect(cr, x_in, 8, max(4.0, x_out - x_in), h - 16, 4)
        cr.set_source_rgba(0.21, 0.52, 0.89, 0.30)
        cr.fill_preserve()
        cr.set_source_rgba(0.21, 0.52, 0.89, 0.75)
        cr.set_line_width(1.0)
        cr.stroke()
        # Splice cuts
        for a, b in rec.cuts:
            round_rect(cr, fx(a), 8, max(3.0, fx(b) - fx(a)), h - 16, 3)
            cr.set_source_rgba(0.88, 0.24, 0.24, 0.60)
            cr.fill()
        if self._cut_mark_start is not None:
            xm = fx(self._cut_mark_start)
            cr.set_source_rgb(0.95, 0.68, 0.15)
            cr.set_line_width(2.0)
            cr.move_to(xm, 5)
            cr.line_to(xm, h - 5)
            cr.stroke()
        # Key-press ticks
        cr.set_source_rgba(rgba.red, rgba.green, rgba.blue, 0.45)
        for i, rf in enumerate(rec.frames):
            if rf.pressed:
                cr.rectangle(fx(i) - 0.75, h - 13, 1.5, 4)
        cr.fill()
        # Playhead
        xp = fx(self._pb_idx)
        cr.set_source_rgba(rgba.red, rgba.green, rgba.blue, 0.95)
        cr.set_line_width(2.0)
        cr.move_to(xp, 3)
        cr.line_to(xp, h - 3)
        cr.stroke()

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
        target_s = "Full Handset" if self._export_mode == "body" else "128×64 ST7565"
        if n == 0:
            self.meta_lbl.set_text(f"Live · {target_s}")
        else:
            seq = rec.frame_durations_ms(speed=self._export_speed, max_idle_ms=600 if self._cap_idle else None)
            tot_ms = sum(d for _, d in seq)
            rf = rec.frames[max(0, min(self._pb_idx, n - 1))]
            keys_s = f" · {', '.join(sorted(rf.pressed))}" if rf.pressed else ""
            cuts_s = f" · {len(rec.cuts)} cut" if rec.cuts else ""
            self.meta_lbl.set_text(f"Frame {self._pb_idx + 1} / {n} · {tot_ms / 1000.0:.2f} s{cuts_s}{keys_s}")

    def _refresh_ui(self) -> None:
        rec = self.runtime.recorder
        n = len(rec.frames)
        seq = rec.frame_durations_ms(speed=self._export_speed, max_idle_ms=600 if self._cap_idle else None)
        has_frames = n > 0
        self.timeline_revealer.set_reveal_child(has_frames)
        self.undo_btn.set_sensitive(bool(rec.cuts))
        self.export_frame_btn.set_sensitive(has_frames)
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
