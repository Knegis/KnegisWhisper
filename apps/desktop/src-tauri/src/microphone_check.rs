//! The Meetings page's "Test microphone" (KNEG-364).
//!
//! Five seconds of the saved microphone (`audio.inputDevice`, the one
//! dictation and meetings both record from), shown on the page's meter by the
//! same publisher every capture uses (`audio_level.rs`, session
//! `microphone-check`), and summed up when it ends: which microphone opened,
//! whether it was the one chosen, whether it heard anyone, and how loud.
//!
//! **Nothing is recorded and nothing is sent.** The samples are drained and
//! dropped as they arrive, nothing here writes a file, and nothing here
//! reaches the network. The microphone is closed when the capture is dropped,
//! at the end of the five seconds, which is also what ends the meter.
//!
//! A button rather than a meter that is always on (ruling, 2026-10-07): a
//! microphone open whenever the Meetings page is open would light the
//! system's microphone indicator with nothing being recorded.
//!
//! Refused while a meeting is under way or a dictation is recording, and
//! while another check runs. The refusals start "Not now: ", so the page
//! shows the sentence itself rather than behind Details (`lib/refusal.ts`).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, State};
use ts_rs::TS;

use crate::audio_level::{LevelSession, LevelSource};

/// How long a check listens.
pub const CHECK_LENGTH: Duration = Duration::from_secs(5);

/// How often the check thread empties the capture's buffer, so five seconds
/// of samples are never held: they are dropped, not kept.
const DRAIN_EVERY: Duration = Duration::from_millis(100);

/// How loud a reading must be to count as someone heard: a quarter of the
/// meter, −45 dBFS (`kw_audio_core::level_from_rms`).
///
/// Measured: a quiet room on the development machine read −83 dBFS, a meter
/// of 0 (KNEG-371's probe, 2026-10-07), and this check's ignored test on the
/// same microphone peaked at 0 (2026-10-08). Believed, not measured: speech at
/// a normal distance is −40 to −25 dBFS, a third of the meter and more. The
/// threshold sits between the two, well above a quiet room and below quiet
/// speech.
const HEARD_LEVEL: f32 = 0.25;

/// How many readings must reach [`HEARD_LEVEL`]: 300 ms of sound at the
/// publisher's 100 ms, so a knock or a click on its own is not "heard you".
const HEARD_READINGS: usize = 3;

/// What a microphone check found.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct MicrophoneCheck {
    /// The microphone the check opened, by its device name; `None` when the
    /// device gave none.
    pub device: Option<String>,
    /// The microphone chosen, when it was not connected and the check tested
    /// the system default (`device`) instead; `None` when it tested the one
    /// chosen, or none was chosen.
    pub fell_back: Option<String>,
    /// Whether it heard someone: [`HEARD_READINGS`] readings at
    /// [`HEARD_LEVEL`] or louder.
    pub heard: bool,
    /// The loudest reading, 0..1 as the meter shows it.
    pub peak_level: f32,
}

/// What else has the microphone, as a check asks before it opens it.
#[derive(Debug, Clone, Copy, Default)]
pub struct InUse {
    /// A meeting is starting or recording.
    pub meeting: bool,
    /// A dictation is recording.
    pub dictation: bool,
}

const MEETING_REFUSAL: &str =
    "Not now: a meeting is under way. Test the microphone once it has ended.";
const DICTATION_REFUSAL: &str =
    "Not now: a dictation is recording. Test the microphone once it has finished.";
const ALREADY_REFUSAL: &str = "Not now: the microphone is already being tested.";

/// Whether a check is running: one at a time. Managed state.
#[derive(Debug, Default)]
pub struct MicrophoneChecks {
    running: Arc<AtomicBool>,
}

/// A check in progress. Dropping it, however the check ended, lets the next
/// one begin.
#[derive(Debug)]
pub struct Checking(Arc<AtomicBool>);

impl Drop for Checking {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

impl MicrophoneChecks {
    /// Begin a check, or say in plain words why not.
    ///
    /// Whatever else has the microphone is asked first, so a refusal for a
    /// meeting or a dictation leaves no check marked as running.
    pub fn begin(&self, in_use: InUse) -> Result<Checking, String> {
        if in_use.meeting {
            return Err(MEETING_REFUSAL.to_owned());
        }
        if in_use.dictation {
            return Err(DICTATION_REFUSAL.to_owned());
        }
        self.running
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| ALREADY_REFUSAL.to_owned())?;
        Ok(Checking(Arc::clone(&self.running)))
    }
}

/// Whether `levels` heard someone, and the loudest of them.
///
/// Pure, over the readings the check took (one per publisher tick), so what
/// counts as heard is tested without a microphone.
fn summary(levels: &[f32]) -> (bool, f32) {
    let loud = levels.iter().filter(|level| **level >= HEARD_LEVEL).count();
    // `f32::max` passes over a NaN, which no reading should be.
    let peak = levels.iter().copied().fold(0.0_f32, f32::max).min(1.0);
    (loud >= HEARD_READINGS, peak)
}

/// The microphone chosen, when the check had to open another.
///
/// `kw_audio_core::Capture::start` finds a chosen device by its exact name
/// and opens the default when none matches, so a capture whose name is not
/// the one chosen is the default standing in for it.
fn fell_back(chosen: Option<&str>, opened: Option<&str>) -> Option<String> {
    chosen
        .filter(|chosen| Some(*chosen) != opened)
        .map(str::to_owned)
}

/// Open `chosen` (the default for `None`) for `length`, read its level every
/// `every`, hand each to `send`, and say what was heard.
///
/// The capture lives on a thread of its own, because `cpal::Stream` is not
/// `Send` (as `dictation::MicRecorder`'s is), and is dropped there when
/// `length` is up. The levels are read here, through `audio_level::run`,
/// which ends when the dropped capture's reader answers `None` and sends a
/// last 0 so the meter empties.
async fn listen(
    chosen: Option<String>,
    length: Duration,
    every: Duration,
    send: impl FnMut(f32),
) -> Result<MicrophoneCheck, String> {
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
    let asked = chosen.clone();
    std::thread::Builder::new()
        .name("kw-microphone-check".to_owned())
        .spawn(move || {
            let capture = match kw_audio_core::Capture::start(asked.as_deref()) {
                Ok(capture) => capture,
                Err(err) => {
                    let _ = ready_tx.send(Err(err.to_string()));
                    return;
                }
            };
            let _ = ready_tx.send(Ok((
                capture.device_name().map(str::to_owned),
                capture.level(),
            )));
            let until = std::time::Instant::now() + length;
            while std::time::Instant::now() < until {
                std::thread::sleep(DRAIN_EVERY);
                // Dropped: nothing is recorded.
                let _ = capture.drain();
            }
            // Dropping the capture closes the microphone, and the system's
            // indicator with it.
            drop(capture);
        })
        .map_err(|err| format!("could not start the microphone check: {err}"))?;

    let (device, reader) = ready_rx
        .await
        .map_err(|_| "the microphone check stopped before it started".to_owned())??;

    let mut levels = Vec::new();
    crate::audio_level::run(
        || {
            let level = reader.take();
            if let Some(level) = level {
                levels.push(level);
            }
            level
        },
        every,
        send,
    )
    .await;

    let (heard, peak_level) = summary(&levels);
    Ok(MicrophoneCheck {
        fell_back: fell_back(chosen.as_deref(), device.as_deref()),
        device,
        heard,
        peak_level,
    })
}

/// Listen to the saved microphone for five seconds and say what was heard
/// (KNEG-364). Records and sends nothing; see the module doc.
#[tauri::command]
pub async fn microphone_check(
    app: AppHandle,
    state: State<'_, crate::state::AppState>,
    checks: State<'_, MicrophoneChecks>,
    dictation: State<'_, Arc<crate::dictation::DictationRuntime>>,
    meetings: State<'_, Arc<crate::meeting::MeetingRuntime>>,
) -> Result<MicrophoneCheck, String> {
    let _checking = checks.begin(InUse {
        meeting: meetings.is_under_way().await,
        dictation: dictation.state() == kw_app_core::dictation::SessionState::Recording,
    })?;
    // The saved choice, as a meeting reads it at Start, so the check opens
    // what a meeting would.
    let chosen = crate::dictation::load_device(state.db()).await;
    listen(
        chosen,
        CHECK_LENGTH,
        crate::audio_level::TICK,
        crate::audio_level::sender(&app, LevelSession::MicrophoneCheck, LevelSource::Microphone),
    )
    .await
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;

    const NOTHING: InUse = InUse {
        meeting: false,
        dictation: false,
    };

    /// The brief's refusal: a meeting under way has the microphone, and a
    /// check would open it again under the meeting. Said in plain words, as
    /// a refusal the page shows in view, and it leaves nothing running.
    #[test]
    fn a_check_is_refused_while_a_meeting_is_under_way() {
        let checks = MicrophoneChecks::default();
        let refusal = checks
            .begin(InUse {
                meeting: true,
                dictation: false,
            })
            .expect_err("refused while a meeting is under way");
        assert_eq!(refusal, MEETING_REFUSAL);
        assert!(refusal.starts_with("Not now: "), "{refusal}");
        assert!(
            checks.begin(NOTHING).is_ok(),
            "a refusal leaves no check running"
        );
    }

    #[test]
    fn a_check_is_refused_while_a_dictation_records() {
        let checks = MicrophoneChecks::default();
        let refusal = checks
            .begin(InUse {
                meeting: false,
                dictation: true,
            })
            .expect_err("refused while a dictation records");
        assert_eq!(refusal, DICTATION_REFUSAL);
        assert!(refusal.starts_with("Not now: "), "{refusal}");
    }

    /// One at a time, and the next may begin once the first has ended,
    /// however it ended: the guard is dropped on every way out of the
    /// command.
    #[test]
    fn one_check_at_a_time() {
        let checks = MicrophoneChecks::default();
        let first = checks.begin(NOTHING).expect("the first check begins");
        let refusal = checks.begin(NOTHING).expect_err("a second is refused");
        assert_eq!(refusal, ALREADY_REFUSAL);
        assert!(refusal.starts_with("Not now: "), "{refusal}");
        drop(first);
        assert!(checks.begin(NOTHING).is_ok(), "the next one begins");
    }

    /// A room with nobody speaking, and a microphone that is muted, both
    /// read as nothing heard, at their own peak.
    #[test]
    fn silence_and_a_quiet_hum_are_not_heard() {
        assert_eq!(summary(&[]), (false, 0.0));
        assert_eq!(summary(&[0.0; 50]), (false, 0.0));
        let hum = [0.2_f32; 50];
        assert_eq!(summary(&hum), (false, 0.2), "under a quarter of the meter");
    }

    /// A knock is loud and short: one reading is not someone speaking.
    #[test]
    fn a_single_knock_is_not_heard_but_is_the_peak() {
        let mut levels = [0.0_f32; 50];
        levels[20] = 0.9;
        assert_eq!(summary(&levels), (false, 0.9));
    }

    /// Speech: several readings at a quarter of the meter or more, not
    /// necessarily in a row.
    #[test]
    fn speaking_is_heard_with_its_peak() {
        let levels = [0.0, 0.3, 0.1, 0.5, 0.25, 0.05, 0.0];
        assert_eq!(summary(&levels), (true, 0.5));
        assert_eq!(
            summary(&[0.3, 0.3]),
            (false, 0.3),
            "200 ms of sound is not enough"
        );
    }

    /// Which microphone stood in for the one chosen, as the page says it.
    #[test]
    fn a_choice_that_was_not_opened_is_named_as_fallen_back_from() {
        assert_eq!(fell_back(None, Some("Microphone Array")), None, "no choice");
        assert_eq!(
            fell_back(Some("Headset"), Some("Headset")),
            None,
            "the choice opened"
        );
        assert_eq!(
            fell_back(Some("Headset"), Some("Microphone Array")).as_deref(),
            Some("Headset")
        );
        assert_eq!(
            fell_back(Some("Headset"), None).as_deref(),
            Some("Headset"),
            "a choice is found by its name, so an unnamed device is not it"
        );
    }

    /// The whole check on this machine's real default microphone, for a
    /// person to run with `--ignored` (KNEG-378): it opens, is named, sends
    /// levels and a last 0, and closes. Ignored so `cargo test` never opens
    /// the microphone on its own; on a machine with none, it fails to open
    /// and says so.
    #[tokio::test]
    #[ignore = "opens the real microphone"]
    async fn the_real_default_microphone_is_checked_and_closed() {
        let mut sent = Vec::new();
        let check = match listen(
            None,
            Duration::from_secs(1),
            Duration::from_millis(100),
            |level| sent.push(level),
        )
        .await
        {
            Ok(check) => check,
            Err(err) => {
                eprintln!("no microphone to check: {err}");
                return;
            }
        };
        eprintln!("{check:?}; sent {sent:?}");
        assert!(check.device.is_some(), "the default is named");
        assert_eq!(check.fell_back, None, "nothing was chosen");
        assert!((0.0..=1.0).contains(&check.peak_level));
        assert_eq!(sent.last(), Some(&0.0), "the meter is emptied at the end");

        let missing = listen(
            Some("No such microphone (KNEG-364)".to_owned()),
            Duration::from_millis(300),
            Duration::from_millis(100),
            |_| {},
        )
        .await
        .expect("the default stands in");
        assert_eq!(
            missing.fell_back.as_deref(),
            Some("No such microphone (KNEG-364)")
        );
        assert_eq!(missing.device, check.device, "the default was tested");
    }
}
