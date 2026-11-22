"""KRX binary & krx.toml manifest inspector, capability decoder, and workspace package discovery."""

from __future__ import annotations

import struct
import subprocess
import sys
import tomllib
import zlib
from pathlib import Path
from typing import Any

import kernfs_mount

from ..ui.widgets.app_icon import KRX_IDENT, _find_krx_icon_asset, _pack_ascii_grid

# Exact mirror of src/abi/src/cap.rs (bits 0..25): (ident, human_label, sensitive)
CAP_TABLE: dict[int, tuple[str, str, bool]] = {
    0: ("RADIO_RX", "Radio RX", False),
    1: ("RADIO_TX", "Radio TX", True),
    2: ("STORAGE_READ", "Storage Read", False),
    3: ("STORAGE_WRITE", "Storage Write", True),
    4: ("AUDIO_OUTPUT", "Audio Out", False),
    5: ("DISPLAY_DIRECT", "Display", False),
    6: ("SERIAL_COMM", "Serial", False),
    7: ("USB_COMM", "USB", False),
    8: ("HARDWARE_CRC", "HW CRC", False),
    9: ("REAL_TIME_CLOCK", "Clock", False),
    10: ("POWER_MANAGE", "Power", True),
    11: ("CRYPTO_AEAD", "Crypto", False),
    12: ("RAW_PERIPHERAL", "Raw Hardware", True),
    13: ("BROAD_BAND_RX", "Broadband RX", False),
    14: ("GNSS_LOCATION", "GNSS", False),
    15: ("WIRELESS_LINK", "Wireless", False),
    16: ("AUDIO_CAPTURE", "Audio In", False),
    17: ("BROADCAST_FM", "Broadcast FM", False),
    18: ("KEYPAD", "Keypad", False),
    19: ("TASK_CONTROL", "Task Control", True),
    20: ("SERVICE_REGISTRY", "Services / IPC", False),
    21: ("BUS_ACCESS", "I2C / SPI Bus", True),
    22: ("GPIO_ACCESS", "GPIO", True),
    23: ("STORAGE_ADMIN", "Storage Admin", True),
    24: ("FACTORY_CALIB", "Factory Calib", True),
    25: ("PREFERENCES", "Preferences", False),
}

CAP_BY_IDENT: dict[str, int] = {ident: bit for bit, (ident, _lbl, _sens) in CAP_TABLE.items()}

LIFECYCLE_FLAG_NAMES: dict[int, str] = {
    0: "Startup",
    1: "Persistent",
    2: "Restart on Fail",
    3: "Singleton",
    4: "Auto-Update",
    5: "RTC",
}

EXEC_TYPE_NAMES: dict[int, str] = {
    1: "App",
    2: "Service",
    3: "Daemon",
    4: "Driver",
    5: "Module",
}

CLASS_NAMES: dict[int, str] = {
    0: "Native",
    1: "Paged",
    2: "VM",
    3: "Declare",
}

PROFILE_LABELS: dict[str, str] = {
    "n0": "N0 · Native Fixed",
    "n1": "N1 · Native PIC",
    "p0": "P0 · Paged",
    "p1": "P1 · Paged+",
    "v0": "V0 · VM",
    "d0": "D0 · Declare",
}

PKG_GROUPS: tuple[str, ...] = ("core", "sys", "apps", "examples")
GROUP_LABELS: dict[str, str] = {
    "core": "Core",
    "sys": "System",
    "apps": "Apps",
    "examples": "Examples",
}


def decode_caps_mask(mask: int) -> list[dict[str, Any]]:
    """Decode a 32-bit capability mask into human-readable descriptors."""
    out: list[dict[str, Any]] = []
    for bit in range(32):
        if not (mask & (1 << bit)):
            continue
        if bit in CAP_TABLE:
            ident, label, sensitive = CAP_TABLE[bit]
        else:
            ident, label, sensitive = f"CAP_{bit}", f"Cap #{bit}", True
        out.append({"bit": bit, "ident": ident, "label": label, "sensitive": sensitive})
    return out


def encode_cap_names(names: Any) -> int:
    """Resolve a list/pipe-separated string/int of capability names into a 32-bit mask."""
    if isinstance(names, int):
        return names
    if isinstance(names, str):
        items = [s.strip() for s in names.split("|") if s.strip()]
    elif isinstance(names, (list, tuple)):
        items = [str(s).strip() for s in names if str(s).strip()]
    else:
        return 0
    mask = 0
    for item in items:
        key = item.upper().removeprefix("KRX_CAP_")
        if key in CAP_BY_IDENT:
            mask |= 1 << CAP_BY_IDENT[key]
        else:
            try:
                mask |= int(item, 0)
            except ValueError:
                pass
    return mask


def decode_lifecycle_flags(flags: int) -> list[str]:
    return [label for bit, label in LIFECYCLE_FLAG_NAMES.items() if flags & (1 << bit)]


def _parse_ascii_icon(text: str, w: int, h: int) -> tuple[int, int, bytes] | None:
    lines = [ln.strip() for ln in text.strip().splitlines() if ln.strip()]
    if len(lines) < h:
        return None
    return (w, h, _pack_ascii_grid(lines[:h], w, h))


def inspect_krx_binary(source: Path | bytes, filename: str = "") -> dict[str, Any]:
    """Deep-inspect a compiled `.krx` or `.mod` binary, including CRC-32, sections, caps & actions."""
    fpath: Path | None = None
    if isinstance(source, (str, Path)):
        fpath = Path(source)
        data = fpath.read_bytes()
        if not filename:
            filename = fpath.name
    else:
        data = bytes(source)

    stem = Path(filename).stem.replace("_", " ").replace("-", " ").title() if filename else "App"
    ext = Path(filename).suffix.lstrip(".").upper() or "KRX"
    remote_path = f"/apps/{filename}" if filename else "/apps/app.krx"

    out: dict[str, Any] = {
        "source_kind": "binary",
        "path": str(fpath) if fpath else "",
        "built_path": str(fpath) if fpath else "",
        "built_exists": True,
        "filename": filename or "app.krx",
        "remote_path": remote_path,
        "protected": remote_path in kernfs_mount.PROTECTED_RECOVERY_MODS,
        "valid_krx": False,
        "crc_ok": False,
        "name": stem,
        "version": "",
        "min_abi": 0,
        "author": "",
        "exec_type": "Module" if ext == "MOD" else "App",
        "class_name": ext,
        "profile_label": ext,
        "lifecycle": [],
        "caps": 0,
        "cap_list": [],
        "actions": [],
        "sections": [],
        "icon_asset": 0,
        "bitmap": None,
        "size": len(data),
        "header_len": min(len(data), 24),
        "min_stack": 0,
        "min_heap": 0,
        "page_size": 0,
        "raw_bytes": data,
    }
    if len(data) < 24 or data[:4] != KRX_IDENT:
        return out

    app_class = data[4]
    sec_count, hdr_len, _flags = struct.unpack_from("<HHH", data, 6)
    payload_crc, header_crc, _link_base = struct.unpack_from("<III", data, 12)

    out["valid_krx"] = True
    out["class_name"] = CLASS_NAMES.get(app_class, f"Class {app_class}")
    out["profile_label"] = "Paged (P0)" if app_class == 1 else ("Native (N1)" if app_class == 0 else out["class_name"])
    out["header_len"] = min(len(data), max(24, hdr_len))
    out["bitmap"] = _find_krx_icon_asset(data, hdr_len)

    if 24 <= hdr_len <= len(data):
        calc_hdr = zlib.crc32(data[24:hdr_len]) & 0xFFFFFFFF
        calc_pay = zlib.crc32(data[hdr_len:]) & 0xFFFFFFFF
        out["crc_ok"] = (calc_hdr == header_crc) and (calc_pay == payload_crc)

    off = 24
    limit = min(len(data), hdr_len)
    sections: list[int] = []
    for _ in range(min(sec_count, 32)):
        if off + 4 > limit:
            break
        sec_id, sec_len = struct.unpack_from("<HH", data, off)
        if sec_len < 4 or off + sec_len > limit:
            break
        sections.append(sec_id)
        body = data[off + 4 : off + sec_len]
        if sec_id == 0x0001 and len(body) >= 25:  # Identity
            name_off = struct.unpack_from("<H", body, 0)[0]
            maj, minor, patch = body[2], body[3], body[4]
            out["version"] = f"v{maj}.{minor}.{patch}"
            out["min_abi"] = struct.unpack_from("<H", body, 5)[0]
            raw_auth = body[7:19].split(b"\x00")[0].decode("utf-8", errors="ignore").strip()
            if raw_auth:
                out["author"] = raw_auth
            out["icon_asset"] = struct.unpack_from("<H", body, 23)[0]
            if 24 <= name_off < limit:
                nul = data.find(b"\x00", name_off, limit)
                end = nul if nul != -1 else limit
                nm = data[name_off:end].decode("utf-8", errors="ignore").strip()
                if nm:
                    out["name"] = nm
        elif sec_id == 0x0002 and len(body) >= 12:  # Execution
            min_stack, min_heap, page_sz = struct.unpack_from("<HHH", body, 0)
            out["min_stack"] = min_stack
            out["min_heap"] = min_heap
            out["page_size"] = page_sz
        elif sec_id == 0x0009 and len(body) >= 12:  # Type & Lifecycle
            out["exec_type"] = EXEC_TYPE_NAMES.get(body[0], "App")
            out["lifecycle"] = decode_lifecycle_flags(body[1])
            caps = struct.unpack_from("<I", body, 8)[0]
            out["caps"] = caps
            out["cap_list"] = decode_caps_mask(caps)
        elif sec_id == 0x000B and len(body) >= 16:  # Actions
            actions: list[str] = []
            for a_off in range(0, len(body) - 15, 16):
                lbl = body[a_off + 4 : a_off + 16].split(b"\x00")[0].decode("utf-8", errors="ignore").strip()
                if lbl:
                    actions.append(lbl)
            out["actions"] = actions
        off += (sec_len + 3) & ~3

    out["sections"] = sections
    return out


def inspect_krx_manifest(toml_path: Path, *, group: str = "", pkg_id: str = "") -> dict[str, Any]:
    """Parse a `krx.toml` package manifest and check whether its compiled `.krx`/`.mod` exists."""
    p = Path(toml_path)
    with p.open("rb") as f:
        m = tomllib.load(f)

    pkg_dir = p.parent
    output_name = str(m.get("output") or f"{pkg_dir.name}.krx")
    built_path = pkg_dir / output_name
    built_exists = built_path.is_file()

    profile_raw = str(m.get("profile", "n1")).lower()
    exec_type_raw = str(m.get("type", "app")).lower()
    exec_type = exec_type_raw.capitalize()
    caps_mask = encode_cap_names(m.get("caps", []))
    lifecycle = [str(f).replace("_", " ").title() for f in m.get("lifecycle", [])]
    actions = [str(a.get("label", "")).strip() for a in m.get("action", []) if str(a.get("label", "")).strip()]

    bitmap: tuple[int, int, bytes] | None = None
    if isinstance(m.get("icon_16"), str):
        bitmap = _parse_ascii_icon(m["icon_16"], 16, 16)
    elif isinstance(m.get("icon_8"), str):
        bitmap = _parse_ascii_icon(m["icon_8"], 8, 8)

    ver_raw = str(m.get("version", "1.0.0")).strip()
    ver = ver_raw if ver_raw.startswith("v") else f"v{ver_raw}"
    remote_path = f"/apps/{output_name}"

    out: dict[str, Any] = {
        "source_kind": "manifest",
        "path": str(p),
        "manifest_path": str(p),
        "built_path": str(built_path),
        "built_exists": built_exists,
        "pkg_id": pkg_id or pkg_dir.name,
        "group": group,
        "filename": output_name,
        "remote_path": remote_path,
        "protected": remote_path in kernfs_mount.PROTECTED_RECOVERY_MODS,
        "valid_krx": True,
        "crc_ok": False,
        "name": str(m.get("name") or pkg_dir.name.replace("_", " ").title()),
        "version": ver,
        "min_abi": 0,
        "author": str(m.get("author", "KERN-TEAM")),
        "exec_type": exec_type,
        "class_name": "Paged" if profile_raw.startswith("p") else "Native",
        "profile_label": PROFILE_LABELS.get(profile_raw, profile_raw.upper()),
        "lifecycle": lifecycle,
        "caps": caps_mask,
        "cap_list": decode_caps_mask(caps_mask),
        "actions": actions,
        "icon_asset": 0,
        "bitmap": bitmap,
        "size": built_path.stat().st_size if built_exists else 0,
        "leaves_count": len(m.get("leaves", [])),
        "blocks_count": len(m.get("blocks", [])),
        "raw_bytes": None,
    }

    if built_exists:
        try:
            bin_info = inspect_krx_binary(built_path, output_name)
            out["crc_ok"] = bin_info["crc_ok"]
            out["size"] = bin_info["size"]
            out["min_abi"] = bin_info["min_abi"]
            if out["bitmap"] is None and bin_info["bitmap"] is not None:
                out["bitmap"] = bin_info["bitmap"]
            if not out["cap_list"] and bin_info["cap_list"]:
                out["caps"] = bin_info["caps"]
                out["cap_list"] = bin_info["cap_list"]
        except Exception:
            pass

    return out


def inspect_package_path(path: str | Path) -> dict[str, Any]:
    """Inspect either a `.krx`/`.mod` binary, a `krx.toml` manifest, or a directory containing `krx.toml`."""
    p = Path(path)
    if p.is_dir():
        toml = p / "krx.toml"
        if toml.is_file():
            return inspect_krx_manifest(toml)
        raise ValueError(f"Directory '{p.name}' does not contain a krx.toml manifest")
    if p.name.lower() == "krx.toml" or p.suffix.lower() == ".toml":
        return inspect_krx_manifest(p)
    info = inspect_krx_binary(p)
    if not info["valid_krx"]:
        raise ValueError(f"'{p.name}' is not a valid KRX package (missing KRX format identifier)")
    return info


def discover_workspace_packages(repo_root: Path) -> list[dict[str, Any]]:
    """Scan `packages/{core,sys,apps,examples}/*/krx.toml` and return parsed package descriptors."""
    pkgs_root = repo_root / "packages"
    if not pkgs_root.is_dir():
        return []
    found: list[dict[str, Any]] = []
    for group in PKG_GROUPS:
        gdir = pkgs_root / group
        if not gdir.is_dir():
            continue
        for entry in sorted(gdir.iterdir(), key=lambda x: x.name.lower()):
            toml = entry / "krx.toml"
            if entry.is_dir() and toml.is_file():
                try:
                    found.append(inspect_krx_manifest(toml, group=group, pkg_id=f"{group}/{entry.name}"))
                except Exception:
                    continue
    return found


def build_workspace_package(pkg_id: str, repo_root: Path) -> tuple[bool, str]:
    """Build a workspace package via `tools/pkg.py` and return `(ok, combined_output)`."""
    script = repo_root / "tools" / "pkg.py"
    if not script.is_file():
        return False, "tools/pkg.py not found in workspace"
    try:
        proc = subprocess.run(
            [sys.executable, str(script), pkg_id],
            cwd=str(repo_root),
            capture_output=True,
            text=True,
            timeout=120,
        )
    except Exception as exc:
        return False, str(exc)
    out = (proc.stdout or "") + (("\n" + proc.stderr) if proc.stderr else "")
    return proc.returncode == 0, out.strip()
