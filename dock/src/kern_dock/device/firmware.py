"""Firmware images, stock-bootloader flashing and .kfw calibration / filesystem backups."""

from __future__ import annotations

import hashlib
import json
import math
import re
import struct
import time
import urllib.parse
import urllib.request
import zipfile
from pathlib import Path
from typing import Any, Callable

import kernfs_mount
import kfw

try:
    import flasher as _flasher
except Exception:
    _flasher = None  # type: ignore[assignment]

FLASH_TOTAL_BYTES = 128 * 1024
_GH_REPO_RE = re.compile(r"^(?:https?://github\.com/)?([A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+?)(?:\.git|/releases.*|/)?$")


def parse_github_repo_slug(repo_or_url: str) -> str | None:
    """Extract `owner/repo` from a GitHub slug or URL; returns None for direct file URLs."""
    s = repo_or_url.strip()
    if not s:
        return None
    if s.startswith(("http://", "https://")):
        parsed = urllib.parse.urlparse(s)
        if parsed.netloc.lower() != "github.com":
            return None
        parts = [p for p in parsed.path.split("/") if p]
        if len(parts) >= 2 and ("/releases/download/" not in parsed.path) and not parts[-1].lower().endswith((".kfw", ".bin")):
            return f"{parts[0]}/{parts[1].removesuffix('.git')}"
        return None
    m = _GH_REPO_RE.match(s)
    return m.group(1) if m else None


def fetch_github_releases(repo_or_url: str, *, limit: int = 8) -> list[dict[str, Any]]:
    """Fetch GitHub releases & `.kfw`/`.bin` assets for an `owner/repo` slug or URL."""
    slug = parse_github_repo_slug(repo_or_url)
    if not slug:
        raise ValueError("Enter a valid GitHub 'owner/repo' or repository URL")
    api_url = f"https://api.github.com/repos/{slug}/releases?per_page={max(1, min(20, limit))}"
    req = urllib.request.Request(
        api_url,
        headers={
            "Accept": "application/vnd.github+json",
            "User-Agent": "KernDock/2.0",
        },
    )
    with urllib.request.urlopen(req, timeout=15) as resp:
        raw = json.loads(resp.read().decode("utf-8"))
    releases: list[dict[str, Any]] = []
    for rel in raw:
        assets: list[dict[str, Any]] = []
        for a in rel.get("assets", []):
            name = str(a.get("name", ""))
            if name.lower().endswith((".kfw", ".bin")):
                assets.append({
                    "name": name,
                    "size": int(a.get("size", 0)),
                    "url": str(a.get("browser_download_url", "")),
                    "digest": str(a.get("digest", "")),
                })
        if assets:
            releases.append({
                "tag": str(rel.get("tag_name", "")),
                "name": str(rel.get("name") or rel.get("tag_name") or ""),
                "published_at": str(rel.get("published_at", ""))[:10],
                "prerelease": bool(rel.get("prerelease", False)),
                "assets": assets,
            })
    return releases


def download_firmware_url(
    url: str,
    dest_dir: Path | None = None,
    *,
    expected_sha256: str = "",
    progress_cb: Callable[[float, str], None] | None = None,
) -> tuple[Path, dict[str, Any]]:
    """Download a `.kfw` or `.bin` firmware image, verify optional SHA-256, and run pre-flight validation."""
    parsed = urllib.parse.urlparse(url.strip())
    if parsed.scheme not in ("http", "https"):
        raise ValueError("Only http:// and https:// URLs are supported")
    fname = Path(urllib.parse.unquote(parsed.path)).name or "firmware.bin"
    fname = re.sub(r"[^A-Za-z0-9._-]", "_", fname)
    if not fname.lower().endswith((".kfw", ".bin")):
        fname += ".bin"

    cache_dir = dest_dir or (Path.home() / ".cache" / "kern-dock" / "firmware")
    cache_dir.mkdir(parents=True, exist_ok=True)
    dest = cache_dir / fname

    req = urllib.request.Request(url.strip(), headers={"User-Agent": "KernDock/2.0"})
    h = hashlib.sha256()
    total_read = 0
    with urllib.request.urlopen(req, timeout=30) as resp:
        total_len = int(resp.headers.get("Content-Length") or 0)
        with dest.open("wb") as out_f:
            while True:
                chunk = resp.read(16384)
                if not chunk:
                    break
                out_f.write(chunk)
                h.update(chunk)
                total_read += len(chunk)
                if progress_cb and total_len > 0:
                    progress_cb(min(1.0, total_read / float(total_len)), f"Downloading {fname}…")

    actual_sha = h.hexdigest().lower()
    exp = expected_sha256.strip().lower().removeprefix("sha256:")
    if exp and actual_sha != exp:
        dest.unlink(missing_ok=True)
        raise ValueError(f"SHA-256 mismatch: expected {exp[:12]}…, got {actual_sha[:12]}…")

    info = inspect_firmware_file(dest)
    info["path"] = str(dest)
    info["source_url"] = url.strip()
    info["sha256"] = actual_sha
    return dest, info


def _check_armv6m_vectors(raw_fw: bytes) -> tuple[bool, int, int]:
    """Sanity-check Cortex-M0+ initial SP & Thumb Reset_Handler vector."""
    if len(raw_fw) < 8:
        return False, 0, 0
    sp, reset = struct.unpack_from("<II", raw_fw, 0)
    sp_ok = (0x2000_0000 <= sp <= 0x2001_0000) and (sp % 4 == 0)
    reset_ok = (reset & 1 == 1) and ((0x0800_0000 <= reset < 0x0804_0000) or (0x0000_0000 < reset < 0x0004_0000))
    return (sp_ok and reset_ok), sp, reset


def discover_local_firmware(repo_root: Path) -> list[Path]:
    """Return recently built `.kfw` or packed `.bin` firmware images under `target/` and `dist/`."""
    candidates: list[Path] = []
    target = repo_root / "target"
    if target.is_dir():
        for name in ("kern.kfw", "firmware.kfw", "firmware_packed.bin"):
            p = target / name
            if p.is_file() and 256 <= p.stat().st_size <= FLASH_TOTAL_BYTES * 4:
                candidates.append(p)
        for p in sorted(target.glob("*.kfw"), key=lambda x: x.stat().st_mtime, reverse=True):
            if p not in candidates and p.is_file():
                candidates.append(p)
    dist = repo_root / "dist"
    if dist.is_dir():
        for p in sorted(dist.glob("*.kfw"), key=lambda x: x.stat().st_mtime, reverse=True):
            if p not in candidates and p.is_file():
                candidates.append(p)
    return candidates[:6]


def inspect_firmware_file(
    fpath: str | Path,
    *,
    trust_store_path: str | Path | None = None,
) -> dict[str, Any]:
    """Inspects and pre-flight validates a `.kfw` container or raw `.bin` firmware image."""
    p = Path(fpath)
    data = p.read_bytes()
    if data[:8] == kfw.FILE_ID_KFW:
        parsed = kfw.parse_kfw(data, verify_root=True)
        pts = kfw.extract_all_parts(parsed)
        fw_parts = [pt for pt in parsed.parts if pt["kind"] == kfw.PART_FIRMWARE]
        mod_parts = [
            pt for pt in parsed.parts
            if pt["kind"] in (kfw.PART_KRX_MODULE, kfw.PART_KERNFS_FILE, kfw.PART_OPT_KRX, kfw.PART_OPT_FILE)
        ]
        targets = [t.slug for t in parsed.targets]
        if not fw_parts:
            raise ValueError("Container has no firmware image part (may be a backup/module pack)")
        raw_fw = pts[fw_parts[0]["index"]]
        vec_ok, sp, reset = _check_armv6m_vectors(raw_fw)
        pages = max(1, math.ceil(len(raw_fw) / 256.0))
        sha_short = hashlib.sha256(raw_fw).hexdigest()[:12]

        ts_file = Path(trust_store_path) if trust_store_path else kfw.default_trust_store_path()
        ts_obj = kfw.load_trust_store(ts_file) if ts_file.is_file() else None
        signed = bool(parsed.auth_tlvs)
        auth_notes: list[str] = []
        trusted = False
        if signed:
            auth_notes = kfw.verify_auth(parsed, trust_store=ts_obj, require_auth=False)
            trusted = bool(auth_notes) and all("UNTRUSTED" not in a and "UNPINNED" not in a for a in auth_notes)

        trust_label = "Signed & Trusted" if trusted else ("Signed (unpinned)" if signed else "Unsigned")
        mod_note = f" · +{len(mod_parts)} mod(s)" if mod_parts else ""
        return {
            "format": "KFW",
            "version": kfw.format_semver(parsed.fw_version),
            "sec_cnt": parsed.sec_cnt,
            "krx_abi": kfw.format_abi(parsed.krx_abi_ver) if parsed.krx_abi_ver else "",
            "targets": targets,
            "fw_bytes": raw_fw,
            "size": len(raw_fw),
            "pages": pages,
            "flash_pct": min(1.0, len(raw_fw) / float(FLASH_TOTAL_BYTES)),
            "ascon_ok": True,
            "signed": signed,
            "trusted": trusted,
            "auth_notes": auth_notes,
            "vector_ok": vec_ok,
            "initial_sp": sp,
            "reset_pc": reset,
            "sha256_short": sha_short,
            "parts_count": len(parsed.parts),
            "modules_count": len(mod_parts),
            "summary": (
                f"KFW · {', '.join(targets) or 'generic'}"
                f" · {len(raw_fw):,} B · {trust_label}{mod_note}"
            ),
        }
    if len(data) < 256 or len(data) > FLASH_TOTAL_BYTES:
        raise ValueError(f"Unexpected firmware size: {len(data):,} B")
    vec_ok, sp, reset = _check_armv6m_vectors(data)
    pages = max(1, math.ceil(len(data) / 256.0))
    sha_short = hashlib.sha256(data).hexdigest()[:12]
    return {
        "format": "Raw BIN",
        "version": p.stem,
        "sec_cnt": 0,
        "krx_abi": "",
        "targets": ["raw"],
        "fw_bytes": data,
        "size": len(data),
        "pages": pages,
        "flash_pct": min(1.0, len(data) / float(FLASH_TOTAL_BYTES)),
        "ascon_ok": False,
        "signed": False,
        "trusted": False,
        "auth_notes": [],
        "vector_ok": vec_ok,
        "initial_sp": sp,
        "reset_pc": reset,
        "sha256_short": sha_short,
        "parts_count": 1,
        "modules_count": 0,
        "summary": f"Raw binary · {p.name} · {len(data):,} B",
    }


def sync_kfw_modules_over_rigwire(client: Any, kfw_path: str | Path) -> dict[str, list[str]]:
    """Sync Phase B `POST_FLASH` KRX modules/assets from a verified `.kfw` container over RigWire."""
    parsed = kfw.parse_kfw(Path(kfw_path).read_bytes(), verify_root=True)
    plaintexts = kfw.extract_all_parts(parsed)
    return kfw.provision_kfw_parts_over_rigwire(client, parsed, plaintexts, dry_run=False)


def flash_via_stock_bootloader(
    port: str,
    fw_bytes: bytes,
    progress_cb: Callable[[float, str], None] | None = None,
    page_cb: Callable[[int, int], None] | None = None,
) -> dict[str, Any]:
    """Flashes raw firmware bytes via Quansheng stock bootloader / CPS / Kern dfu.mod protocol."""
    if _flasher is None:
        raise RuntimeError("tools/flasher.py unavailable")
    import serial

    ser = serial.Serial(port, baudrate=_flasher.BAUDRATE, timeout=0.0001, write_timeout=2.0)
    proto = _flasher.Protocol(port)
    proto.ser = ser
    try:
        ser.reset_input_buffer()
        if progress_cb:
            progress_cb(0.02, "Waiting for bootloader beacon…")

        t0 = time.monotonic()
        last_ts = 0.0
        acc = 0
        dev_info = None
        while time.monotonic() - t0 < 12.0:
            msg = proto.fetch_message()
            if not msg:
                time.sleep(0.008)
                continue
            if msg[0] == _flasher.MSG_NOTIFY_DEV_INFO:
                now = time.monotonic()
                dt = (now - last_ts) * 1000.0
                last_ts = now
                if 5.0 <= dt <= 1000.0:
                    acc += 1
                    if acc >= 4:
                        uid = msg[1][:16].hex()
                        ver = msg[1][16:].split(b"\x00")[0].decode("utf-8", errors="ignore").strip()
                        dev_info = (uid, ver or "7.00")
                        break
                else:
                    acc = 0
        if dev_info is None:
            raise TimeoutError("Bootloader beacon timeout — ensure radio is in DFU / PTT+Power-On mode")

        uid, ver = dev_info
        if progress_cb:
            progress_cb(0.08, f"Bootloader {ver} · Handshaking…")

        sent = 0
        for _ in range(25):
            time.sleep(0.04)
            msg = proto.fetch_message()
            if msg and msg[0] == _flasher.MSG_NOTIFY_DEV_INFO:
                if sent == 0:
                    proto.send(_flasher.MSG_NOTIFY_BL_VER, ver[:4].encode("utf-8"))
                sent += 1
                if sent >= 3:
                    break
        time.sleep(0.18)
        while proto.fetch_message():
            pass

        page_sz = 256
        total_pages = math.ceil(len(fw_bytes) / page_sz)
        ts = int(time.time()) & 0xFFFFFFFF
        t_start = time.monotonic()

        for page in range(total_pages):
            chunk = fw_bytes[page * page_sz : (page + 1) * page_sz]
            payload = bytearray(268)
            struct.pack_into("<IHH", payload, 0, ts, page, total_pages)
            payload[12 : 12 + len(chunk)] = chunk

            ok = False
            for _attempt in range(3):
                proto.send(_flasher.MSG_PROG_FW, payload)
                wait_end = time.monotonic() + 3.0
                while time.monotonic() < wait_end:
                    resp = proto.fetch_message()
                    if not resp:
                        time.sleep(0.004)
                        continue
                    if resp[0] == _flasher.MSG_PROG_FW_RESP and len(resp[1]) >= 8:
                        r_page, r_err = struct.unpack_from("<HH", resp[1], 4)
                        if r_page == page and r_err == 0:
                            ok = True
                            break
                if ok:
                    break
            if not ok:
                raise RuntimeError(f"Flash page {page + 1}/{total_pages} failed")
            if page_cb:
                page_cb(page + 1, total_pages)
            if progress_cb:
                frac = 0.10 + 0.88 * ((page + 1) / float(total_pages))
                progress_cb(frac, f"Flashing page {page + 1}/{total_pages}")

        try:
            proto.send(_flasher.MSG_REBOOT)
        except Exception:
            pass
        if progress_cb:
            progress_cb(1.0, "Complete — rebooting radio")
        return {"bootloader": ver, "uid": uid, "pages": total_pages, "elapsed_s": time.monotonic() - t_start}
    finally:
        try:
            ser.close()
        except Exception:
            pass


def build_kfw_backup_from_backend(
    be: kernfs_mount.FsBackend,
    *,
    volume_name: str,
    calib_only: bool = False,
    progress_cb: Callable[[float, str], None] | None = None,
) -> bytes:
    """Packs verified factory calibration (or full KernFS snapshot) into an Ascon-Hash256 `.kfw` container."""
    files_to_pack: list[tuple[str, bytes]] = []
    if calib_only:
        paths = ["/sys/calib.dat", "/sys/calib.sig"]
    else:
        paths = []
        for root_dir in ("/sys", "/pref", "/user", "/apps"):
            try:
                for entry in be.ls(root_dir):
                    if not entry["is_dir"]:
                        paths.append(f"{root_dir}/{entry['name']}")
            except Exception:
                pass

    total = max(1, len(paths))
    for i, path in enumerate(paths):
        if progress_cb:
            progress_cb(i / float(total), f"Reading {path}…")
        try:
            st = be.stat(path)
            if st.get("exists") and not st.get("is_dir"):
                files_to_pack.append((path, be.read_bytes(path)))
        except Exception:
            continue

    if not files_to_pack:
        raise RuntimeError("No readable files found to back up")

    parts: list[kfw.PartInput] = []
    for path, data in files_to_pack:
        pkind = kfw.PART_CALIBRATION if path == "/sys/calib.dat" else kfw.PART_KERNFS_FILE
        parts.append(kfw.PartInput(kind=pkind, data=data, dest_path=path, in_solid=True))

    t0, bp0, prs0 = kfw._make_target_entries("uv-k1", 0)
    if progress_cb:
        progress_cb(0.95, "Sealing Ascon-Hash256 .kfw backup…")
    blob = kfw.build_kfw(
        kind=kfw.KIND_BACKUP,
        fw_version=kfw.pack_semver("1.0.0"),
        targets=[t0],
        boot_policies=[bp0],
        protect_ranges=prs0,
        parts=parts,
        manifest={
            "name": f"{volume_name} Backup",
            "scope": "calibration" if calib_only else "full-kernfs",
            "timestamp": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
            "file_count": len(files_to_pack),
        },
    )
    if progress_cb:
        progress_cb(1.0, f"Backed up {len(files_to_pack)} file(s)")
    return blob


def restore_kfw_backup_to_backend(
    kfw_path: str | Path,
    be: kernfs_mount.FsBackend,
    policy: kernfs_mount.SafetyPolicy,
    progress_cb: Callable[[float, str], None] | None = None,
) -> tuple[int, int]:
    """Verifies Ascon digests & restores user/pref/app files from a `.kfw` backup while shielding `/sys/*`."""
    parsed = kfw.parse_kfw(Path(kfw_path).read_bytes(), verify_root=True)
    pts = kfw.extract_all_parts(parsed)
    restored = skipped = 0
    file_parts = [
        pt for pt in parsed.parts
        if pt["index"] in parsed.part_paths or pt["kind"] == kfw.PART_CALIBRATION
    ]
    total = max(1, len(file_parts))
    for i, pt in enumerate(file_parts):
        if pt["kind"] == kfw.PART_CALIBRATION:
            skipped += 1
            continue
        path = parsed.part_paths[pt["index"]]["path"]
        data = pts[pt["index"]]
        if progress_cb:
            progress_cb(i / float(total), f"Restoring {path}…")
        if path.startswith("/sys/"):
            skipped += 1
            continue
        policy.check_write(path, data)
        be.write_bytes(path, data)
        restored += 1
    if progress_cb:
        progress_cb(1.0, f"Restored {restored} file(s) ({skipped} protected /sys skipped)")
    return restored, skipped


def build_zip_backup_from_backend(
    be: kernfs_mount.FsBackend,
    *,
    volume_name: str,
    calib_only: bool = False,
    progress_cb: Callable[[float, str], None] | None = None,
) -> bytes:
    """Create a portable, human-inspectable `.zip` radio backup with `manifest.json` and directory tree."""
    import io

    files_to_pack: list[tuple[str, bytes]] = []
    if calib_only:
        paths = ["/sys/calib.dat", "/sys/calib.sig"]
    else:
        paths = []
        for root_dir in ("/sys", "/pref", "/user", "/apps"):
            try:
                for entry in be.ls(root_dir):
                    if not entry["is_dir"]:
                        paths.append(f"{root_dir}/{entry['name']}")
            except Exception:
                pass

    total = max(1, len(paths))
    for i, path in enumerate(paths):
        if progress_cb:
            progress_cb(i / float(total), f"Reading {path}…")
        try:
            st = be.stat(path)
            if st.get("exists") and not st.get("is_dir"):
                files_to_pack.append((path, be.read_bytes(path)))
        except Exception:
            continue

    if not files_to_pack:
        raise RuntimeError("No readable files found to back up")

    manifest = {
        "format": "kern-backup-zip-v1",
        "name": f"{volume_name} Backup",
        "scope": "calibration" if calib_only else "full-kernfs",
        "timestamp": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "file_count": len(files_to_pack),
        "files": [{"path": p, "size": len(d)} for p, d in files_to_pack],
    }
    buf = io.BytesIO()
    with zipfile.ZipFile(buf, "w", compression=zipfile.ZIP_DEFLATED) as zf:
        zf.writestr("manifest.json", json.dumps(manifest, indent=2))
        for path, data in files_to_pack:
            zf.writestr(path.lstrip("/"), data)
    if progress_cb:
        progress_cb(1.0, f"Archived {len(files_to_pack)} file(s) to .zip")
    return buf.getvalue()


def inspect_backup_file(fpath: str | Path) -> dict[str, Any]:
    """
    Inspect a `.kfw` sealed backup, portable `.zip` radio archive, or raw `.dat`/`.bin` calibration file.
    Returns `{format, verified, name, scope, timestamp, entries: [{path, size, data, protected}]}`.
    """
    p = Path(fpath)
    raw = p.read_bytes()
    if raw[:8] == kfw.FILE_ID_KFW:
        parsed = kfw.parse_kfw(raw, verify_root=True)
        pts = kfw.extract_all_parts(parsed)
        entries: list[dict[str, Any]] = []
        for pt in parsed.parts:
            idx = pt["index"]
            data = pts[idx]
            if idx in parsed.part_paths:
                rpath = parsed.part_paths[idx]["path"]
            elif pt["kind"] == kfw.PART_CALIBRATION:
                rpath = "/sys/calib.dat"
            elif pt["kind"] == kfw.PART_FIRMWARE:
                rpath = "/firmware.bin"
            else:
                rpath = f"/part_{idx}.bin"
            entries.append({"path": rpath, "size": len(data), "data": data, "protected": rpath.startswith("/sys/")})
        man = parsed.manifest if isinstance(parsed.manifest, dict) else {}
        return {
            "path": str(p),
            "format": "KFW (Ascon-Hash256)",
            "verified": True,
            "name": str(man.get("name", p.stem)),
            "scope": str(man.get("scope", "snapshot")),
            "timestamp": str(man.get("timestamp", "")),
            "entries": entries,
        }

    if zipfile.is_zipfile(p):
        entries = []
        man: dict[str, Any] = {}
        with zipfile.ZipFile(p, "r") as zf:
            if "manifest.json" in zf.namelist():
                try:
                    man = json.loads(zf.read("manifest.json").decode("utf-8"))
                except Exception:
                    man = {}
            for name in zf.namelist():
                if name.endswith("/") or name == "manifest.json":
                    continue
                data = zf.read(name)
                rpath = "/" + name.lstrip("/")
                entries.append({"path": rpath, "size": len(data), "data": data, "protected": rpath.startswith("/sys/")})
        return {
            "path": str(p),
            "format": "ZIP Archive",
            "verified": True,
            "name": str(man.get("name", p.stem)),
            "scope": str(man.get("scope", "archive")),
            "timestamp": str(man.get("timestamp", "")),
            "entries": entries,
        }

    # Raw calibration / binary dump
    rpath = "/sys/calib.dat" if len(raw) == 512 else f"/{p.name}"
    return {
        "path": str(p),
        "format": "Raw Calibration (512 B)" if len(raw) == 512 else "Raw Binary Dump",
        "verified": len(raw) > 0,
        "name": p.name,
        "scope": "calibration" if len(raw) == 512 else "raw",
        "timestamp": "",
        "entries": [{"path": rpath, "size": len(raw), "data": raw, "protected": rpath.startswith("/sys/")}],
    }


def restore_backup_to_backend(
    backup_path: str | Path,
    be: kernfs_mount.FsBackend,
    policy: kernfs_mount.SafetyPolicy,
    progress_cb: Callable[[float, str], None] | None = None,
) -> tuple[int, int]:
    """Restore user/pref/app files from either a `.kfw` or `.zip` backup while shielding `/sys/*`."""
    p = Path(backup_path)
    if p.read_bytes()[:8] == kfw.FILE_ID_KFW:
        return restore_kfw_backup_to_backend(p, be, policy, progress_cb=progress_cb)
    info = inspect_backup_file(p)
    entries = info["entries"]
    restored = skipped = 0
    total = max(1, len(entries))
    for i, e in enumerate(entries):
        rpath = str(e["path"])
        data = bytes(e["data"])
        if progress_cb:
            progress_cb(i / float(total), f"Restoring {rpath}…")
        if rpath.startswith("/sys/"):
            skipped += 1
            continue
        policy.check_write(rpath, data)
        be.write_bytes(rpath, data)
        restored += 1
    if progress_cb:
        progress_cb(1.0, f"Restored {restored} file(s) ({skipped} protected /sys skipped)")
    return restored, skipped
