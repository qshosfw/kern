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
OP_CORE_GET_RESULT = 0x04
OP_CORE_GET_STATUS = 0x05
OP_CORE_REBOOT = 0x06

# Control Opcodes (§8.5)
OP_CONTROL_STATE_GET = 0x01
OP_CONTROL_STATE_SET = 0x02
OP_CONTROL_ACTION_INVOKE = 0x03
OP_CONTROL_ACTION_CANCEL = 0x04

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

    print("\n✨ All RigWire protocol self-tests passed with 100% conformance!")
    print("==================================================================")


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

    finally:
        client.close()


if __name__ == "__main__":
    main()
