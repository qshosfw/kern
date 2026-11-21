"""Example drop-in Kern Dock plugin (auto-discovered from `dock/plugins/`).

Drop any `.py` file exporting `PLUGIN_CLASS` (or a class with `manifest = PluginManifest(...)`)
into `dock/plugins/` or `~/.config/kern-dock/plugins/` to extend Kern Dock declaratively.
"""

from __future__ import annotations

from kern_dock.core.events import Event
from kern_dock.plugins.api import PluginContext, PluginManifest


class ConnectionEventLoggerPlugin:
    manifest = PluginManifest(
        id="connection-notifier",
        name="Connection Audit Notifier",
        version="1.0.0",
        author="Kern Community",
        description="Example drop-in plugin subscribing to radio connect/disconnect events.",
        capabilities=("events.subscribe", "ui.action"),
        icon="dialog-information-symbolic",
    )

    def __init__(self) -> None:
        self._ctx: PluginContext | None = None
        self.events_seen: int = 0

    def activate(self, ctx: PluginContext) -> None:
        self._ctx = ctx
        ctx.subscribe("link.connected", self._on_event)
        ctx.subscribe("link.disconnected", self._on_event)
        ctx.register_action(
            "show-count",
            "Show Connection Event Count",
            lambda: ctx.toast(f"Connection Audit plugin saw {self.events_seen} link event(s)"),
            subtitle="Example action registered by dock/plugins/example_logger.py",
        )

    def deactivate(self) -> None:
        self._ctx = None

    def _on_event(self, _ev: Event) -> None:
        self.events_seen += 1


PLUGIN_CLASS = ConnectionEventLoggerPlugin
