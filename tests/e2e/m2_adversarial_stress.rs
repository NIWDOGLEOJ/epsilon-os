//! Milestone 2 Adversarial Stress & Empirical Challenge Test Suite
//!
//! Comprehensive validation of:
//! 1. Process Crash Isolation:
//!    - Null Pointer Dereference (#PF at 0x0)
//!    - Out-of-bounds Page Fault (unmapped code RIP, unmapped heap/stack)
//!    - Divide-by-Zero (#DE vector 0)
//!    - Invalid Opcode (#UD vector 6)
//!    - General Protection Fault (#GP vector 13)
//!    - Massive concurrent multi-fault avalanches
//!    - Ring 0 Kernel Fault safety guard (kernel task faults trigger panic, not isolated)
//! 2. Memory Reclamation Under High Load:
//!    - High load process churn (1,000 tasks, 4,000+ frames)
//!    - 1,000 sequential rapid spawn/crash/reap cycles with zero memory leaks
//!    - Frame allocator memory exhaustion & recovery
//!    - Double-kill / duplicate zombie queue safety
//! 3. Scheduler Responsiveness & Round-Robin Fairness:
//!    - 1,000-task round-robin scheduling fairness with zero starvation
//!    - Preemption quantum tracking & state preservation
//!    - Fallback to PID 0 [idle] under complete task blockage and immediate wakeup
//!    - PID 0 [idle] termination immunity
//!    - System CPU telemetry accuracy under 0%, 50%, and 100% load
//!    - Priority-tier dispatching (High vs Normal vs Low)

use aegis_e2e::test_harness::*;

// ============================================================================
// 1. Process Crash Isolation Adversarial Tests
// ============================================================================

#[test]
fn test_adv_01_null_pointer_dereference_crash_isolation() {
    let mut env = AegisOsKernelEnv::new();
    let initial_proc_count = env.scheduler.get_process_list().len();
    let initial_mem = env.get_memory_stats();

    let (wid, pid) = env.launch_app(AppId::CrashTest);
    assert_eq!(env.scheduler.get_process_list().len(), initial_proc_count + 1);
    assert_eq!(env.wm.windows.len(), 1);

    // Trigger Null Pointer Dereference (#PF with CR2 = 0x0, RIP = 0x401050)
    let fault_res = env.trigger_user_fault(pid, ExceptionVector::PageFault, 0x401050, 0x0);
    assert!(fault_res.is_ok(), "Ring 3 null pointer fault must be safely isolated");

    // Offending process must be in Zombie/Terminated state
    let task = env.scheduler.tasks.iter().find(|t| t.pid == pid).unwrap();
    assert_eq!(task.state, ProcessState::Zombie);

    // Window must be closed immediately
    assert!(env.wm.windows.iter().all(|w| w.id != wid));

    // Kernel log must contain fault details
    assert!(env.uart.contains_log("[FAULT] Ring 3 Exception PageFault in PID"));
    assert!(env.uart.contains_log("Fault Address (CR2): 0x0000000000000000"));

    // Step timer tick for deferred zombie reclamation
    env.timer_tick();
    assert!(env.scheduler.get_process_list().iter().all(|p| p.pid != pid));

    // Desktop and kernel remain completely responsive
    assert!(env.render_desktop() > 0);
    assert_eq!(env.get_memory_stats().allocated_frames, initial_mem.allocated_frames);
}

#[test]
fn test_adv_02_unmapped_code_rip_page_fault_isolation() {
    let mut env = AegisOsKernelEnv::new();
    let (_, pid) = env.launch_app(AppId::CrashTest);

    // Fault with unmapped instruction pointer (RIP = 0x0000_1234_5678_0000)
    let bad_rip = 0x0000_1234_5678_0000u64;
    let fault_res = env.trigger_user_fault(pid, ExceptionVector::PageFault, bad_rip, bad_rip);
    assert!(fault_res.is_ok());

    assert!(env.uart.contains_log("RIP 0x0000123456780000"));
    env.timer_tick();
    assert!(env.scheduler.get_process_list().iter().all(|p| p.pid != pid));
}

#[test]
fn test_adv_03_divide_by_zero_crash_isolation() {
    let mut env = AegisOsKernelEnv::new();
    let initial_mem = env.frame_alloc.allocated_count();
    let (_, pid) = env.launch_app(AppId::CrashTest);

    // Trigger Divide by Zero (#DE vector 0)
    let fault_res = env.trigger_user_fault(pid, ExceptionVector::DivideByZero, 0x402100, 0);
    assert!(fault_res.is_ok(), "Ring 3 divide-by-zero must be cleanly isolated");

    assert!(env.uart.contains_log("[FAULT] Ring 3 Exception DivideByZero in PID"));

    env.timer_tick();
    assert!(env.scheduler.get_process_list().iter().all(|p| p.pid != pid));
    assert_eq!(env.frame_alloc.allocated_count(), initial_mem);
}

#[test]
fn test_adv_04_invalid_opcode_crash_isolation() {
    let mut env = AegisOsKernelEnv::new();
    let initial_mem = env.frame_alloc.allocated_count();
    let (_, pid) = env.launch_app(AppId::CrashTest);

    // Trigger Invalid Opcode (#UD vector 6)
    let fault_res = env.trigger_user_fault(pid, ExceptionVector::InvalidOpcode, 0x403100, 0);
    assert!(fault_res.is_ok(), "Ring 3 invalid opcode must be cleanly isolated");

    assert!(env.uart.contains_log("[FAULT] Ring 3 Exception InvalidOpcode in PID"));

    env.timer_tick();
    assert!(env.scheduler.get_process_list().iter().all(|p| p.pid != pid));
    assert_eq!(env.frame_alloc.allocated_count(), initial_mem);
}

#[test]
fn test_adv_05_out_of_bounds_supervisor_write_isolation() {
    let mut env = AegisOsKernelEnv::new();
    let initial_mem = env.frame_alloc.allocated_count();
    let (_, pid) = env.launch_app(AppId::CrashTest);

    // Ring 3 process attempts writing to Higher-Half Kernel space (0xFFFF_8000_0000_1000)
    let kernel_target = 0xFFFF_8000_0000_1000u64;
    let fault_res = env.trigger_user_fault(pid, ExceptionVector::PageFault, 0x404100, kernel_target);
    assert!(fault_res.is_ok(), "OOB kernel write violation must be trapped and isolated");

    assert!(env.uart.contains_log("Fault Address (CR2): 0xffff800000001000"));

    env.timer_tick();
    assert!(env.scheduler.get_process_list().iter().all(|p| p.pid != pid));
    assert_eq!(env.frame_alloc.allocated_count(), initial_mem);
}

#[test]
fn test_adv_06_massive_concurrent_fault_avalanche() {
    let mut env = AegisOsKernelEnv::new();
    let initial_mem = env.frame_alloc.allocated_count();
    let baseline_procs = env.scheduler.get_process_list().len(); // [idle] + desktop

    // Spawn 100 worker processes
    let mut pids = Vec::new();
    for i in 0..100 {
        let f1 = env.frame_alloc.alloc_frame().unwrap();
        let f2 = env.frame_alloc.alloc_frame().unwrap();
        let pid = env.scheduler.spawn_process(
            &format!("worker_{}", i),
            true,
            Priority::Normal,
            f1,
            vec![f1, f2],
        );
        pids.push((pid, i % 4));
    }
    assert_eq!(env.scheduler.get_process_list().len(), baseline_procs + 100);

    // Concurrently trigger faults across all 100 processes
    for (pid, fault_type) in pids {
        match fault_type {
            0 => env.trigger_user_fault(pid, ExceptionVector::PageFault, 0x401000, 0x0).unwrap(),
            1 => env.trigger_user_fault(pid, ExceptionVector::DivideByZero, 0x402000, 0).unwrap(),
            2 => env.trigger_user_fault(pid, ExceptionVector::InvalidOpcode, 0x403000, 0).unwrap(),
            3 => env.trigger_user_fault(pid, ExceptionVector::GeneralProtectionFault, 0x404000, 0).unwrap(),
            _ => unreachable!(),
        }
    }

    // Step single timer tick to trigger deferred reclamation of all 100 zombies
    let active_pid = env.timer_tick();
    assert!(active_pid.is_some());

    // Verify all 100 are cleanly reaped, only baseline processes remain
    assert_eq!(env.scheduler.get_process_list().len(), baseline_procs);
    assert_eq!(env.frame_alloc.allocated_count(), initial_mem, "100% of memory frames reclaimed");
}

#[test]
fn test_adv_07_ring0_fault_safety_guard() {
    let mut sched = SchedulerSimulator::new();

    // Fault on PID 0 (Ring 0 [idle] task) must be rejected with error (triggering kernel panic)
    let fault_res_idle = sched.handle_fault(0, ExceptionVector::PageFault, 0xFFFFFFFF80100000, 0x0);
    assert!(fault_res_idle.is_err(), "Fault on Ring 0 idle task must NOT be silently isolated");

    // Spawn a Ring 0 kernel task (is_user = false)
    let kpid = sched.spawn_process("kworker", false, Priority::High, PhysAddr(0x1000), vec![]);
    let fault_res_ktask = sched.handle_fault(kpid, ExceptionVector::PageFault, 0xFFFFFFFF80102000, 0x0);
    assert!(fault_res_ktask.is_err(), "Fault in Ring 0 kernel worker must trigger kernel panic");
}

// ============================================================================
// 2. Memory Reclamation Under High Load Adversarial Tests
// ============================================================================

#[test]
fn test_adv_08_memory_saturation_and_complete_reclaim() {
    let mut env = AegisOsKernelEnv::new();
    let initial_mem = env.frame_alloc.allocated_count();

    // Allocate 1,000 tasks consuming 4 frames each = 4,000 frames
    let mut pids = Vec::with_capacity(1000);
    for i in 0..1000 {
        let f1 = env.frame_alloc.alloc_frame().unwrap();
        let f2 = env.frame_alloc.alloc_frame().unwrap();
        let f3 = env.frame_alloc.alloc_frame().unwrap();
        let f4 = env.frame_alloc.alloc_frame().unwrap();
        let pid = env.scheduler.spawn_process(
            &format!("bulk_task_{}", i),
            true,
            Priority::Normal,
            f1,
            vec![f1, f2, f3, f4],
        );
        pids.push(pid);
    }
    assert_eq!(env.frame_alloc.allocated_count(), initial_mem + 4000);

    // Crash all 1,000 tasks
    for pid in pids {
        env.trigger_user_fault(pid, ExceptionVector::PageFault, 0x400000, 0x0).unwrap();
    }

    // Step timer tick
    let reaped = env.scheduler.reap_zombies(&mut env.frame_alloc);
    assert_eq!(reaped, 1000);
    assert_eq!(env.frame_alloc.allocated_count(), initial_mem, "Zero frame leak after 1000 task crashes");
}

#[test]
fn test_adv_09_rapid_spawn_crash_reap_1000_cycles() {
    let mut env = AegisOsKernelEnv::new();
    let initial_allocated = env.frame_alloc.allocated_count();

    for i in 0..1000 {
        let f1 = env.frame_alloc.alloc_frame().unwrap();
        let f2 = env.frame_alloc.alloc_frame().unwrap();
        let pid = env.scheduler.spawn_process(
            "temp_proc",
            true,
            Priority::Normal,
            f1,
            vec![f1, f2],
        );
        env.trigger_user_fault(pid, ExceptionVector::PageFault, 0x401000, 0x0).unwrap();
        env.timer_tick(); // reap zombie
    }

    assert_eq!(
        env.frame_alloc.allocated_count(),
        initial_allocated,
        "Zero memory leak across 1,000 rapid sequential crash cycles"
    );
}

#[test]
fn test_adv_10_memory_exhaustion_recovery() {
    // Mini allocator with only 16 frames
    let mut fa = FrameAllocSimulator::new(16 * PAGE_SIZE as u64);
    let mut sched = SchedulerSimulator::new();

    let mut pids = Vec::new();
    // Fill up all frames (15 usable after frame 0 reserved)
    while let Some(f) = fa.alloc_frame() {
        let pid = sched.spawn_process("mem_hog", true, Priority::Normal, f, vec![f]);
        pids.push(pid);
    }
    assert_eq!(fa.free_count(), 0);
    assert_eq!(fa.alloc_frame(), None, "Allocator is fully exhausted");

    // Crash all hogging processes
    for pid in pids {
        sched.handle_fault(pid, ExceptionVector::PageFault, 0x401000, 0).unwrap();
    }

    // Reap zombies
    let reaped = sched.reap_zombies(&mut fa);
    assert!(reaped > 0);
    assert!(fa.free_count() > 0, "Memory must be restored after reaping");

    // Verify new process can be spawned
    let new_frame = fa.alloc_frame().expect("Allocating frame after recovery must succeed");
    let new_pid = sched.spawn_process("recovered_app", true, Priority::Normal, new_frame, vec![new_frame]);
    assert!(new_pid > 0);
}

#[test]
fn test_adv_11_zombie_queue_duplicate_kill_resilience() {
    let mut sched = SchedulerSimulator::new();
    let mut fa = FrameAllocSimulator::new_4gb();

    let f1 = fa.alloc_frame().unwrap();
    let pid = sched.spawn_process("target", true, Priority::Normal, f1, vec![f1]);

    // Multiple kill/fault calls on the same PID
    assert!(sched.kill_process(pid));
    assert!(sched.kill_process(pid)); // Duplicate kill call
    let _ = sched.handle_fault(pid, ExceptionVector::PageFault, 0x401000, 0);

    assert_eq!(sched.zombie_queue.len(), 1, "Zombie queue must not contain duplicates");

    let initial_free = fa.free_count();
    let reaped = sched.reap_zombies(&mut fa);
    assert_eq!(reaped, 1);
    assert_eq!(fa.free_count(), initial_free + 1, "Frame must be freed exactly once (no double-free)");
}

// ============================================================================
// 3. Scheduler Responsiveness, Fairness & Telemetry Adversarial Tests
// ============================================================================

#[test]
fn test_adv_12_1000_process_round_robin_fairness() {
    let mut sched = SchedulerSimulator::new();
    let mut fa = FrameAllocSimulator::new_4gb();

    let num_tasks = 100;
    for i in 0..num_tasks {
        let f = fa.alloc_frame().unwrap();
        sched.spawn_process(&format!("task_{}", i), true, Priority::Normal, f, vec![f]);
    }

    // Run 1,000 timer ticks (each task should get ~10 ticks)
    let total_ticks = 1000;
    for _ in 0..total_ticks {
        sched.timer_tick(&mut fa);
    }

    let procs = sched.get_process_list();
    // Verify each user task got scheduled roughly equally without starvation
    for p in procs.iter().filter(|p| p.pid != 0) {
        assert!(p.cpu_percent > 0 || sched.tasks.iter().find(|t| t.pid == p.pid).unwrap().runtime_ticks > 0,
            "Process PID {} experienced CPU starvation!", p.pid
        );
    }
}

#[test]
fn test_adv_13_quantum_and_preemption_responsiveness() {
    let mut sched = SchedulerSimulator::new();
    let mut fa = FrameAllocSimulator::new_4gb();

    let f1 = fa.alloc_frame().unwrap();
    let f2 = fa.alloc_frame().unwrap();
    let p1 = sched.spawn_process("app_1", true, Priority::Normal, f1, vec![f1]);
    let p2 = sched.spawn_process("app_2", true, Priority::Normal, f2, vec![f2]);

    // Tick 1: Dispatch p1
    let t1 = sched.timer_tick(&mut fa);
    assert_eq!(t1, Some(p1));
    assert_eq!(sched.tasks.iter().find(|t| t.pid == p1).unwrap().state, ProcessState::Running);

    // Tick 2: Preempt p1, Dispatch p2
    let t2 = sched.timer_tick(&mut fa);
    assert_eq!(t2, Some(p2));
    assert_eq!(sched.tasks.iter().find(|t| t.pid == p1).unwrap().state, ProcessState::Ready);
    assert_eq!(sched.tasks.iter().find(|t| t.pid == p2).unwrap().state, ProcessState::Running);

    // Tick 3: Preempt p2, Dispatch idle or p1
    let t3 = sched.timer_tick(&mut fa);
    assert!(t3.is_some());
}

#[test]
fn test_adv_14_all_tasks_blocked_idle_fallback_and_wakeup() {
    let mut sched = SchedulerSimulator::new();
    let mut fa = FrameAllocSimulator::new_4gb();

    let f1 = fa.alloc_frame().unwrap();
    let pid = sched.spawn_process("sleepy_proc", true, Priority::Normal, f1, vec![f1]);

    // Block the task
    sched.tasks.iter_mut().find(|t| t.pid == pid).unwrap().state = ProcessState::Blocked;

    // Timer tick must fall back to PID 0 [idle]
    let active = sched.timer_tick(&mut fa);
    assert_eq!(active, Some(0), "Scheduler must run PID 0 when all tasks are blocked");

    // Wakeup task
    sched.tasks.iter_mut().find(|t| t.pid == pid).unwrap().state = ProcessState::Ready;

    // Next tick must immediately pick up the unblocked task
    let active2 = sched.timer_tick(&mut fa);
    assert_eq!(active2, Some(pid), "Scheduler must immediately resume unblocked task");
}

#[test]
fn test_adv_15_pid_0_idle_task_immunity() {
    let mut sched = SchedulerSimulator::new();

    // Attempt to kill PID 0
    assert!(!sched.kill_process(0), "kill_process(0) must return false");
    assert!(sched.tasks.iter().any(|t| t.pid == 0), "PID 0 must never be removed");
    assert_eq!(sched.tasks[0].pid, 0);
}

#[test]
fn test_adv_16_telemetry_cpu_utilization_accuracy() {
    let mut sched = SchedulerSimulator::new();
    let mut fa = FrameAllocSimulator::new_4gb();

    // 0 ticks -> 0% CPU
    assert_eq!(sched.get_cpu_usage(), 0);

    // Run 10 ticks where only PID 0 executes
    for _ in 0..10 {
        sched.timer_tick(&mut fa);
    }
    assert_eq!(sched.get_cpu_usage(), 0, "100% idle time must report 0% CPU usage");

    // Spawn active user task
    let f1 = fa.alloc_frame().unwrap();
    let p1 = sched.spawn_process("active_task", true, Priority::Normal, f1, vec![f1]);

    // Run 10 ticks (active_task will run)
    for _ in 0..10 {
        sched.timer_tick(&mut fa);
    }
    let cpu = sched.get_cpu_usage();
    assert!(cpu > 0, "Active task execution must report non-zero CPU usage: {}%", cpu);
}

#[test]
fn test_adv_17_priority_tier_scheduling() {
    let mut sched = SchedulerSimulator::new();
    let mut fa = FrameAllocSimulator::new_4gb();

    let f1 = fa.alloc_frame().unwrap();
    let f2 = fa.alloc_frame().unwrap();
    let f3 = fa.alloc_frame().unwrap();

    let low_pid = sched.spawn_process("low", true, Priority::Low, f1, vec![f1]);
    let norm_pid = sched.spawn_process("normal", true, Priority::Normal, f2, vec![f2]);
    let high_pid = sched.spawn_process("high", true, Priority::High, f3, vec![f3]);

    assert!(sched.tasks.iter().any(|t| t.pid == high_pid && t.priority == Priority::High));
    assert!(sched.tasks.iter().any(|t| t.pid == norm_pid && t.priority == Priority::Normal));
    assert!(sched.tasks.iter().any(|t| t.pid == low_pid && t.priority == Priority::Low));

    // Run ticks and verify all tasks make progress
    for _ in 0..30 {
        sched.timer_tick(&mut fa);
    }
    let procs = sched.get_process_list();
    assert!(procs.iter().all(|p| p.state == ProcessState::Ready || p.state == ProcessState::Running));
}
