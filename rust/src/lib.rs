//! UniFFI bindings for [basis](https://github.com/oops-rs/basis): the Rust half
//! of the Swift package.
//!
//! The binding mirrors the run lifecycle — open a workspace, mint or resume a
//! run, send turns — in types of its own, so `basis` carries no FFI concern.
//! Small seams cross as records; the event stream crosses as the JSONL line
//! `basis spawn --json` writes (ADR-0001).

use std::future::Future;
use std::sync::LazyLock;

mod approval;
mod error;
mod events;
mod options;
mod run;
mod workspace;

pub use approval::{ApprovalAnswer, ApprovalDecision, ApprovalRequest, Approver, SideEffectLevel};
pub use error::BasisError;
pub use events::EventObserver;
pub use options::{History, Provider, WorkspaceOptions};
pub use run::{Bound, Run, RunOutcome, RunReport, TurnControl, Usage};
pub use workspace::Workspace;

uniffi::setup_scaffolding!();

/// The one executor every exported call runs on.
///
/// A foreign caller brings no tokio runtime, and basis spawns tasks that need
/// one, so the binding owns it for the life of the process. A Swift `await`
/// polls the join handle, which needs no runtime of its own.
static RUNTIME: LazyLock<tokio::runtime::Runtime> = LazyLock::new(|| {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("basis-ffi")
        .build()
        .expect("the basis-ffi tokio runtime must start")
});

/// Runs `future` on [`RUNTIME`] and waits for it from whatever executor the
/// foreign side polls with.
async fn on_runtime<F, T>(future: F) -> Result<T, BasisError>
where
    F: Future<Output = Result<T, BasisError>> + Send + 'static,
    T: Send + 'static,
{
    RUNTIME
        .spawn(future)
        .await
        .map_err(|join| BasisError::Internal {
            message: join.to_string(),
        })?
}
