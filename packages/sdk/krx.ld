/* Position-independent flat image for every KRX unit (root, leaf, block).
 * The unit entry lives in `.text.entry` and must land at offset 0. */
SECTIONS {
    . = 0;
    .text : {
        KEEP(*(.text.entry))
        *(.text .text.*)
        *(.rodata .rodata.*)
        *(.data .data.*)
        *(.bss .bss.* COMMON)
    }
    /DISCARD/ : {
        *(.ARM.exidx* .ARM.extab* .eh_frame* .note* .comment)
    }
}
