//! Tier 4: Real-World Application Scenarios E2E Test Suite for AegisOS
//!
//! Realistic multi-step end-to-end user workflows:
//! - Scenario 1: Desktop Multitasking & Crash Resilience Full Lifecycle (5 apps, crash isolation, recovery)
//! - Scenario 2: Concurrency Stress & Fault Recovery (10 tasks, simultaneous exceptions, zero panic)
//! - Scenario 3: Interactive Terminal Shell & Process Lifecycle Management (CLI shell, history, ps/kill/free/run)
//! - Scenario 4: GUI Compositor, Windowing & Visual Fidelity (dragging, traffic lights, dock, alpha blending)
//! - Scenario 5: System Specs & Memory Footprint Budget Verification (<60MB RAM constraint, leak checks)

use aegis_e2e::test_harness::*;

#[test]
fn test_tier4_scenario_01_desktop_multitasking_and_crash_resilience_full_lifecycle() {
    // Step 1: Boot AegisOS kernel
    let mut env = AegisOsKernelEnv::new();
    assert!(env.kernel_booted);
    assert_eq!(env.scheduler.get_process_list().len(), 2); // [idle] + kernel_desktop

    let initial_mem = env.get_memory_stats();
    assert!(initial_mem.used_bytes < MAX_IDLE_RAM_BYTES, "Idle memory must be < 60 MB");

    // Step 2: Launch all 5 system applications from Dock
    let (crash_wid, crash_pid) = env.launch_app(AppId::CrashTest);
    let (mon_wid, mon_pid) = env.launch_app(AppId::ActivityMonitor);
    let (term_wid, term_pid) = env.launch_app(AppId::Terminal);
    let (pad_wid, pad_pid) = env.launch_app(AppId::AegisPad);
    let (about_wid, about_pid) = env.launch_app(AppId::AboutDialog);

    // Step 3: Verify all 5 windows are open and layered in Z-order
    assert_eq!(env.wm.windows.len(), 5);
    assert_eq!(env.scheduler.get_process_list().len(), 7); // [idle] + desktop + 5 apps

    // Step 4: Cycle focus through each window
    for wid in [crash_wid, mon_wid, term_wid, pad_wid, about_wid] {
        env.wm.focus_window(wid);
        assert_eq!(env.wm.focused_window().unwrap().id, wid);
    }

    // Step 5: Focus Crash-Test App and trigger intentional Null Pointer Dereference
    env.wm.focus_window(crash_wid);

    let fault_res = env.trigger_user_fault(crash_pid, ExceptionVector::PageFault, 0x4012A0, 0x0);
    assert!(fault_res.is_ok(), "Null pointer crash must be isolated to Crash-Test process");

    // Step 6: Verify Crash-Test window is cleanly dismissed
    assert!(env.wm.windows.iter().all(|w| w.id != crash_wid));
    assert_eq!(env.wm.windows.len(), 4);

    // Step 7: Verify serial console contains fault logs
    assert!(env.uart.contains_log("[FAULT] Ring 3 Exception PageFault in PID"));
    assert!(env.uart.contains_log("[KERNEL] Terminating faulting task PID"));

    // Step 8: Step timer ticks for deferred zombie reclamation
    env.timer_tick();
    assert!(env.scheduler.get_process_list().iter().all(|p| p.pid != crash_pid));

    // Step 9: Verify remaining 4 apps are alive and operational
    assert!(env.activity_monitor_app.is_some());
    assert!(env.terminal_app.is_some());
    assert!(env.pad_app.is_some());
    assert!(env.about_app.is_some());

    // Step 10: Type text in AegisPad and execute commands in Terminal
    let pad = env.pad_app.as_mut().unwrap();
    pad.handle_key(b'O');
    pad.handle_key(b'S');

    let term = env.terminal_app.as_mut().unwrap();
    let ps_out = term.execute_command("ps", &mut env.scheduler, &env.frame_alloc);
    assert!(ps_out.len() >= 5);

    // Render full desktop frame
    let pixels = env.render_desktop();
    assert!(pixels > 0);
}

#[test]
fn test_tier4_scenario_02_stress_and_concurrent_fault_recovery_workflow() {
    let mut env = AegisOsKernelEnv::new();
    env.launch_app(AppId::Terminal);
    env.launch_app(AppId::ActivityMonitor);

    // Spawn 10 worker tasks
    let mut worker_pids = Vec::new();
    for i in 0..10 {
        let pid = env.scheduler.spawn_process(
            &format!("worker_{}", i),
            true,
            Priority::Normal,
            PhysAddr(0x1000 + i as u64 * 0x1000),
            vec![PhysAddr(0x1000 + i as u64 * 0x1000)],
        );
        worker_pids.push(pid);
    }
    assert_eq!(env.scheduler.get_process_list().len(), 14); // [idle], desktop, term, mon, 10 workers

    // Trigger multiple faults concurrently across worker tasks
    env.trigger_user_fault(worker_pids[0], ExceptionVector::DivideByZero, 0x401000, 0).unwrap();
    env.trigger_user_fault(worker_pids[1], ExceptionVector::PageFault, 0x402000, 0x0).unwrap();
    env.trigger_user_fault(worker_pids[2], ExceptionVector::InvalidOpcode, 0x403000, 0).unwrap();
    env.trigger_user_fault(worker_pids[3], ExceptionVector::GeneralProtectionFault, 0x404000, 0).unwrap();

    // Deferred reclamation
    env.timer_tick();

    // Verify 4 crashed workers are reaped, remaining 6 workers and UI apps continue
    let remaining_procs = env.scheduler.get_process_list();
    assert_eq!(remaining_procs.len(), 10);
    assert!(remaining_procs.iter().any(|p| p.name == "terminal_shell"));
    assert!(remaining_procs.iter().any(|p| p.name == "activity_monitor"));

    // Verify UI rendering remains active
    assert!(env.render_desktop() > 0);
}

#[test]
fn test_tier4_scenario_03_interactive_shell_and_process_management_workflow() {
    let mut env = AegisOsKernelEnv::new();
    env.launch_app(AppId::Terminal);

    let term = env.terminal_app.as_mut().unwrap();

    // 1. Verify prompt
    assert_eq!(term.prompt, "aegis:~$ ");

    // 2. Run 'help'
    let help_out = term.execute_command("help", &mut env.scheduler, &env.frame_alloc);
    assert!(help_out.iter().any(|line| line.contains("help")));
    assert!(help_out.iter().any(|line| line.contains("ps")));
    assert!(help_out.iter().any(|line| line.contains("free")));

    // 3. Run 'echo'
    let echo_out = term.execute_command("echo AegisOS Ring 3 isolation verified", &mut env.scheduler, &env.frame_alloc);
    assert_eq!(echo_out, vec!["AegisOS Ring 3 isolation verified"]);

    // 4. Run 'run crashtest'
    let run_out = term.execute_command("run crashtest", &mut env.scheduler, &env.frame_alloc);
    assert!(run_out[0].contains("Spawned process 'crashtest'"));

    let spawned_pid = env.scheduler.get_process_list().last().unwrap().pid;

    // 5. Run 'kill <pid>'
    let kill_cmd = format!("kill {}", spawned_pid);
    let kill_out = term.execute_command(&kill_cmd, &mut env.scheduler, &env.frame_alloc);
    assert!(kill_out[0].contains("Terminated process PID"));

    // 6. Test History navigation
    term.handle_key_input(0x80, &mut env.scheduler, &env.frame_alloc); // Up arrow
    assert_eq!(term.command_buffer, kill_cmd);

    // 7. Clear terminal
    let clear_out = term.execute_command("clear", &mut env.scheduler, &env.frame_alloc);
    assert!(clear_out.is_empty());
    assert!(term.output_lines.is_empty());
}

#[test]
fn test_tier4_scenario_04_gui_compositor_windowing_and_visual_fidelity_workflow() {
    let mut env = AegisOsKernelEnv::new();
    let (term_wid, _) = env.launch_app(AppId::Terminal);
    let (pad_wid, _) = env.launch_app(AppId::AegisPad);

    // 1. Drag Terminal window
    env.wm.handle_mouse_down(210, 160); // Click titlebar
    env.wm.handle_mouse_move(330, 250);
    env.wm.handle_mouse_up();

    let term_win = env.wm.windows.iter().find(|w| w.id == term_wid).unwrap();
    assert_eq!(term_win.x, 270);
    assert_eq!(term_win.y, 240);

    // 2. Minimize Terminal window via yellow button (x: 270 + 32 = 302, y: 240 + 12 = 252)
    env.wm.handle_mouse_down(302, 252);
    let term_win_after_min = env.wm.windows.iter().find(|w| w.id == term_wid).unwrap();
    assert!(term_win_after_min.is_minimized);

    // 3. Maximize AegisPad window via green button (x: 250 + 48 = 298, y: 80 + 12 = 92)
    env.wm.handle_mouse_down(298, 92);
    let pad_win = env.wm.windows.iter().find(|w| w.id == pad_wid).unwrap();
    assert!(pad_win.is_maximized);
    assert_eq!(pad_win.width, 1024);

    // 4. Restore AegisPad
    env.wm.handle_mouse_down(48, TOP_BAR_HEIGHT as i32 + 12);
    let pad_restored = env.wm.windows.iter().find(|w| w.id == pad_wid).unwrap();
    assert!(!pad_restored.is_maximized);

    // 5. Render double-buffered desktop frame
    let pixels = env.render_desktop();
    assert!(pixels > 0);
}

#[test]
fn test_tier4_scenario_05_memory_budget_and_system_specs_validation_workflow() {
    let mut env = AegisOsKernelEnv::new();
    env.launch_app(AppId::AboutDialog);

    // 1. Verify About Dialog specs
    let about = env.about_app.as_ref().unwrap();
    assert_eq!(about.kernel_version, "AegisOS 1.0.0 (Rust no_std)");
    assert_eq!(about.bootloader, "Limine Boot Protocol v2");
    assert_eq!(about.architecture, "x86_64 Long Mode (Ring 0 / Ring 3)");
    assert!(about.memory_footprint_str.contains("< 60MB"));

    // 2. Verify memory budget < 60 MB RAM
    let stats = env.get_memory_stats();
    assert!(stats.used_bytes < MAX_IDLE_RAM_BYTES, "Used RAM must be < 60 MB");
    assert_eq!(stats.total_bytes, TOTAL_RAM_4GB);

    // 3. Render 1,000 frames without memory leak
    let initial_allocated_frames = env.frame_alloc.allocated_count();
    for _ in 0..1000 {
        env.render_desktop();
        env.timer_tick();
    }
    let final_allocated_frames = env.frame_alloc.allocated_count();
    assert_eq!(final_allocated_frames, initial_allocated_frames, "Compositor rendering loop must have zero memory leaks");
}
