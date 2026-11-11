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
