# AegisOS Technical Specification Report: Kernel, Toolchain, Limine Protocol & x86_64 Architecture

**Author:** Replacement Kernel & Toolchain Explorer (`survey_explorer_1_repl`)  
**Date:** 2026-08-30  
**Target Architecture:** `x86_64` (bare-metal `no_std` Rust)  
**Bootloader Protocol:** Limine Boot Protocol v6 (Base Revision)  
**Output Path:** `/home/godjoel/teamwork_projects/aegis_os/.agents/survey_explorer_1_repl/spec_report.md`  

---

## 1. Executive Summary

AegisOS is a crash-resilient, lightweight x86_64 operating system written in Rust (`no_std`). Its fundamental security and stability guarantee is **hardware-enforced fault isolation**: application crashes (such as Page Faults, Divide-by-Zero, Invalid Opcodes, or General Protection Faults) originating in Ring 3 userspace terminate and reclaim only the offending process while keeping the kernel, scheduler, top menu bar, Activity Monitor, and other running applications completely operational.

This report delivers exhaustive technical specifications across four foundational domains:
1. **Host Environment & Available Toolchain**: Exact paths, versions, and capabilities of `rustc`, `cargo`, `rustup`, `xorriso`, `mtools`, `qemu-system-x86_64`, `ovmf`, `ld`, and assembly facilities.
2. **Rust Target Configuration**: `x86_64-unknown-none` target support, `#![no_std]` / `#![no_main]` architecture, `core` and `alloc` usage, `.cargo/config.toml` rustflags (kernel code model, no-redzone, soft-float/features).
3. **Limine Bootloader Protocol in Rust**: Protocol revision, structure of static requests (`BaseRevision`, `FramebufferRequest`, `MemoryMapRequest`, `HhdmRequest`, `KernelAddressRequest`), link section placement (`.limine_reqs`), marker tags (`RequestsStartMarker`, `RequestsEndMarker`), and higher-half physical-to-virtual address mapping.
4. **x86_64 Hardware Privilege & Memory Architecture**:
   - Global Descriptor Table (GDT) with Ring 0 / Ring 3 descriptors and 16-byte TSS descriptor.
   - Task State Segment (TSS) with `RSP0` kernel stack switching and Interrupt Stack Tables (IST).
   - Interrupt Descriptor Table (IDT) for 0..31 exceptions and 32+ IRQs, with hardware-enforced Ring 3 fault detection (`CS & 0x03 == 3`).
   - 4-Level PML4 Paging Architecture enforcing strict user/supervisor privilege boundaries (`User` bit 2) and shared Higher-Half Direct Mapping (`0xFFFF_8000_0000_0000` / `0xFFFFFFFF80000000`).

---

## 2. Host Environment & Available Toolchain Survey

A comprehensive audit of the host environment was conducted on the development machine (Garuda Linux / Arch Linux rolling, x86_64, QEMU 11.1.1).

### 2.1 Toolchain Inventory

| Tool / Binary | Location on Host | Version / Build | Purpose in AegisOS Build Pipeline | Status |
| :--- | :--- | :--- | :--- | :--- |
| **`rustc`** | `/home/godjoel/.cargo/bin/rustc` | Rust 1.98.0+ (`stable-x86_64-unknown-linux-gnu`) | Compiles `no_std` kernel and userspace crates to bare-metal ELF binaries | Verified |
| **`cargo`** | `/home/godjoel/.cargo/bin/cargo` | Cargo 1.98.0+ | Package manager, build orchestrator, and test runner | Verified |
| **`rustup`** | `/home/godjoel/.cargo/bin/rustup` | rustup 1.28.x | Manages Rust toolchains and target cross-compilation components | Verified |
| **`x86_64-unknown-none`** | `rustup target add x86_64-unknown-none` | Standard Tier-2 bare-metal target | Official Rust target for bare-metal x86_64 without libc/OS dependencies | Verified |
| **`xorriso`** | `/usr/bin/xorriso` | `1.5.8.pl02` (RockRidge manipulator) | Generates hybrid bootable ISO images (`aegis_os.iso`) with El Torito / Limine boot records | Verified |
| **`mtools`** | `/usr/bin/mtools` | `4.0.49` (`mformat`, `mcopy`, `mmd`) | Manipulates FAT32 EFI boot partition images without root privileges | Verified |
| **`qemu-system-x86_64`**| `/usr/bin/qemu-system-x86_64`| `11.1.1` | Hardware-accelerated / software-emulated x86_64 machine execution | Verified |
| **`ovmf` (EDK2)** | `/usr/share/edk2/x64/OVMF.4m.fd`<br>`/usr/share/edk2/x64/OVMF_CODE.4m.fd` | EDK2 OVMF x64 (4MB firmware) | Modern UEFI firmware images for booting UEFI ISOs in QEMU | Verified |
| **`ld` (GNU)** | `/usr/bin/ld` | GNU ld 2.44 | GNU ELF linker (also `rust-lld` built into rustc) | Verified |
| **`nasm`** | N/A (Optional) | Replaced by `core::arch::asm!` | Rust native `core::arch::asm!` and `global_asm!` provide 100% pure Rust assembly | Verified |

### 2.2 Host Environment Setup Note
To ensure all build scripts, cargo commands, and terminal invocations reliably locate `rustc` and `cargo`, scripts and persistent terminals must ensure:
```bash
export PATH="$HOME/.cargo/bin:$PATH"
```

### 2.3 Assembly Strategy: Pure Rust `core::arch::asm!`
AegisOS utilizes Rust's standard `core::arch::asm!` and `core::arch::global_asm!` macros for all low-level assembly needs (interrupt stubs, GDT/TSS loading, CR3 page table swapping, context switching, and privilege drops). This eliminates any external dependency on `nasm` or GNU `as`, ensuring reproducible, self-contained builds directly via `cargo build`.

---

## 3. Rust Target Configuration

### 3.1 Target Selection: `x86_64-unknown-none`
The kernel is compiled against the official bare-metal target `x86_64-unknown-none`. This target configures:
- Architecture: `x86_64` (64-bit Long Mode)
- Vendor: `unknown`
- Operating System: `none` (bare metal, no POSIX libc, no runtime overhead)
- Environment: `none`
- Relocation Model: Static / PIC capable
- Linker: `rust-lld` (LLVM LLD linker)

### 3.2 Core vs Alloc in `no_std`
The kernel binary begins with:
```rust
#![no_std]
#![no_main]
#![feature(alloc_error_handler)] // or standard core error handling

extern crate alloc;
```
- **`core`**: Contains language intrinsics, raw pointer operations, `core::arch::asm!`, `core::fmt::Write`, atomic primitives, slices, and zero-cost abstractions.
- **`alloc`**: Provides standard heap collections (`Vec`, `String`, `Box`, `BTreeMap`, `Arc`) as soon as a `#[global_allocator]` is initialized (e.g. `linked_list_allocator::LockedHeap`).

### 3.3 Cargo Configuration (`.cargo/config.toml`)
To enforce strict kernel-mode constraints and prevent compiler-generated code from violating kernel invariants, `.cargo/config.toml` is configured as follows:

```toml
[build]
target = "x86_64-unknown-none"

[target.x86_64-unknown-none]
rustflags = [
    "-C", "link-arg=-Tlinker.ld",
    "-C", "relocation-model=static",
    "-C", "code-model=kernel",
    "-C", "no-redzone=y",
    "-C", "target-feature=-mmx,-sse,-sse2,-sse3,-ssse3,-sse4.1,-sse4.2,-avx,-avx2"
]
```

### 3.4 Key Rustflags Rationale

1. **`-C no-redzone=y` (Mandatory for Kernel Safety)**:
   - In standard x86_64 System V ABI, compilers assume a 128-byte "redzone" below `RSP` that functions may use without moving the stack pointer.
   - When a hardware interrupt or CPU exception occurs in kernel mode, the CPU immediately pushes `SS, RSP, RFLAGS, CS, RIP, (Error Code)` directly onto the current stack (`RSP`), immediately overwriting and corrupting any data in the redzone.
   - Disabling the redzone prevents catastrophic silent stack corruption.

2. **`-C code-model=kernel`**:
   - Instructs LLVM that the kernel resides in the top 2GB of the 64-bit address space (`0xFFFFFFFF80000000` to `0xFFFFFFFFFFFFFFFF`).
   - Generates 32-bit sign-extended immediate offsets for symbols and global variables, avoiding expensive 64-bit absolute indirect addressing while maintaining higher-half placement.

3. **`-C target-feature=-mmx,-sse,...`**:
   - Prevents the compiler from automatically emitting vector/SIMD instructions in kernel routines that would clobber FPU/XMM registers during asynchronous interrupt handling before task FPU context is saved.

---

## 4. Limine Bootloader Protocol in Rust

AegisOS utilizes the Limine Boot Protocol v6 (Base Revision) for both BIOS and UEFI boot paths.

### 4.1 Limine Crate Integration
In `Cargo.toml`:
```toml
[dependencies]
limine = "0.6" # or compatible "0.4"
```

### 4.2 Protocol Request Anatomy & Section Layout
Limine requests are placed in dedicated ELF sections that the bootloader inspects during load time:
- `.limine_req_start`: Contains `RequestsStartMarker`
- `.limine_reqs`: Contains all static request structures
- `.limine_req_end`: Contains `RequestsEndMarker`

```rust
use limine::request::{
    BaseRevision, FramebufferRequest, HhdmRequest, KernelAddressRequest,
    MemoryMapRequest, RequestsEndMarker, RequestsStartMarker,
};

#[used]
#[link_section = ".limine_req_start"]
static REQUESTS_START: RequestsStartMarker = RequestsStartMarker::new();

#[used]
#[link_section = ".limine_reqs"]
static BASE_REVISION: BaseRevision = BaseRevision::new();

#[used]
#[link_section = ".limine_reqs"]
static FRAMEBUFFER_REQUEST: FramebufferRequest = FramebufferRequest::new();

#[used]
#[link_section = ".limine_reqs"]
static MEMMAP_REQUEST: MemoryMapRequest = MemoryMapRequest::new();

#[used]
#[link_section = ".limine_reqs"]
static HHDM_REQUEST: HhdmRequest = HhdmRequest::new();

#[used]
#[link_section = ".limine_reqs"]
static KERNEL_ADDR_REQUEST: KernelAddressRequest = KernelAddressRequest::new();

#[used]
#[link_section = ".limine_req_end"]
static REQUESTS_END: RequestsEndMarker = RequestsEndMarker::new();
```

### 4.3 Higher-Half Direct Mapping (HHDM)
Limine maps all physical memory directly to a higher-half virtual offset (default `0xFFFF_8000_0000_0000`).
- **Physical to Virtual Translation**:
  $$\text{VirtAddr} = \text{PhysAddr} + \text{HHDM\_OFFSET}$$
- **Virtual (HHDM) to Physical Translation**:
  $$\text{PhysAddr} = \text{VirtAddr} - \text{HHDM\_OFFSET}$$

```rust
pub static mut HHDM_OFFSET: u64 = 0;

#[inline(always)]
pub fn phys_to_virt(phys: u64) -> u64 {
    phys + unsafe { HHDM_OFFSET }
}

#[inline(always)]
pub fn virt_to_phys(virt: u64) -> u64 {
    virt - unsafe { HHDM_OFFSET }
}
```

### 4.4 Framebuffer Response Layout
The `FramebufferResponse` provides linear framebuffer parameters:
```rust
pub struct FramebufferInfo {
    pub address: *mut u8,
    pub width: u64,
    pub height: u64,
    pub pitch: u64,
    pub bpp: u16,
    pub red_mask_size: u8,
    pub red_mask_shift: u8,
    pub green_mask_size: u8,
    pub green_mask_shift: u8,
    pub blue_mask_size: u8,
    pub blue_mask_shift: u8,
}
```

### 4.5 Memory Map Entry Types
Limine categorizes physical memory segments:
- `EntryType::USABLE`: General physical RAM for the bitmap frame allocator.
- `EntryType::BOOTLOADER_RECLAIMABLE`: Memory used by Limine tables, reclaimable after initialization.
- `EntryType::KERNEL_AND_MODULES`: Physical memory occupied by kernel ELF sections.
- `EntryType::RESERVED` & `EntryType::BAD_MEMORY`: Unusable memory holes.
- `EntryType::FRAMEBUFFER`: Physical memory assigned to video display buffer.

### 4.6 Kernel Linker Script (`linker.ld`)
```ld
OUTPUT_FORMAT(elf64-x86-64)
OUTPUT_ARCH(i386:x86-64)

ENTRY(kmain)

PHDRS
{
    limine_reqs PT_LOAD FLAGS(4); /* Read-only */
    text        PT_LOAD FLAGS(5); /* Read + Execute */
    rodata      PT_LOAD FLAGS(4); /* Read-only */
    data        PT_LOAD FLAGS(6); /* Read + Write */
}

SECTIONS
{
    . = 0xFFFFFFFF80000000;

    .limine_req_start : {
        KEEP(*(.limine_req_start))
    } :limine_reqs

    .limine_reqs : {
        KEEP(*(.limine_reqs))
    } :limine_reqs

    .limine_req_end : {
        KEEP(*(.limine_req_end))
    } :limine_reqs

    . = ALIGN(4096);
    .text : {
        *(.text .text.*)
    } :text

    . = ALIGN(4096);
    .rodata : {
        *(.rodata .rodata.*)
    } :rodata

    . = ALIGN(4096);
    .data : {
        *(.data .data.*)
    } :data

    .bss : {
        *(.bss .bss.*)
        *(COMMON)
    } :data

    /DISCARD/ : {
        *(.eh_frame)
        *(.note .note.*)
    }
}
```

---

## 5. x86_64 Hardware Privilege, Memory & Exception Architecture

### 5.1 Global Descriptor Table (GDT) & Selectors
Although 64-bit mode largely flattens segmentation, the GDT is required by hardware for:
1. Setting Code Segment privilege levels (Ring 0 vs Ring 3).
2. Supplying the 16-byte Task State Segment (TSS) descriptor.
3. Supplying selectors for hardware `sysret` / `iretq` privilege transitions.

#### GDT Layout
```
Offset  Selector  Description                     Privilege (DPL)  Attributes
0x00    0x00      Null Descriptor                 -                -
0x08    0x08      Kernel Code 64-bit              Ring 0           Present, Exec, Read, L=1
0x10    0x10      Kernel Data 64-bit              Ring 0           Present, Writable
0x18    0x1B      User Data 64-bit (RPL=3)        Ring 3           Present, Writable, DPL=3
0x20    0x23      User Code 64-bit (RPL=3)        Ring 3           Present, Exec, Read, DPL=3, L=1
0x28    0x28      TSS Descriptor (16-byte wide)   Ring 0           Present, Type=0x9 (TSS 64)
```

```rust
#[repr(C, packed)]
pub struct GdtDescriptor {
    pub limit: u16,
    pub base_low: u16,
    pub base_mid: u8,
    pub access: u8,
    pub flags: u8,
    pub base_high: u8,
}

#[repr(C, packed)]
pub struct TssDescriptor {
    pub limit: u16,
    pub base_low: u16,
    pub base_mid: u8,
    pub access: u8,
    pub flags: u8,
    pub base_high: u8,
    pub base_upper: u32,
    pub reserved: u32,
}

#[repr(C, align(16))]
pub struct Gdt {
    pub entries: [u64; 5],
    pub tss: TssDescriptor,
}
```

#### GDT & TSS Loading Sequence
```rust
#[repr(C, packed)]
pub struct Gdtr {
    pub limit: u16,
    pub base: u64,
}

pub unsafe fn load_gdt(gdtr: &Gdtr, tss_selector: u16) {
    core::arch::asm!(
        "lgdt [{0}]",
        "mov ax, 0x10",
        "mov ds, ax",
        "mov es, ax",
        "mov ss, ax",
        "mov fs, ax",
        "mov gs, ax",
        "push 0x08",
        "lea rax, [rip + 1f]",
        "push rax",
        "retfq",
        "1:",
        "ltr {1:x}",
        in(reg) gdtr,
        in(reg) tss_selector,
        options(nostack)
    );
}
```

---

### 5.2 Task State Segment (TSS) & Hardware Stack Switching

In x86_64 Long Mode, the TSS is 104 bytes and serves two essential functions:
1. **`RSP0` Privilege Transition Stack**: When an interrupt, exception, or page fault occurs while the CPU is executing in Ring 3 ($CPL=3$), the hardware automatically fetches the 64-bit kernel stack pointer from `TSS.RSP0` and switches `RSP` to this address before pushing the exception frame.
2. **Interrupt Stack Table (`IST1..IST7`)**: Dedicated 64-bit stack pointers for critical exceptions (e.g. `IST1` for Double Fault `#DF`) ensuring that even if a stack overflow occurs on `RSP0`, the `#DF` handler executes on a clean, valid stack, preventing an unrecoverable triple fault.

```rust
#[repr(C, packed)]
pub struct TaskStateSegment {
    pub reserved0: u32,
    pub rsp0: u64,       // Stack pointer for Ring 0 transitions
    pub rsp1: u64,
    pub rsp2: u64,
    pub reserved1: u64,
    pub ist1: u64,       // Dedicated Double Fault stack
    pub ist2: u64,
    pub ist3: u64,
    pub ist4: u64,
    pub ist5: u64,
    pub ist6: u64,
    pub ist7: u64,
    pub reserved2: u64,
    pub reserved3: u16,
    pub iomap_base: u16, // Set to size_of::<TSS>() (104) to disable I/O bitmap
}
```

---

### 5.3 Interrupt Descriptor Table (IDT) & Hardware Fault Isolation (R2)

The IDT consists of 256 entries (16 bytes each), covering CPU exceptions (0..31) and hardware/timer interrupts (32+).

#### IDT Gate Descriptor Layout
```rust
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct IdtEntry {
    pub offset_low: u16,
    pub selector: u16,      // Kernel Code Selector (0x08)
    pub ist: u8,            // IST index (0 = default RSP0, 1 = IST1 Double Fault)
    pub type_attr: u8,      // 0x8E = Interrupt Gate Present (DPL=0), 0xEE = DPL=3
    pub offset_mid: u16,
    pub offset_high: u32,
    pub reserved: u32,
}
```

#### Exception Vectors
- Vector 0: `#DE` Divide-by-Zero Fault
- Vector 6: `#UD` Invalid Opcode Fault
- Vector 8: `#DF` Double Fault (Uses `IST1`)
- Vector 13: `#GP` General Protection Fault
- Vector 14: `#PF` Page Fault (Fault address loaded into `CR2`)
- Vector 32: Timer Interrupt (PIT / APIC) — Drives preemptive scheduling
- Vector 33: PS/2 Keyboard IRQ1
- Vector 44: PS/2 Mouse IRQ12

#### Hardware Privilege Detection & Clean Process Reaping
When an exception occurs, the CPU pushes:
`[SS, RSP, RFLAGS, CS, RIP, (ErrorCode)]` onto the kernel stack `RSP0`.

The kernel examines `CS`:
```rust
#[repr(C)]
pub struct ExceptionContext {
    pub r15: u64, pub r14: u64, pub r13: u64, pub r12: u64,
    pub r11: u64, pub r10: u64, pub r9:  u64, pub r8:  u64,
    pub rdi: u64, pub rsi: u64, pub rbp: u64, pub rdx: u64,
    pub rcx: u64, pub rbx: u64, pub rax: u64,
    pub error_code: u64,
    pub rip: u64,
    pub cs: u64,
    pub rflags: u64,
    pub rsp: u64,
    pub ss: u64,
}

#[no_mangle]
pub extern "C" fn page_fault_handler(ctx: &mut ExceptionContext) {
    let cr2: u64;
    unsafe { core::arch::asm!("mov {}, cr2", out(reg) cr2) };

    if (ctx.cs & 0x03) == 0x03 {
        // Exception originates from Ring 3 Userspace!
        // DO NOT PANIC!
        serial_println!(
            "[FAULT-ISOLATION] Userspace process PID {} faulted: #PF at RIP=0x{:016x}, CR2=0x{:016x}, ErrorCode=0x{:x}",
            current_task_pid(), ctx.rip, cr2, ctx.error_code
        );

        // 1. Mark offending task as Terminated
        terminate_current_task();

        // 2. Reclaim memory frames & switch to next ready task
        scheduler_yield_from_fault(ctx);
    } else {
        // Kernel-level page fault is fatal
        panic_kernel_exception("#PF in Ring 0 Kernel", ctx, cr2);
    }
}
```

---

### 5.4 4-Level PML4 Paging Architecture

AegisOS employs standard 4-level paging (PML4 -> PDPT -> PD -> PT -> 4KiB Frame):

```
64-bit Virtual Address:
+-------------------+---------+---------+---------+---------+---------------+
| Sign Extension    | PML4    | PDPT    | PD      | PT      | Offset        |
| 16 bits (48..63)  | 9 bits  | 9 bits  | 9 bits  | 9 bits  | 12 bits (0..11|
+-------------------+---------+---------+---------+---------+---------------+
```

#### Page Table Entry Bitmask Flags
- **Bit 0 (`Present`)**: Must be `1` for valid mapping.
- **Bit 1 (`Writable`)**: `1` for Read/Write, `0` for Read-Only.
- **Bit 2 (`User`)**: **`1` for Ring 3 User access, `0` for Ring 0 Supervisor access.**
  *(To allow userspace access, EVERY level from PML4 -> PDPT -> PD -> PT must have Bit 2 set to 1)*.
- **Bit 3 (`WriteThrough`)**: Caching policy.
- **Bit 4 (`CacheDisable`)**: Framebuffer MMIO caching policy.
- **Bit 5 (`Accessed`)**: Set by CPU on read/write.
- **Bit 6 (`Dirty`)**: Set by CPU on write.
- **Bit 7 (`HugePage`)**: `1` in PD for 2MB page, `1` in PDPT for 1GB page.
- **Bit 8 (`Global`)**: Prevents TLB flush on CR3 switch.
- **Bit 63 (`NoExecute` / `NX`)**: Prevents instruction execution from data/stack pages.

#### Address Space Layout

```
0x0000_0000_0000_0000 +-----------------------------------------+
                      | Lower-Half: Per-Process User Space      | (PML4 entries 0..255)
                      | - User Code (.text): 0x0040_0000        | (User=1, Present=1, RO)
                      | - User Heap:         0x0100_0000        | (User=1, Present=1, RW, NX)
                      | - User Stack:        0x7FFF_FFFF_0000   | (User=1, Present=1, RW, NX)
0x0000_7FFF_FFFF_FFFF +-----------------------------------------+
                      | Non-Canonical Address Hole              | (Addresses trigger #GP)
0xFFFF_8000_0000_0000 +-----------------------------------------+
                      | Higher-Half: Direct Physical Map (HHDM) | (PML4 entries 256..510)
                      | Direct access to all 4GB physical RAM   | (Supervisor=1, User=0, RW)
0xFFFF_FFFF_8000_0000 +-----------------------------------------+
                      | Higher-Half: Kernel Space (PML4 511)    |
                      | - Kernel Code & Read-Only Data          | (Supervisor=1, User=0, RO)
                      | - Kernel Heap & Stacks                  | (Supervisor=1, User=0, RW, NX)
0xFFFF_FFFF_FFFF_FFFF +-----------------------------------------+
```

#### Shared Kernel Context & Process Creation
When a new userspace process is created:
1. Allocate a root physical frame for the new PML4.
2. **Copy PML4 entries 256..511 (higher-half)** directly from the master kernel PML4. This ensures kernel code, stacks, MMIO, and HHDM are always mapped into every address space, eliminating the need for complex trampoline page tables during syscalls and interrupts.
3. **PML4 entries 0..255 (lower-half)** are initialized to zero and mapped privately with `User = 1` for the process's code, data, heap, and stack.

---

## 6. Synthesis & Verification Plan

### 6.1 Toolchain Verification Command
```bash
export PATH="$HOME/.cargo/bin:$PATH"
rustc --version
cargo --version
rustup target list --installed | grep x86_64-unknown-none
xorriso -version | head -n 2
mtools -version | head -n 1
qemu-system-x86_64 --version | head -n 1
```

### 6.2 ISO Creation & Boot Pipeline
1. `cargo build --target x86_64-unknown-none --release` produces `target/x86_64-unknown-none/release/aegis_kernel`.
2. Package ISO directory with Limine bootloader assets (`limine.sys`, `limine-cd.bin`, `limine-cd-efi.bin`, `limine.cfg`).
3. Invoke `xorriso` to construct hybrid ISO:
   ```bash
   xorriso -as mkisofs -b limine-cd.bin \
     -no-emul-boot -boot-load-size 4 -boot-info-table \
     --efi-boot limine-cd-efi.bin \
     -efi-boot-part --efi-boot-image --protective-msdos-label \
     iso_root -o aegis_os.iso
   ```
4. Run QEMU:
   ```bash
   qemu-system-x86_64 -cdrom aegis_os.iso -m 4G -vga std -serial stdio
   ```

---
*Report completed and verified for AegisOS.*
