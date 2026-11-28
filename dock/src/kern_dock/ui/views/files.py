"""Files View: native GNOME Files (Nautilus)-style KernFS browser with sidebar, pathbar & WebDAV."""

from __future__ import annotations

import threading
from pathlib import Path
from typing import Any, Callable

import gi

gi.require_version("Gtk", "4.0")
gi.require_version("Gdk", "4.0")
gi.require_version("Adw", "1")
from gi.repository import Adw, Gdk, Gio, GLib, Gtk  # noqa: E402

import kernfs_mount

from ..runtime import DockRuntime
from ..widgets.hex_view import show_remote_hex_viewer_dialog
from ..widgets.util import file_icon_name, fmt_size

_PLACES: tuple[tuple[str, str, str], ...] = (
    ("/", "Root", "drive-harddisk-symbolic"),
    ("/user", "User Data", "folder-documents-symbolic"),
    ("/pref", "Preferences", "preferences-system-symbolic"),
    ("/apps", "Applications", "application-x-executable-symbolic"),
    ("/sys", "System", "security-high-symbolic"),
)


class FilesView:
    """Native GNOME Files-style KernFS browser with navigation sidebar, search, Hex View & WebDAV."""

    def __init__(
        self,
        win: Adw.ApplicationWindow,
        runtime: DockRuntime,
        *,
        on_toast: Callable[[str], None],
        on_confirm: Callable[[str, str, str, Callable[[], None]], None],
    ) -> None:
        self.win = win
        self.runtime = runtime
        self._on_toast = on_toast
        self._on_confirm = on_confirm
        self.current_dir = "/"
        self._cached_entries: list[dict[str, Any]] = []
        self._place_rows: dict[str, Gtk.ListBoxRow] = {}
        self._updating = False
        self._dirty = True

        self.widget = self._build()
        self.widget.connect("map", lambda _w: self._on_mapped())

        kc = Gtk.EventControllerKey()
        kc.connect("key-pressed", self._on_key)
        self.widget.add_controller(kc)

        runtime.events.subscribe("link.connected", lambda _e: self._on_link_change(), main_thread=True)
        runtime.events.subscribe("link.disconnected", lambda _e: self._on_link_change(), main_thread=True)
        runtime.events.subscribe("security.changed", lambda _e: self._render_filtered(), main_thread=True)
        runtime.events.subscribe("dav.changed", lambda _e: self._sync_dav_ui(), main_thread=True)
        self._render_filtered()

    def _build(self) -> Adw.ToolbarView:
        view = Adw.ToolbarView()

        # === TOP TOOLBAR ===
        top_wrap = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=0)

        bar = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=6)
        bar.add_css_class("toolbar")

        self.up_btn = Gtk.Button(icon_name="go-up-symbolic", tooltip_text="Parent Folder (Alt+Up)")
        self.up_btn.add_css_class("flat")
        self.up_btn.connect("clicked", lambda _b: self._browse_parent())
        bar.append(self.up_btn)

        self.crumb_box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=0)
        self.crumb_box.add_css_class("linked")
        bar.append(self.crumb_box)

        bar.append(Gtk.Box(hexpand=True))

        self.search_toggle = Gtk.ToggleButton(icon_name="system-search-symbolic", tooltip_text="Filter Files (Ctrl+F)")
        self.search_toggle.add_css_class("flat")
        bar.append(self.search_toggle)

        self.upload_btn = Gtk.Button(icon_name="document-send-symbolic", tooltip_text="Upload File…")
        self.upload_btn.add_css_class("flat")
        self.upload_btn.connect("clicked", lambda _b: self._on_upload_dialog())
        bar.append(self.upload_btn)

        self.ref_btn = Gtk.Button(icon_name="view-refresh-symbolic", tooltip_text="Refresh (F5)")
        self.ref_btn.add_css_class("flat")
        self.ref_btn.connect("clicked", lambda _b: self.browse(self.current_dir))
        bar.append(self.ref_btn)

        self.mount_btn = Gtk.ToggleButton(icon_name="drive-harddisk-symbolic", tooltip_text="Mount KernFS in OS File Manager")
        self.mount_btn.add_css_class("flat")
        self.mount_btn.connect("toggled", self._on_mount_toggled)
        bar.append(self.mount_btn)
        top_wrap.append(bar)

        self.search = Gtk.SearchEntry(placeholder_text="Filter current folder…", hexpand=True)
        self.search.connect("search-changed", lambda _e: self._render_filtered())
        self.search_bar = Gtk.SearchBar(child=self.search, show_close_button=True)
        self.search_bar.connect_entry(self.search)
        self.search_toggle.bind_property("active", self.search_bar, "search-mode-enabled", 2)
        top_wrap.append(self.search_bar)

        # Native Adw.Banner when KernFS WebDAV drive is mounted
        self.mount_banner = Adw.Banner(revealed=False, button_label="Open Folder")
        self.mount_banner.connect("button-clicked", self._on_open_fs)
        top_wrap.append(self.mount_banner)

        top_wrap.append(Gtk.Separator(orientation=Gtk.Orientation.HORIZONTAL))
        view.add_top_bar(top_wrap)

        # === SPLIT CONTENT: PLACES NAVIGATION SIDEBAR + FILE LIST ===
        split = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=0, hexpand=True, vexpand=True)

        self.places_list = Gtk.ListBox(selection_mode=Gtk.SelectionMode.SINGLE)
        self.places_list.add_css_class("navigation-sidebar")
        self.places_list.set_size_request(144, -1)
        for path, label, icon in _PLACES:
            row = Gtk.ListBoxRow()
            row._place_path = path  # type: ignore[attr-defined]
            rbox = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
            rbox.append(Gtk.Image.new_from_icon_name(icon))
            rbox.append(Gtk.Label(label=label, xalign=0, ellipsize=3))
            row.set_child(rbox)
            self.places_list.append(row)
            self._place_rows[path] = row
        self.places_list.select_row(self._place_rows["/"])
        self.places_list.connect("row-selected", self._on_place_selected)
        split.append(self.places_list)
        split.append(Gtk.Separator(orientation=Gtk.Orientation.VERTICAL))

        self.stack = Gtk.Stack(vexpand=True, hexpand=True)
        scroll = Gtk.ScrolledWindow(vexpand=True, hexpand=True)
        scroll.set_policy(Gtk.PolicyType.NEVER, Gtk.PolicyType.AUTOMATIC)
        self.file_list = Gtk.ListBox(selection_mode=Gtk.SelectionMode.NONE, valign=Gtk.Align.START)
        self.file_list.add_css_class("file-list")
        self.file_list.connect("row-activated", self._on_row_activated)
        scroll.set_child(self.file_list)
        self.stack.add_named(scroll, "list")

        self.empty = Adw.StatusPage(
            icon_name="folder-symbolic",
            title="No Radio Connected",
            description="Connect a RigWire radio to browse /user, /pref, /apps and /sys.",
        )
        self.empty.add_css_class("compact")
        self.stack.add_named(self.empty, "empty")
        self.stack.set_visible_child_name("empty")
        split.append(self.stack)

        view.set_content(split)

        # === BOTTOM STATUS BAR ===
        bot_wrap = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        bot_wrap.append(Gtk.Separator(orientation=Gtk.Orientation.HORIZONTAL))
        foot = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8, margin_top=5, margin_bottom=5, margin_start=12, margin_end=12)
        self.meta_lbl = Gtk.Label(label="Offline", xalign=0, hexpand=True)
        self.meta_lbl.add_css_class("caption")
        self.meta_lbl.add_css_class("dim-label")
        self.meta_lbl.add_css_class("numeric")
        self.mode_lbl = Gtk.Label(label="Drop files to upload", xalign=1)
        self.mode_lbl.add_css_class("caption")
        self.mode_lbl.add_css_class("dim-label")
        foot.append(self.meta_lbl)
        foot.append(self.mode_lbl)
        bot_wrap.append(foot)
        view.add_bottom_bar(bot_wrap)

        self._rebuild_breadcrumbs()

        drop = Gtk.DropTarget.new(Gio.File, Gdk.DragAction.COPY)
        drop.connect("enter", lambda *_a: (view.add_css_class("drop-active"), Gdk.DragAction.COPY)[1])
        drop.connect("leave", lambda *_a: view.remove_css_class("drop-active"))
        drop.connect("drop", self._on_drop_upload)
        view.add_controller(drop)
        return view

    def _on_place_selected(self, _lb: Any, row: Any) -> None:
        if row is None or self._updating:
            return
        path = getattr(row, "_place_path", None)
        if path:
            self.browse(path)

    def _on_key(self, _c: Any, keyval: int, _kc: int, state: Any) -> bool:
        ctrl = bool(state & Gdk.ModifierType.CONTROL_MASK)
        alt = bool(state & Gdk.ModifierType.ALT_MASK)
        if ctrl and keyval in (Gdk.KEY_f, Gdk.KEY_F):
            self.search_toggle.set_active(not self.search_toggle.get_active())
            if self.search_toggle.get_active():
                self.search.grab_focus()
            return True
        if keyval == Gdk.KEY_F5 or (ctrl and keyval in (Gdk.KEY_r, Gdk.KEY_R)):
            self.browse(self.current_dir)
            return True
        if alt and keyval == Gdk.KEY_Up:
            self._browse_parent()
            return True
        return False

    def _on_mapped(self) -> None:
        if self._dirty and self.runtime.link.connected and self.runtime.link.link_mode == "rigwire":
            self.browse(self.current_dir or "/")

    def _on_link_change(self) -> None:
        self._dirty = True
        self._sync_dav_ui()
        if self.runtime.link.connected and self.runtime.link.link_mode == "rigwire":
            if self.widget.get_mapped():
                self.browse("/")
        else:
            self._dirty = False
            self._cached_entries = []
            self._render_filtered()

    def _sync_dav_ui(self) -> None:
        lk = self.runtime.link
        mounted = lk.dav_server is not None
        self._updating = True
        self.mount_btn.set_active(mounted)
        self.mount_banner.set_revealed(mounted)
        if mounted:
            target = lk.dav_open_target or f"dav://127.0.0.1:{lk.dav_port}/"
            self.mount_banner.set_title(f"Mounted at {target}")
        self._updating = False

    def _browse_parent(self) -> None:
        if self.current_dir == "/":
            return
        parent = str(Path(self.current_dir).parent)
        self.browse(parent if parent.startswith("/") else "/")

    def _rebuild_breadcrumbs(self) -> None:
        c = self.crumb_box.get_first_child()
        while c is not None:
            n = c.get_next_sibling()
            self.crumb_box.remove(c)
            c = n
        self.up_btn.set_sensitive(self.current_dir != "/")
        segs = [s for s in self.current_dir.strip("/").split("/") if s]
        root_b = Gtk.Button(label="/")
        root_b.add_css_class("pathbar-btn")
        root_b.connect("clicked", lambda _b: self.browse("/"))
        self.crumb_box.append(root_b)
        acc = ""
        for seg in segs:
            acc += f"/{seg}"
            sb = Gtk.Button(label=seg)
            sb.add_css_class("pathbar-btn")
            sb.connect("clicked", lambda _b, p=acc: self.browse(p))
            self.crumb_box.append(sb)

    def browse(self, path: str) -> None:
        lk = self.runtime.link
        if not lk.connected or lk.fs_backend is None:
            return
        self._dirty = False
        self.current_dir = path
        self._rebuild_breadcrumbs()
        self._updating = True
        top = "/" if path == "/" else "/" + path.strip("/").split("/")[0]
        if top in self._place_rows:
            self.places_list.select_row(self._place_rows[top])
        self._updating = False
        be = lk.fs_backend

        def _w() -> None:
            try:
                entries = be.ls(path)
            except Exception as e:
                GLib.idle_add(self._on_toast, f"List {path} failed: {e}")
                return
            GLib.idle_add(self._set_entries, entries)

        threading.Thread(target=_w, daemon=True).start()

    def _set_entries(self, entries: list[dict[str, Any]]) -> bool:
        self._cached_entries = entries
        self._render_filtered()
        return False

    def _on_row_activated(self, _lb: Any, row: Any) -> None:
        target = getattr(row, "_dir_path", None)
        if target:
            self.browse(target)
            return
        fpath = getattr(row, "_file_path", None)
        fname = getattr(row, "_file_name", None)
        if fpath and fname:
            self._on_hex_view(fpath, fname)

    def _render_filtered(self) -> None:
        c = self.file_list.get_first_child()
        while c is not None:
            n = c.get_next_sibling()
            self.file_list.remove(c)
            c = n
        q = self.search.get_text().strip().lower()
        shown = [e for e in self._cached_entries if (not q or q in e["name"].lower())]
        shown.sort(key=lambda e: (not bool(e["is_dir"]), e["name"].lower()))
        total_b = sum(int(e["size"]) for e in shown if not e["is_dir"])

        online = self.runtime.link.connected and self.runtime.link.link_mode == "rigwire"
        hard = self.runtime.security.hardened
        ro = self.runtime.security.policy.read_only
        self.upload_btn.set_sensitive(online and not ro)
        self.upload_btn.set_tooltip_text(
            "Disabled in Safe Mode"
            if hard
            else ("Disabled in Read-Only mode" if ro else ("Connect a RigWire radio first" if not online else "Upload file to radio"))
        )
        self.ref_btn.set_sensitive(online)
        self.mount_btn.set_sensitive(online)
        self.up_btn.set_sensitive(online and self.current_dir != "/")
        self.places_list.set_sensitive(online)

        if not online:
            self.meta_lbl.set_text("Offline")
            self.mode_lbl.set_text("")
        else:
            self.meta_lbl.set_text(f"{len(shown)} {'item' if len(shown) == 1 else 'items'} · {fmt_size(total_b)}")
            self.mode_lbl.set_text("Read-only" if ro else "Drop files to upload")

        if not shown:
            if not self.runtime.link.connected:
                self.empty.set_title("No Radio Connected")
                self.empty.set_description("Connect a RigWire radio to browse /user, /pref, /apps and /sys.")
            elif q:
                self.empty.set_title("No Matching Files")
                self.empty.set_description(f'Nothing in {self.current_dir} matches "{q}".')
            else:
                self.empty.set_title("Empty Folder")
                self.empty.set_description(f"No files in {self.current_dir}.")
            self.stack.set_visible_child_name("empty")
            return

        self.stack.set_visible_child_name("list")
        for e in shown:
            name, is_dir, size = e["name"], bool(e["is_dir"]), int(e["size"])
            full = f"/{name}" if self.current_dir == "/" else f"{self.current_dir.rstrip('/')}/{name}"
            protected = full.startswith("/sys/") or full in kernfs_mount.PROTECTED_RECOVERY_MODS

            row = Gtk.ListBoxRow(activatable=True)
            if is_dir:
                row._dir_path = full  # type: ignore[attr-defined]
            else:
                row._file_path = full  # type: ignore[attr-defined]
                row._file_name = name  # type: ignore[attr-defined]

            line = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=10)
            line.append(Gtk.Image.new_from_icon_name(file_icon_name(name, full, is_dir)))
            line.append(Gtk.Label(label=name, xalign=0, hexpand=True, ellipsize=3))

            if protected and not is_dir:
                lock = Gtk.Image(icon_name="security-high-symbolic", tooltip_text="Protected system file")
                lock.add_css_class("dim-label")
                line.append(lock)

            sz = Gtk.Label(label="" if is_dir else fmt_size(size), xalign=1)
            sz.add_css_class("file-size")
            line.append(sz)

            if is_dir:
                chev = Gtk.Image.new_from_icon_name("go-next-symbolic")
                chev.add_css_class("dim-label")
                line.append(chev)
            else:
                hv = Gtk.Button(icon_name="accessories-text-editor-symbolic", tooltip_text=f"Inspect Hex ({name})", valign=Gtk.Align.CENTER)
                hv.add_css_class("flat")
                hv.connect("clicked", lambda _b, p=full, n=name: self._on_hex_view(p, n))
                line.append(hv)

                dl = Gtk.Button(icon_name="document-save-symbolic", tooltip_text=f"Download {name}", valign=Gtk.Align.CENTER)
                dl.add_css_class("flat")
                dl.connect("clicked", lambda _b, p=full, n=name: self._on_download(p, n))
                line.append(dl)

                rm_tip = (
                    "Protected system file"
                    if protected
                    else ("Disabled in Safe Mode" if hard else ("Disabled in Read-Only mode" if ro else f"Delete {name}"))
                )
                rm = Gtk.Button(
                    icon_name="user-trash-symbolic",
                    tooltip_text=rm_tip,
                    valign=Gtk.Align.CENTER,
                )
                rm.add_css_class("flat")
                rm.set_sensitive(not protected and not ro)
                rm.connect(
                    "clicked",
                    lambda _b, p=full, n=name: self._on_confirm(
                        f"Delete {n}?",
                        f"{p} will be permanently removed from the radio.",
                        "Delete",
                        lambda: self._delete_file(p),
                    ),
                )
                line.append(rm)

            row.set_child(line)
            self.file_list.append(row)

    def upload_local_file(self, src_path: Path) -> None:
        lk = self.runtime.link
        if not lk.connected or lk.fs_backend is None:
            self._on_toast("Connect a RigWire radio first")
            return
        tdir = self.current_dir if self.current_dir != "/" else "/user"
        remote = f"{tdir.rstrip('/')}/{src_path.name}"
        be = lk.fs_backend

        def _w() -> None:
            try:
                data = src_path.read_bytes()
                clean = self.runtime.security.check_write(remote, data)
                be.write_bytes(clean, data)
                GLib.idle_add(self._on_toast, f"Uploaded {clean}")
                GLib.idle_add(self.browse, tdir)
            except Exception as exc:
                GLib.idle_add(self._on_toast, f"Upload blocked: {exc}")

        threading.Thread(target=_w, daemon=True).start()

    def _on_drop_upload(self, _target: Any, value: Any, _x: float, _y: float) -> bool:
        self.widget.remove_css_class("drop-active")
        if isinstance(value, Gio.File) and value.get_path():
            self.upload_local_file(Path(value.get_path()))
            return True
        return False

    def _on_upload_dialog(self) -> None:
        if not self.runtime.link.connected or self.runtime.link.fs_backend is None:
            self._on_toast("Connect a RigWire radio first")
            return
        dlg = Gtk.FileDialog(title=f"Upload to {self.current_dir}")

        def _picked(d: Any, res: Any) -> None:
            try:
                src = d.open_finish(res).get_path()
            except GLib.Error:
                return
            if src:
                self.upload_local_file(Path(src))

        dlg.open(self.win, None, _picked)

    def _on_hex_view(self, remote_path: str, filename: str) -> None:
        be = self.runtime.link.fs_backend
        if be is None:
            return
        show_remote_hex_viewer_dialog(self.win, be, remote_path, filename)

    def _on_download(self, remote_path: str, filename: str) -> None:
        be = self.runtime.link.fs_backend
        if be is None:
            return
        dlg = Gtk.FileDialog(title=f"Save {filename}")
        dlg.set_initial_name(filename)

        def _picked(d: Any, res: Any) -> None:
            try:
                dest = d.save_finish(res).get_path()
            except GLib.Error:
                return
            if not dest:
                return

            def _w() -> None:
                try:
                    data = be.read_bytes(remote_path)
                    Path(dest).write_bytes(data)
                    GLib.idle_add(self._on_toast, f"Saved {Path(dest).name} ({fmt_size(len(data))})")
                except Exception as exc:
                    GLib.idle_add(self._on_toast, f"Download failed: {exc}")

            threading.Thread(target=_w, daemon=True).start()

        dlg.save(self.win, None, _picked)

    def _delete_file(self, remote_path: str) -> None:
        be = self.runtime.link.fs_backend
        if be is None:
            return

        def _w() -> None:
            try:
                clean = self.runtime.security.check_delete(remote_path)
                be.remove(clean)
                GLib.idle_add(self._on_toast, f"Deleted {clean}")
                GLib.idle_add(self.browse, self.current_dir)
            except Exception as exc:
                GLib.idle_add(self._on_toast, f"Delete failed: {exc}")

        threading.Thread(target=_w, daemon=True).start()

    def _on_mount_toggled(self, btn: Any) -> None:
        if self._updating:
            return
        lk = self.runtime.link
        want = bool(btn.get_active())
        if want:
            if not lk.connected or lk.link_mode != "rigwire":
                self._updating = True
                btn.set_active(False)
                self._updating = False
                self._on_toast("Connect a RigWire radio first")
                return
            ro = self.runtime.security.policy.read_only

            def _w() -> None:
                try:
                    port = lk.start_dav(http_port=0, read_only=ro)
                    GLib.idle_add(self._on_toast, f"Mounted KernFS on dav://127.0.0.1:{port}/")
                except Exception as exc:
                    GLib.idle_add(self._on_toast, f"Mount failed: {exc}")
                finally:
                    GLib.idle_add(self._sync_dav_ui)

            threading.Thread(target=_w, daemon=True).start()
        else:
            lk.stop_dav()
            self._sync_dav_ui()
            self._on_toast("Unmounted radio drive")

    def _on_open_fs(self, _w: Any) -> None:
        lk = self.runtime.link
        target = lk.dav_open_target or (f"dav://127.0.0.1:{lk.dav_port}/" if lk.dav_server else "")
        if not target:
            return
        try:
            Gio.AppInfo.launch_default_for_uri(
                target if "://" in target else Path(target).as_uri(),
                None,
            )
        except Exception as exc:
            self._on_toast(f"Open failed: {exc}")
