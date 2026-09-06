# Harness lifecycle verification

Verified locally on macOS arm64, 2026-09-05, against the working tree (not a published release).

## Acceptance audit

| Requirement | Evidence |
| --- | --- |
| Current means the running command, not the saved default; current is disabled | `scripts/harness-onboarding.test.mjs` covers different live/default recipes, picker, catalog, completion, pending readiness and exited-process recovery. `tests/unit/app__mode_tests.rs` and `scripts/tui-overlay.test.mjs` exercise disabled submission. The isolated TUI displayed `(current)` and ignored Enter on that row. |
| Explicit configured selection completes the handoff | `tests/unit/acp__tests.rs::harness_switch_reinitializes_the_agent_and_binds_an_empty_session` verifies both initialize and session/new. Onboarding tests verify saved npx selections bypass Add preparation and persistence waits for readiness. The isolated TUI switched saved recipes successfully. |
| Reuse installation/cache | The disposable TUI event log recorded one package preparation and three launches of that saved package, including after restarting the TUI. `scripts/harness-package.test.mjs` uses real npx and uvx with isolated offline package fixtures: prepare without launching the agent, then launch twice without another installation. Binary discovery/install tests cover managed executables and native local archive extraction. |
| One current connection/session projection | Config/status/plan/stats reset tests reject late old responses; asynchronous config writes and transaction rollbacks cannot mutate a replacement generation, even with reused session ids. Rust landing tests reject startup runtime/model fallback. Status resolves semantic model/thought_level categories. Native command serialization tests cover handoff versus prompt FIFO and cancellation. |
| ACP authentication semantics | Rust and JS tests distinguish advertised methods from selected login, signing-in, successful authenticate, authentication rejection, and subsequent session setup failure. They preserve the Agent's rejection reason and do not label ambient credentials as API Key. Readiness tests allow user-driven authentication to outlast a setup deadline. |
| Setup versus switch, empty versus started | Onboarding tests ensure Add/Install/Connect only persist recipes, including background completion; saved selection is a separate switch. Actual isolated TUI: empty session switches directly; after a prompt, selection opens confirmation; Esc keeps the transcript; confirming switches and returns to landing. No session deletion is performed; remote history/resume remains owned by the Agent. |

## Gates

- `npm test --prefix npm`: 403 tests, 401 passed, 2 native-Windows-only tests skipped on macOS.
- `cargo test --locked`: 652 unit tests plus 4 integration tests passed.
- `cargo check --locked --tests`: passed (two existing test-code warnings).
- `git diff --check`: passed.
- `cargo build --locked`: passed. The local `npm/vendor/darwin-arm64/martty` was updated and byte-compared with `target/debug/martty`; `node npm/bin/martty.js --version` reports `martty 0.2.31`.

The JS integration gate needs Python with pexpect. This run used an isolated temporary Python environment; no global Python packages or user credentials were changed.

## Verification boundaries

The interactive TUI scenario uses a disposable settings directory and local fake ACP servers; its prompts do not call a model. Real npx/uvx tests install only local fixture packages into isolated caches. These prove the client flow and cache reuse, not the availability, packaging completeness, account eligibility, or login success of every third-party Registry entry. Native Windows execution was not run on this macOS host.

Browser OAuth completion is deliberately not used as proof of successful ACP authentication. Actual third-party errors remain authoritative; this change does not install undeclared upstream prerequisites or alter account eligibility. No published package, Git commit, or user credential/configuration change is claimed by this verification.

## Harness management and direct errors — 2026-09-06

The follow-up goal adds **Delete on a saved row in `/harness`**, also accessible as
`/harness remove [id]`. The first choice keeps installed files. Private-installation
cleanup has a separate confirmation view listing the complete configuration and
resource paths; Enter confirms and Esc cancels. Only an exclusive
`bin/<registry-id>/<version>/<platform>` installation under the settings directory
is eligible. The entire bin directory, external programs, global installations,
shared runner caches, credentials and history are not cleanup targets. Current
processes and product-forced recipes are protected. Symlinked resources are kept.

Removal invalidates pending configuration work, cancels matching downloads, waits
for their tasks, rechecks ownership/references and then removes the recipe. Other
settings are preserved; the removed recipe's default reference is cleared. A changed
recipe or a newly shared resource invalidates an old confirmation. Resource cleanup
errors report their actual remaining path rather than claiming everything was kept.

Connection errors now show their reason, structured data and captured stderr in the
first scrollable view. Enter retries directly; there is no recovery/details menu.
Failed setup clears pending/busy/model/authentication projections in landing and
reports failure in `/status`. A failed adapter is selectable for retry even when its
process remains alive; its files remain protected until switching away. Subsequent
successful setup restores normal state.

Evidence:

- `scripts/harness-removal.test.mjs`: configuration-only cleanup, exact private
  directory deletion, defaults, preserved settings and credentials, shared command
  and argument references, symlinks, stale confirmations and macOS directory aliases.
- `scripts/harness-onboarding.test.mjs`: Delete on the selected saved row (no separate
  Remove Harness menu entry), complete confirmation,
  current protection, cancellation/wait ordering and prevention of late writeback;
  direct error/retry/Esc behavior and failed-current recovery.
- `npm run test:harness-ui --prefix npm`: actual PTY keyboard flow with temporary
  settings and local ACP fixtures. It deletes only a disposable private installation,
  displays a structured missing-executable error plus stderr, presses Enter to retry,
  and switches back successfully. ACP event logs verify two failed session requests
  rather than relying on repeated terminal frames. Requires Python 3 with pexpect and
  a built native binary; no real account or network agent is used.
- `npm test --prefix npm`: pretest 11/11 passed; main suite 411 tests, 409 passed,
  two native-Windows-only tests skipped on macOS.
- `cargo test --locked`: 656 unit tests and four integration tests passed, including
  Delete/Backspace row actions, current protection, search editing and visible key hints.
- `cargo check --locked --tests`, `cargo build --locked`, and `git diff --check`
  passed. The two pre-existing test warnings remain.
- The local macOS arm64 vendor executable was updated from the debug build and
  byte-compared; the actual npm wrapper reports `martty 0.2.31`.

Native Windows operation and third-party sign-in success were not exercised here.
No real user Harness was removed, and no published release is claimed.

## Delete UX and CLI Harness goal acceptance — 2026-09-06

Current-state acceptance of both goal items:

| Requirement | Implementation and exercised evidence |
| --- | --- |
| Delete from the selected TUI row without another target menu | Native Delete/Backspace semantic action; current/non-deletable rows remain protected. `app__mode_tests` and the real PTY flow select the managed row, open its scope confirmation and remove only its disposable installation. |
| Preserve configuration/resource boundaries | TUI and CLI both use `planHarnessRemoval` / `removeHarness`: clear only the selected recipe/default reference; resource cleanup requires exclusive private ownership. Existing shared-path, symlink, stale-confirmation, cancellation and preservation tests still pass. CLI cannot infer other processes' runtime state; its help tells users to stop other instances using the target before cleanup. |
| Complete CLI deletion | `remove <id>` previews exact paths and asks `[y/N]` on a terminal. `--dry-run` never mutates; scripts require `--yes`; `--cleanup` opts into private binary cleanup. Real CLI PTY tests exercise no, Ctrl-C and yes. Public-wrapper tests verify non-TTY confirmation refusal and configuration/default removal. |
| Fast/offline Registry discovery | `find` reads the cached/bundled official snapshot; `find --refresh` fetches and retains the snapshot on failure. Tests assert no request on normal find and retain candidates during an offline refresh. |
| Consistent add/use behavior | `add` saves only; already saved CLI recipes are reused offline without another download. Explicit TUI same-id replacement remains supported. `use` only changes the next-launch default. Wrapper tests read back settings after each step and do not start an agent. |
| Real installer and cancellation | A loopback HTTP server serves a real tar.gz through the CLI wrapper and production downloader/extractor. Tests verify progress, SHA-256, executable contents, label/argument overrides and a second offline add without another request. A stalled real transfer is cancelled with SIGINT, exits 130, removes staging and does not save a recipe. |
| Input and startup errors | Unknown/excess/missing arguments fail before Registry access or settings writes; binary setup honors overrides. Missing `--agent`/`--agent-arg` values fail before TUI startup. Quoted environment commands, spaces, empty arguments and quoted Windows paths are parsed without a shell. |

Final gates: `npm test --prefix npm` passed (pretest 11; main 421 total,
419 passed and two Windows-only skips); `cargo test --locked` passed 656 unit
and four integration tests; `cargo check --locked --tests` passed with the two
existing warnings. `npm run test:harness-ui --prefix npm` passed both actual
terminal flows. `git diff --check` passed. `npm pack --dry-run --json --ignore-scripts`
includes the wrapper, command-argument parser, removal implementation and Registry
snapshot. This is local source/vendor validation, not a published release or native
Windows end-to-end verification.
