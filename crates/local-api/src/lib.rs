//! The local HTTP API: how the CLI, an editor plugin, or a script reaches this
//! application's notes, transcripts, and dictionary.
//!
//! Every one of the brief's requirements for this surface is a property of the
//! code rather than a convention:
//!
//! - **Loopback only.** [`serve`] inspects the listener it is given and refuses
//!   to serve on anything that is not a loopback address. There is no
//!   configuration option to widen it, and the check is in the crate that does
//!   the serving rather than in whoever calls it.
//! - **Authenticated.** Every route except nothing at all requires a bearer
//!   token generated fresh each run from the operating system's randomness, and
//!   compared in constant time by [`kw_security::LocalApiToken`].
//! - **Discoverable only to this user.** The port and token live in a file whose
//!   permissions are read back and reported, never assumed. See [`discovery`].
//! - **Bounded.** Request bodies are capped, page sizes are capped, and the
//!   caps are the same constants the validation tests use.
//!
//! ## What it is not
//!
//! Not a remote API, not a service, and not something to put behind a reverse
//! proxy. It exists so that work already done in this application can be
//! scripted locally. Anything that reaches it is already running as this user,
//! which is why the token matters and why the discovery file's permissions are
//! reported honestly rather than described optimistically.

pub mod discovery;
pub mod runtime;
pub mod server;
pub mod wire;

pub use discovery::{Discovery, DiscoveryError, Permissions};
pub use server::{serve, ApiState, ServeError, Served};
pub use wire::{
    ApiError, DictionaryUpdate, ErrorCode, Health, Item, ListParams, NotePatch, Page, SearchParams,
    MAX_BODY_BYTES, MAX_LIMIT,
};

/// The port range the local API is documented to use.
///
/// A client reads the port from the discovery file, so the range is not needed
/// to find it. It exists so the surface is predictable: a firewall rule can name
/// it, and a listener outside it is recognisably not ours.
pub const PORT_RANGE: (u16, u16) = (8200, 8219);

/// The version of the API surface, reported by `GET /v1/health`.
pub const API_VERSION: u32 = 1;
