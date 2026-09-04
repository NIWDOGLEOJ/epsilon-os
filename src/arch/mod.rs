//! Architecture-Specific Hardware Management (x86_64)

pub mod gdt;
pub mod idt;
pub mod serial;
pub mod syscall;
pub mod time;

pub use time::FramePacer;

/// Saves `RFLAGS` and clears `IF`, restoring the original interrupt state on drop.
///
/// A plain spinlock shared between task context and an interrupt handler is a
/// deadlock waiting to happen on a single CPU: if the interrupt lands between the
/// acquire and the release, the handler spins forever on a lock the code it
/// interrupted still holds and can never release. Holding such a lock inside this
/// guard makes the whole critical section atomic with respect to interrupts.
///
/// Re-enabling is conditional on the saved state, so this nests correctly and is
/// a no-op inside a handler that is already running with interrupts masked.
///
/// # Bind the lock to a local, never leave it a temporary
///
/// ```ignore
/// let _guard = InterruptGuard::acquire();
/// SCHEDULER.lock().get_process_list()   // WRONG
/// ```
///
/// A function's local variables are dropped *before* the temporaries of its
/// tail expression. So `_guard` drops first and re-enables interrupts while the
/// lock is still held, and the lock guard drops after. A timer tick landing in
/// that window spins forever on a lock whose holder can never be resumed --
/// which showed up as an intermittent hang roughly one boot in ten, with the
/// timer ISR pinned on `SCHEDULER`.
///
/// Bind it instead, declared after the guard so it drops first:
///
/// ```ignore
/// let _guard = InterruptGuard::acquire();
/// let scheduler = SCHEDULER.lock();     // RIGHT
/// scheduler.get_process_list()
/// ```
pub struct InterruptGuard {
    was_enabled: bool,
}

impl InterruptGuard {
    #[inline(always)]
    pub fn acquire() -> Self {
        let rflags: u64;
        unsafe {
            core::arch::asm!(
                "pushfq",
                "pop {}",
                "cli",
                out(reg) rflags,
                // No `nostack`: pushfq/pop use the stack.
                // No `preserves_flags`: `cli` clears RFLAGS.IF.
                options(nomem)
            );
        }
        // RFLAGS.IF is bit 9.
        Self { was_enabled: (rflags & (1 << 9)) != 0 }
    }
}

impl Drop for InterruptGuard {
    #[inline(always)]
    fn drop(&mut self) {
        if self.was_enabled {
            unsafe { core::arch::asm!("sti", options(nomem, nostack)) };
        }
    }
}

/// Initializes core x86_64 architecture components: Serial COM1, GDT, TSS, IDT, and PIC.
pub fn init() -> (u16, u16, u16, u16, u16) {
    serial::init_serial();
    let selectors = gdt::init_gdt_tss();
    idt::init_idt();
    // Must follow the GDT: STAR encodes selectors that have to already be valid.
    syscall::init_syscall();
    selectors
}
