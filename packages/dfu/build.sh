#!/bin/bash
set -e
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SRC="$DIR/src"
HOOK="$SRC/hook"
OUT="$DIR/build"
mkdir -p "$OUT"

CC_FLAGS="-mthumb -mcpu=cortex-m0plus -Os -fPIC -fPIE -fvisibility=hidden -nostdlib -fno-builtin -fno-tree-loop-distribute-patterns -fno-jump-tables -ffunction-sections -fdata-sections -I$SRC -I$DIR/../sdk -Wa,-I$OUT"
LD_FLAGS="-Wl,--gc-sections -T $DIR/krx.ld -Wl,--no-dynamic-linker"

# 1. Assemble modular bare-metal SRAM SysTick hook runtime (src/hook/)
arm-none-eabi-gcc -mthumb -mcpu=cortex-m0plus -nostdlib -Wl,--build-id=none -Wl,-e,__dfu_hook_start -Ttext=0 \
    -Wa,-I"$HOOK" "$HOOK/dfu_hook.S" -o "$OUT/dfu_hook.elf"
arm-none-eabi-objcopy -O binary "$OUT/dfu_hook.elf" "$OUT/dfu_hook.bin"

hook_sz=$(stat -c %s "$OUT/dfu_hook.bin")
echo "  dfu_hook.bin: ${hook_sz} B (limit 2304 B)"
if [ "$hook_sz" -gt 2304 ]; then
    echo "ERROR: dfu_hook.bin ($hook_sz B) exceeds DFU_SRAM_CODE_MAX (2304 B)!" >&2
    exit 1
fi

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

# 2. Compile resident root + 11 P0 overlay leaves (with _Static_assert ABI checks)
compile_unit dfu.c            root           _start
compile_unit leaf1_splash.c   leaf1_splash   leaf1_entry
compile_unit leaf2_modal.c    leaf2_modal    leaf2_entry
compile_unit leaf3_stage.c    leaf3_stage    leaf3_entry
compile_unit leaf4_probe.c    leaf4_probe    leaf4_entry
compile_unit leaf5_tools.c    leaf5_tools    leaf5_entry
compile_unit leaf6_info.c     leaf6_info     leaf6_entry
compile_unit leaf7_calib.c    leaf7_calib    leaf7_entry
compile_unit leaf8_rescue.c   leaf8_rescue   leaf8_entry
compile_unit leaf9_scan.c     leaf9_scan     leaf9_entry
compile_unit leaf10_files.c   leaf10_files   leaf10_entry
compile_unit leaf11_inspect.c leaf11_inspect leaf11_entry

LEAVES=(leaf1_splash leaf2_modal leaf3_stage leaf4_probe leaf5_tools leaf6_info leaf7_calib leaf8_rescue leaf9_scan leaf10_files leaf11_inspect)

# 3. Verify strict PIC hygiene (0 undefs, 0 relocs, 0 GOT)
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

# 4. Enforce strict P0 SRAM window limits (root.bin <= 4160 B leaves 960 B staging before 5120 B leaf window)
root_sz=$(stat -c %s "$OUT/root.bin")
if [ "$root_sz" -gt 4160 ]; then
    echo "ERROR: root.bin ($root_sz B) exceeds safe P0 resident limit (4160 B before stage_buf)!" >&2
    exit 1
fi

for leaf in "${LEAVES[@]}"; do
    sz=$(stat -c %s "$OUT/$leaf.bin")
    if [ "$sz" -gt 1024 ]; then
        echo "ERROR: $leaf.bin ($sz B) exceeds 1024 B P0 leaf overlay window!" >&2
        exit 1
    fi
done

# 5. Pack verified P0 Structured Paged module (/apps/dfu.mod)
# Caps: StorageRead (1<<2) | StorageWrite (1<<3) | DisplayDirect (1<<5) | PowerManage (1<<10) | Keypad (1<<18) | ServiceRegistry (1<<20) | StorageAdmin (1<<23) | FactoryCalib (1<<24) = 0x0194042C
python3 "$DIR/../../tools/pack_krx.py" "$DIR/dfu.mod" \
    --name "DFU Update" --author "KERN-SYS" --type module --profile p0 --system-action --icon 49 \
    --caps 0x0194042C --payload "$OUT/root.bin" \
    --leaf "1:$OUT/leaf1_splash.bin" \
    --leaf "2:$OUT/leaf2_modal.bin" \
    --leaf "3:$OUT/leaf3_stage.bin" \
    --leaf "4:$OUT/leaf4_probe.bin" \
    --leaf "5:$OUT/leaf5_tools.bin" \
    --leaf "6:$OUT/leaf6_info.bin" \
    --leaf "7:$OUT/leaf7_calib.bin" \
    --leaf "8:$OUT/leaf8_rescue.bin" \
    --leaf "9:$OUT/leaf9_scan.bin" \
    --leaf "10:$OUT/leaf10_files.bin" \
    --leaf "11:$OUT/leaf11_inspect.bin" \
    --verify

echo "Built dfu.mod ($(stat -c %s "$DIR/dfu.mod") bytes, P0 Structured Paged Module, 11 leaves)"
