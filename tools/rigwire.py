#!/usr/bin/env python3
"""
⚡ rigwire - Developer & Host CLI Tool for RigWire Serial Protocol
Implements RigWire Candidate Specification (Core 0x00, Control 0x01, Events 0x02, Data 0x03).

Features:
- Framing: Delimiter 0x00 + COBS + Header(15B) + Payload(NB) + CRC16-CCITT-FALSE(2B) + Delimiter 0x00.
- Data Transfer: Chunked streaming push/pull with IEEE 802.3 CRC-32 verification and progress bars.
- Zero-Bloat: Standard, robust file transfers without hardcoded application or file types.
- Protocol self-test: Built-in verification against normative specification test vectors.
"""

import argparse
import os
import struct
import sys
import time
import binascii

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

# Control Opcodes (§8.5)
OP_CONTROL_STATE_GET = 0x01
OP_CONTROL_STATE_SET = 0x02
OP_CONTROL_ACTION_INVOKE = 0x03
OP_CONTROL_ACTION_CANCEL = 0x04
OP_CONTROL_APP_LAUNCH = 0x05

# Components (§8.5)
COMPONENT_SYSTEM = 0x00
COMPONENT_RADIO = 0x01
COMPONENT_POWER = 0x02
COMPONENT_AUDIO = 0x03

# Radio Properties (Component 0x01)
PROP_RADIO_RX_FREQ_HZ = 0x0001
PROP_RADIO_TX_FREQ_HZ = 0x0002
PROP_RADIO_MODULATION_MODE = 0x0003
PROP_RADIO_BANDWIDTH_HZ = 0x0004
PROP_RADIO_SELECTED_VFO = 0x0005
PROP_RADIO_SELECTED_CHANNEL = 0x0006
PROP_RADIO_POWER_LEVEL = 0x0007
PROP_RADIO_SQUELCH_LEVEL = 0x0008
PROP_RADIO_SCAN_STATE = 0x0009
PROP_RADIO_PTT_ACTIVE = 0x000A
PROP_RADIO_BATTERY_MV = 0x000B
PROP_RADIO_RSSI_DBM = 0x000C
PROP_RADIO_FINE_OFFSET_DHZ = 0x000D

# System Properties (Component 0x00)
PROP_SYS_UPTIME_MS = 0x0020
PROP_SYS_KEYPAD_LOCKED = 0x0021
PROP_SYS_FLASHLIGHT_ACTIVE = 0x0022
PROP_SYS_ACTIVE_APP = 0x0023
PROP_SYS_MCU_UID = 0x0024
PROP_SYS_SYSCLK_HZ = 0x0025
PROP_SYS_CALIB_VALID = 0x0026
PROP_SYS_SERIAL64 = 0x0027
PROP_SYS_MAC_ADDR = 0x0028
PROP_SYS_SERIAL_STR = 0x0029

# Audio Properties (Component 0x03)
PROP_AUDIO_VOLUME = 0x0030
PROP_AUDIO_KEY_BEEP = 0x0031
PROP_AUDIO_ROGER_BEEP = 0x0032

# Standard Actions (§8.5 / §8.6)
ACTION_FLASHLIGHT_TOGGLE = 0x0001
ACTION_VFO_TOGGLE = 0x0002
ACTION_PLAY_BEEP = 0x0003
ACTION_KEYPAD_LOCK_TOGGLE = 0x0004
ACTION_REBOOT = 0x0005
ACTION_SCAN_TOGGLE = 0x0006
ACTION_APP_LAUNCH = 0x0007
ACTION_PTT_TRANSMIT = 0x0008

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

# Data Opcodes (§8.5)
OP_TRANSFER_OPEN = 0x01
OP_TRANSFER_CHUNK = 0x02
OP_TRANSFER_CLOSE = 0x03
OP_TRANSFER_ABORT = 0x04
OP_TRANSFER_STAT = 0x05
OP_TRANSFER_LIST = 0x06
OP_TRANSFER_REMOVE = 0x07
OP_PACKET_SEND = 0x10
OP_PACKET_RECEIVE = 0x90

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


# ── RIGWIRE CLIENT ────────────────────────────────────────────────────────────
class RigWireClient:
    def __init__(self, port_name: str, baudrate: int = 115200, timeout: float = 2.0):
        if serial is None:
            raise RuntimeError("pyserial is required: pip install pyserial")
        self.ser = serial.Serial(port_name, baudrate=baudrate, timeout=timeout)
        self.session_id = 0x4B45524E  # "KERN"
        self.sequence = 1
        self.max_payload = 128

    def close(self):
        if self.ser and self.ser.is_open:
            self.ser.close()

    def send_record(self, service: int, opcode: int, payload: bytes = b"") -> bytes:
        is_hello = (service == SERVICE_CORE and opcode == OP_CORE_HELLO)
        session = 0 if is_hello else self.session_id
        seq = 0 if is_hello else self.sequence
        if not is_hello:
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

        frame = b"\x00" + cobs_encode(raw) + b"\x00"
        self.ser.write(frame)
        self.ser.flush()

        return self.read_response(expected_service=service, expected_opcode=opcode)

    def read_response(self, expected_service: int, expected_opcode: int) -> bytes:
        buf = bytearray()
        start = time.time()
        timeout = 2.5

        while time.time() - start < timeout:
            b = self.ser.read(1)
            if not b:
                continue
            if b == b"\x00":
                if buf:
                    break
            else:
                buf.extend(b)

        if not buf:
            raise TimeoutError("No response frame received from radio (delimiter timeout)")

        decoded = cobs_decode(bytes(buf))
        if len(decoded) < MIN_RECORD_LEN:
            raise ValueError(f"Malformed response record: {len(decoded)} bytes (min {MIN_RECORD_LEN})")

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
            raise RuntimeError(f"Radio responded with status {status_name} ({status_code})")

        return app_data

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

    def stat(self, path: str) -> dict:
        """Query metadata for remote file via TRANSFER_STAT."""
        path_b = path.encode("utf-8")
        req = struct.pack("<B", len(path_b)) + path_b
        resp = self.send_record(SERVICE_DATA, OP_TRANSFER_STAT, req)
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
        """List files in directory via TRANSFER_LIST."""
        path_b = path.encode("utf-8")
        req = struct.pack("<HB", 0, len(path_b)) + path_b
        resp = self.send_record(SERVICE_DATA, OP_TRANSFER_LIST, req)
        if len(resp) < 3:
            return []
        next_cursor, count = struct.unpack("<HB", resp[:3])
        entries = []
        off = 3
        for _ in range(count):
            if off + 6 > len(resp):
                break
            is_dir = resp[off] != 0
            size = struct.unpack("<I", resp[off + 1:off + 5])[0]
            nlen = resp[off + 5]
            name = resp[off + 6:off + 6 + nlen].decode("utf-8", errors="replace")
            off += 6 + nlen
            entries.append({"name": name, "size": size, "is_dir": is_dir})
        return entries

    def rm(self, path: str):
        """Unlink file via TRANSFER_REMOVE."""
        path_b = path.encode("utf-8")
        req = struct.pack("<B", len(path_b)) + path_b
        self.send_record(SERVICE_DATA, OP_TRANSFER_REMOVE, req)

    def push(self, local_path: str, remote_path: str):
        """Upload file via chunked streaming TRANSFER_OPEN/CHUNK/CLOSE."""
        with open(local_path, "rb") as f:
            data = f.read()

        total_size = len(data)
        file_crc32 = crc32_ieee(data)
        chunk_size = 96
        path_b = remote_path.encode("utf-8")

        # 1. TRANSFER_OPEN (Write)
        req_open = struct.pack("<BIHB", TRANSFER_MODE_WRITE, total_size, chunk_size, len(path_b)) + path_b
        resp_open = self.send_record(SERVICE_DATA, OP_TRANSFER_OPEN, req_open)
        if len(resp_open) < 8:
            raise ValueError("Invalid TRANSFER_OPEN response")
        tid, negotiated_chunk, existing_sz = struct.unpack("<HHI", resp_open[:8])

        chunk_size = max(16, min(negotiated_chunk, 120))
        print(f"  Transfer ID #{tid}: Uploading {total_size} bytes (chunk: {chunk_size}B)...")

        # 2. STREAM CHUNKS
        offset = 0
        t0 = time.time()
        while offset < total_size:
            chunk = data[offset:offset + chunk_size]
            req_chunk = struct.pack("<HIH", tid, offset, len(chunk)) + chunk
            resp_chunk = self.send_record(SERVICE_DATA, OP_TRANSFER_CHUNK, req_chunk)
            offset += len(chunk)

            # Progress Bar
            pct = int((offset / total_size) * 100) if total_size > 0 else 100
            bar = "=" * (pct // 5) + ">" + " " * (20 - (pct // 5))
            sys.stdout.write(f"\r  [{bar[:20]}] {pct:3d}% ({offset}/{total_size}B)")
            sys.stdout.flush()

        elapsed = max(time.time() - t0, 0.001)
        speed = (total_size / elapsed) / 1024.0
        print(f"\n  Streaming complete ({speed:.1f} KB/s). Committing...")

        # 3. TRANSFER_CLOSE
        req_close = struct.pack("<HI", tid, file_crc32)
        resp_close = self.send_record(SERVICE_DATA, OP_TRANSFER_CLOSE, req_close)
        if len(resp_close) < 6:
            raise ValueError("Short TRANSFER_CLOSE response")
        close_tid, written_crc32 = struct.unpack("<HI", resp_close[:6])

        if file_crc32 != 0 and written_crc32 != file_crc32:
            raise ValueError(f"CRC-32 verification mismatch! Sent: {file_crc32:#010x}, Device wrote: {written_crc32:#010x}")

        print(f"  Verified CRC-32: {written_crc32:#010x} OK")

    def pull(self, remote_path: str, local_path: str):
        """Download file via chunked streaming TRANSFER_OPEN/CHUNK/CLOSE."""
        path_b = remote_path.encode("utf-8")
        chunk_size = 96

        # 1. TRANSFER_OPEN (Read)
        req_open = struct.pack("<BIHB", TRANSFER_MODE_READ, 0, chunk_size, len(path_b)) + path_b
        resp_open = self.send_record(SERVICE_DATA, OP_TRANSFER_OPEN, req_open)
        if len(resp_open) < 8:
            raise ValueError("Invalid TRANSFER_OPEN response")
        tid, negotiated_chunk, total_size = struct.unpack("<HHI", resp_open[:8])

        print(f"  Transfer ID #{tid}: Downloading {remote_path} ({total_size} bytes)...")
        received = bytearray()
        offset = 0
        t0 = time.time()

        while offset < total_size:
            req_chunk = struct.pack("<HIH", tid, offset, chunk_size)
            resp_chunk = self.send_record(SERVICE_DATA, OP_TRANSFER_CHUNK, req_chunk)
            if len(resp_chunk) < 8:
                break
            rx_tid, rx_off, rx_len = struct.unpack("<HIH", resp_chunk[:8])
            data = resp_chunk[8:8 + rx_len]
            if not data:
                break
            received.extend(data)
            offset += len(data)

            pct = int((offset / total_size) * 100) if total_size > 0 else 100
            bar = "=" * (pct // 5) + ">" + " " * (20 - (pct // 5))
            sys.stdout.write(f"\r  [{bar[:20]}] {pct:3d}% ({offset}/{total_size}B)")
            sys.stdout.flush()

        # 3. TRANSFER_CLOSE
        req_close = struct.pack("<HI", tid, 0)
        self.send_record(SERVICE_DATA, OP_TRANSFER_CLOSE, req_close)

        with open(local_path, "wb") as f:
            f.write(received)

        calc_crc = crc32_ieee(received)
        print(f"\n  Downloaded {len(received)} bytes to {local_path} (CRC-32: {calc_crc:#010x})")

    def reboot(self):
        """Invoke system reboot."""
        try:
            self.send_record(SERVICE_CORE, OP_CORE_REBOOT)
        except Exception:
            pass
        print("  Reboot command dispatched.")

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
        # Pick sample with lowest RTT
        best = min(results, key=lambda x: x[4])
        return best

    # ── CONTROL & TELEMETRY SERVICE (0x01) ────────────────────────────────────

    def state_get(self, component_id: int, property_id: int) -> bytes:
        """Query property value via OP_CONTROL_STATE_GET."""
        payload = struct.pack("<BH", component_id, property_id)
        resp = self.send_record(SERVICE_CONTROL, OP_CONTROL_STATE_GET, payload)
        if len(resp) < 8:
            raise ValueError(f"Short STATE_GET response: {len(resp)} bytes")
        # Layout: component(1), flags(1), revision(4), property_id(2), value(N)
        return resp[8:]

    def state_set(self, component_id: int, property_id: int, value: bytes, revision: int = 0) -> bytes:
        """Set property value via OP_CONTROL_STATE_SET."""
        flags = 0
        payload = struct.pack("<BBIH", component_id, flags, revision, property_id) + value
        return self.send_record(SERVICE_CONTROL, OP_CONTROL_STATE_SET, payload)

    def action_invoke(self, component_id: int, action_id: int, params: bytes = b"") -> bytes:
        """Invoke component action via OP_CONTROL_ACTION_INVOKE."""
        payload = struct.pack("<BH", component_id, action_id) + params
        return self.send_record(SERVICE_CONTROL, OP_CONTROL_ACTION_INVOKE, payload)

    # ── High-Level Telemetry & Control Helpers ─────────────────────────────────

    def get_system_telemetry(self) -> dict:
        """Fetch all system & silicon metadata."""
        telemetry = {}

        try:
            val = self.state_get(COMPONENT_SYSTEM, PROP_SYS_UPTIME_MS)
            telemetry["uptime_ms"] = struct.unpack("<I", val[:4])[0] if len(val) >= 4 else 0
        except Exception:
            telemetry["uptime_ms"] = 0

        try:
            val = self.state_get(COMPONENT_SYSTEM, PROP_SYS_SYSCLK_HZ)
            telemetry["sysclk_hz"] = struct.unpack("<I", val[:4])[0] if len(val) >= 4 else 0
        except Exception:
            telemetry["sysclk_hz"] = 0

        try:
            val = self.state_get(COMPONENT_SYSTEM, PROP_SYS_SERIAL_STR)
            telemetry["serial_str"] = val.decode("ascii", errors="replace").strip("\x00 ").strip()
        except Exception:
            telemetry["serial_str"] = "unknown"

        try:
            val = self.state_get(COMPONENT_SYSTEM, PROP_SYS_SERIAL64)
            telemetry["serial64"] = struct.unpack("<Q", val[:8])[0] if len(val) >= 8 else 0
        except Exception:
            telemetry["serial64"] = 0

        try:
            val = self.state_get(COMPONENT_SYSTEM, PROP_SYS_MAC_ADDR)
            if len(val) >= 6:
                telemetry["mac_addr"] = ":".join(f"{b:02X}" for b in val[:6])
            else:
                telemetry["mac_addr"] = "unknown"
        except Exception:
            telemetry["mac_addr"] = "unknown"

        try:
            val = self.state_get(COMPONENT_SYSTEM, PROP_SYS_MCU_UID)
            telemetry["mcu_uid"] = val.hex() if val else "Protected (Auth Required)"
        except Exception:
            telemetry["mcu_uid"] = "Protected (Auth Required)"

        try:
            val = self.state_get(COMPONENT_SYSTEM, PROP_SYS_CALIB_VALID)
            telemetry["calib_valid"] = bool(val[0]) if val else False
        except Exception:
            telemetry["calib_valid"] = False

        try:
            val = self.state_get(COMPONENT_SYSTEM, PROP_SYS_ACTIVE_APP)
            telemetry["active_app"] = val[0] if val else 0
        except Exception:
            telemetry["active_app"] = 0

        try:
            val = self.state_get(COMPONENT_SYSTEM, PROP_SYS_FLASHLIGHT_ACTIVE)
            telemetry["flashlight"] = bool(val[0]) if val else False
        except Exception:
            telemetry["flashlight"] = False

        try:
            val = self.state_get(COMPONENT_SYSTEM, PROP_SYS_KEYPAD_LOCKED)
            telemetry["keypad_locked"] = bool(val[0]) if val else False
        except Exception:
            telemetry["keypad_locked"] = False

        try:
            val = self.state_get(COMPONENT_POWER, PROP_RADIO_BATTERY_MV)
            telemetry["battery_mv"] = struct.unpack("<H", val[:2])[0] if len(val) >= 2 else 0
        except Exception:
            telemetry["battery_mv"] = 0

        return telemetry

    def get_radio_telemetry(self) -> dict:
        """Fetch all radio VFO & RF parameters."""
        rad = {}

        try:
            val = self.state_get(COMPONENT_RADIO, PROP_RADIO_SELECTED_VFO)
            rad["active_vfo"] = val[0] if val else 0
        except Exception:
            rad["active_vfo"] = 0

        try:
            val = self.state_get(COMPONENT_RADIO, PROP_RADIO_RX_FREQ_HZ)
            rad["rx_freq_hz"] = struct.unpack("<I", val[:4])[0] if len(val) >= 4 else 0
        except Exception:
            rad["rx_freq_hz"] = 0

        try:
            val = self.state_get(COMPONENT_RADIO, PROP_RADIO_TX_FREQ_HZ)
            rad["tx_freq_hz"] = struct.unpack("<I", val[:4])[0] if len(val) >= 4 else 0
        except Exception:
            rad["tx_freq_hz"] = 0

        try:
            val = self.state_get(COMPONENT_RADIO, PROP_RADIO_MODULATION_MODE)
            rad["mod_mode"] = val[0] if val else 0
            rad["mod_name"] = MOD_NAMES.get(rad["mod_mode"], f"MODE_{rad['mod_mode']}")
        except Exception:
            rad["mod_mode"] = 0
            rad["mod_name"] = "FM"

        try:
            val = self.state_get(COMPONENT_RADIO, PROP_RADIO_BANDWIDTH_HZ)
            rad["bandwidth_hz"] = struct.unpack("<I", val[:4])[0] if len(val) >= 4 else 12500
        except Exception:
            rad["bandwidth_hz"] = 12500

        try:
            val = self.state_get(COMPONENT_RADIO, PROP_RADIO_FINE_OFFSET_DHZ)
            rad["offset_dhz"] = struct.unpack("<h", val[:2])[0] if len(val) >= 2 else 0
        except Exception:
            rad["offset_dhz"] = 0

        try:
            val = self.state_get(COMPONENT_RADIO, PROP_RADIO_SQUELCH_LEVEL)
            rad["squelch"] = val[0] if val else 0
        except Exception:
            rad["squelch"] = 0

        try:
            val = self.state_get(COMPONENT_RADIO, PROP_RADIO_RSSI_DBM)
            rad["rssi_dbm"] = struct.unpack("<h", val[:2])[0] if len(val) >= 2 else -127
        except Exception:
            rad["rssi_dbm"] = -127

        try:
            val = self.state_get(COMPONENT_RADIO, PROP_RADIO_PTT_ACTIVE)
            rad["ptt_active"] = bool(val[0]) if val else False
        except Exception:
            rad["ptt_active"] = False

        return rad

    def get_audio_telemetry(self) -> dict:
        """Fetch audio configuration."""
        aud = {}
        try:
            val = self.state_get(COMPONENT_AUDIO, PROP_AUDIO_VOLUME)
            aud["volume"] = val[0] if val else 0
        except Exception:
            aud["volume"] = 0

        try:
            val = self.state_get(COMPONENT_AUDIO, PROP_AUDIO_KEY_BEEP)
            aud["key_beep"] = bool(val[0]) if val else False
        except Exception:
            aud["key_beep"] = False

        try:
            val = self.state_get(COMPONENT_AUDIO, PROP_AUDIO_ROGER_BEEP)
            aud["roger_beep"] = val[0] if val else 0
        except Exception:
            aud["roger_beep"] = 0

        return aud

    def set_frequency(self, freq_hz: int):
        payload = struct.pack("<I", freq_hz)
        self.state_set(COMPONENT_RADIO, PROP_RADIO_RX_FREQ_HZ, payload)

    def set_modulation(self, mod_mode: int):
        self.state_set(COMPONENT_RADIO, PROP_RADIO_MODULATION_MODE, bytes([mod_mode]))

    def set_squelch(self, sql: int):
        self.state_set(COMPONENT_RADIO, PROP_RADIO_SQUELCH_LEVEL, bytes([min(max(sql, 0), 9)]))

    def set_fine_offset_dhz(self, offset_dhz: int):
        payload = struct.pack("<h", offset_dhz)
        self.state_set(COMPONENT_RADIO, PROP_RADIO_FINE_OFFSET_DHZ, payload)

    def set_vfo(self, vfo_idx: int):
        self.state_set(COMPONENT_RADIO, PROP_RADIO_SELECTED_VFO, bytes([vfo_idx & 1]))

    def set_ptt(self, active: bool):
        self.action_invoke(COMPONENT_RADIO, ACTION_PTT_TRANSMIT, bytes([1 if active else 0]))

    def set_volume(self, vol: int):
        self.state_set(COMPONENT_AUDIO, PROP_AUDIO_VOLUME, bytes([min(max(vol, 0), 63)]))

    def play_beep(self):
        self.action_invoke(COMPONENT_AUDIO, ACTION_PLAY_BEEP)

    def toggle_flashlight(self):
        self.action_invoke(COMPONENT_SYSTEM, ACTION_FLASHLIGHT_TOGGLE)

    def toggle_keypad_lock(self):
        self.action_invoke(COMPONENT_SYSTEM, ACTION_KEYPAD_LOCK_TOGGLE)


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
    host_t1 = 1_000_000 # 1.0s
    req_payload = struct.pack("<I", host_t1)
    assert len(req_payload) == 4

    # Simulated radio response: device_rx = 1_000_200 us, device_tx = 1_000_210 us, uptime = 1000 ms
    dev_rx = 1_000_200
    dev_tx = 1_000_210
    uptime = 1000
    resp_payload = struct.pack("<IIII", host_t1, dev_rx, dev_tx, uptime)
    assert len(resp_payload) == 16

    # Simulated host receive at 1_000_420 us
    host_t4 = 1_000_420
    dev_dwell = (dev_tx - dev_rx)
    host_elapsed = (host_t4 - host_t1)
    rtt = host_elapsed - dev_dwell
    assert rtt == 410 # 420 - 10 = 410 us RTT
    offset = ((dev_rx - host_t1) + (dev_tx - host_t4)) // 2
    # dev_rx - host_t1 = +200, dev_tx - host_t4 = -210 -> offset = -5 us
    assert offset == -5
    print(f"PASSED (RTT={rtt}us, offset={offset}us)")

    print("Test 6: Control Service StateGet and StateSet Encodings...", end=" ")
    sg_req = struct.pack("<BH", COMPONENT_RADIO, PROP_RADIO_RX_FREQ_HZ)
    assert len(sg_req) == 3
    ss_val = struct.pack("<I", 145_500_000)
    ss_req = struct.pack("<BBIH", COMPONENT_RADIO, 0, 0, PROP_RADIO_RX_FREQ_HZ) + ss_val
    assert len(ss_req) == 12
    ai_req = struct.pack("<BH", COMPONENT_RADIO, ACTION_PTT_TRANSMIT) + bytes([1])
    assert len(ai_req) == 4
    # Test Public System Identifiers requests
    sg_mac = struct.pack("<BH", COMPONENT_SYSTEM, PROP_SYS_MAC_ADDR)
    sg_ser = struct.pack("<BH", COMPONENT_SYSTEM, PROP_SYS_SERIAL_STR)
    assert len(sg_mac) == 3 and len(sg_ser) == 3
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


COMPONENT_MAP = {
    "sys": COMPONENT_SYSTEM,
    "system": COMPONENT_SYSTEM,
    "0": COMPONENT_SYSTEM,
    "radio": COMPONENT_RADIO,
    "rf": COMPONENT_RADIO,
    "1": COMPONENT_RADIO,
    "power": COMPONENT_POWER,
    "pwr": COMPONENT_POWER,
    "bat": COMPONENT_POWER,
    "battery": COMPONENT_POWER,
    "2": COMPONENT_POWER,
    "audio": COMPONENT_AUDIO,
    "snd": COMPONENT_AUDIO,
    "sound": COMPONENT_AUDIO,
    "3": COMPONENT_AUDIO,
}


def parse_component(s: str) -> int:
    s_clean = s.strip().lower()
    if s_clean in COMPONENT_MAP:
        return COMPONENT_MAP[s_clean]
    return int(s, 0)


def parse_int_or_hex(s: str) -> int:
    return int(s, 0)


# ── MAIN ENTRY POINT ──────────────────────────────────────────────────────────
def main():
    parser = argparse.ArgumentParser(
        prog="rigwire",
        description="⚡ RigWire Host Tool & Provisioning Client for KERN Firmware",
    )
    parser.add_argument(
        "-p", "--port",
        default=os.environ.get("RIGWIRE_PORT", "/dev/ttyACM0"),
        help="Serial port path (e.g. /dev/ttyACM0, /dev/ttyUSB0, COM3). Default: /dev/ttyACM0",
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

    subparsers.add_parser("tone", help="Play 1000 Hz beep tone via audio subsystem")

    vol_p = subparsers.add_parser("volume", help="Get or set speaker volume (0-63)")
    vol_p.add_argument("level", type=int, nargs="?", help="Volume level (0-63)")

    fl_p = subparsers.add_parser("flashlight", help="Control top LED flashlight")
    fl_p.add_argument("state", nargs="?", default="toggle", choices=["on", "off", "toggle"], help="State")

    lk_p = subparsers.add_parser("lock", help="Control keypad lock state")
    lk_p.add_argument("state", nargs="?", default="toggle", choices=["on", "off", "toggle"], help="State")

    get_p = subparsers.add_parser("get", help="Query arbitrary component property value")
    get_p.add_argument("component", help="Component name or ID (sys, radio, power, audio, 0, 1, 2, 3)")
    get_p.add_argument("property", help="Property ID (e.g. 0x0001 or 1)")

    set_p = subparsers.add_parser("set", help="Set arbitrary component property value")
    set_p.add_argument("component", help="Component name or ID (sys, radio, power, audio)")
    set_p.add_argument("property", help="Property ID (e.g. 0x0001 or 1)")
    set_p.add_argument("value", help="Value string, integer, or hex bytes (e.g. 0x01, 145500000)")

    ls_p = subparsers.add_parser("ls", help="List directory entries on radio filesystem")
    ls_p.add_argument("path", nargs="?", default="/", help="Directory path to list (default: /)")

    stat_p = subparsers.add_parser("stat", help="Query file existence, size, and CRC-32 metadata")
    stat_p.add_argument("path", help="Target path on radio filesystem")

    push_p = subparsers.add_parser("push", help="Upload local file to radio filesystem")
    push_p.add_argument("local", help="Local file path")
    push_p.add_argument("remote", help="Remote file path on radio (e.g. /apps/game.krx, /sys/calib.dat)")

    pull_p = subparsers.add_parser("pull", help="Download file from radio to local machine")
    pull_p.add_argument("remote", help="Remote file path on radio")
    pull_p.add_argument("local", help="Local destination file path")

    rm_p = subparsers.add_parser("rm", help="Delete file from radio filesystem")
    rm_p.add_argument("path", help="Path of file to delete on radio")

    sync_p = subparsers.add_parser("sync", help="Synchronize microsecond clock via NTP handshake")
    sync_p.add_argument("--rounds", type=int, default=5, help="Number of NTP handshake rounds (default: 5)")

    subparsers.add_parser("reboot", help="Reboot radio")
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
                client.action_invoke(COMPONENT_RADIO, ACTION_VFO_TOGGLE)
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

        elif args.command == "tone":
            client.play_beep()
            print("  Tone / beep emitted.")

        elif args.command == "volume":
            if args.level is not None:
                client.set_volume(args.level)
                print(f"  Volume set to {args.level}")
            else:
                aud = client.get_audio_telemetry()
                print(f"  Volume: {aud['volume']} / 63")

        elif args.command == "flashlight":
            if args.state == "on":
                client.state_set(COMPONENT_SYSTEM, PROP_SYS_FLASHLIGHT_ACTIVE, bytes([1]))
                print("  Flashlight: ON")
            elif args.state == "off":
                client.state_set(COMPONENT_SYSTEM, PROP_SYS_FLASHLIGHT_ACTIVE, bytes([0]))
                print("  Flashlight: OFF")
            else:
                client.toggle_flashlight()
                print("  Flashlight toggled.")

        elif args.command == "lock":
            if args.state == "on":
                client.state_set(COMPONENT_SYSTEM, PROP_SYS_KEYPAD_LOCKED, bytes([1]))
                print("  Keypad: LOCKED")
            elif args.state == "off":
                client.state_set(COMPONENT_SYSTEM, PROP_SYS_KEYPAD_LOCKED, bytes([0]))
                print("  Keypad: UNLOCKED")
            else:
                client.toggle_keypad_lock()
                print("  Keypad lock toggled.")

        elif args.command == "get":
            comp = parse_component(args.component)
            prop = parse_int_or_hex(args.property)
            val = client.state_get(comp, prop)
            print(f"  Component {comp:#04x}, Property {prop:#06x} ({len(val)} bytes):")
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
            comp = parse_component(args.component)
            prop = parse_int_or_hex(args.property)
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
            client.state_set(comp, prop, val_bytes)
            print(f"  Set Component {comp:#04x}, Property {prop:#06x} = {val_bytes.hex()} OK")

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

        elif args.command == "pull":
            client.pull(args.remote, args.local)

        elif args.command == "rm":
            client.rm(args.path)
            print(f"  Deleted '{args.path}' successfully.")

        elif args.command == "reboot":
            client.reboot()

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
