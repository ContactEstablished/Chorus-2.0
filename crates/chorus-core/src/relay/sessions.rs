//! Ordinary sessions on the phone (P15 slice 4, HANDOFF-P13's P13a, D101): when a session's change
//! reaches the phone, the cards for an ask, a failure and a finished turn, `/sessions`, `/use` and
//! the digest. Status only by default: a session's name, project, state and an ask's short title.
//! A project can opt in to full text, which the caller scrubs.

use super::render::{Act, Outgoing, clip, escape, without_paths};
use crate::activity::Activity;
use crate::attention::Attention;
use crate::replies::{REPLY_WAIT_SECS, ReplyDetails};

/// A finished turn reaches the phone when the desk has been idle this long (Chorus 1's rule).
pub const DESK_IDLE_SECS: u64 = 5 * 60;

/// What a session's change means for the phone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    /// It started waiting for an approval (`Approval`) or an answer (`Input`).
    Asked(Attention),
    Failed,
    /// A turn ended: it was working or waiting on an ask, and now waits for a prompt.
    TurnDone,
}

/// What to do about a summary, given the attention seen before it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Seen {
    pub send: Option<Change>,
    /// The ask it was waiting on is over, however it ended: the phone's card should say so.
    pub settle: bool,
}

/// `prev` none is the first summary seen since Chorus started: it is only remembered, so a restart
/// never repeats what an earlier run sent.
pub fn change(prev: Option<Attention>, now: Attention) -> Seen {
    let Some(prev) = prev else { return Seen::default() };
    let asked = |a: Attention| matches!(a, Attention::Approval | Attention::Input);
    let send = match now {
        a if asked(a) && a != prev => Some(Change::Asked(a)),
        Attention::Failed if prev != Attention::Failed => Some(Change::Failed),
        Attention::Ready if prev == Attention::Working || asked(prev) => Some(Change::TurnDone),
        _ => None,
    };
    Seen { send, settle: asked(prev) && now != prev }
}

/// Whether a finished turn is worth the phone: one the phone started, or the desk is idle.
pub fn tell_turn_done(phone_turn: bool, desk_idle_secs: u64) -> bool {
    phone_turn || desk_idle_secs >= DESK_IDLE_SECS
}

/// The session a card is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionRef<'a> {
    pub name: &'a str,
    pub project: Option<&'a str>,
}

fn head(s: &SessionRef<'_>) -> String {
    match s.project {
        Some(p) => format!("<b>{}</b> · {}", escape(&clip(s.name, 40)), escape(&clip(p, 40))),
        None => format!("<b>{}</b>", escape(&clip(s.name, 40))),
    }
}

/// An ask the phone can answer: the session, its agent's run and the ask, while the agent holds it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Held {
    pub session: u32,
    pub run: u32,
    pub ask: u32,
}

/// The card for a session waiting on an approval or an answer.
#[derive(Debug, Clone, Copy)]
pub struct AskCard<'a> {
    pub session: SessionRef<'a>,
    pub attention: Attention,
    /// What the agent asked, when it asked through a channel Chorus holds open.
    pub details: Option<&'a ReplyDetails>,
    pub held: Option<Held>,
    /// The project opted in to full text.
    pub full: bool,
    /// The agent's last message, shown with full text when it asked without a held ask.
    pub latest: Option<&'a str>,
}

const MAX_BUTTONS: usize = 6;
const LETTERS: &[u8] = b"ABCDEFGH";
const AT_DESK: &str = "Answer it at your desk.";

fn window() -> String {
    format!("The buttons work for about {} minutes; after that, answer at your desk.", REPLY_WAIT_SECS / 60)
}

pub fn ask_card(c: &AskCard<'_>) -> Outgoing {
    let mut out = Outgoing { html: head(&c.session), silent: false, buttons: Vec::new() };
    match (c.details, c.attention) {
        (Some(ReplyDetails::Approval { tool, input }), _) => {
            out.html.push_str(&format!("\nNeeds approval: <b>{}</b>", escape(&clip(tool, 60))));
            if c.full {
                out.html.push_str(&format!("\n<pre>{}</pre>", escape(&clip(input, 700))));
            }
            match c.held {
                Some(h) => {
                    out.buttons.push(("Allow once".into(), Act::Allow { session: h.session, run: h.run, ask: h.ask }));
                    out.buttons.push(("Deny".into(), Act::Deny { session: h.session, run: h.run, ask: h.ask }));
                    out.html.push_str(&format!("\n{}", window()));
                }
                None => out.html.push_str(&format!("\n{AT_DESK}")),
            }
        }
        (Some(ReplyDetails::Question { questions }), _) => {
            out.html.push_str(if questions.len() == 1 { "\nAsks you:" } else { "\nAsks you several things:" });
            for q in questions {
                out.html.push_str(&format!("\n\n{}", escape(&clip(&q.question, if c.full { 1000 } else { 160 }))));
                for (i, o) in q.options.iter().take(LETTERS.len()).enumerate() {
                    let mut line = format!("\n{}) {}", LETTERS[i] as char, escape(&clip(&o.label, 60)));
                    if c.full && !o.description.trim().is_empty() {
                        line.push_str(&format!(": {}", escape(&clip(&o.description, 200))));
                    }
                    out.html.push_str(&line);
                }
            }
            let single = match questions.as_slice() {
                [q] if !q.multi_select => Some(q),
                _ => None,
            };
            match (c.held, single) {
                (Some(h), Some(q)) => {
                    for o in q.options.iter().take(MAX_BUTTONS) {
                        out.buttons.push((clip(&o.label, 40), Act::Choose { session: h.session, run: h.run, ask: h.ask, answer: o.label.clone() }));
                    }
                    let how = if q.options.is_empty() { "Reply to this message with your answer." } else { "Tap an option, or reply to this message in your own words." };
                    out.html.push_str(&format!("\n\n{how} {}", window()));
                }
                _ => out.html.push_str(&format!("\n\n{AT_DESK}")),
            }
        }
        // It ended its turn with a question, or asked through a channel Chorus doesn't hold: a reply
        // is its next prompt.
        (None, Attention::Approval) => out.html.push_str(&format!("\nNeeds approval. {AT_DESK}")),
        (None, _) => {
            out.html.push_str("\nAsks you something.");
            if let (true, Some(latest)) = (c.full, c.latest) {
                out.html.push_str(&format!("\n\n{}", escape(&clip(latest, 600))));
            }
            out.html.push_str("\nReply to this message to answer.");
        }
    }
    out
}

/// FAILED: why, without paths.
pub fn failed(s: &SessionRef<'_>, why: Option<&str>) -> Outgoing {
    let why = why.map(|w| clip(&without_paths(w), 200)).filter(|w| !w.is_empty()).unwrap_or_else(|| "it stopped".into());
    Outgoing { html: format!("{} · <b>FAILED</b>\n{}", head(s), escape(&why)), silent: false, buttons: Vec::new() }
}

/// A finished turn; with full text, the agent's last message.
pub fn turn_done(s: &SessionRef<'_>, latest: Option<&str>, full: bool) -> Outgoing {
    let mut html = format!("{} finished its turn.", head(s));
    if let (true, Some(latest)) = (full, latest) {
        html.push_str(&format!("\n\n{}", escape(&clip(latest, 600))));
    }
    html.push_str("\nReply to this message to send it another prompt.");
    Outgoing { html, silent: false, buttons: Vec::new() }
}

/// How a settled ask ended, from the phone.
pub fn answered_from_phone(act: &Act) -> String {
    match act {
        Act::Allow { .. } => "Allowed from the phone.".into(),
        Act::Deny { .. } => "Denied from the phone.".into(),
        Act::Choose { answer, .. } => format!("Answered from the phone: {}", clip(answer, 80)),
        _ => "Answered from the phone.".into(),
    }
}

pub const NO_LONGER_WAITING: &str = "No longer waiting: answered at the desk, or the agent moved on.";

/// One open session, for `/sessions` and the digest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionLine {
    pub id: u32,
    pub name: String,
    pub project: Option<String>,
    pub attention: Attention,
    /// How long it has needed you, when it does.
    pub waiting_secs: Option<u64>,
}

pub fn state_word(a: Attention) -> &'static str {
    match a {
        Attention::Approval => "needs approval",
        Attention::Input => "asks you something",
        Attention::Failed => "failed",
        Attention::Working => "working",
        Attention::Ready => "ready",
        Attention::Running => "running",
        Attention::Done => "finished",
    }
}

/// "45 s", "4 min", "2 h 5 min", "3 d".
pub fn wait_text(secs: u64) -> String {
    match secs {
        0..60 => format!("{secs} s"),
        60..3600 => format!("{} min", secs / 60),
        3600..86_400 if secs % 3600 >= 60 => format!("{} h {} min", secs / 3600, secs % 3600 / 60),
        3600..86_400 => format!("{} h", secs / 3600),
        _ => format!("{} d", secs / 86_400),
    }
}

/// The order `/sessions` numbers them in: what needs you first, longest waiting first, then by name.
pub fn order(lines: &mut [SessionLine]) {
    lines.sort_by(|a, b| {
        b.attention.needs_you().cmp(&a.attention.needs_you()).then(b.waiting_secs.cmp(&a.waiting_secs)).then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
}

fn line(l: &SessionLine) -> String {
    let mut s = format!("<b>{}</b>", escape(&clip(&l.name, 40)));
    if let Some(p) = &l.project {
        s.push_str(&format!(" · {}", escape(&clip(p, 40))));
    }
    s.push_str(&format!(" · {}", state_word(l.attention)));
    if let Some(w) = l.waiting_secs.filter(|_| l.attention.needs_you()) {
        s.push_str(&format!(", {}", wait_text(w)));
    }
    s
}

/// `/sessions`: numbered, in [`order`]; the `/use` target marked.
pub fn sessions_list(lines: &[SessionLine], target: Option<u32>) -> String {
    if lines.is_empty() {
        return "No sessions are open.".into();
    }
    let mut out: Vec<String> = lines
        .iter()
        .enumerate()
        .map(|(i, l)| format!("{}. {}{}", i + 1, line(l), if target == Some(l.id) { " ← messages go here" } else { "" }))
        .collect();
    out.push(String::new());
    out.push("Send /use 2 to send plain messages to session 2, or reply to a session's message.".into());
    out.join("\n")
}

/// `/use 2`, `/use api` or `/use off`: the session plain messages go to, or none. `lines` is in
/// the order `/sessions` numbers them.
pub fn resolve_use(lines: &[SessionLine], arg: &str) -> Result<Option<u32>, String> {
    let arg = arg.trim();
    if arg.eq_ignore_ascii_case("off") || arg.eq_ignore_ascii_case("none") {
        return Ok(None);
    }
    if let Ok(n) = arg.parse::<usize>() {
        return lines.get(n.wrapping_sub(1)).map(|l| Some(l.id)).ok_or_else(|| format!("There is no session {n}. Send /sessions to see them."));
    }
    let want = arg.to_lowercase();
    if let Some(l) = lines.iter().find(|l| l.name.to_lowercase() == want) {
        return Ok(Some(l.id));
    }
    let close: Vec<&SessionLine> = lines.iter().filter(|l| l.name.to_lowercase().contains(&want)).collect();
    match close.as_slice() {
        [one] => Ok(Some(one.id)),
        [] => Err(format!("No open session is called \"{}\". Send /sessions to see them.", clip(arg, 40))),
        many => Err(format!("\"{}\" matches {} sessions: use its number from /sessions.", clip(arg, 40), many.len())),
    }
}

/// The digest's time, "HH:MM" on a 24-hour clock.
pub fn parse_time(s: &str) -> Option<(u8, u8)> {
    let (h, m) = s.trim().split_once(':')?;
    if h.is_empty() || h.len() > 2 || m.len() != 2 {
        return None;
    }
    let (hour, minute): (u8, u8) = (h.parse().ok()?, m.parse().ok()?);
    (hour < 24 && minute < 60).then_some((hour, minute))
}

/// What the digest covers.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DigestFacts {
    /// Sessions that need you, oldest first.
    pub needs: Vec<SessionLine>,
    /// Names of sessions whose last turn finished today.
    pub finished: Vec<String>,
    pub failed: Vec<String>,
}

pub fn digest(f: &DigestFacts) -> Outgoing {
    let mut html = String::from("<b>Digest</b>");
    if f.needs.is_empty() && f.finished.is_empty() && f.failed.is_empty() {
        html.push_str("\nNothing needs you, and nothing finished or failed today.");
    }
    if !f.needs.is_empty() {
        html.push_str("\n\n<b>Needs you</b>");
        for l in &f.needs {
            html.push_str(&format!("\n• {}", line(l)));
        }
    }
    let names = |v: &[String]| v.iter().map(|n| escape(&clip(n, 40))).collect::<Vec<_>>().join(", ");
    if !f.finished.is_empty() {
        html.push_str(&format!("\n\n<b>Finished today</b>\n{}", names(&f.finished)));
    }
    if !f.failed.is_empty() {
        html.push_str(&format!("\n\n<b>Failed</b>\n{}", names(&f.failed)));
    }
    Outgoing { html, silent: true, buttons: Vec::new() }
}

/// Why a prompt from the phone can't go to a session now, in the phone's words (D101: a busy
/// session refuses rather than queues). The delivery path checks again and has the last word.
pub fn prompt_refusal(name: &str, locked: bool, live: bool, activity: Activity) -> Option<String> {
    let name = clip(name, 40);
    if locked {
        Some(format!("{name} is locked. Unlock it at your desk first."))
    } else if !live {
        Some(format!("{name} isn't running. Restart it at your desk first."))
    } else if activity == Activity::AwaitingApproval {
        Some(format!("{name} is waiting for an approval. Answer that first."))
    } else if activity == Activity::Working {
        Some(format!("{name} is working; send again when it's ready."))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::replies::{Question, QuestionOption};

    const S: SessionRef<'static> = SessionRef { name: "api", project: Some("Shop") };

    #[test]
    fn changes_reach_the_phone_once_and_never_after_a_restart() {
        use Attention::*;
        assert_eq!(change(None, Approval), Seen::default(), "the first summary is only remembered");
        assert_eq!(change(Some(Working), Approval), Seen { send: Some(Change::Asked(Approval)), settle: false });
        assert_eq!(change(Some(Approval), Approval), Seen::default());
        assert_eq!(change(Some(Approval), Working), Seen { send: None, settle: true });
        assert_eq!(change(Some(Working), Ready).send, Some(Change::TurnDone));
        assert_eq!(change(Some(Input), Ready), Seen { send: Some(Change::TurnDone), settle: true });
        assert_eq!(change(Some(Ready), Ready).send, None);
        assert_eq!(change(Some(Working), Failed).send, Some(Change::Failed));
        assert_eq!(change(Some(Failed), Failed).send, None);
        assert_eq!(change(Some(Approval), Input), Seen { send: Some(Change::Asked(Input)), settle: true });
        assert!(tell_turn_done(true, 0) && tell_turn_done(false, 300) && !tell_turn_done(false, 299));
    }

    fn approval() -> ReplyDetails {
        ReplyDetails::Approval { tool: "Bash".into(), input: "{\"command\": \"rm -rf C:\\\\work\"}".into() }
    }

    #[test]
    fn an_approval_is_status_only_unless_the_project_opts_in() {
        let d = approval();
        let held = Held { session: 4, run: 9, ask: 21 };
        let card = AskCard { session: S, attention: Attention::Approval, details: Some(&d), held: Some(held), full: false, latest: None };
        let out = ask_card(&card);
        assert!(out.html.starts_with("<b>api</b> · Shop\nNeeds approval: <b>Bash</b>") && !out.html.contains("rm -rf"), "{}", out.html);
        assert_eq!(out.buttons, vec![("Allow once".into(), Act::Allow { session: 4, run: 9, ask: 21 }), ("Deny".into(), Act::Deny { session: 4, run: 9, ask: 21 })]);
        assert!(ask_card(&AskCard { full: true, ..card }).html.contains("<pre>{\"command\": \"rm -rf"));
        let gone = ask_card(&AskCard { held: None, ..card });
        assert!(gone.buttons.is_empty() && gone.html.ends_with("Answer it at your desk."));
    }

    #[test]
    fn a_question_gets_option_buttons_only_when_one_single_choice_question_is_held() {
        let q = |text: &str, multi: bool| Question {
            question: text.into(),
            options: vec![QuestionOption { label: "Postgres".into(), description: "the usual".into() }, QuestionOption { label: "SQLite".into(), description: String::new() }],
            multi_select: multi,
        };
        let one = ReplyDetails::Question { questions: vec![q("Which database?", false)] };
        let held = Some(Held { session: 4, run: 9, ask: 22 });
        let card = AskCard { session: S, attention: Attention::Input, details: Some(&one), held, full: false, latest: None };
        let out = ask_card(&card);
        assert!(out.html.contains("Which database?\nA) Postgres\nB) SQLite") && !out.html.contains("the usual"), "{}", out.html);
        assert_eq!(out.buttons[1], ("SQLite".into(), Act::Choose { session: 4, run: 9, ask: 22, answer: "SQLite".into() }));
        let two = ReplyDetails::Question { questions: vec![q("Which database?", false), q("Which cache?", false)] };
        assert!(ask_card(&AskCard { details: Some(&two), ..card }).buttons.is_empty());
        let multi = ReplyDetails::Question { questions: vec![q("Which?", true)] };
        assert!(ask_card(&AskCard { details: Some(&multi), ..card }).buttons.is_empty());
        let ended = ask_card(&AskCard { details: None, held: None, full: true, latest: Some("Shall I <go> on?"), ..card });
        assert!(ended.html.contains("Shall I &lt;go&gt; on?") && ended.html.ends_with("Reply to this message to answer."), "{}", ended.html);
        assert!(!ask_card(&AskCard { details: None, held: None, full: false, latest: Some("secret plan"), ..card }).html.contains("secret"));
    }

    #[test]
    fn failures_and_finished_turns() {
        let f = failed(&S, Some("exit 1: couldn't open C:\\work\\db.sqlite"));
        assert_eq!(f.html, "<b>api</b> · Shop · <b>FAILED</b>\nexit 1: couldn't open …");
        assert!(turn_done(&S, Some("Done: 3 files"), false).html.starts_with("<b>api</b> · Shop finished its turn.\nReply"));
        assert!(turn_done(&S, Some("Done: 3 files"), true).html.contains("\n\nDone: 3 files\n"));
        assert_eq!(answered_from_phone(&Act::Deny { session: 1, run: 1, ask: 1 }), "Denied from the phone.");
    }

    fn l(id: u32, name: &str, a: Attention, wait: Option<u64>) -> SessionLine {
        SessionLine { id, name: name.into(), project: None, attention: a, waiting_secs: wait }
    }

    #[test]
    fn sessions_are_numbered_needs_first_and_use_finds_them() {
        let mut lines = vec![l(1, "web", Attention::Working, None), l(2, "api", Attention::Approval, Some(90)), l(3, "Docs", Attention::Ready, None), l(4, "api-tests", Attention::Input, Some(600))];
        order(&mut lines);
        assert_eq!(lines.iter().map(|l| l.id).collect::<Vec<_>>(), [4, 2, 3, 1]);
        let list = sessions_list(&lines, Some(2));
        assert!(list.starts_with("1. <b>api-tests</b> · asks you something, 10 min\n2. <b>api</b> · needs approval, 1 min ← messages go here\n3. <b>Docs</b> · ready"), "{list}");
        assert_eq!(resolve_use(&lines, "2"), Ok(Some(2)));
        assert_eq!(resolve_use(&lines, "API"), Ok(Some(2)), "an exact name wins over a longer one");
        assert_eq!(resolve_use(&lines, "doc"), Ok(Some(3)));
        assert!(resolve_use(&lines, "ap").unwrap_err().contains("matches 2 sessions"));
        assert!(resolve_use(&lines, "9").unwrap_err().starts_with("There is no session 9"));
        assert!(resolve_use(&lines, "0").is_err());
        assert_eq!(resolve_use(&lines, "off"), Ok(None));
        assert_eq!(sessions_list(&[], None), "No sessions are open.");
    }

    #[test]
    fn the_digest_and_wait_times() {
        assert_eq!(parse_time("08:30"), Some((8, 30)));
        assert_eq!(parse_time(" 7:05 "), Some((7, 5)));
        assert_eq!(parse_time("24:00"), None);
        assert_eq!(parse_time("8:5"), None);
        assert_eq!(parse_time("noon"), None);
        assert_eq!(wait_text(45), "45 s");
        assert_eq!(wait_text(7500), "2 h 5 min");
        assert_eq!(wait_text(7200), "2 h");
        assert_eq!(wait_text(3 * 86_400), "3 d");
        assert!(digest(&DigestFacts::default()).html.contains("Nothing needs you"));
        let d = digest(&DigestFacts { needs: vec![l(2, "api", Attention::Approval, Some(7500))], finished: vec!["web".into(), "docs".into()], failed: vec!["<x>".into()] });
        assert_eq!(d.html, "<b>Digest</b>\n\n<b>Needs you</b>\n• <b>api</b> · needs approval, 2 h 5 min\n\n<b>Finished today</b>\nweb, docs\n\n<b>Failed</b>\n&lt;x&gt;");
        assert!(d.silent);
    }

    #[test]
    fn a_phone_prompt_is_refused_in_the_phones_words() {
        assert_eq!(prompt_refusal("api", false, true, Activity::Working).as_deref(), Some("api is working; send again when it's ready."));
        assert!(prompt_refusal("api", true, true, Activity::Ready).unwrap().contains("locked"));
        assert!(prompt_refusal("api", false, false, Activity::Ready).unwrap().contains("isn't running"));
        assert!(prompt_refusal("api", false, true, Activity::AwaitingApproval).unwrap().contains("approval"));
        assert_eq!(prompt_refusal("api", false, true, Activity::Ready), None);
        assert_eq!(prompt_refusal("api", false, true, Activity::NeedsAnswer), None);
    }
}
