//! 8x16 bitmap text rendering into the window surface.
//!
//! The glyph table is shared with the kernel via `shared/font8x16.inc` rather
//! than copied, so the Ring 3 terminal and the Ring 0 desktop cannot drift apart
//! on what a character looks like.

pub const FONT_WIDTH: usize = 8;
pub const FONT_HEIGHT: usize = 16;

pub static FONT_8X16: [u8; 96 * 16] = include!("../../shared/font8x16.inc");

/// Returns the 16 rows for `c`, or the rows for `?` if it is outside ASCII
/// 32..126. Each row is a bitmask, most significant bit leftmost.
pub fn glyph(c: u8) -> &'static [u8] {
    let index = if (32..=126).contains(&c) {
        (c - 32) as usize
    } else {
        ('?' as u8 - 32) as usize
    };
    let offset = index * FONT_HEIGHT;
    &FONT_8X16[offset..offset + FONT_HEIGHT]
}
