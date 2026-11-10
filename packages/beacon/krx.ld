ENTRY(_start)
SECTIONS {
    . = 0;
    .text : {
        KEEP(*(.text.entry))
        *(.text*)
        *(.rodata*)
        *(.data*)
    }
}
