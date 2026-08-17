import assert from 'node:assert/strict'
import {
  chmodSync,
  mkdtempSync,
  mkdirSync,
  readFileSync,
  realpathSync,
  rmSync,
  writeFileSync,
} from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import { spawnSync } from 'node:child_process'
import test from 'node:test'

const repoRoot = path.resolve(import.meta.dirname, '..')
const guard = path.join(repoRoot, 'scripts', 'cargo-guard.sh')

function fixture() {
  const root = mkdtempSync(path.join(tmpdir(), 'dsh-tui-cargo-guard-'))
  const target = path.join(root, 'target')
  const log = path.join(root, 'cargo.log')
  const cargo = path.join(root, 'fake-cargo')
  writeFileSync(cargo, `#!/bin/sh
printf '%s\n' "$*" >> "$DSH_TUI_TEST_CARGO_LOG"
if [ "$1" = clean ]; then
  rm -rf "$CARGO_TARGET_DIR"
fi
`)
  chmodSync(cargo, 0o755)
  return { root, target, log, cargo }
}

function run(args, item, overrides = {}) {
  return spawnSync(guard, args, {
    cwd: repoRoot,
    encoding: 'utf8',
    env: {
      ...process.env,
      DSH_TUI_CARGO_BIN: item.cargo,
      DSH_TUI_CARGO_TARGET_DIR: item.target,
      DSH_TUI_CARGO_ALLOW_EXTERNAL_TARGET: '1',
      DSH_TUI_RUST_CACHE_MAX_GIB: '20',
      DSH_TUI_RUST_DISK_MIN_GIB: '0',
      DSH_TUI_TEST_CARGO_LOG: item.log,
      ...overrides,
    },
  })
}

test('cargo guard status is read-only and reports the scoped target', () => {
  const item = fixture()
  try {
    mkdirSync(item.target)
    writeFileSync(path.join(item.target, 'artifact'), 'cache')

    const result = run(['status'], item)

    assert.equal(result.status, 0, result.stderr)
    assert.match(result.stdout, /Cargo target:/)
    assert.match(result.stdout, /Cache size:/)
    assert.equal(readFileSync(path.join(item.target, 'artifact'), 'utf8'), 'cache')
    assert.throws(() => readFileSync(item.log, 'utf8'), /ENOENT/)
  } finally {
    rmSync(item.root, { recursive: true, force: true })
  }
})

test('cargo guard cleans an oversized scoped target before running cargo', () => {
  const item = fixture()
  try {
    mkdirSync(item.target)
    writeFileSync(path.join(item.target, 'artifact'), 'cache')

    const result = run(['test', '--locked'], item, {
      DSH_TUI_RUST_CACHE_MAX_GIB: '0',
    })

    assert.equal(result.status, 0, result.stderr)
    assert.deepEqual(readFileSync(item.log, 'utf8').trim().split('\n'), [
      'clean',
      'test --locked',
    ])
  } finally {
    rmSync(item.root, { recursive: true, force: true })
  }
})

test('cargo guard refuses broad destructive target directories', () => {
  const item = fixture()
  try {
    const result = run(['prune'], item, { DSH_TUI_CARGO_TARGET_DIR: '/' })

    assert.notEqual(result.status, 0)
    assert.match(result.stderr, /refusing unsafe Cargo target/i)
    assert.throws(() => readFileSync(item.log, 'utf8'), /ENOENT/)
  } finally {
    rmSync(item.root, { recursive: true, force: true })
  }
})

test('make tui-test rebuilds the debug binary before launching the development profile', () => {
  const item = fixture()
  try {
    const root = realpathSync(item.root)
    const bin = path.join(root, 'bin')
    const target = path.join(root, 'target')
    const debugBinary = path.join(target, 'debug', 'dsh-tui')
    mkdirSync(bin)
    writeFileSync(item.cargo, `#!/bin/sh
printf 'cargo:%s\n' "$*" >> "$DSH_TUI_TEST_DEV_LOG"
if [ "$1" = build ]; then
  mkdir -p "$CARGO_TARGET_DIR/debug"
  : > "$CARGO_TARGET_DIR/debug/dsh-tui"
  chmod +x "$CARGO_TARGET_DIR/debug/dsh-tui"
fi
`)
    chmodSync(item.cargo, 0o755)
    const dsh = path.join(bin, 'dsh')
    writeFileSync(dsh, `#!/bin/sh
if [ ! -x "$DSH_TUI_BIN" ]; then
  echo "development binary was not built: $DSH_TUI_BIN" >&2
  exit 9
fi
printf 'dsh:%s|bin=%s\n' "$*" "$DSH_TUI_BIN" >> "$DSH_TUI_TEST_DEV_LOG"
`)
    chmodSync(dsh, 0o755)
    const devLog = path.join(root, 'dev.log')

    const result = spawnSync('make', [
      '--no-print-directory',
      'tui-test',
      `TUI_DEBUG_BIN=${debugBinary}`,
    ], {
      cwd: repoRoot,
      encoding: 'utf8',
      env: {
        ...process.env,
        PATH: `${bin}:${process.env.PATH}`,
        DSH_TUI_CARGO_BIN: item.cargo,
        DSH_TUI_CARGO_TARGET_DIR: target,
        DSH_TUI_CARGO_ALLOW_EXTERNAL_TARGET: '1',
        DSH_TUI_RUST_CACHE_MAX_GIB: '20',
        DSH_TUI_RUST_DISK_MIN_GIB: '0',
        DSH_TUI_TEST_DEV_LOG: devLog,
      },
    })

    assert.equal(result.status, 0, result.stderr)
    assert.deepEqual(readFileSync(devLog, 'utf8').trim().split('\n'), [
      'cargo:build --locked',
      `dsh:--profile tui-test|bin=${debugBinary}`,
    ])
  } finally {
    rmSync(item.root, { recursive: true, force: true })
  }
})
