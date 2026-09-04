#![no_std]
#![no_main]

//! AegisOS Terminal, running in Ring 3.
//!
//! The same job as `src/apps/terminal.rs`, on the other side of the privilege
//! boundary. It has no access to kernel memory, cannot call a kernel function,
//! and reaches system state only through the syscalls in `sys.rs`. If it
//! dereferences null or panics, the hardware traps it and the kernel reaps it;
//! the desktop does not notice.
//!
//! Deliberately allocation-free. Fixed-size buffers mean no heap, no allocator
//! to write, and no way for a runaway command to exhaust memory.

mod font;
mod rt;
mod surface;
mod sys;

use font::{FONT_HEIGHT, FONT_WIDTH};
use surface::Surface;

// ARGB, matching the kernel's Color conventions.
const COLOR_BG: u32 = 0xFF10_1418;
const COLOR_FG: u32 = 0xFFD0_D8E0;
const COLOR_PROMPT: u32 = 0xFF4C_D964;
const COLOR_ERROR: u32 = 0xFFFF_5F56;
const COLOR_HEADING: u32 = 0xFF5A_C8FA;
const COLOR_CURSOR: u32 = 0xFFD0_D8E0;

const COLOR_BUTTON: u32 = 0xFF2A_313A;
const COLOR_BUTTON_HOVER: u32 = 0xFF3D_4753;
const COLOR_BUTTON_DANGER: u32 = 0xFF5A_2A2A;
const COLOR_BUTTON_EDGE: u32 = 0xFF4A_545F;

const MAX_COLS: usize = 96;
const MAX_ROWS: usize = 32;
const INPUT_CAPACITY: usize = 128;

/// Height of the clickable toolbar strip along the top of the surface.
const TOOLBAR_H: usize = 30;

/// A toolbar button. `command` is fed to the same dispatcher the keyboard uses,
/// so clicking one is indistinguishable from typing it.
struct Button {
    label: &'static str,
    command: &'static str,
    danger: bool,
}

const BUTTONS: &[Button] = &[
    Button { label: "help", command: "help", danger: false },
    Button { label: "ps", command: "ps", danger: false },
    Button { label: "free", command: "free", danger: false },
    Button { label: "ls", command: "ls", danger: false },
    Button { label: "clear", command: "clear", danger: false },
    Button { label: "crash", command: "crash", danger: true },
];

/// Left edge and width of button `index`, laid out left to right.
/// Minimum button width. Wide enough that a short label still gets a target a
/// pointer can comfortably hit, rather than a 16-pixel sliver.
const BUTTON_MIN_W: usize = 64;

fn button_bounds(index: usize) -> (usize, usize) {
    let mut x = 6;
    for (i, button) in BUTTONS.iter().enumerate() {
        let width = ((button.label.len() + 4) * FONT_WIDTH).max(BUTTON_MIN_W);
        if i == index {
            return (x, width);
        }
        x += width + 8;
    }
    (x, 0)
}

/// Which button contains `(x, y)`, if any.
fn button_at(x: usize, y: usize) -> Option<usize> {
    if y >= TOOLBAR_H {
        return None;
    }
    for index in 0..BUTTONS.len() {
        let (bx, bw) = button_bounds(index);
        if x >= bx && x < bx + bw {
            return Some(index);
        }
    }
    None
}

/// One rendered line: fixed-width bytes plus a colour.
#[derive(Clone, Copy)]
struct Line {
    text: [u8; MAX_COLS],
    len: usize,
    color: u32,
}

impl Line {
    const fn blank() -> Self {
        Self { text: [b' '; MAX_COLS], len: 0, color: COLOR_FG }
    }
}

struct Terminal {
    lines: [Line; MAX_ROWS],
    line_count: usize,
    input: [u8; INPUT_CAPACITY],
    input_len: usize,
    cols: usize,
    rows: usize,
    /// Button under the pointer, for hover feedback. Proof that motion events
    /// arrive, not just clicks.
    hover: Option<usize>,
}

impl Terminal {
    fn new(cols: usize, rows: usize) -> Self {
        Self {
            lines: [Line::blank(); MAX_ROWS],
            line_count: 0,
            input: [0; INPUT_CAPACITY],
            input_len: 0,
            cols: cols.min(MAX_COLS),
            // One row is reserved for the input line.
            rows: rows.min(MAX_ROWS),
            hover: None,
        }
    }

    /// Appends a line, scrolling the oldest away once full.
    fn print(&mut self, text: &[u8], color: u32) {
        let visible_rows = self.rows.saturating_sub(1);
        if self.line_count == visible_rows && visible_rows > 0 {
            for i in 1..visible_rows {
                self.lines[i - 1] = self.lines[i];
            }
            self.line_count -= 1;
        }
        if self.line_count >= MAX_ROWS {
            return;
        }

        let mut line = Line::blank();
        line.color = color;
        let count = text.len().min(self.cols);
        line.text[..count].copy_from_slice(&text[..count]);
        line.len = count;
        self.lines[self.line_count] = line;
        self.line_count += 1;
    }

    fn print_str(&mut self, text: &str, color: u32) {
        self.print(text.as_bytes(), color);
    }

    fn clear(&mut self) {
        self.line_count = 0;
    }

    fn render(&self, fb: &mut Surface) {
        fb.fill(COLOR_BG);
        self.render_toolbar(fb);

        for (row, line) in self.lines[..self.line_count].iter().enumerate() {
            fb.draw_text(
                0,
                TOOLBAR_H + row * FONT_HEIGHT,
                &line.text[..line.len],
                line.color,
                None,
            );
        }

        // Input line, pinned to the bottom row.
        let input_row = self.rows.saturating_sub(1);
        let y = TOOLBAR_H + input_row * FONT_HEIGHT;
        fb.draw_text(0, y, b"$ ", COLOR_PROMPT, None);
        let shown = self.input_len.min(self.cols.saturating_sub(3));
        fb.draw_text(2 * FONT_WIDTH, y, &self.input[..shown], COLOR_FG, None);

        // Block cursor.
        fb.fill_rect((2 + shown) * FONT_WIDTH, y, FONT_WIDTH, FONT_HEIGHT, COLOR_CURSOR);
    }

    fn render_toolbar(&self, fb: &mut Surface) {
        for (index, button) in BUTTONS.iter().enumerate() {
            let (x, width) = button_bounds(index);
            if x + width > fb.width {
                break;
            }

            let base = if button.danger { COLOR_BUTTON_DANGER } else { COLOR_BUTTON };
            let fill = if self.hover == Some(index) { COLOR_BUTTON_HOVER } else { base };

            let top = 3;
            let height = TOOLBAR_H - 6;
            fb.fill_rect(x, top, width, height, fill);
            // A one-pixel edge, so the buttons read as buttons.
            fb.fill_rect(x, top, width, 1, COLOR_BUTTON_EDGE);
            fb.fill_rect(x, top + height - 1, width, 1, COLOR_BUTTON_EDGE);

            let label_color = if button.danger { COLOR_ERROR } else { COLOR_FG };
            // Centre the label in the button.
            let label_w = button.label.len() * FONT_WIDTH;
            let label_x = x + (width - label_w) / 2;
            let label_y = top + (height - FONT_HEIGHT) / 2;
            fb.draw_text(label_x, label_y, button.label.as_bytes(), label_color, None);
        }
    }
}

// -----------------------------------------------------------------------------
// Formatting helpers (no allocator, so no `format!`)
// -----------------------------------------------------------------------------

/// Appends `value` in decimal to `buf` at `pos`, returning the new position.
fn push_u64(buf: &mut [u8], mut pos: usize, value: u64) -> usize {
    let mut digits = [0u8; 20];
    let mut count = 0;
    let mut v = value;
    loop {
        digits[count] = b'0' + (v % 10) as u8;
        count += 1;
        v /= 10;
        if v == 0 {
            break;
        }
    }
    while count > 0 && pos < buf.len() {
        count -= 1;
        buf[pos] = digits[count];
        pos += 1;
    }
    pos
}

fn push_str(buf: &mut [u8], mut pos: usize, s: &[u8]) -> usize {
    for &b in s {
        if pos >= buf.len() {
            break;
        }
        buf[pos] = b;
        pos += 1;
    }
    pos
}

/// Splits `input` into the command word and the remainder.
fn split_command(input: &[u8]) -> (&[u8], &[u8]) {
    let mut i = 0;
    while i < input.len() && input[i] != b' ' {
        i += 1;
    }
    let cmd = &input[..i];
    let mut j = i;
    while j < input.len() && input[j] == b' ' {
        j += 1;
    }
    (cmd, &input[j..])
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

// -----------------------------------------------------------------------------
// Commands
// -----------------------------------------------------------------------------

fn run_command(term: &mut Terminal, input: &[u8]) {
    let (cmd, args) = split_command(input);
    let mut buf = [0u8; MAX_COLS];

    match cmd {
        b"" => {}

        b"help" => {
            term.print_str("Ring 3 Terminal - available commands", COLOR_HEADING);
            term.print_str("  help          this list", COLOR_FG);
            term.print_str("  echo <text>   print text", COLOR_FG);
            term.print_str("  clear         clear the screen", COLOR_FG);
            term.print_str("  ps            list processes", COLOR_FG);
            term.print_str("  free          memory statistics", COLOR_FG);
            term.print_str("  kill <pid>    terminate a process", COLOR_FG);
            term.print_str("  ls            list VFS paths", COLOR_FG);
            term.print_str("  cat <path>    print a VFS file", COLOR_FG);
            term.print_str("  uptime        ticks since boot", COLOR_FG);
            term.print_str("  pid           this process's PID", COLOR_FG);
            term.print_str("  beep          tone on the PC speaker", COLOR_FG);
            term.print_str("  crash         dereference null (isolation test)", COLOR_ERROR);
            term.print_str("  panic         Rust panic (isolation test)", COLOR_ERROR);
            term.print_str("  exit          terminate this terminal", COLOR_FG);
        }

        b"echo" => term.print(args, COLOR_FG),

        b"clear" => term.clear(),

        b"ps" => {
            term.print_str("PID STATE CPU% NAME", COLOR_HEADING);
            let count = sys::proc_count();
            for i in 0..count {
                let written = sys::proc_info(i, &mut buf);
                if written > 0 {
                    let len = written as usize;
                    term.print(&buf[..len.min(MAX_COLS)], COLOR_FG);
                }
            }
        }

        b"free" => {
            let (used_kb, total_kb) = sys::mem_stats();
            let mut pos = push_str(&mut buf, 0, b"used ");
            pos = push_u64(&mut buf, pos, used_kb);
            pos = push_str(&mut buf, pos, b" KiB of ");
            pos = push_u64(&mut buf, pos, total_kb);
            pos = push_str(&mut buf, pos, b" KiB");
            term.print(&buf[..pos], COLOR_FG);
        }

        b"kill" => match parse_u64(args) {
            Some(pid) => {
                if sys::kill(pid) == 0 {
                    let mut pos = push_str(&mut buf, 0, b"killed PID ");
                    pos = push_u64(&mut buf, pos, pid);
                    term.print(&buf[..pos], COLOR_FG);
                } else {
                    term.print_str("kill: no such process, or PID 0 is protected", COLOR_ERROR);
                }
            }
            None => term.print_str("usage: kill <pid>", COLOR_ERROR),
        },

        b"ls" => {
            let count = sys::fs_count();
            if count == 0 {
                term.print_str("(empty)", COLOR_FG);
            }
            for i in 0..count {
                let written = sys::fs_name(i, &mut buf);
                if written > 0 {
                    term.print(&buf[..(written as usize).min(MAX_COLS)], COLOR_FG);
                }
            }
        }

        b"cat" => {
            if args.is_empty() {
                term.print_str("usage: cat <path>", COLOR_ERROR);
            } else if let Ok(path) = core::str::from_utf8(args) {
                let mut contents = [0u8; 1024];
                let read = sys::fs_read(path, &mut contents);
                if read < 0 {
                    term.print_str("cat: no such file", COLOR_ERROR);
                } else {
                    // Split on newlines so multi-line files render as lines.
                    let mut start = 0usize;
                    for i in 0..read as usize {
                        if contents[i] == b'\n' {
                            term.print(&contents[start..i], COLOR_FG);
                            start = i + 1;
                        }
                    }
                    if start < read as usize {
                        term.print(&contents[start..read as usize], COLOR_FG);
                    }
                }
            } else {
                term.print_str("cat: path is not valid UTF-8", COLOR_ERROR);
            }
        }

        b"uptime" => {
            let mut pos = push_u64(&mut buf, 0, sys::uptime());
            pos = push_str(&mut buf, pos, b" ticks (100 Hz)");
            term.print(&buf[..pos], COLOR_FG);
        }

        b"pid" => {
            let mut pos = push_str(&mut buf, 0, b"pid ");
            pos = push_u64(&mut buf, pos, sys::getpid());
            term.print(&buf[..pos], COLOR_FG);
        }

        b"beep" => {
            sys::beep(880, 120);
            term.print_str("beep", COLOR_FG);
        }

        // The two commands that exist to be run, not to be useful. Both should
        // take down this process and leave the desktop composing.
        b"crash" => {
            term.print_str("dereferencing null in Ring 3...", COLOR_ERROR);
            term.render_now();
            unsafe {
                core::ptr::write_volatile(0 as *mut u32, 0xDEAD_BEEF);
            }
        }

        b"panic" => {
            term.print_str("panicking in Ring 3...", COLOR_ERROR);
            term.render_now();
            panic!("deliberate userspace panic");
        }

        b"exit" => {
            sys::write_str("[USERTERM] exit requested; terminating.\n");
            sys::exit(0);
        }

        _ => {
            let mut pos = push_str(&mut buf, 0, b"unknown command: ");
            pos = push_str(&mut buf, pos, cmd);
            term.print(&buf[..pos], COLOR_ERROR);
        }
    }
}

impl Terminal {
    /// Forces a frame before a command that is about to kill the process, so
    /// the message explaining what happened is actually on screen.
    fn render_now(&self) {
        if let Some(mut fb) = Surface::map() {
            self.render(&mut fb);
        }
    }
}

/// Echoes a command line and runs it.
///
/// Both the keyboard and the toolbar go through here, so a clicked button is
/// indistinguishable from the same text typed.
fn submit(term: &mut Terminal, line: &[u8]) {
    let mut echo = [0u8; MAX_COLS];
    let mut pos = push_str(&mut echo, 0, b"$ ");
    pos = push_str(&mut echo, pos, line);
    term.print(&echo[..pos], COLOR_PROMPT);
    run_command(term, line);
}

// -----------------------------------------------------------------------------

#[no_mangle]
pub extern "C" fn main() -> ! {
    sys::write_str("[USERTERM] Ring 3 terminal starting.\n");

    let mut fb = match Surface::map() {
        Some(fb) => fb,
        None => {
            sys::write_str("[USERTERM] surface mapping failed; exiting.\n");
            sys::exit(1);
        }
    };

    // The toolbar eats the top of the surface, so the text area has fewer rows
    // than the surface height alone would suggest.
    let text_rows = (fb.height - TOOLBAR_H) / FONT_HEIGHT;
    let mut term = Terminal::new(fb.cols(), text_rows);
    term.print_str("AegisOS Terminal - Ring 3", COLOR_HEADING);
    term.print_str("This shell is a user process. Type 'help' or click above.", COLOR_FG);
    term.print_str("'crash' and 'panic' kill it without taking down the OS.", COLOR_FG);

    // Paint once before waiting for input. Rendering only on events left the
    // window blank until the first keystroke, with the startup banner sitting
    // in the line buffer unseen.
    term.render(&mut fb);

    sys::write_str("[USERTERM] surface mapped, entering event loop.\n");

    let mut announced_move = false;

    loop {
        let mut dirty = false;

        while let Some(event) = sys::poll_event() {
            match event {
                sys::Event::Key(key) => {
                    dirty = true;
                    match key.code {
                        sys::UKEY_ENTER => {
                            let len = term.input_len;
                            let mut line = [0u8; INPUT_CAPACITY];
                            line[..len].copy_from_slice(&term.input[..len]);
                            term.input_len = 0;
                            submit(&mut term, &line[..len]);
                        }
                        sys::UKEY_BACKSPACE => {
                            term.input_len = term.input_len.saturating_sub(1);
                        }
                        sys::UKEY_ESCAPE => {
                            term.input_len = 0;
                        }
                        code if code < 0x100 => {
                            let byte = code as u8;
                            if (32..=126).contains(&byte) && term.input_len < INPUT_CAPACITY {
                                term.input[term.input_len] = byte;
                                term.input_len += 1;
                            }
                        }
                        _ => {}
                    }
                }

                sys::Event::Mouse(mouse) => {
                    let x = mouse.x as usize;
                    let y = mouse.y as usize;

                    // Announce the first pointer event of each kind. Cheap, and
                    // it turns "did input reach Ring 3?" into something readable
                    // on the serial console instead of a guess from pixels.
                    if !announced_move {
                        announced_move = true;
                        sys::write_str("[USERTERM] first mouse event received.\n");
                    }

                    match mouse.action {
                        sys::MOUSE_MOVE => {
                            let hover = button_at(x, y);
                            if hover != term.hover {
                                term.hover = hover;
                                dirty = true;
                            }
                        }
                        sys::MOUSE_DOWN if mouse.button == sys::BUTTON_LEFT => {
                            if let Some(index) = button_at(x, y) {
                                dirty = true;
                                let command = BUTTONS[index].command;
                                sys::write_str("[USERTERM] toolbar click: ");
                                sys::write_str(command);
                                sys::write_str("\n");
                                submit(&mut term, command.as_bytes());
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        if dirty {
            term.render(&mut fb);
        }

        // Nothing to do until the next key. Yielding hands the CPU back rather
        // than spinning through the quantum.
        sys::sched_yield();
    }
}
