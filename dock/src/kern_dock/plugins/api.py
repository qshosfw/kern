"""Declarative plugin manifest, protocol and capability-enforced PluginContext."""

from __future__ import annotations

import dataclasses
from typing import Any, Callable, Protocol

from ..core.events import EventHandler
from ..core.registry import ActionSpec, PageSpec, TrayItemSpec
from ..core.security import SecurityError
from ..render.handset.spec import HandsetSpec

VALID_CAPABILITIES: frozenset[str] = frozenset(
    {
        "events.subscribe",
        "ui.page",
        "ui.action",
        "ui.tray",
        "handset.register",
        "radio.read",
        "radio.control",
        "fs.read",
        "fs.write",
        "os.notify",
    }
)


@dataclasses.dataclass(frozen=True)
class PluginManifest:
    """Declarative metadata and least-privilege capability declaration for a Dock plugin."""

    id: str
    name: str
    version: str
    description: str
    author: str = "Kern Project"
    capabilities: tuple[str, ...] = ("events.subscribe",)
    icon: str = "application-x-addon-symbolic"
    builtin: bool = False


class DockPlugin(Protocol):
    """Protocol implemented by every Kern Dock plugin."""

    manifest: PluginManifest

    def activate(self, ctx: PluginContext) -> None: ...
    def deactivate(self) -> None: ...


@dataclasses.dataclass
class PluginRecord:
    manifest: PluginManifest
    origin: str                         # "builtin" | "workspace" | "user" | "entrypoint"
    enabled: bool = True
    loaded: bool = False
    error: str | None = None
    instance: Any = None


class PluginContext:
    """
    Capability-scoped facade handed to an activated plugin.
    Every registration is tagged with ``plugin_id`` so deactivation cleans up atomically,
    and every privileged action checks both declared capabilities and Hardened Mode.
    """

    def __init__(self, plugin_id: str, manifest: PluginManifest, runtime: Any) -> None:
        self.plugin_id = plugin_id
        self.manifest = manifest
        self._runtime = runtime
        self._sub_ids: list[int] = []
        self._caps = frozenset(manifest.capabilities)

    def _require(self, cap: str) -> None:
        if cap not in self._caps:
            raise SecurityError(f"Plugin '{self.plugin_id}' did not declare capability '{cap}'")
        self._runtime.security.check_plugin_capability(self.plugin_id, cap)

    @property
    def link(self) -> Any:
        self._require("radio.read")
        return self._runtime.link

    def subscribe(self, topic: str, handler: EventHandler, *, main_thread: bool = False) -> int:
        self._require("events.subscribe")
        sid = self._runtime.events.subscribe(topic, handler, main_thread=main_thread)
        self._sub_ids.append(sid)
        return sid

    def register_page(
        self,
        page_id: str,
        title: str,
        icon: str,
        builder: Callable[[], Any],
        *,
        order: int = 200,
        subtitle: str = "",
    ) -> None:
        self._require("ui.page")
        self._runtime.pages.register(
            PageSpec(
                id=f"plugin:{self.plugin_id}:{page_id}",
                title=title,
                icon=icon,
                section="Extensions",
                builder=builder,
                order=order,
                subtitle=subtitle,
                source=f"plugin:{self.plugin_id}",
            )
        )

    def register_action(
        self,
        action_id: str,
        title: str,
        handler: Callable[[], Any],
        *,
        subtitle: str = "",
        icon: str | None = None,
        mutates: bool = False,
    ) -> None:
        self._require("ui.action")
        self._runtime.actions.register(
            ActionSpec(
                id=f"plugin.{self.plugin_id}.{action_id}",
                title=title,
                handler=handler,
                category=self.manifest.name,
                subtitle=subtitle,
                icon=icon or self.manifest.icon,
                mutates=mutates,
                source=f"plugin:{self.plugin_id}",
            )
        )

    def register_tray_item(
        self,
        item_id: str,
        label: str,
        handler: Callable[[], None],
        *,
        toggle_state: Callable[[], bool] | None = None,
        enabled: Callable[[], bool] | None = None,
    ) -> None:
        self._require("ui.tray")
        self._runtime.tray_menu.register(
            TrayItemSpec(
                id=f"plugin:{self.plugin_id}:{item_id}",
                label=label,
                handler=handler,
                section="plugins",
                toggle_state=toggle_state,
                enabled=enabled,
                source=f"plugin:{self.plugin_id}",
            )
        )

    def register_handset_model(self, spec: HandsetSpec) -> None:
        self._require("handset.register")
        from ..render.handset.catalog import register_model

        register_model(spec)
        self._runtime.events.emit("models.changed", {"model_id": spec.id}, source=f"plugin:{self.plugin_id}")

    def tune(self, hz: int, modulation: str | None = None) -> None:
        self._require("radio.control")
        self._runtime.link.set_frequency_hz(hz)
        if modulation:
            self._runtime.link.set_modulation(modulation)

    def read_kernfs(self, path: str) -> bytes:
        self._require("fs.read")
        from ..core.security import sanitize_kernfs_path

        clean = sanitize_kernfs_path(path)
        be = self._runtime.link.fs_backend
        if be is None:
            raise RuntimeError("No KernFS backend available (radio offline)")
        return be.read_bytes(clean)

    def list_kernfs(self, path: str = "/") -> list[dict[str, Any]]:
        self._require("fs.read")
        from ..core.security import sanitize_kernfs_path

        clean = sanitize_kernfs_path(path)
        be = self._runtime.link.fs_backend
        if be is None:
            return []
        return be.ls(clean)

    def notify(self, title: str, body: str) -> None:
        self._require("os.notify")
        self._runtime.notifier.send(title, body, notif_id=f"plugin-{self.plugin_id}")

    def toast(self, message: str) -> None:
        self._runtime.events.emit("ui.toast", {"message": message}, source=f"plugin:{self.plugin_id}")

    def get_config(self) -> dict[str, Any]:
        return self._runtime.config.get_plugin_config(self.plugin_id)

    def set_config(self, data: dict[str, Any]) -> None:
        self._runtime.config.set_plugin_config(self.plugin_id, data)

    def cleanup(self) -> None:
        for sid in self._sub_ids:
            self._runtime.events.unsubscribe(sid)
        self._sub_ids.clear()
        src = f"plugin:{self.plugin_id}"
        self._runtime.pages.unregister_by_source(src)
        self._runtime.actions.unregister_by_source(src)
        self._runtime.tray_menu.unregister_by_source(src)
