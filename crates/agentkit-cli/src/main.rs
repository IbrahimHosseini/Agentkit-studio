use agentkit_protocol::{Attachment, RunInput};
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use serde_json::{json, Value};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "agentkit",
    version,
    about = "Agentkit shared desktop and terminal execution engine"
)]
struct Cli {
    #[arg(long, global = true)]
    json: bool,
    #[arg(
        long,
        global = true,
        help = "Isolated engine state directory (also AGENTKIT_DATA_DIR)"
    )]
    data_dir: Option<PathBuf>,
    #[command(subcommand)]
    command: Commands,
}
#[derive(Subcommand)]
enum Commands {
    /// Expose Studio coordination tools over MCP stdio. Does not change app configuration.
    McpServer,
    /// Send a local Studio API request (same engine used by the GUI).
    Rpc {
        method: String,
        #[arg(long, default_value = "{}")]
        params: String,
    },
    /// Start a run in the shared background engine.
    Run {
        #[arg(long,value_parser=["codex","claude","mock"])]
        provider: String,
        #[arg(long, default_value = ".")]
        project: PathBuf,
        #[arg(long)]
        prompt: String,
        #[arg(long)]
        role: Option<String>,
        #[arg(long)]
        model: Option<String>,
        #[arg(long)]
        effort: Option<String>,
        #[arg(long)]
        image: Vec<PathBuf>,
        #[arg(long)]
        file: Vec<PathBuf>,
    },
    /// List recent shared runs.
    Runs {
        #[arg(long, default_value_t = 100)]
        limit: u64,
    },
    /// Inspect a run without requiring a desktop window.
    Status { run_id: String },
    /// Stop the provider and its owned subprocess tree.
    Stop { run_id: String },
    /// Read persisted activity after a cursor.
    Events {
        run_id: String,
        #[arg(long, default_value_t = 0)]
        after: u64,
        #[arg(long)]
        follow: bool,
    },
    /// Answer a currently pending provider approval.
    Approve {
        run_id: String,
        request_id: String,
        #[arg(long, conflicts_with = "deny")]
        allow: bool,
        #[arg(long, required_unless_present = "allow")]
        deny: bool,
    },
    /// Continue a provider session when supported.
    Resume {
        run_id: String,
        #[arg(long)]
        prompt: Option<String>,
    },
    /// Inspect the engine protocol and provider capabilities.
    Handshake,
    /// Sample engine and optional GUI process-tree resources.
    Metrics {
        #[arg(long)]
        gui_pid: Option<u32>,
    },
    #[command(hide = true)]
    Daemon,
    #[command(hide = true)]
    EngineShutdown,
}
fn attachment(path: PathBuf, kind: &str, index: usize) -> Result<Attachment> {
    let path = std::fs::canonicalize(&path)
        .with_context(|| format!("Attachment not found: {}", path.display()))?;
    let ext = path
        .extension()
        .and_then(|v| v.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let mime = match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "txt" | "md" | "rs" | "py" | "js" | "ts" | "svelte" | "json" | "toml" => "text/plain",
        "pdf" => "application/pdf",
        _ => "application/octet-stream",
    };
    Ok(Attachment {
        id: format!("{kind}-{index}"),
        kind: kind.into(),
        path: path.to_string_lossy().into_owned(),
        mime: mime.into(),
    })
}
fn show(value: &Value, json_mode: bool) -> Result<()> {
    if json_mode {
        println!("{}", serde_json::to_string(value)?);
    } else if let Some(runs) = value.as_array() {
        if runs.is_empty() {
            println!("No runs yet.");
        }
        for run in runs {
            println!(
                "{}  {:<18} {:<7} {}",
                run["id"].as_str().unwrap_or("?"),
                run["status"].as_str().unwrap_or("?"),
                run["provider"].as_str().unwrap_or("?"),
                run["prompt"]
                    .as_str()
                    .unwrap_or("")
                    .chars()
                    .take(100)
                    .collect::<String>()
                    .replace(['\n', '\r'], " ")
            );
        }
    } else if value.get("id").is_some() && value.get("status").is_some() {
        println!(
            "{}  {}  {}",
            value["id"].as_str().unwrap_or("?"),
            value["status"].as_str().unwrap_or("?"),
            value["provider"].as_str().unwrap_or("?")
        );
    } else {
        println!("{}", serde_json::to_string_pretty(value)?);
    }
    Ok(())
}
#[tokio::main]
async fn main() {
    if let Err(err) = execute().await {
        eprintln!("agentkit: {err:#}");
        std::process::exit(1);
    }
}
async fn execute() -> Result<()> {
    let cli = Cli::parse();
    if let Some(path) = cli.data_dir {
        std::env::set_var("AGENTKIT_DATA_DIR", path);
    }
    let (method, params) = match cli.command {
        Commands::McpServer => return mcp_server().await,
        Commands::Rpc { method, params } => {
            let result =
                agentkit_core::client::request(&method, serde_json::from_str(&params)?).await?;
            return show(&result, cli.json);
        }
        Commands::Daemon => return agentkit_core::daemon::serve().await,
        Commands::Run {
            provider,
            project,
            prompt,
            role,
            model,
            effort,
            image,
            file,
        } => {
            let mut attachments = Vec::new();
            for path in image {
                attachments.push(attachment(path, "image", attachments.len())?);
            }
            for path in file {
                attachments.push(attachment(path, "file", attachments.len())?);
            }
            let input = RunInput {
                provider,
                workspace: std::fs::canonicalize(project)?
                    .to_string_lossy()
                    .into_owned(),
                prompt,
                role,
                attachments,
                resume_session: None,
                model,
                effort,
                instructions: None,
                read_only: false,
                claude_auth_mode: None,
            };
            ("start", serde_json::to_value(input)?)
        }
        Commands::Runs { limit } => ("list", json!({"limit":limit})),
        Commands::Status { run_id } => ("get", json!({"run_id":run_id})),
        Commands::Stop { run_id } => ("stop", json!({"run_id":run_id})),
        Commands::Approve {
            run_id,
            request_id,
            allow,
            deny: _,
        } => (
            "approve",
            json!({"run_id":run_id,"request_id":request_id,"decision":allow}),
        ),
        Commands::Resume { run_id, prompt } => ("resume", json!({"run_id":run_id,"prompt":prompt})),
        Commands::Handshake => ("handshake", json!({})),
        Commands::Metrics { gui_pid } => ("metrics", json!({"gui_pid":gui_pid})),
        Commands::EngineShutdown => ("shutdown", json!({})),
        Commands::Events {
            run_id,
            mut after,
            follow,
        } => {
            loop {
                let page = agentkit_core::client::request(
                    "subscribe",
                    json!({"run_id":run_id,"after_seq":after,"limit":50}),
                )
                .await?;
                if cli.json {
                    println!("{}", serde_json::to_string(&page)?);
                } else if let Some(events) = page["events"].as_array() {
                    for event in events {
                        println!(
                            "{} {} {}",
                            event["seq"],
                            event["kind"].as_str().unwrap_or("?"),
                            event["payload"]
                        );
                    }
                }
                after = page["next_seq"].as_u64().unwrap_or(after);
                if !follow {
                    break;
                }
                let run = agentkit_core::client::request("get", json!({"run_id":run_id})).await?;
                if matches!(
                    run["status"].as_str(),
                    Some("completed" | "failed" | "cancelled" | "interrupted")
                ) && page["events"].as_array().is_none_or(|v| v.is_empty())
                {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(350)).await;
            }
            return Ok(());
        }
    };
    let result = agentkit_core::client::request(method, params).await?;
    show(&result, cli.json)
}

async fn mcp_server() -> Result<()> {
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
    let mut input = BufReader::new(tokio::io::stdin());
    let mut output = tokio::io::stdout();
    let definitions=[
        ("list_agents","studio/agents","List installed agents and their Studio settings.",json!({"workspace":{"type":"string"}}),vec!["workspace"]),
        ("delegate","studio/message/send","Submit a request to a conversation. Studio plans and enforces approval before extensive changes.",json!({"conversation_id":{"type":"string"},"text":{"type":"string"}}),vec!["conversation_id","text"]),
        ("get_status","studio/conversation/get","Get conversation messages, execution status, tasks and results.",json!({"conversation_id":{"type":"string"}}),vec!["conversation_id"]),
        ("get_result","studio/conversation/get","Get the retained result and task details for a conversation.",json!({"conversation_id":{"type":"string"}}),vec!["conversation_id"]),
        ("stop","studio/conversation/stop","Stop the conversation's active and queued work.",json!({"conversation_id":{"type":"string"}}),vec!["conversation_id"]),
    ];
    loop {
        let mut line = Vec::new();
        let n = (&mut input)
            .take(2 * 1024 * 1024 + 1)
            .read_until(b'\n', &mut line)
            .await?;
        if n == 0 {
            return Ok(());
        }
        if n > 2 * 1024 * 1024 || !line.ends_with(b"\n") {
            anyhow::bail!("Invalid MCP message size");
        }
        let request: Value = match serde_json::from_slice(&line) {
            Ok(v) => v,
            Err(_) => {
                output.write_all(b"{\"jsonrpc\":\"2.0\",\"id\":null,\"error\":{\"code\":-32700,\"message\":\"Parse error\"}}\n").await?;
                output.flush().await?;
                continue;
            }
        };
        let Some(id) = request.get("id") else {
            continue;
        };
        let method = request["method"].as_str().unwrap_or("");
        let p = &request["params"];
        let result: Result<Value> = match method {
            "initialize" => Ok(
                json!({"protocolVersion":"2024-11-05","capabilities":{"tools":{"listChanged":false}},"serverInfo":{"name":"agentkit-studio","version":env!("CARGO_PKG_VERSION")},"instructions":"Use existing Studio conversations. Delegation never bypasses Studio plan approval. No approval tool is exposed; the user approves in Studio."}),
            ),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(
                json!({"tools":definitions.iter().map(|(name,_,description,properties,required)|json!({"name":name,"description":description,"inputSchema":{"type":"object","properties":properties,"required":required,"additionalProperties":false}})).collect::<Vec<_>>()}),
            ),
            "tools/call" => {
                if let Some((_, method, _, properties, required)) = definitions
                    .iter()
                    .find(|(name, ..)| p["name"].as_str() == Some(name))
                {
                    let args = p.get("arguments").cloned().unwrap_or_else(|| json!({}));
                    let valid = args.as_object().is_some_and(|o| {
                        o.keys().all(|k| properties.get(k).is_some())
                            && required
                                .iter()
                                .all(|k| o.get(*k).is_some_and(Value::is_string))
                    });
                    if !valid {
                        Ok(
                            json!({"content":[{"type":"text","text":"Invalid tool arguments"}],"isError":true}),
                        )
                    } else {
                        let dispatched=async {
                            let settings=agentkit_core::client::request("studio/settings",json!({})).await?;
                            if settings["mcp_enabled"]!=true {anyhow::bail!("MCP tools are disabled. Enable the Studio MCP bridge in Connections first.");}
                            agentkit_core::client::request(method,args).await
                        }.await;
                        match dispatched {
                            Ok(v) => Ok(
                                json!({"content":[{"type":"text","text":serde_json::to_string(&v)?}],"isError":false}),
                            ),
                            Err(e) => Ok(
                                json!({"content":[{"type":"text","text":e.to_string()}],"isError":true}),
                            ),
                        }
                    }
                } else {
                    Err(anyhow::anyhow!("Unknown tool"))
                }
            }
            _ => Err(anyhow::anyhow!("Unknown MCP method")),
        };
        let response = match result {
            Ok(v) => json!({"jsonrpc":"2.0","id":id,"result":v}),
            Err(e) => {
                json!({"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":e.to_string()}})
            }
        };
        output
            .write_all(format!("{}\n", response).as_bytes())
            .await?;
        output.flush().await?;
    }
}
