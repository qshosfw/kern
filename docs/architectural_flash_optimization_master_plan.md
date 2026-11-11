# Kern Architectural Flash Optimization Master Plan
**Target Architecture:** ARM Cortex-M0+ (PY32F071xB) @ 48 MHz  
**Flash Ceiling:** 120,832 bytes (118.0 KB) | **Long-Term Target:** ≤ 80 KB (< 65% utilization)  
**Status:** Living Engineering Blueprint for Multi-Agent Execution  
**Revision:** 1.0.0 (Post-Batch 3 Baseline: 118,968 B Flash, 1,864 B free)

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
2. **No Hardware Integer Divider:** Cortex-M0+ lacks `sdiv`/`udiv`. Division operations (`/` and `%`) lower to `__aeabi_uidiv` or `__aeabi_idivmod`. Never divide by runtime variables in hot paths; use power-of-two bit-shifts (`>> N`) or pre-scaled fixed-point multipliers.
3. **Register Pressure Management (`r0`–`r7`):** ARMv6-M only supports 16-bit Thumb instructions for registers `r0`–`r7`. Instructions referencing `r8`–`r12` require 32-bit encodings or extra `mov` instructions. Functions with >6 active local variables generate severe stack spilling.
4. **Linker Builtin Guard:** Never allow slice assignments on disjoint slices of the same array to lower to `compiler_builtins::mem::memmove` (704 B penalty). Use explicit index loops.
5. **No Cosmetic Type Aliases:** When refactoring, rename cleanly at definition and call sites. Never add `pub type OldName = NewName;` compatibility shims.

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

### Phase 2: Settings Schema Compaction & String Packing
**Location:** `src/firmware/src/apps/settings/`  
**Current Footprint:** 7,795 bytes | **Target:** ~4,200 bytes | **Savings:** ~3,600 bytes

#### Architectural Flaw
`Settings` currently defines 24 separate `&[&'static str]` static arrays for menu choices:
```rust
// Current anti-pattern (costs 8 bytes per option + 8 bytes slice metadata):
pub static SHADING_OPTIONS: &[&str] = &["Outline", "Dither", "Solid"];
pub static PTT_MODE_OPTIONS: &[&str] = &["Momentary", "Toggle"];
pub static ANIMATION_OPTIONS: &[&str] = &["Off", "Standard", "Snappy"];
```
This generates over 2,000 bytes of `.rodata` relocation entries and pointer tables!

#### Implementation Directives
1. **Packed Null-Delimited Option Strings (`\0` Encoding):**
   - Pack option lists into single compact string literals:
     ```rust
     // New pattern: exactly N bytes + null terminators. ZERO pointer tables!
     pub static SHADING_OPTS: &str = "Outline\0Dither\0Solid\0";
     pub static PTT_OPTS: &str = "Momentary\0Toggle\0";
     ```
   - Provide an $O(1)$ zero-alloc index lookup helper:
     ```rust
     pub fn get_packed_option(packed: &str, mut index: usize) -> &str {
         let bytes = packed.as_bytes();
         let mut start = 0;
         for (i, &b) in bytes.iter().enumerate() {
             if b == 0 {
                 if index == 0 {
                     return unsafe { core::str::from_utf8_unchecked(&bytes[start..i]) };
                 }
                 index -= 1;
                 start = i + 1;
             }
         }
         ""
     }
     ```
2. **Compact `PAGE_METAS` & Submenu Indices:**
   - Replace the 304-byte `PAGE_METAS` table with a clean `match page_id` function that returns slice references dynamically.

---

### Phase 3: Unified KRX Package Inspection & Section Reader
**Location:** `src/kernel/src/proc/krx.rs`, `src/firmware/src/runtime/loader.rs`, `src/firmware/src/apps/launcher/catalog.rs`  
**Current Footprint:** ~4,300 bytes combined | **Target:** ~2,500 bytes | **Savings:** ~1,800 bytes

#### Architectural Flaw
`PackageCatalog::inspect_package` and `loader::launch_krx_package` independently implement:
- Preamble signature check
- Header length sanity checks
- Section header iteration loop
- `SECTION_IDENTITY` parsing (name string extraction, icon ID)
- `SECTION_TYPE_LIFECYCLE` parsing (execution type, capability bitmask)

#### Implementation Directives
1. **Core Microkernel Inspector Struct:**
   In `kernel::proc::krx`, define a zero-alloc `KrxSummary` struct:
   ```rust
   pub struct KrxSummary<'a> {
       pub title: &'a str,
       pub icon_id: u16,
       pub exec_type: KrxExecType,
       pub app_class: KrxClass,
       pub caps: CapabilitySet,
       pub min_ram: u16,
       pub min_stack: u16,
   }
   ```
2. **Single-Pass Parser:**
   ```rust
   impl KrxPreamble {
       pub fn summarize<'a>(hdr_bytes: &'a [u8]) -> Result<KrxSummary<'a>, KrxLaunchError>;
   }
   ```
3. Refactor both `catalog.rs` and `loader.rs` to call `KrxPreamble::summarize()`.

---

### Phase 4: RigWire Facet Dispatch Flattening & Response Builder
**Location:** `src/firmware/src/services/wire.rs`  
**Current Footprint:** 4,974 bytes | **Target:** ~3,200 bytes | **Savings:** ~1,770 bytes

#### Architectural Flaw
In `services/wire.rs`, every facet handler manually constructs response byte buffers, repeatedly performing:
```rust
resp[0..2].copy_from_slice(&val.to_le_bytes());
resp[2..4].copy_from_slice(&val2.to_le_bytes());
send_resp(hw, header, WireStatus::Ok, &resp[..len]);
```
This pattern repeats over 25 times across `read_sys_facet`, `read_radio_facet`, `read_ui_facet`, `on_transfer_*`, and `on_handle_*`.

#### Implementation Directives
1. **Compact `WireResponseBuilder`:**
   Implement a minimal 16-byte response writer cursor that chains integers into an out-slice without branch overhead:
   ```rust
   pub struct WireBuf<'a> {
       buf: &'a mut [u8],
       len: usize,
   }
   impl<'a> WireBuf<'a> {
       #[inline(always)]
       pub fn put_u8(&mut self, val: u8) { ... }
       #[inline(always)]
       pub fn put_u16_le(&mut self, val: u16) { ... }
       #[inline(always)]
       pub fn put_u32_le(&mut self, val: u32) { ... }
   }
   ```
2. **Zero-Copy Payload Borrowing:**
   Eliminate the 128-byte stack copy `let mut payload_buf = [0u8; 128];` in `dispatch_wire_record`. Borrow the payload directly from the transport driver.

---

### Phase 5: Modal Compositor & Dialog Layout Unification
**Location:** `src/firmware/src/orchestrator/compositor.rs`, `src/lib/gfx/src/widgets/dialog.rs`  
**Current Footprint:** ~3,700 bytes | **Target:** ~2,300 bytes | **Savings:** ~1,400 bytes

#### Architectural Flaw
`ModalCompositor` has separate procedural rendering branches for Confirm dialogs, Popups, Text Prompts, and Toasts. Each manually calculates box bounds, clears background dither patterns, draws outline rects, and centers button text.

#### Implementation Directives
1. **Unified Modal Chrome TVF (Template View Function):**
   Extract a single standard function `draw_modal_chrome(canvas, w, h, title, msg)` that renders the centered window, clear box, outline, and title banner.
2. **Shared Button Capsule Row:**
   Unify `[ OK ]` and `[ CANCEL ]` rendering between `compositor.rs` and `widgets/dialog.rs`.

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
