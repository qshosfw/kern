/*
 * DFU Leaf 9: Flat Filesystem Scanner (No Folders / No Icons)
 *
 * Populates st->buf.tools.files[] across /boot, /apps, and /:
 *   tool_sub == 0: .kfw, .uf2, .bin (firmware images)
 *   tool_sub == 1: .krx, .ram, .mod (applets & packages)
 */
#include "dfu_shared.h"

static u8 eq_ext(const char* name, const char* ext) {
    u32 n = krx_strlen(name);
    if (n <= 4u) return 0u;
    for (u32 i = 0u; i < 4u; i++) {
        char c = name[n - 4u + i];
        if (c >= 'A' && c <= 'Z') c = (char)(c + 32);
        if (c != ext[i]) return 0u;
    }
    return 1u;
}

__attribute__((visibility("default"), section(".text.entry")))
u32 leaf9_entry(u32 op, const KrxApiTable* api, void* param) {
    (void)op;
    KrxLeafContext* ctx = (KrxLeafContext*)param;
    if (!ctx || !api || !ctx->arg) return 1u;
    DfuSharedState* st = (DfuSharedState*)ctx->arg;
    u8 applets = st->tool_sub;

    DfuToolsScratch* ts = (DfuToolsScratch*)st->stage_buf;
    DfuDirEntry ents[12];
    static const char DIRS[] = "/boot\0/apps\0/\0";
    u8 count = 0u;
    const char* dp = DIRS;

    for (u8 di = 0u; di < 3u && count < DFU_MAX_FILES; di++) {
        KrxFsReq req;
        req.path = (const u8*)dp;
        req.buf = (u8*)ents;
        req.offset = 0u;
        req.len = 12u;
        req.path_len = (u8)krx_strlen(dp);
        req.extra = 0u;
        u32 n = krx_call(api, OP_FS_READ, SUB_FS_READ_LIST_DIR, (u32)&req, 0u);
        for (u32 i = 0u; i < n && count < DFU_MAX_FILES; i++) {
            if (ents[i].is_dir) continue;
            ents[i].name[23] = 0u;
            const char* nm = (const char*)ents[i].name;
            u8 ok = applets ? (eq_ext(nm, ".krx") || eq_ext(nm, ".ram") || eq_ext(nm, ".mod"))
                            : (eq_ext(nm, ".kfw") || eq_ext(nm, ".uf2") || eq_ext(nm, ".bin"));
            if (!ok) continue;
            DfuFlatFile* dst = &ts->files[count];
            u32 j = 0u;
            while (nm[j] && j < 19u) { dst->name[j] = nm[j]; j++; }
            dst->name[j] = 0;
            dst->size = ents[i].size;
            dst->dir_idx = di;
            count++;
        }
        while (*dp) dp++;
        dp++;
    }
    ts->file_count = count;
    ts->file_sel = 0u;
    st->next_leaf = DFU_LEAF_FILES;
    return 0u;
}
