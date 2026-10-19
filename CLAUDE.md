# Chorus 2.0 — Agent Instructions

Read `_docs/PLAN.md` and `_docs/ARCHITECTURE.md` before non-trivial work. Scope is `_docs/SCOPE.md`;
decisions are `_docs/DECISIONS.md`; hard-won facts about agent CLIs and Windows are `_docs/LESSONS.md`.

## What this is
Chorus 2.0 is a local-first, bring-your-own-key desktop app for supervising multiple AI coding agents
(Claude Code, Codex, opencode, Grok and shells) running in real terminals. It is a ground-up rebuild of
Chorus 1 (`../Chorus`), which is the reference implementation for proven behaviour. Windows only for v1.

## Stack (locked — do not substitute)
Tauri 2 (Rust back end, WebView2) · Vue 3 + TypeScript + Vite + Pinia · xterm.js · SQLite via rusqlite ·
portable-pty (ConPTY). No React. No Electron. No alternative state libraries.
Ask before adding any dependency not listed in `_docs/ARCHITECTURE.md` §10.

## Non-negotiable architecture rules
- **The back end owns sessions.** SessionManager in Rust owns every agent process. Windows and views
  attach to a `session_id`. The front end never spawns processes.
- **Pure logic lives in `crates/chorus-core`**: no IO, no Tauri, no filesystem, no network, no SQLite.
  If it can be computed from inputs, it goes there, with unit tests. `src-tauri` does IO and wiring.
- **IPC is typed end to end.** Commands and events use serde types with generated TypeScript bindings.
  Never hand-edit generated bindings. Validate every command's input at the Rust boundary (shape via
  serde, then semantics: ids exist, paths are inside the project, sizes are bounded).
- **Agent state comes from structured channels**: hooks, transcript files and agent servers. PTY silence
  may only extend liveness ("the agent stopped"), never assert "working". Never infer state by
  screen-scraping when a structured channel exists.
- **Every prompt is persisted** to SQLite, scrubbed, before or as it is delivered.
- **Fail visible.** When a feature degrades (unknown CLI version, missing channel, no key), show the reason
  in the UI. Never disable an option silently.
- **Agents run in a Job Object**, so Close and crashes never orphan processes.

## Secrets
- Keys are encrypted with Windows DPAPI (current-user) and stored as blobs. They are decrypted only at
  launch and injected **only as environment variables** of the child process.
- Never put a key or capability token in argv, a settings file, a log line, a transcript, history rows,
  narrator input or an export. The scrubber runs on all of them.
- Hook settings are passed to Claude Code with `--settings <file>` (the file form, never inline JSON) and
  live in 2.0's app data, never in the repo or worktree.

## Agent CLIs
- CLI flags and file formats move fast. Verify against the installed CLI's own `--help`, docs or files
  before hardcoding; don't trust training data. Record the version you verified against.
- **Never run `claude help <x>`.** There is no `help` subcommand, so it becomes a prompt and spends a
  turn. Use `--help`.
- Disable auto-update for launched agents (see LESSONS L7). Treat an unknown CLI version as "try and
  report", never "refuse silently".
- When probing a TUI, send prompt text and Enter as two separate writes (LESSONS L4).

## Data
- SQLite migrations are numbered and forward-only, in `src-tauri/migrations/`.
- Dev and release builds use different app identifiers and data directories. Never point a dev build at
  the installed app's data.

## Working style
- Build in the phase order in `_docs/PLAN.md`. Don't jump ahead to later phases' screens.
- A phase is done when its exit criteria are **seen working in the real app**, not just compiled and
  unit-tested.
- Prefer small, reviewable changes. Explain architectural choices briefly before large edits.
- Record decisions in `_docs/DECISIONS.md` (next free D-number) and new hard-won facts in
  `_docs/LESSONS.md`.
- Checks before committing: `cargo test`, `cargo clippy -- -D warnings`, `npm run typecheck`, `npm test`,
  and the secret scan.
- Line endings are LF (see `.gitattributes`). Don't introduce CRLF into source files.
- Assume the repo is public: don't commit test evidence, screenshots containing local paths, or
  machine-specific notes.
