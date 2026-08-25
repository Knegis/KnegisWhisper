//! Application orchestration.
//!
//! The crates below this one each do one thing: capture audio, resolve a route,
//! send a request, paste text, write a row. This crate is what puts them in
//! order, and the order is where the product behaviour lives.
//!
//! Every collaborator is a trait. That is deliberate: the properties worth
//! guaranteeing about dictation are almost all statements about *sequence* --
//! silence never reaches a provider, a failure still leaves a history row, the
//! session always returns to idle -- and sequence is exactly what stops being
//! testable once these become direct calls into the OS.

pub mod dictation;

pub use dictation::{
    Capture, Cleanup, Dictation, DictationError, History, HistoryEntry, Inserter, InsertionResult,
    Outcome, Recorder, SessionState, StateSink, Transcribed, Transcriber,
};
