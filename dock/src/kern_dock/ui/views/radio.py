"""Radio View: live VFO frequency card, S-meter, direct tuning, modulation, squelch & keyboard controls."""

from __future__ import annotations

from typing import Any, Callable

import gi

gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")
from gi.repository import Adw, Gtk  # noqa: E402

import rigwire

from ..runtime import DockRuntime
from ..widgets.keyboard_bridge import KeyboardBridge


class RadioView:
    """Live RF transceiver control and keyboard bridge view."""

    def __init__(
        self,
        runtime: DockRuntime,
        keyboard: KeyboardBridge,
        *,
        on_toast: Callable[[str], None],
        on_show_shortcuts: Callable[[], None],
    ) -> None:
        self.runtime = runtime
        self.keyboard = keyboard
        self._on_toast = on_toast
        self._on_show_shortcuts = on_show_shortcuts
        self._updating = False
        self._mod_names = list(rigwire.MOD_NAMES.values())

        self.widget = self._build()

        runtime.events.subscribe("link.connected", lambda _e: self.refresh(), main_thread=True)
        runtime.events.subscribe("link.disconnected", lambda _e: self.refresh(), main_thread=True)
        runtime.events.subscribe("link.telemetry", lambda _e: self.refresh(), main_thread=True)
        runtime.events.subscribe("keyboard.changed", lambda _e: self._sync_kbd_switches(), main_thread=True)
        self.refresh()

    def _build(self) -> Gtk.Stack:
        self.stack = Gtk.Stack()
        self.stack.add_named(
            Adw.StatusPage(
                icon_name="network-wireless-symbolic",
                title="Radio Offline",
                description="Connect a Kern OS radio to tune frequencies, switch modulation and type from your PC keyboard.",
            ),
            "offline",
        )

        page = Adw.PreferencesPage()

        # Hero Frequency & S-Meter Card
        hero = Adw.PreferencesGroup()
        card = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=6)
        card.add_css_class("card")
        card.add_css_class("freq-card")

        top = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        self.vfo_badge = Gtk.Label(label="VFO A")
        self.vfo_badge.add_css_class("freq-badge")
        self.mode_badge = Gtk.Label(label="FM")
        self.mode_badge.add_css_class("freq-badge")
        self.mode_badge.add_css_class("accent")
        top.append(self.vfo_badge)
        top.append(self.mode_badge)
        top.append(Gtk.Box(hexpand=True))
        self.rx_badge = Gtk.Label(label="")
        self.rx_badge.add_css_class("freq-badge")
        top.append(self.rx_badge)
        card.append(top)

        frow = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=6)
        self.freq_lbl = Gtk.Label(label="———.———", xalign=0)
        self.freq_lbl.add_css_class("freq-display")
        unit = Gtk.Label(label="MHz", valign=Gtk.Align.END, margin_bottom=8)
        unit.add_css_class("dim-label")
        frow.append(self.freq_lbl)
        frow.append(unit)
        card.append(frow)

        srow = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        s_lbl = Gtk.Label(label="S")
        s_lbl.add_css_class("caption-heading")
        self.rssi_bar = Gtk.LevelBar(min_value=0.0, max_value=9.0, mode=Gtk.LevelBarMode.DISCRETE, hexpand=True, valign=Gtk.Align.CENTER)
        self.rssi_bar.set_size_request(-1, 6)
        self.rssi_lbl = Gtk.Label(label="— dBm")
        self.rssi_lbl.add_css_class("caption")
        self.rssi_lbl.add_css_class("numeric")
        srow.append(s_lbl)
        srow.append(self.rssi_bar)
        srow.append(self.rssi_lbl)
        card.append(srow)
        hero.add(card)
        page.add(hero)

        # Tuning Controls
        tune = Adw.PreferencesGroup(title="Tuning &amp; Receiver")
        self.freq_entry = Adw.EntryRow(title="Direct Frequency (MHz)", show_apply_button=True, input_purpose=Gtk.InputPurpose.NUMBER)
        self.freq_entry.connect("apply", self._on_freq_apply)
        self.freq_entry.connect("entry-activated", self._on_freq_apply)
        tune.add(self.freq_entry)

        self.mode_row = Adw.ComboRow(title="Modulation", model=Gtk.StringList.new(self._mod_names))
        self.mode_row.connect("notify::selected", self._on_mode_selected)
        tune.add(self.mode_row)

        self.sql_row = Adw.SpinRow.new_with_range(0, 9, 1)
        self.sql_row.set_title("Squelch Level")
        self.sql_row.connect("notify::value", lambda r, _p: None if self._updating else self.runtime.link.set_squelch(int(r.get_value())))
        tune.add(self.sql_row)

        swap = Adw.ButtonRow(title="Swap VFO A / B", start_icon_name="view-dual-symbolic")
        swap.connect("activated", lambda _r: self.runtime.link.toggle_vfo())
        tune.add(swap)
        page.add(tune)

        # PC Keyboard Bridge
        kb = Adw.PreferencesGroup(title="Keyboard")
        self.kbd_row = Adw.SwitchRow(title="Keyboard Control", subtitle="Use PC keys for radio buttons (Ctrl+K)", active=self.keyboard.enabled)
        self.kbd_row.connect("notify::active", lambda w, _p: None if self._updating else self.keyboard.set_enabled(w.get_active()))
        kb.add(self.kbd_row)

        self.live_row = Adw.SwitchRow(title="Live Typing", subtitle="Type directly into radio inputs (Ctrl+Shift+T)")
        self.live_row.connect("notify::active", lambda w, _p: None if self._updating else self.keyboard.set_live_typing(w.get_active()))
        kb.add(self.live_row)

        type_row = Adw.ButtonRow(title="Type Text…", start_icon_name="input-keyboard-symbolic")
        type_row.connect("activated", lambda _r: self.keyboard.show_type_dialog())
        kb.add(type_row)

        keys_row = Adw.ButtonRow(title="Key Bindings", start_icon_name="input-keyboard-symbolic")
        keys_row.connect("activated", lambda _r: self._on_show_shortcuts())
        kb.add(keys_row)
        page.add(kb)

        self.stack.add_named(page, "online")
        return self.stack

    def _on_freq_apply(self, row: Any) -> None:
        txt = row.get_text().strip().replace(",", ".")
        try:
            mhz = float(txt)
        except ValueError:
            self._on_toast(f"'{txt}' is not a valid frequency")
            return
        hz = int(round(mhz * 1e6 if mhz < 2000 else mhz))
        self.runtime.link.set_frequency_hz(hz)
        row.set_text("")
        self.refresh()

    def _on_mode_selected(self, row: Any, _p: Any) -> None:
        if self._updating:
            return
        idx = row.get_selected()
        if 0 <= idx < len(self._mod_names):
            self.runtime.link.set_modulation(self._mod_names[idx])

    def _sync_kbd_switches(self) -> None:
        self._updating = True
        self.kbd_row.set_active(self.keyboard.enabled)
        self.live_row.set_active(self.keyboard.live_typing)
        self._updating = False

    def refresh(self) -> None:
        lk = self.runtime.link
        online = lk.connected and lk.link_mode == "kern_os"
        self.stack.set_visible_child_name("online" if online else "offline")
        if not online:
            return
        st = lk.radio_state
        hz = int(st.get("rx_freq_hz") or 0)
        if hz:
            mhz, rem = divmod(hz, 1_000_000)
            self.freq_lbl.set_markup(f"{mhz}.{rem // 1000:03d}<span alpha='55%' size='60%'> {(rem % 1000) // 10:02d}</span>")
        else:
            self.freq_lbl.set_text("———.———")
        mode = str(st.get("mod_name", "FM"))
        self.mode_badge.set_text(mode)
        self.vfo_badge.set_text(f"VFO {'B' if st.get('active_vfo') else 'A'}")
        rssi = int(st.get("rssi_dbm", -127))
        self.rssi_bar.set_value(max(0.0, min(9.0, (rssi + 127) / 6.0)))
        self.rssi_lbl.set_text(f"{rssi} dBm" if rssi > -127 else "— dBm")
        tx, rx = st.get("ptt_active"), st.get("carrier")
        self.rx_badge.set_text("TX" if tx else ("RX" if rx else ""))
        self.rx_badge.set_visible(bool(tx or rx))
        self._updating = True
        if mode in self._mod_names:
            self.mode_row.set_selected(self._mod_names.index(mode))
        if "squelch" in st:
            self.sql_row.set_value(float(st["squelch"]))
        self._updating = False
