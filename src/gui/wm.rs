//! Window Manager, Z-Order Stack, Focus Dispatcher, Toast Notifications & Desktop Compositor
//!
//! Manages floating application windows, titlebar dragging clamped to screen bounds,
//! traffic-light close/minimize actions, dock launcher integration, system notifications,
//! wallpaper themes, and composites at 60 FPS.

use alloc::string::String;
use alloc::vec::Vec;

use crate::drivers::framebuffer::Framebuffer;
use crate::drivers::ps2_mouse::draw_cursor;
use crate::gui::dock::{hit_test_dock, render_dock, AppId};
use crate::gui::font::draw_string;
use crate::gui::menubar::{handle_menubar_click, render_menubar, MenubarAction, WallpaperTheme, MENUBAR_HEIGHT};
use crate::gui::primitives::{draw_rect_outline, draw_rounded_rect, draw_shadow, Color, Rect};
use crate::gui::window::Window;

// ============================================================================
// Window Manager Action Notifications
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WmAction {
    None,
    WindowFocused(u32),
    WindowClosed(u32, Option<u64> /* pid */),
    AppLaunched(AppId),
    RebootRequested,
}

// ============================================================================
// Desktop Notification Toast
// ============================================================================

#[derive(Debug, Clone)]
pub struct NotificationToast {
    pub title: String,
    pub message: String,
    pub color: Color,
    pub ticks_remaining: u32,
}

// ============================================================================
// Window Manager State Machine
// ============================================================================

pub struct WindowManager {
    pub windows: Vec<Window>,
    pub next_window_id: u32,
    pub screen_width: usize,
    pub screen_height: usize,
    pub mouse_x: i32,
    pub mouse_y: i32,
    pub mouse_down: bool,
    pub menu_open: bool,
    pub wallpaper_theme: WallpaperTheme,
    pub desktop_background: crate::gui::wallpaper::DesktopBackground,
    pub notifications: Vec<NotificationToast>,
    pub snap_preview: Option<crate::gui::window::SnapTarget>,
    pub spotlight: crate::gui::spotlight::Spotlight,
}

impl WindowManager {
    /// Creates a new window manager instance.
    pub fn new(screen_width: usize, screen_height: usize) -> Self {
        Self {
            windows: Vec::new(),
            next_window_id: 1,
            screen_width,
            screen_height,
            mouse_x: screen_width as i32 / 2,
            mouse_y: screen_height as i32 / 2,
            mouse_down: false,
            menu_open: false,
            wallpaper_theme: WallpaperTheme::DeepOcean,
            desktop_background: crate::gui::wallpaper::DesktopBackground::Theme(WallpaperTheme::DeepOcean),
            notifications: Vec::new(),
            snap_preview: None,
            spotlight: crate::gui::spotlight::Spotlight::new(),
        }
    }

    /// Sets the active wallpaper theme.
    pub fn set_wallpaper_theme(&mut self, theme: WallpaperTheme) {
        self.wallpaper_theme = theme;
        self.desktop_background = crate::gui::wallpaper::DesktopBackground::Theme(theme);
    }

    /// Sets a custom PPM image as the live desktop wallpaper.
    pub fn set_custom_wallpaper(&mut self, ppm: crate::gui::wallpaper::PpmImage) {
        self.desktop_background = crate::gui::wallpaper::DesktopBackground::Custom(ppm);
    }

    /// Pushes a desktop notification banner to the top-right corner.
    pub fn push_notification(&mut self, title: String, message: String, color: Color) {
        if self.notifications.len() > 3 {
            self.notifications.remove(0);
        }
        self.notifications.push(NotificationToast {
            title,
            message,
            color,
            ticks_remaining: 180, // ~3 seconds at 60 FPS
        });
    }

    /// Spawns a new application window and brings it to the top of the Z-stack.
    pub fn create_window(
        &mut self,
        app_id: AppId,
        title: &str,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
        pid: Option<u64>,
    ) -> u32 {
        let id = self.next_window_id;
        self.next_window_id += 1;

        // Unfocus all current windows
        for win in self.windows.iter_mut() {
            win.is_focused = false;
        }

        let mut win = Window::new(id, app_id, title, x, y, width, height, pid);
        win.is_focused = true;
        win.z_order = self.windows.len();
        self.windows.push(win);

        id
    }

    /// Closes a window by ID and re-focuses the top remaining window.
    pub fn close_window(&mut self, id: u32) -> Option<u64> {
        if let Some(pos) = self.windows.iter().position(|w| w.id == id) {
            let pid = self.windows[pos].pid;
            self.windows.remove(pos);

            // Re-focus top remaining window
            if let Some(top) = self.windows.last_mut() {
                top.is_focused = true;
            }
            pid
        } else {
            None
        }
    }

    /// Brings the specified window to the foreground (top of Z-order).
    pub fn focus_window(&mut self, id: u32) {
        if let Some(pos) = self.windows.iter().position(|w| w.id == id) {
            for win in self.windows.iter_mut() {
                win.is_focused = false;
            }
            let mut win = self.windows.remove(pos);
            win.is_focused = true;
            win.is_minimized = false;
            self.windows.push(win);
        }
    }

    /// Returns a reference to the currently focused window.
    pub fn focused_window(&self) -> Option<&Window> {
        self.windows.iter().rev().find(|w| w.is_focused && !w.is_minimized && !w.is_closed)
    }

    /// Returns a mutable reference to the currently focused window.
    pub fn focused_window_mut(&mut self) -> Option<&mut Window> {
        self.windows.iter_mut().rev().find(|w| w.is_focused && !w.is_minimized && !w.is_closed)
    }

    /// Finds a window by AppId.
    pub fn window_by_app_id(&self, app_id: AppId) -> Option<&Window> {
        self.windows.iter().find(|w| w.app_id == app_id && !w.is_closed)
    }

    /// Finds a mutable window by AppId.
    pub fn window_by_app_id_mut(&mut self, app_id: AppId) -> Option<&mut Window> {
        self.windows.iter_mut().find(|w| w.app_id == app_id && !w.is_closed)
    }

    /// Handles mouse button down events.
    pub fn handle_mouse_down(&mut self, x: i32, y: i32) -> WmAction {
        self.mouse_x = x;
        self.mouse_y = y;
        self.mouse_down = true;

        // 1. Check Menu Bar / System Dropdown
        let menu_act = handle_menubar_click(x, y, self.screen_width, &mut self.menu_open);
        match menu_act {
            MenubarAction::OpenAbout => return WmAction::AppLaunched(AppId::AboutDialog),
            MenubarAction::SetWallpaper(theme) => {
                self.set_wallpaper_theme(theme);
                return WmAction::None;
            }
            MenubarAction::Reboot => return WmAction::RebootRequested,
            MenubarAction::ToggleSpotlight => {
                self.spotlight.toggle();
                crate::drivers::speaker::play_sound_effect(crate::drivers::speaker::SoundEffect::WindowOpen);
                return WmAction::None;
            }
            MenubarAction::None => {}
        }

        // 1b. Check Spotlight Modal Click (if visible)
        if self.spotlight.is_visible {
            let modal_w = 500u32;
            let result_height = if self.spotlight.results.is_empty() { 0 } else { (self.spotlight.results.len() as u32 * 28) + 8 };
            let modal_h = 44 + result_height;
            let mx = ((self.screen_width as i32) - modal_w as i32) / 2;
            let my = (self.screen_height as i32) / 4;
            let modal_rect = Rect::new(mx, my, modal_w, modal_h);

            if !modal_rect.contains(x, y) {
                self.spotlight.hide();
            } else {
                let results_y = my + 40;
                for i in 0..self.spotlight.results.len() {
                    let ry = results_y + (i as i32 * 28);
                    let row_rect = Rect::new(mx + 6, ry, modal_w - 12, 26);
                    if row_rect.contains(x, y) {
                        self.spotlight.selected_idx = i;
                        if let Some(app) = self.spotlight.activate_selected() {
                            return WmAction::AppLaunched(app);
                        }
                    }
                }
                return WmAction::None;
            }
        }

        // 2. Check Launcher Dock Click
        if let Some(app_id) = hit_test_dock(self.screen_width, self.screen_height, x, y) {
            if let Some(win) = self.window_by_app_id(app_id) {
                let wid = win.id;
                self.focus_window(wid);
                return WmAction::WindowFocused(wid);
            } else {
                return WmAction::AppLaunched(app_id);
            }
        }

        // 3. Check Windows in reverse Z-order (top-to-bottom)
        let mut clicked_win_idx = None;
        let mut action = WmAction::None;

        for i in (0..self.windows.len()).rev() {
            let win = &self.windows[i];
            if !win.is_minimized && !win.is_closed && win.contains(x, y) {
                clicked_win_idx = Some(i);

                if win.hit_test_close(x, y) {
                    let wid = win.id;
                    let pid = win.pid;
                    self.close_window(wid);
                    return WmAction::WindowClosed(wid, pid);
                } else if win.hit_test_minimize(x, y) {
                    self.windows[i].is_minimized = true;
                    crate::drivers::speaker::play_sound_effect(crate::drivers::speaker::SoundEffect::WindowClose);
                    if let Some(top) = self.windows.iter_mut().rev().find(|w| !w.is_minimized) {
                        top.is_focused = true;
                    }
                    return WmAction::None;
                } else if win.hit_test_maximize(x, y) {
                    let wid = self.windows[i].id;
                    self.windows[i].toggle_maximize(self.screen_width as u32, self.screen_height as u32);
                    crate::drivers::speaker::play_sound_effect(crate::drivers::speaker::SoundEffect::WindowSnap);
                    self.focus_window(wid);
                    return WmAction::WindowFocused(wid);
                } else if win.hit_test_titlebar(x, y) {
                    let ticks = crate::task::get_uptime_ticks();
                    let last_click = self.windows[i].last_titlebar_click_tick;
                    if ticks.saturating_sub(last_click) < 30 {
                        // Double-click detected! Toggle maximize!
                        self.windows[i].last_titlebar_click_tick = 0;
                        let wid = self.windows[i].id;
                        self.windows[i].toggle_maximize(self.screen_width as u32, self.screen_height as u32);
                        crate::drivers::speaker::play_sound_effect(crate::drivers::speaker::SoundEffect::WindowSnap);
                        self.focus_window(wid);
                        return WmAction::WindowFocused(wid);
                    } else {
                        self.windows[i].last_titlebar_click_tick = ticks;
                        self.windows[i].is_dragging = true;
                        self.windows[i].drag_offset_x = x - self.windows[i].x;
                        self.windows[i].drag_offset_y = y - self.windows[i].y;
                        action = WmAction::WindowFocused(self.windows[i].id);
                    }
                } else {
                    action = WmAction::WindowFocused(self.windows[i].id);
                }
                break;
            }
        }

        // 4. Bring clicked window to top of Z-stack
        if let Some(idx) = clicked_win_idx {
            let wid = self.windows[idx].id;
            self.focus_window(wid);
        }

        action
    }

    /// Handles mouse movement events and titlebar dragging.
    pub fn handle_mouse_move(&mut self, x: i32, y: i32) {
        self.mouse_x = x;
        self.mouse_y = y;

        let screen_w = self.screen_width as i32;
        let mut snap_detected = None;

        if let Some(win) = self.windows.iter_mut().find(|w| w.is_dragging) {
            // If window was maximized and dragged downwards, unmaximize it and re-center drag offset
            if win.is_maximized && (y - MENUBAR_HEIGHT as i32) > 25 {
                if let Some(saved) = win.saved_bounds {
                    win.width = saved.width;
                    win.height = saved.height;
                    win.is_maximized = false;
                    win.drag_offset_x = (saved.width / 2) as i32;
                }
            }

            let new_x = x - win.drag_offset_x;
            let new_y = y - win.drag_offset_y;

            win.x = new_x.clamp(-(win.width as i32 - 40), self.screen_width as i32 - 40);
            win.y = new_y.clamp(
                MENUBAR_HEIGHT as i32,
                self.screen_height as i32 - 30,
            );

            // Edge snap detection
            if x <= 5 {
                snap_detected = Some(crate::gui::window::SnapTarget::LeftHalf);
            } else if x >= screen_w - 5 {
                snap_detected = Some(crate::gui::window::SnapTarget::RightHalf);
            } else if y <= MENUBAR_HEIGHT as i32 + 5 {
                snap_detected = Some(crate::gui::window::SnapTarget::Maximize);
            }
        }

        self.snap_preview = snap_detected;
    }

    /// Handles mouse button release events.
    pub fn handle_mouse_up(&mut self, _x: i32, _y: i32) {
        self.mouse_down = false;
        let snap_target = self.snap_preview.take();

        for win in self.windows.iter_mut() {
            if win.is_dragging {
                win.is_dragging = false;
                if let Some(target) = snap_target {
                    win.snap_to(target, self.screen_width as u32, self.screen_height as u32);
                    crate::drivers::speaker::play_sound_effect(crate::drivers::speaker::SoundEffect::WindowSnap);
                }
            }
        }
    }

    /// Composites the full desktop environment to the backbuffer.
    /// `render_client` draws one window's application content. It is invoked
    /// immediately after that window's frame, inside the window's own scissor
    /// rect, so a lower window's content can never land on top of a higher
    /// window's frame. Drawing every frame first and every client afterwards
    /// would break exactly that.
    pub fn render_desktop(
        &mut self,
        fb: &mut Framebuffer,
        uptime_secs: u64,
        cpu_percent: u32,
        used_ram: u64,
        total_ram: u64,
        render_client: &mut dyn FnMut(&Window, &mut Framebuffer),
    ) {
        // 1. Desktop Background (built-in gradient or custom VFS PPM)
        crate::gui::wallpaper::render_background(
            fb,
            &self.desktop_background,
            self.screen_width,
            self.screen_height,
        );

        // 1b. Translucent Snap Preview Ghost
        if let Some(snap) = self.snap_preview {
            let menubar_h = MENUBAR_HEIGHT;
            let dock_clearance = 60;
            let workspace_h = (self.screen_height as u32).saturating_sub(menubar_h + dock_clearance);
            let snap_rect = match snap {
                crate::gui::window::SnapTarget::Maximize => {
                    Rect::new(0, menubar_h as i32, self.screen_width as u32, workspace_h)
                }
                crate::gui::window::SnapTarget::LeftHalf => {
                    Rect::new(0, menubar_h as i32, (self.screen_width / 2) as u32, workspace_h)
                }
                crate::gui::window::SnapTarget::RightHalf => {
                    Rect::new((self.screen_width / 2) as i32, menubar_h as i32, (self.screen_width / 2) as u32, workspace_h)
                }
            };
            draw_rounded_rect(fb, snap_rect, 8, Color::rgba(60, 140, 240, 45));
            draw_rect_outline(fb, snap_rect, Color::rgb(70, 160, 255), 2);
        }

        // 2. Render Windows in Z-order (bottom to top): frame then client content,
        //    one window at a time, each client scissored to its own client area.
        for win in self.windows.iter() {
            win.render_frame(fb);

            if !win.is_minimized && !win.is_closed {
                let previous_clip = fb.set_clip(win.client_rect());
                render_client(win, fb);
                fb.restore_clip(previous_clip);
            }
        }

        // 3. Render Top System Menu Bar (24px)
        let active_title = self.focused_window().map(|w| w.title.as_str()).unwrap_or("AegisOS");
        render_menubar(
            fb,
            self.screen_width,
            active_title,
            uptime_secs,
            cpu_percent,
            used_ram,
            total_ram,
            self.menu_open,
        );

        // 4. Render Bottom Launcher Dock
        let running: Vec<AppId> = self
            .windows
            .iter()
            .filter(|w| !w.is_closed)
            .map(|w| w.app_id)
            .collect();
        let minimized: Vec<AppId> = self
            .windows
            .iter()
            .filter(|w| !w.is_closed && w.is_minimized)
            .map(|w| w.app_id)
            .collect();
        render_dock(
            fb,
            self.screen_width,
            self.screen_height,
            self.mouse_x,
            self.mouse_y,
            &running,
            &minimized,
        );

        // 5. Render Notification Toasts (Top-Right Corner)
        let mut toast_y = MENUBAR_HEIGHT as i32 + 8;
        let mut expired = Vec::new();

        for (idx, toast) in self.notifications.iter_mut().enumerate() {
            let toast_w = 340;
            let toast_h = 44;
            let toast_x = self.screen_width as i32 - toast_w - 12;
            let toast_rect = Rect::new(toast_x, toast_y, toast_w as u32, toast_h as u32);

            // Toast body is translucent (alpha 245).
            draw_shadow(fb, toast_rect, 6, 120, None);
            draw_rounded_rect(fb, toast_rect, 8, Color::rgba(25, 28, 35, 245));
            draw_rect_outline(fb, toast_rect, Color::rgb(60, 65, 80), 1);

            // Left accent pill
            draw_rounded_rect(
                fb,
                Rect::new(toast_x + 4, toast_y + 4, 4, (toast_h - 8) as u32),
                2,
                toast.color,
            );

            draw_string(fb, toast_x + 14, toast_y + 6, &toast.title, Color::WHITE, None);
            draw_string(fb, toast_x + 14, toast_y + 22, &toast.message, Color::TEXT_PRIMARY, None);

            toast_y += toast_h + 8;
            if toast.ticks_remaining > 0 {
                toast.ticks_remaining -= 1;
            } else {
                expired.push(idx);
            }
        }

        for &idx in expired.iter().rev() {
            self.notifications.remove(idx);
        }

        // 6. Render Spotlight Overlay (if visible)
        self.spotlight.render(fb, self.screen_width, self.screen_height);

        // 7. Render Mouse Cursor Overlay
        draw_cursor(fb, self.mouse_x, self.mouse_y);
    }
}
