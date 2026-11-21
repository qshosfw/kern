/*
 * P0 Leaf 14: Safe Mode Recovery Menu Renderer (<= 1 KB).
 */
#include "panic_shared.h"

static const char SAFE_ITEMS[] =
    "Continue Boot\0Inspect Crash\0Dump Serial\0Reset Prefs\0Reboot Radio";

__attribute__((visibility("default"), section(".text.entry")))
u32 leaf14_entry(u32 op, const KrxApiTable* api, void* param) {
    (void)op;
    KrxLeafContext* ctx = (KrxLeafContext*)param;
    if (!ctx || !api || !ctx->arg) return 1;
    PanicState* st = (PanicState*)ctx->arg;
    const BootSlot* s = &st->cur_slot;
    krx_clear(api);

    krx_fill_rect(api, 0, 0, 128, 8, COLOR_FILL);
    char ln[28];
    u8 strikes = s->crash_count;
    if (strikes == 0u) strikes = (u8)((st->snap.backup_tag >> 16) & 0xFFu);
    u8 p = p_app_s(ln, 0u, ":( SAFE MODE #");
    p += p_put_uint(api, ln + p, strikes);
    krx_draw_str_ex(api, 2, 1, (const u8*)ln, p, FONT_TOMTHUMB, COLOR_CLEAR);

    p = p_app_s(ln, 0u, "PC:");
    p += p_put_hex(ln + p, s->fault_pc, 8u);
    ln[p++] = ' ';
    p += p_copy_cstr(ln + p, (const char*)s->actor_tag, 12u);
    krx_draw_str_max(api, 2, 10, (const u8*)ln, p, FONT_TOMTHUMB, COLOR_FILL, 124);

    u8 sc = (st->menu_sel >= 3u) ? (u8)(st->menu_sel - 2u) : 0u;
    if (sc > 1u) sc = 1u;
    for (u8 v = 0; v < 4u; v++) {
        u8 idx = (u8)(sc + v);
        const char* k = p_nth(SAFE_ITEMS, idx);
        krx_ui_draw_menu_item(api, (i16)(17 + v * 11), (const u8*)k, p_len(k), (const u8*)"", 0u, idx == st->menu_sel, 0u);
    }
    krx_ui_draw_scrollbar(api, 124u, 17u, 44u, sc, 5u, 4u);

    ctx->continuation.kind = KRX_LEAF_KIND_DONE;
    ctx->continuation.target_or_code = 0;
    ctx->continuation.payload = 0;
    return 0;
}
