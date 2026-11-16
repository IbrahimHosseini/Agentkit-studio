# Verification — 2026-11-15

## Codex connection preflight and visible reconnects — 2026-11-15

Implemented by two approved backend workers with root integration review against base `c59aac8b4fed8e56e1a882291d93d11ca2732287`; no supervisor was used. The existing real `arcaptcha` run had failed after 218 seconds with ten native reconnect notifications and `workspace routing discovery timed out`, without model text or command execution. Isolated metadata probes confirmed that the existing native Codex sign-in could resolve account/workspace routing and list models. The old execution adapter did not perform that account check before creating its thread and submitting the prompt.

- **Connection change:** Codex now calls `account/read` with `refreshToken:false` in the same execution process before thread creation/resume and turn submission. It reports the account/routing stage, detects missing sign-in before submission, and keeps the existing 45-second startup bound and Stop handling. Account responses and tokens are not published as events. Custom/no-auth providers remain supported. This fixes the missing preflight; the historical lower-level network/routing failure cannot be conclusively reconstructed from the retained diagnostic alone.
- **Progress change:** explicit connection notices and native `willRetry:true` errors are bridged into task snapshots and shown directly in Activity through the existing notice UI. Notices update during reconnect and clear on productive output, terminal states, cancellation or attempt changes, while preserving separate scheduler policy notices. Native reconnects stay inside one attempt and do not authorize replay of submitted work.
- **Automated checks on macOS arm64:** 32 Rust tests, 19 frontend helper tests, 10 phase-one and 59 phase-two Python tests: **120 passed**. Added coverage checks bootstrap ordering, no prompt after failed account checks, custom/no-auth resumed writes, Stop during discovery, unsafe retry rejection, visible reconnect snapshots and recovery/terminal clearing. Svelte had zero errors and the existing `MessageBody.svelte` warning. Frontend, CLI and macOS release builds passed; dependency lockfiles are unchanged. Ubuntu and Windows were not rerun for this update.
- **User-authorized real Codex test:** exactly one isolated read-only `gpt-6.1-sol/high` submission using native Codex `0.167.0` completed in 12.46 seconds. It read only a temporary fixture and returned its exact random code and complete Persian/English sample. The runtime reported the selected model/effort; no reconnect/error was recorded. The original authentication file and fixture were unchanged, and temporary credentials/state were removed. This is a successful short integration test, not a replay of the user's full project-analysis request or a guarantee against future upstream outages.
- **Native UI fixture check:** the final release bundle visibly displayed `Codex is reconnecting to the model service. Reconnecting... 2/5` in Activity with Stop available and one execution attempt. Releasing the synthetic provider produced the response, completed that same attempt and cleared the notice. This UI scenario used fake providers, separately from the real Codex test above.
- **Running user app:** the usual `target/phase2/Agentkit Studio.app` and standalone CLI were updated, with the previous app backed up. The new GUI and bundled engine are running against the existing user history, with eight conversations, the `arcaptcha` project and Codex connection preferences preserved. The project is selected for a new conversation; the failed historical request was not replayed. The bundled/standalone engine hashes match the release binary used by the successful real test. Installed agent definitions and provider configuration were not rewritten.

Evidence: ignored `target/phase2/codex-connection-fix/` contains `verification.log`, `release-build.log`, zero-inference metadata results, `live-codex.json` and its redacted evidence directory, native progress snapshots, running-app verification and artifact hashes.

## Ubuntu 24.04 arm64 compatibility — 2026-11-15

Verified source commit `6faca1dabdc478e527a8b17f48949f7789a40db0` in a disposable **Ubuntu 24.04.5 LTS arm64** Docker container on the macOS arm64 host. The Linux kernel was Docker Desktop's `7.0.14-linuxkit`, not an Ubuntu desktop kernel. Only project source was copied into the container; no host homes, provider credentials or configuration were mounted. Rust/backend tests and release builds used the unprivileged `ubuntu` user with isolated application homes; frontend checks ran in the disposable container's empty root profile. Existing containers were not modified, and the test container was removed after collecting evidence.

- **Toolchain:** Rust/Cargo 1.100.0, Node.js 24.20.0, npm 11.19.0, Python 3.12, GTK 3.24.41, WebKitGTK 2.52.6, glibc 2.39. Native dependencies were installed using the README's Debian/Ubuntu list; Xvfb, Xauth, D-Bus and fonts were added for headless startup checks.
- **Automated checks:** 29 Rust tests, 19 frontend helper tests, 10 phase-one and 59 phase-two Python tests: **117 passed**. Svelte checking reported zero errors and the existing `MessageBody.svelte` accessibility warning. The frontend production build and CLI build passed. Rust checks ran offline after fetching locked Linux dependencies. `Cargo.lock` and `apps/desktop/package-lock.json` remained byte-identical.
- **Release packaging:** the documented `bash scripts/desktop-build.sh` completed without modifications and produced arm64 `.deb`, aarch64 `.rpm` and aarch64 AppImage packages. The Debian package installed successfully with `dpkg -i`; its declared GTK/WebKit runtime dependencies were present. No missing shared libraries were reported for the installed desktop executable. RPM creation passed, but RPM installation was not tested on Ubuntu.
- **Installed Debian app:** native WebKitGTK under Xvfb mounted the frontend before showing the window and connected to the local daemon. `AGENTKIT_ENGINE_BIN` was unset, exercising the installed app's bundled-engine lookup. Offline fixture definitions were returned by that daemon.
- **AppImage:** startup also passed under Xvfb using `APPIMAGE_EXTRACT_AND_RUN=1`, because the container did not expose FUSE. With `AGENTKIT_ENGINE_BIN` unset, `/proc` confirmed the running daemon came from the AppImage's extracted `usr/lib/Agentkit Studio/binaries/agentkit`. FUSE-mounted execution remains unverified. WebKit sandbox disabling was **not** enabled for either successful startup check.
- **Harness correction:** the first startup attempt failed at GTK display authorization because the fixture environment whitelist omitted Xvfb's `XAUTHORITY`. Preserving that variable fixed the test harness. The failed attempt is retained separately in the evidence; it was not an application defect. Xvfb also reported nonfatal accessibility-bus and graphics-acceleration warnings.

No application compatibility defects were found in these tested paths, so no engine, UI or dependency changes were required. README verification status now reflects the Ubuntu evidence. No real Codex/Claude account or model request was used.

Evidence and generated packages are under ignored `target/ubuntu24.04-arm64/`: `verification-summary.json`, `sha256.json`, the standalone Linux CLI, three release packages and `evidence/` containing build/test logs, native startup reports and the temporary harnesses.

**Limits:** this verifies Ubuntu arm64 userland, native compilation, package installation, automated engine/provider-fixture behavior and native startup in a virtual display. Interactive desktop behavior, file dialogs, clipboard, hardware acceleration, Ubuntu x86_64, other Ubuntu versions, other Linux distributions, Windows and real provider execution on Ubuntu remain unverified. The macOS results below are separate historical evidence.

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

- Writing plans require approval in the default Ask for approval mode. A chat explicitly set to Approve for me or Full access starts validated plans without that extra gate; provider permission policy still applies.
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

## Per-chat permissions — 2026-11-16

Conversations now persist `ask`, `auto` or `full_access`, with `ask` as the default for new and legacy chats. The daemon validates mode changes and refuses them while a request, pending plan or provider process is still active. Validated project plans skip the extra write-plan gate only after an explicit automatic/full-access selection. Project-free direct chats, planners, summaries and read-only workers retain their existing read-only restrictions.

Codex receives explicit sandbox, approval policy and reviewer settings on both thread start/resume and turn start. Writable automatic mode verifies the installed CLI's generated protocol schema before task submission; unsupported review support fails closed. Review-started, approved, denied, timed-out, aborted and warning notifications become human-readable task notices. Claude uses explicit `default`, `auto` or `bypassPermissions` modes, with host permission requests bridged to Studio's Allow once / Deny controls. Read-only runs still deny requests without forwarding them to the user.

Backend verification on macOS arm64 used isolated application homes and synthetic providers: all 7 provider unit tests, the transport suite, all 22 core unit tests and 4 added daemon integration tests passed. Transport scenarios cover all permission modes, resumed Codex thread/turn overrides, read-only precedence, unsupported automatic review, review denials/timeouts/aborts, and Claude host allow/deny. Integration cases cover restart persistence and legacy defaults, invalid values, active-change refusal, projectless full-access restrictions, and repeated project writes through both providers without redundant plan approvals. The debug CLI build passed.

Root reviewed both approved worker reports and the combined changes against base `d1e49f8b999fe295e9a65c1c3aef8113879681bd`; no development supervisor was used. The full isolated suite passed on macOS arm64: **141 tests** (33 Rust, 35 frontend, 10 phase-one and 63 phase-two Python). Svelte checking had zero errors and the existing `MessageBody.svelte` accessibility warning. Frontend, CLI and native macOS release builds passed without dependency changes.

Interactive Chromium checks used the actual Svelte UI connected through a temporary local bridge to an isolated daemon and fake providers. They verified the screenshot-style menu, keyboard navigation and Escape/focus return, narrow-window fit, per-chat selection, persistence after daemon restart, explicit Full access styling, disabled controls during execution, Stop, repeated project writes without extra plan approval, and retaining the previous mode with a visible error after a failed save. The final browser pass had no console errors. A separate native release startup check reported a mounted, visible window after 834 ms; this is startup evidence, not native menu interaction verification.

Installed local Codex schema generation and Claude `--help`/`--version` checks were run with scratch homes to confirm the supported wire fields and permission-mode arguments. They are capability checks, not real provider inference. No credentials, installed settings or agent definitions were changed. Actual account-level automatic review, native permission-menu interaction and Windows/Linux execution were not verified for this change. The release app is under `target/release/bundle/macos/Agentkit Studio.app`; the user's running app was not replaced or restarted.
