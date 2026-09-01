//! AegisOS E2E Test Harness: Integrated Kernel Simulation Environment
//!
//! Integrates physical memory management, paging, privilege architecture,
//! preemptive scheduler, double-buffered compositor, input pipeline, window
//! manager, and 5 applications into a single testable system environment.

use super::types::*;
use super::memory_sim::*;
use super::privilege_sim::*;
use super::scheduler_sim::*;
use super::gui_sim::*;
use super::input_sim::*;
use super::wm_sim::*;
use super::apps_sim::*;

pub struct AegisOsKernelEnv {
    pub frame_alloc: FrameAllocSimulator,
    pub kernel_pml4: Pml4Simulator,
    pub tss: TssSimulator,
    pub idt: IdtSimulator,
    pub uart: UartSerialSimulator,
    pub scheduler: SchedulerSimulator,
    pub fb: FramebufferSimulator,
    pub keyboard: KeyboardSimulator,
    pub mouse: MouseSimulator,
    pub wm: WindowManagerSimulator,

    // App instances
    pub crash_test_app: Option<CrashTestAppSimulator>,
    pub activity_monitor_app: Option<ActivityMonitorAppSimulator>,
    pub terminal_app: Option<TerminalShellAppSimulator>,
    pub pad_app: Option<AegisPadSimulator>,
    pub about_app: Option<AboutDialogSimulator>,

    pub kernel_booted: bool,
    pub kernel_panic_triggered: bool,
}

impl AegisOsKernelEnv {
    pub fn new() -> Self {
        let mut frame_alloc = FrameAllocSimulator::new_4gb();
        let pml4_frame = frame_alloc.alloc_frame().unwrap();
        let kernel_pml4 = Pml4Simulator::new(pml4_frame);
        let mut tss = TssSimulator::new();
        tss.set_rsp0(0xFFFF_FFFF_8008_0000);
        tss.set_ist(1, 0xFFFF_FFFF_8009_0000);

        let mut idt = IdtSimulator::new();
        for vec in 0..=31 {
            idt.set_handler(vec, 0xFFFF_FFFF_8010_0000 + (vec as u64 * 0x20), 0, if vec == 8 { 1 } else { 0 });
        }

        let mut uart = UartSerialSimulator::new();
        uart.write_str("[BOOT] AegisOS Kernel Initializing...\n");
        uart.write_str("[BOOT] GDT, TSS, IDT Privilege Separation Configured (Ring 0 / Ring 3)\n");
        uart.write_str("[BOOT] Physical Memory Frame Allocator: 4096 MB RAM Online\n");

        let mut scheduler = SchedulerSimulator::new();
        // Spawn Desktop Compositor Task (PID 1, Ring 0 kernel task)
        let desktop_frames = vec![frame_alloc.alloc_frame().unwrap(), frame_alloc.alloc_frame().unwrap()];
        scheduler.spawn_process("kernel_desktop", false, Priority::High, pml4_frame, desktop_frames);

        let fb = FramebufferSimulator::default_1024x768();
        let keyboard = KeyboardSimulator::new();
        let mouse = MouseSimulator::new(SCREEN_WIDTH, SCREEN_HEIGHT);
        let wm = WindowManagerSimulator::new(SCREEN_WIDTH, SCREEN_HEIGHT);

        Self {
            frame_alloc,
            kernel_pml4,
            tss,
            idt,
            uart,
            scheduler,
            fb,
            keyboard,
            mouse,
            wm,
            crash_test_app: None,
            activity_monitor_app: None,
            terminal_app: None,
            pad_app: None,
            about_app: None,
            kernel_booted: true,
            kernel_panic_triggered: false,
        }
    }

    pub fn launch_app(&mut self, app_id: AppId) -> (u32 /* window_id */, ProcessId) {
        let frame1 = self.frame_alloc.alloc_frame().unwrap();
        let frame2 = self.frame_alloc.alloc_frame().unwrap();
        let frame3 = self.frame_alloc.alloc_frame().unwrap();
        let user_pml4_frame = self.frame_alloc.alloc_frame().unwrap();

        match app_id {
            AppId::CrashTest => {
                let pid = self.scheduler.spawn_process(
                    "crashtest",
                    true,
                    Priority::Normal,
                    user_pml4_frame,
                    vec![frame1, frame2, frame3, user_pml4_frame],
                );
                self.crash_test_app = Some(CrashTestAppSimulator::new(pid));
                let wid = self.wm.create_window(app_id, "Crash-Test Demo", 60, 60, 480, 320, Some(pid));
                (wid, pid)
            }
            AppId::ActivityMonitor => {
                let pid = self.scheduler.spawn_process(
                    "activity_monitor",
                    true,
                    Priority::Normal,
                    user_pml4_frame,
                    vec![frame1, frame2, frame3, user_pml4_frame],
                );
                self.activity_monitor_app = Some(ActivityMonitorAppSimulator::new(pid));
                let wid = self.wm.create_window(app_id, "Activity Monitor", 200, 100, 620, 400, Some(pid));
                (wid, pid)
            }
            AppId::Terminal => {
                let pid = self.scheduler.spawn_process(
                    "terminal_shell",
                    true,
                    Priority::Normal,
                    user_pml4_frame,
                    vec![frame1, frame2, frame3, user_pml4_frame],
                );
                self.terminal_app = Some(TerminalShellAppSimulator::new(pid));
                let wid = self.wm.create_window(app_id, "Terminal", 150, 150, 560, 360, Some(pid));
                (wid, pid)
            }
            AppId::AegisPad => {
                let pid = self.scheduler.spawn_process(
                    "aegis_pad",
                    true,
                    Priority::Normal,
                    user_pml4_frame,
                    vec![frame1, frame2, frame3, user_pml4_frame],
                );
                self.pad_app = Some(AegisPadSimulator::new(pid));
                let wid = self.wm.create_window(app_id, "AegisPad", 250, 80, 520, 380, Some(pid));
                (wid, pid)
            }
            AppId::AboutDialog => {
                let pid = self.scheduler.spawn_process(
                    "about_aegis",
                    true,
                    Priority::Normal,
                    user_pml4_frame,
                    vec![frame1, frame2, user_pml4_frame],
                );
                self.about_app = Some(AboutDialogSimulator::new());
                let wid = self.wm.create_window(app_id, "About AegisOS", 340, 200, 340, 240, Some(pid));
                (wid, pid)
            }
        }
    }

    pub fn trigger_user_fault(
        &mut self,
        pid: ProcessId,
        vector: ExceptionVector,
        rip: u64,
        cr2: u64,
    ) -> Result<(), &'static str> {
        self.uart.write_str(&format!(
            "[FAULT] Ring 3 Exception {:?} in PID {} at RIP {:#018x}\n",
            vector, pid, rip
        ));
        if vector == ExceptionVector::PageFault {
            self.uart.write_str(&format!("[FAULT] Fault Address (CR2): {:#018x}\n", cr2));
        }

        let res = self.scheduler.handle_fault(pid, vector, rip, cr2);
        if res.is_ok() {
            self.uart.write_str(&format!(
                "[KERNEL] Terminating faulting task PID {}. Queueing deferred reclamation.\n",
                pid
            ));
            // Window manager closes window associated with faulted PID
            self.wm.close_window_by_pid(pid);
        }
        res
    }

    pub fn timer_tick(&mut self) -> Option<ProcessId> {
        let active_pid = self.scheduler.timer_tick(&mut self.frame_alloc);
        self.wm.uptime_seconds += 1;
        if let Some(monitor) = &mut self.activity_monitor_app {
            monitor.update_telemetry(self.scheduler.get_cpu_usage());
        }
        active_pid
    }

    pub fn render_desktop(&mut self) -> usize {
        let cpu_usage = self.scheduler.get_cpu_usage();
        let ram_used = self.frame_alloc.used_bytes();
        self.wm.render(
            &mut self.fb,
            cpu_usage,
            ram_used,
            self.mouse.cursor_x,
            self.mouse.cursor_y,
        );
        self.fb.swap_buffers()
    }

    pub fn send_key_scancode(&mut self, scancode: u8) -> Option<InputEvent> {
        let ev = self.keyboard.handle_scancode(scancode);
        if let Some(InputEvent::KeyDown { key, .. }) = &ev {
            if let Some(focused) = self.wm.focused_window() {
                match focused.app_id {
                    AppId::Terminal => {
                        if let Some(term) = &mut self.terminal_app {
                            term.handle_key_input(*key, &mut self.scheduler, &self.frame_alloc);
                        }
                    }
                    AppId::AegisPad => {
                        if let Some(pad) = &mut self.pad_app {
                            pad.handle_key(*key);
                        }
                    }
                    _ => {}
                }
            }
        }
        ev
    }

    pub fn send_mouse_packet(&mut self, bytes: [u8; 3]) -> Result<Vec<InputEvent>, &'static str> {
        let events = self.mouse.handle_packet(bytes)?;
        for ev in &events {
            match ev {
                InputEvent::MouseMove { x, y, .. } => {
                    self.wm.handle_mouse_move(*x, *y);
                }
                InputEvent::MouseDown { button: MouseButton::Left, x, y } => {
                    self.wm.handle_mouse_down(*x, *y);
                }
                InputEvent::MouseUp { button: MouseButton::Left, .. } => {
                    self.wm.handle_mouse_up();
                }
                _ => {}
            }
        }
        Ok(events)
    }

    pub fn get_memory_stats(&self) -> MemoryStats {
        MemoryStats {
            total_bytes: TOTAL_RAM_4GB,
            used_bytes: self.frame_alloc.used_bytes(),
            free_bytes: TOTAL_RAM_4GB.saturating_sub(self.frame_alloc.used_bytes()),
            allocated_frames: self.frame_alloc.allocated_count(),
            total_frames: self.frame_alloc.total_frames(),
            heap_used_bytes: 2 * 1024 * 1024,
            heap_total_bytes: 16 * 1024 * 1024,
        }
    }
}
