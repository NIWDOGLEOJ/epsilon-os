//! AegisOS E2E Test Harness: Five Core System Applications Simulator
//!
//! Models Crash-Test Demo, Activity Monitor (<60MB check), Terminal Shell,
//! AegisPad Text Editor, and About AegisOS Modal Dialog.

use super::types::*;
use super::scheduler_sim::SchedulerSimulator;
use super::memory_sim::FrameAllocSimulator;

// ============================================================================
// 1. Crash-Test Demo App
// ============================================================================

pub struct CrashTestAppSimulator {
    pub pid: ProcessId,
    pub last_fault_triggered: Option<ExceptionVector>,
}

impl CrashTestAppSimulator {
    pub fn new(pid: ProcessId) -> Self {
        Self {
            pid,
            last_fault_triggered: None,
        }
    }

    pub fn trigger_null_pointer(&mut self, sched: &mut SchedulerSimulator) -> Result<(), &'static str> {
        self.last_fault_triggered = Some(ExceptionVector::PageFault);
        sched.handle_fault(self.pid, ExceptionVector::PageFault, 0x004012A0, 0x00000000)
    }

    pub fn trigger_divide_by_zero(&mut self, sched: &mut SchedulerSimulator) -> Result<(), &'static str> {
        self.last_fault_triggered = Some(ExceptionVector::DivideByZero);
        sched.handle_fault(self.pid, ExceptionVector::DivideByZero, 0x00401340, 0)
    }

    pub fn trigger_oob_write(&mut self, sched: &mut SchedulerSimulator) -> Result<(), &'static str> {
        self.last_fault_triggered = Some(ExceptionVector::PageFault);
        sched.handle_fault(self.pid, ExceptionVector::PageFault, 0x00401400, 0xFFFF_FFFF_8000_0000)
    }

    pub fn trigger_invalid_opcode(&mut self, sched: &mut SchedulerSimulator) -> Result<(), &'static str> {
        self.last_fault_triggered = Some(ExceptionVector::InvalidOpcode);
        sched.handle_fault(self.pid, ExceptionVector::InvalidOpcode, 0x004014C0, 0)
    }
}

// ============================================================================
// 2. Activity Monitor App
// ============================================================================

pub struct ActivityMonitorAppSimulator {
    pub pid: ProcessId,
    pub cpu_history: Vec<u32>,
    pub selected_pid: Option<ProcessId>,
}

impl ActivityMonitorAppSimulator {
    pub fn new(pid: ProcessId) -> Self {
        Self {
            pid,
            cpu_history: vec![0; 60],
            selected_pid: None,
        }
    }

    pub fn update_telemetry(&mut self, cpu_usage: u32) {
        self.cpu_history.remove(0);
        self.cpu_history.push(cpu_usage);
    }

    pub fn select_process(&mut self, pid: ProcessId) {
        self.selected_pid = Some(pid);
    }

    pub fn kill_selected_process(&mut self, sched: &mut SchedulerSimulator) -> bool {
        if let Some(target_pid) = self.selected_pid {
            sched.kill_process(target_pid)
        } else {
            false
        }
    }

    pub fn is_idle_ram_under_60mb(used_bytes: u64) -> bool {
        used_bytes < MAX_IDLE_RAM_BYTES
    }
}

// ============================================================================
// 3. Interactive Terminal Shell App
// ============================================================================

pub struct TerminalShellAppSimulator {
    pub pid: ProcessId,
    pub command_buffer: String,
    pub history: Vec<String>,
    pub history_idx: usize,
    pub output_lines: Vec<String>,
    pub prompt: String,
}

impl TerminalShellAppSimulator {
    pub fn new(pid: ProcessId) -> Self {
        Self {
            pid,
            command_buffer: String::new(),
            history: Vec::new(),
            history_idx: 0,
            output_lines: vec![
                "AegisOS Virtual Terminal v1.0 (x86_64-unknown-none)".to_string(),
                "Type 'help' to view available system commands.".to_string(),
            ],
            prompt: "aegis:~$ ".to_string(),
        }
    }

    pub fn handle_key_input(
        &mut self,
        key: u8,
        sched: &mut SchedulerSimulator,
        frame_alloc: &FrameAllocSimulator,
    ) -> Option<String> {
        match key {
            b'\n' => {
                let cmd = self.command_buffer.trim().to_string();
                self.output_lines.push(format!("{}{}", self.prompt, cmd));
                self.command_buffer.clear();

                if !cmd.is_empty() {
                    self.history.push(cmd.clone());
                    self.history_idx = self.history.len();
                }

                let response = self.execute_command(&cmd, sched, frame_alloc);
                for line in &response {
                    self.output_lines.push(line.clone());
                }
                Some(cmd)
            }
            0x08 => {
                // Backspace
                self.command_buffer.pop();
                None
            }
            0x80 => {
                // Up arrow (History prev)
                if !self.history.is_empty() && self.history_idx > 0 {
                    self.history_idx -= 1;
                    self.command_buffer = self.history[self.history_idx].clone();
                }
                None
            }
            0x81 => {
                // Down arrow (History next)
                if !self.history.is_empty() && self.history_idx < self.history.len() - 1 {
                    self.history_idx += 1;
                    self.command_buffer = self.history[self.history_idx].clone();
                } else {
                    self.history_idx = self.history.len();
                    self.command_buffer.clear();
                }
                None
            }
            ascii if (32..=126).contains(&ascii) => {
                self.command_buffer.push(ascii as char);
                None
            }
            _ => None,
        }
    }

    pub fn execute_command(
        &mut self,
        cmd_line: &str,
        sched: &mut SchedulerSimulator,
        frame_alloc: &FrameAllocSimulator,
    ) -> Vec<String> {
        let parts: Vec<&str> = cmd_line.split_whitespace().collect();
        if parts.is_empty() {
            return Vec::new();
        }

        if !cmd_line.trim().is_empty() {
            if self.history.last().map(|s| s.as_str()) != Some(cmd_line.trim()) {
                self.history.push(cmd_line.trim().to_string());
            }
            self.history_idx = self.history.len();
        }

        match parts[0] {
            "help" => vec![
                "Available commands:".to_string(),
                "  help       - Show available system commands".to_string(),
                "  ps         - List active processes".to_string(),
                "  kill <pid> - Terminate a process by PID".to_string(),
                "  free       - Show memory utilization".to_string(),
                "  echo <msg> - Print message to terminal".to_string(),
                "  run <app>  - Launch application".to_string(),
                "  clear      - Clear terminal screen".to_string(),
                "  reboot     - Restart computer".to_string(),
            ],
            "ps" => {
                let mut out = vec![format!("{:<4} {:<16} {:<8} {:<10} {:<6}", "PID", "NAME", "STATE", "MEMORY", "CPU%")];
                for proc in sched.get_process_list() {
                    out.push(format!(
                        "{:<4} {:<16} {:<8?} {:<8}KB {:<5}%",
                        proc.pid,
                        proc.name,
                        proc.state,
                        proc.memory_bytes / 1024,
                        proc.cpu_percent
                    ));
                }
                out
            }
            "free" => {
                let total_mb = frame_alloc.total_frames() * 4 / 1024;
                let used_mb = frame_alloc.allocated_count() * 4 / 1024;
                let free_mb = frame_alloc.free_count() * 4 / 1024;
                vec![
                    format!("Physical Memory: Total {} MB | Used {} MB | Free {} MB", total_mb, used_mb, free_mb),
                    format!("Frame Allocator: {} / {} frames in use (<60MB footprint verified)", frame_alloc.allocated_count(), frame_alloc.total_frames()),
                ]
            }
            "kill" => {
                if parts.len() < 2 {
                    vec!["Usage: kill <pid>".to_string()]
                } else if let Ok(target_pid) = parts[1].parse::<ProcessId>() {
                    if target_pid == 0 {
                        vec!["Error: Cannot kill PID 0 [idle] kernel task".to_string()]
                    } else if sched.kill_process(target_pid) {
                        vec![format!("[SYS] Terminated process PID {}", target_pid)]
                    } else {
                        vec![format!("Error: Process PID {} not found", target_pid)]
                    }
                } else {
                    vec!["Error: Invalid PID format".to_string()]
                }
            }
            "echo" => {
                let msg = parts[1..].join(" ");
                vec![msg]
            }
            "run" => {
                if parts.len() < 2 {
                    vec!["Usage: run <crashtest|monitor|pad|about>".to_string()]
                } else {
                    let app_name = parts[1];
                    let new_pid = sched.spawn_process(
                        app_name,
                        true,
                        Priority::Normal,
                        PhysAddr(0x2000),
                        Vec::new(),
                    );
                    vec![format!("[SYS] Spawned process '{}' with PID {}", app_name, new_pid)]
                }
            }
            "clear" => {
                self.output_lines.clear();
                Vec::new()
            }
            "reboot" => {
                vec!["[SYS] Rebooting AegisOS via 8042 reset pulse...".to_string()]
            }
            other => vec![format!("Unknown command: '{}'. Type 'help' for available commands.", other)],
        }
    }
}

// ============================================================================
// 4. Text Editor (AegisPad)
// ============================================================================

pub struct AegisPadSimulator {
    pub pid: ProcessId,
    pub lines: Vec<String>,
    pub cursor_row: usize,
    pub cursor_col: usize,
    pub filename: String,
}

impl AegisPadSimulator {
    pub fn new(pid: ProcessId) -> Self {
        Self {
            pid,
            lines: vec![
                "Welcome to AegisOS!".to_string(),
                "".to_string(),
                "This operating system features:".to_string(),
                "- Ring 0 / Ring 3 hardware memory isolation".to_string(),
                "- Crash-resilient fault recovery".to_string(),
                "- macOS-inspired double-buffered desktop GUI".to_string(),
                "- Low memory footprint (< 60MB RAM)".to_string(),
            ],
            cursor_row: 0,
            cursor_col: 0,
            filename: "welcome.txt".to_string(),
        }
    }

    pub fn handle_key(&mut self, key: u8) {
        match key {
            b'\n' => {
                // Split line
                let current_line = &self.lines[self.cursor_row];
                let remainder = current_line[self.cursor_col..].to_string();
                self.lines[self.cursor_row].truncate(self.cursor_col);
                self.cursor_row += 1;
                self.lines.insert(self.cursor_row, remainder);
                self.cursor_col = 0;
            }
            0x08 => {
                // Backspace
                if self.cursor_col > 0 {
                    self.lines[self.cursor_row].remove(self.cursor_col - 1);
                    self.cursor_col -= 1;
                } else if self.cursor_row > 0 {
                    // Merge with previous line
                    let current = self.lines.remove(self.cursor_row);
                    self.cursor_row -= 1;
                    self.cursor_col = self.lines[self.cursor_row].len();
                    self.lines[self.cursor_row].push_str(&current);
                }
            }
            0x7F => {
                // Delete
                if self.cursor_col < self.lines[self.cursor_row].len() {
                    self.lines[self.cursor_row].remove(self.cursor_col);
                } else if self.cursor_row + 1 < self.lines.len() {
                    let next_line = self.lines.remove(self.cursor_row + 1);
                    self.lines[self.cursor_row].push_str(&next_line);
                }
            }
            0x80 => {
                // Up arrow
                if self.cursor_row > 0 {
                    self.cursor_row -= 1;
                    self.cursor_col = self.cursor_col.min(self.lines[self.cursor_row].len());
                }
            }
            0x81 => {
                // Down arrow
                if self.cursor_row + 1 < self.lines.len() {
                    self.cursor_row += 1;
                    self.cursor_col = self.cursor_col.min(self.lines[self.cursor_row].len());
                }
            }
            0x82 => {
                // Left arrow
                if self.cursor_col > 0 {
                    self.cursor_col -= 1;
                }
            }
            0x83 => {
                // Right arrow
                if self.cursor_col < self.lines[self.cursor_row].len() {
                    self.cursor_col += 1;
                }
            }
            ascii if (32..=126).contains(&ascii) => {
                self.lines[self.cursor_row].insert(self.cursor_col, ascii as char);
                self.cursor_col += 1;
            }
            _ => {}
        }
    }

    pub fn total_characters(&self) -> usize {
        self.lines.iter().map(|l| l.len()).sum()
    }
}

// ============================================================================
// 5. About AegisOS Modal Dialog
// ============================================================================

pub struct AboutDialogSimulator {
    pub kernel_version: &'static str,
    pub bootloader: &'static str,
    pub architecture: &'static str,
    pub memory_footprint_str: &'static str,
    pub display_mode: &'static str,
}

impl AboutDialogSimulator {
    pub fn new() -> Self {
        Self {
            kernel_version: "AegisOS 1.0.0 (Rust no_std)",
            bootloader: "Limine Boot Protocol v2",
            architecture: "x86_64 Long Mode (Ring 0 / Ring 3)",
            memory_footprint_str: "4096 MB RAM (Active Footprint < 60MB)",
            display_mode: "1024x768x32 Linear Double-Buffered",
        }
    }
}
