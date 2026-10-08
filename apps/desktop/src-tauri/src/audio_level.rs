//! The live sound-level meters' one event (KNEG-371).
//!
//! A capture's [`LevelReader`] is read here, ten times a second, on a task of
//! its own and never on the audio thread: the capture callback only keeps the
//! loudest block in an atomic (`kw_audio_core::level`), and everything that
//! can wait -- reading it, deciding whether it changed, the IPC -- is this
//! module's.
//!
//! One publisher per capture, started by whoever opened it: dictation's
//! microphone, a meeting's microphone and its system audio, and the Meetings
//! microphone check. It needs no stopping. When the capture is finished,
//! cancelled or dropped, its reader answers `None`, and the publisher sends a
//! last 0 so the meter empties and ends. A meter therefore cannot outlive the
//! capture it shows, whichever way that capture ended.
//!
//! The meters are decoration (`aria-hidden`), not `role=meter`: ten updates a
//! second cannot be announced usefully, and "Listening…" and "Recording"
//! already say that a capture is running (ruling, 2026-10-07).

use std::time::Duration;

use kw_audio_core::LevelReader;
use serde::Serialize;
use tauri::{AppHandle, Emitter as _};
use ts_rs::TS;

/// A capture's level. Carries an [`AudioLevel`].
pub const LEVEL_EVENT: &str = "audio-level";

/// How often a publisher reads its capture's level.
pub const TICK: Duration = Duration::from_millis(100);

/// The smallest change worth sending: a hundredth of the meter, 0.6 dB.
const MIN_CHANGE: f32 = 0.01;

/// Ticks an unchanged, audible level may go unsent before it is sent again.
///
/// The page treats a level it has not heard for 400 ms as stale and shows
/// silence (`lib/levelMeter.ts`), which is right for a capture that went away
/// without its last 0 arriving and wrong for a steady sound whose level does
/// not move by [`MIN_CHANGE`]. Two ticks, 200 ms, keeps a steady sound well
/// inside that. Silence is never repeated, nor is a level under
/// [`MIN_CHANGE`]: the page shows both as nothing without being told.
const REPEAT_AFTER_TICKS: u32 = 2;

/// What a level is for, so each meter listens only to its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum LevelSession {
    Dictation,
    Meeting,
    /// The 5-second test on the Meetings page (KNEG-364,
    /// `microphone_check.rs`).
    MicrophoneCheck,
}

/// Which capture a level is from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum LevelSource {
    Microphone,
    /// What the speakers play: the other people in a meeting.
    System,
}

/// One reading of one capture's level.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AudioLevel {
    pub session: LevelSession,
    pub source: LevelSource,
    /// 0 at −60 dBFS or quieter, 1 at full scale, linear in decibels
    /// (`kw_audio_core::level_from_rms`). The loudest block since the last
    /// reading, so a short sound is not missed between two.
    pub level: f32,
}

/// Send `reader`'s level as [`LEVEL_EVENT`] until its capture is gone.
///
/// Spawns and returns at once. Usable for any capture, whoever opened it --
/// the session and source are only labels on the event.
pub fn publish(app: &AppHandle, session: LevelSession, source: LevelSource, reader: LevelReader) {
    tauri::async_runtime::spawn(run(
        move || reader.take(),
        TICK,
        sender(app, session, source),
    ));
}

/// What sends one capture's levels to the page, labelled with whose they
/// are: for [`publish`], and for a caller that runs [`run`] itself.
pub fn sender(
    app: &AppHandle,
    session: LevelSession,
    source: LevelSource,
) -> impl FnMut(f32) + Send + 'static {
    let app = app.clone();
    move |level| {
        let reading = AudioLevel {
            session,
            source,
            level,
        };
        if let Err(err) = app.emit(LEVEL_EVENT, reading) {
            tracing::debug!(error = %err, "could not send a sound level");
        }
    }
}

/// The publisher's loop, apart from the app so a test can run it.
///
/// `take` reads the level, as [`LevelReader::take`] does: `None` once the
/// capture is gone, which ends the loop. A closure rather than the reader so
/// a caller can keep what it reads as well -- the microphone check sums up
/// every reading (`microphone_check.rs`) -- without a second reader, which
/// would take half of them. Awaited in place there, and spawned by
/// [`publish`].
///
/// A tick that comes late is skipped rather than made up: a burst of stale
/// readings after a stall would only move the meter for nothing.
pub async fn run(
    mut take: impl FnMut() -> Option<f32>,
    every: Duration,
    mut send: impl FnMut(f32),
) {
    let mut ticks = tokio::time::interval(every);
    ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut pacing = Pacing::default();
    loop {
        ticks.tick().await;
        match pacing.next(take()) {
            Step::Send(level) => send(level),
            Step::Hold => {}
            Step::Stop => {
                send(0.0);
                return;
            }
        }
    }
}

/// What one tick does.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Step {
    Send(f32),
    Hold,
    /// The capture is gone: send a last 0 and end.
    Stop,
}

/// What a publisher remembers between ticks.
#[derive(Debug, Default)]
struct Pacing {
    /// The level last sent. The page starts at 0, so this does too.
    shown: f32,
    /// Ticks since it was sent.
    held: u32,
}

impl Pacing {
    fn next(&mut self, taken: Option<f32>) -> Step {
        let Some(level) = taken else {
            return Step::Stop;
        };
        let moved = (level - self.shown).abs() >= MIN_CHANGE;
        // Only a level the page can show is repeated: one under a hundredth
        // of the meter lights nothing, and going stale changes nothing.
        let repeat = self.shown >= MIN_CHANGE && self.held + 1 >= REPEAT_AFTER_TICKS;
        if moved || repeat {
            self.shown = level;
            self.held = 0;
            Step::Send(level)
        } else {
            self.held = self.held.saturating_add(1);
            Step::Hold
        }
    }
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
    use kw_audio_core::{level_from_rms, LevelMeter};

    /// The brief's one hard requirement of the publisher: when the capture
    /// stops, the meter is told 0 and the task ends. The capture here is
    /// dropped from inside the first send, so the very next tick finds it
    /// gone.
    #[tokio::test]
    async fn the_publisher_sends_a_final_zero_and_stops_when_the_capture_goes() {
        let meter = LevelMeter::new();
        meter.record(0.1);
        let reader = meter.reader();
        let mut capture = Some(meter);
        let mut sent = Vec::new();

        tokio::time::timeout(
            Duration::from_secs(5),
            run(
                || reader.take(),
                Duration::from_millis(1),
                |level| {
                    sent.push(level);
                    capture = None;
                },
            ),
        )
        .await
        .expect("the publisher stops once its capture is dropped");

        assert_eq!(sent, vec![level_from_rms(0.1), 0.0]);
    }

    #[test]
    fn a_reader_that_answers_none_stops_the_publisher() {
        let mut pacing = Pacing::default();
        assert_eq!(pacing.next(Some(0.5)), Step::Send(0.5));
        assert_eq!(pacing.next(None), Step::Stop);
    }

    /// Only a change of a hundredth of the meter or more is sent.
    #[test]
    fn a_level_is_sent_when_it_moves_by_a_hundredth() {
        let mut pacing = Pacing::default();
        assert_eq!(pacing.next(Some(0.5)), Step::Send(0.5));
        assert_eq!(pacing.next(Some(0.505)), Step::Hold, "under 0.01");
        assert_eq!(pacing.next(Some(0.52)), Step::Send(0.52));
        assert_eq!(pacing.next(Some(0.3)), Step::Send(0.3), "falling counts");
    }

    /// Silence from the start sends nothing, and goes on sending nothing: the
    /// page shows 0 without being told.
    #[test]
    fn silence_is_not_sent() {
        let mut pacing = Pacing::default();
        for _ in 0..10 {
            assert_eq!(pacing.next(Some(0.0)), Step::Hold);
        }
        assert_eq!(
            pacing.next(Some(0.005)),
            Step::Hold,
            "under 0.01 of silence"
        );
    }

    /// A steady sound is sent again every 200 ms, so the page's 400 ms
    /// staleness does not empty a meter that is still hearing something.
    #[test]
    fn a_steady_sound_is_sent_again_before_the_page_calls_it_stale() {
        let mut pacing = Pacing::default();
        assert_eq!(pacing.next(Some(0.5)), Step::Send(0.5));
        assert_eq!(pacing.next(Some(0.5)), Step::Hold);
        assert_eq!(pacing.next(Some(0.5)), Step::Send(0.5));
        assert_eq!(pacing.next(Some(0.5)), Step::Hold);
        assert_eq!(pacing.next(Some(0.5)), Step::Send(0.5));
        assert!(
            u64::from(REPEAT_AFTER_TICKS) * TICK.as_millis() as u64 <= 200,
            "the page calls a level stale after 400 ms"
        );
    }

    /// A level too small to light anything is sent when it is reached, as
    /// any change is, and then not again: only a level the page can show is
    /// worth repeating against its staleness.
    #[test]
    fn a_level_too_small_to_show_is_not_repeated() {
        let mut pacing = Pacing::default();
        assert_eq!(pacing.next(Some(0.5)), Step::Send(0.5));
        assert_eq!(
            pacing.next(Some(0.005)),
            Step::Send(0.005),
            "falling counts"
        );
        for _ in 0..6 {
            assert_eq!(pacing.next(Some(0.005)), Step::Hold);
        }
    }

    /// Falling silent is sent once, then not again.
    #[test]
    fn going_quiet_is_sent_once() {
        let mut pacing = Pacing::default();
        assert_eq!(pacing.next(Some(0.5)), Step::Send(0.5));
        assert_eq!(pacing.next(Some(0.0)), Step::Send(0.0));
        assert_eq!(pacing.next(Some(0.0)), Step::Hold);
        assert_eq!(pacing.next(Some(0.0)), Step::Hold);
        assert_eq!(pacing.next(Some(0.0)), Step::Hold);
    }

    /// The payload as the page reads it, with the microphone check's session.
    #[test]
    fn a_reading_serializes_as_the_page_reads_it() {
        let json = serde_json::to_value(AudioLevel {
            session: LevelSession::MicrophoneCheck,
            source: LevelSource::System,
            level: 0.5,
        })
        .unwrap();
        assert_eq!(
            json,
            serde_json::json!({ "session": "microphone-check", "source": "system", "level": 0.5 })
        );
        assert_eq!(
            serde_json::to_value(LevelSession::Dictation).unwrap(),
            "dictation"
        );
        assert_eq!(
            serde_json::to_value(LevelSession::Meeting).unwrap(),
            "meeting"
        );
        assert_eq!(
            serde_json::to_value(LevelSource::Microphone).unwrap(),
            "microphone"
        );
    }
}
