"""Declarative registries for actions, workspace pages, tray items and Command Palette."""

from __future__ import annotations

import dataclasses
from typing import Any, Callable


@dataclasses.dataclass(frozen=True)
class ActionSpec:
    """One declarative definition feeding GTK actions, accelerators, menus, IPC and Command Palette."""

    id: str
    title: str
    handler: Callable[[], Any]
    category: str = "Application"     # "Navigation" | "Radio" | "Hardware" | "Capture" | "System" | "Plugin"
    subtitle: str = ""
    accel: str | None = None
    icon: str | None = None
    requires_rigwire: bool = False
    mutates: bool = False
    source: str = "core"


@dataclasses.dataclass(frozen=True)
class PageSpec:
    """Declarative workspace page registered by core views or plugins."""

    id: str
    title: str
    icon: str
    section: str                      # "Workspace" | "Tools" | "Extensions"
    builder: Callable[[], Any]
    order: int = 100
    subtitle: str = ""
    badge: Callable[[], str | None] | None = None
    source: str = "core"


@dataclasses.dataclass(frozen=True)
class TrayItemSpec:
    """Declarative entry exposed in the OS System Tray menu."""

    id: str
    label: str
    handler: Callable[[], None]
    section: str = "plugins"          # "quick" | "hardware" | "plugins"
    toggle_state: Callable[[], bool] | None = None
    enabled: Callable[[], bool] | None = None
    source: str = "core"


class CommandRegistry:
    """Central declarative command & action catalog."""

    def __init__(self) -> None:
        self._actions: dict[str, ActionSpec] = {}
        self._listeners: list[Callable[[], None]] = []

    def register(self, spec: ActionSpec) -> None:
        self._actions[spec.id] = spec
        self._notify()

    def unregister(self, action_id: str) -> None:
        if self._actions.pop(action_id, None) is not None:
            self._notify()

    def unregister_by_source(self, source: str) -> None:
        dead = [k for k, v in self._actions.items() if v.source == source]
        for k in dead:
            del self._actions[k]
        if dead:
            self._notify()

    def get(self, action_id: str) -> ActionSpec | None:
        return self._actions.get(action_id)

    def all(self) -> list[ActionSpec]:
        return list(self._actions.values())

    def search(self, query: str) -> list[ActionSpec]:
        q = query.strip().lower()
        if not q:
            return self.all()
        scored: list[tuple[int, ActionSpec]] = []
        for a in self._actions.values():
            hay = f"{a.title} {a.subtitle} {a.category} {a.id}".lower()
            if a.title.lower().startswith(q):
                scored.append((0, a))
            elif q in a.title.lower():
                scored.append((1, a))
            elif all(tok in hay for tok in q.split()):
                scored.append((2, a))
        scored.sort(key=lambda pair: (pair[0], pair[1].category, pair[1].title))
        return [a for _, a in scored]

    def invoke(self, action_id: str) -> Any:
        spec = self._actions.get(action_id)
        if spec is None:
            raise KeyError(f"Unknown action '{action_id}'")
        return spec.handler()

    def on_change(self, cb: Callable[[], None]) -> None:
        self._listeners.append(cb)

    def _notify(self) -> None:
        for cb in list(self._listeners):
            try:
                cb()
            except Exception:
                pass


class PageRegistry:
    """Ordered registry of core and plugin-contributed workspace pages."""

    SECTION_ORDER = ("Workspace", "Tools", "Extensions")

    def __init__(self) -> None:
        self._pages: dict[str, PageSpec] = {}
        self._listeners: list[Callable[[], None]] = []

    def register(self, spec: PageSpec) -> None:
        self._pages[spec.id] = spec
        self._notify()

    def unregister(self, page_id: str) -> None:
        if self._pages.pop(page_id, None) is not None:
            self._notify()

    def unregister_by_source(self, source: str) -> None:
        dead = [k for k, v in self._pages.items() if v.source == source]
        for k in dead:
            del self._pages[k]
        if dead:
            self._notify()

    def get(self, page_id: str) -> PageSpec | None:
        return self._pages.get(page_id)

    def ordered(self) -> list[PageSpec]:
        def _rank(p: PageSpec) -> tuple[int, int, str]:
            sec_i = self.SECTION_ORDER.index(p.section) if p.section in self.SECTION_ORDER else 99
            return (sec_i, p.order, p.title.lower())

        return sorted(self._pages.values(), key=_rank)

    def on_change(self, cb: Callable[[], None]) -> None:
        self._listeners.append(cb)

    def _notify(self) -> None:
        for cb in list(self._listeners):
            try:
                cb()
            except Exception:
                pass


class TrayMenuRegistry:
    """Extensible registry of plugin-contributed system tray menu items."""

    def __init__(self) -> None:
        self._items: dict[str, TrayItemSpec] = {}
        self._listeners: list[Callable[[], None]] = []

    def register(self, spec: TrayItemSpec) -> None:
        self._items[spec.id] = spec
        self._notify()

    def unregister_by_source(self, source: str) -> None:
        dead = [k for k, v in self._items.items() if v.source == source]
        for k in dead:
            del self._items[k]
        if dead:
            self._notify()

    def items(self, section: str | None = None) -> list[TrayItemSpec]:
        vals = list(self._items.values())
        return [i for i in vals if i.section == section] if section else vals

    def on_change(self, cb: Callable[[], None]) -> None:
        self._listeners.append(cb)

    def _notify(self) -> None:
        for cb in list(self._listeners):
            try:
                cb()
            except Exception:
                pass
