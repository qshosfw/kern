/*
 * P0 Leaf 9: Clean Crash-Loop Safe Mode Recovery Menu (<= 1 KB).
 */
#include "panic_shared.h"

static const char SAFE_ITEMS[] =
    "Continue Boot\0Inspect Fault\0Reset Prefs\0Reboot";

__attribute__((visibility("default"), section(".text.entry")))
u32 leaf9_entry(u32 op, const KrxApiTable* api, void* param) {
    (void)op;
    KrxLeafContext* ctx = (KrxLeafContext*)param;
    if (!ctx || !api || !ctx->arg) return 1u;
    PanicState* st = (PanicState*)ctx->arg;
    const BootSlot* s = &st->slot;

    krx_clear(api);

    char ln[24];
    u8 strikes = s->crash_count;
    if (strikes == 0u) strikes = (u8)((st->snap.backup_tag >> 16) & 0xFFu);
    u8 p = p_app_s(ln, 0u, "SAFE MODE #");
    p += p_put_uint(api, ln + p, strikes);
    krx_ui_draw_tag_badge(api, 2, 1, (const u8*)ln, p, 1u);

    p = p_app_s(ln, 0u, "PC:");
    p += p_put_hex(ln + p, s->fault_pc, 8u);
    krx_draw_str_align(api, 125, 7, (const u8*)ln, p, FONT_TOMTHUMB, COLOR_FILL, ALIGN_RIGHT);

    for (u8 i = 0u; i < 4u; i++) {
        const char* k = p_nth(SAFE_ITEMS, i);
        krx_ui_draw_menu_item(
            api, (i16)(14 + i * 12), (const u8*)k, p_len(k),
            (const u8*)"", 0u, i == st->menu_sel, 0u
        );
    }

    ctx->continuation.kind = KRX_LEAF_KIND_DONE;
    return 0u;
}
