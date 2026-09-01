# Handoff Report — GUI & System Suite Specification

**Agent**: survey_explorer_3  
**Date**: 2026-08-30  
**Type**: Hard Handoff  
**Target Report**: `/home/godjoel/teamwork_projects/aegis_os/.agents/survey_explorer_3/gui_suite_report.md`

---

## 1. Observation
- **Original User Request**: `/home/godjoel/teamwork_projects/aegis_os/.agents/ORIGINAL_REQUEST.md` lines 24–41 mandates:
  - R4: Double-buffered linear RGB framebuffer driver supporting a macOS-inspired desktop GUI with 24px top menu bar (logo, active app name, uptime clock, CPU/RAM badge), floating window manager with draggable title bars, active window focus, traffic-light close buttons, launcher dock with clickable icons, PS/2 mouse packet tracking and keyboard input handling.
  - R5: 5 Core Applications:
    1. Crash-Test Demo App (Null Pointer Dereference, Divide-by-Zero, Out-of-Bounds Memory Write, Invalid Opcode) proving process isolation.
    2. Activity Monitor (live CPU %, real-time memory usage graph verifying < 60MB RAM footprint, process table with PID, status, kill).
    3. Interactive Terminal Shell (`ps`, `kill`, `free`, `echo`, `run`, `clear`, `reboot`).
    4. Text Editor (AegisPad).
    5. About AegisOS Dialog.
  - R6: Automated build pipeline, hybrid bootable ISO (`aegis_os.iso`), and `run_qemu.sh` launch script.
- **Host Tools Probed**:
  - `/usr/bin/qemu-system-x86_64` (version 11.1.1, supports `gtk`, `sdl`, `curses`, `none`, `egl-headless`).
  - `/usr/bin/xorriso` (version 1.5.8.pl02).
  - UEFI firmware available at `/usr/share/edk2/x64/OVMF.4m.fd`.
  - Host OS: Garuda Linux (Arch-based rolling release).

---

## 2. Logic Chain
1. **Linear Framebuffer & Double-Buffering**:
   - Limine provides a linear 32-bit ARGB/XRGB framebuffer at boot.
   - For $1024 \times 768 \times 32\text{ bpp}$, the backbuffer requires exactly $3,145,728\text{ bytes} \approx 3.0\text{ MB}$ of system RAM.
   - Performing all 2D primitive rasterization in RAM and blitting via 64-bit contiguous copy per scanline guarantees tear-free 60 FPS rendering.
2. **Desktop Compositor & Window Manager**:
   - A 24px top menu bar (`y = 0..24`) contains the Aegis logo, active application title, real-time CPU %, RAM footprint badge, and uptime clock.
   - Windows maintain a Z-ordered stack. Mouse clicks hit-test from top to bottom, bringing the active window to index $N-1$ and setting `is_focused = true`.
   - Titlebars support dragging clamped to screen bounds `(0..W-40, 24..H-30)`.
   - Red traffic-light button (`x+16, y+12`, radius 6px) triggers task reap and window closure.
   - Launcher dock (`320x48px`) at bottom center provides instant launch/focus for all 5 core apps with active-indicator dots.
3. **Core Applications & Demo Suite**:
   - **Crash-Test Demo App**: 4 buttons trigger hardware exceptions (`#PF`, `#DE`, `#GP`, `#UD`) in Ring 3. The kernel exception handler logs the fault, terminates only the crashed task, and reclaims memory while desktop and Activity Monitor stay responsive.
   - **Activity Monitor**: Displays rolling CPU history and live RAM usage, explicitly proving the `< 60MB RAM` idle footprint constraint, along with an interactive process table and `[Kill Process]` button.
   - **Interactive Terminal Shell**: 65x18 character console with built-in commands (`ps`, `kill`, `free`, `echo`, `run`, `clear`, `reboot`), scrolling buffer, and command history.
   - **Text Editor (AegisPad)**: Multiline text buffer with line numbering gutter, arrow key navigation, character insertion/deletion, and status bar.
   - **About AegisOS Dialog**: Modal dialog displaying the Aegis Shield logo, OS version, kernel architecture, memory footprint, and display specs.
4. **Build & ISO Packaging Pipeline**:
   - Target `x86_64-unknown-none` with `-C code-model=kernel` and `-C relocation-model=static`.
   - Higher-half linker script `linker.ld` placing kernel at `0xFFFFFFFF80100000`.
   - `xorriso` + `limine bios-install` packaging creates a hybrid ISO bootable on both BIOS and UEFI.
   - `run_qemu.sh` compiles, packages, and launches QEMU with `-serial stdio` and standard VGA display.

---

## 3. Caveats
- Host Rust toolchain (`rustc`, `cargo`) is managed in user home / rustup; downstream workers should ensure `cargo` is in `$PATH` or use `rustup`.
- Framebuffer resolution is defaulted to `1024x768x32`, which is universally supported by Limine, QEMU `std-vga`, and physical hardware.

---

## 4. Conclusion
The GUI engine, desktop environment (R4), 5 core applications (R5), and build pipeline (R6) specifications are fully documented with exact data structures, mathematical bounds, UI wireframes, color schemes, CLI commands, and build scripts in `gui_suite_report.md`. The design is directly actionable and ready for implementation.

---

## 5. Verification Method
1. Inspect the full specification report:
   `view_file /home/godjoel/teamwork_projects/aegis_os/.agents/survey_explorer_3/gui_suite_report.md`
2. Verify all requirements from `ORIGINAL_REQUEST.md` (R4, R5, R6) are addressed.
3. Check that the 5 applications, window manager data structures, mouse/keyboard decoders, and build scripts are specified.
