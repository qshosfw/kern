#include "panic_shared.h"

__attribute__((visibility("default"), section(".text.entry")))
u32 leaf_tab_chrome(u32 op, const KrxApiTable* api, void* param) {
    (void)op;
    KrxLeafContext* ctx = (KrxLeafContext*)param;
    if (!ctx || !ctx->arg) return 1;

    const PanicState* state = (const PanicState*)ctx->arg;
    static const char titles[] = "Sum\0Reg\0Stk\0Asm\0QR\0Log";
    u8 result = krx_draw_tabbed_sheet(
        api, 0u, 0u, 128u, 64u, state->tab, TAB_COUNT,
        KRX_TAB_FLAG_INTEGRATED_86 | KRX_TAB_FLAG_BOTTOM_BORDER,
        titles, sizeof(titles)
    );
    ctx->continuation.kind = KRX_LEAF_KIND_DONE;
    ctx->continuation.target_or_code = 0;
    ctx->continuation.payload = 0;
    return result;
}
