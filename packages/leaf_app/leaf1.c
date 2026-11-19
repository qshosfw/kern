#include <krx.h>

__attribute__((section(".text.entry")))
u32 leaf1_entry(u32 op, const KrxApiTable* api, void* param) {
    (void)op;
    KrxLeafContext* ctx = (KrxLeafContext*)param;
    if (!ctx) return 0xFF;

    if (ctx->leaf_id == 1 && ctx->arg == 42) {
        krx_print(api, "LEAF1: TRAMP L2\n");
        ctx->continuation.kind = KRX_LEAF_KIND_CALL_LEAF;
        ctx->continuation.target_or_code = 2;
        ctx->continuation.payload = ctx->arg + 10;
        return 0;
    }

    ctx->continuation.kind = KRX_LEAF_KIND_ERROR;
    ctx->continuation.target_or_code = 1;
    return 1;
}
