# Chorus 2.0

A local-first desktop app for supervising AI coding agents. Run Claude Code, Codex, opencode, Grok and
plain shells side by side in real terminals, then see at a glance which sessions need you, what each agent
did, and what to do next.

Chorus 2.0 is a ground-up rebuild of Chorus on **Tauri 2 (Rust)** with a **Vue 3** web UI. Windows only
for now. Bring your own keys: Chorus itself only talks to the providers you configure.

**Status:** Phase 1 (session engine) is built: sessions owned by the back end, scrollback saved and
replayed, restore after a restart or crash, Restart and Close, the key vault, the agent lock and a
bare Raw view. Its exit check is waiting for one reboot. Phase 0 closed on 2026-10-19; every spike has
a written result in [`_docs/spikes/`](_docs/spikes/README.md).

## Docs

- [Plan](_docs/PLAN.md): principles, milestones, phases and spikes
- [Scope](_docs/SCOPE.md): what carries over from Chorus 1, what's new, what was dropped
- [Architecture](_docs/ARCHITECTURE.md): how it fits together
- [Design](_docs/design/DESIGN.md): screens, tokens and the splash brief
- [Decisions](_docs/DECISIONS.md) · [Lessons](_docs/LESSONS.md) · [Open questions](_docs/OPEN-QUESTIONS.md)

## Development

Toolchain: Rust (stable, MSVC), Node 22.12 or later, WebView2 (ships with Windows 11). Versions verified
so far are in [_docs/spikes/README.md](_docs/spikes/README.md).

```sh
npm install
npm run dev        # dev build: identifier com.contactestablished.chorus2.dev, its own data directory
npm run build      # release build and per-user NSIS installer
npm run check      # typecheck, vitest, cargo test, clippy -D warnings, secret scan
npm run bindings   # regenerate src/ipc/bindings.ts from the Rust IPC types
```

Layout: `src/` is the Vue front end, `src-tauri/` the Tauri app crate (IO and wiring),
`crates/chorus-core/` the pure logic (no IO; enforced by `tests/purity.rs` and `clippy.toml`).
