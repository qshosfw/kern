#!/usr/bin/env python3
"""Build KRX packages from their `packages/<group>/<name>/krx.toml` manifest.

One compiler invocation, one linker script (`sdk/krx.ld`) and one set of
position-independence guards for every unit of every package.
"""

import argparse
import json
import os
import subprocess
import sys
import tomllib
from pathlib import Path

TOOLS_DIR = Path(__file__).resolve().parent
if str(TOOLS_DIR) not in sys.path:
    sys.path.insert(0, str(TOOLS_DIR))

import cli_ui  # noqa: E402
import pack_krx  # noqa: E402

REPO_ROOT = os.path.abspath(os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
ROOT = os.path.join(REPO_ROOT, "packages")
SDK = os.path.join(REPO_ROOT, "sdk")
BUILD_ROOT = os.path.join(REPO_ROOT, "target", "packages")
PKG_GROUPS = ("core", "sys", "apps", "examples")
CPU = ["-mthumb", "-mcpu=cortex-m0plus", "-nostdlib"]
CFLAGS = CPU + [
    "-Os", "-fPIC", "-fPIE", "-fvisibility=hidden", "-fno-builtin", "-fno-jump-tables",
    "-fno-tree-loop-distribute-patterns", "-ffunction-sections", "-fdata-sections",
    "-Wl,--gc-sections", "-Wl,--no-dynamic-linker", "-Wl,--build-id=none", "-Wl,-e,0",
    "-T", os.path.join(SDK, "krx.ld"), "-I", os.path.join(SDK, "c"),
]
LEAF_MAX = 1024

PROFILES = {
    "n0": (pack_krx.CLASS_NATIVE, pack_krx.PROFILE_N0_FIXED),
    "n1": (pack_krx.CLASS_NATIVE, pack_krx.PROFILE_N1_PIC),
    "p0": (pack_krx.CLASS_PAGED, pack_krx.PROFILE_P0_PAGED),
    "p1": (pack_krx.CLASS_PAGED, pack_krx.PROFILE_P1_PAGED),
}
TYPES = {
    "app": pack_krx.EXEC_APP,
    "service": pack_krx.EXEC_SERVICE,
    "daemon": pack_krx.EXEC_DAEMON,
    "driver": pack_krx.EXEC_DRIVER,
    "module": pack_krx.EXEC_MODULE,
}
FLAGS = {
    "startup": pack_krx.FLAG_STARTUP,
    "persistent": pack_krx.FLAG_PERSISTENT,
    "restart_on_fail": pack_krx.FLAG_RESTART_ON_FAIL,
    "singleton": pack_krx.FLAG_SINGLETON,
}


def run(*cmd: str) -> str:
    return subprocess.run(cmd, check=True, capture_output=True, text=True).stdout


def fail(msg: str):
    cli_ui.err(msg)
    raise SystemExit(cli_ui.EXIT_ERR)


def objcopy(elf: str) -> bytes:
    out = elf[:-4] + ".bin"
    run("arm-none-eabi-objcopy", "-O", "binary", elf, out)
    with open(out, "rb") as f:
        return f.read()


def ensure_rust_sdk_rlibs() -> tuple[str, str, str]:
    """Compile cached `libabi.rlib` and `libkern_sdk.rlib` for thumbv6m-none-eabi PIC packages."""
    sdk_out = os.path.join(BUILD_ROOT, "_rust_sdk")
    os.makedirs(sdk_out, exist_ok=True)
    abi_rlib = os.path.join(sdk_out, "libabi.rlib")
    sdk_rlib = os.path.join(sdk_out, "libkern_sdk.rlib")
    common = [
        "rustc", "--edition", "2024", "--target", "thumbv6m-none-eabi",
        "-C", "opt-level=z", "-C", "panic=abort", "-C", "relocation-model=pic",
        "-C", "codegen-units=1", "-C", "debuginfo=0", "--crate-type", "rlib",
    ]
    run(*common, "--crate-name", "abi", os.path.join(REPO_ROOT, "src", "abi", "src", "lib.rs"), "-o", abi_rlib)
    run(*common, "--crate-name", "kern_sdk", "-L", f"dependency={sdk_out}", "--extern", f"abi={abi_rlib}",
        os.path.join(SDK, "rust", "src", "lib.rs"), "-o", sdk_rlib)
    return sdk_out, abi_rlib, sdk_rlib


def build_unit(src_dir: str, out_dir: str, source: str, limit: int | None) -> str:
    """Compile one flat PIC unit (C or Rust) and enforce the loader's invariants on it."""
    stem = os.path.splitext(os.path.basename(source))[0]
    elf = os.path.join(out_dir, stem + ".elf")
    src_path = os.path.join(src_dir, source)
    if source.endswith(".rs"):
        sdk_out, abi_rlib, sdk_rlib = ensure_rust_sdk_rlibs()
        ld_script = os.path.join(SDK, "krx.ld")
        run(
            "rustc", "--edition", "2024", "--crate-type", "bin",
            "--target", "thumbv6m-none-eabi",
            "-C", "opt-level=z", "-C", "panic=abort", "-C", "relocation-model=pic",
            "-C", "codegen-units=1", "-C", "debuginfo=0",
            "-C", "linker=arm-none-eabi-gcc",
            "-C", "link-arg=-nostdlib",
            "-C", "link-arg=-mthumb",
            "-C", "link-arg=-mcpu=cortex-m0plus",
            "-C", f"link-arg=-T{ld_script}",
            "-C", "link-arg=-Wl,--gc-sections",
            "-C", "link-arg=-Wl,--no-dynamic-linker",
            "-C", "link-arg=-Wl,--build-id=none",
            "-C", "link-arg=-Wl,-e,0",
            "-L", f"dependency={sdk_out}",
            "--extern", f"abi={abi_rlib}",
            "--extern", f"kern_sdk={sdk_rlib}",
            "-o", elf, src_path,
        )
    else:
        run("arm-none-eabi-gcc", *CFLAGS, "-I", src_dir, f"-Wa,-I{out_dir}", "-o", elf, src_path)
    size = len(objcopy(elf))
    undefs = run("arm-none-eabi-nm", "-u", elf).split()
    relocs = run("arm-none-eabi-readelf", "-r", elf).count("R_ARM_")
    symbols = [line.split() for line in run("arm-none-eabi-nm", elf).splitlines()]
    entry = [s[2] for s in symbols if len(s) == 3 and s[1] in "Tt" and int(s[0], 16) & ~1 == 0]
    got = any(s[2] == "_GLOBAL_OFFSET_TABLE_" for s in symbols if len(s) == 3)
    print(f"  {stem}: {size} B")
    if undefs or relocs or got:
        fail(f"{stem}: must have 0 undefined symbols, 0 relocations and no GOT (undefs={undefs}, relocs={relocs})")
    if not entry:
        fail(f"{stem}: no function at offset 0; mark the entry `section(\".text.entry\")`")
    if limit is not None and size > limit:
        fail(f"{stem}: {size} B exceeds its {limit} B window")
    return elf[:-4] + ".bin"


def discover_packages() -> dict[str, str]:
    """Return {short_or_rel_name: pkg_dir} for all packages under `packages/`."""
    found: dict[str, str] = {}
    for group in PKG_GROUPS:
        gdir = os.path.join(ROOT, group)
        if not os.path.isdir(gdir):
            continue
        for entry in sorted(os.listdir(gdir)):
            pdir = os.path.join(gdir, entry)
            if os.path.isfile(os.path.join(pdir, "krx.toml")):
                found[entry] = pdir
                found[f"{group}/{entry}"] = pdir
    for entry in sorted(os.listdir(ROOT)):
        pdir = os.path.join(ROOT, entry)
        if os.path.isfile(os.path.join(pdir, "krx.toml")):
            found[entry] = pdir
    return found


def build_package(name: str, pkg_map: dict[str, str] | None = None):
    if pkg_map is None:
        pkg_map = discover_packages()
    pkg_dir = pkg_map.get(name) or os.path.join(ROOT, name)
    manifest_path = os.path.join(pkg_dir, "krx.toml")
    if not os.path.isfile(manifest_path):
        fail(f"unknown package '{name}' (no {manifest_path})")
    with open(manifest_path, "rb") as f:
        m = tomllib.load(f)
    rel_name = os.path.relpath(pkg_dir, ROOT)
    print(f"==> {rel_name}")
    default_src = "src" if os.path.isdir(os.path.join(pkg_dir, "src")) else "."
    src_dir = os.path.join(pkg_dir, m.get("src", default_src))
    out_dir = os.path.join(BUILD_ROOT, rel_name)
    os.makedirs(out_dir, exist_ok=True)

    for blob in m.get("blob", []):
        source = os.path.join(pkg_dir, blob["src"])
        elf = os.path.join(out_dir, os.path.splitext(os.path.basename(source))[0] + ".elf")
        run("arm-none-eabi-gcc", *CPU, "-Wl,--build-id=none", f"-Wl,-e,{blob['entry']}", "-Ttext=0",
            f"-Wa,-I{os.path.dirname(source)}", source, "-o", elf)
        size = len(objcopy(elf))
        print(f"  {os.path.basename(elf)[:-4]}: {size} B (limit {blob['max']} B)")
        if size > blob["max"]:
            fail(f"{blob['src']}: {size} B exceeds {blob['max']} B")

    app_class, profile = PROFILES[m.get("profile", "n1")]
    builder = pack_krx.KrxPackageBuilder(m["name"], app_class, profile)
    version = tuple(int(p) for p in m.get("version", "1.0.0").split("."))
    icon = pack_krx.resolve_mask("KRX_ICON", m["icon"]) if "icon" in m else 1
    builder.set_identity(m["name"], m.get("author", "KERN-TEAM"), version, icon)
    flags = 0
    for flag in m.get("lifecycle", []):
        flags |= FLAGS[flag]
    builder.set_lifecycle(TYPES[m.get("type", "app")], flags)
    if "caps" in m:
        builder.caps = pack_krx.resolve_mask("KRX_CAP", m["caps"])
    if m.get("sign", False) or rel_name.startswith("sys/"):
        builder.sign = True

    page_size = m.get("page_size", m.get("leaf_max", LEAF_MAX))
    if profile == pack_krx.PROFILE_P1_PAGED:
        builder.page_size = page_size
    if "root" in m:
        root_limit = m.get("root_max", None)
        with open(build_unit(src_dir, out_dir, m["root"], root_limit), "rb") as f:
            builder.set_payload(f.read())
    else:
        builder.set_payload(b"\x70\x47")  # Thumb `bx lr`: paged images enter through block 0
    blocks = m.get("blocks", [])
    if blocks:
        for block_id, source in enumerate(blocks, start=1 if "root" in m else 0):
            builder.add_block_file(block_id, build_unit(src_dir, out_dir, source, builder.page_size))
    elif profile == pack_krx.PROFILE_P1_PAGED:
        builder.auto_page = True
    for action in m.get("action", []):
        scope = pack_krx.resolve_mask("KRX_SCOPE", action["scope"])
        builder.add_action(action.get("id", 0), scope, action["label"], action.get("flags", 0))
    if "icon_8" in m:
        builder.add_icon_ascii(1, 8, 8, m["icon_8"])
    if "icon_16" in m:
        builder.add_icon_ascii(2, 16, 16, m["icon_16"])
    if "icon_tray" in m:
        builder.add_icon_ascii(3, 5, 5, m["icon_tray"])

    image = builder.build()
    pack_krx.verify_package(image)
    output = os.path.join(pkg_dir, m["output"])
    with open(output, "wb") as f:
        f.write(image)
    print(f"  -> {m['output']} ({len(image)} B)")


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(
        prog="kern pkg",
        description="Build or list KRX packages from packages/<group>/<name>/krx.toml manifests.",
    )
    ap.add_argument("packages", nargs="*", help="Package name(s) (e.g. core/settings, dfu, or 'all')")
    ap.add_argument("-l", "--list", action="store_true", help="List all discovered KRX packages")
    ap.add_argument("--json", action="store_true", help="Output package list as JSON (with --list)")
    args = ap.parse_args(argv)

    pkg_map = discover_packages()
    if args.list:
        seen: set[str] = set()
        rows: list[dict[str, str]] = []
        for k, pdir in sorted(pkg_map.items()):
            if "/" in k and pdir not in seen:
                seen.add(pdir)
                with open(os.path.join(pdir, "krx.toml"), "rb") as f:
                    m = tomllib.load(f)
                rows.append({
                    "id": k,
                    "name": str(m.get("name", k)),
                    "type": str(m.get("type", "app")),
                    "profile": str(m.get("profile", "n1")),
                    "output": str(m.get("output", "")),
                })
        if args.json:
            print(json.dumps(rows, indent=2))
        else:
            cli_ui.header(f"KRX Packages ({len(rows)})")
            cli_ui.table(
                ("Package", "Name", "Type", "Profile", "Output"),
                ((r["id"], r["name"], r["type"], r["profile"], r["output"]) for r in rows),
            )
        return cli_ui.EXIT_OK

    names = [n for n in args.packages if n != "all"]
    if not names:
        seen = set()
        names = []
        for k, pdir in pkg_map.items():
            if "/" in k and pdir not in seen:
                seen.add(pdir)
                names.append(k)
    try:
        for name in names:
            build_package(name, pkg_map)
    except subprocess.CalledProcessError as e:
        sys.stderr.write(e.stderr or "")
        fail(f"`{' '.join(e.cmd[:1])}` failed")
    return cli_ui.EXIT_OK


if __name__ == "__main__":
    cli_ui.run_main(main)
