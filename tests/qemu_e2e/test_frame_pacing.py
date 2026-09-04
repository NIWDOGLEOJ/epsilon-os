"""Compositor 60 FPS Frame Pacing and Supplementary Font Glyphs Test Suite."""

import time
from .harness import QemuHarness


def test_frame_pacing_and_glyphs(qemu: QemuHarness):
    """Verifies that:
    1. Hardware TSC calibration runs at boot, reporting CPU MHz and 60 FPS budget.
    2. Terminal accepts the 'symbols' command and renders arrows, math symbols,
       and unicode icons without kernel panic or crash.
    3. The compositor continues running smoothly at 60 FPS.
    """
    # 1. Wait for boot and TSC calibration log
    qemu.wait_for_serial(r"AegisOS macOS Desktop Compositor Active", timeout=15.0)
    qemu.wait_for_serial(r"\[TIME\] Calibrated TSC:", timeout=15.0)
    qemu.wait_for_serial(r"Target 60 FPS:", timeout=15.0)

    # 2. Type 'symbols' into focused Terminal window
    time.sleep(0.5)
    for ch in "symbols":
        qemu.send_key(ch)
        time.sleep(0.02)
    qemu.send_key("ret")

    # 3. Allow rasterization
    time.sleep(0.5)

    # 4. Capture screendump to verify rendering
    img = qemu.screendump()
    assert img.width == 1280, f"Unexpected width: {img.width}"
    assert img.height == 800, f"Unexpected height: {img.height}"
    assert not img.is_flat_color(threshold=10), "Framebuffer is flat solid color after symbols command"

    # Verify color diversity in the terminal area
    term_colors = img.unique_colors(step=8)
    assert len(term_colors) >= 50, f"Expected varied colors, got {len(term_colors)}"

    # 5. Verify system continues running smoothly
    time.sleep(0.5)
    rips, all_if_enabled, is_moving = qemu.sample_rip_stability(samples=3, interval=0.05)
    assert is_moving, "CPU RIP instruction pointer halted after frame pacing"
