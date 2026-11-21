"""HandsetStage: full-scale interactive 3D radio dock pane with compact hardware icon strip & DnD."""

from __future__ import annotations

import dataclasses
import math
from pathlib import Path
from typing import Any, Callable, Mapping

import gi

gi.require_version("Gtk", "4.0")
gi.require_version("Gdk", "4.0")
gi.require_version("Adw", "1")
from gi.repository import Gdk, Gio, GLib, Gtk  # noqa: E402

from ...device.keys import KEY_BINDINGS, keyval_label
from ...render.handset.catalog import compute_handset_geometry
from ...render.handset.parts import draw_pango_text
from ...render.handset.renderer import HandsetRenderer
from ...render.paint import round_rect
from ..runtime import DockRuntime


def hit_test_handset(
    hitboxes: Mapping[str, tuple[float, float, float, float]],
    lx: float,
    ly: float,
) -> str | None:
    """
    Forgiving nearest-button hit-tester in logical handset coordinates:
      1. Exact containment always wins (smallest-area hitbox breaks ties).
      2. Near misses snap to the closest button edge within a size-adaptive
         reach radius (larger halo around small buttons/snap domes/side keys,
         tighter around large keys) without grabbing across empty chassis gaps.
    """
    best_exact: tuple[float, str] | None = None
    best_near: tuple[float, float, str] | None = None

    for kn, (rx, ry, rw, rh) in hitboxes.items():
        if rx <= lx <= rx + rw and ry <= ly <= ry + rh:
            area = rw * rh
            if best_exact is None or area < best_exact[0]:
                best_exact = (area, kn)
            continue

        # Euclidean distance from pointer to clamped rectangle boundary
        dx = rx - lx if lx < rx else (lx - (rx + rw) if lx > rx + rw else 0.0)
        dy = ry - ly if ly < ry else (ly - (ry + rh) if ly > ry + rh else 0.0)
        edge_dist = math.hypot(dx, dy)

        min_dim = min(rw, rh)
        if kn == "_torch":
            reach = 10.0
        elif min_dim <= 28.0:
            reach = 13.0
        elif min_dim <= 38.0:
            reach = 10.5
        else:
            reach = 7.0

        if edge_dist <= reach:
            cx, cy = rx + rw * 0.5, ry + rh * 0.5
            center_dist = math.hypot(lx - cx, ly - cy)
            cand = (edge_dist, center_dist, kn)
            if best_near is None or cand < best_near:
                best_near = cand

    if best_exact is not None:
        return best_exact[1]
    return best_near[2] if best_near is not None else None


class HandsetStage(Gtk.Box):
    """Self-contained interactive 3D handset stage with 60 FPS animations, quick icon strip and DnD."""

    def __init__(
        self,
        runtime: DockRuntime,
        *,
        on_toast: Callable[[str], None],
        on_drop_file: Callable[[Path], None],
        on_toggle_keyboard: Callable[[], None],
        on_record: Callable[[], None],
        on_snapshot: Callable[[], None],
    ) -> None:
        super().__init__(orientation=Gtk.Orientation.VERTICAL, spacing=6)
        self.runtime = runtime
        self._on_toast = on_toast
        self._on_drop_file = on_drop_file
        self._on_toggle_keyboard = on_toggle_keyboard
        self._on_record = on_record
        self._on_snapshot = on_snapshot

        self.add_css_class("handset-stage")
        self.set_hexpand(False)
        self.set_vexpand(True)

        self.renderer = HandsetRenderer()
        self.pressed_keys: set[str] = set()
        self.long_pressing: set[str] = set()
        self._hovered_key: str | None = None
        self._pointer_active_key: str | None = None
        self._press_anim: dict[str, float] = {}
        self._hover_anim: dict[str, float] = {}
        self._anim_timer: int | None = None
        self._hold_timers: dict[str, int] = {}
        self._frame_pending = False
        self.alt_hints = False
        self.playback_override: Callable[[], tuple[bytes, frozenset[str], str, bool] | None] | None = None
        self.on_rec_event: Callable[[], None] | None = None

        # --- Full-Scale Interactive 3D Handset DrawingArea ---
        self.area = Gtk.DrawingArea(halign=Gtk.Align.CENTER, valign=Gtk.Align.CENTER, focusable=True, vexpand=True)
        self.area.update_property([Gtk.AccessibleProperty.LABEL], ["Interactive radio handset"])
        self.area.set_margin_top(6)
        self.area.set_margin_start(10)
        self.area.set_margin_end(10)
        self.area.set_draw_func(self._draw)

        g_l = Gtk.GestureClick(button=1)
        g_l.set_exclusive(True)
        g_l.connect("pressed", self._on_press)
        g_l.connect("released", lambda *_a: self.release_pointer_keys())
        g_l.connect("unpaired-release", lambda *_a: self.release_pointer_keys())
        g_l.connect("cancel", lambda *_a: self.release_pointer_keys())
        self.area.add_controller(g_l)

        g_r = Gtk.GestureClick(button=3)
        g_r.connect("pressed", self._on_right_click)
        self.area.add_controller(g_r)

        g_m = Gtk.EventControllerMotion()
        g_m.connect("motion", lambda _c, x, y: self._set_hovered(self._hit_test(x, y)))
        g_m.connect("leave", lambda _c: self._set_hovered(None))
        self.area.add_controller(g_m)

        # Drag-and-drop (.kfw, .bin, .krx, or KernFS files right onto the radio)
        drop = Gtk.DropTarget.new(Gio.File, Gdk.DragAction.COPY)
        drop.connect("enter", lambda *_a: (self.add_css_class("drop-active"), Gdk.DragAction.COPY)[1])
        drop.connect("leave", lambda *_a: self.remove_css_class("drop-active"))
        drop.connect("drop", self._on_drop)
        self.add_controller(drop)

        self.append(self.area)

        # --- Compact Bottom Hardware Action Strip ---
        strip = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=3, halign=Gtk.Align.CENTER)
        strip.add_css_class("stage-toolbar")
        self._quick_btns: dict[str, Gtk.Button] = {}
        for bid, icon, tip, cb in (
            ("torch", "kern-flashlight-symbolic", "Flashlight (F3)", lambda: self._run_quick("torch")),
            ("lock", "system-lock-screen-symbolic", "Keypad Lock (F4)", lambda: self._run_quick("lock")),
            ("headless", "video-display-symbolic", "Headless Mode (F6 · Hold MENU+EXIT 3s on radio to exit)", lambda: self._run_quick("headless")),
            ("record", "media-record-symbolic", "Record GIF (Ctrl+R)", self._on_record),
            ("shot", "camera-photo-symbolic", "Save Snapshot (Ctrl+S)", self._on_snapshot),
        ):
            b = Gtk.Button(icon_name=icon, tooltip_text=tip)
            b.add_css_class("flat")
            b.add_css_class("circular")
            b.update_property([Gtk.AccessibleProperty.LABEL], [tip])
            b.connect("clicked", lambda _b, f=cb: f())
            strip.append(b)
            self._quick_btns[bid] = b
        self.append(strip)

        # --- Compact Bottom Status Row: Keyboard Bridge Chip + Discord-Style Mic & Headphone Toggles ---
        bottom_row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=6, halign=Gtk.Align.CENTER, margin_bottom=8)

        self.kbd_chip = Gtk.Button()
        self.kbd_chip.add_css_class("kbd-chip")
        chip_in = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=5)
        self.kbd_chip_icon = Gtk.Image(icon_name="input-keyboard-symbolic")
        self.kbd_chip_lbl = Gtk.Label(label="Keyboard → Radio")
        chip_in.append(self.kbd_chip_icon)
        chip_in.append(self.kbd_chip_lbl)
        self.kbd_chip.set_child(chip_in)
        self.kbd_chip.connect("clicked", lambda _b: self._on_toggle_keyboard())
        bottom_row.append(self.kbd_chip)

        # Discord-style Mic & Headphone controls (Headless RigWire audio + Kenwood sound card)
        audio_grp = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=2)
        audio_grp.add_css_class("stage-toolbar")

        self.mic_btn = Gtk.Button(icon_name="microphone-sensitivity-muted-symbolic", tooltip_text="Microphone Mute / Unmute (Kenwood TX)")
        self.mic_btn.add_css_class("flat")
        self.mic_btn.add_css_class("circular")
        self.mic_btn.update_property([Gtk.AccessibleProperty.LABEL], ["Toggle Microphone"])
        self.mic_btn.connect("clicked", lambda _b: self._toggle_mic())
        audio_grp.append(self.mic_btn)

        self.hp_btn = Gtk.Button(icon_name="audio-headphones-symbolic", tooltip_text="Headphones / RigWire Audio Monitor")
        self.hp_btn.add_css_class("flat")
        self.hp_btn.add_css_class("circular")
        self.hp_btn.update_property([Gtk.AccessibleProperty.LABEL], ["Toggle Headphones"])
        self.hp_btn.connect("clicked", lambda _b: self._toggle_headphones())
        audio_grp.append(self.hp_btn)

        bottom_row.append(audio_grp)
        self.append(bottom_row)

        # Subscribe to runtime events
        runtime.events.subscribe("link.frame", lambda _e: self.schedule_frame(), main_thread=False)
        runtime.events.subscribe("link.connected", lambda _e: self.refresh_state(), main_thread=True)
        runtime.events.subscribe("link.disconnected", lambda _e: self.refresh_state(), main_thread=True)
        runtime.events.subscribe("link.telemetry", lambda _e: self.refresh_state(), main_thread=True)
        runtime.events.subscribe("audio.changed", lambda _e: self._refresh_audio_btns(), main_thread=True)
        runtime.events.subscribe("security.changed", lambda _e: self.refresh_state(), main_thread=True)
        runtime.events.subscribe("handset.changed", lambda _e: self._on_model_changed(), main_thread=True)
        runtime.events.subscribe("lcd.changed", lambda _e: self.queue_redraw(), main_thread=True)

        self.sync_geometry()
        self.refresh_state()
        self._refresh_audio_btns()

    def _on_drop(self, _target: Any, value: Any, _x: float, _y: float) -> bool:
        self.remove_css_class("drop-active")
        if isinstance(value, Gio.File):
            path_str = value.get_path()
            if path_str:
                self._on_drop_file(Path(path_str))
                return True
        return False

    def _run_quick(self, action: str) -> None:
        lk = self.runtime.link
        if not lk.connected or lk.link_mode != "rigwire":
            self._on_toast("Connect a RigWire radio first")
            return
        if action == "torch":
            lk.set_flashlight(not lk.torch_on)
        elif action == "lock":
            lk.set_keypad_lock(not lk.keypad_locked)
            self._on_toast("Keypad locked" if lk.keypad_locked else "Keypad unlocked")
        elif action == "vfo":
            lk.toggle_vfo()
        elif action == "headless":
            lk.set_headless(not lk.headless)
            self._on_toast(
                "Headless mode on · Hold MENU+EXIT 3s on radio to exit"
                if lk.headless
                else "Headless mode off"
            )
        if self.on_rec_event:
            self.on_rec_event()
        self.refresh_state()
        self.area.queue_draw()

    def sync_geometry(self) -> tuple[int, int]:
        lw, lh = compute_handset_geometry(self.runtime.active_model)
        self.area.set_content_width(lw)
        self.area.set_content_height(lh)
        return lw, lh

    def _on_model_changed(self) -> None:
        self.sync_geometry()
        self.queue_redraw()

    def queue_redraw(self) -> None:
        fb, _pr, _led, _torch = self.current_display_state()
        self.runtime.lcd_state.step(fb, self.runtime.lcd_st)
        self.area.queue_draw()

    def refresh_state(self) -> None:
        lk = self.runtime.link
        online = lk.connected and lk.link_mode == "rigwire"
        for key, active in (
            ("torch", lk.torch_on),
            ("lock", lk.keypad_locked),
            ("headless", lk.headless),
        ):
            btn = self._quick_btns[key]
            btn.set_sensitive(online)
            pressed = bool(active and online)
            btn.update_state([Gtk.AccessibleState.PRESSED], [1 if pressed else 0])
            if pressed:
                btn.add_css_class("active-toggle")
            else:
                btn.remove_css_class("active-toggle")
        self.area.queue_draw()

    def set_recording(self, recording: bool) -> None:
        btn = self._quick_btns.get("record")
        if btn is None:
            return
        btn.update_state([Gtk.AccessibleState.PRESSED], [1 if recording else 0])
        if recording:
            btn.add_css_class("rec-live")
        else:
            btn.remove_css_class("rec-live")

    def _toggle_mic(self) -> None:
        aud = self.runtime.audio
        if not aud.mic_enabled and aud.kenwood_tx_device is None:
            self._on_toast("Pair a Kenwood TX sound card in Preferences → Audio to route mic")
        aud.set_mic(not aud.mic_enabled)

    def _toggle_headphones(self) -> None:
        aud = self.runtime.audio
        aud.set_headphones(not aud.headphones_enabled)

    def _refresh_audio_btns(self) -> None:
        aud = self.runtime.audio
        # Mic toggle (Discord-style: red muted when off, accent/live when unmuted)
        self.mic_btn.set_icon_name("audio-input-microphone-symbolic" if aud.mic_enabled else "microphone-sensitivity-muted-symbolic")
        self.mic_btn.update_state([Gtk.AccessibleState.PRESSED], [1 if aud.mic_enabled else 0])
        if aud.mic_enabled:
            self.mic_btn.remove_css_class("audio-muted")
            self.mic_btn.add_css_class("active-toggle")
        else:
            self.mic_btn.remove_css_class("active-toggle")
            self.mic_btn.add_css_class("audio-muted")

        # Headphone toggle (Discord-style: red deafened when off, normal when listening)
        self.hp_btn.set_icon_name("audio-headphones-symbolic" if aud.headphones_enabled else "audio-volume-muted-symbolic")
        self.hp_btn.update_state([Gtk.AccessibleState.PRESSED], [1 if aud.headphones_enabled else 0])
        if aud.headphones_enabled:
            self.hp_btn.remove_css_class("audio-muted")
        else:
            self.hp_btn.add_css_class("audio-muted")

    def update_keyboard_chip(self, *, enabled: bool, live_typing: bool) -> None:
        for cls in ("on", "off", "typing"):
            self.kbd_chip.remove_css_class(cls)
        if live_typing:
            state, text = "typing", "Live typing · Esc to stop"
        elif enabled:
            state, text = "on", "Keyboard → Radio"
        else:
            state, text = "off", "Keyboard off"
        self.kbd_chip.add_css_class(state)
        self.kbd_chip_icon.set_from_icon_name("input-keyboard-symbolic")
        self.kbd_chip_lbl.set_text(text)
        self.kbd_chip.set_tooltip_text(
            "Click to stop live typing"
            if live_typing
            else "Click to toggle keyboard & scroll-wheel rotary control (Ctrl+K) · hold Alt for key hints"
        )

    def current_display_state(self) -> tuple[bytes, frozenset[str], str, bool]:
        if self.playback_override is not None:
            ov = self.playback_override()
            if ov is not None:
                return ov
        lk = self.runtime.link
        if not lk.connected:
            return bytes(1024), frozenset(self.pressed_keys), "off", False
        if self.runtime.flashing_active or lk.link_mode == "stock_bl":
            return bytes(1024), frozenset(self.pressed_keys), "off", lk.torch_on
        if lk.demo_mode:
            led = "tx" if "ptt" in self.pressed_keys else ("rx" if "side2" in self.pressed_keys else "off")
        else:
            led = lk.status_led
        return lk.last_fb, frozenset(self.pressed_keys), led, lk.torch_on

    def ensure_anim_loop(self) -> None:
        if self._anim_timer is None:
            self._anim_timer = GLib.timeout_add(16, self._tick_animations)

    def _tick_animations(self) -> bool:
        active = False
        fb, pressed, _led, _torch = self.current_display_state()
        if self.runtime.lcd_state.step(fb, self.runtime.lcd_st):
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

        self.area.queue_draw()
        if not active:
            self._anim_timer = None
            return False
        return True

    def schedule_frame(self) -> None:
        if not self._frame_pending:
            self._frame_pending = True
            GLib.idle_add(self._on_new_frame_main)

    def _on_new_frame_main(self) -> bool:
        self._frame_pending = False
        if self.on_rec_event:
            self.on_rec_event()
        fb, _pr, _led, _torch = self.current_display_state()
        settling = self.runtime.lcd_state.step(fb, self.runtime.lcd_st)
        self.area.queue_draw()
        if settling:
            self.ensure_anim_loop()
        return False

    def _draw(self, _area: Any, cr: Any, w: int, h: int) -> None:
        lw, lh = compute_handset_geometry(self.runtime.active_model)
        scale = min(w / float(lw), h / float(lh))
        ox, oy = (w - lw * scale) * 0.5, (h - lh * scale) * 0.5
        cr.save()
        cr.translate(ox, oy)
        cr.scale(scale, scale)
        _fb, _pr, led, torch = self.current_display_state()
        lk = self.runtime.link
        lcd_off = (not lk.connected) or self.runtime.flashing_active or (lk.link_mode == "stock_bl")
        eff_st = dataclasses.replace(self.runtime.lcd_st, backlight=0) if lcd_off else self.runtime.lcd_st
        self.renderer.paint(
            cr,
            self.runtime.active_model,
            self.runtime.lcd_state,
            eff_st,
            press_anim=self._press_anim,
            hover_anim=self._hover_anim,
            led_state=led,
            torch_on=torch,
        )
        if self.alt_hints:
            self._draw_key_hints(cr)
        cr.restore()

    def _draw_key_hints(self, cr: Any) -> None:
        hints = {action: keyval_label(names[0]) for action, _d, names in KEY_BINDINGS}
        hints["_torch"] = hints["torch"]
        for kn, (rx, ry, rw, rh) in self.renderer.hitboxes.items():
            label = hints.get(kn)
            if not label or label in (kn, "*"):
                continue
            bw = max(18.0, 7.0 * len(label) + 10.0)
            bx, by = rx + rw * 0.5 - bw * 0.5, ry + rh * 0.5 - 9.0
            round_rect(cr, bx, by, bw, 18.0, 5.0)
            cr.set_source_rgba(0.05, 0.07, 0.10, 0.90)
            cr.fill_preserve()
            cr.set_source_rgba(0.22, 0.74, 0.97, 0.95)
            cr.set_line_width(1.2)
            cr.stroke()
            draw_pango_text(cr, label, bx + bw * 0.5, by + 9.0, font_desc="Sans Bold 8", rgba=(0.95, 0.97, 1.0, 1.0), halign="center")

    def _hit_test(self, x: float, y: float) -> str | None:
        lw, lh = compute_handset_geometry(self.runtime.active_model)
        w, h = self.area.get_width(), self.area.get_height()
        if w <= 0 or h <= 0:
            return None
        scale = min(w / float(lw), h / float(lh))
        lx = (x - (w - lw * scale) * 0.5) / scale
        ly = (y - (h - lh * scale) * 0.5) / scale
        return hit_test_handset(self.renderer.hitboxes, lx, ly)

    def _set_hovered(self, kn: str | None) -> None:
        if kn != self._hovered_key:
            self._hovered_key = kn
            self.ensure_anim_loop()

    def _on_press(self, _g: Any, _n: int, x: float, y: float) -> None:
        kn = self._hit_test(x, y)
        if kn == "_torch":
            self._run_quick("torch")
            return
        if kn:
            self._pointer_active_key = kn
            self.press_key(kn)

    def _on_right_click(self, _g: Any, _n: int, x: float, y: float) -> None:
        kn = self._hit_test(x, y)
        if kn and kn != "_torch":
            self.long_click_key(kn)

    def release_pointer_keys(self) -> None:
        kn = self._pointer_active_key
        if kn:
            self._pointer_active_key = None
            self.release_key(kn)

    def release_all_keys(self) -> None:
        self._pointer_active_key = None
        for kn in list(self.pressed_keys):
            self.release_key(kn)

    def press_key(self, kname: str) -> None:
        if kname in self.pressed_keys:
            return
        lk = self.runtime.link
        if kname == "ptt" and lk.connected:
            try:
                self.runtime.security.check_tx(demo_mode=lk.demo_mode)
            except PermissionError as exc:
                self._on_toast(str(exc))
                return
        self.pressed_keys.add(kname)
        if self.on_rec_event:
            self.on_rec_event()
        self.ensure_anim_loop()
        if not lk.connected or lk.link_mode != "rigwire":
            return
        try:
            lk.send_key(kname, "press")
        except PermissionError as exc:
            self.pressed_keys.discard(kname)
            self._on_toast(str(exc))
            return
        if kname in self._hold_timers:
            GLib.source_remove(self._hold_timers.pop(kname))

        def _hold(k: str = kname) -> bool:
            self._hold_timers.pop(k, None)
            if k in self.pressed_keys and lk.connected:
                try:
                    lk.send_key(k, "hold")
                except PermissionError:
                    pass
            return False

        self._hold_timers[kname] = GLib.timeout_add(450, _hold)

    def release_key(self, kname: str) -> None:
        was = kname in self.pressed_keys
        self.pressed_keys.discard(kname)
        if kname in self._hold_timers:
            GLib.source_remove(self._hold_timers.pop(kname))
        if self.on_rec_event:
            self.on_rec_event()
        self.ensure_anim_loop()
        lk = self.runtime.link
        if was and lk.connected and lk.link_mode == "rigwire":
            lk.send_key(kname, "release")

    def long_click_key(self, kname: str) -> None:
        lk = self.runtime.link
        if not lk.connected or lk.link_mode != "rigwire":
            return
        if kname == "ptt":
            try:
                self.runtime.security.check_tx(demo_mode=lk.demo_mode)
            except PermissionError as exc:
                self._on_toast(str(exc))
                return
        self.pressed_keys.add(kname)
        self.long_pressing.add(kname)
        if self.on_rec_event:
            self.on_rec_event()
        self.ensure_anim_loop()
        lk.send_key(kname, "press")
        GLib.timeout_add(120, lambda: (lk.send_key(kname, "hold"), False)[1] if lk.connected else False)

        def _end() -> bool:
            self.long_pressing.discard(kname)
            self.pressed_keys.discard(kname)
            if self.on_rec_event:
                self.on_rec_event()
            self.ensure_anim_loop()
            if lk.connected:
                lk.send_key(kname, "release")
            return False

        GLib.timeout_add(260, _end)
