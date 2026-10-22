# Hand-off for P14: staying mergeable once `handoff/oc2` lands

Written 2026-10-20 by the `oc2` hand-off (opencode 2.1.1 gaps) for the session working on P14
(`feature/p14`, worktree `../Chorus-2.0.worktrees/feature/p14`). Everything in `HANDOFF-COMMON.md`
still applies: your branch only, no pushing, no merging, no commits to `main`.

> **Update, 2026-10-20, 22:10: done, by a merge.** The original `feature/p14` (`a7612a2`) was pushed
> to GitHub's `main` at 22:05, so the main line merged it into its local `main` (oc2, `0e91e1e`) as
> `fec5ccd`, resolved as in §4. The merged tree is identical to `feature/p14-rebased` (`5fe9d57`), so
> that branch's commits are no longer needed. Commit new work on a branch from `main`
> (`git switch -c <branch> main` keeps uncommitted changes, since the trees match). Never push
> `feature/p14-rebased` to `main`: it isn't a descendant of GitHub's `main`.

## 1. Where you stand

- `feature/p14` (tip `a7612a2`) is already rebased on `main` (`45eb41f`). **Today it merges into
  `main` without conflicts.** Nothing to do for `main` as it is.
- One other branch changes the same code: **`handoff/oc2`** (tip `73b1989`, also on `45eb41f`, 4
  commits; the last one only edits its review). It fixes gaps in `main`'s opencode 2.x port. Its review
  is `_docs/HANDOFF-OC2-REVIEW.md` on that branch (`git show handoff/oc2:_docs/HANDOFF-OC2-REVIEW.md`).
- **`handoff/oc2` is not yours to rebase, reset or edit,** and neither is its worktree
  `../Chorus-2.0-oc2`. On 2026-10-20 a session rebased it onto `feature/p14` and dropped `keyless`; it
  has been put back. That attempt survives as the ref `oc2-rebased-onto-p14` (`e082dfe`): don't build
  on it or merge it. The only branch you change is `feature/p14`.
- A trial merge of the two (`git merge-tree --write-tree handoff/oc2 feature/p14`) conflicts in
  **three files, all at opencode's model check**:
  - `crates/chorus-core/src/opencode.rs`
  - `src-tauri/src/agents/opencode.rs`
  - `src-tauri/src/sessions/mod.rs`

  Every other overlapping file merges automatically: `cli.rs`, `agents/commands.rs`, `agents/mod.rs`
  and `bindings.ts`, plus the other hunks of `sessions/mod.rs`.
- The resolution below was built in a throwaway merge of both branches and checked there with
  `npm run check` (§6).

## 2. What `handoff/oc2` changes that touches your code

1. **`check_model` gains an integrations argument.** 2.1.1 has no `connected` list. oc2 reads `GET
   /api/integration` to tell "no key or login" (`ModelCheck::NoKey`) apart from "switched off"
   (`NotConnected`, `activation: "disabled"`). Its signature on oc2:
   `check_model(providers_body, models_body, integrations_body, model)`. Yours adds `keyless` at the
   end. **The merged signature takes both:**
   `check_model(providers_body, models_body, integrations_body, model, keyless)`.
2. **`start_opencode` computes the variant once**, as `variant`, before creating or resuming the
   opencode session. Your hunk recomputes it as `wanted`; use oc2's `variant`.
3. **A resumed opencode run sets the session's model again** with `POST /api/session/{id}/model`
   (the TUI's last pick otherwise wins on resume). This also runs for an Ollama route, with
   `{providerID: "ollama", id: "<model>"}`. Check it at runtime (§5).
4. **`forget_tui_choice` is removed** (2.1.1's TUI keeps no `model.json`), and `start_opencode` takes
   `(parent, state_dir)` for a version gate. You call neither, so this merges cleanly.
5. **opencode below 2.x is refused by name** in detection (`found: false`) and at launch. That
   doesn't affect Gemini or Aider.

## 3. When to do it

- **If `handoff/oc2` is merged into `main` first** (the expected order: it is small, and its
  review is done), rebase your branch onto the new `main` and resolve as in §4.
- **If `feature/p14` is merged first**, do nothing. The oc2 side resolves it the same way.
- Don't rebase onto `handoff/oc2` before it is on `main`. It may still change in review.

## 4. The resolution, exactly

Expect the conflicts while replaying `P14: Ollama as a keyless provider for opencode`, and again in
any later commit that touches the same lines. Resolve them all the same way.

**`crates/chorus-core/src/opencode.rs`: keep oc2's `integration`, `connected` and doc comment, add
your `keyless`, and let it bypass both checks:**

```rust
/// … (oc2's doc comment, ending "or with no integration (Ollama) needs nothing more.")
///
/// `keyless` is a route whose provider needs no key, such as Ollama on this machine: opencode marks
/// that provider disabled for want of a login, which is not the same as the model not running, so
/// neither "switched off" nor "no key" applies to it.
pub fn check_model(providers_body: &str, models_body: &str, integrations_body: &str, model: &str, keyless: bool) -> Option<ModelCheck> {
    // … unchanged from oc2 down to:
    if !keyless && p.get("activation").and_then(Value::as_str) == Some("disabled") {
        return Some(ModelCheck::NotConnected);
    }
    let own_key = p.pointer("/settings/apiKey").and_then(Value::as_str).is_some_and(|k| !k.is_empty());
    let signs_in = p.get("integrationID").and_then(Value::as_str).and_then(|i| integration(&integrations, i));
    if !keyless && !own_key && signs_in.and_then(connected) == Some(false) {
        return Some(ModelCheck::NoKey);
    }
    // … rest unchanged
```

Tests in the same file:
- Keep oc2's tests (`the_model_is_checked_against_the_server`, `a_provider_without_a_key_is_named`,
  `a_resumed_session_is_given_its_model_again`) and add `, false` to every `check_model` call.
- Move your Ollama assertions into `the_model_is_checked_against_the_server`, passing `NO_KEYS` as the
  integrations argument:
  ```rust
  assert_eq!(check_model(WITH_OLLAMA, OLLAMA_MODELS, NO_KEYS, "ollama/llama3.2:1b", false), Some(ModelCheck::NotConnected));
  let known = check_model(WITH_OLLAMA, OLLAMA_MODELS, NO_KEYS, "ollama/llama3.2:1b", true).unwrap();
  ```
- Add one line to `a_provider_without_a_key_is_named`:
  ```rust
  // A keyless route is never "no key", whatever its integration says.
  assert!(matches!(check_model(PROVIDERS, MODELS, NO_KEYS, routed, true), Some(ModelCheck::Known { .. })));
  ```
- Your old assertion `check_model(PROVIDERS, MODELS, "anthropic/claude-sonnet-4-6", false) ==
  NotConnected` goes. In oc2's fixtures Anthropic is an integration with no key (`NoKey`), and the
  switched-off provider is `switched/x`.

**`src-tauri/src/agents/opencode.rs`: keep oc2's integration fetch, and pass `keyless` through.**
Your doc comment and signature for `Api::check_model(&self, model, keyless)` stay as they are:

```rust
        // Which providers have a key or login. Without this answer the check makes no claim about
        // keys, and still checks the provider, model and effort.
        let integrations = match self.get("/api/integration") {
            Ok((200, body)) => body,
            _ => String::new(),
        };
        Ok(opencode::check_model(&providers, &models, &integrations, model, keyless))
```

**`src-tauri/src/sessions/mod.rs`: keep your `keyless`, use oc2's `variant`.** Also fix your comment,
because 2.x has no `connected` list:

```rust
        // opencode lists a provider that takes no key (Ollama) as disabled, which is not the same as
        // the model not running.
        let keyless = row.provider_id.is_some_and(|id| {
            provider_rows::get(&self.store, id).ok().flatten().is_some_and(|p| p.kind == ProviderKind::Ollama)
        });
        if let Some(model) = &options.model {
            match server.api.check_model(model, keyless) {
                Ok(Some(check)) => notes.extend(oc::model_note(model, variant.as_deref(), &check)),
                Ok(None) => notes.push("opencode's model list isn't in the shape Chorus checked, so the model wasn't checked.".into()),
                Err(e) => notes.push(format!("Chorus couldn't check the model with opencode: {e}")),
            }
        }
```

Then run `npm run bindings`. oc2 changed two doc comments in the generated file (`launchArgs`,
`Detection.found`), and regenerating is the rule.

## 5. Checks after the rebase

1. `npm run check` passes. Expect the 5 opencode tests oc2 added, and yours, in the counts.
2. In your own instance (`com.contactestablished.chorus2.dev.p14`, your port), with Ollama running:
   - A new opencode session on your Ollama provider has **no** note: not "switched off", not
     "no key". A turn runs.
   - **Restart that session.** It resumes the same `ses_…` with no note "Chorus couldn't set the
     resumed conversation's model…". This is oc2's resume path on your route, and nobody has run it
     yet.
   - Optional sanity check of oc2's side: an opencode session with model
     `openrouter/deepseek/deepseek-v4.1-flash` and **no** provider shows "opencode has no key or login
     for openrouter in this session…" before any prompt.
3. Read the merged `start_opencode` once from top to bottom. It should have, in order: the version
   gate, `Server::start`, `session_exists`, one `variant`, resume with `set_model` or create, then
   your `keyless` and the model check.

## 6. What was checked for this hand-off

- Trial merge of `handoff/oc2` (`6db0057`; `73b1989` adds only a review edit) and `feature/p14` (`a7612a2`) in a throwaway worktree,
  resolved exactly as in §4, bindings regenerated: `npm run check` passed. That's typecheck clean,
  vitest 19, `chorus-core` 367, `src-tauri` 47, the other crates 3 + 3, clippy `-D warnings` clean,
  and the secret scan clean (7 patterns, with your Google key pattern). The throwaway branch was
  deleted afterwards.
- Not run: the app on the merged tree, since Ollama isn't set up in the oc2 instance. §5's runtime checks
  are yours.

## 7. Your review file, after the rebase

- §Branch: the new base commit, and the conflicts you resolved (these three files).
- §Files: `check_model(…, integrations, model, keyless)`, not `check_model(…, keyless)`.
- **P14-L1:** say which provider shape you measured. oc2 saw 2.1.1's **built-in** `ollama` provider
  (no configuration) as `activation: "enabled"` with no integration, at `127.0.0.1:11434/v1`. You saw
  `"disabled"` with Chorus's `npm`/`options.baseURL` block. Both can be true. Since 2.1.1 already has
  that built-in provider, it may be worth one line in your review on whether the block is still needed.
  It works, so this is not a request to change it.
- Proposed doc ids don't collide: oc2 proposes OC2-D1 to D3 and OC2-L1, and the reviewer numbers both.

## 8. Not your concern

- Port 1435: the oc2 instance moved to 1445 and is no longer running.
- `_docs/HANDOFF-OC2-opencode.md`: you have already marked it superseded. oc2's review (question 4)
  asked for exactly that.

## 9. A P14 bug to fix on `feature/p14` before it merges: Ollama's model name

Found 2026-10-20 in a dev instance on a merge of both branches. The cause is entirely P14's code, as
committed at `a7612a2`. An opencode session on the Ollama provider gets a wrong note and its first turn
fails:

```
note: Ollama doesn't list ollama/llama3.2:1b, so this turn may fail: pull it or pick another model.
turn: failed — model 'ollama/llama3.2:1b' not found
```

Why:
- `Spec::resolve` (`crates/chorus-core/src/agent.rs`, the `is_provider_model` arm) requires opencode's
  model as `provider/model`, so with the Ollama provider the user has to type `ollama/llama3.2:1b`. The
  bare `llama3.2:1b` is refused.
- With a provider, `options.model` is meant to be **that provider's own model id** (`CreateSession`'s
  doc). OpenRouter's ids contain a `/` (`deepseek/deepseek-v4.1-flash`), so they pass the rule by
  accident. Ollama's (`llama3.2:1b`) don't.
- So the Ollama check in `sessions/mod.rs` compares `ollama/llama3.2:1b` with `/api/tags`'s bare
  `llama3.2:1b` (the wrong note), and `providers::route` builds `ollama/{model}` with the block's
  `models` key set to `ollama/llama3.2:1b`. opencode then asks Ollama for that id, which doesn't exist.

The fix is yours to choose. One way: when a session runs on a provider, its route owns the model name,
so `resolve` skips the `provider/model` rule (or the launch resolves after the route has prefixed
`ollama/`), and New session takes Ollama's own id from the provider's model list. Whichever you pick:
- add a unit test with a bare Ollama id going through `resolve` and `providers::route`, ending at
  `ollama/llama3.2:1b` with `models: {"llama3.2:1b": …}`;
- run a **typed turn** in your own instance on Ollama, and record `endReason: done` (the CDP recipe:
  `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=<port>`, then
  `__TAURI_INTERNALS__.invoke('session_write', …)` with the text, then `"\r"` as a separate write, and
  wait about 6 s after a session starts before writing, because the TUI drops input while it starts);
- correct your review: the in-app Ollama turn claimed under task 6 never ran in Chorus, only in the
  standalone probe.

Do this on `feature/p14` now. It doesn't depend on `handoff/oc2` and won't add conflicts with it,
unless the fix changes `check_model`'s call in `start_opencode`. If it does, keep §4's merged
signature in mind.
