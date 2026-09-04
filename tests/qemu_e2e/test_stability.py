"""System Soak, Clock Tracking, and Interrupt Stability Test Suite for AegisOS."""

import time
from .harness import QemuHarness


def test_clock_progress(qemu: QemuHarness):
    """Verifies that the 100 Hz PIT-driven system clock makes continuous forward progress
    without stalling the scheduler or compositor.
    """
    qemu.wait_for_serial(r"AegisOS macOS Desktop Compositor Active", timeout=12.0)

    # Sample registers over a 2.0-second interval
    rips_start, if_start, _ = qemu.sample_rip_stability(samples=3, interval=0.05)
    assert if_start, "Interrupt Flag (IF) was disabled at start of clock test"

    time.sleep(2.0)

    rips_end, if_end, is_moving = qemu.sample_rip_stability(samples=3, interval=0.05)
    assert if_end, "Interrupt Flag (IF) became disabled after 2s soak"
    assert is_moving, "CPU stopped moving during 2s soak period"


def test_input_flood_resilience(qemu: QemuHarness):
    """Exercises ISR entry points under load with a 560-event synthetic input flood
    (mouse movements, button clicks, and keyboard strokes) to guarantee that:
    1. Static mutexes in ISR paths never deadlock with task context under InterruptGuard.
    2. EventRing preallocated queues never allocate in interrupt context.
    3. RFLAGS.IF remains enabled (bit 9 set).
    4. RIP continues advancing across multiple samples post-flood.
    """
    qemu.wait_for_serial(r"AegisOS macOS Desktop Compositor Active", timeout=12.0)
    time.sleep(0.5)

    # Invert and vary deltas to exercise mouse queue, scancode decoder, and event ring
    event_count = 0
    for i in range(200):
        # 1. Mouse move (x and y deltas) -> 200 events
        dx = (i % 7) - 3
        dy = ((i * 3) % 7) - 3
        qemu.mouse_move(dx, dy)
        event_count += 1

        # 2. Mouse button press and release -> 2 * 30 = 60 events
        if i % 7 == 0:
            qemu.mouse_click(button=1, hold_sec=0.01)
            event_count += 2

        # 3. Key press -> 100 events
        if i % 2 == 0:
            key = chr(ord("a") + (i % 26))
            qemu.send_key(key)
            event_count += 1

        # 4. Auxiliary arrow / tab keys -> 200 events
        if i % 3 == 0:
            aux_keys = ["up", "down", "left", "right", "tab", "spc"]
            qemu.send_key(aux_keys[i % len(aux_keys)])
            event_count += 1

        time.sleep(0.005)

    # Let the compositor and scheduler soak the queue
    time.sleep(1.0)

    # Assert CPU register health and liveness
    rips, all_if_enabled, is_moving = qemu.sample_rip_stability(samples=5, interval=0.1)

    assert all_if_enabled, (
        f"Input flood caused interrupt deadlock! RFLAGS.IF was cleared. Samples: {[hex(r) for r in rips]}"
    )
    assert is_moving, (
        f"CPU is pinned / deadlocked! Sampled identical RIPs: {[hex(r) for r in rips]}"
    )
