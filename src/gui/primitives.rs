//! 2D Vector Drawing Primitives, Alpha Blending Engine and macOS Color System
//!
//! Provides pixel blending, anti-aliased / rounded rectangles, circles, gradients,
//! drop shadows, outlines, and geometric intersection maths for AegisOS.

use crate::drivers::framebuffer::Framebuffer;

// ============================================================================
// Color Representation & Alpha Blending
// ============================================================================

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    #[inline(always)]
    pub fn to_u32(self) -> u32 {
        ((self.r as u32) << 16) | ((self.g as u32) << 8) | (self.b as u32)
    }

    #[inline(always)]
    pub fn from_u32(val: u32) -> Self {
        Self {
            r: ((val >> 16) & 0xFF) as u8,
            g: ((val >> 8) & 0xFF) as u8,
            b: (val & 0xFF) as u8,
            a: 255,
        }
    }

    #[inline(always)]
    pub fn blend(src: Color, dst: Color) -> Color {
        if src.a == 255 {
            return src;
        }
        if src.a == 0 {
            return dst;
        }

        let alpha = src.a as u32;
        let inv_alpha = 255 - alpha;

        Color {
            r: (((src.r as u32 * alpha) + (dst.r as u32 * inv_alpha)) / 255) as u8,
            g: (((src.g as u32 * alpha) + (dst.g as u32 * inv_alpha)) / 255) as u8,
            b: (((src.b as u32 * alpha) + (dst.b as u32 * inv_alpha)) / 255) as u8,
            a: 255,
        }
    }

    // --- Standard macOS Color Palette ---
    pub const WHITE: Color = Color::rgb(255, 255, 255);
    pub const BLACK: Color = Color::rgb(0, 0, 0);
    pub const TRANSPARENT: Color = Color::rgba(0, 0, 0, 0);

    pub const RED: Color = Color::rgb(255, 95, 86);
    pub const RED_HOVER: Color = Color::rgb(255, 59, 48);
    pub const YELLOW: Color = Color::rgb(255, 189, 46);
    pub const YELLOW_HOVER: Color = Color::rgb(255, 149, 0);
    pub const GREEN: Color = Color::rgb(39, 201, 63);
    pub const GREEN_HOVER: Color = Color::rgb(40, 205, 65);
    pub const BLUE: Color = Color::rgb(0, 122, 255);

    pub const DESKTOP_BG_TOP: Color = Color::rgb(30, 34, 42);
    pub const DESKTOP_BG_BOTTOM: Color = Color::rgb(20, 22, 28);

    pub const MENUBAR_BG: Color = Color::rgba(24, 24, 26, 235);
    pub const MENUBAR_BORDER: Color = Color::rgb(46, 50, 58);

    pub const DOCK_BG: Color = Color::rgba(26, 29, 36, 225);
    pub const DOCK_BORDER: Color = Color::rgb(62, 68, 81);

    pub const WINDOW_BG: Color = Color::rgb(33, 37, 43);
    pub const WINDOW_BORDER: Color = Color::rgb(59, 64, 72);

    pub const TITLEBAR_ACTIVE_TOP: Color = Color::rgb(44, 49, 60);
    pub const TITLEBAR_ACTIVE_BOTTOM: Color = Color::rgb(36, 40, 48);
    pub const TITLEBAR_INACTIVE: Color = Color::rgb(30, 34, 39);

    pub const TEXT_PRIMARY: Color = Color::rgb(229, 229, 229);
    pub const TEXT_DIM: Color = Color::rgb(138, 145, 158);
    pub const TEXT_HIGHLIGHT: Color = Color::rgb(80, 250, 123);
    pub const TEXT_WARNING: Color = Color::rgb(241, 250, 140);
    pub const TEXT_DANGER: Color = Color::rgb(255, 85, 85);

    pub const BUTTON_BG: Color = Color::rgb(48, 54, 66);
    pub const BUTTON_BORDER: Color = Color::rgb(70, 78, 92);
    pub const BUTTON_HOVER: Color = Color::rgb(60, 68, 82);
    pub const BUTTON_ACTIVE: Color = Color::rgb(0, 122, 255);
}

// ============================================================================
// Geometric Rectangle Structure
// ============================================================================

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, width: u32, height: u32) -> Self {
        Self { x, y, width, height }
    }

    #[inline]
    pub fn right(&self) -> i32 {
        self.x + self.width as i32
    }

    #[inline]
    pub fn bottom(&self) -> i32 {
        self.y + self.height as i32
    }

    #[inline]
    pub fn contains(&self, px: i32, py: i32) -> bool {
        px >= self.x && px < self.right() && py >= self.y && py < self.bottom()
    }

    pub fn intersects(&self, other: &Rect) -> bool {
        self.x < other.right()
            && self.right() > other.x
            && self.y < other.bottom()
            && self.bottom() > other.y
    }

    pub fn intersection(&self, other: &Rect) -> Option<Rect> {
        let ix1 = self.x.max(other.x);
        let iy1 = self.y.max(other.y);
        let ix2 = self.right().min(other.right());
        let iy2 = self.bottom().min(other.bottom());

        if ix1 < ix2 && iy1 < iy2 {
            Some(Rect::new(ix1, iy1, (ix2 - ix1) as u32, (iy2 - iy1) as u32))
        } else {
            None
        }
    }

    pub fn union(&self, other: &Rect) -> Rect {
        let ux1 = self.x.min(other.x);
        let uy1 = self.y.min(other.y);
        let ux2 = self.right().max(other.right());
        let uy2 = self.bottom().max(other.bottom());

        Rect::new(ux1, uy1, (ux2 - ux1) as u32, (uy2 - uy1) as u32)
    }
}

// ============================================================================
// 2D Drawing Primitives
// ============================================================================

/// Draws a filled rectangle
pub fn draw_rect(fb: &mut Framebuffer, rect: Rect, color: Color) {
    if rect.width == 0 || rect.height == 0 {
        return;
    }
    let end_x = rect.right();
    let end_y = rect.bottom();

    for y in rect.y..end_y {
        fb.fill_span(rect.x, end_x, y, color);
    }
}

/// Draws a rectangle border outline
pub fn draw_rect_outline(fb: &mut Framebuffer, rect: Rect, border_color: Color, thickness: u32) {
    if rect.width == 0 || rect.height == 0 || thickness == 0 {
        return;
    }
    let t = thickness as i32;

    // Top horizontal bar
    draw_rect(fb, Rect::new(rect.x, rect.y, rect.width, thickness), border_color);
    // Bottom horizontal bar
    draw_rect(fb, Rect::new(rect.x, rect.bottom() - t, rect.width, thickness), border_color);
    // Left vertical bar
    draw_rect(fb, Rect::new(rect.x, rect.y + t, thickness, rect.height.saturating_sub(thickness * 2)), border_color);
    // Right vertical bar
    draw_rect(fb, Rect::new(rect.right() - t, rect.y + t, thickness, rect.height.saturating_sub(thickness * 2)), border_color);
}

/// Draws a rounded rectangle with circular quadrant corners
pub fn draw_rounded_rect(fb: &mut Framebuffer, rect: Rect, radius: u32, color: Color) {
    if rect.width == 0 || rect.height == 0 {
        return;
    }
    let r = clamp_radius(rect, radius);
    if r <= 0 {
        draw_rect(fb, rect, color);
        return;
    }

    for y in rect.y..rect.bottom() {
        if let Some((start_x, end_x)) = rounded_row_span(rect, r, y) {
            fb.fill_span(start_x, end_x, y, color);
        }
    }
}

/// Clamps a nominal corner radius to what the rectangle can actually accommodate.
fn clamp_radius(rect: Rect, radius: u32) -> i32 {
    radius.min(rect.width / 2).min(rect.height / 2) as i32
}

/// Horizontal extent `[start, end)` of a rounded rectangle on row `y`, or `None`
/// for a row outside it.
///
/// Solves the corner circle for the row instead of testing every pixel against it:
/// a pixel left of the top-left centre is inside when `dx^2 + dy^2 <= r^2`, so the
/// span starts at `cx_left - sqrt(r^2 - dy^2)`. Produces exactly the same pixels as
/// the per-pixel test, one span at a time.
fn rounded_row_span(rect: Rect, r: i32, y: i32) -> Option<(i32, i32)> {
    if y < rect.y || y >= rect.bottom() {
        return None;
    }

    let cy_top = rect.y + r;
    let cy_bottom = rect.bottom() - r - 1;

    let dy = if y < cy_top {
        cy_top - y
    } else if y > cy_bottom {
        y - cy_bottom
    } else {
        // Straight-sided middle band: full width.
        return Some((rect.x, rect.right()));
    };

    let remaining = r * r - dy * dy;
    if remaining < 0 {
        return None;
    }

    let dx = (remaining as u32).isqrt() as i32;
    Some((rect.x + r - dx, rect.right() - r + dx))
}

/// Draws a rounded rectangle outline
pub fn draw_rounded_rect_outline(fb: &mut Framebuffer, rect: Rect, radius: u32, border_color: Color) {
    if rect.width == 0 || rect.height == 0 {
        return;
    }
    let r = radius.min(rect.width / 2).min(rect.height / 2) as i32;
    if r <= 0 {
        draw_rect_outline(fb, rect, border_color, 1);
        return;
    }

    let r_sq = r * r;
    let inner_r_sq = (r - 1).max(0) * (r - 1).max(0);
    let cx_left = rect.x + r;
    let cx_right = rect.right() - r - 1;
    let cy_top = rect.y + r;
    let cy_bottom = rect.bottom() - r - 1;

    for y in rect.y..rect.bottom() {
        for x in rect.x..rect.right() {
            let is_edge = if x < cx_left && y < cy_top {
                let dx = cx_left - x;
                let dy = cy_top - y;
                let d2 = dx * dx + dy * dy;
                d2 <= r_sq && d2 >= inner_r_sq
            } else if x > cx_right && y < cy_top {
                let dx = x - cx_right;
                let dy = cy_top - y;
                let d2 = dx * dx + dy * dy;
                d2 <= r_sq && d2 >= inner_r_sq
            } else if x < cx_left && y > cy_bottom {
                let dx = cx_left - x;
                let dy = y - cy_bottom;
                let d2 = dx * dx + dy * dy;
                d2 <= r_sq && d2 >= inner_r_sq
            } else if x > cx_right && y > cy_bottom {
                let dx = x - cx_right;
                let dy = y - cy_bottom;
                let d2 = dx * dx + dy * dy;
                d2 <= r_sq && d2 >= inner_r_sq
            } else {
                x == rect.x || x == rect.right() - 1 || y == rect.y || y == rect.bottom() - 1
            };

            if is_edge {
                fb.draw_pixel(x, y, border_color);
            }
        }
    }
}

/// Draws a filled circle
pub fn draw_circle(fb: &mut Framebuffer, cx: i32, cy: i32, radius: u32, color: Color) {
    let r = radius as i32;
    let r_sq = r * r;

    for dy in -r..=r {
        for dx in -r..=r {
            if dx * dx + dy * dy <= r_sq {
                fb.draw_pixel(cx + dx, cy + dy, color);
            }
        }
    }
}

/// Draws a circle outline
pub fn draw_circle_outline(fb: &mut Framebuffer, cx: i32, cy: i32, radius: u32, color: Color) {
    let r = radius as i32;
    let r_sq = r * r;
    let inner_sq = (r - 1).max(0) * (r - 1).max(0);

    for dy in -r..=r {
        for dx in -r..=r {
            let d2 = dx * dx + dy * dy;
            if d2 <= r_sq && d2 >= inner_sq {
                fb.draw_pixel(cx + dx, cy + dy, color);
            }
        }
    }
}

/// Draws a linear vertical gradient
pub fn draw_gradient_v(fb: &mut Framebuffer, rect: Rect, top_color: Color, bottom_color: Color) {
    if rect.width == 0 || rect.height == 0 {
        return;
    }
    let h = rect.height as i32;

    for row in 0..h {
        let y = rect.y + row;
        let t = row as u32;
        let inv_t = (h - 1 - row).max(0) as u32;
        let div = (h - 1).max(1) as u32;

        let r = (((top_color.r as u32 * inv_t) + (bottom_color.r as u32 * t)) / div) as u8;
        let g = (((top_color.g as u32 * inv_t) + (bottom_color.g as u32 * t)) / div) as u8;
        let b = (((top_color.b as u32 * inv_t) + (bottom_color.b as u32 * t)) / div) as u8;
        let a = (((top_color.a as u32 * inv_t) + (bottom_color.a as u32 * t)) / div) as u8;

        let row_color = Color::rgba(r, g, b, a);
        fb.fill_span(rect.x, rect.right(), y, row_color);
    }
}

/// Draws a soft blurred drop shadow around a rectangle.
///
/// `occluded_by` names a fully opaque rounded rect — as `(rect, corner_radius)` —
/// that the caller paints over this shadow immediately afterwards. Pixels it will
/// cover are skipped: the concentric steps overdraw the whole interior once per
/// step, and under an opaque body none of that is ever visible. Pass `None` when
/// the body is translucent, because then the shadow beneath it does show through.
pub fn draw_shadow(
    fb: &mut Framebuffer,
    rect: Rect,
    radius: u32,
    opacity: u8,
    occluded_by: Option<(Rect, u32)>,
) {
    let r = radius as i32;
    if r <= 0 || opacity == 0 {
        return;
    }

    let occluder = occluded_by.map(|(body, body_radius)| (body, clamp_radius(body, body_radius)));

    for step in 1..=r {
        let alpha = ((opacity as u32 * (r - step + 1) as u32) / (r as u32 * 3)) as u8;
        if alpha == 0 {
            continue;
        }
        let shadow_color = Color::rgba(0, 0, 0, alpha);
        let s_rect = Rect::new(
            rect.x - step,
            rect.y - step + 2, // Slight downward bias for macOS drop shadow feel
            rect.width + (step * 2) as u32,
            rect.height + (step * 2) as u32,
        );

        let s_radius = clamp_radius(s_rect, radius + 2);
        if s_radius <= 0 {
            draw_rect(fb, s_rect, shadow_color);
            continue;
        }

        for y in s_rect.y..s_rect.bottom() {
            let Some((start_x, end_x)) = rounded_row_span(s_rect, s_radius, y) else {
                continue;
            };

            // Split the span around the occluding body on the rows it spans.
            match occluder.and_then(|(body, body_r)| rounded_row_span(body, body_r, y)) {
                Some((body_start, body_end)) => {
                    fb.fill_span(start_x, end_x.min(body_start), y, shadow_color);
                    fb.fill_span(start_x.max(body_end), end_x, y, shadow_color);
                }
                None => fb.fill_span(start_x, end_x, y, shadow_color),
            }
        }
    }
}

/// Draws a Bresenham line
pub fn draw_line(fb: &mut Framebuffer, mut x0: i32, mut y0: i32, x1: i32, y1: i32, color: Color) {
    let dx = (x1 - x0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let dy = -(y1 - y0).abs();
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy;

    loop {
        fb.draw_pixel(x0, y0, color);
        if x0 == x1 && y0 == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x0 += sx;
        }
        if e2 <= dx {
            err += dx;
            y0 += sy;
        }
    }
}
