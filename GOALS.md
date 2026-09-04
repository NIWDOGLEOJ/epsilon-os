# Project Goals

The target this project is aiming at, as stated by the author:

1. Run on old hardware while still supporting modern software.
2. Support Intel and Ryzen CPUs released after 2020.
3. Be smooth and well optimized.
4. When an application crashes, the OS must not crash.
5. Use system resources efficiently.
6. Support all Windows-based applications.
7. Be optimized the way macOS is optimized.

This file records those goals and, next to each, what the kernel in this
repository does today. The gap is large in places. Keeping the two columns
side by side is deliberate — this project has already been burned once by
documentation that reported success the code had not achieved (see
`TEST_READY.md` §3 and the opening of `HANDOFF.md`).

## Status against each goal

| Goal | Where the code actually is |
|---|---|
| 1. Old hardware, modern software | Not started. The blocker is drivers, not design — see the driver gap below. |
| 2. Intel & Ryzen 2020+ | Boots x86_64 under Limine with BIOS/UEFI support, which is the right foundation. But verified only in QEMU, and the driver set targets 1995-era peripherals. |
| 3. Smooth and well optimized | True where it has been measured: TSC-calibrated pacing to a 16.667 ms frame budget, all software-rendered. No GPU acceleration, so this holds at 1280x800 in a VM and is untested at modern panel resolutions. |
| 4. App crash ≠ OS crash | **The mechanism works and is verified. No real application uses it yet.** See below — this is the most important gap to close. |
| 5. Efficient resource usage | Genuinely strong: 16 MB used of 3064 MB at idle desktop. Offset by using one CPU core; there is no SMP support. |
| 6. Support all Windows applications | No foundation exists yet. There is no syscall interface, no program loader, and no Win32 surface of any kind. This is the largest goal on the list by a wide margin. |
| 7. macOS-like optimization | The desktop follows macOS visually. In the engineering sense — GPU compositing, unified memory handling, power management — none of that is present. |

## Goal 4 is closer than it looks, and further than it reads

Ring 3 fault isolation genuinely works. A userspace process that dereferences
null, divides by zero, executes `ud2`, or writes into kernel space is trapped
by hardware, logged with vector, RIP, error code and CR2, reaped, and its
frames returned to the bitmap allocator. The desktop keeps compositing. This
is real, and it is the hardest part of goal 4.

But the fourteen applications do not run in Ring 3. They are kernel structs
(`AppSuite`, `src/apps/mod.rs`) whose `render` and input handlers are called
directly from the compositor loop in `src/main.rs`, in Ring 0. The four
"app tasks" spawned at boot — `crash_test_task_entry` and friends — are
passed `is_user: false` and do nothing but `spin_loop()` after printing one
line. The only true Ring 3 processes in the system are the hand-assembled
byte arrays in `spawn_user_fault_test` (`src/task/scheduler.rs`), which exist
to crash on purpose.

The practical consequence: **a panic in `src/apps/editor.rs` today reaches
`#[panic_handler]` and halts the machine.** Goal 4 is proven for the
demonstration and not yet true for the product.

## The two things standing between here and the rest

### A userspace ABI

There is no syscall interface. A Ring 3 process in this kernel cannot ask the
kernel for anything — it can compute and it can fault. There is also no
program loader of any kind: no ELF loader, no PE/COFF loader. Userspace code
is `include`d as literal machine-code bytes at compile time.

This one piece of work unblocks goals 4 and 6 simultaneously, which makes it
the natural next milestone:

1. A syscall entry path (`syscall`/`sysret` with `IA32_LSTAR`, or a software
   interrupt gate) with a small initial call set — write, exit, yield, and a
   framebuffer or IPC primitive for drawing.
2. An ELF64 loader, so a process is a file rather than a byte array.
3. Move one application — the Terminal is the natural candidate — across the
   boundary and prove it survives its own panic.

Until step 3 lands, goal 4 is a property of the demo rather than the system.

### The driver gap

For goal 2 in its real sense — booting on actual 2020+ Intel and Ryzen
machines rather than in QEMU — the kernel is missing every driver that a
modern machine needs. Present: 8259 PIC, PIT, PS/2 keyboard and mouse, 16550
UART, PC speaker, Limine-provided linear framebuffer. Absent:

| Missing | Why it matters on 2020+ hardware |
|---|---|
| ACPI | Enumerating hardware, shutdown, sleep, thermal. Effectively mandatory. |
| APIC / x2APIC | The 8259 is emulated by modern chipsets today, but that emulation is a compatibility courtesy, not a guarantee. SMP requires the APIC regardless. |
| PCIe enumeration | Nothing can find a device that is not on the legacy ISA map. |
| USB (xHCI) + HID | The input problem. Most machines built after 2020 have no PS/2 port. Firmware "USB legacy emulation" makes a USB keyboard look like an i8042 device and may carry this kernel far enough to boot on some machines, but it is firmware-dependent, commonly disabled once the OS takes over, and unreliable for mice. It is not something to build on. |
| AHCI / NVMe | There is no storage driver, so there is no persistence. The VFS is a RAM disk and everything in it is gone at reboot. |
| GPU / display | The Limine framebuffer works, but every pixel is composited on the CPU. This is fine at 1280x800 and will not hold at 4K. |
| SMP | A 2020+ Ryzen has 6-16 cores. This kernel uses one. |
| Network hardware | The stack is loopback-only; no NIC driver exists, so nothing leaves the machine. |

Realistically, USB/xHCI and NVMe are each substantial projects on their own.

## A note on goal 6

"Support all Windows-based applications" deserves a straight answer rather
than a plan.

Running Windows binaries requires, at minimum: a PE/COFF loader, an
implementation of the Win32 API surface, the NT syscall layer beneath it, a
registry, NTFS, a working GPU stack for anything graphical, DirectX, plus
audio, networking and printing. Wine has been building exactly this since
1993, with thousands of contributors, and does not support *all* Windows
applications today.

Wine also has an advantage this project does not: it runs on Linux, so it
inherits Linux's drivers, GPU stack, filesystem, scheduler and network
stack, and only has to solve the compatibility layer. Doing it on a
from-scratch kernel means building everything underneath it first — every
row of the driver table above — before the compatibility work can start.

That is not an argument against the goal. It is an argument about ordering
and about what "done" is allowed to mean. A more reachable framing of the
same intent, in the order the dependencies actually fall:

1. Get one application into Ring 3 and prove goal 4 for real code.
2. Add ACPI, APIC and PCIe, then boot on one specific physical machine
   rather than "any" machine.
3. Add xHCI/HID so that machine has a keyboard, and NVMe so it has a disk.
4. Port a POSIX-ish subset and run one existing open-source program
   unmodified.
5. Only then evaluate what Windows compatibility would take, with real
   hardware and a real userspace underneath it.

Step 4 is where "supports modern software" starts being true in a way a user
would notice, and each step before it is independently useful.

## What is genuinely done

Worth stating plainly, because the gaps above are long and the foundation is
good:

- Hardware Ring 0/Ring 3 privilege separation, with fault isolation that
  works and has been verified by booting rather than by assertion.
- 4-level PML4 paging with per-process address spaces and two-phase zombie
  frame reclamation.
- A 100 Hz preemptive scheduler with correct context switching.
- A 16 MB idle footprint, which is a real achievement against goal 5.
- Two test suites that exercise the actual kernel (`tests/qemu_e2e/`,
  `src/selftest/`), replacing a model suite that had reported success while
  the kernel deadlocked on boot.

The kernel core is sound. What is missing is breadth, not depth.
