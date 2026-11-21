"""Firmware Flasher View: pure, focused GNOME Firmware-style bootloader & .kfw/.bin flasher."""

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

from ...device.firmware import flash_via_stock_bootloader, inspect_firmware_file
from ...device.link import REBOOT_BOOTLOADER
from ..runtime import DockRuntime
from ..widgets.hex_view import show_hex_viewer_dialog


class FirmwareView:
    """Dedicated Firmware Flasher tab with pre-flight verification & colored hex inspection."""

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
    ) -> None:
        self.win = win
        self.runtime = runtime
        self._on_toast = on_toast
        self._on_confirm = on_confirm
        self._on_confirm_reboot = on_confirm_reboot
        self._on_progress = on_progress
        self._on_leave_demo_bl = on_leave_demo_bl
        self._on_stage_redraw = on_stage_redraw
        self.selected_fw: dict[str, Any] | None = None

        self.widget = self._build()

        runtime.events.subscribe("link.connected", lambda _e: self.refresh(), main_thread=True)
        runtime.events.subscribe("link.disconnected", lambda _e: self.refresh(), main_thread=True)
        runtime.events.subscribe("security.changed", lambda _e: self.refresh(), main_thread=True)
        self.refresh()

    def _build(self) -> Gtk.Box:
        root = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        page = Adw.PreferencesPage()

        grp_state = Adw.PreferencesGroup(title="Radio Bootloader")
        self.bl_row = Adw.ActionRow(title="Bootloader Status", subtitle="Connect a radio or hold PTT on power-on")
        self.bl_icon = Gtk.Image(icon_name="application-x-firmware-symbolic")
        self.bl_row.add_prefix(self.bl_icon)
        self.bl_btn = Gtk.Button(label="Enter Bootloader", valign=Gtk.Align.CENTER)
        self.bl_btn.add_css_class("pill")
        self.bl_btn.add_css_class("small-pill")
        self.bl_btn.connect("clicked", lambda _b: self._on_confirm_reboot(REBOOT_BOOTLOADER))
        self.bl_row.add_suffix(self.bl_btn)
        grp_state.add(self.bl_row)
        page.add(grp_state)

        grp_img = Adw.PreferencesGroup(title="Firmware Image")
        self.img_row = Adw.ActionRow(title="No Image Selected", subtitle="Drop or choose a .kfw or .bin file")
        self.img_icon = Gtk.Image(icon_name="package-x-generic-symbolic")
        self.img_row.add_prefix(self.img_icon)

        self.hex_btn = Gtk.Button(icon_name="accessories-text-editor-symbolic", valign=Gtk.Align.CENTER, tooltip_text="Inspect Firmware Hex")
        self.hex_btn.add_css_class("flat")
        self.hex_btn.set_visible(False)
        self.hex_btn.connect("clicked", lambda _b: self._inspect_hex())
        self.img_row.add_suffix(self.hex_btn)

        pick_btn = Gtk.Button(label="Choose…", valign=Gtk.Align.CENTER)
        pick_btn.add_css_class("pill")
        pick_btn.add_css_class("small-pill")
        pick_btn.connect("clicked", lambda _b: self.open_firmware_picker())
        self.img_row.add_suffix(pick_btn)
        grp_img.add(self.img_row)

        self.flash_row = Adw.ButtonRow(title="Flash Firmware", start_icon_name="application-x-firmware-symbolic")
        self.flash_row.add_css_class("suggested-action")
        self.flash_row.connect("activated", lambda _r: self.confirm_flash())
        grp_img.add(self.flash_row)
        page.add(grp_img)

        root.append(page)
        drop = Gtk.DropTarget.new(Gio.File, Gdk.DragAction.COPY)
        drop.connect("drop", self._on_drop)
        root.add_controller(drop)
        return root

    def _on_drop(self, _t: Any, value: Any, _x: float, _y: float) -> bool:
        if isinstance(value, Gio.File) and value.get_path():
            self.load_firmware_path(Path(value.get_path()))
            return True
        return False

    def _inspect_hex(self) -> None:
        if self.selected_fw:
            show_hex_viewer_dialog(self.win, self.selected_fw["fw_bytes"], Path(self.selected_fw["path"]).name)

    def refresh(self) -> None:
        lk = self.runtime.link
        hard = self.runtime.security.hardened
        in_bl = lk.connected and lk.link_mode == "stock_bl"
        can_enter = lk.connected and lk.link_mode == "kern_os" and not hard

        if in_bl:
            ver = lk.bootloader_info.get("version", "")
            self.bl_row.set_title("Bootloader Ready")
            self.bl_row.set_subtitle(f"Version {ver}".strip() if ver else "Ready to flash")
        elif lk.connected:
            self.bl_row.set_title("Kern OS Running")
            self.bl_row.set_subtitle("Restart into bootloader to flash firmware")
        else:
            self.bl_row.set_title("No Radio Connected")
            self.bl_row.set_subtitle("Connect a radio or hold PTT while powering on")

        self.bl_icon.set_from_icon_name("object-select-symbolic" if in_bl else "application-x-firmware-symbolic")
        self.bl_icon.set_css_classes(["success"] if in_bl else ["dim-label"])
        self.bl_btn.set_visible(not in_bl)
        self.bl_btn.set_sensitive(can_enter)

        fw = self.selected_fw
        if fw is not None:
            self.img_row.set_title(GLib.markup_escape_text(Path(fw["path"]).name))
            self.img_row.set_subtitle(GLib.markup_escape_text(fw["summary"]))
            self.hex_btn.set_visible(True)
        else:
            self.img_row.set_title("No Image Selected")
            self.img_row.set_subtitle("Choose a signed .kfw or raw .bin file")
            self.hex_btn.set_visible(False)
        self.img_icon.set_from_icon_name("object-select-symbolic" if fw else "package-x-generic-symbolic")
        self.img_icon.set_css_classes(["success"] if fw else ["dim-label"])
        self.flash_row.set_sensitive(bool(in_bl and fw and not self.runtime.flashing_active and not hard))

    def load_firmware_path(self, fpath: Path) -> None:
        try:
            info = inspect_firmware_file(fpath)
        except Exception as exc:
            self._on_toast(f"Invalid firmware: {exc}")
            return
        info["path"] = str(fpath)
        self.selected_fw = info
        self.refresh()
        if self.runtime.link.connected and self.runtime.link.link_mode == "stock_bl":
            self.confirm_flash()
        else:
            self._on_toast(f"Loaded {fpath.name} ({info['size']:,} B)")

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

    def _start_flash(self, info: dict[str, Any]) -> None:
        lk = self.runtime.link
        fw_bytes = info["fw_bytes"]
        total_pages = max(1, math.ceil(len(fw_bytes) / 256.0))
        is_demo = lk.demo_mode
        port = lk.port or ""

        self.runtime.flashing_active = True
        self.refresh()
        lk.torch_on = True
        self.runtime.lcd_state.step(bytes(1024), dataclasses.replace(self.runtime.lcd_st, ghosting=False, backlight=0))
        self._on_progress(True, 0.0, f"Flashing {info['version']} (0/{total_pages} pages)…")
        self._on_stage_redraw()

        def _page(p: int, _tot: int) -> None:
            def _pulse() -> bool:
                lk.torch_on = (p % 2 == 1)
                self._on_stage_redraw()
                return False

            GLib.idle_add(_pulse)

        def _prog(f: float, msg: str) -> None:
            GLib.idle_add(self._on_progress, True, f, msg)

        def _w() -> None:
            flashed_ok = False
            try:
                if is_demo:
                    t0 = time.monotonic()
                    sim_pages = min(total_pages, 64)
                    for pg in range(1, sim_pages + 1):
                        _page(pg, sim_pages)
                        _prog(pg / float(sim_pages), f"Demo flash · Page {pg}/{sim_pages}")
                        time.sleep(0.03)
                    flashed_ok = True
                    GLib.idle_add(self._on_toast, f"Simulated {sim_pages} pages in {time.monotonic() - t0:.1f}s")
                else:
                    res = flash_via_stock_bootloader(port, fw_bytes, progress_cb=_prog, page_cb=_page)
                    flashed_ok = True
                    msg = f"Flashed {res['pages']} pages in {res['elapsed_s']:.1f}s · reconnecting…"
                    GLib.idle_add(self._on_toast, msg)
                    self.runtime.notifier.send("Firmware Flash Complete", msg, notif_id="kern-dock-flash")
            except Exception as exc:
                GLib.idle_add(self._on_toast, f"Flash failed: {exc}")
            finally:
                def _finish() -> bool:
                    self.runtime.flashing_active = False
                    lk.torch_on = False
                    self._on_progress(False, 0.0, "")
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
