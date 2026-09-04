//! Embedded 8x16 Bitmap Font and System Icon Rasterizers
//!
//! Features complete ASCII 32..126 bitmap font rendering with transparency,
//! foreground/background color blending, string measurement, and 16x16 vector/bitmap
//! system icons for the macOS Menu Bar and Dock.

use crate::drivers::framebuffer::Framebuffer;
use crate::gui::primitives::{draw_line, draw_rect, Color, Rect};

pub const FONT_WIDTH: usize = 8;
pub const FONT_HEIGHT: usize = 16;

// ============================================================================
// 8x16 Bitmap Font Table for ASCII 32..127 (16 bytes per glyph)
// ============================================================================

pub static FONT_8X16: [u8; 96 * 16] = include!("../../shared/font8x16.inc");

// ============================================================================
// Text Rendering Routines
// ============================================================================

/// Draws a single character glyph at pixel coordinates (x, y).

// ============================================================================
// Supplementary Glyphs for Non-ASCII Codepoints Used by the UI
// ============================================================================

/// 8x16 glyphs for the handful of non-ASCII codepoints the desktop actually uses.
///
/// The font is otherwise ASCII 32..126. Anything outside this table still falls
/// back to `?`, but it now falls back once per character rather than once per
/// UTF-8 byte -- `draw_string` used to iterate `bytes()`, so a three-byte em dash
/// rendered as `???` and a four-byte emoji as `????`.
static SUPPLEMENTARY_GLYPHS: &[(char, [u8; FONT_HEIGHT])] = &[
    // U+2014 EM DASH: full-width rule on the text baseline-ish centre
    ('\u{2014}', [
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xFF,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    ]),
    // U+00D7 MULTIPLICATION SIGN
    ('\u{00D7}', [
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xC6, 0x6C,
        0x38, 0x6C, 0xC6, 0x00, 0x00, 0x00, 0x00, 0x00,
    ]),
    // U+00F7 DIVISION SIGN
    ('\u{00F7}', [
        0x00, 0x00, 0x00, 0x00, 0x18, 0x18, 0x00, 0xFF,
        0x00, 0x18, 0x18, 0x00, 0x00, 0x00, 0x00, 0x00,
    ]),
    // U+00B1 PLUS-MINUS SIGN
    ('\u{00B1}', [
        0x00, 0x00, 0x18, 0x18, 0x18, 0x7E, 0x18, 0x18,
        0x18, 0x00, 0x7E, 0x00, 0x00, 0x00, 0x00, 0x00,
    ]),
    // U+2713 CHECK MARK
    ('\u{2713}', [
        0x00, 0x00, 0x00, 0x02, 0x06, 0x0C, 0x18, 0xD8,
        0xF0, 0x60, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    ]),
    // U+26A0 WARNING SIGN: outlined triangle enclosing a centred exclamation.
    ('\u{26A0}', [
        0x00, 0x00, 0x18, 0x18, 0x24, 0x24, 0x5A, 0x5A,
        0x99, 0x99, 0x81, 0x99, 0x81, 0xFF, 0x00, 0x00,
    ]),
    // U+1F6E1 SHIELD: solid, tapering to a point.
    ('\u{1F6E1}', [
        0x00, 0x00, 0x7E, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
        0x7E, 0x7E, 0x3C, 0x3C, 0x18, 0x00, 0x00, 0x00,
    ]),
    // U+2190 LEFTWARDS ARROW
    ('\u{2190}', [
        0x00, 0x00, 0x00, 0x08, 0x18, 0x38, 0x7F, 0xFF,
        0x7F, 0x38, 0x18, 0x08, 0x00, 0x00, 0x00, 0x00,
    ]),
    // U+2191 UPWARDS ARROW
    ('\u{2191}', [
        0x00, 0x00, 0x18, 0x3C, 0x7E, 0xFF, 0x18, 0x18,
        0x18, 0x18, 0x18, 0x18, 0x00, 0x00, 0x00, 0x00,
    ]),
    // U+2192 RIGHTWARDS ARROW
    ('\u{2192}', [
        0x00, 0x00, 0x00, 0x10, 0x18, 0x1C, 0xFE, 0xFF,
        0xFE, 0x1C, 0x18, 0x10, 0x00, 0x00, 0x00, 0x00,
    ]),
    // U+2193 DOWNWARDS ARROW
    ('\u{2193}', [
        0x00, 0x00, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18,
        0xFF, 0x7E, 0x3C, 0x18, 0x00, 0x00, 0x00, 0x00,
    ]),
    // U+25B2 BLACK UP-POINTING TRIANGLE
    ('\u{25B2}', [
        0x00, 0x00, 0x18, 0x18, 0x3C, 0x3C, 0x7E, 0x7E,
        0xFF, 0xFF, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    ]),
    // U+25BC BLACK DOWN-POINTING TRIANGLE
    ('\u{25BC}', [
        0x00, 0x00, 0xFF, 0xFF, 0x7E, 0x7E, 0x3C, 0x3C,
        0x18, 0x18, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    ]),
    // U+25C0 BLACK LEFT-POINTING TRIANGLE
    ('\u{25C0}', [
        0x00, 0x00, 0x08, 0x18, 0x38, 0x78, 0xF8, 0x78,
        0x38, 0x18, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00,
    ]),
    // U+25B6 BLACK RIGHT-POINTING TRIANGLE
    ('\u{25B6}', [
        0x00, 0x00, 0x10, 0x18, 0x1C, 0x1E, 0x1F, 0x1E,
        0x1C, 0x18, 0x10, 0x00, 0x00, 0x00, 0x00, 0x00,
    ]),
    // U+2022 BULLET
    ('\u{2022}', [
        0x00, 0x00, 0x00, 0x00, 0x00, 0x3C, 0x7E, 0x7E,
        0x7E, 0x3C, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    ]),
    // U+2026 HORIZONTAL ELLIPSIS
    ('\u{2026}', [
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x6E, 0x6E, 0x00, 0x00,
    ]),
    // U+00B0 DEGREE SIGN
    ('\u{00B0}', [
        0x00, 0x38, 0x44, 0x44, 0x38, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    ]),
    // U+00B2 SUPERSCRIPT TWO
    ('\u{00B2}', [
        0x00, 0x38, 0x44, 0x04, 0x18, 0x20, 0x7C, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    ]),
    // U+00B3 SUPERSCRIPT THREE
    ('\u{00B3}', [
        0x00, 0x38, 0x44, 0x08, 0x04, 0x44, 0x38, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    ]),
    // U+00B5 MICRO SIGN
    ('\u{00B5}', [
        0x00, 0x00, 0x00, 0x00, 0x00, 0x84, 0x84, 0x84,
        0x84, 0xCC, 0xB4, 0x80, 0x80, 0x00, 0x00, 0x00,
    ]),
    // U+00A9 COPYRIGHT SIGN
    ('\u{00A9}', [
        0x00, 0x3C, 0x42, 0x99, 0xA5, 0xA1, 0xA1, 0xA5,
        0x99, 0x42, 0x3C, 0x00, 0x00, 0x00, 0x00, 0x00,
    ]),
    // U+00AE REGISTERED SIGN
    ('\u{00AE}', [
        0x00, 0x3C, 0x42, 0xBD, 0xA5, 0xBD, 0xAD, 0xA5,
        0xA5, 0x42, 0x3C, 0x00, 0x00, 0x00, 0x00, 0x00,
    ]),
    // U+2260 NOT EQUAL TO
    ('\u{2260}', [
        0x00, 0x00, 0x02, 0x04, 0x7E, 0x08, 0x10, 0x7E,
        0x20, 0x40, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    ]),
    // U+2264 LESS-THAN OR EQUAL TO
    ('\u{2264}', [
        0x00, 0x00, 0x08, 0x10, 0x20, 0x40, 0x20, 0x10,
        0x08, 0x00, 0x7E, 0x00, 0x00, 0x00, 0x00, 0x00,
    ]),
    // U+2265 GREATER-THAN OR EQUAL TO
    ('\u{2265}', [
        0x00, 0x00, 0x10, 0x08, 0x04, 0x02, 0x04, 0x08,
        0x10, 0x00, 0x7E, 0x00, 0x00, 0x00, 0x00, 0x00,
    ]),
    // U+2605 BLACK STAR
    ('\u{2605}', [
        0x00, 0x10, 0x10, 0x38, 0xFE, 0x7C, 0x38, 0x6C,
        0xC6, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    ]),
    // U+2665 BLACK HEART SUIT
    ('\u{2665}', [
        0x00, 0x66, 0xFF, 0xFF, 0xFF, 0x7E, 0x3C, 0x18,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    ]),
    // U+221A SQUARE ROOT '√'
    ('\u{221A}', [
        0x00, 0x00, 0x01, 0x01, 0x02, 0x02, 0x04, 0x44,
        0x28, 0x10, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    ]),
    // U+03C0 GREEK SMALL LETTER PI 'π'
    ('\u{03C0}', [
        0x00, 0x00, 0x00, 0x00, 0x00, 0x7E, 0x4A, 0x4A,
        0x4A, 0x4A, 0x4A, 0x69, 0x00, 0x00, 0x00, 0x00,
    ]),
];

/// Zero-width codepoints: consumed without drawing or advancing the pen.
///
/// U+FE0F (VARIATION SELECTOR-16) trails the emoji in the notification strings
/// and is a modifier, not a character to render.
fn is_zero_width(c: char) -> bool {
    matches!(c, '\u{FE0F}' | '\u{200D}' | '\u{FEFF}')
}

/// Looks up the 16-byte bitmap for `c`, or `None` if it should not be drawn.
fn glyph_for(c: char) -> Option<&'static [u8]> {
    if is_zero_width(c) {
        return None;
    }

    let code = c as u32;
    if (32..127).contains(&code) {
        let offset = (code as usize - 32) * FONT_HEIGHT;
        return Some(&FONT_8X16[offset..offset + FONT_HEIGHT]);
    }

    if let Some((_, glyph)) = SUPPLEMENTARY_GLYPHS.iter().find(|(g, _)| *g == c) {
        return Some(glyph.as_slice());
    }

    // Unknown codepoint: one '?' for the whole character.
    let offset = (b'?' - 32) as usize * FONT_HEIGHT;
    Some(&FONT_8X16[offset..offset + FONT_HEIGHT])
}

pub fn draw_char(fb: &mut Framebuffer, x: i32, y: i32, c: u8, fg: Color, bg: Option<Color>) {
    draw_glyph(fb, x, y, c as char, fg, bg);
}

/// Draws a single Unicode codepoint. Zero-width modifiers draw nothing.
pub fn draw_glyph(fb: &mut Framebuffer, x: i32, y: i32, c: char, fg: Color, bg: Option<Color>) {
    let Some(glyph) = glyph_for(c) else {
        return;
    };

    for (row, &byte) in glyph.iter().enumerate() {
        let py = y + row as i32;
        for col in 0..8 {
            let px = x + col as i32;
            let bit = (byte >> (7 - col)) & 1;
            if bit != 0 {
                fb.draw_pixel(px, py, fg);
            } else if let Some(bg_color) = bg {
                fb.draw_pixel(px, py, bg_color);
            }
        }
    }
}

/// Draws an ASCII string horizontally starting at (x, y).
pub fn draw_string(
    fb: &mut Framebuffer,
    mut x: i32,
    y: i32,
    text: &str,
    fg: Color,
    bg: Option<Color>,
) -> i32 {
    // Iterate codepoints, not bytes: a multi-byte character is one glyph, and one
    // fallback '?' if unknown -- never one per UTF-8 byte.
    for c in text.chars() {
        if c == '\n' {
            break;
        }
        if is_zero_width(c) {
            continue;
        }
        draw_glyph(fb, x, y, c, fg, bg);
        x += FONT_WIDTH as i32;
    }
    x
}

/// Measures the pixel width and height of a string, in codepoints.
pub fn measure_string(text: &str) -> (u32, u32) {
    let mut max_len = 0;
    let mut current_len = 0;
    let mut lines = 1;

    for c in text.chars() {
        if is_zero_width(c) {
            continue;
        }
        if c == '\n' {
            lines += 1;
            if current_len > max_len {
                max_len = current_len;
            }
            current_len = 0;
        } else {
            current_len += 1;
        }
    }
    if current_len > max_len {
        max_len = current_len;
    }

    ((max_len * FONT_WIDTH) as u32, (lines * FONT_HEIGHT) as u32)
}

// ============================================================================
// System Vector Icons (16x16 / 32x32)
// ============================================================================

/// Renders the Aegis Shield logo icon (16x16)
pub fn draw_shield_icon(fb: &mut Framebuffer, x: i32, y: i32, color: Color) {
    const SHIELD_BITMAP: [u16; 16] = [
        0b0011111111111100,
        0b0111111111111110,
        0b1111100000011111,
        0b1111011111101111,
        0b1110110000110111,
        0b1110101111010111,
        0b1110101001010111,
        0b1110101001010111,
        0b0111010000101110,
        0b0111011001101110,
        0b0011101111011100,
        0b0011110110111100,
        0b0001111001111000,
        0b0000111111110000,
        0b0000011111100000,
        0b0000000110000000,
    ];

    for (row, &row_bits) in SHIELD_BITMAP.iter().enumerate() {
        let py = y + row as i32;
        for col in 0..16 {
            let px = x + col as i32;
            let bit = (row_bits >> (15 - col)) & 1;
            if bit != 0 {
                fb.draw_pixel(px, py, color);
            }
        }
    }
}

/// Renders Crash-Test App Hazard/Bug Icon (24x24)
pub fn draw_crash_icon(fb: &mut Framebuffer, x: i32, y: i32) {
    let red = Color::RED;
    let white = Color::WHITE;
    for row in 0..22 {
        let span = row as i32;
        let start_x = x + 11 - (span / 2);
        for col in 0..=span {
            fb.draw_pixel(start_x + col, y + row, red);
        }
    }
    for row in 6..14 {
        fb.draw_pixel(x + 11, y + row, white);
        fb.draw_pixel(x + 12, y + row, white);
    }
    fb.draw_pixel(x + 11, y + 17, white);
    fb.draw_pixel(x + 12, y + 17, white);
    fb.draw_pixel(x + 11, y + 18, white);
    fb.draw_pixel(x + 12, y + 18, white);
}

/// Renders Activity Monitor Pulse Icon (24x24)
pub fn draw_pulse_icon(fb: &mut Framebuffer, x: i32, y: i32) {
    let bg = Color::rgb(20, 24, 30);
    let green = Color::GREEN;
    draw_rect(fb, Rect::new(x, y, 24, 24), bg);

    let points: [(i32, i32); 8] = [
        (0, 12),
        (5, 12),
        (8, 4),
        (12, 20),
        (15, 8),
        (18, 14),
        (20, 12),
        (23, 12),
    ];

    for i in 0..points.len() - 1 {
        crate::gui::primitives::draw_line(
            fb,
            x + points[i].0,
            y + points[i].1,
            x + points[i + 1].0,
            y + points[i + 1].1,
            green,
        );
        crate::gui::primitives::draw_line(
            fb,
            x + points[i].0,
            y + points[i].1 + 1,
            x + points[i + 1].0,
            y + points[i + 1].1 + 1,
            green,
        );
    }
}

/// Renders Terminal Console `>_` Icon (24x24)
pub fn draw_terminal_icon(fb: &mut Framebuffer, x: i32, y: i32) {
    let bg = Color::rgb(18, 20, 24);
    let border = Color::rgb(60, 65, 75);
    let green = Color::rgb(80, 250, 123);

    draw_rect(fb, Rect::new(x, y, 24, 24), bg);
    crate::gui::primitives::draw_rect_outline(fb, Rect::new(x, y, 24, 24), border, 1);

    crate::gui::primitives::draw_line(fb, x + 4, y + 6, x + 10, y + 11, green);
    crate::gui::primitives::draw_line(fb, x + 10, y + 11, x + 4, y + 16, green);
    crate::gui::primitives::draw_line(fb, x + 5, y + 6, x + 11, y + 11, green);
    crate::gui::primitives::draw_line(fb, x + 11, y + 11, x + 5, y + 16, green);

    draw_rect(fb, Rect::new(x + 13, y + 15, 7, 2), green);
}

/// Renders AegisPad Text Editor Icon (24x24)
pub fn draw_editor_icon(fb: &mut Framebuffer, x: i32, y: i32) {
    let bg = Color::rgb(240, 240, 245);
    let blue = Color::BLUE;
    let line_color = Color::rgb(160, 170, 185);

    draw_rect(fb, Rect::new(x + 2, y + 2, 20, 20), bg);
    draw_rect(fb, Rect::new(x + 2, y + 2, 20, 4), blue);

    for row in 0..3 {
        let ly = y + 9 + (row * 4);
        draw_rect(fb, Rect::new(x + 5, ly, 14, 2), line_color);
    }
}

/// Renders About AegisOS Modal Dialog Icon (24x24)
pub fn draw_about_icon(fb: &mut Framebuffer, x: i32, y: i32) {
    let gold = Color::YELLOW;
    draw_shield_icon(fb, x + 4, y + 4, gold);
}

/// Renders Calculator App Icon (24x24)
pub fn draw_calc_icon(fb: &mut Framebuffer, x: i32, y: i32) {
    let bg = Color::rgb(45, 48, 56);
    let orange = Color::rgb(255, 149, 0);
    let white = Color::WHITE;

    draw_rect(fb, Rect::new(x + 2, y + 2, 20, 20), bg);
    crate::gui::primitives::draw_rect_outline(fb, Rect::new(x + 2, y + 2, 20, 20), Color::rgb(70, 75, 88), 1);

    // Top display bar
    draw_rect(fb, Rect::new(x + 5, y + 5, 14, 4), Color::rgb(20, 22, 26));

    // 4 key dots / symbols
    draw_rect(fb, Rect::new(x + 5, y + 11, 4, 3), white);
    draw_rect(fb, Rect::new(x + 11, y + 11, 4, 3), white);
    draw_rect(fb, Rect::new(x + 5, y + 16, 4, 3), white);
    draw_rect(fb, Rect::new(x + 11, y + 16, 8, 3), orange); // = key
}

/// Renders Retro Snake Arcade Icon (24x24)
pub fn draw_snake_icon(fb: &mut Framebuffer, x: i32, y: i32) {
    let bg = Color::rgb(14, 18, 22);
    let green = Color::rgb(80, 250, 123);
    let red = Color::rgb(255, 80, 80);

    draw_rect(fb, Rect::new(x + 2, y + 2, 20, 20), bg);
    crate::gui::primitives::draw_rect_outline(fb, Rect::new(x + 2, y + 2, 20, 20), Color::rgb(40, 50, 60), 1);

    // Snake body S-curve
    draw_rect(fb, Rect::new(x + 14, y + 6, 4, 4), green);  // head
    draw_rect(fb, Rect::new(x + 10, y + 6, 4, 4), green);
    draw_rect(fb, Rect::new(x + 6, y + 6, 4, 4), green);
    draw_rect(fb, Rect::new(x + 6, y + 10, 4, 4), green);
    draw_rect(fb, Rect::new(x + 6, y + 14, 4, 4), green);
    draw_rect(fb, Rect::new(x + 10, y + 14, 4, 4), green);

    // Food dot
    draw_rect(fb, Rect::new(x + 14, y + 14, 3, 3), red);
}

/// Renders Aegis Paint Canvas App Icon (24x24)
pub fn draw_paint_icon(fb: &mut Framebuffer, x: i32, y: i32) {
    let bg = Color::rgb(245, 240, 230); // cream easel palette
    let border = Color::rgb(180, 160, 140);

    // Palette disc
    crate::gui::primitives::draw_rounded_rect(fb, Rect::new(x + 2, y + 2, 20, 20), 6, bg);
    crate::gui::primitives::draw_rounded_rect_outline(fb, Rect::new(x + 2, y + 2, 20, 20), 6, border);

    // Pigment dots
    crate::gui::primitives::draw_circle(fb, x + 7, y + 7, 2, Color::rgb(240, 60, 60));   // Red
    crate::gui::primitives::draw_circle(fb, x + 13, y + 6, 2, Color::rgb(255, 180, 20)); // Yellow
    crate::gui::primitives::draw_circle(fb, x + 17, y + 11, 2, Color::rgb(40, 180, 80)); // Green
    crate::gui::primitives::draw_circle(fb, x + 15, y + 16, 2, Color::rgb(30, 130, 230)); // Blue

    // Palette thumb hole
    crate::gui::primitives::draw_circle(fb, x + 7, y + 15, 2, Color::rgb(210, 195, 175));
}

/// Renders Aegis Files Folder App Icon (24x24)
pub fn draw_files_icon(fb: &mut Framebuffer, x: i32, y: i32) {
    let back_tab = Color::rgb(25, 105, 195);
    let front_folder = Color::rgb(50, 145, 245);
    let outline = Color::rgb(20, 80, 160);
    let doc_white = Color::rgb(245, 248, 255);

    // 1. Back folder tab & backing
    crate::gui::primitives::draw_rounded_rect(fb, Rect::new(x + 2, y + 3, 10, 6), 2, back_tab);
    crate::gui::primitives::draw_rounded_rect(fb, Rect::new(x + 2, y + 6, 20, 15), 3, back_tab);

    // 2. White document sheet peeking out
    crate::gui::primitives::draw_rect(fb, Rect::new(x + 6, y + 4, 11, 8), doc_white);
    crate::gui::primitives::draw_rect(fb, Rect::new(x + 8, y + 6, 7, 1), Color::rgb(180, 190, 210));
    crate::gui::primitives::draw_rect(fb, Rect::new(x + 8, y + 8, 5, 1), Color::rgb(180, 190, 210));

    // 3. Front folder flap
    crate::gui::primitives::draw_rounded_rect(fb, Rect::new(x + 2, y + 9, 20, 12), 3, front_folder);
    crate::gui::primitives::draw_rounded_rect_outline(fb, Rect::new(x + 2, y + 9, 20, 12), 3, outline);

    // 4. Subtle top highlight
    crate::gui::primitives::draw_rect(fb, Rect::new(x + 4, y + 10, 16, 1), Color::rgb(110, 185, 255));
}

/// Renders mini 12x10 folder icon for file manager lists
pub fn draw_mini_folder(fb: &mut Framebuffer, x: i32, y: i32) {
    let tab = Color::rgb(35, 120, 215);
    let body = Color::rgb(55, 155, 255);
    draw_rect(fb, Rect::new(x, y + 1, 5, 3), tab);
    draw_rect(fb, Rect::new(x, y + 3, 12, 7), body);
    draw_rect(fb, Rect::new(x + 1, y + 4, 10, 1), Color::rgb(110, 185, 255));
}

/// Renders mini 10x12 document icon for file manager lists
pub fn draw_mini_doc(fb: &mut Framebuffer, x: i32, y: i32) {
    let paper = Color::rgb(220, 230, 245);
    let line = Color::rgb(140, 155, 175);
    draw_rect(fb, Rect::new(x + 1, y, 8, 12), paper);
    draw_rect(fb, Rect::new(x + 3, y + 3, 4, 1), line);
    draw_rect(fb, Rect::new(x + 3, y + 5, 4, 1), line);
    draw_rect(fb, Rect::new(x + 3, y + 7, 4, 1), line);
}

/// Renders mini 12x10 image canvas icon for PPM files
pub fn draw_mini_image(fb: &mut Framebuffer, x: i32, y: i32) {
    let frame = Color::rgb(180, 130, 220);
    let dot = Color::rgb(245, 180, 70);
    draw_rect(fb, Rect::new(x, y + 1, 12, 9), frame);
    draw_rect(fb, Rect::new(x + 2, y + 3, 3, 3), dot);
    draw_rect(fb, Rect::new(x + 3, y + 7, 7, 2), Color::rgb(90, 210, 140));
}

/// Renders System Settings / Preferences Gear Icon (24x24)
pub fn draw_settings_icon(fb: &mut Framebuffer, x: i32, y: i32) {
    let tooth_color = Color::rgb(150, 160, 172);
    let gear_body = Color::rgb(165, 175, 188);
    let outline_dark = Color::rgb(90, 95, 105);
    let hole_bg = Color::rgb(32, 35, 42);

    // 8 Radial Teeth
    // Cardinal
    draw_rect(fb, Rect::new(x + 10, y + 2, 4, 4), tooth_color); // Top
    draw_rect(fb, Rect::new(x + 10, y + 18, 4, 4), tooth_color); // Bottom
    draw_rect(fb, Rect::new(x + 2, y + 10, 4, 4), tooth_color); // Left
    draw_rect(fb, Rect::new(x + 18, y + 10, 4, 4), tooth_color); // Right
    // Diagonal
    draw_rect(fb, Rect::new(x + 4, y + 4, 4, 4), tooth_color); // Top-Left
    draw_rect(fb, Rect::new(x + 16, y + 4, 4, 4), tooth_color); // Top-Right
    draw_rect(fb, Rect::new(x + 4, y + 16, 4, 4), tooth_color); // Bottom-Left
    draw_rect(fb, Rect::new(x + 16, y + 16, 4, 4), tooth_color); // Bottom-Right

    // Main Gear Wheel Hub
    crate::gui::primitives::draw_circle(fb, x + 12, y + 12, 7, gear_body);
    crate::gui::primitives::draw_circle_outline(fb, x + 12, y + 12, 7, outline_dark);

    // Center Axle Hole
    crate::gui::primitives::draw_circle(fb, x + 12, y + 12, 3, hole_bg);
    crate::gui::primitives::draw_circle_outline(fb, x + 12, y + 12, 3, Color::rgb(110, 115, 125));
}

/// Renders Browser Globe Icon (24x24) — Azure blue globe with meridian arcs
pub fn draw_globe_icon(fb: &mut Framebuffer, x: i32, y: i32) {
    let globe_blue = Color::rgb(60, 140, 255);
    let meridian = Color::rgb(100, 180, 255);
    let outline = Color::rgb(40, 100, 200);

    // Globe circle
    crate::gui::primitives::draw_circle(fb, x + 12, y + 12, 10, globe_blue);
    crate::gui::primitives::draw_circle_outline(fb, x + 12, y + 12, 10, outline);

    // Horizontal equator line
    draw_line(fb, x + 3, y + 12, x + 21, y + 12, meridian);

    // Vertical prime meridian
    draw_line(fb, x + 12, y + 3, x + 12, y + 21, meridian);

    // Latitude lines (top and bottom)
    draw_line(fb, x + 5, y + 8, x + 19, y + 8, meridian);
    draw_line(fb, x + 5, y + 16, x + 19, y + 16, meridian);

    // Curved longitude arcs (approximated as offset vertical lines)
    draw_line(fb, x + 8, y + 4, x + 8, y + 20, Color::rgb(80, 160, 240));
    draw_line(fb, x + 16, y + 4, x + 16, y + 20, Color::rgb(80, 160, 240));
}

/// Renders Minesweeper Naval Contact Mine Icon (24x24)
pub fn draw_mine_icon(fb: &mut Framebuffer, x: i32, y: i32) {
    let mine_charcoal = Color::rgb(45, 48, 55);
    let spike_dark = Color::rgb(30, 32, 36);
    let highlight = Color::rgb(180, 190, 205);
    let red_cap = Color::rgb(255, 65, 65);

    let cx = x + 12;
    let cy = y + 12;

    // Contact spikes (cardinal and diagonal)
    draw_line(fb, cx - 9, cy, cx + 9, cy, spike_dark);
    draw_line(fb, cx, cy - 9, cx, cy + 9, spike_dark);
    draw_line(fb, cx - 7, cy - 7, cx + 7, cy + 7, spike_dark);
    draw_line(fb, cx - 7, cy + 7, cx + 7, cy - 7, spike_dark);

    // Spherical mine body
    crate::gui::primitives::draw_circle(fb, cx, cy, 7, mine_charcoal);
    crate::gui::primitives::draw_circle_outline(fb, cx, cy, 7, Color::rgb(20, 22, 26));

    // Red detonator cap on top
    crate::gui::primitives::draw_circle(fb, cx, cy - 6, 2, red_cap);

    // White/silver glint reflection on body
    crate::gui::primitives::draw_circle(fb, cx - 2, cy - 2, 2, highlight);
}

/// Handcrafted 24x24 Beamed Musical Eighth-Notes Icon for AegisSynth.
pub fn draw_music_note_icon(fb: &mut Framebuffer, x: i32, y: i32) {
    let cx = x + 12;
    let cy = y + 12;

    let magenta = Color::rgb(255, 70, 180);
    let cyan = Color::rgb(0, 230, 255);
    let white = Color::WHITE;

    // Dual Note Heads (tilted ovals)
    crate::gui::primitives::draw_circle(fb, cx - 6, cy + 4, 4, magenta);
    crate::gui::primitives::draw_circle(fb, cx + 4, cy + 2, 4, magenta);

    // Note Head White Reflection Dots
    crate::gui::primitives::draw_circle(fb, cx - 7, cy + 3, 1, white);
    crate::gui::primitives::draw_circle(fb, cx + 3, cy + 1, 1, white);

    // Stems rising from note heads
    draw_line(fb, cx - 3, cy + 4, cx - 3, cy - 6, cyan);
    draw_line(fb, cx - 2, cy + 4, cx - 2, cy - 6, cyan);

    draw_line(fb, cx + 7, cy + 2, cx + 7, cy - 8, cyan);
    draw_line(fb, cx + 8, cy + 2, cx + 8, cy - 8, cyan);

    // Connecting Beamed Bar on Top
    for b in 0..3 {
        draw_line(fb, cx - 3, cy - 6 + b, cx + 8, cy - 8 + b, cyan);
    }
}

/// Handcrafted 24x24 Speech Bubble Icon for AegisChat.
pub fn draw_chat_icon(fb: &mut Framebuffer, x: i32, y: i32) {
    let cx = x + 12;
    let cy = y + 10;

    let bubble_green = Color::rgb(36, 196, 96);
    let bubble_outline = Color::rgb(24, 150, 72);
    let white = Color::WHITE;

    // Main Bubble Body (20x13 rounded rect)
    let body_rect = Rect::new(x + 2, y + 3, 20, 13);
    crate::gui::primitives::draw_rounded_rect(fb, body_rect, 4, bubble_green);
    crate::gui::primitives::draw_rounded_rect_outline(fb, body_rect, 4, bubble_outline);

    // Bubble Tail Pointer at Bottom-Left
    for i in 0..4 {
        draw_line(fb, x + 5 + i, y + 15, x + 3, y + 19, bubble_green);
    }
    draw_line(fb, x + 3, y + 19, x + 9, y + 15, bubble_outline);

    // 3 Interior Conversation Dots
    crate::gui::primitives::draw_circle(fb, cx - 5, cy, 1, white);
    crate::gui::primitives::draw_circle(fb, cx, cy, 1, white);
    crate::gui::primitives::draw_circle(fb, cx + 5, cy, 1, white);
}


