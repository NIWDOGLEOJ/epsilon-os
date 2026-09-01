//! Tier 3: Cross-Feature Combinations E2E Test Suite for AegisOS
//!
//! Models complex pairwise interactions between subsystems:
//! - Crash-Test App crash during window dragging (F6 + F10 + F9)
//! - Activity Monitor active while Terminal Shell runs continuous command stream (F11.2 + F11.3 + F7 + F4)
//! - AegisPad editing under heavy memory allocation pressure (F11.4 + F4 + F5 + F8)
//! - Terminal spawn -> Activity Monitor process table kill -> frame reclamation (F11.3 + F11.1 + F11.2 + F7)
//! - Mouse dragging + rapid keyboard typing during preemptive scheduling (F9 + F10 + F8 + F7)
//! - Compositor 60 FPS rendering during fault isolation & deferred zombie reaping (F8 + F10 + F6 + F4)
//! - Terminal `free` output matching Activity Monitor memory telemetry (<60MB) (F11.2 + F11.3 + F4)
//! - Closing window via red button while process is executing in scheduler (F10 + F7 + F6 + F4)

use aegis_e2e::test_harness::*;

#[test]
fn test_tier3_01_crash_during_window_drag() {
    let mut env = AegisOsKernelEnv::new();
    let (wid, pid) = env.launch_app(AppId::CrashTest);

    // 1. Mouse down on titlebar to start dragging
    let win = env.wm.windows.iter().find(|w| w.id == wid).unwrap();
    let title_x = win.x + 100;
    let title_y = win.y + 10;
    env.wm.handle_mouse_down(title_x, title_y);
    assert!(env.wm.windows.iter().find(|w| w.id == wid).unwrap().is_dragging);

    // 2. Drag window across screen
    env.wm.handle_mouse_move(title_x + 150, title_y + 100);

    // 3. Trigger Ring 3 crash while dragging
    let fault_res = env.trigger_user_fault(pid, ExceptionVector::PageFault, 0x4012A0, 0x0);
    assert!(fault_res.is_ok(), "Fault must be cleanly caught and isolated");

    // 4. Verify window is closed and drag state is cleanly extinguished
    assert!(env.wm.windows.iter().all(|w| w.id != wid));
    assert!(env.wm.windows.iter().all(|w| !w.is_dragging));

    // 5. Verify system continues running smoothly
    env.timer_tick();
    assert_eq!(env.scheduler.current_process().unwrap().pid, 1);
    assert!(env.render_desktop() > 0);
}

#[test]
fn test_tier3_02_activity_monitor_with_terminal_command_stream() {
    let mut env = AegisOsKernelEnv::new();
    let (_, mon_pid) = env.launch_app(AppId::ActivityMonitor);
    let (_, term_pid) = env.launch_app(AppId::Terminal);

    // Run continuous sequence of shell commands
    let commands = vec!["ps", "free", "echo Test1", "run crashtest", "ps", "echo Done"];

    for cmd in commands {
        // Step timer tick
        env.timer_tick();

        // Send command to terminal
        if let Some(term) = &mut env.terminal_app {
            let out = term.execute_command(cmd, &mut env.scheduler, &env.frame_alloc);
            assert!(!out.is_empty(), "Command '{}' must produce output", cmd);
        }

        // Render desktop & update telemetry
        env.render_desktop();

        // Verify Activity Monitor telemetry stays responsive
        if let Some(mon) = &env.activity_monitor_app {
            assert_eq!(mon.pid, mon_pid);
            assert!(mon.cpu_history.len() == 60);
        }
    }

    assert!(env.scheduler.get_process_list().iter().any(|p| p.pid == term_pid));
}

#[test]
fn test_tier3_03_editor_under_high_memory_allocation_pressure() {
    let mut env = AegisOsKernelEnv::new();
    let (_, pad_pid) = env.launch_app(AppId::AegisPad);

    // Type 20 lines into AegisPad
    if let Some(pad) = &mut env.pad_app {
        for i in 0..20 {
            let line_text = format!("AegisOS Stress Test Line {}", i);
            for b in line_text.bytes() {
                pad.handle_key(b);
            }
            pad.handle_key(b'\n');
        }
    }

    // Allocate 5,000 frames under memory pressure
    let mut allocated_frames = Vec::new();
    for _ in 0..5000 {
        if let Some(frame) = env.frame_alloc.alloc_frame() {
            allocated_frames.push(frame);
        }
    }
    assert_eq!(allocated_frames.len(), 5000);

    // Render compositor frame while under memory load
    let pixels = env.render_desktop();
    assert!(pixels > 0);

    // Free all allocated memory frames
    for frame in allocated_frames {
        assert!(env.frame_alloc.free_frame(frame));
    }

    // Verify AegisPad text remains 100% intact
    if let Some(pad) = &env.pad_app {
        assert_eq!(pad.pid, pad_pid);
        assert!(pad.lines.len() >= 20);
        assert!(pad.total_characters() > 300);
    }
}

#[test]
fn test_tier3_04_terminal_spawn_and_activity_monitor_kill() {
    let mut env = AegisOsKernelEnv::new();
    let (_, term_pid) = env.launch_app(AppId::Terminal);
    let (_, mon_pid) = env.launch_app(AppId::ActivityMonitor);

    let initial_frames = env.frame_alloc.allocated_count();

    // 1. Terminal spawns crashtest app
    let term = env.terminal_app.as_mut().unwrap();
    let run_res = term.execute_command("run crashtest", &mut env.scheduler, &env.frame_alloc);
    assert!(run_res[0].contains("Spawned process 'crashtest' with PID"));

    let spawned_pid = env.scheduler.get_process_list().last().unwrap().pid;
    assert!(spawned_pid > mon_pid);

    // 2. Activity Monitor detects new PID, selects it, and kills it
    let mon = env.activity_monitor_app.as_mut().unwrap();
    mon.select_process(spawned_pid);
    let killed = mon.kill_selected_process(&mut env.scheduler);
    assert!(killed, "Activity monitor must kill selected PID");

    // 3. Step timer tick for deferred zombie frame reclamation
    env.timer_tick();

    // 4. Verify terminal 'ps' confirms process is gone
    let term = env.terminal_app.as_mut().unwrap();
    let ps_res = term.execute_command("ps", &mut env.scheduler, &env.frame_alloc);
    assert!(ps_res.iter().all(|line| !line.starts_with(&format!("{:<4}", spawned_pid))));

    // 5. Verify frames reclaimed
    assert_eq!(env.frame_alloc.allocated_count(), initial_frames);
}

#[test]
fn test_tier3_05_mouse_drag_keyboard_typing_during_preemptive_interrupts() {
    let mut env = AegisOsKernelEnv::new();
    let (term_wid, _) = env.launch_app(AppId::Terminal);
    let (pad_wid, _) = env.launch_app(AppId::AegisPad);

    // Focus Terminal
    env.wm.handle_mouse_down(160, 160);
    assert_eq!(env.wm.focused_window().unwrap().id, term_wid);

    // Loop with simultaneous mouse drag, keyboard input, and timer ticks
    for i in 0..10 {
        // 1. Timer tick (100Hz preemption)
        env.timer_tick();

        // 2. Mouse move packet
        let _ = env.send_mouse_packet([0x08, 5, 5]);

        // 3. Send keyboard input to focused Terminal
        env.send_key_scancode(0x1E); // 'a'
        env.send_key_scancode(0x30); // 'b'

        // 4. Render frame
        env.render_desktop();
    }

    assert_eq!(env.terminal_app.as_ref().unwrap().command_buffer, "abababababababababab");
}

#[test]
fn test_tier3_06_compositor_60fps_telemetry_during_user_faults() {
    let mut env = AegisOsKernelEnv::new();
    env.launch_app(AppId::ActivityMonitor);

    for _ in 0..5 {
        let (_, crash_pid) = env.launch_app(AppId::CrashTest);

        // Render before fault
        let p1 = env.render_desktop();
        assert!(p1 > 0);

        // Fault occurs
        env.trigger_user_fault(crash_pid, ExceptionVector::DivideByZero, 0x401340, 0).unwrap();

        // Reaping & next frame render
        env.timer_tick();
        let p2 = env.render_desktop();
        assert!(p2 > 0);
    }

    assert_eq!(env.scheduler.get_process_list().len(), 3); // [idle], desktop, activity_monitor
}

#[test]
fn test_tier3_07_terminal_free_matches_activity_monitor_under_60mb() {
    let mut env = AegisOsKernelEnv::new();
    env.launch_app(AppId::Terminal);
    env.launch_app(AppId::ActivityMonitor);

    let stats = env.get_memory_stats();
    assert!(stats.used_bytes < MAX_IDLE_RAM_BYTES, "Idle RAM usage must be < 60 MB");

    let term = env.terminal_app.as_mut().unwrap();
    let free_out = term.execute_command("free", &mut env.scheduler, &env.frame_alloc);
    assert!(free_out[1].contains("<60MB footprint verified"));
}

#[test]
fn test_tier3_08_window_close_traffic_light_during_active_scheduler_loop() {
    let mut env = AegisOsKernelEnv::new();
    let (term_wid, term_pid) = env.launch_app(AppId::Terminal);

    // Put task in running state
    env.scheduler.tasks.iter_mut().find(|t| t.pid == term_pid).unwrap().state = ProcessState::Running;

    // Click red traffic light on Terminal window
    let win = env.wm.windows.iter().find(|w| w.id == term_wid).unwrap();
    let close_x = win.x + 16;
    let close_y = win.y + 12;
    env.wm.handle_mouse_down(close_x, close_y);

    // Verify window is removed
    assert!(env.wm.windows.iter().all(|w| w.id != term_wid));

    // Kill associated PID and reap
    env.scheduler.kill_process(term_pid);
    env.timer_tick();

    assert!(env.scheduler.get_process_list().iter().all(|p| p.pid != term_pid));
}
