OUTPUT_FORMAT(elf64-x86-64)
OUTPUT_ARCH(i386:x86-64)

ENTRY(_start)

PHDRS
{
    limine_reqs PT_LOAD FLAGS(4); /* Read-only (R--) */
    text        PT_LOAD FLAGS(5); /* Read + Execute (R-X) */
    rodata      PT_LOAD FLAGS(4); /* Read-only (R--) */
    data        PT_LOAD FLAGS(6); /* Read + Write (RW-) */
}

SECTIONS
{
    /* Kernel placed in higher-half top 2GB at 1MB offset */
    . = 0xFFFFFFFF80100000;

    .limine_req_start : {
        KEEP(*(.limine_req_start))
    } :limine_reqs

    .limine_reqs : {
        KEEP(*(.limine_reqs))
    } :limine_reqs

    .limine_req_end : {
        KEEP(*(.limine_req_end))
    } :limine_reqs

    . = ALIGN(4096);
    .text : {
        *(.text .text.*)
    } :text

    . = ALIGN(4096);
    .rodata : {
        *(.rodata .rodata.*)
    } :rodata

    . = ALIGN(4096);
    .data : {
        *(.data .data.*)
    } :data

    .bss : {
        *(.bss .bss.*)
        *(COMMON)
    } :data

    /DISCARD/ : {
        *(.eh_frame)
        *(.note .note.*)
    }
}
