"""Dedicated KeyboardBridge controller: PC keyboard -> radio keypad, Live Typing & Type Text dialog."""

from __future__ import annotations

from typing import Any, Callable

import gi

gi.require_version("Gtk", "4.0")
gi.require_version("Gdk", "4.0")
gi.require_version("Adw", "1")
from gi.repository import Adw, Gdk, GLib, Gtk  # noqa: E402

from ...device.keys import DOCK_ACTIONS, binding_index, resolve_binding
from ...device.t9 import T9Planner, T9_GAP_MS, T9_MAX_LEN
from ..runtime import DockRuntime
from .handset_stage import HandsetStage


class KeyboardBridge:
    """Clean standalone controller bridging the PC keyboard & T9 planner to the radio."""

    def __init__(
        self,
        win: Adw.ApplicationWindow,
        runtime: DockRuntime,
        stage: HandsetStage,
        *,
        on_toast: Callable[[str], None],
        on_state_change: Callable[[], None] | None = None,
    ) -> None:
        self.win = win
        self.runtime = runtime
        self.stage = stage
        self._on_toast = on_toast
        self._on_state_change = on_state_change

        self.enabled: bool = bool(runtime.config.get("ui.keyboard_bridge", True))
        self.live_typing: bool = False
        self._planner = T9Planner()
        self._key_index = binding_index()
        self._type_queue: list[tuple[str, str, int]] = []
        self._type_timer: int | None = None
        self._type_total = 0
        self._type_progress: Any = None
        self._scroll_accum: float = 0.0

        kc = Gtk.EventControllerKey(propagation_phase=Gtk.PropagationPhase.CAPTURE)
        kc.connect("key-pressed", self._on_key_pressed)
        kc.connect("key-released", self._on_key_released)
        win.add_controller(kc)

        sc = Gtk.EventControllerScroll(flags=Gtk.EventControllerScrollFlags.VERTICAL)
        sc.connect("scroll", self._on_scroll)
        sc.connect("scroll-end", lambda _c: setattr(self, "_scroll_accum", 0.0))
        stage.add_controller(sc)

        win.connect("notify::is-active", lambda w, _p: None if w.is_active() else self.on_focus_lost())
        self.stage.update_keyboard_chip(enabled=self.enabled, live_typing=self.live_typing)

    def _on_scroll(self, _c: Any, _dx: float, dy: float) -> bool:
        if not self.enabled or dy == 0.0 or self.win.get_visible_dialog() is not None:
            return False
        lk = self.runtime.link
        if not lk.connected or lk.link_mode != "kern_os":
            return False
        self._scroll_accum += float(dy)
        steps = int(self._scroll_accum)
        if steps != 0:
            self._scroll_accum -= float(steps)
            lk.send_encoder(max(-8, min(8, steps)))
        return True

    def _text_focused(self) -> bool:
        foc = self.win.get_focus()
        if foc is None:
            return False
        if isinstance(foc, (Gtk.Text, Gtk.Entry, Gtk.SearchEntry, Gtk.TextView)):
            return True
        # Never intercept keystrokes inside an embedded VTE terminal!
        return type(foc).__name__ == "Terminal"

    def _passthrough(self, state: Any) -> bool:
        chord = Gdk.ModifierType.CONTROL_MASK | Gdk.ModifierType.ALT_MASK | Gdk.ModifierType.SUPER_MASK
        return bool(state & chord) or self._text_focused() or self.win.get_visible_dialog() is not None

    def _key_names(self, keyval: int, keycode: int) -> tuple[str, str | None]:
        name = Gdk.keyval_name(keyval) or ""
        base: str | None = None
        ok, base_kv, _grp, _lvl, _cons = self.win.get_display().translate_key(keycode, 0, 0)
        if ok:
            base = Gdk.keyval_name(base_kv)
        if base is None:
            base = Gdk.keyval_name(Gdk.keyval_to_lower(keyval))
        return name, base

    def _set_alt_hints(self, on: bool) -> None:
        if on != self.stage.alt_hints:
            self.stage.alt_hints = on
            self.stage.area.queue_draw()

    def _on_key_pressed(self, _c: Any, keyval: int, keycode: int, state: Any) -> bool:
        if keyval in (Gdk.KEY_Alt_L, Gdk.KEY_Alt_R) and not self._text_focused():
            self._set_alt_hints(True)
            return False
        if not self.enabled or self._passthrough(state):
            return False
        if self.live_typing:
            return self._on_live_typing_key(keyval)
        name, base = self._key_names(keyval, keycode)
        shift = bool(state & Gdk.ModifierType.SHIFT_MASK)

        # Model-aware navigation: K1 uses < / > (Left / Right) and Up / Down; K5v3 uses Up / Down
        if name in ("Left", "Right") and self.runtime.active_model != "k1":
            return False

        action, long_press = resolve_binding(self._key_index, name, base, shift)
        if action is not None:
            if action in DOCK_ACTIONS:
                self.stage._run_quick(action)
            elif action in self.stage.pressed_keys:
                pass
            elif long_press:
                self.stage.long_click_key(action)
            else:
                self.stage.press_key(action)
            return True

        # Unified direct character input: letters, space, punctuation & BackSpace work
        # directly inside TextInput via FACET_INPUT_TEXT without switching modes!
        lk = self.runtime.link
        if lk.connected and lk.link_mode == "kern_os":
            if keyval == Gdk.KEY_BackSpace:
                lk.send_text("\x08")
                return True
            ch = chr(Gdk.keyval_to_unicode(keyval) or 0)
            if ch.isprintable():
                lk.send_text(ch)
                return True
        return False

    def _on_key_released(self, _c: Any, keyval: int, keycode: int, state: Any) -> None:
        if keyval in (Gdk.KEY_Alt_L, Gdk.KEY_Alt_R):
            self._set_alt_hints(False)
            return
        if self.live_typing:
            return
        name, base = self._key_names(keyval, keycode)
        action, _lp = resolve_binding(self._key_index, name, base, bool(state & Gdk.ModifierType.SHIFT_MASK))
        for cand in (action, self._key_index.get(base or "")):
            if cand and cand in self.stage.pressed_keys and cand not in self.stage.long_pressing:
                self.stage.release_key(cand)

    def _on_live_typing_key(self, keyval: int) -> bool:
        lk = self.runtime.link
        if keyval == Gdk.KEY_Escape:
            self.set_live_typing(False)
            return True
        if not lk.connected or lk.link_mode != "kern_os":
            self._on_toast("Connect a Kern OS radio first")
            return True
        if keyval == Gdk.KEY_BackSpace:
            lk.send_text("\x08")
            return True
        if keyval in (Gdk.KEY_Return, Gdk.KEY_KP_Enter):
            lk.send_text("\n")
            if lk.demo_mode:
                self._finish_demo_text()
            return True
        ch = chr(Gdk.keyval_to_unicode(keyval) or 0)
        if not ch.isprintable():
            return False
        lk.send_text(ch)
        return True

    def _finish_demo_text(self) -> bool:
        if self._type_queue:
            return True
        txt = self.runtime.link.end_demo_text()
        if txt is not None:
            self._on_toast(f"Entered “{txt}”")
        if self.live_typing:
            self.runtime.link.begin_demo_text()
        return False

    def _enqueue_taps(self, seq: list[tuple[str, str, int]]) -> None:
        if not seq:
            return
        lk = self.runtime.link
        if not lk.connected or lk.link_mode != "kern_os":
            self._on_toast("Connect a Kern OS radio first")
            return
        self._type_queue.extend(seq)
        self._type_total += len(seq)
        if self._type_timer is None:
            self._type_timer = GLib.timeout_add(1, self._type_step)

    def _type_step(self) -> bool:
        self._type_timer = None
        lk = self.runtime.link
        if not self._type_queue or not lk.connected:
            self._type_queue.clear()
            self._type_total = 0
            self._update_type_progress()
            return False
        key, state, delay = self._type_queue.pop(0)
        lk.send_key(key, state)
        if state == "press":
            self.stage.pressed_keys.add(key)
        else:
            self.stage.pressed_keys.discard(key)
        self.stage.ensure_anim_loop()
        if self.stage.on_rec_event:
            self.stage.on_rec_event()
        self._update_type_progress()
        self._type_timer = GLib.timeout_add(max(1, delay), self._type_step)
        return False

    def _update_type_progress(self) -> None:
        bar = self._type_progress
        if bar is None:
            return
        done = self._type_total - len(self._type_queue)
        frac = (done / self._type_total) if self._type_total else 1.0
        bar.set_fraction(frac)
        bar.set_text(f"{int(frac * 100)} %")

    def cancel_typing(self) -> None:
        if self._type_timer is not None:
            GLib.source_remove(self._type_timer)
            self._type_timer = None
        self._type_queue.clear()
        self._type_total = 0
        lk = self.runtime.link
        for k in list(self.stage.pressed_keys):
            self.stage.pressed_keys.discard(k)
            if lk.connected and lk.link_mode == "kern_os":
                lk.send_key(k, "release")
        self.stage.ensure_anim_loop()

    def on_focus_lost(self) -> None:
        self._set_alt_hints(False)
        if not self._type_queue:
            self.stage.release_all_keys()

    def set_enabled(self, on: bool) -> None:
        if on == self.enabled:
            return
        self.enabled = on
        self.runtime.config.set("ui.keyboard_bridge", on)
        if not on and self.live_typing:
            self.set_live_typing(False)
        self.stage.release_all_keys()
        self._emit_state()

    def set_live_typing(self, on: bool) -> None:
        if on == self.live_typing:
            return
        lk = self.runtime.link
        if on and not (lk.connected and lk.link_mode == "kern_os"):
            self._on_toast("Connect a Kern OS radio to type on it")
            self._emit_state()
            return
        self.live_typing = on
        self._planner.reset()
        if on:
            if not self.enabled:
                self.enabled = True
            self.stage.release_all_keys()
            if lk.demo_mode:
                lk.begin_demo_text()
            self.stage.area.grab_focus()
            self._on_toast("Live typing active · Esc to stop")
        elif lk.demo_mode:
            lk.end_demo_text()
        self._emit_state()

    def toggle_chip(self) -> None:
        if self.live_typing:
            self.set_live_typing(False)
        else:
            self.set_enabled(not self.enabled)

    def _emit_state(self) -> None:
        self.stage.update_keyboard_chip(enabled=self.enabled, live_typing=self.live_typing)
        self.runtime.events.emit("keyboard.changed", {"enabled": self.enabled, "live_typing": self.live_typing}, source="keyboard")
        if self._on_state_change:
            self._on_state_change()

    def show_type_dialog(self) -> None:
        lk = self.runtime.link
        if not (lk.connected and lk.link_mode == "kern_os"):
            self._on_toast("Connect a Kern OS radio to type on it")
            return
        dlg = Adw.Dialog(title="Type on Radio", content_width=420)
        tb = Adw.ToolbarView()
        tb.add_top_bar(Adw.HeaderBar())
        body = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=12, margin_top=6, margin_bottom=18, margin_start=18, margin_end=18)
        hint = Gtk.Label(
            label="Open a text field on the radio (channel name, contact…), then send directly over RigWire.",
            wrap=True,
            xalign=0,
        )
        hint.add_css_class("dim-label")
        body.append(hint)

        lst = Gtk.ListBox(selection_mode=Gtk.SelectionMode.NONE)
        lst.add_css_class("boxed-list")
        entry = Adw.EntryRow(title="Text")
        entry.set_max_length(T9_MAX_LEN)
        lst.append(entry)
        confirm = Adw.SwitchRow(title="Submit on Send", subtitle="Confirm the field immediately after sending text", active=True)
        lst.append(confirm)
        body.append(lst)

        info = Gtk.Label(xalign=0, wrap=True)
        info.add_css_class("caption")
        info.add_css_class("numeric")
        body.append(info)

        send = Gtk.Button(label="Send to Radio", sensitive=False, halign=Gtk.Align.CENTER)
        send.add_css_class("suggested-action")
        send.add_css_class("pill")
        body.append(send)
        tb.set_content(body)
        dlg.set_child(tb)

        def _preview(*_a: Any) -> None:
            text = entry.get_text()
            info.set_text(f"{len(text)} / {T9_MAX_LEN} characters · direct RigWire text" if text else f"Up to {T9_MAX_LEN} characters")
            send.set_sensitive(bool(text))

        def _send(*_a: Any) -> None:
            text = entry.get_text()
            if not text:
                return
            self.stage.release_all_keys()
            payload = text + ("\n" if confirm.get_active() else "")
            lk.send_text(payload)
            self._on_toast(f"Sent “{text}”")
            dlg.close()

        entry.connect("changed", _preview)
        entry.connect("entry-activated", _send)
        send.connect("clicked", _send)
        _preview()
        dlg.present(self.win)
        entry.grab_focus()
