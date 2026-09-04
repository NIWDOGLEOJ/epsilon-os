# Project: AegisOS

## Architecture

AegisOS is a crash-resilient x86_64 operating system written in pure `no_std` Rust. It
boots via the Limine protocol (dual BIOS/UEFI), enforces hardware Ring 0/Ring 3
privilege separation via GDT/TSS/IDT and 4-level PML4 paging, isolates application
faults (#PF, #DE, #GP, #UD) so a crashing process never takes down the kernel or
freezes the desktop, and drives a double-buffered linear RGB compositor presenting a
macOS-inspired desktop with fourteen applications.

Above the original desktop sit four later subsystems: an in-memory VFS, a PC-speaker
audio stack, a virtual loopback IPv4/UDP network stack, and a Ring 0 agent bridge
reachable over serial.

```
+-------------------------------------------------------------------------+
|                          APPLICATION SUITE (14)                         |
|  Terminal · Activity Monitor · Crash-Test · AegisPad · Aegis Files       |
|  Aegis Browser · Aegis Paint · AegisSynth · AegisChat · Minesweeper      |
|  Snake · Calculator · System Settings · About                            |
+-------------------------------------------------------------------------+
|                   DESKTOP ENVIRONMENT & COMPOSITOR                       |
|  Menu Bar (24px) | Window Manager (Z-order, snapping) | Dock (14 slots)  |
|  Spotlight Search (Ctrl+Space) | Wallpaper Engine (6 themes + PPM)       |
|  Double-Buffered Graphics | TSC Frame Pacer (60 FPS) | PS/2 Mouse & Kbd  |
+-------------------------------------------------------------------------+
|                       SERVICE SUBSYSTEMS (Ring 0)                        |
|  RAM Disk VFS | PC Speaker Audio | Loopback IPv4/UDP | AI Agent Bridge   |
+-------------------------------------------------------------------------+
|                           KERNEL CORE (Ring 0)                           |
|  Preemptive Scheduler | Fault Isolation / ISR | Memory / Paging          |
|  (100Hz Round-Robin)  | (Ring 3 Trap Handler) | (Bitmap & Heap)          |
|  GDT / TSS / IDT      | Serial Console (COM1) | Limine Boot Protocol     |
+-------------------------------------------------------------------------+
```

## Feature Inventory

### Foundation (M1–M5)

| # | Feature | Description | Source |
|---|---------|-------------|--------|
| F1 | Limine Bootloader & Target Config | Higher-half kernel linking, `no_std` Rust, `.limine_reqs`, `limine.cfg` | `main.rs`, `linker.ld` |
| F2 | Serial Console & Panic Handler | 16550 UART on COM1 `0x3F8`, `print!`/`println!`, diagnostic panic handler | `arch/serial.rs` |
| F3 | GDT, TSS & IDT Privilege Architecture | 64-bit GDT (Ring 0/3 selectors), TSS (`RSP0`, `IST1`), 256-vector IDT with naked ISR stubs | `arch/gdt.rs`, `arch/idt.rs` |
| F4 | Physical & Kernel Heap Allocators | 128KB bitmap frame allocator, 16 MB kernel heap backing `alloc` | `memory/frame.rs`, `memory/heap.rs` |
| F5 | 4-Level PML4 Virtual Address Spaces | HHDM higher-half mapping, per-process PML4 with user lower-half isolation | `memory/paging.rs` |
| F6 | Ring 3 Fault Isolation | Exception dispatcher on `(CS & 3) == 3` for #PF/#DE/#GP/#UD, 2-phase deferred zombie reclamation | `task/fault.rs` |
| F7 | Preemptive Multitasking Scheduler | 100Hz timer IRQ context switching (GPRs, RIP, RSP, RBP, RFLAGS, CR3), PCB table, idle task | `task/scheduler.rs`, `task/context.rs` |
| F8 | Double-Buffered Compositor | 32-bit ARGB framebuffer, clipped primitives, embedded 8x16 font rasterizer | `drivers/framebuffer.rs`, `gui/` |
| F9 | PS/2 Mouse & Keyboard | Controller init, scancode decoder, 3-byte packet decoder, cursor with hotspot | `drivers/ps2_*.rs` |
| F10 | Desktop & Window Manager | Menu bar, floating window manager (drag, focus, close), dock | `gui/menubar.rs`, `gui/wm.rs`, `gui/dock.rs` |
| F11 | Core Applications | Crash-Test, Activity Monitor, Terminal, AegisPad, About | `apps/` |
| F12 | Build Pipeline & QEMU Runner | Hybrid BIOS/UEFI `aegis_os.iso` via xorriso, graphical & serial runners | `build_iso.sh`, `run_qemu.sh` |

### Post-repair additions (M6–M17)

| # | Feature | Description | Source |
|---|---------|-------------|--------|
| F13 | TSC Frame Pacer & Font Expansion | TSC calibrated against the PIT, 16.667 ms frame budget; 30 supplementary glyphs beyond ASCII | `arch/time.rs`, `gui/font.rs` |
| F14 | In-Memory VFS (RAM Disk) | Hierarchical inode tree, seeded system docs, full CRUD, `InterruptGuard`-protected | `fs/mod.rs` |
| F15 | Aegis Paint | 436x220 canvas, Bresenham interpolation, 12 swatches, brush sizes, PPM export to VFS | `apps/paint.rs` |
| F16 | Aegis Files | Finder-style split pane, Places sidebar, breadcrumbs, metadata columns, opens into AegisPad | `apps/file_manager.rs` |
| F17 | PC Speaker Audio | PIT channel 2 tone generation, note sequences, sound effects | `drivers/speaker.rs` |
| F18 | Window Snapping & Minimization | Left-half / right-half / maximize edge snapping with live preview, restore, minimize | `gui/wm.rs`, `gui/window.rs` |
| F19 | Wallpaper Engine & System Settings | 6 procedural themes plus custom PPM wallpapers from VFS; Appearance/Sound/Display panes | `gui/wallpaper.rs`, `apps/settings.rs` |
| F20 | Calculator 2.0 | Scientific functions with a history tape | `apps/calculator.rs` |
| F21 | Terminal 2.0 | 20+ commands, tab auto-completion, command history, ANSI colour engine | `apps/terminal.rs` |
| F22 | Agent Bridge, Spotlight & Browser | Ring 0 RPC over serial; Ctrl+Space universal search; `aegis://` and `vfs://` markdown browser | `agent/mod.rs`, `gui/spotlight.rs`, `apps/browser.rs` |
| F23 | Minesweeper | 9x9 and 16x16 modes, first-click safety, flood reveal, flagging | `apps/minesweeper.rs` |
| F24 | AegisPad 2.0 | Multi-tab buffers, line-number gutter, find/replace, keyword syntax highlighting | `apps/editor.rs` |
| F25 | AegisSynth | 2-octave piano roll, 4-track 16-step sequencer, tempo control, chiptune presets | `apps/synth.rs` |
| F26 | Loopback Network Stack & AegisChat | IPv4 (RFC 791), UDP (RFC 768), internet checksum, loopback device, non-blocking `UdpSocket`; multi-channel chat client | `net/mod.rs`, `apps/chat.rs` |

## Milestones

| # | Name | Scope | Status |
|---|------|-------|--------|
| M1 | Bare-Metal Foundation, Memory Subsystem & Architecture | F1–F5 | DONE |
| M2 | Preemptive Scheduler & Ring 3 Fault Isolation | F6, F7 | DONE |
| M3 | Framebuffer Graphics Engine & Input Subsystem | F8, F9 | DONE |
| M4 | Desktop Environment, Window Manager & Core Applications | F10, F11 | DONE |
| M5 | Build Pipeline, Bootable Hybrid ISO & QEMU Harness | F12 | DONE |
| M6 | QEMU E2E Harness & In-Kernel Self-Tests | test infrastructure | DONE |
| M7 | Frame Pacing & Font Expansion | F13 | DONE |
| M8 | Virtual Filesystem & Document Persistence | F14 | DONE |
| M9 | Aegis Paint | F15 | DONE |
| M10 | Aegis Files | F16 | DONE |
| M11 | PC Speaker Audio Subsystem | F17 | DONE |
| M12 | Window Snapping, Maximize/Restore & Minimization | F18 | DONE |
| M13 | Wallpaper Engine & System Settings | F19 | DONE |
| M14 | Calculator 2.0 & Terminal 2.0 | F20, F21 | DONE |
| M15 | Agent Bridge, Spotlight & Browser | F22 | DONE |
| M16 | Minesweeper, AegisPad 2.0 & AegisSynth | F23, F24, F25 | DONE |
| M17 | Loopback Network Stack & AegisChat | F26 | DONE |

Per-milestone detail, including what each one added to the test suites, is in
HANDOFF.md.

## Verified Behaviour

Measured in QEMU (`-accel kvm`, 1280x800x32) rather than asserted:

| | |
|---|---|
| Compositor | paced to a 60 FPS / 16.667 ms budget (`arch::FramePacer`) |
| Uptime clock | accurate to wall time (19s -> 50s over 31s measured) |
| Timer | 100 Hz, PIT-programmed |
| Fault isolation | #PF, #DE, #UD and a Ring 3 write to kernel space all trapped, reaped, desktop survives |
| Input | keyboard and mouse verified under a 560-event flood |
| Memory | 16 MB used of 3064 MB, within the <60 MB target |

A caveat on provenance: these were measured during the boot-repair pass (HANDOFF.md),
before M7–M17 landed. The compositor figure has since changed by design — the free-
running ~72 FPS reported then is now capped at 60 by the frame pacer — and the ~28 Mcyc
per-frame cost predates thirteen subsequent applications. Treat the fault-isolation and
input rows as the durable claims and re-measure the performance rows before quoting
them.

## Engineering Constraints

Two invariants the code now depends on. Breaking either reintroduces a hard hang.

1. **Any `static Mutex` an interrupt handler touches may only be locked from task
   context under an `arch::InterruptGuard`.** Contention between two tasks
   resolves, because the spinning task gets preempted; contention from an ISR
   does not, because the handler runs with `IF` clear and the holder can never be
   rescheduled to release the lock. This applies to `SERIAL1`, `SCHEDULER`,
   `CRASH_CALLBACK`, `KEYBOARD_STATE`, `KEY_QUEUE`, `MOUSE_DRIVER` and
   `MOUSE_QUEUE`, and to the later subsystem globals (`RAM_FS`, `LOOPBACK_DEVICE`,
   `AGENT_TELEMETRY`). `GLOBAL_FRAME_ALLOCATOR` and `FRAMEBUFFER` have no ISR user.

2. **Interrupt handlers must not allocate.** The global allocator is a plain
   spinlock, so an ISR allocating while the interrupted code is itself inside the
   allocator deadlocks the machine. `Vec` and `VecDeque` grow on push and are
   therefore unusable in an ISR; use `drivers::ring::EventRing`, which is
   preallocated in the static itself.

## Known Gaps

- The 8x16 font covers ASCII 32..126 plus 30 supplementary glyphs (arrows, typography,
  math/units, UI status icons) in `gui/font.rs`. Anything else falls back to `?`.
- `run <app>` in the Terminal reaches nine of the fourteen apps. Browser, Minesweeper,
  Synth, Chat and Terminal itself have no `run` target and are dock/Spotlight-only.
- `mkdir` is implemented in the Terminal but is absent from both the `help` output and
  the tab-completion candidate list.
- The network stack is loopback-only. There is no NIC driver, so nothing leaves the VM.
- `tests/e2e/` models the design in host `std` Rust and never links the kernel; see
  the Tests section of README.md.

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
- `drivers::framebuffer::get_dimensions() -> (usize, usize)`
- `gui::primitives::draw_rect(...)` and the `Framebuffer` primitive set
- `gui::font::draw_string(fb, x, y, text, fg, bg)` / `measure_string(text) -> (u32, u32)`
- `drivers::framebuffer::swap_buffers()`
- `drivers::ps2_keyboard::poll_key_event() -> Option<KeyEvent>`
- `drivers::ps2_mouse::poll_mouse_event() -> Option<MouseEvent>`

Earlier revisions of this file listed `get_screen_dimensions`, `draw_text` and a single
`poll_input_events`. Those names are not in the tree; the entries above are what the
code actually exports.

## Code Layout

```
epsilon-os/
├── Cargo.toml              # crate name: aegis_os
├── .cargo/config.toml
├── linker.ld
├── limine.cfg / limine.conf
├── build_iso.sh            # produce aegis_os.iso
├── run_qemu.sh             # build + boot graphically
├── run_e2e_tests.sh        # build + run the QEMU E2E suite
├── run_selftest.sh         # build with --features selftest + run in-kernel tests
├── src/
│   ├── main.rs             # Kernel entrypoint (_start), compositor loop
│   ├── arch/
│   │   ├── mod.rs          # InterruptGuard
│   │   ├── gdt.rs          # GDT & TSS configuration
│   │   ├── idt.rs          # IDT vectors & naked ISR stubs
│   │   ├── time.rs         # TSC calibration & 60 FPS FramePacer
│   │   └── serial.rs       # 16550 UART driver & print macros
│   ├── memory/
│   │   ├── frame.rs        # Physical bitmap frame allocator
│   │   ├── heap.rs         # Kernel heap allocator
│   │   └── paging.rs       # PML4 virtual address spaces & HHDM
│   ├── task/
│   │   ├── pcb.rs          # Process control block & state
│   │   ├── scheduler.rs    # Preemptive round-robin scheduler
│   │   ├── context.rs      # Context switch assembly routines
│   │   └── fault.rs        # Ring 3 fault detection & 2-phase reaping
│   ├── drivers/
│   │   ├── framebuffer.rs  # Linear RGB double-buffered driver & clipping
│   │   ├── ps2_keyboard.rs # PS/2 keyboard driver & scancodes
│   │   ├── ps2_mouse.rs    # PS/2 mouse driver & cursor tracking
│   │   ├── ring.rs         # Preallocated EventRing (ISR-safe queue)
│   │   └── speaker.rs      # PC speaker / PIT channel 2 audio
│   ├── gui/
│   │   ├── font.rs         # 8x16 bitmap font + supplementary glyphs & icons
│   │   ├── primitives.rs   # 2D drawing primitives & color math
│   │   ├── menubar.rs      # Top menu bar
│   │   ├── dock.rs         # Launcher dock & AppId
│   │   ├── window.rs       # Window structure, widgets, SnapTarget
│   │   ├── wm.rs           # Window manager, Z-order, snapping
│   │   ├── wallpaper.rs    # Procedural themes & PPM wallpapers
│   │   └── spotlight.rs    # Ctrl+Space universal search
│   ├── fs/mod.rs           # In-memory RAM disk VFS
│   ├── net/mod.rs          # IPv4/UDP loopback stack
│   ├── agent/mod.rs        # Ring 0 agent bridge over serial
│   ├── selftest/mod.rs     # 14 in-kernel suites (--features selftest)
│   └── apps/               # 14 applications (see README.md)
├── tests/
│   ├── qemu_e2e/           # Python harness driving the real ISO in QEMU
│   └── e2e/                # Historical host-std design model (not linked)
└── docs/                   # Screenshots
```
