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

        self.follow_device_display: bool = bool(self.config.get("lcd.follow_device", True))
        self._manual_lcd: dict[str, Any] = {
            "backlight": int(self.config.get("lcd.backlight", 8)),
            "contrast": int(self.config.get("lcd.contrast", 13)),
            "invert_lcd": bool(self.config.get("lcd.invert", False)),
        }
        self.lcd_st = LcdSettings(
            palette=init_pal,
            pixel_grid=bool(self.config.get("lcd.pixel_grid", True)),
            invert_lcd=bool(self._manual_lcd["invert_lcd"]),
            ghosting=bool(self.config.get("lcd.ghosting", True)),
            contrast=int(self._manual_lcd["contrast"]),
            backlight=int(self._manual_lcd["backlight"]),
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
        self.mini_mode_state: Callable[[], bool] = lambda: False
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
        self.events.subscribe("link.telemetry", self._on_link_telemetry)
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

    def set_follow_device_display(self, follow: bool) -> None:
        self.follow_device_display = bool(follow)
        self.config.set("lcd.follow_device", self.follow_device_display)
        if self.follow_device_display:
            self._sync_device_display()
        else:
            changed = False
            for k, v in self._manual_lcd.items():
                if getattr(self.lcd_st, k) != v:
                    setattr(self.lcd_st, k, v)
                    changed = True
            if changed:
                self.events.emit("lcd.changed", {"follow_device": False}, source="runtime")

    def _sync_device_display(self) -> None:
        if not self.follow_device_display or not self.link.connected or self.link.link_mode != "rigwire":
            return
        t = self.link.last_telemetry
        if "display_backlight" not in t:
            return
        bl = max(0, min(10, int(t["display_backlight"])))
        c_pct = max(0, min(100, int(t.get("display_contrast_pct", 50))))
        contrast = max(0, min(15, int(round(c_pct * 15.0 / 100.0))))
        inv = bool(t.get("display_inverted", False))
        if self.lcd_st.backlight != bl or self.lcd_st.contrast != contrast or self.lcd_st.invert_lcd != inv:
            self.lcd_st.backlight = bl
            self.lcd_st.contrast = contrast
            self.lcd_st.invert_lcd = inv
            self.events.emit("lcd.changed", {"device_sync": True}, source="runtime")

    def _on_link_telemetry(self, _ev: Event) -> None:
        self._sync_device_display()
        self.refresh_tray()

    def set_lcd_setting(self, attr: str, value: Any) -> None:
        setattr(self.lcd_st, attr, value)
        if attr in self._manual_lcd:
            self._manual_lcd[attr] = value
        cfg_key = "lcd.invert" if attr == "invert_lcd" else f"lcd.{attr}"
        self.config.set(cfg_key, value)
        self.events.emit("lcd.changed", {attr: value}, source="runtime")

    # --- Connection helpers ---
    def discover_emulators(self) -> list[Any]:
        """Return all live running `kern-emu` instances (state files + localhost probe)."""
        from ..device.usb import discover_emulators as _discover_emus

        return _discover_emus()

    def emulator_status(self) -> dict[str, Any]:
        """Inspect whether any in-house `kern-emu` hardware emulator is running or ready to launch."""
        import shutil as _shutil

        instances = self.discover_emulators()
        primary = instances[0] if instances else None
        repo_root = Path(__file__).resolve().parents[4]
        emu_script = repo_root / "tools" / "emu.py"
        emu_crate = repo_root / "emu" / "Cargo.toml"
        can_launch = emu_script.is_file() and emu_crate.is_file() and bool(_shutil.which("cargo"))
        return {
            "running": primary is not None,
            "pid": primary.pid if primary else None,
            "wire_url": primary.wire_url if primary else "tcp://127.0.0.1:4446",
            "instances": instances,
            "can_launch": can_launch,
            "emu_script": str(emu_script),
        }

    def stop_emulator_daemon(self, *, state_file: str | None = None, wire_url: str | None = None) -> bool:
        """Stop a running `kern-emu` daemon (specific `state_file`/`wire_url`, or primary instance)."""
        import subprocess as _sub
        import sys as _sys

        st = self.emulator_status()
        target_url = wire_url or st["wire_url"]
        if self.link.connected and self.link.port == target_url:
            self.link.disconnect(reason="Stopped hardware emulator")
        cmd = [_sys.executable, st["emu_script"], "stop"]
        if state_file:
            cmd.extend(["--state-file", state_file])
        res = _sub.run(cmd, capture_output=True)
        self.events.emit("emulators.changed", {"wire_url": target_url, "stopped": True}, source="runtime")
        return res.returncode == 0

    def stop_emulator_daemon_async(
        self,
        *,
        state_file: str | None = None,
        wire_url: str | None = None,
        on_done: Callable[[bool], None] | None = None,
    ) -> None:
        """Stop a running `kern-emu` daemon off the UI thread."""
        def _worker() -> None:
            ok = False
            try:
                ok = self.stop_emulator_daemon(state_file=state_file, wire_url=wire_url)
            finally:
                if on_done:
                    on_done(ok)

        threading.Thread(target=_worker, daemon=True, name="kern-dock-emu-stop").start()

    def connect_or_start_emulator_async(
        self,
        *,
        wire_url: str | None = None,
        on_status: Callable[[str], None] | None = None,
        on_error: Callable[[str], None] | None = None,
    ) -> None:
        """Connect to a running `kern-emu` instance, or auto-build & launch it first."""
        if self._connecting:
            return
        if wire_url:
            if self.link.connected and not self.link.demo_mode and self.link.port == wire_url:
                return
            self.connect_async(wire_url, on_error=on_error)
            return
        st = self.emulator_status()
        if st["running"] or not st["can_launch"]:
            if self.link.connected and not self.link.demo_mode and self.link.port == st["wire_url"]:
                return
            self.connect_async(st["wire_url"], on_error=on_error)
            return

        self._connecting = True
        self.events.emit(
            "link.connecting",
            {"port": st["wire_url"], "emulated": True, "message": "Starting emulator…"},
            source="runtime",
        )
        if on_status:
            on_status("Starting hardware emulator…")

        def _worker() -> None:
            import json as _json
            import subprocess as _sub
            import sys as _sys
            import time as _time

            try:
                proc = _sub.run(
                    [_sys.executable, st["emu_script"], "start", "--daemon", "--turbo", "--provision", "--json"],
                    capture_output=True,
                    text=True,
                    timeout=60,
                )
                if proc.returncode != 0:
                    lines = [ln.strip() for ln in (proc.stderr or proc.stdout or "").splitlines() if ln.strip()]
                    msg = lines[-1] if lines else f"exit {proc.returncode}"
                    raise RuntimeError(f"Emulator failed to start: {msg[:180]}")
                wire_url_out = st["wire_url"]
                for ln in reversed(proc.stdout.splitlines()):
                    ln = ln.strip()
                    if ln.startswith("{") and ln.endswith("}"):
                        try:
                            meta = _json.loads(ln)
                            wire_url_out = str(meta.get("wire_url") or wire_url_out)
                            break
                        except Exception:
                            pass
                self.events.emit("emulators.changed", {"wire_url": wire_url_out, "started": True}, source="runtime")
                baud = int(self.opts.baud or self.config.get("connection.baud", 115200))
                last_exc: Exception | None = None
                for attempt in range(4):
                    try:
                        self.link.connect(wire_url_out, baud=baud, model_override=self.forced_model)
                        last_exc = None
                        break
                    except Exception as exc:
                        last_exc = exc
                        if attempt < 3:
                            _time.sleep(0.25)
                if last_exc is not None:
                    raise last_exc
            except Exception as exc:
                self.events.emit("link.error", {"port": st["wire_url"], "error": str(exc)}, source="runtime")
                if on_error:
                    on_error(str(exc))
            finally:
                self._connecting = False

        threading.Thread(target=_worker, daemon=True, name="kern-dock-emu-connect").start()

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
        if self.start_headless and self.link.link_mode == "rigwire" and not self.link.headless:
            self.link.set_headless(True)
        self.refresh_tray()
        if not self.link.demo_mode:
            mode = "Bootloader" if self.link.link_mode == "stock_bl" else self.link.firmware_name
            self.notifier.send(
                f"{self.link.volume_name} Connected",
                f"{mode} ready on {self.link.port}",
                notif_id="kern-dock-link",
            )

    def _on_link_disconnected(self, ev: Event) -> None:
        if self.follow_device_display:
            for k, v in self._manual_lcd.items():
                setattr(self.lcd_st, k, v)
            self.events.emit("lcd.changed", {"device_sync": False}, source="runtime")
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
        suffix = " (Emulated)" if self.link.is_emulated else (" (Demo)" if self.link.demo_mode else "")
        return state, f"{APP_NAME} — {self.link.volume_name}{suffix}", detail

    def refresh_tray(self) -> None:
        if not self.tray.active:
            return
        state, title, body = self._tray_state()
        self.tray.update(state=state, title=title, body=body)

    def _on_tray_activate(self) -> None:
        if self.on_present_window is not None:
            self.on_present_window()

    _TRAY_STATUS: dict[str, str] = {
        "offline": "Not Connected",
        "online": "Connected",
        "tx": "Transmitting",
        "headless": "Headless",
        "hardened": "Safe Mode",
        "dfu": "Bootloader",
    }

    def build_tray_menu(self) -> list[dict[str, Any]]:
        """
        Short, HIG-style tray menu (no icons, one status line, at most one submenu):
        status · Show/Mini-Dock · quick radio toggles or Connect ▸ · plugin items · Quit.
        Everything else lives in the main window and the command palette.
        """
        lk = self.link
        online = lk.connected and lk.link_mode == "rigwire"
        state, _title, _body = self._tray_state()
        name = lk.volume_name.removeprefix("Quansheng ") if lk.connected else APP_NAME
        sep = {"separator": True}

        items: list[dict[str, Any]] = [
            {"label": f"{name} — {self._TRAY_STATUS.get(state, '')}", "enabled": False},
            sep,
            {"label": "Show Kern Dock", "handler": lambda: self.on_present_window() if self.on_present_window else None},
            {"label": "Mini-Dock", "checked": self.mini_mode_state(), "handler": lambda: self.on_toggle_mini() if self.on_toggle_mini else None},
            sep,
        ]

        if lk.connected:
            items += [
                {"label": "Flashlight", "enabled": online, "checked": lk.torch_on, "handler": lambda: lk.set_flashlight(not lk.torch_on)},
                {"label": "Keypad Lock", "enabled": online, "checked": lk.keypad_locked, "handler": lambda: lk.set_keypad_lock(not lk.keypad_locked)},
                {"label": "Disconnect", "handler": lk.disconnect},
            ]
        else:
            conn: list[dict[str, Any]] = [
                {"label": p.pretty_name, "handler": lambda po=p: self.connect_async(po.device, po)} for p in self.port_monitor.ports[:5]
            ]
            conn += [{"label": f"{e.name} (Emulator)", "handler": lambda url=e.wire_url: self.connect_async(url)} for e in self.discover_emulators()[:3]]
            if conn:
                conn.append(sep)
            conn.append({"label": "Demo Radio", "handler": lambda: self.connect_async("demo")})
            items.append({"label": "Connect", "children": conn})

        plugin_items = self.tray_menu.items()
        if plugin_items:
            items.append(sep)
            items += [
                {
                    "label": pi.label,
                    "enabled": pi.enabled() if pi.enabled else True,
                    "checked": pi.toggle_state() if pi.toggle_state else None,
                    "handler": pi.handler,
                }
                for pi in plugin_items
            ]

        items += [sep, {"label": "Quit", "handler": lambda: self.on_quit() if self.on_quit else None}]
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
            emus = self.discover_emulators()
            return {
                "ports": [{"device": p.device, "name": p.pretty_name, "mode_hint": p.mode_hint, "is_quansheng": p.is_quansheng} for p in ports],
                "emulators": [
                    {
                        "wire_url": e.wire_url,
                        "name": e.name,
                        "engine": e.engine,
                        "pid": e.pid,
                        "firmware": e.firmware,
                        "emulated": True,
                    }
                    for e in emus
                ],
            }
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
