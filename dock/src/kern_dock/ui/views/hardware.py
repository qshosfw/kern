"""Hardware View: SWD debug probes, USB power control & raw UART/module pins (works offline)."""

from __future__ import annotations

import threading
from pathlib import Path
from typing import Any, Callable

import gi

gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")
from gi.repository import Adw, GLib, Gtk  # noqa: E402

from ...device.hardware import (
    SWD_TARGETS,
    RawSerialSession,
    SwdProbe,
    UsbPowerDevice,
    available_hw_tools,
    cycle_usb_authorized,
    discover_swd_probes,
    discover_usb_power_devices,
    parse_raw_tx_input,
    run_swd_action,
    set_usb_power_control,
    uhubctl_power,
)
from ..runtime import DockRuntime
from ..widgets.hex_view import show_hex_viewer_dialog


class HardwareView:
    """Offline-capable Hardware tab: SWD probe, USB power & raw UART/module control."""

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

        self._probes: list[SwdProbe] = []
        self._usb_devs: list[UsbPowerDevice] = []
        self._probe_rows: list[Gtk.Widget] = []
        self._usb_rows: list[Gtk.Widget] = []
        self._swd_busy = False
        self._updating_uart = False

        self._raw_uart = RawSerialSession(
            on_rx=lambda data: GLib.idle_add(self._append_uart_rx, data),
            on_error=lambda err: GLib.idle_add(self._on_uart_error, err),
        )

        self.widget = self._build()
        self.widget.connect("map", lambda _w: self.refresh_devices())
        runtime.events.subscribe("ports.changed", lambda _e: self.refresh_devices(), main_thread=True)
        self.refresh_devices()

    def _build(self) -> Gtk.Box:
        root = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        page = Adw.PreferencesPage()

        # === 1. SWD DEBUG PROBE ===
        self.grp_swd = Adw.PreferencesGroup(
            title="SWD Debug Probe",
            description="Direct Cortex-M0+ control & flashing via CMSIS-DAP, ST-Link, or J-Link (works when radio is off)",
        )
        rescan_swd = Gtk.Button(icon_name="view-refresh-symbolic", tooltip_text="Rescan SWD Probes")
        rescan_swd.add_css_class("flat")
        rescan_swd.connect("clicked", lambda _b: self.refresh_devices())
        self.grp_swd.set_header_suffix(rescan_swd)

        self.swd_target_row = Adw.ComboRow(title="Target MCU")
        self._target_keys = list(SWD_TARGETS.keys())
        self.swd_target_row.set_model(Gtk.StringList.new([SWD_TARGETS[k]["label"] for k in self._target_keys]))
        self.grp_swd.add(self.swd_target_row)

        self.swd_ctrl_row = Adw.ActionRow(title="Target Control", subtitle="Probe, reset, halt, or resume CPU")
        swd_btns = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, valign=Gtk.Align.CENTER)
        swd_btns.add_css_class("linked")
        self._swd_action_btns: list[Gtk.Button] = []
        for act, lbl in (("probe", "Probe"), ("reset", "Reset"), ("halt", "Halt"), ("resume", "Resume")):
            b = Gtk.Button(label=lbl)
            b.connect("clicked", lambda _b, a=act: self._run_swd(a))
            swd_btns.append(b)
            self._swd_action_btns.append(b)
        self.swd_ctrl_row.add_suffix(swd_btns)
        self.grp_swd.add(self.swd_ctrl_row)

        self.swd_mem_row = Adw.ActionRow(title="Memory &amp; Flashing", subtitle="Bypass bootloader &amp; RigWire over SWD")
        mem_btns = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, valign=Gtk.Align.CENTER)
        mem_btns.add_css_class("linked")
        b_flash = Gtk.Button(label="Flash…")
        b_flash.connect("clicked", lambda _b: self._pick_swd_flash())
        b_dump_f = Gtk.Button(label="Dump Flash")
        b_dump_f.connect("clicked", lambda _b: self._run_swd("dump_flash"))
        b_dump_s = Gtk.Button(label="Dump SRAM")
        b_dump_s.connect("clicked", lambda _b: self._run_swd("dump_sram"))
        for b in (b_flash, b_dump_f, b_dump_s):
            mem_btns.append(b)
            self._swd_action_btns.append(b)
        self.swd_mem_row.add_suffix(mem_btns)
        self.grp_swd.add(self.swd_mem_row)

        self.swd_log_exp = Adw.ExpanderRow(title="SWD Output", subtitle="Ready")
        self.swd_log_buf = Gtk.TextBuffer()
        tv = Gtk.TextView(buffer=self.swd_log_buf, editable=False, monospace=True, wrap_mode=Gtk.WrapMode.WORD_CHAR)
        tv.set_size_request(-1, 120)
        sw = Gtk.ScrolledWindow(child=tv, min_content_height=110)
        self.swd_log_exp.add_row(sw)
        self.grp_swd.add(self.swd_log_exp)
        page.add(self.grp_swd)

        # === 2. CONNECTED USB POWER & PORTS ===
        self.grp_power = Adw.PreferencesGroup(
            title="Connected Power &amp; USB",
            description="USB power policy, interface reset &amp; hub VBUS control",
        )
        self.uhub_row = Adw.ActionRow(title="USB Hub VBUS Power", subtitle="Requires uhubctl", visible=False)
        ub = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, valign=Gtk.Align.CENTER)
        ub.add_css_class("linked")
        for act, lbl in (("on", "On"), ("off", "Off"), ("cycle", "Power Cycle")):
            b = Gtk.Button(label=lbl)
            b.connect("clicked", lambda _b, a=act: self._run_uhubctl(a))
            ub.append(b)
        self.uhub_row.add_suffix(ub)
        self.grp_power.add(self.uhub_row)
        page.add(self.grp_power)

        # === 3. RAW UART / PHYSICAL PINS & MODULES (NO RIGWIRE) ===
        grp_raw = Adw.PreferencesGroup(
            title="Raw Serial &amp; Physical Pins",
            description="Direct UART, DTR/RTS reset/boot lines &amp; raw module bytes without RigWire",
        )
        self.uart_port_row = Adw.ComboRow(title="Serial Port")
        self.uart_port_row.set_model(Gtk.StringList.new(["None"]))
        self.grp_raw_ports: list[str] = []

        self.uart_baud_row = Adw.ComboRow(title="Baudrate")
        self._bauds = [38400, 115200, 9600, 921600]
        self.uart_baud_row.set_model(Gtk.StringList.new([str(b) for b in self._bauds]))

        self.uart_open_btn = Gtk.Button(label="Open Raw", valign=Gtk.Align.CENTER)
        self.uart_open_btn.connect("clicked", lambda _b: self._toggle_raw_uart())
        self.uart_port_row.add_suffix(self.uart_open_btn)
        grp_raw.add(self.uart_port_row)
        grp_raw.add(self.uart_baud_row)

        self.sw_dtr = Adw.SwitchRow(title="DTR Line", subtitle="Physical Data Terminal Ready pin (often Reset / Power)")
        self.sw_dtr.connect("notify::active", lambda w, _p: None if self._updating_uart else self._set_line("dtr", w.get_active()))
        grp_raw.add(self.sw_dtr)

        self.sw_rts = Adw.SwitchRow(title="RTS Line", subtitle="Physical Request To Send pin (often BOOT0 / PTT)")
        self.sw_rts.connect("notify::active", lambda w, _p: None if self._updating_uart else self._set_line("rts", w.get_active()))
        grp_raw.add(self.sw_rts)

        pulse_row = Adw.ActionRow(title="Hardware Signals", subtitle="Pulse physical lines or send UART break")
        pbox = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, valign=Gtk.Align.CENTER)
        pbox.add_css_class("linked")
        self._uart_ctrl_btns: list[Gtk.Button] = []
        for lbl, fn in (
            ("Pulse DTR", lambda: self._pulse("dtr")),
            ("Pulse RTS", lambda: self._pulse("rts")),
            ("UART Break", lambda: self._break()),
        ):
            b = Gtk.Button(label=lbl)
            b.connect("clicked", lambda _b, f=fn: f())
            pbox.append(b)
            self._uart_ctrl_btns.append(b)
        pulse_row.add_suffix(pbox)
        grp_raw.add(pulse_row)

        self.raw_tx_row = Adw.EntryRow(title="Send Raw (ASCII or 'hex: aa bb cc')", show_apply_button=True)
        self.raw_tx_row.connect("apply", self._on_raw_tx)
        self.raw_tx_row.connect("entry-activated", self._on_raw_tx)
        grp_raw.add(self.raw_tx_row)

        self.uart_rx_exp = Adw.ExpanderRow(title="Raw RX Monitor", subtitle="Closed")
        self.uart_rx_buf = Gtk.TextBuffer()
        rtv = Gtk.TextView(buffer=self.uart_rx_buf, editable=False, monospace=True, wrap_mode=Gtk.WrapMode.WORD_CHAR)
        rtv.set_size_request(-1, 110)
        rsw = Gtk.ScrolledWindow(child=rtv, min_content_height=100)
        self.uart_rx_exp.add_row(rsw)
        grp_raw.add(self.uart_rx_exp)

        page.add(grp_raw)
        root.append(page)
        self._sync_uart_controls()
        return root

    def refresh_devices(self) -> None:
        tools = available_hw_tools()
        self._probes = discover_swd_probes()
        self._usb_devs = discover_usb_power_devices()

        # Rebuild SWD probe rows
        for w in self._probe_rows:
            self.grp_swd.remove(w)
        self._probe_rows.clear()

        if not self._probes:
            row = Adw.ActionRow(
                title="No SWD Probe Detected",
                subtitle="Connect CMSIS-DAP, ST-Link, or J-Link" + (" · openocd ready" if tools["openocd"] else " · install openocd"),
            )
            row.add_prefix(Gtk.Image(icon_name="network-wired-symbolic"))
            row.set_sensitive(False)
            self.grp_swd.add(row)
            self._probe_rows.append(row)
        else:
            for p in self._probes:
                sub = f"{p.vid}:{p.pid} · {p.driver}" + (f" · S/N {p.serial}" if p.serial else "")
                row = Adw.ActionRow(title=GLib.markup_escape_text(p.name), subtitle=GLib.markup_escape_text(sub))
                row.add_prefix(Gtk.Image(icon_name="object-select-symbolic"))
                self.grp_swd.add(row)
                self._probe_rows.append(row)

        can_swd = bool(self._probes and tools["openocd"] and not self._swd_busy)
        for b in self._swd_action_btns:
            b.set_sensitive(can_swd)
        self.swd_target_row.set_sensitive(bool(self._probes and not self._swd_busy))

        # Rebuild USB Power rows
        self.uhub_row.set_visible(bool(tools["uhubctl"]))
        for w in self._usb_rows:
            self.grp_power.remove(w)
        self._usb_rows.clear()

        if not self._usb_devs:
            row = Adw.ActionRow(title="No USB Radio / Module Connected", subtitle="Plug in a radio, SWD probe, or USB-UART adapter")
            row.set_sensitive(False)
            self.grp_power.add(row)
            self._usb_rows.append(row)
        else:
            for d in self._usb_devs:
                parts = [f"{d.vid}:{d.pid}", d.bus_port]
                if d.max_power_ma:
                    parts.append(f"{d.max_power_ma} mA")
                parts.append(f"power:{d.power_control}/{d.runtime_status}")
                if d.tty_dev:
                    parts.append(d.tty_dev)
                row = Adw.ActionRow(
                    title=GLib.markup_escape_text(d.name),
                    subtitle=GLib.markup_escape_text(" · ".join(parts)),
                )
                btns = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, valign=Gtk.Align.CENTER)
                btns.add_css_class("linked")
                next_mode = "auto" if d.power_control == "on" else "on"
                pm_btn = Gtk.Button(label="Keep On" if next_mode == "on" else "Auto Suspend")
                pm_btn.connect("clicked", lambda _b, sp=d.sysfs_path, m=next_mode: self._set_usb_power(sp, m))
                rst_btn = Gtk.Button(label="USB Reset")
                rst_btn.connect("clicked", lambda _b, sp=d.sysfs_path, nm=d.name: self._reset_usb_dev(sp, nm))
                btns.append(pm_btn)
                btns.append(rst_btn)
                row.add_suffix(btns)
                self.grp_power.add(row)
                self._usb_rows.append(row)

        # Refresh raw serial port list
        ports = [p.device for p in self.runtime.port_monitor.ports]
        self.grp_raw_ports = ports
        self.uart_port_row.set_model(Gtk.StringList.new(ports or ["No serial port"]))
        self.uart_open_btn.set_sensitive(bool(ports) or self._raw_uart.is_open)

    def _selected_target(self) -> str:
        idx = int(self.swd_target_row.get_selected())
        if 0 <= idx < len(self._target_keys):
            return self._target_keys[idx]
        return "py32f071"

    def _run_swd(self, action: str, fw_path: Path | None = None) -> None:
        if not self._probes or self._swd_busy:
            return
        probe = self._probes[0]
        target = self._selected_target()
        self._swd_busy = True
        self.swd_log_exp.set_subtitle(f"Running {action}…")
        self.refresh_devices()

        def _w() -> None:
            ok, out, blob = run_swd_action(probe, target, action, fw_path=fw_path)

            def _done() -> bool:
                self._swd_busy = False
                self.swd_log_buf.set_text(out or ("OK" if ok else "Failed"))
                self.swd_log_exp.set_subtitle("OK" if ok else "Failed")
                self.swd_log_exp.set_expanded(True)
                self._on_toast(f"SWD {action}: {'OK' if ok else 'failed'}")
                if ok and blob is not None:
                    label = "flash_swd.bin" if action == "dump_flash" else "sram_swd.bin"
                    show_hex_viewer_dialog(self.win, blob, label)
                self.refresh_devices()
                return False

            GLib.idle_add(_done)

        threading.Thread(target=_w, daemon=True).start()

    def _pick_swd_flash(self) -> None:
        dlg = Gtk.FileDialog(title="Select Firmware (.bin or .kfw) to Flash via SWD")

        def _picked(d: Any, res: Any) -> None:
            try:
                fpath = d.open_finish(res).get_path()
            except GLib.Error:
                return
            if fpath:
                p = Path(fpath)
                self._on_confirm(
                    f"SWD Flash {p.name}?",
                    "Writes directly to 0x08000000 via the connected SWD debug probe.",
                    "Flash via SWD",
                    lambda: self._run_swd("flash_bin", fw_path=p),
                )

        dlg.open(self.win, None, _picked)

    def _set_usb_power(self, sysfs_path: str, mode: str) -> None:
        ok, msg = set_usb_power_control(sysfs_path, mode)
        self._on_toast(msg)
        if ok:
            self.refresh_devices()

    def _reset_usb_dev(self, sysfs_path: str, name: str) -> None:
        def _w() -> None:
            _ok, msg = cycle_usb_authorized(sysfs_path)
            GLib.idle_add(self._on_toast, f"{name}: {msg}")
            GLib.idle_add(self.refresh_devices)

        threading.Thread(target=_w, daemon=True).start()

    def _run_uhubctl(self, action: str) -> None:
        def _w() -> None:
            _ok, msg = uhubctl_power(action)
            first = next((ln for ln in msg.splitlines() if ln.strip()), msg)
            GLib.idle_add(self._on_toast, first)
            GLib.idle_add(self.refresh_devices)

        threading.Thread(target=_w, daemon=True).start()

    def _sync_uart_controls(self) -> None:
        is_open = self._raw_uart.is_open
        self.uart_open_btn.set_label("Close" if is_open else "Open Raw")
        self.uart_port_row.set_sensitive(not is_open)
        self.uart_baud_row.set_sensitive(not is_open)
        self.sw_dtr.set_sensitive(is_open)
        self.sw_rts.set_sensitive(is_open)
        self.raw_tx_row.set_sensitive(is_open)
        for b in self._uart_ctrl_btns:
            b.set_sensitive(is_open)
        self.uart_rx_exp.set_subtitle(f"Open · {self._raw_uart.port} @ {self._raw_uart.baudrate}" if is_open else "Closed")

    def _toggle_raw_uart(self) -> None:
        if self._raw_uart.is_open:
            self._raw_uart.close()
            self._sync_uart_controls()
            return
        idx = int(self.uart_port_row.get_selected())
        if not (0 <= idx < len(self.grp_raw_ports)):
            self._on_toast("No serial port selected")
            return
        port = self.grp_raw_ports[idx]
        if self.runtime.link.connected and self.runtime.link.port == port:
            self._on_toast("Disconnect active Dock link on that port first, or pick another port")
            return
        b_idx = int(self.uart_baud_row.get_selected())
        baud = self._bauds[b_idx] if 0 <= b_idx < len(self._bauds) else 115200
        try:
            self._raw_uart.open(port, baudrate=baud, dtr=self.sw_dtr.get_active(), rts=self.sw_rts.get_active())
            self.uart_rx_buf.set_text("")
            self.uart_rx_exp.set_expanded(True)
        except Exception as exc:
            self._on_toast(f"Serial open failed: {exc}")
        self._sync_uart_controls()

    def _set_line(self, line: str, state: bool) -> None:
        try:
            if line == "dtr":
                self._raw_uart.set_dtr(state)
            else:
                self._raw_uart.set_rts(state)
        except Exception as exc:
            self._on_toast(str(exc))

    def _pulse(self, line: str) -> None:
        def _w() -> None:
            try:
                self._raw_uart.pulse_line(line, active_high=True, duration_s=0.08)
                GLib.idle_add(self._on_toast, f"Pulsed {line.upper()} (80 ms)")
            except Exception as exc:
                GLib.idle_add(self._on_toast, str(exc))

        threading.Thread(target=_w, daemon=True).start()

    def _break(self) -> None:
        try:
            self._raw_uart.send_break(0.25)
            self._on_toast("Sent UART BREAK")
        except Exception as exc:
            self._on_toast(str(exc))

    def _on_raw_tx(self, _w: Any) -> None:
        txt = self.raw_tx_row.get_text()
        if not txt.strip():
            return
        try:
            payload = parse_raw_tx_input(txt)
            self._raw_uart.write(payload)
            self.raw_tx_row.set_text("")
        except Exception as exc:
            self._on_toast(f"TX error: {exc}")

    def _append_uart_rx(self, data: bytes) -> bool:
        hex_s = data.hex(" ")
        asc_s = "".join(chr(b) if 32 <= b < 127 else "." for b in data)
        line = f"{hex_s}  | {asc_s}\n"
        end_it = self.uart_rx_buf.get_end_iter()
        self.uart_rx_buf.insert(end_it, line)
        return False

    def _on_uart_error(self, err: str) -> bool:
        self._raw_uart.close()
        self._sync_uart_controls()
        self._on_toast(f"Serial error: {err}")
        return False
