#!/usr/bin/env python3
"""
In-House Kern Hardware Emulator CLI & Python SDK (`./kern emu` & `from emu import KernEmulator`).

Drives the native Rust `kern-emu` engine (`emu/` crate — ARMv6-M Cortex-M0+ + PY32F071xB +
ST7565 LCD + PY25Q16 2 MiB SPI Flash + BK4829 RF + 19-key matrix) with zero external/untracked
dependencies:
  • Kern Dock       — native RigWire TCP (`tcp://127.0.0.1:4446`) + 1-click auto-start
  • AI Agents & CI  — `--turbo` sub-second cold boot, JSON-RPC hardware control (`127.0.0.1:4445`),
                      direct GRAM screenshots (even during early boot prompts!), physical GPIO keys,
                      battery/RF telemetry injection, and `--flash-mode` fault testing
  • Hardware Tests  — drop-in replacement for a physical UV-K1 / UV-K5v3 radio
"""

from __future__ import annotations

import argparse
import json
import os
import signal
import socket
import struct
import subprocess
import sys
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

EMU_CRATE_DIR = REPO_ROOT / "emu"
EMU_TARGET_DIR = REPO_ROOT / "target" / "emu-host"
DEFAULT_FLASH_IMG = REPO_ROOT / "target" / "emu" / "flash.img"
DEFAULT_FIRMWARE_BIN = REPO_ROOT / "target" / "firmware.bin"
DEFAULT_FIRMWARE_ELF = REPO_ROOT / "target" / "thumbv6m-none-eabi" / "release" / "firmware"
DEFAULT_STATE_FILE = Path(os.environ.get("KERN_EMU_STATE", "/tmp/kern-emu.json"))
DEFAULT_SERIAL_PORT = 4446
DEFAULT_CTL_PORT = 4445


def _host_rust_target() -> str:
    res = subprocess.run(
        ["rustc", "-vV"],
        capture_output=True,
        text=True,
        env=cli_ui.cargo_env(),
        check=True,
    )
    for line in res.stdout.splitlines():
        if line.startswith("host:"):
            return line.split(":", 1)[1].strip()
    raise RuntimeError("Could not determine host target from `rustc -vV`")


def _emu_binary_path(host_target: str | None = None) -> Path:
    ht = host_target or _host_rust_target()
    exe = "kern-emu.exe" if os.name == "nt" else "kern-emu"
    return EMU_TARGET_DIR / ht / "release" / exe


def ensure_emu_binary(*, force_rebuild: bool = False) -> Path:
    """Build the in-house Rust `kern-emu` binary on demand if missing or sources changed."""
    ht = _host_rust_target()
    bin_path = _emu_binary_path(ht)
    need_build = force_rebuild or not bin_path.is_file()
    if not need_build:
        bin_mtime = bin_path.stat().st_mtime
        srcs = [EMU_CRATE_DIR / "Cargo.toml", *sorted((EMU_CRATE_DIR / "src").glob("*.rs"))]
        if any(s.is_file() and s.stat().st_mtime > bin_mtime for s in srcs):
            need_build = True
    if need_build:
        env = cli_ui.cargo_env({"CARGO_TARGET_DIR": str(EMU_TARGET_DIR)})
        res = subprocess.run(
            [
                "cargo",
                "build",
                "--release",
                "--manifest-path",
                str(EMU_CRATE_DIR / "Cargo.toml"),
                "--target",
                ht,
            ],
            cwd=str(REPO_ROOT),
            env=env,
            capture_output=True,
            text=True,
        )
        if res.returncode != 0:
            raise RuntimeError(f"Failed to compile kern-emu:\n{res.stderr}")
    return bin_path


def resolve_firmware_image(explicit: str | Path | None = None, *, rebuild: bool = False) -> Path:
    if explicit:
        p = Path(explicit)
        if not p.is_file():
            raise FileNotFoundError(f"Firmware image not found: {p}")
        return p
    if rebuild or (not DEFAULT_FIRMWARE_ELF.is_file() and not DEFAULT_FIRMWARE_BIN.is_file()):
        res = subprocess.run([sys.executable, str(TOOLS_DIR / "builder.py")], cwd=str(REPO_ROOT), env=cli_ui.cargo_env())
        if res.returncode != 0:
            raise RuntimeError("Firmware build failed")
    if DEFAULT_FIRMWARE_ELF.is_file() and DEFAULT_FIRMWARE_BIN.is_file():
        return (
            DEFAULT_FIRMWARE_ELF
            if DEFAULT_FIRMWARE_ELF.stat().st_mtime >= DEFAULT_FIRMWARE_BIN.stat().st_mtime
            else DEFAULT_FIRMWARE_BIN
        )
    if DEFAULT_FIRMWARE_ELF.is_file():
        return DEFAULT_FIRMWARE_ELF
    if DEFAULT_FIRMWARE_BIN.is_file():
        return DEFAULT_FIRMWARE_BIN
    raise FileNotFoundError("No compiled firmware found in target/ (run `./kern build`)")


def encode_fb_png(fb: bytes, *, scale: int = 4, invert: bool = False) -> bytes:
    """Render 1024-byte ST7565 framebuffer into a clean PNG (zero third-party deps)."""
    w, h = 128 * scale, 64 * scale
    raw = bytearray()
    for y in range(64):
        page_off = (y >> 3) * 128
        mask = 1 << (y & 7)
        row_px = bytearray()
        for x in range(128):
            bit = bool(fb[page_off + x] & mask) if page_off + x < len(fb) else False
            on = (not bit) if invert else bit
            rgb = b"\x1f\x23\x2b" if on else b"\xc8\xd8\xe4"
            row_px.extend(rgb * scale)
        for _ in range(scale):
            raw.append(0)
            raw.extend(row_px)

    def _chunk(tag: bytes, payload: bytes) -> bytes:
        crc = zlib.crc32(tag + payload) & 0xFFFFFFFF
        return struct.pack(">I", len(payload)) + tag + payload + struct.pack(">I", crc)

    ihdr = struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0)
    return b"\x89PNG\r\n\x1a\n" + _chunk(b"IHDR", ihdr) + _chunk(b"IDAT", zlib.compress(bytes(raw), 6)) + _chunk(b"IEND", b"")


def render_fb_ascii(fb: bytes, *, invert: bool = False) -> str:
    """Compact half-block Unicode preview of the 128×64 ST7565 LCD (ideal for terminal & AI agents)."""
    lines: list[str] = []
    for y in range(0, 64, 2):
        p0, m0 = (y >> 3) * 128, 1 << (y & 7)
        p1, m1 = ((y + 1) >> 3) * 128, 1 << ((y + 1) & 7)
        chars: list[str] = []
        for x in range(0, 128, 2):
            t = bool(fb[p0 + x] & m0) if p0 + x < len(fb) else False
            b = bool(fb[p1 + x] & m1) if p1 + x < len(fb) else False
            if invert:
                t, b = not t, not b
            chars.append("█" if (t and b) else ("▀" if t else ("▄" if b else " ")))
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


class EmuControlClient:
    """Zero-dependency JSON-RPC client for `kern-emu`'s hardware control port (`127.0.0.1:4445`)."""

    def __init__(self, port: int = DEFAULT_CTL_PORT, host: str = "127.0.0.1", timeout: float = 3.0) -> None:
        self.host = host
        self.port = int(port)
        self.timeout = timeout

    def call(self, cmd: str, **kwargs: Any) -> dict[str, Any]:
        payload = {"cmd": cmd, **kwargs}
        with socket.create_connection((self.host, self.port), timeout=self.timeout) as sock:
            sock.sendall((json.dumps(payload) + "\n").encode("utf-8"))
            line = sock.makefile("r", encoding="utf-8").readline()
            if not line:
                raise RuntimeError(f"Empty response from kern-emu ctl ({cmd})")
            resp = json.loads(line)
            if not resp.get("ok", True):
                raise RuntimeError(resp.get("error", f"kern-emu {cmd} failed"))
            return resp


class KernEmulator:
    """Scriptable, Dock-compatible in-house Kern hardware emulator controller."""

    def __init__(
        self,
        *,
        firmware: Path | str | None = None,
        flash: Path | str | None = None,
        serial_port: int = DEFAULT_SERIAL_PORT,
        ctl_port: int = DEFAULT_CTL_PORT,
        ephemeral_flash: bool = False,
        flash_mode: str = "normal",
        boot_key: str | None = None,
        boot_key_ms: int = 1500,
        turbo: bool = False,
        rebuild: bool = False,
        state_file: Path = DEFAULT_STATE_FILE,
    ) -> None:
        self.firmware_arg = firmware
        self.flash = Path(flash) if flash else DEFAULT_FLASH_IMG
        self.serial_port = int(serial_port)
        self.ctl_port = int(ctl_port)
        self.ephemeral_flash = ephemeral_flash
        self.flash_mode = flash_mode
        self.boot_key = boot_key
        self.boot_key_ms = boot_key_ms
        self.turbo = turbo
        self.rebuild = rebuild
        self.state_file = state_file

        self._proc: subprocess.Popen[bytes] | None = None
        self._wire: rigwire.RigWireClient | None = None
        self.ctl = EmuControlClient(self.ctl_port)

    @property
    def wire_url(self) -> str:
        return f"tcp://127.0.0.1:{self.serial_port}"

    def start(self, *, wait_rigwire: bool = True, timeout: float = 10.0, detach: bool = False) -> dict[str, Any]:
        emu_bin = ensure_emu_binary()
        fw_path = resolve_firmware_image(self.firmware_arg, rebuild=self.rebuild)

        cmd = [
            str(emu_bin),
            "--firmware",
            str(fw_path),
            "--serial-port",
            str(self.serial_port),
            "--ctl-port",
            str(self.ctl_port),
            "--flash-mode",
            self.flash_mode,
        ]
        if self.ephemeral_flash:
            cmd.append("--ephemeral")
        else:
            self.flash.parent.mkdir(parents=True, exist_ok=True)
            cmd.extend(["--flash", str(self.flash)])
        if self.boot_key:
            cmd.extend(["--boot-key", self.boot_key, "--boot-key-ms", str(self.boot_key_ms)])
        if self.turbo:
            cmd.append("--turbo")

        self._proc = subprocess.Popen(
            cmd,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            start_new_session=detach,
        )

        deadline = time.monotonic() + min(5.0, timeout)
        while time.monotonic() < deadline:
            if self._proc.poll() is not None:
                raise RuntimeError(f"kern-emu exited early with code {self._proc.returncode}")
            try:
                self.ctl.call("status")
                break
            except OSError:
                time.sleep(0.02)
        else:
            self.stop()
            raise TimeoutError(f"kern-emu control port {self.ctl_port} did not open in time")

        info = {
            "engine": "kern-emu",
            "pid": self._proc.pid,
            "wire_url": self.wire_url,
            "serial_port": self.serial_port,
            "ctl_port": self.ctl_port,
            "firmware": str(fw_path),
            "flash": None if self.ephemeral_flash else str(self.flash),
            "flash_mode": self.flash_mode,
            "ephemeral": self.ephemeral_flash,
            "turbo": self.turbo,
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
        if self._wire is not None:
            try:
                self._wire.close()
            except Exception:
                pass
            self._wire = None

    def wait_ready(self, timeout: float = 10.0) -> dict[str, Any]:
        deadline = time.monotonic() + timeout
        last_err: Exception | None = None
        while time.monotonic() < deadline:
            if self._proc is not None and self._proc.poll() is not None:
                raise RuntimeError(f"kern-emu exited during boot (code {self._proc.returncode})")
            cli: rigwire.RigWireClient | None = None
            try:
                cli = rigwire.RigWireClient(self.wire_url, timeout=0.8)
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
            time.sleep(0.08)
        raise TimeoutError(f"RigWire did not become ready within {timeout:.1f}s ({last_err})")

    def status(self) -> dict[str, Any]:
        return self.ctl.call("status")

    def serial_lines(self) -> list[str]:
        return list(self.ctl.call("serial_log").get("lines", []))

    def wait_virtual_ms(self, delta_ms: int, *, timeout: float = 5.0) -> None:
        """Wait until emulated MCU virtual clock advances by at least `delta_ms`."""
        start_ms = int(self.status().get("virtual_ms", 0))
        target_ms = start_ms + max(0, int(delta_ms))
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            if int(self.status().get("virtual_ms", 0)) >= target_ms:
                return
            time.sleep(0.01)

    def press(self, key: str, *, hold_ms: int = 120, hardware: bool = False, wait: bool = True) -> None:
        """Send a keypad press (via direct GPIO matrix when `hardware=True`, else RigWire)."""
        if hardware:
            self.ctl.call("press_key", key=key, hold_ms=int(hold_ms))
            if wait and hold_ms > 0:
                self.wait_virtual_ms(int(hold_ms) + 40, timeout=max(3.0, hold_ms / 250.0))
            return
        cli = self.wire()
        code = rigwire.KEY_CODES.get(key.lower())
        if code is None:
            raise ValueError(f"Unknown key '{key}' (valid: {', '.join(sorted(rigwire.KEY_CODES))})")
        cli.inject_key(code, rigwire.KEY_STATES["press"])
        if hold_ms >= 500:
            time.sleep(0.22)
            cli.inject_key(code, rigwire.KEY_STATES["hold"])
            time.sleep(max(0.05, (hold_ms - 220) / 1000.0))
        else:
            time.sleep(max(0.04, hold_ms / 1000.0))
        cli.inject_key(code, rigwire.KEY_STATES["release"])

    def encoder(self, delta: int) -> None:
        self.wire().inject_encoder(int(delta))

    def framebuffer(self, *, direct: bool = True) -> bytes:
        """Read 1024-byte ST7565 GRAM directly (works even before RigWire starts!) or over RigWire."""
        if direct:
            resp = self.ctl.call("framebuffer")
            return bytes.fromhex(resp["hex"])
        return self.wire().screencap()

    def wait_frame_change(self, old_fb: bytes, *, timeout: float = 3.0, poll_s: float = 0.05) -> bytes | None:
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            cur = self.framebuffer(direct=True)
            if cur != old_fb:
                return cur
            time.sleep(poll_s)
        return None

    def screenshot(self, dest: str | Path | None = None, *, scale: int = 4) -> bytes:
        resp = self.ctl.call("framebuffer")
        fb = bytes.fromhex(resp["hex"])
        png = encode_fb_png(fb, scale=scale, invert=bool(resp.get("invert", False)))
        if dest is not None:
            Path(dest).write_bytes(png)
        return png

    def ls(self, path: str = "/") -> list[dict[str, Any]]:
        return self.wire().ls(path)

    def sideload(self, pkg_path: str | Path, *, verify: bool = True, force: bool = True) -> dict[str, Any]:
        return self.wire().sideload(str(pkg_path), verify=verify, force=force)

    def set_battery_mv(self, mv: int) -> None:
        self.ctl.call("set_battery_mv", mv=int(mv))

    def set_rf_signal(self, rssi_dbm: int, carrier: bool = True) -> None:
        self.ctl.call("set_rf", rssi_dbm=int(rssi_dbm), carrier=bool(carrier))

    def set_turbo(self, enabled: bool) -> None:
        self.ctl.call("set_turbo", turbo=bool(enabled))

    def reset(self, *, warm: bool = False) -> None:
        self.release_wire()
        self.ctl.call("reset", warm=warm)

    def stop(self) -> None:
        self.release_wire()
        try:
            self.ctl.call("quit")
        except Exception:
            pass
        if self._proc is not None and self._proc.poll() is None:
            try:
                self._proc.wait(timeout=1.5)
            except Exception:
                try:
                    self._proc.terminate()
                    self._proc.wait(timeout=1.5)
                except Exception:
                    try:
                        self._proc.kill()
                    except Exception:
                        pass
        self._proc = None
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
    ctl_port = int(st.get("ctl_port", DEFAULT_CTL_PORT))
    try:
        EmuControlClient(ctl_port, timeout=1.0).call("quit")
    except Exception:
        pass
    pid = int(st["pid"])
    for _ in range(25):
        if not _pid_alive(pid):
            break
        time.sleep(0.05)
    else:
        try:
            os.kill(pid, signal.SIGTERM)
        except OSError:
            pass
    state_file.unlink(missing_ok=True)
    return True


def _provision_core_packages_if_present(emu: KernEmulator) -> list[str]:
    installed: list[str] = []
    for rel in (
        Path("packages/core/vfo/vfo.krx"),
        Path("packages/core/settings/settings.krx"),
    ):
        full = REPO_ROOT / rel
        if full.is_file():
            try:
                emu.sideload(full, verify=True, force=True)
                installed.append(rel.name)
            except Exception:
                pass
    return installed


def _run_e2e_test(rebuild: bool = False) -> int:
    cli_ui.header("Kern In-House Emulator Verification", "ARMv6-M + PY32F071 + ST7565 + PY25Q16 + BK4829 + RigWire")
    t0 = time.monotonic()
    emu = KernEmulator(
        ephemeral_flash=True,
        flash_mode="blank",
        turbo=True,
        rebuild=rebuild,
        serial_port=4456,
        ctl_port=4455,
        state_file=Path("/tmp/kern-emu-test.json"),
    )
    emu.start(wait_rigwire=False)
    try:
        # Handle legacy pre-refactor boot prompts gracefully if present, else waits directly for RigWire
        deadline = time.monotonic() + 8.0
        while time.monotonic() < deadline:
            emu.wait_virtual_ms(250, timeout=1.0)
            try:
                cli = rigwire.RigWireClient(emu.wire_url, timeout=0.35)
                hello = cli.hello()
                cli.close()
                break
            except Exception:
                emu.press("menu", hold_ms=1800, hardware=True)
                emu.wait_virtual_ms(300, timeout=1.5)
                emu.press("menu", hold_ms=150, hardware=True)
                emu.wait_virtual_ms(250, timeout=1.0)
        else:
            cli_ui.err("Firmware did not reach RigWire ready state")
            return cli_ui.EXIT_ERR

        st = emu.status()
        cli_ui.ok(f"Engine booted @ {st.get('mips')} MIPS (virtual {st.get('virtual_ms')} ms)")
        cli_ui.ok(f"UART banner: {emu.serial_lines()}")
        cli_ui.ok(f"RigWire HELLO: {hello}")

        emu.wire().ping()
        cli_ui.ok("RigWire PING OK")

        root_entries = [e["name"] for e in emu.ls("/")]
        cli_ui.ok(f"KernFS ls / -> {root_entries}")

        fb0 = emu.framebuffer(direct=False)
        if len(fb0) != 1024 or not any(fb0):
            cli_ui.err("LCD framebuffer empty")
            return cli_ui.EXIT_ERR
        cli_ui.ok(f"ST7565 LCD active ({sum(bin(b).count('1') for b in fb0)} lit pixels)")

        emu.press("down", hold_ms=80)
        fb1 = emu.wait_frame_change(fb0, timeout=2.0)
        cli_ui.ok(f"Navigation key injected (screen updated={fb1 is not None})")

        installed = _provision_core_packages_if_present(emu)
        if installed:
            cli_ui.ok(f"Sideloaded core packages: {installed}")
    finally:
        emu.stop()

    cli_ui.ok(f"All in-house emulator checks passed in {time.monotonic() - t0:.2f}s.")
    return cli_ui.EXIT_OK


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(
        prog="kern emu",
        description="In-house high-speed Kern hardware emulator (Dock-ready RigWire TCP + agent CLI + test harness).",
    )
    sub = ap.add_subparsers(dest="cmd")

    p_start = sub.add_parser("start", help="Launch in-house kern-emu with RigWire TCP on 127.0.0.1:4446")
    p_start.add_argument("--daemon", "-d", action="store_true", help="Run in background and return once ready")
    p_start.add_argument("--dock", action="store_true", help="Also launch Kern Dock attached to the emulator")
    p_start.add_argument("--port", "-p", type=int, default=DEFAULT_SERIAL_PORT, help="RigWire TCP port (default: 4446)")
    p_start.add_argument("--ctl-port", type=int, default=DEFAULT_CTL_PORT, help="Hardware control JSON-RPC port (default: 4445)")
    p_start.add_argument("--firmware", "--elf", dest="firmware", help="Override firmware ELF or .bin path")
    p_start.add_argument("--flash", help="Override 2 MiB SPI flash image path (default: target/emu/flash.img)")
    p_start.add_argument("--ephemeral", action="store_true", help="Run in-memory without writing flash.img")
    p_start.add_argument(
        "--flash-mode",
        default="normal",
        choices=("normal", "blank", "blank-no-calib", "corrupt"),
        help="Initial SPI flash state for testing first-boot/recovery flows",
    )
    p_start.add_argument("--boot-key", help="Hold a physical key from power-on (e.g. menu, exit, ptt)")
    p_start.add_argument("--turbo", action="store_true", help="Unthrottled virtual clock when idle (instant boot/delays)")
    p_start.add_argument("--provision", action="store_true", help="Auto-sideload core vfo.krx & settings.krx on start if missing")
    p_start.add_argument("--rebuild", "-r", action="store_true", help="Rebuild firmware before starting")
    p_start.add_argument("--no-wait", action="store_true", help="Return immediately as soon as hardware control port opens")
    p_start.add_argument("--json", action="store_true", help="Print startup metadata as JSON")

    sub.add_parser("stop", help="Stop background emulator daemon")

    p_st = sub.add_parser("status", help="Show running emulator hardware & RigWire status")
    p_st.add_argument("--json", action="store_true")

    p_rst = sub.add_parser("reset", help="Reset running emulator (cold power-cycle or --warm SYSRESETREQ)")
    p_rst.add_argument("--warm", action="store_true", help="Preserve SRAM .uninit & backup registers")

    p_key = sub.add_parser("key", help="Send a keypad press to the running emulator")
    p_key.add_argument("name", help="Key name (menu, exit, up, down, 0-9, star, f, ptt, side1, side2)")
    p_key.add_argument("--hold-ms", type=int, default=120)
    p_key.add_argument("--hw", "--gpio", dest="hw", action="store_true", help="Inject via physical GPIO matrix (works on boot dialogs)")

    p_enc = sub.add_parser("encoder", help="Send rotary encoder steps (e.g. -1 or 1)")
    p_enc.add_argument("delta", type=int)

    p_shot = sub.add_parser("screenshot", help="Capture ST7565 LCD PNG or ASCII art from running emulator")
    p_shot.add_argument("output", nargs="?", help="Output .png path (optional if --ascii)")
    p_shot.add_argument("--scale", type=int, default=4)
    p_shot.add_argument("--ascii", action="store_true", help="Print compact Unicode LCD art to stdout")

    p_sl = sub.add_parser("sideload", help="Sideload a .krx/.mod package into the running emulator")
    p_sl.add_argument("package", help="Path to .krx or .mod file")

    p_bat = sub.add_parser("battery", help="Override simulated battery voltage in mV (5800..8500)")
    p_bat.add_argument("mv", type=int)

    p_rf = sub.add_parser("rf", help="Inject simulated RF RSSI (dBm) and carrier detect on BK4829")
    p_rf.add_argument("rssi_dbm", type=int)
    p_rf.add_argument("--no-carrier", action="store_true")

    p_wire = sub.add_parser("wire", help="Run any rigwire.py subcommand against the running emulator")
    p_wire.add_argument("wire_args", nargs=argparse.REMAINDER)

    p_test = sub.add_parser("test", help="Run fast self-contained in-house emulator E2E verification")
    p_test.add_argument("--rebuild", "-r", action="store_true")

    p_chk = sub.add_parser("check", help="Build/verify in-house kern-emu binary & firmware availability")
    p_chk.add_argument("--json", action="store_true")

    raw_args = list(sys.argv[1:] if argv is None else argv)
    if not raw_args or raw_args[0].startswith("-"):
        if "-h" not in raw_args and "--help" not in raw_args:
            raw_args = ["start", *raw_args]
    args = ap.parse_args(raw_args)

    if args.cmd == "check":
        emu_bin = ensure_emu_binary()
        fw = None
        try:
            fw = str(resolve_firmware_image())
        except Exception:
            pass
        res = {
            "engine": "kern-emu",
            "binary": str(emu_bin),
            "firmware": fw,
            "flash": str(DEFAULT_FLASH_IMG) if DEFAULT_FLASH_IMG.is_file() else "auto-created on first start",
            "running": load_running_state() is not None,
        }
        if args.json:
            print(json.dumps(res, indent=2))
        else:
            cli_ui.header("Kern In-House Emulator Check")
            cli_ui.table(("Component", "Status"), ((k, str(v or "missing")) for k, v in res.items()))
        return cli_ui.EXIT_OK if (emu_bin.is_file() and fw) else cli_ui.EXIT_ERR

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
            out["hw"] = EmuControlClient(int(st.get("ctl_port", DEFAULT_CTL_PORT)), timeout=1.5).call("status")
        except Exception as exc:
            out["hw_error"] = str(exc)
        try:
            cli = rigwire.RigWireClient(st["wire_url"], timeout=1.0)
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

    if args.cmd in ("reset", "key", "encoder", "screenshot", "sideload", "battery", "rf", "wire"):
        st = load_running_state()
        if not st:
            cli_ui.err("No running emulator found. Start one first: ./kern emu start --daemon")
            return cli_ui.EXIT_ERR
        emu = KernEmulator(
            serial_port=int(st["serial_port"]),
            ctl_port=int(st.get("ctl_port", DEFAULT_CTL_PORT)),
        )
        try:
            if args.cmd == "reset":
                emu.reset(warm=args.warm)
                cli_ui.ok(f"Emulator {'warm' if args.warm else 'cold'} reset.")
            elif args.cmd == "key":
                emu.press(args.name, hold_ms=args.hold_ms, hardware=args.hw)
                cli_ui.ok(f"Key '{args.name}' sent ({args.hold_ms} ms).")
            elif args.cmd == "encoder":
                emu.encoder(args.delta)
                cli_ui.ok(f"Encoder delta {args.delta:+d} sent.")
            elif args.cmd == "screenshot":
                resp = emu.ctl.call("framebuffer")
                fb = bytes.fromhex(resp["hex"])
                inv = bool(resp.get("invert", False))
                if args.ascii or not args.output:
                    print(render_fb_ascii(fb, invert=inv))
                if args.output:
                    Path(args.output).write_bytes(encode_fb_png(fb, scale=args.scale, invert=inv))
                    cli_ui.ok(f"Saved {args.output}")
            elif args.cmd == "sideload":
                info = emu.sideload(args.package)
                cli_ui.ok(f"Sideloaded {info['remote_path']}")
            elif args.cmd == "battery":
                emu.set_battery_mv(args.mv)
                cli_ui.ok(f"Battery voltage set to {args.mv} mV.")
            elif args.cmd == "rf":
                emu.set_rf_signal(args.rssi_dbm, carrier=not args.no_carrier)
                cli_ui.ok(f"RF signal set to {args.rssi_dbm} dBm (carrier={not args.no_carrier}).")
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
            firmware=args.firmware,
            flash=args.flash,
            serial_port=args.port,
            ctl_port=args.ctl_port,
            ephemeral_flash=args.ephemeral,
            flash_mode=args.flash_mode,
            boot_key=args.boot_key,
            turbo=args.turbo,
            rebuild=args.rebuild,
        )
        wait_rw = (not args.no_wait) and (args.flash_mode == "normal")
        # If first-time `target/emu/flash.img` doesn't exist yet, don't block before OS auto-formats or prompts
        if wait_rw and not args.ephemeral and not Path(args.flash or DEFAULT_FLASH_IMG).is_file():
            wait_rw = False
        info = emu.start(wait_rigwire=wait_rw, detach=args.daemon)
        if args.provision:
            try:
                emu.wait_ready(timeout=5.0)
                existing = {e["name"] for e in emu.ls("/apps")}
                if "vfo.krx" not in existing or "settings.krx" not in existing:
                    _provision_core_packages_if_present(emu)
            except Exception:
                pass
            finally:
                emu.release_wire()
        if args.json:
            print(json.dumps(info, indent=2))
        else:
            cli_ui.header("Kern In-House Emulator Ready", info["wire_url"])
            cli_ui.ok(f"PID {info['pid']} · RigWire {info['wire_url']} · Control 127.0.0.1:{info['ctl_port']}")
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
