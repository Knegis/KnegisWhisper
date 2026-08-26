//! Capturing what the speakers are playing.
//!
//! This is what blocks meeting transcription — not the echo-cancellation helper,
//! which only matters when someone is on speakers rather than headphones. Half a
//! meeting is the other people talking, and without this it is not recorded.
//!
//! # The finding that made this small
//!
//! The plan for this module called for the `wasapi` crate on Windows, replacing
//! upstream's 634-line `windows-system-audio-helper.c`. That turned out to be
//! unnecessary: **`cpal`, which this crate already depends on, does WASAPI
//! loopback.** Its own documentation says so — "if you use a WASAPI output device
//! as an input device it will transparently enable loopback mode". So the Windows
//! implementation is: find the default *output* device, open an *input* stream on
//! it. No new dependency, no C, and it reuses the buffer, gate and encode path
//! that microphone capture already uses and that is already tested.
//!
//! The same trick reaches Linux for free. PulseAudio and PipeWire both expose a
//! `.monitor` source per sink, and those appear in `cpal`'s device list as
//! ordinary inputs — so finding one by name is a real mechanism rather than a
//! stub. It is written and **unverified**: there is no Linux machine here.
//!
//! macOS has no equivalent. `AudioHardwareCreateProcessTap` (14.4 and later) or a
//! user-installed virtual device is required, and `cpal` exposes neither.
//! [`support`] says so rather than returning an empty device list and letting the
//! caller guess.
//!
//! # What this module will not do
//!
//! It does not hide that it is recording. The overlay is shown for system audio
//! exactly as it is for the microphone — see `docs/SECURITY.md`. Capturing what
//! the speakers are playing without the local user knowing is the behaviour the
//! brief rules out by name, and the mechanism here would be perfectly capable of
//! it.

use std::sync::{Arc, Mutex};

#[cfg(any(target_os = "windows", target_os = "linux"))]
use cpal::traits::HostTrait as _;
use cpal::traits::{DeviceTrait as _, StreamTrait as _};

use crate::capture::CaptureError;
use crate::mix::Track;

/// Whether this platform can capture system audio, and how.
///
/// Four states rather than a boolean, for the same reason
/// `overlay::CaptureExclusion` has four: "the platform cannot" and "the platform
/// can and this machine has no such device" are different problems with
/// different fixes, and neither is "it works".
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, ts_rs::TS)]
#[serde(
    tag = "state",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum SystemAudioSupport {
    /// A device was found and a stream can be opened on it.
    Available {
        /// The mechanism, named so the user can recognise it.
        mechanism: &'static str,
        /// The device that will be captured.
        device: String,
        /// True where the mechanism has never been run on real hardware by
        /// anyone working on this project. Reported, not hidden.
        unverified: bool,
    },
    /// The platform supports it and this machine offers no such device.
    NoDevice {
        mechanism: &'static str,
        reason: String,
    },
    /// The platform cannot do this at all.
    Unsupported { reason: String },
}

impl SystemAudioSupport {
    /// Whether a caller may offer system audio as an option.
    #[must_use]
    pub const fn is_available(&self) -> bool {
        matches!(self, Self::Available { .. })
    }

    /// A short phrase for a settings screen.
    #[must_use]
    pub const fn summary(&self) -> &'static str {
        match self {
            Self::Available {
                unverified: false, ..
            } => "Available",
            Self::Available {
                unverified: true, ..
            } => "Available, never tested on this platform",
            Self::NoDevice { .. } => "No device on this machine offers it",
            Self::Unsupported { .. } => "Not supported on this system",
        }
    }
}

/// The mechanism this platform would use, and whether anyone has run it.
///
/// A `const` pair rather than three `cfg` blocks in every function, so the
/// per-platform decision is in one place and the rest of the module is not
/// conditional.
const fn mechanism() -> Option<(&'static str, bool)> {
    #[cfg(target_os = "windows")]
    {
        Some(("WASAPI loopback on the default output device", false))
    }
    #[cfg(target_os = "linux")]
    {
        // PulseAudio and PipeWire both publish a `.monitor` source per sink.
        Some(("a PulseAudio or PipeWire monitor source", true))
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        None
    }
}

/// Why macOS cannot, said once.
const MACOS_REASON: &str =
    "macOS has no loopback device by default. Capturing system audio needs either \
     AudioHardwareCreateProcessTap, which is macOS 14.4 and later and is not exposed by the audio \
     library this application uses, or a virtual audio device the user installs themselves. \
     Neither is something this build can arrange, so it says so rather than offering an option \
     that would record silence.";

/// Find the device this platform would capture system audio from.
///
/// Three functions rather than one with three `cfg` blocks inside it, so each
/// body is ordinary code the compiler and the linter see whole. The single-body
/// version needed an early `return` per branch, which reads as dead code on
/// whichever platform is being built.
///
/// Windows: the default *output* device, opened for input, which is how WASAPI
/// loopback is requested. Linux: an input device whose name marks it as a
/// monitor — a monitor source is a real capture endpoint, so nothing special is
/// needed there.
#[cfg(target_os = "windows")]
fn find_device() -> Option<(cpal::Device, String)> {
    let device = cpal::default_host().default_output_device()?;
    let name = device.name().unwrap_or_else(|_| "<unnamed>".to_owned());
    Some((device, name))
}

#[cfg(target_os = "linux")]
fn find_device() -> Option<(cpal::Device, String)> {
    // `.monitor` is the PulseAudio convention and PipeWire keeps it for
    // compatibility. Matched case-insensitively because the description a device
    // reports is not always the source name.
    cpal::default_host()
        .input_devices()
        .ok()?
        .filter_map(|device| {
            let name = device.name().ok()?;
            name.to_lowercase()
                .contains("monitor")
                .then_some((device, name))
        })
        .next()
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
fn find_device() -> Option<(cpal::Device, String)> {
    None
}

/// What this machine can do, checked rather than assumed.
///
/// Enumerates devices, so it is a real answer about this machine rather than a
/// statement about the platform. Cheap enough to call from a status command.
#[must_use]
pub fn support() -> SystemAudioSupport {
    let Some((mechanism, unverified)) = mechanism() else {
        return SystemAudioSupport::Unsupported {
            reason: MACOS_REASON.to_owned(),
        };
    };

    match find_device() {
        Some((_, device)) => SystemAudioSupport::Available {
            mechanism,
            device,
            unverified,
        },
        None => SystemAudioSupport::NoDevice {
            mechanism,
            reason: if cfg!(target_os = "linux") {
                "no input device on this machine has a name containing \"monitor\". A PulseAudio \
                 or PipeWire sink normally publishes one; a bare ALSA setup does not."
                    .to_owned()
            } else {
                "this machine reports no default output device, so there is nothing playing to \
                 capture."
                    .to_owned()
            },
        },
    }
}

/// A running system-audio capture.
///
/// Dropping it stops the stream. Deliberately the same shape as
/// [`crate::capture::Capture`] so the two can be started and finished together
/// without the caller keeping track of which is which.
pub struct SystemCapture {
    stream: cpal::Stream,
    samples: Arc<Mutex<Vec<f32>>>,
    sample_rate: u32,
    channels: u16,
    device: String,
}

impl std::fmt::Debug for SystemCapture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SystemCapture")
            .field("device", &self.device)
            .field("sample_rate", &self.sample_rate)
            .field("channels", &self.channels)
            .finish_non_exhaustive()
    }
}

impl SystemCapture {
    /// Start capturing what the speakers are playing.
    ///
    /// Fails rather than returning an empty capture when the platform cannot do
    /// it: a caller that gets a `SystemCapture` back must be able to rely on it
    /// having recorded something, or the meeting screen will report system audio
    /// was included when it was not.
    pub fn start() -> Result<Self, CaptureError> {
        if mechanism().is_none() {
            return Err(CaptureError::NoConfig(MACOS_REASON.to_owned()));
        }

        let (device, name) = find_device().ok_or(CaptureError::NoInputDevice)?;

        let config = default_config(&device)?;

        let sample_rate = config.sample_rate().0;
        let channels = config.channels();
        let samples = Arc::new(Mutex::new(Vec::new()));

        tracing::info!(
            device = name,
            sample_rate,
            channels,
            format = ?config.sample_format(),
            "starting system audio capture"
        );

        let stream = build_stream(&device, &config, Arc::clone(&samples))?;
        stream
            .play()
            .map_err(|e| CaptureError::Stream(e.to_string()))?;

        Ok(Self {
            stream,
            samples,
            sample_rate,
            channels,
            device: name,
        })
    }

    /// Stop and return what was captured, unmixed.
    ///
    /// A [`Track`] rather than a finished WAV, because mixing has to happen
    /// before encoding and the caller holds the other track.
    #[must_use]
    pub fn finish(self) -> Track {
        if let Err(err) = self.stream.pause() {
            tracing::debug!(error = %err, "could not pause the system audio stream");
        }

        let guard = self.samples.lock().unwrap_or_else(|e| e.into_inner());
        Track {
            samples: guard.clone(),
            channels: self.channels,
            sample_rate: self.sample_rate,
        }
    }

    /// The device being captured, for a status line.
    #[must_use]
    pub fn device(&self) -> &str {
        &self.device
    }
}

/// The stream format to open, which is not the same call on both platforms.
///
/// This is the one thing a compile could not have told us. `cpal` sets
/// `AUDCLNT_STREAMFLAGS_LOOPBACK` inside `build_input_stream` when the device is
/// a render endpoint — but `default_input_config()` refuses a render endpoint
/// outright, with "the requested stream type is not supported by the device".
/// The mix format has to come from `default_output_config()` and then be used to
/// build an *input* stream. Running it found that in one attempt; reading the
/// documentation had not.
fn default_config(device: &cpal::Device) -> Result<cpal::SupportedStreamConfig, CaptureError> {
    #[cfg(target_os = "windows")]
    let config = device.default_output_config();

    // A PulseAudio monitor source is an ordinary capture device.
    #[cfg(not(target_os = "windows"))]
    let config = device.default_input_config();

    config.map_err(|e| CaptureError::NoConfig(e.to_string()))
}

/// The capture cap, shared with microphone capture for the same reason.
///
/// A meeting left running is the same runaway as a dictation hotkey that never
/// releases, and a stereo 48 kHz stream fills memory six times faster than 16 kHz
/// mono does.
fn capacity(sample_rate: u32, channels: u16) -> usize {
    (sample_rate as usize)
        .saturating_mul(usize::from(channels.max(1)))
        .saturating_mul(crate::capture::MAX_RECORDING_SECONDS as usize)
}

fn build_stream(
    device: &cpal::Device,
    config: &cpal::SupportedStreamConfig,
    samples: Arc<Mutex<Vec<f32>>>,
) -> Result<cpal::Stream, CaptureError> {
    let cap = capacity(config.sample_rate().0, config.channels());

    let on_error = |err: cpal::StreamError| {
        tracing::error!(error = %err, "system audio stream error");
    };

    let stream_config = config.config();

    macro_rules! build {
        ($sample:ty, $convert:expr) => {{
            let samples = Arc::clone(&samples);
            device.build_input_stream(
                &stream_config,
                move |data: &[$sample], _| {
                    let mut guard = samples.lock().unwrap_or_else(|e| e.into_inner());
                    // Append what fits and then stop, rather than grow.
                    let room = cap.saturating_sub(guard.len());
                    guard.extend(data.iter().take(room).copied().map($convert));
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

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;

    /// The support report matches the platform this is built for.
    ///
    /// Not a tautology: it asserts that a platform with a mechanism reports one
    /// of the two states that name it, and that a platform without one reports
    /// `Unsupported` — so a future `cfg` block that forgets a platform fails
    /// here rather than silently offering nothing.
    #[test]
    fn the_support_report_matches_the_platform() {
        let support = support();

        if cfg!(any(target_os = "windows", target_os = "linux")) {
            assert!(
                matches!(
                    support,
                    SystemAudioSupport::Available { .. } | SystemAudioSupport::NoDevice { .. }
                ),
                "{support:?}"
            );
        } else {
            match support {
                SystemAudioSupport::Unsupported { reason } => {
                    // The reason has to name what would be needed, or it is not
                    // a reason.
                    assert!(reason.contains("ProcessTap"), "{reason}");
                }
                other => panic!("expected Unsupported on this platform, got {other:?}"),
            }
        }
    }

    /// Windows reports the mechanism as verified; Linux does not.
    ///
    /// The distinction is the whole point of the `unverified` flag: the Linux
    /// path is written and nobody has run it, and a screen that showed both the
    /// same way would be making a claim about hardware nobody has.
    #[test]
    fn only_a_mechanism_someone_has_run_is_reported_as_verified() {
        if let SystemAudioSupport::Available { unverified, .. } = support() {
            assert_eq!(
                unverified,
                cfg!(target_os = "linux"),
                "linux is written and unverified; windows is the platform this is developed on"
            );
        }
    }

    /// `Available` is the only state a caller may act on.
    #[test]
    fn nothing_but_available_reads_as_available() {
        assert!(!SystemAudioSupport::Unsupported {
            reason: "no".to_owned()
        }
        .is_available());
        assert!(!SystemAudioSupport::NoDevice {
            mechanism: "m",
            reason: "none here".to_owned()
        }
        .is_available());
        assert!(SystemAudioSupport::Available {
            mechanism: "m",
            device: "d".to_owned(),
            unverified: false
        }
        .is_available());
    }

    /// An unverified mechanism does not describe itself as simply available.
    #[test]
    fn an_unverified_mechanism_says_so_in_its_summary() {
        let unverified = SystemAudioSupport::Available {
            mechanism: "m",
            device: "d".to_owned(),
            unverified: true,
        };
        assert!(unverified.summary().contains("never tested"));

        let verified = SystemAudioSupport::Available {
            mechanism: "m",
            device: "d".to_owned(),
            unverified: false,
        };
        assert_eq!(verified.summary(), "Available");
    }

    /// The states cross the IPC boundary as a discriminant, camelCased.
    #[test]
    fn the_states_serialize_with_a_discriminant() {
        let json = serde_json::to_value(SystemAudioSupport::Available {
            mechanism: "WASAPI loopback",
            device: "Speakers".to_owned(),
            unverified: false,
        })
        .unwrap();
        assert_eq!(json["state"], "available");
        assert_eq!(json["device"], "Speakers");

        let json = serde_json::to_value(SystemAudioSupport::NoDevice {
            mechanism: "m",
            reason: "r".to_owned(),
        })
        .unwrap();
        assert_eq!(json["state"], "no-device");
    }

    /// The cap is the same ceiling microphone capture uses, scaled by format.
    ///
    /// A stereo 48 kHz stream is six times the samples per second of 16 kHz
    /// mono, so a shared *sample* count would give it a sixth of the duration.
    #[test]
    fn the_capacity_is_thirty_minutes_of_whatever_format_the_device_uses() {
        let mono_16k = capacity(16_000, 1);
        let stereo_48k = capacity(48_000, 2);

        assert_eq!(mono_16k, 16_000 * 30 * 60);
        assert_eq!(stereo_48k, mono_16k * 6);
    }

    /// Starting on a platform with no mechanism fails with the reason.
    ///
    /// Not an empty capture: a caller holding a `SystemCapture` must be able to
    /// rely on it having recorded, or a meeting screen will say system audio was
    /// included when it was not.
    #[test]
    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    fn starting_where_it_cannot_work_fails_rather_than_recording_nothing() {
        match SystemCapture::start() {
            Err(CaptureError::NoConfig(reason)) => assert!(reason.contains("ProcessTap")),
            other => panic!("expected a refusal, got {other:?}"),
        }
    }
}
