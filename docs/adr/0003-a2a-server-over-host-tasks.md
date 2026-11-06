# 0003 — basis-a2a's hosted server crosses as a foreign task host

> Status: Accepted · 2026-11-06

## Context

A Swift service that runs its own tasks needs an A2A server. basis-a2a serves
a host's tasks through `TaskHost` and an operator's `Authenticator` (basis
ADR-0033, ADR-0036, ADR-0048). Without a binding, the service writes a second
A2A server in Swift.

## Decision

1. **`A2aTaskHost` and `A2aAuthenticator` are foreign traits** that map one to
   one onto `TaskHost` and `Authenticator`; records mirror `Part`, `Artifact`,
   `HostedStart`, `HostedStanding` and `HostedReply`. Message metadata crosses
   as JSON text.
2. **Progress is pulled.** `TaskHost::progress` returns a stream, which UniFFI
   cannot carry. The binding asks `progress(id, cursor)` for the next note and
   ends the stream on `nil` or a thrown error, which ends notes, never the task.
3. **The authenticator declares one scheme** as a record, read once, and an
   unreachable authenticator refuses the request as unauthenticated.
4. **`A2aServer.serve` binds and serves on the binding's runtime**, and
   `shutdown` stops it gracefully, leaving unsettled tasks to the next server.

## Consequences

- The XCFramework links axum and basis-a2a's server.
- It requires basis 0.24, which ships ADR-0048.

## Ablation

Dropping the authenticator's attributes and not forwarding a host's artifacts
each failed `aSwiftHostServesItsTasksOverA2a`; restored, it passed.
