//! AegisOS E2E Test Harness: Double-Buffered Framebuffer & Compositor Simulator
//!
//! Models 1024x768x32 linear ARGB double-buffering, dirty rectangle scanline blitting,
//! 2D vector primitives, alpha blending, and embedded 8x16 font rendering.

use super::types::*;

pub const SCREEN_WIDTH: usize = 1024;
pub const SCREEN_HEIGHT: usize = 768;
pub const BYTES_PER_PIXEL: usize = 4;
pub const PITCH_BYTES: usize = SCREEN_WIDTH * BYTES_PER_PIXEL; // 4096 bytes

// Minimal embedded 8x16 font bitmap (ASCII 0..127)
pub const FONT_WIDTH: usize = 8;
pub const FONT_HEIGHT: usize = 16;

pub struct FramebufferSimulator {
    pub width: usize,
    pub height: usize,
    pub frontbuffer: Vec<u32>,
    pub backbuffer: Vec<u32>,
    pub dirty_rect: Option<Rect>,
    pub total_swaps: usize,
    pub total_pixels_blitted: usize,
}

impl FramebufferSimulator {
    pub fn new(width: usize, height: usize) -> Self {
        let size = width * height;
        Self {
            width,
            height,
            frontbuffer: vec![0xFF000000; size], // Initial black
            backbuffer: vec![0xFF000000; size],
            dirty_rect: None,
            total_swaps: 0,
            total_pixels_blitted: 0,
        }
    }

    pub fn default_1024x768() -> Self {
        Self::new(SCREEN_WIDTH, SCREEN_HEIGHT)
    }

    pub fn mark_dirty(&mut self, rect: Rect) {
        match self.dirty_rect {
            None => self.dirty_rect = Some(rect),
            Some(existing) => {
                let min_x = existing.x.min(rect.x);
                let min_y = existing.y.min(rect.y);
                let max_x = (existing.x + existing.width as i32).max(rect.x + rect.width as i32);
                let max_y = (existing.y + existing.height as i32).max(rect.y + rect.height as i32);
                self.dirty_rect = Some(Rect {
                    x: min_x,
                    y: min_y,
                    width: (max_x - min_x).max(0) as usize,
                    height: (max_y - min_y).max(0) as usize,
                });
            }
        }
    }

    pub fn swap_buffers(&mut self) -> usize {
        self.total_swaps += 1;
        if let Some(rect) = self.dirty_rect.take() {
            let start_x = rect.x.clamp(0, self.width as i32) as usize;
            let end_x = (rect.x + rect.width as i32).clamp(0, self.width as i32) as usize;
            let start_y = rect.y.clamp(0, self.height as i32) as usize;
            let end_y = (rect.y + rect.height as i32).clamp(0, self.height as i32) as usize;

            let copy_width = end_x.saturating_sub(start_x);
            if copy_width == 0 || start_y >= end_y {
                return 0;
            }

            let mut pixels_copied = 0;
            for y in start_y..end_y {
                let row_start = y * self.width + start_x;
                let row_end = row_start + copy_width;
                self.frontbuffer[row_start..row_end].copy_from_slice(&self.backbuffer[row_start..row_end]);
                pixels_copied += copy_width;
            }
            self.total_pixels_blitted += pixels_copied;
            pixels_copied
        } else {
            // Full swap if no dirty rect specified
            self.frontbuffer.copy_from_slice(&self.backbuffer);
            let pixels = self.width * self.height;
            self.total_pixels_blitted += pixels;
            pixels
        }
    }

    pub fn draw_pixel(&mut self, x: i32, y: i32, color: Color) {
        if x < 0 || y < 0 || (x as usize) >= self.width || (y as usize) >= self.height {
            return; // Bounds clipped
        }
        let idx = (y as usize) * self.width + (x as usize);
        if color.a == 255 {
            self.backbuffer[idx] = color.to_u32();
        } else if color.a > 0 {
            let current_raw = self.backbuffer[idx];
            let current = Color {
                a: ((current_raw >> 24) & 0xFF) as u8,
                r: ((current_raw >> 16) & 0xFF) as u8,
                g: ((current_raw >> 8) & 0xFF) as u8,
                b: (current_raw & 0xFF) as u8,
            };
            let blended = Color::blend(color, current);
            self.backbuffer[idx] = blended.to_u32();
        }
        self.mark_dirty(Rect::new(x, y, 1, 1));
    }

    pub fn get_pixel(&self, x: i32, y: i32) -> Option<Color> {
        if x < 0 || y < 0 || (x as usize) >= self.width || (y as usize) >= self.height {
            return None;
        }
        let idx = (y as usize) * self.width + (x as usize);
        let raw = self.backbuffer[idx];
        Some(Color {
            a: ((raw >> 24) & 0xFF) as u8,
            r: ((raw >> 16) & 0xFF) as u8,
            g: ((raw >> 8) & 0xFF) as u8,
            b: (raw & 0xFF) as u8,
        })
    }

    pub fn draw_rect(&mut self, rect: Rect, color: Color) {
        self.mark_dirty(rect);
        let start_x = rect.x.clamp(0, self.width as i32) as usize;
        let end_x = (rect.x + rect.width as i32).clamp(0, self.width as i32) as usize;
        let start_y = rect.y.clamp(0, self.height as i32) as usize;
        let end_y = (rect.y + rect.height as i32).clamp(0, self.height as i32) as usize;

        if start_x >= end_x || start_y >= end_y {
            return;
        }

        for y in start_y..end_y {
            for x in start_x..end_x {
                let idx = y * self.width + x;
                if color.a == 255 {
                    self.backbuffer[idx] = color.to_u32();
                } else if color.a > 0 {
                    let cur_raw = self.backbuffer[idx];
                    let cur = Color {
                        a: ((cur_raw >> 24) & 0xFF) as u8,
                        r: ((cur_raw >> 16) & 0xFF) as u8,
                        g: ((cur_raw >> 8) & 0xFF) as u8,
                        b: (cur_raw & 0xFF) as u8,
                    };
                    self.backbuffer[idx] = Color::blend(color, cur).to_u32();
                }
            }
        }
        self.mark_dirty(rect);
    }

    pub fn draw_rounded_rect(&mut self, rect: Rect, radius: usize, color: Color) {
        // Draw central cross rects and 4 rounded corners
        let r = radius as i32;
        let w = rect.width as i32;
        let h = rect.height as i32;

        for dy in 0..h {
            for dx in 0..w {
                let px = rect.x + dx;
                let py = rect.y + dy;

                let is_in_corner = if dx < r && dy < r {
                    let cx = r;
                    let cy = r;
                    (dx - cx) * (dx - cx) + (dy - cy) * (dy - cy) > r * r
                } else if dx >= w - r && dy < r {
                    let cx = w - r - 1;
                    let cy = r;
                    (dx - cx) * (dx - cx) + (dy - cy) * (dy - cy) > r * r
                } else if dx < r && dy >= h - r {
                    let cx = r;
                    let cy = h - r - 1;
                    (dx - cx) * (dx - cx) + (dy - cy) * (dy - cy) > r * r
                } else if dx >= w - r && dy >= h - r {
                    let cx = w - r - 1;
                    let cy = h - r - 1;
                    (dx - cx) * (dx - cx) + (dy - cy) * (dy - cy) > r * r
                } else {
                    false
                };

                if !is_in_corner {
                    self.draw_pixel(px, py, color);
                }
            }
        }
        self.mark_dirty(rect);
    }

    pub fn draw_circle(&mut self, cx: i32, cy: i32, radius: i32, color: Color) {
        for dy in -radius..=radius {
            for dx in -radius..=radius {
                if dx * dx + dy * dy <= radius * radius {
                    self.draw_pixel(cx + dx, cy + dy, color);
                }
            }
        }
        self.mark_dirty(Rect::new(
            cx - radius,
            cy - radius,
            (radius * 2 + 1) as usize,
            (radius * 2 + 1) as usize,
        ));
    }

    pub fn draw_gradient_v(&mut self, rect: Rect, top: Color, bottom: Color) {
        let h = rect.height.max(1);
        for dy in 0..rect.height {
            let t = dy as f32 / h as f32;
            let r = ((1.0 - t) * top.r as f32 + t * bottom.r as f32) as u8;
            let g = ((1.0 - t) * top.g as f32 + t * bottom.g as f32) as u8;
            let b = ((1.0 - t) * top.b as f32 + t * bottom.b as f32) as u8;
            let a = ((1.0 - t) * top.a as f32 + t * bottom.a as f32) as u8;
            let row_color = Color { r, g, b, a };

            for dx in 0..rect.width {
                self.draw_pixel(rect.x + dx as i32, rect.y + dy as i32, row_color);
            }
        }
        self.mark_dirty(rect);
    }

    pub fn draw_char(&mut self, x: i32, y: i32, c: u8, fg: Color, bg: Option<Color>) {
        // Simple synthetic 8x16 font rendering for test simulator
        if let Some(bg_color) = bg {
            self.draw_rect(Rect::new(x, y, FONT_WIDTH, FONT_HEIGHT), bg_color);
        }
        // Render simple cross/glyph pattern
        if c >= 32 && c <= 126 {
            // Draw character dot grid based on ASCII hash
            let char_code = c as u32;
            for row in 0..FONT_HEIGHT {
                let row_bits = ((char_code * 37 + row as u32 * 17) & 0xFF) as u8;
                for col in 0..FONT_WIDTH {
                    if (row_bits & (1 << (7 - col))) != 0 {
                        self.draw_pixel(x + col as i32, y + row as i32, fg);
                    }
                }
            }
        }
    }

    pub fn draw_string(&mut self, mut x: i32, y: i32, text: &str, fg: Color, bg: Option<Color>) {
        for b in text.bytes() {
            if b == b'\n' {
                break;
            }
            self.draw_char(x, y, b, fg, bg);
            x += FONT_WIDTH as i32;
        }
    }
}
