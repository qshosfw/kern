"""Backups View: Dedicated multi-format backup creator (.kfw, .zip, raw calib) & interactive backup viewer."""

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
    """Dedicated Backups tab: create .kfw / .zip / raw calibration dumps & inspect contents with colored Hex View."""

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

        self.widget = self._build()

        runtime.events.subscribe("link.connected", lambda _e: self._sync_sensitivity(), main_thread=True)
        runtime.events.subscribe("link.disconnected", lambda _e: self._sync_sensitivity(), main_thread=True)
        runtime.events.subscribe("security.changed", lambda _e: self._sync_sensitivity(), main_thread=True)
        self._sync_sensitivity()

    def _build(self) -> Gtk.Box:
        root = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        page = Adw.PreferencesPage()

        # --- Group 1: Create Backup ---
        grp_new = Adw.PreferencesGroup(title="Create Backup")
        self._online_rows: list[Any] = []
        for title, sub, icon, fmt, calib_only in (
            ("Full Snapshot (.kfw)", "Verified Ascon container", "drive-harddisk-symbolic", "kfw", False),
            ("Portable Archive (.zip)", "Standard ZIP with manifest", "package-x-generic-symbolic", "zip", False),
            ("Calibration (.kfw)", "Factory calibration &amp; signature", "security-high-symbolic", "kfw", True),
            ("Raw Calibration (.dat)", "Exact 512 B binary dump", "document-save-symbolic", "raw", True),
        ):
            row = Adw.ActionRow(title=title, subtitle=sub, activatable=True)
            row.add_prefix(Gtk.Image(icon_name=icon))
            row.add_suffix(Gtk.Image(icon_name="go-next-symbolic"))
            row.connect("activated", lambda _r, f=fmt, c=calib_only: self.run_backup(fmt=f, calib_only=c))
            grp_new.add(row)
            self._online_rows.append(row)
        page.add(grp_new)

        # --- Group 2: Inspect & Restore Backup ---
        grp_ins = Adw.PreferencesGroup(title="Inspect &amp; Restore")
        hdr_row = Adw.ActionRow(title="No Backup Open", subtitle="Open or drop a .kfw, .zip, or .dat file")
        hdr_row.add_prefix(Gtk.Image(icon_name="folder-open-symbolic"))

        open_b = Gtk.Button(label="Open…", valign=Gtk.Align.CENTER)
        open_b.connect("clicked", lambda _b: self.open_inspect_picker())
        hdr_row.add_suffix(open_b)

        self.restore_btn = Gtk.Button(label="Restore…", valign=Gtk.Align.CENTER, visible=False)
        self.restore_btn.add_css_class("suggested-action")
        self.restore_btn.connect("clicked", lambda _b: self._confirm_restore_loaded())
        hdr_row.add_suffix(self.restore_btn)

        self.inspect_hdr = hdr_row
        grp_ins.add(hdr_row)
        page.add(grp_ins)

        # Contents list of the opened backup
        self.entries_group = Adw.PreferencesGroup(title="Archive Contents", visible=False)
        self.entries_list = Gtk.ListBox(selection_mode=Gtk.SelectionMode.NONE)
        self.entries_list.add_css_class("boxed-list")
        self.entries_group.add(self.entries_list)
        page.add(self.entries_group)

        root.append(page)
        drop = Gtk.DropTarget.new(Gio.File, Gdk.DragAction.COPY)
        drop.connect("drop", self._on_drop)
        root.add_controller(drop)
        return root

    def _on_drop(self, _t: Any, value: Any, _x: float, _y: float) -> bool:
        if isinstance(value, Gio.File) and value.get_path():
            self.load_backup_file(Path(value.get_path()))
            return True
        return False

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
            and self._loaded_backup["format"] in ("KFW v1.0 (Ascon-Hash256)", "ZIP Archive")
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
                    GLib.idle_add(self._on_toast, f"Saved {out_p.name} ({len(blob):,} B)")
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
        ts = f" · {info['timestamp']}" if info.get("timestamp") else ""
        self.inspect_hdr.set_title(GLib.markup_escape_text(Path(info["path"]).name))
        self.inspect_hdr.set_subtitle(
            GLib.markup_escape_text(f"{info['format']} · {len(entries)} file(s) · {fmt_size(total_b)}{ts}")
        )
        self.restore_btn.set_visible(info["format"] in ("KFW v1.0 (Ascon-Hash256)", "ZIP Archive"))

        c = self.entries_list.get_first_child()
        while c is not None:
            n = c.get_next_sibling()
            self.entries_list.remove(c)
            c = n

        for e in sorted(entries, key=lambda x: str(x["path"])):
            rpath = str(e["path"])
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
                        size=30,
                    )
                )
            if e.get("protected"):
                pb = Gtk.Label(label="Protected", valign=Gtk.Align.CENTER)
                pb.add_css_class("protected-badge")
                row.add_suffix(pb)

            hex_b = Gtk.Button(icon_name="accessories-text-editor-symbolic", valign=Gtk.Align.CENTER, tooltip_text=f"Colored Hex View ({rpath})")
            hex_b.add_css_class("flat")
            hex_b.connect("clicked", lambda _b, d=data, p=rpath: show_hex_viewer_dialog(self.win, d, p))
            row.add_suffix(hex_b)

            ex_b = Gtk.Button(icon_name="document-save-symbolic", valign=Gtk.Align.CENTER, tooltip_text=f"Extract {Path(rpath).name}…")
            ex_b.add_css_class("flat")
            ex_b.connect("clicked", lambda _b, d=data, n=Path(rpath).name: self._extract_entry(d, n))
            row.add_suffix(ex_b)

            self.entries_list.append(row)

        self.entries_group.set_visible(bool(entries))
        self._sync_sensitivity()

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
