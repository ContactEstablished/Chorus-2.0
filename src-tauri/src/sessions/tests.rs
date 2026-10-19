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
    assert!(f.manager.close(id).unwrap_err().contains("locked"));
    assert!(f.manager.restart(id).unwrap_err().contains("locked"));
    crate::lock::set_pin(f.manager.store(), "4321").unwrap();
    assert!(f.manager.set_locked(id, false, Some("0000")).is_err());
    f.manager.set_locked(id, false, Some("4321")).unwrap();
    let report = f.manager.close(id).unwrap();
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
