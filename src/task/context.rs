//! Context Switching Infrastructure for AegisOS
//!
//! Provides register state synchronization between the hardware interrupt stack
//! (`InterruptContext`) and Process Control Blocks (`TaskContext`).

use crate::arch::gdt::set_tss_rsp0;
use crate::arch::idt::InterruptContext;
use crate::memory::paging::write_cr3;
use crate::task::pcb::ProcessControlBlock;

/// Saves the CPU register state from the hardware interrupt frame into the PCB context.
#[inline(always)]
pub fn save_context_from_interrupt(pcb: &mut ProcessControlBlock, ctx: &InterruptContext) {
    pcb.context.r15 = ctx.r15;
    pcb.context.r14 = ctx.r14;
    pcb.context.r13 = ctx.r13;
    pcb.context.r12 = ctx.r12;
    pcb.context.r11 = ctx.r11;
    pcb.context.r10 = ctx.r10;
    pcb.context.r9  = ctx.r9;
    pcb.context.r8  = ctx.r8;
    pcb.context.rdi = ctx.rdi;
    pcb.context.rsi = ctx.rsi;
    pcb.context.rbp = ctx.rbp;
    pcb.context.rdx = ctx.rdx;
    pcb.context.rcx = ctx.rcx;
    pcb.context.rbx = ctx.rbx;
    pcb.context.rax = ctx.rax;

    pcb.context.rip = ctx.rip;
    pcb.context.cs = ctx.cs;
    pcb.context.rflags = ctx.rflags;
    pcb.context.rsp = ctx.rsp;
    pcb.context.ss = ctx.ss;
}

/// Restores the CPU register state from the PCB context into the hardware interrupt frame,
/// updates TSS RSP0 for future Ring 3 -> Ring 0 transitions, and switches the PML4 page table if needed.
#[inline(always)]
pub fn restore_context_to_interrupt(pcb: &ProcessControlBlock, ctx: &mut InterruptContext) {
    ctx.r15 = pcb.context.r15;
    ctx.r14 = pcb.context.r14;
    ctx.r13 = pcb.context.r13;
    ctx.r12 = pcb.context.r12;
    ctx.r11 = pcb.context.r11;
    ctx.r10 = pcb.context.r10;
    ctx.r9  = pcb.context.r9;
    ctx.r8  = pcb.context.r8;
    ctx.rdi = pcb.context.rdi;
    ctx.rsi = pcb.context.rsi;
    ctx.rbp = pcb.context.rbp;
    ctx.rdx = pcb.context.rdx;
    ctx.rcx = pcb.context.rcx;
    ctx.rbx = pcb.context.rbx;
    ctx.rax = pcb.context.rax;

    ctx.rip = pcb.context.rip;
    ctx.cs = pcb.context.cs;
    ctx.rflags = pcb.context.rflags;
    ctx.rsp = pcb.context.rsp;
    ctx.ss = pcb.context.ss;

    // Update TSS RSP0 to top of current task's kernel stack
    set_tss_rsp0(pcb.kernel_stack_top.as_u64());

    // Switch CR3 if PML4 page table changed
    write_cr3(pcb.pml4_root);
}
