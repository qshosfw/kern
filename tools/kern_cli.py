#!/usr/bin/env python3
"""
Cross-platform unified developer CLI for Kern (`./kern` on Linux/macOS, `kern.cmd` on Windows).
"""

from __future__ import annotations

import difflib
import json
import os
import platform
import shutil
import subprocess
import sys
from pathlib import Path

TOOLS_DIR = Path(__file__).resolve().parent
REPO_ROOT = TOOLS_DIR.parent
if str(TOOLS_DIR) not in sys.path:
    sys.path.insert(0, str(TOOLS_DIR))

import cli_ui  # noqa: E402

CLI_VERSION = "2.0.0"

COMMAND_GROUPS: list[tuple[str, list[tuple[str, str]]]] = [
    (
        "Firmware & Build",
        [
            ("build", "Build firmware using kern.toml configuration (default)"),
            ("check", "Fast type and syntax check without emitting binary"),
            ("size", "Display Flash and SRAM section memory breakdown"),
            ("info", "Display target MCU, device, and active feature gates"),
            ("flash", "Build and flash firmware to radio via bootloader"),
            ("clean", "Clean target build directory and firmware outputs"),
            ("distclean", "Deep clean target/, package build caches & dock/build"),
        ],
    ),
    (
        "Quality & Analysis",
        [
            ("test", "Run native host unit tests across the workspace"),
            ("fmt", "Format all Rust code in the workspace (alias: format)"),
            ("lint", "Run static architectural linter (tools/archlint.py)"),
            ("bloat", "Run ELF symbol, section, and rodata bloat analyzer"),
            ("disasm", "Disassemble compiled Cortex-M0+ release firmware"),
        ],
    ),
    (
        "Device & Radio Tools",
        [
            ("dock", "Launch graphical hardware companion (live LCD, keypad, mount)"),
            ("wire", "RigWire serial host tool & console (alias: monitor)"),
            ("provision", "Provision .krx/.mod package bundles over RigWire"),
            ("mount", "Mount radio KernFS over RigWire via cross-platform WebDAV"),
            ("ports", "List connected USB & serial radio ports (alias: devices)"),
            ("emu", "Launch QEMU hardware emulator with interactive Web UI"),
        ],
    ),
    (
        "Packages, Assets & Diagnostics",
        [
            ("pkg", "Build or list KRX packages under packages/ (--list, --help)"),
            ("abi", "Regenerate or verify sdk/c/krx_ops.h from src/abi"),
            ("kfw", "Pack, inspect, verify, sign, or extract .kfw containers"),
            ("media", "Compile & inspect .icn / .fnt / .lng binary asset packs"),
            ("doctor", "Check cross-platform toolchain, Python, Dock & USB health"),
        ],
    ),
]


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
    raise RuntimeError("Could not determine host Rust target from `rustc -vV`")


def _run_py(script: str, args: list[str], extra_env: dict[str, str] | None = None) -> int:
    env = cli_ui.cargo_env(extra_env)
    return subprocess.call([sys.executable, str(TOOLS_DIR / script), *args], cwd=str(REPO_ROOT), env=env)


def _run_exec(cmd: list[str], extra_env: dict[str, str] | None = None) -> int:
    env = cli_ui.cargo_env(extra_env)
    return subprocess.call(cmd, cwd=str(REPO_ROOT), env=env)


def _print_help() -> int:
    cli_ui.header(f"Kern Developer CLI v{CLI_VERSION}", "Cross-platform firmware, RigWire, KernFS & Dock toolchain")
    print(f"\n  {cli_ui.bold('Usage:')}  ./kern <command> [options]   {cli_ui.gray('(or kern.cmd on Windows)')}")
    for group, items in COMMAND_GROUPS:
        cli_ui.section(group)
        for cmd, desc in items:
            print(f"    {cli_ui.cyan(f'{cmd:<10}')}  {desc}")
    cli_ui.section("Global Flags & Builder Options")
    for flag, desc in (
        ("-h, --help", "Show this help message (or `<cmd> --help` for subcommand help)"),
        ("-V, --version", "Show CLI version"),
        ("--no-color", "Disable ANSI colors (also respects NO_COLOR=1)"),
        ("--target, -t", "Override device target (UV-K1, UV-K5v3, UV-K10Pro)"),
        ("--preset", "Build preset ('full', 'lean', 'security')"),
        ("--port, -p", "Serial device port (e.g. /dev/ttyACM0, COM3, /dev/cu.usbmodem*)"),
        ("--json FILE", "Export structured Flash/SRAM telemetry to JSON"),
    ):
        print(f"    {cli_ui.gray(f'{flag:<14}')}  {desc}")
    print("")
    return cli_ui.EXIT_OK


def _run_ports(args: list[str]) -> int:
    as_json = "--json" in args
    show_all = "--all" in args or "-a" in args
    if "-h" in args or "--help" in args:
        print("Usage: ./kern ports [--all] [--json]\n\nList connected USB and serial radio ports across Linux, macOS, and Windows.")
        return cli_ui.EXIT_OK
    try:
        from serial.tools import list_ports  # type: ignore[import-not-found]
    except ImportError:
        cli_ui.err("pyserial is not installed (run: pip install pyserial)")
        return cli_ui.EXIT_ERR

    rows = []
    for p in sorted(list_ports.comports(), key=lambda x: x.device):
        is_usb = p.vid is not None or cli_ui.is_usb_serial_device(p.device)
        if not show_all and not is_usb:
            continue
        vid_pid = f"{p.vid:04x}:{p.pid:04x}" if (p.vid is not None and p.pid is not None) else "—"
        if vid_pid == "36b7:ffff":
            mode = "RigWire CDC"
        elif vid_pid == "36b7:0001":
            mode = "Bootloader"
        elif cli_ui.is_usb_serial_device(p.device):
            mode = "USB Serial"
        else:
            mode = "Serial"
        desc = p.product or p.description or "—"
        rows.append({"device": p.device, "vid_pid": vid_pid, "mode": mode, "description": desc})

    if as_json:
        print(json.dumps(rows, indent=2))
        return cli_ui.EXIT_OK

    cli_ui.header("Connected Radio & Serial Ports", f"Host: {platform.system()} ({platform.machine()})")
    if not rows:
        cli_ui.info("No serial or USB radio ports detected.")
        return cli_ui.EXIT_OK
    cli_ui.table(
        ("Device", "VID:PID", "Mode", "Description"),
        ((r["device"], r["vid_pid"], r["mode"], r["description"]) for r in rows),
    )
    return cli_ui.EXIT_OK


def _check_cmd_ver(cmd: list[str]) -> tuple[bool, str]:
    try:
        res = subprocess.run(cmd, capture_output=True, text=True, env=cli_ui.cargo_env(), timeout=5)
        if res.returncode == 0:
            first = (res.stdout or res.stderr).strip().splitlines()[0] if (res.stdout or res.stderr).strip() else "ok"
            return True, first
        return False, f"exit {res.returncode}"
    except FileNotFoundError:
        return False, "not found"
    except Exception as exc:
        return False, str(exc)


def _run_doctor(args: list[str]) -> int:
    as_json = "--json" in args
    if "-h" in args or "--help" in args:
        print("Usage: ./kern doctor [--json]\n\nVerify Rust, ARM GCC, Python, GTK4/Dock, and OS mount prerequisites.")
        return cli_ui.EXIT_OK

    checks: list[dict[str, object]] = []

    def add(name: str, ok: bool, detail: str, required: bool = True) -> None:
        checks.append({"check": name, "ok": ok, "required": required, "detail": detail})

    py_ok = sys.version_info >= (3, 11)
    add("Python >= 3.11", py_ok, f"{platform.python_version()} ({sys.executable})")

    ok_rc, rc_ver = _check_cmd_ver(["rustc", "--version"])
    add("Rust compiler", ok_rc, rc_ver)

    ok_cg, cg_ver = _check_cmd_ver(["cargo", "--version"])
    add("Cargo", ok_cg, cg_ver)

    ok_t, targets_out = _check_cmd_ver(["rustup", "target", "list", "--installed"])
    has_m0 = ok_t and ("thumbv6m-none-eabi" in subprocess.run(
        ["rustup", "target", "list", "--installed"], capture_output=True, text=True, env=cli_ui.cargo_env()
    ).stdout) if ok_t else False
    add("Cortex-M0+ target", has_m0, "thumbv6m-none-eabi installed" if has_m0 else "run: rustup target add thumbv6m-none-eabi")

    ok_gcc, gcc_ver = _check_cmd_ver(["arm-none-eabi-gcc", "--version"])
    add("ARM GCC (KRX pkgs)", ok_gcc, gcc_ver, required=False)

    try:
        import serial  # type: ignore[import-not-found]
        add("pyserial", True, getattr(serial, "__version__", "installed"), required=False)
    except ImportError:
        add("pyserial", False, "not installed (pip install pyserial)", required=False)

    try:
        import gi  # type: ignore[import-not-found]
        gi.require_version("Gtk", "4.0")
        gi.require_version("Adw", "1")
        from gi.repository import Adw, Gtk  # noqa: F401
        add("GTK4 + Libadwaita", True, f"GTK {Gtk.get_major_version()}.{Gtk.get_minor_version()} / Adw {Adw.get_major_version()}.{Adw.get_minor_version()}", required=False)
    except Exception:
        add("GTK4 + Libadwaita", False, "unavailable (optional, needed for ./kern dock GUI)", required=False)

    sys_name = platform.system()
    if sys_name == "Linux":
        mtool = shutil.which("gio")
        add("OS Mount (GIO)", bool(mtool), mtool or "install gvfs / glib2", required=False)
    elif sys_name == "Darwin":
        mtool = shutil.which("mount_webdav")
        add("OS Mount (WebDAV)", bool(mtool), mtool or "mount_webdav missing", required=False)
    elif sys_name == "Windows":
        mtool = shutil.which("net")
        add("OS Mount (net use)", bool(mtool), mtool or "net.exe missing", required=False)

    if as_json:
        print(json.dumps({"host": platform.platform(), "checks": checks}, indent=2))
        return cli_ui.EXIT_OK if all(c["ok"] or not c["required"] for c in checks) else cli_ui.EXIT_ERR

    cli_ui.header("Kern Toolchain Doctor", f"{platform.system()} {platform.release()} ({platform.machine()})")
    cli_ui.table(
        ("Component", "Status", "Details"),
        (
            (
                c["check"],
                "OK" if c["ok"] else ("FAIL" if c["required"] else "OPT"),
                c["detail"],
            )
            for c in checks
        ),
    )
    missing_req = [c for c in checks if c["required"] and not c["ok"]]
    if missing_req:
        cli_ui.err(f"{len(missing_req)} required prerequisite(s) missing.")
        return cli_ui.EXIT_ERR
    cli_ui.ok("All required toolchain prerequisites are ready.")
    return cli_ui.EXIT_OK


def main(argv: list[str] | None = None) -> int:
    args = list(sys.argv[1:] if argv is None else argv)
    if "--no-color" in args:
        os.environ["NO_COLOR"] = "1"
        args = [a for a in args if a != "--no-color"]
    if not args:
        return _run_py("builder.py", [])

    cmd, rest = args[0], args[1:]

    if cmd in ("help", "--help", "-h"):
        return _print_help()
    if cmd in ("--version", "-V", "version"):
        print(f"kern {CLI_VERSION}")
        return cli_ui.EXIT_OK

    if cmd == "build":
        return _run_py("builder.py", rest)
    if cmd == "check":
        return _run_py("builder.py", ["--check", *rest])
    if cmd in ("dry-run", "info"):
        return _run_py("builder.py", ["--dry-run", *rest])
    if cmd == "flash":
        return _run_py("builder.py", ["--flash", *rest])
    if cmd == "size":
        return _run_py("builder.py", ["--size", *rest])
    if cmd in ("pkg", "packages"):
        return _run_py("pkg.py", rest)
    if cmd == "bloat":
        return _run_py("bloat.py", rest)
    if cmd == "lint":
        return _run_py("archlint.py", rest)
    if cmd in ("wire", "monitor"):
        return _run_py("rigwire.py", rest)
    if cmd == "provision":
        return _run_py("provision.py", rest)
    if cmd == "media":
        return _run_py("pack_media.py", rest)
    if cmd == "kfw":
        return _run_py("kfw.py", rest)
    if cmd == "mount":
        return _run_py("kernfs_mount.py", rest)
    if cmd in ("ports", "devices"):
        return _run_ports(rest)
    if cmd == "doctor":
        return _run_doctor(rest)
    if cmd == "emu":
        return _run_py("emulator_helper.py", rest)
    if cmd == "dock":
        dock_src = str(REPO_ROOT / "dock" / "src")
        cur_pp = os.environ.get("PYTHONPATH", "")
        pp_parts = [dock_src, str(TOOLS_DIR)] + ([cur_pp] if cur_pp else [])
        env = cli_ui.cargo_env({"PYTHONPATH": os.pathsep.join(pp_parts)})
        return subprocess.call([sys.executable, "-m", "kern_dock", *rest], cwd=str(REPO_ROOT), env=env)
    if cmd == "test":
        target = _host_rust_target()
        return _run_exec(["cargo", "test", "--target", target, "--workspace", *rest])
    if cmd == "abi":
        target = _host_rust_target()
        return _run_exec([
            "cargo", "run", "--quiet", "--profile", "host", "--target", target,
            "-p", "abi", "--features", "describe", "--bin", "abigen", "--",
            str(REPO_ROOT / "sdk" / "c" / "krx_ops.h"), *rest,
        ])
    if cmd in ("fmt", "format"):
        return _run_exec(["cargo", "fmt", "--all", *rest])
    if cmd == "disasm":
        return _run_exec(["cargo", "objdump", "--release", "-p", "firmware", "--", "-d", "--no-show-raw-insn", *rest])
    if cmd in ("clean", "distclean"):
        rc = _run_exec(["cargo", "clean"])
        for fname in ("firmware.bin", "firmware_packed.bin", "kern_firmware.elf"):
            p = REPO_ROOT / "target" / fname
            if p.exists():
                p.unlink(missing_ok=True)
        if cmd == "distclean":
            for d in (REPO_ROOT / "dock" / "build", REPO_ROOT / "dock" / "dist"):
                if d.is_dir():
                    shutil.rmtree(d, ignore_errors=True)
            for bdir in (REPO_ROOT / "packages").rglob("build"):
                if bdir.is_dir():
                    shutil.rmtree(bdir, ignore_errors=True)
        if rc == 0:
            cli_ui.ok("Deep clean complete." if cmd == "distclean" else "Clean complete.")
        return rc

    # Backward-compatible default: forward flags directly to builder.py
    if cmd.startswith("-"):
        return _run_py("builder.py", args)

    known = [c for _, grp in COMMAND_GROUPS for c, _ in grp] + ["format", "devices", "monitor", "packages", "help", "version"]
    matches = difflib.get_close_matches(cmd, known, n=2, cutoff=0.6)
    hint = f" Did you mean: {', '.join(matches)}?" if matches else " Run './kern help' to list commands."
    cli_ui.err(f"Unknown command '{cmd}'.{hint}")
    return cli_ui.EXIT_USAGE


if __name__ == "__main__":
    cli_ui.run_main(main)
