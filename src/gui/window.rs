//! macOS Floating Application Window Structure & Frame Renderer
//!
//! Provides draggable titlebars, traffic-light close/minimize/maximize buttons,
//! active window focus styling, drop shadows, and client area geometry.

use alloc::string::{String, ToString};

use crate::drivers::framebuffer::Framebuffer;
use crate::gui::dock::AppId;
use crate::gui::font::{draw_string, measure_string};
use crate::gui::primitives::{
    draw_circle, draw_circle_outline, draw_gradient_v, draw_rect, draw_rect_outline,
    draw_rounded_rect, draw_shadow, Color, Rect,
};

pub const TITLEBAR_HEIGHT: u32 = 24;

/// Target region for window edge snapping and tiling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapTarget {
    Maximize,
    LeftHalf,
    RightHalf,
}

// ============================================================================
// Window Structure & State
// ============================================================================

pub struct Window {
    pub id: u32,
    pub app_id: AppId,
    pub title: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub is_focused: bool,
    pub is_dragging: bool,
    pub is_minimized: bool,
    pub is_closed: bool,
    pub is_maximized: bool,
    pub saved_bounds: Option<Rect>,
    pub last_titlebar_click_tick: u64,
    pub drag_offset_x: i32,
    pub drag_offset_y: i32,
    pub z_order: usize,
    pub pid: Option<u64>,
}

impl Window {
    /// Creates a new floating application window.
    pub fn new(
        id: u32,
        app_id: AppId,
        title: &str,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
        pid: Option<u64>,
    ) -> Self {
        Self {
            id,
            app_id,
            title: title.to_string(),
            x,
            y,
            width,
            height,
            is_focused: true,
            is_dragging: false,
            is_minimized: false,
            is_closed: false,
            is_maximized: false,
            saved_bounds: None,
            last_titlebar_click_tick: 0,
            drag_offset_x: 0,
            drag_offset_y: 0,
            z_order: 0,
            pid,
        }
    }

    /// Toggles window between maximized state and previous floating bounds.
    pub fn toggle_maximize(&mut self, screen_w: u32, screen_h: u32) {
        if self.is_maximized {
            self.restore();
        } else {
            self.snap_to(SnapTarget::Maximize, screen_w, screen_h);
        }
    }

    /// Snaps window to a target workspace region (Maximize, LeftHalf, RightHalf).
    pub fn snap_to(&mut self, target: SnapTarget, screen_w: u32, screen_h: u32) {
        if self.saved_bounds.is_none() {
            self.saved_bounds = Some(self.bounds());
        }

        let menubar_h = 24;
        let dock_clearance = 60;
        let workspace_h = screen_h.saturating_sub(menubar_h + dock_clearance);

        match target {
            SnapTarget::Maximize => {
                self.x = 0;
                self.y = menubar_h as i32;
                self.width = screen_w;
                self.height = workspace_h;
            }
            SnapTarget::LeftHalf => {
                self.x = 0;
                self.y = menubar_h as i32;
                self.width = screen_w / 2;
                self.height = workspace_h;
            }
            SnapTarget::RightHalf => {
                self.x = (screen_w / 2) as i32;
                self.y = menubar_h as i32;
                self.width = screen_w / 2;
                self.height = workspace_h;
            }
        }
        self.is_maximized = true;
    }

    /// Restores the window to its saved pre-snap bounds.
    pub fn restore(&mut self) {
        if let Some(saved) = self.saved_bounds.take() {
            self.x = saved.x;
            self.y = saved.y;
            self.width = saved.width;
            self.height = saved.height;
        }
        self.is_maximized = false;
    }

    #[inline]
    pub fn bounds(&self) -> Rect {
        Rect::new(self.x, self.y, self.width, self.height)
    }

    #[inline]
    pub fn titlebar_rect(&self) -> Rect {
        Rect::new(self.x, self.y, self.width, TITLEBAR_HEIGHT)
    }

    #[inline]
    pub fn client_rect(&self) -> Rect {
        Rect::new(
            self.x + 1,
            self.y + TITLEBAR_HEIGHT as i32,
            self.width.saturating_sub(2),
            self.height.saturating_sub(TITLEBAR_HEIGHT + 1),
        )
    }

    #[inline]
    pub fn contains(&self, px: i32, py: i32) -> bool {
        !self.is_minimized && !self.is_closed && self.bounds().contains(px, py)
    }

    #[inline]
    pub fn hit_test_close(&self, px: i32, py: i32) -> bool {
        let cx = self.x + 16;
        let cy = self.y + 12;
        let dx = px - cx;
        let dy = py - cy;
        (dx * dx + dy * dy) <= 36 // Radius 6px
    }

    #[inline]
    pub fn hit_test_minimize(&self, px: i32, py: i32) -> bool {
        let cx = self.x + 32;
        let cy = self.y + 12;
        let dx = px - cx;
        let dy = py - cy;
        (dx * dx + dy * dy) <= 36
    }

    #[inline]
    pub fn hit_test_maximize(&self, px: i32, py: i32) -> bool {
        let cx = self.x + 48;
        let cy = self.y + 12;
        let dx = px - cx;
        let dy = py - cy;
        (dx * dx + dy * dy) <= 36
    }

    #[inline]
    pub fn hit_test_titlebar(&self, px: i32, py: i32) -> bool {
        self.titlebar_rect().contains(px, py)
            && !self.hit_test_close(px, py)
            && !self.hit_test_minimize(px, py)
            && !self.hit_test_maximize(px, py)
    }

    /// Renders the window outer frame, titlebar, traffic lights, and client background.
    pub fn render_frame(&self, fb: &mut Framebuffer) {
        if self.is_minimized || self.is_closed {
            return;
        }

        let win_rect = self.bounds();

        // 1. Window Drop Shadow
        // WINDOW_BG is opaque, so everything the body covers can be skipped.
        draw_shadow(
            fb,
            win_rect,
            6,
            if self.is_focused { 160 } else { 80 },
            Some((win_rect, 8)),
        );

        // 2. Window Body Background & Border
        draw_rounded_rect(fb, win_rect, 8, Color::WINDOW_BG);
        draw_rect_outline(
            fb,
            win_rect,
            if self.is_focused {
                Color::WINDOW_BORDER
            } else {
                Color::rgb(45, 49, 56)
            },
            1,
        );

        // 3. Titlebar Gradient
        let title_rect = self.titlebar_rect();
        if self.is_focused {
            draw_gradient_v(
                fb,
                title_rect,
                Color::TITLEBAR_ACTIVE_TOP,
                Color::TITLEBAR_ACTIVE_BOTTOM,
            );
        } else {
            draw_rect(fb, title_rect, Color::TITLEBAR_INACTIVE);
        }
        draw_rect(
            fb,
            Rect::new(self.x, self.y + TITLEBAR_HEIGHT as i32 - 1, self.width, 1),
            Color::WINDOW_BORDER,
        );

        // 4. Traffic-Light Buttons (Red, Yellow, Green)
        let red_color = if self.is_focused {
            Color::RED
        } else {
            Color::rgb(180, 80, 70)
        };
        let yellow_color = if self.is_focused {
            Color::YELLOW
        } else {
            Color::rgb(180, 140, 50)
        };
        let green_color = if self.is_focused {
            Color::GREEN
        } else {
            Color::rgb(60, 160, 80)
        };

        // Red Close Button
        draw_circle(fb, self.x + 16, self.y + 12, 6, red_color);
        draw_circle_outline(fb, self.x + 16, self.y + 12, 6, Color::rgba(0, 0, 0, 80));

        // Yellow Minimize Button
        draw_circle(fb, self.x + 32, self.y + 12, 6, yellow_color);
        draw_circle_outline(fb, self.x + 32, self.y + 12, 6, Color::rgba(0, 0, 0, 80));

        // Green Maximize Button
        draw_circle(fb, self.x + 48, self.y + 12, 6, green_color);
        draw_circle_outline(fb, self.x + 48, self.y + 12, 6, Color::rgba(0, 0, 0, 80));

        // 5. Centered Window Title Text
        let (tw, _th) = measure_string(&self.title);
        let title_x = self.x + ((self.width as i32 - tw as i32) / 2);
        let title_y = self.y + 4;
        let text_color = if self.is_focused {
            Color::TEXT_PRIMARY
        } else {
            Color::TEXT_DIM
        };
        draw_string(fb, title_x, title_y, &self.title, text_color, None);

        // 6. Fill Client Area
        draw_rect(fb, self.client_rect(), Color::WINDOW_BG);
    }
}
