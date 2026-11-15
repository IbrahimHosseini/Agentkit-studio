# Verification — 2026-11-15

## Current update: MCP names, optional projects, connections and dropdowns

Implemented by three approved workers without a supervisor. Root integration review found and corrected default model/effort changes conflicting with nonempty allowed sets; four frontend helper tests cover option/default behavior, selection limits and preserving unsaved settings during connection toggles. No package changes were needed: the approved TOML parser addition was abandoned once the actual MCP cause was identified.

- **MCP root cause:** the installed `gitlab-780` definition had a valid URL transport. Studio's quoted dotted override introduced a literal quoted server name with no transport. A nested `mcp_servers` map preserves names and disables the intended server. An isolated installed-Codex bootstrap reproduced the old protocol error and successfully started a thread with the corrected override, retaining inherited model/effort and byte-identical test configuration. It did not send a turn or read account credentials. Genuine malformed installed configuration is not silently rewritten.
- **Automated checks:** 24 Rust tests, 12 frontend helper tests, 10 phase-one and 36 phase-two Python tests: **82 passed**. Svelte check: zero errors/warnings. Frontend, CLI and macOS release builds passed with isolated homes and existing offline dependencies.
- **Native macOS fixture verification:** project-free request/reply; attaching and removing a project while retaining history; both Studio connection toggles; shared catalog display; selecting/saving effort; settings/catalog persistence across chat navigation; New conversation resetting to no project. The full release bundle initially opened blank, then loaded after a delay; startup timing remains a known limitation. See `target/phase2/native-features.json`.
- **Provider boundaries:** no real-account Codex or Claude inference was performed for this update. Claude catalog values come from installed definitions, not an advertised live runtime inventory. Previous live successes and the later Codex routing timeout below remain historical evidence, not verification of this final binary.
- **Platforms:** tested macOS arm64. Windows/Linux unverified. No installed agents/settings were rewritten, and no commit or push was made.

Current artifacts: `target/phase2/Agentkit Studio.app`, `agentkit`, `sha256.json`, `verification-summary.json`, `verification.log`, `release-build.log`. Standalone and bundled CLI hashes match. Restart an older running Studio instance before opening this bundle.

## Real-connection follow-up (explicit supervisor authorization)

The user explicitly authorized supervisor PLAN/REVIEW and one additional inference test per provider. The evidence harness now retains redacted JSONL events, complete final output, native-provider stderr and daemon stderr before removing isolated credentials/state. Eleven offline harness tests cover exact matching, setup failure, failed result plus stderr, timeout, truncation, collection failure, redaction and native OAuth handoff.

- Codex additional test: **passed**. Exactly one Studio submission; runtime-reported model `gpt-6.1-sol`; exact random file code and full Persian line matched; fixture and original authentication unchanged. A native runtime reconnect warning (`workspace routing discovery timed out`) recovered within the same turn; Studio did not submit another turn.
- Claude diagnosis before inference: the configured gateway `127.0.0.1:8080` is served by Docker container `dynamic_price-api-1` (`dynamic_price/backend:dev`). `/v1/models` returned 404. The native Keychain record had neither an access token nor a refresh token. This is configuration/authentication evidence, not a model-inference failure.
- After the user completed official Claude login, the additional Claude test **passed** on 2026-11-15. Exactly one Studio submission using Claude Code `2.1.310`, native OAuth and configured `sonnet`; runtime reported `claude-sonnet-5-5`. The exact random file code and complete Persian/English sample matched, with no provider errors and empty provider stderr. The fixture and installed settings were unchanged; temporary credentials/state were removed. No retry or fallback was needed or attempted for this live test.
- Studio offers a separate native OAuth route without rewriting installed settings. Select **Official Claude account** in Connections and save to use it; existing settings retain their configured route until explicitly changed. The original failure was traced to the unrelated local HTTP service and missing native CLI tokens, rather than inferred from the old incomplete model output. Signing in to Claude Desktop alone did not provide usable CLI credentials here.
- Supervisor integration review: **APPROVED**, with independent isolated native/configured environment and resume checks. The successful live binary's SHA-256 is retained in `claude.json`. The final bundle additionally contains the offline scheduler fix described below; it was not subjected to another live request.
- The successful Claude run retained three daemon scheduler warnings (`invalid type: null, expected a sequence`), while provider stderr was empty. A deterministic offline regression reproduced them: a newly imported CLI run could still be queued when the managed scheduler selected its legacy mirror. The scheduler now observes legacy tasks without admitting them as new executions. The regression verifies repeated ticks leave the original run/task intact and launch nothing. The original evidence is preserved unchanged in `claude.evidence/daemon.stderr.log` and `queued-cli-regression-before.log`.

Current evidence is under `target/phase2/live-diagnostics/`; `codex.json`, `claude.json` and their `.evidence/` directories contain the successful real results, full response text, redacted events and separate provider/daemon stderr. No secret values or full credential-bearing database are retained.

## Environment and automated results

Tested on macOS, Darwin arm64. Windows and Linux were not tested. No new dependency installation, installed configuration modification, commit or push was used. Supervisor use is recorded separately above; the earlier implementation did not use it. One live request per provider was subsequently explicitly authorized; see the separate results below.

`python3 -I scripts/verify-isolated.py` completed successfully with existing offline Cargo caches. Application HOME, CODEX_HOME, Claude configuration and engine state were isolated; inherited provider credentials were excluded.

| Check | Result |
| --- | --- |
| Rust workspace tests (`--locked --offline`) | 28 passed |
| Svelte/TypeScript check | 0 errors, 0 warnings |
| Frontend production build | Passed |
| CLI build | Passed |
| Phase-one Python integration suite | 10 passed |
| Phase-two Python integration suite | 49 passed |
| Attachment and model-setting helper tests (Node) | 17 passed |
| macOS release application bundle | Built successfully |

The 104 tests include fake Codex App Server and Claude Code subprocesses, native adapter flags and compatibility rejection, mixed-provider orchestration, direct answers and conversation continuation, concurrent conversations, model/dependency rejection, approval and denial, stop/restart, hard-crash recovery without replay, historical migration with backup, separate settings and MCP enablement/approval boundaries. Native authentication isolation, durable live-evidence collection, queued CLI mirroring, optional project membership, connection gates and model dropdowns also have offline coverage. These fixtures verify protocol and engine behavior, not compatibility with every installed runtime version or account state.

Build artifacts and complete logs are in `target/phase2/`: `Agentkit Studio.app`, `agentkit`, `verification.log`, `release-build.log` and `sha256.json`. The bundled CLI hash matches the standalone release CLI. The app is a local build; distribution signing/notarization was not verified.

## Interactive verification

Browser preview: inspected the disconnected shell and layout through the local development server.

Native macOS: interacted with an isolated debug Tauri application using fixture providers and the development frontend. Verified selecting a conversation, Persian/English mixed input with half-spaces and emoji, multiline editing and undo, the native file chooser with a Persian filename, attachment display, sending a request, completion in the same conversation, agent defaults, connection compatibility and explicit advertised-model loading. The release bundle was subsequently opened with isolated fixture state and served its embedded interface at `tauri://localhost`. A Persian direct-answer request completed in the release bundle using a fake provider. It initially showed an empty window before loading; startup diagnostics were added, but no deterministic root cause for the initial delay was established.

Native image clipboard was verified by copying actual pixels from Preview, pasting into Studio, removing an unsent image, sending a second pasted image and reopening the conversation. The removed image was absent from scratch storage; the sent image remained present. Drag/drop was verified with user assistance: the user dropped `note فارسی.txt` from Finder into native Studio QA; its attachment chip appeared, a fake-provider request completed, and the filename remained in conversation history. Automated cross-window drag could not be completed reliably, so the action is explicitly recorded as user-assisted. External ChatGPT/Claude/Codex MCP client integration was not exercised.

## Earlier live smoke (historical; superseded by the follow-up above)

The user approved exactly one short read-only request to Codex and one to Claude, using existing authentication and a synthetic file, with no automatic retries. Both attempts were made; no further live request was made after the adapter fixes.

- Codex: runtime reported completed, but the expected file nonce and Persian sample were not verified. This is not an end-to-end acceptance pass. The original authentication file hash was unchanged.
- Claude: runtime reported failed; the original smoke report did not retain a diagnostic. The failure cause cannot be established from the retained evidence.
- Both synthetic files remained unchanged and temporary credentials/state were removed.
- Subsequent source review confirmed missing canonical final-only Codex text and missing normalized terminal failure diagnostics in both adapters. These omissions were fixed and covered with offline subprocess fixtures. They are confirmed code defects, but cannot retrospectively be proven to explain the deleted live evidence.

Historical reports: `target/phase2/live-codex.json`, `target/phase2/live-claude.json`, and `target/phase2/native-clipboard.json`. The separately authorized follow-up reports are in `target/phase2/live-diagnostics/`. Both follow-up inference allowances have now been used; further live tests require new explicit authorization.

## Remaining limits

- All writing plans require approval, including small writes. This is deliberately stricter than approval only for broad changes.
- Managed tasks allow at most three attempts, with 500 ms then 1,000 ms backoff plus 0–200 ms jitter. Only typed adapter evidence before turn/prompt submission allows retry. Codex startup overload/shutdown can retry; structured model-unavailable errors can select an untried model in the current allowlist, preserving effort and provider. Claude only retries eligible transient process-spawn errors. Once input is submitted, errors are ambiguous and require user follow-up. Standalone legacy CLI runs retain their non-replaying behavior.
- Approved file scopes are validated and passed to native adapters as execution instructions. They are not an operating-system per-file write sandbox. The single-writer scheduler prevents overlapping Studio writers for the same canonical project, but cannot control unrelated processes.
- Git changes are bounded before/after observations and may include external edits. Non-Git projects rely on reported output, with comparison availability stated explicitly.
- Hard-killing the daemon can leave a provider descendant alive. Recovery marks unfinished work interrupted and does not rerun it; the crash fixture explicitly cleans up only its own provider process groups.
- Removing staged clipboard images checks history references. Images abandoned by a crash are not automatically pruned; the existing storage quota still applies.
- Runtime-advertised models and requested model/effort are not evidence of successful real execution. Actual values are displayed only when native protocol evidence is available.
- Agent definition parsing supports the documented common TOML/YAML forms; unsupported definitions are surfaced as invalid rather than guessed.

Recovery and attachment regression checks passed. Real-account execution and native scenarios are reported separately; fixture passes do not certify untested platforms or runtime/account combinations.


## Finder Codex executable fix — 2026-11-15

Codex execution and capability/catalog probes now share a resolver: explicit override first, executable absolute PATH candidates, user .local/bin, and standard macOS install locations. This fixes Finder's restricted PATH missing the installed CLI. An isolated regression verifies user-install fallback and PATH/override precedence. All 72 offline tests and the rebuilt macOS bundle passed.

The user authorized a new Codex-only live check. The release CLI ran with Finder-like PATH and no AGENTKIT_CODEX_BIN override, discovering an isolated .local/bin proxy to the installed native runtime. One read-only gpt-6-astra/high request reached turn/started, confirming executable resolution. Catalog discovery timed out; native runtime reported workspace routing discovery timed out and reconnect attempts. The harness cancelled at its 120-second limit without a verified final answer. This is NOT an end-to-end live acceptance pass. Evidence: target/phase2/live-diagnostics/codex-finder.json and codex-finder.evidence/. No Claude live call was made. This was a daemon/adapter test, not a new native GUI interaction test. The updated bundle is target/phase2/Agentkit Studio.app; previously launched daemons must exit before the updated engine takes over.


## Native startup fix — 2026-11-15

The main window now starts hidden and is revealed only after Svelte finishes mounting the application shell. Startup readiness does not wait for the daemon or provider connections. The HTML loading fallback stays until mounting completes. A native 20-second watchdog reports frontend startup failure rather than leaving an invisible app indefinitely; mutually exclusive ready/failed transitions prevent a late readiness callback from reopening a failed startup. Two Rust tests cover these transitions. The native error dialog itself was not visually exercised.

Six isolated release-app launches on macOS arm64 passed: two fresh application profiles, three warm launches and one missing-engine launch. The interface mounted before the window was shown in every case; process-to-visible timings were 347–1,294 ms. A separate native screenshot/accessibility inspection confirmed the complete rendered conversation interface. Fresh profiles do not mean flushed OS/WebKit caches. The earlier prolonged blank delay was not deterministically reproduced, so these results establish readiness ordering and repeated-launch success on this machine, not a universal WebKit timing guarantee.

Evidence: `target/phase2/startup-native.json` and its six adjacent logs, `startup-build.log`, and `verification.log`. Reproduce with `python3 scripts/macos-startup-smoke.py --native --bundle "target/phase2/Agentkit Studio.app" --binary target/phase2/agentkit --report target/phase2/startup-native.json`. This harness uses isolated fake providers and does not submit real model requests. Set `AGENTKIT_STARTUP_DIAGNOSTICS=1` to print only lifecycle stage names and elapsed milliseconds to stderr.

All 84 automated tests, Svelte checking, frontend/CLI builds and the macOS release build passed. No supervisor or real Codex/Claude request was used for this startup fix. Windows/Linux and distribution signing/notarization remain unverified.


## Chat management, unified roles and Claude discovery — 2026-11-15

Implemented confirmed chat deletion (including related provider runs/events to prevent restart resurrection), project removal with chat detachment and file preservation, deterministic chat sorting, and persistent frontend sort preferences. Logical roles are deduplicated by name; native definitions remain source metadata and legacy IDs remain aliases. Provider/model identity and effort constraints are enforced by the daemon for settings, admission and retry. Catalog metadata is persisted as a new entity kind in the existing versioned SQLite store, so this change requires no structural schema migration. A failed refresh retains the previous catalog and marks it stale. Fresh profiles require explicit Studio connection; existing saved preferences remain intact.

Claude execution and compatibility probes now share native executable discovery, including `~/.local/bin` symlinks used by the installed macOS CLI. Error details distinguish missing executable/interpreter, permission denial, failed exit, empty or oversized output, timeout and missing capabilities. No installed CLI or real account was exercised for this change.

Full isolated validation passed: 28 Rust tests, 17 Node helper tests, 10 phase-one and 49 phase-two Python tests (104 total); Svelte check reported zero errors/warnings, frontend and CLI builds passed, and the final macOS app bundle was rebuilt. Evidence: `target/phase2/features-verification.log`, `features-build.log`, `verification-summary.json`, and `sha256.json`.

Native macOS 27.2.1 arm64 verification used the final release bundle with fake provider executables, isolated homes, three fixture chats and 32 native definitions representing 16 role names. Confirmed title sorting in both directions, chat deletion confirmation and removal, project removal confirmation, retention of both project chats as project-free chats, 16 agent cards with two sources each, combined Codex/Claude model choices, and resetting effort when switching from Codex to Claude. Claude's effort menu contained only runtime default and the installed `high` value. A read-only QA daemon check confirmed the project list was empty, both retained chats were detached, the sentinel project file remained unchanged and the logical role count was 16.

The computer-control tool timed out during the subsequent effort-selection action; native save/reopen and connection-button toggling were not completed in this pass. Those persistence and connection flows passed isolated API/fixture tests. Earlier in this pass, a long tool timeout also outlived the first disposable preview. These are recorded as tooling limits, not successful UI checks. Evidence: `target/phase2/features-native.json`. Browser preview, real provider authentication/inference, Windows and Linux were not tested for this change. No supervisor, new packages, installed-setting changes, commit or push were used.


## Compact controls and agent spacing — 2026-11-15

Moved project selection into the composer + disclosure alongside attachments. Replaced the always-visible sidebar sort fields with an icon beside All conversations; the disclosure contains sort field and direction. Both disclosures close on outside click or Escape. Delete buttons are hidden at rest, revealed on row hover or keyboard focus on the button (and remain available on touch). Added 24px separation between the agent grid and model catalogs, with cards aligned at their tops.

Svelte check passed with zero errors/warnings; frontend production and native macOS release builds passed. An isolated release-app inspection verified the plus menu, Escape dismissal, sort disclosure, outside-click dismissal, hidden delete buttons at rest and agent/catalog spacing. Native pointer hover transition was not independently exercised by automation. No new backend or provider behavior changed, so the earlier full-suite results above remain historical validation; that suite was not rerun for this UI-only change. No real provider requests, supervisor, package changes, commits or pushes. Windows/Linux were not tested. Evidence: target/phase2/design-build.log, design-verification.json and updated sha256.json.


## Direct model chat outside projects — 2026-11-15

Project conversations retain Superagent orchestration. Project-free conversations now run one read-only direct task using their stored provider/model/effort selection, without a planner, worker delegation or summarizer. Compact model/effort controls appear in the composer only outside projects. The daemon validates selections against connected catalogs when saving, sending and starting queued work; disconnected choices remain saved but cannot execute. Selection changes affect future requests and are rejected during active work. Attaching a project hides manual controls; detaching restores the prior selection. Historical direct responses retain their provider/model attribution. Bounded persisted history and attachment references carry context across fresh provider sessions. No automatic model fallback occurs for a direct chat.

Added studio/chat-defaults and studio/conversation/selection API methods, optional project-free create/send selections, and per-conversation chat_selection in the existing transactional entity store. No structural schema or dependency change was required.

Native verification exposed a duplicate-admission bug during Claude's pre-execution help probe: its already-owned run was still queued and reconciliation made it eligible for admission again. Scheduling now requires a null run_id for a new admission; managed tasks with owned queued runs stay running. Authorized retries explicitly clear run_id. A deterministic Rust test and a slow-start Claude fixture verify one admission. The initial failure evidence is retained separately rather than overwritten.

Final isolated suite passed 29 Rust, 19 Node, 10 phase-one Python and 54 phase-two Python tests (112 total), with zero Svelte errors/warnings and successful frontend/CLI/macOS release builds. Tests include cross-provider history and attachments, per-chat persistence, invalid configurations, active changes, stop/crash recovery, direct model-unavailable handling without fallback, and delayed provider startup without duplication. Logs: target/phase2/direct-chat-verification.log and direct-chat-build.log.

Native macOS 27.2.1 arm64 release verification used isolated homes/state and fake providers: manually selected Claude/sonnet/high in a chat, sent Persian text, saw the direct response with its model label, attached the chat to a project (Superagent displayed and selectors hidden), then detached it and observed Claude/high restored. On the final fixed build, an intentionally delayed fake Claude startup completed exactly one direct task, one provider attempt and one assistant response. Evidence: direct-chat-native.json, direct-chat-native-after-fix.json/.jsonl and sha256.json under target/phase2. The initial duplicate run is retained in direct-chat-native-snapshot.json and direct-chat-native-before-fix.jsonl; it is not a final acceptance result.

No supervisor, real Codex/Claude account requests, installed-setting changes, new packages, commits or pushes were used. Browser preview and Windows/Linux were not tested for this change.


## Composer selector spacing — 2026-11-15

Closed model and effort controls now size to their selected text instead of the longest native menu item. A hidden, accessibility-excluded sizing span retains precise proportional-font measurement; the native select remains the interactive control. Both use the same 30px height, text/chevron padding and 8px gap, with a nonshrinking send button. Long labels retain a width cap and full-title tooltip.

Svelte check had zero errors/warnings; frontend and native macOS release builds passed. Isolated native macOS inspection verified GPT-6.1-Sol/high layout and opening the effort menu. The fixture model display name is visual test data, not real execution evidence. No provider requests or supervisor use. Windows/Linux and the earlier full engine suite were not rerun for this UI-only change. Evidence: target/phase2/spacing-verification.json, spacing-build.log and updated sha256.json.
