//! `knegiswhisper-mcp` — a stdio MCP server over KnegisWhisper's local API.
//!
//! Upstream's MCP server was a hosted Streamable-HTTP service with nothing
//! local to port, so this is new work. What it ports instead is the *shape*
//! of `crates/local-api`'s surface: this is a second front end over that API,
//! not a second backend.
//!
//! Every tool call in [`tools`] is an HTTP request through [`kw_cli::Client`]
//! — the same HTTP client, the same discovery file, and the same
//! constant-time bearer-token comparison the `knegiswhisper` CLI already
//! uses. Nothing here opens SQLite directly. Doing that would put a second
//! writer on the database and a second implementation of "who may read this
//! note," and duplicating that decision is worse than not having it, because
//! now there are two places that can disagree about the answer.
//!
//! ## The one rule that matters
//!
//! **Nothing but a JSON-RPC message ever reaches stdout.** A stray
//! `println!`, or a panic message hitting the default panic hook, corrupts
//! the stream for whatever is reading it — and from the other end of a pipe,
//! that failure looks like nothing at all, which is a brutal thing to debug.
//! Every diagnostic in this crate goes to `tracing`, which `main` wires to
//! stderr and nowhere else. [`server::run`] is the loop that keeps this true;
//! `tests/protocol.rs` has a test that checks it directly.
//!
//! ## The protocol
//!
//! Three JSON-RPC 2.0 methods, newline-delimited on stdin and stdout — MCP's
//! stdio transport, not the `Content-Length` framing LSP uses:
//!
//! - `initialize` — the handshake.
//! - `tools/list` — [`tools::definitions`]: one tool per local API route that
//!   actually exists. A tool with no route behind it is not here.
//! - `tools/call` — [`tools::call`]. A call this crate refuses before it
//!   reaches the local API (an unknown tool, a missing or mistyped argument)
//!   is a JSON-RPC error, because that is a protocol mistake. A call that
//!   reaches the local API and gets a "no" — the app is not running, the note
//!   does not exist, the bearer token was rejected — comes back as an
//!   ordinary JSON-RPC *result* with `isError: true`: not a protocol failure,
//!   the answer.

pub mod protocol;
pub mod server;
pub mod tools;
