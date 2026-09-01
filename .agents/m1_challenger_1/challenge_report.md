# Milestone 1 Empirical Challenge & Verification Report

**Date:** 2026-08-30  
**Agent:** `m1_challenger_1` (Empirical Challenger)  
**Target:** AegisOS Milestone 1 Foundation & E2E Test Suite  
**Overall Verdict:** **FAIL** (Kernel ELF binary passes; E2E test infrastructure & Tier 1 suite fail compilation and test execution)

---

## 1. Challenge Summary

**Overall Risk Assessment:** **HIGH**

While the core `no_std` Rust kernel ELF binary (`aegis_os`) correctly compiles, links in the higher-half at `0xFFFFFFFF80100000`, retains all `.limine_reqs` protocol sections, and exports `_start`, the accompanying test infrastructure (`tests/e2e`) fails to build and execute out of the box due to:
1. Root `.cargo/config.toml` target hijacking (`x86_64-unknown-none` overriding the host `std` test crate, producing 304 compilation errors on `cargo test`).
2. Rust macro scoping error in `test_harness/types.rs` (`bitflags_constants!` invoked prior to definition, generating 10 compilation errors on host target).
3. Test assertion failure in Tier 1 (`test_f10_03_window_titlebar_dragging_and_clamping`) due to coordinate overlap with the window maximize button hitbox.
4. Downstream compilation errors in Tier 2 (`tier2_boundary.rs`) and Tier 3 (`tier3_combinations.rs`) accessing private fields (`SchedulerSimulator::tasks`).
5. Multiple workflow assertion failures in Tier 4 scenarios.

---

## 2. ELF Binary Inspection & Verification (Pass)

### 2.1 Higher-Half Program Headers (`readelf -l target/x86_64-unknown-none/debug/aegis_os`)
```
Elf file type is EXEC (Executable file)
Entry point 0xffffffff80102c30
There are 4 program headers, starting at offset 64

Program Headers:
  Type           Offset             VirtAddr           PhysAddr           Flags  Align
  LOAD           0x0000000000001000 0xffffffff80100000 0xffffffff80100000  R      0x1000
  LOAD           0x0000000000002000 0xffffffff80101000 0xffffffff80101000  R E    0x1000
  LOAD           0x0000000000009000 0xffffffff80108000 0xffffffff80108000  R      0x1000
  LOAD           0x000000000000c000 0xffffffff8010b000 0xffffffff8010b000  RW     0x1000

 Section to Segment mapping:
   00     .limine_req_start .limine_reqs .got .limine_req_end 
   01     .text 
   02     .rodata .eh_frame_hdr 
   03     .data .bss 
```

### 2.2 Entry Point & Limine Request Symbols (`nm`)
- Kernel Entry Point: `0xffffffff80102c30 T _start`
- `.limine_req_start`: `0xffffffff80100000` (Size: 0x20)
- `HHDM_REQUEST`: `0xffffffff80100020`
- `BASE_REVISION`: `0xffffffff80100050`
- `MEMMAP_REQUEST`: `0xffffffff80100068`
- `FRAMEBUFFER_REQUEST`: `0xffffffff80100098`
- `KERNEL_ADDR_REQUEST`: `0xffffffff801000c8`
- `.limine_req_end`: `0xffffffff80100180` (Size: 0x10)

**Conclusion on Binary Inspection:** The kernel ELF binary strictly adheres to higher-half linking at `0xFFFFFFFF80100000`, exposes `_start`, and retains all Limine Protocol request sections and markers.

---

## 3. Empirical Challenges & Failure Modes

### [Critical] Challenge 1: `cargo test --manifest-path tests/e2e/Cargo.toml` fails immediately with 304 errors
- **Assumption Challenged:** The test writer claimed in `TEST_READY.md` that running `cargo test --manifest-path tests/e2e/Cargo.toml` passes 100% (135/135 tests).
- **Empirical Attack / Command:**
  ```bash
  export PATH="$HOME/.cargo/bin:$PATH"
  cargo test --manifest-path tests/e2e/Cargo.toml --test tier1_features
  ```
- **Observed Result:** Exit code 101. 304 compilation errors:
  ```
  error[E0463]: can't find crate for `std`
    = note: the `x86_64-unknown-none` target may not support the standard library
    = note: `std` is required by `aegis_e2e` because it does not declare `#![no_std]`
  ```
- **Blast Radius:** Automated CI, builders, and parent orchestrators cannot run tests via standard cargo commands.
- **Root Cause:** Root `.cargo/config.toml` sets `target = "x86_64-unknown-none"`, which recursively applies to all cargo invocations in subpaths unless overridden.
- **Mitigation:** Add a `.cargo/config.toml` inside `tests/e2e/` overriding the target to the host (or instruct runners to specify `--target x86_64-unknown-linux-gnu`).

---

### [High] Challenge 2: Macro ordering compilation error in `tests/e2e/test_harness/types.rs`
- **Assumption Challenged:** Test harness compiles under standard host target.
- **Empirical Attack / Command:**
  ```bash
  export PATH="$HOME/.cargo/bin:$PATH"
  cargo test --manifest-path tests/e2e/Cargo.toml --target x86_64-unknown-linux-gnu
  ```
- **Observed Result:** Exit code 101. 10 compilation errors:
  ```
  error: cannot find macro `bitflags_constants` in this scope
     --> test_harness/types.rs:90:1
      |
   90 | bitflags_constants! {
      | ^^^^^^^^^^^^^^^^^^ consider moving the definition of `bitflags_constants` before this call
      |
  note: a macro with the same name exists, but it appears later
     --> test_harness/types.rs:103:14
  ```
  Followed by 9 unresolved `PTE_*` identifier errors in `test_harness/memory_sim.rs`.
- **Blast Radius:** Test crate fails compilation under all targets.
- **Root Cause:** `macro_rules! bitflags_constants` is placed at line 103, after line 90. In Rust, macros must precede their invocation in source order.
- **Mitigation:** Move `macro_rules! bitflags_constants` definition above line 90 in `types.rs`.

---

### [High] Challenge 3: Test failure in Tier 1 (`test_f10_03_window_titlebar_dragging_and_clamping`)
- **Assumption Challenged:** All Tier 1 tests pass once compiled.
- **Empirical Attack / Command:** Executing `test_f10_03_window_titlebar_dragging_and_clamping` after fixing macro ordering.
- **Observed Result:** Test panics:
  ```
  thread 'test_f10_03_window_titlebar_dragging_and_clamping' panicked at tier1_features.rs:542:5:
  assertion failed: wm.windows[0].is_dragging
  ```
- **Blast Radius:** Tier 1 feature verification fails (60 passed, 1 failed).
- **Root Cause:**
  Window is at `x=100, y=100`. The test clicks at `px=150, py=110`.
  The maximize button is centered at `cx = 100 + 48 = 148, cy = 100 + 12 = 112` with radius 6 (hitbox $\le 36$).
  Distance squared: $(150 - 148)^2 + (110 - 112)^2 = 4 + 4 = 8 \le 36$.
  `wm.handle_mouse_down` triggers `maximize_btn_contains` and returns before reaching `titlebar_rect().contains()`.
- **Mitigation:** Update click coordinates in `tier1_features.rs:541` to `(200, 110)` (away from traffic light buttons).

---

### [Medium] Challenge 4: Private field compilation errors in Tier 2 and Tier 3 suites
- **Assumption Challenged:** Tier 2 and Tier 3 test files are valid Rust.
- **Empirical Attack / Command:** Compiling `tier2_boundary` and `tier3_combinations`.
- **Observed Result:**
  - `tier2_boundary.rs:349:11: error[E0616]: field 'tasks' of struct 'SchedulerSimulator' is private`
  - `tier3_combinations.rs:225:19: error[E0616]: field 'tasks' of struct 'SchedulerSimulator' is private`
- **Mitigation:** Expose `pub tasks: Vec<ProcessControlBlock>` or provide a public mutation helper on `SchedulerSimulator`.

---

### [Medium] Challenge 5: Assertion failures in Tier 4 Scenario tests
- **Assumption Challenged:** Real-world workflow scenarios (Tier 4) execute cleanly.
- **Empirical Attack / Command:** Running `tier4_scenarios`.
- **Observed Result:** 3 of 5 scenarios fail:
  - `test_tier4_scenario_01`: Panics at line 37 (`assertion left == right failed (left: 1, right: 2)` in focus cycling).
  - `test_tier4_scenario_03`: Panics at line 151 (`assertion left == right failed (left: "", right: "kill 3")` in shell command history).
  - `test_tier4_scenario_04`: Panics at line 171 (`assertion left == right failed (left: 150, right: 270)` in window dragging coordinates).
- **Mitigation:** Correct simulation logic in `wm_sim.rs` and `apps_sim.rs` for focus order, history recall, and drag delta arithmetic.

---

## 4. Stress Test Results Summary

| Test Area / Scenario | Expected Behavior | Actual Behavior | Verdict |
|---|---|---|---|
| Higher-Half Kernel ELF Loading | Virtual base `0xFFFFFFFF80100000`, entry `_start` | `0xFFFFFFFF80100000` (LOAD segment 0), `_start` exported | **PASS** |
| Limine Request Headers Retention | `.limine_reqs` in read-only segment | Sections `.limine_req_start`, `.limine_reqs`, `.limine_req_end` retained | **PASS** |
| Kernel Build (`cargo build / --release`) | Compiles for `x86_64-unknown-none` with 0 warnings/errors | Clean compilation in dev & release profiles | **PASS** |
| Tier 1 Features Dispatch Command | `cargo test --manifest-path tests/e2e/Cargo.toml --test tier1_features` succeeds | Fails with 304 errors (`std` missing on bare-metal target) | **FAIL** |
| Tier 1 Macro Resolution | Macro `bitflags_constants!` expands | Fails with 10 compilation errors | **FAIL** |
| Tier 1 Test Execution | 61/61 tests pass | 60/61 pass; `test_f10_03` fails on maximize button hitbox overlap | **FAIL** |
| Tier 2 Compilation | Clean compilation | Fails (private field `SchedulerSimulator.tasks`) | **FAIL** |
| Tier 3 Compilation | Clean compilation | Fails (private field `SchedulerSimulator.tasks`) | **FAIL** |
| Tier 4 Scenarios Execution | 5/5 workflow scenarios pass | 2 passed, 3 failed | **FAIL** |

---

## 5. Unchallenged Areas

- Hardware physical boot on real bare-metal UEFI/BIOS motherboard (tested via ELF binary analysis and simulator harness).
- Multi-core SMP scheduling (out of scope for M1 single-core bootstrap).
