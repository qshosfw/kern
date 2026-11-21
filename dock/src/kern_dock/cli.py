"""Unified CLI entry point for Kern Dock (`./kern dock` or `kern-dock`).

Subcommands:
  (default)          Launch the GTK4 + Libadwaita Hardware Dock
  ctl <method> ...   Control a running Dock instance over 0600 local IPC socket
  install-desktop    Install .desktop launcher and bespoke GTK4 SVG radio icons
  uninstall-desktop  Remove installed .desktop launcher and icons
  models             List declarative handset models
  plugins            Discover and print plugins + declared capabilities
  test               Run the full automated headless test suite
  build              Compile-check every module and verify package metadata
"""

from __future__ import annotations

import argparse
import compileall
import json
import sys
import unittest
from pathlib import Path

import kernfs_mount

from . import DOCK_ROOT, __version__
from .core.options import DockOptions
from .render.handset.catalog import MODELS
from .render.lcd import LCD_PALETTES
from .system.desktop import desktop_integration_status, install_desktop_integration, uninstall_desktop_integration
from .system.ipc import IpcClient


def _run_ctl(args: list[str]) -> int:
    if not args or args[0] in ("-h", "--help", "help"):
        print(
            "Usage: kern dock ctl <method> [args...]\n\n"
            "Read-only methods (allowed even in Hardened Mode):\n"
            "  ping | version | status | ports | models | plugins | actions | audit | desktop-status\n\n"
            "Control methods:\n"
            "  connect <port|demo> | disconnect | show | hide | toggle-mini\n"
            "  freq <MHz> | mode <FM|AM|USB|...> | sql <0-9> | vfo\n"
            "  key <name> [press|hold|release] | torch [0|1] | lock [0|1] | headless [0|1]\n"
            "  wire <command...> | mount [port] | unmount | screenshot <path> [lcd|body]\n"
            "  invoke <action_id> | reboot [firmware|recovery|bootloader]"
        )
        return 0

    method = args[0]
    rest = args[1:]
    params: dict[str, object] = {}

    if method == "connect" and rest:
        params["port"] = rest[0]
    elif method == "freq" and rest:
        params["mhz"] = float(rest[0])
    elif method == "mode" and rest:
        params["mode"] = rest[0]
    elif method == "sql" and rest:
        params["level"] = int(rest[0])
    elif method == "key" and rest:
        params["key"] = rest[0]
        if len(rest) > 1:
            params["state"] = rest[1]
    elif method in ("torch", "lock", "headless") and rest:
        params["on"] = rest[0].lower() not in ("0", "off", "false")
    elif method == "wire":
        params["cmd"] = " ".join(rest)
    elif method == "mount" and rest:
        params["port"] = int(rest[0])
    elif method == "screenshot" and rest:
        params["path"] = rest[0]
        if len(rest) > 1:
            params["scope"] = rest[1]
    elif method == "invoke" and rest:
        params["action"] = rest[0]
    elif method == "reboot" and rest:
        params["mode"] = rest[0]

    try:
        resp = IpcClient().call(method, params)
    except Exception as exc:
        kernfs_mount.log_err(str(exc))
        return 1

    print(json.dumps(resp, indent=2))
    return 0 if resp.get("ok") else 1


def _run_tests() -> int:
    tests_dir = DOCK_ROOT / "tests"
    suite = unittest.defaultTestLoader.discover(str(tests_dir), pattern="test_*.py")
    runner = unittest.TextTestRunner(verbosity=2)
    res = runner.run(suite)
    return 0 if res.wasSuccessful() else 1


def _run_build_check() -> int:
    kernfs_mount.print_header(f"Kern Dock v{__version__} Build & Integrity Check")
    ok = compileall.compile_dir(str(DOCK_ROOT / "src"), quiet=1)
    if not ok:
        kernfs_mount.log_err("Bytecode compilation failed")
        return 1
    pyproject = DOCK_ROOT / "pyproject.toml"
    if not pyproject.is_file():
        kernfs_mount.log_err("Missing dock/pyproject.toml")
        return 1
    kernfs_mount.log_ok(f"All modules compiled cleanly ({DOCK_ROOT / 'src'})")
    return _run_tests()


def main(argv: list[str] | None = None) -> int:
    raw = list(sys.argv[1:] if argv is None else argv)

    # Dispatch clean subcommands before argparse GUI flags
    if raw and not raw[0].startswith("-"):
        sub = raw[0]
        sub_args = raw[1:]
        if sub == "ctl":
            return _run_ctl(sub_args)
        if sub == "install-desktop":
            installed = install_desktop_integration()
            kernfs_mount.log_ok("Installed Kern Dock desktop launcher & GTK4 SVG radio icons:")
            for k, v in installed.items():
                print(f"  {k:14s}: {v}")
            return 0
        if sub == "uninstall-desktop":
            removed = uninstall_desktop_integration()
            kernfs_mount.log_ok(f"Removed {len(removed)} desktop integration file(s)")
            return 0
        if sub == "desktop-status":
            print(json.dumps(desktop_integration_status(), indent=2))
            return 0
        if sub == "models":
            for k, m in MODELS.items():
                print(f"  {k:10s}  {m.label:22s}  chassis={m.chassis:9s}  lcd={m.palette:13s}  {m.description}")
            return 0
        if sub == "plugins":
            from .ui.runtime import DockRuntime

            rt = DockRuntime(DockOptions(no_tray=True, no_ipc=True), persist_config=False)
            rt.plugins.discover_all()
            print(json.dumps(rt.plugins.summary(), indent=2))
            return 0
        if sub in ("test", "selftest"):
            return _run_tests()
        if sub == "build":
            return _run_build_check()

    ap = argparse.ArgumentParser(
        prog="kern dock",
        description=(
            f"Kern Dock v{__version__} — Modular GTK4 + Libadwaita Hardware Companion.\n"
            "Subcommands: ctl | install-desktop | uninstall-desktop | models | plugins | test | build"
        ),
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    ap.add_argument("-p", "--port", help="Serial port (/dev/ttyACM0, COM3, tcp://127.0.0.1:4446, or 'demo')")
    ap.add_argument("-b", "--baud", type=int, default=115200, help="Serial baud rate (default: 115200)")
    ap.add_argument("--model", choices=sorted(MODELS), help="Override auto-detected radio chassis mockup")
    ap.add_argument("--palette", choices=list(LCD_PALETTES.keys()), help="Override LCD color palette")
    ap.add_argument("--demo", action="store_true", help="Launch immediately in offline demo mode")
    ap.add_argument("--headless", action="store_true", help="Enable battery-saving headless mode on radio upon connect")
    ap.add_argument("--mini", action="store_true", help="Launch in compact floating Mini-Dock mode")
    ap.add_argument("--hardened", action="store_true", help="Launch directly in Hardened Mode")
    ap.add_argument("--no-tray", action="store_true", help="Disable OS system tray StatusNotifierItem")
    ap.add_argument("--no-ipc", action="store_true", help="Disable local Unix socket & Session D-Bus IPC")
    ap.add_argument("--no-plugins", action="store_true", help="Safe mode: disable plugin loading")
    ap.add_argument("--start-hidden", action="store_true", help="Start minimized in the OS system tray")
    ap.add_argument("--selftest", action="store_true", help="Run headless automated test suite & benchmark")
    args = ap.parse_args(raw)

    if args.selftest:
        return _run_tests()

    opts = DockOptions(
        port="demo" if args.demo else args.port,
        baud=args.baud,
        model=args.model,
        palette=args.palette,
        headless=args.headless,
        mini=args.mini,
        hardened=True if args.hardened else None,
        no_tray=args.no_tray,
        no_ipc=args.no_ipc,
        no_plugins=args.no_plugins,
        start_hidden=args.start_hidden,
    )
    kernfs_mount.print_header(f"Kern Dock v{__version__}")
    kernfs_mount.log_info(f"Launching GTK4 dock (port={opts.port or 'auto-detect'}, mini={opts.mini})")
    try:
        from .ui.app import launch
    except (ImportError, ValueError) as exc:
        kernfs_mount.log_err(f"GTK4 / PyGObject unavailable ({exc}). Install python3-gobject + gtk4 + libadwaita.")
        return 1
    return launch(opts)
