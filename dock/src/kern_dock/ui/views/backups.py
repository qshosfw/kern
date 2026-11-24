"""Backups View: native Libadwaita snapshot creator (.kfw, .zip, calib) & archive inspector."""

from __future__ import annotations

import re
import threading
import time
from pathlib import Path
from typing import Any, Callable

import gi

gi.require_version("Gtk", "4.0")
gi.require_version("Gdk", "4.0")
gi.require_version("Adw", "1")
from gi.repository import Adw, Gdk, Gio, GLib, Gtk  # noqa: E402

from ...device.firmware import (
    build_kfw_backup_from_backend,
    build_zip_backup_from_backend,
    inspect_backup_file,
    restore_backup_to_backend,
)
from ..runtime import DockRuntime
from ..widgets.app_icon import inspect_krx_metadata, make_app_icon_widget
from ..widgets.hex_view import detect_binary_format, show_hex_viewer_dialog
from ..widgets.util import fmt_size


class BackupsView:
    """Native Libadwaita backup creator, verified archive inspector & selective extractor."""

    def __init__(
        self,
        win: Adw.ApplicationWindow,
        runtime: DockRuntime,
        *,
        on_toast: Callable[[str], None],
        on_confirm: Callable[[str, str, str, Callable[[], None]], None],
        on_progress: Callable[[bool, float, str], None],
    ) -> None:
        self.win = win
        self.runtime = runtime
        self._on_toast = on_toast
        self._on_confirm = on_confirm
        self._on_progress = on_progress
        self._loaded_backup: dict[str, Any] | None = None
        self._busy: bool = False
        self._filter_q: str = ""

        self.widget = self._build()

        runtime.events.subscribe("link.connected", lambda _e: self._sync_sensitivity(), main_thread=True)
        runtime.events.subscribe("link.disconnected", lambda _e: self._sync_sensitivity(), main_thread=True)
        runtime.events.subscribe("security.changed", lambda _e: self._sync_sensitivity(), main_thread=True)
        self._sync_sensitivity()

    def _build(self) -> Gtk.Box:
        root = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        page = Adw.PreferencesPage()

        # === 1. CREATE BACKUP ===
        grp_new = Adw.PreferencesGroup(
            title="Create Backup",
            description="Snapshot KernFS files or factory calibration from the connected radio",
        )
        self._online_rows: list[Any] = []
        for title, sub, icon, fmt, calib_only in (
            ("Full Snapshot (.kfw)", "Ascon-Hash256 verified container (/user, /pref, /apps, /sys)", "drive-harddisk-symbolic", "kfw", False),
            ("Portable Archive (.zip)", "Standard ZIP archive with JSON manifest", "package-x-generic-symbolic", "zip", False),
            ("Factory Calibration (.kfw)", "Signed /sys/calib.dat &amp; /sys/calib.sig", "security-high-symbolic", "kfw", True),
            ("Raw Calibration (.dat)", "Exact 512-byte calibration dump", "document-save-symbolic", "raw", True),
        ):
            row = Adw.ActionRow(title=title, subtitle=sub, activatable=True)
            row.add_prefix(Gtk.Image(icon_name=icon))
            row.add_suffix(Gtk.Image(icon_name="go-next-symbolic"))
            row.connect("activated", lambda _r, f=fmt, c=calib_only: self.run_backup(fmt=f, calib_only=c))
            grp_new.add(row)
            self._online_rows.append(row)
        page.add(grp_new)

        # === 2. INSPECT & RESTORE ===
        grp_ins = Adw.PreferencesGroup(title="Inspect &amp; Restore")
        self.inspect_hdr = Adw.ActionRow(title="No Backup Open", subtitle="Open or drop a .kfw, .zip, or .dat file")
        self.inspect_hdr.add_prefix(Gtk.Image(icon_name="folder-open-symbolic"))

        open_b = Gtk.Button(label="Open…", valign=Gtk.Align.CENTER)
        open_b.connect("clicked", lambda _b: self.open_inspect_picker())
        self.inspect_hdr.add_suffix(open_b)

        self.restore_btn = Gtk.Button(label="Restore…", valign=Gtk.Align.CENTER, visible=False)
        self.restore_btn.add_css_class("suggested-action")
        self.restore_btn.connect("clicked", lambda _b: self._confirm_restore_loaded())
        self.inspect_hdr.add_suffix(self.restore_btn)

        self.close_btn = Gtk.Button(icon_name="window-close-symbolic", valign=Gtk.Align.CENTER, tooltip_text="Close Archive", visible=False)
        self.close_btn.add_css_class("flat")
        self.close_btn.connect("clicked", lambda _b: self._close_loaded())
        self.inspect_hdr.add_suffix(self.close_btn)
        grp_ins.add(self.inspect_hdr)

        self.prop_fmt = Adw.ActionRow(title="Format &amp; Integrity", subtitle="", visible=False)
        self.prop_fmt.add_css_class("property")
        grp_ins.add(self.prop_fmt)

        self.prop_summary = Adw.ActionRow(title="Contents", subtitle="", visible=False)
        self.prop_summary.add_css_class("property")
        grp_ins.add(self.prop_summary)
        page.add(grp_ins)

        # === 3. ARCHIVE CONTENTS ===
        self.entries_group = Adw.PreferencesGroup(title="Archive Contents", visible=False)
        extract_all = Gtk.Button(label="Extract All…", valign=Gtk.Align.CENTER)
        extract_all.connect("clicked", lambda _b: self._extract_all())
        self.entries_group.set_header_suffix(extract_all)

        self.filter_entry = Gtk.SearchEntry(placeholder_text="Filter archive files…", margin_bottom=8, visible=False)
        self.filter_entry.connect("search-changed", self._on_filter_changed)
        self.entries_group.add(self.filter_entry)

        self.entries_list = Gtk.ListBox(selection_mode=Gtk.SelectionMode.NONE)
        self.entries_list.add_css_class("boxed-list")
        self.entries_group.add(self.entries_list)
        page.add(self.entries_group)

        root.append(page)
        drop = Gtk.DropTarget.new(Gio.File, Gdk.DragAction.COPY)
        drop.connect("enter", lambda *_a: (root.add_css_class("drop-active"), Gdk.DragAction.COPY)[1])
        drop.connect("leave", lambda *_a: root.remove_css_class("drop-active"))
        drop.connect("drop", self._on_drop)
        root.add_controller(drop)
        return root

    def _on_drop(self, _t: Any, value: Any, _x: float, _y: float) -> bool:
        self.widget.remove_css_class("drop-active")
        if isinstance(value, Gio.File) and value.get_path():
            self.load_backup_file(Path(value.get_path()))
            return True
        return False

    def _on_filter_changed(self, entry: Gtk.SearchEntry) -> None:
        self._filter_q = entry.get_text().strip().lower()
        self._render_entries()

    def _close_loaded(self) -> None:
        self._loaded_backup = None
        self._filter_q = ""
        self.filter_entry.set_text("")
        self.inspect_hdr.set_title("No Backup Open")
        self.inspect_hdr.set_subtitle("Open or drop a .kfw, .zip, or .dat file")
        for w in (self.restore_btn, self.close_btn, self.prop_fmt, self.prop_summary, self.entries_group):
            w.set_visible(False)
        self._sync_sensitivity()

    def _sync_sensitivity(self) -> None:
        lk = self.runtime.link
        online = lk.connected and lk.link_mode == "rigwire" and lk.fs_backend is not None and not self._busy
        for r in self._online_rows:
            r.set_sensitive(online)
        can_restore = bool(
            online
            and self._loaded_backup is not None
            and not self.runtime.security.policy.read_only
            and not self.runtime.security.hardened
            and self._loaded_backup["format"] in ("KFW (Ascon-Hash256)", "ZIP Archive")
        )
        self.restore_btn.set_sensitive(can_restore)

    def run_backup(self, *, fmt: str, calib_only: bool) -> None:
        if self._busy:
            self._on_toast("A backup or restore is already in progress")
            return
        lk = self.runtime.link
        if not lk.connected or lk.fs_backend is None:
            self._on_toast("Connect a RigWire radio first")
            return
        tag = "calib" if calib_only else "kernfs"
        ext = "dat" if fmt == "raw" else fmt
        slug = re.sub(r"[^a-z0-9._-]+", "_", lk.volume_name.lower()).strip("_") or "radio"
        dlg = Gtk.FileDialog(title=f"Save .{ext} Backup")
        dlg.set_initial_name(f"{slug}_{tag}_{time.strftime('%Y%m%d_%H%M%S')}.{ext}")
        be, vname = lk.fs_backend, lk.volume_name

        def _picked(d: Any, res: Any) -> None:
            try:
                dest = d.save_finish(res).get_path()
            except GLib.Error:
                return
            if not dest:
                return
            self._busy = True
            self._sync_sensitivity()
            self._on_progress(True, 0.0, "Reading radio files…")

            def _w() -> None:
                try:
                    if fmt == "raw":
                        blob = be.read_bytes("/sys/calib.dat")
                    elif fmt == "zip":
                        blob = build_zip_backup_from_backend(
                            be,
                            volume_name=vname,
                            calib_only=calib_only,
                            progress_cb=lambda f, m: GLib.idle_add(self._on_progress, True, f, m),
                        )
                    else:
                        blob = build_kfw_backup_from_backend(
                            be,
                            volume_name=vname,
                            calib_only=calib_only,
                            progress_cb=lambda f, m: GLib.idle_add(self._on_progress, True, f, m),
                        )
                    out_p = Path(dest)
                    out_p.write_bytes(blob)
                    GLib.idle_add(self._on_toast, f"Saved {out_p.name} ({fmt_size(len(blob))})")
                    GLib.idle_add(self.load_backup_file, out_p)
                except Exception as exc:
                    GLib.idle_add(self._on_toast, f"Backup failed: {exc}")
                finally:
                    def _done() -> bool:
                        self._busy = False
                        self._on_progress(False, 0.0, "")
                        self._sync_sensitivity()
                        return False

                    GLib.idle_add(_done)

            threading.Thread(target=_w, daemon=True).start()

        dlg.save(self.win, None, _picked)

    def open_inspect_picker(self) -> None:
        dlg = Gtk.FileDialog(title="Open Backup (.kfw, .zip, or .dat)")

        def _picked(d: Any, res: Any) -> None:
            try:
                src = d.open_finish(res).get_path()
            except GLib.Error:
                return
            if src:
                self.load_backup_file(Path(src))

        dlg.open(self.win, None, _picked)

    def load_backup_file(self, fpath: Path) -> None:
        try:
            info = inspect_backup_file(fpath)
        except Exception as exc:
            self._on_toast(f"Could not read backup: {exc}")
            return
        self._loaded_backup = info
        entries = info["entries"]
        total_b = sum(int(e["size"]) for e in entries)
        prot_n = sum(1 for e in entries if e.get("protected"))
        ts = f" · {info['timestamp']}" if info.get("timestamp") else ""

        self.inspect_hdr.set_title(GLib.markup_escape_text(Path(info["path"]).name))
        self.inspect_hdr.set_subtitle(GLib.markup_escape_text(str(Path(info["path"]).parent)))
        self.prop_fmt.set_subtitle(GLib.markup_escape_text(f"{info['format']}{ts}"))
        prot_s = f" · {prot_n} protected /sys" if prot_n else ""
        self.prop_summary.set_subtitle(GLib.markup_escape_text(f"{len(entries)} file(s) · {fmt_size(total_b)}{prot_s}"))

        self.prop_fmt.set_visible(True)
        self.prop_summary.set_visible(True)
        self.close_btn.set_visible(True)
        self.restore_btn.set_visible(info["format"] in ("KFW (Ascon-Hash256)", "ZIP Archive"))
        self.filter_entry.set_visible(len(entries) > 6)
        self.entries_group.set_visible(bool(entries))
        self._render_entries()
        self._sync_sensitivity()

    def _render_entries(self) -> None:
        c = self.entries_list.get_first_child()
        while c is not None:
            n = c.get_next_sibling()
            self.entries_list.remove(c)
            c = n
        if not self._loaded_backup:
            return
        q = self._filter_q
        for e in sorted(self._loaded_backup["entries"], key=lambda x: str(x["path"])):
            rpath = str(e["path"])
            if q and q not in rpath.lower():
                continue
            data = bytes(e["data"])
            fmt_tag = detect_binary_format(data, rpath)
            row = Adw.ActionRow(
                title=GLib.markup_escape_text(rpath),
                subtitle=GLib.markup_escape_text(f"{fmt_tag} · {fmt_size(len(data))}"),
            )
            if rpath.startswith("/apps/") or rpath.endswith((".krx", ".mod")):
                meta = inspect_krx_metadata(data, Path(rpath).name)
                row.add_prefix(
                    make_app_icon_widget(
                        meta["name"],
                        exec_type=meta["exec_type"],
                        icon_asset=int(meta.get("icon_asset", 0) or 0),
                        bitmap=meta.get("bitmap"),
                        size=28,
                    )
                )
            if e.get("protected"):
                lock = Gtk.Image(icon_name="security-high-symbolic", valign=Gtk.Align.CENTER, tooltip_text="Protected /sys file (shielded on restore)")
                lock.add_css_class("dim-label")
                row.add_suffix(lock)

            hex_b = Gtk.Button(icon_name="accessories-text-editor-symbolic", valign=Gtk.Align.CENTER, tooltip_text=f"Inspect Hex ({rpath})")
            hex_b.add_css_class("flat")
            hex_b.connect("clicked", lambda _b, d=data, p=rpath: show_hex_viewer_dialog(self.win, d, p))
            row.add_suffix(hex_b)

            ex_b = Gtk.Button(icon_name="document-save-symbolic", valign=Gtk.Align.CENTER, tooltip_text=f"Extract {Path(rpath).name}…")
            ex_b.add_css_class("flat")
            ex_b.connect("clicked", lambda _b, d=data, n=Path(rpath).name: self._extract_entry(d, n))
            row.add_suffix(ex_b)

            self.entries_list.append(row)

    def _extract_all(self) -> None:
        if not self._loaded_backup:
            return
        dlg = Gtk.FileDialog(title="Choose Folder to Extract Backup")

        def _picked(d: Any, res: Any) -> None:
            try:
                folder = d.select_folder_finish(res).get_path()
            except GLib.Error:
                return
            if not folder or not self._loaded_backup:
                return
            root = Path(folder)
            count = 0
            try:
                for e in self._loaded_backup["entries"]:
                    rel = str(e["path"]).lstrip("/")
                    dest = (root / rel).resolve()
                    if not str(dest).startswith(str(root.resolve())):
                        continue
                    dest.parent.mkdir(parents=True, exist_ok=True)
                    dest.write_bytes(bytes(e["data"]))
                    count += 1
                self._on_toast(f"Extracted {count} file(s) to {root.name}")
            except Exception as exc:
                self._on_toast(f"Extract failed: {exc}")

        dlg.select_folder(self.win, None, _picked)

    def _extract_entry(self, data: bytes, filename: str) -> None:
        dlg = Gtk.FileDialog(title=f"Extract {filename}")
        dlg.set_initial_name(filename)

        def _picked(d: Any, res: Any) -> None:
            try:
                dest = d.save_finish(res).get_path()
            except GLib.Error:
                return
            if dest:
                try:
                    Path(dest).write_bytes(data)
                    self._on_toast(f"Extracted {Path(dest).name}")
                except Exception as exc:
                    self._on_toast(f"Extract failed: {exc}")

        dlg.save(self.win, None, _picked)

    def _confirm_restore_loaded(self) -> None:
        if self._busy or not self._loaded_backup:
            return
        lk = self.runtime.link
        if not lk.connected or lk.fs_backend is None:
            self._on_toast("Connect a RigWire radio first")
            return
        src = str(self._loaded_backup["path"])
        be = lk.fs_backend
        self._on_confirm(
            f"Restore {Path(src).name}?",
            "User, preference and app files on the radio will be replaced. Factory calibration in /sys is shielded.",
            "Restore",
            lambda: self._run_restore(src, be),
        )

    def _run_restore(self, src: str, be: Any) -> None:
        self._busy = True
        self._sync_sensitivity()
        self._on_progress(True, 0.0, "Verifying & restoring backup…")

        def _w() -> None:
            try:
                rest, skip = restore_backup_to_backend(
                    src,
                    be,
                    self.runtime.security.policy,
                    progress_cb=lambda f, m: GLib.idle_add(self._on_progress, True, f, m),
                )
                GLib.idle_add(self._on_toast, f"Restored {rest} file(s) ({skip} /sys shielded)")
            except Exception as exc:
                GLib.idle_add(self._on_toast, f"Restore failed: {exc}")
            finally:
                def _done() -> bool:
                    self._busy = False
                    self._on_progress(False, 0.0, "")
                    self._sync_sensitivity()
                    return False

                GLib.idle_add(_done)

        threading.Thread(target=_w, daemon=True).start()
