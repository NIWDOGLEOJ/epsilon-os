//! Linear RGB Double-Buffered Framebuffer Driver for AegisOS
//!
//! Parses Limine Framebuffer structures, maintains an off-screen 32-bit ARGB
//! backbuffer in system RAM, tracks dirty regions, and performs 64-bit scanline
//! blits for 60 FPS tear-free rendering.

use alloc::vec;
use alloc::vec::Vec;
use spin::Mutex;

use crate::gui::primitives::{Color, Rect};

// ============================================================================
// Framebuffer Driver Structure
// ============================================================================

pub struct Framebuffer {
    pub frontbuffer: *mut u32,
    pub backbuffer: Vec<u32>,
    pub width: usize,
    pub height: usize,
    pub pitch_pixels: usize,
    pub dirty_rect: Option<Rect>,
    /// Active scissor rectangle. `None` means the whole screen is writable.
    /// Honoured by `draw_pixel`, which every drawing primitive routes through.
    clip_rect: Option<Rect>,
}

unsafe impl Send for Framebuffer {}
unsafe impl Sync for Framebuffer {}

impl Framebuffer {
    /// Creates a new double-buffered framebuffer instance.
    pub fn new(frontbuffer: *mut u32, width: usize, height: usize, pitch_pixels: usize) -> Self {
        let pixel_count = width * height;
        let mut backbuffer = vec![0u32; pixel_count];
        // Initialize backbuffer with dark desktop background
        let bg_color = Color::DESKTOP_BG_TOP.to_u32();
        backbuffer.fill(bg_color);

        Self {
            frontbuffer,
            backbuffer,
            width,
            height,
            pitch_pixels,
            dirty_rect: Some(Rect::new(0, 0, width as u32, height as u32)),
            clip_rect: None,
        }
    }

    /// Restricts subsequent drawing to `rect`, returning the previous clip so the
    /// caller can restore it. Used by the compositor to keep an application's
    /// content inside its own window frame.
    pub fn set_clip(&mut self, rect: Rect) -> Option<Rect> {
        let previous = self.clip_rect;
        // Nested clips intersect rather than replace, so a child can never draw
        // outside its parent. A pair that does not overlap collapses to an empty
        // rect, which rejects every pixel -- `None` would wrongly mean "no clip".
        self.clip_rect = Some(match previous {
            Some(outer) => outer.intersection(&rect).unwrap_or(Rect::new(0, 0, 0, 0)),
            None => rect,
        });
        previous
    }

    /// Restores a clip previously returned by `set_clip`.
    pub fn restore_clip(&mut self, previous: Option<Rect>) {
        self.clip_rect = previous;
    }

    #[inline(always)]
    pub fn width(&self) -> usize {
        self.width
    }

    #[inline(always)]
    pub fn height(&self) -> usize {
        self.height
    }

    /// Writes a single pixel with alpha blending and bounds checking.
    #[inline]
    pub fn draw_pixel(&mut self, x: i32, y: i32, color: Color) {
        if x < 0 || y < 0 || (x as usize) >= self.width || (y as usize) >= self.height {
            return;
        }

        if let Some(clip) = self.clip_rect {
            if !clip.contains(x, y) {
                return;
            }
        }

        let idx = (y as usize) * self.width + (x as usize);
        let final_color = if color.a == 255 {
            color
        } else if color.a == 0 {
            return;
        } else {
            let dst_val = self.backbuffer[idx];
            let dst_color = Color::from_u32(dst_val);
            Color::blend(color, dst_color)
        };

        self.backbuffer[idx] = final_color.to_u32();
        self.mark_dirty_pixel(x, y);
    }

    /// Fills the horizontal span `[x0, x1)` on row `y` in one pass.
    ///
    /// Bounds checking, clipping and dirty tracking happen once for the whole span
    /// rather than once per pixel, and a fully opaque span becomes a `slice::fill`.
    /// Every large-area primitive routes through here; going pixel-by-pixel through
    /// `draw_pixel` cost ~170 cycles per pixel, which is what held the compositor
    /// to single-digit frame rates.
    pub fn fill_span(&mut self, x0: i32, x1: i32, y: i32, color: Color) {
        if color.a == 0 || y < 0 || (y as usize) >= self.height {
            return;
        }

        let mut start = x0.max(0);
        let mut end = x1.min(self.width as i32);

        if let Some(clip) = self.clip_rect {
            if y < clip.y || y >= clip.bottom() {
                return;
            }
            start = start.max(clip.x);
            end = end.min(clip.right());
        }

        if start >= end {
            return;
        }

        let row_base = (y as usize) * self.width;
        let slice = &mut self.backbuffer[row_base + start as usize..row_base + end as usize];

        if color.a == 255 {
            slice.fill(color.to_u32());
        } else {
            for pixel in slice.iter_mut() {
                *pixel = Color::blend(color, Color::from_u32(*pixel)).to_u32();
            }
        }

        self.mark_dirty(Rect::new(start, y, (end - start) as u32, 1));
    }

    /// Copies a row of opaque ARGB pixels straight into the backbuffer.
    ///
    /// The fast path for compositing a Ring 3 window surface. Going through
    /// `draw_pixel` costs a bounds check, a clip test and an alpha branch per
    /// pixel; at 640x384 per window per frame that dominated the frame time and
    /// dropped the compositor to about one frame a second with three such
    /// windows open. Clipping once per row and copying the span instead makes
    /// the same work a slice copy.
    ///
    /// Source pixels are treated as opaque: a process owns every pixel of its
    /// own surface, so there is nothing beneath to blend with.
    pub fn blit_row(&mut self, x: i32, y: i32, pixels: &[u32]) {
        if y < 0 || (y as usize) >= self.height || pixels.is_empty() {
            return;
        }

        // Clip horizontally against the framebuffer and the active clip rect,
        // and reject the row entirely if it falls outside the clip vertically.
        let mut start = x;
        let mut end = x + pixels.len() as i32;
        if let Some(clip) = self.clip_rect {
            if y < clip.y || y >= clip.y + clip.height as i32 {
                return;
            }
            start = start.max(clip.x);
            end = end.min(clip.x + clip.width as i32);
        }
        start = start.max(0);
        end = end.min(self.width as i32);
        if start >= end {
            return;
        }

        let src_offset = (start - x) as usize;
        let count = (end - start) as usize;
        let row_base = (y as usize) * self.width + start as usize;
        self.backbuffer[row_base..row_base + count]
            .copy_from_slice(&pixels[src_offset..src_offset + count]);
    }

    /// Reads a pixel value from the backbuffer.
    #[inline]
    pub fn get_pixel(&self, x: i32, y: i32) -> Option<Color> {
        if x < 0 || y < 0 || (x as usize) >= self.width || (y as usize) >= self.height {
            return None;
        }
        let idx = (y as usize) * self.width + (x as usize);
        Some(Color::from_u32(self.backbuffer[idx]))
    }

    /// Fills the entire backbuffer with a solid color.
    pub fn clear(&mut self, color: Color) {
        let val = color.to_u32();
        self.backbuffer.fill(val);
        self.dirty_rect = Some(Rect::new(0, 0, self.width as u32, self.height as u32));
    }

    /// Expands the dirty bounding box to include the specified rectangle.
    pub fn mark_dirty(&mut self, rect: Rect) {
        let clamped_x = rect.x.clamp(0, self.width as i32);
        let clamped_y = rect.y.clamp(0, self.height as i32);
        let clamped_w = (rect.right().clamp(0, self.width as i32) - clamped_x).max(0) as u32;
        let clamped_h = (rect.bottom().clamp(0, self.height as i32) - clamped_y).max(0) as u32;

        if clamped_w == 0 || clamped_h == 0 {
            return;
        }

        let clamped = Rect::new(clamped_x, clamped_y, clamped_w, clamped_h);
        self.dirty_rect = match self.dirty_rect {
            Some(existing) => Some(existing.union(&clamped)),
            None => Some(clamped),
        };
    }

    #[inline]
    fn mark_dirty_pixel(&mut self, x: i32, y: i32) {
        let p_rect = Rect::new(x, y, 1, 1);
        self.dirty_rect = match self.dirty_rect {
            Some(existing) => Some(existing.union(&p_rect)),
            None => Some(p_rect),
        };
    }

    /// Flushes dirty rectangle scanlines from backbuffer in RAM to frontbuffer VRAM.
    ///
    /// Returns the number of pixels copied.
    pub fn swap_buffers(&mut self) -> usize {
        if self.frontbuffer.is_null() {
            return 0;
        }

        let rect = match self.dirty_rect.take() {
            Some(r) => r,
            None => return 0,
        };

        let start_x = rect.x.clamp(0, self.width as i32) as usize;
        let end_x = rect.right().clamp(0, self.width as i32) as usize;
        let start_y = rect.y.clamp(0, self.height as i32) as usize;
        let end_y = rect.bottom().clamp(0, self.height as i32) as usize;
        let copy_width = end_x.saturating_sub(start_x);

        if copy_width == 0 || start_y >= end_y {
            return 0;
        }

        for y in start_y..end_y {
            let src_offset = y * self.width + start_x;
            let dst_ptr = unsafe { self.frontbuffer.add(y * self.pitch_pixels + start_x) };
            unsafe {
                core::ptr::copy_nonoverlapping(
                    self.backbuffer[src_offset..].as_ptr(),
                    dst_ptr,
                    copy_width,
                );
            }
        }

        copy_width * (end_y - start_y)
    }
}

// ============================================================================
// Global Framebuffer Driver Singleton & Helpers
// ============================================================================

pub static FRAMEBUFFER: Mutex<Option<Framebuffer>> = Mutex::new(None);

/// Initializes the global framebuffer driver from a Limine framebuffer handle.
pub fn init_from_limine(fb: &limine::framebuffer::Framebuffer) {
    let frontbuffer = fb.addr() as *mut u32;
    let width = fb.width() as usize;
    let height = fb.height() as usize;
    let pitch_pixels = (fb.pitch() as usize) / 4;

    let driver = Framebuffer::new(frontbuffer, width, height, pitch_pixels);
    *FRAMEBUFFER.lock() = Some(driver);
}

/// Executes a closure with mutable access to the global framebuffer.
pub fn with_framebuffer<F, R>(f: F) -> Option<R>
where
    F: FnOnce(&mut Framebuffer) -> R,
{
    FRAMEBUFFER.lock().as_mut().map(f)
}

/// Swaps backbuffer to VRAM frontbuffer.
pub fn swap_buffers() -> usize {
    if let Some(ref mut fb) = *FRAMEBUFFER.lock() {
        fb.swap_buffers()
    } else {
        0
    }
}

/// Returns screen dimensions (width, height) in pixels.
pub fn get_dimensions() -> (usize, usize) {
    if let Some(ref fb) = *FRAMEBUFFER.lock() {
        (fb.width, fb.height)
    } else {
        (1024, 768)
    }
}

/// Clears screen to given color.
pub fn clear_screen(color: Color) {
    if let Some(ref mut fb) = *FRAMEBUFFER.lock() {
        fb.clear(color);
    }
}
