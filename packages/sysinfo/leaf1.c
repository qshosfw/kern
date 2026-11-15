/*
 * P0 Leaf 1: System Identity, MCU, QS-1 Factory Cert & BootROM Scanner.
 * Queries canonical serial & MAC directly from firmware (`sys_query`) without
 * duplicating Murmur3-64 or Crockford Base32 math, then chains to Leaf 2.
 */

#include "sysinfo_shared.h"

static const char L1_PRES[]    = "Present";
static const char L1_MCU_PFX[] = "PY32F071 ";

static char hex_nibble(u8 v) {
    return (char)(v < 10 ? ('0' + v) : ('A' + (v - 10)));
}

__attribute__((section(".text.entry")))
u32 leaf1_entry(u32 op, const KrxApiTable* api, void* param) {
    (void)op;
    KrxLeafContext* ctx = (KrxLeafContext*)param;
    if (!ctx || !api || !ctx->arg) return 1;

    SysInfoState* st = (SysInfoState*)ctx->arg;
    const CapServiceVTable* s = api->service;

    // 1. Canonical Crockford Base32 hardware serial (`14` chars + NUL)
    if (s && s->sys_query && s->sys_query(KRX_SYS_QUERY_SERIAL_STR, 0, (u8*)st->sn, 14) == 14) {
        st->sn[14] = 0;
    }

    // 2. Canonical IEEE EUI-48 MAC (`XX:XX:XX:XX:XX:XX`)
    u8 mac[6];
    si_zero(mac, 6);
    if (s && s->sys_query) {
        s->sys_query(KRX_SYS_QUERY_MAC, 0, mac, 6);
    }
    for (u8 i = 0; i < 6; i++) {
        st->mac[i * 3 + 0] = hex_nibble(mac[i] >> 4);
        st->mac[i * 3 + 1] = hex_nibble(mac[i] & 0x0Fu);
        st->mac[i * 3 + 2] = (i < 5) ? ':' : 0;
    }

    // 3. MCU Flash size
    u8 kb_raw[2] = { 0, 0 };
    u16 flash_kb = 128;
    if (s && s->sys_query && s->sys_query(KRX_SYS_QUERY_FLASH_SIZE_KB, 0, kb_raw, 2) == 2) {
        flash_kb = (u16)kb_raw[0] | ((u16)kb_raw[1] << 8);
    }
    si_copy((u8*)st->mcu, (const u8*)L1_MCU_PFX, 9);
    u8 klen = si_put_uint(api, st->mcu + 9, flash_kb);
    st->mcu[9 + klen] = 'K';
    st->mcu[10 + klen] = 0;

    // 4. Parse raw 104-byte factory certificate for QS-1 serial (`1-sq` tag)
    u8 cert[104];
    if (s && s->sys_query && s->sys_query(KRX_SYS_QUERY_FACTORY_CERT, 0, cert, 104) == 104) {
        st->has_qs1 = 1;
        u8 ok = (cert[80] == '1' && cert[81] == '-' && cert[82] == 's' && cert[83] == 'q');
        for (u8 i = 0; ok && i < 16; i++) {
            if (cert[84 + i] < 0x20 || cert[84 + i] > 0x7E) ok = 0;
        }
        if (ok) {
            si_copy((u8*)st->qs1, &cert[84], 16);
            st->qs1[16] = 0;
        } else {
            si_copy((u8*)st->qs1, (const u8*)L1_PRES, 8);
        }
    }

    // 5. Scan raw bootloader ROM (`0x0800_1F00..0x0800_2700`) for X.YY.ZZ\0
    st->boot[0] = '-';
    st->boot[1] = '-';
    st->boot[2] = 0;
    if (s && s->sys_query) {
        u8 rom[128];
        u8 found = 0;
        for (u32 off = 0; off < 2048 && !found; off += 120) {
            u16 n = s->sys_query(KRX_SYS_QUERY_BOOTLOADER_ROM, off, rom, sizeof(rom));
            if (n < 8) break;
            for (u16 i = 0; i + 8 <= n; i++) {
                if (rom[i + 1] == '.' && rom[i + 4] == '.' && rom[i + 7] == 0 &&
                    rom[i] >= '0' && rom[i] <= '9' &&
                    rom[i + 2] >= '0' && rom[i + 2] <= '9' &&
                    rom[i + 3] >= '0' && rom[i + 3] <= '9' &&
                    rom[i + 5] >= '0' && rom[i + 5] <= '9' &&
                    rom[i + 6] >= '0' && rom[i + 6] <= '9') {
                    si_copy((u8*)st->boot, &rom[i], 7);
                    st->boot[7] = 0;
                    found = 1;
                    break;
                }
            }
            if (n < sizeof(rom)) break;
        }
    }

    // Trampoline chain directly into Leaf 2 (Firmware Meta, Civil Date & Model)
    ctx->continuation.kind = KRX_LEAF_KIND_CALL_LEAF;
    ctx->continuation.target_or_code = 2;
    ctx->continuation.payload = ctx->arg;
    return 0;
}
