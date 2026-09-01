//! macOS-Style Graphical Calculator Application for AegisOS
//!
//! Compact floating window (260x340) with clickable buttons for 0-9,
//! arithmetic operators (+, -, *, /), decimal, equals, clear, sign inversion,
//! and keyboard numpad/digit support.

use alloc::string::{String, ToString};
use crate::drivers::framebuffer::Framebuffer;
use crate::drivers::ps2_keyboard::{KeyCode, KeyEvent};
use crate::gui::font::{draw_string, measure_string};
use crate::gui::primitives::{draw_rect, draw_rect_outline, draw_rounded_rect, Color, Rect};
use crate::gui::window::Window;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CalcOp {
    Add,
    Subtract,
    Multiply,
    Divide,
}

pub struct CalculatorApp {
    pub display: String,
    pub accumulator: f64,
    pub pending_op: Option<CalcOp>,
    pub clear_on_next_digit: bool,
}

impl CalculatorApp {
    pub fn new() -> Self {
        Self {
            display: "0".to_string(),
            accumulator: 0.0,
            pending_op: None,
            clear_on_next_digit: false,
        }
    }

    pub fn input_digit(&mut self, digit: char) {
        if self.clear_on_next_digit || self.display == "0" {
            self.display.clear();
            self.clear_on_next_digit = false;
        }
        if self.display.len() < 12 {
            self.display.push(digit);
        }
    }

    pub fn input_dot(&mut self) {
        if self.clear_on_next_digit {
            self.display = "0.".to_string();
            self.clear_on_next_digit = false;
            return;
        }
        if !self.display.contains('.') && self.display.len() < 11 {
            self.display.push('.');
        }
    }

    pub fn clear(&mut self) {
        self.display = "0".to_string();
        self.accumulator = 0.0;
        self.pending_op = None;
        self.clear_on_next_digit = false;
    }

    pub fn toggle_sign(&mut self) {
        if self.display != "0" {
            if self.display.starts_with('-') {
                self.display.remove(0);
            } else {
                self.display.insert(0, '-');
            }
        }
    }

    pub fn percent(&mut self) {
        let val = self.parse_display();
        let res = val / 100.0;
        self.set_display_num(res);
    }

    pub fn set_operator(&mut self, op: CalcOp) {
        let val = self.parse_display();
        if let Some(prev_op) = self.pending_op {
            if !self.clear_on_next_digit {
                self.accumulator = self.apply_op(self.accumulator, val, prev_op);
                self.set_display_num(self.accumulator);
            }
        } else {
            self.accumulator = val;
        }
        self.pending_op = Some(op);
        self.clear_on_next_digit = true;
    }

    pub fn equals(&mut self) {
        if let Some(op) = self.pending_op {
            let val = self.parse_display();
            let res = self.apply_op(self.accumulator, val, op);
            self.set_display_num(res);
            self.accumulator = res;
            self.pending_op = None;
            self.clear_on_next_digit = true;
        }
    }

    fn apply_op(&self, a: f64, b: f64, op: CalcOp) -> f64 {
        match op {
            CalcOp::Add => a + b,
            CalcOp::Subtract => a - b,
            CalcOp::Multiply => a * b,
            CalcOp::Divide => {
                if b == 0.0 {
                    0.0
                } else {
                    a / b
                }
            }
        }
    }

    fn parse_display(&self) -> f64 {
        // Simple manual float parse to avoid external std dependencies
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

    fn set_display_num(&mut self, mut num: f64) {
        let neg = num < 0.0;
        if neg { num = -num; }

        let int_part = num as u64;
        let frac_part = ((num - int_part as f64) * 1000.0) as u64;

        let mut res = String::new();
        if neg && (int_part > 0 || frac_part > 0) {
            res.push('-');
        }

        // Convert int part
        let mut int_buf = [0u8; 20];
        let mut len = 0;
        let mut v = int_part;
        if v == 0 {
            res.push('0');
        } else {
            while v > 0 && len < 20 {
                int_buf[len] = b'0' + (v % 10) as u8;
                v /= 10;
                len += 1;
            }
            for i in 0..len {
                res.push(int_buf[len - 1 - i] as char);
            }
        }

        if frac_part > 0 {
            res.push('.');
            let mut f = frac_part;
            let d3 = f % 10; f /= 10;
            let d2 = f % 10; f /= 10;
            let d1 = f % 10;
            res.push((b'0' + d1 as u8) as char);
            if d2 > 0 || d3 > 0 {
                res.push((b'0' + d2 as u8) as char);
            }
            if d3 > 0 {
                res.push((b'0' + d3 as u8) as char);
            }
        }

        self.display = res;
    }

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
                '=' => self.equals(),
                'c' | 'C' => self.clear(),
                '%' => self.percent(),
                _ => {}
            },
            KeyCode::Enter => self.equals(),
            KeyCode::Backspace => {
                if self.display.len() > 1 {
                    self.display.pop();
                } else {
                    self.display = "0".to_string();
                }
            }
            _ => {}
        }
    }

    pub fn handle_click(&mut self, win: &Window, x: i32, y: i32) {
        let client = win.client_rect();
        let pad_x = client.x + 8;
        let pad_y = client.y + 55;
        let btn_w = (client.width - 24) / 4;
        let btn_h = 36;
        let gap = 4;

        let grid = [
            ["C", "±", "%", "÷"],
            ["7", "8", "9", "×"],
            ["4", "5", "6", "-"],
            ["1", "2", "3", "+"],
            ["0", "0", ".", "="],
        ];

        for (row_idx, row) in grid.iter().enumerate() {
            for (col_idx, &label) in row.iter().enumerate() {
                let bx = pad_x + (col_idx as u32 * (btn_w + gap)) as i32;
                let by = pad_y + (row_idx as u32 * (btn_h + gap)) as i32;
                let bw = if row_idx == 4 && col_idx == 0 {
                    btn_w * 2 + gap
                } else if row_idx == 4 && col_idx == 1 {
                    continue; // Skip 2nd half of 0 button
                } else {
                    btn_w
                };

                let btn_rect = Rect::new(bx, by, bw, btn_h);
                if btn_rect.contains(x, y) {
                    match label {
                        "0" | "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" => {
                            self.input_digit(label.chars().next().unwrap());
                        }
                        "." => self.input_dot(),
                        "C" => self.clear(),
                        "±" => self.toggle_sign(),
                        "%" => self.percent(),
                        "+" => self.set_operator(CalcOp::Add),
                        "-" => self.set_operator(CalcOp::Subtract),
                        "×" => self.set_operator(CalcOp::Multiply),
                        "÷" => self.set_operator(CalcOp::Divide),
                        "=" => self.equals(),
                        _ => {}
                    }
                    return;
                }
            }
        }
    }

    pub fn render(&self, win: &Window, fb: &mut Framebuffer) {
        let client = win.client_rect();
        if client.width < 180 || client.height < 220 {
            return;
        }

        // Dark modern background
        draw_rect(fb, client, Color::rgb(30, 32, 38));

        // Display area
        let disp_rect = Rect::new(client.x + 8, client.y + 8, client.width - 16, 40);
        draw_rounded_rect(fb, disp_rect, 6, Color::rgb(18, 20, 24));
        draw_rect_outline(fb, disp_rect, Color::rgb(50, 55, 65), 1);

        let (tw, _th) = measure_string(&self.display);
        let disp_x = (disp_rect.right() - 10).saturating_sub(tw as i32);
        draw_string(fb, disp_x.max(disp_rect.x + 6), disp_rect.y + 12, &self.display, Color::WHITE, None);

        // Buttons grid
        let pad_x = client.x + 8;
        let pad_y = client.y + 55;
        let btn_w = (client.width - 24) / 4;
        let btn_h = 36;
        let gap = 4;

        let grid = [
            ["C", "±", "%", "÷"],
            ["7", "8", "9", "×"],
            ["4", "5", "6", "-"],
            ["1", "2", "3", "+"],
            ["0", "0", ".", "="],
        ];

        for (row_idx, row) in grid.iter().enumerate() {
            for (col_idx, &label) in row.iter().enumerate() {
                let bx = pad_x + (col_idx as u32 * (btn_w + gap)) as i32;
                let by = pad_y + (row_idx as u32 * (btn_h + gap)) as i32;
                let bw = if row_idx == 4 && col_idx == 0 {
                    btn_w * 2 + gap
                } else if row_idx == 4 && col_idx == 1 {
                    continue;
                } else {
                    btn_w
                };

                let btn_rect = Rect::new(bx, by, bw, btn_h);

                let bg_color = match label {
                    "÷" | "×" | "-" | "+" | "=" => Color::rgb(255, 149, 0), // Orange operators
                    "C" | "±" | "%" => Color::rgb(65, 70, 80),            // Gray functional
                    _ => Color::rgb(50, 54, 62),                           // Digits
                };

                draw_rounded_rect(fb, btn_rect, 6, bg_color);
                draw_rect_outline(fb, btn_rect, Color::rgb(70, 75, 88), 1);

                let (lw, lh) = measure_string(label);
                let lx = bx + ((bw as i32 - lw as i32) / 2);
                let ly = by + ((btn_h as i32 - lh as i32) / 2);
                draw_string(fb, lx, ly, label, Color::WHITE, None);
            }
        }
    }
}
