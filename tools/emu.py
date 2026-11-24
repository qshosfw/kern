#!/usr/bin/env python3
"""
Kern Hardware Emulator (`./kern emu` & `from emu import KernEmulator`).

Streamlined, zero-Flask QEMU Cortex-M0+ (UV-K5v3 / UV-K1) runner built for:
  • Kern Dock       — exposes native RigWire TCP (`tcp://127.0.0.1:4446`) out of the box
  • AI Agents & CI  — scriptable Python context manager + `--daemon` + `--json` + ASCII LCD dump
  • Hardware Tests  — drop-in replacement for a physical radio (RigWire, KernFS, sideload, LCD, keys)
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import signal
import socket
import struct
import subprocess
import sys
import tempfile
import time
import zlib
from pathlib import Path
from typing import Any

TOOLS_DIR = Path(__file__).resolve().parent
REPO_ROOT = TOOLS_DIR.parent
if str(TOOLS_DIR) not in sys.path:
    sys.path.insert(0, str(TOOLS_DIR))

import cli_ui  # noqa: E402
import rigwire  # noqa: E402

DEFAULT_EMU_DIR = Path(os.environ.get("KERN_EMU_DIR", str(REPO_ROOT / "inspo" / "uv-k5-v3-emulator")))
DEFAULT_QEMU_BIN = Path(os.environ.get("KERN_QEMU_BIN", str(REPO_ROOT / "inspo" / "qemu-7.2" / "build" / "qemu-system-arm")))
DEFAULT_FLASH_IMG = DEFAULT_EMU_DIR / "assets" / "flash.img"
DEFAULT_ELF = REPO_ROOT / "target" / "kern_firmware.elf"
DEFAULT_STATE_FILE = Path(os.environ.get("KERN_EMU_STATE", "/tmp/kern-emu.json"))
DEFAULT_QMP_SOCK = "/tmp/kern-emu-qmp.sock"
DEFAULT_SERIAL_PORT = 4446
DEFAULT_GDB_PORT = 1234

# Map friendly key names to QMP keypad property names (when using --qmp hardware injection)
_QMP_KEY_MAP: dict[str, str] = {
    "menu": "MENU", "up": "UP", "down": "DOWN", "exit": "EXIT",
    "star": "STAR", "*": "STAR", "f": "F", "ptt": "PTT",
    "side1": "SIDE1", "side2": "SIDE2",
    **{str(d): str(d) for d in range(10)},
}


def find_qemu_binary() -> Path | None:
    if DEFAULT_QEMU_BIN.is_file():
        return DEFAULT_QEMU_BIN
    found = shutil.which("qemu-system-arm")
    return Path(found) if found else None


def ensure_flash_image(flash_path: Path = DEFAULT_FLASH_IMG) -> Path:
    if flash_path.is_file():
        return flash_path
    make_flash = DEFAULT_EMU_DIR / "tools" / "make_flash.py"
    if make_flash.is_file():
        subprocess.run([sys.executable, str(make_flash)], cwd=str(DEFAULT_EMU_DIR), check=True)
        if flash_path.is_file():
            return flash_path
    # Fallback: create blank 8 MiB SPI flash image (0xFF erased)
    flash_path.parent.mkdir(parents=True, exist_ok=True)
    with flash_path.open("wb") as f:
        f.write(b"\xFF" * (8 * 1024 * 1024))
    return flash_path


def ensure_firmware_elf(elf_path: Path = DEFAULT_ELF, *, rebuild: bool = False) -> Path:
    if rebuild or not elf_path.is_file():
        res = subprocess.run([sys.executable, str(TOOLS_DIR / "builder.py")], cwd=str(REPO_ROOT))
        if res.returncode != 0:
            raise RuntimeError("Firmware build failed")
    if not elf_path.is_file():
        raw_bin = REPO_ROOT / "target" / "firmware.bin"
        bin2elf = DEFAULT_EMU_DIR / "tools" / "bin2elf.py"
        if raw_bin.is_file() and bin2elf.is_file():
            subprocess.run([sys.executable, str(bin2elf), str(raw_bin), "-o", str(elf_path)], cwd=str(REPO_ROOT), check=True)
    if not elf_path.is_file():
        raise FileNotFoundError(f"Firmware ELF not found at {elf_path}")
    return elf_path


class QmpClient:
    """Zero-dependency QMP JSON client over Unix socket (or host:port TCP)."""

    def __init__(self, endpoint: str, timeout: float = 4.0) -> None:
        self.endpoint = endpoint
        self.timeout = timeout

    def _connect(self) -> socket.socket:
        host, _, port = self.endpoint.rpartition(":")
        if host and port.isdigit():
            s = socket.create_connection((host, int(port)), timeout=self.timeout)
        else:
            s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
            s.settimeout(self.timeout)
            s.connect(self.endpoint)
        f = s.makefile("rwb", buffering=0)
        f.readline()  # greeting
        f.write(b'{"execute":"qmp_capabilities"}\n')
        f.readline()  # capabilities ack
        return s

    def execute(self, command: str, arguments: dict[str, Any] | None = None) -> Any:
        s = self._connect()
        try:
            payload: dict[str, Any] = {"execute": command}
            if arguments:
                payload["arguments"] = arguments
            s.sendall((json.dumps(payload) + "\n").encode("utf-8"))
            buf = b""
            while True:
                chunk = s.recv(4096)
                if not chunk:
                    break
                buf += chunk
                for raw_line in buf.splitlines():
                    try:
                        obj = json.loads(raw_line.decode("utf-8"))
                    except Exception:
                        continue
                    if "return" in obj:
                        return obj["return"]
                    if "error" in obj:
                        raise RuntimeError(f"QMP {command}: {obj['error']}")
            return None
        finally:
            try:
                s.close()
            except Exception:
                pass

    def press_hardware_key(self, key: str, hold_ms: int = 120) -> None:
        qkey = _QMP_KEY_MAP.get(key.lower(), key.upper())
        self.execute("qom-set", {"path": "/machine/keypad", "property": f"key-{qkey}", "value": True})
        time.sleep(max(0.06, hold_ms / 1000.0))
        self.execute("qom-set", {"path": "/machine/keypad", "property": f"key-{qkey}", "value": False})

    def panel_state(self) -> dict[str, Any]:
        out: dict[str, Any] = {}
        for prop in ("display", "contrast", "invert"):
            try:
                out[prop] = self.execute("qom-get", {"path": "/machine/panel", "property": prop})
            except Exception:
                pass
        return out


def encode_fb_png(fb: bytes, *, scale: int = 4) -> bytes:
    """Render 1024-byte ST7565 framebuffer into a clean PNG (zero third-party deps)."""
    w, h = 128 * scale, 64 * scale
    raw = bytearray()
    for y in range(64):
        page_off = (y >> 3) * 128
        mask = 1 << (y & 7)
        row_px = bytearray()
        for x in range(128):
            on = bool(fb[page_off + x] & mask) if page_off + x < len(fb) else False
            # Classic UV-K1 teal/indigo palette
            rgb = b"\x24\x1f\x7e" if on else b"\xc9\xf7\xec"
            row_px.extend(rgb * scale)
        for _ in range(scale):
            raw.append(0)
            raw.extend(row_px)

    def _chunk(tag: bytes, payload: bytes) -> bytes:
        crc = zlib.crc32(tag + payload) & 0xFFFFFFFF
        return struct.pack(">I", len(payload)) + tag + payload + struct.pack(">I", crc)

    ihdr = struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0)
    return b"\x89PNG\r\n\x1a\n" + _chunk(b"IHDR", ihdr) + _chunk(b"IDAT", zlib.compress(bytes(raw), 6)) + _chunk(b"IEND", b"")


def render_fb_ascii(fb: bytes) -> str:
    """Compact half-block Unicode preview of the 128×64 ST7565 LCD (great for terminal & AI agents)."""
    lines: list[str] = []
    for y in range(0, 64, 2):
        p0, m0 = (y >> 3) * 128, 1 << (y & 7)
        p1, m1 = ((y + 1) >> 3) * 128, 1 << ((y + 1) & 7)
        chars: list[str] = []
        for x in range(0, 128, 2):
            top = bool(fb[p0 + x] & m0) if p0 + x < len(fb) else False
            bot = bool(fb[p1 + x] & m1) if p1 + x < len(fb) else False
            chars.append("█" if (top and bot) else ("▀" if top else ("▄" if bot else " ")))
        lines.append("│" + "".join(chars) + "│")
    border = "─" * 64
    return f"┌{border}┐\n" + "\n".join(lines) + f"\n└{border}┘"


def _pid_alive(pid: int) -> bool:
    try:
        os.kill(pid, 0)
        return True
    except OSError:
        return False


def load_running_state(state_file: Path = DEFAULT_STATE_FILE) -> dict[str, Any] | None:
    if not state_file.is_file():
        return None
    try:
        data = json.loads(state_file.read_text(encoding="utf-8"))
        pid = int(data.get("pid", 0))
        if pid > 0 and _pid_alive(pid):
            return data
    except Exception:
        pass
    try:
        state_file.unlink(missing_ok=True)
    except OSError:
        pass
    return None


class KernEmulator:
    """Scriptable, Dock-compatible Kern QEMU radio emulator controller."""

    def __init__(
        self,
        *,
        elf: Path | str | None = None,
        flash: Path | str | None = None,
        serial_port: int = DEFAULT_SERIAL_PORT,
        gdb_port: int = DEFAULT_GDB_PORT,
        qmp_path: str = DEFAULT_QMP_SOCK,
        ephemeral_flash: bool = False,
        boot_key: str | None = None,
        boot_key_ms: int = 1500,
        rebuild: bool = False,
        state_file: Path = DEFAULT_STATE_FILE,
    ) -> None:
        self.elf = Path(elf) if elf else DEFAULT_ELF
        self.flash = Path(flash) if flash else DEFAULT_FLASH_IMG
        self.serial_port = int(serial_port)
        self.gdb_port = int(gdb_port)
        self.qmp_path = qmp_path
        self.ephemeral_flash = ephemeral_flash
        self.boot_key = boot_key
        self.boot_key_ms = boot_key_ms
        self.rebuild = rebuild
        self.state_file = state_file

        self._proc: subprocess.Popen[bytes] | None = None
        self._tmp_flash: Path | None = None
        self._wire: rigwire.RigWireClient | None = None
        self.qmp = QmpClient(self.qmp_path)

    @property
    def wire_url(self) -> str:
        return f"tcp://127.0.0.1:{self.serial_port}"

    def start(self, *, wait_rigwire: bool = True, timeout: float = 15.0, detach: bool = False) -> dict[str, Any]:
        qemu = find_qemu_binary()
        if qemu is None:
            raise FileNotFoundError(
                f"qemu-system-arm not found at {DEFAULT_QEMU_BIN} or PATH (set KERN_QEMU_BIN)"
            )
        elf = ensure_firmware_elf(self.elf, rebuild=self.rebuild)
        base_flash = ensure_flash_image(self.flash)
        run_flash = base_flash
        if self.ephemeral_flash:
            fd, tmp_name = tempfile.mkstemp(prefix="kern-emu-flash-", suffix=".img")
            os.close(fd)
            shutil.copy2(base_flash, tmp_name)
            self._tmp_flash = Path(tmp_name)
            run_flash = self._tmp_flash

        if ":" not in self.qmp_path and os.path.exists(self.qmp_path):
            try:
                os.unlink(self.qmp_path)
            except OSError:
                pass

        env = dict(os.environ)
        env["UVK5_FLASH_IMAGE"] = str(run_flash)
        if self.boot_key:
            env["UVK5_BOOT_KEY"] = _QMP_KEY_MAP.get(self.boot_key.lower(), self.boot_key.upper())
            env["UVK5_BOOT_KEY_MS"] = str(self.boot_key_ms)

        cmd = [
            str(qemu),
            "-M", "uv-k5-v3",
            "-nographic",
            "-monitor", "none",
            "-qmp", f"unix:{self.qmp_path},server=on,wait=off",
            "-kernel", str(elf),
            "-gdb", f"tcp::{self.gdb_port}",
            "-serial", f"tcp:127.0.0.1:{self.serial_port},server=on,wait=off",
        ]
        self._proc = subprocess.Popen(
            cmd,
            env=env,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            start_new_session=detach,
        )

        # Wait for TCP RigWire socket to listen
        deadline = time.monotonic() + min(8.0, timeout)
        while time.monotonic() < deadline:
            if self._proc.poll() is not None:
                raise RuntimeError(f"QEMU exited early with code {self._proc.returncode}")
            try:
                socket.create_connection(("127.0.0.1", self.serial_port), timeout=0.25).close()
                break
            except OSError:
                time.sleep(0.08)
        else:
            self.stop()
            raise TimeoutError(f"QEMU serial port {self.serial_port} did not open within timeout")

        info = {
            "pid": self._proc.pid,
            "wire_url": self.wire_url,
            "serial_port": self.serial_port,
            "gdb_port": self.gdb_port,
            "qmp_path": self.qmp_path,
            "elf": str(elf),
            "flash": str(run_flash),
            "ephemeral": self.ephemeral_flash,
            "started_at": time.time(),
        }
        try:
            self.state_file.write_text(json.dumps(info, indent=2), encoding="utf-8")
        except OSError:
            pass

        if wait_rigwire:
            self.wait_ready(timeout=timeout)
        return info

    def wire(self, *, handshake: bool = True) -> rigwire.RigWireClient:
        if self._wire is None:
            cli = rigwire.RigWireClient(self.wire_url, timeout=2.5)
            if handshake:
                cli.hello()
            self._wire = cli
        return self._wire

    def release_wire(self) -> None:
        """Close the active RigWire TCP socket so Dock or another tool can attach."""
        if self._wire is not None:
            try:
                self._wire.close()
            except Exception:
                pass
            self._wire = None

    def wait_ready(self, timeout: float = 15.0) -> dict[str, Any]:
        """Wait until firmware boots and responds to RigWire HELLO."""
        deadline = time.monotonic() + timeout
        last_err: Exception | None = None
        while time.monotonic() < deadline:
            if self._proc is not None and self._proc.poll() is not None:
                raise RuntimeError(f"QEMU exited during boot (code {self._proc.returncode})")
            cli: rigwire.RigWireClient | None = None
            try:
                cli = rigwire.RigWireClient(self.wire_url, timeout=1.2)
                hello = cli.hello()
                cli.close()
                return hello
            except Exception as exc:
                last_err = exc
                if cli is not None:
                    try:
                        cli.close()
                    except Exception:
                        pass
            time.sleep(0.25)
        raise TimeoutError(f"RigWire did not become ready within {timeout:.1f}s ({last_err})")

    def press(self, key: str, *, hold_ms: int = 120, via_qmp: bool = False) -> None:
        if via_qmp:
            self.qmp.press_hardware_key(key, hold_ms=hold_ms)
            return
        cli = self.wire()
        code = rigwire.KEY_CODES.get(key.lower())
        if code is None:
            raise ValueError(f"Unknown key '{key}' (valid: {', '.join(sorted(rigwire.KEY_CODES))})")
        cli.inject_key(code, rigwire.KEY_STATES["press"])
        if hold_ms >= 500:
            time.sleep(0.25)
            cli.inject_key(code, rigwire.KEY_STATES["hold"])
            time.sleep(max(0.05, (hold_ms - 250) / 1000.0))
        else:
            time.sleep(max(0.04, hold_ms / 1000.0))
        cli.inject_key(code, rigwire.KEY_STATES["release"])

    def encoder(self, delta: int) -> None:
        self.wire().inject_encoder(int(delta))

    def framebuffer(self) -> bytes:
        return self.wire().screencap()

    def wait_frame_change(self, old_fb: bytes, *, timeout: float = 3.0, poll_s: float = 0.12) -> bytes | None:
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            cur = self.framebuffer()
            if cur != old_fb:
                return cur
            time.sleep(poll_s)
        return None

    def screenshot(self, dest: str | Path | None = None, *, scale: int = 4) -> bytes:
        fb = self.framebuffer()
        png = encode_fb_png(fb, scale=scale)
        if dest is not None:
            Path(dest).write_bytes(png)
        return png

    def ls(self, path: str = "/") -> list[dict[str, Any]]:
        return self.wire().ls(path)

    def sideload(self, pkg_path: str | Path, *, verify: bool = True, force: bool = True) -> dict[str, Any]:
        return self.wire().sideload(str(pkg_path), verify=verify, force=force)

    def reset(self) -> None:
        self.qmp.execute("system_reset")

    def stop(self) -> None:
        if self._wire is not None:
            try:
                self._wire.close()
            except Exception:
                pass
            self._wire = None
        if self._proc is not None and self._proc.poll() is None:
            try:
                self._proc.terminate()
                self._proc.wait(timeout=3.0)
            except Exception:
                try:
                    self._proc.kill()
                except Exception:
                    pass
        self._proc = None
        if self._tmp_flash is not None:
            self._tmp_flash.unlink(missing_ok=True)
            self._tmp_flash = None
        try:
            self.state_file.unlink(missing_ok=True)
        except OSError:
            pass

    def __enter__(self) -> KernEmulator:
        self.start()
        return self

    def __exit__(self, *_exc: Any) -> None:
        self.stop()


def stop_daemon(state_file: Path = DEFAULT_STATE_FILE) -> bool:
    st = load_running_state(state_file)
    if not st:
        return False
    pid = int(st["pid"])
    try:
        os.kill(pid, signal.SIGTERM)
        for _ in range(30):
            if not _pid_alive(pid):
                break
            time.sleep(0.1)
        else:
            os.kill(pid, signal.SIGKILL)
    except OSError:
        pass
    if st.get("ephemeral") and st.get("flash"):
        try:
            Path(st["flash"]).unlink(missing_ok=True)
        except OSError:
            pass
    state_file.unlink(missing_ok=True)
    return True


def _run_e2e_test(rebuild: bool = False) -> int:
    cli_ui.header("Kern Emulator Verification Suite", "Direct QEMU + RigWire + ST7565 LCD + Keypad")
    with KernEmulator(ephemeral_flash=True, rebuild=rebuild, serial_port=4456, gdb_port=1244, qmp_path="/tmp/kern-emu-test.sock", state_file=Path("/tmp/kern-emu-test.json")) as emu:
        hello = emu.wire().hello()
        cli_ui.ok(f"RigWire HELLO: {hello}")
        emu.wire().ping()
        cli_ui.ok("RigWire PING OK")

        root_entries = emu.ls("/")
        cli_ui.ok(f"KernFS ls / -> {[e['name'] for e in root_entries]}")

        fb0 = emu.framebuffer()
        if len(fb0) != 1024 or not any(fb0):
            cli_ui.err("LCD framebuffer empty or wrong size")
            return cli_ui.EXIT_ERR
        cli_ui.ok(f"ST7565 LCD active ({sum(bin(b).count('1') for b in fb0)} lit pixels)")

        emu.press("menu", hold_ms=100)
        time.sleep(0.35)
        fb1 = emu.framebuffer()
        cli_ui.ok(f"Key press injected (screen changed={fb1 != fb0})")
    cli_ui.ok("All emulator checks passed.")
    return cli_ui.EXIT_OK


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(
        prog="kern emu",
        description="Unified Kern hardware emulator (Dock-ready RigWire TCP + agent CLI + test harness).",
    )
    sub = ap.add_subparsers(dest="cmd")

    p_start = sub.add_parser("start", help="Launch QEMU emulator with RigWire TCP on 127.0.0.1:4446")
    p_start.add_argument("--daemon", "-d", action="store_true", help="Run in background and return once RigWire is ready")
    p_start.add_argument("--dock", action="store_true", help="Also launch Kern Dock attached to the emulator")
    p_start.add_argument("--port", "-p", type=int, default=DEFAULT_SERIAL_PORT, help="RigWire TCP port (default: 4446)")
    p_start.add_argument("--gdb", type=int, default=DEFAULT_GDB_PORT, help="GDB stub TCP port (default: 1234)")
    p_start.add_argument("--elf", help="Override firmware ELF path")
    p_start.add_argument("--flash", help="Override SPI flash image path")
    p_start.add_argument("--ephemeral", action="store_true", help="Boot from a temporary copy of flash.img")
    p_start.add_argument("--boot-key", help="Hold a physical key from reset (e.g. PTT, EXIT)")
    p_start.add_argument("--rebuild", "-r", action="store_true", help="Rebuild firmware before starting")
    p_start.add_argument("--no-wait", action="store_true", help="Do not wait for RigWire HELLO")
    p_start.add_argument("--json", action="store_true", help="Print startup metadata as JSON")

    sub.add_parser("stop", help="Stop background emulator daemon")

    p_st = sub.add_parser("status", help="Show running emulator status & live RigWire info")
    p_st.add_argument("--json", action="store_true")

    sub.add_parser("reset", help="Warm-reset running emulator via QMP")

    p_key = sub.add_parser("key", help="Send a keypad press to the running emulator")
    p_key.add_argument("name", help="Key name (menu, exit, up, down, 0-9, star, f, ptt, side1, side2)")
    p_key.add_argument("--hold-ms", type=int, default=120)
    p_key.add_argument("--qmp", action="store_true", help="Inject at QEMU hardware keypad level instead of RigWire")

    p_enc = sub.add_parser("encoder", help="Send rotary encoder steps (e.g. -1 or 1)")
    p_enc.add_argument("delta", type=int)

    p_shot = sub.add_parser("screenshot", help="Capture LCD PNG or ASCII art from running emulator")
    p_shot.add_argument("output", nargs="?", help="Output .png path (optional if --ascii)")
    p_shot.add_argument("--scale", type=int, default=4)
    p_shot.add_argument("--ascii", action="store_true", help="Print compact Unicode LCD art to stdout")

    p_sl = sub.add_parser("sideload", help="Sideload a .krx/.mod package into the running emulator")
    p_sl.add_argument("package", help="Path to .krx or .mod file")

    p_wire = sub.add_parser("wire", help="Run any rigwire.py subcommand against the running emulator")
    p_wire.add_argument("wire_args", nargs=argparse.REMAINDER)

    p_test = sub.add_parser("test", help="Run fast self-contained QEMU + RigWire E2E verification")
    p_test.add_argument("--rebuild", "-r", action="store_true")

    p_chk = sub.add_parser("check", help="Check QEMU binary, flash image & firmware ELF availability")
    p_chk.add_argument("--json", action="store_true")

    raw_args = list(sys.argv[1:] if argv is None else argv)
    # Default to `start` if invoked with no subcommand or legacy flags like `--rebuild`
    if not raw_args or raw_args[0].startswith("-"):
        if "-h" not in raw_args and "--help" not in raw_args:
            raw_args = ["start", *raw_args]
    args = ap.parse_args(raw_args)

    if args.cmd == "check":
        qemu = find_qemu_binary()
        res = {
            "qemu": str(qemu) if qemu else None,
            "flash": str(DEFAULT_FLASH_IMG) if DEFAULT_FLASH_IMG.is_file() else None,
            "elf": str(DEFAULT_ELF) if DEFAULT_ELF.is_file() else None,
            "running": load_running_state() is not None,
        }
        if args.json:
            print(json.dumps(res, indent=2))
        else:
            cli_ui.header("Kern Emulator Check")
            cli_ui.table(("Component", "Path"), ((k, str(v or "missing")) for k, v in res.items()))
        return cli_ui.EXIT_OK if res["qemu"] else cli_ui.EXIT_ERR

    if args.cmd == "stop":
        if stop_daemon():
            cli_ui.ok("Emulator stopped.")
        else:
            cli_ui.info("No running emulator daemon found.")
        return cli_ui.EXIT_OK

    if args.cmd == "status":
        st = load_running_state()
        if not st:
            if args.json:
                print(json.dumps({"running": False}))
            else:
                cli_ui.info("Emulator is not running. Start with: ./kern emu start --daemon")
            return cli_ui.EXIT_OK
        out = dict(st, running=True)
        try:
            cli = rigwire.RigWireClient(st["wire_url"], timeout=1.5)
            out["rigwire"] = cli.hello()
            cli.close()
        except Exception as exc:
            out["rigwire_error"] = str(exc)
        if args.json:
            print(json.dumps(out, indent=2))
        else:
            cli_ui.header("Kern Emulator Status", st["wire_url"])
            cli_ui.table(("Field", "Value"), ((k, str(v)) for k, v in out.items()))
        return cli_ui.EXIT_OK

    if args.cmd == "test":
        return _run_e2e_test(rebuild=args.rebuild)

    if args.cmd in ("reset", "key", "encoder", "screenshot", "sideload", "wire"):
        st = load_running_state()
        if not st:
            cli_ui.err("No running emulator found. Start one first: ./kern emu start --daemon")
            return cli_ui.EXIT_ERR
        emu = KernEmulator(serial_port=int(st["serial_port"]), qmp_path=str(st["qmp_path"]))
        try:
            if args.cmd == "reset":
                emu.reset()
                cli_ui.ok("Emulator reset.")
            elif args.cmd == "key":
                emu.press(args.name, hold_ms=args.hold_ms, via_qmp=args.qmp)
                cli_ui.ok(f"Key '{args.name}' sent ({args.hold_ms} ms).")
            elif args.cmd == "encoder":
                emu.encoder(args.delta)
                cli_ui.ok(f"Encoder delta {args.delta:+d} sent.")
            elif args.cmd == "screenshot":
                fb = emu.framebuffer()
                if args.ascii or not args.output:
                    print(render_fb_ascii(fb))
                if args.output:
                    Path(args.output).write_bytes(encode_fb_png(fb, scale=args.scale))
                    cli_ui.ok(f"Saved {args.output}")
            elif args.cmd == "sideload":
                info = emu.sideload(args.package)
                cli_ui.ok(f"Sideloaded {info['remote_path']}")
            elif args.cmd == "wire":
                wargs = [a for a in args.wire_args if a != "--"]
                return subprocess.call([sys.executable, str(TOOLS_DIR / "rigwire.py"), "--port", st["wire_url"], *wargs], cwd=str(REPO_ROOT))
        finally:
            emu.release_wire()
        return cli_ui.EXIT_OK

    if args.cmd == "start":
        if load_running_state():
            stop_daemon()
        emu = KernEmulator(
            elf=args.elf,
            flash=args.flash,
            serial_port=args.port,
            gdb_port=args.gdb,
            ephemeral_flash=args.ephemeral,
            boot_key=args.boot_key,
            rebuild=args.rebuild,
        )
        info = emu.start(wait_rigwire=not args.no_wait, detach=args.daemon)
        if args.json:
            print(json.dumps(info, indent=2))
        else:
            cli_ui.header("Kern Hardware Emulator Ready", info["wire_url"])
            cli_ui.ok(f"PID {info['pid']} · RigWire {info['wire_url']} · GDB :{info['gdb_port']}")
            cli_ui.info("Attach Dock: ./kern dock --port " + info["wire_url"])
        if args.daemon:
            return cli_ui.EXIT_OK
        if args.dock:
            dock_env = dict(os.environ)
            dock_src = str(REPO_ROOT / "dock" / "src")
            dock_env["PYTHONPATH"] = os.pathsep.join([dock_src, str(TOOLS_DIR), dock_env.get("PYTHONPATH", "")]).strip(os.pathsep)
            try:
                return subprocess.call([sys.executable, "-m", "kern_dock", "--port", info["wire_url"]], cwd=str(REPO_ROOT), env=dock_env)
            finally:
                emu.stop()
        cli_ui.info("Press Ctrl+C to stop emulator…")
        try:
            assert emu._proc is not None
            return emu._proc.wait()
        except KeyboardInterrupt:
            emu.stop()
            return cli_ui.EXIT_OK

    ap.print_help()
    return cli_ui.EXIT_OK


if __name__ == "__main__":
    cli_ui.run_main(main)
