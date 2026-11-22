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
/// every test database starts at 1: these tests run one at a time, each with the maps cleared.
static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn serial() -> std::sync::MutexGuard<'static, ()> {
    let one = SERIAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    super::forget_all();
    one
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
    assert_eq!((v.exists, v.problem.is_none(), v.entries.len()), (true, true, 1), "{:?}", v.problem);
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

/// A commit dated `days_ago` days back (author and committer), with `file` changed.
fn commit_dated(dir: &Path, file: &str, message: &str, days_ago: i64) {
    std::fs::write(dir.join(file), format!("{message}\n")).unwrap();
    git(dir, &["add", file]);
    let when = format!("@{} +0000", (store::now_ms() / 1000) - days_ago * 86_400);
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["commit", "-q", "-m", message])
        .env("GIT_AUTHOR_DATE", &when)
        .env("GIT_COMMITTER_DATE", &when)
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
}

/// The commit a branch on the card points at, as the card shows it.
fn tip(r: &Repo, project: u32, branch: &str) -> String {
    super::branches::report(&r.manager, project).unwrap().branches.into_iter().find(|b| b.name == branch).expect("listed").tip
}

#[test]
fn stale_branches_are_flagged_once_kept_and_deleted_with_care() {
    use super::branches;
    let _one = serial();
    let r = repo("branches");
    let work = r.dir.join("work");
    commit_dated(&work, "a.txt", "old base", 50);
    git(&work, &["branch", "old-kept"]);
    commit_dated(&work, "b.txt", "older base", 45);
    git(&work, &["branch", "old-merged"]);
    git(&work, &["checkout", "-q", "-b", "old-open"]);
    commit_dated(&work, "c.txt", "open one", 60);
    commit_dated(&work, "d.txt", "open two", 60);
    git(&work, &["checkout", "-q", "main"]);
    commit_dated(&work, "e.txt", "main moves on", 0);
    git(&work, &["branch", "fresh"]);

    let report = branches::report(&r.manager, r.project).unwrap();
    assert_eq!(report.base.as_deref(), Some("main"));
    let flagged: Vec<(&str, bool, Option<u32>)> = report.branches.iter().filter(|b| b.stale).map(|b| (b.name.as_str(), b.merged, b.ahead)).collect();
    assert_eq!(flagged, [("old-open", false, Some(2)), ("old-kept", true, Some(0)), ("old-merged", true, Some(0))], "oldest first; fresh and main never");

    // Flagged once a spell, even after the notices themselves are pruned.
    let now = store::now_ms();
    let due = branches::due(&r.manager, r.project, "Atlas", now).unwrap();
    assert_eq!(due.len(), 3);
    for (branch, notice) in &due {
        store::handoff::set_flagged(r.manager.store(), r.project, branch, &notice.dedupe_key).unwrap();
    }
    assert!(branches::due(&r.manager, r.project, "Atlas", now).unwrap().is_empty());
    r.manager.store().with(|c| c.execute("DELETE FROM notifications", [])).unwrap();
    assert!(branches::due(&r.manager, r.project, "Atlas", now).unwrap().is_empty(), "pruned notices aren't raised again");

    // Keep hides one for 30 days; Delete won't touch what the card doesn't list.
    let kept_tip = tip(&r, r.project, "old-kept");
    branches::keep(&r.manager, r.project, "old-kept").unwrap();
    let names: Vec<String> = branches::report(&r.manager, r.project).unwrap().branches.into_iter().map(|b| b.name).collect();
    assert_eq!(names, ["old-open", "old-merged"], "kept for 30 days");
    let refused = branches::delete(&r.manager, r.project, "old-kept", Some(&kept_tip), false).unwrap_err();
    assert!(refused.contains("only deletes the branches it lists"), "{refused}");
    // When the Keep runs out, the branch is flagged again, once.
    r.manager.store().with(|c| c.execute("UPDATE branch_snoozes SET until_at = ?1", [now - 1])).unwrap();
    let again = branches::due(&r.manager, r.project, "Atlas", now).unwrap();
    assert_eq!(again.len(), 1);
    assert_eq!(again[0].0, "old-kept");
    assert!(again[0].1.dedupe_key.ends_with(&format!(":kept-{}", now - 1)), "{}", again[0].1.dedupe_key);

    branches::delete(&r.manager, r.project, "old-merged", None, false).unwrap();
    assert_eq!(git(&work, &["branch", "--list", "old-merged"]).trim(), "");

    // Unmerged commits: asked about first, and only the commits the user was shown.
    let shown = tip(&r, r.project, "old-open");
    let refused = branches::delete(&r.manager, r.project, "old-open", Some(&shown), false).unwrap_err();
    assert!(refused.contains("2 commits") && refused.contains("for good"), "{refused}");
    assert!(branches::delete(&r.manager, r.project, "old-open", None, true).unwrap_err().contains("aren't merged"), "no tip, no unmerged delete");
    git(&work, &["checkout", "-q", "old-open"]);
    commit_dated(&work, "f.txt", "open three", 60);
    git(&work, &["checkout", "-q", "main"]);
    let refused = branches::delete(&r.manager, r.project, "old-open", Some(&shown), true).unwrap_err();
    assert!(refused.contains("has changed since"), "{refused}");
    assert_eq!(git(&work, &["branch", "--list", "old-open"]).trim(), "old-open", "a commit the user wasn't shown keeps it");
    let report = branches::report(&r.manager, r.project).unwrap();
    assert_eq!(report.branches.iter().find(|b| b.name == "old-open").unwrap().ahead, Some(3));
    branches::delete(&r.manager, r.project, "old-open", Some(&tip(&r, r.project, "old-open")), true).unwrap();
    assert_eq!(git(&work, &["branch", "--list", "old-open"]).trim(), "");

    assert!(branches::delete(&r.manager, r.project, "main", None, true).unwrap_err().contains("default branch"));
    assert!(branches::delete(&r.manager, r.project, "-D", None, true).unwrap_err().contains("isn't a branch name"));
    assert!(branches::delete(&r.manager, r.project, "fresh", None, true).unwrap_err().contains("only deletes the branches it lists"));
    let _ = std::fs::remove_dir_all(&r.dir);
}

#[test]
fn a_maus_branch_goes_with_its_worktree_only_when_nothing_there_would_be_lost() {
    use super::{baton, branches};
    let _one = serial();
    let r = repo("mausdelete");
    baton::start(&r.manager, r.project, None, "Tidy").unwrap();
    let a = view(&r.manager, r.project).unwrap().active.unwrap();
    assert!(branches::report(&r.manager, r.project).unwrap().branches.iter().all(|b| b.name != a.branch), "the banner shows it");
    assert!(branches::delete(&r.manager, r.project, &a.branch, None, true).unwrap_err().contains("using"), "an active hand-off is in use");
    baton::take_back(&r.manager, r.project).unwrap();
    let report = branches::report(&r.manager, r.project).unwrap();
    let maus = report.branches.iter().find(|b| b.name == a.branch).expect("listed");
    assert!(maus.maus && maus.merged && maus.worktree.is_some());
    let wt = PathBuf::from(&a.worktree);
    assert!(wt.join(".chorus").join("handoff.md").is_file(), "MausBot's copy of the note is there, and doesn't count");

    std::fs::write(wt.join("scratch.txt"), "dirty\n").unwrap();
    let refused = branches::delete(&r.manager, r.project, &a.branch, None, false).unwrap_err();
    assert!(refused.contains("would lose (scratch.txt)"), "{refused}");
    std::fs::remove_file(wt.join("scratch.txt")).unwrap();
    // An ignored file too: `git worktree remove` would take it without a word.
    let exclude = r.dir.join("work").join(".git").join("info").join("exclude");
    let mut lines = std::fs::read_to_string(&exclude).unwrap();
    lines.push_str("\n.env\n");
    std::fs::write(&exclude, lines).unwrap();
    std::fs::write(wt.join(".env"), "TOKEN=x\n").unwrap();
    let refused = branches::delete(&r.manager, r.project, &a.branch, None, false).unwrap_err();
    assert!(refused.contains("would lose (.env)"), "{refused}");
    assert!(wt.join(".env").is_file());
    std::fs::remove_file(wt.join(".env")).unwrap();

    branches::delete(&r.manager, r.project, &a.branch, None, false).unwrap();
    assert!(!wt.exists(), "the worktree goes first");
    assert_eq!(git(&r.dir.join("work"), &["branch", "--list", &a.branch]).trim(), "");
    assert!(view(&r.manager, r.project).unwrap().returned.is_none(), "its return card is put away");
    let _ = std::fs::remove_dir_all(&r.dir);
}

#[test]
fn branches_in_use_anywhere_are_left_alone() {
    use super::{baton, branches};
    let _one = serial();
    let r = repo("inuse");
    let work = r.dir.join("work");
    let store = r.manager.store();
    // A second project on the same repository, in a folder inside it.
    std::fs::create_dir_all(work.join("app")).unwrap();
    let app_folder = work.join("app").display().to_string();
    let other = projects::insert(store, &NewProject { name: "Atlas app", folder: Some(&app_folder), colour: "#336699" }).unwrap();

    // Another project's active hand-off.
    baton::start(&r.manager, r.project, None, "Tidy").unwrap();
    let a = view(&r.manager, r.project).unwrap().active.unwrap();
    assert!(branches::report(&r.manager, other).unwrap().branches.iter().all(|b| b.name != a.branch));
    let refused = branches::delete(&r.manager, other, &a.branch, None, true).unwrap_err();
    assert!(refused.contains("using"), "{refused}");
    baton::take_back(&r.manager, r.project).unwrap();

    // A loop run waiting weeks for its milestone review.
    commit_dated(&work, "l.txt", "loop work", 40);
    git(&work, &["branch", "run-1"]);
    commit_dated(&work, "m.txt", "main moves on", 0);
    let now = store::now_ms();
    store
        .with(|c| {
            c.execute(
                "INSERT INTO loop_runs (name, repo_root, base_branch, run_branch, config, state, created_at, updated_at)
                 VALUES ('run', 'x', 'main', 'run-1', '{}', 'waiting', ?1, ?1)",
                [now],
            )
        })
        .unwrap();
    assert!(branches::report(&r.manager, r.project).unwrap().branches.iter().all(|b| b.name != "run-1"));
    assert!(branches::delete(&r.manager, r.project, "run-1", None, false).unwrap_err().contains("using"));
    store.with(|c| c.execute("UPDATE loop_runs SET state = 'done'", [])).unwrap();
    assert!(branches::report(&r.manager, r.project).unwrap().branches.iter().any(|b| b.name == "run-1" && b.stale));

    // A worktree that is a project's folder: its branch is that project's own.
    let side = r.dir.join(".chorus2").join("work").join("side");
    git(&work, &["worktree", "add", "-q", "-b", "side", &side.display().to_string(), "run-1"]);
    let side_folder = side.display().to_string();
    let side_project = projects::insert(store, &NewProject { name: "Side", folder: Some(&side_folder), colour: "#336699" }).unwrap();
    assert!(branches::report(&r.manager, r.project).unwrap().branches.iter().all(|b| b.name != "side"));
    for project in [r.project, side_project] {
        let refused = branches::delete(&r.manager, project, "side", None, true).unwrap_err();
        assert!(refused.contains("a project's folder"), "{refused}");
    }
    assert!(side.is_dir());
    let _ = std::fs::remove_dir_all(&r.dir);
}

/// D135: Chorus updates against a stand-in for MausBot.
mod updates_tests {
    use std::sync::Mutex;

    use chorus_core::maus::{Bot, Conversation};
    use chorus_core::maus_updates::{ECHO, Kind, MAX_BYTES, Source};

    use super::{repo, serial};
    use crate::handoff::maus_client::Failure;
    use crate::handoff::updates::{self, MausApi, Pending, Whence};
    use crate::sessions::SessionManager;
    use crate::store::maus_updates as log;

    /// A key of MausBot's shape, made here so the secret scan has nothing to find.
    fn token() -> String {
        format!("omb_sess_{}", "Ab9_-".repeat(9))
    }

    struct Fake {
        paired: bool,
        running: bool,
        bots: Mutex<Vec<Bot>>,
        sent: Mutex<Vec<(String, String, String)>>,
        made: Mutex<u32>,
    }

    impl Fake {
        fn new() -> Self {
            let jammie = Bot { id: "jammie-1".into(), name: "Jammie".into(), title: None, cwd: None, busy: false, conversations: Vec::new() };
            let other = Bot { id: "suzy-1".into(), name: "Suzy".into(), ..jammie.clone() };
            Self { paired: true, running: true, bots: Mutex::new(vec![other, jammie]), sent: Mutex::new(Vec::new()), made: Mutex::new(0) }
        }
        fn sent(&self) -> Vec<(String, String, String)> {
            self.sent.lock().unwrap().clone()
        }
        fn drop_threads(&self) {
            for b in self.bots.lock().unwrap().iter_mut() {
                b.conversations.clear();
            }
        }
    }

    impl MausApi for Fake {
        fn token(&self, _: &SessionManager) -> Result<Option<String>, String> {
            Ok(self.paired.then(token))
        }
        fn bots(&self) -> Result<Vec<Bot>, Failure> {
            if self.running { Ok(self.bots.lock().unwrap().clone()) } else { Err(Failure::NotRunning) }
        }
        fn create(&self, token: &str, bot_id: &str, title: &str) -> Result<String, Failure> {
            assert_eq!(token, self::token());
            assert_eq!(title, "Chorus updates");
            let mut made = self.made.lock().unwrap();
            *made += 1;
            let thread = format!("thread-{made}");
            let mut bots = self.bots.lock().unwrap();
            let bot = bots.iter_mut().find(|b| b.id == bot_id).unwrap();
            bot.conversations.push(Conversation { thread_id: thread.clone(), title: Some(title.into()), cwd: None, busy: false, updated_at_ms: None });
            Ok(thread)
        }
        fn send(&self, token: &str, bot_id: &str, thread_id: &str, text: &str, send_id: &str) -> Result<(), Failure> {
            assert_eq!(token, self::token());
            assert!(send_id.starts_with("chorus-update-") && (16..=80).contains(&send_id.len()));
            let listed = self.bots.lock().unwrap().iter().any(|b| b.id == bot_id && b.conversations.iter().any(|c| c.thread_id == thread_id));
            if !listed {
                return Err(Failure::Refused("no such task".into()));
            }
            self.sent.lock().unwrap().push((bot_id.into(), thread_id.into(), text.into()));
            Ok(())
        }
    }

    fn adhoc(session: u32) -> Whence {
        Whence { source: Source::Session(session), session_id: Some(session), run_id: None }
    }

    /// Another source, with no session row behind it.
    fn other(n: u32) -> Whence {
        Whence { source: Source::Session(n), session_id: None, run_id: None }
    }

    fn rows(m: &SessionManager) -> Vec<log::UpdateRow> {
        log::recent(m.store(), 50).unwrap()
    }

    #[test]
    fn a_message_goes_to_jammie_in_one_conversation_with_the_echo_and_under_one_kilobyte() {
        let _one = serial();
        let r = repo("updates-send");
        let fake = Fake::new();
        let long = format!("[Chorus] Atlas \u{b7} Vince \u{b7} main\n{}", "\u{e9}".repeat(900));
        let sent = updates::deliver(&r.manager, &fake, adhoc(r.session), "ad-hoc", &long).unwrap();
        assert_eq!(sent.bot_name, "Jammie");
        let first = fake.sent();
        assert_eq!(first.len(), 1);
        let (bot, thread, text) = &first[0];
        assert_eq!((bot.as_str(), thread.as_str()), ("jammie-1", "thread-1"));
        assert!(text.starts_with("[Chorus] Atlas \u{b7} Vince \u{b7} main\n") && text.ends_with(ECHO));
        assert!(text.len() <= MAX_BYTES, "{}", text.len());
        // A second message, from elsewhere, reuses the conversation.
        updates::deliver(&r.manager, &fake, Whence { source: Source::Run(4), session_id: None, run_id: Some(4) }, "done", "[Chorus] run \u{b7} done").unwrap();
        assert_eq!(*fake.made.lock().unwrap(), 1);
        assert_eq!(fake.sent()[1].1, "thread-1");
        let logged = rows(&r.manager);
        assert_eq!(logged.iter().map(|l| l.outcome.as_str()).collect::<Vec<_>>(), ["sent", "sent"]);
        assert!(!logged[1].text.contains(ECHO), "the log keeps the message, not the instruction");
        let _ = std::fs::remove_dir_all(&r.dir);
    }

    #[test]
    fn a_lost_conversation_is_made_again() {
        let _one = serial();
        let r = repo("updates-lost");
        let fake = Fake::new();
        updates::deliver(&r.manager, &fake, adhoc(r.session), "ad-hoc", "one").unwrap();
        fake.drop_threads();
        updates::deliver(&r.manager, &fake, other(91), "ad-hoc", "two").unwrap();
        assert_eq!(*fake.made.lock().unwrap(), 2);
        assert_eq!(fake.sent()[1].1, "thread-2");
        assert_eq!(updates::stored(&r.manager).thread_id.as_deref(), Some("thread-2"));
        let _ = std::fs::remove_dir_all(&r.dir);
    }

    #[test]
    fn without_a_key_or_with_mausbot_down_nothing_is_sent_and_the_reason_is_kept() {
        let _one = serial();
        let r = repo("updates-down");
        let unpaired = Fake { paired: false, ..Fake::new() };
        let e = updates::deliver(&r.manager, &unpaired, adhoc(r.session), "ad-hoc", "hello").unwrap_err();
        assert!(e.contains("isn't paired"), "{e}");
        let down = Fake { running: false, ..Fake::new() };
        let e = updates::deliver(&r.manager, &down, adhoc(r.session), "ad-hoc", "hello").unwrap_err();
        assert!(e.contains("isn't running"), "{e}");
        assert!(down.sent().is_empty() && unpaired.sent().is_empty());
        let logged = rows(&r.manager);
        assert!(logged.iter().all(|l| l.outcome == "skipped" && l.reason.is_some()));
        // Skips that never reached MausBot don't use up the session's allowance.
        updates::deliver(&r.manager, &Fake::new(), adhoc(r.session), "ad-hoc", "hello").unwrap();
        let _ = std::fs::remove_dir_all(&r.dir);
    }

    #[test]
    fn a_session_is_rate_limited_and_switching_off_stops_everything() {
        let _one = serial();
        let r = repo("updates-limit");
        let fake = Fake::new();
        updates::deliver(&r.manager, &fake, adhoc(r.session), "ad-hoc", "one").unwrap();
        let e = updates::deliver(&r.manager, &fake, adhoc(r.session), "ad-hoc", "two").unwrap_err();
        assert!(e.contains("Wait"), "{e}");
        assert_eq!(fake.sent().len(), 1);
        updates::change(&r.manager, &updates::UpdatesChange { enabled: Some(false), ..Default::default() }).unwrap();
        let e = updates::deliver(&r.manager, &fake, other(97), "ad-hoc", "three").unwrap_err();
        assert!(e.contains("switched off"), "{e}");
        assert_eq!(fake.sent().len(), 1);
        let _ = std::fs::remove_dir_all(&r.dir);
    }

    #[test]
    fn secrets_are_scrubbed_and_the_key_never_reaches_the_message_or_the_log() {
        let _one = serial();
        let r = repo("updates-scrub");
        let secret = "sk-live-ChorusTestSecret0123456789";
        crate::vault::add(r.manager.store(), "Some key", secret).unwrap();
        let fake = Fake::new();
        updates::deliver(&r.manager, &fake, adhoc(r.session), "ad-hoc", &format!("key {secret} and token {}", token())).unwrap();
        let (_, _, text) = fake.sent().remove(0);
        assert!(!text.contains(secret) && !text.contains(&token()), "{text}");
        let logged = rows(&r.manager).remove(0);
        assert!(!logged.text.contains(secret) && !logged.text.contains(&token()), "{}", logged.text);
        let _ = std::fs::remove_dir_all(&r.dir);
    }

    #[test]
    fn a_notice_names_the_project_session_kind_reply_and_branch() {
        let _one = serial();
        let r = repo("updates-notice");
        let (whence, kind, body) = updates::notice_body(&r.manager, Pending::Session { session: r.session, kind: Kind::Done, closed: false }).unwrap();
        assert_eq!((whence.session_id, kind), (Some(r.session), Kind::Done));
        assert_eq!(body, "[Chorus] Atlas \u{b7} Vince \u{b7} done\nThe store is built.\nBranch: main");
        let (_, _, closed) = updates::notice_body(&r.manager, Pending::Session { session: r.session, kind: Kind::Done, closed: true }).unwrap();
        assert!(closed.contains("\nSession closed. The store is built.\n"), "{closed}");
        let _ = std::fs::remove_dir_all(&r.dir);
    }

    #[test]
    fn a_session_that_isnt_running_is_refused_by_its_tool_server() {
        let _one = serial();
        let r = repo("updates-auth");
        let body = r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#;
        assert!(updates::serve(&r.manager, r.session, "anything", body).is_err());
        assert!(updates::serve(&r.manager, r.session, "", body).is_err());
        let _ = std::fs::remove_dir_all(&r.dir);
    }
}
