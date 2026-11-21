"""Bespoke GTK4 radio icons and dynamic state-tinted ARGB32 tray pixmaps."""

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


_STATE_BADGE_RGB: dict[str, tuple[float, float, float] | None] = {
    "offline": None,
    "online": (0.20, 0.86, 0.48),
    "headless": (0.22, 0.74, 0.97),
    "dfu": (0.96, 0.62, 0.08),
    "tx": (0.96, 0.26, 0.26),
    "hardened": (0.68, 0.54, 0.98),
}


def make_tray_argb_pixmap(size: int = 32, *, state: str = "offline") -> tuple[int, int, bytes]:
    """
    Render a crisp Cairo handheld transceiver icon with an optional live status badge
    in big-endian ARGB32 bytes as required by ``org.kde.StatusNotifierItem.IconPixmap``.
    """
    import cairo

    surf = cairo.ImageSurface(cairo.FORMAT_ARGB32, size, size)
    cr = cairo.Context(surf)
    s = float(size) / 32.0
    cr.scale(s, s)

    # Antenna
    round_rect(cr, 8.2, 2.0, 4.2, 8.6, 2.0)
    cr.set_source_rgba(0.92, 0.94, 0.97, 0.95)
    cr.fill()

    # Volume knob
    round_rect(cr, 19.6, 5.2, 4.8, 5.4, 1.4)
    cr.set_source_rgba(0.32, 0.68, 1.0, 0.95)
    cr.fill()

    # Left PTT paddle
    round_rect(cr, 3.6, 13.0, 2.6, 7.6, 1.2)
    cr.set_source_rgba(0.32, 0.68, 1.0, 0.92)
    cr.fill()

    # Main handset silhouette
    round_rect(cr, 6.8, 9.6, 18.4, 20.4, 3.6)
    cr.set_source_rgba(0.92, 0.94, 0.97, 0.96)
    cr.fill()

    # LCD screen cutout
    round_rect(cr, 9.2, 12.2, 13.6, 8.2, 1.4)
    if state == "offline":
        cr.set_source_rgba(0.16, 0.18, 0.22, 0.96)
    elif state == "dfu":
        cr.set_source_rgba(0.96, 0.62, 0.08, 0.96)
    else:
        cr.set_source_rgba(0.18, 0.24, 0.42, 0.96)
    cr.fill()

    # Crisp keypad dots
    cr.set_source_rgba(0.16, 0.18, 0.22, 0.92)
    for row in (22.6, 25.8):
        for col in (10.4, 14.6, 18.8):
            round_rect(cr, col, row, 2.8, 2.0, 0.7)
            cr.fill()

    # Live status dot in bottom-right corner
    badge = _STATE_BADGE_RGB.get(state)
    if badge is not None:
        cr.arc(25.2, 25.2, 5.4, 0, 6.2831853)
        cr.set_source_rgba(0.08, 0.09, 0.11, 0.96)
        cr.fill()
        cr.arc(25.2, 25.2, 3.8, 0, 6.2831853)
        cr.set_source_rgba(badge[0], badge[1], badge[2], 1.0)
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
