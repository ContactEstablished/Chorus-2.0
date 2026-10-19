# Chorus 2.0

A local-first desktop app for supervising AI coding agents. Run Claude Code, Codex, opencode, Grok and
plain shells side by side in real terminals, then see at a glance which sessions need you, what each agent
did, and what to do next.

Chorus 2.0 is a ground-up rebuild of Chorus on **Tauri 2 (Rust)** with a **Vue 3** web UI. Windows only
for now. Bring your own keys: Chorus itself only talks to the providers you configure.

**Status:** planning. Development starts with Phase 0 (foundations and spikes).

## Docs

- [Plan](_docs/PLAN.md): principles, milestones, phases and spikes
- [Scope](_docs/SCOPE.md): what carries over from Chorus 1, what's new, what was dropped
- [Architecture](_docs/ARCHITECTURE.md): how it fits together
- [Design](_docs/design/DESIGN.md): screens, tokens and the splash brief
- [Decisions](_docs/DECISIONS.md) · [Lessons](_docs/LESSONS.md) · [Open questions](_docs/OPEN-QUESTIONS.md)

## Development

Set up in Phase 0. Expected toolchain: Rust (stable, MSVC), Node 22, WebView2 (ships with Windows 11).
