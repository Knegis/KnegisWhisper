//! Silero voice-activity-detection settings, and the decision about when to use
//! them at all.
//!
//! Ported from upstream's `whisperVadConfig.js` plus `constants/whisperVad.json`.
//! The values are passed through to whisper.cpp, so they are data rather than
//! logic — but the *clamping* is logic, and so is the enabled-by-context rule,
//! which is the interesting part.
//!
//! ## Why dictation does not use VAD by default
//!
//! Upstream disables Silero VAD for dictation and enables it for notes and
//! meetings. That looks arbitrary until you read the reason (their issue #1454):
//! on a pause-heavy dictation, VAD can strip out the speech, leaving whisper
//! decoding near-silence — and because the custom dictionary is injected as a
//! decoder prompt, whisper then returns *the dictionary words* as the transcript.
//! The user says one sentence and gets a list of their own jargon back.
//!
//! Long-form contexts keep VAD because skipping several minutes of silence in a
//! meeting recording is worth a great deal and the same failure is far less
//! likely across an hour of audio.
//!
//! This is exactly the kind of decision that gets lost in a port, so it is
//! encoded in [`silero_enabled_for`] with the reasoning attached rather than
//! left as a default someone later "tidies up".
//!
//! ## Where it is used
//!
//! Nowhere, from the port until KNEG-404 (2026-10-06): the rule had no caller
//! and every long recording went to whisper whole. Since then
//! [`silero_enabled_for`] decides which recordings find their speech first, and
//! [`VadConfig`] is what whisper.cpp's `whisper-vad-speech-segments` is given
//! (`kw_transcription_core::speech`, through the desktop's
//! `local_model::SpeechTools`). Not the server's own VAD, which was measured
//! putting row boundaries up to 30 s from their speech. No preferences are
//! stored yet, so every context gets its default: meetings and imports on,
//! dictation off.

use serde::{Deserialize, Serialize};

/// What the audio is being recorded for. Decides the VAD default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Context {
    /// Short, hotkey-driven, often pause-heavy.
    Dictation,
    /// A recorded note.
    NoteRecording,
    /// A meeting, potentially hours long.
    Meeting,
}

/// Silero VAD parameters, as whisper.cpp takes them.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VadConfig {
    pub threshold: f32,
    pub min_speech_duration_ms: u32,
    pub min_silence_duration_ms: u32,
    pub max_speech_duration_s: u32,
    pub speech_pad_ms: u32,
    pub samples_overlap: f32,
}

impl Default for VadConfig {
    /// Upstream's defaults, unchanged.
    fn default() -> Self {
        Self {
            threshold: 0.5,
            min_speech_duration_ms: 250,
            min_silence_duration_ms: 200,
            max_speech_duration_s: 30,
            speech_pad_ms: 100,
            samples_overlap: 0.5,
        }
    }
}

/// The bounds each field is clamped to, from upstream's `LIMITS`.
const THRESHOLD: (f32, f32) = (0.1, 0.95);
const MIN_SPEECH_MS: (u32, u32) = (50, 2_000);
const MIN_SILENCE_MS: (u32, u32) = (50, 2_000);
const MAX_SPEECH_S: (u32, u32) = (5, 120);
const SPEECH_PAD_MS: (u32, u32) = (0, 1_000);
const SAMPLES_OVERLAP: (f32, f32) = (0.0, 0.95);

impl VadConfig {
    /// Clamp every field into range, replacing anything non-finite with the
    /// default.
    ///
    /// Called on every value that arrives from settings, because whisper.cpp does
    /// not validate these and an out-of-range threshold produces silently
    /// terrible segmentation rather than an error.
    #[must_use]
    pub fn sanitised(self) -> Self {
        let default = Self::default();

        let clamp_f32 = |value: f32, fallback: f32, (min, max): (f32, f32)| {
            if value.is_finite() {
                value.clamp(min, max)
            } else {
                fallback
            }
        };

        Self {
            threshold: clamp_f32(self.threshold, default.threshold, THRESHOLD),
            min_speech_duration_ms: self
                .min_speech_duration_ms
                .clamp(MIN_SPEECH_MS.0, MIN_SPEECH_MS.1),
            min_silence_duration_ms: self
                .min_silence_duration_ms
                .clamp(MIN_SILENCE_MS.0, MIN_SILENCE_MS.1),
            max_speech_duration_s: self
                .max_speech_duration_s
                .clamp(MAX_SPEECH_S.0, MAX_SPEECH_S.1),
            speech_pad_ms: self.speech_pad_ms.clamp(SPEECH_PAD_MS.0, SPEECH_PAD_MS.1),
            samples_overlap: clamp_f32(
                self.samples_overlap,
                default.samples_overlap,
                SAMPLES_OVERLAP,
            ),
        }
    }

    /// Whether these values are already in range.
    #[must_use]
    pub fn is_sanitised(self) -> bool {
        self.sanitised() == self
    }
}

/// Per-context VAD preferences, as stored in settings.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SileroPreferences {
    /// Dictation is opt-**in**. See the module documentation.
    pub dictation: Option<bool>,
    /// Notes are opt-**out**.
    pub note_recording: Option<bool>,
    /// Meetings are opt-**out**.
    pub meeting: Option<bool>,
}

/// Whether Silero VAD should run for `context`.
///
/// Note the asymmetry, which is deliberate: dictation must be explicitly turned
/// on, the long-form contexts must be explicitly turned off.
#[must_use]
pub fn silero_enabled_for(context: Context, prefs: SileroPreferences) -> bool {
    match context {
        // Opt-in: the failure mode (whisper returning dictionary words for a
        // pause-heavy dictation) is worse than the silence it would skip.
        Context::Dictation => prefs.dictation == Some(true),
        // Opt-out: skipping extended silence in long recordings is worth a lot.
        Context::NoteRecording => prefs.note_recording != Some(false),
        Context::Meeting => prefs.meeting != Some(false),
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

    #[test]
    fn the_defaults_are_upstreams_and_are_already_in_range() {
        let d = VadConfig::default();
        assert!((d.threshold - 0.5).abs() < f32::EPSILON);
        assert_eq!(d.min_speech_duration_ms, 250);
        assert_eq!(d.min_silence_duration_ms, 200);
        assert_eq!(d.max_speech_duration_s, 30);
        assert_eq!(d.speech_pad_ms, 100);
        assert!((d.samples_overlap - 0.5).abs() < f32::EPSILON);
        assert!(d.is_sanitised());
    }

    #[test]
    fn out_of_range_values_are_clamped_rather_than_rejected() {
        // whisper.cpp accepts these without complaint and segments badly, so
        // clamping is the only way the user finds out something sensible happens.
        let wild = VadConfig {
            threshold: 5.0,
            min_speech_duration_ms: 0,
            min_silence_duration_ms: 99_999,
            max_speech_duration_s: 1,
            speech_pad_ms: 10_000,
            samples_overlap: -3.0,
        };
        let clean = wild.sanitised();

        assert!((clean.threshold - 0.95).abs() < f32::EPSILON);
        assert_eq!(clean.min_speech_duration_ms, 50);
        assert_eq!(clean.min_silence_duration_ms, 2_000);
        assert_eq!(clean.max_speech_duration_s, 5);
        assert_eq!(clean.speech_pad_ms, 1_000);
        assert!((clean.samples_overlap - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn non_finite_floats_fall_back_to_the_default_rather_than_clamping() {
        // Clamping NaN gives NaN, which would reach whisper.cpp.
        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let clean = VadConfig {
                threshold: bad,
                samples_overlap: bad,
                ..VadConfig::default()
            }
            .sanitised();

            assert!(clean.threshold.is_finite(), "threshold stayed {bad}");
            assert!(clean.samples_overlap.is_finite());
            assert!((clean.threshold - 0.5).abs() < f32::EPSILON);
        }
    }

    #[test]
    fn sanitising_is_idempotent() {
        let once = VadConfig {
            threshold: 99.0,
            ..VadConfig::default()
        }
        .sanitised();
        assert_eq!(once.sanitised(), once);
    }

    /* ------------------------------------------------ the context asymmetry */

    #[test]
    fn dictation_vad_is_off_unless_explicitly_turned_on() {
        // The #1454 rule. VAD on a pause-heavy dictation can strip the speech,
        // leaving whisper to decode near-silence seeded with the dictionary
        // prompt -- and return the dictionary words as the transcript.
        assert!(!silero_enabled_for(
            Context::Dictation,
            SileroPreferences::default()
        ));
        assert!(!silero_enabled_for(
            Context::Dictation,
            SileroPreferences {
                dictation: Some(false),
                ..Default::default()
            }
        ));
        assert!(silero_enabled_for(
            Context::Dictation,
            SileroPreferences {
                dictation: Some(true),
                ..Default::default()
            }
        ));
    }

    #[test]
    fn long_form_vad_is_on_unless_explicitly_turned_off() {
        for context in [Context::NoteRecording, Context::Meeting] {
            assert!(
                silero_enabled_for(context, SileroPreferences::default()),
                "{context:?} should default to on"
            );
        }

        assert!(!silero_enabled_for(
            Context::NoteRecording,
            SileroPreferences {
                note_recording: Some(false),
                ..Default::default()
            }
        ));
        assert!(!silero_enabled_for(
            Context::Meeting,
            SileroPreferences {
                meeting: Some(false),
                ..Default::default()
            }
        ));
    }

    #[test]
    fn a_preference_for_one_context_does_not_affect_another() {
        let prefs = SileroPreferences {
            dictation: Some(true),
            note_recording: Some(false),
            meeting: None,
        };
        assert!(silero_enabled_for(Context::Dictation, prefs));
        assert!(!silero_enabled_for(Context::NoteRecording, prefs));
        assert!(silero_enabled_for(Context::Meeting, prefs));
    }

    #[test]
    fn the_config_round_trips_through_json_with_camel_case_keys() {
        let json = serde_json::to_value(VadConfig::default()).unwrap();
        for key in [
            "threshold",
            "minSpeechDurationMs",
            "minSilenceDurationMs",
            "maxSpeechDurationS",
            "speechPadMs",
            "samplesOverlap",
        ] {
            assert!(json.get(key).is_some(), "missing {key}");
        }
        let back: VadConfig = serde_json::from_value(json).unwrap();
        assert_eq!(back, VadConfig::default());
    }
}
