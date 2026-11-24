#!/usr/bin/env python3
"""
Kern End-to-End Hardware Simulation Test (Direct QEMU + RigWire, zero Flask).

Uses `KernEmulator` from `tools/emu.py` on an ephemeral copy of `flash.img` to verify:
1. Cold boot of Kern firmware on Cortex-M0+ SoC (ST7565 LCD, SPI flash, BK4829).
2. RigWire HELLO, PING, telemetry, and KernFS directory listings.
3. ST7565 LCD framebuffer capture and non-blank rendering.
4. Interactive keypad injection (Launcher -> Radio VFO -> step frequency -> Settings -> back).
5. Rapid multi-cycle app switching health check.
"""

from __future__ import annotations

import sys
import time
from pathlib import Path

TOOLS_DIR = Path(__file__).resolve().parent
if str(TOOLS_DIR) not in sys.path:
    sys.path.insert(0, str(TOOLS_DIR))

from emu import KernEmulator, encode_fb_png  # noqa: E402


def run_test() -> int:
    print("=== Kern Hardware Emulator E2E Test ===")
    with KernEmulator(
        ephemeral_flash=True,
        serial_port=4457,
        gdb_port=1245,
        qmp_path="/tmp/kern-emu-e2e.sock",
        state_file=Path("/tmp/kern-emu-e2e.json"),
    ) as emu:
        # 1. RigWire handshake & telemetry
        hello = emu.wire().hello()
        print(f"  PASS  RigWire HELLO: {hello}")
        emu.wire().ping()
        print("  PASS  RigWire PING OK")

        telem = emu.wire().get_system_telemetry()
        print(f"  PASS  System telemetry: uptime={telem.get('uptime_ms')} ms, batt={telem.get('battery_pct')}%")

        # 2. KernFS check
        root = [e["name"] for e in emu.ls("/")]
        print(f"  PASS  KernFS root entries: {root}")

        # 3. ST7565 LCD capture
        fb0 = emu.framebuffer()
        if len(fb0) != 1024 or not any(fb0):
            sys.exit("FAIL: Initial LCD framebuffer is empty")
        png = encode_fb_png(fb0)
        if not png.startswith(b"\x89PNG\r\n\x1a\n"):
            sys.exit("FAIL: Encoded PNG header invalid")
        print(f"  PASS  LCD frame captured ({sum(bin(b).count('1') for b in fb0)} lit pixels, {len(png)}B PNG)")

        # 4. Launch Radio VFO ('1') and step frequency ('up'), then return ('exit')
        emu.press("1", hold_ms=120)
        fb_radio = emu.wait_frame_change(fb0, timeout=4.0)
        if fb_radio is None:
            sys.exit("FAIL: LCD did not update after pressing '1'")
        print("  PASS  Radio VFO app launched")

        emu.press("up", hold_ms=120)
        fb_up = emu.wait_frame_change(fb_radio, timeout=3.0)
        if fb_up is None:
            sys.exit("FAIL: LCD did not update after pressing 'up'")
        print("  PASS  Frequency step updated LCD")

        emu.press("exit", hold_ms=120)
        fb_home = emu.wait_frame_change(fb_up, timeout=3.0)
        if fb_home is None:
            sys.exit("FAIL: LCD did not return to Launcher after 'exit'")
        print("  PASS  Returned to Launcher")

        # 5. Open Settings ('2') and return ('exit')
        emu.press("2", hold_ms=120)
        fb_settings = emu.wait_frame_change(fb_home, timeout=4.0)
        if fb_settings is None:
            sys.exit("FAIL: LCD did not update after pressing '2' (Settings)")
        print("  PASS  Settings app opened")

        emu.press("exit", hold_ms=120)
        time.sleep(0.3)

        # 6. Rapid navigation stress check
        for cycle in range(1, 4):
            emu.press("1", hold_ms=80)
            time.sleep(0.2)
            emu.press("exit", hold_ms=80)
            time.sleep(0.2)
            emu.wire().ping()
        print("  PASS  3x rapid app-cycle stability verified")

    print("All Kern emulator integration tests passed!")
    return 0


if __name__ == "__main__":
    sys.exit(run_test())
