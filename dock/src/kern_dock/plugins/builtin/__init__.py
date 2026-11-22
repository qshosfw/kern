"""Built-in first-party Kern Dock plugins."""

from __future__ import annotations

from .radio_bridges import RadioBridgesPlugin
from .table_inspector import TableInspectorPlugin

BUILTIN_PLUGINS = (
    RadioBridgesPlugin,
    TableInspectorPlugin,
)

__all__ = [
    "BUILTIN_PLUGINS",
    "RadioBridgesPlugin",
    "TableInspectorPlugin",
]
