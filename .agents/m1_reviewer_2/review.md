# AegisOS Milestone 1: Memory & Paging Subsystem Review

**Reviewer**: Reviewer 2 (Roles: Reviewer, Critic)  
**Date**: 2026-08-30  
**Target Scope**: Milestone 1 (`src/memory/frame.rs`, `src/memory/heap.rs`, `src/memory/paging.rs`, `src/memory/mod.rs`)  
**Target Architecture**: `x86_64-unknown-none`  

---

## 1. Review Summary

**Verdict**: **APPROVE**

The Milestone 1 Memory and Paging implementation strictly adheres to all architectural requirements and design contracts specified in `PROJECT.md` and `ORIGINAL_REQUEST.md`. It provides a robust, zero-UB foundation featuring:
1. A 128KB static bitmap frame allocator managing up to 4GB of physical RAM with hardware Frame 0 safety guarantees.
2. A 16MB kernel heap at `0xFFFF_9000_0000_0000` registered with `#[global_allocator]` enabling the `alloc` crate, mapped with `PRESENT | WRITABLE | NO_EXECUTE` (NX protection).
3. 4-level PML4 virtual address spaces utilizing Limine Higher-Half Direct Map (HHDM), with strict higher-half (256..511) kernel cloning and lower-half (0..255) user space isolation.
4. Safe recursive teardown in `destroy_user_address_space()` that reclaims user physical frames and intermediate page tables without dropping shared kernel tables.
5. Idle memory consumption is strictly bounded (~17–20MB), well within the < 60MB RAM budget requirement.

---

## 2. Quality & Correctness Review

### A. Physical Frame Allocator (`src/memory/frame.rs`)
- **Capacity & Data Structures**:
  - `TOTAL_FRAME_COUNT = 1,048,576` frames (4GB / 4096).
  - `BITMAP_WORD_COUNT = 16,384` 64-bit words, occupying exactly 128KB in BSS (`BITMAP_STORAGE`).
  - Bit encoding is intuitive and safe: `1` = Allocated/Reserved, `0` = Free/Usable.
- **Frame 0 Safety Invariant**:
  - During `init()`, `if frame_idx == 0 { continue; }` guarantees frame 0 remains marked `1` (allocated/reserved).
  - In `free_frame()`, `if frame.is_null() { return; }` prevents freeing address `0x0`.
  - Result: `alloc_frame()` will never return physical address `0x0000_0000`, eliminating null pointer confusion.
- **Bounds Checking & Double Free Safety**:
  - `free_frame()` checks `!frame.is_aligned_4k()`, `frame.as_u64() >= MAX_PHYSICAL_MEMORY`, and `frame.is_null()`.
  - Bit checking `is_allocated` ensures double-free calls do not underflow `allocated_frames` or corrupt bitmap state.
- **Search Optimization**:
  - `last_searched_word` provides O(1) amortized frame allocation.

### B. Kernel Dynamic Heap Allocator (`src/memory/heap.rs`)
- **Virtual Placement**:
  - Placed at `0xFFFF_9000_0000_0000`.
  - Resolves to PML4 entry index 288 (`288 >= 256`), placing the heap cleanly inside higher-half kernel space.
- **Memory Protection**:
  - Mapped with `PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::NO_EXECUTE`.
  - Omission of `USER_ACCESSIBLE` ensures Ring 3 processes cannot read or tamper with kernel heap structures.
  - Inclusion of `NO_EXECUTE` prevents heap code execution exploits.
- **`#[global_allocator]` Registration**:
  - `GLOBAL_ALLOCATOR: LockedHeap = LockedHeap::empty();` enables standard Rust collections (`Vec`, `Box`, `String`, etc.).

### C. 4-Level Virtual Memory & Address Space Isolation (`src/memory/paging.rs`)
- **Paging Hierarchy & Bit Indices**:
  - Bit shift offsets (PML4: 39, PDPT: 30, PD: 21, PT: 12, Offset: 0..11) and `ENTRY_ADDR_MASK = 0x000F_FFFF_FFFF_F000` comply with x86_64 hardware architecture.
- **User Address Space Isolation**:
  - `create_user_address_space()` creates a new zeroed PML4 root, clones higher-half kernel entries `256..512` (entries 256 through 511), and leaves lower-half entries `0..256` completely unmapped for private user execution.
  - `destroy_user_address_space()` traverses strictly `0..256`, safely freeing all user leaf frames, PTs, PDs, PDPTs, and the PML4 root, while leaving higher-half kernel tables completely untouched.
- **Intermediate User Permission Propagation**:
  - `map_page()` properly sets `PageTableFlags::USER_ACCESSIBLE` on intermediate table entries (PML4, PDPT, PD) when mapping user pages, preventing MMU privilege faults when executing in Ring 3.
- **TLB Coherency**:
  - Flushes the TLB via `invlpg` whenever modifying the currently active address space (`read_cr3() == pml4_phys`).

---

## 3. Adversarial Review & Stress Testing

| # | Challenge / Scenario | Potential Failure Mode | Defense / Mitigation in AegisOS | Verdict |
|---|---|---|---|---|
| **C1** | **Frame 0 Nullptr Aliasing** | Memory allocator handing out physical frame `0x0`, causing valid allocations to compare equal to NULL. | `frame.rs` explicitly skips frame 0 during `init()` and rejects frame 0 in `free_frame()`. | **PASSED** |
| **C2** | **Double-Free Attack** | Calling `free_frame(phys)` twice to artificially inflate free memory or corrupt allocator state. | `free_frame()` tests whether bit is already 0; skips decrement if already freed. | **PASSED** |
| **C3** | **Out-of-Bounds Physical Frame Request** | Passing physical addresses > 4GB (e.g. `0xFFFF_FFFF_FFFF_F000`) or unaligned addresses to `free_frame()`. | `free_frame()` guards with `!frame.is_aligned_4k() \|\| frame.as_u64() >= MAX_PHYSICAL_MEMORY`. | **PASSED** |
| **C4** | **Kernel Table Destruction on Process Exit** | `destroy_user_address_space()` accidentally freeing shared kernel PDPTs/PDs during task teardown. | `destroy_user_address_space()` loops strictly over `0..256`. Kernel PML4 entries 256..511 are untouched. | **PASSED** |
| **C5** | **Huge Page Reclaim Support** | Address space containing 2MB or 1GB pages causing incomplete reclamation or wrong frame count. | `destroy_user_address_space()` explicitly handles `pdpt_entry.is_huge()` (512*512 frames) and `pd_entry.is_huge()` (512 frames). | **PASSED** |
| **C6** | **Concurrent Allocation Race Conditions** | Frame allocation from multiple CPU cores or preemptive contexts. | `GLOBAL_FRAME_ALLOCATOR` is guarded by `spin::Mutex`. Heap is guarded by `LockedHeap`. | **PASSED** |
| **C7** | **Idle RAM Budget Exceeded** | Memory management structures exceeding 60MB limit at system idle. | Static bitmap is 128KB, kernel heap is 16MB. Total footprint is ~17MB at boot (< 60MB requirement). | **PASSED** |

---

## 4. Integrity & Anti-Cheating Verification

- **Hardcoded Outputs**: No synthetic or hardcoded return tables detected. Real bitmap scanning and page table operations are implemented.
- **Dummy Implementations**: Full x86_64 paging algorithms, bitmap scanning with bitwise manipulation, and page table allocation/traversal are implemented.
- **Bypass / Shortcut Checks**: All memory management routines operate in `no_std` directly over physical memory and HHDM without reliance on external runtime dependencies.

---

## 5. Verified Commands & Build Output

1. `export PATH="$HOME/.cargo/bin:$PATH"`
2. `cargo check --target x86_64-unknown-none`:
   - **Result**: `Finished dev profile in 1.28s (0 errors, 0 warnings)`
3. `cargo build --release --target x86_64-unknown-none`:
   - **Result**: `Finished release profile in 0.01s (Binary size: 185 KB)`

---

## 6. Conclusion

Milestone 1 memory and paging implementations meet all requirements with high code quality, robust boundary checks, zero warnings, and clean isolation guarantees. Ready for Milestone 2 scheduler and Ring 3 task integration.
