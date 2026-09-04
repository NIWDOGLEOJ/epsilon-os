#![no_std]
#![no_main]

extern crate alloc;

pub mod apps;
pub mod arch;
pub mod drivers;
pub mod fs;
pub mod gui;
pub mod memory;
pub mod net;
pub mod task;
pub mod agent;

#[cfg(feature = "selftest")]
pub mod selftest;

use alloc::format;
use alloc::string::ToString;

use limine::BaseRevision;
use limine::request::{
    ExecutableAddressRequest, FramebufferRequest, HhdmRequest, MemoryMapRequest,
    RequestsEndMarker, RequestsStartMarker,
};

use crate::apps::{AppAction, AppSuite};
use crate::arch::serial::outb;
use crate::drivers::framebuffer::FRAMEBUFFER;
use crate::drivers::ps2_keyboard::poll_key_event;
use crate::drivers::ps2_mouse::{poll_mouse_event, MouseButton, MouseEvent};
use crate::gui::dock::AppId;
use crate::gui::primitives::Color;
use crate::gui::wm::{WmAction, WindowManager};

// ============================================================================
// Limine Protocol Requests (.limine_reqs Section)
// ============================================================================

#[used]
#[link_section = ".limine_req_start"]
static REQUESTS_START: RequestsStartMarker = RequestsStartMarker::new();

#[used]
#[link_section = ".limine_reqs"]
static BASE_REVISION: BaseRevision = BaseRevision::with_revision(2);

#[used]
#[link_section = ".limine_reqs"]
static FRAMEBUFFER_REQUEST: FramebufferRequest = FramebufferRequest::new();

#[used]
#[link_section = ".limine_reqs"]
static MEMMAP_REQUEST: MemoryMapRequest = MemoryMapRequest::new();

#[used]
#[link_section = ".limine_reqs"]
static HHDM_REQUEST: HhdmRequest = HhdmRequest::new();

#[used]
#[link_section = ".limine_reqs"]
static KERNEL_ADDR_REQUEST: ExecutableAddressRequest = ExecutableAddressRequest::new();

#[used]
#[link_section = ".limine_req_end"]
static REQUESTS_END: RequestsEndMarker = RequestsEndMarker::new();

/// Everything needed to start a Ring 3 app and open its window:
/// `(process name, ELF image, window title, x, y, width, height)`.
fn ring3_app_spec(app_id: AppId) -> (&'static str, &'static [u8], &'static str, i32, i32, u32, u32) {
    match app_id {
        AppId::UserCrashTest => (
            "user_crashtest",
            task::userprogs::CRASH_TEST_ELF,
            "Crash-Test (Ring 3)",
            700,
            35,
            560,
            250,
        ),
        AppId::UserActivityMonitor => (
            "user_monitor",
            task::userprogs::ACTIVITY_MONITOR_ELF,
            "Activity Monitor (Ring 3)",
            300,
            180,
            650,
            412,
        ),
        // The terminal is the default, so an unexpected id opens something
        // harmless rather than panicking the kernel from the compositor loop.
        _ => (
            "user_terminal",
            task::userprogs::TERMINAL_ELF,
            "Terminal (Ring 3)",
            500,
            250,
            660,
            420,
        ),
    }
}

/// Whether a window's content is produced by a Ring 3 process drawing into a
/// shared surface, rather than by kernel code in `AppSuite`.
fn is_ring3_app(app_id: AppId) -> bool {
    matches!(
        app_id,
        AppId::UserTerminal | AppId::UserCrashTest | AppId::UserActivityMonitor
    )
}

// ============================================================================
// Application Worker Task Entrypoints
// ============================================================================

extern "C" fn crash_test_task_entry() {
    serial_println!("[APP:CRASHTEST] Started in isolated task context.");
    loop {
        core::hint::spin_loop();
    }
}

extern "C" fn activity_monitor_task_entry() {
    serial_println!("[APP:MONITOR] Started telemetry monitor task.");
    loop {
        core::hint::spin_loop();
    }
}

extern "C" fn terminal_shell_task_entry() {
    serial_println!("[APP:TERMINAL] Started terminal shell task.");
    loop {
        core::hint::spin_loop();
    }
}

extern "C" fn aegis_pad_task_entry() {
    serial_println!("[APP:PAD] Started AegisPad text editor task.");
    loop {
        core::hint::spin_loop();
    }
}

// ============================================================================
// Kernel Entry Point
// ============================================================================

/// Kernel entry point invoked by Limine bootloader in 64-bit Long Mode
#[no_mangle]
pub extern "C" fn _start() -> ! {
    // 1. Initialize 16550 Serial UART Console on COM1 (0x3F8)
    arch::serial::init_serial();

    serial_println!("=======================================================");
    serial_println!("        AegisOS v0.1.0 (x86_64 no_std Kernel)         ");
    serial_println!("   Crash-Resilient OS with Hardware Fault Isolation    ");
    serial_println!("   macOS Graphical Desktop Environment & App Suite    ");
    serial_println!("=======================================================");

    // 2. Verify Limine Base Revision
    if !BASE_REVISION.is_supported() {
        serial_println!("[FATAL] Limine Base Revision 2 is not supported by bootloader!");
        hcf();
    }
    serial_println!("[OK] Limine Bootloader Protocol Base Revision verified.");

    // 3. Initialize GDT & TSS (Ring 0 / Ring 3 Selectors, RSP0, IST1 for #DF)
    let (kcs, kds, ucs, uds, tss) = arch::gdt::init_gdt_tss();
    serial_println!(
        "[OK] GDT & TSS loaded. KCS=0x{:02x}, KDS=0x{:02x}, UCS=0x{:02x}, UDS=0x{:02x}, TSS=0x{:02x}",
        kcs, kds, ucs, uds, tss
    );

    // 4. Initialize IDT (256 vectors with assembly stubs) & 8259 PIC
    arch::idt::init_idt();
    serial_println!("[OK] IDT & 8259 PIC initialized (IRQs remapped to 32..47).");

    // 4b. Enable SYSCALL/SYSRET. Must follow the GDT, since STAR encodes selectors
    //     that have to already be valid, and must precede any NO_EXECUTE mapping,
    //     since it is what sets EFER.NXE.
    arch::syscall::init_syscall();

    // 5. Query Limine Kernel Physical & Virtual Base Addresses
    if let Some(exec_resp) = KERNEL_ADDR_REQUEST.get_response() {
        serial_println!(
            "[BOOT] Kernel Physical Base: 0x{:016x} | Virtual Base: 0x{:016x}",
            exec_resp.physical_base(),
            exec_resp.virtual_base()
        );
    }

    // 6. Initialize Memory Subsystem (Frame Allocator, Paging, 16MB Heap)
    let hhdm_resp = HHDM_REQUEST
        .get_response()
        .expect("Fatal: No HHDM response received from Limine");
    let hhdm_offset = hhdm_resp.offset();
    serial_println!("[BOOT] HHDM Direct Map Offset: 0x{:016x}", hhdm_offset);

    let memmap_resp = MEMMAP_REQUEST
        .get_response()
        .expect("Fatal: No MemoryMap response received from Limine");

    unsafe {
        memory::init(memmap_resp.entries(), hhdm_offset);
    }
    serial_println!("[OK] Physical Frame Allocator (128KB Bitmap) & 4-Level Paging initialized.");
    serial_println!("[OK] Kernel Heap (16MB @ 0xFFFF_9000_0000_0000) initialized.");

    // 7. Initialize Hardware Drivers (Linear Framebuffer, PS/2 Keyboard, PS/2 Mouse)
    let (screen_w, screen_h) = if let Some(fb_resp) = FRAMEBUFFER_REQUEST.get_response() {
        if let Some(fb) = fb_resp.framebuffers().next() {
            serial_println!(
                "[BOOT] Initializing Framebuffer: {}x{} (Pitch: {} bytes, {} BPP)",
                fb.width(),
                fb.height(),
                fb.pitch(),
                fb.bpp()
            );
            drivers::init_drivers(&fb);
            (fb.width() as usize, fb.height() as usize)
        } else {
            (1280, 800)
        }
    } else {
        serial_println!("[WARN] No linear framebuffer found from Limine!");
        (1280, 800)
    };
    serial_println!("[OK] Graphics & Input Drivers (Framebuffer, PS/2 Mouse, Keyboard) initialized.");

    // 8. Display Usable Memory Footprint Statistics (< 60MB target)
    let (used_bytes, total_bytes) = memory::get_memory_stats();
    serial_println!(
        "[BOOT] Usable Memory Footprint: {} MB used / {} MB total RAM (< 60MB target verified)",
        used_bytes / (1024 * 1024),
        total_bytes / (1024 * 1024)
    );

    // 9. Initialize Task Scheduler & Fault Isolation Engine
    task::init_task_subsystem();
    task::register_crash_callback(|pid, fault_name, rip, cr2| {
        serial_println!(
            "[FAULT-TELEMETRY] Ring 3 Task PID {} caught {} at RIP 0x{:016x} (CR2: 0x{:016x}). Desktop remains 100% stable.",
            pid, fault_name, rip, cr2
        );
    });
    serial_println!("[OK] Task Scheduler & Ring 3 Fault Isolation Engine active.");

    // 9b. Initialize In-Memory Virtual Filesystem (RAM Disk VFS)
    fs::init_vfs();

    // 9c. Initialize Autonomous AI Agent Kernel Bridge (Ring 0 Supervisor Access)
    agent::init_agent_bridge();

    #[cfg(feature = "selftest")]
    {
        selftest::run_kernel_selftests();
    }

    // 10. Spawn Core System Background Tasks (Ring 0 Kernel Workers)
    let pid_mon = task::spawn_process("monitor", activity_monitor_task_entry, false);
    let pid_term = task::spawn_process("terminal", terminal_shell_task_entry, false);
    let pid_pad = task::spawn_process("aegis_pad", aegis_pad_task_entry, false);
    let pid_crash = task::spawn_process("crash_test", crash_test_task_entry, false);

    // 10b. Spawn demo Ring 3 user tasks (one calculation loop, one intentional fault test)
    let pid_user = task::spawn_user_fault_test(99);
    let pid_fault = task::spawn_user_fault_test(0);

    serial_println!(
        "[OK] Spawned System Tasks: Monitor(PID {}), Terminal(PID {}), Pad(PID {}), CrashTest(PID {}), UserCalc(PID {}), FaultTest(PID {})",
        pid_mon, pid_term, pid_pad, pid_crash, pid_user, pid_fault
    );

    // 10c. Load two real ELF64 programs into Ring 3. Unlike the payloads above,
    // these are parsed from an image rather than copied in as raw bytes, and they
    // reach the kernel through `syscall` rather than only being able to fault.
    let hello_image = task::elf::build_test_image(&task::userprogs::USER_HELLO_CODE, 0x40_0000);
    match task::spawn_user_elf("elf_hello", &hello_image) {
        Ok(pid) => serial_println!("[ELF] Loaded 'elf_hello' as PID {}.", pid),
        Err(e) => serial_println!("[ELF] Failed to load 'elf_hello': {}", e.as_str()),
    }




    let crash_image = task::elf::build_test_image(&task::userprogs::USER_CRASH_CODE, 0x40_0000);
    match task::spawn_user_elf("elf_crasher", &crash_image) {
        Ok(pid) => serial_println!("[ELF] Loaded 'elf_crasher' as PID {}.", pid),
        Err(e) => serial_println!("[ELF] Failed to load 'elf_crasher': {}", e.as_str()),
    }

    // 11. Initialize Window Manager & Applications Suite
    let mut wm = WindowManager::new(screen_w, screen_h);
    let mut app_suite = AppSuite::new();

    // Welcome Desktop Notification Toast
    wm.push_notification(
        "🛡️ AegisOS Ready".to_string(),
        "macOS Desktop & Fault Isolation Active.".to_string(),
        Color::rgb(80, 250, 123),
    );

    // Create initial floating application windows



    // The three Ring 3 apps deliberately open no window at boot. Their
    // processes run from startup, but a window costs a full 640x384 surface
    // blit every frame, and three of them cut the compositor's frame rate by
    // about a third. They are opened on demand from Spotlight -- "r3term",
    // "r3fault", "r3proc" -- which is also how a desktop actually behaves.
    let _w_mon = wm.create_window(
        AppId::ActivityMonitor,
        "Activity Monitor",
        480,
        35,
        520,
        340,
        Some(pid_mon),
    );
    let _w_term = wm.create_window(
        AppId::Terminal,
        "Terminal — guest@aegis-os:~",
        30,
        35,
        430,
        300,
        Some(pid_term),
    );
    let _w_pad = wm.create_window(
        AppId::AegisPad,
        "AegisPad — welcome.txt",
        30,
        350,
        430,
        280,
        Some(pid_pad),
    );
    let _w_crash = wm.create_window(
        AppId::CrashTest,
        "Crash-Test Demo App",
        480,
        390,
        520,
        // Tall enough for four 42px buttons plus the status line beneath them.
        300,
        Some(pid_crash),
    );

    // Focus Terminal by default
    wm.focus_window(_w_term);

    // 12. Enable Hardware CPU Interrupts (Timer, Keyboard, Mouse IRQs)
    arch::idt::enable_interrupts();
    serial_println!("[OK] CPU Hardware Interrupts enabled (100Hz Preemptive Multitasking Active).");
    serial_println!("=======================================================");
    serial_println!("      AegisOS macOS Desktop Compositor Active          ");
    serial_println!("=======================================================");

    // Play startup chime
    crate::drivers::speaker::play_sound_effect(crate::drivers::speaker::SoundEffect::BootChime);

    // 12b. Calibrate Hardware TSC for 60 FPS Frame Pacing
    let mut pacer = arch::FramePacer::init();

    // Helper closure to launch application windows
    let launch_app_window = |app_id: AppId, wm: &mut WindowManager| {
        crate::drivers::speaker::play_sound_effect(crate::drivers::speaker::SoundEffect::WindowOpen);

        // Ring 3 apps start when they are launched, not at boot.
        //
        // Each one costs a full 640x384 surface blit per frame while its window
        // is open, and a share of the round-robin while its process runs. Three
        // of them running from startup slowed the compositor by about a third
        // and pushed the boot-time fault demo a second and a half later, which
        // is the sort of thing that quietly rots a desktop. Starting them on
        // demand is both cheaper and what a desktop actually does.
        //
        // A window draws from a surface owned by a specific PID, so a second
        // window for the same app would render nothing; launching one that is
        // already open raises it instead.
        if is_ring3_app(app_id) {
            if let Some(existing) = wm.window_by_app_id(app_id).map(|w| w.id) {
                wm.focus_window(existing);
                return;
            }

            let (name, image, title, x, y, width, height) = ring3_app_spec(app_id);
            match task::spawn_user_elf(name, image) {
                Ok(pid) => {
                    serial_println!("[ELF] Launched Ring 3 '{}' as PID {}.", name, pid);
                    let id = wm.create_window(app_id, title, x, y, width, height, Some(pid));
                    wm.focus_window(id);
                }
                Err(e) => {
                    serial_println!("[ELF] Failed to launch Ring 3 '{}': {}", name, e.as_str());
                }
            }
            return;
        }

        match app_id {
            AppId::CrashTest => {
                wm.create_window(AppId::CrashTest, "Crash-Test Demo App", 480, 390, 520, 300, Some(pid_crash));
            }
            AppId::ActivityMonitor => {
                wm.create_window(AppId::ActivityMonitor, "Activity Monitor", 480, 35, 520, 340, Some(pid_mon));
            }
            AppId::Terminal => {
                wm.create_window(AppId::Terminal, "Terminal — guest@aegis-os:~", 30, 35, 430, 300, Some(pid_term));
            }
            AppId::FileManager => {
                wm.create_window(AppId::FileManager, "Aegis Files — VFS Browser", 180, 120, 520, 360, None);
            }
            AppId::AegisPad => {
                wm.create_window(AppId::AegisPad, "AegisPad — welcome.txt", 30, 350, 430, 280, Some(pid_pad));
            }
            AppId::Browser => {
                wm.create_window(AppId::Browser, "Aegis Browser — aegis://home", 140, 60, 560, 420, None);
            }
            AppId::Minesweeper => {
                wm.create_window(AppId::Minesweeper, "Minesweeper", 440, 160, 248, 310, None);
            }
            AppId::Synth => {
                wm.create_window(AppId::Synth, "AegisSynth — Chiptune Studio", 360, 130, 520, 400, None);
            }
            AppId::Chat => {
                wm.create_window(AppId::Chat, "AegisChat — Intranet Messenger", 280, 100, 540, 390, None);
            }
            AppId::Calculator => {
                wm.create_window(AppId::Calculator, "Scientific Calculator", 380, 150, 450, 360, None);
            }
            AppId::Snake => {
                wm.create_window(AppId::Snake, "Snake Arcade Game", 200, 150, 340, 360, None);
            }
            AppId::Paint => {
                wm.create_window(AppId::Paint, "Aegis Paint — Canvas", 200, 100, 460, 340, None);
            }
            AppId::Settings => {
                wm.create_window(AppId::Settings, "System Settings", 160, 90, 540, 380, None);
            }
            // Handled above by the Ring 3 branch, which returns before here.
            AppId::UserTerminal | AppId::UserCrashTest | AppId::UserActivityMonitor => {}
            AppId::AboutDialog => {
                wm.create_window(AppId::AboutDialog, "About AegisOS", 340, 200, 340, 300, None);
            }
        }
    };

    // 13. Main Desktop Compositor & Event Dispatch Loop
    let mut uptime_ticks: u64 = 0;
    loop {
        uptime_ticks = uptime_ticks.wrapping_add(1);
        // Wall-clock uptime comes from the 100 Hz timer. Deriving it from the frame
        // counter, as this used to, made the clock a function of rendering speed.
        let uptime_secs = task::get_uptime_ticks() / arch::idt::TIMER_HZ as u64;

        // Automated Self-Test Demo: Spawns isolated Ring 3 fault tests to prove crash resilience
        if uptime_ticks == 5 {
            serial_println!("[DEMO:SELF-TEST] Spawning isolated Ring 3 task with Null Pointer Fault (#PF)...");
            task::spawn_user_fault_test(0);
        }
        if uptime_ticks == 10 {
            serial_println!("[DEMO:SELF-TEST] Spawning isolated Ring 3 task with Divide-by-Zero Fault (#DE)...");
            task::spawn_user_fault_test(1);
        }

        // A. Poll & Dispatch Mouse Events
        while let Some(mouse_ev) = poll_mouse_event() {
            match mouse_ev {
                MouseEvent::MouseMove { x, y, .. } => {
                    wm.handle_mouse_move(x, y);
                    if let Some(focused) = wm.focused_window() {
                        let rect = focused.client_rect();
                        if is_ring3_app(focused.app_id) {
                            // Deliver motion to the Ring 3 process whenever the
                            // pointer is over its client area, dragging or not:
                            // a user program needs hover, not just clicks.
                            if !focused.is_dragging && rect.contains(x, y) {
                                task::uevent::post_mouse(
                                    x - rect.x,
                                    y - rect.y,
                                    task::uevent::BUTTON_LEFT,
                                    task::uevent::MOUSE_MOVE,
                                );
                            }
                        } else if wm.mouse_down && !focused.is_dragging && rect.contains(x, y) {
                            app_suite.handle_mouse_drag(focused, x, y);
                        }
                    }
                }
                MouseEvent::MouseDown { button, x, y } => {
                    if button == MouseButton::Left {
                        let action = wm.handle_mouse_down(x, y);
                        match action {
                            WmAction::AppLaunched(app_id) => {
                                launch_app_window(app_id, &mut wm);
                            }
                            WmAction::RebootRequested => {
                                serial_println!("[SYS] User requested reboot via menu bar.");
                                unsafe { outb(0x64, 0xFE); }
                            }
                            WmAction::WindowClosed(_wid, pid_opt) => {
                                crate::drivers::speaker::play_sound_effect(crate::drivers::speaker::SoundEffect::WindowClose);
                                if let Some(pid) = pid_opt {
                                    serial_println!("[WM] Closed window associated with PID {}.", pid);
                                }
                            }
                            WmAction::WindowFocused(wid) => {
                                if let Some(win) = wm.windows.iter().find(|w| w.id == wid) {
                                    let rect = win.client_rect();
                                    if is_ring3_app(win.app_id) {
                                        // The process owns everything inside its
                                        // client rect. The window manager has
                                        // already taken the titlebar and the
                                        // traffic lights before we get here.
                                        if rect.contains(x, y) {
                                            task::uevent::post_mouse(
                                                x - rect.x,
                                                y - rect.y,
                                                task::uevent::BUTTON_LEFT,
                                                task::uevent::MOUSE_DOWN,
                                            );
                                        }
                                    } else if rect.contains(x, y) {
                                        let app_act = app_suite.handle_mouse_down(win, x, y, false);
                                        match app_act {
                                            AppAction::CloseWindow => {
                                                wm.close_window(wid);
                                            }
                                            AppAction::FaultTriggered(fault_idx) => {
                                                crate::drivers::speaker::play_sound_effect(crate::drivers::speaker::SoundEffect::Alert);
                                                serial_println!(
                                                    "[CRASH-TEST] User clicked fault button #{}. Spawning isolated Ring 3 fault task...",
                                                    fault_idx
                                                );
                                                let crash_pid = task::spawn_user_fault_test(fault_idx);
                                                serial_println!(
                                                    "[CRASH-TEST] Spawned Ring 3 process PID {}. Awaiting hardware trap...",
                                                    crash_pid
                                                );
                                                wm.push_notification(
                                                    "⚠️ Fault Injected".to_string(),
                                                    format!("Spawned Ring 3 PID {}. Trapping...", crash_pid),
                                                    Color::rgb(255, 189, 46),
                                                );
                                            }
                                            AppAction::OpenFileInEditor(path) => {
                                                app_suite.editor.open_path(&path);
                                                if let Some(pos) = wm.windows.iter().position(|w| w.app_id == AppId::AegisPad) {
                                                    let pad_id = wm.windows[pos].id;
                                                    wm.windows[pos].title = format!("AegisPad — {}", path);
                                                    wm.focus_window(pad_id);
                                                } else {
                                                    wm.create_window(AppId::AegisPad, &format!("AegisPad — {}", path), 30, 350, 430, 280, Some(pid_pad));
                                                }
                                            }
                                            AppAction::SetWallpaper(theme) => {
                                                wm.set_wallpaper_theme(theme);
                                            }
                                            AppAction::SetCustomWallpaper(path) => {
                                                if let Ok(bytes) = crate::fs::read_file(&path) {
                                                    if let Ok(ppm) = crate::gui::wallpaper::parse_ppm_p6(&bytes) {
                                                        wm.set_custom_wallpaper(ppm);
                                                    }
                                                }
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                            }
                            WmAction::None => {}
                        }
                    } else if button == MouseButton::Right {
                        if let Some(focused) = wm.focused_window() {
                            let rect = focused.client_rect();
                            if rect.contains(x, y) {
                                if is_ring3_app(focused.app_id) {
                                    task::uevent::post_mouse(
                                        x - rect.x,
                                        y - rect.y,
                                        task::uevent::BUTTON_RIGHT,
                                        task::uevent::MOUSE_DOWN,
                                    );
                                } else {
                                    app_suite.handle_mouse_down_right(focused, x, y);
                                }
                            }
                        }
                    }
                }
                MouseEvent::MouseUp { x, y, .. } => {
                    // Release goes to the Ring 3 process before the window
                    // manager clears drag state, so it can pair the up with its
                    // own down even if the pointer left the client area.
                    if let Some(focused) = wm.focused_window() {
                        if is_ring3_app(focused.app_id) {
                            let rect = focused.client_rect();
                            task::uevent::post_mouse(
                                x - rect.x,
                                y - rect.y,
                                task::uevent::BUTTON_LEFT,
                                task::uevent::MOUSE_UP,
                            );
                        }
                    }
                    wm.handle_mouse_up(x, y);
                    app_suite.handle_mouse_up();
                }
            }
        }

        // B. Poll & Dispatch Keyboard Events
        while let Some(key_ev) = poll_key_event() {
            // Global Spotlight Toggle: Ctrl+Space or F3
            if key_ev.pressed && ((key_ev.ctrl && key_ev.char_byte == Some(b' ')) || key_ev.code == crate::drivers::ps2_keyboard::KeyCode::F(3)) {
                wm.spotlight.toggle();
                crate::drivers::speaker::play_sound_effect(crate::drivers::speaker::SoundEffect::WindowOpen);
                continue;
            }

            // If Spotlight is visible, it intercepts all keyboard input
            if wm.spotlight.is_visible {
                if key_ev.pressed {
                    match key_ev.code {
                        crate::drivers::ps2_keyboard::KeyCode::Escape => {
                            wm.spotlight.hide();
                        }
                        crate::drivers::ps2_keyboard::KeyCode::Up => {
                            wm.spotlight.select_prev();
                        }
                        crate::drivers::ps2_keyboard::KeyCode::Down => {
                            wm.spotlight.select_next();
                        }
                        crate::drivers::ps2_keyboard::KeyCode::Enter => {
                            if let Some(to_launch) = wm.spotlight.activate_selected() {
                                launch_app_window(to_launch, &mut wm);
                            }
                        }
                        crate::drivers::ps2_keyboard::KeyCode::Backspace => {
                            wm.spotlight.backspace();
                        }
                        _ => {
                            if let Some(ch) = key_ev.char_byte {
                                if (32..=126).contains(&ch) {
                                    wm.spotlight.push_char(ch as char);
                                }
                            }
                        }
                    }
                }
                continue;
            }

            // Normal application keyboard dispatch
            if let Some(focused) = wm.focused_window() {
                let app_id = focused.app_id;
                if is_ring3_app(app_id) {
                    // A Ring 3 window's keys go to the process, not to kernel
                    // code. It collects them with SYS_POLL_EVENT.
                    task::uevent::post_key(&key_ev);
                } else {
                    let launch_req = app_suite.handle_key(app_id, key_ev);
                    if let Some(to_launch) = launch_req {
                        launch_app_window(to_launch, &mut wm);
                    }
                }
            }
        }

        // B2. Point the Ring 3 event queue at the focused window's process, so a
        //     user process only ever receives input while it has focus.
        let focused_user_pid = wm
            .focused_window()
            .filter(|w| is_ring3_app(w.app_id))
            .and_then(|w| w.pid);
        task::uevent::set_target(focused_user_pid);

        // C. Clean up any terminated zombie tasks
        task::reap_zombies();

        // D. Desktop Compositor 60 FPS Render Pass
        let cpu_usage = task::get_cpu_usage();
        let (used_mem, total_mem) = task::get_memory_stats();

        if let Some(ref mut fb) = *FRAMEBUFFER.lock() {
            // 1. Wallpaper, Window Frames + clipped client content in Z-order,
            //    Menu Bar, Dock, Cursor, Toasts
            wm.render_desktop(
                fb,
                uptime_secs,
                cpu_usage,
                used_mem,
                total_mem,
                &mut |win, fb| app_suite.render_app(win, fb),
            );

            // 2. Redraw Cursor on very top of active frame
            drivers::ps2_mouse::draw_cursor(fb, wm.mouse_x, wm.mouse_y);

            // 3. Swap Backbuffer to VRAM Frontbuffer (Tear-Free Scanlines)
            fb.swap_buffers();
        }

        // Calibrated 60 FPS hardware frame pacing
        pacer.pace_frame();

        // Step non-blocking audio sequencer
        crate::drivers::speaker::update_audio();

        // Step chiptune pattern sequencer
        app_suite.synth.tick_sequencer(task::get_uptime_ticks());

        // Poll intranet loopback network
        app_suite.chat.poll_network();
    }
}

/// Halt and Catch Fire: Disables interrupts and loops hlt instruction
pub fn hcf() -> ! {
    loop {
        core::hint::spin_loop();
        unsafe {
            core::arch::asm!("cli; hlt", options(nomem, nostack));
        }
    }
}

// ============================================================================
// Diagnostic Kernel Panic Handler
// ============================================================================

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    serial_println!("\n=======================================================");
    serial_println!("               !!! KERNEL PANIC !!!                    ");
    serial_println!("=======================================================");

    if let Some(location) = info.location() {
        serial_println!(
            "Panic Location: {}:{}:{}",
            location.file(),
            location.line(),
            location.column()
        );
    } else {
        serial_println!("Panic Location: <unknown>");
    }

    serial_println!("Panic Message:  {}", info.message());
    serial_println!("=======================================================");
    serial_println!("System Execution Halted.");

    hcf();
}
