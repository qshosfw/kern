"""Firmware Flasher View: clean, native GNOME Firmware-style bootloader & .kfw/.bin flasher."""

from __future__ import annotations

import dataclasses
import math
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
from ...device.firmware import (
    FLASH_TOTAL_BYTES,
    discover_local_firmware,
    download_firmware_url,
    fetch_github_releases,
    flash_via_stock_bootloader,
    inspect_firmware_file,
    parse_github_repo_slug,
)
from ...device.link import REBOOT_BOOTLOADER
from ..runtime import DockRuntime
from ..widgets.hex_view import show_hex_viewer_dialog
from ..widgets.util import fmt_size


class FirmwareView:
    """Clean, native Libadwaita Firmware Flasher with pre-flight verification & live progress."""

    def __init__(
        self,
        win: Adw.ApplicationWindow,
        runtime: DockRuntime,
        *,
        on_toast: Callable[[str], None],
        on_confirm: Callable[[str, str, str, Callable[[], None]], None],
        on_confirm_reboot: Callable[[int], None],
        on_progress: Callable[[bool, float, str], None],
        on_leave_demo_bl: Callable[[], None],
        on_stage_redraw: Callable[[], None],
        on_open_tab: Callable[[str], None] | None = None,
    ) -> None:
        self.win = win
        self.runtime = runtime
        self._on_toast = on_toast
        self._on_confirm = on_confirm
        self._on_confirm_reboot = on_confirm_reboot
        self._on_progress = on_progress
        self._on_leave_demo_bl = on_leave_demo_bl
        self._on_stage_redraw = on_stage_redraw
        self._on_open_tab = on_open_tab
        self.selected_fw: dict[str, Any] | None = None
        self._flash_t0: float = 0.0
        self._last_flash_summary: str = ""
        self._local_rows: list[Gtk.Widget] = []
        self._remote_rows: list[Gtk.Widget] = []
        self._remote_busy: bool = False

        self.widget = self._build()
        self.widget.connect("map", lambda _w: self._refresh_local_builds())

        runtime.events.subscribe("link.connected", lambda _e: self.refresh(), main_thread=True)
        runtime.events.subscribe("link.disconnected", lambda _e: self.refresh(), main_thread=True)
        runtime.events.subscribe("security.changed", lambda _e: self.refresh(), main_thread=True)
        self._refresh_local_builds()
        self.refresh()

    def _build(self) -> Gtk.Box:
        root = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        page = Adw.PreferencesPage()

        # === DEVICE / BOOTLOADER ===
        grp_state = Adw.PreferencesGroup(title="Radio Bootloader")
        self.bl_row = Adw.ActionRow(title="No Radio Connected", subtitle="Connect via USB, or hold PTT while powering on")
        self.bl_icon = Gtk.Image(icon_name="application-x-firmware-symbolic")
        self.bl_row.add_prefix(self.bl_icon)

        self.backup_btn = Gtk.Button(
            icon_name="drive-harddisk-symbolic",
            valign=Gtk.Align.CENTER,
            tooltip_text="Open Backups before flashing",
            visible=False,
        )
        self.backup_btn.add_css_class("flat")
        self.backup_btn.connect("clicked", lambda _b: self._on_open_tab("backups") if self._on_open_tab else None)
        self.bl_row.add_suffix(self.backup_btn)

        self.bl_btn = Gtk.Button(label="Enter Bootloader", valign=Gtk.Align.CENTER)
        self.bl_btn.connect("clicked", lambda _b: self._on_confirm_reboot(REBOOT_BOOTLOADER))
        self.bl_row.add_suffix(self.bl_btn)

        self.leave_demo_btn = Gtk.Button(label="Exit Bootloader", valign=Gtk.Align.CENTER, visible=False)
        self.leave_demo_btn.connect("clicked", lambda _b: self._on_leave_demo_bl())
        self.bl_row.add_suffix(self.leave_demo_btn)

        grp_state.add(self.bl_row)
        page.add(grp_state)

        # === FIRMWARE IMAGE ===
        grp_img = Adw.PreferencesGroup(title="Firmware Image")
        self.img_row = Adw.ActionRow(title="No Image Selected", subtitle="Drop or choose a .kfw or .bin file")
        self.img_icon = Gtk.Image(icon_name="package-x-generic-symbolic")
        self.img_row.add_prefix(self.img_icon)

        self.hex_btn = Gtk.Button(
            icon_name="accessories-text-editor-symbolic",
            valign=Gtk.Align.CENTER,
            tooltip_text="Inspect Firmware Hex",
            visible=False,
        )
        self.hex_btn.add_css_class("flat")
        self.hex_btn.connect("clicked", lambda _b: self._inspect_hex())
        self.img_row.add_suffix(self.hex_btn)

        self.clear_btn = Gtk.Button(
            icon_name="edit-clear-symbolic",
            valign=Gtk.Align.CENTER,
            tooltip_text="Clear Image",
            visible=False,
        )
        self.clear_btn.add_css_class("flat")
        self.clear_btn.connect("clicked", lambda _b: self._clear_firmware())
        self.img_row.add_suffix(self.clear_btn)

        pick_btn = Gtk.Button(label="Choose…", valign=Gtk.Align.CENTER)
        pick_btn.connect("clicked", lambda _b: self.open_firmware_picker())
        self.img_row.add_suffix(pick_btn)
        grp_img.add(self.img_row)

        # Native Libadwaita property rows for pre-flight details (only shown when an image is loaded)
        self.prop_format = Adw.ActionRow(title="Format &amp; Target", subtitle="", visible=False)
        self.prop_format.add_css_class("property")
        grp_img.add(self.prop_format)

        self.prop_size = Adw.ActionRow(title="Flash Footprint", subtitle="", visible=False)
        self.prop_size.add_css_class("property")
        self.flash_bar = Gtk.LevelBar(min_value=0.0, max_value=1.0, value=0.0, valign=Gtk.Align.CENTER)
        self.flash_bar.set_size_request(110, -1)
        self.flash_bar.add_css_class("stat-bar")
        self.flash_bar.add_offset_value("low", 0.65)
        self.flash_bar.add_offset_value("high", 0.85)
        self.flash_bar.add_offset_value("full", 0.96)
        self.prop_size.add_suffix(self.flash_bar)
        grp_img.add(self.prop_size)

        self.prop_check = Adw.ActionRow(title="Integrity", subtitle="", subtitle_selectable=True, visible=False)
        self.prop_check.add_css_class("property")
        self.check_icon = Gtk.Image(icon_name="object-select-symbolic", valign=Gtk.Align.CENTER)
        self.prop_check.add_suffix(self.check_icon)
        grp_img.add(self.prop_check)

        # Inline flashing progress row (only visible while flashing or right after completion)
        self.prog_row = Adw.ActionRow(title="Flashing…", subtitle="", visible=False)
        self.inline_prog = Gtk.ProgressBar(valign=Gtk.Align.CENTER)
        self.inline_prog.set_size_request(150, -1)
        self.prog_row.add_suffix(self.inline_prog)
        grp_img.add(self.prog_row)

        self.flash_row = Adw.ButtonRow(title="Flash Firmware", start_icon_name="application-x-firmware-symbolic")
        self.flash_row.add_css_class("suggested-action")
        self.flash_row.connect("activated", lambda _r: self.confirm_flash())
        grp_img.add(self.flash_row)
        page.add(grp_img)

        # === LOCAL WORKSPACE BUILDS (auto-discovered from target/) ===
        self.grp_local = Adw.PreferencesGroup(title="Local Builds", visible=False)
        ref_builds = Gtk.Button(icon_name="view-refresh-symbolic", tooltip_text="Rescan target/")
        ref_builds.add_css_class("flat")
        ref_builds.connect("clicked", lambda _b: self._refresh_local_builds())
        self.grp_local.set_header_suffix(ref_builds)
        page.add(self.grp_local)

        # === REMOTE SOURCES & GITHUB RELEASES ===
        self.grp_remote = Adw.PreferencesGroup(
            title="Remote Sources &amp; GitHub Releases",
            description="Fetch .kfw / .bin releases from GitHub or download &amp; validate a direct image URL",
        )
        self.remote_src_row = Adw.EntryRow(title="GitHub Repo (owner/repo) or Firmware URL")
        self.remote_src_row.connect("entry-activated", lambda _w: self._fetch_remote())
        fetch_btn = Gtk.Button(label="Fetch", valign=Gtk.Align.CENTER)
        fetch_btn.connect("clicked", lambda _b: self._fetch_remote())
        self.remote_src_row.add_suffix(fetch_btn)
        self.grp_remote.add(self.remote_src_row)

        self.remote_sha_row = Adw.EntryRow(title="Expected SHA-256 (optional)")
        self.grp_remote.add(self.remote_sha_row)

        self.remote_prog_row = Adw.ActionRow(title="Downloading…", visible=False)
        self.remote_prog = Gtk.ProgressBar(valign=Gtk.Align.CENTER)
        self.remote_prog.set_size_request(140, -1)
        self.remote_prog_row.add_suffix(self.remote_prog)
        self.grp_remote.add(self.remote_prog_row)
        page.add(self.grp_remote)

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
            self.load_firmware_path(Path(value.get_path()))
            return True
        return False

    def _clear_firmware(self) -> None:
        self.selected_fw = None
        self.refresh()

    def _inspect_hex(self) -> None:
        if self.selected_fw:
            show_hex_viewer_dialog(self.win, self.selected_fw["fw_bytes"], Path(self.selected_fw["path"]).name)

    def _refresh_local_builds(self) -> None:
        for row in self._local_rows:
            self.grp_local.remove(row)
        self._local_rows.clear()

        builds = discover_local_firmware(REPO_ROOT)
        self.grp_local.set_visible(bool(builds))
        for p in builds:
            try:
                sz = fmt_size(p.stat().st_size)
            except OSError:
                continue
            row = Adw.ActionRow(
                title=GLib.markup_escape_text(p.name),
                subtitle=GLib.markup_escape_text(f"{p.parent.name}/{p.name} · {sz}"),
                activatable=True,
            )
            row.add_suffix(Gtk.Image(icon_name="go-next-symbolic"))
            row.connect("activated", lambda _r, fp=p: self.load_firmware_path(fp, auto_prompt=False))
            self.grp_local.add(row)
            self._local_rows.append(row)

    def _clear_remote_rows(self) -> None:
        for r in self._remote_rows:
            self.grp_remote.remove(r)
        self._remote_rows.clear()

    def _fetch_remote(self) -> None:
        src = self.remote_src_row.get_text().strip()
        if not src or self._remote_busy:
            return
        slug = parse_github_repo_slug(src)
        if slug:
            self._remote_busy = True
            self._clear_remote_rows()
            self.remote_prog_row.set_visible(True)
            self.remote_prog_row.set_title(GLib.markup_escape_text(f"Fetching releases for {slug}…"))
            self.remote_prog.pulse()

            def _w() -> None:
                try:
                    rels = fetch_github_releases(slug)
                except Exception as exc:
                    GLib.idle_add(self._on_toast, f"GitHub fetch failed: {exc}")
                    rels = []
                finally:
                    def _done() -> bool:
                        self._remote_busy = False
                        self.remote_prog_row.set_visible(False)
                        self._populate_releases(rels)
                        return False

                    GLib.idle_add(_done)

            threading.Thread(target=_w, daemon=True).start()
        elif src.startswith(("http://", "https://")):
            self._download_remote_asset(src, self.remote_sha_row.get_text().strip())
        else:
            self._on_toast("Enter 'owner/repo' or an http(s):// firmware URL")

    def _populate_releases(self, releases: list[dict[str, Any]]) -> None:
        self._clear_remote_rows()
        if not releases:
            row = Adw.ActionRow(title="No .kfw / .bin release assets found", subtitle="Check repository name or upload a .kfw/.bin asset")
            row.set_sensitive(False)
            self.grp_remote.add(row)
            self._remote_rows.append(row)
            return
        for rel in releases:
            for asset in rel["assets"]:
                tag = rel["tag"] or rel["name"]
                sub_parts = [tag]
                if rel.get("published_at"):
                    sub_parts.append(rel["published_at"])
                sub_parts.append(fmt_size(asset["size"]))
                if rel.get("prerelease"):
                    sub_parts.append("Pre-release")
                row = Adw.ActionRow(
                    title=GLib.markup_escape_text(asset["name"]),
                    subtitle=GLib.markup_escape_text(" · ".join(sub_parts)),
                )
                dl_b = Gtk.Button(label="Download", valign=Gtk.Align.CENTER)
                dl_b.connect("clicked", lambda _b, u=asset["url"], d=asset.get("digest", ""): self._download_remote_asset(u, d))
                row.add_suffix(dl_b)
                self.grp_remote.add(row)
                self._remote_rows.append(row)

    def _download_remote_asset(self, url: str, expected_sha: str = "") -> None:
        if self._remote_busy:
            return
        self._remote_busy = True
        self.remote_prog_row.set_visible(True)
        self.remote_prog_row.set_title("Downloading &amp; validating…")
        self.remote_prog.set_fraction(0.0)

        def _prog(f: float, msg: str) -> None:
            GLib.idle_add(lambda: (self.remote_prog.set_fraction(f), self.remote_prog_row.set_title(GLib.markup_escape_text(msg)), False)[2])

        def _w() -> None:
            try:
                dest, info = download_firmware_url(url, expected_sha256=expected_sha, progress_cb=_prog)
                def _ok() -> bool:
                    self._remote_busy = False
                    self.remote_prog_row.set_visible(False)
                    self.selected_fw = info
                    self._last_flash_summary = ""
                    self.prog_row.set_visible(False)
                    self.refresh()
                    self._on_toast(f"Validated {dest.name} ({fmt_size(info['size'])})")
                    return False

                GLib.idle_add(_ok)
            except Exception as exc:
                def _err() -> bool:
                    self._remote_busy = False
                    self.remote_prog_row.set_visible(False)
                    self._on_toast(f"Download/validation failed: {exc}")
                    return False

                GLib.idle_add(_err)

        threading.Thread(target=_w, daemon=True).start()

    def refresh(self) -> None:
        lk = self.runtime.link
        hard = self.runtime.security.hardened
        in_bl = lk.connected and lk.link_mode == "stock_bl"
        in_rw = lk.connected and lk.link_mode == "rigwire"
        can_enter = in_rw and not hard

        if in_bl:
            ver = lk.bootloader_info.get("version", "")
            uid = lk.bootloader_info.get("uid", "")
            self.bl_row.set_title("Bootloader Ready")
            sub = f"Version {ver}" if ver else "Ready to flash"
            if uid:
                sub += f" · UID {uid[:8]}"
            self.bl_row.set_subtitle(GLib.markup_escape_text(sub))
        elif in_rw:
            self.bl_row.set_title(GLib.markup_escape_text(f"{lk.firmware_name} Running"))
            self.bl_row.set_subtitle("Restart into bootloader to flash firmware")
        else:
            self.bl_row.set_title("No Radio Connected")
            self.bl_row.set_subtitle("Connect via USB, or hold PTT while powering on")

        self.bl_icon.set_from_icon_name("object-select-symbolic" if in_bl else "application-x-firmware-symbolic")
        self.bl_icon.set_css_classes(["success"] if in_bl else ["dim-label"])
        self.bl_btn.set_visible(not in_bl)
        self.bl_btn.set_sensitive(can_enter)
        self.bl_btn.set_tooltip_text(
            "Disabled in Safe Mode" if hard else ("Connect a RigWire radio first" if not in_rw else "Restart into Bootloader")
        )
        self.backup_btn.set_visible(bool(in_rw and self._on_open_tab))
        self.leave_demo_btn.set_visible(bool(in_bl and lk.demo_mode))

        fw = self.selected_fw
        has_fw = fw is not None
        if fw is not None:
            self.img_row.set_title(GLib.markup_escape_text(Path(fw["path"]).name))
            self.img_row.set_subtitle(GLib.markup_escape_text(str(Path(fw["path"]).parent)))

            targets = [t for t in fw.get("targets", []) if t and t != "raw"]
            fmt_parts = [str(fw.get("format", "Raw BIN"))]
            if fw.get("version"):
                fmt_parts.append(str(fw["version"]))
            if targets:
                fmt_parts.append(", ".join(targets))
            if fw.get("krx_abi"):
                fmt_parts.append(f"ABI {fw['krx_abi']}")
            if fw.get("sec_cnt"):
                fmt_parts.append(f"sec_cnt {fw['sec_cnt']}")
            if fw.get("modules_count"):
                fmt_parts.append(f"+{fw['modules_count']} module(s)")
            self.prop_format.set_subtitle(GLib.markup_escape_text(" · ".join(fmt_parts)))

            pct = float(fw.get("flash_pct", fw["size"] / float(FLASH_TOTAL_BYTES)))
            pages = int(fw.get("pages", max(1, math.ceil(fw["size"] / 256.0))))
            self.prop_size.set_subtitle(GLib.markup_escape_text(f"{fmt_size(fw['size'])} / 128 KB ({int(round(pct * 100))}%) · {pages} pages"))
            self.flash_bar.set_value(max(0.0, min(1.0, pct)))

            chk_parts: list[str] = []
            if fw.get("trusted"):
                chk_parts.append("Signed & Trusted")
            elif fw.get("signed"):
                chk_parts.append("Signed (unpinned key)")
            elif fw.get("ascon_ok"):
                chk_parts.append("Ascon-Hash256 verified (unsigned)")
            if fw.get("vector_ok"):
                chk_parts.append(f"ARMv6-M vectors OK (SP 0x{int(fw.get('initial_sp', 0)):08X})")
            else:
                chk_parts.append("Vectors unverified")
            if fw.get("sha256_short"):
                chk_parts.append(f"SHA-256 {fw['sha256_short']}")
            self.prop_check.set_subtitle(GLib.markup_escape_text(" · ".join(chk_parts)))
            good_chk = bool(fw.get("vector_ok") or fw.get("ascon_ok"))
            self.check_icon.set_from_icon_name("object-select-symbolic" if good_chk else "dialog-warning-symbolic")
            self.check_icon.set_css_classes(["success"] if good_chk else ["warning"])
        else:
            self.img_row.set_title("No Image Selected")
            self.img_row.set_subtitle("Drop or choose a .kfw or .bin file")

        self.hex_btn.set_visible(has_fw)
        self.clear_btn.set_visible(has_fw and not self.runtime.flashing_active)
        self.prop_format.set_visible(has_fw)
        self.prop_size.set_visible(has_fw)
        self.prop_check.set_visible(has_fw)
        self.img_icon.set_from_icon_name("object-select-symbolic" if has_fw else "package-x-generic-symbolic")
        self.img_icon.set_css_classes(["success"] if has_fw else ["dim-label"])

        self.flash_row.set_sensitive(bool(in_bl and has_fw and not self.runtime.flashing_active and not hard))
        self.flash_row.set_tooltip_text(
            "Disabled in Safe Mode"
            if hard
            else ("Enter Bootloader mode first" if not in_bl else ("Select a firmware image first" if not has_fw else None))
        )

        if self._last_flash_summary and not self.runtime.flashing_active:
            self.prog_row.set_visible(True)
            self.prog_row.set_title("Flash Complete")
            self.prog_row.set_subtitle(GLib.markup_escape_text(self._last_flash_summary))
            self.inline_prog.set_fraction(1.0)

    def load_firmware_path(self, fpath: Path, *, auto_prompt: bool = True) -> None:
        try:
            info = inspect_firmware_file(fpath)
        except Exception as exc:
            self._on_toast(f"Invalid firmware: {exc}")
            return
        info["path"] = str(fpath)
        self.selected_fw = info
        self._last_flash_summary = ""
        self.prog_row.set_visible(False)
        self.refresh()
        if auto_prompt and self.runtime.link.connected and self.runtime.link.link_mode == "stock_bl":
            self.confirm_flash()
        else:
            self._on_toast(f"Loaded {fpath.name} ({fmt_size(info['size'])})")

    def open_firmware_picker(self) -> None:
        dlg = Gtk.FileDialog(title="Select Firmware Image (.kfw or .bin)")

        def _picked(d: Any, res: Any) -> None:
            try:
                fpath = d.open_finish(res).get_path()
            except GLib.Error:
                return
            if fpath:
                self.load_firmware_path(Path(fpath))

        dlg.open(self.win, None, _picked)

    def confirm_flash(self) -> None:
        fw = self.selected_fw
        if fw is None:
            self.open_firmware_picker()
            return
        try:
            self.runtime.security.check_firmware_flash(len(fw["fw_bytes"]))
        except PermissionError as exc:
            self._on_toast(str(exc))
            return
        if not (self.runtime.link.connected and self.runtime.link.link_mode == "stock_bl"):
            self._on_toast("Enter the radio bootloader first")
            return
        self._on_confirm(
            f"Flash {fw['version']}?",
            f"{fw['summary']}\n\nKeep the radio connected and powered until flashing finishes.",
            "Flash",
            lambda: self._start_flash(fw),
        )

    def _update_inline_progress(self, frac: float, status: str, page: int, total_pages: int) -> None:
        elapsed = max(0.0, time.monotonic() - self._flash_t0)
        self.prog_row.set_visible(True)
        self.prog_row.set_title(GLib.markup_escape_text(status))
        self.prog_row.set_subtitle(f"Page {page}/{total_pages} · {elapsed:.1f}s")
        self.inline_prog.set_fraction(max(0.0, min(1.0, frac)))

    def _start_flash(self, info: dict[str, Any]) -> None:
        lk = self.runtime.link
        fw_bytes = info["fw_bytes"]
        total_pages = max(1, math.ceil(len(fw_bytes) / 256.0))
        is_demo = lk.demo_mode
        port = lk.port or ""

        self.runtime.flashing_active = True
        self._flash_t0 = time.monotonic()
        self._last_flash_summary = ""
        self.refresh()
        self._update_inline_progress(0.0, f"Flashing {info['version']}…", 0, total_pages)
        lk.torch_on = True
        self.runtime.lcd_state.step(bytes(1024), dataclasses.replace(self.runtime.lcd_st, ghosting=False, backlight=0))
        self._on_progress(True, 0.0, f"Flashing {info['version']} (0/{total_pages} pages)…")
        self._on_stage_redraw()

        cur_page = [0]

        def _page(p: int, tot: int) -> None:
            cur_page[0] = p

            def _pulse() -> bool:
                lk.torch_on = (p % 2 == 1)
                self._on_stage_redraw()
                self._update_inline_progress(0.10 + 0.88 * (p / float(max(1, tot))), f"Flashing {info['version']}…", p, tot)
                return False

            GLib.idle_add(_pulse)

        def _prog(f: float, msg: str) -> None:
            def _apply() -> bool:
                self._on_progress(True, f, msg)
                self._update_inline_progress(f, msg, cur_page[0], total_pages)
                return False

            GLib.idle_add(_apply)

        def _w() -> None:
            flashed_ok = False
            result_summary = ""
            try:
                if is_demo:
                    t0 = time.monotonic()
                    sim_pages = min(total_pages, 64)
                    for pg in range(1, sim_pages + 1):
                        _page(pg, sim_pages)
                        _prog(pg / float(sim_pages), f"Demo flash · Page {pg}/{sim_pages}")
                        time.sleep(0.025)
                    flashed_ok = True
                    result_summary = f"Simulated {sim_pages} pages ({info['version']}) in {time.monotonic() - t0:.1f}s"
                    GLib.idle_add(self._on_toast, result_summary)
                else:
                    res = flash_via_stock_bootloader(port, fw_bytes, progress_cb=_prog, page_cb=_page)
                    flashed_ok = True
                    result_summary = f"Flashed {res['pages']} pages ({info['version']}) in {res['elapsed_s']:.1f}s · reconnecting…"
                    GLib.idle_add(self._on_toast, result_summary)
                    self.runtime.notifier.send("Firmware Flash Complete", result_summary, notif_id="kern-dock-flash")
            except Exception as exc:
                GLib.idle_add(self._on_toast, f"Flash failed: {exc}")
            finally:
                def _finish() -> bool:
                    self.runtime.flashing_active = False
                    lk.torch_on = False
                    self._on_progress(False, 0.0, "")
                    if flashed_ok:
                        self._last_flash_summary = result_summary
                    else:
                        self.prog_row.set_visible(False)
                    if is_demo and lk.demo_mode:
                        self._on_leave_demo_bl()
                    elif flashed_ok and port and not is_demo:
                        lk.disconnect(reason="Firmware flashed · reconnecting")
                        GLib.timeout_add(
                            1800,
                            lambda p=port: (
                                self.runtime.connect_async(p)
                                if (not self.runtime.link.connected and not self.runtime._connecting)
                                else None,
                                False,
                            )[1],
                        )
                    self._on_stage_redraw()
                    self.refresh()
                    return False

                GLib.idle_add(_finish)

        threading.Thread(target=_w, daemon=True).start()
