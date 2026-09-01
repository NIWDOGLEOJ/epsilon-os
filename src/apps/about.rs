//! About AegisOS Modal Dialog Application
//!
//! Presents system branding, kernel specifications, memory architecture,
//! and hardware privilege separation overview.

use crate::drivers::framebuffer::Framebuffer;
use crate::gui::font::{draw_shield_icon, draw_string, measure_string};
use crate::gui::primitives::{draw_rect_outline, draw_rounded_rect, Color, Rect};
use crate::gui::window::Window;

pub struct AboutDialogApp {}

impl AboutDialogApp {
    pub fn new() -> Self {
        Self {}
    }

    /// Renders the About AegisOS dialog.
    pub fn render(&self, win: &Window, fb: &mut Framebuffer) {
        let client = win.client_rect();
        if client.width < 300 || client.height < 250 {
            return;
        }

        // Center Shield Logo
        let logo_x = client.x + ((client.width as i32 - 16) / 2);
        draw_shield_icon(fb, logo_x, client.y + 14, Color::YELLOW);

        // Title & Version
        let title = "AegisOS";
        let (tw, _) = measure_string(title);
        draw_string(
            fb,
            client.x + ((client.width as i32 - tw as i32) / 2),
            client.y + 36,
            title,
            Color::WHITE,
            None,
        );

        let ver = "Version 1.0.0 (x86_64 Long Mode)";
        let (vw, _) = measure_string(ver);
        draw_string(
            fb,
            client.x + ((client.width as i32 - vw as i32) / 2),
            client.y + 54,
            ver,
            Color::TEXT_DIM,
            None,
        );

        // System Specs Box
        let box_w = client.width.saturating_sub(24);
        let box_h = 130;
        let box_rect = Rect::new(client.x + 12, client.y + 76, box_w, box_h);
        draw_rounded_rect(fb, box_rect, 6, Color::rgb(26, 29, 36));
        draw_rect_outline(fb, box_rect, Color::rgb(50, 56, 70), 1);

        // Report the mode actually in use rather than a hardcoded 1024x768.
        // Read straight off the `fb` we were handed: the compositor already holds
        // the FRAMEBUFFER lock while rendering, so calling
        // `framebuffer::get_dimensions()` here would self-deadlock on it.
        let mut display_buf = [0u8; 48];
        let display = format_display_mode(&mut display_buf, fb.width(), fb.height());

        let specs = [
            ("Kernel:", "Aegis Microkernel (Rust no_std)"),
            ("Bootloader:", "Limine Boot Protocol v2"),
            ("Privilege:", "Hardware Ring 0 / Ring 3 Separation"),
            ("Fault Handler:", "Isolated Process Reaping (Zero Panic)"),
            ("Memory Usage:", "< 60MB Active Footprint at Idle"),
            ("Display:", display),
        ];

        for (i, &(label, val)) in specs.iter().enumerate() {
            let sy = box_rect.y + 8 + (i as i32 * 19);
            draw_string(fb, box_rect.x + 10, sy, label, Color::TEXT_DIM, None);
            draw_string(fb, box_rect.x + 115, sy, val, Color::TEXT_PRIMARY, None);
        }

        // [ OK ] Action Button
        let btn_w = 80;
        let btn_h = 24;
        let btn_x = client.x + ((client.width as i32 - btn_w) / 2);
        let btn_y = client.bottom() - 32;
        let btn_rect = Rect::new(btn_x, btn_y, btn_w as u32, btn_h as u32);

        draw_rounded_rect(fb, btn_rect, 4, Color::BLUE);
        let (ok_w, _) = measure_string("OK");
        draw_string(
            fb,
            btn_x + ((btn_w - ok_w as i32) / 2),
            btn_y + 4,
            "OK",
            Color::WHITE,
            None,
        );
    }

    /// Returns true if the [ OK ] button was clicked.
    pub fn handle_click(&self, win: &Window, px: i32, py: i32) -> bool {
        let client = win.client_rect();
        let btn_w = 80;
        let btn_h = 24;
        let btn_x = client.x + ((client.width as i32 - btn_w) / 2);
        let btn_y = client.bottom() - 32;
        let btn_rect = Rect::new(btn_x, btn_y, btn_w as u32, btn_h as u32);

        btn_rect.contains(px, py)
    }
}

/// Formats "<w>x<h>x<bpp> Double-Buffered RGB" into `buf`.
///
/// Stack buffer rather than `format!`: this runs once per frame while the About
/// window is open, and the app render paths deliberately avoid allocating.
fn format_display_mode<'a>(buf: &'a mut [u8], width: usize, height: usize) -> &'a str {
    // Backbuffer pixels are u32, so the depth follows the type rather than a literal.
    const BPP: usize = core::mem::size_of::<u32>() * 8;

    let mut len = 0;
    let push = |buf: &mut [u8], len: &mut usize, bytes: &[u8]| {
        for &b in bytes {
            if *len < buf.len() {
                buf[*len] = b;
                *len += 1;
            }
        }
    };

    let mut num = [0u8; 20];
    push(buf, &mut len, format_usize(&mut num, width).as_bytes());
    push(buf, &mut len, b"x");
    push(buf, &mut len, format_usize(&mut num, height).as_bytes());
    push(buf, &mut len, b"x");
    push(buf, &mut len, format_usize(&mut num, BPP).as_bytes());
    push(buf, &mut len, b" Double-Buffered RGB");

    core::str::from_utf8(&buf[..len]).unwrap_or("Double-Buffered RGB")
}

/// Decimal-formats `val` into `buf`.
fn format_usize<'a>(buf: &'a mut [u8], mut val: usize) -> &'a str {
    if val == 0 {
        buf[0] = b'0';
        return core::str::from_utf8(&buf[..1]).unwrap_or("0");
    }

    let mut digits = [0u8; 20];
    let mut len = 0;
    while val > 0 && len < digits.len() {
        digits[len] = b'0' + (val % 10) as u8;
        val /= 10;
        len += 1;
    }
    for i in 0..len {
        buf[i] = digits[len - 1 - i];
    }
    core::str::from_utf8(&buf[..len]).unwrap_or("0")
}
