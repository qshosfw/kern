"""Built-in Plugin: Live Signal & Battery Telemetry Sparkline Monitor with CSV Export."""

from __future__ import annotations

import collections
import time
from pathlib import Path
from typing import Any

from ...core.events import Event
from ...render.paint import round_rect
from ..api import PluginContext, PluginManifest


class TelemetryMonitorPlugin:
    manifest = PluginManifest(
        id="telemetry-monitor",
        name="Telemetry Monitor",
        version="2.0.0",
        description="Scrolling RSSI signal sparkline, battery voltage history and CSV telemetry logger.",
        capabilities=("events.subscribe", "ui.page", "ui.action", "radio.read"),
        icon="utilities-system-monitor-symbolic",
        builtin=True,
    )

    def __init__(self) -> None:
        self._ctx: PluginContext | None = None
        self._samples: collections.deque[tuple[float, int, int, float]] = collections.deque(maxlen=180)
        self._area: Any = None
        self._stats_row: Any = None

    def activate(self, ctx: PluginContext) -> None:
        self._ctx = ctx
        ctx.subscribe("link.telemetry", self._on_telemetry, main_thread=True)
        ctx.register_action(
            "clear-history",
            "Clear Telemetry History",
            self._clear,
            subtitle="Reset RSSI & battery sparkline samples",
        )
        ctx.register_page(
            "telemetry",
            "Signal Monitor",
            "utilities-system-monitor-symbolic",
            self._build_page,
            order=20,
            subtitle="Live RSSI history, battery curve and link diagnostics",
        )

    def deactivate(self) -> None:
        self._ctx = None
        self._area = None
        self._stats_row = None

    def _clear(self) -> None:
        self._samples.clear()
        if self._area is not None:
            self._area.queue_draw()

    def _on_telemetry(self, ev: Event) -> None:
        data = ev.payload
        if not data.get("connected"):
            return
        rssi = int(data.get("radio", {}).get("rssi_dbm", -127))
        batt = int(data.get("telemetry", {}).get("battery_mv", 0))
        rtt = float(data.get("rtt_ms", 0.0))
        self._samples.append((time.time(), rssi, batt, rtt))
        if self._area is not None:
            self._area.queue_draw()
        if self._stats_row is not None and self._samples:
            rssis = [s[1] for s in self._samples]
            self._stats_row.set_subtitle(
                f"{len(self._samples)} samples · min {min(rssis)} dBm · max {max(rssis)} dBm · RTT {rtt:.1f} ms"
            )

    def _export_csv(self) -> None:
        if self._ctx is None or not self._samples:
            if self._ctx:
                self._ctx.toast("No telemetry samples captured yet")
            return
        dest = Path.home() / f"kern_telemetry_{time.strftime('%Y%m%d_%H%M%S')}.csv"
        lines = ["timestamp,rssi_dbm,battery_mv,rtt_ms"]
        for ts, rssi, batt, rtt in self._samples:
            lines.append(f"{ts:.3f},{rssi},{batt},{rtt:.2f}")
        dest.write_text("\n".join(lines) + "\n", encoding="utf-8")
        self._ctx.toast(f"Exported {len(self._samples)} samples to {dest.name}")

    def _draw_sparkline(self, _area: Any, cr: Any, w: int, h: int) -> None:
        round_rect(cr, 0, 0, w, h, 10)
        cr.set_source_rgb(0.08, 0.09, 0.12)
        cr.fill_preserve()
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.08)
        cr.set_line_width(1.0)
        cr.stroke()

        # Subtle S-unit gridlines
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.06)
        cr.set_line_width(0.8)
        for frac in (0.25, 0.5, 0.75):
            y = h * frac
            cr.move_to(8, y)
            cr.line_to(w - 8, y)
        cr.stroke()

        pts = list(self._samples)
        if len(pts) < 2:
            return
        pad = 10.0
        span_x = max(1.0, w - pad * 2)
        span_y = max(1.0, h - pad * 2)

        def _xy(i: int, rssi: int) -> tuple[float, float]:
            norm = max(0.0, min(1.0, (rssi + 127.0) / 80.0))
            return pad + (i / float(len(pts) - 1)) * span_x, pad + (1.0 - norm) * span_y

        cr.new_path()
        for i, (_, rssi, _, _) in enumerate(pts):
            px, py = _xy(i, rssi)
            cr.move_to(px, py) if i == 0 else cr.line_to(px, py)
        cr.set_source_rgba(0.22, 0.74, 0.97, 0.95)
        cr.set_line_width(2.0)
        cr.stroke()

    def _build_page(self) -> Any:
        import gi

        gi.require_version("Gtk", "4.0")
        gi.require_version("Adw", "1")
        from gi.repository import Adw, Gtk

        page = Adw.PreferencesPage()
        grp = Adw.PreferencesGroup(title="Live RSSI Signal Trace", description="Rolling 180-sample signal history from RigWire telemetry")
        self._area = Gtk.DrawingArea(content_height=150, hexpand=True)
        self._area.set_draw_func(self._draw_sparkline)
        grp.add(self._area)
        self._stats_row = Adw.ActionRow(title="Session Summary", subtitle="Waiting for telemetry…")
        grp.add(self._stats_row)
        export_row = Adw.ButtonRow(title="Export CSV to Home Folder", start_icon_name="document-save-symbolic")
        export_row.connect("activated", lambda _r: self._export_csv())
        grp.add(export_row)
        clear_row = Adw.ButtonRow(title="Clear Trace", start_icon_name="edit-clear-all-symbolic")
        clear_row.connect("activated", lambda _r: self._clear())
        grp.add(clear_row)
        page.add(grp)
        return page
