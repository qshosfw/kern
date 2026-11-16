SECTIONS {
    . = 0;
    .text : {
        KEEP(*(.text.entry))
        *(.text*)
        *(.rodata*)
        *(.data*)
        *(.bss*)
    }
}
