//! Kern OS Freestanding Zig `.krx` SDK (`abi::VERSION = 5`).
//!
//! Zero-overhead, `#![no_std]`-equivalent Zig bindings over the single-entry
//! `KrxApiTable` (`kern_call`). Compiles directly with:
//!   `zig build-obj -target thumb-freestanding-eabi -mcpu cortex_m0plus -OReleaseSmall`

pub const ABI_VERSION: u32 = 5;

pub const OpLifecycle = enum(u32) {
    start  = 0,
    event  = 1,
    render = 2,
    exit   = 3,
};

pub const Color = enum(u8) {
    clear      = 0,
    fill       = 1,
    invert     = 2,
    dark_gray  = 3,
    light_gray = 4,
};

pub const EventKind = enum(u8) {
    none    = 0,
    key     = 1,
    encoder = 2,
    tick    = 3,
    bus     = 4,
    usb     = 5,
    power   = 6,
    modal   = 7,
    ipc     = 8,
};

pub const Key = enum(u8) {
    ptt   = 0,
    side1 = 1,
    side2 = 2,
    up    = 3,
    down  = 4,
    menu  = 5,
    exit  = 6,
    f     = 7,
    d0    = 8,
    d1    = 9,
    d2    = 10,
    d3    = 11,
    d4    = 12,
    d5    = 13,
    d6    = 14,
    d7    = 15,
    d8    = 16,
    d9    = 17,
    star  = 18,
    hash  = 19,
};

pub const Op = struct {
    pub const SYS_INFO: u32   = 0x00;
    pub const SYS_QUERY: u32  = 0x01;
    pub const SYS_DIV: u32    = 0x02;
    pub const SYS_REBOOT: u32 = 0x03;
    pub const PROP_GET: u32   = 0x06;
    pub const PROP_SET: u32   = 0x07;
    pub const RF_TUNE: u32    = 0x40;
    pub const RF_GET: u32     = 0x41;
    pub const RF_SET: u32     = 0x42;
    pub const RF_VFO: u32     = 0x44;
    pub const AUD_TONE: u32   = 0x50;
    pub const GFX_CLEAR: u32  = 0x60;
    pub const GFX_DRAW: u32   = 0x61;
    pub const GFX_TEXT: u32   = 0x62;
    pub const GFX_COMMIT: u32 = 0x63;
    pub const UI_MODAL: u32   = 0x70;
    pub const UI_STATUS: u32  = 0x71;
    pub const UI_WIDGET: u32  = 0x72;
    pub const IO_KEY: u32     = 0x80;
    pub const IO_POWER: u32   = 0x81;
    pub const IO_SERIAL: u32  = 0x82;
    pub const IO_TIME: u32    = 0x83;
};

pub const SubGfxDraw = struct {
    pub const LINE: u32      = 0;
    pub const RECT: u32      = 1;
    pub const FILL_RECT: u32 = 2;
};

pub const KernCallFn = *const fn (op: u32, a0: u32, a1: u32, a2: u32) callconv(.c) u32;

pub const ApiTable = extern struct {
    abi_version: u32,
    table_size: u16,
    _pad0: u16,
    granted_caps: u32,
    call_fn: KernCallFn,

    pub inline fn call(self: *const ApiTable, op: u32, a0: u32, a1: u32, a2: u32) u32 {
        return self.call_fn(op, a0, a1, a2);
    }

    pub inline fn clear(self: *const ApiTable) void {
        _ = self.call(Op.GFX_CLEAR, 0, 0, 0);
    }

    pub inline fn commitFrame(self: *const ApiTable) void {
        _ = self.call(Op.GFX_COMMIT, 0, 0, 0);
    }

    pub inline fn drawRect(self: *const ApiTable, x: i16, y: i16, w: i16, h: i16, color: Color) void {
        _ = self.call(
            Op.GFX_DRAW,
            SubGfxDraw.RECT | (@as(u32, @intFromEnum(color)) << 8),
            packXy(x, y),
            packXy(w, h),
        );
    }

    pub inline fn fillRect(self: *const ApiTable, x: i16, y: i16, w: i16, h: i16, color: Color) void {
        _ = self.call(
            Op.GFX_DRAW,
            SubGfxDraw.FILL_RECT | (@as(u32, @intFromEnum(color)) << 8),
            packXy(x, y),
            packXy(w, h),
        );
    }

    pub inline fn drawStrAlign(
        self: *const ApiTable,
        x: i16,
        y: i16,
        text: []const u8,
        font: u8,
        color: Color,
        @"align": u8,
    ) void {
        var req = TextReq{
            .text = text.ptr,
            .x = x,
            .y = y,
            .len = @intCast(text.len),
            .arg = @"align",
            .font = font,
            .color = @intFromEnum(color),
            .flags = 0,
            ._pad = 0,
        };
        _ = self.call(Op.GFX_TEXT, @intFromPtr(&req), 0, 0);
    }

    pub inline fn ticksMs(self: *const ApiTable) u32 {
        return self.call(Op.IO_TIME, 0, 0, 0);
    }

    pub inline fn tuneHz(self: *const ApiTable, freq_hz: u32) u32 {
        return self.call(Op.RF_TUNE, freq_hz, 0, 0);
    }
};

pub const Event = extern struct {
    kind: u8,
    key: u8,
    flags: u8,
    reserved: u8,
    payload: u32,
};

pub const TextReq = extern struct {
    text: [*]const u8,
    x: i16,
    y: i16,
    len: u16,
    arg: i16,
    font: u8,
    color: u8,
    flags: u8,
    _pad: u8,
};

pub inline fn packXy(x: i16, y: i16) u32 {
    const ux: u32 = @as(u16, @bitCast(x));
    const uy: u32 = @as(u16, @bitCast(y));
    return ux | (uy << 16);
}

/// Divisionless `/ 10` and `% 10` for `0..=65535` on Cortex-M0+ (zero `__aeabi_uidiv`).
pub inline fn divRem10U16(val: u16) struct { q: u16, r: u8 } {
    const q: u16 = @intCast((@as(u32, val) * 52_429) >> 19);
    const r: u8 = @intCast(val - q * 10);
    return .{ .q = q, .r = r };
}
