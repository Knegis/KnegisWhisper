//! The one writer of `apps/desktop/src/generated/ipc-types.ts`.
//!
//! Three of the five defects found in the first hand-check of the dashboard were
//! the same defect: a TypeScript type describing a Rust struct, written by hand
//! and drifting. `electronPresent` rendered "not present" without checking
//! anything, because the field did not exist in Rust at all. A tagged enum sent
//! `verified_by` while every struct sent `verifiedBy`, because `rename_all` on
//! an enum renames variants and not the fields inside them.
//!
//! So the declarations are no longer written by hand. Every type that crosses
//! the IPC boundary derives [`ts_rs::TS`], and this module renders all of them
//! into a single generated file that `src/lib/ipc.ts` re-exports. The `invoke()`
//! wrappers stay hand-written — the defect was never in the wrappers.
//!
//! # Why one writer rather than `#[ts(export)]`
//!
//! `ts-rs` can export each type on its own during `cargo test`, but the types
//! live in eight crates whose test binaries run concurrently, and they would all
//! be writing one file. A single renderer, called from a single test, has no
//! such race and gives a deterministic order besides.
//!
//! # Adding a type
//!
//! Derive `TS` on it and add it to the `declared!` list below. Then run
//!
//! ```text
//! KW_WRITE_IPC_TYPES=1 cargo test -p knegiswhisper --lib ipc_types
//! ```
//!
//! and commit the result. The same test without that variable is the check: it
//! fails when the committed file no longer matches the Rust types, which is what
//! makes the drift a build failure rather than a runtime fiction.

use std::fmt::Write as _;

use ts_rs::{Config, TS};

/// Where the rendered file lives, relative to this crate's manifest directory.
const OUT: &str = "../src/generated/ipc-types.ts";

/// `i64` and `u64` become `number`, not `bigint`.
///
/// Every 64-bit integer that crosses this boundary is a SQLite rowid, a byte
/// count, or a millisecond duration. None approaches 2^53, `serde_json` sends
/// them as JSON numbers, and Tauri's `invoke` parses them with `JSON.parse` —
/// so a `bigint` declaration would describe a value the frontend never receives.
fn config() -> Config {
    Config::new().with_large_int("number")
}

/// Render one type: its doc comment, then its declaration.
fn declare<T: TS + 'static + ?Sized>(out: &mut String, cfg: &Config, seen: &mut Vec<String>) {
    // Two Rust types in different modules can share a name -- `InstallProgress`
    // existed in both `local_model` and `local_llm` within an hour of the second
    // one being written. In one TypeScript file that is a redeclaration, and the
    // later one silently wins for every consumer. Caught here rather than by
    // `tsc`, because the message here can name the type.
    let name = T::ident(cfg);
    assert!(
        !seen.contains(&name),
        "two types are both declared as `{name}`. Rename one on the Rust side: a          TypeScript file cannot hold both, and the later one would win."
    );
    seen.push(name);

    if let Some(docs) = T::docs() {
        // `ts-rs` yields the JSDoc block already wrapped, with a trailing
        // newline. Anything else would need reflowing; nothing else appears.
        out.push_str(docs.trim_end());
        out.push('\n');
    }
    let _ = writeln!(out, "export {}\n", T::decl(cfg));
}

/// Every type in the file, in the order it appears.
///
/// Grouped to match `src/lib/ipc.ts`, so a reader moving between the two files
/// is looking at the same sequence. A type reachable only as another type's
/// field still belongs here — `ts-rs` refers to it by name rather than inlining
/// it, so an omission is a TypeScript compile error rather than a silent gap.
macro_rules! declared {
    ($out:ident, $cfg:ident, $seen:ident, $($section:literal => [$($ty:ty),* $(,)?]),* $(,)?) => {
        $(
            let _ = writeln!($out, "{}", section_rule($section));
            $( declare::<$ty>(&mut $out, &$cfg, &mut $seen); )*
        )*
    };
}

/// A section banner, padded to the same width `ipc.ts` uses.
fn section_rule(title: &str) -> String {
    const WIDTH: usize = 78;
    let tail = format!(" {title} */");
    let dashes = WIDTH.saturating_sub(tail.len() + 3);
    format!("/* {} {}", "-".repeat(dashes), tail.trim_start())
}

/// The generated file, exactly as it should appear on disk.
fn render() -> String {
    let cfg = config();
    let mut out = String::with_capacity(8 * 1024);
    let mut seen: Vec<String> = Vec::new();

    out.push_str(
        "/**\n\
         \x20* GENERATED FILE — DO NOT EDIT.\n\
         \x20*\n\
         \x20* Source:    the Rust types themselves, via `#[derive(ts_rs::TS)]`\n\
         \x20* Generator: apps/desktop/src-tauri/src/ipc_types.rs\n\
         \x20*\n\
         \x20* Regenerate with:\n\
         \x20*   KW_WRITE_IPC_TYPES=1 cargo test -p knegiswhisper --lib ipc_types\n\
         \x20*\n\
         \x20* `cargo test` fails when this file and the Rust types disagree, so a\n\
         \x20* frontend type can no longer describe a field Rust does not send.\n\
         \x20* Hand-written `invoke()` wrappers live in ../lib/ipc.ts.\n\
         \x20*/\n\n",
    );

    declared! { out, cfg, seen,
        "capabilities" => [
            kw_overlay::Evidence,
            kw_overlay::CaptureExclusion,
            crate::overlay::NonActivating,
            crate::overlay::OverlayStatus,
            crate::diagnostics::CredentialStoreStatus,
            crate::diagnostics::ModelCacheStatus,
            crate::diagnostics::ShortcutHookStatus,
            kw_audio_core::SystemAudioSupport,
            crate::diagnostics::Diagnostics,
        ],
        "hotkeys" => [
            kw_platform::hotkey::HotkeyAction,
            kw_platform::hotkey::Accelerator,
            kw_platform::hotkey::Binding,
            crate::commands::UnhandledAction,
            kw_platform::hotkey::Conflict,
            crate::commands::HotkeyReport,
            crate::shortcuts::Registration,
            crate::commands::HotkeyChange,
        ],
        "providers" => [
            crate::commands::Dispatch,
            crate::commands::ProviderSummary,
            kw_provider_registry::CredentialKind,
            crate::commands::CredentialSlotStatus,
            kw_provider_probe::ProbeKind,
            kw_provider_probe::SkipReason,
            kw_provider_probe::ProbeOutcome,
            kw_provider_probe::ProbeResult,
            kw_provider_probe::AudioKind,
            kw_provider_probe::ProbeReport,
        ],
        "reasoning" => [
            kw_reasoning_core::enterprise::EnterpriseConfig,
            crate::reasoning::CleanupSettings,
            crate::reasoning::ReasoningDispatch,
            crate::reasoning::ReasoningModelSummary,
            crate::reasoning::ReasoningProviderSummary,
            crate::reasoning::EnterpriseField,
            crate::reasoning::EnterpriseStatus,
            crate::reasoning::CleanupRoute,
            crate::reasoning::ReasoningStatus,
        ],
        "history" => [
            kw_storage::transcriptions::Transcription,
        ],
        "meetings" => [
            kw_meeting_core::AudioSource,
            kw_meeting_core::TranscriptEvent,
            kw_meeting_core::KeytermWire,
            kw_meeting_core::KeytermSupport,
            kw_meeting_core::ClientAvailability,
            kw_meeting_core::MeetingProviderSummary,
            crate::meeting::MeetingState,
            crate::meeting::Sources,
            crate::meeting::MeetingStatus,
            crate::meeting::MeetingOutcome,
        ],
        "notes" => [
            kw_notes_core::notes::Note,
            kw_notes_core::notes::Folder,
            kw_notes_core::notes::SearchHit,
            crate::commands::NewNoteRequest,
            crate::commands::NoteUpdateRequest,
            kw_notes_core::transcript::Format,
            crate::commands::ExportedTranscript,
        ],
        "import" => [
            kw_storage::import::TableReport,
            kw_storage::import::RelocatedCredential,
            kw_storage::import::ImportReport,
        ],
        "dictation" => [
            kw_app_core::dictation::SessionState,
            kw_audio_core::GateReason,
            kw_app_core::dictation::Outcome,
            kw_audio_core::InputDevice,
            crate::dictation::RouteReport,
        ],
        "local transcription" => [
            crate::local_model::Availability,
            crate::local_model::VerifiedBy,
            crate::local_model::ModelStatus,
            crate::local_model::LocalStatus,
            crate::local_model::InstallProgress,
        ],
        "local text generation" => [
            crate::local_llm::DraftStatus,
            crate::local_llm::LocalLlmModelStatus,
            crate::local_llm::LocalLlmStatus,
            crate::local_llm::LocalLlmProgress,
        ],
        "local api" => [
            crate::local_api::LocalApiStatus,
        ],
    }

    // `ts-rs` leaves a space before the newline it inserts ahead of a field's
    // doc comment. Harmless to TypeScript, but this file is committed and
    // compared byte for byte, so it should not carry trailing whitespace.
    let mut cleaned = out
        .lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n");

    // Exactly one trailing newline.
    while cleaned.ends_with('\n') {
        cleaned.pop();
    }
    cleaned.push('\n');
    cleaned
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn out_path() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(OUT)
    }

    /// The check, and — with `KW_WRITE_IPC_TYPES=1` — the generator.
    ///
    /// Deliberately a test rather than a CI step. A separate step is a thing
    /// somebody has to remember to add; `cargo test --workspace` already runs
    /// everywhere, so a Rust type that changes without its declaration being
    /// regenerated fails the same gate that everything else fails.
    #[test]
    fn generated_ipc_types_match_the_rust_types() {
        let rendered = render();
        let path = out_path();

        if std::env::var("KW_WRITE_IPC_TYPES").is_ok() {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, &rendered).unwrap();
            println!("wrote {}", path.display());
            return;
        }

        let committed = std::fs::read_to_string(&path).unwrap_or_else(|e| {
            panic!(
                "{} could not be read ({e}). Run\n  \
                 KW_WRITE_IPC_TYPES=1 cargo test -p knegiswhisper --lib ipc_types",
                path.display()
            )
        });

        // Line endings are normalised because git may check the file out with
        // CRLF; the content is what is being asserted, not the checkout style.
        assert_eq!(
            committed.replace("\r\n", "\n"),
            rendered,
            "\n{} is stale. Run\n  \
             KW_WRITE_IPC_TYPES=1 cargo test -p knegiswhisper --lib ipc_types\n\
             and commit the result.\n",
            path.display()
        );
    }

    /// The property the whole module exists for.
    ///
    /// `rename_all` on an enum renames its variants, not the fields inside
    /// them, so a tagged union once sent `verified_by` while every struct sent
    /// `verifiedBy`. The generator has to carry `rename_all_fields` through or
    /// it reintroduces exactly the bug it was written to prevent.
    #[test]
    fn a_tagged_enums_struct_fields_are_camel_case() {
        let decl = kw_overlay::CaptureExclusion::decl(&config());
        assert!(
            decl.contains("verifiedBy"),
            "rename_all_fields did not survive: {decl}"
        );
        assert!(
            !decl.contains("verified_by"),
            "the snake_case field is still being declared: {decl}"
        );
        assert!(
            decl.contains(r#""state": "active""#),
            "the discriminant is not tagged as the UI switches on it: {decl}"
        );
    }

    /// A rowid is a `number`, because that is what arrives.
    #[test]
    fn sixty_four_bit_integers_are_declared_as_numbers() {
        let decl = kw_storage::transcriptions::Transcription::decl(&config());
        assert!(!decl.contains("bigint"), "{decl}");
        assert!(decl.contains("id: number"), "{decl}");
    }
}
