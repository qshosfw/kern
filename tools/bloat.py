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
  baseline  save per-symbol sizes to JSON;  diff  compare current ELF to a baseline

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
    print(f"     Target: dispatch_wire_record (4,656 B) -> ≤ 1,200 B (Est. Saving: ~3,400 B)")
    print("     Action: Replace 30 inline facet serializers with const FacetDesc table.")
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
    json.dump({n: v[0] for n, v in symbols(a.elf).items() if v[1] in "tTrR"}, open(a.out, "w"), indent=0)
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


def cmd_diff(a):
    old = json.load(open(a.base)); new = {n: v[0] for n, v in symbols(a.elf).items() if v[1] in "tTrR"}
    d = sorted(((new.get(k, 0) - old.get(k, 0), k) for k in set(old) | set(new)), key=lambda x: -abs(x[0]))
    print(f"total delta: {sum(new.values()) - sum(old.values()):+d} B")
    for dv, k in d[: a.top]:
        if dv:
            print(f"{dv:+6d}  {k}")


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("cmd", choices="audit stack fields dups outline mono calls consts strings crates baseline diff divs unaligned".split())
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
