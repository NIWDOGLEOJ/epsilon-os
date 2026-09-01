# Milestone 1 Challenge Report: Memory Subsystem & Isolation Boundaries

**Challenger**: Challenger 2 (Empirical Challenger: Critic, Specialist)  
**Target Milestone**: Milestone 1 (Bare-Metal Foundation, Memory Subsystem & Architecture)  
**Target Scope**: 128KB Bitmap Frame Allocator, 16MB Kernel Heap, 4-Level PML4 Paging, Ring 0/Ring 3 Isolation Boundaries, and E2E Tier 2 Boundary Suite  
**Date**: 2026-08-30  

---

## Challenge Summary

**Overall risk assessment**: MEDIUM

The core Milestone 1 kernel implementation (`src/memory/`, `src/arch/`, `src/main.rs`) is architecturally sound and compiles cleanly for `x86_64-unknown-none`. The memory subsystem algorithms (bitmap frame allocation, 16MB heap mapping, PML4 paging hierarchy, and user address space creation/destruction) passed all empirical stress tests across 4GB RAM boundary spaces, heap fragmentation, and page table walks.

However, two critical toolchain and test harness compilation bugs currently prevent the dispatch command `cargo test --manifest-path tests/e2e/Cargo.toml --test tier2_boundary` from executing out-of-the-box, alongside two memory edge-case vulnerabilities in huge page reclamation and reserved frame freeing.

---

## Challenges

### [High] Challenge 1: Default Cargo Target Inheritance Breaks `tests/e2e` Compilation

- **Assumption challenged**: The test suite `tests/e2e` can be executed directly using `cargo test --manifest-path tests/e2e/Cargo.toml --test tier2_boundary`.
- **Attack scenario / Root cause**:
  The project root contains `.cargo/config.toml` configuring `[build] target = "x86_64-unknown-none"`. When invoking `cargo test --manifest-path tests/e2e/Cargo.toml`, Cargo automatically inherits `.cargo/config.toml` from the workspace parent directory. Because `tests/e2e` is an opaque simulator requiring `std` (`std::collections::HashMap`, `Vec`, `String`), compiling for the `#![no_std]` target `x86_64-unknown-none` causes 304 compilation errors (`undeclared type Vec`, `cannot find type String`, `method not found in &str`).
- **Blast radius**: Prevents automated CI and manual execution of all E2E test tiers (Tier 1..4) without explicitly specifying host target flags.
- **Mitigation**: Create `tests/e2e/.cargo/config.toml` with `[build] target = "x86_64-unknown-linux-gnu"` (or enforce `--target x86_64-unknown-linux-gnu` in test dispatch instructions).

---

### [High] Challenge 2: Lexical Macro Ordering Error in `test_harness/types.rs`

- **Assumption challenged**: Running with `--target x86_64-unknown-linux-gnu` allows `tier2_boundary` to compile and run.
- **Attack scenario / Root cause**:
  In `/home/godjoel/teamwork_projects/aegis_os/tests/e2e/test_harness/types.rs`, line 90:
  ```rust
  bitflags_constants! {
      pub const PTE_PRESENT: u64 = 1 << 0;
      pub const PTE_WRITABLE: u64 = 1 << 1;
      pub const PTE_USER: u64 = 1 << 2;
      ...
  }
  ```
  The macro `macro_rules! bitflags_constants` is defined on line 103, *after* its invocation on line 90. In Rust, `macro_rules!` definitions are not available before their lexical position in the file. This results in:
  `error: cannot find macro bitflags_constants in this scope` (line 90) and 9 subsequent `cannot find value PTE_PRESENT / PTE_USER / PTE_WRITABLE` errors across `test_harness/memory_sim.rs`.
- **Blast radius**: Completely blocks compilation of `aegis_e2e_tests` library and all test binaries (`tier1_features`, `tier2_boundary`, `tier3_combinations`, `tier4_scenarios`, `e2e_runner`).
- **Mitigation**: Move `macro_rules! bitflags_constants` definition above line 90 in `tests/e2e/test_harness/types.rs`.

---

### [Medium] Challenge 3: Incomplete Frame Reclamation on Huge Pages in `destroy_user_address_space`

- **Assumption challenged**: `destroy_user_address_space()` safely and completely reclaims all memory when huge pages (2MB or 1GB) are present in the page table hierarchy.
- **Attack scenario / Code analysis**:
  In `src/memory/paging.rs`, lines 388-393 and 404-409:
  ```rust
  if pd_entry.is_huge() {
      // 2MB huge page
      free_frame(pd_entry.addr());
      frames_reclaimed += 512;
      continue;
  }
  ```
  `free_frame(frame: PhysAddr)` in `src/memory/frame.rs` operates on a single 4KB frame. When `free_frame(pd_entry.addr())` is called for a 2MB page, only the base frame (index $0$) is freed in `BITMAP_STORAGE`. The remaining 511 frames ($511 \times 4096 = 2,093,056$ bytes) remain permanently marked as allocated in the bitmap allocator, resulting in physical RAM leak.
- **Blast radius**: Memory leakage if huge pages are mapped in userspace during future milestones.
- **Mitigation**: Loop over all 512 frames (`0..512`) calling `free_frame(PhysAddr::new(base + i * 4096))` or implement a multi-frame `free_contiguous()` helper in `frame.rs`.

---

### [Low] Challenge 4: Absence of Usable Memory Boundary Mask in Bitmap `free_frame`

- **Assumption challenged**: `free_frame()` can only free valid usable RAM frames.
- **Attack scenario / Code analysis**:
  In `src/memory/frame.rs`, `init()` marks all 1,048,576 frames as allocated (`1`) and clears only usable RAM regions to `0`. Non-RAM physical addresses (e.g. MMIO, ACPI tables, BIOS ROM, PCI config spaces) retain bit `1`. If an erroneous call is made to `free_frame(mmio_addr)` on a reserved physical address, `free_frame()` detects bit `1`, clears it to `0`, and adds it to the pool of allocatable RAM. Subsequent `alloc_frame()` calls can then hand out device MMIO or firmware ROM as standard RAM, risking silent hardware memory corruption.
- **Blast radius**: Corruption if buggy kernel or driver code frees an invalid physical address.
- **Mitigation**: Maintain a secondary `USABLE_MASK_BITMAP` (128KB) or validate the target frame index against the memory map prior to freeing.

---

## Stress Test Results

Stress harness `tests/stress_m1_memory.rs` was written and executed to empirically test the memory and isolation logic under extreme load:

| Scenario / Test Case | Expected Behavior | Actual Behavior | Result |
|---|---|---|---|
| **1. 4GB RAM Exhaustion Stress** (1,048,576 frames) | Allocate all usable frames, ensure zero duplicates, preserve Frame 0, return `None` on exhaustion | 1,047,808 unique frames allocated without collision; frame 0 excluded; `alloc_frame() == None` on exhaustion | **PASS** |
| **2. Fragmentation & Recycling Stress** | Free 523,904 alternating frames (even/odd), verify `allocated_frames` decreases, reallocate all recycled frames | Successfully freed 523,904 frames and reallocated all of them; `alloc_frame() == None` after full re-exhaustion | **PASS** |
| **3. Null / Unaligned / OOB Free Guards** | Reject freeing frame 0, `0x1005` (unaligned), and `>= 4GB` physical address | All invalid free attempts returned `false` without modifying bitmap state | **PASS** |
| **4. 16MB Kernel Heap Mapping Geometry** | Map 4096 pages at `0xFFFF_9000_0000_0000` (PML4 index 288); verify higher-half canonical mapping and supervisor-only flags (`!PTE_USER`) | All 4096 pages mapped and translated; canonical higher-half PML4 entry 288 confirmed; verified `WRITABLE` and `!USER_ACCESSIBLE` | **PASS** |
| **5. Isolated User Address Space Lifecycle** | Create user PML4 (entries 256..511 copied from kernel, 0..255 empty); map 100 user pages with `PTE_USER`; destroy user space and verify frame accounting | Higher half shared; lower half initially unmapped; 100 user pages mapped and verified; `destroy_user_address_space` reclaimed 104 frames (100 leaf + PT/PD/PDPT/PML4) | **PASS** |
| **6. Kernel Bare-Metal Compilation** (`x86_64-unknown-none`) | Clean compile with 0 errors / warnings | `cargo build --release --target x86_64-unknown-none` succeeded in 0.01s | **PASS** |
| **7. E2E Tier 2 Boundary Test Suite Execution** | `cargo test --manifest-path tests/e2e/Cargo.toml --test tier2_boundary` | Failed to compile due to missing target override and macro ordering error (Challenges 1 & 2) | **FAIL** |

---

## Unchallenged Areas

- **100Hz Preemptive Scheduler & Context Switch Routine (`src/task/`)**: Out of scope for Milestone 1; planned for Milestone 2.
- **PS/2 Mouse & Keyboard Hardware ISR Integration**: Deferred to Milestone 3 (ISR stubs registered, drivers to be activated in M3).
- **Linear RGB Double-Buffered Compositor Engine**: Deferred to Milestone 3.

---

## Overall Assessment & Recommendation

- **Kernel Implementation**: **APPROVED (Robust)**.
- **Test Infrastructure (`tests/e2e`)**: **NEEDS FIX (Challenges 1 & 2)**.
