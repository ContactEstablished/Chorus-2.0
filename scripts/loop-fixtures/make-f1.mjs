#!/usr/bin/env node
// P15 fixture F1: a tiny TypeScript CLI tested with Node's own runner (no dependencies, so it needs
// no install), with a three-phase roadmap for the loop. Makes a fresh git repository at the folder
// given, which must not exist yet. With --always-ask, decisions.md says to ask before wording any
// message a user sees, so a builder raises a blocking question (P15 scenarios 8 and 10).
// Usage: node scripts/loop-fixtures/make-f1.mjs <folder> [--always-ask]
import { execFileSync } from 'node:child_process'
import { existsSync, mkdirSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'

const dir = process.argv[2]
const alwaysAsk = process.argv.includes('--always-ask')
if (!dir) {
  console.error('usage: make-f1.mjs <folder>')
  process.exit(2)
}
if (existsSync(dir)) {
  console.error(`${dir} already exists`)
  process.exit(2)
}

const files = {
  'package.json': JSON.stringify({ name: 'f1-calc', private: true, type: 'module', scripts: { test: 'node --test' } }, null, 2) + '\n',
  '.gitignore': 'node_modules/\n',
  'src/cli.ts': `// f1-calc: a calculator on the command line.
export const VERSION = '0.1.0'

export function run(args: string[]): string {
  const [command] = args
  if (command === '--version') return VERSION
  return \`unknown command: \${command ?? '(none)'}\`
}

if (import.meta.main) console.log(run(process.argv.slice(2)))
`,
  'test/cli.test.ts': `import { test } from 'node:test'
import assert from 'node:assert/strict'
import { run, VERSION } from '../src/cli.ts'

test('--version prints the version', () => {
  assert.equal(run(['--version']), VERSION)
})
`,
  '.chorus/run/roadmap.md': `# Roadmap: f1-calc

## P01 · Sum command
- Goal: \`sum <a> <b>\` prints the sum of two numbers, and says what is wrong with a missing or non-numeric argument
- Risk: normal

## P02 · Product command
- Goal: \`product <a> <b>\` prints the product of two numbers, with the same argument checks as sum
- Risk: normal

## P03 · Help
- Goal: \`help\` lists every command with one line each, and an unknown command points to help
- Risk: normal
`,
  '.chorus/run/decisions.md': `# Decisions: f1-calc

## How to use this file
Read this before asking anything. If it answers your question, follow it.
If it doesn't, apply "Defaults when unsure", log the choice in assumptions.md, and keep going.

## Stack
- Node 24, TypeScript run directly by Node (type stripping): no build step, no tsc
- Tests: node:test with node:assert/strict, files named test/*.test.ts
- No dependencies. Anything that needs one is a question.

## Conventions
- One command per function in src/cli.ts, dispatched from run()
- Commands return their output as a string; run() never prints except at the entry point
- Erasable TypeScript only (no enums, no namespaces, no parameter properties)

## Testing
- Every acceptance criterion has at least one test
- Never delete, skip or weaken an existing test to get green

## Defaults when unsure
1. Follow an existing pattern in the codebase.
2. Choose the option that is easiest to change later.
3. Don't add a dependency; write the small thing instead.
${alwaysAsk ? `
## Always ask
- The exact wording of any message a user sees (errors and help text), unless a rule below already settles it. Ask once, with the wording you propose, before writing it.
` : ''}`,
}

for (const [path, text] of Object.entries(files)) {
  const full = join(dir, path)
  mkdirSync(dirname(full), { recursive: true })
  writeFileSync(full, text)
}
const git = (...args) => execFileSync('git', ['-C', dir, ...args], { stdio: 'pipe' })
git('init', '-q', '-b', 'main')
git('config', 'user.name', 'F1 fixture')
git('config', 'user.email', 'f1@example.invalid')
git('add', '--all')
git('commit', '-q', '-m', 'F1: a calculator CLI with a three-phase roadmap')
console.log(dir)
