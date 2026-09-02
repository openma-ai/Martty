// Issue #94 opt-in E2E: drive the real Rust TUI (debug bin) in a PTY against
// the real dsh-acp agent and prove two sessions run concurrently through the
// whole client stack — per-session inflight in acp.rs, parked-slot routing,
// tab strip, click-to-switch, background progress.
//
// Assertions read a virtual terminal screen (100x34 character grid fed from
// the PTY byte stream), not the raw bytes: ratatui only emits terminal diffs,
// so a glyph drawn once (e.g. ● on the tab strip) never reappears in the
// stream and raw-window searches are unreliable.
//
// Consumes model tokens. Not part of npm test / CI.
// Usage: cargo build --locked && node scripts/tui-multi-session.e2e.mjs [path-to-martty-bin]

import { mkdtempSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import { createRequire } from 'node:module'

const require = createRequire(import.meta.url)
const pty = require('../npm/node_modules/node-pty')

const COLS = 100
const ROWS = 34

// ---------------------------------------------------------------------------
// Minimal VT100 screen model: a COLS×ROWS grid of glyphs fed by PTY output.
// Handles what ratatui/crossterm actually emits: CUP (\x1b[r;cH, 1-based),
// CR/LF/BS, printable text at the cursor with advance (wide CJK = 2 cells),
// EL (\x1b[K), ED (\x1b[J), SGR (\x1b[...m, parsed and ignored). Everything
// else (other CSI, private modes, OSC, charset switches) is consumed and
// ignored. SGR mouse reports are outbound only.
// ---------------------------------------------------------------------------
class Screen {
  constructor(cols, rows) {
    this.cols = cols
    this.rows = rows
    this.grid = Array.from({ length: rows }, () => Array(cols).fill(' '))
    this.r = 0
    this.c = 0
    this.buf = ''
  }

  static wide(cp) {
    return (
      (cp >= 0x1100 && cp <= 0x115f) || // Hangul Jamo
      (cp >= 0x2e80 && cp <= 0xa4cf) || // CJK radicals … Yi
      (cp >= 0xac00 && cp <= 0xd7a3) || // Hangul syllables
      (cp >= 0xf900 && cp <= 0xfaff) || // CJK compat ideographs
      (cp >= 0xfe30 && cp <= 0xfe4f) || // CJK compat forms
      (cp >= 0xff00 && cp <= 0xff60) || // fullwidth forms
      (cp >= 0xffe0 && cp <= 0xffe6) || // fullwidth signs
      (cp >= 0x20000 && cp <= 0x3fffd) // CJK ext B+
    )
  }

  blank() { return ' ' }

  scrollUp() {
    this.grid.shift()
    this.grid.push(Array(this.cols).fill(' '))
    this.r = this.rows - 1
  }

  newline() {
    this.r += 1
    if (this.r >= this.rows) this.scrollUp()
  }

  putChar(ch) {
    const cp = ch.codePointAt(0)
    const w = Screen.wide(cp) ? 2 : 1
    if (this.c >= this.cols) {
      this.c = 0
      this.newline()
    }
    if (w === 2 && this.c === this.cols - 1) {
      // Wide glyph at the right edge: drop into the last cell, no continuation.
      this.grid[this.r][this.c] = ch
      this.c += 1
      return
    }
    this.grid[this.r][this.c] = ch
    if (w === 2) this.grid[this.r][this.c + 1] = '' // continuation cell
    this.c += w
  }

  eraseLine(mode) {
    const row = this.grid[this.r]
    if (mode === 1) {
      for (let i = 0; i <= this.c && i < this.cols; i++) row[i] = ' '
    } else if (mode === 2) {
      row.fill(' ')
    } else {
      for (let i = this.c; i < this.cols; i++) row[i] = ' '
    }
  }

  eraseDisplay(mode) {
    if (mode === 1) {
      for (let r = 0; r < this.r; r++) this.grid[r].fill(' ')
      this.eraseLine(1)
    } else if (mode === 2) {
      for (const row of this.grid) row.fill(' ')
    } else {
      this.eraseLine(0)
      for (let r = this.r + 1; r < this.rows; r++) this.grid[r].fill(' ')
    }
  }

  csi(params, final) {
    const nums = params.split(';').map((p) => (p === '' || p.startsWith('?') ? NaN : parseInt(p, 10)))
    switch (final) {
      case 'H':
      case 'f': {
        const r = Number.isNaN(nums[0]) ? 1 : nums[0]
        const c = nums.length < 2 || Number.isNaN(nums[1]) ? 1 : nums[1]
        this.r = Math.min(Math.max(r - 1, 0), this.rows - 1)
        this.c = Math.min(Math.max(c - 1, 0), this.cols - 1)
        break
      }
      case 'K':
        this.eraseLine(Number.isNaN(nums[0]) ? 0 : nums[0])
        break
      case 'J':
        this.eraseDisplay(Number.isNaN(nums[0]) ? 0 : nums[0])
        break
      default:
        break // SGR and everything else: ignore
    }
  }

  // Feed a chunk; an incomplete trailing escape is kept in `this.buf`.
  feed(data) {
    const s = this.buf + data
    this.buf = ''
    let i = 0
    const n = s.length
    outer: while (i < n) {
      const ch = s[i]
      if (ch === '\x1b') {
        if (i + 1 >= n) break outer
        const nxt = s[i + 1]
        if (nxt === '[') {
          // CSI: params/intermediates until final byte @-~
          let j = i + 2
          while (j < n && !(s[j] >= '@' && s[j] <= '~')) j++
          if (j >= n) break outer
          this.csi(s.slice(i + 2, j), s[j])
          i = j + 1
          continue
        }
        if (nxt === ']') {
          // OSC: until BEL or ST (\x1b\\)
          let j = i + 2
          while (j < n && s[j] !== '\x07' && !(s[j] === '\x1b' && j + 1 < n && s[j + 1] === '\\')) j++
          if (j >= n) break outer
          i = s[j] === '\x07' ? j + 1 : j + 2
          continue
        }
        if ('()*+#'.includes(nxt)) {
          // Charset / line-size switches: ESC + one intermediate + one byte.
          if (i + 2 >= n) break outer
          i += 3
          continue
        }
        // Two-byte escape (e.g. ESC=, ESC>, ESC7, ESCM): ignore.
        i += 2
        continue
      }
      if (ch === '\r') { this.c = 0; i++; continue }
      if (ch === '\n' || ch === '\x0b' || ch === '\x0c') { this.newline(); i++; continue }
      if (ch === '\x08') { this.c = Math.max(0, this.c - 1); i++; continue }
      if (ch < ' ' || ch === '\x7f') { i++; continue } // other C0/DEL: ignore
      this.putChar(ch)
      i++
    }
    this.buf = s.slice(i)
  }

  rowText(r) { return this.grid[r].join('').replace(/\s+$/, '') }
  screenText() { return this.grid.map((_, r) => this.rowText(r)).join('\n') }
  /** Trimmed row exactly equals `text` (matches output lines, not composer echo). */
  hasLine(text) {
    return this.grid.some((_, r) => this.rowText(r).trim() === text)
  }
}

// ---------------------------------------------------------------------------

const bin = process.argv[2] ?? path.resolve('target/debug/martty')
const agent = path.join(process.env.HOME, '.dsh/profiles/martty/node_modules/.bin/dsh-acp')
const workspace = mkdtempSync(path.join(tmpdir(), 'martty-multi-e2e-'))

const term = pty.spawn(bin, ['-w', workspace, '--agent', agent], {
  name: 'xterm-256color',
  cols: COLS,
  rows: ROWS,
  cwd: workspace,
  env: { ...process.env, TERM: 'xterm-256color' },
})

const screen = new Screen(COLS, ROWS)
term.onData((chunk) => screen.feed(chunk))

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

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms))
// Per-character with tiny pacing so the composer doesn't drop fast bursts.
async function type(text) {
  for (const ch of text) {
    term.write(ch)
    await sleep(4)
  }
}
const SGR_CLICK = (col, row) => `\x1b[<0;${col};${row}M\x1b[<0;${col};${row}m`

const row0 = () => screen.rowText(0)

// Distinctive markers. Session A generates a long stream (stays busy while
// session B answers); session B answers instantly.
const PROMPT_A = 'Output the word ZEPHYR followed by a number, one per line, from "ZEPHYR 1" to "ZEPHYR 150". No other text, but think slowly between lines.'
const PROMPT_B = 'Reply with exactly the word BRAVO and nothing else.'

try {
  // 1. Boot: wait for the welcome hero's hint line.
  await waitFor(() => screen.screenText().includes('/help commands'), 90000, 'TUI boot')
  console.log('boot ok')

  // 2. Prompt session A (long stream). Wait for real output: the prompt text
  //    itself (echo + user bubble) only mentions "ZEPHYR 1" and "ZEPHYR 150",
  //    so any ZEPHYR 2..9 proves the model's stream started — regardless of
  //    whether the model honors "one per line".
  await type(PROMPT_A)
  term.write('\r')
  await waitFor(() => /ZEPHYR [2-9]\b/.test(screen.screenText()), 90000, 'session A streaming')
  console.log('session A streaming')

  // 3. Open a second tab: the native tab strip appears on row 0.
  await type('/new')
  term.write('\r')
  await waitFor(() => row0().includes('│') && row0().includes('+'), 30000, 'tab strip with 2 tabs')
  check('tab strip renders with ≥2 sessions', true)

  // 4. Prompt session B (instant answer) while A is still streaming.
  await type(PROMPT_B)
  term.write('\r')
  await waitFor(() => screen.hasLine('BRAVO'), 120000, 'BRAVO from session B')
  check('session B answered (BRAVO) via its own session', true)

  // 5. At this moment A's tab must show ● — still running in the background.
  check('session A still background-running (● on its tab) when B answered',
    row0().includes('●'), row0().includes('●') ? '' : `row0: ${row0()}`)

  // 6. A's completion badge (✓) must appear on its tab while it is parked —
  //    proof the background session kept running and its idle edge was tracked.
  let badge = true
  try {
    await waitFor(() => row0().includes('✓'), 240000, '✓ completion badge on tab A')
  } catch {
    badge = false
  }
  check('background completion badge (✓) raised on session A tab', badge, badge ? '' : `row0: ${row0()}`)

  // 7. Click session A's tab (first tab, row 1): transcript survived intact
  //    and its full stream completed in the background.
  term.write(SGR_CLICK(3, 1))
  await waitFor(() => screen.screenText().includes('ZEPHYR'), 15000, 'switch to tab A')
  check('click switches to session A; transcript intact', true)
  let fullStream = true
  try {
    // The echoed user prompt also contains "ZEPHYR 150", so require the tail
    // neighbor "ZEPHYR 149" too — only the real stream can produce it.
    await waitFor(
      () => screen.screenText().includes('ZEPHYR 149') && screen.screenText().includes('ZEPHYR 150'),
      15000, 'ZEPHYR 149..150 visible')
  } catch {
    fullStream = false
  }
  check('session A completed its full stream (ZEPHYR 150)', fullStream)
  // Viewing the tab clears the completion badge.
  let badgeCleared = true
  try {
    await waitFor(() => !row0().includes('✓'), 10000, '✓ badge cleared on view')
  } catch {
    badgeCleared = false
  }
  check('✓ badge cleared after viewing tab A', badgeCleared, badgeCleared ? '' : `row0: ${row0()}`)

  // 8. Click session B's tab: compute its column from row 0's layout — the
  //    region between the first │ separator and the next one (or the + cell).
  const strip = row0()
  const sep1 = strip.indexOf('│')
  const tabBStart = sep1 + 2 // 0-based index right after " │ "
  let tabBEnd = strip.indexOf('│', tabBStart)
  const plus = strip.indexOf('+', tabBStart)
  if (tabBEnd < 0 || (plus >= 0 && plus < tabBEnd)) tabBEnd = plus
  if (tabBEnd < 0) tabBEnd = strip.length
  const tabBCol = tabBStart + Math.max(1, Math.floor((tabBEnd - tabBStart) / 2)) + 1 // 1-based
  console.log(`tab B click target: col ${tabBCol} (strip: "${strip}")`)
  term.write(SGR_CLICK(tabBCol, 1))
  let bravoBack = true
  try {
    await waitFor(() => screen.hasLine('BRAVO'), 15000, 'switch back to tab B')
  } catch {
    bravoBack = false
  }
  check('switch back to session B keeps its answer', bravoBack)
} catch (err) {
  console.error(`e2e aborted: ${err.message}`)
  failures.push('aborted')
} finally {
  term.write('\x03')
  await sleep(500)
  term.kill()
}

if (failures.length > 0) {
  console.log('\n--- virtual screen at failure ---')
  console.log(screen.grid.map((_, r) => `${String(r).padStart(2)}|${screen.rowText(r)}`).join('\n'))
}
console.log(failures.length === 0 ? '\nE2E OK' : `\nE2E FAILED: ${failures.join(', ')}`)
process.exit(failures.length === 0 ? 0 : 1)
