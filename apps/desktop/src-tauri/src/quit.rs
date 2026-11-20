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
//!
//! Three things a first version got wrong, found by a second reader the day it
//! was written. It stopped the meeting without first asking the meeting window
//! for an edit it had not yet sent, which the window's own Stop does. It exited
//! once the meeting was over whether or not that had produced a note. And a
//! wait that was cancelled while it was looking could still exit.

use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use tauri::{AppHandle, Emitter as _, Listener as _, Manager as _};
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

/// The meeting window's notes handshake, as `flushMeetingPanelNotes` in
/// `lib/ipc.ts` speaks it: asked on the first, answered on the second.
const FLUSH_NOTES: &str = "meeting-notes-flush";
const NOTES_FLUSHED: &str = "meeting-notes-flushed";
/// As long as the Meetings page gives the window to answer.
const FLUSH_WITHIN: Duration = Duration::from_secs(5);

/// How many meetings have been stopped without becoming a note.
///
/// A count and not the last outcome, because Quit only asks whether one ended
/// that way since it was pressed, and the stop it is waiting for may be one
/// somebody else started.
static ENDED_WITHOUT_NOTE: AtomicU64 = AtomicU64::new(0);

/// A meeting was stopped and no note came of it: nothing was transcribed, or
/// saving failed.
pub fn meeting_ended_without_note() {
    ENDED_WITHOUT_NOTE.fetch_add(1, Ordering::SeqCst);
}

fn ended_without_note() -> u64 {
    ENDED_WITHOUT_NOTE.load(Ordering::SeqCst)
}

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
    /// The exit has been asked for. Nothing more is asked.
    Leaving,
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
        if matches!(self.phase, Phase::Asking | Phase::Leaving) {
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

    /// Take the exit for this wait, if it is still the one wanted. Checked and
    /// taken in one step, so a wait cancelled a moment ago cannot exit and
    /// nothing can be asked between the check and the exit.
    fn claim(&mut self, wait: u64) -> bool {
        let wanted = self.is_waiting(wait);
        if wanted {
            self.phase = Phase::Leaving;
        }
        wanted
    }

    fn leave(&mut self) {
        self.phase = Phase::Leaving;
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
        let idle = step(state().await) == Step::Exit;
        // Looked at again: the wait may have been cancelled while the state
        // was being read, and that read is not instant.
        if !wanted() {
            return false;
        }
        if idle {
            return true;
        }
        tokio::time::sleep(look_again).await;
    }
}

/// What one reply on the notes handshake says about the request `id`: `None`
/// if it answers some other request or cannot be read.
fn flush_reply(payload: &str, id: &str) -> Option<Result<(), String>> {
    let reply: serde_json::Value = serde_json::from_str(payload).ok()?;
    if reply.get("requestId")?.as_str()? != id {
        return None;
    }
    Some(
        match reply.get("error").and_then(serde_json::Value::as_str) {
            Some(error) => Err(error.to_owned()),
            None => Ok(()),
        },
    )
}

/// Ask the meeting window to send any typed notes it is still holding, and
/// wait for it to say it has.
///
/// The window saves as it goes, a little behind the typing, and keeps an edit
/// whose save failed. Stopping closes the notes to further writes, so whatever
/// has not arrived by then is lost: the window's own Stop asks first, and so
/// must anything else that stops a meeting.
pub async fn flush_typed_notes(app: &AppHandle, within: Duration) -> Result<(), String> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let id = format!("quit-{}", NEXT.fetch_add(1, Ordering::Relaxed));

    let (answered, answer) = tokio::sync::oneshot::channel();
    let answered = Mutex::new(Some(answered));
    let wanted = id.clone();
    let listener = app.listen_any(NOTES_FLUSHED, move |event| {
        let Some(reply) = flush_reply(event.payload(), &wanted) else {
            return;
        };
        if let Some(answered) = answered
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
        {
            let _ = answered.send(reply);
        }
    });

    let asked = app.emit_to(
        crate::meeting_window::LABEL,
        FLUSH_NOTES,
        serde_json::json!({ "requestId": id }),
    );
    let reply = match asked {
        Err(error) => Err(format!("The meeting window could not be asked: {error}.")),
        Ok(()) => match tokio::time::timeout(within, answer).await {
            Ok(Ok(reply)) => reply,
            _ => Err("The meeting window didn't answer when asked to save your notes.".to_owned()),
        },
    };
    app.unlisten(listener);
    reply
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
        // Read before the question, so a meeting that ends without a note
        // while it is open is not missed.
        let before = ended_without_note();
        match step(meetings.state().await) {
            Step::Exit => exit(&app),
            Step::AskRecording => ask(
                &app,
                before,
                "A meeting is being recorded. Quitting now would end it before it is saved.",
                MessageDialogButtons::OkCancelCustom(
                    STOP_AND_SAVE.to_owned(),
                    KEEP_RECORDING.to_owned(),
                ),
            ),
            Step::AskSaving => ask(
                &app,
                before,
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

fn ask(app: &AppHandle, before: u64, message: &str, buttons: MessageDialogButtons) {
    let answered = app.clone();
    app.dialog()
        .message(message)
        .title("Quit KnegisWhisper?")
        .kind(MessageDialogKind::Warning)
        .buttons(buttons)
        .show_with_result(move |pressed| act(&answered, answer(&pressed), before));
}

/// `before` is how many meetings had ended without a note when Quit was
/// pressed.
fn act(app: &AppHandle, answer: Answer, before: u64) {
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
                    if let Err(reason) = flush_typed_notes(&app, FLUSH_WITHIN).await {
                        // Not stopped: stopping now could lose what was typed
                        // last. The meeting window's own Stop says the same
                        // and offers the way on.
                        if requests().is_waiting(wait) {
                            stay_open(
                                &app,
                                &format!(
                                    "{reason}\n\nThe meeting is still being recorded and \
                                     KnegisWhisper has not quit. Stop the meeting from its \
                                     window, then quit."
                                ),
                            );
                        }
                        return;
                    }
                    // An error here means the meeting stopped some other way
                    // while the question was open. Either way, wait it out.
                    if let Err(error) = meetings.stop().await {
                        tracing::info!(%error, "the meeting was not stopped by quit");
                    }
                }
                let over = until_idle(
                    || meetings.state(),
                    || requests().is_waiting(wait),
                    LOOK_AGAIN,
                )
                .await;
                if !over {
                    return;
                }
                if ended_without_note() != before {
                    // Over, and not saved. Quitting was conditional on the save.
                    if requests().is_waiting(wait) {
                        stay_open(
                            &app,
                            "The meeting ended without a note, so KnegisWhisper has not quit. \
                             Meetings says what happened and what was kept.",
                        );
                    }
                    return;
                }
                if requests().claim(wait) {
                    exit(&app);
                }
            });
        }
    }
}

/// Give up a Quit that could not do what was chosen, and say why.
fn stay_open(app: &AppHandle, message: &str) {
    tracing::warn!(message, "quit was given up");
    requests().stay();
    crate::tray::quit_waiting(app, false);
    crate::tray::show_dashboard(app);
    app.dialog()
        .message(message)
        .title("KnegisWhisper is still open")
        .kind(MessageDialogKind::Warning)
        .buttons(MessageDialogButtons::Ok)
        .show(|_| {});
}

fn exit(app: &AppHandle) {
    tracing::info!("quitting from the tray menu");
    requests().leave();
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
    async fn a_wait_cancelled_while_it_was_looking_does_not_exit() {
        // The defect: the wait was checked before the look and not after, so
        // one cancelled during a look that then found no meeting still exited.
        let wanted = AtomicBool::new(true);
        let over = until_idle(
            || async {
                wanted.store(false, Ordering::SeqCst);
                MeetingState::Idle
            },
            || wanted.load(Ordering::SeqCst),
            Duration::from_millis(1),
        )
        .await;
        assert!(!over);
    }

    #[test]
    fn only_the_wait_still_wanted_takes_the_exit_and_only_once() {
        let mut requests = Requests::new();
        assert!(requests.begin());
        let first = requests.wait();
        assert!(requests.begin());
        let second = requests.wait();

        assert!(!requests.claim(first));
        assert!(requests.claim(second));
        assert!(!requests.claim(second));
        // Leaving: no further question is opened.
        assert!(!requests.begin());
    }

    #[test]
    fn a_meeting_that_ended_without_a_note_is_counted() {
        let before = ended_without_note();
        meeting_ended_without_note();
        assert_ne!(ended_without_note(), before);
    }

    #[test]
    fn a_notes_reply_is_read_for_its_own_request_only() {
        let ok = r#"{"requestId":"quit-3","error":null}"#;
        assert_eq!(flush_reply(ok, "quit-3"), Some(Ok(())));
        assert_eq!(flush_reply(ok, "quit-4"), None);
        // The Meetings page asks on the same events, with its own ids.
        assert_eq!(
            flush_reply(r#"{"requestId":"1763-1","error":null}"#, "quit-3"),
            None
        );
        assert_eq!(
            flush_reply(
                r#"{"requestId":"quit-3","error":"Could not save"}"#,
                "quit-3"
            ),
            Some(Err("Could not save".to_owned()))
        );
        assert_eq!(flush_reply("not json", "quit-3"), None);
        assert_eq!(flush_reply(r#"{"error":null}"#, "quit-3"), None);
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
