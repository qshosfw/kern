"""Authentic Kern OS pixel-art app icon loader & KRX SECTION_ASSETS (0x0004) bitmap decoder."""

from __future__ import annotations

import struct
from functools import lru_cache
from pathlib import Path
from typing import Any

import gi

gi.require_version("Gtk", "4.0")
from gi.repository import Gtk  # noqa: E402

from ... import REPO_ROOT
from ...render.paint import round_rect

KRX_IDENT = b"\xC0\xDE\x01\xFE"
_KRX_EXEC_TYPES = {1: "App", 2: "Service", 3: "Daemon", 4: "Driver", 5: "Module"}
_KRX_CLASSES = {0: "Native", 1: "Paged", 2: "VM", 3: "Declare"}

# Built-in fallback 8×8 puzzle-piece pixel glyph (used only if repo icon packs aren't on disk)
_FALLBACK_APPS_8X8 = (8, 8, bytes([0x3C, 0x42, 0x99, 0xA5, 0xA5, 0x99, 0x42, 0x3C]))


def _pack_ascii_grid(lines: list[str], w: int, h: int) -> bytes:
    """Pack `#`/`.` ASCII art rows into MSB-first 1-bit monochrome bytes."""
    stride = (w + 7) // 8
    out = bytearray(stride * h)
    for y in range(min(h, len(lines))):
        row = lines[y]
        for x in range(min(w, len(row))):
            if row[x] == "#":
                out[y * stride + (x >> 3)] |= 0x80 >> (x & 7)
    return bytes(out)


@lru_cache(maxsize=1)
def load_kern_pixel_icons() -> tuple[dict[int, tuple[int, int, bytes]], dict[str, tuple[int, int, bytes]]]:
    """Load real 16×16 pixel bitmaps from `assets/icons/kern_apps_16.txt` and `crates/gfx/src/icons/app.rs`."""
    by_id: dict[int, tuple[int, int, bytes]] = {}
    by_name: dict[str, tuple[int, int, bytes]] = {}

    txt = REPO_ROOT / "assets" / "icons" / "kern_apps_16.txt"
    if txt.is_file():
        cur_id: int | None = None
        cur_name = ""
        cur_rows: list[str] = []

        def _flush() -> None:
            if cur_id is not None and len(cur_rows) >= 8:
                bmp = (16, 16, _pack_ascii_grid(cur_rows[:16], 16, 16))
                by_id[cur_id] = bmp
                if cur_name:
                    by_name[cur_name.lower()] = bmp

        for raw_line in txt.read_text(encoding="utf-8", errors="ignore").splitlines():
            s = raw_line.strip()
            if s.startswith("[") and s.endswith("]"):
                _flush()
                parts = s[1:-1].split(None, 1)
                try:
                    cur_id = int(parts[0])
                except ValueError:
                    cur_id = None
                cur_name = parts[1].strip() if len(parts) > 1 else ""
                cur_rows = []
            elif cur_id is not None and len(s) == 16 and set(s) <= {".", "#"}:
                cur_rows.append(s)
        _flush()

    app_rs = REPO_ROOT / "src" / "gfx" / "src" / "icons" / "app.rs"
    if not app_rs.is_file():
        app_rs = REPO_ROOT / "crates" / "gfx" / "src" / "icons" / "app.rs"
    if app_rs.is_file():
        lines = app_rs.read_text(encoding="utf-8", errors="ignore").splitlines()
        cur_slot = ""
        rows: list[str] = []
        for raw_line in lines:
            s = raw_line.strip()
            if "Launcher::" in s and "=>" in s:
                cur_slot = s.split("Launcher::", 1)[1].split()[0].strip(",:").lower()
                rows = []
            elif s.startswith('"') and '",' in s:
                inner = s.split('"', 2)[1]
                if len(inner) == 16 and set(inner) <= {".", "#"}:
                    rows.append(inner)
                    if len(rows) == 16 and cur_slot:
                        by_name.setdefault(cur_slot, (16, 16, _pack_ascii_grid(rows, 16, 16)))
                        rows = []

    return by_id, by_name


def _find_krx_icon_asset(data: bytes, hdr_len: int) -> tuple[int, int, bytes] | None:
    """Exact mirror of `KrxPreamble::find_icon_asset`: reads 4-byte aligned `(kind, w, h, blen, bits)` in SEC_ASSETS."""
    off = 24
    limit = min(len(data), hdr_len)
    best_8: tuple[int, int, bytes] | None = None
    while off + 4 <= limit:
        sec_id, sec_len = struct.unpack_from("<HH", data, off)
        if sec_len < 4 or off + sec_len > limit:
            break
        if sec_id == 0x0004:  # SECTION_ASSETS
            a_off = off + 4
            a_end = off + sec_len
            while a_off + 4 <= a_end:
                _kind, w, h, blen = data[a_off], data[a_off + 1], data[a_off + 2], data[a_off + 3]
                if a_off + 4 + blen > a_end:
                    break
                if 5 <= w <= 32 and 5 <= h <= 32 and blen >= ((w + 7) // 8) * h:
                    bmp = (w, h, data[a_off + 4 : a_off + 4 + blen])
                    if w >= 16:
                        return bmp  # Prefer 16×16 full app icon
                    if best_8 is None:
                        best_8 = bmp
                step = (4 + blen + 3) & ~3
                if step <= 0 or a_off + step > a_end:
                    break
                a_off += step
        step = (sec_len + 3) & ~3
        if step <= 0 or off + step > limit:
            break
        off += step
    return best_8


def resolve_pixel_bitmap(
    name: str,
    *,
    icon_asset: int = 0,
    bitmap: tuple[int, int, bytes] | None = None,
) -> tuple[int, int, bytes]:
    """Return the exact embedded KRX pixel bitmap, or resolve the authentic Kern OS 16×16 pixel bitmap."""
    if bitmap is not None:
        return bitmap
    by_id, by_name = load_kern_pixel_icons()
    low = name.lower()
    # Direct name-keyword match against authentic Kern OS 16×16 icon pack
    for key, aliases in (
        ("spectrum", ("spec", "waterfall")),
        ("fm_radio", ("fm", "broadcast")),
        ("scanner", ("scan", "search")),
        ("contacts", ("contact", "callsign", "dir")),
        ("packet_aprs", ("aprs", "packet", "kiss", "ax25", "mesh")),
        ("morse_cw", ("morse", "cw", "beacon")),
        ("gps_nav", ("gps", "gnss", "nav", "map")),
        ("terminal", ("term", "shell", "wire", "console")),
        ("audio_rec", ("audio", "rec", "voice", "mic")),
        ("weather", ("noaa", "wx", "weather")),
        ("calculator", ("calc", "math")),
        ("games", ("game", "snake", "tetris", "pong")),
        ("clock", ("clock", "time", "timer")),
        ("satellite", ("sat", "amsat", "iss")),
        ("power", ("power", "batt", "sleep")),
        ("upload", ("dfu", "flash", "boot", "update")),
        ("settings", ("set", "pref", "cfg")),
        ("folder", ("file", "expl", "fs")),
        ("flashlight", ("torch", "light")),
        ("diagnostics", ("diag", "debug", "test")),
        ("radio", ("radio", "vfo")),
        ("system", ("sys", "mod", "kern")),
    ):
        if key in by_name and any(a in low for a in aliases):
            return by_name[key]
    if icon_asset in by_id:
        return by_id[icon_asset]
    return by_name.get("apps") or by_name.get("system") or _FALLBACK_APPS_8X8


def inspect_krx_metadata(data: bytes, filename: str = "") -> dict[str, Any]:
    """Extract display metadata and embedded pixel bitmap from a radio application binary."""
    stem = Path(filename).stem.replace("_", " ").replace("-", " ").title() if filename else "App"
    ext = Path(filename).suffix.lstrip(".").upper() or "APP"
    out: dict[str, Any] = {
        "valid_krx": False,
        "name": stem,
        "version": "",
        "author": "",
        "exec_type": "Module" if ext == "MOD" else "App",
        "class_name": ext,
        "icon_asset": 0,
        "caps": 0,
        "header_len": min(len(data), 24),
        "bitmap": None,
    }
    if len(data) < 24 or data[:4] != KRX_IDENT:
        return out

    app_class = data[4]
    sec_count, hdr_len, _flags = struct.unpack_from("<HHH", data, 6)
    out["valid_krx"] = True
    out["class_name"] = _KRX_CLASSES.get(app_class, f"Class {app_class}")
    out["header_len"] = min(len(data), max(24, hdr_len))
    out["bitmap"] = _find_krx_icon_asset(data, hdr_len)

    off = 24
    limit = min(len(data), hdr_len)
    for _ in range(min(sec_count, 24)):
        if off + 4 > limit:
            break
        sec_id, sec_len = struct.unpack_from("<HH", data, off)
        if sec_len < 4 or off + sec_len > limit:
            break
        body = data[off + 4 : off + sec_len]
        if sec_id == 0x0001 and len(body) >= 25:
            name_off = struct.unpack_from("<H", body, 0)[0]
            maj, minor, patch = body[2], body[3], body[4]
            out["version"] = f"v{maj}.{minor}.{patch}"
            raw_auth = body[7:19].split(b"\x00")[0].decode("utf-8", errors="ignore").strip()
            if raw_auth:
                out["author"] = raw_auth
            out["icon_asset"] = struct.unpack_from("<H", body, 23)[0]
            if 24 <= name_off < limit:
                nul = data.find(b"\x00", name_off, limit)
                end = nul if nul != -1 else limit
                nm = data[name_off:end].decode("utf-8", errors="ignore").strip()
                if nm:
                    out["name"] = nm
        elif sec_id == 0x0009 and len(body) >= 12:
            out["exec_type"] = _KRX_EXEC_TYPES.get(body[0], "App")
            out["caps"] = struct.unpack_from("<I", body, 8)[0]
        off += (sec_len + 3) & ~3
    return out


def make_app_icon_widget(
    name: str,
    *,
    exec_type: str = "App",
    icon_asset: int = 0,
    bitmap: tuple[int, int, bytes] | None = None,
    size: int = 36,
) -> Gtk.DrawingArea:
    """Render the authentic 1-bit pixel-art bitmap from the app (or Kern icon pack) on a crisp LCD-styled tile."""
    area = Gtk.DrawingArea(valign=Gtk.Align.CENTER, halign=Gtk.Align.CENTER)
    area.set_content_width(size)
    area.set_content_height(size)
    bw, bh, bits = resolve_pixel_bitmap(name, icon_asset=icon_asset, bitmap=bitmap)

    def _draw(_a: Any, cr: Any, w: int, h: int) -> None:
        import cairo

        s = float(min(w, h))
        ox, oy = (w - s) * 0.5, (h - s) * 0.5
        pad = 1.5
        r = s * 0.22

        # Subtle calm dark-slate LCD tile background
        round_rect(cr, ox + pad, oy + pad, s - pad * 2, s - pad * 2, r)
        cr.set_source_rgba(0.11, 0.13, 0.16, 0.92)
        cr.fill_preserve()
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.12)
        cr.set_line_width(0.9)
        cr.stroke()

        # Crisp integer-snapped pixel blocks (authentic 1-bit radio icon pixels!)
        cr.set_antialias(cairo.ANTIALIAS_NONE)
        cell = max(1, int((s - 8.0) // max(bw, bh)))
        grid_w = bw * cell
        grid_h = bh * cell
        x0 = int(ox + (s - grid_w) * 0.5)
        y0 = int(oy + (s - grid_h) * 0.5)
        stride = (bw + 7) // 8

        # Clean high-contrast crisp pixel ink
        cr.set_source_rgba(0.84, 0.95, 0.92, 0.96)
        for yy in range(bh):
            for xx in range(bw):
                if bits[yy * stride + (xx >> 3)] & (0x80 >> (xx & 7)):
                    cr.rectangle(x0 + xx * cell, y0 + yy * cell, cell, cell)
        cr.fill()

    area.set_draw_func(_draw)
    return area
