# Empirical Challenge Report — Milestone 2: Preemptive Scheduler & Fault Isolation

**Author**: Challenger 1 (critic, specialist)  
**Target Milestone**: Milestone 2 (Features F6 & F7)  
**Date**: 2026-08-30  
**Project**: AegisOS  
**Verdict**: **APPROVE**  

---

## Challenge Summary

**Overall Risk Assessment**: **LOW**

Milestone 2 implements the 100Hz round-robin preemptive scheduler (`src/task/scheduler.rs`), Process Control Block lifecycle (`src/task/pcb.rs`), and hardware Ring 3 fault isolation engine (`src/task/fault.rs`). 

Empirical testing confirmed 100% test pass rate across all 135 opaque-box E2E tests, zero task starvation under 1,000 active processes, zero memory frame leakage across 2-phase deferred zombie reaping, strict privilege separation detecting `(CS & 3) == 3`, and fault recovery for all 4 required exception vectors (#PF, #DE, #GP, #UD).

---

## 1. Test Suite Execution & Pass Rate Telemetry

### A. Full 4-Tier E2E Test Suite (135 Tests)
- **Command**: `cargo test --target x86_64-unknown-linux-gnu --manifest-path tests/e2e/Cargo.toml`
- **Total Tests Executed**: 135
- **Passed**: 135 (100.0%)
- **Failed**: 0 (0.0%)
- **Ignored / Filtered**: 0
- **Total Wall-Clock Execution Time**: ~61.3s (dominated by 1,000-frame double-buffered rendering verification in Scenario 5)

| Test Suite / Binary | Test Count | Passed | Failed | Duration |
|---------------------|------------|--------|--------|----------|
| `tier1_features`    | 61         | 61     | 0      | 0.76s    |
| `tier2_boundary`    | 61         | 61     | 0      | 0.20s    |
| `tier3_combinations`| 8          | 8      | 0      | 0.88s    |
| `tier4_scenarios`   | 5          | 5      | 0      | 59.50s   |
| **Total**           | **135**    | **135**| **0**  | **61.34s**|

### B. Tier 1 Specifically (`tier1_features`)
- **Command**: `cargo test --target x86_64-unknown-linux-gnu --manifest-path tests/e2e/Cargo.toml --test tier1_features`
- **Result**: `ok. 61 passed; 0 failed; finished in 0.76s`
- **Key M2 Tests Verified**:
  - `test_f6_01_null_pointer_dereference_isolation`: Verified Ring 3 `#PF` at null address is trapped, logged, and isolated without kernel panic.
  - `test_f6_02_divide_by_zero_fault_isolation`: Verified `#DE` (vector 0) is caught and reaped.
  - `test_f6_03_out_of_bounds_supervisor_write_isolation`: Verified supervisor write attempts trigger `#PF` logging CR2.
  - `test_f6_04_invalid_opcode_fault_isolation`: Verified `ud2` (`#UD`, vector 6) triggers clean task termination.
  - `test_f6_05_two_phase_deferred_zombie_reclamation`: Verified physical memory frames allocated to crashed tasks are 100% reclaimed on next timer tick.
  - `test_f7_01_round_robin_runqueue_rotation`: Verified runqueue rotates across ready tasks.
  - `test_f7_02_priority_tier_scheduling`: Verified priority scheduling.
  - `test_f7_03_pid_0_idle_task_protection`: Verified PID 0 `[idle]` task cannot be terminated.
  - `test_f7_04_cpu_usage_telemetry_calculation`: Verified CPU usage calculation.
  - `test_f7_05_process_table_query`: Verified PCB table queries.

### C. Tier 3 Specifically (`tier3_combinations`)
- **Command**: `cargo test --target x86_64-unknown-linux-gnu --manifest-path tests/e2e/Cargo.toml --test tier3_combinations`
- **Result**: `ok. 8 passed; 0 failed; finished in 0.88s`
- **Key Integration Workflows Verified**:
  - `test_tier3_01_crash_during_window_drag`: Application fault during active GUI mouse drag isolates app and resets drag state without crashing WM.
  - `test_tier3_02_activity_monitor_with_terminal_command_stream`: Continuous terminal shell commands reflect accurately in process table and CPU telemetry.
  - `test_tier3_03_editor_under_high_memory_allocation_pressure`: Multiline text editor buffer under memory stress remains stable.
  - `test_tier3_04_terminal_spawn_and_activity_monitor_kill`: Shell spawns task, Activity Monitor kills PID, 2-phase reaper reclaims memory frames.
  - `test_tier3_05_mouse_drag_keyboard_typing_during_preemptive_interrupts`: Preemptive 100Hz scheduler ticks do not drop keyboard/mouse events.
  - `test_tier3_06_compositor_60fps_telemetry_during_user_faults`: Desktop compositor continues 60 FPS scanline blitting while user tasks crash.
  - `test_tier3_07_terminal_free_matches_activity_monitor_under_60mb`: RAM stats in shell `free` match Activity Monitor and stay well below 60MB.
  - `test_tier3_08_window_close_traffic_light_during_active_scheduler_loop`: Window close button dispatches PCB termination cleanly.

### D. Bare-Metal Target Compilation
- **Command**: `cargo build --target x86_64-unknown-none`
- **Result**: Clean build in 1.06s with zero errors and zero warnings.

---

## 2. Adversarial Empirical Stress Testing (`tests/stress_m2_scheduler_faults.rs`)

To rigorously test edge cases, an independent empirical stress harness was constructed and executed:

| Challenge # | Stress Dimension | Invariant Tested | Empirical Result | Status |
|---|---|---|---|---|
| **Challenge 1** | **Runqueue Fairness** | 1,000 concurrent tasks + PID 0 executed across 3,003 ticks. Verified every task received exactly 3 quanta without starvation. | 1,001 tasks executed exactly 3 quanta each. Zero starvation. | **PASS** |
| **Challenge 2** | **PID 0 [idle] Immunity** | Attempted `kill_process(0)` and `handle_user_fault(0, ...)`. Verified PID 0 is immune and selected when runqueue is empty. | Kill and user fault rejected. Idle fallback verified. | **PASS** |
| **Challenge 3** | **Fault Isolation & Boundary CR2s** | Tested #PF, #DE, #UD, #GP with extreme CR2 values (`0x0`, `0xFFF`, `0x7FFF_FFFF_FFFF`, `0xFFFF_8000_0000_0000`, `0xFFFF_FFFF_FFFF_FFFF`). Verified Ring 0 exception triggers kernel panic. | All 4 exception vectors trapped. Ring 0 fault panics. All frames reaped. | **PASS** |
| **Challenge 4** | **Rapid Task Lifecycle Churn** | 10,000 pseudo-random cycles of spawn, kill, fault, tick, and reap in an active runqueue. | Zero panics, zero index out-of-bounds, all zombies cleanly reaped. | **PASS** |
| **Challenge 5** | **Telemetry & CPU % Invariant** | Evaluated idle CPU %, 50% load, and bounded telemetry invariant `0 <= CPU% <= 100`. | 0% idle verified, ~50% load verified, invariant holds. | **PASS** |

---

## 3. Analysis of Fault Isolation & Logging Mechanisms

1. **Ring 0 / Ring 3 Privilege Discrimination**:
   - The handler inspects the Code Segment register selector: `(CS & 3) == 3`.
   - Ring 3 user faults (`CS == 0x23` or `CS == 0x1B`) are routed to `handle_user_fault`.
   - Ring 0 supervisor faults (`CS == 0x08`) directly invoke kernel panic diagnostics with register dumps to COM1 serial.

2. **Diagnostic Serial Telemetry**:
   - Verbatim fault output: `[FAULT-ISOLATION] Process PID <pid> ('<name>') crashed due to <fault_name> at RIP 0x<rip> CR2=0x<cr2>`.
   - All fault logs match the required formats in test assertions.

3. **2-Phase Deferred Zombie Frame Reclamation**:
   - **Phase 1 (Fault / Kill Handler)**: Marks PCB as `TaskState::Terminated(reason)`, enqueues `pid` into `zombie_queue`, and triggers immediate reschedule to next ready task.
   - **Phase 2 (Scheduler Context)**: On the subsequent timer tick in safe kernel context, `reap_zombies()` traverses the user PML4 page tables or frame list, calls `free_frame()` for all allocated physical pages, destroys the private PML4 table, and removes the PCB.

---

## 4. Unchallenged Areas

- **Hardware SMP Multi-Core Scheduling**: AegisOS is currently architected as a uniprocessor (UP) x86_64 kernel (1 active CPU core with 100Hz PIT / Local APIC timer). Multi-core AP startup (SMP) is out of scope for Milestone 2.
- **Dynamic File System / Disk Swapping**: AegisOS operates in pure physical RAM with bitmap frame allocation; paging to disk (swap space) is out of scope.

---

## 5. Final Verdict

- **Scheduler Subsystem (F7)**: **APPROVE** (Meets all round-robin, priority, PID 0 idle fallback, and preemption requirements).
- **Fault Isolation Subsystem (F6)**: **APPROVE** (Meets all Ring 3 trap handling, 2-phase deferred zombie reclamation, zero frame leak, and crash resilience requirements).
