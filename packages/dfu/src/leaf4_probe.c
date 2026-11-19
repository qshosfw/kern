/*
 * DFU Leaf 4: Multi-Bootloader ROM Fingerprint & KBHP v1.0 Pre-Flight Probe
 *
 * Inspects 0x0800_0000..0x0800_27FF non-destructively (read-only) to classify
 * the installed primary bootloader family:
 *   - KBHP_BL_STOCK_K1:   Verified Quansheng PY32F071 7.00..7.03 (RVCT trampoline +
 *                         VID:PID 36B7:0001 + 16 B XOR key) -> full 30 Hz hook.
 *   - KBHP_BL_STOCK_SAFE: Stock RVCT trampoline with unrecognized rev -> safe guard hook.
 *   - KBHP_BL_MOTO:       muzkr/moto UF2 MSC bootloader (36B7:FFFF / "Moto") -> raw handoff.
 *   - KBHP_BL_UNKNOWN:    Custom/unknown bootloader -> validated raw handoff.
 *
 * Verifies factory provisioning page integrity (0x0800_2750 == "1-sq"),
 * captures battery voltage & Kern OS version, and signs KbhpEnvelope.
 */
#include "dfu_shared.h"

__attribute__((noinline))
static u8 scan_tag8(const u8* hay, u32 hay_len, u32 w0, u32 w1) {
    for (u32 i = 0; i + 8u <= hay_len; i += 4u) {
        const u32* p = (const u32*)(hay + i);
        if (p[0] == w0 && (w1 == 0u || p[1] == w1)) return 1u;
    }
    return 0u;
}

__attribute__((noinline))
static u8 probe_rvct_trampoline(u32 out[3]) {
    const u8* base = (const u8*)DFU_BL_FLASH_BASE;
    const u32 rom_len = 0x2700u;
    u32 msp = *(const volatile u32*)(base + 0);
    u32 reset = *(const volatile u32*)(base + 4);
    u32 pc_off = (reset - DFU_BL_FLASH_BASE) & ~1u;
    if ((msp & 3u) != 0u || (msp - 0x20000400u) > 0x2A00u ||
        (reset & 1u) == 0u || pc_off < 8u || pc_off + 8u > rom_len) {
        return 0u;
    }
    out[0] = msp;
    for (u32 step = 0; step < 2u; step++) {
        u32 off = pc_off + step * 4u;
        u16 expect = (step == 0u) ? 0x4780u : 0x4700u;
        if (base[off + 1] != 0x48u || *(const volatile u16*)(base + off + 2) != expect) {
            return 0u;
        }
        u32 lit = ((off + 4u) & ~3u) + ((u32)base[off] << 2);
        if (lit + 4u > rom_len) return 0u;
        u32 target = *(const volatile u32*)(base + lit);
        if ((target & 1u) == 0u || (target - (DFU_BL_FLASH_BASE + 0x10u)) >= 0x26F0u) {
            return 0u;
        }
        out[step + 1] = target;
    }
    return 1u;
}

__attribute__((noinline))
static u8 probe_stock_version(char out[8]) {
    const u8* rom = (const u8*)0x08001F00u;
    for (u32 i = 0; i + 8u <= 0x0800u; i++) {
        const u8* w = rom + i;
        if (w[1] == '.' && w[4] == '.' && w[7] == 0u &&
            (u8)(w[0] - '0') <= 9u && (u8)(w[2] - '0') <= 9u &&
            (u8)(w[3] - '0') <= 9u && (u8)(w[5] - '0') <= 9u &&
            (u8)(w[6] - '0') <= 9u) {
            dfu_copy_bytes((u8*)out, w, 8u);
            return 1u;
        }
    }
    return 0u;
}

__attribute__((noinline))
static u32 probe_usb_vid_pid(void) {
    const u8* rom = (const u8*)DFU_BL_FLASH_BASE;
    for (u32 i = 0; i + 18u <= 0x2700u; i++) {
        if (rom[i] == 0x12u && rom[i + 1] == 0x01u &&
            rom[i + 2] == 0x00u && rom[i + 3] == 0x02u && rom[i + 7] == 0x40u) {
            return (((u32)rom[i + 9] << 24) | ((u32)rom[i + 8] << 16) |
                    ((u32)rom[i + 11] << 8) | rom[i + 10]);
        }
    }
    return 0u;
}

__attribute__((visibility("default"), section(".text.entry")))
u32 leaf4_entry(u32 op, const KrxApiTable* api, void* param) {
    (void)op;
    KrxLeafContext* ctx = (KrxLeafContext*)param;
    if (!ctx || !api || !ctx->arg) return 1u;
    DfuSharedState* st = (DfuSharedState*)ctx->arg;

    dfu_zero_bytes((u8*)st, offsetof(DfuSharedState, ui)); /* clears kbhp + tramp + meta */

    KbhpEnvelope* env = &st->kbhp;
    env->ident           = KBHP_IDENT;
    env->version         = KBHP_VERSION;
    env->cmd             = KBHP_CMD_ENTER_DFU;
    env->target_id       = DFU_TARGET_UV_K1;
    env->origin_tag      = KBHP_ORIGIN_KERN;
    env->fw_version      = krx_sys_os_version(api);
    env->battery_mv      = (u16)krx_call(api, OP_IO_POWER, SUB_IO_POWER_BATT_MV, 0u, 0u);
    env->app_max_pages   = DFU_APP_MAX_PAGES;
    env->app_flash_base  = DFU_APP_FLASH_BASE;
    env->hook_ver        = KBHP_HOOK_VERSION;
    env->elapsed_sec     = 0xFFFFu;
    env->hook_state_addr = DFU_SRAM_STATE;

    u32 flags = KBHP_FLAG_ANTIBRICK_GUARD | KBHP_FLAG_FLASHLIGHT_LOCKED;
    u32 is_chg = krx_call(api, OP_IO_POWER, SUB_IO_POWER_CHARGING, 0u, 0u);
    u32 is_low = krx_call(api, OP_IO_POWER, SUB_IO_POWER_LOW, 0u, 0u) |
                 krx_call(api, OP_IO_POWER, SUB_IO_POWER_CRITICAL, 0u, 0u);
    if (is_low && !is_chg) flags |= KBHP_FLAG_LOW_BATTERY;

    if (*(const volatile u32*)(DFU_FACTORY_PAGE + 0x50u) == 0x71732D31u) {
        flags |= KBHP_FLAG_FACTORY_PAGE_OK;
    }

    u32 vid_pid = probe_usb_vid_pid();
    env->bl_vid_pid = vid_pid;

    u8 has_rvct = probe_rvct_trampoline(st->tramp);
    u8 has_ver  = probe_stock_version(env->bl_ver);
    /* Stock 16-byte XOR key leading 8 bytes: 16 6c 14 e6 2e 91 0d 40 */
    u8 has_xor  = scan_tag8((const u8*)0x08001F00u, 0x0800u, 0xE6146C16u, 0x400D912Eu);
    /* Moto UF2 bootloader: VID:PID 36B7:FFFF or "Moto" tag */
    u8 is_moto  = (vid_pid == DFU_MOTO_VID_PID) ||
                  scan_tag8((const u8*)DFU_BL_FLASH_BASE, 0x2700u, 0x6F746F4Du, 0u);

    if (has_rvct) {
        st->use_hook = 1u;
        flags |= KBHP_FLAG_HOOK_ACTIVE;
        u8 known_rev = has_ver && env->bl_ver[0] == '7' && env->bl_ver[2] == '0' &&
                       (u8)(env->bl_ver[3] - '0') <= 3u;
        if (vid_pid == DFU_STOCK_VID_PID && has_xor && known_rev) {
            env->bl_kind = KBHP_BL_STOCK_K1;
        } else {
            env->bl_kind = KBHP_BL_STOCK_SAFE;
            if (!has_ver) dfu_copy_bytes((u8*)env->bl_ver, (const u8*)"Stock", 6u);
        }
    } else if (is_moto) {
        env->bl_kind = KBHP_BL_MOTO;
        dfu_copy_bytes((u8*)env->bl_ver, (const u8*)"Moto BL", 8u);
    } else {
        env->bl_kind = KBHP_BL_UNKNOWN;
        dfu_copy_bytes((u8*)env->bl_ver, (const u8*)"Custom", 7u);
    }

    env->flags = flags;
    env->header_checksum = kbhp_checksum(env);
    return 0u;
}
