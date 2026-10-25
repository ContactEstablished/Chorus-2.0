//! What the relay sends (P15 slice 4): the loop's message types from the spec, in Telegram's HTML
//! (`parse_mode=HTML`). Every message starts with a tag naming the run, phase and task, so parallel
//! work never blurs together. D98: a message carries question text, options and short titles, never
//! code, diffs, file contents or paths; a run set to status only sends no question text at all.

/// Escapes text for Telegram's HTML: `&`, `<` and `>` (Bot API 10.3, "HTML style").
pub fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// At most `n` characters, with "…" when cut. Cut before escaping, so an entity is never split.
pub fn clip(s: &str, n: usize) -> String {
    let s = s.trim();
    if s.chars().count() <= n {
        s.to_owned()
    } else {
        format!("{}…", s.chars().take(n.saturating_sub(1)).collect::<String>().trim_end())
    }
}

/// What a button does, kept in Chorus behind its token.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "act", rename_all = "kebab-case")]
pub enum Act {
    Answer { text: String, rule: bool },
    Approve,
}

/// A message to send: HTML, whether it buzzes, and its buttons, one per row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outgoing {
    pub html: String,
    pub silent: bool,
    pub buttons: Vec<(String, Act)>,
}

impl Outgoing {
    fn loud(html: String) -> Self {
        Self { html, silent: false, buttons: Vec::new() }
    }
}

/// `[P03 T2]` or `[P03 plan]`, then the run's name.
pub fn tag(run: &str, phase: u32, task: Option<&str>) -> String {
    format!("<b>[P{phase:02} {}]</b> {}", escape(task.unwrap_or("plan")), escape(&clip(run, 40)))
}

/// `[P03]`, then the run's name: for what is about a whole phase.
pub fn phase_tag(run: &str, phase: u32) -> String {
    format!("<b>[P{phase:02}]</b> {}", escape(&clip(run, 40)))
}

/// A question as the loop's QUESTION message shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuestionCard<'a> {
    pub run: &'a str,
    pub phase: u32,
    pub task: Option<&'a str>,
    /// blocking | timed
    pub class: &'a str,
    pub question: &'a str,
    pub options: &'a [String],
    pub default: Option<&'a str>,
    pub timed_minutes: u32,
    pub status_only: bool,
    /// Once answered: the answer, whether it became a rule, and whether it was the default.
    pub answered: Option<(&'a str, bool, bool)>,
}

const LETTERS: &[u8] = b"ABCDEF";
pub const MAX_QUESTION: usize = 1500;
pub const MAX_OPTION: usize = 200;
const MAX_LABEL: usize = 40;

pub fn question(q: &QuestionCard<'_>) -> Outgoing {
    let kind = if q.class == "blocking" {
        "Blocking: the task waits for you".to_owned()
    } else {
        format!("Timed: the default in {} min", q.timed_minutes)
    };
    let mut html = format!("{} · {}", tag(q.run, q.phase, q.task), escape(&kind));
    if q.status_only {
        html.push_str("\nA question waits for you in Chorus. This run sends no question text to the phone.");
        return Outgoing::loud(html);
    }
    html.push_str(&format!("\n\n{}", escape(&clip(q.question, MAX_QUESTION))));
    for (i, o) in q.options.iter().enumerate() {
        let default = if Some(o.as_str()) == q.default { " (default)" } else { "" };
        html.push_str(&format!("\n{}) {}{default}", LETTERS[i % LETTERS.len()] as char, escape(&clip(o, MAX_OPTION))));
    }
    if let Some((answer, rule, by_default)) = q.answered {
        let how = if by_default { "Nobody answered in time; the default was taken" } else { "Answered" };
        let rule = if rule { " It is now a rule in decisions.md." } else { "" };
        html.push_str(&format!("\n\n<b>{how}:</b> {}.{rule}", escape(&clip(answer, MAX_OPTION))));
        return Outgoing { html, silent: true, buttons: Vec::new() };
    }
    html.push_str("\n\nTap an option, or reply to this message to answer in your own words.");
    let mut buttons: Vec<(String, Act)> = q
        .options
        .iter()
        .enumerate()
        .map(|(i, o)| (format!("{}) {}", LETTERS[i % LETTERS.len()] as char, clip(o, MAX_LABEL)), Act::Answer { text: o.clone(), rule: false }))
        .collect();
    // "…and make it a rule" for the default, else the first option.
    let pick = q.default.and_then(|d| q.options.iter().position(|o| o == d)).or((!q.options.is_empty()).then_some(0));
    if let Some(i) = pick {
        buttons.push((format!("{}, and make it a rule", LETTERS[i % LETTERS.len()] as char), Act::Answer { text: q.options[i].clone(), rule: true }));
    }
    Outgoing { html, silent: false, buttons }
}

/// `text` with every word that looks like a path (it has a slash or a backslash) replaced by "…",
/// for text bound for the phone (D98: never paths).
pub fn without_paths(text: &str) -> String {
    text.split(' ').map(|w| if w.contains('/') || w.contains('\\') { "…" } else { w }).collect::<Vec<_>>().join(" ")
}

/// STOPPED: which condition, the failing check's summary, and where to look. Never a path.
pub fn stopped(run: &str, phase: u32, title: &str, why: &str, last_good: &str) -> Outgoing {
    let why = without_paths(why);
    Outgoing::loud(format!(
        "{} · <b>STOPPED</b>\nP{phase:02} · {} stopped: {}\nThe last good state is {}. Open the run in Chorus to look at it.",
        phase_tag(run, phase),
        escape(&clip(title, 80)),
        escape(&clip(&why, 600)),
        escape(&clip(last_good, 60)),
    ))
}

/// MILESTONE: a phase passed at a milestone; the run waits for Matthew's review.
pub fn milestone(run: &str, phase: u32, title: &str, tag_name: &str, more: bool) -> Outgoing {
    let next = if more { "Review the run branch, then approve to start the next phase." } else { "It was the last phase in the roadmap." };
    let mut out = Outgoing::loud(format!(
        "<b>MILESTONE</b> · {}\nP{phase:02} · {} passed and is tagged {}. {next}",
        escape(&clip(run, 40)),
        escape(&clip(title, 80)),
        escape(tag_name)
    ));
    if more {
        out.buttons.push(("Approve".into(), Act::Approve));
    }
    out
}

/// PHASE DONE: silent, for the record.
pub fn phase_done(run: &str, phase: u32, title: &str, tag_name: &str) -> Outgoing {
    Outgoing {
        html: format!("{}: P{phase:02} · {} passed, tagged {}.", escape(&clip(run, 40)), escape(&clip(title, 80)), escape(tag_name)),
        silent: true,
        buttons: Vec::new(),
    }
}

/// The run finished every phase.
pub fn run_done(run: &str, tag_name: &str) -> Outgoing {
    Outgoing::loud(format!("<b>DONE</b> · {}\nEvery phase passed; the last is tagged {}. Review and merge the run branch yourself.", escape(&clip(run, 40)), escape(tag_name)))
}

/// One run's line in `/status`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunLine {
    pub name: String,
    pub state: String,
    pub phase: Option<(u32, String)>,
    pub step: Option<String>,
    pub open_questions: u32,
}

pub fn status(runs: &[RunLine]) -> Outgoing {
    if runs.is_empty() {
        return Outgoing { html: "No loop runs.".into(), silent: true, buttons: Vec::new() };
    }
    let lines: Vec<String> = runs
        .iter()
        .map(|r| {
            let mut l = format!("<b>{}</b>: {}", escape(&clip(&r.name, 40)), escape(&r.state));
            if let Some((n, t)) = &r.phase {
                l.push_str(&format!(", P{n:02} · {}", escape(&clip(t, 60))));
            }
            if let Some(s) = &r.step {
                l.push_str(&format!(", {}", escape(&clip(s, 60))));
            }
            if r.open_questions > 0 {
                l.push_str(&format!(". {} question{} waiting", r.open_questions, if r.open_questions == 1 { "" } else { "s" }));
            }
            l
        })
        .collect();
    Outgoing { html: lines.join("\n"), silent: true, buttons: Vec::new() }
}

pub const HELP: &str = "This bot relays Chorus's loop: questions that need you, and when a run stops, reaches a milestone or finishes. Tap an option to answer, or reply to a question in your own words.\n/status what the loop is doing\n/pause and /resume a run\n/approve the milestone a run waits on\n/stop a run";

#[cfg(test)]
mod tests {
    use super::*;

    fn card<'a>(options: &'a [String]) -> QuestionCard<'a> {
        QuestionCard {
            run: "F1 <ask>",
            phase: 3,
            task: Some("T2"),
            class: "timed",
            question: "Offset or cursor? Use `a < b` & co.",
            options,
            default: Some("cursor"),
            timed_minutes: 15,
            status_only: false,
            answered: None,
        }
    }

    #[test]
    fn a_question_has_its_tag_text_options_and_buttons() {
        let options = vec!["offset".to_owned(), "cursor".to_owned()];
        let q = question(&card(&options));
        assert!(q.html.starts_with("<b>[P03 T2]</b> F1 &lt;ask&gt; · Timed: the default in 15 min"), "{}", q.html);
        assert!(q.html.contains("Use `a &lt; b` &amp; co."));
        assert!(q.html.contains("\nA) offset\nB) cursor (default)"));
        assert!(!q.silent);
        assert_eq!(q.buttons.len(), 3);
        assert_eq!(q.buttons[2], ("B, and make it a rule".to_owned(), Act::Answer { text: "cursor".into(), rule: true }));
    }

    #[test]
    fn status_only_sends_no_text_and_answered_has_no_buttons() {
        let options = vec!["offset".to_owned(), "cursor".to_owned()];
        let q = question(&QuestionCard { status_only: true, ..card(&options) });
        assert!(!q.html.contains("Offset") && q.buttons.is_empty() && q.html.contains("waits for you in Chorus"));
        let a = question(&QuestionCard { answered: Some(("cursor", true, false)), ..card(&options) });
        assert!(a.buttons.is_empty() && a.html.contains("<b>Answered:</b> cursor. It is now a rule"), "{}", a.html);
        let d = question(&QuestionCard { answered: Some(("cursor", false, true)), ..card(&options) });
        assert!(d.html.contains("the default was taken"));
    }

    #[test]
    fn a_blocking_question_without_options_still_offers_a_reply() {
        let q = question(&QuestionCard { class: "blocking", default: None, ..card(&[]) });
        assert!(q.html.contains("Blocking: the task waits for you") && q.html.contains("reply to this message"));
        assert!(q.buttons.is_empty());
    }

    #[test]
    fn long_text_is_clipped_before_escaping() {
        let long = "<".repeat(5000);
        let q = question(&QuestionCard { question: &long, ..card(&[]) });
        assert!(q.html.chars().count() < 4096 * 5);
        assert!(!q.html.contains("&l…"), "an entity is never cut");
        assert_eq!(clip("abcdef", 4), "abc…");
    }

    #[test]
    fn stops_milestones_and_status() {
        let s = stopped("F1", 2, "Product", "a check still failed after the last fix attempt. tests: product multiplies failed", "phase-01");
        assert!(s.html.starts_with("<b>[P02]</b> F1 · <b>STOPPED</b>") && s.html.contains("last good state is phase-01") && !s.silent, "{}", s.html);
        let p = stopped("F1", 2, "Product", "build: src/app.ts(12,5): error TS2322 in C:\\work\\x", "main");
        assert!(!p.html.contains("src/app") && !p.html.contains("C:\\") && p.html.contains("error TS2322"), "{}", p.html);
        let m = milestone("F1", 1, "Sum", "phase-01", true);
        assert_eq!(m.buttons, vec![("Approve".to_owned(), Act::Approve)]);
        assert!(milestone("F1", 3, "Help", "phase-03", false).buttons.is_empty());
        assert!(phase_done("F1", 2, "Product", "phase-02").silent);
        let st = status(&[RunLine { name: "F1".into(), state: "running".into(), phase: Some((2, "Product".into())), step: Some("T1 build".into()), open_questions: 1 }]);
        assert_eq!(st.html, "<b>F1</b>: running, P02 · Product, T1 build. 1 question waiting");
    }

    #[test]
    fn acts_round_trip_as_json() {
        let a = Act::Answer { text: "cursor".into(), rule: true };
        assert_eq!(serde_json::from_str::<Act>(&serde_json::to_string(&a).unwrap()).unwrap(), a);
    }
}
