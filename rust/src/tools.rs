use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use basis::{
    CancellationToken, async_trait,
    tools::{
        ParallelToolContext, RuntimeToolDescriptor, ToolCapability, ToolDefinition,
        ToolExecutionCategory, ToolExecutor, ToolResult, ToolSideEffectLevel,
    },
};

use crate::{BasisError, SideEffectLevel};

const CANCEL_POLL: std::time::Duration = std::time::Duration::from_millis(20);

/// How a host tool is offered to the model.
#[derive(Debug, Clone, uniffi::Record)]
pub struct HostToolDescriptor {
    pub name: String,
    pub description: String,
    /// The JSON Schema of the tool's input, as JSON.
    pub input_schema_json: String,
    /// Unknown is never harmless; say what the tool reaches.
    pub side_effect_level: SideEffectLevel,
    /// Takes the place of the builtin tool of the same name, which basis
    /// otherwise refuses to shadow. Without a builtin of that name, opening
    /// the workspace fails.
    #[uniffi(default = false)]
    pub replaces_builtin: bool,
}

/// One call the model made to a host tool.
#[derive(Debug, Clone, uniffi::Record)]
pub struct HostToolCall {
    /// The conversation the call belongs to: `Run::agent_id`.
    pub agent_id: String,
    pub tool_call_id: String,
    /// The tool's input, as JSON.
    pub input_json: String,
}

/// A tool the host implements, offered to the model beside or in place of
/// basis's own.
///
/// The turn waits for `call`. Its result is what the model reads; throwing
/// is a failed call the model reads as an error. Cancelling the turn stops
/// waiting and drops the call, which cancels the Swift task running it;
/// whatever the call already did outside the process stays done.
#[uniffi::export(with_foreign)]
#[async_trait::async_trait]
pub trait HostTool: Send + Sync {
    fn descriptor(&self) -> HostToolDescriptor;
    async fn call(&self, call: HostToolCall) -> Result<String, BasisError>;
}

/// What a host tool waits under: its turn's cancel token and deadline.
#[derive(Clone)]
pub(crate) struct TurnBounds {
    pub(crate) cancel: CancellationToken,
    pub(crate) deadline: Option<tokio::time::Instant>,
}

/// Each conversation's turn in flight, by agent id, so a host tool stops
/// waiting when its turn is cancelled or runs out of time.
#[derive(Default)]
pub(crate) struct ActiveTurns(Mutex<HashMap<String, TurnBounds>>);

impl ActiveTurns {
    pub(crate) fn begin(&self, agent_id: &str, bounds: TurnBounds) {
        if let Ok(mut turns) = self.0.lock() {
            turns.insert(agent_id.to_string(), bounds);
        }
    }

    pub(crate) fn end(&self, agent_id: &str) {
        if let Ok(mut turns) = self.0.lock() {
            turns.remove(agent_id);
        }
    }

    fn bounds(&self, agent_id: &str) -> Option<TurnBounds> {
        self.0.lock().ok()?.get(agent_id).cloned()
    }
}

pub(crate) struct ForeignTool {
    descriptor: HostToolDescriptor,
    schema: serde_json::Value,
    tool: Arc<dyn HostTool>,
    turns: Arc<ActiveTurns>,
}

impl ForeignTool {
    pub(crate) fn new(
        tool: Arc<dyn HostTool>,
        turns: Arc<ActiveTurns>,
    ) -> Result<Self, BasisError> {
        let descriptor = tool.descriptor();
        let schema = serde_json::from_str(&descriptor.input_schema_json).map_err(|error| {
            BasisError::InvalidInput {
                message: format!(
                    "{}: input_schema_json is not JSON: {error}",
                    descriptor.name
                ),
            }
        })?;
        Ok(Self {
            descriptor,
            schema,
            tool,
            turns,
        })
    }

    pub(crate) fn name(&self) -> &str {
        &self.descriptor.name
    }

    pub(crate) fn replaces_builtin(&self) -> bool {
        self.descriptor.replaces_builtin
    }
}

impl ToolDefinition for ForeignTool {
    fn descriptor(&self) -> RuntimeToolDescriptor {
        let (capability, side_effect, category) = match self.descriptor.side_effect_level {
            SideEffectLevel::None => (
                ToolCapability::ReadOnly,
                ToolSideEffectLevel::None,
                ToolExecutionCategory::ReadOnlyParallel,
            ),
            SideEffectLevel::LocalState => (
                ToolCapability::FilesystemWrite,
                ToolSideEffectLevel::LocalState,
                ToolExecutionCategory::ExclusiveLocalMutation,
            ),
            SideEffectLevel::Process => (
                ToolCapability::ProcessExec,
                ToolSideEffectLevel::Process,
                ToolExecutionCategory::ExclusiveLocalMutation,
            ),
            SideEffectLevel::External => (
                ToolCapability::ProcessExec,
                ToolSideEffectLevel::External,
                ToolExecutionCategory::ExclusiveLocalMutation,
            ),
        };
        RuntimeToolDescriptor::builder(&self.descriptor.name)
            .description(&self.descriptor.description)
            .input_schema(self.schema.clone())
            .capability(capability)
            .side_effect_level(side_effect)
            .execution_category(category)
            .build()
    }
}

#[async_trait]
impl ToolExecutor for ForeignTool {
    async fn execute(&self, ctx: ParallelToolContext, input: serde_json::Value) -> ToolResult {
        let bounds = self
            .turns
            .bounds(&ctx.agent_id)
            .ok_or("host tool called outside a turn")?;
        let call = self.tool.call(HostToolCall {
            agent_id: ctx.agent_id.clone(),
            tool_call_id: ctx.tool_call_id.clone(),
            input_json: input.to_string(),
        });
        let deadline = async {
            match bounds.deadline {
                Some(deadline) => tokio::time::sleep_until(deadline).await,
                None => std::future::pending().await,
            }
        };
        // mentra's token is a flag with nothing to await, so it is polled.
        let cancelled = async {
            while !bounds.cancel.is_cancelled() {
                tokio::time::sleep(CANCEL_POLL).await;
            }
        };
        tokio::select! {
            result = call => result.map_err(|error| error.to_string()),
            () = cancelled => Err("the turn was cancelled before this call answered; its effects are unknown".into()),
            () = deadline => Err("the turn's deadline passed before this call answered; its effects are unknown".into()),
        }
    }
}
