"""Desktop environment integration: .desktop & hicolor icons installer, external terminal & xdg-open."""

from __future__ import annotations

import os
import shlex
import shutil
import subprocess
import sys
from pathlib import Path
from typing import Any

from .. import APP_ID, ASSETS_DIR, REPO_ROOT
from .icons import ICON_COLOR_PATH, ICON_SYMBOLIC_PATH

TERMINAL_CANDIDATES: tuple[tuple[str, tuple[str, ...]], ...] = (
    ("ptyxis", ("--working-directory", "{cwd}")),
    ("kgx", ("--working-directory={cwd}",)),
    ("gnome-terminal", ("--working-directory={cwd}",)),
    ("konsole", ("--workdir", "{cwd}")),
    ("kitty", ("--directory", "{cwd}")),
    ("alacritty", ("--working-directory", "{cwd}")),
    ("foot", ("--working-directory={cwd}",)),
    ("wezterm", ("start", "--cwd", "{cwd}")),
    ("xfce4-terminal", ("--working-directory={cwd}",)),
    ("xterm", ()),
)


def _xdg_data_home() -> Path:
    return Path(os.environ.get("XDG_DATA_HOME") or (Path.home() / ".local" / "share"))


def desktop_paths() -> dict[str, Path]:
    data = _xdg_data_home()
    return {
        "desktop": data / "applications" / f"{APP_ID}.desktop",
        "icon_color": data / "icons" / "hicolor" / "scalable" / "apps" / f"{APP_ID}.svg",
        "icon_symbolic": data / "icons" / "hicolor" / "scalable" / "status" / f"{APP_ID}-symbolic.svg",
    }


def desktop_integration_status() -> dict[str, Any]:
    paths = desktop_paths()
    installed = all(p.is_file() for p in paths.values())
    return {
        "installed": installed,
        "desktop_file": str(paths["desktop"]),
        "icon_color": str(paths["icon_color"]),
        "icon_symbolic": str(paths["icon_symbolic"]),
        "terminal": detect_terminal(),
    }


def install_desktop_integration() -> dict[str, str]:
    """Install org.kernos.Dock.desktop and scalable SVG icons into XDG_DATA_HOME."""
    paths = desktop_paths()
    for p in paths.values():
        p.parent.mkdir(parents=True, exist_ok=True)

    kern_bin = REPO_ROOT / "kern"
    if kern_bin.is_file():
        exec_cmd = f"{shlex.quote(str(kern_bin))} dock"
    else:
        exec_cmd = f"{shlex.quote(sys.executable)} -m kern_dock"

    tmpl = (ASSETS_DIR / f"{APP_ID}.desktop.in").read_text(encoding="utf-8")
    paths["desktop"].write_text(tmpl.replace("{exec_cmd}", exec_cmd), encoding="utf-8")
    paths["desktop"].chmod(0o755)
    shutil.copy2(ICON_COLOR_PATH, paths["icon_color"])
    shutil.copy2(ICON_SYMBOLIC_PATH, paths["icon_symbolic"])

    for cmd in (
        ["update-desktop-database", str(paths["desktop"].parent)],
        ["gtk-update-icon-cache", "-f", "-t", str(_xdg_data_home() / "icons" / "hicolor")],
    ):
        if shutil.which(cmd[0]):
            try:
                subprocess.run(cmd, capture_output=True, timeout=5)
            except Exception:
                pass
    return {k: str(v) for k, v in paths.items()}


def uninstall_desktop_integration() -> list[str]:
    removed: list[str] = []
    for p in desktop_paths().values():
        if p.is_file():
            p.unlink()
            removed.append(str(p))
    return removed


def detect_terminal(preferred: str = "auto") -> str | None:
    if preferred and preferred != "auto":
        tok = shlex.split(preferred)[0]
        if shutil.which(tok):
            return preferred
    env_term = os.environ.get("TERMINAL")
    if env_term and shutil.which(env_term):
        return env_term
    for binary, _args in TERMINAL_CANDIDATES:
        if shutil.which(binary):
            return binary
    return None


def build_radio_shell_env(extra: dict[str, str] | None = None) -> dict[str, str]:
    """Build environment variables exposing Kern CLI and active radio context to shells."""
    env = dict(os.environ)
    repo_str = str(REPO_ROOT)
    cur_path = env.get("PATH", "")
    if repo_str not in cur_path.split(os.pathsep):
        env["PATH"] = f"{repo_str}{os.pathsep}{cur_path}" if cur_path else repo_str
    env["KERN_REPO"] = repo_str
    if extra:
        for k, v in extra.items():
            if v is not None:
                env[k] = str(v)
    return env


def launch_external_terminal(
    *,
    preferred: str = "auto",
    cwd: Path | None = None,
    env_extra: dict[str, str] | None = None,
) -> str:
    """Open the user's native OS terminal preloaded with Kern Dock environment variables."""
    target_cwd = str(cwd or REPO_ROOT)
    env = build_radio_shell_env(env_extra)
    chosen = detect_terminal(preferred)
    if not chosen:
        raise RuntimeError("No system terminal emulator found on PATH")

    parts = shlex.split(chosen)
    binary = parts[0]
    args = list(parts[1:])
    if not args:
        for cand, tmpl_args in TERMINAL_CANDIDATES:
            if Path(binary).name == cand:
                args = [a.format(cwd=target_cwd) for a in tmpl_args]
                break

    subprocess.Popen([binary, *args], cwd=target_cwd, env=env, start_new_session=True)
    return binary


def open_path_in_os(target: str | Path) -> bool:
    """Open a local path or URL using the OS desktop handler."""
    t = str(target)
    for opener in ("xdg-open", "gio"):
        if shutil.which(opener):
            cmd = [opener, "open", t] if opener == "gio" else [opener, t]
            try:
                subprocess.Popen(cmd, start_new_session=True)
                return True
            except Exception:
                continue
    return False
