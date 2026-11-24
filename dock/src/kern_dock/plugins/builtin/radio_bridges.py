"""Built-in Plugin: Bridges & Local Integrations (HTTP API, WebDAV, IPC, rigctld, KISS, TCP, OpenOCD GDB)."""

from __future__ import annotations

from typing import Any

from ...bridges import GdbSwdBridge, HttpApiBridge, KissTncBridge, RigctldBridge, TcpCommandBridge
from ...device.hardware import available_hw_tools, discover_swd_probes
from ..api import PluginContext, PluginManifest


class RadioBridgesPlugin:
    manifest = PluginManifest(
        id="radio-bridges",
        name="Bridges & Integrations",
        version="2.0.0",
        description="Local HTTP JSON/LCD API, WebDAV drive, IPC/D-Bus, Hamlib rigctld, KISS TNC, TCP Command & OpenOCD SWD GDB bridges.",
        capabilities=("ui.page", "radio.read", "radio.control"),
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
            subtitle="HTTP API, WebDAV, Hamlib rigctld, KISS TNC, TCP & OpenOCD GDB",
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

    def _refresh_ui(self) -> None:
        try:
            from gi.repository import GLib

            GLib.idle_add(self._apply_ui)
        except Exception:
            pass

    def _apply_ui(self) -> bool:
        if not self._widgets or self._ctx is None:
            return False
        self._updating = True
        try:
            if self.http_api and "http_row" in self._widgets:
                r = self._widgets["http_row"]
                self._widgets["http_sw"].set_active(self.http_api.running)
                r.set_subtitle(
                    f"Listening on {self.http_api.base_url} · {self.http_api.request_count} reqs (/api/status, /api/screen.png)"
                    if self.http_api.running
                    else "HTTP 127.0.0.1:8085 · OBS screen overlay, Home Assistant & JSON status"
                )
            if "dav_row" in self._widgets:
                lk = self._ctx.link
                mounted = lk.dav_server is not None
                self._widgets["dav_sw"].set_active(mounted)
                self._widgets["dav_sw"].set_sensitive(lk.connected and lk.link_mode == "rigwire")
                self._widgets["dav_row"].set_subtitle(
                    f"Mounted at {lk.dav_open_target or f'dav://127.0.0.1:{lk.dav_port}/'}"
                    if mounted
                    else "Expose KernFS (/user, /pref, /apps, /sys) as a local WebDAV drive"
                )
            if self.rigctld and "rig_row" in self._widgets:
                self._widgets["rig_sw"].set_active(self.rigctld.running)
                self._widgets["rig_row"].set_subtitle(
                    f"Listening on 127.0.0.1:{self.rigctld.port} · {self.rigctld.client_count} client(s)"
                    if self.rigctld.running
                    else "TCP 127.0.0.1:4532 · WSJT-X, Gpredict, FLRig, CQRLog, SDR++"
                )
            if self.kiss and "kiss_row" in self._widgets:
                self._widgets["kiss_sw"].set_active(self.kiss.running)
                self._widgets["kiss_row"].set_subtitle(
                    f"Listening on 127.0.0.1:{self.kiss.port} · {self.kiss.client_count} client(s) · TX {self.kiss.tx_count} / RX {self.kiss.rx_count}"
                    if self.kiss.running
                    else "TCP 127.0.0.1:8001 · APRS, Xastir, YAAC, Pat, Winlink"
                )
            if self.tcp_cmd and "tcp_row" in self._widgets:
                self._widgets["tcp_sw"].set_active(self.tcp_cmd.running)
                self._widgets["tcp_row"].set_subtitle(
                    f"Listening on 127.0.0.1:{self.tcp_cmd.port} · {self.tcp_cmd.client_count} client(s) · {self.tcp_cmd.command_count} cmds"
                    if self.tcp_cmd.running
                    else "TCP 127.0.0.1:4447 · Line-based RigWire command bridge for netcat & scripts"
                )
            if self.gdb_swd and "gdb_row" in self._widgets:
                probes = discover_swd_probes()
                has_ocd = bool(available_hw_tools().get("openocd"))
                self._widgets["gdb_sw"].set_active(self.gdb_swd.running)
                self._widgets["gdb_sw"].set_sensitive(self.gdb_swd.running or (bool(probes) and has_ocd))
                if self.gdb_swd.running:
                    self._widgets["gdb_row"].set_subtitle(
                        f"GDB :{self.gdb_swd.gdb_port} · Telnet :{self.gdb_swd.telnet_port} · {self.gdb_swd.active_probe_name}"
                    )
                elif not has_ocd:
                    self._widgets["gdb_row"].set_subtitle("Install openocd to expose an SWD GDB server")
                elif not probes:
                    self._widgets["gdb_row"].set_subtitle("Connect a CMSIS-DAP, ST-Link, or J-Link SWD probe")
                else:
                    self._widgets["gdb_row"].set_subtitle(f"Ready on {probes[0].name} · TCP 127.0.0.1:3333 (works offline)")
        finally:
            self._updating = False
        return False

    def _toggle_http(self, active: bool) -> None:
        if self._updating or self.http_api is None:
            return
        try:
            if active:
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

    def _toggle_kiss(self, active: bool) -> None:
        if self._updating or self.kiss is None:
            return
        try:
            if active:
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

    def _toggle_rigctld(self, active: bool) -> None:
        if self._updating or self.rigctld is None:
            return
        try:
            if active:
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
                p = self.tcp_cmd.start()
                if self._ctx:
                    self._ctx.toast(f"TCP Command bridge listening on 127.0.0.1:{p}")
            else:
                self.tcp_cmd.stop()
                if self._ctx:
                    self._ctx.toast("TCP Command bridge stopped")
        except Exception as exc:
            if self._ctx:
                self._ctx.toast(f"TCP Command error: {exc}")
        self._refresh_ui()

    def _toggle_gdb(self, active: bool) -> None:
        if self._updating or self.gdb_swd is None:
            return
        try:
            if active:
                probes = discover_swd_probes()
                if not probes:
                    raise RuntimeError("No SWD probe detected")
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
        from gi.repository import Adw, Gtk

        page = Adw.PreferencesPage()

        def _make_service_row(
            key: str,
            title: str,
            subtitle: str,
            on_toggle: Any,
            *,
            copy_cmd: Any = None,
            copy_tip: str = "Copy endpoint",
        ) -> Adw.ActionRow:
            row = Adw.ActionRow(title=title, subtitle=subtitle)
            if copy_cmd is not None:
                cb = Gtk.Button(icon_name="edit-copy-symbolic", valign=Gtk.Align.CENTER, tooltip_text=copy_tip)
                cb.add_css_class("flat")
                cb.connect("clicked", lambda _b: copy_cmd())
                row.add_suffix(cb)
            sw = Gtk.Switch(valign=Gtk.Align.CENTER)
            sw.connect("notify::active", lambda w, _p: on_toggle(w.get_active()))
            row.add_suffix(sw)
            row.set_activatable_widget(sw)
            self._widgets[f"{key}_row"] = row
            self._widgets[f"{key}_sw"] = sw
            return row

        # === 1. LOCAL API & DESKTOP INTEGRATIONS ===
        grp_api = Adw.PreferencesGroup(
            title="Local API &amp; Desktop Services",
            description="Expose live radio state, LCD screenshots and KernFS storage to local apps and overlays",
        )
        grp_api.add(
            _make_service_row(
                "http",
                "HTTP JSON &amp; Live LCD API",
                "HTTP 127.0.0.1:8085 · OBS screen overlay, Home Assistant &amp; JSON status",
                self._toggle_http,
                copy_cmd=lambda: self._copy_text(f"http://127.0.0.1:{self.http_api.port if self.http_api else 8085}/api/screen.png", "Copied LCD URL"),
                copy_tip="Copy /api/screen.png URL",
            )
        )
        grp_api.add(
            _make_service_row(
                "dav",
                "KernFS WebDAV Drive",
                "Expose KernFS (/user, /pref, /apps, /sys) as a local WebDAV drive",
                self._toggle_dav,
                copy_cmd=lambda: self._copy_text(
                    self._ctx.link.dav_open_target or f"dav://127.0.0.1:{self._ctx.link.dav_port}/" if (self._ctx and self._ctx.link.dav_server) else "dav://127.0.0.1/",
                    "Copied WebDAV URL",
                ),
                copy_tip="Copy WebDAV URL",
            )
        )
        ipc_row = Adw.ActionRow(
            title="Local IPC &amp; CLI Control",
            subtitle="./kern dock ctl status · org.kernos.Dock D-Bus &amp; Unix socket",
        )
        ipc_copy = Gtk.Button(icon_name="edit-copy-symbolic", valign=Gtk.Align.CENTER, tooltip_text="Copy CLI status command")
        ipc_copy.add_css_class("flat")
        ipc_copy.connect("clicked", lambda _b: self._copy_text("./kern dock ctl status", "Copied CLI"))
        ipc_row.add_suffix(ipc_copy)
        grp_api.add(ipc_row)
        page.add(grp_api)

        # === 2. PROTOCOL BRIDGES ===
        grp_proto = Adw.PreferencesGroup(
            title="Protocol Bridges",
            description="Connect external amateur radio software and automation scripts over localhost TCP",
        )
        grp_proto.add(
            _make_service_row(
                "rig",
                "Hamlib rigctld (TCP)",
                "TCP 127.0.0.1:4532 · WSJT-X, Gpredict, FLRig, CQRLog, SDR++",
                self._toggle_rigctld,
                copy_cmd=lambda: self._copy_text(f"rigctl -m 2 -r 127.0.0.1:{self.rigctld.port if self.rigctld else 4532}", "Copied rigctl command"),
                copy_tip="Copy rigctl command",
            )
        )
        grp_proto.add(
            _make_service_row(
                "kiss",
                "KISS TNC (TCP)",
                "TCP 127.0.0.1:8001 · APRS, Xastir, YAAC, Pat, Winlink",
                self._toggle_kiss,
                copy_cmd=lambda: self._copy_text(f"127.0.0.1:{self.kiss.port if self.kiss else 8001}", "Copied KISS endpoint"),
                copy_tip="Copy KISS TCP host:port",
            )
        )
        grp_proto.add(
            _make_service_row(
                "tcp",
                "TCP RigWire Console",
                "TCP 127.0.0.1:4447 · Line-based RigWire command bridge for netcat &amp; scripts",
                self._toggle_tcp_cmd,
                copy_cmd=lambda: self._copy_text(f"nc 127.0.0.1 {self.tcp_cmd.port if self.tcp_cmd else 4447}", "Copied netcat command"),
                copy_tip="Copy netcat command",
            )
        )
        page.add(grp_proto)

        # === 3. HARDWARE & DEBUG SERVERS (WORKS OFFLINE) ===
        grp_dbg = Adw.PreferencesGroup(
            title="Hardware &amp; Debug Servers",
            description="SWD debug bridges that run directly against connected probes even when the radio is off",
        )
        grp_dbg.add(
            _make_service_row(
                "gdb",
                "OpenOCD GDB Server (SWD)",
                "TCP 127.0.0.1:3333 · Attach arm-none-eabi-gdb or VS Code Cortex-Debug",
                self._toggle_gdb,
                copy_cmd=lambda: self._copy_text(f'arm-none-eabi-gdb -ex "target extended-remote :3333"', "Copied GDB command"),
                copy_tip="Copy GDB attach command",
            )
        )
        page.add(grp_dbg)

        page.connect("map", lambda _w: self._apply_ui())
        self._apply_ui()
        return page
