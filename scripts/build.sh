#!/usr/bin/env bash
# Build script for sql-lsp native binary.
# For local dev, just build for the current platform.
# CI uses the GitHub Actions matrix for cross-compilation.

set -euo pipefail

CRATE="sql-lsp"
OUT_DIR="dist"

mkdir -p "$OUT_DIR"

echo "Building $CRATE for current platform..."
cargo build --release -p "$CRATE"

# Copy binary to dist/ with platform suffix
if [[ "$OSTYPE" == "msys"* || "$OSTYPE" == "cygwin"* || "$OS" == "Windows_NT" ]]; then
    PLATFORM="x86_64-pc-windows-msvc"
    EXT=".exe"
else
    ARCH=$(uname -m)
    OS_NAME=$(uname -s | tr '[:upper:]' '[:lower:]')
    case "$OS_NAME" in
        darwin) OS_LABEL="apple-darwin" ;;
        linux)  OS_LABEL="unknown-linux-gnu" ;;
        *)      echo "Unknown OS: $OS_NAME"; exit 1 ;;
    esac
    PLATFORM="${ARCH}-${OS_LABEL}"
    EXT=""
fi

SRC="target/release/${CRATE}${EXT}"
DST="$OUT_DIR/${CRATE}-${PLATFORM}"

cp "$SRC" "$DST"
echo "Output: $DST"
