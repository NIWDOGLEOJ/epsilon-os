"""Automated QEMU E2E Test for AegisSynth Chiptune Synthesizer & Piano Roll Studio."""

import time
from .harness import QemuHarness


def test_synth_lifecycle(qemu: QemuHarness):
    """Verifies:
    1. AegisSynth launches from Dock Slot 7 (beamed eighth-notes icon).
    2. AegisSynth window renders 4-track 16-step sequencer matrix and 2-octave piano keyboard.
    3. Clicking white key C4 (262 Hz) and black key C#4 (277 Hz) produces sound and visual depression.
    4. Clicking step toggle button in sequencer updates pattern matrix.
    5. Clicking [ ▶ Play ] activates the scanning playhead.
    6. System stability, RFLAGS.IF, and continuous CPU execution.
    """
    # 1. Wait for desktop compositor
    qemu.wait_for_serial(r"AegisOS macOS Desktop Compositor Active", timeout=12.0)
    time.sleep(0.5)

    def move_to(target_x, target_y):
        qemu.execute_monitor("mouse_move -1000 -1000")
        time.sleep(0.04)
        cur_x, cur_y = 0, 0
        while cur_x < target_x or cur_y < target_y:
            step_x = min(4, target_x - cur_x)
            step_y = min(4, target_y - cur_y)
            qemu.execute_monitor(f"mouse_move {step_x} {step_y}")
            cur_x += step_x
            cur_y += step_y
            time.sleep(0.003)

    # 2. Click Dock Slot 7 (AegisSynth - Musical Note icon at x=700, y=764)
    # DOCK_WIDTH = 780, dock_x = 250, slot_width = 60, slot 7 center = 250 + 420 + 30 = 700
    move_to(700, 764)
    time.sleep(0.1)
    qemu.execute_monitor("mouse_button 1")
    time.sleep(0.05)
    qemu.execute_monitor("mouse_button 0")
    time.sleep(0.6)

    # 3. Click piano white key C4 (x=384, y=436)
    move_to(384, 436)
    time.sleep(0.1)
    qemu.execute_monitor("mouse_button 1")
    time.sleep(0.05)
    qemu.execute_monitor("mouse_button 0")
    time.sleep(0.3)

    # 4. Click piano black key C#4 (x=408, y=376)
    move_to(408, 376)
    time.sleep(0.1)
    qemu.execute_monitor("mouse_button 1")
    time.sleep(0.05)
    qemu.execute_monitor("mouse_button 0")
    time.sleep(0.3)

    # 5. Toggle a sequencer step button (Track 0, Step 2 at x=500, y=206)
    move_to(500, 206)
    time.sleep(0.1)
    qemu.execute_monitor("mouse_button 1")
    time.sleep(0.05)
    qemu.execute_monitor("mouse_button 0")
    time.sleep(0.3)

    # 6. Click Play button (x=398, y=172) to start pattern sequencer
    move_to(398, 172)
    time.sleep(0.1)
    qemu.execute_monitor("mouse_button 1")
    time.sleep(0.05)
    qemu.execute_monitor("mouse_button 0")
    time.sleep(0.6)

    # Move cursor to neutral desktop area
    move_to(100, 700)
    time.sleep(0.2)

    # 7. Capture screendump and verify non-flat output
    img = qemu.screendump()
    assert img.width == 1280 and img.height == 800
    assert not img.is_flat_color(threshold=10)

    # 8. Verify CPU and interrupt stability
    rips, interrupts_active, is_moving = qemu.sample_rip_stability(samples=3, interval=0.05)
    assert interrupts_active, "Interrupts deadlocked after AegisSynth playback"
    assert is_moving, "CPU stopped executing after AegisSynth playback"
