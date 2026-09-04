"""Visual Compositor and Framebuffer Verification Test Suite for AegisOS."""

import time
from .harness import QemuHarness


def test_framebuffer_rendering(qemu: QemuHarness):
    """Verifies that the software compositor successfully renders the desktop environment
    with proper resolution, non-flat color palette, top menu bar, and dock.
    """
    # Wait for compositor active
    qemu.wait_for_serial(r"AegisOS macOS Desktop Compositor Active", timeout=12.0)
    time.sleep(1.0)  # Allow a few frames to render and swap

    img = qemu.screendump()

    # 1. Assert Dimensions
    assert img.width == 1280, f"Expected width 1280, got {img.width}"
    assert img.height == 800, f"Expected height 800, got {img.height}"

    # 2. Assert Not Flat Color (catches boot hang where screen was single flat color)
    assert not img.is_flat_color(threshold=10), (
        "Framebuffer is a single flat solid color! Compositor failed to render."
    )

    # 3. Assert Rich Palette Diversity (> 50 unique colors sampled across screen)
    unique_colors = img.unique_colors(step=8)
    assert len(unique_colors) >= 50, (
        f"Expected rich desktop palette (>=50 colors), got only {len(unique_colors)}"
    )

    # 4. Assert Color Variance (proves multiple UI elements and gradients exist)
    variance = img.color_variance(step=8)
    assert variance > 100.0, f"Luminance variance too low: {variance:.2f}"

    # 5. Top Menu Bar Inspection (y = 0..23)
    # The menu bar has a dark gradient background with a 1px border at y=23
    menubar_pixel = img.get_pixel(640, 10)
    # Menubar background is dark slate RGB (~25..45 each channel)
    assert menubar_pixel[0] < 60 and menubar_pixel[1] < 60 and menubar_pixel[2] < 60, (
        f"Unexpected menu bar pixel color: {menubar_pixel}"
    )

    # 6. Shield Logo Icon in Menu Bar (x=8, y=4..18)
    # Yellow shield icon drawn with Color::YELLOW (R=255, G=215, B=0)
    found_shield_yellow = False
    for y in range(4, 20):
        for x in range(6, 26):
            r, g, b = img.get_pixel(x, y)
            if r > 200 and g > 170 and b < 50:
                found_shield_yellow = True
                break
        if found_shield_yellow:
            break
    assert found_shield_yellow, "Shield logo yellow icon not found in top menu bar"
