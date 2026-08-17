import assert from 'node:assert/strict'
import { chmodSync, mkdtempSync, mkdirSync, readFileSync, realpathSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import { spawnSync } from 'node:child_process'
import test from 'node:test'

const repoRoot = path.resolve(import.meta.dirname, '..')
const buildScript = path.join(repoRoot, 'scripts', 'build-npm.sh')

function executable(file, source) {
  writeFileSync(file, source)
  chmodSync(file, 0o755)
}

test('npm packaging builds Rust through the scoped Cargo cache guard', () => {
  const root = mkdtempSync(path.join(tmpdir(), 'dsh-tui-build-npm-'))
  const bin = path.join(root, 'bin')
  const target = path.join(root, 'target')
  const cargoLog = path.join(root, 'cargo.log')
  const nodeLog = path.join(root, 'node.log')
  mkdirSync(bin)

  executable(path.join(bin, 'cargo'), `#!/bin/sh
printf 'target=%s\nargs=%s\n' "\${CARGO_TARGET_DIR-<unset>}" "$*" > "$DSH_TUI_TEST_CARGO_LOG"
mkdir -p "$CARGO_TARGET_DIR/release"
: > "$CARGO_TARGET_DIR/release/dsh-tui"
`)
  executable(path.join(bin, 'node'), `#!/bin/sh
case "$*" in
  '-p process.platform') printf 'darwin\n' ;;
  '-p process.arch') printf 'arm64\n' ;;
  *) printf '%s\n' "$*" > "$DSH_TUI_TEST_NODE_LOG" ;;
esac
`)
  executable(path.join(bin, 'npm'), '#!/bin/sh\nexit 0\n')
  executable(path.join(bin, 'ls'), "#!/bin/sh\nprintf 'dist/fake.tgz\\n'\n")

  try {
    const canonicalTarget = path.join(realpathSync(root), 'target')
    const result = spawnSync(buildScript, [], {
      cwd: repoRoot,
      encoding: 'utf8',
      env: {
        ...process.env,
        PATH: `${bin}:${process.env.PATH}`,
        DSH_TUI_CARGO_BIN: path.join(bin, 'cargo'),
        DSH_TUI_CARGO_TARGET_DIR: target,
        DSH_TUI_CARGO_ALLOW_EXTERNAL_TARGET: '1',
        DSH_TUI_RUST_CACHE_MAX_GIB: '20',
        DSH_TUI_RUST_DISK_MIN_GIB: '0',
        DSH_TUI_TEST_CARGO_LOG: cargoLog,
        DSH_TUI_TEST_NODE_LOG: nodeLog,
      },
    })

    assert.equal(result.status, 0, result.stderr)
    assert.equal(
      readFileSync(cargoLog, 'utf8'),
      `target=${canonicalTarget}\nargs=build --release --locked\n`,
    )
    assert.equal(
      readFileSync(nodeLog, 'utf8'),
      `scripts/package-native.mjs stage --source ${canonicalTarget}/release/dsh-tui --platform darwin --arch arm64 --vendor-root npm/vendor\n`,
    )
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})
