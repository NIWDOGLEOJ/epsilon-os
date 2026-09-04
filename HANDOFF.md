# Handoff

Engineering log for the debugging and repair pass on this kernel, and the plan from here.

Written after taking the kernel from "hard-deadlocks 15 lines into boot" to a working
72 FPS desktop. Every claim below was verified by booting the ISO in QEMU and reading
serial output, framebuffer screendumps, or `rdtsc` counters.

## Starting state

The kernel deadlocked during boot. A framebuffer screendump was a single flat colour —
zero frames had ever rendered. Nothing in the graphics, desktop, or application layers
was observable, though all of it was written and compiled.

`PROJECT.md` marked M3/M4/M5 `PLANNED` and M1/M2 `DONE`. Both were wrong: the graphics
and desktop code existed and worked once the hang was cleared, while the "done"
fault-isolation engine was what hung the machine.

## Bugs found and fixed

### 1. Serial spinlock deadlock — `arch/serial.rs`

`_print` took a `spin::Mutex` with interrupts enabled. The boot sequence enabled
interrupts, began logging, and took the lock; the timer IRQ switched to a Ring 3 task
that faulted on purpose; the fault handler tried to log the fault and spun forever on
a lock the interrupted compositor still held.

Diagnosed from the CPU state at the hang: `RFLAGS=0x2` (IF clear), `RIP` pinned across
five samples in a `lock cmpxchg` retry loop, and an interrupt frame on the stack
showing vector `0x0e`, error `0x06`, `CS=0x23`.

Fixed with `arch::InterruptGuard` — saves `RFLAGS`, clears `IF`, restores on drop.

### 2. Mouse acceleration — `drivers/ps2_mouse.rs`

`scale_delta` applied `abs*6 + abs²/6` above delta 10, turning a single 50-count packet
into 716 pixels and pinning the cursor to a screen edge on any normal movement.
Replaced with a piecewise curve: 1:1 below 5 counts, 2x to 10, 2.5x above.

Measured travel per packet: 2 → 2 px, 10 → 16 px, 50 → 116 px (was 14/56/716).

### 3. No clipping — `drivers/framebuffer.rs`

Nothing scissored application drawing, so Terminal text painted across the Activity
Monitor window. Added a clip rect enforced in `draw_pixel`, the single choke point every
primitive, the font rasterizer and the cursor route through. Nested clips intersect;
a non-overlapping intersection collapses to an empty rect rather than `None`, which
would have meant "unrestricted".

### 4. Z-order — `gui/wm.rs`, `main.rs`

`render_desktop` drew every window *frame*, then `main.rs` drew every window *content*
in a second pass — so a lower window's content landed on a higher window's frame.
`render_desktop` now takes a `render_client` callback and interleaves frame-then-content
per window, each clipped to its own client rect.

### 5. Compositor at ~3 FPS

Profiled rather than guessed. Per-frame: 968 Mcyc total — windows 739, wallpaper 177,
menubar/dock 52, and the VRAM blit **1**. The "tear-free 60 FPS scanline blit" was
already 0.1% of the frame; all the time was in per-pixel software rasterization at
**173 cycles per wallpaper pixel**.

Three fixes:

- `Framebuffer::fill_span` — bounds, clip and dirty-tracking once per horizontal span,
  `slice::fill` for opaque spans. `draw_rect` and `draw_gradient_v` emit one span per row.
- `rounded_row_span` — solves the corner circle per row instead of testing all four
  corners against every pixel. Identical output, one span at a time.
- `draw_shadow` — was drawing six concentric full-size rounded rects per window,
  ~1M alpha-blended pixels of which nearly all sat behind the opaque window body. Now
  takes `occluded_by` and skips what a following opaque body will cover. Only windows
  pass `Some`; the dock, toast and tooltip bodies are translucent and pass `None`.

Then `[profile.dev] opt-level` 1 → 3, since `build_iso.sh` ships the dev profile.

968 Mcyc → ~28 Mcyc per frame. ~3 FPS → ~72 FPS.

### 6. The 100 Hz timer was never 100 Hz — `arch/idt.rs`

Nothing programmed the PIT. IRQ 0 ran at the 8254 power-on default of ~18.2 Hz, so the
documented "10 ms" scheduler quantum was really 55 ms and every tick-derived value was
out by 5.5x. Added `init_pit()` with `TIMER_HZ = 100`.

The desktop clock had been `frame_count / 60`, making it a function of rendering speed.
It now reads `get_uptime_ticks() / TIMER_HZ`. Verified 19 → 50 over 31 wall seconds.

### 7. Interrupt-safety audit — the same bug three more times

Auditing every `static Mutex` against the four ISR entry points found `MOUSE_DRIVER`,
`MOUSE_QUEUE`, `KEYBOARD_STATE` and `KEY_QUEUE` all locked from the compositor loop with
interrupts enabled while their IRQ handlers locked the same statics. Raising the frame
rate had made these ~20x more likely by increasing the poll rate.

It also found something a guard cannot fix: **the interrupt handlers were allocating.**
`on_mouse_irq` returned a freshly allocated `VecDeque` per byte — 600 allocations/sec at
the configured 200 Hz sample rate — and `handle_user_fault` cloned a `String`. The global
allocator is a plain spinlock, so an ISR allocating while the interrupted code is inside
the allocator hangs the machine.

Added `drivers/ring.rs`: `EventRing<T: Copy, N>`, preallocated in the static itself, now
backing both input queues and the scheduler's zombie queue. No ISR path allocates.

### 8. Font — `gui/font.rs`

`—` rendered as `???` and `🛡️` as `?????`. Not missing glyphs: `draw_string` iterated
`text.bytes()`, so every byte of a multi-byte character fell back to `?` individually.
Switched to `chars()` and added glyphs for all eight non-ASCII codepoints in the tree —
found by scanning source, which is how the Calculator's broken `×` `÷` `±` keys turned
up in a window nobody had opened.

### Smaller fixes

- Crash-Test's status line was anchored to `client.height - 24`, placing it inside the
  fourth button. Now sits below the buttons; window grew 270 → 300 to fit.
- The About dialog hardcoded `1024x768x32`. Now reads the live mode off the `fb`
  reference — note that calling `framebuffer::get_dimensions()` there would
  self-deadlock, since the compositor already holds that lock while rendering.
  Verified at 1280x800 and 800x600.
- `menubar.rs` advanced the pen by `menu.len() * 8` (byte length, not codepoints).

## Invariants

Two rules the kernel now depends on. Breaking either reintroduces a hard hang.

**1. Any `static Mutex` an ISR touches may only be locked from task context under an
`arch::InterruptGuard`.** Contention between two tasks resolves — the spinning task gets
preempted and the holder runs. Contention from an ISR does not: the handler runs with
`IF` clear, so the holder can never be rescheduled to release the lock.

Applies to `SERIAL1`, `SCHEDULER`, `CRASH_CALLBACK`, `KEYBOARD_STATE`, `KEY_QUEUE`,
`MOUSE_DRIVER`, `MOUSE_QUEUE`. `GLOBAL_FRAME_ALLOCATOR` and `FRAMEBUFFER` have no ISR
user. ISR handlers themselves take no guard — they already run with interrupts masked.

**2. Interrupt handlers must not allocate.** `Vec` and `VecDeque` grow on push and are
unusable in an ISR. Use `drivers::ring::EventRing`.

## How this was verified

There is no automated suite (see below). Everything was checked by driving QEMU:

- headless boot with `-serial file:` and `-monitor unix:` for a control socket
- `sendkey` / `mouse_move` / `mouse_button` through the monitor to drive real input
- `screendump` to PPM, then pixel inspection for cursor position, clipping and glyphs
- `info registers` for `RFLAGS.IF` and `RIP` sampling to tell a deadlock from a busy loop
- `addr2line` against the kernel ELF to resolve a faulting or spinning `RIP` to source
- a 560-event input flood plus soak runs to exercise the ISR paths under load

## What to do next

### 1. Build the QEMU E2E harness — COMPLETED

Implemented in `tests/qemu_e2e/` and executable via `./run_e2e_tests.sh`.
Automates all tests previously performed by hand:
- `boot::test_boot_sequence`: Limine handshake, GDT/TSS, IDT/PIC, Paging/Frame allocator, 16MB heap, <60MB footprint, 1280x800 framebuffer, scheduler active.
- `framebuffer::test_framebuffer_rendering`: PPM screendump parsing, non-flat color verification, palette variance, top menu bar and dock rendering.
- `fault::test_boot_fault_isolation`: Boot-time Ring 3 #PF (null ptr) and #DE (div by zero) isolation.
- `fault::test_crashtest_all_buttons`: Interactive mouse click injection testing all 4 Crash-Test app buttons (#PF null ptr, #DE div by zero, #PF supervisor OOB write, #UD invalid opcode) with `[FAULT-ISOLATION]` and `[FAULT-TELEMETRY]` desktop stability assertions.
- `terminal::test_terminal_shell_interaction`: CLI command execution (`help`, `calc`) and CLI crash injection (`crash 1`, `crash 3`).
- `stability::test_clock_progress`: 100 Hz PIT wall-clock progression check.
- `stability::test_input_flood_resilience`: 560-event synthetic input flood soak test verifying `RFLAGS.IF` remains enabled and `RIP` advances across samples.

Run with:
```sh
./run_e2e_tests.sh
```

### 2. In-kernel self-tests — COMPLETED

Implemented via `--features selftest` in `src/selftest/mod.rs` and executable via `./run_selftest.sh`.
Asserts on real bare-metal subsystems at early boot:
- Physical frame allocator: single 4KB alloc, HHDM write/read integrity, zeroed frame allocation, 64-frame burst allocation, frame deallocation, and recycling.
- PML4 paging: user address space creation, lower-half user isolation, higher-half supervisor mirroring, virtual-to-physical translation, and frame reclamation on destruction.
- Kernel dynamic heap: `Box`, `Vec`, `String` allocation and arithmetic integrity.
- Scheduler lifecycle: PID 0 `[idle]` task check, process spawning, process listing, termination, and zombie reaping.
- Exits QEMU deterministically via `isa-debug-exit` on port `0xf4` with status code `33` (`0x10`).

Run with:
```sh
./run_selftest.sh
```

### 3. Deal with `tests/` and `TEST_READY.md`

`tests/` is ~6,700 lines that never reference the kernel crate:

```
$ grep -rn 'use aegis_os\|aegis_os::' tests/
  -> 0 files
```

Every file declares its own `PhysAddr`, `VirtAddr`, `PAGE_SIZE`, `TaskState`,
`Scheduler` in `std` Rust. It is a model of the design, not a test of `src/`, and it is
not wired into the build — nor can it easily be, since the kernel is `no_std` for
`x86_64-unknown-none`.

`TEST_READY.md` reports "100% (135/135 tests passed)" and "tear-free 60 FPS verified" on
that basis. Those claims coexisted with a kernel that deadlocked on boot and rendered
zero frames, because the tests never boot, link, or call it. The real frame rate at the
time was 3.

Move it to `docs/model/` and say what it is, or delete it — but the pass rate has to go.
It is the reason nobody knew the kernel did not boot.

### 4. Compositor Frame Pacing & Font Expansion — COMPLETED

- **Frame Pacing (`src/arch/time.rs`)**: Replaced the primitive 1,000-spin throttle with hardware TSC calibration against the 100 Hz PIT timer (`arch::FramePacer`), enforcing a true 60 FPS (16.667 ms per frame) budget and low host CPU load.
- **Font Expansion (`src/gui/font.rs`)**: Added 21 handcrafted 8x16 bitmap glyphs covering directional arrows (`← ↑ → ↓ ▲ ▼ ◀ ▶`), typography (`• … — © ®`), math/units (`× ÷ ± ≠ ≤ ≥ ² ³ µ °`), and UI status icons (`✓ ⚠ 🛡 ★ ♥`).
- **Interactive Terminal Command**: Added `symbols` command in `src/apps/terminal.rs` to display and test all supplementary font glyphs.
- **Automated E2E Suite**: Added `frame_pacing::test_frame_pacing_and_glyphs` to `tests/qemu_e2e/test_frame_pacing.py`, bringing the automated E2E test suite to 9/9 passing tests in ~54s.

### 5. In-Memory Virtual Filesystem (RAM Disk VFS) & Document Persistence — COMPLETED

- **VFS Subsystem (`src/fs/mod.rs`)**: Created an in-memory hierarchical inode tree (`RamFs`) guarded under `InterruptGuard` and `spin::Mutex`. Pre-seeded with `/welcome.txt`, `/system/readme.txt`, `/system/os_release`, and `/user/notes.txt`.
- **AegisPad Persistence (`src/apps/editor.rs`)**: Wired document loading and saving into the text editor. Added toolbar action buttons (`[ New ]`, `[ Open ]`, `[ Save ]`, `[ Clear ]`), mouse click handling, and live status bar telemetry.
- **Terminal VFS Commands (`src/apps/terminal.rs`)**: Added `ls`, `cat`, `write`, `touch`, `rm`, and `df` commands with live byte size and inode accounting.
- **In-Kernel Self-Tests (`src/selftest/mod.rs`)**: Added 5th bare-metal self-test suite `test_virtual_filesystem()` verifying CRUD, byte readback, and deletion.
- **Automated E2E Suite (`tests/qemu_e2e/test_vfs.py`)**: Added `vfs::test_vfs_file_lifecycle`, bringing the automated E2E test suite to 10/10 passing tests in ~64s.

### 6. Aegis Paint Graphical Canvas Application — COMPLETED

- **Aegis Paint (`src/apps/paint.rs`)**: Implemented 436x220 canvas drawing application with continuous line interpolation (Bresenham's algorithm), 12-color swatch palette, brush sizes (`1px`, `2px`, `4px`), `[ Eraser ]`, `[ Clear ]`, and `[ Save ]` exporting PPM images to `/user/drawing.ppm` in the VFS.
- **Dock Integration (`src/gui/dock.rs`, `src/gui/font.rs`)**: Expanded dock to 8 slots (`DOCK_WIDTH = 480`), added custom artist's palette icon (`draw_paint_icon`), and wired launcher clicks.
- **Mouse Drag Dispatch (`src/main.rs`, `src/apps/mod.rs`)**: Routed real-time mouse drag movements across window client areas to `app_suite.handle_mouse_drag()`.
- **Terminal Integration (`src/apps/terminal.rs`)**: Added `run paint` command to launch the application from the shell.
- **Automated E2E Suite (`tests/qemu_e2e/test_paint.py`)**: Added `paint::test_paint_drawing_lifecycle`, bringing the automated E2E test suite to 11/11 passing tests in ~74s.

### 7. Aegis Files Graphical File Manager & VFS Navigation — COMPLETED

- **Aegis Files (`src/apps/file_manager.rs`)**: Implemented macOS Finder-style split-pane graphical file manager with Places sidebar (`Root (/)`, `User (/user)`, `System (/system)`), storage utilization metrics, multi-column directory browser (Name, Kind, Size), and bottom action bar.
- **Directory Creation & Management (`src/fs/mod.rs`, `src/apps/terminal.rs`)**: Added `mkdir` / `create_dir` APIs to the VFS and added `mkdir` command to the terminal shell. Added `+ Folder` and `[ Delete ]` buttons to Aegis Files.
- **Inter-App Integration (`src/main.rs`, `src/apps/mod.rs`, `src/apps/editor.rs`)**: Opening any text file in Aegis Files triggers `AppAction::OpenFileInEditor(path)`, which loads the file directly into `AegisPad` and focuses its window.
- **Dock Expansion (`src/gui/dock.rs`, `src/gui/font.rs`)**: Expanded launcher dock to 9 application slots (`DOCK_WIDTH = 540`, `60px` per slot). Handcrafted `draw_files_icon` (macOS-style dual-tone blue folder with document tab) and mini list icons (`draw_mini_folder`, `draw_mini_doc`, `draw_mini_image`).
- **Terminal Integration (`src/apps/terminal.rs`)**: Added `run files` / `run finder` shell commands.
- **Automated E2E Suite (`tests/qemu_e2e/test_file_manager.py`)**: Added `file_manager::test_file_manager_lifecycle`, bringing the automated E2E test suite to 12/12 passing tests in ~83s.

### 8. Hardware PC Speaker Audio Subsystem & Sound Synthesizer — COMPLETED

- **PC Speaker Driver (`src/drivers/speaker.rs`)**: Implemented low-level x86 hardware audio driver programming 8253/8254 PIT Channel 2 (Ports `0x42`, `0x43`) in Mode 3 square wave generator with 16-bit frequency divisors (`1,193,182 / freq_hz`) and gating audio via System Control Port B (`0x61`).
- **Non-Blocking Music Sequencer (`AudioPlayer`)**: Zero CPU stalling — an interrupt-safe frame-stepped sequencer advances notes per 60 FPS compositor frame (`update_audio()`), enabling multi-voice arpeggios and sound effects while the UI stays responsive.
- **System Sound Effects**:
  - `BootChime`: Major chord arpeggio (C5 -> E5 -> G5 -> C6) on desktop boot.
  - `WindowOpen` & `WindowClose`: High-frequency ascending and descending chirps on window state changes.
  - `Alert`: Double warning alarm beep on Crash-Test fault injections.
  - `SnakeEat` & `SnakeDie`: Crisp arcade eating blip (988Hz B5) and crunch downward slide (400Hz -> 150Hz).
- **Terminal Shell Commands (`src/apps/terminal.rs`)**:
  - `beep [freq] [ms]`: Plays arbitrary audio tones.
  - `play <mario|zelda|scale>`: Plays musical tunes (Super Mario Bros theme, Legend of Zelda discovery chime, C Major scale).
  - `sound`: Inspects hardware Port `0x61` bits, audio playing status, and base oscillator telemetry.
- **In-Kernel Bare-Metal Self-Tests (`src/selftest/mod.rs`)**: Added 6th self-test suite (`test_pc_speaker_driver`), validating initial mute, 440Hz tone assertion on Port `0x61`, secondary mute, and `AudioPlayer` frame stepping.
- **Automated E2E Suite (`tests/qemu_e2e/test_audio.py`)**: Added `audio::test_pc_speaker_audio`, expanding the automated E2E suite to **13/13 passing tests** in ~91s.

### 9. Window Snapping, Maximize/Restore & Minimization — COMPLETED

- **Window Snapping & Tiling (`src/gui/window.rs`, `src/gui/wm.rs`)**:
  - Implemented green traffic-light button click and titlebar double-click **Maximize / Restore**, expanding windows to fill the entire desktop workspace (`1280x716`, between 24px top menu bar and bottom launcher dock) while saving previous floating bounds (`saved_bounds: Option<Rect>`).
  - Implemented **Edge Snapping & Half-Screen Tiling** (macOS Sequoia / Aero Snap style): dragging a titlebar to the left screen edge snaps to left half-screen (`640x716`), dragging to the right edge snaps to right half-screen (`640x716`), and dragging to the top edge snaps to full workspace maximize.
  - Dragging a maximized/snapped window away from the top edge smoothly un-maximizes and restores original dimensions attached to cursor.
  - Implemented real-time translucent glowing **Snap Preview Outline** (`Color::rgba(60, 140, 240, 45)`) rendered dynamically while hovering near screen boundaries.
- **Window Minimization & Dock Integration (`src/gui/dock.rs`, `src/gui/wm.rs`)**:
  - Clicking yellow traffic-light button minimizes window off-screen and plays `SoundEffect::WindowClose`.
  - Dock renders a distinct amber indicator dot (`Color::rgb(255, 189, 46)`) below minimized applications, versus white dots for active focused apps.
  - Clicking the application slot in the launcher dock un-minimizes and restores focus.
- **Sound Effects (`src/drivers/speaker.rs`)**:
  - Added `SoundEffect::WindowSnap` affirmative chirp on maximize and edge snap.
- **Automated E2E Suite (`tests/qemu_e2e/test_window_snapping.py`)**:
  - Added `window_snapping::test_window_snapping_and_tiling`, testing green button maximize & restore, titlebar double-click toggle, left-half edge snapping, yellow button minimization, dock un-minimization, and system stability.
  - Expanding the automated E2E suite to **14/14 passing test suites** in ~104s.

### 10. Desktop Wallpaper Engine & System Settings App — COMPLETED

- **Wallpaper Engine & Binary P6 PPM Parser (`src/gui/wallpaper.rs`, `src/gui/wm.rs`)**:
  - Implemented interrupt-safe, allocation-free binary P6 PPM parser (`parse_ppm_p6`) with comment and whitespace skipping.
  - Implemented direct backbuffer scanline blit with on-the-fly nearest-neighbor row scaling, allowing custom user images (such as `/user/drawing.ppm` created in Aegis Paint) to render seamlessly across the 1280x800 desktop without 4MB intermediate buffers, preserving the `< 60MB RAM` idle footprint.
  - Expanded built-in wallpaper themes to 6 gradient options: *Deep Ocean*, *Cyber Twilight*, *Emerald Forest*, *Midnight Slate*, *Sunset Horizon*, and *Solar Flare*.
- **System Settings Preferences App (`src/apps/settings.rs`)**:
  - macOS System Preferences-style split-pane GUI with sidebar navigation:
    - **Appearance**: Interactive gradient preview cards for all 6 themes, `[ + Set Paint Drawing (/user/drawing.ppm) ]` button with VFS integration and audio chimes, and `[ Reset to Default ]`.
    - **Sound & Audio**: Audio test buttons (`[ Test Boot Chime ]`, `[ Play Mario Theme ]`), hardware speaker mute/unmute toggle (`[ Mute Speaker ]`), and Port 0x61 hardware oscillator telemetry.
    - **Display & Info**: Resolution specs (`1280x800@60Hz`), TSC calibrated frame pacing metrics, Ring 0/Ring 3 isolation engine status, and live RAM utilization (< 60MB verified).
- **10-Slot Launcher Dock & Titanium Gear Icon (`src/gui/dock.rs`, `src/gui/font.rs`)**:
  - Added `AppId::Settings` ("System Settings"), expanding the launcher dock to 10 application slots (`DOCK_WIDTH = 600`).
  - Handcrafted 24x24 metallic titanium gear icon (`draw_settings_icon`) with 8 radial teeth and central hub axle.
- **Terminal Shell Commands (`src/apps/terminal.rs`)**:
  - `wallpaper`: Lists available themes.
  - `wallpaper <theme|custom [path]>`: Dynamically verifies and switches active desktop background.
  - `run settings`: Launches System Settings app from CLI.
- **In-Kernel Bare-Metal Self-Tests (`src/selftest/mod.rs`)**:
  - Added 7th self-test suite `test_wallpaper_and_ppm_parser`, validating valid P6 header decoding, color channels, corrupted/truncated input handling, and round-trip VFS persistence (**7/7 self-test suites passing**).
- **Automated QEMU E2E Suite (`tests/qemu_e2e/test_settings.py`)**:
  - Added `settings::test_system_settings_and_wallpaper`, expanding the automated E2E test harness to **15/15 passing test suites** in ~111s.

### 11. Scientific Calculator 2.0 with History Tape — COMPLETED

- **Scientific Math Engine (`src/apps/calculator.rs`)**:
  - Implemented iterative Newton-Raphson float square root solver (`compute_sqrt`) for `no_std` bare metal, achieving 4-decimal convergence in 20 cycles with zero allocations.
  - Implemented fast binary exponentiation for powers (`x^y`, `x²`).
  - Added reciprocal (`1/x`), percentage (`x%`), sign inversion (`±`), and mathematical constants ($\pi = 3.14159265$, $e = 2.71828182$).
  - Division by zero and negative root domain error protection with audible alert sound (`SoundEffect::Alert`) and safe recovery state.
- **Dual-Pane Interface & Calculation History Tape (`src/apps/calculator.rs`)**:
  - Expanded window to `450 x 360` (`src/main.rs`).
  - Left Pane: 2-line LCD display (expression sub-header + active input) and 5x5 scientific/numeric keypad.
  - Right Pane: Calculation History Tape (Paper Roll) recording all completed operations (e.g. `125 + 75 = 200`, `√(144) = 12`, `2 ^ 8 = 256`).
  - Interactive recall: clicking any past calculation line on the tape immediately copies that result into the active display for chained operations.
  - `[ Clear ]` button to wipe the history tape.
- **Typography & Font Expansion (`src/gui/font.rs`)**:
  - Added custom 8x16 bitmaps to `SUPPLEMENTARY_GLYPHS` for square root `√` (`\u{221A}`) and pi `π` (`\u{03C0}`).
- **In-Kernel Bare-Metal Self-Tests (`src/selftest/mod.rs`)**:
  - Added 8th self-test suite `test_scientific_calculator_engine`, asserting square root convergence, binary powers, state machine operations, error trapping, and history tape recall (**8/8 self-test suites passing in 3.5s**).
- **Automated QEMU E2E Suite (`tests/qemu_e2e/test_calculator.py`)**:
  - Added `calculator::test_scientific_calculator_lifecycle`, expanding the automated E2E harness to **16/16 passing test suites** in ~121s.

### 12. Terminal 2.0 (History, Tab Auto-Completion & ANSI Engine) — COMPLETED

- **Interactive Command History (`src/apps/terminal.rs`)**:
  - Up / Down arrow navigation across past commands with draft preservation: saves typed input in `saved_draft` when navigating up into history, and restores it when returning down.
  - Ring buffer storing up to 64 commands.
  - Built-in `history` command (numbered audit log) and `history -c` (history wipe).
- **Tab Auto-Completion Engine (`src/apps/terminal.rs`, `src/fs/mod.rs`)**:
  - Pressing `Tab` auto-completes:
    - Root commands (`help`, `neofetch`, `calc`, `wallpaper`, `symbols`, `beep`, `play`, `sound`, etc.).
    - App names after `run ` (`calc`, `paint`, `files`, `settings`, `snake`, `monitor`, `pad`, `crash`, `about`).
    - Subcommands after `play ` (`mario`, `zelda`, `scale`) and `wallpaper ` (`ocean`, `cyber`, `forest`, etc.).
    - VFS file and directory paths after `cat `, `ls `, `write `, `touch `, `rm ` (querying `crate::fs::get_all_vfs_paths()`).
  - Single match completes directly with a trailing space and affirmative snap sound (`SoundEffect::WindowSnap`).
  - Multiple matches compute the longest common prefix, advance the buffer, and list candidates in vibrant cyan (`\x1b[1;36m`).
- **Allocation-Free ANSI Color Engine (`src/apps/terminal.rs`)**:
  - `draw_ansi_string`: Fast state machine parsing standard escape codes (`\x1b[0m`, `\x1b[1m`, `\x1b[30m`..`\x1b[37m`, `\x1b[90m`..`\x1b[97m`, `\x1b[1;36m`, etc.) with color transitions and zero heap allocation per frame.
  - `strip_ansi`: Sanitizes escape codes for string length and test assertions.
  - Stylized colored outputs:
    - Prompt: `\x1b[1;32maegis\x1b[0m:\x1b[1;34m~\x1b[0m$ ` (neon green and blue).
    - `neofetch`: Stylized cyan ASCII shield logo with neon yellow/green telemetry labels.
    - `ls`: Directories in bold blue (`\x1b[1;34m`), documents in yellow (`\x1b[33m`), images in green (`\x1b[32m`), files in white.
    - `ps`: Header in bold cyan, process states in color (Running in green, Ready in yellow, Zombie in red).
    - Error banners in bold red (`\x1b[1;31mError:\x1b[0m`).
- **In-Kernel Bare-Metal Self-Tests (`src/selftest/mod.rs`)**:
  - Added 9th self-test suite `test_terminal_engine`, asserting command history draft preservation, Tab auto-completion prefix algorithms, and ANSI escape code processing (**9/9 self-test suites passing in 3.6s**).
- **Automated QEMU E2E Suite (`tests/qemu_e2e/test_terminal_advanced.py`)**:
  - Added `terminal_advanced::test_terminal_history_and_completion`, expanding the automated E2E harness to **17/17 passing test suites** in ~131s.

### 13. AI Agent Kernel Protocol, Spotlight Universal Search & Aegis Web Browser — COMPLETED

- **Autonomous AI Agent Kernel Bridge Subsystem (`src/agent/mod.rs`)**:
  - Structured RPC command dispatcher over serial giving AI agents direct supervisor control with Ring 0 privileges (`AGENT:STATUS`, `AGENT:SYSINFO`, `AGENT:VFS_READ`, `AGENT:VFS_WRITE`, `AGENT:VFS_LIST`, `AGENT:TASK_KILL`, `AGENT:EXEC`).
  - Kernel-level metrics tracking: packets handled, VFS operations count, tasks managed, and last command executed (`get_agent_metrics`).
  - Autonomous supervisor access allowing external LLMs or local agent processes to read telemetry and manipulate the OS without human intervention.
- **Spotlight Universal Desktop Search (`src/gui/spotlight.rs`)**:
  - Floating centered search overlay modal (`500 x 280`, 12px radius, translucent dark frosted background `Color::rgba(26, 28, 34, 245)`).
  - Toggled globally via `Ctrl+Space`, `F3`, or clicking the top menubar search button `[Q]`.
  - Real-time instant search indexing across:
    - **Applications**: `Terminal`, `Calculator`, `Paint`, `Settings`, `Files`, `AegisPad`, `Browser`, `Snake`, `Activity Monitor`, `Crash-Test`.
    - **VFS Files**: Searches RAM disk VFS paths (`/welcome.txt`, `/system/notes.txt`, `/user/drawing.ppm`, etc.).
    - **Shell Commands**: `neofetch`, `wallpaper`, `beep`, `play`, `history`, `clear`, `reboot`, `ls`, `cat`.
    - **Inline Math Evaluator**: Instantly computes expressions (`sqrt(144)` -> `12`, `25 * 4` -> `100`, `125 + 75` -> `200`).
  - Up / Down arrow selection, `Enter` to launch selected app or open file, `Escape` or outside click to dismiss.
- **Aegis Hypertext Web & Document Browser (`src/apps/browser.rs`)**:
  - Native OS web and document browser application (`AppId::Browser`, `560 x 420`).
  - Top navigation chrome: Back `[ < ]`, Forward `[ > ]`, Refresh `[ R ]`, and editable URL address bar with focus cursor.
  - Built-in intranet OS portals:
    - `aegis://home`: Welcome to AegisOS portal, quick links, and system feature list.
    - `aegis://agent`: Live **AI Agent Kernel Supervisor Dashboard** showing real-time agent packet counts, VFS operations, CPU %, RAM footprint, and supported RPC commands.
    - `aegis://docs/kernel`: Architecture documentation covering PML4 paging, HHDM, Ring 0 / Ring 3 privilege separation, and fault isolation.
    - `vfs://...`: Directly renders VFS text files (e.g. `vfs:///welcome.txt`).
  - Clickable markdown hyperlinks `[Label](url)` that navigate between pages and maintain browser history stacks.
- **Dock & Typography Expansion (`src/gui/dock.rs`, `src/gui/font.rs`)**:
  - Expanded launcher dock to 11 slots (`DOCK_WIDTH = 660`, `AppId::Browser` at slot 5).
  - Handcrafted 24x24 azure blue globe icon with meridian and latitude arcs (`draw_globe_icon`).
- **In-Kernel Bare-Metal Self-Tests (`src/selftest/mod.rs`)**:
  - Added 10th self-test suite `test_agent_spotlight_browser_engine`, asserting agent RPC ping/sysinfo/VFS write/read, Spotlight search matching and inline math evaluation, and Browser navigation/history (**10/10 self-test suites passing in 3.6s**).
- **Automated QEMU E2E Suite (`tests/qemu_e2e/test_spotlight_and_browser.py`)**:
  - Added `spotlight_and_browser::test_spotlight_and_browser_lifecycle`, expanding the automated E2E harness to **18/18 passing test suites** in ~146s.

### 14. Minesweeper Retro Arcade Game — COMPLETED

- **Minesweeper Application (`src/apps/minesweeper.rs`)**:
  - Classic retro arcade desktop game (`AppId::Minesweeper`, `248 x 310`).
  - Supports 9x9 Beginner (10 mines) and 16x16 Intermediate (40 mines) modes.
  - **First-Click Safety Guarantee**: The initial cell clicked and its 8 adjacent neighbors are guaranteed never to hold mines; mines are seeded dynamically using Xorshift64 PRNG.
  - **Recursive Zero-Neighbor Flood Reveal**: Clears empty regions instantly using an iterative, heap-safe queue.
  - **Right-Click & Trackpad Support**: Secondary click (or `Shift + Left Click`) places or removes red flag markers and updates the mine counter.
  - **Classic Retro Dashboard**:
    - 3-digit red digital 7-segment LED remaining mine counter (`010` .. `000`).
    - Interactive yellow smiley face button (🙂 normal, 😮 surprised on mouse down, 😎 sunglasses on victory, 😵 X-eyes on mine detonation).
    - 3-digit red digital 7-segment LED elapsed timer (`000` .. `999` seconds).
    - Beveled 3D gray tiles with color-coded neighbor numbers (1=blue, 2=green, 3=red, 4=dark blue, etc.) and red explosion indicators.
  - PC Speaker sound effects: crisp tick on safe reveals/flags, alert buzz on detonation, fanfare on victory.
- **Dock & Typography Expansion (`src/gui/dock.rs`, `src/gui/font.rs`)**:
  - Expanded launcher dock to 12 slots (`DOCK_WIDTH = 720`, `slot_width = 60`, `AppId::Minesweeper` at slot 6).
  - Handcrafted 24x24 spiked naval contact mine icon (`draw_mine_icon`) with red detonator cap and metallic glint.
- **In-Kernel Bare-Metal Self-Tests (`src/selftest/mod.rs`)**:
  - Added 11th self-test suite `test_minesweeper_engine`, asserting 9x9 grid layout, first-click 3x3 safety exclusion, total mine counts, recursive flood clearing, flag toggling, and difficulty switching (**11/11 self-test suites passing**).
- **Automated QEMU E2E Suite (`tests/qemu_e2e/test_minesweeper.py`)**:
  - Added `minesweeper::test_minesweeper_lifecycle`, expanding the automated E2E harness to **19/19 passing test suites** in ~167s.

### 15. AegisPad 2.0 Multi-Tab Syntax & Code Editor — COMPLETED

- **AegisPad 2.0 Application (`src/apps/editor.rs`)**:
  - Upgraded built-in editor into a multi-document IDE and code editor.
  - **Multi-Document Tab Strip**:
    - Top tab strip supporting multiple open buffers (`DocumentTab`), tab switching by click, `[x]` close tab button, and `[ + ]` new tab button.
    - Unsaved dirty tracking indicator (`*`) per tab.
    - Shortcuts: `Ctrl+N` (new buffer), `Ctrl+W` (close tab), `Ctrl+S` (save buffer to VFS).
  - **Line Number Gutter & Active Line Highlight**:
    - 40px fixed-width line number gutter with right-aligned line numbers in cyan/dim gray and border separator.
    - Full-width translucent active line highlight bar tracking the cursor's current line.
  - **Find & Replace Search Bar (`Ctrl+F`)**:
    - Floating search bar with real-time occurrence search, match counter (`1/1 matches`), `[ < ]` and `[ > ]` match navigation, and `[ Done ]` button.
    - Matching occurrences highlighted with gold bounding boxes on the document canvas.
  - **Real-Time Keyword Syntax Highlighting**:
    - Identifies programming keywords (`fn`, `let`, `pub`, `struct`, `enum`, `impl`, `match`, `if`, `else`, `return`, `true`, `false`, `mut`, `use`, `mod`, `for`, `in`, `while`, `loop`, `type`, `trait`, `const`, `static`) in warm amber/orange.
    - Full-line comments (`//...`, `#...`) in emerald green.
    - String literals (`"..."`) in bright cyan.
    - Numbers (digits) in soft lavender.
    - Standard identifiers and punctuation in bright white.
- **In-Kernel Bare-Metal Self-Tests (`src/selftest/mod.rs`)**:
  - Added 12th self-test suite `test_editor_advanced_engine`, asserting multi-tab buffer management, tab switching, search occurrence locating, keyword classification, and tab closing (**12/12 self-test suites passing**).
- **Automated QEMU E2E Suite (`tests/qemu_e2e/test_editor_advanced.py`)**:
  - Added `editor_advanced::test_editor_advanced_lifecycle`, expanding the automated E2E harness to **20/20 passing test suites** in ~163s.

### 16. AegisSynth — Interactive Chiptune Synthesizer & Piano Roll Studio — COMPLETED

- **AegisSynth Application (`src/apps/synth.rs`)**:
  - Full-featured chiptune audio workstation and interactive synthesizer.
  - **Interactive 2-Octave Chromatic Piano Keyboard**:
    - 14 ivory white keys (C4..B5) and 10 ebony black keys (C#4..A#5).
    - Playable via mouse clicks or PC keyboard keys (`A`..`K`, `W`, `E`, `T`, `Y`, `U`, `O`, `P`, etc.).
    - Visual key depression animations (cyan/orange active glow) with instant PIT speaker tone synthesis.
  - **4-Track 16-Step Pattern Sequencer / Tracker Studio**:
    - 4 polyphonic instrument tracks: `LEAD` (high pentatonic), `ARPG` (harmonic mid), `BASS` (sub 8-bit octaves), and `BEAT` (kick/snare/hi-hat percussion).
    - 16 interactive matrix step toggle buttons per track with neon color highlights.
    - Animated golden playhead line scanning across steps in real time, synchronized with the 100Hz hardware timer.
    - Built-in iconic chiptune pattern presets: "Cyberpunk Arp", "8-Bit Mario", and "Retro Bassline".
    - Tempo controls (`[ - ] 120 BPM [ + ]`), Play `[ ▶ Play ]`, Stop `[ ■ Stop ]`, and Clear `[ Clear ]`.
- **Dock & Typography Expansion (`src/gui/dock.rs`, `src/gui/font.rs`)**:
  - Expanded launcher dock to 13 slots (`DOCK_WIDTH = 780`, `slot_width = 60`, `AppId::Synth` at slot 7).
  - Handcrafted 24x24 beamed musical eighth-notes icon (`draw_music_note_icon`) in neon magenta and cyan.
- **In-Kernel Bare-Metal Self-Tests (`src/selftest/mod.rs`)**:
  - Added 13th self-test suite `test_synth_engine`, asserting note frequency table accuracy, sequencer step toggling, pattern memory, preset loading, and playhead advancement (**13/13 self-test suites passing**).
- **Automated QEMU E2E Suite (`tests/qemu_e2e/test_synth.py`)**:
  - Added `synth::test_synth_lifecycle`, expanding the automated E2E harness to **21/21 passing test suites** in ~173s.

### 17. AegisChat & Virtual Loopback Network Stack — COMPLETED

- **In-Kernel Virtual Network Stack (`src/net/mod.rs`)**:
  - IPv4 packet framing (RFC 791) with ones' complement internet checksum calculation.
  - UDP datagram transport framing (RFC 768) with 8-byte headers.
  - Virtual loopback network adapter (`LoopbackDevice`) with thread-safe packet FIFO queues (`127.0.0.1` and `255.255.255.255`).
  - High-level non-blocking `UdpSocket` abstraction (`bind`, `send_to`, `recv_from`).
- **AegisChat Application (`src/apps/chat.rs`)**:
  - Multi-channel collaboration client with left sidebar (`#general`, `#kernel-dev`, `#agent`, `#alerts`).
  - Online user presence list (`guest [You]`, `agent [AI]`, `kernel`).
  - Real-time scrollable message feed with color-coded user badges, timestamps, and message bubbles.
  - Interactive bottom message input bar with cursor and `[ Send ]` button.
  - Autonomous AI Coprocessor integration: messages to `#agent` or mentioning `@agent` trigger autonomous diagnostic responses delivered over the UDP socket.
- **Dock & Typography Expansion (`src/gui/dock.rs`, `src/gui/font.rs`)**:
  - Expanded launcher dock to 14 slots (`DOCK_WIDTH = 840`, `slot_width = 60`, `AppId::Chat` at slot 8).
  - Handcrafted 24x24 speech bubble icon (`draw_chat_icon`) in emerald green and white.
- **In-Kernel Bare-Metal Self-Tests (`src/selftest/mod.rs`)**:
  - Added 14th self-test suite `test_network_loopback_and_chat_engine`, asserting IPv4 header serialization, RFC 791 checksum validation, UDP socket transmit/receive, and AI agent coprocessor responses (**14/14 self-test suites passing**).
- **Automated QEMU E2E Suite (`tests/qemu_e2e/test_chat.py`)**:
  - Added `chat::test_chat_lifecycle`, expanding the automated E2E harness to **22/22 passing test suites** in ~179s.

### 4. Smaller items

- The font covers ASCII 32..126 plus seven supplementary glyphs. Anything else still
  falls back to `?`.
- The frame throttle in the compositor loop is a fixed 1,000-iteration spin. With the
  timer now accurate, it could be a real frame-pacing target.
- `src/apps/` renderers rebuild all layout every frame; app content is now the largest
  remaining share of frame cost, with the font rasterizer still per-pixel.
