"""Kern Dock — Modular, Declarative, Hardened GTK4 + Libadwaita Hardware Companion for Kern OS."""

from __future__ import annotations

import sys
from pathlib import Path

__version__ = "2.0.0"
APP_ID = "org.kernos.Dock"
APP_NAME = "Kern Dock"

PACKAGE_DIR = Path(__file__).resolve().parent
DOCK_ROOT = PACKAGE_DIR.parent.parent
REPO_ROOT = DOCK_ROOT.parent
ASSETS_DIR = DOCK_ROOT / "assets"
WORKSPACE_PLUGINS_DIR = DOCK_ROOT / "plugins"
TOOLS_DIR = REPO_ROOT / "tools"

# Ensure Kern host tools (rigwire, kernfs_mount, kfw, flasher) are importable cleanly.
if TOOLS_DIR.is_dir() and str(TOOLS_DIR) not in sys.path:
    sys.path.insert(0, str(TOOLS_DIR))
