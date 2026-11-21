#!/usr/bin/env python3
"""
Cross-platform unified developer CLI for Kern (`./kern` on Linux/macOS, `kern.cmd` on Windows).
"""

from __future__ import annotations

import os
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
        ],
    ),
    (
        "Quality & Analysis",
        [
            ("test", "Run native host unit tests across the workspace"),
            ("fmt", "Format all Rust code in the workspace"),
            ("lint", "Run static architectural linter (tools/archlint.py)"),
            ("bloat", "Run ELF symbol, section, and rodata bloat analyzer"),
            ("disasm", "Disassemble compiled Cortex-M0+ release firmware"),
        ],
    ),
    (
        "Device & Radio Tools",
        [
            ("dock", "Launch graphical hardware companion (live LCD, keypad, mount)"),
            ("wire", "RigWire serial host tool and file provisioning client"),
            ("mount", "Mount radio KernFS over RigWire via cross-platform WebDAV"),
            ("emu", "Launch QEMU hardware emulator with interactive Web UI"),
        ],
    ),
    (
        "Packages & Formats",
        [
            ("pkg", "Build one or all KRX packages under packages/"),
            ("abi", "Regenerate or verify sdk/c/krx_ops.h from src/abi"),
            ("kfw", "Pack, inspect, verify, sign, or extract .kfw containers"),
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
        ("--target, -t", "Override device target (UV-K1, UV-K5v3, UV-K10Pro)"),
        ("--preset", "Build preset ('full', 'lean', 'security')"),
        ("--port, -p", "Serial device port (e.g. /dev/ttyACM0, COM3, /dev/cu.usbmodem*)"),
        ("--json FILE", "Export structured Flash/SRAM telemetry to JSON"),
    ):
        print(f"    {cli_ui.gray(f'{flag:<14}')}  {desc}")
    print("")
    return cli_ui.EXIT_OK


def main(argv: list[str] | None = None) -> int:
    args = list(sys.argv[1:] if argv is None else argv)
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
    if cmd == "wire":
        return _run_py("rigwire.py", rest)
    if cmd == "kfw":
        return _run_py("kfw.py", rest)
    if cmd == "mount":
        return _run_py("kernfs_mount.py", rest)
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
    if cmd == "fmt":
        return _run_exec(["cargo", "fmt", "--all", *rest])
    if cmd == "disasm":
        return _run_exec(["cargo", "objdump", "--release", "-p", "firmware", "--", "-d", "--no-show-raw-insn", *rest])
    if cmd == "clean":
        rc = _run_exec(["cargo", "clean"])
        for fname in ("firmware.bin", "firmware_packed.bin", "kern_firmware.elf"):
            p = REPO_ROOT / "target" / fname
            if p.exists():
                p.unlink(missing_ok=True)
        if rc == 0:
            cli_ui.ok("Clean complete.")
        return rc

    # Backward-compatible default: forward flags directly to builder.py
    if cmd.startswith("-"):
        return _run_py("builder.py", args)

    cli_ui.err(f"Unknown command '{cmd}'. Run './kern help' to list available commands.")
    return cli_ui.EXIT_USAGE


if __name__ == "__main__":
    cli_ui.run_main(main)
