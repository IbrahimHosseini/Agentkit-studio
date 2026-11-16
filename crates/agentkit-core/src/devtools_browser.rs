//! Conversation-isolated Chromium sessions over a private bounded stdio pipe.
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::{collections::HashMap, path::{Path, PathBuf}, process::Stdio, sync::OnceLock, time::Duration};
use tokio::{io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader}, process::{Child, ChildStdin, ChildStdout, Command}, sync::Mutex};

struct Browser {
    workspace: PathBuf,
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
}
static BROWSERS: OnceLock<Mutex<HashMap<String, Browser>>> = OnceLock::new();
fn helper_path() -> PathBuf {
    std::env::var_os("AGENTKIT_BROWSER_HELPER").map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/desktop/devtools/browser.mjs"))
}
async fn launch(workspace: &Path) -> Result<Browser> {
    let helper = helper_path();
    if !helper.is_file() { bail!("Browser runtime is missing. Reinstall Studio or set AGENTKIT_BROWSER_HELPER."); }
    let node = std::env::var_os("AGENTKIT_NODE_BIN").unwrap_or_else(|| {
        // Finder-launched macOS apps often have only /usr/bin:/bin on PATH.
        ["/opt/homebrew/bin/node", "/usr/local/bin/node"].into_iter()
            .find(|path| Path::new(path).is_file()).unwrap_or("node").into()
    });
    let mut child = Command::new(node)
        .arg(helper).current_dir(workspace).stdin(Stdio::piped()).stdout(Stdio::piped())
        .stderr(Stdio::null()).kill_on_drop(true).spawn().context("Browser needs Node.js installed and available on PATH")?;
    let input = child.stdin.take().context("Missing browser input")?;
    let output = BufReader::new(child.stdout.take().context("Missing browser output")?);
    Ok(Browser { workspace: workspace.to_owned(), child, input, output })
}
pub(crate) async fn dispatch(action: &str, params: Value, workspace: &Path) -> Result<Value> {
    let cid = params["conversation_id"].as_str().context("Missing conversation_id")?;
    let mut browsers = BROWSERS.get_or_init(Default::default).lock().await;
    if browsers.get(cid).is_some_and(|browser| browser.workspace != workspace) {
        if let Some(mut browser) = browsers.remove(cid) {
            let _ = browser.input.shutdown().await;
            if tokio::time::timeout(Duration::from_secs(3), browser.child.wait()).await.is_err() { let _ = browser.child.kill().await; }
        }
    }
    if !browsers.contains_key(cid) {
        if action == "list" || action == "close" { return Ok(json!({"pages":[],"active_page_id":null})); }
        if browsers.len() >= 8 { bail!("Close another browser session before starting one (limit: 8)."); }
        browsers.insert(cid.to_owned(), launch(workspace).await?);
    }
    let browser = browsers.get_mut(cid).context("Browser session unavailable")?;
    let request = serde_json::to_vec(&json!({"action":action,"params":params}))?;
    if request.len() > 128 * 1024 { bail!("Browser request too large"); }
    let result = tokio::time::timeout(Duration::from_secs(25), async {
        browser.input.write_all(&request).await?;
        browser.input.write_all(b"\n").await?;
        browser.input.flush().await?;
        let mut response = Vec::new();
        (&mut browser.output).take(8 * 1024 * 1024 + 1).read_until(b'\n', &mut response).await?;
        if response.len() > 8 * 1024 * 1024 || !response.ends_with(b"\n") { bail!("Browser runtime stopped or returned an oversized response. Check Node.js and playwright-core installation."); }
        let value: Value = serde_json::from_slice(&response)?;
        Ok::<_, anyhow::Error>(value)
    }).await;
    // A broken pipe/timeout must discard the session: the next response could belong to the old request.
    let value = match result {
        Ok(Ok(value)) => value,
        other => {
            if let Some(mut browser) = browsers.remove(cid) { let _ = browser.child.kill().await; }
            return match other { Ok(Err(e)) => Err(e), _ => Err(anyhow::anyhow!("Browser operation timed out; reopen the browser session.")) };
        }
    };
    let empty = value["result"]["pages"].as_array().is_some_and(Vec::is_empty);
    if (action == "close" && (params.get("page_id").and_then(Value::as_str).is_none() || empty))
        || (action == "list" && empty) || (action == "status" && value["result"]["running"] == false) {
        if let Some(mut browser) = browsers.remove(cid) {
            let _ = browser.input.shutdown().await;
            if tokio::time::timeout(Duration::from_secs(3), browser.child.wait()).await.is_err() { let _ = browser.child.kill().await; }
        }
    }
    if let Some(error) = value["error"].as_str() { bail!("{error}"); }
    Ok(value["result"].clone())
}

pub(crate) fn has_sessions() -> bool {
    BROWSERS.get().is_some_and(|sessions| sessions.try_lock().map(|s| !s.is_empty()).unwrap_or(true))
}

pub(crate) async fn close_conversation(conversation: &str) {
    if let Some(browsers) = BROWSERS.get() {
        if let Some(mut browser) = browsers.lock().await.remove(conversation) {
            let _ = browser.input.shutdown().await;
            if tokio::time::timeout(Duration::from_secs(3), browser.child.wait()).await.is_err() { let _ = browser.child.kill().await; }
        }
    }
}

pub(crate) async fn close_workspace(conversation: &str, workspace: &Path) {
    if let Some(browsers) = BROWSERS.get() {
        let mut sessions = browsers.lock().await;
        if sessions.get(conversation).is_some_and(|browser| browser.workspace == workspace) {
            if let Some(mut browser) = sessions.remove(conversation) {
                let _ = browser.input.shutdown().await;
                if tokio::time::timeout(Duration::from_secs(3), browser.child.wait()).await.is_err() { let _ = browser.child.kill().await; }
            }
        }
    }
}

pub(crate) async fn shutdown() {
    if let Some(browsers) = BROWSERS.get() {
        for (_, mut browser) in browsers.lock().await.drain() {
            let _ = browser.input.shutdown().await;
            if tokio::time::timeout(Duration::from_secs(3), browser.child.wait()).await.is_err() { let _ = browser.child.kill().await; }
        }
    }
}
