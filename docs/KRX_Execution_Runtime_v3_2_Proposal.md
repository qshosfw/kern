# KRX Execution Runtime v3.2 — Research-Based Proposal

**Status:** Architectural proposal, not an ABI freeze; v3.2 adopts single-artifact KRX packaging  
**Target:** PY32F071-class Cortex-M0+, 16 KiB SRAM, external SPI NOR  
**Purpose:** Replace the undifferentiated Native/Paged/VM/Declare plan with implementable profiles, explicit trust boundaries, and staged verification.

---

## 1. Executive decision

The current architecture combines five goals that should be independent:

1. **Dynamic discovery and loading** of packages.
2. **Placement independence** of native machine code.
3. **Execution of code larger than RAM.**
4. **Concurrent/background activity.**
5. **Execution of untrusted third-party code.**

They do not require the same mechanism:

- Dynamic loading can work with a **fixed application VMA**.
- Large applications can use **structured overlays** rather than transparent demand paging.
- Background work can use **run-to-completion event handlers**, not native thread contexts.
- Untrusted code should use **VM/graph execution**, not Native or Paged machine code.
- Native PIC and transparent paging can remain optional later profiles rather than foundational dependencies.

### Recommended execution profiles

| Profile | Name | Trust | Production role | Recommendation |
|---|---|---|---|---|
| **N0** | Native-Fixed | Trusted native | Small, fully resident foreground applications and hot DSP | **Implement first** |
| **P0** | Structured Paged | Trusted native | Large applications partitioned into a resident root and leaf overlays | **Implement second** |
| **V0** | Resident KVM-16 | Software sandbox | Portable tools, automation, calculators, community logic | **Implement with a small resident bytecode limit** |
| **D0** | Declare Graph | Software sandbox | UI, data browsers, rules, schedules, templates, simple services | **Make the most common package class** |
| **F0** | Declare Flow / Stream Graph | Software sandbox around trusted operators | Safe audio/RSSI/packet dataflow | **Prototype after D0** |
| **N1** | Native-PIC | Trusted native | Arbitrary placement or multiple native slots | **Defer until a pinned GCC/LLVM toolchain passes conformance** |
| **P1** | Transparent Paged | Trusted native | General cross-page native calls | **Experimental only; not required for the first large apps** |
| **V1** | Streaming KVM-16 | Software sandbox | Large bytecode images | **Add only after V0 and code-cache measurements** |

The key simplification is:

> **N0 and P0 use fixed link addresses tied to a `map_id`. Portable packages use separate V0/D0/F0 artifacts. Every `.krx` file contains exactly one execution profile, target/map, and payload; host build/catalog tooling selects the correct artifact before transfer.**

---

## 2. What the research changes

### 2.1 Position independence is useful, but not a prerequisite for dynamic packages

Tock’s dynamic loader is a valuable precedent: GCC PIC, a GOT base register, and relocation records can support dynamically placed applications. The same account also documents that LLVM did not support the particular dynamic base-register PIC strategy used by its C applications. This is historical evidence, not proof that present-day LLVM cannot work; it demonstrates why the exact compiler and output must be validated rather than assumed. [1](https://www.tockos.org/blog/2016/dynamic-loading/)

Rust exposes `pic`, `ropi`, `rwpi`, and `ropi-rwpi` relocation models, but the existence of a code-generation switch does not by itself prove a complete KRX launch contract, runtime startup, linker script, and relocation extractor. [3](https://doc.rust-lang.org/rustc/codegen-options/index.html)

Arm’s AAPCS explicitly makes `r9` platform-defined and permits its use as a static base in an RWPI environment. That is compatible with a future N1 profile, but KRX must document the register’s role and preserve it at every boundary when that profile is active. [1](https://github.com/ARM-software/abi-aa/blob/main/aapcs32/aapcs32.rst)

**Conclusion:** use ordinary fixed-address Rust and C for N0. Do not make the first dynamic application runtime depend on a custom PIC toolchain.

### 2.2 Transparent overlays are possible, but their difficult cases are real

GNU `ld` supports overlay regions whose members share an execution VMA while having different load addresses. Its `NOCROSSREFS` checks can reject accidental references between overlays. [1](https://ftp.gnu.org/old-gnu/Manuals/ld-2.9.1/html_node/ld_22.html) [1](https://sourceware.org/binutils/docs/ld/Miscellaneous-Commands.html)

Arm Compiler can generate overlay veneers, preserve registers, load targets, and reload a caller on return. However, its documented model still has subtle indirect-call/callback cases in which the overlay manager cannot insert the required transition. [1](https://support.arm.com/documentation/dui0773/l/Overlays/Automatic-overlay-support/Overlay-veneer) [3](https://smtp.keil.com/support/man/docs/armclang_intro/armclang_intro_pge1466079575757.htm)

**Conclusion:** the problem is not merely writing an LR thunk. A production transparent pager needs a complete control-flow and callback contract. P0 avoids the hardest cases by permitting synchronous calls only from a permanently resident root into a leaf overlay.

### 2.3 Native process isolation normally relies on an MPU

Tock uses hardware memory protection to constrain process addresses and system calls to access hardware. Its native process model is not a precedent for strong isolation on a no-MPU target. [1](https://book.tockos.org/doc/processes)

Software fault isolation for native binaries is possible in research and systems such as Native Client/eWASM, but it requires a constrained instruction stream, validation, control-flow integrity, protected return state, and memory-access instrumentation. It is not a small add-on to an arbitrary Thumb-1 binary. [2](https://research.google.com/pubs/archive/34913.pdf) [1](http://www.runyupan.com/wp-content/uploads/2024/08/4-peach20ewasm.pdf)

**Conclusion:** N0/P0/N1/P1 are trusted. Do not attempt a home-grown arbitrary-Thumb sandbox as a near-term work item.

### 2.4 A small verified VM is a better fit than general WebAssembly here

Wasm3’s own stated minimum useful requirements are approximately 64 KiB of code and 10 KiB RAM, before KRX’s other subsystems. That is a poor fit for a 75 KiB total firmware target and 16 KiB total SRAM. [2](https://github.com/matovitch/wasm3)

RIOT’s rBPF/femto-container work demonstrates that a much smaller VM model is feasible: reported figures are roughly 4.4 KiB ROM and 0.6 KiB RAM for the rBPF interpreter in their configuration, with later pre-flight checks and small per-instance state. Those figures are platform- and configuration-specific, but the architectural lessons transfer: explicit memory regions, pre-flight validation, bounded execution, and helper-only host effects. [1](https://arxiv.org/pdf/2210.03432) [5](https://arxiv.org/html/2106.12553v2)

**Conclusion:** retain a small KRX VM, but borrow the rBPF security model rather than adopting full WebAssembly or full eBPF.

### 2.5 Stable API versioning is as important as relocation

Flipper’s FAP loader checks integrity and API compatibility, loads code, processes relocations, and resolves imports against an exported symbol set. Its build tooling treats API major/minor compatibility as a first-class part of the package ecosystem. [1](https://developer.flipper.net/flipperzero/doxygen/apps_on_sd_card.html)

**Conclusion:** KRX should have per-interface ABI levels, a generated export registry, and deterministic compatibility failures. A package format without this discipline will remain fragile even with perfect relocation.

---

## 3. Unified lifecycle before any execution-class work

All classes should implement the same actor lifecycle. The kernel, not the package, owns the main event loop.

```text
validate selected single-artifact package
    -> verify its declared profile/platform/map
    -> admit resources and capabilities
    -> initialize actor
    -> on_start
    -> on_event / render / service_call, each run-to-completion
    -> optional on_checkpoint at a quiescent point
    -> on_stop
    -> revoke leases and release resources
```

A useful C-ABI conceptual shape is:

```c
struct KrxActorV1 {
    uint32_t struct_size;
    KrxStatus (*on_start)(KrxContext *, KrxActorState *);
    KrxResult (*on_event)(KrxContext *, KrxActorState *, const KrxEvent *);
    KrxResult (*render)(KrxContext *, KrxActorState *, KrxCanvas *);
    KrxStatus (*on_checkpoint)(KrxContext *, KrxActorState *, KrxStateWriter *);
    void     (*on_stop)(KrxContext *, KrxActorState *, KrxStopReason);
};
```

The exact representation can differ by class; the semantics should not.

### Scheduling rules

- Native/Paged handlers return to the kernel; they do not own an infinite `main()` loop.
- Native background actors do not receive arbitrary preemptive stacks in v1.
- Each handler declares a measured time budget; real-time handlers use a stricter budget.
- No generic `sys_delay_ms()` in actor code. Timers are events.
- DMA/audio/RF symbol timing remains in kernel ISR code.
- KVM and Declare can suspend at interpreter/walker boundaries with small explicit state.
- A native actor needing persistence performs a clean checkpoint and unload. Do not freeze an arbitrary native stack, GOT, DMA pointer set, or overlay residency state and call that a complete hibernation image.

This removes the need for a universal 64-byte `TaskContext` and prevents cooperative fibers from turning into an accidental preemptive-thread design.

---

## 4. Native plan

### 4.1 N0 — Native-Fixed, recommended production baseline

N0 packages are linked for a fixed execution map identified by `platform_id`, `map_id`, and `native_abi`. A single active native arena means arbitrary placement provides little immediate value.

#### N0 properties

- One resident native actor at a time.
- Fixed `.text`, `.rodata`, `.data`, `.bss`, and stack addresses.
- No runtime text relocation.
- No `r9` PIC requirement; ordinary AAPCS applies.
- Rust and C use their normal static `thumbv6m-none-eabi`/ARM embedded builds.
- Kernel API calls go through an API table passed at entry, not absolute firmware addresses.
- Large strings, bitmaps, audio, and tables remain package assets and are streamed or read on demand.
- No app-owned IRQ vectors and no app code in interrupt context.
- Trusted install/execution policy.

#### Illustrative N0 arena budget

This is a profile to validate, not a universal constant:

| Region | Example budget |
|---|---:|
| `.text` + resident `.rodata` | 3.25 KiB |
| Import/API metadata | 256 B |
| `.data` + `.bss` + persistent state | 512 B |
| Stack | 768 B |
| Guards, alignment, runtime metadata | 256 B |
| **Total** | **5.0 KiB** |

The linker must assert that the complete resident footprint fits. “5 KiB app” must never ambiguously mean 5 KiB of code plus unrelated stack and data.

#### N0 launch sequence

1. Validate container, profile, `map_id`, ABI, lengths, and CRC/authenticity policy.
2. Check all segment VMAs against the selected map; reject any unexpected segment or relocation type.
3. Stop the previous native actor and revoke its resource leases.
4. Copy loadable segments to their fixed addresses.
5. Zero BSS and paint stack guards/high-water patterns.
6. Bind a per-actor API table with granted services or denial stubs.
7. Set SP to an 8-byte-aligned stack top and enter through a kernel trampoline.
8. Execute `DSB; ISB` before entering newly written executable SRAM. Arm recommends this sequence after modifying program memory. [1](https://documentation-service.arm.com/static/5efefb97dbdee951c1cd5aaf)
9. Call `on_start`; dispatch later events through the actor vtable.
10. On return/stop, force release of radio/audio/DMA leases and clear executable regions as policy requires.

#### Why N0 is better than starting with PIC

- Smaller loader and fewer relocation edge cases.
- Stable Rust support is much easier to demonstrate.
- No GOT tax on every global access.
- Absolute function pointers and vtables work naturally.
- Debugging and map-file inspection are simpler.
- A wrong `map_id` fails closed instead of producing subtle corruption.

### 4.2 Separate target artifacts replace most of the need for N1

Host tooling may produce several independent artifacts from one source project:

```text
dist/map-a/aprs_rx.app      N0, map=MapA, one payload
dist/map-b/aprs_rx.app      N0, map=MapB, one payload
dist/portable/aprs_rx.app   V0, portable bytecode, one payload
```

These are alternatives on the host, not sections of a fat on-device package. The catalog or provisioning tool selects exactly one artifact and transfers it under the canonical package basename so configuration identity remains stable. The device never unpacks a suite, selects among embedded binaries, or transforms a large package into a smaller one.

Each `.krx` therefore retains one execution class/profile, one target/map, one entry point, one payload, and one trust meaning. A wrong artifact is rejected by its existing `platform_id`/`map_id`/ABI checks; there is no silent fallback to another embedded variant.

Duplicating private assets between host build outputs is acceptable because only the selected artifact is transferred. Shared large data should be a separate small KRX Data/content provider when independent reuse justifies it, not a fat application bundle.

### 4.3 N1 — Native-PIC, deferred profile

N1 becomes interesting only if at least one of these is true:

- More than one native actor must remain resident.
- The native arena must move without shipping separate map-specific artifacts.
- Install-time arbitrary slot allocation is a demonstrated requirement.

Before N1 is accepted:

- Pin the exact GCC/LLVM/Rust versions.
- Disassemble global accesses, static references, vtables, switch statements, and helper calls.
- Verify the `r9` static-base contract across APIs, callbacks, exceptions, and overlay calls.
- Extract relocations from ELF semantics; do not guess by scanning data words that happen to look like addresses.
- Test load at two different bases with function pointers in `.data` and `.rodata`.
- Verify Rust `core`, compiler builtins, and startup objects are all built with the required model.
- Measure the GOT/RAM/code-size cost against N0.

N1 remains trusted native code even after these tests pass.

---

## 5. Paged plan

### 5.1 P0 — Structured Paged, recommended large-app model

P0 uses a **resident root + leaf overlays** topology.

```text
                 permanently resident
              ┌────────────────────────┐
              │ actor state machine    │
              │ persistent data/BSS    │
              │ common helpers         │
              │ overlay call dispatcher│
              └───────────┬────────────┘
                          │ synchronous call
                          ▼
              ┌────────────────────────┐
              │ one fixed overlay VMA  │
              │ leaf page A or B or C  │
              │ runs to return         │
              └────────────────────────┘
```

The root remains resident while a leaf executes. Therefore, a normal root-to-leaf `BL/BLX` return address remains valid. There is no transparent caller eviction and no shadow return stack.

#### P0 rules

1. Only the resident root directly calls an overlay.
2. A leaf may call itself, common root functions, and kernel APIs.
3. A leaf never directly calls another leaf.
4. If a leaf needs another leaf, it returns a typed continuation/request to the root.
5. Root callbacks invoked by a leaf must not reenter the overlay dispatcher. Reentry returns a defined error in debug and production builds.
6. Persistent application data/BSS is never overlaid.
7. Page-local `.rodata` may be loaded with the page, but pointers to it must not escape the call.
8. Cross-overlay references are build errors; use linker `NOCROSSREFS` plus packer validation.
9. A leaf runs to return. There is no suspended native stack inside an evicted page.
10. Hard real-time code is root-resident or preloaded and locked for the real-time interval.

This model supports normal synchronous root-to-leaf calls while eliminating LR rewriting, caller shadow stacks, and arbitrary cross-page function pointers.

#### Continuation example

```text
root: AX25_RX event
    -> call LEAF_HDLC_DECODE
    <- NeedTxResponse { retry = 0 }
root: schedule TX
    -> call LEAF_AX25_TX
    <- Done
```

The state machine and retry count live in the resident root, not on an evicted stack.

#### Good page boundaries

Partition by working phase, not randomly by function address:

- `HDLC_RX`, `HDLC_TX`
- `AX25_PARSE`, `AX25_ENCODE`
- `APRS_FORMAT`, `APRS_UI`
- `CONFIG_EDITOR`, `DIAGNOSTICS`
- `TABLE_IMPORT`, `TABLE_EXPORT`

Common arithmetic, CRC, ring buffers, and frequently called helpers belong in the root or kernel.

### 5.2 P0 cache and prefetch rules

A single execution window cannot be asynchronously overwritten while leaf code is executing from it. Therefore:

- Prefetch is allowed **only while the root is active and no leaf uses the window**.
- The root may start a DMA prefetch, perform root/UI work, and later call the leaf after completion.
- A real-time session must finish page-in before entering the timing-critical interval.
- If a required page cannot be loaded before the deadline, admission fails or the operation is deferred; it must not silently miss a radio deadline.

At 24 MHz single-SPI wire speed, approximate payload-only minima are:

| Transfer | Wire-time minimum |
|---|---:|
| 256 B | 85.3 µs |
| 512 B | 170.7 µs |
| 1024 B | 341.3 µs |

Command/address bytes, dummy cycles, DMA setup, arbitration, completion, and barriers are additional.

Start with **512 B and 1024 B prototypes**. Smaller pages reduce miss latency but increase metadata and call/fault counts. Select using traces, not intuition.

### 5.3 Dual-window paging is not automatically trivial

Two windows help only if the page addressing model supports them:

- Pages linked for one fixed VMA cannot simply run in either of two different windows.
- Static assignment to W0/W1 restricts scheduling.
- Supporting both VMAs would require separately built target artifacts for each page layout, reducing portability and increasing the host test matrix.
- Making pages position-independent reintroduces the N1 toolchain/relocation problem.

For P0, one window plus root-time prefetch is the clean baseline. Add dual windows only after a real trace demonstrates that their complexity pays off.

### 5.4 P1 — Transparent Paged, experimental only

P1 should be attempted only if P0’s restrictions prevent an important application. A complete P1 design requires:

- Fixed-window VMA semantics.
- Stable direct-call veneers and stable indirect-call descriptors.
- A bounded shadow return stack with block-relative return offsets.
- Nested-call and recursion rules.
- Exact register-save/restore behavior at every fault entry.
- Page-local data lifetime rules.
- No execution from an evicted page in IRQ context.
- DMA completion plus `DSB; ISB` before entry/resume.
- Callback/reentry analysis, including root functions called by leaves.
- Thrash reporting and a maximum permitted miss latency per actor.
- Golden tests for nested calls, indirect calls, callbacks, and returns into evicted pages.

Given Arm’s own documented overlay corner cases, P1 should not be the default promise to application developers. [3](https://smtp.keil.com/support/man/docs/armclang_intro/armclang_intro_pge1466079575757.htm)

### 5.5 Paged application scope

Good initial P0 targets:

- Large reference tools and database-driven applications.
- Import/export and configuration utilities.
- Packet analysis and offline protocol tools.
- Phase-oriented AX.25/APRS helpers after measurement.

Do not use “paging works” as evidence that DMR/M17 voice is feasible. M17 additionally requires suitable baseband hardware, 4FSK processing, framing/FEC, and Codec2-class voice processing; OpenRTX material explicitly discusses hardware and processing constraints. [3](https://www.darc.de/fileadmin/filemounts/distrikte/p/Distrikt_P/Dokumente/IU2KWO_M17_OpenRTX.pdf)

---

## 6. KVM-16 plan

### 6.1 V0 — resident bytecode first

Streaming two-byte instructions directly from SPI NOR on every dispatch would dominate execution time. V0 should therefore load a bounded bytecode image into SRAM:

- Suggested initial bytecode budget: **1–2 KiB**, selected by arena measurements.
- Bounded virtual memory and call-frame state.
- Large data/tables/strings remain external and use checked cursor APIs.
- V1 later adds a sequential prefetch/code-window cache for larger programs.

### 6.2 Adopt explicit memory regions

Borrow the strongest rBPF idea: memory is not one ambiguous “heap.”

```text
Region 0: immutable bytecode, read only
Region 1: virtual RAM, read/write, fixed length
Region 2: persistent state window, checked typed access
Region 3: packet/input buffer, read only or constrained write
Region 4: output buffer, write only or read/write
```

Every load/store carries a region or is statically associated with one. The runtime checks offset, width, and permission. Native pointers never appear in bytecode.

### 6.3 Verification and execution limits

V0 load-time validation should check:

- Known opcodes and instruction lengths.
- Branch targets on instruction boundaries and inside the image.
- No dynamic jumps.
- Call depth and no unintended recursion.
- Stack/frame limits.
- Literal-pool and string-pool bounds.
- Region permissions and static offsets where available.
- Helper IDs and per-helper ABI levels.
- Bounded structured loops, or initially only forward branches plus bounded host iterators.

Runtime still enforces memory bounds and fuel as defense in depth.

Use separate limits for:

- Instructions per scheduler slice.
- Instructions per event.
- Wall-time per helper call.
- Events emitted per rule/evaluation.
- Persistent writes per time window.

Fuel guarantees bounded dispatch slices, not mathematical program termination.

### 6.4 Register versus stack VM: decide by a bake-off

Research comparing register and stack VMs found that register code can execute fewer VM instructions while using larger bytecode; the exact tradeoff is workload- and encoding-dependent. [1](https://dl.acm.org/doi/10.1145/1328195.1328197)

Prototype the same small benchmark suite in both forms:

- Integer calculator.
- Repeater-table filter.
- Checksum/CRC loop.
- AX.25 field formatter.
- Menu/rule evaluation.
- Small fixed-point correlator.

Compare interpreter flash size, bytecode size, per-instance RAM, cycles/operation, and compiler complexity. Do not claim universal 3–4× density before this test.

A reasonable register-VM direction is 32-bit virtual registers, 16-bit base instructions, a checked literal pool, and no separate expression operand stack. If density wins matter more, a stack or hybrid encoding may be better.

### 6.5 Reuse the expression engine

`Declare Expr16` and KVM arithmetic should share one opcode family and one audited implementation where practical:

- Declare uses acyclic expression subsets.
- KVM adds control flow, calls, and memory regions.
- Helpers have class-specific capability tables.

This reduces firmware size and gives both classes the same conformance tests.

---

## 7. Declare plan and the new Flow idea

### 7.1 D0 — make Declare the default ecosystem class

Most of the features in the vision do not need native code:

- Frequency/repeater guides.
- Forms and settings editors.
- Cheat sheets and manuals.
- Schedules and beacon rules.
- Packet templates.
- Table queries and computed columns.
- Notifications and service adapters.

D0 should support bounded rules, schedules, typed state, tables, templates, expressions, and service exports.

Add explicit anti-storm rules:

- Maximum actions per rule.
- Maximum chained rules per incoming event.
- Maximum queued events per actor.
- Maximum graph transitions per input event.
- Rate-limited persistent writes.
- Validated cycles in navigation and service-call graphs.

### 7.2 F0 — Declare Flow / Stream Graph

F0 is a proposed safe dataflow execution class for workloads that otherwise push users toward Native:

```text
ADC source -> decimator -> Goertzel bank -> tone/FSK detector
           -> packet decoder -> event sink -> Declare rule/UI
```

or:

```text
RSSI source -> threshold/holdoff -> scan events -> waterfall/spectrum view
```

The package contains a graph, not executable operator code. Initially, the kernel supplies only generic, independently justified primitives:

- `AdcBlockSource`
- `Decimate`
- `FirQ15`
- `GoertzelQ15`
- `CrcCheck`
- `Threshold`
- `Timer/SymbolSource`
- `EventSink`
- `DisplayWaterfall`

Protocol-specific operations such as `AfskDemod`, `HdlcDecoder`, and `PacketBuilder` should normally remain inside a cohesive trusted protocol service. A service may expose a bounded fused pipeline through a typed interface; it must not require the kernel to adopt an entire protocol stack. Adding native operator packages to an untrusted F0 graph would require a separate trusted-operator registry, admission model, and conformance proof.

Each operator descriptor declares:

- State size and alignment.
- Input/output block geometry.
- Maximum input/output rates.
- Worst-case measured cycles per block at 48/72 MHz.
- Required capabilities and hardware leases.
- Whether it may run during TX/RX quiet windows.

The F0 validator checks an acyclic graph, static buffer sizes, rates, and total block budget. The scheduler executes bounded block work; hardware timers/DMA remain in the kernel.

F0 gives the ecosystem safe, composable DSP without pretending arbitrary native code is sandboxed. It can initially be a Declare profile rather than a fifth top-level container class.

---

## 8. Capability and fault model

### 8.1 Capabilities have different enforcement meanings by class

| Class | Meaning of a granted capability |
|---|---|
| N0/P0/N1/P1 | Trusted-code API policy and resource accounting; not a memory-security boundary |
| V0/V1 | Runtime-enforced helper access and checked buffers |
| D0/F0 | Validator/walker/operator-enforced access |

TX legality, band limits, calibration validity, battery cutoffs, and hardware arbitration remain kernel policy regardless of package class.

### 8.2 Fault policy should also differ

- **V0/V1/D0/F0 validation or runtime fault:** terminate the actor, emit `SERVICE_DIED` if needed, and continue.
- **Trusted Native/Paged clean error return:** unload normally and continue.
- **Trusted Native/Paged HardFault, stack corruption, or watchdog reset:** assume memory integrity may be compromised. Force TX off where possible, reset into the safe radio runlevel, and quarantine the package on reboot.

Trying to continue after arbitrary native corruption is not a reliable containment strategy.

Dynamic Native drivers/modules should not be part of the first open ecosystem. Built-in drivers or full firmware updates are safer until there is a proven need.

---

## 9. Resource leases and memory-reclamation ideas

Replace ambiguous ownership with leases revoked at stop/fault:

- `RadioRx`, `RadioTx`
- `AudioCapture`, `AudioOutput`
- `DisplayCanvas`
- `SpiBus`, `DmaChannel`
- `OverlayWindow`
- `StorageTransaction`
- `Timer/Alarm`

A lease records owner, mode, timeout/deadline, and cleanup action.

### Display workset leasing

A full 1024-byte canvas is not necessary for every Declare/core UI screen. Consider:

- Strip/display-list rendering for core UI and D0.
- A full canvas lease for N0 spectrum/direct-draw applications.
- A union between display workset and application scratch only when ownership is explicit and DMA is quiescent.

This is safer than permanently reserving a full framebuffer for every workload and can help reconcile the competing memory maps.

### No transparent native hibernation in v1

A native checkpoint contains only app-defined serialized state at a quiescent lifecycle point. Resume performs a fresh load and state restore. This avoids saving raw pointers, kernel handles, GOT contents, active DMA descriptors, and page-residency state.

### Flash-wear budgets

Paged reads do not cause NOR program/erase wear, but configuration, crash logs, checkpoints, and hibernation writes do. Add per-actor write counters, minimum commit intervals, and a global wear budget.

---

## 10. Package and ABI improvements

### 10.1 Class + profile + map + role are orthogonal

Each single-artifact package must identify:

```text
execution class: Native / Paged / VMachine / Declare
profile:         N0 / P0 / V0 / D0 / F0 / ...
platform_id:     uv-k5v3 or another board
map_id:          fixed native memory map, if applicable
actor role:      App / Service / Daemon / Driver / Module
native_abi:      launch and register contract
interface ABIs:  per imported interface
```

The loader must never silently interpret a P0 image as P1 or an N0 image as N1.

### 10.2 Keep per-interface ABI versions

A single `abi_version` at the root is insufficient. Use:

- Container format version.
- Runtime profile version.
- Per-interface `min_abi`.
- Append-only method tables.
- Denial stubs for unavailable optional calls.
- Named error codes for missing mandatory interfaces.

### 10.3 Generate the binary contract

One schema should generate Rust decoders, C headers, packer records, offsets/sizes, and golden tests. Manual size comments are already causing incompatible attached documents.

### 10.4 Debug sidecars

Keep symbol/source information out of the runtime package:

```text
app.krx   — compact runtime package
app.kdbg  — optional ELF/map/source debug sidecar
```

A crash record stores package hash, profile, platform/map, actor ID, PC/LR/SP, resident page ID, and page-relative offset. Host tooling symbolicates using the sidecar.

---

## 11. Performance and conformance plan

Do not use DWT_CYCCNT as the universal M0+ measurement plan; Cortex-M0+ does not provide the M3/M4-style cycle counter. Use a free-running timer such as the platform’s 32-bit TIM2 where available, plus logic-analyzer GPIO markers for end-to-end DMA/RF timing. [1](https://community.st.com/t5/stm32-mcus-products/there-is-no-dwt-in-the-m0-m0-kernel/td-p/96177)

### Required prototype bake-offs

| Experiment | Measurements |
|---|---|
| N0 versus N1 build of the same app | Code size, GOT size, reloc count, global-access cycles, Rust/GCC compatibility |
| P0 page sizes 256/512/1024 B | Miss latency, metadata, misses/minute, energy, protocol deadline margins |
| P0 versus a P1 proof of concept | Loader complexity, shadow-stack RAM, callback failures, worst-case resume latency |
| V0 register versus stack/hybrid VM | Interpreter bytes, bytecode bytes, RAM, cycles, compiler complexity |
| V0 resident versus V1 cached bytecode | Instructions/s, SPI bytes/instruction, cache miss rate |
| D0 versus equivalent native settings apps | Firmware bytes saved, package bytes, render latency |
| F0 versus native AFSK/CTCSS chain | Cycles/block, operator RAM, missed blocks, implementation size |

### Minimum target tests

- N0 Rust and C applications at the fixed VMA; wrong `map_id` rejected.
- Function pointers, vtables, literal pools, and switch statements inspected in real builds.
- P0 cross-overlay reference rejection and root-callback reentry rejection.
- P0 repeated page alternation without executing stale instructions.
- DMA page-in followed by verified barrier/entry behavior.
- V0 malformed bytecode/fuzz corpus, region violations, invalid branches, helper ABI mismatches.
- D0/F0 cyclic graphs, action storms, oversized templates, and rate-limit tests.
- Native fault reset/quarantine versus VM/Declare actor-only recovery.
- Power-loss during package/config commits.

Record every number as **estimated**, **host-simulated**, **QEMU-measured**, or **hardware-measured**, with clock, page size, compiler, and board profile.

---

## 12. Recommended implementation order

### Milestone A — one coherent runtime

- Unified actor lifecycle and resource leases.
- Generated package schema and per-interface ABI registry.
- D0 and N0.
- Fixed-map native loader, denial stubs, stack painting, safe-mode reset.
- Reference apps: repeater guide (D0), settings form (D0), spectrum/native demo (N0).

### Milestone B — large and portable applications

- P0 structured overlays with one window and root-time prefetch.
- V0 resident bytecode with regions, verifier, and fuel.
- Reference apps: phase-oriented AX.25/APRS tool (P0), calculator/automation (V0).

### Milestone C — safe streaming and dataflow

- F0 operator graphs.
- V1 bytecode cache only if V0 size limits are a demonstrated problem.
- Display workset leasing and wear accounting.
- Reference apps: CTCSS/AFSK graph, scripted beacon, large table tool.

### Milestone D — optional advanced native mechanisms

- N1 PIC only after the complete pinned-toolchain conformance matrix passes.
- P1 transparent paging only if a concrete application cannot be structured under P0.
- Dual execution windows only after traces justify them.

---

## 13. Additional ideas worth keeping

### High value, moderate effort

1. **Target-specific single artifacts:** build map-specific native and portable alternatives as separate files; host/catalog tooling selects one before transfer.
2. **Host AOT compilation:** compile a validated Declare/KVM subset to N0 on the host as a separate trusted high-performance artifact.
3. **Trace-guided page layout:** record P0 call sequences, then cluster pages and place common helpers in the root using real traces.
4. **Deterministic event replay:** record input/timer/packet events and replay them on host/QEMU for regression tests.
5. **Page heat map in the task manager:** show misses, SPI bytes, worst miss, and current page rather than only “RAM used.”
6. **Lease-aware display/audio memory:** reclaim large buffers when their owning subsystem is inactive.
7. **Per-package storage and wear quotas:** prevent a small automation script from destroying NOR endurance.
8. **Native one-shot tools:** trusted native utilities run to completion and unload instead of becoming hidden permanent services.

### Higher effort, potentially valuable

9. **Flow superoperators:** fuse common `Decimate -> FIR -> Goertzel` paths into one audited kernel operator after profiling.
10. **Declare-to-KVM lowering:** compile complex expressions/rules into the same verified VM core instead of growing a second expression language.
11. **Install-time artifact synthesis:** generate the correct fixed-map native artifact during provisioning on a trusted host, then sign/hash that single output.
12. **Page-specific read-only assets:** colocate page code and its private constants to avoid a separate resident `.rodata` budget.
13. **Radio-quiet DMA scheduler:** prioritize page-in and asset streaming around RF timing requirements using actual measured quiet windows.
14. **Transactional service restart:** checkpoint service state, restart cleanly, and replay only idempotent requests.

### If strong native third-party isolation becomes a hard requirement

15. **Change the hardware assumption:** select an MCU with a real MPU and more SRAM, or add a companion application MCU. Keep the radio MCU responsible for TX policy and communicate over RigWire. This is a more credible isolation boundary than arbitrary-Thumb SFI on the PY32F071.

---

## 14. Recommended short specification wording

> KRX supports fixed-map trusted native packages, structured trusted overlays, and software-sandboxed VM/declarative packages. Every KRX file is a single selected artifact with one execution profile, target/map, entry point, and payload; host tooling selects among separate build outputs before transfer. Dynamic loading does not imply arbitrary placement, preemptive native multitasking, or native memory isolation. Native and Paged packages execute privileged machine code and require trusted installation. KVM and Declare enforce memory and capability access through their respective verifiers and runtimes. P0 overlays use a resident root and run-to-return leaf pages; transparent cross-page native calls are a separate experimental profile. All actor handlers are non-blocking and run within declared resource and timing budgets.

This preserves the ambitious KRX vision while making the first implementation smaller, safer, and substantially easier to verify.
