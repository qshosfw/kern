"""Hardened security controller: policy profiles, TX latch, path/size guards, audit trail."""

from __future__ import annotations

import collections
import dataclasses
import enum
import posixpath
import threading
import time
from typing import Any

import kernfs_mount

from .events import EventBus


class SecurityError(PermissionError):
    """Raised whenever an operation violates Kern Dock's active security posture."""


class SecurityProfile(enum.Enum):
    STANDARD = "standard"
    SAFE = "safe"
    HARDENED = "hardened"


@dataclasses.dataclass(frozen=True)
class AuditEntry:
    ts_wall: str
    category: str       # "auth" | "fs" | "rf" | "firmware" | "ipc" | "plugin" | "system"
    action: str
    outcome: str        # "allow" | "deny" | "info"
    detail: str


MAX_UPLOAD_BYTES = 256 * 1024
MAX_FIRMWARE_BYTES = 128 * 1024
MAX_AUDIT_ENTRIES = 250


def sanitize_kernfs_path(path: str) -> str:
    """
    Strictly normalize and validate a KernFS path:
      • Must be non-empty, ASCII-clean, no NUL bytes or control chars
      • Canonicalized with POSIX rules; no '..' traversal escapes
      • Always begins with '/'
    """
    if not path or "\x00" in path:
        raise SecurityError("Invalid KernFS path (empty or NUL byte)")
    if any(ord(c) < 32 for c in path):
        raise SecurityError("Control characters are forbidden in KernFS paths")
    raw = path.replace("\\", "/")
    if not raw.startswith("/"):
        raw = "/" + raw
    parts = [p for p in raw.split("/") if p]
    if any(p in (".", "..") for p in parts):
        raise SecurityError(f"Path traversal rejected: {path!r}")
    norm = posixpath.normpath("/" + "/".join(parts))
    if not norm.startswith("/"):
        raise SecurityError(f"Malformed KernFS path: {path!r}")
    return norm


class SecurityController:
    """
    Single source of truth for Kern Dock hardening:
      • Wraps kernfs_mount.SafetyPolicy and synchronizes Hardened Mode atomically
      • Enforces TX-over-USB dead-man arming (auto-disarms on connect/disconnect/timeout/hardened)
      • Gates bootloader entry, firmware flashing, KRX sideloading, restores, IPC control and plugin caps
      • Records an in-memory audit ring buffer of every privileged or blocked event
    """

    def __init__(self, events: EventBus, *, hardened: bool = False, tx_relock_seconds: int = 120) -> None:
        self._lock = threading.RLock()
        self.events = events
        self.policy = kernfs_mount.SafetyPolicy(read_only=False)
        self._hardened = False
        self._saved_policy: tuple[bool, bool, bool, bool] | None = None
        self._allow_usb_tx = False
        self._tx_armed_at = 0.0
        self.tx_relock_seconds = max(0, int(tx_relock_seconds))
        self._ipc_allow_control = True
        self._audit: collections.deque[AuditEntry] = collections.deque(maxlen=MAX_AUDIT_ENTRIES)
        if hardened:
            self.set_hardened(True, reason="startup")

    @property
    def hardened(self) -> bool:
        with self._lock:
            return self._hardened

    @property
    def profile(self) -> SecurityProfile:
        with self._lock:
            if self._hardened:
                return SecurityProfile.HARDENED
            if self.policy.read_only:
                return SecurityProfile.SAFE
            return SecurityProfile.STANDARD

    @property
    def allow_usb_tx(self) -> bool:
        with self._lock:
            if self._hardened or not self._allow_usb_tx:
                return False
            if self.tx_relock_seconds > 0 and (time.monotonic() - self._tx_armed_at) > self.tx_relock_seconds:
                self._allow_usb_tx = False
                self._record("rf", "tx_auto_relock", "info", f"Auto-disarmed after {self.tx_relock_seconds}s")
                self.events.emit("security.changed", self.snapshot(), source="security")
                return False
            return True

    @property
    def ipc_allow_control(self) -> bool:
        with self._lock:
            return self._ipc_allow_control and not self._hardened

    def set_ipc_allow_control(self, allowed: bool) -> None:
        with self._lock:
            self._ipc_allow_control = bool(allowed)
            self._record("ipc", "set_control_policy", "info", f"control={'enabled' if allowed else 'read-only'}")
        self.events.emit("security.changed", self.snapshot(), source="security")

    def set_hardened(self, enabled: bool, *, reason: str = "user") -> None:
        with self._lock:
            enabled = bool(enabled)
            if enabled == self._hardened:
                return
            p = self.policy
            if enabled:
                self._saved_policy = (
                    p.read_only,
                    p.validate_formats,
                    p.allow_sys_write,
                    p.allow_system_mod_delete,
                )
                p.read_only = True
                p.validate_formats = True
                p.allow_sys_write = False
                p.allow_system_mod_delete = False
                self._allow_usb_tx = False
            elif self._saved_policy is not None:
                (
                    p.read_only,
                    p.validate_formats,
                    p.allow_sys_write,
                    p.allow_system_mod_delete,
                ) = self._saved_policy
                self._saved_policy = None
            self._hardened = enabled
            self._record("system", "safe_mode", "info", f"{'ENABLED' if enabled else 'DISABLED'} ({reason})")
        self.events.emit("security.changed", self.snapshot(), source="security")

    def set_policy_flag(self, attr: str, value: bool) -> None:
        with self._lock:
            if self._hardened:
                self._record("fs", f"policy.{attr}", "deny", "Blocked while Safe Mode is active")
                raise SecurityError("Policy flags are locked in Safe Mode")
            if attr not in ("read_only", "validate_formats", "allow_sys_write", "allow_system_mod_delete"):
                raise ValueError(f"Unknown policy flag {attr!r}")
            setattr(self.policy, attr, bool(value))
            self._record("fs", f"policy.{attr}", "info", f"Set to {bool(value)}")
        self.events.emit("security.changed", self.snapshot(), source="security")

    def set_tx_armed(self, armed: bool, *, reason: str = "user") -> None:
        with self._lock:
            armed = bool(armed)
            if armed and self._hardened:
                self._record("rf", "arm_tx", "deny", "Blocked by Safe Mode")
                raise SecurityError("Safe Mode blocks TX-over-USB arming")
            self._allow_usb_tx = armed
            self._tx_armed_at = time.monotonic() if armed else 0.0
            self._record("rf", "arm_tx", "allow" if armed else "info", f"TX={'ARMED' if armed else 'LOCKED'} ({reason})")
        self.events.emit("security.changed", self.snapshot(), source="security")

    def disarm_tx(self, reason: str = "link_transition") -> None:
        with self._lock:
            was = self._allow_usb_tx
            self._allow_usb_tx = False
            self._tx_armed_at = 0.0
            if was:
                self._record("rf", "disarm_tx", "info", reason)
        if was:
            self.events.emit("security.changed", self.snapshot(), source="security")

    def check_tx(self, *, demo_mode: bool = False) -> None:
        if demo_mode:
            return
        if not self.allow_usb_tx:
            self._record("rf", "ptt", "deny", "TX-over-USB safety latch engaged")
            raise SecurityError("TX locked over USB (RF safety) — arm 'Allow TX over USB' first")

    def check_bootloader_entry(self) -> None:
        if self.hardened:
            self._record("firmware", "reboot_bootloader", "deny", "Blocked by Safe Mode")
            raise SecurityError("Safe Mode blocks bootloader entry")
        self._record("firmware", "reboot_bootloader", "allow", "Permitted")

    def check_firmware_flash(self, fw_size: int) -> None:
        if self.hardened:
            self._record("firmware", "flash", "deny", "Blocked by Safe Mode")
            raise SecurityError("Safe Mode blocks firmware flashing")
        if fw_size < 256 or fw_size > MAX_FIRMWARE_BYTES:
            self._record("firmware", "flash", "deny", f"Rejected size {fw_size} B")
            raise SecurityError(f"Firmware image size out of bounds ({fw_size:,} B)")
        self._record("firmware", "flash", "allow", f"Pre-flight OK ({fw_size:,} B)")

    def check_sideload(self) -> None:
        if self.hardened:
            self._record("firmware", "sideload", "deny", "Blocked by Safe Mode")
            raise SecurityError("Safe Mode blocks KRX app installs")

    def check_write(self, remote_path: str, data: bytes) -> str:
        clean = sanitize_kernfs_path(remote_path)
        if len(data) > MAX_UPLOAD_BYTES:
            self._record("fs", "write", "deny", f"{clean} exceeds {MAX_UPLOAD_BYTES} B limit")
            raise SecurityError(f"File exceeds {MAX_UPLOAD_BYTES:,} B safety ceiling")
        try:
            self.policy.check_write(clean, data)
        except Exception as exc:
            self._record("fs", "write", "deny", f"{clean}: {exc}")
            raise SecurityError(str(exc)) from exc
        self._record("fs", "write", "allow", f"{clean} ({len(data):,} B)")
        return clean

    def check_delete(self, remote_path: str) -> str:
        clean = sanitize_kernfs_path(remote_path)
        try:
            self.policy.check_delete(clean)
        except Exception as exc:
            self._record("fs", "delete", "deny", f"{clean}: {exc}")
            raise SecurityError(str(exc)) from exc
        self._record("fs", "delete", "allow", clean)
        return clean

    def check_ipc_command(self, cmd: str, *, mutates: bool) -> None:
        if mutates and not self.ipc_allow_control:
            self._record("ipc", cmd, "deny", "Mutating IPC command blocked (read-only or Safe Mode)")
            raise SecurityError(f"IPC command '{cmd}' blocked by security policy")
        self._record("ipc", cmd, "allow", "mutating" if mutates else "read-only")

    def check_plugin_capability(self, plugin_id: str, cap: str) -> None:
        write_caps = {"radio.control", "fs.write", "firmware.flash", "system.exec"}
        if self.hardened and cap in write_caps:
            self._record("plugin", f"{plugin_id}:{cap}", "deny", "Blocked by Safe Mode")
            raise SecurityError(f"Plugin '{plugin_id}' capability '{cap}' blocked in Safe Mode")

    def record(self, category: str, action: str, outcome: str, detail: str) -> None:
        with self._lock:
            self._record(category, action, outcome, detail)

    def _record(self, category: str, action: str, outcome: str, detail: str) -> None:
        self._audit.append(AuditEntry(time.strftime("%H:%M:%S"), category, action, outcome, detail))

    def audit_entries(self, limit: int = 80) -> list[AuditEntry]:
        with self._lock:
            items = list(self._audit)
            return items[-limit:] if limit > 0 else items

    def snapshot(self) -> dict[str, Any]:
        with self._lock:
            return {
                "hardened": self._hardened,
                "profile": self.profile.value,
                "allow_usb_tx": self._allow_usb_tx and not self._hardened,
                "ipc_allow_control": self._ipc_allow_control and not self._hardened,
                "read_only": self.policy.read_only,
                "validate_formats": self.policy.validate_formats,
                "allow_sys_write": self.policy.allow_sys_write,
                "allow_system_mod_delete": self.policy.allow_system_mod_delete,
            }
