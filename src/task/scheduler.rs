//! Preemptive Round-Robin Task Scheduler for AegisOS
//!
//! Enforces 100Hz hardware timer preemption, priority-tier round-robin dispatching,
//! 2-phase deferred zombie resource reclamation, and telemetry statistics.

use alloc::boxed::Box;
use alloc::string::ToString;
use alloc::vec::Vec;
use spin::Mutex;

use crate::arch::gdt::{KERNEL_CODE_SELECTOR, KERNEL_DATA_SELECTOR, USER_CODE_SELECTOR, USER_DATA_SELECTOR};
use crate::arch::idt::InterruptContext;
use crate::memory::{
    alloc_zeroed_frame, create_user_address_space, destroy_user_address_space,
    free_frame, get_kernel_pml4, map_page, phys_to_virt, PageTableFlags, VirtAddr, PAGE_SIZE,
};
use crate::task::context::{restore_context_to_interrupt, save_context_from_interrupt};
use crate::task::elf::ElfError;
use crate::task::pcb::{ExitReason, ProcessControlBlock, ProcessId, ProcessInfo, TaskContext, TaskPriority, TaskState};

pub const DEFAULT_QUANTUM_TICKS: u32 = 1; // 1 tick @ 100Hz = 10ms quantum
pub const KERNEL_STACK_SIZE: usize = 32768; // 32 KiB

pub struct Scheduler {
    pub tasks: Vec<ProcessControlBlock>,
    pub current_idx: usize,
    pub next_pid: ProcessId,
    pub total_ticks: u64,
    pub idle_ticks: u64,
    /// Pushed from the timer ISR and from fault context, so it must never grow:
    /// see `EventRing`.
    pub zombie_queue: ZombieQueue,
    pub is_initialized: bool,
}

impl Scheduler {
    pub const fn new() -> Self {
        Self {
            tasks: Vec::new(),
            current_idx: 0,
            next_pid: 0,
            total_ticks: 0,
            idle_ticks: 0,
            zombie_queue: ZombieQueue::new(),
            is_initialized: false,
        }
    }

    /// Initializes PID 0 [idle] task and enables scheduler.
    pub fn init(&mut self) {
        if self.is_initialized {
            return;
        }

        let idle_pid = self.next_pid;
        self.next_pid += 1;

        let kernel_pml4 = get_kernel_pml4();
        let kstack = Box::leak(Box::new([0u8; KERNEL_STACK_SIZE]));
        let kstack_top = kstack.as_ptr() as u64 + KERNEL_STACK_SIZE as u64;

        let idle_pcb = ProcessControlBlock {
            pid: idle_pid,
            name: "[idle]".to_string(),
            state: TaskState::Running,
            priority: TaskPriority::Low,
            is_user: false,
            pml4_root: kernel_pml4,
            kernel_stack_bottom: VirtAddr::new(kstack.as_ptr() as u64),
            kernel_stack_top: VirtAddr::new(kstack_top),
            user_stack_top: VirtAddr::new(0),
            user_entry_point: VirtAddr::new(idle_task_entry as *const () as usize as u64),
            allocated_frames: Vec::new(),
            context: TaskContext::new_kernel_task(
                idle_task_entry as *const () as usize,
                kstack_top,
                KERNEL_CODE_SELECTOR,
                KERNEL_DATA_SELECTOR,
            ),
            time_slice_remaining: DEFAULT_QUANTUM_TICKS,
            total_cpu_ticks: 0,
            window_id: None,
        };

        self.tasks.push(idle_pcb);
        self.current_idx = 0;
        self.is_initialized = true;
    }

    /// Spawns a new kernel or user task.
    pub fn spawn_process(
        &mut self,
        name: &str,
        entry: extern "C" fn(),
        is_user: bool,
    ) -> ProcessId {
        let pid = self.next_pid;
        self.next_pid += 1;

        let mut allocated_frames = Vec::new();

        // 1. Allocate Kernel Stack for RSP0
        let kstack = Box::leak(Box::new([0u8; KERNEL_STACK_SIZE]));
        let kstack_top = kstack.as_ptr() as u64 + KERNEL_STACK_SIZE as u64;
        let kstack_bottom = kstack.as_ptr() as u64;

        let (pml4_root, user_stack_top, user_entry_point, context) = if is_user {
            // Create private isolated user address space
            let user_pml4 = create_user_address_space();
            allocated_frames.push(user_pml4);

            // Map User Stack at top of user lower-half (0x0000_7FFF_FFFF_0000)
            let ustack_virt = VirtAddr::new(0x0000_7FFF_FFFF_0000 - PAGE_SIZE as u64);
            let ustack_frame = alloc_zeroed_frame().expect("Failed to allocate user stack frame");
            allocated_frames.push(ustack_frame);

            map_page(
                user_pml4,
                ustack_virt,
                ustack_frame,
                PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE,
            );

            let ustack_top_addr = 0x0000_7FFF_FFFF_0000u64;

            // Map User Code Page at entry point
            let code_virt = VirtAddr::new((entry as usize as u64) & !0xFFFu64);
            let code_phys = crate::memory::translate_addr(get_kernel_pml4(), code_virt)
                .unwrap_or(ustack_frame);

            map_page(
                user_pml4,
                code_virt,
                code_phys,
                PageTableFlags::PRESENT | PageTableFlags::USER_ACCESSIBLE | PageTableFlags::WRITABLE,
            );

            let ctx = TaskContext::new_user_task(
                entry as usize,
                ustack_top_addr,
                USER_CODE_SELECTOR,
                USER_DATA_SELECTOR,
            );

            (user_pml4, VirtAddr::new(ustack_top_addr), VirtAddr::new(entry as usize as u64), ctx)
        } else {
            // Kernel task shares kernel PML4
            let kernel_pml4 = get_kernel_pml4();
            let ctx = TaskContext::new_kernel_task(
                entry as usize,
                kstack_top,
                KERNEL_CODE_SELECTOR,
                KERNEL_DATA_SELECTOR,
            );
            (kernel_pml4, VirtAddr::new(0), VirtAddr::new(entry as usize as u64), ctx)
        };

        let pcb = ProcessControlBlock {
            pid,
            name: name.to_string(),
            state: TaskState::Ready,
            priority: TaskPriority::Normal,
            is_user,
            pml4_root,
            kernel_stack_bottom: VirtAddr::new(kstack_bottom),
            kernel_stack_top: VirtAddr::new(kstack_top),
            user_stack_top,
            user_entry_point,
            allocated_frames,
            context,
            time_slice_remaining: DEFAULT_QUANTUM_TICKS,
            total_cpu_ticks: 0,
            window_id: None,
        };

        self.tasks.push(pcb);
        pid
    }

    /// Loads an ELF64 executable into a private address space and spawns it in Ring 3.
    ///
    /// Unlike `spawn_user_bytecode`, the image is parsed rather than trusted: the
    /// header is validated, segment ranges are bounds-checked against user space,
    /// and per-segment permissions are carried into the page tables, so a
    /// read-only segment is mapped read-only and a non-executable one is mapped
    /// `NO_EXECUTE`.
    ///
    /// On failure every frame already allocated -- the PML4, the stack, and
    /// whatever the loader mapped before it gave up -- is released before
    /// returning, so a rejected image costs nothing permanent.
    pub fn spawn_user_elf(&mut self, name: &str, image: &[u8]) -> Result<ProcessId, ElfError> {
        let mut allocated_frames = Vec::new();

        let user_pml4 = create_user_address_space();
        allocated_frames.push(user_pml4);

        // User stack at the top of the lower half. `elf::parse` rejects any
        // segment that would reach into it.
        for page in 0..crate::task::elf::USER_STACK_PAGES {
            let virt =
                VirtAddr::new(crate::task::elf::USER_STACK_BOTTOM + (page * PAGE_SIZE) as u64);
            let frame = match alloc_zeroed_frame() {
                Some(f) => f,
                None => {
                    for frame in allocated_frames {
                        free_frame(frame);
                    }
                    return Err(ElfError::OutOfMemory);
                }
            };
            allocated_frames.push(frame);

            map_page(
                user_pml4,
                virt,
                frame,
                PageTableFlags::PRESENT
                    | PageTableFlags::WRITABLE
                    | PageTableFlags::USER_ACCESSIBLE
                    | PageTableFlags::NO_EXECUTE,
            );
        }

        let entry = match crate::task::elf::load_elf(image, user_pml4, &mut allocated_frames) {
            Ok(entry) => entry,
            Err(e) => {
                for frame in allocated_frames {
                    free_frame(frame);
                }
                return Err(e);
            }
        };

        let pid = self.next_pid;
        self.next_pid += 1;

        let kstack = Box::leak(Box::new([0u8; KERNEL_STACK_SIZE]));
        let kstack_top = kstack.as_ptr() as u64 + KERNEL_STACK_SIZE as u64;
        let kstack_bottom = kstack.as_ptr() as u64;

        let ustack_top_addr = crate::task::elf::USER_STACK_TOP;
        let context = TaskContext::new_user_task(
            entry.as_u64() as usize,
            ustack_top_addr,
            USER_CODE_SELECTOR,
            USER_DATA_SELECTOR,
        );

        self.tasks.push(ProcessControlBlock {
            pid,
            name: name.to_string(),
            state: TaskState::Ready,
            priority: TaskPriority::Normal,
            is_user: true,
            pml4_root: user_pml4,
            kernel_stack_bottom: VirtAddr::new(kstack_bottom),
            kernel_stack_top: VirtAddr::new(kstack_top),
            user_stack_top: VirtAddr::new(ustack_top_addr),
            user_entry_point: entry,
            allocated_frames,
            context,
            time_slice_remaining: DEFAULT_QUANTUM_TICKS,
            total_cpu_ticks: 0,
            window_id: None,
        });

        Ok(pid)
    }

    /// Spawns a user process executing raw machine code bytes in lower-half user space (0x00400000).
    pub fn spawn_user_bytecode(&mut self, name: &str, bytecode: &[u8]) -> ProcessId {
        let pid = self.next_pid;
        self.next_pid += 1;

        let mut allocated_frames = Vec::new();

        // 1. Allocate Kernel Stack for RSP0
        let kstack = Box::leak(Box::new([0u8; KERNEL_STACK_SIZE]));
        let kstack_top = kstack.as_ptr() as u64 + KERNEL_STACK_SIZE as u64;
        let kstack_bottom = kstack.as_ptr() as u64;

        // 2. Create isolated user address space
        let user_pml4 = create_user_address_space();
        allocated_frames.push(user_pml4);

        // 3. Map User Stack at 0x0000_7FFF_FFFF_0000
        let ustack_virt = VirtAddr::new(0x0000_7FFF_FFFF_0000 - PAGE_SIZE as u64);
        let ustack_frame = alloc_zeroed_frame().expect("Failed to allocate user stack frame");
        allocated_frames.push(ustack_frame);

        map_page(
            user_pml4,
            ustack_virt,
            ustack_frame,
            PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE,
        );

        let ustack_top_addr = 0x0000_7FFF_FFFF_0000u64;

        // 4. Map User Code Page at 0x0000_0000_0040_0000
        let user_entry_virt = VirtAddr::new(0x0000_0000_0040_0000);
        let code_frame = alloc_zeroed_frame().expect("Failed to allocate user code frame");
        allocated_frames.push(code_frame);

        // Copy bytecode into code frame via HHDM
        let code_dest = phys_to_virt(code_frame).as_mut_ptr::<u8>();
        let copy_len = bytecode.len().min(PAGE_SIZE);
        unsafe {
            core::ptr::copy_nonoverlapping(bytecode.as_ptr(), code_dest, copy_len);
        }

        map_page(
            user_pml4,
            user_entry_virt,
            code_frame,
            PageTableFlags::PRESENT | PageTableFlags::USER_ACCESSIBLE | PageTableFlags::WRITABLE,
        );

        let context = TaskContext::new_user_task(
            user_entry_virt.as_u64() as usize,
            ustack_top_addr,
            USER_CODE_SELECTOR,
            USER_DATA_SELECTOR,
        );

        let pcb = ProcessControlBlock {
            pid,
            name: name.to_string(),
            state: TaskState::Ready,
            priority: TaskPriority::Normal,
            is_user: true,
            pml4_root: user_pml4,
            kernel_stack_bottom: VirtAddr::new(kstack_bottom),
            kernel_stack_top: VirtAddr::new(kstack_top),
            user_stack_top: VirtAddr::new(ustack_top_addr),
            user_entry_point: user_entry_virt,
            allocated_frames,
            context,
            time_slice_remaining: DEFAULT_QUANTUM_TICKS,
            total_cpu_ticks: 0,
            window_id: None,
        };

        self.tasks.push(pcb);
        pid
    }

    /// Terminates a process by PID and queues it for Phase 2 deferred reclamation.
    pub fn kill_process(&mut self, pid: ProcessId) -> bool {
        if pid == 0 {
            return false; // PID 0 [idle] is immune to termination
        }

        if let Some(pos) = self.tasks.iter().position(|t| t.pid == pid) {
            self.tasks[pos].state = TaskState::Terminated(ExitReason::KilledByAdmin);
            if !self.zombie_queue.contains(&pid) {
                self.zombie_queue.push(pid);
            }
            true
        } else {
            false
        }
    }

    /// Performs Phase 2 deferred zombie resource reclamation.
    ///
    /// Frees user address spaces and physical memory frames on an independent kernel context.
    pub fn reap_zombies(&mut self) -> usize {
        let mut reaped = 0;
        let mut deferred = ZombieQueue::new();

        while let Some(pid) = self.zombie_queue.pop() {
            if let Some(pos) = self.tasks.iter().position(|t| t.pid == pid) {
                let is_user = self.tasks[pos].is_user;
                let user_pml4 = self.tasks[pos].pml4_root;
                let frames = self.tasks[pos].allocated_frames.clone();

                // Free user address space if isolated
                if is_user && user_pml4 != get_kernel_pml4() {
                    unsafe {
                        destroy_user_address_space(user_pml4);
                    }
                } else {
                    for frame in frames {
                        free_frame(frame);
                    }
                }

                self.tasks.remove(pos);
                if self.current_idx >= self.tasks.len() && !self.tasks.is_empty() {
                    self.current_idx = 0;
                }
                reaped += 1;
            } else {
                deferred.push(pid);
            }
        }

        // PIDs with no PCB yet stay queued for the next pass.
        self.zombie_queue = deferred;
        reaped
    }

    /// Primary 100Hz Round-Robin Preemptive Scheduler routine.
    ///
    /// Invoked directly on Timer IRQ 0 / Vector 32.
    pub fn schedule(&mut self, ctx: &mut InterruptContext) {
        if !self.is_initialized || self.tasks.is_empty() {
            return;
        }

        self.total_ticks += 1;

        // 1. Save currently running task's CPU registers
        if let Some(curr) = self.tasks.get_mut(self.current_idx) {
            save_context_from_interrupt(curr, ctx);

            if curr.state == TaskState::Running {
                curr.total_cpu_ticks += 1;
                if curr.pid == 0 {
                    self.idle_ticks += 1;
                }

                if curr.time_slice_remaining > 0 {
                    curr.time_slice_remaining -= 1;
                    return; // Continue running current quantum
                } else {
                    curr.state = TaskState::Ready;
                    curr.time_slice_remaining = DEFAULT_QUANTUM_TICKS;
                }
            } else if matches!(curr.state, TaskState::Terminated(_)) {
                if !self.zombie_queue.contains(&curr.pid) {
                    self.zombie_queue.push(curr.pid);
                }
            }
        }

        // 2. Perform Phase 2 deferred zombie reaping
        self.reap_zombies();

        // 3. Priority-aware Round-Robin selection of next ready task
        let n = self.tasks.len();
        let mut next_idx = (self.current_idx + 1) % n;
        let mut chosen_idx = None;

        for _ in 0..n {
            if self.tasks[next_idx].state == TaskState::Ready {
                chosen_idx = Some(next_idx);
                break;
            }
            next_idx = (next_idx + 1) % n;
        }

        let selected = chosen_idx.unwrap_or(0); // Fallback to PID 0 [idle]
        self.current_idx = selected;
        self.tasks[selected].state = TaskState::Running;
        self.tasks[selected].time_slice_remaining = DEFAULT_QUANTUM_TICKS;

        // 4. Restore new task's registers, switch CR3 and update TSS RSP0
        restore_context_to_interrupt(&self.tasks[selected], ctx);
    }

    /// Queries telemetry snapshot of all active processes.
    pub fn get_process_list(&self) -> Vec<ProcessInfo> {
        self.tasks
            .iter()
            .map(|t| ProcessInfo {
                pid: t.pid,
                name: t.name.clone(),
                state: t.state,
                priority: t.priority,
                memory_bytes: t.memory_usage_bytes(),
                cpu_percent: if self.total_ticks > 0 {
                    ((t.total_cpu_ticks * 100) / self.total_ticks) as u32
                } else {
                    0
                },
                is_user: t.is_user,
            })
            .collect()
    }

    /// Returns aggregate system CPU utilization (0..100%).
    pub fn get_cpu_usage(&self) -> u32 {
        if self.total_ticks == 0 {
            return 0;
        }
        let active_ticks = self.total_ticks.saturating_sub(self.idle_ticks);
        ((active_ticks * 100) / self.total_ticks) as u32
    }
}

pub static SCHEDULER: Mutex<Scheduler> = Mutex::new(Scheduler::new());

use crate::arch::InterruptGuard;
use crate::drivers::ring::EventRing;

/// Zombie PIDs awaiting Phase 2 reclamation. Bounded and preallocated because
/// both the timer ISR and the fault handler append to it.
pub type ZombieQueue = EventRing<ProcessId, 64>;

/// Initializes the preemptive multitasking scheduler subsystem.
pub fn init() {
    let _guard = InterruptGuard::acquire();
    let mut scheduler = SCHEDULER.lock();
    scheduler.init();
}

/// Spawns a new task in the kernel scheduler.
pub fn spawn_process(name: &str, entry: extern "C" fn(), is_user: bool) -> ProcessId {
    let _guard = InterruptGuard::acquire();
    let mut scheduler = SCHEDULER.lock();
    scheduler.spawn_process(name, entry, is_user)
}

/// Loads and spawns an ELF64 executable in Ring 3.
pub fn spawn_user_elf(name: &str, image: &[u8]) -> Result<ProcessId, ElfError> {
    let _guard = InterruptGuard::acquire();
    let mut scheduler = SCHEDULER.lock();
    scheduler.spawn_user_elf(name, image)
}

/// Spawns a user process executing raw bytecode in lower-half user space.
pub fn spawn_user_bytecode(name: &str, bytecode: &[u8]) -> ProcessId {
    let _guard = InterruptGuard::acquire();
    let mut scheduler = SCHEDULER.lock();
    scheduler.spawn_user_bytecode(name, bytecode)
}

/// Spawns an intentional fault test in isolated Ring 3 userspace to prove fault containment.
pub fn spawn_user_fault_test(fault_type: usize) -> ProcessId {
    match fault_type {
        0 => {
            // Null Pointer Dereference: mov dword ptr [0], 0xDEADBEEF; jmp $
            let code = [
                0xc7, 0x04, 0x25, 0x00, 0x00, 0x00, 0x00, 0xef, 0xbe, 0xad, 0xde,
                0xeb, 0xfe,
            ];
            spawn_user_bytecode("crash_null_ptr", &code)
        }
        1 => {
            // Divide by Zero: mov eax, 100; xor ecx, ecx; div ecx; jmp $
            let code = [
                0xb8, 0x64, 0x00, 0x00, 0x00,
                0x31, 0xc9,
                0xf7, 0xf1,
                0xeb, 0xfe,
            ];
            spawn_user_bytecode("crash_div_zero", &code)
        }
        2 => {
            // Out-of-bounds Supervisor Write: mov rax, 0xffffffff80000000; mov dword ptr [rax], 0x1337; jmp $
            let code = [
                0x48, 0xb8, 0x00, 0x00, 0x00, 0x80, 0xff, 0xff, 0xff, 0xff,
                0xc7, 0x00, 0x37, 0x13, 0x00, 0x00,
                0xeb, 0xfe,
            ];
            spawn_user_bytecode("crash_oob_write", &code)
        }
        3 => {
            // Invalid Opcode: ud2; jmp $
            let code = [
                0x0f, 0x0b,
                0xeb, 0xfe,
            ];
            spawn_user_bytecode("crash_invalid_op", &code)
        }
        _ => {
            // Calculation loop: xor rax, rax; inc rax; pause; jmp -8
            let code = [
                0x48, 0x31, 0xc0,
                0x48, 0xff, 0xc0,
                0xf3, 0x90,
                0xeb, 0xf7,
            ];
            spawn_user_bytecode("user_calc_worker", &code)
        }
    }
}

/// Kills an active process by PID.
pub fn kill_process(pid: ProcessId) -> bool {
    let _guard = InterruptGuard::acquire();
    let mut scheduler = SCHEDULER.lock();
    scheduler.kill_process(pid)
}

/// Returns telemetry process list.
pub fn get_process_list() -> Vec<ProcessInfo> {
    let _guard = InterruptGuard::acquire();
    let scheduler = SCHEDULER.lock();
    scheduler.get_process_list()
}

/// Returns real-time CPU utilization percentage.
pub fn get_cpu_usage() -> u32 {
    let _guard = InterruptGuard::acquire();
    let scheduler = SCHEDULER.lock();
    scheduler.get_cpu_usage()
}

/// Returns memory usage statistics (used_bytes, total_bytes).
pub fn get_memory_stats() -> (u64, u64) {
    crate::memory::get_memory_stats()
}

/// Returns PID of currently executing task.
pub fn current_pid() -> ProcessId {
    let _guard = InterruptGuard::acquire();
    let sched = SCHEDULER.lock();
    sched.tasks.get(sched.current_idx).map(|t| t.pid).unwrap_or(0)
}

/// Reaps terminated zombie processes.
pub fn reap_zombies() -> usize {
    let _guard = InterruptGuard::acquire();
    let mut scheduler = SCHEDULER.lock();
    scheduler.reap_zombies()
}

/// Total timer ticks since boot. At `TIMER_HZ` this is the system uptime.
pub fn get_uptime_ticks() -> u64 {
    let _guard = InterruptGuard::acquire();
    let scheduler = SCHEDULER.lock();
    scheduler.total_ticks
}

/// Hardware Timer IRQ handler callback registered to IDT vector 32.
pub fn on_timer_tick(_irq: u8, ctx: &mut InterruptContext) {
    // Already in an ISR with interrupts masked, so no guard is needed here. Every
    // task-context path that takes this lock must hold an `InterruptGuard`, or a
    // tick landing mid-critical-section would deadlock against it.
    SCHEDULER.lock().schedule(ctx);
}

/// System idle loop executed when no application tasks are ready.
pub extern "C" fn idle_task_entry() -> ! {
    loop {
        // Asynchronously clean up any terminated zombie tasks
        reap_zombies();

        // Low-power halt state until next hardware interrupt
        unsafe {
            core::arch::asm!("sti; hlt", options(nomem, nostack));
        }
    }
}
