"""Native OS desktop notification bridge (Gio.Application.send_notification + libnotify fallback)."""

from __future__ import annotations

from typing import Any

from .. import APP_NAME


class DesktopNotifier:
    """Dispatches non-intrusive OS desktop notifications while honoring user preferences."""

    def __init__(self, *, enabled: bool = True) -> None:
        self.enabled = enabled
        self._gio_app: Any = None
        self._notify_mod: Any = None
        try:
            import gi

            gi.require_version("Notify", "0.7")
            from gi.repository import Notify

            if not Notify.is_initted():
                Notify.init(APP_NAME)
            self._notify_mod = Notify
        except Exception:
            self._notify_mod = None

    def bind_application(self, app: Any) -> None:
        self._gio_app = app

    def send(self, title: str, body: str, *, notif_id: str | None = None, urgent: bool = False) -> bool:
        if not self.enabled:
            return False
        if self._gio_app is not None:
            try:
                from gi.repository import Gio

                n = Gio.Notification.new(title)
                n.set_body(body)
                n.set_icon(Gio.ThemedIcon.new("org.kernos.Dock"))
                if urgent:
                    n.set_priority(Gio.NotificationPriority.URGENT)
                self._gio_app.send_notification(notif_id or "kern-dock-event", n)
                return True
            except Exception:
                pass
        if self._notify_mod is not None:
            try:
                n = self._notify_mod.Notification.new(title, body, "org.kernos.Dock")
                n.show()
                return True
            except Exception:
                pass
        return False
