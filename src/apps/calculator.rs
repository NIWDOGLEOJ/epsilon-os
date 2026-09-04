//! Scientific Calculator 2.0 for AegisOS
//!
//! Dual-pane macOS-inspired scientific calculator featuring:
//! - 2-line LCD screen with live expression sub-header and active input
//! - 5x5 Scientific & Numeric keypad with square root, powers, reciprocal, percent, constants (pi, e)
//! - Interactive Calculation History Tape (Paper Roll) with recallable past operations and clear action
//! - Newton-Raphson float square root and binary power exponentiation
//! - Full PS/2 keyboard numpad and hotkey evaluation

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::drivers::framebuffer::Framebuffer;
use crate::drivers::ps2_keyboard::{KeyCode, KeyEvent};
use crate::gui::font::{draw_string, measure_string};
use crate::gui::primitives::{
    draw_rect, draw_rounded_rect, draw_rounded_rect_outline, Color, Rect,
};
use crate::gui::window::Window;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CalcOp {
    Add,
    Subtract,
    Multiply,
    Divide,
    Power,
}

impl CalcOp {
    pub fn symbol(&self) -> &'static str {
        match self {
            CalcOp::Add => "+",
            CalcOp::Subtract => "-",
            CalcOp::Multiply => "×",
            CalcOp::Divide => "÷",
            CalcOp::Power => "^",
        }
    }
}

#[derive(Debug, Clone)]
pub struct HistoryEntry {
    pub expression: String,
    pub result: f64,
}

pub struct CalculatorApp {
    pub display: String,
    pub expression_preview: String,
    pub accumulator: f64,
    pub pending_op: Option<CalcOp>,
    pub clear_on_next_digit: bool,
    pub is_error: bool,
    pub history: Vec<HistoryEntry>,
}

impl CalculatorApp {
    pub const PI: f64 = 3.1415926535;
    pub const E: f64 = 2.7182818284;

    pub fn new() -> Self {
        Self {
            display: "0".to_string(),
            expression_preview: String::new(),
            accumulator: 0.0,
            pending_op: None,
            clear_on_next_digit: false,
            is_error: false,
            history: Vec::new(),
        }
    }

    /// Inputs a numeric digit (0-9)
    pub fn input_digit(&mut self, digit: char) {
        if self.is_error {
            self.clear();
        }
        if self.clear_on_next_digit || self.display == "0" {
            self.display.clear();
            self.clear_on_next_digit = false;
        }
        if self.display.len() < 14 {
            self.display.push(digit);
        }
    }

    /// Inputs a decimal point
    pub fn input_dot(&mut self) {
        if self.is_error {
            self.clear();
        }
        if self.clear_on_next_digit {
            self.display = "0.".to_string();
            self.clear_on_next_digit = false;
            return;
        }
        if !self.display.contains('.') && self.display.len() < 13 {
            self.display.push('.');
        }
    }

    /// Clears active display; second clear resets entire accumulator and expression
    pub fn clear(&mut self) {
        self.display = "0".to_string();
        self.is_error = false;
        if self.clear_on_next_digit {
            self.accumulator = 0.0;
            self.pending_op = None;
            self.expression_preview.clear();
        }
        self.clear_on_next_digit = false;
    }

    /// Clears everything (All Clear)
    pub fn clear_all(&mut self) {
        self.display = "0".to_string();
        self.expression_preview.clear();
        self.accumulator = 0.0;
        self.pending_op = None;
        self.clear_on_next_digit = false;
        self.is_error = false;
    }

    /// Clears the history tape
    pub fn clear_tape(&mut self) {
        self.history.clear();
    }

    /// Toggles active sign (+/-)
    pub fn toggle_sign(&mut self) {
        if self.is_error || self.display == "0" {
            return;
        }
        if self.display.starts_with('-') {
            self.display.remove(0);
        } else {
            self.display.insert(0, '-');
        }
    }

    /// Calculates percentage of current number
    pub fn percent(&mut self) {
        if self.is_error {
            return;
        }
        let val = self.parse_display();
        let res = val / 100.0;
        self.set_display_num(res);
    }

    /// Square root via iterative Newton-Raphson approximation
    pub fn sqrt(&mut self) {
        if self.is_error {
            return;
        }
        let val = self.parse_display();
        if val < 0.0 {
            self.set_error("Domain Error: √ negative");
            return;
        }
        let res = Self::compute_sqrt(val);
        let expr = format!("√({})", self.format_num(val));
        self.push_history(expr, res);
        self.set_display_num(res);
        self.clear_on_next_digit = true;
    }

    /// Computes x²
    pub fn square(&mut self) {
        if self.is_error {
            return;
        }
        let val = self.parse_display();
        let res = val * val;
        let expr = format!("({})^2", self.format_num(val));
        self.push_history(expr, res);
        self.set_display_num(res);
        self.clear_on_next_digit = true;
    }

    /// Computes reciprocal 1/x
    pub fn reciprocal(&mut self) {
        if self.is_error {
            return;
        }
        let val = self.parse_display();
        if val == 0.0 {
            self.set_error("Divide by Zero");
            return;
        }
        let res = 1.0 / val;
        let expr = format!("1/({})", self.format_num(val));
        self.push_history(expr, res);
        self.set_display_num(res);
        self.clear_on_next_digit = true;
    }

    /// Inserts mathematical constant (pi or e)
    pub fn insert_constant(&mut self, val: f64, name: &str) {
        if self.is_error {
            self.clear();
        }
        self.set_display_num(val);
        self.expression_preview = name.to_string();
        self.clear_on_next_digit = true;
    }

    /// Sets binary arithmetic/scientific operator (+, -, *, /, ^)
    pub fn set_operator(&mut self, op: CalcOp) {
        if self.is_error {
            return;
        }
        let val = self.parse_display();
        if let Some(prev_op) = self.pending_op {
            if !self.clear_on_next_digit {
                match self.apply_op(self.accumulator, val, prev_op) {
                    Ok(res) => {
                        self.accumulator = res;
                        self.set_display_num(res);
                    }
                    Err(err) => {
                        self.set_error(err);
                        return;
                    }
                }
            }
        } else {
            self.accumulator = val;
        }

        self.pending_op = Some(op);
        self.expression_preview = format!("{} {}", self.format_num(self.accumulator), op.symbol());
        self.clear_on_next_digit = true;
    }

    /// Evaluates current expression and pushes to history tape
    pub fn equals(&mut self) {
        if self.is_error {
            return;
        }
        if let Some(op) = self.pending_op {
            let val = self.parse_display();
            match self.apply_op(self.accumulator, val, op) {
                Ok(res) => {
                    let expr = format!("{} {} {}", self.format_num(self.accumulator), op.symbol(), self.format_num(val));
                    self.push_history(expr.clone(), res);
                    self.expression_preview = format!("{} =", expr);
                    self.set_display_num(res);
                    self.accumulator = res;
                    self.pending_op = None;
                    self.clear_on_next_digit = true;
                    crate::drivers::speaker::play_sound_effect(crate::drivers::speaker::SoundEffect::BeepSuccess);
                }
                Err(err) => {
                    self.set_error(err);
                }
            }
        }
    }

    /// Recalls a previous result from the history tape into active display
    pub fn recall_history(&mut self, idx: usize) {
        if idx < self.history.len() {
            let res = self.history[idx].result;
            self.set_display_num(res);
            self.expression_preview = format!("Recalled [{}]", idx + 1);
            self.clear_on_next_digit = true;
            crate::drivers::speaker::play_sound_effect(crate::drivers::speaker::SoundEffect::WindowSnap);
        }
    }

    fn push_history(&mut self, expression: String, result: f64) {
        if self.history.len() >= 16 {
            self.history.remove(0);
        }
        self.history.push(HistoryEntry { expression, result });
    }

    fn set_error(&mut self, msg: &'static str) {
        self.display = "Error".to_string();
        self.expression_preview = msg.to_string();
        self.is_error = true;
        self.clear_on_next_digit = true;
        crate::drivers::speaker::play_sound_effect(crate::drivers::speaker::SoundEffect::Alert);
    }

    /// Newton-Raphson approximation for float square root in no_std
    pub fn compute_sqrt(x: f64) -> f64 {
        if x == 0.0 {
            return 0.0;
        }
        let mut guess = if x > 1.0 { x / 2.0 } else { 1.0 };
        for _ in 0..20 {
            guess = 0.5 * (guess + x / guess);
        }
        guess
    }

    /// Binary power exponentiation for integer exponents, plus basic approximation
    pub fn compute_power(base: f64, exp: f64) -> f64 {
        if exp == 0.0 {
            return 1.0;
        }
        if base == 0.0 {
            return 0.0;
        }
        let is_neg_exp = exp < 0.0;
        let abs_exp = if is_neg_exp { -exp } else { exp };

        // For integer exponents: fast binary exponentiation
        let mut n = abs_exp as u64;
        let mut result = 1.0;
        let mut curr_base = base;

        while n > 0 {
            if n % 2 == 1 {
                result *= curr_base;
            }
            curr_base *= curr_base;
            n /= 2;
        }

        if is_neg_exp {
            1.0 / result
        } else {
            result
        }
    }

    fn apply_op(&self, a: f64, b: f64, op: CalcOp) -> Result<f64, &'static str> {
        match op {
            CalcOp::Add => Ok(a + b),
            CalcOp::Subtract => Ok(a - b),
            CalcOp::Multiply => Ok(a * b),
            CalcOp::Divide => {
                if b == 0.0 {
                    Err("Divide by Zero")
                } else {
                    Ok(a / b)
                }
            }
            CalcOp::Power => Ok(Self::compute_power(a, b)),
        }
    }

    pub fn parse_display(&self) -> f64 {
        let s = self.display.as_str();
        let mut neg = false;
        let mut num = 0.0;
        let mut frac = 0.0;
        let mut div = 1.0;
        let mut in_frac = false;

        for c in s.chars() {
            if c == '-' {
                neg = true;
            } else if c == '.' {
                in_frac = true;
            } else if c.is_ascii_digit() {
                let d = (c as u8 - b'0') as f64;
                if !in_frac {
                    num = num * 10.0 + d;
                } else {
                    div *= 10.0;
                    frac += d / div;
                }
            }
        }
        let res = num + frac;
        if neg { -res } else { res }
    }

    fn format_num(&self, num: f64) -> String {
        let mut s = String::new();
        let neg = num < 0.0;
        let val = if neg { -num } else { num };

        let int_part = val as u64;
        let frac_part = ((val - int_part as f64) * 1000.0) as u64;

        if neg && (int_part > 0 || frac_part > 0) {
            s.push('-');
        }

        let mut int_buf = [0u8; 20];
        let mut len = 0;
        let mut v = int_part;
        if v == 0 {
            s.push('0');
        } else {
            while v > 0 && len < 20 {
                int_buf[len] = b'0' + (v % 10) as u8;
                v /= 10;
                len += 1;
            }
            for i in 0..len {
                s.push(int_buf[len - 1 - i] as char);
            }
        }

        if frac_part > 0 {
            s.push('.');
            let mut f = frac_part;
            let d3 = f % 10; f /= 10;
            let d2 = f % 10; f /= 10;
            let d1 = f % 10;
            s.push((b'0' + d1 as u8) as char);
            if d2 > 0 || d3 > 0 {
                s.push((b'0' + d2 as u8) as char);
            }
            if d3 > 0 {
                s.push((b'0' + d3 as u8) as char);
            }
        }

        s
    }

    fn set_display_num(&mut self, num: f64) {
        self.display = self.format_num(num);
    }

    /// Handles keyboard input
    pub fn handle_key(&mut self, event: KeyEvent) {
        if !event.pressed {
            return;
        }
        match event.code {
            KeyCode::Printable(c) => match c as char {
                '0'..='9' => self.input_digit(c as char),
                '.' => self.input_dot(),
                '+' => self.set_operator(CalcOp::Add),
                '-' => self.set_operator(CalcOp::Subtract),
                '*' => self.set_operator(CalcOp::Multiply),
                '/' => self.set_operator(CalcOp::Divide),
                '^' => self.set_operator(CalcOp::Power),
                '=' => self.equals(),
                'c' | 'C' => self.clear(),
                '%' => self.percent(),
                's' | 'S' => self.sqrt(),
                'p' | 'P' => self.insert_constant(Self::PI, "π"),
                'e' | 'E' => self.insert_constant(Self::E, "e"),
                _ => {}
            },
            KeyCode::Enter => self.equals(),
            KeyCode::Escape => self.clear_all(),
            KeyCode::Backspace => {
                if self.display.len() > 1 && !self.is_error {
                    self.display.pop();
                } else {
                    self.display = "0".to_string();
                }
            }
            _ => {}
        }
    }

    /// Handles mouse clicks inside the Calculator window
    pub fn handle_click(&mut self, win: &Window, x: i32, y: i32) {
        let client = win.client_rect();
        let rel_x = x - client.x;
        let rel_y = y - client.y;

        // 1. Right Pane: History Tape (x >= 265)
        if rel_x >= 265 {
            // [ Clear Tape ] button at rel_x: 375..435, rel_y: 8..28
            if (8..30).contains(&rel_y) && rel_x >= 370 {
                self.clear_tape();
                return;
            }

            // Clickable history items (rel_y: 36..330)
            let item_h = 32;
            let start_y = 36;
            if rel_y >= start_y {
                let idx = ((rel_y - start_y) / item_h) as usize;
                if idx < self.history.len() {
                    self.recall_history(idx);
                }
            }
            return;
        }

        // 2. Left Pane: 5x5 Keypad Grid (pad_x: 8, pad_y: 72)
        let pad_x = 8;
        let pad_y = 72;
        let btn_w = 46;
        let btn_h = 34;
        let gap = 4;

        let grid = [
            ["C", "±", "√", "x²", "÷"],
            ["7", "8", "9", "1/x", "×"],
            ["4", "5", "6", "x^y", "-"],
            ["1", "2", "3", "π", "+"],
            ["0", ".", "%", "e", "="],
        ];

        for (row_idx, row) in grid.iter().enumerate() {
            for (col_idx, &label) in row.iter().enumerate() {
                let bx = pad_x + (col_idx as i32 * (btn_w + gap));
                let by = pad_y + (row_idx as i32 * (btn_h + gap));
                let btn_rect = Rect::new(bx, by, btn_w as u32, btn_h as u32);

                if btn_rect.contains(rel_x, rel_y) {
                    match label {
                        "0" | "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" => {
                            self.input_digit(label.chars().next().unwrap());
                        }
                        "." => self.input_dot(),
                        "C" => self.clear(),
                        "±" => self.toggle_sign(),
                        "%" => self.percent(),
                        "√" => self.sqrt(),
                        "x²" => self.square(),
                        "1/x" => self.reciprocal(),
                        "π" => self.insert_constant(Self::PI, "π"),
                        "e" => self.insert_constant(Self::E, "e"),
                        "+" => self.set_operator(CalcOp::Add),
                        "-" => self.set_operator(CalcOp::Subtract),
                        "×" => self.set_operator(CalcOp::Multiply),
                        "÷" => self.set_operator(CalcOp::Divide),
                        "x^y" => self.set_operator(CalcOp::Power),
                        "=" => self.equals(),
                        _ => {}
                    }
                    return;
                }
            }
        }
    }

    /// Renders the complete dual-pane Scientific Calculator interface
    pub fn render(&self, win: &Window, fb: &mut Framebuffer) {
        let client = win.client_rect();
        if client.width < 250 || client.height < 240 {
            return;
        }

        // 1. Dark calculator body background
        draw_rect(fb, client, Color::rgb(26, 28, 34));

        // 2. Dual-pane vertical divider
        let divider_x = client.x + 258;
        draw_rect(fb, Rect::new(divider_x, client.y, 1, client.height), Color::rgb(45, 48, 58));

        // ====================================================================
        // LEFT PANE: LCD Display & 5x5 Keypad
        // ====================================================================
        // LCD Display Box (width: 242, height: 54)
        let disp_rect = Rect::new(client.x + 8, client.y + 8, 242, 54);
        draw_rounded_rect(fb, disp_rect, 6, Color::rgb(14, 16, 20));
        draw_rounded_rect_outline(fb, disp_rect, 6, Color::rgb(48, 52, 62));

        // Sub-expression line (top right)
        if !self.expression_preview.is_empty() {
            let (ew, _eh) = measure_string(&self.expression_preview);
            let ex = (disp_rect.right() - 8).saturating_sub(ew as i32);
            draw_string(fb, ex.max(disp_rect.x + 8), disp_rect.y + 6, &self.expression_preview, Color::rgb(130, 140, 155), None);
        }

        // Active value line (bottom right, bold white or bright alert red on error)
        let (tw, _th) = measure_string(&self.display);
        let disp_x = (disp_rect.right() - 8).saturating_sub(tw as i32);
        let text_color = if self.is_error { Color::rgb(255, 85, 85) } else { Color::WHITE };
        draw_string(fb, disp_x.max(disp_rect.x + 8), disp_rect.y + 28, &self.display, text_color, None);

        // 5x5 Keypad Grid
        let pad_x = client.x + 8;
        let pad_y = client.y + 72;
        let btn_w = 46;
        let btn_h = 34;
        let gap = 4;

        let grid = [
            ["C", "±", "√", "x²", "÷"],
            ["7", "8", "9", "1/x", "×"],
            ["4", "5", "6", "x^y", "-"],
            ["1", "2", "3", "π", "+"],
            ["0", ".", "%", "e", "="],
        ];

        for (row_idx, row) in grid.iter().enumerate() {
            for (col_idx, &label) in row.iter().enumerate() {
                let bx = pad_x + (col_idx as i32 * (btn_w + gap));
                let by = pad_y + (row_idx as i32 * (btn_h + gap));
                let btn_rect = Rect::new(bx, by, btn_w as u32, btn_h as u32);

                let bg_color = match label {
                    "÷" | "×" | "-" | "+" | "=" => Color::rgb(255, 149, 0), // Orange operators
                    "C" => Color::rgb(205, 60, 60),                          // Red clear
                    "±" | "%" | "√" | "x²" | "1/x" | "x^y" | "π" | "e" => Color::rgb(55, 60, 72), // Slate scientific
                    _ => Color::rgb(42, 45, 54),                              // Number keys
                };

                draw_rounded_rect(fb, btn_rect, 5, bg_color);
                draw_rounded_rect_outline(fb, btn_rect, 5, Color::rgb(70, 75, 88));

                let (lw, lh) = measure_string(label);
                let lx = bx + ((btn_w - lw as i32) / 2);
                let ly = by + ((btn_h - lh as i32) / 2);
                draw_string(fb, lx, ly, label, Color::WHITE, None);
            }
        }

        // ====================================================================
        // RIGHT PANE: Calculation History Tape (Paper Roll)
        // ====================================================================
        let tape_x = client.x + 266;
        let tape_w = client.width.saturating_sub(274);

        // Header: "History Tape" + [ Clear ]
        draw_string(fb, tape_x, client.y + 10, "History Tape", Color::rgb(170, 180, 195), None);

        let clear_rect = Rect::new(client.right() - 60, client.y + 7, 52, 18);
        draw_rounded_rect(fb, clear_rect, 3, Color::rgb(45, 48, 58));
        draw_rounded_rect_outline(fb, clear_rect, 3, Color::rgb(65, 70, 82));
        draw_string(fb, client.right() - 54, client.y + 9, "Clear", Color::TEXT_DIM, None);

        // History Paper Roll Background
        let tape_body_rect = Rect::new(tape_x, client.y + 32, tape_w, client.height.saturating_sub(40));
        draw_rounded_rect(fb, tape_body_rect, 4, Color::rgb(20, 22, 27));
        draw_rounded_rect_outline(fb, tape_body_rect, 4, Color::rgb(40, 44, 52));

        if self.history.is_empty() {
            draw_string(fb, tape_x + 12, client.y + 50, "Tape empty.", Color::TEXT_DIM, None);
            draw_string(fb, tape_x + 12, client.y + 70, "Click [=] to save", Color::TEXT_DIM, None);
            draw_string(fb, tape_x + 12, client.y + 88, "calculations.", Color::TEXT_DIM, None);
        } else {
            let mut hy = client.y + 38;
            for (_idx, entry) in self.history.iter().enumerate().rev() {
                if hy + 30 > tape_body_rect.bottom() as i32 {
                    break;
                }
                // Row item
                let row_rect = Rect::new(tape_x + 4, hy, tape_w - 8, 28);
                draw_rounded_rect(fb, row_rect, 3, Color::rgb(28, 31, 38));

                // Expression line
                draw_string(fb, tape_x + 8, hy + 2, &entry.expression, Color::rgb(130, 140, 155), None);
                // Result line (right aligned)
                let res_str = format!("= {}", self.format_num(entry.result));
                let (rw, _rh) = measure_string(&res_str);
                let rx = (tape_x + tape_w as i32 - 12).saturating_sub(rw as i32);
                draw_string(fb, rx, hy + 14, &res_str, Color::rgb(80, 200, 120), None);

                hy += 32;
            }
        }
    }
}
