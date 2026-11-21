"""Pro-grade Colored Hex + ASCII Inspector with column ruler, live data decoder, offset jump & search."""

from __future__ import annotations

import struct
from pathlib import Path
from typing import Any

import gi

gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")
from gi.repository import Adw, Gdk, GLib, Gtk  # noqa: E402

from .app_icon import KRX_IDENT, inspect_krx_metadata
from .util import fmt_size

_MAX_RENDER_BYTES = 64 * 1024  # Cap instant TextBuffer render at 64 KiB

_TABLE_STRIDES: dict[bytes, int] = {
    b"CHAN": 64,
    b"CONT": 32,
    b"BNDP": 32,
    b"HITS": 32,
    b"ZONE": 32,
    b"FMBC": 16,
    b"VFOA": 64,
    b"VFOB": 64,
    b"DICT": 32,
}


def detect_binary_format(data: bytes, name: str = "") -> str:
    """Identify standard Kern binary headers, FourCCs, KRX containers, and calibration blobs."""
    if not data:
        return "Empty (0 B)"
    if data[:4] == KRX_IDENT:
        meta = inspect_krx_metadata(data, name)
        ver = f" {meta['version']}" if meta.get("version") else ""
        return f"KRX {meta['exec_type']} ({meta['name']}{ver})"
    if data[:4] == b"KRX1" or data[:4] == b"\x7fELF":
        return "KRX / ELF Executable"
    if data[:8] == b"KFW1\x00\x00\x00\x00" or data[:4] == b"KFW1":
        return "KFW Container"
    fourcc = data[:4]
    if fourcc in _TABLE_STRIDES:
        stride = _TABLE_STRIDES[fourcc]
        recs = max(0, (len(data) - 16) // stride)
        return f"Kern Table ({fourcc.decode('ascii')}) · {recs} rec × {stride} B"
    if data[:2] == b"PK":
        return "ZIP Archive"
    if len(data) == 512 and ("calib" in name.lower() or name.endswith(".dat")):
        return "Factory Calibration (512 B)"
    if len(data) == 32 and name.endswith(".sig"):
        return "Ascon-128 MAC (32 B)"
    return "Raw Binary"


def decode_inspector_values(data: bytes, offset: int) -> dict[str, str]:
    """Decode u8/i8/u16/u32/f32/bin/char at a byte offset for the live hex inspector bar."""
    if offset < 0 or offset >= len(data):
        return {"offset": "—", "bin": "—", "u8": "—", "u16": "—", "u32": "—", "f32": "—", "char": "—"}
    b0 = data[offset]
    i8 = b0 - 256 if b0 >= 128 else b0
    u16_s = f"{struct.unpack_from('<H', data, offset)[0]} (0x{struct.unpack_from('<H', data, offset)[0]:04X})" if offset + 2 <= len(data) else "—"
    if offset + 4 <= len(data):
        u32_v = struct.unpack_from("<I", data, offset)[0]
        f32_v = struct.unpack_from("<f", data, offset)[0]
        u32_s = f"{u32_v} (0x{u32_v:08X})"
        f32_s = f"{f32_v:.4g}"
    else:
        u32_s = "—"
        f32_s = "—"
    ch = repr(chr(b0)) if 0x20 <= b0 <= 0x7E else f"0x{b0:02X}"
    return {
        "offset": f"0x{offset:08X} ({offset})",
        "bin": f"{b0:08b}",
        "u8": f"{b0} / {i8:+d}",
        "u16": u16_s,
        "u32": u32_s,
        "f32": f32_s,
        "char": ch,
    }


def _byte_tag(b: int, offset: int, header_len: int) -> str:
    if offset < header_len:
        return "hdr"
    if b == 0x00:
        return "zero"
    if b == 0xFF:
        return "ff"
    if 0x20 <= b <= 0x7E:
        return "ascii"
    if b < 0x20:
        return "ctrl"
    return "high"


class HexViewWidget(Gtk.Box):
    """Pro-grade Colored Hex + ASCII Inspector with ruler, jump-to-offset, search & live byte decoder."""

    def __init__(self, data: bytes = b"", title: str = "Binary", *, header_highlight: int | None = None, parent_win: Any = None) -> None:
        super().__init__(orientation=Gtk.Orientation.VERTICAL, spacing=5, hexpand=True, vexpand=True)
        self._data = data
        self._title = title
        self._parent_win = parent_win
        self._header_len = self._infer_header_len(data, header_highlight)
        self._group = 1
        self._line_byte_starts: list[tuple[int, int, int]] = []  # (char_start, char_end, byte_offset)

        # --- Top Inspector Bar ---
        bar = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=6, margin_top=6, margin_start=10, margin_end=10)
        self.badge_lbl = Gtk.Label(label=detect_binary_format(data, title), valign=Gtk.Align.CENTER)
        self.badge_lbl.add_css_class("cap-chip")
        bar.append(self.badge_lbl)

        self.size_lbl = Gtk.Label(label=fmt_size(len(data)), valign=Gtk.Align.CENTER)
        self.size_lbl.add_css_class("caption")
        self.size_lbl.add_css_class("dim-label")
        self.size_lbl.add_css_class("numeric")
        bar.append(self.size_lbl)

        bar.append(Gtk.Box(hexpand=True))

        self.goto_entry = Gtk.Entry(placeholder_text="0x0000", width_chars=8, tooltip_text="Jump to hex or decimal offset")
        self.goto_entry.add_css_class("console-entry")
        self.goto_entry.connect("activate", lambda _e: self._jump_to_offset())
        bar.append(self.goto_entry)

        self.search_entry = Gtk.SearchEntry(placeholder_text="Find hex or ASCII…", width_chars=16)
        self.search_entry.connect("search-changed", lambda _e: self._highlight_search())
        bar.append(self.search_entry)

        self.match_lbl = Gtk.Label(label="", valign=Gtk.Align.CENTER)
        self.match_lbl.add_css_class("caption")
        self.match_lbl.add_css_class("dim-label")
        bar.append(self.match_lbl)

        grp_box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL)
        grp_box.add_css_class("linked")
        first: Gtk.ToggleButton | None = None
        for g in (1, 2, 4):
            tb = Gtk.ToggleButton(label=f"{g}B", active=(g == 1))
            tb.add_css_class("place-btn")
            if first is None:
                first = tb
            else:
                tb.set_group(first)
            tb.connect("toggled", lambda b, sz=g: self._set_group(sz) if b.get_active() else None)
            grp_box.append(tb)
        bar.append(grp_box)

        copy_b = Gtk.Button(icon_name="edit-copy-symbolic", tooltip_text="Copy Hex Dump")
        copy_b.add_css_class("flat")
        copy_b.connect("clicked", lambda _b: self._copy_hex())
        bar.append(copy_b)

        save_b = Gtk.Button(icon_name="document-save-symbolic", tooltip_text="Export Raw Binary…")
        save_b.add_css_class("flat")
        save_b.connect("clicked", lambda _b: self._export_raw())
        bar.append(save_b)
        self.append(bar)

        # --- Color Legend Strip ---
        leg = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=10, margin_start=12, margin_end=12)
        for lbl, cls in (
            ("Header", "hex-leg-hdr"),
            ("ASCII", "hex-leg-ascii"),
            ("Control", "hex-leg-ctrl"),
            ("High", "hex-leg-high"),
            ("0xFF", "hex-leg-ff"),
            ("0x00", "hex-leg-zero"),
        ):
            ll = Gtk.Label(label=f"● {lbl}")
            ll.add_css_class("caption")
            ll.add_css_class(cls)
            leg.append(ll)
        self.append(leg)

        # --- Column Ruler + Colored Monospace Hex Grid ---
        frame = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, vexpand=True, margin_start=10, margin_end=10)
        frame.add_css_class("console-frame")

        self.ruler_lbl = Gtk.Label(
            label="OFFSET    00 01 02 03 04 05 06 07  08 09 0A 0B 0C 0D 0E 0F  │ 0123456789ABCDEF",
            xalign=0,
            margin_start=10,
            margin_top=6,
            margin_bottom=4,
        )
        self.ruler_lbl.add_css_class("hex-ruler")
        frame.append(self.ruler_lbl)
        frame.append(Gtk.Separator(orientation=Gtk.Orientation.HORIZONTAL))

        scroll = Gtk.ScrolledWindow(vexpand=True, hexpand=True)
        self.buf = Gtk.TextBuffer()
        self.buf.create_tag("off", foreground="#64748b", weight=600)
        self.buf.create_tag("sep", foreground="#334155")
        self.buf.create_tag("hdr", foreground="#fbbf24", weight=700)
        self.buf.create_tag("zero", foreground="#475569")
        self.buf.create_tag("ff", foreground="#f59e0b")
        self.buf.create_tag("ascii", foreground="#34d399")
        self.buf.create_tag("ctrl", foreground="#38bdf8")
        self.buf.create_tag("high", foreground="#c084fc")
        self.buf.create_tag("asc_txt", foreground="#e2e8f0")
        self.buf.create_tag("asc_dot", foreground="#475569")
        self.buf.create_tag("match", background="#2563eb", foreground="#ffffff")
        self.buf.create_tag("row_alt", paragraph_background="#161b24")
        self.buf.connect("mark-set", self._on_cursor_moved)

        self.tv = Gtk.TextView(buffer=self.buf, editable=False, monospace=True, cursor_visible=True)
        self.tv.set_wrap_mode(Gtk.WrapMode.NONE)
        self.tv.add_css_class("wire-console")
        scroll.set_child(self.tv)
        frame.append(scroll)
        self.append(frame)

        # --- Live Byte Data Inspector Footer ---
        insp = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=12, margin_start=12, margin_end=12, margin_bottom=8, margin_top=2)
        self._insp_labels: dict[str, Gtk.Label] = {}
        for key, cap in (
            ("offset", "Off"),
            ("bin", "Bin"),
            ("u8", "u8/i8"),
            ("u16", "u16"),
            ("u32", "u32"),
            ("f32", "f32"),
            ("char", "Char"),
        ):
            lbl = Gtk.Label(label=f"{cap}: —", xalign=0)
            lbl.add_css_class("caption")
            lbl.add_css_class("numeric")
            lbl.add_css_class("dim-label" if key != "offset" else "hex-leg-ctrl")
            insp.append(lbl)
            self._insp_labels[key] = lbl
        self.append(insp)

        self.set_data(data, title, header_highlight=header_highlight)

    @staticmethod
    def _infer_header_len(data: bytes, explicit: int | None) -> int:
        if explicit is not None:
            return explicit if len(data) >= explicit else 0
        if len(data) >= 24 and data[:4] == KRX_IDENT:
            return 24
        if len(data) >= 32 and data[:4] == b"KFW1":
            return 32
        if len(data) >= 16 and data[:4] in _TABLE_STRIDES:
            return 16
        return 16 if len(data) >= 16 else 0

    def _set_group(self, g: int) -> None:
        if g != self._group:
            self._group = g
            self._render()

    def set_data(self, data: bytes, title: str = "Binary", *, header_highlight: int | None = None) -> None:
        self._data = data
        self._title = title
        self._header_len = self._infer_header_len(data, header_highlight)
        self.badge_lbl.set_text(detect_binary_format(data, title))
        self.size_lbl.set_text(f"{len(data):,} B ({fmt_size(len(data))})")
        self._render()
        self._update_inspector(0 if data else -1)

    def _render(self) -> None:
        self.buf.set_text("")
        data = self._data[:_MAX_RENDER_BYTES]
        it = self.buf.get_end_iter()
        if not data:
            self.buf.insert_with_tags_by_name(it, "(empty file)\n", "off")
            return

        grp = self._group
        stride = _TABLE_STRIDES.get(data[:4], 0)
        for line_idx, off in enumerate(range(0, len(data), 16)):
            row = data[off : off + 16]
            line_start_off = it.get_offset()
            self.buf.insert_with_tags_by_name(it, f"{off:08X}  ", "off")
            for i in range(16):
                if i < len(row):
                    b = row[i]
                    tag = _byte_tag(b, off + i, self._header_len)
                    sep = " " if (i + 1) % grp == 0 else ""
                    if i == 7:
                        sep += " "
                    self.buf.insert_with_tags_by_name(it, f"{b:02X}{sep}", tag)
                else:
                    sep = " " if (i + 1) % grp == 0 else ""
                    if i == 7:
                        sep += " "
                    self.buf.insert_with_tags_by_name(it, "  " + sep, "zero")
            self.buf.insert_with_tags_by_name(it, " │ ", "sep")
            for i, b in enumerate(row):
                if 0x20 <= b <= 0x7E:
                    self.buf.insert_with_tags_by_name(it, chr(b), "hdr" if (off + i) < self._header_len else "asc_txt")
                else:
                    self.buf.insert_with_tags_by_name(it, "·", "asc_dot")
            self.buf.insert(it, "\n")

            # Subtle alternating record/row shading
            alt = (((off - 16) // stride) % 2 == 1) if (stride and off >= 16) else (line_idx % 2 == 1)
            if alt:
                s_row = self.buf.get_iter_at_offset(line_start_off)
                self.buf.apply_tag_by_name("row_alt", s_row, it)

        if len(self._data) > _MAX_RENDER_BYTES:
            self.buf.insert_with_tags_by_name(
                it,
                f"\n… showing first {_MAX_RENDER_BYTES:,} of {len(self._data):,} bytes\n",
                "off",
            )
        self._highlight_search()

    def _on_cursor_moved(self, _buf: Any, loc: Any, mark: Any) -> None:
        if mark != self.buf.get_insert():
            return
        line = loc.get_line()
        col = loc.get_line_offset()
        base_off = line * 16
        if base_off >= len(self._data):
            return
        # Map column inside hex region (cols 10..58) or ASCII region back to byte index 0..15
        byte_in_row = 0
        if col >= 10:
            line_text = self.buf.get_text(
                self.buf.get_iter_at_line(line)[1],
                loc,
                False,
            )
            if "│" in line_text:
                after = line_text.split("│", 1)[1].lstrip(" ")
                byte_in_row = max(0, min(15, len(after)))
            else:
                hex_part = line_text[10:].replace(" ", "")
                byte_in_row = max(0, min(15, len(hex_part) // 2))
        self._update_inspector(min(len(self._data) - 1, base_off + byte_in_row))

    def _update_inspector(self, offset: int) -> None:
        vals = decode_inspector_values(self._data, offset)
        caps = {"offset": "Off", "bin": "Bin", "u8": "u8/i8", "u16": "u16", "u32": "u32", "f32": "f32", "char": "Char"}
        for k, lbl in self._insp_labels.items():
            lbl.set_text(f"{caps[k]}: {vals[k]}")

    def _jump_to_offset(self) -> None:
        raw = self.goto_entry.get_text().strip()
        if not raw:
            return
        try:
            off = int(raw, 16) if raw.lower().startswith("0x") else int(raw, 16 if any(c in "abcdefABCDEF" for c in raw) else 10)
        except ValueError:
            return
        off = max(0, min(max(0, len(self._data) - 1), off))
        line = min(off // 16, max(0, self.buf.get_line_count() - 1))
        ok, it = self.buf.get_iter_at_line(line)
        if ok:
            self.buf.place_cursor(it)
            self.tv.scroll_to_iter(it, 0.15, False, 0.0, 0.0)
            self._update_inspector(off)

    def _highlight_search(self) -> None:
        s_it, e_it = self.buf.get_bounds()
        self.buf.remove_tag_by_name("match", s_it, e_it)
        q = self.search_entry.get_text().strip()
        if not q:
            self.match_lbl.set_text("")
            return
        text = self.buf.get_text(s_it, e_it, False)
        low_t, low_q = text.lower(), q.lower()
        start = 0
        count = 0
        first_iter = None
        while True:
            idx = low_t.find(low_q, start)
            if idx < 0:
                break
            a = self.buf.get_iter_at_offset(idx)
            b = self.buf.get_iter_at_offset(idx + len(q))
            self.buf.apply_tag_by_name("match", a, b)
            if first_iter is None:
                first_iter = a
            count += 1
            start = idx + max(1, len(q))
        self.match_lbl.set_text(f"{count} match{'es' if count != 1 else ''}" if count else "No match")
        if first_iter is not None:
            self.tv.scroll_to_iter(first_iter, 0.15, False, 0.0, 0.0)

    def _copy_hex(self) -> None:
        d = Gdk.Display.get_default()
        if d is None:
            return
        s_it, e_it = self.buf.get_bounds()
        d.get_clipboard().set(self.buf.get_text(s_it, e_it, False))

    def _export_raw(self) -> None:
        if not self._data:
            return
        dlg = Gtk.FileDialog(title=f"Export {Path(self._title).name or 'binary.bin'}")
        dlg.set_initial_name(Path(self._title).name or "dump.bin")

        def _picked(d: Any, res: Any) -> None:
            try:
                dest = d.save_finish(res).get_path()
            except GLib.Error:
                return
            if dest:
                Path(dest).write_bytes(self._data)

        dlg.save(self._parent_win, None, _picked)


def show_hex_viewer_dialog(parent: Any, data: bytes, title: str, *, header_highlight: int | None = None) -> None:
    """Present a clean Adw.Dialog hosting the pro Colored Hex + ASCII Inspector."""
    dlg = Adw.Dialog(title=f"Hex Inspector — {Path(title).name or title}", content_width=820, content_height=560)
    tv = Adw.ToolbarView()
    tv.add_top_bar(Adw.HeaderBar())
    tv.set_content(HexViewWidget(data, title, header_highlight=header_highlight, parent_win=parent))
    dlg.set_child(tv)
    dlg.present(parent)
