#!/usr/bin/env python3
"""
Shared cross-platform CLI foundation for Kern developer tools.

Adheres to clig.dev, no-color.org, and POSIX/GNU CLI conventions:
  • Zero third-party dependencies (pure Python 3.11+ stdlib).
  • Respects NO_COLOR, FORCE_COLOR / CLICOLOR_FORCE, TERM=dumb, and TTY detection.
  • Enables Windows 10/11 VT100 ANSI sequences & safe UTF-8 console output automatically.
  • Routes diagnostics/warnings/errors to stderr and structured data to stdout.
  • Provides clean exit codes (0 OK, 1 error, 2 usage, 130 SIGINT).
"""

from __future__ import annotations

import ctypes
import os
import platform
import shutil
import sys
from pathlib import Path
from typing import Callable, Iterable, Sequence, TextIO

EXIT_OK = 0
EXIT_ERR = 1
EXIT_USAGE = 2
EXIT_SIGINT = 130

_INITIALIZED = False


def init_console() -> None:
    """Enable Windows VT100 ANSI support and safe UTF-8 stdio without crashing on legacy codepages."""
    global _INITIALIZED
    if _INITIALIZED:
        return
    _INITIALIZED = True

    if os.name == "nt":
        try:
            k32 = ctypes.windll.kernel32  # type: ignore[attr-defined]
            for handle_id in (-11, -12):  # STD_OUTPUT_HANDLE, STD_ERROR_HANDLE
                h = k32.GetStdHandle(handle_id)
                mode = ctypes.c_uint32()
                if k32.GetConsoleMode(h, ctypes.byref(mode)):
                    k32.SetConsoleMode(h, mode.value | 0x0004)  # ENABLE_VIRTUAL_TERMINAL_PROCESSING
        except Exception:
            pass

    for stream in (sys.stdout, sys.stderr):
        try:
            if hasattr(stream, "reconfigure"):
                enc = getattr(stream, "encoding", "") or ""
                if enc.lower().replace("-", "") != "utf8":
                    stream.reconfigure(encoding="utf-8", errors="replace")
        except Exception:
            pass


def supports_unicode(stream: TextIO | None = None) -> bool:
    s = stream or sys.stdout
    enc = (getattr(s, "encoding", None) or "utf-8").lower().replace("-", "")
    return enc in ("utf8", "utf16", "utf32")


def use_color(stream: TextIO | None = None) -> bool:
    """Honors NO_COLOR (no-color.org), FORCE_COLOR, TERM=dumb, and stream TTY state."""
    if os.environ.get("NO_COLOR") is not None and os.environ.get("NO_COLOR") != "":
        return False
    if os.environ.get("FORCE_COLOR") or os.environ.get("CLICOLOR_FORCE"):
        return True
    if os.environ.get("TERM", "").lower() == "dumb":
        return False
    s = stream or sys.stdout
    try:
        return bool(s.isatty())
    except Exception:
        return False


def term_width(default: int = 80) -> int:
    try:
        return max(56, min(110, shutil.get_terminal_size((default, 24)).columns))
    except Exception:
        return default


# Dynamic ANSI code accessors
class _Ansi:
    RESET   = "\033[0m"
    BOLD    = "\033[1m"
    DIM     = "\033[2m"
    INVERT  = "\033[7m"
    RED     = "\033[31m"
    GREEN   = "\033[32m"
    YELLOW  = "\033[33m"
    BLUE    = "\033[34m"
    MAGENTA = "\033[35m"
    CYAN    = "\033[36m"
    WHITE   = "\033[37m"
    GRAY    = "\033[90m"


def style(text: str, *codes: str, stream: TextIO | None = None) -> str:
    if not codes or not use_color(stream):
        return text
    return f"{''.join(codes)}{text}{_Ansi.RESET}"


def bold(text: str, stream: TextIO | None = None) -> str: return style(text, _Ansi.BOLD, stream=stream)
def dim(text: str, stream: TextIO | None = None) -> str: return style(text, _Ansi.DIM, stream=stream)
def cyan(text: str, stream: TextIO | None = None) -> str: return style(text, _Ansi.CYAN, stream=stream)
def green(text: str, stream: TextIO | None = None) -> str: return style(text, _Ansi.GREEN, stream=stream)
def yellow(text: str, stream: TextIO | None = None) -> str: return style(text, _Ansi.YELLOW, stream=stream)
def red(text: str, stream: TextIO | None = None) -> str: return style(text, _Ansi.RED, stream=stream)
def blue(text: str, stream: TextIO | None = None) -> str: return style(text, _Ansi.BLUE, stream=stream)
def gray(text: str, stream: TextIO | None = None) -> str: return style(text, _Ansi.GRAY, stream=stream)


def ansi_map(stream: TextIO | None = None) -> dict[str, str]:
    """Return a dict of ANSI codes (empty strings when color is disabled)."""
    on = use_color(stream)
    return {
        "RESET": _Ansi.RESET if on else "",
        "BOLD": _Ansi.BOLD if on else "",
        "DIM": _Ansi.DIM if on else "",
        "INVERT": _Ansi.INVERT if on else "",
        "RED": _Ansi.RED if on else "",
        "GREEN": _Ansi.GREEN if on else "",
        "YELLOW": _Ansi.YELLOW if on else "",
        "BLUE": _Ansi.BLUE if on else "",
        "MAGENTA": _Ansi.MAGENTA if on else "",
        "CYAN": _Ansi.CYAN if on else "",
        "WHITE": _Ansi.WHITE if on else "",
        "GRAY": _Ansi.GRAY if on else "",
    }


def header(title: str, subtitle: str | None = None, *, stream: TextIO | None = None) -> None:
    init_console()
    out = stream or sys.stdout
    uni = supports_unicode(out)
    bolt = "⚡ " if uni else ">> "
    rule_ch = "─" if uni else "-"
    width = min(72, term_width() - 4)
    print(f"\n  {style(bolt + title, _Ansi.BOLD, _Ansi.CYAN, stream=out)}", file=out)
    if subtitle:
        print(f"  {style(subtitle, _Ansi.GRAY, stream=out)}", file=out)
    print(f"  {style(rule_ch * width, _Ansi.GRAY, stream=out)}", file=out)


def section(title: str, *, stream: TextIO | None = None) -> None:
    out = stream or sys.stdout
    print(f"\n  {style(title, _Ansi.BOLD, _Ansi.WHITE, stream=out)}", file=out)


def _badge(label: str, color: str, stream: TextIO) -> str:
    return style(f" {label:^5} ", color, _Ansi.INVERT, stream=stream)


def ok(msg: str, *, stream: TextIO | None = None) -> None:
    out = stream or sys.stdout
    print(f" {_badge('OK', _Ansi.GREEN, out)} {msg}", file=out)


def info(msg: str, *, stream: TextIO | None = None) -> None:
    out = stream or sys.stdout
    print(f" {_badge('INFO', _Ansi.BLUE, out)} {msg}", file=out)


def step(tag: str, msg: str, *, stream: TextIO | None = None) -> None:
    out = stream or sys.stdout
    print(f" {_badge(tag[:5].upper(), _Ansi.CYAN, out)} {msg}", file=out)


def warn(msg: str, *, stream: TextIO | None = None) -> None:
    out = stream or sys.stderr
    print(f" {_badge('WARN', _Ansi.YELLOW, out)} {msg}", file=out)


def err(msg: str, *, stream: TextIO | None = None) -> None:
    out = stream or sys.stderr
    print(f" {_badge('FAIL', _Ansi.RED, out)} {msg}", file=out)


def kv(label: str, value: str, *, width: int = 16, stream: TextIO | None = None) -> None:
    out = stream or sys.stdout
    print(f"  {style(f'{label:<{width}}', _Ansi.GRAY, stream=out)} {value}", file=out)


def table(
    headers: Sequence[str],
    rows: Iterable[Sequence[object]],
    *,
    right_align: Sequence[int] = (),
    indent: str = "  ",
    stream: TextIO | None = None,
) -> None:
    out = stream or sys.stdout
    mat = [[str(c) for c in r] for r in rows]
    widths = [len(h) for h in headers]
    for r in mat:
        for i, cell in enumerate(r):
            if i < len(widths):
                widths[i] = max(widths[i], len(cell))
    ra = set(right_align)
    uni = supports_unicode(out)

    hdr_cells = [
        f"{h:>{widths[i]}}" if i in ra else f"{h:<{widths[i]}}"
        for i, h in enumerate(headers)
    ]
    print(indent + style("  ".join(hdr_cells), _Ansi.BOLD, _Ansi.CYAN, stream=out), file=out)
    sep = ("─" if uni else "-") * (sum(widths) + 2 * (len(widths) - 1))
    print(indent + style(sep, _Ansi.GRAY, stream=out), file=out)
    for r in mat:
        cells = [
            f"{r[i]:>{widths[i]}}" if i in ra else f"{r[i]:<{widths[i]}}"
            for i in range(len(headers))
        ]
        print(indent + "  ".join(cells), file=out)


def progress_bar(
    current: int,
    total: int,
    *,
    width: int = 28,
    stats: str | None = None,
    stream: TextIO | None = None,
) -> str:
    out = stream or sys.stdout
    pct = (current / total) * 100.0 if total > 0 else 0.0
    val = max(0.0, min(100.0, pct))
    filled = int(width * val / 100.0)
    empty = width - filled
    uni = supports_unicode(out)
    bar = ("█" * filled + "░" * empty) if uni else ("#" * filled + "." * empty)
    col = _Ansi.GREEN if val > 60.0 else _Ansi.CYAN
    st = style(f" ({stats})", _Ansi.GRAY, stream=out) if stats else ""
    cnt = style(f" ({current}/{total})", _Ansi.GRAY, stream=out)
    return f"{style(f'[{bar}]', col, stream=out)} {style(f'{val:5.1f}%', _Ansi.BOLD, stream=out)}{st}{cnt}"


def cargo_env(extra: dict[str, str] | None = None) -> dict[str, str]:
    """Return a cross-platform subprocess environment with ~/.cargo/bin on PATH using os.pathsep."""
    env = os.environ.copy()
    cargo_bin = str(Path.home() / ".cargo" / "bin")
    cur_path = env.get("PATH", "")
    if cargo_bin not in cur_path.split(os.pathsep):
        env["PATH"] = f"{cargo_bin}{os.pathsep}{cur_path}" if cur_path else cargo_bin
    if extra:
        env.update(extra)
    return env


def is_usb_serial_device(device: str) -> bool:
    """Cross-platform check for common USB serial device names on Linux, macOS, and Windows."""
    low = device.lower()
    sys_name = platform.system()
    if sys_name == "Windows":
        return low.startswith("com")
    if sys_name == "Darwin":
        return "usbmodem" in low or "usbserial" in low or "slab_usbtouart" in low
    return "ttyacm" in low or "ttyusb" in low


def run_main(fn: Callable[[], int]) -> None:
    """Run a CLI main() with cross-platform console init and clean SIGINT/BrokenPipe exit codes."""
    init_console()
    try:
        code = fn()
        sys.exit(int(code) if isinstance(code, int) else EXIT_OK)
    except KeyboardInterrupt:
        print("", file=sys.stderr)
        warn("Interrupted by user (Ctrl+C)")
        sys.exit(EXIT_SIGINT)
    except BrokenPipeError:
        try:
            sys.stdout.close()
        except Exception:
            pass
        sys.exit(EXIT_OK)
