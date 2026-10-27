//! opencode 2.x as a server plus a TUI connected to it (D42, as D58 updates it for 2.x; LESSONS L41).
//! Each run starts `opencode serve` on loopback, creates or reuses the session through its API
//! (paths under `/api/`, answers wrapped in `{"data": …}`) with the model and effort variant set on
//! the session itself, and runs `opencode --server <url> --session <id>` in the PTY. Permissions and
//! a provider route are the server's configuration (`OPENCODE_CONFIG_CONTENT`). What a turn really
//! ran with is its `session.step.started` event. Measured on opencode 2.1.1.

use std::collections::{BTreeMap, HashMap, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use specta::Type;

use crate::agent::{LaunchOptions, Spec};
use crate::events::AgentEvent;

/// The basic-auth user `opencode serve` expects when only a password is set.
pub const USERNAME: &str = "opencode";
/// Both the server and the TUI read the password from this variable, so it never needs argv.
pub const PASSWORD_VAR: &str = "OPENCODE_SERVER_PASSWORD";
pub const CONFIG_VAR: &str = "OPENCODE_CONFIG_CONTENT";

/// opencode 2.0 moved the API under `/api/` and replaced `attach` with `--server` (LESSONS L41), so
/// Chorus can't drive an older one at all.
pub const OLDEST_MAJOR: u32 = 2;

/// npm 12 runs no install scripts unless told to, and `@opencode/cli` copies its real binary in one,
/// so a plain `npm install -g` leaves a placeholder (LESSONS L67).
pub const INSTALL: &str = "npm install -g --allow-scripts=@opencode/cli @opencode/cli";

/// The placeholder npm leaves when the install script didn't run is a few hundred bytes; the real
/// 2.1.4 binary is about 247 MB.
const PLACEHOLDER_MAX_BYTES: u64 = 1024 * 1024;

/// Why the program behind the shim is only npm's placeholder, from its size or its `--version`
/// output; `None` when it looks real (LESSONS L67).
pub fn unfinished_install(program_bytes: Option<u64>, version_output: Option<&str>) -> Option<String> {
    let tiny = program_bytes.is_some_and(|n| n < PLACEHOLDER_MAX_BYTES);
    let says_so = version_output.is_some_and(|o| o.to_ascii_lowercase().contains("postinstall script was not run"));
    (tiny || says_so).then(|| format!("opencode's install didn't finish (its install script was blocked). Run {INSTALL}."))
}

/// Why the installed opencode can't be driven, naming the version Chorus checked (`verified`);
/// `None` for 2.x and later, and for a version that couldn't be read, which is tried and reported
/// (LESSONS L7).
pub fn too_old(found: Option<&str>, verified: Option<&str>) -> Option<String> {
    let found = found?;
    let version = crate::cli::parse_version(found)?;
    (version.major < OLDEST_MAJOR).then(|| {
        format!(
            "opencode {found} is too old: Chorus drives opencode {OLDEST_MAJOR}.x (checked {}), whose server and terminal UI work differently. Update it with {INSTALL}.",
            verified.unwrap_or("2.x")
        )
    })
}

/// Port 0 lets the system choose, so the port is read from the server's output.
pub fn serve_args() -> Vec<String> {
    ["serve", "--port", "0", "--hostname", "127.0.0.1"].map(str::to_owned).to_vec()
}

/// The TUI, connected to the run's own server and session (2.x has no `attach`).
pub fn tui_args(url: &str, session_id: &str) -> Vec<String> {
    ["--server", url, "--session", session_id].map(str::to_owned).to_vec()
}

/// A table entry from the opencode spec: a key and its JSON value.
fn entry(a: &[&str]) -> Option<(String, Value)> {
    Some(((*a.first()?).to_owned(), serde_json::from_str(a.get(1)?).ok()?))
}

/// The variant Chorus asks for, if any (`"high"`).
pub fn wanted_variant(spec: &Spec, o: &LaunchOptions) -> Option<String> {
    let (_, v) = entry(spec.effort_args(o.effort?)?)?;
    v.as_str().map(str::to_owned)
}

/// The configuration for options already passed through [`Spec::resolve`], or `None` when nothing
/// is set: the model (its variant travels on the session, [`create_session_body`]), the permission
/// rules or the `plan` agent, and a provider route's block (`providers::Route::opencode_provider`)
/// under `providers`; `o.model` is then the model as opencode names it.
pub fn config_content(spec: &Spec, o: &LaunchOptions, provider: Option<&Value>) -> Option<String> {
    let mut root = Map::new();
    if let Some(p) = provider {
        root.insert("providers".into(), p.clone());
    }
    if let Some(model) = &o.model {
        root.insert("model".into(), json!(model));
    }
    if let Some((key, value)) = o.permission.and_then(|p| spec.permission_args(p)).and_then(entry) {
        root.insert(key, value);
    }
    (!root.is_empty()).then(|| Value::Object(root).to_string())
}

/// What New session's summary shows for options already passed through [`Spec::resolve`]: the
/// configuration's entries and the variant the session is created with, each as `key=value`.
pub fn summary_entries(spec: &Spec, o: &LaunchOptions) -> Vec<String> {
    let config = config_content(spec, o, None).and_then(|c| serde_json::from_str::<Map<String, Value>>(&c).ok()).unwrap_or_default();
    let mut out: Vec<String> = config
        .into_iter()
        .map(|(key, value)| match value {
            Value::String(s) => format!("{key}={s}"),
            other => format!("{key}={other}"),
        })
        .collect();
    out.extend(wanted_variant(spec, o).map(|v| format!("variant={v}")));
    out
}

/// The server's address from its output (`server listening on http://127.0.0.1:4096`). Only a
/// loopback address is taken.
pub fn listening_url(output: &str) -> Option<String> {
    const MARK: &str = "listening on http://127.0.0.1:";
    let at = output.find(MARK)? + MARK.len();
    let port: String = output[at..].chars().take_while(char::is_ascii_digit).collect();
    let port: u16 = port.parse().ok().filter(|p| *p != 0)?;
    Some(format!("http://127.0.0.1:{port}"))
}

/// opencode's session ids: `ses_` and letters and digits.
pub fn is_session_id(s: &str) -> bool {
    s.len() <= 64 && s.strip_prefix("ses_").is_some_and(|r| !r.is_empty() && r.bytes().all(|b| b.is_ascii_alphanumeric()))
}

/// What an API answer holds (`{"data": …}`).
fn data(body: &str) -> Option<Value> {
    let mut v: Value = serde_json::from_str(body).ok()?;
    Some(v.get_mut("data")?.take())
}

/// How many items an API list answer holds; 0 when it isn't one. A new server answers its provider
/// list empty for a moment while it loads (LESSONS L41).
pub fn listed(body: &str) -> usize {
    data(body).and_then(|d| d.as_array().map(Vec::len)).unwrap_or(0)
}

/// Whether a provider list answer holds `provider`. A provider from the configuration (Chorus's
/// Ollama block) can still be missing while the built-in ones are listed.
pub fn lists_provider(body: &str, provider: &str) -> bool {
    data(body)
        .and_then(|d| d.as_array().map(|a| a.iter().any(|p| p.get("id").and_then(Value::as_str) == Some(provider))))
        .unwrap_or(false)
}

/// A session's model as the API names it, `{providerID, id, variant}`, from `provider/model`.
fn model_ref(model: &str, variant: Option<&str>) -> Option<Value> {
    let (provider, id) = model.split_once('/')?;
    let mut m = json!({ "providerID": provider, "id": id });
    if let Some(v) = variant {
        m["variant"] = json!(v);
    }
    Some(m)
}

/// `POST /api/session`'s body: the title, the folder, and the model with its effort variant. The
/// variant only takes effect on the session (a `#variant` in the configuration's model doesn't).
pub fn create_session_body(title: &str, cwd: &str, model: Option<&str>, variant: Option<&str>) -> Value {
    let mut body = json!({ "title": title, "location": { "directory": cwd } });
    if let Some(m) = model.and_then(|m| model_ref(m, variant)) {
        body["model"] = m;
    }
    body
}

/// `POST /api/session/{id}/model`'s body. A session keeps the model and variant its last prompt
/// ran with, one picked in the TUI included, so a resumed run sets the session's own again. (2.x's
/// TUI keeps no `model.json`; the one LESSONS L5 and L35 describe is 1.x's.)
pub fn set_model_body(model: &str, variant: Option<&str>) -> Option<Value> {
    Some(json!({ "model": model_ref(model, variant)? }))
}

/// The session id from `POST /api/session`'s answer.
pub fn created_session_id(body: &str) -> Option<String> {
    data(body)?.get("id")?.as_str().filter(|id| is_session_id(id)).map(str::to_owned)
}

/// What the server says about a `provider/model`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelCheck {
    Known { variants: Vec<String> },
    NoSuchProvider,
    /// The provider is switched off, so its models won't run.
    NotConnected,
    /// The provider needs a key or a login and has neither.
    NoKey,
    NoSuchModel,
}

/// The integration a provider signs in through, from `GET /api/integration`'s list.
fn integration<'a>(integrations: &'a Value, id: &str) -> Option<&'a Value> {
    integrations.as_array()?.iter().find(|i| i.get("id").and_then(Value::as_str) == Some(id))
}

/// Why the key check was skipped, from `GET /api/integration`'s answer: `None` when the answer is
/// the list Chorus reads. The check then makes no claim about keys (oc2 review 12.2), and the
/// session says so instead of saying nothing.
pub fn key_check_skipped(answer: &Result<(u16, String), String>) -> Option<String> {
    let why = match answer {
        Err(e) => e.clone(),
        Ok((200, body)) if data(body).is_some_and(|d| d.is_array()) => return None,
        Ok((200, _)) => "its answer isn't in the shape Chorus checked".to_owned(),
        Ok((status, _)) => format!("opencode answered {status}"),
    };
    Some(format!("Chorus couldn't check opencode's keys ({why}), so a missing key shows only when the first turn fails."))
}

/// Whether an integration has a key or a login: one connection is enough, and a key in the
/// server's environment counts as one (`{"type": "env", "name": "OPENROUTER_API_KEY"}`).
fn connected(integration: &Value) -> Option<bool> {
    Some(!integration.get("connections")?.as_array()?.is_empty())
}

/// From `GET /api/provider`, `GET /api/model` and `GET /api/integration`; `None` when the provider
/// or model list isn't the shape 2.1.1 gives. An integration list that isn't is passed over, so
/// the check never claims a key is missing without seeing it.
///
/// 2.1.1 lists a provider only once it can run: without a key, `anthropic` is in neither list but
/// is an integration. A listed provider with a key in its own settings (opencode's free tier has
/// `"public"`) or with no integration (Ollama) needs nothing more.
///
/// `keyless` is a route whose provider needs no key, such as Ollama on this machine: opencode marks
/// that provider disabled for want of a login, which is not the same as the model not running, so
/// neither "switched off" nor "no key" applies to it.
pub fn check_model(providers_body: &str, models_body: &str, integrations_body: &str, model: &str, keyless: bool) -> Option<ModelCheck> {
    let (provider, id) = model.split_once('/')?;
    let providers = data(providers_body)?;
    let integrations = data(integrations_body).unwrap_or(Value::Null);
    let Some(p) = providers.as_array()?.iter().find(|p| p.get("id").and_then(Value::as_str) == Some(provider)) else {
        return Some(match integration(&integrations, provider).and_then(connected) {
            Some(false) if !keyless => ModelCheck::NoKey,
            _ => ModelCheck::NoSuchProvider,
        });
    };
    if !keyless && p.get("activation").and_then(Value::as_str) == Some("disabled") {
        return Some(ModelCheck::NotConnected);
    }
    let own_key = p.pointer("/settings/apiKey").and_then(Value::as_str).is_some_and(|k| !k.is_empty());
    let signs_in = p.get("integrationID").and_then(Value::as_str).and_then(|i| integration(&integrations, i));
    if !keyless && !own_key && signs_in.and_then(connected) == Some(false) {
        return Some(ModelCheck::NoKey);
    }
    let models = data(models_body)?;
    let Some(m) = models
        .as_array()?
        .iter()
        .find(|m| m.get("providerID").and_then(Value::as_str) == Some(provider) && m.get("id").and_then(Value::as_str) == Some(id))
    else {
        return Some(ModelCheck::NoSuchModel);
    };
    let mut variants: Vec<String> = m
        .get("variants")
        .and_then(Value::as_array)
        .map(|vs| vs.iter().filter_map(|v| v.get("id").and_then(Value::as_str).map(str::to_owned)).collect())
        .unwrap_or_default();
    // Lowest effort first; names off the ladder after it.
    variants.sort_by_key(|v| (LADDER.iter().position(|l| l == v).unwrap_or(LADDER.len()), v.clone()));
    Some(ModelCheck::Known { variants })
}

/// The variant names seen across opencode's catalog, lowest effort first.
const LADDER: [&str; 7] = ["none", "minimal", "low", "medium", "high", "xhigh", "max"];

/// Why a run's model or effort won't apply, for the session's note.
pub fn model_note(model: &str, wanted: Option<&str>, check: &ModelCheck) -> Option<String> {
    let (provider, id) = model.split_once('/').unwrap_or((model, ""));
    match check {
        ModelCheck::NoSuchProvider => Some(format!("opencode doesn't know the provider {provider:?}, so {model} won't run.")),
        ModelCheck::NotConnected => Some(format!("opencode's {provider} provider is switched off, so {model} won't run.")),
        // A session's opencode keeps its data in the session's own folder, so a login made in the
        // user's own opencode isn't there.
        ModelCheck::NoKey => {
            Some(format!("opencode has no key or login for {provider} in this session, so {model} won't run. Run it on a provider from Settings."))
        }
        ModelCheck::NoSuchModel => Some(format!("opencode's {provider} provider has no model {id:?}.")),
        ModelCheck::Known { variants } => {
            let wanted = wanted?;
            if variants.iter().any(|v| v == wanted) {
                None
            } else if variants.is_empty() {
                Some(format!("{model} has no effort settings, so opencode ignores the effort."))
            } else {
                Some(format!("{model} has no {wanted} effort (it has {}), so opencode uses its default.", variants.join(", ")))
            }
        }
    }
}

/// What a turn really ran with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Observed {
    /// `provider/model`.
    pub model: String,
    pub variant: Option<String>,
    pub agent: Option<String>,
}

/// A turn's model, variant and agent, from its `session.step.started` event in `session_id`.
pub fn observed_from_event(event: &Value, session_id: &str) -> Option<Observed> {
    if event.get("type").and_then(Value::as_str) != Some("session.step.started") {
        return None;
    }
    let d = event.get("data")?;
    if d.get("sessionID").and_then(Value::as_str) != Some(session_id) {
        return None;
    }
    let text = |o: &Value, k: &str| o.get(k).and_then(Value::as_str).map(str::to_owned);
    let model = d.get("model")?;
    Some(Observed {
        model: format!("{}/{}", text(model, "providerID")?, text(model, "id")?),
        variant: text(model, "variant"),
        agent: text(d, "agent"),
    })
}

/// Turns opencode 2.1.1's server events (`GET /api/event`, LESSONS L41) into [`AgentEvent`]s for one
/// session. A turn is a user message: it starts when the inbox delivers it and ends when the
/// session's execution succeeds, fails or is interrupted. A prompt sent while a turn runs joins the
/// same execution at its next step (opencode "steers" it), so each prompt of an execution ends with
/// it, oldest first. A turn's reply is the text of its newest assistant step that had any.
#[derive(Debug, Default)]
pub struct EventParser {
    session_id: String,
    /// Inbox id → the prompt's text, until it's delivered.
    prompts: HashMap<String, String>,
    /// The execution's turns, oldest first, each with its assistant steps in order. Steps seen
    /// before any delivery (the stream joined mid-turn) belong to a turn with no id.
    turns: Vec<(Option<String>, Vec<String>)>,
    /// Each step's texts by ordinal.
    texts: HashMap<String, BTreeMap<u64, String>>,
    /// Tool call id → its tool and target, named before the call runs.
    tools: HashMap<String, (String, Option<String>)>,
    started: HashSet<String>,
}

impl EventParser {
    pub fn new(session_id: &str) -> Self {
        Self { session_id: session_id.to_owned(), ..Default::default() }
    }

    fn reply(&self, steps: &[String]) -> Option<String> {
        steps.iter().rev().find_map(|step| {
            let joined = self.texts.get(step)?.values().map(String::as_str).collect::<Vec<_>>().join("\n");
            (!joined.trim().is_empty()).then_some(joined)
        })
    }

    /// The execution ended: one event per turn it held, oldest first (or one for no known turn),
    /// each with its own reply. A turn with no id ends after the named ones, because an end with no
    /// id closes the oldest open turn, which would be a named one.
    fn end_turns(&mut self, end: impl Fn(Option<String>, Option<String>) -> AgentEvent) -> Vec<AgentEvent> {
        let mut turns = std::mem::take(&mut self.turns);
        // Only the first can have no id: it holds the steps seen before any delivery.
        if turns.len() > 1 && turns[0].0.is_none() {
            turns.rotate_left(1);
        }
        let out = if turns.is_empty() {
            vec![end(None, None)]
        } else {
            turns.into_iter().map(|(turn, steps)| end(turn, self.reply(&steps))).collect()
        };
        self.texts.clear();
        out
    }

    /// One event from the stream, as the events it means.
    pub fn push(&mut self, event: &Value) -> Vec<AgentEvent> {
        let kind = event.get("type").and_then(Value::as_str).unwrap_or_default();
        let d = event.get("data").cloned().unwrap_or(Value::Null);
        let text = |o: &Value, k: &str| o.get(k).and_then(Value::as_str).map(str::to_owned);
        let session = text(&d, "sessionID").or_else(|| d.pointer("/form/sessionID").and_then(Value::as_str).map(str::to_owned));
        if session.as_deref() != Some(self.session_id.as_str()) {
            return Vec::new();
        }
        match kind {
            "session.inbox.enqueued" => {
                if let (Some(id), Some("user")) = (text(&d, "inboxID"), d.pointer("/item/type").and_then(Value::as_str)) {
                    let prompt = d.pointer("/item/payload/text").and_then(Value::as_str).unwrap_or_default();
                    self.prompts.insert(id, prompt.to_owned());
                }
                Vec::new()
            }
            "session.inbox.delivered" => {
                let Some(id) = text(&d, "inboxID") else { return Vec::new() };
                let prompt = self.prompts.remove(&id).filter(|p| !p.trim().is_empty());
                self.turns.push((Some(id.clone()), Vec::new()));
                vec![AgentEvent::TurnStarted { turn: Some(id), prompt }]
            }
            "session.step.started" => {
                if let Some(step) = text(&d, "assistantMessageID") {
                    if self.turns.is_empty() {
                        self.turns.push((None, Vec::new()));
                    }
                    let steps = &mut self.turns.last_mut().expect("a turn").1;
                    // A step that retries (`session.retry.scheduled`) starts again under the same id.
                    if steps.last() != Some(&step) {
                        steps.push(step);
                    }
                }
                Vec::new()
            }
            "session.text.ended" => {
                if let (Some(step), Some(body)) = (text(&d, "assistantMessageID"), text(&d, "text")) {
                    let ordinal = d.get("ordinal").and_then(Value::as_u64).unwrap_or(0);
                    self.texts.entry(step).or_default().insert(ordinal, body);
                }
                Vec::new()
            }
            "session.tool.input.started" => {
                if let (Some(id), Some(name)) = (text(&d, "id"), text(&d, "name")) {
                    self.tools.insert(id, (name, None));
                }
                Vec::new()
            }
            "session.tool.called" => {
                let Some(call) = text(&d, "id") else { return Vec::new() };
                if !self.started.insert(call.clone()) {
                    return Vec::new();
                }
                let target = d.get("input").and_then(crate::events::tool_target);
                let slot = self.tools.entry(call.clone()).or_insert_with(|| ("tool".to_owned(), None));
                slot.1.clone_from(&target);
                vec![AgentEvent::ToolStarted { turn: None, call: Some(call), tool: slot.0.clone(), target }]
            }
            "session.tool.success" | "session.tool.failed" => {
                let call = text(&d, "id");
                let (tool, target) = call.as_ref().and_then(|c| self.tools.remove(c)).unwrap_or_else(|| ("tool".to_owned(), None));
                vec![AgentEvent::ToolFinished { turn: None, call, tool, target, failed: kind == "session.tool.failed" }]
            }
            "session.execution.succeeded" => self.end_turns(|turn, message| AgentEvent::TurnEnded { turn, message }),
            "session.execution.failed" => {
                let error = d
                    .pointer("/error/message")
                    .or_else(|| d.get("message"))
                    .and_then(Value::as_str)
                    .unwrap_or("the turn failed")
                    .to_owned();
                self.end_turns(|turn, _| AgentEvent::TurnFailed { turn, error: error.clone() })
            }
            "session.execution.interrupted" => self.end_turns(|turn, _| AgentEvent::TurnInterrupted { turn }),
            "permission.asked" => vec![AgentEvent::ApprovalAsked {
                turn: None,
                ask: text(&d, "id"),
                tool: text(&d, "action").unwrap_or_default(),
                target: d.pointer("/resources/0").and_then(Value::as_str).map(str::to_owned),
            }],
            "permission.replied" => vec![AgentEvent::ApprovalAnswered {
                ask: text(&d, "requestID"),
                allowed: d.get("reply").and_then(Value::as_str) != Some("reject"),
            }],
            "permission.rejected" => vec![AgentEvent::ApprovalAnswered { ask: text(&d, "requestID").or_else(|| text(&d, "id")), allowed: false }],
            "form.created" => {
                let form = d.get("form").cloned().unwrap_or(Value::Null);
                let question = form
                    .pointer("/fields/0/description")
                    .or_else(|| form.pointer("/fields/0/title"))
                    .or_else(|| form.get("title"))
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned();
                vec![AgentEvent::QuestionAsked { turn: None, ask: text(&form, "id"), question }]
            }
            "form.replied" => {
                let answer = d.get("answer").and_then(Value::as_object).and_then(|a| a.values().next()).map(|v| match v {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                });
                vec![AgentEvent::QuestionAnswered { ask: text(&d, "id"), answer }]
            }
            "form.cancelled" => vec![AgentEvent::QuestionAnswered { ask: text(&d, "id"), answer: None }],
            "session.error" => vec![AgentEvent::TurnFailed {
                turn: None,
                error: d.pointer("/error/message").or_else(|| d.get("message")).and_then(Value::as_str).unwrap_or("the turn failed").to_owned(),
            }],
            _ => Vec::new(),
        }
    }
}

/// Splits a server-sent-events buffer into complete `data:` payloads, leaving any partial event
/// in `buffer`. Comment blocks (`: heartbeat`) carry no payload and are dropped.
pub fn take_sse_events(buffer: &mut String) -> Vec<Value> {
    let mut out = Vec::new();
    while let Some(end) = buffer.find("\n\n") {
        let block: String = buffer.drain(..end + 2).collect();
        let data: String = block
            .lines()
            .filter_map(|l| l.strip_prefix("data:"))
            .map(|d| d.strip_prefix(' ').unwrap_or(d))
            .collect::<Vec<_>>()
            .join("\n");
        if let Ok(v) = serde_json::from_str(&data) {
            out.push(v);
        }
    }
    out
}

/// How a turn differs from what the session asked for, one phrase each; empty when it matches.
/// An agent other than `plan` is only a difference when Plan was asked for: switching agents in
/// the TUI is the user's own choice.
pub fn differences(spec: &Spec, o: &LaunchOptions, seen: &Observed) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(model) = o.model.as_deref().filter(|m| *m != seen.model) {
        out.push(format!("ran {}, not {model}", seen.model));
    }
    if let Some(wanted) = wanted_variant(spec, o).filter(|w| seen.variant.as_deref() != Some(w.as_str())) {
        out.push(format!("ran with effort {}, not {wanted}", seen.variant.as_deref().unwrap_or("default")));
    }
    if o.permission == Some(crate::agent::Permission::Plan) && seen.agent.as_deref() != Some("plan") {
        out.push(format!("ran the {} agent, not plan", seen.agent.as_deref().unwrap_or("default")));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{AgentKind, Effort, Permission};

    #[test]
    fn a_turn_is_compared_with_what_was_asked() {
        let o = opts(Some(Effort::Deep), Some(Permission::Plan), Some("opencode/glm-5.3-flash-free"));
        let seen = |model: &str, variant: Option<&str>, agent: &str| Observed {
            model: model.into(),
            variant: variant.map(str::to_owned),
            agent: Some(agent.into()),
        };
        assert!(differences(spec(), &o, &seen("opencode/glm-5.3-flash-free", Some("high"), "plan")).is_empty());
        assert_eq!(
            differences(spec(), &o, &seen("opencode/big-pickle", None, "build")),
            [
                "ran opencode/big-pickle, not opencode/glm-5.3-flash-free",
                "ran with effort default, not high",
                "ran the build agent, not plan"
            ]
        );
    }

    fn spec() -> &'static Spec {
        AgentKind::Opencode.spec()
    }

    fn opts(effort: Option<Effort>, permission: Option<Permission>, model: Option<&str>) -> LaunchOptions {
        spec().resolve(&LaunchOptions { effort, permission, model: model.map(str::to_owned) }).unwrap()
    }

    #[test]
    fn the_effort_travels_on_the_session_and_the_plan_agent_in_the_configuration() {
        let o = opts(Some(Effort::Deep), Some(Permission::Plan), Some("opencode/glm-5.3-flash-free"));
        let c: Value = serde_json::from_str(&config_content(spec(), &o, None).unwrap()).unwrap();
        assert_eq!(c, json!({ "model": "opencode/glm-5.3-flash-free", "default_agent": "plan" }));
        assert_eq!(wanted_variant(spec(), &o).as_deref(), Some("high"));
        let body = create_session_body("Nell", r"C:\w", o.model.as_deref(), Some("high"));
        assert_eq!(
            body,
            json!({ "title": "Nell", "location": { "directory": r"C:\w" }, "model": { "providerID": "opencode", "id": "glm-5.3-flash-free", "variant": "high" } })
        );
        // OpenRouter's model ids keep their own slash.
        let routed = create_session_body("x", r"C:\w", Some("openrouter/deepseek/deepseek-v4.1-flash:nitro"), None);
        assert_eq!(routed["model"], json!({ "providerID": "openrouter", "id": "deepseek/deepseek-v4.1-flash:nitro" }));
        assert!(create_session_body("x", r"C:\w", None, Some("high")).get("model").is_none());
    }

    #[test]
    fn each_permission_becomes_its_rules() {
        let c = |p| serde_json::from_str::<Value>(&config_content(spec(), &opts(None, Some(p), None), None).unwrap()).unwrap();
        assert_eq!(c(Permission::FullAccess), json!({ "permissions": [{ "action": "*", "resource": "*", "effect": "allow" }] }));
        let effect = |p, action: &str| {
            c(p)["permissions"].as_array().unwrap().iter().find(|r| r["action"] == action).map(|r| r["effect"].as_str().unwrap().to_owned())
        };
        assert_eq!(effect(Permission::Manual, "edit").as_deref(), Some("ask"));
        assert_eq!(effect(Permission::Manual, "shell").as_deref(), Some("ask"));
        assert_eq!(effect(Permission::AcceptEdits, "edit").as_deref(), Some("allow"));
        assert_eq!(effect(Permission::AcceptEdits, "shell").as_deref(), Some("ask"));
        assert_eq!(config_content(spec(), &opts(None, None, None), None), None);
        // A route's provider block goes in under `providers`.
        let routed = config_content(spec(), &opts(None, None, Some("openrouter/x/y")), Some(&json!({ "openrouter": {} }))).unwrap();
        assert_eq!(serde_json::from_str::<Value>(&routed).unwrap(), json!({ "providers": { "openrouter": {} }, "model": "openrouter/x/y" }));
    }

    #[test]
    fn reads_the_listening_line() {
        assert_eq!(listening_url("server listening on http://127.0.0.1:4096\n").as_deref(), Some("http://127.0.0.1:4096"));
        assert_eq!(listening_url("noise\r\nserver listening on http://127.0.0.1:51234").as_deref(), Some("http://127.0.0.1:51234"));
        assert_eq!(listening_url("server listening on http://0.0.0.0:4096"), None);
        assert_eq!(listening_url("listening on http://127.0.0.1:0"), None);
        assert_eq!(listening_url("listening on http://127.0.0.1:99999"), None);
    }

    #[test]
    fn session_ids_are_checked() {
        assert!(is_session_id("ses_ea5c225b8ffeNh5sBFcKZ9hIn7"));
        for bad in ["", "ses_", "ses_a b", "sess_x", "ses_x/../y", "--session"] {
            assert!(!is_session_id(bad), "{bad}");
        }
        assert_eq!(created_session_id(r#"{"data":{"id":"ses_abc1","title":"x"}}"#).as_deref(), Some("ses_abc1"));
        assert_eq!(created_session_id(r#"{"id":"ses_abc1"}"#), None, "1.x's bare answer isn't 2.x's");
        assert_eq!(created_session_id(r#"{"data":{"id":"--fork"}}"#), None);
    }

    // The shapes 2.1.1 answers with in a fresh data folder with no keys (trimmed). The `disabled`
    // provider is made up: no provider here was switched off.
    const PROVIDERS: &str = r#"{"location":{"directory":"C:\\w"},"data":[
        {"id":"openrouter","integrationID":"openrouter","name":"OpenRouter","activation":"enabled","settings":{"baseURL":"https://openrouter.ai/api/v1"}},
        {"id":"opencode","integrationID":"opencode","name":"OpenCode","activation":"enabled","settings":{"apiKey":"public","baseURL":"https://opencode.ai/inference/openai/v1"}},
        {"id":"ollama","name":"Ollama","activation":"enabled","settings":{"apiKey":"","baseURL":"http://127.0.0.1:11434/v1","provider":"ollama"}},
        {"id":"switched","name":"Off","activation":"disabled"}]}"#;
    const MODELS: &str = r#"{"location":{"directory":"C:\\w"},"data":[
        {"id":"deepseek-v4.1-flash-free","providerID":"opencode","variants":[{"id":"max"},{"id":"low"},{"id":"high"}]},
        {"id":"big-pickle","providerID":"opencode","variants":[]},
        {"id":"deepseek/deepseek-v4.1-flash","providerID":"openrouter","variants":[{"id":"high"}]},
        {"id":"qwen3","providerID":"ollama","variants":[]}]}"#;
    const NO_KEYS: &str = r#"{"data":[
        {"id":"anthropic","name":"Anthropic","methods":[{"type":"key"},{"type":"env","names":["ANTHROPIC_API_KEY"]}],"connections":[]},
        {"id":"openrouter","name":"OpenRouter","methods":[{"type":"key"},{"type":"env","names":["OPENROUTER_API_KEY"]}],"connections":[]},
        {"id":"opencode","name":"OpenCode Console","methods":[{"type":"key"},{"type":"env","names":["OPENCODE_API_KEY"]}],"connections":[]}]}"#;
    /// The same with `OPENROUTER_API_KEY` in the server's environment.
    const ENV_KEY: &str = r#"{"data":[
        {"id":"openrouter","name":"OpenRouter","methods":[{"type":"key"},{"type":"env","names":["OPENROUTER_API_KEY"]}],"connections":[{"type":"env","name":"OPENROUTER_API_KEY"}]}]}"#;

    #[test]
    fn a_list_answer_is_counted() {
        assert_eq!(listed(PROVIDERS), 4);
        assert_eq!(listed(r#"{"location":{},"data":[]}"#), 0);
        assert_eq!(listed("[]"), 0);
        assert!(lists_provider(PROVIDERS, "ollama"));
        assert!(!lists_provider(PROVIDERS, "anthropic"));
        assert!(!lists_provider("[]", "ollama"));
    }

    #[test]
    fn the_model_is_checked_against_the_server() {
        let known = check_model(PROVIDERS, MODELS, NO_KEYS, "opencode/deepseek-v4.1-flash-free", false).unwrap();
        assert_eq!(known, ModelCheck::Known { variants: vec!["low".into(), "high".into(), "max".into()] });
        assert_eq!(model_note("opencode/deepseek-v4.1-flash-free", Some("high"), &known), None);
        assert_eq!(
            model_note("opencode/deepseek-v4.1-flash-free", Some("medium"), &known).unwrap(),
            "opencode/deepseek-v4.1-flash-free has no medium effort (it has low, high, max), so opencode uses its default."
        );
        let none = check_model(PROVIDERS, MODELS, NO_KEYS, "opencode/big-pickle", false).unwrap();
        assert!(model_note("opencode/big-pickle", Some("low"), &none).unwrap().contains("no effort settings"));
        assert_eq!(check_model(PROVIDERS, MODELS, NO_KEYS, "switched/x", false), Some(ModelCheck::NotConnected));
        assert_eq!(check_model(PROVIDERS, MODELS, NO_KEYS, "nobody/x", false), Some(ModelCheck::NoSuchProvider));
        assert_eq!(check_model(PROVIDERS, MODELS, NO_KEYS, "opencode/nope", false), Some(ModelCheck::NoSuchModel));
        assert_eq!(check_model("[]", MODELS, NO_KEYS, "opencode/x", false), None);
        // A keyless provider (Ollama) runs even though opencode shows it disabled for want of a
        // login: that is what a local provider looks like.
        const WITH_OLLAMA: &str = r#"{"location":{},"data":[{"id":"ollama","name":"Ollama (local)","activation":"disabled","package":"x"}]}"#;
        const OLLAMA_MODELS: &str = r#"{"location":{},"data":[{"id":"llama3.2:1b","providerID":"ollama","variants":[]}]}"#;
        assert_eq!(check_model(WITH_OLLAMA, OLLAMA_MODELS, NO_KEYS, "ollama/llama3.2:1b", false), Some(ModelCheck::NotConnected));
        let known = check_model(WITH_OLLAMA, OLLAMA_MODELS, NO_KEYS, "ollama/llama3.2:1b", true).unwrap();
        assert_eq!(known, ModelCheck::Known { variants: vec![] });
        assert_eq!(model_note("ollama/llama3.2:1b", None, &known), None);
    }

    #[test]
    fn a_provider_without_a_key_is_named() {
        // Listed, but nothing to sign in with: it fails its first turn with a 401.
        let routed = "openrouter/deepseek/deepseek-v4.1-flash";
        assert_eq!(check_model(PROVIDERS, MODELS, NO_KEYS, routed, false), Some(ModelCheck::NoKey));
        assert_eq!(
            model_note(routed, None, &ModelCheck::NoKey).unwrap(),
            "opencode has no key or login for openrouter in this session, so openrouter/deepseek/deepseek-v4.1-flash won't run. Run it on a provider from Settings."
        );
        // A keyless route is never "no key", whatever its integration says, listed or not. Seen at
        // start-up: a relaunch's check ran before opencode had loaded Chorus's Ollama block.
        assert!(matches!(check_model(PROVIDERS, MODELS, NO_KEYS, routed, true), Some(ModelCheck::Known { .. })));
        let loading = r#"{"location":{},"data":[{"id":"opencode","activation":"enabled","settings":{"apiKey":"public"}}]}"#;
        let ollama = r#"{"data":[{"id":"ollama","name":"Ollama","methods":[],"connections":[]}]}"#;
        assert_eq!(check_model(loading, MODELS, ollama, "ollama/qwen3", false), Some(ModelCheck::NoKey));
        assert_eq!(check_model(loading, MODELS, ollama, "ollama/qwen3", true), Some(ModelCheck::NoSuchProvider));
        // A key in the environment is a connection.
        assert!(matches!(check_model(PROVIDERS, MODELS, ENV_KEY, routed, false), Some(ModelCheck::Known { .. })));
        // Not listed at all, but opencode has it as an integration: it's the key that's missing.
        assert_eq!(check_model(PROVIDERS, MODELS, NO_KEYS, "anthropic/claude-sonnet-4-6", false), Some(ModelCheck::NoKey));
        // opencode's free tier has its own key, and Ollama signs in with nothing.
        assert!(matches!(check_model(PROVIDERS, MODELS, NO_KEYS, "opencode/big-pickle", false), Some(ModelCheck::Known { .. })));
        assert!(matches!(check_model(PROVIDERS, MODELS, NO_KEYS, "ollama/qwen3", false), Some(ModelCheck::Known { .. })));
        // No integration list to go on: no claim about keys.
        assert!(matches!(check_model(PROVIDERS, MODELS, "<html>", routed, false), Some(ModelCheck::Known { .. })));
        assert_eq!(check_model(PROVIDERS, MODELS, "<html>", "anthropic/x", false), Some(ModelCheck::NoSuchProvider));
        // Nor is an integration without a connections list.
        let unknown = r#"{"data":[{"id":"openrouter","name":"OpenRouter","methods":[{"type":"key"}]}]}"#;
        assert!(matches!(check_model(PROVIDERS, MODELS, unknown, routed, false), Some(ModelCheck::Known { .. })));
        // Not listed though its integration has a key: opencode doesn't know it.
        let signed_in = r#"{"data":[{"id":"anthropic","name":"Anthropic","methods":[{"type":"key"}],"connections":[{"type":"env","name":"ANTHROPIC_API_KEY"}]}]}"#;
        assert_eq!(check_model(PROVIDERS, MODELS, signed_in, "anthropic/x", false), Some(ModelCheck::NoSuchProvider));
    }

    #[test]
    fn a_resumed_session_is_given_its_model_again() {
        assert_eq!(
            set_model_body("opencode/deepseek-v4.1-flash-free", Some("high")),
            Some(json!({ "model": { "providerID": "opencode", "id": "deepseek-v4.1-flash-free", "variant": "high" } }))
        );
        assert_eq!(
            set_model_body("openrouter/deepseek/deepseek-v4.1-flash", None),
            Some(json!({ "model": { "providerID": "openrouter", "id": "deepseek/deepseek-v4.1-flash" } }))
        );
        assert_eq!(set_model_body("no-provider", Some("high")), None);
    }

    /// One event as the stream sends it: `{ id, type, data }`.
    fn ev(kind: &str, data: Value) -> Value {
        json!({ "id": "evt_1", "type": kind, "data": data })
    }

    #[test]
    fn server_events_become_a_turn() {
        // The sequence measured on 2.1.1 with a question, an approval and a shell call (trimmed).
        let s = "ses_1";
        let mut p = EventParser::new(s);
        let mut all = Vec::new();
        for e in [
            ev("server.connected", json!({})),
            ev("session.inbox.enqueued", json!({"inboxID":"msg_u","sessionID":s,"item":{"type":"user","payload":{"text":"make hello.txt"},"delivery":"steer"}})),
            ev("session.execution.started", json!({"sessionID":s})),
            ev("session.inbox.delivered", json!({"sessionID":s,"inboxID":"msg_u"})),
            ev("session.step.started", json!({"sessionID":s,"agent":"build","model":{"id":"big-pickle","providerID":"opencode","variant":"default"},"assistantMessageID":"msg_a"})),
            ev("session.tool.input.started", json!({"sessionID":s,"assistantMessageID":"msg_a","id":"c1","name":"shell"})),
            ev("session.tool.called", json!({"sessionID":s,"assistantMessageID":"msg_a","id":"c1","input":{"command":"type hello.txt"},"executed":false})),
            ev("permission.asked", json!({"id":"per_1","sessionID":s,"action":"shell","resources":["type hello.txt"],"save":["type *"]})),
            ev("permission.replied", json!({"sessionID":s,"requestID":"per_1","reply":"once"})),
            ev("session.tool.success", json!({"sessionID":s,"assistantMessageID":"msg_a","id":"c1","content":[],"executed":false})),
            ev("form.created", json!({"form":{"id":"frm_1","sessionID":s,"title":"Questions","fields":[{"key":"q0","title":"Colour","description":"Which colour?"}]}})),
            ev("form.replied", json!({"id":"frm_1","sessionID":s,"answer":{"q0":"Red"}})),
            ev("session.step.ended", json!({"sessionID":s,"assistantMessageID":"msg_a","finish":"tool-calls"})),
            ev("session.step.started", json!({"sessionID":s,"agent":"build","model":{"id":"big-pickle","providerID":"opencode"},"assistantMessageID":"msg_b"})),
            ev("session.text.ended", json!({"sessionID":s,"assistantMessageID":"msg_b","ordinal":0,"text":"Created hello.txt."})),
            ev("session.execution.succeeded", json!({"sessionID":s})),
            ev("session.execution.succeeded", json!({"sessionID":"other"})),
        ] {
            all.extend(p.push(&e));
        }
        assert_eq!(
            all,
            [
                AgentEvent::TurnStarted { turn: Some("msg_u".into()), prompt: Some("make hello.txt".into()) },
                AgentEvent::ToolStarted { turn: None, call: Some("c1".into()), tool: "shell".into(), target: Some("type hello.txt".into()) },
                AgentEvent::ApprovalAsked { turn: None, ask: Some("per_1".into()), tool: "shell".into(), target: Some("type hello.txt".into()) },
                AgentEvent::ApprovalAnswered { ask: Some("per_1".into()), allowed: true },
                AgentEvent::ToolFinished { turn: None, call: Some("c1".into()), tool: "shell".into(), target: Some("type hello.txt".into()), failed: false },
                AgentEvent::QuestionAsked { turn: None, ask: Some("frm_1".into()), question: "Which colour?".into() },
                AgentEvent::QuestionAnswered { ask: Some("frm_1".into()), answer: Some("Red".into()) },
                AgentEvent::TurnEnded { turn: Some("msg_u".into()), message: Some("Created hello.txt.".into()) },
            ]
        );
        // An interrupted execution interrupts the turn; a failed one fails it; a rejection denies.
        p.push(&ev("session.inbox.delivered", json!({"sessionID":s,"inboxID":"msg_v"})));
        assert_eq!(p.push(&ev("session.execution.interrupted", json!({"sessionID":s}))), [AgentEvent::TurnInterrupted { turn: Some("msg_v".into()) }]);
        p.push(&ev("session.inbox.delivered", json!({"sessionID":s,"inboxID":"msg_w"})));
        assert_eq!(
            p.push(&ev("session.execution.failed", json!({"sessionID":s,"error":{"message":"rate limited"}}))),
            [AgentEvent::TurnFailed { turn: Some("msg_w".into()), error: "rate limited".into() }]
        );
        assert_eq!(
            p.push(&ev("permission.replied", json!({"sessionID":s,"requestID":"per_2","reply":"reject"}))),
            [AgentEvent::ApprovalAnswered { ask: Some("per_2".into()), allowed: false }]
        );
    }

    #[test]
    fn a_prompt_sent_during_a_turn_ends_with_its_execution() {
        // Measured on 2.1.1: the second prompt is delivered after the first one's step, and one
        // execution answers both. Neither was interrupted.
        let s = "ses_1";
        let mut p = EventParser::new(s);
        let mut all = Vec::new();
        for e in [
            ev("session.inbox.enqueued", json!({"inboxID":"msg_1","sessionID":s,"item":{"type":"user","payload":{"text":"Write a story."},"delivery":"steer"}})),
            ev("session.execution.started", json!({"sessionID":s})),
            ev("session.inbox.delivered", json!({"sessionID":s,"inboxID":"msg_1"})),
            ev("session.step.started", json!({"sessionID":s,"assistantMessageID":"msg_a"})),
            ev("session.inbox.enqueued", json!({"inboxID":"msg_2","sessionID":s,"item":{"type":"user","payload":{"text":"End it with FIN."},"delivery":"steer"}})),
            // A retried step starts again under the same id.
            ev("session.step.started", json!({"sessionID":s,"assistantMessageID":"msg_a"})),
            ev("session.text.ended", json!({"sessionID":s,"assistantMessageID":"msg_a","ordinal":0,"text":"Once upon a time."})),
            ev("session.step.ended", json!({"sessionID":s,"assistantMessageID":"msg_a","finish":"stop"})),
            ev("session.inbox.delivered", json!({"sessionID":s,"inboxID":"msg_2"})),
            ev("session.step.started", json!({"sessionID":s,"assistantMessageID":"msg_b"})),
            ev("session.text.ended", json!({"sessionID":s,"assistantMessageID":"msg_b","ordinal":0,"text":"Once upon a time.\n\nFIN"})),
            ev("session.execution.succeeded", json!({"sessionID":s})),
        ] {
            all.extend(p.push(&e));
        }
        assert_eq!(
            all,
            [
                AgentEvent::TurnStarted { turn: Some("msg_1".into()), prompt: Some("Write a story.".into()) },
                AgentEvent::TurnStarted { turn: Some("msg_2".into()), prompt: Some("End it with FIN.".into()) },
                AgentEvent::TurnEnded { turn: Some("msg_1".into()), message: Some("Once upon a time.".into()) },
                AgentEvent::TurnEnded { turn: Some("msg_2".into()), message: Some("Once upon a time.\n\nFIN".into()) },
            ]
        );
        // A failed execution fails both; then nothing is left open.
        p.push(&ev("session.inbox.delivered", json!({"sessionID":s,"inboxID":"msg_3"})));
        p.push(&ev("session.inbox.delivered", json!({"sessionID":s,"inboxID":"msg_4"})));
        let failed = p.push(&ev("session.execution.failed", json!({"sessionID":s,"error":{"message":"No cookie auth credentials found"}})));
        assert_eq!(
            failed,
            [
                AgentEvent::TurnFailed { turn: Some("msg_3".into()), error: "No cookie auth credentials found".into() },
                AgentEvent::TurnFailed { turn: Some("msg_4".into()), error: "No cookie auth credentials found".into() },
            ]
        );
        // An execution the stream saw no delivery for still ends, as a turn of its own.
        assert_eq!(p.push(&ev("session.execution.succeeded", json!({"sessionID":s}))), [AgentEvent::TurnEnded { turn: None, message: None }]);
    }

    #[test]
    fn a_turn_the_stream_joined_mid_way_ends_after_the_named_ones() {
        // The stream missed the first prompt's delivery (it dropped and came back), then a second
        // prompt was steered in. Each prompt's turn keeps its own reply.
        use crate::activity::{Tracker, TurnChange, TurnEnd};
        let s = "ses_1";
        let (mut p, mut t) = (EventParser::new(s), Tracker::default());
        let mut changes = Vec::new();
        for e in [
            ev("session.step.started", json!({"sessionID":s,"assistantMessageID":"msg_a"})),
            ev("session.text.ended", json!({"sessionID":s,"assistantMessageID":"msg_a","ordinal":0,"text":"A"})),
            ev("session.inbox.enqueued", json!({"inboxID":"msg_2","sessionID":s,"item":{"type":"user","payload":{"text":"And B."}}})),
            ev("session.inbox.delivered", json!({"sessionID":s,"inboxID":"msg_2"})),
            ev("session.step.started", json!({"sessionID":s,"assistantMessageID":"msg_b"})),
            ev("session.text.ended", json!({"sessionID":s,"assistantMessageID":"msg_b","ordinal":0,"text":"B"})),
            ev("session.execution.succeeded", json!({"sessionID":s})),
        ] {
            for event in p.push(&e) {
                changes.extend(t.apply(&event, 0));
            }
        }
        assert_eq!(
            changes,
            [
                TurnChange::Opened { key: 1, turn: Some("msg_2".into()), prompt: Some("And B.".into()) },
                TurnChange::Closed { key: 1, end: TurnEnd::Done, message: Some("B".into()) },
                TurnChange::Opened { key: 2, turn: None, prompt: None },
                TurnChange::Closed { key: 2, end: TurnEnd::Done, message: Some("A".into()) },
            ]
        );
    }

    #[test]
    fn a_step_says_what_the_turn_ran_with() {
        let step = ev(
            "session.step.started",
            json!({"sessionID":"ses_1","agent":"plan","model":{"id":"deepseek-v4.1-flash-free","providerID":"opencode","variant":"high"}}),
        );
        assert_eq!(
            observed_from_event(&step, "ses_1"),
            Some(Observed { model: "opencode/deepseek-v4.1-flash-free".into(), variant: Some("high".into()), agent: Some("plan".into()) })
        );
        assert_eq!(observed_from_event(&step, "ses_2"), None);
        assert_eq!(observed_from_event(&ev("session.step.ended", json!({"sessionID":"ses_1"})), "ses_1"), None);
    }

    #[test]
    fn the_event_stream_is_split_into_payloads() {
        let mut buf = String::from("data: {\"type\":\"a\"}\n\n: heartbeat\n\ndata: {\"type\":\"b\"}\n\ndata: {\"ty");
        let got = take_sse_events(&mut buf);
        assert_eq!(got.len(), 2, "the heartbeat carries nothing");
        assert_eq!(got[1]["type"], "b");
        assert_eq!(buf, "data: {\"ty");
        buf.push_str("pe\":\"c\"}\n\n");
        assert_eq!(take_sse_events(&mut buf)[0]["type"], "c");
        assert!(buf.is_empty());
    }

    #[test]
    fn the_summary_names_what_the_server_and_session_get() {
        let o = opts(Some(Effort::Deep), Some(Permission::Plan), Some("opencode/deepseek-v4.1-flash-free"));
        assert_eq!(summary_entries(spec(), &o), ["default_agent=plan", "model=opencode/deepseek-v4.1-flash-free", "variant=high"]);
        let manual = summary_entries(spec(), &opts(None, Some(Permission::FullAccess), None));
        assert_eq!(manual, [r#"permissions=[{"action":"*","effect":"allow","resource":"*"}]"#]);
        assert!(summary_entries(spec(), &opts(None, None, None)).is_empty());
    }

    #[test]
    fn a_1x_opencode_is_refused_by_name() {
        let refusal = too_old(Some("1.19.3"), Some("2.1.1")).unwrap();
        assert_eq!(
            refusal,
            "opencode 1.19.3 is too old: Chorus drives opencode 2.x (checked 2.1.1), whose server and terminal UI work differently. Update it with npm install -g --allow-scripts=@opencode/cli @opencode/cli."
        );
        assert_eq!(too_old(Some("2.1.1"), Some("2.1.1")), None);
        assert_eq!(too_old(Some("2.4.0"), Some("2.1.1")), None, "newer is tried and reported");
        assert_eq!(too_old(Some("3.0.0"), Some("2.1.1")), None);
        assert_eq!(too_old(None, Some("2.1.1")), None, "an unread version is tried");
        assert_eq!(too_old(Some("garbled"), Some("2.1.1")), None);
    }

    #[test]
    fn a_skipped_key_check_is_said() {
        assert_eq!(key_check_skipped(&Ok((200, r#"{"data":[]}"#.to_owned()))), None);
        let shape = key_check_skipped(&Ok((200, r#"{"data":{}}"#.to_owned()))).unwrap();
        assert_eq!(shape, "Chorus couldn't check opencode's keys (its answer isn't in the shape Chorus checked), so a missing key shows only when the first turn fails.");
        assert!(key_check_skipped(&Ok((404, String::new()))).unwrap().contains("(opencode answered 404)"));
        assert!(key_check_skipped(&Err("timed out".into())).unwrap().contains("(timed out)"));
    }

    #[test]
    fn npms_placeholder_is_named_with_the_command_that_finishes_it() {
        let told = "opencode's install didn't finish (its install script was blocked). Run npm install -g --allow-scripts=@opencode/cli @opencode/cli.";
        assert_eq!(unfinished_install(Some(229), None).as_deref(), Some(told));
        assert_eq!(unfinished_install(None, Some("Error: opencode's postinstall script was not run.")).as_deref(), Some(told));
        assert_eq!(unfinished_install(Some(246_919_560), Some("2.1.4")), None);
        assert_eq!(unfinished_install(None, None), None);
    }

    #[test]
    fn the_argument_lists() {
        assert_eq!(serve_args(), ["serve", "--port", "0", "--hostname", "127.0.0.1"]);
        assert_eq!(tui_args("http://127.0.0.1:4096", "ses_x1"), ["--server", "http://127.0.0.1:4096", "--session", "ses_x1"]);
    }
}
