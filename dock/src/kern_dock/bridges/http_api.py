"""Local HTTP JSON + Live LCD PNG Bridge (127.0.0.1:8085) for OBS, Home Assistant & dashboards."""

from __future__ import annotations

import dataclasses
import http.server
import json
import socketserver
import threading
from typing import Any, Callable

from ..render.capture import encode_png_rgba, render_lcd_to_rgba


class _ThreadingHTTPServer(socketserver.ThreadingMixIn, http.server.HTTPServer):
    daemon_threads = True
    allow_reuse_address = True


class HttpApiBridge:
    """Lightweight localhost HTTP server exposing /api/status, /api/screen.png & /api/framebuffer."""

    def __init__(
        self,
        runtime: Any,
        *,
        host: str = "127.0.0.1",
        port: int = 8085,
        on_change: Callable[[], None] | None = None,
    ) -> None:
        self._runtime = runtime
        self.host = host
        self.port = port
        self._on_change = on_change
        self._server: _ThreadingHTTPServer | None = None
        self._thread: threading.Thread | None = None
        self.request_count = 0

    @property
    def running(self) -> bool:
        return self._server is not None

    @property
    def base_url(self) -> str:
        return f"http://{self.host}:{self.port}"

    def start(self, port: int | None = None) -> int:
        if self._server is not None:
            return self.port
        if port is not None:
            self.port = int(port)
        bridge = self

        class _Handler(http.server.BaseHTTPRequestHandler):
            def log_message(self, _fmt: str, *_args: Any) -> None:
                return

            def _send(self, code: int, ctype: str, body: bytes) -> None:
                self.send_response(code)
                self.send_header("Content-Type", ctype)
                self.send_header("Content-Length", str(len(body)))
                self.send_header("Cache-Control", "no-store")
                self.send_header("Access-Control-Allow-Origin", "*")
                self.end_headers()
                self.wfile.write(body)

            def do_GET(self) -> None:  # noqa: N802
                bridge.request_count += 1
                if bridge._on_change:
                    bridge._on_change()
                path = self.path.split("?", 1)[0]
                rt = bridge._runtime
                if path in ("/", "/api", "/api/status"):
                    snap = rt.link.status_snapshot()
                    snap["model"] = rt.active_model
                    snap["lcd"] = {
                        "palette": rt.lcd_st.palette,
                        "backlight": rt.lcd_st.backlight,
                        "contrast": rt.lcd_st.contrast,
                        "inverted": rt.lcd_st.invert_lcd,
                    }
                    self._send(200, "application/json", json.dumps(snap, indent=2).encode("utf-8"))
                elif path == "/api/screen.png":
                    fb = bytes(rt.link.last_fb)
                    w, h, rgba = render_lcd_to_rgba(fb, dataclasses.replace(rt.lcd_st, ghosting=False), width=540, height=374)
                    self._send(200, "image/png", encode_png_rgba(w, h, rgba))
                elif path == "/api/framebuffer":
                    self._send(200, "application/octet-stream", bytes(rt.link.last_fb[:1024]))
                else:
                    self._send(404, "application/json", b'{"error":"not_found"}')

        srv = _ThreadingHTTPServer((self.host, self.port), _Handler)
        self.port = int(srv.server_address[1])
        self._server = srv
        self._thread = threading.Thread(target=srv.serve_forever, daemon=True, name="kern-http-api")
        self._thread.start()
        if self._on_change:
            self._on_change()
        return self.port

    def stop(self) -> None:
        srv = self._server
        self._server = None
        if srv is not None:
            try:
                srv.shutdown()
                srv.server_close()
            except Exception:
                pass
        if self._on_change:
            self._on_change()
