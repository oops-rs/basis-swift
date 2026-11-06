# Reasoning-summary binding — 2026-11-07

The published basis API already supports independent reasoning effort and summary
settings. This binding previously created ReasoningOptions from effort alone and
always set summary to None. ConversationOptions now mirrors the optional summary
(auto, concise, detailed), including the summary-only case. Saved conversation
options return the persisted selection. No provider behavior is added here.
Omission retains the old wire behavior; setEffort retains basis's documented
compatibility semantics, including clearing a configured summary.

Design ablation: effort alone cannot request a provider-generated summary;
adding a provider-specific extra JSON field at a consumer would bypass the
existing generic API. The independent optional field is the smallest complete
mapping and leaves existing callers unchanged.

Verification:
- Rust test covers every summary selection, absent/present effort, and saved
  round trips while retaining transport, storage and replay settings.
- All 7 Swift package tests passed against the newly generated local FFI,
  including reading summary and effort back through the real binding.
- Rust formatting, tests and all-target Clippy passed.
- Release XCFramework built macOS arm64/x86_64, iOS arm64, and simulator
  arm64/x86_64 using scripts/release.sh 0.2.2. Generated bindings are committed
  with the Rust change; Package.swift contains the release ZIP checksum.
- Two isolated implementation ablations were detected by the Rust behavioral
  test: dropping the requested summary and dropping the saved summary. Each
  compiled, failed its assertion, and was restored. The restored test passed.
