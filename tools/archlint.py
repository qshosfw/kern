#!/usr/bin/env python3
"""Kern architecture linter: detects *design* smells in Rust sources and maps each to a refactor pattern.

Complements tools/bloat.py (binary-level). Heuristic (regex) by design: zero dependencies, fast,
good enough to rank where an agent should spend architectural effort.

  python3 tools/archlint.py report          # scorecard per module + top findings (start here)
  python3 tools/archlint.py plan            # generate Markdown action checklist for agents
  python3 tools/archlint.py <check> [--top N]
checks: layers statics tables enums shapes params generics god inline cfg labels dupfn stubs
Each finding prints:  [PATTERN] location  evidence   -> pattern names are defined in
docs/flash_audit_measured_plan.md §11 (Refactor Pattern Catalogue).
"""
import argparse, collections, hashlib, itertools, json, os, re, sys

ROOT = "src"
# Allowed dependency direction (crate -> crates it may `use`). Anything else is a layering violation.
LAYERS = {
    "pac": {"cortex_m"}, "traits": set(), "utils": {"cortex_m"}, "crypto": {"utils"},
    "hal": {"pac", "traits", "utils", "cortex_m"}, "bsp": {"hal", "pac", "traits", "utils", "cortex_m"},
    "kernfs": {"utils", "crypto", "traits"}, "calib": {"utils", "crypto"},
    "kernel": {"traits", "utils", "crypto", "kernfs", "cortex_m"},
    "gfx": {"kernel", "utils", "traits"},
    "drivers": {"hal", "bsp", "pac", "traits", "utils", "kernel", "kernfs", "crypto", "calib", "cortex_m"},
    "firmware": None,  # integration crate: anything, but see APP_RULES
}
CRATE_NAMES = {"driver": "drivers", "py32_hal": "hal", "py32f071_pac": "pac"}
EXTERN = {"py32_hal": "hal", "py32f071_pac": "pac", "drivers": "drivers", "kernel": "kernel", "gfx": "gfx",
          "kernfs": "kernfs", "utils": "utils", "crypto": "crypto", "bsp": "bsp", "traits": "traits", "calib": "calib",
          "cortex_m": "cortex_m"}
# Module-level rules inside firmware & drivers (AGENTS.md: apps never touch hardware; chip drivers are portable)
APP_RULES = [
    (r"firmware/src/apps/", {"drivers", "hal", "pac", "bsp", "cortex_m"}, "APPS-TOUCH-HW"),
    (
        r"driver/src/(st7565|py25q16|bk4829|bk1080|backlight|uart|wire|audio|battery|filesystem|gnss)",
        {"hal", "pac", "bsp"},
        "DRIVER-TOUCH-MCU",
    ),
]


def files():
    for d, _, fs in os.walk(ROOT):
        if "/target" in d or "/tests" in d:
            continue
        for f in fs:
            if f.endswith(".rs"):
                p = os.path.join(d, f)
                yield p, open(p, encoding="utf-8", errors="ignore").read()


def crate_of(path):
    parts = path.split("/")
    i = parts.index("src", 1) if "src" in parts[1:] else 2
    name = parts[i - 1]
    return CRATE_NAMES.get(name, name)


def strip_tests(src):
    i = src.find("#[cfg(test)]")
    return src if i < 0 else src[:i]


def fn_bodies(src):
    """Yield (name, generics, params, body, line) for each fn via brace matching."""
    for m in re.finditer(r"\bfn\s+(\w+)\s*(<[^>{]*>)?\s*\(([^)]*)\)[^{;]*\{", src):
        depth, i = 1, m.end()
        while depth and i < len(src):
            c = src[i]; depth += (c == "{") - (c == "}"); i += 1
        yield m.group(1), m.group(2) or "", m.group(3), src[m.end():i], src.count("\n", 0, m.start()) + 1


def out(tag, loc, msg):
    print(f"[{tag}] {loc}  {msg}")


# ---------------------------------------------------------------- checks
def chk_layers(a, F):
    n = 0
    for p, s in F:
        c = crate_of(p); allowed = LAYERS.get(c)
        for ln, line in enumerate(strip_tests(s).splitlines(), 1):
            m = re.match(r"\s*(?:pub\s+)?use\s+(\w+)::", line) or re.search(r"\b(drivers|py32_hal|py32f071_pac|bsp|cortex_m)::\w", line)
            if not m:
                continue
            dep = EXTERN.get(m.group(1))
            if not dep or dep == c:
                continue
            if allowed is not None and dep not in allowed:
                out("LAYER-INVERSION", f"{p}:{ln}", f"{c} -> {dep}"); n += 1
            for rx, banned, tag in APP_RULES:
                if re.search(rx, p) and dep in banned and not line.strip().startswith("//"):
                    out(tag, f"{p}:{ln}", line.strip()[:100]); n += 1
    return n


def chk_statics(a, F):
    per = collections.Counter(); n = 0
    for p, s in F:
        for ln, line in enumerate(strip_tests(s).splitlines(), 1):
            if re.search(r"\bstatic\s+mut\b", line):
                per[p] += 1; n += 1
                if getattr(a, "verbose", False):
                    out("GLOBAL-STATE", f"{p}:{ln}", line.strip()[:90])
    for p, c in per.most_common(a.top):
        out("GLOBAL-STATE", p, f"{c} `static mut` -> group into one owned context/singleton")
    return n


def chk_tables(a, F):
    """Policy-as-code: big matches, runs of same-callee calls, if-chains on same var."""
    rows = []
    for p, s in F:
        for name, _, _, body, ln in fn_bodies(strip_tests(s)):
            arms = len(re.findall(r"=>", body))
            calls = re.findall(r"(\w+(?:\.\w+)*)\s*\(", body)
            run = max((len(list(g)) for _, g in itertools.groupby(c.split(".")[-1] for c in calls)), default=0)
            same = collections.Counter(c.split(".")[-1] for c in calls).most_common(1)
            ifs = len(re.findall(r"\bif\s+\w+\s*==", body))
            score = arms + 2 * max(run - 3, 0) + (same[0][1] if same and same[0][1] >= 6 else 0) + max(ifs - 3, 0)
            if score >= a.min_score:
                rows.append((score, f"{p}:{ln}", name, arms, same[0] if same else ("", 0), ifs))
    for sc, loc, name, arms, (cal, k), ifs in sorted(rows, reverse=True)[: a.top]:
        out("DATA-NOT-CODE", loc, f"{name}: {arms} match arms, `{cal}`x{k}, {ifs} if==chains (score {sc}) -> const table + 1 interpreter")
    return len(rows)


def chk_enums(a, F):
    enums = []
    for p, s in F:
        for m in re.finditer(r"enum\s+(\w+)[^{]*\{([^}]*)\}", strip_tests(s)):
            vs = {re.sub(r"[^a-z]", "", v.split("(")[0].split("{")[0].split("=")[0].lower())
                  for v in m.group(2).split(",")}
            vs = {v for v in vs if v and not v.startswith("//")}
            if len(vs) >= 3:
                enums.append((m.group(1), p, vs))
    rows = []
    for (n1, p1, v1), (n2, p2, v2) in itertools.combinations(enums, 2):
        j = len(v1 & v2) / len(v1 | v2)
        if j >= a.sim and n1 != n2:
            rows.append((j, n1, p1, n2, p2, sorted(v1 & v2)[:6]))
    for j, n1, p1, n2, p2, common in sorted(rows, reverse=True)[: a.top]:
        out("PARALLEL-TYPES", f"{n1}({p1}) ~ {n2}({p2})", f"overlap {j:.2f} {common} -> one shared repr(u8) type")
    return len(rows)


def chk_shapes(a, F):
    shapes = collections.defaultdict(list)
    for p, s in F:
        for m in re.finditer(r"struct\s+(\w+)[^{;(]*\{([^}]*)\}", strip_tests(s)):
            types = tuple(t.strip() for t in re.findall(r":\s*([^,\n]+)", m.group(2)))
            if len(types) >= 3:
                shapes[types].append(f"{m.group(1)}({p})")
    n = 0
    for t, names in sorted(shapes.items(), key=lambda kv: -len(kv[0])):
        if len(names) > 1:
            out("PARALLEL-TYPES", " = ".join(names[:4]), f"same field shape {t[:5]} -> one type or a shared header"); n += 1
            if n >= a.top:
                break
    return n


def chk_params(a, F):
    rows = []
    for p, s in F:
        for name, _, params, _, ln in fn_bodies(strip_tests(s)):
            k = len([x for x in params.split(",") if ":" in x and "self" not in x])
            if k >= 5:
                rows.append((k, f"{p}:{ln}", name))
    for k, loc, name in sorted(rows, reverse=True)[: a.top]:
        out("PARAM-BLOCK", loc, f"{name}: {k} args (>4 spill to stack on AAPCS) -> pass a #[repr(C)] descriptor")
    return len(rows)


def chk_generics(a, F):
    n = 0
    for p, s in F:
        for im in re.finditer(r"impl(?:<[^>]*>)?\s+(\w+)[^{]*\{", strip_tests(s)):
            bounds = collections.Counter(g for _, g, _, _, _ in fn_bodies(s[im.end():im.end() + 20000]) if g)
            for g, c in bounds.items():
                if c >= 3:
                    out("GENERIC-ON-TYPE", f"{p} impl {im.group(1)}", f"{c} methods repeat `{g}` -> move param to the struct / &dyn at cold boundary"); n += 1
    return n


def chk_god(a, F):
    rows = []
    for p, s in F:
        L = s.count("\n")
        if L > 600:
            rows.append((L, "GOD-MODULE", p, f"{L} lines -> split by responsibility"))
        for name, _, _, body, ln in fn_bodies(strip_tests(s)):
            bl = body.count("\n")
            if bl > 90:
                rows.append((bl, "GOD-FN", f"{p}:{ln}", f"{name}: {bl} lines -> split per state/step"))
        for m in re.finditer(r"struct\s+(\w+)[^{;(]*\{([^}]*)\}", s):
            k = len(re.findall(r"^\s*(?:pub(?:\([^)]*\))?\s+)?\w+\s*:", m.group(2), re.M))
            if k >= 14:
                rows.append((k * 10, "GOD-STRUCT", f"{p}:{m.group(1)}", f"{k} fields -> split by concern, hot fields first (Thumb imm5)"))
    for _, tag, loc, msg in sorted(rows, reverse=True)[: a.top]:
        out(tag, loc, msg)
    return len(rows)


def chk_inline(a, F):
    n = 0
    for p, s in F:
        for m in re.finditer(r"#\[inline\(always\)\]\s*(?:pub(?:\([^)]*\))?\s+)?(?:const\s+)?(?:unsafe\s+)?(?:extern\s+\"C\"\s+)?fn\s+(\w+)", s):
            body = next((b for nm, _, _, b, _ in fn_bodies(s[m.start():m.start() + 8000]) if nm == m.group(1)), "")
            if body.count(";") > 4:
                out("INLINE-POLICY", f"{p}:{m.group(1)}", f"inline(always) on {body.count(';')}-stmt body -> duplicated at every call site"); n += 1
    return n


def chk_cfg(a, F):
    per = collections.Counter({p: len(re.findall(r"#\[cfg\(\s*(?:not\()?\s*(?:any\(|all\()?\s*feature", s)) for p, s in F})
    for p, c in per.most_common(a.top):
        if c >= 8:
            out("CFG-SPRAWL", p, f"{c} feature cfgs -> gate whole modules/registry entries, not statements")
    return sum(1 for c in per.values() if c >= 8)


def chk_labels(a, F):
    rows = [(p, ln) for p, s in F for name, _, _, body, ln in fn_bodies(strip_tests(s))
            if name in ("as_str", "label", "name", "to_str", "describe") and body.count('"') >= 8]
    for p, ln in rows[: a.top]:
        out("STRING-SPRAWL", f"{p}:{ln}", "enum->text match; centralize in one status/label string pool")
    return len(rows)


def chk_dupfn(a, F):
    """Source-level clone detection: normalized fn bodies (identifiers -> ID, literals -> N)."""
    h = collections.defaultdict(list)
    for p, s in F:
        for name, _, _, body, ln in fn_bodies(strip_tests(s)):
            if body.count(";") < 4:
                continue
            nb = re.sub(r"\b[0-9][0-9a-fx_]*\b", "N", body)
            nb = re.sub(r'"[^"]*"', "S", nb)
            nb = re.sub(r"\b[a-z_]\w*\b", "id", nb)
            nb = re.sub(r"\s+", "", nb)
            h[hashlib.sha1(nb.encode()).hexdigest()].append(f"{p}:{ln}:{name}")
    n = 0
    for k, v in sorted(h.items(), key=lambda kv: -len(kv[1])):
        if len(v) > 1:
            out("CLONE", " | ".join(v[:4]), f"x{len(v)} structurally identical -> one generic-free fn over data"); n += 1
            if n >= a.top:
                break
    return n


def chk_stubs(a, F):
    """Find empty, trivial or noop functions that can be consolidated into 1 stub."""
    stubs = []
    for p, s in F:
        for name, _, _, body, ln in fn_bodies(strip_tests(s)):
            clean = body.strip().replace(" ", "").replace("\n", "")
            if clean in ("", "{}", "0", "{0}", "true", "{true}", "false", "{false}", "None", "{None}", "Ok(())", "{Ok(())}") or name.startswith("noop_"):
                stubs.append((p, ln, name, clean))
    print(f"== found {len(stubs)} dummy / stub functions ==")
    for p, ln, name, body in stubs[: a.top]:
        out("STUB-SPRAWL", f"{p}:{ln}", f"`{name}` body `{body}` -> consolidate into 1 universal stub or NULL binding")
    return len(stubs)


def chk_packed(a, F):
    """Find #[repr(C, packed)] and #[repr(packed)] structs causing unaligned software traps on ARMv6-M."""
    packed = []
    for p, s in F:
        for ln, line in enumerate(strip_tests(s).splitlines(), 1):
            if re.search(r"#\[repr\([^)]*packed[^)]*\)\]", line):
                packed.append((p, ln, line.strip()))
    print(f"== found {len(packed)} packed structs generating unaligned software traps ==")
    for p, ln, line in packed[: a.top]:
        out("PACKED-STRUCT", f"{p}:{ln}", f"`{line}` -> use naturally aligned #[repr(C)] + size assert to kill __aeabi_uread/write")
    return len(packed)


def chk_divs(a, F):
    """Find division and modulo operators causing software emulation calls on Cortex-M0+."""
    divs = []
    for p, s in F:
        for ln, line in enumerate(strip_tests(s).splitlines(), 1):
            ls = line.split("//")[0].strip()
            if not ls or ls.startswith("/*") or ls.startswith("*"):
                continue
            if any(k in ls for k in ("http://", "https://", "use ", "<", ">")):
                continue
            m = re.search(r"\b([a-zA-Z0-9_.]+)\s*([/%])\s*([a-zA-Z0-9_.]+)", ls)
            if m and not m.group(1).startswith("http") and not m.group(3).startswith("/"):
                divs.append((p, ln, ls))
    print(f"== found {len(divs)} division/modulo sites ==")
    for p, ln, line in divs[: a.top]:
        out("SOFTWARE-DIV", f"{p}:{ln}", f"`{line[:80]}` -> replace with reciprocal multiply / power-of-two shift")
    return len(divs)


CHECKS = {k[4:]: v for k, v in globals().items() if k.startswith("chk_")}
CHECKS["clone"] = CHECKS["dupfn"]  # alias


def plan(a, F):
    """Generate prioritized Markdown action plan for agents to execute directly."""
    print("# Kern Architectural Refactoring Action Checklist")
    print("> Generated by `tools/archlint.py plan`. Follow §11-12 of `flash_audit_measured_plan.md`.")
    print()
    print("## High-Priority Architectural Flaws")
    print()

    # 1. Layer Violations
    print("### 1. Fix Layering Violations in Apps (Apps touching hardware directly)")
    for p, s in F:
        for ln, line in enumerate(strip_tests(s).splitlines(), 1):
            for rx, banned, _ in APP_RULES:
                if re.search(rx, p) and any(b in line for b in banned) and not line.strip().startswith("//"):
                    print(f"- [ ] `{p}:{ln}`: `{line.strip()}` -> delegate through `SysContext`")
    print()

    # 2. Parallel Types
    print("### 2. Consolidate Parallel / Duplicate Enums & Structs")
    enums = []
    for p, s in F:
        for m in re.finditer(r"enum\s+(\w+)[^{]*\{([^}]*)\}", strip_tests(s)):
            vs = {re.sub(r"[^a-z]", "", v.split("(")[0].split("{")[0].split("=")[0].lower()) for v in m.group(2).split(",")}
            vs = {v for v in vs if v and not v.startswith("//")}
            if len(vs) >= 3:
                enums.append((m.group(1), p, vs))
    for (n1, p1, v1), (n2, p2, v2) in itertools.combinations(enums, 2):
        if n1 != n2 and (len(v1 & v2) / len(v1 | v2)) >= 0.6:
            print(f"- [ ] Merge `{n1}` ({p1}) and `{n2}` ({p2}) into one `#[repr(u8)]` type")
    print()

    # 3. Monolithic Match Tables -> Const Arrays
    print("### 3. Replace Monolithic Match Trees with Const Descriptor Tables (`DATA-NOT-CODE`)")
    matches = []
    for p, s in F:
        for name, _, _, body, ln in fn_bodies(strip_tests(s)):
            arms = len(re.findall(r"=>", body))
            if arms >= 20:
                matches.append((arms, f"{p}:{ln}", name))
    matches.sort(reverse=True)
    for arms, loc, name in matches[:8]:
        print(f"- [ ] `{loc}`: `{name}` ({arms} match arms) -> convert to `const [Descriptor; N]` + 1 interpreter loop")
    print()

    # 4. Param blocks
    print("### 4. Group Functions with >4 Arguments into #[repr(C)] Descriptors (`PARAM-BLOCK`)")
    params = []
    for p, s in F:
        for name, _, par, _, ln in fn_bodies(strip_tests(s)):
            k = len([x for x in par.split(",") if ":" in x and "self" not in x])
            if k >= 6:
                params.append((k, f"{p}:{ln}", name))
    params.sort(reverse=True)
    for k, loc, name in params[:8]:
        print(f"- [ ] `{loc}`: `{name}` ({k} arguments) -> group into `&ParamBlock` to eliminate AAPCS stack spills")
    print()

    # 5. Global Statics
    print("### 5. Group Scattered `static mut` into Owned Contexts (`GLOBAL-STATE`)")
    per = collections.Counter()
    for p, s in F:
        for line in strip_tests(s).splitlines():
            if re.search(r"\bstatic\s+mut\b", line):
                per[p] += 1
    for p, c in per.most_common(6):
        print(f"- [ ] `{p}`: {c} `static mut` -> group into single owned struct context or `critical_section`")
    print()


def report(a, F):
    import contextlib, io
    score = collections.Counter(); totals = {}
    for name, fn in CHECKS.items():
        if name in ("clone", "plan"):
            continue
        buf = io.StringIO()
        with contextlib.redirect_stdout(buf):
            totals[name] = fn(argparse.Namespace(**{**vars(a), "top": 10_000, "verbose": False}), F)
        for line in buf.getvalue().splitlines():
            for path in re.findall(r"(src/[\w/]+)\.rs", line):
                parts = [x for x in path.split("/") if x != "src"]
                score["/".join(parts[:3])] += 1
    print("══════════════════════════════════════════════════════════════════")
    print(" 🔎 KERN SOURCE ARCHITECTURE & DESIGN SMELL REPORT")
    print("══════════════════════════════════════════════════════════════════")
    print(" Smell Counts by Architectural Anti-Pattern:")
    for k, v in sorted(totals.items(), key=lambda kv: -kv[1]):
        print(f"  • {k:<12} {v:4d} occurrences")
    print("──────────────────────────────────────────────────────────────────")
    print(" Top Modules with Highest Design-Smell Density:")
    for m, c in score.most_common(a.top):
        print(f"  {c:4d}  {m}")
    print("══════════════════════════════════════════════════════════════════")
    print(" Run `python3 tools/archlint.py plan` to generate Markdown action items.")
    if a.json:
        with open(a.json, "w") as jf:
            json.dump({"totals": totals, "modules": score}, jf, indent=2)
        print(f"Saved report JSON to {a.json}")


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("cmd", choices=["report", "plan", *CHECKS]); p.add_argument("--top", type=int, default=20)
    p.add_argument("--sim", type=float, default=0.5); p.add_argument("--min-score", type=int, default=14)
    p.add_argument("--verbose", action="store_true"); p.add_argument("--json")
    a = p.parse_args(); F = list(files())
    if a.cmd == "report":
        report(a, F)
    elif a.cmd == "plan":
        plan(a, F)
    else:
        CHECKS[a.cmd](a, F)


if __name__ == "__main__":
    sys.exit(main())
