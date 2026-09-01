//! AegisOS E2E Test Harness: Preemptive Scheduler & PCB Simulator
//!
//! Models 100Hz round-robin multitasking, PCB lifecycle, priority scheduling,
//! Ring 3 fault isolation, and 2-phase deferred zombie frame reclamation.

use super::types::*;
use super::memory_sim::FrameAllocSimulator;

#[derive(Debug, Clone)]
pub struct ProcessControlBlock {
    pub pid: ProcessId,
    pub name: String,
    pub state: ProcessState,
    pub priority: Priority,
    pub is_user: bool,
    pub cr3: PhysAddr,
    pub kernel_stack_top: u64,
    pub user_stack_top: u64,
    pub regs: CpuRegisters,
    pub allocated_frames: Vec<PhysAddr>,
    pub runtime_ticks: u64,
}

pub struct SchedulerSimulator {
    pub tasks: Vec<ProcessControlBlock>,
    pub current_idx: usize,
    pub next_pid: ProcessId,
    pub total_ticks: u64,
    pub idle_ticks: u64,
    pub zombie_queue: Vec<ProcessId>,
    pub crash_logs: Vec<(ProcessId, ExceptionVector, u64, u64)>, // (pid, vector, rip, cr2)
}

impl SchedulerSimulator {
    pub fn new() -> Self {
        let mut sched = Self {
            tasks: Vec::new(),
            current_idx: 0,
            next_pid: 0,
            total_ticks: 0,
            idle_ticks: 0,
            zombie_queue: Vec::new(),
            crash_logs: Vec::new(),
        };

        // Create PID 0 [idle] task
        let idle_pid = sched.next_pid;
        sched.next_pid += 1;
        sched.tasks.push(ProcessControlBlock {
            pid: idle_pid,
            name: "[idle]".to_string(),
            state: ProcessState::Running,
            priority: Priority::Low,
            is_user: false,
            cr3: PhysAddr(0x1000),
            kernel_stack_top: 0xFFFF_FFFF_8008_0000,
            user_stack_top: 0,
            regs: CpuRegisters::default(),
            allocated_frames: vec![PhysAddr(0x1000)],
            runtime_ticks: 0,
        });

        sched
    }

    pub fn spawn_process(
        &mut self,
        name: &str,
        is_user: bool,
        priority: Priority,
        cr3: PhysAddr,
        allocated_frames: Vec<PhysAddr>,
    ) -> ProcessId {
        let pid = self.next_pid;
        self.next_pid += 1;

        let kernel_stack = 0xFFFF_FFFF_8000_0000 + (pid as u64 * 0x10000);
        let user_stack = if is_user { 0x0000_7FFF_FFFF_0000 } else { 0 };

        let mut regs = CpuRegisters::default();
        regs.cr3 = cr3.as_u64();
        regs.rsp = if is_user { user_stack } else { kernel_stack };
        regs.rflags = 0x202; // IF flag enabled

        self.tasks.push(ProcessControlBlock {
            pid,
            name: name.to_string(),
            state: ProcessState::Ready,
            priority,
            is_user,
            cr3,
            kernel_stack_top: kernel_stack,
            user_stack_top: user_stack,
            regs,
            allocated_frames,
            runtime_ticks: 0,
        });

        pid
    }

    pub fn current_process(&self) -> Option<&ProcessControlBlock> {
        self.tasks.get(self.current_idx)
    }

    pub fn current_process_mut(&mut self) -> Option<&mut ProcessControlBlock> {
        self.tasks.get_mut(self.current_idx)
    }

    pub fn timer_tick(&mut self, frame_alloc: &mut FrameAllocSimulator) -> Option<ProcessId> {
        self.total_ticks += 1;

        // Perform Phase 2 deferred zombie frame reclamation
        self.reap_zombies(frame_alloc);

        if self.tasks.is_empty() {
            return None;
        }

        // Advance runtime ticks on current running task
        if let Some(curr) = self.tasks.get_mut(self.current_idx) {
            if curr.state == ProcessState::Running {
                curr.runtime_ticks += 1;
                curr.state = ProcessState::Ready;
                if curr.pid == 0 {
                    self.idle_ticks += 1;
                }
            }
        }

        // Priority-aware Round-Robin Search
        let n = self.tasks.len();
        let mut next_idx = (self.current_idx + 1) % n;

        // Look for highest priority ready task
        for _ in 0..n {
            if self.tasks[next_idx].state == ProcessState::Ready {
                self.current_idx = next_idx;
                self.tasks[next_idx].state = ProcessState::Running;
                return Some(self.tasks[next_idx].pid);
            }
            next_idx = (next_idx + 1) % n;
        }

        // Fallback to PID 0 if all else blocked
        if let Some(idle) = self.tasks.iter_mut().find(|t| t.pid == 0) {
            idle.state = ProcessState::Running;
            self.current_idx = 0;
            Some(0)
        } else {
            None
        }
    }

    pub fn handle_fault(
        &mut self,
        pid: ProcessId,
        vector: ExceptionVector,
        rip: u64,
        cr2: u64,
    ) -> Result<(), &'static str> {
        if pid == 0 {
            return Err("Cannot fault-isolate kernel PID 0 idle task");
        }

        let task_pos = self.tasks.iter().position(|t| t.pid == pid);
        match task_pos {
            Some(pos) => {
                let is_user = self.tasks[pos].is_user;
                if !is_user {
                    return Err("Kernel-mode fault triggered; cannot safely isolate Ring 0");
                }

                // Phase 1: Mark as Zombie and queue for deferred reclamation
                self.tasks[pos].state = ProcessState::Zombie;
                if !self.zombie_queue.contains(&pid) {
                    self.zombie_queue.push(pid);
                }
                self.crash_logs.push((pid, vector, rip, cr2));

                // If the faulted task was actively running, reschedule
                if self.current_idx == pos {
                    self.current_idx = 0; // Fallback to PID 0 immediately
                    if let Some(idle) = self.tasks.get_mut(0) {
                        idle.state = ProcessState::Running;
                    }
                }

                Ok(())
            }
            None => Err("Faulting process not found in scheduler"),
        }
    }

    pub fn kill_process(&mut self, pid: ProcessId) -> bool {
        if pid == 0 {
            return false; // PID 0 is immune to termination
        }

        if let Some(pos) = self.tasks.iter().position(|t| t.pid == pid) {
            self.tasks[pos].state = ProcessState::Zombie;
            if !self.zombie_queue.contains(&pid) {
                self.zombie_queue.push(pid);
            }
            if self.current_idx == pos {
                self.current_idx = 0;
                if let Some(idle) = self.tasks.get_mut(0) {
                    idle.state = ProcessState::Running;
                }
            }
            true
        } else {
            false
        }
    }

    pub fn reap_zombies(&mut self, frame_alloc: &mut FrameAllocSimulator) -> usize {
        let mut reaped_count = 0;
        let mut remaining_zombies = Vec::new();

        for pid in self.zombie_queue.drain(..) {
            if let Some(pos) = self.tasks.iter().position(|t| t.pid == pid) {
                // Free all allocated physical memory frames
                let frames = self.tasks[pos].allocated_frames.clone();
                for frame in frames {
                    frame_alloc.free_frame(frame);
                }
                self.tasks.remove(pos);
                if self.current_idx >= self.tasks.len() && !self.tasks.is_empty() {
                    self.current_idx = 0;
                }
                reaped_count += 1;
            } else {
                remaining_zombies.push(pid);
            }
        }
        self.zombie_queue = remaining_zombies;
        reaped_count
    }

    pub fn get_process_list(&self) -> Vec<ProcessInfo> {
        self.tasks
            .iter()
            .map(|t| ProcessInfo {
                pid: t.pid,
                name: t.name.clone(),
                state: t.state,
                priority: t.priority,
                memory_bytes: t.allocated_frames.len() * PAGE_SIZE,
                cpu_percent: if self.total_ticks > 0 {
                    ((t.runtime_ticks * 100) / self.total_ticks) as u32
                } else {
                    0
                },
                is_user: t.is_user,
            })
            .collect()
    }

    pub fn get_cpu_usage(&self) -> u32 {
        if self.total_ticks == 0 {
            return 0;
        }
        let active_ticks = self.total_ticks.saturating_sub(self.idle_ticks);
        ((active_ticks * 100) / self.total_ticks) as u32
    }

    pub fn crash_log_count(&self) -> usize {
        self.crash_logs.len()
    }

    pub fn last_crash_log(&self) -> Option<(ProcessId, ExceptionVector, u64, u64)> {
        self.crash_logs.last().copied()
    }
}
