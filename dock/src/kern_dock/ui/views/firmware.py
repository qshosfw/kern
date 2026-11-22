"""Firmware Flasher View: flagship GNOME Firmware-style bootloader & .kfw/.bin flasher."""

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
    flash_via_stock_bootloader,
    inspect_firmware_file,
)
from ...device.link import REBOOT_BOOTLOADER
from ..runtime import DockRuntime
from ..widgets.hex_view import show_hex_viewer_dialog
from ..widgets.util import fmt_size


class FirmwareView:
    """Dedicated Firmware Flasher tab with pre-flight validation, local build picks & live progress."""

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

        # === STEP 1: RADIO & BOOTLOADER ===
        grp_state = Adw.PreferencesGroup(title="1 · Radio Bootloader")
        self.bl_row = Adw.ActionRow(title="Bootloader Status", subtitle="Connect a radio or hold PTT on power-on")
        self.bl_icon = Gtk.Image(icon_name="application-x-firmware-symbolic")
        self.bl_row.add_prefix(self.bl_icon)

        self.bl_pill = Gtk.Label(label="Offline", valign=Gtk.Align.CENTER)
        self.bl_pill.add_css_class("status-pill")
        self.bl_row.add_suffix(self.bl_pill)

        self.backup_btn = Gtk.Button(label="Backup First", valign=Gtk.Align.CENTER, tooltip_text="Open Backups before flashing")
        self.backup_btn.add_css_class("flat")
        self.backup_btn.set_visible(False)
        self.backup_btn.connect("clicked", lambda _b: self._on_open_tab("backups") if self._on_open_tab else None)
        self.bl_row.add_suffix(self.backup_btn)

        self.bl_btn = Gtk.Button(label="Enter Bootloader", valign=Gtk.Align.CENTER)
        self.bl_btn.add_css_class("pill")
        self.bl_btn.add_css_class("small-pill")
        self.bl_btn.connect("clicked", lambda _b: self._on_confirm_reboot(REBOOT_BOOTLOADER))
        self.bl_row.add_suffix(self.bl_btn)

        self.leave_demo_btn = Gtk.Button(label="Exit Demo BL", valign=Gtk.Align.CENTER)
        self.leave_demo_btn.add_css_class("pill")
        self.leave_demo_btn.add_css_class("small-pill")
        self.leave_demo_btn.set_visible(False)
        self.leave_demo_btn.connect("clicked", lambda _b: self._on_leave_demo_bl())
        self.bl_row.add_suffix(self.leave_demo_btn)

        grp_state.add(self.bl_row)
        page.add(grp_state)

        # === STEP 2: FIRMWARE IMAGE & PRE-FLIGHT ===
        grp_img = Adw.PreferencesGroup(title="2 · Firmware Image")

        self.img_row = Adw.ActionRow(title="No Image Selected", subtitle="Drop or choose a signed .kfw or raw .bin file")
        self.img_icon = Gtk.Image(icon_name="package-x-generic-symbolic")
        self.img_row.add_prefix(self.img_icon)

        self.hex_btn = Gtk.Button(
            icon_name="accessories-text-editor-symbolic",
            valign=Gtk.Align.CENTER,
            tooltip_text="Inspect Firmware Hex",
        )
        self.hex_btn.add_css_class("flat")
        self.hex_btn.set_visible(False)
        self.hex_btn.connect("clicked", lambda _b: self._inspect_hex())
        self.img_row.add_suffix(self.hex_btn)

        self.clear_btn = Gtk.Button(icon_name="edit-clear-symbolic", valign=Gtk.Align.CENTER, tooltip_text="Clear Image")
        self.clear_btn.add_css_class("flat")
        self.clear_btn.set_visible(False)
        self.clear_btn.connect("clicked", lambda _b: self._clear_firmware())
        self.img_row.add_suffix(self.clear_btn)

        pick_btn = Gtk.Button(label="Choose…", valign=Gtk.Align.CENTER)
        pick_btn.add_css_class("pill")
        pick_btn.add_css_class("small-pill")
        pick_btn.connect("clicked", lambda _b: self.open_firmware_picker())
        self.img_row.add_suffix(pick_btn)
        grp_img.add(self.img_row)

        # Local workspace quick-picks (target/firmware_packed.bin, target/*.kfw)
        self.local_builds_box = Gtk.Box(
            orientation=Gtk.Orientation.HORIZONTAL,
            spacing=6,
            margin_top=6,
            margin_bottom=4,
            visible=False,
        )
        local_lbl = Gtk.Label(label="Local builds:", valign=Gtk.Align.CENTER)
        local_lbl.add_css_class("caption")
        local_lbl.add_css_class("dim-label")
        self.local_builds_box.append(local_lbl)
        self.local_chips = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=6, hexpand=True)
        self.local_builds_box.append(self.local_chips)
        ref_builds = Gtk.Button(icon_name="view-refresh-symbolic", tooltip_text="Rescan target/ builds")
        ref_builds.add_css_class("flat")
        ref_builds.connect("clicked", lambda _b: self._refresh_local_builds())
        self.local_builds_box.append(ref_builds)
        grp_img.add(self.local_builds_box)

        # Rich Pre-Flight Card (revealed when an image is loaded)
        self.preflight_card = Gtk.Box(
            orientation=Gtk.Orientation.VERTICAL,
            spacing=10,
            margin_top=8,
            visible=False,
        )
        self.preflight_card.add_css_class("card")
        self.preflight_card.add_css_class("preflight-card")

        # Badges flowbox (Format, Version, Targets, Vectors, SHA-256)
        self.preflight_chips = Gtk.FlowBox(
            selection_mode=Gtk.SelectionMode.NONE,
            row_spacing=6,
            column_spacing=6,
            max_children_per_line=8,
            homogeneous=False,
        )
        self.preflight_card.append(self.preflight_chips)

        # Flash utilization bar
        bar_hdr = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        bar_t = Gtk.Label(label="Flash Footprint", xalign=0, hexpand=True)
        bar_t.add_css_class("caption-heading")
        bar_t.add_css_class("dim-label")
        self.flash_cap_lbl = Gtk.Label(label="0 KB / 128 KB", xalign=1)
        self.flash_cap_lbl.add_css_class("caption")
        self.flash_cap_lbl.add_css_class("monospace")
        bar_hdr.append(bar_t)
        bar_hdr.append(self.flash_cap_lbl)
        self.preflight_card.append(bar_hdr)

        self.flash_bar = Gtk.LevelBar(min_value=0.0, max_value=1.0, value=0.0)
        self.flash_bar.add_css_class("stat-bar")
        self.flash_bar.add_offset_value("low", 0.65)
        self.flash_bar.add_offset_value("high", 0.85)
        self.flash_bar.add_offset_value("full", 0.96)
        self.preflight_card.append(self.flash_bar)

        grp_img.add(self.preflight_card)
        page.add(grp_img)

        # === STEP 3: FLASH & LIVE PROGRESS ===
        grp_flash = Adw.PreferencesGroup(title="3 · Write to Radio")
        self.flash_row = Adw.ButtonRow(title="Flash Firmware", start_icon_name="application-x-firmware-symbolic")
        self.flash_row.add_css_class("suggested-action")
        self.flash_row.connect("activated", lambda _r: self.confirm_flash())
        grp_flash.add(self.flash_row)

        self.flash_hint = Gtk.Label(label="", xalign=0, wrap=True, margin_top=6)
        self.flash_hint.add_css_class("caption")
        self.flash_hint.add_css_class("dim-label")
        grp_flash.add(self.flash_hint)

        # Inline Live Progress Card
        self.prog_card = Gtk.Box(
            orientation=Gtk.Orientation.VERTICAL,
            spacing=8,
            margin_top=8,
            visible=False,
        )
        self.prog_card.add_css_class("card")
        self.prog_card.add_css_class("preflight-card")

        prog_top = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        self.prog_title = Gtk.Label(label="Flashing…", xalign=0, hexpand=True)
        self.prog_title.add_css_class("heading")
        self.prog_meta = Gtk.Label(label="0 / 0 pages · 0.0s", xalign=1)
        self.prog_meta.add_css_class("caption")
        self.prog_meta.add_css_class("monospace")
        self.prog_meta.add_css_class("dim-label")
        prog_top.append(self.prog_title)
        prog_top.append(self.prog_meta)
        self.prog_card.append(prog_top)

        self.inline_prog = Gtk.ProgressBar(show_text=False)
        self.prog_card.append(self.inline_prog)

        self.prog_sub = Gtk.Label(label="Keep the radio connected and powered until flashing finishes.", xalign=0, wrap=True)
        self.prog_sub.add_css_class("caption")
        self.prog_sub.add_css_class("dim-label")
        self.prog_card.append(self.prog_sub)

        grp_flash.add(self.prog_card)
        page.add(grp_flash)

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
        c = self.local_chips.get_first_child()
        while c is not None:
            n = c.get_next_sibling()
            self.local_chips.remove(c)
            c = n
        builds = discover_local_firmware(REPO_ROOT)
        self.local_builds_box.set_visible(bool(builds))
        for p in builds:
            try:
                sz = fmt_size(p.stat().st_size)
            except OSError:
                continue
            btn = Gtk.Button(label=f"{p.name} · {sz}", tooltip_text=str(p))
            btn.add_css_class("quick-chip")
            btn.connect("clicked", lambda _b, fp=p: self.load_firmware_path(fp, auto_prompt=False))
            self.local_chips.append(btn)

    def _rebuild_preflight_chips(self, fw: dict[str, Any]) -> None:
        c = self.preflight_chips.get_first_child()
        while c is not None:
            n = c.get_next_sibling()
            self.preflight_chips.remove(c)
            c = n

        def _add_chip(text: str, *classes: str, tip: str = "") -> None:
            lbl = Gtk.Label(label=text, tooltip_text=tip)
            for cls in classes:
                lbl.add_css_class(cls)
            self.preflight_chips.append(lbl)

        if fw.get("ascon_ok"):
            _add_chip(f"{fw['format']} · Ascon OK", "status-pill", "live", tip="Ascon-Hash256 container root verified")
        else:
            _add_chip(str(fw.get("format", "Raw BIN")), "status-pill", tip="Unwrapped raw binary image")

        if fw.get("version"):
            _add_chip(str(fw["version"]), "cap-chip", tip="Firmware version / build identifier")

        targets = [t for t in fw.get("targets", []) if t and t != "raw"]
        if targets:
            _add_chip(", ".join(targets), "cap-chip", tip="Declared hardware targets")

        if fw.get("vector_ok"):
            sp = int(fw.get("initial_sp", 0))
            _add_chip("ARMv6-M Vectors OK", "status-pill", "live", tip=f"Initial SP 0x{sp:08X}, Thumb Reset vector valid")
        else:
            _add_chip("Vectors Unverified", "status-pill", "dfu", tip="Non-standard or packed vector table")

        if fw.get("sha256_short"):
            _add_chip(f"sha256:{fw['sha256_short']}", "cap-chip", tip="Short SHA-256 digest of firmware payload")

    def refresh(self) -> None:
        lk = self.runtime.link
        hard = self.runtime.security.hardened
        in_bl = lk.connected and lk.link_mode == "stock_bl"
        in_rw = lk.connected and lk.link_mode == "rigwire"
        can_enter = in_rw and not hard

        for cls in ("live", "dfu", "headless", "warn"):
            self.bl_pill.remove_css_class(cls)

        if in_bl:
            ver = lk.bootloader_info.get("version", "")
            uid = lk.bootloader_info.get("uid", "")
            self.bl_row.set_title("Bootloader Ready")
            sub = f"Version {ver}" if ver else "Ready to flash"
            if uid:
                sub += f" · UID {uid[:8]}"
            self.bl_row.set_subtitle(GLib.markup_escape_text(sub))
            self.bl_pill.set_text("DFU Ready")
            self.bl_pill.add_css_class("live")
        elif in_rw:
            self.bl_row.set_title(GLib.markup_escape_text(f"{lk.firmware_name} Running"))
            self.bl_row.set_subtitle("Restart into bootloader to flash firmware")
            self.bl_pill.set_text("Online")
            self.bl_pill.add_css_class("headless")
        else:
            self.bl_row.set_title("No Radio Connected")
            self.bl_row.set_subtitle("Connect via USB, or hold PTT while powering on for bootloader")
            self.bl_pill.set_text("Offline")

        self.bl_icon.set_from_icon_name("object-select-symbolic" if in_bl else "application-x-firmware-symbolic")
        self.bl_icon.set_css_classes(["success"] if in_bl else ["dim-label"])
        self.bl_btn.set_visible(not in_bl)
        self.bl_btn.set_sensitive(can_enter)
        self.backup_btn.set_visible(bool(in_rw and self._on_open_tab))
        self.leave_demo_btn.set_visible(bool(in_bl and lk.demo_mode))

        fw = self.selected_fw
        if fw is not None:
            self.img_row.set_title(GLib.markup_escape_text(Path(fw["path"]).name))
            self.img_row.set_subtitle(GLib.markup_escape_text(fw["summary"]))
            self.hex_btn.set_visible(True)
            self.clear_btn.set_visible(not self.runtime.flashing_active)
            self.preflight_card.set_visible(True)
            self._rebuild_preflight_chips(fw)
            pct = float(fw.get("flash_pct", fw["size"] / float(FLASH_TOTAL_BYTES)))
            pages = int(fw.get("pages", max(1, math.ceil(fw["size"] / 256.0))))
            self.flash_bar.set_value(max(0.0, min(1.0, pct)))
            self.flash_cap_lbl.set_text(f"{fmt_size(fw['size'])} / 128 KB · {pages} pages ({int(round(pct * 100))}%)")
        else:
            self.img_row.set_title("No Image Selected")
            self.img_row.set_subtitle("Drop or choose a signed .kfw or raw .bin file")
            self.hex_btn.set_visible(False)
            self.clear_btn.set_visible(False)
            self.preflight_card.set_visible(False)

        self.img_icon.set_from_icon_name("object-select-symbolic" if fw else "package-x-generic-symbolic")
        self.img_icon.set_css_classes(["success"] if fw else ["dim-label"])

        ready = bool(in_bl and fw and not self.runtime.flashing_active and not hard)
        self.flash_row.set_sensitive(ready)

        if self.runtime.flashing_active:
            self.flash_hint.set_text("Flashing in progress — do not disconnect the radio.")
        elif hard:
            self.flash_hint.set_text("Hardened Mode is active — firmware flashing is blocked.")
        elif fw is None:
            self.flash_hint.set_text("Choose a .kfw or .bin image above (or click a local build chip).")
        elif not in_bl:
            self.flash_hint.set_text("Enter bootloader mode above before flashing.")
        else:
            pages = int(fw.get("pages", max(1, math.ceil(fw["size"] / 256.0))))
            self.flash_hint.set_text(f"Ready to write {pages} pages ({fmt_size(fw['size'])}) via stock bootloader.")

        if self._last_flash_summary and not self.runtime.flashing_active:
            self.prog_card.set_visible(True)
            self.prog_title.set_text("Last Flash Complete")
            self.prog_meta.set_text("")
            self.inline_prog.set_fraction(1.0)
            self.prog_sub.set_text(self._last_flash_summary)

    def load_firmware_path(self, fpath: Path, *, auto_prompt: bool = True) -> None:
        try:
            info = inspect_firmware_file(fpath)
        except Exception as exc:
            self._on_toast(f"Invalid firmware: {exc}")
            return
        info["path"] = str(fpath)
        self.selected_fw = info
        self._last_flash_summary = ""
        self.prog_card.set_visible(False)
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
        self.prog_card.set_visible(True)
        self.prog_title.set_text(status)
        self.prog_meta.set_text(f"Page {page}/{total_pages} · {elapsed:.1f}s")
        self.inline_prog.set_fraction(max(0.0, min(1.0, frac)))
        self.prog_sub.set_text("Keep the radio connected and powered until flashing finishes.")

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
                        self.prog_card.set_visible(False)
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
