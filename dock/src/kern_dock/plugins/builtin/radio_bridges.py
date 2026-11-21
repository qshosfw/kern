"""Built-in Plugin: Standard Amateur Radio App Bridges (KISS TNC TCP & Hamlib rigctld TCP)."""

from __future__ import annotations

from typing import Any

from ...bridges import KissTncBridge, RigctldBridge
from ..api import PluginContext, PluginManifest


class RadioBridgesPlugin:
    manifest = PluginManifest(
        id="radio-bridges",
        name="Radio Bridges",
        version="2.0.0",
        description="Connect standard amateur radio apps via KISS TNC (APRS/Winlink/Pat) and Hamlib rigctld (WSJT-X/Gpredict/FLRig).",
        capabilities=("ui.page", "radio.read", "radio.control"),
        icon="network-server-symbolic",
        builtin=True,
    )

    def __init__(self) -> None:
        self._ctx: PluginContext | None = None
        self.kiss: KissTncBridge | None = None
        self.rigctld: RigctldBridge | None = None
        self._kiss_row: Any = None
        self._rig_row: Any = None

    def activate(self, ctx: PluginContext) -> None:
        self._ctx = ctx
        self.kiss = KissTncBridge(
            port=8001,
            on_tx_frame=lambda _f: self._refresh_ui(),
            on_change=self._refresh_ui,
        )
        self.rigctld = RigctldBridge(
            ctx.link,
            port=4532,
            on_change=self._refresh_ui,
        )
        ctx.register_page(
            "bridges",
            "Bridges",
            "network-server-symbolic",
            self._build_page,
            order=40,
            subtitle="KISS TNC (8001) & Hamlib rigctld (4532) TCP bridges",
        )

    def deactivate(self) -> None:
        if self.kiss:
            self.kiss.stop()
        if self.rigctld:
            self.rigctld.stop()
        self._ctx = None
        self._kiss_row = None
        self._rig_row = None

    def _refresh_ui(self) -> None:
        try:
            from gi.repository import GLib

            GLib.idle_add(self._apply_ui)
        except Exception:
            pass

    def _apply_ui(self) -> bool:
        if self._kiss_row is not None and self.kiss is not None:
            if self.kiss.running:
                self._kiss_row.set_subtitle(
                    f"Listening on 127.0.0.1:{self.kiss.port} · {self.kiss.client_count} client(s) · TX {self.kiss.tx_count} / RX {self.kiss.rx_count}"
                )
            else:
                self._kiss_row.set_subtitle("TCP 127.0.0.1:8001 · APRS, Xastir, YAAC, Pat, Winlink")
        if self._rig_row is not None and self.rigctld is not None:
            if self.rigctld.running:
                self._rig_row.set_subtitle(
                    f"Listening on 127.0.0.1:{self.rigctld.port} · {self.rigctld.client_count} client(s)"
                )
            else:
                self._rig_row.set_subtitle("TCP 127.0.0.1:4532 · WSJT-X, Gpredict, FLRig, CQRLog, SDR++")
        return False

    def _toggle_kiss(self, active: bool) -> None:
        if self.kiss is None:
            return
        try:
            if active:
                p = self.kiss.start()
                if self._ctx:
                    self._ctx.toast(f"KISS TNC bridge listening on 127.0.0.1:{p}")
            else:
                self.kiss.stop()
                if self._ctx:
                    self._ctx.toast("KISS TNC bridge stopped")
        except Exception as exc:
            if self._ctx:
                self._ctx.toast(f"KISS TNC error: {exc}")
        self._refresh_ui()

    def _toggle_rigctld(self, active: bool) -> None:
        if self.rigctld is None:
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

    def _build_page(self) -> Any:
        import gi

        gi.require_version("Gtk", "4.0")
        gi.require_version("Adw", "1")
        from gi.repository import Adw

        page = Adw.PreferencesPage()
        grp = Adw.PreferencesGroup(
            title="Standard Radio Bridges",
            description="Expose the connected radio to third-party amateur radio software over local TCP.",
        )

        self._kiss_row = Adw.SwitchRow(
            title="KISS TNC (TCP)",
            subtitle="TCP 127.0.0.1:8001 · APRS, Xastir, YAAC, Pat, Winlink",
            active=bool(self.kiss and self.kiss.running),
        )
        self._kiss_row.connect("notify::active", lambda w, _p: self._toggle_kiss(w.get_active()))
        grp.add(self._kiss_row)

        self._rig_row = Adw.SwitchRow(
            title="Hamlib rigctld (TCP)",
            subtitle="TCP 127.0.0.1:4532 · WSJT-X, Gpredict, FLRig, CQRLog, SDR++",
            active=bool(self.rigctld and self.rigctld.running),
        )
        self._rig_row.connect("notify::active", lambda w, _p: self._toggle_rigctld(w.get_active()))
        grp.add(self._rig_row)

        page.add(grp)
        self._apply_ui()
        return page
