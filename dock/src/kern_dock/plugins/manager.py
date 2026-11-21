"""Plugin discovery, isolated loading, lifecycle management and enable/disable persistence."""

from __future__ import annotations

import importlib.util
from pathlib import Path
from typing import Any

from .. import WORKSPACE_PLUGINS_DIR
from ..core.config import xdg_config_dir
from .api import PluginContext, PluginManifest, PluginRecord, VALID_CAPABILITIES


def user_plugins_dir() -> Path:
    return xdg_config_dir() / "plugins"


class PluginManager:
    """Discovers, validates, activates and isolates Kern Dock plugins."""

    def __init__(self, runtime: Any) -> None:
        self._runtime = runtime
        self._records: dict[str, PluginRecord] = {}
        self._contexts: dict[str, PluginContext] = {}

    def records(self) -> list[PluginRecord]:
        return sorted(self._records.values(), key=lambda r: (0 if r.manifest.builtin else 1, r.manifest.name.lower()))

    def get(self, plugin_id: str) -> PluginRecord | None:
        return self._records.get(plugin_id)

    def discover_all(self) -> list[PluginRecord]:
        disabled = set(self._runtime.config.get("plugins.disabled", []))

        # 1. Built-in plugins
        from .builtin import BUILTIN_PLUGINS

        for cls in BUILTIN_PLUGINS:
            self._register_class(cls, origin="builtin", disabled=disabled)

        # 2. Workspace drop-in plugins (`dock/plugins/*.py`)
        self._scan_directory(WORKSPACE_PLUGINS_DIR, origin="workspace", disabled=disabled)

        # 3. User plugins (`~/.config/kern-dock/plugins/*.py`)
        self._scan_directory(user_plugins_dir(), origin="user", disabled=disabled)

        # 4. Installed Python entry points (`kern_dock.plugins`)
        try:
            from importlib.metadata import entry_points

            for ep in entry_points(group="kern_dock.plugins"):
                try:
                    cls = ep.load()
                    if hasattr(cls, "manifest") and cls.manifest.id not in self._records:
                        self._register_class(cls, origin="entrypoint", disabled=disabled)
                except Exception:
                    continue
        except Exception:
            pass

        return self.records()

    def _scan_directory(self, folder: Path, *, origin: str, disabled: set[str]) -> None:
        if not folder.is_dir():
            return
        for py in sorted(folder.glob("*.py")):
            if py.name.startswith("_"):
                continue
            try:
                spec = importlib.util.spec_from_file_location(f"kern_dock_ext_{origin}_{py.stem}", py)
                if spec is None or spec.loader is None:
                    continue
                mod = importlib.util.module_from_spec(spec)
                spec.loader.exec_module(mod)
                cls = getattr(mod, "PLUGIN_CLASS", None)
                if cls is None:
                    for obj in vars(mod).values():
                        if isinstance(obj, type) and isinstance(getattr(obj, "manifest", None), PluginManifest):
                            cls = obj
                            break
                if cls is not None:
                    self._register_class(cls, origin=origin, disabled=disabled)
            except Exception as exc:
                self._runtime.security.record("plugin", f"discover:{py.name}", "deny", str(exc))

    def _register_class(self, cls: Any, *, origin: str, disabled: set[str]) -> None:
        manifest = getattr(cls, "manifest", None)
        if not isinstance(manifest, PluginManifest):
            return
        bad_caps = [c for c in manifest.capabilities if c not in VALID_CAPABILITIES]
        if bad_caps:
            self._records[manifest.id] = PluginRecord(
                manifest=manifest,
                origin=origin,
                enabled=False,
                loaded=False,
                error=f"Undeclared/unknown capabilities: {', '.join(bad_caps)}",
            )
            return
        if manifest.id in self._records:
            return
        try:
            inst = cls()
        except Exception as exc:
            self._records[manifest.id] = PluginRecord(
                manifest=manifest, origin=origin, enabled=False, loaded=False, error=str(exc)
            )
            return
        self._records[manifest.id] = PluginRecord(
            manifest=manifest,
            origin=origin,
            enabled=manifest.id not in disabled,
            loaded=False,
            instance=inst,
        )

    def activate_enabled(self) -> None:
        for rec in self.records():
            if rec.enabled and not rec.loaded and rec.instance is not None:
                self.activate(rec.manifest.id)

    def activate(self, plugin_id: str) -> bool:
        rec = self._records.get(plugin_id)
        if rec is None or rec.instance is None:
            return False
        if rec.loaded:
            return True
        ctx = PluginContext(plugin_id, rec.manifest, self._runtime)
        try:
            rec.instance.activate(ctx)
            self._contexts[plugin_id] = ctx
            rec.loaded = True
            rec.enabled = True
            rec.error = None
            self._runtime.security.record("plugin", f"activate:{plugin_id}", "allow", f"origin={rec.origin}")
            self._runtime.events.emit("plugins.changed", {"plugin_id": plugin_id, "loaded": True}, source="plugins")
            return True
        except Exception as exc:
            ctx.cleanup()
            rec.loaded = False
            rec.error = str(exc)
            self._runtime.security.record("plugin", f"activate:{plugin_id}", "deny", str(exc))
            self._runtime.events.emit("plugins.changed", {"plugin_id": plugin_id, "error": rec.error}, source="plugins")
            return False

    def deactivate(self, plugin_id: str) -> None:
        rec = self._records.get(plugin_id)
        if rec is None:
            return
        if rec.loaded and rec.instance is not None:
            try:
                rec.instance.deactivate()
            except Exception:
                pass
        ctx = self._contexts.pop(plugin_id, None)
        if ctx is not None:
            ctx.cleanup()
        rec.loaded = False
        self._runtime.events.emit("plugins.changed", {"plugin_id": plugin_id, "loaded": False}, source="plugins")

    def set_enabled(self, plugin_id: str, enabled: bool) -> bool:
        rec = self._records.get(plugin_id)
        if rec is None:
            return False
        disabled = set(self._runtime.config.get("plugins.disabled", []))
        if enabled:
            disabled.discard(plugin_id)
            self._runtime.config.set("plugins.disabled", sorted(disabled))
            rec.enabled = True
            return self.activate(plugin_id)
        disabled.add(plugin_id)
        self._runtime.config.set("plugins.disabled", sorted(disabled))
        rec.enabled = False
        self.deactivate(plugin_id)
        return True

    def shutdown(self) -> None:
        for pid in list(self._contexts.keys()):
            self.deactivate(pid)

    def summary(self) -> list[dict[str, Any]]:
        return [
            {
                "id": r.manifest.id,
                "name": r.manifest.name,
                "version": r.manifest.version,
                "description": r.manifest.description,
                "author": r.manifest.author,
                "origin": r.origin,
                "builtin": r.manifest.builtin,
                "capabilities": list(r.manifest.capabilities),
                "enabled": r.enabled,
                "loaded": r.loaded,
                "error": r.error,
            }
            for r in self.records()
        ]
