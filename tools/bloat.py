#!/usr/bin/env python3
"""Kern flash-bloat analyzer: finds *actionable* refactor candidates in the release ELF.

Subcommands
  dups      identical / near-identical functions (normalized Thumb disassembly) -> merge or share
  outline   repeated instruction sequences across functions -> extract helper / data table
  mono      generic instantiations grouped by base path -> de-monomorphize candidates
  calls     call-site fan-in x setup cost; large single-caller fns (inlining monoliths)
  consts    32-bit literals repeated across functions -> shared tables/enums
  strings   duplicate / near-duplicate rodata strings -> string pool
  crates    .text/.rodata rollup per crate::module (depth configurable)
  baseline  save per-symbol sizes to JSON;  diff  compare current ELF to a baseline
Usage: tools/bloat.py <cmd> [--elf PATH] [--top N] [...]
"""
import argparse, collections, hashlib, json, re, subprocess, sys

DEF_ELF = "target/thumbv6m-none-eabi/release/firmware"
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


def cmd_diff(a):
    old = json.load(open(a.base)); new = {n: v[0] for n, v in symbols(a.elf).items() if v[1] in "tTrR"}
    d = sorted(((new.get(k, 0) - old.get(k, 0), k) for k in set(old) | set(new)), key=lambda x: -abs(x[0]))
    print(f"total delta: {sum(new.values()) - sum(old.values()):+d} B")
    for dv, k in d[: a.top]:
        if dv:
            print(f"{dv:+6d}  {k}")


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("cmd", choices="dups outline mono calls consts strings crates baseline diff".split())
    p.add_argument("--elf", default=DEF_ELF); p.add_argument("--top", type=int, default=25)
    p.add_argument("--min", type=int, default=24, help="min fn bytes"); p.add_argument("--sim", type=float, default=0.80)
    p.add_argument("--len", type=int, default=8, help="outline window (instructions)")
    p.add_argument("--count", type=int, default=3, help="min occurrences for outline")
    p.add_argument("--depth", type=int, default=3); p.add_argument("--out", default="docs/flash_baseline.json")
    p.add_argument("--base", default="docs/flash_baseline.json")
    a = p.parse_args()
    if a.cmd == "calls" and a.min == 24:
        a.min = 400
    globals()["cmd_" + a.cmd](a)


if __name__ == "__main__":
    sys.exit(main())
