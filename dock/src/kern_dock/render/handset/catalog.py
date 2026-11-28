"""Declarative handset catalogue of real photographed Quansheng hardware & developer bench views."""

from __future__ import annotations

from .spec import HandsetSpec

K1_MATRIX: list[list[tuple[str, str, str, str]]] = [
    [("menu", "MENU", "", ""), ("up", "_K1_LT_", "", ""), ("down", "_K1_GT_", "", ""), ("exit", "EXIT", "", "")],
    [("1", "1", "Band", ".,?"), ("2", "2", "A/B", "abc"), ("3", "3", "V/M", "def"), ("star", "*", "Scan", "_RET_")],
    [("4", "4", "FC", "ghi"), ("5", "5", "WX", "jkl"), ("6", "6", "H/L", "mno"), ("0", "0", "FM", "")],
    [("7", "7", "VOX", "pqrs"), ("8", "8", "R", "tuv"), ("9", "9", "Call", "wxyz"), ("f", "#", "_KEY_", "F")],
]

K5_BOTTOM_MATRIX: list[list[tuple[str, str, str, str]]] = [
    [("1", "1", "Band", ""), ("2", "2", "A/B", ""), ("3", "3", "VFO", "MR"), ("star", "*", "Scan", "")],
    [("4", "4", "FC", ""), ("5", "5", "NO", "AA"), ("6", "6", "H/", "M/L"), ("0", "0", "FM", "")],
    [("7", "7", "VOX", ""), ("8", "8", "R", ""), ("9", "9", "Call", ""), ("f", "F", "#", "_KEY_")],
]

K5_99_MATRIX: list[list[tuple[str, str, str, str]]] = [
    [("1", "1", "Step", ""), ("2", "2", "A/B", ""), ("3", "3", "VFO", "MR"), ("star", "*", "V/M", "Scan")],
    [("4", "4", "Vox", ""), ("5", "5", "W/N", ""), ("6", "6", "ABR", ""), ("0", "0", "Band", "SQL")],
    [("7", "7", "TDR", ""), ("8", "8", "Beep", ""), ("9", "9", "TOT", ""), ("f", "#", "_KEY_", "")],
]

K10_PRO_MATRIX: list[list[tuple[str, str, str, str]]] = [
    [("menu", "MENU", "", ""), ("up", "_K1_LT_", "", ""), ("down", "_K1_GT_", "", ""), ("exit", "EXIT", "", "")],
    [("1", "1", "Sql", ".,?"), ("2", "2", "Msg", "abc"), ("3", "3", "Mod", "def"), ("star", "*", "selc", "scan")],
    [("4", "4", "FC", "ghi"), ("5", "5", "Scr", "jkl"), ("6", "6", "WX", "mno"), ("0", "0", "_RADIO_", "")],
    [("7", "7", "H/L", "pqrs"), ("8", "8", "Spec", "tuv"), ("9", "9", "Call", "wxyz"), ("f", "#", "Dial", "lock")],
]

MODELS: dict[str, HandsetSpec] = {
    # --- UV-K1 Series ---
    "k1": HandsetSpec(
        id="k1",
        label="UV-K1(8)",
        chassis="k1-metal",
        palette="UV-K1",
        accent=(0.18, 0.56, 1.00),
        armor_stops=(
            (0.00, (0.46, 0.44, 0.41)),
            (0.28, (0.39, 0.37, 0.35)),
            (0.66, (0.31, 0.30, 0.28)),
            (1.00, (0.40, 0.38, 0.36)),
        ),
        body_tone=(0.10, 0.105, 0.115),
        size=(356, 548),
        antenna_label="A1",
        tagline="UHF/VHF Transceiver",
        description="Metallic armor · Blue trim",
    ),
    "k1-polymer": HandsetSpec(
        id="k1-polymer",
        label="UV-K1 Matte Black",
        chassis="k1-polymer",
        palette="UV-K1",
        accent=(0.18, 0.56, 1.00),
        armor_stops=(
            (0.00, (0.16, 0.165, 0.18)),
            (0.45, (0.12, 0.125, 0.14)),
            (1.00, (0.085, 0.09, 0.10)),
        ),
        body_tone=(0.095, 0.10, 0.11),
        size=(356, 548),
        antenna_label="A1",
        tagline="UHF/VHF Transceiver",
        description="Matte polymer · Mini Kong",
    ),
    "k1-clear": HandsetSpec(
        id="k1-clear",
        label="UV-K1(8) Transparent",
        chassis="k1-clear",
        palette="Ice White",
        accent=(0.16, 0.54, 0.98),
        armor_stops=(
            (0.00, (0.82, 0.86, 0.88)),
            (1.00, (0.64, 0.69, 0.72)),
        ),
        body_tone=(0.76, 0.80, 0.82),
        size=(356, 548),
        antenna_label="A1",
        description="Clear shell · Green PCB",
    ),
    # --- UV-K5 / UV-K6 / UV-5R Series ---
    "k5v3": HandsetSpec(
        id="k5v3",
        label="UV-K5 v3 / UV-K6",
        chassis="k5v3",
        palette="UV-K5",
        accent=(0.98, 0.46, 0.06),
        armor_stops=(
            (0.00, (0.46, 0.46, 0.45)),
            (0.16, (0.78, 0.77, 0.75)),
            (0.50, (0.64, 0.63, 0.61)),
            (0.84, (0.75, 0.74, 0.72)),
            (1.00, (0.43, 0.43, 0.42)),
        ),
        body_tone=(0.115, 0.12, 0.135),
        size=(348, 598),
        description="Brushed silver · Amber LCD",
    ),
    "k5-orange": HandsetSpec(
        id="k5-orange",
        label="UV-K5(8) Safety Orange",
        chassis="k5v3",
        palette="UV-5R Blue",
        accent=(0.98, 0.44, 0.08),
        armor_stops=(
            (0.00, (0.28, 0.29, 0.31)),
            (0.18, (0.44, 0.45, 0.47)),
            (0.50, (0.34, 0.35, 0.37)),
            (0.84, (0.42, 0.43, 0.45)),
            (1.00, (0.24, 0.25, 0.27)),
        ),
        body_tone=(0.94, 0.39, 0.10),
        size=(348, 598),
        description="Safety orange · Blue LCD",
    ),
    "k5-black": HandsetSpec(
        id="k5-black",
        label="UV-K5 Diagonal Grille",
        chassis="k5-black",
        palette="Ice White",
        accent=(0.98, 0.46, 0.06),
        armor_stops=((0.0, (0.14, 0.15, 0.16)), (1.0, (0.09, 0.10, 0.11))),
        body_tone=(0.105, 0.11, 0.12),
        size=(348, 598),
        description="Matte black · White LCD",
    ),
    "k5-99": HandsetSpec(
        id="k5-99",
        label="UV-K5(99) Charcoal",
        chassis="k5-99",
        palette="UV-K5",
        accent=(0.98, 0.56, 0.08),
        armor_stops=((0.0, (0.42, 0.44, 0.46)), (0.5, (0.31, 0.33, 0.35)), (1.0, (0.22, 0.24, 0.26))),
        body_tone=(0.10, 0.105, 0.115),
        size=(348, 598),
        description="Charcoal armor · 4-slat",
    ),
    "k5-99-red": HandsetSpec(
        id="k5-99-red",
        label="UV-K5(99) Red",
        chassis="k5-99",
        palette="UV-K5",
        accent=(0.98, 0.56, 0.08),
        armor_stops=((0.0, (0.86, 0.14, 0.14)), (0.5, (0.72, 0.09, 0.10)), (1.0, (0.54, 0.06, 0.07))),
        body_tone=(0.10, 0.105, 0.115),
        size=(348, 598),
        description="Crimson armor · 4-slat",
    ),
    "uv5r-plus": HandsetSpec(
        id="uv5r-plus",
        label="UV-5R Plus",
        chassis="uv5r-plus",
        palette="UV-5R Blue",
        accent=(0.96, 0.44, 0.10),
        armor_stops=((0.0, (0.16, 0.17, 0.19)), (1.0, (0.10, 0.11, 0.12))),
        body_tone=(0.11, 0.115, 0.128),
        size=(348, 594),
        description="Blue LCD · Dot grille",
    ),
    # --- UV-K10 Series ---
    "k10-pro": HandsetSpec(
        id="k10-pro",
        label="UV-K10 Pro",
        chassis="k10-pro",
        palette="K10 Warm",
        accent=(0.12, 0.70, 0.95),
        armor_stops=((0.0, (0.58, 0.56, 0.51)), (0.5, (0.44, 0.42, 0.39)), (1.0, (0.31, 0.30, 0.28))),
        body_tone=(0.21, 0.27, 0.36),
        size=(348, 584),
        description="Slate blue · Warm LCD",
    ),
    # --- Developer Bench Hardware Views ---
    "k1-naked": HandsetSpec(
        id="k1-naked",
        label="UV-K1 Open Shell",
        chassis="k1-naked",
        palette="UV-K1",
        accent=(0.18, 0.56, 1.00),
        armor_stops=((0.0, (0.14, 0.15, 0.16)), (1.0, (0.09, 0.10, 0.11))),
        body_tone=(0.095, 0.10, 0.11),
        size=(356, 548),
        antenna_label="A1",
        description="No cover · Snap domes",
    ),
    "pcb-r51": HandsetSpec(
        id="pcb-r51",
        label="R51-V2.1 Bare PCB",
        chassis="pcb-r51",
        palette="Classic Green",
        accent=(0.98, 0.62, 0.12),
        armor_stops=((0.0, (0.11, 0.43, 0.21)), (0.5, (0.09, 0.36, 0.17)), (1.0, (0.07, 0.30, 0.14))),
        body_tone=(0.09, 0.38, 0.18),
        size=(340, 574),
        description="Green PCB · SWD pads",
    ),
}

MODEL_ALIASES: dict[str, str] = {
    "k6": "k5v3",
    "k5": "k5v3",
    "k5-8": "k5v3",
    "k5(8)": "k5v3",
    "uv-k1": "k1",
    "uv-k5": "k5v3",
    "uv-k5v3": "k5v3",
    "uv-k6": "k5v3",
    "k599": "k5-99",
    "uv-k5-99": "k5-99",
    "k10": "k10-pro",
    "uv-k10": "k10-pro",
    "uv5r": "uv5r-plus",
    "naked": "k1-naked",
    "pcb": "pcb-r51",
    "r51": "pcb-r51",
}


def register_model(spec: HandsetSpec) -> None:
    """Register or override a declarative handset model at runtime (e.g. from a plugin)."""
    MODELS[spec.id] = spec


def radio_model(model: str) -> HandsetSpec:
    key = MODEL_ALIASES.get(model.lower(), model)
    return MODELS.get(key, MODELS["k1"])


def compute_handset_geometry(model: str) -> tuple[int, int]:
    return radio_model(model).size
