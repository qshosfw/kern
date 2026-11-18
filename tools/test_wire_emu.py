#!/usr/bin/env python3
import json
import os
import struct
import subprocess
import sys
import time
import urllib.request
from pathlib import Path

ROOT_DIR = Path(__file__).resolve().parent.parent
EMU_DIR = ROOT_DIR / "inspo" / "uv-k5-v3-emulator"
QEMU_BIN = ROOT_DIR / "inspo" / "qemu-7.2" / "build" / "qemu-system-arm"
FLASH_IMG = EMU_DIR / "assets" / "flash.img"
KERN_ELF = ROOT_DIR / "target" / "kern_firmware.elf"
HTTP_PORT = 8098
SERIAL_PORT = 4446
BASE_URL = f"http://127.0.0.1:{HTTP_PORT}"

def http_get_json(path, timeout=5):
    req = urllib.request.Request(f"{BASE_URL}{path}")
    with urllib.request.urlopen(req, timeout=timeout) as r:
        return json.loads(r.read().decode("utf-8"))

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

def main():
    os.environ["no_proxy"] = "localhost,127.0.0.1,::1"
    os.environ["NO_PROXY"] = "localhost,127.0.0.1,::1"

    webui_script = EMU_DIR / "tools" / "webui.py"
    cmd = [
        sys.executable, str(webui_script),
        "--elf", str(KERN_ELF),
        "--flash", str(FLASH_IMG),
        "--host", "127.0.0.1",
        "--port", str(HTTP_PORT),
        "--serial-port", str(SERIAL_PORT),
    ]

    print("Starting WebUI server on port", HTTP_PORT, "serial", SERIAL_PORT)
    server_proc = subprocess.Popen(
        cmd,
        cwd=str(EMU_DIR),
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE
    )

    try:
        # Wait for WebUI server to start
        for _ in range(50):
            try:
                status = http_get_json("/api/status", timeout=1)
                break
            except Exception:
                time.sleep(0.1)
        else:
            print("WebUI server failed to start")
            out, err = server_proc.communicate(timeout=2)
            print("STDOUT:", out.decode())
            print("STDERR:", err.decode())
            sys.exit(1)

        print("Powering on virtual radio...")
        resp = http_post_json("/api/power/on")
        print("Power on response:", resp)

        time.sleep(4)
        status = http_get_json("/api/status")
        print("System status:", status.get("powered"), status.get("running"))

        # Test RigWire
        sys.path.insert(0, str(ROOT_DIR / "tools"))
        import rigwire
        client = rigwire.RigWireClient(f"tcp://127.0.0.1:{SERIAL_PORT}", timeout=3.0)
        print("RigWire hello...")
        try:
            hello_res = client.hello()
            print("Hello result:", hello_res)
        except Exception as e:
            print("Hello failed:", e)

        print("RigWire ping...")
        try:
            ping_res = client.ping()
            print("Ping result:", ping_res)
        except Exception as e:
            print("Ping failed:", e)

        print("Listing root /:")
        try:
            entries = client.ls("/")
            for e in entries:
                t = 'DIR' if e['is_dir'] else f"{e['size']} B"
                print(f"  {e['name']}: {t}")
        except Exception as e:
            print("ls / failed:", e)

        print("Listing /apps:")
        try:
            entries = client.ls("/apps")
            for e in entries:
                t = 'DIR' if e['is_dir'] else f"{e['size']} B"
                print(f"  {e['name']}: {t}")
        except Exception as e:
            print("ls /apps failed:", e)

        def get_frame():
            req = urllib.request.Request(f"{BASE_URL}/frame.png")
            with urllib.request.urlopen(req, timeout=5) as r:
                return r.read()

        def wait_frame_change(old_frame, timeout=4.0):
            t0 = time.time()
            while time.time() - t0 < timeout:
                cur = get_frame()
                if cur != old_frame:
                    return cur
                time.sleep(0.1)
            return old_frame

        def send_key(key, hold_ms=150, wait_after=0.25):
            http_post_json("/api/key", {"key": key, "hold_ms": hold_ms})
            time.sleep(wait_after)

        launcher_frame = get_frame()

        print("Testing sideload of sysinfo.krx...")
        try:
            res = client.sideload(str(ROOT_DIR / "packages" / "sysinfo" / "sysinfo.krx"), force=True)
            print("Sideload result:", res)
        except Exception as e:
            import traceback
            traceback.print_exc()

        # Save launcher frame
        with open("/tmp/frame_0_launcher.png", "wb") as f:
            f.write(launcher_frame)

        # Launch Settings
        print("Launching Settings App (Key '2')...")
        send_key("2", hold_ms=150, wait_after=0.5)
        settings_frame = wait_frame_change(launcher_frame)
        with open("/tmp/frame_1_settings.png", "wb") as f:
            f.write(settings_frame)
        if settings_frame == launcher_frame:
            print("WARNING: Screen did not update after Key '2'!")
        else:
            print(f"Settings opened successfully! Diff bytes: {len(settings_frame)} vs {len(launcher_frame)}")

        # In Settings: Display, Audio, Buttons, Battery, Device, Apps, System
        print("Navigating down to System (6 down presses)...")
        cur_frame = settings_frame
        for i in range(6):
            send_key("DOWN", hold_ms=100, wait_after=0.15)
            cur_frame = wait_frame_change(cur_frame, timeout=0.5)
        with open("/tmp/frame_2_system_sel.png", "wb") as f:
            f.write(cur_frame)

        print("Entering System submenu (MENU)...")
        send_key("MENU", hold_ms=150, wait_after=0.5)
        system_frame = wait_frame_change(cur_frame)
        with open("/tmp/frame_3_system_sub.png", "wb") as f:
            f.write(system_frame)
        if system_frame == cur_frame:
            print("WARNING: Screen did not update after MENU into System!")
        else:
            print("System submenu entered successfully!")

        # In System menu: 0:Clock, 1:UART Baud, 2:Info
        print("Navigating down to Info (2 down presses)...")
        send_key("DOWN", hold_ms=100, wait_after=0.4)
        send_key("DOWN", hold_ms=100, wait_after=0.4)
        time.sleep(0.3)
        cur_frame = get_frame()
        with open("/tmp/frame_4_info_sel.png", "wb") as f:
            f.write(cur_frame)

        print("Launching Info (MENU)...")
        send_key("MENU", hold_ms=150, wait_after=0.8)
        time.sleep(0.5)
        info_frame = get_frame()
        with open("/tmp/frame_5_info_run.png", "wb") as f:
            f.write(info_frame)
        print("Info screen captured.")

        # Check logs for any panic or errors
        logs = http_get_json("/api/logs")
        print("All serial lines:")
        for entry in logs.get("entries", []):
            if entry.get("source") == "serial":
                print(f"  [serial] {entry.get('text')}")

    finally:
        server_proc.terminate()
        server_proc.wait(timeout=3)

if __name__ == "__main__":
    main()
