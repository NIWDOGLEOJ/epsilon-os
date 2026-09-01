# Handoff Report: Fault Isolation, Multitasking Scheduler, and Memory Architecture

**Agent:** `survey_explorer_2` (Fault Isolation & Scheduler Explorer)  
**Date:** 2026-08-30  
**Target Recipient:** `parent` (Orchestrator: `c28358f3-14dd-4701-b6af-d43416c28150`)  
**Scope:** Architectural requirements, assembly ISR bindings, 2-phase deferred reaping, preemptive round-robin scheduler, and bitmap/paging memory subsystems.

---

## 1. Observation

1. **System Requirements (`ORIGINAL_REQUEST.md`)**:
   - **R1 (Kernel Architecture)**: Bare-metal `x86_64` Rust `no_std`, Limine bootloader (BIOS/UEFI), GDT, TSS, IDT, and 4-level PML4 paging (`ORIGINAL_REQUEST.md:13`).
   - **R2 (Fault Isolation & Crash Resilience)**: Exception handlers for Page Fault (`#PF`, 14), Divide-by-Zero (`#DE`, 0), General Protection Fault (`#GP`, 13), and Invalid Opcode (`#UD`, 6) must detect Ring 3 origin, log faulting address, terminate task, reclaim frames, and reschedule without panicking or freezing the GUI (`ORIGINAL_REQUEST.md:16-19`).
   - **R3 (Memory Management & Scheduling)**: Physical frame allocator via Limine memory map, kernel heap allocator, per-process virtual address spaces, and preemptive round-robin scheduler driven by timer interrupts (`ORIGINAL_REQUEST.md:21-23`).
   - **System Performance**: Idle memory consumption must remain `< 60 MB RAM` with support for up to 4 GB RAM (`ORIGINAL_REQUEST.md:57-58`).

2. **Hardware Constraints & x86_64 Architecture Specs**:
   - In x86_64 Long Mode, the CPU pushes `SS`, `RSP`, `RFLAGS`, `CS`, `RIP`, and optionally `ErrorCode` onto the kernel stack specified in `TSS.RSP0` when an interrupt/exception triggers a privilege transition from Ring 3 to Ring 0.
   - The Code Segment selector `CS` contains the privilege level in its two least significant bits (`CS & 0x03`). Ring 3 user mode is unequivocally identified when `(CS & 0x03) == 3`.
   - On `#PF`, the faulting virtual address is stored in `CR2`. Error code bit 0 indicates whether the page was not present (`0`) or protection violated (`1`); bit 1 indicates write (`1`) or read (`0`); bit 2 indicates user (`1`) or kernel (`0`).

3. **Host Tooling**:
   - Host environment has `qemu-system-x86_64`, `xorriso`, `curl`, `gcc`, `make`, and `clang` available in `/usr/bin/`.

---

## 2. Logic Chain

1. **Premise**: User process crashes must not panic the kernel or corrupt active kernel state.
   - **Inference 1**: By inspecting `(frame.cs & 0x03)` in the exception dispatcher, the kernel can deterministically branch: `0` (Kernel Mode) triggers a panic/halt, while `3` (User Mode) bypasses kernel panic entirely.
   - **Inference 2**: Because vector 0 (`#DE`) and vector 6 (`#UD`) do not push hardware error codes while vector 13 (`#GP`) and vector 14 (`#PF`) do, assembly ISR wrappers must push a dummy `0` error code for non-error-code vectors to guarantee uniform stack alignment for the Rust dispatcher.

2. **Premise**: Immediate self-destruction of an executing task's stack and active page table causes CPU triple-faults.
   - **Inference 3**: Deallocation must be decoupled into two phases:
     - *Phase 1 (Synchronous in ISR)*: Mark task `Terminated`, queue into `zombie_queue`, notify GUI window manager, and immediately switch context to the next ready task or idle task.
     - *Phase 2 (Asynchronous Deferred Reaping)*: While running on an independent stack and CR3, the Idle Task / Reaper walks the terminated process's lower-half PML4 (`0..255`), frees all leaf physical frames and page table structures back to the frame allocator, frees the task's kernel stack, and deallocates the PCB.

3. **Premise**: Idle memory consumption must stay under 60 MB for 4 GB RAM.
   - **Inference 4**: A physical bitmap allocator requires only 1 bit per 4 KB frame. For 4 GB RAM, the entire allocation map requires only 128 KB of RAM ($1,048,576 / 8 = 131,072$ bytes), leaving > 59.8 MB of headroom.
   - **Inference 5**: Higher-half kernel PML4 entries (`256..511`) can be cloned by value into every new user PML4, creating full address space isolation in the lower half (`0..255`) with zero duplicated kernel data structures.

4. **Premise**: Preemption requires periodic CPU control transfer and full register preservation.
   - **Inference 6**: A PIT timer at 100 Hz (10 ms quantum) sending IRQ 0 / Vector 32 interrupts allows the scheduler to save GPRs (`rax`..`r15`), update `TSS.RSP0`, swap `CR3`, and switch `RSP` to the next ready task in the round-robin queue.

---

## 3. Caveats

- **SMP / Multi-Core**: The initial design focuses on uniprocessor preemption (1 CPU core) as required for the base system; multi-core scheduling with per-CPU runqueues and IPIs (Inter-Processor Interrupts) can be layered on top of this PCB / TSS foundation later if needed.
- **FPU / SSE / AVX State**: If user applications utilize SSE/AVX vector instructions, `FXSAVE64` / `FXRSTOR64` (or `XSAVE` / `XRSTOR`) should be added to the context switch routine with a 512-byte 16-byte aligned buffer in the PCB.

---

## 4. Conclusion

The architectural design for Fault Isolation (R2), Clean Task Termination, Preemptive Multitasking Scheduler (R3), and Memory Allocators (R3) is fully specified with concrete Rust data structures, assembly ISR stubs, paging destruction routines, and context-switching mechanisms in `/home/godjoel/teamwork_projects/aegis_os/.agents/survey_explorer_2/analysis.md`.

Key components ready for implementation:
1. `InterruptStackFrame` and `rust_exception_dispatcher` with `(cs & 3) == 3` Ring 3 fault detection.
2. Assembly ISR stubs for `#DE`, `#UD`, `#GP`, and `#PF` with standardized stack alignment.
3. 2-Phase Zombie Reaping and recursive lower-half (`0..256`) Page Table deallocator.
4. Preemptive round-robin `Scheduler` driven by 100 Hz timer IRQ with `TSS.RSP0` synchronization.
5. 128 KB `BitmapFrameAllocator` handling 4 GB physical memory under 60 MB idle RAM footprint.
6. Per-process virtual address space cloning with guarded user stack layout.

---

## 5. Verification Method

To independently verify the architecture and its implementation:
1. **Source Inspection**:
   - Verify `/home/godjoel/teamwork_projects/aegis_os/.agents/survey_explorer_2/analysis.md` for complete code blueprints and diagrams.
2. **Crash Resilience Verification in QEMU**:
   - Launch AegisOS in QEMU with serial output redirected to stdio.
   - Trigger Null Pointer Dereference in Crash-Test App -> Observe serial log `[AegisOS Crash Isolator] User Process PID X crashed: Page Fault at RIP ..., CR2=0x0000000000000000`. Confirm GUI window closes and Activity Monitor continues live updates without kernel panic.
   - Trigger Divide-by-Zero in Crash-Test App -> Observe serial log `[AegisOS Crash Isolator] User Process PID X crashed: Divide-by-Zero`. Confirm process is reaped and memory reclaimed.
   - Trigger Invalid Opcode (`ud2`) -> Observe serial log `[AegisOS Crash Isolator] User Process PID X crashed: Invalid Opcode`. Confirm clean return to scheduler.
3. **Memory Footprint Verification**:
   - Inspect Activity Monitor heap/frame statistics: verify total memory allocated is under 60 MB at idle desktop.
