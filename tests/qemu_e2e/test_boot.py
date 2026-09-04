"""Boot and Subsystem Initialization Test Suite for AegisOS."""

import re
from .harness import QemuHarness


def test_boot_sequence(qemu: QemuHarness):
    """Verifies that Limine bootloader, kernel CPU privilege, memory subsystem,
    and compositor successfully initialize in proper sequence.
    """
    # 1. Limine Protocol Verification
    line = qemu.wait_for_serial(r"\[OK\] Limine Bootloader Protocol Base Revision verified\.", timeout=12.0)
    assert line, "Limine protocol verification line missing"

    # 2. GDT and TSS Privilege Architecture (Ring 0 & Ring 3 selectors)
    gdt_line = qemu.wait_for_serial(
        r"\[OK\] GDT & TSS loaded\. KCS=0x08, KDS=0x10, UCS=0x23, UDS=0x1b, TSS=0x28"
    )
    assert gdt_line, "GDT/TSS selector initialization failed"

    # 3. IDT and 8259 PIC IRQ Remapping
    idt_line = qemu.wait_for_serial(
        r"\[OK\] IDT & 8259 PIC initialized \(IRQs remapped to 32\.\.47\)\."
    )
    assert idt_line, "IDT/PIC initialization failed"

    # 4. HHDM & Paging / Frame Allocator
    qemu.wait_for_serial(r"\[BOOT\] HHDM Direct Map Offset: 0xffff800000000000")
    qemu.wait_for_serial(r"\[OK\] Physical Frame Allocator \(128KB Bitmap\) & 4-Level Paging initialized\.")
    qemu.wait_for_serial(r"\[OK\] Kernel Heap \(16MB @ 0xFFFF_9000_0000_0000\) initialized\.")

    # 5. Memory footprint within < 60MB target
    mem_line = qemu.wait_for_serial(r"\[BOOT\] Usable Memory Footprint: (\d+) MB used / (\d+) MB total RAM \(< 60MB target verified\)")
    match = re.search(r"(\d+) MB used", mem_line)
    assert match, "Failed to parse memory footprint"
    used_mb = int(match.group(1))
    assert used_mb < 60, f"Memory footprint exceeded 60MB limit: {used_mb}MB"

    # 6. Linear Framebuffer & Hardware Drivers
    qemu.wait_for_serial(r"\[BOOT\] Initializing Framebuffer: 1280x800 \(Pitch: 5120 bytes, 32 BPP\)")
    qemu.wait_for_serial(r"\[OK\] Graphics & Input Drivers \(Framebuffer, PS/2 Mouse, Keyboard\) initialized\.")

    # 7. Preemptive Scheduler & Core System Tasks
    qemu.wait_for_serial(r"\[OK\] Task Scheduler & Ring 3 Fault Isolation Engine active\.")
    qemu.wait_for_serial(r"\[OK\] Spawned System Tasks: Monitor\(PID 1\), Terminal\(PID 2\), Pad\(PID 3\), CrashTest\(PID 4\)")
    qemu.wait_for_serial(r"\[OK\] CPU Hardware Interrupts enabled \(100Hz Preemptive Multitasking Active\)\.")

    # 8. Main Compositor Active
    active_line = qemu.wait_for_serial(r"AegisOS macOS Desktop Compositor Active", timeout=5.0)
    assert active_line, "Desktop Compositor did not become active"
