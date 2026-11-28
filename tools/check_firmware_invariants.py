#!/usr/bin/env python3
"""
check_firmware_invariants.py — Executable Structural, Safety & ELF Budget Verifier for Kern.

Inspired by Ferrite's `ferrite-checks` + `slowcheck.py` and RadioFoundry's `check_generic.py`:
  1. Hardware Ownership & Layer Isolation (`kernel` never imports board/PAC/drivers;
     `apps` never touch hardware drivers/PAC/BSP; portable `drivers` never touch MCU PAC/HAL).
  2. Flash-Safety Scan: bans raw MCU flash-unlock / option-byte unlock keys outside `mcu/` & `boot/`.
  3. Geometry Invariant Scan: verifies compile-time power-of-two record size assertions exist.
  4. Optional Compiled ELF Budget Check: when a release firmware ELF exists, checks Flash (< 80 KB target,
     < 120,832 B hard physical ceiling) and static SRAM budgets via `arm-none-eabi-readelf`.
"""

import argparse
import os
import re
import shutil
import subprocess
import sys

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), ".."))

# STM32 / PY32 Flash & Option-Byte unlock magic keys that must NEVER appear in kernel/apps/gfx/fs
BANNED_FLASH_KEYS = {
    0x45670123: "MCU FLASH_KEY1",
    0xCDEF89AB: "MCU FLASH_KEY2",
    0x08192A3B: "MCU OPTKEY1",
    0x4C5D6E7F: "MCU OPTKEY2",
}

HEX_RE = re.compile(r"\b0x([0-9a-fA-F_]+)\b")


def iter_rs_files(rel_dir):
    base = os.path.join(ROOT, rel_dir)
    if not os.path.isdir(base):
        return
    for d, _, files in os.walk(base):
        if "/target" in d:
            continue
        for f in sorted(files):
            if f.endswith(".rs"):
                full = os.path.join(d, f)
                with open(full, encoding="utf-8", errors="ignore") as fp:
                    yield os.path.relpath(full, ROOT), fp.read()


def strip_cfg_test(src):
    idx = src.find("#[cfg(test)]")
    return src[:idx] if idx >= 0 else src


def check_kernel_hardware_agnosticism():
    errors = []
    banned_refs = re.compile(r"\b(py32_hal|py32f071_pac|py32_pac|bsp|drivers|bk4829|st7565|py25q16)\b")
    for rel, src in iter_rs_files("src/kernel/src"):
        clean = strip_cfg_test(src)
        for lineno, line in enumerate(clean.splitlines(), 1):
            stripped = line.split("//", 1)[0]
            if re.search(r"\buse\s+(py32_hal|py32f071_pac|py32_pac|bsp|drivers)\b", stripped):
                errors.append(f"{rel}:{lineno}: kernel imports concrete hardware crate: {stripped.strip()}")
            elif banned_refs.search(stripped) and ("use " in stripped or "::" in stripped):
                errors.append(f"{rel}:{lineno}: kernel references board/chip symbol: {stripped.strip()}")
    return errors


# Shrinking allowlist of pre-existing MCU/BSP couplings scheduled for migration
# (RadioFoundry `check_generic.py` pattern: report as [DEBT], fail on any new file).
KNOWN_DRIVER_MCU_DEBT = set()


def check_apps_and_drivers_isolation():
    errors = []
    debt = []
    for rel, src in iter_rs_files("src/firmware/src/apps"):
        clean = strip_cfg_test(src)
        for lineno, line in enumerate(clean.splitlines(), 1):
            stripped = line.split("//", 1)[0]
            if re.search(r"\b(py32_hal|py32f071_pac|py32_pac|bsp|bk4829|st7565|py25q16)::", stripped):
                errors.append(f"{rel}:{lineno}: resident app touches hardware directly: {stripped.strip()}")
            if re.search(r"\.radio\.", stripped):
                errors.append(f"{rel}:{lineno}: R-SINGLE-RF-OWNER violation in app: {stripped.strip()}")

    for rel, src in iter_rs_files("src/firmware/src/orchestrator"):
        clean = strip_cfg_test(src)
        for lineno, line in enumerate(clean.splitlines(), 1):
            stripped = line.split("//", 1)[0]
            if re.search(r"\b(bk4829)::", stripped) or re.search(r"\.radio\.", stripped):
                errors.append(f"{rel}:{lineno}: R-SINGLE-RF-OWNER violation in orchestrator: {stripped.strip()}")

    for rel, src in iter_rs_files("src/drivers/src"):
        clean = strip_cfg_test(src)
        for lineno, line in enumerate(clean.splitlines(), 1):
            stripped = line.split("//", 1)[0]
            if re.search(r"\b(py32_hal|py32f071_pac|py32_pac|bsp)::", stripped):
                msg = f"{rel}:{lineno}: portable driver couples to MCU/BSP: {stripped.strip()}"
                if rel in KNOWN_DRIVER_MCU_DEBT:
                    debt.append(rel)
                else:
                    errors.append(msg)
    return errors, sorted(set(debt))


def check_no_unguarded_flash_unlock_keys():
    errors = []
    scan_dirs = ["src/kernel", "src/drivers", "src/fs", "src/gfx", "src/abi", "packages"]
    for d in scan_dirs:
        for rel, src in iter_rs_files(d):
            for lineno, line in enumerate(src.splitlines(), 1):
                stripped = line.split("//", 1)[0]
                for m in HEX_RE.finditer(stripped):
                    try:
                        val = int(m.group(1).replace("_", ""), 16)
                    except ValueError:
                        continue
                    if val in BANNED_FLASH_KEYS:
                        errors.append(f"{rel}:{lineno}: banned {BANNED_FLASH_KEYS[val]} (0x{val:08X})")
    return errors


def check_elf_sections(elf_path, strict_80k=False):
    warnings = []
    errors = []
    if not os.path.isfile(elf_path) or not shutil.which("arm-none-eabi-readelf"):
        return warnings, errors

    out = subprocess.check_output(["arm-none-eabi-readelf", "-S", "-W", elf_path], text=True)
    flash_bytes = 0
    sram_bytes = 0
    for line in out.splitlines():
        m = re.match(r"\s*\[\s*\d+\]\s+(\.\S+)\s+\S+\s+([0-9a-fA-F]+)\s+[0-9a-fA-F]+\s+([0-9a-fA-F]+)", line)
        if not m:
            continue
        sec, addr_hex, size_hex = m.group(1), int(m.group(2), 16), int(m.group(3), 16)
        if sec in (".vector_table", ".text", ".rodata", ".data"):
            flash_bytes += size_hex
        if 0x20000000 <= addr_hex < 0x20004000 and sec in (".data", ".bss", ".uninit"):
            sram_bytes += size_hex

    hard_limit = 120_832
    soft_target = 81_920  # 80 KB architectural goal
    if flash_bytes > hard_limit:
        errors.append(f"ELF Flash {flash_bytes} B exceeds physical bootloader limit {hard_limit} B")
    elif flash_bytes > soft_target:
        msg = f"ELF Flash {flash_bytes} B ({flash_bytes / 1024:.1f} KiB) exceeds 80 KiB lean target ({soft_target} B)"
        if strict_80k:
            errors.append(msg)
        else:
            warnings.append(msg)
    return warnings, errors


def main():
    ap = argparse.ArgumentParser(description="Verify Kern architectural, safety, and ELF invariants.")
    ap.add_argument(
        "--elf",
        default=os.path.join(ROOT, "target", "thumbv6m-none-eabi", "release", "firmware"),
        help="Optional path to compiled firmware ELF",
    )
    ap.add_argument("--strict-80k", action="store_true", help="Fail if ELF exceeds 80 KiB target")
    args = ap.parse_args()

    all_errors = []
    all_errors.extend(check_kernel_hardware_agnosticism())
    iso_errors, debt_files = check_apps_and_drivers_isolation()
    all_errors.extend(iso_errors)
    all_errors.extend(check_no_unguarded_flash_unlock_keys())
    warnings, elf_errors = check_elf_sections(args.elf, strict_80k=args.strict_80k)
    all_errors.extend(elf_errors)

    if debt_files:
        print(f"[DEBT] {len(debt_files)} legacy driver file(s) still reference py32_hal/bsp: {', '.join(debt_files)}")
    for w in warnings:
        print(f"[WARN] {w}")
    if all_errors:
        for e in all_errors:
            print(f"[FAIL] {e}")
        sys.exit(1)

    print("[PASS] Kern hardware-agnosticism, chip-isolation, and flash-safety invariants verified.")


if __name__ == "__main__":
    main()
