//! Desktop Wallpaper Engine & Binary PPM Image Parser
//!
//! Supports built-in 6-theme gradient rendering and custom VFS PPM image wallpapers
//! with on-the-fly scanline nearest-neighbor row scaling.

use alloc::vec::Vec;

use crate::drivers::framebuffer::Framebuffer;
use crate::gui::menubar::WallpaperTheme;
use crate::gui::primitives::{draw_gradient_v, Color, Rect};

// ============================================================================
// PPM Image Structure & Binary P6 Decoder
// ============================================================================

pub struct PpmImage {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<Color>,
}

impl PpmImage {
    pub fn new(width: u32, height: u32, pixels: Vec<Color>) -> Self {
        Self { width, height, pixels }
    }
}

/// Parses a binary P6 format PPM image byte slice.
pub fn parse_ppm_p6(data: &[u8]) -> Result<PpmImage, &'static str> {
    if data.len() < 10 {
        return Err("PPM data too short");
    }

    if data[0] != b'P' || data[1] != b'6' {
        return Err("Invalid PPM magic header (expected 'P6')");
    }

    let idx = 2;

    // Helper: skip whitespace and comments starting with '#'
    let skip_whitespace_and_comments = |data: &[u8], mut i: usize| -> usize {
        while i < data.len() {
            let b = data[i];
            if b == b'#' {
                // Comment line; skip to newline
                while i < data.len() && data[i] != b'\n' {
                    i += 1;
                }
                if i < data.len() {
                    i += 1;
                }
            } else if b == b' ' || b == b'\t' || b == b'\r' || b == b'\n' {
                i += 1;
            } else {
                break;
            }
        }
        i
    };

    // Helper: read next positive integer
    let read_int = |data: &[u8], mut i: usize| -> Result<(u32, usize), &'static str> {
        i = skip_whitespace_and_comments(data, i);
        if i >= data.len() {
            return Err("Unexpected EOF while parsing PPM header");
        }
        let mut val: u32 = 0;
        let start = i;
        while i < data.len() && data[i] >= b'0' && data[i] <= b'9' {
            val = val
                .checked_mul(10)
                .and_then(|v| v.checked_add((data[i] - b'0') as u32))
                .ok_or("Integer overflow in PPM header")?;
            i += 1;
        }
        if i == start {
            return Err("Expected integer in PPM header");
        }
        Ok((val, i))
    };

    let (width, next_idx) = read_int(data, idx)?;
    let (height, next_idx2) = read_int(data, next_idx)?;
    let (maxval, next_idx3) = read_int(data, next_idx2)?;

    if width == 0 || height == 0 {
        return Err("Invalid PPM dimensions (0 width or height)");
    }
    if maxval == 0 || maxval > 255 {
        return Err("Unsupported PPM maxval (only 1..255 supported)");
    }

    // Following maxval, exactly one whitespace character delimits pixel data
    let mut pixel_start = next_idx3;
    if pixel_start < data.len() && (data[pixel_start] == b' ' || data[pixel_start] == b'\n' || data[pixel_start] == b'\r') {
        pixel_start += 1;
    }

    let expected_bytes = (width as usize)
        .checked_mul(height as usize)
        .and_then(|n| n.checked_mul(3))
        .ok_or("PPM dimensions overflow")?;

    let remaining_bytes = data.len().saturating_sub(pixel_start);
    if remaining_bytes < expected_bytes {
        return Err("Truncated PPM pixel data");
    }

    let mut pixels = Vec::with_capacity((width * height) as usize);
    let mut p_idx = pixel_start;
    for _ in 0..(width * height) {
        let r = data[p_idx];
        let g = data[p_idx + 1];
        let b = data[p_idx + 2];
        p_idx += 3;
        pixels.push(Color::rgb(r, g, b));
    }

    Ok(PpmImage::new(width, height, pixels))
}

// ============================================================================
// Desktop Background State & Rendering
// ============================================================================

pub enum DesktopBackground {
    Theme(WallpaperTheme),
    Custom(PpmImage),
}

/// Renders desktop background (either vertical gradient or scaled custom PPM).
pub fn render_background(
    fb: &mut Framebuffer,
    bg: &DesktopBackground,
    screen_w: usize,
    screen_h: usize,
) {
    let bg_rect = Rect::new(0, 0, screen_w as u32, screen_h as u32);

    match bg {
        DesktopBackground::Theme(theme) => {
            let (top_col, bot_col) = match theme {
                WallpaperTheme::DeepOcean => (Color::rgb(20, 45, 80), Color::rgb(10, 18, 35)),
                WallpaperTheme::CyberTwilight => (Color::rgb(60, 20, 75), Color::rgb(18, 12, 35)),
                WallpaperTheme::EmeraldForest => (Color::rgb(18, 55, 40), Color::rgb(10, 25, 18)),
                WallpaperTheme::MidnightSlate => (Color::rgb(35, 40, 50), Color::rgb(18, 20, 25)),
                WallpaperTheme::SunsetHorizon => (Color::rgb(130, 45, 60), Color::rgb(35, 15, 45)),
                WallpaperTheme::SolarFlare => (Color::rgb(125, 65, 20), Color::rgb(35, 20, 15)),
            };
            draw_gradient_v(fb, bg_rect, top_col, bot_col);
        }
        DesktopBackground::Custom(ppm) => {
            if ppm.width == 0 || ppm.height == 0 || ppm.pixels.is_empty() {
                // Fallback to Deep Ocean
                draw_gradient_v(fb, bg_rect, Color::rgb(20, 45, 80), Color::rgb(10, 18, 35));
                return;
            }

            let src_w = ppm.width as usize;
            let src_h = ppm.height as usize;
            let dst_w = screen_w;
            let dst_h = screen_h;

            // Direct scanline blit with nearest-neighbor scaling
            for y in 0..dst_h {
                let src_y = (y * src_h) / dst_h;
                let row_start = src_y * src_w;
                let dst_row = y * dst_w;

                for x in 0..dst_w {
                    let src_x = (x * src_w) / dst_w;
                    let color = ppm.pixels[row_start + src_x];
                    fb.backbuffer[dst_row + x] = color.to_u32();
                }
            }
            fb.mark_dirty(Rect::new(0, 0, dst_w as u32, dst_h as u32));
        }
    }
}
