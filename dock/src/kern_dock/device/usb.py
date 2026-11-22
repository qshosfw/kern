"""USB port discovery: GUdev hotplug (pyserial fallback), DFU / CPS classification, model detection."""

from __future__ import annotations

import dataclasses
import time
from typing import Any, Callable

try:
    import flasher as _flasher
except Exception:
    _flasher = None  # type: ignore[assignment]

VID_PID_STOCK_BL = "36b7:0001"
VID_PID_KERN_CDC = "36b7:ffff"


@dataclasses.dataclass(frozen=True)
class DetectedPort:
    device: str
    pretty_name: str
    vendor: str = ""
    model: str = ""
    serial: str = ""
    vid: str = ""
    pid: str = ""
    mode_hint: str = "rigwire"  # "rigwire" | "stock_bl"
    is_quansheng: bool = False


def classify_usb_mode(vid: str, pid: str, pretty_name: str) -> tuple[str, bool]:
    """
    Classifies USB serial devices:
      • 36b7:ffff -> Native USB-C CDC (RigWire / firmware runtime)
      • 36b7:0001 -> Quansheng K1/K5v3 CPS or Stock Bootloader (live-probed on connect)
      • 1a86:7523 / 10c4:ea60 / 0403:6001 / 067b:2303 -> CH340/CP2102/FT232/PL2303 UART / programming cable
    """
    vp = f"{vid}:{pid}".lower() if (vid and pid) else ""
    low = pretty_name.lower()
    if vp == VID_PID_KERN_CDC:
        return "rigwire", True
    if vp == VID_PID_STOCK_BL:
        return "stock_bl", True
    if vp in ("1a86:7523", "1a86:5523", "1a86:55d4", "10c4:ea60", "0403:6001", "067b:2303"):
        return "stock_bl", True
    is_qs = any(k in low for k in ("quansheng", "uv-k", "kern", "rigwire", "ch340", "ch341"))
    return "rigwire", is_qs


class PortMonitor:
    """Real-time USB hotplug monitor recognizing RigWire USB-C CDC and Quansheng Bootloader ports."""

    def __init__(self, on_change: Callable[[list[DetectedPort]], None] | None = None) -> None:
        self.on_change = on_change
        self.ports: list[DetectedPort] = []
        self._gudev_client: Any = None
        try:
            import gi

            gi.require_version("GUdev", "1.0")
            from gi.repository import GUdev

            self._gudev_client = GUdev.Client(subsystems=["tty"])
            self._gudev_client.connect("uevent", lambda _c, _act, _dev: self.scan())
        except Exception:
            self._gudev_client = None
        self.scan()

    def scan(self) -> list[DetectedPort]:
        found: list[DetectedPort] = []
        if self._gudev_client is not None:
            for dev in self._gudev_client.query_by_subsystem("tty"):
                dev_file = dev.get_device_file()
                if not dev_file:
                    continue
                name = dev.get_name() or ""
                if not ((dev.get_property("ID_BUS") == "usb") or ("USB" in name) or ("ACM" in name)):
                    continue
                vid = pid = vendor = model = ser = ""
                cur = dev
                for _ in range(10):
                    if cur is None:
                        break
                    vid = vid or cur.get_property("ID_VENDOR_ID") or cur.get_property("ID_USB_VENDOR_ID") or cur.get_sysfs_attr("idVendor") or ""
                    pid = pid or cur.get_property("ID_MODEL_ID") or cur.get_property("ID_USB_MODEL_ID") or cur.get_sysfs_attr("idProduct") or ""
                    vendor = vendor or cur.get_property("ID_VENDOR") or cur.get_sysfs_attr("manufacturer") or ""
                    model = model or cur.get_property("ID_MODEL") or cur.get_sysfs_attr("product") or ""
                    ser = ser or cur.get_property("ID_SERIAL_SHORT") or cur.get_sysfs_attr("serial") or ""
                    if vid and pid and vendor and model:
                        break
                    cur = cur.get_parent()
                v_c, m_c = vendor.replace("_", " ").strip(), model.replace("_", " ").strip()
                pretty = m_c if m_c.lower().startswith("quansheng") else f"{v_c} {m_c}".strip() or dev_file
                mode_hint, is_qs = classify_usb_mode(vid, pid, pretty)
                found.append(DetectedPort(dev_file, pretty, v_c, m_c, ser, vid, pid, mode_hint, is_qs))
        else:
            try:
                from serial.tools import list_ports

                for p in list_ports.comports():
                    desc = (p.product or p.description or "").strip()
                    vid = f"{p.vid:04x}" if p.vid is not None else ""
                    pid = f"{p.pid:04x}" if p.pid is not None else ""
                    mode_hint, is_qs = classify_usb_mode(vid, pid, desc)
                    found.append(
                        DetectedPort(
                            p.device,
                            desc or p.device,
                            model=desc,
                            serial=p.serial_number or "",
                            vid=vid,
                            pid=pid,
                            mode_hint=mode_hint,
                            is_quansheng=is_qs,
                        )
                    )
            except Exception:
                pass

        found.sort(key=lambda d: (0 if d.is_quansheng else 1, d.device))
        changed = [(p.device, p.mode_hint) for p in found] != [(p.device, p.mode_hint) for p in self.ports]
        self.ports = found
        if changed and self.on_change is not None:
            self.on_change(found)
        return found


def probe_bootloader_beacon(port: str, duration_s: float = 0.85) -> dict[str, str] | None:
    """Listens briefly at 38400 baud for Quansheng stock bootloader / CPS 0x0518 beacon packets."""
    if _flasher is None or port.startswith("tcp://") or port == "demo":
        return None
    try:
        import serial

        ser = serial.Serial(port, baudrate=_flasher.BAUDRATE, timeout=0.0001)
    except Exception:
        return None
    try:
        ser.reset_input_buffer()
        proto = _flasher.Protocol(port)
        proto.ser = ser
        deadline = time.monotonic() + duration_s
        while time.monotonic() < deadline:
            msg = proto.fetch_message()
            if msg and msg[0] == _flasher.MSG_NOTIFY_DEV_INFO:
                data = msg[1]
                uid = data[:16].hex()
                ver = data[16:].split(b"\x00")[0].decode("utf-8", errors="ignore").strip()
                return {"version": ver or "Stock BL", "uid": uid}
            time.sleep(0.01)
    finally:
        try:
            ser.close()
        except Exception:
            pass
    return None


def detect_hardware_model(
    volume_name: str,
    hello: dict[str, Any] | None = None,
    port_info: DetectedPort | None = None,
) -> str:
    blob = " ".join(
        [
            volume_name or "",
            (port_info.pretty_name if port_info else ""),
            (port_info.model if port_info else ""),
        ]
    ).lower()
    if "k10" in blob:
        return "k10-pro"
    if "5r" in blob:
        return "uv5r-plus"
    if "k5(99)" in blob or "k5-99" in blob or "k599" in blob:
        return "k5-99"
    if "k6" in blob or "k5" in blob:
        return "k5v3"
    if "k1" in blob:
        return "k1"
    if hello and (int(hello.get("hardware_profile", 0) or 0) & 0xFF) == 0x05:
        return "k5v3"
    return "k1"
