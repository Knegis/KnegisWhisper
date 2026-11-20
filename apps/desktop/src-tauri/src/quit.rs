//! Quit, asked for from the tray.
//!
//! Quit used to exit at once. During a meeting that cost the meeting: while it
//! was being saved the process ended before any note existed, and while it was
//! being recorded the shutdown discarded the recording (KNEG-563). Saving a
//! long meeting can take minutes with little on screen, which is exactly when
//! somebody takes the application for hung and quits it.
//!
//! So Quit looks first. With no meeting it exits as before. With one, it asks,
//! and nothing is lost without the person choosing that.
//!
//! Speaker labels are not waited for: they are a job on a note that is already
//! saved, and one cut off by an exit is marked on the note with Retry.

use std::future::Future;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use tauri::{AppHandle, Manager as _};
use tauri_plugin_dialog::{
    DialogExt as _, MessageDialogButtons, MessageDialogKind, MessageDialogResult,
};

use crate::meeting::{MeetingRuntime, MeetingState};

const STOP_AND_SAVE: &str = "Stop and save, then quit";
const KEEP_RECORDING: &str = "Keep recording";
const WHEN_SAVED: &str = "Quit when saved";
const QUIT_NOW: &str = "Quit now";
const CANCEL: &str = "Cancel";

/// How often a Quit that is waiting for a meeting looks again.
const LOOK_AGAIN: Duration = Duration::from_millis(500);

/// What Quit does, given what the meeting is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    /// Nothing would be lost.
    Exit,
    /// A meeting is being recorded.
    AskRecording,
    /// A meeting has stopped and is being transcribed and saved.
    AskSaving,
}

const fn step(state: MeetingState) -> Step {
    match state {
        MeetingState::Idle => Step::Exit,
        // Stalled too: what was captured is on disk and Stop can still save it.
        MeetingState::Recording
        | MeetingState::RecordingWithoutTranscript
        | MeetingState::Stalled => Step::AskRecording,
        MeetingState::Finishing => Step::AskSaving,
    }
}

/// What the person chose.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Answer {
    /// Carry on as if Quit had not been pressed.
    Stay,
    /// Stop the meeting, save it, then exit.
    StopAndSave,
    /// Exit once the save in progress has finished.
    WhenSaved,
    /// Exit, leaving the save unfinished and the recording to be recovered.
    Now,
}

/// The answer a pressed button stands for.
///
/// Anything unrecognised is `Stay`, which includes a box closed with Escape or
/// its own close button: leaving is the choice that can lose something, so it
/// is never the one arrived at by accident.
fn answer(pressed: &MessageDialogResult) -> Answer {
    match pressed {
        MessageDialogResult::Custom(label) if label == STOP_AND_SAVE => Answer::StopAndSave,
        MessageDialogResult::Custom(label) if label == WHEN_SAVED => Answer::WhenSaved,
        MessageDialogResult::Custom(label) if label == QUIT_NOW => Answer::Now,
        _ => Answer::Stay,
    }
}

/// Where a Quit has got to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// Nobody has asked, or they chose to stay.
    Idle,
    /// The question is on screen.
    Asking,
    /// The application exits when the meeting is saved. Numbered, so a wait
    /// the person has since cancelled can tell it is no longer wanted.
    Waiting(u64),
}

/// The one Quit in progress.
#[derive(Debug)]
struct Requests {
    phase: Phase,
    waits: u64,
}

impl Requests {
    const fn new() -> Self {
        Self {
            phase: Phase::Idle,
            waits: 0,
        }
    }

    /// Whether this Quit may ask. Not while a question is already open, so two
    /// presses never stack two boxes. A wait may be asked about again: that is
    /// how somebody who chose to wait changes their mind.
    fn begin(&mut self) -> bool {
        if self.phase == Phase::Asking {
            return false;
        }
        self.phase = Phase::Asking;
        true
    }

    /// Start a wait, replacing any earlier one.
    fn wait(&mut self) -> u64 {
        self.waits = self.waits.wrapping_add(1);
        self.phase = Phase::Waiting(self.waits);
        self.waits
    }

    fn stay(&mut self) {
        self.phase = Phase::Idle;
    }

    fn is_waiting(&self, wait: u64) -> bool {
        self.phase == Phase::Waiting(wait)
    }
}

static REQUESTS: Mutex<Requests> = Mutex::new(Requests::new());

fn requests() -> std::sync::MutexGuard<'static, Requests> {
    REQUESTS.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Wait until `state` reports no meeting. `false` if `wanted` stopped being
/// true first, which is a wait the person cancelled.
async fn until_idle<S, F>(mut state: S, wanted: impl Fn() -> bool, look_again: Duration) -> bool
where
    S: FnMut() -> F,
    F: Future<Output = MeetingState>,
{
    loop {
        if !wanted() {
            return false;
        }
        if step(state().await) == Step::Exit {
            return true;
        }
        tokio::time::sleep(look_again).await;
    }
}

/// Quit was chosen from the tray.
pub fn request(app: &AppHandle) {
    if !requests().begin() {
        tracing::debug!("quit asked for again while the question is open");
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let meetings = app.state::<Arc<MeetingRuntime>>().inner().clone();
        match step(meetings.status().await.state) {
            Step::Exit => exit(&app),
            Step::AskRecording => ask(
                &app,
                "A meeting is being recorded. Quitting now would end it before it is saved.",
                MessageDialogButtons::OkCancelCustom(
                    STOP_AND_SAVE.to_owned(),
                    KEEP_RECORDING.to_owned(),
                ),
            ),
            Step::AskSaving => ask(
                &app,
                "A meeting is still being saved, which can take several minutes for a long \
                 one.\n\nIf you quit now it has no note yet. The recording and what you typed \
                 stay on this computer, and Meetings offers to recover them the next time \
                 KnegisWhisper opens.",
                MessageDialogButtons::YesNoCancelCustom(
                    WHEN_SAVED.to_owned(),
                    QUIT_NOW.to_owned(),
                    CANCEL.to_owned(),
                ),
            ),
        }
    });
}

fn ask(app: &AppHandle, message: &str, buttons: MessageDialogButtons) {
    let answered = app.clone();
    app.dialog()
        .message(message)
        .title("Quit KnegisWhisper?")
        .kind(MessageDialogKind::Warning)
        .buttons(buttons)
        .show_with_result(move |pressed| act(&answered, answer(&pressed)));
}

fn act(app: &AppHandle, answer: Answer) {
    tracing::info!(?answer, "quit asked for during a meeting");
    match answer {
        Answer::Stay => {
            requests().stay();
            crate::tray::quit_waiting(app, false);
        }
        Answer::Now => exit(app),
        Answer::StopAndSave | Answer::WhenSaved => {
            let wait = requests().wait();
            crate::tray::quit_waiting(app, true);
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                let meetings = app.state::<Arc<MeetingRuntime>>().inner().clone();
                if answer == Answer::StopAndSave {
                    // An error here means the meeting stopped some other way
                    // while the question was open. Either way, wait it out.
                    if let Err(error) = meetings.stop().await {
                        tracing::info!(%error, "the meeting was not stopped by quit");
                    }
                }
                let saved = until_idle(
                    || async { meetings.status().await.state },
                    || requests().is_waiting(wait),
                    LOOK_AGAIN,
                )
                .await;
                if saved {
                    exit(&app);
                }
            });
        }
    }
}

fn exit(app: &AppHandle) {
    tracing::info!("quitting from the tray menu");
    app.exit(0);
}

/// The Quit line of the tray menu, which says when a Quit is waiting.
#[must_use]
pub const fn label(waiting: bool) -> &'static str {
    if waiting {
        "Quitting when the meeting is saved\u{2026}"
    } else {
        "Quit"
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    #[test]
    fn quit_exits_only_when_no_meeting_would_be_lost() {
        assert_eq!(step(MeetingState::Idle), Step::Exit);
        assert_eq!(step(MeetingState::Recording), Step::AskRecording);
        assert_eq!(
            step(MeetingState::RecordingWithoutTranscript),
            Step::AskRecording
        );
        assert_eq!(step(MeetingState::Stalled), Step::AskRecording);
        // The defect: this exited, and the meeting had no note.
        assert_eq!(step(MeetingState::Finishing), Step::AskSaving);
    }

    #[test]
    fn a_closed_box_or_an_unknown_button_stays() {
        assert_eq!(answer(&MessageDialogResult::Cancel), Answer::Stay);
        assert_eq!(answer(&MessageDialogResult::Ok), Answer::Stay);
        assert_eq!(answer(&MessageDialogResult::Yes), Answer::Stay);
        for label in [KEEP_RECORDING, CANCEL, ""] {
            assert_eq!(
                answer(&MessageDialogResult::Custom(label.to_owned())),
                Answer::Stay
            );
        }
    }

    #[test]
    fn each_button_that_leaves_is_its_own_answer() {
        let pressed = |label: &str| answer(&MessageDialogResult::Custom(label.to_owned()));
        assert_eq!(pressed(STOP_AND_SAVE), Answer::StopAndSave);
        assert_eq!(pressed(WHEN_SAVED), Answer::WhenSaved);
        assert_eq!(pressed(QUIT_NOW), Answer::Now);
        // Matched by their words, so two sharing a label would share an answer.
        let labels = [STOP_AND_SAVE, KEEP_RECORDING, WHEN_SAVED, QUIT_NOW, CANCEL];
        let distinct: std::collections::HashSet<_> = labels.iter().collect();
        assert_eq!(distinct.len(), labels.len());
    }

    #[test]
    fn a_second_quit_does_not_open_a_second_question() {
        let mut requests = Requests::new();
        assert!(requests.begin());
        assert!(!requests.begin());
        requests.stay();
        assert!(requests.begin());
    }

    #[test]
    fn a_quit_that_is_waiting_can_be_asked_about_again_and_cancelled() {
        let mut requests = Requests::new();
        assert!(requests.begin());
        let wait = requests.wait();
        assert!(requests.is_waiting(wait));

        // Quit again: the question opens, and the wait is no longer wanted
        // until it is answered.
        assert!(requests.begin());
        assert!(!requests.is_waiting(wait));
        requests.stay();
        assert!(!requests.is_waiting(wait));

        // Choosing to wait again is a new wait; the old one stays cancelled.
        assert!(requests.begin());
        let again = requests.wait();
        assert_ne!(again, wait);
        assert!(requests.is_waiting(again) && !requests.is_waiting(wait));
    }

    #[tokio::test]
    async fn the_wait_ends_once_the_meeting_is_saved_and_not_before() {
        let looks = AtomicUsize::new(0);
        let saved = until_idle(
            || async {
                if looks.fetch_add(1, Ordering::SeqCst) < 3 {
                    MeetingState::Finishing
                } else {
                    MeetingState::Idle
                }
            },
            || true,
            Duration::from_millis(1),
        )
        .await;
        assert!(saved);
        assert_eq!(looks.load(Ordering::SeqCst), 4);
    }

    #[tokio::test]
    async fn a_cancelled_wait_does_not_exit() {
        let wanted = AtomicBool::new(true);
        let saved = until_idle(
            || async {
                // Cancelled while the meeting is still being saved.
                wanted.store(false, Ordering::SeqCst);
                MeetingState::Finishing
            },
            || wanted.load(Ordering::SeqCst),
            Duration::from_millis(1),
        )
        .await;
        assert!(!saved);
    }

    #[test]
    fn the_quit_line_says_when_it_is_waiting() {
        assert_eq!(label(false), "Quit");
        assert_ne!(label(true), label(false));
        assert!(label(true).contains("saved"));
    }
}
