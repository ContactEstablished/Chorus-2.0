import { describe, expect, it } from 'vitest'
import { nameFor, parseChecks, seconds } from './loopChecks'

// KICKOFF.md §3's checks for a run on Chorus 2, as the kickoff session writes them.
const KICKOFF = `Checks, in this order; each is also a command builders may run while they work:

| Name | Command | Timeout |
|---|---|---|
| deps | \`npm install --prefer-offline --no-audit --no-fund\` | 900 s |
| bindings | \`npm run bindings\` | 2400 s |
| typecheck | \`npm run typecheck\` | 600 s |
| web tests | \`npm test\` | 600 s |
| rust tests | \`npm run test:rust\` | 2400 s |
| clippy | \`npm run lint:rust\` | 2400 s |
| secrets | \`npm run grep:secrets\` | 300 s |

The worktree starts without node_modules or a Rust build, so the first round is slow.`

const EXPECTED = [
  { name: 'deps', command: 'npm install --prefer-offline --no-audit --no-fund', timeoutSecs: 900 },
  { name: 'bindings', command: 'npm run bindings', timeoutSecs: 2400 },
  { name: 'typecheck', command: 'npm run typecheck', timeoutSecs: 600 },
  { name: 'web tests', command: 'npm test', timeoutSecs: 600 },
  { name: 'rust tests', command: 'npm run test:rust', timeoutSecs: 2400 },
  { name: 'clippy', command: 'npm run lint:rust', timeoutSecs: 2400 },
  { name: 'secrets', command: 'npm run grep:secrets', timeoutSecs: 300 },
]

describe('pasting checks', () => {
  it('reads a Markdown table as it is, and says what prose it left out', () => {
    const got = parseChecks(KICKOFF)
    expect(got.checks).toEqual(EXPECTED)
    expect(got.notes).toEqual(["2 lines that aren't table rows were left out."])
    expect(got.problems).toEqual([])
    // As KICKOFF.md's source has it, indented under a list item.
    expect(parseChecks(KICKOFF.replace(/^\|/gm, '  |')).checks).toEqual(EXPECTED)
  })

  it('reads the same table copied from a page, where cells are separated by tabs', () => {
    const rendered = ['Name\tCommand\tTimeout', ...EXPECTED.map((c) => `${c.name}\t${c.command}\t${c.timeoutSecs} s`)].join('\r\n')
    expect(parseChecks(rendered)).toEqual({ checks: EXPECTED, notes: [], problems: [] })
  })

  it('reads a table a terminal drew, joining a command that wrapped', () => {
    const drawn = `┌───────────┬──────────────────────────────┬─────────┐
│ Name      │ Command                      │ Timeout │
├───────────┼──────────────────────────────┼─────────┤
│ deps      │ npm install --prefer-offline │ 900 s   │
│           │ --no-audit --no-fund         │         │
├───────────┼──────────────────────────────┼─────────┤
│ web tests │ npm test                     │ 600 s   │
└───────────┴──────────────────────────────┴─────────┘`
    expect(parseChecks(drawn)).toEqual({ checks: [EXPECTED[0], EXPECTED[3]], notes: [], problems: [] })
  })

  it('finds the columns by their headers, in any order, and ignores extra ones', () => {
    const got = parseChecks('| Command | Why | Time limit (s) | Check |\n|-|-|-|-|\n| `npm test` | the tests | 120 | tests |')
    expect(got.checks).toEqual([{ name: 'tests', command: 'npm test', timeoutSecs: 120 }])
  })

  it('reads rows without a header as name, command, time limit', () => {
    expect(parseChecks('| lint | `npx eslint .` |').checks).toEqual([{ name: 'lint', command: 'npx eslint .', timeoutSecs: 600 }])
  })

  it('keeps an escaped pipe inside its command', () => {
    expect(parseChecks('| lint | `npx eslint . \\| tee lint.txt` | 60 |').checks[0]!.command).toBe('npx eslint . | tee lint.txt')
  })

  it('names bare commands in a code block after what they run, and reads only the block', () => {
    const got = parseChecks('Run these:\n```sh\nnpm run typecheck\nnpm test\ncargo clippy -- -D warnings\nnpm test -- --coverage\n```')
    expect(got.checks).toEqual([
      { name: 'typecheck', command: 'npm run typecheck', timeoutSecs: 600 },
      { name: 'test', command: 'npm test', timeoutSecs: 600 },
      { name: 'clippy', command: 'cargo clippy -- -D warnings', timeoutSecs: 600 },
      { name: 'test 2', command: 'npm test -- --coverage', timeoutSecs: 600 },
    ])
    expect(got.notes).toEqual(['Only the lines inside the code block were read; 1 other line was left out.'])
  })

  it('reads a list, one check per line, with or without code spans and time limits', () => {
    const got = parseChecks(
      '- typecheck: `npm run typecheck` (10 min)\n* `web tests`: `npm test`\n1. **rust tests**: npm run test:rust (2400 s)\n- `npm run grep:secrets`\nnpm run test:rust',
    )
    expect(got.checks).toEqual([
      { name: 'typecheck', command: 'npm run typecheck', timeoutSecs: 600 },
      { name: 'web tests', command: 'npm test', timeoutSecs: 600 },
      { name: 'rust tests', command: 'npm run test:rust', timeoutSecs: 2400 },
      { name: 'grep:secrets', command: 'npm run grep:secrets', timeoutSecs: 600 },
      { name: 'test:rust', command: 'npm run test:rust', timeoutSecs: 600 },
    ])
    expect(got.notes).toEqual([])
    expect(got.problems).toEqual([])
  })

  it('keeps a command with a pipe whole when it is not a table row', () => {
    expect(parseChecks('npm test | tee out.txt').checks).toEqual([{ name: 'test', command: 'npm test | tee out.txt', timeoutSecs: 600 }])
  })

  it('says what it left out or assumed apart from what to change, and keeps the rest', () => {
    const got = parseChecks('| Name | Command | Timeout |\n|---|---|---|\n| tests | `npm test` | forever |\n| empty | | 60 |\n| tests | `npm run e2e` | 3 h |\n| quick | `npm run lint` | 5 |')
    expect(got.checks.map((c) => [c.name, c.timeoutSecs])).toEqual([
      ['tests', 600],
      ['tests', 10800],
      ['quick', 5],
    ])
    expect(got.notes).toEqual(['Line 3: couldn\'t read the time limit "forever", so it\'s 600 s.', 'Line 4 was left out: it has no command.'])
    expect(got.problems).toEqual([
      'Two checks are called "tests": rename one before saving.',
      'tests: 10800 s is outside 10 seconds to 2 hours; change it before saving.',
      'quick: 5 s is outside 10 seconds to 2 hours; change it before saving.',
    ])
  })

  it('flags a long name, a long command and more checks than a run takes', () => {
    const long = parseChecks(`| ${'n'.repeat(41)} | \`echo ${'x'.repeat(500)}\` |`)
    expect(long.problems).toEqual([
      `Line 1: the name "${'n'.repeat(41)}" is over 40 characters; shorten it before saving.`,
      `${'n'.repeat(41)}: write the command on one line, up to 500 characters, before saving.`,
    ])
    const many = parseChecks(Array.from({ length: 21 }, (_, i) => `npm run c${i}`).join('\n'))
    expect(many.checks).toHaveLength(21)
    expect(many.problems).toEqual(["That's 21 checks: a run has up to 20, so remove 1 before saving."])
  })

  it('finds nothing in empty text', () => {
    expect(parseChecks('')).toEqual({ checks: [], notes: [], problems: [] })
    expect(parseChecks('  \n\n---\n')).toEqual({ checks: [], notes: [], problems: [] })
  })

  it('reads time limits in seconds, minutes and hours', () => {
    expect([seconds('900'), seconds('900s'), seconds('900 s'), seconds('15 min'), seconds('1.5 h'), seconds('2 hours')]).toEqual([900, 900, 900, 900, 5400, 7200])
    expect([seconds('900ms'), seconds('soon'), seconds('')]).toEqual([null, null, null])
  })

  it('names a command after what it runs', () => {
    expect(['npm run typecheck', 'npm test', 'pnpm lint', 'npx vitest run', 'cargo test -p core', 'dotnet test', 'node scripts/check.mjs', 'gitleaks detect', '.\\build.ps1', '--weird'].map(nameFor)).toEqual([
      'typecheck',
      'test',
      'lint',
      'vitest',
      'test',
      'test',
      'check',
      'gitleaks',
      'build',
      'weird',
    ])
  })
})
