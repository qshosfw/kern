"""Extensions & OS View: live plugin manager, capability inspection, system tray, desktop & IPC status."""

from __future__ import annotations

from typing import Any, Callable

import gi

gi.require_version("Gtk", "4.0")
gi.require_version("Gdk", "4.0")
gi.require_version("Adw", "1")
from gi.repository import Adw, Gdk, GLib, Gtk  # noqa: E402

from ... import WORKSPACE_PLUGINS_DIR
from ...plugins.manager import user_plugins_dir
from ...system.desktop import desktop_integration_status, install_desktop_integration, open_path_in_os
from ..runtime import DockRuntime


class ExtensionsView:
    """Plugin manager and OS integration control hub."""

    def __init__(self, runtime: DockRuntime, *, on_toast: Callable[[str], None]) -> None:
        self.runtime = runtime
        self._on_toast = on_toast
        self.widget = self._build()
        runtime.events.subscribe("plugins.changed", lambda _e: self._rebuild_plugins(), main_thread=True)
        self._rebuild_plugins()

    def _build(self) -> Adw.PreferencesPage:
        page = Adw.PreferencesPage()

        # OS Integration Overview
        os_grp = Adw.PreferencesGroup(
            title="Operating System Integration",
            description="Interface Kern Dock with your desktop shell, tray, terminal, scripts and external apps.",
        )
        st = desktop_integration_status()
        self.desk_row = Adw.ActionRow(
            title="Desktop Launcher &amp; GTK4 Radio Icon",
            subtitle="Installed in ~/.local/share" if st["installed"] else "Install org.kernos.Dock.desktop + scalable SVG radio icon",
        )
        self.desk_row.add_prefix(Gtk.Image(icon_name="org.kernos.Dock"))
        desk_btn = Gtk.Button(label="Install" if not st["installed"] else "Reinstall", valign=Gtk.Align.CENTER)
        desk_btn.add_css_class("pill")
        desk_btn.connect("clicked", self._on_install_desktop)
        self.desk_row.add_suffix(desk_btn)
        os_grp.add(self.desk_row)

        tray_state = "Active (StatusNotifierItem exported)" if self.runtime.tray.active else "Inactive"
        self.tray_row = Adw.ActionRow(title="System Tray Indicator", subtitle=tray_state)
        self.tray_row.add_prefix(Gtk.Image(icon_name="preferences-desktop-notifications-symbolic"))
        os_grp.add(self.tray_row)

        ipc_sub = f"Socket: {self.runtime.ipc.socket_path} · D-Bus: {'org.kernos.Dock' if self.runtime.ipc.dbus_active else 'off'}"
        ipc_row = Adw.ActionRow(title="Local IPC &amp; Session D-Bus", subtitle=ipc_sub)
        ipc_row.add_prefix(Gtk.Image(icon_name="network-server-symbolic"))
        copy_cli = Gtk.Button(label="Copy CLI", valign=Gtk.Align.CENTER, tooltip_text="Copy './kern dock ctl status'")
        copy_cli.add_css_class("pill")
        copy_cli.add_css_class("small-pill")
        copy_cli.connect("clicked", lambda _b: (Gdk.Display.get_default().get_clipboard().set("./kern dock ctl status"), self._on_toast("Copied './kern dock ctl status'")))
        ipc_row.add_suffix(copy_cli)
        os_grp.add(ipc_row)

        term_row = Adw.ActionRow(
            title="External OS Terminal",
            subtitle=f"Detected: {st['terminal'] or 'none'} · launches with KERN_PORT &amp; ./kern on PATH",
            activatable=True,
        )
        term_row.add_prefix(Gtk.Image(icon_name="utilities-terminal-symbolic"))
        term_row.add_suffix(Gtk.Image(icon_name="go-next-symbolic"))
        term_row.connect("activated", lambda _r: self._open_term())
        os_grp.add(term_row)
        page.add(os_grp)

        # Plugins Group
        self.plugins_grp = Adw.PreferencesGroup(
            title="Loaded Plugins",
            description=f"Auto-discovered from built-ins, {WORKSPACE_PLUGINS_DIR.name}/ and ~/.config/kern-dock/plugins/",
        )
        self.plugins_list = Gtk.ListBox(selection_mode=Gtk.SelectionMode.NONE)
        self.plugins_list.add_css_class("boxed-list")
        self.plugins_grp.add(self.plugins_list)

        open_dir = Adw.ButtonRow(title="Open User Plugins Folder", start_icon_name="folder-open-symbolic")
        open_dir.connect("activated", lambda _r: self._open_plugins_dir())
        self.plugins_grp.add(open_dir)
        page.add(self.plugins_grp)
        return page

    def _on_install_desktop(self, btn: Any) -> None:
        install_desktop_integration()
        self.desk_row.set_subtitle("Installed in ~/.local/share")
        btn.set_label("Reinstall")
        self._on_toast("Installed desktop launcher & GTK4 radio icon")

    def _open_term(self) -> None:
        try:
            term = self.runtime.open_external_terminal()
            self._on_toast(f"Opened {term}")
        except Exception as exc:
            self._on_toast(str(exc))

    def _open_plugins_dir(self) -> None:
        d = user_plugins_dir()
        d.mkdir(parents=True, exist_ok=True)
        open_path_in_os(d)

    def _rebuild_plugins(self) -> None:
        c = self.plugins_list.get_first_child()
        while c is not None:
            n = c.get_next_sibling()
            self.plugins_list.remove(c)
            c = n
        recs = self.runtime.plugins.records()
        if not recs:
            self.plugins_list.append(Adw.ActionRow(title="No plugins discovered"))
            return
        for rec in recs:
            m = rec.manifest
            sub = m.description
            if rec.error:
                sub = f"Error: {rec.error}"
            row = Adw.ExpanderRow(
                title=GLib.markup_escape_text(f"{m.name} v{m.version}"),
                subtitle=GLib.markup_escape_text(sub),
            )
            row.add_prefix(Gtk.Image(icon_name=m.icon))

            origin_lbl = Gtk.Label(label=rec.origin, valign=Gtk.Align.CENTER)
            origin_lbl.add_css_class("cap-chip")
            row.add_suffix(origin_lbl)

            sw = Gtk.Switch(active=rec.enabled, valign=Gtk.Align.CENTER)
            sw.connect("notify::active", lambda w, _p, pid=m.id: self._on_toggle_plugin(pid, w.get_active()))
            row.add_suffix(sw)

            caps_row = Adw.ActionRow(
                title="Declared Capabilities",
                subtitle=", ".join(m.capabilities) if m.capabilities else "(none)",
            )
            row.add_row(caps_row)
            meta_row = Adw.ActionRow(title="Plugin ID &amp; Author", subtitle=f"{m.id} · {m.author}")
            row.add_row(meta_row)
            self.plugins_list.append(row)

    def _on_toggle_plugin(self, plugin_id: str, enabled: bool) -> None:
        self.runtime.plugins.set_enabled(plugin_id, enabled)
        self._on_toast(f"Plugin '{plugin_id}' {'enabled' if enabled else 'disabled'}")
