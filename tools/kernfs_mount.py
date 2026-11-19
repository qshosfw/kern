#!/usr/bin/env python3
"""
KernFS Universal Multi-Platform Mounter (Hardened WebDAV Class 2 over RigWire).

Mounts the radio's LittleFS / KernFS natively on Linux, macOS, and Windows via
a localhost-only WebDAV Class 2 bridge (127.0.0.1) with automatic USB-style
volume naming (e.g. "Quansheng UV-K1 1D9E").

Hardening & Anti-Brick Safeguards:
  • DNS-rebinding & CSRF defense (strict loopback Host & Origin allowlists).
  • RFC 4918 finite-depth PROPFIND enforcement (blocks Depth: infinity scans).
  • RFC 4331 SPI flash quota reporting (accurate free space in Finder/Explorer).
  • Conditional ETag support (If-None-Match -> 304, If-Match -> 412).
  • Socket timeouts (15s) and TTL-pruned Class-2 lock table.
  • Single serialized RigWire transaction lock (no interleaved COBS frames).
  • OS junk-file blackhole (.DS_Store, ._*, Thumbs.db, .Spotlight-V100, etc.).
  • Protected /sys/* (read-only) and /apps/{dfu.mod,panic.mod} delete shield.
  • Pre-write KRX package & ORDS-01 table validation before touching SPI flash.
  • Zero-wear idempotent skip on identical CRC-32 + post-write stat verification.
"""

from __future__ import annotations

import argparse
import email.utils
import http.client
import http.server
import mimetypes
import os
import platform
import posixpath
import re
import shutil
import socketserver
import struct
import subprocess
import sys
import threading
import time
import urllib.parse
import uuid
import xml.etree.ElementTree as ET
from pathlib import Path
from typing import Any

TOOLS_DIR = Path(__file__).resolve().parent
if str(TOOLS_DIR) not in sys.path:
    sys.path.insert(0, str(TOOLS_DIR))

import rigwire  # noqa: E402

# =============================================================================
# 1. KERN TUI STYLE HELPERS (Matches tools/builder.py & tools/flasher.py)
# =============================================================================

_USE_COLOR = sys.stdout.isatty() and not os.environ.get("NO_COLOR")
RESET  = "\033[0m"  if _USE_COLOR else ""
BOLD   = "\033[1m"  if _USE_COLOR else ""
INVERT = "\033[7m"  if _USE_COLOR else ""
RED    = "\033[31m" if _USE_COLOR else ""
GREEN  = "\033[32m" if _USE_COLOR else ""
YELLOW = "\033[33m" if _USE_COLOR else ""
BLUE   = "\033[34m" if _USE_COLOR else ""
CYAN   = "\033[36m" if _USE_COLOR else ""
GRAY   = "\033[90m" if _USE_COLOR else ""


def print_header(title: str = "KernFS Universal Mounter") -> None:
    print(f"\n  ⚡ {BOLD}{CYAN}{title}{RESET}")
    print("  ══════════════════════════════════════════════════════════════════")


def log_info(msg: str) -> None: print(f" {BLUE}{INVERT} INFO  {RESET} {msg}")
def log_ok(msg: str) -> None:   print(f" {GREEN}{INVERT}  OK   {RESET} {msg}")
def log_warn(msg: str) -> None: print(f" {YELLOW}{INVERT} WARN  {RESET} {msg}")
def log_err(msg: str) -> None:  print(f" {RED}{INVERT} FAIL  {RESET} {msg}", file=sys.stderr)


# =============================================================================
# 2. DEVICE VOLUME IDENTITY ("Quansheng UV-K1 1D9E" — Matches USB Descriptors)
# =============================================================================

SPI_FLASH_TOTAL_BYTES = 2 * 1024 * 1024  # 2 MiB PY25Q16 default


def _sanitize_volume_name(name: str) -> str:
    cleaned = re.sub(r"[^A-Za-z0-9 _.-]+", "", name).strip()
    cleaned = re.sub(r"\s+", " ", cleaned)
    return cleaned[:32] or "Quansheng UV-K1"


def resolve_volume_name(
    port_name: str | None = None,
    hello: dict[str, Any] | None = None,
    telemetry: dict[str, Any] | None = None,
    override: str | None = None,
) -> str:
    """Resolves USB-style volume name like 'Quansheng UV-K1 1D9E'."""
    if override:
        return _sanitize_volume_name(override)

    # 1. Exact USB String 2 product descriptor from OS if available (e.g. "Quansheng UV-K1 1D9E")
    if port_name and not port_name.startswith("tcp://") and port_name != "demo":
        try:
            from serial.tools import list_ports
            for p in list_ports.comports():
                if p.device == port_name and p.product:
                    prod = p.product.strip()
                    if prod.lower().startswith("quansheng"):
                        return _sanitize_volume_name(prod)
        except Exception:
            pass

    # 2. Construct identically to src/driver/src/usb.rs::update_device_identity("UV-K1", tag)
    model = "UV-K1"
    if hello:
        raw_model = str(hello.get("device") or hello.get("model") or hello.get("target") or "").strip()
        if "k10" in raw_model.lower():
            model = "UV-K10 Pro"
        elif "k5" in raw_model.lower():
            model = "UV-K5 v3"
        elif raw_model:
            model = raw_model.removeprefix("Quansheng ").strip()[:12] or "UV-K1"

    tag = ""
    if telemetry:
        s_str = str(telemetry.get("serial_str", "")).strip()
        alnum = re.sub(r"[^A-Za-z0-9]", "", s_str).upper()
        if len(alnum) >= 4 and alnum != "UNKNOWN":
            tag = alnum[-4:]
        elif isinstance(telemetry.get("serial64"), int) and telemetry["serial64"]:
            tag = f"{telemetry['serial64'] & 0xFFFF:04X}"

    return _sanitize_volume_name(f"Quansheng {model} {tag}" if tag else f"Quansheng {model}")


# =============================================================================
# 3. SAFETY POLICY, OS JUNK BLACKHOLE & FORMAT VALIDATORS
# =============================================================================

MAX_KERNFS_PATH = 32
PKG_EXTS = (".krx", ".app", ".svc", ".dmn", ".drv", ".mod")
PROTECTED_RECOVERY_MODS = frozenset({"/apps/dfu.mod", "/apps/panic.mod"})

ORDS01_TABLES: dict[str, tuple[bytes, int]] = {
    ".ch": (b"CHAN", 64),
    ".ct": (b"CONT", 32),
    ".bnd": (b"BNDP", 32),
    ".zn": (b"ZONE", 32),
    ".hit": (b"HITS", 32),
    ".fm": (b"FMBC", 16),
}

OS_JUNK_NAMES = frozenset({
    ".ds_store", ".localized", ".spotlight-v100", ".trashes", ".fseventsd",
    ".temporaryitems", ".documentrevisions-v100", ".apdisk", ".metadata_never_index",
    ".directory", ".xdg-volume-info", "desktop.ini", "thumbs.db", "ehthumbs.db",
    "$recycle.bin", "system volume information", "autorun.inf",
})


def is_os_junk_path(path: str) -> bool:
    for seg in (p for p in path.split("/") if p):
        low = seg.lower()
        if low in OS_JUNK_NAMES or seg.startswith("._"):
            return True
        if low.startswith(".trash-") or low.startswith(".gvfs"):
            return True
        if seg.endswith("~") or (seg.startswith(".") and seg.endswith(".swp")):
            return True
    return False


def normalize_path(raw_url_path: str) -> str:
    unquoted = urllib.parse.unquote(raw_url_path.split("?", 1)[0])
    if "\\" in unquoted:
        raise ValueError("Backslashes are not allowed in KernFS paths")
    raw_segs = [s for s in unquoted.split("/") if s]
    if any(s == ".." for s in raw_segs):
        raise ValueError("Path traversal ('..') is forbidden")
    norm = posixpath.normpath("/" + "/".join(raw_segs))
    return norm if norm.startswith("/") else "/" + norm


class SafetyPolicy:
    def __init__(
        self,
        *,
        read_only: bool = False,
        allow_sys_write: bool = False,
        allow_system_mod_delete: bool = False,
        validate_formats: bool = True,
    ) -> None:
        self.read_only = read_only
        self.allow_sys_write = allow_sys_write
        self.allow_system_mod_delete = allow_system_mod_delete
        self.validate_formats = validate_formats

    def check_path_valid(self, path: str) -> None:
        raw = path.encode("utf-8")
        if not path.startswith("/") or "//" in path or ".." in path:
            raise PermissionError(f"Invalid KernFS path '{path}'")
        if len(raw) > MAX_KERNFS_PATH:
            raise ValueError(f"Path '{path}' ({len(raw)} B) exceeds KernFS {MAX_KERNFS_PATH}-byte limit")
        if any(ord(c) < 0x20 or ord(c) > 0x7E for c in path):
            raise ValueError(f"Path '{path}' contains invalid characters")

    def check_write(self, path: str, data: bytes) -> None:
        if self.read_only:
            raise PermissionError("Mount is in --read-only mode")
        self.check_path_valid(path)
        if path == "/":
            raise PermissionError("Cannot overwrite root directory")
        if (path == "/sys" or path.startswith("/sys/")) and not self.allow_sys_write:
            raise PermissionError(f"Protected path '{path}' is read-only (pass --unsafe-allow-sys-write)")
        if path == "/sys/calib.dat" and len(data) != 512:
            raise ValueError(f"/sys/calib.dat must be exactly 512 bytes (got {len(data)} B)")
        if path == "/sys/calib.sig" and len(data) != 32:
            raise ValueError(f"/sys/calib.sig must be exactly 32 bytes (got {len(data)} B)")
        if len(data) > rigwire.MAX_WIRE_FILE_SIZE:
            raise ValueError(f"File ({len(data)} B) exceeds RigWire {rigwire.MAX_WIRE_FILE_SIZE}-byte ceiling")

        if not self.validate_formats:
            return

        low = path.lower()
        if low.endswith(PKG_EXTS):
            try:
                rigwire.validate_krx_package(data)
            except Exception as e:
                raise ValueError(f"Rejected invalid KRX package '{path}': {e}") from e

        for ext, (fourcc, stride) in ORDS01_TABLES.items():
            if low.endswith(ext):
                if len(data) < 16:
                    raise ValueError(f"ORDS-01 table '{path}' too short ({len(data)} B < 16 B)")
                if data[:4] != fourcc:
                    raise ValueError(f"ORDS-01 table '{path}' FourCC {data[:4]!r} != {fourcc!r}")
                if (len(data) - 16) % stride != 0:
                    raise ValueError(f"ORDS-01 table '{path}' payload not aligned to {stride} B stride")
                break

    def check_delete(self, path: str) -> None:
        if self.read_only:
            raise PermissionError("Mount is in --read-only mode")
        self.check_path_valid(path)
        if path in ("/", "/apps", "/user", "/pref", "/sys"):
            raise PermissionError(f"Cannot delete core directory '{path}'")
        if path.startswith("/sys/") and not self.allow_sys_write:
            raise PermissionError(f"Protected system file '{path}' cannot be deleted")
        if path in PROTECTED_RECOVERY_MODS and not self.allow_system_mod_delete:
            raise PermissionError(f"Protected recovery module '{path}' cannot be deleted")


# =============================================================================
# 4. BACKENDS (Serialized RigWire Backend + Memory Backend)
# =============================================================================

class FsBackend:
    def stat(self, path: str) -> dict[str, Any]: raise NotImplementedError
    def ls(self, path: str) -> list[dict[str, Any]]: raise NotImplementedError
    def read_bytes(self, path: str) -> bytes: raise NotImplementedError
    def write_bytes(self, path: str, data: bytes) -> dict[str, Any]: raise NotImplementedError
    def remove(self, path: str) -> None: raise NotImplementedError
    def mkdir(self, path: str) -> None: raise NotImplementedError
    def quota_bytes(self) -> tuple[int, int]: return (0, SPI_FLASH_TOTAL_BYTES)
    def close(self) -> None: pass


class RigWireFsBackend(FsBackend):
    """Serialized, CRC-verified RigWire backend with coherent short-TTL caching."""

    def __init__(self, client: rigwire.RigWireClient, cache_ttl: float = 3.0, shared_lock: threading.RLock | None = None) -> None:
        self._client = client
        self._lock = shared_lock if shared_lock is not None else threading.RLock()
        self._cache_ttl = cache_ttl
        self._stat_cache: dict[str, tuple[float, dict[str, Any]]] = {}
        self._ls_cache: dict[str, tuple[float, list[dict[str, Any]]]] = {}
        self._data_cache: dict[tuple[str, int, int], bytes] = {}
        self._virtual_dirs: set[str] = set()

    def _invalidate(self, path: str) -> None:
        parent = posixpath.dirname(path) or "/"
        self._stat_cache.pop(path, None)
        self._stat_cache.pop(parent, None)
        self._ls_cache.pop(path, None)
        self._ls_cache.pop(parent, None)
        for k in [k for k in self._data_cache if k[0] == path]:
            self._data_cache.pop(k, None)

    def stat(self, path: str) -> dict[str, Any]:
        with self._lock:
            now = time.monotonic()
            cached = self._stat_cache.get(path)
            if cached and (now - cached[0]) < self._cache_ttl:
                return dict(cached[1])
            if path == "/":
                st = {"path": "/", "exists": True, "is_dir": True, "size": 0, "crc32": 0}
            else:
                st = self._client.stat(path)
                if not st["exists"] and path in self._virtual_dirs:
                    st = {"path": path, "exists": True, "is_dir": True, "size": 0, "crc32": 0}
            self._stat_cache[path] = (now, st)
            return dict(st)

    def ls(self, path: str) -> list[dict[str, Any]]:
        with self._lock:
            now = time.monotonic()
            cached = self._ls_cache.get(path)
            if cached and (now - cached[0]) < self._cache_ttl:
                return list(cached[1])
            try:
                entries = self._client.ls(path)
            except rigwire.RigWireError as e:
                if e.status_code == 22 and path in self._virtual_dirs:
                    entries = []
                else:
                    raise
            seen = {e["name"] for e in entries}
            prefix = "/" if path == "/" else path + "/"
            for vd in sorted(self._virtual_dirs):
                if vd.startswith(prefix):
                    rest = vd[len(prefix):]
                    if rest and "/" not in rest and rest not in seen:
                        entries.append({"name": rest, "is_dir": True, "size": 0})
                        seen.add(rest)
            self._ls_cache[path] = (now, entries)
            return list(entries)

    def quota_bytes(self) -> tuple[int, int]:
        with self._lock:
            used = sum(st["size"] for _, (_, st) in self._stat_cache.items() if st.get("exists") and not st.get("is_dir"))
            avail = max(65536, SPI_FLASH_TOTAL_BYTES - used)
            return (used, avail)

    def read_bytes(self, path: str) -> bytes:
        with self._lock:
            st = self.stat(path)
            if not st["exists"] or st["is_dir"]:
                raise FileNotFoundError(path)
            ckey = (path, st["size"], st["crc32"])
            if ckey in self._data_cache:
                return self._data_cache[ckey]

            path_b = path.encode("utf-8")
            req_open = struct.pack("<BIHB", rigwire.TRANSFER_MODE_READ, 0, 96, len(path_b)) + path_b
            resp_open = self._client.send_record(rigwire.SERVICE_DATA, rigwire.OP_TRANSFER_OPEN, req_open)
            if len(resp_open) < 8:
                raise RuntimeError("Short TRANSFER_OPEN response")
            tid, negotiated_chunk, total_size = struct.unpack("<HHI", resp_open[:8])
            chunk_size = max(16, min(negotiated_chunk, 120))
            buf = bytearray()
            offset = 0
            try:
                while offset < total_size:
                    req_len = min(chunk_size, total_size - offset)
                    resp_chunk = self._client.send_record(
                        rigwire.SERVICE_DATA, rigwire.OP_TRANSFER_CHUNK, struct.pack("<HIH", tid, offset, req_len)
                    )
                    if len(resp_chunk) < 8:
                        raise RuntimeError("Short TRANSFER_CHUNK response")
                    _, _, rx_len = struct.unpack("<HIH", resp_chunk[:8])
                    chunk = resp_chunk[8 : 8 + rx_len]
                    if not chunk:
                        raise RuntimeError(f"Unexpected EOF at {offset}/{total_size}")
                    buf.extend(chunk)
                    offset += len(chunk)
                self._client.send_record(rigwire.SERVICE_DATA, rigwire.OP_TRANSFER_CLOSE, struct.pack("<HI", tid, 0))
            except Exception:
                self._client._abort_transfer(tid)
                raise

            data = bytes(buf)
            if st["crc32"] and rigwire.crc32_ieee(data) != st["crc32"]:
                raise RuntimeError(f"Read CRC-32 mismatch for {path}")
            if len(self._data_cache) > 64:
                self._data_cache.clear()
            self._data_cache[(path, len(data), rigwire.crc32_ieee(data))] = data
            return data

    def write_bytes(self, path: str, data: bytes) -> dict[str, Any]:
        with self._lock:
            file_crc = rigwire.crc32_ieee(data)
            cur = self._client.stat(path)
            if cur["exists"] and not cur["is_dir"] and cur["size"] == len(data) and cur["crc32"] == file_crc:
                self._stat_cache[path] = (time.monotonic(), cur)
                return {"written": False, "skipped_identical": True, "size": len(data), "crc32": file_crc}

            path_b = path.encode("utf-8")
            req_open = struct.pack("<BIHB", rigwire.TRANSFER_MODE_WRITE, len(data), 96, len(path_b)) + path_b
            resp_open = self._client.send_record(rigwire.SERVICE_DATA, rigwire.OP_TRANSFER_OPEN, req_open)
            if len(resp_open) < 8:
                raise RuntimeError("Short TRANSFER_OPEN response")
            tid, negotiated_chunk, _ = struct.unpack("<HHI", resp_open[:8])
            chunk_size = max(16, min(negotiated_chunk, 120))
            try:
                offset = 0
                while offset < len(data):
                    chunk = data[offset : offset + chunk_size]
                    self._client.send_record(
                        rigwire.SERVICE_DATA, rigwire.OP_TRANSFER_CHUNK,
                        struct.pack("<HIH", tid, offset, len(chunk)) + chunk,
                    )
                    offset += len(chunk)
                resp_close = self._client.send_record(
                    rigwire.SERVICE_DATA, rigwire.OP_TRANSFER_CLOSE, struct.pack("<HI", tid, file_crc)
                )
            except Exception:
                self._client._abort_transfer(tid)
                raise

            if len(resp_close) < 6:
                raise RuntimeError("Short TRANSFER_CLOSE response")
            _, written_crc = struct.unpack("<HI", resp_close[:6])
            if written_crc != file_crc:
                raise RuntimeError(f"Write CRC mismatch: sent {file_crc:#010x}, device {written_crc:#010x}")

            self._invalidate(path)
            st = self._client.stat(path)
            if not st["exists"] or st["is_dir"] or st["size"] != len(data) or st["crc32"] != file_crc:
                raise RuntimeError(f"Post-write stat verification failed for {path}: {st}")
            self._virtual_dirs.discard(path)
            self._stat_cache[path] = (time.monotonic(), st)
            self._data_cache[(path, len(data), file_crc)] = data
            return {"written": True, "skipped_identical": False, "size": len(data), "crc32": file_crc}

    def remove(self, path: str) -> None:
        with self._lock:
            if path in self._virtual_dirs:
                self._virtual_dirs.discard(path)
                self._invalidate(path)
                return
            self._client.rm(path)
            self._invalidate(path)

    def mkdir(self, path: str) -> None:
        with self._lock:
            st = self.stat(path)
            if st["exists"]:
                if st["is_dir"]:
                    return
                raise FileExistsError(f"File exists at '{path}'")
            self._virtual_dirs.add(path)
            self._invalidate(path)

    def close(self) -> None:
        with self._lock:
            self._client.close()


class MemoryFsBackend(FsBackend):
    """In-memory KernFS simulator for --selftest and demo mode."""

    def __init__(self) -> None:
        self._lock = threading.RLock()
        self.dirs: set[str] = {"/", "/apps", "/user", "/pref", "/sys"}
        self.files: dict[str, bytes] = {}
        self.write_count = 0

    def stat(self, path: str) -> dict[str, Any]:
        with self._lock:
            if path in self.dirs:
                return {"path": path, "exists": True, "is_dir": True, "size": 0, "crc32": 0}
            if path in self.files:
                b = self.files[path]
                return {"path": path, "exists": True, "is_dir": False, "size": len(b), "crc32": rigwire.crc32_ieee(b)}
            return {"path": path, "exists": False, "is_dir": False, "size": 0, "crc32": 0}

    def ls(self, path: str) -> list[dict[str, Any]]:
        with self._lock:
            if path not in self.dirs:
                raise FileNotFoundError(path)
            prefix = "/" if path == "/" else path + "/"
            entries: list[dict[str, Any]] = []
            seen: set[str] = set()
            for d in sorted(self.dirs):
                if d != path and d.startswith(prefix):
                    rest = d[len(prefix):]
                    if "/" not in rest and rest not in seen:
                        entries.append({"name": rest, "is_dir": True, "size": 0})
                        seen.add(rest)
            for f, b in sorted(self.files.items()):
                if f.startswith(prefix):
                    rest = f[len(prefix):]
                    if "/" not in rest and rest not in seen:
                        entries.append({"name": rest, "is_dir": False, "size": len(b)})
                        seen.add(rest)
            return entries

    def quota_bytes(self) -> tuple[int, int]:
        with self._lock:
            used = sum(len(b) for b in self.files.values())
            return (used, max(0, SPI_FLASH_TOTAL_BYTES - used))

    def read_bytes(self, path: str) -> bytes:
        with self._lock:
            if path not in self.files:
                raise FileNotFoundError(path)
            return self.files[path]

    def write_bytes(self, path: str, data: bytes) -> dict[str, Any]:
        with self._lock:
            crc = rigwire.crc32_ieee(data)
            existing = self.files.get(path)
            if existing is not None and len(existing) == len(data) and rigwire.crc32_ieee(existing) == crc:
                return {"written": False, "skipped_identical": True, "size": len(data), "crc32": crc}
            self.dirs.add(posixpath.dirname(path) or "/")
            self.files[path] = data
            self.write_count += 1
            return {"written": True, "skipped_identical": False, "size": len(data), "crc32": crc}

    def remove(self, path: str) -> None:
        with self._lock:
            if path in self.files:
                del self.files[path]
                return
            if path in self.dirs and path not in ("/", "/apps", "/user", "/pref", "/sys"):
                self.dirs.discard(path)
                return
            raise FileNotFoundError(path)

    def mkdir(self, path: str) -> None:
        with self._lock:
            if path in self.files:
                raise FileExistsError(path)
            self.dirs.add(path)


# =============================================================================
# 5. HARDENED WEBDAV CLASS 2 SERVER (Loopback-Only, DNS-Rebind Proof, RFC 4331)
# =============================================================================

MAX_LOCKS = 64
LOCK_TTL_SEC = 600.0


class KernWebDAVServer(socketserver.ThreadingMixIn, http.server.HTTPServer):
    daemon_threads = True
    allow_reuse_address = True

    def __init__(
        self,
        server_address: tuple[str, int],
        backend: FsBackend,
        policy: SafetyPolicy,
        *,
        volume_name: str = "Quansheng UV-K1",
        verbose: bool = False,
    ) -> None:
        super().__init__(server_address, KernWebDAVHandler)
        self.backend = backend
        self.policy = policy
        self.volume_name = _sanitize_volume_name(volume_name)
        self.verbose = verbose
        self.lock_table: dict[str, tuple[str, float]] = {}  # token -> (path, expires_mono)
        self.lock_mutex = threading.Lock()
        self.boot_http_date = email.utils.formatdate(time.time(), usegmt=True)


class KernWebDAVHandler(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    server: KernWebDAVServer  # type: ignore[assignment]

    def setup(self) -> None:
        self.request.settimeout(15.0)
        super().setup()

    def log_message(self, fmt: str, *args: Any) -> None:
        if self.server.verbose:
            sys.stderr.write(f" {GRAY}[dav] {self.command} {self.path} — {fmt % args}{RESET}\n")

    def _check_loopback_origin(self) -> bool:
        """Blocks DNS-rebinding & cross-origin browser requests."""
        host = (self.headers.get("Host") or "").strip().lower()
        if host:
            h_only = host.split(":", 1)[0]
            if h_only not in ("127.0.0.1", "localhost"):
                self._send_text_err(403, f"Forbidden Host header '{host}' (loopback only)")
                return False
        origin = (self.headers.get("Origin") or "").strip().lower()
        if origin:
            parsed = urllib.parse.urlparse(origin)
            if parsed.hostname not in ("127.0.0.1", "localhost"):
                self._send_text_err(403, f"Forbidden cross-origin request '{origin}'")
                return False
        return True

    def _send_empty(self, code: int, extra_headers: dict[str, str] | None = None) -> None:
        self.send_response(code)
        self.send_header("Content-Length", "0")
        self.send_header("X-Content-Type-Options", "nosniff")
        if extra_headers:
            for k, v in extra_headers.items():
                self.send_header(k, v)
        self.end_headers()

    def _send_bytes(self, code: int, body: bytes, content_type: str, extra_headers: dict[str, str] | None = None) -> None:
        self.send_response(code)
        self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(len(body)))
        self.send_header("X-Content-Type-Options", "nosniff")
        if extra_headers:
            for k, v in extra_headers.items():
                self.send_header(k, v)
        self.end_headers()
        if self.command != "HEAD":
            self.wfile.write(body)

    def _send_text_err(self, code: int, msg: str) -> None:
        self._send_bytes(code, (msg + "\n").encode("utf-8"), "text/plain; charset=utf-8")

    def _read_body(self, max_bytes: int = rigwire.MAX_WIRE_FILE_SIZE + 4096) -> bytes:
        cl = self.headers.get("Content-Length")
        if cl is not None:
            n = int(cl)
            if n < 0 or n > max_bytes:
                raise OverflowError(f"Body {n} B exceeds {max_bytes} B ceiling")
            return self.rfile.read(n) if n > 0 else b""
        if self.headers.get("Transfer-Encoding", "").lower() == "chunked":
            buf = bytearray()
            while True:
                line = self.rfile.readline(64).strip()
                if not line:
                    break
                sz = int(line.split(b";", 1)[0], 16)
                if sz == 0:
                    self.rfile.readline(64)
                    break
                if len(buf) + sz > max_bytes:
                    raise OverflowError("Chunked body exceeds transfer ceiling")
                buf.extend(self.rfile.read(sz))
                self.rfile.readline(64)
            return bytes(buf)
        return b""

    def _safe_path(self, raw: str | None = None) -> str:
        return normalize_path(self.path if raw is None else raw)

    def _etag(self, st: dict[str, Any]) -> str:
        return f'"{st["size"]:x}-{st["crc32"]:08x}"'

    # --- WebDAV Class 2 Handlers ---

    def do_OPTIONS(self) -> None:
        if not self._check_loopback_origin():
            return
        methods = "OPTIONS, PROPFIND, GET, HEAD" if self.server.policy.read_only else (
            "OPTIONS, PROPFIND, PROPPATCH, GET, HEAD, PUT, DELETE, MKCOL, COPY, MOVE, LOCK, UNLOCK"
        )
        self._send_empty(200, {
            "DAV": "1, 2",
            "MS-Author-Via": "DAV",
            "Allow": methods,
            "Accept-Ranges": "bytes",
        })

    def do_PROPFIND(self) -> None:
        if not self._check_loopback_origin():
            return
        try:
            _ = self._read_body(max_bytes=16384)
            path = self._safe_path()
        except OverflowError as e:
            self._send_text_err(413, str(e))
            return
        except ValueError as e:
            self._send_text_err(400, str(e))
            return

        depth = self.headers.get("Depth", "1").strip().lower()
        if depth == "infinity":
            err_xml = b'<?xml version="1.0" encoding="utf-8"?><D:error xmlns:D="DAV:"><D:propfind-finite-depth/></D:error>'
            self._send_bytes(403, err_xml, "application/xml; charset=utf-8")
            return

        if is_os_junk_path(path):
            self._send_text_err(404, "Not found")
            return

        try:
            st = self.server.backend.stat(path)
        except Exception as e:
            self._send_text_err(502, f"RigWire stat failed: {e}")
            return

        if not st["exists"]:
            self._send_text_err(404, "Not found")
            return

        items: list[tuple[str, dict[str, Any]]] = [(path, st)]
        if st["is_dir"] and depth != "0":
            try:
                children = self.server.backend.ls(path)
            except Exception as e:
                self._send_text_err(502, f"RigWire ls failed: {e}")
                return
            for entry in children:
                cname = entry["name"]
                cpath = "/" + cname if path == "/" else f"{path}/{cname}"
                if is_os_junk_path(cpath):
                    continue
                items.append((cpath, {
                    "path": cpath, "exists": True, "is_dir": bool(entry["is_dir"]),
                    "size": int(entry.get("size", 0)), "crc32": 0,
                }))

        used_b, avail_b = self.server.backend.quota_bytes()
        xml_body = self._build_multistatus(items, used_b, avail_b)
        self._send_bytes(207, xml_body, "application/xml; charset=utf-8")

    def _build_multistatus(self, items: list[tuple[str, dict[str, Any]]], used_b: int, avail_b: int) -> bytes:
        ns = "DAV:"
        root = ET.Element(f"{{{ns}}}multistatus")
        for ipath, st in items:
            resp = ET.SubElement(root, f"{{{ns}}}response")
            href = urllib.parse.quote(ipath, safe="/")
            if st["is_dir"] and not href.endswith("/"):
                href += "/"
            ET.SubElement(resp, f"{{{ns}}}href").text = href
            ps = ET.SubElement(resp, f"{{{ns}}}propstat")
            prop = ET.SubElement(ps, f"{{{ns}}}prop")
            dname = self.server.volume_name if ipath == "/" else posixpath.basename(ipath.rstrip("/"))
            ET.SubElement(prop, f"{{{ns}}}displayname").text = dname
            rt = ET.SubElement(prop, f"{{{ns}}}resourcetype")
            if st["is_dir"]:
                ET.SubElement(rt, f"{{{ns}}}collection")
                ET.SubElement(prop, f"{{{ns}}}quota-used-bytes").text = str(used_b)
                ET.SubElement(prop, f"{{{ns}}}quota-available-bytes").text = str(avail_b)
            else:
                ET.SubElement(prop, f"{{{ns}}}getcontentlength").text = str(st["size"])
                ctype = mimetypes.guess_type(ipath)[0] or "application/octet-stream"
                ET.SubElement(prop, f"{{{ns}}}getcontenttype").text = ctype
                ET.SubElement(prop, f"{{{ns}}}getetag").text = self._etag(st)
            ET.SubElement(prop, f"{{{ns}}}getlastmodified").text = self.server.boot_http_date
            ET.SubElement(ps, f"{{{ns}}}status").text = "HTTP/1.1 200 OK"
        ET.register_namespace("D", ns)
        return b'<?xml version="1.0" encoding="utf-8"?>\n' + ET.tostring(root, encoding="utf-8")

    def do_PROPPATCH(self) -> None:
        if not self._check_loopback_origin():
            return
        try:
            _ = self._read_body(max_bytes=16384)
            path = self._safe_path()
        except Exception as e:
            self._send_text_err(400, str(e))
            return
        if self.server.policy.read_only:
            self._send_text_err(403, "Read-only mount")
            return
        xml = (
            b'<?xml version="1.0" encoding="utf-8"?>'
            b'<D:multistatus xmlns:D="DAV:"><D:response>'
            b"<D:href>" + urllib.parse.quote(path, safe="/").encode("utf-8") + b"</D:href>"
            b"<D:propstat><D:prop/><D:status>HTTP/1.1 200 OK</D:status></D:propstat>"
            b"</D:response></D:multistatus>"
        )
        self._send_bytes(207, xml, "application/xml; charset=utf-8")

    def do_HEAD(self) -> None:
        self.do_GET()

    def do_GET(self) -> None:
        if not self._check_loopback_origin():
            return
        try:
            path = self._safe_path()
        except ValueError as e:
            self._send_text_err(400, str(e))
            return

        if is_os_junk_path(path):
            self._send_text_err(404, "Not found")
            return

        try:
            st = self.server.backend.stat(path)
        except Exception as e:
            self._send_text_err(502, f"RigWire stat failed: {e}")
            return

        if not st["exists"]:
            self._send_text_err(404, "Not found")
            return

        if st["is_dir"]:
            entries = self.server.backend.ls(path)
            lines = [f"<html><body><h3>{self.server.volume_name} — {path}</h3><ul>"]
            if path != "/":
                lines.append('<li><a href="../">../</a></li>')
            for e in entries:
                suffix = "/" if e["is_dir"] else ""
                nm = urllib.parse.quote(e["name"])
                sz = "" if e["is_dir"] else f" ({e.get('size', 0):,} B)"
                lines.append(f'<li><a href="{nm}{suffix}">{e["name"]}{suffix}</a>{sz}</li>')
            lines.append("</ul></body></html>")
            self._send_bytes(200, "\n".join(lines).encode("utf-8"), "text/html; charset=utf-8")
            return

        etag = self._etag(st)
        if self.headers.get("If-None-Match", "").strip() == etag:
            self._send_empty(304, {"ETag": etag})
            return

        ctype = mimetypes.guess_type(path)[0] or "application/octet-stream"
        headers = {"ETag": etag, "Accept-Ranges": "bytes", "Last-Modified": self.server.boot_http_date}
        if self.command == "HEAD":
            self.send_response(200)
            self.send_header("Content-Type", ctype)
            self.send_header("Content-Length", str(st["size"]))
            for k, v in headers.items():
                self.send_header(k, v)
            self.end_headers()
            return

        try:
            data = self.server.backend.read_bytes(path)
        except FileNotFoundError:
            self._send_text_err(404, "Not found")
            return
        except Exception as e:
            self._send_text_err(502, f"RigWire read failed: {e}")
            return

        rng = self.headers.get("Range")
        if rng and rng.startswith("bytes="):
            spec = rng[6:].split(",", 1)[0].strip()
            s_str, _, e_str = spec.partition("-")
            start = int(s_str) if s_str else 0
            end = min(int(e_str) if e_str else len(data) - 1, len(data) - 1)
            if 0 <= start <= end < len(data):
                headers["Content-Range"] = f"bytes {start}-{end}/{len(data)}"
                self._send_bytes(206, data[start : end + 1], ctype, headers)
                return

        self._send_bytes(200, data, ctype, headers)

    def do_PUT(self) -> None:
        if not self._check_loopback_origin():
            return
        try:
            path = self._safe_path()
            data = self._read_body()
        except OverflowError as e:
            self._send_text_err(413, str(e))
            return
        except ValueError as e:
            self._send_text_err(400, str(e))
            return

        if is_os_junk_path(path):
            self._send_empty(204)
            return

        try:
            self.server.policy.check_write(path, data)
        except PermissionError as e:
            self._send_text_err(403, str(e))
            return
        except ValueError as e:
            self._send_text_err(422, str(e))
            return

        try:
            cur_st = self.server.backend.stat(path)
            if_match = self.headers.get("If-Match", "").strip()
            if if_match and if_match != "*" and (not cur_st["exists"] or self._etag(cur_st) != if_match):
                self._send_text_err(412, "ETag precondition failed")
                return
            res = self.server.backend.write_bytes(path, data)
        except Exception as e:
            self._send_text_err(502, f"RigWire write failed: {e}")
            return

        etag = f'"{res["size"]:x}-{res["crc32"]:08x}"'
        self._send_empty(204 if cur_st["exists"] else 201, {"ETag": etag})

    def do_DELETE(self) -> None:
        if not self._check_loopback_origin():
            return
        try:
            path = self._safe_path()
        except ValueError as e:
            self._send_text_err(400, str(e))
            return

        if is_os_junk_path(path):
            self._send_empty(204)
            return

        try:
            self.server.policy.check_delete(path)
        except PermissionError as e:
            self._send_text_err(403, str(e))
            return
        except ValueError as e:
            self._send_text_err(400, str(e))
            return

        try:
            if not self.server.backend.stat(path)["exists"]:
                self._send_text_err(404, "Not found")
                return
            self.server.backend.remove(path)
        except Exception as e:
            self._send_text_err(502, f"RigWire delete failed: {e}")
            return

        self._send_empty(204)

    def do_MKCOL(self) -> None:
        if not self._check_loopback_origin():
            return
        try:
            _ = self._read_body(max_bytes=4096)
            path = self._safe_path()
        except Exception as e:
            self._send_text_err(400, str(e))
            return

        if is_os_junk_path(path):
            self._send_empty(201)
            return
        if self.server.policy.read_only:
            self._send_text_err(403, "Mount is in --read-only mode")
            return
        try:
            self.server.policy.check_path_valid(path)
            if path.startswith("/sys") and not self.server.policy.allow_sys_write:
                raise PermissionError("Cannot create directories under protected /sys")
            self.server.backend.mkdir(path)
        except PermissionError as e:
            self._send_text_err(403, str(e))
            return
        except FileExistsError:
            self._send_text_err(405, "Resource already exists")
            return
        except ValueError as e:
            self._send_text_err(400, str(e))
            return
        self._send_empty(201)

    def _handle_copy_or_move(self, is_move: bool) -> None:
        if not self._check_loopback_origin():
            return
        if self.server.policy.read_only:
            self._send_text_err(403, "Mount is in --read-only mode")
            return
        dest_hdr = self.headers.get("Destination")
        if not dest_hdr:
            self._send_text_err(400, "Missing Destination header")
            return
        try:
            src_path = self._safe_path()
            dst_path = self._safe_path(urllib.parse.urlparse(dest_hdr).path)
        except ValueError as e:
            self._send_text_err(400, str(e))
            return

        if is_os_junk_path(src_path) or is_os_junk_path(dst_path):
            self._send_empty(204)
            return
        if src_path == dst_path:
            self._send_text_err(403, "Source and destination are identical")
            return

        overwrite = self.headers.get("Overwrite", "T").upper() != "F"
        try:
            if is_move:
                self.server.policy.check_delete(src_path)
            src_st = self.server.backend.stat(src_path)
            if not src_st["exists"] or src_st["is_dir"]:
                self._send_text_err(404 if not src_st["exists"] else 409, "Source missing or is directory")
                return
            dst_st = self.server.backend.stat(dst_path)
            if dst_st["exists"] and not overwrite:
                self._send_text_err(412, "Destination exists and Overwrite=F")
                return
            data = self.server.backend.read_bytes(src_path)
            self.server.policy.check_write(dst_path, data)
            self.server.backend.write_bytes(dst_path, data)
            if is_move:
                self.server.backend.remove(src_path)
        except PermissionError as e:
            self._send_text_err(403, str(e))
            return
        except ValueError as e:
            self._send_text_err(422, str(e))
            return
        except Exception as e:
            self._send_text_err(502, f"RigWire {'move' if is_move else 'copy'} failed: {e}")
            return
        self._send_empty(204 if dst_st["exists"] else 201)

    def do_COPY(self) -> None: self._handle_copy_or_move(is_move=False)
    def do_MOVE(self) -> None: self._handle_copy_or_move(is_move=True)

    def do_LOCK(self) -> None:
        if not self._check_loopback_origin():
            return
        try:
            _ = self._read_body(max_bytes=8192)
            path = self._safe_path()
        except Exception as e:
            self._send_text_err(400, str(e))
            return
        if self.server.policy.read_only:
            self._send_text_err(403, "Read-only mount")
            return
        now = time.monotonic()
        token = f"urn:uuid:{uuid.uuid4()}"
        with self.server.lock_mutex:
            expired = [k for k, (_, exp) in self.server.lock_table.items() if exp <= now]
            for k in expired:
                self.server.lock_table.pop(k, None)
            if len(self.server.lock_table) >= MAX_LOCKS:
                oldest = min(self.server.lock_table.items(), key=lambda kv: kv[1][1])[0]
                self.server.lock_table.pop(oldest, None)
            self.server.lock_table[token] = (path, now + LOCK_TTL_SEC)
        xml = (
            '<?xml version="1.0" encoding="utf-8"?>'
            '<D:prop xmlns:D="DAV:"><D:lockdiscovery><D:activelock>'
            "<D:locktype><D:write/></D:locktype><D:lockscope><D:exclusive/></D:lockscope>"
            f"<D:depth>0</D:depth><D:timeout>Second-{int(LOCK_TTL_SEC)}</D:timeout>"
            f"<D:locktoken><D:href>{token}</D:href></D:locktoken>"
            "</D:activelock></D:lockdiscovery></D:prop>"
        ).encode("utf-8")
        self._send_bytes(200, xml, "application/xml; charset=utf-8", {"Lock-Token": f"<{token}>"})

    def do_UNLOCK(self) -> None:
        if not self._check_loopback_origin():
            return
        tok = self.headers.get("Lock-Token", "").strip("<> ")
        with self.server.lock_mutex:
            self.server.lock_table.pop(tok, None)
        self._send_empty(204)


# =============================================================================
# 6. OS NATIVE MOUNTING & OPEN-FOLDER HELPERS
# =============================================================================

def print_mount_instructions(url: str, volume_name: str, mountpoint: str | None) -> None:
    sys_name = platform.system()
    log_ok(f"Volume: {BOLD}{volume_name}{RESET}  |  URL: {CYAN}{url}{RESET}")
    if sys_name == "Linux":
        print(f"   • GIO / GNOME:  {BOLD}gio mount {url}{RESET}")
        print(f"   • File Manager: Open {CYAN}dav://{url.removeprefix('http://')}{RESET}")
        if mountpoint:
            print(f"   • davfs2:       {BOLD}sudo mount -t davfs {url} \"{mountpoint}\"{RESET}")
    elif sys_name == "Darwin":
        mp = mountpoint or f"/Volumes/{volume_name}"
        print(f"   • macOS CLI:    {BOLD}mkdir -p \"{mp}\" && mount_webdav -v \"{volume_name}\" {url} \"{mp}\"{RESET}")
        print(f"   • Finder:       Cmd+K → {CYAN}{url}{RESET}")
    elif sys_name == "Windows":
        drive = mountpoint or "K:"
        print(f"   • Windows CLI:  {BOLD}net use {drive} {url}{RESET}")
        print(f"   • Explorer:     Map Network Drive → {CYAN}{url}{RESET}")
    print(f"  {GRAY}Press Ctrl+C to cleanly unmount and disconnect.{RESET}\n")


def try_auto_mount(url: str, volume_name: str, mountpoint: str | None = None) -> tuple[str | None, list[str] | None]:
    """Mounts natively and returns (local_open_target, unmount_cmd)."""
    sys_name = platform.system()
    if sys_name == "Linux" and shutil.which("gio"):
        res = subprocess.run(["gio", "mount", url], capture_output=True, text=True)
        if res.returncode == 0:
            log_ok(f"Mounted '{volume_name}' via GIO ({url})")
            return f"dav://{url.removeprefix('http://')}", ["gio", "mount", "-u", url]
        log_warn(f"gio mount failed: {res.stderr.strip()}")
    elif sys_name == "Darwin":
        mp = mountpoint or f"/Volumes/{volume_name}"
        Path(mp).mkdir(parents=True, exist_ok=True)
        res = subprocess.run(["mount_webdav", "-v", volume_name, url, mp], capture_output=True, text=True)
        if res.returncode == 0:
            log_ok(f"Mounted '{volume_name}' at {mp}")
            return mp, ["umount", mp]
        log_warn(f"mount_webdav failed: {res.stderr.strip()}")
    elif sys_name == "Windows":
        drive = mountpoint or "K:"
        res = subprocess.run(["net", "use", drive, url], capture_output=True, text=True)
        if res.returncode == 0:
            log_ok(f"Mounted '{volume_name}' as drive {drive}")
            return drive + "\\", ["net", "use", drive, "/delete", "/y"]
        log_warn(f"net use failed: {res.stderr.strip()}")
    return None, None


def open_in_file_manager(target: str) -> None:
    sys_name = platform.system()
    if sys_name == "Linux" and shutil.which("xdg-open"):
        subprocess.Popen(["xdg-open", target], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    elif sys_name == "Darwin":
        subprocess.Popen(["open", target], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    elif sys_name == "Windows":
        os.startfile(target)  # type: ignore[attr-defined]


# =============================================================================
# 7. SELFTEST (WebDAV Class 2 + Security Hardening + Volume Identity)
# =============================================================================

def run_selftest() -> int:
    # Volume name matching USB descriptor format ("Quansheng UV-K1 1D9E")
    vn = resolve_volume_name(hello={"device": "UV-K1"}, telemetry={"serial_str": "7K9MP3R2W81D9E"})
    assert vn == "Quansheng UV-K1 1D9E", f"Unexpected volume name: {vn}"
    vn10 = resolve_volume_name(hello={"device": "UV-K10Pro"}, telemetry={"serial64": 0x1234ABCD})
    assert vn10 == "Quansheng UV-K10 Pro ABCD", f"Unexpected K10 volume name: {vn10}"

    backend = MemoryFsBackend()
    backend.files["/sys/calib.dat"] = b"\xAA" * 512
    backend.files["/sys/calib.sig"] = b"\xBB" * 32
    backend.files["/apps/dfu.mod"] = b"\xC0\xDE\x01\xFE" + b"\x00" * 60
    valid_chan = b"CHAN" + b"\x00" * 12 + b"\x11" * 64
    backend.files["/user/channels.ch"] = valid_chan

    policy = SafetyPolicy(read_only=False, allow_sys_write=False, allow_system_mod_delete=False, validate_formats=True)
    srv = KernWebDAVServer(("127.0.0.1", 0), backend, policy, volume_name=vn)
    port = srv.server_address[1]
    threading.Thread(target=srv.serve_forever, daemon=True).start()

    def req(method: str, path: str, body: bytes = b"", headers: dict[str, str] | None = None) -> tuple[int, bytes, dict[str, str]]:
        conn = http.client.HTTPConnection("127.0.0.1", port, timeout=5)
        try:
            conn.request(method, path, body=body, headers=headers or {})
            r = conn.getresponse()
            return r.status, r.read(), {k.lower(): v for k, v in r.getheaders()}
        finally:
            conn.close()

    try:
        # 1. DNS-rebinding & CSRF rejection
        st, _, _ = req("GET", "/", headers={"Host": "evil.example.com"})
        assert st == 403, f"DNS rebind Host not blocked: {st}"
        st, _, _ = req("GET", "/", headers={"Origin": "https://evil.example.com"})
        assert st == 403, f"Cross-origin Origin not blocked: {st}"

        # 2. PROPFIND root exposes USB-style volume name + RFC 4331 quota; Depth: infinity blocked
        st, body, _ = req("PROPFIND", "/", headers={"Depth": "1"})
        assert st == 207 and b"Quansheng UV-K1 1D9E" in body and b"quota-available-bytes" in body
        st, _, _ = req("PROPFIND", "/", headers={"Depth": "infinity"})
        assert st == 403, f"Depth: infinity not rejected: {st}"

        # 3. GET + Range + ETag 304 Not Modified
        st, body, hdrs = req("GET", "/sys/calib.dat")
        assert st == 200 and len(body) == 512 and "etag" in hdrs
        st, _, _ = req("GET", "/sys/calib.dat", headers={"If-None-Match": hdrs["etag"]})
        assert st == 304, f"Expected 304 Not Modified, got {st}"
        st, body, hdrs = req("GET", "/sys/calib.dat", headers={"Range": "bytes=0-15"})
        assert st == 206 and len(body) == 16

        # 4. Anti-brick shields: /sys/calib.dat & /apps/dfu.mod
        assert req("PUT", "/sys/calib.dat", body=b"\x00" * 512)[0] == 403
        assert req("DELETE", "/sys/calib.dat")[0] == 403
        assert req("DELETE", "/apps/dfu.mod")[0] == 403
        assert req("PUT", "/apps/broken.krx", body=b"NOT_A_KRX")[0] == 422
        assert req("PUT", "/user/bad.ch", body=b"CHAN" + b"\x00" * 12 + b"\x22" * 17)[0] == 422

        # 5. Valid write + If-Match + zero-wear idempotency + OS junk blackhole
        w0 = backend.write_count
        st, _, hdrs = req("PUT", "/user/new.ch", body=valid_chan)
        assert st == 201 and backend.write_count == w0 + 1
        assert req("PUT", "/user/new.ch", body=valid_chan, headers={"If-Match": '"wrong"'})[0] == 412
        assert req("PUT", "/user/new.ch", body=valid_chan)[0] == 204 and backend.write_count == w0 + 1
        for junk in ("/.DS_Store", "/user/._channels.ch", "/user/Thumbs.db"):
            assert req("PUT", junk, body=b"junk")[0] == 204
        assert backend.write_count == w0 + 1
        assert req("PUT", "/user/this_filename_is_way_too_long_for_kernfs.txt", body=b"hi")[0] == 422

        # 6. Class-2 LOCK/UNLOCK + atomic COPY/MOVE + read-only enforcement
        st, _, hdrs = req("LOCK", "/user/new.ch")
        assert st == 200 and "lock-token" in hdrs
        assert req("UNLOCK", "/user/new.ch", headers={"Lock-Token": hdrs["lock-token"]})[0] == 204
        assert req("COPY", "/user/new.ch", headers={"Destination": f"http://127.0.0.1:{port}/user/copy.ch"})[0] == 201
        assert req("MOVE", "/user/copy.ch", headers={"Destination": f"http://127.0.0.1:{port}/user/moved.ch"})[0] == 201
        assert "/user/copy.ch" not in backend.files and backend.files["/user/moved.ch"] == valid_chan

        policy.read_only = True
        assert req("PUT", "/user/ro.txt", body=b"no")[0] == 403
        assert req("DELETE", "/user/moved.ch")[0] == 403

        log_ok("All KernFS WebDAV mounter selftests passed (volume identity, DNS-rebind/CSRF, RFC 4331 quota, ETag 304/412, Class-2 DAV, anti-brick shields).")
        return 0
    finally:
        srv.shutdown()
        srv.server_close()


def main() -> int:
    ap = argparse.ArgumentParser(description="KernFS Universal Multi-Platform Mounter (Hardened WebDAV over RigWire)")
    ap.add_argument("-p", "--port", help="Serial port (/dev/ttyACM0, COM3, /dev/cu.usbmodem*) or tcp://127.0.0.1:4446")
    ap.add_argument("-b", "--baud", type=int, default=115200, help="Serial baud rate (default: 115200)")
    ap.add_argument("--http-port", type=int, default=8765, help="Localhost WebDAV TCP port (default: 8765)")
    ap.add_argument("--name", help="Override mounted volume display name (default: auto 'Quansheng UV-K1 XXXX')")
    ap.add_argument("--mountpoint", "-m", help="Optional mountpoint path/drive letter")
    ap.add_argument("--auto-mount", action="store_true", help="Auto-mount natively and unmount cleanly on exit")
    ap.add_argument("--open", dest="open_fm", action="store_true", help="Open mounted volume in native file manager")
    ap.add_argument("--read-only", action="store_true", help="Mount strictly read-only (0 writes/deletes possible)")
    ap.add_argument("--cache-ttl", type=float, default=3.0, help="Metadata cache TTL in seconds (default: 3.0)")
    ap.add_argument("--no-format-checks", action="store_true", help="Disable pre-write KRX & ORDS-01 validation")
    ap.add_argument("--allow-system-mod-delete", action="store_true", help="Allow deleting /apps/{dfu.mod,panic.mod}")
    ap.add_argument("--unsafe-allow-sys-write", action="store_true", help="UNSAFE: allow writes/deletes under /sys/*")
    ap.add_argument("-v", "--verbose", action="store_true", help="Log every WebDAV request")
    ap.add_argument("--selftest", action="store_true", help="Run built-in selftest (no radio required)")
    args = ap.parse_args()

    if args.selftest:
        return run_selftest()

    print_header()
    port_name = args.port
    if not port_name:
        try:
            from serial.tools import list_ports
            cands = [p.device for p in list_ports.comports() if "ACM" in p.device or "USB" in p.device or "usbmodem" in p.device]
            if len(cands) == 1:
                port_name = cands[0]
        except Exception:
            pass
    if not port_name:
        log_err("Specify radio port with -p /dev/ttyACM0 (or tcp://127.0.0.1:4446 for QEMU)")
        return 2

    log_info(f"Connecting via RigWire on {BOLD}{port_name}{RESET}...")
    client = rigwire.RigWireClient(port_name, baudrate=args.baud, timeout=3.0)
    try:
        hello = client.hello()
        telemetry = client.get_system_telemetry()
    except Exception as e:
        client.close()
        log_err(f"RigWire handshake failed on {port_name}: {e}")
        return 1

    volume_name = resolve_volume_name(port_name, hello=hello, telemetry=telemetry, override=args.name)
    backend = RigWireFsBackend(client, cache_ttl=args.cache_ttl)
    policy = SafetyPolicy(
        read_only=args.read_only,
        allow_sys_write=args.unsafe_allow_sys_write,
        allow_system_mod_delete=args.allow_system_mod_delete,
        validate_formats=not args.no_format_checks,
    )
    srv = KernWebDAVServer(("127.0.0.1", args.http_port), backend, policy, volume_name=volume_name, verbose=args.verbose)
    url = f"http://127.0.0.1:{args.http_port}/"
    print_mount_instructions(url, volume_name, args.mountpoint)

    unmount_cmd: list[str] | None = None
    if args.auto_mount:
        open_target, unmount_cmd = try_auto_mount(url, volume_name, args.mountpoint)
        if args.open_fm and open_target:
            open_in_file_manager(open_target)

    try:
        srv.serve_forever()
    except KeyboardInterrupt:
        print()
        log_info("Shutting down KernFS WebDAV bridge...")
    finally:
        if unmount_cmd:
            subprocess.run(unmount_cmd, capture_output=True)
        srv.server_close()
        backend.close()
        log_ok("Disconnected cleanly.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
