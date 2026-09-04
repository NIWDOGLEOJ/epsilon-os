"""In-Kernel Bare-Metal Self-Test Suite Runner & Assertion Validator."""

import os
import subprocess

# Suite labels in execution order, as `src/selftest/mod.rs` prints them. Kept as
# data rather than a wall of assertions so that adding a suite is a one-line
# change here instead of a renumbering of every line.
EXPECTED_SUITES = [
    "Physical Frame Allocator",
    "PML4 Paging & Isolation",
    "Kernel Dynamic Heap",
    "Task Scheduler Lifecycle",
    "In-Memory VFS",
    "PC Speaker Audio",
    "Wallpaper & PPM Parser",
    "Scientific Calculator",
    "Terminal 2.0 Engine",
    "AI Agent, Spotlight & Browser",
    "Minesweeper Retro Arcade",
    "AegisPad 2.0 Advanced Editor",
    "AegisSynth Chiptune Studio",
    "Virtual Network & AegisChat",
    "ELF64 Loader & Syscall Interface",
]


def test_in_kernel_selftests():
    """Builds the kernel with --features selftest and executes it in QEMU with
    isa-debug-exit enabled, asserting every bare-metal suite reports PASS and
    that QEMU exits with status 33 (success code 0x10).
    """
    repo_root = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
    script_path = os.path.join(repo_root, "run_selftest.sh")

    proc = subprocess.run(
        ["bash", script_path],
        cwd=repo_root,
        capture_output=True,
        text=True,
        timeout=180.0,
    )

    output = proc.stdout + proc.stderr

    assert proc.returncode == 0, (
        f"run_selftest.sh returned non-zero exit code: {proc.returncode}\nOutput:\n{output}"
    )

    total = len(EXPECTED_SUITES)
    for index, name in enumerate(EXPECTED_SUITES, start=1):
        expected = f"[SELFTEST:{index}/{total}] [PASS] {name} Suite OK."
        assert expected in output, f"{name} suite did not report PASS (looked for: {expected!r})"

    assert (
        f"[SELFTEST:PASS] All bare-metal in-kernel unit tests passed! ({total}/{total} suites)"
        in output
    ), "Master in-kernel selftest pass banner missing"

    assert "QEMU Process Exit Code: 33" in output, (
        "Expected QEMU isa-debug-exit status code 33"
    )
