"""Device layer: USB discovery, RigWire link, demo radio, firmware, keypad bindings & T9."""

from __future__ import annotations

from .demo import make_demo_framebuffer, make_text_input_framebuffer
from .firmware import (
    build_kfw_backup_from_backend,
    flash_via_stock_bootloader,
    inspect_firmware_file,
    restore_kfw_backup_to_backend,
)
from .keys import APP_SHORTCUTS, DOCK_ACTIONS, KEY_BINDINGS, binding_index, keyval_label, resolve_binding
from .link import DockLink, REBOOT_BOOTLOADER, REBOOT_FIRMWARE, REBOOT_MODES, REBOOT_RECOVERY
from .t9 import T9Model, T9Planner, T9_GAP_MS, T9_MAX_LEN, simulate_text_input, text_to_keypad_sequence
from .usb import (
    DEFAULT_EMU_WIRE_URL,
    DetectedEmulator,
    DetectedPort,
    PortMonitor,
    VID_PID_KERN_CDC,
    VID_PID_STOCK_BL,
    classify_usb_mode,
    detect_hardware_model,
    discover_emulators,
)

__all__ = [
    "APP_SHORTCUTS",
    "DEFAULT_EMU_WIRE_URL",
    "DOCK_ACTIONS",
    "DetectedEmulator",
    "DetectedPort",
    "DockLink",
    "KEY_BINDINGS",
    "PortMonitor",
    "REBOOT_BOOTLOADER",
    "REBOOT_FIRMWARE",
    "REBOOT_MODES",
    "REBOOT_RECOVERY",
    "T9Model",
    "T9Planner",
    "T9_GAP_MS",
    "T9_MAX_LEN",
    "VID_PID_KERN_CDC",
    "VID_PID_STOCK_BL",
    "binding_index",
    "build_kfw_backup_from_backend",
    "classify_usb_mode",
    "detect_hardware_model",
    "discover_emulators",
    "flash_via_stock_bootloader",
    "inspect_firmware_file",
    "keyval_label",
    "make_demo_framebuffer",
    "make_text_input_framebuffer",
    "resolve_binding",
    "restore_kfw_backup_to_backend",
    "simulate_text_input",
    "text_to_keypad_sequence",
]
