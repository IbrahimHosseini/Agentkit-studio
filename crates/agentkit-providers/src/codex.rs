use crate::{
    emit,
    process::{self, Process},
    prompt_with_files,
};
use agentkit_protocol::{ProviderControl, ProviderEvent, RunInput};
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use tokio::{
    io::AsyncWriteExt,
    process::Command,
    sync::mpsc::{Receiver, Sender},
};

pub async fn run(
    input: RunInput,
    events: Sender<ProviderEvent>,
    mut controls: Receiver<ProviderControl>,
) -> Result<()> {
    let executable = crate::codex_executable();
    let mut command = Command::new(executable);
    command.arg("app-server").current_dir(&input.workspace);
    let mut child = Process::spawn(&mut command).map_err(crate::spawn_error)?;
    let result = drive(&input, &events, &mut controls, &mut child).await;
    child.finish().await;
    result
}
async fn write(stdin: &mut tokio::process::ChildStdin, message: Value) -> Result<()> {
    let mut bytes = serde_json::to_vec(&message)?;
    bytes.push(b'\n');
    stdin.write_all(&bytes).await?;
    stdin.flush().await?;
    Ok(())
}
async fn drive(
    input: &RunInput,
    events: &Sender<ProviderEvent>,
    controls: &mut Receiver<ProviderControl>,
    process: &mut Process,
) -> Result<()> {
    let mut stdin = process.child.stdin.take().context("Codex stdin missing")?;
    let mut stdout = process::reader(&mut process.child)?;
    let mut thread: Option<String> = None;
    let mut turn: Option<String> = None;
    let mut pending: HashMap<String, Value> = HashMap::new();
    let mut readonly_config = json!({});
    let mut turn_submitted = false;
    let mut execution_observed = false;
    let mut completed_messages = HashSet::new();
    let mut last_error: Option<String> = None;
    emit(events, "status", json!({"status":"running"})).await?;
    write(&mut stdin,json!({"id":1,"method":"initialize","params":{"clientInfo":{"name":"agentkit","title":"Agentkit","version":"0.1.0"}}})).await?;
    let startup = tokio::time::sleep(std::time::Duration::from_secs(45));
    tokio::pin!(startup);
    loop {
        tokio::select! {
            _=&mut startup, if turn.is_none()=>bail!("Codex startup did not complete within 45 seconds"),
            control=controls.recv()=>match control {
                Some(ProviderControl::Stop)|None=> {
                    if let (Some(t),Some(u))=(&thread,&turn) { let _=write(&mut stdin,json!({"id":"interrupt","method":"turn/interrupt","params":{"threadId":t,"turnId":u}})).await; }
                    emit(events,"status",json!({"status":"cancelled"})).await?; return Ok(());
                }
                Some(ProviderControl::Approve{request_id,decision})=> {
                    let id=pending.remove(&request_id).context("Approval request is no longer pending")?;
                    write(&mut stdin,json!({"id":id,"result":{"decision":if decision {"accept"} else {"decline"}}})).await?;
                    emit(events,"approval_resolved",json!({"request_id":request_id,"decision":decision})).await?;
                    if pending.is_empty() {emit(events,"status",json!({"status":"running"})).await?;}
                }
            },
            next=process::line(&mut stdout)=> {
                let Some(line)=next? else {
                    let status=process.child.wait().await?;
                    let stderr=tokio::time::timeout(std::time::Duration::from_millis(200),&mut process.stderr).await.ok().and_then(|x|x.ok()).unwrap_or_default();
                    bail!("Codex app-server exited before turn completion ({status}): {stderr}");
                };
                if line.trim().is_empty() {continue;}
                let raw:Value=serde_json::from_str(&line).context("Invalid JSON from Codex app-server")?;
                if raw["method"].as_str().is_some_and(|m|m=="turn/started"||m.starts_with("item/")){execution_observed=true;}
                if let Some(error)=raw.get("error") {if matches!(raw["id"].as_u64(),Some(1|2|4)) {if let Some(classified)=crate::pre_turn_error(error,turn_submitted||execution_observed){return Err(classified);}}bail!("Codex protocol error: {error}");}
                if raw.get("id")==Some(&json!(1)) && raw.get("result").is_some() {
                    write(&mut stdin,json!({"method":"initialized"})).await?;
                    if input.read_only {
                        write(&mut stdin,json!({"id":4,"method":"config/read","params":{"includeLayers":false}})).await?;
                        continue;
                    }
                }
                if raw.get("id")==Some(&json!(4)) && raw.get("result").is_some() {
                    let config=raw.pointer("/result/config").context("Cannot verify read-only runtime configuration")?;
                    if let Some(servers)=config.get("mcp_servers").and_then(Value::as_object) {
                        // Dotted override keys do not parse TOML quotes. A nested map
                        // preserves names containing dots/quotes and the native transport.
                        readonly_config["mcp_servers"]=json!(servers.iter().map(|(name, _)| (name.clone(), json!({"enabled":false}))).collect::<serde_json::Map<String, Value>>());
                    }
                    readonly_config["features.apps"]=json!(false);
                    readonly_config["features.multi_agent"]=json!(false);
                }
                if (raw.get("id")==Some(&json!(1)) && !input.read_only || raw.get("id")==Some(&json!(4)) && input.read_only) && raw.get("result").is_some() {
                    let (method,mut params)=match &input.resume_session {
                        Some(session)=>("thread/resume",json!({"threadId":session,"cwd":input.workspace})),
                        None=>("thread/start",json!({"cwd":input.workspace})),
                    };
                    if let Some(model)=&input.model { params["model"]=json!(model); }
                    if let Some(instructions)=&input.instructions { params["developerInstructions"]=json!(instructions); params["config"]=json!({"features.multi_agent":false}); }
                    if input.read_only { params["sandbox"]=json!("read-only"); params["approvalPolicy"]=json!("never"); params["config"]=readonly_config.clone(); }
                    if let Some(effort)=&input.effort { if params["config"].is_null(){params["config"]=json!({});} params["config"]["model_reasoning_effort"]=json!(effort); }
                    write(&mut stdin,json!({"id":2,"method":method,"params":params})).await?;
                    continue;
                }
                if raw.get("id")==Some(&json!(2)) && raw.get("result").is_some() {
                    let id=raw.pointer("/result/thread/id").and_then(Value::as_str).context("Codex thread response missing ID")?.to_owned();
                    emit(events,"session",json!({"session_id":id,"raw":raw})).await?;
                    thread=Some(id.clone());
                    if raw.pointer("/result/model").is_some() {
                        emit(events,"execution_config",json!({"model":raw["result"]["model"],"effort":raw["result"]["reasoningEffort"],"verification":"runtime_reported"})).await?;
                    }
                    let mut prompt=prompt_with_files(input);
                    if let Some(role)=input.role.as_ref().filter(|_|input.instructions.is_none()) {
                        prompt=format!("Use the installed agent role {role:?} for this task through native agent delegation. If this role is unavailable, explicitly report that limitation.\n\n{prompt}");
                        emit(events,"notice",json!({"text":"Selected role requested through Codex native delegation; inspect actual child-agent events to confirm availability.","role":role})).await?;
                    }
                    let mut content=vec![json!({"type":"text","text":prompt})];
                    for a in input.attachments.iter().filter(|a|a.kind=="image") {content.push(json!({"type":"localImage","path":a.path}));}
                    let mut params=json!({"threadId":id,"input":content});
                    if let Some(model)=&input.model { params["model"]=json!(model); }
                    if let Some(effort)=&input.effort { params["effort"]=json!(effort); }
                    if input.read_only { params["sandboxPolicy"]=json!({"type":"readOnly"}); params["approvalPolicy"]=json!("never"); }
                    emit(events,"execution_config",json!({"model":input.model,"effort":input.effort,"read_only":input.read_only,"verification":"requested"})).await?;
                    turn_submitted=true; // Set BEFORE write: even a partial write makes execution ambiguous.
                    write(&mut stdin,json!({"id":3,"method":"turn/start","params":params})).await?;
                    continue;
                }
                if raw.get("id")==Some(&json!(3)) && raw.get("result").is_some() {
                    turn=raw.pointer("/result/turn/id").and_then(Value::as_str).map(str::to_owned);
                    continue;
                }
                let method=raw["method"].as_str().unwrap_or("");
                let params=&raw["params"];
                if method=="turn/started" {turn=params.pointer("/turn/id").and_then(Value::as_str).map(str::to_owned);}
                if let Some(id)=raw.get("id").filter(|_|!method.is_empty()) {
                    if matches!(method,"item/commandExecution/requestApproval"|"item/fileChange/requestApproval") {
                        if input.read_only {
                            write(&mut stdin,json!({"id":id,"result":{"decision":"decline"}})).await?;
                            emit(events,"permission_denied",json!({"text":"Read-only execution cannot grant command or file-change approval."})).await?;
                            continue;
                        }
                        let request_id=serde_json::to_string(id)?;
                        pending.insert(request_id.clone(),id.clone());
                        emit(events,"approval",json!({"request_id":request_id,"reason":params["reason"],"command":params["command"],"method":method,"raw":raw})).await?;
                        emit(events,"status",json!({"status":"awaiting_approval"})).await?;
                    } else {
                        // Do not silently grant permission expansion, user answers or unknown host tools.
                        write(&mut stdin,json!({"id":id,"error":{"code":-32601,"message":"Agentkit phase1 does not support this interactive request"}})).await?;
                        emit(events,"notice",json!({"text":"Unsupported Codex interactive request was declined.","method":method,"raw":raw})).await?;
                    }
                    continue;
                }
                if method=="turn/completed" {
                    // Some runtime versions only return final items in the turn snapshot.
                    if let Some(items)=params.pointer("/turn/items").and_then(Value::as_array) {
                        for item in items {emit_final_message(item,params["threadId"].as_str().or(thread.as_deref()),events,&mut completed_messages).await?;}
                    }
                    let status=match params.pointer("/turn/status").and_then(Value::as_str) {Some("completed")=>"completed",Some("interrupted")=>"interrupted",_=>"failed"};
                    let mut payload=json!({"status":status,"raw":raw});
                    if status=="failed" {
                        payload["error"]=json!(crate::diagnostic(&params["turn"]["error"]).or(last_error).unwrap_or_else(||format!("Codex turn failed ({}) without a diagnostic",params["turn"]["status"].as_str().unwrap_or("unknown status"))));
                    }
                    emit(events,"status",payload).await?;
                    return Ok(());
                }
                if method=="error" {
                    let error=crate::diagnostic(params).unwrap_or_else(||"Codex reported an error without a diagnostic".into());
                    last_error=Some(error.clone());
                    emit(events,"error",json!({"text":error,"error":error,"raw":raw})).await?;
                    continue;
                }
                if method=="item/completed" && params.pointer("/item/type").and_then(Value::as_str)==Some("agentMessage") {
                    emit_final_message(&params["item"],params["threadId"].as_str().or(thread.as_deref()),events,&mut completed_messages).await?;
                    continue;
                }
                let kind=if method=="item/agentMessage/delta" {"text"} else if method=="item/commandExecution/outputDelta" {"output"} else {"activity"};
                let agent_id=params["threadId"].as_str().map(str::to_owned).or_else(||thread.clone());
                let event=ProviderEvent {agent_id,parent_agent_id:None,kind:kind.into(),payload:json!({"text":params["delta"],"method":method,"raw":raw})};
                // Empty acknowledgements are not user activity.
                if !method.is_empty() {events.send(event).await?;}
                if matches!(method,"item/started"|"item/completed") && params.pointer("/item/type").and_then(Value::as_str)==Some("collabToolCall") {
                    let item=&params["item"];
                    if let Some(child)=item["newThreadId"].as_str().or_else(||item["receiverThreadId"].as_str()) {
                        events.send(ProviderEvent {agent_id:Some(child.into()),parent_agent_id:item["senderThreadId"].as_str().map(str::to_owned),kind:"agent".into(),payload:json!({"status":item["agentStatus"],"tool":item["tool"],"raw":raw})}).await?;
                    }
                }
            }
        }
    }
}

/// A canonical message replaces streamed deltas in Studio's task result. Keep the
/// final snapshot even if no deltas were sent, and avoid repeated turn snapshots.
async fn emit_final_message(
    item: &Value,
    agent: Option<&str>,
    events: &Sender<ProviderEvent>,
    seen: &mut HashSet<String>,
) -> Result<()> {
    if item["type"] != "agentMessage" {
        return Ok(());
    }
    let text = item["text"].as_str().map(str::to_owned).or_else(|| {
        item["content"].as_array().map(|blocks| {
            blocks
                .iter()
                .filter_map(|b| b["text"].as_str())
                .collect::<Vec<_>>()
                .join("\n")
        })
    });
    let Some(text) = text.filter(|s| !s.is_empty()) else {
        return Ok(());
    };
    use std::hash::{Hash, Hasher};
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut hash);
    let key = format!(
        "{}:{}",
        agent.unwrap_or(""),
        item["id"]
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| hash.finish().to_string())
    );
    if seen.contains(&key) {
        return Ok(());
    }
    if seen.len() >= 4096 {
        bail!("Codex final message count exceeds the per-turn limit");
    }
    seen.insert(key);
    events
        .send(ProviderEvent {
            agent_id: agent.map(str::to_owned),
            parent_agent_id: None,
            kind: "message".into(),
            payload: json!({"text":text,"item_id":item["id"],"phase":item["phase"]}),
        })
        .await?;
    Ok(())
}
