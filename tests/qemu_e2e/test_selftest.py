"""In-Kernel Bare-Metal Self-Test Suite Runner & Assertion Validator."""

import os
import subprocess


def test_in_kernel_selftests():
    """Builds the kernel with --features selftest and executes in QEMU with
    isa-debug-exit enabled, asserting that all 4 bare-metal unit test suites pass
    and QEMU exits with status code 33 (0x10 success).
    """
    repo_root = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
    script_path = os.path.join(repo_root, "run_selftest.sh")

    proc = subprocess.run(
        ["bash", script_path],
        cwd=repo_root,
        capture_output=True,
        text=True,
        timeout=45.0,
    )

    output = proc.stdout + proc.stderr

    assert proc.returncode == 0, (
        f"run_selftest.sh returned non-zero exit code: {proc.returncode}\nOutput:\n{output}"
    )

    # Validate individual suite pass logs
    assert "[SELFTEST:1/14] [PASS] Physical Frame Allocator Suite OK." in output, (
        "Physical Frame Allocator suite did not report PASS"
    )
    assert "[SELFTEST:2/14] [PASS] PML4 Paging & Isolation Suite OK." in output, (
        "PML4 Paging & Isolation suite did not report PASS"
    )
    assert "[SELFTEST:3/14] [PASS] Kernel Dynamic Heap Suite OK." in output, (
        "Kernel Dynamic Heap suite did not report PASS"
    )
    assert "[SELFTEST:4/14] [PASS] Task Scheduler Lifecycle Suite OK." in output, (
        "Task Scheduler Lifecycle suite did not report PASS"
    )
    assert "[SELFTEST:5/14] [PASS] In-Memory VFS Suite OK." in output, (
        "In-Memory VFS suite did not report PASS"
    )
    assert "[SELFTEST:6/14] [PASS] PC Speaker Audio Suite OK." in output, (
        "PC Speaker Audio suite did not report PASS"
    )
    assert "[SELFTEST:7/14] [PASS] Wallpaper & PPM Parser Suite OK." in output, (
        "Wallpaper & PPM Parser suite did not report PASS"
    )
    assert "[SELFTEST:8/14] [PASS] Scientific Calculator Suite OK." in output, (
        "Scientific Calculator suite did not report PASS"
    )
    assert "[SELFTEST:9/14] [PASS] Terminal 2.0 Engine Suite OK." in output, (
        "Terminal 2.0 Engine suite did not report PASS"
    )
    assert "[SELFTEST:10/14] [PASS] AI Agent, Spotlight & Browser Suite OK." in output, (
        "AI Agent, Spotlight & Browser suite did not report PASS"
    )
    assert "[SELFTEST:11/14] [PASS] Minesweeper Retro Arcade Suite OK." in output, (
        "Minesweeper Retro Arcade suite did not report PASS"
    )
    assert "[SELFTEST:12/14] [PASS] AegisPad 2.0 Advanced Editor Suite OK." in output, (
        "AegisPad 2.0 Advanced Editor suite did not report PASS"
    )
    assert "[SELFTEST:13/14] [PASS] AegisSynth Chiptune Studio Suite OK." in output, (
        "AegisSynth Chiptune Studio suite did not report PASS"
    )
    assert "[SELFTEST:14/14] [PASS] Virtual Network & AegisChat Suite OK." in output, (
        "Virtual Network & AegisChat suite did not report PASS"
    )
    assert "[SELFTEST:PASS] All bare-metal in-kernel unit tests passed! (14/14 suites)" in output, (
        "Master in-kernel selftest pass banner missing"
    )
    assert "QEMU Process Exit Code: 33" in output, (
        "Expected QEMU isa-debug-exit status code 33"
    )
