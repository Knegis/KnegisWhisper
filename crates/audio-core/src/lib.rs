//! Microphone capture, speech gating, and audio preparation.
//!
//! The crate is arranged so that almost nothing needs a microphone to be tested.
//! [`gate`], [`vad`], and [`wav`] are pure and fully covered; [`capture`] is the
//! thin device layer, and it is small on purpose.
//!
//! Two decisions worth reading the modules for, because both are non-obvious and
//! both would be lost in a careless port:
//!
//! * The speech gate **fails open** when it cannot measure. Discarding a
//!   dictation the user actually spoke is worse than transcribing silence.
//! * Silero VAD is **off by default for dictation**. See [`vad`] for the reason,
//!   which is not a preference but a real failure mode.
//!
//! [`system_audio`] and [`mix`] are the meeting half, and they are split the same
//! way: capturing what the speakers play needs a device, and deciding what the
//! two streams sound like together does not — so all of the latter is tested.

pub mod capture;
pub mod gate;
pub mod mix;
pub mod system_audio;
pub mod vad;
pub mod wav;

pub use capture::{input_devices, Capture, CaptureError, InputDevice, Recording};
pub use gate::{GateDecision, GateReason, SpeechGate};
pub use mix::{mix, mix_to_wav, Mixed, Track};
pub use system_audio::{SystemAudioSupport, SystemCapture};
pub use vad::{silero_enabled_for, Context, SileroPreferences, VadConfig};
pub use wav::{duration_ms, prepare, WavWriter, TARGET_SAMPLE_RATE};
