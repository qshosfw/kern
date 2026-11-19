#!/bin/bash
set -e
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Compile Block 0
arm-none-eabi-gcc -mthumb -mcpu=cortex-m0plus -Os -fPIC -fPIE -nostdlib -fno-builtin -fno-jump-tables \
    -I"$DIR/../sdk" -T "$DIR/../sdk/krx.ld" -Wl,--no-dynamic-linker -o "$DIR/block0.elf" "$DIR/paged_app.c"
arm-none-eabi-objcopy -O binary "$DIR/block0.elf" "$DIR/block0.bin"

# Compile Block 1
arm-none-eabi-gcc -mthumb -mcpu=cortex-m0plus -Os -fPIC -fPIE -nostdlib -fno-builtin -fno-jump-tables \
    -I"$DIR/../sdk" -T "$DIR/../sdk/krx.ld" -Wl,--no-dynamic-linker -o "$DIR/block1.elf" "$DIR/block1.c"
arm-none-eabi-objcopy -O binary "$DIR/block1.elf" "$DIR/block1.bin"

# Pack into P1 Transparent Paged package
python3 "$DIR/../../tools/pack_krx.py" "$DIR/paged.krx" --name "Paged Demo" --profile p1 \
    --page-size 1024 --cache-blocks 2 \
    --block "0:$DIR/block0.bin" \
    --block "1:$DIR/block1.bin"

echo "Built paged.krx ($(stat -c %s "$DIR/paged.krx") bytes, P1 Transparent Paged)"
