#!/usr/bin/env python3
"""Compatibility entry point forwarding directly to the in-house `tools/emu.py`."""

from __future__ import annotations

import sys
from pathlib import Path

TOOLS_DIR = Path(__file__).resolve().parent
if str(TOOLS_DIR) not in sys.path:
    sys.path.insert(0, str(TOOLS_DIR))

import emu  # noqa: E402


def main(argv: list[str] | None = None) -> int:
    raw = list(sys.argv[1:] if argv is None else argv)
    if raw and raw[0] == "run":
        raw[0] = "start"
    return emu.main(raw)


if __name__ == "__main__":
    sys.exit(main())
