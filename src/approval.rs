use std::sync::Arc;

use basis::ToolSideEffectLevel;

use crate::BasisError;

/// How far outside the process a call reaches. Absent means unknown, and
/// unknown is never harmless: judge it as [`SideEffectLevel::External`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum SideEffectLevel {
    None,
    LocalState,
    Process,
    External,
}

impl From<ToolSideEffectLevel> for SideEffectLevel {
    fn from(level: ToolSideEffectLevel) -> Self {
        match level {
            ToolSideEffectLevel::None => Self::None,
            ToolSideEffectLevel::LocalState => Self::LocalState,
            ToolSideEffectLevel::Process => Self::Process,
            ToolSideEffectLevel::External => Self::External,
        }
    }
}

/// What the agent wants to do.
#[derive(Debug, Clone, uniffi::Record)]
pub struct ApprovalRequest {
    pub request_id: String,
    pub tool_call_id: String,
    pub tool_name: String,
    /// Why approval is being asked for.
    pub description: String,
    /// The tool's input, as JSON.
    pub input_json: String,
    pub side_effect_level: Option<SideEffectLevel>,
}

impl From<&basis::ApprovalRequest> for ApprovalRequest {
    fn from(request: &basis::ApprovalRequest) -> Self {
        Self {
            request_id: request.request_id.clone(),
            tool_call_id: request.tool_call_id.clone(),
            tool_name: request.tool_name.clone(),
            description: request.description.clone(),
            input_json: request.input.to_string(),
            side_effect_level: request.side_effect_level.map(Into::into),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum ApprovalDecision {
    Allow,
    Deny,
    /// Allow, and stop asking about this tool for the live session.
    AllowForSession,
    /// Deny, and stop asking about this tool for the live session.
    DenyForSession,
}

/// The decision, and — when it refuses — the words the model reads.
#[derive(Debug, Clone, uniffi::Record)]
pub struct ApprovalAnswer {
    pub decision: ApprovalDecision,
    #[uniffi(default = None)]
    pub reason: Option<String>,
}

impl From<ApprovalAnswer> for basis::ApprovalAnswer {
    fn from(answer: ApprovalAnswer) -> Self {
        let decision = match answer.decision {
            ApprovalDecision::Allow => basis::ApprovalDecision::Allow,
            ApprovalDecision::Deny => basis::ApprovalDecision::Deny,
            ApprovalDecision::AllowForSession => basis::ApprovalDecision::AllowForSession,
            ApprovalDecision::DenyForSession => basis::ApprovalDecision::DenyForSession,
        };
        Self {
            decision,
            reason: answer.reason,
        }
    }
}

/// Answers each consequential tool call while the turn waits.
///
/// The turn is blocked until this returns, so answer — a sheet the person
/// dismisses, a policy lookup — rather than defer past the run. Throwing
/// denies.
#[uniffi::export(with_foreign)]
#[async_trait::async_trait]
pub trait Approver: Send + Sync {
    async fn approve(&self, request: ApprovalRequest) -> Result<ApprovalAnswer, BasisError>;
}

/// The foreign approver as basis's, failing closed.
pub(crate) struct ForeignApprover(pub(crate) Arc<dyn Approver>);

#[async_trait::async_trait]
impl basis::Approver for ForeignApprover {
    async fn approve(&mut self, request: &basis::ApprovalRequest) -> basis::ApprovalAnswer {
        match self.0.approve(request.into()).await {
            Ok(answer) => answer.into(),
            Err(error) => basis::ApprovalAnswer::new(basis::ApprovalDecision::Deny)
                .because(format!("the host's approver failed: {error}")),
        }
    }
}
