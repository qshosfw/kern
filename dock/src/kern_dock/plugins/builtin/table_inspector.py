"""Built-in Plugin: Binary Storage Table Inspector (`/user/*.ch`, `.ct`, `.zn`, `.vfo`)."""

from __future__ import annotations

import struct
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
    for idx in range(min(n_recs, 256)):
        chunk = payload[idx * eff_stride : (idx + 1) * eff_stride]
        if not any(chunk):
            continue
        name = chunk[:16].split(b"\x00", 1)[0].decode("ascii", errors="ignore").strip()
        item: dict[str, Any] = {"index": idx, "offset": 16 + idx * eff_stride, "name": name or f"#{idx}"}
        if expected_fourcc == b"CHAN" and len(chunk) >= 26:
            rx_hz, tx_hz, mod, _fl = struct.unpack_from("<IIBB", chunk, 16)
            item["rx_mhz"] = f"{rx_hz / 1e6:.4f} MHz" if rx_hz else "—"
            item["tx_mhz"] = f"{tx_hz / 1e6:.4f} MHz" if tx_hz else "—"
            item["mod"] = mod
        decoded.append(item)
    return {
        "fourcc": fourcc.decode("ascii", errors="replace"),
        "version": ver,
        "stride": eff_stride,
        "header_count": count,
        "flags": flags,
        "records": decoded,
    }


class TableInspectorPlugin:
    manifest = PluginManifest(
        id="table-inspector",
        name="Table Inspector",
        version="2.0.0",
        description="Inspects KernFS binary tables (/user/channels.ch, contacts, zones) directly from the radio.",
        capabilities=("ui.page", "radio.read", "fs.read"),
        icon="document-properties-symbolic",
        builtin=True,
    )

    def __init__(self) -> None:
        self._ctx: PluginContext | None = None
        self._list_box: Any = None
        self._status_lbl: Any = None

    def activate(self, ctx: PluginContext) -> None:
        self._ctx = ctx
        ctx.register_page(
            "tables",
            "Tables",
            "document-properties-symbolic",
            self._build_page,
            order=30,
            subtitle="Inspect KernFS binary storage tables",
        )

    def deactivate(self) -> None:
        self._ctx = None
        self._list_box = None
        self._status_lbl = None

    def _inspect(self, path: str, fourcc: bytes, rec_sz: int) -> None:
        if self._ctx is None or self._list_box is None:
            return
        c = self._list_box.get_first_child()
        while c is not None:
            n = c.get_next_sibling()
            self._list_box.remove(c)
            c = n
        try:
            raw = self._ctx.read_kernfs(path)
            parsed = parse_binary_table(raw, fourcc, rec_sz)
        except Exception as exc:
            self._status_lbl.set_text(f"{path}: {exc}")
            return
        self._status_lbl.set_text(
            f"{path} · {parsed['fourcc']} v{parsed['version']} · {parsed['stride']} B stride · {len(parsed['records'])} active"
        )
        import gi

        gi.require_version("Adw", "1")
        from gi.repository import Adw

        for rec in parsed["records"]:
            sub = f"offset 0x{rec['offset']:04X}"
            if "rx_mhz" in rec:
                sub = f"RX {rec['rx_mhz']} · TX {rec['tx_mhz']} · {sub}"
            self._list_box.append(Adw.ActionRow(title=f"[{rec['index']:03d}] {rec['name']}", subtitle=sub))

    def _build_page(self) -> Any:
        import gi

        gi.require_version("Gtk", "4.0")
        gi.require_version("Adw", "1")
        from gi.repository import Adw, Gtk

        page = Adw.PreferencesPage()
        grp_sel = Adw.PreferencesGroup(title="Radio Tables", description="Select a KernFS table to decode its records")
        for path, fourcc, rec_sz, label in RADIO_TABLES:
            row = Adw.ActionRow(title=label, subtitle=path, activatable=True)
            row.add_suffix(Gtk.Image.new_from_icon_name("go-next-symbolic"))
            row.connect("activated", lambda _r, p=path, f=fourcc, s=rec_sz: self._inspect(p, f, s))
            grp_sel.add(row)
        page.add(grp_sel)

        grp_res = Adw.PreferencesGroup(title="Decoded Records")
        self._status_lbl = Gtk.Label(label="Choose a table above while connected to Kern OS", xalign=0, wrap=True)
        self._status_lbl.add_css_class("dim-label")
        self._status_lbl.add_css_class("caption")
        grp_res.add(self._status_lbl)
        self._list_box = Gtk.ListBox(selection_mode=Gtk.SelectionMode.NONE)
        self._list_box.add_css_class("boxed-list")
        grp_res.add(self._list_box)
        page.add(grp_res)
        return page
