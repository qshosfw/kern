"""Hardened OS IPC: 0600 Unix domain JSON-RPC socket + Session D-Bus service (`org.kernos.Dock`)."""

from __future__ import annotations

import json
import os
import socket
import threading
from pathlib import Path
from typing import Any, Callable

from .. import APP_ID
from ..core.config import xdg_runtime_dir

_DBUS_XML = """
<node>
  <interface name="org.kernos.Dock">
    <method name="Call">
      <arg name="method" type="s" direction="in"/>
      <arg name="params_json" type="s" direction="in"/>
      <arg name="result_json" type="s" direction="out"/>
    </method>
    <method name="Status">
      <arg name="status_json" type="s" direction="out"/>
    </method>
  </interface>
</node>
"""


def default_socket_path() -> Path:
    return xdg_runtime_dir() / "kern-dock.sock"


class IpcServer:
    """Exposes Kern Dock control/telemetry over a local 0600 Unix socket and Session D-Bus."""

    DBUS_PATH = "/org/kernos/Dock"

    def __init__(
        self,
        dispatcher: Callable[[str, dict[str, Any]], dict[str, Any]],
        *,
        socket_path: Path | None = None,
        enable_dbus: bool = True,
    ) -> None:
        self._dispatcher = dispatcher
        self.socket_path = socket_path or default_socket_path()
        self.enable_dbus = enable_dbus
        self._sock: socket.socket | None = None
        self._stop = threading.Event()
        self._thread: threading.Thread | None = None
        self._dbus_conn: Any = None
        self._dbus_reg_id: int = 0
        self._dbus_owner_id: int = 0
        self.running: bool = False
        self.dbus_active: bool = False

    def start(self) -> bool:
        if self.running:
            return True
        self._stop.clear()
        try:
            self.socket_path.parent.mkdir(parents=True, exist_ok=True)
            if self.socket_path.exists():
                self.socket_path.unlink()
            sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
            sock.bind(str(self.socket_path))
            os.chmod(self.socket_path, 0o600)
            sock.listen(8)
            sock.settimeout(0.25)
            self._sock = sock
            self._thread = threading.Thread(target=self._serve_loop, daemon=True, name="kern-dock-ipc")
            self._thread.start()
            self.running = True
        except Exception:
            self.running = False
            return False

        if self.enable_dbus:
            self._start_dbus()
        return True

    def _start_dbus(self) -> None:
        try:
            from gi.repository import Gio

            self._dbus_conn = Gio.bus_get_sync(Gio.BusType.SESSION, None)
            node_info = Gio.DBusNodeInfo.new_for_xml(_DBUS_XML)
            self._dbus_reg_id = self._dbus_conn.register_object(
                self.DBUS_PATH,
                node_info.interfaces[0],
                self._on_dbus_call,
                None,
                None,
            )
            self._dbus_owner_id = Gio.bus_own_name_on_connection(
                self._dbus_conn, APP_ID, Gio.BusNameOwnerFlags.NONE, None, None
            )
            self.dbus_active = True
        except Exception:
            self.dbus_active = False

    def _on_dbus_call(
        self, _conn: Any, _sender: str, _path: str, _iface: str, method: str, params: Any, inv: Any
    ) -> None:
        from gi.repository import GLib

        try:
            if method == "Status":
                res = self._handle_request({"method": "status", "params": {}})
            elif method == "Call":
                m_name, p_json = params.unpack()
                p_obj = json.loads(p_json) if p_json else {}
                res = self._handle_request({"method": m_name, "params": p_obj if isinstance(p_obj, dict) else {}})
            else:
                res = {"ok": False, "error": f"Unknown D-Bus method {method}"}
            inv.return_value(GLib.Variant("(s)", (json.dumps(res),)))
        except Exception as exc:
            inv.return_value(GLib.Variant("(s)", (json.dumps({"ok": False, "error": str(exc)}),)))

    def stop(self) -> None:
        self._stop.set()
        self.running = False
        if self._sock is not None:
            try:
                self._sock.close()
            except Exception:
                pass
            self._sock = None
        if self._thread and self._thread.is_alive():
            self._thread.join(timeout=1.0)
        self._thread = None
        try:
            if self.socket_path.exists():
                self.socket_path.unlink()
        except Exception:
            pass
        if self._dbus_owner_id:
            try:
                from gi.repository import Gio

                Gio.bus_unown_name(self._dbus_owner_id)
            except Exception:
                pass
            self._dbus_owner_id = 0
        if self._dbus_conn and self._dbus_reg_id:
            try:
                self._dbus_conn.unregister_object(self._dbus_reg_id)
            except Exception:
                pass
            self._dbus_reg_id = 0
        self._dbus_conn = None
        self.dbus_active = False

    def _handle_request(self, req: dict[str, Any]) -> dict[str, Any]:
        method = str(req.get("method", "")).strip()
        params = req.get("params")
        if not isinstance(params, dict):
            params = {}
        try:
            data = self._dispatcher(method, params)
            return {"ok": True, "result": data}
        except Exception as exc:
            return {"ok": False, "error": str(exc)}

    def _serve_loop(self) -> None:
        while not self._stop.is_set() and self._sock is not None:
            try:
                conn, _ = self._sock.accept()
            except socket.timeout:
                continue
            except OSError:
                break
            try:
                conn.settimeout(3.0)
                buf = b""
                while len(buf) < 65536:
                    chunk = conn.recv(4096)
                    if not chunk:
                        break
                    buf += chunk
                    if b"\n" in buf:
                        break
                line = buf.split(b"\n", 1)[0].decode("utf-8", errors="replace").strip()
                if not line:
                    resp = {"ok": False, "error": "Empty request"}
                else:
                    req = json.loads(line)
                    resp = self._handle_request(req if isinstance(req, dict) else {})
                conn.sendall((json.dumps(resp) + "\n").encode("utf-8"))
            except Exception as exc:
                try:
                    conn.sendall((json.dumps({"ok": False, "error": str(exc)}) + "\n").encode("utf-8"))
                except Exception:
                    pass
            finally:
                try:
                    conn.close()
                except Exception:
                    pass


class IpcClient:
    """Thin client talking to a running Kern Dock instance over its 0600 local Unix socket."""

    def __init__(self, socket_path: Path | None = None, timeout: float = 3.0) -> None:
        self.socket_path = socket_path or default_socket_path()
        self.timeout = timeout

    def call(self, method: str, params: dict[str, Any] | None = None) -> dict[str, Any]:
        if not self.socket_path.exists():
            raise RuntimeError(f"Kern Dock is not running (no socket at {self.socket_path})")
        sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        sock.settimeout(self.timeout)
        try:
            sock.connect(str(self.socket_path))
            payload = (json.dumps({"method": method, "params": params or {}}) + "\n").encode("utf-8")
            sock.sendall(payload)
            buf = b""
            while len(buf) < 262144:
                chunk = sock.recv(4096)
                if not chunk:
                    break
                buf += chunk
                if b"\n" in buf:
                    break
            return json.loads(buf.decode("utf-8").strip())
        finally:
            sock.close()
