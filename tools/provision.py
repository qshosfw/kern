#!/usr/bin/env python3
"""Kern RigWire Provisioning Tool.

Provisions compiled `.krx` applications and `.mod` system modules onto a
connected Kern radio (or QEMU emulator) over RigWire instead of embedding
packages into MCU Flash.

Examples:
    python3 tools/provision.py --port /dev/ttyACM0                 # Core bundle (settings)
    python3 tools/provision.py --port /dev/ttyACM0 --bundle sys    # Core + system modules
    python3 tools/provision.py --port /dev/ttyACM0 --bundle all    # All non-example packages
    python3 tools/provision.py --port /dev/ttyACM0 --build settings sysinfo
    python3 tools/provision.py --port socket://127.0.0.1:8097 settings
"""

import argparse
import os
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
PKG_TOOL = os.path.join(ROOT, "tools", "pkg.py")
RIGWIRE_TOOL = os.path.join(ROOT, "tools", "rigwire.py")

BUNDLES = {
    "core": [
        "core/vfo",
        "core/settings",
    ],
    "sys": [
        "core/vfo",
        "core/settings",
        "sys/calib",
        "sys/sysinfo",
        "sys/recovery",
        "sys/panic",
        "sys/dfu",
    ],
    "all": [
        "core/vfo",
        "core/settings",
        "sys/calib",
        "sys/sysinfo",
        "sys/recovery",
        "sys/panic",
        "sys/dfu",
        "sys/multiboot",
        "apps/beacon",
        "apps/bench",
    ],
    "examples": [
        "examples/hello_rust",
        "examples/hello_c",
        "examples/leaf",
        "examples/paged",
    ],
}

ALIASES = {
    "vfo": "core/vfo",
    "settings": "core/settings",
    "calib": "sys/calib",
    "sysinfo": "sys/sysinfo",
    "recovery": "sys/recovery",
    "panic": "sys/panic",
    "dfu": "sys/dfu",
    "multiboot": "sys/multiboot",
    "beacon": "apps/beacon",
    "bench": "apps/bench",
    "hello_rust": "examples/hello_rust",
    "hello_c": "examples/hello_c",
    "leaf": "examples/leaf",
    "paged": "examples/paged",
}


def find_artifact(rel_pkg: str) -> str:
    pkg_dir = os.path.join(ROOT, "packages", rel_pkg)
    if not os.path.isdir(pkg_dir):
        raise SystemExit(f"ERROR: package directory not found: {pkg_dir}")
    for entry in sorted(os.listdir(pkg_dir)):
        if entry.endswith(".krx") or entry.endswith(".mod"):
            return os.path.join(pkg_dir, entry)
    raise SystemExit(f"ERROR: no built .krx/.mod artifact found in {pkg_dir} (pass --build)")


def build_packages(pkg_names: list[str]) -> None:
    short_names = [os.path.basename(p) for p in pkg_names]
    cmd = [sys.executable, PKG_TOOL, *short_names]
    print(f"Building packages: {' '.join(short_names)}")
    subprocess.run(cmd, check=True, cwd=ROOT)


def sideload_artifact(port: str, baud: int, artifact: str, dry_run: bool) -> None:
    remote_path = f"/apps/{os.path.basename(artifact)}"
    cmd = [
        sys.executable,
        RIGWIRE_TOOL,
        "--port",
        port,
        "--baud",
        str(baud),
        "push",
        artifact,
        remote_path,
    ]
    rel = os.path.relpath(artifact, ROOT)
    if dry_run:
        print(f"  [dry-run] {' '.join(cmd)}")
        return
    # Validate package header & CRC locally before streaming over RigWire
    sys.path.insert(0, os.path.join(ROOT, "tools"))
    import pack_krx  # noqa: E402

    with open(artifact, "rb") as f:
        pack_krx.verify_package(f.read())
    print(f"  -> push {rel} -> {remote_path} ({os.path.getsize(artifact)} B)")
    subprocess.run(cmd, check=True, cwd=ROOT)


def main() -> int:
    ap = argparse.ArgumentParser(description="Provision Kern .krx/.mod packages over RigWire")
    ap.add_argument("-p", "--port", default="/dev/ttyACM0", help="Serial port or socket://host:port")
    ap.add_argument("-b", "--baud", type=int, default=115200, help="Serial baud rate")
    ap.add_argument(
        "--bundle",
        choices=sorted(BUNDLES.keys()),
        default=None,
        help="Named package bundle to provision (default: core when no explicit packages given)",
    )
    ap.add_argument("--build", action="store_true", help="Build selected packages before sideloading")
    ap.add_argument("--dry-run", action="store_true", help="Print RigWire commands without transmitting")
    ap.add_argument("packages", nargs="*", help="Explicit package names (e.g. settings sysinfo hello_rust)")
    args = ap.parse_args()

    selected: list[str] = []
    if args.bundle:
        selected.extend(BUNDLES[args.bundle])
    for name in args.packages:
        rel = ALIASES.get(name, name)
        if rel not in selected:
            selected.append(rel)
    if not selected:
        selected = list(BUNDLES["core"])

    if args.build:
        build_packages(selected)

    artifacts = [find_artifact(p) for p in selected]
    print(f"Provisioning {len(artifacts)} package(s) via RigWire ({args.port}):")
    for art in artifacts:
        sideload_artifact(args.port, args.baud, art, args.dry_run)
    print("Done.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
