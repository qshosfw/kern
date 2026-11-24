"""Apps View: unified radio app manager, manifest/KRX sideloader & workspace package builder."""

from __future__ import annotations

import threading
import time
from pathlib import Path
from typing import Any, Callable

import gi

gi.require_version("Gtk", "4.0")
gi.require_version("Gdk", "4.0")
gi.require_version("Adw", "1")
from gi.repository import Adw, Gdk, Gio, GLib, Gtk  # noqa: E402

import kernfs_mount

from ... import REPO_ROOT
from ...device.packages import (
    GROUP_LABELS,
    build_workspace_package,
    discover_workspace_packages,
    inspect_package_path,
)
from ..runtime import DockRuntime
from ..widgets.app_icon import inspect_krx_metadata, make_app_icon_widget
from ..widgets.hex_view import show_hex_viewer_dialog, show_remote_hex_viewer_dialog
from ..widgets.util import fmt_size


class AppsView:
    """Unified GNOME Software-style radio app manager, sideloader & workspace package browser."""

    def __init__(
        self,
        win: Adw.ApplicationWindow,
        runtime: DockRuntime,
        *,
        on_toast: Callable[[str], None],
        on_confirm: Callable[[str, str, str, Callable[[], None]], None],
        on_progress: Callable[[bool, float, str], None] | None = None,
    ) -> None:
        self.win = win
        self.runtime = runtime
        self._on_toast = on_toast
        self._on_confirm = on_confirm
        self._on_progress = on_progress

        self.staged: dict[str, Any] | None = None
        self._installed_apps: list[dict[str, Any]] = []
        self._installed_names: set[str] = set()
        self._workspace_pkgs: list[dict[str, Any]] = []
        self._meta_cache: dict[tuple[str, str, int], dict[str, Any]] = {}
        self._filter_group: str = "all"
        self._filter_query: str = ""
        self._dirty: bool = True
        self._refresh_gen: int = 0
        self._busy: bool = False

        self.widget = self._build()
        self.widget.connect("map", lambda _w: self._on_mapped())

        runtime.events.subscribe("link.connected", lambda _e: self._on_link_changed(), main_thread=True)
        runtime.events.subscribe("link.disconnected", lambda _e: self._on_link_changed(), main_thread=True)
        runtime.events.subscribe("security.changed", lambda _e: self._refresh_all_ui(), main_thread=True)
        runtime.events.subscribe("apps.changed", lambda _e: self._on_apps_changed(), main_thread=True)

        self._scan_workspace()
        self._refresh_all_ui()

    def _build(self) -> Gtk.Box:
        root = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        self.page = Adw.PreferencesPage()

        # === 1. SIDELOAD / PACKAGE INSPECTOR ===
        self.grp_stage = Adw.PreferencesGroup(title="Sideload")

        self.empty_row = Adw.ActionRow(
            title="Install or Inspect a Package",
            subtitle="Drop a .krx, .mod, or krx.toml file to inspect permissions and install",
        )
        self.empty_row.add_prefix(Gtk.Image(icon_name="application-x-executable-symbolic", pixel_size=28))
        pick_btn = Gtk.Button(label="Choose…", valign=Gtk.Align.CENTER)
        pick_btn.add_css_class("suggested-action")
        pick_btn.connect("clicked", lambda _b: self.open_sideload_picker())
        self.empty_row.add_suffix(pick_btn)
        self.grp_stage.add(self.empty_row)

        self.prev_hdr_row = Adw.ActionRow(title="", subtitle="", visible=False)
        self.prev_icon_slot = Gtk.Box(valign=Gtk.Align.CENTER)
        self.prev_hdr_row.add_prefix(self.prev_icon_slot)

        self.prev_hex_btn = Gtk.Button(
            icon_name="accessories-text-editor-symbolic",
            valign=Gtk.Align.CENTER,
            tooltip_text="Inspect Package Hex",
        )
        self.prev_hex_btn.add_css_class("flat")
        self.prev_hex_btn.connect("clicked", lambda _b: self._inspect_staged_hex())
        self.prev_hdr_row.add_suffix(self.prev_hex_btn)

        self.prev_build_btn = Gtk.Button(label="Rebuild", valign=Gtk.Align.CENTER, visible=False)
        self.prev_build_btn.connect("clicked", lambda _b: self._build_staged(then_install=False))
        self.prev_hdr_row.add_suffix(self.prev_build_btn)

        self.prev_install_btn = Gtk.Button(label="Install", valign=Gtk.Align.CENTER)
        self.prev_install_btn.add_css_class("suggested-action")
        self.prev_install_btn.connect("clicked", lambda _b: self._install_staged())
        self.prev_hdr_row.add_suffix(self.prev_install_btn)

        close_b = Gtk.Button(icon_name="window-close-symbolic", valign=Gtk.Align.CENTER, tooltip_text="Close Preview")
        close_b.add_css_class("flat")
        close_b.connect("clicked", lambda _b: self._clear_staged())
        self.prev_hdr_row.add_suffix(close_b)
        self.grp_stage.add(self.prev_hdr_row)

        self.prop_pkg = Adw.ActionRow(title="Package", subtitle="", visible=False)
        self.prop_pkg.add_css_class("property")
        self.grp_stage.add(self.prop_pkg)

        self.prop_caps = Adw.ActionRow(title="Permissions", subtitle="", subtitle_selectable=True, visible=False)
        self.prop_caps.add_css_class("property")
        self.caps_warn_icon = Gtk.Image(
            icon_name="dialog-warning-symbolic",
            valign=Gtk.Align.CENTER,
            tooltip_text="Requests sensitive hardware or storage permissions",
            visible=False,
        )
        self.caps_warn_icon.add_css_class("warning")
        self.prop_caps.add_suffix(self.caps_warn_icon)
        self.grp_stage.add(self.prop_caps)

        self.prop_extra = Adw.ActionRow(title="Actions &amp; Lifecycle", subtitle="", visible=False)
        self.prop_extra.add_css_class("property")
        self.grp_stage.add(self.prop_extra)

        self.prev_prog_row = Adw.ActionRow(title="Working…", subtitle="", visible=False)
        self.prev_prog = Gtk.ProgressBar(valign=Gtk.Align.CENTER)
        self.prev_prog.set_size_request(140, -1)
        self.prev_prog_row.add_suffix(self.prev_prog)
        self.grp_stage.add(self.prev_prog_row)
        self.page.add(self.grp_stage)

        # === 2. INSTALLED ON RADIO (/apps) ===
        grp_inst = Adw.PreferencesGroup(title="Installed on Radio (/apps)")
        ref_a = Gtk.Button(icon_name="view-refresh-symbolic", tooltip_text="Refresh Installed Apps")
        ref_a.add_css_class("flat")
        ref_a.connect("clicked", lambda _b: self.refresh_apps(force_headers=True))
        grp_inst.set_header_suffix(ref_a)

        self.apps_list = Gtk.ListBox(selection_mode=Gtk.SelectionMode.NONE)
        self.apps_list.add_css_class("boxed-list")
        grp_inst.add(self.apps_list)
        self.page.add(grp_inst)

        # === 3. WORKSPACE PACKAGES ===
        self.ws_section = Adw.PreferencesGroup(title="Workspace Packages", visible=False)
        ref_ws = Gtk.Button(icon_name="view-refresh-symbolic", tooltip_text="Rescan Workspace Packages")
        ref_ws.add_css_class("flat")
        ref_ws.connect("clicked", lambda _b: (self._scan_workspace(), self._refresh_all_ui()))
        self.ws_section.set_header_suffix(ref_ws)

        fbar = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=6, margin_bottom=8)
        self.search_entry = Gtk.SearchEntry(placeholder_text="Filter packages…", hexpand=True)
        self.search_entry.connect("search-changed", self._on_search_changed)
        fbar.append(self.search_entry)

        grp_box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=0)
        grp_box.add_css_class("linked")
        first_btn: Gtk.ToggleButton | None = None
        for gid, glabel in (("all", "All"), ("core", "Core"), ("sys", "System"), ("apps", "Apps"), ("examples", "Examples")):
            tb = Gtk.ToggleButton(label=glabel)
            if first_btn is None:
                first_btn = tb
                tb.set_active(True)
            else:
                tb.set_group(first_btn)
            tb.connect("toggled", lambda b, g=gid: self._set_group_filter(g) if b.get_active() else None)
            grp_box.append(tb)
        fbar.append(grp_box)
        self.ws_section.add(fbar)

        self.ws_list = Gtk.ListBox(selection_mode=Gtk.SelectionMode.NONE)
        self.ws_list.add_css_class("boxed-list")
        self.ws_section.add(self.ws_list)
        self.page.add(self.ws_section)

        # === 4. BUNDLES ===
        self.bundles_section = Adw.PreferencesGroup(title="Bundles", visible=False)
        self.bundles_list = Gtk.ListBox(selection_mode=Gtk.SelectionMode.NONE)
        self.bundles_list.add_css_class("boxed-list")
        self.bundles_section.add(self.bundles_list)
        self.page.add(self.bundles_section)

        root.append(self.page)

        drop = Gtk.DropTarget.new(Gio.File, Gdk.DragAction.COPY)
        drop.connect("enter", lambda *_a: (root.add_css_class("drop-active"), Gdk.DragAction.COPY)[1])
        drop.connect("leave", lambda *_a: root.remove_css_class("drop-active"))
        drop.connect("drop", self._on_drop)
        root.add_controller(drop)
        return root

    def _on_drop(self, _t: Any, value: Any, _x: float, _y: float) -> bool:
        self.widget.remove_css_class("drop-active")
        if isinstance(value, Gio.File) and value.get_path():
            self.load_package_path(Path(value.get_path()))
            return True
        return False

    def _on_link_changed(self) -> None:
        self._dirty = True
        lk = self.runtime.link
        if not lk.connected or lk.link_mode != "rigwire":
            self._installed_apps = []
            self._installed_names = set()
            self._refresh_all_ui()
            return
        if self.widget.get_mapped():
            self.refresh_apps()

    def _on_apps_changed(self) -> None:
        self._dirty = True
        if self.widget.get_mapped():
            self.refresh_apps(force_headers=True)

    def _on_mapped(self) -> None:
        self._scan_workspace()
        if self._dirty:
            self.refresh_apps()

    def _device_cache_key(self) -> str:
        lk = self.runtime.link
        return str(lk.last_telemetry.get("serial_str") or lk.port or lk.volume_name or "radio")

    def _scan_workspace(self) -> None:
        self._workspace_pkgs = discover_workspace_packages(REPO_ROOT)
        self._render_bundles()
        self._render_workspace_list()

    def _on_search_changed(self, entry: Gtk.SearchEntry) -> None:
        self._filter_query = entry.get_text().strip().lower()
        self._render_workspace_list()

    def _set_group_filter(self, group: str) -> None:
        self._filter_group = group
        self._render_workspace_list()

    def refresh_apps(self, *, force_headers: bool = False) -> None:
        lk = self.runtime.link
        self._refresh_gen += 1
        gen = self._refresh_gen
        if not lk.connected or lk.link_mode != "rigwire" or lk.fs_backend is None:
            self._dirty = False
            self._installed_apps = []
            self._installed_names = set()
            self._refresh_all_ui()
            return
        self._dirty = False
        be = lk.fs_backend
        dev_key = self._device_cache_key()
        if force_headers:
            for k in [k for k in self._meta_cache if k[0] == dev_key]:
                self._meta_cache.pop(k, None)

        def _w() -> None:
            try:
                entries = be.ls("/apps")
            except Exception:
                entries = []

            files: list[dict[str, Any]] = []
            pending: list[tuple[int, str, str, int]] = []
            for e in entries:
                if e.get("is_dir"):
                    continue
                item = dict(e)
                name = str(e["name"])
                size = int(e.get("size", 0))
                full = f"/apps/{name}"
                ckey = (dev_key, full, size)
                cached = self._meta_cache.get(ckey)
                if cached is not None:
                    item["meta"] = cached
                else:
                    item["meta"] = inspect_krx_metadata(b"", name)
                    if Path(name).suffix.lower() in kernfs_mount.PKG_EXTS and size >= 24:
                        pending.append((len(files), full, name, size))
                files.append(item)

            GLib.idle_add(self._set_apps, gen, [dict(x) for x in files])

            if not pending:
                return
            updated = False
            for idx, full, name, size in pending:
                if gen != self._refresh_gen:
                    return
                try:
                    head = be.read_head(full, 1024)
                    meta = inspect_krx_metadata(head, name)
                except Exception:
                    meta = inspect_krx_metadata(b"", name)
                self._meta_cache[(dev_key, full, size)] = meta
                files[idx]["meta"] = meta
                updated = True
            if updated and gen == self._refresh_gen:
                GLib.idle_add(self._set_apps, gen, [dict(x) for x in files])

        threading.Thread(target=_w, daemon=True).start()

    def _set_apps(self, gen: int, entries: list[dict[str, Any]]) -> bool:
        if gen != self._refresh_gen:
            return False
        self._installed_apps = [e for e in entries if not e.get("is_dir")]
        self._installed_names = {str(e["name"]) for e in self._installed_apps}
        self._refresh_all_ui()
        return False

    def _refresh_all_ui(self) -> None:
        self._render_preview()
        self._render_apps()
        self._render_workspace_list()
        self._render_bundles()

    def _clear_staged(self) -> None:
        self.staged = None
        self._render_preview()

    def load_package_path(self, fpath: Path) -> None:
        try:
            info = inspect_package_path(fpath)
        except Exception as exc:
            self._on_toast(f"Cannot read package: {exc}")
            return
        self.staged = info
        self._render_preview()
        self.page.scroll_to_top()

    def _stage_workspace_pkg(self, pkg: dict[str, Any]) -> None:
        mpath = pkg.get("manifest_path") or pkg.get("path")
        if mpath and Path(mpath).is_file():
            try:
                fresh = inspect_package_path(Path(mpath))
                fresh["group"] = pkg.get("group", "")
                fresh["pkg_id"] = pkg.get("pkg_id", "")
                self.staged = fresh
            except Exception:
                self.staged = dict(pkg)
        else:
            self.staged = dict(pkg)
        self._render_preview()
        self.page.scroll_to_top()

    def _render_preview(self) -> None:
        pkg = self.staged
        has_pkg = pkg is not None
        self.empty_row.set_visible(not has_pkg)
        self.prev_hdr_row.set_visible(has_pkg)
        self.prop_pkg.set_visible(has_pkg)
        self.prop_caps.set_visible(has_pkg)
        if pkg is None:
            self.prop_extra.set_visible(False)
            self.prev_prog_row.set_visible(False)
            return

        c = self.prev_icon_slot.get_first_child()
        while c is not None:
            n = c.get_next_sibling()
            self.prev_icon_slot.remove(c)
            c = n
        self.prev_icon_slot.append(
            make_app_icon_widget(
                str(pkg.get("name", "App")),
                exec_type=str(pkg.get("exec_type", "App")),
                icon_asset=int(pkg.get("icon_asset", 0) or 0),
                bitmap=pkg.get("bitmap"),
                size=32,
            )
        )

        ver = str(pkg.get("version", ""))
        title_s = f"{pkg.get('name', 'App')} {ver}".strip()
        author = str(pkg.get("author", ""))
        remote_path = str(pkg.get("remote_path", ""))
        fname = str(pkg.get("filename", ""))
        built_ok = bool(pkg.get("built_exists"))
        state = "Not built" if not built_ok else ("Protected system" if pkg.get("protected") else ("Update" if fname in self._installed_names else "New install"))
        sub_parts = [p for p in (author, remote_path, state) if p]
        self.prev_hdr_row.set_title(GLib.markup_escape_text(title_s))
        self.prev_hdr_row.set_subtitle(GLib.markup_escape_text(" · ".join(sub_parts)))

        pkg_parts = [str(pkg.get("exec_type", "App"))]
        if pkg.get("profile_label"):
            pkg_parts.append(str(pkg["profile_label"]))
        if built_ok:
            pkg_parts.append(fmt_size(int(pkg.get("size", 0))))
            pkg_parts.append("CRC-32 verified" if pkg.get("crc_ok") else "CRC mismatch")
        else:
            pkg_parts.append("Source manifest (krx.toml)")
        self.prop_pkg.set_subtitle(GLib.markup_escape_text(" · ".join(pkg_parts)))

        cap_list = list(pkg.get("cap_list", []))
        if not cap_list:
            self.prop_caps.set_subtitle("None")
            self.caps_warn_icon.set_visible(False)
        else:
            self.prop_caps.set_subtitle(GLib.markup_escape_text(", ".join(c["label"] for c in cap_list)))
            self.caps_warn_icon.set_visible(any(c["sensitive"] for c in cap_list))

        extra_parts = list(pkg.get("lifecycle", [])) + [f"Action: {a}" for a in pkg.get("actions", [])]
        self.prop_extra.set_visible(bool(extra_parts))
        if extra_parts:
            self.prop_extra.set_subtitle(GLib.markup_escape_text(" · ".join(extra_parts)))

        lk = self.runtime.link
        hard = self.runtime.security.hardened
        can_radio = lk.connected and lk.link_mode == "rigwire" and not hard
        has_manifest = pkg.get("source_kind") == "manifest" and bool(pkg.get("pkg_id"))

        if not built_ok and has_manifest:
            self.prev_install_btn.set_label("Build & Install" if can_radio else "Build")
            self.prev_install_btn.set_sensitive(not self._busy)
        else:
            self.prev_install_btn.set_label("Update" if fname in self._installed_names else "Install")
            self.prev_install_btn.set_sensitive(bool(can_radio and built_ok and not self._busy))

        self.prev_build_btn.set_visible(bool(has_manifest and built_ok))
        self.prev_build_btn.set_sensitive(not self._busy)
        self.prev_hex_btn.set_visible(built_ok)

    def _render_apps(self) -> None:
        c = self.apps_list.get_first_child()
        while c is not None:
            n = c.get_next_sibling()
            self.apps_list.remove(c)
            c = n
        lk = self.runtime.link
        if not lk.connected or lk.link_mode != "rigwire":
            row = Adw.ActionRow(title="No Radio Connected", subtitle="Connect a RigWire radio to view or remove installed apps")
            row.set_sensitive(False)
            self.apps_list.append(row)
            return
        if not self._installed_apps:
            row = Adw.ActionRow(title="No Installed Apps", subtitle="Drop a .krx/.mod or install from Workspace Packages below")
            row.set_sensitive(False)
            self.apps_list.append(row)
            return

        ro = self.runtime.security.policy.read_only
        for e in sorted(self._installed_apps, key=lambda x: x["name"].lower()):
            name = e["name"]
            size = int(e.get("size", 0))
            full = f"/apps/{name}"
            protected = full in kernfs_mount.PROTECTED_RECOVERY_MODS
            meta = e.get("meta") or inspect_krx_metadata(b"", name)
            disp_name = str(meta.get("name") or Path(name).stem.replace("_", " ").replace("-", " ").title())
            ver = str(meta.get("version") or "")
            author = str(meta.get("author") or "")
            exec_type = str(meta.get("exec_type") or "App")
            title_s = f"{disp_name} {ver}".strip()
            sub_parts = [p for p in (author, full, fmt_size(size)) if p]

            row = Adw.ActionRow(
                title=GLib.markup_escape_text(title_s),
                subtitle=GLib.markup_escape_text(" · ".join(sub_parts)),
            )
            row.add_prefix(
                make_app_icon_widget(
                    disp_name,
                    exec_type=exec_type,
                    icon_asset=int(meta.get("icon_asset", 0) or 0),
                    bitmap=meta.get("bitmap"),
                    size=28,
                )
            )

            if protected:
                lock = Gtk.Image(icon_name="security-high-symbolic", valign=Gtk.Align.CENTER, tooltip_text="Protected system module")
                lock.add_css_class("dim-label")
                row.add_suffix(lock)

            hex_b = Gtk.Button(icon_name="accessories-text-editor-symbolic", valign=Gtk.Align.CENTER, tooltip_text=f"Inspect {name} Hex")
            hex_b.add_css_class("flat")
            hex_b.connect("clicked", lambda _b, p=full, n=name: self._inspect_app_hex(p, n))
            row.add_suffix(hex_b)

            if not protected:
                rm = Gtk.Button(icon_name="user-trash-symbolic", valign=Gtk.Align.CENTER, tooltip_text=f"Uninstall {name}")
                rm.add_css_class("flat")
                rm.set_sensitive(not ro)
                rm.connect(
                    "clicked",
                    lambda _b, p=full, n=name: self._on_confirm(
                        f"Remove {n}?",
                        f"{p} will be uninstalled from the radio.",
                        "Remove",
                        lambda: self._remove_app(p),
                    ),
                )
                row.add_suffix(rm)
            self.apps_list.append(row)

    def _render_workspace_list(self) -> None:
        c = self.ws_list.get_first_child()
        while c is not None:
            n = c.get_next_sibling()
            self.ws_list.remove(c)
            c = n

        self.ws_section.set_visible(bool(self._workspace_pkgs))
        if not self._workspace_pkgs:
            return

        lk = self.runtime.link
        can_radio = lk.connected and lk.link_mode == "rigwire" and not self.runtime.security.hardened

        filtered: list[dict[str, Any]] = []
        for p in self._workspace_pkgs:
            if self._filter_group != "all" and p.get("group") != self._filter_group:
                continue
            if self._filter_query:
                hay = f"{p.get('name', '')} {p.get('pkg_id', '')} {p.get('filename', '')} {p.get('author', '')}".lower()
                if self._filter_query not in hay:
                    continue
            filtered.append(p)

        if not filtered:
            empty = Adw.ActionRow(title="No matching packages", subtitle="Try clearing the search or category filter")
            empty.set_sensitive(False)
            self.ws_list.append(empty)
            return

        for p in filtered:
            built = bool(p.get("built_exists"))
            fname = str(p.get("filename", ""))
            installed = fname in self._installed_names
            grp_lbl = GROUP_LABELS.get(str(p.get("group", "")), "")
            sz_txt = fmt_size(int(p.get("size", 0))) if built else "Not built"
            sub_parts = [x for x in (grp_lbl, fname, sz_txt, "Installed" if installed else "") if x]

            row = Adw.ActionRow(
                title=GLib.markup_escape_text(f"{p['name']} {p.get('version', '')}".strip()),
                subtitle=GLib.markup_escape_text(" · ".join(sub_parts)),
                activatable=True,
            )
            row.connect("activated", lambda _r, pkg=p: self._stage_workspace_pkg(pkg))
            row.add_prefix(
                make_app_icon_widget(
                    str(p["name"]),
                    exec_type=str(p.get("exec_type", "App")),
                    icon_asset=int(p.get("icon_asset", 0) or 0),
                    bitmap=p.get("bitmap"),
                    size=28,
                )
            )

            if built:
                act_b = Gtk.Button(label="Update" if installed else "Install", valign=Gtk.Align.CENTER)
                act_b.set_sensitive(bool(can_radio and not self._busy))
                act_b.connect("clicked", lambda _b, pkg=p: self._install_package(pkg))
            else:
                act_b = Gtk.Button(label="Build", valign=Gtk.Align.CENTER)
                act_b.set_sensitive(not self._busy)
                act_b.connect("clicked", lambda _b, pkg=p: self._build_and_maybe_install(pkg, then_install=False))
            row.add_suffix(act_b)
            self.ws_list.append(row)

    def _render_bundles(self) -> None:
        c = self.bundles_list.get_first_child()
        while c is not None:
            n = c.get_next_sibling()
            self.bundles_list.remove(c)
            c = n

        by_group: dict[str, list[dict[str, Any]]] = {}
        for p in self._workspace_pkgs:
            by_group.setdefault(str(p.get("group", "")), []).append(p)

        bundles: list[tuple[str, str, list[dict[str, Any]]]] = []
        if by_group.get("core"):
            bundles.append(("Core Bundle", "Essential radio applications", by_group["core"]))
        if by_group.get("sys"):
            bundles.append(("System Modules", "DFU, recovery, calibration & diagnostics", by_group["sys"]))

        self.bundles_section.set_visible(bool(bundles))
        lk = self.runtime.link
        can_radio = lk.connected and lk.link_mode == "rigwire" and not self.runtime.security.hardened

        for title, desc, pkgs in bundles:
            built = [p for p in pkgs if p.get("built_exists")]
            names = ", ".join(p["name"] for p in pkgs[:4]) + ("…" if len(pkgs) > 4 else "")
            row = Adw.ActionRow(
                title=GLib.markup_escape_text(title),
                subtitle=GLib.markup_escape_text(f"{len(built)}/{len(pkgs)} built · {names or desc}"),
            )
            row.add_prefix(Gtk.Image(icon_name="package-x-generic-symbolic", pixel_size=24))
            btn = Gtk.Button(label=f"Install ({len(built)})", valign=Gtk.Align.CENTER)
            btn.set_sensitive(bool(can_radio and built and not self._busy))
            btn.connect("clicked", lambda _b, t=title, ps=built: self._confirm_install_bundle(t, ps))
            row.add_suffix(btn)
            self.bundles_list.append(row)

    def _inspect_staged_hex(self) -> None:
        if not self.staged:
            return
        bpath = self.staged.get("built_path")
        raw = self.staged.get("raw_bytes")
        if raw is None and bpath and Path(bpath).is_file():
            try:
                raw = Path(bpath).read_bytes()
            except OSError as exc:
                self._on_toast(f"Read failed: {exc}")
                return
        if raw:
            show_hex_viewer_dialog(self.win, raw, str(self.staged.get("filename", "package.krx")))

    def _inspect_app_hex(self, remote_path: str, name: str) -> None:
        be = self.runtime.link.fs_backend
        if be is None:
            return
        show_remote_hex_viewer_dialog(self.win, be, remote_path, name)

    def _remove_app(self, remote_path: str) -> None:
        be = self.runtime.link.fs_backend
        if be is None:
            return

        def _w() -> None:
            try:
                clean = self.runtime.security.check_delete(remote_path)
                be.remove(clean)
                GLib.idle_add(self._on_toast, f"Removed {clean}")
                GLib.idle_add(self.refresh_apps)
            except Exception as exc:
                GLib.idle_add(self._on_toast, f"Remove failed: {exc}")

        threading.Thread(target=_w, daemon=True).start()

    def open_sideload_picker(self) -> None:
        dlg = Gtk.FileDialog(title="Select Package (.krx, .mod, or krx.toml)")

        def _done(d: Any, res: Any) -> None:
            try:
                fpath = d.open_finish(res).get_path()
            except GLib.Error:
                return
            if fpath:
                self.load_package_path(Path(fpath))

        dlg.open(self.win, None, _done)

    def sideload_path(self, fpath: Path) -> None:
        self.load_package_path(fpath)

    def _set_busy(self, busy: bool, text: str = "", frac: float = 0.0) -> None:
        self._busy = busy
        self.prev_prog_row.set_visible(busy and self.staged is not None)
        if busy:
            self.prev_prog_row.set_title(GLib.markup_escape_text(text or "Working…"))
            self.prev_prog.set_fraction(max(0.0, min(1.0, frac)))
        if self._on_progress is not None:
            self._on_progress(busy, frac, text)
        self._refresh_all_ui()

    def _install_staged(self) -> None:
        if not self.staged:
            return
        if not self.staged.get("built_exists") and self.staged.get("pkg_id"):
            lk = self.runtime.link
            can_radio = lk.connected and lk.link_mode == "rigwire" and not self.runtime.security.hardened
            self._build_and_maybe_install(self.staged, then_install=can_radio)
            return
        self._install_package(self.staged)

    def _build_staged(self, *, then_install: bool) -> None:
        if self.staged and self.staged.get("pkg_id"):
            self._build_and_maybe_install(self.staged, then_install=then_install)

    def _build_and_maybe_install(self, pkg: dict[str, Any], *, then_install: bool) -> None:
        pkg_id = str(pkg.get("pkg_id", ""))
        if not pkg_id or self._busy:
            return
        self._set_busy(True, f"Building {pkg['name']}…", 0.25)

        def _w() -> None:
            ok, out = build_workspace_package(pkg_id, REPO_ROOT)

            def _done() -> bool:
                self._set_busy(False)
                self._scan_workspace()
                if not ok:
                    first_err = next((ln for ln in reversed(out.splitlines()) if ln.strip()), "Build failed")
                    self._on_toast(f"Build failed: {first_err}")
                    return False
                self._on_toast(f"Built {pkg['name']}")
                if self.staged and self.staged.get("pkg_id") == pkg_id:
                    self._stage_workspace_pkg(pkg)
                if then_install:
                    refreshed = next((x for x in self._workspace_pkgs if x.get("pkg_id") == pkg_id), pkg)
                    self._install_package(refreshed)
                return False

            GLib.idle_add(_done)

        threading.Thread(target=_w, daemon=True).start()

    def _install_package(self, pkg: dict[str, Any]) -> None:
        try:
            self.runtime.security.check_sideload()
        except PermissionError as exc:
            self._on_toast(str(exc))
            return
        lk = self.runtime.link
        if not lk.connected or lk.link_mode != "rigwire":
            self._on_toast("Connect a RigWire radio first")
            return
        bpath = Path(str(pkg.get("built_path") or pkg.get("path") or ""))
        if not bpath.is_file():
            self._on_toast("Package binary not found — build it first")
            return
        if self._busy:
            return

        self._set_busy(True, f"Installing {pkg['name']}…", 0.35)

        def _w() -> None:
            try:
                if lk.demo_mode:
                    time.sleep(0.2)
                    remote = f"/apps/{bpath.name}"
                    if lk.fs_backend is not None:
                        lk.fs_backend.write_bytes(remote, bpath.read_bytes())
                    msg = f"Installed {pkg['name']} -> {remote} (demo)"
                else:
                    if not lk.client:
                        raise RuntimeError("No active RigWire client")
                    with lk.lock:
                        info = lk.client.sideload(str(bpath), verify=True, force=True)  # type: ignore[union-attr]
                    msg = f"Installed {pkg['name']} -> {info['remote_path']}"
                GLib.idle_add(self._on_toast, msg)
                GLib.idle_add(lambda: (self.runtime.events.publish("apps.changed", {"path": str(bpath)}), False)[1])
            except Exception as exc:
                GLib.idle_add(self._on_toast, f"Install failed: {exc}")
            finally:
                GLib.idle_add(lambda: (self._set_busy(False), self.refresh_apps(), False)[2])

        threading.Thread(target=_w, daemon=True).start()

    def _confirm_install_bundle(self, title: str, pkgs: list[dict[str, Any]]) -> None:
        names = ", ".join(p["name"] for p in pkgs)
        self._on_confirm(
            f"Install {title}?",
            f"Install {len(pkgs)} package(s) to /apps:\n{names}",
            "Install All",
            lambda: self._install_bundle(title, pkgs),
        )

    def _install_bundle(self, title: str, pkgs: list[dict[str, Any]]) -> None:
        try:
            self.runtime.security.check_sideload()
        except PermissionError as exc:
            self._on_toast(str(exc))
            return
        lk = self.runtime.link
        if not lk.connected or lk.link_mode != "rigwire" or self._busy:
            return

        self._set_busy(True, f"Installing {title}…", 0.05)

        def _w() -> None:
            installed = 0
            total = max(1, len(pkgs))
            try:
                for i, pkg in enumerate(pkgs):
                    bpath = Path(str(pkg.get("built_path") or ""))
                    if not bpath.is_file():
                        continue
                    frac = (i + 0.5) / float(total)
                    GLib.idle_add(self._set_busy, True, f"Installing {pkg['name']} ({i + 1}/{total})…", frac)
                    if lk.demo_mode:
                        time.sleep(0.1)
                        if lk.fs_backend is not None:
                            lk.fs_backend.write_bytes(f"/apps/{bpath.name}", bpath.read_bytes())
                    else:
                        if not lk.client:
                            raise RuntimeError("No active RigWire client")
                        with lk.lock:
                            lk.client.sideload(str(bpath), verify=True, force=True)  # type: ignore[union-attr]
                    installed += 1
                GLib.idle_add(self._on_toast, f"Installed {installed}/{len(pkgs)} packages from {title}")
                GLib.idle_add(lambda: (self.runtime.events.publish("apps.changed", {"bundle": title}), False)[1])
            except Exception as exc:
                GLib.idle_add(self._on_toast, f"Bundle install failed after {installed}: {exc}")
            finally:
                GLib.idle_add(lambda: (self._set_busy(False), self.refresh_apps(), False)[2])

        threading.Thread(target=_w, daemon=True).start()
