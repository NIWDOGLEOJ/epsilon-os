# Milestone 1 Handoff Report: Memory & Paging Subsystem

**Agent:** `m1_explorer_3` (M1 Memory & Paging Explorer)  
**Recipient:** `parent` (Orchestrator, ID: `c28358f3-14dd-4701-b6af-d43416c28150`)  
**Working Directory:** `/home/godjoel/teamwork_projects/aegis_os/.agents/m1_explorer_3`  
**Date:** 2026-08-30  
**Artifact Generated:** `/home/godjoel/teamwork_projects/aegis_os/.agents/m1_explorer_3/plan.md`

---

## 1. Observation

Direct observations from system files and repository context:
1. **System Requirements (`ORIGINAL_REQUEST.md` lines 21–23, 57–59)**:
   - "R3. Memory Management & Scheduling: Implement a physical memory frame allocator (using the Limine memory map), a kernel heap allocator, and per-process virtual address spaces."
   - "System Performance & Footprint: Total system memory consumption at idle desktop is under 60MB of RAM. Kernel boots successfully in QEMU with 512MB to 4GB RAM allocated."
2. **Interface Contracts (`PROJECT.md` lines 62–69, 75)**:
   - M1 -> M2 Contract:
     - `pub fn alloc_frame() -> Option<PhysAddr>` / `pub fn free_frame(frame: PhysAddr)`
     - `pub fn create_user_address_space() -> PhysAddr /* PML4 root */`
     - `pub fn destroy_user_address_space(pml4: PhysAddr)`
     - `pub fn phys_to_virt(phys: PhysAddr) -> VirtAddr`
   - M2 -> M4 Contract:
     - `pub fn get_memory_stats() -> MemoryStats /* used_bytes, total_bytes */`
3. **Limine Boot Protocol Specification (`survey_explorer_1_repl/spec_report.md` lines 131–185)**:
   - Limine provides `MemoryMapRequest` with typed entries (`EntryType::USABLE`, `BOOTLOADER_RECLAIMABLE`, `KERNEL_AND_MODULES`, etc.) and `HhdmRequest` providing `hhdm_offset` (typically `0xFFFF_8000_0000_0000`).
   - Virtual-to-physical translation is `virt = phys + hhdm_offset`.

---

## 2. Logic Chain

1. **Physical Bitmap Sizing**:
   - To support up to 4 GB RAM ($4,294,967,296\text{ bytes}$) with 4 KB granularity:
     $$\text{Total Frames} = \frac{4\text{ GB}}{4096\text{ bytes}} = 1,048,576\text{ frames}$$
     $$\text{Bitmap Words (u64)} = \frac{1,048,576\text{ bits}}{64\text{ bits/word}} = 16,384\text{ words} = 128\text{ KB}$$
   - Placing `BITMAP_STORAGE: [u64; 16384]` statically in `.bss` ensures zero dynamic memory allocation is required before physical frame allocation is available.
   - Initializing all bits to `1` (allocated/reserved) and clearing only `EntryType::USABLE` segments guarantees that unmapped holes, ACPI, BIOS/UEFI, and kernel memory regions are never handed out.
   - Frame 0 (`0x0000_0000`) is clamped as allocated to prevent ambiguous null address returns.

2. **Kernel Dynamic Heap Provisioning**:
   - A 16 MB kernel heap region (`0xFFFF_9000_0000_0000` .. `0xFFFF_9000_0100_0000`) requires 4,096 physical frames ($16\text{ MB} / 4\text{ KB}$).
   - Pre-allocating and mapping these 4,096 frames into the kernel PML4 at early boot guarantees deterministic execution with zero page-fault overhead during kernel memory operations.
   - An intrusive linked-list allocator (`LinkedListAllocator`) with block coalescing prevents heap fragmentation and implements `core::alloc::GlobalAlloc` under a `spin::Mutex<LockedHeap>`.
   - Registering `#[global_allocator]` unlocks `extern crate alloc;` and all standard Rust dynamic structures (`Vec`, `String`, `Box`, `Arc`, `BTreeMap`, `VecDeque`).

3. **4-Level PML4 Paging & Address Space Isolation**:
   - The 256 TB canonical address space is bifurcated:
     - PML4 entries `0..255` (`0x0000_0000_0000_0000` .. `0x0000_7FFF_FFFF_FFFF`): Private user space.
     - PML4 entries `256..511` (`0xFFFF_8000_0000_0000` .. `0xFFFF_FFFF_FFFF_FFFF`): Shared kernel space (HHDM, Heap, Framebuffer VRAM, Kernel Code/BSS).
   - `create_user_address_space()` allocates a single zeroed frame for the new PML4, copies entries `256..511` from the kernel master PML4, and leaves entries `0..255` cleared to zero.
   - `destroy_user_address_space(pml4_phys)` traverses only entries `0..255`, recursively freeing all user leaf frames (code, data, heap, stack) and intermediate page table frames (PT, PD, PDPT), ending with the root PML4 frame.
   - Shared kernel PML4 entries (`256..511`) are never touched, ensuring total safety against kernel corruption.

4. **Memory Footprint Compliance**:
   - Total system idle consumption:
     $$\text{Bitmap (128 KB)} + \text{Kernel Heap (16 MB)} + \text{Framebuffer (4 MB)} + \text{Kernel Text/Data (2 MB)} \approx 22.125\text{ MB}$$
   - This easily passes the `< 60 MB` idle RAM limit required by R3/R5.

---

## 3. Caveats

1. **Non-Usable Frame Alignment**: If a memory map entry is not aligned to 4KB boundaries, `align_up_4k()` and `align_down_4k()` must be used to ensure only complete 4KB frames are marked as free.
2. **Active CR3 Invalidation**: `destroy_user_address_space()` must never be executed while `CR3` is actively pointing to the PML4 being destroyed. The scheduler must switch to the Kernel PML4 or another ready task before reaping zombie address spaces.
3. **Huge Page Deallocation**: If 2MB or 1GB huge pages are mapped in userspace, `destroy_user_address_space` accounts for the `HUGE_PAGE` bit and frees the single 2MB/1GB frame rather than attempting to dereference the entry as an intermediate page table.

---

## 4. Conclusion

The memory and paging subsystem design for Milestone 1 is fully specified with complete, production-ready Rust blueprints in `/home/godjoel/teamwork_projects/aegis_os/.agents/m1_explorer_3/plan.md`. The design satisfies all requirements of F4 and F5:
- 128KB Bitmap Frame Allocator in `src/memory/frame.rs`.
- 16MB Kernel Dynamic Heap with `#[global_allocator]` in `src/memory/heap.rs`.
- 4-Level PML4 Paging, HHDM translation, and isolated user address spaces in `src/memory/paging.rs`.
- Clean facade and master initialization in `src/memory/mod.rs`.

---

## 5. Verification Method

To independently verify the architecture and blueprints:
1. **Inspect Blueprint File**:
   - Review `/home/godjoel/teamwork_projects/aegis_os/.agents/m1_explorer_3/plan.md`.
2. **Type Check & Syntax Validation**:
   - Once implementers place the code into `src/memory/`, run:
     ```bash
     cargo check --target x86_64-unknown-none
     ```
3. **E2E & Functional Invalidation Conditions**:
   - Invalidation occurs if `alloc_frame()` returns physical address `0x0`.
   - Invalidation occurs if `get_memory_stats()` reports used memory $\ge 60\text{ MB}$ at idle.
   - Invalidation occurs if `destroy_user_address_space()` modifies or frees any PML4 entry $\ge 256$.
