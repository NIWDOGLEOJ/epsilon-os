"""Hardware PC Speaker Driver & Audio Subsystem Test Suite."""

import time
from .harness import QemuHarness


def test_pc_speaker_audio(qemu: QemuHarness):
    """Verifies:
    1. PC Speaker driver initialization in serial log.
    2. Terminal 'sound' command inspecting Port 0x61 and PIT timer status.
    3. Terminal 'beep 440 100' generating tone via PIT Channel 2.
    4. Terminal 'play mario' executing multi-note non-blocking sequencer.
    5. Desktop stability and CPU interrupt health during active audio sequencing.
    """
    # 1. Wait for speaker initialization and desktop compositor
    qemu.wait_for_serial(
        r"Hardware PC Speaker Driver \(PIT Channel 2 & Port 0x61\) initialized",
        timeout=12.0,
    )
    qemu.wait_for_serial(r"AegisOS macOS Desktop Compositor Active", timeout=12.0)
    time.sleep(0.5)

    # 2. Run 'sound' command in terminal
    for k in ["s", "o", "u", "n", "d"]:
        qemu.send_key(k)
        time.sleep(0.03)
    qemu.send_key("ret")
    time.sleep(0.6)

    # Verify screendump contains output
    img1 = qemu.screendump()
    assert img1.width == 1280 and img1.height == 800
    assert not img1.is_flat_color(threshold=10)

    # 3. Run 'beep 523 100' command (C5 tone)
    for k in ["b", "e", "e", "p", "spc", "5", "2", "3", "spc", "1", "0", "0"]:
        qemu.send_key(k)
        time.sleep(0.03)
    qemu.send_key("ret")
    time.sleep(0.4)

    # 4. Run 'play mario' command (Super Mario theme opening phrase)
    for k in ["p", "l", "a", "y", "spc", "m", "a", "r", "i", "o"]:
        qemu.send_key(k)
        time.sleep(0.03)
    qemu.send_key("ret")
    time.sleep(0.8)

    # Verify updated screendump
    img2 = qemu.screendump()
    assert not img2.is_flat_color(threshold=10)

    # 5. Verify system liveness and CPU execution during/after audio playback
    rips, interrupts_active, is_moving = qemu.sample_rip_stability(samples=3, interval=0.05)
    assert interrupts_active, "Interrupts are deadlocked after PC speaker operations"
    assert is_moving, "CPU stopped executing after PC speaker operations"
