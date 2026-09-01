//! Ring 3 Fault Isolation and Exception Recovery Engine for AegisOS
//!
//! Catches user-mode faults (#PF, #DE, #GP, #UD), logs diagnostic post-mortems,
//! marks offending processes as terminated, triggers 2-phase deferred zombie reclamation,
//! and context-switches to the next ready task without panicking the kernel or freezing the desktop.

use spin::Mutex;
use crate::arch::idt::InterruptContext;
use crate::task::pcb::{ExitReason, ProcessId, TaskState};
use crate::task::scheduler::SCHEDULER;

pub type CrashCallback = fn(pid: ProcessId, fault_name: &str, rip: u64, cr2: u64);

static CRASH_CALLBACK: Mutex<Option<CrashCallback>> = Mutex::new(None);

/// Registers a system crash callback (e.g. for GUI Activity Monitor crash logging).
pub fn register_crash_callback(cb: CrashCallback) {
    // CRASH_CALLBACK is read from exception context in `handle_user_fault`.
    let _guard = crate::arch::InterruptGuard::acquire();
    *CRASH_CALLBACK.lock() = Some(cb);
}

/// Primary Fault Isolation Handler for Ring 3 Userspace Exceptions.
///
/// Invoked by `arch::idt::handle_exception` when `(CS & 3) == 3`.
pub fn handle_user_fault(vector: u64, ctx: &mut InterruptContext, cr2: u64) {
    let (fault_name, exit_reason) = match vector {
        0 => ("Divide-by-Zero (#DE)", ExitReason::DivideByZero),
        6 => ("Invalid Opcode (#UD)", ExitReason::InvalidOpcode),
        13 => (
            "General Protection Fault (#GP)",
            ExitReason::GeneralProtection { error_code: ctx.error_code },
        ),
        14 => (
            "Page Fault (#PF)",
            ExitReason::PageFault { cr2, error_code: ctx.error_code },
        ),
        _ => ("Unexpected User Exception", ExitReason::Normal(-1)),
    };

    let mut sched = SCHEDULER.lock();
    let current_idx = sched.current_idx;

    if let Some(pcb) = sched.tasks.get_mut(current_idx) {
        let pid = pcb.pid;

        // 1. Diagnostic serial logging.
        //    Formatted straight from the PCB rather than through a `String` clone:
        //    this runs in exception context, where allocating risks deadlocking
        //    against a task interrupted inside the global allocator.
        crate::arch::serial::_print(format_args!(
            "[FAULT-ISOLATION] Process PID {} ('{}') crashed due to {} at RIP 0x{:016x} CR2=0x{:016x}\n",
            pid, pcb.name, fault_name, ctx.rip, cr2
        ));

        // 2. Mark PCB as Terminated and queue for Phase 2 deferred reclamation
        pcb.state = TaskState::Terminated(exit_reason);
        if !sched.zombie_queue.contains(&pid) {
            sched.zombie_queue.push(pid);
        }

        // 3. Notify external crash callback if registered
        drop(sched);
        if let Some(cb) = *CRASH_CALLBACK.lock() {
            cb(pid, fault_name, ctx.rip, cr2);
        }
        let mut sched = SCHEDULER.lock();

        // 4. Force immediate schedule to next ready task
        sched.schedule(ctx);
    } else {
        crate::arch::serial::_print(format_args!(
            "[FAULT-ISOLATION] Fault occurred with no active PCB in scheduler.\n"
        ));
    }
}
