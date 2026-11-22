"""Comprehensive headless automated test suite for Kern Dock."""

from __future__ import annotations

import http.client
import sys
import tempfile
import time
import unittest
from pathlib import Path

# Ensure dock/src and tools/ are importable even when run directly via unittest
_DOCK_ROOT = Path(__file__).resolve().parent.parent
_REPO_ROOT = _DOCK_ROOT.parent
for _p in (str(_DOCK_ROOT / "src"), str(_REPO_ROOT / "tools")):
    if _p not in sys.path:
        sys.path.insert(0, _p)

import kfw
import rigwire

from kern_dock.core.config import ConfigManager
from kern_dock.core.events import EventBus
from kern_dock.core.options import DockOptions
from kern_dock.core.registry import ActionSpec, CommandRegistry, PageRegistry, PageSpec
from kern_dock.core.security import SecurityController, SecurityError, sanitize_kernfs_path
from kern_dock.device.demo import make_demo_channels_table, make_demo_framebuffer
from kern_dock.device.firmware import build_kfw_backup_from_backend, restore_kfw_backup_to_backend
from kern_dock.device.keys import APP_SHORTCUTS, DOCK_ACTIONS, KEY_BINDINGS, binding_index, resolve_binding
from kern_dock.device.link import DockLink, REBOOT_BOOTLOADER, REBOOT_FIRMWARE
from kern_dock.device.t9 import T9Planner, T9_MAX_LEN, simulate_text_input, text_to_keypad_sequence
from kern_dock.device.usb import classify_usb_mode, detect_hardware_model
from kern_dock.plugins.builtin.table_inspector import parse_binary_table
from kern_dock.render.capture import RecordedFrame, RecordingSession, encode_png_rgba, render_handset_to_rgba, render_lcd_to_rgba
from kern_dock.render.handset.catalog import K1_MATRIX, K5_BOTTOM_MATRIX, MODELS, compute_handset_geometry
from kern_dock.render.handset.renderer import HandsetRenderer
from kern_dock.render.lcd import FastLcdState, LCD_PALETTES, LcdSettings
from kern_dock.system.icons import ICON_COLOR_PATH, ICON_SYMBOLIC_PATH, make_tray_argb_pixmap
from kern_dock.system.ipc import IpcClient
from kern_dock.system.tray import SystemTrayService
from kern_dock.ui.runtime import DockRuntime


class TestCoreAndSecurity(unittest.TestCase):
    def test_event_bus_exact_and_wildcard(self) -> None:
        bus = EventBus()
        seen: list[str] = []
        sid1 = bus.subscribe("link.connected", lambda e: seen.append(f"exact:{e.topic}"))
        sid2 = bus.subscribe("link.*", lambda e: seen.append(f"wild:{e.topic}"))
        bus.emit("link.connected", {"ok": True})
        self.assertEqual(seen, ["exact:link.connected", "wild:link.connected"])
        bus.unsubscribe(sid1)
        bus.unsubscribe(sid2)
        bus.emit("link.connected", {})
        self.assertEqual(len(seen), 2)

    def test_config_schema_validation_and_persistence(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            p = Path(td) / "config.toml"
            cfg = ConfigManager(path=p, persist=True)
            cfg.set("lcd.backlight", 99)  # Clamped to 10
            self.assertEqual(cfg.get("lcd.backlight"), 10)
            cfg.set("ui.color_scheme", "bogus")  # Falls back to default
            self.assertEqual(cfg.get("ui.color_scheme"), "dark")
            cfg.set("security.hardened", True)
            cfg.set_plugin_config("demo-plugin", {"enabled_extra": True})

            cfg2 = ConfigManager(path=p, persist=True)
            self.assertEqual(cfg2.get("lcd.backlight"), 10)
            self.assertTrue(cfg2.get("security.hardened"))
            self.assertEqual(cfg2.get_plugin_config("demo-plugin"), {"enabled_extra": True})

    def test_security_hardening_and_path_sanitization(self) -> None:
        self.assertEqual(sanitize_kernfs_path("/user//channels.ch"), "/user/channels.ch")
        for bad in ("", "/user/../sys/calib.dat", "/user/a\x00b", "/user/\nfoo"):
            with self.assertRaises(SecurityError):
                sanitize_kernfs_path(bad)

        sec = SecurityController(EventBus())
        with self.assertRaises(SecurityError):
            sec.check_tx(demo_mode=False)
        sec.set_tx_armed(True)
        sec.check_tx(demo_mode=False)

        sec.set_hardened(True)
        self.assertTrue(sec.hardened)
        self.assertFalse(sec.allow_usb_tx)
        self.assertFalse(sec.ipc_allow_control)
        with self.assertRaises(SecurityError):
            sec.check_bootloader_entry()
        with self.assertRaises(SecurityError):
            sec.check_ipc_command("freq", mutates=True)
        sec.check_ipc_command("status", mutates=False)
        self.assertGreaterEqual(len(sec.audit_entries()), 4)

    def test_registries(self) -> None:
        cr = CommandRegistry()
        ran: list[str] = []
        cr.register(ActionSpec("a1", "Tune 2m Calling", lambda: ran.append("ok"), category="Radio"))
        self.assertEqual([a.id for a in cr.search("2m")], ["a1"])
        cr.invoke("a1")
        self.assertEqual(ran, ["ok"])

        pr = PageRegistry()
        pr.register(PageSpec("z", "Z", "icon", "Tools", lambda: None, order=20))
        pr.register(PageSpec("a", "A", "icon", "Workspace", lambda: None, order=10))
        self.assertEqual([p.id for p in pr.ordered()], ["a", "z"])


class TestDeviceAndKeys(unittest.TestCase):
    def test_key_matrices_and_bindings(self) -> None:
        k1_keys = [k[0] for row in K1_MATRIX for k in row]
        k5_keys = ["menu", "up", "down", "exit"] + [k[0] for row in K5_BOTTOM_MATRIX for k in row]
        self.assertEqual(len(k1_keys), 16)
        self.assertEqual(len(k5_keys), 16)
        for kn in set(k1_keys + k5_keys):
            self.assertIn(kn, rigwire.KEY_CODES)

        idx = binding_index()
        for action, _desc, names in KEY_BINDINGS:
            self.assertTrue(names)
            self.assertTrue(action in DOCK_ACTIONS or action in rigwire.KEY_CODES)
        self.assertEqual(resolve_binding(idx, "Return", "Return", False), ("menu", False))
        self.assertEqual(resolve_binding(idx, "Return", "Return", True), ("menu", True))
        self.assertEqual(resolve_binding(idx, "BackSpace", "BackSpace", False), ("exit", False))
        self.assertEqual(resolve_binding(idx, "Escape", "Escape", False), ("exit", False))
        self.assertEqual(resolve_binding(idx, "asterisk", "8", True), ("star", False))
        self.assertEqual(resolve_binding(idx, "exclam", "1", True), ("1", True))
        self.assertEqual(resolve_binding(idx, "KP_7", "KP_7", False), ("7", False))
        # Letters must NOT be hijacked so direct typing works seamlessly
        self.assertEqual(resolve_binding(idx, "m", "m", False), (None, False))
        self.assertEqual(len({a[0] for a in APP_SHORTCUTS}), len(APP_SHORTCUTS))

    def test_usb_detection(self) -> None:
        self.assertEqual(detect_hardware_model("Quansheng UV-K1 1D9E"), "k1")
        self.assertEqual(detect_hardware_model("Quansheng UV-K5 v3 ABCD"), "k5v3")
        self.assertEqual(detect_hardware_model("Quansheng UV-K6 ABCD"), "k5v3")
        self.assertEqual(classify_usb_mode("36b7", "0001", "QUANSHENG K1 CPS"), ("stock_bl", True))
        self.assertEqual(classify_usb_mode("36b7", "ffff", "Quansheng UV-K1 1D9E"), ("rigwire", True))

    def test_t9_round_trips(self) -> None:
        for txt in ("HELLO", "HI", "Kern OS 1.0", "aaa", "CQ DX de IZ1ABC", "x@y.it", "73!"):
            self.assertEqual(simulate_text_input(text_to_keypad_sequence(txt)), txt)
        pl = T9Planner()
        seq = pl.plan_text("ABD") + pl.plan_backspace() + pl.plan_char("C")
        self.assertEqual(simulate_text_input(seq), "ABC")
        self.assertEqual(T9Planner().plan_backspace(), [])
        self.assertFalse(T9Planner().supports("€"))
        pl2 = T9Planner()
        self.assertGreater(len(pl2.plan_text("A" * 40)), 0)
        self.assertEqual(pl2.length, T9_MAX_LEN)

    def test_demo_link_backups_webdav_and_reboot(self) -> None:
        bus = EventBus()
        sec = SecurityController(bus)
        link = DockLink(bus, sec)
        link.connect("demo")
        self.assertTrue(link.connected and link.fs_backend is not None)
        self.assertIn("PONG", link.run_wire_command("ping"))

        # Binary storage table decode test
        parsed_ch = parse_binary_table(make_demo_channels_table(), b"CHAN", 64)
        self.assertEqual(parsed_ch["fourcc"], "CHAN")
        self.assertEqual(len(parsed_ch["records"]), 3)

        # .kfw backup + /sys shielded restore
        with tempfile.TemporaryDirectory() as td:
            bk_cal = Path(td) / "calib.kfw"
            bk_full = Path(td) / "full.kfw"
            bk_cal.write_bytes(build_kfw_backup_from_backend(link.fs_backend, volume_name=link.volume_name, calib_only=True))
            bk_full.write_bytes(build_kfw_backup_from_backend(link.fs_backend, volume_name=link.volume_name, calib_only=False))
            self.assertEqual(kfw.parse_kfw(bk_cal.read_bytes(), verify_root=True).kind, kfw.KIND_BACKUP)
            rest, skip = restore_kfw_backup_to_backend(bk_full, link.fs_backend, link.policy)
            self.assertGreaterEqual(rest, 3)
            self.assertGreaterEqual(skip, 2)

        # WebDAV mount test
        link.start_dav(http_port=0, read_only=True)
        conn = http.client.HTTPConnection("127.0.0.1", link.dav_port, timeout=5)
        try:
            conn.request("PROPFIND", "/", headers={"Depth": "1"})
            resp = conn.getresponse()
            self.assertEqual(resp.status, 207)
            self.assertIn(b"DEMO</D:displayname>", resp.read())
        finally:
            conn.close()
        link.stop_dav()

        # Bootloader + Hardened round trip
        sec.set_hardened(True)
        with self.assertRaises(PermissionError):
            link.reboot(REBOOT_BOOTLOADER)
        sec.set_hardened(False)
        link.reboot(REBOOT_BOOTLOADER)
        self.assertEqual(link.link_mode, "stock_bl")
        self.assertTrue(link.torch_on)
        link.demo_power_cycle()
        self.assertEqual(link.link_mode, "rigwire")
        link.reboot(REBOOT_FIRMWARE)
        link.disconnect()


class TestRenderingAndCapture(unittest.TestCase):
    def test_all_distinct_handsets_and_lcd_benchmark(self) -> None:
        self.assertEqual(
            set(MODELS.keys()),
            {"k1", "k1-polymer", "k1-clear", "k5v3", "k5-orange", "k5-black", "k5-99", "k5-99-red", "uv5r-plus", "k10-pro", "k1-naked", "pcb-r51"},
        )
        fb1 = make_demo_framebuffer("433.500", "145.500", "VFO")
        fb2 = make_demo_framebuffer("433.525", "145.500", "UP")
        st = LcdSettings(palette="UV-K1")
        fs = FastLcdState()
        self.assertTrue(fs.step(fb1, st))

        import cairo

        rend = HandsetRenderer()
        for key, spec in MODELS.items():
            self.assertIn(spec.palette, LCD_PALETTES)
            self.assertEqual(compute_handset_geometry(key), spec.size)
            w, h, rgba = render_handset_to_rgba(key, fb2, LcdSettings(palette=spec.palette), pressed_keys=frozenset({"up", "ptt"}), led_state="rx", renderer=rend)
            self.assertGreater(w, 200)
            self.assertGreater(h, 400)
            self.assertEqual(len(rgba), w * h * 4)

        w, h, rgba = render_lcd_to_rgba(fb1, st)
        png = encode_png_rgba(w, h, rgba)
        self.assertTrue(png.startswith(b"\x89PNG\r\n\x1a\n") and png.endswith(b"IEND\xaeB`\x82"))

        # Icon assets + tray ARGB pixmap
        self.assertTrue(ICON_COLOR_PATH.is_file())
        self.assertTrue(ICON_SYMBOLIC_PATH.is_file())
        tw, th, argb = make_tray_argb_pixmap(32, state="online")
        self.assertEqual((tw, th, len(argb)), (32, 32, 32 * 32 * 4))

        # GIF export + splice
        rec = RecordingSession()
        rec.frames = [
            RecordedFrame(0, fb1, frozenset(), "rx", False),
            RecordedFrame(150, fb2, frozenset({"up"}), "rx", False),
            RecordedFrame(450, fb2, frozenset(), "rx", False),
        ]
        rec.trim_in, rec.trim_out = 0, 2
        rec.add_cut(1, 1)
        self.assertEqual(rec.active_indices(), [0, 2])
        try:
            from PIL import Image as _pil  # noqa: F401

            with tempfile.TemporaryDirectory() as td:
                dest = Path(td) / "clip.gif"
                sz = rec.export_gif(dest, model="k1", lcd_st=st, mode="lcd")
                self.assertGreater(sz, 100)
                self.assertEqual(dest.read_bytes()[:6], b"GIF89a")
        except ImportError:
            pass

    def test_nearest_button_hit_test_and_alsa_silence(self) -> None:
        from kern_dock.audio.synth import silence_alsa_errors
        from kern_dock.ui.widgets.handset_stage import hit_test_handset

        silence_alsa_errors()
        boxes = {
            "small_a": (100.0, 100.0, 24.0, 24.0),  # min_dim=24 -> forgiving 13px reach
            "small_b": (140.0, 100.0, 24.0, 24.0),
            "big_key": (100.0, 200.0, 64.0, 40.0),  # min_dim=40 -> tighter 7px reach
        }
        # Exact hit always wins
        self.assertEqual(hit_test_handset(boxes, 110.0, 110.0), "small_a")
        # Near miss beside small button snaps to closest button
        self.assertEqual(hit_test_handset(boxes, 129.0, 112.0), "small_a")
        self.assertEqual(hit_test_handset(boxes, 134.0, 112.0), "small_b")
        # Far empty area returns None (never grabs from too far away)
        self.assertIsNone(hit_test_handset(boxes, 50.0, 50.0))
        self.assertIsNone(hit_test_handset(boxes, 130.0, 160.0))
        # Big key uses tighter reach (10px outside returns None, 5px outside hits)
        self.assertEqual(hit_test_handset(boxes, 130.0, 195.0), "big_key")
        self.assertIsNone(hit_test_handset(boxes, 130.0, 190.0))

    def test_zip_and_kfw_backups_and_hex_format_detection(self) -> None:
        from kern_dock.device.firmware import (
            build_kfw_backup_from_backend,
            build_zip_backup_from_backend,
            inspect_backup_file,
            restore_backup_to_backend,
        )
        from kern_dock.ui.widgets.hex_view import decode_inspector_values, detect_binary_format

        self.assertEqual(detect_binary_format(b"CHAN" + b"\x00" * 60, "channels.ch"), "Kern Table (CHAN) · 0 rec × 64 B")
        self.assertEqual(detect_binary_format(b"\x00" * 512, "calib.dat"), "Factory Calibration (512 B)")
        self.assertEqual(detect_binary_format(b"KRX1" + b"\x00" * 16, "app.krx"), "KRX / ELF Executable")
        dec = decode_inspector_values(b"\x41\x00\x01\x00", 0)
        self.assertEqual(dec["char"], "'A'")
        self.assertEqual(dec["bin"], "01000001")

        # Authentic Kern OS 16×16 pixel icon pack + KRX SECTION_ASSETS bitmap decoding
        from kern_dock.ui.widgets.app_icon import load_kern_pixel_icons, resolve_pixel_bitmap

        by_id, by_name = load_kern_pixel_icons()
        self.assertGreaterEqual(len(by_id), 10)
        self.assertIn("spectrum", by_name)
        bw, bh, bits = resolve_pixel_bitmap("dfu.mod")
        self.assertEqual((bw, bh, len(bits)), (16, 16, 32))

        # Bounded read_head avoids full-file CRC stat() and full-binary downloads
        import struct as _st
        import kernfs_mount as _kfm
        import rigwire as _rw

        payload = b"\xC0\xDE\x01\xFE" + _st.pack("<BBHHH", 0, 0, 1, 96, 0) + b"\x00" * 16 + b"\x55" * 4096
        mem = _kfm.MemoryFsBackend()
        mem.files["/apps/big.krx"] = payload
        self.assertEqual(len(mem.read_head("/apps/big.krx", 256)), 256)

        calls: list[int] = []

        class _StubClient:
            def stat(self, _path: str) -> dict:
                raise AssertionError("read_head must not call stat() full-file CRC scan")

            def send_record(self, _svc: int, op: int, req: bytes) -> bytes:
                calls.append(op)
                if op == _rw.OP_TRANSFER_OPEN:
                    return _st.pack("<HHI", 7, 120, len(payload))
                if op == _rw.OP_TRANSFER_CHUNK:
                    tid, off, req_len = _st.unpack("<HIH", req[:8])
                    chunk = payload[off : off + req_len]
                    return _st.pack("<HIH", tid, off, len(chunk)) + chunk
                if op == _rw.OP_TRANSFER_CLOSE:
                    return _st.pack("<HI", 7, 0)
                raise AssertionError(f"unexpected op {op}")

            def _abort_transfer(self, _tid: int) -> None:
                pass

        rw_be = _kfm.RigWireFsBackend(_StubClient())  # type: ignore[arg-type]
        head = rw_be.read_head("/apps/big.krx", 512)
        # Smart early-stop stops after first 120 B chunk because KRX hdr_len == 96 <= 120
        self.assertEqual(len(head), 120)
        self.assertEqual(calls.count(_rw.OP_TRANSFER_CHUNK), 1)
        # Second call hits _head_cache (0 chunk reads)
        head2 = rw_be.read_head("/apps/big.krx", 96)
        self.assertEqual(head2, payload[:96])
        self.assertEqual(calls.count(_rw.OP_TRANSFER_CHUNK), 1)

        # BK4829 DDS pitch quantization matches firmware tone_gen::calc_tone_word
        from kern_dock.audio.synth import bk4829_quantize_hz, synthesize_notes_pcm

        for freq in (440, 650, 880, 1046, 1175, 1318):
            self.assertAlmostEqual(bk4829_quantize_hz(freq), float(freq), delta=0.2)
        self.assertGreater(len(synthesize_notes_pcm([(1046, 20), (880, 40)])), 1000)

        # Rotary encoder delta in Demo mode steps VFO cleanly
        ev = EventBus()
        sec = SecurityController(ev)
        lk = DockLink(ev, sec)
        lk.connect("demo")
        f0 = lk.demo_freq_khz
        lk.send_encoder(-1)
        self.assertEqual(lk.demo_freq_khz, f0 + 25)
        lk.send_encoder(2)
        self.assertEqual(lk.demo_freq_khz, f0 - 25)
        lk.disconnect()

        bus = EventBus()
        sec = SecurityController(bus)
        link = DockLink(bus, sec)
        link.connect("demo")
        assert link.fs_backend is not None
        with tempfile.TemporaryDirectory() as td:
            kfw_p = Path(td) / "snap.kfw"
            zip_p = Path(td) / "snap.zip"
            kfw_p.write_bytes(build_kfw_backup_from_backend(link.fs_backend, volume_name="Demo"))
            zip_p.write_bytes(build_zip_backup_from_backend(link.fs_backend, volume_name="Demo"))
            ki = inspect_backup_file(kfw_p)
            zi = inspect_backup_file(zip_p)
            self.assertEqual(ki["format"], "KFW v1.0 (Ascon-Hash256)")
            self.assertEqual(zi["format"], "ZIP Archive")
            self.assertGreaterEqual(len(ki["entries"]), 4)
            self.assertGreaterEqual(len(zi["entries"]), 4)
            rest, skip = restore_backup_to_backend(zip_p, link.fs_backend, link.policy)
            self.assertGreater(rest, 0)
            self.assertGreaterEqual(skip, 1)
        link.disconnect()


class TestRuntimePluginsAndIpc(unittest.TestCase):
    def test_plugins_and_ipc_round_trip(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            sock = Path(td) / "dock.sock"
            cfg = Path(td) / "config.toml"
            rt = DockRuntime(
                DockOptions(no_tray=True),
                config_path=cfg,
                persist_config=True,
                socket_path=sock,
                enable_dbus=False,
            )
            rt.start_services()
            try:
                # Builtin + workspace example plugin discovered and active
                pids = {p["id"] for p in rt.plugins.summary()}
                self.assertTrue({"band-presets", "telemetry-monitor", "table-inspector", "connection-notifier"}.issubset(pids))

                # IPC client round trip
                cli = IpcClient(socket_path=sock)
                self.assertTrue(cli.call("ping")["ok"])
                self.assertTrue(cli.call("connect", {"port": "demo"})["ok"])
                self.assertTrue(cli.call("freq", {"mhz": 145.5})["ok"])
                st = cli.call("status")["result"]
                self.assertEqual(st["radio"]["rx_freq_hz"], 145_500_000)

                # Screenshot via IPC
                shot = Path(td) / "shot.png"
                sres = cli.call("screenshot", {"path": str(shot), "scope": "lcd"})
                self.assertTrue(sres["ok"] and shot.is_file() and shot.read_bytes().startswith(b"\x89PNG"))

                # Hardened mode blocks mutating IPC while keeping read-only status open
                rt.set_hardened(True)
                self.assertFalse(cli.call("freq", {"mhz": 433.5})["ok"])
                self.assertTrue(cli.call("status")["ok"])
            finally:
                rt.shutdown()

    def test_tray_get_layout_variant(self) -> None:
        items = [
            {"label": "Show Kern Dock", "handler": lambda: None},
            {"separator": True},
            {"label": "Flashlight", "checked": True, "handler": lambda: None},
            {"label": "Hardened Mode", "checked": False, "handler": lambda: None},
        ]
        svc = SystemTrayService(on_activate=lambda: None, menu_provider=lambda: items)
        v = svc._build_get_layout_variant()
        self.assertEqual(v.get_type_string(), "(u(ia{sv}av))")
        rev, root = v.unpack()
        self.assertEqual(rev, 1)
        root_id, root_props, children = root
        self.assertEqual(root_id, 0)
        self.assertEqual(root_props["children-display"], "submenu")
        self.assertEqual(len(children), 4)
        self.assertEqual(children[0][1]["label"], "Show Kern Dock")
        self.assertEqual(children[1][1]["type"], "separator")
        self.assertEqual(children[2][1]["toggle-state"], 1)
        self.assertEqual(children[3][1]["toggle-state"], 0)

    def test_kiss_tnc_and_rigctld_bridges(self) -> None:
        from kern_dock.bridges import KissTncBridge, RigctldBridge, kiss_decode_stream, kiss_encode

        # KISS framing round-trip with FEND & FESC escaping
        payload = bytes([0x82, 0xC0, 0xDB, 0xDC, 0xDD, 0x41])
        enc = kiss_encode(payload)
        buf = bytearray(enc)
        frames = kiss_decode_stream(buf)
        self.assertEqual(frames, [(0, 0, payload)])

        # Hamlib rigctld protocol round-trip on Demo link
        bus = EventBus()
        sec = SecurityController(bus)
        link = DockLink(bus, sec)
        link.connect("demo")
        try:
            rig = RigctldBridge(link, port=0)
            self.assertEqual(rig.handle_command("F 144800000"), "RPRT 0\n")
            self.assertEqual(rig.handle_command("f"), "144800000\n")
            self.assertEqual(rig.handle_command("M FM 12500"), "RPRT 0\n")
            self.assertTrue(rig.handle_command("m").startswith("FM\n"))
            self.assertEqual(rig.handle_command("v"), "VFOA\n")

            # KISS TNC TCP server start/stop smoke
            kiss = KissTncBridge(port=0)
            p = kiss.start()
            self.assertGreater(p, 0)
            kiss.stop()
        finally:
            link.disconnect()

    def test_headless_audio_synth_and_manager(self) -> None:
        from kern_dock.audio import AudioManager, synthesize_notes_pcm

        pcm = synthesize_notes_pcm([(880, 40), (0, 20), (1320, 60)], volume=0.4)
        # (40 + 20 + 60) ms @ 44100 Hz * 2 bytes/sample ≈ 10584 bytes
        self.assertGreater(len(pcm), 9000)
        self.assertEqual(synthesize_notes_pcm([(880, 40)], volume=0.0), b"")

        aud = AudioManager()
        self.assertTrue(aud.headphones_enabled)
        self.assertFalse(aud.mic_enabled)
        aud.set_headphones(False)
        aud.set_mic(True)
        self.assertFalse(aud.headphones_enabled)
        self.assertTrue(aud.mic_enabled)
        aud.shutdown()

    def test_flasher_preflight_and_sideload_manifest_parser(self) -> None:
        import struct
        import pack_krx
        from kern_dock.device.firmware import inspect_firmware_file
        from kern_dock.device.packages import (
            decode_caps_mask,
            discover_workspace_packages,
            encode_cap_names,
            inspect_krx_binary,
            inspect_krx_manifest,
            inspect_package_path,
        )

        # 1. Firmware pre-flight ARMv6-M vector validation & SHA-256
        with tempfile.TemporaryDirectory() as td:
            good_fw = Path(td) / "good.bin"
            # Initial SP = 0x20004000, Thumb Reset_Handler = 0x08002001
            good_fw.write_bytes(struct.pack("<II", 0x20004000, 0x08002001) + b"\x00" * 512)
            info = inspect_firmware_file(good_fw)
            self.assertTrue(info["vector_ok"])
            self.assertEqual(info["initial_sp"], 0x20004000)
            self.assertEqual(info["pages"], 3)
            self.assertEqual(len(info["sha256_short"]), 12)

            bad_fw = Path(td) / "bad.bin"
            bad_fw.write_bytes(b"\xFF" * 512)
            info_bad = inspect_firmware_file(bad_fw)
            self.assertFalse(info_bad["vector_ok"])

        # 2. Capability mask round-trip & sensitive classification
        mask = encode_cap_names(["DISPLAY_DIRECT", "KEYPAD", "RADIO_TX", "STORAGE_WRITE"])
        caps = decode_caps_mask(mask)
        by_ident = {c["ident"]: c for c in caps}
        self.assertIn("DISPLAY_DIRECT", by_ident)
        self.assertFalse(by_ident["DISPLAY_DIRECT"]["sensitive"])
        self.assertTrue(by_ident["RADIO_TX"]["sensitive"])
        self.assertTrue(by_ident["STORAGE_WRITE"]["sensitive"])

        # 3. Compiled KRX binary deep inspection (CRC-32, caps, actions, icon)
        b = pack_krx.KrxPackageBuilder("Cute App", pack_krx.CLASS_NATIVE, pack_krx.PROFILE_N1_PIC)
        b.set_identity("Cute App", "KERN-LAB", (2, 1, 0), 1)
        b.set_lifecycle(pack_krx.EXEC_APP, pack_krx.FLAG_SINGLETON)
        b.caps = mask
        b.add_action(1, 1, "Launch Cute", 0)
        b.add_icon_ascii(1, 8, 8, "\n".join(["########"] * 8))
        b.set_payload(b"\x70\x47\x00\x00")
        raw_krx = b.build()

        parsed = inspect_krx_binary(raw_krx, "cute_app.krx")
        self.assertTrue(parsed["valid_krx"])
        self.assertTrue(parsed["crc_ok"])
        self.assertEqual(parsed["name"], "Cute App")
        self.assertEqual(parsed["version"], "v2.1.0")
        self.assertEqual(parsed["author"], "KERN-LAB")
        self.assertEqual(parsed["lifecycle"], ["Singleton"])
        self.assertEqual(parsed["actions"], ["Launch Cute"])
        self.assertIsNotNone(parsed["bitmap"])
        self.assertEqual(len(parsed["cap_list"]), 4)

        # 4. Real workspace krx.toml manifest parsing & package discovery
        pkgs = discover_workspace_packages(_REPO_ROOT)
        self.assertGreaterEqual(len(pkgs), 2)
        settings_toml = _REPO_ROOT / "packages" / "core" / "settings" / "krx.toml"
        if settings_toml.is_file():
            sm = inspect_krx_manifest(settings_toml, group="core", pkg_id="core/settings")
            self.assertEqual(sm["name"], "Settings")
            self.assertEqual(sm["filename"], "settings.krx")
            self.assertIsNotNone(sm["bitmap"])
            self.assertGreaterEqual(len(sm["cap_list"]), 5)
            sm2 = inspect_package_path(settings_toml.parent)
            self.assertEqual(sm2["name"], "Settings")
        dfu_toml = _REPO_ROOT / "packages" / "sys" / "dfu" / "krx.toml"
        if dfu_toml.is_file():
            dm = inspect_krx_manifest(dfu_toml, group="sys", pkg_id="sys/dfu")
            self.assertIsNotNone(dm["bitmap"])
            self.assertEqual(dm["bitmap"][:2], (16, 16))

        # 5. Unknown icon returns None (falls back to native GTK4 executable icon) & narrow bitmaps decode accurately
        from kern_dock.ui.widgets.app_icon import _pack_ascii_grid, _pixel_is_set, resolve_pixel_bitmap

        self.assertIsNone(resolve_pixel_bitmap("totally_unknown_custom_app.krx"))
        bits5 = _pack_ascii_grid(["#...#", ".#.#.", "..#..", ".#.#.", "#...#"], 5, 5)
        self.assertTrue(_pixel_is_set(bits5, 5, 0, 0))
        self.assertFalse(_pixel_is_set(bits5, 5, 1, 0))
        self.assertTrue(_pixel_is_set(bits5, 5, 4, 0))
        self.assertTrue(_pixel_is_set(bits5, 5, 2, 2))


if __name__ == "__main__":
    unittest.main()
