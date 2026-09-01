//! Process Control Block (PCB) and Task State Definitions for AegisOS
//!
//! Provides hardware-isolated task representations, CPU register state contexts,
//! privilege tracking (Ring 0 vs Ring 3), and runtime scheduling metadata.

use alloc::string::String;
use alloc::vec::Vec;
use crate::memory::{PhysAddr, VirtAddr, PAGE_SIZE};

pub type ProcessId = u64;

/// Execution state of a process in AegisOS.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskState {
    Ready,
    Running,
    Blocked(BlockReason),
    Zombie,
    Terminated(ExitReason),
}

/// Reason a task is blocked waiting for an event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockReason {
    Sleep(u64 /* wakeup_tick */),
    WaitChild(ProcessId),
    IpcReceive,
    IoWait,
}

/// Reason for task termination.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitReason {
    Normal(i32),
    PageFault { cr2: u64, error_code: u64 },
    DivideByZero,
    GeneralProtection { error_code: u64 },
    InvalidOpcode,
    KilledByAdmin,
}

/// Priority tiers for preemptive scheduling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TaskPriority {
    Low = 0,
    Normal = 1,
    High = 2,
    Realtime = 3,
}

/// Full CPU general-purpose and interrupt context state for a task.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct TaskContext {
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

    pub rip: u64,
    pub cs: u64,
    pub rflags: u64,
    pub rsp: u64,
    pub ss: u64,
}

impl TaskContext {
    pub const fn empty() -> Self {
        Self {
            r15: 0, r14: 0, r13: 0, r12: 0,
            r11: 0, r10: 0, r9:  0, r8:  0,
            rdi: 0, rsi: 0, rbp: 0, rdx: 0,
            rcx: 0, rbx: 0, rax: 0,
            rip: 0, cs: 0, rflags: 0x202, // IF (Interrupt Flag) enabled
            rsp: 0, ss: 0,
        }
    }

    pub fn new_kernel_task(entry: usize, kstack_top: u64, cs: u16, ds: u16) -> Self {
        Self {
            r15: 0, r14: 0, r13: 0, r12: 0,
            r11: 0, r10: 0, r9:  0, r8:  0,
            rdi: 0, rsi: 0, rbp: kstack_top, rdx: 0,
            rcx: 0, rbx: 0, rax: 0,
            rip: entry as u64,
            cs: cs as u64,
            rflags: 0x202, // IF=1
            rsp: kstack_top,
            ss: ds as u64,
        }
    }

    pub fn new_user_task(entry: usize, ustack_top: u64, user_cs: u16, user_ds: u16) -> Self {
        Self {
            r15: 0, r14: 0, r13: 0, r12: 0,
            r11: 0, r10: 0, r9:  0, r8:  0,
            rdi: 0, rsi: 0, rbp: ustack_top, rdx: 0,
            rcx: 0, rbx: 0, rax: 0,
            rip: entry as u64,
            cs: user_cs as u64,
            rflags: 0x202, // IF=1, IOPL=0
            rsp: ustack_top,
            ss: user_ds as u64,
        }
    }
}

/// Process Control Block (PCB) holding all runtime state for a process.
#[derive(Debug, Clone)]
pub struct ProcessControlBlock {
    pub pid: ProcessId,
    pub name: String,
    pub state: TaskState,
    pub priority: TaskPriority,
    pub is_user: bool,

    // Hardware Memory & Paging State
    pub pml4_root: PhysAddr,
    pub kernel_stack_bottom: VirtAddr,
    pub kernel_stack_top: VirtAddr,
    pub user_stack_top: VirtAddr,
    pub user_entry_point: VirtAddr,
    pub allocated_frames: Vec<PhysAddr>,

    // Saved Register State
    pub context: TaskContext,

    // Scheduling Metrics
    pub time_slice_remaining: u32,
    pub total_cpu_ticks: u64,

    // GUI Linkage
    pub window_id: Option<u64>,
}

impl ProcessControlBlock {
    pub fn is_alive(&self) -> bool {
        matches!(self.state, TaskState::Ready | TaskState::Running | TaskState::Blocked(_))
    }

    pub fn memory_usage_bytes(&self) -> usize {
        self.allocated_frames.len() * PAGE_SIZE
    }
}

/// Snapshot telemetry info for system diagnostic queries (e.g. Activity Monitor, `ps`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessInfo {
    pub pid: ProcessId,
    pub name: String,
    pub state: TaskState,
    pub priority: TaskPriority,
    pub memory_bytes: usize,
    pub cpu_percent: u32,
    pub is_user: bool,
}
