#!/usr/bin/env python3
"""
Kern Dock — Multi-Platform Graphical Radio Companion (Linux / macOS / Windows).

Zero-dependency Python 3 + tkinter graphical window that renders a realistic
Quansheng UV-K1 / UV-K5 v3 handheld radio with a live ST7565 128x64 dot-matrix
LCD, interactive physical keypad + side keys (press/hold/release + PC keyboard
shortcuts), live telemetry, crisp PNG screenshot export, KRX sideloading, and
an integrated KernFS WebDAV mount toggle sharing the same serialized RigWire link.

Works over:
  • Real radio USB-C CDC / UART serial (/dev/ttyACM0, COM3, /dev/cu.usbmodem*)
  • QEMU hardware emulator (tcp://127.0.0.1:4446)
  • Built-in offline preview mode (--demo)
"""

from __future__ import annotations

import argparse
import http.client
import os
import queue
import struct
import subprocess
import sys
import threading
import time
import zlib
from pathlib import Path
from typing import Any

TOOLS_DIR = Path(__file__).resolve().parent
if str(TOOLS_DIR) not in sys.path:
    sys.path.insert(0, str(TOOLS_DIR))

import rigwire  # noqa: E402
import kernfs_mount  # noqa: E402

# =============================================================================
# 1. ST7565 128x64 REALISTIC LCD RASTERIZER & PURE-STDLIB PNG ENCODER
# =============================================================================

LCD_W = 128
LCD_H = 64

# (name, bg_rgb, fg_rgb, grid_rgb, bezel_glow_rgb)
LCD_THEMES: dict[str, tuple[tuple[int, int, int], tuple[int, int, int], tuple[int, int, int]]] = {
    "Amber (UV-K1)":  ((238, 162, 54),  (28, 18, 6),    (220, 145, 42)),
    "Ice White":      ((214, 232, 246), (16, 28, 40),   (194, 214, 230)),
    "STN Green":      ((164, 194, 132), (24, 38, 18),   (148, 178, 118)),
    "Inverted OLED":  ((12, 16, 22),    (124, 224, 255),(18, 24, 34)),
    "Pure Mono":      ((255, 255, 255), (0, 0, 0),      (255, 255, 255)),
}


def render_st7565_rgb(
    fb: bytes,
    scale: int = 3,
    theme_name: str = "Amber (UV-K1)",
    pixel_grid: bool = True,
) -> tuple[int, int, bytes]:
    """Converts a 1024-byte ST7565 vertical-page framebuffer into scaled RGB24 bytes."""
    if len(fb) < 1024:
        fb = fb.ljust(1024, b"\x00")
    bg, fg, grid = LCD_THEMES.get(theme_name, LCD_THEMES["Amber (UV-K1)"])
    bg_b = bytes(bg)
    fg_b = bytes(fg)
    grid_b = bytes(grid)

    out_w = LCD_W * scale
    out_h = LCD_H * scale
    use_grid = pixel_grid and scale >= 3 and grid != bg

    out = bytearray(out_w * out_h * 3)
    row_stride = out_w * 3

    for y in range(LCD_H):
        page_off = (y >> 3) * 128
        bit_mask = 1 << (y & 7)
        # Build one un-gridded scanline of length out_w * 3
        line_main = bytearray(row_stride)
        line_grid = bytearray(row_stride) if use_grid else None

        pos = 0
        for x in range(LCD_W):
            on = (fb[page_off + x] & bit_mask) != 0
            px = fg_b if on else bg_b
            if use_grid:
                # Subtle 1px gap on right/bottom edge of each LCD dot for authentic STN look
                for _ in range(scale - 1):
                    line_main[pos : pos + 3] = px
                    pos += 3
                line_main[pos : pos + 3] = grid_b if not on else px
                pos += 3
            else:
                for _ in range(scale):
                    line_main[pos : pos + 3] = px
                    pos += 3

        if use_grid and line_grid is not None:
            gpos = 0
            for x in range(LCD_W):
                on = (fb[page_off + x] & bit_mask) != 0
                edge = grid_b if not on else bytes((
                    (fg[0] * 3 + bg[0]) >> 2,
                    (fg[1] * 3 + bg[1]) >> 2,
                    (fg[2] * 3 + bg[2]) >> 2,
                ))
                for _ in range(scale):
                    line_grid[gpos : gpos + 3] = edge
                    gpos += 3

        y0 = y * scale
        for sy in range(scale):
            dst = (y0 + sy) * row_stride
            if use_grid and sy == scale - 1 and line_grid is not None:
                out[dst : dst + row_stride] = line_grid
            else:
                out[dst : dst + row_stride] = line_main

    return out_w, out_h, bytes(out)


def rgb_to_ppm(w: int, h: int, rgb: bytes) -> bytes:
    """Wraps RGB24 bytes into a binary P6 PPM image for zero-dependency tk.PhotoImage."""
    return f"P6\n{w} {h}\n255\n".encode("ascii") + rgb


def encode_png_rgb(w: int, h: int, rgb: bytes) -> bytes:
    """Pure-stdlib RGB24 PNG encoder (zero Pillow/PIL dependency)."""
    def chunk(tag: bytes, payload: bytes) -> bytes:
        crc = zlib.crc32(tag + payload) & 0xFFFFFFFF
        return struct.pack(">I", len(payload)) + tag + payload + struct.pack(">I", crc)

    stride = w * 3
    raw = bytearray()
    for y in range(h):
        raw.append(0)  # Filter type 0 (None)
        raw.extend(rgb[y * stride : (y + 1) * stride])

    ihdr = struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0)
    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", ihdr)
        + chunk(b"IDAT", zlib.compress(bytes(raw), level=6))
        + chunk(b"IEND", b"")
    )


# Tiny 5x7 pixel font for offline --demo mode framebuffer generation
_DEMO_FONT: dict[str, tuple[int, ...]] = {
    " ": (0, 0, 0, 0, 0),
    ".": (0, 0, 0x60, 0x60, 0),
    ":": (0, 0x36, 0x36, 0, 0),
    "-": (0x08, 0x08, 0x08, 0x08, 0x08),
    "%": (0x23, 0x13, 0x08, 0x64, 0x62),
    "0": (0x3E, 0x51, 0x49, 0x45, 0x3E),
    "1": (0x00, 0x42, 0x7F, 0x40, 0x00),
    "2": (0x42, 0x61, 0x51, 0x49, 0x46),
    "3": (0x21, 0x41, 0x45, 0x4B, 0x31),
    "4": (0x18, 0x14, 0x12, 0x7F, 0x10),
    "5": (0x27, 0x45, 0x45, 0x45, 0x39),
    "6": (0x3C, 0x4A, 0x49, 0x49, 0x30),
    "7": (0x01, 0x71, 0x09, 0x05, 0x03),
    "8": (0x36, 0x49, 0x49, 0x49, 0x36),
    "9": (0x06, 0x49, 0x49, 0x29, 0x1E),
    "A": (0x7E, 0x11, 0x11, 0x11, 0x7E),
    "B": (0x7F, 0x49, 0x49, 0x49, 0x36),
    "C": (0x3E, 0x41, 0x41, 0x41, 0x22),
    "D": (0x7F, 0x41, 0x41, 0x22, 0x1C),
    "E": (0x7F, 0x49, 0x49, 0x49, 0x41),
    "F": (0x7F, 0x09, 0x09, 0x09, 0x01),
    "G": (0x3E, 0x41, 0x49, 0x49, 0x7A),
    "H": (0x7F, 0x08, 0x08, 0x08, 0x7F),
    "I": (0x00, 0x41, 0x7F, 0x41, 0x00),
    "K": (0x7F, 0x08, 0x14, 0x22, 0x41),
    "L": (0x7F, 0x40, 0x40, 0x40, 0x40),
    "M": (0x7F, 0x02, 0x0C, 0x02, 0x7F),
    "N": (0x7F, 0x04, 0x08, 0x10, 0x7F),
    "O": (0x3E, 0x41, 0x41, 0x41, 0x3E),
    "P": (0x7F, 0x09, 0x09, 0x09, 0x06),
    "R": (0x7F, 0x09, 0x19, 0x29, 0x46),
    "S": (0x46, 0x49, 0x49, 0x49, 0x31),
    "T": (0x01, 0x01, 0x7F, 0x01, 0x01),
    "U": (0x3F, 0x40, 0x40, 0x40, 0x3F),
    "V": (0x1F, 0x20, 0x40, 0x20, 0x1F),
    "W": (0x3F, 0x40, 0x38, 0x40, 0x3F),
    "X": (0x63, 0x14, 0x08, 0x14, 0x63),
    "Y": (0x07, 0x08, 0x70, 0x08, 0x07),
    "Z": (0x61, 0x51, 0x49, 0x45, 0x43),
}


def make_demo_framebuffer(freq_a: str = "433.500", freq_b: str = "145.500", status: str = "DEMO") -> bytes:
    """Generates a realistic 1024-byte ST7565 preview frame when no radio is connected."""
    fb = bytearray(1024)

    def set_px(x: int, y: int, val: bool = True) -> None:
        if 0 <= x < 128 and 0 <= y < 64:
            idx = (y >> 3) * 128 + x
            bit = 1 << (y & 7)
            if val:
                fb[idx] |= bit
            else:
                fb[idx] &= ~bit

    def draw_text(x: int, y: int, text: str, scale: int = 1, invert: bool = False) -> None:
        cx = x
        for ch in text.upper():
            cols = _DEMO_FONT.get(ch, _DEMO_FONT[" "])
            for ci, col in enumerate(cols):
                for row in range(7):
                    bit = bool((col >> row) & 1)
                    if invert:
                        bit = not bit
                    if bit:
                        for sy in range(scale):
                            for sx in range(scale):
                                set_px(cx + ci * scale + sx, y + row * scale + sy, True)
            cx += 6 * scale

    # Top bar
    for x in range(128):
        for y in range(9):
            set_px(x, y, True)
    draw_text(3, 1, "KERN OS", 1, invert=True)
    draw_text(64, 1, status[:6], 1, invert=True)
    draw_text(104, 1, "92%", 1, invert=True)

    # VFO A badge & frequency
    for x in range(2, 18):
        for y in range(13, 23):
            set_px(x, y, True)
    draw_text(4, 15, "V1", 1, invert=True)
    draw_text(24, 13, freq_a, 2)
    draw_text(112, 13, "FM", 1)

    # Dotted divider
    for x in range(0, 128, 2):
        set_px(x, 33, True)

    # VFO B
    for x in range(2, 18):
        set_px(x, 37, True)
        set_px(x, 46, True)
    for y in range(37, 47):
        set_px(2, y, True)
        set_px(17, y, True)
    draw_text(4, 39, "V2", 1, invert=True)
    for x in range(3, 17):
        for y in range(38, 46):
            idx = (y >> 3) * 128 + x
            fb[idx] ^= 1 << (y & 7)
    draw_text(24, 37, freq_b, 2)
    draw_text(112, 37, "FM", 1)

    # Bottom S-meter bar
    draw_text(2, 55, "S", 1)
    for i in range(12):
        bx = 14 + i * 7
        for y in range(56, 62):
            for x in range(bx, bx + 5):
                if i < 7 or y in (56, 61) or x in (bx, bx + 4):
                    set_px(x, y, True)
    draw_text(102, 55, "RX", 1)
    return bytes(fb)


# =============================================================================
# 2. SERIALIZED RIGWIRE WORKER (Non-Blocking Screen Poller + Priority Key Queue)
# =============================================================================

class DockLink:
    """Thread-safe RigWire bridge shared by the GUI poller, key injector, and WebDAV mounter."""

    def __init__(self) -> None:
        self.lock = threading.RLock()
        self.client: rigwire.RigWireClient | None = None
        self.port: str | None = None
        self.volume_name: str = "Quansheng UV-K1"
        self.demo_mode: bool = False
        self.demo_freq_khz: int = 433500
        self.demo_status: str = "DEMO"
        self.last_fb: bytes = make_demo_framebuffer()
        self.last_telemetry: dict[str, Any] = {}
        self.fps: float = 0.0
        self.connected: bool = False
        self.last_error: str = ""
        self._cmd_q: queue.Queue[tuple[str, Any]] = queue.Queue()
        self._stop = threading.Event()
        self._thread: threading.Thread | None = None
        self.dav_server: kernfs_mount.KernWebDAVServer | None = None
        self.dav_port: int = 8765
        self.dav_open_target: str | None = None
        self.dav_unmount_cmd: list[str] | None = None

    def connect(self, port: str, baud: int = 115200) -> None:
        self.disconnect()
        if port.lower() == "demo":
            self.demo_mode = True
            self.port = "demo"
            self.volume_name = "Quansheng UV-K1 DEMO"
            self.connected = True
            self.last_error = ""
            self.last_fb = make_demo_framebuffer()
            return
        with self.lock:
            cli = rigwire.RigWireClient(port, baudrate=baud, timeout=2.0)
            try:
                hello = cli.hello()
                try:
                    telem = cli.get_system_telemetry()
                except Exception:
                    telem = {}
            except Exception:
                cli.close()
                raise
            self.client = cli
            self.port = port
            self.last_telemetry = telem
            self.volume_name = kernfs_mount.resolve_volume_name(port, hello=hello, telemetry=telem)
            self.demo_mode = False
            self.connected = True
            self.last_error = ""
        self._stop.clear()
        self._thread = threading.Thread(target=self._poll_loop, daemon=True)
        self._thread.start()

    def disconnect(self) -> None:
        self.stop_dav()
        self._stop.set()
        if self._thread and self._thread.is_alive():
            self._thread.join(timeout=1.5)
        self._thread = None
        with self.lock:
            if self.client is not None:
                try:
                    self.client.close()
                except Exception:
                    pass
                self.client = None
        self.connected = False
        self.demo_mode = False
        self.volume_name = "Quansheng UV-K1"
        self.last_telemetry = {}
        self.fps = 0.0

    def send_key(self, key_name: str, state_name: str) -> None:
        if self.demo_mode:
            if state_name == "press":
                if key_name == "up":
                    self.demo_freq_khz += 25
                elif key_name == "down":
                    self.demo_freq_khz = max(18000, self.demo_freq_khz - 25)
                self.demo_status = key_name.upper()[:6]
                fa = f"{self.demo_freq_khz // 1000}.{self.demo_freq_khz % 1000:03d}"
                self.last_fb = make_demo_framebuffer(freq_a=fa, status=self.demo_status)
            return
        code = rigwire.KEY_CODES[key_name]
        state = rigwire.KEY_STATES[state_name]
        self._cmd_q.put(("key", (code, state)))

    def toggle_flashlight(self) -> None:
        if self.demo_mode:
            self.demo_status = "TORCH"
            self.last_fb = make_demo_framebuffer(status="TORCH")
            return
        self._cmd_q.put(("torch", None))

    def start_dav(self, http_port: int = 8765, read_only: bool = False) -> str:
        if self.dav_server is not None:
            return f"http://127.0.0.1:{self.dav_port}/"
        if self.demo_mode:
            be: kernfs_mount.FsBackend = kernfs_mount.MemoryFsBackend()
            be.files["/sys/calib.dat"] = b"\xAA" * 512
        else:
            if not self.client:
                raise RuntimeError("Connect to a radio first")
            be = kernfs_mount.RigWireFsBackend(self.client, cache_ttl=3.0, shared_lock=self.lock)
        pol = kernfs_mount.SafetyPolicy(read_only=read_only)
        srv = kernfs_mount.KernWebDAVServer(("127.0.0.1", http_port), be, pol, volume_name=self.volume_name)
        self.dav_server = srv
        self.dav_port = srv.server_address[1]
        threading.Thread(target=srv.serve_forever, daemon=True).start()
        return f"http://127.0.0.1:{self.dav_port}/"

    def stop_dav(self) -> None:
        if self.dav_unmount_cmd:
            try:
                subprocess.run(self.dav_unmount_cmd, capture_output=True, timeout=5)
            except Exception:
                pass
            self.dav_unmount_cmd = None
        self.dav_open_target = None
        if self.dav_server is not None:
            try:
                self.dav_server.shutdown()
                self.dav_server.server_close()
            except Exception:
                pass
            self.dav_server = None

    def _poll_loop(self) -> None:
        frames = 0
        t_fps = time.monotonic()
        last_telem = 0.0
        while not self._stop.is_set() and self.connected:
            # 1. Drain priority key/control commands first
            while not self._cmd_q.empty():
                try:
                    kind, payload = self._cmd_q.get_nowait()
                    with self.lock:
                        if not self.client:
                            break
                        if kind == "key":
                            self.client.inject_key(payload[0], payload[1])
                        elif kind == "torch":
                            self.client.set_flashlight(2)
                except Exception as e:
                    self.last_error = str(e)

            # 2. Poll framebuffer
            try:
                with self.lock:
                    if not self.client:
                        break
                    fb = self.client.screencap()
                if len(fb) == 1024:
                    self.last_fb = fb
                    frames += 1
                    self.last_error = ""
            except Exception as e:
                self.last_error = str(e)
                time.sleep(0.25)
                continue

            now = time.monotonic()
            if now - t_fps >= 1.0:
                self.fps = frames / (now - t_fps)
                frames = 0
                t_fps = now

            # 3. Poll system telemetry every 2.5 s
            if now - last_telem >= 2.5:
                last_telem = now
                try:
                    with self.lock:
                        if self.client:
                            self.last_telemetry = self.client.get_system_telemetry()
                except Exception:
                    pass

            time.sleep(0.04)


# =============================================================================
# 3. GRAPHICAL RADIO WINDOW (tkinter — Native Linux / macOS / Windows)
# =============================================================================

def list_available_ports() -> list[str]:
    ports = ["demo", "tcp://127.0.0.1:4446"]
    try:
        from serial.tools import list_ports
        for p in list_ports.comports():
            ports.insert(0, p.device)
    except Exception:
        pass
    return ports


def launch_gui(initial_port: str | None = None, baud: int = 115200, scale: int = 3, theme: str = "Amber (UV-K1)") -> int:
    try:
        import tkinter as tk
        from tkinter import filedialog, messagebox, ttk
    except Exception as e:
        kernfs_mount.log_err(f"tkinter GUI toolkit unavailable: {e}")
        return 1

    link = DockLink()
    root = tk.Tk()
    root.title("Kern Dock — Radio Companion")
    root.configure(bg="#181B20")
    root.resizable(False, False)

    # State variables
    port_var = tk.StringVar(value=initial_port or list_available_ports()[0])
    theme_var = tk.StringVar(value=theme if theme in LCD_THEMES else "Amber (UV-K1)")
    grid_var = tk.BooleanVar(value=True)
    status_var = tk.StringVar(value="Disconnected — select a port or 'demo'")
    mount_var = tk.StringVar(value="Mount FS")
    device_label_var = tk.StringVar(value="QUANSHENG  •  KERN OS")
    hold_timers: dict[str, str] = {}
    photo_ref: list[Any] = [None]
    last_rendered_key: list[Any] = [None]

    # --- Top Connection & Tools Bar ---
    top = tk.Frame(root, bg="#22262E", padx=10, pady=8)
    top.pack(fill="x")

    tk.Label(top, text="Port:", fg="#B8C2CC", bg="#22262E", font=("TkDefaultFont", 9, "bold")).pack(side="left")
    port_box = ttk.Combobox(top, textvariable=port_var, values=list_available_ports(), width=20)
    port_box.pack(side="left", padx=(4, 6))

    conn_btn = tk.Button(
        top, text="Connect", bg="#2D6A4F", fg="#FFFFFF", activebackground="#40916C",
        relief="flat", padx=10, pady=3, font=("TkDefaultFont", 9, "bold"),
    )
    conn_btn.pack(side="left", padx=2)

    theme_box = ttk.Combobox(top, textvariable=theme_var, values=list(LCD_THEMES.keys()), state="readonly", width=13)
    theme_box.pack(side="left", padx=(8, 4))

    def save_screenshot() -> None:
        w, h, rgb = render_st7565_rgb(link.last_fb, scale=4, theme_name=theme_var.get(), pixel_grid=grid_var.get())
        png = encode_png_rgb(w, h, rgb)
        default_name = f"kern_screen_{time.strftime('%Y%m%d_%H%M%S')}.png"
        fpath = filedialog.asksaveasfilename(
            title="Save LCD Screenshot",
            defaultextension=".png",
            initialfile=default_name,
            filetypes=[("PNG Image", "*.png")],
        )
        if fpath:
            Path(fpath).write_bytes(png)
            status_var.set(f"Saved screenshot: {Path(fpath).name}")

    tk.Button(
        top, text="PNG", command=save_screenshot, bg="#363D4A", fg="#E6EDF3",
        relief="flat", padx=8, pady=3, font=("TkDefaultFont", 9),
    ).pack(side="left", padx=2)

    def open_mounted_fs() -> None:
        if link.dav_server is None:
            return
        target = link.dav_open_target or f"http://127.0.0.1:{link.dav_port}/"
        try:
            kernfs_mount.open_in_file_manager(target)
        except Exception as e:
            messagebox.showerror("Open Folder", str(e))

    open_fs_btn = tk.Button(
        top, text="Open FS", command=open_mounted_fs, bg="#363D4A", fg="#E6EDF3",
        relief="flat", padx=8, pady=3, font=("TkDefaultFont", 9), state="disabled",
    )

    def toggle_mount() -> None:
        if link.dav_server is not None:
            link.stop_dav()
            mount_var.set("Mount FS")
            open_fs_btn.configure(state="disabled")
            status_var.set("KernFS WebDAV bridge unmounted & stopped")
            return
        try:
            url = link.start_dav(http_port=8765)
            open_target, unmount_cmd = kernfs_mount.try_auto_mount(url, link.volume_name)
            link.dav_open_target = open_target
            link.dav_unmount_cmd = unmount_cmd
            mount_var.set("Unmount FS")
            open_fs_btn.configure(state="normal")
            status_var.set(f"Mounted '{link.volume_name}' at {url}")
        except Exception as e:
            messagebox.showerror("KernFS Mount", str(e))

    tk.Button(
        top, textvariable=mount_var, command=toggle_mount, bg="#363D4A", fg="#E6EDF3",
        relief="flat", padx=8, pady=3, font=("TkDefaultFont", 9),
    ).pack(side="left", padx=2)
    open_fs_btn.pack(side="left", padx=2)

    def sideload_pkg() -> None:
        if not link.connected or link.demo_mode or not link.client:
            messagebox.showinfo("Sideload", "Connect to a live radio (or QEMU) first.")
            return
        fpath = filedialog.askopenfilename(
            title="Select KRX Package / Module",
            filetypes=[("KRX Packages", "*.krx *.mod *.app *.svc *.dmn *.drv"), ("All files", "*.*")],
        )
        if not fpath:
            return
        try:
            with link.lock:
                info = link.client.sideload(fpath, verify=True, force=True)
            status_var.set(f"Installed {info['remote_path']}")
        except Exception as e:
            messagebox.showerror("Sideload Failed", str(e))

    tk.Button(
        top, text="Sideload", command=sideload_pkg, bg="#363D4A", fg="#E6EDF3",
        relief="flat", padx=8, pady=3, font=("TkDefaultFont", 9),
    ).pack(side="left", padx=2)

    def on_connect_toggle() -> None:
        if link.connected:
            link.disconnect()
            conn_btn.configure(text="Connect", bg="#2D6A4F")
            mount_var.set("Mount FS")
            open_fs_btn.configure(state="disabled")
            root.title("Kern Dock — Radio Companion")
            device_label_var.set("QUANSHENG  •  KERN OS")
            status_var.set("Disconnected")
            return
        p = port_var.get().strip()
        try:
            link.connect(p, baud=baud)
            conn_btn.configure(text="Disconnect", bg="#9B2226")
            root.title(f"Kern Dock — {link.volume_name}")
            device_label_var.set(f"{link.volume_name.upper()}  •  KERN OS")
            status_var.set(f"Connected: {link.volume_name} ({p})")
        except Exception as e:
            messagebox.showerror("Connection Failed", f"Could not connect to '{p}':\n{e}")

    conn_btn.configure(command=on_connect_toggle)

    # --- Radio Hardware Body ---
    body_wrap = tk.Frame(root, bg="#181B20", padx=18, pady=14)
    body_wrap.pack()

    # Left flank: physical PTT, SK1, SK2 buttons
    flank = tk.Frame(body_wrap, bg="#181B20", padx=6)
    flank.pack(side="left", fill="y", pady=(26, 0))

    chassis = tk.Frame(body_wrap, bg="#252A33", bd=2, relief="ridge", padx=16, pady=14)
    chassis.pack(side="left")

    # Top speaker grille & status LED bar
    header_bar = tk.Frame(chassis, bg="#252A33")
    header_bar.pack(fill="x", pady=(0, 8))
    led_dot = tk.Canvas(header_bar, width=12, height=12, bg="#252A33", highlightthickness=0)
    led_oval = led_dot.create_oval(2, 2, 10, 10, fill="#3B4252", outline="#111419")
    led_dot.pack(side="left")
    tk.Label(
        header_bar, textvariable=device_label_var, fg="#8B949E", bg="#252A33",
        font=("TkDefaultFont", 9, "bold"),
    ).pack(side="left", padx=8)
    tk.Checkbutton(
        header_bar, text="LCD Grid", variable=grid_var, fg="#8B949E", bg="#252A33",
        selectcolor="#181B20", activebackground="#252A33", activeforeground="#E6EDF3",
        font=("TkDefaultFont", 8),
    ).pack(side="right")

    # Recessed dark glass LCD bezel
    bezel = tk.Frame(chassis, bg="#0E1116", bd=4, relief="sunken", padx=10, pady=10)
    bezel.pack(pady=(0, 14))
    lcd_label = tk.Label(bezel, bg="#0E1116", bd=0)
    lcd_label.pack()

    # --- Key Press / Hold / Release Helpers ---
    def key_press(kname: str) -> None:
        if not link.connected:
            return
        link.send_key(kname, "press")
        if kname == "ptt":
            led_dot.itemconfig(led_oval, fill="#E63946")
        if kname in hold_timers:
            root.after_cancel(hold_timers[kname])
        hold_timers[kname] = root.after(450, lambda: link.send_key(kname, "hold"))

    def key_release(kname: str) -> None:
        if kname in hold_timers:
            root.after_cancel(hold_timers.pop(kname))
        if not link.connected:
            return
        link.send_key(kname, "release")
        if kname == "ptt":
            led_dot.itemconfig(led_oval, fill="#2A9D8F" if link.connected else "#3B4252")

    def key_long_click(kname: str) -> None:
        if not link.connected:
            return
        link.send_key(kname, "press")
        root.after(120, lambda: link.send_key(kname, "hold"))
        root.after(260, lambda: link.send_key(kname, "release"))

    def make_hw_key(parent: Any, label: str, kname: str, *, bg: str = "#323844", fg: str = "#F0F4F8", width: int = 7, height: int = 2, sub: str = "") -> tk.Button:
        txt = f"{label}\n{sub}" if sub else label
        btn = tk.Button(
            parent, text=txt, bg=bg, fg=fg, activebackground="#4A5366", activeforeground="#FFFFFF",
            relief="raised", bd=2, width=width, height=height, font=("TkDefaultFont", 9, "bold"),
        )
        btn.bind("<ButtonPress-1>", lambda _e, k=kname: key_press(k))
        btn.bind("<ButtonRelease-1>", lambda _e, k=kname: key_release(k))
        btn.bind("<Button-3>", lambda _e, k=kname: key_long_click(k))
        return btn

    # Populate side keys on left flank
    make_hw_key(flank, "PTT", "ptt", bg="#8C2323", fg="#FFFFFF", width=5, height=5).pack(pady=(0, 8))
    make_hw_key(flank, "SK1", "side1", bg="#3A414F", width=5, height=2).pack(pady=4)
    make_hw_key(flank, "SK2", "side2", bg="#3A414F", width=5, height=2).pack(pady=4)
    tk.Button(
        flank, text="LED", command=link.toggle_flashlight, bg="#4F462A", fg="#F9E79F",
        relief="raised", bd=2, width=5, height=1, font=("TkDefaultFont", 8, "bold"),
    ).pack(pady=(12, 0))

    # Navigation row: M | UP | DOWN | EXIT
    nav = tk.Frame(chassis, bg="#252A33")
    nav.pack(fill="x", pady=(0, 8))
    for i, (lbl, kn, col) in enumerate([
        ("MENU", "menu", "#2E4A62"),
        ("▲", "up", "#323844"),
        ("▼", "down", "#323844"),
        ("EXIT", "exit", "#5E2B2B"),
    ]):
        b = make_hw_key(nav, lbl, kn, bg=col, width=8, height=2)
        b.grid(row=0, column=i, padx=4, sticky="ew")
        nav.columnconfigure(i, weight=1)

    # 4x3 Numeric + F Keypad
    pad = tk.Frame(chassis, bg="#252A33")
    pad.pack(fill="x")
    rows = [
        [("1", "1", "BAND"), ("2", "2", "A/B"), ("3", "3", "V/M"), ("F", "f", "FUNC")],
        [("4", "4", "FC"),   ("5", "5", "NOAA"), ("6", "6", "H/M/L"), ("*", "star", "SCAN")],
        [("7", "7", "VOX"),  ("8", "8", "REV"),  ("9", "9", "CALL"),  ("#", "hash", "LOCK")],
        [("0", "0", "FM"),   None,               None,                None],
    ]
    for r_idx, row in enumerate(rows):
        for c_idx, cell in enumerate(row):
            if cell is None:
                continue
            lbl, kn, sub = cell
            col_bg = "#3B4A38" if kn == "f" else "#2C313C"
            b = make_hw_key(pad, lbl, kn, bg=col_bg, width=8, height=2, sub=sub)
            cspan = 4 if (r_idx == 3 and c_idx == 0) else 1
            b.grid(row=r_idx, column=c_idx, columnspan=cspan, padx=4, pady=3, sticky="ew")
            pad.columnconfigure(c_idx, weight=1)

    # PC Keyboard shortcuts (only when port combobox is not focused)
    key_map = {
        "Return": "menu", "Escape": "exit", "BackSpace": "exit",
        "Up": "up", "Down": "down", "f": "f", "F": "f",
        "asterisk": "star", "numbersign": "hash",
        "F1": "side1", "F2": "side2", "space": "ptt",
        **{str(d): str(d) for d in range(10)},
    }

    def on_key_down(ev: Any) -> None:
        if ev.widget is port_box:
            return
        kn = key_map.get(ev.keysym)
        if kn:
            key_press(kn)

    def on_key_up(ev: Any) -> None:
        if ev.widget is port_box:
            return
        kn = key_map.get(ev.keysym)
        if kn:
            key_release(kn)

    root.bind("<KeyPress>", on_key_down)
    root.bind("<KeyRelease>", on_key_up)

    # --- Bottom Status Bar ---
    bot = tk.Frame(root, bg="#14171C", padx=10, pady=6)
    bot.pack(fill="x")
    tk.Label(bot, textvariable=status_var, fg="#9DA7B3", bg="#14171C", font=("TkDefaultFont", 8)).pack(side="left")
    tk.Label(bot, text="Right-click any key for Long Press", fg="#636E7B", bg="#14171C", font=("TkDefaultFont", 8)).pack(side="right")

    # --- Refresh Loop (30 Hz UI check; only re-rasterizes when framebuffer/theme changes) ---
    def refresh_ui() -> None:
        rkey = (link.last_fb, theme_var.get(), grid_var.get())
        if rkey != last_rendered_key[0]:
            w, h, rgb = render_st7565_rgb(link.last_fb, scale=scale, theme_name=theme_var.get(), pixel_grid=grid_var.get())
            ppm = rgb_to_ppm(w, h, rgb)
            img = tk.PhotoImage(data=ppm)
            photo_ref[0] = img
            lcd_label.configure(image=img)
            last_rendered_key[0] = rkey

        if link.connected:
            led_dot.itemconfig(led_oval, fill="#2A9D8F")
            if not link.demo_mode:
                t = link.last_telemetry
                batt = f" | Batt {t['battery_mv']/1000:.2f}V ({t['battery_pct']}%)" if "battery_mv" in t else ""
                err = f" | {link.last_error}" if link.last_error else ""
                dav = f" | WebDAV :{link.dav_port}" if link.dav_server else ""
                status_var.set(f"{link.volume_name} ({link.port}) | {link.fps:.1f} FPS{batt}{dav}{err}")
        else:
            led_dot.itemconfig(led_oval, fill="#3B4252")

        root.after(33, refresh_ui)

    if initial_port:
        root.after(100, on_connect_toggle)
    refresh_ui()

    def on_close() -> None:
        link.disconnect()
        root.destroy()

    root.protocol("WM_DELETE_WINDOW", on_close)
    root.mainloop()
    return 0


# =============================================================================
# 4. HEADLESS SELFTEST (Safe for CI Without X11/Wayland)
# =============================================================================

def run_selftest() -> int:
    fb = make_demo_framebuffer("433.500", "145.500", "TEST")
    assert len(fb) == 1024
    for theme_name in LCD_THEMES:
        w, h, rgb = render_st7565_rgb(fb, scale=3, theme_name=theme_name, pixel_grid=True)
        assert w == 384 and h == 192 and len(rgb) == 384 * 192 * 3
        ppm = rgb_to_ppm(w, h, rgb)
        assert ppm.startswith(b"P6\n384 192\n255\n")
    w4, h4, rgb4 = render_st7565_rgb(fb, scale=4, theme_name="Amber (UV-K1)", pixel_grid=True)
    png = encode_png_rgb(w4, h4, rgb4)
    assert png.startswith(b"\x89PNG\r\n\x1a\n") and png.endswith(b"IEND\xaeB`\x82")

    # Test DockLink demo mode + key injection + shared WebDAV volume identity propagation
    link = DockLink()
    link.connect("demo")
    assert link.connected and link.demo_mode and link.volume_name == "Quansheng UV-K1 DEMO"
    link.send_key("up", "press")
    assert link.demo_freq_khz == 433525
    url = link.start_dav(http_port=0, read_only=True)
    assert url.startswith("http://127.0.0.1:")
    conn = http.client.HTTPConnection("127.0.0.1", link.dav_port, timeout=5)
    try:
        conn.request("PROPFIND", "/", headers={"Depth": "1"})
        resp = conn.getresponse()
        xml_body = resp.read()
        assert resp.status == 207 and b"Quansheng UV-K1 DEMO" in xml_body
    finally:
        conn.close()
    link.disconnect()
    assert not link.connected and link.dav_server is None

    kernfs_mount.log_ok("All Kern Dock headless selftests passed (ST7565 5-theme rasterizer, PPM, pure-stdlib PNG encoder, DockLink demo & WebDAV volume identity).")
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description="Kern Dock — Multi-platform graphical radio companion")
    ap.add_argument("-p", "--port", help="Serial port (/dev/ttyACM0, COM3, tcp://127.0.0.1:4446, or 'demo')")
    ap.add_argument("-b", "--baud", type=int, default=115200, help="Serial baud rate (default: 115200)")
    ap.add_argument("--scale", type=int, default=3, choices=[2, 3, 4], help="LCD integer pixel scale (default: 3)")
    ap.add_argument("--theme", default="Amber (UV-K1)", choices=list(LCD_THEMES.keys()), help="Initial LCD backlight theme")
    ap.add_argument("--demo", action="store_true", help="Launch immediately in offline demo preview mode")
    ap.add_argument("--selftest", action="store_true", help="Run headless rasterizer/PNG/link selftest")
    args = ap.parse_args()

    if args.selftest:
        return run_selftest()
    port = "demo" if args.demo else args.port
    kernfs_mount.print_header("Kern Dock — Radio Companion")
    kernfs_mount.log_info(f"Launching GUI (theme={args.theme}, scale={args.scale}x, port={port or 'auto'})")
    return launch_gui(initial_port=port, baud=args.baud, scale=args.scale, theme=args.theme)


if __name__ == "__main__":
    raise SystemExit(main())
