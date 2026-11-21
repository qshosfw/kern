"""Cairo geometry, color and procedural surface helpers shared across renderers."""

from __future__ import annotations

import math
from typing import Any


def clamp1(v: float) -> float:
    return 0.0 if v < 0.0 else (1.0 if v > 1.0 else v)


def round_rect(cr: Any, x: float, y: float, w: float, h: float, r: float) -> None:
    r = max(0.0, min(r, w * 0.5, h * 0.5))
    cr.new_sub_path()
    cr.arc(x + w - r, y + r, r, -math.pi / 2, 0)
    cr.arc(x + w - r, y + h - r, r, 0, math.pi / 2)
    cr.arc(x + r, y + h - r, r, math.pi / 2, math.pi)
    cr.arc(x + r, y + r, r, math.pi, 3 * math.pi / 2)
    cr.close_path()


def paint_brushed_metal_grain(cr: Any, x: float, y: float, w: float, h: float, *, vertical: bool = False, alpha: float = 0.032) -> None:
    """Subtle procedural micro-brush hairline texture clipped to the active path."""
    cr.save()
    cr.set_line_width(0.65)
    if vertical:
        step = 2.5
        pos = x
        i = 0
        while pos < x + w:
            bright = 1.0 if (i % 3 == 0) else 0.0
            a = alpha * (0.85 if (i % 5 == 0) else 0.45)
            cr.set_source_rgba(bright, bright, bright, a)
            cr.move_to(pos, y)
            cr.line_to(pos, y + h)
            cr.stroke()
            pos += step
            i += 1
    else:
        step = 2.4
        pos = y
        i = 0
        while pos < y + h:
            bright = 1.0 if (i % 3 == 0) else 0.0
            a = alpha * (0.85 if (i % 5 == 0) else 0.45)
            cr.set_source_rgba(bright, bright, bright, a)
            cr.move_to(x, pos)
            cr.line_to(x + w, pos)
            cr.stroke()
            pos += step
            i += 1
    cr.restore()
