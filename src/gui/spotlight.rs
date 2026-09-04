//! macOS-Inspired Spotlight Universal Desktop Search for AegisOS
//!
//! Triggered by Ctrl+Space or menubar magnifying glass. Provides real-time
//! search across applications, VFS files, shell commands, and inline math.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::drivers::framebuffer::Framebuffer;
use crate::gui::dock::AppId;
use crate::gui::font::draw_string;
use crate::gui::font::FONT_WIDTH;
use crate::gui::primitives::{draw_rounded_rect, draw_rounded_rect_outline, draw_shadow, Color, Rect};

/// A search result entry.
#[derive(Clone)]
pub enum SearchResult {
    App(AppId, &'static str),
    File(String, usize),
    Command(&'static str, &'static str),
    MathResult(String, f64),
}

impl SearchResult {
    pub fn display_text(&self) -> String {
        match self {
            SearchResult::App(_, name) => format!("App: {}", name),
            SearchResult::File(path, size) => format!("File: {} ({} B)", path, size),
            SearchResult::Command(cmd, desc) => format!("Cmd: {} — {}", cmd, desc),
            SearchResult::MathResult(expr, val) => format!("{} = {}", expr, val),
        }
    }

    pub fn category_label(&self) -> &'static str {
        match self {
            SearchResult::App(_, _) => "APPLICATION",
            SearchResult::File(_, _) => "VFS FILE",
            SearchResult::Command(_, _) => "COMMAND",
            SearchResult::MathResult(_, _) => "MATH",
        }
    }

    pub fn category_color(&self) -> Color {
        match self {
            SearchResult::App(_, _) => Color::rgb(90, 155, 255),
            SearchResult::File(_, _) => Color::rgb(255, 215, 0),
            SearchResult::Command(_, _) => Color::rgb(215, 120, 255),
            SearchResult::MathResult(_, _) => Color::rgb(80, 250, 123),
        }
    }
}

/// The Spotlight overlay state.
pub struct Spotlight {
    pub is_visible: bool,
    pub query: String,
    pub selected_idx: usize,
    pub results: Vec<SearchResult>,
}

impl Spotlight {
    pub fn new() -> Self {
        Self {
            is_visible: false,
            query: String::new(),
            selected_idx: 0,
            results: Vec::new(),
        }
    }

    /// Toggle visibility (show/hide).
    pub fn toggle(&mut self) {
        self.is_visible = !self.is_visible;
        if self.is_visible {
            self.query.clear();
            self.results.clear();
            self.selected_idx = 0;
        }
    }

    /// Dismiss spotlight.
    pub fn hide(&mut self) {
        self.is_visible = false;
    }

    /// Push a character to the query and re-search.
    pub fn push_char(&mut self, c: char) {
        self.query.push(c);
        self.refresh_results();
    }

    /// Delete last character and re-search.
    pub fn backspace(&mut self) {
        self.query.pop();
        self.refresh_results();
    }

    /// Move selection up.
    pub fn select_prev(&mut self) {
        if self.selected_idx > 0 {
            self.selected_idx -= 1;
        }
    }

    /// Move selection down.
    pub fn select_next(&mut self) {
        if !self.results.is_empty() && self.selected_idx + 1 < self.results.len() {
            self.selected_idx += 1;
        }
    }

    /// Activate the selected result. Returns an optional AppId to launch.
    pub fn activate_selected(&mut self) -> Option<AppId> {
        if let Some(result) = self.results.get(self.selected_idx) {
            let app = match result {
                SearchResult::App(id, _) => Some(*id),
                _ => None,
            };
            self.hide();
            return app;
        }
        None
    }

    /// Refresh search results based on current query.
    fn refresh_results(&mut self) {
        self.results.clear();
        self.selected_idx = 0;

        if self.query.is_empty() {
            return;
        }

        let q = self.query.to_lowercase();

        // 1. Search applications
        let apps: &[(&str, AppId)] = &[
            ("terminal", AppId::Terminal),
            ("calculator", AppId::Calculator),
            ("paint", AppId::Paint),
            ("settings", AppId::Settings),
            ("files", AppId::FileManager),
            ("aegispad", AppId::AegisPad),
            ("snake", AppId::Snake),
            ("monitor", AppId::ActivityMonitor),
            ("crash", AppId::CrashTest),
            ("about", AppId::AboutDialog),
            ("browser", AppId::Browser),
            ("minesweeper", AppId::Minesweeper),
            ("mine", AppId::Minesweeper),
            ("synth", AppId::Synth),
            ("music", AppId::Synth),
            ("piano", AppId::Synth),
            ("chat", AppId::Chat),
            ("message", AppId::Chat),
            ("network", AppId::Chat),
            ("intranet", AppId::Chat),
            // Ring 3 applications. Prefixed rather than named after the app
            // they mirror, so a query for one cannot ambiguously match both --
            // the matcher tests substrings in both directions.
            ("r3term", AppId::UserTerminal),
            ("r3fault", AppId::UserCrashTest),
            ("r3proc", AppId::UserActivityMonitor),
        ];

        for &(name, id) in apps {
            if name.contains(&q) || q.contains(name) {
                self.results.push(SearchResult::App(id, id.name()));
            }
        }

        // 2. Search VFS files
        let all_paths = crate::fs::get_all_vfs_paths();
        for path in all_paths {
            let lower = path.to_lowercase();
            if lower.contains(&q) {
                let size = crate::fs::read_to_string(&path).map(|s| s.len()).unwrap_or(0);
                self.results.push(SearchResult::File(path, size));
            }
        }

        // 3. Search shell commands
        let commands: &[(&str, &str)] = &[
            ("neofetch", "Display OS specs banner"),
            ("ps", "List active processes"),
            ("history", "Command history tape"),
            ("wallpaper", "Change desktop theme"),
            ("beep", "Play audio tone"),
            ("play", "Play musical tune"),
            ("clear", "Clear terminal screen"),
            ("reboot", "Trigger CPU reset"),
            ("ls", "List VFS files"),
            ("cat", "Display file contents"),
        ];

        for &(cmd, desc) in commands {
            if cmd.contains(&q) {
                self.results.push(SearchResult::Command(cmd, desc));
            }
        }

        // 4. Try inline math evaluation
        if let Some(result) = self.try_eval_math(&self.query.clone()) {
            self.results.insert(0, SearchResult::MathResult(self.query.clone(), result));
        }

        // Limit results
        if self.results.len() > 8 {
            self.results.truncate(8);
        }
    }

    /// Very simple inline math evaluator: supports `a + b`, `a - b`, `a * b`, `a / b`, `sqrt(n)`.
    fn try_eval_math(&self, expr: &str) -> Option<f64> {
        let expr = expr.trim();

        // sqrt(n)
        if expr.starts_with("sqrt(") && expr.ends_with(')') {
            let inner = &expr[5..expr.len() - 1];
            if let Ok(n) = inner.parse::<f64>() {
                if n >= 0.0 {
                    return Some(crate::apps::calculator::CalculatorApp::compute_sqrt(n));
                }
            }
            return None;
        }

        // Binary operations
        for op in [" + ", " - ", " * ", " / "].iter() {
            if let Some(pos) = expr.find(op) {
                let left = expr[..pos].trim().parse::<f64>().ok()?;
                let right = expr[pos + op.len()..].trim().parse::<f64>().ok()?;
                return match *op {
                    " + " => Some(left + right),
                    " - " => Some(left - right),
                    " * " => Some(left * right),
                    " / " => {
                        if right != 0.0 {
                            Some(left / right)
                        } else {
                            None
                        }
                    }
                    _ => None,
                };
            }
        }

        None
    }

    /// Render the Spotlight modal overlay.
    pub fn render(&self, fb: &mut Framebuffer, screen_width: usize, screen_height: usize) {
        if !self.is_visible {
            return;
        }

        let modal_w = 500u32;
        let result_height = if self.results.is_empty() { 0 } else { (self.results.len() as u32 * 28) + 8 };
        let modal_h = 44 + result_height;
        let mx = ((screen_width as i32) - modal_w as i32) / 2;
        let my = (screen_height as i32) / 4;
        let modal_rect = Rect::new(mx, my, modal_w, modal_h);

        // Shadow & background
        draw_shadow(fb, modal_rect, 12, 180, None);
        draw_rounded_rect(fb, modal_rect, 12, Color::rgba(26, 28, 34, 245));
        draw_rounded_rect_outline(fb, modal_rect, 12, Color::rgb(70, 75, 88));

        // Search icon and text field
        draw_string(fb, mx + 14, my + 12, "Search:", Color::rgb(140, 145, 155), None);
        let query_x = mx + 78;
        draw_string(fb, query_x, my + 12, &self.query, Color::WHITE, None);
        let cursor_x = query_x + (self.query.len() * FONT_WIDTH) as i32;
        draw_string(fb, cursor_x, my + 12, "_", Color::rgb(80, 250, 123), None);

        // Results
        let results_y = my + 40;
        for (i, result) in self.results.iter().enumerate() {
            let ry = results_y + (i as i32 * 28);

            // Selection highlight
            if i == self.selected_idx {
                let sel_rect = Rect::new(mx + 6, ry, modal_w - 12, 26);
                draw_rounded_rect(fb, sel_rect, 6, Color::rgba(80, 250, 123, 40));
            }

            // Category badge
            let badge = result.category_label();
            let badge_color = result.category_color();
            let badge_rect = Rect::new(mx + 14, ry + 3, (badge.len() as u32 * FONT_WIDTH as u32) + 10, 18);
            draw_rounded_rect(fb, badge_rect, 4, Color::rgba(badge_color.r, badge_color.g, badge_color.b, 50));
            draw_string(fb, mx + 19, ry + 5, badge, badge_color, None);

            // Result text
            let text_x = mx + 19 + badge_rect.width as i32 + 8;
            draw_string(fb, text_x, ry + 5, &result.display_text(), Color::rgb(220, 225, 235), None);
        }
    }
}
