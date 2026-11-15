/*
 * P0 Leaf 3: Live MCU & Power Telemetry Modal Formatter.
 * Triggered on KEY_MENU from SysInfo Root via KRX UiApi::ui_show_popup().
 */

#include "sysinfo_shared.h"

static const char L3_STR[] = "RAM \0B  Stk \0B\nBat \0mV \0%\nCPU \0MHz \0MCU & Power\0";

static u8 add_lit(char* dst, u8 off, u8 len) {
    u8 i = 0;
    while (i < len) { dst[i] = L3_STR[off + i]; i++; }
    return len;
}

__attribute__((section(".text.entry")))
u32 leaf3_entry(u32 op, const KrxApiTable* api, void* param) {
    (void)op;
    KrxLeafContext* ctx = (KrxLeafContext*)param;
    if (!ctx || !api || !api->ui || !api->ui->ui_show_popup) return 1;

    u16 ram_free = (api->service && api->service->sys_ram_available)
        ? api->service->sys_ram_available() : 0;
    u16 stk_free = (api->service && api->service->sys_stack_margin)
        ? api->service->sys_stack_margin() : 0;
    u16 mv = (api->power && api->power->sys_battery_mv)
        ? api->power->sys_battery_mv() : 0;
    u8 pct = (api->power && api->power->sys_battery_pct)
        ? api->power->sys_battery_pct() : 0;
    i16 temp_c = (api->power && api->power->sys_mcu_temp)
        ? api->power->sys_mcu_temp() : 0;
    u32 mhz = (api->power && api->power->sys_cpu_freq_hz)
        ? si_udivmod(api, api->power->sys_cpu_freq_hz(), 1000000u, 0) : 48u;

    char msg[72];
    u8 p = 0;
    p += add_lit(msg + p, 0, 4);    // "RAM "
    p += si_put_uint(api, msg + p, ram_free);
    p += add_lit(msg + p, 5, 7);    // "B  Stk "
    p += si_put_uint(api, msg + p, stk_free);
    p += add_lit(msg + p, 13, 6);   // "B\nBat "
    p += si_put_uint(api, msg + p, mv);
    p += add_lit(msg + p, 20, 3);   // "mV "
    p += si_put_uint(api, msg + p, pct);
    p += add_lit(msg + p, 24, 6);   // "%\nCPU "
    p += si_put_uint(api, msg + p, mhz);
    p += add_lit(msg + p, 31, 4);   // "MHz "
    if (temp_c < 0) {
        msg[p++] = '-';
        temp_c = (i16)(-temp_c);
    }
    p += si_put_uint(api, msg + p, (u32)(u16)temp_c);
    msg[p++] = 'C';
    msg[p] = 0;

    api->ui->ui_show_popup(7, (const u8*)(L3_STR + 36), 11, (const u8*)msg, p);

    ctx->continuation.kind = KRX_LEAF_KIND_DONE;
    ctx->continuation.target_or_code = 0;
    ctx->continuation.payload = 0;
    return 0;
}
