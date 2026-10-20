# Chorus 2.0 — Scope

Baseline: Matthew's triage of all 92 Chorus 1 features on 2026-10-18, recorded in the triage artifact,
plus the "Harness" mockups in `_assets/`. After his follow-up the same day (voice moves to a separate
app, D22), the count is **73 kept, 14 discarded, 4 moved out, 1 deferred**. Phases refer to
[PLAN.md](PLAN.md); D-numbers to [DECISIONS.md](DECISIONS.md).

## Changes Matthew asked for

| Feature | Change |
|---|---|
| Pane controls | **Restart and Close only.** Relaunch-with-fresh-key folds into Restart (a capped key is re-minted on every launch anyway); "kill but keep" goes away. |
| Prompt history | **Every prompt durably in SQLite**, surviving crashes and reboots. Each turn also keeps the agent's final message, its tool-call list, and any question or approval with its answer (D7). |
| Startup splash | **3D, eye-catching, still only a few seconds.** Brief in [design/DESIGN.md](design/DESIGN.md#startup-splash). |

## New in 2.0

From the Harness mockups, confirmed as the UI direction (D9), plus one setup aid (D33).

| Feature | What it is | Phase |
|---|---|---|
| Overview board | Sessions as cards grouped Needs you / In flight / Finished (or by project), with filters, project tags and a headline count. **This is the Mission Control redesign.** | P4 |
| Friendly view | Readable timeline: your messages, agent messages, plan checklist, collapsed tool-call groups, question and approval cards | P6 |
| Raw view | The real terminal, same session; switching never restarts the agent; unsent drafts carry over | P4 |
| Now / Blocker / Up next | Three-line live summary at the top of a session | P6 |
| Next steps panel | Suggested actions (Continue, Answer, Approve, Deny, Retry) showing exactly what each sends and roughly how long it takes; blocked steps say why; done steps are never suggested again | P6 |
| Queued instructions | Text sent while an agent is busy is delivered at its next pause | P6 |
| Session facts | Project, bot, model, started, tokens, changes | P4 (tokens P11, changes P5) |
| Bots | Reusable agent configuration: provider, model, effort, permissions, instructions, tags. Starting one creates a session; editing one never changes running sessions. Grows out of launch profiles and presets. | P8 |
| Automations | A Bot run on a schedule or trigger; each run is an ordinary session | P8 |
| Documents | Plans, specs and reports produced by sessions, tagged by project and linked to the session | P8 |
| Notes | Project-tagged context for agents and future-you; untagged notes apply everywhere. Injected into agent instructions. | P8 |
| Themes | Glacier (default) and Dusk (D25) | P4 |
| Import projects from Chorus 1 | Dropped on 2026-10-20 (D50): Matthew brings projects over by hand | — |
| Session tags | Free-form labels beside the project, typed with autocomplete; on cards, in the facts panel and in search (D52) | P4 |

## Kept features and where they live

### Sessions
| Feature | 2.0 home | Phase |
|---|---|---|
| Real terminal panes | Raw view (xterm.js over ConPTY) | P1 |
| Sessions outlive their panes | SessionManager in Rust | P1 |
| Restore after restart | Boot restore + heal | P1 |
| Conversation resume | Per-adapter capability | P2 |
| Saved scrollback | Scrollback mirror + replay | P1 |
| Auto-titled panes | Session title from the terminal title until you rename it | P4 |
| Session name and note | Launch flow; card and header | P2 |
| Pane controls | **Restart and Close** | P1 |
| Terminal copy and paste | Raw view | P1 |
| Prompt history | Durable SQLite history + per-session panel with copy | P3, P4 |
| Plain shell pane | Shell agent | P1 |
| Agent lock | Lock with optional PIN | P1 |
| Activity lights and bar | Status dots, card bars and rail lights from the event model | P3 |

### Layout
| Feature | 2.0 home | Phase |
|---|---|---|
| Focus + filmstrip view | Session view + session strip (D26) | P4 |
| Grid view | "Wall" layout of Raw terminals under Sessions (D26) | P4 |
| Project rail | Projects section of the rail | P4 |
| Hide, archive and delete projects | Project menu | P4 |
| Project colors | Project tags and colours | P4 |
| Command palette (Ctrl+K) | Palette, now also searching history | P4 |
| Project switcher (Ctrl+G) | Switcher | P4 |
| Status bar | Status bar | P4 |
| Custom title bar | Frameless main window | P4 |
| Startup splash | **3D splash** | P4 (spike S7) |
| Text size zoom | Zoom | P4 |
| In-app notices | Toasts and notices | P4 |

### Agents
| Feature | 2.0 home | Phase |
|---|---|---|
| Claude Code | Adapter | P2 |
| Codex | Adapter | P2 |
| opencode | Adapter (server + a TUI connected to it; 2.x since D58) | P2 |
| Grok | Adapter | P2 |
| CLI detection | Settings → Agents | P2 |
| Capability-driven adapters | Adapter declarations + conformance test | P2 |
| Launch dialog | New session flow | P2, P4 |
| One effort scale | Fast / Balanced / Deep / Max | P2 |
| Launch presets | Solo, Pair, Workbench, Swarm | P2 |
| Saved launch profiles | Become **Bots** | P2 → P8 |
| More agents (Gemini, Aider, Ollama) | Adapters | P14 |
| DeepSeek Harness (`dsh`) | DeepSeek's own agent on the DeepSeek API directly, its key from the vault as an environment variable; spike S9 first (D59, Q21) | P14 |

### Git
| Feature | 2.0 home | Phase |
|---|---|---|
| Worktree per agent | Launch option: suggested when the folder's tree is in use (D53) | P4 (attach to a kept one: P5) |
| Boot reconcile | Boot | P5 |
| Worktree panel | Settings / project view | P5 |
| Diff summary on panes | Session facts "Changes" + card | P5 |
| Keep or remove on close | Close flow | P5 |

### Keys and providers
| Feature | 2.0 home | Phase |
|---|---|---|
| Encrypted key vault | DPAPI vault (keys re-entered) | P1 |
| Secret scrubber | Output, history, narrator and exports | P1 |
| Provider routes | Settings → Providers | P2 |
| Model catalog and shortlist | Settings → Providers | P2 |
| OpenRouter capped keys | Per launch | P2 |
| Repo secret scanner | Script + CI | P0 |
| JEV AI key | Settings → Providers: a stored key for TypeSafe's Jev API (typed decisions, not text) with a connection test (D40) | P2 |

### Routing
| Feature | 2.0 home | Phase |
|---|---|---|
| Routing tiers | opencode on OpenRouter, in New session (D57) | P4 |
| Endpoint observation | Background sampler, with the provider the user ticks (D57) | P4 |
| Routing inspector | Settings → Routing | P12 |

### Council
| Feature | 2.0 home | Phase |
|---|---|---|
| Council review | Council screen | P10 |
| Dissent enforced in code | Council core | P10 |
| Council docket | Council screen (cost reconciled this time) | P10 |
| Council member setup | Council settings | P10 |
| Council cases and exhibits | Attach repo files to a brief | P10 |

### Measurement
| Feature | 2.0 home | Phase |
|---|---|---|
| Attention sampler | Background; now shown | P11 |
| Per-task cost attribution | Session facts, Usage screen | P11 |
| Subscription metering | Usage screen | P11 |
| Turn capture | Event model (foundation of P3) | P3 |
| Token usage panel | Session facts "Tokens", Usage screen | P11 |
| Day summary | Overview / palette | P11 |

### Multi-agent and attention
| Feature | 2.0 home | Phase |
|---|---|---|
| Fleet addressing (Claude) | Session header chip + roster | P7 |
| Attention inbox | Overview "Needs you" with j/k | P7 |
| Event timeline | Folded into the Friendly timeline | P6 |
| Notification centre and tray badge | In-app centre + tray icon badge | P7 |
| Fleet switcher overlay | Always-on-top status strip | P7 (spike S3) |
| Scrollback search | Raw view search | P7 |
| Transcript export | Session menu | P7 |
| Skills framework | Bots (instructions + skill folders) and Automations (app-run work) (D31) | P8 |
| Mission Control board | Overview board; forecasts only after P3 data proves complete (D18) | P4, P11 |
| Phone relay (Telegram) | Relay using the same action pipeline | P13 |

### Ship
| Feature | 2.0 home | Phase |
|---|---|---|
| Windows installer | Tauri NSIS, per-user | P0 (spike S8), P14 |

## Moved to a separate app

Matthew wants dictation in every application, not only in Chorus, so voice becomes a separate,
system-wide dictation app (D22). Chorus stays compatible with it like any text field: it never steals
focus, the composer and the Raw terminal accept inserted or pasted text, and nothing it receives is
auto-submitted (D23).

| Feature | Chorus 1 behaviour, now the dictation app's job |
|---|---|
| Push-to-talk and click-to-talk | Global hotkey (Scroll Lock by default) and a click target |
| Local transcription | `whisper-cli.exe`, downloaded on first use |
| Mic overlay and target ring | Always-on-top level meter, and a ring on the field that will receive the text |
| Transcript cleanup | Verbatim / Clean up / Organize, keeping the original |

## Deferred

| Feature | Note |
|---|---|
| Pop-out windows | No verdict in the triage; deferred (D27). Any window can attach to a session, so it can come later. |

## Discarded

| Feature | Why it went |
|---|---|
| Team sessions | Never faster or cheaper than one agent (median 194 s vs 51 s; 3.4M vs 0.31M lead tokens) |
| Project memory graph (Neo4j) | Zero agent writes ever measured; replaced by plain Notes |
| Memory schema and provenance | Part of the graph |
| Index code | Part of the graph |
| Symbol index | 51.6% of call links were name-match guesses |
| Remote hosts | Unfinished; Telegram is the remote channel |
| Kimi Code | Dropped as a CLI (Kimi models remain reachable through OpenRouter) |
| Context ring | Dropped |
| Desktop notification on exit (OS toast) | Toasts are disabled on this machine; notifications are in-app |
| API chat pane | Never built |
| Agent messaging UI | Never built |
| Token reduction | Never built |
| Cloud speech-to-text | Never built; dictation now lives in a separate app (D22) |
| Custom keybindings | Never built |
