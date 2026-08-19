#!/usr/bin/env node

import {
  cpSync,
  existsSync,
  readFileSync,
  readdirSync,
  statSync,
  writeFileSync,
} from 'node:fs'
import path from 'node:path'

const legacyName = '@openma/deepseek-harness-tui'
const textExtensions = new Set(['.js', '.json', '.md', '.mjs', '.yaml', '.yml'])

function rewriteTextFiles(root, aliasName) {
  for (const entry of readdirSync(root, { withFileTypes: true })) {
    const file = path.join(root, entry.name)
    if (entry.isDirectory()) {
      if (entry.name !== 'vendor') rewriteTextFiles(file, aliasName)
      continue
    }
    if (!entry.isFile() || !textExtensions.has(path.extname(entry.name))) continue
    const before = readFileSync(file, 'utf8')
    const after = before.replaceAll(legacyName, aliasName)
    if (after !== before) writeFileSync(file, after)
  }
}

try {
  const [sourceArg, destinationArg, aliasName] = process.argv.slice(2)
  if (!sourceArg || !destinationArg || !aliasName) {
    throw new Error('usage: package-alias.mjs <source> <destination> <alias-name>')
  }
  const source = path.resolve(sourceArg)
  const destination = path.resolve(destinationArg)
  if (!existsSync(source) || !statSync(source).isDirectory()) {
    throw new Error(`source package directory not found: ${source}`)
  }
  if (existsSync(destination)) {
    throw new Error(`destination already exists: ${destination}`)
  }
  const packageJson = JSON.parse(readFileSync(path.join(source, 'package.json'), 'utf8'))
  if (packageJson.name !== legacyName) {
    throw new Error(`expected source package ${legacyName}, found ${packageJson.name}`)
  }

  cpSync(source, destination, { recursive: true, errorOnExist: true, force: false })
  rewriteTextFiles(destination, aliasName)
  process.stdout.write(`${destination}\n`)
} catch (error) {
  process.stderr.write(`${error.message}\n`)
  process.exitCode = 1
}
