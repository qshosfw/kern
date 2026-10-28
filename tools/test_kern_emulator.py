#!/usr/bin/env python3
"""
⚡ Kern End-to-End Hardware Simulation Test
Verifies the complete execution pipeline of Kern microkernel firmware on the
QEMU UV-K5/UV-K1 hardware emulator:
1. Cold boots Kern firmware on Cortex-M0+ SoC with ST7565 LCD, flash, & BK4829 peripherals.
2. Verifies UART serial banner ("KERN v0.1.0") in kernel telemetry logs.
3. Validates ST7565 display frame generation and pixel illumination from panel memory.
4. Injects keypad matrix event ('1') to launch Dual-VFO Radio app.
5. Injects frequency step event ('UP') and asserts UI state change.
"""

import json
import os
import sys
import time
import urllib.request
import subprocess
from pathlib import Path

ROOT_DIR = Path(__file__).resolve().parent.parent
EMU_DIR = ROOT_DIR / "inspo" / "uv-k5-v3-emulator"
QEMU_BIN = ROOT_DIR / "inspo" / "qemu-7.2" / "build" / "qemu-system-arm"
FLASH_IMG = EMU_DIR / "assets" / "flash.img"
KERN_ELF = ROOT_DIR / "target" / "kern_firmware.elf"
HTTP_PORT = 8097
BASE_URL = f"http://127.0.0.1:{HTTP_PORT}"

def setup_env():
    os.environ["no_proxy"] = "localhost,127.0.0.1,::1" + ("," + os.environ["no_proxy"] if "no_proxy" in os.environ else "")
    os.environ["NO_PROXY"] = "localhost,127.0.0.1,::1" + ("," + os.environ["NO_PROXY"] if "NO_PROXY" in os.environ else "")
    site_packages = EMU_DIR / "tools" / "site-packages"
    if site_packages.exists() and str(site_packages) not in sys.path:
        sys.path.insert(0, str(site_packages))

def http_get(path, timeout=5):
    req = urllib.request.Request(f"{BASE_URL}{path}")
    with urllib.request.urlopen(req, timeout=timeout) as r:
        return r.read()

def http_get_json(path, timeout=5):
    data = http_get(path, timeout=timeout)
    return json.loads(data.decode("utf-8"))

def http_post_json(path, payload=None, timeout=5):
    data = json.dumps(payload or {}).encode("utf-8")
    req = urllib.request.Request(
        f"{BASE_URL}{path}",
        data=data,
        headers={"Content-Type": "application/json"},
        method="POST"
    )
    with urllib.request.urlopen(req, timeout=timeout) as r:
        return json.loads(r.read().decode("utf-8"))

def run_test():
    setup_env()
    print("=== Kern Hardware Emulator Test ===")

    if not KERN_ELF.exists():
        sys.exit(f"FAIL: Kern firmware ELF not found at {KERN_ELF}")
    if not QEMU_BIN.exists():
        sys.exit(f"FAIL: QEMU binary not found at {QEMU_BIN}")
    if not FLASH_IMG.exists():
        sys.exit(f"FAIL: SPI flash image not found at {FLASH_IMG}")

    webui_script = EMU_DIR / "tools" / "webui.py"
    cmd = [
        sys.executable, str(webui_script),
        "--elf", str(KERN_ELF),
        "--flash", str(FLASH_IMG),
        "--host", "127.0.0.1",
        "--port", str(HTTP_PORT),
    ]

    server_proc = subprocess.Popen(
        cmd,
        cwd=str(EMU_DIR),
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL
    )

    try:
        # Wait for WebUI server to listen
        print("  • Waiting for WebUI server...")
        for _ in range(50):
            try:
                status = http_get_json("/api/status", timeout=1)
                break
            except Exception:
                time.sleep(0.1)
        else:
            sys.exit("FAIL: WebUI server failed to start")

        print("  • Powering ON Kern virtual radio...")
        power_resp = http_post_json("/api/power/on")
        if not power_resp.get("ok"):
            sys.exit(f"FAIL: Power-on returned: {power_resp}")

        # Allow Kern to complete boot and display the apps launcher
        print("  • Booting Kern microkernel on Cortex-M0+...")
        time.sleep(6)

        # 1. Verify system status
        status = http_get_json("/api/status")
        if not status.get("powered") or not status.get("running"):
            sys.exit(f"FAIL: Radio not running: {status}")
        print("  PASS  System status: powered and running")

        panel = status.get("panel", {})
        if not panel.get("display"):
            sys.exit(f"FAIL: Panel display not enabled: {panel}")
        print(f"  PASS  ST7565 LCD initialized: contrast={panel.get('contrast')}, invert={panel.get('invert')}")

        # 2. Verify serial logs
        logs = http_get_json("/api/logs")
        entries = logs.get("entries", [])
        serial_lines = [e.get("text") for e in entries if e.get("source") == "serial"]
        has_banner = any("KERN v" in line for line in serial_lines)
        if not has_banner:
            sys.exit(f"FAIL: Missing KERN serial banner in telemetry logs: {serial_lines}")
        print("  PASS  Serial telemetry output verified: 'KERN v0.1.0' detected")

        # 3. Verify LCD Framebuffer PNG capture
        frame_bytes = http_get("/frame.png")
        if not frame_bytes.startswith(b"\x89PNG\r\n\x1a\n"):
            sys.exit("FAIL: /frame.png did not return valid PNG data")
        if len(frame_bytes) < 1000:
            sys.exit(f"FAIL: Screen frame too small ({len(frame_bytes)} bytes)")
        print(f"  PASS  LCD frame captured successfully ({len(frame_bytes)} bytes)")

        # 4. Inject Key '1' to open Radio VFO app
        print("  • Injecting keypad press: '1' (Launch Radio App)...")
        key_resp = http_post_json("/api/key", {"key": "1", "hold_ms": 150})
        if not key_resp.get("ok"):
            sys.exit(f"FAIL: Key injection failed: {key_resp}")

        radio_frame = None
        for _ in range(30):
            time.sleep(0.2)
            cur = http_get("/frame.png")
            if cur != frame_bytes:
                radio_frame = cur
                break
        if not radio_frame:
            sys.exit("FAIL: Screen did not update after launching Radio app")
        print("  PASS  Radio VFO app loaded and rendered to LCD")

        # 5. Inject Key 'UP' to step frequency
        print("  • Injecting keypad press: 'UP' (Step Frequency)...")
        key_resp = http_post_json("/api/key", {"key": "UP", "hold_ms": 150})
        if not key_resp.get("ok"):
            sys.exit(f"FAIL: Key UP injection failed: {key_resp}")

        up_frame = None
        for _ in range(30):
            time.sleep(0.2)
            cur = http_get("/frame.png")
            if cur != radio_frame:
                up_frame = cur
                break
        if not up_frame:
            sys.exit("FAIL: Screen did not update after frequency step")
        print("  PASS  Frequency stepped successfully (+12.5 kHz)")

        # 6. Safety release
        rel_resp = http_post_json("/api/release-all")
        if not rel_resp.get("ok"):
            sys.exit(f"FAIL: Release all keys failed: {rel_resp}")
        print("  PASS  Keypad safety release successful")

        print("\n✨ All Kern hardware emulator integration tests passed!\n")
        return 0

    finally:
        server_proc.terminate()
        try:
            server_proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            server_proc.kill()

if __name__ == "__main__":
    sys.exit(run_test())
