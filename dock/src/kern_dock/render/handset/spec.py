"""Declarative HandsetSpec schema for authentic, distinct Quansheng radio chassis blueprints."""

from __future__ import annotations

import dataclasses

RGB = tuple[float, float, float]


@dataclasses.dataclass(frozen=True)
class HandsetSpec:
    """Declarative hardware mockup blueprint consumed by HandsetRenderer."""

    id: str
    label: str
    chassis: str                              # "k1-metal" | "k1-poly" | "k5v3" | "k5-orig" | "uv5r-plus"
    palette: str                              # Default LCD_PALETTES key
    accent: RGB                               # Primary hardware accent (knob / PTT / pressed ring)
    armor_stops: tuple[tuple[float, RGB], ...]
    body_tone: RGB
    size: tuple[int, int]                     # True-to-hardware logical canvas (W, H)
    antenna_label: str = "A1"
    tagline: str = "UHF/VHF Transceiver"
    description: str = ""
