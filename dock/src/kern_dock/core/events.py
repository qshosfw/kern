"""Thread-safe typed EventBus with optional GLib main-loop marshaling."""

from __future__ import annotations

import dataclasses
import threading
import time
from typing import Any, Callable


@dataclasses.dataclass(frozen=True)
class Event:
    """Immutable domain event flowing between Dock services, plugins and UI."""

    topic: str
    payload: dict[str, Any] = dataclasses.field(default_factory=dict)
    source: str = "core"
    ts_mono: float = dataclasses.field(default_factory=time.monotonic)


EventHandler = Callable[[Event], None]


@dataclasses.dataclass
class _Subscription:
    sub_id: int
    topic: str
    handler: EventHandler
    main_thread: bool


class EventBus:
    """
    Lightweight publish/subscribe bus decoupling link/hardware threads, plugins,
    OS integrations and GTK4 widgets.

    Topics use dot-separated namespaces (e.g. ``link.connected``, ``radio.state``,
    ``security.changed``). Prefix wildcards like ``link.*`` or ``*`` are supported.
    """

    def __init__(self, main_dispatcher: Callable[[Callable[[], Any]], Any] | None = None) -> None:
        self._lock = threading.RLock()
        self._next_id = 1
        self._subs: dict[int, _Subscription] = {}
        self._main_dispatcher = main_dispatcher

    def set_main_dispatcher(self, dispatcher: Callable[[Callable[[], Any]], Any] | None) -> None:
        with self._lock:
            self._main_dispatcher = dispatcher

    def subscribe(self, topic: str, handler: EventHandler, *, main_thread: bool = False) -> int:
        with self._lock:
            sid = self._next_id
            self._next_id += 1
            self._subs[sid] = _Subscription(sid, topic, handler, main_thread)
            return sid

    def unsubscribe(self, sub_id: int) -> None:
        with self._lock:
            self._subs.pop(sub_id, None)

    @staticmethod
    def _matches(pattern: str, topic: str) -> bool:
        if pattern == "*" or pattern == topic:
            return True
        if pattern.endswith(".*"):
            return topic.startswith(pattern[:-1])
        return False

    def emit(self, topic: str, payload: dict[str, Any] | None = None, *, source: str = "core") -> Event:
        ev = Event(topic=topic, payload=dict(payload or {}), source=source)
        with self._lock:
            targets = [s for s in self._subs.values() if self._matches(s.topic, topic)]
            dispatcher = self._main_dispatcher

        for sub in targets:
            if sub.main_thread and dispatcher is not None:
                def _invoke(h: EventHandler = sub.handler, e: Event = ev) -> bool:
                    try:
                        h(e)
                    except Exception:
                        pass
                    return False

                dispatcher(_invoke)
            else:
                try:
                    sub.handler(ev)
                except Exception:
                    pass
        return ev
