#!/bin/bash
set -e
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SRC="$DIR/src"
OUT="$DIR/build"
mkdir -p "$OUT"

CC_FLAGS="-mthumb -mcpu=cortex-m0plus -Os -fPIC -fPIE -fvisibility=hidden -nostdlib -fno-builtin -fno-tree-loop-distribute-patterns -fno-jump-tables -ffunction-sections -fdata-sections -I$SRC -I$DIR/../sdk"
LD_FLAGS="-Wl,--gc-sections -T $DIR/krx.ld -Wl,--no-dynamic-linker"

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

compile_unit recovery.c          root            _start
compile_unit leaf1_menu.c        leaf1_menu      leaf1_entry
compile_unit leaf2_info.c        leaf2_info      leaf2_entry
compile_unit leaf3_peer.c        leaf3_peer      leaf3_entry
compile_unit leaf4_scan.c        leaf4_scan      leaf4_entry
compile_unit leaf5_files.c       leaf5_files     leaf5_entry
compile_unit leaf6_inspect.c     leaf6_inspect   leaf6_entry
compile_unit leaf7_console.c     leaf7_console   leaf7_entry
compile_unit leaf8_flash_chk.c   leaf8_flash_chk leaf8_entry
compile_unit leaf9_flash_ui.c    leaf9_flash_ui  leaf9_entry
compile_unit leaf10_flash_sram.c leaf10_flash_go leaf10_entry

LEAVES=(leaf1_menu leaf2_info leaf3_peer leaf4_scan leaf5_files leaf6_inspect leaf7_console leaf8_flash_chk leaf9_flash_ui leaf10_flash_go)

for elf in root "${LEAVES[@]}"; do
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
if [ "$root_sz" -gt 3584 ]; then
    echo "ERROR: root.bin ($root_sz B) exceeds REC_SCRATCH_OFFSET (3584 B)!" >&2
    exit 1
fi

for leaf in "${LEAVES[@]}"; do
    sz=$(stat -c %s "$OUT/$leaf.bin")
    if [ "$sz" -gt 1024 ]; then
        echo "ERROR: $leaf.bin ($sz B) exceeds 1024 B P0 leaf overlay window!" >&2
        exit 1
    fi
done

# Caps: StorageRead (1<<2) | StorageWrite (1<<3) | DisplayDirect (1<<5) | PowerManage (1<<10) | Keypad (1<<18) | ServiceRegistry (1<<20) | StorageAdmin (1<<23) = 0x0094042C
python3 "$DIR/../../tools/pack_krx.py" "$DIR/recovery.mod" \
    --name "Recovery" --author "KERN-SYS" --type module --profile p0 --system-menu --icon 48 \
    --caps 0x0094042C --payload "$OUT/root.bin" \
    --leaf "1:$OUT/leaf1_menu.bin" \
    --leaf "2:$OUT/leaf2_info.bin" \
    --leaf "3:$OUT/leaf3_peer.bin" \
    --leaf "4:$OUT/leaf4_scan.bin" \
    --leaf "5:$OUT/leaf5_files.bin" \
    --leaf "6:$OUT/leaf6_inspect.bin" \
    --leaf "7:$OUT/leaf7_console.bin" \
    --leaf "8:$OUT/leaf8_flash_chk.bin" \
    --leaf "9:$OUT/leaf9_flash_ui.bin" \
    --leaf "10:$OUT/leaf10_flash_go.bin" \
    --verify

echo "Built recovery.mod ($(stat -c %s "$DIR/recovery.mod") bytes, P0 Structured Paged Module, 10 leaves)"
