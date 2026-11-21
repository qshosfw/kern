"""Built-in first-party Kern Dock plugins."""

from __future__ import annotations

from .band_presets import BandPresetsPlugin
from .radio_bridges import RadioBridgesPlugin
from .table_inspector import TableInspectorPlugin
from .telemetry_monitor import TelemetryMonitorPlugin

BUILTIN_PLUGINS = (
    BandPresetsPlugin,
    RadioBridgesPlugin,
    TelemetryMonitorPlugin,
    TableInspectorPlugin,
)

__all__ = [
    "BUILTIN_PLUGINS",
    "BandPresetsPlugin",
    "RadioBridgesPlugin",
    "TableInspectorPlugin",
    "TelemetryMonitorPlugin",
]
