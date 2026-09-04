//! Drawing into the window surface the kernel maps for this process.
//!
//! Every pixel this program puts on screen goes through here. There is no
//! syscall that draws: the kernel hands over a block of memory and blits it,
//! which means a bug in this file can corrupt this window and nothing else.

use crate::font::{glyph, FONT_HEIGHT, FONT_WIDTH};
use crate::sys;

pub struct Surface {
    base: *mut u32,
    pub width: usize,
    pub height: usize,
}

impl Surface {
    /// Asks the kernel to map this process's surface.
    pub fn map() -> Option<Self> {
        let (base, width, height) = sys::surface_map()?;
        Some(Self { base, width, height })
    }

    #[inline]
    pub fn put(&mut self, x: usize, y: usize, argb: u32) {
        if x >= self.width || y >= self.height {
            return;
        }
        unsafe { self.base.add(y * self.width + x).write_volatile(argb) };
    }

    pub fn fill(&mut self, argb: u32) {
        for i in 0..self.width * self.height {
            unsafe { self.base.add(i).write_volatile(argb) };
        }
    }

    pub fn fill_rect(&mut self, x: usize, y: usize, w: usize, h: usize, argb: u32) {
        for row in y..(y + h).min(self.height) {
            for col in x..(x + w).min(self.width) {
                self.put(col, row, argb);
            }
        }
    }

    /// Draws one character. `bg` of `None` leaves the background untouched.
    pub fn draw_char(&mut self, x: usize, y: usize, c: u8, fg: u32, bg: Option<u32>) {
        let rows = glyph(c);
        for (dy, row) in rows.iter().enumerate() {
            for dx in 0..FONT_WIDTH {
                let lit = row & (0x80 >> dx) != 0;
                if lit {
                    self.put(x + dx, y + dy, fg);
                } else if let Some(bg) = bg {
                    self.put(x + dx, y + dy, bg);
                }
            }
        }
    }

    pub fn draw_text(&mut self, x: usize, y: usize, text: &[u8], fg: u32, bg: Option<u32>) {
        for (i, &c) in text.iter().enumerate() {
            self.draw_char(x + i * FONT_WIDTH, y, c, fg, bg);
        }
    }

    pub fn cols(&self) -> usize {
        self.width / FONT_WIDTH
    }

    pub fn rows(&self) -> usize {
        self.height / FONT_HEIGHT
    }
}
