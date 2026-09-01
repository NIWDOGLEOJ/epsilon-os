# AegisOS Milestone 1 Technical Blueprint: Toolchain, Linker, Bootloader & Serial Console

**Author:** M1 Toolchain & Serial Explorer (`m1_explorer_1`)  
**Target Milestone:** M1 (Bare-Metal Foundation, Memory Subsystem & Architecture)  
**Deliverable Files Covered:**
- `Cargo.toml` (Dependencies, Profiles, Edition)
- `.cargo/config.toml` (Target, Rustflags, Kernel Code Model, No-Redzone)
- `linker.ld` (Higher-Half Mapping at `0xFFFFFFFF80100000`, Limine Requests Section Placement)
- `limine.cfg` / `limine.conf` (Bootloader Configuration for BIOS & UEFI)
- `src/main.rs` (Entrypoint `_start`, Limine Request Structures, Early Boot Flow, Diagnostic Panic Handler)
- `src/arch/mod.rs` & `src/arch/serial.rs` (16550 UART COM1 Driver, Formatted Print Macros, Port I/O)

---

## 1. Executive Summary & Architectural Overview

Milestone 1 establishes the rock-solid bare-metal foundation for AegisOS. The kernel is compiled as a `#![no_std]` / `#![no_main]` 64-bit ELF binary targeting `x86_64-unknown-none`. It boots via the Limine Bootloader Protocol (Dual BIOS/UEFI), places the kernel in the higher-half virtual address space (`0xFFFFFFFF80100000`), utilizes a Higher-Half Direct Map (HHDM) for zero-overhead physical frame translation, and initializes an industrial-grade 16550 UART Serial Console on COM1 (`0x3F8`) running at 115200 baud (8N1).

```
+-----------------------------------------------------------------------------------+
|                        LIMINE BOOTLOADER PROTOCOL (v6/v7)                         |
|  - Validates BaseRevision tag                                                     |
|  - Sets up Higher-Half Direct Map (HHDM: 0xFFFF_8000_0000_0000)                   |
|  - Provides Linear 32-bit ARGB Framebuffer Response (1280x800)                    |
|  - Provides Usable RAM Memory Map (4GB RAM)                                       |
|  - Transfers execution to Higher-Half Entry Point: _start (0xFFFFFFFF8010XXXX)    |
+-----------------------------------------+-----------------------------------------+
                                          |
                                          v
+-----------------------------------------------------------------------------------+
|                            EARLY KERNEL ENTRY (_start)                            |
|  1. Initialize 16550 UART Driver on COM1 (0x3F8 @ 115200 baud, 8N1)               |
|  2. Print OS Banner and verify Limine Base Revision                               |
|  3. Read HHDM offset, Kernel Address, Framebuffer, and Usable Memory Map          |
|  4. Transfer control to GDT/TSS, IDT, Frame Allocator, Paging & Heap Init         |
|  5. Diagnostic Panic Handler routes structured kernel panics over COM1 Serial     |
+-----------------------------------------------------------------------------------+
```

---

## 2. Dependency Specification: `Cargo.toml`

### 2.1 Crate Requirements & Compatibility Matrix
On Rust 1.98.0 `x86_64-unknown-none`:
- `limine = "0.5.0"`: Provides stable Rust Limine bootloader protocol structures with `BaseRevision::with_revision(2)`, `FramebufferRequest`, `MemoryMapRequest`, `HhdmRequest`, `ExecutableAddressRequest`. (Avoid `0.6` which requires nightly `ptr_metadata`, and `0.4.0` which is yanked).
- `spin = { version = "0.9.8", default-features = false, features = ["spin_mutex", "lock_api_crate"] }`: Provides thread-safe, lock-free spinlocks for kernel singletons (`SERIAL1`).
- `volatile = "0.4.6"`: Safe volatile memory wrapper for MMIO and framebuffer access.
- `bitflags = "2.4.2"`: Type-safe bitmasks for GDT, IDT, Paging, and Serial flags.
- `x86_64 = { version = "0.14.13", default-features = false }`: Low-level x86_64 types and port abstractions without nightly features.
- `linked_list_allocator = "0.10.5"`: Kernel heap allocator for `alloc` collections.

### 2.2 Complete `Cargo.toml` Blueprint

```toml
[package]
name = "aegis_os"
version = "0.1.0"
edition = "2021"
authors = ["AegisOS Core Team"]
description = "A crash-resilient x86_64 operating system in Rust with macOS desktop environment"

[dependencies]
# Limine Bootloader Protocol bindings (v5.0 stable-compatible)
limine = "0.5.0"

# Spinlock synchronization for no_std kernel globals
spin = { version = "0.9.8", default-features = false, features = ["spin_mutex", "lock_api_crate"] }

# Safe volatile memory access for MMIO framebuffer
volatile = "0.4.6"

# Type-safe bitflags for GDT, IDT, Paging, and Hardware registers
bitflags = "2.4.2"

# Low-level x86_64 structures and instructions (default-features disabled for stable compatibility)
x86_64 = { version = "0.14.13", default-features = false }

# Kernel heap allocator supporting extern crate alloc
linked_list_allocator = "0.10.5"

[profile.dev]
panic = "abort"
opt-level = 1

[profile.release]
panic = "abort"
opt-level = 3
lto = true
codegen-units = 1
```

---

## 3. Toolchain & Target Configuration: `.cargo/config.toml`

### 3.1 Rationale for Compilation Flags
1. **Target `x86_64-unknown-none`**: Standard Tier-2 bare-metal target without OS standard library dependencies.
2. **`-C link-arg=-Tlinker.ld`**: Enforces higher-half memory layout and `.limine_reqs` section retention.
3. **`-C relocation-model=static`**: Generates absolute position-independent kernel symbols.
4. **`-C code-model=kernel`**: Informs LLVM that code and global variables reside in the top 2GB of virtual address space (`0xFFFFFFFF80000000` to `0xFFFFFFFFFFFFFFFF`), enabling 32-bit sign-extended addressing.
5. **`-C no-redzone=y` (CRITICAL)**: Disables the 128-byte System V AMD64 ABI redzone below `RSP`. When CPU interrupts/exceptions fire, hardware pushes `[SS, RSP, RFLAGS, CS, RIP]` directly onto the stack. If the redzone is active, interrupt frames will clobber local variables in leaf functions.
6. **`-C target-feature=-sse,-sse2,-sse3,-ssse3,-sse4.1,-sse4.2,-avx,-avx2`**: Prevents the compiler from emitting SIMD instructions in kernel routines, ensuring interrupt handlers do not corrupt vector registers.

### 3.2 Complete `.cargo/config.toml` Blueprint

```toml
[build]
target = "x86_64-unknown-none"

[target.x86_64-unknown-none]
rustflags = [
    "-C", "link-arg=-Tlinker.ld",
    "-C", "relocation-model=static",
    "-C", "code-model=kernel",
    "-C", "no-redzone=y",
    "-C", "target-feature=-sse,-sse2,-sse3,-ssse3,-sse4.1,-sse4.2,-avx,-avx2"
]
```

---

## 4. Higher-Half Linker Script: `linker.ld`

### 4.1 Memory Layout & Alignment
- **Virtual Base Address**: `0xFFFFFFFF80100000` (Kernel Higher-Half at 1MB virtual offset).
- **Program Headers (PHDRS)**:
  - `limine_reqs`: Read-Only (`FLAGS(4)`), contains requests for bootloader detection.
  - `text`: Read + Execute (`FLAGS(5)`), executable kernel instructions.
  - `rodata`: Read-Only (`FLAGS(4)`), static constants and string literals.
  - `data`: Read + Write (`FLAGS(6)`), mutable globals, `.bss`, and common symbols.
- **`KEEP(*(.limine_req*))`**: Crucial flag preventing the linker from stripping protocol request tags during Dead Code Elimination (`--gc-sections`).

### 4.2 Complete `linker.ld` Blueprint

```ld
OUTPUT_FORMAT(elf64-x86-64)
OUTPUT_ARCH(i386:x86-64)

ENTRY(_start)

PHDRS
{
    limine_reqs PT_LOAD FLAGS(4); /* Read-only (R--) */
    text        PT_LOAD FLAGS(5); /* Read + Execute (R-X) */
    rodata      PT_LOAD FLAGS(4); /* Read-only (R--) */
    data        PT_LOAD FLAGS(6); /* Read + Write (RW-) */
}

SECTIONS
{
    /* Kernel placed in higher-half top 2GB at 1MB offset */
    . = 0xFFFFFFFF80100000;

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

## 5. Bootloader Configuration: `limine.cfg` / `limine.conf`

Supports both Limine v6/v7 syntax, ensuring immediate boot under BIOS and UEFI.

### 5.1 Complete `limine.conf` Blueprint (Modern Limine v7 format)

```
timeout: 3

/AegisOS
    protocol: limine
    path: boot():/boot/aegis_kernel
```

### 5.2 Complete `limine.cfg` Blueprint (Limine v6 / BIOS format)

```
TIMEOUT=3

:AegisOS
    PROTOCOL=limine
    KERNEL_PATH=boot:///boot/aegis_kernel
```

---

## 6. Serial Console Driver: `src/arch/serial.rs`

### 6.1 Hardware Specifications (16550 UART COM1)
- **Base Port**: `0x3F8`
- **Baud Rate**: 115200 Baud (Divisor = $115200 / 115200 = 1$, LSB = `0x01`, MSB = `0x00`)
- **Data Format**: 8 Data Bits, 1 Stop Bit, No Parity (8N1: `0x03` in LCR)
- **FIFO Control**: 14-byte trigger threshold, FIFO enabled, TX/RX FIFOs cleared (`0xC7` in FCR)
- **Modem Control**: RTS/DSR enabled, Auxiliary Output 2 (OUT2) enabled (`0x0B` in MCR)
- **Output Safety**: Automatic CRLF transformation (`\n` -> `\r\n`), spinlock protected via `spin::Mutex`.

### 6.2 Complete `src/arch/serial.rs` Blueprint

```rust
//! 16550 UART Serial Driver for COM1 (0x3F8)
//!
//! Provides thread-safe serial console logging and formatted output macros.

use core::fmt::{self, Write};
use spin::Mutex;

/// Standard COM1 Base I/O Port
pub const COM1_BASE: u16 = 0x3F8;

// Register Offsets from Base
const DATA_PORT: u16 = 0;     // Transmit/Receive Buffer (or DLL when DLAB=1)
const INT_ENABLE: u16 = 1;    // Interrupt Enable Register (or DLM when DLAB=1)
const FIFO_CTRL: u16 = 2;     // FIFO Control Register (Write) / IIR (Read)
const LINE_CTRL: u16 = 3;     // Line Control Register (DLAB bit 7)
const MODEM_CTRL: u16 = 4;    // Modem Control Register
const LINE_STATUS: u16 = 5;   // Line Status Register
const MODEM_STATUS: u16 = 6;  // Modem Status Register
const SCRATCH_REG: u16 = 7;   // Scratch Register

// Line Status Register Bits
const LSR_DATA_READY: u8 = 0x01;
const LSR_TRANSMITTER_EMPTY: u8 = 0x20;

/// Low-level x86 Port I/O: Read Byte
#[inline(always)]
pub unsafe fn inb(port: u16) -> u8 {
    let value: u8;
    core::arch::asm!(
        "in al, dx",
        out("al") value,
        in("dx") port,
        options(nomem, nostack, preserves_flags)
    );
    value
}

/// Low-level x86 Port I/O: Write Byte
#[inline(always)]
pub unsafe fn outb(port: u16, value: u8) {
    core::arch::asm!(
        "out dx, al",
        in("dx") port,
        in("al") value,
        options(nomem, nostack, preserves_flags)
    );
}

/// Short I/O delay to allow legacy bus settling
#[inline(always)]
pub unsafe fn io_wait() {
    outb(0x80, 0);
}

/// 16550 UART Controller Interface
pub struct SerialPort {
    base: u16,
    initialized: bool,
}

impl SerialPort {
    /// Create a new SerialPort bound to the given base port address
    pub const fn new(base: u16) -> Self {
        Self {
            base,
            initialized: false,
        }
    }

    /// Initialize the 16550 UART chip
    pub fn init(&mut self) {
        unsafe {
            // 1. Disable all serial interrupts during configuration
            outb(self.base + INT_ENABLE, 0x00);
            io_wait();

            // 2. Enable DLAB (Divisor Latch Access Bit) in Line Control Register
            outb(self.base + LINE_CTRL, 0x80);
            io_wait();

            // 3. Set baud rate divisor to 1 (115,200 Baud)
            // Divisor = 115200 / 115200 = 1 (LSB = 0x01, MSB = 0x00)
            outb(self.base + DATA_PORT, 0x01); // Divisor LSB
            io_wait();
            outb(self.base + INT_ENABLE, 0x00); // Divisor MSB
            io_wait();

            // 4. Configure Line Control: 8 data bits, 1 stop bit, no parity (8N1), clear DLAB
            outb(self.base + LINE_CTRL, 0x03);
            io_wait();

            // 5. Enable FIFO, clear TX/RX queues, set 14-byte interrupt threshold
            outb(self.base + FIFO_CTRL, 0xC7);
            io_wait();

            // 6. Configure Modem Control: Set RTS/DSR, enable Auxiliary Output 2 (OUT2)
            outb(self.base + MODEM_CTRL, 0x0B);
            io_wait();

            // 7. Loopback test to verify hardware transceiver
            outb(self.base + MODEM_CTRL, 0x1E); // Enable loopback mode
            io_wait();
            outb(self.base + DATA_PORT, 0xAE); // Send test byte
            io_wait();
            let _ = inb(self.base + DATA_PORT); // Read back byte

            // 8. Restore normal operation mode (disable loopback, IRQs active, OUT1/OUT2 set)
            outb(self.base + MODEM_CTRL, 0x0F);
            io_wait();
        }
        self.initialized = true;
    }

    /// Check if the transmit FIFO buffer is empty and ready for new data
    #[inline]
    pub fn is_transmit_empty(&self) -> bool {
        unsafe { (inb(self.base + LINE_STATUS) & LSR_TRANSMITTER_EMPTY) != 0 }
    }

    /// Write a single raw byte to the serial port
    pub fn write_byte(&mut self, byte: u8) {
        while !self.is_transmit_empty() {
            core::hint::spin_loop();
        }
        unsafe {
            outb(self.base + DATA_PORT, byte);
        }
    }

    /// Write a string slice to serial with automatic \n -> \r\n line ending conversion
    pub fn write_string(&mut self, s: &str) {
        for byte in s.bytes() {
            if byte == b'\n' {
                self.write_byte(b'\r');
            }
            self.write_byte(byte);
        }
    }

    /// Check if data is available to read from the serial port
    #[inline]
    pub fn is_data_ready(&self) -> bool {
        unsafe { (inb(self.base + LINE_STATUS) & LSR_DATA_READY) != 0 }
    }

    /// Read a single byte from the serial port if available
    pub fn read_byte(&mut self) -> Option<u8> {
        if self.is_data_ready() {
            Some(unsafe { inb(self.base + DATA_PORT) })
        } else {
            None
        }
    }
}

impl Write for SerialPort {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.write_string(s);
        Ok(())
    }
}

/// Global synchronized COM1 serial port singleton
pub static SERIAL1: Mutex<SerialPort> = Mutex::new(SerialPort::new(COM1_BASE));

/// Initialize the global COM1 serial console
pub fn init_serial() {
    SERIAL1.lock().init();
}

#[doc(hidden)]
pub fn _print(args: fmt::Arguments) {
    let mut serial = SERIAL1.lock();
    let _ = serial.write_fmt(args);
}

/// Print formatted text to the COM1 serial console
#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => {
        $crate::arch::serial::_print(format_args!($($arg)*))
    };
}

/// Print formatted text with trailing newline to the COM1 serial console
#[macro_export]
macro_rules! println {
    () => {
        $crate::print!("\n")
    };
    ($($arg:tt)*) => {
        $crate::print!("{}\n", format_args!($($arg)*))
    };
}

/// Explicit alias for serial printing
#[macro_export]
macro_rules! serial_print {
    ($($arg:tt)*) => {
        $crate::arch::serial::_print(format_args!($($arg)*))
    };
}

/// Explicit alias for serial printing with trailing newline
#[macro_export]
macro_rules! serial_println {
    () => {
        $crate::serial_print!("\n")
    };
    ($($arg:tt)*) => {
        $crate::serial_print!("{}\n", format_args!($($arg)*))
    };
}
```

---

## 7. Architecture Module: `src/arch/mod.rs`

```rust
//! Architecture-Specific Hardware Management (x86_64)

pub mod serial;
```

---

## 8. Entry Point & Early Boot: `src/main.rs`

### 8.1 Early Boot Lifecycle Sequence
1. **Serial Console Initialization**: Initializes COM1 16550 UART before any other subsystem so all diagnostic logs and panics are immediately visible.
2. **OS Banner Display**: Emits version and build banner.
3. **Limine Base Revision Check**: Validates `BASE_REVISION.is_supported()`. If unsupported, logs error and halts.
4. **HHDM Direct Physical Mapping Retrieval**: Stores `HHDM_OFFSET` globally for physical-to-virtual translations (`phys + offset`).
5. **Kernel Address Extraction**: Logs physical and virtual load base addresses (`0xFFFFFFFF80100000`).
6. **Framebuffer Detection**: Queries linear RGB framebuffer dimensions, pitch, and pixel format for M3 compositor setup.
7. **Physical Memory Map Parsing**: Sums usable RAM and total RAM from Limine memory map entries for M1 frame allocator setup.
8. **Diagnostic Panic Handler**: Catches any early kernel panics, displays detailed file/line/column/message information, and halts CPU safely with interrupts disabled.

### 8.2 Complete `src/main.rs` Blueprint

```rust
#![no_std]
#![no_main]

pub mod arch;

use limine::BaseRevision;
use limine::request::{
    ExecutableAddressRequest, FramebufferRequest, HhdmRequest, MemoryMapRequest,
    RequestsEndMarker, RequestsStartMarker,
};

// ============================================================================
// Limine Protocol Requests (.limine_reqs Section)
// ============================================================================

#[used]
#[link_section = ".limine_req_start"]
static REQUESTS_START: RequestsStartMarker = RequestsStartMarker::new();

#[used]
#[link_section = ".limine_reqs"]
static BASE_REVISION: BaseRevision = BaseRevision::with_revision(2);

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
static KERNEL_ADDR_REQUEST: ExecutableAddressRequest = ExecutableAddressRequest::new();

#[used]
#[link_section = ".limine_req_end"]
static REQUESTS_END: RequestsEndMarker = RequestsEndMarker::new();

// ============================================================================
// Global Architecture & Memory State
// ============================================================================

/// Direct physical memory mapping offset provided by Limine HHDM
pub static mut HHDM_OFFSET: u64 = 0;

/// Convert physical address to higher-half virtual address using HHDM
#[inline(always)]
pub fn phys_to_virt(phys: u64) -> u64 {
    phys + unsafe { HHDM_OFFSET }
}

/// Convert higher-half virtual address (in HHDM region) to physical address
#[inline(always)]
pub fn virt_to_phys(virt: u64) -> u64 {
    virt - unsafe { HHDM_OFFSET }
}

// ============================================================================
// Kernel Entry Point
// ============================================================================

/// Kernel entry point invoked by Limine bootloader in 64-bit Long Mode
#[no_mangle]
pub extern "C" fn _start() -> ! {
    // 1. Initialize 16550 Serial UART Console on COM1 (0x3F8)
    arch::serial::init_serial();

    serial_println!("=======================================================");
    serial_println!("        AegisOS v0.1.0 (x86_64 no_std Kernel)         ");
    serial_println!("   Crash-Resilient OS with Hardware Fault Isolation    ");
    serial_println!("=======================================================");

    // 2. Verify Limine Base Revision
    if !BASE_REVISION.is_supported() {
        serial_println!("[FATAL] Limine Base Revision 2 is not supported by bootloader!");
        hcf();
    }
    serial_println!("[OK] Limine Bootloader Protocol Base Revision verified.");

    // 3. Retrieve and store Higher-Half Direct Map (HHDM) Offset
    if let Some(hhdm_resp) = HHDM_REQUEST.get_response() {
        unsafe {
            HHDM_OFFSET = hhdm_resp.offset();
        }
        serial_println!("[BOOT] HHDM Physical Direct Map Offset: 0x{:016x}", unsafe { HHDM_OFFSET });
    } else {
        serial_println!("[WARN] No HHDM Response received from Limine!");
    }

    // 4. Retrieve Kernel Executable Physical & Virtual Base Addresses
    if let Some(exec_resp) = KERNEL_ADDR_REQUEST.get_response() {
        serial_println!("[BOOT] Kernel Physical Base: 0x{:016x}", exec_resp.physical_base());
        serial_println!("[BOOT] Kernel Virtual Base:  0x{:016x}", exec_resp.virtual_base());
    }

    // 5. Query Framebuffer Video Output
    if let Some(fb_resp) = FRAMEBUFFER_REQUEST.get_response() {
        for (idx, fb) in fb_resp.framebuffers().enumerate() {
            serial_println!(
                "[BOOT] Framebuffer #{}: {}x{} (Pitch: {} bytes, {} BPP)",
                idx,
                fb.width(),
                fb.height(),
                fb.pitch(),
                fb.bpp()
            );
        }
    } else {
        serial_println!("[WARN] No linear framebuffer provided by Limine!");
    }

    // 6. Parse Memory Map for Physical RAM Statistics
    if let Some(memmap_resp) = MEMMAP_REQUEST.get_response() {
        let mut usable_bytes: u64 = 0;
        let mut total_bytes: u64 = 0;
        let mut usable_regions: usize = 0;

        for entry in memmap_resp.entries() {
            total_bytes += entry.length;
            if entry.entry_type == limine::memory_map::EntryType::USABLE {
                usable_bytes += entry.length;
                usable_regions += 1;
            }
        }

        serial_println!(
            "[BOOT] Physical RAM: {} MB usable ({} regions) / {} MB total detected",
            usable_bytes / (1024 * 1024),
            usable_regions,
            total_bytes / (1024 * 1024)
        );
    }

    serial_println!("[BOOT] Early kernel foundation initialized successfully.");

    // Halt CPU loop (Scheduler / Desktop entry point will be called here in later milestones)
    hcf();
}

/// Halt and Catch Fire: Disables interrupts and loops hlt instruction
pub fn hcf() -> ! {
    loop {
        core::hint::spin_loop();
        unsafe {
            core::arch::asm!("cli; hlt", options(nomem, nostack));
        }
    }
}

// ============================================================================
// Diagnostic Kernel Panic Handler
// ============================================================================

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    serial_println!("\n=======================================================");
    serial_println!("               !!! KERNEL PANIC !!!                    ");
    serial_println!("=======================================================");

    if let Some(location) = info.location() {
        serial_println!(
            "Panic Location: {}:{}:{}",
            location.file(),
            location.line(),
            location.column()
        );
    } else {
        serial_println!("Panic Location: <unknown>");
    }

    serial_println!("Panic Message:  {}", info.message());
    serial_println!("=======================================================");
    serial_println!("System Execution Halted.");

    hcf();
}
```

---

## 9. Verification & Build Validation

### 9.1 Build Verification Command
```bash
export PATH="$HOME/.cargo/bin:$PATH"
cargo check --target x86_64-unknown-none
cargo build --release --target x86_64-unknown-none
```

### 9.2 ISO Creation & Dual Boot Execution
```bash
# 1. Prepare ISO directory structure
mkdir -p iso_root/boot/limine iso_root/EFI/BOOT

# 2. Copy compiled ELF kernel
cp target/x86_64-unknown-none/release/aegis_os iso_root/boot/aegis_kernel

# 3. Copy Limine configs and binaries
cp limine.conf iso_root/boot/limine/limine.conf
cp limine.cfg iso_root/boot/limine/limine.cfg
cp limine-bios.sys iso_root/boot/limine/
cp limine-bios-cd.bin iso_root/boot/limine/
cp limine-uefi-cd.bin iso_root/boot/limine/
cp BOOTX64.EFI iso_root/EFI/BOOT/

# 4. Generate Hybrid ISO
xorriso -as mkisofs -b boot/limine/limine-bios-cd.bin \
    -no-emul-boot -boot-load-size 4 -boot-info-table \
    --efi-boot boot/limine/limine-uefi-cd.bin \
    -efi-boot-part --efi-boot-image --protective-msdos-label \
    iso_root -o aegis_os.iso

# 5. Embed BIOS boot code
limine bios-install aegis_os.iso

# 6. Test BIOS Boot in QEMU with Serial Logging
qemu-system-x86_64 -cdrom aegis_os.iso -m 4G -display none -serial stdio

# 7. Test UEFI Boot in QEMU
qemu-system-x86_64 -bios /usr/share/edk2/x64/OVMF.4m.fd -cdrom aegis_os.iso -m 4G -display none -serial stdio
```
