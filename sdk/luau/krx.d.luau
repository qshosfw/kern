--!strict
-- Kern OS Luau `.krx` SDK Type Definitions (`abi::VERSION = 5`)
-- Used by `luau-lsp` (VS Code / Neovim) and `tools/luau_krx.py` (Zero-GC Luau -> C89 -> .krx AOT).

export type Color = number
export type Font = number
export type Align = number
export type KeyCode = number
export type EventKind = number

export type KrxEvent = {
    kind: EventKind,
    key: KeyCode,
    flags: number,
    payload: number,
}

export type KrxGfx = {
    COLOR_CLEAR: Color,
    COLOR_FILL: Color,
    COLOR_INVERT: Color,
    FONT_MUMATRIX: Font,
    FONT_TOMTHUMB: Font,
    ALIGN_LEFT: Align,
    ALIGN_CENTER: Align,
    ALIGN_RIGHT: Align,

    clear: () -> (),
    commit: () -> (),
    pixel: (x: number, y: number, color: Color) -> (),
    line: (x0: number, y0: number, x1: number, y1: number, color: Color) -> (),
    rect: (x: number, y: number, w: number, h: number, color: Color) -> (),
    fill_rect: (x: number, y: number, w: number, h: number, color: Color) -> (),
    text: (x: number, y: number, s: string, font: Font?, color: Color?, align: Align?) -> (),
}

export type KrxRadio = {
    tune_hz: (freq_hz: number) -> number,
    rssi_dbm: () -> number,
    set_squelch: (level: number) -> number,
    is_carrier_open: () -> boolean,
}

export type KrxUi = {
    toast: (msg: string, duration_ms: number) -> (),
    s_meter: (x: number, y: number, rssi_dbm: number) -> (),
    progress: (x: number, y: number, w: number, h: number, pct: number) -> (),
}

export type KrxAudio = {
    beep: (freq_hz: number, duration_ms: number) -> (),
}

export type KrxSys = {
    EV_NONE: EventKind,
    EV_KEY: EventKind,
    EV_ENCODER: EventKind,
    EV_TICK: EventKind,
    EV_MODAL: EventKind,
    EV_IPC: EventKind,

    KEY_PTT: KeyCode,
    KEY_SIDE1: KeyCode,
    KEY_SIDE2: KeyCode,
    KEY_UP: KeyCode,
    KEY_DOWN: KeyCode,
    KEY_MENU: KeyCode,
    KEY_EXIT: KeyCode,
    KEY_F: KeyCode,

    ticks_ms: () -> number,
    prop_get: (prop_id: number) -> number,
    prop_set: (prop_id: number, val: number) -> number,
    udiv: (n: number, d: number) -> number,
    print: (msg: string) -> (),
}

declare krx: {
    gfx: KrxGfx,
    rf: KrxRadio,
    ui: KrxUi,
    audio: KrxAudio,
    sys: KrxSys,
}
