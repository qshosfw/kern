# Autonomous Systems Research & Architecture Mandate: KRN Microkernel & Firmware Framework

> **Target:** Autonomous Research & Systems Architecture Agent  
> **Repository:** `kern` (Amateur Radio Firmware & Microkernel Platform)  
> **Silicon Target:** ARM Cortex-M0+ (PY32F071xB) @ 48 MHz / 72 MHz  
> **Memory Realities:** 128 KiB On-Chip Flash (120 KiB allocatable), 16 KiB SRAM, 2 MiB External SPI NOR (PY25Q16)  
> **Execution Baseline:** `#![no_std]`, Zero Heap (`no_alloc`), Zero `core::fmt`, Zero Unsafe Memory Leaks, Strict 0 Compiler Warnings

---

## 1. Executive Context & Architectural Philosophy

The `kern` project is a mathematically verifiable, capability-secure microkernel and firmware platform designed for handheld transceiver hardware (specifically the Quansheng UV-K1, UV-K5, UV-K5v3, and future modular hardware variations).

Monolithic C firmwares in this domain (e.g. F4HWN, trakt, delta, HAWK) are monolithic forks of vendor codebases that suffer from fundamental architectural flaws:
1. Hardcoded, non-relocatable feature bloat directly compiled into on-chip MCU Flash.
2. Direct register manipulation of peripheral transceivers (BK4819/BK4829) scattered across UI code.
3. Brittle static memory overlays with unverified stack growth and zero memory protection.
4. Monolithic configuration blobs where changing a single byte corrupts EEPROM / flash layouts.

### The KRN Microkernel Philosophy:
1. **Abstracted**: Strict layered decoupling. User applications NEVER interact directly with peripheral hardware registers (BK4819, Si4732). All radio operations route through microkernel traits (`RfChannel`, `RfTuning`, `RfModulation`, `RfSquelch`, `RfSubaudible`, `RadioCoordinator`).
2. **Lightweight**: Zero heap allocation across the entire workspace. Release firmware target is $< 80$ KiB Flash ($< 65\%$ utilization) to leave ample headroom for installable apps and user storage.
3. **Efficient**: $O(1)$ power-of-two geometries ($2^N$ bytes) for shift-only seeking (`16 + (index << N)`). Zero software division on Cortex-M0+. Sector-boundary aligned LittleFS flash I/O.
4. **Modular**: Settings partitioned into domain-specific, CRC32-protected files (`/pref/*.cfg`).
5. **Flexible**: Multi-frontend support (BK4819 VHF/UHF, Si4732 HF, BK1080 FM) without structural refactoring.
6. **Extensible**: Capability-checked installable packages (`.krx`, `.app`) executing in a shared 4 KiB transient arena.
7. **Elegant**: Zero type aliases as compatibility shims. Zero warnings. FourCC identifiers.
8. **KISS (Keep It Simple, Sensible)**: Eliminate unnecessary layers of indirection and convoluted mechanisms whenever a clean, direct solution exists.

---

## 2. Silicon Realities & Non-Negotiable Invariants

### 2.1 ARM Cortex-M0+ (PY32F071xB) Realities
1. **Zero Hardware Divider**: Division and modulo instructions do NOT exist in hardware. Integer division invokes runtime software routines taking ~45–65 cycles. Modulo and division must NEVER appear in seeking, index calculations, or hot DSP loops. All database record lengths, headers, and buffers must be exact powers of two ($2^N$), enabling bit-shift indexing (`offset = 16 + (index << N)`).
2. **Single-Cycle 32×32 Multiplier**: PY32F071xB implements the single-cycle fast multiplier. Fixed-point arithmetic and Goertzel tone detection run at maximum speed (18–22 cycles).
3. **2-Stage Instruction Pipeline (1 CPI)**: No branch prediction. Branches and jumps carry a 2-cycle penalty. Hot loops should minimize branching and eliminate virtual function table dispatch (`dyn Trait`) in hot driver loops.
4. **Thumb-16 Dominant Instruction Set**: Cortex-M0+ supports only Thumb-16 and a tiny subset of 32-bit Thumb-2 instructions (`BL`, `DMB`, `DSB`, `ISB`, `MRS`, `MSR`). Code density is high, but register spills quickly bloat Flash.

### 2.2 System Invariants
- **Zero Dynamic Heap Allocation**: The entire workspace executes `#![no_std]` without `alloc`. No `Box`, `Vec`, `String`. All memory must be bounded with `heapless` or stack-allocated.
- **Flash Budget Target (< 80 KiB / < 65% Utilization)**: The core release firmware must remain under 80 KiB of Flash to guarantee headroom for user storage, resident services, and installable applications.
- **Zero UI/Navigation Lag Invariant (< 30 ms Response)**: Navigation transitions (VFO to Launcher, Launcher to Settings, Option Sheets, Modals) must execute instantaneously. No peripheral operation (e.g. audio amplifier warm-up, flash metadata scans, tone synthesis) may block the interactive UI event loop. File and package catalogs must be cached in RAM rather than re-queried during UI transitions.
- **State & Lifecycle Preservation Invariant**: View context, cursor indices, active frequencies, and sub-menu states must NEVER be discarded or cleared unintentionally on application transitions.
- **No Type Aliases**: Strictly avoid using type aliases as backward-compatibility crutches, compatibility shims, or cosmetic rename layers. When types are refactored, definition and call sites must be refactored cleanly across the entire workspace.
- **Zero Warnings Policy**: Zero tolerance for compiler warnings. Every change must pass `./kern check` with 0 warnings.

---

## 3. Storage & Provisioning Architecture: USB RigWire vs. Bootstrap Flash

### 3.1 Provisioning Modes (No Direct SPI Flashing)
External SPI NOR Flash (2 MiB PY25Q16) is connected to the MCU via SPI. We do NOT use external SPI hardware programmers. Instead, the architecture provides two distinct deployment modes:

1. **Mode A: USB Protocol Streaming (RigWire File Transfer - Primary Day-to-Day Mode)**:
   - Files are inserted, listed, streamed, and removed directly into LittleFS directories (`/apps/`, `/pref/`, `/user/`, `/user/splash/`, `/user/jingles/`, `/user/rogers/`) over USB CDC / serial using the RigWire protocol.
   - Developers and users push `.krx` applications, `.rle` splash screens, `.ton` melody files, and `.ch` channel tables directly to flash without reflashing the MCU firmware image.
2. **Mode B: First Provision Flash (Bootstrap Factory Mode)**:
   - When flashing blank or wiped hardware, the firmware can be compiled with provisioning features enabled in `kern.toml`:
     ```toml
     [features.provisioning]
     provision_apps = true    # Embeds core KRX packages into ROM for first boot
     boot_assets = true       # Embeds default splash bitmaps
     boot_jingle = true       # Embeds default boot melody tones
     provision_rogers = true  # Embeds roger beeps into /user/rogers/
     ```
   - On the very first boot, if `/sys/init.done` is absent, the firmware formats LittleFS, creates the directory hierarchy, and writes the default files to external SPI flash.
   - Once initialized, `/sys/init.done` is created.
   - **Crucial Optimization**: After first provisioning, the user or developer re-builds firmware with `provision_apps = false`, `boot_assets = false`, `provision_rogers = false` in `kern.toml`. This immediately frees **~20–25 KiB of Flash** on the MCU for ongoing operation!

### 3.2 ORDS-01 Geometry & Sector Alignment Rules
- **Header Geometry**: 16 bytes.
- **Record Geometries**:
  - Contacts, Zones, Hits, Bands: 32 bytes ($2^5$).
  - Channels, Capabilities: 64 bytes ($2^6$).
  - Calibration: 512 bytes ($2^9$).
- **Offset Calculation**: `16 + (index << N)` (zero division instructions).
- **Sector Boundary Alignment**: Record sizes divide cleanly into LittleFS 4 KiB flash sectors:
  - 64 Channels per sector ($64 	imes 64 = 4096$).
  - 128 Contacts per sector ($128 	imes 32 = 4096$).
  - 256 FM Radio stations per sector ($256 	imes 16 = 4096$).
- **Two-Tier VFO Resilience**: Frequency and tuning changes update in-memory volatile SRAM state instantaneously ($< 1.8$ µs). Cold-boot persistence to `/user/vfo_a.vfo` and `/user/vfo_b.vfo` occurs on idle or power-down.

---

## 4. Flash Bloat Autopsy: Why SettingsApp is 16 KiB and How to Reduce It

### 4.1 Memory Allocation Analysis
Current firmware release size is **103,640 bytes (85.8% of 120,832 B budget)**:
- Vectors: 192 B
- Text (code): 89,284 B
- Rodata (constants/tables): 13,484 B
- Data: 680 B | BSS: 3,428 B | Uninit Arena: 4,852 B

### 4.2 Why `SettingsApp` Consumes ~16 KiB
Symbol inspection reveals `SettingsApp` accounts for ~12 KiB of dedicated code + static tables, and ~16 KiB when including its menu/sheet instantiation overhead:
1. **`format_live_metric` (944 Bytes)**: A massive `match` statement formatting battery status, voltages, currents, temperatures, storage sector statistics, serial numbers, MAC addresses, and calibration values. Each branch does custom string manipulation, push calls, and math.
2. **`build_asset_chooser` (808 Bytes) + Redundant Chooser Builders**: Separate builders (`build_roger_chooser`, `build_splash_chooser`, `build_jingle_chooser`) each setting up duplicate `AssetChooserParams` structs and path buffers.
3. **`apps.rs` (Package Management Subpage, ~1.8 KiB)**: Functions `populate_catalog_list`, `populate_package_detail`, `format_size`, `app_path`, and `push_action` implement package inspection, capability token decoding, and size formatting directly inside the Settings application code.
4. **Triple Field Matching**:
   - `build_item_type` (568 Bytes): Matches every setting field enum to construct menu items.
   - `set_val` (112 Bytes) & `handle_radio_confirmed` (300 Bytes): Matches fields again to update `SysContext`.
   - `apply_page_changes` (376 Bytes): Matches fields again to apply updates.
5. **Static Table Bloat (~3.5 KiB `.rodata`)**: Large static option string arrays (`ALL_CHOICE_OPTIONS`, `PAGES0`, `BUTTONS_SHORTCUTS`, `SHORTCUT_OPTIONS`, `BATTERY_INFO`, etc.) with repetitive string pointers.

### 4.3 Concrete Plan to Reduce SettingsApp to < 6 KiB Without Affecting Behavior
1. **Table-Driven Metric Formatter**:
   - Consolidate `format_live_metric` from 944 bytes down to ~250 bytes by using a compact descriptor table `(metric_id, type_tag, unit_str)` and routing value acquisition through unified integer formatters (`gfx::text::push_int_fixed`).
2. **Unified Asset Chooser Parameterization**:
   - Merge `build_roger_chooser`, `build_splash_chooser`, and `build_jingle_chooser` into a single compact factory taking a const enum tag `AssetKind { Splash, Roger, Jingle }`, eliminating ~600 bytes of duplicated boilerplate.
3. **Decouple Package Details from Settings (`apps.rs`)**:
   - Share package inspection routines directly with `apps/launcher/catalog.rs` rather than duplicating header parsing and size formatting in Settings.
4. **Unified Setting Field Setter**:
   - Replace triple matching with direct struct offset pointers or a concise table `(field_id, config_domain, offset, kind)`, saving ~1.2 KiB of repetitive branch instructions.
5. **Future Architectural Target (Declare 3.0 Forms)**:
   - Convert Settings sub-pages into declarative Declare 3.0 form definitions stored in LittleFS (`/apps/settings/*.dcl`). The firmware `SettingsApp` becomes a pure 2 KiB generic AST renderer!

---

## 5. Execution Runtime Framework: KRX-ABI 2.5.0 / 3.0

### 5.1 Package Model Overview
```
┌─────────────────────────────────────────────────────────────────────────────┐
│                          FOUR PACKAGE EXECUTION TIERS                       │
├─────────────────────┬──────────────────────┬────────────────────────────────┤
│ Class N0 / N1       │ Class P0 / P1        │ Class V0 / V1 (KVM-16)         │
│ Native Machine Code │ Paged Overlay Exec   │ 16-Bit Register Sandbox VM     │
│ In-place execution  │ LRU page cache;      │ 16 registers (r0-r15);         │
│ in 4KB arena; fast. │ shadow return stack; │ instruction gas limit;         │
│ Used for DSP / VFO. │ LR-thunk trampoline. │ zero pointer escape.           │
├─────────────────────┴──────────────────────┴────────────────────────────────┤
│ Class D0 (Declare 3.0)                                                      │
│ Pure Declarative AST State Machine                                         │
│ Safe UI layouts, forms, menus, and rules. < 1 KB payload. Zero code exec.   │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 5.2 Complete C-ABI VTable Specifications
All dynamically loaded packages interact with the microkernel via stable C-ABI function pointer tables:

- **`StorageVTable` (32 Bytes)**:
  ```rust
  #[repr(C)]
  pub struct StorageVTable {
      pub storage_read: unsafe extern "C" fn(path: *const u8, path_len: usize, buf: *mut u8, max_len: usize) -> i32,
      pub storage_write: unsafe extern "C" fn(path: *const u8, path_len: usize, buf: *const u8, len: usize) -> i32,
      pub storage_exists: unsafe extern "C" fn(path: *const u8, path_len: usize) -> bool,
      pub storage_file_size: unsafe extern "C" fn(path: *const u8, path_len: usize) -> i32,
      pub storage_list_dir: unsafe extern "C" fn(path: *const u8, path_len: usize, out_entries: *mut KrxDirEntry, max_entries: usize) -> i32,
      pub storage_remove: unsafe extern "C" fn(path: *const u8, path_len: usize) -> bool,
  }
  ```
- **`UiVTable` (64 Bytes)**:
  ```rust
  #[repr(C)]
  pub struct UiVTable {
      pub ui_show_modal: unsafe extern "C" fn(title: *const u8, title_len: usize, msg: *const u8, msg_len: usize),
      pub ui_close_modal: unsafe extern "C" fn(),
      pub ui_has_modal: unsafe extern "C" fn() -> bool,
      pub ui_show_popup: unsafe extern "C" fn(tag: u8, title: *const u8, title_len: usize, msg: *const u8, msg_len: usize),
      pub ui_show_confirm: unsafe extern "C" fn(tag: u8, title: *const u8, title_len: usize, msg: *const u8, msg_len: usize),
      pub ui_draw_tabbed_frame: unsafe extern "C" fn(active_tab: u8, total_tabs: u8),
      pub ui_draw_scrollbar: unsafe extern "C" fn(x: u8, y: u8, height: u8, total: u16, visible: u16, top: u16),
      pub ui_draw_progress_bar: unsafe extern "C" fn(x: u8, y: u8, width: u8, height: u8, progress_pct: u8),
      pub ui_draw_selection_box: unsafe extern "C" fn(x: u8, y: u8, width: u8, height: u8, is_pressed: bool),
      pub ui_draw_key_badge: unsafe extern "C" fn(x: u8, y: u8, key_char: u8, is_held: bool),
      pub ui_scroll_step: unsafe extern "C" fn(current: u16, max: u16, delta: i8) -> u16,
  }
  ```

### 5.3 Declare 3.0 Engine & Interactive Visual Feedback
- **Windowed Viewport**: 5-row visible viewport over unbounded items with dynamic `gfx::widgets::draw_scrollbar` integration.
- **Navigation History Stack**: `nav_stack: [(u8, u8, u8); 4]` storing `(node_id, cursor_pos, scroll_top)` to ensure back-navigation (`KeyCode::Exit`) restores exact scroll and selection positions.
- **Interactive Press/Hold Effects**:
  - Full selection box transforms into a crisp rounded border (`draw_rect` + `clear_corners`) while holding `KeyCode::Menu`.
  - The pressing border persists until the app or action launches, and snaps back if canceled.
  - Arrow indicator in Launcher provides left-right kinetic animation while selected.

---

## 6. Priority Research Tasks & Deliverables

The research agent must analyze, design, and author technical specifications and implementation plans for:

1. **Mandate 1: Declarative App Conversions (Explorer & Tasks)**:
   - Provide the complete architecture to implement `/apps/explorer.krx` and `/apps/tasks.krx` as dynamic Declare 3.0 packages.
   - Use `storage_list_dir` and `storage_remove` from `StorageVTable` to browse LittleFS without consuming MCU ROM.
2. **Mandate 2: SettingsApp Size Reduction to < 6 KiB**:
   - Implement the table-driven metric and asset chooser refactoring outlined in Section 4.3.
   - Verify that all existing settings behaviors, ranges, and live metrics remain 100% identical.
3. **Mandate 3: Paged Execution (Class P0) Overlay Trampoline**:
   - Formulate the exact linker script, overlay section structure, LR-thunk trampoline, and shadow return stack for running apps larger than the 4 KiB transient arena.
4. **Mandate 4: RigWire Protocol Tooling**:
   - Define the host-side file management toolchain (e.g. `./kern push <file> <path>`) for transferring packages and assets over USB CDC without reflashing firmware.

---

## 7. Developer Toolchain Verification Contracts

All work must pass the standard CLI toolchain:

```bash
# 1. Type and syntax verification (Strict 0 warnings)
PATH="$HOME/.cargo/bin:$PATH" ./kern check

# 2. Compile release firmware and pack bootloader payload
PATH="$HOME/.cargo/bin:$PATH" ./kern build

# 3. Section breakdown and Flash budget inspection
PATH="$HOME/.cargo/bin:$PATH" ./kern size

# 4. Host unit test suite (All tests must pass)
PATH="$HOME/.cargo/bin:$PATH" cargo test --target x86_64-unknown-linux-gnu -p kernel -p gfx -p crypto
```
