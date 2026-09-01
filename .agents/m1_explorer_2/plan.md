# Milestone 1 Architectural Plan & Code Blueprints: GDT, TSS & IDT Subsystem

**Author:** M1 GDT, TSS & IDT Explorer (`m1_explorer_2`)  
**Target Architecture:** x86_64 Long Mode (`no_std` Rust)  
**Target Files:** `src/arch/gdt.rs`, `src/arch/idt.rs`  
**Milestone:** M1 (Bare-Metal Foundation, Memory Subsystem & Architecture)  
**Date:** 2026-08-30  

---

## 1. Executive Summary & Architectural Overview

In AegisOS, the Global Descriptor Table (GDT), Task State Segment (TSS), and Interrupt Descriptor Table (IDT) form the hardware-enforced foundation of privilege separation and fault isolation:

1. **GDT (Global Descriptor Table)**: Defines 64-bit segment descriptors establishing Ring 0 (Kernel Code/Data) and Ring 3 (User Code/Data) privilege boundaries. It also hosts the 16-byte TSS descriptor.
2. **TSS (Task State Segment)**: Hardware mechanism in x86_64 responsible for:
   - **`RSP0` Privilege Transition Stack**: When a user process executing in Ring 3 ($CPL=3$) triggers an interrupt or exception, the CPU automatically switches to the kernel stack address stored in `TSS.RSP0`.
   - **`IST1` (Interrupt Stack Table 1)**: Dedicated stack for Double Faults (`#DF`, vector 8) to prevent catastrophic triple-fault machine resets if a stack overflow occurs in kernel space.
3. **IDT (Interrupt Descriptor Table)**: 256-entry gate table mapping CPU exceptions (0..31) and hardware IRQs (32..255) to naked assembly entry stubs.
4. **Ring 0 vs Ring 3 Hardware Privilege Hook**: When an exception occurs, the CPU pushes the saved Code Segment (`CS`) onto the stack. The exception dispatcher inspects `(ctx.cs & 0x03) == 3`:
   - If **Ring 3**: Faulting user process is logged, marked for termination, and reaped by the scheduler. **Zero kernel panic, zero desktop freeze.**
   - If **Ring 0**: Kernel-level bug detected, triggers fatal diagnostic kernel panic with full CPU register dump.

---

## 2. GDT & TSS Architecture (`src/arch/gdt.rs`)

### 2.1 Segment Selectors & Descriptor Layout

In x86_64 Long Mode, segmentation is flattened (base=0, limit=unbounded for CS/DS/SS/ES), but segment descriptors remain mandatory for:
- Configuring 64-bit Long Mode execution (`L=1, D/B=0`).
- Setting Privilege Levels (`DPL=0` vs `DPL=3`).
- Providing the 16-byte Task State Segment (TSS) descriptor.

#### GDT Selector Table

| Index | Selector (Offset) | RPL | Name | DPL | Attributes / Type | Raw Value (`u64`) |
|---|---|---|---|---|---|---|
| **0** | `0x00` | 0 | Null Descriptor | 0 | Unused | `0x0000_0000_0000_0000` |
| **1** | `0x08` | 0 | Kernel Code 64-bit | 0 | Present, Exec, Read, L=1 | `0x0020_9A00_0000_0000` |
| **2** | `0x10` | 0 | Kernel Data 64-bit | 0 | Present, Writable | `0x0000_9200_0000_0000` |
| **3** | `0x18` (`0x1B`) | 3 | User Data 64-bit | 3 | Present, Writable, DPL=3 | `0x0000_F200_0000_0000` |
| **4** | `0x20` (`0x23`) | 3 | User Code 64-bit | 3 | Present, Exec, Read, DPL=3, L=1 | `0x0020_FA00_0000_0000` |
| **5** | `0x28` | 0 | TSS Descriptor (Low) | 0 | Present, Type=0x9 (TSS 64 Avail) | Formatted dynamically |
| **6** | `0x30` | 0 | TSS Descriptor (High)| 0 | Upper 32-bit Base Address | Formatted dynamically |

*Note on RPL:* When returning to or running in Ring 3, the selector value passed to `iretq` or segment registers must include Requestor Privilege Level 3 (`RPL=3`):
- `USER_DATA_SELECTOR = 0x18 | 3 = 0x1B`
- `USER_CODE_SELECTOR = 0x20 | 3 = 0x23`

### 2.2 Task State Segment (TSS) Structure (104 Bytes)

```rust
#[repr(C, packed)]
pub struct TaskStateSegment {
    pub reserved0: u32,
    pub rsp0: u64,       // Stack pointer for Ring 0 transitions
    pub rsp1: u64,
    pub rsp2: u64,
    pub reserved1: u64,
    pub ist1: u64,       // IST1: Double Fault (#DF) Stack
    pub ist2: u64,       // IST2
    pub ist3: u64,       // IST3
    pub ist4: u64,       // IST4
    pub ist5: u64,       // IST5
    pub ist6: u64,       // IST6
    pub ist7: u64,       // IST7
    pub reserved2: u64,
    pub reserved3: u16,
    pub iomap_base: u16, // Offset to I/O Permission Bitmap (104 = none)
}
```

### 2.3 GDT Reload & Segment Register Reload Sequence

1. `lgdt [gdtr]`: Loads GDTR register with limit (`sizeof(GDT) - 1 = 55`) and base virtual address.
2. Data Segment Reload: Loads `DS`, `ES`, `SS`, `FS`, `GS` with `0x10` (Kernel Data Selector).
3. Code Segment Reload (`retfq`): `CS` cannot be loaded via `mov cs, ax`. We push `0x08` and the target return address onto the stack, then issue `retfq` (64-bit far return), which pops both `RIP` and `CS`.
4. Task Register Reload (`ltr`): Issues `ltr ax` with `ax = 0x28` (TSS Selector).

---

### 2.4 Complete Code Blueprint: `src/arch/gdt.rs`

```rust
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
    pub fn set_tss(&mut self, tss: &'static TaskStateSegment) {
        let base = tss as *const _ as u64;
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
    pub fn pointer(&'static self) -> Gdtr {
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
struct KernelStack([u8; 32768]);

/// Dedicated 16 KiB Double Fault Stack (IST1).
#[repr(align(16))]
struct DoubleFaultStack([u8; 16384]);

static mut GDT: Gdt = Gdt::new();
static mut TSS: TaskStateSegment = TaskStateSegment::new();
static mut INITIAL_KERNEL_STACK: KernelStack = KernelStack([0; 32768]);
static mut DOUBLE_FAULT_STACK: DoubleFaultStack = DoubleFaultStack([0; 16384]);

/// Initializes GDT, TSS, loads GDTR, reloads CS/DS/ES/SS, and executes `ltr`.
///
/// Returns tuple `(kernel_cs, kernel_ds, user_cs, user_ds, tss_sel)`.
pub fn init_gdt_tss() -> (u16, u16, u16, u16, u16) {
    unsafe {
        // Set up IST1 for Double Fault
        let df_stack_top = (&raw const DOUBLE_FAULT_STACK as *const u8 as u64) + 16384;
        TSS.ist1 = df_stack_top;

        // Set up initial RSP0
        let kstack_top = (&raw const INITIAL_KERNEL_STACK as *const u8 as u64) + 32768;
        TSS.rsp0 = kstack_top;

        // Link TSS descriptor into GDT
        GDT.set_tss(&TSS);

        // Load GDTR
        let gdtr = GDT.pointer();
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
            "lea {tmp}, [1f + rip]",
            "push {tmp}",
            "retfq",
            "1:",
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
        TSS.rsp0 = stack_top;
    }
}

/// Updates an IST stack pointer in the TSS (1 <= index <= 7).
pub fn set_tss_ist(index: usize, stack_top: u64) {
    unsafe {
        match index {
            1 => TSS.ist1 = stack_top,
            2 => TSS.ist2 = stack_top,
            3 => TSS.ist3 = stack_top,
            4 => TSS.ist4 = stack_top,
            5 => TSS.ist5 = stack_top,
            6 => TSS.ist6 = stack_top,
            7 => TSS.ist7 = stack_top,
            _ => panic!("Invalid IST index: {}", index),
        }
    }
}
```

---

## 3. IDT & Exception Dispatcher Architecture (`src/arch/idt.rs`)

### 3.1 IDT Gate Descriptor Layout (16 Bytes)

```rust
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct IdtEntry {
    pub offset_low: u16,   // Bits 0..15 of ISR entry address
    pub selector: u16,     // Kernel Code Segment Selector (0x08)
    pub ist: u8,           // Bits 0..2: IST index (0 = RSP0, 1 = IST1 Double Fault); Bits 3..7 = 0
    pub type_attr: u8,     // 0x8E = Interrupt Gate (Present=1, DPL=0, Type=0xE)
    pub offset_mid: u16,   // Bits 16..31 of ISR entry address
    pub offset_high: u32,  // Bits 32..63 of ISR entry address
    pub reserved: u32,     // Must be 0
}
```

### 3.2 CPU Exception Vectors & Error Code Invariants

| Vector | Mnemonic | Exception Description | Type | Pushes Error Code? | Stack Used |
|---|---|---|---|---|---|
| **0** | `#DE` | Divide-by-Zero Error | Fault | **No** (Stub pushes 0) | `RSP0` |
| **1** | `#DB` | Debug Exception | Fault/Trap | **No** (Stub pushes 0) | `RSP0` |
| **2** | `NMI` | Non-Maskable Interrupt | Interrupt | **No** (Stub pushes 0) | `RSP0` |
| **3** | `#BP` | Breakpoint (`int3`) | Trap | **No** (Stub pushes 0) | `RSP0` |
| **4** | `#OF` | Overflow (`into`) | Trap | **No** (Stub pushes 0) | `RSP0` |
| **5** | `#BR` | Bound Range Exceeded | Fault | **No** (Stub pushes 0) | `RSP0` |
| **6** | `#UD` | Invalid Opcode | Fault | **No** (Stub pushes 0) | `RSP0` |
| **7** | `#NM` | Device Not Available (FPU) | Fault | **No** (Stub pushes 0) | `RSP0` |
| **8** | `#DF` | Double Fault | Abort | **Yes** (CPU pushes Code)| **`IST1`** |
| **9** | - | Coprocessor Segment Overrun | Fault | **No** (Stub pushes 0) | `RSP0` |
| **10** | `#TS` | Invalid TSS | Fault | **Yes** (CPU pushes Code)| `RSP0` |
| **11** | `#NP` | Segment Not Present | Fault | **Yes** (CPU pushes Code)| `RSP0` |
| **12** | `#SS` | Stack-Segment Fault | Fault | **Yes** (CPU pushes Code)| `RSP0` |
| **13** | `#GP` | General Protection Fault | Fault | **Yes** (CPU pushes Code)| `RSP0` |
| **14** | `#PF` | Page Fault (`CR2` holds addr)| Fault | **Yes** (CPU pushes Code)| `RSP0` |
| **15** | - | Reserved | - | **No** (Stub pushes 0) | `RSP0` |
| **16** | `#MF` | x87 FPU Floating-Point Error| Fault | **No** (Stub pushes 0) | `RSP0` |
| **17** | `#AC` | Alignment Check | Fault | **Yes** (CPU pushes Code)| `RSP0` |
| **18** | `#MC` | Machine Check | Abort | **No** (Stub pushes 0) | `RSP0` |
| **19** | `#XM` | SIMD Floating-Point Exception| Fault | **No** (Stub pushes 0) | `RSP0` |
| **20** | `#VE` | Virtualization Exception | Fault | **No** (Stub pushes 0) | `RSP0` |
| **21** | `#CP` | Control Protection Exception| Fault | **Yes** (CPU pushes Code)| `RSP0` |
| **22..28**| - | Reserved | - | **No** (Stub pushes 0) | `RSP0` |
| **29** | `#VC` | VMM Communication Exception | Fault | **Yes** (CPU pushes Code)| `RSP0` |
| **30** | `#SX` | Security Exception | Fault | **Yes** (CPU pushes Code)| `RSP0` |
| **31** | - | Reserved | - | **No** (Stub pushes 0) | `RSP0` |
| **32** | `IRQ0`| Programmable Interval Timer | IRQ | **No** (Stub pushes 0) | `RSP0` |
| **33** | `IRQ1`| PS/2 Keyboard | IRQ | **No** (Stub pushes 0) | `RSP0` |
| **44** | `IRQ12`| PS/2 Mouse | IRQ | **No** (Stub pushes 0) | `RSP0` |
| **32..255**| `IRQ` | Hardware / Software IRQs | IRQ/Trap| **No** (Stub pushes 0) | `RSP0` |

---

### 3.3 Stack Layout & `InterruptContext` (176 Bytes)

When an interrupt occurs:
1. Hardware pushes `SS`, `RSP`, `RFLAGS`, `CS`, `RIP` (5 quadwords = 40 bytes).
2. For vectors without error code, the stub pushes `0` (dummy error code, 8 bytes).
3. The stub pushes the vector number (8 bytes).
4. `isr_common_stub` pushes 15 general-purpose registers (15 quadwords = 120 bytes).

```
Stack Growth (High to Low):
+-------------------------+
| SS                      | +168 (pushed by CPU)
| RSP                     | +160 (pushed by CPU)
| RFLAGS                  | +152 (pushed by CPU)
| CS                      | +144 (pushed by CPU)
| RIP                     | +136 (pushed by CPU)
+-------------------------+
| Error Code (or dummy 0) | +128 (pushed by CPU or stub)
| Vector Number           | +120 (pushed by stub)
+-------------------------+
| RAX                     | +112 (pushed by isr_common_stub)
| RBX                     | +104
| RCX                     | +96
| RDX                     | +88
| RBP                     | +80
| RSI                     | +72
| RDI                     | +64
| R8                      | +56
| R9                      | +48
| R10                     | +40
| R11                     | +32
| R12                     | +24
| R13                     | +16
| R14                     | +8
| R15                     | +0  <-- RSP passed as &mut InterruptContext to Rust
+-------------------------+
Total Frame Size: 176 Bytes (176 % 16 == 0 -> 16-byte aligned for System V ABI!)
```

```rust
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct InterruptContext {
    pub r15: u64,
    pub r14: u64,
    pub r13: u64,
    pub r12: u64,
    pub r11: u64,
    pub r10: u64,
    pub r9:  u64,
    pub r8:  u64,
    pub rdi: u64,
    pub rsi: u64,
    pub rbp: u64,
    pub rdx: u64,
    pub rcx: u64,
    pub rbx: u64,
    pub rax: u64,

    pub vector: u64,
    pub error_code: u64,

    pub rip: u64,
    pub cs: u64,
    pub rflags: u64,
    pub rsp: u64,
    pub ss: u64,
}
```

---

### 3.4 Ring 0 vs Ring 3 Fault Isolation Hook (R2 Acceptance Criteria)

Inside `rust_interrupt_handler`:
```rust
let is_user = (ctx.cs & 0x03) == 3;
```

#### Fault Decision Matrix:
1. **Ring 3 Fault (`is_user == true`)**:
   - Offending process triggered #DE (div by zero), #UD (invalid opcode), #GP (segment/protection), or #PF (null pointer / out of bounds).
   - Read `CR2` if `#PF`.
   - Log serial message:
     ```text
     [FAULT-ISOLATION] Userspace Fault (Vector {vector}, {name}) at RIP=0x{rip:016x}, Error=0x{error:x}, CR2=0x{cr2:016x}
     ```
   - Invoke registered fault handler or M2 task reaper (`crate::task::fault::handle_user_fault(ctx)`). Offending task is marked `Dead`, its frames are reclaimed, and scheduler advances to next ready task.
   - **Kernel and Desktop GUI continue running without interruption.**
2. **Ring 0 Fault (`is_user == false`)**:
   - Kernel bug detected!
   - Print full diagnostic panic log to serial and halt CPU (`cli; hlt`).

---

### 3.5 8259 PIC Driver & IRQ Management

The legacy dual 8259 PIC (Master `0x20`/`0x21`, Slave `0xA0`/`0xA1`) maps:
- Master IRQs 0..7 -> Vectors `32..39` (`0x20..0x27`).
- Slave IRQs 8..15 -> Vectors `40..47` (`0x28..0x2F`).

```rust
const PIC1_COMMAND: u16 = 0x20;
const PIC1_DATA:    u16 = 0x21;
const PIC2_COMMAND: u16 = 0xA0;
const PIC2_DATA:    u16 = 0xA1;

const PIC_EOI:      u8  = 0x20;
```

When an IRQ finishes, an End-Of-Interrupt (`EOI`) byte `0x20` must be issued:
- If `irq >= 8`: write `0x20` to `PIC2_COMMAND (0xA0)`.
- Always: write `0x20` to `PIC1_COMMAND (0x20)`.

---

### 3.6 Complete Code Blueprint: `src/arch/idt.rs`

```rust
//! Interrupt Descriptor Table (IDT), Naked ISR Stubs, 8259 PIC and Exception Dispatcher.
//!
//! Enforces hardware Ring 0 vs Ring 3 fault isolation, IST stack assignment for #DF,
//! and dispatches hardware interrupts (Timer, Keyboard, Mouse).

use core::arch::global_asm;
use core::mem::size_of;

/// Saved CPU register context passed to the Rust interrupt dispatcher.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct InterruptContext {
    pub r15: u64,
    pub r14: u64,
    pub r13: u64,
    pub r12: u64,
    pub r11: u64,
    pub r10: u64,
    pub r9:  u64,
    pub r8:  u64,
    pub rdi: u64,
    pub rsi: u64,
    pub rbp: u64,
    pub rdx: u64,
    pub rcx: u64,
    pub rbx: u64,
    pub rax: u64,

    pub vector: u64,
    pub error_code: u64,

    pub rip: u64,
    pub cs: u64,
    pub rflags: u64,
    pub rsp: u64,
    pub ss: u64,
}

/// 16-byte IDT Entry Descriptor for x86_64.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct IdtEntry {
    pub offset_low: u16,
    pub selector: u16,
    pub ist: u8,
    pub type_attr: u8,
    pub offset_mid: u16,
    pub offset_high: u32,
    pub reserved: u32,
}

impl IdtEntry {
    pub const fn missing() -> Self {
        Self {
            offset_low: 0,
            selector: 0,
            ist: 0,
            type_attr: 0,
            offset_mid: 0,
            offset_high: 0,
            reserved: 0,
        }
    }

    pub fn new(handler: usize, selector: u16, ist: u8, dpl: u8) -> Self {
        let offset = handler as u64;
        let type_attr = 0x80 | ((dpl & 0x03) << 5) | 0x0E; // Present=1, DPL, 64-bit Interrupt Gate (0xE)
        Self {
            offset_low: offset as u16,
            selector,
            ist: ist & 0x07,
            type_attr,
            offset_mid: (offset >> 16) as u16,
            offset_high: (offset >> 32) as u32,
            reserved: 0,
        }
    }
}

/// IDTR register layout for `lidt`.
#[repr(C, packed)]
pub struct Idtr {
    pub limit: u16,
    pub base: u64,
}

/// 256-entry Interrupt Descriptor Table.
#[repr(C, align(16))]
pub struct Idt {
    pub entries: [IdtEntry; 256],
}

impl Idt {
    pub const fn new() -> Self {
        Self {
            entries: [IdtEntry::missing(); 256],
        }
    }

    pub fn set_handler(&mut self, vector: usize, handler: usize, ist: u8, dpl: u8) {
        self.entries[vector] = IdtEntry::new(handler, super::gdt::KERNEL_CODE_SELECTOR, ist, dpl);
    }

    pub fn pointer(&'static self) -> Idtr {
        Idtr {
            limit: (size_of::<Idt>() - 1) as u16,
            base: self as *const _ as u64,
        }
    }
}

static mut IDT: Idt = Idt::new();

// -----------------------------------------------------------------------------
// 8259 Programmable Interrupt Controller (PIC)
// -----------------------------------------------------------------------------

const PIC1_CMD:  u16 = 0x20;
const PIC1_DATA: u16 = 0x21;
const PIC2_CMD:  u16 = 0xA0;
const PIC2_DATA: u16 = 0xA1;

const PIC_EOI:   u8  = 0x20;

#[inline]
unsafe fn io_wait() {
    core::arch::asm!("out 0x80, al", in("al") 0u8, options(nostack, preserves_flags));
}

#[inline]
unsafe fn outb(port: u16, val: u8) {
    core::arch::asm!("out dx, al", in("dx") port, in("al") val, options(nostack, preserves_flags));
}

#[inline]
unsafe fn inb(port: u16) -> u8 {
    let val: u8;
    core::arch::asm!("in al, dx", in("dx") port, out("al") val, options(nostack, preserves_flags));
    val
}

/// Remaps 8259 PIC IRQs 0..15 to IDT vectors 32..47.
pub unsafe fn init_pic() {
    let mask1 = inb(PIC1_DATA);
    let mask2 = inb(PIC2_DATA);

    // ICW1: Start init sequence in cascade mode
    outb(PIC1_CMD, 0x11);
    io_wait();
    outb(PIC2_CMD, 0x11);
    io_wait();

    // ICW2: Vector offsets (Master = 32, Slave = 40)
    outb(PIC1_DATA, 0x20);
    io_wait();
    outb(PIC2_DATA, 0x28);
    io_wait();

    // ICW3: Cascade setup (Slave connected to IRQ2 of Master)
    outb(PIC1_DATA, 0x04);
    io_wait();
    outb(PIC2_DATA, 0x02);
    io_wait();

    // ICW4: 8086/88 mode
    outb(PIC1_DATA, 0x01);
    io_wait();
    outb(PIC2_DATA, 0x01);
    io_wait();

    // Restore masks (or enable IRQ0 Timer, IRQ1 Keyboard, IRQ2 Cascade, IRQ12 Mouse)
    // IRQ0 (bit 0), IRQ1 (bit 1), IRQ2 (bit 2) unmasked on master: 0xF8
    // IRQ12 (bit 4) unmasked on slave: 0xEF
    outb(PIC1_DATA, 0xF8);
    io_wait();
    outb(PIC2_DATA, 0xEF);
    io_wait();
}

/// Sends End-Of-Interrupt (EOI) to 8259 PIC.
pub fn pic_send_eoi(irq: u8) {
    unsafe {
        if irq >= 8 {
            outb(PIC2_CMD, PIC_EOI);
        }
        outb(PIC1_CMD, PIC_EOI);
    }
}

// -----------------------------------------------------------------------------
// Assembly ISR Stubs Generation via global_asm!
// -----------------------------------------------------------------------------

extern "C" {
    static isr_stub_table: [usize; 256];
}

global_asm!(
    r#"
    .altmacro
    .macro isr_no_err_stub nr
    .global isr_stub_\nr
    isr_stub_\nr:
        push 0
        push \nr
        jmp isr_common_stub
    .endm

    .macro isr_err_stub nr
    .global isr_stub_\nr
    isr_stub_\nr:
        push \nr
        jmp isr_common_stub
    .endm

    /* Vectors 0..31 (Exceptions) */
    isr_no_err_stub 0
    isr_no_err_stub 1
    isr_no_err_stub 2
    isr_no_err_stub 3
    isr_no_err_stub 4
    isr_no_err_stub 5
    isr_no_err_stub 6
    isr_no_err_stub 7
    isr_err_stub    8
    isr_no_err_stub 9
    isr_err_stub    10
    isr_err_stub    11
    isr_err_stub    12
    isr_err_stub    13
    isr_err_stub    14
    isr_no_err_stub 15
    isr_no_err_stub 16
    isr_err_stub    17
    isr_no_err_stub 18
    isr_no_err_stub 19
    isr_no_err_stub 20
    isr_err_stub    21
    isr_no_err_stub 22
    isr_no_err_stub 23
    isr_no_err_stub 24
    isr_no_err_stub 25
    isr_no_err_stub 26
    isr_no_err_stub 27
    isr_no_err_stub 28
    isr_err_stub    29
    isr_err_stub    30
    isr_no_err_stub 31

    /* Vectors 32..255 (IRQs and Software Interrupts) */
    .set i, 32
    .rept 224
        isr_no_err_stub %i
        .set i, i + 1
    .endr

    /* Common ISR Stub: Saves all GPRs and invokes Rust dispatcher */
    .global isr_common_stub
    isr_common_stub:
        push rax
        push rbx
        push rcx
        push rdx
        push rbp
        push rsi
        push rdi
        push r8
        push r9
        push r10
        push r11
        push r12
        push r13
        push r14
        push r15

        mov rdi, rsp
        call rust_interrupt_handler

        pop r15
        pop r14
        pop r13
        pop r12
        pop r11
        pop r10
        pop r9
        pop r8
        pop rdi
        pop rsi
        pop rbp
        pop rdx
        pop rcx
        pop rbx
        pop rax

        add rsp, 16 /* Pop vector number and error code */
        iretq

    /* Array of 256 function pointers to ISR entry stubs */
    .section .rodata
    .global isr_stub_table
    .align 8
    isr_stub_table:
    .set j, 0
    .rept 256
        .quad isr_stub_\j
        .set j, j + 1
    .endr
    "#
);

// -----------------------------------------------------------------------------
// Rust Exception & Interrupt Dispatcher
// -----------------------------------------------------------------------------

pub type IrqCallback = fn(irq: u8, ctx: &mut InterruptContext);
pub type FaultCallback = fn(vector: u64, ctx: &mut InterruptContext, cr2: u64);

static mut TIMER_CALLBACK: Option<IrqCallback> = None;
static mut KEYBOARD_CALLBACK: Option<IrqCallback> = None;
static mut MOUSE_CALLBACK: Option<IrqCallback> = None;
static mut FAULT_CALLBACK: Option<FaultCallback> = None;

pub fn register_timer_callback(cb: IrqCallback) {
    unsafe { TIMER_CALLBACK = Some(cb); }
}

pub fn register_keyboard_callback(cb: IrqCallback) {
    unsafe { KEYBOARD_CALLBACK = Some(cb); }
}

pub fn register_mouse_callback(cb: IrqCallback) {
    unsafe { MOUSE_CALLBACK = Some(cb); }
}

pub fn register_fault_callback(cb: FaultCallback) {
    unsafe { FAULT_CALLBACK = Some(cb); }
}

/// Primary Rust Interrupt and Exception Dispatcher.
///
/// Called directly from `isr_common_stub`.
#[no_mangle]
pub extern "C" fn rust_interrupt_handler(ctx: *mut InterruptContext) {
    let ctx_ref = unsafe { &mut *ctx };
    let vector = ctx_ref.vector;

    if vector < 32 {
        // CPU Exception (0..31)
        handle_exception(ctx_ref);
    } else if vector >= 32 && vector < 48 {
        // Hardware IRQ (32..47)
        handle_irq(ctx_ref);
    } else {
        // Software Interrupt or Unhandled Vector
        crate::arch::serial::_print(format_args!("[IDT] Received unexpected vector {}\n", vector));
    }
}

/// Handles CPU exceptions with hardware Ring 0 vs Ring 3 privilege discrimination.
fn handle_exception(ctx: &mut InterruptContext) {
    let is_user = (ctx.cs & 0x03) == 3;
    let vector = ctx.vector;

    let cr2: u64 = if vector == 14 {
        let val: u64;
        unsafe { core::arch::asm!("mov {}, cr2", out(reg) val, options(nomem, nostack, preserves_flags)) };
        val
    } else {
        0
    };

    let vector_name = match vector {
        0 => "Divide-by-Zero (#DE)",
        1 => "Debug (#DB)",
        2 => "Non-Maskable Interrupt (NMI)",
        3 => "Breakpoint (#BP)",
        4 => "Overflow (#OF)",
        5 => "Bound Range Exceeded (#BR)",
        6 => "Invalid Opcode (#UD)",
        7 => "Device Not Available (#NM)",
        8 => "Double Fault (#DF)",
        10 => "Invalid TSS (#TS)",
        11 => "Segment Not Present (#NP)",
        12 => "Stack-Segment Fault (#SS)",
        13 => "General Protection Fault (#GP)",
        14 => "Page Fault (#PF)",
        16 => "x87 FPU Error (#MF)",
        17 => "Alignment Check (#AC)",
        18 => "Machine Check (#MC)",
        19 => "SIMD Floating-Point (#XM)",
        21 => "Control Protection (#CP)",
        _ => "Reserved / Unknown Exception",
    };

    if is_user {
        // ---------------------------------------------------------------------
        // RING 3 USERSPACE FAULT ISOLATION (R2 Requirement)
        // ---------------------------------------------------------------------
        crate::arch::serial::_print(format_args!(
            "[FAULT-ISOLATION] Userspace Fault caught: {} (vec {}) at RIP=0x{:016x}, ErrorCode=0x{:x}, CR2=0x{:016x}\n",
            vector_name, vector, ctx.rip, ctx.error_code, cr2
        ));

        // Invoke registered fault callback (M2 Task Reaper / Scheduler)
        if let Some(cb) = unsafe { FAULT_CALLBACK } {
            cb(vector, ctx, cr2);
        } else {
            // Early fallback before M2 scheduler runs: advance RIP to skip instruction or halt
            crate::arch::serial::_print(format_args!("[FAULT-ISOLATION] No fault callback registered; spinning user fault.\n"));
            loop {
                core::hint::spin_loop();
            }
        }
    } else {
        // ---------------------------------------------------------------------
        // RING 0 KERNEL FAULT -> FATAL PANIC
        // ---------------------------------------------------------------------
        crate::arch::serial::_print(format_args!(
            "\n==================== KERNEL EXCEPTION PANIC ====================\n\
             Exception: {} (Vector {})\n\
             Error Code: 0x{:016x} | CR2: 0x{:016x}\n\
             RIP: 0x{:016x} | CS: 0x{:04x} | RFLAGS: 0x{:016x}\n\
             RSP: 0x{:016x} | SS: 0x{:04x}\n\
             RAX: 0x{:016x} | RBX: 0x{:016x} | RCX: 0x{:016x}\n\
             RDX: 0x{:016x} | RSI: 0x{:016x} | RDI: 0x{:016x}\n\
             RBP: 0x{:016x} | R8:  0x{:016x} | R9:  0x{:016x}\n\
             R10: 0x{:016x} | R11: 0x{:016x} | R12: 0x{:016x}\n\
             R13: 0x{:016x} | R14: 0x{:016x} | R15: 0x{:016x}\n\
             ================================================================\n",
            vector_name, vector, ctx.error_code, cr2,
            ctx.rip, ctx.cs, ctx.rflags, ctx.rsp, ctx.ss,
            ctx.rax, ctx.rbx, ctx.rcx, ctx.rdx, ctx.rsi, ctx.rdi,
            ctx.rbp, ctx.r8, ctx.r9, ctx.r10, ctx.r11, ctx.r12,
            ctx.r13, ctx.r14, ctx.r15
        ));

        panic!("Fatal kernel exception: {}", vector_name);
    }
}

/// Handles hardware IRQs (32..47) and sends PIC EOI.
fn handle_irq(ctx: &mut InterruptContext) {
    let irq = (ctx.vector - 32) as u8;

    match irq {
        0 => {
            // IRQ 0: Timer Tick
            if let Some(cb) = unsafe { TIMER_CALLBACK } {
                cb(irq, ctx);
            }
        }
        1 => {
            // IRQ 1: PS/2 Keyboard
            if let Some(cb) = unsafe { KEYBOARD_CALLBACK } {
                cb(irq, ctx);
            }
        }
        12 => {
            // IRQ 12: PS/2 Mouse
            if let Some(cb) = unsafe { MOUSE_CALLBACK } {
                cb(irq, ctx);
            }
        }
        _ => {
            // Unhandled IRQ
        }
    }

    // Acknowledge interrupt to 8259 PIC
    pic_send_eoi(irq);
}

// -----------------------------------------------------------------------------
// IDT Public Initialization & Interrupt Control APIs
// -----------------------------------------------------------------------------

/// Initializes and loads the 256-entry IDT and configures 8259 PIC.
pub fn init_idt() {
    unsafe {
        // Populate all 256 entries from the assembly stub table
        for (i, &stub_addr) in isr_stub_table.iter().enumerate() {
            let ist = if i == 8 { 1 } else { 0 }; // Vector 8 (#DF) uses IST1
            IDT.set_handler(i, stub_addr, ist, 0);
        }

        // Load IDTR
        let idtr = IDT.pointer();
        core::arch::asm!(
            "lidt [{0}]",
            in(reg) &idtr,
            options(readonly, nostack, preserves_flags)
        );

        // Initialize 8259 PIC
        init_pic();
    }
}

/// Enables CPU hardware interrupts (`sti`).
#[inline(always)]
pub fn enable_interrupts() {
    unsafe {
        core::arch::asm!("sti", options(nomem, nostack));
    }
}

/// Disables CPU hardware interrupts (`cli`).
#[inline(always)]
pub fn disable_interrupts() {
    unsafe {
        core::arch::asm!("cli", options(nomem, nostack));
    }
}

/// Checks if CPU interrupts are currently enabled (RFLAGS IF bit 9).
#[inline(always)]
pub fn are_interrupts_enabled() -> bool {
    let rflags: u64;
    unsafe {
        core::arch::asm!("pushfq; pop {}", out(reg) rflags, options(nomem));
    }
    (rflags & (1 << 9)) != 0
}

/// Executes a closure with interrupts disabled, restoring the previous interrupt state upon exit.
pub fn without_interrupts<F, R>(f: F) -> R
where
    F: FnOnce() -> R,
{
    let saved = are_interrupts_enabled();
    if saved {
        disable_interrupts();
    }
    let ret = f();
    if saved {
        enable_interrupts();
    }
    ret
}
```

---

## 4. Subsystem Integration & Contracts

### 4.1 Kernel Entry Boot Sequence (`src/main.rs`)

During Milestone 1 bootup, the kernel initialization order must be:
```rust
#[no_mangle]
pub extern "C" fn _start() -> ! {
    // 1. Initialize Serial Port for debug logging
    arch::serial::init_serial();
    serial_println!("[AegisOS] Booting...");

    // 2. Initialize GDT and TSS (Reload CS, DS, ES, SS and load TR)
    let (kcs, kds, ucs, uds, tss) = arch::gdt::init_gdt_tss();
    serial_println!("[AegisOS] GDT & TSS initialized (KCS: 0x{:02x}, TSS: 0x{:02x})", kcs, tss);

    // 3. Initialize IDT and 8259 PIC (Remap IRQs to 32..47)
    arch::idt::init_idt();
    serial_println!("[AegisOS] IDT & 8259 PIC initialized");

    // 4. Initialize Memory Management (Frame allocator, Heap, Paging)
    memory::init(&MEMMAP_REQUEST, &HHDM_REQUEST);

    // 5. Enable hardware interrupts
    arch::idt::enable_interrupts();
    serial_println!("[AegisOS] Interrupts enabled");

    // Loop or handoff to scheduler
    loop {
        core::hint::spin_loop();
    }
}
```

### 4.2 M2 Scheduler Integration Contract

- **`pub fn set_tss_rsp0(stack_top: u64)`**:
  On every task context switch, `src/task/scheduler.rs` calls `set_tss_rsp0(next_task.kernel_stack_top)` so that if `next_task` (executing in Ring 3) faults or receives a timer interrupt, the CPU switches `RSP` directly to `next_task`'s kernel stack.
- **`pub fn register_timer_callback(cb: IrqCallback)`**:
  `src/task/scheduler.rs` registers its 100Hz round-robin tick handler to vector 32.

### 4.3 M2 Fault Isolation & Reaper Contract

- **`pub fn register_fault_callback(cb: FaultCallback)`**:
  `src/task/fault.rs` registers its user fault reaper callback:
  ```rust
  fn user_fault_reaper(vector: u64, ctx: &mut InterruptContext, cr2: u64) {
      let current_pid = scheduler::current_pid();
      serial_println!("[REAPER] Terminating PID {} due to fault vector {}", current_pid, vector);
      scheduler::terminate_current_process(ctx);
  }
  ```

### 4.4 M3 Graphics & Input Integration Contract

- **`pub fn register_keyboard_callback(cb: IrqCallback)`**:
  `src/drivers/ps2_keyboard.rs` registers IRQ 1 handler to read scancodes from port `0x60`.
- **`pub fn register_mouse_callback(cb: IrqCallback)`**:
  `src/drivers/ps2_mouse.rs` registers IRQ 12 handler to read 3-byte packets from port `0x60`.

---

## 5. Verification & Test Strategy

1. **GDT / TSS Loading Verification**:
   - Verify `CS == 0x08`, `DS == 0x10`, `SS == 0x10`.
   - Verify `str` instruction returns `0x28` (TSS Selector).
2. **Interrupt Table & IST Verification**:
   - Trigger software interrupt `int 3` (#BP) and verify control reaches `rust_interrupt_handler` with vector 3 and returns cleanly without crash.
   - Verify vector 8 (#DF) entry in IDT has `ist == 1`.
3. **Ring 0 vs Ring 3 Fault Isolation Verification**:
   - In Ring 0, trigger a divide-by-zero (`let _ = 1 / 0;`) -> verify fatal kernel panic log is output.
   - In Ring 3, trigger a divide-by-zero or page fault (`CS & 3 == 3`) -> verify `[FAULT-ISOLATION]` log is printed and the kernel continues executing without panic.
4. **8259 PIC & Timer Verification**:
   - Enable interrupts (`sti`), verify timer IRQ 0 ticks increment and serial logs tick events.
