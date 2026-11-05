use std::path::PathBuf;

use basis::{
    BuiltinProvider, ModelInfo, ModelSelector, RuntimeBuilder, SystemPrompt, ToolResultPolicy,
    ToolRoster, WorkspaceBuilder, runtime::FileToolProfile,
};

use crate::BasisError;

/// Which built-in provider answers, when the host names one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum Provider {
    Anthropic,
    Gemini,
    OpenAi,
    OpenRouter,
    Ollama,
    LmStudio,
}

impl From<Provider> for BuiltinProvider {
    fn from(provider: Provider) -> Self {
        match provider {
            Provider::Anthropic => Self::Anthropic,
            Provider::Gemini => Self::Gemini,
            Provider::OpenAi => Self::OpenAI,
            Provider::OpenRouter => Self::OpenRouter,
            Provider::Ollama => Self::Ollama,
            Provider::LmStudio => Self::LmStudio,
        }
    }
}

/// The request shape spoken to a base URL, as basis's `Wire`. A provider
/// preset with no base URL speaks its vendor's own wire and ignores this.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum Wire {
    ChatCompletions,
    Responses,
}

impl From<Wire> for basis::runtime::Wire {
    fn from(wire: Wire) -> Self {
        match wire {
            Wire::ChatCompletions => Self::ChatCompletions,
            Wire::Responses => Self::Responses,
        }
    }
}

/// Where conversations are kept, so a later process can resume them.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum History {
    /// mentra's default location.
    Default,
    /// A directory the host names — an app's container, typically.
    Directory { path: String },
    /// In memory: resumable while the workspace lives, gone with the process.
    Ephemeral,
}

/// What a workspace is opened with. Every field is optional; unset means
/// basis's own default, and basis reads no config file it was not handed.
#[derive(Debug, Clone, uniffi::Record)]
pub struct WorkspaceOptions {
    #[uniffi(default = None)]
    pub provider: Option<Provider>,
    /// A model id; unset asks the provider for its newest.
    #[uniffi(default = None)]
    pub model: Option<String>,
    /// Use `model` exactly as given rather than asking the provider to
    /// resolve it, as `WorkspaceBuilder::with_resolved_model`. Needs both
    /// `provider` and `model`.
    #[uniffi(default = false)]
    pub model_is_resolved: bool,
    #[uniffi(default = None)]
    pub base_url: Option<String>,
    /// Read only beside `base_url`, as `RuntimeBuilder::with_wire`.
    #[uniffi(default = None)]
    pub wire: Option<Wire>,
    /// A JSON object added to every request sent to `base_url`, as
    /// `RuntimeBuilder::with_request_fields`.
    #[uniffi(default = None)]
    pub request_fields_json: Option<String>,
    #[uniffi(default = None)]
    pub api_key: Option<String>,
    /// A `config.json` to read as the host's own, exactly as basis's
    /// `WorkspaceBuilder::with_config_file` does.
    #[uniffi(default = None)]
    pub config_file: Option<String>,
    /// Unset is [`History::Default`].
    #[uniffi(default = None)]
    pub history: Option<History>,
    /// Replaces basis's system prompt outright.
    #[uniffi(default = None)]
    pub system_prompt: Option<String>,
    /// `false` is `WorkspaceBuilder::without_discovery`: no project or user
    /// context, skills, or config is read from disk.
    #[uniffi(default = true)]
    pub discovery: bool,
    /// The only tools the model is offered, as `ToolRoster::only`. Unset
    /// offers basis's default roster.
    #[uniffi(default = None)]
    pub tool_roster: Option<Vec<String>>,
    /// `false` is `FileToolProfile::None`: no builtin file tools exist.
    #[uniffi(default = true)]
    pub file_tools: bool,
    /// `true` is `ToolResultPolicy::unlimited()`: tool results reach the
    /// model whole, for a host that already bounds them.
    #[uniffi(default = false)]
    pub unlimited_tool_results: bool,
    /// As `RuntimeBuilder::with_provider_retry_budget`.
    #[uniffi(default = None)]
    pub provider_retry_budget: Option<u32>,
}

impl WorkspaceOptions {
    pub(crate) fn apply(self, path: PathBuf) -> Result<WorkspaceBuilder, BasisError> {
        let mut runtime = RuntimeBuilder::default();
        if let Some(provider) = self.provider {
            runtime = runtime.with_provider(provider.into());
        }
        if let Some(model) = self.model.clone().filter(|_| !self.model_is_resolved) {
            runtime = runtime.with_model(ModelSelector::Id(model));
        }
        if let Some(base_url) = self.base_url {
            runtime = runtime.with_base_url(base_url);
        }
        if let Some(wire) = self.wire {
            runtime = runtime.with_wire(wire.into());
        }
        if let Some(fields) = self.request_fields_json {
            let fields =
                serde_json::from_str(&fields).map_err(|error| BasisError::InvalidInput {
                    message: format!("request_fields_json must be a JSON object: {error}"),
                })?;
            runtime = runtime.with_request_fields(fields);
        }
        if let Some(api_key) = self.api_key {
            runtime = runtime.with_api_key(api_key);
        }
        if !self.file_tools {
            runtime = runtime.with_file_tools(FileToolProfile::None);
        }
        if self.unlimited_tool_results {
            runtime = runtime.with_tool_result_policy(ToolResultPolicy::unlimited());
        }
        if let Some(budget) = self.provider_retry_budget {
            runtime = runtime.with_provider_retry_budget(budget as usize);
        }
        runtime = match self.history.unwrap_or(History::Default) {
            History::Default => runtime,
            History::Directory { path } => runtime.with_store_dir(path),
            History::Ephemeral => runtime.with_ephemeral_history(),
        };

        let mut workspace = basis::Workspace::builder(path).with_runtime_builder(runtime);
        if self.model_is_resolved {
            let (Some(model), Some(provider)) = (self.model, self.provider) else {
                return Err(BasisError::InvalidInput {
                    message: "model_is_resolved needs both provider and model".into(),
                });
            };
            workspace = workspace
                .with_resolved_model(ModelInfo::new(model, BuiltinProvider::from(provider)));
        }
        if let Some(config_file) = self.config_file {
            workspace = workspace.with_config_file(config_file);
        }
        if let Some(prompt) = self.system_prompt {
            workspace = workspace.with_system_prompt(SystemPrompt::Replace(prompt));
        }
        if !self.discovery {
            workspace = workspace.without_discovery();
        }
        if let Some(roster) = self.tool_roster {
            workspace = workspace.with_tool_roster(ToolRoster::only(roster));
        }
        Ok(workspace)
    }
}
