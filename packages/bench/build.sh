#!/bin/bash
set -e
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
arm-none-eabi-gcc -mthumb -mcpu=cortex-m0plus -Os -fPIC -fPIE -nostdlib -fno-builtin -fno-jump-tables \
    -ffunction-sections -fdata-sections -Wl,--gc-sections \
    -T "$DIR/krx.ld" -Wl,--no-dynamic-linker -o "$DIR/bench.elf" "$DIR/bench.c"
arm-none-eabi-objcopy -O binary "$DIR/bench.elf" "$DIR/bench.bin"
# Caps: Display|Keypad|AudioOut|AudioCap|RadioRx|RadioTx|StorageRd|StorageWr|Serial|Power = 0x1FFF
python3 "$DIR/../../tools/pack_krx.py" "$DIR/bench.krx" --name Bench --profile n1 --caps 0x1FFF --payload "$DIR/bench.bin"
echo "Built bench.bin ($(stat -c %s "$DIR/bench.bin") bytes) and bench.krx"
