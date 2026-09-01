# Project: AegisOS

## Architecture
AegisOS is a lightweight, crash-resilient x86_64 operating system written in pure `no_std` Rust. It utilizes the Limine bootloader protocol for dual BIOS/UEFI boot, enforces hardware Ring 0/Ring 3 privilege separation via GDT/TSS/IDT and 4-level PML4 paging, isolates application faults (#PF, #DE, #GP, #UD) to prevent kernel panics and desktop freezes, features a double-buffered linear RGB graphical compositor with a macOS-inspired desktop interface, and includes a full suite of interactive system applications.

```
+-------------------------------------------------------------------------+
|                              USERSPACE                                  |
|  +--------------------+  +--------------------+  +-------------------+  |
|  |  Crash-Test Demo   |  |  Activity Monitor  |  |  Terminal Shell   |  |
|  +--------------------+  +--------------------+  +-------------------+  |
|  +--------------------+  +--------------------+                         |
|  |  AegisPad (Editor) |  |   About AegisOS    |                         |
|  +--------------------+  +--------------------+                         |
+-------------------------------------------------------------------------+
|                   DESKTOP ENVIRONMENT & COMPOSITOR                     |
|  +-------------------------------------------------------------------+  |
|  | Top Menu Bar (24px) | Window Manager (Z-order) | Bottom Dock      |  |
|  +-------------------------------------------------------------------+  |
|  | Double-Buffered Graphics Engine (60 FPS) | PS/2 Mouse & Keyboard  |  |
+-------------------------------------------------------------------------+
|                           KERNEL CORE (Ring 0)                          |
|  +---------------------+ +----------------------+ +------------------+  |
|  | Preemptive Scheduler| | Fault Isolation / ISR| | Memory / Paging  |  |
|  | (100Hz Round-Robin) | | (Ring 3 Trap Handler)| | (Bitmap & Heap)  |  |
|  +---------------------+ +----------------------+ +------------------+  |
|  | GDT / TSS / IDT     | | Serial Console (COM1)| | Limine Boot Protocol|
+-------------------------------------------------------------------------+
```

## Feature Inventory
| # | Feature | Description | Milestone | Source |
|---|---------|-------------|-----------|--------|
| F1 | Limine Bootloader & Target Config | Higher-half kernel linking, `no_std` Rust, `.limine_reqs`, `limine.cfg`, target config | M1 | ORIGINAL_REQUEST §R1 |
| F2 | Serial Console & Panic Handler | 16550 UART driver on COM1 `0x3F8`, `print!`/`println!` macros, diagnostic panic handler | M1 | ORIGINAL_REQUEST §R6 |
| F3 | GDT, TSS & IDT Privilege Architecture | 64-bit GDT (Ring 0/3 selectors), TSS (`RSP0`, `IST1`), 256-vector IDT with naked ISR stubs | M1 | ORIGINAL_REQUEST §R1 |
| F4 | Physical & Kernel Heap Allocators | 128KB Bitmap frame allocator for 4GB RAM, kernel heap allocator supporting `alloc` crate | M1 | ORIGINAL_REQUEST §R3 |
| F5 | 4-Level PML4 Virtual Address Spaces | HHDM higher-half mapping, per-process PML4 page tables with user lower-half isolation | M1 | ORIGINAL_REQUEST §R1, R3 |
| F6 | Ring 3 Fault Isolation & Crash Resilience | Exception dispatcher detecting `(CS & 3) == 3` for #PF, #DE, #GP, #UD, logging fault, 2-phase deferred zombie frame reclamation | M2 | ORIGINAL_REQUEST §R2 |
| F7 | Preemptive Multitasking Scheduler | 100Hz timer IRQ context switching (GPRs, RIP, RSP, RBP, RFLAGS, CR3), PCB table, idle task | M2 | ORIGINAL_REQUEST §R3 |
| F8 | Linear RGB Double-Buffered Compositor | 32-bit ARGB framebuffer driver, 60 FPS tear-free scanline blitting, embedded font rasterizer, 2D primitives | M3 | ORIGINAL_REQUEST §R4 |
| F9 | PS/2 Mouse & Keyboard Drivers | PS/2 controller init, scancode decoder, 3-byte packet decoder, fluid cursor renderer with hotspot | M3 | ORIGINAL_REQUEST §R4 |
| F10 | macOS Desktop & Window Manager | 24px top menu bar (logo, active app, uptime clock, CPU/RAM badge), floating window manager (dragging, focus, close), bottom dock | M4 | ORIGINAL_REQUEST §R4 |
| F11.1 | Crash-Test Demo App | Interactive UI with buttons triggering #PF (Null ptr / OOB), #DE (Div by zero), #UD (Invalid opcode) proving process isolation | M4 | ORIGINAL_REQUEST §R5.1 |
| F11.2 | Activity Monitor App | Real-time CPU % history, live RAM usage graph (< 60MB RAM check), interactive process table with Kill button | M4 | ORIGINAL_REQUEST §R5.2 |
| F11.3 | Interactive Terminal Shell App | CLI shell window with commands: `ps`, `kill`, `free`, `echo`, `run`, `clear`, `reboot`, command history | M4 | ORIGINAL_REQUEST §R5.3 |
| F11.4 | AegisPad Text Editor App | Multiline text editor window with line numbers, cursor navigation, editing | M4 | ORIGINAL_REQUEST §R5.4 |
| F11.5 | About AegisOS Dialog App | Modal dialog with shield logo, kernel version, architecture, memory footprint stats | M4 | ORIGINAL_REQUEST §R5.5 |
| F12 | Automated Build Pipeline & QEMU Runner | `run_qemu.sh`, hybrid BIOS/UEFI bootable ISO `aegis_os.iso` via xorriso, QEMU graphical & serial runner | M5 | ORIGINAL_REQUEST §R6 |

## Milestones
| # | Name | Scope | Dependencies | Status |
|---|------|-------|-------------|--------|
| M1 | Bare-Metal Foundation, Memory Subsystem & Architecture | F1, F2, F3, F4, F5 | none | DONE |
| M2 | Preemptive Scheduler, Ring 3 Fault Isolation & Crash Resilience | F6, F7 | M1 | DONE |
| M3 | Framebuffer Graphics Engine & Input Subsystem | F8, F9 | M1 | DONE |
| M4 | macOS Desktop Environment, Window Manager & 5 Core System Applications | F10, F11.1, F11.2, F11.3, F11.4, F11.5 | M2, M3 | DONE |
| M5 | Build Pipeline, Bootable Hybrid ISO, QEMU Harness & E2E Acceptance Verification | F12, Phase 1 (100% E2E Pass) & Phase 2 (Adversarial Hardening) | M4 | DONE |

All five milestones boot and run: verified in QEMU by serial log and framebuffer
screendump, not by inspection. Two applications beyond the original five also
ship (Calculator, Snake), reachable from the dock and from `run <app>`.

## Verified Behaviour

Measured in QEMU (`-accel kvm`, 1280x800x32) rather than asserted:

| | |
|---|---|
| Compositor | ~72 FPS (was ~3) |
| Frame cost | ~28 Mcyc (was 968 Mcyc) |
| Uptime clock | accurate to wall time (19s -> 50s over 31s measured) |
| Timer | 100 Hz, PIT-programmed |
| Fault isolation | #PF, #DE, #UD and a Ring 3 write to kernel space all trapped, reaped, desktop survives |
| Input | keyboard and mouse verified under a 560-event flood |
| Memory | 16 MB used of 3064 MB, within the <60 MB target |

## Engineering Constraints

Two invariants the code now depends on. Breaking either reintroduces a hard hang.

1. **Any `static Mutex` an interrupt handler touches may only be locked from task
   context under an `arch::InterruptGuard`.** Contention between two tasks
   resolves, because the spinning task gets preempted; contention from an ISR
   does not, because the handler runs with `IF` clear and the holder can never be
   rescheduled to release the lock. This applies to `SERIAL1`, `SCHEDULER`,
   `CRASH_CALLBACK`, `KEYBOARD_STATE`, `KEY_QUEUE`, `MOUSE_DRIVER` and
   `MOUSE_QUEUE`. `GLOBAL_FRAME_ALLOCATOR` and `FRAMEBUFFER` have no ISR user.

2. **Interrupt handlers must not allocate.** The global allocator is a plain
   spinlock, so an ISR allocating while the interrupted code is itself inside the
   allocator deadlocks the machine. `Vec` and `VecDeque` grow on push and are
   therefore unusable in an ISR; use `drivers::ring::EventRing`, which is
   preallocated in the static itself.

## Known Gaps

- The 8x16 font covers ASCII 32..126 plus seven supplementary glyphs
  (`drivers`/`gui/font.rs`). Any further non-ASCII character falls back to `?`.
- `tests/` contains host-target stress harnesses that have never been executed
  against the kernel; the verification above is all via QEMU.

## Interface Contracts

### M1 (Foundation & Memory) -> M2 (Scheduler & Faults)
- `pub fn init_gdt_tss() -> (u16 /* kernel_cs */, u16 /* kernel_ds */, u16 /* user_cs */, u16 /* user_ds */, u16 /* tss_sel */)`
- `pub fn set_tss_rsp0(stack_top: u64)`
- `pub fn alloc_frame() -> Option<PhysAddr>` / `pub fn free_frame(frame: PhysAddr)`
- `pub fn create_user_address_space() -> PhysAddr /* PML4 root */`
- `pub fn destroy_user_address_space(pml4: PhysAddr)`
- `pub fn phys_to_virt(phys: PhysAddr) -> VirtAddr`

### M2 (Scheduler & Faults) -> M4 (Desktop & Apps)
- `pub fn spawn_process(name: &str, entry: extern "C" fn(), is_user: bool) -> ProcessId`
- `pub fn kill_process(pid: ProcessId) -> bool`
- `pub fn get_process_list() -> Vec<ProcessInfo>`
- `pub fn get_cpu_usage() -> u32 /* 0..100 */`
- `pub fn get_memory_stats() -> MemoryStats /* used_bytes, total_bytes */`
- `pub fn register_crash_callback(cb: fn(pid: ProcessId, fault_name: &str, rip: u64, cr2: u64))`

### M3 (Graphics & Input) -> M4 (Desktop & Apps)
- `pub fn get_screen_dimensions() -> (usize, usize)`
- `pub fn draw_rect(x: i32, y: i32, w: usize, h: usize, color: Color)`
- `pub fn draw_text(x: i32, y: i32, text: &str, color: Color)`
- `pub fn swap_buffers()`
- `pub fn poll_input_events() -> Option<InputEvent>` (Key, MouseMove, MouseButton)

## Code Layout
```
aegis_os/
├── Cargo.toml
├── .cargo/config.toml
├── linker.ld
├── limine.cfg
├── run_qemu.sh
├── Makefile
├── src/
│   ├── main.rs                 # Kernel entrypoint (_start)
│   ├── arch/
│   │   ├── mod.rs
│   │   ├── gdt.rs              # GDT & TSS configuration
│   │   ├── idt.rs              # IDT vectors & naked ISR stubs
│   │   └── serial.rs           # 16550 UART driver & print macros
│   ├── memory/
│   │   ├── mod.rs
│   │   ├── frame.rs            # Physical bitmap frame allocator
│   │   ├── heap.rs             # Kernel heap allocator
│   │   └── paging.rs           # PML4 virtual address spaces & HHDM
│   ├── task/
│   │   ├── mod.rs
│   │   ├── pcb.rs              # Process control block & state
│   │   ├── scheduler.rs        # Preemptive round-robin scheduler
│   │   ├── context.rs          # Context switch assembly routines
│   │   └── fault.rs            # Ring 3 fault detection & 2-phase reaping
│   ├── drivers/
│   │   ├── mod.rs
│   │   ├── framebuffer.rs      # Linear RGB double-buffered driver
│   │   ├── ps2_keyboard.rs     # PS/2 keyboard driver & scancodes
│   │   └── ps2_mouse.rs        # PS/2 mouse driver & cursor tracking
│   ├── gui/
│   │   ├── mod.rs
│   │   ├── font.rs             # Embedded 8x16 bitmap font
│   │   ├── primitives.rs       # 2D drawing primitives & color math
│   │   ├── menubar.rs          # 24px macOS top menu bar
│   │   ├── dock.rs             # Launcher dock
│   │   ├── window.rs           # Floating window structure & widgets
│   │   └── wm.rs               # Window manager & event dispatch
│   └── apps/
│       ├── mod.rs
│       ├── crash_test.rs       # Crash-Test Demo App
│       ├── activity_monitor.rs # Activity Monitor (CPU/RAM/<60MB)
│       ├── terminal.rs         # Terminal Shell (ps, kill, free, etc.)
│       ├── editor.rs           # Text Editor (AegisPad)
│       └── about.rs            # About AegisOS Dialog
└── tests/
    └── e2e/                    # Opaque-box E2E test suites
```
