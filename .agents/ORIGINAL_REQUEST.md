# Original User Request

## Initial Request — 2026-08-30T12:04:54Z

Build **AegisOS**, a lightweight, highly stable x86_64 operating system in Rust (`no_std`) featuring hardware-enforced process isolation where application crashes (page faults, divide-by-zero, invalid opcodes) terminate only the faulting process without crashing the kernel or other running applications. The OS runs on x86_64 hardware with 4GB RAM (using < 60MB RAM at idle) with a macOS-inspired graphical desktop environment and an interactive system application suite.

Working directory: ~/teamwork_projects/aegis_os
Integrity mode: development

## Requirements

### R1. Kernel Architecture & Hardware Protection
The kernel must be built in Rust (`no_std`) for the `x86_64` architecture, booting via the Limine bootloader (supporting both legacy BIOS and modern UEFI). It must configure the Global Descriptor Table (GDT), Task State Segment (TSS), Interrupt Descriptor Table (IDT), and 4-level paging (PML4) to enforce strict hardware privilege separation between Ring 0 (Kernel) and Ring 3 (Userspace).

### R2. Fault Isolation & Crash Resilience
The kernel exception handlers for Page Faults (`#PF`, vector 14), Divide-by-Zero (`#DE`, vector 0), General Protection Faults (`#GP`, vector 13), and Invalid Opcodes (`#UD`, vector 6) must detect when an exception originates from Ring 3 userspace. Upon catching a user fault, the kernel must:
1. Log the faulting process and address.
2. Terminate the faulting task and safely reclaim its allocated memory frames.
3. Reschedule the next ready task without triggering a kernel panic or freezing the graphical desktop and other running processes.

### R3. Memory Management & Scheduling
Implement a physical memory frame allocator (using the Limine memory map), a kernel heap allocator, and per-process virtual address spaces. Include a preemptive round-robin task scheduler driven by timer interrupts.

### R4. Graphical Compositor & Desktop Environment
Implement a double-buffered linear RGB framebuffer driver supporting a macOS-inspired desktop GUI:
- A top system menu bar (24px) displaying the OS logo, active application name, uptime clock, and real-time CPU/RAM badge.
- A floating application window manager with draggable title bars, active window focus, and traffic-light close buttons.
- An application launcher dock with clickable icons.
- Mouse cursor rendering with PS/2 mouse packet tracking and keyboard input handling.

### R5. Core System Applications & Demo Suite
Implement the following graphical/terminal applications:
1. **Crash-Test Demo App**: An interactive application with buttons to trigger intentional faults (Null Pointer Dereference, Divide-by-Zero, Out-of-Bounds Memory Write, Invalid Opcode) to visually prove that clicking any fault terminates only the app while the rest of the OS continues running flawlessly.
2. **Activity Monitor**: Displays live CPU utilization, real-time memory usage graph (verifying < 60MB RAM consumption), and a process table with PID, status, and kill capability.
3. **Interactive Terminal Shell**: Command line shell supporting basic process and system utilities (`ps`, `kill`, `free`, `echo`, `run`, `clear`, `reboot`).
4. **Text Editor (AegisPad)**: Lightweight text editing window.
5. **About AegisOS Dialog**: Displays kernel build, architecture, and memory specs.

### R6. Build System & Bootable Artifacts
Provide an automated build pipeline that compiles the kernel, packages a hybrid bootable ISO image (`aegis_os.iso`), and provides a one-click QEMU launch script (`run_qemu.sh` or `cargo run`) supporting standard display and serial debug logging.

---

## Acceptance Criteria

### Crash Isolation & Fault Recovery
- [ ] Triggering a null pointer dereference in the Crash-Test Demo App terminates only that app window; the top bar, Activity Monitor, and mouse cursor remain responsive with zero kernel panic.
- [ ] Triggering a divide-by-zero exception in userspace reaps the offending process and reclaims its allocated memory in the Activity Monitor graph.
- [ ] Triggering an invalid opcode or out-of-bounds page fault in userspace logs the exception to serial output and cleanly returns execution to the scheduler.

### Desktop & User Interface
- [ ] Top menu bar, launcher dock, and floating windows render smoothly with double-buffering at 60 FPS without screen tearing.
- [ ] Windows can be dragged across the desktop, focused by clicking, and closed via the red titlebar button.
- [ ] Mouse cursor moves fluidly across the screen following PS/2 mouse inputs.

### System Performance & Footprint
- [ ] Total system memory consumption at idle desktop is under 60MB of RAM.
- [ ] Kernel boots successfully in QEMU with 512MB to 4GB RAM allocated.

### Packaging & Execution
- [ ] Running `./run_qemu.sh` (or `cargo run`) builds the kernel, packages the Limine ISO, and launches QEMU with graphical display and serial logging.
