//! Shared wire types for local clients, daemon and provider adapters.
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const PROTOCOL_VERSION: u32 = 2;
pub const MAX_ATTACHMENTS: usize = 12;
pub const MAX_ATTACHMENT_BYTES: u64 = 16 * 1024 * 1024;
pub const MAX_ATTACHMENTS_BYTES: u64 = 40 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attachment {
    pub id: String,
    pub kind: String,
    pub path: String,
    pub mime: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaudeAuthMode {
    #[default]
    Configured,
    Native,
}

/// User-selected approval boundary. Read-only runs always take precedence.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionMode {
    #[default]
    Ask,
    Auto,
    FullAccess,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunInput {
    pub provider: String,
    pub workspace: String,
    pub prompt: String,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub attachments: Vec<Attachment>,
    #[serde(default)]
    pub resume_session: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub effort: Option<String>,
    #[serde(default)]
    pub instructions: Option<String>,
    #[serde(default)]
    pub read_only: bool,
    #[serde(default)]
    pub claude_auth_mode: Option<ClaudeAuthMode>,
    #[serde(default)]
    pub permission_mode: PermissionMode,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Run {
    pub id: String,
    pub provider: String,
    pub workspace: String,
    pub prompt: String,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
    pub provider_session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunEvent {
    pub seq: u64,
    pub run_id: String,
    pub timestamp: String,
    pub agent_id: Option<String>,
    pub parent_agent_id: Option<String>,
    pub kind: String,
    pub payload: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderEvent {
    pub agent_id: Option<String>,
    pub parent_agent_id: Option<String>,
    pub kind: String,
    pub payload: Value,
}
impl ProviderEvent {
    pub fn new(kind: impl Into<String>, payload: Value) -> Self {
        Self {
            agent_id: None,
            parent_agent_id: None,
            kind: kind.into(),
            payload,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProviderControl {
    Stop,
    Approve { request_id: String, decision: bool },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Capabilities {
    pub provider: String,
    pub text: bool,
    pub images: bool,
    pub files: bool,
    pub subagents: bool,
    pub approvals: bool,
    pub interrupt: bool,
    pub resume: bool,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    pub id: String,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcError {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}
