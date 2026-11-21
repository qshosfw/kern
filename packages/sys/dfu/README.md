# 📤 KERN DFU Update Module (`/apps/dfu.mod`)

A lean, relocatable, zero-firmware-bloat **P0 Structured Paged (`FLAG_SYSTEM_ACTION`)** KRX system module providing USB-C / UART DFU handoff, multi-bootloader fingerprinting, hardware quiescing, the open **KBHP v1.0** firmware-to-bootloader handoff protocol, and a 30 Hz bare-metal SRAM SysTick telemetry & anti-brick runtime on Quansheng UV-K1 / UV-K5 v3 (`PY32F071xB`).

DFU is **presence-aware** and cooperates cleanly with the standalone [`packages/recovery/`](../recovery/README.md) (`/apps/recovery.mod`) module:

- **When `/apps/recovery.mod` is installed:** Shows `[MENU] Tools` on the bottom soft bar and enables the instant, zero-flash Cortex-M0+ `EXC_RETURN` jump directly into Kern Recovery while idle.
- **When `/apps/recovery.mod` is NOT installed:** Automatically hides `[MENU] Tools` (showing only `[EXIT] Reboot`) and sets `HOOK_FLAG_HAS_RECOVERY = 0` so `hook_main.S` ignores `MENU` completely.

> 🛡️ **Zero-SWD-Brick Guarantee:** `/apps/dfu.mod` **never** erases or writes below `0x0800_2800`, never modifies `0x0800_2700` (factory provisioning), never touches Option Bytes (`RDP`/`WRP`), and never reconfigures `PA13`/`PA14` (`SWDIO`/`SWCLK`).

---

## 🎨 DFU Interface & Controls

The 128×64 ST7565 DFU screen is vertically balanced with strict page-aligned zones and zero visual clutter:

- **Top Header (Page 0):** Auto-detected bootloader version & build date (e.g. `v7.02.02 (2025-08-08)`, `Moto BL`, or `Custom`) alongside the minimalist UV-K1 antenna (`y = 1..9`) and solid power/volume knob (`x = 33..36, y = 7..9`).
- **Radio · Connection Arrow · USB-C Cable (Pages 1–3):** Vertically centered across `y = 10..31` with equal 9 px horizontal spacing, taller 18×9 px LCD cutout (`y = 12..20`), subtle PTT + side button, and 2 sleek speaker grille slots.
  - **Connection Arrow (`x = 49..67`):** Slow ~1 Hz blink in Standby, solid steady when USB or UART is connected / ready / writing / complete, brisk activity pulse on active command traffic (`Link active`), and hidden on mid-flash abort.
  - **Link Badge (`x = 76..115`, Page 1):** Displays USB `VID:PID` (`36B7:0001`) or live UART baud (`115200bps`) directly above the USB-C plug when connected.
  - **Radio LCD Cutout (`x = 19..34, y = 12..20`):** Displays `"DFU"` at rest, live percentage (`0%..100%`) during writes, and alternates `100%` / `<N>s` elapsed time on completion.
- **Dual-Mode Status & Progress Band (Pages 4–5, `y = 32..47`):**
  - **When Idle / Connected / Ready:** Two-line status at baselines `y = 39` / `y = 47` (`DFU Mode` / `Connect USB/serial`, `USB connected` / `Wait for host...`, `Host linked` / `Clear for transfer`, `Link active` / `Data in transit...`, `Serial linked` / `Serial cable link`).
  - **When Writing / Complete / Aborted:** Page 4 (`y = 34..39`) renders a 6 px rounded progress bar (dashed on stall/abort) while Page 5 (`y = 47`) shows `Write block X/Y`, `Transfer done`, or `Halted - reconnect`.
- **Two-Sided Soft Bar (`y = 54..60`):**
  - Left (only if `/apps/recovery.mod` is installed): **`[MENU] Tools`** — tap when idle to jump directly into Kern Recovery via Cortex-M0+ `EXC_RETURN` (`0xFFFF_FFF9`) with zero screen flash.
  - Right: **`[EXIT] Reboot`** — hold 0.5 s when idle to verify application vectors and reboot into Kern OS.
  - Side Key 1 short press toggles LCD backlight; long hold (0.5 s) also performs a verified reboot.

---

## 🏗️ Modular Layout (4 Leaves, `7,349 B`)

```text
packages/dfu/
├── README.md                    # Module documentation & install guide
├── krx.toml                     # Package manifest: units, size windows, capabilities, icon, Settings placement
├── dfu.mod                      # Verified P0 Structured Paged module (7,349 B -> /apps/dfu.mod)
├── screenshots/                 # Rendered ST7565 128x64 UI screenshots
└── src/
    ├── dfu_abi.h                # SRAM map, RTC_ALR tag, HOOK_FLAG_HAS_RECOVERY & KBHP v1.0
    ├── dfu_shared.h             # DfuSharedState & TRANSIENT_ARENA staging scratch
    ├── dfu_quiesce.h            # Hardware quiescing, GPIO LCKR hardening, PF8 latch & USB detach
    ├── dfu.c                    # Resident root (3,832 B): /apps/recovery.mod probe, P0 chain & handoff
    ├── leaf1_splash.c           # Leaf 1  (875 B): UV-K1 + USB-C PackBits RLE splash art
    ├── leaf2_modal.c            # Leaf 2  (411 B): Settings hold-confirm modal + low-battery warning
    ├── leaf3_stage.c            # Leaf 3 (1,020 B): Key-release guard, presence-aware soft bar & strip staging
    ├── leaf4_probe.c            # Leaf 4 (1,013 B): Multi-BL ROM fingerprint, 0x0800_2750 check & KBHP prep
    └── hook/
        ├── dfu_hook.S           # Top-level modular bare-metal SysTick hook assembler unit (2,304 B)
        ├── hook_defs.inc        # Symbolic SRAM/MMIO/struct offsets (verified against dfu_abi.h)
        ├── hook_main.S          # 30 Hz SysTick ISR: safety lock, gated MENU/EXIT, arrow/bar & KBHP
        └── hook_drivers.S       # Bare-metal drivers: EXC_RETURN tools jump, ST7565 SPI1, BK4829 LED & glyphs
```

---

## 🔧 Build & Sideload

```bash
./kern pkg dfu
./kern wire sideload packages/dfu/dfu.mod --force
```

---

## 📚 Companion Modules & Specifications

- [`packages/recovery/README.md`](../recovery/README.md) — Standalone on-device Recovery suite & anti-brick SRAM flasher (`/apps/recovery.mod`)
- [`packages/multiboot/README.md`](../multiboot/README.md) — Golden backup & multiboot slot manager (`/apps/multiboot.mod`)
- [`packages/calib/README.md`](../calib/README.md) — Standalone calibration editor & multimeter battery wizard (`/apps/calib.mod`)
- [`docs/bootloader-protocol.md`](../../docs/bootloader-protocol.md) — Normative **KBHP v1.0** open handoff standard
- [`docs/kfw-spec.md`](../../docs/kfw-spec.md) — `.kfw` v1.0 firmware & backup container specification
