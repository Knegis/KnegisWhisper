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
//!
//! [`encode_wav`] assembles a whole recording in memory, which is right for a
//! dictation of a few seconds. [`WavWriter`] is the same format written straight
//! through to a file, which is what a meeting needs: four hours of 48 kHz stereo
//! `f32` is gigabytes before it is encoded at all. Both build the header with
//! [`header`], so there is one description of the layout rather than two.

use std::fs;
use std::io::{self, Read as _, Seek as _, SeekFrom, Write as _};
use std::path::Path;

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
/// The whisper case, which is most of them. [`resample`] is the same thing for a
/// provider that wants a different rate — OpenAI Realtime takes 24 kHz only, and
/// treating 16 kHz as universal is how a stream of chipmunks gets sent to it.
#[must_use]
pub fn resample_to_target(samples: &[f32], from_rate: u32) -> Vec<f32> {
    resample(samples, from_rate, TARGET_SAMPLE_RATE)
}

/// Resample mono audio between any two rates by linear interpolation.
///
/// Returns the input unchanged when the rates match, so the common case costs
/// nothing. Linear interpolation is adequate for speech going into a speech
/// model and has no dependencies; it is not adequate for music, and nothing here
/// carries music.
#[must_use]
pub fn resample(samples: &[f32], from_rate: u32, to_rate: u32) -> Vec<f32> {
    if from_rate == to_rate || from_rate == 0 || to_rate == 0 || samples.len() < 2 {
        return samples.to_vec();
    }

    let ratio = f64::from(to_rate) / f64::from(from_rate);
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

/// Convert mono `f32` samples to little-endian 16-bit PCM, appending to `out`.
///
/// Samples are clamped to `-1.0..=1.0` before scaling. Without the clamp, a
/// sample above 1.0 — which mixing or gain can easily produce — wraps around and
/// becomes a loud click rather than clipping quietly.
fn pcm_bytes(samples: &[f32], out: &mut Vec<u8>) {
    for sample in samples {
        let clamped = sample.clamp(-1.0, 1.0);
        // 32767 rather than 32768: scaling by 32768 makes -1.0 valid and +1.0
        // overflow by one.
        #[allow(clippy::cast_possible_truncation)]
        let scaled = (clamped * 32_767.0) as i16;
        out.extend_from_slice(&scaled.to_le_bytes());
    }
}

/// The 44-byte canonical WAV header for `data_len` bytes of PCM.
///
/// One builder for both writers. `data_len` is not known until the last frame
/// arrives, so [`WavWriter`] passes zero here and patches the two fields
/// afterwards — which is only safe while the layout lives in one place.
#[must_use]
fn header(data_len: u32, sample_rate: u32, channels: u16) -> [u8; HEADER_LEN] {
    let byte_rate = sample_rate * u32::from(channels) * 2;
    let block_align = channels * 2;

    let mut out = Vec::with_capacity(HEADER_LEN);
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

    let mut fixed = [0_u8; HEADER_LEN];
    fixed.copy_from_slice(&out);
    fixed
}

/// Length of the canonical header the two writers share.
const HEADER_LEN: usize = 44;
/// Where the RIFF chunk size sits, for a writer that has to go back and patch it.
const RIFF_SIZE_OFFSET: u64 = 4;
/// Where the data chunk size sits, same reason.
const DATA_SIZE_OFFSET: u64 = 40;

/// Encode mono `f32` samples as a 16-bit PCM WAV.
#[must_use]
pub fn encode_wav(samples: &[f32], sample_rate: u32) -> Vec<u8> {
    let mut pcm = Vec::with_capacity(samples.len() * 2);
    pcm_bytes(samples, &mut pcm);
    wrap_pcm(&pcm, sample_rate, 1)
}

/// Decode little-endian 16-bit PCM back to `f32`, the inverse of the encoder.
///
/// Needed because audio crosses a boundary as PCM bytes — a meeting provider
/// takes `AudioChunk { pcm, .. }` — and the windowing arithmetic works in `f32`.
/// A trailing odd byte is dropped: half a sample is a framing error, and
/// inventing the other half puts a click in the audio.
#[must_use]
pub fn from_pcm(bytes: &[u8]) -> Vec<f32> {
    let (pairs, _) = bytes.as_chunks::<2>();

    pairs
        .iter()
        .map(|&pair| f32::from(i16::from_le_bytes(pair)) / 32_767.0)
        .collect()
}

/// Wrap raw little-endian 16-bit PCM in a WAV container.
#[must_use]
pub fn wrap_pcm(pcm: &[u8], sample_rate: u32, channels: u16) -> Vec<u8> {
    let data_len = u32::try_from(pcm.len()).unwrap_or(u32::MAX);
    let mut out = Vec::with_capacity(pcm.len() + HEADER_LEN);
    out.extend_from_slice(&header(data_len, sample_rate, channels));
    out.extend_from_slice(pcm);
    out
}

/// A WAV file written while the recording is still going on.
///
/// The counterpart to [`encode_wav`], for audio too long to hold. Frames go
/// through to the file as they arrive and nothing accumulates in memory.
///
/// The header carries two lengths that are unknown until the last frame, so both
/// are written as zero and patched by [`WavWriter::finish`]. A file left behind
/// by a crash therefore has a zero-length `data` chunk, which every decoder
/// reads as an empty recording until the user explicitly recovers it with
/// [`recover_interrupted`]. A cut-short meeting must not appear complete.
///
/// Mono, 16-bit, at whatever rate it is given — the same format everything
/// downstream accepts, so a caller downmixes and resamples before appending.
#[derive(Debug)]
pub struct WavWriter {
    file: fs::File,
    sample_rate: u32,
    samples: u64,
}

impl WavWriter {
    /// Create the file and write a placeholder header.
    pub fn create(path: &Path, sample_rate: u32) -> io::Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        // An unfinished meeting may already live at this name. Never replace
        // its audio while starting a new recording.
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)?;
        file.write_all(&header(0, sample_rate, 1))?;

        Ok(Self {
            file,
            sample_rate,
            samples: 0,
        })
    }

    /// Append mono samples.
    pub fn append(&mut self, samples: &[f32]) -> io::Result<()> {
        if samples.is_empty() {
            return Ok(());
        }

        let mut pcm = Vec::with_capacity(samples.len() * 2);
        pcm_bytes(samples, &mut pcm);
        self.file.write_all(&pcm)?;
        self.samples = self.samples.saturating_add(samples.len() as u64);
        Ok(())
    }

    /// How many samples have been written.
    #[must_use]
    pub const fn samples(&self) -> u64 {
        self.samples
    }

    /// How long the file is so far.
    #[must_use]
    pub fn duration_ms(&self) -> u64 {
        if self.sample_rate == 0 {
            return 0;
        }
        self.samples.saturating_mul(1_000) / u64::from(self.sample_rate)
    }

    /// Patch the two lengths and close the file.
    ///
    /// Returns the sample count, so a caller does not have to ask the filesystem
    /// how long the recording it just made was.
    pub fn finish(mut self) -> io::Result<u64> {
        let data_len = u32::try_from(self.samples.saturating_mul(2)).unwrap_or(u32::MAX);

        self.file.seek(SeekFrom::Start(RIFF_SIZE_OFFSET))?;
        self.file
            .write_all(&(36_u32.saturating_add(data_len)).to_le_bytes())?;
        self.file.seek(SeekFrom::Start(DATA_SIZE_OFFSET))?;
        self.file.write_all(&data_len.to_le_bytes())?;
        self.file.flush()?;

        Ok(self.samples)
    }
}

/// Finish the header of a meeting WAV left open by process termination.
///
/// Accept only the exact format written by `WavWriter`. A normal, already
/// finished file is accepted too: the process may have died after `finish` and
/// before its note was committed. No sample bytes are changed.
pub fn recover_interrupted(path: &Path) -> io::Result<f64> {
    let mut file = fs::OpenOptions::new().read(true).write(true).open(path)?;
    let len = file.metadata()?.len();
    let data_len = len.checked_sub(HEADER_LEN as u64).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "the recording has no WAV header",
        )
    })?;
    if data_len % 2 != 0 || data_len > u64::from(u32::MAX - 36) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "the recording has an incomplete sample or exceeds WAV's size limit",
        ));
    }
    let data_len = u32::try_from(data_len).map_err(io::Error::other)?;
    let expected = header(data_len, TARGET_SAMPLE_RATE, 1);
    let mut actual = [0; HEADER_LEN];
    file.read_exact(&mut actual)?;
    let riff = u32::from_le_bytes(actual[4..8].try_into().map_err(io::Error::other)?);
    let data = u32::from_le_bytes(actual[40..44].try_into().map_err(io::Error::other)?);
    if actual[..4] != expected[..4]
        || actual[8..40] != expected[8..40]
        || riff != 36 && riff != 36 + data_len
        || data != 0 && data != data_len
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "the recording is not an application-owned 16 kHz mono PCM WAV",
        ));
    }
    if riff != 36 + data_len || data != data_len {
        file.seek(SeekFrom::Start(0))?;
        file.write_all(&expected)?;
        file.sync_all()?;
    }
    Ok(f64::from(data_len) / (f64::from(TARGET_SAMPLE_RATE) * 2.0))
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

    /* ------------------------------------------------------ writing to disk */

    #[test]
    fn a_streamed_file_is_byte_identical_to_one_encoded_whole() {
        // The strongest statement available: whatever `encode_wav` produces for a
        // recording, appending the same samples in arbitrary chunks produces the
        // same bytes. If the header patching is wrong by a single field, this
        // fails.
        let samples: Vec<f32> = (0..5_000)
            .map(|i| ((i as f32) / 400.0).sin() * 0.6)
            .collect();

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("streamed.wav");

        let mut writer = WavWriter::create(&path, TARGET_SAMPLE_RATE).unwrap();
        // Deliberately uneven chunks, including an empty one -- a capture
        // callback delivers whatever it has.
        for chunk in samples.chunks(777) {
            writer.append(chunk).unwrap();
        }
        writer.append(&[]).unwrap();
        let written = writer.finish().unwrap();

        assert_eq!(written, samples.len() as u64);
        assert_eq!(
            std::fs::read(&path).unwrap(),
            encode_wav(&samples, TARGET_SAMPLE_RATE)
        );
    }

    #[test]
    fn interrupted_wav_recovers_only_its_header() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("interrupted.wav");
        let samples = [0.25, -0.25, 0.5];
        let mut writer = WavWriter::create(&path, TARGET_SAMPLE_RATE).unwrap();
        writer.append(&samples).unwrap();
        drop(writer);
        assert_eq!(recover_interrupted(&path).unwrap(), 3.0 / 16_000.0);
        assert_eq!(
            std::fs::read(&path).unwrap(),
            encode_wav(&samples, TARGET_SAMPLE_RATE)
        );
        assert_eq!(recover_interrupted(&path).unwrap(), 3.0 / 16_000.0);
    }

    #[test]
    fn recovery_refuses_other_audio_and_does_not_change_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("foreign.wav");
        let bytes = encode_wav(&[0.5], 8_000);
        std::fs::write(&path, &bytes).unwrap();
        assert!(recover_interrupted(&path).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }

    #[test]
    fn starting_a_writer_never_overwrites_an_interrupted_recording() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("meeting.wav");
        let mut first = WavWriter::create(&path, TARGET_SAMPLE_RATE).unwrap();
        first.append(&[0.5]).unwrap();
        drop(first);
        let before = std::fs::read(&path).unwrap();
        assert!(WavWriter::create(&path, TARGET_SAMPLE_RATE).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }

    #[test]
    fn the_two_length_fields_are_patched_and_agree_with_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("lengths.wav");

        let mut writer = WavWriter::create(&path, 16_000).unwrap();
        writer.append(&[0.5; 100]).unwrap();
        writer.finish().unwrap();

        let bytes = std::fs::read(&path).unwrap();
        let riff = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
        let data = u32::from_le_bytes(bytes[40..44].try_into().unwrap());

        assert_eq!(data, 200, "100 samples at 2 bytes each");
        assert_eq!(riff, 236, "36 plus the data length");
        assert_eq!(bytes.len(), HEADER_LEN + 200);
        assert_eq!(
            riff as usize,
            bytes.len() - 8,
            "RIFF counts what follows it"
        );
    }

    #[test]
    fn an_unfinished_file_reads_as_empty_rather_than_as_garbage() {
        // The failure this design chooses. A meeting recording cut short by a
        // crash has a zero-length data chunk, so every decoder reads it as an
        // empty recording -- which is far better than a file that looks complete
        // and is not.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("abandoned.wav");

        let mut writer = WavWriter::create(&path, 16_000).unwrap();
        writer.append(&[0.5; 4_000]).unwrap();
        drop(writer); // no finish

        let bytes = std::fs::read(&path).unwrap();
        assert!(bytes.len() > HEADER_LEN, "the samples are on disk");
        assert_eq!(
            u32::from_le_bytes(bytes[40..44].try_into().unwrap()),
            0,
            "and the header does not claim them"
        );
    }

    #[test]
    fn a_streamed_file_reports_its_own_length_as_it_grows() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("growing.wav");

        let mut writer = WavWriter::create(&path, 16_000).unwrap();
        assert_eq!(writer.duration_ms(), 0);

        writer.append(&[0.0; 16_000]).unwrap();
        assert_eq!(writer.duration_ms(), 1_000);

        writer.append(&[0.0; 8_000]).unwrap();
        assert_eq!(writer.samples(), 24_000);
        assert_eq!(writer.duration_ms(), 1_500);
    }

    #[test]
    fn a_file_longer_than_the_old_in_memory_cap_is_written_without_holding_it() {
        // The reason this type exists. Forty minutes is past
        // `capture::MAX_RECORDING_SECONDS`, and the peak allocation here is one
        // chunk rather than the recording.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("long.wav");

        let chunk = vec![0.25_f32; 16_000];
        let mut writer = WavWriter::create(&path, 16_000).unwrap();
        for _ in 0..(40 * 60) {
            writer.append(&chunk).unwrap();
        }
        let samples = writer.finish().unwrap();

        assert_eq!(samples, 16_000 * 40 * 60);
        let expected = HEADER_LEN as u64 + samples * 2;
        assert_eq!(std::fs::metadata(&path).unwrap().len(), expected);
    }

    /* ------------------------------------------------- resampling generally */

    #[test]
    fn resampling_upwards_lengthens_by_the_ratio() {
        // The case that made this general: OpenAI Realtime takes 24 kHz only.
        let one_second: Vec<f32> = (0..16_000)
            .map(|i| ((i as f32) / 200.0).sin() * 0.5)
            .collect();

        let up = resample(&one_second, 16_000, 24_000);
        assert_eq!(up.len(), 24_000, "one second is still one second");

        let back = resample(&up, 24_000, 16_000);
        assert_eq!(back.len(), 16_000);
    }

    #[test]
    fn resampling_between_equal_rates_is_free_and_exact() {
        let samples = vec![0.1_f32, -0.2, 0.3];
        assert_eq!(resample(&samples, 24_000, 24_000), samples);
    }

    #[test]
    fn a_zero_rate_is_returned_unchanged_rather_than_dividing_by_it() {
        let samples = vec![0.1_f32, 0.2];
        assert_eq!(resample(&samples, 0, 24_000), samples);
        assert_eq!(resample(&samples, 16_000, 0), samples);
    }

    #[test]
    fn the_whisper_helper_is_the_general_one_at_sixteen_kilohertz() {
        // One implementation, so a fix to the interpolation cannot reach one
        // caller and miss the other.
        let samples: Vec<f32> = (0..4_800).map(|i| ((i as f32) / 90.0).sin()).collect();
        assert_eq!(
            resample_to_target(&samples, 48_000),
            resample(&samples, 48_000, TARGET_SAMPLE_RATE)
        );
    }
}
