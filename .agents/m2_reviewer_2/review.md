# Milestone 2 Review Report: Zombie Reclamation & Fault Isolation

**Reviewer**: Reviewer 2 (Roles: Reviewer, Adversarial Critic)  
**Target Milestone**: AegisOS Milestone 2 (Preemptive Multitasking Scheduler, Ring 3 Fault Isolation, 2-Phase Deferred Zombie Reaping)  
**Date**: 2026-08-30  
**Verdict**: **APPROVE**

---

## 1. Executive Summary

Milestone 2 implements preemption, fault isolation, and resource reclamation for AegisOS. All requirements specified in `PROJECT.md` and `ORIGINAL_REQUEST.md` (Features F6 and F7) have been rigorously examined, verified via compiler checks and 4-tier E2E simulation suites, and stress-tested against adversarial failure modes.

### Key Verification Metrics
- `cargo check --target x86_64-unknown-none`: **PASS** (zero warnings / errors)
- `cargo build --release --target x86_64-unknown-none`: **PASS** (optimized kernel ELF produced)
- `cargo test --target x86_64-unknown-linux-gnu --manifest-path tests/e2e/Cargo.toml`: **PASS (135/135 tests passed, 0 failures)**
  - Tier 1 (Feature Coverage): 61/61 passed
  - Tier 2 (Boundary & Corner Cases): 61/61 passed
  - Tier 3 (Cross-Feature Combinations): 8/8 passed
  - Tier 4 (Real-World Scenarios): 5/5 passed

---

## 2. Quality & Architecture Review

### 2.1 Two-Phase Deferred Zombie Frame Reclamation
- **Phase 1 (Fault Trap / Explicit Kill)**:
  - When an exception (#PF, #DE, #GP, #UD) occurs in Ring 3 (`(ctx.cs & 3) == 3`), the IDT handler routes to `src/task/fault.rs::handle_user_fault`.
  - The offending task is immediately marked as `TaskState::Terminated(exit_reason)` and its PID is queued into `Scheduler::zombie_queue`.
  - The scheduler avoids doing immediate synchronous page table deallocations within the interrupt context of the faulting task, preventing stack exhaustion and dangling memory traps.
- **Phase 2 (Deferred Reclamation & Freeing)**:
  - Invoked during scheduler rotation (`Scheduler::schedule`) and within `Scheduler::idle_task_entry`.
  - `reap_zombies` drains `zombie_queue`, traverses `allocated_frames`, and calls `destroy_user_address_space(user_pml4)` for private user page tables.
  - All lower-half leaf pages, PT frames, PD frames, PDPT frames, and the PML4 root are reclaimed back to the physical bitmap allocator (`FrameAllocSimulator` / `frame.rs`).
  - Memory leak checks in Scenario 5 confirmed 0-byte leakage over 1,000 continuous rendering and context-switch iterations.

### 2.2 Safety of `destroy_user_address_space` on Independent Kernel Stack
- **Stack Independence**:
  - Each process is allocated a dedicated 32 KiB kernel stack (`KERNEL_STACK_SIZE = 32768`) in higher-half memory.
  - On every context switch (`restore_context_to_interrupt`), `TSS.RSP0` is updated to `pcb.kernel_stack_top`.
  - When transitioning from Ring 3 to Ring 0 upon an exception, the CPU hardware automatically loads `RSP0`.
  - Deallocation routines in `src/memory/paging.rs` execute on the kernel stack and access physical memory exclusively via the Limine Higher-Half Direct Map (HHDM), completely decoupled from the user stack.
- **Page Table Bounds**:
  - `destroy_user_address_space` strictly iterates over PML4 entries `0..256` (the user lower-half).
  - Higher-half entries `256..512` (containing kernel code, HHDM, GDT/TSS, IDT, and heap) are strictly untouched and shared across all PML4s.

### 2.3 Process Control Block (PCB) State Transitions
- **State Machine Integrity**:
  - `TaskState` transitions follow the strict lifecycle:
    `Ready` $\rightarrow$ `Running` $\rightarrow$ `Ready` (on quantum preemption)
    `Running` $\rightarrow$ `Terminated` (on Ring 3 fault or `kill_process`)
    `Terminated` $\rightarrow$ [Reaped / Removed] (on Phase 2 zombie collection)
  - `PID 0 [idle]` is strictly immune to termination (`kill_process(0)` returns `false`).
  - Fallback logic in `schedule` guarantees that if all application tasks are terminated or blocked, the CPU falls back to `PID 0` executing `sti; hlt` low-power wait.

---

## 3. Adversarial / Critic Analysis & Failure Mode Stress Testing

### Challenge 1: Active CR3 Deallocation During Exception Dispatch
- **Assumption**: The kernel can safely reclaim user page tables while executing within the exception handler.
- **Stress Analysis**: If `destroy_user_address_space` frees the root PML4 frame while `CR3` is still active on that same PML4, hardware TLB prefetching or paging walks could access freed physical frames.
- **AegisOS Defense**:
  1. The kernel executes in the higher-half direct map (PML4 entries 256..511), which is identical across all address spaces.
  2. In `schedule()`, the scheduler immediately restores the new task's context and writes the new task's PML4 root to `CR3` (`write_cr3(pcb.pml4_root)`).
  3. In `idle_task_entry()`, `CR3` is set to the master `kernel_pml4`, ensuring background reclamation occurs with zero dependency on any user PML4.
- **Verdict**: **PASS (Mitigated & Safe)**.

### Challenge 2: Rapid-Burst Fault Cascades & Concurrency Stress
- **Assumption**: Rapid successive process faults will not exhaust scheduler capacity or cause runqueue corruption.
- **Stress Analysis**: Simulated 10 concurrent worker tasks triggering simultaneous faults (#DE, #PF, #UD, #GP) in `test_tier4_scenario_02_stress_and_concurrent_fault_recovery_workflow` and `test_f6_b01_rapid_100x_crash_burst_stress`.
- **Result**: All crashed tasks were cleanly reaped, non-faulting UI apps (Terminal, Activity Monitor) and kernel background threads continued without interruption, and desktop frame blitting remained active at 60 FPS.
- **Verdict**: **PASS**.

### Challenge 3: Hardware Privilege Level Validation
- **Assumption**: Exceptions originating in Ring 0 are never mistakenly isolated or reaped as user tasks.
- **Stress Analysis**: `src/arch/idt.rs::handle_exception` inspects `(ctx.cs & 0x03) == 3`. When `(ctx.cs & 3) == 0` (Ring 0), the kernel executes a full register diagnostic dump to COM1 serial and panics intentionally rather than attempting recovery on a corrupted kernel state (`test_f6_b04_kernel_mode_fault_triggers_panic_not_isolate`).
- **Verdict**: **PASS**.

---

## 4. Integrity Assessment

In accordance with system integrity standards, the implementation was inspected for cheating patterns:
1. **Hardcoded test results / Facade logic**: None detected. Frame allocation, bitmap math, page table translations, quantum accounting, and context switching use real logic and real state tracking.
2. **Shortcuts & external delegation**: None. The scheduler, PCB management, ISR stubs, and fault recovery are implemented in native `no_std` Rust.
3. **Verification outputs**: All 135 tests in `tests/e2e` were independently executed and passed cleanly.

---

## 5. Review Conclusion & Verdict

The Milestone 2 implementation for AegisOS is robust, correct, thread-safe, and fully satisfies all isolation and multitasking specifications.

**Verdict: APPROVE**
