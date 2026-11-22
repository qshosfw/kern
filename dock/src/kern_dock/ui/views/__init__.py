"""Modular GTK4 + Libadwaita 1.8 workspace views."""

from __future__ import annotations

from .apps import AppsView
from .backups import BackupsView
from .capture import CaptureView
from .extensions import ExtensionsView
from .files import FilesView
from .firmware import FirmwareView
from .hardware import HardwareView
from .overview import OverviewView
from .terminal import TerminalView

__all__ = [
    "AppsView",
    "BackupsView",
    "CaptureView",
    "ExtensionsView",
    "FilesView",
    "FirmwareView",
    "HardwareView",
    "OverviewView",
    "TerminalView",
]
