"""Raw hardware control without RigWire: SWD debug probes, USB power management & raw UART lines."""

from __future__ import annotations

import dataclasses
import os
import shutil
import subprocess
import tempfile
import threading
import time
from pathlib import Path
from typing import Any, Callable

import kfw

SWD_PROBE_IDS: dict[tuple[str, str], tuple[str, str, str]] = {
    ("0483", "3748"): ("ST-Link/V2", "stlink", "interface/stlink.cfg"),
    ("0483", "374b"): ("ST-Link/V2-1", "stlink", "interface/stlink.cfg"),
    ("0483", "3752"): ("ST-Link/V2-1", "stlink", "interface/stlink.cfg"),
    ("0483", "374e"): ("ST-Link/V3", "stlink", "interface/stlink.cfg"),
    ("0483", "374f"): ("ST-Link/V3", "stlink", "interface/stlink.cfg"),
    ("0483", "3753"): ("ST-Link/V3", "stlink", "interface/stlink.cfg"),
    ("0483", "3754"): ("ST-Link/V3", "stlink", "interface/stlink.cfg"),
    ("0d28", "0204"): ("ARM CMSIS-DAP", "cmsis-dap", "interface/cmsis-dap.cfg"),
    ("2e8a", "000c"): ("Raspberry Pi Debug Probe", "cmsis-dap", "interface/cmsis-dap.cfg"),
    ("1fc9", "0143"): ("NXP MCU-Link", "cmsis-dap", "interface/cmsis-dap.cfg"),
}

UART_BRIDGE_IDS: dict[tuple[str, str], str] = {
    ("1a86", "7523"): "WCH CH340",
    ("1a86", "5523"): "WCH CH341",
    ("1a86", "55d4"): "WCH CH9102",
    ("1a86", "55d3"): "WCH CH343",
    ("10c4", "ea60"): "Silicon Labs CP2102/CP210x",
    ("10c4", "ea70"): "Silicon Labs CP2105",
    ("0403", "6001"): "FTDI FT232R",
    ("0403", "6010"): "FTDI FT2232",
    ("0403", "6014"): "FTDI FT232H",
    ("0403", "6015"): "FTDI FT-X",
    ("067b", "2303"): "Prolific PL2303",
    ("067b", "23a3"): "Prolific PL2303GC",
    ("36b7", "0001"): "Quansheng CPS / Bootloader",
    ("36b7", "ffff"): "Quansheng RigWire CDC",
}

SWD_TARGETS: dict[str, dict[str, Any]] = {
    "py32f071": {
        "label": "PY32F071xB (Cortex-M0+ · 128 KB)",
        "openocd_cfg": "target/stm32f0x.cfg",
        "flash_base": 0x08000000,
        "flash_size": 128 * 1024,
        "sram_base": 0x20000000,
        "sram_size": 16 * 1024,
    },
    "cortex_m": {
        "label": "Generic Cortex-M0+ (SWD)",
        "openocd_cfg": "target/stm32f0x.cfg",
        "flash_base": 0x08000000,
        "flash_size": 128 * 1024,
        "sram_base": 0x20000000,
        "sram_size": 16 * 1024,
    },
}

_KNOWN_SERIAL_VIDS = {"36b7", "1a86", "10c4", "0403", "067b", "0483", "0d28", "2e8a", "1366"}


@dataclasses.dataclass(frozen=True)
class SwdProbe:
    id: str
    name: str
    vendor: str
    product: str
    vid: str
    pid: str
    serial: str
    driver: str
    openocd_iface: str
    sysfs_path: str


@dataclasses.dataclass(frozen=True)
class UsbPowerDevice:
    bus_port: str
    name: str
    vid: str
    pid: str
    serial: str
    max_power_ma: int
    power_control: str
    runtime_status: str
    authorized: bool
    tty_dev: str
    is_probe: bool
    sysfs_path: str
    chip_name: str = ""


def available_hw_tools() -> dict[str, str | None]:
    """Check which host SWD and USB power utilities are installed."""
    return {
        "openocd": shutil.which("openocd"),
        "probe_rs": shutil.which("probe-rs"),
        "pyocd": shutil.which("pyocd"),
        "st_flash": shutil.which("st-flash"),
        "st_info": shutil.which("st-info"),
        "uhubctl": shutil.which("uhubctl"),
    }


def _read_sysfs_text(path: Path, default: str = "") -> str:
    try:
        return path.read_text(encoding="utf-8", errors="ignore").strip()
    except OSError:
        return default


def classify_uart_chip(vid: str, pid: str, product: str = "") -> str:
    """Identify common USB-UART bridge chips (CH340, CP2102, FT232R, PL2303, etc.)."""
    key = (vid.lower(), pid.lower())
    if key in UART_BRIDGE_IDS:
        return UART_BRIDGE_IDS[key]
    low = product.lower()
    for token, label in (
        ("ch340", "WCH CH340"),
        ("ch341", "WCH CH341"),
        ("cp210", "Silicon Labs CP210x"),
        ("ft232", "FTDI FT232"),
        ("pl2303", "Prolific PL2303"),
    ):
        if token in low:
            return label
    return ""


def _classify_swd_probe(vid: str, pid: str, product: str, vendor: str) -> tuple[str, str, str] | None:
    v, p = vid.lower(), pid.lower()
    if (v, p) in SWD_PROBE_IDS:
        return SWD_PROBE_IDS[(v, p)]
    if v == "1366":
        return (product or "SEGGER J-Link", "jlink", "interface/jlink.cfg")
    blob = f"{vendor} {product}".lower()
    if "cmsis-dap" in blob or "daplink" in blob or "picoprobe" in blob or "debug probe" in blob:
        return (product or "CMSIS-DAP Probe", "cmsis-dap", "interface/cmsis-dap.cfg")
    if "st-link" in blob or "stlink" in blob:
        return (product or "ST-Link Probe", "stlink", "interface/stlink.cfg")
    if "j-link" in blob:
        return (product or "J-Link Probe", "jlink", "interface/jlink.cfg")
    return None


def _find_tty_under(usb_dev: Path) -> str:
    try:
        for p in usb_dev.glob("*/tty*/tty/*"):
            return f"/dev/{p.name}"
        for p in usb_dev.glob("*/tty/*"):
            return f"/dev/{p.name}"
    except OSError:
        pass
    return ""


def discover_swd_probes() -> list[SwdProbe]:
    """Discover connected SWD debug probes (CMSIS-DAP, ST-Link, J-Link) via Linux USB sysfs."""
    usb_root = Path("/sys/bus/usb/devices")
    if not usb_root.is_dir():
        return []
    probes: list[SwdProbe] = []
    for dev in sorted(usb_root.iterdir(), key=lambda x: x.name):
        if ":" in dev.name or not (dev / "idVendor").is_file():
            continue
        vid = _read_sysfs_text(dev / "idVendor").lower()
        pid = _read_sysfs_text(dev / "idProduct").lower()
        product = _read_sysfs_text(dev / "product")
        vendor = _read_sysfs_text(dev / "manufacturer")
        serial = _read_sysfs_text(dev / "serial")
        match = _classify_swd_probe(vid, pid, product, vendor)
        if match is None:
            continue
        label, driver, iface = match
        probes.append(
            SwdProbe(
                id=f"{dev.name}:{vid}:{pid}",
                name=product or label,
                vendor=vendor,
                product=product or label,
                vid=vid,
                pid=pid,
                serial=serial,
                driver=driver,
                openocd_iface=iface,
                sysfs_path=str(dev),
            )
        )
    return probes


def discover_usb_power_devices() -> list[UsbPowerDevice]:
    """Discover connected radios, SWD probes & USB-UART modules with their USB power state."""
    usb_root = Path("/sys/bus/usb/devices")
    if not usb_root.is_dir():
        return []
    out: list[UsbPowerDevice] = []
    for dev in sorted(usb_root.iterdir(), key=lambda x: x.name):
        if ":" in dev.name or not (dev / "idVendor").is_file():
            continue
        vid = _read_sysfs_text(dev / "idVendor").lower()
        pid = _read_sysfs_text(dev / "idProduct").lower()
        product = _read_sysfs_text(dev / "product")
        vendor = _read_sysfs_text(dev / "manufacturer")
        serial = _read_sysfs_text(dev / "serial")
        tty_dev = _find_tty_under(dev)
        is_probe = _classify_swd_probe(vid, pid, product, vendor) is not None
        if not (is_probe or tty_dev or vid in _KNOWN_SERIAL_VIDS):
            continue
        max_p_raw = _read_sysfs_text(dev / "bMaxPower", "0mA").lower().rstrip("ma").strip()
        try:
            max_ma = int(max_p_raw)
        except ValueError:
            max_ma = 0
        pctrl = _read_sysfs_text(dev / "power" / "control", "on")
        rstat = _read_sysfs_text(dev / "power" / "runtime_status", "active")
        auth = _read_sysfs_text(dev / "authorized", "1") != "0"
        chip = classify_uart_chip(vid, pid, product)
        name = product or chip or f"{vendor} {vid}:{pid}".strip()
        out.append(
            UsbPowerDevice(
                bus_port=dev.name,
                name=name,
                vid=vid,
                pid=pid,
                serial=serial,
                max_power_ma=max_ma,
                power_control=pctrl,
                runtime_status=rstat,
                authorized=auth,
                tty_dev=tty_dev,
                is_probe=is_probe,
                sysfs_path=str(dev),
                chip_name=chip,
            )
        )
    return out


def set_usb_power_control(sysfs_path: str, mode: str) -> tuple[bool, str]:
    """Set `/sys/bus/usb/devices/<dev>/power/control` to `'on'` or `'auto'`."""
    if mode not in ("on", "auto"):
        return False, f"Invalid power mode: {mode}"
    target = Path(sysfs_path) / "power" / "control"
    try:
        target.write_text(mode + "\n", encoding="utf-8")
        return True, f"USB power control set to '{mode}'"
    except PermissionError:
        return False, f"Permission denied writing {target} (run udev rule or pkexec)"
    except OSError as exc:
        return False, str(exc)


def cycle_usb_authorized(sysfs_path: str) -> tuple[bool, str]:
    """Reset/re-enumerate a USB device by toggling its sysfs `authorized` attribute."""
    target = Path(sysfs_path) / "authorized"
    try:
        target.write_text("0\n", encoding="utf-8")
        time.sleep(0.25)
        target.write_text("1\n", encoding="utf-8")
        return True, "USB device re-authorized"
    except PermissionError:
        return False, f"Permission denied writing {target} (run udev rule or pkexec)"
    except OSError as exc:
        return False, str(exc)


def uhubctl_power(action: str, location: str = "", port: str = "") -> tuple[bool, str]:
    """Control physical 5V USB VBUS power via `uhubctl` when available."""
    exe = shutil.which("uhubctl")
    if not exe:
        return False, "uhubctl not installed"
    act_map = {"on": "on", "off": "off", "cycle": "cycle"}
    if action not in act_map:
        return False, f"Unsupported uhubctl action: {action}"
    cmd = [exe, "-a", act_map[action]]
    if location:
        cmd.extend(["-l", location])
    if port:
        cmd.extend(["-p", port])
    try:
        proc = subprocess.run(cmd, capture_output=True, text=True, timeout=10)
        out = ((proc.stdout or "") + "\n" + (proc.stderr or "")).strip()
        return proc.returncode == 0, out or f"uhubctl {action} completed"
    except Exception as exc:
        return False, str(exc)


def _extract_fw_bin_for_swd(fpath: Path, tmp_dir: Path) -> Path:
    data = fpath.read_bytes()
    if data[:8] == kfw.FILE_ID_KFW:
        parsed = kfw.parse_kfw(data, verify_root=True)
        pts = kfw.extract_all_parts(parsed)
        fw_parts = [pt for pt in parsed.parts if pt["kind"] == kfw.PART_FIRMWARE]
        if not fw_parts:
            raise ValueError("KFW container has no firmware part")
        out = tmp_dir / (fpath.stem + ".bin")
        out.write_bytes(pts[fw_parts[0]["index"]])
        return out
    return fpath


def run_swd_action(
    probe: SwdProbe,
    target_key: str,
    action: str,
    *,
    fw_path: Path | None = None,
) -> tuple[bool, str, bytes | None]:
    """Execute a raw SWD debug/flash/dump operation via OpenOCD without needing RigWire."""
    openocd = shutil.which("openocd")
    if not openocd:
        return False, "openocd not found on PATH", None

    tinfo = SWD_TARGETS.get(target_key, SWD_TARGETS["py32f071"])
    iface = probe.openocd_iface
    target_cfg = tinfo["openocd_cfg"]

    with tempfile.TemporaryDirectory() as td:
        td_path = Path(td)
        dump_out = td_path / "dump.bin"
        extra_cmds: list[str] = []

        if action == "probe":
            script = "init; dap info; mdw 0xE000ED00 1; exit"
        elif action == "reset":
            script = "init; reset run; exit"
        elif action == "halt":
            script = "init; halt; reg; exit"
        elif action == "resume":
            script = "init; resume; exit"
        elif action == "dump_flash":
            script = f"init; halt; dump_image {dump_out} 0x{tinfo['flash_base']:08x} 0x{tinfo['flash_size']:x}; resume; exit"
        elif action == "dump_sram":
            script = f"init; halt; dump_image {dump_out} 0x{tinfo['sram_base']:08x} 0x{tinfo['sram_size']:x}; resume; exit"
        elif action == "flash_bin":
            if fw_path is None:
                return False, "No firmware file selected", None
            bin_path = _extract_fw_bin_for_swd(fw_path, td_path)
            script = (
                f"init; reset halt; "
                f"flash write_image erase {bin_path} 0x{tinfo['flash_base']:08x}; "
                f"verify_image {bin_path} 0x{tinfo['flash_base']:08x}; "
                f"reset run; exit"
            )
        else:
            return False, f"Unknown SWD action: {action}", None

        cmd = [
            openocd,
            "-f",
            iface,
            "-c",
            "transport select swd",
            "-f",
            target_cfg,
            *extra_cmds,
            "-c",
            script,
        ]
        try:
            proc = subprocess.run(cmd, capture_output=True, text=True, timeout=30)
        except Exception as exc:
            return False, str(exc), None

        combined = ((proc.stdout or "") + "\n" + (proc.stderr or "")).strip()
        blob: bytes | None = None
        if proc.returncode == 0 and dump_out.is_file():
            blob = dump_out.read_bytes()
        return proc.returncode == 0, combined, blob


def parse_raw_tx_input(text: str) -> bytes:
    """Parse raw TX input: `hex: 01 aa ff` or `0x01 0xaa` sends raw bytes; otherwise UTF-8 + `\\r\\n` escapes."""
    s = text.strip()
    if s.lower().startswith("hex:"):
        tokens = s[4:].replace(",", " ").split()
        return bytes(int(tok, 16) for tok in tokens)
    # Support \r and \n escapes in ASCII mode
    return s.encode("utf-8").decode("unicode_escape").encode("latin-1")


class RawSerialSession:
    """Direct raw UART & physical DTR/RTS pin controller (independent of RigWire)."""

    def __init__(self, on_rx: Callable[[bytes], None] | None = None, on_error: Callable[[str], None] | None = None) -> None:
        self._on_rx = on_rx
        self._on_error = on_error
        self._ser: Any = None
        self._stop = threading.Event()
        self._thread: threading.Thread | None = None
        self.port: str = ""
        self.baudrate: int = 115200

    @property
    def is_open(self) -> bool:
        return self._ser is not None and bool(getattr(self._ser, "is_open", False))

    def open(self, port: str, baudrate: int = 115200, *, dtr: bool = False, rts: bool = False) -> None:
        import serial

        self.close()
        ser = serial.Serial(port, baudrate=baudrate, timeout=0.05, write_timeout=1.0)
        ser.dtr = dtr
        ser.rts = rts
        self._ser = ser
        self.port = port
        self.baudrate = baudrate
        self._stop.clear()
        self._thread = threading.Thread(target=self._rx_loop, daemon=True)
        self._thread.start()

    def close(self) -> None:
        self._stop.set()
        ser = self._ser
        self._ser = None
        if ser is not None:
            try:
                ser.close()
            except Exception:
                pass

    def set_dtr(self, state: bool) -> None:
        if self._ser is not None:
            self._ser.dtr = state

    def set_rts(self, state: bool) -> None:
        if self._ser is not None:
            self._ser.rts = state

    def pulse_line(self, line: str = "dtr", active_high: bool = True, duration_s: float = 0.08) -> None:
        if self._ser is None:
            return
        setter = self.set_dtr if line == "dtr" else self.set_rts
        setter(active_high)
        time.sleep(max(0.01, min(1.0, duration_s)))
        setter(not active_high)

    def send_break(self, duration_s: float = 0.25) -> None:
        if self._ser is not None:
            self._ser.send_break(duration=duration_s)

    def write(self, data: bytes) -> None:
        if self._ser is not None and data:
            self._ser.write(data)
            self._ser.flush()

    def _rx_loop(self) -> None:
        while not self._stop.is_set():
            ser = self._ser
            if ser is None:
                break
            try:
                n = ser.in_waiting
                chunk = ser.read(max(1, min(512, n))) if n or True else b""
                if chunk and self._on_rx is not None:
                    self._on_rx(chunk)
            except Exception as exc:
                if not self._stop.is_set() and self._on_error is not None:
                    self._on_error(str(exc))
                break
