//! Sidecar binaries: what they are, where they live, and how they are stopped.
//!
//! This application does its local speech-to-text and local reasoning by
//! talking to separate processes — `whisper-server`, `llama-server`, sherpa-onnx
//! — over loopback HTTP. They are not libraries linked into the app: they are
//! large, they carry their own accelerator backends, and upstream ships them as
//! downloaded executables for good reasons that are worth keeping.
//!
//! Running a downloaded executable is the single most dangerous thing this
//! application does, so the rules are in the type system rather than in a
//! review checklist:
//!
//! - **Nothing runs unpinned.** [`manifest::Asset`] has a mandatory SHA-256, so
//!   there is no way to describe a binary we would execute without knowing
//!   exactly which bytes it is. A config entry with a missing or malformed
//!   digest fails to load.
//! - **Nothing binds beyond loopback.** [`port`] probes and allocates on
//!   `127.0.0.1` only, and takes no address parameter.
//! - **No process is signalled on the strength of a PID file.** [`reap`] reads
//!   the live process's command first and refuses to signal anything it cannot
//!   prove is ours, because process ids get reused and killing an editor is
//!   worse than leaking a server.
//! - **"Not published" is a first-class answer.** macOS is a real example today:
//!   upstream `whisper.cpp` releases carry an xcframework for embedding in an
//!   Xcode project, not a `whisper-server` executable. That is reported as
//!   [`manifest::Availability::NotPublished`] with a reason, not as a silent
//!   absence.
//!
//! ## What lives where
//!
//! | Module | Responsibility |
//! | --- | --- |
//! | [`manifest`] | What a sidecar is; digest and target types |
//! | [`catalog`] | Loading and validating `config/sidecars.json` |
//! | [`integrity`] | Proving a downloaded file is the pinned one |
//! | [`install`] | Download, verify, unpack -- strictly in that order |
//! | [`resolve`] | Finding an installed sidecar, or saying why not |
//! | [`port`] | Choosing a free loopback port from a declared range |
//! | [`pid`] | Remembering what we started, so a crash can be cleaned up |
//! | [`reap`] | Stopping what a previous run left behind, safely |
//!
//! Process lifecycle builds on these and lands in the commit that follows.

pub mod catalog;
pub mod install;
pub mod integrity;
pub mod manifest;
pub mod pid;
pub mod port;
pub mod process;
pub mod reap;
pub mod resolve;

pub use catalog::{Catalog, SCHEMA_VERSION};
pub use install::{extract, install, InstallError, Installed, Progress};
pub use integrity::IntegrityError;
pub use manifest::{
    ArchiveKind, Asset, Availability, DigestError, PortRange, Sha256Digest, SidecarSpec, Source,
    Target, TargetState,
};
pub use pid::{Inventory, PidError, PidRecord, PidStore};
pub use port::{allocate, BindProbe, PortError, PortProbe, LOOPBACK};
pub use process::{start, Health, HttpHealth, Launch, ProcessError, Running, StartTimings};
pub use reap::{reap_one, Liveness, Outcome, ProcessTable, SystemProcessTable, Verdict};
pub use resolve::{locate, Installation, RealFs, SearchPath};

/// Anything that can go wrong installing or running a sidecar.
#[derive(Debug, thiserror::Error)]
pub enum SidecarError {
    #[error(transparent)]
    Integrity(#[from] IntegrityError),

    #[error(transparent)]
    Pid(#[from] PidError),

    #[error(transparent)]
    Port(#[from] PortError),

    #[error("{sidecar} is not published for {target}: {reason}")]
    NotPublished {
        sidecar: String,
        target: &'static str,
        reason: String,
    },

    #[error(
        "{sidecar} has no entry for {target} in config/sidecars.json. This is a gap in the \
         manifest, not a statement that the platform cannot run it."
    )]
    NoManifestEntry {
        sidecar: String,
        target: &'static str,
    },

    #[error("this platform is not one any sidecar is published for")]
    UnknownTarget,

    #[error("config/sidecars.json is not usable: {problems}")]
    ManifestInvalid { problems: String },
}
