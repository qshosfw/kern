"""OS integration layer: GTK4 radio icons, desktop installer, system tray, IPC/D-Bus & notifications."""

from __future__ import annotations

from .desktop import (
    desktop_integration_status,
    detect_terminal,
    install_desktop_integration,
    launch_external_terminal,
    open_path_in_os,
    uninstall_desktop_integration,
)
from .icons import make_radio_image, make_tray_argb_pixmap, register_icon_theme_paths
from .ipc import IpcClient, IpcServer, default_socket_path
from .notifications import DesktopNotifier
from .tray import SystemTrayService

__all__ = [
    "DesktopNotifier",
    "IpcClient",
    "IpcServer",
    "SystemTrayService",
    "default_socket_path",
    "desktop_integration_status",
    "detect_terminal",
    "install_desktop_integration",
    "launch_external_terminal",
    "make_radio_image",
    "make_tray_argb_pixmap",
    "open_path_in_os",
    "register_icon_theme_paths",
    "uninstall_desktop_integration",
]
