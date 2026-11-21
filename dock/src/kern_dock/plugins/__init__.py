"""Declarative, capability-gated Plugin API and discovery manager for Kern Dock."""

from __future__ import annotations

from .api import DockPlugin, PluginContext, PluginManifest, PluginRecord
from .manager import PluginManager

__all__ = [
    "DockPlugin",
    "PluginContext",
    "PluginManifest",
    "PluginManager",
    "PluginRecord",
]
