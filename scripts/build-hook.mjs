#!/usr/bin/env node
// Builds `chorus-hook` (Codex's hook relay, D46) in release and puts it where Tauri's `externalBin`
// looks for it: `src-tauri/binaries/chorus-hook-<target triple>.exe`. The installer then ships it
// beside `chorus.exe` as `chorus-hook.exe`. Run by `npm run build` (src-tauri/tauri.bundle.conf.json).
import { spawnSync } from 'node:child_process'
import { copyFileSync, mkdirSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..')

function run(cmd, args) {
  const r = spawnSync(cmd, args, { cwd: root, stdio: ['ignore', 'pipe', 'inherit'], encoding: 'utf8' })
  if (r.status !== 0) {
    console.error(`${cmd} ${args.join(' ')} failed`)
    process.exit(r.status ?? 1)
  }
  return r.stdout
}

const triple = /^host: (.+)$/m.exec(run('rustc', ['-vV']))?.[1]?.trim()
if (!triple) {
  console.error("couldn't read the target triple from `rustc -vV`")
  process.exit(1)
}
run('cargo', ['build', '-p', 'chorus-hook', '--release'])
const targetDir = process.env.CARGO_TARGET_DIR ? resolve(process.env.CARGO_TARGET_DIR) : join(root, 'target')
const out = join(root, 'src-tauri', 'binaries', `chorus-hook-${triple}.exe`)
mkdirSync(dirname(out), { recursive: true })
copyFileSync(join(targetDir, 'release', 'chorus-hook.exe'), out)
console.log(`chorus-hook -> ${out}`)
