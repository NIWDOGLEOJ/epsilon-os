## 2026-08-30T12:34:18Z
You are the M1 GDT, TSS & IDT Explorer for AegisOS.
Your working directory is /home/godjoel/teamwork_projects/aegis_os/.agents/m1_explorer_2.
Project root is /home/godjoel/teamwork_projects/aegis_os.
You MUST read /home/godjoel/teamwork_projects/aegis_os/.agents/ORIGINAL_REQUEST.md and /home/godjoel/teamwork_projects/aegis_os/PROJECT.md.

Your mission for Milestone 1 (M1):
1. Design `src/arch/gdt.rs`:
   - 64-bit GDT table: Null descriptor, Kernel Code (0x08, DPL=0, L=1), Kernel Data (0x10, DPL=0), User Data (0x18, DPL=3), User Code (0x20, DPL=3, L=1), 16-byte TSS descriptor (0x28).
   - `lgdt` reload and segment register reload (`cs`, `ds`, `es`, `ss`).
   - TSS structure initialization with `RSP0` kernel stack and `IST1` double fault stack. `ltr` reload.
2. Design `src/arch/idt.rs`:
   - 256-entry IDT descriptor and table.
   - Naked ISR entry stubs (using `core::arch::global_asm!` or `core::arch::asm!`) saving registers, pushing error code (or dummy 0), calling Rust exception dispatcher, and `iretq`.
   - Ring 0 vs Ring 3 fault classification hook (`CS & 0x03 == 3`).
   - Dedicated handlers for vectors 0 (#DE), 6 (#UD), 8 (#DF), 13 (#GP), 14 (#PF, reading CR2), and PIC/APIC IRQs (vector 32+).

Write your detailed plan and code blueprints to /home/godjoel/teamwork_projects/aegis_os/.agents/m1_explorer_2/plan.md and complete handoff.md. Send a message to parent when done.
