//! The local speech gate: deciding whether a recording contains speech at all.
//!
//! A direct port of upstream's `localSpeechGate.ts`. Its purpose is to avoid
//! sending silence to a transcription provider — which costs money, costs time,
//! and returns a hallucinated transcript often enough to matter.
//!
//! The design decision worth preserving is which way it fails. **Recording no
//! windows fails the gate open**, and so does a measurement that cannot be
//! taken. Skipping a real dictation because the analyser stalled is far worse
//! than sending a second of silence: the user spoke, saw nothing appear, and has
//! no way to tell why.
//!
//! Note that this is *not* voice activity detection in the Silero sense. It does
//! not segment audio or trim silence; it answers one question about a whole
//! recording. See [`crate::vad`] for the segmenting kind, and for why dictation
//! deliberately does not use it.

use serde::Serialize;
use ts_rs::TS;

/// Below this peak RMS the recording is silence.
const SILENCE_RMS: f32 = 0.002;
/// A window must reach this RMS to count as speech.
const SPEECH_WINDOW_RMS: f32 = 0.003;
/// ...and this peak amplitude. Both, not either: a DC offset raises RMS without
/// any speech in it.
const SPEECH_WINDOW_PEAK: f32 = 0.02;
/// A peak RMS this high counts as speech even if no single window qualified,
/// which covers quiet speech spread thinly across windows.
const STRONG_SPEECH_RMS: f32 = 0.006;

/// Accumulates measurements across a recording.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SpeechGate {
    peak_rms: f32,
    peak_amplitude: f32,
    window_count: u32,
    speech_window_count: u32,
    consecutive_speech_windows: u32,
    max_consecutive_speech_windows: u32,
}

/// What the gate concluded, and the numbers behind it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GateDecision {
    /// Whether to skip transcription entirely.
    pub skip: bool,
    pub reason: GateReason,
    pub peak_rms: f32,
    pub peak_amplitude: f32,
    pub window_count: u32,
    pub speech_window_count: u32,
    pub max_consecutive_speech_windows: u32,
}

/// Why the gate decided what it did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum GateReason {
    /// Nothing was measured. Fails open — see the module documentation.
    Unavailable,
    /// The whole recording is below the silence floor.
    Silence,
    /// Audible, but nothing that looks like speech.
    InsufficientSpeech,
    /// Speech found; transcribe it.
    SpeechDetected,
}

impl SpeechGate {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Record one analysis window.
    ///
    /// `rms` and `peak` are both in `0.0..=1.0`, as computed from normalised
    /// samples.
    pub fn observe(&mut self, rms: f32, peak: f32) {
        self.window_count = self.window_count.saturating_add(1);
        self.peak_rms = self.peak_rms.max(rms);
        self.peak_amplitude = self.peak_amplitude.max(peak);

        // Both thresholds, not either. RMS alone is fooled by a DC offset or a
        // steady hum; peak alone is fooled by a single click.
        let is_speech = rms >= SPEECH_WINDOW_RMS && peak >= SPEECH_WINDOW_PEAK;

        if !is_speech {
            self.consecutive_speech_windows = 0;
            return;
        }

        self.speech_window_count = self.speech_window_count.saturating_add(1);
        self.consecutive_speech_windows = self.consecutive_speech_windows.saturating_add(1);
        self.max_consecutive_speech_windows = self
            .max_consecutive_speech_windows
            .max(self.consecutive_speech_windows);
    }

    /// Observe a block of samples, computing its RMS and peak.
    pub fn observe_samples(&mut self, samples: &[f32]) {
        if samples.is_empty() {
            return;
        }

        let mut sum_squares = 0.0_f64;
        let mut peak = 0.0_f32;
        for sample in samples {
            sum_squares += f64::from(*sample) * f64::from(*sample);
            peak = peak.max(sample.abs());
        }

        #[allow(clippy::cast_possible_truncation)]
        let rms = (sum_squares / samples.len() as f64).sqrt() as f32;

        self.observe(rms, peak);
    }

    /// Decide whether to transcribe.
    #[must_use]
    pub fn decide(&self) -> GateDecision {
        let base = |skip: bool, reason: GateReason| GateDecision {
            skip,
            reason,
            peak_rms: self.peak_rms,
            peak_amplitude: self.peak_amplitude,
            window_count: self.window_count,
            speech_window_count: self.speech_window_count,
            max_consecutive_speech_windows: self.max_consecutive_speech_windows,
        };

        // Nothing measured: fail open. A stalled analyser must not silently
        // discard a dictation the user actually spoke.
        if self.window_count == 0 {
            return base(false, GateReason::Unavailable);
        }

        if self.peak_rms < SILENCE_RMS {
            return base(true, GateReason::Silence);
        }

        let has_speech = self.speech_window_count >= 1 || self.peak_rms >= STRONG_SPEECH_RMS;
        if !has_speech {
            return base(true, GateReason::InsufficientSpeech);
        }

        base(false, GateReason::SpeechDetected)
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

    /// A block of samples at a given amplitude, as a sine so RMS and peak differ
    /// the way they do in real audio.
    fn tone(amplitude: f32, len: usize) -> Vec<f32> {
        (0..len)
            .map(|i| {
                #[allow(clippy::cast_precision_loss)]
                let t = i as f32 / 16_000.0;
                (t * 440.0 * std::f32::consts::TAU).sin() * amplitude
            })
            .collect()
    }

    #[test]
    fn nothing_measured_fails_open() {
        // The most important property here. If the analyser never ran we must
        // transcribe anyway; discarding a dictation the user spoke is far worse
        // than transcribing a second of silence.
        let decision = SpeechGate::new().decide();
        assert!(!decision.skip);
        assert_eq!(decision.reason, GateReason::Unavailable);
    }

    #[test]
    fn digital_silence_is_skipped() {
        let mut gate = SpeechGate::new();
        for _ in 0..40 {
            gate.observe_samples(&vec![0.0; 512]);
        }
        let decision = gate.decide();
        assert!(decision.skip);
        assert_eq!(decision.reason, GateReason::Silence);
        assert_eq!(decision.window_count, 40);
    }

    #[test]
    fn ordinary_speech_level_audio_is_transcribed() {
        let mut gate = SpeechGate::new();
        for _ in 0..30 {
            // Well above both window thresholds.
            gate.observe_samples(&tone(0.25, 512));
        }
        let decision = gate.decide();
        assert!(!decision.skip, "{decision:?}");
        assert_eq!(decision.reason, GateReason::SpeechDetected);
        assert!(decision.speech_window_count > 0);
    }

    #[test]
    fn faint_room_noise_is_audible_but_not_speech() {
        let mut gate = SpeechGate::new();
        // Above the silence floor, below the speech thresholds.
        for _ in 0..30 {
            gate.observe(0.0025, 0.008);
        }
        let decision = gate.decide();
        assert!(decision.skip);
        assert_eq!(decision.reason, GateReason::InsufficientSpeech);
    }

    #[test]
    fn a_single_speech_window_is_enough() {
        // Someone saying one short word.
        let mut gate = SpeechGate::new();
        for _ in 0..30 {
            gate.observe(0.0025, 0.008);
        }
        gate.observe(0.05, 0.3);
        assert!(!gate.decide().skip);
        assert_eq!(gate.decide().speech_window_count, 1);
    }

    #[test]
    fn strong_overall_level_counts_even_with_no_qualifying_window() {
        // Quiet speech spread thinly: no single window passes the peak test, but
        // the overall RMS is clearly not silence.
        let mut gate = SpeechGate::new();
        for _ in 0..30 {
            gate.observe(0.007, 0.015);
        }
        let decision = gate.decide();
        assert_eq!(decision.speech_window_count, 0, "no window qualified");
        assert!(
            !decision.skip,
            "but the strong-RMS rule should still let it through"
        );
        assert_eq!(decision.reason, GateReason::SpeechDetected);
    }

    #[test]
    fn both_thresholds_are_required_for_a_speech_window() {
        // A steady hum: high RMS, low peak. Should not count.
        let mut hum = SpeechGate::new();
        hum.observe(0.004, 0.005);
        assert_eq!(
            hum.speech_window_count, 0,
            "high RMS with no peak is not speech"
        );

        // A single click: high peak, negligible RMS.
        let mut click = SpeechGate::new();
        click.observe(0.0005, 0.9);
        assert_eq!(click.speech_window_count, 0, "a click is not speech");
    }

    #[test]
    fn consecutive_speech_windows_reset_on_a_quiet_window() {
        let mut gate = SpeechGate::new();
        gate.observe(0.05, 0.3);
        gate.observe(0.05, 0.3);
        gate.observe(0.05, 0.3);
        assert_eq!(gate.max_consecutive_speech_windows, 3);

        gate.observe(0.0, 0.0);
        gate.observe(0.05, 0.3);

        assert_eq!(gate.consecutive_speech_windows, 1, "the run restarted");
        assert_eq!(
            gate.max_consecutive_speech_windows, 3,
            "the maximum is remembered"
        );
    }

    #[test]
    fn an_empty_sample_block_is_not_a_window() {
        // Otherwise an empty read would count as a silent window and could tip a
        // real dictation into being skipped.
        let mut gate = SpeechGate::new();
        gate.observe_samples(&[]);
        assert_eq!(gate.window_count, 0);
        assert_eq!(gate.decide().reason, GateReason::Unavailable);
    }

    #[test]
    fn rms_and_peak_are_computed_correctly_for_a_known_signal() {
        let mut gate = SpeechGate::new();
        // A square wave at ±0.5: RMS is exactly 0.5, peak is exactly 0.5.
        let samples: Vec<f32> = (0..1000)
            .map(|i| if i % 2 == 0 { 0.5 } else { -0.5 })
            .collect();
        gate.observe_samples(&samples);
        assert!(
            (gate.peak_rms - 0.5).abs() < 1e-6,
            "rms was {}",
            gate.peak_rms
        );
        assert!((gate.peak_amplitude - 0.5).abs() < 1e-6);
    }

    #[test]
    fn the_decision_serializes_with_the_metrics_that_explain_it() {
        let mut gate = SpeechGate::new();
        gate.observe_samples(&tone(0.25, 512));
        let json = serde_json::to_value(gate.decide()).unwrap();
        for key in [
            "skip",
            "reason",
            "peakRms",
            "peakAmplitude",
            "windowCount",
            "speechWindowCount",
            "maxConsecutiveSpeechWindows",
        ] {
            assert!(json.get(key).is_some(), "missing {key}");
        }
    }
}
