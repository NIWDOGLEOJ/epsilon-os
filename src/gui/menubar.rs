//! macOS-Inspired 24px Top System Menu Bar for AegisOS
//!
//! Renders Aegis Shield logo, active application title, contextual menus,
//! real-time CPU % gauge, live RAM footprint telemetry (< 60MB verification),
//! system uptime clock, and interactive system dropdown menu for wallpaper switching.

use crate::drivers::framebuffer::Framebuffer;
use crate::gui::font::{draw_shield_icon, draw_string, FONT_WIDTH};
use crate::gui::primitives::{draw_gradient_v, draw_rect, draw_rect_outline, draw_rounded_rect, Color, Rect};

pub const MENUBAR_HEIGHT: u32 = 24;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WallpaperTheme {
    DeepOcean,
    CyberTwilight,
    EmeraldForest,
    MidnightSlate,
    SunsetHorizon,
    SolarFlare,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenubarAction {
    None,
    OpenAbout,
    SetWallpaper(WallpaperTheme),
    Reboot,
    ToggleSpotlight,
}

/// Renders the top menu bar across the screen width.
pub fn render_menubar(
    fb: &mut Framebuffer,
    width: usize,
    active_app_title: &str,
    uptime_secs: u64,
    cpu_percent: u32,
    used_ram_bytes: u64,
    _total_ram_bytes: u64,
    menu_open: bool,
) {
    let w = width as u32;

    // 1. Translucent Gradient Background & Bottom Divider
    let bar_rect = Rect::new(0, 0, w, MENUBAR_HEIGHT);
    draw_gradient_v(fb, bar_rect, Color::rgb(32, 34, 38), Color::rgb(22, 24, 28));
    draw_rect(fb, Rect::new(0, (MENUBAR_HEIGHT - 1) as i32, w, 1), Color::MENUBAR_BORDER);

    // 2. Aegis Shield Logo (16x16 at x=8, y=4)
    let logo_bg = if menu_open { Color::rgb(60, 65, 75) } else { Color::rgb(22, 24, 28) };
    draw_rounded_rect(fb, Rect::new(4, 2, 24, 20), 4, logo_bg);
    draw_shield_icon(fb, 8, 4, Color::YELLOW);

    // 3. OS Brand Title
    draw_string(fb, 32, 4, "AegisOS", Color::WHITE, None);

    // 4. Active Application Indicator
    let mut curr_x = 105;
    if !active_app_title.is_empty() {
        curr_x = draw_string(fb, curr_x, 4, active_app_title, Color::WHITE, None) + 20;
    }

    // 5. Contextual Menus ("File", "Edit", "View", "Window", "Help")
    let menus = ["File", "Edit", "View", "Window", "Help"];
    for menu in menus.iter() {
        if curr_x + 50 > (w as i32 - 350) {
            break; // Avoid colliding with telemetry badges on smaller resolutions
        }
        draw_string(fb, curr_x, 4, menu, Color::TEXT_DIM, None);
        // Codepoints, not bytes: `len()` would over-advance on any non-ASCII label.
        curr_x += (menu.chars().count() as i32 * FONT_WIDTH as i32) + 16;
    }

    // 5b. Spotlight Search Button (at w - 336)
    let search_x = w as i32 - 336;
    draw_rounded_rect(fb, Rect::new(search_x, 2, 26, 20), 4, Color::rgb(40, 44, 52));
    draw_string(fb, search_x + 6, 4, "Q", Color::rgb(100, 230, 245), None);

    // 6. Right Telemetry: CPU Utilization Badge
    let cpu_x = w as i32 - 300;
    let cpu_color = if cpu_percent < 50 {
        Color::TEXT_HIGHLIGHT
    } else if cpu_percent < 80 {
        Color::TEXT_WARNING
    } else {
        Color::TEXT_DANGER
    };
    draw_rounded_rect(fb, Rect::new(cpu_x, 2, 85, 20), 4, Color::rgb(40, 44, 52));
    let mut cpu_buf = [0u8; 16];
    let cpu_str = format_cpu_badge(&mut cpu_buf, cpu_percent);
    draw_string(fb, cpu_x + 6, 4, cpu_str, cpu_color, None);

    // 7. Right Telemetry: Live RAM Footprint Badge (< 60MB RAM check)
    let ram_x = w as i32 - 205;
    let used_mb_tenths = (used_ram_bytes * 10) / (1024 * 1024);
    let ram_color = if used_mb_tenths <= 600 {
        Color::TEXT_HIGHLIGHT // Green: < 60MB Target Met!
    } else {
        Color::TEXT_DANGER
    };
    draw_rounded_rect(fb, Rect::new(ram_x, 2, 105, 20), 4, Color::rgb(40, 44, 52));
    let mut ram_buf = [0u8; 20];
    let ram_str = format_ram_badge(&mut ram_buf, used_mb_tenths);
    draw_string(fb, ram_x + 6, 4, ram_str, ram_color, None);

    // 8. Right Telemetry: Uptime Clock [ HH:MM:SS ]
    let clock_x = w as i32 - 90;
    draw_rounded_rect(fb, Rect::new(clock_x, 2, 82, 20), 4, Color::rgb(40, 44, 52));
    let mut clock_buf = [0u8; 16];
    let clock_str = format_clock(&mut clock_buf, uptime_secs);
    draw_string(fb, clock_x + 6, 4, clock_str, Color::TEXT_PRIMARY, None);

    // 9. Dropdown Menu (If active)
    if menu_open {
        let menu_rect = Rect::new(4, MENUBAR_HEIGHT as i32 + 2, 210, 140);
        draw_rounded_rect(fb, menu_rect, 6, Color::rgba(25, 28, 34, 245));
        draw_rect_outline(fb, menu_rect, Color::rgb(60, 65, 78), 1);

        let items = [
            "About AegisOS",
            "Theme: Deep Ocean",
            "Theme: Cyber Twilight",
            "Theme: Emerald Forest",
            "Theme: Midnight Slate",
            "Restart System",
        ];

        for (i, item) in items.iter().enumerate() {
            let iy = menu_rect.y + 6 + (i as i32 * 21);
            if i == 5 {
                draw_rect(fb, Rect::new(menu_rect.x + 8, iy - 2, 194, 1), Color::rgb(50, 55, 65));
            }
            let text_color = if i == 5 { Color::rgb(255, 100, 100) } else { Color::WHITE };
            draw_string(fb, menu_rect.x + 12, iy, item, text_color, None);
        }
    }
}

/// Hit-tests a mouse click on the menu bar or open dropdown menu.
pub fn handle_menubar_click(x: i32, y: i32, screen_width: usize, menu_open: &mut bool) -> MenubarAction {
    if y <= MENUBAR_HEIGHT as i32 {
        if x < 40 {
            *menu_open = !*menu_open;
            return MenubarAction::None;
        }
        let search_x = screen_width as i32 - 336;
        if x >= search_x && x <= search_x + 26 {
            return MenubarAction::ToggleSpotlight;
        }
    }

    if *menu_open {
        let menu_rect = Rect::new(4, MENUBAR_HEIGHT as i32 + 2, 210, 140);
        if menu_rect.contains(x, y) {
            let rel_y = y - (menu_rect.y + 6);
            let item_idx = rel_y / 21;
            *menu_open = false;

            return match item_idx {
                0 => MenubarAction::OpenAbout,
                1 => MenubarAction::SetWallpaper(WallpaperTheme::DeepOcean),
                2 => MenubarAction::SetWallpaper(WallpaperTheme::CyberTwilight),
                3 => MenubarAction::SetWallpaper(WallpaperTheme::EmeraldForest),
                4 => MenubarAction::SetWallpaper(WallpaperTheme::MidnightSlate),
                5 => MenubarAction::Reboot,
                _ => MenubarAction::None,
            };
        } else {
            *menu_open = false;
        }
    }

    MenubarAction::None
}

fn format_cpu_badge<'a>(buf: &'a mut [u8], cpu: u32) -> &'a str {
    let prefix = b"CPU: ";
    buf[..5].copy_from_slice(prefix);
    let val = cpu.min(100);
    if val >= 100 {
        buf[5] = b'1';
        buf[6] = b'0';
        buf[7] = b'0';
        buf[8] = b'%';
        core::str::from_utf8(&buf[..9]).unwrap_or("CPU: 100%")
    } else {
        buf[5] = b'0' + (val / 10) as u8;
        buf[6] = b'0' + (val % 10) as u8;
        buf[7] = b'%';
        core::str::from_utf8(&buf[..8]).unwrap_or("CPU: 00%")
    }
}

fn format_ram_badge<'a>(buf: &'a mut [u8], mb_tenths: u64) -> &'a str {
    let prefix = b"RAM: ";
    buf[..5].copy_from_slice(prefix);
    let mb = mb_tenths / 10;
    let tenth = mb_tenths % 10;

    let mut pos = 5;
    if mb >= 100 {
        buf[pos] = b'0' + ((mb / 100) % 10) as u8;
        pos += 1;
    }
    buf[pos] = b'0' + ((mb / 10) % 10) as u8;
    pos += 1;
    buf[pos] = b'0' + (mb % 10) as u8;
    pos += 1;
    buf[pos] = b'.';
    pos += 1;
    buf[pos] = b'0' + (tenth % 10) as u8;
    pos += 1;
    buf[pos] = b'M';
    pos += 1;
    buf[pos] = b'B';
    pos += 1;

    core::str::from_utf8(&buf[..pos]).unwrap_or("RAM: 16.0MB")
}

fn format_clock<'a>(buf: &'a mut [u8], total_secs: u64) -> &'a str {
    let hours = (total_secs / 3600) % 100;
    let mins = (total_secs / 60) % 60;
    let secs = total_secs % 60;

    buf[0] = b'0' + (hours / 10) as u8;
    buf[1] = b'0' + (hours % 10) as u8;
    buf[2] = b':';
    buf[3] = b'0' + (mins / 10) as u8;
    buf[4] = b'0' + (mins % 10) as u8;
    buf[5] = b':';
    buf[6] = b'0' + (secs / 10) as u8;
    buf[7] = b'0' + (secs % 10) as u8;

    core::str::from_utf8(&buf[..8]).unwrap_or("00:00:00")
}
