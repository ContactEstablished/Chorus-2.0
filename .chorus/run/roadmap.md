# Roadmap: Chorus 2.0

The first Chorus Loop run on Chorus 2 itself (D102's real-world test). Read `decisions.md` beside
this file before planning or building: it says what each phase includes, what it leaves out, and
where this run overrides `CLAUDE.md` and `_docs/PLAN.md`.

## P12 · Routing inspector
- Goal: Settings → Routing shows how OpenRouter's endpoints rank a model for Budget, Balanced, Fast and Nitro. It lets Matthew change every ranking setting, shows the background observer's status with a per-model Stop, and keeps each endpoint's speed history and a log of Refreshes with their spend. Routing refuses Anthropic and OpenAI models and says why.
- Risk: normal

## P14 · More agents and ship
- Goal: an agent that isn't installed says how to install it; Gemini CLI's self-updating is stated on screen; and Settings → Agents describes version checks accurately. Only the part of P14 that needs no installs, keys, spending or release decisions.
- Risk: normal
