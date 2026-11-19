//! The hand-off note against a real git repository: a refresh writes the note and its exclude
//! lines, git doesn't see it, a second refresh replaces its entry, and a broken note is left as it is.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use chorus_core::agent::{AgentKind, LaunchOptions};
use rusqlite::params;

use super::{note, refresh, triggers, view};
use crate::sessions::SessionManager;
use crate::store::projects::{self, NewProject};
use crate::store::sessions::{self as rows, NewSession};
use crate::store::{self, Store};

fn git(dir: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git").arg("-C").arg(dir).args(args).output().unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).into_owned()
}

struct Repo {
    dir: PathBuf,
    manager: Arc<SessionManager>,
    project: u32,
    session: u32,
}

fn repo(name: &str) -> Repo {
    let dir = std::env::temp_dir().join(format!("chorus-handoff-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let work = dir.join("work");
    std::fs::create_dir_all(&work).unwrap();
    git(&work, &["init", "-q", "-b", "main"]);
    git(&work, &["config", "user.email", "test@example.invalid"]);
    git(&work, &["config", "user.name", "Test"]);
    git(&work, &["commit", "-q", "--allow-empty", "-m", "first"]);
    let manager = SessionManager::new(Store::open(&dir.join("chorus.db")).unwrap(), dir.clone(), Box::new(|_| {}));
    let folder = work.display().to_string();
    let project = projects::insert(manager.store(), &NewProject { name: "Atlas", folder: Some(&folder), colour: "#336699" }).unwrap();
    let session = rows::insert(manager.store(), &NewSession {
        name: "Vince", agent: AgentKind::Claude, cwd: &folder, credential_id: None, credential_env: None, cols: 80, rows: 24,
        options: &LaunchOptions::default(), agent_session_id: None, provider_id: None, description: None,
        project_id: Some(project), worktree_id: None, routing: None,
    })
    .unwrap();
    manager
        .store()
        .with(|c| {
            c.execute("INSERT INTO prompts (session_id, source, text, created_at) VALUES (?1, 'hook', 'Build the store', 1)", params![session])?;
            let prompt = c.last_insert_rowid();
            c.execute(
                "INSERT INTO turns (session_id, prompt_id, started_at, ended_at, final_message) VALUES (?1, ?2, 1, ?3, 'The store is built.')",
                params![session, prompt, store::now_ms()],
            )
        })
        .unwrap();
    Repo { dir, manager, project, session }
}

#[test]
fn a_refresh_writes_an_ignored_note_and_replaces_its_own_entry() {
    let r = repo("write");
    let work = r.dir.join("work");
    assert_eq!(refresh::refresh(&r.manager, r.project, Some(r.session)).unwrap(), Some(note::IgnoreState::Ignored));
    let text = std::fs::read_to_string(note::note_path(&work)).unwrap();
    assert!(text.starts_with("---\nproject: Atlas\nbaton: chorus\n"), "{text}");
    assert!(text.contains("· Chorus · latest work\nDone: Last turn: Build the store\nState: branch main @ "), "{text}");
    assert!(text.contains("Session: Vince (Claude Code)\nLast reply ("), "{text}");
    assert_eq!(git(&work, &["status", "--porcelain"]), "", "git ignores the note");
    let exclude = std::fs::read_to_string(work.join(".git").join("info").join("exclude")).unwrap();
    assert!(exclude.contains("/.chorus/handoff.md\n") && !exclude.contains("/.chorus/\n"));

    refresh::refresh(&r.manager, r.project, None).unwrap();
    let again = std::fs::read_to_string(note::note_path(&work)).unwrap();
    assert_eq!(again.matches("latest work").count(), 1, "the entry is replaced, not stacked");
    let v = view(&r.manager, r.project).unwrap();
    assert_eq!((v.exists, v.problem.is_none(), v.entries.len()), (true, true, 1));
    assert_eq!(v.ignored, Some(note::IgnoreState::Ignored));
    let _ = std::fs::remove_dir_all(&r.dir);
}

#[test]
fn a_broken_note_is_left_as_it_is_and_says_why() {
    let r = repo("broken");
    let work = r.dir.join("work");
    let path = note::note_path(&work);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let broken = "---\nproject: Atlas\nbaton: chorus\n\n## mine\nDone: x\n";
    std::fs::write(&path, broken).unwrap();
    let e = refresh::refresh(&r.manager, r.project, Some(r.session)).unwrap_err();
    assert!(e.contains("left the note as it is") && e.contains("no closing line"), "{e}");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), broken, "not a byte changed");
    let v = view(&r.manager, r.project).unwrap();
    assert!(v.problem.as_deref().is_some_and(|p| p.contains("no closing line")), "{v:?}");
    let _ = std::fs::remove_dir_all(&r.dir);
}

#[test]
fn only_a_turn_end_or_a_close_refreshes_the_note() {
    // (working, closed)
    assert!(triggers(Some((true, false)), (false, false)), "a turn ended");
    assert!(triggers(Some((false, false)), (false, true)), "the session closed");
    assert!(triggers(Some((true, false)), (false, true)), "closed mid-turn");
    assert!(!triggers(None, (false, false)), "first seen: start-up summaries don't count");
    assert!(!triggers(None, (false, true)), "first seen closed");
    assert!(!triggers(Some((false, false)), (true, false)), "a turn started");
    assert!(!triggers(Some((false, false)), (false, false)), "nothing changed");
    assert!(!triggers(Some((false, true)), (false, true)), "already closed");
}
