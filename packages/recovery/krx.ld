OUTPUT_FORMAT("elf32-littlearm")
ENTRY(_start)

SECTIONS {
    . = 0x00000000;
    .text : {
        KEEP(*(.text.entry))
        *(.text .text.*)
        *(.rodata .rodata.*)
        *(.data .data.*)
        *(.bss .bss.* COMMON)
    }
    /DISCARD/ : {
        *(.ARM.exidx* .ARM.extab* .eh_frame* .note* .interp .dynamic .dynsym .dynstr .hash .gnu.hash .rel* .rela*)
    }
}
