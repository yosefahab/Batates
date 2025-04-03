#!/bin/bash

set -euo pipefail

command -v cargo >/dev/null 2>&1 || { echo "Cargo not found. Install Rust."; exit 1; }
command -v jq >/dev/null 2>&1 || { echo "jq not found. Install it."; exit 1; }
command -v cargo-bundle >/dev/null 2>&1 || { echo "cargo-bundle not found. Install cargo-bundle."; exit 1; }
command -v lipo >/dev/null 2>&1 || { echo "lipo not found. Install Xcode Command Line Tools."; exit 1; }

PROJECT_NAME=$(cargo metadata --no-deps --format-version 1 | jq -r '.packages[0].name')
VERSION=$(cargo metadata --no-deps --format-version 1 | jq -r '.packages[0].version')
RELEASE_DIR="$(pwd)/releases"
WINDOWS_TARGET="x86_64-pc-windows-gnu"
MACOS_TARGETS=("x86_64-apple-darwin" "aarch64-apple-darwin" "aarch64-apple-darwin")
MACOS_APP_NAME="${PROJECT_NAME}.app"
BUILD_CMD="cargo build --release --target"

build_windows_release() {
  echo "Building for Windows ($WINDOWS_TARGET)..."
  $BUILD_CMD "$WINDOWS_TARGET"
  WINDOWS_BIN_PATH="target/$WINDOWS_TARGET/release/$PROJECT_NAME.exe"
  if [[ ! -f "$WINDOWS_BIN_PATH" ]]; then
    echo "Error: Windows executable not found at $WINDOWS_BIN_PATH"
    return 1
  fi
  WINDOWS_RELEASE_FOLDER="$RELEASE_DIR/${PROJECT_NAME}-${VERSION}-${WINDOWS_TARGET}"
  mkdir -p "$WINDOWS_RELEASE_FOLDER"
  cp "$WINDOWS_BIN_PATH" "$WINDOWS_RELEASE_FOLDER/"
  WINDOWS_ARCHIVE_NAME="${PROJECT_NAME}-${VERSION}-${WINDOWS_TARGET}.zip"
  pushd "$RELEASE_DIR" > /dev/null
  zip -r "$WINDOWS_ARCHIVE_NAME" "$(basename "$WINDOWS_RELEASE_FOLDER")"
  popd > /dev/null
  rm -rf "$WINDOWS_RELEASE_FOLDER"
  echo "Created: $RELEASE_DIR/$WINDOWS_ARCHIVE_NAME"
  return 0
}

build_macos_with_bundle() {
  local TARGET="aarch64-apple-darwin"
  echo "Building and bundling for macOS ($TARGET) using cargo bundle..."
  cargo bundle --release --target "$TARGET"
  local BUNDLED_APP_PATH="target/$TARGET/release/bundle/osx/${PROJECT_NAME}.app"
  if [[ -d "$BUNDLED_APP_PATH" ]]; then
    local MACOS_ARCHIVE_NAME="${PROJECT_NAME}-${VERSION}-${TARGET}-bundle.zip"
    pushd "target/$TARGET/release/bundle/osx" > /dev/null
    zip -r "$RELEASE_DIR/$MACOS_ARCHIVE_NAME" "${PROJECT_NAME}.app"
    popd > /dev/null
    echo "Created: $RELEASE_DIR/$MACOS_ARCHIVE_NAME"
    return 0
  else
    echo "Error: macOS application bundle not found at '$BUNDLED_APP_PATH'"
    return 1
  fi
}

build_macos_custom() {
  echo "Building and packaging for macOS (custom implementation)..."
  local MACOS_BUILD_DIR="target/macos_build"
  mkdir -p "$MACOS_BUILD_DIR"

  for TARGET in "${MACOS_TARGETS[@]}"; do
    echo "Building for $TARGET..."
    $BUILD_CMD "$TARGET"
    cp "target/$TARGET/release/$PROJECT_NAME" "$MACOS_BUILD_DIR/$PROJECT_NAME-$TARGET"
  done

  echo "Creating universal binary..."
  lipo -create "$MACOS_BUILD_DIR/$PROJECT_NAME-x86_64-apple-darwin" "$MACOS_BUILD_DIR/$PROJECT_NAME-aarch64-apple-darwin" -output "$MACOS_BUILD_DIR/$PROJECT_NAME"

  echo "Creating macOS application bundle '$MACOS_APP_NAME'..."
  mkdir -p "$RELEASE_DIR/$MACOS_APP_NAME/Contents/MacOS"
  mkdir -p "$RELEASE_DIR/$MACOS_APP_NAME/Contents/Resources"

  if [[ -f "Info.plist" ]]; then
    cp "Info.plist" "$RELEASE_DIR/$MACOS_APP_NAME/Contents/Info.plist"
  else
    echo "Warning: Info.plist not found. macOS bundle might be incomplete."
  fi

  if [[ -f "AppIcon.icns" ]]; then
    cp "AppIcon.icns" "$RELEASE_DIR/$MACOS_APP_NAME/Contents/Resources/AppIcon.icns"
  else
    echo "Warning: AppIcon.icns not found. macOS bundle will be missing an icon."
  fi

  if [[ -d "assets" ]]; then
    cp -r "assets" "$RELEASE_DIR/$MACOS_APP_NAME/Contents/Resources/"
  fi

  mv "$MACOS_BUILD_DIR/$PROJECT_NAME" "$RELEASE_DIR/$MACOS_APP_NAME/Contents/MacOS/"

  local MACOS_ARCHIVE_NAME="${PROJECT_NAME}-${VERSION}-macos-custom.zip"
  pushd "$RELEASE_DIR" > /dev/null
  zip -r "$MACOS_ARCHIVE_NAME" "$MACOS_APP_NAME"
  popd > /dev/null
  rm -rf "$RELEASE_DIR/$MACOS_APP_NAME"
  rm -rf "$MACOS_BUILD_DIR"
  echo "Created: $RELEASE_DIR/$MACOS_ARCHIVE_NAME"
  return 0
}

echo "Starting build process..."

# build_windows_release

echo ""
echo "Testing macOS build with 'cargo bundle' (aarch64 only)..."
build_macos_with_bundle

# echo ""
# echo "Testing macOS build with custom implementation (universal binary)..."
# build_macos_custom

echo ""
echo "All requested builds completed!"
