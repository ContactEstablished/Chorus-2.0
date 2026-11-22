//! Chorus updates (D135): short messages from Chorus to Matthew through OpenMausBot, sent into
//! one conversation, "Chorus updates", with the bot he chooses (Jammie by default). A session sends
//! one with its `maus_send` tool; Chorus sends one by itself, while Matthew is away, when a session
//! finishes, fails or needs him, or a loop run stops.
//!
//! MausBot 0.1.173 has no notify-only route: a message is a prompt, the bot replies, and the
//! bot's finished turn is what pushes to the phone ("Jammie finished"). So each message ends with
//! an instruction to echo it and do nothing else. Pure: formatting, the 1 KB cap, the debounce
//! and the rate limits, with every clock passed in.

use std::collections::{HashMap, VecDeque};

use serde_json::{Value, json};

use crate::activity::Activity;
use crate::handoff::one_line;
use crate::maus::Bot;

/// The whole message MausBot gets, instruction included, is at most this many bytes.
pub const MAX_BYTES: usize = 1024;
/// What a session may write, before the cap.
pub const MAX_MESSAGE_CHARS: usize = 1000;
pub const CONVERSATION_TITLE: &str = "Chorus updates";
/// The bot chosen when Matthew hasn't picked one.
pub const DEFAULT_BOT_NAME: &str = "Jammie";
/// The tool's name on Chorus's session MCP server, and the name Claude Code gives it.
pub const TOOL_NAME: &str = "maus_send";
pub const CLAUDE_TOOL_NAME: &str = "mcp__chorus__maus_send";
/// The bot is told to echo the message: its reply is what MausBot's notification shows, and an
/// echo keeps the turn small and stops it acting on a notice ("failed") by itself.
pub const ECHO: &str = "(A notice from Chorus. Reply with the text above, word for word, and do nothing else.)";
/// How long a session's automatic notice waits for the session to settle (K3's mechanism, longer:
/// a turn that ends and starts again at once is not worth a message).
pub const SETTLE_MS: i64 = 10_000;

/// Where a message comes from. Missing parts are left out of the prefix.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Origin {
    pub project: Option<String>,
    pub session: Option<String>,
    pub branch: Option<String>,
}

fn part(s: &Option<String>, max: usize) -> Option<String> {
    s.as_deref().map(|v| one_line(v, max)).filter(|v| !v.is_empty())
}

/// `[Chorus] <project> · <session>`, with `tail` (the branch, or a notice's kind) after them.
fn head(o: &Origin, tail: Option<&str>) -> String {
    let parts: Vec<String> = [part(&o.project, 60), part(&o.session, 60), tail.map(|t| one_line(t, 80)).filter(|t| !t.is_empty())]
        .into_iter()
        .flatten()
        .collect();
    if parts.is_empty() { "[Chorus]".to_owned() } else { format!("[Chorus] {}", parts.join(" · ")) }
}

/// `[Chorus] <project> · <session> · <branch>`.
pub fn prefix(o: &Origin) -> String {
    head(o, o.branch.as_deref())
}

/// The longest prefix of `s` that fits `max` bytes, on a character boundary.
fn fit(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// The message as MausBot gets it: `body`, cut with "…" so that it and the echo instruction fit
/// [`MAX_BYTES`]. Returns (what was kept of the body, the full text to send).
pub fn finish(body: &str) -> (String, String) {
    let room = MAX_BYTES - ECHO.len() - 2;
    let kept = if body.len() <= room {
        body.to_owned()
    } else {
        let cut = fit(body, room - '…'.len_utf8());
        format!("{}…", cut.trim_end())
    };
    let full = format!("{kept}\n\n{ECHO}");
    (kept, full)
}

/// A session's own message: checked, with Chorus's prefix. Returns the body before [`finish`].
pub fn ad_hoc(o: &Origin, message: &str) -> Result<String, String> {
    let message = message.trim();
    if message.is_empty() {
        return Err("message is required.".into());
    }
    if message.chars().count() > MAX_MESSAGE_CHARS {
        return Err(format!("Keep the message under {MAX_MESSAGE_CHARS} characters."));
    }
    // Control characters go; line breaks stay.
    let message: String = message.chars().filter(|c| !c.is_control() || *c == '\n').collect();
    Ok(format!("{}\n{message}", prefix(o)))
}

/// Why Chorus sends a notice by itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Done,
    Failed,
    NeedsYou,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Done => "done",
            Self::Failed => "failed",
            Self::NeedsYou => "needs you",
        }
    }

    /// The log's word for it.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Done => "done",
            Self::Failed => "failed",
            Self::NeedsYou => "needs-you",
        }
    }

    /// A turn that ended in `activity`: finished, failed, or waiting on Matthew. `None` while it
    /// works or without an event channel.
    pub fn of_turn_end(activity: Activity) -> Option<Self> {
        match activity {
            Activity::Ready => Some(Self::Done),
            Activity::Failed => Some(Self::Failed),
            Activity::NeedsAnswer | Activity::AwaitingApproval => Some(Self::NeedsYou),
            Activity::Working | Activity::Unknown => None,
        }
    }
}

/// What a notice says.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NoticeFacts {
    /// One line on what finished: the last reply, else the plan step.
    pub what: Option<String>,
    pub tests: Option<String>,
}

/// `[Chorus] <project> · <session> · <kind>`, a line on what finished, then the branch and the
/// test result when there are any.
pub fn notice(o: &Origin, kind: Kind, f: &NoticeFacts) -> String {
    let mut s = head(o, Some(kind.label()));
    if let Some(what) = part(&f.what, 300) {
        s.push('\n');
        s.push_str(&what);
    }
    if let Some(branch) = part(&o.branch, 120) {
        s.push_str(&format!("\nBranch: {branch}"));
    }
    if let Some(tests) = part(&f.tests, 200) {
        s.push_str(&format!("\nTests: {tests}"));
    }
    s
}

/// The line on what finished: the reply's first non-empty line, else the plan step in progress,
/// else the last one completed. Plan steps are `(text, status)`.
pub fn what_finished(reply: Option<&str>, plan: &[(String, String)]) -> Option<String> {
    if let Some(line) = reply.and_then(|r| r.lines().map(str::trim).find(|l| !l.is_empty())) {
        return Some(one_line(line, 300));
    }
    let doing = plan.iter().find(|(_, s)| matches!(s.as_str(), "in-progress" | "in_progress" | "inProgress"));
    let done = plan.iter().rev().find(|(_, s)| s == "completed");
    doing.or(done).map(|(t, _)| one_line(t, 300))
}

/// A loop step's checks in a few words: `(name, passed)` each. `None` without checks.
pub fn tests_line(checks: &[(String, bool)]) -> Option<String> {
    if checks.is_empty() {
        return None;
    }
    let failed: Vec<&str> = checks.iter().filter(|(_, ok)| !ok).map(|(n, _)| n.as_str()).collect();
    Some(if failed.is_empty() {
        format!("all {} checks passed", checks.len())
    } else {
        one_line(&format!("{} of {} checks failed: {}", failed.len(), checks.len(), failed.join(", ")), 200)
    })
}

/// MausBot's send id for one logged message, so a repeated send never posts twice: 16–80 letters,
/// digits, `-` or `_`.
pub fn send_id(log_id: i64, nonce_hex: &str) -> String {
    let nonce: String = nonce_hex.chars().filter(char::is_ascii_hexdigit).take(16).collect();
    format!("chorus-update-{log_id}-{nonce}")
}

/// The bot to send to: the one chosen if MausBot still lists it, else the one named
/// [`DEFAULT_BOT_NAME`] (any case).
pub fn pick_bot<'a>(bots: &'a [Bot], chosen: Option<&str>) -> Option<&'a Bot> {
    if let Some(id) = chosen {
        return bots.iter().find(|b| b.id == id);
    }
    bots.iter().find(|b| b.name.trim().eq_ignore_ascii_case(DEFAULT_BOT_NAME))
}

/// Whether the bot still has the conversation.
pub fn has_thread(bot: &Bot, thread_id: &str) -> bool {
    bot.conversations.iter().any(|c| c.thread_id == thread_id)
}

/// Whose messages a limit counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Source {
    Session(u32),
    Run(u32),
    /// Settings → MausBot's Send test message.
    Test,
}

/// The rate limits: per source, a gap between messages and a count per hour; and a count per hour
/// for everything together.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub min_gap_ms: i64,
    pub per_source_hour: usize,
    pub all_hour: usize,
}

pub const LIMITS: Limits = Limits { min_gap_ms: 30_000, per_source_hour: 10, all_hour: 30 };
const HOUR_MS: i64 = 3_600_000;

/// Messages sent in the last hour, by source and in all.
#[derive(Debug, Default)]
pub struct Limiter {
    by_source: HashMap<Source, VecDeque<i64>>,
    all: VecDeque<i64>,
}

fn prune(q: &mut VecDeque<i64>, now: i64) {
    while q.front().is_some_and(|t| now - t >= HOUR_MS) {
        q.pop_front();
    }
}

fn wait_words(ms: i64) -> String {
    let secs = (ms + 999) / 1000;
    if secs < 90 { format!("{secs} s") } else { format!("{} min", (secs + 59) / 60) }
}

impl Limiter {
    /// `Err` with the reason, in words, when `source` may not send now.
    pub fn check(&mut self, source: Source, limits: Limits, now: i64) -> Result<(), String> {
        prune(&mut self.all, now);
        if self.all.len() >= limits.all_hour {
            let wait = self.all.front().map_or(0, |t| t + HOUR_MS - now);
            return Err(format!("Chorus has sent {} messages to MausBot in the last hour, its limit. Try again in {}.", limits.all_hour, wait_words(wait)));
        }
        let q = self.by_source.entry(source).or_default();
        prune(q, now);
        if let Some(last) = q.back()
            && now - last < limits.min_gap_ms
        {
            return Err(format!("This sent a message {} ago. Wait {} before the next.", wait_words(now - last), wait_words(last + limits.min_gap_ms - now)));
        }
        if q.len() >= limits.per_source_hour {
            let wait = q.front().map_or(0, |t| t + HOUR_MS - now);
            return Err(format!("This has sent {} messages in the last hour, its limit. Try again in {}.", limits.per_source_hour, wait_words(wait)));
        }
        Ok(())
    }

    pub fn record(&mut self, source: Source, now: i64) {
        self.by_source.entry(source).or_default().push_back(now);
        self.all.push_back(now);
    }
}

/// Automatic notices waiting for their source to settle. A newer one for the same source replaces
/// the older; the source working again cancels it.
#[derive(Debug)]
pub struct Debounce<T> {
    pending: HashMap<Source, (T, i64)>,
}

impl<T> Default for Debounce<T> {
    fn default() -> Self {
        Self { pending: HashMap::new() }
    }
}

impl<T> Debounce<T> {
    pub fn queue(&mut self, source: Source, notice: T, now: i64, settle_ms: i64) {
        self.pending.insert(source, (notice, now + settle_ms));
    }

    pub fn cancel(&mut self, source: Source) {
        self.pending.remove(&source);
    }

    /// The notices due at `now`, taken out.
    pub fn due(&mut self, now: i64) -> Vec<(Source, T)> {
        let ready: Vec<Source> = self.pending.iter().filter(|(_, (_, at))| *at <= now).map(|(s, _)| *s).collect();
        ready.into_iter().filter_map(|s| self.pending.remove(&s).map(|(n, _)| (s, n))).collect()
    }

    /// When the next one is due.
    pub fn next_at(&self) -> Option<i64> {
        self.pending.values().map(|(_, at)| *at).min()
    }
}

/// The tool, as `tools/list` describes it.
pub fn tool_list() -> Value {
    json!([{
        "name": TOOL_NAME,
        "description": "Send Matthew a short message through OpenMausBot, which reaches his phone wherever he is. Use it when he'd want to know now: something he's waiting on is ready, finished or failed, or you need him. Don't use it for routine progress. Give only the message: Chorus adds the project, session and branch. Messages are rate-limited per session; the result says whether it was sent, or why not.",
        "inputSchema": {
            "type": "object",
            "properties": {"message": {"type": "string", "description": "What to tell him, in a sentence or two (under 1000 characters)."}},
            "required": ["message"]
        }
    }])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::maus::Conversation;

    fn origin() -> Origin {
        Origin { project: Some("Atlas".into()), session: Some("Fix login".into()), branch: Some("main".into()) }
    }

    #[test]
    fn the_prefix_names_project_session_and_branch_and_skips_what_is_missing() {
        assert_eq!(prefix(&origin()), "[Chorus] Atlas · Fix login · main");
        assert_eq!(prefix(&Origin { project: None, ..origin() }), "[Chorus] Fix login · main");
        assert_eq!(prefix(&Origin::default()), "[Chorus]");
        assert_eq!(prefix(&Origin { session: Some("a\nb".into()), ..origin() }), "[Chorus] Atlas · a b · main", "one line");
    }

    #[test]
    fn an_ad_hoc_message_is_checked_and_prefixed() {
        assert_eq!(ad_hoc(&origin(), "  Preview is up \u{7}\n at x  ").unwrap(), "[Chorus] Atlas · Fix login · main\nPreview is up \n at x");
        assert!(ad_hoc(&origin(), "  ").unwrap_err().contains("required"));
        assert!(ad_hoc(&origin(), &"x".repeat(1001)).unwrap_err().contains("1000"));
    }

    #[test]
    fn the_whole_message_fits_one_kilobyte_and_ends_with_the_echo() {
        let (kept, full) = finish("short");
        assert_eq!(kept, "short");
        assert_eq!(full, format!("short\n\n{ECHO}"));
        // Multi-byte characters right at the cut.
        let long = "é".repeat(800);
        let (kept, full) = finish(&long);
        assert!(full.len() <= MAX_BYTES, "{}", full.len());
        assert!(kept.ends_with('…') && full.ends_with(ECHO));
        let exact = "a".repeat(MAX_BYTES - ECHO.len() - 2);
        assert_eq!(finish(&exact).1.len(), MAX_BYTES);
        assert!(!finish(&exact).0.ends_with('…'));
        assert!(finish(&format!("{exact}b")).0.ends_with('…'));
    }

    #[test]
    fn a_notice_says_kind_what_branch_and_tests() {
        let f = NoticeFacts { what: Some("Deployed the preview".into()), tests: Some("212 passed".into()) };
        assert_eq!(notice(&origin(), Kind::Done, &f), "[Chorus] Atlas · Fix login · done\nDeployed the preview\nBranch: main\nTests: 212 passed");
        let bare = notice(&Origin { branch: None, ..origin() }, Kind::NeedsYou, &NoticeFacts::default());
        assert_eq!(bare, "[Chorus] Atlas · Fix login · needs you");
    }

    #[test]
    fn turn_ends_map_to_kinds() {
        assert_eq!(Kind::of_turn_end(Activity::Ready), Some(Kind::Done));
        assert_eq!(Kind::of_turn_end(Activity::Failed), Some(Kind::Failed));
        assert_eq!(Kind::of_turn_end(Activity::NeedsAnswer), Some(Kind::NeedsYou));
        assert_eq!(Kind::of_turn_end(Activity::AwaitingApproval), Some(Kind::NeedsYou));
        assert_eq!(Kind::of_turn_end(Activity::Working), None);
        assert_eq!(Kind::of_turn_end(Activity::Unknown), None);
    }

    #[test]
    fn what_finished_prefers_the_reply_then_the_plan() {
        let plan = vec![("Write tests".to_owned(), "completed".to_owned()), ("Deploy".to_owned(), "in-progress".to_owned())];
        assert_eq!(what_finished(Some("\n  All green.\nMore"), &plan).as_deref(), Some("All green."));
        assert_eq!(what_finished(None, &plan).as_deref(), Some("Deploy"));
        assert_eq!(what_finished(Some("  "), &plan[..1]).as_deref(), Some("Write tests"));
        assert_eq!(what_finished(None, &[]), None);
    }

    #[test]
    fn checks_become_a_tests_line() {
        assert_eq!(tests_line(&[]), None);
        assert_eq!(tests_line(&[("test".into(), true), ("lint".into(), true)]).as_deref(), Some("all 2 checks passed"));
        assert_eq!(tests_line(&[("test".into(), false), ("lint".into(), true)]).as_deref(), Some("1 of 2 checks failed: test"));
    }

    #[test]
    fn send_ids_fit_mausbots_rule() {
        let id = send_id(42, "00ff11ee22dd33cc44bb");
        assert_eq!(id, "chorus-update-42-00ff11ee22dd33cc");
        assert!((16..=80).contains(&id.len()));
    }

    fn bot(id: &str, name: &str, threads: &[&str]) -> Bot {
        Bot {
            id: id.into(),
            name: name.into(),
            title: None,
            cwd: None,
            busy: false,
            conversations: threads.iter().map(|t| Conversation { thread_id: (*t).into(), title: None, cwd: None, busy: false, updated_at_ms: None }).collect(),
        }
    }

    #[test]
    fn the_bot_is_the_chosen_one_else_jammie() {
        let bots = vec![bot("a", "Suzy", &[]), bot("b", " jammie ", &["t1"])];
        assert_eq!(pick_bot(&bots, None).map(|b| b.id.as_str()), Some("b"));
        assert_eq!(pick_bot(&bots, Some("a")).map(|b| b.id.as_str()), Some("a"));
        assert_eq!(pick_bot(&bots, Some("gone")), None, "a chosen bot that's gone isn't swapped silently");
        assert_eq!(pick_bot(&bots[..1], None), None);
        assert!(has_thread(&bots[1], "t1") && !has_thread(&bots[1], "t2"));
    }

    #[test]
    fn the_limiter_keeps_a_gap_and_hourly_counts() {
        let mut l = Limiter::default();
        let s = Source::Session(1);
        assert!(l.check(s, LIMITS, 0).is_ok());
        l.record(s, 0);
        let e = l.check(s, LIMITS, 10_000).unwrap_err();
        assert!(e.contains("Wait 20 s"), "{e}");
        assert!(l.check(Source::Session(2), LIMITS, 10_000).is_ok(), "another session has its own gap");
        for i in 1..10 {
            l.record(s, i * 30_000);
        }
        let e = l.check(s, LIMITS, 10 * 30_000).unwrap_err();
        assert!(e.contains("10 messages in the last hour"), "{e}");
        assert!(l.check(s, LIMITS, HOUR_MS + 1).is_ok(), "the oldest has aged out");
        // Everything together.
        let mut l = Limiter::default();
        for i in 0..30 {
            l.record(Source::Run(i), 0);
        }
        assert!(l.check(Source::Session(9), LIMITS, 1000).unwrap_err().contains("30 messages"));
    }

    #[test]
    fn the_debounce_keeps_the_newest_and_cancels_on_work() {
        let mut d = Debounce::default();
        d.queue(Source::Session(1), "first", 0, SETTLE_MS);
        d.queue(Source::Session(1), "second", 5_000, SETTLE_MS);
        d.queue(Source::Session(2), "other", 1_000, SETTLE_MS);
        assert_eq!(d.next_at(), Some(11_000));
        assert!(d.due(10_999).is_empty());
        assert_eq!(d.due(11_000), vec![(Source::Session(2), "other")]);
        assert!(d.due(14_999).is_empty(), "the second notice restarted the wait");
        assert_eq!(d.due(15_000), vec![(Source::Session(1), "second")]);
        d.queue(Source::Session(3), "x", 0, SETTLE_MS);
        d.cancel(Source::Session(3));
        assert!(d.due(i64::MAX).is_empty() && d.next_at().is_none());
    }

    #[test]
    fn the_tool_takes_one_message() {
        let t = tool_list();
        assert_eq!(t[0]["name"], TOOL_NAME);
        assert_eq!(t[0]["inputSchema"]["required"][0], "message");
        assert!(CLAUDE_TOOL_NAME.ends_with(TOOL_NAME));
    }
}
