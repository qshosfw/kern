"""Built-in Plugin: Bridges & Local Integrations (KISS TNC, Hamlib rigctld, HTTP API, WebDAV, TCP Console, OpenOCD GDB)."""

from __future__ import annotations

from typing import Any

from ...bridges import GdbSwdBridge, HttpApiBridge, KissTncBridge, RigctldBridge, TcpCommandBridge
from ...device.hardware import available_hw_tools, discover_swd_probes
from ..api import PluginContext, PluginManifest


def _build_test_aprs_ax25() -> bytes:
    """Build a valid AX.25 UI beacon frame (NOCALL>APKERN:>Kern Dock KISS TNC test)."""

    def _encode_call(call: str, ssid: int = 0, *, last: bool = False) -> bytes:
        padded = call.upper().ljust(6)[:6]
        out = bytearray((ord(ch) << 1) for ch in padded)
        out.append(((ssid & 0x0F) << 1) | 0x60 | (0x01 if last else 0x00))
        return bytes(out)

    return (
        _encode_call("APKERN", 0, last=False)
        + _encode_call("NOCALL", 0, last=True)
        + b"\x03\xF0>Kern Dock KISS TNC test frame"
    )


class RadioBridgesPlugin:
    manifest = PluginManifest(
        id="radio-bridges",
        name="Bridges & Integrations",
        version="2.1.0",
        description="KISS TNC (APRS/Winlink), Hamlib rigctld, Local HTTP JSON/LCD API, KernFS WebDAV, TCP Console & OpenOCD SWD GDB.",
        capabilities=("events.subscribe", "ui.page", "radio.read", "radio.control"),
        icon="network-server-symbolic",
        builtin=True,
    )

    def __init__(self) -> None:
        self._ctx: PluginContext | None = None
        self.http_api: HttpApiBridge | None = None
        self.kiss: KissTncBridge | None = None
        self.rigctld: RigctldBridge | None = None
        self.tcp_cmd: TcpCommandBridge | None = None
        self.gdb_swd: GdbSwdBridge | None = None
        self._widgets: dict[str, Any] = {}
        self._updating = False

    def activate(self, ctx: PluginContext) -> None:
        self._ctx = ctx
        self.http_api = HttpApiBridge(ctx._runtime, port=8085, on_change=self._refresh_ui)
        self.kiss = KissTncBridge(port=8001, on_tx_frame=lambda _f: self._refresh_ui(), on_change=self._refresh_ui)
        self.rigctld = RigctldBridge(ctx.link, port=4532, on_change=self._refresh_ui)
        self.tcp_cmd = TcpCommandBridge(ctx.link, port=4447, on_change=self._refresh_ui)
        self.gdb_swd = GdbSwdBridge(gdb_port=3333, telnet_port=4444, on_change=self._refresh_ui)

        ctx.subscribe("dav.changed", lambda _e: self._refresh_ui(), main_thread=True)
        ctx.subscribe("link.connected", lambda _e: self._refresh_ui(), main_thread=True)
        ctx.subscribe("link.disconnected", lambda _e: self._refresh_ui(), main_thread=True)

        ctx.register_page(
            "bridges",
            "Integrations",
            "network-server-symbolic",
            self._build_page,
            order=46,
            subtitle="KISS TNC, Hamlib rigctld, HTTP API, WebDAV, TCP Console & OpenOCD GDB",
        )

    def deactivate(self) -> None:
        for srv in (self.http_api, self.kiss, self.rigctld, self.tcp_cmd, self.gdb_swd):
            if srv is not None:
                srv.stop()
        self._ctx = None
        self._widgets.clear()

    def _copy_text(self, text: str, label: str = "Copied") -> None:
        try:
            from gi.repository import Gdk

            Gdk.Display.get_default().get_clipboard().set(text)
            if self._ctx:
                self._ctx.toast(f"{label}: {text}")
        except Exception:
            pass

    def _open_uri(self, uri: str) -> None:
        try:
            from gi.repository import Gio

            Gio.AppInfo.launch_default_for_uri(uri, None)
        except Exception as exc:
            if self._ctx:
                self._ctx.toast(f"Could not open {uri}: {exc}")

    def _refresh_ui(self) -> None:
        try:
            from gi.repository import GLib

            GLib.idle_add(self._apply_ui)
        except Exception:
            pass

    def _apply_ui(self) -> bool:
        if not self._widgets or self._ctx is None:
            return False
        from gi.repository import GLib

        esc = GLib.markup_escape_text
        self._updating = True
        try:
            if self.kiss and "kiss_row" in self._widgets:
                on = self.kiss.running
                self._widgets["kiss_sw"].set_active(on)
                self._widgets["kiss_port"].set_sensitive(not on)
                self._widgets["kiss_test_btn"].set_sensitive(on)
                self._widgets["kiss_row"].set_subtitle(
                    esc(f"Running on 127.0.0.1:{self.kiss.port} · {self.kiss.client_count} client(s) · TX {self.kiss.tx_count} / RX {self.kiss.rx_count}")
                    if on
                    else esc(f"TCP 127.0.0.1:{self.kiss.port} · APRS, Xastir, YAAC, Pat, Winlink")
                )
                self._widgets["kiss_cmd"].set_subtitle(esc(f"127.0.0.1:{self.kiss.port}"))

            if self.rigctld and "rig_row" in self._widgets:
                on = self.rigctld.running
                self._widgets["rig_sw"].set_active(on)
                self._widgets["rig_port"].set_sensitive(not on)
                self._widgets["rig_row"].set_subtitle(
                    esc(f"Running on 127.0.0.1:{self.rigctld.port} · {self.rigctld.client_count} client(s)")
                    if on
                    else esc(f"TCP 127.0.0.1:{self.rigctld.port} · WSJT-X, Gpredict, FLRig, CQRLog, SDR++")
                )
                self._widgets["rig_cmd"].set_subtitle(esc(f"rigctl -m 2 -r 127.0.0.1:{self.rigctld.port}"))

            if self.tcp_cmd and "tcp_row" in self._widgets:
                on = self.tcp_cmd.running
                self._widgets["tcp_sw"].set_active(on)
                self._widgets["tcp_port"].set_sensitive(not on)
                self._widgets["tcp_row"].set_subtitle(
                    esc(f"Running on 127.0.0.1:{self.tcp_cmd.port} · {self.tcp_cmd.client_count} client(s) · {self.tcp_cmd.command_count} cmds")
                    if on
                    else esc(f"TCP 127.0.0.1:{self.tcp_cmd.port} · Line-based RigWire command bridge for netcat & scripts")
                )
                self._widgets["tcp_cmd"].set_subtitle(esc(f"nc 127.0.0.1 {self.tcp_cmd.port}"))

            if self.http_api and "http_row" in self._widgets:
                on = self.http_api.running
                self._widgets["http_sw"].set_active(on)
                self._widgets["http_port"].set_sensitive(not on)
                self._widgets["http_open_btn"].set_sensitive(on)
                self._widgets["http_row"].set_subtitle(
                    esc(f"Running on {self.http_api.base_url} · {self.http_api.request_count} requests")
                    if on
                    else esc(f"HTTP 127.0.0.1:{self.http_api.port} · OBS LCD overlay, Home Assistant & JSON status")
                )
                self._widgets["http_cmd"].set_subtitle(esc(f"http://127.0.0.1:{self.http_api.port}/api/screen.png"))

            if "dav_row" in self._widgets:
                lk = self._ctx.link
                mounted = lk.dav_server is not None
                self._widgets["dav_sw"].set_active(mounted)
                self._widgets["dav_sw"].set_sensitive(mounted or (lk.connected and lk.link_mode == "rigwire"))
                self._widgets["dav_open_btn"].set_sensitive(mounted)
                url = lk.dav_open_target or (f"dav://127.0.0.1:{lk.dav_port}/" if mounted else "Requires connected RigWire radio")
                self._widgets["dav_row"].set_subtitle(
                    esc(f"Mounted at {url}")
                    if mounted
                    else esc("Expose KernFS (/user, /pref, /apps, /sys) as a local WebDAV drive")
                )
                self._widgets["dav_cmd"].set_subtitle(esc(url))

            if self.gdb_swd and "gdb_row" in self._widgets:
                probes = discover_swd_probes()
                has_ocd = bool(available_hw_tools().get("openocd"))
                on = self.gdb_swd.running
                self._widgets["gdb_sw"].set_active(on)
                self._widgets["gdb_port"].set_sensitive(not on)
                self._widgets["gdb_sw"].set_sensitive(on or (bool(probes) and has_ocd))
                if on:
                    sub = f"Running · GDB :{self.gdb_swd.gdb_port} · Telnet :{self.gdb_swd.telnet_port} · {self.gdb_swd.active_probe_name}"
                elif not has_ocd:
                    sub = "Install openocd to expose an SWD GDB server"
                elif not probes:
                    sub = "Connect a CMSIS-DAP, ST-Link, or J-Link SWD probe (works offline)"
                else:
                    sub = f"Ready on {probes[0].name} · TCP 127.0.0.1:{self.gdb_swd.gdb_port} (works offline)"
                self._widgets["gdb_row"].set_subtitle(esc(sub))
                self._widgets["gdb_cmd"].set_subtitle(
                    esc(f'arm-none-eabi-gdb -ex "target extended-remote :{self.gdb_swd.gdb_port}"')
                )
        finally:
            self._updating = False
        return False

    def _toggle_kiss(self, active: bool) -> None:
        if self._updating or self.kiss is None:
            return
        try:
            if active:
                if "kiss_port" in self._widgets:
                    self.kiss.port = int(self._widgets["kiss_port"].get_value())
                p = self.kiss.start()
                if self._ctx:
                    self._ctx.toast(f"KISS TNC listening on 127.0.0.1:{p}")
            else:
                self.kiss.stop()
                if self._ctx:
                    self._ctx.toast("KISS TNC stopped")
        except Exception as exc:
            if self._ctx:
                self._ctx.toast(f"KISS TNC error: {exc}")
        self._refresh_ui()

    def _send_kiss_test(self) -> None:
        if self.kiss is None or not self.kiss.running:
            return
        self.kiss.publish_rx_frame(_build_test_aprs_ax25())
        if self._ctx:
            self._ctx.toast("Sent test APRS AX.25 frame to connected KISS clients")
        self._refresh_ui()

    def _toggle_rigctld(self, active: bool) -> None:
        if self._updating or self.rigctld is None:
            return
        try:
            if active:
                if "rig_port" in self._widgets:
                    self.rigctld.port = int(self._widgets["rig_port"].get_value())
                p = self.rigctld.start()
                if self._ctx:
                    self._ctx.toast(f"Hamlib rigctld listening on 127.0.0.1:{p}")
            else:
                self.rigctld.stop()
                if self._ctx:
                    self._ctx.toast("Hamlib rigctld stopped")
        except Exception as exc:
            if self._ctx:
                self._ctx.toast(f"rigctld error: {exc}")
        self._refresh_ui()

    def _toggle_tcp_cmd(self, active: bool) -> None:
        if self._updating or self.tcp_cmd is None:
            return
        try:
            if active:
                if "tcp_port" in self._widgets:
                    self.tcp_cmd.port = int(self._widgets["tcp_port"].get_value())
                p = self.tcp_cmd.start()
                if self._ctx:
                    self._ctx.toast(f"TCP RigWire Console listening on 127.0.0.1:{p}")
            else:
                self.tcp_cmd.stop()
                if self._ctx:
                    self._ctx.toast("TCP RigWire Console stopped")
        except Exception as exc:
            if self._ctx:
                self._ctx.toast(f"TCP Console error: {exc}")
        self._refresh_ui()

    def _toggle_http(self, active: bool) -> None:
        if self._updating or self.http_api is None:
            return
        try:
            if active:
                if "http_port" in self._widgets:
                    self.http_api.port = int(self._widgets["http_port"].get_value())
                p = self.http_api.start()
                if self._ctx:
                    self._ctx.toast(f"HTTP API listening on http://127.0.0.1:{p}/api/status")
            else:
                self.http_api.stop()
                if self._ctx:
                    self._ctx.toast("HTTP API stopped")
        except Exception as exc:
            if self._ctx:
                self._ctx.toast(f"HTTP API error: {exc}")
        self._refresh_ui()

    def _toggle_dav(self, active: bool) -> None:
        if self._updating or self._ctx is None:
            return
        lk = self._ctx.link
        try:
            if active:
                p = lk.start_dav(http_port=0, read_only=self._ctx._runtime.security.policy.read_only)
                self._ctx.toast(f"WebDAV mounted on dav://127.0.0.1:{p}/")
            else:
                lk.stop_dav()
                self._ctx.toast("WebDAV stopped")
        except Exception as exc:
            self._ctx.toast(f"WebDAV error: {exc}")
        self._refresh_ui()

    def _toggle_gdb(self, active: bool) -> None:
        if self._updating or self.gdb_swd is None:
            return
        try:
            if active:
                probes = discover_swd_probes()
                if not probes:
                    raise RuntimeError("No SWD probe detected")
                if "gdb_port" in self._widgets:
                    self.gdb_swd.gdb_port = int(self._widgets["gdb_port"].get_value())
                p = self.gdb_swd.start(probes[0])
                if self._ctx:
                    self._ctx.toast(f"OpenOCD GDB server listening on 127.0.0.1:{p}")
            else:
                self.gdb_swd.stop()
                if self._ctx:
                    self._ctx.toast("OpenOCD GDB server stopped")
        except Exception as exc:
            if self._ctx:
                self._ctx.toast(f"OpenOCD GDB error: {exc}")
        self._refresh_ui()

    def _build_page(self) -> Any:
        import gi

        gi.require_version("Gtk", "4.0")
        gi.require_version("Adw", "1")
        from gi.repository import Adw, GLib, Gtk

        page = Adw.PreferencesPage()

        def _make_bridge_expander(
            key: str,
            icon_name: str,
            title: str,
            subtitle: str,
            on_toggle: Any,
            *,
            port: int | None = None,
            cmd_title: str = "Endpoint",
            cmd_value: str = "",
            copy_label: str = "Copied",
        ) -> Adw.ExpanderRow:
            row = Adw.ExpanderRow(title=title, subtitle=GLib.markup_escape_text(subtitle))
            row.add_prefix(Gtk.Image.new_from_icon_name(icon_name))
            sw = Gtk.Switch(valign=Gtk.Align.CENTER)
            sw.connect("notify::active", lambda w, _p: on_toggle(w.get_active()))
            row.add_suffix(sw)
            self._widgets[f"{key}_row"] = row
            self._widgets[f"{key}_sw"] = sw

            if port is not None:
                p_row = Adw.SpinRow.new_with_range(1024, 65535, 1)
                p_row.set_title("TCP Port")
                p_row.set_subtitle("Localhost bind port (127.0.0.1)")
                p_row.set_value(float(port))
                row.add_row(p_row)
                self._widgets[f"{key}_port"] = p_row

            cmd_row = Adw.ActionRow(title=cmd_title, subtitle=GLib.markup_escape_text(cmd_value), subtitle_selectable=True)
            cmd_row.add_css_class("property")
            copy_b = Gtk.Button(icon_name="edit-copy-symbolic", valign=Gtk.Align.CENTER, tooltip_text="Copy to clipboard")
            copy_b.add_css_class("flat")
            copy_b.connect("clicked", lambda _b, r=cmd_row, lbl=copy_label: self._copy_text(r.get_subtitle() or "", lbl))
            cmd_row.add_suffix(copy_b)
            row.add_row(cmd_row)
            self._widgets[f"{key}_cmd"] = cmd_row
            return row

        # === 1. AMATEUR RADIO PROTOCOL BRIDGES ===
        grp_proto = Adw.PreferencesGroup(
            title="Radio Protocol Bridges",
            description="Connect APRS, CAT control, and automation clients over localhost TCP",
        )

        kiss_exp = _make_bridge_expander(
            "kiss",
            "network-wireless-symbolic",
            "KISS TNC (TCP)",
            "TCP 127.0.0.1:8001 · APRS, Xastir, YAAC, Pat, Winlink",
            self._toggle_kiss,
            port=self.kiss.port if self.kiss else 8001,
            cmd_title="KISS TCP Host",
            cmd_value="127.0.0.1:8001",
            copy_label="Copied KISS endpoint",
        )
        test_btn = Gtk.Button(label="Send Test Frame", valign=Gtk.Align.CENTER, sensitive=False)
        test_btn.connect("clicked", lambda _b: self._send_kiss_test())
        kiss_test_row = Adw.ActionRow(title="Loopback Verification", subtitle="Broadcast a test AX.25 APRS UI frame to connected KISS clients")
        kiss_test_row.add_suffix(test_btn)
        kiss_exp.add_row(kiss_test_row)
        self._widgets["kiss_test_btn"] = test_btn
        grp_proto.add(kiss_exp)

        rig_exp = _make_bridge_expander(
            "rig",
            "audio-input-microphone-symbolic",
            "Hamlib rigctld (TCP)",
            "TCP 127.0.0.1:4532 · WSJT-X, Gpredict, FLRig, CQRLog, SDR++",
            self._toggle_rigctld,
            port=self.rigctld.port if self.rigctld else 4532,
            cmd_title="Hamlib Command",
            cmd_value="rigctl -m 2 -r 127.0.0.1:4532",
            copy_label="Copied rigctl command",
        )
        grp_proto.add(rig_exp)

        tcp_exp = _make_bridge_expander(
            "tcp",
            "utilities-terminal-symbolic",
            "TCP RigWire Console",
            "TCP 127.0.0.1:4447 · Line-based RigWire command bridge for netcat & scripts",
            self._toggle_tcp_cmd,
            port=self.tcp_cmd.port if self.tcp_cmd else 4447,
            cmd_title="Connect Command",
            cmd_value="nc 127.0.0.1 4447",
            copy_label="Copied netcat command",
        )
        grp_proto.add(tcp_exp)
        page.add(grp_proto)

        # === 2. LOCAL API & DESKTOP SERVICES ===
        grp_api = Adw.PreferencesGroup(
            title="Local API &amp; Desktop Services",
            description="Expose live radio state, LCD screenshots, and KernFS storage to local apps",
        )

        http_exp = _make_bridge_expander(
            "http",
            "network-server-symbolic",
            "HTTP JSON &amp; Live LCD API",
            "HTTP 127.0.0.1:8085 · OBS LCD overlay, Home Assistant & JSON status",
            self._toggle_http,
            port=self.http_api.port if self.http_api else 8085,
            cmd_title="Live LCD Image URL",
            cmd_value="http://127.0.0.1:8085/api/screen.png",
            copy_label="Copied LCD URL",
        )
        http_open = Gtk.Button(label="Open Status", valign=Gtk.Align.CENTER, sensitive=False)
        http_open.connect("clicked", lambda _b: self._open_uri(f"{self.http_api.base_url}/api/status") if self.http_api else None)
        http_act = Adw.ActionRow(title="Endpoints", subtitle="/api/status · /api/screen.png · /api/key")
        http_act.add_suffix(http_open)
        http_exp.add_row(http_act)
        self._widgets["http_open_btn"] = http_open
        grp_api.add(http_exp)

        dav_exp = _make_bridge_expander(
            "dav",
            "folder-remote-symbolic",
            "KernFS WebDAV Drive",
            "Expose KernFS (/user, /pref, /apps, /sys) as a local WebDAV drive",
            self._toggle_dav,
            port=None,
            cmd_title="WebDAV Mount URL",
            cmd_value="Requires connected RigWire radio",
            copy_label="Copied WebDAV URL",
        )
        dav_open = Gtk.Button(label="Open Folder", valign=Gtk.Align.CENTER, sensitive=False)
        dav_open.connect(
            "clicked",
            lambda _b: self._open_uri(self._ctx.link.dav_open_target or f"dav://127.0.0.1:{self._ctx.link.dav_port}/")
            if (self._ctx and self._ctx.link.dav_server)
            else None,
        )
        dav_act = Adw.ActionRow(title="File Manager", subtitle="Open mounted radio filesystem in Nautilus / Dolphin")
        dav_act.add_suffix(dav_open)
        dav_exp.add_row(dav_act)
        self._widgets["dav_open_btn"] = dav_open
        grp_api.add(dav_exp)

        ipc_row = Adw.ActionRow(
            title="Local IPC &amp; CLI Control",
            subtitle="./kern dock ctl status · org.kernos.Dock D-Bus &amp; Unix socket",
        )
        ipc_row.add_prefix(Gtk.Image.new_from_icon_name("system-run-symbolic"))
        ipc_copy = Gtk.Button(icon_name="edit-copy-symbolic", valign=Gtk.Align.CENTER, tooltip_text="Copy CLI status command")
        ipc_copy.add_css_class("flat")
        ipc_copy.connect("clicked", lambda _b: self._copy_text("./kern dock ctl status", "Copied CLI"))
        ipc_row.add_suffix(ipc_copy)
        grp_api.add(ipc_row)
        page.add(grp_api)

        # === 3. HARDWARE & DEBUG SERVERS (WORKS OFFLINE) ===
        grp_dbg = Adw.PreferencesGroup(
            title="Hardware &amp; Debug Servers",
            description="SWD debug bridges that run directly against connected probes even when the radio is off",
        )
        gdb_exp = _make_bridge_expander(
            "gdb",
            "preferences-other-symbolic",
            "OpenOCD GDB Server (SWD)",
            "TCP 127.0.0.1:3333 · Attach arm-none-eabi-gdb or VS Code Cortex-Debug",
            self._toggle_gdb,
            port=self.gdb_swd.gdb_port if self.gdb_swd else 3333,
            cmd_title="GDB Attach Command",
            cmd_value='arm-none-eabi-gdb -ex "target extended-remote :3333"',
            copy_label="Copied GDB command",
        )
        grp_dbg.add(gdb_exp)
        page.add(grp_dbg)

        page.connect("map", lambda _w: self._apply_ui())
        self._apply_ui()
        return page
