"""DockLink: decoupled, hardened RigWire session, bootloader detection, demo radio & WebDAV."""

from __future__ import annotations

import queue
import struct
import subprocess
import threading
import time
from typing import Any

import kernfs_mount
import rigwire

from ..core.events import EventBus
from ..core.security import SecurityController
from ..render.handset.catalog import MODELS, radio_model
from .demo import make_demo_channels_table, make_demo_framebuffer, make_demo_krx, make_text_input_framebuffer
from .t9 import T9Model
from .usb import DetectedPort, detect_hardware_model, probe_bootloader_beacon

REBOOT_FIRMWARE, REBOOT_BOOTLOADER, REBOOT_RECOVERY = 0, 1, 2

REBOOT_MODES: dict[int, tuple[str, tuple[str, ...]]] = {
    REBOOT_FIRMWARE: ("Restarting firmware", ("firmware", "fw", "normal", "0")),
    REBOOT_BOOTLOADER: ("Restarting into the bootloader", ("bootloader", "bl", "dfu", "1")),
    REBOOT_RECOVERY: ("Restarting into safe recovery", ("recovery", "safe", "2")),
}


def resolve_firmware_name(hello: dict[str, Any] | None = None, telemetry: dict[str, Any] | None = None) -> str:
    """Resolve the connected radio's advertised firmware/OS name, defaulting to 'RigWire'."""
    for src in (telemetry or {}, hello or {}):
        for key in ("firmware_name", "os_name", "edition", "firmware"):
            val = str(src.get(key) or "").strip()
            if val:
                return val
    return "RigWire"


class DockLink:
    """Pipelined low-latency RigWire & Bootloader/DFU link publishing typed events to EventBus."""

    def __init__(self, events: EventBus, security: SecurityController) -> None:
        self.events = events
        self.security = security
        self.lock = threading.RLock()
        self.client: rigwire.RigWireClient | None = None
        self.fs_backend: kernfs_mount.FsBackend | None = None
        self.port: str | None = None
        self.baud: int = 115200
        self.link_mode: str = "disconnected"  # "disconnected" | "rigwire" | "stock_bl"
        self.firmware_name: str = "RigWire"
        self.bootloader_info: dict[str, str] = {}
        self.volume_name: str = "Quansheng UV-K1"
        self.detected_model: str = "k1"
        self.hello_info: dict[str, Any] = {}
        self.last_telemetry: dict[str, Any] = {}
        self.demo_mode: bool = False
        self.demo_freq_khz: int = 433500
        self.demo_freq_b_khz: int = 145500
        self.demo_status: str = "VFO"
        self.demo_text: T9Model | None = None
        self._demo_fs: kernfs_mount.FsBackend | None = None
        self.connected_at: float = 0.0
        self.radio_state: dict[str, Any] = {}
        self.torch_on: bool = False
        self.keypad_locked: bool = False
        self.headless: bool = False
        self.status_led: str = "off"
        self.bk4829_gpio: int = 0
        self.last_fb: bytes = bytes(1024)
        self.fps: float = 0.0
        self.rtt_ms: float = 0.0
        self.error_count: int = 0
        self.connected: bool = False
        self._cmd_q: queue.Queue[tuple[str, Any]] = queue.Queue()
        self._wake_ev = threading.Event()
        self._stop = threading.Event()
        self._thread: threading.Thread | None = None
        self.dav_server: kernfs_mount.KernWebDAVServer | None = None
        self.dav_port: int = 8765
        self.dav_open_target: str | None = None
        self.dav_unmount_cmd: list[str] | None = None

    @property
    def policy(self) -> kernfs_mount.SafetyPolicy:
        return self.security.policy

    @property
    def is_rigwire(self) -> bool:
        return self.connected and self.link_mode == "rigwire"

    def _emit_log(self, line: str) -> None:
        self.events.emit("log.line", {"line": f"[{time.strftime('%H:%M:%S')}] {line}"}, source="link")

    def status_snapshot(self) -> dict[str, Any]:
        return {
            "connected": self.connected,
            "link_mode": self.link_mode,
            "firmware_name": self.firmware_name,
            "demo_mode": self.demo_mode,
            "port": self.port,
            "volume_name": self.volume_name,
            "detected_model": self.detected_model,
            "fps": round(self.fps, 1),
            "rtt_ms": round(self.rtt_ms, 2),
            "error_count": self.error_count,
            "torch_on": self.torch_on,
            "keypad_locked": self.keypad_locked,
            "headless": self.headless,
            "status_led": self.status_led,
            "dav_mounted": self.dav_server is not None,
            "dav_url": f"http://127.0.0.1:{self.dav_port}/" if self.dav_server else None,
            "bootloader": dict(self.bootloader_info),
            "telemetry": dict(self.last_telemetry),
            "radio": dict(self.radio_state),
            "security": self.security.snapshot(),
        }

    def connect(
        self,
        port: str,
        baud: int = 115200,
        model_override: str | None = None,
        port_info: DetectedPort | None = None,
    ) -> None:
        self.disconnect(emit_event=False)
        self.baud = baud
        self.security.disarm_tx("connect")

        if port.lower() == "demo":
            self.demo_mode = True
            self.link_mode = "rigwire"
            self.firmware_name = "RigWire"
            self.port = "demo"
            self.detected_model = model_override if model_override in MODELS else "k1"
            self.volume_name = f"Quansheng {radio_model(self.detected_model).label} DEMO"
            self.hello_info = {"minor_version": 1, "accepted_max_payload": 128}
            self.last_telemetry = {
                "serial_str": "DEMO0001K1",
                "mac_addr": "02:4B:45:52:4E:01",
                "battery_mv": 8040,
                "battery_pct": 92,
                "sysclk_hz": 48_000_000,
                "calib_valid": True,
                "mcu_uid": "Protected (Auth Required)",
            }
            mem = kernfs_mount.MemoryFsBackend()
            mem.files["/sys/calib.dat"] = b"\xAA" * 512
            mem.files["/sys/calib.sig"] = b"\xBB" * 32
            mem.files["/apps/dfu.mod"] = make_demo_krx("dfu", 128)
            mem.files["/apps/sysinfo.krx"] = make_demo_krx("sysinfo", 256)
            mem.files["/user/channels.ch"] = make_demo_channels_table()
            mem.files["/user/vfo_a.vfo"] = b"VFO " + b"\x00" * 156
            mem.files["/pref/display.cfg"] = b"DISP" + b"\x00" * 28
            self.fs_backend = mem
            self.connected = True
            self.connected_at = time.monotonic()
            self.rtt_ms = 0.2
            self.demo_freq_khz, self.demo_freq_b_khz, self.demo_status = 433500, 145500, "VFO"
            self.radio_state = {
                "rx_freq_hz": 433_500_000,
                "mod_name": "FM",
                "squelch": 3,
                "step_hz": 25000,
                "rssi_dbm": -96,
                "active_vfo": 0,
            }
            self.last_fb = make_demo_framebuffer()
            self.security.record("system", "connect", "allow", f"Demo radio ({self.detected_model})")
            self._emit_log(f"Connected {self.volume_name} (demo)")
            self.events.emit("link.connected", self.status_snapshot(), source="link")
            self.events.emit("link.frame", {}, source="link")
            self.events.emit("link.telemetry", self.status_snapshot(), source="link")
            return

        with self.lock:
            last_err: Exception | None = None
            cli: rigwire.RigWireClient | None = None
            hello: dict[str, Any] = {}
            telem: dict[str, Any] = {}
            for attempt in range(2):
                cand = rigwire.RigWireClient(port, baudrate=baud, timeout=0.75)
                try:
                    hello = cand.hello()
                    try:
                        telem = cand.get_system_telemetry()
                    except Exception:
                        telem = {}
                    cli = cand
                    break
                except Exception as exc:
                    last_err = exc
                    try:
                        cand.close()
                    except Exception:
                        pass
                    if attempt == 0:
                        time.sleep(0.08)

            if cli is None:
                bl = probe_bootloader_beacon(port, duration_s=0.85)
                if bl is not None:
                    self.port = port
                    self.link_mode = "stock_bl"
                    self.bootloader_info = bl
                    self.volume_name = f"Bootloader {bl['version']}"
                    self.detected_model = model_override or detect_hardware_model(
                        port_info.pretty_name if port_info else "", port_info=port_info
                    )
                    self.connected = True
                    self.last_fb = bytes(1024)
                    self.torch_on = True
                    self.security.record("firmware", "connect_bootloader", "info", f"{port} ({bl['version']})")
                    self._emit_log(f"Bootloader {bl['version']} beacon verified on {port}")
                    self.events.emit("link.connected", self.status_snapshot(), source="link")
                    self.events.emit("link.frame", {}, source="link")
                    self.events.emit("link.telemetry", self.status_snapshot(), source="link")
                    return
                raise last_err or RuntimeError(f"No RigWire or Bootloader response on {port}")

            self.client = cli
            self.client.timeout = 0.45
            self.fs_backend = kernfs_mount.RigWireFsBackend(cli, cache_ttl=2.0, shared_lock=self.lock)
            self.port = port
            self.link_mode = "rigwire"
            self.firmware_name = resolve_firmware_name(hello, telem)
            self.hello_info = hello
            self.last_telemetry = telem
            self.torch_on = bool(telem.get("flashlight", False))
            self.keypad_locked = bool(telem.get("keypad_locked", False))
            self.headless = bool(telem.get("headless", False))
            led_map = {0: "off", 1: "tx", 2: "rx", 3: "orange"}
            self.status_led = led_map.get(int(telem.get("status_led_raw", 0)) & 0x03, "off")
            self.bk4829_gpio = int(telem.get("bk4829_gpio", 0))
            self.volume_name = kernfs_mount.resolve_volume_name(port, hello=hello, telemetry=telem)
            self.detected_model = model_override or detect_hardware_model(
                self.volume_name, hello=hello, port_info=port_info
            )
            self.demo_mode = False
            self.connected = True
            self.connected_at = time.monotonic()
            self.error_count = 0
            self.rtt_ms = cli.last_rtt_ms

        self.security.record("system", "connect", "allow", f"{port} -> {self.volume_name}")
        self._emit_log(f"Connected {port} -> {self.volume_name} ({self.detected_model.upper()}, RTT {self.rtt_ms:.1f} ms)")
        self._stop.clear()
        self._thread = threading.Thread(target=self._poll_loop, daemon=True, name="kern-dock-poll")
        self._thread.start()
        self.events.emit("link.connected", self.status_snapshot(), source="link")

    def disconnect(self, *, reason: str = "Disconnected", emit_event: bool = True) -> None:
        self.stop_dav()
        self._stop.set()
        if self._thread and self._thread.is_alive() and self._thread is not threading.current_thread():
            self._thread.join(timeout=1.5)
        self._thread = None
        with self.lock:
            if self.client is not None:
                try:
                    self.client.close()
                except Exception:
                    pass
                self.client = None
            self.fs_backend = None
        was = self.connected
        self.connected = False
        self.link_mode = "disconnected"
        self.firmware_name = "RigWire"
        self.bootloader_info = {}
        self.demo_mode = False
        self.torch_on = False
        self.keypad_locked = False
        self.headless = False
        self.status_led = "off"
        self.bk4829_gpio = 0
        self.last_fb = bytes(1024)
        self.volume_name = "Quansheng UV-K1"
        self.hello_info = {}
        self.last_telemetry = {}
        self.radio_state = {}
        self.demo_text = None
        self._demo_fs = None
        self.fps = 0.0
        self.rtt_ms = 0.0
        self.security.disarm_tx("disconnect")
        if was:
            self._emit_log(reason)
            if emit_event:
                self.events.emit("link.disconnected", {"reason": reason}, source="link")

    # --- Demo radio helpers ---
    def _demo_render(self) -> None:
        if self.demo_text is not None:
            t = self.demo_text
            self.last_fb = make_text_input_framebuffer(t.buffer, t.charset, t.last_key is not None)
        else:
            def fmt(khz: int) -> str:
                return f"{khz // 1000}.{khz % 1000:03d}"

            self.last_fb = make_demo_framebuffer(
                freq_a=fmt(self.demo_freq_khz),
                freq_b=fmt(self.demo_freq_b_khz),
                status=self.demo_status,
                mode=self.radio_state.get("mod_name", "FM"),
            )
        self.events.emit("link.frame", {}, source="link")

    def _demo_publish_radio(self) -> None:
        self.radio_state["rx_freq_hz"] = self.demo_freq_khz * 1000
        self.events.emit("link.telemetry", self.status_snapshot(), source="link")

    def begin_demo_text(self) -> None:
        if self.demo_mode:
            self.demo_text = T9Model()
            self._demo_render()

    def end_demo_text(self) -> str | None:
        txt = self.demo_text.buffer if self.demo_text is not None else None
        self.demo_text = None
        if self.demo_mode:
            self.demo_status = "SAVED" if txt else "VFO"
            self._demo_render()
        return txt

    # --- Power & Boot ---
    def reboot(self, mode: int) -> None:
        if mode not in REBOOT_MODES:
            raise ValueError(f"Unknown reboot mode {mode}")
        if mode == REBOOT_BOOTLOADER:
            self.security.check_bootloader_entry()
        if not (self.connected and self.link_mode == "rigwire"):
            raise RuntimeError("Restart needs a connected RigWire radio")
        label = REBOOT_MODES[mode][0]
        self.security.record("system", "reboot", "allow", f"mode={mode}")
        if self.demo_mode:
            self._demo_reboot(mode)
            self._emit_log(f"{label} (demo)")
            return
        self._stop.set()
        if self._thread and self._thread.is_alive() and self._thread is not threading.current_thread():
            self._thread.join(timeout=1.5)
        with self.lock:
            if self.client is not None:
                try:
                    self.client.timeout = 0.3
                    self.client.handle_invoke(
                        rigwire.DOMAIN_SYSTEM, 0, rigwire.FACET_SYS_IDENTITY, rigwire.ACTION_SYS_REBOOT, bytes([mode])
                    )
                except Exception:
                    pass
        self._emit_log(f"{label} requested on {self.port}")
        self.disconnect(reason=label)

    def _demo_reboot(self, mode: int) -> None:
        self.demo_text = None
        self.headless = False
        self.keypad_locked = False
        if mode == REBOOT_BOOTLOADER:
            self._demo_fs, self.fs_backend = self.fs_backend, None
            self.link_mode = "stock_bl"
            self.bootloader_info = {"version": "5.00.01", "raw": "demo"}
            self.volume_name = f"Bootloader 5.00.01 ({radio_model(self.detected_model).label} DEMO)"
            self.torch_on = True
            self.last_fb = bytes(1024)
            self.events.emit("link.connected", self.status_snapshot(), source="link")
            self.events.emit("link.frame", {}, source="link")
            self.events.emit("link.telemetry", self.status_snapshot(), source="link")
            return
        self.torch_on = False
        self.connected_at = time.monotonic()
        self.demo_status = "SAFE" if mode == REBOOT_RECOVERY else "BOOT"
        self._demo_render()
        self.events.emit("link.telemetry", self.status_snapshot(), source="link")

    def demo_power_cycle(self) -> None:
        if not (self.demo_mode and self.link_mode == "stock_bl"):
            return
        self.fs_backend, self._demo_fs = self._demo_fs, None
        self.link_mode = "rigwire"
        self.firmware_name = "RigWire"
        self.bootloader_info = {}
        self.volume_name = f"Quansheng {radio_model(self.detected_model).label} DEMO"
        self.torch_on = False
        self.connected_at = time.monotonic()
        self.demo_status = "BOOT"
        self._demo_render()
        self._emit_log("Power cycled into firmware (demo)")
        self.events.emit("link.connected", self.status_snapshot(), source="link")
        self.events.emit("link.telemetry", self.status_snapshot(), source="link")

    # --- Key & RF Control ---
    def send_key(self, key_name: str, state_name: str) -> None:
        if self.link_mode != "rigwire":
            return
        if key_name == "ptt" and state_name in ("press", "hold"):
            self.security.check_tx(demo_mode=self.demo_mode)
        if self.demo_mode:
            if self.demo_text is not None:
                self.demo_text.feed(key_name, state_name, int(time.monotonic() * 1000))
                if self.demo_text.done or self.demo_text.canceled:
                    keep = self.demo_text.done
                    txt = self.end_demo_text()
                    self._emit_log(f"Text entry {'saved: ' + repr(txt) if keep else 'canceled'}")
                else:
                    self._demo_render()
                return
            if state_name == "press":
                if self.headless:
                    note = (1046, 20) if key_name in ("up", "down") else (880, 40)
                    self.events.emit("link.audio_notes", {"notes": [note]}, source="link")
                if key_name == "up":
                    self.demo_freq_khz += 25
                    self._demo_publish_radio()
                elif key_name == "down":
                    self.demo_freq_khz = max(18000, self.demo_freq_khz - 25)
                    self._demo_publish_radio()
                self.demo_status = key_name.upper()[:5]
                self._demo_render()
            elif state_name == "hold":
                self.demo_status = f"H:{key_name.upper()[:3]}"
                self._demo_render()
            return
        self._cmd_q.put(("key", (rigwire.KEY_CODES[key_name], rigwire.KEY_STATES[state_name])))
        self._wake_ev.set()

    def send_encoder(self, delta: int) -> None:
        """Send rotary encoder detent delta (-127..127; -1 = up/prev, +1 = down/next)."""
        if self.link_mode != "rigwire":
            return
        d = max(-127, min(127, int(delta)))
        if d == 0:
            return
        if self.demo_mode:
            if self.headless:
                self.events.emit("link.audio_notes", {"notes": [(1046, 20)]}, source="link")
            self.demo_freq_khz = max(18000, self.demo_freq_khz + (-d) * 25)
            self.demo_status = "ENC+" if d < 0 else "ENC-"
            self._demo_publish_radio()
            self._demo_render()
            return
        self._cmd_q.put(("encoder", d))
        self._wake_ev.set()

    def send_text(self, text: str) -> None:
        """Send direct UTF-8 characters / control keys (BS, Enter, Esc) via FACET_INPUT_TEXT."""
        if self.link_mode != "rigwire" or not text:
            return
        if self.demo_mode:
            if self.demo_text is None:
                self.begin_demo_text()
            dt = self.demo_text
            if dt is not None:
                for ch in text:
                    if ch in ("\x08", "\x7f"):
                        dt.buffer = dt.buffer[:-1]
                    elif ch in ("\r", "\n"):
                        dt.done = True
                    elif ch == "\x1b":
                        dt.canceled = True
                        dt.buffer = ""
                    elif ch.isprintable() and len(dt.buffer) < dt.max_len:
                        dt.buffer += ch
                if dt.done or dt.canceled:
                    keep = dt.done
                    txt = self.end_demo_text()
                    self._emit_log(f"Text entry {'saved: ' + repr(txt) if keep else 'canceled'}")
                else:
                    self._demo_render()
            return
        self._cmd_q.put(("text", text))
        self._wake_ev.set()

    def set_frequency_hz(self, hz: int) -> None:
        hz = max(18_000_000, min(1_300_000_000, int(hz)))
        self.radio_state["rx_freq_hz"] = hz
        if self.demo_mode:
            self.demo_freq_khz = hz // 1000
            self.demo_status = "TUNE"
            self._demo_render()
            self._demo_publish_radio()
            return
        if self.connected and self.link_mode == "rigwire":
            self._cmd_q.put(("freq", hz))
            self._wake_ev.set()
            self.events.emit("link.telemetry", self.status_snapshot(), source="link")

    def set_modulation(self, name: str) -> None:
        code = rigwire.MOD_BY_NAME.get(name.upper())
        if code is None:
            raise ValueError(f"Unknown modulation '{name}'")
        self.radio_state["mod_name"] = rigwire.MOD_NAMES[code]
        if self.demo_mode:
            self._demo_render()
            self.events.emit("link.telemetry", self.status_snapshot(), source="link")
            return
        if self.connected and self.link_mode == "rigwire":
            self._cmd_q.put(("mod", code))
            self._wake_ev.set()
            self.events.emit("link.telemetry", self.status_snapshot(), source="link")

    def set_squelch(self, level: int) -> None:
        level = max(0, min(9, int(level)))
        self.radio_state["squelch"] = level
        if self.demo_mode:
            self.events.emit("link.telemetry", self.status_snapshot(), source="link")
            return
        if self.connected and self.link_mode == "rigwire":
            self._cmd_q.put(("sql", level))
            self._wake_ev.set()
            self.events.emit("link.telemetry", self.status_snapshot(), source="link")

    def set_flashlight(self, enabled: bool) -> None:
        self.torch_on = bool(enabled)
        if self.demo_mode:
            self._emit_log(f"Flashlight {'ON' if self.torch_on else 'OFF'}")
            self.events.emit("link.frame", {}, source="link")
            self.events.emit("link.telemetry", self.status_snapshot(), source="link")
            return
        if self.connected and self.link_mode == "rigwire":
            self._cmd_q.put(("torch", 1 if self.torch_on else 0))
            self._wake_ev.set()
            self.events.emit("link.frame", {}, source="link")
            self.events.emit("link.telemetry", self.status_snapshot(), source="link")

    def set_keypad_lock(self, locked: bool) -> None:
        self.keypad_locked = bool(locked)
        if self.demo_mode:
            self._emit_log(f"Keypad lock {'ON' if self.keypad_locked else 'OFF'}")
            self.events.emit("link.telemetry", self.status_snapshot(), source="link")
            return
        if self.connected and self.link_mode == "rigwire":
            self._cmd_q.put(("lock", self.keypad_locked))
            self._wake_ev.set()
            self.events.emit("link.telemetry", self.status_snapshot(), source="link")

    def set_headless(self, enabled: bool) -> None:
        self.headless = bool(enabled)
        if self.demo_mode:
            self._emit_log(f"Headless mode {'ON (LCD & backlight off)' if self.headless else 'OFF (LCD active)'}")
            self.events.emit("link.frame", {}, source="link")
            self.events.emit("link.telemetry", self.status_snapshot(), source="link")
            return
        if self.connected and self.link_mode == "rigwire":
            self._cmd_q.put(("headless", 1 if self.headless else 0))
            self._wake_ev.set()
            self.events.emit("link.frame", {}, source="link")
            self.events.emit("link.telemetry", self.status_snapshot(), source="link")

    def toggle_vfo(self) -> None:
        if self.demo_mode:
            self.demo_freq_khz, self.demo_freq_b_khz = self.demo_freq_b_khz, self.demo_freq_khz
            self.radio_state["active_vfo"] = 0 if self.radio_state.get("active_vfo") else 1
            self.demo_status = "SWAP"
            self._demo_render()
            self._demo_publish_radio()
            self._emit_log("Swapped active VFO")
            return
        if self.connected and self.link_mode == "rigwire":
            self._cmd_q.put(("vfo", None))
            self._wake_ev.set()

    def run_wire_command(self, cmd_line: str) -> str:
        parts = cmd_line.strip().split()
        if not parts:
            return ""
        op = parts[0].lower()
        if op == "help":
            return (
                "Commands: ping, info, hello, ls [dir], stat <path>, key <name> [press|hold|release], "
                "reboot [firmware|bootloader|recovery], vfo, freq <MHz>, mode <FM|AM|USB|…>, "
                "sql <0-9>, torch <0|1>, lock <0|1>, headless <0|1>, audit, clear"
            )
        if op == "audit":
            entries = self.security.audit_entries(20)
            return "\n".join(f"  [{e.ts_wall}] {e.category:8s} {e.outcome:5s} {e.action}: {e.detail}" for e in entries) or "(audit log empty)"
        if op == "info":
            t = self.last_telemetry
            mv = t.get("battery_mv", 0)
            pct = t.get("battery_pct", 0)
            return (
                f"{self.volume_name} ({self.detected_model.upper()}) on {self.port}\n"
                f"  Link: {self.link_mode}{' (headless)' if self.headless else ''} · {self.fps:.0f} FPS · {self.rtt_ms:.1f} ms RTT · {self.error_count} err\n"
                f"  Battery: {mv / 1000.0:.2f} V ({pct}%) · Torch: {'ON' if self.torch_on else 'OFF'} · Lock: {'ON' if self.keypad_locked else 'OFF'}\n"
                f"  Security: {self.security.profile.value.upper()} · Status LED: {self.status_led.upper()}"
            )
        if not self.connected or self.link_mode != "rigwire":
            return f"RigWire unavailable (mode={self.link_mode})."
        if op == "ping":
            if self.demo_mode:
                return "PONG (demo, 0.2 ms)"
            with self.lock:
                t0 = time.monotonic()
                assert self.client is not None
                self.client.ping()
                self.rtt_ms = (time.monotonic() - t0) * 1000.0
                return f"PONG ({self.rtt_ms:.1f} ms)"
        if op == "hello":
            if self.demo_mode:
                return str(self.hello_info)
            with self.lock:
                assert self.client is not None
                self.hello_info = self.client.hello()
                return str(self.hello_info)
        if op == "ls":
            path = parts[1] if len(parts) > 1 else "/"
            assert self.fs_backend is not None
            items = self.fs_backend.ls(path)
            return "\n".join(f"  {'d' if e['is_dir'] else '-'}  {e['size']:>7} B  {e['name']}" for e in items) or f"{path}: (empty)"
        if op == "stat" and len(parts) > 1:
            assert self.fs_backend is not None
            return str(self.fs_backend.stat(parts[1]))
        if op == "key" and len(parts) > 1:
            kn = parts[1].lower()
            st_n = parts[2].lower() if len(parts) > 2 else "press"
            if kn not in rigwire.KEY_CODES or st_n not in rigwire.KEY_STATES:
                return f"Invalid key/state. Keys: {', '.join(rigwire.KEY_CODES.keys())}"
            try:
                self.send_key(kn, st_n)
            except PermissionError as e:
                return str(e)
            return f"Sent key {kn} ({st_n})"
        if op == "vfo":
            self.toggle_vfo()
            return "Swapped VFO"
        if op == "reboot":
            arg = parts[1].lower() if len(parts) > 1 else "firmware"
            mode = next((m for m, (_l, alias) in REBOOT_MODES.items() if arg in alias), None)
            if mode is None:
                return "Usage: reboot [firmware|bootloader|recovery]"
            try:
                self.reboot(mode)
            except (PermissionError, RuntimeError) as e:
                return str(e)
            return REBOOT_MODES[mode][0]
        if op == "freq" and len(parts) > 1:
            try:
                mhz = float(parts[1])
            except ValueError:
                return f"Invalid frequency '{parts[1]}'"
            hz = int(round(mhz * 1_000_000))
            self.set_frequency_hz(hz)
            return f"Tuned to {self.radio_state['rx_freq_hz'] / 1e6:.4f} MHz"
        if op == "mode" and len(parts) > 1:
            try:
                self.set_modulation(parts[1])
            except ValueError as e:
                return f"{e}. Modes: {', '.join(rigwire.MOD_NAMES.values())}"
            return f"Modulation {self.radio_state['mod_name']}"
        if op == "sql" and len(parts) > 1 and parts[1].isdigit():
            self.set_squelch(int(parts[1]))
            return f"Squelch {self.radio_state['squelch']}"
        if op == "torch":
            val = (parts[1] != "0") if len(parts) > 1 else (not self.torch_on)
            self.set_flashlight(val)
            return f"Flashlight {'ON' if val else 'OFF'}"
        if op == "lock":
            val = (parts[1] != "0") if len(parts) > 1 else (not self.keypad_locked)
            self.set_keypad_lock(val)
            return f"Keypad lock {'ON' if val else 'OFF'}"
        if op == "headless":
            val = (parts[1].lower() not in ("0", "off", "false")) if len(parts) > 1 else (not self.headless)
            self.set_headless(val)
            return f"Headless mode {'ON (LCD & backlight off)' if val else 'OFF (LCD active)'}"
        return f"Unknown command '{op}'. Type 'help'."

    # --- WebDAV OS Mount ---
    def start_dav(self, http_port: int = 8765, read_only: bool | None = None) -> str:
        if self.dav_server is not None:
            return f"http://127.0.0.1:{self.dav_port}/"
        if not self.fs_backend:
            raise RuntimeError("Connect a RigWire radio first")
        if read_only is not None and not self.security.hardened:
            self.security.policy.read_only = read_only
        try:
            srv = kernfs_mount.KernWebDAVServer(
                ("127.0.0.1", http_port), self.fs_backend, self.security.policy, volume_name=self.volume_name
            )
        except OSError:
            if http_port == 0:
                raise
            srv = kernfs_mount.KernWebDAVServer(
                ("127.0.0.1", 0), self.fs_backend, self.security.policy, volume_name=self.volume_name
            )
        self.dav_server = srv
        self.dav_port = srv.server_address[1]
        threading.Thread(target=srv.serve_forever, daemon=True, name="kern-dock-webdav").start()
        url = f"http://127.0.0.1:{self.dav_port}/"
        self.security.record("fs", "webdav_mount", "allow", url)
        self._emit_log(f"Mounted '{self.volume_name}' at {url}")
        self.events.emit("dav.changed", {"mounted": True, "url": url, "dav_url": f"dav://127.0.0.1:{self.dav_port}/"}, source="link")
        return url

    def stop_dav(self) -> None:
        if self.dav_unmount_cmd:
            try:
                subprocess.run(self.dav_unmount_cmd, capture_output=True, timeout=5)
            except Exception:
                pass
            self.dav_unmount_cmd = None
        self.dav_open_target = None
        if self.dav_server is not None:
            try:
                self.dav_server.shutdown()
                self.dav_server.server_close()
            except Exception:
                pass
            self.dav_server = None
            self._emit_log("Unmounted WebDAV drive")
            self.events.emit("dav.changed", {"mounted": False, "url": None}, source="link")

    # --- Background Polling ---
    def _drain_priority_cmds(self) -> None:
        while not self._cmd_q.empty():
            try:
                kind, payload = self._cmd_q.get_nowait()
            except queue.Empty:
                break
            if not self.client:
                break
            for attempt in range(2):
                try:
                    if kind == "key":
                        self.client.inject_key(payload[0], payload[1])
                    elif kind == "text":
                        self.client.inject_text(payload)
                    elif kind == "encoder":
                        self.client.inject_encoder(payload)
                    elif kind == "torch":
                        self.client.set_flashlight(payload)
                    elif kind == "lock":
                        self.client.set_keypad_lock(payload)
                    elif kind == "headless":
                        self.headless = self.client.set_headless(payload)
                    elif kind == "vfo":
                        self.client.toggle_vfo()
                    elif kind == "freq":
                        self.client.set_frequency(payload)
                    elif kind == "mod":
                        self.client.set_modulation(payload)
                    elif kind == "sql":
                        self.client.set_squelch(payload)
                    self.rtt_ms = self.client.last_rtt_ms
                    break
                except Exception:
                    if attempt == 0:
                        try:
                            self.client.resync()
                            continue
                        except Exception:
                            pass
                    self.error_count += 1

    def _poll_loop(self) -> None:
        target_dt = 1.0 / 45.0
        frames = 0
        consec_fail = 0
        t_fps = time.monotonic()
        last_telem = time.monotonic()
        last_radio = 0.0
        last_audio = 0.0
        while not self._stop.is_set() and self.connected:
            t_frame = time.monotonic()
            try:
                with self.lock:
                    if not self.client:
                        break
                    self._drain_priority_cmds()
                    fb = self.client.screencap(on_between_chunks=self._drain_priority_cmds, pipeline=1, delta=True)
                    self.rtt_ms = self.client.last_rtt_ms
                    if self.headless and (time.monotonic() - last_audio) >= 0.025:
                        last_audio = time.monotonic()
                        try:
                            notes = self.client.drain_audio_notes()
                            if notes:
                                self.events.emit("link.audio_notes", {"notes": notes}, source="link")
                        except Exception:
                            pass
                consec_fail = 0
                if len(fb) == 1024:
                    changed = fb != self.last_fb
                    self.last_fb = fb
                    frames += 1
                    if changed:
                        self.events.emit("link.frame", {}, source="link")
            except OSError:
                if not self._stop.is_set():
                    self.disconnect(reason="Radio disconnected (USB link lost)")
                break
            except Exception as exc:
                if type(exc).__name__ == "SerialException":
                    if not self._stop.is_set():
                        self.disconnect(reason="Radio disconnected (serial port closed)")
                    break
                recovered = False
                try:
                    with self.lock:
                        if self.client:
                            self.client.resync()
                            recovered = True
                except Exception:
                    pass
                if recovered:
                    consec_fail = 0
                else:
                    self.error_count += 1
                    consec_fail += 1
                    if consec_fail >= 12:
                        if not self._stop.is_set():
                            self.disconnect(reason="Radio stopped responding")
                        break
                    backoff = min(0.50, 0.10 * consec_fail)
                    self._wake_ev.wait(backoff)
                    self._wake_ev.clear()
                continue

            now = time.monotonic()
            if now - t_fps >= 1.0:
                self.fps = frames / (now - t_fps)
                frames = 0
                t_fps = now
                self.events.emit("link.telemetry", self.status_snapshot(), source="link")

            if now - last_radio >= 1.0:
                last_radio = now
                try:
                    with self.lock:
                        if self.client:
                            self.radio_state.update(self.client.get_radio_telemetry())
                except Exception:
                    pass

            if now - last_telem >= 0.20:
                last_telem = now
                try:
                    prev_led = self.status_led
                    prev_torch = self.torch_on
                    with self.lock:
                        if self.client:
                            met = self.client.handle_get(rigwire.DOMAIN_SYSTEM, 0, rigwire.FACET_SYS_METRICS)
                            if len(met) >= 14:
                                self.last_telemetry["uptime_ms"] = struct.unpack("<I", met[:4])[0]
                                self.last_telemetry["sysclk_hz"] = struct.unpack("<I", met[4:8])[0]
                                self.last_telemetry["battery_mv"] = struct.unpack("<H", met[8:10])[0]
                                self.last_telemetry["battery_pct"] = met[10]
                                self.last_telemetry["active_app"] = met[11]
                                self.keypad_locked = bool(met[12])
                                self.torch_on = bool(met[13])
                                if len(met) >= 15:
                                    led_bits = met[14] & 0x03
                                    self.status_led = ("off", "tx", "rx", "orange")[led_bits]
                                    self.last_telemetry["status_led_raw"] = led_bits
                                    if len(met) >= 17:
                                        self.bk4829_gpio = struct.unpack("<H", met[15:17])[0]
                                        self.last_telemetry["bk4829_gpio"] = self.bk4829_gpio
                                    if len(met) >= 18:
                                        self.headless = bool(met[17])
                                        self.last_telemetry["headless"] = self.headless
                                    if len(met) >= 21:
                                        self.last_telemetry["display_backlight"] = min(10, int(met[18]))
                                        self.last_telemetry["display_contrast_pct"] = min(100, int(met[19]))
                                        self.last_telemetry["display_inverted"] = bool(met[20])
                                else:
                                    tx_b = self.client.handle_get(rigwire.DOMAIN_RADIO, rigwire.RADIO_INSTANCE_ACTIVE, rigwire.FACET_RADIO_TRANSMIT)
                                    sq_b = self.client.handle_get(rigwire.DOMAIN_RADIO, rigwire.RADIO_INSTANCE_ACTIVE, rigwire.FACET_RADIO_SQUELCH)
                                    is_tx = bool(tx_b[0]) if len(tx_b) >= 1 else False
                                    sq_open = bool(sq_b[3]) if len(sq_b) >= 4 else False
                                    self.status_led = "tx" if is_tx else ("rx" if sq_open else "off")
                            if self.headless:
                                try:
                                    notes = self.client.drain_audio_notes()
                                    if notes:
                                        self.events.emit("link.audio_notes", {"notes": notes}, source="link")
                                except Exception:
                                    pass
                    if self.status_led != prev_led or self.torch_on != prev_torch:
                        self.events.emit("link.frame", {}, source="link")
                    self.events.emit("link.telemetry", self.status_snapshot(), source="link")
                except Exception:
                    pass

            rem = target_dt - (time.monotonic() - t_frame)
            if rem > 0.001 and self._cmd_q.empty():
                self._wake_ev.wait(rem)
            self._wake_ev.clear()
