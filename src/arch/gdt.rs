//! Global Descriptor Table (GDT) and Task State Segment (TSS) Subsystem.
//!
//! Enforces hardware privilege levels (Ring 0 Kernel, Ring 3 Userspace)
//! and provides stack switching via TSS RSP0 and IST1 (Double Fault).

use core::mem::size_of;

pub const KERNEL_CODE_SELECTOR: u16 = 0x08;
pub const KERNEL_DATA_SELECTOR: u16 = 0x10;
pub const USER_DATA_SELECTOR:   u16 = 0x18 | 3; // RPL = 3 -> 0x1B
pub const USER_CODE_SELECTOR:   u16 = 0x20 | 3; // RPL = 3 -> 0x23
pub const TSS_SELECTOR:         u16 = 0x28;

/// 64-bit Task State Segment (104 bytes).
#[repr(C, packed)]
pub struct TaskStateSegment {
    pub reserved0: u32,
    pub rsp0: u64,
    pub rsp1: u64,
    pub rsp2: u64,
    pub reserved1: u64,
    pub ist1: u64,
    pub ist2: u64,
    pub ist3: u64,
    pub ist4: u64,
    pub ist5: u64,
    pub ist6: u64,
    pub ist7: u64,
    pub reserved2: u64,
    pub reserved3: u16,
    pub iomap_base: u16,
}

impl TaskStateSegment {
    pub const fn new() -> Self {
        Self {
            reserved0: 0,
            rsp0: 0,
            rsp1: 0,
            rsp2: 0,
            reserved1: 0,
            ist1: 0,
            ist2: 0,
            ist3: 0,
            ist4: 0,
            ist5: 0,
            ist6: 0,
            ist7: 0,
            reserved2: 0,
            reserved3: 0,
            iomap_base: size_of::<TaskStateSegment>() as u16,
        }
    }
}

/// GDT Pointer for `lgdt` instruction.
#[repr(C, packed)]
pub struct Gdtr {
    pub limit: u16,
    pub base: u64,
}

/// 64-bit Global Descriptor Table with 7 entries (including 16-byte TSS).
#[repr(C, align(16))]
pub struct Gdt {
    pub entries: [u64; 7],
}

impl Gdt {
    pub const fn new() -> Self {
        Self {
            entries: [
                0x0000_0000_0000_0000, // 0x00: Null Descriptor
                0x0020_9A00_0000_0000, // 0x08: Kernel Code 64-bit (DPL=0, L=1, Present, Exec, Read)
                0x0000_9200_0000_0000, // 0x10: Kernel Data 64-bit (DPL=0, Present, Writable)
                0x0000_F200_0000_0000, // 0x18: User Data 64-bit (DPL=3, Present, Writable)
                0x0020_FA00_0000_0000, // 0x20: User Code 64-bit (DPL=3, L=1, Present, Exec, Read)
                0x0000_0000_0000_0000, // 0x28: TSS Low (Populated dynamically)
                0x0000_0000_0000_0000, // 0x30: TSS High (Populated dynamically)
            ],
        }
    }

    /// Configures the 16-byte TSS descriptor at entries 5 and 6.
    pub fn set_tss(&mut self, base: u64) {
        let limit = (size_of::<TaskStateSegment>() - 1) as u64;

        // Low 64 bits: Limit[0..15] | Base[0..23] | Type=0x9 (TSS 64 avail), DPL=0, Present=1 | Limit[16..19] | Base[24..31]
        let mut low: u64 = limit & 0xFFFF;
        low |= (base & 0x00FF_FFFF) << 16;
        low |= 0x89u64 << 40; // Present=1, DPL=0, Type=9
        low |= ((limit >> 16) & 0x0F) << 48;
        low |= ((base >> 24) & 0xFF) << 56;

        // High 64 bits: Base[32..63] | Reserved
        let high: u64 = base >> 32;

        self.entries[5] = low;
        self.entries[6] = high;
    }

    /// Constructs the `Gdtr` pointer for `lgdt`.
    pub fn pointer(&self) -> Gdtr {
        Gdtr {
            limit: (size_of::<Gdt>() - 1) as u16,
            base: self as *const _ as u64,
        }
    }
}

// -----------------------------------------------------------------------------
// Static Storage for GDT, TSS and Stacks
// -----------------------------------------------------------------------------

/// Initial 32 KiB Kernel Stack (RSP0 default).
#[repr(align(16))]
pub struct KernelStack(pub [u8; 32768]);

/// Dedicated 16 KiB Double Fault Stack (IST1).
#[repr(align(16))]
pub struct DoubleFaultStack(pub [u8; 16384]);

static mut GDT: Gdt = Gdt::new();
static mut TSS: TaskStateSegment = TaskStateSegment::new();
static mut INITIAL_KERNEL_STACK: KernelStack = KernelStack([0; 32768]);
static mut DOUBLE_FAULT_STACK: DoubleFaultStack = DoubleFaultStack([0; 16384]);

/// Initializes GDT, TSS, loads GDTR, reloads CS/DS/ES/SS, and executes `ltr`.
///
/// Returns tuple `(kernel_cs, kernel_ds, user_cs, user_ds, tss_sel)`.
pub fn init_gdt_tss() -> (u16, u16, u16, u16, u16) {
    unsafe {
        let tss_ptr = &raw mut TSS;
        let gdt_ptr = &raw mut GDT;

        // Set up IST1 for Double Fault
        let df_stack_top = (&raw const DOUBLE_FAULT_STACK as *const u8 as u64) + 16384;
        (*tss_ptr).ist1 = df_stack_top;

        // Set up initial RSP0
        let kstack_top = (&raw const INITIAL_KERNEL_STACK as *const u8 as u64) + 32768;
        (*tss_ptr).rsp0 = kstack_top;

        // Link TSS descriptor into GDT
        (*gdt_ptr).set_tss(tss_ptr as u64);

        // Load GDTR
        let gdtr = (*gdt_ptr).pointer();
        core::arch::asm!(
            "lgdt [{0}]",
            in(reg) &gdtr,
            options(readonly, nostack, preserves_flags)
        );

        // Reload data segment registers
        core::arch::asm!(
            "mov ds, ax",
            "mov es, ax",
            "mov ss, ax",
            "mov fs, ax",
            "mov gs, ax",
            in("ax") KERNEL_DATA_SELECTOR,
            options(nostack, preserves_flags)
        );

        // Far return (retfq) to reload CS with 0x08
        core::arch::asm!(
            "push {cs}",
            "lea {tmp}, [2f + rip]",
            "push {tmp}",
            "retfq",
            "2:",
            cs = in(reg) (KERNEL_CODE_SELECTOR as u64),
            tmp = out(reg) _,
            options(preserves_flags)
        );

        // Load Task Register (TR)
        core::arch::asm!(
            "ltr ax",
            in("ax") TSS_SELECTOR,
            options(nostack, preserves_flags)
        );
    }

    (
        KERNEL_CODE_SELECTOR,
        KERNEL_DATA_SELECTOR,
        USER_CODE_SELECTOR,
        USER_DATA_SELECTOR,
        TSS_SELECTOR,
    )
}

/// Updates TSS `RSP0` for the currently active task.
///
/// Called by the scheduler during task context switches to guarantee
/// that hardware privilege transitions land on the task's private kernel stack.
pub fn set_tss_rsp0(stack_top: u64) {
    unsafe {
        let tss_ptr = &raw mut TSS;
        (*tss_ptr).rsp0 = stack_top;
    }
}

/// Updates an IST stack pointer in the TSS (1 <= index <= 7).
pub fn set_tss_ist(index: usize, stack_top: u64) {
    unsafe {
        let tss_ptr = &raw mut TSS;
        match index {
            1 => (*tss_ptr).ist1 = stack_top,
            2 => (*tss_ptr).ist2 = stack_top,
            3 => (*tss_ptr).ist3 = stack_top,
            4 => (*tss_ptr).ist4 = stack_top,
            5 => (*tss_ptr).ist5 = stack_top,
            6 => (*tss_ptr).ist6 = stack_top,
            7 => (*tss_ptr).ist7 = stack_top,
            _ => panic!("Invalid IST index: {}", index),
        }
    }
}
