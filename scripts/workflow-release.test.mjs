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
