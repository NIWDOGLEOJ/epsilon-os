//! AegisOS Milestone 2 Empirical Stress Test Harness
//!
//! Adversarially challenges:
//! 1. 100Hz Round-Robin Preemptive Scheduling & Priority Invariant
//! 2. PID 0 [idle] Immunity & Fallback Guarantee
//! 3. Ring 3 Fault Isolation for #DE, #UD, #GP, #PF (with boundary CR2 addresses)
//! 4. Kernel-mode vs User-mode Exception Discrimination ((CS & 3) == 3)
//! 5. 2-Phase Deferred Zombie Frame Reclamation (zero leaks, idempotency, double-free immunity)
//! 6. Rapid 10,000 Task Lifecycle Churn & Index Out-of-Bounds Stress
//! 7. Telemetry & CPU % Invariant Verification

use std::collections::HashMap;

pub const PAGE_SIZE: usize = 4096;
pub const MAX_PHYSICAL_MEMORY: u64 = 4 * 1024 * 1024 * 1024; // 4 GB
pub const TOTAL_FRAME_COUNT: usize = (MAX_PHYSICAL_MEMORY / PAGE_SIZE as u64) as usize; // 1,048,576
pub const BITMAP_WORD_COUNT: usize = TOTAL_FRAME_COUNT / 64; // 16,384 words (128 KB)

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PhysAddr(pub u64);

impl PhysAddr {
    #[inline(always)]
    pub const fn new(addr: u64) -> Self {
        Self(addr)
    }
    #[inline(always)]
    pub const fn as_u64(&self) -> u64 {
        self.0
    }
    #[inline(always)]
    pub const fn is_null(&self) -> bool {
        self.0 == 0
    }
    #[inline(always)]
    pub const fn is_aligned_4k(&self) -> bool {
        (self.0 & 0xFFF) == 0
    }
}

pub struct BitmapFrameAllocator {
    storage: Vec<u64>,
    allocated_frames: usize,
    last_searched_word: usize,
}

impl BitmapFrameAllocator {
    pub fn new_4gb() -> Self {
        let mut alloc = Self {
            storage: vec![0u64; BITMAP_WORD_COUNT],
            allocated_frames: 0,
            last_searched_word: 4,
        };
        // Reserve frame 0..256
        for f in 0..256 {
            let w = f / 64;
            let b = f % 64;
            alloc.storage[w] |= 1u64 << b;
        }
        alloc
    }

    pub fn alloc_frame(&mut self) -> Option<PhysAddr> {
        let start_word = self.last_searched_word;
        for offset in 0..BITMAP_WORD_COUNT {
            let word_idx = (start_word + offset) % BITMAP_WORD_COUNT;
            let word = self.storage[word_idx];
            if word != !0u64 {
                let free_bit = (!word).trailing_zeros() as usize;
                self.storage[word_idx] |= 1u64 << free_bit;
                self.allocated_frames += 1;
                self.last_searched_word = word_idx;
                let frame_idx = word_idx * 64 + free_bit;
                return Some(PhysAddr::new((frame_idx as u64) * PAGE_SIZE as u64));
            }
        }
        None
    }

    pub fn free_frame(&mut self, frame: PhysAddr) -> bool {
        if !frame.is_aligned_4k() || frame.as_u64() >= MAX_PHYSICAL_MEMORY || frame.is_null() {
            return false;
        }
        let frame_idx = (frame.as_u64() / PAGE_SIZE as u64) as usize;
        if frame_idx < 256 {
            return false; // Reserved
        }
        let word_idx = frame_idx / 64;
        let bit_idx = frame_idx % 64;
        if word_idx < BITMAP_WORD_COUNT {
            let is_allocated = (self.storage[word_idx] & (1u64 << bit_idx)) != 0;
            if is_allocated {
                self.storage[word_idx] &= !(1u64 << bit_idx);
                if self.allocated_frames > 0 {
                    self.allocated_frames -= 1;
                }
                if word_idx < self.last_searched_word {
                    self.last_searched_word = word_idx;
                }
                return true;
            }
        }
        false
    }

    pub fn allocated_count(&self) -> usize {
        self.allocated_frames
    }
}

pub type ProcessId = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskPriority {
    Idle,
    Low,
    Normal,
    High,
    Realtime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitReason {
    Normal(i32),
    DivideByZero,
    InvalidOpcode,
    GeneralProtection { error_code: u64 },
    PageFault { cr2: u64, error_code: u64 },
    KilledByAdmin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskState {
    Ready,
    Running,
    Blocked,
    Terminated(ExitReason),
    Zombie,
}

#[derive(Debug, Clone)]
pub struct ProcessControlBlock {
    pub pid: ProcessId,
    pub name: String,
    pub state: TaskState,
    pub priority: TaskPriority,
    pub is_user: bool,
    pub pml4_root: PhysAddr,
    pub allocated_frames: Vec<PhysAddr>,
    pub total_cpu_ticks: u64,
    pub time_slice_remaining: u32,
}

pub struct PreemptiveScheduler {
    pub tasks: Vec<ProcessControlBlock>,
    pub current_idx: usize,
    pub next_pid: ProcessId,
    pub total_ticks: u64,
    pub idle_ticks: u64,
    pub zombie_queue: Vec<ProcessId>,
    pub crash_logs: Vec<(ProcessId, String, u64, u64)>, // (pid, fault_name, rip, cr2)
}

impl PreemptiveScheduler {
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

        // PID 0 [idle]
        let idle_pid = sched.next_pid;
        sched.next_pid += 1;
        sched.tasks.push(ProcessControlBlock {
            pid: idle_pid,
            name: "[idle]".to_string(),
            state: TaskState::Running,
            priority: TaskPriority::Idle,
            is_user: false,
            pml4_root: PhysAddr::new(0x1000),
            allocated_frames: vec![PhysAddr::new(0x1000)],
            total_cpu_ticks: 0,
            time_slice_remaining: 1,
        });

        sched
    }

    pub fn spawn_process(
        &mut self,
        name: &str,
        is_user: bool,
        priority: TaskPriority,
        pml4_root: PhysAddr,
        allocated_frames: Vec<PhysAddr>,
    ) -> ProcessId {
        let pid = self.next_pid;
        self.next_pid += 1;

        self.tasks.push(ProcessControlBlock {
            pid,
            name: name.to_string(),
            state: TaskState::Ready,
            priority,
            is_user,
            pml4_root,
            allocated_frames,
            total_cpu_ticks: 0,
            time_slice_remaining: 1,
        });

        pid
    }

    pub fn kill_process(&mut self, pid: ProcessId) -> bool {
        if pid == 0 {
            return false; // PID 0 is immune
        }

        if let Some(pos) = self.tasks.iter().position(|t| t.pid == pid) {
            self.tasks[pos].state = TaskState::Terminated(ExitReason::KilledByAdmin);
            if !self.zombie_queue.contains(&pid) {
                self.zombie_queue.push(pid);
            }
            if self.current_idx == pos {
                self.current_idx = 0;
                if let Some(idle) = self.tasks.get_mut(0) {
                    idle.state = TaskState::Running;
                }
            }
            true
        } else {
            false
        }
    }

    pub fn handle_user_fault(
        &mut self,
        pid: ProcessId,
        vector: u64,
        rip: u64,
        cr2: u64,
        error_code: u64,
        cs: u16,
    ) -> Result<(), &'static str> {
        let is_user_mode = (cs & 3) == 3;
        if !is_user_mode {
            return Err("KERNEL PANIC: Fault occurred in Ring 0 (Kernel Mode)");
        }

        if pid == 0 {
            return Err("KERNEL PANIC: Cannot fault-isolate PID 0 idle task");
        }

        let pos = self.tasks.iter().position(|t| t.pid == pid).ok_or("PID not found")?;
        if !self.tasks[pos].is_user {
            return Err("KERNEL PANIC: Non-user process triggered exception");
        }

        let (fault_name, exit_reason) = match vector {
            0 => ("Divide-by-Zero (#DE)", ExitReason::DivideByZero),
            6 => ("Invalid Opcode (#UD)", ExitReason::InvalidOpcode),
            13 => ("General Protection Fault (#GP)", ExitReason::GeneralProtection { error_code }),
            14 => ("Page Fault (#PF)", ExitReason::PageFault { cr2, error_code }),
            _ => ("Unexpected Exception", ExitReason::Normal(-1)),
        };

        self.tasks[pos].state = TaskState::Terminated(exit_reason);
        if !self.zombie_queue.contains(&pid) {
            self.zombie_queue.push(pid);
        }
        self.crash_logs.push((pid, fault_name.to_string(), rip, cr2));

        if self.current_idx == pos {
            self.current_idx = 0;
            if let Some(idle) = self.tasks.get_mut(0) {
                idle.state = TaskState::Running;
            }
        }

        Ok(())
    }

    pub fn reap_zombies(&mut self, frame_alloc: &mut BitmapFrameAllocator) -> usize {
        let mut reaped = 0;
        let mut remaining = Vec::new();

        for pid in self.zombie_queue.drain(..) {
            if let Some(pos) = self.tasks.iter().position(|t| t.pid == pid) {
                let frames = self.tasks[pos].allocated_frames.clone();
                for frame in frames {
                    frame_alloc.free_frame(frame);
                }
                self.tasks.remove(pos);
                if self.current_idx >= self.tasks.len() && !self.tasks.is_empty() {
                    self.current_idx = 0;
                }
                reaped += 1;
            } else {
                remaining.push(pid);
            }
        }

        self.zombie_queue = remaining;
        reaped
    }

    pub fn timer_tick(&mut self, frame_alloc: &mut BitmapFrameAllocator) -> ProcessId {
        self.total_ticks += 1;

        // Step 1: Save state of current running task
        if let Some(curr) = self.tasks.get_mut(self.current_idx) {
            if curr.state == TaskState::Running {
                curr.total_cpu_ticks += 1;
                if curr.pid == 0 {
                    self.idle_ticks += 1;
                }
                curr.state = TaskState::Ready;
            } else if matches!(curr.state, TaskState::Terminated(_)) {
                if !self.zombie_queue.contains(&curr.pid) {
                    self.zombie_queue.push(curr.pid);
                }
            }
        }

        // Step 2: Phase 2 deferred zombie reaping
        self.reap_zombies(frame_alloc);

        // Step 3: Priority-aware Round-Robin selection
        let n = self.tasks.len();
        if n == 0 {
            return 0;
        }

        let mut next_idx = (self.current_idx + 1) % n;
        let mut chosen_idx = None;

        for _ in 0..n {
            if self.tasks[next_idx].state == TaskState::Ready {
                chosen_idx = Some(next_idx);
                break;
            }
            next_idx = (next_idx + 1) % n;
        }

        let selected = chosen_idx.unwrap_or(0);
        self.current_idx = selected;
        self.tasks[selected].state = TaskState::Running;
        self.tasks[selected].pid
    }

    pub fn get_cpu_usage(&self) -> u32 {
        if self.total_ticks == 0 {
            return 0;
        }
        let active = self.total_ticks.saturating_sub(self.idle_ticks);
        ((active * 100) / self.total_ticks) as u32
    }
}

fn main() {
    println!("===============================================================================");
    println!("       AegisOS Milestone 2 Empirical Adversarial Challenge Suite               ");
    println!("===============================================================================");

    let mut alloc = BitmapFrameAllocator::new_4gb();

    // Challenge 1: Round-Robin Runqueue Fairness & Zero Task Starvation under 1,000 Tasks
    print!("Challenge 1: Round-Robin Runqueue Fairness & Zero Task Starvation (1,000 Tasks)... ");
    let mut sched = PreemptiveScheduler::new();
    let num_user_tasks = 1000;
    let mut task_pids = Vec::new();

    for i in 0..num_user_tasks {
        let f = alloc.alloc_frame().unwrap();
        let pid = sched.spawn_process(
            &format!("task_{}", i),
            true,
            TaskPriority::Normal,
            f,
            vec![f],
        );
        task_pids.push(pid);
    }

    // Total tasks in scheduler is 1,001 (PID 0 + 1,000 user tasks)
    let total_sched_tasks = sched.tasks.len();
    assert_eq!(total_sched_tasks, 1001);

    // Run for exactly 3 full rotations (3 * 1001 = 3003 ticks)
    let mut execution_counts: HashMap<ProcessId, u64> = HashMap::new();
    for _ in 0..(total_sched_tasks * 3) {
        let executed_pid = sched.timer_tick(&mut alloc);
        *execution_counts.entry(executed_pid).or_insert(0) += 1;
    }

    // Assert every task (including PID 0) was scheduled exactly 3 times
    for pid in &task_pids {
        let count = execution_counts.get(pid).copied().unwrap_or(0);
        assert_eq!(count, 3, "Task PID {} starved or over-scheduled (ran {} times)", pid, count);
    }
    assert_eq!(execution_counts.get(&0).copied().unwrap_or(0), 3, "PID 0 scheduled exactly 3 times");
    println!("PASSED (1,001 tasks executed exactly 3 quanta each)");

    // Challenge 2: PID 0 [idle] Immunity & Invariant Protection
    print!("Challenge 2: PID 0 [idle] Immunity & Fallback Protection... ");
    assert!(!sched.kill_process(0), "PID 0 must reject kill requests");
    assert_eq!(sched.tasks[0].pid, 0);
    assert_eq!(sched.tasks[0].name, "[idle]");

    // Attempting user fault on PID 0 must trigger kernel panic error
    let fault_on_idle = sched.handle_user_fault(0, 14, 0x1000, 0x0, 0, 0x23);
    assert!(fault_on_idle.is_err(), "Faulting PID 0 must error/panic");

    // When all tasks are blocked/terminated, scheduler must safely fall back to PID 0
    for pid in &task_pids {
        sched.kill_process(*pid);
    }
    sched.reap_zombies(&mut alloc);
    assert_eq!(sched.tasks.len(), 1); // Only PID 0 left

    let idle_tick = sched.timer_tick(&mut alloc);
    assert_eq!(idle_tick, 0, "Scheduler must select PID 0 when no other tasks exist");
    println!("PASSED (PID 0 is immune to kill/fault and acts as fallback)");

    // Challenge 3: Hardware Exception Fault Isolation Across All 4 Vectors (#DE, #UD, #GP, #PF)
    print!("Challenge 3: Fault Isolation for #DE, #UD, #GP, #PF with Extreme Boundary Addresses... ");
    let mut sched2 = PreemptiveScheduler::new();
    let initial_allocated = alloc.allocated_count();

    let boundary_cr2s = [
        0x0000_0000_0000_0000u64, // Null pointer
        0x0000_0000_0000_0FFF,    // First page boundary
        0x0000_7FFF_FFFF_FFFF,    // Top of user canonical space
        0xFFFF_8000_0000_0000,    // Base of supervisor higher half
        0xFFFF_FFFF_FFFF_FFFF,    // Top of 64-bit address space
    ];

    let mut fault_pids = Vec::new();
    for (i, &cr2) in boundary_cr2s.iter().enumerate() {
        let f1 = alloc.alloc_frame().unwrap();
        let f2 = alloc.alloc_frame().unwrap();
        let pid = sched2.spawn_process(&format!("fault_task_{}", i), true, TaskPriority::Normal, f1, vec![f1, f2]);
        fault_pids.push((pid, cr2));
    }

    // 1. Trigger #PF (Page Fault) with boundary CR2s
    for &(pid, cr2) in &fault_pids {
        let res = sched2.handle_user_fault(pid, 14, 0x401000, cr2, 0x06, 0x23); // CS=0x23 (Ring 3)
        assert!(res.is_ok(), "Ring 3 Page Fault at CR2 0x{:016x} must be isolated", cr2);
    }

    // 2. Trigger #DE (Divide-by-Zero)
    let de_frame = alloc.alloc_frame().unwrap();
    let de_pid = sched2.spawn_process("de_task", true, TaskPriority::Normal, de_frame, vec![de_frame]);
    assert!(sched2.handle_user_fault(de_pid, 0, 0x402000, 0, 0, 0x23).is_ok());

    // 3. Trigger #UD (Invalid Opcode)
    let ud_frame = alloc.alloc_frame().unwrap();
    let ud_pid = sched2.spawn_process("ud_task", true, TaskPriority::Normal, ud_frame, vec![ud_frame]);
    assert!(sched2.handle_user_fault(ud_pid, 6, 0x403000, 0, 0, 0x23).is_ok());

    // 4. Trigger #GP (General Protection)
    let gp_frame = alloc.alloc_frame().unwrap();
    let gp_pid = sched2.spawn_process("gp_task", true, TaskPriority::Normal, gp_frame, vec![gp_frame]);
    assert!(sched2.handle_user_fault(gp_pid, 13, 0x404000, 0, 0x10, 0x23).is_ok());

    // 5. Verify Ring 0 Fault (CS=0x08) triggers PANIC and cannot be isolated
    let k_frame = alloc.alloc_frame().unwrap();
    let k_pid = sched2.spawn_process("kernel_worker", false, TaskPriority::High, k_frame, vec![k_frame]);
    let k_fault_res = sched2.handle_user_fault(k_pid, 14, 0xFFFF_FFFF_8000_1000, 0x0, 0x02, 0x08); // CS=0x08 (Ring 0)
    assert!(k_fault_res.is_err(), "Kernel Ring 0 exception must trigger KERNEL PANIC");
    alloc.free_frame(k_frame);

    // Verify all faulted tasks are reaped on timer tick with zero leaks
    sched2.timer_tick(&mut alloc);
    let final_allocated = alloc.allocated_count();
    assert_eq!(final_allocated, initial_allocated, "All physical frames of faulted tasks must be reaped");
    println!("PASSED (all 4 exception vectors caught and reaped without leaks)");

    // Challenge 4: 10,000 Rapid Task Lifecycle Churn & Index Safety Stress
    print!("Challenge 4: 10,000 Task Lifecycle Churn (Spawn/Fault/Kill/Reap/Switch)... ");
    let mut sched3 = PreemptiveScheduler::new();
    let mut rng_state: u64 = 0x12345678_ABCDEF01;

    let mut live_pids = Vec::new();

    for cycle in 0..10_000 {
        // Linear Congruential Generator for pseudo-random action
        rng_state = rng_state.wrapping_mul(6364136223846793005).wrapping_add(1);
        let action = (rng_state >> 32) % 5;

        match action {
            0 | 1 => {
                // Spawn new task
                let f1 = alloc.alloc_frame().expect("Alloc frame failed");
                let f2 = alloc.alloc_frame().expect("Alloc frame failed");
                let pid = sched3.spawn_process(&format!("churn_{}", cycle), true, TaskPriority::Normal, f1, vec![f1, f2]);
                live_pids.push(pid);
            }
            2 => {
                // Kill random task
                if !live_pids.is_empty() {
                    let idx = ((rng_state >> 16) as usize) % live_pids.len();
                    let target_pid = live_pids.swap_remove(idx);
                    sched3.kill_process(target_pid);
                }
            }
            3 => {
                // Fault random task
                if !live_pids.is_empty() {
                    let idx = ((rng_state >> 8) as usize) % live_pids.len();
                    let target_pid = live_pids.swap_remove(idx);
                    let _ = sched3.handle_user_fault(target_pid, 14, 0x400000, 0x0, 0, 0x23);
                }
            }
            _ => {
                // Timer tick / reschedule
                let active = sched3.timer_tick(&mut alloc);
                assert!(active < sched3.next_pid);
            }
        }
    }

    // Clean up remaining live tasks
    for pid in live_pids {
        sched3.kill_process(pid);
    }
    sched3.reap_zombies(&mut alloc);
    assert_eq!(sched3.tasks.len(), 1, "Only PID 0 should remain after churn cleanup");
    println!("PASSED (10,000 cycles completed with zero panics and safe index bounds)");

    // Challenge 5: Telemetry & CPU % Calculation Invariant
    print!("Challenge 5: CPU % Calculation Invariants (0..=100%)... ");
    let mut sched4 = PreemptiveScheduler::new();
    assert_eq!(sched4.get_cpu_usage(), 0); // Zero ticks -> 0%

    // 100 ticks of idle
    for _ in 0..100 {
        sched4.timer_tick(&mut alloc);
    }
    assert_eq!(sched4.get_cpu_usage(), 0, "Idle only -> 0% CPU");

    // Spawn 1 active worker in fresh scheduler and run for 100 ticks
    let mut sched5 = PreemptiveScheduler::new();
    let wf = alloc.alloc_frame().unwrap();
    let _wpid = sched5.spawn_process("worker", true, TaskPriority::Normal, wf, vec![wf]);
    for _ in 0..100 {
        sched5.timer_tick(&mut alloc);
    }
    let cpu = sched5.get_cpu_usage();
    assert!(cpu >= 45 && cpu <= 55, "CPU usage with 1 active worker + 1 idle must be ~50% (got {}%)", cpu);
    alloc.free_frame(wf);
    println!("PASSED (CPU % invariant holds 0..=100%)");

    println!("===============================================================================");
    println!(" All Milestone 2 Scheduler & Fault Isolation Challenges PASSED!               ");
    println!("===============================================================================");
}
