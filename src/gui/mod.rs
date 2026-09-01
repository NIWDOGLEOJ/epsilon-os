//! macOS Graphical Desktop Subsystem & Window Compositor
//!
//! Exposes 2D drawing primitives, 8x16 font renderer, 24px top menu bar,
//! bottom launcher dock, floating window model, and Z-ordered window manager.

pub mod dock;
pub mod font;
pub mod menubar;
pub mod primitives;
pub mod window;
pub mod wm;

pub use dock::{get_dock_rect, hit_test_dock, render_dock, AppId, DOCK_HEIGHT, DOCK_WIDTH};
pub use font::{draw_char, draw_string, measure_string, FONT_HEIGHT, FONT_WIDTH};
pub use menubar::{render_menubar, MENUBAR_HEIGHT};
pub use primitives::{
    draw_circle, draw_circle_outline, draw_gradient_v, draw_line, draw_rect, draw_rect_outline,
    draw_rounded_rect, draw_rounded_rect_outline, draw_shadow, Color, Rect,
};
pub use window::{Window, TITLEBAR_HEIGHT};
pub use wm::{WmAction, WindowManager};
