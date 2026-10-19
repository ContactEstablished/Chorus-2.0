#!/usr/bin/env node
// Regenerates src/ipc/bindings.ts from the Rust IPC types (src-tauri/src/ipc.rs) without
// launching the app. `cargo test` fails while the committed file is stale.
import { spawnSync } from 'node:child_process'

const result = spawnSync(
  'cargo',
  ['test', '-p', 'chorus', '--lib', 'ipc::tests::bindings_are_current', '--', '--exact'],
  { stdio: 'inherit', env: { ...process.env, CHORUS_UPDATE_BINDINGS: '1' } },
)
process.exit(result.status ?? 1)
