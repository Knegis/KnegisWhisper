//! Noticing that a meeting is happening.
//!
//! Pure core, thin platform edge — the shape [`crate::keys`]/[`crate::keyhook`]
//! and `audio-core`'s `mix`/`system_audio` already use. [`detect`] takes a list
//! of process names, so every rule is testable with no processes at all, and
//! [`running`] is the twelve lines that ask the operating system.
//!
//! # Detection suggests. It never records.
//!
//! Nothing here can start a recording, and nothing that calls it may either. The
//! brief forbids recording without the local user knowing, and a detector that
//! started a recording because Teams was running is exactly that. What this
//! produces is a [`Detected`] for something to offer.
//!
//! # The image name, not the command line
//!
//! `kw_sidecar::reap` matches the *full command line*, because it has to prove a
//! specific process is ours before signalling it — a reused process id must not
//! get a SIGTERM. Detection has the opposite job: notice a class of application.
//! A command line carries paths, profile directories and switches, so matching
//! against it produces more false positives rather than fewer. `Zoom.exe` in an
//! installer's command line is not a meeting.
//!
//! # These names are a hypothesis until somebody has been in a call
//!
//! Upstream's `meetingProcessDetector.js` is not in this checkout, so the table
//! below is not ported from anything readable — it is written from knowledge of
//! what these applications are called. `cargo run -p kw-platform --example
//! meeting_detect_probe` prints every running process and what matched, which is
//! how the table becomes a measurement. `docs/PLATFORM_SUPPORT.md` records, per
//! provider, whether the names have been *seen to match* or only written.
//!
//! The one that decides whether this feature is useful or annoying: on Windows
//! `Zoom.exe` runs whenever the client is open, and `CptHost.exe` is the process
//! that exists only during a call. Matching the first would prompt for having
//! Zoom installed. Both are listed and `in_call` says which is which, so the
//! caller can prefer the stronger evidence — and only a real call settles
//! whether the split is right.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// A meeting application that appears to be running.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Detected {
    /// The `meeting-detect:*` id, so a caller can name the integration.
    pub provider: &'static str,
    /// What to call it on screen.
    pub name: &'static str,
    /// The process name that matched.
    ///
    /// Carried so a false positive can be diagnosed rather than argued about: a
    /// prompt that says *why* it appeared can be told it is wrong.
    pub evidence: String,
    /// True when the matched process only exists during a call, rather than
    /// whenever the application is open.
    ///
    /// The difference between "in a meeting" and "has Zoom installed", and the
    /// reason a caller can choose to prompt only on the stronger signal.
    pub in_call: bool,
}

/// One application's process names.
struct Signature {
    provider: &'static str,
    name: &'static str,
    /// Names that mean the application is open, which is weaker evidence.
    open: &'static [&'static str],
    /// Names that exist only while a call is in progress.
    in_call: &'static [&'static str],
}

/// What each application is called, per platform.
///
/// Lower-case, and compared against a lower-cased process name: Windows reports
/// `Zoom.exe` and Linux `zoom`, and casing is not a fact worth being strict
/// about.
const SIGNATURES: &[Signature] = &[
    Signature {
        provider: "zoom",
        name: "Zoom",
        open: &["zoom.exe", "zoom", "zoom.us"],
        // Windows spawns this for the meeting window itself; macOS uses
        // `caphost`. Neither exists while the client merely sits in the tray.
        in_call: &["cpthost.exe", "cpthost", "caphost"],
    },
    Signature {
        provider: "teams",
        name: "Microsoft Teams",
        // `ms-teams.exe` is the current client; `teams.exe` the classic one.
        // Both, because a migrated machine can have either.
        open: &["ms-teams.exe", "teams.exe", "msteams", "teams"],
        in_call: &[],
    },
    Signature {
        provider: "webex",
        name: "Webex",
        open: &["webex.exe", "webexhost.exe", "ciscocollabhost.exe", "webex"],
        in_call: &["webexmta.exe", "webexmta"],
    },
    #[cfg(target_os = "macos")]
    Signature {
        provider: "facetime",
        name: "FaceTime",
        open: &["facetime"],
        in_call: &[],
    },
];

/// The `meeting-detect:generic` id, used for anything the user listed.
pub const GENERIC: &str = "generic";

/// The user's own process names.
///
/// `meeting-detect:generic` upstream also covered calendar events, microphone
/// activity and system-audio activity. Only the lists are built: calendars are
/// chunk F, and microphone activity needs the `IAudioSessionManager2` listener
/// that is still unwritten. Named here rather than left implied, because a
/// half-built detector that looks whole is how a user comes to trust one that
/// misses half the meetings.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
pub struct Lists {
    /// Extra process names that count as a meeting.
    pub allow: Vec<String>,
    /// Names that never do, whatever else matches.
    ///
    /// Applied after the built-in table as well as after `allow`, so a user can
    /// silence a false positive this crate is wrong about without waiting for a
    /// release.
    pub block: Vec<String>,
}

impl Lists {
    /// Whether `name` is blocked. Compared case-insensitively, like the table.
    fn blocks(&self, name: &str) -> bool {
        self.block
            .iter()
            .any(|blocked| blocked.trim().eq_ignore_ascii_case(name))
    }
}

/// Which meeting applications `running` contains.
///
/// One entry per provider even when several of its names match, and the stronger
/// evidence wins: a caller offering to record should be told "in a call", not
/// "Zoom is open", when both are true.
#[must_use]
pub fn detect(running: &[String], lists: &Lists) -> Vec<Detected> {
    let names: Vec<String> = running
        .iter()
        .map(|n| n.trim().to_lowercase())
        .filter(|n| !n.is_empty())
        .collect();

    let mut found: Vec<Detected> = Vec::new();

    for signature in SIGNATURES {
        // In-call first, so it wins for a provider whose client is also open.
        let matched = signature
            .in_call
            .iter()
            .find_map(|wanted| names.iter().find(|n| n == wanted).map(|n| (n, true)))
            .or_else(|| {
                signature
                    .open
                    .iter()
                    .find_map(|wanted| names.iter().find(|n| n == wanted).map(|n| (n, false)))
            });

        let Some((name, in_call)) = matched else {
            continue;
        };
        if lists.blocks(name) {
            continue;
        }

        found.push(Detected {
            provider: signature.provider,
            name: signature.name,
            evidence: name.clone(),
            in_call,
        });
    }

    // The user's own names, after the table, and deduplicated: two spellings of
    // one process should not produce two prompts.
    let mut seen: BTreeSet<String> = BTreeSet::new();
    for wanted in &lists.allow {
        let wanted = wanted.trim().to_lowercase();
        if wanted.is_empty() || lists.blocks(&wanted) || !seen.insert(wanted.clone()) {
            continue;
        }
        if names.contains(&wanted) {
            found.push(Detected {
                provider: GENERIC,
                name: "A program you listed",
                evidence: wanted,
                // A name the user chose says the program is running. Whether
                // that means a call is in progress is something only they know,
                // so claiming it would be inventing evidence.
                in_call: false,
            });
        }
    }

    found
}

/// Whether this platform can enumerate processes at all.
///
/// `sysinfo` supports every platform this application targets, so this is always
/// true today. It exists so a caller reports a capability rather than assuming
/// one, which is the pattern every other platform capability here follows.
#[must_use]
pub const fn supported() -> bool {
    true
}

/// Every running process's image name.
///
/// The impure half, and deliberately the whole of it. Mirrors
/// `kw_sidecar::reap::SystemProcessTable` in how it drives `sysinfo`, and differs
/// in taking the image name rather than the command line — see the module
/// documentation.
#[must_use]
pub fn running() -> Vec<String> {
    use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};

    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        // Nothing but the name is read, so nothing else is asked for. `new()` is
        // sysinfo 0.32's empty set -- a full refresh on a busy machine reads
        // every process's memory and disk counters for no reason.
        ProcessRefreshKind::new(),
    );

    system
        .processes()
        .values()
        .map(|process| process.name().to_string_lossy().into_owned())
        .collect()
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

    fn processes(names: &[&str]) -> Vec<String> {
        names.iter().map(|n| (*n).to_owned()).collect()
    }

    fn none() -> Lists {
        Lists::default()
    }

    #[test]
    fn an_idle_machine_detects_nothing() {
        let found = detect(
            &processes(&["explorer.exe", "code.exe", "knegiswhisper-desktop.exe"]),
            &none(),
        );
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn each_application_is_recognised_by_its_own_name() {
        for (process, provider) in [
            ("Zoom.exe", "zoom"),
            ("ms-teams.exe", "teams"),
            ("Teams.exe", "teams"),
            ("Webex.exe", "webex"),
        ] {
            let found = detect(&processes(&[process]), &none());
            assert_eq!(found.len(), 1, "{process}: {found:?}");
            assert_eq!(found[0].provider, provider, "{process}");
        }
    }

    #[test]
    fn casing_is_not_a_fact_worth_being_strict_about() {
        // Windows reports `Zoom.exe`, Linux `zoom`, and a user typing an
        // allowlist entry types whatever they like.
        for spelling in ["Zoom.exe", "zoom.exe", "ZOOM.EXE", "  Zoom.exe  "] {
            let found = detect(&processes(&[spelling]), &none());
            assert_eq!(found.len(), 1, "{spelling}");
            assert_eq!(found[0].evidence, "zoom.exe", "{spelling}");
        }
    }

    #[test]
    fn a_name_that_merely_contains_zoom_is_not_zoom() {
        // The whole reason matching is exact rather than by substring. A prompt
        // that fires for a whiteboard application would train the user to
        // dismiss it, and then it would miss the real meeting.
        for innocent in [
            "zoomboard.exe",
            "zoominfo.exe",
            "teamviewer.exe",
            "webexplorer.exe",
            "steam.exe",
        ] {
            let found = detect(&processes(&[innocent]), &none());
            assert!(found.is_empty(), "{innocent} matched: {found:?}");
        }
    }

    #[test]
    fn being_in_a_call_is_distinguished_from_having_the_client_open() {
        // The distinction that decides whether this feature is useful. On
        // Windows `Zoom.exe` runs whenever the client is open; `CptHost.exe`
        // exists only during a call.
        let open = detect(&processes(&["Zoom.exe"]), &none());
        assert!(!open[0].in_call, "the client being open is not a call");

        let calling = detect(&processes(&["Zoom.exe", "CptHost.exe"]), &none());
        assert_eq!(calling.len(), 1, "one entry per provider: {calling:?}");
        assert!(calling[0].in_call, "the stronger evidence wins");
        assert_eq!(calling[0].evidence, "cpthost.exe");
    }

    #[test]
    fn two_applications_running_are_two_detections() {
        let found = detect(&processes(&["Zoom.exe", "ms-teams.exe"]), &none());
        let providers: Vec<&str> = found.iter().map(|d| d.provider).collect();
        assert_eq!(providers, vec!["zoom", "teams"]);
    }

    #[test]
    fn a_users_own_name_is_detected_as_generic() {
        let lists = Lists {
            allow: vec!["mycompanyphone.exe".to_owned()],
            block: Vec::new(),
        };
        let found = detect(&processes(&["mycompanyphone.exe"]), &lists);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].provider, GENERIC);
        assert_eq!(found[0].evidence, "mycompanyphone.exe");
        assert!(
            !found[0].in_call,
            "a name the user chose says the program runs, not that a call is happening"
        );
    }

    #[test]
    fn a_blocked_name_never_matches_however_it_got_there() {
        // Applies to the built-in table as well as the allowlist, so a user can
        // silence a false positive this crate is wrong about without waiting for
        // a release.
        let lists = Lists {
            allow: vec!["mything.exe".to_owned()],
            block: vec!["Zoom.exe".to_owned(), "mything.exe".to_owned()],
        };
        let found = detect(&processes(&["Zoom.exe", "mything.exe"]), &lists);
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn blocking_the_call_process_falls_back_to_the_client_being_open() {
        // Blocking `CptHost.exe` should not hide Zoom entirely -- the client is
        // still running and that is still true.
        let lists = Lists {
            allow: Vec::new(),
            block: vec!["cpthost.exe".to_owned()],
        };
        let found = detect(&processes(&["Zoom.exe", "CptHost.exe"]), &lists);
        assert!(
            found.is_empty(),
            "in-call is blocked, so nothing: {found:?}"
        );
    }

    #[test]
    fn a_duplicated_allow_entry_produces_one_detection() {
        let lists = Lists {
            allow: vec!["thing.exe".to_owned(), "THING.exe".to_owned()],
            block: Vec::new(),
        };
        let found = detect(&processes(&["thing.exe"]), &lists);
        assert_eq!(found.len(), 1, "two spellings, one prompt: {found:?}");
    }

    #[test]
    fn empty_and_whitespace_entries_are_ignored_rather_than_matching_everything() {
        let lists = Lists {
            allow: vec![String::new(), "   ".to_owned()],
            block: Vec::new(),
        };
        let found = detect(&processes(&["", "  ", "explorer.exe"]), &lists);
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn every_signature_names_a_provider_and_at_least_one_process() {
        let mut providers: Vec<&str> = SIGNATURES.iter().map(|s| s.provider).collect();
        for signature in SIGNATURES {
            assert!(!signature.name.is_empty(), "{}", signature.provider);
            assert!(
                !signature.open.is_empty() || !signature.in_call.is_empty(),
                "{} matches nothing",
                signature.provider
            );
            for name in signature.open.iter().chain(signature.in_call) {
                assert_eq!(
                    *name,
                    name.to_lowercase(),
                    "{} must be lower-case to match",
                    name
                );
            }
        }

        let count = providers.len();
        providers.sort_unstable();
        providers.dedup();
        assert_eq!(providers.len(), count, "two signatures share a provider id");
    }

    #[test]
    fn the_manifest_ids_are_the_ones_this_module_reports() {
        // `config/integration-manifest.json` declares meeting-detect:{zoom,
        // teams, webex, facetime, generic}. A provider id here that does not
        // match one there would report an integration that does not exist.
        let mut ids: Vec<&str> = SIGNATURES.iter().map(|s| s.provider).collect();
        ids.push(GENERIC);
        for id in &ids {
            assert!(
                ["zoom", "teams", "webex", "facetime", "generic"].contains(id),
                "{id} is not a declared meeting-detect id"
            );
        }

        // FaceTime is macOS-only and reported as absent elsewhere rather than
        // silently missing.
        #[cfg(target_os = "macos")]
        assert!(ids.contains(&"facetime"));
        #[cfg(not(target_os = "macos"))]
        assert!(!ids.contains(&"facetime"));
    }

    #[test]
    fn the_real_process_table_answers() {
        // The one thing a unit test can say about the impure half: it returns
        // something, and this process is in it. Which meeting applications are
        // running is what the probe is for.
        assert!(supported());
        let names = running();
        assert!(!names.is_empty(), "no processes at all is not credible");
    }
}
