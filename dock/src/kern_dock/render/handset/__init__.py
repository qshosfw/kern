"""Declarative handset subsystem."""

from __future__ import annotations

from .catalog import K1_MATRIX, K5_BOTTOM_MATRIX, MODELS, compute_handset_geometry, radio_model, register_model
from .renderer import HandsetRenderer
from .spec import HandsetSpec

__all__ = [
    "HandsetRenderer",
    "HandsetSpec",
    "K1_MATRIX",
    "K5_BOTTOM_MATRIX",
    "MODELS",
    "compute_handset_geometry",
    "radio_model",
    "register_model",
]
