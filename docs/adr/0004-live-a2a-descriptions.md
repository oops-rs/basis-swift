# ADR-0004: Live A2A descriptions mirror Basis

Status: accepted

## Context

A hosted agent can learn skills while its server is running. Basis ADR-0049 adds
`A2aService::update_identity`; restarting an HTTP listener is unnecessary.

## Decision

`A2aServer.updateIdentity` mirrors that synchronous host operation using the
already-bound name, description, version and skill shapes. The server retains its
service only until shutdown. After shutdown, mutation fails and the state directory
can be reopened even while the old Swift server object remains alive.

## Consequences

Swift hosts authorize and persist their metadata. No A2A mutation method, policy,
credentials or endpoint override is added. The real Swift HTTP fixture exercises
live metadata, stable authentication/capabilities, task admission, one-hour deadline
forwarding, shutdown and reopening. Generated bindings are regenerated with Rust.

## Verification

The final artifact was built against published basis-a2a 0.24.1 for macOS (arm64,
x86_64), iOS and both simulator architectures. Seven Swift tests and strict Rust
Clippy pass. The upstream identity-replacement ablation and Tiamat's cross-FFI
publication ablation both detect a missing update; restored baselines pass.
