#!/usr/bin/env node
// P15: the scripted steps for F1's first phase, for the loop's scripted worker (debug builds).
// The first build's sum subtracts, so the checks fail; the fix adds. With --question, the first
// build instead asks a blocking question and builds correctly once it is answered.
// Usage: make-f1-script.mjs <file> [--question]
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
writeFileSync(process.argv[2], JSON.stringify({ steps }, null, 2) + '\n')
console.log(process.argv[2])
