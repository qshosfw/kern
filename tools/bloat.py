#!/usr/bin/env python3
"""Kern flash-bloat analyzer: finds *actionable* refactor candidates in the release ELF.

Subcommands:
  audit     executive architectural health report & top actionable bloat sources (START HERE)
  stack     stack-frame size audit: finds functions allocating massive stack frames (>96 B)
  fields    Cortex-M0+ imm5 struct offset penalties (movs + ldr/str due to offsets >31/62/124 B)
  dups      identical / near-identical functions (normalized Thumb disassembly) -> merge or share
  outline   repeated instruction sequences across functions -> extract helper / data table
  mono      generic instantiations grouped by base path -> de-monomorphize candidates
  calls     call-site fan-in x setup cost; large single-caller fns (inlining monoliths)
  consts    32-bit literals repeated across functions -> shared tables/enums
  strings   duplicate / near-duplicate rodata strings -> string pool
  crates    .text/.rodata rollup per crate::module (depth configurable)
  suggest   ranked pattern-coded refactor tasks ([SPLIT-1K] [GLOBAL-ANCHOR] [RECIPROCAL-32] ...)
  rodata    attribute anonymous .rodata gaps to referencing fns + classify (strings/glyphs/vtables)
  pools     literal-pool tax, most-reloaded statics, fns exceeding +1020 B ldr reach
  math64    __aeabi_l* 64-bit emulation callers;  memcpy  mem* callers;  traps  udf check sites
  divs / unaligned   software division / unaligned-access helper callers
  phases    live scoreboard against docs/flash_audit_measured_plan.md Phases 10-19 targets
  syscalls  list runtime::syscall shims sorted by size (Phase 10 target <= 24 B)
  baseline  save per-symbol sizes + section totals to JSON;  diff  compare current ELF to baseline

Usage: tools/bloat.py <cmd> [--elf PATH] [--top N] [--json FILE] [...]
"""
import argparse, collections, hashlib, json, re, subprocess, sys

DEF_ELF = "target/thumbv6m-none-eabi/release/firmware"
FLASH_MAX = 120832  # 128KB minus bootloader reserved pages on PY32F071xB
FN_RE = re.compile(r"^([0-9a-f]+) <(.+)>:$")
INS_RE = re.compile(r"^\s*([0-9a-f]+):\s+((?:[0-9a-f]{4}\s?)+)\s+(\S+)\s*(.*)$")


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
            cur = m.group(2); fns[cur] = []; continue
        m = INS_RE.match(ln)
        if m and cur:
            size = len(m.group(2).replace(" ", "")) // 2
            fns[cur].append((int(m.group(1), 16), size, m.group(3), m.group(4)))
    return fns


def norm(ins):
    """Normalize away addresses, call targets, literal offsets; keep shape + registers."""
    _, _, mn, op = ins
    op = re.sub(r"<[^>]*>", "<T>", op)
    op = re.sub(r";.*$", "", op)
    op = re.sub(r"\b[0-9a-f]{5,}\b", "A", op)
    op = re.sub(r"\[pc, #\d+\]", "[pc,L]", op)
    return mn + " " + op.strip()


def fbytes(body):
    return sum(i[1] for i in body)


def base_name(n):
    s = re.sub(r"::<.*>$", "", n)
    return re.sub(r"<([^<>]*?) as [^<>]*>", r"\1", s)


def cmd_audit(a):
    """Generate comprehensive architectural bloat diagnosis and actionable score."""
    syms = symbols(a.elf)
    secs = elf_sections(a.elf)
    text_sz = secs.get(".text", (0, 0))[0]
    rodata_sz = secs.get(".rodata", (0, 0))[0]
    data_sz = secs.get(".data", (0, 0))[0]
    bss_sz = secs.get(".bss", (0, 0))[0]
    flash_used = text_sz + rodata_sz + data_sz
    free_headroom = FLASH_MAX - flash_used
    util_pct = (flash_used / FLASH_MAX) * 100.0

    named_rodata = sum(v[0] for v in syms.values() if v[1] in "rR")
    anon_rodata = max(0, rodata_sz - named_rodata)

    fns = disasm(a.elf)

    # 1. Emulation traps (__aeabi_*)
    aeabi_calls = collections.defaultdict(lambda: collections.defaultdict(int))
    for f, body in fns.items():
        for _, _, mn, op in body:
            if mn in ("bl", "blx"):
                m = re.search(r"<(__aeabi_[^>+]+)", op)
                if m:
                    aeabi_calls[m.group(1)][f] += 1
    total_aeabi_calls = sum(sum(c.values()) for c in aeabi_calls.values())

    # 2. Stack hogs
    stack_hogs = []
    for f, body in fns.items():
        sp_sub = 0
        for _, _, mn, op in body[:15]:
            if mn in ("sub", "subs") and "sp" in op:
                m = re.search(r"#(\d+)", op)
                if m:
                    sp_sub += int(m.group(1))
        if sp_sub >= 96:
            stack_hogs.append((sp_sub, f))
    stack_hogs.sort(reverse=True)

    # 3. Imm5 struct offset penalties
    imm_penalties = []
    for f, body in fns.items():
        count = 0
        for i in range(len(body) - 1):
            _, _, mn1, op1 = body[i]
            _, _, mn2, _ = body[i + 1]
            m1 = re.search(r"movs\s+r\d+,\s*#(\d+)", mn1 + " " + op1)
            if m1:
                imm = int(m1.group(1))
                if ("ldrb" in mn2 or "strb" in mn2) and imm > 31: count += 1
                elif ("ldrh" in mn2 or "strh" in mn2) and imm > 62: count += 1
                elif ("ldr" in mn2 or "str" in mn2) and imm > 124: count += 1
        if count > 0:
            imm_penalties.append((count, f))
    imm_penalties.sort(reverse=True)

    # 4. Monolith functions (>800 B)
    callers = collections.defaultdict(set)
    for f, body in fns.items():
        for _, _, mn, op in body:
            if mn in ("bl", "blx"):
                m = re.search(r"<(.+?)(\+0x[0-9a-f]+)?>", op)
                if m:
                    callers[m.group(1)].add(f)
    monoliths = []
    for f, (sz, t, _) in syms.items():
        if t in "tT" and sz >= 800:
            monoliths.append((sz, len(callers.get(f, ())), f))
    monoliths.sort(reverse=True)

    # 5. Syscall wrapper inflation
    syscall_syms = {k: v[0] for k, v in syms.items() if "runtime::syscall" in k and v[1] in "tT"}
    syscall_total = sum(syscall_syms.values())
    syscall_avg = (syscall_total / len(syscall_syms)) if syscall_syms else 0

    print("══════════════════════════════════════════════════════════════════")
    print(" ⚡ KERN ARCHITECTURAL FLASH AUDIT & HEALTH REPORT")
    print("══════════════════════════════════════════════════════════════════")
    print(f" Flash Usage:     {flash_used:7d} / {FLASH_MAX} B  ({util_pct:.1f}% util | {free_headroom} B free)")
    print(f"  • .text:        {text_sz:7d} B  ({(text_sz/flash_used)*100:.1f}%)")
    print(f"  • .rodata:      {rodata_sz:7d} B  ({(rodata_sz/flash_used)*100:.1f}%) [Named: {named_rodata} B | Anonymous: {anon_rodata} B ({anon_rodata*100/rodata_sz:.1f}%)]")
    print(f"  • SRAM (.bss):  {bss_sz:7d} B | (.data): {data_sz} B")
    print("──────────────────────────────────────────────────────────────────")
    print(" 🚨 ARCHITECTURAL BLOAT INDICATORS")
    print("──────────────────────────────────────────────────────────────────")
    print(f" 1. Software Emulation:  {total_aeabi_calls:4d} calls across {len(aeabi_calls)} __aeabi functions (~{total_aeabi_calls * 14} B code tax)")
    for t, c in sorted(aeabi_calls.items(), key=lambda kv: -sum(kv[1].values()))[:4]:
        print(f"     - {t:<18} {sum(c.values()):3d} call sites ({len(c)} caller fns)")
    print(f" 2. Syscall Shim Bloat:  {len(syscall_syms):4d} shims = {syscall_total:5d} B (avg {syscall_avg:.1f} B/shim, target ≤ 24 B)")
    print(f" 3. Stack Frame Hogs:    {len(stack_hogs):4d} fns allocate ≥ 96 B stack (up to {stack_hogs[0][0]} B in single fn)")
    print(f" 4. Imm5 Offset Penalty: {len(imm_penalties):4d} fns suffer from struct fields > 31/124 B ({sum(c for c, _ in imm_penalties)} total penalties)")
    print(f" 5. Monolith Functions:  {len(monoliths):4d} fns ≥ 800 B (top: {monoliths[0][0]} B)")
    print("──────────────────────────────────────────────────────────────────")
    print(" 🎯 TOP 5 HIGH-YIELD ARCHITECTURAL ACTIONS (Ranked by Savings)")
    print("──────────────────────────────────────────────────────────────────")
    print(" [1] PHASE 10: Collapse Syscall Shims into Canvas Signed Primitives")
    print(f"     Target: {syscall_total} B -> ~4,000 B (Est. Saving: ~7,500 B)")
    print("     Action: Remove inlined gfx routines from shims; use Canvas signed entry points.")
    print(" [2] PHASE 18: Attribute Anonymous Rodata & Move Fonts/Packages to KernFS")
    print(f"     Target: {anon_rodata} B anonymous rodata -> ~4,000 B (Est. Saving: ~6,000–10,000 B)")
    print("     Action: Subset fonts, PackBits RLE for icons, string pool for UI labels.")
    print(" [3] PHASE 11: RigWire Facet Table & ByteWriter Cursor")
    wire_top = max(((sz, k) for k, (sz, t, _) in syms.items() if "services::wire::" in k and t in "tT"), default=(0, ""))
    print(f"     Target: top wire fn ({wire_top[1].split('::')[-1]}: {wire_top[0]} B) -> <= 1,200 B")
    print("     Action: Replace inline facet serializers with const FacetDesc table.")
    print(" [4] PHASE 15: Unified Sheet Modal Compositor")
    print("     Target: Compositor + dialogs + settings sheets -> one Sheet model (Est. Saving: ~3,000 B)")
    print("     Action: Merge 4 overlapping modal renderers into single Sheet::render().")
    print(" [5] PHASE 12 & 13: Naturally Aligned Structs & Division Elimination")
    print(f"     Target: {total_aeabi_calls} __aeabi calls -> ≤ 30 calls (Est. Saving: ~2,500 B)")
    print("     Action: Replace #[repr(C, packed)] with #[repr(C)] aligned; Q16 / shifts for division.")
    print("══════════════════════════════════════════════════════════════════")

    if a.json:
        report_data = {
            "flash_used": flash_used, "flash_max": FLASH_MAX, "util_pct": util_pct,
            "text_sz": text_sz, "rodata_sz": rodata_sz, "named_rodata": named_rodata, "anon_rodata": anon_rodata,
            "total_aeabi_calls": total_aeabi_calls, "aeabi_breakdown": {t: sum(c.values()) for t, c in aeabi_calls.items()},
            "syscall_total": syscall_total, "syscall_count": len(syscall_syms), "syscall_avg": syscall_avg,
            "stack_hogs_count": len(stack_hogs), "top_stack_hogs": stack_hogs[:10],
            "imm5_penalties_count": len(imm_penalties), "top_imm5_penalties": imm_penalties[:10],
            "monoliths_count": len(monoliths), "top_monoliths": monoliths[:10],
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
    print("   (Allocate large buffers in central TRANSIENT_ARENA instead of on stack!)")
    for sz, f in stack_usage[: a.top]:
        print(f"{sz:5d} B stack  {f}")


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
    print("   (Order structs with hot u8/u16 in first 32 bytes to allow 1-insn ldrb/ldrh!)")
    for c, f, top_off in imm_penalties[: a.top]:
        offs_str = ", ".join(f"{k}x{v}" for k, v in top_off)
        print(f"{c:3d} penalties  {f:<75}  [{offs_str}]")


def cmd_dups(a):
    fns = disasm(a.elf); groups = collections.defaultdict(list)
    for f, b in fns.items():
        if fbytes(b) >= a.min:
            groups[hashlib.sha1("\n".join(map(norm, b)).encode()).hexdigest()].append(f)
    print("== exact (normalized) duplicates: keep one, route others to it ==")
    rows = [(fbytes(fns[g[0]]) * (len(g) - 1), g) for g in groups.values() if len(g) > 1]
    for save, g in sorted(rows, reverse=True)[: a.top]:
        print(f"{save:6d} B  x{len(g)}  " + " | ".join(g))
    print("\n== near duplicates (shingle Jaccard >= %.2f): extract shared core ==" % a.sim)
    cand = [(f, set(zip(*[list(map(norm, b))[i:] for i in range(4)]))) for f, b in fns.items() if fbytes(b) >= max(a.min, 48)]
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
        print(f"{sz:6d} B  sim={s:.2f}  {f1}  ~  {f2}")


def cmd_outline(a):
    fns = disasm(a.elf); seqs = collections.defaultdict(set); cost = {}
    for f, b in fns.items():
        n = [norm(i) for i in b]
        for k in range(len(n) - a.len + 1):
            win = tuple(n[k:k + a.len])
            if any(w.startswith(("b ", "bx", "pop", "push")) for w in win[1:-1]):
                continue  # don't cross basic-block boundaries crudely
            seqs[win].add(f); cost[win] = sum(i[1] for i in b[k:k + a.len])
    rows = [((len(fs) - 1) * cost[w] - len(fs) * 4, w, fs) for w, fs in seqs.items() if len(fs) >= a.count]
    rows.sort(key=lambda r: -r[0]); seen = set()
    print(f"== repeated {a.len}-instruction sequences (est. saving if outlined into one helper) ==")
    for save, w, fs in rows:
        key = frozenset(fs)
        if save <= 0 or key in seen:
            continue
        seen.add(key)
        print(f"{save:5d} B  in {len(fs)} fns: {', '.join(sorted(fs)[:5])}{' ...' if len(fs) > 5 else ''}")
        print("        " + " ; ".join(w))
        if len(seen) >= a.top:
            break


def cmd_mono(a):
    syms = symbols(a.elf); g = collections.defaultdict(list)
    for n, (sz, t, _) in syms.items():
        if t in "tT" and "::<" in n:
            g[base_name(n)].append((sz, n))
    rows = sorted(((sum(s for s, _ in v), k, v) for k, v in g.items()), reverse=True)
    print("== generic families (total bytes, instances). >1 instance or 1 instance of a large generic = de-mono candidate ==")
    for tot, k, v in rows[: a.top]:
        print(f"{tot:6d} B  x{len(v)}  {k}")
        for sz, n in sorted(v, reverse=True)[:3]:
            print(f"           {sz:5d}  {n[len(k):][:110]}")


def cmd_calls(a):
    fns = disasm(a.elf); syms = symbols(a.elf); fan = collections.Counter(); callers = collections.defaultdict(set)
    for f, b in fns.items():
        for i in b:
            if i[2] in ("bl", "blx"):
                m = re.search(r"<(.+?)(\+0x[0-9a-f]+)?>", i[3])
                if m:
                    fan[m.group(1)] += 1; callers[m.group(1)].add(f)
    print("== hot callees: call sites x ~6 B arg setup. Fold args into a struct/table or make the callee take one descriptor ==")
    for t, c in fan.most_common(a.top):
        print(f"{c:4d} sites  ~{c * 6:5d} B setup  {t}")
    print("\n== large single-caller functions (fused monoliths; split by state/table, or inline legitimately) ==")
    for f, (sz, t, _) in sorted(syms.items(), key=lambda kv: -kv[1][0]):
        if t in "tT" and sz >= a.min and len(callers.get(f, ())) <= 1:
            print(f"{sz:6d} B  callers={sorted(callers.get(f, {'<vtable/entry>'}))[:1]}  {f}")


def cmd_consts(a):
    fns = disasm(a.elf); use = collections.defaultdict(set)
    for f, b in fns.items():
        for i in b:
            if i[2] == ".word":
                v = i[3].split()[0]
                if not v.startswith("0x08") and not v.startswith("0x20"):
                    use[v].add(f)
            m = re.search(r"#(\d{3,})", i[3])
            if m and i[2] in ("movs", "cmp", "adds", "subs"):
                use["#" + m.group(1)].add(f)
    print("== constants repeated across functions -> name them once (enum/const table) and centralize logic using them ==")
    for v, fs in sorted(use.items(), key=lambda kv: -len(kv[1]))[: a.top]:
        if len(fs) >= 3:
            print(f"{len(fs):3d} fns  {v:>12}  {', '.join(sorted(fs)[:4])}")


def cmd_strings(a):
    raw = subprocess.run(["arm-none-eabi-objcopy", "-O", "binary", "-j", ".rodata", a.elf, "/dev/stdout"], capture_output=True).stdout
    strs = [m.group().decode() for m in re.finditer(rb"[\x20-\x7e]{4,}", raw)]
    c = collections.Counter(strs)
    print(f"== rodata printable runs: {len(strs)}, {sum(map(len, strs))} B ==")
    print("-- exact duplicates --")
    for s, n in c.most_common(a.top):
        if n > 1:
            print(f"x{n}  {s!r}")
    print("-- prefix/suffix sharing candidates (one string is a suffix of another) --")
    uniq = sorted(set(strs), key=len); k = 0
    for i, s in enumerate(uniq):
        for t in uniq[i + 1:]:
            if t.endswith(s) or t.startswith(s):
                print(f"{len(s):3d} B  {s!r}  in  {t!r}"); k += 1; break
        if k >= a.top:
            break
    print("-- longest strings (move to host tool / shorten / feature-gate) --")
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
        print(f"{cnt:3d} calls  {fn}")


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
        print(f"{cnt:3d} calls  {fn}")


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
    tot = collections.Counter()
    for t, c in res.items():
        print(f"== {t}: {sum(c.values())} calls / {len(c)} fns ==")
        for f, n in c.most_common(a.top):
            print(f"{n:3d}  {f}"); tot[f] += n
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
        print(f"{n:3d}  {f}")
    print("\nFIX [BY-REF]: return/accept large structs by &mut out-param; build in place (MaybeUninit or "
          "field writes) instead of `x = Struct{..}`; zero big buffers once in the arena, not per call.")


def cmd_pools(a):
    """PC-relative literal pools: global-anchor reloads and >1020 B reach splits."""
    secs = elf_sections(a.elf); ro_sz, ro = secs.get(".rodata", (0, 0))
    syms = {}
    for k, v in sorted(symbols(a.elf).items(), key=lambda kv: kv[1][0]):
        if v[1] in "bBdD":
            syms[v[2]] = f"{k} ({v[0]} B)"  # largest symbol at an address wins
    fns = disasm(a.elf); vals = collections.Counter(); per = {}
    for f, body in fns.items():
        w = [(ad, op.split()[0]) for ad, _, mn, op in body if mn == ".word" and op]
        if w:
            per[f] = (len(w), body[-1][0] - body[0][0] + body[-1][1])
            for _, v in w:
                vals[v] += 1
    n = sum(vals.values()); txt = secs.get(".text", (1, 0))[0]
    print(f"== literal pool words: {n} ({n*4} B = {n*400/txt:.1f}% of .text), {len(vals)} distinct ==")
    print("-- most reloaded literals (each extra copy = 4 B pool + ldr) --")
    for v, c in vals.most_common(a.top):
        iv = int(v, 16); kind = syms.get(iv, "")
        if not kind:
            kind = "rodata" if ro <= iv < ro + ro_sz else ("sram" if iv >> 28 == 2 else ("flash" if iv >> 24 == 8 else "const"))
        print(f"{c:4d}x {c*4:5d} B  {v}  {kind[:80]}")
    print("-- functions whose span exceeds the +1020 B ldr reach (pool is split/duplicated) --")
    for f, (k, span) in sorted(per.items(), key=lambda kv: -kv[1][1]):
        if span > 1020:
            print(f"{span:5d} B span  {k:3d} words  {f}")
    print("\nNOTE: a hot SRAM literal labelled with a tiny static is usually an LLVM GlobalMerge base "
          "(_MergedGlobals): already anchored, its count = number of fns touching small statics.")
    print("FIX [GLOBAL-ANCHOR]: pass &mut Ctx/&mut Canvas down the call tree, or group hot statics "
          "into one #[repr(C)] struct so all accesses share one base literal (ldr r,[base,#imm5]).")


def cmd_rodata(a):
    """Attribute anonymous .rodata gaps to referencing functions and classify content."""
    secs = elf_sections(a.elf); sz, ro = secs[".rodata"]; end = ro + sz
    raw = subprocess.run(["arm-none-eabi-objcopy", "-O", "binary", "-j", ".rodata", a.elf, "/dev/stdout"],
                         capture_output=True).stdout
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
        print(f"{s:6d} B  +{ad-ro:<6d} {k}")
    print("-- top anonymous .rodata gaps --")
    for g, gs in sorted(gaps, key=lambda x: -x[1])[: a.top]:
        who = collections.Counter()
        for v, fs in refs.items():
            if g <= v < g + gs:
                who.update(fs)
        b = raw[g - ro:g - ro + gs]
        print(f"{gs:6d} B  +{g-ro:<6d} {classify(b):<30} refs: {', '.join(f for f, _ in who.most_common(3))[:120]}")
    print("\nFIX [NAME-ASSET]: give fonts/icons named `static`s; [STR-POOL]: dedupe labels; "
          "[VTABLE]: collapse trait objects with 1 impl; move non-boot assets to KernFS.")


def cmd_traps(a):
    """udf/abort trap sites = unreachable/bounds/overflow checks surviving panic=abort."""
    fns = disasm(a.elf); c = collections.Counter()
    for f, body in fns.items():
        c[f] = sum(1 for i in body if i[2] == "udf")
    c = +c
    print(f"== udf trap sites: {sum(c.values())} in {len(c)} fns ==")
    print("   (1 udf/fn is usually the noreturn/unreachable tail; >1 = surviving bounds/overflow checks)")
    for f, n in c.most_common(a.top):
        print(f"{n:3d}  {f}")
    print("\nFIX [NO-CHECK]: index via get()/iterators or `&arr[..N]` sub-slicing once; mask indices "
          "with power-of-two lengths (i & (N-1)); use wrapping_* where overflow is intended.")


def cmd_suggest(a):
    """Ranked, pattern-coded refactor tasks generated from live ELF telemetry."""
    syms = symbols(a.elf); fns = disasm(a.elf); tasks = []
    for t, c in bl_callers(fns, lambda t: t.startswith("__aeabi_")).items():
        for f, n in c.items():
            code = {"lmul": "RECIPROCAL-32", "uidiv": "RECIPROCAL-DIV", "idiv": "RECIPROCAL-DIV",
                    "uread": "ALIGN-RECORD", "uwrite": "ALIGN-RECORD", "mem": "BY-REF"}
            k = next((v for key, v in code.items() if key in t), None)
            if k and n >= 2:
                tasks.append((n * 10, k, f"{f}: {n}x {t}"))
    for f, (sz, ty, _) in syms.items():
        if ty in "tT" and sz > 1020:
            tasks.append((sz // 4, "SPLIT-1K", f"{f}: {sz} B > ldr reach; split by state / table-dispatch"))
    for f, body in fns.items():
        w = collections.Counter(op.split()[0] for _, _, mn, op in body if mn == ".word" and op)
        dup = sum(n for v, n in w.items() if v.startswith("0x200"))
        if dup >= 8:
            tasks.append((dup * 4, "GLOBAL-ANCHOR", f"{f}: {dup} SRAM-static literals; pass &mut ctx"))
        sp = sum(int(m.group(1)) for _, _, mn, op in body[:15] if mn.startswith("sub") and "sp" in op
                 for m in [re.search(r"#(\d+)", op)] if m)
        if sp >= 256:
            tasks.append((sp // 8, "STACK-ARENA", f"{f}: {sp} B stack frame -> TRANSIENT_ARENA lease"))
    for i, (s, k, d) in enumerate(sorted(tasks, reverse=True)[: a.top], 1):
        print(f"{i:2d}. ~{s:4d} B  [{k}]  {d}")


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
        print(f"{flag}{sz:5d} B  {k:<62}  -> {uniq_c}")


def cmd_phases(a):
    """Live scoreboard against docs/flash_audit_measured_plan.md Phases 10-19."""
    syms = symbols(a.elf)
    secs = elf_sections(a.elf)
    fns = disasm(a.elf)
    text_sz = secs.get(".text", (0, 0))[0]
    rodata_sz = secs.get(".rodata", (0, 0))[0]
    data_sz = secs.get(".data", (0, 0))[0]
    flash = text_sz + rodata_sz + data_sz
    named_ro = sum(v[0] for v in syms.values() if v[1] in "rR")
    anon_ro = max(0, rodata_sz - named_ro)

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
    ]
    print("══════════════════════════════════════════════════════════════════════════")
    print(" 📋 MEASURED PLAN PHASE SCOREBOARD (docs/flash_audit_measured_plan.md)")
    print("══════════════════════════════════════════════════════════════════════════")
    for label, cur, tgt, note in rows:
        st = "PASS" if cur <= tgt else "OPEN"
        print(f" [{st}] {label:<14} {cur:6d} / {tgt:<6d}  | {note}")


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
            print(f"{dv:+6d}  {k}")


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("cmd", choices="audit stack fields dups outline mono calls consts strings crates baseline diff divs unaligned math64 memcpy pools rodata traps suggest phases syscalls".split())
    p.add_argument("--elf", default=DEF_ELF); p.add_argument("--top", type=int, default=25)
    p.add_argument("--min", type=int, default=24, help="min fn or stack bytes"); p.add_argument("--sim", type=float, default=0.80)
    p.add_argument("--len", type=int, default=8, help="outline window (instructions)")
    p.add_argument("--count", type=int, default=3, help="min occurrences for outline")
    p.add_argument("--depth", type=int, default=3); p.add_argument("--out", default="docs/flash_baseline.json")
    p.add_argument("--base", default="docs/flash_baseline.json")
    p.add_argument("--json", help="path to export JSON report")
    a = p.parse_args()
    if a.cmd == "calls" and a.min == 24:
        a.min = 400
    if a.cmd == "stack" and a.min == 24:
        a.min = 96
    globals()["cmd_" + a.cmd](a)


if __name__ == "__main__":
    sys.exit(main())
