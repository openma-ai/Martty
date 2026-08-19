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

test('release workflow packages and publishes both npm names', () => {
  assert.match(workflow, /npm install --prefix npm --ignore-scripts --no-package-lock --no-audit --no-fund/)
  assert.match(workflow, /package-alias\.mjs npm npm-martty martty/)
  assert.match(workflow, /npm pack \.\/npm-martty --pack-destination dist/)
  assert.match(workflow, /npm publish \.\/dist\/martty-\[0-9\]\*\.tgz/)
  assert.match(workflow, /npm publish \.\/dist\/openma-deepseek-harness-tui-\[0-9\]\*\.tgz/)
})
