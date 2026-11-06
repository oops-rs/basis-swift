//! basis-a2a's server over a host's own tasks (basis ADR-0033, ADR-0048),
//! bound for Swift.
//!
//! The host implements [`A2aTaskHost`]; the server does the rest — both
//! bindings, the card, principals, quotas, streams, hops and restarts —
//! exactly as basis-a2a does. Two seams differ from the Rust trait only in
//! shape: progress is pulled with a cursor, because a stream cannot cross
//! UniFFI, and an [`A2aAuthenticator`] declares its scheme as a record.

use std::{
    collections::HashMap,
    net::SocketAddr,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

use basis_a2a::{
    A2aService, AgentIdentity, Authenticator, HostError, HostedProgress, HostedReply as RustReply,
    HostedStanding as RustStanding, HostedStart as RustStart, Principal, Refusal, RequestHead,
    Security, ServeConfig, TaskHost,
    a2a::{
        AgentSkill, ApiKeySecurityScheme, Artifact, HttpAuthSecurityScheme, Part, PartContent,
        SecurityScheme,
    },
};
use futures::{
    FutureExt,
    future::BoxFuture,
    stream::{self, BoxStream, StreamExt},
};
use tokio::sync::oneshot;

use crate::{BasisError, RUNTIME, on_runtime};

/// What one part of a message or artifact holds.
#[derive(Debug, Clone, PartialEq, uniffi::Enum)]
pub enum A2aPartContent {
    Text {
        text: String,
    },
    /// Structured data, as JSON.
    Data {
        json: String,
    },
    Raw {
        bytes: Vec<u8>,
    },
    Url {
        url: String,
    },
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct A2aPart {
    pub content: A2aPartContent,
    #[uniffi(default = None)]
    pub filename: Option<String>,
    #[uniffi(default = None)]
    pub media_type: Option<String>,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct A2aArtifact {
    /// Unique within the task; `result` is the server's own.
    pub artifact_id: String,
    #[uniffi(default = None)]
    pub name: Option<String>,
    #[uniffi(default = None)]
    pub description: Option<String>,
    /// At least one.
    pub parts: Vec<A2aPart>,
}

/// What a message asks the host to start.
#[derive(Debug, Clone, uniffi::Record)]
pub struct A2aHostedStart {
    /// The A2A id callers see; the host may key its task by it.
    pub task_id: String,
    pub principal: String,
    /// What the authenticator vouched for, beside the name. The caller cannot
    /// write it.
    pub attributes: HashMap<String, String>,
    pub context_id: String,
    /// The host's id for the latest task in this context, for a follow-up.
    pub continues: Option<String>,
    /// The message rendered as one prompt.
    pub prompt: String,
    pub parts: Vec<A2aPart>,
    /// The message's `metadata`, as JSON; `{}` when it had none.
    pub metadata_json: String,
    /// The request's own `metadata`, as JSON.
    pub request_metadata_json: String,
    pub extensions: Vec<String>,
    pub hop: u32,
    /// When the task must have settled by, in milliseconds since the epoch.
    pub deadline_ms: u64,
}

/// Where a host's task stands.
#[derive(Debug, Clone, PartialEq, uniffi::Enum)]
pub enum A2aHostedStanding {
    Submitted,
    Working,
    /// Its parts become the `result` artifact; none means no result artifact.
    Completed {
        parts: Vec<A2aPart>,
    },
    Failed {
        error: String,
    },
    Canceled,
    /// Paused on a question to its caller; these parts are the question.
    InputRequired {
        parts: Vec<A2aPart>,
    },
}

/// A caller's reply to a task paused on a question.
#[derive(Debug, Clone, uniffi::Record)]
pub struct A2aHostedReply {
    pub message_id: String,
    pub text: String,
    pub parts: Vec<A2aPart>,
}

/// One progress note, and the cursor that follows it.
#[derive(Debug, Clone, uniffi::Record)]
pub struct A2aProgressNote {
    pub cursor: u64,
    pub note: String,
}

/// Why a host refused or failed: whose fault it is.
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum A2aHostError {
    /// The context is still working on an earlier message.
    #[error("busy: {message}")]
    Busy { message: String },
    /// The caller's to fix; told to it as invalid params.
    #[error("refused: {message}")]
    Refused { message: String },
    /// The host's own failure; the caller is told only that the server failed.
    #[error("{message}")]
    Internal { message: String },
}

impl From<uniffi::UnexpectedUniFFICallbackError> for A2aHostError {
    fn from(error: uniffi::UnexpectedUniFFICallbackError) -> Self {
        Self::Internal {
            message: error.reason,
        }
    }
}

/// The tasks a server serves, run by the host (basis-a2a's `TaskHost`).
#[uniffi::export(with_foreign)]
#[async_trait::async_trait]
pub trait A2aTaskHost: Send + Sync {
    /// Begins the work and returns the host's own id for it.
    async fn start(&self, start: A2aHostedStart) -> Result<String, A2aHostError>;
    async fn standing(&self, id: String) -> Result<A2aHostedStanding, A2aHostError>;
    async fn cancel(&self, id: String) -> Result<(), A2aHostError>;
    /// Returns once the task has settled; a task asking its caller has not.
    async fn settled(&self, id: String) -> Result<(), A2aHostError>;
    /// Hands a task paused on a question its caller's reply.
    async fn reply(&self, id: String, reply: A2aHostedReply) -> Result<(), A2aHostError>;
    /// The artifacts the task has produced so far, served after `result`.
    async fn artifacts(&self, id: String) -> Result<Vec<A2aArtifact>, A2aHostError>;
    /// The first note after `cursor` — after now, when `cursor` is `nil` —
    /// waiting for one; `nil` once the task has settled or has nothing more
    /// to say. A thrown error ends the notes, never the task.
    async fn progress(
        &self,
        id: String,
        cursor: Option<u64>,
    ) -> Result<Option<A2aProgressNote>, A2aHostError>;
}

/// What an authenticator is shown of a request: never its body.
#[derive(Debug, Clone, uniffi::Record)]
pub struct A2aRequestHead {
    pub method: String,
    pub uri: String,
    /// Every header, in order; a repeated name appears once per value.
    pub headers: Vec<A2aHeader>,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct A2aHeader {
    pub name: String,
    /// Lossy for bytes that are not UTF-8.
    pub value: String,
}

/// Who a request is from.
#[derive(Debug, Clone, uniffi::Record)]
pub struct A2aPrincipal {
    /// 1–64 ASCII letters, digits, `-`, `_`, `.` or `:`. Tasks are owned by it.
    pub name: String,
    /// What the authenticator verified about this request, handed to the host.
    pub attributes: HashMap<String, String>,
}

/// Why a request was refused before its body was read.
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum A2aRefusal {
    /// `401`. Also what an authenticator that could not be reached means.
    #[error("unauthenticated")]
    Unauthenticated,
    /// `403`.
    #[error("forbidden")]
    Forbidden,
}

impl From<uniffi::UnexpectedUniFFICallbackError> for A2aRefusal {
    fn from(_: uniffi::UnexpectedUniFFICallbackError) -> Self {
        Self::Unauthenticated
    }
}

/// The scheme a card declares callers must present.
#[derive(Debug, Clone, uniffi::Enum)]
pub enum A2aSecurityScheme {
    HttpAuth {
        scheme: String,
        description: Option<String>,
        bearer_format: Option<String>,
    },
    ApiKey {
        location: String,
        name: String,
        description: Option<String>,
    },
}

/// Turns a request's head into the principal it acts as (basis ADR-0036).
#[uniffi::export(with_foreign)]
#[async_trait::async_trait]
pub trait A2aAuthenticator: Send + Sync {
    /// Called before the body is read; bound any waiting yourself.
    async fn authenticate(&self, head: A2aRequestHead) -> Result<A2aPrincipal, A2aRefusal>;
    /// The scheme callers must present, under `scheme_name`; read once.
    fn security(&self) -> A2aSecurity;
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct A2aSecurity {
    pub scheme_name: String,
    pub scheme: A2aSecurityScheme,
    /// The `WWW-Authenticate` value of a `401`.
    #[uniffi(default = "Bearer")]
    pub challenge: String,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct A2aSkill {
    pub id: String,
    pub name: String,
    pub description: String,
    #[uniffi(default = [])]
    pub tags: Vec<String>,
    #[uniffi(default = [])]
    pub examples: Vec<String>,
}

/// How a server is opened. Unset means basis-a2a's default.
#[derive(Debug, Clone, uniffi::Record)]
pub struct A2aServeOptions {
    pub name: String,
    pub description: String,
    #[uniffi(default = None)]
    pub version: Option<String>,
    #[uniffi(default = [])]
    pub skills: Vec<A2aSkill>,
    /// Where the server keeps its task index; the host's own state is its own.
    pub state_dir: String,
    /// `host:port`; port 0 picks one.
    pub address: String,
    /// The URL the card advertises, behind a proxy.
    #[uniffi(default = None)]
    pub public_url: Option<String>,
    #[uniffi(default = None)]
    pub task_deadline_seconds: Option<u64>,
    #[uniffi(default = None)]
    pub live_task_quota: Option<u32>,
    #[uniffi(default = None)]
    pub stream_cap: Option<u32>,
    #[uniffi(default = None)]
    pub hop_cap: Option<u32>,
}

/// An A2A server, serving until [`shutdown`](Self::shutdown).
#[derive(uniffi::Object)]
pub struct A2aServer {
    endpoint: String,
    port: u16,
    stop: Mutex<Option<oneshot::Sender<()>>>,
    stopped: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

#[uniffi::export]
impl A2aServer {
    /// Opens the server over `host`, binds `address` and starts serving.
    /// Without an authenticator, only a loopback address is allowed.
    #[uniffi::constructor(default(authenticator = None))]
    pub async fn serve(
        options: A2aServeOptions,
        host: Arc<dyn A2aTaskHost>,
        authenticator: Option<Arc<dyn A2aAuthenticator>>,
    ) -> Result<Arc<Self>, BasisError> {
        on_runtime(async move {
            let address: SocketAddr = options.address.parse().map_err(|error| {
                invalid(format!(
                    "address {:?} is not host:port: {error}",
                    options.address
                ))
            })?;
            let config = config(&options, host, authenticator)?;
            let service = A2aService::open(config).map_err(run_error)?;
            let server = basis_a2a::A2aServer::bind(address, service)
                .await
                .map_err(run_error)?;
            let endpoint = server.endpoint().to_string();
            let port = server.local_addr().port();
            let (stop, stopped) = oneshot::channel();
            let serving = RUNTIME.spawn(async move {
                let signal = async {
                    let _ = stopped.await;
                };
                if let Err(error) = server.serve_until(signal).await {
                    eprintln!("basis-swift: the A2A server stopped: {error}");
                }
            });
            Ok(Arc::new(Self {
                endpoint,
                port,
                stop: Mutex::new(Some(stop)),
                stopped: Mutex::new(Some(serving)),
            }))
        })
        .await
    }

    /// The JSON-RPC endpoint the card advertises.
    pub fn endpoint(&self) -> String {
        self.endpoint.clone()
    }

    /// The port actually bound.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Stops accepting, lets in-flight requests finish, and leaves every
    /// unsettled task to be awaited again by the next server.
    pub async fn shutdown(&self) {
        let stop = self.stop.lock().ok().and_then(|mut stop| stop.take());
        let serving = self
            .stopped
            .lock()
            .ok()
            .and_then(|mut serving| serving.take());
        if let Some(stop) = stop {
            let _ = stop.send(());
        }
        if let Some(serving) = serving {
            let _ = serving.await;
        }
    }
}

fn invalid(message: String) -> BasisError {
    BasisError::InvalidInput { message }
}

fn run_error(error: impl std::fmt::Display) -> BasisError {
    BasisError::Run {
        message: error.to_string(),
    }
}

fn config(
    options: &A2aServeOptions,
    host: Arc<dyn A2aTaskHost>,
    authenticator: Option<Arc<dyn A2aAuthenticator>>,
) -> Result<ServeConfig, BasisError> {
    let mut identity = AgentIdentity::new(&options.name, &options.description);
    if let Some(version) = &options.version {
        identity = identity.with_version(version);
    }
    for skill in &options.skills {
        identity = identity.with_skill(AgentSkill {
            id: skill.id.clone(),
            name: skill.name.clone(),
            description: skill.description.clone(),
            tags: skill.tags.clone(),
            examples: (!skill.examples.is_empty()).then(|| skill.examples.clone()),
            input_modes: None,
            output_modes: None,
            security_requirements: None,
        });
    }
    let host: Arc<dyn TaskHost> = Arc::new(ForeignHost(host));
    let mut config = ServeConfig::hosted(identity, PathBuf::from(&options.state_dir), host);
    if let Some(authenticator) = authenticator {
        config = config.with_authenticator(Arc::new(ForeignAuthenticator::new(authenticator)));
    }
    if let Some(url) = &options.public_url {
        config = config.with_public_url(url);
    }
    if let Some(seconds) = options.task_deadline_seconds {
        config = config.with_task_deadline(Duration::from_secs(seconds));
    }
    if let Some(quota) = options.live_task_quota {
        config = config.with_live_task_quota(quota as usize);
    }
    if let Some(cap) = options.stream_cap {
        config = config.with_stream_cap(cap as usize);
    }
    if let Some(cap) = options.hop_cap {
        config = config.with_hop_cap(cap);
    }
    Ok(config)
}

impl From<A2aPart> for Part {
    fn from(part: A2aPart) -> Self {
        let content = match part.content {
            A2aPartContent::Text { text } => PartContent::Text(text),
            // Not JSON: kept as the text it is rather than dropped.
            A2aPartContent::Data { json } => serde_json::from_str(&json)
                .map(PartContent::Data)
                .unwrap_or(PartContent::Text(json)),
            A2aPartContent::Raw { bytes } => PartContent::Raw(bytes),
            A2aPartContent::Url { url } => PartContent::Url(url),
        };
        Part {
            content,
            filename: part.filename,
            media_type: part.media_type,
            metadata: None,
        }
    }
}

impl From<Part> for A2aPart {
    fn from(part: Part) -> Self {
        let content = match part.content {
            PartContent::Text(text) => A2aPartContent::Text { text },
            PartContent::Data(value) => A2aPartContent::Data {
                json: value.to_string(),
            },
            PartContent::Raw(bytes) => A2aPartContent::Raw { bytes },
            PartContent::Url(url) => A2aPartContent::Url { url },
        };
        Self {
            content,
            filename: part.filename,
            media_type: part.media_type,
        }
    }
}

impl From<A2aArtifact> for Artifact {
    fn from(artifact: A2aArtifact) -> Self {
        Artifact {
            artifact_id: artifact.artifact_id,
            name: artifact.name,
            description: artifact.description,
            parts: artifact.parts.into_iter().map(Part::from).collect(),
            metadata: None,
            extensions: None,
        }
    }
}

impl From<A2aHostedStanding> for RustStanding {
    fn from(standing: A2aHostedStanding) -> Self {
        let parts = |parts: Vec<A2aPart>| parts.into_iter().map(Part::from).collect();
        match standing {
            A2aHostedStanding::Submitted => Self::Submitted,
            A2aHostedStanding::Working => Self::Working,
            A2aHostedStanding::Completed { parts: answer } => Self::Completed {
                parts: parts(answer),
            },
            A2aHostedStanding::Failed { error } => Self::Failed { error },
            A2aHostedStanding::Canceled => Self::Canceled,
            A2aHostedStanding::InputRequired { parts: question } => Self::InputRequired {
                parts: parts(question),
            },
        }
    }
}

impl From<A2aHostError> for HostError {
    fn from(error: A2aHostError) -> Self {
        match error {
            A2aHostError::Busy { message } => Self::Busy(message),
            A2aHostError::Refused { message } => Self::Refused(message),
            A2aHostError::Internal { message } => Self::Internal(message),
        }
    }
}

fn json_map(map: &HashMap<String, serde_json::Value>) -> String {
    serde_json::to_string(map).unwrap_or_else(|_| "{}".to_string())
}

/// A foreign host, as basis-a2a's `TaskHost`.
struct ForeignHost(Arc<dyn A2aTaskHost>);

impl TaskHost for ForeignHost {
    fn start(&self, start: RustStart) -> BoxFuture<'_, Result<String, HostError>> {
        let start = A2aHostedStart {
            task_id: start.task_id,
            principal: start.principal,
            attributes: start.attributes.into_iter().collect(),
            context_id: start.context_id,
            continues: start.continues,
            prompt: start.prompt,
            parts: start.parts.into_iter().map(A2aPart::from).collect(),
            metadata_json: json_map(&start.metadata),
            request_metadata_json: json_map(&start.request_metadata),
            extensions: start.extensions,
            hop: start.hop,
            deadline_ms: start.deadline_ms,
        };
        async move { self.0.start(start).await.map_err(HostError::from) }.boxed()
    }

    fn standing(&self, id: &str) -> BoxFuture<'_, Result<RustStanding, HostError>> {
        let id = id.to_string();
        async move {
            self.0
                .standing(id)
                .await
                .map(RustStanding::from)
                .map_err(HostError::from)
        }
        .boxed()
    }

    fn cancel(&self, id: &str) -> BoxFuture<'_, Result<(), HostError>> {
        let id = id.to_string();
        async move { self.0.cancel(id).await.map_err(HostError::from) }.boxed()
    }

    fn settled(&self, id: &str) -> BoxFuture<'_, Result<(), HostError>> {
        let id = id.to_string();
        async move { self.0.settled(id).await.map_err(HostError::from) }.boxed()
    }

    fn reply(&self, id: &str, reply: RustReply) -> BoxFuture<'_, Result<(), HostError>> {
        let id = id.to_string();
        let reply = A2aHostedReply {
            message_id: reply.message_id,
            text: reply.text,
            parts: reply.parts.into_iter().map(A2aPart::from).collect(),
        };
        async move { self.0.reply(id, reply).await.map_err(HostError::from) }.boxed()
    }

    fn artifacts(&self, id: &str) -> BoxFuture<'_, Result<Vec<Artifact>, HostError>> {
        let id = id.to_string();
        async move {
            self.0
                .artifacts(id)
                .await
                .map(|artifacts| artifacts.into_iter().map(Artifact::from).collect())
                .map_err(HostError::from)
        }
        .boxed()
    }

    fn progress(&self, id: &str) -> BoxStream<'static, HostedProgress> {
        let host = Arc::clone(&self.0);
        let id = id.to_string();
        stream::unfold(Some(None), move |cursor: Option<Option<u64>>| {
            let host = Arc::clone(&host);
            let id = id.clone();
            async move {
                let cursor = cursor?;
                match host.progress(id, cursor).await {
                    Ok(Some(note)) => {
                        Some((HostedProgress::new(note.note), Some(Some(note.cursor))))
                    }
                    Ok(None) | Err(_) => None,
                }
            }
        })
        .boxed()
    }
}

/// A foreign authenticator, as basis-a2a's `Authenticator`.
struct ForeignAuthenticator {
    inner: Arc<dyn A2aAuthenticator>,
    security: Security,
    challenge: String,
}

impl ForeignAuthenticator {
    fn new(inner: Arc<dyn A2aAuthenticator>) -> Self {
        let declared = inner.security();
        let scheme = match declared.scheme {
            A2aSecurityScheme::HttpAuth {
                scheme,
                description,
                bearer_format,
            } => SecurityScheme::HttpAuth(HttpAuthSecurityScheme {
                scheme,
                description,
                bearer_format,
            }),
            A2aSecurityScheme::ApiKey {
                location,
                name,
                description,
            } => SecurityScheme::ApiKey(ApiKeySecurityScheme {
                location,
                name,
                description,
            }),
        };
        Self {
            inner,
            security: Security::required(declared.scheme_name, scheme),
            challenge: declared.challenge,
        }
    }
}

impl Authenticator for ForeignAuthenticator {
    fn authenticate<'a>(
        &'a self,
        head: &'a RequestHead,
    ) -> BoxFuture<'a, Result<Principal, Refusal>> {
        let head = A2aRequestHead {
            method: head.method.to_string(),
            uri: head.uri.to_string(),
            headers: head
                .headers
                .iter()
                .map(|(name, value)| A2aHeader {
                    name: name.as_str().to_string(),
                    value: String::from_utf8_lossy(value.as_bytes()).into_owned(),
                })
                .collect(),
        };
        async move {
            let principal =
                self.inner
                    .authenticate(head)
                    .await
                    .map_err(|refusal| match refusal {
                        A2aRefusal::Unauthenticated => Refusal::Unauthenticated,
                        A2aRefusal::Forbidden => Refusal::Forbidden,
                    })?;
            let mut attributes: Vec<_> = principal.attributes.into_iter().collect();
            attributes.sort();
            let named = Principal::new(principal.name).ok_or(Refusal::Unauthenticated)?;
            Ok(attributes.into_iter().fold(named, |named, (key, value)| {
                named.with_attribute(key, value)
            }))
        }
        .boxed()
    }

    fn security(&self) -> Option<Security> {
        Some(self.security.clone())
    }

    fn challenge(&self) -> &str {
        &self.challenge
    }
}
