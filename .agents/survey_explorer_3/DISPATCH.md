## 2026-08-30T17:35:47Z
You are the GUI & System Suite Spec Miner for AegisOS.
Your working directory is /home/godjoel/teamwork_projects/aegis_os/.agents/survey_explorer_3.
Project root is /home/godjoel/teamwork_projects/aegis_os.
You MUST read /home/godjoel/teamwork_projects/aegis_os/.agents/ORIGINAL_REQUEST.md.

Your mission:
Investigate and specify the requirements for:
1. Double-buffered linear RGB framebuffer rendering: Limine framebuffer format (ARGB/RGB 32-bit), backbuffer swapping, dirty rectangle or full blit at 60 FPS, font rendering (embedded 8x16 bitmap font or similar).
2. Desktop environment (R4): macOS-inspired layout: 24px top menu bar (logo, active app name, uptime clock, CPU & RAM badge), floating window manager (window structures, draggable title bars, z-ordering / focus, traffic-light close button), launcher dock at bottom with clickable icons, PS/2 mouse packet decoding & cursor rendering, PS/2 keyboard scancode decoding & focus event routing.
3. 5 Core Applications & Demo Suite (R5):
   - Crash-Test Demo App (buttons for Null Pointer Dereference, Divide-by-Zero, Out-of-Bounds Write, Invalid Opcode)
   - Activity Monitor (real-time CPU %, live RAM graph verifying < 60MB RAM footprint, process table with PID, status, kill)
   - Interactive Terminal Shell (CLI commands: `ps`, `kill`, `free`, `echo`, `run`, `clear`, `reboot`)
   - Text Editor (AegisPad)
   - About AegisOS Dialog
4. Build & ISO packaging pipeline (R6): Cargo build flags, linker script, `limine.cfg`, `xorriso`/`limine` bios/uefi bootable ISO generation, and `run_qemu.sh` script with display and serial options.
5. Write a comprehensive report in /home/godjoel/teamwork_projects/aegis_os/.agents/survey_explorer_3/gui_suite_report.md and complete handoff.md. Send a message to parent when done.
