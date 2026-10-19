//! opencode as a server plus an attached TUI (S5, ARCHITECTURE §5). Each run starts
//! `opencode serve` on loopback, creates or reuses the session through its API, and runs
//! `opencode attach` in the PTY. The options are the server's configuration
//! (`OPENCODE_CONFIG_CONTENT`), and what a turn really ran with is read back from its stored
//! messages, the only proof opencode gives (LESSONS L5). Measured on opencode 1.19.3.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use specta::Type;

use crate::agent::{LaunchOptions, Spec};

/// The basic-auth user `opencode serve` expects when only a password is set.
pub const USERNAME: &str = "opencode";
/// Both the server and `attach` read the password from this variable, so it never needs argv.
pub const PASSWORD_VAR: &str = "OPENCODE_SERVER_PASSWORD";
pub const CONFIG_VAR: &str = "OPENCODE_CONFIG_CONTENT";
/// How many of the newest messages a read-back fetches (`GET /session/{id}/message?limit=`).
pub const READ_BACK_MESSAGES: u32 = 6;

/// The agents an effort is set for: `build` (the default) and `plan`. A variant applies only to the
/// agent whose block names the same model the turn runs.
const EFFORT_AGENTS: [&str; 2] = ["build", "plan"];

/// Port 0 lets opencode choose (it tries 4096 first), so the port is read from its output.
pub fn serve_args() -> Vec<String> {
    ["serve", "--port", "0", "--hostname", "127.0.0.1"].map(str::to_owned).to_vec()
}

pub fn attach_args(url: &str, session_id: &str) -> Vec<String> {
    ["attach", url, "--session", session_id].map(str::to_owned).to_vec()
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
/// is set. The effort goes into an agent block beside the model, because opencode drops a variant
/// whose model differs from the turn's.
pub fn config_content(spec: &Spec, o: &LaunchOptions) -> Option<String> {
    let mut root = Map::new();
    if let Some(model) = &o.model {
        root.insert("model".into(), json!(model));
        if let Some(variant) = wanted_variant(spec, o) {
            let agents: Map<String, Value> =
                EFFORT_AGENTS.iter().map(|a| ((*a).to_owned(), json!({ "model": model, "variant": variant }))).collect();
            root.insert("agent".into(), Value::Object(agents));
        }
    }
    if let Some((key, value)) = o.permission.and_then(|p| spec.permission_args(p)).and_then(entry) {
        root.insert(key, value);
    }
    (!root.is_empty()).then(|| Value::Object(root).to_string())
}

/// The server's address from its output: `opencode server listening on http://127.0.0.1:4096`.
/// Only a loopback address is taken.
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

/// The session id from `POST /session`'s answer.
pub fn created_session_id(body: &str) -> Option<String> {
    let v: Value = serde_json::from_str(body).ok()?;
    v.get("id")?.as_str().filter(|id| is_session_id(id)).map(str::to_owned)
}

/// What the server says about a `provider/model` (`GET /provider`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelCheck {
    Known { variants: Vec<String> },
    NoSuchProvider,
    /// The provider exists but has no key or login, so its models won't run.
    NotConnected,
    NoSuchModel,
}

/// `None` when the answer isn't the shape 1.19.3 gives.
pub fn check_model(provider_body: &str, model: &str) -> Option<ModelCheck> {
    let v: Value = serde_json::from_str(provider_body).ok()?;
    let (provider, id) = model.split_once('/')?;
    let all = v.get("all")?.as_array()?;
    let Some(p) = all.iter().find(|p| p.get("id").and_then(Value::as_str) == Some(provider)) else {
        return Some(ModelCheck::NoSuchProvider);
    };
    let connected = v.get("connected")?.as_array()?;
    if !connected.iter().any(|c| c.as_str() == Some(provider)) {
        return Some(ModelCheck::NotConnected);
    }
    let Some(m) = p.get("models")?.get(id) else {
        return Some(ModelCheck::NoSuchModel);
    };
    let mut variants: Vec<String> =
        m.get("variants").and_then(Value::as_object).map(|o| o.keys().cloned().collect()).unwrap_or_default();
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
        ModelCheck::NotConnected => Some(format!("opencode has no key or login for {provider}, so {model} won't run.")),
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

/// What a turn really ran with, read back from opencode's stored message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Observed {
    /// `provider/model`.
    pub model: String,
    pub variant: Option<String>,
    pub agent: Option<String>,
}

/// The newest user message's model, variant and agent, from `GET /session/{id}/message`.
pub fn latest_user_turn(messages_body: &str) -> Option<Observed> {
    let v: Value = serde_json::from_str(messages_body).ok()?;
    let info = v.as_array()?.iter().rev().filter_map(|m| m.get("info")).find(|i| i.get("role").and_then(Value::as_str) == Some("user"))?;
    let text = |o: &Value, k: &str| o.get(k).and_then(Value::as_str).map(str::to_owned);
    let model = info.get("model")?;
    Some(Observed {
        model: format!("{}/{}", text(model, "providerID")?, text(model, "modelID")?),
        variant: text(model, "variant"),
        agent: text(info, "agent"),
    })
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
        // Nothing asked: nothing to differ from, whatever the user picked in the TUI.
        assert!(differences(spec(), &opts(None, None, None), &seen("opencode/big-pickle", None, "plan")).is_empty());
    }

    fn spec() -> &'static Spec {
        AgentKind::Opencode.spec()
    }

    fn opts(effort: Option<Effort>, permission: Option<Permission>, model: Option<&str>) -> LaunchOptions {
        spec().resolve(&LaunchOptions { effort, permission, model: model.map(str::to_owned) }).unwrap()
    }

    #[test]
    fn the_effort_travels_beside_its_model() {
        let o = opts(Some(Effort::Deep), Some(Permission::Plan), Some("opencode/glm-5.3-flash-free"));
        let c: Value = serde_json::from_str(&config_content(spec(), &o).unwrap()).unwrap();
        assert_eq!(c["model"], "opencode/glm-5.3-flash-free");
        for agent in ["build", "plan"] {
            assert_eq!(c["agent"][agent], json!({ "model": "opencode/glm-5.3-flash-free", "variant": "high" }));
        }
        assert_eq!(c["default_agent"], "plan");
        assert_eq!(wanted_variant(spec(), &o).as_deref(), Some("high"));
    }

    #[test]
    fn each_permission_becomes_its_configuration() {
        let c = |p| serde_json::from_str::<Value>(&config_content(spec(), &opts(None, Some(p), None)).unwrap()).unwrap();
        assert_eq!(c(Permission::FullAccess), json!({ "permission": "allow" }));
        assert_eq!(c(Permission::Manual)["permission"]["edit"], "ask");
        assert_eq!(c(Permission::AcceptEdits)["permission"]["edit"], "allow");
        assert_eq!(c(Permission::AcceptEdits)["permission"]["bash"], "ask");
        // A model alone sets the model and no agent block.
        let only_model = config_content(spec(), &opts(None, None, Some("opencode/big-pickle"))).unwrap();
        assert_eq!(serde_json::from_str::<Value>(&only_model).unwrap(), json!({ "model": "opencode/big-pickle" }));
        assert_eq!(config_content(spec(), &opts(None, None, None)), None);
    }

    #[test]
    fn reads_the_listening_line() {
        assert_eq!(listening_url("opencode server listening on http://127.0.0.1:4096\n").as_deref(), Some("http://127.0.0.1:4096"));
        assert_eq!(listening_url("noise\r\nopencode server listening on http://127.0.0.1:51234").as_deref(), Some("http://127.0.0.1:51234"));
        assert_eq!(listening_url("opencode server listening on http://0.0.0.0:4096"), None);
        assert_eq!(listening_url("listening on http://127.0.0.1:0"), None);
        assert_eq!(listening_url("listening on http://127.0.0.1:99999"), None);
    }

    #[test]
    fn session_ids_are_checked() {
        assert!(is_session_id("ses_eaa39d315ffeDlPW7mMYhRWT3t"));
        for bad in ["", "ses_", "ses_a b", "sess_x", "ses_x/../y", "--session"] {
            assert!(!is_session_id(bad), "{bad}");
        }
        assert_eq!(created_session_id(r#"{"id":"ses_abc1","title":"x"}"#).as_deref(), Some("ses_abc1"));
        assert_eq!(created_session_id(r#"{"id":"--fork"}"#), None);
    }

    const PROVIDERS: &str = r#"{"all":[
        {"id":"opencode","models":{"glm-5.3-flash-free":{"variants":{"low":{},"high":{},"max":{}}},"big-pickle":{"variants":{}}}},
        {"id":"anthropic","models":{"claude-sonnet-4-6":{}}}],
        "default":{},"connected":["opencode"]}"#;

    #[test]
    fn the_model_is_checked_against_the_server() {
        let known = check_model(PROVIDERS, "opencode/glm-5.3-flash-free").unwrap();
        assert_eq!(known, ModelCheck::Known { variants: vec!["low".into(), "high".into(), "max".into()] });
        assert_eq!(model_note("opencode/glm-5.3-flash-free", Some("high"), &known), None);
        assert_eq!(
            model_note("opencode/glm-5.3-flash-free", Some("medium"), &known).unwrap(),
            "opencode/glm-5.3-flash-free has no medium effort (it has low, high, max), so opencode uses its default."
        );
        let none = check_model(PROVIDERS, "opencode/big-pickle").unwrap();
        assert!(model_note("opencode/big-pickle", Some("low"), &none).unwrap().contains("no effort settings"));
        assert_eq!(model_note("opencode/big-pickle", None, &none), None);
        assert_eq!(check_model(PROVIDERS, "anthropic/claude-sonnet-4-6"), Some(ModelCheck::NotConnected));
        assert_eq!(check_model(PROVIDERS, "nobody/x"), Some(ModelCheck::NoSuchProvider));
        assert_eq!(check_model(PROVIDERS, "opencode/nope"), Some(ModelCheck::NoSuchModel));
        assert_eq!(check_model("[]", "opencode/x"), None);
    }

    #[test]
    fn reads_back_what_the_newest_turn_ran_with() {
        // The shape 1.19.3 stores (trimmed): a user message carries the model and variant.
        let body = r#"[
            {"info":{"role":"user","agent":"build","model":{"providerID":"opencode","modelID":"glm-5.3-flash-free","variant":"high"}},"parts":[]},
            {"info":{"role":"assistant","providerID":"opencode","modelID":"glm-5.3-flash-free","variant":"high"},"parts":[]},
            {"info":{"role":"user","agent":"plan","model":{"providerID":"opencode","modelID":"big-pickle"}},"parts":[]},
            {"info":{"role":"assistant"},"parts":[]}]"#;
        assert_eq!(
            latest_user_turn(body),
            Some(Observed { model: "opencode/big-pickle".into(), variant: None, agent: Some("plan".into()) })
        );
        assert_eq!(latest_user_turn("[]"), None);
        assert_eq!(latest_user_turn("{}"), None);
    }

    #[test]
    fn the_argument_lists() {
        assert_eq!(serve_args(), ["serve", "--port", "0", "--hostname", "127.0.0.1"]);
        assert_eq!(attach_args("http://127.0.0.1:4096", "ses_x1"), ["attach", "http://127.0.0.1:4096", "--session", "ses_x1"]);
    }
}
