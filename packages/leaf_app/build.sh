#!/bin/bash
set -e
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Compile Root
arm-none-eabi-gcc -mthumb -mcpu=cortex-m0plus -Os -fPIC -fPIE -nostdlib -fno-builtin -fno-jump-tables \
    -I"$DIR/../sdk" -T "$DIR/../sdk/krx.ld" -Wl,--no-dynamic-linker -o "$DIR/root.elf" "$DIR/leaf_app.c"
arm-none-eabi-objcopy -O binary "$DIR/root.elf" "$DIR/root.bin"

# Compile Leaf 1
arm-none-eabi-gcc -mthumb -mcpu=cortex-m0plus -Os -fPIC -fPIE -nostdlib -fno-builtin -fno-jump-tables \
    -I"$DIR/../sdk" -T "$DIR/../sdk/krx.ld" -Wl,--no-dynamic-linker -o "$DIR/leaf1.elf" "$DIR/leaf1.c"
arm-none-eabi-objcopy -O binary "$DIR/leaf1.elf" "$DIR/leaf1.bin"

# Compile Leaf 2
arm-none-eabi-gcc -mthumb -mcpu=cortex-m0plus -Os -fPIC -fPIE -nostdlib -fno-builtin -fno-jump-tables \
    -I"$DIR/../sdk" -T "$DIR/../sdk/krx.ld" -Wl,--no-dynamic-linker -o "$DIR/leaf2.elf" "$DIR/leaf2.c"
arm-none-eabi-objcopy -O binary "$DIR/leaf2.elf" "$DIR/leaf2.bin"

# Pack into P0 Structured Paged package
python3 "$DIR/../../tools/pack_krx.py" "$DIR/leaf.krx" --name "Leaf Demo" --profile p0 \
    --payload "$DIR/root.bin" \
    --leaf "1:$DIR/leaf1.bin" \
    --leaf "2:$DIR/leaf2.bin"

echo "Built leaf.krx ($(stat -c %s "$DIR/leaf.krx") bytes, P0 Structured Paged)"
