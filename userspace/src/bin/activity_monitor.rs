#![no_std]
#![no_main]

//! AegisOS Activity Monitor, running in Ring 3.
//!
//! The Ring 0 version (`src/apps/activity_monitor.rs`) reads the scheduler and
//! frame allocator directly. This one cannot see either: it asks for a CPU
//! sample, a memory total and the process table through syscalls, and draws the
//! result into a surface the kernel maps for it.
//!
//! It is the first Ring 3 app that needs to *poll* rather than react. The
//! terminal and crash-test only redraw when input arrives; a monitor has to
//! resample on a clock, so it watches the uptime tick counter and refreshes
//! when it crosses a boundary.

#[path = "../rt.rs"]
mod rt;

use aegis_user::font::FONT_WIDTH;
use aegis_user::surface::Surface;
use aegis_user::sys;
use aegis_user::text::{
    push_str, push_u64, COLOR_BG, COLOR_BUTTON, COLOR_BUTTON_DANGER, COLOR_BUTTON_EDGE,
    COLOR_BUTTON_HOVER, COLOR_DIM, COLOR_ERROR, COLOR_FG, COLOR_HEADING, COLOR_PROMPT, COLOR_WARN,
};

/// Samples kept in the CPU history graph, matching the Ring 0 app.
const HISTORY_CAPACITY: usize = 60;

/// Timer ticks between samples. The kernel timer is 100 Hz, so this is 0.5s.
const SAMPLE_INTERVAL_TICKS: u64 = 50;

/// Most processes shown in the table. The kernel reports more than fit; the
/// rest are summarised in the footer rather than silently dropped.
const MAX_ROWS: usize = 9;

const CARD_TOP: usize = 24;
const CARD_H: usize = 122;
const CPU_CARD_X: usize = 8;
const MEM_CARD_X: usize = 328;
const CARD_W: usize = 304;

const TABLE_TOP: usize = 156;
const ROW_H: usize = 16;

const KILL_X: usize = 8;
const KILL_Y: usize = 340;
const KILL_W: usize = 152;
const KILL_H: usize = 28;

/// One row of the process table, parsed from `SYS_PROC_INFO`.
#[derive(Clone, Copy)]
struct Process {
    pid: u64,
    state: [u8; 8],
    state_len: usize,
    cpu: u64,
    mem_kib: u64,
    name: [u8; 24],
    name_len: usize,
}

impl Process {
    const fn blank() -> Self {
        Self { pid: 0, state: [0; 8], state_len: 0, cpu: 0, mem_kib: 0, name: [0; 24], name_len: 0 }
    }
}

/// Splits `"<pid> <state> <cpu> <mem_kib> <name>"`, taking everything after the
/// fourth space as the name so a name containing spaces still parses.
fn parse_process(line: &[u8]) -> Option<Process> {
    let mut fields = [0usize; 4];
    let mut found = 0;
    for (i, &b) in line.iter().enumerate() {
        if b == b' ' {
            fields[found] = i;
            found += 1;
            if found == 4 {
                break;
            }
        }
    }
    if found < 4 {
        return None;
    }

    let mut process = Process::blank();
    process.pid = parse_u64(&line[..fields[0]])?;
    process.cpu = parse_u64(&line[fields[1] + 1..fields[2]])?;
    process.mem_kib = parse_u64(&line[fields[2] + 1..fields[3]])?;

    let state = &line[fields[0] + 1..fields[1]];
    process.state_len = state.len().min(process.state.len());
    process.state[..process.state_len].copy_from_slice(&state[..process.state_len]);

    let name = &line[fields[3] + 1..];
    process.name_len = name.len().min(process.name.len());
    process.name[..process.name_len].copy_from_slice(&name[..process.name_len]);

    Some(process)
}

fn parse_u64(bytes: &[u8]) -> Option<u64> {
    if bytes.is_empty() {
        return None;
    }
    let mut value: u64 = 0;
    for &b in bytes {
        if !b.is_ascii_digit() {
            return None;
        }
        value = value.checked_mul(10)?.checked_add((b - b'0') as u64)?;
    }
    Some(value)
}

struct Monitor {
    cpu_history: [u32; HISTORY_CAPACITY],
    history_len: usize,
    processes: [Process; MAX_ROWS],
    process_len: usize,
    total_processes: u64,
    used_kib: u64,
    total_kib: u64,
    selected: Option<usize>,
    hover_kill: bool,
    status: &'static str,
    status_color: u32,
}

impl Monitor {
    const fn new() -> Self {
        Self {
            cpu_history: [0; HISTORY_CAPACITY],
            history_len: 0,
            processes: [Process::blank(); MAX_ROWS],
            process_len: 0,
            total_processes: 0,
            used_kib: 0,
            total_kib: 0,
            selected: None,
            hover_kill: false,
            status: "Sampling.",
            status_color: COLOR_DIM,
        }
    }

    fn record_cpu(&mut self, sample: u32) {
        let sample = sample.min(100);
        if self.history_len < HISTORY_CAPACITY {
            self.cpu_history[self.history_len] = sample;
            self.history_len += 1;
        } else {
            for i in 0..HISTORY_CAPACITY - 1 {
                self.cpu_history[i] = self.cpu_history[i + 1];
            }
            self.cpu_history[HISTORY_CAPACITY - 1] = sample;
        }
    }

    /// Re-reads everything the kernel will tell us about the system.
    fn sample(&mut self) {
        self.record_cpu(sys::cpu_usage() as u32);

        let (used, total) = sys::mem_stats();
        self.used_kib = used;
        self.total_kib = total;

        self.total_processes = sys::proc_count();
        self.process_len = 0;
        let mut buf = [0u8; 128];
        for index in 0..self.total_processes {
            if self.process_len == MAX_ROWS {
                break;
            }
            let written = sys::proc_info(index, &mut buf);
            if written <= 0 {
                continue;
            }
            if let Some(process) = parse_process(&buf[..written as usize]) {
                self.processes[self.process_len] = process;
                self.process_len += 1;
            }
        }

        // A selection is an index into a list that just changed underneath it.
        if let Some(selected) = self.selected {
            if selected >= self.process_len {
                self.selected = None;
            }
        }
    }

    fn row_at(&self, x: usize, y: usize) -> Option<usize> {
        if x < 8 || y < TABLE_TOP + ROW_H {
            return None;
        }
        let row = (y - TABLE_TOP - ROW_H) / ROW_H;
        if row < self.process_len {
            Some(row)
        } else {
            None
        }
    }

    fn kill_hit(&self, x: usize, y: usize) -> bool {
        x >= KILL_X && x < KILL_X + KILL_W && y >= KILL_Y && y < KILL_Y + KILL_H
    }

    fn render(&self, fb: &mut Surface) {
        fb.fill(COLOR_BG);
        fb.draw_text(8, 4, b"Activity Monitor - Ring 3", COLOR_HEADING, None);
        fb.draw_text(
            8 + 26 * FONT_WIDTH,
            4,
            b"all figures via syscall",
            COLOR_DIM,
            None,
        );

        self.render_cpu_card(fb);
        self.render_memory_card(fb);
        self.render_table(fb);
        self.render_controls(fb);
    }

    fn render_cpu_card(&self, fb: &mut Surface) {
        fb.draw_rect_outline(CPU_CARD_X, CARD_TOP, CARD_W, CARD_H, COLOR_BUTTON_EDGE);
        fb.draw_text(CPU_CARD_X + 8, CARD_TOP + 6, b"CPU", COLOR_FG, None);

        let current = if self.history_len == 0 {
            0
        } else {
            self.cpu_history[self.history_len - 1]
        };

        let mut buf = [0u8; 32];
        let mut pos = push_u64(&mut buf, 0, current as u64);
        pos = push_str(&mut buf, pos, b"%");
        let colour = if current >= 80 { COLOR_ERROR } else if current >= 50 { COLOR_WARN } else { COLOR_PROMPT };
        fb.draw_text(CPU_CARD_X + CARD_W - 56, CARD_TOP + 6, &buf[..pos], colour, None);

        // History graph: one bar per sample, oldest at the left.
        let graph_x = CPU_CARD_X + 8;
        let graph_bottom = CARD_TOP + CARD_H - 10;
        let graph_h = 78;
        fb.fill_rect(graph_x, graph_bottom - graph_h, CARD_W - 16, graph_h, 0xFF16_1B21);

        // 50% reference line, so the bars have a scale to read against.
        fb.fill_rect(graph_x, graph_bottom - graph_h / 2, CARD_W - 16, 1, 0xFF2A_313A);

        for (i, &sample) in self.cpu_history[..self.history_len].iter().enumerate() {
            let bar_x = graph_x + i * 4;
            if bar_x + 3 > graph_x + CARD_W - 16 {
                break;
            }
            let colour = if sample >= 80 { COLOR_ERROR } else if sample >= 50 { COLOR_WARN } else { COLOR_PROMPT };
            fb.draw_bar(bar_x, graph_bottom, 3, graph_h, sample, 100, colour);
        }

        fb.draw_text(graph_x, graph_bottom + 1, b"60 samples @ 0.5s", COLOR_DIM, None);
    }

    fn render_memory_card(&self, fb: &mut Surface) {
        fb.draw_rect_outline(MEM_CARD_X, CARD_TOP, CARD_W, CARD_H, COLOR_BUTTON_EDGE);
        fb.draw_text(MEM_CARD_X + 8, CARD_TOP + 6, b"Memory", COLOR_FG, None);

        let used_mib = self.used_kib / 1024;
        let total_mib = self.total_kib / 1024;

        let mut buf = [0u8; 48];
        let mut pos = push_u64(&mut buf, 0, used_mib);
        pos = push_str(&mut buf, pos, b" MiB of ");
        pos = push_u64(&mut buf, pos, total_mib);
        pos = push_str(&mut buf, pos, b" MiB");
        fb.draw_text(MEM_CARD_X + 8, CARD_TOP + 28, &buf[..pos], COLOR_FG, None);

        // Usage bar. The project's stated target is a footprint under 60 MiB, so
        // the bar is scaled to that rather than to installed RAM, where the
        // reading would be an invisible sliver.
        let bar_x = MEM_CARD_X + 8;
        let bar_y = CARD_TOP + 52;
        let bar_w = CARD_W - 16;
        fb.fill_rect(bar_x, bar_y, bar_w, 18, 0xFF16_1B21);
        let filled = ((used_mib.min(60) as usize) * bar_w) / 60;
        let colour = if used_mib > 60 { COLOR_ERROR } else { COLOR_PROMPT };
        fb.fill_rect(bar_x, bar_y, filled, 18, colour);
        fb.draw_rect_outline(bar_x, bar_y, bar_w, 18, COLOR_BUTTON_EDGE);

        let mut pos = push_str(&mut buf, 0, b"budget 60 MiB  ");
        pos = if used_mib > 60 {
            push_str(&mut buf, pos, b"OVER")
        } else {
            push_str(&mut buf, pos, b"within")
        };
        fb.draw_text(bar_x, bar_y + 24, &buf[..pos], COLOR_DIM, None);

        let mut pos = push_str(&mut buf, 0, b"processes: ");
        pos = push_u64(&mut buf, pos, self.total_processes);
        fb.draw_text(bar_x, bar_y + 42, &buf[..pos], COLOR_DIM, None);
    }

    fn render_table(&self, fb: &mut Surface) {
        fb.draw_text(8, TABLE_TOP, b"PID  STATE  CPU%   MEM  NAME", COLOR_HEADING, None);
        fb.fill_rect(8, TABLE_TOP + ROW_H - 2, 624, 1, COLOR_BUTTON_EDGE);

        let mut buf = [0u8; 64];
        for (row, process) in self.processes[..self.process_len].iter().enumerate() {
            let y = TABLE_TOP + ROW_H + row * ROW_H;
            if self.selected == Some(row) {
                fb.fill_rect(6, y - 1, 628, ROW_H, 0xFF23_2C36);
            }

            let mut pos = push_u64(&mut buf, 0, process.pid);
            while pos < 5 {
                pos = push_str(&mut buf, pos, b" ");
            }
            pos = push_str(&mut buf, pos, &process.state[..process.state_len]);
            while pos < 12 {
                pos = push_str(&mut buf, pos, b" ");
            }
            pos = push_u64(&mut buf, pos, process.cpu);
            while pos < 18 {
                pos = push_str(&mut buf, pos, b" ");
            }
            pos = push_u64(&mut buf, pos, process.mem_kib);
            while pos < 24 {
                pos = push_str(&mut buf, pos, b" ");
            }
            pos = push_str(&mut buf, pos, &process.name[..process.name_len]);

            // PID 0 is the idle task and cannot be killed; dim it so the table
            // says why the button will refuse.
            let colour = if process.pid == 0 { COLOR_DIM } else { COLOR_FG };
            fb.draw_text(8, y, &buf[..pos], colour, None);
        }

        if self.total_processes as usize > self.process_len {
            let mut pos = push_str(&mut buf, 0, b"... ");
            pos = push_u64(&mut buf, pos, self.total_processes - self.process_len as u64);
            pos = push_str(&mut buf, pos, b" more not shown");
            let y = TABLE_TOP + ROW_H + self.process_len * ROW_H;
            fb.draw_text(8, y, &buf[..pos], COLOR_DIM, None);
        }
    }

    fn render_controls(&self, fb: &mut Surface) {
        let enabled = self.selected.is_some();
        let fill = if !enabled {
            COLOR_BUTTON
        } else if self.hover_kill {
            COLOR_BUTTON_HOVER
        } else {
            COLOR_BUTTON_DANGER
        };
        fb.fill_rect(KILL_X, KILL_Y, KILL_W, KILL_H, fill);
        fb.draw_rect_outline(KILL_X, KILL_Y, KILL_W, KILL_H, COLOR_BUTTON_EDGE);
        let label_colour = if enabled { COLOR_ERROR } else { COLOR_DIM };
        fb.draw_text(KILL_X + 16, KILL_Y + 6, b"Kill Process", label_colour, None);

        fb.draw_text(KILL_X + KILL_W + 16, KILL_Y + 0, self.status.as_bytes(), self.status_color, None);
        fb.draw_text(
            KILL_X + KILL_W + 16,
            KILL_Y + 14,
            b"Click a row to select. PID 0 is protected.",
            COLOR_DIM,
            None,
        );
    }
}

#[no_mangle]
pub extern "C" fn main() -> ! {
    sys::write_str("[USERMON] Ring 3 activity monitor starting.\n");

    let mut fb = match Surface::map() {
        Some(fb) => fb,
        None => {
            sys::write_str("[USERMON] surface mapping failed; exiting.\n");
            sys::exit(1);
        }
    };

    let mut monitor = Monitor::new();
    monitor.sample();
    monitor.render(&mut fb);
    sys::write_str("[USERMON] surface mapped, entering event loop.\n");

    let mut next_sample = sys::uptime() + SAMPLE_INTERVAL_TICKS;

    loop {
        let mut dirty = false;

        while let Some(event) = sys::poll_event() {
            match event {
                sys::Event::Mouse(mouse) => {
                    let x = mouse.x as usize;
                    let y = mouse.y as usize;
                    match mouse.action {
                        sys::MOUSE_MOVE => {
                            let hover = monitor.kill_hit(x, y);
                            if hover != monitor.hover_kill {
                                monitor.hover_kill = hover;
                                dirty = true;
                            }
                        }
                        sys::MOUSE_DOWN if mouse.button == sys::BUTTON_LEFT => {
                            if monitor.kill_hit(x, y) {
                                kill_selected(&mut monitor);
                                dirty = true;
                            } else if let Some(row) = monitor.row_at(x, y) {
                                monitor.selected = Some(row);
                                monitor.status = "Process selected.";
                                monitor.status_color = COLOR_DIM;
                                // Announced so a click can be confirmed from
                                // outside; which row a click landed on is not
                                // otherwise observable.
                                let mut buf = [0u8; 48];
                                let mut pos = push_str(&mut buf, 0, b"[USERMON] selected PID ");
                                pos = push_u64(&mut buf, pos, monitor.processes[row].pid);
                                pos = push_str(&mut buf, pos, b"\n");
                                if let Ok(text) = core::str::from_utf8(&buf[..pos]) {
                                    sys::write_str(text);
                                }
                                dirty = true;
                            }
                        }
                        _ => {}
                    }
                }

                // 'k' kills the selection without needing the pointer.
                sys::Event::Key(key) => {
                    if key.code == b'k' as u16 {
                        kill_selected(&mut monitor);
                        dirty = true;
                    }
                }
            }
        }

        // Resample on a clock rather than on input -- this is the first Ring 3
        // app whose display changes without anyone touching it.
        let now = sys::uptime();
        if now >= next_sample {
            next_sample = now + SAMPLE_INTERVAL_TICKS;
            monitor.sample();
            dirty = true;
        }

        if dirty {
            monitor.render(&mut fb);
        }

        sys::sched_yield();
    }
}

fn kill_selected(monitor: &mut Monitor) {
    let Some(row) = monitor.selected else {
        monitor.status = "Select a process first.";
        monitor.status_color = COLOR_WARN;
        return;
    };
    let pid = monitor.processes[row].pid;

    if sys::kill(pid) == 0 {
        // Logged with the PID so the kill is attributable from outside; the
        // kernel's own reap message names the process, not who asked.
        let mut buf = [0u8; 48];
        let mut pos = push_str(&mut buf, 0, b"[USERMON] killed PID ");
        pos = push_u64(&mut buf, pos, pid);
        pos = push_str(&mut buf, pos, b" from Ring 3.\n");
        if let Ok(text) = core::str::from_utf8(&buf[..pos]) {
            sys::write_str(text);
        }
        monitor.status = "Process terminated.";
        monitor.status_color = COLOR_PROMPT;
        monitor.selected = None;
        monitor.sample();
    } else {
        sys::write_str("[USERMON] kill refused by the kernel.\n");
        monitor.status = "Kill refused (PID 0 is protected).";
        monitor.status_color = COLOR_ERROR;
    }
}
