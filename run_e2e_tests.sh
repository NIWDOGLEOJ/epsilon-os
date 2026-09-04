#!/usr/bin/env bash
# =============================================================================
# AegisOS (Epsilon OS) Bare-Metal QEMU E2E Test Suite Runner
# Builds kernel/ISO if necessary and runs the automated test harness.
# =============================================================================

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ISO_PATH="$SCRIPT_DIR/aegis_os.iso"

# 1. Check if ISO exists or if sources are newer
BUILD_NEEDED=0
if [ ! -f "$ISO_PATH" ]; then
    BUILD_NEEDED=1
else
    # Check if any source file is newer than ISO
    if [ -n "$(find "$SCRIPT_DIR/src" "$SCRIPT_DIR/Cargo.toml" "$SCRIPT_DIR/linker.ld" -newer "$ISO_PATH" 2>/dev/null | head -n 1)" ]; then
        BUILD_NEEDED=1
    fi
fi

if [ "$BUILD_NEEDED" -eq 1 ]; then
    echo "=== Building fresh AegisOS ISO image ==="
    bash "$SCRIPT_DIR/build_iso.sh"
fi

# 2. Run the E2E Python Test Runner
export PYTHONPATH="$SCRIPT_DIR:${PYTHONPATH:-}"
python3 -m tests.qemu_e2e.runner "$@"
