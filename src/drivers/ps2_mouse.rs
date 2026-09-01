//! PS/2 Mouse Driver, Dynamic Velocity Acceleration & Hardware Cursor Renderer
//!
//! Enables the 8042 auxiliary device, configures 200Hz sample rate and 8 counts/mm resolution,
//! decodes 3-byte packets with dynamic acceleration scaling, clamps coordinates to screen bounds,
//! and renders a macOS-style 12x18 arrow cursor with hotspot at (0, 0).

use crate::drivers::ring::EventRing;
use spin::Mutex;

use crate::arch::serial::{inb, outb};
use crate::drivers::framebuffer::Framebuffer;
use crate::gui::primitives::Color;

// ============================================================================
// Mouse Event Data Structures
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseEvent {
    MouseMove { x: i32, y: i32, dx: i32, dy: i32 },
    MouseDown { button: MouseButton, x: i32, y: i32 },
    MouseUp { button: MouseButton, x: i32, y: i32 },
}

// ============================================================================
// Mouse Driver State Machine
// ============================================================================

pub struct MouseDriver {
    pub cursor_x: i32,
    pub cursor_y: i32,
    pub screen_width: i32,
    pub screen_height: i32,
    pub left_btn: bool,
    pub right_btn: bool,
    pub middle_btn: bool,
    pub packet_bytes: [u8; 3],
    pub packet_idx: usize,
}

impl MouseDriver {
    pub const fn new(screen_width: i32, screen_height: i32) -> Self {
        Self {
            cursor_x: screen_width / 2,
            cursor_y: screen_height / 2,
            screen_width,
            screen_height,
            left_btn: false,
            right_btn: false,
            middle_btn: false,
            packet_bytes: [0; 3],
            packet_idx: 0,
        }
    }

    /// Feeds a raw byte from port 0x60 into the packet decoder, appending any
    /// decoded events to `events`.
    ///
    /// Takes the destination by reference rather than returning a fresh
    /// `VecDeque`: this runs in the IRQ 12 handler, where allocating would risk
    /// deadlocking against a task interrupted inside the global allocator.
    pub fn process_byte(&mut self, byte: u8, events: &mut MouseEventQueue) {
        // Byte 0 validation: Bit 3 must ALWAYS be 1 in standard PS/2 packets
        if self.packet_idx == 0 && (byte & 0x08) == 0 {
            // Out of sync; discard byte
            return;
        }

        self.packet_bytes[self.packet_idx] = byte;
        self.packet_idx += 1;

        if self.packet_idx == 3 {
            self.packet_idx = 0;
            self.decode_packet(events);
        }
    }

    /// Applies dynamic non-linear acceleration curve to raw mouse delta.
    /// Maps a raw PS/2 count to a pixel delta.
    ///
    /// The controller is configured for 8 counts/mm at 200 Hz, so a count is
    /// already about a pixel at this resolution: a slow 10 cm drag across the mat
    /// produces ~800 counts, roughly the width of the screen. The curve therefore
    /// stays 1:1 for fine movement and only accelerates for fast flicks, capping
    /// at 2.5x. The previous quadratic term (`abs * 6 + abs * abs / 6`) turned a
    /// single 50-count packet into 716 pixels and slammed the cursor into a screen
    /// edge on any normal movement.
    fn scale_delta(delta: i32) -> i32 {
        let abs = delta.abs();
        let sign = if delta < 0 { -1 } else { 1 };

        let scaled = if abs == 0 {
            0
        } else if abs <= 4 {
            // Precision zone: no acceleration, so single counts stay addressable.
            abs
        } else if abs <= 10 {
            // Gentle ramp: 2x above the precision threshold.
            4 + (abs - 4) * 2
        } else {
            // Fast flick: 2.5x, so a full-scale 127-count packet moves ~308 px.
            16 + (abs - 10) * 5 / 2
        };

        sign * scaled
    }

    /// Decodes complete 3-byte PS/2 packet and emits movement and button events.
    fn decode_packet(&mut self, events: &mut MouseEventQueue) {
        let b0 = self.packet_bytes[0];
        let b1 = self.packet_bytes[1];
        let b2 = self.packet_bytes[2];

        let new_left = (b0 & 0x01) != 0;
        let new_right = (b0 & 0x02) != 0;
        let new_middle = (b0 & 0x04) != 0;

        let mut raw_dx = b1 as i32;
        if (b0 & 0x10) != 0 {
            raw_dx |= !0xFF; // Sign extend negative X delta
        }

        let mut raw_dy = b2 as i32;
        if (b0 & 0x20) != 0 {
            raw_dy |= !0xFF; // Sign extend negative Y delta
        }

        // Invert Y: PS/2 reports +Y upwards; screen coordinates are +Y downwards
        let raw_screen_dy = -raw_dy;

        // Apply smooth dynamic acceleration scaling
        let dx = Self::scale_delta(raw_dx);
        let screen_dy = Self::scale_delta(raw_screen_dy);

        // Apply movement delta and clamp to screen bounds
        let old_x = self.cursor_x;
        let old_y = self.cursor_y;
        self.cursor_x = (self.cursor_x + dx).clamp(0, self.screen_width - 1);
        self.cursor_y = (self.cursor_y + screen_dy).clamp(0, self.screen_height - 1);

        if self.cursor_x != old_x || self.cursor_y != old_y || dx != 0 || screen_dy != 0 {
            events.push(MouseEvent::MouseMove {
                x: self.cursor_x,
                y: self.cursor_y,
                dx,
                dy: screen_dy,
            });
        }

        // Check Left Button transitions
        if new_left != self.left_btn {
            self.left_btn = new_left;
            if new_left {
                events.push(MouseEvent::MouseDown {
                    button: MouseButton::Left,
                    x: self.cursor_x,
                    y: self.cursor_y,
                });
            } else {
                events.push(MouseEvent::MouseUp {
                    button: MouseButton::Left,
                    x: self.cursor_x,
                    y: self.cursor_y,
                });
            }
        }

        // Check Right Button transitions
        if new_right != self.right_btn {
            self.right_btn = new_right;
            if new_right {
                events.push(MouseEvent::MouseDown {
                    button: MouseButton::Right,
                    x: self.cursor_x,
                    y: self.cursor_y,
                });
            } else {
                events.push(MouseEvent::MouseUp {
                    button: MouseButton::Right,
                    x: self.cursor_x,
                    y: self.cursor_y,
                });
            }
        }

        // Check Middle Button transitions
        if new_middle != self.middle_btn {
            self.middle_btn = new_middle;
            if new_middle {
                events.push(MouseEvent::MouseDown {
                    button: MouseButton::Middle,
                    x: self.cursor_x,
                    y: self.cursor_y,
                });
            } else {
                events.push(MouseEvent::MouseUp {
                    button: MouseButton::Middle,
                    x: self.cursor_x,
                    y: self.cursor_y,
                });
            }
        }
    }
}

// ============================================================================
// macOS 12x18 Arrow Cursor Sprite
// ============================================================================

const CURSOR_WIDTH: usize = 12;
const CURSOR_HEIGHT: usize = 18;

// 1 = White Interior, 2 = Black Outline, 0 = Transparent
static CURSOR_SPRITE: [[u8; CURSOR_WIDTH]; CURSOR_HEIGHT] = [
    [2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    [2, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    [2, 1, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    [2, 1, 1, 2, 0, 0, 0, 0, 0, 0, 0, 0],
    [2, 1, 1, 1, 2, 0, 0, 0, 0, 0, 0, 0],
    [2, 1, 1, 1, 1, 2, 0, 0, 0, 0, 0, 0],
    [2, 1, 1, 1, 1, 1, 2, 0, 0, 0, 0, 0],
    [2, 1, 1, 1, 1, 1, 1, 2, 0, 0, 0, 0],
    [2, 1, 1, 1, 1, 1, 1, 1, 2, 0, 0, 0],
    [2, 1, 1, 1, 1, 1, 1, 1, 1, 2, 0, 0],
    [2, 1, 1, 1, 1, 1, 2, 2, 2, 2, 2, 0],
    [2, 1, 1, 2, 1, 1, 2, 0, 0, 0, 0, 0],
    [2, 1, 2, 0, 2, 1, 1, 2, 0, 0, 0, 0],
    [2, 2, 0, 0, 2, 1, 1, 2, 0, 0, 0, 0],
    [2, 0, 0, 0, 0, 2, 1, 1, 2, 0, 0, 0],
    [0, 0, 0, 0, 0, 2, 1, 1, 2, 0, 0, 0],
    [0, 0, 0, 0, 0, 0, 2, 2, 0, 0, 0, 0],
    [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
];

/// Renders the mouse cursor at (x, y) with tip hotspot at (0, 0).
pub fn draw_cursor(fb: &mut Framebuffer, x: i32, y: i32) {
    let white = Color::WHITE;
    let black = Color::BLACK;

    for (row, line) in CURSOR_SPRITE.iter().enumerate() {
        let py = y + row as i32;
        for (col, &pixel) in line.iter().enumerate() {
            let px = x + col as i32;
            if pixel == 1 {
                fb.draw_pixel(px, py, white);
            } else if pixel == 2 {
                fb.draw_pixel(px, py, black);
            }
        }
    }
}

// ============================================================================
// Global PS/2 Mouse Driver Singleton & Queue
// ============================================================================

/// Preallocated queue handed from the IRQ handler to the compositor loop.
pub type MouseEventQueue = EventRing<MouseEvent, 256>;

static MOUSE_DRIVER: Mutex<MouseDriver> = Mutex::new(MouseDriver::new(1024, 768));
static MOUSE_QUEUE: Mutex<MouseEventQueue> = Mutex::new(MouseEventQueue::new());

fn mouse_wait_write() {
    for _ in 0..100_000 {
        let status = unsafe { inb(0x64) };
        if (status & 0x02) == 0 {
            return;
        }
        core::hint::spin_loop();
    }
}

fn mouse_wait_read() {
    for _ in 0..100_000 {
        let status = unsafe { inb(0x64) };
        if (status & 0x01) != 0 {
            return;
        }
        core::hint::spin_loop();
    }
}

/// Hardware Mouse IRQ callback registered on IRQ 12 (vector 44).
pub fn on_mouse_irq(_irq: u8, _ctx: &mut crate::arch::idt::InterruptContext) {
    let status = unsafe { inb(0x64) };
    if (status & 0x01) != 0 && (status & 0x20) != 0 {
        let byte = unsafe { inb(0x60) };

        // Lock order driver -> queue, matching `poll_mouse_event`.
        let mut driver = MOUSE_DRIVER.lock();
        let mut queue = MOUSE_QUEUE.lock();
        driver.process_byte(byte, &mut queue);
    }
}

/// Initializes the 8042 PS/2 mouse auxiliary controller with high sample rate & resolution.
pub fn init_ps2_mouse(width: usize, height: usize) {
    {
        let _guard = crate::arch::InterruptGuard::acquire();
        let mut driver = MOUSE_DRIVER.lock();
        driver.screen_width = width as i32;
        driver.screen_height = height as i32;
        driver.cursor_x = width as i32 / 2;
        driver.cursor_y = height as i32 / 2;
    }

    // 1. Enable Auxiliary Device
    mouse_wait_write();
    unsafe { outb(0x64, 0xA8) };

    // 2. Read Controller Configuration Byte
    mouse_wait_write();
    unsafe { outb(0x64, 0x20) };
    mouse_wait_read();
    let mut config = unsafe { inb(0x60) };

    // 3. Enable IRQ 12 and disable mouse clock inhibit
    config |= 0x02; // Enable IRQ 12
    config &= !0x20; // Clear mouse clock disable bit
    mouse_wait_write();
    unsafe { outb(0x64, 0x60) };
    mouse_wait_write();
    unsafe { outb(0x60, config) };

    // 4. Send Reset / Default command to mouse
    mouse_wait_write();
    unsafe { outb(0x64, 0xD4) };
    mouse_wait_write();
    unsafe { outb(0x60, 0xF6) }; // Set defaults
    mouse_wait_read();
    let _ack1 = unsafe { inb(0x60) };

    // 5. Configure Sample Rate to 200 samples/sec (Max smoothness)
    mouse_wait_write();
    unsafe { outb(0x64, 0xD4) };
    mouse_wait_write();
    unsafe { outb(0x60, 0xF3) };
    mouse_wait_read();
    let _ = unsafe { inb(0x60) };

    mouse_wait_write();
    unsafe { outb(0x64, 0xD4) };
    mouse_wait_write();
    unsafe { outb(0x60, 200) }; // 200 Hz
    mouse_wait_read();
    let _ = unsafe { inb(0x60) };

    // 6. Configure Resolution to 8 counts/mm (Max sensitivity)
    mouse_wait_write();
    unsafe { outb(0x64, 0xD4) };
    mouse_wait_write();
    unsafe { outb(0x60, 0xE8) };
    mouse_wait_read();
    let _ = unsafe { inb(0x60) };

    mouse_wait_write();
    unsafe { outb(0x64, 0xD4) };
    mouse_wait_write();
    unsafe { outb(0x60, 0x03) }; // 8 counts/mm
    mouse_wait_read();
    let _ = unsafe { inb(0x60) };

    // 7. Enable 2:1 Scaling
    mouse_wait_write();
    unsafe { outb(0x64, 0xD4) };
    mouse_wait_write();
    unsafe { outb(0x60, 0xE7) };
    mouse_wait_read();
    let _ = unsafe { inb(0x60) };

    // 8. Enable Data Reporting (Streaming Mode)
    mouse_wait_write();
    unsafe { outb(0x64, 0xD4) };
    mouse_wait_write();
    unsafe { outb(0x60, 0xF4) }; // Enable streaming
    mouse_wait_read();
    let _ack2 = unsafe { inb(0x60) };

    // Register IRQ callback
    crate::arch::idt::register_mouse_callback(on_mouse_irq);
}

/// Polls and drains all pending hardware mouse bytes.
pub fn poll_mouse_event() -> Option<MouseEvent> {
    // MOUSE_DRIVER and MOUSE_QUEUE are shared with `on_mouse_irq`. Without the
    // guard, an IRQ 12 landing inside this function deadlocks against it.
    let _guard = crate::arch::InterruptGuard::acquire();
    let status = unsafe { inb(0x64) };
    if (status & 0x01) != 0 && (status & 0x20) != 0 {
        let byte = unsafe { inb(0x60) };
        let mut driver = MOUSE_DRIVER.lock();
        let mut queue = MOUSE_QUEUE.lock();
        driver.process_byte(byte, &mut queue);
    }

    MOUSE_QUEUE.lock().pop()
}

/// Returns current (x, y) coordinates of the mouse cursor.
pub fn get_mouse_position() -> (i32, i32) {
    let _guard = crate::arch::InterruptGuard::acquire();
    let driver = MOUSE_DRIVER.lock();
    (driver.cursor_x, driver.cursor_y)
}
