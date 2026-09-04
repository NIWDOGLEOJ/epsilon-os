//! Syscall shims for AegisOS userspace.
//!
//! Thin wrappers over the ABI in `docs/SYSCALL_ABI.md`. `rcx` and `r11` are
//! clobbered by the `syscall` instruction itself and are declared as such on
//! every one of these.

#![allow(dead_code)]

pub const SYS_EXIT: u64 = 0;
pub const SYS_WRITE: u64 = 1;
pub const SYS_YIELD: u64 = 2;
pub const SYS_GET_PID: u64 = 3;
pub const SYS_UPTIME: u64 = 4;

#[inline(always)]
pub unsafe fn syscall0(n: u64) -> u64 {
    let ret: u64;
    core::arch::asm!(
        "syscall",
        inlateout("rax") n => ret,
        lateout("rcx") _, lateout("r11") _,
        options(nostack)
    );
    ret
}

#[inline(always)]
pub unsafe fn syscall1(n: u64, a1: u64) -> u64 {
    let ret: u64;
    core::arch::asm!(
        "syscall",
        inlateout("rax") n => ret,
        in("rdi") a1,
        lateout("rcx") _, lateout("r11") _,
        options(nostack)
    );
    ret
}

#[inline(always)]
pub unsafe fn syscall3(n: u64, a1: u64, a2: u64, a3: u64) -> u64 {
    let ret: u64;
    core::arch::asm!(
        "syscall",
        inlateout("rax") n => ret,
        in("rdi") a1, in("rsi") a2, in("rdx") a3,
        lateout("rcx") _, lateout("r11") _,
        options(nostack)
    );
    ret
}

pub fn write_str(s: &str) -> i64 {
    unsafe { syscall3(SYS_WRITE, 1, s.as_ptr() as u64, s.len() as u64) as i64 }
}

pub fn exit(code: i64) -> ! {
    unsafe { syscall1(SYS_EXIT, code as u64) };
    // SYS_EXIT does not return. If it somehow did, fault rather than run on.
    unsafe { core::arch::asm!("ud2", options(noreturn)) }
}

pub fn sched_yield() {
    unsafe { syscall0(SYS_YIELD) };
}

pub fn getpid() -> u64 {
    unsafe { syscall0(SYS_GET_PID) }
}

pub fn uptime() -> u64 {
    unsafe { syscall0(SYS_UPTIME) }
}

// -----------------------------------------------------------------------------
// Window surface, process and filesystem services
// -----------------------------------------------------------------------------

pub const SYS_SURFACE_MAP: u64 = 5;
pub const SYS_SURFACE_DIMS: u64 = 6;
pub const SYS_POLL_EVENT: u64 = 7;
pub const SYS_PROC_COUNT: u64 = 8;
pub const SYS_PROC_INFO: u64 = 9;
pub const SYS_MEM_STATS: u64 = 10;
pub const SYS_KILL: u64 = 11;
pub const SYS_FS_COUNT: u64 = 12;
pub const SYS_FS_NAME: u64 = 13;
pub const SYS_FS_READ: u64 = 14;
pub const SYS_BEEP: u64 = 15;

/// Event tags and special key codes, mirroring `src/task/uevent.rs`.
pub const EVENT_NONE: u64 = 0;
pub const EVENT_KEY: u64 = 1;
pub const EVENT_MOUSE: u64 = 2;

pub const MOUSE_MOVE: u8 = 0;
pub const MOUSE_DOWN: u8 = 1;
pub const MOUSE_UP: u8 = 2;

pub const BUTTON_LEFT: u8 = 0;
pub const BUTTON_RIGHT: u8 = 1;
pub const BUTTON_MIDDLE: u8 = 2;

pub const UKEY_ENTER: u16 = 0x100;
pub const UKEY_BACKSPACE: u16 = 0x101;
pub const UKEY_TAB: u16 = 0x102;
pub const UKEY_UP: u16 = 0x103;
pub const UKEY_DOWN: u16 = 0x104;
pub const UKEY_LEFT: u16 = 0x105;
pub const UKEY_RIGHT: u16 = 0x106;
pub const UKEY_ESCAPE: u16 = 0x107;

#[inline(always)]
pub unsafe fn syscall2(n: u64, a1: u64, a2: u64) -> u64 {
    let ret: u64;
    core::arch::asm!(
        "syscall",
        inlateout("rax") n => ret,
        in("rdi") a1, in("rsi") a2,
        lateout("rcx") _, lateout("r11") _,
        options(nostack)
    );
    ret
}

#[inline(always)]
pub unsafe fn syscall4(n: u64, a1: u64, a2: u64, a3: u64, a4: u64) -> u64 {
    let ret: u64;
    core::arch::asm!(
        "syscall",
        inlateout("rax") n => ret,
        in("rdi") a1, in("rsi") a2, in("rdx") a3, in("r10") a4,
        lateout("rcx") _, lateout("r11") _,
        options(nostack)
    );
    ret
}

pub struct KeyPress {
    pub code: u16,
    pub shift: bool,
    pub ctrl: bool,
}

/// A pointer event. Coordinates are relative to this window's client area, so
/// the process never learns where its window sits on screen.
pub struct MouseMove {
    pub x: u16,
    pub y: u16,
    pub button: u8,
    pub action: u8,
}

pub enum Event {
    Key(KeyPress),
    Mouse(MouseMove),
}

/// Collects the next input event, or `None` if the queue is empty.
pub fn poll_event() -> Option<Event> {
    let packed = unsafe { syscall0(SYS_POLL_EVENT) };
    match packed >> 56 {
        EVENT_KEY => Some(Event::Key(KeyPress {
            code: ((packed >> 40) & 0xFFFF) as u16,
            shift: packed & (1 << 0) != 0,
            ctrl: packed & (1 << 1) != 0,
        })),
        EVENT_MOUSE => Some(Event::Mouse(MouseMove {
            x: ((packed >> 40) & 0xFFFF) as u16,
            y: ((packed >> 24) & 0xFFFF) as u16,
            button: ((packed >> 16) & 0xFF) as u8,
            action: ((packed >> 8) & 0xFF) as u8,
        })),
        _ => None,
    }
}

/// Maps this process's window surface. Returns `(base, width, height)`.
pub fn surface_map() -> Option<(*mut u32, usize, usize)> {
    let addr = unsafe { syscall0(SYS_SURFACE_MAP) };
    if (addr as i64) < 0 || addr == 0 {
        return None;
    }
    let dims = unsafe { syscall0(SYS_SURFACE_DIMS) };
    Some((addr as *mut u32, (dims >> 32) as usize, (dims & 0xFFFF_FFFF) as usize))
}

pub fn proc_count() -> u64 {
    unsafe { syscall0(SYS_PROC_COUNT) }
}

/// Fills `buf` with `"<pid> <state> <cpu%> <name>"`. Returns the byte count.
pub fn proc_info(index: u64, buf: &mut [u8]) -> i64 {
    unsafe { syscall3(SYS_PROC_INFO, index, buf.as_mut_ptr() as u64, buf.len() as u64) as i64 }
}

/// Returns `(used_kib, total_kib)`.
pub fn mem_stats() -> (u64, u64) {
    let packed = unsafe { syscall0(SYS_MEM_STATS) };
    (packed >> 32, packed & 0xFFFF_FFFF)
}

pub fn kill(pid: u64) -> i64 {
    unsafe { syscall1(SYS_KILL, pid) as i64 }
}

pub fn fs_count() -> u64 {
    unsafe { syscall0(SYS_FS_COUNT) }
}

pub fn fs_name(index: u64, buf: &mut [u8]) -> i64 {
    unsafe { syscall3(SYS_FS_NAME, index, buf.as_mut_ptr() as u64, buf.len() as u64) as i64 }
}

pub fn fs_read(path: &str, buf: &mut [u8]) -> i64 {
    unsafe {
        syscall4(
            SYS_FS_READ,
            path.as_ptr() as u64,
            path.len() as u64,
            buf.as_mut_ptr() as u64,
            buf.len() as u64,
        ) as i64
    }
}

pub fn beep(freq_hz: u32, duration_ms: u32) -> i64 {
    unsafe { syscall2(SYS_BEEP, freq_hz as u64, duration_ms as u64) as i64 }
}

pub const SYS_SPAWN_FAULT: u64 = 16;

/// Asks the kernel to spawn a Ring 3 process that faults on purpose.
///
/// Returns the new PID, or a negative error. `kind` selects the fault: 0 null
/// dereference, 1 divide by zero, 2 write into kernel space, 3 invalid opcode.
pub fn spawn_fault(kind: u64) -> i64 {
    unsafe { syscall1(SYS_SPAWN_FAULT, kind) as i64 }
}

pub const SYS_CPU_USAGE: u64 = 17;

/// System-wide CPU utilisation, 0..100.
pub fn cpu_usage() -> u64 {
    unsafe { syscall0(SYS_CPU_USAGE) }
}
