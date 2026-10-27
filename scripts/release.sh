#!/usr/bin/env bash
# Prepares a release: builds every slice, zips the XCFramework, and points
# Package.swift's binary target at the zip's release URL and checksum.
#
#   scripts/release.sh 0.1.0
#
# It does not tag, push, or upload. The printed steps do that, so what gets
# published is reviewed first.
set -euo pipefail

VERSION="${1:?usage: scripts/release.sh <version>}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

crate_version=$(sed -n 's/^version = "\(.*\)"/\1/p' rust/Cargo.toml | head -1)
if [[ "$crate_version" != "$VERSION" ]]; then
    echo "rust/Cargo.toml is at $crate_version; bump it to $VERSION first" >&2
    exit 1
fi

PROFILE=release scripts/build-xcframework.sh

ZIP="build/BasisFFI.xcframework.zip"
ditto -c -k --sequesterRsrc --keepParent build/BasisFFI.xcframework "$ZIP"
CHECKSUM=$(swift package compute-checksum "$ZIP")
URL="https://github.com/oops-rs/basis-swift/releases/download/$VERSION/BasisFFI.xcframework.zip"

sed -i '' \
    -e "s|^let ffiURL = \".*\"|let ffiURL = \"$URL\"|" \
    -e "s|^let ffiChecksum = \".*\"|let ffiChecksum = \"$CHECKSUM\"|" \
    Package.swift

cat <<NEXT
Package.swift now points at
  $URL
  checksum $CHECKSUM

Next:
  git add Package.swift Sources/Basis/Generated rust/Cargo.toml rust/Cargo.lock
  git commit -m "chore(release): prepare basis-swift $VERSION"
  git tag $VERSION && git push origin main $VERSION
  gh release create $VERSION $ZIP --title $VERSION
NEXT
