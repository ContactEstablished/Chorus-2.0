import type { LoopCheck } from '../ipc'

/** What a run's checks must meet. `check_config` (src-tauri/src/run_loop/mod.rs) refuses them on
 * save; here they only say what to change before saving. */
export const MAX_CHECKS = 20
export const MAX_NAME = 40
export const MAX_COMMAND = 500
export const MIN_SECS = 10
export const MAX_SECS = 7200
/** A check's time limit when none is given, as "Add a check" starts it. */
export const DEFAULT_SECS = 600

export interface PastedChecks {
  checks: LoopCheck[]
  /** What was left out or assumed, each naming its line. */
  notes: string[]
  /** What a run would refuse, to change before saving, each naming its line or check. */
  problems: string[]
}

interface Line {
  n: number
  raw: string
}

interface Entry {
  n: number
  name: string
  command: string
  time: string
}

type RowKind = 'markdown' | 'tabs' | 'box'

const BOX_BAR = /[│┃║]/
/** A table's borders and separator rows, and rules between paragraphs: punctuation only. */
const FURNITURE = /^[\s|│┃║─━═┼╋╬├┤┌┐└┘┬┴╭╮╰╯╞╡╪+:*_=-]+$/
const FENCE = /^(```|~~~)/
const LIST = /^(?:[-*+•]|\d+[.)])\s+/
const UNIT = '(?:s|secs?|seconds?|m|mins?|minutes?|h|hrs?|hours?)'
const DURATION = new RegExp(`^(\\d+(?:\\.\\d+)?)\\s*(${UNIT})?\\.?$`, 'i')
const DURATION_IN = new RegExp(`\\d+(?:\\.\\d+)?\\s*${UNIT}?\\b`, 'i')
const TRAILING = /\s+\((\d+(?:\.\d+)?\s*[a-z]*)\)$/i
const CONTROL = /[\u0000-\u001f\u007f-\u009f]/

/** A cell's text without the Markdown around it: `code`, **bold**, __bold__. */
function unquote(cell: string): string {
  let t = cell.trim()
  for (const wrap of [/^`+([\s\S]*?)`+$/, /^\*\*([\s\S]*?)\*\*$/, /^__([\s\S]*?)__$/]) {
    const m = wrap.exec(t)
    if (m) t = m[1]!.trim()
  }
  return t
}

/** A line's kind of table row: a Markdown row starts with `|`, a terminal's draws `│`, and a
 * rendered table copied from a page separates its cells with tabs. A command with a pipe in it
 * (`npm test | tee out.txt`) doesn't start with one, so it stays a command. */
function rowKind(raw: string): RowKind | null {
  const t = raw.trim()
  if (BOX_BAR.test(t)) return 'box'
  if (t.startsWith('|')) return 'markdown'
  if (raw.split('\t').filter((c) => c.trim()).length >= 2) return 'tabs'
  return null
}

function cells(raw: string, kind: RowKind): string[] {
  let parts: string[]
  if (kind === 'tabs') parts = raw.split('\t')
  else if (kind === 'box') parts = raw.split(BOX_BAR)
  else parts = raw.split(/(?<!\\)\|/).map((c) => c.replace(/\\\|/g, '|'))
  if (kind !== 'tabs' && parts[0]?.trim() === '') parts.shift()
  if (kind !== 'tabs' && parts.length && parts[parts.length - 1]?.trim() === '') parts.pop()
  return parts.map(unquote)
}

/** Seconds from `900`, `900 s`, `15 min` or `2 h`; a number alone is seconds. */
export function seconds(text: string): number | null {
  const m = DURATION.exec(text.trim())
  if (!m) return null
  const unit = (m[2] ?? 's').toLowerCase()
  return Math.round(Number(m[1]) * (unit.startsWith('h') ? 3600 : unit.startsWith('m') ? 60 : 1))
}

const RUNNERS = new Set(['npm', 'pnpm', 'yarn', 'bun', 'npx'])
const TOOLS = new Set(['cargo', 'dotnet', 'go'])

/** A name for a check given only its command: the script a runner runs (`npm run typecheck` →
 * `typecheck`, `npm test` → `test`), a tool's subcommand (`cargo clippy` → `clippy`), a script
 * node runs, else the program's own name. */
export function nameFor(command: string): string {
  const [first = '', second, third] = command.trim().split(/\s+/)
  const plain = (w: string) => (w.split(/[\\/]/).pop() ?? w).replace(/\.(cmd|exe|bat|ps1|sh|mjs|cjs|js|ts)$/i, '').toLowerCase()
  const program = plain(first)
  let name = program
  if (RUNNERS.has(program) && second) name = second === 'run' && third ? third : second
  else if (TOOLS.has(program) && second) name = second
  else if (program === 'node' && second) name = plain(second)
  return name.replace(/^\W+|\W+$/g, '').slice(0, MAX_NAME) || 'check'
}

/** One check written on a line of its own: ``typecheck: `npm run typecheck` (10 min)``,
 * `` `typecheck`: `npm run typecheck` ``, `typecheck: npm run typecheck (600)`, or just the
 * command. A colon ends a name only when a space follows it, so `npm run test:rust` stays one
 * command. */
function single(line: Line): Entry {
  let t = line.raw.trim().replace(LIST, '')
  const spans = [...t.matchAll(/`+([^`]+)`+/g)]
  if (spans.length) {
    const code = spans[spans.length > 1 ? 1 : 0]!
    const before = spans.length > 1 ? spans[0]![1]! : t.slice(0, code.index).replace(/[\s:–—-]+$/, '')
    const after = DURATION_IN.exec(t.slice(code.index + code[0].length))
    return { n: line.n, name: unquote(before), command: code[1]!.trim(), time: after ? after[0] : '' }
  }
  let time = ''
  const trailing = TRAILING.exec(t)
  if (trailing) {
    time = trailing[1]!
    t = t.slice(0, trailing.index)
  }
  const named = /^([\w*][\w .*-]{0,60}):\s+(\S.*)$/.exec(t)
  if (named) return { n: line.n, name: unquote(named[1]!), command: named[2]!.trim(), time }
  return { n: line.n, name: '', command: t.trim(), time }
}

/** Table rows as checks. A first row that names a "command" column is a header and says which
 * column holds what; without one, the columns are the name, the command and the time limit. In a
 * terminal's table a long command wraps onto rows with an empty name, which join the row above. */
function table(rows: { line: Line; kind: RowKind }[]): Entry[] {
  let cols = { name: 0, command: 1, time: 2 }
  const out: Entry[] = []
  rows.forEach(({ line, kind }, i) => {
    const c = cells(line.raw, kind)
    const at = (re: RegExp) => c.findIndex((x) => re.test(x))
    if (i === 0 && at(/^command/i) >= 0) {
      cols = { name: Math.max(at(/^(name|check)/i), 0), command: at(/^command/i), time: at(/time|limit|secs|seconds/i) }
      return
    }
    const name = c[cols.name] ?? ''
    const command = c[cols.command] ?? ''
    const time = cols.time >= 0 ? (c[cols.time] ?? '') : ''
    const above = out[out.length - 1]
    if (kind === 'box' && !name && above) {
      above.command = `${above.command} ${command}`.trim()
      above.time = `${above.time} ${time}`.trim()
      return
    }
    out.push({ n: line.n, name, command, time })
  })
  return out
}

/** Turns pasted text into a run's checks: a Markdown table, a table copied from a page or a
 * terminal, or one check per line. Prose around a table, or outside a fenced block, is left out,
 * and a note says so; so is a row with no command. Values a run would refuse are kept, each with a
 * problem saying what to change. */
export function parseChecks(text: string): PastedChecks {
  const content: Line[] = text
    .split('\n')
    .map((raw, i) => ({ n: i + 1, raw: raw.replace(/\r$/, '') }))
    .filter((l) => l.raw.trim() && !FURNITURE.test(l.raw))
  const fence = (l: Line) => FENCE.test(l.raw.trim())
  const notes: string[] = []
  const problems: string[] = []
  let entries: Entry[]

  const rows = content.flatMap((line) => {
    const kind = rowKind(line.raw)
    return kind ? [{ line, kind }] : []
  })
  if (rows.length) {
    const left = content.filter((l) => !fence(l)).length - rows.length
    if (left) notes.push(`${left} line${left === 1 ? " that isn't a table row was" : "s that aren't table rows were"} left out.`)
    entries = table(rows)
  } else if (content.some(fence)) {
    let inside = false
    const kept: Line[] = []
    let outside = 0
    for (const l of content) {
      if (fence(l)) inside = !inside
      else if (inside) kept.push(l)
      else outside++
    }
    if (outside) notes.push(`Only the lines inside the code block were read; ${outside} other line${outside === 1 ? ' was' : 's were'} left out.`)
    entries = kept.map(single)
  } else {
    entries = content.map(single)
  }

  const taken = new Set(entries.map((e) => e.name.toLowerCase()).filter(Boolean))
  const named = new Set<string>()
  const checks: LoopCheck[] = []
  for (const e of entries) {
    if (!e.command) {
      notes.push(`Line ${e.n} was left out: it has no command.`)
      continue
    }
    let name = e.name
    if (!name) {
      const base = nameFor(e.command)
      name = base
      for (let k = 2; taken.has(name.toLowerCase()); k++) name = `${base} ${k}`
      taken.add(name.toLowerCase())
    } else if (named.has(name.toLowerCase())) {
      problems.push(`Two checks are called "${name}": rename one before saving.`)
    }
    named.add(name.toLowerCase())
    let timeoutSecs = DEFAULT_SECS
    if (e.time) {
      const secs = seconds(e.time)
      if (secs === null) notes.push(`Line ${e.n}: couldn't read the time limit "${e.time}", so it's ${DEFAULT_SECS} s.`)
      else timeoutSecs = secs
    }
    // Counted as check_config counts them: a name's characters, a command's bytes.
    if ([...name].length > MAX_NAME) problems.push(`Line ${e.n}: the name "${name}" is over ${MAX_NAME} characters; shorten it before saving.`)
    if (new TextEncoder().encode(e.command).length > MAX_COMMAND || CONTROL.test(e.command)) {
      problems.push(`${name}: write the command on one line, up to ${MAX_COMMAND} characters, before saving.`)
    }
    if (timeoutSecs < MIN_SECS || timeoutSecs > MAX_SECS) problems.push(`${name}: ${timeoutSecs} s is outside 10 seconds to 2 hours; change it before saving.`)
    checks.push({ name, command: e.command, timeoutSecs })
  }
  if (checks.length > MAX_CHECKS) problems.push(`That's ${checks.length} checks: a run has up to ${MAX_CHECKS}, so remove ${checks.length - MAX_CHECKS} before saving.`)
  return { checks, notes, problems }
}
