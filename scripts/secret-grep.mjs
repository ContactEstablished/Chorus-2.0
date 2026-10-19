#!/usr/bin/env node
/**
 * Secret-grep gate, ported from Chorus 1 (scripts/secret-grep.mjs). Scans the repo's sources,
 * docs and _verify/ evidence for known credential shapes and exits non-zero on any hit, so the
 * check is a command anyone can run rather than an assertion.
 *
 * The patterns come from crates/chorus-core/secret-patterns.json, the same list the scrubber
 * uses. One list, two consumers.
 *
 * Limits, stated so nobody assumes more: this scans files in the repo (plus _verify/, which is
 * gitignored but is exactly where a careless dump would land a key). It does not scan agent
 * config under the user profile or 2.0's app data; the scrubber and the vault cover those paths.
 *
 * A match is never printed: only the file, line, pattern name and match length.
 *
 * Plain Node ESM, no dependencies. Usage: node scripts/secret-grep.mjs
 */
import { readFileSync, readdirSync } from 'node:fs'
import { join, relative } from 'node:path'
import { fileURLToPath } from 'node:url'

const repoRoot = fileURLToPath(new URL('..', import.meta.url))
const { patterns } = JSON.parse(
  readFileSync(new URL('../crates/chorus-core/secret-patterns.json', import.meta.url), 'utf8'),
)
const SECRET_PATTERNS = patterns.map((p) => ({ name: p.name, re: new RegExp(p.source, 'g') }))

/** Directories to scan, relative to the repo root. */
const SCAN_DIRS = ['src', 'src-tauri', 'crates', 'scripts', '_docs', '_verify']
/** Repo-root files: manifests, configs and docs (not lockfiles, which are full of opaque hashes). */
const SCAN_ROOT_FILE = /^(package\.json|Cargo\.toml|.*\.config\.(ts|js|mjs|cjs)|tsconfig.*\.json|.*\.md|index\.html|\.env.*)$/
const SKIP_DIRS = new Set(['node_modules', 'target', 'gen', 'dist', '.git'])
const NUL = String.fromCharCode(0)

function* walk(dir) {
  let entries
  try {
    entries = readdirSync(dir, { withFileTypes: true })
  } catch {
    return // absent optional dir (for example _verify on a fresh clone)
  }
  for (const e of entries) {
    const full = join(dir, e.name)
    if (e.isDirectory()) {
      if (!SKIP_DIRS.has(e.name)) yield* walk(full)
    } else if (e.isFile()) {
      yield full
    }
  }
}

const hits = []
let scanned = 0
function scan(file) {
  let text
  try {
    text = readFileSync(file, 'utf8')
  } catch {
    return
  }
  if (text.includes(NUL)) return // binary
  scanned++
  text.split('\n').forEach((line, i) => {
    for (const { name, re } of SECRET_PATTERNS) {
      re.lastIndex = 0
      let m
      while ((m = re.exec(line)) !== null) {
        hits.push({ file, line: i + 1, pattern: name, length: m[0].length })
      }
    }
  })
}

for (const dir of SCAN_DIRS) {
  for (const file of walk(join(repoRoot, dir))) scan(file)
}
for (const e of readdirSync(repoRoot, { withFileTypes: true })) {
  if (e.isFile() && SCAN_ROOT_FILE.test(e.name)) scan(join(repoRoot, e.name))
}

if (hits.length > 0) {
  console.error(`secret-grep: ${hits.length} key-shaped string(s) found:`)
  for (const h of hits) {
    console.error(`  ${relative(repoRoot, h.file)}:${h.line}  [${h.pattern}] (${h.length} chars, masked)`)
  }
  process.exit(1)
}
console.log(`secret-grep: clean (${SECRET_PATTERNS.length} patterns over ${scanned} files)`)
