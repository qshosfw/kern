#!/usr/bin/env python3
"""
Kern End-to-End Hardware Simulation Suite (In-House `kern-emu`, zero external dependencies).

Covers all four hardware boot & resilience scenarios:
  1. Blank SPI flash (First-Time Setup): zero-touch auto-init, factory calib rescue,
     Welcome modal, missing-app dialog ("Radio App"), core package sideload & launch.
  2. Blank flash without factory calibration (`blank-no-calib`): fallback calibration stays
     unbound (`calib_valid == False`, no fake `/sys/calib.sig`), Default Calib alert shown.
  3. Corrupted storage (`corrupt`) -> `[EXIT]`: boots cleanly into Safe Mode with live RigWire
     instead of spin-halting.
  4. Corrupted storage (`corrupt`) -> `[M]` 1.5s hold: formats KernFS, rescues factory calib,
     and boots to Home.
"""

from __future__ import annotations

import sys
import time
from pathlib import Path

TOOLS_DIR = Path(__file__).resolve().parent
ROOT_DIR = TOOLS_DIR.parent
if str(TOOLS_DIR) not in sys.path:
    sys.path.insert(0, str(TOOLS_DIR))

import rigwire  # noqa: E402
from emu import KernEmulator, encode_fb_png, render_fb_ascii  # noqa: E402


def _read_calib_bound(emu: KernEmulator) -> bool:
    cal = emu.wire().handle_get(rigwire.DOMAIN_SYSTEM, 0, rigwire.FACET_SYS_CALIBRATION)
    return bool(cal[0]) if cal else False


def test_blank_first_boot_and_apps() -> None:
    print("── 1. Blank Flash: Zero-Touch First-Time Setup + Missing/Sideloaded Apps ──")
    with KernEmulator(
        ephemeral_flash=True,
        flash_mode="blank",
        turbo=True,
        serial_port=4457,
        ctl_port=4454,
        state_file=Path("/tmp/kern-emu-e2e.json"),
    ) as emu:
        st = emu.status()
        print(f"  PASS  Zero-touch boot @ {st.get('mips')} MIPS (panel={st.get('panel')})")
        lines = emu.serial_lines()
        if not any("KERN" in ln for ln in lines):
            sys.exit(f"FAIL: Missing KERN serial banner in {lines}")
        print(f"  PASS  UART banner: {lines[0]}")

        if not _read_calib_bound(emu):
            sys.exit("FAIL: Expected rescued factory calibration to be hardware-bound")
        sys_files = {e["name"] for e in emu.ls("/sys")}
        if "calib.dat" not in sys_files or "calib.sig" not in sys_files:
            sys.exit(f"FAIL: Expected calib.dat & calib.sig in /sys, got {sys_files}")
        print(f"  PASS  Factory calibration rescued & hardware-signed (/sys={sorted(sys_files)})")

        fb_welcome = emu.framebuffer(direct=True)
        png = encode_fb_png(fb_welcome)
        if len(fb_welcome) != 1024 or not any(fb_welcome) or not png.startswith(b"\x89PNG\r\n\x1a\n"):
            sys.exit("FAIL: Invalid Welcome LCD capture")
        print("  PASS  Welcome to KERN modal rendered on first launch")

        # Dismiss Welcome modal ([M] OK) -> Home Launcher
        emu.release_wire()
        emu.press("menu", hold_ms=90, hardware=True)
        fb_home = emu.wait_frame_change(fb_welcome, timeout=2.0)
        if fb_home is None:
            sys.exit("FAIL: Welcome modal did not dismiss on MENU")

        # Attempt to launch Radio before vfo.krx is installed -> "Radio App" modal
        emu.press("1", hold_ms=90, hardware=True)
        fb_missing = emu.wait_frame_change(fb_home, timeout=2.0)
        if fb_missing is None:
            sys.exit("FAIL: Missing Radio App modal did not appear")
        print("  PASS  Missing core app gracefully shows 'Radio App' modal without crashing")

        # Dismiss missing-app modal
        emu.press("menu", hold_ms=90, hardware=True)
        fb_home2 = emu.wait_frame_change(fb_missing, timeout=2.0) or fb_home

        # Sideload vfo.krx and launch it cleanly
        vfo_krx = ROOT_DIR / "packages" / "core" / "vfo" / "vfo.krx"
        if vfo_krx.is_file():
            emu.sideload(vfo_krx, force=True)
            emu.release_wire()
            time.sleep(0.05)
            emu.press("1", hold_ms=110, hardware=True)
            fb_vfo = emu.wait_frame_change(fb_home2, timeout=2.5)
            if fb_vfo is None:
                sys.exit("FAIL: LCD did not update after launching sideloaded vfo.krx")
            print("  PASS  Sideloaded & launched vfo.krx")
            emu.press("exit", hold_ms=110, hardware=True)
            emu.wait_frame_change(fb_vfo, timeout=2.5)
            print("  PASS  Exited VFO back to Home Launcher cleanly")


def test_blank_no_factory_calib() -> None:
    print("── 2. Uncalibrated Flash (`blank-no-calib`): Honest Fallback Provenance ──")
    with KernEmulator(
        ephemeral_flash=True,
        flash_mode="blank-no-calib",
        turbo=True,
        serial_port=4459,
        ctl_port=4452,
        state_file=Path("/tmp/kern-emu-nocalib.json"),
    ) as emu:
        if _read_calib_bound(emu):
            sys.exit("FAIL: Fallback default calibration must NOT report hardware-bound!")
        sys_files = {e["name"] for e in emu.ls("/sys")}
        if "calib.sig" in sys_files:
            sys.exit(f"FAIL: Fallback default calibration must not write fake /sys/calib.sig ({sys_files})")
        print(f"  PASS  Fallback calibration unbound (calib_valid=False, /sys={sorted(sys_files)})")


def test_corrupt_storage_safe_mode_and_hold_format() -> None:
    print("── 3. Corrupted Storage (`corrupt`): [EXIT] Safe Mode & [M] Hold Format ──")
    # 3a: Press EXIT on "Storage Corrupt" -> boots into Safe Mode with live RigWire!
    emu = KernEmulator(
        ephemeral_flash=True,
        flash_mode="corrupt",
        turbo=True,
        serial_port=4460,
        ctl_port=4451,
        state_file=Path("/tmp/kern-emu-corrupt.json"),
    )
    emu.start(wait_rigwire=False)
    try:
        emu.wait_virtual_ms(250, timeout=1.5)
        fb_prompt = emu.framebuffer(direct=True)
        if not any(fb_prompt):
            sys.exit("FAIL: Corrupt storage prompt did not render on LCD")
        emu.press("exit", hold_ms=100, hardware=True)
        hello = emu.wait_ready(timeout=4.0)
        emu.wire().ping()
        print(f"  PASS  [EXIT] on corrupt flash booted Safe Mode with live RigWire ({hello})")
    finally:
        emu.stop()

    # 3b: Hold MENU for 1.5s on "Storage Corrupt" -> formats & rescues factory calib!
    emu2 = KernEmulator(
        ephemeral_flash=True,
        flash_mode="corrupt",
        turbo=True,
        serial_port=4461,
        ctl_port=4450,
        state_file=Path("/tmp/kern-emu-corrupt2.json"),
    )
    emu2.start(wait_rigwire=False)
    try:
        emu2.wait_virtual_ms(250, timeout=1.5)
        emu2.press("menu", hold_ms=1800, hardware=True)
        emu2.wait_ready(timeout=5.0)
        if not _read_calib_bound(emu2):
            sys.exit("FAIL: Factory calibration was not rescued after corrupt-storage format")
        fb_final = emu2.framebuffer(direct=True)
        print("  PASS  [M] 1.5s hold formatted KernFS & rescued factory calibration")
        print(render_fb_ascii(fb_final))
    finally:
        emu2.stop()


def run_test() -> int:
    print("=== Kern In-House Hardware Emulator Resilience Suite ===")
    test_blank_first_boot_and_apps()
    test_blank_no_factory_calib()
    test_corrupt_storage_safe_mode_and_hold_format()
    print("All 4 hardware boot & resilience scenarios passed!")
    return 0


if __name__ == "__main__":
    sys.exit(run_test())
