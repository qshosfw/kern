#!/usr/bin/env python3
"""
⚡ Kern Firmware CLI Builder & Memory Analyzer
Compiles the firmware, dynamically parses `kern.toml` for granular feature gates,
generates raw/packed binaries, analyzes Flash & SRAM budget against MCU limits,
and optionally flashes the radio.
"""

import sys
import os
import subprocess
import argparse
import tomllib
from pathlib import Path

# ========== TUI STYLES ==========
RESET   = "\033[0m"
BOLD    = "\033[1m"
DIM     = "\033[2m"
INVERT  = "\033[7m"
RED     = "\033[31m"
GREEN   = "\033[32m"
YELLOW  = "\033[33m"
BLUE    = "\033[34m"
MAGENTA = "\033[35m"
CYAN    = "\033[36m"
WHITE   = "\033[37m"
GRAY    = "\033[90m"

# ========== DEFAULT BUDGETS (PY32F071xB) ==========
DEFAULT_FLASH_BUDGET = 118 * 1024  # 120,832 bytes (safe margin below 128KB)
DEFAULT_RAM_BUDGET   = 16 * 1024   # 16,384 bytes (16 KB SRAM)

def print_header(title="Kern Firmware Builder & Analyzer"):
    print(f"\n  ⚡ {BOLD}{CYAN}{title}{RESET}")
    print(f"  ══════════════════════════════════════════════════════════════════")

def draw_bar(used, total, width=32, label=""):
    pct = (used / total) * 100.0 if total > 0 else 0.0
    filled = int(width * (pct / 100.0))
    filled = max(0, min(width, filled))
    empty = width - filled

    color = GREEN
    if pct > 75.0: color = YELLOW
    if pct > 90.0: color = RED

    bar = "█" * filled + "░" * empty
    remaining = total - used
    print(f"  {BOLD}{label:<8}{RESET} {color}[{bar}]{RESET} {BOLD}{pct:5.1f}%{RESET} ({used:,} / {total:,} bytes | {remaining:,} B free)")

def run_cmd(cmd, cwd=None):
    res = subprocess.run(cmd, shell=True, cwd=cwd, capture_output=True, text=True)
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

    # Memory budget from config
    flash_budget = cfg.get("project", {}).get("flash_budget", DEFAULT_FLASH_BUDGET) if cfg else DEFAULT_FLASH_BUDGET
    ram_budget   = cfg.get("project", {}).get("ram_budget", DEFAULT_RAM_BUDGET) if cfg else DEFAULT_RAM_BUDGET

    # Resolve features
    extra_feats = args.features.split(",") if args.features else None
    categories, active_features = resolve_features(cfg, preset=args.preset, extra_features=extra_feats)

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
    else:
        print(f" {GREEN}{INVERT}  OK   {RESET} Compilation successful with 0 warnings.")

    if args.check:
        print(f"\n {GREEN}{BOLD}✨ Syntax and type check passed successfully!{RESET}")
        return

    # Step 2: Size & Memory Allocation Analysis
    size_cmd = f"cargo size {profile_flag} -p firmware {std_flags} {feature_flags} -- -A".strip()
    code, stdout, stderr = run_cmd(size_cmd)
    sections = parse_cargo_size(stdout) if code == 0 else {}

    vec_size    = sections.get(".vector_table", 192)
    text_size   = sections.get(".text", 0)
    rodata_size = sections.get(".rodata", 0)
    data_size   = sections.get(".data", 0)
    bss_size    = sections.get(".bss", 0)

    total_flash = vec_size + text_size + rodata_size + data_size
    total_ram   = data_size + bss_size

    print(f"\n {BOLD}📊 Memory Allocation Summary{RESET}")
    print(f" ══════════════════════════════════════════════════════════════════")
    draw_bar(total_flash, flash_budget, label="FLASH")
    draw_bar(total_ram,   ram_budget,   label="SRAM")
    print(f" ──────────────────────────────────────────────────────────────────")
    print(f"  Vectors: {vec_size:>6} B | Text: {text_size:>6} B | Rodata: {rodata_size:>6} B")
    print(f"  Data:    {data_size:>6} B | BSS:  {bss_size:>6} B")
    print(f" ══════════════════════════════════════════════════════════════════")

    if total_flash > flash_budget:
        print(f"\n {RED}{INVERT} CRITICAL {RESET} Flash budget exceeded by {total_flash - flash_budget:,} bytes!")
        sys.exit(1)

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

    # Step 4.1: Wrap Raw Binary in ELF for QEMU Hardware Emulation
    target_elf = "target/kern_firmware.elf"
    bin2elf_tool = root_dir / "inspo" / "uv-k5-v3-emulator" / "tools" / "bin2elf.py"
    if bin2elf_tool.exists():
        code, stdout, stderr = run_cmd(f"python3 {bin2elf_tool} {target_bin} -o {target_elf}")
        if code != 0:
            print(f" {YELLOW}{INVERT} WARN  {RESET} bin2elf failed: {stderr.strip()}")

    print(f"\n {GREEN}{BOLD}✨ Build Artifacts Ready:{RESET}")
    print(f"   • Raw Binary:    {WHITE}{target_bin}{RESET} ({total_flash:,} bytes)")
    if Path(target_packed).exists():
        print(f"   • Packed Binary: {WHITE}{target_packed}{RESET} ({os.path.getsize(target_packed):,} bytes)")
    if Path(target_elf).exists():
        print(f"   • Emulator ELF:  {WHITE}{target_elf}{RESET} ({os.path.getsize(target_elf):,} bytes)")

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

