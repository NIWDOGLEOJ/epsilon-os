//! Hardware Device Drivers for AegisOS
//!
//! Includes double-buffered linear RGB framebuffer, PS/2 keyboard, and PS/2 mouse.

pub mod ring;
pub mod framebuffer;
pub mod ps2_keyboard;
pub mod ps2_mouse;

pub use framebuffer::{clear_screen, get_dimensions, swap_buffers, with_framebuffer, Framebuffer};
pub use ps2_keyboard::{init_ps2_keyboard, poll_key_event, KeyCode, KeyEvent};
pub use ps2_mouse::{get_mouse_position, init_ps2_mouse, poll_mouse_event, MouseButton, MouseEvent};

/// Initializes all hardware drivers given a Limine Framebuffer reference.
pub fn init_drivers(fb: &limine::framebuffer::Framebuffer) {
    framebuffer::init_from_limine(fb);
    ps2_keyboard::init_ps2_keyboard();
    ps2_mouse::init_ps2_mouse(fb.width() as usize, fb.height() as usize);
}
