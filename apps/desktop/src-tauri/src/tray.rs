//! The system tray icon.
//!
//! The application's real home: closing the dashboard window hides it rather
//! than quitting, because dictation is a background service and a user who
//! closes a window has not asked to stop being able to dictate.
//!
//! Quitting is explicit, from this menu. That is deliberate — an application
//! that captures audio on a hotkey should be obvious about still running. What
//! Quit does while a meeting is being recorded or saved is `quit.rs`.
//!
//! And it says what it is doing. The tooltip and the first menu line report
//! whether the microphone is open, because the tray is the one part of this
//! application that is always on screen. `meeting.rs` claimed this was true
//! before it was — a doc comment describing a feature nobody had written — which
//! is the same defect as an interface reporting a state it never read.

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager as _};

/// Menu item ids. Matched exhaustively, so a new item cannot be forgotten.
const SHOW: &str = "show";
const QUIT: &str = "quit";
/// The line that says what is happening. Disabled: it is a report, not a button.
const ACTIVITY: &str = "activity";

/// What the application is doing, for the one indicator that is always visible.
///
/// An enum rather than a boolean because the three states need three different
/// sentences, and because a fourth — a dictation *during* a meeting — is a real
/// combination that a boolean would have to pick a winner for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Activity {
    Idle,
    /// The dictation microphone is open.
    Dictating,
    /// A meeting is being recorded.
    Meeting,
    /// Both, which happens if somebody dictates a note mid-meeting.
    MeetingAndDictating,
}

impl Activity {
    /// The sentence shown in the tooltip and the menu.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Idle => "Not recording",
            Self::Dictating => "Recording a dictation",
            Self::Meeting => "Recording a meeting",
            Self::MeetingAndDictating => "Recording a meeting and a dictation",
        }
    }

    /// This state with dictation switched on or off, leaving the meeting alone.
    #[must_use]
    pub const fn with_dictating(self, dictating: bool) -> Self {
        match (self, dictating) {
            (Self::Meeting | Self::MeetingAndDictating, true) => Self::MeetingAndDictating,
            (Self::Meeting | Self::MeetingAndDictating, false) => Self::Meeting,
            (_, true) => Self::Dictating,
            (_, false) => Self::Idle,
        }
    }

    /// This state with the meeting switched on or off, leaving dictation alone.
    #[must_use]
    pub const fn with_meeting(self, meeting: bool) -> Self {
        match (self, meeting) {
            (Self::Dictating | Self::MeetingAndDictating, true) => Self::MeetingAndDictating,
            (Self::Dictating | Self::MeetingAndDictating, false) => Self::Dictating,
            (_, true) => Self::Meeting,
            (_, false) => Self::Idle,
        }
    }
}

/// What the tray is currently reporting.
///
/// Held here rather than passed in, because the two callers each know about only
/// half of it: the dictation runtime knows whether it is recording and nothing
/// about meetings, and the meeting runtime the reverse. Combining them anywhere
/// else would mean one of them overwriting the other's state.
static ACTIVITY_STATE: std::sync::Mutex<Activity> = std::sync::Mutex::new(Activity::Idle);

/// The Quit line, kept so it can say when a Quit is waiting for a meeting.
struct QuitItem(MenuItem<tauri::Wry>);

/// Build the tray icon and its menu.
pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let activity = MenuItem::with_id(
        app,
        ACTIVITY,
        Activity::Idle.label(),
        // Disabled, because it reports rather than does. A clickable line here
        // would invite somebody to press it to stop recording, and stopping
        // belongs where starting is.
        false,
        None::<&str>,
    )?;
    let show = MenuItem::with_id(app, SHOW, "Open KnegisWhisper", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, QUIT, crate::quit::label(false), true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&activity, &separator, &show, &separator, &quit])?;
    app.manage(QuitItem(quit));

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
            // Disabled, so it should never fire. Named rather than left to the
            // catch-all, which would log it as unrecognised and send somebody
            // looking for a missing handler.
            ACTIVITY => {}
            QUIT => crate::quit::request(app),
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

/// Report that dictation started or stopped, leaving any meeting alone.
pub fn dictating(app: &AppHandle, recording: bool) {
    update(app, |current| current.with_dictating(recording));
}

/// Report that a meeting started or stopped, leaving any dictation alone.
pub fn meeting(app: &AppHandle, recording: bool) {
    update(app, |current| current.with_meeting(recording));
}

/// Say on the Quit line whether a Quit is waiting for a meeting to be saved.
///
/// The line stays enabled while it waits: pressing it again is how the wait is
/// cancelled, or turned into quitting now.
pub fn quit_waiting(app: &AppHandle, waiting: bool) {
    let Some(item) = app.try_state::<QuitItem>() else {
        return;
    };
    if let Err(err) = item.0.set_text(crate::quit::label(waiting)) {
        tracing::warn!(error = %err, "could not update the tray's Quit line");
    }
}

/// Apply a change to the reported activity.
fn update(app: &AppHandle, change: impl FnOnce(Activity) -> Activity) {
    let next = {
        let mut held = ACTIVITY_STATE
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let next = change(*held);
        if next == *held {
            return;
        }
        *held = next;
        next
    };

    let Some(tray) = app.tray_by_id("main-tray") else {
        // Not fatal, and not silent: on a platform with no tray the recording is
        // still indicated by the overlay and the notification window.
        tracing::debug!(?next, "no tray icon to update");
        return;
    };

    if let Err(err) = tray.set_tooltip(Some(format!("KnegisWhisper — {}", next.label()))) {
        tracing::warn!(error = %err, "could not update the tray tooltip");
    }
    tracing::debug!(?next, "tray activity");
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
        assert_ne!(SHOW, ACTIVITY);
        assert_ne!(QUIT, ACTIVITY);
    }

    #[test]
    fn the_two_recordings_do_not_overwrite_each_other() {
        // The reason this is a state machine and not a boolean. The dictation
        // runtime knows nothing about meetings and the meeting runtime nothing
        // about dictation, so either one setting a flag would clear the other's.
        let both = Activity::Idle.with_meeting(true).with_dictating(true);
        assert_eq!(both, Activity::MeetingAndDictating);

        // Dictation ends; the meeting is still recording.
        assert_eq!(both.with_dictating(false), Activity::Meeting);
        // The meeting ends; the dictation is still recording.
        assert_eq!(both.with_meeting(false), Activity::Dictating);
    }

    #[test]
    fn every_activity_has_its_own_sentence() {
        // Four states and four distinct sentences. Two sharing one would make the
        // always-visible indicator ambiguous about which microphone is open,
        // which is the one thing it exists to answer.
        let all = [
            Activity::Idle,
            Activity::Dictating,
            Activity::Meeting,
            Activity::MeetingAndDictating,
        ];

        let mut labels: Vec<&str> = all.iter().map(|a| a.label()).collect();
        labels.sort_unstable();
        let count = labels.len();
        labels.dedup();
        assert_eq!(labels.len(), count, "two states share a sentence");

        assert_eq!(Activity::Idle.label(), "Not recording");
        for activity in all {
            if activity != Activity::Idle {
                assert!(
                    activity.label().starts_with("Recording"),
                    "{activity:?}: {}",
                    activity.label()
                );
            }
        }
    }

    #[test]
    fn switching_something_off_that_was_never_on_stays_idle() {
        assert_eq!(Activity::Idle.with_dictating(false), Activity::Idle);
        assert_eq!(Activity::Idle.with_meeting(false), Activity::Idle);
    }
}
