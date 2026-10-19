# Phase 0 spikes

Each spike answers one question before P1 builds on it (PLAN.md §4, P0). Results are summarised here
and written up in `S<n>.md`; raw evidence stays in `_verify/`, which is never committed.

| Spike | Question | Result |
|---|---|---|
| S1 | PTY host: ConPTY to xterm.js over a Tauri channel, Job Object cleanup | [Pass](S1.md): 10 MB burst with no frame over 9 ms; Close and crash kill whole trees; Close must spare browsers |
| S2 | ~~Push-to-talk hotkey~~ | Removed (D22) |
| S3 | Fleet strip: on top, never activated, draggable | [Changed approach, pass](S3.md): needs a native input shield; then an external app keeps focus through clicks and drags. Multi-DPI untested (one monitor) |
| S4 | DPAPI vault and env-only key injection | [Pass](S4.md): key reaches the child (length and hash match), never in argv, redacted on screen even when split |
| S5 | Agent channels per CLI | [Pass, Grok changed approach](S5.md): Claude Code, Codex and opencode approvals, questions and queueing answered from Chorus; Grok unverifiable (not signed in) |
| S6 | ~~Local transcription~~ | Removed (D22) |
| S7 | 3D splash | pending |
| S8 | Per-user installer beside Chorus 1 | pending |

## Toolchain (verified 2026-10-18)

| Tool | Version |
|---|---|
| Windows | 11 Home, build 26200 |
| rustc / cargo | 1.98.1 (stable, `x86_64-pc-windows-msvc`) |
| MSVC | Visual Studio 2022 17.14 (also 2026 18.11 installed) |
| node / npm | 22.14.0 / 11.12.1 |
| WebView2 runtime | 155.0.4283.62 (evergreen, machine-wide) |

## Pinned versions

Tauri is pinned to 2.x (D4); 3.0 is in alpha (`tauri` 3.0.0-alpha.5, `@tauri-apps/cli` 3.0.0-alpha.5 on
2026-10-18). `Cargo.lock` and `package-lock.json` hold the exact resolved versions.

| Crate / package | Requirement | Resolved |
|---|---|---|
| `tauri` | `2.12` | 2.12.4 |
| `tauri-build` | `2.7` | 2.7.3 |
| `@tauri-apps/cli` / `@tauri-apps/api` | `^2.12.3` / `^2.12.4` | 2.12.3 / 2.12.4 |
| `specta` / `tauri-specta` | `=2.0.0-rc.25` (release candidates; pinned exactly, as their docs require) | rc.25, which requires `tauri ^2` |
| `specta-typescript` | `=0.0.12` | 0.0.12 |
| `vue` / `pinia` | `^3.5.44` / `^4.1.0` | |
| `vite` / `@vitejs/plugin-vue` | `^8.3.4` / `^6.0.10` | |
| `typescript` | `~6.0.3` | TypeScript 7 is the native (Go) compiler, which `vue-tsc` cannot drive yet |
| `vue-tsc` / `vitest` | `^3.3.13` / `^5.0.5` | |

`@vitejs/plugin-vue` is not named in ARCHITECTURE §10 but is the Vite plugin that the approved Vue + Vite
stack requires; it adds nothing beyond that.

## Agent CLIs on this machine

| CLI | Version | Install |
|---|---|---|
| Claude Code | 2.1.300 | native `claude.exe` in `~/.local/bin` |
| Codex | codex-cli 0.162.1 | npm shim `codex.cmd` → `node …\@openai\codex\bin\codex.js` |
| opencode | 1.18.34 | npm shim `opencode.cmd` |
| Grok | 1.0.5 at the start of S1; **1.0.62** after it updated itself on first launch (S5 finding 1) | native `grok.exe` in `~/.grok/bin` |
