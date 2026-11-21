"""Core kernel-style infrastructure for Kern Dock: events, config, security, registries."""

from __future__ import annotations

from .config import ConfigManager, ConfigSpec, CONFIG_SCHEMA
from .events import Event, EventBus
from .options import DockOptions
from .registry import (
    ActionSpec,
    CommandRegistry,
    PageRegistry,
    PageSpec,
    TrayItemSpec,
    TrayMenuRegistry,
)
from .security import AuditEntry, SecurityController, SecurityError, SecurityProfile

__all__ = [
    "ActionSpec",
    "AuditEntry",
    "CONFIG_SCHEMA",
    "CommandRegistry",
    "ConfigManager",
    "ConfigSpec",
    "DockOptions",
    "Event",
    "EventBus",
    "PageRegistry",
    "PageSpec",
    "SecurityController",
    "SecurityError",
    "SecurityProfile",
    "TrayItemSpec",
    "TrayMenuRegistry",
]
