# Agent notes

Read [docs/architecture.md](docs/architecture.md) and [docs/plugins.md](docs/plugins.md) before changing UI, the runner, or plugin loading. The migration sequence and phase 1 demo live in [docs/migration.md](docs/migration.md).

## Commands & verification

- Rust is the CI gate: `cargo test --locked` and `cargo check --locked --tests`. `make rust-test` / `make rust-build` wrap cargo in `scripts/cargo-guard.sh` (cache-size + disk guards, `DSH_TUI_RUST_CACHE_MAX_GIB`). No clippy/fmt gate exists.
- Linker OOM: on small-RAM machines `cc`/`ld` can get killed (signal 9) linking the test binary. Retry with `RUSTFLAGS="-C link-arg=-fuse-ld=mold" cargo test` (mold is much lighter). `release` uses `lto = true` and is slow — verify with debug builds.
- JS tests have no root `package.json`. Suites are `scripts/*.test.mjs` (`node --test`) run from the npm package: `npm test --prefix npm`; a single file is `node --test scripts/<name>.test.mjs`.
- Rust unit tests live in `tests/unit/*__tests.rs` but are wired in by a `#[cfg(test)] #[path = "../tests/unit/…"] mod tests;` include at the bottom of the owning `src/*.rs` file — add that include for new test files.
- No-TTY visual check: `cargo run -- --dump-frame 100x34` renders the canned demo frame as text; useful to diff transcript/markdown output before and after a rendering change.
- Demos: `martty --demo` stays on built-in `default` theme; `martty --demo-skin` is the gallery (`ember`) path. `make tui-test` drives a dsh profile against the debug bin; `make real-agent-e2e` consumes model tokens — opt-in.

## Release & versioning

- The version lives in three files that must match the `v*` tag: `Cargo.toml`, `npm/package.json`, and `npm-martty/package.json` (`scripts/check-release-tag.mjs` enforces tag == npm version == Cargo version).
- Add user-visible changes to `CHANGELOG.md` under `[Unreleased]` (Keep-a-Changelog style).
- CI (`package-npm.yml`, PRs + tags): Rust tests/check, `npm test --prefix npm`, `test:profile-install-matrix`; release builds additionally run static-ELF and old-glibc smoke checks.

## Protocol & plugin constraints

- The ACP client is a Cordis plugin providing `ctx.acpClient`. It is either the client composition root or an `insert` in another client tree. It attaches to an ACP agent by spawn (`dsh-acp` / `dsh --profile acp` / other) or `config.stream`. It does not import a harness.
- Do not sync plugin ids, `inject`, or fibers. Server plugins surface through ACP; client plugins stay on the client tree. Both sides negotiate protocol 0 through `initialize` `_meta.dsh.cordis`; only then may they use `_dsh/cordis/*` Extension Requests/Notifications. TUI painter methods are the `_dsh/cordis/tui/*` child domain. This is serialized capability projection, not plugin-tree sync.
- The primary profile path has two processes: the dsh Host tree mounts Base + the ACP plugin, then its runner starts a separate TUI Client process. ACP uses that Client process's stdin/stdout. The Client process receives the user TTY on fd 3/4 and maps it to the Rust painter; those descriptors never carry Host↔Client ACP. Standalone `martty` may instead spawn any ACP agent on standard pipes (`dsh-tui` remains a compatibility alias).
- Third-party packs are sibling `insert` rows plus `inject = ['tuiTheme']`. Do not nest them with `ctx.plugin` inside the runner.
- TUI npm may depend on `@deepseek-ai/cordis` only among `@deepseek-ai/*`; do not depend directly on `dsh-*`. It deliberately carries `@openma/deepseek-harness-acp` as a runtime dependency and exports its own Creator overlay. The TUI bundle mounts both Host plugins on the Base tree; neither enters the separate Client tree.
- The `tui-theme` service catalog starts with builtin `default`. Palette packs are sibling `insert` rows that `register` into the catalog (like agent presets). `/theme` and `tuiTheme.activate` switch which pack covers. `--demo-skin` is the only shipped path that selects gallery `ember`.
- The first shell slot proof is `chrome.right`: plugins inject `tuiSlots` and contribute validated `TuiNode` trees. Conversation timeline slots remain later work.
- Do not give plugins the TTY, kitty, raw mode, or global terminal size.
- Transcript paint comes from ACP `session/update`. Do not drive chrome or palette from `session/update`. Do not put TUI chrome in ACP `_meta`.
- ACP auth follows Backchat: read `initialize.authMethods`, submit in-app forms through `authenticate` `_meta`, run Terminal Auth (`_meta["terminal-auth"]`) on the TTY, then `authenticate` to re-check. `/login` stays the agent's slash when advertised. Phase 1 demo is `--demo-skin` (`ember`).
- Token names, kinds, and slot names are append-only; field meaning changes bump `protocol`.
