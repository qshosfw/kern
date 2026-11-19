#include <krx.h>

__attribute__((section(".text.entry")))
u32 leaf2_entry(u32 op, const KrxApiTable* api, void* param) {
    (void)op;
    KrxLeafContext* ctx = (KrxLeafContext*)param;
    if (!ctx) return 0xFF;

    if (ctx->leaf_id == 2 && ctx->arg == 52) {
        krx_print(api, "LEAF2: DONE(104)\n");
        ctx->continuation.kind = KRX_LEAF_KIND_DONE;
        ctx->continuation.target_or_code = 0;
        ctx->continuation.payload = ctx->arg * 2;
        return 104;
    }

    ctx->continuation.kind = KRX_LEAF_KIND_ERROR;
    ctx->continuation.target_or_code = 2;
    return 2;
}
