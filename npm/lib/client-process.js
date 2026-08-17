#!/usr/bin/env node

/** Independent TUI Client Cordis process launched by the Host runner. */

import { bootClient } from './boot.js'

await bootClient({
  stream: { stdin: process.stdout, stdout: process.stdin },
  extraArgs: process.argv.slice(2),
  tty: { stdin: 3, stdout: 4 },
})
