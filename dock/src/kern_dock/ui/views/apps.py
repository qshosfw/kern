"""Apps View: Dedicated KernFS /apps KRX & MOD package manager with colored hex inspection."""

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
from ..widgets.app_icon import inspect_krx_metadata, make_app_icon_widget
from ..widgets.hex_view import show_hex_viewer_dialog
from ..widgets.util import fmt_size


class AppsView:
    """Dedicated GNOME Software-style manager for installed /apps .krx & .mod packages."""

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
        self._installed_apps: list[dict[str, Any]] = []
        self._meta_cache: dict[tuple[str, str, int], dict[str, Any]] = {}
        self._dirty: bool = True
        self._refresh_gen: int = 0

        self.widget = self._build()
        self.widget.connect("map", lambda _w: self._on_mapped())

        runtime.events.subscribe("link.connected", lambda _e: self._on_link_changed(), main_thread=True)
        runtime.events.subscribe("link.disconnected", lambda _e: self._on_link_changed(), main_thread=True)
        runtime.events.subscribe("security.changed", lambda _e: self._render_apps(), main_thread=True)
        self._render_apps()

    def _build(self) -> Gtk.Box:
        root = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=8, margin_top=10, margin_bottom=12, margin_start=14, margin_end=14)

        abar = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        albl = Gtk.Label(label="Installed Apps (/apps)", xalign=0, hexpand=True)
        albl.add_css_class("heading")
        abar.append(albl)

        ref_a = Gtk.Button(icon_name="view-refresh-symbolic", tooltip_text="Refresh Installed Apps")
        ref_a.add_css_class("flat")
        ref_a.connect("clicked", lambda _b: self.refresh_apps())
        abar.append(ref_a)

        self.install_btn = Gtk.Button(label="Install App…")
        self.install_btn.add_css_class("suggested-action")
        self.install_btn.add_css_class("pill")
        self.install_btn.add_css_class("small-pill")
        self.install_btn.connect("clicked", lambda _b: self.open_sideload_picker())
        abar.append(self.install_btn)
        root.append(abar)

        self.apps_stack = Gtk.Stack(vexpand=True)
        ascroll = Gtk.ScrolledWindow(vexpand=True)
        ascroll.set_policy(Gtk.PolicyType.NEVER, Gtk.PolicyType.AUTOMATIC)
        self.apps_list = Gtk.ListBox(selection_mode=Gtk.SelectionMode.NONE, valign=Gtk.Align.START)
        self.apps_list.add_css_class("boxed-list")
        ascroll.set_child(Adw.Clamp(maximum_size=640, child=self.apps_list))
        self.apps_stack.add_named(ascroll, "list")

        self.apps_empty = Adw.StatusPage(
            icon_name="application-x-executable-symbolic",
            title="No Radio Connected",
            description="Connect a Kern OS radio to view, inspect, or install applications.",
        )
        self.apps_empty.add_css_class("compact")
        self.apps_stack.add_named(self.apps_empty, "empty")
        self.apps_stack.set_visible_child_name("empty")
        root.append(self.apps_stack)

        drop = Gtk.DropTarget.new(Gio.File, Gdk.DragAction.COPY)
        drop.connect("drop", self._on_drop)
        root.add_controller(drop)
        return root

    def _on_drop(self, _t: Any, value: Any, _x: float, _y: float) -> bool:
        if isinstance(value, Gio.File) and value.get_path():
            self.sideload_path(Path(value.get_path()))
            return True
        return False

    def _on_link_changed(self) -> None:
        self._dirty = True
        lk = self.runtime.link
        if not lk.connected or lk.link_mode != "kern_os":
            self._installed_apps = []
            self._render_apps()
            return
        if self.widget.get_mapped():
            self.refresh_apps()

    def _on_mapped(self) -> None:
        if self._dirty:
            self.refresh_apps()

    def _device_cache_key(self) -> str:
        lk = self.runtime.link
        return str(lk.last_telemetry.get("serial_str") or lk.port or lk.volume_name or "radio")

    def refresh_apps(self, *, force_headers: bool = False) -> None:
        lk = self.runtime.link
        self._refresh_gen += 1
        gen = self._refresh_gen
        if not lk.connected or lk.link_mode != "kern_os" or lk.fs_backend is None:
            self._dirty = False
            self._installed_apps = []
            self._render_apps()
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

            # Phase 1: Render immediately from directory listing + in-memory cache (0 flash reads).
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

            # Phase 2: Lazily read only tiny KRX headers (<= 512 B, no full-file CRC/download) for uncached apps.
            if not pending:
                return
            updated = False
            for idx, full, name, size in pending:
                if gen != self._refresh_gen:
                    return
                try:
                    head = be.read_head(full, 512)
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
        self._render_apps()
        return False

    def _render_apps(self) -> None:
        c = self.apps_list.get_first_child()
        while c is not None:
            n = c.get_next_sibling()
            self.apps_list.remove(c)
            c = n
        lk = self.runtime.link
        self.install_btn.set_sensitive(not self.runtime.security.hardened)
        if not lk.connected or lk.link_mode != "kern_os":
            self.apps_empty.set_title("No Radio Connected")
            self.apps_empty.set_description("Connect a Kern OS radio to manage installed applications.")
            self.apps_stack.set_visible_child_name("empty")
            return
        if not self._installed_apps:
            self.apps_empty.set_title("No Installed Apps")
            self.apps_empty.set_description("Install an application or drop one onto this window.")
            self.apps_stack.set_visible_child_name("empty")
            return

        self.apps_stack.set_visible_child_name("list")
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
                    size=36,
                )
            )

            tag = Gtk.Label(label="System" if protected else exec_type, valign=Gtk.Align.CENTER)
            tag.add_css_class("protected-badge" if protected else "cap-chip")
            row.add_suffix(tag)

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

    def _inspect_app_hex(self, remote_path: str, name: str) -> None:
        be = self.runtime.link.fs_backend
        if be is None:
            return

        def _w() -> None:
            try:
                data = be.read_bytes(remote_path)
                GLib.idle_add(show_hex_viewer_dialog, self.win, data, name)
            except Exception as exc:
                GLib.idle_add(self._on_toast, f"Read failed: {exc}")

        threading.Thread(target=_w, daemon=True).start()

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
        try:
            self.runtime.security.check_sideload()
        except PermissionError as exc:
            self._on_toast(str(exc))
            return
        dlg = Gtk.FileDialog(title="Install .krx / .mod App")

        def _done(d: Any, res: Any) -> None:
            try:
                fpath = d.open_finish(res).get_path()
            except GLib.Error:
                return
            if fpath:
                self.sideload_path(Path(fpath))

        dlg.open(self.win, None, _done)

    def sideload_path(self, fpath: Path) -> None:
        try:
            self.runtime.security.check_sideload()
        except PermissionError as exc:
            self._on_toast(str(exc))
            return
        lk = self.runtime.link
        if not lk.connected or lk.demo_mode or not lk.client:
            self._on_toast("Connect a live radio (or QEMU) to install applications")
            return

        def _w() -> None:
            try:
                with lk.lock:
                    info = lk.client.sideload(str(fpath), verify=True, force=True)  # type: ignore[union-attr]
                GLib.idle_add(self._on_toast, f"Installed {info['remote_path']}")
                GLib.idle_add(self.refresh_apps)
            except Exception as exc:
                GLib.idle_add(self._on_toast, f"Install failed: {exc}")

        threading.Thread(target=_w, daemon=True).start()
