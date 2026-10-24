//! P7: what needs you, outside the session views. Every session summary passes through
//! `Attention::observe` (one call in `lib.rs`'s notify closure), which keeps the waiting count for
//! the tray and the taskbar, and stores one notification per waiting episode for the centre.
//! The fleet strip reads summaries itself, from the same `session-updated` event.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, OnceLock, Weak};

use chorus_core::inbox::{NOTICE_KEEP_MS, NoticeFacts, notice_for};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Manager, State};
use tauri_specta::Event;

use crate::sessions::{SessionManager, SessionStatus, SessionSummary};
use crate::store::notifications::{self, Notification};
use crate::store::now_ms;
use crate::tray;

type ManagerState<'a> = State<'a, Arc<SessionManager>>;

/// A notification was stored; the centre adds it and the bell shows the new unread count.
#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct NotificationAdded {
    pub notification: Notification,
    pub unread: u32,
}

/// Asks the main window to show a session, or the Overview's Needs you (the tray, a strip chip).
#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct AttentionNavigate {
    pub session_id: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct NotificationsPage {
    /// Newest first.
    pub items: Vec<Notification>,
    pub unread: u32,
}

/// Most notifications the centre lists.
const LIST_LIMIT: u32 = 200;

pub struct Attention {
    app: AppHandle,
    manager: OnceLock<Weak<SessionManager>>,
    /// Open sessions that need you now, by id.
    waiting: Mutex<HashMap<u32, bool>>,
    /// The count last shown, so the tray is only redrawn when it changes.
    shown: Mutex<Option<u32>>,
    /// Episodes already stored this run, so a repeated summary never decrypts the vault again.
    stored: Mutex<HashSet<String>>,
}

impl Attention {
    pub fn new(app: AppHandle) -> Arc<Self> {
        Arc::new(Self {
            app,
            manager: OnceLock::new(),
            waiting: Mutex::new(HashMap::new()),
            shown: Mutex::new(None),
            stored: Mutex::new(HashSet::new()),
        })
    }

    pub fn bind(&self, manager: &Arc<SessionManager>) {
        let _ = self.manager.set(Arc::downgrade(manager));
        if let Err(e) = notifications::prune(manager.store(), now_ms() - NOTICE_KEEP_MS) {
            eprintln!("notifications: {e}");
        }
    }

    pub fn observe(&self, s: &SessionSummary) {
        let needs = s.attention.needs_you() && !matches!(s.status, SessionStatus::Closed);
        let count = {
            let mut waiting = self.waiting.lock().expect("waiting lock");
            if needs { waiting.insert(s.id, true) } else { waiting.remove(&s.id) };
            waiting.values().filter(|w| **w).count() as u32
        };
        let changed = {
            let mut shown = self.shown.lock().expect("shown lock");
            let changed = *shown != Some(count);
            *shown = Some(count);
            changed
        };
        if changed {
            tray::show_count(&self.app, count);
            // For P7's exit check: when the tray changed, beside the hook's arrival in the database.
            #[cfg(debug_assertions)]
            eprintln!("attention: tray count {count} at {}", now_ms());
        }
        if needs {
            self.notify(s);
        }
    }

    fn notify(&self, s: &SessionSummary) {
        let Some(manager) = self.manager.get().and_then(Weak::upgrade) else { return };
        let fallback = s.run_started_at_secs.unwrap_or(s.created_at_secs);
        let mut facts = NoticeFacts {
            session: s.id,
            name: &s.name,
            attention: s.attention,
            since_ms: s.needs_you_since.map(|at| at as i64),
            fallback_at: i64::from(fallback) * 1000,
            now_ms: now_ms(),
            detail: None,
        };
        // The episode's key doesn't depend on the body, so a repeated summary stops here, before
        // any query.
        let Some(first) = notice_for(&facts) else { return };
        if !self.stored.lock().expect("stored lock").insert(first.dedupe_key) {
            return;
        }
        // What it is waiting on: the request itself for an approval or a question, else the last
        // message; for a failure, why it stopped.
        let kind = match s.attention {
            chorus_core::attention::Attention::Approval => Some("approval"),
            chorus_core::attention::Attention::Input => Some("question"),
            _ => None,
        };
        let ask = kind.and_then(|k| crate::store::attention::open_ask_subject(manager.store(), s.id, k).ok().flatten());
        facts.detail = if matches!(s.attention, chorus_core::attention::Attention::Failed) {
            s.note.as_deref().or(s.latest.as_deref())
        } else {
            ask.as_deref().or(s.latest.as_deref())
        };
        let Some(mut notice) = notice_for(&facts) else { return };
        // The body comes from scrubbed history or Chorus's own note; scrub it again anyway, with
        // every vault key, because it is shown outside the session.
        let (keys, _) = crate::vault::all_for_scrubbing(manager.store());
        let patterns = chorus_core::scrub::builtin_patterns();
        let scrub = |t: &str| String::from_utf8_lossy(&chorus_core::scrub::scrub_all(t.as_bytes(), &keys, &patterns)).into_owned();
        notice.title = scrub(&notice.title);
        notice.body = scrub(&notice.body);
        match notifications::insert(manager.store(), s.id, &notice, now_ms()) {
            Ok(Some(notification)) => {
                let unread = notifications::unread(manager.store()).unwrap_or(0);
                let _ = NotificationAdded { notification, unread }.emit(&self.app);
            }
            Ok(None) => {}
            Err(e) => eprintln!("notifications: {e}"),
        }
    }
}

/// Shows the main window, on a session or on the Overview's Needs you.
pub fn show_main(app: &AppHandle, session_id: Option<u32>) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
        let _ = AttentionNavigate { session_id }.emit_to(app, "main");
    }
}

#[tauri::command]
#[specta::specta]
pub fn notifications_list(state: ManagerState<'_>) -> Result<NotificationsPage, String> {
    Ok(NotificationsPage {
        items: notifications::list(state.store(), LIST_LIMIT)?,
        unread: notifications::unread(state.store())?,
    })
}

/// Marks one notification read and returns the unread count.
#[tauri::command]
#[specta::specta]
pub fn notification_read(state: ManagerState<'_>, id: u32) -> Result<u32, String> {
    if !notifications::mark_read(state.store(), id, now_ms())? {
        return Err(format!("There is no notification {id}."));
    }
    notifications::unread(state.store())
}

#[tauri::command]
#[specta::specta]
pub fn notifications_read_all(state: ManagerState<'_>) -> Result<(), String> {
    notifications::mark_all_read(state.store(), now_ms())
}
