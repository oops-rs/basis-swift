// swift-tools-version: 5.9
import PackageDescription

// The Rust half ships as a prebuilt XCFramework. A release points the binary
// target at its zip; `scripts/release.sh` rewrites these two lines. Unset, or
// with BASIS_SWIFT_LOCAL_FFI set, the package builds against the one
// `scripts/build-xcframework.sh` left in build/.
let ffiURL = ""
let ffiChecksum = ""

let useLocalFFI = ffiURL.isEmpty || Context.environment["BASIS_SWIFT_LOCAL_FFI"] != nil
let ffi: Target = useLocalFFI
    ? .binaryTarget(name: "BasisFFI", path: "build/BasisFFI.xcframework")
    : .binaryTarget(name: "BasisFFI", url: ffiURL, checksum: ffiChecksum)

let package = Package(
    name: "Basis",
    platforms: [.macOS(.v13), .iOS(.v16)],
    products: [
        .library(name: "Basis", targets: ["Basis"]),
    ],
    targets: [
        ffi,
        .target(
            name: "Basis",
            dependencies: ["BasisFFI"],
            path: "Sources/Basis",
            linkerSettings: [
                // What the static library's TLS and proxy lookups link against.
                .linkedFramework("Security"),
                .linkedFramework("SystemConfiguration"),
                .linkedFramework("CoreFoundation"),
            ]
        ),
        .testTarget(name: "BasisTests", dependencies: ["Basis"]),
    ]
)
