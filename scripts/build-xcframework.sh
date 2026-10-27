#!/usr/bin/env bash
# Builds BasisFFI.xcframework from the Rust crate and regenerates the Swift
# sources the package compiles (ADR-0001 §7).
#
#   scripts/build-xcframework.sh                  # release, macOS + iOS + simulator
#   PLATFORMS=macos scripts/build-xcframework.sh  # one slice, for local tests
#   PROFILE=debug scripts/build-xcframework.sh
#
# Outputs: build/BasisFFI.xcframework (ignored) and
# Sources/Basis/Generated/Basis.swift (committed, because a developer adding
# the package through SwiftPM never runs this script).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
RUST="$ROOT/rust"
BUILD="$ROOT/build"
GENERATED="$ROOT/Sources/Basis/Generated"
LIB=libbasis_swift.a

PROFILE="${PROFILE:-release}"
PLATFORMS="${PLATFORMS:-macos ios ios-simulator}"
if [[ "$PROFILE" == release ]]; then
    CARGO_PROFILE=(--release)
else
    CARGO_PROFILE=()
fi

targets_for() {
    case "$1" in
        macos) echo "aarch64-apple-darwin x86_64-apple-darwin" ;;
        ios) echo "aarch64-apple-ios" ;;
        ios-simulator) echo "aarch64-apple-ios-sim x86_64-apple-ios" ;;
        *) echo "unknown platform: $1" >&2; exit 1 ;;
    esac
}

# The package's `platforms` floors, which the C and assembly that ring
# compiles must not exceed — left alone, cc builds them for the SDK's own
# version and every app link warns. Not MACOSX_DEPLOYMENT_TARGET: cargo would
# apply that to the host-built proc-macro dylibs too, and macOS's loader then
# refuses them. rustc's own floors already sit below these.
export CFLAGS_aarch64_apple_darwin="-mmacosx-version-min=13.0"
export CFLAGS_x86_64_apple_darwin="-mmacosx-version-min=13.0"
export IPHONEOS_DEPLOYMENT_TARGET=16.0
cd "$RUST"
for platform in $PLATFORMS; do
    for target in $(targets_for "$platform"); do
        echo "==> cargo build --target $target"
        cargo build --lib "${CARGO_PROFILE[@]}" --target "$target"
    done
done

rm -rf "$BUILD"
mkdir -p "$BUILD/headers" "$GENERATED"

# Bindings are read out of a compiled library's metadata; every slice holds
# the same, so the first one built serves. uniffi.toml beside Cargo.toml names
# the modules.
first_target=$(targets_for "${PLATFORMS%% *}" | cut -d' ' -f1)
echo "==> uniffi-bindgen generate"
cargo run --quiet --bin uniffi-bindgen -- generate \
    --library "target/$first_target/$PROFILE/$LIB" \
    --language swift \
    --out-dir "$BUILD/swift"

mv "$BUILD/swift/Basis.swift" "$GENERATED/Basis.swift"
mv "$BUILD/swift/BasisFFI.h" "$BUILD/headers/"
# An xcframework's headers directory carries one module map under this name,
# which is how SwiftPM finds the C module the generated Swift imports.
mv "$BUILD/swift/BasisFFI.modulemap" "$BUILD/headers/module.modulemap"

xcframework_args=()
for platform in $PLATFORMS; do
    mkdir -p "$BUILD/$platform"
    slices=()
    for target in $(targets_for "$platform"); do
        slices+=("target/$target/$PROFILE/$LIB")
    done
    lipo -create "${slices[@]}" -output "$BUILD/$platform/$LIB"
    # Debug info is most of the archive (146 MB → 31 MB for iOS) and none of
    # what an app links; symbols an app's crash reports need stay.
    strip -S "$BUILD/$platform/$LIB" 2> >(grep -v "already stripped" >&2)
    xcframework_args+=(-library "$BUILD/$platform/$LIB" -headers "$BUILD/headers")
done

echo "==> xcodebuild -create-xcframework"
xcodebuild -create-xcframework "${xcframework_args[@]}" \
    -output "$BUILD/BasisFFI.xcframework" >/dev/null

echo "built $BUILD/BasisFFI.xcframework"
