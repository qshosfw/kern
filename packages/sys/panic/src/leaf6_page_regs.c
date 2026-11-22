/*
 * P0 Leaf 6: Page 2 (Core Registers + xPSR Flags) & Page 3 (Extended Registers) (<= 1 KB).
 */
#include "panic_shared.h"

static const char CORE_RN[] = "R0\0R1\0R2\0R3\0IP\0LR\0PC\0XP";
static const char EXT_RN[]  = "R4\0R5\0R6\0R7\0R8\0R9\0RA\0RB";

static __attribute__((noinline)) void draw_ln(const KrxApiTable* api, i16 y, const char* s, u8 len) {
    krx_draw_str_ex(api, 2, y, (const u8*)s, len, FONT_TOMTHUMB, COLOR_FILL);
}

static __attribute__((noinline)) u8 put_h(char* o, u32 v, u8 dig) {
    return p_put_hex(o, v, dig);
}

static __attribute__((noinline)) void draw_pair(
    const KrxApiTable* api, i16 y, const char* rn0, u32 v0, const char* rn1, u32 v1
) {
    char ln[26];
    u8 p = 0u;
    ln[p++] = rn0[0]; ln[p++] = rn0[1]; ln[p++] = ':';
    p += put_h(ln + p, v0, 8u);
    ln[p++] = ' ';
    ln[p++] = rn1[0]; ln[p++] = rn1[1]; ln[p++] = ':';
    p += put_h(ln + p, v1, 8u);
    draw_ln(api, y, ln, p);
}

__attribute__((visibility("default"), section(".text.entry")))
u32 leaf6_entry(u32 op, const KrxApiTable* api, void* param) {
    (void)op;
    KrxLeafContext* ctx = (KrxLeafContext*)param;
    if (!ctx || !api || !ctx->arg) return 1u;
    PanicState* st = (PanicState*)ctx->arg;
    const BootSlot* s = &st->slot;

    krx_clear(api);
    u8 is_ext = (st->page == PAGE_EXT) ? 1u : 0u;
    const char* title = is_ext ? "EXT REGS" : "REGS";
    krx_ui_draw_tag_badge(api, 2, 1, (const u8*)title, p_len(title), 1u);

    char pg[4] = { (char)('1' + st->page), '/', (char)('0' + PAGE_COUNT), 0 };
    krx_draw_str_align(api, 125, 7, (const u8*)pg, 3u, FONT_TOMTHUMB, COLOR_FILL, ALIGN_RIGHT);

    if (!is_ext || st->extra_applies) {
        const char* names = is_ext ? EXT_RN : CORE_RN;
        for (u8 r = 0u; r < 4u; r++) {
            u8 idx = (u8)(r * 2u);
            u32 v0 = is_ext ? st->snap.extra.callee[idx] : s->stack_dump[idx];
            u32 v1 = is_ext ? st->snap.extra.callee[idx + 1u] : s->stack_dump[idx + 1u];
            draw_pair(api, (i16)(17 + r * 8), p_nth(names, idx), v0, p_nth(names, (u8)(idx + 1u)), v1);
        }
        if (is_ext) {
            draw_pair(api, 49, "MS", st->snap.extra.msp, "PS", st->snap.extra.psp);
        } else {
            char ln[30];
            u8 p = 0u;
            ln[p++] = 'S'; ln[p++] = 'P'; ln[p++] = ':';
            p += put_h(ln + p, s->fault_sp, 8u);
            ln[p++] = ' ';
            u32 xp = s->xpsr;
            ln[p++] = (xp & (1u << 31)) ? 'N' : '-';
            ln[p++] = (xp & (1u << 30)) ? 'Z' : '-';
            ln[p++] = (xp & (1u << 29)) ? 'C' : '-';
            ln[p++] = (xp & (1u << 28)) ? 'V' : '-';
            ln[p++] = ' ';
            ln[p++] = (s->forensics.exec_mode & 1u) ? 'P' : 'M';
            ln[p++] = ' ';
            ln[p++] = (xp & (1u << 24)) ? 'T' : '!';
            draw_ln(api, 49, ln, p);
        }
    } else {
        draw_ln(api, 17, "No ext frame", 12u);
        draw_pair(api, 26, "SG", st->ana.crash_sig, "CR", s->panic_file_hash);
        draw_pair(api, 35, "BK", st->snap.backup_tag, "ST", s->crash_count);
        draw_pair(api, 44, "LN", s->panic_line, "TR", s->trap_code);
    }

    ctx->continuation.kind = KRX_LEAF_KIND_DONE;
    return 0u;
}
