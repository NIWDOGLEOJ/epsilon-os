#![no_std]
#![no_main]

//! AegisOS Crash-Test demo, running in Ring 3.
//!
//! The Ring 0 version of this app (`src/apps/crash_test.rs`) asks the kernel to
//! spawn a faulting process by returning an `AppAction` the compositor acts on.
//! This one asks through `SYS_SPAWN_FAULT` instead, and is itself a user process
//! while it does so.
//!
//! That makes the demonstration stricter than it used to be. Before, a Ring 0
//! app drew the proof that Ring 3 faults are contained. Now the app making the
//! claim is subject to it: two user processes are involved, one deliberately
//! dies, and the one that asked for it carries on drawing.

#[path = "../rt.rs"]
mod rt;

use aegis_user::font::{FONT_HEIGHT, FONT_WIDTH};
use aegis_user::surface::Surface;
use aegis_user::sys;
use aegis_user::text::{
    push_str, push_u64, COLOR_BG, COLOR_BUTTON, COLOR_BUTTON_DANGER, COLOR_BUTTON_EDGE,
    COLOR_BUTTON_HOVER, COLOR_DIM, COLOR_ERROR, COLOR_FG, COLOR_HEADING, COLOR_PROMPT, COLOR_WARN,
};

/// One fault the demo can inject. `kind` is the argument to `SYS_SPAWN_FAULT`,
/// matching `Scheduler::spawn_user_fault_test`.
struct Fault {
    kind: u64,
    label: &'static str,
    detail: &'static str,
}

const FAULTS: &[Fault] = &[
    Fault { kind: 0, label: "Null Pointer", detail: "write to 0x0  -> #PF (vector 14)" },
    Fault { kind: 1, label: "Divide by Zero", detail: "div by 0      -> #DE (vector 0)" },
    Fault { kind: 2, label: "Kernel Write", detail: "write to Ring 0 -> #PF (vector 14)" },
    Fault { kind: 3, label: "Invalid Opcode", detail: "ud2           -> #UD (vector 6)" },
];

// Kept compact deliberately: the window has to sit above the Ring 3 terminal's
// titlebar row so that both windows keep a strip a pointer can reach.
const BUTTON_X: usize = 16;
const BUTTON_W: usize = 240;
const BUTTON_H: usize = 34;
const BUTTON_TOP: usize = 44;
const BUTTON_GAP: usize = 6;

fn button_y(index: usize) -> usize {
    BUTTON_TOP + index * (BUTTON_H + BUTTON_GAP)
}

fn button_at(x: usize, y: usize) -> Option<usize> {
    if x < BUTTON_X || x >= BUTTON_X + BUTTON_W {
        return None;
    }
    for index in 0..FAULTS.len() {
        let top = button_y(index);
        if y >= top && y < top + BUTTON_H {
            return Some(index);
        }
    }
    None
}

struct App {
    hover: Option<usize>,
    /// PIDs spawned so far, newest last, for the log panel.
    log: [(u64, usize); 8],
    log_len: usize,
    spawned: u64,
}

impl App {
    const fn new() -> Self {
        Self { hover: None, log: [(0, 0); 8], log_len: 0, spawned: 0 }
    }

    fn record(&mut self, pid: u64, fault: usize) {
        if self.log_len == self.log.len() {
            for i in 1..self.log.len() {
                self.log[i - 1] = self.log[i];
            }
            self.log_len -= 1;
        }
        self.log[self.log_len] = (pid, fault);
        self.log_len += 1;
        self.spawned += 1;
    }

    fn render(&self, fb: &mut Surface) {
        fb.fill(COLOR_BG);

        fb.draw_text(16, 4, b"Ring 3 Hardware Fault Isolation", COLOR_HEADING, None);
        fb.draw_text(
            16,
            22,
            b"This app is a user process. So is every fault it spawns.",
            COLOR_DIM,
            None,
        );

        for (index, fault) in FAULTS.iter().enumerate() {
            let y = button_y(index);
            if y + BUTTON_H > fb.height {
                break;
            }
            let fill = if self.hover == Some(index) { COLOR_BUTTON_HOVER } else { COLOR_BUTTON_DANGER };
            fb.fill_rect(BUTTON_X, y, BUTTON_W, BUTTON_H, fill);
            fb.fill_rect(BUTTON_X, y, BUTTON_W, 1, COLOR_BUTTON_EDGE);
            fb.fill_rect(BUTTON_X, y + BUTTON_H - 1, BUTTON_W, 1, COLOR_BUTTON_EDGE);
            fb.draw_text(BUTTON_X + 12, y + 2, fault.label.as_bytes(), COLOR_ERROR, None);
            fb.draw_text(BUTTON_X + 12, y + 17, fault.detail.as_bytes(), COLOR_DIM, None);
        }

        // Right-hand panel: what has been injected so far.
        let panel_x = BUTTON_X + BUTTON_W + 24;
        fb.draw_text(panel_x, BUTTON_TOP - 18, b"Injected faults", COLOR_HEADING, None);

        let mut buf = [0u8; 64];
        if self.log_len == 0 {
            fb.draw_text(panel_x, BUTTON_TOP, b"(none yet - click a button)", COLOR_DIM, None);
        } else {
            for (row, &(pid, fault)) in self.log[..self.log_len].iter().enumerate() {
                let mut pos = push_str(&mut buf, 0, b"PID ");
                pos = push_u64(&mut buf, pos, pid);
                pos = push_str(&mut buf, pos, b"  ");
                pos = push_str(&mut buf, pos, FAULTS[fault].label.as_bytes());
                fb.draw_text(panel_x, BUTTON_TOP + row * FONT_HEIGHT, &buf[..pos], COLOR_FG, None);
            }
        }

        let mut pos = push_str(&mut buf, 0, b"Spawned: ");
        pos = push_u64(&mut buf, pos, self.spawned);
        pos = push_str(&mut buf, pos, b"   This process: PID ");
        pos = push_u64(&mut buf, pos, sys::getpid());
        let status_y = BUTTON_TOP + FAULTS.len() * (BUTTON_H + BUTTON_GAP) + 4;
        fb.draw_text(16, status_y, &buf[..pos], COLOR_PROMPT, None);
        fb.draw_text(
            16,
            status_y + FONT_HEIGHT + 2,
            b"Still drawing after every one of them.",
            COLOR_WARN,
            None,
        );
    }
}

#[no_mangle]
pub extern "C" fn main() -> ! {
    sys::write_str("[USERCRASH] Ring 3 crash-test starting.\n");

    let mut fb = match Surface::map() {
        Some(fb) => fb,
        None => {
            sys::write_str("[USERCRASH] surface mapping failed; exiting.\n");
            sys::exit(1);
        }
    };

    let mut app = App::new();
    app.render(&mut fb);
    sys::write_str("[USERCRASH] surface mapped, entering event loop.\n");

    loop {
        let mut dirty = false;

        while let Some(event) = sys::poll_event() {
            match event {
                sys::Event::Mouse(mouse) => {
                    let x = mouse.x as usize;
                    let y = mouse.y as usize;
                    match mouse.action {
                        sys::MOUSE_MOVE => {
                            let hover = button_at(x, y);
                            if hover != app.hover {
                                app.hover = hover;
                                dirty = true;
                            }
                        }
                        sys::MOUSE_DOWN if mouse.button == sys::BUTTON_LEFT => {
                            if let Some(index) = button_at(x, y) {
                                let pid = sys::spawn_fault(FAULTS[index].kind);
                                sys::write_str("[USERCRASH] requested fault: ");
                                sys::write_str(FAULTS[index].label);
                                sys::write_str("\n");
                                if pid >= 0 {
                                    app.record(pid as u64, index);
                                }
                                dirty = true;
                            }
                        }
                        _ => {}
                    }
                }

                // Keys 1..4 inject the same faults, so the demo is reachable
                // without a pointer.
                sys::Event::Key(key) => {
                    if key.code < 0x100 && (b'1'..=b'4').contains(&(key.code as u8)) {
                        let index = (key.code as u8 - b'1') as usize;
                        let pid = sys::spawn_fault(FAULTS[index].kind);
                        // Logged identically to a click, so the two input paths
                        // are indistinguishable from outside.
                        sys::write_str("[USERCRASH] requested fault: ");
                        sys::write_str(FAULTS[index].label);
                        sys::write_str("\n");
                        if pid >= 0 {
                            app.record(pid as u64, index);
                        }
                        dirty = true;
                    }
                }
            }
        }

        if dirty {
            app.render(&mut fb);
        }

        sys::sched_yield();
    }
}
