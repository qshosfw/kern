# Kern Architectural Flash Optimization Master Plan
**Target Architecture:** ARM Cortex-M0+ (PY32F071xB) @ 48 MHz  
**Flash Ceiling:** 120,832 bytes (118.0 KB) | **Long-Term Target:** ≤ 80 KB (< 65% utilization)  
**Status:** Living Engineering Blueprint for Multi-Agent Execution  
**Revision:** 1.2.0 (Post-Batch 4 Baseline: 118,528 B Flash, 2,304 B free, 0 warnings)

---

## 1. Executive Summary & Architectural Motivation

Kern is a high-assurance, `#![no_std]`, zero-heap amateur radio microkernel engineered for ARM Cortex-M0+ silicon. While functional completeness has been achieved across radio modulation, RigWire RPC, KernFS storage (ORDS-01), KRX relocatable execution, and graphical compositing, the firmware currently occupies **118,968 bytes of Flash (98.5%)**.

This tight utilization leaves insufficient headroom for installable third-party userland applications, extended amateur radio digital modes, or expanded filesystem partitions.

Crucially, this consumption is **not caused by feature count**. Rather, deep static analysis of the compiled binary (`arm-none-eabi-nm`) reveals systemic architectural overengineering:
1. **Proliferation of Monolithic C-ABI Shims:** 183 separate `extern "C"` syscall functions in `firmware::runtime::syscall` consuming **12,717 bytes**, many inlining full graphical algorithms (`api_draw_line`, `api_fill_rect_dither`, `api_scroll`, `api_ui_draw_menu_item`).
2. **Excessive rodata Pointer Tables in Declarative Apps:** `apps::settings` consumes **7,795 bytes**, driven by dozens of `&[&'static str]` option slice tables where every string pointer and slice descriptor costs 8–16 bytes on 32-bit ARM.
3. **Mega-Function Inlining under ARMv6-M Register Starvation:** Monolithic handlers like `dispatch_wire_record` (4.6 KB) and `dispatch_event` (2.2 KB) inline multi-layer branches, exhausting the 8 low registers (`r0`–`r7`) and generating massive stack frame push/pop sequences.
4. **Duplicated Binary Format Inspection:** Package validation and header parsing are duplicated across `catalog::inspect_package` (556 B), `loader::launch_krx_package` (1,416 B), and `native_runner::load_native_from_storage` (1,508 B).
5. **Procedural Math & Duplicated Widget Layouts:** Widgets duplicate integer-to-string routines, coordinate clamping, and layout math that could be computed via compact table strides.

### Optimization Target Roadmap
| Subsystem | Baseline Size | Architectural Target | Projected Savings |
|---|---|---|---|
| `runtime::syscall` (Native ABI) | 12,717 B | 7,200 B | **-5,517 B** |
| `apps::settings` (Options & Metas) | 7,795 B | 4,200 B | **-3,595 B** |
| `services::wire` (RigWire Facets) | 4,974 B | 3,200 B | **-1,774 B** |
| `orchestrator` & `compositor` | 7,118 B | 4,500 B | **-2,618 B** |
| `runtime::loader` & `catalog` | 3,472 B | 2,100 B | **-1,372 B** |
| `boot` Subsystem (Splash, Recovery) | 5,260 B | 3,600 B | **-1,660 B** |
| `lib/gfx` & Widgets | 10,050 B | 7,500 B | **-2,550 B** |
| **Total Projected Reduction** | **51,386 B** | **32,300 B** | **~19,086 B Flash** |
| **New Firmware Size** | **118,968 B (98.5%)** | **~99,800 B (82.5%)** | **> 21 KB Free** |

---

## 2. Silicon Constraints & ARMv6-M Invariants

All refactorings must adhere to the physical characteristics of the Cortex-M0+ core:
1. **Strict Zero Dynamic Heap Allocation:** Every buffer is bounded with `heapless` or allocated from the 6 KB transient arena (`TRANSIENT_ARENA`).
2. **No Hardware Integer Divider (Signed Division Elimination):** Cortex-M0+ silicon lacks `sdiv`/`udiv`. Division operations (`/` and `%`) lower to `__aeabi_uidiv` or `__aeabi_idivmod`. By systematically eliminating all signed division (`/`) call sites across `menu.rs`, `adc.rs`, `calib.rs`, `vernier.rs`, and replacing `api_math_idivmod` with branchless sign-masking around `__aeabi_uidivmod`, the linker `--gc-sections` completely discards `__aeabi_idiv`, `__aeabi_idivmod`, and `compiler_builtins::int::sdiv::__divsi3` from the binary.
3. **Register Pressure Management (`r0`–`r7`):** ARMv6-M only supports 16-bit Thumb instructions for registers `r0`–`r7`. Instructions referencing `r8`–`r12` require 32-bit encodings or extra `mov` instructions. Functions with >6 active local variables generate severe stack spilling.
4. **Linker Builtin Guard:** Never allow slice assignments on disjoint slices of the same array to lower to `compiler_builtins::mem::memmove` (704 B penalty). Use explicit index loops.
5. **No Cosmetic Type Aliases:** When refactoring, rename cleanly at definition and call sites. Never add `pub type OldName = NewName;` compatibility shims.
6. **The Inlining Myth vs. Reality on Cortex-M0+:**
   - *Rule 1*: Marking single-call-site private functions as `#[inline(never)]` ALWAYS increases binary size due to function prologue/epilogue (`push {r4..r7, lr}`, `pop {r4..r7, pc}`) and argument register setup.
   - *Rule 2*: `#[inline(never)]` ONLY saves space if the function is called from **two or more call sites** where the body size exceeds the call sequence overhead.
7. **Static Slice Indexing vs Dynamic Stream Cursors on ARMv6-M:**
   - Constant-range slice copies on small arrays (`out[0..2].copy_from_slice(&val.to_le_bytes())`) lower directly to 1-cycle `strh`/`str` instructions with zero branch checks.
   - Dynamic cursors (e.g. `WireBuf` with per-byte bounds check and length tracking) add runtime bounds checks, counter arithmetic, and register pressure on ARMv6-M (+392 bytes penalty). Retain direct slice writes for fixed-stride wire and table protocols.
8. **Fat Pointer Tuple Return Penalty on ARMv6-M:**
   - Returning multi-element fat pointer tuples (e.g. `(&'static str, &'static [SettingDef])` = 16 bytes, 4 words `r0`..`r3`) forces register spilling and massive match table branches (loading 4 words per arm).
   - Functions must be decomposed by single responsibility (`page_title` returning `&str` and `page_defs` returning `&[SettingDef]`), allowing the compiler to optimize each branch table into compact 1-word or 2-word returns.

---

## 3. Subsystem Architectural Blueprints

### Phase 1: Syscall Layer De-monomorphization & Thin-Wrapper Refactoring
**Location:** `src/firmware/src/runtime/syscall/`  
**Current Footprint:** 12,717 bytes | **Target:** ~7,200 bytes | **Savings:** ~5,500 bytes

#### Architectural Flaw
`firmware::runtime::syscall` defines 183 separate `extern "C"` functions. Over 30 of these functions (`api_draw_line`, `api_fill_rect_dither`, `api_scroll`, `api_ui_draw_menu_item`, `api_ui_draw_battery`) re-implement or inline complex algorithms directly into the syscall handler rather than dispatching to shared kernel/gfx functions.

#### Implementation Directives
1. **Unified Canvas Extraction:**
   - Every display syscall currently executes `active_canvas_mut()`.
   - Forward all graphics primitives directly to `gfx::draw` or `Canvas` methods with `#[inline(never)]` delegates.
   - Replace `api_ui_draw_battery` (178 B) with a direct call to `gfx::widgets::status_bar::draw_battery_compact(canvas, x, y, pct, charging)`.
   - Replace `api_ui_draw_menu_item` (228 B) with a shared row renderer call.
2. **Consolidated String Marshalling:**
   - Instead of 18 functions individually calling `parse_str(text, len as usize)`, create a unified C-ABI string helper:
     ```rust
     #[inline(always)]
     pub unsafe fn c_str_slice<'a>(ptr: *const u8, len: u16) -> &'a str {
         if ptr.is_null() || len == 0 {
             ""
         } else {
             core::str::from_utf8_unchecked(core::slice::from_raw_parts(ptr, len as usize))
         }
     }
     ```
3. **De-duplicate Math VTable:**
   - `api_math_crc32` and `api_math_crc16` forward directly to `crypto::crc::CrcEngine`. Ensure they are marked `#[inline(never)]` to prevent duplicating the CRC table setup.

---

### Phase 2: Settings Schema Compaction & Decomposed Page Lookups
**Location:** `src/firmware/src/apps/settings/`  
**Current Footprint:** 7,460 bytes | **Target:** ~4,200 bytes | **Savings:** ~3,200 bytes

#### Completed Refactoring (Batch 4)
- **Eliminated `PAGE_METAS` Array:** Deleted the 304-byte static array of 16-byte structs (`PageMeta { title: &str, defs: &[SettingDef] }`).
- **Decomposed Lookups:** Replaced the fat tuple return `get_page() -> (&str, &[SettingDef])` (which forced 4-word return register spilling and 360B of match branch loads) with two independent single-responsibility functions:
  ```rust
  pub fn page_title(page: PageId) -> &'static str { ... }
  pub fn page_defs(page: PageId) -> &'static [SettingDef] { ... }
  ```
  This immediately reduced binary size by **108 bytes Flash** (328 bytes text reduction), with `page_title` compiling to only 28 bytes and `page_defs` to 48 bytes.

#### Remaining Implementation Directives
1. **Packed Null-Delimited Option Strings (`\0` Encoding):**
   - Pack remaining multi-element option lists into single compact string literals (`pub static SHADING_OPTS: &str = "Outline\0Dither\0Solid\0";`).
   - Use `SheetOptions::Packed(str)` to avoid slice-of-pointers relocation tables.

---

### Phase 3: Unified KRX Package Inspection & Section Reader [COMPLETED]
**Location:** `src/kernel/src/proc/krx.rs`, `src/firmware/src/runtime/loader.rs`, `src/firmware/src/apps/launcher/catalog.rs`  
**Status:** Completed via `KrxPreamble::summarize()`. Both `catalog.rs` and `loader.rs` now share the single-pass microkernel parser, avoiding duplicated section iteration loops.

---

### Phase 4: RigWire Facet Dispatch Flattening & Zero-Copy Payloads
**Location:** `src/firmware/src/services/wire.rs`  
**Current Footprint:** 4,974 bytes | **Target:** ~3,500 bytes | **Savings:** ~1,400 bytes

#### Architectural Flaw & Invariant Insight
Initial experiments with a dynamic stream cursor (`WireBuf` with write pointers and per-byte bounds checks) **increased binary size by +392 bytes** on ARMv6-M due to pointer tracking and branch checks. On Cortex-M0+, constant-range slice copies (`out[0..2].copy_from_slice(&val.to_le_bytes())`) lower directly to 1-cycle `strh`/`str` instructions with zero overhead.

#### Implementation Directives
1. **Zero-Copy Payload Borrowing:**
   - In `dispatch_wire_record`, eliminate the redundant 128-byte stack buffer `let mut payload_buf = [0u8; 128];`.
   - Borrow payload slices directly from the transport driver buffer `hw.wire.current_payload(header)` to eliminate stack copying and reduce register exhaustion in `dispatch_wire_record`.
2. **Consolidate Response Status Dispatch:**
   - Factor common status code return envelopes (`send_status(hw, header, status)`) into an `#[inline(never)]` common tail to eliminate duplicate framing sequences.

---

### Phase 5: Modal Compositor & Dialog Layout Unification
**Location:** `src/firmware/src/orchestrator/compositor.rs`, `src/lib/gfx/src/widgets/dialog.rs`  
**Current Footprint:** ~3,700 bytes | **Target:** ~2,300 bytes | **Savings:** ~1,400 bytes

#### Architectural Flaw
`ModalCompositor` (`render_overlay` at 1,492 B and `handle_event` at 1,316 B) inlines full rendering loops for popups, text inputs, sheets, and toasts.

#### Implementation Directives
1. **Unified Modal Chrome TVF (Template View Function):**
   - Extract a single standard function `draw_modal_chrome(canvas, w, h, title, msg)` that renders the centered window, clear box, outline, and title banner.
2. **Shared Button Capsule Row:**
   - Unify `[ OK ]` and `[ CANCEL ]` rendering between `compositor.rs`, `widgets/dialog.rs`, and `api_ui_draw_capsule_button`.

---

### Phase 6: RadioCoordinator De-monomorphization
**Location:** `src/firmware/src/services/radio/coordinator.rs`  
**Current Footprint:** 3,420 bytes (`tick_10ms` alone is 1,876 bytes) | **Target:** ~2,100 bytes | **Savings:** ~1,300 bytes

#### Architectural Flaw
`RadioCoordinator` defines generic methods `<R: RadioService>` for `tick_10ms`, `select_vfo`, `start_tx`, `stop_tx`, `request_ptt_start`, and `request_ptt_stop`. On the UV-K1/UV-K5 architecture, the radio driver is always `drivers::bk4829::Bk4829`. Monomorphizing these complex state machines over a generic trait type parameter adds code bloat and prevents the compiler from optimizing cross-service inlining effectively.

#### Implementation Directives
1. **Static Dispatch Against Concrete Driver Facets:**
   - In `firmware::services::radio::coordinator`, instantiate coordination methods directly against `&mut Bk4829` or use static dispatch through the BSP.
   - Separate large sub-tick state machines (`tick_tx`, `tick_arbitrator`, `tick_rx`) into modular, non-generic helper functions.

---

### Phase 7: Boot Splash & System Recovery De-duplication
**Location:** `src/firmware/src/boot/orchestration.rs`, `src/firmware/src/boot/recovery.rs`  
**Current Footprint:** 7,203 bytes (`run_boot_splash` 1,616 B, `draw_recovery_screen` 984 B) | **Target:** ~4,200 bytes | **Savings:** ~3,000 bytes

#### Architectural Flaw
`run_boot_splash` duplicates frame interpolation math, canvas blitting, and key polling. `draw_recovery_screen` manually formats hex lines, stack markers, and boxes using independent procedural code.

#### Implementation Directives
1. **Consolidated Boot Dialog Renderer:**
   - Share the box, banner, and key-prompt renderer (`render_dialog`) across boot stages, recovery, and format prompts.
2. **Compact Splash Key Polling:**
   - Factor splash animation ticking and key break detection into a unified, tight event poll.

---

### Phase 8: OpenApps / KRX Overlay Runtime Unification (from Falcon / F4HWN)
**Location:** `src/firmware/src/runtime/native_runner.rs`, `src/kernel/src/proc/krx.rs`  
**Current Footprint:** ~18,000 bytes combined | **Target:** ~12,000 bytes | **Savings:** ~6,000 bytes

#### Inspiration from Falcon & F4HWN Architecture
Analysis of Falcon (`inspo/Falcon-firmware-UV-K1-K5v3/`) and F4HWN reveals how community firmwares run dozens of full-featured apps (APRS, CW Keyer, SSTV, AIS, Spectrum) within 118 KB Flash:
1. **Transient Arena as Overlay RAM:**
   - All external applications run inside the 4 KiB sector cache / transient arena (`TRANSIENT_ARENA`).
   - The resident firmware only maintains the kernel, the hardware drivers, and the compositor.
2. **Zero-Copy ABI Table:**
   - Resident services are exposed via clean static function tables (`KrxApiTable` / `app_api_t`), eliminating dynamic dispatch overhead in userland.
3. **De-duplicate Relocation Routines:**
   - Consolidate delta relocation application in `native_runner::apply_package_relocs` to standard pointer-offset arithmetic, ensuring P0 leaf overlays and resident apps share identical execution trampolines.

---

## 4. Execution Protocol & Verification Standard

Successor agents must execute this master plan in structured, atomic batches:
1. **Zero-Warning Policy:** Run `./kern check` before and after every modification. Zero compiler warnings tolerated.
2. **Target Memory Verification:** Run `./kern size` to measure exact byte deltas. Verify that:
   - Flash size decreases monotonically.
   - SRAM utilization remains ≤ 12.3 KB (≤ 75%).
3. **Automated Test Suite:** Run `cargo test --target x86_64-unknown-linux-gnu -p kernel -p kernfs -p utils` to prevent semantic regressions.
4. **Git Staging Rules:**
   - Commit only workspace changes: `git add -u src/ docs/`.
   - **STRICTLY NEVER** stage or commit files in `inspo/*`.
