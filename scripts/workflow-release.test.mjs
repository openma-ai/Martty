import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import path from 'node:path'
import test from 'node:test'

const workflow = readFileSync(
  path.resolve(import.meta.dirname, '..', '.github', 'workflows', 'package-npm.yml'),
  'utf8',
)

test('release jobs are gated to the current canonical repository', () => {
  assert.doesNotMatch(workflow, /github\.repository == 'openma-ai\/deepseek-harness-tui'/)
  assert.equal(
    workflow.match(/github\.repository == 'openma-ai\/Martty'/g)?.length,
    2,
  )
})

test('release workflow packages and publishes only martty', () => {
  assert.match(
    workflow,
    /cache: npm\n\s+cache-dependency-path: npm\/package-lock\.json/,
  )
  assert.match(
    workflow,
    /npm ci --prefix npm --ignore-scripts --no-audit --no-fund/,
  )
  assert.match(workflow, /package-alias\.mjs npm npm-martty martty/)
  assert.match(workflow, /npm pack \.\/npm-martty --pack-destination dist/)
  assert.match(workflow, /npm publish \.\/dist\/martty-\[0-9\]\*\.tgz/)
  assert.doesNotMatch(workflow, /npm pack \.\/npm --pack-destination dist/)
  assert.doesNotMatch(workflow, /npm publish \.\/dist\/openma-deepseek-harness-tui/)
})

test('Windows CI boots an installed profile through the real Node loader', () => {
  assert.match(
    workflow,
    /uses: pnpm\/action-setup@v6\n\s+with:/,
    'pnpm setup must not exclude Windows',
  )
  assert.match(
    workflow,
    /name: Install Node test dependencies\n\s+run: npm ci --prefix npm/,
    'Node dependencies must be installed on Windows',
  )
  assert.match(workflow, /name: Build Windows profile smoke launcher/)
  assert.match(workflow, /name: Run Windows profile smoke/)
  assert.match(workflow, /DSH_TUI_MATRIX_CASE: missing/)
  assert.match(workflow, /DSH_TUI_LOCAL_DEPS: "1"/)
  assert.match(workflow, /DSH_TUI_MATRIX_TMP: \$\{\{ runner\.temp \}\}/)
  assert.match(workflow, /npm run test:profile-install-matrix --prefix npm/)
})
