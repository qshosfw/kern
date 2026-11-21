"""Launch options shared by the CLI, tests, headless runtime and GTK application."""

from __future__ import annotations

import dataclasses


@dataclasses.dataclass(frozen=True)
class DockOptions:
    port: str | None = None          # Serial port, tcp://host:port, "demo", or None to auto-detect
    baud: int = 115200
    model: str | None = None         # Pinned handset mockup key
    palette: str | None = None       # Pinned LCD palette key
    headless: bool = False           # Switch Kern OS radios to headless on connect
    mini: bool = False               # Launch directly in compact mini-dock mode
    hardened: bool | None = None     # Override persisted hardened mode on startup
    no_tray: bool = False            # Disable system tray integration
    no_ipc: bool = False             # Disable local IPC / D-Bus control server
    no_plugins: bool = False         # Safe mode: skip loading plugins
    start_hidden: bool = False       # Start minimized in the system tray
