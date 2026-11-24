"""Home View (pinned tab): connection screen, tool tab launcher, device health, identity & security."""

from __future__ import annotations

import time
from typing import Any, Callable

import gi

gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")
from gi.repository import Adw, GLib, Gtk  # noqa: E402

from ...device.hardware import discover_swd_probes
from ...device.link import REBOOT_BOOTLOADER, REBOOT_FIRMWARE, REBOOT_RECOVERY
from ...render.handset.catalog import radio_model
from ...system.icons import make_radio_image
from ..runtime import DockRuntime
from ..widgets.util import battery_icon_name, fmt_duration, set_pill


class OverviewView:
    """Pinned Home tab: device status, tool tab launcher, identity, security & power."""

    def __init__(
        self,
        win: Adw.ApplicationWindow,
        runtime: DockRuntime,
        *,
        on_toast: Callable[[str], None],
        on_open_tab: Callable[[str], None],
        on_confirm_reboot: Callable[[int], None],
        on_leave_demo_bl: Callable[[], None],
    ) -> None:
        self.win = win
        self.runtime = runtime
        self._on_toast = on_toast
        self._on_open_tab = on_open_tab
        self._on_confirm_reboot = on_confirm_reboot
        self._on_leave_demo_bl = on_leave_demo_bl
        self._updating = False

        self.widget = self._build()

        runtime.events.subscribe("ports.changed", lambda _e: self.rebuild_port_list(), main_thread=True)
        runtime.events.subscribe("link.connected", lambda _e: self.refresh(), main_thread=True)
        runtime.events.subscribe("link.disconnected", lambda _e: self.refresh(), main_thread=True)
        runtime.events.subscribe("link.telemetry", lambda _e: self.refresh_telemetry(), main_thread=True)
        runtime.events.subscribe("security.changed", lambda _e: self.refresh_security(), main_thread=True)

        self.rebuild_port_list()
        self.rebuild_tools()
        self.refresh()

    def _build(self) -> Gtk.Stack:
        self.stack = Gtk.Stack()

        # --- Offline Connect Screen ---
        offline = Adw.StatusPage(
            icon_name="org.kernos.Dock",
            title="Connect a Radio",
            description="Plug in via USB-C, start the hardware emulator, or open the offline demo.",
        )
        offline.add_css_class("compact")
        off_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=14)
        self.usb_list = Gtk.ListBox(selection_mode=Gtk.SelectionMode.NONE)
        self.usb_list.add_css_class("boxed-list")
        off_box.append(self.usb_list)

        btns = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=10, halign=Gtk.Align.CENTER)
        demo_b = Gtk.Button(label="Demo Radio")
        demo_b.add_css_class("pill")
        demo_b.add_css_class("suggested-action")
        demo_b.connect("clicked", lambda _b: self.runtime.connect_async("demo"))
        qemu_b = Gtk.Button(label="Hardware Emulator")
        qemu_b.add_css_class("pill")
        qemu_b.set_tooltip_text("Attach to or auto-start the in-house hardware emulator (tcp://127.0.0.1:4446)")
        qemu_b.connect(
            "clicked",
            lambda _b: self.runtime.connect_or_start_emulator_async(
                on_status=lambda m: GLib.idle_add(self._on_toast, m),
            ),
        )
        hw_b = Gtk.Button(label="Hardware & SWD")
        hw_b.add_css_class("pill")
        hw_b.set_tooltip_text("SWD probe, USB power & raw serial control (works offline)")
        hw_b.connect("clicked", lambda _b: self._on_open_tab("hardware"))
        btns.append(demo_b)
        btns.append(qemu_b)
        btns.append(hw_b)
        off_box.append(btns)
        offline.set_child(Adw.Clamp(maximum_size=420, child=off_box))
        self.stack.add_named(offline, "offline")

        # --- Online Home Dashboard ---
        page = Adw.PreferencesPage()

        # Compact Device Hero + 4-Tile Metrics
        hero = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=10)
        hero.add_css_class("card")
        hero.add_css_class("device-hero")

        head = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=10)
        head.append(make_radio_image(32, color=True))
        names = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=1, hexpand=True, valign=Gtk.Align.CENTER)
        self.dev_title = Gtk.Label(label="Radio", xalign=0, ellipsize=3)
        self.dev_title.add_css_class("heading")
        self.dev_sub = Gtk.Label(label="", xalign=0, ellipsize=3)
        self.dev_sub.add_css_class("dim-label")
        self.dev_sub.add_css_class("caption")
        names.append(self.dev_title)
        names.append(self.dev_sub)
        head.append(names)

        self.state_pill = Gtk.Label(label="RigWire", valign=Gtk.Align.CENTER)
        self.state_pill.add_css_class("status-pill")
        head.append(self.state_pill)

        disc = Gtk.Button(icon_name="media-eject-symbolic", valign=Gtk.Align.CENTER, tooltip_text="Disconnect")
        disc.add_css_class("flat")
        disc.add_css_class("circular")
        disc.connect("clicked", lambda _b: self.runtime.link.disconnect())
        head.append(disc)
        hero.append(head)

        tiles = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8, homogeneous=True)
        self._tiles: dict[str, tuple[Any, Any]] = {}
        for key, title, icon_name in (
            ("battery", "Battery", "battery-good-symbolic"),
            ("link", "Link", "network-transmit-receive-symbolic"),
            ("uptime", "Uptime", "preferences-system-time-symbolic"),
        ):
            tile = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=2)
            tile.add_css_class("stat-tile")
            cap = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=5)
            ti = Gtk.Image(icon_name=icon_name)
            ti.add_css_class("dim-label")
            cap.append(ti)
            tl = Gtk.Label(label=title, xalign=0)
            tl.add_css_class("caption")
            tl.add_css_class("dim-label")
            cap.append(tl)
            tile.append(cap)
            val = Gtk.Label(label="—", xalign=0, ellipsize=3)
            val.add_css_class("stat-value")
            sub = Gtk.Label(label="", xalign=0, ellipsize=3)
            sub.add_css_class("caption")
            sub.add_css_class("dim-label")
            tile.append(val)
            tile.append(sub)
            if key == "battery":
                self.batt_icon = ti
                self.batt_bar = Gtk.LevelBar(min_value=0.0, max_value=100.0, margin_top=3)
                self.batt_bar.add_css_class("stat-bar")
                self.batt_bar.add_offset_value("low", 20.0)
                self.batt_bar.add_offset_value("high", 50.0)
                self.batt_bar.add_offset_value("full", 85.0)
                tile.append(self.batt_bar)
            tiles.append(tile)
            self._tiles[key] = (val, sub)
        self.dev_tiles = tiles
        hero.append(tiles)
        hero_grp = Adw.PreferencesGroup()
        hero_grp.add(hero)
        page.add(hero_grp)

        # Bootloader contextual group
        self.bl_group = Adw.PreferencesGroup(title="Bootloader Mode", visible=False)
        self.bl_ver_row = Adw.ActionRow(title="Bootloader Version", subtitle="—")
        self.bl_ver_row.add_css_class("property")
        self.bl_group.add(self.bl_ver_row)
        bl_flash = Adw.ButtonRow(title="Open Firmware Tab", start_icon_name="application-x-firmware-symbolic")
        bl_flash.add_css_class("suggested-action")
        bl_flash.connect("activated", lambda _r: self._on_open_tab("firmware"))
        self.bl_group.add(bl_flash)
        self.bl_leave_row = Adw.ButtonRow(title="Power Cycle into Firmware (Demo)", start_icon_name="system-reboot-symbolic")
        self.bl_leave_row.connect("activated", lambda _r: self._on_leave_demo_bl())
        self.bl_group.add(self.bl_leave_row)
        self.bl_hint_row = Adw.ActionRow(title="Exit Bootloader", subtitle="Power the radio off and on to boot back into firmware.")
        self.bl_group.add(self.bl_hint_row)
        page.add(self.bl_group)

        # Native Libadwaita Tools list (clean ActionRows with icon, title, subtitle & chevron)
        self.tools_grp = Adw.PreferencesGroup(title="Tools")
        self._tool_rows: list[Gtk.Widget] = []
        page.add(self.tools_grp)

        # Collapsible Device Identity (reduces visual clutter on Home)
        self.info_group = Adw.PreferencesGroup()
        self.info_exp = Adw.ExpanderRow(title="Device Identity", subtitle="Serial, MAC &amp; calibration")
        self._info_rows: dict[str, Adw.ActionRow] = {}
        for key, title in (
            ("serial", "Serial"),
            ("mac", "MAC"),
            ("calib", "Calibration"),
            ("proto", "Protocol &amp; Clock"),
        ):
            r = Adw.ActionRow(title=title, subtitle="—", subtitle_selectable=True)
            r.add_css_class("property")
            self.info_exp.add_row(r)
            self._info_rows[key] = r
        self.calib_icon = Gtk.Image(icon_name="object-select-symbolic", valign=Gtk.Align.CENTER, visible=False)
        self._info_rows["calib"].add_suffix(self.calib_icon)
        self.info_group.add(self.info_exp)
        page.add(self.info_group)

        # Safety & Power
        self.sec_group = Adw.PreferencesGroup(title="Safety &amp; Power")
        self.sw_tx = Adw.SwitchRow(title="Allow USB TX", subtitle="Unlock PTT over USB")
        self.sw_tx.connect("notify::active", lambda w, _p: None if self._updating else self._toggle_tx(w.get_active()))
        self.sec_group.add(self.sw_tx)

        self.policy_exp = Adw.ExpanderRow(title="Storage Policy", subtitle="Read &amp; write")
        self._policy_rows: dict[str, Adw.SwitchRow] = {}
        for attr, title, sub in (
            ("read_only", "Read-Only", "Block writes and deletes"),
            ("validate_formats", "Validate Formats", "Verify tables and app binaries"),
            ("allow_sys_write", "Allow /sys Writes", "Unlock factory calibration"),
            ("allow_system_mod_delete", "Allow Recovery Removal", "Unlock system modules"),
        ):
            r = Adw.SwitchRow(title=title, subtitle=sub, active=getattr(self.runtime.security.policy, attr))
            if attr.startswith("allow_"):
                r.add_css_class("warning-row")
            r.connect("notify::active", lambda w, _p, a=attr: None if self._updating else self._set_policy(a, w.get_active()))
            self.policy_exp.add_row(r)
            self._policy_rows[attr] = r
        self.sec_group.add(self.policy_exp)

        self.power_row = Adw.ActionRow(title="Power")
        seg = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, valign=Gtk.Align.CENTER)
        seg.add_css_class("linked")
        self._reboot_btns: dict[int, Gtk.Button] = {}
        for mode, label in (
            (REBOOT_FIRMWARE, "Restart"),
            (REBOOT_RECOVERY, "Recovery"),
            (REBOOT_BOOTLOADER, "Bootloader"),
        ):
            b = Gtk.Button(label=label)
            b.connect("clicked", lambda _b, m=mode: self._on_confirm_reboot(m))
            seg.append(b)
            self._reboot_btns[mode] = b
        self.power_row.add_suffix(seg)
        self.sec_group.add(self.power_row)
        page.add(self.sec_group)

        self.stack.add_named(page, "online")
        return self.stack

    def rebuild_tools(self) -> None:
        for row in self._tool_rows:
            self.tools_grp.remove(row)
        self._tool_rows.clear()
        for spec in self.runtime.pages.ordered():
            if spec.id == "overview":
                continue
            row = Adw.ActionRow(
                title=GLib.markup_escape_text(spec.title),
                subtitle=GLib.markup_escape_text(spec.subtitle or ""),
                activatable=True,
            )
            row.add_prefix(Gtk.Image.new_from_icon_name(spec.icon))
            row.add_suffix(Gtk.Image.new_from_icon_name("go-next-symbolic"))
            row.connect("activated", lambda _r, pid=spec.id: self._on_open_tab(pid))
            self.tools_grp.add(row)
            self._tool_rows.append(row)

    def _toggle_hardened(self, on: bool) -> None:
        self.runtime.set_hardened(on)
        self._on_toast("Hardened Mode ON" if on else "Hardened Mode OFF")

    def _toggle_tx(self, on: bool) -> None:
        try:
            self.runtime.security.set_tx_armed(on)
        except PermissionError as exc:
            self._on_toast(str(exc))
            self.refresh_security()

    def _set_policy(self, attr: str, val: bool) -> None:
        try:
            self.runtime.security.set_policy_flag(attr, val)
        except PermissionError as exc:
            self._on_toast(str(exc))
            self.refresh_security()

    def rebuild_port_list(self) -> None:
        c = self.usb_list.get_first_child()
        while c is not None:
            n = c.get_next_sibling()
            self.usb_list.remove(c)
            c = n
        ports = self.runtime.port_monitor.ports
        probes = discover_swd_probes()
        if not ports and not probes:
            empty = Adw.ActionRow(title="No USB radio or SWD probe detected", subtitle="Plug in via USB-C, programming cable, or SWD probe")
            empty.add_prefix(Gtk.Image(icon_name="network-wired-symbolic"))
            empty.set_sensitive(False)
            self.usb_list.append(empty)
            return
        for p in ports:
            row = Adw.ActionRow(
                title=GLib.markup_escape_text(p.pretty_name),
                subtitle=GLib.markup_escape_text(f"{p.device} · {'Bootloader' if p.mode_hint == 'stock_bl' else 'USB Serial'}"),
                activatable=True,
            )
            row.add_prefix(make_radio_image(18) if p.is_quansheng else Gtk.Image(icon_name="network-wired-symbolic"))
            row.add_suffix(Gtk.Image(icon_name="go-next-symbolic"))
            row.connect("activated", lambda _r, po=p: self.runtime.connect_async(po.device, po))
            self.usb_list.append(row)
        for pr in probes:
            row = Adw.ActionRow(
                title=GLib.markup_escape_text(pr.name),
                subtitle=GLib.markup_escape_text(f"SWD Probe ({pr.driver}) · {pr.vid}:{pr.pid}"),
                activatable=True,
            )
            row.add_prefix(Gtk.Image(icon_name="preferences-other-symbolic"))
            row.add_suffix(Gtk.Image(icon_name="go-next-symbolic"))
            row.connect("activated", lambda _r: self._on_open_tab("hardware"))
            self.usb_list.append(row)

    def refresh(self) -> None:
        lk = self.runtime.link
        if not lk.connected:
            self.stack.set_visible_child_name("offline")
            self.refresh_security()
            return
        is_bl = lk.link_mode == "stock_bl"
        where = "Demo" if lk.demo_mode else (lk.port or "")
        if is_bl:
            self.dev_title.set_text(f"Quansheng {radio_model(self.runtime.active_model).label}")
            self.dev_sub.set_text(f"{where} · Bootloader {lk.bootloader_info.get('version', '')}".rstrip())
        else:
            self.dev_title.set_text(lk.volume_name)
            self.dev_sub.set_text(f"{where} · {lk.firmware_name}")
        self.bl_group.set_visible(is_bl)
        self.bl_leave_row.set_visible(is_bl and lk.demo_mode)
        self.bl_hint_row.set_visible(is_bl and not lk.demo_mode)
        self.bl_ver_row.set_subtitle(str(lk.bootloader_info.get("version", "Unknown")))
        for w in (self.dev_tiles, self.info_group, self.power_row):
            w.set_visible(not is_bl)
        self.stack.set_visible_child_name("online")
        self.refresh_security()
        self.refresh_telemetry()

    def refresh_security(self) -> None:
        sec = self.runtime.security
        hard = sec.hardened
        self._updating = True
        self.sw_tx.set_active(sec.allow_usb_tx)
        for attr, row in self._policy_rows.items():
            row.set_active(bool(getattr(sec.policy, attr)))
        self._updating = False
        bits = ["Read-only" if sec.policy.read_only else "Read &amp; write"]
        if sec.policy.allow_sys_write:
            bits.append("/sys writable")
        if not sec.policy.validate_formats:
            bits.append("no validation")
        self.policy_exp.set_subtitle(" · ".join(bits))
        self._reboot_btns[REBOOT_BOOTLOADER].set_sensitive(not hard)

    def refresh_telemetry(self) -> None:
        lk = self.runtime.link
        if not lk.connected:
            self.batt_bar.set_value(0.0)
            return
        if lk.link_mode != "rigwire":
            set_pill(self.state_pill, "Bootloader", "dfu")
            return

        t = lk.last_telemetry
        mv, pct = t.get("battery_mv"), t.get("battery_pct")
        bval, bsub = self._tiles["battery"]
        if mv is not None:
            pct_i = int(max(0, min(100, pct or 0)))
            bval.set_text(f"{pct_i}%")
            bsub.set_text(f"{mv / 1000.0:.2f} V")
            self.batt_bar.set_value(float(pct_i))
            self.batt_icon.set_from_icon_name(battery_icon_name(pct))
        else:
            bval.set_text("—")
            bsub.set_text("No reading")

        lval, lsub = self._tiles["link"]
        lval.set_text("Demo" if lk.demo_mode else f"{lk.fps:.0f} fps")
        lsub.set_text(f"{lk.rtt_ms:.1f} ms")

        uval, usub = self._tiles["uptime"]
        up_ms = t.get("uptime_ms")
        uval.set_text(fmt_duration(up_ms / 1000.0) if up_ms is not None else fmt_duration(time.monotonic() - lk.connected_at))
        usub.set_text("Power on" if up_ms is not None else "Session")

        if self.runtime.security.hardened:
            set_pill(self.state_pill, "Hardened", "hardened")
        elif lk.headless:
            set_pill(self.state_pill, "Headless", "headless")
        elif lk.demo_status == "SAFE":
            set_pill(self.state_pill, "Recovery", "warn")
        else:
            set_pill(self.state_pill, lk.firmware_name, "live")

        rows = self._info_rows
        rows["serial"].set_subtitle(str(t.get("serial_str", "—")))
        rows["mac"].set_subtitle(str(t.get("mac_addr", "—")))
        calib = t.get("calib_valid")
        if calib is None:
            rows["calib"].set_subtitle("Unknown")
            self.calib_icon.set_visible(False)
        else:
            rows["calib"].set_subtitle("Signed /sys/calib.dat (valid)" if calib else "Unsigned or missing")
            self.calib_icon.set_visible(True)
            self.calib_icon.set_from_icon_name("object-select-symbolic" if calib else "dialog-warning-symbolic")
            self.calib_icon.set_css_classes(["success"] if calib else ["warning"])
        clk = t.get("sysclk_hz")
        hi = lk.hello_info or {}
        clk_s = f" · {clk / 1e6:.0f} MHz" if clk else ""
        rows["proto"].set_subtitle(f"RigWire v1.{hi.get('minor_version', 0)}{clk_s}")
