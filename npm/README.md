# @openma/deepseek-harness-tui

DeepSeek Build (`dsh-tui`) — grok-build style terminal UI for the deepseek-harness agent runtime.

This package bundles a platform-native binary and installs two commands:

- `dsh-tui` (primary)
- `dsb` (alias)

## Install

```sh
npm i -g @openma/deepseek-harness-tui@beta
```

Or from a local tarball built from source:

```sh
# From the repo root, after building the tarball:
npm i -g ./dist/<tgz>
```

## Runtime discovery

The deepseek-harness runtime is discovered separately from this package. Either:

- install the SDK into a `.venv` next to your workspace:

  ```sh
  python -m venv .venv
  .venv/bin/pip install deepseek-harness-sdk
  ```

- or point directly at a runtime binary with the `DSH_RUNTIME_BIN` environment variable.

## Uninstall

```sh
npm uninstall -g @openma/deepseek-harness-tui
```

## Rebuild

The bundled binary is platform-specific. To (re)build the binary and the tarball on your machine:

```sh
bash scripts/build-npm.sh
```

This runs `cargo build --release`, copies the binary into `npm/vendor/<platform>-<arch>/`, and writes a fresh `.tgz` into `dist/`.
