"""Sideload View: cute, manifest-aware app installer & workspace package browser."""

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

from ... import REPO_ROOT
from ...device.packages import (
    GROUP_LABELS,
    build_workspace_package,
    discover_workspace_packages,
    inspect_package_path,
)
from ..runtime import DockRuntime
from ..widgets.app_icon import make_app_icon_widget
from ..widgets.hex_view import show_hex_viewer_dialog
from ..widgets.util import fmt_size


class SideloadView:
    """Dedicated GNOME Software-style Sideload tool with manifest/KRX inspection & bundles."""

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

        self.staged: dict[str, Any] | None = None
        self._workspace_pkgs: list[dict[str, Any]] = []
        self._installed_names: set[str] = set()
        self._filter_group: str = "all"
        self._filter_query: str = ""
        self._busy: bool = False

        self.widget = self._build()
        self.widget.connect("map", lambda _w: self._on_mapped())

        runtime.events.subscribe("link.connected", lambda _e: self._refresh_installed(), main_thread=True)
        runtime.events.subscribe("link.disconnected", lambda _e: self._refresh_installed(), main_thread=True)
        runtime.events.subscribe("security.changed", lambda _e: self._refresh_ui(), main_thread=True)
        runtime.events.subscribe("apps.changed", lambda _e: self._refresh_installed(), main_thread=True)

        self._scan_workspace()
        self._refresh_ui()

    def _build(self) -> Gtk.Box:
        root = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        self.scroll = Gtk.ScrolledWindow(vexpand=True)
        self.scroll.set_policy(Gtk.PolicyType.NEVER, Gtk.PolicyType.AUTOMATIC)

        body = Gtk.Box(
            orientation=Gtk.Orientation.VERTICAL,
            spacing=14,
            margin_top=12,
            margin_bottom=18,
            margin_start=14,
            margin_end=14,
        )

        # === 1. HERO DROPZONE / STAGED APP PREVIEW CARD ===
        self.hero_stack = Gtk.Stack(transition_type=Gtk.StackTransitionType.CROSSFADE, transition_duration=140)

        # Empty Dropzone Card
        empty_card = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=10, halign=Gtk.Align.FILL)
        empty_card.add_css_class("card")
        empty_card.add_css_class("sideload-dropzone")

        drop_top = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=14, valign=Gtk.Align.CENTER)
        drop_top.append(make_app_icon_widget("apps", size=48))
        drop_txt = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=3, hexpand=True, valign=Gtk.Align.CENTER)
        dt_title = Gtk.Label(label="Sideload an Application", xalign=0)
        dt_title.add_css_class("title-3")
        dt_sub = Gtk.Label(
            label="Drop a compiled .krx / .mod package or a krx.toml manifest here to inspect permissions and install.",
            xalign=0,
            wrap=True,
        )
        dt_sub.add_css_class("caption")
        dt_sub.add_css_class("dim-label")
        drop_txt.append(dt_title)
        drop_txt.append(dt_sub)
        drop_top.append(drop_txt)

        pick_btn = Gtk.Button(label="Choose…", valign=Gtk.Align.CENTER)
        pick_btn.add_css_class("suggested-action")
        pick_btn.add_css_class("pill")
        pick_btn.connect("clicked", lambda _b: self.open_package_picker())
        drop_top.append(pick_btn)
        empty_card.append(drop_top)
        self.hero_stack.add_named(empty_card, "empty")

        # Staged App Preview Card
        self.preview_card = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=12)
        self.preview_card.add_css_class("card")
        self.preview_card.add_css_class("sideload-preview")

        # Preview header
        phdr = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=12, valign=Gtk.Align.CENTER)
        self.prev_icon_slot = Gtk.Box(valign=Gtk.Align.CENTER)
        phdr.append(self.prev_icon_slot)

        ptxt = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=2, hexpand=True, valign=Gtk.Align.CENTER)
        title_line = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8, valign=Gtk.Align.CENTER)
        self.prev_name = Gtk.Label(label="App", xalign=0)
        self.prev_name.add_css_class("title-3")
        self.prev_ver = Gtk.Label(label="v1.0.0", valign=Gtk.Align.CENTER)
        self.prev_ver.add_css_class("cap-chip")
        title_line.append(self.prev_name)
        title_line.append(self.prev_ver)
        ptxt.append(title_line)

        self.prev_sub = Gtk.Label(label="", xalign=0, ellipsize=3)
        self.prev_sub.add_css_class("caption")
        self.prev_sub.add_css_class("dim-label")
        ptxt.append(self.prev_sub)
        phdr.append(ptxt)

        self.prev_status_pill = Gtk.Label(label="New Install", valign=Gtk.Align.CENTER)
        self.prev_status_pill.add_css_class("status-pill")
        phdr.append(self.prev_status_pill)
        self.preview_card.append(phdr)

        # Metadata chips row
        self.prev_meta_chips = Gtk.FlowBox(
            selection_mode=Gtk.SelectionMode.NONE,
            row_spacing=6,
            column_spacing=6,
            max_children_per_line=8,
            homogeneous=False,
        )
        self.preview_card.append(self.prev_meta_chips)

        # Permissions / Capabilities section
        self.prev_caps_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=6)
        caps_hdr = Gtk.Label(label="Requested Capabilities", xalign=0)
        caps_hdr.add_css_class("caption-heading")
        caps_hdr.add_css_class("dim-label")
        self.prev_caps_box.append(caps_hdr)
        self.prev_caps_flow = Gtk.FlowBox(
            selection_mode=Gtk.SelectionMode.NONE,
            row_spacing=6,
            column_spacing=6,
            max_children_per_line=8,
            homogeneous=False,
        )
        self.prev_caps_box.append(self.prev_caps_flow)
        self.preview_card.append(self.prev_caps_box)

        # Inline progress bar (hidden when idle)
        self.prev_prog = Gtk.ProgressBar(show_text=True, visible=False)
        self.preview_card.append(self.prev_prog)

        # Action footer
        pfoot = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        self.prev_install_btn = Gtk.Button(label="Install to Radio")
        self.prev_install_btn.add_css_class("suggested-action")
        self.prev_install_btn.add_css_class("pill")
        self.prev_install_btn.connect("clicked", lambda _b: self._install_staged())
        pfoot.append(self.prev_install_btn)

        self.prev_build_btn = Gtk.Button(label="Rebuild", visible=False)
        self.prev_build_btn.add_css_class("pill")
        self.prev_build_btn.connect("clicked", lambda _b: self._build_staged(then_install=False))
        pfoot.append(self.prev_build_btn)

        spacer = Gtk.Box(hexpand=True)
        pfoot.append(spacer)

        self.prev_hex_btn = Gtk.Button(icon_name="accessories-text-editor-symbolic", tooltip_text="Inspect Package Hex")
        self.prev_hex_btn.add_css_class("flat")
        self.prev_hex_btn.connect("clicked", lambda _b: self._inspect_staged_hex())
        pfoot.append(self.prev_hex_btn)

        pick_other = Gtk.Button(icon_name="document-open-symbolic", tooltip_text="Choose Another Package…")
        pick_other.add_css_class("flat")
        pick_other.connect("clicked", lambda _b: self.open_package_picker())
        pfoot.append(pick_other)

        clear_b = Gtk.Button(icon_name="window-close-symbolic", tooltip_text="Close Preview")
        clear_b.add_css_class("flat")
        clear_b.connect("clicked", lambda _b: self._clear_staged())
        pfoot.append(clear_b)

        self.preview_card.append(pfoot)
        self.hero_stack.add_named(self.preview_card, "preview")
        self.hero_stack.set_visible_child_name("empty")
        body.append(self.hero_stack)

        # === 2. QUICK PROVISION BUNDLES ===
        self.bundles_section = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=6, visible=False)
        blbl = Gtk.Label(label="Quick Bundles", xalign=0)
        blbl.add_css_class("heading")
        self.bundles_section.append(blbl)
        self.bundles_list = Gtk.ListBox(selection_mode=Gtk.SelectionMode.NONE)
        self.bundles_list.add_css_class("boxed-list")
        self.bundles_section.append(self.bundles_list)
        body.append(self.bundles_section)

        # === 3. WORKSPACE PACKAGES BROWSER ===
        self.ws_section = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=8, visible=False)

        ws_hdr = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        ws_lbl = Gtk.Label(label="Workspace Packages", xalign=0, hexpand=True)
        ws_lbl.add_css_class("heading")
        ws_hdr.append(ws_lbl)

        ref_ws = Gtk.Button(icon_name="view-refresh-symbolic", tooltip_text="Rescan Workspace Packages")
        ref_ws.add_css_class("flat")
        ref_ws.connect("clicked", lambda _b: (self._scan_workspace(), self._refresh_installed()))
        ws_hdr.append(ref_ws)
        self.ws_section.append(ws_hdr)

        # Filter bar: search + group pills
        fbar = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=6)
        self.search_entry = Gtk.SearchEntry(placeholder_text="Filter packages…", hexpand=True)
        self.search_entry.connect("search-changed", self._on_search_changed)
        fbar.append(self.search_entry)

        self._group_btns: dict[str, Gtk.ToggleButton] = {}
        grp_box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=0)
        grp_box.add_css_class("linked")
        first_btn: Gtk.ToggleButton | None = None
        for gid, glabel in (("all", "All"), ("core", "Core"), ("sys", "System"), ("apps", "Apps"), ("examples", "Examples")):
            tb = Gtk.ToggleButton(label=glabel)
            tb.add_css_class("place-btn")
            if first_btn is None:
                first_btn = tb
                tb.set_active(True)
            else:
                tb.set_group(first_btn)
            tb.connect("toggled", lambda b, g=gid: self._set_group_filter(g) if b.get_active() else None)
            grp_box.append(tb)
            self._group_btns[gid] = tb
        fbar.append(grp_box)
        self.ws_section.append(fbar)

        self.ws_list = Gtk.ListBox(selection_mode=Gtk.SelectionMode.NONE)
        self.ws_list.add_css_class("boxed-list")
        self.ws_section.append(self.ws_list)
        body.append(self.ws_section)

        self.scroll.set_child(Adw.Clamp(maximum_size=680, child=body))
        root.append(self.scroll)

        drop = Gtk.DropTarget.new(Gio.File, Gdk.DragAction.COPY)
        drop.connect("enter", lambda *_a: (root.add_css_class("drop-active"), Gdk.DragAction.COPY)[1])
        drop.connect("leave", lambda *_a: root.remove_css_class("drop-active"))
        drop.connect("drop", self._on_drop)
        root.add_controller(drop)
        return root

    def _on_mapped(self) -> None:
        self._scan_workspace()
        self._refresh_installed()

    def _on_drop(self, _t: Any, value: Any, _x: float, _y: float) -> bool:
        self.widget.remove_css_class("drop-active")
        if isinstance(value, Gio.File) and value.get_path():
            self.load_package_path(Path(value.get_path()))
            return True
        return False

    def _on_search_changed(self, entry: Gtk.SearchEntry) -> None:
        self._filter_query = entry.get_text().strip().lower()
        self._render_workspace_list()

    def _set_group_filter(self, group: str) -> None:
        self._filter_group = group
        self._render_workspace_list()

    def _scan_workspace(self) -> None:
        self._workspace_pkgs = discover_workspace_packages(REPO_ROOT)
        self._render_bundles()
        self._render_workspace_list()

    def _refresh_installed(self) -> None:
        lk = self.runtime.link
        if not lk.connected or lk.link_mode != "rigwire" or lk.fs_backend is None:
            self._installed_names = set()
            self._refresh_ui()
            return
        be = lk.fs_backend

        def _w() -> None:
            try:
                names = {str(e["name"]) for e in be.ls("/apps") if not e.get("is_dir")}
            except Exception:
                names = set()

            def _apply() -> bool:
                self._installed_names = names
                self._refresh_ui()
                return False

            GLib.idle_add(_apply)

        threading.Thread(target=_w, daemon=True).start()

    def _refresh_ui(self) -> None:
        self._render_preview()
        self._render_bundles()
        self._render_workspace_list()

    def _clear_staged(self) -> None:
        self.staged = None
        self.hero_stack.set_visible_child_name("empty")

    def open_package_picker(self) -> None:
        dlg = Gtk.FileDialog(title="Select Package (.krx, .mod, or krx.toml)")

        def _done(d: Any, res: Any) -> None:
            try:
                fpath = d.open_finish(res).get_path()
            except GLib.Error:
                return
            if fpath:
                self.load_package_path(Path(fpath))

        dlg.open(self.win, None, _done)

    def load_package_path(self, fpath: Path) -> None:
        try:
            info = inspect_package_path(fpath)
        except Exception as exc:
            self._on_toast(f"Cannot read package: {exc}")
            return
        self.staged = info
        self._render_preview()
        self.hero_stack.set_visible_child_name("preview")
        # Scroll to top so the cute preview card is immediately visible
        adj = self.scroll.get_vadjustment()
        if adj is not None:
            adj.set_value(0)

    def _stage_workspace_pkg(self, pkg: dict[str, Any]) -> None:
        # Re-inspect fresh from disk in case it was just built
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
        self.hero_stack.set_visible_child_name("preview")
        adj = self.scroll.get_vadjustment()
        if adj is not None:
            adj.set_value(0)

    def _render_preview(self) -> None:
        pkg = self.staged
        if pkg is None:
            self.hero_stack.set_visible_child_name("empty")
            return

        # Icon tile (48px)
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
                size=48,
            )
        )

        self.prev_name.set_text(str(pkg.get("name", "App")))
        ver = str(pkg.get("version", ""))
        self.prev_ver.set_text(ver or "v1.0.0")
        self.prev_ver.set_visible(bool(ver))

        author = str(pkg.get("author", ""))
        remote_path = str(pkg.get("remote_path", ""))
        sub_parts = [p for p in (f"by {author}" if author else "", remote_path) if p]
        self.prev_sub.set_text(" · ".join(sub_parts))

        # Status pill (New Install / Update / Protected / Source Only)
        for cls in ("live", "headless", "dfu", "warn"):
            self.prev_status_pill.remove_css_class(cls)
        fname = str(pkg.get("filename", ""))
        built_ok = bool(pkg.get("built_exists"))
        if not built_ok:
            self.prev_status_pill.set_text("Not Built")
            self.prev_status_pill.add_css_class("dfu")
        elif pkg.get("protected"):
            self.prev_status_pill.set_text("Protected System")
            self.prev_status_pill.add_css_class("dfu")
        elif fname in self._installed_names:
            self.prev_status_pill.set_text("Update")
            self.prev_status_pill.add_css_class("headless")
        else:
            self.prev_status_pill.set_text("New Install")
            self.prev_status_pill.add_css_class("live")

        # Metadata chips
        c = self.prev_meta_chips.get_first_child()
        while c is not None:
            n = c.get_next_sibling()
            self.prev_meta_chips.remove(c)
            c = n

        def _chip(parent: Gtk.FlowBox, text: str, *classes: str, tip: str = "") -> None:
            lbl = Gtk.Label(label=text, tooltip_text=tip)
            for cls in classes:
                lbl.add_css_class(cls)
            parent.append(lbl)

        _chip(self.prev_meta_chips, str(pkg.get("exec_type", "App")), "cap-chip", tip="KRX execution type")
        if pkg.get("profile_label"):
            _chip(self.prev_meta_chips, str(pkg["profile_label"]), "cap-chip", tip="KRX execution profile")
        if built_ok:
            if pkg.get("crc_ok"):
                _chip(self.prev_meta_chips, "CRC-32 OK", "status-pill", "live", tip="Header and payload CRC-32 verified")
            else:
                _chip(self.prev_meta_chips, "CRC Mismatch", "status-pill", "warn", tip="Binary CRC-32 check failed")
            _chip(self.prev_meta_chips, fmt_size(int(pkg.get("size", 0))), "cap-chip", tip="Compiled binary size")
        else:
            _chip(self.prev_meta_chips, "Manifest (krx.toml)", "cap-chip", tip="Source manifest — build before installing")

        for flag in pkg.get("lifecycle", []):
            _chip(self.prev_meta_chips, str(flag), "cap-chip", tip="Lifecycle policy")
        for act in pkg.get("actions", []):
            _chip(self.prev_meta_chips, f"Action: {act}", "cap-chip", tip="Registered system menu action")

        # Capabilities / Permissions chips
        c = self.prev_caps_flow.get_first_child()
        while c is not None:
            n = c.get_next_sibling()
            self.prev_caps_flow.remove(c)
            c = n
        cap_list = list(pkg.get("cap_list", []))
        if not cap_list:
            _chip(self.prev_caps_flow, "No special permissions", "cap-chip")
        else:
            for cap in cap_list:
                if cap["sensitive"]:
                    _chip(
                        self.prev_caps_flow,
                        cap["label"],
                        "cap-chip",
                        "sensitive",
                        tip=f"Sensitive capability ({cap['ident']})",
                    )
                else:
                    _chip(self.prev_caps_flow, cap["label"], "cap-chip", tip=cap["ident"])

        # Action buttons state
        lk = self.runtime.link
        hard = self.runtime.security.hardened
        can_radio = lk.connected and lk.link_mode == "rigwire" and not hard
        has_manifest = pkg.get("source_kind") == "manifest" and bool(pkg.get("pkg_id"))

        if not built_ok and has_manifest:
            self.prev_install_btn.set_label("Build & Install" if can_radio else "Build Package")
            self.prev_install_btn.set_sensitive(not self._busy)
        else:
            verb = "Update on Radio" if (fname in self._installed_names) else "Install to Radio"
            self.prev_install_btn.set_label(verb)
            self.prev_install_btn.set_sensitive(bool(can_radio and built_ok and not self._busy))

        self.prev_build_btn.set_visible(bool(has_manifest and built_ok))
        self.prev_build_btn.set_sensitive(not self._busy)
        self.prev_hex_btn.set_visible(built_ok)

    def _render_bundles(self) -> None:
        c = self.bundles_list.get_first_child()
        while c is not None:
            n = c.get_next_sibling()
            self.bundles_list.remove(c)
            c = n

        by_group: dict[str, list[dict[str, Any]]] = {}
        for p in self._workspace_pkgs:
            by_group.setdefault(str(p.get("group", "")), []).append(p)

        bundles: list[tuple[str, str, str, list[dict[str, Any]]]] = []
        if by_group.get("core"):
            bundles.append(("core", "Core Bundle", "Essential radio applications", by_group["core"]))
        if by_group.get("sys"):
            bundles.append(("sys", "System Modules", "DFU, recovery, calibration & diagnostics", by_group["sys"]))

        self.bundles_section.set_visible(bool(bundles))
        lk = self.runtime.link
        can_radio = lk.connected and lk.link_mode == "rigwire" and not self.runtime.security.hardened

        for _gid, title, desc, pkgs in bundles:
            built = [p for p in pkgs if p.get("built_exists")]
            names = ", ".join(p["name"] for p in pkgs[:4]) + ("…" if len(pkgs) > 4 else "")
            row = Adw.ActionRow(
                title=GLib.markup_escape_text(title),
                subtitle=GLib.markup_escape_text(f"{len(built)}/{len(pkgs)} built · {names or desc}"),
            )
            row.add_prefix(make_app_icon_widget(title, size=32))
            btn = Gtk.Button(label=f"Install ({len(built)})", valign=Gtk.Align.CENTER)
            btn.add_css_class("pill")
            btn.add_css_class("small-pill")
            btn.set_sensitive(bool(can_radio and built and not self._busy))
            btn.connect("clicked", lambda _b, t=title, ps=built: self._confirm_install_bundle(t, ps))
            row.add_suffix(btn)
            self.bundles_list.append(row)

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
            sub = " · ".join(x for x in (grp_lbl, fname, sz_txt) if x)

            row = Adw.ActionRow(
                title=GLib.markup_escape_text(f"{p['name']} {p.get('version', '')}".strip()),
                subtitle=GLib.markup_escape_text(sub),
                activatable=True,
            )
            row.connect("activated", lambda _r, pkg=p: self._stage_workspace_pkg(pkg))
            row.add_prefix(
                make_app_icon_widget(
                    str(p["name"]),
                    exec_type=str(p.get("exec_type", "App")),
                    bitmap=p.get("bitmap"),
                    size=36,
                )
            )

            if installed:
                badge = Gtk.Label(label="Installed", valign=Gtk.Align.CENTER)
                badge.add_css_class("status-pill")
                badge.add_css_class("live")
                row.add_suffix(badge)
            elif not built:
                badge = Gtk.Label(label="Source", valign=Gtk.Align.CENTER)
                badge.add_css_class("cap-chip")
                row.add_suffix(badge)

            inspect_b = Gtk.Button(
                icon_name="document-properties-symbolic",
                valign=Gtk.Align.CENTER,
                tooltip_text=f"Inspect {p['name']} manifest & permissions",
            )
            inspect_b.add_css_class("flat")
            inspect_b.connect("clicked", lambda _b, pkg=p: self._stage_workspace_pkg(pkg))
            row.add_suffix(inspect_b)

            if built:
                act_b = Gtk.Button(label="Update" if installed else "Install", valign=Gtk.Align.CENTER)
                act_b.add_css_class("pill")
                act_b.add_css_class("small-pill")
                act_b.set_sensitive(bool(can_radio and not self._busy))
                act_b.connect("clicked", lambda _b, pkg=p: self._install_package(pkg))
            else:
                act_b = Gtk.Button(label="Build", valign=Gtk.Align.CENTER)
                act_b.add_css_class("pill")
                act_b.add_css_class("small-pill")
                act_b.set_sensitive(not self._busy)
                act_b.connect("clicked", lambda _b, pkg=p: self._build_and_maybe_install(pkg, then_install=False))
            row.add_suffix(act_b)

            self.ws_list.append(row)

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

    def _set_busy(self, busy: bool, text: str = "", frac: float = 0.0) -> None:
        self._busy = busy
        self.prev_prog.set_visible(busy and self.staged is not None)
        if busy:
            self.prev_prog.set_fraction(max(0.0, min(1.0, frac)))
            self.prev_prog.set_text(text)
        self._on_progress(busy, frac, text)
        self._refresh_ui()

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
                    time.sleep(0.25)
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
                GLib.idle_add(lambda: (self._set_busy(False), self._refresh_installed(), False)[2])

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
                        time.sleep(0.12)
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
                GLib.idle_add(lambda: (self._set_busy(False), self._refresh_installed(), False)[2])

        threading.Thread(target=_w, daemon=True).start()
