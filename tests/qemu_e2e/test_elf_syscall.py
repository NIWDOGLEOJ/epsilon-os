"""ELF64 Loader & SYSCALL/SYSRET Interface Test Suite.

Covers the boundary that the in-kernel self-tests cannot reach: a program parsed
from an ELF image, running in Ring 3, calling into the kernel and coming back.
Everything asserted here is read from the real serial console of a booted ISO.
"""

from .harness import QemuHarness


def test_elf_load_syscall_and_isolation(qemu: QemuHarness):
    """Verifies:
    1. SYSCALL/SYSRET is enabled at boot with both EFER.SCE and EFER.NXE.
    2. Two ELF64 images are parsed and loaded into private address spaces.
    3. A loaded program reaches the kernel via `syscall` (SYS_WRITE reaches serial).
    4. A loaded program terminates cleanly through SYS_EXIT.
    5. A *loaded ELF program* that faults is isolated and reaped, not merely the
       hand-assembled payloads that predate the loader.
    6. The kernel never panics through any of it.
    """
    # 1. Syscall MSRs programmed during boot.
    qemu.wait_for_serial(r"\[SYSCALL\] SYSCALL/SYSRET enabled \(EFER\.SCE \+ EFER\.NXE\)", timeout=15.0)

    # 2. Both images accepted by the loader.
    qemu.wait_for_serial(r"\[ELF\] Loaded 'elf_hello' as PID \d+", timeout=15.0)
    qemu.wait_for_serial(r"\[ELF\] Loaded 'elf_crasher' as PID \d+", timeout=15.0)

    # 3. SYS_WRITE from Ring 3 landed on the console. This is the load-bearing
    #    assertion: the string only exists inside the loaded image, so seeing it
    #    means user code executed and successfully entered the kernel.
    qemu.wait_for_serial(r"\[USERSPACE\] Hello from a loaded ELF64 program in Ring 3", timeout=20.0)

    # 4. SYS_EXIT was honoured and the process reported a clean status.
    qemu.wait_for_serial(r"\[SYSCALL\] Process PID \d+ \('elf_hello'\) exited with code 0", timeout=20.0)

    # 5. The crasher spoke first, proving it was a live process rather than an
    #    image that faulted on its first instruction, and was then contained.
    qemu.wait_for_serial(r"\[USERSPACE\] ELF program about to fault deliberately", timeout=20.0)
    qemu.wait_for_serial(
        r"\[FAULT-ISOLATION\] Process PID \d+ \('elf_crasher'\) crashed due to Page Fault", timeout=20.0
    )

    # 6. Desktop still composites, kernel never panicked.
    qemu.wait_for_serial(r"AegisOS macOS Desktop Compositor Active", timeout=20.0)
    log = qemu.get_serial_log()
    assert "KERNEL PANIC" not in log, "Kernel panicked during ELF/syscall execution"
    assert "KERNEL EXCEPTION PANIC" not in log, "Kernel took a Ring 0 exception"

    # 7. CPU still advancing with interrupts enabled.
    _rips, _all_if, is_moving = qemu.sample_rip_stability(samples=3, interval=0.05)
    assert is_moving, "CPU stopped executing after ELF/syscall activity"
