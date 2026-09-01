## 2026-08-30T13:03:24Z

You are the M2 Worker for AegisOS.
Your working directory is /home/godjoel/teamwork_projects/aegis_os/.agents/m2_worker_1.
Project root is /home/godjoel/teamwork_projects/aegis_os.
You MUST read /home/godjoel/teamwork_projects/aegis_os/.agents/ORIGINAL_REQUEST.md and /home/godjoel/teamwork_projects/aegis_os/PROJECT.md.

MANDATORY INTEGRITY WARNING:
DO NOT CHEAT. All implementations must be genuine. DO NOT hardcode test results, create dummy/facade implementations, or circumvent the intended task. A auditor will independently verify your work. Integrity violations WILL be detected and your work WILL be rejected.

Your mission for Milestone 2 (M2):
Implement the Preemptive Round-Robin Multitasking Scheduler, Process Control Blocks (PCB), Ring 3 Fault Isolation, and 2-Phase Zombie Reaping.

Read the detailed blueprints from survey_explorer_2:
- /home/godjoel/teamwork_projects/aegis_os/.agents/survey_explorer_2/analysis.md

Files you own and must implement/update:
1. `src/task/pcb.rs`:
   - Process Control Block (`ProcessControlBlock`), `ProcessId`, `TaskState` (`Ready`, `Running`, `Blocked`, `Terminated`).
   - `TaskContext` (R15..RAX, RIP, CS, RFLAGS, RSP, SS), kernel stack pointer, user page table physical address (`pml4_root`), allocated memory frames list, process name, CPU ticks counter.
2. `src/task/context.rs`:
   - Assembly context switch routines (or interrupt-based context switching via `InterruptContext`).
3. `src/task/scheduler.rs`:
   - 100Hz Round-Robin Preemptive Scheduler under `spin::Mutex<Scheduler>`.
   - Runqueue, terminated zombie queue, `spawn_process()`, `kill_process()`, `schedule()`, `yield_now()`, `exit_process()`, `get_process_list()`, `get_cpu_usage()`, `get_memory_stats()`.
   - Timer IRQ 0 / Vector 32 handler switching tasks, updating `TSS.RSP0`, switching CR3 to next process PML4.
   - Idle task loop performing Phase 2 deferred zombie reaping (calling `destroy_user_address_space()` and freeing PCB).
4. `src/task/fault.rs`:
   - Ring 3 fault handler catching `#PF` (vector 14), `#DE` (vector 0), `#GP` (vector 13), `#UD` (vector 6).
   - Serial logging: `[FAULT-ISOLATION] Process PID X ('name') crashed due to <FaultName> at RIP 0x... CR2=0x...`.
   - Mark faulting task `Terminated`, push to `zombie_queue`, call scheduler to switch to next ready task without kernel panic or desktop freeze.
5. `src/task/mod.rs`: Task subsystem initialization and public re-exports matching PROJECT.md interface contracts.
6. `src/arch/idt.rs`: Connect vector 32 (IRQ 0 Timer) and exception vectors (0, 6, 13, 14) to the task scheduler and fault isolation engine.
7. `src/main.rs`: Initialize scheduler and spawn background worker tasks / idle task.
8. Fix E2E test harness files in `tests/e2e/`:
   - Create `tests/e2e/.cargo/config.toml` specifying `[build] target = "x86_64-unknown-linux-gnu"`.
   - In `tests/e2e/test_harness/types.rs`, place `macro_rules! bitflags_constants` before its invocation.
   - Fix `SchedulerSimulator` field access or add public helper methods.
   - In `tests/e2e/tier1_features.rs:541`, use `(200, 110)` for titlebar dragging click.
   - Run `cargo test --manifest-path tests/e2e/Cargo.toml` and ensure all 135 tests pass!

Verification commands:
- `export PATH="$HOME/.cargo/bin:$PATH"`
- `cargo check --target x86_64-unknown-none`
- `cargo build --release --target x86_64-unknown-none`
- `cargo test --manifest-path tests/e2e/Cargo.toml`

Write your handoff report to /home/godjoel/teamwork_projects/aegis_os/.agents/m2_worker_1/handoff.md and send a message to parent when done.
