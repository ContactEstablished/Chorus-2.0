#!/usr/bin/env node
// P15: the scripted steps for F1's first phase, for the loop's scripted worker (debug builds).
// The first build's sum subtracts, so the checks fail; the fix adds. With --question, the first
// build instead asks a blocking question and builds correctly once it is answered. --phone adds
// phases 2 and 3 (see below).
// Usage: make-f1-script.mjs <file> [--question] [--phone]
import { writeFileSync } from 'node:fs'

const dir = '.chorus/run/phases/01-sum-command'
const cli = (op) => `// f1-calc: a calculator on the command line.
export const VERSION = '0.1.0'

export function sum(args: string[]): string {
  if (args.length !== 2) return 'sum needs two numbers: sum <a> <b>'
  const [a, b] = args.map(Number)
  if (Number.isNaN(a) || Number.isNaN(b)) return \`not a number: \${args.find((x) => Number.isNaN(Number(x)))}\`
  return String(a ${op} b)
}

export function run(args: string[]): string {
  const [command, ...rest] = args
  if (command === '--version') return VERSION
  if (command === 'sum') return sum(rest)
  return \`unknown command: \${command ?? '(none)'}\`
}

if (import.meta.main) console.log(run(process.argv.slice(2)))
`
const test = `import { test } from 'node:test'
import assert from 'node:assert/strict'
import { run } from '../src/cli.ts'

test('sum adds two numbers', () => {
  assert.equal(run(['sum', '2', '3']), '5')
})

test('sum names a non-numeric argument', () => {
  assert.equal(run(['sum', '2', 'x']), 'not a number: x')
})

test('sum needs two arguments', () => {
  assert.equal(run(['sum', '2']), 'sum needs two numbers: sum <a> <b>')
})
`
const question = process.argv.includes('--question')
const steps = [
  {
    phase: 1,
    step: 'plan',
    write: {
      [`${dir}/spec.md`]: '# P01 · Sum command\n\n`sum <a> <b>` prints a + b.\n\n## Acceptance\n- `sum 2 3` prints 5\n- `sum 2 x` prints "not a number: x"\n- `sum 2` prints the usage line\n',
      [`${dir}/tasks.md`]: '# Tasks: P01 · Sum command\n\n## T1 · Sum command\n- Depends on: none\n- Claims: src/cli.ts, test/sum.test.ts\n- Accept: `sum 2 3` prints 5\n- Accept: `sum 2 x` prints "not a number: x"\n- Accept: `sum 2` prints the usage line\n- Status: pending\n',
    },
  },
  ...(question
    ? [
        {
          phase: 1,
          step: 'build',
          task: 'T1',
          attempt: 0,
          write: { 'test/sum.test.ts': test },
          ask: {
            class: 'blocking',
            question: 'decisions.md says to ask about user-facing wording. For a missing argument, may I print "sum needs two numbers: sum <a> <b>"?',
            options: ['Yes, use it', 'Use "sum: missing argument"'],
          },
          after_answer: { 'src/cli.ts': cli('+') },
        },
      ]
    : [
        { phase: 1, step: 'build', task: 'T1', attempt: 0, write: { 'src/cli.ts': cli('-'), 'test/sum.test.ts': test } },
        { phase: 1, step: 'build', task: 'T1', attempt: 1, write: { 'src/cli.ts': cli('+') } },
      ]),
]

// With --phone (and --question), all three phases, for the relay: P01's blocking question, P02's
// timed one, then P03 finishes the run.
const fn = (name, op) => cli(op).split('\n\n').find((s) => s.startsWith('export function sum')).replaceAll('sum', name) + '\n\n'
const withProduct = cli('+')
  .replace('export function run(', `${fn('product', '*')}export function run(`)
  .replace("  if (command === 'sum') return sum(rest)\n", "  if (command === 'sum') return sum(rest)\n  if (command === 'product') return product(rest)\n")
const helpFn =
  "export function help(): string {\n  return ['sum <a> <b>      prints a + b', 'product <a> <b>  prints a * b', 'help             lists the commands', '--version        prints the version'].join('\\n')\n}\n\n"
const withHelp = withProduct
  .replace('export function run(', `${helpFn}export function run(`)
  .replace('  return `unknown command: ${command ?? \'(none)\'}`', "  if (command === 'help') return help()\n  return `unknown command: ${command ?? '(none)'} (try: help)`")
const helpTest = `import { test } from 'node:test'
import assert from 'node:assert/strict'
import { run } from '../src/cli.ts'

test('help lists every command', () => {
  for (const c of ['sum <a> <b>', 'product <a> <b>', 'help', '--version']) assert.ok(run(['help']).includes(c), c)
})

test('an unknown command points to help', () => {
  assert.equal(run(['nope']), 'unknown command: nope (try: help)')
})
`
const plan = (n, slug, title, accept, claims) => ({
  phase: n,
  step: 'plan',
  write: {
    [`.chorus/run/phases/${slug}/spec.md`]: `# P0${n} · ${title}\n\n## Acceptance\n${accept.map((a) => `- ${a}`).join('\n')}\n`,
    [`.chorus/run/phases/${slug}/tasks.md`]: `# Tasks: P0${n} · ${title}\n\n## T1 · ${title}\n- Depends on: none\n- Claims: ${claims}\n${accept.map((a) => `- Accept: ${a}`).join('\n')}\n- Status: pending\n`,
  },
})
if (process.argv.includes('--phone')) {
  steps.push(
    plan(2, '02-product-command', 'Product command', ['`product 2 3` prints 6', '`product 2` prints the usage line'], 'src/cli.ts, test/product.test.ts'),
    {
      phase: 2,
      step: 'build',
      task: 'T1',
      attempt: 0,
      write: { 'test/product.test.ts': test.replaceAll('sum', 'product').replace('adds', 'multiplies').replace("'5'", "'6'") },
      ask: {
        class: 'timed',
        question: 'For product with a missing argument, may I print "product needs two numbers: product <a> <b>", the same shape as sum?',
        options: ['Yes, like sum', 'Use "product: missing argument"'],
        default: 'Yes, like sum',
      },
      after_answer: { 'src/cli.ts': withProduct },
    },
    plan(3, '03-help', 'Help', ['`help` lists every command', 'an unknown command points to help'], 'src/cli.ts, test/help.test.ts'),
    { phase: 3, step: 'build', task: 'T1', attempt: 0, write: { 'src/cli.ts': withHelp, 'test/help.test.ts': helpTest } },
  )
}
writeFileSync(process.argv[2], JSON.stringify({ steps }, null, 2) + '\n')
console.log(process.argv[2])
