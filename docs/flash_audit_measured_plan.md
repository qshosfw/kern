# Kern Measured Flash Audit & Refactor Plan (Part II)

> Companion to `architectural_flash_optimization_master_plan.md` (Phases 1–9). This document adds **Phases 10–21**, every item grounded in symbol-level measurements of the release ELF. Agents execute phases in order of the *Priority* column, one atomic commit per phase, following §8 of the master plan (`./kern check` 0 warnings, `./kern size` monotonic decrease, host tests green, never stage `inspo/*`).

## 0. Baseline (2026-11-11, commit `28f6a2184`, 19 features, UV-K1)

| Section | Bytes | Note |
|---|---|---|
| Flash used | 120,652 / 120,832 | **180 B free** — every new feature is currently blocked |
| `.text` | 101,812 | |
| `.rodata` | 18,616 | only **2,599 B** is in named symbols → ~16 KB anonymous (fonts, icons, `include_bytes!` packages, strings, jump tables) |
| SRAM | 12,312 / 16,384 | |

`.text` by crate: `firmware` **64,646** (63 %) · `drivers` 12,730 · `gfx` 9,692 · `kernel` 5,762 · `py32_hal` 2,284 · `kernfs` 1,446 · `bsp` 1,406 · `compiler_builtins` 820 · `crypto` 624 · `utils` 616.

**Core diagnosis:** the microkernel (`kernel`, 5.7 KB) is lean; the bloat is the `firmware` integration crate, where policy, UI, protocol and ABI glue are written as hand-unrolled *code* instead of *data + one interpreter*, and where `opt-level=z` still inlines single-use helpers into giant monoliths that duplicate shared logic.

`firmware` hot spots (bytes): `runtime::syscall` 11,520 · `apps::settings` 7,150 · `services::wire` 4,942 · `orchestrator::compositor` 4,144 · `apps::launcher` 3,302 · `services::radio` 3,286 · `runtime::native_runner` 3,194 · `Orchestrator` 3,036 · `boot::orchestration` 2,468 · `apps::vfo` 2,076 · `main` 2,048 · `run_system_loop` 1,936 · `boot::recovery` 1,344.

### Reproduce the audit
```sh
E=target/thumbv6m-none-eabi/release/firmware
arm-none-eabi-nm -S -C --size-sort $E | tail -60                       # largest symbols
arm-none-eabi-objdump -d -C $E | grep -E 'bl\s' | sed -E 's/.*<(.*)>.*/\1/' | sort | uniq -c | sort -rn | head -30   # call-site fan-in
arm-none-eabi-objdump -d -C $E | awk '/^[0-9a-f]+ <.*>:$/{f=$0} /bl.*__aeabi_uidiv/{print f}' | sort | uniq -c | sort -rn  # who divides
```
Add any new finding to §3 with its measured bytes before implementing it.

---

## 1. Design Laws (derived from the measurements)

1. **Data over code.** A table entry costs 2–4 B; an unrolled call site costs 8–14 B. Any sequence of ≥4 similar calls becomes a `const` table walked by one `#[inline(never)]` interpreter.
2. **One clip/validate site per primitive.** Bounds, null and range checks live in the *callee* (`Canvas`, `ByteWriter`, `RegPatch::apply`), never repeated in each shim/caller.
3. **Shared primitives are `#[inline(never)]`.** Anything with ≥2 callers and >24 B body. `#[inline(always)]` is reserved for ≤3-instruction accessors. Audit with the fan-in command above.
4. **Natural alignment by construction.** On-flash power-of-two records are laid out so every field is naturally aligned; use `#[repr(C)]` + `const _: () = assert!(size_of::<T>() == 2^N)` instead of `#[repr(C, packed)]`. `packed` is only permitted where the external wire format forces misalignment, and then accessed via `u32::from_le_bytes` on a byte slice, never via field references.
5. **No hardware division in firmware.** Cortex-M0+ has none; `__aeabi_uidiv` is called from **68 sites**. Use shifts, reciprocal multiplies, or power-of-two geometry.
6. **No bounds-checked slice indexing in serializers.** Use a cursor (`ByteWriter`/`ByteReader`) whose single out-of-space branch returns `WireStatus::BadLength`.
7. **Resident = mechanism; KRX = policy.** If a feature is not needed to boot, recover, receive/transmit, or install a package, it is a KRX app on SPI flash.

---

## 2. Phases

| Phase | Title | Est. saving | Priority | Risk |
|---|---|---|---|---|
| 10 | Syscall shim collapse & canvas-owned clipping | 3.0–4.0 KB | P0 | Low |
| 11 | RigWire facet descriptor tables + `ByteWriter` | 2.0–2.8 KB | P0 | Med |
| 12 | Aligned record layouts (kill `__aeabi_uread4/uwrite4`) | 0.6–0.9 KB | P1 | Med |
| 13 | Division elimination | 0.4–0.7 KB | P1 | Low |
| 14 | BK4829 register patch tables | 0.8–1.2 KB | P1 | Med |
| 15 | Unified `Sheet` modal model (compositor + dialogs + settings sheets) | 2.5–3.5 KB | P0 | Med |
| 16 | `TextScreen` primitive for splash / recovery / panic | 1.5–2.0 KB | P1 | Low |
| 17 | Event pipeline: filters as table, de-inlined `dispatch_event` | 1.0–1.5 KB | P2 | Med |
| 18 | Rodata attribution & asset relocation to KernFS | 4–8 KB | P0 | Low |
| 19 | Native runner / loader single-pass pipeline | 1.0–1.5 KB | P2 | Med |
| 20 | Settings & launcher policy offload to KRX | 6–9 KB | P1 | High |
| 21 | Shared formatting primitives (`fmt_fixed`) | 0.6–0.9 KB | P1 | Low |

Cumulative realistic target: **−25 to −40 KB** → resident ≈ 80–95 KB without removing any kernel feature. Phases 18 and 20 move bytes to SPI flash rather than deleting functionality.

---

### Phase 10 — Syscall shim collapse & canvas-owned clipping
**Measured:** `runtime::syscall` 11,520 B. `api_draw_line` 356, `api_scroll` 348, `api_fill_rect_dither` 332, `api_ui_draw_menu_item` 228, `api_ui_set_status_bar` 216, `api_draw_pixel` 138. Each shim is a 2-line wrapper, but the gfx callee is *inlined into it*, so the body of `draw_line`, `scroll_*`, `fill_rect_dither` exists twice (resident caller + shim). `active_canvas_mut` is called from 30 sites; `color_from_u8` 14; `parse_str` 14; `clamp_pos_rect` duplicated inline in ~8 shims.

**Directives**
1. Move clipping into `kernel::canvas::Canvas`: add signed-coordinate entry points `fill_rect_i(x: i16, y: i16, w: i16, h: i16, c)`, `draw_rect_i`, `pixel_i`, `hline_i`, `vline_i`, `clear_corners_i`, `fill_rect_dither_i`, each clipping once and delegating to the existing `u8` routine. Delete `clamp_pos_rect` and per-shim range checks.
2. Mark `gfx::draw::draw_line`, `draw_dotted_line_h`, `draw_bitmap`, `Canvas::scroll_vertical/horizontal`, `Canvas::fill_rect_dither`, `gfx::text::draw_str_align`, `str_width` `#[inline(never)]`.
3. Make `color_from_u8`/`font_from_u8` zero-cost: define `Color`/`Font` as `#[repr(u8)]` with values identical to the ABI encoding and validate with a single `if v > MAX { DEFAULT }` (`Color::from_abi`), `#[inline(always)]`.
4. Replace `active_canvas_mut()` function with direct access to the `CANVAS` static through an `#[inline(always)]` accessor (it is a single static; no indirection needed). Verify no `bl active_canvas_mut` remains.
5. Merge `api_draw_str` into `api_draw_str_align` at the vtable level (point both slots at one function with `align` defaulting via ABI register = 0 is *not* safe; instead keep `api_draw_str` as a tail-call `b` — confirm via disasm it is ≤ 6 B).
6. `api_scroll`: split into `scroll_v`/`scroll_h` vtable slots only if the ABI revision allows; otherwise keep but ensure it only contains two tail calls.
7. Gate `syscall/bench.rs` strictly behind a `bench` feature (verify 0 B in default build).

**Acceptance:** every `api_*` display shim ≤ 24 B; `runtime::syscall` ≤ 8 KB; KRX demo apps render identically (run emulator screenshot diff).

### Phase 11 — RigWire facet descriptor tables + `ByteWriter`
**Measured:** `dispatch_wire_record` **4,656 B** (largest function in firmware), plus 10 unaligned-access calls, 6 divisions, and `validate_file_path` 204 B. `read_sys_facet` etc. are fully inlined, each facet hand-serialising with `out[a..b].copy_from_slice(..)` (each a bounds-check + memcpy call: 122 `memcpy` call sites firmware-wide).

**Directives**
1. Add `utils::bytes::{ByteWriter, ByteReader}`: `put(&[u8]) -> Result<(), WireStatus>`, `put_u8/u16/u32` (LE), `zero(n)`, `get_u8/u16/u32`. `put` is `#[inline(never)]`; typed helpers are `#[inline(always)]` wrappers over it. Remove `get_u8/get_u32_le/get_i16_le` local helpers.
2. Replace facet `match` arms with a `const SYS_FACETS: [FacetDesc; N]`:
   ```rust
   #[repr(C)] struct FacetDesc { id: u16, flags: u8 /* AUTH_REQ, WRITABLE */, read: fn(&SystemHardware, u32, &mut ByteWriter) -> Result<(), WireStatus>, write: Option<fn(&mut SystemHardware, &mut ByteReader) -> Result<(), WireStatus>> }
   ```
   One lookup loop + one auth gate replaces N duplicated auth checks.
3. Each service handler (`handle_control_service`, `handle_data_service`, `handle_events_service`) is `#[inline(never)]`; `dispatch_wire_record` becomes ≤ 80 B.
4. Remove the 128-byte `payload_buf` copy: pass `hw.wire.current_payload(header)` by splitting the borrow (`WireDriver::split_payload(&mut self) -> (&[u8], &mut Session)`); saves 128 B stack + memcpy.
5. Path validation: single `kernfs::path::validate(&[u8], allowed_roots: u8)` shared with `runtime::syscall::parse_user_path` (116 B duplicate logic).

**Acceptance:** `services::wire` ≤ 2.4 KB; host-side RigWire integration tests (`tools/` scripts) pass byte-identical responses.

### Phase 12 — Aligned record layouts
**Measured:** 87 calls to `__aeabi_uread4/uwrite4/uread8/uwrite8` (helpers 96 B + ~8 B per call + lost register allocation). Top: `hard_fault_inner` 17, `dispatch_wire_record` 10, `draw_recovery_screen` 9, `load_native_from_storage` 8, `MemoryMapLayout::init_regions` 6, `WireDriver::handle_core_operation` 6.

**Directives**
1. For each `#[repr(C, packed)]` type, reorder fields by descending alignment, insert explicit `_pad`/`reserved` bytes, and switch to `#[repr(C)]` (records are power-of-two, so this is always possible without changing size). Static-assert size and field offsets (`core::mem::offset_of!`).
2. On-flash compatibility: if a field offset changes, bump the table header `version` byte and add a one-shot migration in `boot::mount_or_format` (only for formats already shipped; ORDS-01 files not yet released need no migration).
3. Crash dump / fault frame (`hard_fault_inner`): read the exception stack frame as `&[u32; 8]` (it is 8-byte aligned by hardware), never via packed struct.
4. KRX headers: `KrxPreamble` read into a `#[repr(C, align(4))]` stack buffer, then reinterpreted — no packed field access. `KrxPreamble::summarize` is 1,080 B; re-check after.

**Acceptance:** `grep -c "uread4\|uwrite4"` in disasm call sites ≤ 10; master plan rule §3 "zero-copy packed" is amended to "zero-copy naturally aligned".

### Phase 13 — Division elimination
**Measured:** 68 `__aeabi_uidiv` + 19 `__aeabi_uidivmod` call sites. Top: `telemetry::sample_battery_and_system` 7, `draw_progress_bar` 6, `dispatch_wire_record` 6, `write_cdcss_golay_code` 4, `BatteryDriver::calculate_percentage` 4, `get_rssi_dbm` 3, `format_metric` 3, `get_crockford_serial` 3.

**Directives**
1. `utils::math`: `div10(u32) -> (u32, u32)` via `(x * 0xCCCD) >> 19` for x < 2^16 and the 64-bit variant otherwise; `scale(x, num_q16)` = `(x * num_q16) >> 16` with precomputed Q16 constants.
2. Progress bar: precompute `step_q16 = (width << 16) / max` once per widget configuration (or require max to be power-of-two); per-frame is multiply + shift.
3. Battery: replace percentage division with a 16-entry piecewise-linear table indexed by `(mv - MIN) >> 5`.
4. RSSI dBm: BK4829 RSSI is `raw/2 - 160` → shift.
5. Crockford base32: 5-bit shifts (`>> 5`), no `% 32`.
6. CDCSS Golay: precompute the 23-bit codewords for the 104 standard codes into a `const [u32; 104]` table (416 B) only if it removes >416 B of code; otherwise compute with shifts (Golay is polynomial XOR, never needs division — fix the implementation).

**Acceptance:** ≤ 15 `uidiv` call sites remain (only in genuinely variable-denominator user math).

### Phase 14 — BK4829 register patch tables
**Measured:** `Bk4829::write_reg` called from 51 sites; `set_af` 18; driver total ~3.8 KB (`Bk4829` 2,240 + trait impls 1,540). Top callers: `apply_subaudible_hw` 7, `set_frequency` 5, `set_modulation` 5, tone sequencing 6.

**Directives**
1. `#[repr(C)] pub struct RegPatch { reg: u8, _r: u8, mask: u16, val: u16 }` (6 B, u16-aligned). `fn apply(&mut self, p: &[RegPatch])` performs read-modify-write only when `mask != 0xFFFF`.
2. Modulation profiles (`FM/AM/USB/LSB/FSK/CW`) become `const MOD_FM: &[RegPatch] = &[...]`; `set_modulation` = table select + `apply`. This also realises the master-plan `ModulationProfile` contract as pure data.
3. Init sequence in `Driver::init` → one table.
4. Subaudible (CTCSS/DCS/off) → three tables + one computed register.
5. Keep `set_frequency` computed (PLL math) but share one `write_reg32(lo, hi, value)`.

**Acceptance:** `write_reg` call sites ≤ 15; RX/TX on hardware verified for FM, AM, CTCSS 88.5, DCS 023.

### Phase 15 — Unified `Sheet` modal model
**Measured:** `ModalCompositor::render_overlay` 1,500 B + `handle_event` 1,316 B; `gfx::widgets::dialog` 1,738 B; settings `build_sheet` 644, `handle_sheet_selected` 248, `prompt_config_str` 228, `handle_text_input_done` 182, `handle_action_press` 144; `MenuView::on_event` 592 + `render_menu_row` 808. Four overlapping list/dialog renderers.

**Directives**
1. One model: `struct Sheet<'a> { title: &'a str, kind: SheetKind /* Confirm, Choice, Info, Input, Progress */, rows: RowSource<'a>, sel: u8, top: u8 }` where `RowSource` is either `&'a [Row]` or `fn(u8, &mut RowBuf) -> bool` (lazy rows, no RAM list).
2. Rendering: frame + title via `dialog::draw_frame`; rows via the existing `render_menu_row` (now the *only* row renderer, `#[inline(never)]`).
3. Events: one `Sheet::on_key(Key) -> SheetResult { None, Moved, Picked(u8), Cancel }`; compositor and settings only map `SheetResult` to actions.
4. Remove bespoke confirm/info drawing in `render_overlay`; it becomes `sheet.render(canvas)`.
5. Expose `Sheet` through the UI facet so KRX apps reuse resident code instead of shipping their own (`api_ui_draw_menu_item` collapses into it).

**Acceptance:** compositor ≤ 1.6 KB, dialog widget ≤ 0.8 KB; lifecycle tests (AGENTS.md §5.2) cover open → pick → close → focus restored.

### Phase 16 — `TextScreen` primitive
**Measured:** `run_boot_splash` 1,612 + `boot_orchestration` 708 + `draw_recovery_screen` 924 + `panic::baremetal::render_recovery_frame`/loop ~500 + `hard_fault_inner` 444. All draw full-screen static text with hand-placed `draw_str` calls (32 call sites of `draw_str`, 14 of `draw_str_align`).

**Directives**
1. `#[repr(C)] struct TextLine { y: u8, font: u8, align: u8, text: &'static str }` and `fn draw_text_screen(c: &mut Canvas, lines: &[TextLine], dyn_fn: Option<fn(&mut Canvas)>)`.
2. Recovery, panic and splash become `const` line tables + a small dynamic hook (serial number, fault PC). The panic path must stay allocation-free and not depend on `Sheet` (fault isolation) — `TextScreen` lives in `gfx` and uses only `Canvas` + font.
3. Splash animation keyframes as data (`&[(frame, op, arg)]`), not code (coordinate with the splash-owning agent per AGENTS.md §5.5 before touching `splash.rs`/`orchestration.rs`).

**Acceptance:** combined ≤ 2 KB (from ~4.2 KB); hard-fault screen tested in emulator via injected `udf`.

### Phase 17 — Event pipeline as filter table
**Measured:** `Orchestrator::dispatch_event` 2,052 B, `switch_app_internal` 504, `KeypadLockManager::handle_event` 724, `run_system_loop` 1,936, `main` 2,048.

**Directives**
1. `const FILTERS: [fn(&mut Orchestrator, &mut Event) -> Flow; N]` (keypad lock, PTT, global hotkeys, modal capture, power key) iterated in order; `Flow::{Continue, Consumed}`. Each filter `#[inline(never)]` in its own module, feature-gated in the array via `cfg`.
2. `run_system_loop` → `const TASKS: [(period_ms_shift, fn(&mut Sys, u32))]`; scheduling by `now & mask` (power-of-two periods), no per-task counters.
3. `main`: hardware bring-up as an ordered `const INIT: [fn(&mut Hw) -> Result<(), ()>]` driven by `PeripheralDescriptor` registry; failure index reported to recovery.

**Acceptance:** `dispatch_event` ≤ 400 B; state-preservation tests for VFO↔Launcher↔Settings still pass.

### Phase 18 — Rodata attribution & asset relocation
**Measured:** 16 KB of `.rodata` is anonymous.

**Directives**
1. Attribution first: give every `include_bytes!`, font and icon array a named `static` (not `const`) so `nm` reports it; add `./kern size --rodata` listing the top 30 rodata symbols. Record results in §3 here.
2. Fonts: keep exactly one resident font (status/boot/panic glyphs, ASCII 0x20–0x7E). Larger/alternate fonts → `/sys/font_*.fnt` on KernFS with a 32-glyph SRAM cache in the transient arena; `Font` enum selects resident vs. paged.
3. Provisioning payloads (`provision_apps`, ≈5.5 KB per master plan) → Mode B (Phase 8): delivered via RigWire/`kern flash --assets`, not embedded. Default build ships 0 B of package images.
4. Icons: 1bpp, pack row-major with width ≤ 8 into `u8` per row; dedupe identical icons across launcher/settings (`apps::settings::apps::app_icons` 156 B duplicates launcher icons).
5. Strings: dedupe repeated UI labels into one `static STR_*` pool; disallow `format!`-like long literals in resident code.

**Acceptance:** anonymous rodata ≤ 4 KB.

### Phase 19 — Native runner / loader single pass
**Measured:** `load_native_from_storage` 1,568, `launch_krx_package` 848, `KrxPreamble::summarize` 1,080, `PackageCatalog::inspect_package` 484, `bind_cap_imports` 196 — four parsers walk the same KRX header.

**Directives**
1. One `KrxView::parse(&[u8; HDR]) -> Result<KrxView, KrxError>` in `kernel::proc::krx` producing all fields needed by catalog, loader and capability binder; catalog caches `KrxView` (AGENTS.md §5.3: no re-query on UI transitions).
2. `summarize` (diagnostic text) moves behind `settings_debug` feature or into the KRX "Apps" app.
3. Relocation loop: table-driven by reloc type, no per-type inlined copies.

**Acceptance:** runtime (`native_runner`+`loader`+`krx`) ≤ 2.2 KB.

### Phase 20 — Settings & launcher policy offload
**Measured:** `apps::settings` 7,150 + `apps::launcher` 3,302 + `apps::vfo` 2,076 + `config::MasterConfig` 1,046 (`sanitize` 556).

**Directives**
1. Resident keeps: VFO (core radio UX), a minimal launcher grid (already `GridMenuView`), and the `PrefModule` 2.0 interpreter (Phase 9).
2. Settings pages become `PrefModule` descriptor files (`/sys/prefs/*.pm`) interpreted by one resident page engine; `populate_item` (748 B), `apply_page_changes`, `format_metric` collapse into the engine. Package list/detail (`populate_package_list/detail`, 780 B) → KRX "Apps" app.
3. `MasterConfig::sanitize` → per-field `(min, max, default)` table in `CONFIG_METAS` (already exists, 72 B) and one clamp loop.
4. Requirement: settings must remain reachable when SPI flash is unmounted → resident fallback page with only *Format storage / Reboot / Calibration reset* rendered via `TextScreen`/`Sheet`.

**Acceptance:** `apps::settings` ≤ 2 KB resident; full settings parity via descriptors.

### Phase 21 — Shared formatting primitives
**Measured:** `format_metric` 528, `format_asset_name` 244, `utils::format::format_int_fixed` (callers duplicate), `status_bar::render` 476, telemetry formatting.

**Directives**
1. Single `utils::format::fmt_fixed(buf, value: i32, frac_digits: u8, unit: &str) -> &str` using `div10` (Phase 13); all metric/frequency/voltage/percent printing routes through it.
2. Frequency display uses the same with 5 fractional digits; delete bespoke frequency formatters in `apps::vfo`.

---

## 3. Findings Log (append measured items here)
| Date | Symbol(s) | Bytes | Finding | Phase |
|---|---|---|---|---|
| 2026-11-11 | `kernel::app::AppVTable::from_app::v_*` | 5 thunks × 3 apps | Acceptable; do not de-genericise further | — |
| 2026-11-11 | `RadioCoordinator::tick_10ms::<Bk4829>` | 1,936 | Single instantiation; size is inlined state machine → split per state `#[inline(never)]` (Phase 6) | 6 |
| 2026-11-11 | `py32_hal::gpio::Pin::{set_pull,set_mode,configure_input}` | 2 copies each | Duplicate codegen units (LTO `fat` + `codegen-units=1` check in `Cargo.toml` profile) | 10 |
| 2026-11-11 | `compiler_builtins::mem::memcpy` | 316 | Generic memcpy pulled; most copies ≤ 16 B constant-size → ensure fixed-size arrays copy by value | 11 |

## 4. Theoretical Floor (feature-complete resident)
| Block | Now | Floor |
|---|---|---|
| HAL+BSP+vectors | 3.9 KB | 3.5 KB |
| Drivers | 12.7 KB | 9.0 KB (Phase 14, 12) |
| Kernel + KernFS + crypto | 7.8 KB | 7.5 KB |
| gfx (one font, Sheet, TextScreen) | 9.7 KB + fonts | 7.0 KB |
| Syscall ABI | 11.5 KB | 6.5 KB |
| RigWire | 6.5 KB | 3.5 KB |
| Runtime loader | 4.0 KB | 2.2 KB |
| Orchestrator/system/boot/recovery/panic | 13 KB | 6.5 KB |
| Resident apps (VFO, launcher, pref engine) | 12.5 KB | 6.0 KB |
| Rodata (assets, strings) | 18.6 KB | 5.0 KB |
| **Total** | **118 KB** | **≈ 57 KB** |

---

## 5. KRX ABI & Package Architecture Review (v3.2 target)

Every change below is strictly additive in capability or neutral, and removes code. Source of truth reviewed: `src/firmware/src/runtime/syscall/mod.rs`, `src/kernel/src/proc/{abi,krx,declare,paged,vm}.rs` (8,094 lines in `proc/`), `native_runner.rs`.

### 5.1 Concrete flaws found

| ID | Flaw | Evidence | Consequence |
|---|---|---|---|
| A1 | **Three parallel binding mechanisms** for the same 13 vtables: `NATIVE_API_TABLE` (v3.0 composite), `NATIVE_SYS_TABLE` (v3.1 composite, field-for-field identical), and `INTERFACE_TABLE` + `bind_cap_imports` (GOT by `interface_id`). | `syscall/mod.rs` | 3 ABIs to maintain/test; dead `NATIVE_SYS_TABLE`; ~200 B rodata + docs/spec drift. |
| A2 | **Capability bypass.** `NATIVE_API_TABLE.granted_caps = 0xFFFF_FFFF` and the table is passed to every native app (`native_runner.rs:243, :581`). Shims perform **zero** per-call cap checks (`grep` = 0 in display/radio/audio/storage/io/service). Any app that reads `api->radio` instead of its GOT slot gets TX. | `native_runner.rs`, shims | Violates AGENTS.md capability contract; the GOT binder's gating is decorative. |
| A3 | **Format sniffing instead of declared layout.** `bind_cap_imports` guesses record stride via `len % 8 == 0 && len % 6 != 0` (24 B is ambiguous → mis-parses 3×8 as 4×6). | `mod.rs::bind_cap_imports` | Latent mis-binding; division/modulo on M0+. |
| A4 | **Per-interface `min_abi` is ignored** (`_min_abi` param). Vtables have no length header. | `resolve_capability_vtable` | An app built for a newer vtable calls past the end → HardFault. No forward-compat story. |
| A5 | **UTF-8 validation on every string syscall.** `parse_str` → `core::str::from_utf8` (368 B) on each `draw_str`/`str_width`/path call; fonts are ASCII-only. | `parse_str`, 14 call sites | 368 B + per-call O(n) scan; no safety gain (glyph lookup already clamps). |
| A6 | **User pointers are only null-checked**, never range-checked against the process `MemoryMapLayout`. | `parse_str`, `api_draw_bitmap`, storage shims | App can make the kernel read/write arbitrary SRAM/flash (calibration MAC, PCB table). |
| A7 | **Global mutable context with null-fallback per call**: `ACTIVE_CTX`, `ACTIVE_CANVAS`, `ACTIVE_SWAP` as `static mut` raw pointers; `active_canvas_mut()` is a non-inlined call with a fallback branch (30 call sites). | `mod.rs` | 30×(bl + branch); unsound aliasing; hides lifecycle bugs (drawing when no app active). |
| A8 | **`parse_user_path` duplicates policy** with `services::wire::validate_file_path` and repeats `caps.contains` four times with identical bodies. | `mod.rs`, `wire.rs` | Two security policies that can diverge. |
| A9 | **Header parsed by four independent readers** (`summarize`, `inspect_package`, `launch_krx_package`, `load_native_from_storage`). `krx.rs` is 3,201 lines with ~30 record structs. | Phase 19 | ~3 KB code; inconsistent validation between catalog and loader (TOCTOU between "shown as installable" and "loads"). |
| A10 | **Execution-model sprawl**: `KrxExecType {App, Service, Daemon, Driver, Module}` × profiles × `declare`/`vm`/`paged`/leaf continuation. | `krx.rs`, `paged.rs`, `vm.rs`, `declare.rs` | Each model brings its own loader branch; most are not shipped in default build yet still shape the resident loader. |

### 5.2 Target ABI v3.2 ("one table, bound once, checked once")

1. **Single binding path = GOT import by interface (keep), delete both composite tables.** The entry point receives only `(op: u32, arg: u32)`; vtable pointers come exclusively from GOT slots filled at load. Removes A1/A2 at the root. Cost of an import stays one `ldr` from GOT + `blx` (identical to today's table walk).
2. **Vtable header for versioning:**
   ```rust
   #[repr(C)] pub struct Vt<const N: usize> { pub rev: u16, pub len: u16, pub f: [unsafe extern "C" fn(); N] }
   ```
   Vtables are **append-only**; `len` = number of slots. Binder rejects a mandatory import whose `min_slots > len` (replaces unused `min_abi`, fixes A4). Optional imports bind `NULL`, so new kernels run old apps and old kernels refuse new apps cleanly.
3. **Declared section layout:** CAP section header carries `stride: u8` (6 or 8) and `count: u8`; binder does `for i in 0..count` with `offset = i << stride_shift`. No modulo, no sniffing (A3).
4. **Capability gating at bind time only, with fine-grained facets.** Split vtables where one interface mixes privilege levels: `RadioRx` vs `RadioTx` (TX setters in their own `RadioTxApi` vtable), `StorageRead` vs `StorageWrite`. Then a granted slot *is* the capability — zero per-call checks, zero bypass (A2). `INTERFACE_TABLE` becomes `[(vt, caps_all_required: u32)]` with **AND** semantics (current code uses OR: any one of `RadioRx|RadioTx|BroadBandRx` unlocks the whole radio vtable — a second bypass).
5. **`UserSlice` validation primitive** (A5, A6, A8 unified):
   ```rust
   #[inline(never)] pub fn user_bytes(ptr: *const u8, len: u16) -> Option<&'static [u8]>   // range-checks against active PCB regions
   #[inline(never)] pub fn user_bytes_mut(ptr: *mut u8, len: u16) -> Option<&'static mut [u8]>
   ```
   Text APIs take `&[u8]` (gfx `draw_str` accepts bytes; glyph table already maps out-of-range to `?`). Delete `parse_str`, `str_or_default`, and the `from_utf8` dependency (−368 B, −per-call scan). Path checks: one `kernfs::path::authorize(path: &[u8], caps) -> Option<Root>` used by syscalls and RigWire.
6. **Explicit execution context instead of `static mut` pointers** (A7): the runner owns `struct Exec { ctx: &mut SysContext, canvas: &mut Canvas, pcb: u8 }` placed in one `static` slot set on entry and cleared on exit; shims call `exec()` which is `#[inline(always)]` and *debug-asserts* non-null (no fallback canvas — drawing outside an app frame is a bug). Removes 30 `bl` + branches.
7. **Strings and buffers as `(ptr, len:u16)` pairs in r0–r3 only.** Rule: ≤4 word args per call (AAPCS registers); anything larger is a `#[repr(C)]` param block passed by pointer (validated once with `user_bytes`). Today `api_draw_str_align` takes 7 args → stack spills on both sides.
8. **One header view** (A9): `KrxView::parse(&[u8; 64])` in `kernel::proc::krx` validates once (magic/FourCC, version, section bounds, CRC) and is cached by the catalog keyed by `(inode, mtime)`. Loader re-validates only the CRC of the payload it actually maps. Catalog and loader can no longer disagree.
9. **Collapse execution models to two** (A10): `Native` (relocated Thumb code, single entry `(op,arg)`, exec types become a *flags* byte: `BACKGROUND | SERVICE_EXPORT | DRIVER_CLAIM`) and `Declare` (data-only descriptors interpreted by resident `PrefModule`/rules engine). `vm` and `paged`/leaf become *loader strategies* behind the same `Native` contract (`paged` = overlay fetch on GOT miss), compiled only with their features. `KrxExecType` 5-variant enum → `u8` flags; its `as_str` moves to the host tool.
10. **Host-side work moves to the packer.** Relocation pre-resolution (only `R_ARM_ABS32` against segment base remains), section ordering, CRC, and stride selection are done by `tools/krx-pack`; the device applies one relocation type in a single loop. Spec text lives in `docs/app_format_spec.md` (update in the same commit).

**Estimated effect:** −1.5 to −2.5 KB resident (tables, `from_utf8`, 4 header parsers → 1, path policy dedupe, exec-type branches) on top of Phases 10/19, plus closing three security holes. Requires a coordinated rebuild of `packages/*` (`leaf_app.c`, `paged_app.c`) against the new `kern_abi.h`; bump `KRX_ABI_VERSION` to `0x0003_0002` and have the loader reject < 3.2 with `KrxLaunchError::AbiTooOld` (no compatibility shims, per AGENTS.md "no aliases").

---

## 6. Structural Flaws Beyond Byte Counts (firmware-wide)

These are architecture defects whose fix *naturally* removes bloat; they are not micro-optimisations.

| ID | Flaw | Fix | Est. |
|---|---|---|---|
| S1 | **`firmware` is a god-crate** (63 % of `.text`): ABI, protocol, UI apps, boot, policy, services all in one crate, so nothing is independently feature-gated or testable on host. | Split into `kern-abi` (syscall shims + binder), `kern-wire` (RigWire services), `kern-ui` (compositor, Sheet, TextScreen, resident apps), `kern-boot` (boot, recovery, provisioning). `firmware` keeps only `main` + wiring. Crate boundaries force `pub` APIs → duplicate helpers surface and get deleted. | enables Phases 10–21 |
| S2 | **Two security policies** (syscall path auth vs. RigWire path auth) and **OR-semantics capability masks**. | One `kernfs::path::authorize` + AND masks (§5.2.4–5). | 0.3 KB |
| S3 | **Policy as code**: settings pages, modulation profiles, register sequences, boot steps, event filters, facet serialisers are all `match` trees. | Descriptor tables + one interpreter each (Phases 11, 14, 15, 17, 20). This is the dominant source of bloat. | 8–12 KB |
| S4 | **Packed structs across the codebase** cost unaligned helper calls on every access and forbid references. | Naturally aligned power-of-two layouts (Phase 12); amend master-plan rule. | 0.7 KB |
| S5 | **Assets compiled into ROM** (fonts, icons, provisioning packages): ~16 KB anonymous rodata. | KernFS-resident assets with tiny resident fallback (Phase 18). | 6–10 KB |
| S6 | **Division-based math** contradicts the power-of-two design law (87 call sites). | Phase 13. | 0.5 KB |
| S7 | **Inline policy left to the compiler**: single-use helpers fused into 1.5–4.6 KB monoliths that duplicate shared code. | Explicit `#[inline(never)]` on shared primitives; CI script fails if any function > 1 KB without an allow-list entry. | 2–3 KB |
| S8 | **Diagnostics compiled into release** (`KrxPreamble::summarize` 1 KB, `KrxExecType::as_str`, verbose labels). | Move to host tool / `settings_debug` feature. | 1.2 KB |
| S9 | **Static-mut raw-pointer globals** for execution context (`ACTIVE_*`, `PROC_TABLE`, `NATIVE_SLOT`). | Single `Exec` context + `critical_section`-guarded singletons. | 0.3 KB, soundness |

### 6.1 Guard rails (add to `./kern check`)
- `scripts/flash_guard.sh`: fail if `.text + .rodata` grows vs. `docs/flash_baseline.txt` without the commit message containing `[flash+N]`.
- Fail if any symbol > 1,024 B is not in `docs/flash_allowlist.txt`.
- Fail if `__aeabi_uidiv` call sites > budget, or `core::str::from_utf8` / `core::fmt` appear in the release ELF.

---

## 7. Path to ≤ 80 % Flash Utilisation (≤ 96,665 B used, −23,987 B)

Ordered so each step is an architectural fix, not a patch. Cumulative column uses the low end of each estimate.

| Step | Phase / ID | Low est. | Cumulative used | Util. |
|---|---|---|---|---|
| 0 | Baseline | — | 120,652 | 99.9 % |
| 1 | §5 ABI v3.2 (A1–A10) + Phase 10 shim collapse | −4,500 | 116,152 | 96.1 % |
| 2 | Phase 18 assets → KernFS, provisioning Mode B | −6,000 | 110,152 | 91.2 % |
| 3 | Phase 15 Sheet + Phase 16 TextScreen | −4,000 | 106,152 | 87.9 % |
| 4 | Phase 11 RigWire tables + ByteWriter | −2,000 | 104,152 | 86.2 % |
| 5 | Phase 20 settings descriptor engine (resident part) | −4,000 | 100,152 | 82.9 % |
| 6 | Phases 12, 13, 14 (alignment, division, reg tables) | −1,800 | 98,352 | 81.4 % |
| 7 | Phases 17, 19, 21 + S7/S8 | −2,500 | 95,852 | **79.3 %** |

High-end estimates reach ≈ 85 KB (70 %). No user-visible feature is removed: assets and settings pages move to SPI flash and remain available; a resident fallback (recovery `TextScreen` + minimal settings `Sheet`) guarantees operation with unmounted storage.

---

## 8. Bloat Tooling for Agents — `tools/bloat.py`

Pattern-level analyzer (not a "largest symbols" list). Each subcommand emits a *refactor action*, not just a size. Run after `./kern size`.

| Command | Finds | Action it implies |
|---|---|---|
| `dups [--sim 0.8]` | Exact duplicates after normalising addresses/targets; near-duplicates via 4-gram shingle Jaccard | Merge into one fn / extract shared core |
| `outline [--len 8 --count 3]` | Identical instruction windows repeated across ≥N functions, ranked by `(n-1)·bytes − n·4` | Extract a helper, hoist a guard, or replace with a table |
| `mono` | Generic families grouped by base path with per-instance sizes | De-monomorphise (move type param to struct, `&dyn` at cold boundaries, non-generic inner fn) |
| `calls [--min 400]` | Call-site fan-in × arg-setup cost; large **single-caller** monoliths | Pass a descriptor/struct; split monolith by state/table |
| `consts` | Literal/immediate values reused across many fns | Name once; centralise the logic that uses it (e.g. watchdog feed) |
| `strings` | Duplicate, prefix/suffix-shareable, and longest rodata strings | String pool, error-code space, host-side text |
| `crates --depth N` | `.text`/`.rodata` rollup per module path | Ownership/budget per module |
| `baseline` / `diff` | Per-symbol JSON baseline and delta | Guard against regressions in every commit |

**Agent protocol:** `python3 tools/bloat.py baseline` before a phase → refactor → `./kern build` → `python3 tools/bloat.py diff` and paste the top deltas into the commit body. Planned extensions (implement as needed): `fields` (histogram of struct field offsets > Thumb imm5 range per type, from DWARF with `debug=1` build), `inlined` (with `debug=1`, list DW_TAG_inlined_subroutine counts per callee → shows which shared helpers got duplicated by inlining).

### 8.1 First-run findings (2026-11-11)
| Finding | Evidence (tool) | Fix |
|---|---|---|
| `api_ui_draw_big_digit` ≡ `api_ui_draw_small_digit` | `dups` exact | One fn with `font` arg; both vtable slots point to it |
| `api_draw_rect`/`fill_rect`/`invert_rect` share clip prologue | `dups`/`outline` | Phase 10 (clip in `Canvas`) |
| Every display/radio/io shim re-walks `ACTIVE_CTX → field → Option` with null checks (`ldr [r0,#24]; cmp; beq; ldr [r0,#4]; cmp; beq`) | `outline` ×3+ families | §5.2.6 `Exec` context: resolve once, non-optional |
| Radio getters each re-derive active VFO via byte at offset 218 (`movs r1,#218; ldrb [r0,r1]`) in 15 fns | `outline`, `consts #218` | `active_vfo()` accessor + hot field reorder (§9.1) |
| IWDG reload (`0x40003000`, key `0xAAAA`) inlined in 13 fns | `consts` | `bsp::watchdog::feed()` `#[inline(never)]`; ideally fed from one place (system tick) only |
| Asset chooser closures (`JINGLE_ASSET`, `ROGER_ASSET`, …) are separate `FnOnce` instantiations with identical bodies | `outline` | `AssetDescriptor { cfg_field: ConfigField, dir: &str }` data, one handler |
| `Bk4829` methods: 165 call sites | `calls` | Phase 14 register tables |
| `FileSystemDriver` (60 sites) wraps `KernFS` (40 sites) with per-op `map_err` closure instantiations | `calls`, `mono` | §9.2 collapse storage layers |
| Hex alphabet `0123456789ABCDEF` stored twice; Crockford alphabet separate; default identity `N0CALL…UV-K1` ×2; `KERN-TEAM` ×4; asset paths ×2 | `strings` | One `utils::HEX`, path constants module, single default identity record |
| Three parallel human-readable label sets: launch errors (431 B), capability names (244 B), status codes (157 B) | `strings` | §9.4 unified status code space |
| ~407 field accesses need `movs rX,#imm; ldr [rA,rX]` (offset beyond imm5 range) | disasm grep | §9.1 hot-field-first layout, split god structs |
| Only 5.7 KB of rodata is text → ≈10 KB is fonts/bitmaps/packages | `strings` total vs `.rodata` | §10 resident asset compaction (no SPI dependency) |

---

## 9. Additional Design Problems (deduplication, de-monomorphisation, reuse)

### 9.1 God structs defeat Thumb addressing — `SysContext`, `MasterConfig`, `Orchestrator`
Thumb-1 `ldr/str` immediate offsets reach only 124 B (word), 62 B (half), **31 B (byte)**. Fields beyond cost an extra `movs` (+2 B) and a register on every access; 407 such sites exist.
- **Split by concern**: `SysContext` → `RadioState`, `UiState`, `PowerState` each ≤ 128 B, passed separately (each shim touches one). `MasterConfig` → the already-separate `/pref/*.cfg` structs held as independent statics (matches ORDS-01 domain separation; today they are merged in RAM, undoing the on-disk modularity).
- **Hot-first ordering**: within each struct place `u8` flags/hot bytes in the first 32 B, `u16` next, then `u32`/pointers, then cold arrays last.
- Rule: no struct > 128 B is accessed in hot paths; static-assert sizes.

### 9.2 Redundant storage layering
`SysContext::StoragePrimitive` → `FileSystemDriver<Py25q16>` → `KernFS<Py25q16>` → `Py25q16FlashDriver`, each with its own error enum (`FsError` → `StorageError` → ABI code) and `map_err` closure per method.
- One error type end-to-end: `kernfs::Error` as `#[repr(u8)]`, equal to the ABI status byte → conversions vanish.
- Delete `FileSystemDriver` wrapper; `KernFS` *is* the storage service. `SysContext` holds `&mut KernFS<Flash>`.
- `KernFS` is generic over the flash device but there is exactly one per target; keep the generic on the **struct** only and route all methods through a non-generic `BlockDev` vtable (`read/prog/erase/sync` 4 fn pointers) — this is how LittleFS itself is designed and removes per-method monomorphisation if a second device (internal flash, RAM disk for tests) is ever added.

### 9.3 Generic-on-method instead of generic-on-type
`RadioCoordinator::{tick_10ms, start_tx, stop_tx, request_ptt_*, select_vfo, toggle_vfo, set_audio_muted}::<Bk4829>`: the driver is a method type-param on 8 methods. Move to `RadioCoordinator<R: RadioService>` (or non-generic holding `&mut dyn RadioService` at the 10 ms boundary — a single indirect call per tick costs nothing). Benefit: adding the Si4732 HF receiver later does not double 2.9 KB of coordinator code.
General rule (add to AGENTS.md): *Generic parameters for hardware go on the struct; hot inner loops are non-generic functions taking primitives; cold boundaries (≤ 1 kHz) may use `&dyn`.*

### 9.4 One status/error code space
`KrxLaunchError`, `StorageError`, `FsError`, `WireStatus`, ABI return codes, panic reasons and capability names each have their own enum + `as_str`. Define `kernel::Status` (`#[repr(u8)]`, ranges per subsystem: 0x00 ok, 0x10 storage, 0x20 krx, 0x30 wire, 0x40 caps…). One `Status::label()` table (packed `&[u8]` with offsets, see §10.3); RigWire, ABI and UI all return the same byte. Host tools print full text; device shows short labels.

### 9.5 Two event models
`KrxEvent` (ABI), `kernel::Event` (resident apps), `View::on_event` (gfx) and modal results each define keys/encoder/tick variants and translate between them. Make `KrxEvent` (8-byte `#[repr(C)]`) **the** event type everywhere; resident apps receive the same struct → translation code disappears and resident apps become portable to KRX unchanged.

### 9.6 Resident apps implement `App` via generic `AppVTable::from_app::<T>`
15 thunks (5 per app). Fine today, but the pattern means each new resident app adds 5 thunks + vtable. With §9.5, resident apps can use the same `(op, arg)` single entry as KRX apps: one `fn entry(&mut self, op: u8, ev: &KrxEvent)` per app, no thunks, one dispatch path in the orchestrator for resident and native apps alike.

### 9.7 Two view frameworks
`gfx::gui::View<Model>` (MenuView, PopupView, GridMenuView<T>) and ad-hoc `draw_*` functions in apps/compositor. `GridMenuView` is generic over entry type (`<LauncherEntry>`). Converge on: `ListModel` trait object with `fn row(&self, i: u8, out: &mut RowBuf)` (lazy), non-generic views taking `&dyn ListModel`; the cost of one indirect call per visible row (≤ 6 rows) is negligible.

### 9.8 Boot/provision/recovery each reimplement "ensure file exists with default"
`ensure_default_files`, `provision_packages`, `mount_or_format`, config loaders each open/stat/write. Single `kernfs::ensure(path, default: &[u8] | fn(&mut [u8]))` over a `const DEFAULT_FILES: [(&str, Default)]` table.

### 9.9 Watchdog, delays and busy-waits scattered
IWDG fed in 13 functions; `delay_ms` called from 19 sites in boot/drivers. Feed only in the system tick; drivers expose non-blocking state machines (`poll()`), boot sequence waits via one `wait_until(cond, timeout)` helper.

---

## 10. Keeping Assets and Settings Resident — Architectural Compaction & Silicon Tips

Moving everything to SPI flash is not always acceptable (recovery must work with blank/failed SPI; boot latency). These techniques shrink what stays **in internal flash**.

### 10.1 Fonts
- **One glyph store, many fonts by metadata**: proportional fonts stored as column bitmaps with a per-glyph `(offset:u16, width:u4, yoff:u4)` table; trim blank columns/rows per glyph (typically −25–35 %).
- **Glyph subsetting at build time**: `tools/convert_fonts_modular.py` emits only glyphs referenced by resident strings for large fonts (big digits need `0-9 . - :` only); small font keeps full ASCII.
- **Derive instead of store**: bold = draw twice offset by 1 px; inverted = XOR fill; double-size big digits from the small font with 2× pixel scaling when quality permits.
- **Seven-segment big digits** as 7 rectangle draws (7 B/digit table) instead of bitmaps.

### 10.2 Bitmaps / icons / splash
- Store as **PackBits RLE** (already used for `.rle` splash) for all icons > 32 B; decoder is ~60 B and shared with splash.
- Icon atlas with dedupe (identical icons across launcher/settings/status tray); icons referenced by `u8` index.
- Resident splash = procedurally drawn logo (lines/text via `TextScreen`), bitmap splash optional on SPI.

### 10.3 Strings
- **Single string pool** with `u16` offsets + lengths packed (`[u8]` blob + `[u16]` index); dedupe and suffix-share at build time (`build.rs` generates `strings.rs`). Removes per-`&str` 8-byte fat pointers in tables (a menu of 30 labels: 240 B of pointers → 60 B of `u16`).
- Labels in tables become `StrId(u8|u16)`; `draw_str_id(id)` API.
- Short canonical labels; long explanatory text lives in host tools / KRX help.

### 10.4 Settings kept resident, but as data
Even without SPI, settings can be pure data: `#[repr(C)] struct PrefItem { label: StrId, kind: u8, cfg_off: u8, min: u8, max: u8, choices: StrId }` (8 B/item) + one engine. 60 items ≈ 480 B + ~1.2 KB engine replaces ~7 KB of `apps::settings` code. SPI-hosted `PrefModule` files then only *extend* the resident set (apps' settings).

### 10.5 Pointer-free tables
Every `&'static T` in a const table costs 4 B and a relocation-free but 4-aligned slot. Use `u8`/`u16` indices into one array per kind (strings, icons, handlers). Function-pointer tables stay pointers (4 B, but replace 10+ B per match arm).

### 10.6 Cortex-M0+/PY32 instruction-level rules
| Rule | Why |
|---|---|
| Hot fields in first 32 B of a struct; struct ≤ 128 B | `ldrb/ldrh/ldr` imm5 scaled offsets (31/62/124 B) |
| Group mutable globals into one `#[repr(C)] struct` static accessed via one base pointer | each separate static costs a 4 B literal-pool word per function using it |
| Use `u32`/`usize` for locals and loop counters; `u8` only in storage | avoids `uxtb`/`sxtb` after arithmetic |
| Immediates ≤ 255; prefer enum discriminants 0..N dense | `movs #imm8`; dense matches compile to `__gnu_thumb1_case_*` jump tables instead of compare chains |
| Power-of-two strides and `>>`/`<<` instead of `/`/`%` | no hardware divider; `__aeabi_uidiv` ≈ 40–100 cycles |
| `u32` multiply is single-cycle (PY32F071 M0+ fast multiplier) | reciprocal multiply for scaling is cheap |
| ≤ 4 word arguments; return ≤ 1 word or small `#[repr(C)]` in r0–r1 | AAPCS: 5th arg spills to stack both sides |
| Avoid `u64` arithmetic outside crypto/timekeeping | pulls `__aeabi_lmul`/`__aeabi_uldivmod` |
| `core::fmt`, `from_utf8`, `char` iteration banned in resident code | each pulls 0.3–3 KB; ASCII byte APIs instead |
| Copy fixed small structs by value (`Copy`, ≤ 16 B) | avoids `__aeabi_memcpy` call (122 sites today) |
| Bit-packed flags in one `u32` with const masks | one load + `tst`; replaces scattered `bool` fields |
| Place SRAM-executed hot code (`.ramfunc`) only for flash-wait-sensitive ISR paths | at 72 MHz flash has wait states; copying costs flash once and SRAM |

### 10.7 Toolchain levers (measure each with `bloat.py diff`, keep only wins)
- Per-crate `opt-level` overrides: `[profile.release.package.crypto] opt-level = 3` only if needed for speed; try `"s"` vs `"z"` per crate — `z` occasionally loses on table-heavy code.
- `-Zshare-generics=y`, `-Zfmt-debug=none`, `-Zlocation-detail=none` (no-ops if fmt already absent; enforce they stay absent).
- `-C link-arg=--icf=all` with `lld` (identical code folding) — merges exact duplicates found by `dups` automatically; keep code-level dedupe anyway for clarity.
- `-Cllvm-args=-inline-threshold=` sweep (e.g. 0–25) with `bloat.py diff`; pick the minimum, then pin hot paths with `#[inline]`.
- Check `-Cllvm-args=-enable-machine-outliner` support for Thumb-1 in the current LLVM; if unsupported, `bloat.py outline` is the manual substitute.
- Linker `--gc-sections` + `KEEP` only for vectors and KRX ABI tables; verify no `#[used]` keeps dead diagnostics.

### 10.8 Revised 80 % roadmap without full SPI offload
| Step | Source | Low est. |
|---|---|---|
| ABI v3.2 + shim collapse | §5, Ph 10 | −4.5 KB |
| Data-driven resident settings (no SPI) | §10.4 | −4.5 KB |
| Sheet / TextScreen / unified views & events | Ph 15/16, §9.5–9.7 | −5.0 KB |
| Resident asset compaction (fonts, RLE icons, string pool) | §10.1–10.3, 10.5 | −3.5 KB |
| Storage layer collapse + one Status space | §9.2, §9.4 | −1.5 KB |
| RigWire tables, reg tables, alignment, division, struct split | Ph 11–14, §9.1 | −4.0 KB |
| Provisioning payloads via RigWire (only true offload) | Ph 18.3 | −5.5 KB |
| **Total** | | **≈ −28.5 KB → ~92 KB (76 %)** |

---

## 11. Architectural Refactor Pattern Catalogue (The 9 Golden Patterns)

Both `tools/archlint.py` and `tools/bloat.py` classify codebase design smells into these 9 canonical architectural refactoring patterns. Every developer and AI agent working on Kern must use these patterns rather than ad-hoc code edits.

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                    THE 9 CANONICAL REFACTORING PATTERNS                     │
├────────────────────┬────────────────────────────────────────────────────────┤
│ Pattern Tag        │ Architectural Meaning & Implementation Rule            │
├────────────────────┼────────────────────────────────────────────────────────┤
│ [DATA-NOT-CODE]    │ Replace procedural match trees with const tables.      │
│ [PARAM-BLOCK]      │ Pass a &[repr(C)] descriptor when fn has >4 args.      │
│ [PARALLEL-TYPES]   │ Unify duplicated structs and enums across crates.      │
│ [GLOBAL-STATE]     │ Consolidate static mut into owned execution contexts.  │
│ [APPS-TOUCH-HW]    │ Decouple user apps from HAL/BSP; route via SysContext. │
│ [INLINE-POLICY]    │ Mark non-trivial fns #[inline(never)]; keep callers lean│
│ [HOT-FIELD-FIRST]  │ Place u8/u16 in first 32B of structs (Thumb imm5).     │
│ [ARENA-NOT-STACK]  │ Borrow from TRANSIENT_ARENA instead of [u8; N] on stack│
│ [UNIVERSAL-STUB]   │ Replace 177 distinct noop functions with 1 generic stub│
└────────────────────┴────────────────────────────────────────────────────────┘
```

### Pattern 1: `[DATA-NOT-CODE]` — Table-Driven Interpretation
* **The Flaw:** Writing logic as giant procedural `match` expressions or cascades of `if ... == ...`. Every match arm in ARM Thumb-1 compiles to 4 to 8 bytes of comparison and branch instructions (`cmp`, `bne`, `ldr`, `bl`).
* **The Fix:** Factor the invariants into a compact `#[repr(C)]` struct (2 to 8 bytes per entry) stored in a `const [Descriptor; N]` array in flash, processed by a single `#[inline(never)]` interpreter loop.
* **Top Targets:**
  1. `firmware::services::wire::dispatch_wire_record` (4,656 B) → `const SYS_FACETS: [FacetDesc; N]`
  2. `firmware::apps::settings::metric::format_metric` (528 B, 42 match arms) → `const METRIC_DEFS: [MetricDef; 42]`
  3. `utils::gsm7::decode_extended` & `decode_basic` (345 B, 107 match arms) → `const GSM7_TABLE: [u8; 128]`
  4. `kernel::caps::first_cap` & `next` (190 B, 52 match arms) → `const CAP_BITS: [u8; 32]` or single bit-scan `31 - leading_zeros()`

### Pattern 2: `[PARAM-BLOCK]` — AAPCS Register Parameter Packing
* **The Flaw:** Functions taking 5 or more parameters. Under AAPCS-Thumb, arguments 1 through 4 are passed in registers `r0` through `r3`. Arguments 5+ are pushed onto the stack, requiring `str` before the call and `ldr` in the callee prologue.
* **The Fix:** Group related arguments into a `#[repr(C)]` parameter descriptor passed by immutable reference (`&ParamBlock`), consuming exactly one register (`r0`).
* **Top Targets:**
  1. `gfx::widgets::input::draw_num_keypad_input` (9 arguments)
  2. `kernel::proc::abi::noop_ui_show_confirm_labels` & `syscall::ui::api_ui_show_confirm_labels` (9 arguments)
  3. `gfx::widgets::menu::draw_menu_list` (8 arguments)
  4. `kernel::wire::session::frame_record_raw` (7 arguments)
  5. `syscall::display::api_draw_str_align` (7 arguments)

### Pattern 3: `[PARALLEL-TYPES]` — Type Deduplication & Canonicalization
* **The Flaw:** Defining identical or near-identical structs and enums across different crates because authors wanted "crate autonomy". Each duplicate type brings separate method instantiations, conversions, and match tables.
* **The Fix:** Consolidate onto a single canonical definition in `kernel` or `platform/traits`. For errors, standardize on a single `kernel::Status` (`#[repr(u8)]`).
* **Top Targets:**
  1. `kernfs::device::FsError` (8 variants) ~ `kernel::service::StorageError` (8 variants) → overlap 0.75.
  2. `kernel::proc::abi::KrxApiTable` ~ `KrxSysTable` (identical 13 pointer fields!).
  3. `calib::SquelchThresholds` ~ `kernel::service::radio::squelch::SquelchThresholds` (identical `[u16; 5]`).
  4. `kernfs::fs::FsStats` ~ `kernel::service::StorageStats` (identical 5 fields).
  5. `traits::rtc::RtcTime` ~ `hal::rtc::DateTime` (identical 5 date-time fields).
  6. `firmware::services::melody::presets::JingleDef` ~ `RogerDef` (identical string and tone slice fields).

### Pattern 4: `[GLOBAL-STATE]` — Consolidation of `static mut` Globals
* **The Flaw:** 64 separate `static mut` globals scattered across modules. In Thumb-1, accessing each independent static requires loading its 32-bit absolute address from a local literal pool (`ldr rX, [pc, #offset]`), costing 6 bytes per access site plus register pressure.
* **The Fix:** Group subsystem globals into a single cohesive struct (e.g. `SystemGlobals` or `ExecutionContext`) referenced by a single base pointer, allowing cheap offset-based access (`ldr r0, [rBase, #offset]`).
* **Top Targets:**
  1. `firmware::src::main.rs`: 11 statics
  2. `firmware::src::orchestrator.rs`: 8 statics
  3. `firmware::src::runtime::syscall/mod.rs`: 7 statics (`ACTIVE_CTX`, `ACTIVE_CANVAS`, `ACTIVE_SWAP`, etc.)
  4. `firmware::src::services::wire.rs`: 5 statics

### Pattern 5: `[APPS-TOUCH-HW]` — Microkernel Boundary Decoupling
* **The Flaw:** Userland and resident applications directly invoking silicon HAL or BSP functions (`py32_hal::delay::millis()`, `py32_hal::wdt::Watchdog::feed()`, `bsp::uid::get_crockford_serial()`). This violates AGENTS.md Principle 1 (Strict layered decoupling: apps must never touch hardware registers or silicon peripherals directly).
* **The Fix:** Mediate all peripheral services through `SysContext` or microkernel trait facets. Apps receive monotonic time, battery voltage, and identity via `ctx.now_ms()`, `ctx.battery_mv()`, and `ctx.serial()`.
* **Top Targets:**
  1. `src/firmware/src/apps/vfo.rs:98`: `py32_hal::delay::millis()` → `ctx.now_ms`
  2. `src/firmware/src/apps/launcher.rs:369`: `py32_hal::delay::millis()` → `ctx.now_ms`
  3. `src/firmware/src/apps/settings/security.rs:86`: `py32_hal::wdt::Watchdog::feed()` → remove (tick feeds WDT)
  4. `src/firmware/src/apps/settings/metric.rs:38`: `bsp::uid::get_crockford_serial()` → `ctx.device_serial()`
  5. `src/firmware/src/apps/settings/metric.rs:43`: `bsp::uid::read_mac_address()` → `ctx.mac_address()`

### Pattern 6: `[INLINE-POLICY]` — De-Inlining Shared Primitives
* **The Flaw:** Excessive `#[inline(always)]` on non-trivial functions (> 5 statements). `Canvas::draw_pixel` has 19 statements and a color switch jump table; because it was marked `#[inline(always)]`, its entire 100-byte body was copied into `api_draw_pixel`, `draw_line`, `draw_bitmap`, `fill_rect`, etc., resulting in > 1,200 bytes of duplicated pixel drawing code!
* **The Fix:** Reserve `#[inline(always)]` strictly for 1-to-3 statement accessors. All shared primitives with > 2 callers and > 24 bytes MUST be marked `#[inline(never)]`.
* **Top Targets:**
  1. `kernel::canvas::Canvas::draw_pixel` (19 statements, jump table) → `#[inline(never)]`
  2. `kernel::canvas::Canvas::scroll_plane_vertical` (24 statements) → `#[inline(never)]`
  3. `kernel::canvas::Canvas::scroll_plane_horizontal` (16 statements) → `#[inline(never)]`
  4. `crypto::ascon::round` (23 statements) → `#[inline(never)]`
  5. `bsp::board::bitbang_write_u16` (7 statements) → `#[inline(never)]`

### Pattern 7: `[HOT-FIELD-FIRST]` — Struct Layout Optimization for Thumb imm5
* **The Flaw:** In ARMv6-M (Thumb-1), `ldrb/strb` instructions have an immediate offset limit of 31 bytes (5-bit imm). `ldrh/strh` reach 62 bytes. `ldr/str` reach 124 bytes. Structs with frequently accessed `u8` or `u16` fields placed past offset 31 force the compiler to emit `movs rX, #offset; ldrb rY, [rBase, rX]`, burning 454 extra instructions across 183 functions!
* **The Fix:** Reorder all core structs (`RadioCoordinator`, `ModalCompositor`, `Orchestrator`, `MasterConfig`, `SysContext`) so that:
  - Byte offsets 0..31: All hot `u8` flags, states, and counters.
  - Byte offsets 32..62: All hot `u16` frequencies and dimensions.
  - Byte offsets 64..124: Pointers, `u32` words.
  - Byte offsets 128+: Cold arrays, buffers, and string slices.
* **Top Targets:**
  1. `RadioCoordinator`: 44 offset penalties (`half@188x3`, `byte@184x3`, `half@114x2`)
  2. `run_system_loop`: 27 offset penalties (`byte@69x9`, `byte@72x3`, `word@200x2`)
  3. `ModalCompositor`: 21 offset penalties (`byte@95x3`, `byte@84x3`, `byte@91x2`)
  4. `Orchestrator`: 18 offset penalties (`byte@81x10`, `byte@82x3`)

### Pattern 8: `[ARENA-NOT-STACK]` — Eliminating Large Stack Frames
* **The Flaw:** Allocating large local buffers (`[u8; 128]`, `[u8; 256]`, `[u8; 512]`, `[u8; 1024]`) on the stack. 29 functions allocate >= 96 bytes on the stack, peaking at 1,412 bytes in `run_boot_splash` and 1,308 bytes in `dispatch_wire_record`. Because Cortex-M0+ cannot adjust SP by > 508 bytes in one instruction, it emits cascades of `sub sp, #508; sub sp, #508...` and requires multi-instruction address math to access local variables.
* **The Fix:** Borrow temporary scratch buffers from `firmware::data::arena::TRANSIENT_ARENA` (6 KB statically reserved in `.bss`), which is already designed for zero-alloc staging!
* **Top Targets:**
  1. `run_boot_splash` (1,412 B stack) → borrow splash decode buffer from arena
  2. `dispatch_wire_record` (1,308 B stack) → borrow wire payload and response buffer from arena
  3. `build_sheet` (780 B stack) → borrow sheet row buffer from arena
  4. `PackageCatalog::scan_internal` (772 B stack) → borrow package inspect buffer from arena
  5. `provision_packages` (724 B stack) → borrow write buffer from arena

### Pattern 9: `[UNIVERSAL-STUB]` — Consolidating Dummy VTable Functions
* **The Flaw:** In `kernel/src/proc/abi.rs`, 177 distinct `noop_*` functions are written out by hand (`noop_draw_pixel`, `noop_draw_line`, `noop_rf_tune`, etc.), taking up hundreds of lines of code and cluttering the symbols.
* **The Fix:** In ARM Thumb-1, a universal noop function is simply:
  ```rust
  #[inline(never)]
  pub unsafe extern "C" fn generic_noop() -> u32 { 0 }
  ```
  In assembly, `movs r0, #0; bx lr` (2 instructions, 4 bytes). A single `generic_noop` function can populate EVERY unused function pointer slot across all stub vtables.

---

## 12. Architectural Playbook for AI Agents (The Rule of One)

When weak or specialized AI agents perform architectural refactoring on Kern, they must strictly adhere to the **Rule of One**:

1. **One Subsystem per Turn:** Never refactor multiple crates simultaneously. Choose one phase (e.g. Phase 10: Syscall layer) and complete it atomically.
2. **One Pattern per Commit:** Focus on applying one pattern (e.g. `[ARENA-NOT-STACK]` on `dispatch_wire_record`, or `[HOT-FIELD-FIRST]` on `RadioCoordinator`).
3. **Execution Protocol Checklist:**
   ```
   [ ] Step A: Run baseline audit:
       python3 tools/bloat.py audit --json docs/baseline_before.json
   [ ] Step B: Run architectural smell check:
       python3 tools/archlint.py <check>
   [ ] Step C: Apply refactoring pattern adhering to no_std and zero heap allocation.
   [ ] Step D: Run fast syntax and type check (0 warnings required):
       ./kern check
   [ ] Step E: Verify monotonic flash decrease:
       ./kern size
       python3 tools/bloat.py diff --base docs/baseline_before.json
   [ ] Step F: Run host tests:
       cargo test --target x86_64-unknown-linux-gnu -p kernel -p kernfs -p utils
   [ ] Step G: Atomic Git commit:
       git add src/ tools/ docs/
       git commit -m "refactor(<subsystem>): apply [<PATTERN>] to <target>"
   ```

---

## 13. Comprehensive Inventory of Concrete Design Flaws

| Subsystem | File & Location | Measured Cost | Root Cause Flaw | Architectural Remedy |
|---|---|---|---|---|
| **Wire RPC** | `firmware/src/services/wire.rs:112` | 4,656 B code<br>1,308 B stack | Monolithic match tree; stack-allocated 1KB buffers | `[DATA-NOT-CODE]` `[ARENA-NOT-STACK]` Const `FacetDesc` table + borrow from arena |
| **Radio State** | `firmware/src/services/radio/coordinator.rs:248` | 1,936 B code<br>44 imm5 penalties | Inlined dual-watch, tone stepping, squelch polling; fields at offset 188+ | `[HOT-FIELD-FIRST]` Split states into `#[inline(never)]` handlers; reorder hot fields |
| **Syscall Shims** | `firmware/src/runtime/syscall/display.rs` | 11,520 B code<br>(183 symbols) | Inlining `Canvas::draw_pixel` & `draw_line` into every shim | `[INLINE-POLICY]` Canvas signed coordinate primitives; direct tail calls |
| **Boot Splash** | `firmware/src/boot/orchestration.rs:70` | 1,612 B code<br>1,412 B stack | Procedural animation loop; 1.4KB stack buffer | `[ARENA-NOT-STACK]` `[DATA-NOT-CODE]` Data-driven keyframe table + borrow arena |
| **Catalog Scan** | `firmware/src/apps/launcher/catalog.rs:100` | 772 B stack<br>484 B code | Stack-allocated package read buffers | `[ARENA-NOT-STACK]` Stream inspect directly through shared arena buffer |
| **ABI VTable Stubs**| `kernel/src/proc/abi.rs:612-1450` | 177 stub fns<br>600 lines code | 177 hand-written noop functions | `[UNIVERSAL-STUB]` Single `generic_noop() -> u32` for all unused slots |
| **App Boundaries** | `firmware/src/apps/vfo.rs:98`<br>`launcher.rs:369`<br>`security.rs:86` | 7 violations | Apps calling silicon HAL (`delay::millis`, `Watchdog::feed`) | `[APPS-TOUCH-HW]` Route time & peripheral queries strictly through `SysContext` |
| **Packed Structs** | `kernel/src/proc/memory.rs`<br>`firmware/src/panic/baremetal.rs` | 87 `__aeabi_uread4/uwrite4` calls | `#[repr(C, packed)]` causes unaligned software trap generation | `[HOT-FIELD-FIRST]` Naturally aligned power-of-two `#[repr(C)]` with padding |
| **Software Div** | `telemetry.rs:sample_battery`<br>`BatteryDriver::calculate_percentage` | 87 `__aeabi_uidiv/mod` calls | Division on Cortex-M0+ hardware | Use reciprocal multiply Q16 or power-of-two bit shifts |
