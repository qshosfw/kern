"""Terminal View: GNOME Console-style embedded VTE shell & RigWire command console."""

from __future__ import annotations

import os
import threading
from typing import Any, Callable

import gi

gi.require_version("Gtk", "4.0")
gi.require_version("Gdk", "4.0")
gi.require_version("Adw", "1")
from gi.repository import Adw, Gdk, Gio, GLib, Gtk  # noqa: E402

from ... import REPO_ROOT
from ...core.events import Event
from ...system.desktop import build_radio_shell_env
from ..runtime import DockRuntime


class TerminalView:
    """Clean Adw.ToolbarView workbench: full-bleed GTK4 VTE shell + RigWire command console."""

    def __init__(self, runtime: DockRuntime, *, on_toast: Callable[[str], None]) -> None:
        self.runtime = runtime
        self._on_toast = on_toast
        self._cmd_history: list[str] = []
        self._cmd_hist_pos = 0
        self._vte_term: Any = None
        self._vte_spawned = False

        self.widget = self._build()
        runtime.events.subscribe("log.line", self._on_log_event, main_thread=True)

    def _build(self) -> Adw.ToolbarView:
        view = Adw.ToolbarView()
        self.sub_stack = Adw.ViewStack(vexpand=True, hexpand=True)

        # === TOP TOOLBAR (GNOME Console style) ===
        top_wrap = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        bar = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=6)
        bar.add_css_class("toolbar")

        switcher = Adw.ViewSwitcher(stack=self.sub_stack, policy=Adw.ViewSwitcherPolicy.WIDE)
        bar.append(switcher)
        bar.append(Gtk.Box(hexpand=True))

        # Clean Snippets popover (replaces quick-chip pill clutter)
        snip_btn = Gtk.MenuButton(icon_name="view-list-symbolic", tooltip_text="Quick Commands")
        snip_btn.add_css_class("flat")
        snip_btn.set_popover(self._build_snippets_popover())
        bar.append(snip_btn)

        # Shell actions (visible in Shell mode)
        self.shell_tools = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=2)
        term_actions = Gio.SimpleActionGroup()
        for name, cb in (
            ("copy", self._copy_active),
            ("paste", self._paste_vte),
            ("zoom-in", lambda: self._vte_zoom(0.1)),
            ("zoom-out", lambda: self._vte_zoom(-0.1)),
            ("restart", lambda: self._spawn_vte(force=True)),
        ):
            act = Gio.SimpleAction.new(name, None)
            act.connect("activate", lambda _a, _p, f=cb: f())
            term_actions.add_action(act)
        view.insert_action_group("term", term_actions)
        restart_b = Gtk.Button(icon_name="view-refresh-symbolic", tooltip_text="Restart shell with latest radio env", action_name="term.restart")
        restart_b.add_css_class("flat")
        self.shell_tools.append(restart_b)
        term_menu = Gio.Menu()
        edit_sec = Gio.Menu()
        edit_sec.append("Copy", "term.copy")
        edit_sec.append("Paste", "term.paste")
        term_menu.append_section(None, edit_sec)
        zoom_sec = Gio.Menu()
        zoom_sec.append("Zoom In", "term.zoom-in")
        zoom_sec.append("Zoom Out", "term.zoom-out")
        term_menu.append_section(None, zoom_sec)
        more_b = Gtk.MenuButton(icon_name="view-more-symbolic", tooltip_text="Terminal Menu", menu_model=term_menu)
        more_b.add_css_class("flat")
        self.shell_tools.append(more_b)
        bar.append(self.shell_tools)

        # RigWire actions (visible in RigWire mode)
        self.wire_tools = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=2, visible=False)
        for icon, tip, cb in (
            ("edit-copy-symbolic", "Copy console log", self._copy_console),
            ("edit-clear-all-symbolic", "Clear console", lambda: self.console_buf.set_text("")),
        ):
            tb = Gtk.Button(icon_name=icon, tooltip_text=tip)
            tb.add_css_class("flat")
            tb.connect("clicked", lambda _b, f=cb: f())
            self.wire_tools.append(tb)
        bar.append(self.wire_tools)

        ext_btn = Gtk.Button(icon_name="window-new-symbolic", tooltip_text="Open in External Terminal (Ctrl+Shift+Enter)")
        ext_btn.add_css_class("flat")
        ext_btn.connect("clicked", lambda _b: self._launch_external())
        bar.append(ext_btn)

        top_wrap.append(bar)
        top_wrap.append(Gtk.Separator(orientation=Gtk.Orientation.HORIZONTAL))
        view.add_top_bar(top_wrap)

        self.sub_stack.add_titled_with_icon(self._build_vte_terminal(), "shell", "Shell", "utilities-terminal-symbolic")
        self.sub_stack.add_titled_with_icon(self._build_wire_console(), "wire", "RigWire", "network-transmit-receive-symbolic")
        self.sub_stack.set_visible_child_name("shell")
        self.sub_stack.connect("notify::visible-child-name", self._on_mode_changed)

        view.set_content(self.sub_stack)
        return view

    def _on_mode_changed(self, stack: Adw.ViewStack, _p: Any) -> None:
        is_shell = stack.get_visible_child_name() == "shell"
        self.shell_tools.set_visible(is_shell)
        self.wire_tools.set_visible(not is_shell)

    def _build_snippets_popover(self) -> Gtk.Popover:
        pop = Gtk.Popover()
        box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=2, margin_top=6, margin_bottom=6, margin_start=6, margin_end=6)

        def _hdr(text: str) -> None:
            l = Gtk.Label(label=text, xalign=0, margin_start=6, margin_top=4, margin_bottom=2)
            l.add_css_class("caption-heading")
            l.add_css_class("dim-label")
            box.append(l)

        _hdr("Shell Commands")
        for cmd in ("./kern info", "./kern check", "./kern doctor", "./kern ports", "./kern dock ctl status"):
            b = Gtk.Button(label=cmd)
            b.add_css_class("flat")
            b.set_halign(Gtk.Align.FILL)
            b.connect("clicked", lambda _b, c=cmd: (pop.popdown(), self.sub_stack.set_visible_child_name("shell"), self._feed_vte(c + "\n")))
            box.append(b)

        box.append(Gtk.Separator(orientation=Gtk.Orientation.HORIZONTAL, margin_top=4, margin_bottom=4))
        _hdr("RigWire Console")
        for cmd in ("ping", "info", "ls /", "ls /apps", "headless", "audit", "help"):
            b = Gtk.Button(label=cmd)
            b.add_css_class("flat")
            b.set_halign(Gtk.Align.FILL)
            b.connect("clicked", lambda _b, c=cmd: (pop.popdown(), self.sub_stack.set_visible_child_name("wire"), self.run_command(c)))
            box.append(b)

        pop.set_child(box)
        return pop

    def _build_wire_console(self) -> Gtk.Box:
        box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=0)

        scroll = Gtk.ScrolledWindow(vexpand=True)
        self.console_buf = Gtk.TextBuffer()
        self.console_buf.create_tag("cmd", foreground="#38bdf8", weight=700)
        self.console_buf.create_tag("ok", foreground="#4ade80")
        self.console_buf.create_tag("err", foreground="#f87171")
        self.console_buf.create_tag("meta", foreground="#94a3b8")
        self.console_view = Gtk.TextView(
            buffer=self.console_buf,
            editable=False,
            monospace=True,
            top_margin=10,
            bottom_margin=10,
            left_margin=12,
            right_margin=12,
        )
        self.console_view.set_wrap_mode(Gtk.WrapMode.WORD_CHAR)
        self.console_view.add_css_class("wire-console")
        scroll.set_child(self.console_view)
        box.append(scroll)

        box.append(Gtk.Separator(orientation=Gtk.Orientation.HORIZONTAL))
        row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=6)
        row.add_css_class("toolbar")
        p_lbl = Gtk.Label(label="❯")
        p_lbl.add_css_class("console-prompt")
        row.append(p_lbl)
        self.cmd_entry = Gtk.Entry(hexpand=True, placeholder_text="RigWire command (ping, info, ls /, audit, help)…")
        self.cmd_entry.add_css_class("console-entry")
        self.cmd_entry.connect("activate", lambda _w: self._submit())
        ec = Gtk.EventControllerKey()
        ec.connect("key-pressed", self._on_history_key)
        self.cmd_entry.add_controller(ec)
        row.append(self.cmd_entry)

        btn = Gtk.Button(icon_name="go-next-symbolic", tooltip_text="Run command")
        btn.add_css_class("suggested-action")
        btn.connect("clicked", lambda _b: self._submit())
        row.append(btn)
        box.append(row)
        return box

    def _build_vte_terminal(self) -> Gtk.Box:
        box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        try:
            gi.require_version("Vte", "3.91")
            from gi.repository import Pango, Vte

            term = Vte.Terminal(hexpand=True, vexpand=True)
            term.set_scrollback_lines(10000)
            term.set_scroll_on_output(False)
            term.set_scroll_on_keystroke(True)
            term.set_audible_bell(False)
            term.set_cursor_blink_mode(Vte.CursorBlinkMode.SYSTEM)
            term.set_font(Pango.FontDescription.from_string("Monospace 10"))

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

            kc = Gtk.EventControllerKey(propagation_phase=Gtk.PropagationPhase.CAPTURE)
            kc.connect("key-pressed", self._on_vte_key)
            term.add_controller(kc)
            self._vte_term = term

            scroll = Gtk.ScrolledWindow(vexpand=True)
            scroll.set_child(term)
            box.append(scroll)
            box.connect("map", lambda _w: self._spawn_vte(force=False))
        except Exception as exc:
            box.append(
                Adw.StatusPage(
                    icon_name="utilities-terminal-symbolic",
                    title="Embedded VTE Unavailable",
                    description=f"Install vte4 (Vte-3.91) for an embedded GTK4 terminal, or open an external terminal ({exc}).",
                )
            )
        return box

    def _copy_active(self) -> None:
        if self._vte_term is not None:
            gi.require_version("Vte", "3.91")
            from gi.repository import Vte

            self._vte_term.copy_clipboard_format(Vte.Format.TEXT)

    def _paste_vte(self) -> None:
        if self._vte_term is not None:
            self._vte_term.paste_clipboard()

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
            self._copy_active()
            return True
        if ctrl and shift and keyval in (Gdk.KEY_V, Gdk.KEY_v):
            self._paste_vte()
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
