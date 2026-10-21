//! The session engine against real `cmd.exe` processes: output reaches a view and the mirror
//! scrubbed, a planted key appears nowhere, exits and Restart are recorded, and a quit followed by
//! a restore relaunches what was running.

use super::*;
use chorus_core::scrub::{PLACEHOLDER, longest_shared_run};
use chorus_core::session::RunEnd;

struct Fixture {
    dir: PathBuf,
    manager: Arc<SessionManager>,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("chorus-sessions-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("work")).unwrap();
        Self { manager: Self::manager(&dir), dir }
    }

    fn manager(dir: &Path) -> Arc<SessionManager> {
        let store = Store::open(&dir.join("chorus.db")).unwrap();
        SessionManager::new(store, dir.to_path_buf(), Box::new(|_| {}))
    }

    fn shell(&self, credential: Option<CredentialUse>) -> u32 {
        let req = CreateSession {
            agent: AgentKind::Shell,
            cwd: self.dir.join("work").display().to_string(),
            name: None,
            credential,
            cols: 100,
            rows: 30,
            options: Default::default(),
            provider_id: None,
            description: None,
            project_id: None,
            tags: Vec::new(),
            workspace: Default::default(),
            routing: None,
        };
        let s = self.manager.create(req).unwrap();
        assert!(s.live, "shell should be running: {:?}", s.note);
        s.id
    }

    fn view(&self, id: u32) -> Arc<Mutex<Vec<u8>>> {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let sink = seen.clone();
        self.manager
            .attach(
                id,
                Box::new(move |b: &[u8]| {
                    sink.lock().unwrap().extend_from_slice(b);
                    true
                }),
            )
            .unwrap();
        seen
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        for s in self.manager.list().unwrap_or_default() {
            if let Some(live) = self.manager.live.lock().unwrap().get(&s.id).cloned() {
                self.manager.end_run_for(&live, ExitCause::UserClose);
            }
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn wait_for(what: &str, mut ok: impl FnMut() -> bool) {
    wait_for_or(what, &mut ok, String::new);
}

fn wait_for_or(what: &str, ok: &mut impl FnMut() -> bool, context: impl Fn() -> String) {
    let t0 = Instant::now();
    while !ok() {
        assert!(t0.elapsed() < Duration::from_secs(15), "timed out waiting for {what}; saw {:?}", context());
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Waits until the view has shown `needle` at least `times` times (a typed command is echoed,
/// so its output is the second occurrence).
fn wait_text(seen: &Arc<Mutex<Vec<u8>>>, needle: &str, times: usize) {
    wait_for_or(needle, &mut || text(seen).matches(needle).count() >= times, || text(seen));
}

fn text(seen: &Arc<Mutex<Vec<u8>>>) -> String {
    String::from_utf8_lossy(&seen.lock().unwrap()).into_owned()
}

#[test]
fn a_planted_key_is_redacted_in_the_view_the_mirror_and_the_database() {
    let f = Fixture::new("leak");
    // Built at run time so the repo holds no key-shaped string (the secret scan would flag it).
    let key = &format!("sk-or-v1-{}", "0123456789abcdef".repeat(4));
    let cred = vault::add(f.manager.store(), "test", key).unwrap();
    let id = f.shell(Some(CredentialUse { id: cred, env_var: "CHORUS_TEST_KEY".into() }));
    let seen = f.view(id);
    f.manager.write(id, "echo begin:%CHORUS_TEST_KEY%:end").unwrap();
    f.manager.write(id, "\r").unwrap();
    wait_text(&seen, ":end", 2);

    let shown = text(&seen);
    assert!(shown.contains(std::str::from_utf8(PLACEHOLDER).unwrap()), "placeholder missing: {shown}");
    assert_eq!(longest_shared_run(key.as_bytes(), shown.as_bytes(), 8), 0, "key leaked to the view");

    let mirror = std::fs::read(f.dir.join("scrollback").join(format!("{id}.log"))).unwrap();
    assert!(mirror.windows(PLACEHOLDER.len()).any(|w| w == PLACEHOLDER));
    assert_eq!(longest_shared_run(key.as_bytes(), &mirror, 8), 0, "key leaked to the scrollback file");

    for name in ["chorus.db", "chorus.db-wal"] {
        if let Ok(bytes) = std::fs::read(f.dir.join(name)) {
            assert_eq!(longest_shared_run(key.as_bytes(), &bytes, 8), 0, "key leaked to {name}");
        }
    }
}

#[test]
fn a_late_view_gets_the_history_then_live_output() {
    let f = Fixture::new("attach");
    let id = f.shell(None);
    f.manager.write(id, "echo first-line").unwrap();
    f.manager.write(id, "\r").unwrap();
    let early = f.view(id);
    wait_text(&early, "first-line", 2);

    let late = f.view(id);
    assert!(text(&late).contains("first-line"), "history not replayed");
    f.manager.write(id, "echo second-line").unwrap();
    f.manager.write(id, "\r").unwrap();
    wait_text(&late, "second-line", 2);
    std::thread::sleep(Duration::from_millis(500));
    // The command as typed, and its output: twice, never more.
    let shown = text(&late);
    assert_eq!(shown.matches("second-line").count(), 2, "live output repeated: {shown:?}");
}

#[test]
fn exiting_by_itself_is_recorded_and_restart_runs_again() {
    let f = Fixture::new("exit");
    let id = f.shell(None);
    let seen = f.view(id);
    f.manager.write(id, "exit 3").unwrap();
    f.manager.write(id, "\r").unwrap();
    wait_for("exit", || f.manager.summary(id).is_ok_and(|s| !s.live && s.status == SessionStatus::Exited));
    let s = f.manager.summary(id).unwrap();
    assert_eq!(s.last_exit_code, Some(3));
    wait_text(&seen, "exited with code 3", 1);

    let s = f.manager.restart(id).unwrap();
    assert!(s.live && s.status == SessionStatus::Running);
    let last = rows::last_run(f.manager.store(), id).unwrap().unwrap();
    assert!(last.ended_at.is_none());
}

#[test]
fn restart_and_close_are_refused_while_locked() {
    let f = Fixture::new("lock");
    let id = f.shell(None);
    f.manager.set_locked(id, true, None).unwrap();
    assert!(f.manager.close(id, false).unwrap_err().contains("locked"));
    assert!(f.manager.restart(id).unwrap_err().contains("locked"));
    crate::lock::set_pin(f.manager.store(), "4321").unwrap();
    assert!(f.manager.set_locked(id, false, Some("0000")).is_err());
    f.manager.set_locked(id, false, Some("4321")).unwrap();
    let report = f.manager.close(id, false).unwrap();
    assert!(report.killed >= 1);
    assert_eq!(rows::get(f.manager.store(), id).unwrap().unwrap().state, SessionState::Closed);
    assert!(!f.dir.join("scrollback").join(format!("{id}.log")).exists(), "mirror outlived Close");
    let last = rows::last_run(f.manager.store(), id).unwrap().unwrap();
    assert_eq!(last.end_reason, Some(RunEnd::Closed));
}

#[test]
fn a_quit_then_restore_relaunches_with_history() {
    let f = Fixture::new("restore");
    let id = f.shell(None);
    let seen = f.view(id);
    f.manager.write(id, "echo before-quit").unwrap();
    f.manager.write(id, "\r").unwrap();
    wait_text(&seen, "before-quit", 2);
    let old_run = rows::last_run(f.manager.store(), id).unwrap().unwrap().id;

    // Quit: record the runs, then the processes die with their jobs.
    f.manager.shutdown();
    if let Some(live) = f.manager.live.lock().unwrap().get(&id).cloned() {
        live.current_run().unwrap().end_processes();
    }
    let ended = rows::last_run(f.manager.store(), id).unwrap().unwrap();
    assert_eq!((ended.id, ended.end_reason), (old_run, Some(RunEnd::ChorusClosed)));

    let next = Fixture::manager(&f.dir);
    next.restore().unwrap();
    let s = next.summary(id).unwrap();
    assert!(s.live && s.status == SessionStatus::Running, "not relaunched: {:?}", s.note);
    let replay = Arc::new(Mutex::new(Vec::new()));
    let sink = replay.clone();
    next.attach(id, Box::new(move |b: &[u8]| {
        sink.lock().unwrap().extend_from_slice(b);
        true
    }))
    .unwrap();
    let shown = text(&replay);
    let old = shown.find("before-quit").expect("history kept");
    let line = shown.find("session relaunched").expect("boundary shown");
    assert!(old < line);
    if let Some(live) = next.live.lock().unwrap().get(&id).cloned() {
        next.end_run_for(&live, ExitCause::UserClose);
    }
}

#[test]
fn runs_left_open_by_a_crash_are_healed_at_restore() {
    let f = Fixture::new("crash");
    let id = f.shell(None);
    let run = rows::last_run(f.manager.store(), id).unwrap().unwrap().id;
    // A crash: no shutdown() record; the processes die with the job.
    if let Some(live) = f.manager.live.lock().unwrap().get(&id).cloned() {
        *live.current_run().unwrap().requested.lock().unwrap() = Some(ExitCause::ChorusQuitting);
        live.current_run().unwrap().end_processes();
        live.current_run().map(|r| r.wait_finished(FINISH_TIMEOUT));
    }
    // The watcher of this (still running) manager recorded it; simulate a crash that didn't.
    f.manager.store().with(|c| c.execute("UPDATE runs SET ended_at = NULL, end_reason = NULL WHERE id = ?1", [run])).unwrap();

    let next = Fixture::manager(&f.dir);
    next.restore().unwrap();
    let healed = next.store().with(|c| {
        c.query_row("SELECT end_reason FROM runs WHERE id = ?1", [run], |r| r.get::<_, String>(0))
    });
    assert_eq!(healed.unwrap(), "chorus-crashed");
    assert!(next.summary(id).unwrap().live);
    if let Some(live) = next.live.lock().unwrap().get(&id).cloned() {
        next.end_run_for(&live, ExitCause::UserClose);
    }
}

#[test]
fn bad_input_is_refused_at_the_boundary() {
    let f = Fixture::new("input");
    let mut req = CreateSession {
        agent: AgentKind::Shell,
        cwd: "relative\\path".into(),
        name: None,
        credential: None,
        cols: 100,
        rows: 30,
        options: Default::default(),
        provider_id: None,
        description: None,
        project_id: None,
        tags: Vec::new(),
        workspace: Default::default(),
        routing: None,
    };
    assert!(f.manager.create(req.clone()).unwrap_err().contains("full path"));
    req.cwd = f.dir.join("missing").display().to_string();
    assert!(f.manager.create(req.clone()).unwrap_err().contains("not a folder"));
    req.cwd = f.dir.join("work").display().to_string();
    req.cols = 0;
    assert!(f.manager.create(req.clone()).is_err());
    req.cols = 100;
    req.credential = Some(CredentialUse { id: 99, env_var: "OK_NAME".into() });
    assert!(f.manager.create(req.clone()).unwrap_err().contains("no longer exists"));
    req.credential = Some(CredentialUse { id: 99, env_var: "PATH".into() });
    assert!(f.manager.create(req).is_err());
    assert!(f.manager.write(12345, "x").is_err());
}

#[test]
fn a_preset_launch_names_and_notes_its_sessions() {
    let f = Fixture::new("launch");
    let session = CreateSession {
        agent: AgentKind::Shell,
        cwd: f.dir.join("work").display().to_string(),
        name: Some("Fix login".into()),
        credential: None,
        cols: 100,
        rows: 30,
        options: Default::default(),
        provider_id: None,
        description: Some("  Ticket 42  ".into()),
        project_id: None,
        tags: Vec::new(),
        workspace: Default::default(),
        routing: None,
    };
    // A shell can't build: Workbench and Pair say so, and nothing starts.
    let refused = f.manager.launch(LaunchRequest { preset: Preset::Workbench, session: session.clone(), count: None }).unwrap_err();
    assert!(refused.contains("other than Shell"), "{refused}");
    assert!(f.manager.list().unwrap().is_empty());
    let swarm = f.manager.presets(AgentKind::Codex).into_iter().find(|p| p.preset == Preset::Swarm).unwrap();
    assert!(swarm.reason.is_none() && swarm.slots.len() == launch_rules::SWARM_MIN as usize);

    let out = f.manager.launch(LaunchRequest { preset: Preset::Solo, session, count: None }).unwrap();
    assert_eq!(out.len(), 1);
    let s = out[0].session.clone().unwrap();
    assert_eq!((s.name.as_str(), s.description.as_deref()), ("Fix login", Some("Ticket 42")));

    let renamed = f.manager.rename(s.id, "Fix the login page").unwrap();
    assert_eq!(renamed.name, "Fix the login page");
    assert!(f.manager.rename(s.id, "  ").is_err());
    assert_eq!(f.manager.set_description(s.id, None).unwrap().description, None);
    assert!(f.manager.set_description(s.id, Some(&"x".repeat(501))).is_err());
}

/// Runs git for a test repository, with an identity so commits work anywhere.
fn git_in(dir: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.name=Chorus test", "-c", "user.email=test@example.invalid"])
        .args(args)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn a_folder_given_in_short_names_is_kept_in_full() {
    // GitHub's Windows runner has its temp folder at C:\Users\RUNNER~1\…, which git writes in full.
    let f = Fixture::new("short-names");
    let long = f.dir.join("a folder with a long name");
    std::fs::create_dir_all(&long).unwrap();
    let full = check_cwd(&long.display().to_string()).unwrap();
    assert!(!full.contains('~'), "{full}");
    // On a volume without short names this is the same spelling.
    let short = crate::agents::short_path(&long).unwrap().display().to_string();
    assert_eq!(check_cwd(&short).unwrap(), full);
}

#[test]
fn a_session_can_work_in_a_worktree_of_its_own() {
    let f = Fixture::new("worktree");
    let repo = f.dir.join("ledger");
    std::fs::create_dir_all(repo.join("web")).unwrap();
    std::fs::write(repo.join("web").join("index.txt"), "hello").unwrap();
    git_in(&repo, &["init", "-q", "-b", "main"]);
    git_in(&repo, &["add", "."]);
    git_in(&repo, &["commit", "-q", "-m", "first"]);
    let folder = repo.join("web").display().to_string();
    let request = CreateSession {
        agent: AgentKind::Shell,
        cwd: folder.clone(),
        name: None,
        credential: None,
        cols: 100,
        rows: 30,
        options: Default::default(),
        provider_id: None,
        description: None,
        project_id: None,
        tags: vec!["Spike".into(), "spike".into(), "review".into()],
        workspace: Workspace::Here,
        routing: None,
    };
    let first = f.manager.create(request.clone()).unwrap();
    assert_eq!(first.tags, ["spike", "review"]);
    assert_eq!(f.manager.known_tags().unwrap().iter().map(|t| t.tag.as_str()).collect::<Vec<_>>(), ["review", "spike"]);

    // A second session for that folder hears who's already there.
    let check = f.manager.check_folder(&folder);
    let git = check.git.unwrap();
    assert_eq!((git.branch.as_deref(), git.repo.as_str(), git.in_worktree), (Some("main"), "ledger", false));
    assert!(git.new_branch.unwrap().starts_with("chorus2/ledger/"));
    assert_eq!(check.sharing.iter().map(|s| s.id).collect::<Vec<_>>(), [first.id]);

    let second = f.manager.create(CreateSession { workspace: Workspace::NewWorktree, tags: Vec::new(), ..request.clone() }).unwrap();
    let wt = second.worktree.clone().unwrap();
    assert!(wt.branch.starts_with("chorus2/ledger/") && wt.base_branch.as_deref() == Some("main"), "{wt:?}");
    let beside = long_path(&f.dir.join(".chorus2").join("ledger").display().to_string());
    assert!(second.cwd.starts_with(&beside) && second.cwd.ends_with(r"\web"), "{}", second.cwd);
    assert!(second.live, "{:?}", second.note);
    let inside = f.manager.check_folder(&second.cwd);
    assert!(inside.git.unwrap().in_worktree);
    assert_eq!(inside.sharing.iter().map(|s| s.id).collect::<Vec<_>>(), [second.id], "the first session isn't in this tree");

    // Uncommitted work is never removed.
    let draft = Path::new(&wt.path).join("notes.txt");
    std::fs::write(&draft, "draft").unwrap();
    assert_eq!(f.manager.close_check(second.id).unwrap().worktree.unwrap().clean, Some(false));
    std::fs::remove_file(&draft).unwrap();
    let check = f.manager.close_check(second.id).unwrap().worktree.unwrap();
    assert_eq!((check.clean, check.others), (Some(true), 0));
    let note = f.manager.close(second.id, true).unwrap().worktree_note.unwrap();
    assert!(note.contains("Removed"), "{note}");
    assert!(!Path::new(&wt.path).exists());
    assert!(f.manager.summary(second.id).unwrap().worktree.unwrap().removed);
    assert!(git_in(&repo, &["branch", "--list", &wt.branch]).contains(&wt.branch), "the branch is kept");
    assert_eq!(f.manager.recent_folders(), [long_path(&folder)]);

    // A worktree folder deleted outside Chorus: Close clears git's record of it.
    let third = f.manager.create(CreateSession { workspace: Workspace::NewWorktree, ..request }).unwrap();
    let gone = third.worktree.clone().unwrap();
    let live = f.manager.live.lock().unwrap().get(&third.id).cloned().unwrap();
    f.manager.end_run_for(&live, ExitCause::UserClose);
    std::fs::remove_dir_all(&gone.path).unwrap();
    let check = f.manager.close_check(third.id).unwrap().worktree.unwrap();
    assert!(check.gone && check.clean.is_none());
    let note = f.manager.close(third.id, true).unwrap().worktree_note.unwrap();
    assert!(note.contains("already gone"), "{note}");
    assert!(!git_in(&repo, &["worktree", "list"]).contains(&gone.branch));
}

#[test]
fn a_worktree_git_removed_only_in_part_is_reported_so() {
    let f = Fixture::new("worktree-in-use");
    let repo = f.dir.join("ledger");
    std::fs::create_dir_all(repo.join("web")).unwrap();
    std::fs::write(repo.join("web").join("index.txt"), "hello").unwrap();
    git_in(&repo, &["init", "-q", "-b", "main"]);
    git_in(&repo, &["add", "."]);
    git_in(&repo, &["commit", "-q", "-m", "first"]);
    let s = f
        .manager
        .create(CreateSession {
            agent: AgentKind::Shell,
            cwd: repo.display().to_string(),
            name: None,
            credential: None,
            cols: 100,
            rows: 30,
            options: Default::default(),
            provider_id: None,
            description: None,
            project_id: None,
            tags: Vec::new(),
            workspace: Workspace::NewWorktree,
            routing: None,
        })
        .unwrap();
    let wt = s.worktree.clone().unwrap();

    // A process outside the session works in the worktree, so git can't delete that folder (L44).
    let mut holder = std::process::Command::new("cmd.exe")
        .arg("/k")
        .current_dir(Path::new(&wt.path).join("web"))
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let note = f.manager.close(s.id, true).unwrap().worktree_note.unwrap();
    let _ = holder.kill();
    let _ = holder.wait();
    assert!(note.starts_with("Removed the worktree from git, but couldn't delete all of its folder"), "{note}");
    assert!(f.manager.summary(s.id).unwrap().worktree.unwrap().removed);
    assert!(!git_in(&repo, &["worktree", "list"]).contains(&wt.branch));
}
