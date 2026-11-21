"""Standard Hamlib rigctld TCP Bridge (default 127.0.0.1:4532) for WSJT-X, Gpredict, FLRig, CQRLog, SDR++."""

from __future__ import annotations

import socket
import threading
from typing import Any, Callable


class RigctldBridge:
    """Hamlib rigctld network protocol bridge backed by DockLink."""

    def __init__(
        self,
        link: Any,
        *,
        host: str = "127.0.0.1",
        port: int = 4532,
        on_change: Callable[[], None] | None = None,
    ) -> None:
        self._link = link
        self.host = host
        self.port = port
        self._on_change = on_change
        self._sock: socket.socket | None = None
        self._clients: list[socket.socket] = []
        self._lock = threading.Lock()
        self._stop = threading.Event()

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
            self.port = port
        srv = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        srv.bind((self.host, self.port))
        srv.listen(8)
        srv.settimeout(0.25)
        self.port = int(srv.getsockname()[1])
        self._sock = srv
        self._stop.clear()
        threading.Thread(target=self._accept_loop, daemon=True, name="kern-rigctld").start()
        if self._on_change:
            self._on_change()
        return self.port

    def stop(self) -> None:
        self._stop.set()
        if self._sock is not None:
            try:
                self._sock.close()
            except Exception:
                pass
            self._sock = None
        with self._lock:
            for c in self._clients:
                try:
                    c.close()
                except Exception:
                    pass
            self._clients.clear()
        if self._on_change:
            self._on_change()

    def handle_command(self, line: str) -> str | None:
        """Process one rigctld protocol line and return the response (or None to close)."""
        parts = line.strip().split()
        if not parts:
            return ""
        cmd = parts[0]
        lk = self._link
        if cmd in ("q", "quit"):
            return None
        if cmd == "\\chk_vfo":
            return "0\n"
        if cmd == "\\dump_state":
            return "0\n2\n0\n18000000 1300000000 0x2f -1 -1 0x3 0x0\n0 0 0 0 0 0 0\n"
        if cmd in ("f", "\\get_freq"):
            hz = int(lk.radio_state.get("rx_freq_hz") or 0)
            return f"{hz}\n"
        if cmd in ("F", "\\set_freq") and len(parts) >= 2:
            try:
                lk.set_frequency_hz(int(float(parts[1])))
                return "RPRT 0\n"
            except Exception:
                return "RPRT -1\n"
        if cmd in ("m", "\\get_mode"):
            mode = str(lk.radio_state.get("mod_name", "FM"))
            return f"{mode}\n12500\n"
        if cmd in ("M", "\\set_mode") and len(parts) >= 2:
            try:
                lk.set_modulation(parts[1].upper())
                return "RPRT 0\n"
            except Exception:
                return "RPRT -1\n"
        if cmd in ("v", "\\get_vfo"):
            return "VFOB\n" if lk.radio_state.get("active_vfo") else "VFOA\n"
        if cmd in ("V", "\\set_vfo") and len(parts) >= 2:
            want_b = parts[1].upper() == "VFOB"
            if bool(lk.radio_state.get("active_vfo")) != want_b:
                lk.toggle_vfo()
            return "RPRT 0\n"
        if cmd in ("t", "\\get_ptt"):
            return "1\n" if lk.radio_state.get("ptt_active") else "0\n"
        if cmd in ("T", "\\set_ptt") and len(parts) >= 2:
            want_tx = parts[1] != "0"
            try:
                lk.send_key("ptt", "press" if want_tx else "release")
                return "RPRT 0\n"
            except Exception:
                return "RPRT -11\n"
        if cmd in ("l", "\\get_level") and len(parts) >= 2 and parts[1].upper() == "STRENGTH":
            rssi = int(lk.radio_state.get("rssi_dbm", -127))
            return f"{rssi + 73}\n"
        return "RPRT -4\n"

    def _accept_loop(self) -> None:
        while not self._stop.is_set() and self._sock is not None:
            try:
                conn, _addr = self._sock.accept()
            except socket.timeout:
                continue
            except OSError:
                break
            with self._lock:
                self._clients.append(conn)
            if self._on_change:
                self._on_change()
            threading.Thread(target=self._client_loop, args=(conn,), daemon=True).start()

    def _client_loop(self, conn: socket.socket) -> None:
        buf = b""
        conn.settimeout(0.5)
        try:
            while not self._stop.is_set():
                try:
                    chunk = conn.recv(1024)
                except socket.timeout:
                    continue
                if not chunk:
                    break
                buf += chunk
                while b"\n" in buf:
                    raw_line, buf = buf.split(b"\n", 1)
                    resp = self.handle_command(raw_line.decode("utf-8", errors="ignore"))
                    if resp is None:
                        return
                    if resp:
                        conn.sendall(resp.encode("utf-8"))
        except Exception:
            pass
        finally:
            with self._lock:
                if conn in self._clients:
                    self._clients.remove(conn)
            try:
                conn.close()
            except Exception:
                pass
            if self._on_change:
                self._on_change()
