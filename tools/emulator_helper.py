#!/usr/bin/env python3
"""
⚡ Kern Hardware Emulator Helper
Manages QEMU-based UV-K5 / UV-K1 hardware simulation for Kern firmware.
Provides interactive Web UI remote control on localhost with LCD display stream,
keyboard/keypad event injection, serial logs, and automated integration test suites.
"""

import sys
import os
import subprocess
import argparse
from pathlib import Path

TOOLS_DIR = Path(__file__).resolve().parent
if str(TOOLS_DIR) not in sys.path:
    sys.path.insert(0, str(TOOLS_DIR))

import cli_ui  # noqa: E402

cli_ui.init_console()
_C = cli_ui.ansi_map()
RESET   = _C["RESET"]
BOLD    = _C["BOLD"]
GREEN   = _C["GREEN"]
YELLOW  = _C["YELLOW"]
BLUE    = _C["BLUE"]
CYAN    = _C["CYAN"]
RED     = _C["RED"]
GRAY    = _C["GRAY"]
INVERT  = _C["INVERT"]
WHITE   = _C["WHITE"]

ROOT_DIR = Path(__file__).resolve().parent.parent
EMU_DIR = Path(os.environ.get("KERN_EMU_DIR", str(ROOT_DIR / "inspo" / "uv-k5-v3-emulator")))
QEMU_BIN = Path(os.environ.get("KERN_QEMU_BIN", str(ROOT_DIR / "inspo" / "qemu-7.2" / "build" / "qemu-system-arm")))
FLASH_IMG = EMU_DIR / "assets" / "flash.img"
KERN_ELF = ROOT_DIR / "target" / "kern_firmware.elf"
SITE_PACKAGES = EMU_DIR / "tools" / "site-packages"

def print_header(title="Kern Radio Emulator"):
    cli_ui.header(title)

def ensure_env():
    """Sets loopback no_proxy and Python path for local dependencies."""
    os.environ["no_proxy"] = "localhost,127.0.0.1,::1" + ("," + os.environ["no_proxy"] if "no_proxy" in os.environ else "")
    os.environ["NO_PROXY"] = "localhost,127.0.0.1,::1" + ("," + os.environ["NO_PROXY"] if "NO_PROXY" in os.environ else "")
    if SITE_PACKAGES.exists():
        cur_pypath = os.environ.get("PYTHONPATH", "")
        os.environ["PYTHONPATH"] = str(SITE_PACKAGES) + (os.pathsep + cur_pypath if cur_pypath else "")
        if str(SITE_PACKAGES) not in sys.path:
            sys.path.insert(0, str(SITE_PACKAGES))

def ensure_qemu() -> bool:
    """Verifies that the patched QEMU binary exists."""
    if not QEMU_BIN.exists():
        print(f" {RED}{INVERT} ERROR {RESET} QEMU binary not found at: {QEMU_BIN}")
        print(f"   Set KERN_EMU_DIR and KERN_QEMU_BIN to your uv-k5-v3-emulator and qemu-system-arm paths,")
        print(f"   or run tools/setup_qemu.sh inside your emulator checkout.")
        return False
    return True

def ensure_flash() -> bool:
    """Ensures SPI flash image exists; creates it if missing."""
    if not FLASH_IMG.exists():
        print(f" {BLUE}{INVERT} FLASH {RESET} Generating SPI flash image (assets/flash.img)...")
        make_flash = EMU_DIR / "tools" / "make_flash.py"
        res = subprocess.run([sys.executable, str(make_flash)], cwd=str(EMU_DIR))
        return res.returncode == 0
    return True

def ensure_firmware(force_rebuild=False) -> bool:
    """Ensures target/kern_firmware.elf is built."""
    if force_rebuild or not KERN_ELF.exists():
        print(f" {BLUE}{INVERT} BUILD {RESET} Compiling Kern firmware for emulator...")
        builder = ROOT_DIR / "tools" / "builder.py"
        res = subprocess.run([sys.executable, str(builder)], cwd=str(ROOT_DIR))
        if res.returncode != 0:
            return False
    if not KERN_ELF.exists():
        raw_bin = ROOT_DIR / "target" / "firmware.bin"
        bin2elf = EMU_DIR / "tools" / "bin2elf.py"
        if raw_bin.exists() and bin2elf.exists():
            subprocess.run([sys.executable, str(bin2elf), str(raw_bin), "-o", str(KERN_ELF)], cwd=str(ROOT_DIR))
    return KERN_ELF.exists()

def run_tests() -> int:
    """Executes the complete emulator verification suite."""
    print_header("Kern Emulator Verification Suite")
    ensure_env()

    if not ensure_qemu():
        return 1
    if not ensure_flash():
        return 1
    if not ensure_firmware():
        return 1

    os.environ["QEMU"] = str(QEMU_BIN)
    os.environ["ELF"] = str(KERN_ELF)
    os.environ["UVK5_FLASH_IMAGE"] = str(FLASH_IMG)

    tests_script = EMU_DIR / "tools" / "run_tests.sh"
    e2e_script = EMU_DIR / "tools" / "test_webui_e2e.py"

    ref_fw = list((EMU_DIR / "assets" / "firmware").glob("*.elf"))
    ref_elf = ref_fw[0] if ref_fw else KERN_ELF

    print(f"\n {BLUE}{INVERT} STEP 1 {RESET} Running unit tests suite (models & web UI)...")
    res1 = subprocess.run(["bash", str(tests_script), "-q"], cwd=str(EMU_DIR))
    if res1.returncode != 0:
        print(f" {RED}{INVERT} FAILED {RESET} Unit tests failed.")
        return res1.returncode

    print(f"\n {BLUE}{INVERT} STEP 2 {RESET} Running hardware peripheral model tests...")
    hw_tests = [
        ("BK4819 Registers", EMU_DIR / "tools" / "test_bk4819.py"),
        ("Audio Amplifier Intent", EMU_DIR / "tools" / "test_audio_path.py"),
        ("SPI Flash Persistence", EMU_DIR / "tools" / "test_flash_persist.py"),
    ]
    env_hw = os.environ.copy()
    env_hw["ELF"] = str(ref_elf)
    for label, script in hw_tests:
        print(f"  • Checking {label}...")
        r = subprocess.run([sys.executable, str(script)], cwd=str(EMU_DIR), env=env_hw)
        if r.returncode != 0:
            print(f" {RED}{INVERT} FAILED {RESET} {label} test failed.")
            return r.returncode

    print(f"\n {BLUE}{INVERT} STEP 3 {RESET} Running Web UI end-to-end integration test...")
    res3 = subprocess.run([sys.executable, str(e2e_script)], cwd=str(EMU_DIR), env=env_hw)
    if res3.returncode != 0:
        print(f" {RED}{INVERT} FAILED {RESET} Web UI E2E test failed.")
        return res3.returncode

    print(f"\n {BLUE}{INVERT} STEP 4 {RESET} Running Kern microkernel firmware emulation test...")
    kern_test_script = ROOT_DIR / "tools" / "test_kern_emulator.py"
    res4 = subprocess.run([sys.executable, str(kern_test_script)], cwd=str(ROOT_DIR))
    if res4.returncode != 0:
        print(f" {RED}{INVERT} FAILED {RESET} Kern firmware emulation test failed.")
        return res4.returncode

    print(f"\n {GREEN}{BOLD}✨ All emulator test suites passed successfully!{RESET}\n")
    return 0

def launch_emulator(host="127.0.0.1", port=8080, headless=False, force_rebuild=False) -> int:
    """Launches the emulator and serves the Web UI."""
    print_header("Kern Hardware Emulator & Web UI")
    ensure_env()

    if not ensure_qemu():
        return 1
    if not ensure_flash():
        return 1
    if not ensure_firmware(force_rebuild):
        print(f" {RED}{INVERT} ERROR {RESET} Failed to locate or build firmware ELF ({KERN_ELF})")
        return 1

    if headless:
        print(f" {BLUE}{INVERT} HEADLESS {RESET} Launching headless QEMU with GDB stub on port 1234...")
        cmd = [
            str(QEMU_BIN),
            "-M", f"uv-k5-v3,flash-image={FLASH_IMG}",
            "-nographic", "-monitor", "none",
            "-qmp", "unix:/tmp/uvk5-qmp.sock,server=on,wait=off",
            "-kernel", str(KERN_ELF),
            "-gdb", "tcp::1234",
            "-serial", "stdio"
        ]
        return subprocess.run(cmd).returncode

    webui_script = EMU_DIR / "tools" / "webui.py"
    url = f"http://{host}:{port}/"
    print(f" {GREEN}{INVERT} READY {RESET} Launching radio emulator on {BOLD}{url}{RESET}")
    print(f"   • Firmware: {WHITE}{KERN_ELF}{RESET}")
    print(f"   • SPI Flash: {WHITE}{FLASH_IMG}{RESET}")
    print(f"   • Display: Real-time ST7565 LCD frame pump")
    print(f"   • Controls: Interactive keypad buttons + keyboard hotkeys")
    print(f"   • Telemetry: Serial terminal & kernel log buffer")
    print(f" ──────────────────────────────────────────────────────────────────")
    print(f" {YELLOW}Open {BOLD}{url}{RESET}{YELLOW} in your browser to interact with Kern.{RESET}\n")

    cmd = [
        sys.executable, str(webui_script),
        "--elf", str(KERN_ELF),
        "--flash", str(FLASH_IMG),
        "--host", host,
        "--port", str(port),
    ]
    return subprocess.run(cmd, cwd=str(EMU_DIR)).returncode

def main():
    parser = argparse.ArgumentParser(description="⚡ Kern Radio Hardware Emulator")
    parser.add_argument("action", nargs="?", default="run", choices=["run", "test", "check"],
                        help="Action to perform: run (default), test, check")
    parser.add_argument("--test", "-t", action="store_true", help="Run full emulator verification test suite")
    parser.add_argument("--port", "-p", type=int, default=8080, help="Web UI port (default: 8080)")
    parser.add_argument("--host", default="127.0.0.1", help="Web UI bind host (default: 127.0.0.1)")
    parser.add_argument("--headless", action="store_true", help="Run headless QEMU without Web UI")
    parser.add_argument("--rebuild", "-r", action="store_true", help="Force rebuild firmware before running")

    args = parser.parse_args()

    if args.action == "test" or args.test:
        sys.exit(run_tests())
    elif args.action == "check":
        ensure_env()
        qemu_ok = ensure_qemu()
        flash_ok = ensure_flash()
        fw_ok = ensure_firmware()
        print(f"Emulator environment check: QEMU={qemu_ok}, Flash={flash_ok}, Firmware={fw_ok}")
        sys.exit(0 if (qemu_ok and flash_ok and fw_ok) else 1)
    else:
        sys.exit(launch_emulator(host=args.host, port=args.port, headless=args.headless, force_rebuild=args.rebuild))

if __name__ == "__main__":
    main()
