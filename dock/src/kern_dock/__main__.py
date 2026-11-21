"""Module execution entry point (`python3 -m kern_dock`)."""

from __future__ import annotations

from .cli import main

if __name__ == "__main__":
    raise SystemExit(main())
