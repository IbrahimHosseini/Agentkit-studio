//! Conversation-scoped development surfaces shared by the desktop and MCP/CLI.
use crate::daemon::{lock, required, Engine};
use anyhow::{bail, Context, Result};
use base64::Engine as _;
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use serde_json::{json, Value};
use std::{
    collections::{HashMap, VecDeque},
    io::{Read, Write},
    path::Path,
    sync::{Arc, Mutex, OnceLock},
};

const OUTPUT_LIMIT: usize = 1024 * 1024;
const READ_LIMIT: usize = 64 * 1024;
static OPERATIONS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
pub(crate) fn has_operations() -> bool { OPERATIONS.load(std::sync::atomic::Ordering::SeqCst) != 0 }
struct Operation;
impl Operation {
    fn start() -> Self { OPERATIONS.fetch_add(1, std::sync::atomic::Ordering::SeqCst); Self }
}
impl Drop for Operation {
    fn drop(&mut self) { OPERATIONS.fetch_sub(1, std::sync::atomic::Ordering::SeqCst); }
}
const MAX_TERMINALS: usize = 16;
const MAX_TOTAL_TERMINALS: usize = 128;

#[derive(Default)]
struct Output {
    bytes: VecDeque<u8>,
    start: u64,
    ended: bool,
}
impl Output {
    fn append(&mut self, bytes: &[u8]) {
        self.bytes.extend(bytes);
        let discard = self.bytes.len().saturating_sub(OUTPUT_LIMIT);
        self.bytes.drain(..discard);
        self.start += discard as u64;
    }
    fn read(&self, after: u64) -> Result<Value> {
        let end = self.start + self.bytes.len() as u64;
        if after > end { bail!("Terminal output cursor is ahead of the stream"); }
        let cursor = after.max(self.start);
        let bytes: Vec<_> = self.bytes.iter().skip((cursor - self.start) as usize).take(READ_LIMIT).copied().collect();
        Ok(json!({"data":base64::engine::general_purpose::STANDARD.encode(&bytes),"text":String::from_utf8_lossy(&bytes),"encoding":"base64","cursor":cursor + bytes.len() as u64,"truncated":after<self.start,"ended":self.ended}))
    }
}
struct Terminal {
    conversation: String,
    workspace: std::path::PathBuf,
    title: String,
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send + Sync>,
    output: Arc<Mutex<Output>>,
    cols: u16,
    rows: u16,
}
impl Terminal {
    fn summary(&mut self, id: &str) -> Result<Value> {
        let exit = self.child.try_wait()?;
        Ok(json!({"id":id,"title":self.title,"cols":self.cols,"rows":self.rows,"exited":exit.is_some(),"exit_code":exit.map(|s|s.exit_code())}))
    }
}
impl Drop for Terminal {
    fn drop(&mut self) {
        // The PTY belongs to this service. Close its foreground command as well as
        // the shell; never signal an unrelated process discovered by name.
        #[cfg(unix)]
        if let Some(group) = self.master.process_group_leader().filter(|p| *p > 1) {
            unsafe { libc::kill(-group, libc::SIGHUP); }
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
fn terminals() -> &'static Mutex<HashMap<String, Terminal>> {
    static TERMINALS: OnceLock<Mutex<HashMap<String, Terminal>>> = OnceLock::new();
    TERMINALS.get_or_init(|| Mutex::new(HashMap::new()))
}
pub(crate) fn has_terminals() -> bool {
    terminals().lock().map(|t| !t.is_empty()).unwrap_or(false)
}
pub(crate) async fn close_conversation(conversation: &str) {
    if let Ok(mut all) = terminals().lock() { all.retain(|_,t|t.conversation!=conversation); }
    crate::devtools_browser::close_conversation(conversation).await;
}
/// Retire only the old workspace after a conversation is moved or detached.
/// Matching the path prevents a concurrent new-workspace terminal being closed.
pub(crate) fn close_workspace(conversation: &str, workspace: &Path) {
    if let Ok(mut all) = terminals().lock() {
        all.retain(|_,t|t.conversation!=conversation || t.workspace!=workspace);
    }
}
pub(crate) fn close_all() {
    if let Ok(mut all) = terminals().lock() { all.clear(); }
}
fn size(params: &Value) -> Result<PtySize> {
    let cols = params.get("cols").map(|v|v.as_u64().context("cols must be an integer")).transpose()?.unwrap_or(100);
    let rows = params.get("rows").map(|v|v.as_u64().context("rows must be an integer")).transpose()?.unwrap_or(30);
    if !(2..=500).contains(&cols) || !(1..=300).contains(&rows) { bail!("Terminal size must be 2–500 columns and 1–300 rows"); }
    Ok(PtySize { rows: rows as u16, cols: cols as u16, pixel_width: 0, pixel_height: 0 })
}
fn terminal_dispatch(action: &str, params: &Value, workspace: &Path) -> Result<Value> {
    let conversation = required(params,"conversation_id")?;
    let mut all = lock(terminals())?;
    all.retain(|_,t|t.conversation!=conversation || t.workspace==workspace);
    match action {
        "create" => {
            if all.len() >= MAX_TOTAL_TERMINALS { bail!("Close a terminal before opening another (application limit 128)"); }
            if all.values().filter(|t|t.conversation == conversation).count() >= MAX_TERMINALS { bail!("Close a terminal before opening another (limit 16)"); }
            let size = size(params)?;
            let pair = native_pty_system().openpty(size).context("Could not open a native terminal")?;
            #[cfg(unix)]
            let shell = std::env::var("SHELL").ok().filter(|p|Path::new(p).is_absolute() && Path::new(p).is_file()).unwrap_or_else(||"/bin/sh".into());
            #[cfg(windows)]
            let shell = std::env::var("COMSPEC").unwrap_or_else(|_|"cmd.exe".into());
            let mut command = CommandBuilder::new(&shell);
            command.cwd(workspace);
            command.env("TERM", "xterm-256color");
            command.env("COLORTERM", "truecolor");
            let child = pair.slave.spawn_command(command).context("Could not start the terminal shell")?;
            drop(pair.slave);
            let mut reader = pair.master.try_clone_reader()?;
            let writer = pair.master.take_writer()?;
            let output = Arc::new(Mutex::new(Output::default()));
            let sink = output.clone();
            std::thread::spawn(move || {
                let mut buf = [0u8;8192];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => { if let Ok(mut o) = sink.lock() { o.append(&buf[..n]); } else { break; } }
                    }
                }
                if let Ok(mut o) = sink.lock() { o.ended = true; }
            });
            let id = uuid::Uuid::new_v4().to_string();
            let mut terminal = Terminal { conversation:conversation.into(),workspace:workspace.into(),title:Path::new(&shell).file_name().unwrap_or_default().to_string_lossy().into_owned(),master:pair.master,writer,child,output,cols:size.cols,rows:size.rows };
            let summary = terminal.summary(&id)?;
            all.insert(id, terminal);
            Ok(summary)
        }
        "list" | "status" => {
            let mut summaries = vec![];
            for (id,terminal) in all.iter_mut().filter(|(_,t)|t.conversation==conversation) { summaries.push(terminal.summary(id)?); }
            Ok(json!({"terminals":summaries}))
        }
        "close" if params.get("id").is_none() => {
            all.retain(|_,t|t.conversation!=conversation);
            Ok(json!({"closed":true}))
        }
        "read" | "write" | "resize" | "close" => {
            let id = required(params,"id")?;
            let terminal = all.get_mut(id).filter(|t|t.conversation==conversation).context("Terminal not found in this conversation")?;
            match action {
                "read" => {
                    let after = params.get("after").map(|v|v.as_u64().context("after must be a nonnegative integer")).transpose()?.unwrap_or(0);
                    let mut result = lock(&terminal.output)?.read(after)?;
                    let summary = terminal.summary(id)?;
                    result["id"] = json!(id);
                    result["exited"] = summary["exited"].clone();
                    result["exit_code"] = summary["exit_code"].clone();
                    Ok(result)
                }
                "write" => {
                    let data = required(params,"data")?;
                    if data.len()>64*1024 { bail!("Terminal input exceeds 64 KiB"); }
                    if terminal.child.try_wait()?.is_some() { bail!("Terminal has exited; open a new terminal"); }
                    terminal.writer.write_all(data.as_bytes())?;
                    terminal.writer.flush()?;
                    Ok(json!({"written":data.len()}))
                }
                "resize" => {
                    let size = size(params)?;
                    terminal.master.resize(size)?;
                    terminal.cols = size.cols;
                    terminal.rows = size.rows;
                    terminal.summary(id)
                }
                _ => { all.remove(id); Ok(json!({"closed":true})) }
            }
        }
        _ => bail!("Unknown terminal action"),
    }
}

fn access_target(target: &str) -> Result<()> {
    if !matches!(target,"terminal"|"browser"|"ios"|"android") { bail!("Unknown development tool target"); }
    Ok(())
}
fn default_access(conversation: &str) -> Value {
    json!({"id":conversation,"terminal":false,"browser":false,"ios":false,"android":false})
}
impl Engine {
    pub(crate) async fn devtools_dispatch(&self, method: &str, input: &Value) -> Result<Value> {
        let _operation = Operation::start();
        let conversation = required(input,"conversation_id")?;
        let (workspace, mut access) = {
            let store = lock(&self.store)?;
            let c = store.entity("conversation", conversation)?;
            let project = c["project_id"].as_str().context("Attach a project to use development tools")?;
            let p = store.entity("project", project)?;
            let path = std::fs::canonicalize(required(&p,"path")?).context("Project folder is unavailable")?;
            if !path.is_dir() { bail!("Project folder is unavailable"); }
            let mut access = store.entity("devtools_access", conversation).unwrap_or_else(|_|default_access(conversation));
            if access["workspace"].as_str()!=path.to_str() { access=default_access(conversation); }
            access["workspace"] = json!(path);
            (path,access)
        };
        let actor = input.get("actor").and_then(Value::as_str).unwrap_or("user");
        if !matches!(actor,"user"|"agent") { bail!("Unknown development tool actor"); }
        let parts: Vec<_> = method.split('/').collect();
        if parts.len()!=3 || parts[0]!="devtools" { bail!("Unknown development tool method"); }
        let (target,action) = (parts[1],parts[2]);
        if target=="activity" && action=="get" {
            let mut activity=lock(&self.store)?.entity("devtools_activity",conversation).unwrap_or_else(|_|json!({"id":null}));
            activity["id"]=activity.get("event_id").cloned().unwrap_or(Value::Null);
            return Ok(activity);
        }
        if target=="access" {
            match action {
                "get" => return Ok(access),
                "grant"|"revoke" => {
                    if actor!="user" { bail!("Only the user can change agent tool access"); }
                    let target = required(input,"target")?;
                    access_target(target)?;
                    access[target] = json!(action=="grant");
                    lock(&self.store)?.put("devtools_access",conversation,&access)?;
                    return Ok(access);
                }
                _ => bail!("Unknown access action"),
            }
        }
        let access_key = if target=="mobile" { required(input,"platform")? } else { target };
        access_target(access_key)?;
        if target=="mobile" && !matches!(access_key,"ios"|"android") { bail!("Mobile platform must be ios or android"); }
        let metadata_only = target=="mobile" && matches!(action,"list"|"status");
        if actor=="agent" && !metadata_only && access[access_key]!=true {
            bail!("Agent access to {access_key} is disabled. Ask the user to allow access in this conversation's tool panel.");
        }
        let mut params = input.clone();
        params.as_object_mut().context("Parameters must be an object")?.remove("_auth");
        let result = match target {
            "terminal" => {
                let action = action.to_owned();
                tokio::task::spawn_blocking(move ||terminal_dispatch(&action,&params,&workspace)).await?
            }
            "browser" => crate::devtools_browser::dispatch(action,params,&workspace).await,
            "mobile" => crate::devtools_mobile::dispatch(action,params,&workspace).await,
            _ => bail!("Unknown development tool target"),
        }?;
        if actor=="agent" && matches!(action,"create"|"open"|"navigate"|"boot"|"launch"|"click"|"type"|"key"|"scroll"|"tap"|"swipe"|"install"|"write") {
            let device_id=result.get("device_id").and_then(Value::as_str).or_else(||input.get("device_id").and_then(Value::as_str));
            let terminal_id=if target=="terminal" { result.get("id").and_then(Value::as_str).or_else(||input.get("id").and_then(Value::as_str)) } else { None };
            let activity=json!({"id":conversation,"event_id":uuid::Uuid::new_v4().to_string(),"target":access_key,"device_id":device_id,"terminal_id":terminal_id,"at":crate::store::now()});
            lock(&self.store)?.put("devtools_activity",conversation,&activity)?;
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn terminal_output_is_bounded_and_reports_lost_bytes() {
        let mut output = Output::default();
        output.append(&vec![b'a';OUTPUT_LIMIT]);
        output.append("سلام".as_bytes());
        assert_eq!(output.bytes.len(),OUTPUT_LIMIT);
        let result = output.read(0).unwrap();
        assert_eq!(result["truncated"],true);
        assert_eq!(result["cursor"],8+READ_LIMIT);
        let tail=output.read(OUTPUT_LIMIT as u64).unwrap();
        assert_eq!(base64::engine::general_purpose::STANDARD.decode(tail["data"].as_str().unwrap()).unwrap(),"سلام".as_bytes());
        assert!(output.read(OUTPUT_LIMIT as u64+9).is_err());
    }
    #[test]
    fn terminal_size_rejects_bad_values() {
        for params in [json!({"cols":0}),json!({"rows":301}),json!({"cols":"80"}),json!({"cols":65536})] { assert!(size(&params).is_err()); }
        assert_eq!(size(&json!({"cols":80,"rows":24})).unwrap().cols,80);
    }
    #[test]
    fn access_is_default_denied_and_targets_are_fixed() {
        let access=default_access("conversation");
        for target in ["terminal","browser","ios","android"] { access_target(target).unwrap(); assert_eq!(access[target],false); }
        assert!(access_target("access").is_err());
        assert!(access_target("../terminal").is_err());
    }
}
