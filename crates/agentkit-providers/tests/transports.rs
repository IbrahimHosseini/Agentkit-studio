//! Real subprocess boundaries with synthetic protocols, never an installed model CLI.
#![cfg(unix)]
use agentkit_protocol::{ClaudeAuthMode, PermissionMode, ProviderEvent, RunInput};
use serde_json::{json, Value};
use std::{ffi::OsString, fs, path::PathBuf, time::Duration};
use tokio::sync::mpsc;

struct Fixture {
    root: PathBuf,
    old: Vec<(String, Option<OsString>)>,
}
impl Fixture {
    fn new() -> Self {
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().join(format!("studio-transport-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let script = root.join("provider");
        fs::write(&script, FIXTURE).unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).unwrap();
        let mut old = Vec::new();
        for (name, value) in [
            ("HOME", root.clone()),
            ("USERPROFILE", root.clone()),
            ("CODEX_HOME", root.join(".codex")),
            ("CLAUDE_CONFIG_DIR", root.join(".claude")),
            ("AGENTKIT_DATA_DIR", root.join("state")),
            ("AGENTKIT_CODEX_BIN", script.clone()),
            ("AGENTKIT_CLAUDE_BIN", script),
            ("STUDIO_TRANSPORT_LOG", root.join("log")),
        ] {
            old.push((name.into(), std::env::var_os(name)));
            std::env::set_var(name, value);
        }
        old.push((
            "CLAUDE_CODE_EFFORT_LEVEL".into(),
            std::env::var_os("CLAUDE_CODE_EFFORT_LEVEL"),
        ));
        std::env::set_var("CLAUDE_CODE_EFFORT_LEVEL", "low");
        Self { root, old }
    }
    fn input(&self, provider: &str) -> RunInput {
        serde_json::from_value(json!({"provider":provider,"workspace":self.root,"prompt":"Read fixture","model":"fixture-model","effort":"high","instructions":"Fixture role instructions","read_only":true})).unwrap()
    }
    fn log(&self) -> Vec<Value> {
        fs::read_to_string(self.root.join("log"))
            .unwrap()
            .lines()
            .map(|s| serde_json::from_str(s).unwrap())
            .collect()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        for (name, value) in self.old.drain(..) {
            if let Some(v) = value {
                std::env::set_var(name, v)
            } else {
                std::env::remove_var(name)
            }
        }
        let _ = fs::remove_dir_all(&self.root);
    }
}
async fn run(input: RunInput) -> (anyhow::Result<()>, Vec<ProviderEvent>) {
    let (tx, mut rx) = mpsc::channel(64);
    let (_controls, cr) = mpsc::channel(4);
    let result = tokio::time::timeout(
        Duration::from_secs(10),
        agentkit_providers::run(input, "fixture".into(), tx, cr),
    )
    .await
    .unwrap();
    let mut events = Vec::new();
    while let Some(e) = rx.recv().await {
        events.push(e);
    }
    (result, events)
}
async fn run_deciding(input: RunInput, decision: bool) -> (anyhow::Result<()>, Vec<ProviderEvent>) {
    let (tx, mut rx) = mpsc::channel(64);
    let (controls, cr) = mpsc::channel(4);
    let task = tokio::spawn(agentkit_providers::run(input, "fixture".into(), tx, cr));
    let mut events = Vec::new();
    while let Some(event) = tokio::time::timeout(Duration::from_secs(10), rx.recv()).await.unwrap() {
        if event.kind == "approval" {
            controls.send(agentkit_protocol::ProviderControl::Approve {
                request_id: event.payload["request_id"].as_str().unwrap().into(), decision,
            }).await.unwrap();
        }
        events.push(event);
    }
    (task.await.unwrap(), events)
}

async fn permission_mode_scenarios(fixture: &Fixture) {
    fs::write(fixture.root.join("scenario"), "permission-completed").unwrap();
    for mode in [PermissionMode::Ask, PermissionMode::Auto, PermissionMode::FullAccess] {
        for readonly in [false, true] {
            let before = fixture.log().len();
            let mut input = fixture.input("codex");
            input.read_only = readonly;
            input.permission_mode = mode;
            input.resume_session = Some("fixture-existing-session".into());
            let (result, events) = run(input).await;
            result.unwrap();
            let calls = fixture.log();
            let calls = &calls[before..];
            let start = &calls.iter().find(|v|v["method"]=="thread/resume").unwrap()["params"];
            let turn = &calls.iter().find(|v|v["method"]=="turn/start").unwrap()["params"];
            let (sandbox, policy, reviewer, kind) = if readonly { ("read-only", "never", "user", "readOnly") } else {
                match mode {
                    PermissionMode::Ask => ("workspace-write", "untrusted", "user", "workspaceWrite"),
                    PermissionMode::Auto => ("workspace-write", "on-request", "auto_review", "workspaceWrite"),
                    PermissionMode::FullAccess => ("danger-full-access", "never", "user", "dangerFullAccess"),
                }
            };
            assert_eq!(start["sandbox"], sandbox);
            for params in [start, turn] {
                assert_eq!(params["approvalPolicy"], policy);
                assert_eq!(params["approvalsReviewer"], reviewer);
            }
            assert_eq!(turn["sandboxPolicy"]["type"], kind);
            assert!(events.iter().any(|event| event.kind=="execution_config" && event.payload["permission_mode"]==json!(mode)));

            let before = fixture.log().len();
            let mut input = fixture.input("claude");input.read_only=readonly;input.permission_mode=mode;
            run(input).await.0.unwrap();
            let calls = fixture.log();
            let args = calls[before..].iter().find_map(|value|value["argv"].as_array()).unwrap();
            let index = args.iter().position(|arg|arg=="--permission-mode").unwrap();
            assert_eq!(args[index+1], if readonly { "plan" } else { match mode {PermissionMode::Ask=>"default",PermissionMode::Auto=>"auto",PermissionMode::FullAccess=>"bypassPermissions"} });
            let index = args.iter().position(|arg|arg=="--permission-prompts").unwrap();
            assert_eq!(args[index+1], if readonly { "none" } else { "host" });
        }
    }
    fs::write(fixture.root.join("unsupported-auto"), "").unwrap();
    let before = fixture.log().len();
    let mut input=fixture.input("codex");input.read_only=false;input.permission_mode=PermissionMode::Auto;
    assert!(run(input).await.0.unwrap_err().to_string().contains("cannot verify Approve for me"));
    assert_eq!(fixture.log().len(),before,"unsupported auto must not submit a task");
    fs::remove_file(fixture.root.join("unsupported-auto")).unwrap();
    for status in ["approved", "denied", "timedOut", "aborted"] {
        fs::write(fixture.root.join("scenario"), format!("review-{status}")).unwrap();
        let mut input=fixture.input("codex");input.read_only=false;input.permission_mode=PermissionMode::Auto;
        let (result,events)=run(input).await;result.unwrap();
        assert!(events.iter().any(|event|event.kind=="notice" && event.payload["phase"]=="permission_review" && event.payload["state"]==status));
        if status!="approved" {assert!(events.iter().any(|event|event.payload["text"].as_str().is_some_and(|text|text.contains("not granted"))));}
        assert!(!events.iter().any(|event|event.kind=="approval"));
    }

    fs::write(fixture.root.join("scenario"), "claude-permission").unwrap();
    for (readonly, decision) in [(false,true),(false,false),(true,true)] {
        let before=fixture.log().len();
        let mut input=fixture.input("claude");input.read_only=readonly;
        let (result,events)=run_deciding(input,decision).await;result.unwrap();
        let calls=fixture.log();
        let response=calls[before..].iter().find(|v|v["type"]=="control_response").unwrap();
        assert_eq!(response["response"]["request_id"],"claude-permission-1");
        assert_eq!(response["response"]["response"]["behavior"],if !readonly && decision {"allow"} else {"deny"});
        assert_eq!(events.iter().filter(|event|event.kind=="approval").count(),usize::from(!readonly));
        if !readonly {assert!(events.iter().any(|event|event.kind=="approval_resolved"));}
    }
    fs::remove_file(fixture.root.join("scenario")).unwrap();
}
#[tokio::test]
async fn explicit_config_and_readonly_cross_provider_boundary() {
    let mut fixture = Fixture::new();
    let config_path = fixture.root.join(".codex/config.toml");
    fs::create_dir_all(config_path.parent().unwrap()).unwrap();
    let inherited = "model=\"inherited-default\"\nmodel_reasoning_effort=\"high\"\n[mcp_servers.gitlab-780]\nurl=\"http://127.0.0.1:1/never\"\n";
    fs::write(&config_path, inherited).unwrap();
    let (result, events) = run(fixture.input("codex")).await;
    result.unwrap();
    let logs = fixture.log();
    let account = logs.iter().position(|v| v["method"] == "account/read").unwrap();
    let config = logs.iter().position(|v| v["method"] == "config/read").unwrap();
    let thread = logs.iter().position(|v| v["method"] == "thread/start").unwrap();
    let submitted = logs.iter().position(|v| v["method"] == "turn/start").unwrap();
    assert!(account < config && config < thread && thread < submitted);
    assert_eq!(logs[account]["params"]["refreshToken"], false);
    let start = logs.iter().find(|v| v["method"] == "thread/start").unwrap();
    assert_eq!(start["params"]["model"], "fixture-model");
    assert_eq!(
        start["params"]["developerInstructions"],
        "Fixture role instructions"
    );
    assert_eq!(start["params"]["sandbox"], "read-only");
    assert_eq!(start["params"]["approvalPolicy"], "never");
    assert_eq!(start["params"]["config"]["model_reasoning_effort"], "high");
    assert_eq!(
        start["params"]["config"]["mcp_servers"]["unsafe.remote"]["enabled"],
        false
    );
    assert_eq!(
        start["params"]["config"]["mcp_servers"]["gitlab-780"]["enabled"],
        false
    );
    assert_eq!(
        start["params"]["config"]["mcp_servers"]["quoted\"name"]["enabled"],
        false
    );
    assert!(!start["params"]["config"]
        .as_object()
        .unwrap()
        .keys()
        .any(|k| k.starts_with("mcp_servers.")));
    assert_eq!(fs::read_to_string(&config_path).unwrap(), inherited);
    assert_eq!(start["params"]["config"]["features.apps"], false);
    assert_eq!(start["params"]["config"]["features.multi_agent"], false);
    let turn = logs.iter().find(|v| v["method"] == "turn/start").unwrap();
    assert_eq!(turn["params"]["model"], "fixture-model");
    assert_eq!(turn["params"]["effort"], "high");
    assert_eq!(turn["params"]["sandboxPolicy"]["type"], "readOnly");
    assert!(logs
        .iter()
        .any(|v| v["id"] == 99 && v["result"]["decision"] == "decline"));
    assert!(!events.iter().any(|e| e.kind == "approval"));
    assert!(events.iter().any(|e| e.kind == "execution_config"
        && e.payload["verification"] == "runtime_reported"
        && e.payload["model"] == "runtime-model"));
    assert!(events.iter().any(|e| e.kind == "notice" && e.payload["state"] == "checking_account"));
    assert!(events.iter().any(|e| e.kind == "notice" && e.payload["state"] == "ready"));
    assert!(!serde_json::to_string(&events).unwrap().contains("private-account-canary"));
    codex_account_bootstrap(&fixture).await;
    let (result, events) = run(fixture.input("claude")).await;
    result.unwrap();
    let logs = fixture.log();
    let invocation = logs
        .iter()
        .find(|v| {
            v["argv"]
                .as_array()
                .is_some_and(|a| a.contains(&json!("--print")))
        })
        .unwrap();
    let args = invocation["argv"].as_array().unwrap();
    for (flag, value) in [
        ("--model", "fixture-model"),
        ("--effort", "high"),
        ("--tools", "Read,Grep,Glob"),
        ("--disallowedTools", "mcp__*"),
        ("--permission-mode", "plan"),
    ] {
        let position = args.iter().position(|a| a == flag).unwrap();
        assert_eq!(args[position + 1], value);
    }
    assert!(args.contains(&json!("--safe-mode")));
    assert!(
        invocation["env_effort"].is_null(),
        "ambient effort must not override per-agent selection"
    );
    assert!(events
        .iter()
        .any(|e| e.payload["verification"] == "runtime_reported"
            && e.payload["model"] == "claude-runtime-model"));
    final_messages_and_failures(&fixture).await;
    native_auth_scenarios(&mut fixture).await;
    permission_mode_scenarios(&fixture).await;
    devtools_bridge_scenarios(&fixture).await;
    fs::write(fixture.root.join("unsupported"), "").unwrap();
    let before = fixture.log().len();
    let (result, _) = run(fixture.input("claude")).await;
    assert!(result.unwrap_err().to_string().contains("cannot enforce"));
    assert_eq!(
        fixture.log().len(),
        before,
        "unsupported runtime must not start inference"
    );
}
async fn devtools_bridge_scenarios(fixture: &Fixture) {
    // Per-run bridge configuration must never rewrite the inherited provider setup,
    // and read-only runs must not regain modifying tools through the new bridge.
    fs::write(fixture.root.join("scenario"), "permission-completed").unwrap();
    for provider in ["codex", "claude"] {
        for read_only in [false, true] {
            let before = fixture.log().len();
            let mut input = fixture.input(provider);
            input.read_only = read_only;
            input.devtools_conversation_id = Some("fixture-conversation".into());
            input.devtools_cli = Some("/fixture/agentkit".into());
            let (result, _) = run(input).await;
            result.unwrap();
            let calls = fixture.log();
            let calls = &calls[before..];
            if provider == "codex" {
                let start = calls.iter().find(|v|v["method"]=="thread/start").unwrap();
                let bridge = &start["params"]["config"]["mcp_servers"]["studio_devtools"];
                if read_only { assert!(bridge.is_null()); }
                else {
                    assert_eq!(bridge["command"],"/fixture/agentkit");
                    assert_eq!(bridge["args"],json!(["mcp-server"]));
                    assert_eq!(bridge["env"]["AGENTKIT_DEVTOOLS_CONVERSATION"],"fixture-conversation");
                }
            } else {
                let call=calls.iter().find(|v|v["argv"].as_array().is_some_and(|a|a.contains(&json!("--print")))).unwrap();
                let args=call["argv"].as_array().unwrap();
                let position=args.iter().position(|a|a=="--mcp-config");
                if read_only { assert!(position.is_none()); }
                else {
                    let config:Value=serde_json::from_str(args[position.unwrap()+1].as_str().unwrap()).unwrap();
                    let bridge=&config["mcpServers"]["studio_devtools"];
                    assert_eq!(bridge["command"],"/fixture/agentkit");
                    assert_eq!(bridge["args"],json!(["mcp-server"]));
                    assert_eq!(bridge["env"]["AGENTKIT_DEVTOOLS_CONVERSATION"],"fixture-conversation");
                }
            }
        }
    }
    fs::remove_file(fixture.root.join("scenario")).unwrap();
}
async fn codex_account_bootstrap(fixture: &Fixture) {
    for (scenario, expected, retryable) in [
        ("account-missing", "not signed in", false),
        ("account-routing-failed", "account/routing check failed", false),
        ("account-overloaded", "before task submission", true),
        ("account-unexpected-turn", "account/routing check failed", false),
    ] {
        fs::write(fixture.root.join("scenario"), scenario).unwrap();
        let before = fixture.log().len();
        let (result, _) = run(fixture.input("codex")).await;
        let error = result.unwrap_err();
        assert!(error.to_string().contains(expected), "{scenario}: {error}");
        assert_eq!(agentkit_providers::retry_evidence(&error).is_some(), retryable);
        let calls = fixture.log();
        let calls = &calls[before..];
        assert_eq!(calls.iter().filter(|v| v["method"] == "account/read").count(), 1);
        assert!(!calls.iter().any(|v| matches!(v["method"].as_str(), Some("thread/start" | "thread/resume" | "turn/start"))), "{scenario}: task submitted after failed account check");
    }
    // A no-auth custom/API provider remains supported, including resumed writes.
    fs::write(fixture.root.join("scenario"), "account-no-auth").unwrap();
    let before = fixture.log().len();
    let mut input = fixture.input("codex");
    input.read_only = false;
    input.resume_session = Some("fixture-resume".into());
    run(input).await.0.unwrap();
    let calls = fixture.log();
    let calls = &calls[before..];
    let account = calls.iter().position(|v|v["method"] == "account/read").unwrap();
    let resumed = calls.iter().position(|v|v["method"] == "thread/resume").unwrap();
    let turn = calls.iter().position(|v|v["method"] == "turn/start").unwrap();
    assert!(account < resumed && resumed < turn);
    assert!(!calls.iter().any(|v|v["method"] == "config/read"));

    // Reconnects after submission remain native turn events, not safe task replays.
    fs::write(fixture.root.join("scenario"), "routing-post-turn").unwrap();
    let (result, events) = run(fixture.input("codex")).await;
    result.unwrap();
    assert!(events.iter().any(|e|e.kind == "error" && e.payload.pointer("/raw/params/willRetry") == Some(&json!(true))));
    let terminal = events.iter().rev().find(|e|e.kind == "status").unwrap();
    assert_eq!(terminal.payload["status"], "failed");
    assert_eq!(terminal.payload["error"], "workspace routing discovery timed out");
    assert!(terminal.payload.get("retry").is_none());

    // Stop stays responsive while account discovery is pending, before any turn.
    fs::write(fixture.root.join("scenario"), "account-hold").unwrap();
    let before = fixture.log().len();
    let (tx, mut rx) = mpsc::channel(64);
    let (controls, cr) = mpsc::channel(4);
    let input = fixture.input("codex");
    let running = tokio::spawn(async move { agentkit_providers::run(input, "fixture".into(), tx, cr).await });
    while let Some(event) = rx.recv().await {
        if event.kind == "notice" && event.payload["state"] == "checking_account" { break; }
    }
    controls.send(agentkit_protocol::ProviderControl::Stop).await.unwrap();
    tokio::time::timeout(Duration::from_secs(3), running).await.unwrap().unwrap().unwrap();
    assert!(!fixture.log()[before..].iter().any(|v|matches!(v["method"].as_str(),Some("thread/start" | "turn/start"))));
    fs::remove_file(fixture.root.join("scenario")).unwrap();
}
async fn final_messages_and_failures(fixture: &Fixture) {
    for provider in ["codex", "claude"] {
        for scenario in [
            "final-only",
            "turn-only",
            "stream-and-final",
            "failed",
            "failed-notification",
            "failed-without-diagnostic",
        ] {
            fs::write(fixture.root.join("scenario"), scenario).unwrap();
            let (outcome, events) = run(fixture.input(provider)).await;
            outcome.unwrap();
            let terminal = events.iter().rev().find(|e| e.kind == "status").unwrap();
            if scenario.starts_with("failed") {
                assert_eq!(terminal.payload["status"], "failed");
                let error = terminal.payload["error"].as_str().unwrap();
                assert!(!error.is_empty());
                if scenario == "failed" || scenario == "failed-notification" {
                    assert!(
                        error.contains("fixture model unavailable"),
                        "{provider}: {error}"
                    );
                }
                // A terminal provider failure has no pre-turn retry evidence.
                assert!(terminal.payload.get("retry").is_none());
            } else {
                assert_eq!(terminal.payload["status"], "completed");
                let messages = events
                    .iter()
                    .filter(|e| e.kind == "message")
                    .collect::<Vec<_>>();
                assert_eq!(
                    messages.len(),
                    1,
                    "final item repeated in turn snapshot must not duplicate canonical text"
                );
                assert_eq!(messages[0].payload["text"], "NONCE-123 — نیم‌فاصله English");
            }
        }
    }
    fs::remove_file(fixture.root.join("scenario")).unwrap();
}
async fn native_auth_scenarios(fixture: &mut Fixture) {
    let conflicts = [
        "ANTHROPIC_API_KEY",
        "ANTHROPIC_AUTH_TOKEN",
        "ANTHROPIC_BASE_URL",
        "ANTHROPIC_CUSTOM_HEADERS",
        "ANTHROPIC_FOUNDRY_BASE_URL",
        "ANTHROPIC_VERTEX_PROJECT_ID",
        "CLAUDE_CODE_USE_BEDROCK",
        "CLAUDE_CODE_USE_VERTEX",
        "CLAUDE_CODE_USE_FOUNDRY",
        "CLAUDE_CODE_USE_MANTLE",
        "CLAUDE_CODE_USE_ANTHROPIC_AWS",
        "CLAUDE_CODE_SKIP_BEDROCK_AUTH",
        "AWS_BEARER_TOKEN_BEDROCK",
        "CLAUDE_CODE_ENABLE_GATEWAY_MODEL_DISCOVERY",
    ];
    for key in conflicts
        .into_iter()
        .chain(["CLAUDE_CODE_OAUTH_TOKEN", "HTTPS_PROXY"])
    {
        fixture.old.push((key.into(), std::env::var_os(key)));
        std::env::set_var(
            key,
            if key == "CLAUDE_CODE_OAUTH_TOKEN" {
                "fixture-native-oauth"
            } else {
                "fixture-routing-value"
            },
        );
    }
    // Configured mode keeps intentional existing gateway/cloud behavior.
    let mut input = fixture.input("claude");
    input.claude_auth_mode = Some(ClaudeAuthMode::Configured);
    let (result, events) = run(input).await;
    result.unwrap();
    let logs = fixture.log();
    let call = logs.iter().rev().find(|v| v.get("argv").is_some()).unwrap();
    assert_eq!(
        call["route_keys"].as_array().unwrap().len(),
        conflicts.len()
    );
    assert!(!call["argv"]
        .as_array()
        .unwrap()
        .contains(&json!("--setting-sources")));
    assert!(events
        .iter()
        .any(|e| e.kind == "execution_config" && e.payload["claude_auth_mode"] == "configured"));
    // Native mode applies to write-capable runs too; it is not tied to safe-mode.
    let mut input = fixture.input("claude");
    input.read_only = false;
    input.claude_auth_mode = Some(ClaudeAuthMode::Native);
    input.resume_session = Some("fixture-resume".into());
    let (result, events) = run(input.clone()).await;
    result.unwrap();
    let logs = fixture.log();
    let call = logs.iter().rev().find(|v| v.get("argv").is_some()).unwrap();
    assert!(call["route_keys"].as_array().unwrap().is_empty());
    assert_eq!(call["oauth_preserved"], true);
    assert_eq!(call["config_dir_preserved"], true);
    assert_eq!(call["proxy_preserved"], true);
    let args = call["argv"].as_array().unwrap();
    let index = args.iter().position(|v| v == "--setting-sources").unwrap();
    assert_eq!(args[index + 1], "");
    assert!(!args.contains(&json!("--bare")));
    assert!(args.contains(&json!("--resume")));
    assert!(events
        .iter()
        .any(|e| e.kind == "execution_config" && e.payload["claude_auth_mode"] == "native"));
    // Losing isolation support fails closed before launching a model request.
    fs::write(fixture.root.join("unsupported-native"), "").unwrap();
    let before = fixture.log().len();
    let (result, _) = run(input).await;
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("--setting-sources"));
    assert_eq!(fixture.log().len(), before);
    fs::remove_file(fixture.root.join("unsupported-native")).unwrap();
    // Parent environment stays intact: only the child command is filtered.
    for key in conflicts {
        assert!(std::env::var_os(key).is_some());
    }
}
const FIXTURE: &str = r##"#!/usr/bin/env python3
import json,os,sys
from pathlib import Path

def emit(v): print(json.dumps(v),flush=True)
def record(v):
    with open(os.environ['STUDIO_TRANSPORT_LOG'],'a') as f: f.write(json.dumps(v)+'\n')
if 'generate-json-schema' in sys.argv:
    schema=Path(sys.argv[sys.argv.index('--out')+1])/'v2';schema.mkdir(parents=True)
    for name in ['ThreadStartParams','ThreadResumeParams','TurnStartParams']:
        value={'properties':{'approvalsReviewer':{}},'definitions':{'ApprovalsReviewer':{'enum':['user','auto_review']}}}
        if Path(__file__).with_name('unsupported-auto').exists():value={}
        (schema/(name+'.json')).write_text(json.dumps(value))
    sys.exit(0)
if '--help' in sys.argv:
    print(('--safe-mode --tools --disallowedTools --permission-mode --permission-prompts host auto bypassPermissions default' if not Path(__file__).with_name('unsupported').exists() else '--tools') + (' --setting-sources' if not Path(__file__).with_name('unsupported-native').exists() else ''))
    sys.exit(0)
record({'argv':sys.argv[1:],'env_effort':os.environ.get('CLAUDE_CODE_EFFORT_LEVEL'),'route_keys':[k for k in os.environ if k.startswith('ANTHROPIC_') or k in ['CLAUDE_CODE_USE_BEDROCK','CLAUDE_CODE_USE_VERTEX','CLAUDE_CODE_USE_FOUNDRY','CLAUDE_CODE_USE_MANTLE','CLAUDE_CODE_USE_ANTHROPIC_AWS','CLAUDE_CODE_SKIP_BEDROCK_AUTH','AWS_BEARER_TOKEN_BEDROCK','CLAUDE_CODE_ENABLE_GATEWAY_MODEL_DISCOVERY']],'oauth_preserved':os.environ.get('CLAUDE_CODE_OAUTH_TOKEN')=='fixture-native-oauth','config_dir_preserved':os.environ.get('CLAUDE_CONFIG_DIR')==str(Path(__file__).parent/'.claude'),'proxy_preserved':os.environ.get('HTTPS_PROXY')=='fixture-routing-value'})
scenario_path=Path(__file__).with_name('scenario')
scenario=scenario_path.read_text() if scenario_path.exists() else ''
answer='NONCE-123 — نیم‌فاصله English'
if 'app-server' in sys.argv:
    account_checked=False
    for line in sys.stdin:
        v=json.loads(line);record(v);method=v.get('method')
        if method=='initialize': emit({'id':v['id'],'result':{}})
        elif method=='account/read':
            if scenario=='account-hold': continue
            if scenario=='account-routing-failed': emit({'id':v['id'],'error':{'code':-32603,'message':'workspace routing discovery timed out'}})
            elif scenario in ('account-overloaded','account-unexpected-turn'):
                if scenario=='account-unexpected-turn': emit({'method':'turn/started','params':{'turn':{'id':'unexpected'}}})
                emit({'id':v['id'],'error':{'code':-32001,'message':'Server overloaded; retry later.'}})
            else:
                account_checked=True
                emit({'id':v['id'],'result':{'account':None if scenario in ('account-missing','account-no-auth') else {'type':'chatgpt','email':'private-account-canary'},'requiresOpenaiAuth':scenario!='account-no-auth','workspaceRouting':{'backendOrigin':'private-account-canary'}}})
        elif method=='config/read': emit({'id':v['id'],'result':{'config':{'mcp_servers':{name:{'url':'http://127.0.0.1:1/never','enabled':True} for name in ['unsafe.remote','gitlab-780','quoted"name']}}}})
        elif method in ('thread/start','thread/resume'):
            if not account_checked:
                emit({'id':v['id'],'error':{'code':-32603,'message':'account routing was not initialized'}})
                continue
            config=v['params'].get('config',{})
            if any(k.startswith('mcp_servers.') for k in config):
                emit({'id':v['id'],'error':{'code':-32600,'message':'invalid transport from quoted dotted MCP override'}})
            else: emit({'id':v['id'],'result':{'thread':{'id':'fixture'},'model':'runtime-model','reasoningEffort':'high'}})
        elif method=='turn/start':
            emit({'id':v['id'],'result':{'turn':{'id':'turn'}}})
            if scenario.startswith('review-'):
                emit({'method':'item/autoApprovalReview/started','params':{'review':{'status':'inProgress'}}})
                emit({'method':'item/autoApprovalReview/completed','params':{'review':{'status':scenario.removeprefix('review-')}}})
            if scenario=='routing-post-turn':
                emit({'method':'error','params':{'error':{'message':'Reconnecting... 2/5'},'willRetry':True}})
                emit({'method':'turn/completed','params':{'turn':{'status':'failed','error':{'message':'workspace routing discovery timed out'}}}})
                continue
            if scenario:
                if scenario.startswith('failed'):
                    turn={'status':'failed'}
                    if scenario=='failed': turn['error']={'message':'fixture model unavailable','codexErrorInfo':'modelUnavailable'}
                    if scenario=='failed-notification': emit({'method':'error','params':{'error':{'message':'fixture model unavailable'}}})
                    emit({'method':'turn/completed','params':{'turn':turn}})
                else:
                    item={'type':'agentMessage','id':'final-1','text':answer,'phase':'final_answer'}
                    if scenario=='stream-and-final': emit({'method':'item/agentMessage/delta','params':{'threadId':'fixture','delta':'NONCE-123 — '}})
                    if scenario!='turn-only': emit({'method':'item/completed','params':{'threadId':'fixture','item':item}})
                    emit({'method':'turn/completed','params':{'threadId':'fixture','turn':{'status':'completed','items':[item]}}})
            else: emit({'id':99,'method':'item/fileChange/requestApproval','params':{'reason':'Should be denied'}})
        elif v.get('id')==99:
            emit({'method':'turn/completed','params':{'turn':{'status':'completed'}}})
else:
    record(json.loads(sys.stdin.readline()))
    emit({'type':'system','subtype':'init','session_id':'fixture','model':'claude-runtime-model','effort':'high'})
    if scenario.startswith('failed'):
        if scenario=='failed-notification': emit({'type':'error','error':{'message':'fixture model unavailable'}})
        emit({'type':'result','session_id':'fixture','subtype':'error_during_execution','is_error':True,'errors':['fixture model unavailable'] if scenario=='failed' else []})
    elif scenario=='claude-permission':
        emit({'type':'control_request','request_id':'claude-permission-1','request':{'subtype':'can_use_tool','tool_name':'Bash','input':{'command':'fixture command'},'decision_reason':'Fixture action needs approval'}})
        record(json.loads(sys.stdin.readline()))
        emit({'type':'result','session_id':'fixture','result':'Permission resolved','is_error':False})
    elif scenario:
        if scenario=='stream-and-final': emit({'type':'stream_event','session_id':'fixture','event':{'delta':{'text':'NONCE-123 — '}}})
        emit({'type':'assistant','session_id':'fixture','parent_tool_use_id':None,'message':{'content':[{'type':'text','text':answer}]}})
        emit({'type':'assistant','session_id':'fixture','parent_tool_use_id':'child','message':{'content':[{'type':'text','text':'Do not replace root final answer'}]}})
        emit({'type':'result','session_id':'fixture','subtype':'success','is_error':False})
    else: emit({'type':'result','session_id':'fixture','result':'done','is_error':False})
"##;
