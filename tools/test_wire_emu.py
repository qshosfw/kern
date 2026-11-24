#!/usr/bin/env python3
"""RigWire + KernFS + Sideload integration test on the streamlined Kern QEMU emulator."""

from __future__ import annotations

import sys
import time
from pathlib import Path

TOOLS_DIR = Path(__file__).resolve().parent
ROOT_DIR = TOOLS_DIR.parent
if str(TOOLS_DIR) not in sys.path:
    sys.path.insert(0, str(TOOLS_DIR))

from emu import KernEmulator, render_fb_ascii  # noqa: E402


def main() -> int:
    print("=== Kern RigWire Emulator Integration Test ===")
    with KernEmulator(
        ephemeral_flash=True,
        serial_port=4458,
        gdb_port=1246,
        qmp_path="/tmp/kern-emu-wire.sock",
        state_file=Path("/tmp/kern-emu-wire.json"),
    ) as emu:
        print("  • HELLO:", emu.wire().hello())
        print("  • PING:", emu.wire().ping())

        for directory in ("/", "/apps"):
            try:
                entries = emu.ls(directory)
                names = [f"{e['name']}{'/' if e['is_dir'] else f' ({e[\"size\"]}B)'}" for e in entries]
                print(f"  • ls {directory}: {names}")
            except Exception as exc:
                print(f"  • ls {directory} failed: {exc}")

        sysinfo_krx = ROOT_DIR / "packages" / "sys" / "sysinfo" / "sysinfo.krx"
        if sysinfo_krx.is_file():
            res = emu.sideload(sysinfo_krx, force=True)
            print("  • Sideloaded sysinfo.krx:", res)

        fb0 = emu.framebuffer()
        emu.screenshot("/tmp/frame_0_launcher.png")
        print(render_fb_ascii(fb0))

        emu.press("2", hold_ms=120)
        fb1 = emu.wait_frame_change(fb0, timeout=4.0)
        if fb1 is not None:
            emu.screenshot("/tmp/frame_1_settings.png")
            print("  • Settings screen updated and saved to /tmp/frame_1_settings.png")
        time.sleep(0.2)
    print("RigWire emulator test complete.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
