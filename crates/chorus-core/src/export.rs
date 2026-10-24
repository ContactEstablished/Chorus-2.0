//! Transcript export (P7): a session's recorded history as Markdown. Everything rendered here
//! comes from the history tables, which are scrubbed when written (D7); the caller scrubs the
//! result again before it leaves Chorus.

pub struct ExportSession<'a> {
    pub name: &'a str,
    pub agent: &'a str,
    pub cwd: &'a str,
    /// Unix seconds.
    pub exported_at: i64,
}

pub struct ExportTool<'a> {
    pub tool: &'a str,
    pub target: Option<&'a str>,
    pub failed: bool,
    pub finished: bool,
}

pub struct ExportAsk<'a> {
    pub kind: &'a str,
    pub subject: &'a str,
    pub answer: Option<&'a str>,
}

pub struct ExportTurn<'a> {
    pub prompt: Option<&'a str>,
    pub source: Option<&'a str>,
    /// Unix seconds.
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub end_reason: Option<&'a str>,
    pub final_message: Option<&'a str>,
    pub tools: Vec<ExportTool<'a>>,
    pub asks: Vec<ExportAsk<'a>>,
}

/// "2026-10-24 08:15:02 UTC".
pub fn utc(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02} UTC", rem / 3600, rem / 60 % 60, rem % 60)
}

/// Text inside a fenced block: a fence longer than any backtick run in the text, so nothing in
/// it can close the block early.
fn fenced(text: &str) -> String {
    let longest = text.split(|c| c != '`').map(str::len).max().unwrap_or(0);
    let fence = "`".repeat(longest.max(2) + 1);
    format!("{fence}text\n{}\n{fence}\n", text.trim_end())
}

/// One line of inline text: line breaks become spaces, backticks are kept out of code spans.
fn inline(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn markdown(session: &ExportSession, turns: &[ExportTurn]) -> String {
    let mut out = format!(
        "# {}\n\n- Agent: {}\n- Folder: `{}`\n- Exported: {}\n- Turns: {}\n\nFrom Chorus's recorded history: prompts, final messages, tool calls (tool and target, not their output) and questions or approvals with their answers. Secrets are redacted.\n",
        inline(session.name),
        session.agent,
        session.cwd.replace('`', "'"),
        utc(session.exported_at),
        turns.len()
    );
    for (i, t) in turns.iter().enumerate() {
        out.push_str(&format!("\n## Turn {} · {}\n\n", i + 1, utc(t.started_at)));
        match t.prompt {
            Some(p) => {
                out.push_str(&format!("**Prompt** ({}):\n\n", t.source.unwrap_or("unknown source")));
                out.push_str(&fenced(p));
            }
            None => out.push_str("**Prompt:** not recorded\n"),
        }
        if !t.tools.is_empty() {
            out.push_str("\n**Tool calls:**\n\n");
            for tool in &t.tools {
                let outcome = if tool.failed { "failed" } else if tool.finished { "done" } else { "not finished" };
                match tool.target {
                    Some(target) => out.push_str(&format!("- {} `{}` ({outcome})\n", inline(tool.tool), inline(target).replace('`', "'"))),
                    None => out.push_str(&format!("- {} ({outcome})\n", inline(tool.tool))),
                }
            }
        }
        if !t.asks.is_empty() {
            out.push_str("\n**Questions and approvals:**\n\n");
            for ask in &t.asks {
                let answer = ask.answer.map_or_else(|| "no answer recorded".to_owned(), inline);
                out.push_str(&format!("- {}: {} → {answer}\n", ask.kind, inline(ask.subject)));
            }
        }
        match (t.ended_at, t.final_message) {
            (Some(end), Some(message)) => {
                out.push_str(&format!("\n**Final message** ({}, {}):\n\n", t.end_reason.unwrap_or("ended"), utc(end)));
                out.push_str(&fenced(message));
            }
            (Some(end), None) => out.push_str(&format!("\n**Ended** ({}, {}) with no final message.\n", t.end_reason.unwrap_or("ended"), utc(end))),
            (None, _) => out.push_str("\n**Not ended** when exported.\n"),
        }
    }
    out
}

/// A file name for the export: the session's name with anything Windows refuses replaced.
pub fn file_name(session_name: &str) -> String {
    let clean: String = session_name
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' || c == ' ' { c } else { '-' })
        .collect();
    let clean = clean.split_whitespace().collect::<Vec<_>>().join(" ");
    let clean: String = clean.trim_matches(['-', ' ', '.']).chars().take(60).collect();
    format!("{} transcript.md", if clean.is_empty() { "Session" } else { &clean })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utc_formats_known_instants() {
        assert_eq!(utc(0), "1970-01-01 00:00:00 UTC");
        assert_eq!(utc(951_782_400), "2000-02-29 00:00:00 UTC");
        assert_eq!(utc(1_792_844_158), "2026-10-24 12:15:58 UTC");
    }

    fn session() -> ExportSession<'static> {
        ExportSession { name: "Ada\nfix", agent: "Claude Code", cwd: "C:\\work", exported_at: 0 }
    }

    #[test]
    fn renders_prompts_tools_asks_and_final_messages() {
        let turns = [ExportTurn {
            prompt: Some("Create p7.txt"),
            source: Some("hook"),
            started_at: 60,
            ended_at: Some(120),
            end_reason: Some("done"),
            final_message: Some("Created it."),
            tools: vec![
                ExportTool { tool: "Write", target: Some("p7.txt"), failed: false, finished: true },
                ExportTool { tool: "Bash", target: None, failed: true, finished: true },
            ],
            asks: vec![ExportAsk { kind: "approval", subject: "Write p7.txt", answer: Some("allow") }],
        }];
        let md = markdown(&session(), &turns);
        assert!(md.starts_with("# Ada fix\n"));
        assert!(md.contains("**Prompt** (hook):\n\n```text\nCreate p7.txt\n```"));
        assert!(md.contains("- Write `p7.txt` (done)"));
        assert!(md.contains("- Bash (failed)"));
        assert!(md.contains("- approval: Write p7.txt → allow"));
        assert!(md.contains("**Final message** (done, 1970-01-01 00:02:00 UTC):\n\n```text\nCreated it.\n```"));
    }

    #[test]
    fn fences_outlast_backticks_in_the_text() {
        let turns = [ExportTurn {
            prompt: Some("use ```rust``` here"),
            source: None,
            started_at: 0,
            ended_at: None,
            end_reason: None,
            final_message: None,
            tools: vec![],
            asks: vec![],
        }];
        let md = markdown(&session(), &turns);
        assert!(md.contains("````text\nuse ```rust``` here\n````"));
        assert!(md.contains("(unknown source)"));
        assert!(md.contains("**Not ended** when exported."));
    }

    #[test]
    fn file_names_are_safe_on_windows() {
        assert_eq!(file_name("Ada: fix/bug?"), "Ada- fix-bug transcript.md");
        assert_eq!(file_name("  ..  "), "Session transcript.md");
        assert_eq!(file_name("P7 approval check"), "P7 approval check transcript.md");
    }
}
