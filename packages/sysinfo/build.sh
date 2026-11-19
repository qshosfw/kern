#!/bin/bash
set -e
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

CC_FLAGS="-mthumb -mcpu=cortex-m0plus -Os -fPIC -fPIE -nostdlib -fno-builtin -fno-tree-loop-distribute-patterns -fno-jump-tables -ffunction-sections -fdata-sections -I$DIR -I$DIR/../sdk"
LD_FLAGS="-Wl,--gc-sections -T $DIR/krx.ld -Wl,--no-dynamic-linker"

compile_unit() {
    local src="$1"
    local stem="$2"
    local entry="$3"
    arm-none-eabi-gcc $CC_FLAGS $LD_FLAGS -Wl,-e,"$entry" -o "$DIR/$stem.elf" "$DIR/$src"
    arm-none-eabi-objcopy -O binary "$DIR/$stem.elf" "$DIR/$stem.bin"
}

compile_unit sysinfo.c root  _start
compile_unit leaf1.c   leaf1 leaf1_entry
compile_unit leaf2.c   leaf2 leaf2_entry

for elf in root leaf1 leaf2; do
    undefs=$(arm-none-eabi-nm -u "$DIR/$elf.elf" | wc -l)
    relocs=$(arm-none-eabi-readelf -r "$DIR/$elf.elf" | grep -c "R_ARM_" || true)
    sz=$(stat -c %s "$DIR/$elf.bin")
    echo "  $elf.bin: ${sz} B (undefs=$undefs, relocs=$relocs)"
    if [ "$undefs" -ne 0 ] || [ "$relocs" -ne 0 ]; then
        echo "ERROR: $elf.elf must have 0 undefs and 0 relocs!" >&2
        exit 1
    fi
done

for leaf in leaf1 leaf2; do
    sz=$(stat -c %s "$DIR/$leaf.bin")
    if [ "$sz" -gt 1024 ]; then
        echo "ERROR: $leaf.bin ($sz B) exceeds 1024 B P0 leaf overlay window!" >&2
        exit 1
    fi
done

# Caps: DisplayDirect (1<<5) | RealTimeClock (1<<9) | PowerManage (1<<10)
#     | Keypad (1<<18) | ServiceRegistry (1<<20) = 0x00140620
python3 "$DIR/../../tools/pack_krx.py" "$DIR/sysinfo.krx" \
    --name "Info" --author "KERN-SYS" --type module --profile p0 --system-menu --icon 30 \
    --caps 0x00140620 --payload "$DIR/root.bin" \
    --leaf "1:$DIR/leaf1.bin" \
    --leaf "2:$DIR/leaf2.bin" \
    --verify

echo "Built sysinfo.krx ($(stat -c %s "$DIR/sysinfo.krx") bytes, P0 Structured Paged Module)"
