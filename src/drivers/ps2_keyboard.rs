//! PS/2 Keyboard Driver & Set 1 Scancode Decoder for AegisOS
//!
//! Handles Make and Break scancodes, Shift/Ctrl/Alt/CapsLock modifier tracking,
//! extended 0xE0 navigation scancodes (arrows, delete), and translates inputs
//! into structured `KeyEvent` records.

use crate::drivers::ring::EventRing;
use spin::Mutex;

use crate::arch::serial::inb;

// ============================================================================
// Key Codes & Event Data Structures
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyCode {
    Printable(u8),
    Up,
    Down,
    Left,
    Right,
    Enter,
    Backspace,
    Delete,
    Tab,
    Escape,
    Home,
    End,
    PageUp,
    PageDown,
    F(u8),
    Unknown(u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyEvent {
    pub code: KeyCode,
    pub char_byte: Option<u8>,
    pub pressed: bool,
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub caps: bool,
    pub scancode: u8,
}

// ============================================================================
// Set 1 Scancode Translation Tables (US QWERTY)
// ============================================================================

static SCANCODE_MAP_NORMAL: [Option<u8>; 128] = {
    let mut map = [None; 128];
    map[0x01] = Some(0x1B); // Escape
    map[0x02] = Some(b'1');
    map[0x03] = Some(b'2');
    map[0x04] = Some(b'3');
    map[0x05] = Some(b'4');
    map[0x06] = Some(b'5');
    map[0x07] = Some(b'6');
    map[0x08] = Some(b'7');
    map[0x09] = Some(b'8');
    map[0x0A] = Some(b'9');
    map[0x0B] = Some(b'0');
    map[0x0C] = Some(b'-');
    map[0x0D] = Some(b'=');
    map[0x0E] = Some(0x08); // Backspace
    map[0x0F] = Some(b'\t'); // Tab
    map[0x10] = Some(b'q');
    map[0x11] = Some(b'w');
    map[0x12] = Some(b'e');
    map[0x13] = Some(b'r');
    map[0x14] = Some(b't');
    map[0x15] = Some(b'y');
    map[0x16] = Some(b'u');
    map[0x17] = Some(b'i');
    map[0x18] = Some(b'o');
    map[0x19] = Some(b'p');
    map[0x1A] = Some(b'[');
    map[0x1B] = Some(b']');
    map[0x1C] = Some(b'\n'); // Enter
    map[0x1E] = Some(b'a');
    map[0x1F] = Some(b's');
    map[0x20] = Some(b'd');
    map[0x21] = Some(b'f');
    map[0x22] = Some(b'g');
    map[0x23] = Some(b'h');
    map[0x24] = Some(b'j');
    map[0x25] = Some(b'k');
    map[0x26] = Some(b'l');
    map[0x27] = Some(b';');
    map[0x28] = Some(b'\'');
    map[0x29] = Some(b'`');
    map[0x2B] = Some(b'\\');
    map[0x2C] = Some(b'z');
    map[0x2D] = Some(b'x');
    map[0x2E] = Some(b'c');
    map[0x2F] = Some(b'v');
    map[0x30] = Some(b'b');
    map[0x31] = Some(b'n');
    map[0x32] = Some(b'm');
    map[0x33] = Some(b',');
    map[0x34] = Some(b'.');
    map[0x35] = Some(b'/');
    map[0x39] = Some(b' '); // Space
    map
};

static SCANCODE_MAP_SHIFTED: [Option<u8>; 128] = {
    let mut map = [None; 128];
    map[0x01] = Some(0x1B);
    map[0x02] = Some(b'!');
    map[0x03] = Some(b'@');
    map[0x04] = Some(b'#');
    map[0x05] = Some(b'$');
    map[0x06] = Some(b'%');
    map[0x07] = Some(b'^');
    map[0x08] = Some(b'&');
    map[0x09] = Some(b'*');
    map[0x0A] = Some(b'(');
    map[0x0B] = Some(b')');
    map[0x0C] = Some(b'_');
    map[0x0D] = Some(b'+');
    map[0x0E] = Some(0x08);
    map[0x0F] = Some(b'\t');
    map[0x10] = Some(b'Q');
    map[0x11] = Some(b'W');
    map[0x12] = Some(b'E');
    map[0x13] = Some(b'R');
    map[0x14] = Some(b'T');
    map[0x15] = Some(b'Y');
    map[0x16] = Some(b'U');
    map[0x17] = Some(b'I');
    map[0x18] = Some(b'O');
    map[0x19] = Some(b'P');
    map[0x1A] = Some(b'{');
    map[0x1B] = Some(b'}');
    map[0x1C] = Some(b'\n');
    map[0x1E] = Some(b'A');
    map[0x1F] = Some(b'S');
    map[0x20] = Some(b'D');
    map[0x21] = Some(b'F');
    map[0x22] = Some(b'G');
    map[0x23] = Some(b'H');
    map[0x24] = Some(b'J');
    map[0x25] = Some(b'K');
    map[0x26] = Some(b'L');
    map[0x27] = Some(b':');
    map[0x28] = Some(b'"');
    map[0x29] = Some(b'~');
    map[0x2B] = Some(b'|');
    map[0x2C] = Some(b'Z');
    map[0x2D] = Some(b'X');
    map[0x2E] = Some(b'C');
    map[0x2F] = Some(b'V');
    map[0x30] = Some(b'B');
    map[0x31] = Some(b'N');
    map[0x32] = Some(b'M');
    map[0x33] = Some(b'<');
    map[0x34] = Some(b'>');
    map[0x35] = Some(b'?');
    map[0x39] = Some(b' ');
    map
};

// ============================================================================
// PS/2 Keyboard State Machine
// ============================================================================

pub struct KeyboardState {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub caps_lock: bool,
    pub is_extended: bool,
}

impl KeyboardState {
    pub const fn new() -> Self {
        Self {
            shift: false,
            ctrl: false,
            alt: false,
            caps_lock: false,
            is_extended: false,
        }
    }

    /// Processes an incoming raw 8-bit scancode and returns a `KeyEvent` if applicable.
    pub fn process_scancode(&mut self, scancode: u8) -> Option<KeyEvent> {
        // Extended scancode prefix
        if scancode == 0xE0 {
            self.is_extended = true;
            return None;
        }

        let is_break = (scancode & 0x80) != 0;
        let make_code = scancode & 0x7F;

        if self.is_extended {
            self.is_extended = false;
            let code = match make_code {
                0x48 => KeyCode::Up,
                0x50 => KeyCode::Down,
                0x4B => KeyCode::Left,
                0x4D => KeyCode::Right,
                0x53 => KeyCode::Delete,
                0x47 => KeyCode::Home,
                0x4F => KeyCode::End,
                0x49 => KeyCode::PageUp,
                0x51 => KeyCode::PageDown,
                _ => KeyCode::Unknown(scancode),
            };

            let char_byte = match code {
                KeyCode::Up => Some(0x80),
                KeyCode::Down => Some(0x81),
                KeyCode::Left => Some(0x82),
                KeyCode::Right => Some(0x83),
                KeyCode::Delete => Some(0x7F),
                _ => None,
            };

            return Some(KeyEvent {
                code,
                char_byte,
                pressed: !is_break,
                shift: self.shift,
                ctrl: self.ctrl,
                alt: self.alt,
                caps: self.caps_lock,
                scancode,
            });
        }

        // Handle modifier keys
        match make_code {
            0x2A | 0x36 => {
                // Left or Right Shift
                self.shift = !is_break;
                return None;
            }
            0x1D => {
                // Ctrl
                self.ctrl = !is_break;
                return None;
            }
            0x38 => {
                // Alt
                self.alt = !is_break;
                return None;
            }
            0x3A => {
                // Caps Lock toggle on key press
                if !is_break {
                    self.caps_lock = !self.caps_lock;
                }
                return None;
            }
            _ => {}
        }

        if is_break {
            return None;
        }

        // Lookup character from translation tables
        let use_shifted = self.shift ^ (self.caps_lock && (0x10..=0x32).contains(&make_code));
        let char_opt = if use_shifted {
            SCANCODE_MAP_SHIFTED[make_code as usize]
        } else {
            SCANCODE_MAP_NORMAL[make_code as usize]
        };

        let code = match make_code {
            0x01 => KeyCode::Escape,
            0x0E => KeyCode::Backspace,
            0x0F => KeyCode::Tab,
            0x1C => KeyCode::Enter,
            0x3B..=0x44 => KeyCode::F(make_code - 0x3A),
            _ => {
                if let Some(c) = char_opt {
                    KeyCode::Printable(c)
                } else {
                    KeyCode::Unknown(make_code)
                }
            }
        };

        Some(KeyEvent {
            code,
            char_byte: char_opt,
            pressed: true,
            shift: self.shift,
            ctrl: self.ctrl,
            alt: self.alt,
            caps: self.caps_lock,
            scancode,
        })
    }
}

// ============================================================================
// Global Driver Queue & Initialization
// ============================================================================

static KEYBOARD_STATE: Mutex<KeyboardState> = Mutex::new(KeyboardState::new());
/// Preallocated queue handed from the IRQ handler to the compositor loop.
pub type KeyEventQueue = EventRing<KeyEvent, 256>;

static KEY_QUEUE: Mutex<KeyEventQueue> = Mutex::new(KeyEventQueue::new());

/// Hardware Keyboard IRQ callback registered on IRQ 1 (vector 33).
pub fn on_keyboard_irq(_irq: u8, _ctx: &mut crate::arch::idt::InterruptContext) {
    let status = unsafe { inb(0x64) };
    if (status & 0x01) != 0 && (status & 0x20) == 0 {
        let scancode = unsafe { inb(0x60) };

        let mut state = KEYBOARD_STATE.lock();
        if let Some(event) = state.process_scancode(scancode) {
            // Preallocated ring: pushing here must not touch the allocator.
            KEY_QUEUE.lock().push(event);
        }
    }
}

/// Initializes the PS/2 keyboard controller.
pub fn init_ps2_keyboard() {
    crate::arch::idt::register_keyboard_callback(on_keyboard_irq);
}

/// Polls for the next available keyboard event from the queue.
pub fn poll_key_event() -> Option<KeyEvent> {
    // KEYBOARD_STATE and KEY_QUEUE are shared with `on_keyboard_irq`. Without the
    // guard, an IRQ 1 landing inside this function deadlocks against it.
    let _guard = crate::arch::InterruptGuard::acquire();
    let status = unsafe { inb(0x64) };
    if (status & 0x01) != 0 && (status & 0x20) == 0 {
        let scancode = unsafe { inb(0x60) };
        let mut state = KEYBOARD_STATE.lock();
        if let Some(event) = state.process_scancode(scancode) {
            KEY_QUEUE.lock().push(event);
        }
    }

    KEY_QUEUE.lock().pop()
}
