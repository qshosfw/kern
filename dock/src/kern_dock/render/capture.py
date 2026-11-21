"""PNG encoder, offscreen LCD / handset renders and the GIF recorder with trim / cut / splice."""

from __future__ import annotations

import dataclasses
import struct
import time
import zlib
from pathlib import Path
from typing import Callable

try:
    import numpy as _np
except ImportError:
    _np = None  # type: ignore[assignment]

from .handset.catalog import compute_handset_geometry
from .handset.renderer import HandsetRenderer
from .lcd import FastLcdState, LcdLayerCache, LcdSettings, paint_fast_lcd


def _bgra_to_rgba(raw: bytes) -> bytes:
    if _np is not None:
        return _np.frombuffer(raw, dtype=_np.uint8).reshape(-1, 4)[:, [2, 1, 0, 3]].tobytes()
    out = bytearray(len(raw))
    out[0::4], out[1::4], out[2::4], out[3::4] = raw[2::4], raw[1::4], raw[0::4], raw[3::4]
    return bytes(out)


def render_lcd_to_rgba(fb: bytes, lcd_st: LcdSettings, width: int = 440, height: int = 304) -> tuple[int, int, bytes]:
    import cairo

    surf = cairo.ImageSurface(cairo.FORMAT_ARGB32, width, height)
    cr = cairo.Context(surf)
    st_s = dataclasses.replace(lcd_st, ghosting=False)
    fs = FastLcdState()
    fs.step(fb, st_s)
    paint_fast_lcd(cr, 0, 0, width, height, fs, st_s, LcdLayerCache())
    surf.flush()
    return width, height, _bgra_to_rgba(bytes(surf.get_data()))


def render_handset_to_rgba(
    model: str,
    fb: bytes,
    lcd_st: LcdSettings,
    *,
    pressed_keys: frozenset[str] = frozenset(),
    led_state: str = "rx",
    torch_on: bool = False,
    scale: float = 1.0,
    bg_rgb: tuple[float, float, float] | None = (0.11, 0.12, 0.14),
    renderer: HandsetRenderer | None = None,
) -> tuple[int, int, bytes]:
    import cairo

    lw, lh = compute_handset_geometry(model)
    w, h = int(round(lw * scale)), int(round(lh * scale))
    surf = cairo.ImageSurface(cairo.FORMAT_ARGB32, w, h)
    cr = cairo.Context(surf)
    if bg_rgb is not None:
        cr.set_source_rgb(*bg_rgb)
        cr.rectangle(0, 0, w, h)
        cr.fill()
    cr.scale(scale, scale)
    st_s = dataclasses.replace(lcd_st, ghosting=False)
    fs = FastLcdState()
    fs.step(fb, st_s)
    rend = renderer or HandsetRenderer()
    rend.paint(
        cr,
        model,
        fs,
        st_s,
        press_anim={k: 1.0 for k in pressed_keys},
        hover_anim={},
        led_state=led_state,
        torch_on=torch_on,
    )
    surf.flush()
    return w, h, _bgra_to_rgba(bytes(surf.get_data()))


def encode_png_rgba(w: int, h: int, rgba: bytes) -> bytes:
    def chunk(tag: bytes, payload: bytes) -> bytes:
        crc = zlib.crc32(tag + payload) & 0xFFFFFFFF
        return struct.pack(">I", len(payload)) + tag + payload + struct.pack(">I", crc)

    stride = w * 4
    raw = bytearray()
    for y in range(h):
        raw.append(0)
        raw.extend(rgba[y * stride : (y + 1) * stride])
    ihdr = struct.pack(">IIBBBBB", w, h, 8, 6, 0, 0, 0)
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", ihdr) + chunk(b"IDAT", zlib.compress(bytes(raw), 6)) + chunk(b"IEND", b"")


@dataclasses.dataclass
class RecordedFrame:
    ts_ms: int
    fb: bytes
    pressed: frozenset[str]
    led: str
    torch: bool


class RecordingSession:
    def __init__(self) -> None:
        self.frames: list[RecordedFrame] = []
        self.recording: bool = False
        self.t0_mono: float = 0.0
        self.trim_in: int = 0
        self.trim_out: int = 0
        self.cuts: list[tuple[int, int]] = []

    def start(self, initial_fb: bytes, pressed: frozenset[str], led: str, torch: bool) -> None:
        self.frames = [RecordedFrame(0, bytes(initial_fb), frozenset(pressed), led, torch)]
        self.trim_in = self.trim_out = 0
        self.cuts = []
        self.t0_mono = time.monotonic()
        self.recording = True

    def push(self, fb: bytes, pressed: frozenset[str], led: str, torch: bool) -> None:
        if not self.recording:
            return
        if self.frames:
            last = self.frames[-1]
            if last.fb == fb and last.pressed == pressed and last.led == led and last.torch == torch:
                return
        self.frames.append(RecordedFrame(int((time.monotonic() - self.t0_mono) * 1000.0), bytes(fb), frozenset(pressed), led, torch))
        self.trim_out = len(self.frames) - 1

    def stop(self) -> None:
        if self.recording:
            self.recording = False
            self.trim_out = max(0, len(self.frames) - 1)

    def add_cut(self, start_idx: int, end_idx: int) -> None:
        a, b = max(self.trim_in, min(start_idx, end_idx)), min(self.trim_out, max(start_idx, end_idx))
        if a <= b:
            self.cuts.append((a, b))

    def undo_cut(self) -> None:
        if self.cuts:
            self.cuts.pop()

    def is_cut(self, idx: int) -> bool:
        return any(a <= idx <= b for a, b in self.cuts)

    def active_indices(self) -> list[int]:
        if not self.frames:
            return []
        lo = max(0, min(self.trim_in, len(self.frames) - 1))
        hi = max(lo, min(self.trim_out, len(self.frames) - 1))
        return [i for i in range(lo, hi + 1) if not self.is_cut(i)]

    def frame_durations_ms(self, speed: float = 1.0, max_idle_ms: int | None = None) -> list[tuple[int, int]]:
        idxs = self.active_indices()
        if not idxs:
            return []
        spd = max(0.25, speed)
        out: list[tuple[int, int]] = []
        for pos, fi in enumerate(idxs):
            if pos + 1 < len(idxs):
                nxt = idxs[pos + 1]
                raw_dt = (self.frames[nxt].ts_ms - self.frames[fi].ts_ms) if nxt == fi + 1 else 120
            else:
                raw_dt = 350
            dt = max(25, raw_dt)
            if max_idle_ms is not None and dt > max_idle_ms:
                dt = max_idle_ms
            out.append((fi, max(20, int(round(dt / spd)))))
        return out

    def export_gif(
        self,
        out_path: str | Path,
        *,
        model: str,
        lcd_st: LcdSettings,
        mode: str = "lcd",
        speed: float = 1.0,
        max_idle_ms: int | None = None,
        progress_cb: Callable[[float], None] | None = None,
    ) -> int:
        seq = self.frame_durations_ms(speed=speed, max_idle_ms=max_idle_ms)
        if not seq:
            raise ValueError("No active frames to export")
        from PIL import Image

        rend = HandsetRenderer()
        pil_frames, delays = [], []
        for pos, (fi, dt_ms) in enumerate(seq):
            rf = self.frames[fi]
            if mode == "body":
                w, h, rgba = render_handset_to_rgba(
                    model, rf.fb, lcd_st, pressed_keys=rf.pressed, led_state=rf.led, torch_on=rf.torch, scale=1.0, renderer=rend
                )
            else:
                w, h, rgba = render_lcd_to_rgba(rf.fb, lcd_st, width=440, height=304)
            img = Image.frombytes("RGBA", (w, h), rgba).convert("RGB")
            pil_frames.append(img.quantize(colors=64 if mode == "lcd" else 128, method=Image.Quantize.MEDIANCUT, dither=Image.Dither.NONE))
            delays.append(dt_ms)
            if progress_cb:
                progress_cb((pos + 1) / float(len(seq)))
        dest = Path(out_path)
        pil_frames[0].save(dest, format="GIF", save_all=True, append_images=pil_frames[1:], duration=delays, loop=0, optimize=True, disposal=2)
        return dest.stat().st_size
