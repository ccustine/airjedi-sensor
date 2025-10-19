#!/bin/bash
# Native cross-compilation build script for Raspberry Pi 5 (ARM64)
# This script builds AirJedi for aarch64-unknown-linux-gnu target using native macOS toolchain

set -e  # Exit on error

echo "======================================"
echo "AirJedi Cross-Compilation for RPi 5"
echo "======================================"
echo ""

# Colors for output
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m' # No Color

# Check if the aarch64 cross-compiler is installed
if ! command -v aarch64-unknown-linux-gnu-gcc &> /dev/null; then
    echo -e "${RED}Error: aarch64-unknown-linux-gnu-gcc not found${NC}"
    echo ""
    echo "Please install the ARM64 cross-compiler toolchain:"
    echo "  brew tap messense/macos-cross-toolchains"
    echo "  brew install aarch64-unknown-linux-gnu"
    echo ""
    exit 1
fi

# Ensure ARM64 target is installed
echo -e "${GREEN}Ensuring ARM64 target is installed...${NC}"
rustup target add aarch64-unknown-linux-gnu

# Build the project using native toolchain
echo -e "${GREEN}Building AirJedi for ARM64 (Raspberry Pi 5)...${NC}"
echo -e "${GREEN}Using native cross-compiler: $(which aarch64-unknown-linux-gnu-gcc)${NC}"
echo ""
cargo build --release --target aarch64-unknown-linux-gnu

# Verify the binary
if [ -f "target/aarch64-unknown-linux-gnu/release/airjedi" ]; then
    echo ""
    echo -e "${GREEN}✓ Build successful!${NC}"
    echo ""
    echo "Binary Details:"
    ls -lh target/aarch64-unknown-linux-gnu/release/airjedi
    file target/aarch64-unknown-linux-gnu/release/airjedi
    echo ""
    echo "======================================"
    echo "Next Steps:"
    echo "======================================"
    echo "1. Transfer to Raspberry Pi:"
    echo "   scp target/aarch64-unknown-linux-gnu/release/airjedi pi@raspberrypi.local:/home/pi/"
    echo ""
    echo "2. Transfer supporting files:"
    echo "   scp config.toml pi@raspberrypi.local:/home/pi/"
    echo "   scp -r dist/ pi@raspberrypi.local:/home/pi/"
    echo ""
    echo "3. See DEPLOY_RPI5.md for complete deployment instructions"
    echo ""
else
    echo -e "${YELLOW}✗ Build failed - binary not found${NC}"
    exit 1
fi
