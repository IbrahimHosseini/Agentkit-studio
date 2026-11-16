use crate::paths;
use agentkit_protocol::{Request, RpcError};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Deserializer};
use serde_json::{json, Value};
use std::{
    path::PathBuf,
    process::{Command, Stdio},
    time::Duration,
};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};

#[derive(Deserialize)]
struct ClientResponse {
    id: String,
    #[serde(default, deserialize_with = "present_result")]
    result: Option<Value>,
    error: Option<RpcError>,
}

// A successful null result must remain distinct from an absent result field.
fn present_result<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Option<Value>, D::Error> {
    Value::deserialize(deserializer).map(Some)
}

trait Duplex: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Duplex for T {}
async fn connect(root: &std::path::Path) -> Result<Box<dyn Duplex>> {
    #[cfg(unix)]
    {
        Ok(Box::new(
            tokio::net::UnixStream::connect(paths::endpoint(root)?).await?,
        ))
    }
    #[cfg(windows)]
    {
        Ok(Box::new(
            tokio::net::windows::named_pipe::ClientOptions::new().open(paths::endpoint(root)?)?,
        ))
    }
}
fn engine_binary() -> Result<PathBuf> {
    if let Some(value) = std::env::var_os("AGENTKIT_ENGINE_BIN") {
        return Ok(PathBuf::from(value));
    }
    let current = std::env::current_exe()?;
    let name = if cfg!(windows) {
        "agentkit.exe"
    } else {
        "agentkit"
    };
    if let Some(parent) = current.parent() {
        let candidate = parent.join(name);
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    bail!("Cannot locate the Rust engine binary beside this application. Set AGENTKIT_ENGINE_BIN to its absolute path")
}
fn launch() -> Result<()> {
    let binary = engine_binary()?;
    let mut cmd = Command::new(&binary);
    cmd.arg("daemon")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000 | 0x00000200); // NO_WINDOW | NEW_PROCESS_GROUP
    }
    cmd.spawn()
        .with_context(|| format!("Could not start engine {}", binary.display()))?;
    Ok(())
}
/// Send exactly once after establishing a connection. Never retry a submitted mutation.
pub async fn request(method: &str, params: Value) -> Result<Value> {
    let root = paths::data_root()?;
    let mut stream = match connect(&root).await {
        Ok(stream) => stream,
        Err(_) => {
            launch()?;
            let mut ready = None;
            for _ in 0..100 {
                tokio::time::sleep(Duration::from_millis(100)).await;
                if let Ok(stream) = connect(&root).await {
                    ready = Some(stream);
                    break;
                }
            }
            ready.context("Engine did not become ready within 10 seconds")?
        }
    };
    let mut params = if params.is_null() { json!({}) } else { params };
    params
        .as_object_mut()
        .context("Request parameters must be an object")?
        .insert("_auth".into(), json!(paths::token(&root)?));
    let request = Request {
        id: uuid::Uuid::new_v4().to_string(),
        method: method.into(),
        params,
    };
    let mut bytes = serde_json::to_vec(&request)?;
    if bytes.len() > 2 * 1024 * 1024 {
        bail!("Request exceeds the 2 MiB local message limit");
    }
    bytes.push(b'\n');
    stream.write_all(&bytes).await?;
    stream.flush().await?;
    let mut line = Vec::new();
    // Device screenshots have a separate bounded response budget.
    let mut reader = BufReader::new(stream).take(12 * 1024 * 1024 + 1);
    let timeout = if method.starts_with("devtools/mobile/") { 150 } else { 30 };
    tokio::time::timeout(Duration::from_secs(timeout), reader.read_until(b'\n', &mut line))
        .await
        .context("Engine response timed out")??;
    if line.len() > 12 * 1024 * 1024 || !line.ends_with(b"\n") {
        bail!("Invalid or oversized engine response");
    }
    let response: ClientResponse = serde_json::from_slice(&line)?;
    if response.id != request.id {
        bail!("Engine response identifier mismatch");
    }
    if let Some(error) = response.error {
        bail!("{}: {}", error.code, error.message);
    }
    response.result.context("Engine returned no result")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn response_distinguishes_null_from_missing_result() {
        let null: ClientResponse = serde_json::from_str(r#"{"id":"test","result":null}"#).unwrap();
        assert_eq!(null.result, Some(Value::Null));
        let missing: ClientResponse = serde_json::from_str(r#"{"id":"test"}"#).unwrap();
        assert!(missing.result.is_none());
        let object: ClientResponse =
            serde_json::from_str(r#"{"id":"test","result":{"ready":true}}"#).unwrap();
        assert_eq!(object.result, Some(json!({"ready": true})));
    }
}
