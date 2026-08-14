# Changelog

All notable changes to this project are documented here. The project follows
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.2] - 2026-08-14

### Fixed

- Closing the TUI no longer crashes the host Node process with unhandled
  `write EPIPE` on the JSON-RPC extra fds. The runner now treats those pipe
  errors as non-fatal, matching the stock SDK client, and skips flushing a
  pipe the native binary already closed.

[0.1.2]: https://github.com/openma-ai/deepseek-harness-tui/releases/tag/v0.1.2

## [0.1.1] - 2026-08-14

### Fixed

- The dsh profile runner is now ESM. A CommonJS runner that `require()`d the
  host's ESM packages raced Cordis's parallel `import()` and crashed Node 24
  with `ERR_REQUIRE_ESM_RACE_CONDITION` on `dsh --profile tui`.

[0.1.1]: https://github.com/openma-ai/deepseek-harness-tui/releases/tag/v0.1.1

## [0.1.0] - 2026-08-14

### Added

- Terminal-native DeepSeek Harness client built with Rust and ratatui.
- dsh profile bundle and standalone SDK JSON-RPC runtime modes.
- Streaming reasoning, tool calls, subagent activity, usage, and durable session
  resume.
- Host model, agent preset, permission preset, and plan-mode controls.
- Dark and light themes, mouse selection, routed clipboard support, and the
  optional `/liang` pixel companion.
- Native npm binaries for macOS arm64, macOS x64, Linux x64, and Windows x64.
- Tag-driven npm Trusted Publishing with provenance and GitHub Releases.

### Changed

- Installation now uses the unqualified npm package name and resolves through
  the `latest` dist-tag.
- Project and npm documentation now lead with quick start, supported platforms,
  runtime-mode differences, and troubleshooting.

[0.1.0]: https://github.com/openma-ai/deepseek-harness-tui/releases/tag/v0.1.0
