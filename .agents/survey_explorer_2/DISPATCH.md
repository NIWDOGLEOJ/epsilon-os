## 2026-08-30T12:05:47Z

You are the Fault Isolation & Scheduler Explorer for AegisOS.
Your working directory is /home/godjoel/teamwork_projects/aegis_os/.agents/survey_explorer_2.
Project root is /home/godjoel/teamwork_projects/aegis_os.
You MUST read /home/godjoel/teamwork_projects/aegis_os/.agents/ORIGINAL_REQUEST.md.

Your mission:
Investigate and design the architectural requirements for:
1. Fault isolation & crash resilience (R2): Ring 3 exception handling for Page Fault (#PF vector 14, CR2, error code check), Divide-by-Zero (#DE vector 0), General Protection Fault (#GP vector 13), Invalid Opcode (#UD vector 6). How to distinguish Ring 0 vs Ring 3 faults via CS selector / interrupt stack frame.
2. Clean task termination: How a faulted task is safely reaped, its physical memory frames freed from its page tables, open resources cleared, and control transferred to the scheduler without panicking the kernel.
3. Preemptive multitasking scheduler (R3): Timer interrupt (PIT or APIC timer), task context state save/restore (RIP, RSP, RBP, GPRs, RFLAGS, CR3), round-robin runqueue, process table with PID, status (Running, Ready, Blocked, Terminated).
4. Memory allocators (R3): Physical frame allocator (bitmap or bump/freelist from Limine memory map), kernel heap (`linked_list_allocator` or slab), per-process virtual address space creation (deep copy or higher-half mapping sharing kernel PML4 entries, private user PML4 lower half).
5. Write your detailed technical findings and recommendations to /home/godjoel/teamwork_projects/aegis_os/.agents/survey_explorer_2/analysis.md and complete handoff.md. Send a message to parent when done.
