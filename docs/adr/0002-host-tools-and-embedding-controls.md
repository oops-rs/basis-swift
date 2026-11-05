# 0002 — Host tools and the embedding controls cross as mirrors of basis

> Status: Accepted · 2026-11-06

## Context

An embedding host such as a service (rather than a chat app) needs more of basis than
0.1.0 exposed: a tool it implements itself, an exact tool roster, no discovery of
ambient project context, an explicit wire and request fields, a model it names
without a provider round trip, per-turn budgets, and the conversation's provider
options read back after a resume. Without them a host had to embed basis through
its own Rust crate and C ABI.

basis 0.23 already has each of these: `RuntimeBuilder::with_tool`,
`WorkspaceBuilder::with_tool_roster`, `without_discovery`, `with_resolved_model`,
`with_wire`, `with_request_fields`, `TurnOptions` budgets, and the session's saved
`ProviderRequestOptions`.

## Decision

1. **Each control is a field or argument that maps one-to-one onto a basis call.**
   `WorkspaceOptions` gains the workspace and runtime knobs, `Workspace.prepare`
   takes `ConversationOptions`, `Run.send` takes `TurnLimits`, and `Run` reads back
   `conversationOptions()` and sets `setEffort`. Unset means basis's default.
2. **A host tool is a foreign `HostTool` trait** describing itself with a JSON
   Schema and side-effect level. It is registered with `with_tool`, so basis's
   refusal to shadow a builtin stands. `replacesBuiltin` is the explicit opt-in:
   the binding unregisters that builtin from the opened workspace's runtime and
   registers the host tool before any run can be minted. Opening fails if no
   builtin of that name exists.
3. **A waiting host tool honours its turn.** Turn cancellation or the turn's
   deadline ends the wait with an error result. mentra's cancel token is a flag, so
   the binding polls it. Dropping the call cancels the Swift task running it, and
   nothing is replayed.
4. **WebSocket over TLS.** The binding enables basis's `responses-websocket` and
   adds `tokio-tungstenite`'s rustls connector, which a `wss://` endpoint needs.

## Consequences

- A host gets basis's own semantics with no second implementation in Rust.
  Accounting a provider does not report, redaction and identity stay host policy.
- With no base URL or key set, basis still reads its environment variables
  (`BASIS_GATEWAYS`, `BASIS_BASE_URL`, `OPENAI_BASE_URL`, provider keys). A host
  that must not inherit them either sets both explicitly or checks its
  environment; removing the lookup belongs in basis.
- `Package.swift` points at the local XCFramework until `scripts/release.sh`
  publishes 0.2.0.
