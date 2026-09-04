//! AegisPad 2.0 Advanced Multi-Tab Syntax & Code Editor for AegisOS
//!
//! Features multi-document tabs with dirty tracking, line number gutter,
//! active line highlight bar, real-time keyword/comment/string/number syntax
//! highlighting, integrated find bar (`Ctrl+F`) with match navigation, and
//! in-memory RAM disk VFS persistence (`Ctrl+S`, `Ctrl+N`, `Ctrl+W`).

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use crate::drivers::framebuffer::Framebuffer;
use crate::drivers::ps2_keyboard::{KeyCode, KeyEvent};
use crate::gui::font::{draw_char, draw_string, FONT_HEIGHT, FONT_WIDTH};
use crate::gui::primitives::{draw_rect, draw_rounded_rect, draw_rounded_rect_outline, Color, Rect};
use crate::gui::window::Window;

/// A single open document buffer inside AegisPad.
#[derive(Clone)]
pub struct DocumentTab {
    pub path: String,
    pub title: String,
    pub lines: Vec<String>,
    pub cursor_row: usize,
    pub cursor_col: usize,
    pub scroll_offset: usize,
    pub dirty: bool,
}

impl DocumentTab {
    pub fn new(path: String, title: String, lines: Vec<String>) -> Self {
        let cursor_row = lines.len().saturating_sub(1);
        Self {
            path,
            title,
            lines,
            cursor_row,
            cursor_col: 0,
            scroll_offset: 0,
            dirty: false,
        }
    }

    pub fn from_path(path: &str) -> Self {
        let path_str = path.to_string();
        let title = path.rsplit('/').next().unwrap_or(path).to_string();

        let lines = if let Ok(content) = crate::fs::read_to_string(path) {
            let parsed: Vec<String> = content.lines().map(|l| l.to_string()).collect();
            if parsed.is_empty() {
                vec![String::new()]
            } else {
                parsed
            }
        } else {
            let mut l = Vec::new();
            l.push("Welcome to AegisPad 2.0 on AegisOS!".to_string());
            l.push("".to_string());
            l.push("// Key Features:".to_string());
            l.push("fn main() {".to_string());
            l.push("    let memory_isolated = true;".to_string());
            l.push("    let max_ram_mb = 60;".to_string());
            l.push("    println!(\"AegisOS Ring 0/Ring 3 Active\");".to_string());
            l.push("}".to_string());
            l.push("".to_string());
            l.push("// Press Ctrl+F to search, Ctrl+N for new tab, Ctrl+S to save".to_string());
            l
        };

        Self::new(path_str, title, lines)
    }
}

pub struct EditorApp {
    pub tabs: Vec<DocumentTab>,
    pub active_tab: usize,
    pub find_active: bool,
    pub find_query: String,
    pub find_matches: Vec<(usize, usize)>, // (row, col)
    pub find_match_idx: usize,
    pub status_message: Option<String>,
}

impl EditorApp {
    pub fn new() -> Self {
        let initial_tab = DocumentTab::from_path("/welcome.txt");
        Self {
            tabs: vec![initial_tab],
            active_tab: 0,
            find_active: false,
            find_query: String::new(),
            find_matches: Vec::new(),
            find_match_idx: 0,
            status_message: Some("Ready".to_string()),
        }
    }

    /// Returns a mutable reference to the currently active document tab.
    pub fn active_tab_mut(&mut self) -> &mut DocumentTab {
        &mut self.tabs[self.active_tab]
    }

    /// Returns a reference to the currently active document tab.
    pub fn active_tab(&self) -> &DocumentTab {
        &self.tabs[self.active_tab]
    }

    /// Loads a specific file path into a tab (switches if already open).
    pub fn open_path(&mut self, path: &str) -> bool {
        // If already open in an existing tab, switch to it
        if let Some(idx) = self.tabs.iter().position(|t| t.path == path) {
            self.active_tab = idx;
            self.status_message = Some(format!("Switched to: {}", path));
            return true;
        }

        // Otherwise open in a new tab
        if let Ok(content) = crate::fs::read_to_string(path) {
            let mut lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();
            if lines.is_empty() {
                lines.push(String::new());
            }
            let title = path.rsplit('/').next().unwrap_or(path).to_string();
            let new_tab = DocumentTab::new(path.to_string(), title, lines);
            self.tabs.push(new_tab);
            self.active_tab = self.tabs.len() - 1;
            self.status_message = Some(format!("Opened: {}", path));
            true
        } else {
            false
        }
    }

    /// Creates a new untitled document tab.
    pub fn new_tab(&mut self, path: &str, content: Option<&str>) {
        let title = path.rsplit('/').next().unwrap_or(path).to_string();
        let lines = if let Some(c) = content {
            c.lines().map(|l| l.to_string()).collect()
        } else {
            vec![String::new()]
        };
        let new_tab = DocumentTab::new(path.to_string(), title, lines);
        self.tabs.push(new_tab);
        self.active_tab = self.tabs.len() - 1;
        self.status_message = Some(format!("New buffer: {}", path));
    }

    /// Closes a document tab by index.
    pub fn close_tab(&mut self, idx: usize) {
        if idx < self.tabs.len() && self.tabs.len() > 1 {
            self.tabs.remove(idx);
            if self.active_tab >= self.tabs.len() {
                self.active_tab = self.tabs.len() - 1;
            }
            self.status_message = Some("Tab closed".to_string());
        }
    }

    /// Persists active document to the VFS.
    pub fn save_current(&mut self) -> bool {
        let tab = &mut self.tabs[self.active_tab];
        let content = tab.lines.join("\n");
        let res = crate::fs::write_file(&tab.path, content.as_bytes());
        if res.is_ok() {
            tab.dirty = false;
            self.status_message = Some(format!("Saved: {}", tab.path));
            crate::drivers::speaker::play_sound_effect(crate::drivers::speaker::SoundEffect::SnakeEat);
            true
        } else {
            self.status_message = Some("Error: Save failed".to_string());
            crate::drivers::speaker::play_sound_effect(crate::drivers::speaker::SoundEffect::Alert);
            false
        }
    }

    /// Updates search matches across the active tab for `find_query`.
    pub fn update_find_matches(&mut self) {
        self.find_matches.clear();
        self.find_match_idx = 0;

        if self.find_query.is_empty() {
            return;
        }

        let query_lower = self.find_query.to_lowercase();
        let tab = &self.tabs[self.active_tab];

        for (r, line) in tab.lines.iter().enumerate() {
            let line_lower = line.to_lowercase();
            let mut start = 0;
            while let Some(pos) = line_lower[start..].find(&query_lower) {
                let col = start + pos;
                self.find_matches.push((r, col));
                start = col + query_lower.len().max(1);
            }
        }

        // Jump cursor to first match if any
        if !self.find_matches.is_empty() {
            let (mr, mc) = self.find_matches[0];
            let tab = &mut self.tabs[self.active_tab];
            tab.cursor_row = mr;
            tab.cursor_col = mc;
        }
    }

    /// Jumps to the next search occurrence.
    pub fn next_match(&mut self) {
        if self.find_matches.is_empty() {
            return;
        }
        self.find_match_idx = (self.find_match_idx + 1) % self.find_matches.len();
        let (mr, mc) = self.find_matches[self.find_match_idx];
        let tab = &mut self.tabs[self.active_tab];
        tab.cursor_row = mr;
        tab.cursor_col = mc;
        crate::drivers::speaker::play_sound_effect(crate::drivers::speaker::SoundEffect::SnakeEat);
    }

    /// Jumps to the previous search occurrence.
    pub fn prev_match(&mut self) {
        if self.find_matches.is_empty() {
            return;
        }
        if self.find_match_idx == 0 {
            self.find_match_idx = self.find_matches.len() - 1;
        } else {
            self.find_match_idx -= 1;
        }
        let (mr, mc) = self.find_matches[self.find_match_idx];
        let tab = &mut self.tabs[self.active_tab];
        tab.cursor_row = mr;
        tab.cursor_col = mc;
        crate::drivers::speaker::play_sound_effect(crate::drivers::speaker::SoundEffect::SnakeEat);
    }

    /// Handles keyboard events when AegisPad has active focus.
    pub fn handle_key(&mut self, event: KeyEvent) {
        if !event.pressed {
            return;
        }

        // Global Editor Shortcuts:
        // Ctrl+F: Toggle find bar
        if event.ctrl && (event.char_byte == Some(b'f') || event.char_byte == Some(b'F')) {
            self.find_active = !self.find_active;
            if self.find_active {
                self.update_find_matches();
            }
            return;
        }

        // Ctrl+N: New tab
        if event.ctrl && (event.char_byte == Some(b'n') || event.char_byte == Some(b'N')) {
            let next_id = self.tabs.len() + 1;
            let path = format!("/user/untitled_{}.txt", next_id);
            self.new_tab(&path, None);
            return;
        }

        // Ctrl+W: Close active tab
        if event.ctrl && (event.char_byte == Some(b'w') || event.char_byte == Some(b'W')) {
            let active = self.active_tab;
            self.close_tab(active);
            return;
        }

        // Ctrl+S: Save active document
        if event.ctrl && (event.char_byte == Some(b's') || event.char_byte == Some(b'S')) {
            self.save_current();
            return;
        }

        // If Find Bar is active, route typing into find input
        if self.find_active {
            match event.code {
                KeyCode::Escape => {
                    self.find_active = false;
                    return;
                }
                KeyCode::Enter => {
                    self.next_match();
                    return;
                }
                KeyCode::Backspace => {
                    self.find_query.pop();
                    self.update_find_matches();
                    return;
                }
                _ => {
                    if let Some(c) = event.char_byte {
                        if (32..=126).contains(&c) && !event.ctrl {
                            self.find_query.push(c as char);
                            self.update_find_matches();
                            return;
                        }
                    }
                }
            }
        }

        // Standard text editing on active tab
        let tab = &mut self.tabs[self.active_tab];
        match event.code {
            KeyCode::Enter => {
                if tab.cursor_row < tab.lines.len() {
                    let curr_line = &tab.lines[tab.cursor_row];
                    let col = tab.cursor_col.min(curr_line.len());
                    let (left, right) = curr_line.split_at(col);
                    let left_str = left.to_string();
                    let right_str = right.to_string();

                    tab.lines[tab.cursor_row] = left_str;
                    tab.lines.insert(tab.cursor_row + 1, right_str);
                    tab.cursor_row += 1;
                    tab.cursor_col = 0;
                    tab.dirty = true;
                }
            }
            KeyCode::Backspace => {
                if tab.cursor_col > 0 {
                    if tab.cursor_row < tab.lines.len() {
                        let mut line = tab.lines[tab.cursor_row].clone();
                        if tab.cursor_col <= line.len() {
                            line.remove(tab.cursor_col - 1);
                            tab.lines[tab.cursor_row] = line;
                            tab.cursor_col -= 1;
                            tab.dirty = true;
                        }
                    }
                } else if tab.cursor_row > 0 {
                    let curr_line = tab.lines.remove(tab.cursor_row);
                    tab.cursor_row -= 1;
                    let prev_len = tab.lines[tab.cursor_row].len();
                    tab.lines[tab.cursor_row].push_str(&curr_line);
                    tab.cursor_col = prev_len;
                    tab.dirty = true;
                }
            }
            KeyCode::Delete => {
                if tab.cursor_row < tab.lines.len() {
                    let line_len = tab.lines[tab.cursor_row].len();
                    if tab.cursor_col < line_len {
                        let mut line = tab.lines[tab.cursor_row].clone();
                        line.remove(tab.cursor_col);
                        tab.lines[tab.cursor_row] = line;
                        tab.dirty = true;
                    } else if tab.cursor_row + 1 < tab.lines.len() {
                        let next_line = tab.lines.remove(tab.cursor_row + 1);
                        tab.lines[tab.cursor_row].push_str(&next_line);
                        tab.dirty = true;
                    }
                }
            }
            KeyCode::Up => {
                if tab.cursor_row > 0 {
                    tab.cursor_row -= 1;
                    Self::clamp_tab_cursor(tab);
                }
            }
            KeyCode::Down => {
                if tab.cursor_row + 1 < tab.lines.len() {
                    tab.cursor_row += 1;
                    Self::clamp_tab_cursor(tab);
                }
            }
            KeyCode::Left => {
                if tab.cursor_col > 0 {
                    tab.cursor_col -= 1;
                } else if tab.cursor_row > 0 {
                    tab.cursor_row -= 1;
                    tab.cursor_col = tab.lines[tab.cursor_row].len();
                }
            }
            KeyCode::Right => {
                if tab.cursor_row < tab.lines.len() {
                    let line_len = tab.lines[tab.cursor_row].len();
                    if tab.cursor_col < line_len {
                        tab.cursor_col += 1;
                    } else if tab.cursor_row + 1 < tab.lines.len() {
                        tab.cursor_row += 1;
                        tab.cursor_col = 0;
                    }
                }
            }
            KeyCode::Printable(c) => {
                Self::insert_char_in_tab(tab, c as char);
            }
            _ => {
                if let Some(c) = event.char_byte {
                    if (32..=126).contains(&c) && !event.ctrl {
                        Self::insert_char_in_tab(tab, c as char);
                    }
                }
            }
        }
    }

    fn insert_char_in_tab(tab: &mut DocumentTab, c: char) {
        if tab.cursor_row >= tab.lines.len() {
            tab.lines.push(String::new());
            tab.cursor_row = tab.lines.len() - 1;
            tab.cursor_col = 0;
        }

        let mut line = tab.lines[tab.cursor_row].clone();
        let col = tab.cursor_col.min(line.len());
        line.insert(col, c);
        tab.lines[tab.cursor_row] = line;
        tab.cursor_col += 1;
        tab.dirty = true;
    }

    fn clamp_tab_cursor(tab: &mut DocumentTab) {
        if tab.cursor_row < tab.lines.len() {
            let max_col = tab.lines[tab.cursor_row].len();
            if tab.cursor_col > max_col {
                tab.cursor_col = max_col;
            }
        } else {
            tab.cursor_col = 0;
        }
    }

    /// Handles mouse clicks on the tab strip, action toolbar, or find bar.
    pub fn handle_click(&mut self, win: &Window, x: i32, y: i32) -> bool {
        let client = win.client_rect();

        // 1. Tab Strip Click (y = client.y .. client.y + 24)
        if y >= client.y && y < client.y + 24 {
            let mut curr_x = client.x + 4;
            for (idx, tab) in self.tabs.iter().enumerate() {
                let tab_w = 90i32.max((tab.title.len() as i32 * 8) + 32);
                let tab_rect = Rect::new(curr_x, client.y + 2, tab_w as u32, 20);

                if tab_rect.contains(x, y) {
                    // Check close button [x]
                    let close_x = curr_x + tab_w - 18;
                    if x >= close_x && x <= close_x + 14 {
                        self.close_tab(idx);
                    } else {
                        self.active_tab = idx;
                    }
                    return true;
                }
                curr_x += tab_w + 4;
            }

            // [ + ] New tab button
            let plus_rect = Rect::new(curr_x, client.y + 2, 24, 20);
            if plus_rect.contains(x, y) {
                let next_id = self.tabs.len() + 1;
                let path = format!("/user/untitled_{}.txt", next_id);
                self.new_tab(&path, None);
                return true;
            }
        }

        // 2. Toolbar Action Bar Click (y = client.y + 24 .. client.y + 48)
        let bar_y = client.y + 24;
        if y >= bar_y && y < bar_y + 24 {
            let rel_x = x - client.x;
            if (8..=56).contains(&rel_x) {
                // [ New ]
                let next_id = self.tabs.len() + 1;
                let path = format!("/user/untitled_{}.txt", next_id);
                self.new_tab(&path, None);
                return true;
            } else if (62..=116).contains(&rel_x) {
                // [ Open ] -> Cycles through available VFS files
                let all_files = crate::fs::get_all_file_paths();
                if !all_files.is_empty() {
                    let curr_path = self.active_tab().path.clone();
                    let next_file = if let Some(idx) = all_files.iter().position(|p| p == &curr_path) {
                        all_files[(idx + 1) % all_files.len()].clone()
                    } else {
                        all_files[0].clone()
                    };
                    self.open_path(&next_file);
                }
                return true;
            } else if (122..=176).contains(&rel_x) {
                // [ Save ] -> Persists current lines to VFS
                self.save_current();
                return true;
            } else if (182..=236).contains(&rel_x) {
                // [ Find ] -> Toggles find bar
                self.find_active = !self.find_active;
                if self.find_active {
                    self.update_find_matches();
                }
                return true;
            } else if (242..=296).contains(&rel_x) {
                // [ Clear ]
                let tab = &mut self.tabs[self.active_tab];
                tab.lines.clear();
                tab.lines.push(String::new());
                tab.cursor_row = 0;
                tab.cursor_col = 0;
                tab.scroll_offset = 0;
                tab.dirty = true;
                self.status_message = Some("Buffer Cleared".to_string());
                return true;
            }
        }

        // 3. Find Bar Controls Click (if find bar is active)
        if self.find_active && y >= client.y + 48 && y < client.y + 72 {
            let find_y = client.y + 48;
            // [ Prev ] button (around x = client.x + 240)
            let prev_rect = Rect::new(client.x + 240, find_y + 3, 36, 18);
            if prev_rect.contains(x, y) {
                self.prev_match();
                return true;
            }
            // [ Next ] button (around x = client.x + 280)
            let next_rect = Rect::new(client.x + 280, find_y + 3, 36, 18);
            if next_rect.contains(x, y) {
                self.next_match();
                return true;
            }
            // [ Done ] button (around x = client.x + 324)
            let done_rect = Rect::new(client.x + 324, find_y + 3, 44, 18);
            if done_rect.contains(x, y) {
                self.find_active = false;
                return true;
            }
        }

        false
    }

    /// Renders AegisPad 2.0 inside the window client area.
    pub fn render(&mut self, win: &Window, fb: &mut Framebuffer) {
        let client = win.client_rect();
        if client.width < 200 || client.height < 150 {
            return;
        }

        // ── 1. Top Document Tab Strip (24px) ──
        let tab_bar_rect = Rect::new(client.x, client.y, client.width, 24);
        draw_rect(fb, tab_bar_rect, Color::rgb(22, 25, 32));
        draw_rect(fb, Rect::new(client.x, client.y + 23, client.width, 1), Color::WINDOW_BORDER);

        let mut curr_x = client.x + 4;
        for (idx, tab) in self.tabs.iter().enumerate() {
            let tab_w = 90i32.max((tab.title.len() as i32 * 8) + 32);
            let is_active = idx == self.active_tab;
            let tab_bg = if is_active {
                Color::rgb(36, 40, 50)
            } else {
                Color::rgb(26, 29, 36)
            };
            let tab_rect = Rect::new(curr_x, client.y + 2, tab_w as u32, 21);
            draw_rounded_rect(fb, tab_rect, 4, tab_bg);
            if is_active {
                draw_rounded_rect_outline(fb, tab_rect, 4, Color::rgb(70, 140, 240));
            }

            // Title
            let text_color = if is_active { Color::WHITE } else { Color::TEXT_DIM };
            draw_string(fb, curr_x + 8, client.y + 5, &tab.title, text_color, None);

            // Dirty indicator dot
            if tab.dirty {
                draw_char(fb, curr_x + tab_w - 24, client.y + 5, b'*', Color::rgb(255, 189, 46), None);
            }

            // Close 'x' button
            draw_char(fb, curr_x + tab_w - 14, client.y + 5, b'x', Color::rgb(140, 150, 165), None);

            curr_x += tab_w + 4;
        }

        // [ + ] New tab button
        let plus_rect = Rect::new(curr_x, client.y + 2, 22, 21);
        draw_rounded_rect(fb, plus_rect, 4, Color::rgb(36, 40, 50));
        draw_char(fb, curr_x + 7, client.y + 5, b'+', Color::TEXT_PRIMARY, None);

        // ── 2. Action Toolbar (24px) ──
        let bar_y = client.y + 24;
        let bar_rect = Rect::new(client.x, bar_y, client.width, 24);
        draw_rect(fb, bar_rect, Color::rgb(30, 34, 42));
        draw_rect(fb, Rect::new(client.x, bar_y + 23, client.width, 1), Color::WINDOW_BORDER);

        // [ New ] (8..56)
        draw_rounded_rect(fb, Rect::new(client.x + 8, bar_y + 3, 48, 18), 3, Color::BUTTON_BG);
        draw_string(fb, client.x + 16, bar_y + 4, "New", Color::WHITE, None);

        // [ Open ] (62..116)
        draw_rounded_rect(fb, Rect::new(client.x + 62, bar_y + 3, 54, 18), 3, Color::BUTTON_BG);
        draw_string(fb, client.x + 70, bar_y + 4, "Open", Color::WHITE, None);

        // [ Save ] (122..176)
        draw_rounded_rect(fb, Rect::new(client.x + 122, bar_y + 3, 54, 18), 3, Color::rgb(40, 110, 70));
        draw_string(fb, client.x + 130, bar_y + 4, "Save", Color::WHITE, None);

        // [ Find ] (182..236)
        let find_bg = if self.find_active { Color::rgb(70, 130, 220) } else { Color::BUTTON_BG };
        draw_rounded_rect(fb, Rect::new(client.x + 182, bar_y + 3, 54, 18), 3, find_bg);
        draw_string(fb, client.x + 190, bar_y + 4, "Find", Color::WHITE, None);

        // [ Clear ] (242..296)
        draw_rounded_rect(fb, Rect::new(client.x + 242, bar_y + 3, 54, 18), 3, Color::BUTTON_BG);
        draw_string(fb, client.x + 250, bar_y + 4, "Clear", Color::WHITE, None);

        // ── 3. Find Bar Overlay (24px, if active) ──
        let mut top_offset = 48i32;
        if self.find_active {
            let find_y = client.y + 48;
            let find_rect = Rect::new(client.x, find_y, client.width, 24);
            draw_rect(fb, find_rect, Color::rgb(26, 30, 38));
            draw_rect(fb, Rect::new(client.x, find_y + 23, client.width, 1), Color::WINDOW_BORDER);

            draw_string(fb, client.x + 8, find_y + 4, "Find:", Color::TEXT_HIGHLIGHT, None);
            let input_rect = Rect::new(client.x + 52, find_y + 2, 130, 20);
            draw_rounded_rect(fb, input_rect, 3, Color::rgb(18, 20, 24));
            draw_rounded_rect_outline(fb, input_rect, 3, Color::rgb(70, 80, 100));
            draw_string(fb, client.x + 56, find_y + 4, &self.find_query, Color::WHITE, None);
            // Blinking cursor
            let qx = client.x + 56 + (self.find_query.len() as i32 * 8);
            draw_rect(fb, Rect::new(qx, find_y + 4, 2, 14), Color::WHITE);

            // Match count
            let count_str = if self.find_matches.is_empty() {
                "0 matches".to_string()
            } else {
                format!("{}/{}", self.find_match_idx + 1, self.find_matches.len())
            };
            draw_string(fb, client.x + 188, find_y + 4, &count_str, Color::TEXT_DIM, None);

            // [ < Prev ]
            draw_rounded_rect(fb, Rect::new(client.x + 240, find_y + 3, 36, 18), 3, Color::BUTTON_BG);
            draw_string(fb, client.x + 246, find_y + 4, "<", Color::WHITE, None);

            // [ Next > ]
            draw_rounded_rect(fb, Rect::new(client.x + 280, find_y + 3, 36, 18), 3, Color::BUTTON_BG);
            draw_string(fb, client.x + 286, find_y + 4, ">", Color::WHITE, None);

            // [ Done ]
            draw_rounded_rect(fb, Rect::new(client.x + 324, find_y + 3, 44, 18), 3, Color::BUTTON_BG);
            draw_string(fb, client.x + 330, find_y + 4, "Done", Color::WHITE, None);

            top_offset += 24;
        }

        // ── 4. Editor Body, Gutter & Syntax Highlighted Text ──
        let gutter_w = 40;
        let body_y = client.y + top_offset;
        let body_h = client.height.saturating_sub(top_offset as u32 + 20); // 20px status bar
        let line_height = 18;
        let max_visible = (body_h as usize) / line_height;

        // Gutter background
        let gutter_rect = Rect::new(client.x, body_y, gutter_w, body_h);
        draw_rect(fb, gutter_rect, Color::rgb(24, 27, 34));
        draw_rect(fb, Rect::new(client.x + gutter_w as i32 - 1, body_y, 1, body_h), Color::WINDOW_BORDER);

        // Text area background
        let text_rect = Rect::new(client.x + gutter_w as i32, body_y, client.width.saturating_sub(gutter_w), body_h);
        draw_rect(fb, text_rect, Color::rgb(30, 34, 42));

        let tab = &mut self.tabs[self.active_tab];

        // Adjust scroll offset
        let scroll = if tab.cursor_row >= tab.scroll_offset + max_visible {
            tab.cursor_row - max_visible + 1
        } else if tab.cursor_row < tab.scroll_offset {
            tab.cursor_row
        } else {
            tab.scroll_offset
        };
        tab.scroll_offset = scroll;

        let active_row = tab.cursor_row;
        let cursor_col = tab.cursor_col;
        let lines_count = tab.lines.len();

        for i in 0..max_visible {
            let row = scroll + i;
            if row >= lines_count {
                break;
            }

            let ly = body_y + 3 + (i as i32 * line_height as i32);

            // Active Line Highlight Bar across editor width
            if row == active_row && win.is_focused {
                draw_rect(
                    fb,
                    Rect::new(client.x + gutter_w as i32, ly - 2, client.width.saturating_sub(gutter_w), line_height as u32),
                    Color::rgb(38, 44, 56),
                );
            }

            // Gutter Line Number
            let line_num_str = format!("{:>3} ", row + 1);
            let num_color = if row == active_row { Color::rgb(100, 200, 240) } else { Color::rgb(80, 90, 110) };
            draw_string(fb, client.x + 4, ly, &line_num_str, num_color, None);

            // Syntax Highlighted Tokens on Line
            let line = &self.tabs[self.active_tab].lines[row];
            Self::render_syntax_line(fb, client.x + gutter_w as i32 + 8, ly, line);

            // Search Match Highlight Boxes
            if self.find_active && !self.find_query.is_empty() {
                for (mr, mc) in self.find_matches.iter() {
                    if *mr == row {
                        let mx = client.x + gutter_w as i32 + 8 + (*mc as i32 * FONT_WIDTH as i32);
                        let mw = (self.find_query.len() * FONT_WIDTH) as u32;
                        draw_rounded_rect(fb, Rect::new(mx, ly, mw, FONT_HEIGHT as u32), 2, Color::rgba(255, 215, 0, 70));
                    }
                }
            }

            // Cursor on Active Line
            if row == active_row && win.is_focused {
                let cur_x = client.x + gutter_w as i32 + 8 + (cursor_col as i32 * FONT_WIDTH as i32);
                draw_rect(fb, Rect::new(cur_x, ly, 2, 16), Color::WHITE);
            }
        }

        // ── 5. Bottom Telemetry Status Bar (20px) ──
        let status_y = client.bottom() - 20;
        let status_rect = Rect::new(client.x, status_y, client.width, 20);
        draw_rect(fb, status_rect, Color::rgb(24, 27, 34));
        draw_rect(fb, Rect::new(client.x, status_y, client.width, 1), Color::WINDOW_BORDER);

        let active_tab = &self.tabs[self.active_tab];
        let total_chars: usize = active_tab.lines.iter().map(|l| l.len()).sum();
        let dirty_str = if active_tab.dirty { " [Modified]" } else { "" };
        let msg = self.status_message.as_deref().unwrap_or("Ready");
        let status_text = format!(
            "Ln: {}, Col: {} | {} chars | {}{} | [{}]",
            active_tab.cursor_row + 1,
            active_tab.cursor_col + 1,
            total_chars,
            active_tab.path,
            dirty_str,
            msg
        );
        draw_string(fb, client.x + 8, status_y + 2, &status_text, Color::TEXT_DIM, None);
    }

    /// Fast line syntax highlighter for Rust/C/Shell code in AegisPad.
    fn render_syntax_line(fb: &mut Framebuffer, mut x: i32, y: i32, line: &str) {
        if line.is_empty() {
            return;
        }

        // Check for full line comments
        let trimmed = line.trim_start();
        if trimmed.starts_with("//") || trimmed.starts_with('#') {
            draw_string(fb, x, y, line, Color::rgb(95, 185, 105), None);
            return;
        }

        let words: Vec<&str> = line.split_inclusive(|c: char| !c.is_alphanumeric() && c != '_').collect();

        let mut in_string = false;

        for chunk in words {
            for ch in chunk.chars() {
                if ch == '"' {
                    in_string = !in_string;
                }
            }

            let color = if in_string {
                Color::rgb(100, 230, 245) // Cyan for strings
            } else if Self::is_keyword(chunk.trim_end_matches(|c: char| !c.is_alphanumeric())) {
                Color::rgb(255, 175, 60) // Amber/orange for keywords
            } else if chunk.chars().all(|c| c.is_ascii_digit() || !c.is_alphanumeric()) && chunk.chars().any(|c| c.is_ascii_digit()) {
                Color::rgb(215, 150, 255) // Lavender for numbers
            } else {
                Color::WHITE // White for normal tokens
            };

            x = draw_string(fb, x, y, chunk, color, None);
        }
    }

    /// Identifies common programming keywords for syntax highlighting.
    pub fn is_keyword(w: &str) -> bool {
        matches!(
            w,
            "fn" | "let"
                | "pub"
                | "struct"
                | "enum"
                | "impl"
                | "match"
                | "if"
                | "else"
                | "return"
                | "true"
                | "false"
                | "mut"
                | "use"
                | "mod"
                | "for"
                | "in"
                | "while"
                | "loop"
                | "type"
                | "trait"
                | "const"
                | "static"
        )
    }
}
