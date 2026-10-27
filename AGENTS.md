# AGENTS

basis-ffi follows the conventions of [basis](https://github.com/oops-rs/basis) and
[mentra](https://github.com/oops-rs/mentra); when in doubt, basis's AGENTS.md is the reference.

## Project Rules

- This crate binds basis to other languages through UniFFI (ADR-0001). It mirrors basis's
  surface; it never adds behavior. A gap in what basis exposes is fixed in basis, with an issue
  filed there, not worked around here.
- basis knows nothing about this crate. No UniFFI derive or FFI type goes into basis.
- Events cross as the JSONL `EventLine` (`basis spawn --json`'s wire), never as a mirrored enum.
- A foreign seam fails closed: a throwing approver denies, a throwing observer ends the run.
- Decisions go in `docs/adr/` (numbered, Context/Decision/Consequences).

## Workflow Discipline

- Commit each completed step before starting the next step.

## Commit Style

- Conventional Commits: `<type>(<scope>): <summary>`, types `feat`, `fix`, `docs`, `refactor`,
  `chore`, `test`; narrow scopes; imperative summaries.

## Rust Programming

- Edition 2024, MSRV 1.95. Prefer `foo.rs` plus `foo/` over `foo/mod.rs`.
- Run `cargo fmt` after Rust edits; `cargo clippy --all-targets -- -D warnings` must pass.
- `uniffi` stays pinned to an exact version (`=x.y.z`): bindgen and scaffolding must match.
