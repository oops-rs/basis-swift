//! UniFFI bindings for [basis](https://github.com/oops-rs/basis): the Rust half
//! of the Swift package.
//!
//! The binding mirrors the run lifecycle — open a workspace, mint or resume a
//! run, send turns — in types of its own, so `basis` carries no FFI concern.
//! Small seams cross as records; the event stream crosses as the JSONL line
//! `basis spawn --json` writes (ADR-0001).

use std::future::Future;
use std::sync::LazyLock;

mod a2a;
mod approval;
mod error;
mod events;
mod options;
mod run;
mod tools;
mod workspace;

pub use a2a::{
    A2aArtifact, A2aAuthenticator, A2aHeader, A2aHostError, A2aHostedReply, A2aHostedStanding,
    A2aHostedStart, A2aPart, A2aPartContent, A2aPrincipal, A2aProgressNote, A2aRefusal,
    A2aRequestHead, A2aSecurity, A2aSecurityScheme, A2aServeOptions, A2aServer, A2aSkill,
    A2aTaskHost,
};
pub use approval::{ApprovalAnswer, ApprovalDecision, ApprovalRequest, Approver, SideEffectLevel};
pub use error::BasisError;
pub use events::EventObserver;
pub use options::{History, Provider, Wire, WorkspaceOptions};
pub use run::{
    Bound, ConversationOptions, Effort, ResponsesTransport, Run, RunOutcome, RunReport,
    TurnControl, TurnLimits, Usage,
};
pub use tools::{HostTool, HostToolCall, HostToolDescriptor};
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
