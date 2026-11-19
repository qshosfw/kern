/*
 * DFU Leaf 11: Pre-Flight Firmware Image & Applet Inspector
 */
#include "dfu_shared.h"

__attribute__((visibility("default"), section(".text.entry")))
u32 leaf11_entry(u32 op, const KrxApiTable* api, void* param) {
    (void)op;
    KrxLeafContext* ctx = (KrxLeafContext*)param;
    if (!ctx || !api || !ctx->arg) return 1u;
    DfuSharedState* st = (DfuSharedState*)ctx->arg;

    u8 hdr[16];
    dfu_zero_bytes(hdr, 16u);
    const char* path = st->buf.tools.sel_path;
    u16 rd = krx_fs_read(api, (const u8*)path, (u8)krx_strlen(path), hdr, 16u);
    u32 w0 = (u32)hdr[0] | ((u32)hdr[1] << 8) | ((u32)hdr[2] << 16) | ((u32)hdr[3] << 24);
    u32 w1 = (u32)hdr[4] | ((u32)hdr[5] << 8) | ((u32)hdr[6] << 16) | ((u32)hdr[7] << 24);

    const char* kind = "Binary";
    if (rd >= 4u) {
        if (w0 == 0x3157464Bu) kind = "KFW1 Signed";
        else if (w0 == 0x0A324655u) kind = "UF2 Image";
        else if ((w0 & 0x00FFFFFFu) == 0x0058524Bu) kind = "KRX Package";
        else if ((w0 & 3u) == 0u && (w0 - 0x20000400u) <= 0x3C00u &&
                 (w1 & 1u) != 0u && (w1 - DFU_APP_FLASH_BASE) < 0x1D800u) {
            kind = "ARM Firmware";
        }
    }

    char sz[16];
    u32 sn = st->ui.u32_dec(sz, st->buf.tools.sel_size);
    sz[sn++] = ' '; sz[sn++] = 'B'; sz[sn] = 0;

    const char* fit = st->tool_sub ? "KernFS Package" :
                      (st->buf.tools.sel_size > 0u && st->buf.tools.sel_size <= 120832u)
                          ? "Fits 118 KB" : "Too Large";

    krx_clear(api);
    st->ui.header(api, st->buf.tools.sel_name);
    st->ui.kv(api, 17, "Path", path);
    st->ui.kv(api, 25, "Format", kind);
    st->ui.kv(api, 33, "Size", sz);
    st->ui.kv(api, 41, "Check", fit);
    st->ui.softbar(api, "", "", "EXIT", "Back");
    krx_commit_frame(api);

    for (;;) {
        u8 k = st->ui.wait_key(api);
        if (k == KEY_EXIT || k == KEY_MENU) return 0u;
    }
}
