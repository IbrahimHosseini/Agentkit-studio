use crate::{
    paths,
    store::{self, Store},
};
use agentkit_protocol::{
    ProviderControl, ProviderEvent, Request, Response, RpcError, RunInput, MAX_ATTACHMENTS,
    MAX_ATTACHMENTS_BYTES, MAX_ATTACHMENT_BYTES, PROTOCOL_VERSION,
};
use anyhow::{bail, Context, Result};
use fs2::FileExt;
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    fs::{self, OpenOptions},
    sync::{Arc, Mutex},
};
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader},
    sync::{mpsc, Semaphore},
};

pub(crate) struct Active {
    pub(crate) controls: mpsc::Sender<ProviderControl>,
    pub(crate) approvals: HashSet<String>,
    pub(crate) stopping: bool,
    abort: Option<tokio::task::AbortHandle>,
    pub(crate) workspace: String,
    pub(crate) read_only: bool,
}
pub(crate) struct Engine {
    pub(crate) store: Mutex<Store>,
    pub(crate) active: Mutex<HashMap<String, Active>>,
    pub(crate) instance: String,
    pub(crate) root: String,
    pub(crate) token: String,
    pub(crate) shutdown: tokio::sync::Notify,
    pub(crate) last_activity: Mutex<std::time::Instant>,
    pub(crate) admission: Mutex<()>,
    pub(crate) shutting_down: std::sync::atomic::AtomicBool,
    pub(crate) scheduling: Mutex<()>,
    pub(crate) last_conversation: Mutex<String>,
    pub(crate) baselines: Mutex<HashMap<String, Value>>,
}
pub(crate) fn required<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .with_context(|| format!("Missing string parameter: {key}"))
}
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> Result<std::sync::MutexGuard<'_, T>> {
    mutex
        .lock()
        .map_err(|_| anyhow::anyhow!("Engine state lock poisoned"))
}
pub(crate) fn validate(mut input: RunInput) -> Result<RunInput> {
    if !matches!(input.provider.as_str(), "codex" | "claude" | "mock") {
        bail!("Unknown provider; expected codex, claude or mock");
    }
    let prompt_limit = if input.instructions.is_some() {
        262144
    } else {
        65536
    };
    if input.prompt.trim().is_empty() || input.prompt.len() > prompt_limit {
        bail!("Prompt must contain 1–{prompt_limit} bytes");
    }
    let workspace =
        fs::canonicalize(&input.workspace).context("Project directory does not exist")?;
    if !workspace.is_dir() {
        bail!("Project must be a directory");
    }
    input.workspace = workspace.to_string_lossy().into_owned();
    if let Some(role) = &input.role {
        if role.is_empty()
            || role.len() > 80
            || !role
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | ':'))
        {
            bail!("Invalid role name");
        }
    }
    if input.attachments.len() > MAX_ATTACHMENTS {
        bail!("At most 12 attachments are accepted");
    }
    let mut size = 0u64;
    for attachment in &mut input.attachments {
        if !matches!(attachment.kind.as_str(), "image" | "file") {
            bail!("Attachment kind must be image or file");
        }
        let path = fs::canonicalize(&attachment.path).context("Attachment does not exist")?;
        let meta = fs::metadata(&path)?;
        if !meta.is_file() {
            bail!("Attachment must be a regular file");
        }
        size += meta.len();
        if meta.len() > MAX_ATTACHMENT_BYTES || size > MAX_ATTACHMENTS_BYTES {
            bail!("Attachment limit is 16 MiB per file and 40 MiB per request");
        }
        if attachment.kind == "image"
            && !matches!(
                attachment.mime.as_str(),
                "image/png" | "image/jpeg" | "image/webp" | "image/gif"
            )
        {
            bail!("Unsupported image MIME type");
        }
        if attachment.mime.is_empty()
            || attachment.mime.len() > 128
            || attachment.mime.chars().any(char::is_control)
        {
            bail!("Invalid attachment MIME type");
        }
        attachment.path = path.to_string_lossy().into_owned();
    }
    Ok(input)
}
// Every execution path (legacy start, resume and managed tasks) passes through
// start_managed. The current Studio setting wins over saved or caller input.
fn apply_auth_mode(input: &mut RunInput, settings: &Value) -> Result<()> {
    if input.provider != "claude" {
        input.claude_auth_mode = None;
        return Ok(());
    }
    let mode: agentkit_protocol::ClaudeAuthMode = serde_json::from_value(
        settings
            .get("claude_auth_mode")
            .cloned()
            .unwrap_or(json!("configured")),
    )
    .context("Invalid Studio Claude authentication mode")?;
    if input.resume_session.is_some() && input.claude_auth_mode.unwrap_or_default() != mode {
        bail!("Cannot resume a Claude session after its authentication mode changed. Start a new run with the selected mode.");
    }
    input.claude_auth_mode = Some(mode);
    Ok(())
}

impl Engine {
    async fn dispatch(self: &Arc<Self>, request: Request) -> Result<Value> {
        if request.params.get("_auth").and_then(Value::as_str) != Some(self.token.as_str()) {
            bail!("Local authentication failed");
        }
        *lock(&self.last_activity)? = std::time::Instant::now();
        let p = &request.params;
        if request.method.starts_with("studio/") {
            return self.studio_dispatch(&request.method, p).await;
        }
        match request.method.as_str() {
            "handshake" => Ok(
                json!({"protocol_version":PROTOCOL_VERSION,"server_instance_id":self.instance,"engine_version":env!("CARGO_PKG_VERSION"),"data_root":self.root,"capabilities":[agentkit_providers::capabilities("codex"),agentkit_providers::capabilities("claude"),agentkit_providers::capabilities("mock")]}),
            ),
            "shutdown" => {
                if !lock(&self.active)?.is_empty() || self.studio_busy()? {
                    bail!("Cannot shut down an engine with active runs");
                }
                Ok(json!({"stopping":true}))
            }
            "metrics" => {
                crate::metrics::sample(
                    p.get("gui_pid")
                        .and_then(Value::as_u64)
                        .map(|pid| u32::try_from(pid))
                        .transpose()
                        .context("gui_pid exceeds u32")?,
                )
                .await
            }
            "start" => {
                let run = self.start(serde_json::from_value(p.clone())?).await?;
                lock(&self.store)?.import_run(required(&run, "id")?)?;
                Ok(run)
            }
            "list" => Ok(serde_json::to_value(
                lock(&self.store)?.list(
                    p.get("limit")
                        .and_then(Value::as_u64)
                        .unwrap_or(100)
                        .min(500) as usize,
                )?,
            )?),
            "get" => Ok(serde_json::to_value(
                lock(&self.store)?.get(required(p, "run_id")?)?,
            )?),
            "subscribe" => {
                let after = p.get("after_seq").and_then(Value::as_u64).unwrap_or(0);
                // A response is bounded both by row count and the transport's byte budget.
                let events = lock(&self.store)?.events(
                    required(p, "run_id")?,
                    after,
                    p.get("limit")
                        .and_then(Value::as_u64)
                        .unwrap_or(50)
                        .min(100) as usize,
                )?;
                let mut selected = Vec::new();
                let mut bytes = 0usize;
                for event in events {
                    let len = serde_json::to_vec(&event)?.len();
                    if bytes + len > 1024 * 1024 {
                        break;
                    }
                    bytes += len;
                    selected.push(event);
                }
                let next = selected.last().map(|e| e.seq).unwrap_or(after);
                Ok(json!({"events":selected,"next_seq":next}))
            }
            "stop" => {
                let id = required(p, "run_id")?;
                let run = lock(&self.store)?.get(id)?;
                if store::terminal(&run.status) {
                    return Ok(serde_json::to_value(run)?);
                }
                let controls = {
                    let mut active = lock(&self.active)?;
                    let run = active.get_mut(id).context("Run is not active")?;
                    run.stopping = true;
                    run.controls.clone()
                };
                lock(&self.store)?.event(
                    id,
                    ProviderEvent::new("status", json!({"status":"stopping"})),
                )?;
                controls
                    .send(ProviderControl::Stop)
                    .await
                    .context("Provider control channel is closed")?;
                Ok(serde_json::to_value(lock(&self.store)?.get(id)?)?)
            }
            "approve" => {
                let id = required(p, "run_id")?;
                let request_id = required(p, "request_id")?;
                let decision = p
                    .get("decision")
                    .and_then(Value::as_bool)
                    .context("Missing boolean decision")?;
                let controls = {
                    let mut active = lock(&self.active)?;
                    let run = active.get_mut(id).context("Run is not active")?;
                    if !run.approvals.remove(request_id) {
                        bail!("Approval is absent or already resolved");
                    }
                    run.controls.clone()
                };
                controls
                    .send(ProviderControl::Approve {
                        request_id: request_id.into(),
                        decision,
                    })
                    .await?;
                Ok(json!({"accepted":true}))
            }
            "resume" => {
                let id = required(p, "run_id")?;
                let mut input = {
                    let store = lock(&self.store)?;
                    let run = store.get(id)?;
                    if !store::terminal(&run.status) {
                        bail!("Only stopped runs can be resumed");
                    }
                    if !agentkit_providers::capabilities(&run.provider).resume {
                        bail!("Provider does not support resume");
                    }
                    let mut input = store.input(id)?;
                    input.resume_session = Some(
                        run.provider_session_id
                            .context("No provider session was recorded")?,
                    );
                    input
                };
                if let Some(prompt) = p.get("prompt").and_then(Value::as_str) {
                    input.prompt = prompt.into();
                } else {
                    input.prompt = "Continue the previous task.".into();
                }
                let run = self.start(input).await?;
                lock(&self.store)?.import_run(required(&run, "id")?)?;
                Ok(run)
            }
            _ => bail!("Unknown engine method: {}", request.method),
        }
    }
    pub(crate) async fn start(self: &Arc<Self>, input: RunInput) -> Result<Value> {
        self.start_managed(input, None).await
    }
    pub(crate) async fn start_managed(
        self: &Arc<Self>,
        input: RunInput,
        task_id: Option<&str>,
    ) -> Result<Value> {
        let mut input = validate(input)?;
        let _admission = lock(&self.admission)?;
        if self.shutting_down.load(std::sync::atomic::Ordering::SeqCst) {
            bail!("Engine is shutting down");
        }
        let settings = lock(&self.store)?.settings()?;
        if !crate::scheduler::provider_enabled(&settings, &input.provider) { bail!("{} is disconnected in Studio. Connect it in Connections before starting a request.", input.provider); }
        apply_auth_mode(&mut input, &settings)?;
        let maximum = settings["concurrency"].as_u64().unwrap_or(4) as usize;
        {
            let active = lock(&self.active)?;
            if active.len() >= maximum {
                bail!("Execution capacity is full; wait for an active run");
            }
            if !input.read_only
                && active
                    .values()
                    .any(|a| !a.read_only && a.workspace == input.workspace)
            {
                bail!("Project already has an active writer");
            }
        }
        let run = {
            let mut store = lock(&self.store)?;
            let mut managed = task_id.map(|id| store.entity("task", id)).transpose()?;
            if managed.as_ref().is_some_and(|t| t["status"] != "starting") {
                bail!("Task was cancelled before execution");
            }
            if let Some(task) = managed.as_ref() {
                crate::scheduler::check_task_policy(&store, task, &input.workspace)?;
            }
            let run = store.create(&input)?;
            if let Some(task) = managed.as_mut() {
                task["run_id"] = json!(run.id);
                task["status"] = json!("running");
                if !task["attempts"].is_array() {
                    task["attempts"] = json!([]);
                }
                let attempt = json!({"number":task["attempts"].as_array().unwrap().len()+1,"run_id":run.id,"model":input.model,"effort":input.effort,"status":"running","started_at":store::now(),"error":null,"retry_evidence":null,"claude_auth_mode":input.claude_auth_mode});
                task["attempts"].as_array_mut().unwrap().push(attempt);
                task["retry_at"] = Value::Null;
                store.put("task", task["conversation_id"].as_str().unwrap_or(""), task)?;
            }
            run
        };
        let id = run.id.clone();
        let (events_tx, mut events_rx) = mpsc::channel(128);
        let (controls_tx, controls_rx) = mpsc::channel(16);
        lock(&self.active)?.insert(
            id.clone(),
            Active {
                controls: controls_tx,
                approvals: HashSet::new(),
                stopping: false,
                abort: None,
                workspace: input.workspace.clone(),
                read_only: input.read_only,
            },
        );
        let engine = Arc::clone(self);
        tokio::spawn(async move {
            let provider_id = id.clone();
            let task = tokio::spawn(async move {
                agentkit_providers::run(input, provider_id, events_tx, controls_rx).await
            });
            if let Ok(mut active) = engine.active.lock() {
                if let Some(run) = active.get_mut(&id) {
                    run.abort = Some(task.abort_handle());
                }
            }
            let mut terminal_event = None;
            let mut persistence_error = None;
            while let Some(event) = events_rx.recv().await {
                if event.kind == "status" {
                    if let Some(status) = event.payload.get("status").and_then(Value::as_str) {
                        if store::terminal(status) {
                            terminal_event = Some(event);
                            continue;
                        }
                        if engine
                            .active
                            .lock()
                            .ok()
                            .and_then(|active| active.get(&id).map(|run| run.stopping))
                            .unwrap_or(false)
                            && status != "stopping"
                        {
                            continue;
                        }
                    }
                }
                if event.kind == "approval_resolved" {
                    if let Some(request_id) =
                        event.payload.get("request_id").and_then(Value::as_str)
                    {
                        if let Ok(mut active) = engine.active.lock() {
                            if let Some(run) = active.get_mut(&id) {
                                run.approvals.remove(request_id);
                            }
                        }
                    }
                }
                if event.kind == "approval" || event.kind == "approval_request" {
                    if let Some(request_id) =
                        event.payload.get("request_id").and_then(Value::as_str)
                    {
                        if let Ok(mut active) = engine.active.lock() {
                            if let Some(run) = active.get_mut(&id) {
                                run.approvals.insert(request_id.to_owned());
                            }
                        }
                    }
                }
                if let Ok(mut store) = engine.store.lock() {
                    if let Err(err) = store.event(&id, event) {
                        persistence_error =
                            Some(format!("Could not persist provider event: {err}"));
                        if let Ok(active) = engine.active.lock() {
                            if let Some(run) = active.get(&id) {
                                let _ = run.controls.try_send(ProviderControl::Stop);
                            }
                        }
                    }
                }
            }
            let outcome = task.await;
            let stopping = engine
                .active
                .lock()
                .ok()
                .and_then(|active| active.get(&id).map(|run| run.stopping))
                .unwrap_or(false);
            if let Ok(mut store) = engine.store.lock() {
                let final_event = if let Some(error) = persistence_error {
                    ProviderEvent::new("status", json!({"status":"failed","error":error}))
                } else {
                    match outcome {
                        Ok(Ok(())) => terminal_event.unwrap_or_else(|| {
                            ProviderEvent::new(
                                "status",
                                json!({"status":if stopping{"cancelled"}else{"completed"}}),
                            )
                        }),
                        Ok(Err(err)) => ProviderEvent::new(
                            "status",
                            json!({"status":if stopping{"cancelled"}else{"failed"},"error":format!("{err:#}"),"retry":agentkit_providers::retry_evidence(&err)}),
                        ),
                        Err(err) => ProviderEvent::new(
                            "status",
                            json!({"status":"failed","error":err.to_string()}),
                        ),
                    }
                };
                if let Err(err) = store.event(&id, final_event) {
                    eprintln!("Could not persist terminal provider event: {err}");
                }
            }
            if let Ok(mut active) = engine.active.lock() {
                active.remove(&id);
            }
        });
        Ok(serde_json::to_value(run)?)
    }
}
async fn connection<S: AsyncRead + AsyncWrite + Unpin>(
    stream: S,
    engine: Arc<Engine>,
) -> Result<()> {
    let mut reader = BufReader::new(stream).take(2 * 1024 * 1024 + 1);
    let mut line = Vec::new();
    tokio::time::timeout(
        std::time::Duration::from_secs(10),
        reader.read_until(b'\n', &mut line),
    )
    .await??;
    if line.len() > 2 * 1024 * 1024 || !line.ends_with(b"\n") {
        bail!("Invalid or oversized local message");
    }
    let request: Request = serde_json::from_slice(&line)?;
    let id = request.id.clone();
    let shutdown = request.method == "shutdown";
    let response = match engine.dispatch(request).await {
        Ok(result) => Response {
            id,
            result: Some(result),
            error: None,
        },
        Err(err) => Response {
            id,
            result: None,
            error: Some(RpcError {
                code: "request_failed".into(),
                message: format!("{err:#}"),
            }),
        },
    };
    let mut out = serde_json::to_vec(&response)?;
    out.push(b'\n');
    let successful_shutdown = shutdown && response.error.is_none();
    let mut stream = reader.into_inner().into_inner();
    stream.write_all(&out).await?;
    stream.flush().await?;
    if successful_shutdown {
        engine.shutdown.notify_one();
    }
    Ok(())
}
pub async fn serve() -> Result<()> {
    let root = paths::data_root()?;
    let lock_file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(root.join("engine.lock"))?;
    if lock_file.try_lock_exclusive().is_err() {
        return Ok(());
    }
    let token = uuid::Uuid::new_v4().to_string();
    fs::write(root.join("engine.token"), &token)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(root.join("engine.token"), fs::Permissions::from_mode(0o600))?;
    }
    let mut store = Store::open(&root)?;
    store.recover()?;
    let engine = Arc::new(Engine {
        store: Mutex::new(store),
        active: Mutex::new(HashMap::new()),
        instance: uuid::Uuid::new_v4().to_string(),
        root: root.to_string_lossy().into_owned(),
        token,
        shutdown: tokio::sync::Notify::new(),
        last_activity: Mutex::new(std::time::Instant::now()),
        shutting_down: std::sync::atomic::AtomicBool::new(false),
        admission: Mutex::new(()),
        scheduling: Mutex::new(()),
        last_conversation: Mutex::new(String::new()),
        baselines: Mutex::new(HashMap::new()),
    });
    let scheduler = Arc::downgrade(&engine);
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            let Some(engine) = scheduler.upgrade() else {
                break;
            };
            if let Err(error) = engine.schedule().await {
                eprintln!("Studio scheduler: {error:#}");
            }
        }
    });
    let capacity = Arc::new(Semaphore::new(32));
    let termination = termination_signal();
    tokio::pin!(termination);
    #[cfg(unix)]
    {
        let endpoint = paths::endpoint(&root)?;
        if endpoint.exists() {
            fs::remove_file(&endpoint)?;
        }
        let listener = tokio::net::UnixListener::bind(&endpoint)?;
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&endpoint, fs::Permissions::from_mode(0o600))?;
        loop {
            let permit = Arc::clone(&capacity).acquire_owned().await?;
            let accepted = tokio::select! { accepted=listener.accept()=>Some(accepted?), _=engine.shutdown.notified()=>None, _=&mut termination=>None, _=tokio::time::sleep(std::time::Duration::from_secs(5))=>{if lock(&engine.active)?.is_empty()&&!engine.studio_busy()?&&lock(&engine.last_activity)?.elapsed().as_secs()>=60{None}else{continue;}} };
            let Some((stream, _)) = accepted else {
                let _ = fs::remove_file(&endpoint);
                break;
            };
            let engine = Arc::clone(&engine);
            tokio::spawn(async move {
                let _permit = permit;
                if let Err(err) = connection(stream, engine).await {
                    eprintln!("Local connection: {err}");
                }
            });
        }
    }
    #[cfg(windows)]
    {
        use tokio::net::windows::named_pipe::ServerOptions;
        let endpoint = paths::endpoint(&root)?;
        let mut listener = ServerOptions::new()
            .first_pipe_instance(true)
            .reject_remote_clients(true)
            .create(&endpoint)?;
        loop {
            let permit = Arc::clone(&capacity).acquire_owned().await?;
            let connected_ok = tokio::select! { result=listener.connect()=>{result?;true}, _=engine.shutdown.notified()=>false, _=&mut termination=>false, _=tokio::time::sleep(std::time::Duration::from_secs(5))=>{if lock(&engine.active)?.is_empty()&&!engine.studio_busy()?&&lock(&engine.last_activity)?.elapsed().as_secs()>=60{false}else{continue;}}};
            if !connected_ok {
                break;
            }
            let connected = listener;
            listener = ServerOptions::new()
                .reject_remote_clients(true)
                .create(&endpoint)?;
            let engine = Arc::clone(&engine);
            tokio::spawn(async move {
                let _permit = permit;
                if let Err(err) = connection(connected, engine).await {
                    eprintln!("Local connection: {err}");
                }
            });
        }
    }
    engine
        .shutting_down
        .store(true, std::sync::atomic::Ordering::SeqCst);
    stop_all(&engine).await;
    Ok(())
}

async fn termination_signal() {
    #[cfg(unix)]
    {
        if let Ok(mut signal) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            tokio::select! {_=signal.recv()=>{},_=tokio::signal::ctrl_c()=>{}}
        } else {
            let _ = tokio::signal::ctrl_c().await;
        }
    }
    #[cfg(windows)]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}
async fn stop_all(engine: &Arc<Engine>) {
    let controls = if let Ok(mut active) = engine.active.lock() {
        active
            .values_mut()
            .map(|run| {
                run.stopping = true;
                run.controls.clone()
            })
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    for controls in controls {
        let _ = controls.try_send(ProviderControl::Stop);
    }
    for _ in 0..50 {
        if engine
            .active
            .lock()
            .map(|active| active.is_empty())
            .unwrap_or(true)
        {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    if let Ok(active) = engine.active.lock() {
        for run in active.values() {
            if let Some(abort) = &run.abort {
                abort.abort();
            }
        }
    }
    for _ in 0..20 {
        if engine
            .active
            .lock()
            .map(|active| active.is_empty())
            .unwrap_or(true)
        {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn input() -> RunInput {
        RunInput {
            provider: "mock".into(),
            workspace: std::env::temp_dir().to_string_lossy().into_owned(),
            prompt: "فارسی + Rust".into(),
            role: None,
            attachments: vec![],
            resume_session: None,
            model: None,
            effort: None,
            instructions: None,
            read_only: false,
            claude_auth_mode: None,
            permission_mode: Default::default(),
        }
    }
    #[test]
    fn current_auth_mode_is_applied_to_new_and_resumed_inputs() {
        use agentkit_protocol::ClaudeAuthMode;
        let mut run = input();
        run.provider = "claude".into();
        apply_auth_mode(&mut run, &json!({})).unwrap();
        assert_eq!(run.claude_auth_mode, Some(ClaudeAuthMode::Configured));
        apply_auth_mode(&mut run, &json!({"claude_auth_mode":"native"})).unwrap();
        assert_eq!(run.claude_auth_mode, Some(ClaudeAuthMode::Native));
        // Session identifiers must never cross authentication routes silently.
        run.resume_session = Some("session".into());
        assert!(
            apply_auth_mode(&mut run, &json!({"claude_auth_mode":"configured"}))
                .unwrap_err()
                .to_string()
                .contains("Start a new run")
        );
        apply_auth_mode(&mut run, &json!({"claude_auth_mode":"native"})).unwrap();
        assert_eq!(run.claude_auth_mode, Some(ClaudeAuthMode::Native));
        run.resume_session = None;
        apply_auth_mode(&mut run, &json!({"claude_auth_mode":"configured"})).unwrap();
        assert_eq!(run.claude_auth_mode, Some(ClaudeAuthMode::Configured));
        assert!(apply_auth_mode(&mut run, &json!({"claude_auth_mode":"invalid"})).is_err());
        run.provider = "codex".into();
        run.claude_auth_mode = Some(ClaudeAuthMode::Native);
        apply_auth_mode(&mut run, &json!({"claude_auth_mode":"native"})).unwrap();
        assert_eq!(run.claude_auth_mode, None);
    }
    #[test]
    fn validate_rejects_unknown_providers_and_unsafe_roles() {
        let mut value = input();
        value.provider = "missing".into();
        assert!(validate(value).is_err());
        let mut value = input();
        value.role = Some("backend\nignore rules".into());
        assert!(validate(value).is_err());
        assert!(std::path::Path::new(&validate(input()).unwrap().workspace).is_absolute());
    }
    #[tokio::test]
    async fn oversized_approval_keeps_replayed_id_and_can_be_resolved() {
        let root = std::env::temp_dir().join(format!("ak-approval-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let mut store = Store::open(&root).unwrap();
        let run = store.create(&input()).unwrap();
        let request_id = "approval-42-نیم‌فاصله";
        let event = store
            .event(
                &run.id,
                ProviderEvent::new(
                    "approval",
                    json!({
                        "request_id":request_id,"reason":"دلیل طولانی ".repeat(10000),
                        "raw":{"id":42,"detail":"x".repeat(70000)},"command":"read project file",
                    }),
                ),
            )
            .unwrap();
        assert!(serde_json::to_vec(&event.payload).unwrap().len() <= 65_536);
        let replay = store.events(&run.id, 0, 10).unwrap();
        let persisted_id = replay
            .iter()
            .find(|event| event.kind == "approval")
            .unwrap()
            .payload["request_id"]
            .as_str()
            .unwrap()
            .to_owned();
        assert_eq!(persisted_id, request_id);
        let (controls, mut received) = mpsc::channel(1);
        let mut active = HashMap::new();
        active.insert(
            run.id.clone(),
            Active {
                controls,
                approvals: HashSet::from([request_id.into()]),
                stopping: false,
                abort: None,
                workspace: String::new(),
                read_only: false,
            },
        );
        let engine = Arc::new(Engine {
            store: Mutex::new(store),
            active: Mutex::new(active),
            instance: "test".into(),
            root: root.to_string_lossy().into_owned(),
            token: "test-token".into(),
            shutdown: tokio::sync::Notify::new(),
            last_activity: Mutex::new(std::time::Instant::now()),
            shutting_down: std::sync::atomic::AtomicBool::new(false),
            admission: Mutex::new(()),
            scheduling: Mutex::new(()),
            last_conversation: Mutex::new(String::new()),
            baselines: Mutex::new(HashMap::new()),
        });
        let result=engine.dispatch(Request{id:"approve".into(),method:"approve".into(),params:json!({"_auth":"test-token","run_id":run.id,"request_id":persisted_id,"decision":true})}).await.unwrap();
        assert_eq!(result["accepted"], true);
        match received.recv().await.unwrap() {
            ProviderControl::Approve {
                request_id: id,
                decision,
            } => {
                assert_eq!(id, request_id);
                assert!(decision);
            }
            _ => panic!("Expected approval control"),
        }
        drop(engine);
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(test)]
mod admission_tests {
    use super::*;
    #[tokio::test]
    async fn admission_enforces_configured_global_cap_and_single_project_writer() {
        let root = std::env::temp_dir().join(format!("studio-admission-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let root = std::fs::canonicalize(root).unwrap();
        let store = Store::open(&root).unwrap();
        store
            .put("settings", "", &json!({"id":"main","concurrency":1}))
            .unwrap();
        let (controls, _received) = mpsc::channel(1);
        let mut active = HashMap::new();
        active.insert(
            "existing".into(),
            Active {
                controls,
                approvals: HashSet::new(),
                stopping: false,
                abort: None,
                workspace: root.to_string_lossy().into_owned(),
                read_only: false,
            },
        );
        let engine = Arc::new(Engine {
            store: Mutex::new(store),
            active: Mutex::new(active),
            instance: "test".into(),
            root: root.to_string_lossy().into_owned(),
            token: "isolated".into(),
            shutdown: tokio::sync::Notify::new(),
            last_activity: Mutex::new(std::time::Instant::now()),
            admission: Mutex::new(()),
            shutting_down: std::sync::atomic::AtomicBool::new(false),
            scheduling: Mutex::new(()),
            last_conversation: Mutex::new(String::new()),
            baselines: Mutex::new(HashMap::new()),
        });
        let input: RunInput = serde_json::from_value(
            json!({"provider":"mock","workspace":root,"prompt":"Do not start","attachments":[]}),
        )
        .unwrap();
        assert!(engine
            .start(input.clone())
            .await
            .unwrap_err()
            .to_string()
            .contains("capacity"));
        lock(&engine.store)
            .unwrap()
            .put("settings", "", &json!({"id":"main","concurrency":4}))
            .unwrap();
        assert!(engine
            .start(input)
            .await
            .unwrap_err()
            .to_string()
            .contains("active writer"));
        assert!(lock(&engine.store).unwrap().list(10).unwrap().is_empty());
        drop(engine);
        std::fs::remove_dir_all(root).unwrap();
    }
}
