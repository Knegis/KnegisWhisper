//! Microphone capture.
//!
//! Deliberately the thinnest part of this crate. Everything worth testing — the
//! speech gate, the downmix, the resample, the encode — lives elsewhere and is
//! tested without a device. What is left here is device selection and a callback
//! that appends samples, which cannot be tested on a machine with no microphone
//! and should therefore be as small as possible.
//!
//! One decision that is not obvious: the recording buffer is **bounded**, and it
//! stops accepting samples rather than growing. A dictation hotkey that is
//! pressed and never released — because the key was rebound, or the release
//! event was swallowed by another application — would otherwise allocate until
//! the process dies. Thirty minutes of 16 kHz mono is about 115 MB, which is a
//! reasonable ceiling for something described as dictation.

use std::sync::{Arc, Mutex};
use ts_rs::TS;

use cpal::traits::{DeviceTrait as _, HostTrait as _, StreamTrait as _};

use crate::gate::{GateDecision, SpeechGate};
use crate::wav;

/// The longest recording that will be kept, in seconds.
///
/// See the module documentation for why this is a hard stop and not a warning.
pub const MAX_RECORDING_SECONDS: u32 = 30 * 60;

/// Failures from capturing audio.
#[derive(Debug, thiserror::Error)]
pub enum CaptureError {
    #[error("no input device is available. Check that a microphone is connected and that this application is allowed to use it.")]
    NoInputDevice,

    #[error("the input device reported no usable configuration: {0}")]
    NoConfig(String),

    #[error("could not open the input stream: {0}")]
    Stream(String),

    #[error("the input device uses the sample format {0}, which is not supported")]
    UnsupportedFormat(String),

    #[error("recording is already in progress")]
    AlreadyRecording,

    #[error("no recording is in progress")]
    NotRecording,
}

/// An audio input device the user can choose.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct InputDevice {
    pub name: String,
    pub is_default: bool,
}

/// List the input devices.
///
/// Errors from individual devices are skipped rather than failing the whole
/// list: one misbehaving virtual device should not make the picker empty.
pub fn input_devices() -> Result<Vec<InputDevice>, CaptureError> {
    let host = cpal::default_host();
    let default_name = host
        .default_input_device()
        .and_then(|d| d.name().ok())
        .unwrap_or_default();

    let devices = host
        .input_devices()
        .map_err(|e| CaptureError::NoConfig(e.to_string()))?;

    Ok(devices
        .filter_map(|device| device.name().ok())
        .map(|name| InputDevice {
            is_default: name == default_name,
            name,
        })
        .collect())
}

/// What a finished recording produced.
#[derive(Debug, Clone)]
pub struct Recording {
    /// 16-bit mono PCM at 16 kHz, in a WAV container.
    pub wav: Vec<u8>,
    pub duration_ms: u64,
    /// Whether the audio contains speech, and the numbers behind that.
    pub gate: GateDecision,
    /// True when the recording hit [`MAX_RECORDING_SECONDS`] and was cut short.
    pub truncated: bool,
}

/// The buffer a capture callback writes into.
#[derive(Debug)]
struct Buffer {
    samples: Vec<f32>,
    gate: SpeechGate,
    capacity: usize,
    truncated: bool,
}

impl Buffer {
    fn new(sample_rate: u32, channels: u16) -> Self {
        let capacity = (sample_rate as usize)
            .saturating_mul(usize::from(channels.max(1)))
            .saturating_mul(MAX_RECORDING_SECONDS as usize);

        Self {
            samples: Vec::new(),
            gate: SpeechGate::new(),
            capacity,
            truncated: false,
        }
    }

    /// Append what fits. Once full, stop rather than grow.
    fn push(&mut self, data: &[f32]) {
        self.gate.observe_samples(data);

        let remaining = self.capacity.saturating_sub(self.samples.len());
        if remaining == 0 {
            self.truncated = true;
            return;
        }

        if data.len() <= remaining {
            self.samples.extend_from_slice(data);
        } else {
            self.samples
                .extend_from_slice(data.get(..remaining).unwrap_or(&[]));
            self.truncated = true;
        }
    }
}

/// A microphone recording in progress.
///
/// Dropping this stops the stream, so a cancelled dictation cannot leave the
/// microphone open — which would leave the operating system's recording
/// indicator lit and rightly alarm the user.
pub struct Capture {
    stream: cpal::Stream,
    buffer: Arc<Mutex<Buffer>>,
    sample_rate: u32,
    channels: u16,
}

impl std::fmt::Debug for Capture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Capture")
            .field("sample_rate", &self.sample_rate)
            .field("channels", &self.channels)
            .finish_non_exhaustive()
    }
}

impl Capture {
    /// Start recording from `device_name`, or the default device.
    pub fn start(device_name: Option<&str>) -> Result<Self, CaptureError> {
        let host = cpal::default_host();

        let device = match device_name {
            Some(wanted) => host
                .input_devices()
                .map_err(|e| CaptureError::NoConfig(e.to_string()))?
                .find(|d| d.name().is_ok_and(|n| n == wanted))
                // Falling back to the default is right: a device that has been
                // unplugged should not stop dictation working.
                .or_else(|| host.default_input_device())
                .ok_or(CaptureError::NoInputDevice)?,
            None => host
                .default_input_device()
                .ok_or(CaptureError::NoInputDevice)?,
        };

        let config = device
            .default_input_config()
            .map_err(|e| CaptureError::NoConfig(e.to_string()))?;

        let sample_rate = config.sample_rate().0;
        let channels = config.channels();
        let buffer = Arc::new(Mutex::new(Buffer::new(sample_rate, channels)));

        tracing::info!(
            device = device.name().unwrap_or_else(|_| "<unnamed>".to_owned()),
            sample_rate,
            channels,
            format = ?config.sample_format(),
            "starting microphone capture"
        );

        let stream = Self::build_stream(&device, &config, Arc::clone(&buffer))?;
        stream
            .play()
            .map_err(|e| CaptureError::Stream(e.to_string()))?;

        Ok(Self {
            stream,
            buffer,
            sample_rate,
            channels,
        })
    }

    fn build_stream(
        device: &cpal::Device,
        config: &cpal::SupportedStreamConfig,
        buffer: Arc<Mutex<Buffer>>,
    ) -> Result<cpal::Stream, CaptureError> {
        // A device error mid-recording is logged, not propagated: there is
        // nowhere to propagate to from an audio callback, and the samples
        // already captured are still worth transcribing.
        let on_error = |err: cpal::StreamError| {
            tracing::error!(error = %err, "microphone stream error");
        };

        let stream_config = config.config();

        macro_rules! build {
            ($sample:ty, $convert:expr) => {{
                let buffer = Arc::clone(&buffer);
                device.build_input_stream(
                    &stream_config,
                    move |data: &[$sample], _| {
                        let converted: Vec<f32> = data.iter().copied().map($convert).collect();
                        // Recover from a poisoned lock rather than panicking in
                        // an audio callback.
                        let mut guard = buffer.lock().unwrap_or_else(|e| e.into_inner());
                        guard.push(&converted);
                    },
                    on_error,
                    None,
                )
            }};
        }

        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => build!(f32, |s| s),
            cpal::SampleFormat::I16 => build!(i16, |s| f32::from(s) / 32_768.0),
            cpal::SampleFormat::U16 => build!(u16, |s| (f32::from(s) - 32_768.0) / 32_768.0),
            other => return Err(CaptureError::UnsupportedFormat(format!("{other:?}"))),
        };

        stream.map_err(|e| CaptureError::Stream(e.to_string()))
    }

    /// Stop recording and return the audio, prepared for transcription.
    pub fn finish(self) -> Recording {
        // Pause before reading so no further samples arrive mid-copy.
        if let Err(err) = self.stream.pause() {
            tracing::debug!(error = %err, "could not pause the stream before finishing");
        }

        let guard = self.buffer.lock().unwrap_or_else(|e| e.into_inner());
        let frames = guard.samples.len() / usize::from(self.channels.max(1));

        Recording {
            wav: wav::prepare(&guard.samples, self.channels, self.sample_rate),
            duration_ms: wav::duration_ms(frames, self.sample_rate),
            gate: guard.gate.decide(),
            truncated: guard.truncated,
        }
    }

    /// How long the recording has been running.
    #[must_use]
    pub fn elapsed_ms(&self) -> u64 {
        let guard = self.buffer.lock().unwrap_or_else(|e| e.into_inner());
        let frames = guard.samples.len() / usize::from(self.channels.max(1));
        wav::duration_ms(frames, self.sample_rate)
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
    fn the_buffer_stops_growing_at_the_cap() {
        // The runaway-hotkey guard. A press with no release must not allocate
        // until the process dies.
        let mut buffer = Buffer::new(16_000, 1);
        let one_second = vec![0.1_f32; 16_000];

        for _ in 0..(MAX_RECORDING_SECONDS + 60) {
            buffer.push(&one_second);
        }

        assert_eq!(
            buffer.samples.len(),
            buffer.capacity,
            "the buffer grew past its cap"
        );
        assert!(
            buffer.truncated,
            "truncation should be reported, not silent"
        );
    }

    #[test]
    fn a_partial_block_at_the_cap_is_kept_up_to_the_limit() {
        let mut buffer = Buffer::new(10, 1);
        buffer.capacity = 5;
        buffer.push(&[0.1, 0.2, 0.3]);
        assert_eq!(buffer.samples.len(), 3);
        assert!(!buffer.truncated);

        buffer.push(&[0.4, 0.5, 0.6, 0.7]);
        assert_eq!(buffer.samples.len(), 5, "only what fits is kept");
        assert!(buffer.truncated);
    }

    #[test]
    fn the_gate_keeps_measuring_even_after_the_buffer_is_full() {
        // Otherwise a long recording that fills up would report the speech
        // characteristics of only its first half hour.
        let mut buffer = Buffer::new(10, 1);
        buffer.capacity = 2;
        buffer.push(&[0.0, 0.0]);
        let windows_before = buffer.gate.decide().window_count;

        buffer.push(&vec![0.5_f32; 512]);
        assert!(
            buffer.gate.decide().window_count > windows_before,
            "the gate should keep observing"
        );
    }

    #[test]
    fn the_cap_scales_with_the_channel_count() {
        // A stereo device produces twice the samples per second, so the cap in
        // *samples* has to be twice as large to represent the same duration.
        let mono = Buffer::new(16_000, 1);
        let stereo = Buffer::new(16_000, 2);
        assert_eq!(stereo.capacity, mono.capacity * 2);
    }

    #[test]
    fn thirty_minutes_of_audio_is_a_sane_ceiling() {
        let buffer = Buffer::new(16_000, 1);
        let bytes = buffer.capacity * 2;
        assert!(bytes > 50_000_000, "the cap should not cut off real use");
        assert!(bytes < 200_000_000, "but it should still bound memory");
    }

    #[test]
    fn a_zero_channel_device_does_not_divide_by_zero() {
        // cpal should never report this, but the arithmetic must not panic if it
        // does.
        let buffer = Buffer::new(16_000, 0);
        assert!(buffer.capacity > 0);
    }

    #[test]
    fn listing_devices_does_not_fail_on_a_machine_without_a_microphone() {
        // CI has no audio device. The list may be empty; it must not error.
        match input_devices() {
            Ok(devices) => {
                // At most one device may claim to be the default.
                assert!(devices.iter().filter(|d| d.is_default).count() <= 1);
            }
            Err(err) => {
                // A host with no audio subsystem at all is also acceptable.
                assert!(matches!(
                    err,
                    CaptureError::NoConfig(_) | CaptureError::NoInputDevice
                ));
            }
        }
    }
}
