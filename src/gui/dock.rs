//! macOS-Style Translucent Launcher Dock for AegisOS
//!
//! Centered at screen bottom (420x48px, 12px radius), featuring 7 clickable
//! system application icons, hover tooltips, and running process status indicators.

use crate::drivers::framebuffer::Framebuffer;
use crate::gui::font::{
    draw_about_icon, draw_calc_icon, draw_crash_icon, draw_editor_icon, draw_pulse_icon,
    draw_snake_icon, draw_string, draw_terminal_icon, measure_string,
};
use crate::gui::primitives::{
    draw_circle, draw_rounded_rect, draw_rounded_rect_outline, draw_shadow, Color, Rect,
};

pub const DOCK_WIDTH: u32 = 420;
pub const DOCK_HEIGHT: u32 = 48;
pub const DOCK_RADIUS: u32 = 12;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppId {
    CrashTest,
    ActivityMonitor,
    Terminal,
    AegisPad,
    Calculator,
    Snake,
    AboutDialog,
}

impl AppId {
    pub const ALL: [AppId; 7] = [
        AppId::CrashTest,
        AppId::ActivityMonitor,
        AppId::Terminal,
        AppId::AegisPad,
        AppId::Calculator,
        AppId::Snake,
        AppId::AboutDialog,
    ];

    pub fn name(&self) -> &'static str {
        match self {
            AppId::CrashTest => "Crash-Test Demo",
            AppId::ActivityMonitor => "Activity Monitor",
            AppId::Terminal => "Terminal Shell",
            AppId::AegisPad => "AegisPad",
            AppId::Calculator => "Calculator",
            AppId::Snake => "Snake Game",
            AppId::AboutDialog => "About AegisOS",
        }
    }
}

/// Calculates dock rectangle on screen.
pub fn get_dock_rect(screen_width: usize, screen_height: usize) -> Rect {
    let x = (screen_width as i32 - DOCK_WIDTH as i32) / 2;
    let y = screen_height as i32 - DOCK_HEIGHT as i32 - 8;
    Rect::new(x, y, DOCK_WIDTH, DOCK_HEIGHT)
}

/// Renders the launcher dock at the bottom of the screen.
pub fn render_dock(
    fb: &mut Framebuffer,
    screen_width: usize,
    screen_height: usize,
    mouse_x: i32,
    mouse_y: i32,
    running_apps: &[AppId],
) {
    let dock_rect = get_dock_rect(screen_width, screen_height);

    // 1. Soft Blurred Drop Shadow
    // DOCK_BG is translucent, so the shadow under it stays visible.
    draw_shadow(fb, dock_rect, 6, 140, None);

    // 2. Translucent Rounded Pill Container & Border
    draw_rounded_rect(fb, dock_rect, DOCK_RADIUS, Color::DOCK_BG);
    draw_rounded_rect_outline(fb, dock_rect, DOCK_RADIUS, Color::DOCK_BORDER);

    // 3. Render 7 App Icons
    let slot_width = DOCK_WIDTH / 7;
    let mut hovered_app: Option<AppId> = None;

    for (i, &app) in AppId::ALL.iter().enumerate() {
        let slot_x = dock_rect.x + (i as u32 * slot_width) as i32;
        let slot_rect = Rect::new(slot_x, dock_rect.y, slot_width, DOCK_HEIGHT);

        let icon_x = slot_x + ((slot_width as i32 - 24) / 2);
        let icon_y = dock_rect.y + 8;

        // Check hover
        let is_hovered = slot_rect.contains(mouse_x, mouse_y);
        if is_hovered {
            hovered_app = Some(app);
            // Draw subtle hover glow
            draw_rounded_rect(
                fb,
                Rect::new(slot_x + 3, dock_rect.y + 4, slot_width - 6, DOCK_HEIGHT - 8),
                8,
                Color::rgba(255, 255, 255, 30),
            );
        }

        // Draw specific icon
        match app {
            AppId::CrashTest => draw_crash_icon(fb, icon_x, icon_y),
            AppId::ActivityMonitor => draw_pulse_icon(fb, icon_x, icon_y),
            AppId::Terminal => draw_terminal_icon(fb, icon_x, icon_y),
            AppId::AegisPad => draw_editor_icon(fb, icon_x, icon_y),
            AppId::Calculator => draw_calc_icon(fb, icon_x, icon_y),
            AppId::Snake => draw_snake_icon(fb, icon_x, icon_y),
            AppId::AboutDialog => draw_about_icon(fb, icon_x, icon_y),
        }

        // 4. Draw Running Indicator Dot (3px white dot below active app)
        if running_apps.contains(&app) {
            let dot_x = slot_x + (slot_width as i32 / 2);
            let dot_y = dock_rect.y + DOCK_HEIGHT as i32 - 6;
            draw_circle(fb, dot_x, dot_y, 2, Color::WHITE);
        }
    }

    // 5. Render Hover Tooltip if hovering over any dock icon
    if let Some(app) = hovered_app {
        let title = app.name();
        let (tw, th) = measure_string(title);
        let tip_w = tw + 16;
        let tip_h = th + 8;
        let tip_x = mouse_x - (tip_w as i32 / 2);
        let tip_y = dock_rect.y - tip_h as i32 - 6;

        let tip_rect = Rect::new(tip_x, tip_y, tip_w, tip_h);
        // Tooltip body is translucent (alpha 240).
        draw_shadow(fb, tip_rect, 4, 100, None);
        draw_rounded_rect(fb, tip_rect, 6, Color::rgba(20, 22, 26, 240));
        draw_rounded_rect_outline(fb, tip_rect, 6, Color::rgb(60, 65, 75));
        draw_string(fb, tip_x + 8, tip_y + 4, title, Color::WHITE, None);
    }
}

/// Hit-tests a mouse click against the dock icons.
pub fn hit_test_dock(
    screen_width: usize,
    screen_height: usize,
    mouse_x: i32,
    mouse_y: i32,
) -> Option<AppId> {
    let dock_rect = get_dock_rect(screen_width, screen_height);
    if !dock_rect.contains(mouse_x, mouse_y) {
        return None;
    }

    let slot_width = DOCK_WIDTH / 7;
    let rel_x = (mouse_x - dock_rect.x) as u32;
    let index = (rel_x / slot_width).min(6) as usize;

    Some(AppId::ALL[index])
}
