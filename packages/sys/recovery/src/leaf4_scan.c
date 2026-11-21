/*
 * Recovery Leaf 4: Flat Filesystem Scanner (/boot, /apps, /)
 */
#include "rec_shared.h"

typedef struct {
    u8  name[24];
    u32 size;
    u8  is_dir;
    u8  reserved[3];
} RecDirEntry;

static u8 has_ext(const char* name, u32 len, const char* ext) {
    if (len < 4u) return 0u;
    const char* p = name + (len - 4u);
    for (u32 i = 0u; i < 4u; i++) {
        char a = p[i], b = ext[i];
        if (a >= 'A' && a <= 'Z') a = (char)(a + 32);
        if (a != b) return 0u;
    }
    return 1u;
}

__attribute__((visibility("default"), section(".text.entry")))
u32 leaf4_entry(u32 op, const KrxApiTable* api, void* param) {
    (void)op;
    KrxLeafContext* ctx = (KrxLeafContext*)param;
    if (!ctx || !api || !ctx->arg) return 1u;
    RecState* st = (RecState*)ctx->arg;

    RecScratch* sc = st->sc;
    sc->file_count = 0u;
    sc->file_sel   = 0u;
    u8 applets = st->tool_sub;

    static const char DIRS[] = "/boot\0/apps\0/user\0/\0";
    for (u8 d = 0u; d < 4u && sc->file_count < REC_MAX_FILES; d++) {
        if (applets && d == 2u) continue; /* skip /user when scanning applets */
        const char* dir = st->ui.nth_s(DIRS, d);
        RecDirEntry ents[12];
        KrxFsReq req;
        req.path = (const u8*)dir; req.buf = (u8*)ents;
        req.offset = 0u; req.len = 12u; req.path_len = (u8)krx_strlen(dir); req.extra = 0u;
        u32 n = krx_call(api, OP_FS_READ, SUB_FS_READ_LIST_DIR, (u32)&req, 0u);
        if (n > 12u) n = 12u;
        for (u32 i = 0u; i < n && sc->file_count < REC_MAX_FILES; i++) {
            if (ents[i].is_dir) continue;
            const char* nm = (const char*)ents[i].name;
            u32 nl = krx_strlen(nm);
            u8 ok = applets
                ? (has_ext(nm, nl, ".krx") || has_ext(nm, nl, ".ram") || has_ext(nm, nl, ".mod"))
                : (has_ext(nm, nl, ".kfw") || has_ext(nm, nl, ".uf2") || has_ext(nm, nl, ".bin"));
            if (!ok) continue;
            RecFlatFile* f = &sc->files[sc->file_count++];
            u32 cl = (nl < 19u) ? nl : 19u;
            rec_copy((u8*)f->name, (const u8*)nm, cl);
            f->name[cl] = 0;
            f->size    = ents[i].size;
            f->dir_idx = d;
        }
    }

    st->next_leaf = REC_LEAF_FILES;
    return 0u;
}
