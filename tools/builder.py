#!/usr/bin/env python3
"""
⚡ Kern Firmware CLI Builder & Memory Analyzer
Compiles the firmware, dynamically parses `kern.toml` for granular feature gates,
generates raw/packed binaries, analyzes Flash & SRAM budget against MCU limits,
and optionally flashes the radio.
"""

import sys
import os
import json
import shutil
import subprocess
import argparse
import tomllib
from pathlib import Path

TOOLS_DIR = Path(__file__).resolve().parent
if str(TOOLS_DIR) not in sys.path:
    sys.path.insert(0, str(TOOLS_DIR))

import cli_ui  # noqa: E402

cli_ui.init_console()
_C = cli_ui.ansi_map()
RESET   = _C["RESET"]
BOLD    = _C["BOLD"]
DIM     = _C["DIM"]
INVERT  = _C["INVERT"]
RED     = _C["RED"]
GREEN   = _C["GREEN"]
YELLOW  = _C["YELLOW"]
BLUE    = _C["BLUE"]
MAGENTA = _C["MAGENTA"]
CYAN    = _C["CYAN"]
WHITE   = _C["WHITE"]
GRAY    = _C["GRAY"]

# ========== DEFAULT BUDGETS (PY32F071xB) ==========
DEFAULT_FLASH_BUDGET = 118 * 1024  # 120,832 bytes (safe margin below 128KB)
DEFAULT_RAM_BUDGET   = 16 * 1024   # 16,384 bytes (16 KB SRAM)

# Target hardware profiles: keep compatible variants (UV-K1, UV-K5v3) on the unified base image
TARGET_PROFILES = {
    "uv-k1": {
        "display": "UV-K1",
        "flash_budget": 118 * 1024,
        "features": [],
    },
    "uv-k5v3": {
        "display": "UV-K5v3",
        "flash_budget": 118 * 1024,
        "features": [],
    },
    "uv-k10pro": {
        "display": "UV-K10Pro",
        "flash_budget": 120 * 1024,
        "features": ["target_k10pro"],
    },
}

def print_header(title="Kern Firmware Builder & Analyzer"):
    cli_ui.header(title)

def draw_bar(used, total, width=32, label=""):
    pct = (used / total) * 100.0 if total > 0 else 0.0
    filled = int(width * (pct / 100.0))
    filled = max(0, min(width, filled))
    empty = width - filled

    color = GREEN
    if pct > 75.0: color = YELLOW
    if pct > 90.0: color = RED

    bar = ("█" * filled + "░" * empty) if cli_ui.supports_unicode() else ("#" * filled + "." * empty)
    remaining = total - used
    print(f"  {BOLD}{label:<8}{RESET} {color}[{bar}]{RESET} {BOLD}{pct:5.1f}%{RESET} ({used:,} / {total:,} bytes | {remaining:,} B free)")

def run_cmd(cmd, cwd=None):
    env = cli_ui.cargo_env()
    res = subprocess.run(cmd, shell=True, cwd=cwd, capture_output=True, text=True, env=env)
    return res.returncode, res.stdout, res.stderr

def parse_cargo_size(output):
    sections = {}
    for line in output.splitlines():
        parts = line.split()
        if len(parts) >= 3 and parts[0].startswith('.'):
            try:
                sec_name = parts[0]
                sec_size = int(parts[1])
                sections[sec_name] = sec_size
            except ValueError:
                pass
    return sections

def find_root_dir() -> Path:
    """Finds the root repository directory containing kern.toml or Cargo.toml."""
    current = Path(__file__).resolve().parent
    while current != current.parent:
        if (current / "kern.toml").exists() or (current / "Cargo.toml").exists():
            return current
        current = current.parent
    return Path.cwd()

def load_kern_config(config_path: Path):
    if not config_path.exists():
        return None
    try:
        with open(config_path, "rb") as f:
            return tomllib.load(f)
    except Exception as e:
        print(f" {YELLOW}{INVERT} WARN  {RESET} Failed to parse {config_path}: {e}")
        return None

def resolve_features(cfg: dict | None, preset: str | None = None, extra_features: list[str] | None = None):
    """Extracts features dictionary and active feature list."""
    feature_categories = {}
    active_features = set()

    if preset:
        active_features.add(preset)
        return feature_categories, sorted(list(active_features))

    if cfg and "features" in cfg:
        for cat_name, items in cfg["features"].items():
            if isinstance(items, dict):
                feature_categories[cat_name] = {}
                for feat, enabled in items.items():
                    feature_categories[cat_name][feat] = bool(enabled)
                    if enabled:
                        active_features.add(feat)

    if extra_features:
        for f in extra_features:
            f = f.strip()
            if f:
                active_features.add(f)

    return feature_categories, sorted(list(active_features))

def display_config_summary(cfg: dict | None, categories: dict, active_features: list[str]):
    proj = cfg.get("project", {}) if cfg else {}
    target = cfg.get("target", {}) if cfg else {}

    chip = proj.get("chip", "PY32F071xB")
    device = target.get("device", "UV-K1")
    cpu_clock = target.get("cpu_clock", 48)

    print(f"  {BOLD}Target:{RESET} {CYAN}{device}{RESET} ({chip}) @ {BOLD}{cpu_clock} MHz{RESET}")
    if active_features:
        board_feats = [f for f in active_features if f.startswith("target_") or f in ("gnss", "rgb_backlight")]
        if board_feats:
            print(f"  {BOLD}Profile:{RESET} {MAGENTA}{', '.join(board_feats)}{RESET}")
    print(f"  {BOLD}Active Features ({len(active_features)}):{RESET}")

    if categories:
        for cat, feats in categories.items():
            enabled_list = [f for f, en in feats.items() if en]
            disabled_list = [f for f, en in feats.items() if not en]
            
            line = f"   • {BOLD}{cat:<18}{RESET} "
            if enabled_list:
                line += f"{GREEN}{','.join(enabled_list)}{RESET}"
            if disabled_list:
                line += f" {GRAY}(off: {','.join(disabled_list)}){RESET}"
            print(line)
    else:
        print(f"   {GREEN}{', '.join(active_features)}{RESET}")
    print(f"  ──────────────────────────────────────────────────────────────────")

def main():
    root_dir = find_root_dir()
    os.chdir(root_dir)

    parser = argparse.ArgumentParser(description="Kern Firmware CLI Builder & Memory Analyzer")
    parser.add_argument("--config", "-c", default="kern.toml", help="Path to config TOML (default: kern.toml)")
    parser.add_argument("--target", "-t", help="Override [target].device (e.g. UV-K1, UV-K5v3, UV-K10Pro)")
    parser.add_argument("--preset", choices=["full", "security", "lean"], help="Build using a predefined feature preset")
    parser.add_argument("--features", help="Comma-separated list of extra features to enable")
    parser.add_argument("--no-config", action="store_true", help="Ignore kern.toml and use Cargo default features")
    parser.add_argument("--check", action="store_true", help="Run cargo check instead of building binary")
    parser.add_argument("--dry-run", action="store_true", help="Show resolved configuration without compiling")
    parser.add_argument("--flash", "-f", action="store_true", help="Auto-flash to radio bootloader after successful build")
    parser.add_argument("--port", "-p", help="Serial port for flashing (e.g. /dev/ttyACM0, /dev/ttyUSB0)")
    parser.add_argument("--clean", action="store_true", help="Clean cargo build artifacts before compiling")
    parser.add_argument("--dev", action="store_true", help="Build or check using dev profile (optimized for flash)")
    parser.add_argument("--size", action="store_true", help="Display memory section allocation breakdown")
    parser.add_argument("--deny-warnings", action="store_true", help="Fail the build if any compiler warnings are emitted")
    parser.add_argument("--json", dest="json_out", help="Write structured memory usage telemetry to a JSON file")
    parser.add_argument("--version", dest="fw_version", help="Override firmware SemVer embedded in target/kern.kfw")
    parser.add_argument("--sec-cnt", type=int, default=0, help="Monotonic anti-rollback security counter for target/kern.kfw")
    parser.add_argument("--lms-key", help="LMS-Ascon private key JSON to sign target/kern.kfw (or env KERN_LMS_KEY)")
    parser.add_argument("--mac-key", help="16-byte hex key/file for Ascon-128-MAC signing (or env KERN_MAC_KEY)")
    parser.add_argument("--signer", default="krel", help="4-char ASCII signer tag for .kfw auth trailer")
    parser.add_argument("--require-signed", action="store_true", help="Fail build if no signing key is configured")

    args = parser.parse_args()

    print_header()

    # Load configuration
    cfg = None
    config_file = root_dir / args.config
    if not args.no_config and config_file.exists():
        cfg = load_kern_config(config_file)
        print(f"  {BOLD}Config:{RESET} {WHITE}{config_file.name}{RESET}")
    else:
        print(f"  {BOLD}Config:{RESET} {YELLOW}None (using defaults){RESET}")

    # Resolve target profile cleanly so binary-compatible models share one image
    if args.target:
        cfg = dict(cfg) if cfg else {}
        target_tbl = dict(cfg.get("target", {}))
        target_tbl["device"] = args.target
        cfg["target"] = target_tbl

    device_raw = (cfg.get("target", {}).get("device", "UV-K1") if cfg else "UV-K1").strip()
    profile = TARGET_PROFILES.get(device_raw.lower(), {"display": device_raw, "flash_budget": DEFAULT_FLASH_BUDGET, "features": []})

    # Memory budget from config
    cfg_flash = cfg.get("project", {}).get("flash_budget") if cfg else None
    flash_budget = cfg_flash if (cfg_flash is not None and profile["flash_budget"] == DEFAULT_FLASH_BUDGET and not args.target) else profile["flash_budget"]
    ram_budget   = cfg.get("project", {}).get("ram_budget", DEFAULT_RAM_BUDGET) if cfg else DEFAULT_RAM_BUDGET

    # Resolve features
    extra_feats = list(profile["features"])
    if args.features:
        extra_feats.extend(args.features.split(","))
    categories, active_features = resolve_features(cfg, preset=args.preset, extra_features=extra_feats or None)

    display_config_summary(cfg, categories, active_features)

    if args.dry_run:
        print(f"\n {CYAN}{INVERT} DRY-RUN {RESET} Done. No build executed.")
        return

    if args.clean:
        print(f"\n {YELLOW}{INVERT} CLEAN {RESET} Cleaning cargo target directory...")
        run_cmd("cargo clean")

    # Step 1: Cargo Check or Build
    action_label = "CHECK" if args.check else "BUILD"
    action_verb = "Checking" if args.check else "Compiling"

    feature_flags = ""
    if active_features:
        feature_flags = f"--no-default-features --features {','.join(active_features)}"

    profile_flag = "" if args.dev else "--release"
    profile_name = "dev" if args.dev else "release"
    std_flags = "-Zbuild-std=core,compiler_builtins -Zbuild-std-features=optimize_for_size"

    cmd = f"cargo {'check' if args.check else 'build'} {profile_flag} -p firmware {std_flags} {feature_flags}".strip()
    print(f"\n {BLUE}{INVERT} {action_label} {RESET} {action_verb} firmware {profile_name} profile...")
    print(f" {GRAY}$ {cmd}{RESET}")

    code, stdout, stderr = run_cmd(cmd)

    if code != 0:
        print(f"\n {RED}{INVERT} ERROR {RESET} Cargo compilation failed:")
        print(stderr)
        sys.exit(1)

    # Check for compiler warnings
    warnings = [line for line in stderr.splitlines() if "warning:" in line]
    if warnings:
        print(f" {YELLOW}{INVERT} WARN  {RESET} {len(warnings)} compiler warning(s) detected:")
        for w in warnings[:5]:
            print(f"   {YELLOW}• {w}{RESET}")
        if len(warnings) > 5:
            print(f"   {GRAY}...and {len(warnings) - 5} more{RESET}")
        if args.deny_warnings:
            print(f"\n {RED}{INVERT} ERROR {RESET} Failing build due to --deny-warnings.")
            sys.exit(1)
    else:
        print(f" {GREEN}{INVERT}  OK   {RESET} Compilation successful with 0 warnings.")

    if args.check:
        print(f"\n {GREEN}{BOLD}✨ Syntax and type check passed successfully!{RESET}")
        return

    # Step 2: Size & Memory Allocation Analysis
    elf_path = root_dir / "target" / "thumbv6m-none-eabi" / ("debug" if args.dev else "release") / "firmware"
    sections = {}
    size_cmd = f"cargo size {profile_flag} -p firmware {std_flags} {feature_flags} -- -A".strip()
    code, stdout, stderr = run_cmd(size_cmd)
    if code == 0:
        sections = parse_cargo_size(stdout)
    if not sections and elf_path.exists():
        for tool in ("arm-none-eabi-size", "llvm-size"):
            if shutil.which(tool):
                code, stdout, stderr = run_cmd(f"{tool} -A {elf_path}")
                if code == 0:
                    sections = parse_cargo_size(stdout)
                    break

    if not sections or ".text" not in sections:
        print(f" {RED}{INVERT} ERROR {RESET} Failed to measure ELF section sizes (install cargo-binutils or arm-none-eabi-size).")
        if stderr:
            print(stderr)
        sys.exit(1)

    vec_size    = sections.get(".vector_table", 192)
    text_size   = sections.get(".text", 0)
    rodata_size = sections.get(".rodata", 0)
    data_size   = sections.get(".data", 0)
    bss_size    = sections.get(".bss", 0)
    uninit_size = sections.get(".uninit", 0)

    total_flash = vec_size + text_size + rodata_size + data_size
    total_ram   = data_size + bss_size + uninit_size

    print(f"\n {BOLD}📊 Memory Allocation Summary{RESET}")
    print(f" ══════════════════════════════════════════════════════════════════")
    draw_bar(total_flash, flash_budget, label="FLASH")
    draw_bar(total_ram,   ram_budget,   label="SRAM")
    print(f" ──────────────────────────────────────────────────────────────────")
    print(f"  Vectors: {vec_size:>6} B | Text: {text_size:>6} B | Rodata: {rodata_size:>6} B")
    print(f"  Data:    {data_size:>6} B | BSS:  {bss_size:>6} B | Uninit: {uninit_size:>6} B")
    print(f" ══════════════════════════════════════════════════════════════════")

    if args.json_out:
        out_path = Path(args.json_out)
        out_path.parent.mkdir(parents=True, exist_ok=True)
        telemetry = {
            "target": profile["display"],
            "chip": (cfg.get("project", {}) if cfg else {}).get("chip", "PY32F071xB"),
            "profile": profile_name,
            "flash_used": total_flash,
            "flash_budget": flash_budget,
            "flash_free": flash_budget - total_flash,
            "flash_pct": round((total_flash / flash_budget) * 100.0, 2) if flash_budget else 0.0,
            "ram_used": total_ram,
            "ram_budget": ram_budget,
            "ram_free": ram_budget - total_ram,
            "ram_pct": round((total_ram / ram_budget) * 100.0, 2) if ram_budget else 0.0,
            "warnings": len(warnings),
            "sections": {
                "vectors": vec_size,
                "text": text_size,
                "rodata": rodata_size,
                "data": data_size,
                "bss": bss_size,
                "uninit": uninit_size,
            },
        }
        out_path.write_text(json.dumps(telemetry, indent=2) + "\n")

    if total_flash > flash_budget:
        print(f"\n {RED}{INVERT} CRITICAL {RESET} Flash budget exceeded by {total_flash - flash_budget:,} bytes!")
        sys.exit(1)

    flash_ceiling = cfg.get("project", {}).get("flash_ceiling") if cfg else None
    if flash_ceiling is not None and not args.target:
        delta = total_flash - flash_ceiling
        if delta > 0:
            print(f"\n {RED}{INVERT} CRITICAL {RESET} Flash grew {delta:,} bytes past the recorded ceiling ({flash_ceiling:,} B).")
            sys.exit(1)
        if delta < 0:
            print(f"\n  Flash is {-delta:,} bytes under the ceiling; lower flash_ceiling in kern.toml to {total_flash}.")

    if total_ram > ram_budget:
        print(f"\n {RED}{INVERT} CRITICAL {RESET} SRAM budget exceeded by {total_ram - ram_budget:,} bytes!")
        sys.exit(1)

    if args.size:
        return

    # Step 3: Objcopy Raw ARM Cortex-M0+ Binary
    target_bin = "target/firmware.bin"
    target_packed = "target/firmware_packed.bin"

    print(f" {BLUE}{INVERT} BIN   {RESET} Extracting raw ARM Cortex-M0+ binary...")
    code, stdout, stderr = run_cmd(f"cargo objcopy {profile_flag} -p firmware {std_flags} {feature_flags} -- -O binary {target_bin}".strip())
    if code != 0 and elf_path.exists():
        for tool in ("arm-none-eabi-objcopy", "llvm-objcopy"):
            if shutil.which(tool):
                code, stdout, stderr = run_cmd(f"{tool} -O binary {elf_path} {target_bin}")
                if code == 0:
                    break
    if code != 0:
        print(f" {RED}{INVERT} ERROR {RESET} Objcopy failed: {stderr}")
        sys.exit(1)

    # Step 4: Pack Bootloader Image (XOR Obfuscation for Quansheng bootloader)
    print(f" {BLUE}{INVERT} PACK  {RESET} XOR obfuscating image for bootloader flashing...")
    pack_tool = root_dir / "tools" / "pack.py"
    if pack_tool.exists():
        code, stdout, stderr = run_cmd(f"python3 {pack_tool} {target_bin} {target_packed}")
        if code != 0:
            print(f" {RED}{INVERT} ERROR {RESET} Packing failed: {stderr}")
            sys.exit(1)
    else:
        print(f" {YELLOW}{INVERT} SKIP  {RESET} tools/pack.py not found, skipping bootloader packaging.")

    # Step 4.05: Pack MCU-OTA .kfw Container (verifiable & flashable on-device from Recovery)
    target_kfw = "target/kern.kfw"
    kfw_tool = root_dir / "tools" / "kfw.py"
    if kfw_tool.exists():
        kfw_slug = "uv-k10pro" if device_raw.lower() == "uv-k10pro" else "uv-k1"
        fw_ver = args.fw_version or (cfg.get("project", {}) if cfg else {}).get("version", "0.1.0")
        lms_key = args.lms_key or os.environ.get("KERN_LMS_KEY")
        mac_key = args.mac_key or os.environ.get("KERN_MAC_KEY")
        if args.require_signed and not (lms_key or mac_key):
            print(f" {RED}{INVERT} ERROR {RESET} --require-signed set, but no --lms-key / --mac-key / KERN_LMS_KEY / KERN_MAC_KEY provided.")
            sys.exit(1)
        signed_note = " + signing" if (lms_key or mac_key) else ""
        print(f" {BLUE}{INVERT} KFW   {RESET} Packing & verifying MCU-OTA .kfw container ({kfw_slug}{signed_note})...")
        kfw_argv = [
            sys.executable, str(kfw_tool), "pack", target_kfw,
            "--kind", "firmware", "--mcu-ota",
            "--name", "Kern OS",
            "--version", str(fw_ver),
            "--sec-cnt", str(args.sec_cnt),
            "--fw", f"{kfw_slug}:{target_bin}",
        ]
        if lms_key:
            kfw_argv.extend(["--lms-key", lms_key])
        if mac_key:
            kfw_argv.extend(["--mac-key", mac_key])
        if lms_key or mac_key:
            kfw_argv.extend(["--signer", args.signer])
        res_kfw = subprocess.run(kfw_argv, capture_output=True, text=True, env=cli_ui.cargo_env())
        if res_kfw.returncode != 0:
            print(f" {RED}{INVERT} ERROR {RESET} KFW packing failed: {res_kfw.stderr or res_kfw.stdout}")
            sys.exit(1)
        verify_argv = [sys.executable, str(kfw_tool), "verify", target_kfw]
        trust_store = root_dir / "keys" / "trusted-keys.json"
        if trust_store.is_file() and (lms_key or mac_key):
            verify_argv.extend(["--trust-store", str(trust_store)])
        if mac_key:
            verify_argv.extend(["--mac-key", mac_key])
        if args.require_signed:
            verify_argv.append("--require-auth")
        res_ver = subprocess.run(verify_argv, capture_output=True, text=True, env=cli_ui.cargo_env())
        if res_ver.returncode != 0:
            print(f" {RED}{INVERT} ERROR {RESET} KFW verification failed: {res_ver.stderr or res_ver.stdout}")
            sys.exit(1)

    # Step 4.1: Stage Native ARM ELF for `kern-emu` & GDB/SWD debugging
    target_elf = root_dir / "target" / "kern_firmware.elf"
    if elf_path.exists():
        shutil.copy2(elf_path, target_elf)

    print(f"\n {GREEN}{BOLD}✨ Build Artifacts Ready:{RESET}")
    print(f"   • Raw Binary:    {WHITE}{target_bin}{RESET} ({total_flash:,} bytes)")
    if Path(target_packed).exists():
        print(f"   • Packed Binary: {WHITE}{target_packed}{RESET} ({os.path.getsize(target_packed):,} bytes)")
    if Path(target_kfw).exists():
        print(f"   • KFW Container: {WHITE}{target_kfw}{RESET} ({os.path.getsize(target_kfw):,} bytes, MCU-OTA verified)")
    if target_elf.exists():
        print(f"   • Firmware ELF:  {WHITE}target/kern_firmware.elf{RESET} ({target_elf.stat().st_size:,} bytes)")

    # Step 5: Optional Auto-Flash
    if args.flash:
        print(f"\n {BLUE}{INVERT} FLASH {RESET} Launching radio bootloader flasher...")
        flasher_tool = root_dir / "tools" / "flasher.py"
        flash_cmd = f"python3 {flasher_tool} {target_packed}"
        if args.port:
            flash_cmd += f" --port {args.port}"
        os.system(flash_cmd)

if __name__ == "__main__":
    main()

