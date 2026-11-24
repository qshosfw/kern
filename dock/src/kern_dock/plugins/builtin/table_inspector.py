"""Built-in Plugin: Binary Storage Table Inspector (`/user/*.ch`, `.ct`, `.zn`, `.bnd`, `.hit`, `.fm`)."""

from __future__ import annotations

import csv
import io
import struct
import threading
from pathlib import Path
from typing import Any

from ..api import PluginContext, PluginManifest

RADIO_TABLES: tuple[tuple[str, bytes, int, str], ...] = (
    ("/user/channels.ch", b"CHAN", 64, "Channels (64 B)"),
    ("/user/contacts.ct", b"CONT", 32, "Contacts (32 B)"),
    ("/user/zones.zn", b"ZONE", 32, "Zones (32 B)"),
    ("/user/bandplans.bnd", b"BNDP", 32, "Band Plans (32 B)"),
    ("/user/scanner.hit", b"HITS", 32, "Scanner Hits (32 B)"),
    ("/user/fm_radio.fm", b"FMBC", 16, "FM Stations (16 B)"),
)

_MOD_NAMES = {0: "FM", 1: "AM", 2: "USB", 3: "LSB", 4: "CW", 5: "NFM", 6: "FSK"}


def parse_binary_table(raw: bytes, expected_fourcc: bytes, record_size: int) -> dict[str, Any]:
    """Zero-copy table decoder verifying 16-byte header and power-of-two record geometry."""
    if len(raw) < 16:
        raise ValueError(f"Truncated table header ({len(raw)} B < 16 B)")
    fourcc, ver, stride, count, flags = struct.unpack_from("<4sHHII", raw, 0)
    if fourcc != expected_fourcc:
        raise ValueError(f"Format identifier mismatch: got {fourcc!r}, expected {expected_fourcc!r}")
    payload = raw[16:]
    eff_stride = stride if (stride and (stride & (stride - 1)) == 0) else record_size
    n_recs = len(payload) // eff_stride
    decoded: list[dict[str, Any]] = []
    for idx in range(min(n_recs, 512)):
        chunk = payload[idx * eff_stride : (idx + 1) * eff_stride]
        if not any(chunk):
            continue
        name_len = 8 if expected_fourcc == b"FMBC" else 16
        name = chunk[:name_len].split(b"\x00", 1)[0].decode("ascii", errors="ignore").strip()
        item: dict[str, Any] = {
            "index": idx,
            "offset": 16 + idx * eff_stride,
            "name": name or f"#{idx}",
            "raw": chunk,
            "detail": "",
        }
        if expected_fourcc == b"CHAN" and len(chunk) >= 26:
            rx_hz, tx_hz, mod, _fl = struct.unpack_from("<IIBB", chunk, 16)
            item["rx_mhz"] = f"{rx_hz / 1e6:.4f} MHz" if rx_hz else "—"
            item["tx_mhz"] = f"{tx_hz / 1e6:.4f} MHz" if tx_hz else "—"
            item["mod"] = mod
            mod_s = _MOD_NAMES.get(mod, f"M{mod}")
            item["detail"] = f"RX {item['rx_mhz']} · TX {item['tx_mhz']} · {mod_s}"
        elif expected_fourcc == b"CONT" and len(chunk) >= 24:
            cid, cflags = struct.unpack_from("<II", chunk, 16)
            item["detail"] = f"ID {cid} · flags 0x{cflags:04X}"
        elif expected_fourcc == b"ZONE" and len(chunk) >= 20:
            members, zflags = struct.unpack_from("<HH", chunk, 16)
            item["detail"] = f"{members} channel(s) · flags 0x{zflags:04X}"
        elif expected_fourcc == b"BNDP" and len(chunk) >= 24:
            lo_hz, hi_hz = struct.unpack_from("<II", chunk, 16)
            item["detail"] = f"{lo_hz / 1e6:.3f} – {hi_hz / 1e6:.3f} MHz" if (lo_hz or hi_hz) else "Unassigned"
        elif expected_fourcc == b"HITS" and len(chunk) >= 24:
            hz, hits = struct.unpack_from("<II", chunk, 16)
            item["detail"] = f"{hz / 1e6:.4f} MHz · {hits} hit(s)" if hz else "—"
        elif expected_fourcc == b"FMBC" and len(chunk) >= 12:
            khz = struct.unpack_from("<I", chunk, 8)[0]
            mhz = (khz / 1000.0) if khz > 10000 else (khz / 10.0)
            item["detail"] = f"{mhz:.1f} MHz" if khz else "—"
        decoded.append(item)
    return {
        "fourcc": fourcc.decode("ascii", errors="replace"),
        "version": ver,
        "stride": eff_stride,
        "header_count": count,
        "capacity": n_recs,
        "flags": flags,
        "records": decoded,
    }


def table_to_csv(parsed: dict[str, Any]) -> str:
    """Serialize decoded table records to CSV."""
    buf = io.StringIO()
    w = csv.writer(buf)
    fourcc = parsed.get("fourcc", "")
    if fourcc == "CHAN":
        w.writerow(["index", "offset", "name", "rx_mhz", "tx_mhz", "mod"])
        for r in parsed["records"]:
            w.writerow([r["index"], f"0x{r['offset']:04X}", r["name"], r.get("rx_mhz", ""), r.get("tx_mhz", ""), _MOD_NAMES.get(r.get("mod", 0), r.get("mod", ""))])
    else:
        w.writerow(["index", "offset", "name", "detail"])
        for r in parsed["records"]:
            w.writerow([r["index"], f"0x{r['offset']:04X}", r["name"], r.get("detail", "")])
    return buf.getvalue()


class TableInspectorPlugin:
    manifest = PluginManifest(
        id="table-inspector",
        name="Table Inspector",
        version="2.0.0",
        description="Inspects KernFS binary storage tables (/user/channels.ch, contacts, zones, bandplans, hits, FM) with search, Hex View and CSV export.",
        capabilities=("events.subscribe", "ui.page", "radio.read", "fs.read"),
        icon="document-properties-symbolic",
        builtin=True,
    )

    def __init__(self) -> None:
        self._ctx: PluginContext | None = None
        self._parsed: dict[str, Any] | None = None
        self._raw_bytes: bytes = b""
        self._cur_table_idx: int = 0
        self._list_box: Any = None
        self._stack: Any = None
        self._empty: Any = None
        self._meta_lbl: Any = None
        self._search_entry: Any = None
        self._hex_btn: Any = None
        self._csv_btn: Any = None

    def activate(self, ctx: PluginContext) -> None:
        self._ctx = ctx
        ctx.subscribe("link.connected", lambda _e: self._load_selected(), main_thread=True)
        ctx.subscribe("link.disconnected", lambda _e: self._set_offline(), main_thread=True)
        ctx.register_page(
            "tables",
            "Tables",
            "document-properties-symbolic",
            self._build_page,
            order=35,
            subtitle="Inspect KernFS binary storage tables & export CSV",
        )

    def deactivate(self) -> None:
        self._ctx = None
        self._list_box = None
        self._stack = None

    def _set_offline(self) -> None:
        self._parsed = None
        self._raw_bytes = b""
        if self._hex_btn is not None:
            self._hex_btn.set_sensitive(False)
            self._csv_btn.set_sensitive(False)
        if self._meta_lbl is not None:
            self._meta_lbl.set_text("Offline")
        if self._empty is not None and self._stack is not None:
            self._empty.set_title("No Radio Connected")
            self._empty.set_description("Connect a RigWire radio to inspect KernFS binary tables.")
            self._stack.set_visible_child_name("empty")

    def _load_selected(self) -> None:
        if self._ctx is None or self._stack is None:
            return
        lk = self._ctx.link
        if not lk.connected or lk.link_mode != "rigwire":
            self._set_offline()
            return
        path, fourcc, rec_sz, label = RADIO_TABLES[self._cur_table_idx]
        if self._meta_lbl is not None:
            self._meta_lbl.set_text(f"Reading {path}…")

        def _w() -> None:
            try:
                raw = self._ctx.read_kernfs(path) if self._ctx else b""
                parsed = parse_binary_table(raw, fourcc, rec_sz)
                err = ""
            except Exception as exc:
                raw = b""
                parsed = None
                err = str(exc)

            from gi.repository import GLib

            def _apply() -> bool:
                self._raw_bytes = raw
                self._parsed = parsed
                if self._hex_btn is not None:
                    self._hex_btn.set_sensitive(bool(raw))
                    self._csv_btn.set_sensitive(bool(parsed and parsed["records"]))
                if err or parsed is None:
                    if self._meta_lbl is not None:
                        self._meta_lbl.set_text(f"{path} · unavailable")
                    if self._empty is not None and self._stack is not None:
                        self._empty.set_title(label)
                        self._empty.set_description(err or f"{path} not found on radio")
                        self._stack.set_visible_child_name("empty")
                    return False
                if self._meta_lbl is not None:
                    self._meta_lbl.set_text(
                        f"{path} · {parsed['fourcc']} v{parsed['version']} · {parsed['stride']} B stride · {len(parsed['records'])}/{parsed['capacity']} active"
                    )
                self._render_records()
                return False

            GLib.idle_add(_apply)

        threading.Thread(target=_w, daemon=True).start()

    def _render_records(self) -> None:
        if self._list_box is None or self._stack is None or self._parsed is None:
            return
        import gi

        gi.require_version("Adw", "1")
        gi.require_version("Gtk", "4.0")
        from gi.repository import Adw, GLib, Gtk

        c = self._list_box.get_first_child()
        while c is not None:
            n = c.get_next_sibling()
            self._list_box.remove(c)
            c = n

        q = self._search_entry.get_text().strip().lower() if self._search_entry is not None else ""
        recs = [
            r for r in self._parsed["records"]
            if not q or q in r["name"].lower() or q in r.get("detail", "").lower() or q in str(r["index"])
        ]
        if not recs:
            self._empty.set_title("No Records")
            self._empty.set_description(f'No rows match "{q}".' if q else "This table has no populated records.")
            self._stack.set_visible_child_name("empty")
            return

        self._stack.set_visible_child_name("list")
        for rec in recs:
            off_s = f"0x{rec['offset']:04X}"
            sub = f"{rec['detail']} · {off_s}" if rec.get("detail") else off_s
            row = Adw.ActionRow(
                title=GLib.markup_escape_text(f"[{rec['index']:03d}] {rec['name']}"),
                subtitle=GLib.markup_escape_text(sub),
                activatable=True,
            )
            row.add_suffix(Gtk.Image(icon_name="accessories-text-editor-symbolic", tooltip_text="Inspect Record Hex"))
            row.connect("activated", lambda _r, d=rec["raw"], t=f"{rec['name']} ({off_s})": self._show_hex(d, t))
            self._list_box.append(row)

    def _show_hex(self, data: bytes, title: str) -> None:
        if not data or self._list_box is None:
            return
        from ...ui.widgets.hex_view import show_hex_viewer_dialog

        win = self._list_box.get_root()
        show_hex_viewer_dialog(win, data, title)

    def _export_csv(self) -> None:
        if not self._parsed or self._list_box is None:
            return
        from gi.repository import GLib, Gtk

        path = RADIO_TABLES[self._cur_table_idx][0]
        stem = Path(path).stem
        dlg = Gtk.FileDialog(title=f"Export {stem}.csv")
        dlg.set_initial_name(f"{stem}.csv")
        csv_text = table_to_csv(self._parsed)

        def _done(d: Any, res: Any) -> None:
            try:
                dest = d.save_finish(res).get_path()
                if dest:
                    Path(dest).write_text(csv_text, encoding="utf-8")
                    if self._ctx:
                        self._ctx.toast(f"Exported {Path(dest).name}")
            except GLib.Error:
                pass

        dlg.save(self._list_box.get_root(), None, _done)

    def _build_page(self) -> Any:
        import gi

        gi.require_version("Gtk", "4.0")
        gi.require_version("Adw", "1")
        from gi.repository import Adw, Gtk

        view = Adw.ToolbarView()

        top_wrap = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        bar = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=6)
        bar.add_css_class("toolbar")

        dd = Gtk.DropDown.new_from_strings([t[3] for t in RADIO_TABLES])
        dd.connect("notify::selected", lambda d, _p: (setattr(self, "_cur_table_idx", int(d.get_selected())), self._load_selected()))
        bar.append(dd)

        ref_b = Gtk.Button(icon_name="view-refresh-symbolic", tooltip_text="Reload table")
        ref_b.add_css_class("flat")
        ref_b.connect("clicked", lambda _b: self._load_selected())
        bar.append(ref_b)

        bar.append(Gtk.Box(hexpand=True))

        srch_t = Gtk.ToggleButton(icon_name="system-search-symbolic", tooltip_text="Filter records")
        srch_t.add_css_class("flat")
        bar.append(srch_t)

        self._hex_btn = Gtk.Button(icon_name="accessories-text-editor-symbolic", tooltip_text="Inspect Full Table Hex", sensitive=False)
        self._hex_btn.add_css_class("flat")
        self._hex_btn.connect("clicked", lambda _b: self._show_hex(self._raw_bytes, Path(RADIO_TABLES[self._cur_table_idx][0]).name))
        bar.append(self._hex_btn)

        self._csv_btn = Gtk.Button(icon_name="document-save-symbolic", tooltip_text="Export CSV…", sensitive=False)
        self._csv_btn.add_css_class("flat")
        self._csv_btn.connect("clicked", lambda _b: self._export_csv())
        bar.append(self._csv_btn)
        top_wrap.append(bar)

        self._search_entry = Gtk.SearchEntry(placeholder_text="Filter by name, index, or frequency…", hexpand=True)
        self._search_entry.connect("search-changed", lambda _e: self._render_records())
        sbar = Gtk.SearchBar(child=self._search_entry, show_close_button=True)
        sbar.connect_entry(self._search_entry)
        srch_t.bind_property("active", sbar, "search-mode-enabled", 2)
        top_wrap.append(sbar)
        top_wrap.append(Gtk.Separator(orientation=Gtk.Orientation.HORIZONTAL))
        view.add_top_bar(top_wrap)

        self._stack = Gtk.Stack(vexpand=True, hexpand=True)
        scroll = Gtk.ScrolledWindow(vexpand=True)
        scroll.set_policy(Gtk.PolicyType.NEVER, Gtk.PolicyType.AUTOMATIC)
        self._list_box = Gtk.ListBox(selection_mode=Gtk.SelectionMode.NONE, valign=Gtk.Align.START)
        self._list_box.add_css_class("boxed-list")
        clamp = Adw.Clamp(maximum_size=640, margin_top=12, margin_bottom=14, margin_start=12, margin_end=12, child=self._list_box)
        scroll.set_child(clamp)
        self._stack.add_named(scroll, "list")

        self._empty = Adw.StatusPage(
            icon_name="document-properties-symbolic",
            title="No Radio Connected",
            description="Connect a RigWire radio to inspect KernFS binary tables.",
        )
        self._empty.add_css_class("compact")
        self._stack.add_named(self._empty, "empty")
        self._stack.set_visible_child_name("empty")
        view.set_content(self._stack)

        bot_wrap = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        bot_wrap.append(Gtk.Separator(orientation=Gtk.Orientation.HORIZONTAL))
        foot = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8, margin_top=5, margin_bottom=5, margin_start=12, margin_end=12)
        self._meta_lbl = Gtk.Label(label="Offline", xalign=0, hexpand=True, ellipsize=3)
        self._meta_lbl.add_css_class("caption")
        self._meta_lbl.add_css_class("dim-label")
        self._meta_lbl.add_css_class("monospace")
        foot.append(self._meta_lbl)
        bot_wrap.append(foot)
        view.add_bottom_bar(bot_wrap)

        view.connect("map", lambda _w: self._load_selected())
        return view
