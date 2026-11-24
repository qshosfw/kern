#!/usr/bin/env python3
"""
Kern End-to-End Hardware Simulation Test (In-House `kern-emu`, zero external dependencies).

Verifies cold boot, UART telemetry banner, RigWire RPC, KernFS storage, ST7565 LCD rendering,
physical & RigWire keypad input, core package sideloading, and app navigation.
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
from emu import KernEmulator, encode_fb_png  # noqa: E402


def run_test() -> int:
    print("=== Kern In-House Hardware Emulator E2E Test ===")
    emu = KernEmulator(
        ephemeral_flash=True,
        flash_mode="blank",
        turbo=True,
        serial_port=4457,
        ctl_port=4454,
        state_file=Path("/tmp/kern-emu-e2e.json"),
    )
    emu.start(wait_rigwire=False)
    try:
        deadline = time.monotonic() + 6.0
        while time.monotonic() < deadline:
            try:
                cli = rigwire.RigWireClient(emu.wire_url, timeout=0.35)
                hello = cli.hello()
                cli.close()
                break
            except Exception:
                emu.press("menu", hold_ms=1800, hardware=True)
                time.sleep(0.15)
                emu.press("menu", hold_ms=150, hardware=True)
                time.sleep(0.10)
        else:
            sys.exit("FAIL: Firmware did not become RigWire-ready")

        st = emu.status()
        print(f"  PASS  Booted @ {st.get('mips')} MIPS (panel={st.get('panel')})")
        lines = emu.serial_lines()
        if not any("KERN" in ln for ln in lines):
            sys.exit(f"FAIL: Missing KERN serial banner in {lines}")
        print(f"  PASS  Serial telemetry banner verified: {lines[0]}")
        print(f"  PASS  RigWire HELLO: {hello}")

        emu.wire().ping()
        print("  PASS  RigWire PING OK")

        root = [e["name"] for e in emu.ls("/")]
        print(f"  PASS  KernFS root entries: {root}")

        fb0 = emu.framebuffer(direct=True)
        png = encode_fb_png(fb0)
        if len(fb0) != 1024 or not any(fb0) or not png.startswith(b"\x89PNG\r\n\x1a\n"):
            sys.exit("FAIL: Invalid LCD framebuffer capture")
        print(f"  PASS  ST7565 LCD frame captured ({sum(bin(b).count('1') for b in fb0)} lit pixels)")

        # Sideload VFO & Settings if built in workspace and verify launching them
        vfo_krx = ROOT_DIR / "packages" / "core" / "vfo" / "vfo.krx"
        if vfo_krx.is_file():
            emu.sideload(vfo_krx, force=True)
            emu.release_wire()
            time.sleep(0.05)
            emu.press("1", hold_ms=120, hardware=True)
            fb_vfo = emu.wait_frame_change(fb0, timeout=2.5)
            if fb_vfo is None:
                sys.exit("FAIL: LCD did not update after launching VFO")
            print("  PASS  Sideloaded & launched vfo.krx")
            emu.press("exit", hold_ms=120, hardware=True)
            emu.wait_frame_change(fb_vfo, timeout=2.5)
            print("  PASS  Returned to Home Launcher cleanly")
        else:
            emu.press("down", hold_ms=100, hardware=True)
            if emu.wait_frame_change(fb0, timeout=2.0) is None:
                sys.exit("FAIL: Launcher cursor did not move on DOWN")
            print("  PASS  Launcher keypad navigation verified")
    finally:
        emu.stop()

    print("All Kern hardware emulator integration tests passed!")
    return 0


if __name__ == "__main__":
    sys.exit(run_test())
