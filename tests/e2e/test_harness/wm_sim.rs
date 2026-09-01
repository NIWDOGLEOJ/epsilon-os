//! AegisOS E2E Test Harness: macOS Window Manager & Desktop Simulator
//!
//! Models 24px top menu bar, draggable floating windows, traffic-light controls,
//! Z-order focus cycling, and launcher dock.

use super::types::*;
use super::gui_sim::*;

pub const TOP_BAR_HEIGHT: usize = 24;
pub const DOCK_WIDTH: usize = 320;
pub const DOCK_HEIGHT: usize = 48;

#[derive(Debug, Clone)]
pub struct WindowSimulator {
    pub id: u32,
    pub app_id: AppId,
    pub title: String,
    pub x: i32,
    pub y: i32,
    pub width: usize,
    pub height: usize,
    pub is_focused: bool,
    pub is_dragging: bool,
    pub is_minimized: bool,
    pub is_closed: bool,
    pub is_maximized: bool,
    pub saved_rect: Rect,
    pub drag_offset_x: i32,
    pub drag_offset_y: i32,
    pub pid: Option<ProcessId>,
}

impl WindowSimulator {
    pub fn new(id: u32, app_id: AppId, title: &str, x: i32, y: i32, width: usize, height: usize, pid: Option<ProcessId>) -> Self {
        Self {
            id,
            app_id,
            title: title.to_string(),
            x,
            y,
            width,
            height,
            is_focused: false,
            is_dragging: false,
            is_minimized: false,
            is_closed: false,
            is_maximized: false,
            saved_rect: Rect::new(x, y, width, height),
            drag_offset_x: 0,
            drag_offset_y: 0,
            pid,
        }
    }

    pub fn titlebar_rect(&self) -> Rect {
        Rect::new(self.x, self.y, self.width, 24)
    }

    pub fn close_btn_contains(&self, px: i32, py: i32) -> bool {
        let cx = self.x + 16;
        let cy = self.y + 12;
        (px - cx) * (px - cx) + (py - cy) * (py - cy) <= 36
    }

    pub fn minimize_btn_contains(&self, px: i32, py: i32) -> bool {
        let cx = self.x + 32;
        let cy = self.y + 12;
        (px - cx) * (px - cx) + (py - cy) * (py - cy) <= 36
    }

    pub fn maximize_btn_contains(&self, px: i32, py: i32) -> bool {
        let cx = self.x + 48;
        let cy = self.y + 12;
        (px - cx) * (px - cx) + (py - cy) * (py - cy) <= 36
    }

    pub fn client_rect(&self) -> Rect {
        Rect::new(
            self.x,
            self.y + 24,
            self.width,
            self.height.saturating_sub(24),
        )
    }
}

pub struct WindowManagerSimulator {
    pub windows: Vec<WindowSimulator>,
    next_window_id: u32,
    screen_width: usize,
    screen_height: usize,
    pub active_app_title: String,
    pub uptime_seconds: u64,
}

impl WindowManagerSimulator {
    pub fn new(screen_width: usize, screen_height: usize) -> Self {
        Self {
            windows: Vec::new(),
            next_window_id: 1,
            screen_width,
            screen_height,
            active_app_title: "Finder".to_string(),
            uptime_seconds: 0,
        }
    }

    pub fn create_window(
        &mut self,
        app_id: AppId,
        title: &str,
        x: i32,
        y: i32,
        w: usize,
        h: usize,
        pid: Option<ProcessId>,
    ) -> u32 {
        let wid = self.next_window_id;
        self.next_window_id += 1;

        // Defocus all other windows
        for win in self.windows.iter_mut() {
            win.is_focused = false;
        }

        let mut win = WindowSimulator::new(wid, app_id, title, x, y, w, h, pid);
        win.is_focused = true;
        self.active_app_title = title.to_string();
        self.windows.push(win);
        wid
    }

    pub fn close_window(&mut self, window_id: u32) -> Option<ProcessId> {
        let mut target_pid = None;
        if let Some(pos) = self.windows.iter().position(|w| w.id == window_id) {
            target_pid = self.windows[pos].pid;
            self.windows.remove(pos);
            // Focus new top window
            if let Some(top) = self.windows.last_mut() {
                top.is_focused = true;
                self.active_app_title = top.title.clone();
            } else {
                self.active_app_title = "Finder".to_string();
            }
        }
        target_pid
    }

    pub fn close_window_by_pid(&mut self, pid: ProcessId) -> bool {
        if let Some(pos) = self.windows.iter().position(|w| w.pid == Some(pid)) {
            self.windows.remove(pos);
            if let Some(top) = self.windows.last_mut() {
                top.is_focused = true;
                self.active_app_title = top.title.clone();
            } else {
                self.active_app_title = "Finder".to_string();
            }
            true
        } else {
            false
        }
    }

    pub fn handle_mouse_down(&mut self, px: i32, py: i32) -> Option<u32> {
        // Check Top Menu Bar (y: 0..24)
        if py < TOP_BAR_HEIGHT as i32 {
            return None;
        }

        // Check Launcher Dock at bottom
        let dock_x = (self.screen_width.saturating_sub(DOCK_WIDTH)) / 2;
        let dock_y = self.screen_height.saturating_sub(DOCK_HEIGHT + 8);
        let dock_rect = Rect::new(dock_x as i32, dock_y as i32, DOCK_WIDTH, DOCK_HEIGHT);
        if dock_rect.contains(px, py) {
            // Clicked inside dock!
            return None;
        }

        // Check Windows in reverse Z-order (top to bottom)
        let mut clicked_idx = None;
        for (i, win) in self.windows.iter().enumerate().rev() {
            if win.is_minimized || win.is_closed {
                continue;
            }
            let win_rect = Rect::new(win.x, win.y, win.width, win.height);
            if win_rect.contains(px, py) {
                clicked_idx = Some(i);
                break;
            }
        }

        if let Some(idx) = clicked_idx {
            // Check traffic light buttons
            let win = &mut self.windows[idx];
            if win.close_btn_contains(px, py) {
                let wid = win.id;
                self.close_window(wid);
                return Some(wid);
            }
            if win.minimize_btn_contains(px, py) {
                win.is_minimized = true;
                win.is_focused = false;
                return Some(win.id);
            }
            if win.maximize_btn_contains(px, py) {
                if win.is_maximized {
                    win.x = win.saved_rect.x;
                    win.y = win.saved_rect.y;
                    win.width = win.saved_rect.width;
                    win.height = win.saved_rect.height;
                    win.is_maximized = false;
                } else {
                    win.saved_rect = Rect::new(win.x, win.y, win.width, win.height);
                    win.x = 0;
                    win.y = TOP_BAR_HEIGHT as i32;
                    win.width = self.screen_width;
                    win.height = self.screen_height - TOP_BAR_HEIGHT - DOCK_HEIGHT - 16;
                    win.is_maximized = true;
                }
                return Some(win.id);
            }

            // Check titlebar drag
            if win.titlebar_rect().contains(px, py) {
                win.is_dragging = true;
                win.drag_offset_x = px - win.x;
                win.drag_offset_y = py - win.y;
            }

            // Bring window to top of Z-order
            let win_obj = self.windows.remove(idx);
            for w in self.windows.iter_mut() {
                w.is_focused = false;
            }
            let wid = win_obj.id;
            let title = win_obj.title.clone();
            let mut focused_win = win_obj;
            focused_win.is_focused = true;
            self.windows.push(focused_win);
            self.active_app_title = title;

            Some(wid)
        } else {
            None
        }
    }

    pub fn handle_mouse_move(&mut self, px: i32, py: i32) {
        if let Some(win) = self.windows.iter_mut().find(|w| w.is_dragging) {
            let mut new_x = px - win.drag_offset_x;
            let mut new_y = py - win.drag_offset_y;

            // Clamping rules
            new_x = new_x.clamp(-(win.width as i32 - 40), self.screen_width as i32 - 40);
            new_y = new_y.clamp(TOP_BAR_HEIGHT as i32, self.screen_height as i32 - 30);

            win.x = new_x;
            win.y = new_y;
        }
    }

    pub fn handle_mouse_up(&mut self) {
        for win in self.windows.iter_mut() {
            win.is_dragging = false;
        }
    }

    pub fn focus_window(&mut self, window_id: u32) -> bool {
        if let Some(pos) = self.windows.iter().position(|w| w.id == window_id) {
            let win_obj = self.windows.remove(pos);
            for w in self.windows.iter_mut() {
                w.is_focused = false;
            }
            let title = win_obj.title.clone();
            let mut focused_win = win_obj;
            focused_win.is_focused = true;
            self.windows.push(focused_win);
            self.active_app_title = title;
            true
        } else {
            false
        }
    }

    pub fn focused_window(&self) -> Option<&WindowSimulator> {
        self.windows.iter().find(|w| w.is_focused && !w.is_minimized && !w.is_closed)
    }

    pub fn focused_window_mut(&mut self) -> Option<&mut WindowSimulator> {
        self.windows.iter_mut().find(|w| w.is_focused && !w.is_minimized && !w.is_closed)
    }

    pub fn render(
        &self,
        fb: &mut FramebufferSimulator,
        cpu_usage_pct: u32,
        ram_used_bytes: u64,
        mouse_x: i32,
        mouse_y: i32,
    ) {
        // 1. Wallpaper background
        fb.draw_gradient_v(
            Rect::new(0, 0, self.screen_width, self.screen_height),
            Color::rgb(30, 34, 42),
            Color::rgb(20, 22, 28),
        );

        // 2. Windows in Z-order
        for win in &self.windows {
            if win.is_minimized || win.is_closed {
                continue;
            }

            // Window Drop Shadow
            fb.draw_shadow(Rect::new(win.x, win.y, win.width, win.height), 4, 120);

            // Window Background
            fb.draw_rounded_rect(
                Rect::new(win.x, win.y, win.width, win.height),
                6,
                Color::rgb(33, 37, 43),
            );

            // Titlebar
            let titlebar_color = if win.is_focused {
                Color::rgb(44, 49, 60)
            } else {
                Color::rgb(30, 34, 39)
            };
            fb.draw_rect(win.titlebar_rect(), titlebar_color);

            // Traffic Light Buttons
            fb.draw_circle(win.x + 16, win.y + 12, 5, Color::rgb(255, 95, 86)); // Close Red
            fb.draw_circle(win.x + 32, win.y + 12, 5, Color::rgb(255, 189, 46)); // Min Yellow
            fb.draw_circle(win.x + 48, win.y + 12, 5, Color::rgb(39, 201, 63)); // Max Green

            // Title String
            fb.draw_string(
                win.x + 64,
                win.y + 4,
                &win.title,
                Color::rgb(229, 229, 229),
                None,
            );
        }

        // 3. Top Menu Bar (24px)
        fb.draw_rect(
            Rect::new(0, 0, self.screen_width, TOP_BAR_HEIGHT),
            Color::rgba(24, 24, 26, 235),
        );
        fb.draw_string(8, 4, "[#] AegisOS", Color::rgb(255, 255, 255), None);
        fb.draw_string(120, 4, &self.active_app_title, Color::rgb(0, 122, 255), None);

        // Telemetry badges
        let ram_mb = (ram_used_bytes as f64) / (1024.0 * 1024.0);
        let cpu_str = format!("[CPU: {:2}%]", cpu_usage_pct);
        let ram_str = format!("[RAM: {:.1}MB]", ram_mb);
        let clock_str = format!("[ {:02}:{:02}:{:02} ]", (self.uptime_seconds / 3600) % 24, (self.uptime_seconds / 60) % 60, self.uptime_seconds % 60);

        fb.draw_string(self.screen_width as i32 - 320, 4, &cpu_str, Color::rgb(80, 250, 123), None);
        fb.draw_string(self.screen_width as i32 - 200, 4, &ram_str, Color::rgb(241, 250, 140), None);
        fb.draw_string(self.screen_width as i32 - 90, 4, &clock_str, Color::rgb(229, 229, 229), None);

        // 4. Launcher Dock at Bottom
        let dock_x = (self.screen_width.saturating_sub(DOCK_WIDTH)) / 2;
        let dock_y = self.screen_height.saturating_sub(DOCK_HEIGHT + 8);
        fb.draw_rounded_rect(
            Rect::new(dock_x as i32, dock_y as i32, DOCK_WIDTH, DOCK_HEIGHT),
            12,
            Color::rgba(26, 29, 36, 225),
        );

        // 5. Mouse Cursor
        fb.draw_circle(mouse_x, mouse_y, 4, Color::rgb(255, 255, 255));
    }
}

trait ShadowBlender {
    fn draw_shadow(&mut self, rect: Rect, radius: usize, opacity: u8);
}

impl ShadowBlender for FramebufferSimulator {
    fn draw_shadow(&mut self, rect: Rect, radius: usize, opacity: u8) {
        let r = radius as i32;
        let shadow_rect = Rect::new(
            rect.x - r,
            rect.y - r,
            rect.width + 2 * radius,
            rect.height + 2 * radius,
        );
        self.draw_rect(shadow_rect, Color::rgba(0, 0, 0, opacity));
    }
}
