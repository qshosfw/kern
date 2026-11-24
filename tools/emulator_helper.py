#!/usr/bin/env python3
"""
Legacy compatibility wrapper for `tools/emu.py` (`./kern emu`).

All primary emulation, Dock attachment, agent automation, and E2E testing
now live in `tools/emu.py`. Pass `--webui` only if you specifically want the
upstream Flask web UI from `inspo/uv-k5-v3-emulator`.
"""

from __future__ import annotations

import subprocess
import sys
from pathlib import Path

TOOLS_DIR = Path(__file__).resolve().parent
if str(TOOLS_DIR) not in sys.path:
    sys.path.insert(0, str(TOOLS_DIR))

import emu  # noqa: E402


def main(argv: list[str] | None = None) -> int:
    raw = list(sys.argv[1:] if argv is None else argv)
    if "--webui" in raw:
        raw.remove("--webui")
        webui = emu.DEFAULT_EMU_DIR / "tools" / "webui.py"
        if not webui.is_file():
            sys.stderr.write(f"Legacy webui.py not found at {webui}\n")
            return 1
        elf = emu.ensure_firmware_elf()
        flash = emu.ensure_flash_image()
        return subprocess.call(
            [sys.executable, str(webui), "--elf", str(elf), "--flash", str(flash), *raw],
            cwd=str(emu.DEFAULT_EMU_DIR),
        )
    if raw and raw[0] == "run":
        raw[0] = "start"
    return emu.main(raw)


if __name__ == "__main__":
    sys.exit(main())
