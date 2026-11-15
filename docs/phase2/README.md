# Studio conversations and orchestration

Studio keeps projects, conversations, messages and execution tasks in its local daemon. The desktop and CLI use that same daemon; only it opens the SQLite history database. A conversation survives individual provider sessions and can use Codex and Claude workers.

## Using the desktop

Start a conversation and send your request, with or without a project. Open the composer’s **+** menu to attach files or choose a project folder or **No project**. You can attach, move or detach an existing chat after its active request or pending plan ends; history stays in that chat. Project-free chats use private per-chat storage and require an attached project before file changes can be proposed. New conversation starts without a project. Inside a project, the composer shows **Superagent** and the internal coordinator proposes a structured task graph or answers directly. Without a project, the composer shows compact model and effort dropdowns. Each message goes directly to the selected connected provider, with no planning, delegation or summarization step. The selection is stored separately for each conversation; changing it affects future messages. Attaching a project hides the manual controls; detaching it restores the saved direct-chat selection. A disconnected saved provider produces an actionable error rather than an automatic model switch. Read-only work can proceed immediately. Plans containing writes wait for approval before their workers start. You can stop a task or the whole request. Provider permission requests remain separate from approval of the overall plan.

Use the sort icon beside **All conversations** to sort by latest activity, creation date or title and choose the direction. Project/chat removal controls appear on hover or when their buttons receive keyboard focus. Deleting a project removes its Studio entry and detaches its chats; it never deletes the project folder. Deleting a chat requires confirmation. Active requests and pending plans must end before deletion.

The activity panel displays direct-chat executions, coordinator and worker tasks, their requested model/effort, dependencies, results and errors. Requested settings are not proof that a native runtime actually selected that model. Native execution evidence must be reported separately.

Agents shows one logical role per installed name, even when the same role exists in both Codex and Claude. A role has a stable `agent:<name>` identity; its native definitions remain available as sources, and project definitions still shadow user definitions within each runtime. Existing native identifiers are retained as aliases for compatibility. Studio settings never rewrite installed role definitions. The internal coordinator remains separate from the installed supervisor role.

Each role can select a model from any provider connected to Studio. Models are identified by both provider and model ID; effort choices belong to that pair. A provider switch does not carry a foreign native session or agent identifier into the new runtime. Automatic-selection options use explicit provider/model/effort combinations. Model metadata persists in the daemon-owned SQLite history rather than the frontend session. Disconnected providers are excluded from selectable catalogs even if their CLIs remain installed.

Catalog provenance remains visible: Codex can advertise runtime models and supported efforts on explicit refresh; Claude metadata currently comes from installed definitions and is not a verified runtime inventory. Neither installed nor advertised metadata proves successful account execution. Refreshing metadata does not submit a model turn.

Connections has **Connect to Studio** and **Disconnect from Studio** controls. These enable or disable Studio execution without signing out of either account. A fresh profile starts disconnected; existing saved connection preferences are preserved. Stop active or queued work for that provider before disconnecting. A disconnected superagent cannot start a request; reconnect it or choose a connected runtime in Preferences. Legacy CLI execution and resume follow the same connection setting.

Connections probes native executable compatibility without running a model. Execution and probes use the same native executable resolver, including macOS user installations in `~/.local/bin`. Missing executables, permission errors, probe timeouts and missing required capabilities have distinct diagnostics. Explicit model-catalog refresh can query Codex account/catalog metadata; advertised models are not marked as successfully executed. Existing native CLI sign-in is used. No new API key or separate ChatGPT inference worker is required. In Connections, Claude can keep its existing configuration (the backward-compatible default) or use the official Claude account. Native mode uses existing OAuth sign-in and ignores custom gateway/API/cloud routing for new runs, without rewriting installed settings. Log in with the native CLI first; Studio does not collect passwords or tokens.

## Execution and recovery

The initial limit is four active provider executions across conversations, including coordinator calls. The setting accepts 1–16. Independent read-only tasks can overlap; writes to the same canonical project path are serialized. This is not a guarantee against changes made by unrelated external processes.

The scheduler validates structured proposals, agent identities, allowed selections, file scopes and dependency graphs. Failed dependencies cancel dependent work. Managed tasks can retry at most twice after adapter-confirmed transient failures before execution, with backoff that releases the global slot. Codex model-unavailable startup errors can fall back to an untried model in the same agent’s explicit allowed set. Attempts and changes are recorded in Activity and the conversation. Policy is checked again at launch; stop cancels waiting retries. Submitted, stopped or interrupted operations are not automatically replayed. After daemon restart, unfinished work is marked interrupted and must be restarted by the user.

The first upgrade makes a SQLite backup named `history.pre-studio-v1-<id>.sqlite3`, then migrates in a transaction. Legacy run/event identifiers remain available to CLI commands and imported conversations. Backups remain in the engine data directory.

## CLI and MCP

Existing `run`, `runs`, `status`, `events`, `stop`, `approve` and `resume` commands remain available. `run` additionally accepts `--model` and `--effort`.

The `rpc` command exposes Studio methods for scripts, for example:

```sh
agentkit --json rpc studio/projects
agentkit --json rpc studio/conversations --params '{"sort":"updated_at","direction":"desc"}'
agentkit --json rpc studio/model-catalog
agentkit --json rpc studio/settings
agentkit --json rpc studio/agents --params '{"workspace":"/absolute/project"}'
agentkit --json rpc studio/conversation/create --params '{"project_id":null}'
agentkit --json rpc studio/conversation/selection --params '{"conversation_id":"CHAT_ID","selection":{"provider":"codex","model":null,"effort":null}}'
agentkit --json rpc studio/conversation/project --params '{"conversation_id":"CHAT_ID","project_id":null}'
agentkit --json rpc studio/connection/set --params '{"provider":"codex","enabled":false}'
agentkit --json rpc studio/models --params '{"provider":"codex","workspace":"/absolute/project"}'
```

Enable external MCP access in Connections before configuring a compatible client. Use the absolute path of this workspace's CLI, with arguments `["mcp-server"]`. If you use `AGENTKIT_DATA_DIR`, configure that same value on the MCP process. The MCP bridge delegates through the Studio conversation workflow; it does not grant approval or bypass the scheduler. No external client configuration is modified automatically.

## Verification

Run the complete isolated check suite with installed dependencies and a Rust toolchain:

```sh
python3 -I scripts/verify-isolated.py
```

`CARGO_HOME`, `RUSTUP_HOME` and `CARGO_TARGET_DIR` can point to existing offline caches. The script isolates application homes and removes inherited provider credentials. Integration suites use fake native protocols, never real model inference. Native desktop, actual account/model execution and each supported operating system require separate verification. See VERIFICATION.md for the results recorded for this change.
