"""Rendering subsystem: ST7565 LCD engine, procedural Cairo helpers, declarative handsets & capture."""

from __future__ import annotations

from .capture import (
    RecordedFrame,
    RecordingSession,
    encode_png_rgba,
    render_handset_to_rgba,
    render_lcd_to_rgba,
)
from .handset import (
    HandsetRenderer,
    HandsetSpec,
    K1_MATRIX,
    K5_BOTTOM_MATRIX,
    MODELS,
    compute_handset_geometry,
    radio_model,
    register_model,
)
from .lcd import FastLcdState, LCD_H, LCD_PALETTES, LCD_W, LcdLayerCache, LcdSettings, paint_fast_lcd

__all__ = [
    "FastLcdState",
    "HandsetRenderer",
    "HandsetSpec",
    "K1_MATRIX",
    "K5_BOTTOM_MATRIX",
    "LCD_H",
    "LCD_PALETTES",
    "LCD_W",
    "LcdLayerCache",
    "LcdSettings",
    "MODELS",
    "RecordedFrame",
    "RecordingSession",
    "compute_handset_geometry",
    "encode_png_rgba",
    "paint_fast_lcd",
    "radio_model",
    "register_model",
    "render_handset_to_rgba",
    "render_lcd_to_rgba",
]
