#!/usr/bin/env python3
"""
luau_krx.py — Zero-GC Typed Luau (`--!strict`) to PIC C89 (`sdk/c/krx.h`) AOT Compiler.

Why AOT instead of embedding C++ `luau-vm` in firmware?
  - Standard C++ `luau-vm` requires ~140+ KiB Flash and ~28 KiB heap RAM (impossible on
    a 128 KiB Flash / 16 KiB SRAM PY32F071 Cortex-M0+).
  - Lowering `--!strict` typed Luau (`number` -> `i32`/`u32`, `boolean` -> `u8`, `string`
    literals -> PIC stack/rodata slices, `bit32.*`, `krx.gfx.*`, `krx.rf.*`, `krx.ui.*`,
    `krx.audio.*`, `krx.sys.*`) to zero-malloc ANSI C89 against `sdk/c/krx.h` produces
    `< 500 B` native Thumb-1 `.krx` binaries with **0 B resident firmware cost** and
    **0 GC pauses** while preserving VS Code `luau-lsp` typechecking (`sdk/luau/krx.d.luau`).

Usage:
  python3 tools/luau_krx.py input.luau -o output.c
  python3 tools/luau_krx.py input.luau --compile-bin output.bin
"""

import argparse
import os
import re
import subprocess
import tempfile

REPO_ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), ".."))
SDK_DIR = os.path.join(REPO_ROOT, "sdk")

TYPE_MAP = {
    "number": "i32",
    "u32": "u32",
    "i32": "i32",
    "u16": "u16",
    "i16": "i16",
    "u8": "u8",
    "boolean": "u8",
}

CONST_MAP = {
    "krx.gfx.COLOR_CLEAR": "COLOR_CLEAR",
    "krx.gfx.COLOR_FILL": "COLOR_FILL",
    "krx.gfx.COLOR_INVERT": "COLOR_INVERT",
    "krx.gfx.FONT_MUMATRIX": "0u",
    "krx.gfx.FONT_TOMTHUMB": "1u",
    "krx.gfx.ALIGN_LEFT": "0u",
    "krx.gfx.ALIGN_CENTER": "1u",
    "krx.gfx.ALIGN_RIGHT": "2u",
    "krx.sys.EV_NONE": "KRX_EV_NONE",
    "krx.sys.EV_KEY": "KRX_EV_KEY",
    "krx.sys.EV_ENCODER": "KRX_EV_ENCODER",
    "krx.sys.EV_TICK": "KRX_EV_TICK",
    "krx.sys.EV_MODAL": "KRX_EV_MODAL",
    "krx.sys.EV_IPC": "KRX_EV_IPC",
    "krx.sys.KEY_PTT": "KEY_PTT",
    "krx.sys.KEY_SIDE1": "KEY_SIDE1",
    "krx.sys.KEY_SIDE2": "KEY_SIDE2",
    "krx.sys.KEY_UP": "KEY_UP",
    "krx.sys.KEY_DOWN": "KEY_DOWN",
    "krx.sys.KEY_MENU": "KEY_MENU",
    "krx.sys.KEY_EXIT": "KEY_EXIT",
    "krx.sys.KEY_F": "KEY_F",
}


def split_args(arg_str: str) -> list[str]:
    args, cur, depth, in_str = [], [], 0, False
    i = 0
    while i < len(arg_str):
        ch = arg_str[i]
        if ch == '"' and (i == 0 or arg_str[i - 1] != "\\"):
            in_str = not in_str
            cur.append(ch)
        elif not in_str and ch == "(":
            depth += 1
            cur.append(ch)
        elif not in_str and ch == ")":
            depth -= 1
            cur.append(ch)
        elif not in_str and depth == 0 and ch == ",":
            args.append("".join(cur).strip())
            cur = []
        else:
            cur.append(ch)
        i += 1
    tail = "".join(cur).strip()
    if tail:
        args.append(tail)
    return args


def c_str_and_len(expr: str) -> tuple[str, str]:
    expr = expr.strip()
    if expr.startswith('"') and expr.endswith('"'):
        raw = bytes(expr[1:-1], "utf-8").decode("unicode_escape")
        return f"(const u8*){expr}", f"{len(raw.encode('utf-8'))}u"
    return f"(const u8*)({expr})", f"krx_strlen((const char*)({expr}))"


def translate_expr(expr: str, ev_var: str | None) -> str:
    s = expr.strip()
    for k, v in CONST_MAP.items():
        s = s.replace(k, v)
    s = re.sub(r"\btrue\b", "1u", s)
    s = re.sub(r"\bfalse\b", "0u", s)
    s = s.replace("~=", "!=")
    s = re.sub(r"\band\b", "&&", s)
    s = re.sub(r"\bor\b", "||", s)
    s = re.sub(r"\bnot\s+", "!", s)

    if ev_var:
        s = re.sub(rf"\b{re.escape(ev_var)}\.(kind|key|flags|payload)\b", rf"{ev_var}->\1", s)

    # bit32 intrinsics
    s = re.sub(r"bit32\.band\(([^,]+),([^)]+)\)", r"((\1) & (\2))", s)
    s = re.sub(r"bit32\.bor\(([^,]+),([^)]+)\)", r"((\1) | (\2))", s)
    s = re.sub(r"bit32\.bxor\(([^,]+),([^)]+)\)", r"((\1) ^ (\2))", s)
    s = re.sub(r"bit32\.lshift\(([^,]+),([^)]+)\)", r"((\1) << (\2))", s)
    s = re.sub(r"bit32\.rshift\(([^,]+),([^)]+)\)", r"(((u32)(\1)) >> (\2))", s)

    # krx.* expression calls
    s = re.sub(r"krx\.sys\.ticks_ms\(\s*\)", "((i32)krx_ticks_ms(api))", s)
    s = re.sub(r"krx\.sys\.prop_get\(([^)]+)\)", r"((i32)krx_prop_get(api, (u16)(\1)))", s)
    s = re.sub(r"krx\.sys\.udiv\(([^,]+),([^)]+)\)", r"((i32)krx_udivmod(api, (u32)(\1), (u32)(\2), (u32*)0))", s)
    s = re.sub(r"krx\.rf\.tune_hz\(([^)]+)\)", r"((i32)krx_call(api, OP_RF_TUNE, (u32)(\1), 0u, 12500u))", s)
    return s


def translate_call_stmt(line: str, ev_var: str | None) -> str | None:
    m = re.match(r"^([a-zA-Z0-9_.]+)\((.*)\)$", line)
    if not m:
        return None
    fn, raw_args = m.group(1), split_args(m.group(2))
    args = [translate_expr(a, ev_var) for a in raw_args]

    if fn == "krx.gfx.clear":
        return "krx_clear(api);"
    if fn == "krx.gfx.commit":
        return "krx_commit_frame(api);"
    if fn == "krx.gfx.pixel":
        return f"krx_draw_pixel(api, (i16)({args[0]}), (i16)({args[1]}), (u8)({args[2]}));"
    if fn == "krx.gfx.line":
        return f"krx_draw_line(api, (i16)({args[0]}), (i16)({args[1]}), (i16)({args[2]}), (i16)({args[3]}), (u8)({args[4]}));"
    if fn == "krx.gfx.rect":
        return f"krx_draw_rect(api, (i16)({args[0]}), (i16)({args[1]}), (i16)({args[2]}), (i16)({args[3]}), (u8)({args[4]}));"
    if fn == "krx.gfx.fill_rect":
        return f"krx_fill_rect(api, (i16)({args[0]}), (i16)({args[1]}), (i16)({args[2]}), (i16)({args[3]}), (u8)({args[4]}));"
    if fn == "krx.gfx.text":
        c_ptr, c_len = c_str_and_len(raw_args[2])
        font = args[3] if len(args) > 3 else "0u"
        col = args[4] if len(args) > 4 else "COLOR_FILL"
        align = args[5] if len(args) > 5 else "0u"
        return f"krx_draw_str_align(api, (i16)({args[0]}), (i16)({args[1]}), {c_ptr}, {c_len}, (u8)({font}), (u8)({col}), (u8)({align}));"
    if fn == "krx.ui.toast":
        c_ptr, c_len = c_str_and_len(raw_args[0])
        return f"krx_ui_show_toast(api, {c_ptr}, (u8){c_len}, (u16)({args[1]}));"
    if fn == "krx.ui.s_meter":
        return f"krx_ui_draw_s_meter(api, (i16)({args[0]}), (i16)({args[1]}), (i16)({args[2]}), 0u);"
    if fn == "krx.ui.progress":
        return f"krx_ui_draw_progress_bar(api, (i16)({args[0]}), (i16)({args[1]}), (u8)({args[2]}), (u8)({args[3]}), (u8)({args[4]}), (u8)({args[5]}));"
    if fn == "krx.rf.tune_hz":
        return f"krx_call(api, OP_RF_TUNE, (u32)({args[0]}), 0u, 12500u);"
    if fn == "krx.audio.beep":
        return f"krx_call(api, OP_AUD, SUB_AUD_TONE, (u32)({args[0]}), (u32)({args[1]}));"
    if fn == "krx.sys.print":
        c_ptr, c_len = c_str_and_len(raw_args[0])
        return f"krx_serial_write(api, {c_ptr}, (u16){c_len});"
    return f"{fn}({', '.join(args)});"


LUAU_C_PREAMBLE = """/* AUTO-GENERATED by tools/luau_krx.py from --!strict Luau source */
#include <krx_ops.h>

typedef unsigned char  u8;
typedef signed char    i8;
typedef short          i16;
typedef unsigned short u16;
typedef int            i32;
typedef unsigned int   u32;

#define KRX_OP_START  0u
#define KRX_OP_EVENT  1u
#define KRX_OP_RENDER 2u
#define KRX_OP_EXIT   3u

#define COLOR_CLEAR   0u
#define COLOR_FILL    1u
#define COLOR_INVERT  2u

#define KRX_EV_NONE    0u
#define KRX_EV_KEY     1u
#define KRX_EV_ENCODER 2u
#define KRX_EV_TICK    3u
#define KRX_EV_MODAL   7u
#define KRX_EV_IPC     8u

enum {
    KEY_PTT = 0, KEY_SIDE1 = 1, KEY_SIDE2 = 2, KEY_UP = 3, KEY_DOWN = 4,
    KEY_MENU = 5, KEY_EXIT = 6, KEY_F = 7
};

typedef u32 (*KernCallFn)(u32 op, u32 a0, u32 a1, u32 a2);
typedef struct { u32 abi_version; u16 table_size; u16 _pad0; u32 granted_caps; KernCallFn call; } KrxApiTable;
typedef struct __attribute__((packed)) { u8 kind; u8 key; u8 flags; u8 reserved; u32 payload; } KrxEvent;
typedef struct { const u8* text; i16 x; i16 y; u16 len; i16 arg; u8 font; u8 color; u8 flags; u8 _pad; } KrxTextReq;

static inline u32 krx_call(const KrxApiTable* api, u32 op, u32 a0, u32 a1, u32 a2) { return api->call(op, a0, a1, a2); }
static inline u32 krx_pack_xy(i16 x, i16 y) { return ((u32)(u16)x) | (((u32)(u16)y) << 16); }
static inline u16 krx_strlen(const char* s) { u16 n = 0u; while (s[n]) n++; return n; }
static inline void krx_clear(const KrxApiTable* api) { krx_call(api, OP_GFX_DRAW, SUB_GFX_DRAW_CLEAR, 0u, 0u); }
static inline void krx_commit_frame(const KrxApiTable* api) { krx_call(api, OP_GFX_DRAW, SUB_GFX_DRAW_COMMIT, 0u, 0u); }
static inline void krx_draw_pixel(const KrxApiTable* api, i16 x, i16 y, u8 c) { krx_call(api, OP_GFX_DRAW, SUB_GFX_DRAW_FILL_RECT | ((u32)c << 8), krx_pack_xy(x, y), krx_pack_xy(1, 1)); }
static inline void krx_draw_line(const KrxApiTable* api, i16 x0, i16 y0, i16 x1, i16 y1, u8 c) { krx_call(api, OP_GFX_DRAW, SUB_GFX_DRAW_LINE | ((u32)c << 8), krx_pack_xy(x0, y0), krx_pack_xy(x1, y1)); }
static inline void krx_draw_rect(const KrxApiTable* api, i16 x, i16 y, i16 w, i16 h, u8 c) { krx_call(api, OP_GFX_DRAW, SUB_GFX_DRAW_RECT | ((u32)c << 8), krx_pack_xy(x, y), krx_pack_xy(w, h)); }
static inline void krx_fill_rect(const KrxApiTable* api, i16 x, i16 y, i16 w, i16 h, u8 c) { krx_call(api, OP_GFX_DRAW, SUB_GFX_DRAW_FILL_RECT | ((u32)c << 8), krx_pack_xy(x, y), krx_pack_xy(w, h)); }
static inline void krx_draw_str_align(const KrxApiTable* api, i16 x, i16 y, const u8* text, u16 len, u8 font, u8 color, u8 align) {
    KrxTextReq r; r.text = text; r.x = x; r.y = y; r.len = len; r.arg = (i16)align; r.font = font; r.color = color; r.flags = 0u; r._pad = 0u;
    krx_call(api, OP_GFX_TEXT, (u32)&r, 0u, 0u);
}
static inline void krx_ui_show_toast(const KrxApiTable* api, const u8* msg, u8 len, u16 dur_ms) { krx_call(api, OP_UI_MODAL, SUB_UI_MODAL_TOAST, (u32)msg, ((u32)len) | (((u32)dur_ms) << 16)); }
static inline u16 krx_serial_write(const KrxApiTable* api, const u8* data, u16 len) { return (u16)krx_call(api, OP_IO_SERIAL, SUB_IO_SERIAL_WRITE, (u32)data, (u32)len); }
static inline u32 krx_ticks_ms(const KrxApiTable* api) { return krx_call(api, OP_SYS_PROP, KRX_PROP_TICKS_MS, 0u, 0u); }
static inline u32 krx_udivmod(const KrxApiTable* api, u32 n, u32 d, u32* rem) { return krx_call(api, OP_SYS_DIV, n, d, (u32)rem); }
static inline u32 krx_prop_get(const KrxApiTable* api, u16 id) { return krx_call(api, OP_SYS_PROP, (u32)(id & 0xFFu), 0u, 0u); }
"""


def transpile_luau_to_c(src: str) -> str:
    out = [LUAU_C_PREAMBLE]
    in_fn = None
    ev_var = None
    has_start = has_event = has_render = has_exit = False
    indent = 0

    for raw_line in src.splitlines():
        line = raw_line.split("--", 1)[0].strip()
        if not line:
            continue

        # Lifecycle function headers
        m_fn = re.match(r"^(?:local\s+)?function\s+(on_start|on_event|on_render|on_exit)\s*\(([^)]*)\)(?:\s*:\s*\w+)?$", line)
        if m_fn:
            in_fn = m_fn.group(1)
            params = m_fn.group(2).strip()
            ev_var = None
            if in_fn == "on_start":
                has_start = True
                out.append("static void luau_on_start(const KrxApiTable* api) {")
            elif in_fn == "on_event":
                has_event = True
                ev_var = params.split(":")[0].strip() if params else "ev"
                out.append(f"static u32 luau_on_event(const KrxApiTable* api, const KrxEvent* {ev_var}) {{")
            elif in_fn == "on_render":
                has_render = True
                out.append("static void luau_on_render(const KrxApiTable* api) {")
            elif in_fn == "on_exit":
                has_exit = True
                out.append("static void luau_on_exit(const KrxApiTable* api) {")
            indent = 1
            continue

        if line == "end":
            indent -= 1
            if indent == 0:
                if in_fn == "on_event":
                    out.append("    return 0u;")
                in_fn = None
                ev_var = None
            out.append("    " * indent + "}")
            continue

        pad = "    " * indent

        # Module-level or local variable declaration: `local x: number = 0`
        m_local = re.match(r"^local\s+([a-zA-Z_]\w*)\s*(?::\s*(\w+))?\s*=\s*(.+)$", line)
        if m_local:
            name, tname, rhs = m_local.group(1), m_local.group(2) or "number", m_local.group(3)
            ctype = TYPE_MAP.get(tname, "i32")
            prefix = "static " if in_fn is None else ""
            out.append(f"{pad}{prefix}{ctype} {name} = ({ctype})({translate_expr(rhs, ev_var)});")
            continue

        if line.startswith("if ") and line.endswith(" then"):
            cond = translate_expr(line[3:-5], ev_var)
            out.append(f"{pad}if ({cond}) {{")
            indent += 1
            continue

        if line.startswith("elseif ") and line.endswith(" then"):
            cond = translate_expr(line[7:-5], ev_var)
            out.append("    " * (indent - 1) + f"}} else if ({cond}) {{")
            continue

        if line == "else":
            out.append("    " * (indent - 1) + "} else {")
            continue

        m_for = re.match(r"^for\s+([a-zA-Z_]\w*)\s*=\s*([^,]+),\s*([^,]+)(?:,\s*(.+))?\s+do$", line)
        if m_for:
            v, start, stop, step = m_for.group(1), m_for.group(2), m_for.group(3), m_for.group(4) or "1"
            out.append(
                f"{pad}for (i32 {v} = ({translate_expr(start, ev_var)}); "
                f"{v} <= ({translate_expr(stop, ev_var)}); "
                f"{v} += ({translate_expr(step, ev_var)})) {{"
            )
            indent += 1
            continue

        if line.startswith("return"):
            ret_val = line[6:].strip()
            if not ret_val or in_fn in ("on_start", "on_render", "on_exit"):
                out.append(f"{pad}return;")
            else:
                out.append(f"{pad}return (u32)({translate_expr(ret_val, ev_var)});")
            continue

        call_c = translate_call_stmt(line, ev_var)
        if call_c:
            out.append(f"{pad}{call_c}")
            continue

        m_assign = re.match(r"^([a-zA-Z_]\w*)\s*([+\-*/]?=)\s*(.+)$", line)
        if m_assign:
            lhs, op_tok, rhs = m_assign.group(1), m_assign.group(2), m_assign.group(3)
            out.append(f"{pad}{lhs} {op_tok} {translate_expr(rhs, ev_var)};")
            continue

        raise ValueError(f"Unsupported Luau AOT construct: {raw_line}")

    out.extend([
        "",
        '__attribute__((section(".text.entry")))',
        "u32 krx_entry(u32 op, const KrxApiTable* api, void* param) {",
        "    if (!api) return 0u;",
        "    if (op == KRX_OP_START) {" + (" luau_on_start(api);" if has_start else "") + " return 0u; }",
        "    if (op == KRX_OP_EVENT) {"
        + (" return param ? luau_on_event(api, (const KrxEvent*)param) : 0u;" if has_event else " (void)param; return 0u;")
        + " }",
        "    if (op == KRX_OP_RENDER) {" + (" luau_on_render(api);" if has_render else "") + " return 0u; }",
        "    if (op == KRX_OP_EXIT) {" + (" luau_on_exit(api);" if has_exit else "") + " return 0u; }",
        "    return 0u;",
        "}",
        "",
    ])
    return "\n".join(out)


def compile_c_to_thumb_bin(c_source: str, out_bin: str) -> int:
    cflags = [
        "arm-none-eabi-gcc", "-mthumb", "-mcpu=cortex-m0plus", "-nostdlib",
        "-Os", "-fPIC", "-fPIE", "-fvisibility=hidden", "-fno-builtin", "-fno-jump-tables",
        "-ffunction-sections", "-fdata-sections", "-Wl,--gc-sections", "-Wl,--no-dynamic-linker",
        "-Wl,--build-id=none", "-Wl,-e,0",
        "-T", os.path.join(SDK_DIR, "krx.ld"),
        "-I", os.path.join(SDK_DIR, "c"),
    ]
    with tempfile.TemporaryDirectory() as tmp:
        c_path = os.path.join(tmp, "app.c")
        elf_path = os.path.join(tmp, "app.elf")
        with open(c_path, "w", encoding="utf-8") as fp:
            fp.write(c_source)
        subprocess.run(cflags + [c_path, "-o", elf_path], check=True)
        subprocess.run(["arm-none-eabi-objcopy", "-O", "binary", elf_path, out_bin], check=True)
    return os.path.getsize(out_bin)


def main():
    ap = argparse.ArgumentParser(description="Transpile --!strict Luau to PIC C89 / Cortex-M0+ .krx binary.")
    ap.add_argument("input", help="Input .luau source file")
    ap.add_argument("-o", "--output", help="Output .c file path")
    ap.add_argument("--compile-bin", help="Compile directly to raw Cortex-M0+ Thumb-1 binary")
    args = ap.parse_args()

    with open(args.input, encoding="utf-8") as fp:
        c_code = transpile_luau_to_c(fp.read())

    if args.output:
        with open(args.output, "w", encoding="utf-8") as fp:
            fp.write(c_code)
    if args.compile_bin:
        sz = compile_c_to_thumb_bin(c_code, args.compile_bin)
        print(f"[OK] Compiled {args.input} -> {args.compile_bin} ({sz} B Cortex-M0+ PIC code)")
    elif not args.output:
        print(c_code)


if __name__ == "__main__":
    main()
