//! Turning captured samples into the one audio format everything downstream
//! accepts: 16-bit signed PCM, mono, in a WAV container.
//!
//! Every transcription provider in the registry takes this, and so do
//! whisper.cpp and sherpa-onnx, so converting once here means there is no
//! per-provider transcoding to get wrong.
//!
//! Two conversions happen, and both have a way of being done badly:
//!
//! * **Downmixing to mono.** Averaging channels is right; taking channel 0 is
//!   not. Many laptops present a stereo device where one channel is a
//!   noise-cancellation reference that is nearly silent, so picking channel 0 is
//!   a coin flip on whether you get audio at all.
//!
//! * **Resampling to 16 kHz.** Whisper models are trained at 16 kHz and
//!   whisper.cpp resamples internally, but sending 48 kHz means three times the
//!   upload for no accuracy. The resampler here is linear interpolation, which is
//!   adequate for speech going into a speech model and has no dependencies.

/// The sample rate every model here expects.
pub const TARGET_SAMPLE_RATE: u32 = 16_000;

/// Downmix interleaved samples to mono by averaging channels.
///
/// A trailing partial frame is dropped rather than padded: half a frame is a
/// capture artefact, and padding it with silence puts a click in the audio.
#[must_use]
pub fn to_mono(interleaved: &[f32], channels: u16) -> Vec<f32> {
    if channels <= 1 {
        return interleaved.to_vec();
    }

    let channels = usize::from(channels);
    let frames = interleaved.len() / channels;
    let mut mono = Vec::with_capacity(frames);

    for frame in 0..frames {
        let start = frame * channels;
        let sum: f32 = interleaved
            .get(start..start + channels)
            .map_or(0.0, |slice| slice.iter().sum());
        #[allow(clippy::cast_precision_loss)]
        mono.push(sum / channels as f32);
    }

    mono
}

/// Resample mono audio to [`TARGET_SAMPLE_RATE`] by linear interpolation.
///
/// Returns the input unchanged when it is already at the target rate, so the
/// common case costs nothing.
#[must_use]
pub fn resample_to_target(samples: &[f32], from_rate: u32) -> Vec<f32> {
    if from_rate == TARGET_SAMPLE_RATE || from_rate == 0 || samples.len() < 2 {
        return samples.to_vec();
    }

    let ratio = f64::from(TARGET_SAMPLE_RATE) / f64::from(from_rate);
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_precision_loss,
        clippy::cast_sign_loss
    )]
    let out_len = ((samples.len() as f64) * ratio).round().max(1.0) as usize;

    let mut out = Vec::with_capacity(out_len);
    let last = samples.len() - 1;

    for i in 0..out_len {
        #[allow(clippy::cast_precision_loss)]
        let source = (i as f64) / ratio;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let index = source.floor() as usize;

        if index >= last {
            out.push(*samples.get(last).unwrap_or(&0.0));
            continue;
        }

        #[allow(clippy::cast_possible_truncation)]
        let fraction = (source - source.floor()) as f32;
        let a = *samples.get(index).unwrap_or(&0.0);
        let b = *samples.get(index + 1).unwrap_or(&0.0);
        out.push(a + (b - a) * fraction);
    }

    out
}

/// Encode mono `f32` samples as a 16-bit PCM WAV.
///
/// Samples are clamped to `-1.0..=1.0` before scaling. Without the clamp, a
/// sample above 1.0 — which mixing or gain can easily produce — wraps around and
/// becomes a loud click rather than clipping quietly.
#[must_use]
pub fn encode_wav(samples: &[f32], sample_rate: u32) -> Vec<u8> {
    let mut pcm = Vec::with_capacity(samples.len() * 2);
    for sample in samples {
        let clamped = sample.clamp(-1.0, 1.0);
        // 32767 rather than 32768: scaling by 32768 makes -1.0 valid and +1.0
        // overflow by one.
        #[allow(clippy::cast_possible_truncation)]
        let scaled = (clamped * 32_767.0) as i16;
        pcm.extend_from_slice(&scaled.to_le_bytes());
    }

    wrap_pcm(&pcm, sample_rate, 1)
}

/// Wrap raw little-endian 16-bit PCM in a WAV container.
#[must_use]
pub fn wrap_pcm(pcm: &[u8], sample_rate: u32, channels: u16) -> Vec<u8> {
    let data_len = u32::try_from(pcm.len()).unwrap_or(u32::MAX);
    let byte_rate = sample_rate * u32::from(channels) * 2;
    let block_align = channels * 2;

    let mut out = Vec::with_capacity(pcm.len() + 44);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36_u32.saturating_add(data_len)).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16_u32.to_le_bytes());
    out.extend_from_slice(&1_u16.to_le_bytes()); // PCM
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&byte_rate.to_le_bytes());
    out.extend_from_slice(&block_align.to_le_bytes());
    out.extend_from_slice(&16_u16.to_le_bytes()); // bits per sample
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    out.extend_from_slice(pcm);
    out
}

/// Prepare captured audio for transcription: mono, 16 kHz, WAV.
#[must_use]
pub fn prepare(interleaved: &[f32], channels: u16, sample_rate: u32) -> Vec<u8> {
    let mono = to_mono(interleaved, channels);
    let resampled = resample_to_target(&mono, sample_rate);
    encode_wav(&resampled, TARGET_SAMPLE_RATE)
}

/// How long `samples` lasts at `sample_rate`.
#[must_use]
pub fn duration_ms(sample_count: usize, sample_rate: u32) -> u64 {
    if sample_rate == 0 {
        return 0;
    }
    (sample_count as u64 * 1_000) / u64::from(sample_rate)
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

    fn u32_at(bytes: &[u8], offset: usize) -> u32 {
        u32::from_le_bytes([
            bytes[offset],
            bytes[offset + 1],
            bytes[offset + 2],
            bytes[offset + 3],
        ])
    }

    fn u16_at(bytes: &[u8], offset: usize) -> u16 {
        u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
    }

    /* ---------------------------------------------------------------- mono */

    #[test]
    fn stereo_is_averaged_not_channel_picked() {
        // The realistic failure this guards: a laptop where the second channel
        // is a near-silent noise-cancellation reference. Averaging halves the
        // level; picking channel 1 would give near silence.
        let interleaved = [1.0, 0.0, 1.0, 0.0, 1.0, 0.0];
        assert_eq!(to_mono(&interleaved, 2), vec![0.5, 0.5, 0.5]);
    }

    #[test]
    fn mono_input_is_returned_unchanged() {
        let samples = [0.1, -0.2, 0.3];
        assert_eq!(to_mono(&samples, 1), samples.to_vec());
        assert_eq!(to_mono(&samples, 0), samples.to_vec());
    }

    #[test]
    fn a_trailing_partial_frame_is_dropped_rather_than_padded() {
        // Padding with silence would put a click at the end of every recording.
        let interleaved = [1.0, 1.0, 1.0];
        assert_eq!(
            to_mono(&interleaved, 2),
            vec![1.0],
            "the lone third sample is dropped"
        );
    }

    #[test]
    fn averages_more_than_two_channels() {
        let interleaved = [0.0, 0.5, 1.0, 0.5];
        assert_eq!(to_mono(&interleaved, 4), vec![0.5]);
    }

    /* ----------------------------------------------------------- resampling */

    #[test]
    fn audio_already_at_the_target_rate_is_untouched() {
        let samples = vec![0.1, 0.2, 0.3];
        assert_eq!(resample_to_target(&samples, TARGET_SAMPLE_RATE), samples);
    }

    #[test]
    fn downsampling_produces_the_expected_length() {
        // 48 kHz to 16 kHz is one sample in three.
        let samples: Vec<f32> = (0..4_800).map(|i| (i % 100) as f32 / 100.0).collect();
        let out = resample_to_target(&samples, 48_000);
        assert_eq!(out.len(), 1_600);
    }

    #[test]
    fn upsampling_produces_the_expected_length() {
        let samples: Vec<f32> = (0..800).map(|i| (i % 10) as f32 / 10.0).collect();
        let out = resample_to_target(&samples, 8_000);
        assert_eq!(out.len(), 1_600);
    }

    #[test]
    fn a_constant_signal_survives_resampling_unchanged() {
        // Interpolation between equal values must not drift.
        let samples = vec![0.5_f32; 4_800];
        let out = resample_to_target(&samples, 48_000);
        assert!(
            out.iter().all(|s| (s - 0.5).abs() < 1e-6),
            "interpolation drifted"
        );
    }

    #[test]
    fn resampling_degenerate_input_does_not_panic() {
        assert!(resample_to_target(&[], 48_000).is_empty());
        assert_eq!(resample_to_target(&[0.5], 48_000), vec![0.5]);
        assert_eq!(resample_to_target(&[0.5, 0.5], 0), vec![0.5, 0.5]);
    }

    /* -------------------------------------------------------------- encoding */

    #[test]
    fn the_wav_header_describes_the_data_that_follows() {
        let samples = vec![0.0_f32; 16_000];
        let wav = encode_wav(&samples, TARGET_SAMPLE_RATE);

        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(&wav[12..16], b"fmt ");
        assert_eq!(u32_at(&wav, 16), 16, "fmt chunk size");
        assert_eq!(u16_at(&wav, 20), 1, "PCM");
        assert_eq!(u16_at(&wav, 22), 1, "mono");
        assert_eq!(u32_at(&wav, 24), 16_000, "sample rate");
        assert_eq!(u32_at(&wav, 28), 32_000, "byte rate");
        assert_eq!(u16_at(&wav, 32), 2, "block align");
        assert_eq!(u16_at(&wav, 34), 16, "bits per sample");
        assert_eq!(&wav[36..40], b"data");

        let data_len = u32_at(&wav, 40) as usize;
        assert_eq!(data_len, samples.len() * 2);
        assert_eq!(wav.len(), 44 + data_len);
        assert_eq!(u32_at(&wav, 4) as usize, 36 + data_len, "RIFF size");
    }

    #[test]
    fn samples_above_full_scale_clip_rather_than_wrapping() {
        // Without the clamp these wrap to large negative values, which is an
        // audible click rather than clipping.
        let wav = encode_wav(&[2.0, -2.0, 1.0, -1.0], TARGET_SAMPLE_RATE);
        let pcm = &wav[44..];

        let sample = |i: usize| i16::from_le_bytes([pcm[i * 2], pcm[i * 2 + 1]]);
        assert_eq!(sample(0), 32_767);
        assert_eq!(sample(1), -32_767);
        assert_eq!(sample(2), 32_767);
        assert_eq!(sample(3), -32_767);
    }

    #[test]
    fn full_scale_does_not_overflow() {
        // Scaling by 32768 would make +1.0 overflow i16.
        let wav = encode_wav(&[1.0], TARGET_SAMPLE_RATE);
        let pcm = &wav[44..];
        assert_eq!(i16::from_le_bytes([pcm[0], pcm[1]]), 32_767);
    }

    #[test]
    fn silence_encodes_to_zeroes() {
        let wav = encode_wav(&[0.0; 100], TARGET_SAMPLE_RATE);
        assert!(wav[44..].iter().all(|b| *b == 0));
    }

    #[test]
    fn an_empty_recording_still_produces_a_valid_header() {
        let wav = encode_wav(&[], TARGET_SAMPLE_RATE);
        assert_eq!(wav.len(), 44);
        assert_eq!(u32_at(&wav, 40), 0, "zero-length data chunk");
    }

    /* -------------------------------------------------------------- prepare */

    #[test]
    fn prepare_downmixes_resamples_and_encodes_in_one_step() {
        // One second of 48 kHz stereo becomes one second of 16 kHz mono. The
        // two channels differ so the downmix is actually exercised: left is
        // 0.75, right is 0.25, so mono must come out at 0.5.
        let interleaved: Vec<f32> = (0..48_000 * 2)
            .map(|i| if i % 2 == 0 { 0.75 } else { 0.25 })
            .collect();
        let wav = prepare(&interleaved, 2, 48_000);

        assert_eq!(u32_at(&wav, 24), TARGET_SAMPLE_RATE);
        assert_eq!(u16_at(&wav, 22), 1);
        // 16000 samples at 2 bytes each, plus the header.
        assert_eq!(wav.len(), 44 + 16_000 * 2);

        // And the downmixed level is the average of the two channels.
        let first = i16::from_le_bytes([wav[44], wav[45]]);
        let expected = (0.5_f32 * 32_767.0) as i16;
        assert!(
            (first - expected).abs() <= 1,
            "got {first}, expected about {expected}"
        );
    }

    #[test]
    fn a_realistic_recording_is_small_enough_to_matter() {
        // The reason for resampling: 30 seconds at 48 kHz stereo is 5.5 MB;
        // at 16 kHz mono it is under a megabyte.
        let thirty_seconds: Vec<f32> = vec![0.1; 48_000 * 2 * 30];
        let wav = prepare(&thirty_seconds, 2, 48_000);
        assert!(wav.len() < 1_000_000, "{} bytes", wav.len());
    }

    /* -------------------------------------------------------------- duration */

    #[test]
    fn duration_is_computed_from_the_sample_count() {
        assert_eq!(duration_ms(16_000, 16_000), 1_000);
        assert_eq!(duration_ms(8_000, 16_000), 500);
        assert_eq!(duration_ms(0, 16_000), 0);
        assert_eq!(
            duration_ms(100, 0),
            0,
            "a zero rate must not divide by zero"
        );
    }
}
