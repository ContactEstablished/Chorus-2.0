# Chorus 2.0 — Decisions

Fresh numbering for 2.0. Chorus 1's roadmap decisions (D1–D17x) are referred to as "Chorus 1 D<n>".
Add new decisions at the bottom; never renumber. Status is **Decided** (Matthew agreed) or **Proposed**
(recommended, awaiting his yes).

| # | Date | Decision | Status | Why |
|---|---|---|---|---|
| D1 | 2026-10-18 | Rebuild Chorus from scratch as **Chorus 2.0** in a new repo, keeping 77 of 92 features and dropping 14 (see SCOPE.md) | Decided | Shed features that never worked (Team sessions, memory graph, remote hosts…) instead of carrying their weight |
| D2 | 2026-10-18 | **Tauri with a web UI** (Rust back end, WebView2 front end), not Electron and not a native Rust UI | Decided | Keeps xterm.js and web design freedom; Rust back end; smaller and faster than Electron |
| D3 | 2026-10-18 | **Vue 3 + TypeScript + Vite + Pinia** for the front end | Decided | Existing patterns and design skill carry over |
| D4 | 2026-10-18 | Pin **Tauri 2.x** (CLI 2.12 at time of writing), not the 3.0 alpha | Proposed | Stable plugin ecosystem; revisit when 3.0 is stable |
| D5 | 2026-10-18 | **Re-enter API keys** in 2.0; no migration of Chorus 1's encrypted blobs | Decided | Chorus 1 keys are wrapped in Electron's OSCrypt key; migrating is fiddly for a handful of keys |
| D6 | 2026-10-18 | Session controls are **Restart and Close only** | Decided | Matthew: restart, relaunch, kill and close overlapped |
| D7 | 2026-10-18 | **Every prompt is written durably to SQLite** (WAL, `synchronous=FULL`), scrubbed first, from every source | Decided (responses scope open, Q6) | Chorus 1's prompt history didn't always survive a reboot or crash |
| D8 | 2026-10-18 | The startup splash becomes a **3D, few-second** piece | Decided | Matthew's triage note |
| D9 | 2026-10-18 | The **Harness mockups** are the UI direction: Overview board, Friendly/Raw session view, Next steps, Bots, Automations, Documents, Notes | Proposed (Q1) | Matthew added them to the new repo's `_assets/` |
| D10 | 2026-10-18 | Agent state comes from **structured channels** (hooks, transcripts, agent servers); PTY silence is only a liveness extender | Proposed | Screen-scraping fails silently with each CLI update; Chorus 1 measured these channels and found them reliable |
| D11 | 2026-10-18 | Business logic lives in a **`chorus-core` crate** with no IO and no Tauri | Proposed | Carries Chorus 1's `*Core.ts` rule, now enforced by the compiler |
| D12 | 2026-10-18 | **Typed bindings are generated** from Rust types; validation happens at the Rust boundary | Proposed | Removes Zod and the "object could not be cloned" trap; renames break the build, not the runtime |
| D13 | 2026-10-18 | Agents run inside a **Windows Job Object** | Proposed | Close and crashes never leave orphaned agent processes |
| D14 | 2026-10-18 | **Dev and release builds use different app identifiers** and data directories, and the status bar shows which database is live | Proposed | Chorus 1 ended up with five databases and dev and installed copies colliding |
| D15 | 2026-10-18 | **Notifications are in-app only** (centre, tray badge, fleet strip) | Decided | OS exit toasts were discarded; toasts are disabled on this machine |
| D16 | 2026-10-18 | **Telegram is the only remote access** | Decided | Remote hosts was discarded |
| D17 | 2026-10-18 | **Notes replace project memory**: plain project-tagged notes injected into agent instructions | Proposed (Q12) | The Neo4j graph measured zero agent writes |
| D18 | 2026-10-18 | **Mission Control becomes the Overview board**; forecasts wait until turn data is proven complete | Proposed (Q10) | Chorus 1 recorded 5 of 172 dispatches as finished |
| D19 | 2026-10-18 | **Fail visible**: any degraded feature (unknown CLI version, missing channel, no narrator key) says so on screen | Proposed | Chorus 1's costliest bugs were silent version gates and silently dropped options |
| D20 | 2026-10-18 | Local transcription shells out to a **downloaded `whisper-cli.exe`**, as in Chorus 1, rather than linking whisper.cpp | Proposed | Proven on this machine; avoids a native C++ build in the Rust toolchain |
| D21 | 2026-10-18 | Windows only for v1 | Proposed (Q14) | ConPTY, DPAPI, Job Objects and the tray badge are Windows-specific; Tauri keeps macOS possible later |
