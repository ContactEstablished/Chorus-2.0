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

/// The hand-off module keeps problems in process-wide maps keyed by project and hand-off id, which
/// every test database starts at 1: these tests run one at a time.
static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn serial() -> std::sync::MutexGuard<'static, ()> {
    SERIAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

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
    let _one = serial();
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
    let _one = serial();
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
    let _one = serial();
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

fn worktree_note(path: &str) -> String {
    std::fs::read_to_string(note::note_path(Path::new(path))).unwrap()
}

#[test]
fn a_handoff_makes_its_own_branch_and_comes_back_with_the_bots_entry() {
    let _one = serial();
    use super::baton;
    let r = repo("baton");
    let work = r.dir.join("work");
    baton::start(&r.manager, r.project, None, "Add a line").unwrap();
    let v = view(&r.manager, r.project).unwrap();
    let a = v.active.clone().expect("active");
    assert!(a.branch.starts_with("maus/") && a.branch.ends_with("-add-a-line"), "{}", a.branch);
    assert!(a.worktree.contains(".chorus2") && Path::new(&a.worktree).is_dir());
    assert!(a.start_problem.as_deref().is_some_and(|p| p.contains("No bot chosen")));
    assert!(a.message.as_deref().is_some_and(|m| m.contains(&a.worktree) && m.contains(&a.branch)));
    assert_eq!(v.baton, chorus_core::handoff::Baton::Mausbot);
    assert!(v.entries[0].heading.ends_with("Chorus → MausBot"));
    let copy = worktree_note(&a.worktree);
    assert!(copy.contains("baton: mausbot") && copy.contains("Chorus → MausBot"));
    assert_eq!(git(&work, &["status", "--porcelain"]), "", "the project's checkout is untouched");
    assert!(baton::start(&r.manager, r.project, None, "").unwrap_err().contains("already with MausBot"));

    // MausBot works: a commit on its branch, its own entry, and the baton handed back.
    let wt = Path::new(&a.worktree);
    std::fs::write(wt.join("NOTES.md"), "a line\n").unwrap();
    git(wt, &["add", "NOTES.md"]);
    git(wt, &["commit", "-q", "-m", "Add a line to NOTES.md"]);
    let mine = copy.replacen("baton: mausbot", "baton: chorus", 1).replacen(
        "Add yours at the top; leave the others as they are.\n\n",
        "Add yours at the top; leave the others as they are.\n\n## 2026-11-18 16:40 · MausBot → Chorus\nDone: Added a line\nState: committed\nOpen: Nothing\nNext: Merge it\n\n",
        1,
    );
    std::fs::write(note::note_path(wt), mine).unwrap();
    assert_eq!(baton::watch(&r.manager, false), vec![r.project]);
    let v = view(&r.manager, r.project).unwrap();
    assert!(v.active.is_none());
    let back = v.returned.expect("returned");
    assert_eq!((back.returned_by.as_str(), back.from_git, back.merged, back.ahead), ("mausbot", false, false, 1));
    assert_eq!(back.commits.len(), 1);
    assert_eq!(back.commits[0].subject, "Add a line to NOTES.md");
    assert_eq!(back.files.iter().map(|f| (f.path.as_str(), f.added)).collect::<Vec<_>>(), [("NOTES.md", Some(1))]);
    assert_eq!(back.entries[0].done.as_deref(), Some("Added a line"));
    assert_eq!(v.baton, chorus_core::handoff::Baton::Chorus);
    let text = baton::continue_text(&r.manager, r.project).unwrap();
    assert!(text.contains(&back.branch) && text.contains("1 commit ahead") && text.contains("Done: Added a line"), "{text}");

    baton::dismiss(&r.manager, r.project).unwrap();
    let v = view(&r.manager, r.project).unwrap();
    assert!(v.returned.is_none());
    let project_note = std::fs::read_to_string(note::note_path(&work)).unwrap();
    assert!(project_note.contains("branch:\nworktree:\nbase:\n"), "{project_note}");
    assert_eq!(git(&work, &["branch", "--list", &back.branch]).trim().trim_start_matches(['*', '+', ' ']), back.branch, "the branch stays");
    let _ = std::fs::remove_dir_all(&r.dir);
}

#[test]
fn taking_it_back_writes_an_entry_from_git_and_refusals_say_why() {
    let _one = serial();
    use super::baton;
    let r = repo("takeback");
    baton::start(&r.manager, r.project, None, "").unwrap();
    let a = view(&r.manager, r.project).unwrap().active.unwrap();
    assert!(a.branch.ends_with("-handoff"));
    baton::take_back(&r.manager, r.project).unwrap();
    let back = view(&r.manager, r.project).unwrap().returned.unwrap();
    assert_eq!((back.returned_by.as_str(), back.from_git, back.ahead), ("chorus", true, 0));
    assert_eq!(back.entries[0].done.as_deref(), Some(&*format!("No commits on {}", back.branch)));
    assert!(baton::take_back(&r.manager, r.project).unwrap_err().contains("isn't with MausBot"));

    // A broken project note stops a hand-off before anything is made.
    let path = note::note_path(&r.dir.join("work"));
    std::fs::write(&path, "---\nbaton: chorus\n").unwrap();
    assert!(baton::start(&r.manager, r.project, None, "").unwrap_err().contains("Fix the hand-off note first"));
    let _ = std::fs::remove_dir_all(&r.dir);
}

#[test]
fn a_folder_that_isnt_a_repository_cant_be_handed_over() {
    let _one = serial();
    use super::baton;
    let dir = std::env::temp_dir().join(format!("chorus-handoff-plain-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("work")).unwrap();
    let manager = SessionManager::new(Store::open(&dir.join("chorus.db")).unwrap(), dir.clone(), Box::new(|_| {}));
    let folder = dir.join("work").display().to_string();
    let project = projects::insert(manager.store(), &NewProject { name: "Plain", folder: Some(&folder), colour: "#336699" }).unwrap();
    assert!(baton::start(&manager, project, None, "").unwrap_err().contains("isn't a git repository"));
    assert!(baton::start(&manager, project, Some("bad id!".into()), "").unwrap_err().contains("isn't a MausBot bot"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_new_handoff_puts_the_old_return_away_so_dismiss_never_brings_it_back() {
    let _one = serial();
    use super::baton;
    let r = repo("second");
    let a = baton::start(&r.manager, r.project, None, "first").unwrap();
    baton::take_back(&r.manager, r.project).unwrap();
    let b = baton::start(&r.manager, r.project, None, "second").unwrap();
    assert!(view(&r.manager, r.project).unwrap().returned.is_none(), "the first return is put away");
    baton::take_back(&r.manager, r.project).unwrap();
    assert_eq!(view(&r.manager, r.project).unwrap().returned.map(|x| x.id), Some(b));
    baton::dismiss(&r.manager, r.project).unwrap();
    assert!(view(&r.manager, r.project).unwrap().returned.is_none(), "{a} doesn't come back");
    let _ = std::fs::remove_dir_all(&r.dir);
}

#[test]
fn the_entries_survive_a_project_folder_that_went_away() {
    let _one = serial();
    use super::baton;
    let r = repo("gone");
    baton::start(&r.manager, r.project, None, "").unwrap();
    let a = view(&r.manager, r.project).unwrap().active.unwrap();
    let copy = worktree_note(&a.worktree);
    let mine = copy.replacen("baton: mausbot", "baton: chorus", 1).replacen(
        "Add yours at the top; leave the others as they are.\n\n",
        "Add yours at the top; leave the others as they are.\n\n## 2026-11-18 16:40 · MausBot → Chorus\nDone: Kept safe\n\n",
        1,
    );
    std::fs::write(note::note_path(Path::new(&a.worktree)), mine).unwrap();
    // The project now points at a folder that isn't there.
    r.manager.store().with(|c| c.execute("UPDATE projects SET folder = ?1 WHERE id = ?2", rusqlite::params![r.dir.join("gone").display().to_string(), r.project])).unwrap();
    assert_eq!(super::baton::watch(&r.manager, false), vec![r.project]);
    let v = view(&r.manager, r.project).unwrap();
    let back = v.returned.expect("returned");
    assert_eq!(back.entries[0].done.as_deref(), Some("Kept safe"), "kept on the hand-off's row");
    assert!(v.problem.as_deref().is_some_and(|p| p.contains("only on the return card")), "{:?}", v.problem);
    let _ = std::fs::remove_dir_all(&r.dir);
}

#[test]
fn a_branch_git_cant_read_gets_an_entry_that_says_so() {
    let _one = serial();
    use super::baton;
    let r = repo("unreadable");
    let id = baton::start(&r.manager, r.project, None, "").unwrap();
    r.manager.store().with(|c| c.execute("UPDATE handoffs SET base_sha = ?1 WHERE id = ?2", rusqlite::params!["0".repeat(40), id])).unwrap();
    baton::take_back(&r.manager, r.project).unwrap();
    let back = view(&r.manager, r.project).unwrap().returned.unwrap();
    assert!(back.entries[0].done.as_deref().is_some_and(|d| d.starts_with("Chorus couldn't read")), "{:?}", back.entries[0].done);
    assert!(back.problem.is_some(), "the card says git couldn't read it");
    assert!(baton::take_back(&r.manager, r.project).unwrap_err().contains("isn't with MausBot"));
    let _ = std::fs::remove_dir_all(&r.dir);
}
