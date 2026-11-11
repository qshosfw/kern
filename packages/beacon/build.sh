#!/bin/bash
set -e
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
arm-none-eabi-gcc -mthumb -mcpu=cortex-m0plus -Os -fPIC -fPIE -nostdlib -fno-builtin -fno-jump-tables \
    -ffunction-sections -fdata-sections -Wl,--gc-sections \
    -T "$DIR/krx.ld" -Wl,--no-dynamic-linker -o "$DIR/beacon.elf" "$DIR/beacon.c"
arm-none-eabi-objcopy -O binary "$DIR/beacon.elf" "$DIR/beacon.bin"
# Caps: RadioRx|RadioTx|Display|Keypad = 0x40023
python3 "$DIR/../../tools/pack_krx.py" "$DIR/beacon.krx" --name Beacon --profile n1 --caps 0x40023 --payload "$DIR/beacon.bin"
echo "Built beacon.bin ($(stat -c %s "$DIR/beacon.bin") bytes) and beacon.krx"
