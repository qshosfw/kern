"""Native Libadwaita Paginated Hex + ASCII Inspector with lazy remote range streaming."""

from __future__ import annotations

import struct
import threading
from pathlib import Path
from typing import Any, Callable

import gi

gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")
from gi.repository import Adw, Gdk, GLib, Gtk  # noqa: E402

from .app_icon import KRX_IDENT, inspect_krx_metadata
from .util import fmt_size

_PAGE_BYTES = 2048  # 128 lines × 16 B per page -> renders in < 1 ms, zero UI lag

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


def _format_page_lines(chunk: bytes, base_offset: int) -> str:
    """Fast pure-Python formatter producing 128 aligned hex+ASCII lines in < 0.5 ms."""
    lines: list[str] = []
    n = len(chunk)
    for rel in range(0, n, 16):
        row = chunk[rel : rel + 16]
        off = base_offset + rel
        left = " ".join(f"{b:02X}" for b in row[:8]).ljust(23)
        right = " ".join(f"{b:02X}" for b in row[8:16]).ljust(23)
        asc = "".join(chr(b) if 0x20 <= b <= 0x7E else "·" for b in row)
        lines.append(f"{off:08X}   {left}  {right}   {asc}")
    return "\n".join(lines)


class HexViewWidget(Gtk.Box):
    """Native Libadwaita paginated Hex + ASCII inspector (zero lag, lazy remote chunk reads)."""

    def __init__(
        self,
        data: bytes = b"",
        title: str = "Binary",
        *,
        header_highlight: int | None = None,
        parent_win: Any = None,
        remote_loader: Callable[[int, int], tuple[bytes, int]] | None = None,
        full_downloader: Callable[[], bytes] | None = None,
    ) -> None:
        super().__init__(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        self._toolbar_view = Adw.ToolbarView(hexpand=True, vexpand=True)
        self.append(self._toolbar_view)
        self._data: bytes = data
        self._page_bytes: bytes = b""
        self._page_offset: int = 0
        self._total_size: int = len(data)
        self._title = title
        self._parent_win = parent_win
        self._explicit_hdr = header_highlight
        self._header_len = self._infer_header_len(data, header_highlight)
        self._remote_loader = remote_loader
        self._full_downloader = full_downloader
        self._page_cache: dict[int, bytes] = {}
        self._loading = False

        # === TOP HEADERBAR + SEARCH BAR ===
        top_wrap = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        hdr = Adw.HeaderBar()
        self._win_title = Adw.WindowTitle(title=Path(title).name or title, subtitle="")
        hdr.set_title_widget(self._win_title)

        self.search_toggle = Gtk.ToggleButton(icon_name="system-search-symbolic", tooltip_text="Search in page (Ctrl+F)")
        hdr.pack_start(self.search_toggle)

        # Jump-to-offset popover button
        jump_btn = Gtk.MenuButton(icon_name="go-jump-symbolic", tooltip_text="Jump to Offset")
        jpop = Gtk.Popover()
        jbox = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=6, margin_top=8, margin_bottom=8, margin_start=8, margin_end=8)
        self.goto_entry = Gtk.Entry(placeholder_text="0x0000", width_chars=10)
        self.goto_entry.connect("activate", lambda _e: (jpop.popdown(), self._jump_to_offset()))
        go_b = Gtk.Button(label="Go")
        go_b.add_css_class("suggested-action")
        go_b.connect("clicked", lambda _b: (jpop.popdown(), self._jump_to_offset()))
        jbox.append(self.goto_entry)
        jbox.append(go_b)
        jpop.set_child(jbox)
        jump_btn.set_popover(jpop)
        hdr.pack_start(jump_btn)

        copy_b = Gtk.Button(icon_name="edit-copy-symbolic", tooltip_text="Copy visible hex")
        copy_b.connect("clicked", lambda _b: self._copy_hex())
        hdr.pack_end(copy_b)

        save_b = Gtk.Button(icon_name="document-save-symbolic", tooltip_text="Export Raw Binary…")
        save_b.connect("clicked", lambda _b: self._export_raw())
        hdr.pack_end(save_b)
        top_wrap.append(hdr)

        self.search_entry = Gtk.SearchEntry(placeholder_text="Find hex or ASCII in current page…", hexpand=True)
        self.search_entry.connect("search-changed", lambda _e: self._highlight_search())
        self.match_lbl = Gtk.Label(label="", valign=Gtk.Align.CENTER)
        self.match_lbl.add_css_class("caption")
        self.match_lbl.add_css_class("dim-label")
        sbox = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        sbox.append(self.search_entry)
        sbox.append(self.match_lbl)
        self.search_bar = Gtk.SearchBar(child=sbox, show_close_button=True)
        self.search_bar.connect_entry(self.search_entry)
        self.search_toggle.bind_property("active", self.search_bar, "search-mode-enabled", 2)
        top_wrap.append(self.search_bar)
        self._toolbar_view.add_top_bar(top_wrap)

        # === CENTER NATIVE ADWAITA MONOSPACE HEX VIEW ===
        self.stack = Gtk.Stack(vexpand=True, hexpand=True)

        content_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=0, hexpand=True, vexpand=True)
        ruler = Gtk.Label(
            label="OFFSET     00 01 02 03 04 05 06 07  08 09 0A 0B 0C 0D 0E 0F   ASCII",
            xalign=0,
            margin_start=14,
            margin_end=14,
            margin_top=6,
            margin_bottom=6,
        )
        ruler.add_css_class("monospace")
        ruler.add_css_class("caption")
        ruler.add_css_class("dim-label")
        content_box.append(ruler)
        content_box.append(Gtk.Separator(orientation=Gtk.Orientation.HORIZONTAL))

        scroll = Gtk.ScrolledWindow(vexpand=True, hexpand=True)
        self.buf = Gtk.TextBuffer()
        self.buf.create_tag("dim", foreground_rgba=Gdk.RGBA(0.55, 0.58, 0.64, 0.72))
        self.buf.create_tag("hdr", weight=700)
        self.buf.create_tag("match", background_rgba=Gdk.RGBA(0.21, 0.52, 0.89, 0.45))
        self.buf.connect("mark-set", self._on_cursor_moved)

        self.tv = Gtk.TextView(
            buffer=self.buf,
            editable=False,
            monospace=True,
            cursor_visible=True,
            top_margin=8,
            bottom_margin=10,
            left_margin=14,
            right_margin=14,
        )
        self.tv.set_wrap_mode(Gtk.WrapMode.NONE)
        scroll.set_child(self.tv)
        content_box.append(scroll)
        self.stack.add_named(content_box, "hex")

        loading_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=10, halign=Gtk.Align.CENTER, valign=Gtk.Align.CENTER)
        self.spinner = Adw.Spinner() if hasattr(Adw, "Spinner") else Gtk.Spinner(spinning=True)
        loading_box.append(self.spinner)
        self.loading_lbl = Gtk.Label(label="Reading chunk…")
        self.loading_lbl.add_css_class("dim-label")
        loading_box.append(self.loading_lbl)
        self.stack.add_named(loading_box, "loading")

        self._toolbar_view.set_content(self.stack)

        # === BOTTOM TOOLBAR: PAGINATION + LIVE BYTE DECODER ===
        bot_wrap = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        bot_wrap.append(Gtk.Separator(orientation=Gtk.Orientation.HORIZONTAL))
        foot = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=10)
        foot.add_css_class("toolbar")

        nav = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL)
        nav.add_css_class("linked")
        self.prev_pg_btn = Gtk.Button(icon_name="go-previous-symbolic", tooltip_text="Previous 2 KB page")
        self.prev_pg_btn.connect("clicked", lambda _b: self._load_page(max(0, self._page_offset - _PAGE_BYTES)))
        self.next_pg_btn = Gtk.Button(icon_name="go-next-symbolic", tooltip_text="Next 2 KB page")
        self.next_pg_btn.connect("clicked", lambda _b: self._load_page(self._page_offset + _PAGE_BYTES))
        nav.append(self.prev_pg_btn)
        nav.append(self.next_pg_btn)
        foot.append(nav)

        self.page_lbl = Gtk.Label(label="Page 1/1", xalign=0)
        self.page_lbl.add_css_class("caption")
        self.page_lbl.add_css_class("dim-label")
        self.page_lbl.add_css_class("numeric")
        foot.append(self.page_lbl)

        foot.append(Gtk.Box(hexpand=True))

        self.insp_lbl = Gtk.Label(label="Click any byte to inspect", xalign=1, ellipsize=3)
        self.insp_lbl.add_css_class("caption")
        self.insp_lbl.add_css_class("dim-label")
        self.insp_lbl.add_css_class("monospace")
        foot.append(self.insp_lbl)

        bot_wrap.append(foot)
        self._toolbar_view.add_bottom_bar(bot_wrap)

        kc = Gtk.EventControllerKey()
        kc.connect("key-pressed", self._on_key)
        self.add_controller(kc)

        if remote_loader is not None:
            self._load_page(0)
        else:
            self.set_data(data, title, header_highlight=header_highlight)

    def _on_key(self, _c: Any, keyval: int, _kc: int, state: Any) -> bool:
        ctrl = bool(state & Gdk.ModifierType.CONTROL_MASK)
        if ctrl and keyval in (Gdk.KEY_f, Gdk.KEY_F):
            self.search_toggle.set_active(not self.search_toggle.get_active())
            if self.search_toggle.get_active():
                self.search_entry.grab_focus()
            return True
        return False

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

    def set_data(self, data: bytes, title: str = "Binary", *, header_highlight: int | None = None) -> None:
        self._data = data
        self._title = title
        self._total_size = len(data)
        self._header_len = self._infer_header_len(data, header_highlight)
        self._win_title.set_title(Path(title).name or title)
        self._win_title.set_subtitle(f"{detect_binary_format(data, title)} · {fmt_size(len(data))}")
        self._load_page(0)

    def _load_page(self, offset: int) -> None:
        page_off = (max(0, offset) // _PAGE_BYTES) * _PAGE_BYTES
        if self._total_size > 0 and page_off >= self._total_size:
            page_off = ((self._total_size - 1) // _PAGE_BYTES) * _PAGE_BYTES

        if self._remote_loader is None:
            self._page_offset = page_off
            self._page_bytes = self._data[page_off : page_off + _PAGE_BYTES]
            self._render_current_page()
            return

        if page_off in self._page_cache:
            self._page_offset = page_off
            self._page_bytes = self._page_cache[page_off]
            self._render_current_page()
            return

        if self._loading:
            return
        self._loading = True
        self.loading_lbl.set_text(f"Reading 0x{page_off:04X}…")
        self.stack.set_visible_child_name("loading")
        loader = self._remote_loader

        def _w() -> None:
            try:
                chunk, total = loader(page_off, _PAGE_BYTES)
                err = ""
            except Exception as exc:
                chunk, total, err = b"", self._total_size, str(exc)

            def _done() -> bool:
                self._loading = False
                if err:
                    self._win_title.set_subtitle(f"Read failed: {err}")
                    self._page_bytes = b""
                    self._render_current_page()
                    return False
                self._total_size = total
                self._page_offset = page_off
                self._page_bytes = chunk
                self._page_cache[page_off] = chunk
                if page_off == 0:
                    self._header_len = self._infer_header_len(chunk, self._explicit_hdr)
                    self._win_title.set_subtitle(f"{detect_binary_format(chunk, self._title)} · {fmt_size(total)}")
                self._render_current_page()
                return False

            GLib.idle_add(_done)

        threading.Thread(target=_w, daemon=True).start()

    def _render_current_page(self) -> None:
        self.stack.set_visible_child_name("hex")
        chunk = self._page_bytes
        base = self._page_offset
        if not chunk:
            self.buf.set_text("(empty)")
        else:
            text = _format_page_lines(chunk, base)
            self.buf.set_text(text)
            # Apply lightweight line-level tags (1 tag per line for offset column + header lines -> instant!)
            n_lines = (len(chunk) + 15) // 16
            for li in range(n_lines):
                ok_s, it_s = self.buf.get_iter_at_line(li)
                if not ok_s:
                    break
                it_off_end = it_s.copy()
                it_off_end.forward_chars(8)
                self.buf.apply_tag_by_name("dim", it_s, it_off_end)
                if base + li * 16 < self._header_len:
                    it_hdr_s = it_s.copy()
                    it_hdr_s.forward_chars(11)
                    it_hdr_e = it_s.copy()
                    it_hdr_e.forward_to_line_end()
                    self.buf.apply_tag_by_name("hdr", it_hdr_s, it_hdr_e)

        total_pages = max(1, (self._total_size + _PAGE_BYTES - 1) // _PAGE_BYTES)
        cur_page = (base // _PAGE_BYTES) + 1
        end_off = max(base, base + len(chunk) - 1)
        self.page_lbl.set_text(f"Page {cur_page}/{total_pages} · 0x{base:04X}–0x{end_off:04X}")
        self.prev_pg_btn.set_sensitive(base > 0)
        self.next_pg_btn.set_sensitive(base + _PAGE_BYTES < self._total_size)
        self._update_inspector(0 if chunk else -1)
        self._highlight_search()

    def _on_cursor_moved(self, _buf: Any, loc: Any, mark: Any) -> None:
        if mark != self.buf.get_insert() or not self._page_bytes:
            return
        line = loc.get_line()
        col = loc.get_line_offset()
        rel_base = line * 16
        if rel_base >= len(self._page_bytes):
            return
        byte_in_row = 0
        if col >= 62:
            byte_in_row = max(0, min(15, col - 62))
        elif col >= 11:
            line_text = self.buf.get_text(self.buf.get_iter_at_line(line)[1], loc, False)
            hex_part = line_text[11:59].replace(" ", "")
            byte_in_row = max(0, min(15, len(hex_part) // 2))
        self._update_inspector(min(len(self._page_bytes) - 1, rel_base + byte_in_row))

    def _update_inspector(self, rel_offset: int) -> None:
        if rel_offset < 0 or rel_offset >= len(self._page_bytes):
            self.insp_lbl.set_text("—")
            return
        vals = decode_inspector_values(self._page_bytes, rel_offset)
        abs_off = self._page_offset + rel_offset
        self.insp_lbl.set_text(
            f"0x{abs_off:06X} · u8 {vals['u8']} · u16 {vals['u16']} · u32 {vals['u32']} · {vals['char']}"
        )

    def _jump_to_offset(self) -> None:
        raw = self.goto_entry.get_text().strip()
        if not raw:
            return
        try:
            off = int(raw, 16) if raw.lower().startswith("0x") else int(raw, 16 if any(c in "abcdefABCDEF" for c in raw) else 10)
        except ValueError:
            return
        off = max(0, min(max(0, self._total_size - 1), off))
        target_page = (off // _PAGE_BYTES) * _PAGE_BYTES
        if target_page != self._page_offset:
            self._load_page(target_page)
        rel = off - target_page
        line = min(rel // 16, max(0, self.buf.get_line_count() - 1))
        ok, it = self.buf.get_iter_at_line(line)
        if ok:
            self.buf.place_cursor(it)
            self.tv.scroll_to_iter(it, 0.15, False, 0.0, 0.0)
            self._update_inspector(rel)

    def _highlight_search(self) -> None:
        s_it, e_it = self.buf.get_bounds()
        self.buf.remove_tag_by_name("match", s_it, e_it)
        q = self.search_entry.get_text().strip()
        if not q:
            self.match_lbl.set_text("")
            return
        text = self.buf.get_text(s_it, e_it, False)
        low_t, low_q = text.lower(), q.lower()
        start = count = 0
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
        dlg = Gtk.FileDialog(title=f"Export {Path(self._title).name or 'binary.bin'}")
        dlg.set_initial_name(Path(self._title).name or "dump.bin")

        def _picked(d: Any, res: Any) -> None:
            try:
                dest = d.save_finish(res).get_path()
            except GLib.Error:
                return
            if not dest:
                return
            if self._full_downloader is not None:
                dl = self._full_downloader
                threading.Thread(target=lambda: Path(dest).write_bytes(dl()), daemon=True).start()
            elif self._data:
                Path(dest).write_bytes(self._data)
            elif self._page_bytes:
                Path(dest).write_bytes(self._page_bytes)

        dlg.save(self._parent_win, None, _picked)


def show_hex_viewer_dialog(parent: Any, data: bytes, title: str, *, header_highlight: int | None = None) -> None:
    """Present a native Adw.Dialog hosting the paginated Hex + ASCII Inspector."""
    dlg = Adw.Dialog(title=Path(title).name or title, content_width=760, content_height=520)
    dlg.set_child(HexViewWidget(data, title, header_highlight=header_highlight, parent_win=parent))
    dlg.present(parent)


def show_remote_hex_viewer_dialog(parent: Any, fs_backend: Any, remote_path: str, title: str) -> None:
    """Open the Hex Inspector instantly, lazily streaming only visible 2 KB pages from KernFS."""
    dlg = Adw.Dialog(title=Path(title).name or title, content_width=760, content_height=520)
    dlg.set_child(
        HexViewWidget(
            b"",
            title,
            parent_win=parent,
            remote_loader=lambda off, ln: fs_backend.read_range(remote_path, off, ln),
            full_downloader=lambda: fs_backend.read_bytes(remote_path),
        )
    )
    dlg.present(parent)
