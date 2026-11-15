use crate::{
    emit,
    process::{self, Process},
    prompt_with_files,
};
use agentkit_protocol::{
    ClaudeAuthMode, ProviderControl, ProviderEvent, RunInput, MAX_ATTACHMENT_BYTES,
};
use anyhow::{bail, Context, Result};
use base64::Engine;
use serde_json::{json, Value};
use std::collections::HashMap;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::Command,
    sync::mpsc::{Receiver, Sender},
};

// Native mode deliberately leaves HOME, CLAUDE_CONFIG_DIR and native OAuth
// credentials untouched. Remove routing overrides by name, never inspect/log
// their values. Ordinary network proxies remain available for connectivity.
fn conflicting_native_variable(name: &str) -> bool {
    name.starts_with("ANTHROPIC_")
        || matches!(
            name,
            "CLAUDE_CODE_USE_BEDROCK"
                | "CLAUDE_CODE_USE_VERTEX"
                | "CLAUDE_CODE_USE_FOUNDRY"
                | "CLAUDE_CODE_USE_MANTLE"
                | "CLAUDE_CODE_USE_ANTHROPIC_AWS"
                | "CLAUDE_CODE_SKIP_BEDROCK_AUTH"
                | "CLAUDE_CODE_SKIP_VERTEX_AUTH"
                | "CLAUDE_CODE_SKIP_FOUNDRY_AUTH"
                | "CLAUDE_CODE_SKIP_MANTLE_AUTH"
                | "CLAUDE_CODE_PROVIDER_MANAGED_BY_HOST"
                | "CLAUDE_CODE_ENABLE_GATEWAY_MODEL_DISCOVERY"
                | "CLAUDE_CODE_GATEWAY_MODEL_DISCOVERY_TIMEOUT_MS"
                | "AWS_BEARER_TOKEN_BEDROCK"
                | "AWS_ENDPOINT_URL_BEDROCK"
                | "AWS_ENDPOINT_URL_BEDROCK_RUNTIME"
        )
}
fn native_environment(command: &mut Command) {
    for (key, _) in std::env::vars_os() {
        if conflicting_native_variable(&key.to_string_lossy()) {
            command.env_remove(key);
        }
    }
}

pub async fn run(
    input: RunInput,
    events: Sender<ProviderEvent>,
    mut controls: Receiver<ProviderControl>,
) -> Result<()> {
    let mut content = vec![json!({"type":"text","text":prompt_with_files(&input)})];
    for a in input.attachments.iter().filter(|a| a.kind == "image") {
        let file = tokio::fs::File::open(&a.path).await?;
        let mut bytes = vec![];
        file.take(MAX_ATTACHMENT_BYTES + 1)
            .read_to_end(&mut bytes)
            .await?;
        if bytes.len() as u64 > MAX_ATTACHMENT_BYTES {
            bail!(
                "Image exceeds {MAX_ATTACHMENT_BYTES}-byte limit: {}",
                a.path
            );
        }
        content.push(json!({"type":"image","source":{"type":"base64","media_type":a.mime,"data":base64::engine::general_purpose::STANDARD.encode(&bytes)}}));
    }
    let executable = crate::claude_executable();
    let auth_mode = input.claude_auth_mode.unwrap_or_default();
    if input.read_only || auth_mode == ClaudeAuthMode::Native {
        let mut probe_command = Command::new(&executable);
        if auth_mode == ClaudeAuthMode::Native {
            native_environment(&mut probe_command);
        }
        let mut probe = probe_command
            .arg("--help")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true)
            .spawn()?;
        let mut bytes = Vec::new();
        let checked=tokio::time::timeout(std::time::Duration::from_secs(5),async {
            probe.stdout.take().context("Missing help output")?.take(128*1024+1).read_to_end(&mut bytes).await?;
            if bytes.len()>128*1024 {bail!("Claude help output exceeds limit");}
            let help=String::from_utf8_lossy(&bytes);
            if input.read_only && !["--safe-mode","--tools","--disallowedTools","--permission-mode"].iter().all(|flag|help.contains(flag)){bail!("Installed Claude Code cannot enforce Studio read-only execution; update the native CLI");}
            if auth_mode==ClaudeAuthMode::Native && !help.contains("--setting-sources") {bail!("Installed Claude Code cannot isolate native login from configured settings; --setting-sources is required. Update the CLI or explicitly select configured authentication.");}
            if !probe.wait().await?.success(){bail!("Claude capability probe failed");}
            Ok::<_,anyhow::Error>(())
        }).await;
        let _ = probe.kill().await;
        checked.context("Claude capability probe timed out")??;
    }
    let mut command = Command::new(executable);
    if auth_mode == ClaudeAuthMode::Native {
        native_environment(&mut command);
        command.args(["--setting-sources", ""]);
    }
    command.current_dir(&input.workspace).args([
        "--print",
        "--verbose",
        "--input-format",
        "stream-json",
        "--output-format",
        "stream-json",
        "--include-partial-messages",
        "--forward-subagent-text",
        "--permission-prompts",
        "none",
    ]);
    if let Some(role) = input
        .role
        .as_ref()
        .filter(|_| input.instructions.is_none() && !input.read_only)
    {
        command.arg("--agent").arg(role);
    }
    if let Some(model) = &input.model {
        command.arg("--model").arg(model);
    }
    if let Some(effort) = &input.effort {
        command.arg("--effort").arg(effort);
        command.env_remove("CLAUDE_CODE_EFFORT_LEVEL");
    }
    if let Some(instructions) = &input.instructions {
        command.arg("--append-system-prompt").arg(instructions);
    }
    if input.instructions.is_some() && !input.read_only {
        command.args(["--disallowedTools", "Agent,Task"]);
    }
    if input.read_only {
        command.args([
            "--safe-mode",
            "--tools",
            "Read,Grep,Glob",
            "--disallowedTools",
            "mcp__*",
            "--permission-mode",
            "plan",
        ]);
    }
    emit(&events,"execution_config",json!({"model":input.model,"effort":input.effort,"read_only":input.read_only,"claude_auth_mode":auth_mode,"verification":"requested"})).await?;
    if let Some(session) = &input.resume_session {
        command.arg("--resume").arg(session);
    }
    let mut process = Process::spawn(&mut command).map_err(crate::spawn_error)?;
    let result = drive(content, &events, &mut controls, &mut process, auth_mode).await;
    process.finish().await;
    result
}
async fn drive(
    content: Vec<Value>,
    events: &Sender<ProviderEvent>,
    controls: &mut Receiver<ProviderControl>,
    process: &mut Process,
    auth_mode: ClaudeAuthMode,
) -> Result<()> {
    let mut stdin = process.child.stdin.take().context("Claude stdin missing")?;
    let mut stdout = process::reader(&mut process.child)?;
    let mut request = serde_json::to_vec(
        &json!({"type":"user","message":{"role":"user","content":content},"parent_tool_use_id":null}),
    )?;
    request.push(b'\n');
    emit(events,"notice",json!({"text":"Claude interactive approval bridging is unavailable in phase1. Tools requiring approval will be denied by the provider."})).await?;
    emit(events, "status", json!({"status":"running"})).await?;
    stdin.write_all(&request).await?;
    stdin.flush().await?;
    drop(request);
    let mut session: Option<String> = None;
    let mut last_message: Option<String> = None;
    let mut last_error: Option<String> = None;
    let mut spawners: HashMap<String, String> = HashMap::new();
    let startup = tokio::time::sleep(std::time::Duration::from_secs(45));
    tokio::pin!(startup);
    loop {
        tokio::select! {
            _=&mut startup, if session.is_none()=>bail!("Claude startup did not produce a session within 45 seconds"),
            control=controls.recv()=>match control {
                Some(ProviderControl::Stop)|None=> {
                    // Gracefully interrupt the active turn on POSIX, then Process::finish bounds cleanup.
                    #[cfg(unix)]
                    if let Some(pid)=process.child.id() {unsafe {libc::kill(-(pid as i32),libc::SIGINT);}}
                    emit(events,"status",json!({"status":"cancelled"})).await?;
                    return Ok(());
                }
                Some(ProviderControl::Approve{..})=> {emit(events,"notice",json!({"text":"Claude approval control is not supported; no permission was granted."})).await?;}
            },
            next=process::line(&mut stdout)=> {
                let Some(line)=next? else {
                    let status=process.child.wait().await?;
                    let stderr=tokio::time::timeout(std::time::Duration::from_millis(200),&mut process.stderr).await.ok().and_then(|x|x.ok()).unwrap_or_default();
                    bail!("Claude CLI exited before a result ({status}): {stderr}");
                };
                if line.trim().is_empty() {continue;}
                let raw:Value=serde_json::from_str(&line).context("Invalid JSON from Claude CLI")?;
                if let Some(id)=raw["session_id"].as_str() {
                    if session.as_deref()!=Some(id) {
                        session=Some(id.to_owned());
                        emit(events,"session",json!({"session_id":id,"raw":raw})).await?;
                    }
                }
                if raw["type"]=="system" && raw["model"].is_string() {
                    emit(events,"execution_config",json!({"model":raw["model"],"effort":raw["effort"],"claude_auth_mode":auth_mode,"verification":"runtime_reported"})).await?;
                }
                let typ=raw["type"].as_str().unwrap_or("unknown");
                let (agent,parent)=identity(&raw, session.as_deref(), &mut spawners);
                if typ=="assistant" && raw["parent_tool_use_id"].is_null() {
                    if let Some(blocks)=raw.pointer("/message/content").and_then(Value::as_array) {
                        let text=blocks.iter().filter_map(|b|if b["type"]=="text"{b["text"].as_str()}else{None}).collect::<Vec<_>>().join("\n");
                        if !text.is_empty(){last_message=Some(text);}
                    }
                    if let Some(error)=crate::diagnostic(&raw["error"]) {last_error=Some(error);}
                }
                if typ=="error" {last_error=crate::diagnostic(&raw).or(last_error);}
                if typ=="result" {
                    if raw["permission_denials"].as_array().is_some_and(|x|!x.is_empty()) {
                        emit(events,"permission_denied",json!({"text":"Claude denied one or more protected tools.","raw":raw})).await?;
                    }
                    let text=raw["result"].as_str().filter(|s|!s.is_empty()).map(str::to_owned).or(last_message);
                    if let Some(text)=text.as_ref(){events.send(ProviderEvent {agent_id:agent,parent_agent_id:parent,kind:"message".into(),payload:json!({"text":text,"raw":raw})}).await?;}
                    let failed=raw["is_error"].as_bool().unwrap_or(false) || raw["subtype"].as_str().is_some_and(|s|s.starts_with("error"));
                    let mut payload=json!({"status":if failed {"failed"}else{"completed"},"raw":raw});
                    if failed {
                        payload["error"]=json!(crate::diagnostic(&raw["errors"]).or_else(||crate::diagnostic(&raw["error"])).or_else(||crate::diagnostic(&raw["message"])).or(last_error).or(text).unwrap_or_else(||format!("Claude failed ({}) without a diagnostic",raw["subtype"].as_str().unwrap_or("unknown error"))));
                    }
                    emit(events,"status",payload).await?;
                    return Ok(());
                }
                if typ=="system" && raw["subtype"]=="permission_denied" {
                    events.send(ProviderEvent {agent_id:agent,parent_agent_id:parent,kind:"permission_denied".into(),payload:json!({"text":"Claude denied a protected tool.","raw":raw})}).await?;
                    continue;
                }
                let text=if typ=="stream_event" {raw.pointer("/event/delta/text").cloned().unwrap_or(Value::Null)} else {Value::Null};
                let kind=if text.is_string(){"text"}else{"activity"};
                events.send(ProviderEvent {agent_id:agent,parent_agent_id:parent,kind:kind.into(),payload:json!({"text":text,"message_type":typ,"raw":raw})}).await?;
            }
        }
    }
}

// parent_tool_use_id identifies a tool call, not an agent. Resolve it only through
// an observed spawn emitted by a known agent; otherwise preserve unknown identity.
fn identity(
    raw: &Value,
    session: Option<&str>,
    spawners: &mut HashMap<String, String>,
) -> (Option<String>, Option<String>) {
    let parent_call = raw["parent_tool_use_id"].as_str();
    let agent = raw["agent_id"].as_str().map(str::to_owned).or_else(|| {
        if parent_call.is_none() {
            session.map(str::to_owned)
        } else {
            None
        }
    });
    let parent = parent_call.and_then(|id| spawners.get(id)).cloned();
    if let Some(owner) = &agent {
        let blocks = raw.pointer("/message/content").and_then(Value::as_array);
        let streamed = raw.pointer("/event/content_block");
        for block in blocks.into_iter().flatten().chain(streamed) {
            if block["type"] == "tool_use"
                && matches!(block["name"].as_str(), Some("Agent" | "Task"))
            {
                if let Some(id) = block["id"].as_str() {
                    // Bound identity metadata even for unusually long nested sessions.
                    if spawners.len() < 4096 {
                        spawners.insert(id.to_owned(), owner.clone());
                    }
                }
            }
        }
    }
    (agent, parent)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parent_tool_calls_resolve_only_to_observed_agents() {
        let mut map = HashMap::new();
        let child = json!({"agent_id":"child-1","parent_tool_use_id":"call-1"});
        assert_eq!(
            identity(&child, Some("session-root"), &mut map),
            (Some("child-1".into()), None)
        );
        let root = json!({"type":"assistant","message":{"content":[{"type":"tool_use","name":"Agent","id":"call-1"}]}});
        identity(&root, Some("session-root"), &mut map);
        assert_eq!(
            identity(&child, Some("session-root"), &mut map),
            (Some("child-1".into()), Some("session-root".into()))
        );
        let nested_spawn = json!({"agent_id":"child-1","parent_tool_use_id":"call-1","message":{"content":[{"type":"tool_use","name":"Task","id":"call-2"}]}});
        identity(&nested_spawn, Some("session-root"), &mut map);
        let nested = json!({"agent_id":"child-2","parent_tool_use_id":"call-2"});
        assert_eq!(
            identity(&nested, Some("session-root"), &mut map),
            (Some("child-2".into()), Some("child-1".into()))
        );
    }
    #[test]
    fn unknown_child_never_inherits_root_identity() {
        let mut map = HashMap::new();
        assert_eq!(
            identity(
                &json!({"parent_tool_use_id":"unobserved"}),
                Some("root"),
                &mut map
            ),
            (None, None)
        );
    }
}
