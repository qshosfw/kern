"""UI-agnostic DockRuntime orchestrating core services, link, plugins, tray, IPC & notifications."""

from __future__ import annotations

import threading
from pathlib import Path
from typing import Any, Callable

from .. import APP_NAME, __version__
from ..audio import AudioManager
from ..core.config import ConfigManager
from ..core.events import Event, EventBus
from ..core.options import DockOptions
from ..core.registry import CommandRegistry, PageRegistry, TrayMenuRegistry
from ..core.security import SecurityController
from ..device.link import DockLink, REBOOT_BOOTLOADER, REBOOT_FIRMWARE, REBOOT_MODES, REBOOT_RECOVERY
from ..device.usb import DetectedPort, PortMonitor
from ..plugins.manager import PluginManager
from ..render.capture import RecordingSession, encode_png_rgba, render_handset_to_rgba, render_lcd_to_rgba
from ..render.handset.catalog import MODELS, radio_model
from ..render.lcd import FastLcdState, LCD_PALETTES, LcdSettings
from ..system.desktop import desktop_integration_status, install_desktop_integration, launch_external_terminal
from ..system.ipc import IpcServer
from ..system.notifications import DesktopNotifier
from ..system.tray import SystemTrayService


class DockRuntime:
    """
    Central service container cleanly separating business logic, OS integration and plugins
    from GTK4 widgets. Fully instantiable headless for CLI, IPC, and automated tests.
    """

    def __init__(
        self,
        opts: DockOptions | None = None,
        *,
        config_path: Path | None = None,
        persist_config: bool = True,
        socket_path: Path | None = None,
        enable_dbus: bool = True,
    ) -> None:
        self.opts = opts or DockOptions()
        self.config = ConfigManager(path=config_path, persist=persist_config)
        self.events = EventBus()

        init_hardened = self.opts.hardened if self.opts.hardened is not None else bool(self.config.get("security.hardened", False))
        self.security = SecurityController(
            self.events,
            hardened=init_hardened,
            tx_relock_seconds=int(self.config.get("security.tx_relock_seconds", 120)),
        )
        self.security.set_ipc_allow_control(bool(self.config.get("security.ipc_allow_control", True)))

        self.actions = CommandRegistry()
        self.pages = PageRegistry()
        self.tray_menu = TrayMenuRegistry()

        self.link = DockLink(self.events, self.security)
        self.audio = AudioManager(on_change=lambda: self.events.emit("audio.changed", {}, source="audio"))
        self.recorder = RecordingSession()
        self.lcd_state = FastLcdState()

        cfg_model = str(self.config.get("handset.model", "auto"))
        self.forced_model: str | None = (
            self.opts.model if (self.opts.model in MODELS) else (cfg_model if cfg_model in MODELS else None)
        )
        self.active_model: str = self.forced_model or "k1"

        cfg_pal = str(self.config.get("lcd.palette", "auto"))
        if self.opts.palette and self.opts.palette in LCD_PALETTES:
            init_pal = self.opts.palette
            self.user_picked_palette = True
        elif cfg_pal in LCD_PALETTES:
            init_pal = cfg_pal
            self.user_picked_palette = True
        else:
            init_pal = radio_model(self.active_model).palette
            self.user_picked_palette = False

        self.lcd_st = LcdSettings(
            palette=init_pal,
            pixel_grid=bool(self.config.get("lcd.pixel_grid", True)),
            invert_lcd=bool(self.config.get("lcd.invert", False)),
            ghosting=bool(self.config.get("lcd.ghosting", True)),
            contrast=int(self.config.get("lcd.contrast", 13)),
            backlight=int(self.config.get("lcd.backlight", 8)),
            backlight_glow=bool(self.config.get("lcd.backlight_glow", True)),
        )

        self.auto_connect_usb: bool = (self.opts.port is None) and bool(self.config.get("connection.auto_connect", True))
        self.start_headless: bool = self.opts.headless
        self._connecting = False
        self.flashing_active = False
        self._known_devs: set[str] = set()

        self.notifier = DesktopNotifier(enabled=bool(self.config.get("os.notifications", True)))
        self.plugins = PluginManager(self)

        # UI hooks optionally bound by KernDockApp
        self.on_present_window: Callable[[], None] | None = None
        self.on_hide_window: Callable[[], None] | None = None
        self.on_quit: Callable[[], None] | None = None
        self.on_toggle_mini: Callable[[], None] | None = None
        self.on_navigate_page: Callable[[str], None] | None = None

        self.tray = SystemTrayService(
            on_activate=self._on_tray_activate,
            menu_provider=self.build_tray_menu,
        )
        self.ipc = IpcServer(
            self.dispatch_ipc,
            socket_path=socket_path,
            enable_dbus=enable_dbus,
        )
        self.port_monitor = PortMonitor(on_change=self._on_ports_changed)

        self.events.subscribe("link.connected", self._on_link_connected)
        self.events.subscribe("link.disconnected", self._on_link_disconnected)
        self.events.subscribe("link.telemetry", lambda _e: self.refresh_tray())
        self.events.subscribe("link.audio_notes", lambda e: self.audio.play_headless_notes(e.payload.get("notes", [])))
        self.events.subscribe("security.changed", lambda _e: self.refresh_tray())
        self.tray_menu.on_change(self.refresh_tray)

    def start_services(self) -> None:
        if not self.opts.no_plugins:
            self.plugins.discover_all()
            self.plugins.activate_enabled()
        if not self.opts.no_ipc and bool(self.config.get("os.ipc_enabled", True)):
            self.ipc.start()
        if not self.opts.no_tray and bool(self.config.get("os.tray_enabled", True)):
            self.tray.start()
            self.refresh_tray()

    def shutdown(self) -> None:
        self.audio.shutdown()
        self.tray.stop()
        self.ipc.stop()
        self.plugins.shutdown()
        self.link.disconnect()

    # --- Model & LCD Configuration ---
    def set_model(self, model: str | None) -> None:
        self.forced_model = model if (model and model in MODELS) else None
        self.config.set("handset.model", self.forced_model or "auto")
        self.active_model = self.forced_model or (self.link.detected_model if self.link.connected else "k1")
        if not self.user_picked_palette:
            self.lcd_st.palette = radio_model(self.active_model).palette
        self.events.emit("handset.changed", {"model": self.active_model}, source="runtime")

    def set_palette(self, palette: str | None) -> None:
        if palette is None or palette == "auto":
            self.user_picked_palette = False
            self.lcd_st.palette = radio_model(self.active_model).palette
            self.config.set("lcd.palette", "auto")
        elif palette in LCD_PALETTES:
            self.user_picked_palette = True
            self.lcd_st.palette = palette
            self.config.set("lcd.palette", palette)
        self.events.emit("lcd.changed", {"palette": self.lcd_st.palette}, source="runtime")

    def set_lcd_setting(self, attr: str, value: Any) -> None:
        setattr(self.lcd_st, attr, value)
        cfg_key = "lcd.invert" if attr == "invert_lcd" else f"lcd.{attr}"
        self.config.set(cfg_key, value)
        self.events.emit("lcd.changed", {attr: value}, source="runtime")

    # --- Connection helpers ---
    def connect_async(
        self,
        port: str,
        port_info: DetectedPort | None = None,
        *,
        on_error: Callable[[str], None] | None = None,
    ) -> None:
        if self._connecting:
            return
        self._connecting = True
        self.events.emit("link.connecting", {"port": port}, source="runtime")

        def _worker() -> None:
            try:
                baud = int(self.opts.baud or self.config.get("connection.baud", 115200))
                self.link.connect(port, baud=baud, model_override=self.forced_model, port_info=port_info)
            except Exception as exc:
                self.events.emit("link.error", {"port": port, "error": str(exc)}, source="runtime")
                if on_error:
                    on_error(str(exc))
            finally:
                self._connecting = False

        threading.Thread(target=_worker, daemon=True, name="kern-dock-connect").start()

    def _on_ports_changed(self, ports: list[DetectedPort]) -> None:
        self.events.emit("ports.changed", {"count": len(ports)}, source="usb")
        active_devs = {p.device for p in ports}
        newly_plugged = [p for p in ports if p.is_quansheng and p.device not in self._known_devs]
        self._known_devs = active_devs
        if (
            self.link.connected
            and not self.link.demo_mode
            and self.link.port
            and not self.link.port.startswith("tcp://")
            and self.link.port not in active_devs
        ):
            self.link.disconnect(reason="USB radio unplugged")
            return
        if (
            newly_plugged
            and self.auto_connect_usb
            and not self.link.connected
            and not self._connecting
            and not self.flashing_active
        ):
            self.connect_async(newly_plugged[0].device, newly_plugged[0])
            return
        self.refresh_tray()

    def _on_link_connected(self, ev: Event) -> None:
        want = self.forced_model or self.link.detected_model
        if want != self.active_model:
            self.active_model = want
            if not self.user_picked_palette:
                self.lcd_st.palette = radio_model(self.active_model).palette
            self.events.emit("handset.changed", {"model": self.active_model}, source="runtime")
        if self.start_headless and self.link.link_mode == "kern_os" and not self.link.headless:
            self.link.set_headless(True)
        self.refresh_tray()
        if not self.link.demo_mode:
            mode = "Bootloader" if self.link.link_mode == "stock_bl" else "Kern OS"
            self.notifier.send(
                f"{self.link.volume_name} Connected",
                f"{mode} ready on {self.link.port}",
                notif_id="kern-dock-link",
            )

    def _on_link_disconnected(self, ev: Event) -> None:
        self.refresh_tray()
        reason = str(ev.payload.get("reason", "Disconnected"))
        if "unplugged" in reason.lower() or "lost" in reason.lower():
            self.notifier.send("Radio Disconnected", reason, notif_id="kern-dock-link")

    # --- Shell / External Terminal ---
    def shell_env_extra(self) -> dict[str, str]:
        env: dict[str, str] = {"KERN_MODEL": self.active_model}
        if self.link.connected and self.link.port:
            env["KERN_PORT"] = self.link.port
            env["KERN_VOLUME"] = self.link.volume_name
        if self.link.dav_server is not None:
            env["KERN_DAV_URL"] = f"http://127.0.0.1:{self.link.dav_port}/"
            if self.link.dav_open_target:
                env["KERN_MOUNT_PATH"] = self.link.dav_open_target
        return env

    def open_external_terminal(self) -> str:
        pref = str(self.config.get("os.terminal_cmd", "auto"))
        term = launch_external_terminal(preferred=pref, env_extra=self.shell_env_extra())
        self.security.record("system", "open_terminal", "allow", term)
        return term

    # --- System Tray ---
    def _tray_state(self) -> tuple[str, str, str]:
        if not self.link.connected:
            return "offline", APP_NAME, "No radio connected"
        if self.link.link_mode == "stock_bl":
            ver = self.link.bootloader_info.get("version", "Stock BL")
            return "dfu", f"{APP_NAME} — Bootloader {ver}", f"Ready to flash on {self.link.port}"
        if self.security.hardened:
            state = "hardened"
        elif self.link.status_led == "tx":
            state = "tx"
        elif self.link.headless:
            state = "headless"
        else:
            state = "online"
        hz = int(self.link.radio_state.get("rx_freq_hz") or 0)
        freq = f"{hz / 1e6:.4f} MHz" if hz else ""
        pct = self.link.last_telemetry.get("battery_pct")
        batt = f" · {pct}%" if pct is not None else ""
        detail = f"{freq} {self.link.radio_state.get('mod_name', '')}{batt}".strip() or f"{self.link.port}"
        return state, f"{APP_NAME} — {self.link.volume_name}", detail

    def refresh_tray(self) -> None:
        if not self.tray.active:
            return
        state, title, body = self._tray_state()
        self.tray.update(state=state, title=title, body=body)

    def _on_tray_activate(self) -> None:
        if self.on_present_window is not None:
            self.on_present_window()

    def build_tray_menu(self) -> list[dict[str, Any]]:
        online = self.link.connected and self.link.link_mode == "kern_os"
        state, _title, body = self._tray_state()
        items: list[dict[str, Any]] = [
            {"label": f"{self.link.volume_name if self.link.connected else 'Kern Dock'} ({state.upper()})", "enabled": False},
            {"label": body, "enabled": False},
            {"separator": True},
            {"label": "Show Kern Dock", "handler": lambda: self.on_present_window() if self.on_present_window else None},
            {"label": "Toggle Compact Mini-Dock", "handler": lambda: self.on_toggle_mini() if self.on_toggle_mini else None},
            {"label": "Open External Terminal", "handler": lambda: self.open_external_terminal()},
            {"separator": True},
        ]
        if self.link.connected:
            items.extend(
                [
                    {"label": "Swap VFO A / B", "enabled": online, "handler": self.link.toggle_vfo},
                    {
                        "label": "Flashlight",
                        "enabled": online,
                        "checked": self.link.torch_on,
                        "handler": lambda: self.link.set_flashlight(not self.link.torch_on),
                    },
                    {
                        "label": "Keypad Lock",
                        "enabled": online,
                        "checked": self.link.keypad_locked,
                        "handler": lambda: self.link.set_keypad_lock(not self.link.keypad_locked),
                    },
                    {
                        "label": "Headless Mode",
                        "enabled": online,
                        "checked": self.link.headless,
                        "handler": lambda: self.link.set_headless(not self.link.headless),
                    },
                    {"label": "Disconnect", "handler": lambda: self.link.disconnect()},
                ]
            )
        else:
            for p in self.port_monitor.ports[:4]:
                items.append(
                    {
                        "label": f"Connect {p.pretty_name} ({p.device})",
                        "handler": lambda po=p: self.connect_async(po.device, po),
                    }
                )
            items.append({"label": "Launch Offline Demo Radio", "handler": lambda: self.connect_async("demo")})

        plugin_items = self.tray_menu.items()
        if plugin_items:
            items.append({"separator": True})
            for pi in plugin_items:
                items.append(
                    {
                        "label": pi.label,
                        "enabled": pi.enabled() if pi.enabled else True,
                        "checked": pi.toggle_state() if pi.toggle_state else None,
                        "handler": pi.handler,
                    }
                )

        items.extend(
            [
                {"separator": True},
                {
                    "label": "Hardened Mode",
                    "checked": self.security.hardened,
                    "handler": lambda: self.set_hardened(not self.security.hardened),
                },
                {"label": "Quit Kern Dock", "handler": lambda: self.on_quit() if self.on_quit else None},
            ]
        )
        return items

    def set_hardened(self, enabled: bool) -> None:
        self.security.set_hardened(enabled)
        self.config.set("security.hardened", enabled)

    # --- IPC / D-Bus Command Dispatcher ---
    def dispatch_ipc(self, method: str, params: dict[str, Any]) -> dict[str, Any]:
        read_only_methods = {
            "ping", "version", "status", "ports", "plugins", "actions", "audit", "desktop-status", "models"
        }
        mutates = method not in read_only_methods
        self.security.check_ipc_command(method, mutates=mutates)

        if method == "ping":
            return {"pong": True, "version": __version__}
        if method == "version":
            return {"version": __version__, "app": APP_NAME}
        if method == "status":
            return self.link.status_snapshot()
        if method == "ports":
            ports = self.port_monitor.scan()
            return {"ports": [{"device": p.device, "name": p.pretty_name, "mode_hint": p.mode_hint, "is_quansheng": p.is_quansheng} for p in ports]}
        if method == "models":
            return {"active": self.active_model, "models": {k: v.label for k, v in MODELS.items()}}
        if method == "plugins":
            return {"plugins": self.plugins.summary()}
        if method == "actions":
            return {"actions": [{"id": a.id, "title": a.title, "category": a.category, "mutates": a.mutates} for a in self.actions.all()]}
        if method == "audit":
            lim = int(params.get("limit", 40))
            return {
                "entries": [
                    {"ts": e.ts_wall, "category": e.category, "action": e.action, "outcome": e.outcome, "detail": e.detail}
                    for e in self.security.audit_entries(lim)
                ]
            }
        if method == "desktop-status":
            return desktop_integration_status()
        if method == "install-desktop":
            return {"installed": install_desktop_integration()}
        if method == "connect":
            port = str(params.get("port", "demo"))
            self.link.connect(port, baud=int(params.get("baud", self.opts.baud)), model_override=self.forced_model)
            return self.link.status_snapshot()
        if method == "disconnect":
            self.link.disconnect()
            return {"connected": False}
        if method == "freq":
            mhz = float(params["mhz"])
            self.link.set_frequency_hz(int(round(mhz * 1_000_000)))
            return {"rx_freq_hz": self.link.radio_state.get("rx_freq_hz")}
        if method == "mode":
            self.link.set_modulation(str(params["mode"]))
            return {"mod_name": self.link.radio_state.get("mod_name")}
        if method == "sql":
            self.link.set_squelch(int(params["level"]))
            return {"squelch": self.link.radio_state.get("squelch")}
        if method == "key":
            self.link.send_key(str(params["key"]), str(params.get("state", "press")))
            return {"sent": True}
        if method == "torch":
            val = bool(params["on"]) if "on" in params else (not self.link.torch_on)
            self.link.set_flashlight(val)
            return {"torch_on": self.link.torch_on}
        if method == "lock":
            val = bool(params["on"]) if "on" in params else (not self.link.keypad_locked)
            self.link.set_keypad_lock(val)
            return {"keypad_locked": self.link.keypad_locked}
        if method == "headless":
            val = bool(params["on"]) if "on" in params else (not self.link.headless)
            self.link.set_headless(val)
            return {"headless": self.link.headless}
        if method == "vfo":
            self.link.toggle_vfo()
            return {"vfo_toggled": True}
        if method == "reboot":
            arg = str(params.get("mode", "firmware")).lower()
            mode = next((m for m, (_l, alias) in REBOOT_MODES.items() if arg in alias), REBOOT_FIRMWARE)
            self.link.reboot(mode)
            return {"reboot": REBOOT_MODES[mode][0]}
        if method == "wire":
            out = self.link.run_wire_command(str(params.get("cmd", "")))
            return {"output": out}
        if method == "mount":
            url = self.link.start_dav(http_port=int(params.get("port", 8765)))
            return {"url": url}
        if method == "unmount":
            self.link.stop_dav()
            return {"mounted": False}
        if method == "screenshot":
            dest = Path(str(params["path"])).expanduser().resolve()
            scope = str(params.get("scope", "lcd"))
            if scope == "body":
                w, h, rgba = render_handset_to_rgba(
                    self.active_model, self.link.last_fb, self.lcd_st, led_state=self.link.status_led, torch_on=self.link.torch_on, scale=2.0
                )
            else:
                w, h, rgba = render_lcd_to_rgba(self.link.last_fb, self.lcd_st, width=540, height=374)
            png = encode_png_rgba(w, h, rgba)
            dest.write_bytes(png)
            return {"path": str(dest), "bytes": len(png)}
        if method == "invoke":
            self.actions.invoke(str(params["action"]))
            return {"invoked": params["action"]}
        if method == "show":
            if self.on_present_window:
                self.on_present_window()
            return {"shown": True}
        if method == "hide":
            if self.on_hide_window:
                self.on_hide_window()
            return {"hidden": True}
        if method == "toggle-mini":
            if self.on_toggle_mini:
                self.on_toggle_mini()
            return {"toggled": True}
        raise ValueError(f"Unknown IPC method '{method}'")
