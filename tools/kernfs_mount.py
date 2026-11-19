#!/usr/bin/env python3
"""
KernFS Universal Multi-Platform Mounter (WebDAV Class 2 over RigWire).

Mounts the radio's LittleFS / KernFS natively on Linux, macOS, and Windows via
a localhost-only WebDAV Class 2 bridge (127.0.0.1) — zero kernel drivers, zero
third-party dependencies beyond Python stdlib + pyserial (via rigwire.py).

Anti-Brick & Anti-Corruption Safeguards:
  1. Single serialized RigWire transaction lock (no interleaved serial frames).
  2. OS metadata junk blackhole (.DS_Store, ._*, Thumbs.db, .Spotlight-V100,
     System Volume Information, .Trash-*, etc.) — zero junk written to flash.
  3. Protected paths: /sys/* is strictly read-only by default; /apps/dfu.mod
     and /apps/panic.mod cannot be accidentally deleted.
  4. Pre-write validation BEFORE touching flash:
     - KernFS path length (<= 32 UTF-8 bytes) & traversal checks
     - Upload ceiling (<= MAX_WIRE_FILE_SIZE)
     - KRX header & CRC-32 validation for .krx/.mod/.app/.svc/.dmn/.drv
     - ORDS-01 FourCC & power-of-two record stride checks for .ch/.ct/.bnd/.zn/.hit/.fm
  5. Whole-file buffered writes, zero-wear idempotent skip when size+CRC match,
     automatic TRANSFER_ABORT on error, and post-write stat CRC-32 verification.
  6. Safe MOVE/COPY (write destination -> verify CRC -> delete source only on success).
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

# Ensure sibling `tools/rigwire.py` is importable
TOOLS_DIR = Path(__file__).resolve().parent
if str(TOOLS_DIR) not in sys.path:
    sys.path.insert(0, str(TOOLS_DIR))

import rigwire  # noqa: E402

# =============================================================================
# 1. SAFETY POLICY, OS JUNK FILTER & FORMAT VALIDATORS
# =============================================================================

MAX_KERNFS_PATH = 32
PKG_EXTS = (".krx", ".app", ".svc", ".dmn", ".drv", ".mod")
PROTECTED_RECOVERY_MODS = frozenset({"/apps/dfu.mod", "/apps/panic.mod"})

# ORDS-01 native table rules: extension -> (FourCC bytes, record stride bytes)
ORDS01_TABLES: dict[str, tuple[bytes, int]] = {
    ".ch": (b"CHAN", 64),
    ".ct": (b"CONT", 32),
    ".bnd": (b"BNDP", 32),
    ".zn": (b"ZONE", 32),
    ".hit": (b"HITS", 32),
    ".fm": (b"FMBC", 16),
}

# OS metadata junk basenames / prefixes that must NEVER hit SPI flash
OS_JUNK_NAMES = frozenset({
    ".ds_store",
    ".localized",
    ".spotlight-v100",
    ".trashes",
    ".fseventsd",
    ".temporaryitems",
    ".documentrevisions-v100",
    ".apdisk",
    ".metadata_never_index",
    ".directory",
    ".xdg-volume-info",
    "desktop.ini",
    "thumbs.db",
    "ehthumbs.db",
    "$recycle.bin",
    "system volume information",
    "autorun.inf",
})


def is_os_junk_path(path: str) -> bool:
    """Returns True for macOS/Windows/Linux desktop clutter paths."""
    parts = [p for p in path.split("/") if p]
    for seg in parts:
        low = seg.lower()
        if low in OS_JUNK_NAMES:
            return True
        if seg.startswith("._"):  # macOS AppleDouble resource forks
            return True
        if low.startswith(".trash-") or low.startswith(".gvfs"):
            return True
        if seg.endswith("~") or (seg.startswith(".") and seg.endswith(".swp")):
            return True
    return False


def normalize_path(raw_url_path: str) -> str:
    unquoted = urllib.parse.unquote(raw_url_path)
    # Strip query string if present
    if "?" in unquoted:
        unquoted = unquoted.split("?", 1)[0]
    if "\\" in unquoted:
        raise ValueError("Backslashes are not allowed in KernFS paths")
    raw_segs = [s for s in unquoted.split("/") if s]
    if any(s == ".." for s in raw_segs):
        raise ValueError("Path traversal ('..') is forbidden")
    norm = posixpath.normpath("/" + "/".join(raw_segs))
    if not norm.startswith("/"):
        norm = "/" + norm
    return norm


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
            raise ValueError(f"Path '{path}' contains non-ASCII control characters")

    def check_write(self, path: str, data: bytes) -> None:
        if self.read_only:
            raise PermissionError("Mount is in --read-only mode")
        self.check_path_valid(path)
        if path == "/":
            raise PermissionError("Cannot overwrite root directory")
        if (path == "/sys" or path.startswith("/sys/")) and not self.allow_sys_write:
            raise PermissionError(f"Protected path '{path}' is read-only (pass --unsafe-allow-sys-write to override)")
        if path == "/sys/calib.dat" and len(data) != 512:
            raise ValueError(f"/sys/calib.dat must be exactly 512 bytes (got {len(data)} B)")
        if path == "/sys/calib.sig" and len(data) != 32:
            raise ValueError(f"/sys/calib.sig must be exactly 32 bytes (got {len(data)} B)")
        if len(data) > rigwire.MAX_WIRE_FILE_SIZE:
            raise ValueError(f"File ({len(data)} B) exceeds RigWire {rigwire.MAX_WIRE_FILE_SIZE}-byte ceiling")

        if not self.validate_formats:
            return

        low = path.lower()
        # Validate KRX executables/modules before touching flash
        if low.endswith(PKG_EXTS):
            try:
                rigwire.validate_krx_package(data)
            except Exception as e:
                raise ValueError(f"Rejected invalid KRX package '{path}': {e}") from e

        # Validate ORDS-01 fixed-stride table files
        for ext, (fourcc, stride) in ORDS01_TABLES.items():
            if low.endswith(ext):
                if len(data) < 16:
                    raise ValueError(f"ORDS-01 table '{path}' too short ({len(data)} B < 16 B header)")
                if data[:4] != fourcc:
                    raise ValueError(
                        f"ORDS-01 table '{path}' header FourCC {data[:4]!r} != expected {fourcc!r}"
                    )
                if (len(data) - 16) % stride != 0:
                    raise ValueError(
                        f"ORDS-01 table '{path}' payload ({len(data) - 16} B) not aligned to {stride}-byte stride"
                    )
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
            raise PermissionError(
                f"Protected recovery module '{path}' cannot be deleted (pass --allow-system-mod-delete to override)"
            )


# =============================================================================
# 2. BACKENDS (RigWire Hardware Backend + In-Memory Test Backend)
# =============================================================================

class FsBackend:
    """Thread-safe abstract KernFS backend with short-TTL read/metadata cache."""

    def stat(self, path: str) -> dict[str, Any]:
        raise NotImplementedError

    def ls(self, path: str) -> list[dict[str, Any]]:
        raise NotImplementedError

    def read_bytes(self, path: str) -> bytes:
        raise NotImplementedError

    def write_bytes(self, path: str, data: bytes) -> dict[str, Any]:
        raise NotImplementedError

    def remove(self, path: str) -> None:
        raise NotImplementedError

    def mkdir(self, path: str) -> None:
        raise NotImplementedError

    def close(self) -> None:
        pass


class RigWireFsBackend(FsBackend):
    """Serialized, CRC-verified RigWire backend with coherent short-TTL caching."""

    def __init__(self, client: rigwire.RigWireClient, cache_ttl: float = 3.0) -> None:
        self._client = client
        self._lock = threading.RLock()
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
        stale_keys = [k for k in self._data_cache if k[0] == path]
        for k in stale_keys:
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

    def read_bytes(self, path: str) -> bytes:
        with self._lock:
            st = self.stat(path)
            if not st["exists"] or st["is_dir"]:
                raise FileNotFoundError(path)
            ckey = (path, st["size"], st["crc32"])
            if ckey in self._data_cache:
                return self._data_cache[ckey]

            # Quiet chunked download via RigWire OP_TRANSFER_OPEN/CHUNK/CLOSE
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
                    req_chunk = struct.pack("<HIH", tid, offset, req_len)
                    resp_chunk = self._client.send_record(rigwire.SERVICE_DATA, rigwire.OP_TRANSFER_CHUNK, req_chunk)
                    if len(resp_chunk) < 8:
                        raise RuntimeError("Short TRANSFER_CHUNK response")
                    _tid, _off, rx_len = struct.unpack("<HIH", resp_chunk[:8])
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
            # Keep bounded cache (max 64 files)
            if len(self._data_cache) > 64:
                self._data_cache.clear()
            self._data_cache[(path, len(data), rigwire.crc32_ieee(data))] = data
            return data

    def write_bytes(self, path: str, data: bytes) -> dict[str, Any]:
        with self._lock:
            file_crc = rigwire.crc32_ieee(data)
            # Zero-wear idempotency: skip flash write if identical size & CRC-32 already on device!
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
                    req_chunk = struct.pack("<HIH", tid, offset, len(chunk)) + chunk
                    self._client.send_record(rigwire.SERVICE_DATA, rigwire.OP_TRANSFER_CHUNK, req_chunk)
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
            # Post-write stat verification
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
    """In-memory KernFS simulator for --selftest (no physical radio required)."""

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
            parent = posixpath.dirname(path) or "/"
            self.dirs.add(parent)
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
# 3. WEBDAV CLASS 2 HTTP SERVER (Linux / macOS Finder / Windows Explorer)
# =============================================================================

class KernWebDAVServer(socketserver.ThreadingMixIn, http.server.HTTPServer):
    daemon_threads = True
    allow_reuse_address = True

    def __init__(
        self,
        server_address: tuple[str, int],
        backend: FsBackend,
        policy: SafetyPolicy,
        verbose: bool = False,
    ) -> None:
        super().__init__(server_address, KernWebDAVHandler)
        self.backend = backend
        self.policy = policy
        self.verbose = verbose
        self.lock_table: dict[str, str] = {}  # token -> path
        self.lock_mutex = threading.Lock()
        self.boot_http_date = email.utils.formatdate(time.time(), usegmt=True)


class KernWebDAVHandler(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    server: KernWebDAVServer  # type: ignore[assignment]

    def log_message(self, fmt: str, *args: Any) -> None:
        if self.server.verbose:
            sys.stderr.write(f"[kernfs] {self.command} {self.path} — {fmt % args}\n")

    def _send_empty(self, code: int, extra_headers: dict[str, str] | None = None) -> None:
        self.send_response(code)
        self.send_header("Content-Length", "0")
        if extra_headers:
            for k, v in extra_headers.items():
                self.send_header(k, v)
        self.end_headers()

    def _send_bytes(self, code: int, body: bytes, content_type: str, extra_headers: dict[str, str] | None = None) -> None:
        self.send_response(code)
        self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(len(body)))
        if extra_headers:
            for k, v in extra_headers.items():
                self.send_header(k, v)
        self.end_headers()
        if self.command != "HEAD":
            self.wfile.write(body)

    def _send_text_err(self, code: int, msg: str) -> None:
        self._send_bytes(code, (msg + "\n").encode("utf-8"), "text/plain; charset=utf-8")

    def _read_body(self) -> bytes:
        cl = self.headers.get("Content-Length")
        if cl is not None:
            n = int(cl)
            if n > rigwire.MAX_WIRE_FILE_SIZE + 4096:
                raise ValueError(f"Request body {n} B exceeds transfer ceiling")
            return self.rfile.read(n) if n > 0 else b""
        if self.headers.get("Transfer-Encoding", "").lower() == "chunked":
            buf = bytearray()
            while True:
                line = self.rfile.readline().strip()
                if not line:
                    break
                sz = int(line.split(b";", 1)[0], 16)
                if sz == 0:
                    self.rfile.readline()
                    break
                if len(buf) + sz > rigwire.MAX_WIRE_FILE_SIZE + 4096:
                    raise ValueError("Chunked body exceeds transfer ceiling")
                buf.extend(self.rfile.read(sz))
                self.rfile.readline()
            return bytes(buf)
        return b""

    def _safe_path(self, raw: str | None = None) -> str:
        return normalize_path(self.path if raw is None else raw)

    def _etag(self, st: dict[str, Any]) -> str:
        return f'"{st["size"]:x}-{st["crc32"]:08x}"'

    # --- WebDAV Class 2 Methods ---

    def do_OPTIONS(self) -> None:
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
        _ = self._read_body()
        try:
            path = self._safe_path()
        except ValueError as e:
            self._send_text_err(400, str(e))
            return

        if is_os_junk_path(path):
            self._send_text_err(404, "Not found")
            return

        depth = self.headers.get("Depth", "1").strip().lower()
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
                    "path": cpath,
                    "exists": True,
                    "is_dir": bool(entry["is_dir"]),
                    "size": int(entry.get("size", 0)),
                    "crc32": 0,
                }))

        xml_body = self._build_multistatus(items)
        self._send_bytes(207, xml_body, "application/xml; charset=utf-8")

    def _build_multistatus(self, items: list[tuple[str, dict[str, Any]]]) -> bytes:
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
            ET.SubElement(prop, f"{{{ns}}}displayname").text = posixpath.basename(ipath.rstrip("/")) or "KernFS"
            rt = ET.SubElement(prop, f"{{{ns}}}resourcetype")
            if st["is_dir"]:
                ET.SubElement(rt, f"{{{ns}}}collection")
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
        _ = self._read_body()
        if self.server.policy.read_only:
            self._send_text_err(403, "Read-only mount")
            return
        # Cleanly acknowledge Finder/Explorer metadata probes without touching flash
        path = self._safe_path()
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
            # Simple read-only HTML index for browser inspection
            entries = self.server.backend.ls(path)
            lines = [f"<html><body><h3>KernFS: {path}</h3><ul>"]
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

        ctype = mimetypes.guess_type(path)[0] or "application/octet-stream"
        headers = {"ETag": self._etag(st), "Accept-Ranges": "bytes", "Last-Modified": self.server.boot_http_date}
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

        # Support HTTP Range requests (macOS Finder / streaming readers)
        rng = self.headers.get("Range")
        if rng and rng.startswith("bytes="):
            spec = rng[6:].split(",", 1)[0].strip()
            s_str, _, e_str = spec.partition("-")
            start = int(s_str) if s_str else 0
            end = int(e_str) if e_str else len(data) - 1
            end = min(end, len(data) - 1)
            if 0 <= start <= end < len(data):
                headers["Content-Range"] = f"bytes {start}-{end}/{len(data)}"
                self._send_bytes(206, data[start : end + 1], ctype, headers)
                return

        self._send_bytes(200, data, ctype, headers)

    def do_PUT(self) -> None:
        try:
            path = self._safe_path()
            data = self._read_body()
        except ValueError as e:
            self._send_text_err(400, str(e))
            return

        # Blackhole desktop OS metadata clutter — never touch radio flash!
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
            existed = self.server.backend.stat(path)["exists"]
            res = self.server.backend.write_bytes(path, data)
        except Exception as e:
            self._send_text_err(502, f"RigWire write failed: {e}")
            return

        etag = f'"{res["size"]:x}-{res["crc32"]:08x}"'
        self._send_empty(204 if existed else 201, {"ETag": etag})

    def do_DELETE(self) -> None:
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
            st = self.server.backend.stat(path)
            if not st["exists"]:
                self._send_text_err(404, "Not found")
                return
            self.server.backend.remove(path)
        except Exception as e:
            self._send_text_err(502, f"RigWire delete failed: {e}")
            return

        self._send_empty(204)

    def do_MKCOL(self) -> None:
        _ = self._read_body()
        try:
            path = self._safe_path()
        except ValueError as e:
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
            # Atomic safety: write + CRC-verify destination FIRST; delete source ONLY after verified write!
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

    def do_COPY(self) -> None:
        self._handle_copy_or_move(is_move=False)

    def do_MOVE(self) -> None:
        self._handle_copy_or_move(is_move=True)

    def do_LOCK(self) -> None:
        _ = self._read_body()
        if self.server.policy.read_only:
            self._send_text_err(403, "Read-only mount")
            return
        path = self._safe_path()
        token = f"urn:uuid:{uuid.uuid4()}"
        with self.server.lock_mutex:
            self.server.lock_table[token] = path
        xml = (
            '<?xml version="1.0" encoding="utf-8"?>'
            '<D:prop xmlns:D="DAV:"><D:lockdiscovery><D:activelock>'
            "<D:locktype><D:write/></D:locktype>"
            "<D:lockscope><D:exclusive/></D:lockscope>"
            "<D:depth>0</D:depth>"
            "<D:timeout>Second-600</D:timeout>"
            f"<D:locktoken><D:href>{token}</D:href></D:locktoken>"
            "</D:activelock></D:lockdiscovery></D:prop>"
        ).encode("utf-8")
        self._send_bytes(200, xml, "application/xml; charset=utf-8", {"Lock-Token": f"<{token}>"})

    def do_UNLOCK(self) -> None:
        tok = self.headers.get("Lock-Token", "").strip("<> ")
        with self.server.lock_mutex:
            self.server.lock_table.pop(tok, None)
        self._send_empty(204)


# =============================================================================
# 4. OS MOUNT HELPERS & CLI
# =============================================================================

def print_mount_instructions(url: str, mountpoint: str | None) -> None:
    sys_name = platform.system()
    print(f"\n  KernFS WebDAV bridge listening at: {url}")
    print("  ------------------------------------------------------------------")
    if sys_name == "Linux":
        print(f"  Linux (GNOME/GIO):    gio mount {url}")
        print(f"  Linux (File Manager): Open dav://{url.removeprefix('http://')} in Nautilus / Dolphin")
        if mountpoint:
            print(f"  Linux (davfs2):       sudo mount -t davfs {url} {mountpoint}")
    elif sys_name == "Darwin":
        mp = mountpoint or "/Volumes/KernFS"
        print(f"  macOS (CLI):          mkdir -p {mp} && mount_webdav {url} {mp}")
        print(f"  macOS (Finder):       Cmd+K -> {url}")
    elif sys_name == "Windows":
        drive = mountpoint or "K:"
        print(f"  Windows (CLI):        net use {drive} {url}")
        print(f"  Windows (Explorer):   Map Network Drive -> {url}")
    else:
        print(f"  Mount URL:            {url}")
    print("  Press Ctrl+C to cleanly unmount and disconnect.\n")


def try_auto_mount(url: str, mountpoint: str | None) -> tuple[bool, list[str] | None]:
    """Attempts native OS mount and returns (ok, unmount_cmd)."""
    sys_name = platform.system()
    if sys_name == "Linux" and shutil.which("gio"):
        res = subprocess.run(["gio", "mount", url], capture_output=True, text=True)
        if res.returncode == 0:
            print(f"  Auto-mounted via GIO ({url})")
            return True, ["gio", "mount", "-u", url]
        print(f"  WARN: gio mount failed: {res.stderr.strip()}")
    elif sys_name == "Darwin":
        mp = mountpoint or "/Volumes/KernFS"
        Path(mp).mkdir(parents=True, exist_ok=True)
        res = subprocess.run(["mount_webdav", url, mp], capture_output=True, text=True)
        if res.returncode == 0:
            print(f"  Auto-mounted at {mp}")
            return True, ["umount", mp]
        print(f"  WARN: mount_webdav failed: {res.stderr.strip()}")
    elif sys_name == "Windows":
        drive = mountpoint or "K:"
        res = subprocess.run(["net", "use", drive, url], capture_output=True, text=True)
        if res.returncode == 0:
            print(f"  Auto-mounted drive {drive}")
            return True, ["net", "use", drive, "/delete", "/y"]
        print(f"  WARN: net use failed: {res.stderr.strip()}")
    return False, None


# =============================================================================
# 5. BUILT-IN END-TO-END WEBDAV & ANTI-BRICK SELFTEST
# =============================================================================

def run_selftest() -> int:
    backend = MemoryFsBackend()
    # Seed realistic KernFS files
    backend.files["/sys/calib.dat"] = b"\xAA" * 512
    backend.files["/sys/calib.sig"] = b"\xBB" * 32
    backend.files["/apps/dfu.mod"] = b"\xC0\xDE\x01\xFE" + b"\x00" * 60
    valid_chan = b"CHAN" + b"\x00" * 12 + b"\x11" * 64
    backend.files["/user/channels.ch"] = valid_chan

    policy = SafetyPolicy(read_only=False, allow_sys_write=False, allow_system_mod_delete=False, validate_formats=True)
    srv = KernWebDAVServer(("127.0.0.1", 0), backend, policy, verbose=False)
    port = srv.server_address[1]
    t = threading.Thread(target=srv.serve_forever, daemon=True)
    t.start()

    def req(method: str, path: str, body: bytes = b"", headers: dict[str, str] | None = None) -> tuple[int, bytes, dict[str, str]]:
        conn = http.client.HTTPConnection("127.0.0.1", port, timeout=5)
        try:
            conn.request(method, path, body=body, headers=headers or {})
            r = conn.getresponse()
            data = r.read()
            return r.status, data, {k.lower(): v for k, v in r.getheaders()}
        finally:
            conn.close()

    try:
        # 1. OPTIONS advertises WebDAV Class 2
        st, _, hdrs = req("OPTIONS", "/")
        assert st == 200 and "2" in hdrs.get("dav", ""), f"OPTIONS failed: {st} {hdrs}"

        # 2. PROPFIND root & /sys
        st, body, _ = req("PROPFIND", "/", headers={"Depth": "1"})
        assert st == 207 and b"/apps/" in body and b"/sys/" in body

        # 3. GET + Range request on /sys/calib.dat
        st, body, _ = req("GET", "/sys/calib.dat")
        assert st == 200 and len(body) == 512
        st, body, hdrs = req("GET", "/sys/calib.dat", headers={"Range": "bytes=0-15"})
        assert st == 206 and len(body) == 16 and hdrs.get("content-range") == "bytes 0-15/512"

        # 4. Anti-brick: PUT & DELETE on /sys/calib.dat must be blocked (403)
        st, _, _ = req("PUT", "/sys/calib.dat", body=b"\x00" * 512)
        assert st == 403, f"Expected 403 on /sys/calib.dat PUT, got {st}"
        st, _, _ = req("DELETE", "/sys/calib.dat")
        assert st == 403, f"Expected 403 on /sys/calib.dat DELETE, got {st}"
        assert backend.files["/sys/calib.dat"] == b"\xAA" * 512

        # 5. Anti-brick: DELETE on /apps/dfu.mod must be blocked (403)
        st, _, _ = req("DELETE", "/apps/dfu.mod")
        assert st == 403, f"Expected 403 on /apps/dfu.mod DELETE, got {st}"

        # 6. Anti-brick: corrupted KRX package in /apps/ must be rejected (422)
        st, _, _ = req("PUT", "/apps/broken.krx", body=b"NOT_A_VALID_KRX")
        assert st == 422, f"Expected 422 on invalid KRX, got {st}"
        assert "/apps/broken.krx" not in backend.files

        # 7. Anti-brick: misaligned ORDS-01 table must be rejected (422)
        st, _, _ = req("PUT", "/user/bad.ch", body=b"CHAN" + b"\x00" * 12 + b"\x22" * 17)
        assert st == 422, f"Expected 422 on misaligned ORDS-01 .ch, got {st}"
        st, _, _ = req("PUT", "/user/badhdr.ch", body=b"NOPE" + b"\x00" * 12 + b"\x22" * 64)
        assert st == 422, f"Expected 422 on bad FourCC .ch, got {st}"

        # 8. Valid ORDS-01 .ch write + zero-wear idempotent re-write
        w_before = backend.write_count
        st, _, _ = req("PUT", "/user/new.ch", body=valid_chan)
        assert st == 201 and backend.write_count == w_before + 1
        st, _, _ = req("PUT", "/user/new.ch", body=valid_chan)
        assert st == 204 and backend.write_count == w_before + 1, "Idempotent PUT caused unnecessary flash write!"

        # 9. OS junk files (.DS_Store, ._foo, Thumbs.db) must be blackholed with 0 flash writes
        w_before = backend.write_count
        for junk in ("/.DS_Store", "/user/._channels.ch", "/user/Thumbs.db", "/.Spotlight-V100/Store"):
            st, _, _ = req("PUT", junk, body=b"junk")
            assert st == 204, f"Expected 204 junk sink for {junk}, got {st}"
        assert backend.write_count == w_before, "OS junk leaked into flash backend!"

        # 10. Path > 32 bytes rejected (422)
        st, _, _ = req("PUT", "/user/this_filename_is_way_too_long_for_kernfs.txt", body=b"hi")
        assert st == 422, f"Expected 422 for >32 B path, got {st}"

        # 11. LOCK / UNLOCK (macOS Finder / Windows Explorer Class-2 RW requirement)
        st, _, hdrs = req("LOCK", "/user/new.ch")
        assert st == 200 and "lock-token" in hdrs
        st, _, _ = req("UNLOCK", "/user/new.ch", headers={"Lock-Token": hdrs["lock-token"]})
        assert st == 204

        # 12. Atomic MOVE & COPY
        st, _, _ = req("COPY", "/user/new.ch", headers={"Destination": f"http://127.0.0.1:{port}/user/copy.ch"})
        assert st == 201 and backend.files["/user/copy.ch"] == valid_chan
        st, _, _ = req("MOVE", "/user/copy.ch", headers={"Destination": f"http://127.0.0.1:{port}/user/moved.ch"})
        assert st == 201 and "/user/copy.ch" not in backend.files and backend.files["/user/moved.ch"] == valid_chan

        # 13. Read-only mode enforcement
        policy.read_only = True
        st, _, _ = req("PUT", "/user/ro.txt", body=b"nope")
        assert st == 403
        st, _, _ = req("DELETE", "/user/moved.ch")
        assert st == 403

        print("OK: All KernFS WebDAV mounter selftests passed (Class-2 DAV, anti-brick shields, KRX/ORDS-01 validation, junk filter, idempotency, atomic MOVE/COPY, read-only).")
        return 0
    finally:
        srv.shutdown()
        srv.server_close()


def main() -> int:
    ap = argparse.ArgumentParser(
        description="Universal multi-platform KernFS mounter over RigWire (localhost WebDAV Class 2)"
    )
    ap.add_argument("-p", "--port", help="Serial port (/dev/ttyACM0, COM3, /dev/cu.usbmodem*) or tcp://127.0.0.1:4446")
    ap.add_argument("-b", "--baud", type=int, default=115200, help="Serial baud rate (default: 115200)")
    ap.add_argument("--http-port", type=int, default=8765, help="Localhost WebDAV TCP port (default: 8765)")
    ap.add_argument("--mountpoint", "-m", help="Optional mountpoint path/drive letter (e.g. /Volumes/KernFS or K:)")
    ap.add_argument("--auto-mount", action="store_true", help="Automatically mount via OS native WebDAV client and unmount on exit")
    ap.add_argument("--read-only", action="store_true", help="Mount strictly read-only (0 writes/deletes possible)")
    ap.add_argument("--cache-ttl", type=float, default=3.0, help="Metadata cache TTL in seconds (default: 3.0)")
    ap.add_argument("--no-format-checks", action="store_true", help="Disable pre-write KRX & ORDS-01 format validation")
    ap.add_argument("--allow-system-mod-delete", action="store_true", help="Allow deleting /apps/dfu.mod or /apps/panic.mod")
    ap.add_argument("--unsafe-allow-sys-write", action="store_true", help="UNSAFE: allow writing/deleting under /sys/*")
    ap.add_argument("-v", "--verbose", action="store_true", help="Log every WebDAV request")
    ap.add_argument("--selftest", action="store_true", help="Run built-in WebDAV & anti-brick selftest (no radio required)")
    args = ap.parse_args()

    if args.selftest:
        return run_selftest()

    port_name = args.port
    if not port_name:
        # Auto-detect via pyserial if available
        try:
            from serial.tools import list_ports
            cands = [p.device for p in list_ports.comports() if "ACM" in p.device or "USB" in p.device or "usbmodem" in p.device]
            if len(cands) == 1:
                port_name = cands[0]
        except Exception:
            pass
    if not port_name:
        print("ERROR: Specify radio serial port with -p /dev/ttyACM0 (or tcp://127.0.0.1:4446 for QEMU)", file=sys.stderr)
        return 2

    print(f"  Connecting to Kern radio via RigWire on {port_name}...")
    client = rigwire.RigWireClient(port_name, baudrate=args.baud, timeout=3.0)
    try:
        hello = client.hello()
        print(f"  Connected: {hello}")
    except Exception as e:
        client.close()
        print(f"ERROR: RigWire HELLO failed on {port_name}: {e}", file=sys.stderr)
        return 1

    backend = RigWireFsBackend(client, cache_ttl=args.cache_ttl)
    policy = SafetyPolicy(
        read_only=args.read_only,
        allow_sys_write=args.unsafe_allow_sys_write,
        allow_system_mod_delete=args.allow_system_mod_delete,
        validate_formats=not args.no_format_checks,
    )
    srv = KernWebDAVServer(("127.0.0.1", args.http_port), backend, policy, verbose=args.verbose)
    url = f"http://127.0.0.1:{args.http_port}/"
    print_mount_instructions(url, args.mountpoint)

    unmount_cmd: list[str] | None = None
    if args.auto_mount:
        _, unmount_cmd = try_auto_mount(url, args.mountpoint)

    try:
        srv.serve_forever()
    except KeyboardInterrupt:
        print("\n  Shutting down KernFS bridge...")
    finally:
        if unmount_cmd:
            subprocess.run(unmount_cmd, capture_output=True)
        srv.server_close()
        backend.close()
        print("  Disconnected cleanly.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
