# AegisOS E2E Testing Infrastructure & Specification

**Document Version:** 1.0.0  
**Author:** E2E Test Writer (`e2e_test_writer_1`)  
**Scope:** 4-Tier Opaque-Box E2E Testing Framework, Test Harness, and Coverage Matrix  
**Target Architecture:** `x86_64` (Limine Boot Protocol, Ring 0/Ring 3 Isolation, Linear Double-Buffered GUI)

---

## 1. Executive Summary & Testing Philosophy

AegisOS is an x86_64 operating system designed with a core guarantee: **hardware-enforced fault isolation and crash resilience**. Userspace application crashes (such as Page Faults, Divide-by-Zero, Invalid Opcodes, and Out-of-Bounds writes) must terminate only the faulting process without crashing the kernel, freezing the desktop environment, or corrupting memory of other applications.

### 1.1 Automated Bare-Metal QEMU E2E Suite (`tests/qemu_e2e/`)
The primary regression and acceptance test suite for AegisOS boots the real bare-metal `aegis_os.iso` in QEMU (with KVM/TCG acceleration) and verifies system operations end-to-end:
- **Headless QEMU Orchestration**: Managed via `tests/qemu_e2e/harness.py`.
- **Serial Console Telemetry**: Parses COM1 UART logs for Limine boot milestones, memory allocator footprint (<60MB), and fault isolation tags.
- **QEMU Monitor Integration**: Drives interactive PS/2 mouse movements, button clicks, and keyboard strokes.
- **PPM Framebuffer Verification**: Captures `screendump` PPM buffers and validates non-flat color variety, palette diversity, and GUI chrome rendering.
- **CPU Register & Interrupt Stability**: Queries `info registers` to assert `RFLAGS.IF` remains enabled and `RIP` advances under 560-event input flood conditions.

Run the bare-metal E2E test suite with:
```sh
./run_e2e_tests.sh
```

### 1.2 Historical Host Simulation Mock Suite (`tests/e2e/`)
Note: `tests/e2e/` contains a historical 4-tier design simulation implemented in standard Rust (`std`) modeling kernel interfaces on the host platform.


```
+---------------------------------------------------------------------------------------------------+
|                                 AegisOS 4-Tier E2E Test Suite                                     |
+---------------------------------------------------------------------------------------------------+
|  +---------------------------------------------------------------------------------------------+  |
|  | Tier 1: Feature Coverage (61 Tests)                                                         |  |
|  | F1: Limine Boot (5)        | F2: Serial UART (5)        | F3: GDT/TSS/IDT (5)               |  |
|  | F4: Bitmap & Heap (5)      | F5: PML4 Paging (5)        | F6: Ring 3 Faults (5)             |  |
|  | F7: Scheduler (5)          | F8: Compositor (5)         | F9: PS/2 Input (5)                |  |
|  | F10: Window Manager (5)    | F11: 5 Core Apps (6)       | F12: ISO & QEMU (5)               |  |
|  +---------------------------------------------------------------------------------------------+  |
|  +---------------------------------------------------------------------------------------------+  |
|  | Tier 2: Boundary & Corner Cases (61 Tests)                                                  |  |
|  | Zero/Negative Inputs, Null Pointers, 4GB RAM Exhaustion, Screen Clamping, Corrupted Packets,  |  |
|  | 1000-Task Runqueue Stress, 100x Crash Bursts, 1000-Line Editor Buffers, Zero-Sized Rects    |  |
|  +---------------------------------------------------------------------------------------------+  |
|  +---------------------------------------------------------------------------------------------+  |
|  | Tier 3: Cross-Feature Combinations (8 Complex Tests)                                        |  |
|  | Crash during Window Drag | Terminal Stream & Activity Monitor | Editor under Memory Load    |  |
|  | Terminal Spawn -> Monitor Kill | Input during Preemption | 60 FPS Compositor during Faults  |  |
|  +---------------------------------------------------------------------------------------------+  |
|  +---------------------------------------------------------------------------------------------+  |
|  | Tier 4: Real-World Application Scenarios (5 Full Workflows)                                 |  |
|  | Scenario 1: Desktop Multitasking & Crash Resilience Full Lifecycle (5 Apps + Crash Recovery)|  |
|  | Scenario 2: High-Concurrency Stress & Simultaneous Fault Recovery (10 Worker Tasks)        |  |
|  | Scenario 3: Interactive Terminal Shell & Process Management (ps, kill, free, run, history)  |  |
|  | Scenario 4: GUI Compositor, Windowing & Visual Fidelity (Dragging, Traffic Lights, Dock)   |  |
|  | Scenario 5: System Specs & Memory Footprint Budget Verification (< 60MB RAM Constraint)     |  |
|  +---------------------------------------------------------------------------------------------+  |
+---------------------------------------------------------------------------------------------------+
```

---

## 2. Test Architecture & Directory Layout

The historical host-simulation suite resides in `tests/e2e/`, relative to the
repository root:

```
tests/e2e/
├── Cargo.toml                  # Test package configuration & target bindings
├── lib.rs                      # Test library root exposing test harness
├── runner.rs                   # Standalone CLI test runner binary (e2e_runner)
├── tier1_features.rs           # Tier 1: Feature Coverage (61 tests)
├── tier2_boundary.rs           # Tier 2: Boundary & Corner Cases (61 tests)
├── tier3_combinations.rs       # Tier 3: Cross-Feature Combinations (8 tests)
├── tier4_scenarios.rs          # Tier 4: Real-World Workflows (5 tests)
└── test_harness/
    ├── mod.rs                  # Module index
    ├── types.rs                # Core types: PhysAddr, VirtAddr, Color, Rect, ProcessInfo, etc.
    ├── memory_sim.rs           # 128KB Bitmap Frame Allocator & PML4 4-Level Paging Simulator
    ├── privilege_sim.rs        # GDT, TSS RSP0/IST1, 256-Vector IDT & UART Serial Simulator
    ├── scheduler_sim.rs        # 100Hz Round-Robin Scheduler, PCB Table & 2-Phase Zombie Reaper
    ├── gui_sim.rs              # 1024x768x32 Double-Buffered Compositor, Blitter & Font Renderer
    ├── input_sim.rs            # PS/2 Set 1 Keyboard & 3-Byte Mouse Packet Decoders
    ├── wm_sim.rs               # macOS Window Manager (24px Top Bar, Floating Windows, Dock)
    ├── apps_sim.rs             # 5 System Apps (CrashTest, ActivityMonitor, Terminal, Pad, About)
    └── os_kernel_env.rs        # Integrated Unified AegisOS Kernel Simulation Environment
```

---

## 3. Test Tier Breakdown & Methodology

### 3.1 Tier 1: Feature Coverage (>=5 Tests per Feature F1..F12)

| Feature ID | Feature Name | Test Count | Key Verification Points |
|---|---|---|---|
| **F1** | Limine Bootloader & Target Config | 5 | Higher-half canonical entry `0xFFFFFFFF80100000`, protocol request headers, 1024x768x32 framebuffer layout, Limine memory map parsing, HHDM `0xFFFF_8000_0000_0000`. |
| **F2** | Serial Console & Diagnostic Panic | 5 | COM1 `0x3F8` UART init at 115200 8N1, `print!`/`println!` formatted output, multiline buffering, panic diagnostics (file, line, registers), tag filtering (`[BOOT]`, `[FAULT]`, `[KERNEL]`, `[SCHED]`). |
| **F3** | GDT, TSS & IDT Privilege Architecture | 5 | Kernel CS (0x08), Kernel DS (0x10), User DS (0x1B), User CS (0x23), TSS `RSP0` stack switching, IST1 double-fault stack, 256 IDT gates, privilege detection (`CS & 3 == 3`). |
| **F4** | Physical Bitmap & Kernel Heap Allocators | 5 | 128KB bitmap managing 1,048,576 frames (4GB RAM), single frame alloc/free, contiguous frame allocation, double-free protection, idle RAM footprint verification (< 60MB). |
| **F5** | 4-Level PML4 Virtual Address Spaces | 5 | Higher-half supervisor mapping (`!PTE_USER`), user lower-half mapping (`PTE_USER`), privilege enforcement (Ring 3 access to Ring 0 memory causes `#PF`), write-protection enforcement, per-process PML4 isolation. |
| **F6** | Ring 3 Fault Isolation & Crash Resilience | 5 | Null pointer dereference isolation (`0x0`), divide-by-zero isolation (`100/0`), supervisor OOB write isolation, invalid opcode (`ud2`) isolation, 2-phase deferred zombie frame reclamation. |
| **F7** | Preemptive Multitasking Scheduler | 5 | 100Hz round-robin runqueue rotation, priority tier dispatch (High/Normal/Low), PID 0 [idle] task termination immunity, CPU% telemetry calculation, process table query (`get_process_list`). |
| **F8** | Linear RGB Double-Buffered Compositor | 5 | 1024x768 front/back buffer initialization, dirty rectangle scanline blitting, alpha blending formula $((src \cdot \alpha) + (dst \cdot (255 - \alpha))) / 255$, 2D primitives (`draw_rounded_rect`, `draw_circle`, `draw_gradient_v`), embedded 8x16 font rasterizer. |
| **F9** | PS/2 Mouse & Keyboard Drivers | 5 | Set 1 scancode translation to ASCII, Shift and CapsLock state transitions, extended `0xE0` arrow keys, 3-byte mouse packet parsing with sign extension, mouse cursor coordinate clamping. |
| **F10** | macOS Desktop & Window Manager | 5 | 24px top menu bar telemetry (logo, active app, uptime clock, CPU/RAM badges), floating window creation/layering, draggable titlebar with screen clamping, traffic-light button actions (Close/Minimize/Maximize), Z-order focus cycling on click. |
| **F11** | 5 Core System Applications | 6 | Crash-Test Demo fault triggers, Activity Monitor telemetry and `[Kill Process]`, Terminal Shell builtins (`ps`, `free`, `echo`), CLI process lifecycle (`run` and `kill`), AegisPad multiline text editing/cursor, About Dialog specifications. |
| **F12** | Automated Build Pipeline & QEMU Runner | 5 | Linker script structure (`.limine_requests` at `0xFFFFFFFF80100000`), `limine.cfg` syntax, hybrid ISO file structure, `run_qemu.sh` launch arguments, QEMU serial telemetry assertion. |

**Total Tier 1 Tests:** 61 tests

---

### 3.2 Tier 2: Boundary & Corner Cases (>=5 Tests per Feature F1..F12)

Tier 2 stresses system edge conditions, extreme parameters, and resource boundaries:
- **F1 Boundaries**: Zero physical RAM in memory map, non-canonical entry detection, 64-bit address space limits, unaligned framebuffer pitch, extreme HHDM offsets.
- **F2 Boundaries**: 10,000-byte UART burst writes, null bytes and control chars, rapid consecutive newlines, multiline panic stack traces, UART ring buffer clear/reuse.
- **F3 Boundaries**: Uninitialized TSS `RSP0` detection, IDT vector 255 upper limit, RFLAGS reserved bit handling, DPL=3 user-callable gates, out-of-range IST indices.
- **F4 Boundaries**: 100% frame exhaustion (out-of-memory), zero-count contiguous alloc, single request exceeding 4GB RAM, unaligned frame free attempt, freeing address beyond 4GB physical space.
- **F5 Boundaries**: Mapping zero page with User permissions, unmapped page translation fault (`present: false`), unmap and re-translation fault, unaligned virtual/physical address mapping rejection, non-canonical address mapping rejection.
- **F6 Boundaries**: Rapid 100x crash burst stress test, page fault at offset 4095 page boundary, multiple concurrent crashed tasks queued for reaping, Ring 0 kernel task fault triggering diagnostic panic instead of user isolation, fault on non-existent PID.
- **F7 Boundaries**: Scheduler runqueue under 1,000 tasks, rapid spawn/kill cycle of 500 tasks, kill on non-existent PID, all tasks blocked falling back to PID 0 [idle], zero-runtime CPU% edge.
- **F8 Boundaries**: Negative coordinates (-50, -50) drawing clipping, screen maximum bounds (1024, 768) and beyond (2000, 3000), zero-dimension rectangle drawing, 100% transparent (alpha=0) and 100% opaque (alpha=255) alpha blending, unprintable ASCII fallback.
- **F9 Boundaries**: Corrupted 3-byte mouse packet with bit 3 clear (rejected), extreme signed deltas (+127, -128), cursor clamping to screen edges (0..1023, 0..767), unmapped keyboard scancode `0xFF`, rapid 500-scancode burst.
- **F10 Boundaries**: Window dragging clamped so window cannot be lost off-screen, 50-window overlapping Z-stack stress, closing non-existent window, clicking empty desktop outside windows, maximize/restore geometry cycle.
- **F11 Boundaries**: Terminal 1,000-char single-line command buffer overflow, empty input execution, `kill 0` blocked with error, AegisPad backspace at (0, 0), AegisPad 1,000-line buffer stress, Activity Monitor 100% CPU spike telemetry.
- **F12 Boundaries**: ISO catalog validation, QEMU boot with minimum 512MB RAM vs 4GB RAM, serial log line truncation safety, headless timeout watchdog, 10 consecutive reboot cycles.

**Total Tier 2 Tests:** 61 tests

---

### 3.3 Tier 3: Cross-Feature Combinations (8 Complex Tests)

1. **`test_tier3_01_crash_during_window_drag`** (`F6` + `F10` + `F9`):
   Simulates a user dragging the Crash-Test window while clicking a fault button. Verifies that the fault is caught, window drag state is cleanly extinguished, window is closed, memory is reclaimed, and mouse cursor / desktop remain responsive.
2. **`test_tier3_02_activity_monitor_with_terminal_command_stream`** (`F11.2` + `F11.3` + `F7` + `F4`):
   Activity Monitor continuously samples CPU and process tables while the Terminal Shell executes rapid continuous commands (`ps`, `free`, `echo`, `run`). Verifies zero race conditions, telemetry synchronization, and memory accounting consistency.
3. **`test_tier3_03_editor_under_high_memory_allocation_pressure`** (`F11.4` + `F4` + `F5` + `F8`):
   AegisPad executes multiline typing while the system allocates and frees 5,000 physical frames. Verifies text buffer integrity, zero memory corruption, and correct compositor blits.
4. **`test_tier3_04_terminal_spawn_and_activity_monitor_kill`** (`F11.3` + `F11.1` + `F11.2` + `F7`):
   Terminal spawns Crash-Test task via `run crashtest`, Activity Monitor discovers the PID, user selects the row and clicks `[Kill Process]`, and Terminal `ps` confirms termination and frame reclamation.
5. **`test_tier3_05_mouse_drag_keyboard_typing_during_preemptive_interrupts`** (`F9` + `F10` + `F8` + `F7`):
   User rapidly types into Terminal while moving mouse cursor as 100Hz timer IRQs preempt tasks. Verifies event dispatch routing and no dropped characters.
6. **`test_tier3_06_compositor_60fps_telemetry_during_user_faults`** (`F8` + `F10` + `F6` + `F4`):
   Compositor renders top menu bar telemetry at 60 FPS while kernel catches multiple Ring 3 faults and reclaims zombie frames.
7. **`test_tier3_07_terminal_free_matches_activity_monitor_under_60mb`** (`F11.2` + `F11.3` + `F4`):
   Verifies that Terminal `free` CLI command output matches Activity Monitor memory telemetry and validates that idle RAM usage remains strictly < 60 MB RAM.
8. **`test_tier3_08_window_close_traffic_light_during_active_scheduler_loop`** (`F10` + `F7` + `F6` + `F4`):
   User clicks red traffic-light close button while application process is in active running state. Verifies PCB transition to Zombie, deferred frame freeing, and seamless context switch.

---

### 3.4 Tier 4: Real-World Application Scenarios (5 Multi-Step Workflows)

1. **Scenario 1: Desktop Multitasking & Crash Resilience Full Lifecycle**:
   Boot AegisOS -> verify initial telemetry (< 60MB RAM) -> launch all 5 apps from Dock -> verify 5 windows open and layered in Z-order -> switch focus among all windows -> trigger Null Pointer crash in Crash-Test -> verify Crash-Test window cleanly terminates and closes -> verify Activity Monitor updates PID list and reclaims memory -> verify AegisPad and Terminal continue running without freeze -> type text in AegisPad -> run `ps` and `free` in Terminal -> graceful exit.
2. **Scenario 2: High-Concurrency Stress & Fault Recovery Workflow**:
   Launch Terminal -> spawn 10 background worker tasks via CLI -> monitor memory and CPU in Activity Monitor -> trigger multiple simultaneous faults (#DE, #UD, #PF) in Crash-Test tasks -> verify kernel catches all faults, logs to serial UART, reclaims frames -> verify Terminal and Activity Monitor remain 100% responsive.
3. **Scenario 3: Interactive Terminal Shell & Process Management Workflow**:
   Launch Terminal -> verify prompt `aegis:~$` -> run `help`, `ps`, `free`, `echo "AegisOS isolation verified"` -> run `run crashtest` -> check `ps` shows new PID -> run `kill <pid>` -> check `ps` confirms termination and frame reclamation -> test command history (Up/Down arrow navigation) -> test `clear`.
4. **Scenario 4: GUI Compositor, Windowing & Visual Fidelity Workflow**:
   Drag windows across screen boundaries -> test minimizing window into dock and restoring by clicking dock icon -> test maximizing window with green traffic light button -> test close with red button -> verify double-buffered backbuffer swap without tearing -> verify alpha blending on translucent dock and shadows -> verify font rendering for full ASCII range.
5. **Scenario 5: Memory Budget & System Specs Validation Workflow**:
   Open "About AegisOS" modal dialog -> verify kernel version, Limine protocol, x86_64 Long Mode, and memory specs -> query memory via Activity Monitor and Terminal `free` -> assert total memory usage is strictly < 60 MB RAM at idle -> verify no memory leaks over 1,000 compositor frames.

---

## 4. Test Invocation & Verification Commands

### 4.1 Running the Full E2E Test Suite via Cargo
```bash
export PATH="$HOME/.cargo/bin:$PATH"
cargo test --manifest-path tests/e2e/Cargo.toml
```

### 4.2 Running Specific Test Tiers
```bash
# Tier 1 (Feature Coverage)
cargo test --manifest-path tests/e2e/Cargo.toml --test tier1_features

# Tier 2 (Boundary & Corner Cases)
cargo test --manifest-path tests/e2e/Cargo.toml --test tier2_boundary

# Tier 3 (Cross-Feature Combinations)
cargo test --manifest-path tests/e2e/Cargo.toml --test tier3_combinations

# Tier 4 (Real-World Scenarios)
cargo test --manifest-path tests/e2e/Cargo.toml --test tier4_scenarios
```

### 4.3 Running via Standalone CLI Runner Binary
```bash
cargo run --manifest-path tests/e2e/Cargo.toml --bin e2e_runner
cargo run --manifest-path tests/e2e/Cargo.toml --bin e2e_runner -- --tier 1
cargo run --manifest-path tests/e2e/Cargo.toml --bin e2e_runner -- --tier 2
cargo run --manifest-path tests/e2e/Cargo.toml --bin e2e_runner -- --tier 3
cargo run --manifest-path tests/e2e/Cargo.toml --bin e2e_runner -- --tier 4
cargo run --manifest-path tests/e2e/Cargo.toml --bin e2e_runner -- --json
```

### 4.4 QEMU Headless Integration Verification
```bash
./run_e2e_tests.sh    # builds the ISO if stale, then runs the QEMU E2E suite
./run_selftest.sh     # builds with --features selftest, exits via isa-debug-exit
```

`run_qemu.sh` takes no arguments — it always builds and boots graphically. Earlier
revisions of this document showed `./run_qemu.sh --headless --test-assert`; those flags
have never existed. For a headless boot without the test harness, invoke QEMU directly:

```bash
qemu-system-x86_64 -cdrom aegis_os.iso -m 4G -accel kvm -vga std \
                   -display none -serial stdio
```

---

## 5. Coverage Statistics & Metrics

### 5.1 Suites that exercise the kernel

- **QEMU E2E suite (`tests/qemu_e2e/`)**: 22 test functions across 20 modules, driving
  the real `aegis_os.iso`. Run with `./run_e2e_tests.sh`.
- **In-kernel self-tests (`src/selftest/`)**: 14 bare-metal suites behind
  `--features selftest`. Run with `./run_selftest.sh`.
- **Memory constraint verified by boot**: 16 MB used of 3064 MB at idle desktop,
  inside the < 60 MB budget.

HANDOFF.md records these at 22/22 and 14/14 as of milestone 17. Run the scripts for
numbers from your own checkout rather than citing that figure as current.

### 5.2 Historical model (`tests/e2e/`) — case counts only, not a pass rate

- Tier 1: 61 cases (>= 5 per feature F1..F12)
- Tier 2: 61 cases (>= 5 per feature F1..F12)
- Tier 3: 8 pairwise interaction cases
- Tier 4: 5 multi-step workflow cases

Earlier revisions reported these 135 cases as a "100% pass rate" covering R1–R6 and
F1–F12. They cover models of those features, written in host `std` Rust; no file in
`tests/e2e/` references the kernel crate, so the suite passed while the kernel
deadlocked on boot. The counts are kept as a description of the design; the pass rate
has been removed. See section 3 of TEST_READY.md.
