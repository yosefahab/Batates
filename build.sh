#!/bin/bash

set -euo pipefail

command -v cargo >/dev/null 2>&1 || { echo "Cargo not found. Install Rust."; exit 1; }
command -v jq >/dev/null 2>&1 || { echo "jq not found. Install it."; exit 1; }
command -v cargo-bundle >/dev/null 2>&1 || { echo "cargo-bundle not found. Install cargo-bundle."; exit 1; }

PROJECT_NAME=$(cargo metadata --no-deps --format-version 1 | jq -r '.packages[0].name')
VERSION=$(cargo metadata --no-deps --format-version 1 | jq -r '.packages[0].version')
RELEASE_DIR="releases"
TARGETS=(
    # "x86_64-unknown-linux-gnu"
    # "aarch64-unknown-linux-gnu"  
    # "x86_64-apple-darwin"
    # "aarch64-apple-darwin"  
    "x86_64-pc-windows-gnu"
)
ASSETS=("README.md" "LICENSE")

BUILD_CMD="cargo build --release --target"
BUNDLE_CMD="cargo bundle --release --target"

mkdir -p "$RELEASE_DIR"

for TARGET in "${TARGETS[@]}"; do
    echo "Building for $TARGET..."

    # Run cargo build
    $BUILD_CMD "$TARGET"

    # Check if bundling is required (e.g., Windows or macOS)
    if [[ "$TARGET" == *"apple"* || "$TARGET" == *"pc-windows"* ]]; then
        echo "Bundling for $TARGET..."
        $BUNDLE_CMD "$TARGET"
    fi

    case "$TARGET" in
        *-pc-windows-*)
            EXT="zip"
            BIN_PATH="target/$TARGET/release/$PROJECT_NAME.exe"
            ;;
        *)
            EXT="tar.gz"
            BIN_PATH="target/$TARGET/release/$PROJECT_NAME"
            ;;
    esac

    if [[ ! -f "$BIN_PATH" ]]; then
        echo "Error: Binary not found at $BIN_PATH"
        exit 1
    fi

    # Create a directory for this release
    RELEASE_FOLDER="$RELEASE_DIR/${PROJECT_NAME}-${VERSION}-${TARGET}"
    mkdir -p "$RELEASE_FOLDER"

    # Copy the binary
    cp "$BIN_PATH" "$RELEASE_FOLDER/"

    # Copy assets (if they exist)
    for ASSET in "${ASSETS[@]}"; do
        if [[ -e "$ASSET" ]]; then
            cp -r "$ASSET" "$RELEASE_FOLDER/"
        fi
    done

    # Package it
    ARCHIVE_NAME="${PROJECT_NAME}-${VERSION}-${TARGET}.${EXT}"
    echo "Packaging: $ARCHIVE_NAME"

    pushd "$RELEASE_DIR" > /dev/null
    if [[ "$EXT" == "zip" ]]; then
        zip -r "$ARCHIVE_NAME" "$(basename "$RELEASE_FOLDER")"
    else
        tar -czvf "$ARCHIVE_NAME" "$(basename "$RELEASE_FOLDER")"
    fi
    popd > /dev/null

    # Cleanup extracted files (keep only the archive)
    rm -rf "$RELEASE_FOLDER"

    echo "Created: $RELEASE_DIR/$ARCHIVE_NAME"
done

echo "All releases built successfully!"
