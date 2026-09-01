# AegisOS Architecture Specification: Fault Isolation, Multitasking Scheduler, and Memory Management

**Author:** Fault Isolation & Scheduler Explorer (`survey_explorer_2`)  
**Date:** 2026-08-30  
**Target Architecture:** `x86_64` (bare-metal `no_std` Rust, Limine bootloader protocol)  
**Requirements Addressed:** R1 (Kernel Architecture), R2 (Fault Isolation & Crash Resilience), R3 (Memory Management & Preemptive Scheduling)

---

## 1. Executive Summary & Architectural Overview

AegisOS is engineered to provide uncompromising hardware-enforced fault isolation: **no userspace application crash (whether Null Pointer Dereference, Page Fault, Divide-by-Zero, Invalid Opcode, or General Protection Fault) shall ever panic the kernel, corrupt kernel memory, or freeze running applications / the graphical desktop.**

To achieve this, AegisOS enforces a strict 4-pillar architectural foundation:
1. **Privilege & Exception Demarcation**: Hardware privilege levels (Ring 0 Kernel vs. Ring 3 Userspace) are tracked on every interrupt and exception using the CPU-pushed `CS` selector (`CS & 0x03 == 3`). Ring 0 exceptions panic with diagnostic dumps; Ring 3 exceptions cleanly trigger process reaping and context rescheduling without unwinding or panicking the kernel.
2. **Asynchronous Multi-Phase Resource Reclamation (Zombie Reaping)**: Because a running CPU core cannot deallocate the stack or page tables (CR3) it is actively executing on, task termination is divided into *Phase 1: Mark & Deschedule* (synchronous) and *Phase 2: Deferred Frame Reclamation* (asynchronous by kernel reaper / idle task).
3. **Preemptive Round-Robin Scheduling**: Driven by hardware timer interrupts (PIT / Local APIC), the scheduler saves full general-purpose register (GPR) state, switches CR3 address spaces, updates TSS `RSP0` for the next Ring 3 privilege transition, and dispatches tasks from a priority-aware round-robin runqueue.
4. **Isolated Virtual Address Spaces & Bitmap Frame Allocator**: A highly compact 128 KB physical bitmap manages all 4 GB RAM ($1,048,576 \times 4\text{ KB}$ frames), keeping idle memory well under the 60 MB budget. Per-process virtual address spaces clone the top 256 PML4 entries (higher-half shared kernel space) and maintain an isolated lower-half (entries 0..255) for private user code, data, heap, and guarded stacks.

```
+-----------------------------------------------------------------------------------+
|                                 AegisOS CPU Core                                  |
|                                                                                   |
|  +-------------------------------------+   +------------------------------------+ |
|  |       Ring 3: Userspace Tasks       |   |       Ring 0: AegisOS Kernel       | |
|  | - Crash-Test App (Triggers #PF/#DE) |   | - IDT Exception & Interrupt Handlers| |
|  | - Activity Monitor (PID / Mem View) |   | - Preemptive Round-Robin Scheduler | |
|  | - Terminal Shell / AegisPad Editor  |   | - Physical Bitmap Frame Allocator  | |
|  | - GUI Window Manager / Compositor   |   | - Kernel Heap (linked_list_alloc)  | |
|  | - Lower-Half PML4 (0x0000... private)|   | - Higher-Half PML4 (0xFFFF... shared)| |
|  +-------------------------------------+   +------------------------------------+ |
|                     |                                         ^                   |
|                     | Fault (#PF, #DE, #GP, #UD)              |                   |
|                     +-----------------------------------------+                   |
|                       CPU pushes InterruptFrame (CS & 3 == 3)                     |
|                       TSS switches to Kernel RSP0 Stack                           |
|                       Log -> Terminate PCB -> Deferred Reaping -> Reschedule Next |
+-----------------------------------------------------------------------------------+
```

---

## 2. Domain 1: Hardware-Enforced Fault Isolation & Exception Handling (R2)

### 2.1 Interrupt Stack Frame Anatomy & Privilege Detection

When an exception occurs in x86_64 Long Mode:
1. **Stack Switching via TSS**: If the processor detects a privilege transition from Ring 3 ($CPL=3$) to Ring 0 ($CPL=0$), it automatically loads the kernel stack pointer from `TSS.RSP0` into `RSP`.
2. **Hardware Push Sequence**: The CPU pushes the following 64-bit values onto the newly established Ring 0 stack:
   - `SS`: Stack Segment of the interrupted context
   - `RSP`: Stack Pointer of the interrupted context
   - `RFLAGS`: CPU Flags register (IF, IOPL, etc.)
   - `CS`: Code Segment of the interrupted context
   - `RIP`: Instruction Pointer where the exception occurred
   - `ErrorCode`: (Pushed only for certain vectors: `#DF`, `#TS`, `#NP`, `#SS`, `#GP`, `#PF`, `#AC`, `#CP`).
3. **Assembly Wrapper Normalization**: For exceptions that do not push an error code (`#DE`, `#UD`, `#DB`, `#BP`, `#OF`, `#BR`, `#NM`, `#MF`), the assembly ISR stub pushes a dummy `0` error code to guarantee that every exception handler receives an identical, predictable stack layout.

```
Low Address
+------------------------------------+  <- RSP after general registers pushed
| R15, R14, R13, R12, R11, R10, R9, R8|
| RDI, RSI, RBP, RDX, RCX, RBX, RAX  |
+------------------------------------+
| Error Code (or 0 dummy)            |  <- RSP after ISR stub entry
+------------------------------------+
| RIP                                |  <- Hardware pushed by CPU
| CS                                 |
| RFLAGS                             |
| RSP                                |
| SS                                 |
+------------------------------------+
High Address (Kernel Stack Top: TSS.RSP0)
```

### 2.2 The Privilege Determination Algorithm

The Code Segment (`CS`) register selector contains the Requested Privilege Level (RPL) in bits 0 and 1:
$$\text{Privilege Level} = \text{CS} \ \& \ 0x03$$

- If `(frame.cs & 0x03) == 0x03` (or `frame.cs != KERNEL_CS`): The fault occurred in **Ring 3 Userspace**.
- If `(frame.cs & 0x03) == 0x00`: The fault occurred in **Ring 0 Kernel**.

```rust
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct InterruptStackFrame {
    pub rip: u64,
    pub cs: u64,
    pub rflags: u64,
    pub rsp: u64,
    pub ss: u64,
}

impl InterruptStackFrame {
    #[inline(always)]
    pub fn is_user_mode(&self) -> bool {
        (self.cs & 0x03) == 0x03
    }
}
```

### 2.3 Exception Vector Breakdown & Specific Diagnostics

#### A. Page Fault (`#PF`, Vector 14)
- **Fault Address**: Loaded by hardware into the `CR2` control register (`x86_64::registers::control::Cr2::read()`).
- **Error Code Interpretation**:
  - `Bit 0 (P)`: `0` = Page Not Present (Null pointer dereference or unmapped virtual page); `1` = Protection Violation (Write to read-only page or User accessing Supervisor page).
  - `Bit 1 (W/R)`: `0` = Read access; `1` = Write access.
  - `Bit 2 (U/S)`: `0` = Kernel access; `1` = User access.
  - `Bit 3 (RSVD)`: `1` = Reserved bit set in page table entry.
  - `Bit 4 (I/D)`: `1` = Instruction fetch (NX / No-Execute violation).
- **User Fault Handling**:
  1. Read `fault_addr = Cr2::read()`.
  2. Format detailed diagnostic log:
     ```
     [FAULT-PF] PID 4 ('crash_demo') Page Fault at RIP 0x00000000004012A0, CR2=0x0000000000000000
                Reason: NULL_POINTER_OR_UNMAPPED (P=0, W=1, U=1, NX=0)
     ```
  3. Dispatch to `task_terminate_and_reschedule(ExitReason::PageFault { cr2: fault_addr, error_code })`.

#### B. Divide-by-Zero (`#DE`, Vector 0)
- **Cause**: An `idiv` or `div` instruction with a zero divisor, or quotient overflow.
- **Hardware Push**: No error code (stub pushes dummy 0).
- **User Fault Handling**:
  1. Log:
     ```
     [FAULT-DE] PID 4 ('crash_demo') Divide by Zero at RIP 0x0000000000401314
     ```
  2. Dispatch to `task_terminate_and_reschedule(ExitReason::DivideByZero)`.

#### C. General Protection Fault (`#GP`, Vector 13)
- **Cause**: Executing privileged instructions in Ring 3 (`cli`, `sti`, `hlt`, `in`, `out`, `mov %cr0, %rax`), accessing non-canonical virtual addresses, segment limit violations, or writing to unauthorized MSRs.
- **Hardware Push**: Pushes Segment Selector error code (or 0).
- **User Fault Handling**:
  1. Log:
     ```
     [FAULT-GP] PID 4 ('crash_demo') General Protection Fault at RIP 0x000000000040138C, Code=0x00000000
     ```
  2. Dispatch to `task_terminate_and_reschedule(ExitReason::GeneralProtection { error_code })`.

#### D. Invalid Opcode (`#UD`, Vector 6)
- **Cause**: CPU encountered unrecognized or illegal instruction bytes (e.g., `ud2` opcode `0x0F 0x0B`, corrupted binary code, or unsupported vector extension).
- **Hardware Push**: No error code (stub pushes dummy 0).
- **User Fault Handling**:
  1. Log:
     ```
     [FAULT-UD] PID 4 ('crash_demo') Invalid Opcode (#UD) at RIP 0x0000000000401420
     ```
  2. Dispatch to `task_terminate_and_reschedule(ExitReason::InvalidOpcode)`.

### 2.4 Non-Panicking Fault Handler Control Flow

```
+--------------------------------------------------------------------+
|                   CPU Triggers Exception Vector                   |
+--------------------------------------------------------------------+
                                  |
                                  v
+--------------------------------------------------------------------+
|  Assembly ISR Stub saves GPRs, normalizes error code, calls Rust  |
+--------------------------------------------------------------------+
                                  |
                                  v
+--------------------------------------------------------------------+
|                 Check Privilege: (frame.cs & 3)                    |
+--------------------------------------------------------------------+
              /                                        \
    == 0 (Kernel Mode)                           == 3 (User Mode)
            /                                            \
           v                                              v
+-----------------------+              +-----------------------------------+
| KERNEL PANIC!         |              | 1. Serial Log & Activity Mon Event|
| - Print register dump |              | 2. Notify GUI (mark window closed)|
| - Print stack trace   |              | 3. Set PCB status = Terminated    |
| - Halts system (hlt)  |              | 4. Transfer to Zombie List        |
+-----------------------+              | 5. Switch to Next Task via Sched  |
                                       +-----------------------------------+
                                                          |
                                                          v
                                       +-----------------------------------+
                                       | CPU resumes running other apps!   |
                                       | Framebuffer & Desktop stay smooth |
                                       +-----------------------------------+
```

---

## 3. Domain 2: Clean Task Termination & Resource Reclamation (R2/R3)

### 3.1 The "Active Execution Context" Trap
A fundamental rule of operating system design is: **A thread cannot free its own stack or its own active page table while executing on them.**
- If a fault handler immediately frees the task's PML4 root while `CR3` still points to it, any subsequent instruction fetch or stack access generates an immediate recursive Page Fault or Double Fault (`#DF`), causing a CPU triple fault and instant machine reset.
- If a fault handler frees the current kernel stack (`RSP0`) while inside the Rust exception handler, the stack frame is invalidated under its feet.

### 3.2 Two-Phase Deferred Reaping Architecture

To guarantee 100% safety and zero memory leaks, AegisOS implements a 2-phase deferred reaping model:

```
[Faulting Ring 3 Task]
        | (Encountered #PF / #DE / #GP / #UD)
        v
+-----------------------------------------------------------------------------+
| Phase 1: Immediate Synchronous Mark & Deschedule (Inside ISR)              |
| 1. Acquire Scheduler Lock.                                                  |
| 2. Set current_task.status = TaskStatus::Terminated(exit_reason).           |
| 3. Remove task from run_queue.                                              |
| 4. Append task to zombie_queue.                                             |
| 5. Notify Desktop Compositor: close/mark destroyed associated window.       |
| 6. Call context_switch_to(next_task) -> switches RSP and CR3 to next task!  |
+-----------------------------------------------------------------------------+
        |
        v (Next ready task or Kernel Idle Task is now executing)
+-----------------------------------------------------------------------------+
| Phase 2: Deferred Asynchronous Reaping (Inside Idle Loop / Sched Epoch)     |
| 1. Executed on a completely separate kernel stack and Kernel PML4.          |
| 2. Pop PCB from zombie_queue.                                               |
| 3. Walk PCB's PML4 lower-half (entries 0..255):                             |
|    - Recursively free all leaf physical frames (Code, Data, Heap, Stack).   |
|    - Free Page Tables (PT), Page Directories (PD), PDPTs.                   |
|    - Free root PML4 physical frame.                                         |
| 4. Free Task Kernel Stack allocation.                                       |
| 5. Drop PCB struct (freeing heap descriptor and returning PID).             |
| 6. Update Activity Monitor: free memory metric decreases, task disappears. |
+-----------------------------------------------------------------------------+
```

### 3.3 Recursive Page Table Deallocator Algorithm

When traversing a process's 4-level paging structure:
- **Shared Higher-Half Protection**: PML4 entries `256..511` point to shared kernel PDPTs. **These must NEVER be traversed or freed by a user task reclaimer!**
- **Private Lower-Half Traversal**: Only PML4 entries `0..255` are inspected.

```rust
/// Safely reclaims all physical memory frames mapped in a user address space.
/// Must be executed when CR3 is NOT set to `user_pml4_phys`.
pub unsafe fn destroy_user_address_space(
    user_pml4_phys: PhysAddr,
    hhdm_offset: u64,
    frame_allocator: &mut BitmapFrameAllocator,
) -> usize {
    let mut frames_freed = 0;
    let pml4_virt = (user_pml4_phys.as_u64() + hhdm_offset) as *mut PageTable;
    let pml4 = &mut *pml4_virt;

    // Only iterate through LOWER HALF (userspace: 0..256)
    for pml4_idx in 0..256 {
        let pml4_entry = &pml4[pml4_idx];
        if !pml4_entry.flags().contains(PageTableFlags::PRESENT) {
            continue;
        }

        let pdpt_phys = pml4_entry.addr();
        let pdpt_virt = (pdpt_phys.as_u64() + hhdm_offset) as *mut PageTable;
        let pdpt = &mut *pdpt_virt;

        for pdpt_idx in 0..512 {
            let pdpt_entry = &pdpt[pdpt_idx];
            if !pdpt_entry.flags().contains(PageTableFlags::PRESENT) {
                continue;
            }
            // If 1GB huge page, free single 1GB frame and continue
            if pdpt_entry.flags().contains(PageTableFlags::HUGE_PAGE) {
                frame_allocator.free_frame(PhysFrame::containing_address(pdpt_entry.addr()));
                frames_freed += 512 * 512;
                continue;
            }

            let pd_phys = pdpt_entry.addr();
            let pd_virt = (pd_phys.as_u64() + hhdm_offset) as *mut PageTable;
            let pd = &mut *pd_virt;

            for pd_idx in 0..512 {
                let pd_entry = &pd[pd_idx];
                if !pd_entry.flags().contains(PageTableFlags::PRESENT) {
                    continue;
                }
                // If 2MB huge page, free single 2MB frame and continue
                if pd_entry.flags().contains(PageTableFlags::HUGE_PAGE) {
                    frame_allocator.free_frame(PhysFrame::containing_address(pd_entry.addr()));
                    frames_freed += 512;
                    continue;
                }

                let pt_phys = pd_entry.addr();
                let pt_virt = (pt_phys.as_u64() + hhdm_offset) as *mut PageTable;
                let pt = &mut *pt_virt;

                for pt_idx in 0..512 {
                    let pt_entry = &pt[pt_idx];
                    if pt_entry.flags().contains(PageTableFlags::PRESENT) {
                        let user_frame = PhysFrame::containing_address(pt_entry.addr());
                        frame_allocator.free_frame(user_frame);
                        frames_freed += 1;
                    }
                }

                // Free the PT frame
                frame_allocator.free_frame(PhysFrame::containing_address(pd_entry.addr()));
                frames_freed += 1;
            }

            // Free the PD frame
            frame_allocator.free_frame(PhysFrame::containing_address(pdpt_entry.addr()));
            frames_freed += 1;
        }

        // Free the PDPT frame
        frame_allocator.free_frame(PhysFrame::containing_address(pml4_entry.addr()));
        frames_freed += 1;
    }

    // Finally, free the root PML4 frame itself
    frame_allocator.free_frame(PhysFrame::containing_address(user_pml4_phys));
    frames_freed += 1;

    frames_freed
}
```

---

## 4. Domain 3: Preemptive Multitasking Scheduler (R3)

### 4.1 Hardware Timer & Interrupt Architecture
Multitasking is driven preemptively by the system timer:
- **Timer Options**:
  - **Programmable Interval Timer (PIT 8254)**: Standard across all PC hardware and QEMU. Operates on frequency $1.193182\text{ MHz}$. Configured via I/O ports `0x43` (Mode Command) and `0x40` (Channel 0 Data). Set to $100\text{ Hz}$ ($10\text{ ms}$ quantum) or $1000\text{ Hz}$ ($1\text{ ms}$ tick).
  - **Local APIC Timer**: High precision, integrated into CPU core. Configured via LAPIC MMIO register `0xFEE00320` (Timer LVT) and `0xFEE00380` (Initial Count).
- **Interrupt Routing**:
  - PIT mapped via 8259 PIC to IDT Vector 32 (`0x20`).
  - Timer ISR sends EOI (`0x20` to master PIC / `0xfee000b0` to APIC), increments tick counter, and invokes `scheduler::on_timer_tick()`.

### 4.2 Process Control Block (PCB) & State Machine

```rust
pub type ProcessId = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskStatus {
    Ready,
    Running,
    Blocked(BlockReason),
    Terminated(ExitReason),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitReason {
    Normal(i32),
    PageFault { cr2: u64, error_code: u64 },
    DivideByZero,
    GeneralProtection { error_code: u64 },
    InvalidOpcode,
    KilledByAdmin,
}

#[repr(C)]
#[derive(Debug, Clone)]
pub struct ProcessControlBlock {
    pub pid: ProcessId,
    pub name: String,
    pub status: TaskStatus,
    pub priority: u8,
    pub time_slice_remaining: u32,
    pub total_cpu_ticks: u64,
    
    // Memory and Page Tables
    pub cr3_phys: PhysAddr,
    pub kernel_stack_bottom: VirtAddr,
    pub kernel_stack_top: VirtAddr,
    pub user_stack_top: VirtAddr,
    pub user_entry_point: VirtAddr,
    pub allocated_frame_count: usize,
    
    // Saved context when descheduled
    pub saved_rsp: u64,
    
    // GUI Window linkage
    pub window_id: Option<u64>,
}
```

```
+---------------+     Dispatched by Scheduler     +---------------+
|     READY     | ------------------------------> |    RUNNING    |
+---------------+                                 +---------------+
        ^                                           |     |     |
        |              Quantum Expired              |     |     |
        +-------------------------------------------+     |     |
        |                                                 |     |
        | Event Triggered (Key/Mouse/IPC)                 |     | Wait on Event
        |                                                 |     v
+---------------+                                 +---------------+
|    BLOCKED    | <------------------------------ |    BLOCKED    |
+---------------+                                 +---------------+
                                                          |
                                                          | Fault / Exit
                                                          v
                                                  +---------------+
                                                  |  TERMINATED   | (Awaiting Reaper)
                                                  +---------------+
```

### 4.3 Low-Level Context Switch Implementation

When switching from `prev_task` to `next_task`:
1. **Save Callee-Saved Registers and Flags**: Push `r15`, `r14`, `r13`, `r12`, `rbp`, `rbx`, and `rflags` onto the current kernel stack.
2. **Save Stack Pointer**: Save `RSP` into `prev_task.saved_rsp`.
3. **Load Next Stack Pointer**: Load `RSP` from `next_task.saved_rsp`.
4. **Update TSS**: Update `TSS.RSP0 = next_task.kernel_stack_top` so any future Ring 3 interrupt on this CPU uses the new task's kernel stack!
5. **Switch Address Space**: If `next_task.cr3_phys != current_cr3`, write `next_task.cr3_phys` to `CR3` (flushing TLB for user pages while keeping global kernel pages cached if `PGE` enabled).
6. **Restore Context**: Pop `rbx`, `rbp`, `r12`, `r13`, `r14`, `r15`, `rflags`, and `ret` (or `iretq` if returning from interrupt).

```nasm
; System Context Switch Routine
; fn switch_context(prev_rsp: *mut u64, next_rsp: u64, next_cr3: u64, next_kstack_top: u64)
global switch_context
switch_context:
    ; RDI = &mut prev_task.saved_rsp
    ; RSI = next_task.saved_rsp
    ; RDX = next_task.cr3_phys
    ; RCX = next_task.kernel_stack_top

    ; 1. Save callee-saved registers of current task
    push rbp
    push rbx
    push r12
    push r13
    push r14
    push r15
    pushfq

    ; 2. Save current RSP to prev_task.saved_rsp
    mov [rdi], rsp

    ; 3. Switch to next task's RSP
    mov rsp, rsi

    ; 4. Update TSS RSP0 for future Ring 3 -> Ring 0 transitions
    ; External TSS variable or call
    mov [tss_rsp0_slot], rcx

    ; 5. Switch Address Space (CR3) if changed
    mov rax, cr3
    cmp rax, rdx
    je .skip_cr3_flush
    mov cr3, rdx
.skip_cr3_flush:

    ; 6. Restore callee-saved registers of next task
    popfq
    pop r15
    pop r14
    pop r13
    pop r12
    pop rbx
    pop rbp

    ret
```

### 4.4 Round-Robin Scheduler & Runqueue Logic

```rust
pub struct Scheduler {
    run_queue: VecDeque<Arc<Mutex<ProcessControlBlock>>>,
    zombie_queue: Vec<Arc<Mutex<ProcessControlBlock>>>,
    current_task: Option<Arc<Mutex<ProcessControlBlock>>>,
    idle_task: Arc<Mutex<ProcessControlBlock>>,
    next_pid: u64,
}

impl Scheduler {
    pub fn schedule(&mut self) {
        // 1. Check if current task should continue or yield
        if let Some(current) = &self.current_task {
            let mut curr_guard = current.lock();
            if curr_guard.status == TaskStatus::Running {
                if curr_guard.time_slice_remaining > 0 {
                    curr_guard.time_slice_remaining -= 1;
                    return; // Continue running current task
                } else {
                    // Time slice exhausted: requeue at back of run_queue
                    curr_guard.status = TaskStatus::Ready;
                    curr_guard.time_slice_remaining = DEFAULT_QUANTUM;
                    drop(curr_guard);
                    self.run_queue.push_back(current.clone());
                }
            } else if matches!(curr_guard.status, TaskStatus::Terminated(_)) {
                // Task faulted or exited: move to zombie queue
                self.zombie_queue.push(current.clone());
            }
        }

        // 2. Pick next ready task from run queue
        let next_task = self.run_queue.pop_front().unwrap_or_else(|| self.idle_task.clone());
        
        {
            let mut next_guard = next_task.lock();
            next_guard.status = TaskStatus::Running;
            next_guard.time_slice_remaining = DEFAULT_QUANTUM;
        }

        let prev_task = self.current_task.replace(next_task.clone());

        // 3. Perform hardware context switch if changing tasks
        if let Some(prev) = prev_task {
            if !Arc::ptr_eq(&prev, &next_task) {
                Self::perform_context_switch(&prev, &next_task);
            }
        }
    }
}
```

### 4.5 The Kernel Idle Task
When no user applications are ready (e.g. all blocked on events or queues empty):
- The `idle_task` runs in Ring 0 executing:
  ```rust
  fn idle_task_loop() -> ! {
      loop {
          // Reclaim zombie tasks while CPU is idle
          scheduler::reap_zombies();
          
          // Put CPU into low-power halt state until next interrupt
          x86_64::instructions::interrupts::enable_and_hlt();
      }
  }
  ```
- This satisfies the energy and performance criteria: CPU does not spin in a burning loop, and background memory reclamation happens with zero perceptible latency to userspace.

---

## 5. Domain 4: Memory Allocators & Per-Process Address Spaces (R3)

### 5.1 Limine Boot Protocol Integration
AegisOS utilizes Limine bootloader protocol requests:
- `LIMINE_MEMMAP_REQUEST`: Returns an array of physical memory regions (`usable`, `reserved`, `acpi`, `bootloader_reclaimable`, `framebuffer`, `kernel_and_modules`).
- `LIMINE_HHDM_REQUEST`: Provides the Higher-Half Direct Map base offset (typically `0xFFFF_8000_0000_0000`). All physical memory $P$ is directly accessible at virtual address `hhdm_offset + P`.

### 5.2 Physical Frame Allocator (Bitmap Design)

To meet the requirement of **< 60 MB RAM at idle** and handle up to **4 GB RAM**:
- 4 GB RAM consists of $4 \times 1024 \times 1024 \times 1024 / 4096 = 1,048,576$ frames of 4 KB each.
- 1 bit represents 1 frame (0 = free, 1 = allocated).
- Total bitmap storage size:
  $$\text{Bitmap Size} = \frac{1,048,576\text{ bits}}{8\text{ bits/byte}} = 131,072\text{ bytes} = 128\text{ KB}$$
- **128 KB** is negligible overhead (< 0.25% of the 60 MB limit), making a Bitmap Allocator the optimal choice over complex buddy or tree systems.

```rust
pub struct BitmapFrameAllocator {
    bitmap: &'static mut [u64],
    total_frames: usize,
    allocated_frames: usize,
    hhdm_offset: u64,
}

impl BitmapFrameAllocator {
    pub fn allocate_frame(&mut self) -> Option<PhysFrame> {
        for (word_idx, word) in self.bitmap.iter_mut().enumerate() {
            if *word != u64::MAX {
                let bit_idx = (!*word).trailing_zeros() as usize;
                *word |= 1 << bit_idx;
                self.allocated_frames += 1;
                let frame_idx = word_idx * 64 + bit_idx;
                let phys_addr = PhysAddr::new((frame_idx as u64) * 4096);
                return Some(PhysFrame::containing_address(phys_addr));
            }
        }
        None // Out of physical memory
    }

    pub fn allocate_zeroed_frame(&mut self) -> Option<PhysFrame> {
        let frame = self.allocate_frame()?;
        let virt_ptr = (frame.start_address().as_u64() + self.hhdm_offset) as *mut u8;
        unsafe {
            core::ptr::write_bytes(virt_ptr, 0, 4096);
        }
        Some(frame)
    }

    pub fn free_frame(&mut self, frame: PhysFrame) {
        let frame_idx = (frame.start_address().as_u64() / 4096) as usize;
        let word_idx = frame_idx / 64;
        let bit_idx = frame_idx % 64;
        if word_idx < self.bitmap.len() {
            if (self.bitmap[word_idx] & (1 << bit_idx)) != 0 {
                self.bitmap[word_idx] &= !(1 << bit_idx);
                self.allocated_frames -= 1;
            }
        }
    }
}
```

### 5.3 Kernel Heap Allocator

- Initialized via `#[global_allocator]` with `linked_list_allocator::LockedHeap` (or intrusive free-list slab allocator).
- Allocated a contiguous virtual range in kernel higher-half (e.g. `0xFFFF_FFFF_8000_0000`, 16 MB initial size backed by frames from the Bitmap Allocator).
- Supports standard Rust collections: `Vec`, `Box`, `String`, `Arc`, `BTreeMap`, `VecDeque`.

### 5.4 4-Level Paging Architecture & Per-Process Address Space Isolation

The 64-bit virtual address space ($256\text{ TB}$) is bifurcated into:
1. **User Lower-Half (`0x0000_0000_0000_0000` .. `0x0000_7FFF_FFFF_FFFF`)**:
   - PML4 entries `0..255`
   - Completely private to each process.
   - User Code / Data mapped at `0x0000_0000_0040_0000` (4MB mark).
   - User Heap mapped at `0x0000_0000_1000_0000` expanding upward.
   - User Stack mapped at `0x0000_7FFF_FFFF_0000` (top of user space) growing downward.
   - **Guard Page**: Virtual page immediately below user stack (`0x0000_7FFF_FFFE_0000`) is left UNMAPPED (`PRESENT=0`). If the user stack overflows, it immediately triggers `#PF` instead of silently corrupting other user data!
2. **Kernel Higher-Half (`0xFFFF_8000_0000_0000` .. `0xFFFF_FFFF_FFFF_FFFF`)**:
   - PML4 entries `256..511`
   - **Shared identically across ALL processes**.
   - Contains HHDM physical direct mapping, Kernel executable text & BSS, GDT/IDT/TSS, Kernel Heap, Framebuffer linear RGB buffer, and APIC/IO-APIC MMIO.

```
0x0000_0000_0000_0000 +------------------------------------------+
                      | NULL Pointer Guard (Unmapped)            |
0x0000_0000_0040_0000 +------------------------------------------+
                      | User Text & Data (ELF Segments) (R/W/X)  |
0x0000_0000_1000_0000 +------------------------------------------+
                      | User Heap (brk / mmap)                   |
                      |                 ...                      |
0x0000_7FFF_FFFE_0000 +------------------------------------------+
                      | User Stack Guard Page (PRESENT=0)        |
0x0000_7FFF_FFFF_0000 +------------------------------------------+
                      | User Stack (grows down) (R/W, NX)        |
0x0000_7FFF_FFFF_FFFF +==========================================+ <-- End of User Space
                      | Non-canonical address hole               |
0xFFFF_8000_0000_0000 +==========================================+ <-- Start of Kernel Space
                      | Limine Higher-Half Direct Map (HHDM)     |
                      +------------------------------------------+
                      | Kernel Heap & Stacks                     |
                      +------------------------------------------+
                      | Framebuffer Linear RGB VRAM              |
                      +------------------------------------------+
                      | Kernel Code, Data, BSS                   |
0xFFFF_FFFF_FFFF_FFFF +------------------------------------------+
```

### 5.5 Process Address Space Instantiation Procedure

```rust
pub fn create_user_address_space(
    master_kernel_pml4_phys: PhysAddr,
    frame_allocator: &mut BitmapFrameAllocator,
    hhdm_offset: u64,
) -> Result<PhysAddr, AllocError> {
    // 1. Allocate a clean zeroed 4KB frame for new PML4
    let new_pml4_frame = frame_allocator
        .allocate_zeroed_frame()
        .ok_or(AllocError)?;
    let new_pml4_phys = new_pml4_frame.start_address();
    
    let master_pml4_virt = (master_kernel_pml4_phys.as_u64() + hhdm_offset) as *const PageTable;
    let new_pml4_virt = (new_pml4_phys.as_u64() + hhdm_offset) as *mut PageTable;

    unsafe {
        let master = &*master_pml4_virt;
        let new_table = &mut *new_pml4_virt;

        // 2. Clone HIGHER-HALF (entries 256..512) for shared kernel mappings
        for i in 256..512 {
            new_table[i] = master[i].clone();
        }

        // Lower-half (0..256) remains clean 0 (all unmapped)
    }

    Ok(new_pml4_phys)
}
```

---

## 6. Detailed Implementation Blueprints & Assembly Stubs

### 6.1 Unified Assembly Exception Stubs

To ensure seamless integration with Rust exception handlers without compiler ABI mismatch:

```nasm
; assembly/interrupts.s
[bits 64]
section .text

extern rust_exception_dispatcher

%macro EXCEPTION_ERR 1
global isr_exception_%1
isr_exception_%1:
    push %1                 ; Push Vector Number
    jmp isr_common_stub
%endmacro

%macro EXCEPTION_NO_ERR 1
global isr_exception_%1
isr_exception_%1:
    push 0                  ; Push Dummy Error Code
    push %1                 ; Push Vector Number
    jmp isr_common_stub
%endmacro

; Exceptions without error code
EXCEPTION_NO_ERR 0   ; #DE Divide by Zero
EXCEPTION_NO_ERR 6   ; #UD Invalid Opcode
; Exceptions with error code
EXCEPTION_ERR    13  ; #GP General Protection Fault
EXCEPTION_ERR    14  ; #PF Page Fault

isr_common_stub:
    ; Stack layout:
    ; [RSP + 0] = Vector Number
    ; [RSP + 8] = Error Code
    ; [RSP + 16] = RIP
    ; [RSP + 24] = CS
    ; [RSP + 32] = RFLAGS
    ; [RSP + 40] = RSP
    ; [RSP + 48] = SS

    ; Save all general-purpose registers
    push rax
    push rbx
    push rcx
    push rdx
    push rsi
    push rdi
    push rbp
    push r8
    push r9
    push r10
    push r11
    push r12
    push r13
    push r14
    push r15

    mov rdi, rsp            ; Pass pointer to full ExceptionContext as 1st argument (RDI)
    call rust_exception_dispatcher

    ; Restore registers
    pop r15
    pop r14
    pop r13
    pop r12
    pop r11
    pop r10
    pop r9
    pop r8
    pop rbp
    pop rdi
    pop rsi
    pop rdx
    pop rcx
    pop rbx
    pop rax

    add rsp, 16             ; Pop Vector Number and Error Code
    iretq
```

### 6.2 Rust Exception Dispatcher

```rust
#[repr(C)]
#[derive(Debug)]
pub struct ExceptionContext {
    pub r15: u64,
    pub r14: u64,
    pub r13: u64,
    pub r12: u64,
    pub r11: u64,
    pub r10: u64,
    pub r9: u64,
    pub r8: u64,
    pub rbp: u64,
    pub rdi: u64,
    pub rsi: u64,
    pub rdx: u64,
    pub rcx: u64,
    pub rbx: u64,
    pub rax: u64,
    pub vector: u64,
    pub error_code: u64,
    pub rip: u64,
    pub cs: u64,
    pub rflags: u64,
    pub rsp: u64,
    pub ss: u64,
}

#[no_mangle]
pub extern "C" fn rust_exception_dispatcher(ctx: &mut ExceptionContext) {
    let is_user = (ctx.cs & 0x03) == 0x03;

    match ctx.vector {
        0 => { // #DE Divide-by-Zero
            if is_user {
                serial_println!("[AegisOS Crash Isolator] User Process PID {} crashed: Divide-by-Zero at RIP {:#018x}", scheduler::current_pid(), ctx.rip);
                scheduler::terminate_current_and_reschedule(ExitReason::DivideByZero);
            } else {
                panic!("KERNEL PANIC: Divide-by-Zero in Ring 0 at RIP {:#018x}", ctx.rip);
            }
        }
        6 => { // #UD Invalid Opcode
            if is_user {
                serial_println!("[AegisOS Crash Isolator] User Process PID {} crashed: Invalid Opcode at RIP {:#018x}", scheduler::current_pid(), ctx.rip);
                scheduler::terminate_current_and_reschedule(ExitReason::InvalidOpcode);
            } else {
                panic!("KERNEL PANIC: Invalid Opcode in Ring 0 at RIP {:#018x}", ctx.rip);
            }
        }
        13 => { // #GP General Protection Fault
            if is_user {
                serial_println!("[AegisOS Crash Isolator] User Process PID {} crashed: General Protection Fault (Code {:#x}) at RIP {:#018x}", scheduler::current_pid(), ctx.error_code, ctx.rip);
                scheduler::terminate_current_and_reschedule(ExitReason::GeneralProtection { error_code: ctx.error_code });
            } else {
                panic!("KERNEL PANIC: General Protection Fault in Ring 0 at RIP {:#018x}, Code {:#x}", ctx.rip, ctx.error_code);
            }
        }
        14 => { // #PF Page Fault
            let cr2 = x86_64::registers::control::Cr2::read().as_u64();
            if is_user {
                serial_println!("[AegisOS Crash Isolator] User Process PID {} crashed: Page Fault at RIP {:#018x}, CR2={:#018x}, ErrorCode={:#b}", scheduler::current_pid(), ctx.rip, cr2, ctx.error_code);
                scheduler::terminate_current_and_reschedule(ExitReason::PageFault { cr2, error_code: ctx.error_code });
            } else {
                panic!("KERNEL PANIC: Page Fault in Ring 0 at RIP {:#018x}, accessing CR2={:#018x}, ErrorCode={:#b}", ctx.rip, cr2, ctx.error_code);
            }
        }
        vec => {
            if is_user {
                serial_println!("[AegisOS Crash Isolator] User Process PID {} terminated on unhandled vector {}", scheduler::current_pid(), vec);
                scheduler::terminate_current_and_reschedule(ExitReason::Normal(-1));
            } else {
                panic!("KERNEL PANIC: Unhandled Exception Vector {} in Ring 0 at RIP {:#018x}", vec, ctx.rip);
            }
        }
    }
}
```

---

## 7. Verification & Acceptance Testing Matrix

| Test Case | Method / Trigger | Expected Observation | Acceptance Criterion Met |
|---|---|---|---|
| **#PF Null Pointer** | Crash-Test App dereferences `*(volatile u32*)0x0 = 0xDEAD` | Serial logs `#PF` CR2=`0x0`, PID reaped, window closes, GUI remains interactive at 60 FPS | AC: Null pointer crash isolation |
| **#PF Out-of-Bounds** | Crash-Test App writes to `*(volatile u32*)0xDEAD_BEEF = 0x42` | Serial logs `#PF` CR2=`0xDEAD_BEEF`, PID reaped, Activity Monitor shows memory freed | AC: Out-of-bounds page fault recovery |
| **#DE Divide-by-Zero**| Crash-Test App executes `div %rcx` where `rcx=0` | Serial logs `#DE` vector 0, offending process reaped, Activity Monitor updates task list | AC: Divide-by-zero crash isolation |
| **#UD Invalid Opcode**| Crash-Test App executes opcode `0x0F 0x0B` (`ud2`) | Serial logs `#UD` vector 6, clean return to scheduler, other apps unaffected | AC: Invalid opcode recovery |
| **#GP Privilege Test** | Userspace app attempts `cli` or `mov cr0, rax` | Serial logs `#GP` vector 13, user app terminated without kernel compromise | AC: Privilege level enforcement |
| **Memory Reclamation** | Spawn multiple tasks, trigger crashes, observe RAM | Activity Monitor memory usage drops back to idle (< 60 MB), no leaked frames in bitmap | AC: System memory < 60 MB |
| **Multitasking Preemption** | Run Terminal shell, Activity Monitor, and Crash-Test concurrently | Smooth mouse tracking, live CPU graph updates, no jitter or thread starvation | AC: Preemptive round-robin scheduler |

---

## 8. Summary of Architectural Recommendations for Implementation Agents

1. **GDT/TSS Integration**: Ensure `TSS.RSP0` points to the top of the currently running task's kernel stack on every context switch.
2. **Vector Normalization**: Use the assembly macros in Section 6.1 so Rust handlers never encounter misaligned stack frames.
3. **Paging Bounds**: Always isolate user allocations to PML4 indices `0..255`. Maintain PML4 indices `256..511` as exact clones of the master kernel table.
4. **Bitmap Simplicity**: Use the 128 KB bitmap allocator directly initialized from the Limine memory map; it is deterministic, crash-proof, and consumes almost zero RAM.
5. **Reaper Scheduling**: Execute physical frame deallocation exclusively during Phase 2 deferred reaping (inside the Idle Task or on scheduler tick boundaries) to preserve stack and TLB integrity.
