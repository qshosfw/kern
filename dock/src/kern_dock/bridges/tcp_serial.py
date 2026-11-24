"""Local TCP RigWire Command Bridge (127.0.0.1:4447) for netcat, scripts & automation."""

from __future__ import annotations

import socket
import threading
from typing import Any, Callable


class TcpCommandBridge:
    """Line-oriented localhost TCP server forwarding commands to DockLink.run_wire_command()."""

    def __init__(
        self,
        link: Any,
        *,
        host: str = "127.0.0.1",
        port: int = 4447,
        on_change: Callable[[], None] | None = None,
    ) -> None:
        self._link = link
        self.host = host
        self.port = port
        self._on_change = on_change
        self._sock: socket.socket | None = None
        self._stop = threading.Event()
        self._clients: set[socket.socket] = set()
        self._lock = threading.Lock()
        self.command_count = 0

    @property
    def running(self) -> bool:
        return self._sock is not None

    @property
    def client_count(self) -> int:
        with self._lock:
            return len(self._clients)

    def start(self, port: int | None = None) -> int:
        if self._sock is not None:
            return self.port
        if port is not None:
            self.port = int(port)
        s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        s.bind((self.host, self.port))
        s.listen(8)
        s.settimeout(0.4)
        self.port = int(s.getsockname()[1])
        self._sock = s
        self._stop.clear()
        threading.Thread(target=self._accept_loop, daemon=True, name="kern-tcp-cmd").start()
        if self._on_change:
            self._on_change()
        return self.port

    def stop(self) -> None:
        self._stop.set()
        s = self._sock
        self._sock = None
        if s is not None:
            try:
                s.close()
            except Exception:
                pass
        with self._lock:
            for c in list(self._clients):
                try:
                    c.close()
                except Exception:
                    pass
            self._clients.clear()
        if self._on_change:
            self._on_change()

    def _accept_loop(self) -> None:
        while not self._stop.is_set() and self._sock is not None:
            try:
                conn, _addr = self._sock.accept()
            except TimeoutError:
                continue
            except OSError:
                break
            with self._lock:
                self._clients.add(conn)
            if self._on_change:
                self._on_change()
            threading.Thread(target=self._client_loop, args=(conn,), daemon=True).start()

    def _client_loop(self, conn: socket.socket) -> None:
        buf = b""
        try:
            conn.settimeout(1.0)
            while not self._stop.is_set():
                try:
                    chunk = conn.recv(1024)
                except TimeoutError:
                    continue
                if not chunk:
                    break
                buf += chunk
                while b"\n" in buf:
                    raw_line, buf = buf.split(b"\n", 1)
                    cmd = raw_line.decode("utf-8", errors="ignore").strip()
                    if not cmd:
                        continue
                    if cmd.lower() in ("quit", "exit"):
                        return
                    self.command_count += 1
                    try:
                        out = str(self._link.run_wire_command(cmd))
                    except Exception as exc:
                        out = f"ERR: {exc}"
                    conn.sendall((out.rstrip("\n") + "\n").encode("utf-8"))
                    if self._on_change:
                        self._on_change()
        except OSError:
            pass
        finally:
            with self._lock:
                self._clients.discard(conn)
            try:
                conn.close()
            except Exception:
                pass
            if self._on_change:
                self._on_change()
