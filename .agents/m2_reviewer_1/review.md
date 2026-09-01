# AegisOS Milestone 2 Code Review & Adversarial Analysis Report

**Reviewer**: Reviewer 1 (Roles: Reviewer, Critic)  
**Date**: 2026-08-30  
**Target Milestone**: Milestone 2 — Preemptive Multitasking, Ring 3 Fault Isolation & Crash Resilience  
**Verdict**: **APPROVE**  

---

## 1. Executive Summary

Milestone 2 establishes the core multitasking and crash-resilience engine for AegisOS:
1. **Preemptive Round-Robin Scheduler (`src/task/scheduler.rs`)**: Implements 100Hz hardware timer IRQ 0 (Vector 32) preemption, priority-tier round-robin runqueue scheduling, PID 0 `[idle]` task protection, and dynamic CPU/RAM telemetry calculation.
2. **Context Synchronization (`src/task/context.rs`)**: Synchronizes full CPU register frames (`TaskContext` <-> `InterruptContext`), updates `TSS.RSP0` on task switches to ensure isolated kernel stacks for userspace traps, and reloads `CR3` for virtual address space isolation.
3. **Ring 3 Fault Isolation (`src/task/fault.rs` & `src/arch/idt.rs`)**: Enforces hardware privilege discrimination via `(CS & 3) == 3`. When userspace tasks trigger `#PF` (vector 14), `#DE` (vector 0), `#GP` (vector 13), or `#UD` (vector 6), the kernel logs diagnostic post-mortems over COM1 UART, terminates the offending process, reclaims physical frames via 2-phase deferred zombie reaping, and context-switches to the next ready task without panicking the kernel or freezing the desktop.
4. **Subsystem Integration (`src/task/mod.rs` & `src/main.rs`)**: Registers timer and fault callbacks into the IDT dispatcher, boots the task subsystem during kernel startup in `_start()`, spawns background worker tasks, enables CPU interrupts (`sti`), and enters the low-power `idle_task_entry()` loop (`sti; hlt`).

All 135 opaque-box E2E tests across 4 tiers pass cleanly, and the kernel compiles with zero errors or warnings for target `x86_64-unknown-none`.

---

## 2. Detailed Component Review

### 2.1 Process Control Block & Task State (`src/task/pcb.rs`)
- **State Machine**: Correctly models process execution lifecycle with `Ready`, `Running`, `Blocked(BlockReason)`, `Zombie`, and `Terminated(ExitReason)`.
- **Termination Taxonomy**: Rich `ExitReason` enum distinguishes `Normal(i32)`, `PageFault { cr2, error_code }`, `DivideByZero`, `GeneralProtection { error_code }`, `InvalidOpcode`, and `KilledByAdmin`.
- **Context Structure**: `TaskContext` stores all 15 x86_64 general purpose registers (`r15`..`rax`) and CPU control registers (`rip`, `cs`, `rflags`, `rsp`, `ss`). Context constructors properly set `RFLAGS = 0x202` (IF=1) and differentiate kernel segment selectors (`0x08`/`0x10`) from user segment selectors with RPL=3 (`0x23`/`0x1B`).
- **Memory Footprint Tracking**: Tracks private PML4 root, kernel/user stack pointers, and `allocated_frames: Vec<PhysAddr>` for deterministic frame reclamation.

### 2.2 Context Switching Engine (`src/task/context.rs`)
- **Interrupt Context Mapping**: `save_context_from_interrupt` and `restore_context_to_interrupt` provide complete bidirectional register synchronization between the hardware interrupt stack frame (`InterruptContext`) and PCB storage (`TaskContext`).
- **TSS RSP0 Synchronization**: Calls `arch::gdt::set_tss_rsp0(pcb.kernel_stack_top.as_u64())` during every task restoration, guaranteeing that future Ring 3 -> Ring 0 privilege transitions switch to the active task's private 32KB kernel stack.
- **PML4 CR3 Swapping**: Invokes `memory::paging::write_cr3(pcb.pml4_root)` on task restoration, ensuring instant page directory activation and flushing non-global TLB entries.

### 2.3 Preemptive Scheduler & Telemetry (`src/task/scheduler.rs`)
- **PID 0 Idle Task**: `Scheduler::init()` initializes PID 0 `[idle]` with a dedicated 32KB kernel stack (`Box::leak`) and kernel PML4. PID 0 is immune to termination (`kill_process(0) -> false`).
- **Task Spawning**: `spawn_process` handles both kernel tasks (sharing kernel PML4) and user tasks (creating private PML4 via `create_user_address_space()`, mapping user stack at `0x0000_7FFF_FFFF_0000 - PAGE_SIZE`, and mapping user code page).
- **Preemptive Quantum**: 100Hz timer ticks decrement `time_slice_remaining`; once exhausted, the running task is marked `Ready` and rotated via round-robin.
- **Phase 2 Deferred Zombie Reaping**: `reap_zombies()` drains `zombie_queue` and calls `destroy_user_address_space()` for isolated user address spaces, freeing leaf frames, intermediate tables (PT, PD, PDPT), and the root PML4 without leaking physical memory.
- **System Telemetry**: Computes aggregate CPU utilization (`((total_ticks - idle_ticks) * 100) / total_ticks`) and per-process telemetry snapshots for the Activity Monitor and CLI shell.

### 2.4 Ring 3 Fault Isolation Engine (`src/task/fault.rs`)
- **Privilege Separation**: Invoked by IDT dispatcher when `(ctx.cs & 3) == 3`.
- **Fault Diagnostic Logging**: Outputs structured serial UART log: `[FAULT-ISOLATION] Process PID {} ('{}') crashed due to {} at RIP 0x{:016x} CR2=0x{:016x}`.
- **Crash Callback & Rescheduling**: Drops scheduler lock before invoking `CRASH_CALLBACK` (avoiding potential lock reentrancy deadlocks), reacquires lock, and invokes `schedule(ctx)` to replace the faulting task's stack context with the next ready task.
- **Kernel Panic Guard**: Ring 0 exceptions continue to trigger full diagnostic panics in `src/arch/idt.rs` with register dumps, preserving safety invariants.

### 2.5 Architecture & Kernel Integration (`src/arch/idt.rs`, `src/main.rs`)
- **IDT Hooking**: `src/task/mod.rs::init_task_subsystem()` connects `on_timer_tick` to IRQ 0 / Vector 32 and `handle_user_fault` to CPU exceptions.
- **Main Kernel Sequence**: `src/main.rs` initializes GDT/TSS, IDT/PIC, memory management, task subsystem, spawns demo background tasks, enables interrupts (`sti`), and enters `idle_task_entry()`.

---

## 3. Adversarial Review & Stress-Testing

| Challenge / Hypothesis | Attack Scenario / Edge Case | Observed System Behavior | Assessment |
|-----------------------|------------------------------|--------------------------|------------|
| **1. Total Task Starvation / All Blocked** | All user and background worker tasks terminate or enter blocked state. | Scheduler round-robin loop completes 360-degree scan and falls back to PID 0 `[idle]`. `idle_task_entry()` executes `sti; hlt` in a low-power wait loop. | **ROBUST** |
| **2. Admin Termination of Kernel Idle Task** | User or malicious program invokes `kill_process(0)`. | Guard `if pid == 0 { return false; }` prevents PID 0 from being queued or terminated. | **ROBUST** |
| **3. Crash Callback Reentrancy Deadlock** | `CRASH_CALLBACK` attempts to query scheduler process list while scheduler mutex is locked. | `handle_user_fault` explicitly drops `sched` lock before invoking `CRASH_CALLBACK`, then re-acquires lock before `schedule()`. | **ROBUST** |
| **4. Rapid 100x Crash Burst** | Continuous user-mode crashes triggered in rapid succession. | Phase 1 marks tasks as Zombie; Phase 2 reaps physical memory frames and PML4 hierarchies; verified in `test_f6_b01_rapid_100x_crash_burst_stress`. | **ROBUST** |
| **5. Ring 0 Kernel Fault vs Ring 3 User Fault** | Exception occurs while executing kernel code (`(CS & 3) == 0`). | `src/arch/idt.rs` detects kernel privilege, dumps full register state, and calls `panic!()`, halting the CPU safely. | **ROBUST** |
| **6. User Address Space Memory Leak on Crash** | User task with allocated heap and stack pages crashes. | `destroy_user_address_space` traverses PML4 lower-half (0..256), reclaiming all user leaf pages, intermediate page tables, and root PML4. | **ROBUST** |

---

## 4. Empirical Verification & Test Results

### 4.1 Target Compilation Verification
- `cargo check --target x86_64-unknown-none`: **PASS** (0 warnings, 0 errors)
- `cargo build --release --target x86_64-unknown-none`: **PASS** (0 warnings, 0 errors)

### 4.2 Opaque-Box E2E Test Suite (`cargo test --target x86_64-unknown-linux-gnu --manifest-path tests/e2e/Cargo.toml`)
- **Tier 1 (Core Features)**: 61/61 PASSED
  - `test_f6_01_null_pointer_dereference_isolation` ... ok
  - `test_f6_02_divide_by_zero_fault_isolation` ... ok
  - `test_f6_03_out_of_bounds_supervisor_write_isolation` ... ok
  - `test_f6_04_invalid_opcode_fault_isolation` ... ok
  - `test_f6_05_two_phase_deferred_zombie_reclamation` ... ok
  - `test_f7_01_round_robin_runqueue_rotation` ... ok
  - `test_f7_02_priority_tier_scheduling` ... ok
  - `test_f7_03_pid_0_idle_task_protection` ... ok
  - `test_f7_04_cpu_usage_telemetry_calculation` ... ok
  - `test_f7_05_process_table_query` ... ok
- **Tier 2 (Boundary & Stress Cases)**: 61/61 PASSED
  - `test_f7_b01_1000_tasks_runqueue_stress` ... ok
  - `test_f7_b02_spawn_and_kill_500_tasks_rapid_cycle` ... ok
  - `test_f7_b03_kill_non_existent_pid` ... ok
  - `test_f7_b04_all_tasks_blocked_falls_back_to_idle` ... ok
  - `test_f6_b01_rapid_100x_crash_burst_stress` ... ok
  - `test_f6_b03_multiple_crashed_tasks_queued_simultaneously` ... ok
  - `test_f6_b04_kernel_mode_fault_triggers_panic_not_isolate` ... ok
  - `test_f6_b05_fault_on_non_existent_pid` ... ok
- **Tier 3 (Subsystem Combinations)**: 8/8 PASSED
  - Multitasking with terminal execution, activity monitor telemetry, editor memory pressure, and crash during window drag.
- **Tier 4 (Full Scenario Workflows)**: 5/5 PASSED
  - Full desktop multitasking, stress and concurrent fault recovery, process lifecycle, and memory budget validation.

**Total E2E Tests**: **135 PASSED, 0 FAILED**.

---

## 5. Integrity Verification

- **Hardcoded Test Outputs**: None found. All calculations (CPU percentage, memory usage, scheduling decisions) derive dynamically from runtime metrics.
- **Facade Implementations**: None found. Full 15-GPR context swapping, TSS RSP0 updates, CR3 reloads, and recursive 4-level PML4 frame destructions are fully implemented.
- **Shortcuts & Bypasses**: None found.

---

## 6. Review Verdict

**Verdict**: **APPROVE**

Milestone 2 meets all functional, architectural, performance, and crash-resilience criteria specified in `ORIGINAL_REQUEST.md` and `PROJECT.md`.
