#include <krx.h>

__attribute__((section(".text.entry")))
u32 block1_compute(const KrxApiTable* api, u32 arg) {
    krx_print(api, "BLK1: RUN CACHE SLOT 1\n");
    return arg + 7;
}
