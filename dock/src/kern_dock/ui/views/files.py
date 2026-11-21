"""Files View: native GNOME Files-style KernFS browser with pathbar, collapsible search & WebDAV mount."""

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
from ..widgets.hex_view import show_hex_viewer_dialog
from ..widgets.util import file_icon_name, fmt_size


class FilesView:
    """Compact GNOME Files-style KernFS file browser with DnD and local WebDAV mounting."""

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
        self._place_btns: dict[str, Gtk.ToggleButton] = {}
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

    def _build(self) -> Gtk.Box:
        root = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=0)

        # --- Compact Pathbar & Action Toolbar ---
        bar = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=6, margin_top=8, margin_bottom=6, margin_start=12, margin_end=12)

        self.up_btn = Gtk.Button(icon_name="go-up-symbolic", tooltip_text="Parent Folder")
        self.up_btn.add_css_class("flat")
        self.up_btn.update_property([Gtk.AccessibleProperty.LABEL], ["Parent Folder"])
        self.up_btn.connect("clicked", lambda _b: self._browse_parent())
        bar.append(self.up_btn)

        self.crumb_box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=0)
        self.crumb_box.add_css_class("linked")
        bar.append(self.crumb_box)

        bar.append(Gtk.Box(hexpand=True))

        self.search_toggle = Gtk.ToggleButton(icon_name="system-search-symbolic", tooltip_text="Filter Files")
        self.search_toggle.add_css_class("flat")
        self.search_toggle.update_property([Gtk.AccessibleProperty.LABEL], ["Filter Files"])
        bar.append(self.search_toggle)

        self.upload_btn = Gtk.Button(icon_name="document-send-symbolic", tooltip_text="Upload File…")
        self.upload_btn.add_css_class("flat")
        self.upload_btn.update_property([Gtk.AccessibleProperty.LABEL], ["Upload File"])
        self.upload_btn.connect("clicked", lambda _b: self._on_upload_dialog())
        bar.append(self.upload_btn)

        self.ref_btn = Gtk.Button(icon_name="view-refresh-symbolic", tooltip_text="Refresh Folder")
        self.ref_btn.add_css_class("flat")
        self.ref_btn.update_property([Gtk.AccessibleProperty.LABEL], ["Refresh Folder"])
        self.ref_btn.connect("clicked", lambda _b: self.browse(self.current_dir))
        bar.append(self.ref_btn)

        self.mount_btn = Gtk.ToggleButton(icon_name="drive-harddisk-symbolic", tooltip_text="Mount KernFS in OS File Manager")
        self.mount_btn.add_css_class("flat")
        self.mount_btn.update_property([Gtk.AccessibleProperty.LABEL], ["Mount Radio Storage"])
        self.mount_btn.connect("toggled", self._on_mount_toggled)
        bar.append(self.mount_btn)

        self.open_fs_btn = Gtk.Button(icon_name="folder-open-symbolic", visible=False, tooltip_text="Open in OS File Manager")
        self.open_fs_btn.add_css_class("flat")
        self.open_fs_btn.update_property([Gtk.AccessibleProperty.LABEL], ["Open in OS File Manager"])
        self.open_fs_btn.connect("clicked", self._on_open_fs)
        bar.append(self.open_fs_btn)
        root.append(bar)

        # --- Collapsible Search Bar ---
        self.search = Gtk.SearchEntry(placeholder_text="Filter current folder…", hexpand=True)
        self.search.connect("search-changed", lambda _e: self._render_filtered())
        self.search_bar = Gtk.SearchBar(child=self.search, show_close_button=True)
        self.search_bar.connect_entry(self.search)
        self.search_toggle.bind_property("active", self.search_bar, "search-mode-enabled", 2)
        root.append(self.search_bar)

        # --- Compact Quick-Places Bar ---
        places = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=4, margin_bottom=6, margin_start=12, margin_end=12)
        first = None
        for d, lbl in (("/", "Root"), ("/user", "user"), ("/pref", "pref"), ("/apps", "apps"), ("/sys", "sys")):
            b = Gtk.ToggleButton(label=lbl, active=(d == "/"))
            b.add_css_class("flat")
            b.add_css_class("place-btn")
            if first is None:
                first = b
            else:
                b.set_group(first)
            b.connect("toggled", lambda btn, p=d: self.browse(p) if (btn.get_active() and not self._updating) else None)
            places.append(b)
            self._place_btns[d] = b
        root.append(places)

        # --- Inline Mounted Drive Status Strip (shown while KernFS is mounted) ---
        self.mount_strip = Gtk.Box(
            orientation=Gtk.Orientation.HORIZONTAL,
            spacing=8,
            visible=False,
            margin_bottom=6,
            margin_start=12,
            margin_end=12,
        )
        self.mount_strip.add_css_class("card")
        self.mount_strip.add_css_class("stat-tile")
        self.mount_strip.append(Gtk.Image(icon_name="drive-harddisk-symbolic"))
        self.mount_strip_lbl = Gtk.Label(label="", xalign=0, hexpand=True, ellipsize=3)
        self.mount_strip_lbl.add_css_class("caption")
        self.mount_strip_lbl.add_css_class("monospace")
        self.mount_strip.append(self.mount_strip_lbl)

        open_b = Gtk.Button(label="Open")
        open_b.add_css_class("flat")
        open_b.connect("clicked", self._on_open_fs)
        self.mount_strip.append(open_b)

        copy_b = Gtk.Button(icon_name="edit-copy-symbolic", tooltip_text="Copy Mount Path / URL")
        copy_b.add_css_class("flat")
        copy_b.connect("clicked", self._on_copy_mount_url)
        self.mount_strip.append(copy_b)

        unm_b = Gtk.Button(icon_name="media-eject-symbolic", tooltip_text="Unmount Radio Drive")
        unm_b.add_css_class("flat")
        unm_b.connect("clicked", lambda _b: self.mount_btn.set_active(False))
        self.mount_strip.append(unm_b)
        root.append(self.mount_strip)

        root.append(Gtk.Separator(orientation=Gtk.Orientation.HORIZONTAL))

        # --- Compact File List / Status Stack ---
        self.stack = Gtk.Stack(vexpand=True)
        scroll = Gtk.ScrolledWindow(vexpand=True)
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
        root.append(self.stack)

        # --- Subtle 1-Line Footer Status Bar ---
        root.append(Gtk.Separator(orientation=Gtk.Orientation.HORIZONTAL))
        foot = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8, margin_top=4, margin_bottom=4, margin_start=12, margin_end=12)
        self.meta_lbl = Gtk.Label(label="", xalign=0, hexpand=True)
        self.meta_lbl.add_css_class("caption")
        self.meta_lbl.add_css_class("dim-label")
        self.meta_lbl.add_css_class("numeric")
        hint = Gtk.Label(label="Drop files to upload", xalign=1)
        hint.add_css_class("caption")
        hint.add_css_class("dim-label")
        foot.append(self.meta_lbl)
        foot.append(hint)
        root.append(foot)

        self._rebuild_breadcrumbs()

        drop = Gtk.DropTarget.new(Gio.File, Gdk.DragAction.COPY)
        drop.connect("drop", self._on_drop_upload)
        root.add_controller(drop)
        return root

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
        self.open_fs_btn.set_visible(mounted)
        self.mount_strip.set_visible(mounted)
        if mounted:
            target = lk.dav_open_target or f"dav://127.0.0.1:{lk.dav_port}/"
            self.mount_strip_lbl.set_text(f"Mounted · {target}")
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
        if top in self._place_btns:
            self._place_btns[top].set_active(True)
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
        ro = self.runtime.security.policy.read_only
        self.upload_btn.set_sensitive(online and not ro)
        self.ref_btn.set_sensitive(online)
        self.mount_btn.set_sensitive(online)
        self.up_btn.set_sensitive(online and self.current_dir != "/")
        for pb in self._place_btns.values():
            pb.set_sensitive(online)

        if not online:
            self.meta_lbl.set_text("Offline")
        else:
            self.meta_lbl.set_text(f"{len(shown)} {'item' if len(shown) == 1 else 'items'} · {fmt_size(total_b)}")

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
        ro = self.runtime.security.policy.read_only
        for e in shown:
            name, is_dir, size = e["name"], bool(e["is_dir"]), int(e["size"])
            full = f"/{name}" if self.current_dir == "/" else f"{self.current_dir.rstrip('/')}/{name}"
            protected = full.startswith("/sys/") or full in kernfs_mount.PROTECTED_RECOVERY_MODS

            row = Gtk.ListBoxRow(activatable=is_dir)
            if is_dir:
                row._dir_path = full  # type: ignore[attr-defined]
            line = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=10)
            line.append(Gtk.Image.new_from_icon_name(file_icon_name(name, full, is_dir)))

            nlbl = Gtk.Label(label=name, xalign=0, hexpand=True, ellipsize=3)
            line.append(nlbl)

            if protected and not is_dir:
                pb = Gtk.Label(label="Protected", valign=Gtk.Align.CENTER)
                pb.add_css_class("protected-badge")
                line.append(pb)

            sz = Gtk.Label(label="" if is_dir else fmt_size(size), xalign=1)
            sz.add_css_class("file-size")
            line.append(sz)

            if is_dir:
                chev = Gtk.Image.new_from_icon_name("go-next-symbolic")
                chev.add_css_class("dim-label")
                line.append(chev)
            else:
                hv = Gtk.Button(icon_name="accessories-text-editor-symbolic", tooltip_text=f"Colored Hex View ({name})", valign=Gtk.Align.CENTER)
                hv.add_css_class("flat")
                hv.connect("clicked", lambda _b, p=full, n=name: self._on_hex_view(p, n))
                line.append(hv)

                dl = Gtk.Button(icon_name="document-save-symbolic", tooltip_text=f"Download {name}", valign=Gtk.Align.CENTER)
                dl.add_css_class("flat")
                dl.connect("clicked", lambda _b, p=full, n=name: self._on_download(p, n))
                line.append(dl)

                rm = Gtk.Button(
                    icon_name="user-trash-symbolic",
                    tooltip_text="Protected system file" if protected else f"Delete {name}",
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

        def _w() -> None:
            try:
                data = be.read_bytes(remote_path)
                GLib.idle_add(show_hex_viewer_dialog, self.win, data, filename)
            except Exception as exc:
                GLib.idle_add(self._on_toast, f"Read {filename} failed: {exc}")

        threading.Thread(target=_w, daemon=True).start()

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
            if dest:
                threading.Thread(
                    target=lambda: (Path(dest).write_bytes(be.read_bytes(remote_path)), GLib.idle_add(self._on_toast, f"Saved {Path(dest).name}")),
                    daemon=True,
                ).start()

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
                GLib.idle_add(self._on_toast, f"Delete blocked: {exc}")

        threading.Thread(target=_w, daemon=True).start()

    def _on_mount_toggled(self, btn: Any) -> None:
        if self._updating:
            return
        lk = self.runtime.link
        if not btn.get_active():
            if lk.dav_server is not None:
                self.mount_btn.set_sensitive(False)

                def _unmount() -> None:
                    lk.stop_dav()
                    GLib.idle_add(self._finish_unmount)

                threading.Thread(target=_unmount, daemon=True, name="kern-dock-unmount").start()
            return
        if not lk.connected or lk.fs_backend is None:
            btn.set_active(False)
            self._on_toast("Connect a RigWire radio first")
            return
        self.mount_btn.set_sensitive(False)
        self._on_toast(f"Mounting {lk.volume_name}…")

        def _mount() -> None:
            try:
                url = lk.start_dav(http_port=8765)
                open_t, unmount_c = kernfs_mount.try_auto_mount(url, lk.volume_name)
                lk.dav_open_target = open_t or f"dav://127.0.0.1:{lk.dav_port}/"
                lk.dav_unmount_cmd = unmount_c
                GLib.idle_add(self._finish_mount, True, lk.dav_open_target, open_t is not None)
            except Exception as exc:
                lk.stop_dav()
                GLib.idle_add(self._finish_mount, False, str(exc), False)

        threading.Thread(target=_mount, daemon=True, name="kern-dock-mount").start()

    def _finish_mount(self, ok: bool, detail: str, auto_mounted: bool) -> bool:
        self.mount_btn.set_sensitive(True)
        self._sync_dav_ui()
        if ok:
            lk = self.runtime.link
            if auto_mounted:
                self._on_toast(f"Mounted {lk.volume_name}")
                kernfs_mount.open_in_file_manager(detail)
            else:
                self._on_toast(f"WebDAV ready at {detail} (open in file manager)")
        else:
            self._updating = True
            self.mount_btn.set_active(False)
            self._updating = False
            self._on_toast(f"Mount failed: {detail}")
        return False

    def _finish_unmount(self) -> bool:
        self.mount_btn.set_sensitive(True)
        self._sync_dav_ui()
        self._on_toast("Unmounted radio drive")
        return False

    def _on_copy_mount_url(self, _btn: Any) -> None:
        lk = self.runtime.link
        if lk.dav_server is None:
            return
        target = lk.dav_open_target or f"dav://127.0.0.1:{lk.dav_port}/"
        disp = Gdk.Display.get_default()
        if disp is not None:
            disp.get_clipboard().set(target)
        self._on_toast(f"Copied {target}")

    def _on_open_fs(self, _btn: Any) -> None:
        lk = self.runtime.link
        if lk.dav_server is not None:
            kernfs_mount.open_in_file_manager(lk.dav_open_target or f"dav://127.0.0.1:{lk.dav_port}/")
