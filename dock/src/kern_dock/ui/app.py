"""Kern Dock GTK4 + Libadwaita 1.8 Application Shell.

Native 2-pane GTK4 layout:
  • Left Pane: Full-scale interactive 3D radio handset with a GNOME HIG device connection
    popover, Mini-Dock toggle, and compact bottom hardware icon strip.
  • Right Pane: Native Adw.TabView + Adw.TabBar with a pinned non-closable Home tab,
    closable tool & plugin tabs opened from Home or the '+' tab button, and top-right
    Preferences (Dock, Plugins, OS, Security) + Primary Menu.
  • Compact Mini-Dock Mode (Ctrl+M): Floating hardware handset only.
"""

from __future__ import annotations

from pathlib import Path
from typing import Any, Callable

import gi

gi.require_version("Gtk", "4.0")
gi.require_version("Gdk", "4.0")
gi.require_version("Adw", "1")
from gi.repository import Adw, Gdk, Gio, GLib, Gtk  # noqa: E402

from .. import APP_ID, APP_NAME, ASSETS_DIR
from ..core.events import Event
from ..core.options import DockOptions
from ..core.registry import ActionSpec, PageSpec
from ..device.keys import APP_SHORTCUTS
from ..device.link import REBOOT_BOOTLOADER, REBOOT_FIRMWARE, REBOOT_MODES, REBOOT_RECOVERY
from ..system.icons import make_radio_image, register_icon_theme_paths
from .runtime import DockRuntime
from .views import AppsView, BackupsView, CaptureView, FilesView, FirmwareView, HardwareView, OverviewView, TerminalView
from .widgets.command_palette import CommandPaletteDialog
from .widgets.dialogs import show_about_dialog, show_preferences_dialog, show_shortcuts_dialog
from .widgets.handset_stage import HandsetStage
from .widgets.keyboard_bridge import KeyboardBridge
from .widgets.util import battery_icon_name


class KernDockApp(Adw.Application):
    """Compact, native GTK4 + Libadwaita 1.8 tabbed shell bound to DockRuntime."""

    def __init__(self, opts: DockOptions) -> None:
        super().__init__(application_id=APP_ID, flags=Gio.ApplicationFlags.NON_UNIQUE)
        self.opts = opts
        self.runtime = DockRuntime(opts)
        self.runtime.events.set_main_dispatcher(GLib.idle_add)
        self.runtime.notifier.bind_application(self)

        self.win: Adw.ApplicationWindow | None = None
        self._page_widgets: dict[str, Gtk.Widget] = {}
        self._open_tabs: dict[str, Adw.TabPage] = {}
        self._tab_to_id: dict[Adw.TabPage, str] = {}
        self._mini_mode = opts.mini or bool(self.runtime.config.get("ui.mini_mode", False))
        self._handset_visible = bool(self.runtime.config.get("ui.show_handset", True))
        self._quitting = False
        self._updating_nav = False

    def do_activate(self) -> None:
        if self.win is not None:
            self.win.present()
            return

        display = Gdk.Display.get_default()
        register_icon_theme_paths(display)
        self._apply_color_scheme(str(self.runtime.config.get("ui.color_scheme", "dark")))

        css = Gtk.CssProvider()
        css.load_from_path(str(ASSETS_DIR / "style.css"))
        Gtk.StyleContext.add_provider_for_display(display, css, Gtk.STYLE_PROVIDER_PRIORITY_APPLICATION)

        theme = Gtk.IconTheme.get_for_display(display)
        if not all(theme.has_icon(n) for n in ("computer-symbolic", "network-wireless-symbolic", "input-keyboard-symbolic")):
            Gtk.Settings.get_default().set_property("gtk-icon-theme-name", "Adwaita")

        self.win = Adw.ApplicationWindow(application=self, title=APP_NAME)
        self.win.set_icon_name(APP_ID)
        self.win.set_size_request(380, 600)

        self.runtime.on_present_window = lambda: GLib.idle_add(self._present_window)
        self.runtime.on_hide_window = lambda: GLib.idle_add(lambda: (self.win.set_visible(False), False)[1])
        self.runtime.on_toggle_mini = lambda: GLib.idle_add(lambda: (self._set_mini_mode(not self._mini_mode), False)[1])
        self.runtime.on_navigate_page = lambda pid: GLib.idle_add(lambda: (self._open_tab(pid), False)[1])
        self.runtime.on_quit = lambda: GLib.idle_add(self._force_quit)

        # --- Build Core Stage & Keyboard Controller ---
        self.stage = HandsetStage(
            self.runtime,
            on_toast=self._toast,
            on_drop_file=self._handle_dropped_file,
            on_toggle_keyboard=lambda: self.keyboard.toggle_chip(),
            on_record=lambda: self.capture_view.toggle_record(),
            on_snapshot=lambda: self.capture_view.save_screenshot(),
        )
        self.keyboard = KeyboardBridge(self.win, self.runtime, self.stage, on_toast=self._toast)

        # --- Build Core Modular Views ---
        self.overview_view = OverviewView(
            self.win,
            self.runtime,
            on_toast=self._toast,
            on_open_tab=self._open_tab,
            on_confirm_reboot=self._confirm_reboot,
            on_leave_demo_bl=self._leave_demo_bootloader,
        )
        self.hardware_view = HardwareView(
            self.win,
            self.runtime,
            on_toast=self._toast,
            on_confirm=self._confirm,
        )
        self.files_view = FilesView(
            self.win,
            self.runtime,
            on_toast=self._toast,
            on_confirm=self._confirm,
        )
        self.firmware_view = FirmwareView(
            self.win,
            self.runtime,
            on_toast=self._toast,
            on_confirm=self._confirm,
            on_confirm_reboot=self._confirm_reboot,
            on_progress=self._set_progress,
            on_leave_demo_bl=self._leave_demo_bootloader,
            on_stage_redraw=lambda: self.stage.area.queue_draw(),
            on_open_tab=self._open_tab,
        )
        self.apps_view = AppsView(
            self.win,
            self.runtime,
            on_toast=self._toast,
            on_confirm=self._confirm,
            on_progress=self._set_progress,
        )
        self.backups_view = BackupsView(
            self.win,
            self.runtime,
            on_toast=self._toast,
            on_confirm=self._confirm,
            on_progress=self._set_progress,
        )
        self.capture_view = CaptureView(
            self.win,
            self.runtime,
            self.stage,
            on_toast=self._toast,
            on_recording_changed=self._on_recording_changed,
        )
        self.terminal_view = TerminalView(self.runtime, on_toast=self._toast)

        self.stage.on_rec_event = self.capture_view.capture_event
        self.stage.playback_override = lambda: self.capture_view.playback_frame_if_active(self._active_tab_id() == "capture")

        # --- Register Core Declarative Pages ---
        for spec in (
            PageSpec("overview", "Home", "go-home-symbolic", "Workspace", lambda: self.overview_view.widget, order=10, subtitle="Device status, identity, security & tools"),
            PageSpec("hardware", "Hardware", "preferences-other-symbolic", "Workspace", lambda: self.hardware_view.widget, order=20, subtitle="SWD probe, USB power & raw serial control (works offline)"),
            PageSpec("files", "Files", "folder-symbolic", "Workspace", lambda: self.files_view.widget, order=30, subtitle="KernFS browser, colored hex viewer & WebDAV"),
            PageSpec("firmware", "Flasher", "application-x-firmware-symbolic", "Workspace", lambda: self.firmware_view.widget, order=40, subtitle="Bootloader, GitHub releases & .kfw/.bin flasher"),
            PageSpec("apps", "Apps", "application-x-executable-symbolic", "Workspace", lambda: self.apps_view.widget, order=45, subtitle="Install, inspect manifests, build & manage radio apps"),
            PageSpec("backups", "Backups", "drive-harddisk-symbolic", "Workspace", lambda: self.backups_view.widget, order=48, subtitle=".kfw / .zip / calibration snapshots & hex viewer"),
            PageSpec("terminal", "Terminal", "utilities-terminal-symbolic", "Tools", lambda: self.terminal_view.widget, order=50, subtitle="Embedded VTE shell & RigWire console"),
            PageSpec("capture", "Capture", "camera-photo-symbolic", "Tools", lambda: self.capture_view.widget, order=60, subtitle="PNG snapshots & GIF timeline editor"),
        ):
            self.runtime.pages.register(spec)

        # --- Assemble Compact 2-Pane Tabbed GTK4 Shell ---
        self._build_shell()
        self._register_core_actions()

        self.runtime.start_services()
        self.runtime.pages.on_change(lambda: GLib.idle_add(lambda: (self._sync_pages(), False)[1]))
        self._sync_pages()

        # Open pinned non-closable Home tab
        self._open_tab("overview", pinned=True)

        # Event subscriptions
        self.runtime.events.subscribe("link.connected", lambda _e: self._refresh_chrome(), main_thread=True)
        self.runtime.events.subscribe("link.disconnected", lambda _e: self._refresh_chrome(), main_thread=True)
        self.runtime.events.subscribe("link.connecting", lambda _e: self.conn_lbl.set_text("Connecting…"), main_thread=True)
        self.runtime.events.subscribe("link.error", self._on_link_error, main_thread=True)
        self.runtime.events.subscribe("link.telemetry", lambda _e: self._refresh_chrome(), main_thread=True)
        self.runtime.events.subscribe("security.changed", lambda _e: self._refresh_chrome(), main_thread=True)
        self.runtime.events.subscribe("ports.changed", self._on_ports_changed_ui, main_thread=True)
        self.runtime.events.subscribe("ui.toast", lambda e: self._toast(str(e.payload.get("message", ""))), main_thread=True)
        self.runtime.events.subscribe("handset.changed", lambda _e: self._sync_window_size(), main_thread=True)

        self.win.connect("close-request", self._on_close_request)
        self._refresh_chrome()
        self._rebuild_device_popover()
        self._set_mini_mode(self._mini_mode, initial=True)
        self._sync_window_size()

        if self.opts.port:
            GLib.timeout_add(70, lambda: (self.runtime.connect_async(self.opts.port or "demo"), False)[1])
        elif self.runtime.auto_connect_usb and any(p.is_quansheng for p in self.runtime.port_monitor.ports):
            first_qs = next(p for p in self.runtime.port_monitor.ports if p.is_quansheng)
            GLib.timeout_add(120, lambda: (self.runtime.connect_async(first_qs.device, first_qs), False)[1])

        if not self.opts.start_hidden:
            self.win.present()

    def _build_shell(self) -> None:
        assert self.win is not None

        # === LEFT PANE: FULL-SCALE INTERACTIVE RADIO HANDSET ===
        self.left_pane = Adw.ToolbarView()
        self.left_pane.add_css_class("handset-pane")
        left_hdr = Adw.HeaderBar(show_end_title_buttons=False)

        # GNOME HIG Radio Connection MenuButton
        self.switcher_btn = Gtk.MenuButton(tooltip_text="Select radio connection")
        self.switcher_btn.add_css_class("flat")
        conn_in = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=6)
        self.conn_dot = Gtk.Box(valign=Gtk.Align.CENTER)
        self.conn_dot.add_css_class("status-dot")
        self.conn_lbl = Gtk.Label(label="No Radio", ellipsize=3, max_width_chars=18)
        self.conn_lbl.add_css_class("heading")
        self.hdr_batt_icon = Gtk.Image(icon_name="battery-missing-symbolic", visible=False)
        conn_in.append(self.conn_dot)
        conn_in.append(self.conn_lbl)
        conn_in.append(self.hdr_batt_icon)
        conn_in.append(Gtk.Image.new_from_icon_name("pan-down-symbolic"))
        self.switcher_btn.set_child(conn_in)
        self.switcher_popover = Gtk.Popover()
        self.switcher_popover.add_css_class("device-popover")
        self.switcher_btn.set_popover(self.switcher_popover)
        left_hdr.set_title_widget(self.switcher_btn)

        self.mini_btn = Gtk.ToggleButton(icon_name="view-restore-symbolic", tooltip_text="Mini-Dock Mode (Ctrl+M)")
        self.mini_btn.connect("toggled", lambda b: None if self._updating_nav else self._set_mini_mode(b.get_active()))
        left_hdr.pack_end(self.mini_btn)

        self.left_pane.add_top_bar(left_hdr)
        self.left_stage_slot = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, vexpand=True)
        self.left_stage_slot.append(self.stage)
        self.left_pane.set_content(self.left_stage_slot)

        # === RIGHT PANE: LEFT-ALIGNED TOP TABS + CLEAN HAMBURGER MENU ON TOP RIGHT ===
        ws_view = Adw.ToolbarView()
        ws_view.set_hexpand(True)

        self.tab_view = Adw.TabView(hexpand=True, vexpand=True)
        self.tab_view.connect("close-page", self._on_tab_close_request)

        # Full-width top TabBar: fixed-width tabs starting flush-left across the whole top bar
        self.tab_bar = Adw.TabBar(view=self.tab_view, autohide=False, expand_tabs=False, hexpand=True)
        self.tab_bar.add_css_class("inline")

        # Only the primary hamburger menu + native window controls on the top right
        menu_b = Gtk.MenuButton(icon_name="open-menu-symbolic", primary=True, tooltip_text="Main Menu")
        menu_b.add_css_class("flat")
        menu_b.update_property([Gtk.AccessibleProperty.LABEL], ["Main Menu"])
        menu_b.set_menu_model(self._build_primary_menu())

        self.hdr_rec_dot = Gtk.Box(valign=Gtk.Align.CENTER, visible=False, tooltip_text="Recording GIF")
        self.hdr_rec_dot.add_css_class("status-dot")
        self.hdr_rec_dot.add_css_class("rec")

        end_box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=6, valign=Gtk.Align.CENTER, margin_start=4, margin_end=6)
        end_box.append(self.hdr_rec_dot)
        end_box.append(menu_b)
        end_box.append(Gtk.WindowControls(side=Gtk.PackType.END))

        top_row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=0, hexpand=True)
        top_row.add_css_class("top-tab-header")
        top_row.append(self.tab_bar)
        top_row.append(end_box)
        self.ws_hdr = Gtk.WindowHandle(child=top_row)

        # Contextual Banner + Progress Bar
        self.dfu_banner = Adw.Banner(revealed=False, button_label="Flash…")
        self.dfu_banner.connect("button-clicked", lambda _b: self._open_tab("firmware"))
        self.op_prog = Gtk.ProgressBar(show_text=True, visible=False, margin_start=12, margin_end=12, margin_top=6)

        ws_body = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, hexpand=True, vexpand=True)
        ws_body.append(self.dfu_banner)
        ws_body.append(self.op_prog)
        ws_body.append(self.tab_view)

        ws_view.add_top_bar(self.ws_hdr)
        ws_view.set_content(ws_body)

        # Clean 2-Pane Split: Left Big Radio + Right Native Tabbed Workspace
        self.full_box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, hexpand=True, vexpand=True)
        self.full_box.append(self.left_pane)
        self.full_box.append(ws_view)

        # Compact Mini-Dock Mode
        self.mini_box = Adw.ToolbarView()
        mini_hdr = Adw.HeaderBar()
        self.mini_title = Adw.WindowTitle(title=APP_NAME, subtitle="Mini-Dock")
        mini_hdr.set_title_widget(self.mini_title)
        expand_btn = Gtk.Button(icon_name="view-fullscreen-symbolic", tooltip_text="Expand Full Dock (Ctrl+M)")
        expand_btn.connect("clicked", lambda _b: self._set_mini_mode(False))
        mini_hdr.pack_start(expand_btn)
        mini_pal = Gtk.Button(icon_name="system-search-symbolic", tooltip_text="Command Palette (Ctrl+P)")
        mini_pal.connect("clicked", lambda _b: self._show_command_palette())
        mini_hdr.pack_end(mini_pal)
        self.mini_box.add_top_bar(mini_hdr)
        self.mini_stage_slot = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, hexpand=True, vexpand=True)
        self.mini_box.set_content(self.mini_stage_slot)

        self.root_stack = Gtk.Stack(transition_type=Gtk.StackTransitionType.CROSSFADE, transition_duration=120)
        self.root_stack.add_named(self.full_box, "full")
        self.root_stack.add_named(self.mini_box, "mini")
        self.root_stack.set_visible_child_name("full")

        self.toast_overlay = Adw.ToastOverlay(child=self.root_stack)
        self.win.set_content(self.toast_overlay)

    # =========================================================================
    # Tab Lifecycle & Page Registry
    # =========================================================================
    def _active_tab_id(self) -> str:
        page = self.tab_view.get_selected_page()
        return self._tab_to_id.get(page, "overview") if page is not None else "overview"

    def _on_tab_close_request(self, tv: Any, page: Adw.TabPage) -> bool:
        pid = self._tab_to_id.get(page)
        if pid == "overview" or page.get_pinned():
            tv.close_page_finish(page, False)
            return True
        if pid:
            self._open_tabs.pop(pid, None)
        self._tab_to_id.pop(page, None)
        tv.close_page_finish(page, True)
        return True

    def _close_active_tab(self) -> None:
        page = self.tab_view.get_selected_page()
        if page is not None and not page.get_pinned():
            self.tab_view.close_page(page)

    def _open_tab(self, page_id: str, *, pinned: bool = False) -> None:
        if self._mini_mode:
            self._set_mini_mode(False)
        existing = self._open_tabs.get(page_id)
        if existing is not None:
            self.tab_view.set_selected_page(existing)
            return
        spec = self.runtime.pages.get(page_id)
        if spec is None:
            return
        widget = self._page_widgets.get(page_id)
        if widget is None:
            try:
                widget = spec.builder()
            except Exception as exc:
                widget = Adw.StatusPage(icon_name="dialog-error-symbolic", title=spec.title, description=str(exc))
            self._page_widgets[page_id] = widget

        tab = self.tab_view.append_pinned(widget) if (pinned or page_id == "overview") else self.tab_view.append(widget)
        tab.set_title(spec.title)
        tab.set_icon(Gio.ThemedIcon.new(spec.icon))
        self._open_tabs[page_id] = tab
        self._tab_to_id[tab] = page_id
        self.tab_view.set_selected_page(tab)

    def _sync_pages(self) -> None:
        pages = self.runtime.pages.ordered()
        # Close tabs for removed/disabled plugin pages
        valid_ids = {p.id for p in pages}
        for pid in list(self._open_tabs.keys()):
            if pid not in valid_ids and pid != "overview":
                self.tab_view.close_page(self._open_tabs[pid])

        # Refresh Home tab's Tools launcher grid
        self.overview_view.rebuild_tools()

        # Register actions & Ctrl+1..9
        for i, p in enumerate(pages):
            accel = f"<Control>{i + 1}" if i < 9 else None
            self.runtime.actions.register(
                ActionSpec(
                    id=f"nav.{p.id}",
                    title=f"Open {p.title}",
                    handler=lambda pid=p.id: self._open_tab(pid),
                    category="Tabs",
                    subtitle=p.subtitle or p.section,
                    accel=accel,
                    icon=p.icon,
                )
            )
            if accel:
                self._bind_gtk_action(f"page-{i + 1}", lambda pid=p.id: self._open_tab(pid), accel)

    def _register_core_actions(self) -> None:
        handlers: dict[str, tuple[str, str, Callable[[], None], bool]] = {
            "command-palette": ("Application", "system-search-symbolic", self._show_command_palette, False),
            "type-text": ("Radio", "input-keyboard-symbolic", self.keyboard.show_type_dialog, True),
            "live-typing": ("Radio", "input-keyboard-symbolic", lambda: self.keyboard.set_live_typing(not self.keyboard.live_typing), True),
            "toggle-keyboard": ("Radio", "input-keyboard-symbolic", lambda: self.keyboard.set_enabled(not self.keyboard.enabled), False),
            "toggle-handset": ("Layout", "sidebar-show-symbolic", lambda: self._set_handset_visible(not self._handset_visible), False),
            "toggle-mini": ("Layout", "view-restore-symbolic", lambda: self._set_mini_mode(not self._mini_mode), False),
            "open-terminal": ("System", "utilities-terminal-symbolic", self._open_external_terminal, False),
            "screenshot": ("Capture", "camera-photo-symbolic", self.capture_view.save_screenshot, False),
            "toggle-record": ("Capture", "media-record-symbolic", self.capture_view.toggle_record, False),
            "preferences": ("Application", "preferences-system-symbolic", self._show_preferences, False),
            "shortcuts": ("Application", "input-keyboard-symbolic", lambda: show_shortcuts_dialog(self.win), False),
            "quit": ("Application", "application-exit-symbolic", self._force_quit, False),
        }
        for name, desc, accel in APP_SHORTCUTS:
            cat, icon, fn, mut = handlers[name]
            self._bind_gtk_action(name, fn, accel)
            self.runtime.actions.register(
                ActionSpec(id=name, title=desc, handler=fn, category=cat, accel=accel, icon=icon, mutates=mut)
            )

        self._bind_gtk_action("close-tab", self._close_active_tab, "<Control>w")
        self.runtime.actions.register(
            ActionSpec(id="close-tab", title="Close Active Tab", handler=self._close_active_tab, category="Tabs", accel="<Control>w", icon="window-close-symbolic")
        )

        extra_actions: list[tuple[str, str, str, str, Callable[[], None], bool]] = [
            ("connect-demo", "Connect Demo Radio", "Radio", "org.kernos.Dock-symbolic", lambda: self.runtime.connect_async("demo"), True),
            ("connect-qemu", "Connect QEMU Emulator", "Radio", "computer-symbolic", lambda: self.runtime.connect_async("tcp://127.0.0.1:4446"), True),
            ("disconnect", "Disconnect Radio", "Radio", "media-eject-symbolic", lambda: self.runtime.link.disconnect(), True),
            ("mount-drive", "Mount / Unmount Radio Drive", "Files", "drive-harddisk-symbolic", self._toggle_mount_from_palette, True),
            ("reboot", "Restart Radio", "Hardware", "system-reboot-symbolic", lambda: self._confirm_reboot(REBOOT_FIRMWARE), True),
            ("reboot-recovery", "Restart into Safe Recovery", "Hardware", "applications-engineering-symbolic", lambda: self._confirm_reboot(REBOOT_RECOVERY), True),
            ("reboot-bootloader", "Restart into Bootloader", "Hardware", "application-x-firmware-symbolic", lambda: self._confirm_reboot(REBOOT_BOOTLOADER), True),
            ("about", "About Kern Dock", "Application", "help-about-symbolic", lambda: show_about_dialog(self.win), False),
        ]
        for aid, title, cat, icon, fn, mut in extra_actions:
            self._bind_gtk_action(aid, fn, None)
            self.runtime.actions.register(ActionSpec(id=aid, title=title, handler=fn, category=cat, icon=icon, mutates=mut))

    def _bind_gtk_action(self, name: str, cb: Callable[[], None], accel: str | None) -> None:
        if self.lookup_action(name) is None:
            act = Gio.SimpleAction.new(name, None)
            act.connect("activate", lambda _a, _p: cb())
            self.add_action(act)
        if accel:
            self.set_accels_for_action(f"app.{name}", [accel])

    def _build_primary_menu(self) -> Gio.Menu:
        menu = Gio.Menu()
        s1 = Gio.Menu()
        s1.append("Command Palette…", "app.command-palette")
        s1.append("Type Text on Radio…", "app.type-text")
        s1.append("Open External Terminal", "app.open-terminal")
        menu.append_section(None, s1)

        s2 = Gio.Menu()
        s2.append("Restart Radio", "app.reboot")
        s2.append("Safe Recovery", "app.reboot-recovery")
        s2.append("Enter Bootloader", "app.reboot-bootloader")
        menu.append_section("Hardware", s2)

        s3 = Gio.Menu()
        s3.append("Preferences", "app.preferences")
        s3.append("Keyboard Shortcuts", "app.shortcuts")
        s3.append("About Kern Dock", "app.about")
        s3.append("Quit", "app.quit")
        menu.append_section(None, s3)
        return menu

    # =========================================================================
    # GNOME HIG Device Connection Popover
    # =========================================================================
    def _rebuild_device_popover(self) -> None:
        root = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=10, margin_top=12, margin_bottom=12, margin_start=12, margin_end=12)
        root.set_size_request(290, -1)

        lk = self.runtime.link

        # Active connection card (if connected)
        if lk.connected:
            active_lb = Gtk.ListBox(selection_mode=Gtk.SelectionMode.NONE)
            active_lb.add_css_class("boxed-list")
            sub = "Demo Radio" if lk.demo_mode else (lk.port or "Connected")
            if lk.link_mode == "stock_bl":
                sub = f"{sub} · Bootloader"
            arow = Adw.ActionRow(
                title=GLib.markup_escape_text(lk.volume_name),
                subtitle=GLib.markup_escape_text(sub),
            )
            arow.add_prefix(make_radio_image(20, color=True))
            disc = Gtk.Button(icon_name="media-eject-symbolic", valign=Gtk.Align.CENTER, tooltip_text="Disconnect")
            disc.add_css_class("flat")
            disc.add_css_class("destructive-action")
            disc.connect("clicked", lambda _b: (self.switcher_popover.popdown(), lk.disconnect()))
            arow.add_suffix(disc)
            active_lb.append(arow)
            root.append(active_lb)

        # USB Radios section (with inline Rescan button)
        usb_hdr = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=6)
        usb_lbl = Gtk.Label(label="USB Radios", xalign=0, hexpand=True)
        usb_lbl.add_css_class("caption-heading")
        usb_lbl.add_css_class("dim-label")
        rescan = Gtk.Button(icon_name="view-refresh-symbolic", tooltip_text="Rescan USB Ports")
        rescan.add_css_class("flat")
        rescan.connect("clicked", lambda _b: (self.runtime.port_monitor.scan(), self._rebuild_device_popover()))
        usb_hdr.append(usb_lbl)
        usb_hdr.append(rescan)
        root.append(usb_hdr)

        usb_lb = Gtk.ListBox(selection_mode=Gtk.SelectionMode.NONE)
        usb_lb.add_css_class("boxed-list")
        ports = self.runtime.port_monitor.ports
        if not ports:
            empty = Adw.ActionRow(title="No USB radio found", subtitle="Plug in via USB-C or cable")
            empty.add_prefix(Gtk.Image(icon_name="network-wired-symbolic"))
            empty.set_sensitive(False)
            usb_lb.append(empty)
        else:
            for p in ports:
                is_cur = lk.connected and not lk.demo_mode and lk.port == p.device
                row = Adw.ActionRow(
                    title=GLib.markup_escape_text(p.pretty_name),
                    subtitle=GLib.markup_escape_text(f"{p.device} · {'Bootloader' if p.mode_hint == 'stock_bl' else 'USB Serial'}"),
                    activatable=not is_cur,
                )
                row.add_prefix(make_radio_image(18) if p.is_quansheng else Gtk.Image(icon_name="network-wired-symbolic"))
                if is_cur:
                    chk = Gtk.Image(icon_name="object-select-symbolic")
                    chk.add_css_class("accent")
                    row.add_suffix(chk)
                else:
                    row.add_suffix(Gtk.Image(icon_name="go-next-symbolic"))
                    row.connect("activated", lambda _r, po=p: (self.switcher_popover.popdown(), self.runtime.connect_async(po.device, po)))
                usb_lb.append(row)
        root.append(usb_lb)

        # Virtual & Emulators section
        virt_lbl = Gtk.Label(label="Virtual", xalign=0)
        virt_lbl.add_css_class("caption-heading")
        virt_lbl.add_css_class("dim-label")
        root.append(virt_lbl)

        virt_lb = Gtk.ListBox(selection_mode=Gtk.SelectionMode.NONE)
        virt_lb.add_css_class("boxed-list")

        is_demo = lk.connected and lk.demo_mode
        demo_row = Adw.ActionRow(title="Demo Radio", subtitle="Built-in offline simulator", activatable=not is_demo)
        demo_row.add_prefix(Gtk.Image(icon_name="org.kernos.Dock-symbolic"))
        if is_demo:
            chk = Gtk.Image(icon_name="object-select-symbolic")
            chk.add_css_class("accent")
            demo_row.add_suffix(chk)
        else:
            demo_row.add_suffix(Gtk.Image(icon_name="go-next-symbolic"))
            demo_row.connect("activated", lambda _r: (self.switcher_popover.popdown(), self.runtime.connect_async("demo")))
        virt_lb.append(demo_row)

        is_qemu = lk.connected and lk.port == "tcp://127.0.0.1:4446"
        qemu_row = Adw.ActionRow(title="QEMU Emulator", subtitle="tcp://127.0.0.1:4446", activatable=not is_qemu)
        qemu_row.add_prefix(Gtk.Image(icon_name="computer-symbolic"))
        if is_qemu:
            chk = Gtk.Image(icon_name="object-select-symbolic")
            chk.add_css_class("accent")
            qemu_row.add_suffix(chk)
        else:
            qemu_row.add_suffix(Gtk.Image(icon_name="go-next-symbolic"))
            qemu_row.connect("activated", lambda _r: (self.switcher_popover.popdown(), self.runtime.connect_async("tcp://127.0.0.1:4446")))
        virt_lb.append(qemu_row)
        root.append(virt_lb)

        self.switcher_popover.set_child(root)

    def _set_handset_visible(self, visible: bool) -> None:
        self._handset_visible = visible
        self.runtime.config.set("ui.show_handset", visible)
        if not self._mini_mode:
            self.left_pane.set_visible(visible)
        self._sync_window_size()

    def _set_mini_mode(self, mini: bool, *, initial: bool = False) -> None:
        self._mini_mode = mini
        if not initial:
            self.runtime.config.set("ui.mini_mode", mini)
        parent = self.stage.get_parent()
        if mini:
            if parent is self.left_stage_slot:
                self.left_stage_slot.remove(self.stage)
            if self.stage.get_parent() is not self.mini_stage_slot:
                self.mini_stage_slot.append(self.stage)
            self.stage.set_visible(True)
            self.root_stack.set_visible_child_name("mini")
        else:
            if parent is self.mini_stage_slot:
                self.mini_stage_slot.remove(self.stage)
            if self.stage.get_parent() is not self.left_stage_slot:
                self.left_stage_slot.append(self.stage)
            self.left_pane.set_visible(self._handset_visible)
            self.stage.set_visible(True)
            self.root_stack.set_visible_child_name("full")

        self._updating_nav = True
        self.mini_btn.set_active(mini)
        self._updating_nav = False
        self._sync_window_size()

    def _sync_window_size(self) -> None:
        if self.win is None:
            return
        lw, lh = self.stage.sync_geometry()
        if self._mini_mode:
            self.win.set_default_size(lw + 28, lh + 130)
        else:
            left_w = (lw + 24) if self._handset_visible else 0
            self.win.set_default_size(left_w + 680, max(lh + 128, 700))

    def _refresh_chrome(self) -> None:
        lk = self.runtime.link
        sec = self.runtime.security

        for cls in ("online", "headless", "dfu", "hardened"):
            self.conn_dot.remove_css_class(cls)

        if not lk.connected:
            self.conn_lbl.set_text("No Radio")
            self.hdr_batt_icon.set_visible(False)
            self.dfu_banner.set_revealed(False)
            if self.win:
                self.win.set_title(APP_NAME)
            self.mini_title.set_subtitle("No Radio")
        elif lk.link_mode == "stock_bl":
            ver = lk.bootloader_info.get("version", "BL")
            self.conn_dot.add_css_class("dfu")
            self.conn_lbl.set_text(f"Bootloader {ver}")
            self.hdr_batt_icon.set_visible(False)
            self.dfu_banner.set_title(f"Bootloader {ver} ready on {lk.port}")
            self.dfu_banner.set_revealed(True)
            if self.win:
                self.win.set_title(f"{APP_NAME} — Bootloader {ver}")
            self.mini_title.set_subtitle(f"Bootloader {ver}")
        else:
            short = lk.volume_name.removeprefix("Quansheng ")
            self.conn_lbl.set_text(short)
            if sec.hardened:
                self.conn_dot.add_css_class("hardened")
            elif lk.headless:
                self.conn_dot.add_css_class("headless")
            else:
                self.conn_dot.add_css_class("online")
            pct = lk.last_telemetry.get("battery_pct")
            self.hdr_batt_icon.set_visible(pct is not None)
            if pct is not None:
                self.hdr_batt_icon.set_from_icon_name(battery_icon_name(pct))
                self.hdr_batt_icon.set_tooltip_text(f"Battery {pct}%")
            self.dfu_banner.set_revealed(False)
            if self.win:
                self.win.set_title(f"{APP_NAME} — {lk.volume_name}")
            hz = int(lk.radio_state.get("rx_freq_hz") or 0)
            freq_s = f"{hz / 1e6:.3f} MHz" if hz else (lk.port or lk.firmware_name)
            self.mini_title.set_subtitle(f"{short} · {freq_s}")

        self._rebuild_device_popover()

    def _on_ports_changed_ui(self, _ev: Event) -> None:
        self._rebuild_device_popover()
        if self.runtime.auto_connect_usb and not self.runtime.link.connected and not self.runtime._connecting:
            for p in self.runtime.port_monitor.ports:
                if p.is_quansheng:
                    GLib.timeout_add(
                        280,
                        lambda dev=p.device, pi=p: (
                            self.runtime.connect_async(dev, pi) if (not self.runtime.link.connected and not self.runtime._connecting) else None,
                            False,
                        )[1],
                    )
                    break

    def _on_link_error(self, ev: Event) -> None:
        self._refresh_chrome()
        self._toast(f"Connection failed: {ev.payload.get('error', 'unknown error')}")

    def _handle_dropped_file(self, path: Path) -> None:
        ext = path.suffix.lower()
        if ext in (".kfw", ".bin"):
            # Inspect whether .kfw is a backup or firmware image
            if ext == ".kfw":
                try:
                    from ..device.firmware import inspect_backup_file

                    binfo = inspect_backup_file(path)
                    if not any(e["path"] == "/firmware.bin" for e in binfo["entries"]):
                        self._open_tab("backups")
                        self.backups_view.load_backup_file(path)
                        return
                except Exception:
                    pass
            self._open_tab("firmware")
            self.firmware_view.load_firmware_path(path)
        elif ext == ".zip":
            self._open_tab("backups")
            self.backups_view.load_backup_file(path)
        elif ext in (".krx", ".mod") or path.name.lower() == "krx.toml" or (path.is_dir() and (path / "krx.toml").is_file()):
            self._open_tab("apps")
            self.apps_view.load_package_path(path)
        else:
            self._open_tab("files")
            self.files_view.upload_local_file(path)

    def _confirm_reboot(self, mode: int) -> None:
        lk = self.runtime.link
        if not (lk.connected and lk.link_mode == "rigwire"):
            self._toast("Connect a RigWire radio first")
            return
        if mode == REBOOT_BOOTLOADER and self.runtime.security.hardened:
            self._toast("Hardened Mode blocks bootloader entry")
            return
        heading, body, verb = {
            REBOOT_FIRMWARE: ("Restart Radio?", "The radio restarts and Dock reconnects once USB re-enumerates.", "Restart"),
            REBOOT_RECOVERY: ("Restart into Safe Recovery?", "Boots the minimal recovery shell; normal apps stay off until reboot.", "Restart"),
            REBOOT_BOOTLOADER: ("Restart into Bootloader?", "Leaves firmware runtime for flashing. LCD stays dark until flashed or power cycled.", "Enter Bootloader"),
        }[mode]
        self._confirm(heading, body, verb, lambda: self._do_reboot(mode), destructive=(mode == REBOOT_BOOTLOADER))

    def _do_reboot(self, mode: int) -> None:
        self.keyboard.cancel_typing()
        self.stage.release_all_keys()
        port = self.runtime.link.port
        demo = self.runtime.link.demo_mode
        try:
            self.runtime.link.reboot(mode)
        except Exception as exc:
            self._toast(str(exc))
            return
        if demo:
            if mode == REBOOT_BOOTLOADER:
                self._open_tab("firmware")
            self._toast(REBOOT_MODES[mode][0].replace("Restarting", "Restarted") + " (demo)")
            return
        self._toast(REBOOT_MODES[mode][0] + "…")
        if port and not port.startswith("tcp://"):
            GLib.timeout_add(
                2200,
                lambda: (self.runtime.connect_async(port) if (not self.runtime.link.connected and not self.runtime._connecting) else None, False)[1],
            )

    def _leave_demo_bootloader(self) -> None:
        self.runtime.link.demo_power_cycle()

    def _toggle_mount_from_palette(self) -> None:
        self._open_tab("files")
        self.files_view.mount_btn.set_active(not self.files_view.mount_btn.get_active())

    def _open_external_terminal(self) -> None:
        try:
            term = self.runtime.open_external_terminal()
            self._toast(f"Opened {term}")
        except Exception as exc:
            self._toast(str(exc))

    def _show_command_palette(self) -> None:
        dlg = CommandPaletteDialog(self.runtime.actions)
        dlg.present(self.win)
        dlg.search.grab_focus()

    def _show_preferences(self) -> None:
        show_preferences_dialog(self.win, self.runtime, on_toast=self._toast, on_scheme=self._apply_color_scheme)

    def _apply_color_scheme(self, scheme: str) -> None:
        sm = Adw.StyleManager.get_default()
        if scheme == "light":
            sm.set_color_scheme(Adw.ColorScheme.FORCE_LIGHT)
        elif scheme == "system":
            sm.set_color_scheme(Adw.ColorScheme.DEFAULT)
        else:
            sm.set_color_scheme(Adw.ColorScheme.PREFER_DARK)

    def _on_recording_changed(self, recording: bool) -> None:
        self.hdr_rec_dot.set_visible(recording)
        self.stage.set_recording(recording)

    def _set_progress(self, visible: bool, frac: float, text: str) -> None:
        self.op_prog.set_visible(visible)
        self.op_prog.set_fraction(max(0.0, min(1.0, frac)))
        self.op_prog.set_text(text)

    def _toast(self, msg: str) -> None:
        t = Adw.Toast.new(msg)
        t.set_timeout(2)
        self.toast_overlay.add_toast(t)

    def _confirm(self, heading: str, body: str, verb: str, on_ok: Callable[[], None], destructive: bool = True) -> None:
        dlg = Adw.AlertDialog(heading=heading, body=body)
        dlg.add_response("cancel", "Cancel")
        dlg.add_response("ok", verb)
        dlg.set_response_appearance("ok", Adw.ResponseAppearance.DESTRUCTIVE if destructive else Adw.ResponseAppearance.SUGGESTED)
        dlg.set_default_response("cancel")
        dlg.set_close_response("cancel")
        dlg.connect("response", lambda _d, r: on_ok() if r == "ok" else None)
        dlg.present(self.win)

    def _present_window(self) -> bool:
        if self.win is not None:
            self.win.set_visible(True)
            self.win.present()
        return False

    def _force_quit(self) -> bool:
        self._quitting = True
        self.capture_view.stop_playback()
        self.hardware_view.shutdown()
        self.runtime.shutdown()
        if self.win is not None:
            self.win.close()
        self.quit()
        return False

    def _on_close_request(self, win: Any) -> bool:
        if not self._quitting and self.runtime.tray.active and bool(self.runtime.config.get("os.close_to_tray", False)):
            win.set_visible(False)
            self.runtime.notifier.send(APP_NAME, "Minimized to system tray — click the radio icon to restore", notif_id="kern-dock-tray")
            return True
        self.capture_view.stop_playback()
        self.hardware_view.shutdown()
        self.runtime.shutdown()
        return False


def launch(opts: DockOptions) -> int:
    return int(KernDockApp(opts).run([]))
