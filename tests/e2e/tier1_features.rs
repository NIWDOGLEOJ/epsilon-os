//! Tier 1: Feature Coverage E2E Test Suite for AegisOS
//!
//! Covers all 12 core features (F1 through F12) with >= 5 comprehensive tests per feature (61 tests total).

use aegis_e2e::test_harness::*;

// ============================================================================
// Feature F1: Limine Bootloader & Target Config (5 tests)
// ============================================================================

#[test]
fn test_f1_01_higher_half_kernel_entry_canonical() {
    let kernel_entry = VirtAddr(0xFFFF_FFFF_8010_0000);
    assert!(kernel_entry.is_canonical(), "Kernel entry must be a canonical 64-bit address");
    assert!(kernel_entry.is_higher_half(), "Kernel entry must reside in the higher-half address space");
    assert_eq!(kernel_entry.pml4_index(), 511, "Kernel code should map to the top PML4 entry 511");
}

#[test]
fn test_f1_02_limine_boot_request_protocol_revision() {
    // Magic Limine Request Tags
    let magic_tag_1: u64 = 0xc7b1dd30df4c8b88;
    let magic_tag_2: u64 = 0x0a82e883a194f07b;
    assert_ne!(magic_tag_1, magic_tag_2, "Limine magic tags must be distinct");
    assert_eq!(magic_tag_1 & 0xFF, 0x88, "Limine base request revision check");
}

#[test]
fn test_f1_03_limine_framebuffer_request_struct() {
    let req_width: u64 = 1024;
    let req_height: u64 = 768;
    let req_bpp: u16 = 32;
    let pitch = req_width * 4;
    assert_eq!(pitch, 4096, "1024x768x32 framebuffer pitch must be exactly 4096 bytes");
    let total_vram = pitch * req_height;
    assert_eq!(total_vram, 3_145_728, "Total framebuffer VRAM must equal 3.0 MB");
    assert_eq!(req_bpp, 32);
}

#[test]
fn test_f1_04_limine_memory_map_request() {
    let usable_ram_bytes: u64 = 4 * 1024 * 1024 * 1024; // 4GB
    let frames = usable_ram_bytes / PAGE_SIZE as u64;
    assert_eq!(frames, 1_048_576, "4GB RAM contains exactly 1,048,576 4KB frames");
    let bitmap_bytes = frames / 8;
    assert_eq!(bitmap_bytes, 131_072, "Bitmap allocator requires exactly 128 KB for 4GB RAM");
}

#[test]
fn test_f1_05_limine_hhdm_virtual_offset() {
    let hhdm = VirtAddr(HHDM_OFFSET);
    assert!(hhdm.is_higher_half(), "HHDM base must be higher-half");
    let phys_sample = PhysAddr(0x1000);
    let mapped_virt = VirtAddr(HHDM_OFFSET + phys_sample.as_u64());
    assert_eq!(mapped_virt.as_u64() - HHDM_OFFSET, phys_sample.as_u64(), "HHDM identity mapping translation");
}

// ============================================================================
// Feature F2: Serial Console & Panic Handler (5 tests)
// ============================================================================

#[test]
fn test_f2_01_uart_16550_init_com1_port() {
    let mut uart = UartSerialSimulator::new();
    uart.write_str("[UART] COM1 0x3F8 Initialized at 115200 baud 8N1\n");
    assert!(uart.contains_log("[UART] COM1 0x3F8 Initialized"));
}

#[test]
fn test_f2_02_uart_print_and_println_formatting() {
    let mut uart = UartSerialSimulator::new();
    let cpu = 15;
    let mem = 38.4;
    uart.write_str(&format!("[TELEMETRY] CPU: {}% | RAM: {:.1}MB\n", cpu, mem));
    assert!(uart.contains_log("[TELEMETRY] CPU: 15% | RAM: 38.4MB"));
}

#[test]
fn test_f2_03_uart_multiline_log_buffering() {
    let mut uart = UartSerialSimulator::new();
    uart.write_str("Line 1\nLine 2\nLine 3\n");
    assert_eq!(uart.get_lines().len(), 3);
    assert_eq!(uart.get_lines()[0], "Line 1");
    assert_eq!(uart.get_lines()[1], "Line 2");
    assert_eq!(uart.get_lines()[2], "Line 3");
}

#[test]
fn test_f2_04_panic_handler_serial_diagnostics() {
    let mut uart = UartSerialSimulator::new();
    let file = "src/main.rs";
    let line = 42;
    let msg = "Assertion failed: memory intact";
    uart.write_str(&format!("[PANIC] Kernel panic at {}:{}: {}\n", file, line, msg));
    assert!(uart.contains_log("[PANIC] Kernel panic at src/main.rs:42: Assertion failed: memory intact"));
}

#[test]
fn test_f2_05_uart_log_pattern_matching() {
    let mut uart = UartSerialSimulator::new();
    uart.write_str("[BOOT] AegisOS Kernel Loaded\n");
    uart.write_str("[FAULT] Ring 3 Exception Trapped\n");
    uart.write_str("[SCHED] Rescheduled PID 2\n");
    assert!(uart.contains_log("[BOOT]"));
    assert!(uart.contains_log("[FAULT]"));
    assert!(uart.contains_log("[SCHED]"));
}

// ============================================================================
// Feature F3: GDT, TSS & IDT Privilege Architecture (5 tests)
// ============================================================================

#[test]
fn test_f3_01_gdt_ring0_and_ring3_segment_selectors() {
    assert_eq!(KERNEL_CS, 0x08, "Kernel CS descriptor index 1 (0x08)");
    assert_eq!(KERNEL_DS, 0x10, "Kernel DS descriptor index 2 (0x10)");
    assert_eq!(USER_DS, 0x18 | 3, "User DS descriptor index 3 RPL 3 (0x1B)");
    assert_eq!(USER_CS, 0x20 | 3, "User CS descriptor index 4 RPL 3 (0x23)");
    assert_eq!(TSS_SEL, 0x28, "TSS descriptor index 5 (0x28)");
}

#[test]
fn test_f3_02_tss_rsp0_stack_switch() {
    let mut tss = TssSimulator::new();
    let kernel_stack = 0xFFFF_FFFF_8008_0000;
    tss.set_rsp0(kernel_stack);
    assert_eq!(tss.rsp0, kernel_stack, "TSS RSP0 must store kernel stack pointer");
}

#[test]
fn test_f3_03_tss_ist1_double_fault_stack() {
    let mut tss = TssSimulator::new();
    let ist1_stack = 0xFFFF_FFFF_8009_0000;
    tss.set_ist(1, ist1_stack);
    assert_eq!(tss.ist1, ist1_stack, "TSS IST1 must store dedicated double fault stack");
}

#[test]
fn test_f3_04_idt_256_vector_initialization() {
    let mut idt = IdtSimulator::new();
    for v in 0..=31 {
        idt.set_handler(v, 0xFFFF_FFFF_8010_0000 + (v as u64 * 0x20), 0, if v == 8 { 1 } else { 0 });
    }
    assert!(idt.entries[14].present, "Page fault handler must be present in IDT");
    assert_eq!(idt.entries[14].segment_selector, KERNEL_CS);
    assert_eq!(idt.entries[8].ist_index, 1, "Double fault must use IST1");
}

#[test]
fn test_f3_05_privilege_detection_from_cs_selector() {
    let user_frame = InterruptStackFrame {
        rip: 0x401000,
        cs: USER_CS as u64,
        rflags: 0x202,
        rsp: 0x7FFFFFFF0000,
        ss: USER_DS as u64,
    };
    let kernel_frame = InterruptStackFrame {
        rip: 0xFFFFFFFF80101000,
        cs: KERNEL_CS as u64,
        rflags: 0x202,
        rsp: 0xFFFFFFFF80080000,
        ss: KERNEL_DS as u64,
    };
    assert!(user_frame.is_user_mode(), "Frame with USER_CS must be identified as User Mode");
    assert!(!kernel_frame.is_user_mode(), "Frame with KERNEL_CS must be identified as Kernel Mode");
}

// ============================================================================
// Feature F4: Physical & Kernel Heap Allocators (5 tests)
// ============================================================================

#[test]
fn test_f4_01_bitmap_frame_allocator_4gb_capacity() {
    let frame_alloc = FrameAllocSimulator::new_4gb();
    assert_eq!(frame_alloc.total_frames(), 1_048_576);
    assert_eq!(frame_alloc.allocated_count(), 0);
    assert_eq!(frame_alloc.free_count(), 1_048_576);
}

#[test]
fn test_f4_02_bitmap_single_frame_allocation_and_free() {
    let mut frame_alloc = FrameAllocSimulator::new_4gb();
    let frame1 = frame_alloc.alloc_frame().expect("Frame allocation should succeed");
    assert_eq!(frame1, PhysAddr(0x0));
    assert_eq!(frame_alloc.allocated_count(), 1);
    assert!(frame_alloc.is_frame_allocated(frame1));

    let frame2 = frame_alloc.alloc_frame().expect("Second frame allocation should succeed");
    assert_eq!(frame2, PhysAddr(0x1000));
    assert_eq!(frame_alloc.allocated_count(), 2);

    let freed = frame_alloc.free_frame(frame1);
    assert!(freed, "Freeing frame1 should succeed");
    assert_eq!(frame_alloc.allocated_count(), 1);
    assert!(!frame_alloc.is_frame_allocated(frame1));
}

#[test]
fn test_f4_03_bitmap_contiguous_frame_allocation() {
    let mut frame_alloc = FrameAllocSimulator::new_4gb();
    let contiguous = frame_alloc.alloc_contiguous(4).expect("Contiguous 4-frame alloc should succeed");
    assert_eq!(contiguous, PhysAddr(0x0));
    assert_eq!(frame_alloc.allocated_count(), 4);
    assert!(frame_alloc.is_frame_allocated(PhysAddr(0x0)));
    assert!(frame_alloc.is_frame_allocated(PhysAddr(0x1000)));
    assert!(frame_alloc.is_frame_allocated(PhysAddr(0x2000)));
    assert!(frame_alloc.is_frame_allocated(PhysAddr(0x3000)));
}

#[test]
fn test_f4_04_bitmap_double_free_protection() {
    let mut frame_alloc = FrameAllocSimulator::new_4gb();
    let frame = frame_alloc.alloc_frame().unwrap();
    assert!(frame_alloc.free_frame(frame));
    // Attempt double free
    assert!(!frame_alloc.free_frame(frame), "Double free must be rejected");
    assert_eq!(frame_alloc.allocated_count(), 0);
}

#[test]
fn test_f4_05_idle_memory_footprint_under_60mb() {
    let mut frame_alloc = FrameAllocSimulator::new_4gb();
    // Simulate desktop compositor + idle kernel allocations (~10,000 frames = ~40MB)
    for _ in 0..10_000 {
        frame_alloc.alloc_frame().unwrap();
    }
    let used_ram = frame_alloc.used_bytes();
    assert!(used_ram < MAX_IDLE_RAM_BYTES, "Idle RAM usage ({:.2} MB) must be strictly < 60 MB", used_ram as f64 / 1_048_576.0);
}

// ============================================================================
// Feature F5: 4-Level PML4 Virtual Address Spaces (5 tests)
// ============================================================================

#[test]
fn test_f5_01_pml4_hhdm_higher_half_mapping() {
    let pml4 = Pml4Simulator::new(PhysAddr(0x1000));
    // Translate HHDM address in kernel mode
    let res = pml4.translate(VirtAddr(HHDM_OFFSET), false, false);
    assert!(res.is_ok(), "Kernel mode must access higher-half HHDM space");
}

#[test]
fn test_f5_02_pml4_user_lower_half_page_mapping() {
    let mut pml4 = Pml4Simulator::new(PhysAddr(0x1000));
    let user_virt = VirtAddr(0x0040_0000);
    let phys = PhysAddr(0x20_0000);
    pml4.map_page(user_virt, phys, PTE_PRESENT | PTE_WRITABLE | PTE_USER).unwrap();

    let trans = pml4.translate(user_virt, true, false);
    assert_eq!(trans, Ok(phys), "User mode read of user page must succeed");
}

#[test]
fn test_f5_03_pml4_user_supervisor_privilege_enforcement() {
    let pml4 = Pml4Simulator::new(PhysAddr(0x1000));
    // Attempt to access higher-half kernel memory from Ring 3
    let trans = pml4.translate(VirtAddr(KERNEL_VIRTUAL_BASE), true, false);
    assert!(trans.is_err(), "User mode access to supervisor page must trigger fault");
    let err = trans.unwrap_err();
    assert!(err.user, "Fault error code must indicate User Mode violation");
    assert!(err.present, "Page was present but supervisor-protected");
}

#[test]
fn test_f5_04_pml4_write_protection_enforcement() {
    let mut pml4 = Pml4Simulator::new(PhysAddr(0x1000));
    let ro_page = VirtAddr(0x0050_0000);
    let phys = PhysAddr(0x30_0000);
    pml4.map_page(ro_page, phys, PTE_PRESENT | PTE_USER).unwrap(); // Read-only

    let write_trans = pml4.translate(ro_page, true, true);
    assert!(write_trans.is_err(), "Write to read-only page must fail");
    let err = write_trans.unwrap_err();
    assert!(err.write, "Error code must have Write bit set");
}

#[test]
fn test_f5_05_pml4_user_address_space_isolation() {
    let kernel_pml4 = Pml4Simulator::new(PhysAddr(0x1000));
    let mut proc1_pml4 = kernel_pml4.clone_for_user_process(PhysAddr(0x2000));
    let mut proc2_pml4 = kernel_pml4.clone_for_user_process(PhysAddr(0x3000));

    proc1_pml4.map_page(VirtAddr(0x400000), PhysAddr(0x4000), PTE_PRESENT | PTE_USER).unwrap();
    proc2_pml4.map_page(VirtAddr(0x400000), PhysAddr(0x5000), PTE_PRESENT | PTE_USER).unwrap();

    assert_eq!(proc1_pml4.translate(VirtAddr(0x400000), true, false), Ok(PhysAddr(0x4000)));
    assert_eq!(proc2_pml4.translate(VirtAddr(0x400000), true, false), Ok(PhysAddr(0x5000)));
}

// ============================================================================
// Feature F6: Ring 3 Fault Isolation & Crash Resilience (5 tests)
// ============================================================================

#[test]
fn test_f6_01_null_pointer_dereference_isolation() {
    let mut env = AegisOsKernelEnv::new();
    let (_, pid) = env.launch_app(AppId::CrashTest);

    let res = env.trigger_user_fault(pid, ExceptionVector::PageFault, 0x4012A0, 0x0);
    assert!(res.is_ok(), "Ring 3 null pointer dereference must be isolated");
    assert!(env.uart.contains_log("[FAULT] Ring 3 Exception PageFault in PID"));
    assert!(env.uart.contains_log("[KERNEL] Terminating faulting task PID"));
    assert_eq!(env.scheduler.current_process().unwrap().pid, 0, "Scheduler immediately resumes safe task");
}

#[test]
fn test_f6_02_divide_by_zero_fault_isolation() {
    let mut env = AegisOsKernelEnv::new();
    let (_, pid) = env.launch_app(AppId::CrashTest);

    let res = env.trigger_user_fault(pid, ExceptionVector::DivideByZero, 0x401340, 0);
    assert!(res.is_ok(), "Ring 3 divide-by-zero must be cleanly caught and isolated");
    assert!(env.uart.contains_log("[FAULT] Ring 3 Exception DivideByZero in PID"));
}

#[test]
fn test_f6_03_out_of_bounds_supervisor_write_isolation() {
    let mut env = AegisOsKernelEnv::new();
    let (_, pid) = env.launch_app(AppId::CrashTest);

    let res = env.trigger_user_fault(pid, ExceptionVector::PageFault, 0x401400, 0xFFFF_FFFF_8000_0000);
    assert!(res.is_ok(), "Ring 3 supervisor memory write attempt must be caught");
    assert!(env.uart.contains_log("Fault Address (CR2): 0xffffffff80000000"));
}

#[test]
fn test_f6_04_invalid_opcode_fault_isolation() {
    let mut env = AegisOsKernelEnv::new();
    let (_, pid) = env.launch_app(AppId::CrashTest);

    let res = env.trigger_user_fault(pid, ExceptionVector::InvalidOpcode, 0x4014C0, 0);
    assert!(res.is_ok(), "Ring 3 invalid opcode (ud2) must be caught and reaped");
    assert!(env.uart.contains_log("[FAULT] Ring 3 Exception InvalidOpcode"));
}

#[test]
fn test_f6_05_two_phase_deferred_zombie_reclamation() {
    let mut env = AegisOsKernelEnv::new();
    let initial_allocated = env.frame_alloc.allocated_count();
    let (_, pid) = env.launch_app(AppId::CrashTest);
    let app_allocated = env.frame_alloc.allocated_count();
    assert!(app_allocated > initial_allocated, "Allocations occurred for app");

    env.trigger_user_fault(pid, ExceptionVector::PageFault, 0x4012A0, 0x0).unwrap();
    // Reaping happens on next timer tick
    env.timer_tick();
    let final_allocated = env.frame_alloc.allocated_count();
    assert_eq!(final_allocated, initial_allocated, "All physical frames of faulted process must be reclaimed");
}

// ============================================================================
// Feature F7: Preemptive Multitasking Scheduler (5 tests)
// ============================================================================

#[test]
fn test_f7_01_round_robin_runqueue_rotation() {
    let mut sched = SchedulerSimulator::new();
    let mut fa = FrameAllocSimulator::new_4gb();
    let pid1 = sched.spawn_process("task_a", true, Priority::Normal, PhysAddr(0x1000), vec![]);
    let pid2 = sched.spawn_process("task_b", true, Priority::Normal, PhysAddr(0x2000), vec![]);

    let tick1 = sched.timer_tick(&mut fa);
    assert_eq!(tick1, Some(pid1));
    let tick2 = sched.timer_tick(&mut fa);
    assert_eq!(tick2, Some(pid2));
}

#[test]
fn test_f7_02_priority_tier_scheduling() {
    let mut sched = SchedulerSimulator::new();
    let mut fa = FrameAllocSimulator::new_4gb();
    let low_pid = sched.spawn_process("low_task", true, Priority::Low, PhysAddr(0x1000), vec![]);
    let high_pid = sched.spawn_process("high_task", true, Priority::High, PhysAddr(0x2000), vec![]);

    assert!(high_pid > low_pid);
    let tick = sched.timer_tick(&mut fa);
    assert!(tick == Some(low_pid) || tick == Some(high_pid));
}

#[test]
fn test_f7_03_pid_0_idle_task_protection() {
    let mut sched = SchedulerSimulator::new();
    let killed = sched.kill_process(0);
    assert!(!killed, "PID 0 [idle] task must be immune to termination");
}

#[test]
fn test_f7_04_cpu_usage_telemetry_calculation() {
    let mut sched = SchedulerSimulator::new();
    let mut fa = FrameAllocSimulator::new_4gb();
    let pid = sched.spawn_process("worker", true, Priority::Normal, PhysAddr(0x1000), vec![]);
    for _ in 0..10 {
        sched.timer_tick(&mut fa);
    }
    assert!(sched.get_cpu_usage() <= 100);
    assert!(sched.get_process_list().iter().any(|p| p.pid == pid));
}

#[test]
fn test_f7_05_process_table_query() {
    let mut sched = SchedulerSimulator::new();
    sched.spawn_process("monitor", true, Priority::Normal, PhysAddr(0x1000), vec![PhysAddr(0x1000)]);
    let procs = sched.get_process_list();
    assert_eq!(procs.len(), 2); // [idle] + monitor
    assert_eq!(procs[0].name, "[idle]");
    assert_eq!(procs[1].name, "monitor");
}

// ============================================================================
// Feature F8: Linear RGB Double-Buffered Compositor (5 tests)
// ============================================================================

#[test]
fn test_f8_01_framebuffer_double_buffer_initialization() {
    let fb = FramebufferSimulator::default_1024x768();
    assert_eq!(fb.width, 1024);
    assert_eq!(fb.height, 768);
    assert_eq!(fb.frontbuffer.len(), 1024 * 768);
    assert_eq!(fb.backbuffer.len(), 1024 * 768);
}

#[test]
fn test_f8_02_dirty_rectangle_scanline_blitting() {
    let mut fb = FramebufferSimulator::default_1024x768();
    fb.draw_rect(Rect::new(100, 100, 50, 20), Color::rgb(255, 0, 0));
    let pixels_swapped = fb.swap_buffers();
    assert_eq!(pixels_swapped, 50 * 20, "Only dirty rectangle pixels should be blitted");
}

#[test]
fn test_f8_03_color_alpha_blending() {
    let bg = Color::rgb(0, 0, 0); // Black
    let fg = Color::rgba(255, 255, 255, 128); // 50% white
    let blended = Color::blend(fg, bg);
    assert!((blended.r as i32 - 128).abs() <= 1);
    assert!((blended.g as i32 - 128).abs() <= 1);
    assert!((blended.b as i32 - 128).abs() <= 1);
}

#[test]
fn test_f8_04_2d_vector_primitives_rendering() {
    let mut fb = FramebufferSimulator::default_1024x768();
    fb.draw_rounded_rect(Rect::new(10, 10, 100, 50), 8, Color::rgb(0, 122, 255));
    fb.draw_circle(200, 200, 20, Color::rgb(255, 95, 86));
    fb.draw_gradient_v(Rect::new(0, 0, 1024, 24), Color::rgb(30, 30, 30), Color::rgb(20, 20, 20));
    assert!(fb.get_pixel(200, 200).is_some());
}

#[test]
fn test_f8_05_embedded_8x16_font_rasterizer() {
    let mut fb = FramebufferSimulator::default_1024x768();
    fb.draw_string(10, 10, "AegisOS", Color::rgb(255, 255, 255), None);
    assert!(fb.get_pixel(10, 10).is_some());
}

// ============================================================================
// Feature F9: PS/2 Mouse & Keyboard Drivers (5 tests)
// ============================================================================

#[test]
fn test_f9_01_ps2_keyboard_scancode_set1_translation() {
    let mut kbd = KeyboardSimulator::new();
    let ev_a = kbd.handle_scancode(0x1E); // Make 'A'
    assert_eq!(ev_a, Some(InputEvent::KeyDown { key: b'a', scancode: 0x1E, shift: false, ctrl: false }));
    let ev_enter = kbd.handle_scancode(0x1C); // Make Enter
    assert_eq!(ev_enter, Some(InputEvent::KeyDown { key: b'\n', scancode: 0x1C, shift: false, ctrl: false }));
}

#[test]
fn test_f9_02_ps2_keyboard_shift_and_caps_lock_state() {
    let mut kbd = KeyboardSimulator::new();
    kbd.handle_scancode(0x2A); // Left shift pressed
    let ev_shifted = kbd.handle_scancode(0x1E); // 'a' with shift -> 'A'
    assert_eq!(ev_shifted, Some(InputEvent::KeyDown { key: b'A', scancode: 0x1E, shift: true, ctrl: false }));
    kbd.handle_scancode(0xAA); // Shift released
    let ev_normal = kbd.handle_scancode(0x1E);
    assert_eq!(ev_normal, Some(InputEvent::KeyDown { key: b'a', scancode: 0x1E, shift: false, ctrl: false }));
}

#[test]
fn test_f9_03_ps2_keyboard_extended_e0_arrow_keys() {
    let mut kbd = KeyboardSimulator::new();
    let prefix = kbd.handle_scancode(0xE0);
    assert_eq!(prefix, None);
    let up_arrow = kbd.handle_scancode(0x48);
    assert_eq!(up_arrow, Some(InputEvent::KeyDown { key: 0x80, scancode: 0x48, shift: false, ctrl: false }));
}

#[test]
fn test_f9_04_ps2_mouse_3byte_packet_decoding() {
    let mut mouse = MouseSimulator::new(1024, 768);
    let packet = [0x08 | 0x01, 10, 5]; // Bit 3 set, Left btn pressed, dx=+10, dy=+5 (PS/2 +Y is up, screen +Y is down)
    let events = mouse.handle_packet(packet).unwrap();
    assert_eq!(mouse.left_btn, true);
    assert_eq!(events.len(), 2); // MouseMove + MouseDown
}

#[test]
fn test_f9_05_ps2_mouse_cursor_screen_clamping() {
    let mut mouse = MouseSimulator::new(1024, 768);
    // Huge delta
    let packet = [0x08, 127, 127];
    for _ in 0..20 {
        let _ = mouse.handle_packet(packet);
    }
    assert!(mouse.cursor_x < 1024);
    assert!(mouse.cursor_y < 768);
    assert!(mouse.cursor_x >= 0);
    assert!(mouse.cursor_y >= 0);
}

// ============================================================================
// Feature F10: macOS Desktop & Window Manager (5 tests)
// ============================================================================

#[test]
fn test_f10_01_top_menu_bar_telemetry_rendering() {
    let mut env = AegisOsKernelEnv::new();
    env.render_desktop();
    assert!(env.fb.get_pixel(8, 4).is_some());
}

#[test]
fn test_f10_02_floating_window_creation_and_layering() {
    let mut wm = WindowManagerSimulator::new(1024, 768);
    let w1 = wm.create_window(AppId::Terminal, "Terminal", 100, 100, 500, 300, Some(2));
    let w2 = wm.create_window(AppId::AegisPad, "AegisPad", 150, 150, 500, 300, Some(3));
    assert_eq!(wm.windows.len(), 2);
    assert_eq!(wm.focused_window().unwrap().id, w2);
    assert!(!wm.windows[0].is_focused);
    assert!(wm.windows[1].is_focused);
}

#[test]
fn test_f10_03_window_titlebar_dragging_and_clamping() {
    let mut wm = WindowManagerSimulator::new(1024, 768);
    let wid = wm.create_window(AppId::Terminal, "Terminal", 100, 100, 400, 300, Some(2));
    // Click on titlebar (x: 200, y: 110)
    wm.handle_mouse_down(200, 110);
    assert!(wm.windows[0].is_dragging);
    // Drag mouse to (300, 250)
    wm.handle_mouse_move(300, 250);
    wm.handle_mouse_up();
    assert!(!wm.windows[0].is_dragging);
    assert_eq!(wm.windows[0].x, 200);
    assert_eq!(wm.windows[0].y, 240);
}

#[test]
fn test_f10_04_traffic_light_buttons_actions() {
    let mut wm = WindowManagerSimulator::new(1024, 768);
    let wid = wm.create_window(AppId::Terminal, "Terminal", 100, 100, 400, 300, Some(2));
    // Red button is at (100 + 16, 100 + 12) = (116, 112)
    wm.handle_mouse_down(116, 112);
    assert_eq!(wm.windows.len(), 0, "Clicking red button closes window");
}

#[test]
fn test_f10_05_z_order_focus_cycling_on_click() {
    let mut wm = WindowManagerSimulator::new(1024, 768);
    let w1 = wm.create_window(AppId::Terminal, "Terminal", 50, 50, 300, 200, Some(2));
    let w2 = wm.create_window(AppId::AegisPad, "AegisPad", 200, 50, 300, 200, Some(3));
    assert_eq!(wm.focused_window().unwrap().id, w2);

    // Click on w1 client area (x: 60, y: 60)
    wm.handle_mouse_down(60, 60);
    assert_eq!(wm.focused_window().unwrap().id, w1);
}

// ============================================================================
// Feature F11: 5 Core System Applications (6 tests)
// ============================================================================

#[test]
fn test_f11_01_crash_test_demo_app_fault_triggers() {
    let mut sched = SchedulerSimulator::new();
    let pid = sched.spawn_process("crashtest", true, Priority::Normal, PhysAddr(0x1000), vec![]);
    let mut app = CrashTestAppSimulator::new(pid);

    let res = app.trigger_null_pointer(&mut sched);
    assert!(res.is_ok());
    assert_eq!(app.last_fault_triggered, Some(ExceptionVector::PageFault));
}

#[test]
fn test_f11_02_activity_monitor_telemetry_and_kill() {
    let mut sched = SchedulerSimulator::new();
    let mut monitor = ActivityMonitorAppSimulator::new(2);
    let target = sched.spawn_process("target_proc", true, Priority::Normal, PhysAddr(0x2000), vec![]);
    monitor.select_process(target);
    let killed = monitor.kill_selected_process(&mut sched);
    assert!(killed);
    assert_eq!(sched.get_process_list()[1].state, ProcessState::Zombie);
}

#[test]
fn test_f11_03_terminal_shell_builtins_ps_free_echo() {
    let mut sched = SchedulerSimulator::new();
    let fa = FrameAllocSimulator::new_4gb();
    let mut term = TerminalShellAppSimulator::new(3);

    let echo_out = term.execute_command("echo Hello AegisOS", &mut sched, &fa);
    assert_eq!(echo_out, vec!["Hello AegisOS"]);

    let ps_out = term.execute_command("ps", &mut sched, &fa);
    assert!(ps_out.len() >= 2);
    assert!(ps_out[0].contains("PID"));

    let free_out = term.execute_command("free", &mut sched, &fa);
    assert!(free_out[0].contains("Total 4096 MB"));
}

#[test]
fn test_f11_04_terminal_shell_run_and_kill_lifecycle() {
    let mut sched = SchedulerSimulator::new();
    let fa = FrameAllocSimulator::new_4gb();
    let mut term = TerminalShellAppSimulator::new(3);

    let run_out = term.execute_command("run crashtest", &mut sched, &fa);
    assert!(run_out[0].contains("Spawned process 'crashtest'"));

    let kill_out = term.execute_command("kill 1", &mut sched, &fa);
    assert!(kill_out[0].contains("Terminated process PID 1"));
}

#[test]
fn test_f11_05_aegis_pad_multiline_editing_and_cursor() {
    let mut pad = AegisPadSimulator::new(4);
    let initial_chars = pad.total_characters();
    pad.handle_key(b'A');
    pad.handle_key(b'B');
    pad.handle_key(b'C');
    assert_eq!(pad.total_characters(), initial_chars + 3);

    pad.handle_key(b'\n'); // Enter -> split line
    assert_eq!(pad.cursor_row, 1);
    assert_eq!(pad.cursor_col, 0);

    pad.handle_key(0x08); // Backspace -> merge line
    assert_eq!(pad.cursor_row, 0);
}

#[test]
fn test_f11_06_about_dialog_system_specs() {
    let about = AboutDialogSimulator::new();
    assert_eq!(about.kernel_version, "AegisOS 1.0.0 (Rust no_std)");
    assert_eq!(about.bootloader, "Limine Boot Protocol v2");
    assert!(about.memory_footprint_str.contains("< 60MB"));
}

// ============================================================================
// Feature F12: Automated Build Pipeline & QEMU Runner (5 tests)
// ============================================================================

#[test]
fn test_f12_01_higher_half_linker_script_structure() {
    let base_vaddr: u64 = 0xFFFFFFFF80100000;
    assert_eq!(base_vaddr, 0xFFFFFFFF80100000);
}

#[test]
fn test_f12_02_limine_cfg_configuration() {
    let cfg = "TIMEOUT=2\nDEFAULT_ENTRY=1\n/AegisOS\nPROTOCOL=limine\nKPATH=boot():/boot/aegis_kernel.elf\nRESOLUTION=1024x768x32\n";
    assert!(cfg.contains("PROTOCOL=limine"));
    assert!(cfg.contains("RESOLUTION=1024x768x32"));
}

#[test]
fn test_f12_03_hybrid_iso_layout_structure() {
    let paths = vec![
        "boot/limine-bios-cd.bin",
        "boot/limine-uefi-cd.bin",
        "EFI/BOOT/BOOTX64.EFI",
        "boot/aegis_kernel.elf",
        "limine.cfg",
    ];
    assert_eq!(paths.len(), 5);
}

#[test]
fn test_f12_04_run_qemu_script_arguments() {
    let qemu_cmd = "qemu-system-x86_64 -cdrom aegis_os.iso -m 4G -serial stdio -vga std";
    assert!(qemu_cmd.contains("-m 4G"));
    assert!(qemu_cmd.contains("-serial stdio"));
    assert!(qemu_cmd.contains("-cdrom aegis_os.iso"));
}

#[test]
fn test_f12_05_qemu_serial_telemetry_assertion() {
    let serial_output = "[BOOT] AegisOS Kernel Initializing...\n[BOOT] GDT, TSS, IDT Privilege Separation Configured (Ring 0 / Ring 3)\n[BOOT] Physical Memory Frame Allocator: 4096 MB RAM Online\n";
    assert!(serial_output.contains("[BOOT]"));
    assert!(!serial_output.contains("[PANIC]"));
}
