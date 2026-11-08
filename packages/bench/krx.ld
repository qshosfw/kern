SECTIONS {
    . = 0;
    .text : {
        *(.text.entry)
        *(.text*)
        *(.rodata*)
        *(.data*)
    }
}
