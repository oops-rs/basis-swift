use std::sync::Arc;

use basis::{RunProfile, RunSpec};

use crate::tools::{ActiveTurns, ForeignTool};
use crate::{BasisError, ConversationOptions, HostTool, Run, WorkspaceOptions, on_runtime};

/// An opened repository: its context, model and tools settled once, so runs
/// mint from it without settling them again.
#[derive(uniffi::Object)]
pub struct Workspace {
    inner: Arc<basis::Workspace>,
    turns: Arc<ActiveTurns>,
}

#[uniffi::export]
impl Workspace {
    /// Opens the workspace rooted at `path`, offering the model `tools` beside
    /// (or, with `replaces_builtin`, in place of) basis's own.
    #[uniffi::constructor(default(tools = []))]
    pub async fn open(
        path: String,
        options: WorkspaceOptions,
        tools: Vec<Arc<dyn HostTool>>,
    ) -> Result<Arc<Self>, BasisError> {
        on_runtime(async move {
            let turns = Arc::new(ActiveTurns::default());
            let tools = tools
                .into_iter()
                .map(|tool| ForeignTool::new(tool, Arc::clone(&turns)))
                .collect::<Result<Vec<_>, _>>()?;
            let mut builder = options.apply(path.into())?;
            let mut replacing = Vec::new();
            for tool in tools {
                if tool.replaces_builtin() {
                    replacing.push(tool);
                } else {
                    builder = builder.with_tool(tool);
                }
            }
            let workspace = builder.open().await?;
            // basis refuses a host tool that shadows a builtin; one that means
            // to replace it does so here, before any run can be minted.
            let runtime = workspace.mentra_runtime();
            for tool in replacing {
                let name = tool.name().to_string();
                if !runtime.unregister_tool(&name) {
                    return Err(BasisError::InvalidInput {
                        message: format!("{name} replaces no builtin tool"),
                    });
                }
                runtime
                    .try_register_tool(tool)
                    .map_err(|error| BasisError::Run {
                        message: error.to_string(),
                    })?;
            }
            Ok(Arc::new(Self {
                inner: Arc::new(workspace),
                turns,
            }))
        })
        .await
    }

    /// Mints a new conversation. Nothing is sent until [`Run::send`].
    #[uniffi::method(default(options = None))]
    pub fn prepare(&self, options: Option<ConversationOptions>) -> Result<Arc<Run>, BasisError> {
        let mut spec = RunSpec::new("");
        if let Some(options) = options {
            if let Some(name) = &options.session_name {
                spec = spec.with_session_name(name.clone());
            }
            spec = spec.with_profile(
                RunProfile::new().with_provider_request_options(options.request_options()),
            );
        }
        let run = self.inner.prepare(spec)?;
        Ok(Run::new(run, Arc::clone(&self.turns)))
    }

    /// Picks up the conversation `agent_id` names, from this workspace's
    /// history, with the options it was prepared with. Resuming another
    /// workspace's conversation is refused.
    pub fn resume(&self, agent_id: String) -> Result<Arc<Run>, BasisError> {
        let run = self.inner.resume(&agent_id, RunSpec::new(""))?;
        Ok(Run::new(run, Arc::clone(&self.turns)))
    }
}
