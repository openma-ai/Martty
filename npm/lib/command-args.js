/** Split an argv string without shell execution, expansion, or substitution. */
export function tokenizeCommandArgs(value) {
  const tokens = []
  let token = '', quote
  let escaped = false, started = false
  for (const char of String(value ?? '')) {
    if (escaped) {
      if (quote === '"' && !['\\', '"', '$', '`', '\n'].includes(char)) token += '\\'
      token += char
      escaped = false
    } else if (char === '\\' && quote !== "'") {
      escaped = true; started = true
    } else if (quote !== undefined) {
      if (char === quote) quote = undefined
      else token += char
    } else if (char === '"' || char === "'") {
      quote = char; started = true
    } else if (/\s/.test(char)) {
      if (started) { tokens.push(token); token = ''; started = false }
    } else {
      token += char; started = true
    }
  }
  if (escaped) token += '\\'
  if (quote !== undefined) throw Object.assign(new Error(`Unclosed ${quote} quote in command`), { exitCode: 2 })
  if (started) tokens.push(token)
  return tokens
}
