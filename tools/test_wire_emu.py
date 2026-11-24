#!/usr/bin/env python3
"""RigWire + KernFS + Sideload integration test on the in-house `kern-emu` hardware emulator."""

from __future__ import annotations

import sys
import time
from pathlib import Path

TOOLS_DIR = Path(__file__).resolve().parent
ROOT_DIR = TOOLS_DIR.parent
if str(TOOLS_DIR) not in sys.path:
    sys.path.insert(0, str(TOOLS_DIR))

import rigwire  # noqa: E402
from emu import KernEmulator, render_fb_ascii  # noqa: E402


def main() -> int:
    print("=== Kern RigWire In-House Emulator Test ===")
    emu = KernEmulator(
        ephemeral_flash=True,
        flash_mode="blank",
        turbo=True,
        serial_port=4458,
        ctl_port=4453,
        state_file=Path("/tmp/kern-emu-wire.json"),
    )
    emu.start(wait_rigwire=False)
    try:
        deadline = time.monotonic() + 8.0
        while time.monotonic() < deadline:
            emu.wait_virtual_ms(250, timeout=1.0)
            try:
                cli = rigwire.RigWireClient(emu.wire_url, timeout=0.35)
                print("  • HELLO:", cli.hello())
                cli.close()
                break
            except Exception:
                emu.press("menu", hold_ms=1800, hardware=True)
                emu.wait_virtual_ms(300, timeout=1.5)
                emu.press("menu", hold_ms=150, hardware=True)
                emu.wait_virtual_ms(250, timeout=1.0)

        print("  • PING:", emu.wire().ping())
        for directory in ("/", "/apps"):
            entries = emu.ls(directory)
            names = [
                f"{e['name']}/" if e["is_dir"] else f"{e['name']} ({e['size']}B)"
                for e in entries
            ]
            print(f"  • ls {directory}: {names}")

        sysinfo_krx = ROOT_DIR / "packages" / "sys" / "sysinfo" / "sysinfo.krx"
        if sysinfo_krx.is_file():
            res = emu.sideload(sysinfo_krx, force=True)
            print("  • Sideloaded sysinfo.krx:", res)

        fb0 = emu.framebuffer(direct=True)
        print(render_fb_ascii(fb0))
    finally:
        emu.stop()
    print("RigWire emulator test complete.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
