#!/bin/bash
set -e
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SRC="$DIR/src"
OUT="$DIR/build"
mkdir -p "$OUT"

CC_FLAGS="-mthumb -mcpu=cortex-m0plus -Os -fPIC -fPIE -fvisibility=hidden -nostdlib -fno-builtin -fno-tree-loop-distribute-patterns -fno-jump-tables -ffunction-sections -fdata-sections -I$SRC -I$DIR/../bench -Wa,-I$OUT"
LD_FLAGS="-Wl,--gc-sections -T $DIR/krx.ld -Wl,--no-dynamic-linker"

arm-none-eabi-gcc -mthumb -mcpu=cortex-m0plus -nostdlib -Wl,--build-id=none -Ttext=0 \
    "$SRC/dfu_hook.S" -o "$OUT/dfu_hook.elf"
arm-none-eabi-objcopy -O binary "$OUT/dfu_hook.elf" "$OUT/dfu_hook.bin"

compile_unit() {
    local src="$1"
    local stem="$2"
    local entry="$3"
    arm-none-eabi-gcc $CC_FLAGS $LD_FLAGS -Wl,-e,"$entry" -o "$OUT/$stem.elf" "$SRC/$src"
    local entry_addr
    entry_addr=$(arm-none-eabi-nm "$OUT/$stem.elf" | awk -v sym="$entry" '$3 == sym { print $1 }')
    if [ "$entry_addr" != "00000000" ]; then
        echo "ERROR: $stem.elf entry $entry at '$entry_addr' (must be 00000000)!" >&2
        exit 1
    fi
    arm-none-eabi-objcopy -O binary "$OUT/$stem.elf" "$OUT/$stem.bin"
}

compile_unit dfu.c          root         _start
compile_unit leaf1_splash.c leaf1_splash leaf1_entry
compile_unit leaf2_modal.c  leaf2_modal  leaf2_entry
compile_unit leaf3_stage.c  leaf3_stage  leaf3_entry

for elf in root leaf1_splash leaf2_modal leaf3_stage; do
    undefs=$(arm-none-eabi-nm -u "$OUT/$elf.elf" | wc -l)
    relocs=$(arm-none-eabi-readelf -r "$OUT/$elf.elf" | grep -c "R_ARM_" || true)
    sz=$(stat -c %s "$OUT/$elf.bin")
    got_sz=$(arm-none-eabi-readelf -S "$OUT/$elf.elf" | awk '/\.got/{print $6}' | grep -v '^0\+$' || true)
    echo "  $elf.bin: ${sz} B (undefs=$undefs, relocs=$relocs)"
    if [ "$undefs" -ne 0 ] || [ "$relocs" -ne 0 ] || [ -n "$got_sz" ]; then
        echo "ERROR: $elf.elf must have 0 undefs, 0 relocs, and 0 GOT!" >&2
        exit 1
    fi
done

root_sz=$(stat -c %s "$OUT/root.bin")
if [ "$root_sz" -gt 4900 ]; then
    echo "ERROR: root.bin ($root_sz B) exceeds safe P0 resident limit (4900 B)!" >&2
    exit 1
fi

for leaf in leaf1_splash leaf2_modal leaf3_stage; do
    sz=$(stat -c %s "$OUT/$leaf.bin")
    if [ "$sz" -gt 1024 ]; then
        echo "ERROR: $leaf.bin ($sz B) exceeds 1024 B P0 leaf overlay window!" >&2
        exit 1
    fi
done

# Caps: StorageRead (1<<2) | DisplayDirect (1<<5) | PowerManage (1<<10) | Keypad (1<<18) | ServiceRegistry (1<<20) | StorageAdmin (1<<23) | FactoryCalib (1<<24) = 0x01940424
python3 "$DIR/../../tools/pack_krx.py" "$DIR/dfu.mod" \
    --name "DFU Update" --author "KERN-SYS" --type module --profile p0 --system-action \
    --caps 0x01940424 --payload "$OUT/root.bin" \
    --leaf "1:$OUT/leaf1_splash.bin" \
    --leaf "2:$OUT/leaf2_modal.bin" \
    --leaf "3:$OUT/leaf3_stage.bin" \
    --verify

echo "Built dfu.mod ($(stat -c %s "$DIR/dfu.mod") bytes, P0 Structured Paged Module)"
