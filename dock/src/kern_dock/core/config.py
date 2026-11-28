"""Declarative, schema-validated persistent configuration stored under XDG_CONFIG_HOME."""

from __future__ import annotations

import dataclasses
import os
import tempfile
import threading
import tomllib
from pathlib import Path
from typing import Any, Callable


@dataclasses.dataclass(frozen=True)
class ConfigSpec:
    """Declarative descriptor for one configuration key."""

    key: str                  # Dotted key: "section.name"
    kind: str                 # "bool" | "int" | "float" | "str" | "list"
    default: Any
    title: str
    description: str = ""
    min_val: float | None = None
    max_val: float | None = None
    choices: tuple[str, ...] = ()


CONFIG_SCHEMA: tuple[ConfigSpec, ...] = (
    # Handset & LCD
    ConfigSpec("handset.model", "str", "auto", "Radio Model", "Handset chassis mockup or 'auto'"),
    ConfigSpec("lcd.palette", "str", "auto", "LCD Palette", "Color profile or 'auto' (follow model)"),
    ConfigSpec("lcd.follow_device", "bool", True, "Follow Device Display", "Mirror live backlight, contrast and inversion over RigWire"),
    ConfigSpec("lcd.backlight", "int", 8, "Backlight", "Backlight brightness level", 0, 10),
    ConfigSpec("lcd.contrast", "int", 13, "Contrast", "ST7565 contrast curve", 0, 15),
    ConfigSpec("lcd.ghosting", "bool", True, "STN Ghosting", "Liquid-crystal rise and decay"),
    ConfigSpec("lcd.pixel_grid", "bool", True, "Pixel Grid", "Hairline dividers between 128×64 dots"),
    ConfigSpec("lcd.backlight_glow", "bool", True, "Backlight Glow", "Edge-lit glow on LCD surround"),
    ConfigSpec("lcd.invert", "bool", False, "Invert LCD", "Swap active/inactive pixel polarity"),
    # Connection & Layout
    ConfigSpec("connection.auto_connect", "bool", True, "Auto-Connect USB", "Attach automatically when a compatible USB radio is plugged in"),
    ConfigSpec("connection.baud", "int", 115200, "Baud Rate", "Serial link baud rate", 9600, 921600),
    ConfigSpec("ui.show_handset", "bool", True, "Show Handset Stage", "Display interactive 3D radio dock pane"),
    ConfigSpec("ui.mini_mode", "bool", False, "Compact Mini-Dock", "Minimal floating handset mode"),
    ConfigSpec("ui.color_scheme", "str", "dark", "Color Scheme", "Preferred theme", choices=("system", "dark", "light")),
    ConfigSpec("ui.keyboard_bridge", "bool", True, "Keyboard Control", "Drive radio keypad from PC keyboard"),
    # OS Integration
    ConfigSpec("os.tray_enabled", "bool", True, "System Tray Icon", "Show live radio status icon in the desktop tray"),
    ConfigSpec("os.close_to_tray", "bool", False, "Minimize to Tray on Close", "Keep Dock running in the system tray when closing the window"),
    ConfigSpec("os.notifications", "bool", True, "Desktop Notifications", "Show OS notifications for hotplug, DFU and flash completion"),
    ConfigSpec("os.ipc_enabled", "bool", True, "Local IPC & D-Bus Control", "Expose org.kernos.Dock D-Bus & local socket for CLI/scripts"),
    ConfigSpec("os.terminal_cmd", "str", "auto", "External Terminal", "Preferred OS terminal emulator or 'auto'"),
    # Security & Hardening
    ConfigSpec("security.hardened", "bool", False, "Safe Mode", "Lock files read-only, block bootloader/flashing/restores and restrict IPC/plugins"),
    ConfigSpec("security.tx_relock_seconds", "int", 120, "TX Auto-Relock (s)", "Automatically disarm TX-over-USB after inactivity (0 disables)", 0, 3600),
    ConfigSpec("security.ipc_allow_control", "bool", True, "Allow IPC Control Commands", "Permit external CLI/D-Bus clients to send keys or tune (blocked in Safe Mode)"),
    # Plugins
    ConfigSpec("plugins.disabled", "list", [], "Disabled Plugins", "Plugin IDs disabled by user"),
)

_SCHEMA_BY_KEY: dict[str, ConfigSpec] = {s.key: s for s in CONFIG_SCHEMA}


def xdg_config_dir() -> Path:
    base = Path(os.environ.get("XDG_CONFIG_HOME") or (Path.home() / ".config"))
    return base / "kern-dock"


def xdg_runtime_dir() -> Path:
    base = Path(os.environ.get("XDG_RUNTIME_DIR") or tempfile.gettempdir())
    return base


def _coerce(spec: ConfigSpec, val: Any) -> Any:
    if spec.kind == "bool":
        return bool(val)
    if spec.kind == "int":
        try:
            iv = int(val)
        except (TypeError, ValueError):
            return spec.default
        if spec.min_val is not None:
            iv = max(int(spec.min_val), iv)
        if spec.max_val is not None:
            iv = min(int(spec.max_val), iv)
        return iv
    if spec.kind == "float":
        try:
            fv = float(val)
        except (TypeError, ValueError):
            return spec.default
        if spec.min_val is not None:
            fv = max(float(spec.min_val), fv)
        if spec.max_val is not None:
            fv = min(float(spec.max_val), fv)
        return fv
    if spec.kind == "str":
        sv = str(val) if val is not None else str(spec.default)
        if spec.choices and sv not in spec.choices:
            return spec.default
        return sv
    if spec.kind == "list":
        if isinstance(val, (list, tuple)):
            return [str(x) for x in val]
        return list(spec.default)
    return val


def _toml_literal(val: Any) -> str:
    if isinstance(val, bool):
        return "true" if val else "false"
    if isinstance(val, int):
        return str(val)
    if isinstance(val, float):
        return repr(val)
    if isinstance(val, str):
        esc = val.replace("\\", "\\\\").replace('"', '\\"').replace("\n", "\\n")
        return f'"{esc}"'
    if isinstance(val, list):
        return "[" + ", ".join(_toml_literal(x) for x in val) + "]"
    return '""'


class ConfigManager:
    """Thread-safe, schema-validated TOML configuration store with atomic persistence."""

    def __init__(self, path: Path | None = None, *, persist: bool = True) -> None:
        self.path = path or (xdg_config_dir() / "config.toml")
        self.persist = persist
        self._lock = threading.RLock()
        self._values: dict[str, Any] = {
            s.key: (list(s.default) if isinstance(s.default, list) else s.default)
            for s in CONFIG_SCHEMA
        }
        self._plugin_data: dict[str, dict[str, Any]] = {}
        self._listeners: list[Callable[[str, Any], None]] = []
        if self.persist:
            self.load()

    def on_change(self, cb: Callable[[str, Any], None]) -> None:
        with self._lock:
            self._listeners.append(cb)

    def get(self, key: str, default: Any = None) -> Any:
        with self._lock:
            if key in self._values:
                v = self._values[key]
                return list(v) if isinstance(v, list) else v
            return default

    def set(self, key: str, value: Any, *, save: bool = True) -> Any:
        with self._lock:
            spec = _SCHEMA_BY_KEY.get(key)
            coerced = _coerce(spec, value) if spec else value
            prev = self._values.get(key)
            self._values[key] = coerced
            changed = prev != coerced
            listeners = list(self._listeners)
            if changed and save and self.persist:
                self.save()
        if changed:
            for cb in listeners:
                try:
                    cb(key, coerced)
                except Exception:
                    pass
        return coerced

    def get_plugin_config(self, plugin_id: str) -> dict[str, Any]:
        with self._lock:
            return dict(self._plugin_data.get(plugin_id, {}))

    def set_plugin_config(self, plugin_id: str, data: dict[str, Any], *, save: bool = True) -> None:
        with self._lock:
            self._plugin_data[plugin_id] = dict(data)
            if save and self.persist:
                self.save()

    def load(self) -> None:
        with self._lock:
            if not self.path.is_file():
                return
            try:
                raw = tomllib.loads(self.path.read_text(encoding="utf-8"))
            except Exception:
                return
            for sec, table in raw.items():
                if sec == "plugin_config" and isinstance(table, dict):
                    for pid, pcfg in table.items():
                        if isinstance(pcfg, dict):
                            self._plugin_data[pid] = dict(pcfg)
                    continue
                if isinstance(table, dict):
                    for k, v in table.items():
                        dotted = f"{sec}.{k}"
                        spec = _SCHEMA_BY_KEY.get(dotted)
                        if spec is not None:
                            self._values[dotted] = _coerce(spec, v)

    def save(self) -> None:
        if not self.persist:
            return
        with self._lock:
            sections: dict[str, list[tuple[str, Any]]] = {}
            for spec in CONFIG_SCHEMA:
                sec, name = spec.key.split(".", 1)
                sections.setdefault(sec, []).append((name, self._values[spec.key]))

            lines = ["# Kern Dock configuration (auto-generated)\n"]
            for sec, items in sections.items():
                lines.append(f"[{sec}]")
                for name, val in items:
                    lines.append(f"{name} = {_toml_literal(val)}")
                lines.append("")

            for pid, pcfg in sorted(self._plugin_data.items()):
                safe_pid = "".join(c for c in pid if c.isalnum() or c in "-_")
                if not safe_pid:
                    continue
                lines.append(f"[plugin_config.{safe_pid}]")
                for k, v in sorted(pcfg.items()):
                    if isinstance(v, (bool, int, float, str, list)):
                        lines.append(f"{k} = {_toml_literal(v)}")
                lines.append("")

            payload = "\n".join(lines).encode("utf-8")
            try:
                self.path.parent.mkdir(parents=True, exist_ok=True)
                fd, tmp_name = tempfile.mkstemp(prefix=".config-", suffix=".tmp", dir=str(self.path.parent))
                try:
                    os.fchmod(fd, 0o600)
                    os.write(fd, payload)
                    os.fsync(fd)
                finally:
                    os.close(fd)
                os.replace(tmp_name, self.path)
            except OSError:
                pass
