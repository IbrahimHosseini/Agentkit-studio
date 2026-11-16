//! Provider transports. Native protocols remain distinct; unsupported capabilities stay visible.
mod claude;
mod codex;
mod process;

use agentkit_protocol::{
    Capabilities, ProviderControl, ProviderEvent, RunInput, MAX_ATTACHMENTS, MAX_ATTACHMENTS_BYTES,
    MAX_ATTACHMENT_BYTES,
};
use anyhow::{bail, Result};
use serde_json::json;
use tokio::sync::mpsc::{Receiver, Sender};

/// Resolve native runtimes consistently for Finder launches, probes and execution.
/// Explicit overrides remain authoritative even when invalid.
pub fn codex_executable() -> std::ffi::OsString {
    runtime_executable("codex", "AGENTKIT_CODEX_BIN")
}
pub fn claude_executable() -> std::ffi::OsString {
    runtime_executable("claude", "AGENTKIT_CLAUDE_BIN")
}
fn runtime_executable(name: &str, variable: &str) -> std::ffi::OsString {
    resolve_executable(name, std::env::var_os(variable), std::env::var_os("PATH"),
        std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(std::path::PathBuf::from))
}
fn resolve_executable(name: &str, override_: Option<std::ffi::OsString>, path: Option<std::ffi::OsString>, home: Option<std::path::PathBuf>) -> std::ffi::OsString {
    use std::path::Path;
    fn executable(path: &Path) -> bool {
        let Ok(meta) = path.metadata() else { return false; };
        if !meta.is_file() { return false; }
        #[cfg(unix)] {
            use std::os::unix::fs::PermissionsExt;
            meta.permissions().mode() & 0o111 != 0
        }
        #[cfg(not(unix))] { true }
    }
    if let Some(value) = override_ { return value; }
    let filename = if cfg!(windows) { format!("{name}.exe") } else { name.to_owned() };
    let mut candidates = path.as_ref().map(|p| std::env::split_paths(p).filter(|p| p.is_absolute()).map(|p| p.join(&filename)).collect::<Vec<_>>()).unwrap_or_default();
    if let Some(home) = home.filter(|p| p.is_absolute()) {
        candidates.push(home.join(".local/bin").join(&filename));
        #[cfg(target_os = "macos")]
        if name == "codex" { candidates.push(home.join("Applications/Codex.app/Contents/Resources/codex")); }
    }
    #[cfg(target_os = "macos")] {
        candidates.extend([format!("/opt/homebrew/bin/{filename}"), format!("/usr/local/bin/{filename}")].map(std::path::PathBuf::from));
        if name == "codex" { candidates.push("/Applications/Codex.app/Contents/Resources/codex".into()); }
    }
    candidates.into_iter().find(|p| executable(p)).map(|p| p.into_os_string()).unwrap_or_else(|| filename.into())
}

#[cfg(all(test, unix))]
mod executable_tests {
    use super::*;
    #[test]
    fn finder_path_finds_native_installs_and_respects_overrides() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let root = std::env::temp_dir().join(format!("studio-resolver-{}", uuid::Uuid::new_v4()));
        let local = root.join(".local/bin");
        let bin = root.join("bin");
        std::fs::create_dir_all(&local).unwrap();
        std::fs::create_dir_all(&bin).unwrap();
        for name in ["codex", "claude"] {
            let native = root.join(format!("native-{name}"));
            for file in [&native, &bin.join(name)] {
                std::fs::write(file, "#!/bin/sh\nexit 0\n").unwrap();
                std::fs::set_permissions(file, std::fs::Permissions::from_mode(0o700)).unwrap();
            }
            symlink(&native, local.join(name)).unwrap();
            assert_eq!(resolve_executable(name, None, Some("/nonexistent".into()), Some(root.clone())), local.join(name));
            assert_eq!(resolve_executable(name, None, Some(bin.clone().into_os_string()), Some(root.clone())), bin.join(name));
            assert_eq!(resolve_executable(name, Some("/explicit/missing".into()), None, Some(root.clone())), "/explicit/missing");
            std::fs::set_permissions(bin.join(name), std::fs::Permissions::from_mode(0o600)).unwrap();
            assert_eq!(resolve_executable(name, None, Some(bin.clone().into_os_string()), Some(root.clone())), local.join(name));
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}

pub fn capabilities(provider: &str) -> Capabilities {
    let known = matches!(provider, "codex" | "claude" | "mock");
    Capabilities {
        provider: provider.into(), text: known, images: known, files: known,
        subagents: known, approvals: known,
        interrupt: known, resume: provider == "codex" || provider == "claude",
        notes: match provider {
            "codex" => vec!["App-server protocol adapter; real account and installed-version behavior require a live smoke test.".into(), "Selected roles are requested through native delegation; actual role availability is reported by Codex.".into()],
            "claude" => vec!["Structured CLI adapter with host permission prompts and native per-run permission modes.".into(), "Automatic review availability depends on the installed CLI, account and managed policies; it never falls back to full access.".into()],
            "mock" => vec!["Synthetic local provider. No model or account is contacted. [approval], [hold], [history], [fail] exercise specific states.".into()],
            _ => vec!["Unknown provider.".into()],
        },
    }
}

pub async fn run(
    input: RunInput,
    run_id: String,
    events: Sender<ProviderEvent>,
    controls: Receiver<ProviderControl>,
) -> Result<()> {
    validate(&input)?;
    match input.provider.as_str() {
        "codex" => codex::run(input, events, controls).await,
        "claude" => claude::run(input, events, controls).await,
        "mock" => mock(input, run_id, events, controls).await,
        other => bail!("Unknown provider: {other}"),
    }
}

fn validate(input: &RunInput) -> Result<()> {
    if !std::path::Path::new(&input.workspace).is_dir() {
        bail!("Workspace must be an existing directory");
    }
    for (name, value) in [("model", &input.model), ("effort", &input.effort)] {
        if let Some(value) = value {
            if value.is_empty() || value.len() > 160 || value.chars().any(char::is_control) {
                bail!("Invalid {name}");
            }
        }
    }
    if input
        .instructions
        .as_ref()
        .is_some_and(|v| v.len() > 128 * 1024)
    {
        bail!("Agent instructions exceed 128 KiB");
    }
    if input.prompt.trim().is_empty() {
        bail!("Prompt must not be empty");
    }
    if let Some(role) = &input.role {
        if role.is_empty()
            || role.len() > 80
            || !role
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "_-:".contains(c))
        {
            bail!("Invalid agent role name");
        }
    }
    if input.attachments.len() > MAX_ATTACHMENTS {
        bail!("At most {MAX_ATTACHMENTS} attachments are allowed");
    }
    let mut total_bytes = 0u64;
    for a in &input.attachments {
        let path = std::path::Path::new(&a.path);
        if !path.is_absolute() || !path.is_file() {
            bail!("Attachment must be an existing absolute file: {}", a.path);
        }
        let bytes = std::fs::metadata(path)?.len();
        total_bytes = total_bytes.saturating_add(bytes);
        if total_bytes > MAX_ATTACHMENTS_BYTES {
            bail!("Attachments exceed the combined {MAX_ATTACHMENTS_BYTES}-byte limit");
        }
        if bytes > MAX_ATTACHMENT_BYTES {
            bail!(
                "Attachment exceeds {MAX_ATTACHMENT_BYTES}-byte limit: {}",
                a.path
            );
        }
        if a.kind == "image" {
            if !matches!(
                a.mime.as_str(),
                "image/png" | "image/jpeg" | "image/gif" | "image/webp"
            ) {
                bail!("Unsupported image MIME type: {}", a.mime);
            }
        } else if a.kind != "file" {
            bail!("Unsupported attachment kind: {}", a.kind);
        }
    }
    Ok(())
}

pub(crate) async fn emit(
    events: &Sender<ProviderEvent>,
    kind: &str,
    payload: serde_json::Value,
) -> Result<()> {
    events
        .send(ProviderEvent::new(kind, payload))
        .await
        .map_err(|_| anyhow::anyhow!("Event consumer closed"))
}

pub(crate) fn prompt_with_files(input: &RunInput) -> String {
    let mut prompt = input.prompt.clone();
    for a in input.attachments.iter().filter(|a| a.kind == "file") {
        prompt.push_str("\n\nUser-attached file (read using your file tools): ");
        // JSON quoting keeps filenames containing newlines or quotes unambiguous.
        prompt.push_str(&serde_json::to_string(&a.path).unwrap_or_default());
    }
    prompt
}

async fn mock(
    input: RunInput,
    run_id: String,
    events: Sender<ProviderEvent>,
    mut controls: Receiver<ProviderControl>,
) -> Result<()> {
    emit(&events, "status", json!({"status":"running"})).await?;
    emit(
        &events,
        "notice",
        json!({"text":"Synthetic mock run; no LLM was contacted."}),
    )
    .await?;
    let root = format!("mock-{run_id}");
    events
        .send(ProviderEvent {
            agent_id: Some(root.clone()),
            parent_agent_id: None,
            kind: "agent".into(),
            payload: json!({"role":"mock supervisor", "status":"running"}),
        })
        .await?;
    if input.prompt.contains("[approval]") {
        emit(&events, "status", json!({"status":"awaiting_approval"})).await?;
        emit(&events, "approval", json!({"request_id":"mock-approval", "reason":"Synthetic approval request", "command":"mock action"})).await?;
        loop {
            match controls.recv().await {
                Some(ProviderControl::Approve {
                    request_id,
                    decision,
                }) if request_id == "mock-approval" => {
                    emit(
                        &events,
                        "approval_resolved",
                        json!({"request_id":request_id,"decision":decision}),
                    )
                    .await?;
                    if !decision {
                        emit(&events, "status", json!({"status":"cancelled"})).await?;
                        return Ok(());
                    }
                    break;
                }
                Some(ProviderControl::Stop) | None => {
                    emit(&events, "status", json!({"status":"cancelled"})).await?;
                    return Ok(());
                }
                _ => {}
            }
        }
        emit(&events, "status", json!({"status":"running"})).await?;
    }
    let count = if input.prompt.contains("[history]") {
        10_000
    } else {
        8
    };
    for i in 0..count {
        tokio::select! {
            control = controls.recv() => {
                if matches!(control, Some(ProviderControl::Stop) | None) {
                    emit(&events,"status",json!({"status":"cancelled"})).await?; return Ok(());
                }
            }
            _ = tokio::time::sleep(std::time::Duration::from_millis(if count > 100 { 0 } else { 150 })) => {
                events.send(ProviderEvent { agent_id: Some("mock-worker".into()), parent_agent_id: Some(root.clone()), kind: "text".into(), payload: json!({"text":format!("Mock activity {} — نمونهٔ فعالیت\n", i+1)}) }).await?;
            }
        }
    }
    if input.prompt.contains("[hold]") {
        while let Some(control) = controls.recv().await {
            if matches!(control, ProviderControl::Stop) {
                emit(&events, "status", json!({"status":"cancelled"})).await?;
                return Ok(());
            }
        }
        emit(&events, "status", json!({"status":"interrupted"})).await?;
    } else {
        emit(
            &events,
            "status",
            json!({"status":if input.prompt.contains("[fail]") {"failed"} else {"completed"}}),
        )
        .await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn capabilities_include_claude_host_approvals() {
        assert!(capabilities("claude").approvals);
        assert!(capabilities("codex").approvals);
        assert!(!capabilities("unknown").text);
    }
    #[tokio::test]
    async fn mock_waits_for_explicit_approval() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(32);
        let (cx, cr) = tokio::sync::mpsc::channel(4);
        let input = RunInput {
            provider: "mock".into(),
            workspace: std::env::temp_dir().to_string_lossy().into_owned(),
            prompt: "[approval]".into(),
            role: None,
            attachments: vec![],
            resume_session: None,
            model: None,
            effort: None,
            instructions: None,
            read_only: false,
            claude_auth_mode: None,
            permission_mode: Default::default(),
            devtools_conversation_id: None,
            devtools_cli: None,
        };
        let task = tokio::spawn(run(input, "test".into(), tx, cr));
        while let Some(event) = rx.recv().await {
            if event.kind == "approval" {
                break;
            }
        }
        assert!(!task.is_finished());
        cx.send(ProviderControl::Approve {
            request_id: "mock-approval".into(),
            decision: false,
        })
        .await
        .unwrap();
        task.await.unwrap().unwrap();
        let mut statuses = vec![];
        while let Some(event) = rx.recv().await {
            if event.kind == "status" {
                statuses.push(event.payload["status"].as_str().unwrap().to_owned());
            }
        }
        assert_eq!(statuses, vec!["cancelled"]);
    }
}

/// Evidence created by adapter control flow, never by model text or a tool output.
#[derive(Debug)]
pub struct PreExecutionFailure {
    category: &'static str,
    phase: &'static str,
    code: String,
    message: String,
}
impl std::fmt::Display for PreExecutionFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}
impl std::error::Error for PreExecutionFailure {}
pub fn retry_evidence(error: &anyhow::Error) -> Option<serde_json::Value> {
    error.downcast_ref::<PreExecutionFailure>().map(|e|json!({"safe_to_retry":true,"category":e.category,"phase":e.phase,"code":e.code,"evidence":"Adapter confirmed that no task prompt or turn was submitted."}))
}
pub(crate) fn spawn_error(error: anyhow::Error) -> anyhow::Error {
    let transient = error
        .chain()
        .filter_map(|e| e.downcast_ref::<std::io::Error>())
        .any(|e| {
            matches!(
                e.kind(),
                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
            )
        });
    if transient {
        PreExecutionFailure {
            category: "transient",
            phase: "process_spawn",
            code: "os_transient".into(),
            message: format!("Provider process could not start: {error}"),
        }
        .into()
    } else {
        error
    }
}
pub(crate) fn pre_turn_error(error: &serde_json::Value, submitted: bool) -> Option<anyhow::Error> {
    if submitted {
        return None;
    }
    let code = error["code"].as_i64();
    let reason = error
        .pointer("/data/reason")
        .and_then(serde_json::Value::as_str);
    let info = error.pointer("/data/codexErrorInfo");
    let category =
        if code == Some(-32001) || (code == Some(-32600) && reason == Some("serverShuttingDown")) {
            "transient"
        } else if reason == Some("modelNotFound")
            || info.is_some_and(|v| v == "modelNotFound" || v.get("modelNotFound").is_some())
        {
            "model_unavailable"
        } else {
            return None;
        };
    Some(
        PreExecutionFailure {
            category,
            phase: "before_turn_submission",
            code: reason.map(str::to_owned).unwrap_or_else(|| {
                code.map(|c| c.to_string())
                    .unwrap_or_else(|| "modelNotFound".into())
            }),
            message: format!(
                "Codex rejected startup before task submission: {}",
                error["message"]
                    .as_str()
                    .unwrap_or("Provider error")
                    .chars()
                    .take(1024)
                    .collect::<String>()
            ),
        }
        .into(),
    )
}

#[cfg(test)]
mod retry_tests {
    use super::*;
    #[test]
    fn retry_requires_known_code_and_pre_submission_evidence() {
        let overloaded = json!({"code":-32001,"message":"Server overloaded; retry later."});
        assert!(retry_evidence(&pre_turn_error(&overloaded, false).unwrap()).is_some());
        assert!(pre_turn_error(&overloaded, true).is_none());
        assert!(pre_turn_error(
            &json!({"code":-32603,"message":"Please retry transient failure"}),
            false
        )
        .is_none());
        assert!(pre_turn_error(&json!({"code":401,"message":"Not logged in"}), false).is_none());
        let missing = json!({"code":-32600,"data":{"codexErrorInfo":"modelNotFound"}});
        assert_eq!(
            retry_evidence(&pre_turn_error(&missing, false).unwrap()).unwrap()["category"],
            "model_unavailable"
        );
    }
}

/// Extract provider diagnostics without treating an entire raw response as an error message.
pub(crate) fn diagnostic(value: &serde_json::Value) -> Option<String> {
    use serde_json::Value;
    fn collect(value: &Value, depth: usize) -> Option<String> {
        if depth > 5 {
            return None;
        }
        match value {
            Value::String(s) if !s.trim().is_empty() => Some(s.chars().take(16000).collect()),
            Value::Array(items) => {
                let parts = items
                    .iter()
                    .take(16)
                    .filter_map(|v| collect(v, depth + 1))
                    .collect::<Vec<_>>();
                if parts.is_empty() {
                    None
                } else {
                    Some(parts.join("\n"))
                }
            }
            Value::Object(fields) => [
                "message",
                "error",
                "errors",
                "detail",
                "details",
                "additionalDetails",
            ]
            .iter()
            .filter_map(|key| fields.get(*key).and_then(|v| collect(v, depth + 1)))
            .next(),
            _ => None,
        }
    }
    collect(value, 0)
}
