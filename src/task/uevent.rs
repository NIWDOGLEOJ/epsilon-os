//! Input Event Delivery to Ring 3 Processes
//!
//! A Ring 0 app is handed a `KeyEvent` by the compositor loop. A Ring 3 app has
//! to ask for one, so events destined for it are parked here and collected by
//! `SYS_POLL_EVENT`.
//!
//! The queue is a fixed-capacity ring rather than a `VecDeque` for the reason
//! `PROJECT.md` gives: a growable collection allocates on push, and this is
//! written from the compositor loop while syscall context reads it with
//! interrupts masked. A preallocated ring cannot allocate, so it cannot deadlock
//! against a task interrupted inside the allocator.

use spin::Mutex;

use crate::arch::InterruptGuard;
use crate::drivers::ps2_keyboard::{KeyCode, KeyEvent};

const QUEUE_CAPACITY: usize = 64;

/// Event type tags, mirrored in `userspace/src/sys.rs`.
pub const EVENT_NONE: u64 = 0;
pub const EVENT_KEY: u64 = 1;
pub const EVENT_MOUSE: u64 = 2;

/// Mouse actions carried in an `EVENT_MOUSE`.
pub const MOUSE_MOVE: u8 = 0;
pub const MOUSE_DOWN: u8 = 1;
pub const MOUSE_UP: u8 = 2;

/// Mouse buttons.
pub const BUTTON_LEFT: u8 = 0;
pub const BUTTON_RIGHT: u8 = 1;
pub const BUTTON_MIDDLE: u8 = 2;

/// Special key codes as delivered to userspace. Printable keys arrive as their
/// ASCII byte; these occupy a range ASCII does not use.
pub const UKEY_ENTER: u16 = 0x100;
pub const UKEY_BACKSPACE: u16 = 0x101;
pub const UKEY_TAB: u16 = 0x102;
pub const UKEY_UP: u16 = 0x103;
pub const UKEY_DOWN: u16 = 0x104;
pub const UKEY_LEFT: u16 = 0x105;
pub const UKEY_RIGHT: u16 = 0x106;
pub const UKEY_ESCAPE: u16 = 0x107;

/// Packs a key event into one `u64` so `SYS_POLL_EVENT` can return it in `rax`
/// without needing a user buffer to write through.
///
/// ```text
///  bits 63..56  event type (1 = key)
///  bits 55..40  key code (ASCII byte, or UKEY_* for special keys)
///  bits  39..8  reserved
///  bit       2  alt
///  bit       1  ctrl
///  bit       0  shift
/// ```
/// Packs a mouse event. Coordinates are client-relative, so a process never
/// learns where its window sits on screen and cannot use the pointer to probe
/// the rest of the desktop.
///
/// ```text
///  bits 63..56  event type (2 = mouse)
///  bits 55..40  x within the client area
///  bits 39..24  y within the client area
///  bits 23..16  button (0 left, 1 right, 2 middle)
///  bits  15..8  action (0 move, 1 down, 2 up)
/// ```
fn pack_mouse(x: u16, y: u16, button: u8, action: u8) -> u64 {
    (EVENT_MOUSE << 56)
        | ((x as u64) << 40)
        | ((y as u64) << 24)
        | ((button as u64) << 16)
        | ((action as u64) << 8)
}

fn pack_key(event: &KeyEvent) -> u64 {
    let code: u16 = match event.code {
        KeyCode::Printable(byte) => byte as u16,
        KeyCode::Enter => UKEY_ENTER,
        KeyCode::Backspace => UKEY_BACKSPACE,
        KeyCode::Tab => UKEY_TAB,
        KeyCode::Up => UKEY_UP,
        KeyCode::Down => UKEY_DOWN,
        KeyCode::Left => UKEY_LEFT,
        KeyCode::Right => UKEY_RIGHT,
        KeyCode::Escape => UKEY_ESCAPE,
        _ => match event.char_byte {
            Some(byte) => byte as u16,
            None => return EVENT_NONE,
        },
    };

    let mut modifiers = 0u64;
    if event.shift {
        modifiers |= 1 << 0;
    }
    if event.ctrl {
        modifiers |= 1 << 1;
    }
    if event.alt {
        modifiers |= 1 << 2;
    }

    (EVENT_KEY << 56) | ((code as u64) << 40) | modifiers
}

struct EventQueue {
    slots: [u64; QUEUE_CAPACITY],
    head: usize,
    len: usize,
    /// PID currently receiving events, if any.
    target: Option<u64>,
}

impl EventQueue {
    const fn new() -> Self {
        Self { slots: [0; QUEUE_CAPACITY], head: 0, len: 0, target: None }
    }

    fn push(&mut self, packed: u64) {
        if self.len == QUEUE_CAPACITY {
            // Drop the oldest. A user process that stops polling must not be
            // able to stall the compositor loop by filling the queue.
            self.head = (self.head + 1) % QUEUE_CAPACITY;
            self.len -= 1;
        }
        let tail = (self.head + self.len) % QUEUE_CAPACITY;
        self.slots[tail] = packed;
        self.len += 1;
    }

    fn pop(&mut self) -> u64 {
        if self.len == 0 {
            return EVENT_NONE;
        }
        let value = self.slots[self.head];
        self.head = (self.head + 1) % QUEUE_CAPACITY;
        self.len -= 1;
        value
    }
}

static QUEUE: Mutex<EventQueue> = Mutex::new(EventQueue::new());

/// Directs subsequent input to `pid`, clearing anything already queued so a new
/// owner does not inherit the previous one's keystrokes.
pub fn set_target(pid: Option<u64>) {
    let _guard = InterruptGuard::acquire();
    let mut queue = QUEUE.lock();
    if queue.target != pid {
        queue.head = 0;
        queue.len = 0;
        queue.target = pid;
    }
}

pub fn target() -> Option<u64> {
    let _guard = InterruptGuard::acquire();
    QUEUE.lock().target
}

/// Queues a key event for the current target. Called from the compositor loop
/// when the focused window belongs to a Ring 3 process.
pub fn post_key(event: &KeyEvent) {
    if !event.pressed {
        return;
    }
    let packed = pack_key(event);
    if packed == EVENT_NONE {
        return;
    }

    let _guard = InterruptGuard::acquire();
    let mut queue = QUEUE.lock();
    if queue.target.is_some() {
        queue.push(packed);
    }
}

/// Queues a mouse event for the current target.
///
/// `x` and `y` must already be client-relative; the caller does the translation
/// because only the compositor knows where the window is. Coordinates outside
/// the surface are dropped rather than clamped, so a process never sees a click
/// it could not have received.
pub fn post_mouse(x: i32, y: i32, button: u8, action: u8) {
    use crate::gui::surface::{SURFACE_HEIGHT, SURFACE_WIDTH};

    if x < 0 || y < 0 || x as usize >= SURFACE_WIDTH || y as usize >= SURFACE_HEIGHT {
        return;
    }

    let packed = pack_mouse(x as u16, y as u16, button, action);

    let _guard = InterruptGuard::acquire();
    let mut queue = QUEUE.lock();
    if queue.target.is_none() {
        return;
    }

    // Motion coalescing. The PS/2 stream produces far more moves than a process
    // redrawing at frame rate can consume, and a full queue of stale positions
    // would push out the button events that follow them. Replacing a trailing
    // move keeps the newest position without displacing anything else.
    if action == MOUSE_MOVE && queue.len > 0 {
        let tail = (queue.head + queue.len - 1) % QUEUE_CAPACITY;
        let previous = queue.slots[tail];
        let is_move = (previous >> 56) == EVENT_MOUSE && ((previous >> 8) & 0xFF) as u8 == MOUSE_MOVE;
        if is_move {
            queue.slots[tail] = packed;
            return;
        }
    }

    queue.push(packed);
}

/// Collects the next event for `pid`, or `EVENT_NONE`.
pub fn poll(pid: u64) -> u64 {
    let _guard = InterruptGuard::acquire();
    let mut queue = QUEUE.lock();
    if queue.target != Some(pid) {
        return EVENT_NONE;
    }
    queue.pop()
}
