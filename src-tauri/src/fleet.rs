//! The fleet switcher strip (spike S3, made real in P7).
//!
//! An always-on-top window that is never activated, so an external dictation app keeps the focus
//! while you click or drag the strip (D23). Only a click on a session's chip brings the main window
//! forward, on that session: an explicit request to go to Chorus (P7, decision 4). Whether it is
//! shown (default: shown) and where it was dragged are kept in settings, and the remembered place
//! is pulled back onto a screen when the displays change (`chorus_core::strip`).
//!
//! `focusable(false)` (WS_EX_NOACTIVATE) is not enough on its own: a click lands on WebView2's
//! input window, which belongs to the browser process and answers WM_MOUSEACTIVATE with
//! MA_ACTIVATE, so the strip became the foreground window (S3, first run). The strip therefore
//! carries an **input shield**: a nearly transparent native child window, owned by Chorus, over the
//! web content. It answers MA_NOACTIVATE, drags the strip (never-activated windows get no caption
//! move loop, LESSONS L11), and reports a short click to the page, which clicks the element under
//! it. Coordinates are physical pixels throughout, so the strip is correct at any DPI.

use std::sync::mpsc;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Manager, PhysicalPosition, WebviewUrl, WebviewWindowBuilder};
use tauri_specta::Event;
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GWL_EXSTYLE, GetClientRect, GetCursorPos, GetForegroundWindow, GetParent,
    GetWindowLongPtrW, GetWindowRect, HWND_TOP, IDC_HAND, LWA_ALPHA, LoadCursorW, MA_NOACTIVATE, RegisterClassExW,
    SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER, SetLayeredWindowAttributes, SetWindowPos, WM_LBUTTONDOWN,
    WM_MOUSEACTIVATE, WNDCLASSEXW, WS_CHILD, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOPMOST, WS_VISIBLE,
};
use windows::core::w;

use crate::sessions::SessionManager;
use crate::store::settings;

pub const LABEL: &str = "fleet";
const SHOWN_KEY: &str = "fleet.shown";
const POSITION_KEY: &str = "fleet.position";
/// The strip's size in logical pixels.
const WIDTH: f64 = 460.0;
const HEIGHT: f64 = 56.0;

/// Movement below this many pixels between press and release is a click, not a drag.
const CLICK_SLOP_PX: i32 = 4;

static APP: OnceLock<AppHandle> = OnceLock::new();

/// A click on the strip, in physical pixels from its top-left corner. The page turns it into a
/// click on the element under that point.
#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct FleetClick {
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct FleetProbe {
    pub hwnd: String,
    pub left: i32,
    pub top: i32,
    pub width: i32,
    pub height: i32,
    pub dpi: u32,
    pub no_activate: bool,
    pub topmost: bool,
    pub shield: bool,
    pub foreground_hwnd: String,
}

fn probe_hwnd(hwnd: HWND, shield: bool) -> FleetProbe {
    let mut rect = RECT::default();
    // SAFETY: plain Win32 queries on a window we own.
    unsafe {
        let _ = GetWindowRect(hwnd, &mut rect);
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
        FleetProbe {
            hwnd: format!("{:#x}", hwnd.0 as usize),
            left: rect.left,
            top: rect.top,
            width: rect.right - rect.left,
            height: rect.bottom - rect.top,
            dpi: GetDpiForWindow(hwnd),
            no_activate: ex & WS_EX_NOACTIVATE.0 != 0,
            topmost: ex & WS_EX_TOPMOST.0 != 0,
            shield,
            foreground_hwnd: format!("{:#x}", GetForegroundWindow().0 as usize),
        }
    }
}

/// Follows the cursor until the left button is released; a release within the click slop is
/// reported as a click at `origin` (client pixels).
fn track_press(strip: HWND, origin: (i32, i32)) {
    let raw = strip.0 as usize;
    std::thread::spawn(move || {
        let strip = HWND(raw as *mut _);
        let mut start = POINT::default();
        let mut rect = RECT::default();
        let mut moved = 0;
        // SAFETY: Win32 calls on our own window; the loop ends when the button is released.
        unsafe {
            if GetCursorPos(&mut start).is_err() || GetWindowRect(strip, &mut rect).is_err() {
                return;
            }
            while GetAsyncKeyState(i32::from(VK_LBUTTON.0)) as u16 & 0x8000 != 0 {
                let mut now = POINT::default();
                if GetCursorPos(&mut now).is_ok() {
                    let (dx, dy) = (now.x - start.x, now.y - start.y);
                    moved = moved.max(dx.abs().max(dy.abs()));
                    if moved >= CLICK_SLOP_PX {
                        let flags = SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE;
                        let _ = SetWindowPos(strip, None, rect.left + dx, rect.top + dy, 0, 0, flags);
                    }
                }
                std::thread::sleep(Duration::from_millis(8));
            }
        }
        let Some(app) = APP.get() else { return };
        if moved < CLICK_SLOP_PX {
            let _ = FleetClick { x: origin.0, y: origin.1 }.emit_to(app, LABEL);
        } else {
            let mut end = RECT::default();
            // SAFETY: a plain query on our own window.
            if unsafe { GetWindowRect(strip, &mut end) }.is_ok()
                && let Some(manager) = app.try_state::<Arc<SessionManager>>()
            {
                let _ = settings::set(manager.store(), POSITION_KEY, &format!("{},{}", end.left, end.top));
            }
        }
    });
}

unsafe extern "system" fn shield_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_LBUTTONDOWN => {
            let x = i32::from((lparam.0 & 0xffff) as u16 as i16);
            let y = i32::from(((lparam.0 >> 16) & 0xffff) as u16 as i16);
            // SAFETY: the shield's parent is the strip.
            if let Ok(strip) = unsafe { GetParent(hwnd) } {
                track_press(strip, (x, y));
            }
            LRESULT(0)
        }
        // SAFETY: default handling for everything else.
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

/// Creates the input shield over the strip's client area. Must run on the window's UI thread.
fn install_shield(strip: HWND) -> windows::core::Result<()> {
    static REGISTERED: OnceLock<()> = OnceLock::new();
    // SAFETY: standard window class registration and child creation on the UI thread.
    unsafe {
        let instance = GetModuleHandleW(None)?;
        REGISTERED.get_or_init(|| {
            let class = WNDCLASSEXW {
                cbSize: size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(shield_proc),
                hInstance: instance.into(),
                hCursor: LoadCursorW(None, IDC_HAND).unwrap_or_default(),
                lpszClassName: w!("ChorusFleetShield"),
                ..Default::default()
            };
            RegisterClassExW(&class);
        });
        let mut client = RECT::default();
        GetClientRect(strip, &mut client)?;
        let shield = CreateWindowExW(
            WS_EX_LAYERED,
            w!("ChorusFleetShield"),
            None,
            WS_CHILD | WS_VISIBLE,
            0,
            0,
            client.right,
            client.bottom,
            Some(strip),
            None,
            Some(instance.into()),
            None,
        )?;
        // Alpha 1 of 255: invisible, but still hit-tested (alpha 0 would let clicks through).
        SetLayeredWindowAttributes(shield, COLORREF(0), 1, LWA_ALPHA)?;
        SetWindowPos(shield, Some(HWND_TOP), 0, 0, client.right, client.bottom, SWP_NOACTIVATE)?;
    }
    Ok(())
}

/// Every screen's work area, the primary first (so a strip that was never moved goes there), and
/// the primary's scale.
fn work_areas(app: &AppHandle) -> (Vec<chorus_core::strip::Area>, f64) {
    let monitors = app.available_monitors().unwrap_or_default();
    let primary = app.primary_monitor().ok().flatten();
    let scale = primary.as_ref().or(monitors.first()).map_or(1.0, |m| m.scale_factor());
    let mut areas: Vec<chorus_core::strip::Area> = Vec::new();
    for m in primary.iter().chain(monitors.iter()) {
        let r = m.work_area();
        let area = chorus_core::strip::Area {
            x: r.position.x,
            y: r.position.y,
            width: r.size.width as i32,
            height: r.size.height as i32,
        };
        if !areas.contains(&area) {
            areas.push(area);
        }
    }
    (areas, scale)
}

/// Where the strip should open, in physical pixels: the remembered place, kept on a screen.
fn placement(app: &AppHandle) -> (PhysicalPosition<i32>, f64) {
    let (areas, scale) = work_areas(app);
    let saved = app.try_state::<Arc<SessionManager>>().and_then(|m| settings::get(m.store(), POSITION_KEY).ok().flatten()).and_then(|v| {
        let (x, y) = v.split_once(',')?;
        Some((x.trim().parse().ok()?, y.trim().parse().ok()?))
    });
    let size = ((WIDTH * scale).round() as i32, (HEIGHT * scale).round() as i32);
    let (x, y) = chorus_core::strip::place(saved, size, &areas);
    (PhysicalPosition::new(x, y), scale)
}

/// How often an open strip is checked against the screens.
const SCREEN_CHECK: Duration = Duration::from_secs(5);

/// Pulls an open strip back onto a screen if it is no longer on one (a monitor was unplugged, or
/// the resolution or taskbar changed), and remembers where it went. Never activates it.
fn keep_on_screen(app: &AppHandle) {
    let Some(w) = app.get_webview_window(LABEL) else { return };
    let (Ok(at), Ok(size)) = (w.outer_position(), w.outer_size()) else { return };
    let (areas, _) = work_areas(app);
    let size = (size.width as i32, size.height as i32);
    // Only a strip whose centre is off every screen moves; one the user put across two monitors or
    // a little over an edge stays.
    if areas.is_empty() || chorus_core::strip::on_screen((at.x, at.y), size, &areas) {
        return;
    }
    let (x, y) = chorus_core::strip::place(Some((at.x, at.y)), size, &areas);
    if (x, y) != (at.x, at.y) && w.set_position(PhysicalPosition::new(x, y)).is_ok()
        && let Some(manager) = app.try_state::<Arc<SessionManager>>()
    {
        let _ = settings::set(manager.store(), POSITION_KEY, &format!("{x},{y}"));
    }
}

/// Opens the strip (or returns the open one) without activating it. Async because building a
/// window on the thread that runs the event loop deadlocks on Windows (L28).
async fn open(app: AppHandle) -> Result<FleetProbe, String> {
    let _ = APP.set(app.clone());
    if let Some(w) = app.get_webview_window(LABEL) {
        return Ok(probe_hwnd(w.hwnd().map_err(|e| e.to_string())?, true));
    }
    let (at, scale) = placement(&app);
    let window = WebviewWindowBuilder::new(&app, LABEL, WebviewUrl::App("index.html?view=fleet".into()))
        .title("Chorus fleet")
        .inner_size(WIDTH, HEIGHT)
        .position(f64::from(at.x) / scale, f64::from(at.y) / scale)
        .decorations(false)
        .resizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .shadow(false)
        .focused(false)
        .focusable(false)
        .build()
        .map_err(|e| e.to_string())?;
    let _ = window.set_position(at);
    let hwnd = window.hwnd().map_err(|e| e.to_string())?;
    let raw = hwnd.0 as usize;
    let (tx, rx) = mpsc::channel();
    app.run_on_main_thread(move || {
        let _ = tx.send(install_shield(HWND(raw as *mut _)).map_err(|e| e.to_string()));
    })
    .map_err(|e| e.to_string())?;
    let shield = rx.recv_timeout(Duration::from_secs(5)).map_err(|e| e.to_string())?;
    shield?;
    Ok(probe_hwnd(hwnd, true))
}

/// Whether the strip should be shown: the stored choice, shown when never chosen.
pub fn wanted(manager: &SessionManager) -> bool {
    settings::get(manager.store(), SHOWN_KEY).ok().flatten().is_none_or(|v| v != "false")
}

pub fn is_shown(app: &AppHandle) -> bool {
    app.get_webview_window(LABEL).is_some()
}

/// Shows or hides the strip and remembers the choice. Never activates anything.
pub fn set_shown(app: &AppHandle, shown: bool) {
    let _ = APP.set(app.clone());
    if let Some(manager) = app.try_state::<Arc<SessionManager>>() {
        let _ = settings::set(manager.store(), SHOWN_KEY, if shown { "true" } else { "false" });
    }
    crate::tray::strip_checked(shown);
    if shown {
        let opening = app.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(e) = open(opening).await {
                eprintln!("fleet strip: {e}");
            }
        });
    } else if let Some(w) = app.get_webview_window(LABEL) {
        let _ = w.close();
    }
    let _ = FleetShown { shown }.emit(app);
}

/// At start: opens the strip if it is wanted.
pub fn start(app: &AppHandle, manager: &SessionManager) {
    if wanted(manager) {
        set_shown(app, true);
    }
    // Displays change while Chorus runs; an open strip is kept on one.
    let watching = app.clone();
    let _ = std::thread::Builder::new().name("fleet-screens".into()).spawn(move || {
        loop {
            std::thread::sleep(SCREEN_CHECK);
            keep_on_screen(&watching);
        }
    });
}

/// Whether the strip is shown, for Settings.
#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct FleetShown {
    pub shown: bool,
}

#[tauri::command]
#[specta::specta]
pub fn fleet_shown(state: tauri::State<'_, Arc<SessionManager>>) -> bool {
    wanted(&state)
}

#[tauri::command]
#[specta::specta]
pub fn fleet_set_shown(app: AppHandle, shown: bool) {
    set_shown(&app, shown);
}

/// A chip on the strip was clicked: bring the main window forward on that session.
#[tauri::command]
#[specta::specta]
pub fn fleet_open_session(app: AppHandle, state: tauri::State<'_, Arc<SessionManager>>, session_id: u32) -> Result<(), String> {
    state.summary(session_id)?;
    crate::attention::show_main(&app, Some(session_id));
    Ok(())
}

/// The strip's geometry, styles and the foreground window, for runtime checks (D23).
#[tauri::command]
#[specta::specta]
pub fn fleet_probe(app: AppHandle) -> Result<FleetProbe, String> {
    let window = app.get_webview_window(LABEL).ok_or("fleet strip is not open")?;
    Ok(probe_hwnd(window.hwnd().map_err(|e| e.to_string())?, true))
}
