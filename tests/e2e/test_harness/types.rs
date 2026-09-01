//! AegisOS E2E Test Harness: Shared Types and Data Structures
//!
//! Provides hardware, kernel, GUI, and application definitions matching
//! the exact AegisOS contracts in PROJECT.md.

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PhysAddr(pub u64);

impl PhysAddr {
    #[inline(always)]
    pub const fn new(addr: u64) -> Self {
        Self(addr)
    }

    #[inline(always)]
    pub const fn as_u64(&self) -> u64 {
        self.0
    }

    #[inline(always)]
    pub const fn is_aligned_4k(&self) -> bool {
        (self.0 & 0xFFF) == 0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VirtAddr(pub u64);

impl VirtAddr {
    #[inline(always)]
    pub const fn new(addr: u64) -> Self {
        Self(addr)
    }

    #[inline(always)]
    pub const fn as_u64(&self) -> u64 {
        self.0
    }

    #[inline(always)]
    pub const fn is_canonical(&self) -> bool {
        let sign_extension = (self.0 >> 47) & 0x1FFFF;
        sign_extension == 0 || sign_extension == 0x1FFFF
    }

    #[inline(always)]
    pub const fn is_higher_half(&self) -> bool {
        self.0 >= 0xFFFF_8000_0000_0000
    }

    #[inline(always)]
    pub const fn is_user_lower_half(&self) -> bool {
        self.0 <= 0x0000_7FFF_FFFF_FFFF
    }

    #[inline(always)]
    pub const fn pml4_index(&self) -> usize {
        ((self.0 >> 39) & 0x1FF) as usize
    }

    #[inline(always)]
    pub const fn pdpt_index(&self) -> usize {
        ((self.0 >> 30) & 0x1FF) as usize
    }

    #[inline(always)]
    pub const fn pd_index(&self) -> usize {
        ((self.0 >> 21) & 0x1FF) as usize
    }

    #[inline(always)]
    pub const fn pt_index(&self) -> usize {
        ((self.0 >> 12) & 0x1FF) as usize
    }

    #[inline(always)]
    pub const fn page_offset(&self) -> u64 {
        self.0 & 0xFFF
    }
}

pub const PAGE_SIZE: usize = 4096;
pub const TOTAL_RAM_4GB: u64 = 4 * 1024 * 1024 * 1024; // 4 GB
pub const TOTAL_FRAMES_4GB: usize = (TOTAL_RAM_4GB / PAGE_SIZE as u64) as usize; // 1,048,576 frames
pub const BITMAP_SIZE_BYTES: usize = TOTAL_FRAMES_4GB / 8; // 131,072 bytes (128 KB)
pub const HHDM_OFFSET: u64 = 0xFFFF_8000_0000_0000;
pub const KERNEL_VIRTUAL_BASE: u64 = 0xFFFF_FFFF_8000_0000;
pub const MAX_IDLE_RAM_BYTES: u64 = 60 * 1024 * 1024; // < 60 MB

macro_rules! bitflags_constants {
    ($(pub const $name:ident: $t:ty = $val:expr;)*) => {
        $(pub const $name: $t = $val;)*
    };
}

bitflags_constants! {
    pub const PTE_PRESENT: u64 = 1 << 0;
    pub const PTE_WRITABLE: u64 = 1 << 1;
    pub const PTE_USER: u64 = 1 << 2;
    pub const PTE_WRITE_THROUGH: u64 = 1 << 3;
    pub const PTE_NO_CACHE: u64 = 1 << 4;
    pub const PTE_ACCESSED: u64 = 1 << 5;
    pub const PTE_DIRTY: u64 = 1 << 6;
    pub const PTE_HUGE_PAGE: u64 = 1 << 7;
    pub const PTE_GLOBAL: u64 = 1 << 8;
    pub const PTE_NO_EXECUTE: u64 = 1 << 63;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivilegeLevel {
    Ring0Kernel,
    Ring3Userspace,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InterruptStackFrame {
    pub rip: u64,
    pub cs: u64,
    pub rflags: u64,
    pub rsp: u64,
    pub ss: u64,
}

impl InterruptStackFrame {
    #[inline(always)]
    pub fn is_user_mode(&self) -> bool {
        (self.cs & 0x03) == 0x03
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExceptionVector {
    DivideByZero = 0,
    Debug = 1,
    NonMaskableInterrupt = 2,
    Breakpoint = 3,
    Overflow = 4,
    BoundRangeExceeded = 5,
    InvalidOpcode = 6,
    DeviceNotAvailable = 7,
    DoubleFault = 8,
    InvalidTss = 10,
    SegmentNotPresent = 11,
    StackSegmentFault = 12,
    GeneralProtectionFault = 13,
    PageFault = 14,
    X87FloatingPoint = 16,
    AlignmentCheck = 17,
    MachineCheck = 18,
    SimdFloatingPoint = 19,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageFaultErrorCode {
    pub present: bool,
    pub write: bool,
    pub user: bool,
    pub reserved_write: bool,
    pub instruction_fetch: bool,
}

impl PageFaultErrorCode {
    pub fn from_raw(raw: u64) -> Self {
        Self {
            present: (raw & (1 << 0)) != 0,
            write: (raw & (1 << 1)) != 0,
            user: (raw & (1 << 2)) != 0,
            reserved_write: (raw & (1 << 3)) != 0,
            instruction_fetch: (raw & (1 << 4)) != 0,
        }
    }

    pub fn to_raw(&self) -> u64 {
        let mut raw = 0;
        if self.present { raw |= 1 << 0; }
        if self.write { raw |= 1 << 1; }
        if self.user { raw |= 1 << 2; }
        if self.reserved_write { raw |= 1 << 3; }
        if self.instruction_fetch { raw |= 1 << 4; }
        raw
    }
}

pub type ProcessId = u32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessState {
    Ready,
    Running,
    Blocked,
    Zombie,
    Terminated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Priority {
    Low = 0,
    Normal = 1,
    High = 2,
    Realtime = 3,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessInfo {
    pub pid: ProcessId,
    pub name: String,
    pub state: ProcessState,
    pub priority: Priority,
    pub memory_bytes: usize,
    pub cpu_percent: u32,
    pub is_user: bool,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct CpuRegisters {
    pub rax: u64,
    pub rbx: u64,
    pub rcx: u64,
    pub rdx: u64,
    pub rsi: u64,
    pub rdi: u64,
    pub rbp: u64,
    pub rsp: u64,
    pub r8: u64,
    pub r9: u64,
    pub r10: u64,
    pub r11: u64,
    pub r12: u64,
    pub r13: u64,
    pub r14: u64,
    pub r15: u64,
    pub rip: u64,
    pub rflags: u64,
    pub cr3: u64,
}

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
    pub fn to_u32(&self) -> u32 {
        ((self.a as u32) << 24) | ((self.r as u32) << 16) | ((self.g as u32) << 8) | (self.b as u32)
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: usize,
    pub height: usize,
}

impl Rect {
    pub fn new(x: i32, y: i32, width: usize, height: usize) -> Self {
        Self { x, y, width, height }
    }

    pub fn contains(&self, px: i32, py: i32) -> bool {
        px >= self.x
            && px < self.x + self.width as i32
            && py >= self.y
            && py < self.y + self.height as i32
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AppId {
    CrashTest,
    ActivityMonitor,
    Terminal,
    AegisPad,
    AboutDialog,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputEvent {
    KeyDown { key: u8, scancode: u8, shift: bool, ctrl: bool },
    KeyUp { scancode: u8 },
    MouseMove { x: i32, y: i32, dx: i32, dy: i32 },
    MouseDown { button: MouseButton, x: i32, y: i32 },
    MouseUp { button: MouseButton, x: i32, y: i32 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryStats {
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub free_bytes: u64,
    pub allocated_frames: usize,
    pub total_frames: usize,
    pub heap_used_bytes: usize,
    pub heap_total_bytes: usize,
}
