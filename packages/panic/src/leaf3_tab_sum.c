/*
 * P0 Leaf 3: Summary Tab Renderer (<= 1 KB).
 */
#include "panic_shared.h"

__attribute__((visibility("default"), section(".text.entry")))
u32 leaf3_entry(u32 op, const KrxApiTable* api, void* param) {
    (void)op;
    KrxLeafContext* ctx = (KrxLeafContext*)param;
    if (!ctx || !api || !ctx->arg) return 1;
    PanicState* st = (PanicState*)ctx->arg;
    const BootSlot* s = &st->cur_slot;

    char ln[34];
    if (!st->has_slot) {
        p_copy_cstr(ln, "No crash records stored", 28);
        krx_draw_str_ex(api, 8, 28, (const u8*)ln, p_len(ln), FONT_TOMTHUMB, COLOR_FILL);
    } else {
        const char* rsn = p_reason_name(s->reset_reason);
        krx_ui_draw_tag_badge(api, 4, 13, (const u8*)rsn, p_len(rsn), 1u);
        u8 p = 0;
        ln[p++] = '#';
        p += p_put_uint(api, ln + p, s->crash_count);
        ln[p++] = ' ';
        p += p_copy_cstr(ln + p, (const char*)s->actor_tag, 14);
        ln[p] = 0;
        krx_draw_str_align(api, 122, 15, (const u8*)ln, p, FONT_TOMTHUMB, COLOR_FILL, ALIGN_RIGHT);

        krx_draw_str_ex(api, 4, 24, (const u8*)st->ana.line1, p_len(st->ana.line1), FONT_TOMTHUMB, COLOR_FILL);
        krx_draw_str_ex(api, 4, 32, (const u8*)st->ana.line2, p_len(st->ana.line2), FONT_TOMTHUMB, COLOR_FILL);

        p = p_app_s(ln, 0, "PC:");
        p += p_put_hex(ln + p, s->fault_pc, 8u);
        p = p_app_s(ln, p, " SP:");
        p += p_put_hex(ln + p, s->fault_sp & 0xFFFFu, 4u);
        ln[p] = 0;
        krx_draw_str_ex(api, 4, 42, (const u8*)ln, p, FONT_TOMTHUMB, COLOR_FILL);

        p = p_app_s(ln, 0, "Sig:");
        p += p_put_hex(ln + p, st->ana.crash_sig, 8u);
        p = p_app_s(ln, p, " IRQ:");
        p += p_put_uint(api, ln + p, s->forensics.ipsr_num);
        ln[p] = 0;
        krx_draw_str_ex(api, 4, 51, (const u8*)ln, p, FONT_TOMTHUMB, COLOR_FILL);
    }

    ctx->continuation.kind = KRX_LEAF_KIND_DONE;
    ctx->continuation.target_or_code = 0;
    ctx->continuation.payload = 0;
    return 0;
}
