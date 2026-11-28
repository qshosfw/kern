"""Small GTK4 formatting and icon helpers."""

from __future__ import annotations

from typing import Any

import kernfs_mount


def fmt_size(n: int) -> str:
    if n < 1024:
        return f"{n} B"
    if n < 1024 * 1024:
        return f"{n / 1024.0:.1f} KB"
    return f"{n / (1024.0 * 1024.0):.2f} MB"


def fmt_duration(seconds: float) -> str:
    s = int(max(0, seconds))
    h, m = s // 3600, (s // 60) % 60
    if h:
        return f"{h} h {m:02d} m"
    if m:
        return f"{m} m {s % 60:02d} s"
    return f"{s} s"


def file_icon_name(name: str, full: str, is_dir: bool) -> str:
    if is_dir:
        return "folder-symbolic"
    if full.startswith("/sys/"):
        return "security-high-symbolic"
    if name.endswith(kernfs_mount.PKG_EXTS):
        return "application-x-executable-symbolic"
    if name.endswith(".cfg"):
        return "preferences-system-symbolic"
    if name.endswith((".vfo", ".ch", ".ct", ".zn", ".bnd", ".fm", ".hit")):
        return "document-properties-symbolic"
    return "text-x-generic-symbolic"


def battery_icon_name(pct: int | None) -> str:
    if pct is None:
        return "battery-missing-symbolic"
    steps = (0, 10, 20, 30, 40, 50, 60, 70, 80, 90, 100)
    return f"battery-level-{min(steps, key=lambda s: abs(s - pct))}-symbolic"


def set_pill(pill: Any, text: str, kind: str | None) -> None:
    pill.set_text(text)
    for c in ("live", "emu", "demo", "headless", "dfu", "hardened", "warn"):
        if c != kind:
            pill.remove_css_class(c)
    if kind:
        pill.add_css_class(kind)


def wrap_actions(*buttons: Any, align: float = 1.0) -> Any:
    """
    Row suffix / button cluster that flows onto extra lines in narrow (phone) windows instead of
    forcing the whole page wider: minimum width is the widest button, natural width is one line.
    """
    import gi

    gi.require_version("Gtk", "4.0")
    gi.require_version("Adw", "1")
    from gi.repository import Adw, Gtk

    box = Adw.WrapBox(child_spacing=6, line_spacing=6, align=align, valign=Gtk.Align.CENTER)
    for b in buttons:
        box.append(b)
    return box
