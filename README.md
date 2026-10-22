# Chorus 2.0

A local-first desktop app for supervising AI coding agents. Run Claude Code, Codex, opencode, Grok and
plain shells side by side in real terminals, then see at a glance which sessions need you, what each agent
did, and what to do next.

Chorus 2.0 is a ground-up rebuild of Chorus on **Tauri 2 (Rust)** with a **Vue 3** web UI. Windows only
for now. Bring your own keys: Chorus itself only talks to the providers you configure.

**Status (2026-10-22):** P0–P5 are closed; the Daily driver milestone is complete with the recorded
waivers. P4's full day of real-work acceptance was explicitly waived, not performed (D72). P5's
worktree reuse, recovery, panel and diff summaries are implemented and verified in the app.
**Next: Friendly view and Next steps (P6), then attention (P7), for the Supervisor milestone.**

There are **14 active phases**, numbered P0–P14 with P9 (voice) removed. Worktree launch and Close
were brought into P4 from P5, launch routing from P12, and part of P14 is already merged (Gemini,
Aider, Ollama, CI and installer polish). P12 and P14 still have remaining work. The
[roadmap's status table](_docs/PLAN.md#current-position-2026-10-22) is the current source of truth;
it links the preserved hand-off plans and distinguishes implementation from runtime acceptance.
Recorded limits remain: P1's reboot check was waived; Grok has only launch verification and no event
channel; Gemini and Aider still need completed real turns. The review fixes in Q28–Q30 are implemented
(conditional folder trust, corrected permission labels, isolated history; D69–D71).

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
