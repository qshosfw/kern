"""Standard KISS TNC TCP Bridge (default 127.0.0.1:8001) for APRS, Pat, Winlink, Xastir, YAAC."""

from __future__ import annotations

import socket
import threading
from typing import Callable

FEND = 0xC0
FESC = 0xDB
TFEND = 0xDC
TFESC = 0xDD


def kiss_encode(payload: bytes, port: int = 0, cmd: int = 0) -> bytes:
    """Encode an AX.25 frame into standard KISS framing."""
    out = bytearray([FEND, ((port & 0x0F) << 4) | (cmd & 0x0F)])
    for b in payload:
        if b == FEND:
            out.extend((FESC, TFEND))
        elif b == FESC:
            out.extend((FESC, TFESC))
        else:
            out.append(b)
    out.append(FEND)
    return bytes(out)


def kiss_decode_stream(buf: bytearray) -> list[tuple[int, int, bytes]]:
    """Extract complete (port, cmd, payload) KISS frames from a byte stream buffer."""
    frames: list[tuple[int, int, bytes]] = []
    while True:
        try:
            start = buf.index(FEND)
        except ValueError:
            buf.clear()
            break
        if start > 0:
            del buf[:start]
        try:
            end = buf.index(FEND, 1)
        except ValueError:
            break
        raw = bytes(buf[1:end])
        del buf[:end]
        if not raw:
            continue
        type_byte = raw[0]
        port = (type_byte >> 4) & 0x0F
        cmd = type_byte & 0x0F
        out = bytearray()
        esc = False
        for b in raw[1:]:
            if esc:
                if b == TFEND:
                    out.append(FEND)
                elif b == TFESC:
                    out.append(FESC)
                esc = False
            elif b == FESC:
                esc = True
            else:
                out.append(b)
        frames.append((port, cmd, bytes(out)))
    return frames


class KissTncBridge:
    """Lightweight multi-client KISS TNC over TCP bridge."""

    def __init__(
        self,
        *,
        host: str = "127.0.0.1",
        port: int = 8001,
        on_tx_frame: Callable[[bytes], None] | None = None,
        on_change: Callable[[], None] | None = None,
    ) -> None:
        self.host = host
        self.port = port
        self._on_tx_frame = on_tx_frame
        self._on_change = on_change
        self._sock: socket.socket | None = None
        self._clients: list[socket.socket] = []
        self._lock = threading.Lock()
        self._stop = threading.Event()
        self.tx_count = 0
        self.rx_count = 0

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
        threading.Thread(target=self._accept_loop, daemon=True, name="kern-kiss-tnc").start()
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

    def broadcast_rx(self, ax25_payload: bytes) -> None:
        """Broadcast a received radio packet to all connected KISS clients."""
        encoded = kiss_encode(ax25_payload)
        self.rx_count += 1
        dead: list[socket.socket] = []
        with self._lock:
            for c in self._clients:
                try:
                    c.sendall(encoded)
                except Exception:
                    dead.append(c)
            for c in dead:
                self._clients.remove(c)
                try:
                    c.close()
                except Exception:
                    pass

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
        buf = bytearray()
        conn.settimeout(0.5)
        try:
            while not self._stop.is_set():
                try:
                    chunk = conn.recv(1024)
                except socket.timeout:
                    continue
                if not chunk:
                    break
                buf.extend(chunk)
                for _port, cmd, payload in kiss_decode_stream(buf):
                    if cmd == 0 and payload:
                        self.tx_count += 1
                        if self._on_tx_frame:
                            self._on_tx_frame(payload)
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
