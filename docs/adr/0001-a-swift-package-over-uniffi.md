# 0001 — basis reaches Swift as a package built over UniFFI

> Status: Accepted · 2026-10-27

## Context

basis's in-process surface is a Rust crate, so only Rust can embed it. Its
other two surfaces, ACP and `basis spawn --json`, need a `basis` binary to run
as a subprocess. That is a cost on macOS and impossible on iOS, where an app
spawns no processes. A Swift app also wants its approver and its event view to
be its own code, running while the turn waits.

Two ways to reach Swift in process:

- **A hand-written C API with a Swift wrapper.** C has no async, errors,
  lifetimes or typed callbacks, so the wrapper rebuilds all of them by hand
  and keeps them in step with the C side.
- **UniFFI.** It generates the wrapper from Rust definitions. `async fn`
  becomes Swift `async`, `Result` becomes `throws`, `Arc` objects become
  classes, and a trait the host implements becomes a Swift protocol.

## Decision

1. **This repository is a Swift package, and Swift is its only language.**
   Its product is `Basis`, added through SwiftPM. The Rust crate under
   `rust/` is how the package is built, not something it offers. Another
   language would get a repository of its own.

2. **The binding sits over the published `basis`, outside it.** basis gains
   no UniFFI derive and no FFI type, and it does not know this package
   exists. The crate mirrors what basis exposes in records of its own.

3. **Small, stable seams cross as types, and the event stream crosses as its
   wire line.** Options, the approval request and answer, and the run report
   are records and enums that a Swift host switches on. An event crosses as
   one JSON `EventLine`, exactly what `basis spawn --json` writes, and Swift
   wraps it as `Event`. basis's `Event` is large and grows, and it already has
   a versioned schema (`EVENT_SCHEMA_VERSION`). Mirroring it would create a
   second definition of that schema.

4. **The binding owns its tokio runtime.** Swift brings no executor, so one
   multi-thread runtime lives for the process. Every exported `async fn` runs
   its work there, and a Swift `await` only waits.

5. **A Swift seam fails closed.** An approver that throws denies, and an
   observer that throws ends the run, the same rules basis applies to a
   broken hook and a refused sink write.

6. **The first cut is the run lifecycle:** open a workspace with options,
   prepare or resume a run, send a turn with an observer, an approver and a
   stop control, and read the report. Interceptors, native tools, typed
   output and a shared runtime for many workspaces come when an app needs
   them. Each is one more foreign trait or record over a seam basis already
   has.

7. **Generated code is committed, and the binary ships as a release asset.**
   `scripts/build-xcframework.sh` builds the static libraries, regenerates
   `Sources/BasisBindings/BasisBindings.swift` and assembles
   `BasisFFI.xcframework`. A developer adding the package never runs it, so
   the Swift it generates is committed. The XCFramework is too large for git;
   `scripts/release.sh` zips it for a GitHub release and points
   `Package.swift`'s binary target at that zip and its checksum.

8. **The package is Swift 6.4; only the generated bindings are not in Swift 6
   mode.** UniFFI 0.32.2's template for async foreign traits hands
   non-`@Sendable` closures to a `Task`, which Swift 6's data-race checks
   reject. So the generated file is its own target, `BasisBindings`, built in
   Swift 5 mode. `Basis` re-exports it (`@_exported import`) and holds the
   hand-written Swift (`Event`, closure conveniences), which is checked in
   Swift 6 mode like the tests. A developer imports `Basis` and never names
   the split. When a UniFFI release generates Swift 6-clean code, the
   language-mode setting goes. The two targets can stay, since they keep
   generated and hand-written code apart.

## Consequences

- A Swift app embeds basis in process on macOS and iOS. Tools that spawn
  (`shell`, declared tools, stdio MCP, subprocess hooks) still fail where the
  OS refuses to spawn. That is the platform's limit, not the binding's.
- A change that bumps `EVENT_SCHEMA_VERSION` breaks Swift readers the same
  way it breaks JSONL readers, and the version tells them so.
- `uniffi` is pinned to an exact version: the committed Swift and the
  compiled scaffolding must come from the same UniFFI.
- Swift 6 binds an unlabeled trailing closure to the first closure
  parameter. The closure `send` therefore lists `onEvent` before `approve`,
  so `send(prompt) { event in … } approve: { … }` reads as written.
- Working in this repository needs the Rust toolchain and the Apple targets;
  using the package needs neither.
- `uniffi` is MPL-2.0. Its file-level copyleft does not reach this package's
  MIT sources.
