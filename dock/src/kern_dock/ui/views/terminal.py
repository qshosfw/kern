"""Terminal & Wire View: Embedded GTK4 VTE-3.91 OS Terminal + RigWire Command Console."""

from __future__ import annotations

import os
import threading
from typing import Any, Callable

import gi

gi.require_version("Gtk", "4.0")
gi.require_version("Gdk", "4.0")
gi.require_version("Adw", "1")
from gi.repository import Adw, Gdk, GLib, Gtk  # noqa: E402

from ... import REPO_ROOT
from ...core.events import Event
from ...system.desktop import build_radio_shell_env
from ..runtime import DockRuntime
from ..widgets.util import set_pill


class TerminalView:
    """Dual-mode workbench combining a real GTK4 VTE shell with the interactive RigWire console."""

    def __init__(self, runtime: DockRuntime, *, on_toast: Callable[[str], None]) -> None:
        self.runtime = runtime
        self._on_toast = on_toast
        self._cmd_history: list[str] = []
        self._cmd_hist_pos = 0
        self._vte_term: Any = None
        self._vte_spawned = False

        self.widget = self._build()

        runtime.events.subscribe("log.line", self._on_log_event, main_thread=True)
        runtime.events.subscribe("link.connected", lambda _e: self._refresh_pill(), main_thread=True)
        runtime.events.subscribe("link.disconnected", lambda _e: self._refresh_pill(), main_thread=True)
        runtime.events.subscribe("link.telemetry", lambda _e: self._refresh_pill(), main_thread=True)
        self._refresh_pill()

    def _build(self) -> Gtk.Box:
        root = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=8, margin_top=10, margin_bottom=12, margin_start=14, margin_end=14)

        # Header utility strip: Mode switcher (RigWire Console | OS Terminal) + Status Pill + External Terminal
        top = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        self.sub_stack = Adw.ViewStack(vexpand=True, hexpand=True)
        switcher = Adw.ViewSwitcher(stack=self.sub_stack, policy=Adw.ViewSwitcherPolicy.WIDE)
        top.append(switcher)
        top.append(Gtk.Box(hexpand=True))

        self.status_pill = Gtk.Label(label="Offline", valign=Gtk.Align.CENTER)
        self.status_pill.add_css_class("status-pill")
        top.append(self.status_pill)

        ext_btn = Gtk.Button(tooltip_text="Open in External OS Terminal (Ctrl+Shift+Enter)")
        eb_in = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=5)
        eb_in.append(Gtk.Image.new_from_icon_name("utilities-terminal-symbolic"))
        eb_in.append(Gtk.Label(label="Pop Out"))
        ext_btn.set_child(eb_in)
        ext_btn.connect("clicked", lambda _b: self._launch_external())
        top.append(ext_btn)
        root.append(top)

        self.sub_stack.add_titled_with_icon(self._build_vte_terminal(), "shell", "Terminal", "utilities-terminal-symbolic")
        self.sub_stack.add_titled_with_icon(self._build_wire_console(), "wire", "RigWire", "network-transmit-receive-symbolic")
        self.sub_stack.set_visible_child_name("shell")
        root.append(self.sub_stack)
        return root

    def _build_wire_console(self) -> Gtk.Box:
        box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=8, margin_top=6)

        chips = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=5)
        for cmd in ("ping", "info", "ls /", "vfo", "headless", "audit", "help"):
            cb = Gtk.Button(label=cmd)
            cb.add_css_class("quick-chip")
            cb.connect("clicked", lambda _b, c=cmd: self.run_command(c))
            chips.append(cb)
        chips.append(Gtk.Box(hexpand=True))
        copy_b = Gtk.Button.new_from_icon_name("edit-copy-symbolic")
        copy_b.add_css_class("flat")
        copy_b.set_tooltip_text("Copy console log")
        copy_b.connect("clicked", lambda _b: self._copy_console())
        chips.append(copy_b)
        clr_b = Gtk.Button.new_from_icon_name("edit-clear-all-symbolic")
        clr_b.add_css_class("flat")
        clr_b.set_tooltip_text("Clear console")
        clr_b.connect("clicked", lambda _b: self.console_buf.set_text(""))
        chips.append(clr_b)
        box.append(chips)

        frame = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, vexpand=True)
        frame.add_css_class("console-frame")
        scroll = Gtk.ScrolledWindow(vexpand=True)
        self.console_buf = Gtk.TextBuffer()
        self.console_buf.create_tag("cmd", foreground="#38bdf8", weight=700)
        self.console_buf.create_tag("ok", foreground="#4ade80")
        self.console_buf.create_tag("err", foreground="#f87171")
        self.console_buf.create_tag("meta", foreground="#94a3b8")
        self.console_view = Gtk.TextView(buffer=self.console_buf, editable=False, monospace=True)
        self.console_view.set_wrap_mode(Gtk.WrapMode.WORD_CHAR)
        self.console_view.add_css_class("wire-console")
        scroll.set_child(self.console_view)
        frame.append(scroll)
        box.append(frame)

        row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=6)
        p_lbl = Gtk.Label(label="❯")
        p_lbl.add_css_class("console-prompt")
        row.append(p_lbl)
        self.cmd_entry = Gtk.Entry(hexpand=True, placeholder_text="RigWire command (ping, info, freq 433.500, audit, help)…")
        self.cmd_entry.add_css_class("console-entry")
        self.cmd_entry.connect("activate", lambda _w: self._submit())
        ec = Gtk.EventControllerKey()
        ec.connect("key-pressed", self._on_history_key)
        self.cmd_entry.add_controller(ec)
        row.append(self.cmd_entry)

        btn = Gtk.Button.new_from_icon_name("go-next-symbolic")
        btn.add_css_class("suggested-action")
        btn.connect("clicked", lambda _b: self._submit())
        row.append(btn)
        box.append(row)
        return box

    def _build_vte_terminal(self) -> Gtk.Box:
        box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=8, margin_top=6)
        try:
            gi.require_version("Vte", "3.91")
            from gi.repository import Pango, Vte

            bar = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=4)
            for lbl, snippet in (
                ("./kern info", "./kern info\n"),
                ("./kern dock ctl status", "./kern dock ctl status\n"),
                ("./kern check", "./kern check\n"),
            ):
                b = Gtk.Button(label=lbl)
                b.add_css_class("quick-chip")
                b.connect("clicked", lambda _b, s=snippet: self._feed_vte(s))
                bar.append(b)
            bar.append(Gtk.Box(hexpand=True))

            for icon, tip, cb in (
                ("edit-copy-symbolic", "Copy (Ctrl+Shift+C)", lambda: self._vte_term.copy_clipboard_format(Vte.Format.TEXT)),
                ("edit-paste-symbolic", "Paste (Ctrl+Shift+V)", lambda: self._vte_term.paste_clipboard()),
                ("zoom-out-symbolic", "Zoom Out", lambda: self._vte_zoom(-0.1)),
                ("zoom-in-symbolic", "Zoom In", lambda: self._vte_zoom(0.1)),
                ("view-refresh-symbolic", "Respawn shell with latest radio env", lambda: self._spawn_vte(force=True)),
            ):
                tb = Gtk.Button(icon_name=icon, tooltip_text=tip)
                tb.add_css_class("flat")
                tb.connect("clicked", lambda _b, f=cb: f())
                bar.append(tb)
            box.append(bar)

            term = Vte.Terminal(hexpand=True, vexpand=True)
            term.set_scrollback_lines(10000)
            term.set_scroll_on_output(False)
            term.set_scroll_on_keystroke(True)
            term.set_audible_bell(False)
            term.set_cursor_blink_mode(Vte.CursorBlinkMode.SYSTEM)
            term.set_font(Pango.FontDescription.from_string("Monospace 10"))

            # Crisp 16-color modern GNOME/Adwaita dark ANSI terminal palette
            palette_hex = (
                "#1e222a", "#f87171", "#4ade80", "#fbbf24", "#60a5fa", "#c084fc", "#22d3ee", "#cbd5e1",
                "#475569", "#fca5a5", "#86efac", "#fde047", "#93c5fd", "#d8b4fe", "#67e8f9", "#f8fafc",
            )
            palette: list[Gdk.RGBA] = []
            for h in palette_hex:
                c = Gdk.RGBA()
                c.parse(h)
                palette.append(c)
            bg = Gdk.RGBA()
            bg.parse("#101318")
            fg = Gdk.RGBA()
            fg.parse("#e5e7eb")
            term.set_colors(fg, bg, palette)

            # Ctrl+Shift+C / Ctrl+Shift+V / Ctrl+Plus / Ctrl+Minus / Ctrl+0
            kc = Gtk.EventControllerKey(propagation_phase=Gtk.PropagationPhase.CAPTURE)
            kc.connect("key-pressed", self._on_vte_key)
            term.add_controller(kc)
            self._vte_term = term

            frame = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, vexpand=True)
            frame.add_css_class("console-frame")
            scroll = Gtk.ScrolledWindow(vexpand=True)
            scroll.set_child(term)
            frame.append(scroll)
            box.append(frame)

            # Lazy-spawn shell when the Terminal tab is actually opened (picks up live KERN_PORT/KERN_MOUNT_PATH)
            box.connect("map", lambda _w: self._spawn_vte(force=False))
        except Exception as exc:
            box.append(
                Adw.StatusPage(
                    icon_name="utilities-terminal-symbolic",
                    title="Embedded VTE Unavailable",
                    description=f"Install vte4 (Vte-3.91) for an embedded GTK4 terminal, or click Pop Out above ({exc}).",
                )
            )
        return box

    def _spawn_vte(self, *, force: bool) -> None:
        if self._vte_term is None or (self._vte_spawned and not force):
            return
        try:
            gi.require_version("Vte", "3.91")
            from gi.repository import Vte

            shell = os.environ.get("SHELL", "/bin/bash")
            env_map = build_radio_shell_env(self.runtime.shell_env_extra())
            envv = [f"{k}={v}" for k, v in env_map.items()]
            if force:
                self._vte_term.reset(True, True)
            self._vte_term.spawn_async(
                Vte.PtyFlags.DEFAULT,
                str(REPO_ROOT),
                [shell],
                envv,
                GLib.SpawnFlags.DEFAULT,
                None,
                None,
                -1,
                None,
                None,
            )
            self._vte_spawned = True
        except Exception as exc:
            self._on_toast(f"Shell spawn failed: {exc}")

    def _vte_zoom(self, delta: float) -> None:
        if self._vte_term is None:
            return
        cur = float(self._vte_term.get_font_scale())
        self._vte_term.set_font_scale(max(0.6, min(2.2, cur + delta)))

    def _on_vte_key(self, _c: Any, keyval: int, _kc: int, state: Any) -> bool:
        if self._vte_term is None:
            return False
        ctrl = bool(state & Gdk.ModifierType.CONTROL_MASK)
        shift = bool(state & Gdk.ModifierType.SHIFT_MASK)
        if ctrl and shift and keyval in (Gdk.KEY_C, Gdk.KEY_c):
            gi.require_version("Vte", "3.91")
            from gi.repository import Vte

            self._vte_term.copy_clipboard_format(Vte.Format.TEXT)
            return True
        if ctrl and shift and keyval in (Gdk.KEY_V, Gdk.KEY_v):
            self._vte_term.paste_clipboard()
            return True
        if ctrl and keyval in (Gdk.KEY_plus, Gdk.KEY_equal, Gdk.KEY_KP_Add):
            self._vte_zoom(0.1)
            return True
        if ctrl and keyval in (Gdk.KEY_minus, Gdk.KEY_KP_Subtract):
            self._vte_zoom(-0.1)
            return True
        if ctrl and keyval in (Gdk.KEY_0, Gdk.KEY_KP_0):
            self._vte_term.set_font_scale(1.0)
            return True
        return False

    def _feed_vte(self, text: str) -> None:
        if self._vte_term is not None:
            self._vte_term.feed_child(text.encode("utf-8"))
            self._vte_term.grab_focus()

    def _launch_external(self) -> None:
        try:
            term = self.runtime.open_external_terminal()
            self._on_toast(f"Opened {term} with Kern environment")
        except Exception as exc:
            self._on_toast(str(exc))

    def _on_log_event(self, ev: Event) -> None:
        line = str(ev.payload.get("line", ""))
        if line:
            self.append_line(line)

    def _refresh_pill(self) -> None:
        lk = self.runtime.link
        if not lk.connected:
            set_pill(self.status_pill, "Offline", None)
        elif lk.link_mode != "rigwire":
            set_pill(self.status_pill, f"Bootloader {lk.bootloader_info.get('version', '')}".strip(), "dfu")
        elif lk.headless:
            set_pill(self.status_pill, f"Headless · {lk.fps:.0f} FPS", "headless")
        else:
            fps_s = "Demo" if lk.demo_mode else f"{lk.fps:.0f} FPS"
            set_pill(self.status_pill, f"Connected · {fps_s} · {lk.rtt_ms:.1f} ms", "live")

    def append_line(self, line: str) -> None:
        end = self.console_buf.get_end_iter()
        s = line.strip()
        tag = None
        if s.startswith("❯ ") or s.startswith("> "):
            tag = "cmd"
        elif s.startswith("["):
            tag = "meta"
        elif any(tok in s for tok in ("PONG", "Connected", "Mounted", "ON (", "Tuned", "Swapped")):
            tag = "ok"
        elif any(tok in s.lower() for tok in ("error", "failed", "unknown", "invalid", "unavailable", "blocked")):
            tag = "err"
        if tag:
            self.console_buf.insert_with_tags_by_name(end, line + "\n", tag)
        else:
            self.console_buf.insert(end, line + "\n")
        if self.console_buf.get_line_count() > 2000:
            ok, cut = self.console_buf.get_iter_at_line(self.console_buf.get_line_count() - 1800)
            if ok:
                self.console_buf.delete(self.console_buf.get_start_iter(), cut)
        self.console_view.scroll_to_iter(self.console_buf.get_end_iter(), 0.0, False, 0.0, 1.0)

    def _copy_console(self) -> None:
        start, end = self.console_buf.get_bounds()
        txt = self.console_buf.get_text(start, end, False)
        if txt:
            Gdk.Display.get_default().get_clipboard().set(txt)
            self._on_toast("Copied console log")

    def _on_history_key(self, _c: Any, keyval: int, _kc: int, _st: Any) -> bool:
        if not self._cmd_history:
            return False
        if keyval == Gdk.KEY_Up:
            self._cmd_hist_pos = max(0, self._cmd_hist_pos - 1)
            self.cmd_entry.set_text(self._cmd_history[self._cmd_hist_pos])
            self.cmd_entry.set_position(-1)
            return True
        if keyval == Gdk.KEY_Down:
            self._cmd_hist_pos = min(len(self._cmd_history), self._cmd_hist_pos + 1)
            self.cmd_entry.set_text(self._cmd_history[self._cmd_hist_pos] if self._cmd_hist_pos < len(self._cmd_history) else "")
            self.cmd_entry.set_position(-1)
            return True
        return False

    def _submit(self) -> None:
        cmd = self.cmd_entry.get_text().strip()
        if not cmd:
            return
        self.cmd_entry.set_text("")
        self.run_command(cmd)

    def run_command(self, cmd: str) -> None:
        cmd = cmd.strip()
        if not cmd:
            return
        if cmd.lower() == "clear":
            self.console_buf.set_text("")
            return
        if not self._cmd_history or self._cmd_history[-1] != cmd:
            self._cmd_history.append(cmd)
        self._cmd_hist_pos = len(self._cmd_history)
        self.append_line(f"❯ {cmd}")
        threading.Thread(
            target=lambda: GLib.idle_add(lambda out=self.runtime.link.run_wire_command(cmd): (self.append_line(out), False)[1]),
            daemon=True,
        ).start()
