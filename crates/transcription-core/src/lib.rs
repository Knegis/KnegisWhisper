//! Batch speech-to-text: route resolution and execution.
//!
//! Two halves, kept apart on purpose.
//!
//! [`route`] decides *where* audio goes. It is pure, needs no network, and
//! carries credential *references* rather than credentials -- so every routing
//! decision, including all the fail-closed ones, is tested without a key.
//!
//! [`executor`] sends it. That is the only half that needs a credential, and it
//! resolves one from the OS credential store at the last possible moment.
//!
//! The seam is what makes the important properties testable. See
//! `docs/PORTING_MATRIX.md` for the divergences from the upstream router.

pub mod clients;
pub mod executor;
pub mod local;
pub mod models;
pub mod route;
pub mod sherpa;
pub mod sherpa_models;
pub mod url;

pub use executor::{Executor, Transcript, TranscriptionError};
pub use local::{
    build_args, interpret, Inference, LocalTranscriber, ServerOptions, VadSettings, WhisperServer,
    HEALTH_PATH, INFERENCE_PATH,
};
pub use route::{
    resolve, Auth, AuthScheme, CortiConfig, ProviderClientKind, RouteError, RouteRequest,
    RouteSettings, Transport,
};
pub use sherpa::{LayoutError, SherpaOffline, TransducerFiles};
