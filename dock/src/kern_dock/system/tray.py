"""Pure GLib/Gio D-Bus System Tray (`org.kde.StatusNotifierItem` + `com.canonical.dbusmenu`).

Works natively with GTK4 + Libadwaita (zero GTK3/AppIndicator conflicts) across GNOME Shell
AppIndicator extension, KDE Plasma, Hyprland/Waybar, Sway, XFCE, MATE and Cinnamon.
"""

from __future__ import annotations

import os
from typing import Any, Callable

from .. import APP_ID, APP_NAME
from .icons import make_tray_argb_pixmap

_SNI_XML = """
<node>
  <interface name="org.kde.StatusNotifierItem">
    <property name="Category" type="s" access="read"/>
    <property name="Id" type="s" access="read"/>
    <property name="Title" type="s" access="read"/>
    <property name="Status" type="s" access="read"/>
    <property name="WindowId" type="i" access="read"/>
    <property name="IconName" type="s" access="read"/>
    <property name="IconPixmap" type="a(iiay)" access="read"/>
    <property name="ToolTip" type="(sa(iiay)ss)" access="read"/>
    <property name="ItemIsMenu" type="b" access="read"/>
    <property name="Menu" type="o" access="read"/>
    <method name="Activate">
      <arg name="x" type="i" direction="in"/>
      <arg name="y" type="i" direction="in"/>
    </method>
    <method name="SecondaryActivate">
      <arg name="x" type="i" direction="in"/>
      <arg name="y" type="i" direction="in"/>
    </method>
    <method name="ContextMenu">
      <arg name="x" type="i" direction="in"/>
      <arg name="y" type="i" direction="in"/>
    </method>
    <signal name="NewTitle"/>
    <signal name="NewIcon"/>
    <signal name="NewStatus"><arg type="s"/></signal>
    <signal name="NewToolTip"/>
  </interface>
</node>
"""

_DBUSMENU_XML = """
<node>
  <interface name="com.canonical.dbusmenu">
    <property name="Version" type="u" access="read"/>
    <property name="TextDirection" type="s" access="read"/>
    <property name="Status" type="s" access="read"/>
    <property name="IconThemePath" type="as" access="read"/>
    <method name="GetLayout">
      <arg name="parentId" type="i" direction="in"/>
      <arg name="recursionDepth" type="i" direction="in"/>
      <arg name="propertyNames" type="as" direction="in"/>
      <arg name="revision" type="u" direction="out"/>
      <arg name="layout" type="(ia{sv}av)" direction="out"/>
    </method>
    <method name="GetGroupProperties">
      <arg name="ids" type="ai" direction="in"/>
      <arg name="propertyNames" type="as" direction="in"/>
      <arg name="properties" type="a(ia{sv})" direction="out"/>
    </method>
    <method name="GetProperty">
      <arg name="id" type="i" direction="in"/>
      <arg name="name" type="s" direction="in"/>
      <arg name="value" type="v" direction="out"/>
    </method>
    <method name="Event">
      <arg name="id" type="i" direction="in"/>
      <arg name="eventId" type="s" direction="in"/>
      <arg name="data" type="v" direction="in"/>
      <arg name="timestamp" type="u" direction="in"/>
    </method>
    <method name="EventGroup">
      <arg name="events" type="a(isvu)" direction="in"/>
      <arg name="idErrors" type="ai" direction="out"/>
    </method>
    <method name="AboutToShow">
      <arg name="id" type="i" direction="in"/>
      <arg name="needUpdate" type="b" direction="out"/>
    </method>
    <method name="AboutToShowGroup">
      <arg name="ids" type="ai" direction="in"/>
      <arg name="updatesNeeded" type="ai" direction="out"/>
      <arg name="idErrors" type="ai" direction="out"/>
    </method>
    <signal name="LayoutUpdated">
      <arg name="revision" type="u"/>
      <arg name="parent" type="i"/>
    </signal>
  </interface>
</node>
"""


class SystemTrayService:
    """Exports a GTK4-safe StatusNotifierItem with live radio pixmap and dynamic D-Bus menu."""

    SNI_PATH = "/StatusNotifierItem"
    MENU_PATH = "/MenuBar"

    def __init__(
        self,
        *,
        on_activate: Callable[[], None],
        menu_provider: Callable[[], list[dict[str, Any]]],
    ) -> None:
        self._on_activate = on_activate
        self._menu_provider = menu_provider
        self._conn: Any = None
        self._bus_name: str = f"org.kde.StatusNotifierItem-{os.getpid()}-1"
        self._name_owner_id: int = 0
        self._sni_reg_id: int = 0
        self._menu_reg_id: int = 0
        self._watcher_id: int = 0
        self._revision: int = 1
        self._state: str = "offline"
        self._tooltip_title: str = APP_NAME
        self._tooltip_body: str = "No radio connected"
        self._handlers: dict[int, Callable[[], None]] = {}
        self._pixmap_cache: dict[str, list[tuple[int, int, bytes]]] = {}
        self.active: bool = False
        self.watcher_present: bool = False

    def start(self) -> bool:
        if self.active:
            return True
        try:
            from gi.repository import Gio, GLib
        except Exception:
            return False
        try:
            self._conn = Gio.bus_get_sync(Gio.BusType.SESSION, None)
            sni_info = Gio.DBusNodeInfo.new_for_xml(_SNI_XML)
            menu_info = Gio.DBusNodeInfo.new_for_xml(_DBUSMENU_XML)
            self._sni_reg_id = self._conn.register_object(
                self.SNI_PATH,
                sni_info.interfaces[0],
                self._on_sni_call,
                self._on_sni_get_prop,
                None,
            )
            self._menu_reg_id = self._conn.register_object(
                self.MENU_PATH,
                menu_info.interfaces[0],
                self._on_menu_call,
                self._on_menu_get_prop,
                None,
            )
            self._name_owner_id = Gio.bus_own_name_on_connection(
                self._conn,
                self._bus_name,
                Gio.BusNameOwnerFlags.NONE,
                lambda *_a: self._register_with_watcher(),
                None,
            )
            self._watcher_id = Gio.bus_watch_name_on_connection(
                self._conn,
                "org.kde.StatusNotifierWatcher",
                Gio.BusNameWatcherFlags.NONE,
                lambda *_a: self._register_with_watcher(),
                lambda *_a: setattr(self, "watcher_present", False),
            )
            self.active = True
            return True
        except Exception:
            self.stop()
            return False

    def stop(self) -> None:
        self.active = False
        self.watcher_present = False
        try:
            from gi.repository import Gio
        except Exception:
            return
        if self._watcher_id:
            try:
                Gio.bus_unwatch_name(self._watcher_id)
            except Exception:
                pass
            self._watcher_id = 0
        if self._name_owner_id:
            try:
                Gio.bus_unown_name(self._name_owner_id)
            except Exception:
                pass
            self._name_owner_id = 0
        if self._conn is not None:
            for rid in (self._sni_reg_id, self._menu_reg_id):
                if rid:
                    try:
                        self._conn.unregister_object(rid)
                    except Exception:
                        pass
            self._sni_reg_id = 0
            self._menu_reg_id = 0
            self._conn = None

    def _register_with_watcher(self) -> None:
        if self._conn is None:
            return
        try:
            from gi.repository import Gio, GLib

            self._conn.call(
                "org.kde.StatusNotifierWatcher",
                "/StatusNotifierWatcher",
                "org.kde.StatusNotifierWatcher",
                "RegisterStatusNotifierItem",
                GLib.Variant("(s)", (self._bus_name,)),
                None,
                Gio.DBusCallFlags.NONE,
                2000,
                None,
                self._on_registered,
            )
        except Exception:
            pass

    def _on_registered(self, _conn: Any, res: Any) -> None:
        try:
            self._conn.call_finish(res)
            self.watcher_present = True
        except Exception:
            self.watcher_present = False

    def update(self, *, state: str, title: str, body: str) -> None:
        changed_icon = state != self._state
        changed_tip = (title != self._tooltip_title) or (body != self._tooltip_body)
        self._state = state
        self._tooltip_title = title
        self._tooltip_body = body
        self._revision += 1
        if not self.active or self._conn is None:
            return
        try:
            from gi.repository import GLib

            if changed_icon:
                self._conn.emit_signal(None, self.SNI_PATH, "org.kde.StatusNotifierItem", "NewIcon", None)
                self._conn.emit_signal(
                    None,
                    self.SNI_PATH,
                    "org.kde.StatusNotifierItem",
                    "NewStatus",
                    GLib.Variant("(s)", ("Active" if state != "offline" else "Passive",)),
                )
            if changed_tip:
                self._conn.emit_signal(None, self.SNI_PATH, "org.kde.StatusNotifierItem", "NewToolTip", None)
            self._conn.emit_signal(
                None,
                self.MENU_PATH,
                "com.canonical.dbusmenu",
                "LayoutUpdated",
                GLib.Variant("(ui)", (self._revision, 0)),
            )
        except Exception:
            pass

    def _pixmaps_for_state(self) -> list[tuple[int, int, bytes]]:
        cached = self._pixmap_cache.get(self._state)
        if cached is not None:
            return cached
        try:
            pix = [make_tray_argb_pixmap(24, state=self._state), make_tray_argb_pixmap(32, state=self._state)]
        except Exception:
            pix = []
        self._pixmap_cache[self._state] = pix
        return pix

    def _on_sni_get_prop(self, _conn: Any, _sender: str, _path: str, _iface: str, prop: str) -> Any:
        from gi.repository import GLib

        if prop == "Category":
            return GLib.Variant("s", "Hardware")
        if prop == "Id":
            return GLib.Variant("s", APP_ID)
        if prop == "Title":
            return GLib.Variant("s", APP_NAME)
        if prop == "Status":
            return GLib.Variant("s", "Active")
        if prop == "WindowId":
            return GLib.Variant("i", 0)
        if prop == "IconName":
            return GLib.Variant("s", "")
        if prop == "IconPixmap":
            return GLib.Variant("a(iiay)", self._pixmaps_for_state())
        if prop == "ToolTip":
            return GLib.Variant("(sa(iiay)ss)", ("", [], self._tooltip_title, self._tooltip_body))
        if prop == "ItemIsMenu":
            return GLib.Variant("b", False)
        if prop == "Menu":
            return GLib.Variant("o", self.MENU_PATH)
        return None

    def _on_sni_call(
        self, _conn: Any, _sender: str, _path: str, _iface: str, method: str, _params: Any, inv: Any
    ) -> None:
        if method in ("Activate", "SecondaryActivate"):
            try:
                self._on_activate()
            except Exception:
                pass
        inv.return_value(None)

    def _build_layout_tuple(self) -> tuple[int, dict[str, Any], list[Any]]:
        from gi.repository import GLib

        self._handlers.clear()
        children: list[Any] = []
        next_id = 1
        for entry in self._menu_provider():
            item_id = next_id
            next_id += 1
            props: dict[str, Any] = {}
            if entry.get("separator"):
                props["type"] = GLib.Variant("s", "separator")
            else:
                props["label"] = GLib.Variant("s", str(entry.get("label", "")))
                props["enabled"] = GLib.Variant("b", bool(entry.get("enabled", True)))
                props["visible"] = GLib.Variant("b", True)
                if "checked" in entry and entry["checked"] is not None:
                    props["toggle-type"] = GLib.Variant("s", "checkmark")
                    props["toggle-state"] = GLib.Variant("i", 1 if entry["checked"] else 0)
                if callable(entry.get("handler")):
                    self._handlers[item_id] = entry["handler"]
            children.append(GLib.Variant("(ia{sv}av)", (item_id, props, [])))
        root_props = {"children-display": GLib.Variant("s", "submenu")}
        return (0, root_props, children)

    def _build_get_layout_variant(self) -> Any:
        from gi.repository import GLib

        return GLib.Variant("(u(ia{sv}av))", (self._revision, self._build_layout_tuple()))

    def _on_menu_get_prop(self, _conn: Any, _sender: str, _path: str, _iface: str, prop: str) -> Any:
        from gi.repository import GLib

        if prop == "Version":
            return GLib.Variant("u", 3)
        if prop == "TextDirection":
            return GLib.Variant("s", "ltr")
        if prop == "Status":
            return GLib.Variant("s", "normal")
        if prop == "IconThemePath":
            return GLib.Variant("as", [])
        return None

    def _on_menu_call(
        self, _conn: Any, _sender: str, _path: str, _iface: str, method: str, params: Any, inv: Any
    ) -> None:
        from gi.repository import GLib

        if method == "GetLayout":
            inv.return_value(self._build_get_layout_variant())
            return
        if method == "GetGroupProperties":
            inv.return_value(GLib.Variant("(a(ia{sv}))", ([],)))
            return
        if method == "GetProperty":
            inv.return_value(GLib.Variant("(v)", (GLib.Variant("s", ""),)))
            return
        if method == "AboutToShow":
            inv.return_value(GLib.Variant("(b)", (True,)))
            return
        if method == "AboutToShowGroup":
            inv.return_value(GLib.Variant("(aiai)", ([], [])))
            return
        if method == "Event":
            item_id, event_id, _data, _ts = params.unpack()
            if event_id == "clicked" and item_id in self._handlers:
                cb = self._handlers[item_id]
                GLib.idle_add(lambda c=cb: (c(), False)[1])
            inv.return_value(None)
            return
        if method == "EventGroup":
            for item_id, event_id, _data, _ts in params.unpack()[0]:
                if event_id == "clicked" and item_id in self._handlers:
                    cb = self._handlers[item_id]
                    GLib.idle_add(lambda c=cb: (c(), False)[1])
            inv.return_value(GLib.Variant("(ai)", ([],)))
            return
        inv.return_value(None)
