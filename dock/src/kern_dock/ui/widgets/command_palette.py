"""Ctrl+P Fuzzy Command Palette dialog over every registered core & plugin ActionSpec."""

from __future__ import annotations

from typing import Any

import gi

gi.require_version("Gtk", "4.0")
gi.require_version("Gdk", "4.0")
gi.require_version("Adw", "1")
from gi.repository import Adw, Gdk, GLib, Gtk  # noqa: E402

from ...core.registry import ActionSpec, CommandRegistry


class CommandPaletteDialog(Adw.Dialog):
    """Instant searchable Command Palette inspired by GNOME Builder / VS Code."""

    def __init__(self, registry: CommandRegistry) -> None:
        super().__init__(title="Command Palette", content_width=520, content_height=440)
        self._registry = registry
        self._shown: list[ActionSpec] = []

        tb = Adw.ToolbarView()
        tb.add_top_bar(Adw.HeaderBar(show_end_title_buttons=True))

        box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=10, margin_top=6, margin_bottom=14, margin_start=14, margin_end=14)
        self.search = Gtk.SearchEntry(placeholder_text="Jump to page, tune preset, toggle hardware, run plugin…")
        self.search.add_css_class("palette-search")
        self.search.connect("search-changed", lambda _e: self._rebuild())
        self.search.connect("activate", lambda _e: self._activate_selected())
        ec = Gtk.EventControllerKey()
        ec.connect("key-pressed", self._on_key)
        self.search.add_controller(ec)
        box.append(self.search)

        scroll = Gtk.ScrolledWindow(vexpand=True)
        scroll.set_policy(Gtk.PolicyType.NEVER, Gtk.PolicyType.AUTOMATIC)
        self.listbox = Gtk.ListBox(selection_mode=Gtk.SelectionMode.SINGLE)
        self.listbox.add_css_class("boxed-list")
        self.listbox.connect("row-activated", self._on_row_activated)
        scroll.set_child(self.listbox)
        box.append(scroll)

        tb.set_content(box)
        self.set_child(tb)
        self._rebuild()

    def _rebuild(self) -> None:
        c = self.listbox.get_first_child()
        while c is not None:
            n = c.get_next_sibling()
            self.listbox.remove(c)
            c = n
        self._shown = self._registry.search(self.search.get_text())[:40]
        if not self._shown:
            empty = Adw.ActionRow(title="No matching commands", subtitle="Try another search term")
            empty.set_sensitive(False)
            self.listbox.append(empty)
            return
        for spec in self._shown:
            row = Adw.ActionRow(
                title=GLib.markup_escape_text(spec.title),
                subtitle=GLib.markup_escape_text(spec.subtitle or spec.category),
                activatable=True,
            )
            if spec.icon:
                row.add_prefix(Gtk.Image.new_from_icon_name(spec.icon))
            cat = Gtk.Label(label=spec.category, valign=Gtk.Align.CENTER)
            cat.add_css_class("cap-chip")
            row.add_suffix(cat)
            if spec.accel:
                sc = Gtk.ShortcutLabel(accelerator=spec.accel, valign=Gtk.Align.CENTER)
                row.add_suffix(sc)
            row._action_spec = spec  # type: ignore[attr-defined]
            self.listbox.append(row)
        first = self.listbox.get_row_at_index(0)
        if first is not None:
            self.listbox.select_row(first)

    def _on_key(self, _c: Any, keyval: int, _kc: int, _st: Any) -> bool:
        if keyval == Gdk.KEY_Escape:
            self.close()
            return True
        if keyval in (Gdk.KEY_Down, Gdk.KEY_Up):
            cur = self.listbox.get_selected_row()
            idx = cur.get_index() if cur else 0
            nxt_idx = max(0, min(len(self._shown) - 1, idx + (1 if keyval == Gdk.KEY_Down else -1)))
            nxt = self.listbox.get_row_at_index(nxt_idx)
            if nxt is not None:
                self.listbox.select_row(nxt)
            return True
        return False

    def _activate_selected(self) -> None:
        row = self.listbox.get_selected_row()
        if row is not None:
            self._on_row_activated(self.listbox, row)

    def _on_row_activated(self, _lb: Any, row: Any) -> None:
        spec: ActionSpec | None = getattr(row, "_action_spec", None)
        self.close()
        if spec is not None:
            GLib.idle_add(lambda s=spec: (s.handler(), False)[1])
