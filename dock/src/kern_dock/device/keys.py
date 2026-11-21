"""Declarative PC keyboard -> radio bindings and application shortcuts."""

from __future__ import annotations

KEY_BINDINGS: tuple[tuple[str, str, tuple[str, ...]], ...] = (
    ("menu", "Menu / OK", ("Return", "KP_Enter", "Menu")),
    ("exit", "Exit / Back", ("Escape", "Delete", "KP_Decimal", "KP_Delete")),
    ("up", "Up / Previous (<)", ("Up", "Left", "Page_Up", "KP_Add", "KP_Up", "KP_Left", "KP_Page_Up")),
    ("down", "Down / Next (>)", ("Down", "Right", "Page_Down", "KP_Subtract", "KP_Down", "KP_Right", "KP_Page_Down")),
    *((str(d), f"Digit {d}", (str(d), f"KP_{d}")) for d in range(10)),
    ("star", "Star / Scan", ("asterisk", "KP_Multiply")),
    ("f", "Function (F / #)", ("numbersign", "KP_Divide")),
    ("side1", "Side Key 1", ("F1",)),
    ("side2", "Side Key 2", ("F2",)),
    ("ptt", "Push-To-Talk (TX armed)", ("F7",)),
    ("torch", "Toggle Flashlight", ("F3",)),
    ("lock", "Toggle Keypad Lock", ("F4",)),
    ("vfo", "Swap VFO A/B", ("F5",)),
    ("headless", "Toggle Headless Mode", ("F6",)),
)

DOCK_ACTIONS = ("torch", "lock", "vfo", "headless")

_KEYVAL_LABELS = {
    "Return": "Enter", "KP_Enter": "Enter", "Escape": "Esc", "BackSpace": "⌫", "Delete": "Del",
    "Up": "↑", "Down": "↓", "Left": "←", "Right": "→", "Page_Up": "PgUp", "Page_Down": "PgDn",
    "asterisk": "*", "numbersign": "#", "slash": "/", "plus": "+", "minus": "-", "equal": "=",
    "KP_Multiply": "Num *", "KP_Add": "Num +", "KP_Subtract": "Num −", "KP_Divide": "Num /",
    "KP_Decimal": "Num .", "bracketleft": "[", "bracketright": "]", "space": "Space", "Menu": "☰",
}


def keyval_label(name: str) -> str:
    if name in _KEYVAL_LABELS:
        return _KEYVAL_LABELS[name]
    if name.startswith("KP_") and name[3:].isdigit():
        return f"Num {name[3:]}"
    return name.upper() if len(name) == 1 else name


def binding_index() -> dict[str, str]:
    """Flatten KEY_BINDINGS into {keyval name: action} with duplicate detection."""
    out: dict[str, str] = {}
    for action, _desc, names in KEY_BINDINGS:
        for nm in names:
            if nm in out:
                raise ValueError(f"Keyval '{nm}' bound twice ({out[nm]} / {action})")
            out[nm] = action
    return out


def resolve_binding(index: dict[str, str], name: str, base: str | None, shift: bool) -> tuple[str | None, bool]:
    """
    Resolve a pressed keyval to (action, long_press).
    Shifted symbols ('*' = Shift+8, '#' = Shift+3) keep their own binding;
    Shift on any other bound key triggers an instant long-press.
    """
    act = index.get(name)
    if act is not None:
        return act, shift and base == name
    for cand in (base, name.lower()):
        if cand and cand in index:
            return index[cand], shift
    return None, False


APP_SHORTCUTS: tuple[tuple[str, str, str], ...] = (
    ("command-palette", "Command Palette", "<Control>p"),
    ("type-text", "Type Text on Radio…", "<Control>t"),
    ("live-typing", "Live Typing Mode", "<Control><Shift>t"),
    ("toggle-keyboard", "Keyboard Control On / Off", "<Control>k"),
    ("toggle-handset", "Show / Hide Handset Stage", "<Control>h"),
    ("toggle-mini", "Compact Mini-Dock Mode", "<Control>m"),
    ("open-terminal", "Open External Terminal", "<Control><Shift>Return"),
    ("screenshot", "Save PNG Snapshot", "<Control>s"),
    ("toggle-record", "Start / Stop GIF Recording", "<Control>r"),
    ("preferences", "Preferences", "<Control>comma"),
    ("shortcuts", "Keyboard Shortcuts", "<Control>question"),
    ("quit", "Quit", "<Control>q"),
)
