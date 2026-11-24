"""Persistent OpenOCD GDB / Telnet Server Bridge for SWD debugging (works offline)."""

from __future__ import annotations

import shutil
import subprocess
from typing import Callable

from ..device.hardware import SWD_TARGETS, SwdProbe


class GdbSwdBridge:
    """Manages a local OpenOCD GDB/Telnet server process bound to a connected SWD probe."""

    def __init__(
        self,
        *,
        gdb_port: int = 3333,
        telnet_port: int = 4444,
        on_change: Callable[[], None] | None = None,
    ) -> None:
        self.gdb_port = gdb_port
        self.telnet_port = telnet_port
        self._on_change = on_change
        self._proc: subprocess.Popen[str] | None = None
        self.active_probe_name: str = ""

    @property
    def running(self) -> bool:
        return self._proc is not None and self._proc.poll() is None

    def start(self, probe: SwdProbe, target_key: str = "py32f071", *, gdb_port: int | None = None) -> int:
        if self.running:
            return self.gdb_port
        openocd = shutil.which("openocd")
        if not openocd:
            raise RuntimeError("openocd is not installed on PATH")
        if gdb_port is not None:
            self.gdb_port = int(gdb_port)
        tinfo = SWD_TARGETS.get(target_key, SWD_TARGETS["py32f071"])
        cmd = [
            openocd,
            "-f",
            probe.openocd_iface,
            "-c",
            "transport select swd",
            "-f",
            tinfo["openocd_cfg"],
            "-c",
            f"gdb_port {self.gdb_port}",
            "-c",
            f"telnet_port {self.telnet_port}",
            "-c",
            "tcl_port disabled",
        ]
        self._proc = subprocess.Popen(
            cmd,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            text=True,
        )
        self.active_probe_name = probe.name
        if self._on_change:
            self._on_change()
        return self.gdb_port

    def stop(self) -> None:
        proc = self._proc
        self._proc = None
        self.active_probe_name = ""
        if proc is not None and proc.poll() is None:
            try:
                proc.terminate()
                proc.wait(timeout=2.0)
            except Exception:
                try:
                    proc.kill()
                except Exception:
                    pass
        if self._on_change:
            self._on_change()
