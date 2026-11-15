/*
 * P0 Leaf 3: Keypad Guide & Raw Identity Summary Popup (on KEY_F).
 */

#include "sysinfo_shared.h"

static const char L3_TITLE[] = "Controls & Info";
static const char L3_BODY[]  = "1-4 / */# / MENU: Tab\nUP / DOWN: Scroll HW\nEXIT: Return";

__attribute__((section(".text.entry")))
u32 leaf3_entry(u32 op, const KrxApiTable* api, void* param) {
    (void)op;
    KrxLeafContext* ctx = (KrxLeafContext*)param;
    if (!ctx || !api || !api->ui || !api->ui->ui_show_popup) return 1;

    api->ui->ui_show_popup(
        7,
        (const u8*)L3_TITLE,
        si_len(L3_TITLE),
        (const u8*)L3_BODY,
        si_len(L3_BODY)
    );

    ctx->continuation.kind = KRX_LEAF_KIND_DONE;
    ctx->continuation.target_or_code = 0;
    ctx->continuation.payload = 0;
    return 0;
}
