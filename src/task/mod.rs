//! Task Management, Preemptive Scheduling and Ring 3 Fault Isolation for AegisOS
//!
//! Exposes the 100Hz Round-Robin Preemptive Scheduler, Process Control Blocks (PCB),
//! 2-Phase Deferred Zombie Reaping, and Hardware Fault Isolation.

pub mod context;
pub mod elf;
pub mod fault;
pub mod pcb;
pub mod scheduler;
pub mod userprogs;

pub use elf::{load_elf, ElfError};
pub use fault::{handle_user_fault, register_crash_callback, CrashCallback};
pub use pcb::{
    BlockReason, ExitReason, ProcessControlBlock, ProcessId, ProcessInfo, TaskContext,
    TaskPriority, TaskState,
};
pub use scheduler::{
    current_pid, get_cpu_usage, get_memory_stats, get_process_list, get_uptime_ticks,
    idle_task_entry, init,
    kill_process, on_timer_tick, reap_zombies, spawn_process, spawn_user_bytecode, spawn_user_elf,
    spawn_user_fault_test,
    Scheduler, DEFAULT_QUANTUM_TICKS, KERNEL_STACK_SIZE, SCHEDULER,
};

/// Initializes the task and scheduler subsystem, connects Timer IRQ 0 and fault isolation hooks.
pub fn init_task_subsystem() {
    scheduler::init();
    crate::arch::idt::register_timer_callback(scheduler::on_timer_tick);
    crate::arch::idt::register_fault_callback(fault::handle_user_fault);
}
