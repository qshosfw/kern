"""Multi-page Preferences (Handset, Plugins, OS & Desktop, Security), Shortcuts and About dialogs."""

from __future__ import annotations

from typing import Any, Callable

import gi

gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")
from gi.repository import Adw, Gdk, GLib, Gtk  # noqa: E402

from ... import APP_NAME, WORKSPACE_PLUGINS_DIR, __version__
from ...audio import list_audio_devices
from ...device.keys import APP_SHORTCUTS, DOCK_ACTIONS, KEY_BINDINGS
from ...plugins.manager import user_plugins_dir
from ...render.handset.catalog import MODELS
from ...render.lcd import LCD_PALETTES
from ...system.desktop import (
    desktop_integration_status,
    install_desktop_integration,
    open_path_in_os,
    uninstall_desktop_integration,
)
from ..runtime import DockRuntime


def show_preferences_dialog(win: Any, runtime: DockRuntime, *, on_toast: Callable[[str], None], on_scheme: Callable[[str], None]) -> None:
    dlg = Adw.PreferencesDialog(title="Preferences")

    # =========================================================================
    # Page 1: Dock & Handset
    # =========================================================================
    p_disp = Adw.PreferencesPage(title="Dock & Handset", icon_name="preferences-system-symbolic")

    grp_m = Adw.PreferencesGroup(title="Handset Model")
    first_chk: Gtk.CheckButton | None = None

    def _make_swatch(mkey: str) -> Gtk.DrawingArea:
        spec = MODELS[mkey]
        pal = LCD_PALETTES.get(spec.palette, LCD_PALETTES["UV-K1"])
        lcd_rgb = tuple(c / 255.0 for c in pal["bg"])
        # Pick the most visually iconic shell color (crimson armor on k5-99-red, safety orange body, etc.)
        shell_rgb = spec.armor_stops[0][1] if mkey in ("k5-99-red", "k1", "k5v3", "k10-pro") else spec.body_tone
        sw = Gtk.DrawingArea(valign=Gtk.Align.CENTER)
        sw.set_content_width(22)
        sw.set_content_height(14)

        def _draw(_a: Any, cr: Any, w: int, h: int) -> None:
            cr.save()
            # Split pill: left half = chassis finish, right half = LCD backlight
            r = 4.0
            cr.new_sub_path()
            cr.arc(w - r, r, r, -1.5708, 0.0)
            cr.arc(w - r, h - r, r, 0.0, 1.5708)
            cr.arc(r, h - r, r, 1.5708, 3.1416)
            cr.arc(r, r, r, 3.1416, 4.7124)
            cr.close_path()
            cr.clip_preserve()
            cr.set_source_rgb(*shell_rgb)
            cr.rectangle(0, 0, w * 0.55, h)
            cr.fill()
            cr.set_source_rgb(*lcd_rgb)
            cr.rectangle(w * 0.55, 0, w * 0.45, h)
            cr.fill()
            cr.set_source_rgba(1.0, 1.0, 1.0, 0.25)
            cr.set_line_width(1.0)
            cr.stroke()
            cr.restore()

        sw.set_draw_func(_draw)
        return sw

    def _add_model_row(container: Any, mkey: str | None, mtitle: str, msub: str, badge: str | None = None) -> None:
        nonlocal first_chk
        row = Adw.ActionRow(
            title=GLib.markup_escape_text(mtitle),
            subtitle=GLib.markup_escape_text(msub),
            activatable=True,
        )
        chk = Gtk.CheckButton(active=(runtime.forced_model == mkey))
        if first_chk is None:
            first_chk = chk
        else:
            chk.set_group(first_chk)
        chk.connect("toggled", lambda b, k=mkey: runtime.set_model(k) if b.get_active() else None)
        row.add_prefix(chk)
        row.set_activatable_widget(chk)
        if mkey is not None:
            row.add_suffix(_make_swatch(mkey))
        if badge:
            tag = Gtk.Label(label=badge, valign=Gtk.Align.CENTER)
            tag.add_css_class("cap-chip")
            row.add_suffix(tag)
        if isinstance(container, Adw.ExpanderRow):
            container.add_row(row)
        else:
            container.add(row)

    _add_model_row(grp_m, None, "Auto-detect", "Match connected radio")
    for sec_title, sec_keys, badge in (
        ("UV-K1 Series", ("k1", "k1-polymer", "k1-clear"), None),
        ("UV-K5 / K6 / 5R", ("k5v3", "k5-orange", "k5-black", "k5-99", "k5-99-red", "uv5r-plus"), None),
        ("UV-K10 Series", ("k10-pro",), None),
        ("Developer Views", ("k1-naked", "pcb-r51"), "Dev"),
    ):
        n = len(sec_keys)
        unit = "view" if badge else "model"
        exp = Adw.ExpanderRow(
            title=GLib.markup_escape_text(sec_title),
            subtitle=f"{n} {unit}{'s' if n != 1 else ''}",
            expanded=(runtime.forced_model in sec_keys),
        )
        for mk in sec_keys:
            _add_model_row(exp, mk, MODELS[mk].label, MODELS[mk].description, badge=badge)
        grp_m.add(exp)

    schemes = ["dark", "system", "light"]
    scheme_row = Adw.ComboRow(title="Window Theme", model=Gtk.StringList.new(["Dark", "Follow System", "Light"]))
    cur_sc = str(runtime.config.get("ui.color_scheme", "dark"))
    scheme_row.set_selected(schemes.index(cur_sc) if cur_sc in schemes else 0)
    scheme_row.connect("notify::selected", lambda r, _p: (runtime.config.set("ui.color_scheme", schemes[r.get_selected()]), on_scheme(schemes[r.get_selected()])))
    grp_m.add(scheme_row)
    p_disp.add(grp_m)

    grp_lcd = Adw.PreferencesGroup(title="ST7565 LCD Optics")
    pnames = ["auto"] + list(LCD_PALETTES.keys())
    plabels = ["Match Model"] + list(LCD_PALETTES.keys())
    pal_row = Adw.ComboRow(title="LCD Palette", model=Gtk.StringList.new(plabels))
    cur_pal = "auto" if not runtime.user_picked_palette else runtime.lcd_st.palette
    pal_row.set_selected(pnames.index(cur_pal) if cur_pal in pnames else 0)
    pal_row.connect("notify::selected", lambda r, _p: runtime.set_palette(pnames[r.get_selected()]))
    grp_lcd.add(pal_row)

    for title, lo, hi, attr in (("Backlight", 0, 10, "backlight"), ("Contrast", 0, 15, "contrast")):
        row = Adw.SpinRow.new_with_range(lo, hi, 1)
        row.set_title(title)
        row.set_value(getattr(runtime.lcd_st, attr))
        row.connect("notify::value", lambda s, _p, a=attr: runtime.set_lcd_setting(a, int(s.get_value())))
        grp_lcd.add(row)

    for title, sub, attr in (
        ("STN Ghosting", "Liquid-crystal rise &amp; decay", "ghosting"),
        ("Pixel Grid", "128×64 dot dividers", "pixel_grid"),
        ("Backlight Glow", "Soft bezel light spill", "backlight_glow"),
        ("Invert LCD", "Swap pixel polarity", "invert_lcd"),
    ):
        sw = Adw.SwitchRow(title=title, subtitle=sub, active=getattr(runtime.lcd_st, attr))
        sw.connect("notify::active", lambda w, _p, a=attr: runtime.set_lcd_setting(a, w.get_active()))
        grp_lcd.add(sw)
    p_disp.add(grp_lcd)
    dlg.add(p_disp)

    # =========================================================================
    # Page 1b: Audio & Kenwood Sound Card
    # =========================================================================
    p_aud = Adw.PreferencesPage(title="Audio", icon_name="audio-headphones-symbolic")
    aud = runtime.audio

    grp_hl = Adw.PreferencesGroup(title="Headless Audio")
    sw_hp = Adw.SwitchRow(
        title="Headphones",
        subtitle="Play RigWire tones &amp; Kenwood RX",
        active=aud.headphones_enabled,
    )
    sw_hp.connect("notify::active", lambda w, _p: aud.set_headphones(w.get_active()))
    grp_hl.add(sw_hp)

    sw_mic = Adw.SwitchRow(
        title="Microphone",
        subtitle="Send PC mic to Kenwood TX",
        active=aud.mic_enabled,
    )
    sw_mic.connect("notify::active", lambda w, _p: aud.set_mic(w.get_active()))
    grp_hl.add(sw_mic)

    vol_row = Adw.SpinRow.new_with_range(0, 100, 5)
    vol_row.set_title("Beep Volume (%)")
    vol_row.set_value(int(round(aud.volume * 100)))
    vol_row.connect("notify::value", lambda s, _p: aud.set_volume(s.get_value() / 100.0))
    grp_hl.add(vol_row)

    test_tone = Adw.ButtonRow(title="Test Chime", start_icon_name="audio-speakers-symbolic")
    test_tone.connect("activated", lambda _r: aud.play_headless_notes([(880, 55), (1320, 85)]))
    grp_hl.add(test_tone)
    p_aud.add(grp_hl)

    grp_kw = Adw.PreferencesGroup(title="Kenwood Sound Card")
    devs = list_audio_devices()
    in_devs = [d for d in devs if d.max_inputs > 0]
    out_devs = [d for d in devs if d.max_outputs > 0]

    in_labels = ["None (RigWire Beeps Only)"] + [f"{d.name} (#{d.index})" for d in in_devs]
    in_ids: list[int | None] = [None] + [d.index for d in in_devs]
    rx_row = Adw.ComboRow(title="Radio RX Input (Speaker Jack)", model=Gtk.StringList.new(in_labels))
    rx_row.set_selected(in_ids.index(aud.kenwood_rx_device) if aud.kenwood_rx_device in in_ids else 0)
    rx_row.connect("notify::selected", lambda r, _p: aud.pair_kenwood(in_ids[r.get_selected()], aud.kenwood_tx_device))
    grp_kw.add(rx_row)

    out_labels = ["None (TX Audio Disabled)"] + [f"{d.name} (#{d.index})" for d in out_devs]
    out_ids: list[int | None] = [None] + [d.index for d in out_devs]
    tx_row = Adw.ComboRow(title="Radio TX Output (Mic Jack)", model=Gtk.StringList.new(out_labels))
    tx_row.set_selected(out_ids.index(aud.kenwood_tx_device) if aud.kenwood_tx_device in out_ids else 0)
    tx_row.connect("notify::selected", lambda r, _p: aud.pair_kenwood(aud.kenwood_rx_device, out_ids[r.get_selected()]))
    grp_kw.add(tx_row)

    p_aud.add(grp_kw)
    dlg.add(p_aud)

    # =========================================================================
    # Page 2: Plugins
    # =========================================================================
    p_plug = Adw.PreferencesPage(title="Plugins", icon_name="application-x-addon-symbolic")
    grp_p = Adw.PreferencesGroup(
        title="Installed Plugins",
        description=f"Loaded from built-ins, {WORKSPACE_PLUGINS_DIR.name}/ and ~/.config/kern-dock/plugins/",
    )
    recs = runtime.plugins.records()
    if not recs:
        grp_p.add(Adw.ActionRow(title="No plugins discovered"))
    else:
        for rec in recs:
            m = rec.manifest
            sub = f"Error: {rec.error}" if rec.error else m.description
            row = Adw.ExpanderRow(
                title=GLib.markup_escape_text(f"{m.name} v{m.version}"),
                subtitle=GLib.markup_escape_text(sub),
            )
            row.add_prefix(Gtk.Image(icon_name=m.icon))

            origin_lbl = Gtk.Label(label=rec.origin, valign=Gtk.Align.CENTER)
            origin_lbl.add_css_class("cap-chip")
            row.add_suffix(origin_lbl)

            sw = Gtk.Switch(active=rec.enabled, valign=Gtk.Align.CENTER)
            sw.connect(
                "notify::active",
                lambda w, _p, pid=m.id: (
                    runtime.plugins.set_enabled(pid, w.get_active()),
                    on_toast(f"Plugin '{pid}' {'enabled' if w.get_active() else 'disabled'}"),
                ),
            )
            row.add_suffix(sw)

            row.add_row(Adw.ActionRow(title="Capabilities", subtitle=", ".join(m.capabilities) if m.capabilities else "(none)"))
            row.add_row(Adw.ActionRow(title="ID &amp; Author", subtitle=GLib.markup_escape_text(f"{m.id} · {m.author}")))
            grp_p.add(row)

    open_dir = Adw.ButtonRow(title="Open User Plugins Folder", start_icon_name="folder-open-symbolic")

    def _open_pdir(_r: Any) -> None:
        d = user_plugins_dir()
        d.mkdir(parents=True, exist_ok=True)
        open_path_in_os(d)

    open_dir.connect("activated", _open_pdir)
    grp_p.add(open_dir)
    p_plug.add(grp_p)
    dlg.add(p_plug)

    # =========================================================================
    # Page 3: OS & Desktop Integration
    # =========================================================================
    p_os = Adw.PreferencesPage(title="OS & Desktop", icon_name="preferences-desktop-symbolic")

    grp_conn = Adw.PreferencesGroup(title="Connection &amp; Tray")
    sw_ac = Adw.SwitchRow(title="Auto-Connect USB", subtitle="Attach when plugged in", active=runtime.auto_connect_usb)
    sw_ac.connect("notify::active", lambda w, _p: (setattr(runtime, "auto_connect_usb", w.get_active()), runtime.config.set("connection.auto_connect", w.get_active())))
    grp_conn.add(sw_ac)

    sw_tray = Adw.SwitchRow(title="System Tray", subtitle="Show panel indicator", active=bool(runtime.config.get("os.tray_enabled", True)))

    def _toggle_tray(w: Any, _p: Any) -> None:
        on = w.get_active()
        runtime.config.set("os.tray_enabled", on)
        if on:
            runtime.tray.start()
            runtime.refresh_tray()
        else:
            runtime.tray.stop()

    sw_tray.connect("notify::active", _toggle_tray)
    grp_conn.add(sw_tray)

    sw_c2t = Adw.SwitchRow(title="Minimize to Tray", subtitle="Keep running on close", active=bool(runtime.config.get("os.close_to_tray", False)))
    sw_c2t.connect("notify::active", lambda w, _p: runtime.config.set("os.close_to_tray", w.get_active()))
    grp_conn.add(sw_c2t)

    sw_notif = Adw.SwitchRow(title="Notifications", subtitle="Connect &amp; bootloader alerts", active=runtime.notifier.enabled)
    sw_notif.connect("notify::active", lambda w, _p: (setattr(runtime.notifier, "enabled", w.get_active()), runtime.config.set("os.notifications", w.get_active())))
    grp_conn.add(sw_notif)
    p_os.add(grp_conn)

    grp_desk = Adw.PreferencesGroup(title="Desktop &amp; IPC")
    st = desktop_integration_status()
    inst_row = Adw.ActionRow(
        title="Desktop Entry &amp; Radio Icon",
        subtitle="Installed in ~/.local/share" if st["installed"] else "Not yet installed in ~/.local/share",
    )
    inst_btn = Gtk.Button(label="Reinstall" if st["installed"] else "Install", valign=Gtk.Align.CENTER)
    inst_btn.add_css_class("pill")

    def _do_install(_b: Any) -> None:
        install_desktop_integration()
        inst_row.set_subtitle("Installed in ~/.local/share")
        inst_btn.set_label("Reinstall")
        on_toast("Installed desktop launcher & radio icon")

    inst_btn.connect("clicked", _do_install)
    inst_row.add_suffix(inst_btn)
    grp_desk.add(inst_row)

    rm_row = Adw.ButtonRow(title="Remove Desktop Launcher &amp; Icon")
    rm_row.connect("activated", lambda _r: (uninstall_desktop_integration(), inst_row.set_subtitle("Removed from ~/.local/share"), on_toast("Removed desktop launcher")))
    grp_desk.add(rm_row)

    term_row = Adw.EntryRow(title="External Terminal Command", show_apply_button=True)
    term_row.set_text(str(runtime.config.get("os.terminal_cmd", "auto")))
    term_row.connect("apply", lambda r: (runtime.config.set("os.terminal_cmd", r.get_text().strip() or "auto"), on_toast("Updated terminal preference")))
    grp_desk.add(term_row)

    sw_ipc = Adw.SwitchRow(title="Local IPC &amp; Session D-Bus", subtitle=GLib.markup_escape_text(f"org.kernos.Dock · {runtime.ipc.socket_path}"), active=bool(runtime.config.get("os.ipc_enabled", True)))
    sw_ipc.connect("notify::active", lambda w, _p: (runtime.config.set("os.ipc_enabled", w.get_active()), runtime.ipc.start() if w.get_active() else runtime.ipc.stop()))
    copy_cli = Gtk.Button(label="Copy CLI", valign=Gtk.Align.CENTER, tooltip_text="Copy './kern dock ctl status'")
    copy_cli.add_css_class("pill")
    copy_cli.add_css_class("small-pill")
    copy_cli.connect("clicked", lambda _b: (Gdk.Display.get_default().get_clipboard().set("./kern dock ctl status"), on_toast("Copied './kern dock ctl status'")))
    sw_ipc.add_suffix(copy_cli)
    grp_desk.add(sw_ipc)
    p_os.add(grp_desk)
    dlg.add(p_os)

    # =========================================================================
    # Page 4: Safety & Audit Log
    # =========================================================================
    p_sec = Adw.PreferencesPage(title="Safety", icon_name="security-high-symbolic")
    grp_s = Adw.PreferencesGroup(title="RF &amp; IPC Safety")

    tx_spin = Adw.SpinRow.new_with_range(0, 600, 15)
    tx_spin.set_title("TX Auto-Relock (s)")
    tx_spin.set_subtitle("Auto-disarm USB TX (0 disables)")
    tx_spin.set_value(runtime.security.tx_relock_seconds)
    tx_spin.connect("notify::value", lambda s, _p: (setattr(runtime.security, "tx_relock_seconds", int(s.get_value())), runtime.config.set("security.tx_relock_seconds", int(s.get_value()))))
    grp_s.add(tx_spin)

    sw_ipc_ctl = Adw.SwitchRow(title="Local IPC Control", subtitle="Allow CLI &amp; D-Bus control", active=bool(runtime.config.get("security.ipc_allow_control", True)))
    sw_ipc_ctl.connect("notify::active", lambda w, _p: (runtime.security.set_ipc_allow_control(w.get_active()), runtime.config.set("security.ipc_allow_control", w.get_active())))
    grp_s.add(sw_ipc_ctl)
    p_sec.add(grp_s)

    grp_aud = Adw.PreferencesGroup(title="Recent Security Audit Log")
    entries = runtime.security.audit_entries(18)
    if not entries:
        grp_aud.add(Adw.ActionRow(title="No audit events yet"))
    else:
        for e in reversed(entries):
            r = Adw.ActionRow(
                title=GLib.markup_escape_text(f"[{e.ts_wall}] {e.category}.{e.action}"),
                subtitle=GLib.markup_escape_text(e.detail),
            )
            badge = Gtk.Label(label=e.outcome.upper(), valign=Gtk.Align.CENTER)
            badge.add_css_class("status-pill")
            badge.add_css_class("warn" if e.outcome == "deny" else ("live" if e.outcome == "allow" else "headless"))
            r.add_suffix(badge)
            grp_aud.add(r)
    p_sec.add(grp_aud)
    dlg.add(p_sec)

    dlg.present(win)


def show_shortcuts_dialog(win: Any) -> None:
    dlg = Adw.ShortcutsDialog()
    radio = Adw.ShortcutsSection(title="Radio Keys")
    radio.add(Adw.ShortcutsItem(title="Digits", accelerator="0...9"))
    dock = Adw.ShortcutsSection(title="Quick Hardware Controls")
    for action, desc, names in KEY_BINDINGS:
        if action.isdigit():
            continue
        item = Adw.ShortcutsItem(title=desc, accelerator=" ".join(n for n in names if not n.startswith("KP_")))
        (dock if action in DOCK_ACTIONS else radio).add(item)
    radio.add(Adw.ShortcutsItem(title="Long Press (or hold key)", accelerator="<Shift>m"))
    radio.add(Adw.ShortcutsItem(title="Show Key Hints on Handset", accelerator="Alt_L"))

    app_sec = Adw.ShortcutsSection(title="Dock & Tabs")
    for _name, desc, accel in APP_SHORTCUTS:
        app_sec.add(Adw.ShortcutsItem(title=desc, accelerator=accel))
    app_sec.add(Adw.ShortcutsItem(title="Close Active Tab", accelerator="<Control>w"))
    app_sec.add(Adw.ShortcutsItem(title="Open / Focus Tool Tab", accelerator="<Control>1...9"))
    for sec in (radio, dock, app_sec):
        dlg.add(sec)
    dlg.present(win)


def show_about_dialog(win: Any) -> None:
    Adw.AboutDialog(
        application_name=APP_NAME,
        application_icon="org.kernos.Dock",
        developer_name="Kern Project",
        version=__version__,
        comments="Modular, declarative, hardened GTK4 + Libadwaita hardware dock for Kern OS radios.",
        license_type=Gtk.License.GPL_3_0,
    ).present(win)
