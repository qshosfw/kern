# KRX Small-Package Application Ecosystem — Research Proposal

**Status:** v2; supersedes the rejected domain-bundle and fat/multi-variant draft  
**Target:** PY32F071-class Cortex-M0+, 16 KiB SRAM, no MPU, external SPI NOR  
**Companion:** `KRX_Execution_Runtime_v3_2_Proposal.md`  
**Design center:** Small standalone KRX files, compact headers, typed services, bounded residency, and no device-side package transformation

---

## 1. Executive decision

KRX should remain a **small, single-purpose, single-artifact package ecosystem**.

A `.app`, `.svc`, `.dmn`, or generic `.krx` file is a complete deployable object. It is not a suite, archive of alternate applications, or collection of target binaries waiting to be unpacked on the radio.

### Normative direction

1. **One file, one purpose.** A package has one coherent application, service, daemon, driver, module, or content role.
2. **One executable artifact.** Each file has one execution class/profile, target/map, entry point, trust meaning, and executable payload.
3. **No fat packages.** Alternate native maps or portable implementations are separate host build outputs. Host/catalog tooling selects one before transfer.
4. **No domain bundles.** Related applications remain separate files when they have independent lifecycles.
5. **No micro-fragmentation.** A cohesive app may contain several screens, and a cohesive protocol service may contain several internal layers.
6. **Reuse through the SDK first.** Pure functions and single-client components are statically linked from shared source, with unused code removed by the linker.
7. **Reuse through services when ownership matters.** Independent activation, replacement, background work, shared mutable state, or multiple concurrent clients can justify a `.svc`.
8. **Keep KRX’s existing style.** Compact TLV sections, 128/256-byte scans, filename identity, capability imports by interface, lazy providers, Declare Data/Trigger exports, per-package configuration, and the five-entry AppVTable remain the base.
9. **Keep protocols out of the kernel.** The kernel provides execution, arbitration, timing, storage, and generic primitives. Protocol stacks remain packages unless the App-Implementability Test proves otherwise.
10. **Optimize resident working sets, not just file size.** Many tiny providers with a deep resident dependency chain can be worse than one cohesive service.

The central rule is:

> **Small in scope, singular in artifact, bounded in residency.**

“Small” is not an arbitrary byte limit. A single P0 AX.25 service may need substantial paged code while remaining one focused artifact. It must not also contain APRS UI, KISS, DIGI, games, alternate CPU binaries, and unrelated assets.

---

## 2. The KRX style that should be preserved

The attached KRX material already contains a strong packaging philosophy. The ecosystem should elaborate it rather than replace it with a foreign bundle model.

| Existing KRX rule | Ecosystem consequence |
|---|---|
| One file, one scan, one launch | No unpacking or installation-time application extraction |
| `.app`, `.svc`, `.dmn`, generic `.krx` | Different citizens remain separate small files |
| Header ≤256 B; 128 B fast-scan prefix | Discovery cost stays independent of payload size |
| Execution class and exec type are orthogonal | Native/Paged/VM/Declare does not determine App/Service/Daemon role |
| CapImports name interfaces, not provider UIDs | Replaceable providers without package dependency bundles |
| Services activate on first bind | Unused protocol code need not occupy RAM |
| Idle unpinned services may suspend | Small resident working sets are possible |
| Declare has Screens, Rules, Schedules, State, Tables, Templates | Many small apps/services need no machine code |
| Declare can export Data and Trigger | Tiny content providers and rule providers fit the same package model |
| Per-package config is keyed by basename | Configuration remains simple and survives replacement with the same canonical name |
| AppVTable has five entries | Existing app lifecycle is sufficient; no multi-activity descriptor framework is needed |
| The kernel ships no protocol | AX.25, CW, Spectrum, SSTV, and similar functionality belong in replaceable packages |

### Ideas withdrawn from the previous draft

- Multi-activity domain packages.
- Fat packages containing several execution variants.
- Device-side selection among embedded native and portable payloads.
- A new global document-store architecture as a prerequisite.
- Runtime shared native libraries as the default reuse mechanism.
- Moving protocol implementations into the kernel merely to make application files smaller.
- Splitting every screen or helper into a separate service.

---

## 3. What the additional research changes

### 3.1 F4HWN: separate apps are not the root problem

The F4HWN audit found duplicated modem/UI/configuration logic across separate applications. The solution is not necessarily to merge those applications into one large binary.

The actual problems are:

- copied implementations instead of one authoritative source component;
- app-owned versions of generic infrastructure;
- cross-application data represented by private physical layouts;
- repeated real-time and cleanup policy;
- no clean way to offer one protocol implementation to multiple clients.

KRX can solve those with SDK components, a small number of interface-bound services, Declare data providers, and coherent workflows—while keeping each app as its own file.

F4HWN’s host models and tests remain worth adopting. A shared source component should have one host test suite even when several small KRX artifacts statically link it.

### 3.2 Flipper: single apps plus explicit static-library cost

Flipper builds external applications as individual FAP files. Its manifest documentation also makes the trade-off explicit: adding `fap_libs` provides functions not exported by firmware **at the expense of FAP size and RAM consumption**. Private libraries are built as part of the app rather than becoming a suite of runtime packages. [1](https://developer.flipper.net/flipperzero/doxygen/app_manifests.html)

This supports two KRX decisions:

- one application artifact per file;
- static SDK reuse is valid, but the build must show exactly what each component adds to file size and resident memory.

KRX should not hide static-library cost behind an elegant component name.

### 3.3 RIOT: build-time modules remove source duplication without runtime packages

RIOT applications select modules at build time. Modules declare dependencies, and the build system resolves the required source set before linking. [1](https://guide.riot-os.org/advanced_tutorials/creating_modules/)

The lesson for KRX is:

> A reusable component does not automatically need a `.krx` file.

CRC, ring buffers, fixed-point helpers, parsers, formatting, and single-client protocol code can live in an SDK source registry. `krx-pack` consumes only the selected slices and reports their final contributions.

### 3.4 Tock: distinguish the single object from the distribution bundle

Tock’s TBF object describes one application binary and its resource requirements. Its separate TAB format can bundle several architecture-specific TBF objects for host-side installation convenience. [1](https://book.tockos.org/doc/tock_binary_format) [2](https://book.tockos.org/doc/compilation)

KRX should follow the **single-object** part, not put a TAB-like archive on the radio:

- one KRX file is analogous to one selected executable object;
- target alternatives remain separate host artifacts;
- there is no on-device fat-binary parser;
- there is no requirement to create a host archive either—CI can publish separate artifacts directly.

### 3.5 OSGi: service interfaces are useful; unbounded service dynamics are not

OSGi demonstrates publish/find/bind composition and delayed activation. Its own Declarative Services specification identifies memory footprint, startup time, and the complexity of services appearing/disappearing as reasons for a more declarative model. [7](https://www.osgi.org/resources/architecture/) [8](https://docs.osgi.org/specification/osgi.cmpn/7.0.0/service.component.html)

KRX already has the preferable dependency axis: **interfaces, not bundle/package UIDs**.

The constraint for KRX is to bound that dynamism:

- static declared dependencies;
- acyclic provider graphs;
- explicit activation cost;
- bounded provider depth and stack chain;
- generation-checked rebinding after `SERVICE_DIED`;
- no arbitrary runtime discovery in a real-time path.

### 3.6 Femto-containers: small event-driven functions are a real ecosystem class

Femto-container research targets small, event-driven, short-lived virtual functions with checked memory access, host helpers, and small key-value state rather than large application images. [1](https://arxiv.org/pdf/2210.03432)

This supports KRX’s V0/D0 direction:

- small calculators, rules, monitors, adapters, and protocol policies;
- event-triggered execution rather than retained native threads;
- typed state/helpers instead of direct hardware access;
- many small packages without many native stacks.

It does not justify putting real-time native DSP inside untrusted packages.

### 3.7 Microkernel granularity: coalesce tightly coupled hot paths

Microkernel research repeatedly shows that fine-grained modularity increases invocation frequency and can duplicate bookkeeping. Recent work explicitly co-locates or coalesces tightly coupled services in performance-demanding paths while retaining separation where isolation or independent ownership matters. [1](https://ipads.sjtu.edu.cn/_media/publications/chen-osdi24.pdf)

For KRX, the corresponding rule is:

- separate control-plane applications and policy where useful;
- keep a sample-rate or symbol-rate pipeline inside one cohesive provider;
- do not turn `CRC`, `NRZI`, `HDLC`, and `AX.25` into a chain of tiny services called for every bit.

---

## 4. The single-artifact KRX contract

### 4.1 One file has one deployable meaning

A KRX file contains:

```text
one canonical package identity (filename on device)
one execution class/profile
one exec type or content role
one platform/map target where applicable
one primary entry point or Declare graph root
one capability/import set
one optional private asset set
one optional config schema
one optional service-export set belonging to the same purpose
```

A Service may expose more than one interface only when those interfaces are facets of the same cohesive provider. For example, a repeater-book Declare provider can expose Data and Trigger without becoming a multi-application bundle.

### 4.2 No alternate executable payloads

A file must not contain:

- MapA and MapB native images;
- Native and VM fallback programs;
- several independent apps;
- an application plus unrelated service implementations;
- a host SDK, debug ELF, symbols, and source archive in the runtime file;
- payloads that require the device to synthesize a smaller executable.

Debug information remains a host-side `.kdbg`/ELF/map sidecar.

### 4.3 Private assets are allowed; bundling is not the same as duplication

The existing KRX asset directory remains useful for icons, bitmaps, fonts, melodies, and private protocol tables belonging to that one package.

This does not violate the small-package rule. The distinction is:

- **allowed:** assets used by the file’s single purpose;
- **not allowed:** duplicated application binaries or unrelated domain content.

Large read-only data shared by several packages can become a separate small Declare/Data content provider when that saves real storage and independent ownership is clear.

### 4.4 Target selection happens before transfer

One source project may produce:

```text
dist/py32-map-a/aprs_rx.app    N0 / MapA
dist/py32-map-b/aprs_rx.app    N0 / MapB
dist/portable/aprs_rx.app      V0 / portable
```

The host catalog or provisioning tool selects one and installs it as the canonical `aprs_rx.app`. The radio sees only that file.

Consequences:

- configuration remains attached to the same basename;
- the device never pays storage for unused variants;
- the loader retains simple fail-closed target checks;
- each artifact has one trust meaning;
- CI tests every published artifact independently.

---

## 5. Choosing the right package granularity

### 5.1 Split at lifecycle and ownership boundaries

A separate KRX file is justified when at least one strong boundary exists:

- independently launchable user workflow;
- independent background lifetime;
- replaceable provider of a stable interface;
- distinct trust/install consent;
- ownership of shared mutable state;
- independent content update;
- measured reduction of active resident working set;
- portability/execution class that cannot be represented in the same single artifact.

Do not create a file merely because a function, class, screen, or protocol layer looks reusable in source.

### 5.2 Merge natural workflows

Small packages do not mean one package per screen.

For example, APRS TX and APRS profile editing are one natural workflow. A single `aprs_tx.app` can contain:

- transmit screen;
- profile edit screen;
- validation rules;
- private `aprs_tx.cfg` state;
- packet template;
- send/cancel/progress behavior.

That removes the old cross-app handoff and configuration proxy without creating an APRS suite.

### 5.3 The reuse placement matrix

| Reusable thing | Default home | When to make a separate KRX service/content file |
|---|---|---|
| CRC, integer math, small fixed-point helpers | SDK source or existing CapMath | Essentially never as an independent service |
| Ring buffers, packet builders, parsers | SDK source component | When independent provider ownership is demonstrably useful |
| Generic menu/form/table rendering | Existing Declare/core UI substrate | Not as a runtime package |
| Custom drawing helpers | SDK source | Only for a genuinely independent graphics provider, unlikely in v1 |
| Private icons/fonts/tables | Assets inside the owning file | Shared read-only data may become a Data provider |
| APRS codec used by one app | SDK source | When several clients need a replaceable APRS provider |
| AX.25 modem/session used by RX, KISS, DIGI | Prefer one cohesive `ax25.svc` | Already has independent ownership and multiple clients |
| Shared mutable configuration | Owning app’s private config | A service only when multiple packages must share the same committed state |
| Read-only repeater/symbol database | Private asset if single-client | Declare `.krx` exporting Data when shared or independently updated |
| Sample/symbol-rate DSP pipeline | One cohesive trusted provider | Do not split into per-stage microservices |
| Background beacon/digipeat policy | `.dmn` or Declare service | Separate because its lifetime is autonomous |
| Game math/sprites/input helpers | SDK source and private assets | Not separate runtime services |

### 5.4 Service granularity test

Before creating or splitting a `.svc`, answer:

1. Does it own a resource or session independently of any one UI app?
2. Will at least two real clients use the same interface?
3. Must it remain active while its caller is suspended?
4. Is independent replacement without rebuilding clients valuable?
5. Does it reduce the maximum resident working set?
6. Does it isolate a different trust or failure policy?
7. Will the split avoid more complexity than it adds?

If none produce a concrete affirmative answer, keep the component in the SDK or the owning app.

---

## 6. Residency and service-call architecture

### 6.1 File size is not the main scarce resource

A small `.svc` still consumes resources when active: resident code or page root, state, stack, buffers, registry entries, and possibly pinned assets/pages.

The admission calculation must include the complete active closure:

```text
active_workset =
    caller resident code/state
  + provider resident code/state
  + transitive provider closure
  + shared packet/audio/display buffers
  + maximum service-call stack chain
  + runtime metadata and guards
```

This must fit the selected arena/map with measured margin. A directory full of tiny files says nothing about whether that closure fits.

Declare and VM UI clients help because their execution engine is resident in firmware; their packages contribute bounded state/bytecode rather than another native machine-code stack.

### 6.2 Three reuse lanes

**Lane A — SDK/static**

- One active native image.
- No provider activation or runtime dependency.
- Best for pure helpers and single-client native components.
- External NOR may contain the same code in several small artifacts; only the active artifact’s code occupies SRAM.

**Lane B — Bound package service**

- Independent provider lifecycle and interface.
- Best for shared protocols, sessions, background work, and shared mutable state.
- Requires closure admission and defined failure/rebinding semantics.

**Lane C — Declare Data/Trigger**

- Tables, content, rules, adapters, and small automation providers.
- Very small resident state; rules/tables can stream.
- Often the cheapest way to share data or behavior among small packages.

### 6.3 Synchronous calls are for short resident operations

Direct vtable calls are appropriate only when:

- caller and provider are admitted and resident;
- the method is nonblocking and has a declared stack/time cost;
- buffer lifetimes are explicit;
- no page-in or provider activation is needed on the call’s hot path;
- the provider cannot recursively reenter the same call chain unexpectedly.

Examples: read a typed configuration value, query a row, encode a small bounded frame, query service state.

### 6.4 Long operations use sessions and events

RX streaming, TX, sweep, serial sessions, storage transactions, and long computations use:

```text
open/request session
→ provider becomes active and required pages/worksets are admitted
→ start
→ bounded events carry progress/data/completion
→ stop/cancel/timeout
→ leases released; idle provider may suspend
```

Events use the existing MBuf/token model for bulk data. They do not carry native pointers with unclear lifetime.

### 6.5 No transparent provider page-in on an arbitrary caller fault path

The KRX 2.5 idea of a paged provider appearing transparently on a caller’s fault path should not become the production baseline. It can require caller code, provider code, page metadata, return state, and I/O completion all to remain valid simultaneously.

Instead:

- resolve and admit the provider before entering the method/session;
- preload and lock required real-time code before starting;
- use asynchronous completion for operations that cannot complete synchronously;
- fail admission before RF/session start if the closure does not fit.

### 6.6 Keep provider graphs shallow

The initial production shape should be:

```text
App/Daemon → one package provider → kernel primitives
```

Nested package-to-package providers must be acyclic, declared, and included in stack/residency calculations. They should require evidence rather than becoming the default design style.

`PIN_PROVIDER` is appropriate for an active real-time session, not as a convenient way to keep every installed service resident forever.

---

## 7. Lifecycle, UI, state, and IPC in KRX style

### 7.1 Keep the existing AppVTable

The existing contract is sufficient:

```text
on_start
on_event
on_modal_result
render
on_exit
```

A normal app does not need a new nested activity framework. Screens are internal app state or Declare nodes. The shell continues to own the foreground app, modal compositor, global shortcuts, and event routing.

Natural navigation within an app stays inside that app. Cross-package workflows use existing events, services, Trigger, and modal/result channels rather than embedding several activities into one package.

### 7.2 Declare is the default for small ordinary apps

Use Declare for:

- settings and profile forms;
- reference/table browsers;
- SysInfo/data dashboards;
- simple calculators and validators;
- beacon/rule adapters;
- small Data and Trigger providers;
- packet templates and canned service compositions.

Use V0 for bounded custom logic that exceeds expressions/rules. Use N0/P0 for trusted custom rendering, DSP, or code that genuinely requires native execution.

### 7.3 Configuration has one owner

The default remains `/apps/cfg/<name>.cfg` with the package’s typed schema.

Rules:

- one package owns and writes its configuration;
- other packages never edit it through physical offsets or a proxy descriptor;
- read-only shared tables use Data;
- shared mutable state required by several packages gets an explicit service owner and versioned interface;
- if only one workflow uses the state, keep it inside that workflow’s app instead of creating a shared service.

This is simpler than introducing a global document architecture.

### 7.4 Data and Trigger cover many tiny packages

Examples:

- `repeater_book.krx`: tables + computed columns + Data export.
- `band_plan.krx`: content tables + TableBrowser screens.
- `aprs_symbols.krx`: optional shared read-only symbol metadata.
- `canned_beacon.svc`: Declare schedule/rule + Trigger export.
- `message_templates.krx`: templates and Data rows consumed by a small messenger app.

These are single-purpose files, not resource bundles for an application suite.

### 7.5 IPC remains typed and bounded

- CapImports resolve interface IDs and ABI levels.
- SysEvent handles small command routing.
- Service interfaces define structured method arguments.
- MBuf tokens carry bounded bulk payloads.
- Declare events and rules use validated IDs/offsets.
- Provider death produces `SERVICE_DIED`; clients re-resolve rather than retaining stale vtable pointers.
- Queue capacity, backpressure, and event rates are part of each interface contract.

---

## 8. Radio, DSP, and protocol packages

### 8.1 Preserve the kernel’s minimal role

The kernel owns:

- hardware drivers and bus arbitration;
- IRQ/cycle-accurate primitives;
- memory, package, and actor management;
- RF leases and policy-neutral arbitration;
- TX safety/policy checks and regulatory data;
- generic math, buffer, timing, storage, and streaming primitives.

Protocol implementations remain packages:

- AX.25;
- APRS;
- CW;
- DTMF;
- POCSAG/DSC/AIS decoders;
- SSTV;
- sonde decoders;
- application-specific measurement policy.

### 8.2 Keep hot pipelines cohesive

A practical `ax25.svc` may internally contain:

```text
audio capture session
Bell 202 RX/TX
HDLC/NRZI/framing/FCS
AX.25 frame validation and bounded encode/decode
packet/session events
```

That is not an oversized domain bundle. It is one protocol provider with one session boundary and one failure/ownership domain.

Splitting these stages into separate packages would increase resident metadata, call depth, buffer handoffs, and failure combinations without helping users.

### 8.3 F0 must follow the same rule

If F0 is pursued:

- the kernel may provide generic graph walking, buffer scheduling, and truly generic operators justified by the App-Implementability Test;
- protocol-specific operators should be supplied by trusted packages or exposed as one fused provider;
- a Declare file references operators by ID and never carries untrusted native operator code;
- the complete operator/session working set is admitted before real-time start.

F0 is not a reason to move APRS/AX.25 into the kernel or to create a chain of tiny DSP packages.

### 8.4 Radio ownership still outranks modularity

Whether AX.25 is statically linked or provided by `ax25.svc`, the same invariants apply:

- explicit RX/TX/audio leases;
- central TX admission;
- bounded session deadlines;
- forced stop on actor/package failure;
- previous radio state restoration;
- no app-owned IRQ vectors;
- no arbitrary package writes to PA/RF registers.

---

## 9. SDK and host tooling for small artifacts

### 9.1 `krx-pack` builds one target at a time

Conceptual workflow:

```text
krx-pack \
  --package aprs_rx.app \
  --platform py32f071 \
  --profile N0 \
  --map MapA \
  --output dist/py32-map-a/aprs_rx.app
```

Another invocation produces another independent artifact. There is no `variants = [...]` section in the runtime file.

The packer should reject:

- multiple execution classes for one output;
- multiple entry actors;
- embedded alternate payloads;
- unresolved required interfaces;
- wrong map/segment layouts;
- hidden native library dependencies;
- package assets not referenced by the package’s purpose;
- missing size/residency reports.

### 9.2 SDK components are source-level modules

Recommended structure:

```text
krx-rt                 lifecycle, ABI, handles, events
krx-components/
  crc/
  fixed_point/
  packet/
  ax25_codec/
  aprs_codec/
  ui_helpers/
  history/
  serial_framing/
krx-idl/               public interface/schema source of truth
```

Build with section garbage collection and, where verified, LTO. The host report attributes bytes to each selected component so “shared SDK” does not become uncontrolled bloat.

### 9.3 Host/catalog CI produces alternatives, not bundles

A build matrix can publish:

```text
aprs_rx.app / py32-map-a
aprs_rx.app / py32-map-b
aprs_rx.app / portable-v0
```

The catalog selects using device platform/map/ABI information. All transformation—ELF stripping, relocation extraction, asset encoding, compression, target selection—happens on the host.

The runtime KRX file remains directly scannable and loadable.

### 9.4 Measure four different sizes

Every artifact report should distinguish:

1. **File size** in external NOR.
2. **Resident code/root size** while active.
3. **State/stack/workset size** including required buffers.
4. **Active closure size** including bound providers.

Optimizing only the first can make the fourth worse.

---

## 10. Reference design: APRS as small KRX files

### 10.1 Primary file set

| File | Role | Typical class | Responsibility |
|---|---|---|---|
| `ax25.svc` | Service | N0 or P0 | Cohesive Bell 202/HDLC/AX.25 modem and session provider |
| `aprs_rx.app` | App | D0 or V0 | Receiver screens, history, packet detail, APRS presentation |
| `aprs_tx.app` | App | D0 or V0 | TX screen **and internal profile editor**, validation, private config, send workflow |
| `aprs_digi.dmn` | Daemon | D0 or V0 | Digipeat policy, path rewrite, dedupe, channel-access behavior |
| `kiss_tnc.app` | App | V0 or N0 | KISS serial framing, host backpressure, AX.25 session client |

Optional only when a real second client requires it:

| File | When justified |
|---|---|
| `aprs.svc` | Shared APRS composer/profile/session owned independently of the TX app |
| `aprs_symbols.krx` | Shared read-only symbol table used by several packages or updated independently |

There is deliberately no `aprs_edit.app`: editing belongs inside the TX workflow unless another package genuinely needs to edit the same committed profile.

### 10.2 What is shared and how

- Bell 202/HDLC/AX.25: one `ax25.svc` provider.
- APRS parse/format helpers: initially an SDK `aprs_codec` source component.
- TX profile: private `aprs_tx.cfg` while only TX uses it.
- Packet history: bounded state/table owned by `aprs_rx.app`.
- Icons/help: private assets unless independently shared.
- Host tests: one SDK/protocol test suite reused by service and app builds.

### 10.3 Residency scenarios

**Receiving**

```text
aprs_rx.app (small D0/V0 state)
  → ax25.svc active native/paged provider
  → kernel radio/audio primitives
```

Other APRS apps need not be resident.

**Editing the TX profile**

- `aprs_tx.app` edits its detached local state.
- AX.25 need not be active.
- Commit goes to its own typed config.
- No cross-app handoff or config proxy exists.

**Transmitting**

- The same app validates the profile and opens an AX.25 TX session.
- Required provider pages/worksets are admitted before start.
- Completion arrives as an event; cancellation and TX policy remain centralized.

**KISS or DIGI**

- Only the selected client and AX.25 provider own the active session.
- An incompatible concurrent DIGI/KISS/TX request is refused or serialized by the RF/session arbiter.
- Small scope does not imply all related packages run simultaneously.

### 10.4 When an APRS service becomes justified

If a beacon daemon, TX app, and external automation all need the same committed profile/composer, introduce `aprs.svc` as the owner of that state/interface. The TX app then becomes a client of that service.

Do not create this extra provider preemptively. The correct small-package architecture allows it to be added without changing the AX.25 interface or merging files.

---

## 11. Other example ecosystems

### 11.1 Sonde receivers

```text
sonde_rx.app                 common presentation/history
sonde_m10.svc                M10 decoder/session provider
sonde_m20.svc                M20 decoder/session provider
sonde_rs41.svc               RS41 decoder/session provider
krx-components/sonde/        shared source math/UI/formatting helpers
```

Only the selected decoder and UI need to be active. Separate providers make sense if users select protocols independently and residency measurements favor loading one at a time. If all three are always needed together, one cohesive `sonde.svc` with P0 decoder leaves may be better; decide by measurement.

### 11.2 Spectrum and direction finding

```text
spectrum.svc                 bounded sweep/measurement sessions
spectrum.app                 waterfall/display
foxhunt.app                  direction-finding policy and presentation
sigfind.app                  signal-search policy
```

The sweep provider is shared; each app remains small and single-purpose. Real-time sweep state is not duplicated across UI apps.

### 11.3 SysInfo and reference tools

```text
sysinfo.svc                  typed diagnostic Data/provider
sysinfo.app                  Declare TableBrowser/InfoCard screens
resistor.app                 one Declare calculator/form
repeater_book.krx            Data tables plus optional browser screens
```

A native app is unnecessary merely because the displayed values originate in hardware diagnostics.

### 11.4 Games

Each game remains one V0/N0 file with private assets. Shared random/math/sprite helpers come from the SDK. There is no `games.krx` suite and no runtime `sprite.svc` unless measurements produce an extraordinary reason.

---

## 12. Developer experience and verification

### 12.1 Host tools

```text
krx new <template>          one app/service/daemon/content project
krx check                   schema, imports, ownership, size, graph checks
krx test                    component and package tests
krx sim                     host UI/service/event simulation
krx replay <trace>          deterministic event replay
krx pack --target ...       exactly one output artifact
krx size                    file + resident + workset + closure reports
krx symbolicate <fault>     host-side debug sidecar
```

### 12.2 Required tests

- Single-entry/single-payload container conformance.
- Wrong platform/map/profile artifact rejection.
- Header fast scan independent of payload size.
- No hidden runtime native dependencies.
- SDK component host tests and byte attribution.
- Provider activation/suspend/rebind sequences.
- Active-closure admission failures before RF start.
- Stack-chain and reentrancy tests.
- Queue backpressure and stale MBuf/handle tests.
- Config ownership and power-fail commit tests.
- `SERVICE_DIED` recovery and generation checks.
- Host loopback for AX.25 and other protocols.
- Hardware timing tests for every real-time provider.

### 12.3 Architectural lint rules

Reject or warn on:

- alternate executable payloads in one file;
- app suites or unrelated roles in one package;
- package-UID dependencies where an interface dependency suffices;
- cross-package physical config offsets;
- protocol code added to the kernel without passing the App-Implementability Test;
- per-helper runtime services;
- unbounded provider depth or recursive provider graphs;
- provider page-in during an unplanned real-time call;
- private assets duplicated into every target artifact on the device;
- reports showing file size without active closure size.

---

## 13. Implementation sequence

### K0 — Align the runtime and packer

- Apply the single-artifact rule in `krx-pack` and the loader.
- Remove fat/multi-variant assumptions.
- Keep target alternatives in host output directories.
- Complete the existing N0/ABI/I/O/capability correctness work.

**Gate:** two host artifacts from one source project install independently under the same canonical basename; each is validated as one payload.

### K1 — Establish SDK source reuse

- `krx-rt`, generated `krx-idl` bindings, initial component registry.
- Section GC/LTO reports and host tests.
- No runtime service for pure helpers.

**Gate:** two small native apps share one authoritative component source without sharing a runtime library.

### K2 — Prove small Declare apps and content providers

- Resistor/calculator app.
- SysInfo/Data app.
- Repeater-book Data provider.
- Typed per-package config and power-fail tests.

**Gate:** these packages need no native app-specific UI loop and no multi-file installation.

### K3 — Implement one cohesive AX.25 service

- Bounded RX/TX sessions.
- Host vectors and loopback.
- Lazy activation, idle suspension, explicit closure accounting.

**Gate:** `aprs_rx.app` and `aprs_tx.app` bind the same provider without embedding another modem implementation.

### K4 — Prove independent clients and granularity

- Add `kiss_tnc.app` and `aprs_digi.dmn`.
- Add the internal TX profile editor.
- Test RF arbitration and provider failure/rebind.

**Gate:** no copied modem, no `aprs_edit.app` handoff, no APRS suite package, and no unbounded provider chain.

### K5 — Generalize only after measurement

- Add sonde or spectrum providers.
- Compare one cohesive provider versus separate decoder services using active workset and call-frequency measurements.
- Consider F0 only after the native service path is correct and measured.

---

## 14. Final position

The corrected KRX ecosystem is neither a set of monolithic application suites nor a cloud of microscopic runtime libraries.

It is:

> **A collection of small standalone KRX artifacts, each with one purpose and one executable meaning, composed through build-time SDK components, compact typed interfaces, lazily activated services, Declare Data/Trigger providers, and bounded resident working sets.**

This preserves the original KRX style:

- one file, one scan, one launch;
- compact headers and direct loading;
- Apps, Services, Daemons, Drivers, Modules as separate citizens;
- protocols outside the kernel;
- no multi-file application directories;
- no device-side variant extraction;
- no giant `.krx` packages;
- no requirement to merge natural related functionality merely to avoid duplication.

The key elegance is not the number of files or their byte count. It is that **each file has one clear reason to exist, each shared implementation has one authoritative home, and every active combination has a provable memory and lifecycle budget.**

---

## 15. Sources

### KRX inputs

1. `uploads/app_format_spec.md` — original single-file KRX vision, compact sections, capabilities, assets, and SDK direction.
2. `uploads/krx_spec_extracted.txt` / `uploads/krx-spec-temp.pdf` — KRX-ABI 2.5.0; single-file goals, exec types, interface dependencies, lazy services, Declare Data/Trigger, per-package config, AppVTable, and package examples.
3. `KRX_Execution_Runtime_v3_2_Proposal.md` — corrected execution profiles and single-artifact target selection.

### Audited firmware

4. [F4HWN](https://github.com/armel/uv-k1-k5v3-firmware-custom), snapshot `3574e942e0d6` — application patterns, duplicated helpers, protocol implementations, configuration/handoff, and host tests.
5. [k1-fw](https://github.com/fagci/k1-fw), snapshot `b415012e5f1b` — small app lifecycle, navigation, shared radio state, and global-coupling trade-offs.
6. [Trakt](https://github.com/shorekeeper/trakt-quansheng), snapshot `99033446ff58` — concrete ownership, bounded dispatch, editing, TX policy, and storage journal.
7. [rQuansheng](https://github.com/Kezii/rQuansheng), snapshot `2df29a1e40cb` — static Rust ownership and host/target reuse.

### Additional ecosystem research

8. [Flipper FAP application manifests](https://developer.flipper.net/flipperzero/doxygen/app_manifests.html) — individual external apps and explicit static-library size/RAM trade-offs.
9. [Flipper FAP loading and API compatibility](https://developer.flipper.net/flipperzero/doxygen/apps_on_sd_card.html) — single-app loading and compatibility checks.
10. [RIOT module creation](https://guide.riot-os.org/advanced_tutorials/creating_modules/) — build-time reusable modules and dependencies.
11. [Tock Binary Format](https://book.tockos.org/doc/tock_binary_format) — one application object and its resource metadata.
12. [Tock compilation/TAB](https://book.tockos.org/doc/compilation) — separate target artifacts and host-side application bundles.
13. [OSGi architecture](https://www.osgi.org/resources/architecture/) — service-based composition and interface boundaries.
14. [OSGi Declarative Services](https://docs.osgi.org/specification/osgi.cmpn/7.0.0/service.component.html) — delayed activation, memory footprint, and service-dynamics complexity.
15. [Femto-Containers](https://arxiv.org/pdf/2210.03432) — small event-driven isolated functions and key-value state.
16. [HM microkernel performance work](https://ipads.sjtu.edu.cn/_media/publications/chen-osdi24.pdf) — coalescing tightly coupled services and separating control/data paths where justified.
