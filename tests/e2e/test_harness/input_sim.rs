//! AegisOS E2E Test Harness: PS/2 Mouse & Keyboard Drivers Simulator
//!
//! Models PS/2 Set 1 scancode decoding, shift/caps state machine,
//! 3-byte mouse packet parsing with sign extension, and cursor clamping.

use super::types::*;

pub struct KeyboardSimulator {
    pub shift_pressed: bool,
    pub ctrl_pressed: bool,
    pub caps_lock: bool,
    pub extended_prefix: bool,
    pub key_events: Vec<InputEvent>,
}

impl KeyboardSimulator {
    pub fn new() -> Self {
        Self {
            shift_pressed: false,
            ctrl_pressed: false,
            caps_lock: false,
            extended_prefix: false,
            key_events: Vec::new(),
        }
    }

    pub fn handle_scancode(&mut self, scancode: u8) -> Option<InputEvent> {
        if scancode == 0xE0 {
            self.extended_prefix = true;
            return None;
        }

        let is_break = (scancode & 0x80) != 0;
        let make_code = scancode & 0x7F;

        if is_break {
            match make_code {
                0x2A | 0x36 => self.shift_pressed = false,
                0x1D => self.ctrl_pressed = false,
                _ => {}
            }
            let event = InputEvent::KeyUp { scancode: make_code };
            self.key_events.push(event.clone());
            self.extended_prefix = false;
            return Some(event);
        }

        // Make code (Key Pressed)
        match make_code {
            0x2A | 0x36 => {
                self.shift_pressed = true;
            }
            0x1D => {
                self.ctrl_pressed = true;
            }
            0x3A => {
                self.caps_lock = !self.caps_lock;
            }
            _ => {}
        }

        let ascii = self.scancode_to_ascii(make_code, self.extended_prefix);
        self.extended_prefix = false;

        let event = InputEvent::KeyDown {
            key: ascii,
            scancode: make_code,
            shift: self.shift_pressed,
            ctrl: self.ctrl_pressed,
        };
        self.key_events.push(event.clone());
        Some(event)
    }

    fn scancode_to_ascii(&self, make_code: u8, extended: bool) -> u8 {
        if extended {
            return match make_code {
                0x48 => 0x80, // Up arrow
                0x50 => 0x81, // Down arrow
                0x4B => 0x82, // Left arrow
                0x4D => 0x83, // Right arrow
                0x53 => 0x7F, // Delete
                _ => 0,
            };
        }

        let shift = self.shift_pressed ^ self.caps_lock;
        match make_code {
            0x01 => 0x1B, // ESC
            0x02 => if self.shift_pressed { b'!' } else { b'1' },
            0x03 => if self.shift_pressed { b'@' } else { b'2' },
            0x04 => if self.shift_pressed { b'#' } else { b'3' },
            0x05 => if self.shift_pressed { b'$' } else { b'4' },
            0x06 => if self.shift_pressed { b'%' } else { b'5' },
            0x07 => if self.shift_pressed { b'^' } else { b'6' },
            0x08 => if self.shift_pressed { b'&' } else { b'7' },
            0x09 => if self.shift_pressed { b'*' } else { b'8' },
            0x0A => if self.shift_pressed { b'(' } else { b'9' },
            0x0B => if self.shift_pressed { b')' } else { b'0' },
            0x0C => if self.shift_pressed { b'_' } else { b'-' },
            0x0D => if self.shift_pressed { b'+' } else { b'=' },
            0x0E => 0x08, // Backspace
            0x0F => b'\t', // Tab
            0x10 => if shift { b'Q' } else { b'q' },
            0x11 => if shift { b'W' } else { b'w' },
            0x12 => if shift { b'E' } else { b'e' },
            0x13 => if shift { b'R' } else { b'r' },
            0x14 => if shift { b'T' } else { b't' },
            0x15 => if shift { b'Y' } else { b'y' },
            0x16 => if shift { b'U' } else { b'u' },
            0x17 => if shift { b'I' } else { b'i' },
            0x18 => if shift { b'O' } else { b'o' },
            0x19 => if shift { b'P' } else { b'p' },
            0x1C => b'\n', // Enter
            0x1E => if shift { b'A' } else { b'a' },
            0x1F => if shift { b'S' } else { b's' },
            0x20 => if shift { b'D' } else { b'd' },
            0x21 => if shift { b'F' } else { b'f' },
            0x22 => if shift { b'G' } else { b'g' },
            0x23 => if shift { b'H' } else { b'h' },
            0x24 => if shift { b'J' } else { b'j' },
            0x25 => if shift { b'K' } else { b'k' },
            0x26 => if shift { b'L' } else { b'l' },
            0x2C => if shift { b'Z' } else { b'z' },
            0x2D => if shift { b'X' } else { b'x' },
            0x2E => if shift { b'C' } else { b'c' },
            0x2F => if shift { b'V' } else { b'v' },
            0x30 => if shift { b'B' } else { b'b' },
            0x31 => if shift { b'N' } else { b'n' },
            0x32 => if shift { b'M' } else { b'm' },
            0x39 => b' ',  // Space
            _ => 0,
        }
    }
}

pub struct MouseSimulator {
    pub cursor_x: i32,
    pub cursor_y: i32,
    pub screen_width: usize,
    pub screen_height: usize,
    pub left_btn: bool,
    pub right_btn: bool,
    pub middle_btn: bool,
    pub events: Vec<InputEvent>,
}

impl MouseSimulator {
    pub fn new(screen_width: usize, screen_height: usize) -> Self {
        Self {
            cursor_x: (screen_width / 2) as i32,
            cursor_y: (screen_height / 2) as i32,
            screen_width,
            screen_height,
            left_btn: false,
            right_btn: false,
            middle_btn: false,
            events: Vec::new(),
        }
    }

    pub fn handle_packet(&mut self, bytes: [u8; 3]) -> Result<Vec<InputEvent>, &'static str> {
        // Bit 3 of byte 0 must always be 1
        if (bytes[0] & 0x08) == 0 {
            return Err("Corrupted mouse packet: Bit 3 is not set");
        }

        let left = (bytes[0] & 0x01) != 0;
        let right = (bytes[0] & 0x02) != 0;
        let middle = (bytes[0] & 0x04) != 0;

        let mut dx = bytes[1] as i32;
        if (bytes[0] & 0x10) != 0 {
            dx |= !0xFF; // Sign extend negative X
        }

        let mut dy = bytes[2] as i32;
        if (bytes[0] & 0x20) != 0 {
            dy |= !0xFF; // Sign extend negative Y
        }
        dy = -dy; // Invert PS/2 Y axis for screen downward coordinate space

        // Update cursor position and clamp to screen bounds
        let new_x = (self.cursor_x + dx).clamp(0, self.screen_width as i32 - 1);
        let new_y = (self.cursor_y + dy).clamp(0, self.screen_height as i32 - 1);
        self.cursor_x = new_x;
        self.cursor_y = new_y;

        let mut emitted = Vec::new();

        if dx != 0 || dy != 0 {
            let move_ev = InputEvent::MouseMove {
                x: self.cursor_x,
                y: self.cursor_y,
                dx,
                dy,
            };
            self.events.push(move_ev.clone());
            emitted.push(move_ev);
        }

        // Check Left button state transitions
        if left != self.left_btn {
            self.left_btn = left;
            let ev = if left {
                InputEvent::MouseDown {
                    button: MouseButton::Left,
                    x: self.cursor_x,
                    y: self.cursor_y,
                }
            } else {
                InputEvent::MouseUp {
                    button: MouseButton::Left,
                    x: self.cursor_x,
                    y: self.cursor_y,
                }
            };
            self.events.push(ev.clone());
            emitted.push(ev);
        }

        // Check Right button state transitions
        if right != self.right_btn {
            self.right_btn = right;
            let ev = if right {
                InputEvent::MouseDown {
                    button: MouseButton::Right,
                    x: self.cursor_x,
                    y: self.cursor_y,
                }
            } else {
                InputEvent::MouseUp {
                    button: MouseButton::Right,
                    x: self.cursor_x,
                    y: self.cursor_y,
                }
            };
            self.events.push(ev.clone());
            emitted.push(ev);
        }

        Ok(emitted)
    }
}
