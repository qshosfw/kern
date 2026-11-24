#!/usr/bin/env python3
"""Kern End-to-End Signed Release Pipeline (`./kern release`).

Orchestrates reproducible multi-target firmware builds, core + system KRX package builds,
universal Solid ARMTHUMB+LZMA2 + per-target MCU-OTA `.kfw` bundling, PQ LMS-Ascon /
Ascon-128-MAC cryptographic signing, trust-store verification, and signed
`release-manifest.json` + `SHA256SUMS.txt` emission.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path
from typing import Any

TOOLS_DIR = Path(__file__).resolve().parent
REPO_ROOT = TOOLS_DIR.parent
if str(TOOLS_DIR) not in sys.path:
    sys.path.insert(0, str(TOOLS_DIR))

import cli_ui  # noqa: E402
import kfw  # noqa: E402

cli_ui.init_console()

# Canonical release firmware targets: (cli_target, kfw_slug, artifact_slug)
RELEASE_TARGETS: list[tuple[str, str, str]] = [
    ("UV-K1", "uv-k1", "uv-k1-k5v3"),
    ("UV-K10Pro", "uv-k10pro", "uv-k10pro"),
]

# Core & System KRX packages bundled in every universal release `.kfw`
RELEASE_MODULES: list[tuple[str, str, str]] = [
    ("packages/core/vfo/vfo.krx", "/apps/vfo.krx", "always"),
    ("packages/core/settings/settings.krx", "/apps/settings.krx", "always"),
    ("packages/sys/dfu/dfu.mod", "/apps/dfu.mod", "always"),
    ("packages/sys/panic/panic.mod", "/apps/panic.mod", "always"),
    ("packages/sys/recovery/recovery.mod", "/apps/recovery.mod", "always"),
    ("packages/sys/calib/calib.mod", "/apps/calib.mod", "always"),
    ("packages/sys/storage/storage.mod", "/apps/storage.mod", "always"),
    ("packages/sys/sysinfo/sysinfo.krx", "/apps/sysinfo.krx", "always"),
]


def _sha256_file(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as fh:
        while True:
            chunk = fh.read(65536)
            if not chunk:
                break
            h.update(chunk)
    return h.hexdigest()


def _infer_channel(version: str) -> str:
    low = version.lower()
    if "dev" in low or "nightly" in low:
        return "nightly"
    if any(k in low for k in ("-alpha", "-beta", "-rc")):
        return "beta"
    return "stable"


def _run_checked(cmd: list[str], label: str) -> None:
    cli_ui.info(f"{label}: {' '.join(cmd)}")
    res = subprocess.run(cmd, cwd=str(REPO_ROOT), env=cli_ui.cargo_env())
    if res.returncode != 0:
        raise RuntimeError(f"{label} failed (exit {res.returncode})")


def _resolve_lms_key(args: argparse.Namespace) -> tuple[Path | None, bool]:
    """Return `(key_path, is_temp_file)` from `--lms-key`, `KERN_LMS_KEY`, or `KERN_LMS_KEY_JSON`."""
    if args.lms_key:
        return Path(args.lms_key), False
    env_path = os.environ.get("KERN_LMS_KEY")
    if env_path:
        return Path(env_path), False
    env_json = os.environ.get("KERN_LMS_KEY_JSON")
    if env_json:
        fd, tmp = tempfile.mkstemp(prefix="kern-lms-", suffix=".json")
        os.close(fd)
        Path(tmp).write_text(env_json, encoding="utf-8")
        return Path(tmp), True
    return None, False


def _kfw_asset_meta(kfw_path: Path, rel_name: str) -> dict[str, Any]:
    raw = kfw_path.read_bytes()
    p = kfw.parse_kfw(raw, verify_root=True)
    sigs = []
    for tag, val in p.auth_tlvs:
        if tag == kfw.TLV_AUTH_LMS_ASCON:
            sigs.append({
                "alg": "lms-ascon",
                "key_id": val[0:4].hex(),
                "signer": val[4:8].rstrip(b"\x00").decode("ascii", errors="replace"),
            })
        elif tag == kfw.TLV_AUTH_ASCON_MAC:
            sigs.append({
                "alg": "ascon-128-mac",
                "key_id": val[0:4].hex(),
                "signer": val[4:8].rstrip(b"\x00").decode("ascii", errors="replace"),
            })
    return {
        "filename": rel_name,
        "kind": kfw.KIND_NAMES.get(p.kind, str(p.kind)).lower(),
        "mcu_ota": bool(p.flags & kfw.HFLAG_MCU_OTA),
        "targets": [t.slug for t in p.targets],
        "size": len(raw),
        "sha256": _sha256_file(kfw_path),
        "ascon_root": p.root_digest.hex(),
        "signatures": sigs,
    }


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description="Kern Signed Release Pipeline (./kern release)")
    ap.add_argument("--version", "-v", required=True, help="Release version label (e.g. v0.2.0 or 0.2.0-rc.1)")
    ap.add_argument("--sec-cnt", type=int, default=1, help="Monotonic anti-rollback security counter (default: 1)")
    ap.add_argument("--channel", choices=["stable", "beta", "nightly"], help="Release channel (default: inferred from version)")
    ap.add_argument("--out", "-o", default="dist", help="Output distribution directory (default: dist)")
    ap.add_argument("--targets", help="Comma-separated target subset (uv-k1,uv-k10pro)")
    ap.add_argument("--notes", help="Path to Markdown release notes to embed in universal .kfw")
    ap.add_argument("--lms-key", help="LMS-Ascon private key JSON (or env KERN_LMS_KEY / KERN_LMS_KEY_JSON)")
    ap.add_argument("--mac-key", help="16-byte hex key/file for Ascon-128-MAC (or env KERN_MAC_KEY)")
    ap.add_argument("--signer", default="krel", help="4-char ASCII signer tag (default: krel)")
    ap.add_argument("--trust-store", help="Trust store JSON to verify produced signatures (default: keys/trusted-keys.json if present)")
    ap.add_argument("--require-signed", action="store_true", help="Fail if no signing key is provided or signature fails trust store")
    ap.add_argument("--skip-tests", action="store_true", help="Skip workspace unit tests & ABI sync check")
    ap.add_argument("--skip-build", action="store_true", help="Skip firmware/package compilation (reuse existing target/ & packages/)")
    ap.add_argument("--json", action="store_true", help="Print final release-manifest.json to stdout")
    args = ap.parse_args(argv)

    ver_clean = args.version.strip().lstrip("v")
    label = f"v{ver_clean}"
    channel = args.channel or _infer_channel(ver_clean)
    krx_abi = kfw.detect_workspace_abi(REPO_ROOT)
    out_dir = Path(args.out)
    out_dir.mkdir(parents=True, exist_ok=True)
    (out_dir / "modules").mkdir(exist_ok=True)
    (out_dir / "tools").mkdir(exist_ok=True)

    wanted = {t.strip().lower() for t in args.targets.split(",")} if args.targets else None
    targets = [t for t in RELEASE_TARGETS if not wanted or t[1] in wanted or t[0].lower() in wanted]
    if not targets:
        cli_ui.err("No matching release targets selected.")
        return 1

    lms_key_path, lms_is_tmp = _resolve_lms_key(args)
    mac_key_arg = args.mac_key or os.environ.get("KERN_MAC_KEY")
    if args.require_signed and not (lms_key_path or mac_key_arg):
        cli_ui.err("--require-signed set, but no signing key configured (--lms-key / KERN_LMS_KEY_JSON / --mac-key).")
        return 1

    cli_ui.header(
        f"Kern Release Pipeline — {label}",
        f"Channel: {channel} · sec_cnt={args.sec_cnt} · KRX-ABI={krx_abi} · Signed={'YES' if (lms_key_path or mac_key_arg) else 'NO (unsigned staging)'}",
    )

    try:
        # 1. Pre-flight Verification
        if not args.skip_tests:
            _run_checked([sys.executable, str(TOOLS_DIR / "kern_cli.py"), "abi", "--check"], "Verify ABI header sync")
            _run_checked([sys.executable, str(TOOLS_DIR / "kfw.py"), "selftest"], "Run .kfw crypto/anti-brick selftest")
            _run_checked([sys.executable, str(TOOLS_DIR / "rigwire.py"), "test"], "Run RigWire protocol selftest")
            _run_checked([sys.executable, str(TOOLS_DIR / "kern_cli.py"), "test"], "Run Rust workspace unit tests")

        # 2. Build Multi-Target Firmwares & Core/Sys KRX Packages
        built_fws: list[tuple[str, str, Path, Path]] = []
        for cli_tgt, kfw_slug, art_slug in targets:
            fw_bin = out_dir / f"kern-{art_slug}-{label}.bin"
            fw_packed = out_dir / f"kern-{art_slug}-{label}-packed.bin"
            size_json = out_dir / f"kern-{art_slug}-{label}-size.json"
            if not args.skip_build:
                _run_checked(
                    [
                        sys.executable, str(TOOLS_DIR / "builder.py"),
                        "--target", cli_tgt,
                        "--deny-warnings",
                        "--version", ver_clean,
                        "--sec-cnt", str(args.sec_cnt),
                        "--json", str(size_json),
                    ],
                    f"Build firmware ({cli_tgt})",
                )
            src_bin = REPO_ROOT / "target" / "firmware.bin"
            src_packed = REPO_ROOT / "target" / "firmware_packed.bin"
            if not src_bin.is_file():
                raise RuntimeError(f"Missing built firmware binary: {src_bin}")
            shutil.copy2(src_bin, fw_bin)
            if src_packed.is_file():
                shutil.copy2(src_packed, fw_packed)
            built_fws.append((kfw_slug, art_slug, fw_bin, size_json))

        if not args.skip_build:
            _run_checked([sys.executable, str(TOOLS_DIR / "pkg.py"), "all"], "Build all KRX packages")

        mod_specs: list[str] = []
        mod_manifest_entries: list[dict[str, Any]] = []
        for rel_src, dest_path, policy in RELEASE_MODULES:
            src_p = REPO_ROOT / rel_src
            if not src_p.is_file():
                raise RuntimeError(f"Missing release module: {src_p}")
            dst_mod = out_dir / "modules" / src_p.name
            shutil.copy2(src_p, dst_mod)
            mod_specs.append(f"{src_p}:{dest_path}:{policy}")
            mod_manifest_entries.append({
                "path": dest_path,
                "filename": f"modules/{src_p.name}",
                "policy": policy,
                "size": dst_mod.stat().st_size,
                "sha256": _sha256_file(dst_mod),
            })

        for tool_name in ("kfw.py", "rigwire.py", "flasher.py", "cli_ui.py"):
            src_t = TOOLS_DIR / tool_name
            if src_t.is_file():
                shutil.copy2(src_t, out_dir / "tools" / tool_name)

        # 3. Pack & Sign Universal Solid Bundle + Per-Target MCU-OTA Containers
        sign_flags: list[str] = ["--signer", args.signer]
        if lms_key_path:
            sign_flags.extend(["--lms-key", str(lms_key_path)])
        if mac_key_arg:
            sign_flags.extend(["--mac-key", mac_key_arg])

        verify_flags: list[str] = []
        ts_path = Path(args.trust_store) if args.trust_store else kfw.default_trust_store_path(REPO_ROOT)
        trust_store_obj: dict[str, Any] | None = None
        if ts_path.is_file():
            trust_store_obj = kfw.load_trust_store(ts_path)
            if lms_key_path and not trust_store_obj.get("keys"):
                trust_store_obj = None
        if trust_store_obj is not None:
            verify_flags.extend(["--trust-store", str(ts_path), "--channel", channel])
        elif lms_key_path:
            kobj_tmp = json.loads(lms_key_path.read_text(encoding="utf-8"))
            verify_flags.extend(["--lms-pubkey", str(kobj_tmp["pubkey_hex"])])
        if mac_key_arg:
            verify_flags.extend(["--mac-key", mac_key_arg])
        if args.require_signed or lms_key_path or mac_key_arg:
            verify_flags.append("--require-auth")

        kfw_outputs: list[Path] = []

        # 3a. Universal Solid Bundle (All Targets + All Core/Sys Modules)
        univ_kfw = out_dir / f"kern-{label}.kfw"
        pack_univ = [
            sys.executable, str(TOOLS_DIR / "kfw.py"), "pack", str(univ_kfw),
            "--name", "Kern OS",
            "--version", ver_clean,
            "--sec-cnt", str(args.sec_cnt),
            "--krx-abi", krx_abi,
        ]
        for kfw_slug, _art_slug, fw_bin, _sz in built_fws:
            pack_univ.extend(["--fw", f"{kfw_slug}:{fw_bin}"])
        for ms in mod_specs:
            pack_univ.extend(["--mod", ms])
        if args.notes:
            pack_univ.extend(["--notes", args.notes])
        pack_univ.extend(sign_flags)
        _run_checked(pack_univ, f"Pack universal bundle {univ_kfw.name}")
        _run_checked([sys.executable, str(TOOLS_DIR / "kfw.py"), "verify", str(univ_kfw), *verify_flags], f"Verify {univ_kfw.name}")
        kfw_outputs.append(univ_kfw)

        # 3b. Per-Target MCU-OTA Streamable Containers (<= 4 KB RAM on-device recovery OTA)
        for kfw_slug, art_slug, fw_bin, _sz in built_fws:
            ota_kfw = out_dir / f"kern-{art_slug}-{label}-ota.kfw"
            pack_ota = [
                sys.executable, str(TOOLS_DIR / "kfw.py"), "pack", str(ota_kfw),
                "--kind", "firmware", "--mcu-ota",
                "--name", "Kern OS",
                "--version", ver_clean,
                "--sec-cnt", str(args.sec_cnt),
                "--krx-abi", krx_abi,
                "--fw", f"{kfw_slug}:{fw_bin}",
                *sign_flags,
            ]
            _run_checked(pack_ota, f"Pack MCU-OTA {ota_kfw.name}")
            _run_checked([sys.executable, str(TOOLS_DIR / "kfw.py"), "verify", str(ota_kfw), *verify_flags], f"Verify {ota_kfw.name}")
            kfw_outputs.append(ota_kfw)

        # 4. Copy Trust Store (if present) & Emit Canonical release-manifest.json (+ .sig)
        if ts_path.is_file():
            shutil.copy2(ts_path, out_dir / "trusted-keys.json")

        manifest_obj: dict[str, Any] = {
            "schema": 1,
            "name": "Kern OS",
            "version": ver_clean,
            "tag": label,
            "channel": channel,
            "sec_cnt": args.sec_cnt,
            "krx_abi": krx_abi,
            "git_commit": kfw._git_short_commit(),
            "published_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
            "signed": bool(lms_key_path or mac_key_arg),
            "assets": [_kfw_asset_meta(p, p.name) for p in kfw_outputs],
            "modules": mod_manifest_entries,
        }
        man_bytes = (json.dumps(manifest_obj, indent=2, sort_keys=True) + "\n").encode("utf-8")
        man_path = out_dir / "release-manifest.json"
        man_path.write_bytes(man_bytes)

        if lms_key_path or mac_key_arg:
            lms_obj = json.loads(lms_key_path.read_text(encoding="utf-8")) if lms_key_path else None
            mac_bytes = kfw._parse_hex_key(mac_key_arg, 16) if mac_key_arg else None
            sig_env = kfw.sign_manifest_bytes(man_bytes, lms_key_obj=lms_obj, mac_key=mac_bytes, signer=args.signer)
            if lms_obj is not None and lms_key_path is not None and not lms_is_tmp:
                kfw.save_lms_key_atomic(lms_key_path, lms_obj)
            sig_path = out_dir / "release-manifest.json.sig"
            sig_path.write_text(json.dumps(sig_env, indent=2, sort_keys=True) + "\n", encoding="utf-8")
            # Immediately self-verify the signed manifest
            kfw.verify_manifest_bytes(
                man_bytes,
                sig_env,
                lms_pubkey=bytes.fromhex(lms_obj["pubkey_hex"]) if lms_obj else None,
                mac_key=mac_bytes,
                trust_store=trust_store_obj,
                channel=channel,
                require_auth=True,
            )
            cli_ui.ok(f"Signed & verified {sig_path.name}")

        # 5. Emit SHA256SUMS.txt across all release files
        sum_lines: list[str] = []
        for p in sorted(out_dir.rglob("*")):
            if p.is_file() and p.name != "SHA256SUMS.txt":
                rel = p.relative_to(out_dir).as_posix()
                sum_lines.append(f"{_sha256_file(p)}  {rel}")
        (out_dir / "SHA256SUMS.txt").write_text("\n".join(sum_lines) + "\n", encoding="utf-8")

        if args.json:
            print(man_bytes.decode("utf-8"))
        cli_ui.ok(f"Release {label} complete in {out_dir}/ ({len(kfw_outputs)} .kfw containers, {len(mod_manifest_entries)} modules).")
        return 0
    finally:
        if lms_is_tmp and lms_key_path is not None:
            lms_key_path.unlink(missing_ok=True)


if __name__ == "__main__":
    raise SystemExit(main())
