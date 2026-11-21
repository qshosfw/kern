"""Capture Studio View: PNG/clipboard snapshots and GIF recorder with trim/cut/splice timeline."""

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
from ...render.paint import round_rect
from ..runtime import DockRuntime
from ..widgets.handset_stage import HandsetStage


class CaptureView:
    """Snapshot and non-destructive GIF studio view."""

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

    def _build(self) -> Adw.PreferencesPage:
        page = Adw.PreferencesPage()

        # --- Quick Actions Group ---
        act_grp = Adw.PreferencesGroup()
        top_row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        self.rec_btn = Gtk.ToggleButton(hexpand=True, tooltip_text="Start / stop live recording (Ctrl+R)")
        rec_in = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=6, halign=Gtk.Align.CENTER)
        rec_in.append(Gtk.Image.new_from_icon_name("media-record-symbolic"))
        self.rec_lbl = Gtk.Label(label="Record GIF")
        rec_in.append(self.rec_lbl)
        self.rec_btn.set_child(rec_in)
        self.rec_btn.connect("toggled", self._on_record_toggled)
        top_row.append(self.rec_btn)

        shot_grp = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL)
        shot_grp.add_css_class("linked")
        shot_btn = Gtk.Button(tooltip_text="Save PNG snapshot (Ctrl+S)")
        sh_in = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=6)
        sh_in.append(Gtk.Image.new_from_icon_name("camera-photo-symbolic"))
        sh_in.append(Gtk.Label(label="Snapshot"))
        shot_btn.set_child(sh_in)
        shot_btn.connect("clicked", lambda _b: self.save_screenshot())
        shot_grp.append(shot_btn)

        copy_png = Gtk.Button.new_from_icon_name("edit-copy-symbolic")
        copy_png.set_tooltip_text("Copy PNG to clipboard")
        copy_png.connect("clicked", lambda _b: self.copy_screenshot())
        shot_grp.append(copy_png)
        top_row.append(shot_grp)
        act_grp.add(top_row)
        page.add(act_grp)

        # --- Timeline Group ---
        tl_grp = Adw.PreferencesGroup()
        tl_card = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=10)
        tl_card.add_css_class("card")
        tl_card.add_css_class("timeline-card")

        tl_hdr = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=6)
        tl_title = Gtk.Label(label="Timeline", xalign=0)
        tl_title.add_css_class("heading")
        tl_hdr.append(tl_title)
        self.summary_lbl = Gtk.Label(label="No frames recorded", xalign=1, hexpand=True, ellipsize=3)
        self.summary_lbl.add_css_class("caption")
        self.summary_lbl.add_css_class("dim-label")
        self.summary_lbl.add_css_class("numeric")
        tl_hdr.append(self.summary_lbl)
        self.clr_btn = Gtk.Button.new_from_icon_name("user-trash-symbolic")
        self.clr_btn.add_css_class("flat")
        self.clr_btn.set_tooltip_text("Discard clip")
        self.clr_btn.update_property([Gtk.AccessibleProperty.LABEL], ["Discard clip"])
        self.clr_btn.set_sensitive(False)
        self.clr_btn.connect("clicked", lambda _b: self._discard())
        tl_hdr.append(self.clr_btn)
        tl_card.append(tl_hdr)

        self.timeline_area = Gtk.DrawingArea(content_height=48, hexpand=True)
        self.timeline_area.set_draw_func(self._draw_timeline)
        tl_c = Gtk.GestureClick()
        tl_c.connect("pressed", self._on_tl_click)
        self.timeline_area.add_controller(tl_c)
        tl_d = Gtk.GestureDrag()
        tl_d.connect("drag-update", self._on_tl_drag)
        self.timeline_area.add_controller(tl_d)
        tl_card.append(self.timeline_area)

        tr = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        nav_g = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL)
        nav_g.add_css_class("linked")
        self.prev_btn = Gtk.Button.new_from_icon_name("go-previous-symbolic")
        self.prev_btn.set_tooltip_text("Previous frame")
        self.prev_btn.update_property([Gtk.AccessibleProperty.LABEL], ["Previous frame"])
        self.prev_btn.connect("clicked", lambda _b: self._step_pb(-1))
        self.pb_btn = Gtk.Button.new_from_icon_name("media-playback-start-symbolic")
        self.pb_btn.set_tooltip_text("Play / Pause")
        self.pb_btn.update_property([Gtk.AccessibleProperty.LABEL], ["Play or pause preview"])
        self.pb_btn.connect("clicked", lambda _b: self._toggle_pb())
        self.next_btn = Gtk.Button.new_from_icon_name("go-next-symbolic")
        self.next_btn.set_tooltip_text("Next frame")
        self.next_btn.update_property([Gtk.AccessibleProperty.LABEL], ["Next frame"])
        self.next_btn.connect("clicked", lambda _b: self._step_pb(1))
        for w in (self.prev_btn, self.pb_btn, self.next_btn):
            w.set_sensitive(False)
            nav_g.append(w)
        tr.append(nav_g)
        tr.append(Gtk.Box(hexpand=True))

        edit_g = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL)
        edit_g.add_css_class("linked")
        self.b_in = Gtk.Button(label="In", tooltip_text="Trim start", sensitive=False)
        self.b_in.connect("clicked", lambda _b: self._set_trim_in())
        self.b_out = Gtk.Button(label="Out", tooltip_text="Trim end", sensitive=False)
        self.b_out.connect("clicked", lambda _b: self._set_trim_out())
        self.cut_btn = Gtk.Button(label="Cut", tooltip_text="Mark splice cut", sensitive=False)
        self.cut_btn.connect("clicked", lambda _b: self._cut_splice())
        self.undo_btn = Gtk.Button.new_from_icon_name("edit-undo-symbolic")
        self.undo_btn.set_tooltip_text("Undo cut")
        self.undo_btn.update_property([Gtk.AccessibleProperty.LABEL], ["Undo cut"])
        self.undo_btn.set_sensitive(False)
        self.undo_btn.connect("clicked", lambda _b: (self.runtime.recorder.undo_cut(), self._refresh_ui()))
        for w in (self.b_in, self.b_out, self.cut_btn, self.undo_btn):
            edit_g.append(w)
        tr.append(edit_g)
        tl_card.append(tr)
        tl_grp.add(tl_card)
        page.add(tl_grp)

        # --- Export Options Group ---
        opt = Adw.PreferencesGroup(title="Export")
        r_mode = Adw.ActionRow(title="Frame", subtitle="LCD only or full handset")
        mg = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, valign=Gtk.Align.CENTER)
        mg.add_css_class("linked")
        b_lcd = Gtk.ToggleButton(label="LCD", active=True)
        b_body = Gtk.ToggleButton(label="Full Radio", group=b_lcd)
        b_lcd.connect("toggled", lambda b: setattr(self, "_export_mode", "lcd") if b.get_active() else None)
        b_body.connect("toggled", lambda b: setattr(self, "_export_mode", "body") if b.get_active() else None)
        mg.append(b_lcd)
        mg.append(b_body)
        r_mode.add_suffix(mg)
        opt.add(r_mode)

        r_spd = Adw.ActionRow(title="Speed")
        sg = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, valign=Gtk.Align.CENTER)
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
        r_spd.add_suffix(sg)
        opt.add(r_spd)

        r_cap = Adw.SwitchRow(title="Clamp Idle Pauses", subtitle="Cap pauses at 0.6 s", active=True)
        r_cap.connect("notify::active", lambda sw, _p: (setattr(self, "_cap_idle", sw.get_active()), self._refresh_ui()))
        opt.add(r_cap)

        self.gif_progress = Gtk.ProgressBar(show_text=True, visible=False, margin_top=6)
        opt.add(self.gif_progress)

        self.export_btn = Adw.ButtonRow(title="Export Animated GIF…", start_icon_name="document-save-symbolic", sensitive=False)
        self.export_btn.add_css_class("suggested-action")
        self.export_btn.connect("activated", lambda _r: self._export_gif())
        opt.add(self.export_btn)
        page.add(opt)
        return page

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
            self.rec_lbl.set_text(f"Stop ({dt // 60:02d}:{dt % 60:02d} · {len(rec.frames)}f)")
        else:
            self.rec_lbl.set_text("Record GIF")

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

    def _draw_timeline(self, _area: Any, cr: Any, w: int, h: int) -> None:
        rec = self.runtime.recorder
        n = len(rec.frames)
        round_rect(cr, 0, 6, w, h - 12, 8)
        cr.set_source_rgb(0.09, 0.10, 0.12)
        cr.fill_preserve()
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.08)
        cr.set_line_width(1.0)
        cr.stroke()
        if n <= 0:
            draw_pango_text(cr, "Press Record GIF to capture frames", w * 0.5, h * 0.5, font_desc="Adwaita Sans 9", rgba=(0.65, 0.68, 0.74, 0.55), halign="center", valign="center")
            return
        span = max(1, n - 1)
        fx = lambda i: 10.0 + (i / float(span)) * max(1.0, w - 20.0)
        x_in, x_out = fx(rec.trim_in), fx(rec.trim_out)
        round_rect(cr, x_in, 9, max(4.0, x_out - x_in), h - 18, 5)
        cr.set_source_rgba(0.22, 0.58, 0.98, 0.32)
        cr.fill_preserve()
        cr.set_source_rgba(0.36, 0.68, 1.0, 0.55)
        cr.set_line_width(1.0)
        cr.stroke()
        for a, b in rec.cuts:
            round_rect(cr, fx(a), 9, max(3.0, fx(b) - fx(a)), h - 18, 3)
            cr.set_source_rgba(0.94, 0.26, 0.26, 0.62)
            cr.fill()
        if self._cut_mark_start is not None:
            xm = fx(self._cut_mark_start)
            cr.set_source_rgb(0.98, 0.72, 0.18)
            cr.set_line_width(2.0)
            cr.move_to(xm, 6)
            cr.line_to(xm, h - 6)
            cr.stroke()
        cr.set_source_rgba(0.88, 0.92, 0.98, 0.50)
        for i, rf in enumerate(rec.frames):
            if rf.pressed:
                cr.rectangle(fx(i) - 1.0, h - 15, 2.0, 6)
        cr.fill()
        xp = fx(self._pb_idx)
        cr.set_source_rgb(1.0, 1.0, 1.0)
        cr.set_line_width(2.2)
        cr.move_to(xp, 5)
        cr.line_to(xp, h - 5)
        cr.stroke()
        round_rect(cr, xp - 4.0, 2.5, 8.0, 6.0, 3.0)
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

    def _refresh_ui(self) -> None:
        rec = self.runtime.recorder
        n = len(rec.frames)
        seq = rec.frame_durations_ms(speed=self._export_speed, max_idle_ms=600 if self._cap_idle else None)
        tot_ms = sum(d for _, d in seq)
        has_frames = n > 0
        for w in (self.clr_btn, self.prev_btn, self.pb_btn, self.next_btn, self.b_in, self.b_out, self.cut_btn):
            w.set_sensitive(has_frames)
        self.undo_btn.set_sensitive(bool(rec.cuts))
        if n == 0:
            self.summary_lbl.set_text("No frames recorded")
            self.export_btn.set_sensitive(False)
        else:
            cuts_s = f" · {len(rec.cuts)} cut" if rec.cuts else ""
            self.summary_lbl.set_text(f"{self._pb_idx + 1}/{n}f · {tot_ms / 1000.0:.2f}s{cuts_s}")
            self.export_btn.set_sensitive(len(seq) > 0)
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
