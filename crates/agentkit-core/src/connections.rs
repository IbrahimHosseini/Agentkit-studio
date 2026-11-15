//! Explicit runtime capability probes; never starts a model turn.
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::{process::Stdio, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    process::Command,
};
fn executable(provider: &str) -> std::ffi::OsString {
    match provider {
        "codex" => agentkit_providers::codex_executable(),
        "claude" => agentkit_providers::claude_executable(),
        _ => provider.into(),
    }
}
async fn output(provider: &str, arg: &str) -> Result<String> {
    probe_output(provider, &executable(provider), arg, Duration::from_secs(5)).await
}
// Never return raw stdout/stderr on failure: native configuration can contain secrets.
async fn probe_output(provider: &str, executable: &std::ffi::OsStr, arg: &str, timeout: Duration) -> Result<String> {
    let mut child = Command::new(executable)
        .arg(arg).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null())
        .kill_on_drop(true).spawn().map_err(|error| {
            let reason = match error.kind() {
                std::io::ErrorKind::NotFound => "executable was not found (or its interpreter is missing); install the native CLI or check the Studio executable override",
                std::io::ErrorKind::PermissionDenied => "executable permission was denied; check the native CLI installation",
                _ => "operating system could not start the native CLI",
            };
            anyhow::anyhow!("{provider} {arg} probe: {reason} (OS error {:?})", error.raw_os_error())
        })?;
    let mut bytes = Vec::new();
    let result = tokio::time::timeout(timeout, async {
        child.stdout.take().context("Missing probe stdout")?.take(128 * 1024 + 1)
            .read_to_end(&mut bytes).await.context("Cannot read runtime probe output")?;
        if bytes.len() > 128 * 1024 { bail!("{provider} {arg} probe output exceeds 128 KiB"); }
        let status = child.wait().await.context("Cannot wait for runtime probe")?;
        if !status.success() { bail!("{provider} {arg} probe exited unsuccessfully ({status}); repair or update the native CLI"); }
        let text = String::from_utf8_lossy(&bytes).into_owned();
        if text.trim().is_empty() { bail!("{provider} {arg} probe returned empty output"); }
        Ok::<_, anyhow::Error>(text)
    }).await;
    match result {
        Ok(value) => value,
        Err(_) => {
            let _ = child.kill().await;
            bail!("{provider} {arg} probe timed out after {} ms", timeout.as_millis())
        }
    }
}
fn inspected(provider: &str, version: String, help: Result<String>) -> Value {
    let help = match help {
        Ok(help) => help,
        Err(error) => return json!({"provider":provider,"installed":true,"version":version.trim().chars().take(160).collect::<String>(),"compatible":false,"capabilities":{"model":false,"effort":false,"read_only":false},"status":"unavailable","detail":error.to_string()}),
    };
    let required: &[&str] = if provider == "codex" { &["app-server"] } else {
        &["--input-format", "--output-format", "--permission-prompts", "--forward-subagent-text", "--safe-mode", "--tools", "--disallowedTools", "--permission-mode"]
    };
    let missing: Vec<_> = required.iter().copied().filter(|flag| !help.contains(flag)).collect();
    let compatible = missing.is_empty();
    let read_only = if provider == "codex" { help.contains("app-server") } else {
        ["--safe-mode", "--tools", "--disallowedTools", "--permission-mode"].iter().all(|flag| help.contains(flag))
    };
    let detail = if compatible {
        "CLI detected. Account authentication and successful model execution are not verified by this probe.".to_owned()
    } else { format!("Native CLI is missing required capabilities: {}. Update the native CLI; Studio connection permission alone cannot add these capabilities.", missing.join(", ")) };
    json!({"provider":provider,"installed":true,"version":version.trim().chars().take(160).collect::<String>(),"compatible":compatible,
        "capabilities":{"model":help.contains("--model"),"effort":provider=="codex"||help.contains("--effort"),"read_only":read_only,"native_auth":provider=="claude" && help.contains("--setting-sources")},
        "status":if compatible{"available"}else{"incompatible"},"detail":detail})
}
pub async fn inspect() -> Value {
    let mut providers = Vec::new();
    for provider in ["codex", "claude"] {
        match output(provider,"--version").await {
            Ok(version)=> providers.push(inspected(provider, version, output(provider,"--help").await)),
            Err(error)=>providers.push(json!({"provider":provider,"installed":false,"version":null,"compatible":false,"capabilities":{"model":false,"effort":false,"read_only":false},"status":"unavailable","detail":error.to_string()})),
        }
    }
    json!({"providers":providers,"mcp":{"transport":"stdio","command":"agentkit","args":["mcp-server"],"automatic_config":false}})
}
pub async fn codex_catalog() -> Result<Value> {
    let mut child = Command::new(executable("codex"))
        .arg("app-server")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()?;
    let result=tokio::time::timeout(Duration::from_secs(15),async{
        let mut input=child.stdin.take().context("Missing stdin")?;
        let mut output=BufReader::new(child.stdout.take().context("Missing stdout")?);
        async fn send(input:&mut tokio::process::ChildStdin,v:Value)->Result<()>{input.write_all(format!("{}\n",v).as_bytes()).await?;input.flush().await?;Ok(())}
        async fn response(output:&mut BufReader<tokio::process::ChildStdout>,id:u64)->Result<Value>{
            for _ in 0..256 {
                let mut line=Vec::new();let n=(&mut *output).take(2*1024*1024+1).read_until(b'\n',&mut line).await?;
                if n==0||n>2*1024*1024||!line.ends_with(b"\n"){bail!("Invalid catalog response");}
                let v:Value=serde_json::from_slice(&line)?;
                if v["id"]==id {if let Some(error)=v.get("error"){bail!("Catalog request rejected: {}",error["message"].as_str().unwrap_or("runtime error"));}return Ok(v["result"].clone());}
            }
            bail!("Too many unrelated catalog events")
        }
        send(&mut input,json!({"id":1,"method":"initialize","params":{"clientInfo":{"name":"agentkit-studio-catalog","version":"0.1.0"}}})).await?;
        response(&mut output,1).await?;send(&mut input,json!({"method":"initialized"})).await?;
        send(&mut input,json!({"id":2,"method":"account/read","params":{"refreshToken":false}})).await?;
        let account=response(&mut output,2).await?;
        let mut models=Vec::new();let mut cursor=Value::Null;
        for page in 0..20 {
            let id=page+3;send(&mut input,json!({"id":id,"method":"model/list","params":{"limit":100,"cursor":cursor,"includeHidden":false}})).await?;
            let result=response(&mut output,id).await?;
            if let Some(data)=result["data"].as_array(){for model in data {models.push(json!({"id":model["id"],"model":model["model"],"displayName":model["displayName"],"defaultReasoningEffort":model["defaultReasoningEffort"],"supportedReasoningEfforts":model["supportedReasoningEfforts"],"inputModalities":model["inputModalities"],"verification":"advertised"}));}}
            cursor=result["nextCursor"].clone();if cursor.is_null(){break;}
        }
        Ok::<_,anyhow::Error>(json!({"provider":"codex","models":models,"account_type":account["account"]["type"],"requires_auth":account["requiresOpenaiAuth"],"verified_execution":false}))
    }).await;
    let _ = child.kill().await;
    result.context("Catalog probe timed out")?
}

/// Advertised capabilities and installed definition defaults remain distinct.
pub async fn catalog(provider: &str, workspace: &str) -> Result<Value> {
    if !matches!(provider, "codex" | "claude") {
        bail!("Unknown model catalog provider");
    }
    let installed = installed_models(provider, crate::registry::discover(workspace)?);
    if provider == "claude" {
        return Ok(
            json!({"provider":provider,"models":installed,"verified_execution":false,
            "detail":"Models and effort values come from installed agent definitions; runtime availability is not verified."}),
        );
    }
    match codex_catalog().await {
        Ok(mut catalog) => {
            let models = catalog["models"]
                .as_array_mut()
                .context("Invalid model catalog")?;
            for model in installed {
                if !models.iter().any(|m| m["model"] == model["model"]) {
                    models.push(model);
                }
            }
            Ok(catalog)
        }
        Err(error) => Ok(
            json!({"provider":provider,"models":installed,"verified_execution":false,
            "catalog_error":error.to_string(),"detail":"Runtime catalog unavailable; showing installed definitions only."}),
        ),
    }
}

fn installed_models(provider: &str, definitions: Vec<Value>) -> Vec<Value> {
    let mut installed = std::collections::BTreeMap::<String, Value>::new();
    for agent in definitions {
        if agent["provider"] != provider || agent["available"] != true {
            continue;
        }
        let Some(model) = agent["model"].as_str().filter(|m| !m.is_empty()) else {
            continue;
        };
        let entry = installed.entry(model.to_owned()).or_insert_with(|| {
            json!({
                "id":model,"model":model,"displayName":model,"defaultReasoningEffort":null,
                "supportedReasoningEfforts":[],"verification":"installed","installedEfforts":[]
            })
        });
        if let Some(effort) = agent["effort"].as_str() {
            let efforts = entry["installedEfforts"].as_array_mut().unwrap();
            if !efforts.contains(&json!(effort)) {
                efforts.push(json!(effort));
            }
        }
    }
    installed.into_values().collect()
}

#[cfg(test)]
mod catalog_tests {
    use super::*;
    #[test]
    fn installed_values_are_not_advertised_capabilities() {
        let models = installed_models(
            "claude",
            vec![
                json!({"provider":"claude","available":true,"model":"installed","effort":"high"}),
                json!({"provider":"claude","available":true,"model":"installed","effort":"high"}),
                json!({"provider":"claude","available":true,"model":"installed","effort":"low"}),
                json!({"provider":"claude","available":true,"model":null,"effort":null}),
                json!({"provider":"codex","available":true,"model":"foreign"}),
                json!({"provider":"claude","available":false,"model":"broken"}),
            ],
        );
        assert_eq!(models.len(), 1);
        assert_eq!(models[0]["verification"], "installed");
        assert_eq!(models[0]["installedEfforts"], json!(["high", "low"]));
        assert_eq!(models[0]["supportedReasoningEfforts"], json!([]));
        assert!(models[0]["defaultReasoningEffort"].is_null());
    }
}

#[cfg(all(test, unix))]
mod probe_tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    #[tokio::test]
    async fn diagnostic_failures_are_precise_and_do_not_echo_process_output() {
        let root = std::env::temp_dir().join(format!("studio-probe-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let missing = root.join("missing");
        let error = probe_output("claude", missing.as_os_str(), "--version", Duration::from_millis(100)).await.unwrap_err().to_string();
        assert!(error.contains("--version") && error.contains("not found"));
        let script = root.join("fake");
        std::fs::write(&script, "#!/bin/sh\necho PRIVATE_STDOUT\necho PRIVATE_STDERR >&2\nexit 7\n").unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
        let error = probe_output("claude", script.as_os_str(), "--help", Duration::from_secs(1)).await.unwrap_err().to_string();
        assert!(error.contains("--help") && error.contains("7"));
        assert!(!error.contains("PRIVATE"));
        std::fs::write(&script, "#!/bin/sh\nexec /bin/sleep 1\n").unwrap();
        let error = probe_output("claude", script.as_os_str(), "--help", Duration::from_millis(30)).await.unwrap_err().to_string();
        assert!(error.contains("timed out after 30 ms"));
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(probe_output("claude", script.as_os_str(), "--version", Duration::from_millis(100)).await.unwrap_err().to_string().contains("permission was denied"));
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn capability_failures_distinguish_help_failure_from_missing_flags() {
        let failed = inspected("claude", "test-version".into(), Err(anyhow::anyhow!("help probe timed out")));
        assert_eq!(failed["installed"], true);
        assert_eq!(failed["status"], "unavailable");
        assert_eq!(failed["detail"], "help probe timed out");
        let incompatible = inspected("claude", "test-version".into(), Ok("--model".into()));
        assert_eq!(incompatible["status"], "incompatible");
        assert!(incompatible["detail"].as_str().unwrap().contains("--permission-mode"));
        let available = inspected("claude", "test-version".into(), Ok("--input-format --output-format --permission-prompts --forward-subagent-text --safe-mode --tools --disallowedTools --permission-mode --effort --model --setting-sources".into()));
        assert_eq!(available["compatible"], true);
        assert_eq!(available["capabilities"]["native_auth"], true);
    }
}
