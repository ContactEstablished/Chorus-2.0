//! The tray icon (P7): Chorus's mark with the waiting-count badge (`chorus_core::badge`), a
//! tooltip, and a menu. A left click opens the Overview's Needs you. The main window's taskbar
//! button gets the same count as an overlay icon. Closing the main window still quits Chorus.

use std::sync::OnceLock;

use chorus_core::badge;
use tauri::image::Image;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Wry};

use crate::{attention, fleet};

const TRAY: &str = "chorus";
/// The taskbar overlay is drawn on its own small canvas.
const OVERLAY_PX: u32 = 32;

static STRIP_ITEM: OnceLock<CheckMenuItem<Wry>> = OnceLock::new();

pub fn install(app: &AppHandle, strip_shown: bool) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open Chorus", true, None::<&str>)?;
    let strip = CheckMenuItem::with_id(app, "strip", "Show fleet strip", true, strip_shown, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Chorus", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &strip, &PredefinedMenuItem::separator(app)?, &quit])?;
    let _ = STRIP_ITEM.set(strip);
    let mut tray = TrayIconBuilder::with_id(TRAY)
        .tooltip(badge::tooltip(0))
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => attention::show_main(app, None),
            "strip" => {
                let show = !fleet::is_shown(app);
                fleet::set_shown(app, show);
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                attention::show_main(tray.app_handle(), None);
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

/// Keeps the menu's tick in step when the strip is shown or hidden elsewhere.
pub fn strip_checked(shown: bool) {
    if let Some(item) = STRIP_ITEM.get() {
        let _ = item.set_checked(shown);
    }
}

/// Redraws the tray icon, its tooltip and the taskbar overlay for `count` waiting sessions.
pub fn show_count(app: &AppHandle, count: u32) {
    if let Some(tray) = app.tray_by_id(TRAY) {
        if let Some(base) = app.default_window_icon() {
            let rgba = badge::overlay(base.rgba(), base.width(), base.height(), count);
            let _ = tray.set_icon(Some(Image::new_owned(rgba, base.width(), base.height())));
        }
        let _ = tray.set_tooltip(Some(badge::tooltip(count)));
    }
    if let Some(window) = app.get_webview_window("main") {
        let overlay = (count > 0).then(|| {
            let blank = vec![0u8; (OVERLAY_PX * OVERLAY_PX * 4) as usize];
            Image::new_owned(badge::alone(&blank, OVERLAY_PX, count), OVERLAY_PX, OVERLAY_PX)
        });
        let _ = window.set_overlay_icon(overlay);
    }
}
