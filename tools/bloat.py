#!/usr/bin/env python3
"""Kern flash-bloat analyzer: finds *actionable* refactor candidates in the release ELF.

Subcommands:
  tasks     autopilot task cards with exact file:line, ROI, & step-by-step fix recipes (AI/DEV START HERE)
  audit     executive architectural health report & dynamically ranked top bloat sources
  phases    live scoreboard against docs/flash_audit_measured_plan.md Phases 10-21 targets
  stack     stack-frame size audit: finds functions allocating large stack frames (>= 96 B)
  fields    Cortex-M0+ imm5 struct offset penalties (movs + ldr/str due to offsets >31/62/124 B)
  dups      identical / near-identical functions (normalized Thumb disassembly) -> merge or share
  outline   repeated instruction sequences across functions -> extract helper / data table
  mono      generic instantiations grouped by base path -> de-monomorphize candidates
  calls     call-site fan-in x setup cost; large single-caller fns (inlining monoliths)
  vtables   indirect `blx rN` call sites (dyn Trait dispatch in drivers/hot paths)
  consts    32-bit literals repeated across functions -> shared tables/enums
  strings   duplicate / near-duplicate rodata strings -> string pool / nth_cstr
  crates    .text/.rodata rollup per crate::module (depth configurable)
  suggest   compact ranked pattern-coded refactor list with file:line
  rodata    attribute anonymous .rodata gaps to referencing fns + classify (strings/glyphs/tables)
  pools     literal-pool tax, most-reloaded statics, fns exceeding +1020 B ldr reach
  math64    __aeabi_l* 64-bit emulation callers;  memcpy  mem* callers;  traps  udf check sites
  divs / unaligned   software division / unaligned-access helper callers
  syscalls  list runtime::syscall shims sorted by size (Phase 10 target <= 24 B)
  find      search symbols by substring with file:line and callees
  modules   module flash budget (.text + named .rodata)
  baseline  save per-symbol sizes + section totals to JSON;  diff  compare current ELF to baseline

Usage: tools/bloat.py <cmd> [--elf PATH] [--top N] [--json FILE] [...]
"""
import argparse, collections, hashlib, json, os, re, subprocess, sys

DEF_ELF = "target/thumbv6m-none-eabi/release/firmware"
FLASH_MAX = 120832  # 128KB minus bootloader reserved pages on PY32F071xB
FN_RE = re.compile(r"^([0-9a-f]+) <(.+)>:$")
INS_RE = re.compile(r"^\s*([0-9a-f]+):\s+((?:[0-9a-f]{4}\s?)+)\s+(\S+)\s*(.*)$")

CRATE_ROOTS = {
    "firmware": "src/firmware/src",
    "kernel": "src/kernel/src",
    "drivers": "src/drivers/src",
    "py32_hal": "src/mcu/py32f071/hal/src",
    "py32_pac": "src/mcu/py32f071/pac/src",
    "bsp": "src/board/uv-k1/src",
    "kern_platform_traits": "src/hal-traits/src",
    "gfx": "src/gfx/src",
    "kernfs": "src/fs/src",
    "calib": "src/calib/src",
    "crypto": "src/crypto/src",
    "utils": "src/util/src",
}

UNACTIONABLE_PREFIXES = (
    "compiler_builtins::",
    "core::",
    "__aeabi_",
    "__cortex_m_rt_",
    "Reset",
    "DefaultHandler",
    "HardFault",
)


def sh(*a):
    return subprocess.run(a, capture_output=True, text=True, check=True).stdout


def symbols(elf):
    out = {}
    for ln in sh("arm-none-eabi-nm", "-S", "-C", "--size-sort", elf).splitlines():
        p = ln.split(" ", 3)
        if len(p) == 4:
            out[p[3]] = (int(p[1], 16), p[2], int(p[0], 16))
    return out


def elf_sections(elf):
    """Return {sec_name: (size, addr)} from objdump."""
    secs = {}
    for ln in sh("arm-none-eabi-objdump", "-h", elf).splitlines():
        parts = ln.split()
        if len(parts) >= 6 and parts[0].isdigit():
            name, sz, addr = parts[1], int(parts[2], 16), int(parts[3], 16)
            secs[name] = (sz, addr)
    return secs


def disasm(elf):
    """-> {fn: [(addr, size, mnemonic, operands)]}"""
    fns, cur = {}, None
    for ln in sh("arm-none-eabi-objdump", "-d", "-C", "-w", elf).splitlines():
        m = FN_RE.match(ln)
        if m:
            cur = m.group(2)
            fns[cur] = []
            continue
        m = INS_RE.match(ln)
        if m and cur:
            size = len(m.group(2).replace(" ", "")) // 2
            fns[cur].append((int(m.group(1), 16), size, m.group(3), m.group(4)))
    return fns


class SourceIndex:
    """Fast regex index over src/**/*.rs to map demangled Rust symbols -> file:line without DWARF."""

    FN_DEF_RE = re.compile(r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:unsafe\s+)?(?:const\s+)?(?:extern\s+\"[^\"]*\"\s+)?fn\s+([A-Za-z_]\w*)")
    IMPL_RE = re.compile(r"^\s*impl(?:<[^>]*>)?\s+(?:[A-Za-z0-9_:<>]+\s+for\s+)?([A-Za-z_]\w*)")
    TYPE_RE = re.compile(r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:struct|enum|static|trait)\s+([A-Za-z_]\w*)")

    def __init__(self, root="src"):
        self.by_fn = collections.defaultdict(list)       # fn_name -> [(rel_path, line, impl_type)]
        self.by_type = collections.defaultdict(list)     # type_name -> [(rel_path, line)]
        if not os.path.isdir(root):
            return
        for dirpath, _, filenames in os.walk(root):
            if "/target" in dirpath:
                continue
            for fn in filenames:
                if not fn.endswith(".rs"):
                    continue
                path = os.path.join(dirpath, fn)
                cur_type = ""
                brace_depth = 0
                impl_depth = -1
                try:
                    with open(path, encoding="utf-8", errors="ignore") as f:
                        for lineno, line in enumerate(f, 1):
                            if impl_depth < 0:
                                m_imp = self.IMPL_RE.match(line)
                                if m_imp and "{" in line:
                                    cur_type = m_imp.group(1)
                                    impl_depth = brace_depth
                            m_fn = self.FN_DEF_RE.match(line)
                            if m_fn:
                                self.by_fn[m_fn.group(1)].append((path, lineno, cur_type))
                            m_ty = self.TYPE_RE.match(line)
                            if m_ty:
                                self.by_type[m_ty.group(1)].append((path, lineno))
                            brace_depth += line.count("{") - line.count("}")
                            if impl_depth >= 0 and brace_depth <= impl_depth:
                                cur_type = ""
                                impl_depth = -1
                except OSError:
                    pass

    @staticmethod
    def _clean_parts(sym):
        s = sym.strip()
        if s.startswith("<"):
            # <crate::mod::Type<...> as Trait>::method::<...>
            m = re.match(r"^<([^<>]+?)(?:<[^>]*>)?(?:\s+as\s+[^>]+)?>(.*)$", s)
            if m:
                s = m.group(1) + m.group(2)
        s = re.sub(r"::<[^>]*>", "", s)
        s = re.sub(r"<[^>]*>", "", s)
        return [p for p in s.split("::") if p and p != "{{closure}}"]

    def locate(self, sym):
        """Return 'src/.../file.rs:line' or '' if unresolved."""
        parts = self._clean_parts(sym)
        if not parts:
            return ""
        crate = parts[0]
        crate_root = CRATE_ROOTS.get(crate, "")
        fn_name = parts[-1]
        if fn_name in ("v_render", "v_on_event", "v_on_start", "v_on_exit") and len(parts) >= 2:
            # AppVTable trampoline: prefer type or app_vtable
            fn_name = parts[-1]

        # Expected path prefix from module segments
        mod_segs = [p for p in parts[1:-1] if p and p[0].islower()]
        expected_prefixes = []
        if crate_root and mod_segs:
            for k in range(len(mod_segs), 0, -1):
                sub = "/".join(mod_segs[:k])
                expected_prefixes.append(f"{crate_root}/{sub}.rs")
                expected_prefixes.append(f"{crate_root}/{sub}/mod.rs")
                expected_prefixes.append(f"{crate_root}/{sub}/")

        type_hint = ""
        for p in reversed(parts[:-1]):
            if p and p[0].isupper():
                type_hint = p
                break

        cands = self.by_fn.get(fn_name, [])
        if cands:
            best = None
            best_score = -1
            for path, line, impl_ty in cands:
                score = 0
                if crate_root and path.startswith(crate_root + "/"):
                    score += 10
                for idx, pref in enumerate(expected_prefixes):
                    if path == pref or path.startswith(pref):
                        score += 40 - idx * 3
                        break
                if type_hint and impl_ty == type_hint:
                    score += 20
                if "/tests" in path or path.endswith("_test.rs"):
                    score -= 15
                if score > best_score:
                    best_score = score
                    best = (path, line)
            if best and best_score >= 10:
                return f"{best[0]}:{best[1]}"

        # Fallback: locate static / struct / type
        for cand_name in reversed(parts):
            if cand_name in self.by_type:
                for path, line in self.by_type[cand_name]:
                    if not crate_root or path.startswith(crate_root + "/"):
                        return f"{path}:{line}"
        if expected_prefixes:
            for pref in expected_prefixes:
                if os.path.isfile(pref):
                    return pref
        return ""


_SRC_IDX = None


def src_index():
    global _SRC_IDX
    if _SRC_IDX is None:
        _SRC_IDX = SourceIndex("src")
    return _SRC_IDX


def loc_str(sym):
    loc = src_index().locate(sym)
    return f" @ {loc}" if loc else ""


def is_actionable_sym(sym):
    return not sym.startswith(UNACTIONABLE_PREFIXES)


def norm(ins, relax_regs=False):
    """Normalize away addresses, call targets, literal offsets; optionally abstract Thumb registers."""
    _, _, mn, op = ins
    op = re.sub(r"<[^>]*>", "<T>", op)
    op = re.sub(r";.*$", "", op)
    op = re.sub(r"\b[0-9a-f]{5,}\b", "A", op)
    op = re.sub(r"\[pc, #\d+\]", "[pc,L]", op)
    if relax_regs:
        op = re.sub(r"\br[0-7]\b", "rL", op)
        op = re.sub(r"\br(?:[89]|1[0-2])\b", "rH", op)
    return mn + " " + op.strip()


def fbytes(body):
    return sum(i[1] for i in body)


def base_name(n):
    s = re.sub(r"::<.*>$", "", n)
    return re.sub(r"<([^<>]*?) as [^<>]*>", r"\1", s)


def short_fn_name(sym):
    return base_name(sym).split("::")[-1]


def rodata_stats(syms, secs):
    sz, ro = secs.get(".rodata", (0, 0))
    end = ro + sz
    named = sum(v[0] for v in syms.values() if v[1] in "rR" and ro <= v[2] < end)
    return sz, ro, named, max(0, sz - named)


def analyze_elf(elf):
    """Collect shared ELF telemetry once for audit, tasks, suggest, and JSON export."""
    syms = symbols(elf)
    secs = elf_sections(elf)
    fns = disasm(elf)

    text_sz = secs.get(".text", (0, 0))[0]
    rodata_sz, ro_addr, named_rodata, anon_rodata = rodata_stats(syms, secs)
    data_sz = secs.get(".data", (0, 0))[0]
    bss_sz = secs.get(".bss", (0, 0))[0]
    flash_used = text_sz + rodata_sz + data_sz

    # Call graph & __aeabi_ counters
    callers = collections.defaultdict(set)
    aeabi_calls = collections.defaultdict(lambda: collections.defaultdict(int))
    blx_sites = collections.defaultdict(int)
    for f, body in fns.items():
        for _, _, mn, op in body:
            if mn == "blx" and re.match(r"^r\d+$", op.strip()):
                blx_sites[f] += 1
            if mn in ("bl", "blx"):
                m = re.search(r"<([^>+]+)", op)
                if m:
                    tgt = m.group(1)
                    callers[tgt].add(f)
                    if tgt.startswith("__aeabi_"):
                        aeabi_calls[tgt][f] += 1

    # Stack frame sizes
    stack_hogs = []
    for f, body in fns.items():
        sp_sub = 0
        for _, _, mn, op in body[:15]:
            if mn in ("sub", "subs") and "sp" in op:
                m = re.search(r"#(\d+)", op)
                if m:
                    sp_sub += int(m.group(1))
        if sp_sub >= 96 and is_actionable_sym(f):
            stack_hogs.append((sp_sub, f))
    stack_hogs.sort(reverse=True)

    # Imm5 struct offset penalties
    imm_penalties = []
    for f, body in fns.items():
        if not is_actionable_sym(f):
            continue
        count = 0
        details = collections.defaultdict(int)
        for i in range(len(body) - 1):
            _, _, mn1, op1 = body[i]
            _, _, mn2, _ = body[i + 1]
            m1 = re.search(r"movs\s+r\d+,\s*#(\d+)", mn1 + " " + op1)
            if m1:
                imm = int(m1.group(1))
                if ("ldrb" in mn2 or "strb" in mn2) and imm > 31:
                    count += 1; details[f"byte@{imm}"] += 1
                elif ("ldrh" in mn2 or "strh" in mn2) and imm > 62:
                    count += 1; details[f"half@{imm}"] += 1
                elif ("ldr" in mn2 or "str" in mn2) and imm > 124:
                    count += 1; details[f"word@{imm}"] += 1
        if count > 0:
            top_off = sorted(details.items(), key=lambda kv: -kv[1])[:3]
            imm_penalties.append((count, f, top_off))
    imm_penalties.sort(reverse=True)

    # Monolith functions
    monoliths = []
    for f, (sz, t, _) in syms.items():
        if t in "tT" and sz >= 600 and is_actionable_sym(f):
            monoliths.append((sz, len(callers.get(f, ())), f))
    monoliths.sort(reverse=True)

    # Syscalls
    syscall_syms = {k: v[0] for k, v in syms.items() if "runtime::syscall" in k and v[1] in "tT"}

    # Anonymous rodata gaps
    ro_end = ro_addr + rodata_sz
    named_ranges = sorted((v[2], v[0], k) for k, v in syms.items() if v[1] in "rR" and ro_addr <= v[2] < ro_end)
    gaps, cur = [], ro_addr
    for ad, s, _ in named_ranges:
        if ad > cur:
            gaps.append((cur, ad - cur))
        cur = max(cur, ad + s)
    if cur < ro_end:
        gaps.append((cur, ro_end - cur))
    ro_refs = collections.defaultdict(set)
    for f, body in fns.items():
        for _, _, mn, op in body:
            if mn == ".word" and op:
                v = int(op.split()[0], 16)
                if ro_addr <= v < ro_end:
                    ro_refs[v].add(f)

    return {
        "syms": syms, "secs": secs, "fns": fns, "callers": callers,
        "flash_used": flash_used, "text_sz": text_sz, "rodata_sz": rodata_sz,
        "ro_addr": ro_addr, "named_rodata": named_rodata, "anon_rodata": anon_rodata,
        "data_sz": data_sz, "bss_sz": bss_sz,
        "aeabi_calls": aeabi_calls, "blx_sites": blx_sites,
        "stack_hogs": stack_hogs, "imm_penalties": imm_penalties,
        "monoliths": monoliths, "syscall_syms": syscall_syms,
        "gaps": gaps, "ro_refs": ro_refs,
    }


def build_task_cards(info):
    """Build ranked, self-contained refactor task cards tailored for automated execution."""
    syms = info["syms"]
    fns = info["fns"]
    aeabi = info["aeabi_calls"]
    tasks = []

    # 1. Syscall shim clusters > 48 B
    sc_groups = collections.defaultdict(list)
    for fn, sz in info["syscall_syms"].items():
        if sz > 48:
            mod = fn.split("::")[3] if len(fn.split("::")) > 4 else "core"
            sc_groups[mod].append((sz, fn))
    for mod, items in sc_groups.items():
        items.sort(reverse=True)
        tot = sum(s for s, _ in items)
        est = sum(max(0, s - 24) for s, _ in items)
        if est >= 120:
            top_fn = items[0][1]
            tasks.append({
                "est": est,
                "pattern": "SYSCALL-THIN",
                "title": f"Thin {len(items)} shims in runtime::syscall::{mod} ({tot} B -> ~{tot - est} B)",
                "target": f"{top_fn}{loc_str(top_fn)}",
                "items": [f"{fn.split('::')[-1]} ({sz} B){loc_str(fn)}" for sz, fn in items[:5]],
                "why": "Each shim exceeds the 24 B Phase-10 ceiling due to inline active_ctx_opt() guards, by-value struct copies, or inlined gfx/parse calls.",
                "steps": [
                    "Write output fields directly through `*mut Out` / `MaybeUninit` instead of constructing a local struct and calling `copy_nonoverlapping`.",
                    "Factor repeated `(active_ctx_opt() + slice/path parse)` prologues into one shared `#[inline(never)]` helper in `syscall/mod.rs` or `syscall/{mod}.rs`.",
                    "Delegate multi-step UI/radio logic to one `#[inline(never)]` `SysContext` primitive so the C-ABI shim is <= 24 B.",
                ],
                "verify": "./kern check && ./kern build && python3 tools/bloat.py syscalls --top 15",
            })

    # 2. Anonymous .rodata gaps (>= 250 B)
    raw_ro = subprocess.run(
        ["arm-none-eabi-objcopy", "-O", "binary", "-j", ".rodata", info.get("_elf", DEF_ELF), "/dev/stdout"],
        capture_output=True,
    ).stdout
    ro_addr = info["ro_addr"]
    for g, gs in sorted(info["gaps"], key=lambda x: -x[1])[:8]:
        if gs < 250:
            continue
        who = collections.Counter()
        for v, fs in info["ro_refs"].items():
            if g <= v < g + gs:
                who.update(fs)
        if not who:
            continue
        top_fn, _ = who.most_common(1)[0]
        if not is_actionable_sym(top_fn):
            continue
        b = raw_ro[g - ro_addr:g - ro_addr + gs]
        pr = sum(32 <= x < 127 for x in b) / max(1, len(b))
        kind = "strings/labels" if pr > 0.65 else "const tables/bitmaps"
        tasks.append({
            "est": gs // 2,
            "pattern": "ANON-RODATA",
            "title": f"Collapse {gs} B anonymous .rodata ({kind}) referenced by {short_fn_name(top_fn)}",
            "target": f"{top_fn}{loc_str(top_fn)}",
            "items": [f"{fn}{loc_str(fn)}" for fn, _ in who.most_common(3)],
            "why": f"Match arms returning `&'static str` or unnamed arrays emit 8 B fat pointers + unshared literals in .rodata (+{g - ro_addr}).",
            "steps": [
                "If matching an enum/index to strings: pack all strings into one NUL-delimited `const STRINGS: &[u8] = b\"...\\0\";` and look up with `utils::format::nth_cstr(STRINGS, idx)`.",
                "If using repeated UI geometry/layout branches: convert `match` arms into a `const &[RowDesc]` slice and one shared loop.",
                "Name any large static bitmap/table explicitly (`static FOO: ...`) or move non-boot assets to KernFS.",
            ],
            "verify": "./kern check && ./kern build && python3 tools/bloat.py rodata --top 10",
        })

    # 3. mem* / return-by-value struct callers
    mem_tot = collections.Counter()
    for t, c in aeabi.items():
        if "mem" in t:
            for f, n in c.items():
                if is_actionable_sym(f):
                    mem_tot[f] += n
    for f, n in mem_tot.most_common(10):
        if n < 2:
            continue
        sz = syms.get(f, (0,))[0]
        tasks.append({
            "est": n * 16 + (sz // 8),
            "pattern": "BY-REF",
            "title": f"Eliminate {n}x __aeabi_mem* calls in {short_fn_name(f)} ({sz} B)",
            "target": f"{f}{loc_str(f)}",
            "items": [f"{t}: {c[f]}x" for t, c in aeabi.items() if "mem" in t and f in c],
            "why": "Returning or assigning large structs by value or zeroing large stack structs emits __aeabi_memcpy/__aeabi_memclr4 calls plus stack spills.",
            "steps": [
                "Replace `let mut s = BigStruct::new()` + copy with `out: &mut MaybeUninit<BigStruct>` (`init_in_place`) or field-by-field writes into `&mut out`.",
                "Pass large read-only descriptors by `&T` instead of `T` by value.",
                "Avoid re-zeroing large buffers every call; initialize only valid prefix bytes (`buf[..len]`).",
            ],
            "verify": "./kern check && ./kern build && python3 tools/bloat.py memcpy --top 10",
        })

    # 4. Hot struct imm5 offset penalties (>= 4 penalties)
    for cnt, f, top_off in info["imm_penalties"][:10]:
        if cnt < 4:
            break
        offs_str = ", ".join(f"{k}x{v}" for k, v in top_off)
        tasks.append({
            "est": cnt * 6,
            "pattern": "HOT-STRUCT-ORDER",
            "title": f"Fix {cnt} Thumb-1 imm5 offset penalties in {short_fn_name(f)} [{offs_str}]",
            "target": f"{f}{loc_str(f)}",
            "items": [offs_str],
            "why": "Cortex-M0+ Thumb-1 only encodes 1-instruction `ldrb`/`strb` at offset <= 31, `ldrh` <= 62, `ldr` <= 124; larger offsets force extra `movs rX, #off` instructions.",
            "steps": [
                "Inspect the primary `#[repr(C)]` struct accessed in this function.",
                "Move frequently accessed `u8`/`bool` fields into bytes `0..31`, `u16` fields into `0..62`, and hot `u32` fields into `0..124` (keep large `[u8; N]` arrays at the end).",
                "Verify no wire/flash binary format contract is broken before reordering (only reorder RAM-only state structs or versioned internal structs).",
            ],
            "verify": "./kern check && ./kern build && python3 tools/bloat.py fields --top 10",
        })

    # 5. Stack hogs (>= 140 B)
    for sp, f in info["stack_hogs"][:8]:
        if sp < 140:
            break
        tasks.append({
            "est": 40 + sp // 4,
            "pattern": "STACK-ARENA",
            "title": f"Shrink {sp} B stack frame in {short_fn_name(f)} (SRAM safety + code size)",
            "target": f"{f}{loc_str(f)}",
            "items": [f"{sp} B allocated on MSP stack"],
            "why": "Large stack buffers risk overflowing 16 KB SRAM and force __aeabi_memclr4/movs sp prologue overhead.",
            "steps": [
                "Move >= 64 B temporary buffers out of locals into `crate::data::arena::staging_buf()` or a smaller streaming chunk loop (32–64 B).",
                "For `KernFS` / `WireDriver` helpers, reuse `self.page_buf` or a caller-provided `&mut [u8]` scratch slice instead of stack-allocating a 256 B array.",
            ],
            "verify": "./kern check && ./kern build && python3 tools/bloat.py stack --top 10",
        })

    # 6. Monoliths > 700 B
    for sz, callers_cnt, f in info["monoliths"][:10]:
        if sz < 700:
            break
        tasks.append({
            "est": sz // 4,
            "pattern": "SPLIT-1K",
            "title": f"Split/table-drive {sz} B monolith {short_fn_name(f)} (callers={callers_cnt})",
            "target": f"{f}{loc_str(f)}",
            "items": [f"{sz} B .text (> +1020 B ldr literal reach causes pool duplication)" if sz > 1020 else f"{sz} B .text"],
            "why": "Large monolithic functions exceed Cortex-M0+ +1020 B PC-relative literal pool reach and inline cold error/UI branches into hot code.",
            "steps": [
                "Extract cold error, confirmation, or one-shot setup branches into `#[inline(never)]` helpers.",
                "Replace repetitive `match` arms (e.g., menu items, command opcodes, widget layouts) with a `const` descriptor table + shared loop.",
            ],
            "verify": "./kern check && ./kern build && python3 tools/bloat.py calls --top 15",
        })

    # 7. Indirect `blx rN` dyn Trait calls in drivers/hot paths
    for f, cnt in sorted(info["blx_sites"].items(), key=lambda kv: -kv[1])[:8]:
        if cnt < 2 or not is_actionable_sym(f):
            continue
        if not (f.startswith("drivers::") or "st7565" in f or "py25q16" in f or "services::" in f):
            continue
        tasks.append({
            "est": cnt * 14,
            "pattern": "DYN-TO-STATIC",
            "title": f"Replace {cnt}x indirect `blx` vtable calls in {short_fn_name(f)} with static dispatch",
            "target": f"{f}{loc_str(f)}",
            "items": [f"{cnt} indirect register calls (`blx rN`)"],
            "why": "`&mut dyn SpiDevice` / `&mut dyn OutputPin` forces indirect vtable calls per byte/pin toggle and emits `.rodata` vtables.",
            "steps": [
                "Parameterize the driver struct over static trait bounds (e.g., `St7565<SPI, CS, A0>` / `Py25q16FlashDriver<SPI, CS>`) or bind concrete BSP types.",
                "Add a slice-level `write_bytes(&mut self, data: &[u8])` method so LCD/flash transfers loop inside HAL/SPI instead of dispatching per byte.",
            ],
            "verify": "./kern check && ./kern build && python3 tools/bloat.py vtables --top 15",
        })

    # 8. Software division & unaligned traps
    for t in ("__aeabi_uidiv", "__aeabi_uidivmod", "__aeabi_idiv", "__aeabi_idivmod"):
        for f, n in aeabi.get(t, {}).items():
            if is_actionable_sym(f) and "api_math_" not in f and n >= 1:
                tasks.append({
                    "est": n * 24,
                    "pattern": "RECIPROCAL-DIV",
                    "title": f"Replace {n}x software division (`{t}`) in {short_fn_name(f)}",
                    "target": f"{f}{loc_str(f)}",
                    "items": [f"{n}x {t}"],
                    "why": "Cortex-M0+ has no hardware divider; `/` and `%` emit library calls + 4-register clobbers.",
                    "steps": [
                        "Use `utils::math::div10_u16` / `div10_u32` for base-10 formatting, `(x * K) >> S` reciprocal multiply for fixed denominators (e.g. * 100 / max), or `& (N - 1)` for powers of two.",
                    ],
                    "verify": "./kern check && ./kern build && python3 tools/bloat.py divs",
                })
    for t in ("__aeabi_uread4", "__aeabi_uwrite4", "__aeabi_uread8", "__aeabi_uwrite8"):
        for f, n in aeabi.get(t, {}).items():
            if is_actionable_sym(f):
                tasks.append({
                    "est": n * 18,
                    "pattern": "ALIGN-RECORD",
                    "title": f"Fix {n}x unaligned trap (`{t}`) in {short_fn_name(f)}",
                    "target": f"{f}{loc_str(f)}",
                    "items": [f"{n}x {t}"],
                    "why": "Cortex-M0+ faults on unaligned 16/32-bit loads, so `#[repr(C, packed)]` emits multi-byte helper calls.",
                    "steps": [
                        "Replace `#[repr(C, packed)]` with naturally aligned `#[repr(C)]` fields, or decode unaligned wire bytes explicitly via `u32::from_le_bytes`.",
                    ],
                    "verify": "./kern check && ./kern build && python3 tools/bloat.py unaligned",
                })

    # 9. Repeated SRAM literal reloads (>= 12)
    for f, body in fns.items():
        if not is_actionable_sym(f):
            continue
        w = collections.Counter(op.split()[0] for _, _, mn, op in body if mn == ".word" and op)
        dup = sum(n for v, n in w.items() if v.startswith("0x200"))
        if dup >= 10:
            tasks.append({
                "est": dup * 4,
                "pattern": "GLOBAL-ANCHOR",
                "title": f"Consolidate {dup} SRAM static literal loads in {short_fn_name(f)}",
                "target": f"{f}{loc_str(f)}",
                "items": [f"{dup} .word 0x200xxxxx pool entries"],
                "why": "Accessing multiple separate `static mut` globals reloads multiple 4 B literal pool addresses.",
                "steps": [
                    "Pass `&mut Ctx` / `&mut Canvas` / `&mut MasterConfig` as a single parameter down the call tree instead of re-fetching global pointers.",
                ],
                "verify": "./kern check && ./kern build && python3 tools/bloat.py pools --top 12",
            })

    # 10. Multi-instance generic bloat (>= 2 instances, >= 120 B)
    g = collections.defaultdict(list)
    for n, (sz, t, _) in syms.items():
        if t in "tT" and "::<" in n and is_actionable_sym(n):
            g[base_name(n)].append((sz, n))
    for base, insts in g.items():
        if len(insts) >= 2:
            tot = sum(s for s, _ in insts)
            est = tot - max(s for s, _ in insts)
            if est >= 100:
                tasks.append({
                    "est": est,
                    "pattern": "DE-MONO",
                    "title": f"De-monomorphize {len(insts)}x instances of {base.split('::')[-1]} ({tot} B)",
                    "target": f"{insts[0][1]}{loc_str(insts[0][1])}",
                    "items": [f"{s} B: {n[:80]}" for s, n in sorted(insts, reverse=True)[:3]],
                    "why": "Multiple monomorphizations duplicate identical machine code in Flash.",
                    "steps": [
                        "Move the type-independent body into one non-generic `#[inline(never)] fn ..._inner(...)` helper and keep only thin typed wrappers.",
                    ],
                    "verify": "./kern check && ./kern build && python3 tools/bloat.py mono --top 12",
                })

    tasks.sort(key=lambda t: -t["est"])
    return tasks


def cmd_tasks(a):
    """Print ranked, self-contained optimization task cards for automated/agent execution."""
    info = analyze_elf(a.elf)
    info["_elf"] = a.elf
    tasks = build_task_cards(info)
    if getattr(a, "pattern", ""):
        pats = [p.strip().upper() for p in re.split(r"[|,]", a.pattern) if p.strip()]
        tasks = [t for t in tasks if any(p in t["pattern"] for p in pats)]
    if getattr(a, "task", 0):
        idx = a.task - 1
        tasks = [tasks[idx]] if 0 <= idx < len(tasks) else []

    if a.json:
        with open(a.json, "w") as jf:
            json.dump({"flash_used": info["flash_used"], "flash_max": FLASH_MAX, "tasks": tasks[:a.top]}, jf, indent=2)
        print(f"Saved {len(tasks[:a.top])} task cards to {a.json}")
        return

    free_b = FLASH_MAX - info["flash_used"]
    target_80 = int(FLASH_MAX * 0.80)
    print("══════════════════════════════════════════════════════════════════════════")
    print(" 🤖 KERN AUTOPILOT REFACTOR PLAYBOOK (Sorted by Estimated Net Flash ROI)")
    print("══════════════════════════════════════════════════════════════════════════")
    print(f" Current Flash: {info['flash_used']} / {FLASH_MAX} B ({info['flash_used']*100/FLASH_MAX:.1f}% | {free_b} B free | {info['flash_used'] - target_80:+d} B to 80%)")
    print(" Protocol for each task:")
    print("   1. Save baseline:  python3 tools/bloat.py baseline --out /tmp/pre.json")
    print("   2. Apply ONE task below at the exact file:line (keep 0 warnings, no alloc, no behavior loss).")
    print("   3. Verify & keep:  ./kern check && ./kern test && ./kern build && python3 tools/bloat.py diff --base /tmp/pre.json")
    print("──────────────────────────────────────────────────────────────────────────")
    for idx, t in enumerate(tasks[:a.top], 1):
        print(f"\n[TASK #{idx:02d}] ~{t['est']:4d} B est. saving | [{t['pattern']}] {t['title']}")
        print(f"  Target: {t['target']}")
        if t["items"]:
            print(f"  Scope:  " + " ; ".join(t["items"][:4]))
        print(f"  Why:    {t['why']}")
        for s_i, step in enumerate(t["steps"], 1):
            print(f"  Step {s_i}: {step}")
        print(f"  Verify: {t['verify']}")


def cmd_audit(a):
    """Generate comprehensive architectural bloat diagnosis and dynamically ranked actions."""
    info = analyze_elf(a.elf)
    info["_elf"] = a.elf
    flash_used = info["flash_used"]
    free_headroom = FLASH_MAX - flash_used
    util_pct = (flash_used / FLASH_MAX) * 100.0
    aeabi_calls = info["aeabi_calls"]
    total_aeabi_calls = sum(sum(c.values()) for c in aeabi_calls.values())
    syscall_total = sum(info["syscall_syms"].values())
    syscall_avg = (syscall_total / len(info["syscall_syms"])) if info["syscall_syms"] else 0.0
    blx_total = sum(info["blx_sites"].values())

    print("══════════════════════════════════════════════════════════════════")
    print(" ⚡ KERN ARCHITECTURAL FLASH AUDIT & HEALTH REPORT")
    print("══════════════════════════════════════════════════════════════════")
    print(f" Flash Usage:     {flash_used:7d} / {FLASH_MAX} B  ({util_pct:.1f}% util | {free_headroom} B free)")
    print(f"  • .text:        {info['text_sz']:7d} B  ({(info['text_sz']/flash_used)*100:.1f}%)")
    print(f"  • .rodata:      {info['rodata_sz']:7d} B  ({(info['rodata_sz']/flash_used)*100:.1f}%) [Named: {info['named_rodata']} B | Anonymous: {info['anon_rodata']} B ({info['anon_rodata']*100/max(1,info['rodata_sz']):.1f}%)]")
    print(f"  • SRAM (.bss):  {info['bss_sz']:7d} B | (.data): {info['data_sz']} B")
    print("──────────────────────────────────────────────────────────────────")
    print(" 🚨 ARCHITECTURAL BLOAT INDICATORS")
    print("──────────────────────────────────────────────────────────────────")
    print(f" 1. Software Emulation:  {total_aeabi_calls:4d} calls across {len(aeabi_calls)} __aeabi functions (~{total_aeabi_calls * 14} B code tax)")
    for t, c in sorted(aeabi_calls.items(), key=lambda kv: -sum(kv[1].values()))[:4]:
        print(f"     - {t:<18} {sum(c.values()):3d} call sites ({len(c)} caller fns)")
    print(f" 2. Syscall Shim Bloat:  {len(info['syscall_syms']):4d} shims = {syscall_total:5d} B (avg {syscall_avg:.1f} B/shim, target <= 24 B)")
    top_stack = info["stack_hogs"][0][0] if info["stack_hogs"] else 0
    print(f" 3. Stack Frame Hogs:    {len(info['stack_hogs']):4d} fns allocate >= 96 B stack (max {top_stack} B)")
    print(f" 4. Imm5 Offset Penalty: {len(info['imm_penalties']):4d} fns suffer from struct fields > 31/124 B ({sum(c for c, _, _ in info['imm_penalties'])} total)")
    top_mono = info["monoliths"][0][0] if info["monoliths"] else 0
    print(f" 5. Monolith Functions:  {len(info['monoliths']):4d} fns >= 600 B (top: {top_mono} B)")
    print(f" 6. Indirect `blx` Calls:{blx_total:4d} register calls across {len(info['blx_sites'])} fns")
    print("──────────────────────────────────────────────────────────────────")
    print(" 🎯 TOP DYNAMIC REFACTOR TARGETS (Run `tools/bloat.py tasks` for step-by-step recipes)")
    print("──────────────────────────────────────────────────────────────────")
    tasks = build_task_cards(info)
    for idx, t in enumerate(tasks[:8], 1):
        print(f" [{idx}] ~{t['est']:4d} B  [{t['pattern']}] {t['title']}")
        print(f"     -> {t['target']}")
    print("══════════════════════════════════════════════════════════════════")

    if a.json:
        report_data = {
            "flash_used": flash_used, "flash_max": FLASH_MAX, "util_pct": util_pct,
            "text_sz": info["text_sz"], "rodata_sz": info["rodata_sz"],
            "named_rodata": info["named_rodata"], "anon_rodata": info["anon_rodata"],
            "total_aeabi_calls": total_aeabi_calls,
            "aeabi_breakdown": {t: sum(c.values()) for t, c in aeabi_calls.items()},
            "syscall_total": syscall_total, "syscall_count": len(info["syscall_syms"]), "syscall_avg": syscall_avg,
            "stack_hogs_count": len(info["stack_hogs"]), "top_stack_hogs": info["stack_hogs"][:10],
            "imm5_penalties_count": len(info["imm_penalties"]),
            "monoliths_count": len(info["monoliths"]),
            "top_tasks": tasks[:10],
        }
        with open(a.json, "w") as jf:
            json.dump(report_data, jf, indent=2)
        print(f"Saved audit JSON to {a.json}")


def cmd_stack(a):
    """List functions allocating large stack frames (risk of stack overflow & code bloat)."""
    fns = disasm(a.elf)
    stack_usage = []
    for f, body in fns.items():
        sp_sub = 0
        for _, _, mn, op in body[:15]:
            if mn in ("sub", "subs") and "sp" in op:
                m = re.search(r"#(\d+)", op)
                if m:
                    sp_sub += int(m.group(1))
        if sp_sub >= a.min:
            stack_usage.append((sp_sub, f))
    stack_usage.sort(reverse=True)
    print(f"== stack allocation audit: {len(stack_usage)} functions allocating >= {a.min} bytes ==")
    print("   (Move large buffers to TRANSIENT_ARENA / staging_buf() or stream in small chunks!)")
    for sz, f in stack_usage[: a.top]:
        print(f"{sz:5d} B stack  {f}{loc_str(f)}")


def cmd_fields(a):
    """List functions suffering from Thumb-1 imm5 struct field offset penalties."""
    fns = disasm(a.elf)
    imm_penalties = []
    for f, body in fns.items():
        count = 0
        details = collections.defaultdict(int)
        for i in range(len(body) - 1):
            _, _, mn1, op1 = body[i]
            _, _, mn2, _ = body[i + 1]
            m1 = re.search(r"movs\s+r\d+,\s*#(\d+)", mn1 + " " + op1)
            if m1:
                imm = int(m1.group(1))
                if ("ldrb" in mn2 or "strb" in mn2) and imm > 31:
                    count += 1; details[f"byte@{imm}"] += 1
                elif ("ldrh" in mn2 or "strh" in mn2) and imm > 62:
                    count += 1; details[f"half@{imm}"] += 1
                elif ("ldr" in mn2 or "str" in mn2) and imm > 124:
                    count += 1; details[f"word@{imm}"] += 1
        if count > 0:
            top_off = sorted(details.items(), key=lambda kv: -kv[1])[:3]
            imm_penalties.append((count, f, top_off))
    imm_penalties.sort(reverse=True)
    print(f"== Cortex-M0+ imm5 struct offset penalties ({len(imm_penalties)} functions) ==")
    print("   (Order structs with hot u8/u16 in first 32/64 bytes to allow 1-insn ldrb/ldrh!)")
    for c, f, top_off in imm_penalties[: a.top]:
        offs_str = ", ".join(f"{k}x{v}" for k, v in top_off)
        print(f"{c:3d} penalties  {f}{loc_str(f)}  [{offs_str}]")


def cmd_dups(a):
    fns = disasm(a.elf)
    groups = collections.defaultdict(list)
    relaxed_groups = collections.defaultdict(list)
    for f, b in fns.items():
        if fbytes(b) >= a.min and is_actionable_sym(f):
            groups[hashlib.sha1("\n".join(norm(i, False) for i in b).encode()).hexdigest()].append(f)
            relaxed_groups[hashlib.sha1("\n".join(norm(i, True) for i in b).encode()).hexdigest()].append(f)
    print("== exact (normalized) duplicates: keep one, route others to it ==")
    rows = [(fbytes(fns[g[0]]) * (len(g) - 1), g) for g in groups.values() if len(g) > 1]
    for save, g in sorted(rows, reverse=True)[: a.top]:
        print(f"{save:6d} B  x{len(g)}  " + " | ".join(f"{x}{loc_str(x)}" for x in g))
    print("\n== register-normalized duplicates (identical shape, differing only in register allocation) ==")
    exact_sets = {frozenset(g) for g in groups.values() if len(g) > 1}
    rrows = [(fbytes(fns[g[0]]) * (len(g) - 1), g) for g in relaxed_groups.values() if len(g) > 1 and frozenset(g) not in exact_sets]
    for save, g in sorted(rrows, reverse=True)[: a.top]:
        print(f"{save:6d} B  x{len(g)}  " + " | ".join(f"{x}{loc_str(x)}" for x in g))
    print("\n== near duplicates (shingle Jaccard >= %.2f): extract shared core ==" % a.sim)
    cand = [(f, set(zip(*[[norm(i, True) for i in b][k:] for k in range(4)])))
            for f, b in fns.items() if fbytes(b) >= max(a.min, 48) and is_actionable_sym(f)]
    res = []
    for i in range(len(cand)):
        for j in range(i + 1, len(cand)):
            x, y = cand[i][1], cand[j][1]
            if not x or not y or min(len(x), len(y)) / max(len(x), len(y)) < a.sim:
                continue
            s = len(x & y) / len(x | y)
            if s >= a.sim:
                res.append((min(fbytes(fns[cand[i][0]]), fbytes(fns[cand[j][0]])), s, cand[i][0], cand[j][0]))
    for sz, s, f1, f2 in sorted(res, reverse=True)[: a.top]:
        print(f"{sz:6d} B  sim={s:.2f}  {f1}{loc_str(f1)}  ~  {f2}{loc_str(f2)}")


def cmd_outline(a):
    fns = disasm(a.elf)
    seqs = collections.defaultdict(set)
    cost = {}
    for f, b in fns.items():
        if not is_actionable_sym(f):
            continue
        n = [norm(i) for i in b]
        for k in range(len(n) - a.len + 1):
            win = tuple(n[k:k + a.len])
            if any(w.startswith(("b ", "bx", "pop", "push")) for w in win[1:-1]):
                continue
            seqs[win].add(f)
            cost[win] = sum(i[1] for i in b[k:k + a.len])
    rows = [((len(fs) - 1) * cost[w] - len(fs) * 4, w, fs) for w, fs in seqs.items() if len(fs) >= a.count]
    rows.sort(key=lambda r: -r[0])
    seen = set()
    print(f"== repeated {a.len}-instruction sequences (est. saving if outlined into one helper) ==")
    for save, w, fs in rows:
        key = frozenset(fs)
        if save <= 0 or key in seen:
            continue
        seen.add(key)
        shown = ", ".join(f"{f}{loc_str(f)}" for f in sorted(fs)[:4])
        print(f"{save:5d} B  in {len(fs)} fns: {shown}{' ...' if len(fs) > 4 else ''}")
        print("        " + " ; ".join(w))
        if len(seen) >= a.top:
            break


def cmd_mono(a):
    syms = symbols(a.elf)
    g = collections.defaultdict(list)
    for n, (sz, t, _) in syms.items():
        if t in "tT" and "::<" in n and is_actionable_sym(n):
            g[base_name(n)].append((sz, n))
    rows = sorted(((sum(s for s, _ in v), k, v) for k, v in g.items()), reverse=True)
    print("== generic families (total bytes, instances). x2+ = highest-ROI de-mono targets ==")
    for tot, k, v in rows[: a.top]:
        print(f"{tot:6d} B  x{len(v)}  {k}{loc_str(v[0][1])}")
        for sz, n in sorted(v, reverse=True)[:3]:
            print(f"           {sz:5d}  {n[len(k):][:110]}")


def cmd_calls(a):
    fns = disasm(a.elf)
    syms = symbols(a.elf)
    fan = collections.Counter()
    callers = collections.defaultdict(set)
    for f, b in fns.items():
        for i in b:
            if i[2] in ("bl", "blx"):
                m = re.search(r"<(.+?)(\+0x[0-9a-f]+)?>", i[3])
                if m:
                    fan[m.group(1)] += 1
                    callers[m.group(1)].add(f)
    print("== hot callees: call sites x ~6 B arg setup. Fold args into a descriptor or coalesce calls ==")
    for t, c in fan.most_common(a.top):
        print(f"{c:4d} sites  ~{c * 6:5d} B setup  {t}{loc_str(t)}")
    print("\n== large single-caller / entry monoliths (split cold paths or replace match with const table) ==")
    for f, (sz, t, _) in sorted(syms.items(), key=lambda kv: -kv[1][0]):
        if t in "tT" and sz >= a.min and len(callers.get(f, ())) <= 1 and is_actionable_sym(f):
            print(f"{sz:6d} B  callers={sorted(callers.get(f, {'<vtable/entry>'}))[:1]}  {f}{loc_str(f)}")


def cmd_vtables(a):
    """Find indirect `blx rN` register calls (dyn Trait vtable or fn-ptr dispatch)."""
    fns = disasm(a.elf)
    sites = []
    for f, body in fns.items():
        if not is_actionable_sym(f):
            continue
        cnt = sum(1 for _, _, mn, op in body if mn == "blx" and re.match(r"^r\d+$", op.strip()))
        if cnt:
            sites.append((cnt, fbytes(body), f))
    sites.sort(reverse=True)
    print(f"== indirect `blx rN` dispatch sites: {sum(c for c, _, _ in sites)} calls in {len(sites)} fns ==")
    print("   (Replace `&mut dyn Trait` in hot driver/display loops with static generics + slice methods)")
    for cnt, sz, f in sites[: a.top]:
        print(f"{cnt:3d} blx  {sz:5d} B  {f}{loc_str(f)}")


def cmd_consts(a):
    fns = disasm(a.elf)
    use = collections.defaultdict(set)
    for f, b in fns.items():
        if not is_actionable_sym(f):
            continue
        for i in b:
            if i[2] == ".word":
                v = i[3].split()[0]
                if not v.startswith("0x08") and not v.startswith("0x20"):
                    use[v].add(f)
            m = re.search(r"#(\d{3,})", i[3])
            if m and i[2] in ("movs", "cmp", "adds", "subs"):
                use["#" + m.group(1)].add(f)
    print("== constants repeated across functions -> name them once (enum/const table) ==")
    for v, fs in sorted(use.items(), key=lambda kv: -len(kv[1]))[: a.top]:
        if len(fs) >= 3:
            print(f"{len(fs):3d} fns  {v:>12}  {', '.join(sorted(fs)[:3])}")


def is_likely_text(s):
    """Filter out binary bitmap/font bytes accidentally matched as ASCII in .rodata."""
    if len(s) < 4:
        return False
    # Reject repetitive 1-2 char binary patterns like '222ddd', 'KKKxxx'
    if len(set(s)) <= 2 and len(s) >= 5:
        return False
    alpha_or_sep = sum(c.isalnum() or c in " /._:-()[]" for c in s)
    letters = sum(c.isalpha() for c in s)
    return letters >= 2 and (alpha_or_sep / len(s)) >= 0.80


def cmd_strings(a):
    raw = subprocess.run(["arm-none-eabi-objcopy", "-O", "binary", "-j", ".rodata", a.elf, "/dev/stdout"], capture_output=True).stdout
    strs = [m.group().decode() for m in re.finditer(rb"[\x20-\x7e]{4,}", raw)]
    strs = [s for s in strs if is_likely_text(s)]
    c = collections.Counter(strs)
    print(f"== rodata text runs (bitmap noise filtered): {len(strs)}, {sum(map(len, strs))} B ==")
    print("-- exact duplicates --")
    for s, n in c.most_common(a.top):
        if n > 1:
            print(f"x{n}  {s!r}")
    print("-- prefix/suffix sharing candidates --")
    uniq = sorted(set(strs), key=len)
    k = 0
    for i, s in enumerate(uniq):
        for t in uniq[i + 1:]:
            if t.endswith(s) or t.startswith(s):
                print(f"{len(s):3d} B  {s!r}  in  {t!r}")
                k += 1
                break
        if k >= a.top:
            break
    print("-- longest text runs (split/shorten or move to KernFS / nth_cstr) --")
    for s in sorted(set(strs), key=len, reverse=True)[: a.top]:
        print(f"{len(s):4d}  {s[:100]!r}")


def cmd_crates(a):
    agg = collections.Counter()
    for n, (sz, t, _) in symbols(a.elf).items():
        if t not in "tTrR":
            continue
        clean = re.sub(r"[<>&]", "", n).replace(" as ", "::")
        agg[(("text" if t in "tT" else "rodata"), "::".join(clean.split("::")[: a.depth]))] += sz
    for (sec, k), v in agg.most_common(a.top):
        print(f"{v:7d}  {sec:6s}  {k}")


def cmd_baseline(a):
    secs = elf_sections(a.elf)
    payload = {
        "__sections__": {
            "text": secs.get(".text", (0, 0))[0],
            "rodata": secs.get(".rodata", (0, 0))[0],
            "data": secs.get(".data", (0, 0))[0],
            "bss": secs.get(".bss", (0, 0))[0],
        },
        "symbols": {n: v[0] for n, v in symbols(a.elf).items() if v[1] in "tTrR"},
    }
    with open(a.out, "w") as f:
        json.dump(payload, f, indent=0)
    print("saved", a.out)


def cmd_divs(a):
    fns = disasm(a.elf)
    div_targets = ("__aeabi_uidiv", "__aeabi_uidivmod", "__aeabi_idiv", "__aeabi_idivmod")
    callers = collections.Counter()
    for fn, ins in fns.items():
        for _, _, mn, op in ins:
            if mn in ("bl", "blx") and any(t in op for t in div_targets):
                callers[fn] += 1
    total_calls = sum(callers.values())
    print(f"== Cortex-M0+ Software Division Callers: {total_calls} calls across {len(callers)} functions ==")
    for fn, cnt in callers.most_common(a.top):
        print(f"{cnt:3d} calls  {fn}{loc_str(fn)}")


def cmd_unaligned(a):
    fns = disasm(a.elf)
    unaligned_targets = ("__aeabi_uread4", "__aeabi_uwrite4", "__aeabi_uread8", "__aeabi_uwrite8")
    callers = collections.Counter()
    for fn, ins in fns.items():
        for _, _, mn, op in ins:
            if mn in ("bl", "blx") and any(t in op for t in unaligned_targets):
                callers[fn] += 1
    total_calls = sum(callers.values())
    print(f"== Cortex-M0+ Unaligned Access Traps: {total_calls} calls across {len(callers)} functions ==")
    for fn, cnt in callers.most_common(a.top):
        print(f"{cnt:3d} calls  {fn}{loc_str(fn)}")


def bl_callers(fns, pred):
    """{callee: Counter(caller)} for bl/blx whose target name satisfies pred."""
    res = collections.defaultdict(collections.Counter)
    for f, body in fns.items():
        for _, _, mn, op in body:
            if mn in ("bl", "blx"):
                m = re.search(r"<([^>+]+)", op)
                if m and pred(m.group(1)):
                    res[m.group(1)][f] += 1
    return res


def cmd_math64(a):
    """64-bit emulation callers; each lmul site ~10 B setup + 4-reg pressure."""
    res = bl_callers(disasm(a.elf), lambda t: t.startswith("__aeabi_l") or "udivmoddi" in t or "divdi" in t)
    for t, c in res.items():
        print(f"== {t}: {sum(c.values())} calls / {len(c)} fns ==")
        for f, n in c.most_common(a.top):
            print(f"{n:3d}  {f}{loc_str(f)}")
    print("\nFIX [RECIPROCAL-32]: if operand <= 0xFFFF, use (x as u32 * K16) >> s (single muls);"
          " precompute Q16 constants at init; keep u64 only for true 64-bit values (UID hashing).")


def cmd_memcpy(a):
    """memcpy/memclr/memset callers -> pass-by-value structs, zero-init of big locals."""
    res = bl_callers(disasm(a.elf), lambda t: re.search(r"mem(cpy|clr|set|move)", t) is not None)
    per = collections.Counter()
    for c in res.values():
        per.update(c)
    print(f"== mem* calls: {sum(per.values())} sites / {len(per)} fns (~8 B each) ==")
    for t, c in sorted(res.items(), key=lambda kv: -sum(kv[1].values())):
        print(f"   {t:<20} {sum(c.values())}")
    for f, n in per.most_common(a.top):
        print(f"{n:3d}  {f}{loc_str(f)}")
    print("\nFIX [BY-REF]: return/accept large structs by &mut out-param; build in place (MaybeUninit or "
          "field writes) instead of `x = Struct{..}`; zero big buffers once in the arena, not per call.")


def cmd_pools(a):
    """PC-relative literal pools: global-anchor reloads and >1020 B reach splits."""
    secs = elf_sections(a.elf)
    ro_sz, ro = secs.get(".rodata", (0, 0))
    syms = {}
    for k, v in sorted(symbols(a.elf).items(), key=lambda kv: kv[1][0]):
        if v[1] in "bBdD":
            syms[v[2]] = f"{k} ({v[0]} B)"
    fns = disasm(a.elf)
    vals = collections.Counter()
    per = {}
    for f, body in fns.items():
        w = [(ad, op.split()[0]) for ad, _, mn, op in body if mn == ".word" and op]
        if w:
            per[f] = (len(w), body[-1][0] - body[0][0] + body[-1][1])
            for _, v in w:
                vals[v] += 1
    n = sum(vals.values())
    txt = secs.get(".text", (1, 0))[0]
    print(f"== literal pool words: {n} ({n*4} B = {n*400/txt:.1f}% of .text), {len(vals)} distinct ==")
    print("-- most reloaded literals (each extra copy = 4 B pool + ldr) --")
    for v, c in vals.most_common(a.top):
        iv = int(v, 16)
        kind = syms.get(iv, "")
        if not kind:
            kind = "rodata" if ro <= iv < ro + ro_sz else ("sram" if iv >> 28 == 2 else ("flash" if iv >> 24 == 8 else "const"))
        print(f"{c:4d}x {c*4:5d} B  {v}  {kind[:80]}")
    print("-- functions whose span exceeds the +1020 B ldr reach (pool is split/duplicated) --")
    for f, (k, span) in sorted(per.items(), key=lambda kv: -kv[1][1]):
        if span > 1020:
            print(f"{span:5d} B span  {k:3d} words  {f}{loc_str(f)}")
    print("\nFIX [GLOBAL-ANCHOR]: pass &mut Ctx/&mut Canvas down the call tree, or group hot statics "
          "into one #[repr(C)] struct so all accesses share one base literal (ldr r,[base,#imm5]).")


def cmd_rodata(a):
    """Attribute anonymous .rodata gaps to referencing functions and classify content."""
    secs = elf_sections(a.elf)
    sz, ro = secs[".rodata"]
    end = ro + sz
    raw = subprocess.run(["arm-none-eabi-objcopy", "-O", "binary", "-j", ".rodata", a.elf, "/dev/stdout"], capture_output=True).stdout
    named = sorted((v[2], v[0], k) for k, v in symbols(a.elf).items() if v[1] in "rR" and ro <= v[2] < end)
    gaps, cur = [], ro
    for ad, s, _ in named:
        if ad > cur:
            gaps.append((cur, ad - cur))
        cur = max(cur, ad + s)
    if cur < end:
        gaps.append((cur, end - cur))
    refs = collections.defaultdict(set)
    for f, body in disasm(a.elf).items():
        for _, _, mn, op in body:
            if mn == ".word" and op:
                v = int(op.split()[0], 16)
                if ro <= v < end:
                    refs[v].add(f)
    text_lo, text_hi = secs[".text"][1], secs[".text"][1] + secs[".text"][0]

    def classify(b):
        if not b:
            return "?"
        pr = sum(32 <= x < 127 for x in b) / len(b)
        words = [int.from_bytes(b[i:i+4], "little") for i in range(0, len(b) - 3, 4)]
        fp = sum(text_lo <= w < text_hi for w in words) / max(1, len(words))
        if fp > 0.3:
            return f"fn-ptrs/vtables ({fp*100:.0f}%)"
        if pr > 0.7:
            return "strings"
        return "binary (glyphs/icons/tables)"

    print(f"== .rodata {sz} B: named {sum(s for _, s, _ in named)} B, anonymous {sum(g for _, g in gaps)} B in {len(gaps)} gaps ==")
    print("-- top named .rodata symbols --")
    for ad, s, k in sorted(named, key=lambda x: -x[1])[: min(10, a.top)]:
        print(f"{s:6d} B  +{ad-ro:<6d} {k}{loc_str(k)}")
    print("-- top anonymous .rodata gaps --")
    for g, gs in sorted(gaps, key=lambda x: -x[1])[: a.top]:
        who = collections.Counter()
        for v, fs in refs.items():
            if g <= v < g + gs:
                who.update(fs)
        b = raw[g - ro:g - ro + gs]
        top_refs = ", ".join(f"{f}{loc_str(f)}" for f, _ in who.most_common(2))
        print(f"{gs:6d} B  +{g-ro:<6d} {classify(b):<28} refs: {top_refs[:120]}")
    print("\nFIX [ANON-RODATA]: pack match-arm strings via `utils::format::nth_cstr`, convert UI branches to `const` tables, move non-boot assets to KernFS.")


def cmd_traps(a):
    """udf/abort trap sites = unreachable/bounds/overflow checks surviving panic=abort."""
    fns = disasm(a.elf)
    c = collections.Counter()
    for f, body in fns.items():
        c[f] = sum(1 for i in body if i[2] == "udf")
    c = +c
    print(f"== udf trap sites: {sum(c.values())} in {len(c)} fns ==")
    print("   (1 udf/fn is usually the noreturn/unreachable tail; >1 = surviving bounds/overflow checks)")
    for f, n in c.most_common(a.top):
        print(f"{n:3d}  {f}{loc_str(f)}")
    print("\nFIX [NO-CHECK]: index via get()/iterators or `&arr[..N]` sub-slicing once; mask indices "
          "with power-of-two lengths (i & (N-1)); use wrapping_* where overflow is intended.")


def cmd_suggest(a):
    """Compact ranked pattern-coded refactor list with file:line (use `tasks` for full recipes)."""
    info = analyze_elf(a.elf)
    info["_elf"] = a.elf
    tasks = build_task_cards(info)
    for i, t in enumerate(tasks[: a.top], 1):
        print(f"{i:2d}. ~{t['est']:4d} B  [{t['pattern']}]  {t['title']}  ({t['target']})")


def cmd_find(a):
    """List symbols matching --match (substring) sorted by size with file:line and callees."""
    pat = a.match or ""
    syms = symbols(a.elf)
    fns = disasm(a.elf)
    matches = sorted(
        ((sz, t, k) for k, (sz, t, _) in syms.items() if pat in k and t in "tTrRdDbB" and sz >= a.min),
        reverse=True,
    )
    tot = sum(sz for sz, _, _ in matches)
    print(f"== match '{pat}' (>={a.min} B): {len(matches)} symbols, {tot} B total ==")
    for sz, t, k in matches[: a.top]:
        callees = ""
        if t in "tT" and k in fns:
            cs = [m.group(1).split("::")[-1] for _, _, mn, op in fns[k]
                  if mn in ("bl", "blx", "b.w", "b") and (m := re.search(r"<([^>+]+)", op)) and m.group(1) != k]
            if cs:
                callees = " -> " + ", ".join(dict.fromkeys(cs))[:50]
        print(f"{sz:6d} B  [{t}]  {k}{loc_str(k)}{callees}")


def cmd_syscalls(a):
    """List runtime::syscall functions sorted by size with callees (Phase 10 target <= 24 B)."""
    syms = symbols(a.elf)
    fns = disasm(a.elf)
    sc = sorted(((sz, k) for k, (sz, t, _) in syms.items() if "runtime::syscall" in k and t in "tT"), reverse=True)
    tot = sum(sz for sz, _ in sc)
    over = sum(1 for sz, _ in sc if sz > 24)
    print(f"== runtime::syscall: {len(sc)} symbols, {tot} B total, {over} > 24 B (Phase 10 target <= 8,192 B) ==")
    for sz, k in sc[: a.top]:
        callees = []
        for _, _, mn, op in fns.get(k, ()):
            if mn in ("bl", "blx", "b.w", "b"):
                m = re.search(r"<([^>+]+)", op)
                if m and m.group(1) != k:
                    callees.append(m.group(1).split("::")[-1])
        uniq_c = ", ".join(dict.fromkeys(callees))[:55]
        flag = "!" if sz > 24 else " "
        print(f"{flag}{sz:5d} B  {k}{loc_str(k)}  -> {uniq_c}")


def cmd_phases(a):
    """Live scoreboard against docs/flash_audit_measured_plan.md Phases 10-21."""
    syms = symbols(a.elf)
    secs = elf_sections(a.elf)
    fns = disasm(a.elf)
    text_sz = secs.get(".text", (0, 0))[0]
    rodata_sz, _, named_ro, anon_ro = rodata_stats(syms, secs)
    data_sz = secs.get(".data", (0, 0))[0]
    flash = text_sz + rodata_sz + data_sz

    def sum_mod(substr, types="tT"):
        return sum(sz for k, (sz, t, _) in syms.items() if substr in k and t in types)

    def max_fn(substr):
        return max(((sz, k) for k, (sz, t, _) in syms.items() if substr in k and t in "tT"), default=(0, ""))

    def count_bl(pred):
        return sum(
            1 for body in fns.values() for _, _, mn, op in body
            if mn in ("bl", "blx") and (m := re.search(r"<([^>+]+)", op)) and pred(m.group(1))
        )

    sc_tot = sum_mod("runtime::syscall")
    sc_apis = [sz for k, (sz, t, _) in syms.items() if "runtime::syscall" in k and "::api_" in k and t in "tT"]
    wire_tot = sum_mod("services::wire")
    wire_top_sz, wire_top_nm = max_fn("services::wire::")
    uread_cnt = count_bl(lambda t: any(u in t for u in ("__aeabi_uread4", "__aeabi_uwrite4", "__aeabi_uread8", "__aeabi_uwrite8")))
    div_cnt = count_bl(lambda t: t in ("__aeabi_uidiv", "__aeabi_uidivmod", "__aeabi_idiv", "__aeabi_idivmod"))
    wreg_cnt = count_bl(lambda t: "Bk4829>::write_reg" in t or "Bk4829::write_reg" in t)
    comp_tot = sum_mod("orchestrator::compositor")
    dlg_tot = sum_mod("gfx::widgets::dialog")
    p16_tot = sum_mod("boot::recovery") + sum_mod("panic::baremetal") + sum_mod("boot::orchestration")
    disp_sz, _ = max_fn("::dispatch_event")
    loop_sz, _ = max_fn("run_system_loop")
    krx_tot = (sum_mod("native_runner::load_native") + sum_mod("loader::launch_krx")
               + sum_mod("KrxPreamble") + sum_mod("inspect_package") + sum_mod("bind_cap_imports"))
    settings_tot = sum_mod("apps::settings", "tTrR")
    vfo_tot = sum_mod("apps::vfo", "tTrR")

    target_80 = int(FLASH_MAX * 0.80)
    rows = [
        ("Flash <=80%", flash, target_80, f"{flash*100/FLASH_MAX:.1f}% ({flash - target_80:+d} B to 80%)"),
        ("P10 Syscall", sc_tot, 8192, f"{len(sc_apis)} api_* shims, max={max(sc_apis, default=0)} B"),
        ("P11 Wire", wire_tot, 2400, f"top={wire_top_nm.split('::')[-1]} ({wire_top_sz} B)"),
        ("P12 Unaligned", uread_cnt, 10, f"{uread_cnt} __aeabi_uread/uwrite calls"),
        ("P13 Divisions", div_cnt, 15, f"{div_cnt} __aeabi_uidiv/idiv calls"),
        ("P14 BK4829", wreg_cnt, 15, f"{wreg_cnt} write_reg call sites"),
        ("P15 Modal/Dlg", comp_tot + dlg_tot, 2400, f"compositor={comp_tot} B, dialog={dlg_tot} B"),
        ("P16 TextScreen", p16_tot, 2000, f"recovery+baremetal+orchestration={p16_tot} B"),
        ("P17 Dispatch", disp_sz, 400, f"dispatch_event={disp_sz} B, loop={loop_sz} B"),
        ("P18 AnonRodata", anon_ro, 4096, f"total .rodata={rodata_sz} B (named={named_ro} B)"),
        ("P19 KRX Parse", krx_tot, 1800, f"loader+native+preamble+catalog+cap={krx_tot} B"),
        ("P20 Settings", settings_tot, 4000, f".text+.rodata={settings_tot} B"),
        ("P21 VFO", vfo_tot, 2500, f".text+.rodata={vfo_tot} B"),
    ]
    print("══════════════════════════════════════════════════════════════════════════")
    print(" 📋 MEASURED PLAN PHASE SCOREBOARD (docs/flash_audit_measured_plan.md)")
    print("══════════════════════════════════════════════════════════════════════════")
    for label, cur, tgt, note in rows:
        st = "PASS" if cur <= tgt else "OPEN"
        print(f" [{st}] {label:<14} {cur:6d} / {tgt:<6d}  | {note}")


def cmd_modules(a):
    from collections import defaultdict
    syms = symbols(a.elf)
    buckets = defaultdict(lambda: [0, 0, []])
    for k, (sz, t, _) in syms.items():
        if t not in "tTrR":
            continue
        clean = k.lstrip("<").split(" as ")[0]
        parts = clean.split("::")
        mod = "::".join(parts[:3]) if len(parts) >= 3 and parts[0] == "firmware" else "::".join(parts[:2])
        buckets[mod][0] += sz
        buckets[mod][1] += 1
        if t in "tT":
            buckets[mod][2].append((sz, parts[-1]))
    print(f"== module flash budget (.text + named .rodata, top {a.top}) ==")
    for mod, (tot, cnt, fns) in sorted(buckets.items(), key=lambda x: -x[1][0])[: a.top]:
        top3 = ", ".join(f"{nm}({sz})" for sz, nm in sorted(fns, reverse=True)[:3])
        print(f"{tot:6d} B ({cnt:3d} syms)  {mod:<36}  {top3}")


def cmd_diff(a):
    raw_old = json.load(open(a.base))
    if "__sections__" in raw_old:
        old_secs = raw_old["__sections__"]
        old = raw_old["symbols"]
        secs = elf_sections(a.elf)
        new_secs = {k: secs.get("." + k, (0, 0))[0] for k in ("text", "rodata", "data", "bss")}
        old_fl = old_secs["text"] + old_secs["rodata"] + old_secs["data"]
        new_fl = new_secs["text"] + new_secs["rodata"] + new_secs["data"]
        print(
            f"FLASH delta: {new_fl - old_fl:+d} B ({old_fl} -> {new_fl}) | "
            f".text {new_secs['text'] - old_secs['text']:+d}, "
            f".rodata {new_secs['rodata'] - old_secs['rodata']:+d}, "
            f".data {new_secs['data'] - old_secs['data']:+d}, "
            f".bss {new_secs['bss'] - old_secs['bss']:+d}"
        )
    else:
        old = raw_old
    new = {n: v[0] for n, v in symbols(a.elf).items() if v[1] in "tTrR"}
    d = sorted(((new.get(k, 0) - old.get(k, 0), k) for k in set(old) | set(new)), key=lambda x: -abs(x[0]))
    print(f"named symbols delta: {sum(new.values()) - sum(old.values()):+d} B")
    for dv, k in d[: a.top]:
        if dv:
            print(f"{dv:+6d}  {k}{loc_str(k)}")


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument(
        "cmd",
        choices="tasks audit stack fields dups outline mono calls vtables consts strings crates baseline diff divs unaligned math64 memcpy pools rodata traps suggest phases syscalls find modules".split(),
    )
    p.add_argument("--elf", default=DEF_ELF)
    p.add_argument("--top", type=int, default=25)
    p.add_argument("--min", type=int, default=24, help="min fn or stack bytes")
    p.add_argument("--sim", type=float, default=0.80)
    p.add_argument("--len", type=int, default=8, help="outline window (instructions)")
    p.add_argument("--count", type=int, default=3, help="min occurrences for outline")
    p.add_argument("--depth", type=int, default=3)
    p.add_argument("--out", default="docs/flash_baseline.json")
    p.add_argument("--base", default="docs/flash_baseline.json")
    p.add_argument("--match", default="", help="substring filter for 'find'")
    p.add_argument("--pattern", default="", help="filter tasks by pattern code (e.g. BY-REF, SYSCALL-THIN)")
    p.add_argument("--task", type=int, default=0, help="show only 1-based task #N in `tasks`")
    p.add_argument("--json", help="path to export JSON report")
    a = p.parse_args()
    if a.cmd == "calls" and a.min == 24:
        a.min = 400
    if a.cmd == "stack" and a.min == 24:
        a.min = 96
    globals()["cmd_" + a.cmd](a)


if __name__ == "__main__":
    sys.exit(main())
