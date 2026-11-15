# Verification evidence

Date: 2026-11-12 (Asia/Tehran). Host: macOS arm64.

## Automated checks

The phase-one results below were captured before project extraction unless marked otherwise. They are historical evidence, not commands or dependencies on the separate agentkit project.

- Rust workspace: 10 unit tests passed (5 core, 5 provider).
- Fresh standalone CLI: 11 offline black-box integration tests passed, no skips.
- Existing Python lifecycle suite: 48 tests, OK, 7 skipped.
- Existing shell lifecycle suite: 81 passed, 0 failed, 1 preserved known XFAIL.
- Repository validation: passed.
- Svelte check: 0 errors and 0 warnings.
- Vite production build: passed; approximately 75.54 kB JavaScript / 26.00 kB gzip.
- Native macOS release bundle: built and launched, approximately 15 MiB before compression.

## Release memory sampling

The five observed processes were the GUI main process, daemon, and three WebKit helpers newly observed during the isolated app launch. Those helpers exited with the app. The table reports median RSS samples in MiB; shared pages can be counted more than once. CPU uses the platform ps estimate. The provider was explicitly mock, not a real model.

| Scenario | UI and helpers | Engine | Total RSS |
| --- | ---: | ---: | ---: |
| Idle window, no agent | 161.2 | 12.9 | 174.1 |
| One active mock run | 159.7 | 9.0 | 168.6 |
| 10,000 stored engine events; native UI selection not verified | 158.5 | 11.6 | 170.1 |
| After stopping run, window still open | 155.4 | 11.6 | 167.0 |

The observed total range was approximately 166–182 MiB RSS. This is not a guarantee, unique physical footprint, or a benchmark for authenticated Codex/Claude activity. See resources.json for raw PID samples and attribution limits.

A mock run remained running after its GUI process exited, then became cancelled when stopped from CLI. The engine and tracked WebView helpers were absent after final shutdown. Graceful provider-descendant cleanup also passed the separate fake-provider heartbeat test. A SIGKILL of the engine itself can still leave a POSIX descendant alive; that limitation is distinct from the tested stop path.

## RTL and platform coverage

- English/LTR application shell with automatic RTL prose and isolated file names; code, commands and paths are LTR.
- Chromium browser fixtures on macOS verified mixed Persian/English, half-space, emoji, multiline selection and undo.
- Browser approval regressions covered 100-row pages, byte-capped short pages, external CLI-style resolutions, and pending approvals older than the 500-row display buffer.
- Native Tauri process launch and CLI independence are verified. Native WebKit typing, clipboard, file dialogs and drag/drop remain unverified: the UI inspection tool timed out and macOS denied accessibility automation.
- Windows and Linux native builds/execution remain unverified. No claim is inferred from source portability or browser tests.

## Authentication

Initial installed-CLI checks with empty scratch homes returned HTTP 401 / Not logged in; those historical results remain in provider-smoke.json.

After explicit permission, one authenticated Codex run passed in 43 seconds: 75 streamed events, the exact random text-file code, correct red-left/blue-right image colors, and the exact mixed Persian/English/half-space/emoji line. Read-only command execution was observed. Fixture files were unchanged. The private auth copy, temporary homes and session state were deleted, and the source auth cache hash was unchanged.

Claude Code used the existing environment token and configured local gateway. The first isolated attempt omitted the user’s `sonnet` selection and defaulted to unavailable/inaccessible `claude-opus-5-5`. A separately approved corrected attempt preserved `sonnet`, which resolved to `claude-sonnet-5-5`, and received the same model-access error. Both reported zero input and output tokens. A subsequent read-only `/v1/models` metadata probe returned HTTP 404; no third inference attempt was sent. Claude model authentication, text/image/file processing and model availability remain unverified. This result does not establish an invalid credential, only that the selected model could not be used through that route.

No original settings or agents were changed. Live subagents/installed roles, approval controls, interruption and resume were not exercised by these short authenticated attempts; their earlier fake-protocol coverage must not be presented as live proof. Detailed sanitized evidence is in authenticated-smoke.json.

## Review

Supervisor review was limited to two rounds. Round-one control-payload/approval-replay/test-identity findings and the round-two page-size finding were fixed by their owning workers. Final verdict: APPROVED for the phase-one prototype code. No unresolved code FIX remained after round two. This is not acceptance of the still-unverified authenticated/native-input/other-OS capabilities above.

## Independent project extraction

Date: 2026-11-12. Studio was moved to its own sibling folder, agentkitStudio. All 1,685 transferred files/symlinks were verified before the documented standalone adaptations. The original agentkit checkout was restored to its inspected HEAD and is clean. LICENSE was copied, while original assets and agent definitions stayed in agentkit.

From the new Studio root:

- All five Cargo workspace manifests resolve within Studio; no original Python launcher is needed.
- Rust workspace: 10 tests passed.
- Standalone engine integration suite: 10 tests passed in 21.478 seconds. The obsolete eleventh test covered the original repository's Python launcher and was removed because that integration was removed.
- Svelte: 0 errors, 0 warnings.
- Production frontend build passed.
- Native macOS release bundle rebuilt successfully from the independent workspace (15 MiB). Artifacts under target/phase1 were refreshed, and the bundled CLI hash matches the standalone executable.

The existing design and documented provider/platform limitations are preserved. The original lifecycle suites listed above belong to agentkit and are not part of Studio's check commands. No new dependencies were installed and no Git repository or commits were created as part of extraction.
