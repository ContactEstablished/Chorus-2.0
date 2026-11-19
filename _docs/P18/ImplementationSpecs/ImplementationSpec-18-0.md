# Implementation spec 18-0: Spikes

The results are in [spikes/P18.md](../../spikes/P18.md). This spec lists the live runs still to do
once Matthew has chosen how Chorus gets its MausBot token (spikes §4). It also records how the
spike evidence was gathered, so it can be repeated after a MausBot update.

## Repeating the read-only checks

The program lives in the per-user Programs folder, under `openmausbot\`. Its bundled server is
readable JavaScript in `resources\server\`.

- **The CLI** runs as the app's own Node, with `ELECTRON_RUN_AS_NODE=1` and the app's executable on
  `resources\server\openmausbot.js`. `openmausbot` itself isn't on PATH for the desktop install.
- **Version:** `curl -s http://127.0.0.1:8799/api/health` → `"version"`.
- **No token needed for reads:** `curl -s -o NUL -w "%{http_code}" http://127.0.0.1:8799/api/bots`
  → 200. Print only shapes, never content.
- **Refused write:** `curl -X POST -H "content-type: application/json" -d "{}"
  http://127.0.0.1:8799/api/bots/x/tasks` → 403.

## Live runs once a token exists

Pair first: Matthew makes a pairing code in MausBot (Settings → Phone & devices), with full access
if offered, and enters it in Chorus's Settings → MausBot (built in 18.2) in the `p18` instance.
Then:

1. **Make the test bot.** A bot named "Chorus test", made through the token
   (`POST /api/bots {"name":"Chorus test"}`) or by Matthew in MausBot. Nothing else in MausBot is
   changed.
2. **S1 live.** In a scratch repository project, Hand to MausBot with "Chorus test" and the task
   "Add a line to NOTES.md and commit it". Pass when all of these hold:
   - the conversation's `cwd` in `GET /api/bots` is the `maus/…` worktree
   - the bot's commit lands on `maus/…`
   - the project's main checkout is untouched
3. **S3 live.** With the same run, see whether the bot added its own entry at the top of
   `<worktree>/.chorus/handoff.md` and set `baton: chorus`. Record the result. If it didn't, Chorus's
   "from git" entry must appear instead.
4. **Clean up.** Delete the test conversation in MausBot or leave it, as Matthew prefers. Revoke
   the token only if Matthew asks.

Write the outcomes into `spikes/P18.md` under S1 and S3, replacing "waits for a token".
