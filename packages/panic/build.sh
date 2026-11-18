#!/bin/bash
set -e
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SRC="$DIR/src"
OUT="$DIR/build"
mkdir -p "$OUT"

CC_FLAGS="-mthumb -mcpu=cortex-m0plus -Os -fPIC -fPIE -fvisibility=hidden -nostdlib -fno-builtin -fno-tree-loop-distribute-patterns -fno-jump-tables -ffunction-sections -fdata-sections -I$SRC -I$DIR/../bench"
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

compile_unit panic.c           root            _start
compile_unit leaf1_prep.c      leaf1_prep      leaf1_entry
compile_unit leaf2_cause.c     leaf2_cause     leaf2_entry
compile_unit leaf3_tab_sum.c   leaf3_tab_sum   leaf3_entry
compile_unit leaf4_tab_reg.c   leaf4_tab_reg   leaf4_entry
compile_unit leaf5_tab_stk.c   leaf5_tab_stk   leaf5_entry
compile_unit leaf6_dis_lo.c    leaf6_dis_lo    leaf6_entry
compile_unit leaf7_dis_mid.c   leaf7_dis_mid   leaf7_entry
compile_unit leaf8_dis_hi.c    leaf8_dis_hi    leaf8_entry
compile_unit leaf9_asm_draw.c  leaf9_asm_draw  leaf9_entry
compile_unit leaf10_rpt_head.c leaf10_rpt_head leaf10_entry
compile_unit leaf11_rpt_tail.c leaf11_rpt_tail leaf11_entry
compile_unit leaf12_splash.c      leaf12_splash      leaf12_entry
compile_unit leaf13_splash_info.c leaf13_splash_info leaf13_entry
compile_unit leaf14_safemenu.c    leaf14_safemenu    leaf14_entry
compile_unit leaf15_qr_encode.c   leaf15_qr_encode   leaf15_entry
compile_unit leaf16_qr_matrix.c   leaf16_qr_matrix   leaf16_entry
compile_unit leaf17_qr_draw.c     leaf17_qr_draw     leaf17_entry

UNITS="root leaf1_prep leaf2_cause leaf3_tab_sum leaf4_tab_reg leaf5_tab_stk leaf6_dis_lo leaf7_dis_mid leaf8_dis_hi leaf9_asm_draw leaf10_rpt_head leaf11_rpt_tail leaf12_splash leaf13_splash_info leaf14_safemenu leaf15_qr_encode leaf16_qr_matrix leaf17_qr_draw"
for elf in $UNITS; do
    undefs=$(arm-none-eabi-nm -u "$OUT/$elf.elf" | wc -l)
    relocs=$(arm-none-eabi-readelf -r "$OUT/$elf.elf" | grep -c "R_ARM_" || true)
    got_sz=$(arm-none-eabi-readelf -S "$OUT/$elf.elf" | awk '/\.got/{print $6}' | grep -v '^0\+$' || true)
    sz=$(stat -c %s "$OUT/$elf.bin")
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

for leaf in leaf1_prep leaf2_cause leaf3_tab_sum leaf4_tab_reg leaf5_tab_stk leaf6_dis_lo leaf7_dis_mid leaf8_dis_hi leaf9_asm_draw leaf10_rpt_head leaf11_rpt_tail leaf12_splash leaf13_splash_info leaf14_safemenu leaf15_qr_encode leaf16_qr_matrix leaf17_qr_draw; do
    sz=$(stat -c %s "$OUT/$leaf.bin")
    if [ "$sz" -gt 1024 ]; then
        echo "ERROR: $leaf.bin ($sz B) exceeds 1024 B P0 leaf overlay window!" >&2
        exit 1
    fi
done

# Caps: StorageRead (1<<2) | StorageWrite (1<<3) | DisplayDirect (1<<5) | SerialComm (1<<6)
#     | UsbComm (1<<7) | PowerManage (1<<10) | Keypad (1<<18) | ServiceRegistry (1<<20)
#     | StorageAdmin (1<<23) = 0x009404EC
python3 "$DIR/../../tools/pack_krx.py" "$DIR/panic.mod" \
    --name "Crash Logs" --author "KERN-SYS" --type module --profile p0 --system-menu --icon 30 \
    --caps 0x009404EC --payload "$OUT/root.bin" \
    --leaf "1:$OUT/leaf1_prep.bin" \
    --leaf "2:$OUT/leaf2_cause.bin" \
    --leaf "3:$OUT/leaf3_tab_sum.bin" \
    --leaf "4:$OUT/leaf4_tab_reg.bin" \
    --leaf "5:$OUT/leaf5_tab_stk.bin" \
    --leaf "6:$OUT/leaf6_dis_lo.bin" \
    --leaf "7:$OUT/leaf7_dis_mid.bin" \
    --leaf "8:$OUT/leaf8_dis_hi.bin" \
    --leaf "9:$OUT/leaf9_asm_draw.bin" \
    --leaf "10:$OUT/leaf10_rpt_head.bin" \
    --leaf "11:$OUT/leaf11_rpt_tail.bin" \
    --leaf "12:$OUT/leaf12_splash.bin" \
    --leaf "13:$OUT/leaf13_splash_info.bin" \
    --leaf "14:$OUT/leaf14_safemenu.bin" \
    --leaf "15:$OUT/leaf15_qr_encode.bin" \
    --leaf "16:$OUT/leaf16_qr_matrix.bin" \
    --leaf "17:$OUT/leaf17_qr_draw.bin" \
    --verify

echo "Built panic.mod ($(stat -c %s "$DIR/panic.mod") bytes, P0 Structured Paged Module)"
