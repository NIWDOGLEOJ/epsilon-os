#!/bin/bash
# =============================================================================
# AegisOS ISO Builder Script
# Creates a hybrid BIOS/UEFI bootable ISO using Limine bootloader and xorriso.
# =============================================================================

set -euo pipefail

PROJECT_DIR="$(cd "$(dirname "$0")" && pwd)"
export PATH="$HOME/.cargo/bin:$PATH"

KERNEL_ELF="$PROJECT_DIR/target/x86_64-unknown-none/debug/aegis_os"
ISO_OUTPUT="${ISO_OUTPUT:-$PROJECT_DIR/aegis_os.iso}"
ISO_ROOT="/tmp/aegis_os_iso_root"

LIMINE_DIR="/tmp/limine"
LIMINE_DEPLOY="$LIMINE_DIR/limine"

echo "=== AegisOS ISO Build Pipeline ==="

CARGO_FEATURES="${CARGO_FEATURES:-${1:-}}"
FEATURES_FLAG=""
if [ -n "$CARGO_FEATURES" ]; then
    if [[ "$CARGO_FEATURES" == --features* ]]; then
        FEATURES_FLAG="$CARGO_FEATURES"
    else
        FEATURES_FLAG="--features $CARGO_FEATURES"
    fi
fi

# 1. Build the kernel
echo "[1/4] Compiling AegisOS kernel... ${FEATURES_FLAG}"
cargo build --manifest-path="$PROJECT_DIR/Cargo.toml" $FEATURES_FLAG 2>&1

if [ ! -f "$KERNEL_ELF" ]; then
    echo "[FATAL] Kernel ELF not found at $KERNEL_ELF"
    exit 1
fi

echo "[OK] Kernel ELF: $(file "$KERNEL_ELF" | cut -d: -f2-)"

# 2. Fetch Limine bootloader (if not already cached)
if [ ! -f "$LIMINE_DEPLOY" ]; then
    echo "[2/4] Downloading Limine bootloader..."
    rm -rf "$LIMINE_DIR"
    git clone --depth 1 https://github.com/limine-bootloader/limine.git --branch=v8.x-binary "$LIMINE_DIR"
fi

# Build limine-deploy if needed
if [ ! -x "$LIMINE_DEPLOY" ]; then
    echo "[2/4] Building limine deploy tool..."
    make -C "$LIMINE_DIR" 2>/dev/null || cc -o "$LIMINE_DEPLOY" "$LIMINE_DIR/limine.c" 2>/dev/null || true
fi

echo "[OK] Limine bootloader ready."

# 3. Create ISO root directory structure
echo "[3/4] Assembling ISO filesystem..."
rm -rf "$ISO_ROOT"
mkdir -p "$ISO_ROOT/boot/limine"
mkdir -p "$ISO_ROOT/EFI/BOOT"

# Copy kernel binary
cp "$KERNEL_ELF" "$ISO_ROOT/boot/aegis_kernel"

# Copy Limine config (try both formats)
if [ -f "$PROJECT_DIR/limine.conf" ]; then
    cp "$PROJECT_DIR/limine.conf" "$ISO_ROOT/boot/limine/limine.conf"
fi
if [ -f "$PROJECT_DIR/limine.cfg" ]; then
    cp "$PROJECT_DIR/limine.cfg" "$ISO_ROOT/boot/limine/limine.cfg"
fi

# Copy Limine bootloader binaries
cp "$LIMINE_DIR/limine-bios.sys"    "$ISO_ROOT/boot/limine/" 2>/dev/null || true
cp "$LIMINE_DIR/limine-bios-cd.bin" "$ISO_ROOT/boot/limine/" 2>/dev/null || true
cp "$LIMINE_DIR/limine-uefi-cd.bin" "$ISO_ROOT/boot/limine/" 2>/dev/null || true

# UEFI boot files
cp "$LIMINE_DIR/BOOT"*.EFI "$ISO_ROOT/EFI/BOOT/" 2>/dev/null || true
cp "$LIMINE_DIR/BOOTX64.EFI" "$ISO_ROOT/EFI/BOOT/" 2>/dev/null || true
cp "$LIMINE_DIR/BOOTIA32.EFI" "$ISO_ROOT/EFI/BOOT/" 2>/dev/null || true

# 4. Build ISO with xorriso
echo "[4/4] Packaging hybrid BIOS/UEFI ISO with xorriso..."
xorriso -as mkisofs \
    -b boot/limine/limine-bios-cd.bin \
    -no-emul-boot \
    -boot-load-size 4 \
    -boot-info-table \
    --efi-boot boot/limine/limine-uefi-cd.bin \
    -efi-boot-part --efi-boot-image --protective-msdos-label \
    "$ISO_ROOT" -o "$ISO_OUTPUT" 2>&1

# Install Limine BIOS boot record
if [ -x "$LIMINE_DEPLOY" ]; then
    "$LIMINE_DEPLOY" bios-install "$ISO_OUTPUT" 2>/dev/null || true
fi

echo ""
echo "=== BUILD COMPLETE ==="
echo "ISO Image: $ISO_OUTPUT ($(du -h "$ISO_OUTPUT" | cut -f1))"
echo ""
echo "To boot in QEMU:  ./run_qemu.sh"
echo "To flash to USB:   sudo dd if=$ISO_OUTPUT of=/dev/sdX bs=4M status=progress"
