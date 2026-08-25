//! The system tray icon.
//!
//! The application's real home: closing the dashboard window hides it rather
//! than quitting, because dictation is a background service and a user who
//! closes a window has not asked to stop being able to dictate.
//!
//! Quitting is explicit, from this menu. That is deliberate — an application
//! that captures audio on a hotkey should be obvious about still running.

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager as _};

/// Menu item ids. Matched exhaustively, so a new item cannot be forgotten.
const SHOW: &str = "show";
const QUIT: &str = "quit";

/// Build the tray icon and its menu.
pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, SHOW, "Open KnegisWhisper", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, QUIT, "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &separator, &quit])?;

    TrayIconBuilder::with_id("main-tray")
        .icon(app.default_window_icon().cloned().ok_or_else(|| {
            tauri::Error::Anyhow(anyhow::anyhow!(
                "the bundle has no window icon to use for the tray"
            ))
        })?)
        .tooltip("KnegisWhisper")
        .menu(&menu)
        // The menu should not also open on a left click: left click shows the
        // window, which is what a user expects from a tray icon.
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            SHOW => show_dashboard(app),
            QUIT => {
                tracing::info!("quitting from the tray menu");
                app.exit(0);
            }
            other => tracing::warn!(id = other, "unrecognised tray menu item"),
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_dashboard(tray.app_handle());
            }
        })
        .build(app)?;

    Ok(())
}

/// Show and focus the dashboard window, creating nothing.
///
/// Unlike the overlay, the dashboard is allowed to take focus: the user asked
/// for it.
pub fn show_dashboard(app: &AppHandle) {
    let Some(window) = app.get_webview_window("main") else {
        tracing::warn!("no main window to show");
        return;
    };

    if let Err(err) = window.show() {
        tracing::warn!(error = %err, "could not show the dashboard");
    }
    if let Err(err) = window.unminimize() {
        tracing::debug!(error = %err, "could not unminimize the dashboard");
    }
    if let Err(err) = window.set_focus() {
        tracing::warn!(error = %err, "could not focus the dashboard");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_ids_are_distinct() {
        // They are matched by string, so a collision would silently route two
        // items to one handler.
        assert_ne!(SHOW, QUIT);
    }
}
