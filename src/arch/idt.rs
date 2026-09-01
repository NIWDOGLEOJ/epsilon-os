//! Interrupt Descriptor Table (IDT), Naked ISR Stubs, 8259 PIC and Exception Dispatcher.
//!
//! Enforces hardware Ring 0 vs Ring 3 fault isolation, IST stack assignment for #DF,
//! and dispatches hardware interrupts (Timer, Keyboard, Mouse).

use core::arch::global_asm;
use core::mem::size_of;

/// Saved CPU register context passed to the Rust interrupt dispatcher.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct InterruptContext {
    pub r15: u64,
    pub r14: u64,
    pub r13: u64,
    pub r12: u64,
    pub r11: u64,
    pub r10: u64,
    pub r9:  u64,
    pub r8:  u64,
    pub rdi: u64,
    pub rsi: u64,
    pub rbp: u64,
    pub rdx: u64,
    pub rcx: u64,
    pub rbx: u64,
    pub rax: u64,

    pub vector: u64,
    pub error_code: u64,

    pub rip: u64,
    pub cs: u64,
    pub rflags: u64,
    pub rsp: u64,
    pub ss: u64,
}

/// 16-byte IDT Entry Descriptor for x86_64.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct IdtEntry {
    pub offset_low: u16,
    pub selector: u16,
    pub ist: u8,
    pub type_attr: u8,
    pub offset_mid: u16,
    pub offset_high: u32,
    pub reserved: u32,
}

impl IdtEntry {
    pub const fn missing() -> Self {
        Self {
            offset_low: 0,
            selector: 0,
            ist: 0,
            type_attr: 0,
            offset_mid: 0,
            offset_high: 0,
            reserved: 0,
        }
    }

    pub fn new(handler: usize, selector: u16, ist: u8, dpl: u8) -> Self {
        let offset = handler as u64;
        let type_attr = 0x80 | ((dpl & 0x03) << 5) | 0x0E; // Present=1, DPL, 64-bit Interrupt Gate (0xE)
        Self {
            offset_low: offset as u16,
            selector,
            ist: ist & 0x07,
            type_attr,
            offset_mid: (offset >> 16) as u16,
            offset_high: (offset >> 32) as u32,
            reserved: 0,
        }
    }
}

/// IDTR register layout for `lidt`.
#[repr(C, packed)]
pub struct Idtr {
    pub limit: u16,
    pub base: u64,
}

/// 256-entry Interrupt Descriptor Table.
#[repr(C, align(16))]
pub struct Idt {
    pub entries: [IdtEntry; 256],
}

impl Idt {
    pub const fn new() -> Self {
        Self {
            entries: [IdtEntry::missing(); 256],
        }
    }

    pub fn set_handler(&mut self, vector: usize, handler: usize, ist: u8, dpl: u8) {
        self.entries[vector] = IdtEntry::new(handler, super::gdt::KERNEL_CODE_SELECTOR, ist, dpl);
    }

    pub fn pointer(&self) -> Idtr {
        Idtr {
            limit: (size_of::<Idt>() - 1) as u16,
            base: self as *const _ as u64,
        }
    }
}

static mut IDT: Idt = Idt::new();

// -----------------------------------------------------------------------------
// 8259 Programmable Interrupt Controller (PIC)
// -----------------------------------------------------------------------------

const PIC1_CMD:  u16 = 0x20;
const PIC1_DATA: u16 = 0x21;
const PIC2_CMD:  u16 = 0xA0;
const PIC2_DATA: u16 = 0xA1;

const PIC_EOI:   u8  = 0x20;

#[inline]
unsafe fn io_wait() {
    core::arch::asm!("out 0x80, al", in("al") 0u8, options(nostack, preserves_flags));
}

#[inline]
unsafe fn outb(port: u16, val: u8) {
    core::arch::asm!("out dx, al", in("dx") port, in("al") val, options(nostack, preserves_flags));
}

#[inline]
unsafe fn inb(port: u16) -> u8 {
    let val: u8;
    core::arch::asm!("in al, dx", in("dx") port, out("al") val, options(nostack, preserves_flags));
    val
}

/// Scheduler / uptime tick rate. `DEFAULT_QUANTUM_TICKS` and the desktop clock
/// both assume the timer actually fires at this rate.
pub const TIMER_HZ: u32 = 100;

/// Input clock of PIT channel 0, in Hz.
const PIT_BASE_FREQUENCY: u32 = 1_193_182;

/// Programs PIT channel 0 to raise IRQ 0 at `TIMER_HZ`.
///
/// Without this the 8254 free-runs at its power-on default of ~18.2 Hz, so every
/// tick-derived quantity is out by a factor of 5.5: the "10 ms" scheduler quantum
/// is really 55 ms, and an uptime clock counting ticks runs correspondingly slow.
unsafe fn init_pit() {
    let divisor = (PIT_BASE_FREQUENCY / TIMER_HZ) as u16;

    // Channel 0, lobyte then hibyte, mode 3 (square wave), binary counting.
    outb(0x43, 0x36);
    outb(0x40, (divisor & 0xFF) as u8);
    outb(0x40, (divisor >> 8) as u8);
}

/// Remaps 8259 PIC IRQs 0..15 to IDT vectors 32..47.
pub unsafe fn init_pic() {
    let _mask1 = inb(PIC1_DATA);
    let _mask2 = inb(PIC2_DATA);

    // ICW1: Start init sequence in cascade mode
    outb(PIC1_CMD, 0x11);
    io_wait();
    outb(PIC2_CMD, 0x11);
    io_wait();

    // ICW2: Vector offsets (Master = 32, Slave = 40)
    outb(PIC1_DATA, 0x20);
    io_wait();
    outb(PIC2_DATA, 0x28);
    io_wait();

    // ICW3: Cascade setup (Slave connected to IRQ2 of Master)
    outb(PIC1_DATA, 0x04);
    io_wait();
    outb(PIC2_DATA, 0x02);
    io_wait();

    // ICW4: 8086/88 mode
    outb(PIC1_DATA, 0x01);
    io_wait();
    outb(PIC2_DATA, 0x01);
    io_wait();

    // Restore masks (or enable IRQ0 Timer, IRQ1 Keyboard, IRQ2 Cascade, IRQ12 Mouse)
    // IRQ0 (bit 0), IRQ1 (bit 1), IRQ2 (bit 2) unmasked on master: 0xF8
    // IRQ12 (bit 4) unmasked on slave: 0xEF
    outb(PIC1_DATA, 0xF8);
    io_wait();
    outb(PIC2_DATA, 0xEF);
    io_wait();
}

/// Sends End-Of-Interrupt (EOI) to 8259 PIC.
pub fn pic_send_eoi(irq: u8) {
    unsafe {
        if irq >= 8 {
            outb(PIC2_CMD, PIC_EOI);
        }
        outb(PIC1_CMD, PIC_EOI);
    }
}

// -----------------------------------------------------------------------------
// Assembly ISR Stubs Generation via global_asm!
// -----------------------------------------------------------------------------

extern "C" {
    static isr_stub_table: [usize; 256];
}

global_asm!(
    r#"
    .altmacro
    .macro isr_no_err_stub nr
    .global isr_stub_\nr
    isr_stub_\nr:
        push 0
        push \nr
        jmp isr_common_stub
    .endm

    .macro isr_err_stub nr
    .global isr_stub_\nr
    isr_stub_\nr:
        push \nr
        jmp isr_common_stub
    .endm

    .macro isr_table_entry k
        .quad isr_stub_\k
    .endm

    /* Vectors 0..31 (Exceptions) */
    isr_no_err_stub 0
    isr_no_err_stub 1
    isr_no_err_stub 2
    isr_no_err_stub 3
    isr_no_err_stub 4
    isr_no_err_stub 5
    isr_no_err_stub 6
    isr_no_err_stub 7
    isr_err_stub    8
    isr_no_err_stub 9
    isr_err_stub    10
    isr_err_stub    11
    isr_err_stub    12
    isr_err_stub    13
    isr_err_stub    14
    isr_no_err_stub 15
    isr_no_err_stub 16
    isr_err_stub    17
    isr_no_err_stub 18
    isr_no_err_stub 19
    isr_no_err_stub 20
    isr_err_stub    21
    isr_no_err_stub 22
    isr_no_err_stub 23
    isr_no_err_stub 24
    isr_no_err_stub 25
    isr_no_err_stub 26
    isr_no_err_stub 27
    isr_no_err_stub 28
    isr_err_stub    29
    isr_err_stub    30
    isr_no_err_stub 31

    /* Vectors 32..255 (IRQs and Software Interrupts) */
    .set i, 32
    .rept 224
        isr_no_err_stub %i
        .set i, i + 1
    .endr

    /* Common ISR Stub: Saves all GPRs and invokes Rust dispatcher */
    .global isr_common_stub
    isr_common_stub:
        push rax
        push rbx
        push rcx
        push rdx
        push rbp
        push rsi
        push rdi
        push r8
        push r9
        push r10
        push r11
        push r12
        push r13
        push r14
        push r15

        mov rdi, rsp
        call rust_interrupt_handler

        pop r15
        pop r14
        pop r13
        pop r12
        pop r11
        pop r10
        pop r9
        pop r8
        pop rdi
        pop rsi
        pop rbp
        pop rdx
        pop rcx
        pop rbx
        pop rax

        add rsp, 16 /* Pop vector number and error code */
        iretq

    /* Array of 256 function pointers to ISR entry stubs */
    .section .rodata
    .global isr_stub_table
    .align 8
    isr_stub_table:
    .set j, 0
    .rept 256
        isr_table_entry %j
        .set j, j + 1
    .endr
    "#
);

// -----------------------------------------------------------------------------
// Rust Exception & Interrupt Dispatcher
// -----------------------------------------------------------------------------

pub type IrqCallback = fn(irq: u8, ctx: &mut InterruptContext);
pub type FaultCallback = fn(vector: u64, ctx: &mut InterruptContext, cr2: u64);

static mut TIMER_CALLBACK: Option<IrqCallback> = None;
static mut KEYBOARD_CALLBACK: Option<IrqCallback> = None;
static mut MOUSE_CALLBACK: Option<IrqCallback> = None;
static mut FAULT_CALLBACK: Option<FaultCallback> = None;

pub fn register_timer_callback(cb: IrqCallback) {
    unsafe { TIMER_CALLBACK = Some(cb); }
}

pub fn register_keyboard_callback(cb: IrqCallback) {
    unsafe { KEYBOARD_CALLBACK = Some(cb); }
}

pub fn register_mouse_callback(cb: IrqCallback) {
    unsafe { MOUSE_CALLBACK = Some(cb); }
}

pub fn register_fault_callback(cb: FaultCallback) {
    unsafe { FAULT_CALLBACK = Some(cb); }
}

/// Primary Rust Interrupt and Exception Dispatcher.
///
/// Called directly from `isr_common_stub`.
#[no_mangle]
pub extern "C" fn rust_interrupt_handler(ctx: *mut InterruptContext) {
    let ctx_ref = unsafe { &mut *ctx };
    let vector = ctx_ref.vector;

    if vector < 32 {
        // CPU Exception (0..31)
        handle_exception(ctx_ref);
    } else if vector >= 32 && vector < 48 {
        // Hardware IRQ (32..47)
        handle_irq(ctx_ref);
    } else {
        // Software Interrupt or Unhandled Vector
        crate::arch::serial::_print(format_args!("[IDT] Received unexpected vector {}\n", vector));
    }
}

/// Handles CPU exceptions with hardware Ring 0 vs Ring 3 privilege discrimination.
fn handle_exception(ctx: &mut InterruptContext) {
    let is_user = (ctx.cs & 0x03) == 3;
    let vector = ctx.vector;

    let cr2: u64 = if vector == 14 {
        let val: u64;
        unsafe { core::arch::asm!("mov {}, cr2", out(reg) val, options(nomem, nostack, preserves_flags)) };
        val
    } else {
        0
    };

    let vector_name = match vector {
        0 => "Divide-by-Zero (#DE)",
        1 => "Debug (#DB)",
        2 => "Non-Maskable Interrupt (NMI)",
        3 => "Breakpoint (#BP)",
        4 => "Overflow (#OF)",
        5 => "Bound Range Exceeded (#BR)",
        6 => "Invalid Opcode (#UD)",
        7 => "Device Not Available (#NM)",
        8 => "Double Fault (#DF)",
        10 => "Invalid TSS (#TS)",
        11 => "Segment Not Present (#NP)",
        12 => "Stack-Segment Fault (#SS)",
        13 => "General Protection Fault (#GP)",
        14 => "Page Fault (#PF)",
        16 => "x87 FPU Error (#MF)",
        17 => "Alignment Check (#AC)",
        18 => "Machine Check (#MC)",
        19 => "SIMD Floating-Point (#XM)",
        21 => "Control Protection (#CP)",
        _ => "Reserved / Unknown Exception",
    };

    if is_user {
        // ---------------------------------------------------------------------
        // RING 3 USERSPACE FAULT ISOLATION (R2 Requirement)
        // ---------------------------------------------------------------------
        crate::arch::serial::_print(format_args!(
            "[FAULT-ISOLATION] Userspace Fault caught: {} (vec {}) at RIP=0x{:016x}, ErrorCode=0x{:x}, CR2=0x{:016x}\n",
            vector_name, vector, ctx.rip, ctx.error_code, cr2
        ));

        // Invoke registered fault callback (M2 Task Reaper / Scheduler)
        if let Some(cb) = unsafe { FAULT_CALLBACK } {
            cb(vector, ctx, cr2);
        } else {
            // Early fallback before M2 scheduler runs: spin user fault
            crate::arch::serial::_print(format_args!("[FAULT-ISOLATION] No fault callback registered; spinning user fault.\n"));
            loop {
                core::hint::spin_loop();
            }
        }
    } else {
        // ---------------------------------------------------------------------
        // RING 0 KERNEL FAULT -> FATAL PANIC
        // ---------------------------------------------------------------------
        crate::arch::serial::_print(format_args!(
            "\n==================== KERNEL EXCEPTION PANIC ====================\n\
             Exception: {} (Vector {})\n\
             Error Code: 0x{:016x} | CR2: 0x{:016x}\n\
             RIP: 0x{:016x} | CS: 0x{:04x} | RFLAGS: 0x{:016x}\n\
             RSP: 0x{:016x} | SS: 0x{:04x}\n\
             RAX: 0x{:016x} | RBX: 0x{:016x} | RCX: 0x{:016x}\n\
             RDX: 0x{:016x} | RSI: 0x{:016x} | RDI: 0x{:016x}\n\
             RBP: 0x{:016x} | R8:  0x{:016x} | R9:  0x{:016x}\n\
             R10: 0x{:016x} | R11: 0x{:016x} | R12: 0x{:016x}\n\
             R13: 0x{:016x} | R14: 0x{:016x} | R15: 0x{:016x}\n\
             ================================================================\n",
            vector_name, vector, ctx.error_code, cr2,
            ctx.rip, ctx.cs, ctx.rflags, ctx.rsp, ctx.ss,
            ctx.rax, ctx.rbx, ctx.rcx, ctx.rdx, ctx.rsi, ctx.rdi,
            ctx.rbp, ctx.r8, ctx.r9, ctx.r10, ctx.r11, ctx.r12,
            ctx.r13, ctx.r14, ctx.r15
        ));

        panic!("Fatal kernel exception: {}", vector_name);
    }
}

/// Handles hardware IRQs (32..47) and sends PIC EOI.
fn handle_irq(ctx: &mut InterruptContext) {
    let irq = (ctx.vector - 32) as u8;

    match irq {
        0 => {
            // IRQ 0: Timer Tick
            if let Some(cb) = unsafe { TIMER_CALLBACK } {
                cb(irq, ctx);
            }
        }
        1 => {
            // IRQ 1: PS/2 Keyboard
            if let Some(cb) = unsafe { KEYBOARD_CALLBACK } {
                cb(irq, ctx);
            }
        }
        12 => {
            // IRQ 12: PS/2 Mouse
            if let Some(cb) = unsafe { MOUSE_CALLBACK } {
                cb(irq, ctx);
            }
        }
        _ => {
            // Unhandled IRQ
        }
    }

    // Acknowledge interrupt to 8259 PIC
    pic_send_eoi(irq);
}

// -----------------------------------------------------------------------------
// IDT Public Initialization & Interrupt Control APIs
// -----------------------------------------------------------------------------

/// Initializes and loads the 256-entry IDT and configures 8259 PIC.
pub fn init_idt() {
    unsafe {
        let idt_ptr = &raw mut IDT;

        // Populate all 256 entries from the assembly stub table
        for (i, &stub_addr) in isr_stub_table.iter().enumerate() {
            let ist = if i == 8 { 1 } else { 0 }; // Vector 8 (#DF) uses IST1
            (*idt_ptr).set_handler(i, stub_addr, ist, 0);
        }

        // Load IDTR
        let idtr = (*idt_ptr).pointer();
        core::arch::asm!(
            "lidt [{0}]",
            in(reg) &idtr,
            options(readonly, nostack, preserves_flags)
        );

        // Initialize 8259 PIC
        init_pic();

        // Bring the timer to the rate the scheduler and clock are written for.
        init_pit();
    }
}

/// Enables CPU hardware interrupts (`sti`).
#[inline(always)]
pub fn enable_interrupts() {
    unsafe {
        core::arch::asm!("sti", options(nomem, nostack));
    }
}

/// Disables CPU hardware interrupts (`cli`).
#[inline(always)]
pub fn disable_interrupts() {
    unsafe {
        core::arch::asm!("cli", options(nomem, nostack));
    }
}

/// Checks if CPU interrupts are currently enabled (RFLAGS IF bit 9).
#[inline(always)]
pub fn are_interrupts_enabled() -> bool {
    let rflags: u64;
    unsafe {
        core::arch::asm!("pushfq; pop {}", out(reg) rflags, options(nomem));
    }
    (rflags & (1 << 9)) != 0
}

/// Executes a closure with interrupts disabled, restoring the previous interrupt state upon exit.
pub fn without_interrupts<F, R>(f: F) -> R
where
    F: FnOnce() -> R,
{
    let saved = are_interrupts_enabled();
    if saved {
        disable_interrupts();
    }
    let ret = f();
    if saved {
        enable_interrupts();
    }
    ret
}
