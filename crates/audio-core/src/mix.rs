//! Combining two recordings of the same moment into one track.
//!
//! A meeting has two sources: the microphone, and whatever the speakers are
//! playing. They are captured by separate streams at separate sample rates,
//! started a few milliseconds apart, and they have to become one 16 kHz mono
//! track before anything can transcribe them.
//!
//! Everything here is pure. That is the point of putting it in its own module —
//! system-audio capture cannot be tested on a machine with no loopback device,
//! and the part that decides what the mixed audio *sounds* like can be.
//!
//! # The three decisions
//!
//! **Alignment is at the start, not by timestamp.** Both streams begin when
//! recording begins. Aligning them by wall-clock timestamps would need a shared
//! clock the two callbacks do not have, and would be wrong by the buffer latency
//! of each device anyway. Starting both and lining up sample zero is accurate to
//! within one buffer, which is tens of milliseconds — inaudible in a transcript.
//!
//! **The shorter track is padded, never the longer truncated.** The two streams
//! stop a few milliseconds apart, and the last words of a sentence are usually
//! at the end. Truncating to the shorter one to make the arithmetic tidy would
//! throw away the part most likely to matter.
//!
//! **Sum first, limit only if it clips.** Dividing by the number of tracks is
//! the obvious way to avoid overflow and it halves the volume of every passage
//! where only one source is active — which is most of a meeting, because people
//! do not usually talk over the audio the whole time. Summing and applying gain
//! only when the peak actually exceeds full scale keeps the common case at full
//! volume.

use crate::wav;

/// One captured stream, as its device produced it.
#[derive(Debug, Clone, PartialEq)]
pub struct Track {
    /// Interleaved samples, as captured.
    pub samples: Vec<f32>,
    pub channels: u16,
    pub sample_rate: u32,
}

impl Track {
    /// A mono track already at the target rate. For tests and for callers that
    /// have done their own conversion.
    #[must_use]
    pub fn mono(samples: Vec<f32>) -> Self {
        Self {
            samples,
            channels: 1,
            sample_rate: wav::TARGET_SAMPLE_RATE,
        }
    }

    /// Whether this track carries any audio at all.
    ///
    /// An empty track is skipped by [`mix`] rather than mixed as silence. The
    /// difference matters: a system-audio stream that never received a callback
    /// would otherwise pull the headroom calculation around for no reason.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// Mono, at [`wav::TARGET_SAMPLE_RATE`].
    fn normalised(&self) -> Vec<f32> {
        let mono = wav::to_mono(&self.samples, self.channels);
        wav::resample_to_target(&mono, self.sample_rate)
    }
}

/// What mixing did, so a caller can say something true about it.
#[derive(Debug, Clone, PartialEq)]
pub struct Mixed {
    /// Mono, 16 kHz.
    pub samples: Vec<f32>,
    /// How many tracks actually contributed audio.
    pub sources: usize,
    /// The gain applied to keep the sum inside full scale. `1.0` means none was
    /// needed, which is the common case.
    pub gain: f32,
    /// The peak before any gain. Above `1.0` means the sum would have clipped.
    pub peak: f32,
}

/// Mix tracks captured over the same period into one mono 16 kHz track.
///
/// Empty tracks are skipped. With nothing to mix the result is empty rather than
/// an error: a meeting where the microphone was muted and nothing played is a
/// real thing, and it is the gate's job to say there was no speech.
#[must_use]
pub fn mix(tracks: &[Track]) -> Mixed {
    let normalised: Vec<Vec<f32>> = tracks
        .iter()
        .filter(|t| !t.is_empty())
        .map(Track::normalised)
        .collect();

    if normalised.is_empty() {
        return Mixed {
            samples: Vec::new(),
            sources: 0,
            gain: 1.0,
            peak: 0.0,
        };
    }

    // Padded to the longest, not cut to the shortest.
    let length = normalised.iter().map(Vec::len).max().unwrap_or(0);

    let mut summed = vec![0.0_f32; length];
    for track in &normalised {
        for (out, sample) in summed.iter_mut().zip(track.iter()) {
            *out += *sample;
        }
    }

    let peak = summed.iter().fold(0.0_f32, |worst, s| worst.max(s.abs()));

    // Only when it would clip. A meeting where one source is quiet must not be
    // attenuated for the sake of a moment where both were loud.
    let gain = if peak > 1.0 { 1.0 / peak } else { 1.0 };
    if gain < 1.0 {
        for sample in &mut summed {
            *sample *= gain;
        }
    }

    Mixed {
        samples: summed,
        sources: normalised.len(),
        gain,
        peak,
    }
}

/// How long a mixed track lasts.
///
/// Mono at the target rate, so this is just the sample count — but going through
/// [`wav::duration_ms`] keeps the one place that knows the arithmetic.
#[must_use]
pub fn duration_ms(mixed: &Mixed) -> u64 {
    wav::duration_ms(mixed.samples.len(), wav::TARGET_SAMPLE_RATE)
}

/// Mix and encode, ready for transcription.
#[must_use]
pub fn mix_to_wav(tracks: &[Track]) -> Vec<u8> {
    let mixed = mix(tracks);
    // Already mono at the target rate, so `prepare` only encodes.
    wav::prepare(&mixed.samples, 1, wav::TARGET_SAMPLE_RATE)
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

    /// Nothing to mix is an empty result, not an error.
    #[test]
    fn mixing_nothing_produces_nothing() {
        assert_eq!(mix(&[]).sources, 0);
        assert!(mix(&[]).samples.is_empty());

        // And an empty track is skipped rather than mixed as silence.
        let mixed = mix(&[Track::mono(vec![]), Track::mono(vec![0.5, 0.5])]);
        assert_eq!(mixed.sources, 1);
        assert_eq!(mixed.samples, vec![0.5, 0.5]);
    }

    /// One track passes through untouched.
    ///
    /// The case that matters most, because it is what a meeting with no system
    /// audio produces — and a mixer that quietly halved it would make every
    /// solo recording quieter than before the mixer existed.
    #[test]
    fn a_single_track_is_not_attenuated() {
        let mixed = mix(&[Track::mono(vec![0.9, -0.9, 0.4])]);
        assert_eq!(mixed.gain, 1.0);
        assert_eq!(mixed.samples, vec![0.9, -0.9, 0.4]);
        assert!((mixed.peak - 0.9).abs() < 1e-6);
    }

    /// Two quiet tracks sum without being scaled down.
    #[test]
    fn two_tracks_that_fit_are_summed_at_full_volume() {
        let mixed = mix(&[Track::mono(vec![0.3, 0.3]), Track::mono(vec![0.2, -0.1])]);
        assert_eq!(mixed.gain, 1.0);
        assert_eq!(mixed.sources, 2);
        assert!((mixed.samples[0] - 0.5).abs() < 1e-6);
        assert!((mixed.samples[1] - 0.2).abs() < 1e-6);
    }

    /// A sum that would clip is scaled to exactly full scale, not below it.
    #[test]
    fn a_clipping_sum_is_brought_back_to_full_scale() {
        let mixed = mix(&[Track::mono(vec![0.9, 0.0]), Track::mono(vec![0.9, 0.0])]);

        assert!((mixed.peak - 1.8).abs() < 1e-6, "{}", mixed.peak);
        assert!(mixed.gain < 1.0);

        let loudest = mixed.samples.iter().fold(0.0_f32, |w, s| w.max(s.abs()));
        assert!(
            (loudest - 1.0).abs() < 1e-5,
            "should sit at full scale, got {loudest}"
        );
    }

    /// The shorter track is padded; the longer one keeps its tail.
    ///
    /// The end of a recording is where the end of a sentence is. Cutting to the
    /// shorter stream to make the lengths match would throw that away.
    #[test]
    fn the_longer_track_keeps_its_tail() {
        let mixed = mix(&[
            Track::mono(vec![0.1, 0.1, 0.1, 0.1]),
            Track::mono(vec![0.2, 0.2]),
        ]);

        assert_eq!(mixed.samples.len(), 4, "the tail must survive");
        assert!((mixed.samples[0] - 0.3).abs() < 1e-6);
        assert!((mixed.samples[1] - 0.3).abs() < 1e-6);
        // Past the shorter track, only the longer one contributes.
        assert!((mixed.samples[2] - 0.1).abs() < 1e-6);
        assert!((mixed.samples[3] - 0.1).abs() < 1e-6);
    }

    /// Tracks at different rates and channel counts are reconciled first.
    #[test]
    fn different_rates_and_channel_counts_are_normalised_before_summing() {
        // Stereo at 32 kHz: four frames, which downmix to four mono samples and
        // then halve to two at 16 kHz.
        let stereo = Track {
            samples: vec![1.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0, 0.0],
            channels: 2,
            sample_rate: 32_000,
        };
        let mixed = mix(&[stereo]);

        assert_eq!(mixed.sources, 1);
        // Downmixed to 0.5 by averaging the silent right channel.
        assert!(
            mixed.samples.iter().all(|s| (*s - 0.5).abs() < 1e-3),
            "{:?}",
            mixed.samples
        );
        assert_eq!(mixed.samples.len(), 2, "{:?}", mixed.samples);
    }

    /// A mixed track encodes to a WAV the transcription path already accepts.
    #[test]
    fn mixing_produces_a_wav_at_the_target_rate() {
        let wav = mix_to_wav(&[Track::mono(vec![0.1; 160]), Track::mono(vec![0.1; 160])]);

        // RIFF header, then the target sample rate at offset 24.
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        let rate = u32::from_le_bytes([wav[24], wav[25], wav[26], wav[27]]);
        assert_eq!(rate, wav::TARGET_SAMPLE_RATE);
    }

    /// A real 48 kHz stereo loopback capture reduces to the expected length.
    ///
    /// Not a hypothetical: this is the shape a WASAPI loopback stream actually
    /// produced on the development machine — 97 920 interleaved samples, two
    /// channels at 48 kHz, which is 1.02 seconds. The first attempt at the
    /// probe printed a *sample* count and read as though a second of audio had
    /// become a third of one, so the arithmetic is pinned here.
    #[test]
    fn a_real_stereo_forty_eight_kilohertz_capture_keeps_its_duration() {
        let captured = Track {
            samples: vec![0.25; 97_920],
            channels: 2,
            sample_rate: 48_000,
        };

        let mixed = mix(&[captured]);

        // 97 920 / 2 channels = 48 960 frames = 1.02s, which at 16 kHz is
        // 16 320 samples.
        assert_eq!(mixed.samples.len(), 16_320);
        assert_eq!(duration_ms(&mixed), 1_020);
    }

    /// The report says how many sources contributed and whether it clipped.
    ///
    /// Not decoration: "the meeting was recorded with system audio" and "the
    /// system audio device produced nothing" look identical in the transcript,
    /// and a screen that cannot tell them apart will claim the first.
    #[test]
    fn the_report_distinguishes_a_missing_source_from_a_silent_one() {
        let missing = mix(&[Track::mono(vec![0.5; 10]), Track::mono(vec![])]);
        assert_eq!(missing.sources, 1);

        let silent = mix(&[Track::mono(vec![0.5; 10]), Track::mono(vec![0.0; 10])]);
        assert_eq!(silent.sources, 2);
    }
}
