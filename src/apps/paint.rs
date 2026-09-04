//! Aegis Paint Interactive Graphical Canvas Application for AegisOS
//!
//! Features 436x220 pixel canvas, Bresenham line interpolation for gap-free
//! drawing, 12-color swatch palette, brush thickness toggles (1px, 2px, 4px),
//! eraser mode, canvas clearing, and PPM image export to the RAM disk VFS.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::drivers::framebuffer::Framebuffer;
use crate::gui::font::draw_string;
use crate::gui::primitives::{
    draw_circle_outline, draw_rect, draw_rect_outline, draw_rounded_rect,
    draw_rounded_rect_outline, Color, Rect,
};
use crate::gui::window::Window;

pub const CANVAS_WIDTH: usize = 436;
pub const CANVAS_HEIGHT: usize = 220;

pub const PALETTE: [Color; 12] = [
    Color::rgb(0, 0, 0),       // Black
    Color::rgb(255, 255, 255), // White
    Color::rgb(110, 120, 135), // Slate Gray
    Color::rgb(235, 55, 55),   // Crimson Red
    Color::rgb(255, 130, 40),  // Coral Orange
    Color::rgb(255, 205, 30),  // Amber Yellow
    Color::rgb(45, 190, 85),   // Emerald Green
    Color::rgb(90, 230, 160),  // Mint Green
    Color::rgb(45, 190, 245),  // Aqua / Cyan
    Color::rgb(35, 130, 235),  // Royal Blue
    Color::rgb(160, 70, 235),  // Violet
    Color::rgb(245, 80, 165),  // Hot Pink
];

pub struct PaintApp {
    pub canvas: Vec<Color>,
    pub selected_color_idx: usize,
    pub brush_size: u32,
    pub is_eraser: bool,
    pub last_pt: Option<(i32, i32)>,
    pub is_drawing: bool,
    pub status_message: Option<String>,
}

impl PaintApp {
    pub fn new() -> Self {
        let mut canvas = Vec::with_capacity(CANVAS_WIDTH * CANVAS_HEIGHT);
        canvas.resize(CANVAS_WIDTH * CANVAS_HEIGHT, Color::WHITE);

        Self {
            canvas,
            selected_color_idx: 3, // Default to Crimson Red
            brush_size: 2,         // Default 2px brush
            is_eraser: false,
            last_pt: None,
            is_drawing: false,
            status_message: Some("Ready — Click or drag to draw".to_string()),
        }
    }

    /// Clears the canvas buffer with solid white.
    pub fn clear_canvas(&mut self) {
        for pixel in self.canvas.iter_mut() {
            *pixel = Color::WHITE;
        }
        self.status_message = Some("Canvas Cleared".to_string());
    }

    /// Returns the currently active drawing color.
    pub fn active_color(&self) -> Color {
        if self.is_eraser {
            Color::WHITE
        } else {
            PALETTE[self.selected_color_idx.min(PALETTE.len() - 1)]
        }
    }

    /// Draws a solid brush stamp at canvas coordinates `(cx, cy)`.
    fn draw_brush_stamp(&mut self, cx: i32, cy: i32, color: Color) {
        let r = (self.brush_size as i32) / 2;

        if self.brush_size <= 1 {
            if cx >= 0 && cx < CANVAS_WIDTH as i32 && cy >= 0 && cy < CANVAS_HEIGHT as i32 {
                let idx = (cy as usize * CANVAS_WIDTH) + cx as usize;
                self.canvas[idx] = color;
            }
        } else {
            for dy in -r..=r {
                for dx in -r..=r {
                    let px = cx + dx;
                    let py = cy + dy;
                    if px >= 0 && px < CANVAS_WIDTH as i32 && py >= 0 && py < CANVAS_HEIGHT as i32 {
                        let idx = (py as usize * CANVAS_WIDTH) + px as usize;
                        self.canvas[idx] = color;
                    }
                }
            }
        }
    }

    /// Bresenham's line algorithm rendering continuous brush strokes between points.
    pub fn stroke_line(&mut self, x0: i32, y0: i32, x1: i32, y1: i32) {
        let color = self.active_color();
        let dx = (x1 - x0).abs();
        let dy = -(y1 - y0).abs();
        let sx = if x0 < x1 { 1 } else { -1 };
        let sy = if y0 < y1 { 1 } else { -1 };
        let mut err = dx + dy;

        let mut curr_x = x0;
        let mut curr_y = y0;

        loop {
            self.draw_brush_stamp(curr_x, curr_y, color);
            if curr_x == x1 && curr_y == y1 {
                break;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                curr_x += sx;
            }
            if e2 <= dx {
                err += dx;
                curr_y += sy;
            }
        }
    }

    /// Serializes canvas into a standard binary PPM image file in the VFS.
    pub fn save_to_vfs(&mut self) -> Result<(), &'static str> {
        let mut ppm_bytes = Vec::new();

        // PPM Header: P6\n<width> <height>\n255\n
        let header = format!("P6\n{} {}\n255\n", CANVAS_WIDTH, CANVAS_HEIGHT);
        ppm_bytes.extend_from_slice(header.as_bytes());

        // Pixel data: R, G, B bytes
        for pixel in &self.canvas {
            ppm_bytes.push(pixel.r);
            ppm_bytes.push(pixel.g);
            ppm_bytes.push(pixel.b);
        }

        let save_path = "/user/drawing.ppm";
        crate::fs::write_file(save_path, &ppm_bytes)?;
        self.status_message = Some(format!("Saved: {} ({} KB)", save_path, ppm_bytes.len() / 1024));
        Ok(())
    }

    /// Handles mouse clicks inside the Paint window.
    pub fn handle_mouse_down(&mut self, win: &Window, x: i32, y: i32) -> bool {
        let client = win.client_rect();
        let rel_x = x - client.x;
        let rel_y = y - client.y;

        // 1. Top Action Toolbar (y: 0..26)
        if (0..26).contains(&rel_y) {
            // [ Clear ] (8..58)
            if (8..58).contains(&rel_x) {
                self.clear_canvas();
                return true;
            }
            // [ Eraser ] (64..124)
            if (64..124).contains(&rel_x) {
                self.is_eraser = !self.is_eraser;
                let state = if self.is_eraser { "ON" } else { "OFF" };
                self.status_message = Some(format!("Eraser {}", state));
                return true;
            }
            // [ 1px ] (130..162)
            if (130..162).contains(&rel_x) {
                self.brush_size = 1;
                self.status_message = Some("Brush: 1px".to_string());
                return true;
            }
            // [ 2px ] (168..200)
            if (168..200).contains(&rel_x) {
                self.brush_size = 2;
                self.status_message = Some("Brush: 2px".to_string());
                return true;
            }
            // [ 4px ] (206..238)
            if (206..238).contains(&rel_x) {
                self.brush_size = 4;
                self.status_message = Some("Brush: 4px".to_string());
                return true;
            }
            // [ Save ] (244..296)
            if (244..296).contains(&rel_x) {
                if let Err(err) = self.save_to_vfs() {
                    self.status_message = Some(format!("Save failed: {}", err));
                }
                return true;
            }
        }

        // 2. Color Palette Swatches (y: 28..50)
        if (28..50).contains(&rel_y) {
            let swatch_w = 28;
            let start_x = 10;
            for i in 0..PALETTE.len() {
                let sx = start_x + (i as i32 * swatch_w);
                if (sx..sx + 24).contains(&rel_x) {
                    self.selected_color_idx = i;
                    self.is_eraser = false;
                    self.status_message = Some(format!("Color #{} selected", i + 1));
                    return true;
                }
            }
        }

        // 3. Canvas Area (y: 54..274, x: 10..446)
        let canvas_x0 = 10;
        let canvas_y0 = 54;
        let cx = rel_x - canvas_x0;
        let cy = rel_y - canvas_y0;

        if (0..CANVAS_WIDTH as i32).contains(&cx) && (0..CANVAS_HEIGHT as i32).contains(&cy) {
            self.is_drawing = true;
            self.last_pt = Some((cx, cy));
            let color = self.active_color();
            self.draw_brush_stamp(cx, cy, color);
            return true;
        }

        false
    }

    /// Handles continuous mouse dragging across the canvas.
    pub fn handle_mouse_drag(&mut self, win: &Window, x: i32, y: i32) {
        if !self.is_drawing {
            return;
        }

        let client = win.client_rect();
        let canvas_x0 = client.x + 10;
        let canvas_y0 = client.y + 54;

        let cx = (x - canvas_x0).clamp(0, CANVAS_WIDTH as i32 - 1);
        let cy = (y - canvas_y0).clamp(0, CANVAS_HEIGHT as i32 - 1);

        if let Some((prev_x, prev_y)) = self.last_pt {
            self.stroke_line(prev_x, prev_y, cx, cy);
        } else {
            let color = self.active_color();
            self.draw_brush_stamp(cx, cy, color);
        }

        self.last_pt = Some((cx, cy));
    }

    /// Handles mouse button release.
    pub fn handle_mouse_up(&mut self) {
        self.is_drawing = false;
        self.last_pt = None;
    }

    /// Renders the complete Aegis Paint interface inside the window.
    pub fn render(&self, win: &Window, fb: &mut Framebuffer) {
        let client = win.client_rect();
        if client.width < 320 || client.height < 240 {
            return;
        }

        // 1. Top Action Toolbar (y: 0..26)
        let bar_h = 26;
        let bar_rect = Rect::new(client.x, client.y, client.width, bar_h);
        draw_rect(fb, bar_rect, Color::rgb(36, 40, 48));
        draw_rect(fb, Rect::new(client.x, client.y + bar_h as i32 - 1, client.width, 1), Color::WINDOW_BORDER);

        // [ Clear ]
        draw_rounded_rect(fb, Rect::new(client.x + 8, client.y + 4, 50, 18), 3, Color::BUTTON_BG);
        draw_string(fb, client.x + 14, client.y + 5, "Clear", Color::WHITE, None);

        // [ Eraser ]
        let eraser_bg = if self.is_eraser { Color::rgb(200, 70, 70) } else { Color::BUTTON_BG };
        draw_rounded_rect(fb, Rect::new(client.x + 64, client.y + 4, 60, 18), 3, eraser_bg);
        draw_string(fb, client.x + 70, client.y + 5, "Eraser", Color::WHITE, None);

        // Brush size toggles: [ 1px ] [ 2px ] [ 4px ]
        let b1_bg = if self.brush_size == 1 && !self.is_eraser { Color::rgb(40, 110, 180) } else { Color::BUTTON_BG };
        let b2_bg = if self.brush_size == 2 && !self.is_eraser { Color::rgb(40, 110, 180) } else { Color::BUTTON_BG };
        let b4_bg = if self.brush_size == 4 && !self.is_eraser { Color::rgb(40, 110, 180) } else { Color::BUTTON_BG };

        draw_rounded_rect(fb, Rect::new(client.x + 130, client.y + 4, 32, 18), 3, b1_bg);
        draw_string(fb, client.x + 134, client.y + 5, "1px", Color::WHITE, None);

        draw_rounded_rect(fb, Rect::new(client.x + 168, client.y + 4, 32, 18), 3, b2_bg);
        draw_string(fb, client.x + 172, client.y + 5, "2px", Color::WHITE, None);

        draw_rounded_rect(fb, Rect::new(client.x + 206, client.y + 4, 32, 18), 3, b4_bg);
        draw_string(fb, client.x + 210, client.y + 5, "4px", Color::WHITE, None);

        // [ Save ] (Emerald Green)
        draw_rounded_rect(fb, Rect::new(client.x + 244, client.y + 4, 52, 18), 3, Color::rgb(40, 120, 70));
        draw_string(fb, client.x + 252, client.y + 5, "Save", Color::WHITE, None);

        // 2. Color Palette Swatches (y: 28..50)
        let swatch_bar_y = client.y + bar_h as i32 + 2;
        let swatch_bar_rect = Rect::new(client.x, swatch_bar_y, client.width, 24);
        draw_rect(fb, swatch_bar_rect, Color::rgb(28, 31, 38));

        let start_x = client.x + 10;
        let swatch_w = 28;
        for (i, &color) in PALETTE.iter().enumerate() {
            let sx = start_x + (i as i32 * swatch_w);
            let sy = swatch_bar_y + 3;

            // Swatch rect
            draw_rounded_rect(fb, Rect::new(sx, sy, 22, 18), 3, color);
            draw_rounded_rect_outline(fb, Rect::new(sx, sy, 22, 18), 3, Color::rgb(80, 85, 95));

            // Selection highlight ring
            if i == self.selected_color_idx && !self.is_eraser {
                draw_circle_outline(fb, sx + 11, sy + 9, 7, Color::WHITE);
                draw_circle_outline(fb, sx + 11, sy + 9, 8, Color::rgb(40, 40, 40));
            }
        }

        // 3. Canvas Outer Frame & Shadow
        let canvas_x = client.x + 10;
        let canvas_y = swatch_bar_y + 26;
        let canvas_w = CANVAS_WIDTH as u32;
        let canvas_h = CANVAS_HEIGHT as u32;

        draw_rect_outline(fb, Rect::new(canvas_x - 1, canvas_y - 1, canvas_w + 2, canvas_h + 2), Color::rgb(60, 65, 75), 1);

        // Render Canvas Pixels to Framebuffer
        for cy in 0..CANVAS_HEIGHT {
            let py = canvas_y + cy as i32;
            let row_offset = cy * CANVAS_WIDTH;

            for cx in 0..CANVAS_WIDTH {
                let px = canvas_x + cx as i32;
                let color = self.canvas[row_offset + cx];
                fb.draw_pixel(px, py, color);
            }
        }

        // 4. Bottom Status Bar
        let status_y = client.bottom() - 20;
        let status_rect = Rect::new(client.x, status_y, client.width, 20);
        draw_rect(fb, status_rect, Color::rgb(26, 29, 36));
        draw_rect(fb, Rect::new(client.x, status_y, client.width, 1), Color::WINDOW_BORDER);

        let msg = self.status_message.as_deref().unwrap_or("Ready");
        let tool_name = if self.is_eraser { "Eraser" } else { "Brush" };
        let status_str = format!(
            "{} ({}px) | Canvas: 436x220 | {}",
            tool_name, self.brush_size, msg
        );
        draw_string(fb, client.x + 8, status_y + 2, &status_str, Color::TEXT_DIM, None);
    }
}
