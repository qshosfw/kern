#!/usr/bin/env python3
"""
⚡ rigwire - Developer & Host CLI Tool for RigWire Serial Protocol
Implements RigWire Microkernel Resource Handle Architecture (Core 0x00, Control 0x01, Events 0x02, Data 0x03).

Features:
- Universal Resource Handle RPC: Domain (0x00-0x06), Instance, Facet architecture.
- Full RF Arbitration & Tuning: Direct coordination through microkernel RadioCoordinator.
- Front Panel & ABI Control: Remote screen mirroring (screencap), keypad event injection, modal dialog interaction.
- Hardened Data Transfer: Chunked streaming push/pull with IEEE 802.3 CRC-32 verification and 8 KiB safety ceiling.
- Zero-Bloat: Standard, robust file transfers without hardcoded application or file types.
- Protocol self-test: Built-in verification against normative specification test vectors.
"""

import argparse
import glob
import os
import re
import socket
import struct
import sys
import time
import binascii
import zlib

try:
    import serial
except ImportError:
    serial = None

# ── WIRE CONSTANTS ────────────────────────────────────────────────────────────
WIRE_MAGIC = 0x52  # 'R'
WIRE_MAJOR = 0x01

FLAG_RESPONSE = 0x01
FLAG_EVENT = 0x02

HEADER_LEN = 15
CRC_LEN = 2
MIN_RECORD_LEN = 17
MAX_WIRE_FILE_SIZE = 262144
MAX_APP_PATH_LEN = 24
MAX_CATALOG_PACKAGES = 8

KRX_MAGIC = b"\xC0\xDE\x01\xFE"
KRX_CLASSES = {0: "Native", 1: "Paged", 2: "VMachine", 3: "Declare"}
KRX_EXEC_TYPES = {1: "App", 2: "Service", 3: "Daemon", 4: "Driver", 5: "Module"}
KRX_PROFILES = {
    0x10: "N0 (NativeFixed)",
    0x11: "P0 (StructuredPaged)",
    0x12: "N1 (NativePic)",
    0x13: "P1 (TransparentPaged)",
    0x20: "V0 (ResidentVm)",
    0x30: "D0 (DeclareGraph)",
    0x31: "F0 (FlowApp)",
}

# Service Domains (§8.5)
SERVICE_CORE = 0x00
SERVICE_CONTROL = 0x01
SERVICE_EVENTS = 0x02
SERVICE_DATA = 0x03

# Core Opcodes (§5 / §8.5)
OP_CORE_HELLO = 0x01
OP_CORE_GET_CAPS = 0x02
OP_CORE_PING = 0x03
OP_CORE_GET_STATUS = 0x04
OP_CORE_GET_RESULT = 0x05
OP_CORE_REBOOT = 0x06
OP_CORE_TIME_SYNC = 0x07
OP_CORE_GET_TIME = 0x08

# Universal Handle Opcodes (§8.5)
OP_HANDLE_GET = 0x01
OP_HANDLE_SET = 0x02
OP_HANDLE_INVOKE = 0x03
OP_HANDLE_STREAM = 0x04

# Resource Domains (§8.5)
DOMAIN_SYSTEM = 0x00
DOMAIN_RADIO = 0x01
DOMAIN_UI = 0x02
DOMAIN_INPUT = 0x03
DOMAIN_CONFIG = 0x04
DOMAIN_APP = 0x05
DOMAIN_STORAGE = 0x06

# DOMAIN_SYSTEM (0x00) Facets & Actions
FACET_SYS_IDENTITY = 0x0001
FACET_SYS_METRICS = 0x0002
FACET_SYS_CALIBRATION = 0x0003
FACET_SYS_AUDIO = 0x0004

ACTION_SYS_REBOOT = 0x0001
ACTION_SYS_FLASHLIGHT = 0x0002
ACTION_SYS_HEADLESS = 0x0004

# DOMAIN_RADIO (0x01) Instances, Facets & Actions
RADIO_INSTANCE_ACTIVE = 0x00
RADIO_INSTANCE_VFO_A = 0x01
RADIO_INSTANCE_VFO_B = 0x02

FACET_RADIO_TUNING = 0x0001
FACET_RADIO_MODULATION = 0x0002
FACET_RADIO_SQUELCH = 0x0003
FACET_RADIO_TRANSMIT = 0x0004
FACET_RADIO_DIGITAL = 0x0005

ACTION_RADIO_VFO_TOGGLE = 0x0001

# DOMAIN_UI (0x02) Facets & Actions
UI_INSTANCE_PRIMARY = 0x00

FACET_UI_SURFACE = 0x0001
FACET_UI_MODAL = 0x0002
FACET_UI_VIEW = 0x0003

ACTION_MODAL_DISMISS = 0x0001
ACTION_MODAL_CONFIRM = 0x0002
ACTION_MODAL_CANCEL = 0x0003
ACTION_VIEW_LAUNCHER = 0x0004

# DOMAIN_INPUT (0x03) Facets & Actions
INPUT_INSTANCE_KEYPAD = 0x00

FACET_INPUT_KEY = 0x0001
FACET_INPUT_LOCK = 0x0002
FACET_INPUT_TEXT = 0x0003
FACET_INPUT_ENCODER = 0x0004

ACTION_INPUT_LOCK_TOGGLE = 0x0001

# DOMAIN_CONFIG (0x04) Instances
CONFIG_DOMAIN_DISPLAY = 0x00
CONFIG_DOMAIN_AUDIO = 0x01
CONFIG_DOMAIN_KEYS = 0x02
CONFIG_DOMAIN_SYSTEM = 0x03
CONFIG_DOMAIN_BATTERY = 0x04
CONFIG_DOMAIN_APPS = 0x05

# DOMAIN_APP (0x05) Facets & Actions
FACET_APP_REGISTRY = 0x0001
FACET_APP_LIFECYCLE = 0x0002

ACTION_APP_LAUNCH = 0x0001
ACTION_APP_KILL = 0x0002

MOD_NAMES = {
    0: "FM",
    1: "AM",
    2: "AMB",
    3: "USB",
    4: "DSB",
    5: "CW",
    6: "WFM",
    7: "PKT",
    8: "DIG",
    9: "RAW",
    10: "BYP",
}
MOD_BY_NAME = {v.upper(): k for k, v in MOD_NAMES.items()}

KEY_CODES = {
    "ptt": 0,
    "side1": 1,
    "side2": 2,
    "up": 3,
    "down": 4,
    "menu": 5,
    "exit": 6,
    "f": 7,
    "0": 8,
    "1": 9,
    "2": 10,
    "3": 11,
    "4": 12,
    "5": 13,
    "6": 14,
    "7": 15,
    "8": 16,
    "9": 17,
    "star": 18,
    "*": 18,
    "hash": 19,
    "#": 19,
}

KEY_STATES = {
    "press": 0,
    "pressed": 0,
    "hold": 1,
    "held": 1,
    "release": 2,
    "released": 2,
}

# Data Opcodes (§8.5)
OP_TRANSFER_OPEN = 0x01
OP_TRANSFER_CHUNK = 0x02
OP_TRANSFER_CLOSE = 0x03
OP_TRANSFER_ABORT = 0x04
OP_TRANSFER_STAT = 0x05
OP_TRANSFER_LIST = 0x06
OP_TRANSFER_REMOVE = 0x07

TRANSFER_MODE_READ = 0
TRANSFER_MODE_WRITE = 1

# Wire Status Codes (§6)
STATUS_NAMES = {
    0: "OK",
    1: "ACCEPTED",
    2: "BAD_LENGTH",
    3: "BAD_RANGE",
    4: "BAD_VERSION",
    5: "BAD_FLAGS",
    6: "BAD_SESSION",
    7: "STALE_SEQUENCE",
    8: "DUPLICATE_CONFLICT",
    9: "UNSUPPORTED",
    10: "BUSY",
    11: "STALE_REVISION",
    12: "STALE_COMPONENT",
    13: "SAFETY_DENIED",
    14: "AUTH_REQUIRED",
    15: "AUTHZ_DENIED",
    16: "NOT_PERSISTABLE",
    17: "UNSUPPORTED_ATOMICITY",
    18: "FAILED",
    19: "IN_PROGRESS",
    20: "STALE_CATALOGUE",
    21: "UNSUPPORTED_FIELD",
    22: "NOT_FOUND",
    23: "IO_ERROR",
    27: "TIMEOUT",
    28: "INTERNAL",
}


# ── PROTOCOL PRIMITIVES ───────────────────────────────────────────────────────
def crc16_ccitt(data: bytes) -> int:
    """CRC-16/CCITT-FALSE (poly 0x1021, init 0xFFFF)."""
    crc = 0xFFFF
    for b in data:
        crc ^= (b << 8)
        for _ in range(8):
            if crc & 0x8000:
                crc = ((crc << 1) ^ 0x1021) & 0xFFFF
            else:
                crc = (crc << 1) & 0xFFFF
    return crc


def cobs_encode(src: bytes) -> bytes:
    """COBS encode byte sequence. Guarantees 0x00 never appears in output."""
    dst = bytearray(len(src) + (len(src) // 254) + 1)
    read_idx = 0
    write_idx = 1
    code_idx = 0
    code = 1
    while read_idx < len(src):
        b = src[read_idx]
        read_idx += 1
        if b == 0:
            dst[code_idx] = code
            code_idx = write_idx
            write_idx += 1
            code = 1
        else:
            dst[write_idx] = b
            write_idx += 1
            code += 1
            if code == 0xFF:
                dst[code_idx] = code
                code_idx = write_idx
                write_idx += 1
                code = 1
    dst[code_idx] = code
    return bytes(dst[:write_idx])


def cobs_decode(src: bytes) -> bytes:
    """COBS decode byte sequence."""
    if not src:
        return b""
    dst = bytearray()
    read_idx = 0
    while read_idx < len(src):
        code = src[read_idx]
        read_idx += 1
        if code == 0:
            raise ValueError("Zero byte inside COBS frame")
        count = code - 1
        if read_idx + count > len(src):
            raise ValueError("Truncated COBS frame")
        dst.extend(src[read_idx:read_idx + count])
        read_idx += count
        if code < 0xFF and read_idx < len(src):
            dst.append(0)
    return bytes(dst)


def crc32_ieee(data: bytes) -> int:
    """Standard IEEE 802.3 CRC-32."""
    return binascii.crc32(data) & 0xFFFFFFFF


class RigWireError(RuntimeError):
    """Typed RigWire protocol error carrying device status code."""

    def __init__(self, status_code: int, status_name: str, message: str):
        super().__init__(message)
        self.status_code = status_code
        self.status_name = status_name


class _SocketTransport:
    """pyserial-compatible buffered adapter over a TCP host:port socket."""

    def __init__(self, host: str, port: int, timeout: float = 2.0):
        self._sock = socket.create_connection((host, port), timeout=timeout)
        try:
            self._sock.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
        except OSError:
            pass
        self._sock.settimeout(timeout)
        self._rx = bytearray()
        self.is_open = True

    def write(self, data: bytes) -> int:
        self._sock.sendall(data)
        return len(data)

    def flush(self):
        pass

    def reset_input_buffer(self):
        self._rx.clear()
        prev_timeout = self._sock.gettimeout()
        try:
            self._sock.settimeout(0.01)
            while True:
                chunk = self._sock.recv(4096)
                if not chunk:
                    break
        except (socket.timeout, BlockingIOError, OSError):
            pass
        finally:
            self._sock.settimeout(prev_timeout)

    def reset_output_buffer(self):
        pass

    def read_until_zero(self, deadline: float):
        """Read one 0x00-delimited chunk from stream, or None on deadline."""
        while True:
            idx = self._rx.find(0)
            if idx != -1:
                frame = bytes(self._rx[:idx])
                del self._rx[:idx + 1]
                return frame
            rem = deadline - time.time()
            if rem <= 0:
                return None
            self._sock.settimeout(min(rem, 0.25))
            try:
                chunk = self._sock.recv(4096)
                if not chunk:
                    return None
                self._rx.extend(chunk)
            except socket.timeout:
                continue

    def read(self, n: int = 1) -> bytes:
        while len(self._rx) < n:
            chunk = self._sock.recv(4096)
            if not chunk:
                break
            self._rx.extend(chunk)
        out = bytes(self._rx[:n])
        del self._rx[:n]
        return out

    def close(self):
        if self.is_open:
            self.is_open = False
            self._sock.close()


def _parse_tcp_target(port_name: str):
    target = port_name.strip()
    for prefix in ("tcp://", "socket://"):
        if target.startswith(prefix):
            target = target[len(prefix):]
            break
    else:
        if "://" in target or ":" not in target:
            return None
    host, _, port_str = target.rpartition(":")
    if not port_str.isdigit():
        return None
    return (host or "127.0.0.1", int(port_str))


def parse_dir_list_page(resp: bytes):
    """Decode one TRANSFER_LIST response page into (next_cursor, entries)."""
    if len(resp) < 3:
        return 0xFFFF, []
    next_cursor, count = struct.unpack("<HB", resp[:3])
    entries = []
    off = 3
    for _ in range(count):
        if off + 6 > len(resp):
            break
        is_dir = resp[off] != 0
        size = struct.unpack("<I", resp[off + 1:off + 5])[0]
        nlen = resp[off + 5]
        if off + 6 + nlen > len(resp):
            break
        name = resp[off + 6:off + 6 + nlen].decode("utf-8", errors="replace")
        off += 6 + nlen
        entries.append({"name": name, "size": size, "is_dir": is_dir})
    return next_cursor, entries


def validate_krx_package(data: bytes) -> dict:
    """Validate a .krx container against KRX-ABI preamble, sections, bounds, and CRC-32."""
    if len(data) < 24:
        raise ValueError(f"KRX too small ({len(data)} < 24 bytes)")
    if len(data) > MAX_WIRE_FILE_SIZE:
        raise ValueError(
            f"KRX size ({len(data)} bytes) exceeds RigWire {MAX_WIRE_FILE_SIZE}-byte ceiling"
        )
    if data[:4] != KRX_MAGIC:
        raise ValueError(f"Invalid KRX FourCC ident: {data[:4].hex()} (expected c0de01fe)")

    app_class = data[4]
    version = data[5]
    if app_class not in KRX_CLASSES:
        raise ValueError(f"Invalid KRX class: {app_class}")
    if version == 0 or version > 5:
        raise ValueError(f"Unsupported KRX version: {version}")

    section_count, header_len, flags, payload_crc, header_crc, link_base = struct.unpack(
        "<HHHIII", data[6:24]
    )
    if header_len < 24 or (header_len & 3) != 0 or header_len > len(data):
        raise ValueError(f"Invalid KRX header_len: {header_len}")
    if section_count == 0:
        raise ValueError("KRX package contains 0 sections")

    actual_hdr_crc = crc32_ieee(data[24:header_len])
    if actual_hdr_crc != header_crc:
        raise ValueError(
            f"Header CRC-32 mismatch: expected {header_crc:#010x}, calculated {actual_hdr_crc:#010x}"
        )

    payload = data[header_len:]
    actual_payload_crc = crc32_ieee(payload)
    if actual_payload_crc != payload_crc:
        raise ValueError(
            f"Payload CRC-32 mismatch: expected {payload_crc:#010x}, calculated {actual_payload_crc:#010x}"
        )

    info = {
        "class": app_class,
        "class_name": KRX_CLASSES[app_class],
        "version": version,
        "section_count": section_count,
        "header_len": header_len,
        "payload_len": len(payload),
        "header_crc32": actual_hdr_crc,
        "payload_crc32": actual_payload_crc,
        "file_crc32": crc32_ieee(data),
        "name": None,
        "semver": None,
        "exec_type": 1,
        "exec_type_name": "App",
        "caps": 0,
        "min_stack": 0,
        "entry_point": 0,
        "exec_payload_size": 0,
        "profile_name": KRX_CLASSES[app_class],
    }

    off = 24
    seen = set()
    for _ in range(section_count):
        if off + 4 > header_len:
            raise ValueError("Truncated KRX section table")
        sec_id, sec_len = struct.unpack("<HH", data[off:off + 4])
        if sec_len < 4 or off + sec_len > header_len:
            raise ValueError(f"Invalid KRX section 0x{sec_id:04X} length {sec_len}")
        body = data[off + 4:off + sec_len]
        seen.add(sec_id)

        if sec_id == 0x0001 and len(body) >= 40:
            name_off = struct.unpack("<H", body[0:2])[0]
            info["semver"] = (body[2], body[3], body[4])
            if name_off < header_len:
                nul = data.find(b"\x00", name_off, header_len)
                end = nul if nul != -1 else header_len
                info["name"] = data[name_off:end].decode("utf-8", errors="replace")
        elif sec_id == 0x0009 and len(body) >= 12:
            exec_type = body[0]
            if exec_type not in KRX_EXEC_TYPES:
                raise ValueError(f"Invalid KRX exec_type: {exec_type}")
            info["exec_type"] = exec_type
            info["exec_type_name"] = KRX_EXEC_TYPES[exec_type]
            info["caps"] = struct.unpack("<I", body[8:12])[0]
        elif sec_id == 0x0002 and len(body) >= 28:
            min_stack, _min_heap, page_sz, _cache_blks, _dma = struct.unpack("<HHHBB", body[0:8])
            entry_pt, p_sz, _reloc_off, _reloc_cnt, _got_off, _rt_pin, _lat = struct.unpack(
                "<IIHHHHH", body[8:26]
            )
            profile_raw = body[26]
            info["min_stack"] = min_stack
            info["entry_point"] = entry_pt
            info["exec_payload_size"] = p_sz
            if profile_raw in KRX_PROFILES:
                info["profile_name"] = KRX_PROFILES[profile_raw]
            if app_class == 0 and p_sz > 8192:
                raise ValueError(f"Native resident payload ({p_sz}B) exceeds 8192B arena")
            if len(payload) > 0 and app_class in (0, 1) and entry_pt >= len(payload):
                raise ValueError(f"Entry point 0x{entry_pt:x} outside payload ({len(payload)}B)")
        elif sec_id == 0x0005:  # BlockMap
            for boff in range(0, len(body) - (len(body) % 8), 8):
                blk_sz, blk_off = struct.unpack("<HI", body[boff + 2:boff + 8])
                if blk_off < header_len or blk_off + blk_sz > len(data):
                    raise ValueError(f"BlockMap entry out of bounds: off={blk_off}, sz={blk_sz}")
        elif sec_id == 0x000A:  # LeafMap
            for loff in range(0, len(body) - (len(body) % 16), 16):
                leaf_sz, leaf_off = struct.unpack("<HI", body[loff + 2:loff + 8])
                if leaf_off < header_len or leaf_off + leaf_sz > len(data):
                    raise ValueError(f"LeafMap entry out of bounds: off={leaf_off}, sz={leaf_sz}")

        off += (sec_len + 3) & ~3

    if 0x0001 not in seen or 0x0002 not in seen:
        raise ValueError("KRX package missing mandatory Identity or Execution section")

    return info


# ── RIGWIRE CLIENT ────────────────────────────────────────────────────────────
class RigWireClient:
    def __init__(self, port_name: str, baudrate: int = 115200, timeout: float = 2.0):
        tcp = _parse_tcp_target(port_name)
        if tcp is not None:
            self.ser = _SocketTransport(tcp[0], tcp[1], timeout=timeout)
        else:
            if serial is None:
                raise RuntimeError("pyserial is required for serial ports: pip install pyserial")
            if "://" in port_name:
                self.ser = serial.serial_for_url(port_name, baudrate=baudrate, timeout=min(timeout, 0.05))
            else:
                self.ser = serial.Serial(port_name, baudrate=baudrate, timeout=min(timeout, 0.05))
            try:
                self.ser.dtr = True
                self.ser.rts = True
                self.ser.reset_input_buffer()
                self.ser.reset_output_buffer()
            except Exception:
                pass
        self.timeout = timeout
        self.session_id = 0x4B45524E  # "KERN"
        self.sequence = 1
        self.max_payload = 128
        self._ser_rx = bytearray()
        self.last_rtt_ms = 0.0
        self._fb_cache: bytearray | None = None
        self._strip_crcs: list[int] = [0] * 16
        self._delta_supported: bool | None = None

    def close(self):
        if self.ser and self.ser.is_open:
            self.ser.close()

    def _read_serial_frame_until_zero(self, deadline: float):
        """Buffered 0x00-delimited reader for pyserial (avoids 1-byte read() syscall overhead)."""
        rx = self._ser_rx
        while True:
            idx = rx.find(0)
            if idx != -1:
                frame = bytes(rx[:idx])
                del rx[:idx + 1]
                return frame
            if time.monotonic() >= deadline:
                return None
            waiting = getattr(self.ser, "in_waiting", 0) or 0
            if waiting > 0:
                chunk = self.ser.read(waiting)
                if chunk:
                    rx.extend(chunk)
            else:
                b1 = self.ser.read(1)
                if b1:
                    rx.extend(b1)
                    w2 = getattr(self.ser, "in_waiting", 0) or 0
                    if w2 > 0:
                        c2 = self.ser.read(w2)
                        if c2:
                            rx.extend(c2)

    def _encode_record(self, service: int, opcode: int, payload: bytes = b"") -> bytes:
        is_hello = (service == SERVICE_CORE and opcode == OP_CORE_HELLO)
        session = 0 if is_hello else self.session_id
        # Firmware WireSession only advances expected_sequence for non-core services.
        seq = 0 if service == SERVICE_CORE else self.sequence
        if service != SERVICE_CORE:
            self.sequence = (self.sequence + 1) & 0xFFFFFFFF

        header = struct.pack(
            "<BBBBBIIH",
            WIRE_MAGIC,
            WIRE_MAJOR,
            0,  # flags: request
            service,
            opcode,
            session,
            seq,
            len(payload),
        )
        raw = header + payload
        crc = crc16_ccitt(raw)
        raw += struct.pack("<H", crc)
        return b"\x00" + cobs_encode(raw) + b"\x00"

    def send_record(self, service: int, opcode: int, payload: bytes = b"") -> bytes:
        t0 = time.monotonic()
        frame = self._encode_record(service, opcode, payload)
        self.ser.write(frame)
        resp = self.read_response(expected_service=service, expected_opcode=opcode)
        self.last_rtt_ms = (time.monotonic() - t0) * 1000.0
        return resp

    def resync(self) -> dict:
        """Flush stale RX bytes and re-establish session sequence=1 via CORE HELLO."""
        self._ser_rx.clear()
        self._fb_cache = None
        self._strip_crcs = [0] * 16
        try:
            if hasattr(self.ser, "reset_input_buffer"):
                self.ser.reset_input_buffer()
        except Exception:
            pass
        return self.hello()

    def read_response(self, expected_service: int, expected_opcode: int) -> bytes:
        wait_s = max(0.25, float(self.timeout))
        wall_deadline = time.time() + wait_s
        mono_deadline = time.monotonic() + wait_s
        decoded = None

        while time.monotonic() < mono_deadline:
            if hasattr(self.ser, "read_until_zero"):
                raw_frame = self.ser.read_until_zero(wall_deadline)
            else:
                raw_frame = self._read_serial_frame_until_zero(mono_deadline)
            if raw_frame is None:
                break

            if not raw_frame:
                continue
            try:
                cand = cobs_decode(raw_frame)
            except Exception:
                continue
            if len(cand) < MIN_RECORD_LEN or cand[0] != WIRE_MAGIC or cand[1] != WIRE_MAJOR:
                continue
            flags = cand[2]
            if (flags & FLAG_EVENT) != 0:
                continue
            if cand[3] != expected_service or cand[4] != expected_opcode:
                continue
            decoded = cand
            break

        if decoded is None:
            raise TimeoutError("No valid response frame received from radio (delimiter timeout)")

        magic, major, flags, service, opcode, session, seq, plen = struct.unpack(
            "<BBBBBIIH", decoded[:HEADER_LEN]
        )

        if magic != WIRE_MAGIC or major != WIRE_MAJOR:
            raise ValueError(f"Invalid wire identity: magic={magic:#04x}, major={major:#04x}")

        payload_and_crc = decoded[HEADER_LEN:]
        payload = payload_and_crc[:-CRC_LEN]
        recv_crc = struct.unpack("<H", payload_and_crc[-CRC_LEN:])[0]

        calc_crc = crc16_ccitt(decoded[:-CRC_LEN])
        if recv_crc != calc_crc:
            raise ValueError(f"CRC mismatch: expected {calc_crc:#06x}, received {recv_crc:#06x}")

        if len(payload) != plen:
            raise ValueError(f"Payload length discrepancy: header says {plen}, got {len(payload)}")

        if not payload:
            raise ValueError("Response record missing required status byte")

        status_code = payload[0]
        status_name = STATUS_NAMES.get(status_code, f"UNKNOWN({status_code})")
        app_data = payload[1:]

        if status_code != 0:
            raise RigWireError(
                status_code,
                status_name,
                f"Radio responded with status {status_name} ({status_code})",
            )

        return app_data

    # ── CORE SERVICE (0x00) ───────────────────────────────────────────────────

    def hello(self) -> dict:
        """Perform HELLO handshake (§5.1)."""
        req = struct.pack("<BBIH", 1, 1, self.session_id, 128)
        resp = self.send_record(SERVICE_CORE, OP_CORE_HELLO, req)
        if len(resp) < 17:
            raise ValueError(f"Invalid HELLO response length: {len(resp)} bytes")

        selected_minor = resp[0]
        accepted_max_payload = struct.unpack("<H", resp[1:3])[0]
        catalogue_gen = struct.unpack("<I", resp[3:7])[0]
        transport_prof = struct.unpack("<H", resp[7:9])[0]
        hw_prof = struct.unpack("<I", resp[9:13])[0]
        fw_prof = struct.unpack("<I", resp[13:17])[0]

        self.max_payload = accepted_max_payload
        self.sequence = 1

        info = {
            "minor_version": selected_minor,
            "accepted_max_payload": accepted_max_payload,
            "catalogue_generation": catalogue_gen,
            "transport_profile": transport_prof,
            "hardware_profile": hw_prof,
            "firmware_profile": fw_prof,
        }
        return info

    def ping(self) -> dict:
        """Send PING and measure RTT (§5.3)."""
        cookie = 0xAA55BEEF
        t0 = time.time()
        resp = self.send_record(SERVICE_CORE, OP_CORE_PING, struct.pack("<I", cookie))
        rtt_ms = (time.time() - t0) * 1000.0

        if len(resp) < 8:
            raise ValueError("Short PING response")
        rx_cookie, uptime_ms = struct.unpack("<II", resp[:8])
        if rx_cookie != cookie:
            raise ValueError(f"Cookie mismatch: sent {cookie:#010x}, got {rx_cookie:#010x}")

        return {"uptime_ms": uptime_ms, "rtt_ms": rtt_ms}

    def caps(self) -> list:
        """Enumerate device capabilities via GET_CAPS (§5.2)."""
        cursor = 0
        generation = 0
        records = []

        while cursor != 0xFFFF:
            req = struct.pack("<IH", generation, cursor)
            resp = self.send_record(SERVICE_CORE, OP_CORE_GET_CAPS, req)
            if len(resp) < 7:
                break
            resp_gen, next_cursor, count = struct.unpack("<IHB", resp[:7])
            generation = resp_gen
            off = 7
            for _ in range(count):
                if off + 2 > len(resp):
                    break
                rec_type = resp[off]
                rec_len = resp[off + 1]
                data = resp[off + 2:off + 2 + rec_len]
                off += 2 + rec_len
                records.append({"type": rec_type, "data": data})

            cursor = next_cursor

        return records

    def time_sync(self, host_time_us=None):
        """Execute a single NTP-like microsecond time synchronization round.
        Returns: (host_tx_us, device_rx_us, device_tx_us, uptime_ms, rtt_us, offset_us)
        """
        if host_time_us is None:
            host_time_us = int(time.monotonic() * 1_000_000) & 0xFFFFFFFF
        t1 = host_time_us
        payload = struct.pack("<I", t1)
        resp = self.send_record(SERVICE_CORE, OP_CORE_TIME_SYNC, payload)
        t4 = int(time.monotonic() * 1_000_000) & 0xFFFFFFFF
        if len(resp) < 12:
            raise RuntimeError(f"Time sync response too short: {len(resp)} bytes")
        echo_tx, dev_rx, dev_tx = struct.unpack("<III", resp[:12])
        uptime_ms = struct.unpack("<I", resp[12:16])[0] if len(resp) >= 16 else 0

        # RTT = (T4 - T1) - (T3 - T2)
        dev_dwell = (dev_tx - dev_rx) & 0xFFFFFFFF
        host_elapsed = (t4 - t1) & 0xFFFFFFFF
        rtt_us = (host_elapsed - dev_dwell) & 0xFFFFFFFF
        # Offset = ((T2 - T1) + (T3 - T4)) / 2 (signed 32-bit math)
        d1 = struct.unpack("<i", struct.pack("<I", (dev_rx - t1) & 0xFFFFFFFF))[0]
        d2 = struct.unpack("<i", struct.pack("<I", (dev_tx - t4) & 0xFFFFFFFF))[0]
        offset_us = (d1 + d2) // 2

        return (echo_tx, dev_rx, dev_tx, uptime_ms, rtt_us, offset_us)

    def sync_clock(self, rounds=5):
        """Perform multi-round NTP clock synchronization and return best offset."""
        results = []
        for _ in range(rounds):
            try:
                res = self.time_sync()
                results.append(res)
                time.sleep(0.02)
            except Exception:
                continue
        if not results:
            raise RuntimeError("Clock synchronization failed across all rounds")
        best = min(results, key=lambda x: x[4])
        return best

    def reboot(self):
        """Reboot the radio.

        Firmware only honours REBOOT inside an established session (a stray byte stream on a
        shared line must never reset a remote node), so open one first if needed. The radio
        may reset before its acknowledgement drains, so a missing reply is expected; an
        explicit BAD_SESSION refusal is not and is surfaced to the caller.
        """
        try:
            self.send_record(SERVICE_CORE, OP_CORE_REBOOT)
        except RigWireError as e:
            if e.status_name != "BAD_SESSION":
                raise
            self.resync()
            try:
                self.send_record(SERVICE_CORE, OP_CORE_REBOOT)
            except TimeoutError:
                pass
        except TimeoutError:
            pass
        print("  Reboot command dispatched.")

    def panic_test(self, hardfault: bool = False):
        """Trigger an intentional panic (3) or ARM HardFault (4) via ACTION_SYS_REBOOT.

        Only firmware built with `settings_debug` accepts these modes; release builds refuse.
        """
        mode = 4 if hardfault else 3
        try:
            self.handle_invoke(DOMAIN_SYSTEM, 0, FACET_SYS_IDENTITY, ACTION_SYS_REBOOT, bytes([mode]))
        except Exception:
            pass
        kind = "HardFault (udf #0x7F)" if hardfault else "Rust panic"
        print(f"  Triggered {kind} test on target.")

    # ── UNIVERSAL RESOURCE HANDLE RPC (SERVICE_CONTROL 0x01) ─────────────────

    def handle_get(self, domain: int, instance: int, facet: int, params: bytes = b"") -> bytes:
        """Query resource facet state via OP_HANDLE_GET."""
        payload = struct.pack("<BBH", domain, instance, facet) + params
        resp = self.send_record(SERVICE_CONTROL, OP_HANDLE_GET, payload)
        if len(resp) < 4:
            raise ValueError(f"Short HANDLE_GET response: {len(resp)} bytes")
        # Layout: domain(1), instance(1), facet(2), data(...)
        return resp[4:]

    def handle_set(self, domain: int, instance: int, facet: int, value: bytes) -> bytes:
        """Mutate resource facet state via OP_HANDLE_SET."""
        payload = struct.pack("<BBH", domain, instance, facet) + value
        return self.send_record(SERVICE_CONTROL, OP_HANDLE_SET, payload)

    def handle_invoke(self, domain: int, instance: int, facet: int, action_id: int, params: bytes = b"") -> bytes:
        """Invoke action on resource facet via OP_HANDLE_INVOKE."""
        payload = struct.pack("<BBHH", domain, instance, facet, action_id) + params
        resp = self.send_record(SERVICE_CONTROL, OP_HANDLE_INVOKE, payload)
        if len(resp) < 6:
            raise ValueError(f"Short HANDLE_INVOKE response: {len(resp)} bytes")
        return resp[6:]

    def handle_stream(self, domain: int, instance: int, facet: int, offset: int, length: int) -> bytes:
        """Stream data chunk from resource facet via OP_HANDLE_STREAM."""
        payload = struct.pack("<BBHIH", domain, instance, facet, offset, length)
        resp = self.send_record(SERVICE_CONTROL, OP_HANDLE_STREAM, payload)
        if len(resp) < 10:
            raise ValueError(f"Short HANDLE_STREAM response: {len(resp)} bytes")
        rx_off, rx_len = struct.unpack("<IH", resp[4:10])
        return resp[10:10 + rx_len]

    # ── High-Level Telemetry & Control Helpers ─────────────────────────────────

    def get_system_telemetry(self) -> dict:
        """Fetch all system & silicon metadata via unified Handle RPC."""
        telemetry = {}

        # 1. Identity Facet (41 bytes)
        try:
            ident = self.handle_get(DOMAIN_SYSTEM, 0, FACET_SYS_IDENTITY)
            if len(ident) >= 28:
                telemetry["serial64"] = struct.unpack("<Q", ident[:8])[0]
                telemetry["mac_addr"] = ":".join(f"{b:02X}" for b in ident[8:14])
                telemetry["serial_str"] = ident[14:28].decode("ascii", errors="replace").strip("\x00 ")
                is_auth = ident[28] != 0
                if is_auth and len(ident) >= 41:
                    telemetry["mcu_uid"] = ident[29:41].hex()
                else:
                    telemetry["mcu_uid"] = "Protected (Auth Required)"
            else:
                telemetry["serial_str"] = "unknown"
                telemetry["mac_addr"] = "unknown"
                telemetry["mcu_uid"] = "Protected (Auth Required)"
        except Exception:
            telemetry["serial_str"] = "unknown"
            telemetry["mac_addr"] = "unknown"
            telemetry["mcu_uid"] = "Protected (Auth Required)"

        # 2. Metrics Facet (14 bytes base; 17 bytes with live BK4829 GPIO status LED)
        try:
            met = self.handle_get(DOMAIN_SYSTEM, 0, FACET_SYS_METRICS)
            if len(met) >= 14:
                telemetry["uptime_ms"] = struct.unpack("<I", met[:4])[0]
                telemetry["sysclk_hz"] = struct.unpack("<I", met[4:8])[0]
                telemetry["battery_mv"] = struct.unpack("<H", met[8:10])[0]
                telemetry["battery_pct"] = met[10]
                telemetry["active_app"] = met[11]
                telemetry["keypad_locked"] = bool(met[12])
                telemetry["flashlight"] = bool(met[13])
                if len(met) >= 15:
                    led_bits = met[14] & 0x03
                    telemetry["status_led_raw"] = led_bits
                    telemetry["status_led"] = ("off", "red", "green", "orange")[led_bits]
                if len(met) >= 17:
                    telemetry["bk4829_gpio"] = struct.unpack("<H", met[15:17])[0]
                if len(met) >= 18:
                    telemetry["headless"] = bool(met[17])
            else:
                telemetry["uptime_ms"] = 0
                telemetry["sysclk_hz"] = 0
                telemetry["battery_mv"] = 0
                telemetry["active_app"] = 0
        except Exception:
            telemetry["uptime_ms"] = 0
            telemetry["sysclk_hz"] = 0
            telemetry["battery_mv"] = 0
            telemetry["active_app"] = 0

        # 3. Calibration Facet (5 bytes)
        try:
            cal = self.handle_get(DOMAIN_SYSTEM, 0, FACET_SYS_CALIBRATION)
            telemetry["calib_valid"] = bool(cal[0]) if cal else False
        except Exception:
            telemetry["calib_valid"] = False

        return telemetry

    def get_radio_telemetry(self, vfo_instance: int = RADIO_INSTANCE_ACTIVE) -> dict:
        """Fetch all radio VFO & RF parameters via unified Handle RPC."""
        rad = {}

        # 1. Tuning Facet (9 bytes: freq 4B, step 4B, active_vfo 1B)
        try:
            tun = self.handle_get(DOMAIN_RADIO, vfo_instance, FACET_RADIO_TUNING)
            if len(tun) >= 9:
                rad["rx_freq_hz"] = struct.unpack("<I", tun[:4])[0]
                rad["tx_freq_hz"] = rad["rx_freq_hz"]
                rad["step_hz"] = struct.unpack("<I", tun[4:8])[0]
                rad["active_vfo"] = tun[8]
            else:
                rad["rx_freq_hz"] = 0
                rad["tx_freq_hz"] = 0
                rad["active_vfo"] = 0
        except Exception:
            rad["rx_freq_hz"] = 0
            rad["tx_freq_hz"] = 0
            rad["active_vfo"] = 0

        # 2. Modulation Facet (5 bytes: mode 1B, bw 4B)
        try:
            md = self.handle_get(DOMAIN_RADIO, vfo_instance, FACET_RADIO_MODULATION)
            if len(md) >= 5:
                rad["mod_mode"] = md[0]
                rad["mod_name"] = MOD_NAMES.get(rad["mod_mode"], f"MODE_{rad['mod_mode']}")
                rad["bandwidth_hz"] = struct.unpack("<I", md[1:5])[0]
            else:
                rad["mod_mode"] = 0
                rad["mod_name"] = "FM"
                rad["bandwidth_hz"] = 12500
        except Exception:
            rad["mod_mode"] = 0
            rad["mod_name"] = "FM"
            rad["bandwidth_hz"] = 12500

        # 3. Squelch & Signal Facet (4 bytes: sql 1B, rssi 2B, carrier 1B)
        try:
            sq = self.handle_get(DOMAIN_RADIO, vfo_instance, FACET_RADIO_SQUELCH)
            if len(sq) >= 4:
                rad["squelch"] = sq[0]
                rad["rssi_dbm"] = struct.unpack("<h", sq[1:3])[0]
                rad["carrier"] = bool(sq[3])
            else:
                rad["squelch"] = 0
                rad["rssi_dbm"] = -127
                rad["carrier"] = False
        except Exception:
            rad["squelch"] = 0
            rad["rssi_dbm"] = -127
            rad["carrier"] = False

        # 4. Transmit Facet (2 bytes: tx_active 1B, power 1B)
        try:
            tx = self.handle_get(DOMAIN_RADIO, vfo_instance, FACET_RADIO_TRANSMIT)
            if len(tx) >= 2:
                rad["ptt_active"] = bool(tx[0])
                rad["power_level"] = tx[1]
            else:
                rad["ptt_active"] = False
                rad["power_level"] = 0
        except Exception:
            rad["ptt_active"] = False
            rad["power_level"] = 0

        # 5. Digital Facet (2 bytes: offset_dhz 2B)
        try:
            dig = self.handle_get(DOMAIN_RADIO, vfo_instance, FACET_RADIO_DIGITAL)
            if len(dig) >= 2:
                rad["offset_dhz"] = struct.unpack("<h", dig[:2])[0]
            else:
                rad["offset_dhz"] = 0
        except Exception:
            rad["offset_dhz"] = 0

        return rad

    def get_audio_telemetry(self) -> dict:
        """Fetch audio configuration from CONFIG domain."""
        aud = {}
        try:
            cfg = self.handle_get(DOMAIN_CONFIG, CONFIG_DOMAIN_AUDIO, 0)
            if len(cfg) >= 4:
                aud["volume"] = cfg[1]
                aud["key_beep"] = bool(cfg[2])
                aud["roger_beep"] = cfg[3]
            else:
                aud["volume"] = 0
                aud["key_beep"] = False
                aud["roger_beep"] = 0
        except Exception:
            aud["volume"] = 0
            aud["key_beep"] = False
            aud["roger_beep"] = 0
        return aud

    def set_frequency(self, freq_hz: int, vfo_instance: int = RADIO_INSTANCE_ACTIVE):
        payload = struct.pack("<I", freq_hz)
        self.handle_set(DOMAIN_RADIO, vfo_instance, FACET_RADIO_TUNING, payload)

    def set_modulation(self, mod_mode: int, vfo_instance: int = RADIO_INSTANCE_ACTIVE):
        self.handle_set(DOMAIN_RADIO, vfo_instance, FACET_RADIO_MODULATION, bytes([mod_mode]))

    def set_squelch(self, sql: int):
        self.handle_set(DOMAIN_RADIO, RADIO_INSTANCE_ACTIVE, FACET_RADIO_SQUELCH, bytes([min(max(sql, 0), 9)]))

    def set_fine_offset_dhz(self, offset_dhz: int, vfo_instance: int = RADIO_INSTANCE_ACTIVE):
        payload = struct.pack("<h", offset_dhz)
        self.handle_set(DOMAIN_RADIO, vfo_instance, FACET_RADIO_DIGITAL, payload)

    def set_vfo(self, vfo_idx: int):
        target = RADIO_INSTANCE_VFO_A if vfo_idx == 0 else RADIO_INSTANCE_VFO_B
        self.handle_set(DOMAIN_RADIO, target, FACET_RADIO_TUNING, b"")

    def toggle_vfo(self):
        self.handle_invoke(DOMAIN_RADIO, RADIO_INSTANCE_ACTIVE, FACET_RADIO_TUNING, ACTION_RADIO_VFO_TOGGLE)

    def set_ptt(self, active: bool):
        self.handle_set(DOMAIN_RADIO, RADIO_INSTANCE_ACTIVE, FACET_RADIO_TRANSMIT, bytes([1 if active else 0]))

    def set_flashlight(self, state: int):
        """0=off, 1=on, 2=toggle."""
        self.handle_invoke(DOMAIN_SYSTEM, 0, FACET_SYS_METRICS, ACTION_SYS_FLASHLIGHT, bytes([state]))

    def set_headless(self, state: int) -> bool:
        """0=off (wake LCD/backlight), 1=on (LCD sleep + backlight off, zero overhead), 2=toggle."""
        res = self.handle_invoke(DOMAIN_SYSTEM, 0, FACET_SYS_METRICS, ACTION_SYS_HEADLESS, bytes([state]))
        return bool(res[0]) if res else (state == 1)

    def drain_audio_notes(self) -> list[tuple[int, int]]:
        """Drain queued headless system audio/beep notes as [(freq_hz, duration_ms), ...]."""
        raw = self.handle_get(DOMAIN_SYSTEM, 0, FACET_SYS_AUDIO)
        notes: list[tuple[int, int]] = []
        for off in range(0, len(raw) - 3, 4):
            freq_hz, dur_ms = struct.unpack_from("<HH", raw, off)
            notes.append((freq_hz, dur_ms))
        return notes

    def set_keypad_lock(self, locked: bool):
        self.handle_set(DOMAIN_INPUT, INPUT_INSTANCE_KEYPAD, FACET_INPUT_LOCK, bytes([1 if locked else 0]))

    def toggle_keypad_lock(self):
        self.handle_invoke(DOMAIN_INPUT, INPUT_INSTANCE_KEYPAD, FACET_INPUT_LOCK, ACTION_INPUT_LOCK_TOGGLE)

    def inject_key(self, key_code: int, key_state: int = 0):
        """Inject keypad event (key_code 0-19, state 0=pressed, 1=held, 2=released)."""
        payload = bytes([key_code, key_state])
        self.handle_set(DOMAIN_INPUT, INPUT_INSTANCE_KEYPAD, FACET_INPUT_KEY, payload)

    def inject_text(self, text: str):
        """Inject direct UTF-8 characters / control keys (BS, Enter, Esc) via FACET_INPUT_TEXT."""
        raw = text.encode("utf-8")
        for i in range(0, len(raw), 32):
            self.handle_set(DOMAIN_INPUT, INPUT_INSTANCE_KEYPAD, FACET_INPUT_TEXT, raw[i : i + 32])

    def inject_encoder(self, delta: int):
        """Inject rotary encoder step delta (-127..127) via FACET_INPUT_ENCODER."""
        d = max(-127, min(127, int(delta)))
        if d != 0:
            self.handle_set(DOMAIN_INPUT, INPUT_INSTANCE_KEYPAD, FACET_INPUT_ENCODER, struct.pack("<b", d))

    def _decode_packbits(self, src: bytes, expected_len: int = 64) -> bytes:
        out = bytearray()
        i = 0
        n = len(src)
        while i < n and len(out) < expected_len:
            hdr = src[i]
            i += 1
            if hdr < 128:
                cnt = hdr + 1
                out.extend(src[i : i + cnt])
                i += cnt
            elif hdr > 128 and i < n:
                cnt = 257 - hdr
                out.extend(bytes([src[i]]) * cnt)
                i += 1
        if len(out) < expected_len:
            out.extend(b"\x00" * (expected_len - len(out)))
        return bytes(out[:expected_len])

    def screencap(self, on_between_chunks=None, pipeline: int = 2, delta: bool = True) -> bytes:
        """
        Stream 1024-byte framebuffer (128x64 bits) from primary display surface.
        Uses 16x64B strip CRC-16 Delta + PackBits RLE streaming (`offset = 0x80000000`)
        to achieve 30-100+ FPS over USB CDC / UART (1 packet when idle or partially changed,
        2-3 packets on full-screen transitions), with automatic fallback to pipelined raw chunks.
        """
        if delta and self._delta_supported is not False:
            try:
                for _round in range(6):
                    if on_between_chunks is not None:
                        on_between_chunks()
                    req = struct.pack(
                        "<BBHIH16H",
                        DOMAIN_UI,
                        UI_INSTANCE_PRIMARY,
                        FACET_UI_SURFACE,
                        0x80000000,
                        16,
                        *self._strip_crcs,
                    )
                    resp = self.send_record(SERVICE_CONTROL, OP_HANDLE_STREAM, req)
                    if len(resp) < 12:
                        self._delta_supported = False
                        break
                    rx_tag, dirty_mask, sent_mask = struct.unpack_from("<IHH", resp, 4)
                    if rx_tag != 0x80000000:
                        self._delta_supported = False
                        break
                    self._delta_supported = True
                    if self._fb_cache is None:
                        self._fb_cache = bytearray(1024)
                    if dirty_mask == 0:
                        return bytes(self._fb_cache)
                    pos = 12
                    for s in range(16):
                        if (sent_mask & (1 << s)) != 0:
                            if pos >= len(resp):
                                break
                            slen = resp[pos]
                            pos += 1
                            raw = resp[pos : pos + slen]
                            pos += slen
                            strip = raw if slen == 64 else self._decode_packbits(raw, 64)
                            self._fb_cache[s * 64 : (s + 1) * 64] = strip
                            self._strip_crcs[s] = crc16_ccitt(strip)
                    if (dirty_mask & ~sent_mask) == 0 or sent_mask == 0:
                        return bytes(self._fb_cache)
                if self._delta_supported and self._fb_cache is not None:
                    return bytes(self._fb_cache)
            except RigWireError:
                self._delta_supported = False

        buffer = bytearray(1024)
        chunk_len = 100
        offsets = list(range(0, 1024, chunk_len))
        win = max(1, min(2, int(pipeline)))
        idx = 0
        total = len(offsets)
        while idx < total:
            if on_between_chunks is not None:
                on_between_chunks()
            batch_end = min(total, idx + win)
            t0 = time.monotonic()
            frames = bytearray()
            for j in range(idx, batch_end):
                off = offsets[j]
                req_len = min(chunk_len, 1024 - off)
                payload = struct.pack("<BBHIH", DOMAIN_UI, UI_INSTANCE_PRIMARY, FACET_UI_SURFACE, off, req_len)
                frames.extend(self._encode_record(SERVICE_CONTROL, OP_HANDLE_STREAM, payload))
            self.ser.write(bytes(frames))
            for _ in range(idx, batch_end):
                resp = self.read_response(expected_service=SERVICE_CONTROL, expected_opcode=OP_HANDLE_STREAM)
                if len(resp) < 10:
                    raise ValueError(f"Short HANDLE_STREAM response: {len(resp)} bytes")
                rx_off, rx_len = struct.unpack("<IH", resp[4:10])
                chunk = resp[10:10 + rx_len]
                if rx_off < 1024 and chunk:
                    end_off = min(1024, rx_off + len(chunk))
                    buffer[rx_off:end_off] = chunk[:end_off - rx_off]
            self.last_rtt_ms = ((time.monotonic() - t0) * 1000.0) / max(1, batch_end - idx)
            idx = batch_end
        self._fb_cache = bytearray(buffer)
        self._strip_crcs = [crc16_ccitt(bytes(buffer[s * 64 : (s + 1) * 64])) for s in range(16)]
        return bytes(buffer)

    def modal_action(self, action_name: str) -> dict:
        """Inspect or interact with active modal dialog."""
        if action_name == "inspect":
            data = self.handle_get(DOMAIN_UI, UI_INSTANCE_PRIMARY, FACET_UI_MODAL)
            is_active = bool(data[0]) if len(data) >= 1 else False
            tag = data[1] if len(data) >= 2 else 0
            return {"active": is_active, "tag": tag}
        elif action_name == "dismiss":
            self.handle_invoke(DOMAIN_UI, UI_INSTANCE_PRIMARY, FACET_UI_MODAL, ACTION_MODAL_DISMISS)
            return {"status": "dismissed"}
        elif action_name == "confirm":
            self.handle_invoke(DOMAIN_UI, UI_INSTANCE_PRIMARY, FACET_UI_MODAL, ACTION_MODAL_CONFIRM)
            return {"status": "confirmed"}
        elif action_name == "cancel":
            self.handle_invoke(DOMAIN_UI, UI_INSTANCE_PRIMARY, FACET_UI_MODAL, ACTION_MODAL_CANCEL)
            return {"status": "canceled"}
        else:
            raise ValueError(f"Unknown modal action: {action_name}")

    # ── HARDENED DATA SERVICE (0x03) ──────────────────────────────────────────

    def _validate_remote_path(self, path: str):
        raw = path.encode("utf-8")
        if not path.startswith("/") or len(raw) > 32 or "//" in path or ".." in path:
            raise ValueError(f"Invalid KernFS path '{path}' (must start with /, <= 32 bytes)")

    def _abort_transfer(self, tid: int):
        try:
            self.send_record(SERVICE_DATA, OP_TRANSFER_ABORT, struct.pack("<H", tid))
        except Exception:
            pass

    def stat(self, path: str) -> dict:
        """Query metadata for remote file via TRANSFER_STAT."""
        path_b = path.encode("utf-8")
        req = struct.pack("<B", len(path_b)) + path_b
        try:
            resp = self.send_record(SERVICE_DATA, OP_TRANSFER_STAT, req)
        except RigWireError as e:
            if e.status_code == 22:  # NOT_FOUND
                return {"path": path, "exists": False, "is_dir": False, "size": 0, "crc32": 0}
            raise
        if len(resp) < 10:
            raise ValueError("Short TRANSFER_STAT response")
        exists, is_dir, size, crc32 = struct.unpack("<BBII", resp[:10])
        return {
            "path": path,
            "exists": bool(exists),
            "is_dir": bool(is_dir),
            "size": size,
            "crc32": crc32,
        }

    def ls(self, path: str = "/") -> list:
        """List files in directory via paginated TRANSFER_LIST."""
        path_b = path.encode("utf-8")
        entries = []
        cursor = 0
        while True:
            req = struct.pack("<HB", cursor, len(path_b)) + path_b
            resp = self.send_record(SERVICE_DATA, OP_TRANSFER_LIST, req)
            next_cursor, page = parse_dir_list_page(resp)
            entries.extend(page)
            if next_cursor == 0xFFFF or not page or next_cursor <= cursor:
                break
            cursor = next_cursor
        return entries

    def rm(self, path: str):
        """Unlink file via TRANSFER_REMOVE."""
        self._validate_remote_path(path)
        path_b = path.encode("utf-8")
        req = struct.pack("<B", len(path_b)) + path_b
        self.send_record(SERVICE_DATA, OP_TRANSFER_REMOVE, req)

    def push(self, local_path: str, remote_path: str):
        """Upload file via chunked streaming TRANSFER_OPEN/CHUNK/CLOSE (max 6 KiB)."""
        with open(local_path, "rb") as f:
            data = f.read()
        self.push_bytes(data, remote_path)

    def push_bytes(self, data: bytes, remote_path: str):
        """Upload raw bytes via chunked streaming TRANSFER_OPEN/CHUNK/CLOSE."""
        self._validate_remote_path(remote_path)
        total_size = len(data)
        if total_size > MAX_WIRE_FILE_SIZE:
            raise ValueError(
                f"File size ({total_size} bytes) exceeds hardened KernFS {MAX_WIRE_FILE_SIZE}-byte upload ceiling!"
            )

        file_crc32 = crc32_ieee(data)
        chunk_size = 96
        path_b = remote_path.encode("utf-8")

        # 1. TRANSFER_OPEN (Write)
        req_open = struct.pack("<BIHB", TRANSFER_MODE_WRITE, total_size, chunk_size, len(path_b)) + path_b
        resp_open = self.send_record(SERVICE_DATA, OP_TRANSFER_OPEN, req_open)
        if len(resp_open) < 8:
            raise ValueError("Invalid TRANSFER_OPEN response")
        tid, negotiated_chunk, _existing_sz = struct.unpack("<HHI", resp_open[:8])

        chunk_size = max(16, min(negotiated_chunk, 120))
        print(f"  Transfer ID #{tid}: Uploading {total_size} bytes to {remote_path} (chunk: {chunk_size}B)...")

        try:
            # 2. STREAM CHUNKS
            offset = 0
            t0 = time.time()
            while offset < total_size:
                chunk = data[offset:offset + chunk_size]
                req_chunk = struct.pack("<HIH", tid, offset, len(chunk)) + chunk
                self.send_record(SERVICE_DATA, OP_TRANSFER_CHUNK, req_chunk)
                offset += len(chunk)

                pct = int((offset / total_size) * 100)
                bar = "=" * (pct // 5) + ">" + " " * (20 - (pct // 5))
                sys.stdout.write(f"\r  [{bar[:20]}] {pct:3d}% ({offset}/{total_size}B)")
                sys.stdout.flush()

            elapsed = max(time.time() - t0, 0.001)
            speed = (total_size / elapsed) / 1024.0
            print(f"\n  Streaming complete ({speed:.1f} KB/s). Committing...")

            # 3. TRANSFER_CLOSE
            req_close = struct.pack("<HI", tid, file_crc32)
            resp_close = self.send_record(SERVICE_DATA, OP_TRANSFER_CLOSE, req_close)
        except Exception:
            self._abort_transfer(tid)
            raise

        if len(resp_close) < 6:
            raise ValueError("Short TRANSFER_CLOSE response")
        _close_tid, written_crc32 = struct.unpack("<HI", resp_close[:6])

        if written_crc32 != file_crc32:
            raise ValueError(
                f"CRC-32 verification mismatch! Sent: {file_crc32:#010x}, Device wrote: {written_crc32:#010x}"
            )

        print(f"  Verified CRC-32: {written_crc32:#010x} OK")

    def sideload(self, local_path: str, name: str = None, verify: bool = True, force: bool = False) -> dict:
        """Validate a .krx package and install it into /apps/ on the radio."""
        with open(local_path, "rb") as f:
            data = f.read()

        info = validate_krx_package(data)
        PKG_EXTS = (".krx", ".app", ".svc", ".dmn", ".drv", ".mod")
        raw_name = name if name else os.path.basename(local_path)
        base = os.path.basename(raw_name.strip())
        ext = ".krx"
        for candidate in PKG_EXTS:
            if base.lower().endswith(candidate):
                ext = candidate
                base = base[:-len(candidate)]
                break
        if not base or not re.fullmatch(r"[A-Za-z0-9_-]+", base):
            raise ValueError(f"Invalid app filename stem '{base}' (use ASCII letters, digits, _, -)")
        filename = f"{base}{ext}"
        remote_path = f"/apps/{filename}"
        if len(remote_path) > MAX_APP_PATH_LEN:
            raise ValueError(
                f"Remote path '{remote_path}' ({len(remote_path)} chars) exceeds firmware catalog limit ({MAX_APP_PATH_LEN} chars)"
            )

        existing = self.stat(remote_path)
        if existing["exists"]:
            if existing["is_dir"]:
                raise ValueError(f"Target path '{remote_path}' is a directory")
            if not force:
                raise FileExistsError(
                    f"'{remote_path}' already exists ({existing['size']}B); pass --force to overwrite"
                )
        else:
            apps_entries = self.ls("/apps")
            pkg_count = sum(
                1 for e in apps_entries if not e["is_dir"] and e["name"].lower().endswith(PKG_EXTS)
            )
            if pkg_count >= MAX_CATALOG_PACKAGES and not force:
                raise RuntimeError(
                    f"/apps already has {pkg_count}/{MAX_CATALOG_PACKAGES} catalog packages; remove one or pass --force"
                )

        ver = info["semver"]
        ver_str = f"{ver[0]}.{ver[1]}.{ver[2]}" if ver else "unknown"
        print(
            f"  Validated KRX '{info['name'] or base}' v{ver_str} "
            f"[{info['profile_name']}, {info['exec_type_name']}, caps={info['caps']:#x}, "
            f"{len(data)}B, crc={info['file_crc32']:#010x}]"
        )

        self.push_bytes(data, remote_path)

        if verify:
            st = self.stat(remote_path)
            if (
                not st["exists"]
                or st["is_dir"]
                or st["size"] != len(data)
                or st["crc32"] != info["file_crc32"]
            ):
                raise RuntimeError(f"Post-install verification failed for {remote_path}: {st}")
            print(f"  Installed and verified '{remote_path}' ({st['size']} B, CRC-32 {st['crc32']:#010x})")

        info["remote_path"] = remote_path
        return info

    def pull(self, remote_path: str, local_path: str):
        """Download file via chunked streaming TRANSFER_OPEN/CHUNK/CLOSE."""
        self._validate_remote_path(remote_path)
        path_b = remote_path.encode("utf-8")
        chunk_size = 96

        # 1. TRANSFER_OPEN (Read)
        req_open = struct.pack("<BIHB", TRANSFER_MODE_READ, 0, chunk_size, len(path_b)) + path_b
        resp_open = self.send_record(SERVICE_DATA, OP_TRANSFER_OPEN, req_open)
        if len(resp_open) < 8:
            raise ValueError("Invalid TRANSFER_OPEN response")
        tid, negotiated_chunk, total_size = struct.unpack("<HHI", resp_open[:8])
        chunk_size = max(16, min(negotiated_chunk, 120))

        print(f"  Transfer ID #{tid}: Downloading {remote_path} ({total_size} bytes)...")
        received = bytearray()
        offset = 0

        try:
            while offset < total_size:
                req_len = min(chunk_size, total_size - offset)
                req_chunk = struct.pack("<HIH", tid, offset, req_len)
                resp_chunk = self.send_record(SERVICE_DATA, OP_TRANSFER_CHUNK, req_chunk)
                if len(resp_chunk) < 8:
                    raise RuntimeError("Short TRANSFER_CHUNK response")
                _rx_tid, _rx_off, rx_len = struct.unpack("<HIH", resp_chunk[:8])
                data = resp_chunk[8:8 + rx_len]
                if not data:
                    raise RuntimeError(f"Unexpected EOF at offset {offset}/{total_size}")
                received.extend(data)
                offset += len(data)

                pct = int((offset / total_size) * 100)
                bar = "=" * (pct // 5) + ">" + " " * (20 - (pct // 5))
                sys.stdout.write(f"\r  [{bar[:20]}] {pct:3d}% ({offset}/{total_size}B)")
                sys.stdout.flush()

            # 3. TRANSFER_CLOSE
            req_close = struct.pack("<HI", tid, 0)
            self.send_record(SERVICE_DATA, OP_TRANSFER_CLOSE, req_close)
        except Exception:
            self._abort_transfer(tid)
            raise

        with open(local_path, "wb") as f:
            f.write(received)

        calc_crc = crc32_ieee(received)
        print(f"\n  Downloaded {len(received)} bytes to {local_path} (CRC-32: {calc_crc:#010x})")


# ── ASCII & UNICODE SCREEN MIRRORING ──────────────────────────────────────────
def render_canvas_braille(buf: bytes) -> str:
    """Render 128x64 1-bit ST7565 canvas buffer using Unicode 2x4 Braille characters (64x16 text grid)."""
    if len(buf) < 1024:
        return "Invalid canvas buffer size"
    lines = []
    lines.append("┌" + "─" * 64 + "┐")
    for y_block in range(0, 64, 4):
        row_str = ["│"]
        for x_block in range(0, 128, 2):
            # Braille dots mapping (U+2800 + bitmask):
            # Dot 1: (x,   y)   -> bit 0 (0x01)
            # Dot 2: (x,   y+1) -> bit 1 (0x02)
            # Dot 3: (x,   y+2) -> bit 2 (0x04)
            # Dot 4: (x+1, y)   -> bit 3 (0x08)
            # Dot 5: (x+1, y+1) -> bit 4 (0x10)
            # Dot 6: (x+1, y+2) -> bit 5 (0x20)
            # Dot 7: (x,   y+3) -> bit 6 (0x40)
            # Dot 8: (x+1, y+3) -> bit 7 (0x80)
            d1 = (buf[(y_block // 8) * 128 + x_block] >> (y_block % 8)) & 1
            d2 = (buf[((y_block + 1) // 8) * 128 + x_block] >> ((y_block + 1) % 8)) & 1
            d3 = (buf[((y_block + 2) // 8) * 128 + x_block] >> ((y_block + 2) % 8)) & 1
            d7 = (buf[((y_block + 3) // 8) * 128 + x_block] >> ((y_block + 3) % 8)) & 1

            d4 = (buf[(y_block // 8) * 128 + x_block + 1] >> (y_block % 8)) & 1
            d5 = (buf[((y_block + 1) // 8) * 128 + x_block + 1] >> ((y_block + 1) % 8)) & 1
            d6 = (buf[((y_block + 2) // 8) * 128 + x_block + 1] >> ((y_block + 2) % 8)) & 1
            d8 = (buf[((y_block + 3) // 8) * 128 + x_block + 1] >> ((y_block + 3) % 8)) & 1

            code = (d1 << 0) | (d2 << 1) | (d3 << 2) | (d4 << 3) | (d5 << 4) | (d6 << 5) | (d7 << 6) | (d8 << 7)
            row_str.append(chr(0x2800 + code))
        row_str.append("│")
        lines.append("".join(row_str))
    lines.append("└" + "─" * 64 + "┘")
    return "\n".join(lines)


def render_canvas_ascii(buf: bytes) -> str:
    """Render 128x64 1-bit ST7565 canvas buffer using Unicode half-blocks (32 text rows)."""
    if len(buf) < 1024:
        return "Invalid canvas buffer size"
    lines = []
    lines.append("┌" + "─" * 128 + "┐")
    for y in range(0, 64, 2):
        row_str = ["│"]
        for x in range(128):
            # Top pixel (y)
            top_page = y // 8
            top_bit = y % 8
            top_val = (buf[top_page * 128 + x] >> top_bit) & 1

            # Bottom pixel (y + 1)
            bot_page = (y + 1) // 8
            bot_bit = (y + 1) % 8
            bot_val = (buf[bot_page * 128 + x] >> bot_bit) & 1

            if top_val and bot_val:
                row_str.append("█")
            elif top_val and not bot_val:
                row_str.append("▀")
            elif not top_val and bot_val:
                row_str.append("▄")
            else:
                row_str.append(" ")
        row_str.append("│")
        lines.append("".join(row_str))
    lines.append("└" + "─" * 128 + "┘")
    return "\n".join(lines)


# ── SELF-TEST SUITE ───────────────────────────────────────────────────────────
def run_protocol_tests():
    print("==================================================================")
    print("⚡ RigWire Protocol Verification & Conformance Test Suite")
    print("==================================================================")

    print("Test 1: CRC-16/CCITT-FALSE Standard Vector (§4.2)...", end=" ")
    v = crc16_ccitt(b"123456789")
    assert v == 0x29B1, f"Expected 0x29B1, got {v:#06x}"
    print(f"PASSED ({v:#06x})")

    print("Test 2: COBS Encode/Decode Edge Boundaries...", end=" ")
    test_vectors = [
        b"",
        b"\x00",
        b"\x00\x00\x00",
        b"KernOS RigWire Protocol",
        b"prefix\x00delimiter\x00test",
        bytes(range(254)),
        bytes(range(255)),
        bytes(range(256)),
        b"\x00" * 300,
        b"A" * 512,
    ]
    for tv in test_vectors:
        enc = cobs_encode(tv)
        assert b"\x00" not in enc, f"0x00 found in COBS encoded body for len {len(tv)}"
        dec = cobs_decode(enc)
        assert dec == tv, f"COBS roundtrip mismatch for len {len(tv)}"
    print(f"PASSED ({len(test_vectors)} vectors verified)")

    print("Test 3: Wire Record Header Layout & Struct Packing...", end=" ")
    header = struct.pack(
        "<BBBBBIIH",
        WIRE_MAGIC,
        WIRE_MAJOR,
        0,
        SERVICE_DATA,
        OP_TRANSFER_OPEN,
        0x12345678,
        42,
        64,
    )
    assert len(header) == HEADER_LEN, f"Header len must be 15, got {len(header)}"
    unpacked = struct.unpack("<BBBBBIIH", header)
    assert unpacked[0] == WIRE_MAGIC and unpacked[1] == WIRE_MAJOR
    assert unpacked[3] == SERVICE_DATA and unpacked[4] == OP_TRANSFER_OPEN
    assert unpacked[5] == 0x12345678 and unpacked[6] == 42 and unpacked[7] == 64
    print("PASSED (15-byte little-endian packed format)")

    print("Test 4: IEEE 802.3 CRC-32 Checksum Engine...", end=" ")
    crc32_val = crc32_ieee(b"123456789")
    assert crc32_val == 0xCBF43926, f"Expected 0xCBF43926, got {crc32_val:#010x}"
    print(f"PASSED ({crc32_val:#010x})")

    print("Test 5: NTP-Like Microsecond Time Sync Packet Encoding & Midpoint Math...", end=" ")
    host_t1 = 1_000_000
    req_payload = struct.pack("<I", host_t1)
    assert len(req_payload) == 4

    dev_rx = 1_000_200
    dev_tx = 1_000_210
    uptime = 1000
    resp_payload = struct.pack("<IIII", host_t1, dev_rx, dev_tx, uptime)
    assert len(resp_payload) == 16

    host_t4 = 1_000_420
    dev_dwell = (dev_tx - dev_rx)
    host_elapsed = (host_t4 - host_t1)
    rtt = host_elapsed - dev_dwell
    assert rtt == 410
    offset = ((dev_rx - host_t1) + (dev_tx - host_t4)) // 2
    assert offset == -5
    print(f"PASSED (RTT={rtt}us, offset={offset}us)")

    print("Test 6: Universal Resource Handle Packing & Facet RPC Encodings...", end=" ")
    # Handle Get (DOMAIN_RADIO, VFO_A, FACET_RADIO_TUNING)
    h_get = struct.pack("<BBH", DOMAIN_RADIO, RADIO_INSTANCE_VFO_A, FACET_RADIO_TUNING)
    assert len(h_get) == 4
    assert h_get == bytes([0x01, 0x01, 0x01, 0x00])

    # Handle Set (DOMAIN_RADIO, ACTIVE, FACET_RADIO_TUNING, 145.5 MHz)
    h_set = struct.pack("<BBHI", DOMAIN_RADIO, RADIO_INSTANCE_ACTIVE, FACET_RADIO_TUNING, 145_500_000)
    assert len(h_set) == 8

    # Handle Invoke (DOMAIN_UI, PRIMARY, FACET_UI_MODAL, ACTION_MODAL_CONFIRM)
    h_inv = struct.pack("<BBHH", DOMAIN_UI, UI_INSTANCE_PRIMARY, FACET_UI_MODAL, ACTION_MODAL_CONFIRM)
    assert len(h_inv) == 6

    # Handle Stream (DOMAIN_UI, PRIMARY, FACET_UI_SURFACE, offset=0, len=96)
    h_str = struct.pack("<BBHIH", DOMAIN_UI, UI_INSTANCE_PRIMARY, FACET_UI_SURFACE, 0, 96)
    assert len(h_str) == 10

    # Key Injection (DOMAIN_INPUT, KEYPAD, FACET_INPUT_KEY, KeyCode::Menu, KeyState::Pressed)
    h_key = struct.pack("<BBHBB", DOMAIN_INPUT, INPUT_INSTANCE_KEYPAD, FACET_INPUT_KEY, KEY_CODES["menu"], KEY_STATES["press"])
    assert len(h_key) == 6
    print("PASSED")

    print("Test 7: High-Density Braille Screen Encoder (64x16 character grid)...", end=" ")
    fake_fb = bytearray(1024)
    # Set top-left pixel (x=0, y=0) -> bit 0 in page 0 -> braille dot 1 (0x2801)
    fake_fb[0] = 0x01
    # Set top-left right pixel (x=1, y=0) -> bit 0 in page 0 -> braille dot 4 (0x2808)
    fake_fb[1] = 0x01
    # Set bottom pixel of block (x=0, y=3) -> bit 3 in page 0 -> braille dot 7 (0x2840)
    fake_fb[0] |= 0x08
    rendered = render_canvas_braille(bytes(fake_fb))
    lines = rendered.splitlines()
    assert len(lines) == 18, f"Expected 18 lines (border + 16 rows), got {len(lines)}"
    assert lines[0] == "┌" + "─" * 64 + "┐"
    # First character inside row 1 should be chr(0x2800 | 0x01 | 0x08 | 0x40) = chr(0x2849)
    first_char = lines[1][1]
    assert first_char == chr(0x2849), f"Expected {chr(0x2849)} (0x2849), got {first_char} ({ord(first_char):#x})"
    print(f"PASSED (verified character '{first_char}')")

    print("Test 8: KRX Validator & Paginated Dir-List Decoder...", end=" ")
    sample_krx = os.path.join(os.path.dirname(os.path.dirname(__file__)), "packages", "beacon", "beacon.krx")
    if os.path.exists(sample_krx):
        kinfo = validate_krx_package(open(sample_krx, "rb").read())
        assert kinfo["name"] and kinfo["class_name"] == "Native"
    bad_crc = bytearray(open(sample_krx, "rb").read())
    bad_crc[-1] ^= 0xFF
    try:
        validate_krx_package(bytes(bad_crc))
        raise AssertionError("Corrupt KRX CRC should fail")
    except ValueError:
        pass
    page_blob = struct.pack("<HBBIB", 4, 1, 0, 1234, 5) + b"a.krx"
    nc, ents = parse_dir_list_page(page_blob)
    assert nc == 4 and len(ents) == 1 and ents[0]["name"] == "a.krx" and ents[0]["size"] == 1234
    print("PASSED")

    print("\n✨ All RigWire protocol self-tests passed with 100% conformance!")
    print("==================================================================")


def parse_frequency(val_str: str) -> int:
    """Parse frequency string supporting MHz, kHz, or raw Hz."""
    s = val_str.strip()
    if s.lower().endswith("mhz") or s.lower().endswith("m"):
        num_str = s.rstrip("MmHzhz").strip()
        return int(float(num_str) * 1_000_000)
    elif s.lower().endswith("khz") or s.lower().endswith("k"):
        num_str = s.rstrip("KkHzhz").strip()
        return int(float(num_str) * 1_000)
    elif "." in s:
        return int(float(s) * 1_000_000)
    else:
        return int(s)


def detect_default_port() -> str:
    env_port = os.environ.get("RIGWIRE_PORT")
    if env_port:
        return env_port

    for pattern in (
        "/dev/serial/by-id/*Quansheng*",
        "/dev/ttyACM*",
        "/dev/ttyUSB*",
    ):
        matches = sorted(glob.glob(pattern))
        if matches:
            return os.path.realpath(matches[0])

    return "/dev/ttyACM0"


# ── MAIN ENTRY POINT ──────────────────────────────────────────────────────────
def main():
    parser = argparse.ArgumentParser(
        prog="rigwire",
        description="⚡ RigWire Host Tool & Provisioning Client for KERN Firmware",
    )
    parser.add_argument(
        "-p", "--port",
        default=detect_default_port(),
        help="Serial port path (e.g. /dev/ttyACM0, /dev/ttyUSB0, COM3). Default: auto-detect",
    )
    parser.add_argument(
        "-b", "--baud",
        type=int,
        default=115200,
        help="Baud rate (default: 115200)",
    )

    subparsers = parser.add_subparsers(dest="command", help="Command to execute")

    subparsers.add_parser("hello", help="Perform HELLO handshake and inspect device profiles")
    subparsers.add_parser("ping", help="Ping device and measure round-trip latency and uptime")
    subparsers.add_parser("caps", help="Query advertised hardware capabilities catalogue")
    subparsers.add_parser("info", help="Display full radio, hardware, and audio telemetry dashboard")
    subparsers.add_parser("status", help="Alias for info")

    rad_p = subparsers.add_parser("radio", help="Inspect or adjust radio RF parameters and VFO")
    rad_p.add_argument("--vfo", type=int, choices=[0, 1], help="Select active VFO (0=VFO-A, 1=VFO-B)")
    rad_p.add_argument("--toggle-vfo", action="store_true", help="Toggle between VFO-A and VFO-B")
    rad_p.add_argument("--freq", type=str, help="Set frequency (e.g. 145.5M, 145500000, 433.250)")
    rad_p.add_argument("--mod", type=str, help="Set modulation mode (FM, AM, USB, CW, DIG, PKT, etc.)")
    rad_p.add_argument("--squelch", type=int, choices=range(0, 10), help="Set squelch level (0-9)")
    rad_p.add_argument("--offset", type=int, help="Set Vernier sub-Hz fine offset in deci-Hertz (dHz)")

    ptt_p = subparsers.add_parser("ptt", help="Control transmitter PTT state")
    ptt_p.add_argument("state", choices=["on", "off", "1", "0", "tx", "rx"], help="PTT state (on/off)")

    scr_p = subparsers.add_parser("screen", help="Capture live front panel display and render in terminal (Braille 64x16 or Halfblock 128x32)")
    for p in (scr_p, subparsers.add_parser("screencap", help="Alias for screen")):
        p.add_argument("--mode", "-m", choices=["braille", "ascii", "halfblock"], default="braille", help="Rendering mode (default: braille)")
        p.add_argument("--ascii", action="store_true", help="Render using Unicode half-blocks (128x32 text grid)")
        p.add_argument("--braille", action="store_true", help="Render using 2x4 Braille characters (64x16 text grid)")

    key_p = subparsers.add_parser("key", help="Inject keypad event remotely")
    key_p.add_argument("name", help="Key name (menu, exit, up, down, ptt, side1, side2, f, 0-9, star, hash)")
    key_p.add_argument("state", nargs="?", default="press", choices=["press", "hold", "release"], help="Key state (default: press)")

    modal_p = subparsers.add_parser("modal", help="Inspect or interact with active modal dialog")
    modal_p.add_argument("action", choices=["inspect", "confirm", "cancel", "dismiss"], help="Modal action")

    subparsers.add_parser("tone", help="Play 1000 Hz beep tone via audio subsystem")

    fl_p = subparsers.add_parser("flashlight", help="Control top LED flashlight")
    fl_p.add_argument("state", nargs="?", default="toggle", choices=["on", "off", "toggle"], help="State")

    hl_p = subparsers.add_parser("headless", help="Zero-overhead headless mode (physically powers off LCD & backlight)")
    hl_p.add_argument("state", nargs="?", default="toggle", choices=["on", "off", "toggle"], help="State")

    lk_p = subparsers.add_parser("lock", help="Control keypad lock state")
    lk_p.add_argument("state", nargs="?", default="toggle", choices=["on", "off", "toggle"], help="State")

    get_p = subparsers.add_parser("get", help="Query arbitrary microkernel Resource Handle")
    get_p.add_argument("domain", type=int, help="Domain ID (0=System, 1=Radio, 2=UI, 3=Input, 4=Config, 5=App, 6=Storage)")
    get_p.add_argument("instance", type=int, help="Instance ID")
    get_p.add_argument("facet", type=lambda s: int(s, 0), help="Facet ID (hex or decimal)")

    set_p = subparsers.add_parser("set", help="Mutate arbitrary microkernel Resource Handle")
    set_p.add_argument("domain", type=int, help="Domain ID")
    set_p.add_argument("instance", type=int, help="Instance ID")
    set_p.add_argument("facet", type=lambda s: int(s, 0), help="Facet ID")
    set_p.add_argument("value", help="Value string, integer, or hex bytes")

    ls_p = subparsers.add_parser("ls", help="List directory entries on radio filesystem")
    ls_p.add_argument("path", nargs="?", default="/", help="Directory path to list (default: /)")

    stat_p = subparsers.add_parser("stat", help="Query file existence, size, and CRC-32 metadata")
    stat_p.add_argument("path", help="Target path on radio filesystem")

    push_p = subparsers.add_parser("push", help="Upload local file to radio filesystem (max 6 KiB)")
    push_p.add_argument("local", help="Local file path")
    push_p.add_argument("remote", help="Remote file path on radio (e.g. /apps/game.krx, /sys/calib.dat)")

    side_p = subparsers.add_parser("sideload", help="Validate and install a .krx package into /apps/")
    side_p.add_argument("krx", help="Local .krx package path")
    side_p.add_argument("-n", "--name", help="Optional target app stem name in /apps/ (default: filename stem)")
    side_p.add_argument("-f", "--force", action="store_true", help="Overwrite existing app if present")
    side_p.add_argument("--no-verify", action="store_true", help="Skip post-upload stat/CRC verification")

    pull_p = subparsers.add_parser("pull", help="Download file from radio to local machine")
    pull_p.add_argument("remote", help="Remote file path on radio")
    pull_p.add_argument("local", help="Local destination file path")

    rm_p = subparsers.add_parser("rm", help="Delete file from radio filesystem")
    rm_p.add_argument("path", help="Path of file to delete on radio")

    sync_p = subparsers.add_parser("sync", help="Synchronize microsecond clock via NTP handshake")
    sync_p.add_argument("--rounds", type=int, default=5, help="Number of NTP handshake rounds (default: 5)")

    subparsers.add_parser("reboot", help="Reboot radio")
    pt_p = subparsers.add_parser("panic-test", help="Trigger an intentional panic or HardFault for crash handler testing")
    pt_p.add_argument("--hardfault", action="store_true", help="Trigger hardware HardFault (udf #0x7F) instead of software panic")
    subparsers.add_parser("test", help="Execute protocol unit self-tests (no hardware needed)")

    args = parser.parse_args()

    if args.command == "test":
        run_protocol_tests()
        sys.exit(0)

    if not args.command:
        parser.print_help()
        sys.exit(1)

    print(f"Connecting to RigWire on {args.port} @ {args.baud} baud...")
    client = RigWireClient(args.port, args.baud)

    try:
        if args.command != "hello":
            client.hello()

        if args.command == "hello":
            info = client.hello()
            print("  Status: Connected (OK)")
            print(f"  Minor Version:       v{info['minor_version']}")
            print(f"  Accepted Max Payload:{info['accepted_max_payload']} bytes")
            print(f"  Catalogue Gen:       #{info['catalogue_generation']}")
            print(f"  Transport Profile:   {info['transport_profile']:#06x}")
            print(f"  Hardware Profile:    {info['hardware_profile']:#010x}")
            print(f"  Firmware Profile:    {info['firmware_profile']:#010x}")

        elif args.command == "ping":
            p = client.ping()
            print(f"  Status: PONG OK")
            print(f"  Uptime: {p['uptime_ms']} ms ({p['uptime_ms'] / 1000.0:.2f} s)")
            print(f"  Round-Trip Time: {p['rtt_ms']:.2f} ms")

        elif args.command == "caps":
            recs = client.caps()
            print(f"  Discovered {len(recs)} capability records:")
            for r in recs:
                print(f"    - Type 0x{r['type']:02x} ({len(r['data'])} bytes): {r['data'].hex()}")

        elif args.command in ("info", "status"):
            sys_t = client.get_system_telemetry()
            rad_t = client.get_radio_telemetry()
            aud_t = client.get_audio_telemetry()

            print("==================================================================")
            print("⚡ Kern RigWire System & Radio Telemetry")
            print("==================================================================")
            print("  Hardware & Silicon:")
            ser_str = sys_t.get("serial_str", "unknown")
            ser64 = sys_t.get("serial64", 0)
            print(f"    Hardware Serial : {ser_str} ({ser64:#018x})")
            print(f"    MAC Address     : {sys_t.get('mac_addr', 'unknown')}")
            uid_str = sys_t.get("mcu_uid", "Protected (Auth Required)")
            if len(uid_str) == 24:
                uid_fmt = f"{uid_str[0:8]}-{uid_str[8:16]}-{uid_str[16:24]}"
            else:
                uid_fmt = uid_str
            print(f"    MCU Unique ID   : {uid_fmt}")
            clk_mhz = sys_t["sysclk_hz"] / 1_000_000.0
            print(f"    System Clock    : {clk_mhz:.1f} MHz")
            calib_str = "Valid (Authenticated)" if sys_t["calib_valid"] else "Default / Uncalibrated"
            print(f"    Calibration     : {calib_str}")
            print(f"    Battery Level   : {sys_t['battery_mv']} mV ({sys_t['battery_mv'] / 1000.0:.2f} V)")
            print(f"    Uptime          : {sys_t['uptime_ms'] / 1000.0:.2f} s ({sys_t['uptime_ms']} ms)")
            print(f"    Active App ID   : #{sys_t['active_app']}")

            print("\n  Radio State:")
            vfo_name = "VFO-A" if rad_t["active_vfo"] == 0 else "VFO-B"
            print(f"    Active VFO      : {vfo_name}")
            rx_mhz = rad_t["rx_freq_hz"] / 1_000_000.0
            tx_mhz = rad_t["tx_freq_hz"] / 1_000_000.0
            print(f"    RX Frequency    : {rx_mhz:10.6f} MHz ({rad_t['rx_freq_hz']:,} Hz)")
            print(f"    TX Frequency    : {tx_mhz:10.6f} MHz ({rad_t['tx_freq_hz']:,} Hz)")
            print(f"    Modulation Mode : {rad_t['mod_name']} (Mode #{rad_t['mod_mode']})")
            bw_khz = rad_t["bandwidth_hz"] / 1000.0
            print(f"    Bandwidth       : {bw_khz:.1f} kHz ({rad_t['bandwidth_hz']} Hz)")
            off_hz = rad_t["offset_dhz"] / 10.0
            print(f"    Vernier Offset  : {off_hz:+.1f} Hz ({rad_t['offset_dhz']:+d} dHz)")
            print(f"    Squelch Level   : {rad_t['squelch']} / 9")
            rssi = rad_t["rssi_dbm"]
            print(f"    Signal (RSSI)   : {rssi} dBm")
            ptt_str = "TRANSMITTING (TX)" if rad_t["ptt_active"] else "IDLE (RX)"
            print(f"    PTT State       : {ptt_str}")

            print("\n  Audio & Preferences:")
            print(f"    Speaker Volume  : {aud_t['volume']} / 63")
            print(f"    Keypad Beep     : {'ON' if aud_t['key_beep'] else 'OFF'}")
            print(f"    Roger Beep      : {aud_t['roger_beep']}")
            print(f"    Keypad Lock     : {'LOCKED' if sys_t['keypad_locked'] else 'UNLOCKED'}")
            print(f"    Flashlight      : {'ON' if sys_t['flashlight'] else 'OFF'}")
            print("==================================================================")

        elif args.command == "radio":
            modified = False
            if args.vfo is not None:
                client.set_vfo(args.vfo)
                print(f"  VFO set to VFO-{'A' if args.vfo == 0 else 'B'}")
                modified = True
            if args.toggle_vfo:
                client.toggle_vfo()
                print("  VFO toggled.")
                modified = True
            if args.freq is not None:
                freq_hz = parse_frequency(args.freq)
                client.set_frequency(freq_hz)
                print(f"  Frequency set to {freq_hz:,} Hz ({freq_hz / 1_000_000.0:.6f} MHz)")
                modified = True
            if args.mod is not None:
                mod_str = args.mod.strip().upper()
                if mod_str in MOD_BY_NAME:
                    mod_id = MOD_BY_NAME[mod_str]
                else:
                    mod_id = int(args.mod)
                client.set_modulation(mod_id)
                print(f"  Modulation set to {MOD_NAMES.get(mod_id, str(mod_id))} (#{mod_id})")
                modified = True
            if args.squelch is not None:
                client.set_squelch(args.squelch)
                print(f"  Squelch set to {args.squelch}")
                modified = True
            if args.offset is not None:
                client.set_fine_offset_dhz(args.offset)
                print(f"  Vernier fine offset set to {args.offset:+d} dHz ({args.offset / 10.0:+.1f} Hz)")
                modified = True

            if not modified:
                rad_t = client.get_radio_telemetry()
                vfo_name = "VFO-A" if rad_t["active_vfo"] == 0 else "VFO-B"
                rx_mhz = rad_t["rx_freq_hz"] / 1_000_000.0
                print(f"  Active VFO   : {vfo_name}")
                print(f"  Frequency    : {rx_mhz:.6f} MHz ({rad_t['rx_freq_hz']:,} Hz)")
                print(f"  Modulation   : {rad_t['mod_name']} (#{rad_t['mod_mode']})")
                print(f"  Bandwidth    : {rad_t['bandwidth_hz'] / 1000.0:.1f} kHz")
                print(f"  Vernier Off  : {rad_t['offset_dhz'] / 10.0:+.1f} Hz")
                print(f"  Squelch      : {rad_t['squelch']} / 9")
                print(f"  RSSI         : {rad_t['rssi_dbm']} dBm")
                print(f"  PTT          : {'TX' if rad_t['ptt_active'] else 'RX'}")

        elif args.command == "ptt":
            state = args.state.lower() in ("on", "1", "tx", "start", "true")
            client.set_ptt(state)
            print(f"  PTT {'TRANSMITTING' if state else 'RELEASED (RX)'}")

        elif args.command in ("screen", "screencap"):
            print("Fetching live display surface from radio...")
            fb = client.screencap()
            use_ascii = getattr(args, "ascii", False) or (getattr(args, "mode", "braille") in ("ascii", "halfblock"))
            if getattr(args, "braille", False):
                use_ascii = False
            if use_ascii:
                print(render_canvas_ascii(fb))
            else:
                print(render_canvas_braille(fb))

        elif args.command == "key":
            key_name = args.name.lower()
            if key_name not in KEY_CODES:
                print(f"Error: Unknown key '{args.name}'. Choices: {list(KEY_CODES.keys())}")
                sys.exit(1)
            code = KEY_CODES[key_name]
            state = KEY_STATES[args.state.lower()]
            client.inject_key(code, state)
            print(f"  Key '{args.name}' ({args.state}) injected successfully.")

        elif args.command == "modal":
            res = client.modal_action(args.action)
            if args.action == "inspect":
                print(f"  Active Modal: {res['active']} (Tag: {res['tag']})")
            else:
                print(f"  Modal Action '{args.action}': {res['status']}")

        elif args.command == "tone":
            client.handle_invoke(DOMAIN_SYSTEM, 0, FACET_SYS_METRICS, 0x0003)
            print("  Tone / beep emitted.")

        elif args.command == "flashlight":
            if args.state == "on":
                client.set_flashlight(1)
                print("  Flashlight: ON")
            elif args.state == "off":
                client.set_flashlight(0)
                print("  Flashlight: OFF")
            else:
                client.set_flashlight(2)
                print("  Flashlight toggled.")

        elif args.command == "headless":
            st_val = 1 if args.state == "on" else (0 if args.state == "off" else 2)
            active = client.set_headless(st_val)
            print(f"  Headless Mode: {'ON (LCD & backlight off)' if active else 'OFF (LCD active)'}")

        elif args.command == "lock":
            if args.state == "on":
                client.set_keypad_lock(True)
                print("  Keypad: LOCKED")
            elif args.state == "off":
                client.set_keypad_lock(False)
                print("  Keypad: UNLOCKED")
            else:
                client.toggle_keypad_lock()
                print("  Keypad lock toggled.")

        elif args.command == "get":
            val = client.handle_get(args.domain, args.instance, args.facet)
            print(f"  Handle ({args.domain}, {args.instance}, {args.facet:#06x}) [{len(val)} bytes]:")
            print(f"    Hex : {val.hex()}")
            if len(val) == 1:
                print(f"    u8  : {val[0]}")
            elif len(val) == 2:
                u16_val = struct.unpack("<H", val)[0]
                i16_val = struct.unpack("<h", val)[0]
                print(f"    u16 : {u16_val} (signed: {i16_val})")
            elif len(val) == 4:
                u32_val = struct.unpack("<I", val)[0]
                i32_val = struct.unpack("<i", val)[0]
                print(f"    u32 : {u32_val} (signed: {i32_val})")

        elif args.command == "set":
            val_str = args.value.strip()
            if val_str.startswith("0x") or val_str.startswith("0X"):
                hex_digits = val_str[2:]
                if len(hex_digits) % 2 != 0:
                    hex_digits = "0" + hex_digits
                val_bytes = bytes.fromhex(hex_digits)
            elif val_str.isdigit() or (val_str.startswith("-") and val_str[1:].isdigit()):
                num = int(val_str)
                if -128 <= num <= 255:
                    val_bytes = struct.pack("<b" if num < 0 else "<B", num)
                elif -32768 <= num <= 65535:
                    val_bytes = struct.pack("<h" if num < 0 else "<H", num)
                else:
                    val_bytes = struct.pack("<i" if num < 0 else "<I", num)
            else:
                val_bytes = val_str.encode("utf-8")
            client.handle_set(args.domain, args.instance, args.facet, val_bytes)
            print(f"  Set Handle ({args.domain}, {args.instance}, {args.facet:#06x}) = {val_bytes.hex()} OK")

        elif args.command == "ls":
            entries = client.ls(args.path)
            print(f"\nDirectory {args.path} ({len(entries)} items):")
            print("  TYPE    SIZE        NAME")
            print("  ────    ────────    ────")
            for e in entries:
                kind = "DIR " if e["is_dir"] else "FILE"
                print(f"  {kind}    {e['size']:8d} B  {e['name']}")

        elif args.command == "stat":
            s = client.stat(args.path)
            if not s["exists"]:
                print(f"  File '{args.path}' not found.")
            else:
                kind = "Directory" if s["is_dir"] else "File"
                print(f"  Target:   {s['path']}")
                print(f"  Type:     {kind}")
                print(f"  Size:     {s['size']} bytes")
                print(f"  CRC-32:   {s['crc32']:#010x}")

        elif args.command == "push":
            client.push(args.local, args.remote)

        elif args.command == "sideload":
            client.sideload(
                args.krx,
                name=args.name,
                verify=not args.no_verify,
                force=args.force,
            )

        elif args.command == "pull":
            client.pull(args.remote, args.local)

        elif args.command == "rm":
            client.rm(args.path)
            print(f"  Deleted '{args.path}' successfully.")

        elif args.command == "reboot":
            client.reboot()

        elif args.command == "panic-test":
            client.panic_test(hardfault=args.hardfault)

        elif args.command == "sync":
            print(f"Synchronizing clock with radio ({args.rounds} rounds)...")
            best = client.sync_clock(args.rounds)
            print(f"  Echo Host TX : {best[0]} us")
            print(f"  Radio RX     : {best[1]} us")
            print(f"  Radio TX     : {best[2]} us")
            print(f"  Radio Uptime : {best[3]} ms")
            print(f"  Measured RTT : {best[4]} us")
            print(f"  Clock Offset : {best[5]:+d} us")
            print("  ✨ Microsecond clock synchronized successfully!")

    finally:
        client.close()


if __name__ == "__main__":
    main()
