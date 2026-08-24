#!/usr/bin/env node

/** Independent TUI Client Cordis process launched by the Host runner. */

import { bootClient, parseClientPluginsEnv } from './boot.js'

// The Host runner kills this process with SIGTERM on shutdown, and the user's
// Ctrl+C reaches it as SIGINT from the foreground terminal group. Node's
// default signal handling terminates without running 'exit' listeners, which
// would strand the Rust painter as an orphan holding the TTY (it is only
// killed from the Client's 'exit' hook). Convert signals into an orderly
// process.exit so painter teardown and TTY restore always run.
for (const [signal, code] of [['SIGTERM', 143], ['SIGINT', 130], ['SIGHUP', 129]]) {
  process.on(signal, () => process.exit(code))
}

await bootClient({
  stream: { stdin: process.stdout, stdout: process.stdin },
  extraArgs: process.argv.slice(2),
  tty: { stdin: 3, stdout: 4 },
  packagePlugins: parseClientPluginsEnv(process.env.DSH_TUI_CLIENT_PLUGINS_V0),
})
