//! AegisPad Lightweight Text Editor Application for AegisOS
//!
//! Features multiline text editing, line number gutter, cursor navigation,
//! character insertions, line joins/splits, and bottom status telemetry.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::drivers::framebuffer::Framebuffer;
use crate::drivers::ps2_keyboard::{KeyCode, KeyEvent};
use crate::gui::font::draw_string;
use crate::gui::primitives::{draw_rect, draw_rounded_rect, Color, Rect};
use crate::gui::window::Window;

pub struct EditorApp {
    pub lines: Vec<String>,
    pub cursor_row: usize,
    pub cursor_col: usize,
    pub scroll_offset: usize,
    pub filename: &'static str,
}

impl EditorApp {
    pub fn new() -> Self {
        let mut lines = Vec::new();
        lines.push("Welcome to AegisPad on AegisOS!".to_string());
        lines.push("".to_string());
        lines.push("Key Features:".to_string());
        lines.push("1. Ring 0 / Ring 3 hardware memory isolation".to_string());
        lines.push("2. Crash-resilient fault recovery without desktop freezes".to_string());
        lines.push("3. macOS-inspired double-buffered 60 FPS compositor".to_string());
        lines.push("4. Ultralight memory footprint (< 60MB RAM at idle)".to_string());
        lines.push("".to_string());
        lines.push("Type anywhere to edit this file...".to_string());

        Self {
            lines,
            cursor_row: 8,
            cursor_col: 0,
            scroll_offset: 0,
            filename: "welcome.txt",
        }
    }

    /// Handles keyboard events when the text editor has active focus.
    pub fn handle_key(&mut self, event: KeyEvent) {
        if !event.pressed {
            return;
        }

        match event.code {
            KeyCode::Enter => {
                if self.cursor_row < self.lines.len() {
                    let curr_line = &self.lines[self.cursor_row];
                    let col = self.cursor_col.min(curr_line.len());
                    let (left, right) = curr_line.split_at(col);
                    let left_str = left.to_string();
                    let right_str = right.to_string();

                    self.lines[self.cursor_row] = left_str;
                    self.lines.insert(self.cursor_row + 1, right_str);
                    self.cursor_row += 1;
                    self.cursor_col = 0;
                }
            }
            KeyCode::Backspace => {
                if self.cursor_col > 0 {
                    if self.cursor_row < self.lines.len() {
                        let mut line = self.lines[self.cursor_row].clone();
                        if self.cursor_col <= line.len() {
                            line.remove(self.cursor_col - 1);
                            self.lines[self.cursor_row] = line;
                            self.cursor_col -= 1;
                        }
                    }
                } else if self.cursor_row > 0 {
                    let curr_line = self.lines.remove(self.cursor_row);
                    self.cursor_row -= 1;
                    let prev_len = self.lines[self.cursor_row].len();
                    self.lines[self.cursor_row].push_str(&curr_line);
                    self.cursor_col = prev_len;
                }
            }
            KeyCode::Delete => {
                if self.cursor_row < self.lines.len() {
                    let line_len = self.lines[self.cursor_row].len();
                    if self.cursor_col < line_len {
                        let mut line = self.lines[self.cursor_row].clone();
                        line.remove(self.cursor_col);
                        self.lines[self.cursor_row] = line;
                    } else if self.cursor_row + 1 < self.lines.len() {
                        let next_line = self.lines.remove(self.cursor_row + 1);
                        self.lines[self.cursor_row].push_str(&next_line);
                    }
                }
            }
            KeyCode::Up => {
                if self.cursor_row > 0 {
                    self.cursor_row -= 1;
                    self.clamp_cursor();
                }
            }
            KeyCode::Down => {
                if self.cursor_row + 1 < self.lines.len() {
                    self.cursor_row += 1;
                    self.clamp_cursor();
                }
            }
            KeyCode::Left => {
                if self.cursor_col > 0 {
                    self.cursor_col -= 1;
                } else if self.cursor_row > 0 {
                    self.cursor_row -= 1;
                    self.cursor_col = self.lines[self.cursor_row].len();
                }
            }
            KeyCode::Right => {
                if self.cursor_row < self.lines.len() {
                    let line_len = self.lines[self.cursor_row].len();
                    if self.cursor_col < line_len {
                        self.cursor_col += 1;
                    } else if self.cursor_row + 1 < self.lines.len() {
                        self.cursor_row += 1;
                        self.cursor_col = 0;
                    }
                }
            }
            KeyCode::Printable(c) => {
                self.insert_char(c as char);
            }
            _ => {
                if let Some(c) = event.char_byte {
                    if (32..=126).contains(&c) {
                        self.insert_char(c as char);
                    }
                }
            }
        }
    }

    fn insert_char(&mut self, c: char) {
        if self.cursor_row >= self.lines.len() {
            self.lines.push(String::new());
            self.cursor_row = self.lines.len() - 1;
            self.cursor_col = 0;
        }

        let mut line = self.lines[self.cursor_row].clone();
        let col = self.cursor_col.min(line.len());
        line.insert(col, c);
        self.lines[self.cursor_row] = line;
        self.cursor_col += 1;
    }

    fn clamp_cursor(&mut self) {
        if self.cursor_row < self.lines.len() {
            let max_col = self.lines[self.cursor_row].len();
            if self.cursor_col > max_col {
                self.cursor_col = max_col;
            }
        } else {
            self.cursor_col = 0;
        }
    }

    /// Renders AegisPad inside the window client area.
    pub fn render(&self, win: &Window, fb: &mut Framebuffer) {
        let client = win.client_rect();
        if client.width < 200 || client.height < 150 {
            return;
        }

        // 1. Top Action Bar: [ New ] [ Clear ]
        let bar_h = 24;
        let bar_rect = Rect::new(client.x, client.y, client.width, bar_h);
        draw_rect(fb, bar_rect, Color::rgb(36, 40, 48));
        draw_rect(fb, Rect::new(client.x, client.y + bar_h as i32 - 1, client.width, 1), Color::WINDOW_BORDER);

        draw_rounded_rect(fb, Rect::new(client.x + 8, client.y + 3, 50, 18), 3, Color::BUTTON_BG);
        draw_string(fb, client.x + 16, client.y + 4, "New", Color::WHITE, None);

        draw_rounded_rect(fb, Rect::new(client.x + 64, client.y + 3, 58, 18), 3, Color::BUTTON_BG);
        draw_string(fb, client.x + 72, client.y + 4, "Clear", Color::WHITE, None);

        // 2. Editor Body & Line Number Gutter
        let gutter_w = 40;
        let body_y = client.y + bar_h as i32;
        let body_h = client.height.saturating_sub(bar_h + 20); // 20px status bar
        let line_height = 18;
        let max_visible = (body_h as usize) / line_height;

        // Gutter background
        let gutter_rect = Rect::new(client.x, body_y, gutter_w, body_h);
        draw_rect(fb, gutter_rect, Color::rgb(26, 29, 36));
        draw_rect(fb, Rect::new(client.x + gutter_w as i32 - 1, body_y, 1, body_h), Color::WINDOW_BORDER);

        // Text area background
        let text_rect = Rect::new(client.x + gutter_w as i32, body_y, client.width.saturating_sub(gutter_w), body_h);
        draw_rect(fb, text_rect, Color::rgb(30, 34, 42));

        // Adjust scroll offset to keep cursor visible
        let scroll = if self.cursor_row >= self.scroll_offset + max_visible {
            self.cursor_row - max_visible + 1
        } else if self.cursor_row < self.scroll_offset {
            self.cursor_row
        } else {
            self.scroll_offset
        };

        for i in 0..max_visible {
            let row = scroll + i;
            if row >= self.lines.len() {
                break;
            }

            let ly = body_y + 4 + (i as i32 * line_height as i32);

            // Gutter line number
            let line_num_str = format!("{:>3} ", row + 1);
            draw_string(fb, client.x + 4, ly, &line_num_str, Color::rgb(90, 100, 120), None);

            // Line text
            let text = &self.lines[row];
            draw_string(fb, client.x + gutter_w as i32 + 8, ly, text, Color::TEXT_PRIMARY, None);

            // Draw cursor if this is active row
            if row == self.cursor_row && win.is_focused {
                let cur_x = client.x + gutter_w as i32 + 8 + (self.cursor_col as i32 * 8);
                draw_rect(fb, Rect::new(cur_x, ly, 2, 16), Color::WHITE);
            }
        }

        // 3. Bottom Status Bar
        let status_y = client.bottom() - 20;
        let status_rect = Rect::new(client.x, status_y, client.width, 20);
        draw_rect(fb, status_rect, Color::rgb(26, 29, 36));
        draw_rect(fb, Rect::new(client.x, status_y, client.width, 1), Color::WINDOW_BORDER);

        let total_chars: usize = self.lines.iter().map(|l| l.len()).sum();
        let status_text = format!(
            "Line: {}, Col: {} | {} chars | UTF-8 | {}",
            self.cursor_row + 1,
            self.cursor_col + 1,
            total_chars,
            self.filename
        );
        draw_string(fb, client.x + 8, status_y + 2, &status_text, Color::TEXT_DIM, None);
    }
}
