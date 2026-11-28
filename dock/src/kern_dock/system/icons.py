"""Bespoke GTK4 radio icons and monochrome symbolic ARGB32 tray pixmaps."""

from __future__ import annotations

from pathlib import Path
from typing import Any

from .. import ASSETS_DIR
from ..render.paint import round_rect

ICON_COLOR_PATH = ASSETS_DIR / "icons" / "org.kernos.Dock.svg"
ICON_SYMBOLIC_PATH = ASSETS_DIR / "icons" / "org.kernos.Dock-symbolic.svg"


def register_icon_theme_paths(display: Any) -> None:
    """Expose Kern Dock's bundled SVG radio icons to GTK4's default IconTheme."""
    try:
        import gi

        gi.require_version("Gtk", "4.0")
        from gi.repository import Gtk

        theme = Gtk.IconTheme.get_for_display(display)
        icon_dir = str(ASSETS_DIR / "icons")
        if icon_dir not in theme.get_search_path():
            theme.add_search_path(icon_dir)
    except Exception:
        pass


def make_radio_image(size: int = 16, *, color: bool = False) -> Any:
    """Create a crisp GTK4 Gtk.Image using Kern Dock's bespoke SVG radio icon."""
    import gi

    gi.require_version("Gtk", "4.0")
    gi.require_version("Gdk", "4.0")
    from gi.repository import Gdk, GLib, Gtk

    path: Path = ICON_COLOR_PATH if color else ICON_SYMBOLIC_PATH
    try:
        raw = path.read_bytes()
        tex = Gdk.Texture.new_from_bytes(GLib.Bytes.new(raw))
        img = Gtk.Image.new_from_paintable(tex)
        img.set_pixel_size(size)
        return img
    except Exception:
        img = Gtk.Image.new_from_icon_name("network-wireless-symbolic")
        img.set_pixel_size(size)
        return img


# Tray status emblems: only states the user must notice get a coloured dot (GNOME emblem style);
# "offline" dims the glyph, everything else shows the plain monochrome symbolic glyph.
_STATE_BADGE_RGB: dict[str, tuple[float, float, float]] = {
    "dfu": (0.96, 0.62, 0.08),
    "tx": (0.88, 0.11, 0.14),
}


def make_tray_argb_pixmap(size: int = 22, *, state: str = "offline") -> tuple[int, int, bytes]:
    """
    Render the monochrome ``org.kernos.Dock-symbolic`` glyph (16 px grid, same geometry as the SVG)
    in big-endian ARGB32 bytes as required by ``org.kde.StatusNotifierItem.IconPixmap``.
    """
    import cairo

    surf = cairo.ImageSurface(cairo.FORMAT_ARGB32, size, size)
    cr = cairo.Context(surf)
    cr.scale(size / 16.0, size / 16.0)
    alpha = 0.55 if state == "offline" else 1.0
    cr.set_source_rgba(0.96, 0.96, 0.96, alpha)

    # Antenna, volume knob, side PTT
    for x, y, w, h, r in ((4.0, 0.8, 2.2, 4.4, 1.1), (10.0, 2.6, 2.5, 2.6, 0.7), (1.5, 6.8, 1.2, 3.8, 0.6)):
        round_rect(cr, x, y, w, h, r)
        cr.fill()

    # Body with screen + keypad cut-outs (even-odd, matches the SVG path)
    cr.set_fill_rule(cairo.FILL_RULE_EVEN_ODD)
    round_rect(cr, 2.5, 4.8, 11.0, 10.4, 1.7)
    cr.rectangle(4.8, 6.8, 6.4, 3.3)
    for cx in (5.2, 8.0, 10.8):
        cr.new_sub_path()
        cr.arc(cx, 12.05, 0.75, 0, 6.2831853)
    cr.fill()

    badge = _STATE_BADGE_RGB.get(state)
    if badge is not None:
        cr.set_fill_rule(cairo.FILL_RULE_WINDING)
        cr.set_operator(cairo.OPERATOR_CLEAR)
        cr.arc(12.8, 12.8, 3.6, 0, 6.2831853)
        cr.fill()
        cr.set_operator(cairo.OPERATOR_OVER)
        cr.arc(12.8, 12.8, 2.6, 0, 6.2831853)
        cr.set_source_rgb(*badge)
        cr.fill()

    surf.flush()
    raw = bytes(surf.get_data())
    # Cairo ARGB32 on little-endian is BGRA in memory; SNI IconPixmap requires big-endian ARGB.
    out = bytearray(len(raw))
    out[0::4] = raw[3::4]  # A
    out[1::4] = raw[2::4]  # R
    out[2::4] = raw[1::4]  # G
    out[3::4] = raw[0::4]  # B
    return size, size, bytes(out)
