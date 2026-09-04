//! Aegis Hypertext Web & Document Browser for AegisOS
//!
//! A native OS web browser with URL address bar, back/forward navigation,
//! markdown/hypertext rendering engine, and built-in `aegis://` and `vfs://` protocols.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::drivers::framebuffer::Framebuffer;
use crate::drivers::ps2_keyboard::{KeyCode, KeyEvent};
use crate::gui::font::{draw_string, FONT_HEIGHT, FONT_WIDTH};
use crate::gui::primitives::{draw_line, draw_rect, draw_rounded_rect, draw_rounded_rect_outline, Color, Rect};
use crate::gui::window::Window;

/// A rendered line of browser content with optional color and style.
#[derive(Clone)]
pub struct BrowserLine {
    pub text: String,
    pub color: Color,
    pub bold: bool,
    pub is_header: bool,
    pub link_url: Option<String>,
}

/// The Aegis Browser application state.
pub struct BrowserApp {
    pub url_input: String,
    pub current_url: String,
    pub history: Vec<String>,
    pub history_idx: usize,
    pub rendered_lines: Vec<BrowserLine>,
    pub scroll_offset: usize,
    pub editing_url: bool,
}

impl BrowserApp {
    pub fn new() -> Self {
        let mut app = Self {
            url_input: String::from("aegis://home"),
            current_url: String::new(),
            history: Vec::new(),
            history_idx: 0,
            rendered_lines: Vec::new(),
            scroll_offset: 0,
            editing_url: false,
        };
        app.navigate("aegis://home");
        app
    }

    /// Navigate to a URL and render the page.
    pub fn navigate(&mut self, url: &str) {
        let url_str = url.to_string();
        self.current_url = url_str.clone();
        self.url_input = url_str.clone();
        self.scroll_offset = 0;

        // Add to history
        if self.history.is_empty() || self.history.last() != Some(&url_str) {
            // Truncate forward history if navigating from mid-history
            if !self.history.is_empty() && self.history_idx < self.history.len().saturating_sub(1) {
                self.history.truncate(self.history_idx + 1);
            }
            self.history.push(url_str);
            self.history_idx = self.history.len().saturating_sub(1);
        }

        self.rendered_lines = self.render_page(&self.current_url.clone());
    }

    /// Go back in history.
    pub fn go_back(&mut self) {
        if self.history_idx > 0 {
            self.history_idx -= 1;
            let url = self.history[self.history_idx].clone();
            self.current_url = url.clone();
            self.url_input = url.clone();
            self.scroll_offset = 0;
            self.rendered_lines = self.render_page(&url);
        }
    }

    /// Go forward in history.
    pub fn go_forward(&mut self) {
        if self.history_idx + 1 < self.history.len() {
            self.history_idx += 1;
            let url = self.history[self.history_idx].clone();
            self.current_url = url.clone();
            self.url_input = url.clone();
            self.scroll_offset = 0;
            self.rendered_lines = self.render_page(&url);
        }
    }

    /// Render a page from its URL, returning styled lines.
    fn render_page(&self, url: &str) -> Vec<BrowserLine> {
        let mut lines: Vec<BrowserLine> = Vec::new();

        if url == "aegis://home" {
            Self::page_home(&mut lines);
        } else if url == "aegis://agent" {
            Self::page_agent(&mut lines);
        } else if url == "aegis://docs/kernel" {
            Self::page_docs_kernel(&mut lines);
        } else if url.starts_with("vfs://") {
            let path = &url["vfs://".len()..];
            Self::page_vfs(&mut lines, path);
        } else {
            lines.push(BrowserLine {
                text: format!("404 — Page Not Found: {}", url),
                color: Color::rgb(255, 85, 85),
                bold: true,
                is_header: true,
                link_url: None,
            });
            lines.push(BrowserLine {
                text: String::from("The requested URL could not be resolved."),
                color: Color::rgb(180, 180, 190),
                bold: false,
                is_header: false,
                link_url: None,
            });
        }

        lines
    }

    fn page_home(lines: &mut Vec<BrowserLine>) {
        lines.push(BrowserLine { text: "AegisOS Web Portal".into(), color: Color::rgb(100, 230, 245), bold: true, is_header: true, link_url: None });
        lines.push(BrowserLine { text: String::new(), color: Color::WHITE, bold: false, is_header: false, link_url: None });
        lines.push(BrowserLine { text: "Welcome to AegisOS — a crash-resilient, no_std operating system".into(), color: Color::rgb(220, 225, 230), bold: false, is_header: false, link_url: None });
        lines.push(BrowserLine { text: "written entirely in Rust for the x86_64 architecture.".into(), color: Color::rgb(220, 225, 230), bold: false, is_header: false, link_url: None });
        lines.push(BrowserLine { text: String::new(), color: Color::WHITE, bold: false, is_header: false, link_url: None });
        lines.push(BrowserLine { text: "Quick Links".into(), color: Color::rgb(255, 215, 0), bold: true, is_header: true, link_url: None });
        lines.push(BrowserLine { text: String::new(), color: Color::WHITE, bold: false, is_header: false, link_url: None });
        lines.push(BrowserLine { text: "  > AI Agent Kernel Dashboard".into(), color: Color::rgb(80, 250, 123), bold: false, is_header: false, link_url: Some("aegis://agent".into()) });
        lines.push(BrowserLine { text: "  > Kernel Architecture Docs".into(), color: Color::rgb(80, 250, 123), bold: false, is_header: false, link_url: Some("aegis://docs/kernel".into()) });
        lines.push(BrowserLine { text: "  > VFS: /welcome.txt".into(), color: Color::rgb(80, 250, 123), bold: false, is_header: false, link_url: Some("vfs:///welcome.txt".into()) });
        lines.push(BrowserLine { text: String::new(), color: Color::WHITE, bold: false, is_header: false, link_url: None });
        lines.push(BrowserLine { text: "Features".into(), color: Color::rgb(255, 215, 0), bold: true, is_header: true, link_url: None });
        lines.push(BrowserLine { text: "  * Ring 0 / Ring 3 hardware memory isolation".into(), color: Color::rgb(200, 205, 210), bold: false, is_header: false, link_url: None });
        lines.push(BrowserLine { text: "  * Crash-resilient fault recovery without desktop downtime".into(), color: Color::rgb(200, 205, 210), bold: false, is_header: false, link_url: None });
        lines.push(BrowserLine { text: "  * macOS-style composited desktop with 60 FPS rendering".into(), color: Color::rgb(200, 205, 210), bold: false, is_header: false, link_url: None });
        lines.push(BrowserLine { text: "  * Autonomous AI Agent kernel supervisor via serial RPC".into(), color: Color::rgb(200, 205, 210), bold: false, is_header: false, link_url: None });
        lines.push(BrowserLine { text: "  * Tab auto-completion and ANSI color terminal".into(), color: Color::rgb(200, 205, 210), bold: false, is_header: false, link_url: None });
        lines.push(BrowserLine { text: "  * In-memory RAM disk VFS with file manager".into(), color: Color::rgb(200, 205, 210), bold: false, is_header: false, link_url: None });
        lines.push(BrowserLine { text: "  * Scientific calculator, paint, and snake arcade".into(), color: Color::rgb(200, 205, 210), bold: false, is_header: false, link_url: None });
        lines.push(BrowserLine { text: String::new(), color: Color::WHITE, bold: false, is_header: false, link_url: None });
        lines.push(BrowserLine { text: "AegisOS v0.1.0 — Built with Rust (no_std) + x86_64 Long Mode".into(), color: Color::rgb(110, 115, 125), bold: false, is_header: false, link_url: None });
    }

    fn page_agent(lines: &mut Vec<BrowserLine>) {
        let (packets, vfs_ops, tasks_managed, last_cmd) = crate::agent::get_agent_metrics();
        let (used_bytes, total_bytes) = crate::task::get_memory_stats();
        let cpu = crate::task::get_cpu_usage();
        let procs = crate::task::get_process_list();

        lines.push(BrowserLine { text: "AI Agent Kernel Supervisor Dashboard".into(), color: Color::rgb(100, 230, 245), bold: true, is_header: true, link_url: None });
        lines.push(BrowserLine { text: String::new(), color: Color::WHITE, bold: false, is_header: false, link_url: None });
        lines.push(BrowserLine { text: "Agent Bridge Status".into(), color: Color::rgb(255, 215, 0), bold: true, is_header: true, link_url: None });
        lines.push(BrowserLine { text: format!("  Mode:              Ring 0 Kernel Supervisor (Autonomous)"), color: Color::rgb(80, 250, 123), bold: false, is_header: false, link_url: None });
        lines.push(BrowserLine { text: format!("  Packets Handled:   {}", packets), color: Color::rgb(220, 225, 230), bold: false, is_header: false, link_url: None });
        lines.push(BrowserLine { text: format!("  VFS Operations:    {}", vfs_ops), color: Color::rgb(220, 225, 230), bold: false, is_header: false, link_url: None });
        lines.push(BrowserLine { text: format!("  Tasks Managed:     {}", tasks_managed), color: Color::rgb(220, 225, 230), bold: false, is_header: false, link_url: None });
        lines.push(BrowserLine { text: format!("  Last Command:      {}", last_cmd), color: Color::rgb(220, 225, 230), bold: false, is_header: false, link_url: None });
        lines.push(BrowserLine { text: String::new(), color: Color::WHITE, bold: false, is_header: false, link_url: None });
        lines.push(BrowserLine { text: "System Telemetry".into(), color: Color::rgb(255, 215, 0), bold: true, is_header: true, link_url: None });
        lines.push(BrowserLine { text: format!("  CPU Usage:         {}%", cpu), color: Color::rgb(100, 230, 245), bold: false, is_header: false, link_url: None });
        lines.push(BrowserLine { text: format!("  Memory:            {} MB / {} MB", used_bytes / (1024*1024), total_bytes / (1024*1024)), color: Color::rgb(100, 230, 245), bold: false, is_header: false, link_url: None });
        lines.push(BrowserLine { text: format!("  Active Tasks:      {}", procs.len()), color: Color::rgb(100, 230, 245), bold: false, is_header: false, link_url: None });
        lines.push(BrowserLine { text: String::new(), color: Color::WHITE, bold: false, is_header: false, link_url: None });
        lines.push(BrowserLine { text: "Supported RPC Commands".into(), color: Color::rgb(255, 215, 0), bold: true, is_header: true, link_url: None });
        lines.push(BrowserLine { text: "  AGENT:STATUS      — Bridge health check".into(), color: Color::rgb(200, 205, 210), bold: false, is_header: false, link_url: None });
        lines.push(BrowserLine { text: "  AGENT:SYSINFO     — Query CPU, memory, task telemetry".into(), color: Color::rgb(200, 205, 210), bold: false, is_header: false, link_url: None });
        lines.push(BrowserLine { text: "  AGENT:VFS_READ    — Read file from VFS".into(), color: Color::rgb(200, 205, 210), bold: false, is_header: false, link_url: None });
        lines.push(BrowserLine { text: "  AGENT:VFS_WRITE   — Write file to VFS".into(), color: Color::rgb(200, 205, 210), bold: false, is_header: false, link_url: None });
        lines.push(BrowserLine { text: "  AGENT:VFS_LIST    — List directory entries".into(), color: Color::rgb(200, 205, 210), bold: false, is_header: false, link_url: None });
        lines.push(BrowserLine { text: "  AGENT:TASK_KILL   — Terminate process by PID".into(), color: Color::rgb(200, 205, 210), bold: false, is_header: false, link_url: None });
        lines.push(BrowserLine { text: "  AGENT:EXEC        — Execute shell command".into(), color: Color::rgb(200, 205, 210), bold: false, is_header: false, link_url: None });
        lines.push(BrowserLine { text: String::new(), color: Color::WHITE, bold: false, is_header: false, link_url: None });
        lines.push(BrowserLine { text: "  > Back to Home".into(), color: Color::rgb(80, 250, 123), bold: false, is_header: false, link_url: Some("aegis://home".into()) });
    }

    fn page_docs_kernel(lines: &mut Vec<BrowserLine>) {
        lines.push(BrowserLine { text: "AegisOS Kernel Architecture".into(), color: Color::rgb(100, 230, 245), bold: true, is_header: true, link_url: None });
        lines.push(BrowserLine { text: String::new(), color: Color::WHITE, bold: false, is_header: false, link_url: None });
        lines.push(BrowserLine { text: "Memory Model".into(), color: Color::rgb(255, 215, 0), bold: true, is_header: true, link_url: None });
        lines.push(BrowserLine { text: "  AegisOS uses x86_64 4-level paging (PML4) with separate address".into(), color: Color::rgb(200, 205, 210), bold: false, is_header: false, link_url: None });
        lines.push(BrowserLine { text: "  spaces for kernel (Ring 0) and user (Ring 3) code. The Higher Half".into(), color: Color::rgb(200, 205, 210), bold: false, is_header: false, link_url: None });
        lines.push(BrowserLine { text: "  Direct Map (HHDM) provides identity-mapped physical memory access.".into(), color: Color::rgb(200, 205, 210), bold: false, is_header: false, link_url: None });
        lines.push(BrowserLine { text: String::new(), color: Color::WHITE, bold: false, is_header: false, link_url: None });
        lines.push(BrowserLine { text: "Fault Isolation".into(), color: Color::rgb(255, 215, 0), bold: true, is_header: true, link_url: None });
        lines.push(BrowserLine { text: "  Ring 3 user tasks that trigger #PF, #DE, #UD, or #GP faults are".into(), color: Color::rgb(200, 205, 210), bold: false, is_header: false, link_url: None });
        lines.push(BrowserLine { text: "  terminated by the kernel's ISR handlers without affecting desktop".into(), color: Color::rgb(200, 205, 210), bold: false, is_header: false, link_url: None });
        lines.push(BrowserLine { text: "  stability. The compositor continues rendering at 60 FPS.".into(), color: Color::rgb(200, 205, 210), bold: false, is_header: false, link_url: None });
        lines.push(BrowserLine { text: String::new(), color: Color::WHITE, bold: false, is_header: false, link_url: None });
        lines.push(BrowserLine { text: "Task Scheduler".into(), color: Color::rgb(255, 215, 0), bold: true, is_header: true, link_url: None });
        lines.push(BrowserLine { text: "  100Hz preemptive round-robin scheduler via PIT Channel 0 IRQ0.".into(), color: Color::rgb(200, 205, 210), bold: false, is_header: false, link_url: None });
        lines.push(BrowserLine { text: "  TSS-based RSP0 stack switching for privilege level transitions.".into(), color: Color::rgb(200, 205, 210), bold: false, is_header: false, link_url: None });
        lines.push(BrowserLine { text: String::new(), color: Color::WHITE, bold: false, is_header: false, link_url: None });
        lines.push(BrowserLine { text: "  > AI Agent Dashboard".into(), color: Color::rgb(80, 250, 123), bold: false, is_header: false, link_url: Some("aegis://agent".into()) });
        lines.push(BrowserLine { text: "  > Back to Home".into(), color: Color::rgb(80, 250, 123), bold: false, is_header: false, link_url: Some("aegis://home".into()) });
    }

    fn page_vfs(lines: &mut Vec<BrowserLine>, path: &str) {
        lines.push(BrowserLine { text: format!("VFS Document: {}", path), color: Color::rgb(100, 230, 245), bold: true, is_header: true, link_url: None });
        lines.push(BrowserLine { text: String::new(), color: Color::WHITE, bold: false, is_header: false, link_url: None });

        match crate::fs::read_to_string(path) {
            Ok(content) => {
                for line in content.lines() {
                    lines.push(BrowserLine { text: line.to_string(), color: Color::rgb(220, 225, 230), bold: false, is_header: false, link_url: None });
                }
            }
            Err(e) => {
                lines.push(BrowserLine { text: format!("Error reading {}: {}", path, e), color: Color::rgb(255, 85, 85), bold: true, is_header: false, link_url: None });
            }
        }
        lines.push(BrowserLine { text: String::new(), color: Color::WHITE, bold: false, is_header: false, link_url: None });
        lines.push(BrowserLine { text: "  > Back to Home".into(), color: Color::rgb(80, 250, 123), bold: false, is_header: false, link_url: Some("aegis://home".into()) });
    }

    /// Handles keyboard input (URL editing, Enter to navigate).
    pub fn handle_key(&mut self, event: KeyEvent) {
        if !event.pressed {
            return;
        }

        match event.code {
            KeyCode::Backspace => {
                if self.editing_url {
                    self.url_input.pop();
                }
            }
            KeyCode::Enter => {
                if self.editing_url {
                    self.editing_url = false;
                    let url = self.url_input.clone();
                    self.navigate(&url);
                }
            }
            KeyCode::Up => {
                if self.scroll_offset > 0 {
                    self.scroll_offset -= 1;
                }
            }
            KeyCode::Down => {
                if self.scroll_offset + 1 < self.rendered_lines.len() {
                    self.scroll_offset += 1;
                }
            }
            KeyCode::Tab => {
                self.editing_url = !self.editing_url;
            }
            _ => {
                if self.editing_url {
                    if let Some(ch) = event.char_byte {
                        if (32..=126).contains(&ch) {
                            self.url_input.push(ch as char);
                        }
                    }
                }
            }
        }
    }

    /// Handle mouse click within the browser window.
    pub fn handle_click(&mut self, win: &Window, mx: i32, my: i32) -> BrowserAction {
        let client = win.client_rect();

        // URL bar area
        let url_bar_y = client.y + 4;
        let url_bar_h = 22;

        // Back button
        let back_rect = Rect::new(client.x + 6, url_bar_y, 28, url_bar_h as u32);
        if back_rect.contains(mx, my) {
            self.go_back();
            return BrowserAction::None;
        }

        // Forward button
        let fwd_rect = Rect::new(client.x + 38, url_bar_y, 28, url_bar_h as u32);
        if fwd_rect.contains(mx, my) {
            self.go_forward();
            return BrowserAction::None;
        }

        // Refresh button
        let ref_rect = Rect::new(client.x + 70, url_bar_y, 28, url_bar_h as u32);
        if ref_rect.contains(mx, my) {
            let url = self.current_url.clone();
            self.navigate(&url);
            return BrowserAction::None;
        }

        // URL input area click to focus
        let url_input_x = client.x + 104;
        let url_input_w = (client.width as i32 - 114).max(60) as u32;
        let url_rect = Rect::new(url_input_x, url_bar_y, url_input_w, url_bar_h as u32);
        if url_rect.contains(mx, my) {
            self.editing_url = true;
            return BrowserAction::None;
        }

        // Content area: check for link clicks
        let content_y_start = client.y + 32;
        let line_height = (FONT_HEIGHT + 4) as i32;
        let rel_y = my - content_y_start;
        if rel_y >= 0 {
            let line_idx = self.scroll_offset + (rel_y / line_height) as usize;
            if line_idx < self.rendered_lines.len() {
                if let Some(ref url) = self.rendered_lines[line_idx].link_url {
                    let nav_url = url.clone();
                    self.navigate(&nav_url);
                    return BrowserAction::None;
                }
            }
        }

        BrowserAction::None
    }

    /// Renders the browser inside the window client area.
    pub fn render(&self, win: &Window, fb: &mut Framebuffer) {
        let client = win.client_rect();
        if client.width < 120 || client.height < 80 {
            return;
        }

        // Dark browser canvas
        draw_rect(fb, client, Color::rgb(22, 24, 28));

        // ── Navigation Bar ──
        let nav_y = client.y + 4;
        let nav_h = 22u32;

        // Back button
        let back_rect = Rect::new(client.x + 6, nav_y, 28, nav_h);
        draw_rounded_rect(fb, back_rect, 4, Color::rgb(50, 54, 62));
        draw_string(fb, client.x + 13, nav_y + 3, "<", Color::rgb(180, 185, 195), None);

        // Forward button
        let fwd_rect = Rect::new(client.x + 38, nav_y, 28, nav_h);
        draw_rounded_rect(fb, fwd_rect, 4, Color::rgb(50, 54, 62));
        draw_string(fb, client.x + 45, nav_y + 3, ">", Color::rgb(180, 185, 195), None);

        // Refresh button
        let ref_rect = Rect::new(client.x + 70, nav_y, 28, nav_h);
        draw_rounded_rect(fb, ref_rect, 4, Color::rgb(50, 54, 62));
        draw_string(fb, client.x + 77, nav_y + 3, "R", Color::rgb(180, 185, 195), None);

        // URL bar
        let url_x = client.x + 104;
        let url_w = (client.width as i32 - 114).max(60) as u32;
        let url_rect = Rect::new(url_x, nav_y, url_w, nav_h);
        let url_bg = if self.editing_url { Color::rgb(40, 42, 50) } else { Color::rgb(32, 34, 40) };
        draw_rounded_rect(fb, url_rect, 6, url_bg);
        draw_rounded_rect_outline(fb, url_rect, 6, if self.editing_url { Color::rgb(80, 250, 123) } else { Color::rgb(60, 65, 75) });

        // URL text (truncate to fit)
        let max_url_chars = ((url_w as usize).saturating_sub(16)) / FONT_WIDTH;
        let display_url = if self.url_input.len() > max_url_chars {
            &self.url_input[self.url_input.len() - max_url_chars..]
        } else {
            &self.url_input
        };
        draw_string(fb, url_x + 8, nav_y + 3, display_url, Color::rgb(200, 205, 215), None);
        if self.editing_url {
            let cursor_x = url_x + 8 + (display_url.len() * FONT_WIDTH) as i32;
            draw_string(fb, cursor_x, nav_y + 3, "_", Color::rgb(80, 250, 123), None);
        }

        // ── Thin separator line ──
        let sep_y = nav_y + nav_h as i32 + 3;
        draw_line(fb, client.x + 4, sep_y, client.x + client.width as i32 - 4, sep_y, Color::rgb(45, 48, 55));

        // ── Content Area ──
        let content_y_start = sep_y + 4;
        let line_height = (FONT_HEIGHT + 4) as i32;
        let max_visible = ((client.y + client.height as i32 - content_y_start) / line_height).max(0) as usize;

        let start = self.scroll_offset;
        let end = (start + max_visible).min(self.rendered_lines.len());

        for (i, line) in self.rendered_lines[start..end].iter().enumerate() {
            let ly = content_y_start + (i as i32 * line_height);
            let cx = client.x + 10;

            if line.is_header {
                // Headers rendered slightly larger feel via bright color
                draw_string(fb, cx, ly, &line.text, line.color, None);
                if !line.text.is_empty() {
                    // Underline for headers
                    let uw = (line.text.len() * FONT_WIDTH) as i32;
                    draw_line(fb, cx, ly + FONT_HEIGHT as i32 + 1, cx + uw, ly + FONT_HEIGHT as i32 + 1, Color::rgb(60, 65, 75));
                }
            } else if line.link_url.is_some() {
                // Clickable link with underline
                draw_string(fb, cx, ly, &line.text, line.color, None);
                let uw = (line.text.len() * FONT_WIDTH) as i32;
                draw_line(fb, cx, ly + FONT_HEIGHT as i32, cx + uw, ly + FONT_HEIGHT as i32, Color::rgb(60, 200, 100));
            } else {
                draw_string(fb, cx, ly, &line.text, line.color, None);
            }
        }

        // Scroll indicators
        if self.scroll_offset > 0 {
            draw_string(fb, client.x + client.width as i32 - 20, content_y_start, "^", Color::rgb(120, 125, 135), None);
        }
        if end < self.rendered_lines.len() {
            draw_string(fb, client.x + client.width as i32 - 20, client.y + client.height as i32 - 18, "v", Color::rgb(120, 125, 135), None);
        }
    }
}

/// Browser-specific action results.
pub enum BrowserAction {
    None,
}
