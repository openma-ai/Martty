// Opt-in E2E: drive the real Rust TUI (debug bin) in a PTY against a real
// `mcode acp` (MiniMax Code) agent and prove Ctrl+Enter steers the active
// turn through the agent's `mcode/session/steer` extension. MiniMax Code
// rejects a concurrent `session/prompt`, so before the route negotiation
// every Send Now degraded to "deferred" and re-queued.
//
// Assertions read a virtual terminal screen (see tui-multi-session.e2e.mjs
// for the rationale). Consumes model tokens. Not part of npm test / CI.
//
// Usage: cargo build --locked && \
//   MCODE_CLI=/path/to/@minimax-ai/code/cli.js [NODE_PTY=/path/to/node-pty] node scripts/mcode-steer.e2e.mjs [martty-bin]

import { mkdtempSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import { createRequire } from 'node:module'

const require = createRequire(import.meta.url)
const pty = require(process.env.NODE_PTY ?? '../npm/node_modules/node-pty')

const COLS = 110
const ROWS = 40

class Screen {
  constructor(cols, rows) {
    this.cols = cols; this.rows = rows
    this.grid = Array.from({ length: rows }, () => Array(cols).fill(' '))
    this.r = 0; this.c = 0; this.buf = ''
  }
  static wide(cp) {
    return (cp >= 0x1100 && cp <= 0x115f) || (cp >= 0x2e80 && cp <= 0xa4cf) || (cp >= 0xac00 && cp <= 0xd7a3) ||
      (cp >= 0xf900 && cp <= 0xfaff) || (cp >= 0xfe30 && cp <= 0xfe4f) || (cp >= 0xff00 && cp <= 0xff60) ||
      (cp >= 0xffe0 && cp <= 0xffe6) || (cp >= 0x20000 && cp <= 0x3fffd)
  }
  scrollUp() { this.grid.shift(); this.grid.push(Array(this.cols).fill(' ')); this.r = this.rows - 1 }
  newline() { this.r += 1; if (this.r >= this.rows) this.scrollUp() }
  putChar(ch) {
    const w = Screen.wide(ch.codePointAt(0)) ? 2 : 1
    if (this.c >= this.cols) { this.c = 0; this.newline() }
    if (w === 2 && this.c === this.cols - 1) { this.grid[this.r][this.c] = ch; this.c += 1; return }
    this.grid[this.r][this.c] = ch
    if (w === 2) this.grid[this.r][this.c + 1] = ''
    this.c += w
  }
  eraseLine(mode) {
    const row = this.grid[this.r]
    if (mode === 1) { for (let i = 0; i <= this.c && i < this.cols; i++) row[i] = ' ' }
    else if (mode === 2) row.fill(' ')
    else { for (let i = this.c; i < this.cols; i++) row[i] = ' ' }
  }
  eraseDisplay(mode) {
    if (mode === 1) { for (let r = 0; r < this.r; r++) this.grid[r].fill(' '); this.eraseLine(1) }
    else if (mode === 2) { for (const row of this.grid) row.fill(' ') }
    else { this.eraseLine(0); for (let r = this.r + 1; r < this.rows; r++) this.grid[r].fill(' ') }
  }
  csi(params, final) {
    const nums = params.split(';').map((p) => (p === '' || p.startsWith('?') ? NaN : parseInt(p, 10)))
    switch (final) {
      case 'H': case 'f': {
        const r = Number.isNaN(nums[0]) ? 1 : nums[0]
        const c = nums.length < 2 || Number.isNaN(nums[1]) ? 1 : nums[1]
        this.r = Math.min(Math.max(r - 1, 0), this.rows - 1)
        this.c = Math.min(Math.max(c - 1, 0), this.cols - 1)
        break
      }
      case 'K': this.eraseLine(Number.isNaN(nums[0]) ? 0 : nums[0]); break
      case 'J': this.eraseDisplay(Number.isNaN(nums[0]) ? 0 : nums[0]); break
      default: break
    }
  }
  feed(data) {
    const s = this.buf + data; this.buf = ''
    let i = 0; const n = s.length
    outer: while (i < n) {
      const ch = s[i]
      if (ch === '\x1b') {
        if (i + 1 >= n) break outer
        const nxt = s[i + 1]
        if (nxt === '[') {
          let j = i + 2
          while (j < n && !(s[j] >= '@' && s[j] <= '~')) j++
          if (j >= n) break outer
          this.csi(s.slice(i + 2, j), s[j]); i = j + 1; continue
        }
        if (nxt === ']') {
          let j = i + 2
          while (j < n && s[j] !== '\x07' && !(s[j] === '\x1b' && j + 1 < n && s[j + 1] === '\\')) j++
          if (j >= n) break outer
          i = s[j] === '\x07' ? j + 1 : j + 2; continue
        }
        if (nxt === '_' || nxt === 'P') { // APC (kitty graphics) / DCS: until ST
          let j = i + 2
          while (j < n && !(s[j] === '\x1b' && j + 1 < n && s[j + 1] === '\\')) j++
          if (j >= n) break outer
          i = j + 2; continue
        }
        if ('()*+#'.includes(nxt)) { if (i + 2 >= n) break outer; i += 3; continue }
        i += 2; continue
      }
      if (ch === '\r') { this.c = 0; i++; continue }
      if (ch === '\n' || ch === '\x0b' || ch === '\x0c') { this.newline(); i++; continue }
      if (ch === '\x08') { this.c = Math.max(0, this.c - 1); i++; continue }
      if (ch < ' ' || ch === '\x7f') { i++; continue }
      this.putChar(ch); i++
    }
    this.buf = s.slice(i)
  }
  rowText(r) { return this.grid[r].join('').replace(/\s+$/, '') }
  screenText() { return this.grid.map((_, r) => this.rowText(r)).join('\n') }
}

const bin = process.argv[2] ?? path.resolve('target/debug/martty')
const mcodeCli = process.env.MCODE_CLI
if (!mcodeCli) { console.error('set MCODE_CLI=/path/to/@minimax-ai/code/cli.js'); process.exit(2) }
const workspace = mkdtempSync(path.join(tmpdir(), 'martty-mcode-e2e-'))
const home = mkdtempSync(path.join(tmpdir(), 'martty-mcode-home-'))

const term = pty.spawn(bin, ['-w', workspace, '--agent', process.execPath, '--agent-arg', mcodeCli, '--agent-arg', 'acp'], {
  name: 'xterm-256color', cols: COLS, rows: ROWS, cwd: workspace,
  env: { ...process.env, TERM: 'xterm-256color', MARTTY_HOME: home },
})
const screen = new Screen(COLS, ROWS)
let raw = ''
term.onData((chunk) => { screen.feed(chunk); raw += chunk })

const failures = []
function check(name, cond, detail = '') {
  console.log(`${cond ? 'PASS' : 'FAIL'}  ${name}${detail ? ` — ${detail}` : ''}`)
  if (!cond) failures.push(name)
}
function waitFor(pred, ms, label) {
  const start = Date.now()
  return new Promise((resolve, reject) => {
    const tick = () => {
      if (pred()) return resolve()
      if (Date.now() - start > ms) return reject(new Error(`timeout waiting for ${label}`))
      setTimeout(tick, 200)
    }
    tick()
  })
}
const sleep = (ms) => new Promise((r) => setTimeout(r, ms))
async function type(text) { for (const ch of text) { term.write(ch); await sleep(4) } }
// Ctrl+Enter as the kitty/CSI-u encoding (crossterm decodes it on any terminal).
const CTRL_ENTER = '\x1b[13;5u'

const PROMPT = 'Count down from 300 to 1, one number per line, writing a short sentence about each number. Do not use any tools.'
const STEER = 'Stop the countdown now. Reply with exactly the single word PEACH and nothing else.'
const DEFERRED_TIP = /deferred Send Now|暂缓了立即发送/

try {
  await waitFor(() => screen.screenText().includes('/help commands'), 120000, 'TUI boot')
  console.log('boot ok')
  await waitFor(() => /minimax-code|MiniMax/i.test(screen.screenText()), 60000, 'agent name in chrome')
  console.log('agent initialized')

  await type(PROMPT)
  term.write('\r')
  // The echoed prompt mentions 300 and 1 only; "29x" followed by a separator proves streaming.
  await waitFor(() => /\b29[0-9]\s*[—.\-:]/.test(screen.screenText()), 120000, 'countdown streaming')
  console.log('turn streaming; sending Ctrl+Enter steer')

  await type(STEER)
  term.write(CTRL_ENTER)
  await sleep(1500)
  const tipEarly = DEFERRED_TIP.test(screen.screenText())
  check('steer not reported as deferred right after Ctrl+Enter', !tipEarly)

  let peach = true
  try {
    await waitFor(() => /^\s*PEACH\s*$/m.test(screen.screenText()) || /\bPEACH\b/.test(screen.screenText().split(STEER).pop() ?? ''), 180000, 'PEACH reply')
  } catch { peach = false }
  check('agent honored the steer within the same turn (PEACH)', peach)
  check('no deferred tip at any point', !DEFERRED_TIP.test(raw))
  // Deferred steers re-enter the client FIFO; the queue counter would show it.
  check('nothing left queued', !/[Qq]ueue · \d+/.test(screen.screenText()), screen.screenText().match(/[Qq]ueue · \d+[^\n]{0,20}/)?.[0] ?? '')
  if (!peach || tipEarly) console.log('--- screen ---\n' + screen.screenText())
} catch (err) {
  console.error(`e2e aborted: ${err.message}`)
  console.log('--- screen ---\n' + screen.screenText())
  failures.push('aborted')
} finally {
  term.write('\x03'); await sleep(300); term.write('\x03'); await sleep(500)
  term.kill()
}
console.log(failures.length ? `FAILURES: ${failures.join(', ')}` : 'ALL PASS')
process.exit(failures.length ? 1 : 0)
