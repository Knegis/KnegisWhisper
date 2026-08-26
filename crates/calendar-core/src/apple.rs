//! Apple Calendar via EventKit -- described, not implemented.
//!
//! EventKit is native macOS API, not an OAuth flow, and it is not implemented
//! anywhere in this codebase, on any platform, including macOS. There is no
//! macOS machine in this development environment to write or verify that
//! binding against, so this module stops at declaring the shape a future
//! implementation would fill in and reporting [`AppleCalendarSupport::Unsupported`]
//! with the reason -- following the precedent in
//! `kw_audio_core::system_audio`: a variant that says why, not silence and
//! not an empty calendar list that would read as "you have none".
//!
//! [`AppleCalendarSource`] is deliberately unimplemented by anything in this
//! crate. There is no `#[cfg(target_os = "macos")]` stub that would compile
//! and lie by returning an empty list; the honest state on every platform
//! this builds for today is `Unsupported`.

use serde::Serialize;
use ts_rs::TS;

/// What this build can do about Apple Calendar, checked rather than assumed.
///
/// One variant today because nothing is implemented anywhere -- see the
/// module doc. Shaped as a tagged enum, like
/// [`kw_audio_core::SystemAudioSupport`], so a future `Available` variant
/// slots in without changing how the frontend switches on it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(
    tag = "state",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum AppleCalendarSupport {
    Unsupported { reason: String },
}

/// Why, said once so [`support`] and any future caller share the exact words.
const REASON: &str = "Apple Calendar sync uses EventKit, native macOS API with no Rust binding \
    anywhere in this project. This is unimplemented on every platform, including macOS -- not \
    only unavailable elsewhere -- and unverifiable on this development machine even once \
    written, since there is no macOS machine here to run it on.";

/// The shape a future EventKit implementation would fill in: listing the
/// calendars EventKit exposes. Nothing implements this trait; it exists so
/// the interface a real implementation would satisfy is written down
/// somewhere other than a comment.
pub trait AppleCalendarSource: Send + Sync {
    fn list_calendars(&self) -> Result<Vec<AppleCalendarSummary>, AppleCalendarError>;
}

/// One calendar as EventKit would describe it, matching
/// `crates/storage/migrations/0001_initial.sql`'s `apple_calendars` table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AppleCalendarSummary {
    pub id: String,
    pub title: String,
    pub color: Option<String>,
    pub source_name: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum AppleCalendarError {
    #[error("Apple Calendar sync is not implemented: {0}")]
    Unimplemented(String),
}

/// What this build can do about Apple Calendar. Always `Unsupported`, on
/// every platform -- see the module doc for why that includes macOS.
#[must_use]
pub fn support() -> AppleCalendarSupport {
    AppleCalendarSupport::Unsupported {
        reason: REASON.to_owned(),
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
    fn support_is_unsupported_on_every_platform_this_builds_for() {
        // Not a tautology: this pins the honest fact that nothing implements
        // `AppleCalendarSource` anywhere yet, including on macOS, so a future
        // change that adds a real implementation on one platform has to
        // change this test rather than silently leaving it passing by
        // accident.
        match support() {
            AppleCalendarSupport::Unsupported { reason } => {
                assert!(reason.contains("EventKit"), "{reason}");
                assert!(reason.contains("macOS"), "{reason}");
            }
        }
    }

    #[test]
    fn the_state_serializes_with_a_discriminant() {
        let json = serde_json::to_value(support()).unwrap();
        assert_eq!(json["state"], "unsupported");
        assert!(json["reason"].as_str().unwrap().contains("EventKit"));
    }
}
