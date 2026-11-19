# 📤 KERN DFU Update Module (`/apps/dfu.mod`)

A relocatable, zero-firmware-bloat **P0 Structured Paged (`FLAG_SYSTEM_ACTION`)** KRX system module providing a balanced USB-C / UART DFU UI, an on-device **DFU Tools** menu (flat firmware/applet browser, Calibration manager, Buddy Rescue, and Bootloader Information), multi-bootloader fingerprinting, hardware quiescing, the open **KBHP v1.0** firmware-to-bootloader handoff protocol, and a modular 30 Hz bare-metal SRAM SysTick telemetry & anti-brick runtime on Quansheng UV-K1 / UV-K5 v3 (`PY32F071xB`).

> 🛡️ **Zero-SWD-Brick Guarantee:** `/apps/dfu.mod` **never** erases or writes below `0x0800_2800`, never modifies `0x0800_2700` (factory provisioning), never touches Option Bytes (`RDP`/`WRP`), and never reconfigures `PA13`/`PA14` (`SWDIO`/`SWCLK`).

---

## 🎨 DFU Interface & Controls

The 128×64 ST7565 DFU screen is vertically balanced with strict page-aligned zones and zero visual clutter:

- **Top Header (Page 0):** Auto-detected bootloader version & build date (e.g. `v7.02.02 (2025-08-08)`, `Moto BL`, or `Custom`) alongside the taller UV-K1 antenna (`y = 1..9`).
- **Radio · Connection Arrow · USB-C Cable (Pages 1–3):** Vertically centered across `y = 10..31` with equal 9 px horizontal spacing.
  - **Connection Arrow (`x = 49..67`):** Represents live link status — slow ~1 Hz blink in Standby, solid steady when USB or UART is connected / ready / writing / complete, brisk activity pulse on active command traffic (`Link active`), and hidden on mid-flash abort.
  - **Link Badge (`x = 76..115`, Page 1):** Displays USB `VID:PID` (`36B7:0001`) or live UART baud (`115200bps`) directly above the USB-C plug when connected.
  - **Radio LCD Cutout (`x = 19..34`):** Displays `"DFU"` at rest, live percentage (`0%..100%`) during writes, and alternates `100%` / `<N>s` elapsed time on completion.
- **Dual-Mode Status & Progress Band (Pages 4–5, `y = 32..47`):**
  - **When Idle / Connected / Ready:** Displays an expressive, tool-agnostic **two-line** status (`DFU Mode` / `Plug in USB or serial`, `USB connected` / `Waiting for host...`, `Host ready` / `Ready for transfer`, `Link active` / `Exchanging data...`, `Serial connected` / `Ready on 2-pin serial`). All Page-5 glyphs including lowercase descenders sit cleanly at baseline `y = 46` (zero Page-6 bleed).
  - **When Writing / Complete / Aborted:** Page 4 (`y = 34..39`) transforms into a sleek 6 px rounded progress bar (dashed on stall/abort) while Page 5 shows `Writing page X/Y`, `Transfer complete`, or `Aborted - retry`.
- **Two-Sided Soft Bar (`y = 54..60`, native square Kern key badges):**
  - Left: **`[MENU] Tools`** — tap when idle to jump seamlessly into the on-device **Recovery** menu (signaled across warm reset via `BackupCrashTag::FLAG_DFU_TOOLS_REQ` while keeping `BACKLIGHT_DUTY = 10` and ST7565 continuously lit — zero LCD reset or PWM blink).
  - Right: **`[EXIT] Reboot`** — hold 0.5 s when idle to verify application vectors and reboot into Kern OS.
  - Side Key 1 short press toggles LCD backlight; long hold (0.5 s) also performs a verified reboot.

---

## 🧰 Recovery Menu (`[MENU] Tools`)

Pressing **`MENU`** from idle DFU opens an icon-driven **Recovery** menu rendered with Kern's **Comfortable (`13 px`)** row density (icons + right chevrons matching Settings), while flat file lists use **Compact (`10 px`)** density (4 rows visible):

| Item | Purpose |
| :--- | :--- |
| **Flash Image** | Compact flat browser listing flashable images (`.kfw`, `.uf2`, `.bin`) across `/boot`, `/apps`, and `/` (no folder or icon clutter) with pre-flight format (`KFW1`, `UF2`, Raw ARM) and 118 KB size verification. |
| **Applets** | Compact flat browser listing executable packages and diagnostic applets (`.krx`, `.ram`, `.mod`) with format & size inspection. |
| **Radio Clone** | Measures the active local image at `0x0800_2800` and reports 2-pin K1 (`38400` baud) radio-to-radio serial transfer readiness. |
| **Calibration** | Inspects `/sys/calib.dat`, `/sys/calib.sig` (Ascon-128 MAC), and `0x0800_2750` (`QS-1` factory page), with one-press backup to `/boot/calib.bkp`. |
| **Information** | Displays bootloader family & version, USB `VID:PID`, `QS-1` factory ID status, battery voltage / app flash region, MCU target (`PY32F071`), and `KBHP 1.0`. |

Pressing **`[EXIT] DFU`** returns seamlessly to live DFU mode.

---

## 📸 Screenshots

| Services List (`Settings → Apps → Services`) | Hold-Confirm Bottom Sheet (`Settings → System`) |
| :---: | :---: |
| ![Settings Services List](screenshots/settings_services.png) | ![Settings DFU Confirm](screenshots/settings_confirm.png) |

| Standby (`Plug in cable`) | USB Enumerated (`Ready to flash`) |
| :---: | :---: |
| ![DFU Standby](screenshots/dfu_standby.png) | ![DFU USB Ready](screenshots/dfu_usb_ready.png) |

| Active Flash (`Flashing page 18/64`) | Flash Complete (`100%` / elapsed seconds) |
| :---: | :---: |
| ![DFU Flashing](screenshots/dfu_flashing.png) | ![DFU Complete](screenshots/dfu_complete.png) |

---

## 🏗️ Architecture & Modular Layout

Because `/apps/dfu.mod` loads from 2 MB KernFS (`PY25Q16`) via **P0 Structured Paging** (1 resident root + up to 16 × 1,024 B on-demand overlay leaves), all DFU UI, Tools screens, ROM probing, and SysTick hook payloads cost **0 bytes of resident main firmware Flash**.

```text
packages/dfu/
├── README.md                    # Module documentation, UX guide, KBHP v1.0 overview & install guide
├── build.sh                     # Cortex-M0+ PIC build, _Static_assert ABI checks, size/reloc guards & packer
├── dfu.mod                      # Verified P0 Structured Paged module (12,857 B, install to /apps/dfu.mod)
├── krx.ld                       # Shared Thumb-1 PIC linker script (.text.entry pinned at 0x00000000)
├── screenshots/                 # Rendered ST7565 128x64 UI screenshots
└── src/
    ├── dfu_abi.h                # Single-source SRAM map, RTC_ALR tools tag, KBHP v1.0 & DfuHookState contracts
    ├── dfu_shared.h             # DfuSharedState, TRANSIENT_ARENA staging scratch & resident DfuUiOps
    ├── dfu_quiesce.h            # Hardware quiescing, GPIO LCKR hardening, PF8 backlight latch & USB SE0 detach
    ├── dfu.c                    # Resident root (4,116 B): native badge soft bar, P0 dispatch, MSP evac & handoff
    ├── leaf1_splash.c           # Leaf 1  (880 B): Taller-antenna PackBits RLE splash & bootloader header
    ├── leaf2_modal.c            # Leaf 2  (411 B): Settings hold-confirm modal + low-battery brownout warning
    ├── leaf3_stage.c            # Leaf 3  (992 B): Key-release guard, soft bar & double-line SRAM strip staging
    ├── leaf4_probe.c            # Leaf 4 (1,013 B): Multi-BL ROM fingerprint, 0x0800_2750 check & KBHP prep
    ├── leaf5_tools.c            # Leaf 5  (750 B): RTC_ALR tools gate & Comfortable 13 px Recovery menu
    ├── leaf6_info.c             # Leaf 6  (770 B): Bootloader & hardware Information screen
    ├── leaf7_calib.c            # Leaf 7  (897 B): Calibration inspector & /boot/calib.bkp backup
    ├── leaf8_rescue.c           # Leaf 8  (540 B): Radio Clone 2-pin serial readiness screen
    ├── leaf9_scan.c             # Leaf 9  (447 B): Flat filesystem scanner (/boot, /apps, /)
    ├── leaf10_files.c           # Leaf 10 (1,016 B): Compact 10 px flat image & applet browser
    ├── leaf11_inspect.c         # Leaf 11 (705 B): Pre-flight KFW1/UF2/KRX/ARM format & 118 KB fit check
    └── hook/
        ├── dfu_hook.S           # Top-level modular bare-metal SysTick hook assembler unit (2,260 B)
        ├── hook_defs.inc        # Symbolic SRAM/MMIO/struct offsets (compile-time verified vs dfu_abi.h)
        ├── hook_main.S          # 30 Hz SysTick ISR: safety lock, EXIT/MENU guard, arrow/bar & KBHP mirror
        └── hook_drivers.S       # Bare-metal drivers: RTC_ALR tools reboot, ST7565 SPI1, BK4829 LED & glyphs
```

---

## 🧭 Multi-Bootloader Detection & Compatibility Matrix

`leaf4_probe.c` non-destructively fingerprints `0x0800_0000..0x0800_27FF` before handoff and chooses the safest mode automatically:

| Detected Family (`KBHP_BL_*`) | Fingerprint Criteria | Handoff Behavior |
| :--- | :--- | :--- |
| **`KBHP_BL_STOCK_K1` (`1`)** | Valid RVCT trampoline + `36B7:0001` USB descriptor + 16 B Quansheng XOR key + verified `7.00..7.03` version | Attaches full 30 Hz SRAM SysTick hook: connection arrow state, on-demand progress bar, page counter, LED choreography, 10 s stall detector, `[EXIT]` anti-brick reboot, `[MENU] Tools` bridge, and KBHP live telemetry mirror. |
| **`KBHP_BL_STOCK_SAFE` (`2`)** | Valid RVCT trampoline, unrecognized version or descriptor | Attaches **safe guard-only hook** (`HOOK_FLAG_STOCK_K1 = 0`): enforces `PC13` flashlight lock, Side Key 1 backlight toggle, and hardened `0x0800_2800` vector check on `EXIT`/`MENU` without reading unverified stock SRAM offsets. |
| **`KBHP_BL_MOTO` (`3`)** | `36B7:FFFF` USB descriptor or `"Moto"` ROM signature | Displays `"Moto BL"`, locks `PB10` LOW and `PB14`/`PB15` HIGH (matching Moto's exact `!PB10 && PB14 && PB15` UF2 DFU entry chord), publishes KBHP envelope, validates `0x0800_0000` vectors, and hands off cleanly. |
| **`KBHP_BL_UNKNOWN` (`0`)** | Any other custom primary bootloader | Displays `"Custom"`, publishes KBHP envelope, validates `0x0800_0000` vectors (refusing wild jumps via clean `AIRCR` reset), and performs raw handoff. |

---

## 🔗 Open Firmware ↔ Bootloader Protocol (`KBHP v1.0`)

To let **any** firmware (Kern, F4HWN, DeltaFW, Egzumer/CEC, UV-KX, custom C/Rust) exchange structured identity, policy, and live telemetry with hooks and future Stage-2 / custom bootloaders **without breaking factory ROM bootloaders**, `dfu.c` publishes a 64-byte (`2^6`), naturally aligned `KbhpEnvelope` at **`0x2000_35C0..0x2000_35FF`**:

- **100% Factory-Safe:** Stock Quansheng bootloaders (`7.00..7.03`) keep `.data`/`.bss`/stack strictly inside `0x2000_0000..0x2000_2FFF` and never touch `0x2000_35C0..0x2000_35FF`. Moto re-initializes SRAM cleanly on entry.
- **Checksum-Invariant Split Layout ([`src/dfu_abi.h`](src/dfu_abi.h)):**
  - **Bytes `[0x00..0x2F]` (48 B — Immutable Signed Header):** `ident` (`"KBHP"`), `version` (`0x0100`), `cmd`, `bl_kind`, `flags`, `target_id` (`0x5153_0101`), vendor-neutral `origin_tag` (`"KERN"`), `bl_vid_pid`, `fw_version`, `battery_mv`, `app_max_pages`, `bl_ver[8]`, `app_flash_base`, sealed by FNV-1a 32-bit `header_checksum` at `+0x2C`.
  - **Bytes `[0x30..0x3F]` (16 B — Mutable Live Telemetry Block):** `status`, `error_code`, `hook_ver`, `flags_live`, `flashed_pages`, `total_pages`, `elapsed_sec`, `boot_strikes`, `hook_state_addr` — updated at 30 Hz without invalidating `header_checksum`.
- **Full Normative Spec & Roadmap:** See [`docs/bootloader-protocol.md`](../../docs/bootloader-protocol.md).

---

## 🛡️ Hardened Anti-Brick Safeguards

- **Dynamic `MSP` Evacuation:** Evacuates `MSP` strictly below `dfu_root_base & ~7` before staging copies (never colliding with `g_state` or upper SRAM) and copies UI strips in overlap-safe reverse order (`dfu_move_bytes`).
- **Battery Brownout Warning (`leaf2_modal.c`):** Checks `OP_IO_POWER` (`LOW` / `CRITICAL` / `CHARGING`) before showing the Settings hold-confirm sheet and warns users to plug in USB-C power first when battery is low.
- **Mid-Flash & Stall Lockout (`hook_main.S`):** Blocks `EXIT`, `Side Key 1`, and `MENU` reboots whenever `FLASH->SR.BSY == 1`, `FLASH->CR.LOCK == 0`, active flashing (`state == 2`), or mid-flash abort (`FLAG_WAS_FLASH`) is latched. Detects 10-second stalls (`300` ticks @ 30 Hz) and displays `"Aborted - reflash"` with a red blinking LED, dashed bar, and hidden connection arrow.
- **4-Point Vector Verification (`hook_main.S` & `dfu_quiesce.h`):** Before rebooting to application firmware, opening DFU Tools, or jumping to an unknown bootloader, validates 4-byte-aligned SRAM `MSP`, odd Thumb `Reset`/`NMI`/`HardFault` vectors inside valid flash bounds, and non-erased entry code.
- **Hardware Pin Locking (`dfu_quiesce.h`):** Locks `PB10` (PTT LOW), `PB14`/`PB15` (side keys HIGH), and `PC13` (flashlight LOW) via `GPIOx->LCKR`; keeps `PB3` (`EXIT`) and `PB6` (`MENU`) pulse-scannable; never alters `PA13`/`PA14` (SWD).

---

## 🗺️ Memory Map During Bootloader Handoff

| Region | Size | Purpose |
| :--- | :--- | :--- |
| `0x2000_0000..dfu_root_base` | ~7.3 KB | Evacuated `MSP` stack (growing down below `TRANSIENT_ARENA`) during handoff |
| `TRANSIENT_ARENA[0..4116)` | 4,116 B | Resident root (`dfu.c`, native badge soft bar, embedded `dfu_hook.bin`, `g_state`) |
| `TRANSIENT_ARENA[4160..5120)` | 960 B | Dual-use scratch (`DfuToolsScratch` in Recovery; staged banners + titles before handoff) |
| `TRANSIENT_ARENA[5120..6144)` | 1,024 B | Shared P0 leaf overlay window (`leaf1..leaf11`, 11 on-demand leaves) |
| `0x2000_3000..0x2000_327F` | 640 B | Staged Page-5 status subtitles (`8 × 80 B`, baseline `y = 46`, descender-safe) |
| `0x2000_3280..0x2000_33BF` | 320 B | Staged Page-4 idle primary titles (`5 × 64 B`, baseline `y = 38`, power-of-2 stride) |
| `0x2000_3540..0x2000_35A7` | 78 B | Staged connection arrow (`38 B`) & Page-1 `VID:PID` badge (`40 B`) |
| `0x2000_35C0..0x2000_35FF` | 64 B | **KBHP v1.0 Handoff Envelope** (48 B signed header + 16 B live telemetry) |
| `0x2000_3600..0x2000_36BF` | 192 B | Relocated Cortex-M0+ `VTOR` vector table (SysTick -> `0x2000_3701`) |
| `0x2000_36C0..0x2000_36FF` | 64 B | Live SysTick hook runtime state (`DfuHookState`) |
| `0x2000_3700..0x2000_3FFF` | 2,304 B | Live `dfu_hook.bin` SysTick ISR + drivers (`2,260 B`) |

---

## 🔧 Build & Verify

```bash
bash packages/dfu/build.sh
```

`build.sh` assembles `src/hook/dfu_hook.S`, compiles `src/dfu.c` and all 11 leaves with `-fPIC -fPIE -fvisibility=hidden`, enforces `_Static_assert` ABI alignment, zero undefined symbols, zero ELF relocations, zero `.got` sections, exact `0x00000000` entry offsets, and strict SRAM limits (`hook <= 2304 B`, `root <= 4900 B`, each leaf `<= 1024 B`), then packs and verifies `packages/dfu/dfu.mod` via [pack_krx.py](../../tools/pack_krx.py).

---

## ⚡ Sideloading & Installation (RigWire)

Install `packages/dfu/dfu.mod` to `/apps/dfu.mod` on KernFS using RigWire (USB-C CDC or UART @ 115200 baud):

```bash
# Recommended: validates KRX header, profile, capabilities, and CRC-32 before & after upload
./kern wire sideload packages/dfu/dfu.mod --force

# Specify serial port explicitly if needed (e.g. /dev/ttyACM0, /dev/ttyUSB0, COM3)
./kern wire -p /dev/ttyACM0 sideload packages/dfu/dfu.mod --force

# Verify installation on KernFS
./kern wire stat /apps/dfu.mod
```

> 💡 **Note:** Keep the `.mod` extension so cold-boot `PTT + Side Key 2` resolves `/apps/dfu.mod` directly. To inspect or remove the module on-device, open **Settings → Apps → Services → DFU Update**, or run `./kern wire rm /apps/dfu.mod`.

---

## 📚 Related Specifications

- [`docs/bootloader-protocol.md`](../../docs/bootloader-protocol.md) — Normative **KBHP v1.0** open handoff standard, bootloader audit (`moto` / `ichi` / `TinyK5BL`), RigWire integration & next-gen anti-brick roadmap
- [`docs/stage2-boot-architecture.md`](../../docs/stage2-boot-architecture.md) — Stage-2 `kboot` anchor (`0x0801_E800`), volatile `PY25Q16` guest lock & 3-strike auto-rollback
- [`docs/kfw-spec.md`](../../docs/kfw-spec.md) — `.kfw` Ascon-verified firmware & device backup container format
- [`docs/rigwire.md`](../../docs/rigwire.md) — RigWire v2 serial control, `CORE::RUNLEVEL`, and streaming storage fabric
- [`docs/stock-bootloader-provisioning.md`](../../docs/stock-bootloader-provisioning.md) — Stock Quansheng PY32F071 bootloader & `0x0800_2700` factory provisioning analysis
