//! Loading `config/sidecars.json`.
//!
//! The file is compiled into the binary with `include_str!` rather than read
//! from disk at runtime. That is the whole point: a digest pinned in a file the
//! user can edit is not a pin. Re-pinning a sidecar is a source change, a diff,
//! and a release — which is what makes "this binary was reviewed" mean
//! something.
//!
//! Validation happens on load and reports every problem at once, because the
//! only person who ever sees these errors is whoever just edited the file.

use std::collections::BTreeSet;

use serde::Deserialize;

use crate::manifest::{SidecarSpec, Target, TargetState};
use crate::SidecarError;

/// The shipped catalogue, as source.
const SHIPPED: &str = include_str!("../../../config/sidecars.json");

/// Bump when the shape changes in a way an older build could misread.
pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Document {
    #[serde(rename = "$comment", default)]
    _comment: Option<String>,
    schema_version: u32,
    sidecars: Vec<SidecarSpec>,
}

/// Every sidecar this build knows how to install.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Catalog {
    sidecars: Vec<SidecarSpec>,
}

impl Catalog {
    /// The catalogue compiled into this build.
    ///
    /// Fails only if `config/sidecars.json` is malformed, which a test in this
    /// module rules out at build time.
    pub fn shipped() -> Result<Self, SidecarError> {
        Self::parse(SHIPPED)
    }

    /// Parse and validate a catalogue.
    pub fn parse(json: &str) -> Result<Self, SidecarError> {
        let document: Document =
            serde_json::from_str(json).map_err(|source| SidecarError::ManifestInvalid {
                problems: source.to_string(),
            })?;

        if document.schema_version != SCHEMA_VERSION {
            return Err(SidecarError::ManifestInvalid {
                problems: format!(
                    "schema version {} is not the {SCHEMA_VERSION} this build understands",
                    document.schema_version
                ),
            });
        }

        let mut problems: Vec<String> = Vec::new();
        let mut seen: BTreeSet<&str> = BTreeSet::new();
        for spec in &document.sidecars {
            problems.extend(spec.problems());
            if !seen.insert(spec.name.as_str()) {
                problems.push(format!(
                    "{:?} appears twice; one entry would silently shadow the other",
                    spec.name
                ));
            }
        }

        if !problems.is_empty() {
            return Err(SidecarError::ManifestInvalid {
                problems: problems.join("; "),
            });
        }

        Ok(Self {
            sidecars: document.sidecars,
        })
    }

    /// One sidecar by name.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&SidecarSpec> {
        self.sidecars.iter().find(|spec| spec.name == name)
    }

    /// Every sidecar, in the order the file lists them.
    #[must_use]
    pub fn all(&self) -> &[SidecarSpec] {
        &self.sidecars
    }

    /// What a sidecar can do on the platform this build is running on.
    ///
    /// The three failure modes are kept apart deliberately: an unknown platform,
    /// a platform the manifest says nothing about, and a platform where the
    /// asset genuinely does not exist. Only the last is a fact about the world;
    /// the other two are gaps, and reporting them as "unsupported" would hide
    /// work that needs doing.
    pub fn resolve<'a>(
        &'a self,
        name: &str,
    ) -> Result<(&'a SidecarSpec, &'a crate::Asset), SidecarError> {
        let spec = self
            .get(name)
            .ok_or_else(|| SidecarError::NoManifestEntry {
                sidecar: name.to_owned(),
                target: "any",
            })?;

        let target = Target::current().ok_or(SidecarError::UnknownTarget)?;

        match spec.availability(target) {
            TargetState::Installable(asset) => Ok((spec, asset)),
            TargetState::Unsupported { reason, .. } => Err(SidecarError::NotPublished {
                sidecar: name.to_owned(),
                target: target.as_str(),
                reason: reason.to_owned(),
            }),
            TargetState::Unknown => Err(SidecarError::NoManifestEntry {
                sidecar: name.to_owned(),
                target: target.as_str(),
            }),
        }
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
    use crate::manifest::Availability;

    #[test]
    fn the_shipped_catalogue_loads_and_every_entry_is_sound() {
        // This is the test that makes config/sidecars.json real rather than
        // decorative: a malformed digest, a plaintext URL, a reversed port
        // range, or a missing command fragment fails the build here.
        let catalog = Catalog::shipped().unwrap();
        assert!(!catalog.all().is_empty());
    }

    #[test]
    fn the_shipped_catalogue_includes_whisper() {
        let catalog = Catalog::shipped().unwrap();
        let whisper = catalog
            .get("whisper")
            .expect("whisper is the point of this");
        assert_eq!(whisper.source.repo, "ggml-org/whisper.cpp");
        assert!(
            !whisper.source.repo.starts_with("OpenWhispr/"),
            "the whole exercise is not depending on the vendor's org"
        );
        assert!(whisper.port_range.contains(8178));
    }

    #[test]
    fn every_published_asset_in_the_shipped_catalogue_is_pinned_over_https() {
        // Belt and braces: Asset cannot exist without a digest, so this checks
        // the half the type cannot -- that the URL is not plaintext and the
        // size was filled in.
        for spec in Catalog::shipped().unwrap().all() {
            for (target, availability) in &spec.targets {
                if let Availability::Published(asset) = availability {
                    assert!(
                        asset.url.starts_with("https://"),
                        "{}/{}",
                        spec.name,
                        target.as_str()
                    );
                    assert_eq!(asset.sha256.as_str().len(), 64);
                    assert!(asset.size_bytes > 0);
                }
            }
        }
    }

    #[test]
    fn macos_is_recorded_as_not_published_with_a_reason() {
        // Upstream whisper.cpp releases carry an xcframework, which is a library
        // for embedding in an Xcode project, not a whisper-server executable.
        // Saying nothing would read as "we forgot"; saying "unsupported"
        // without a reason would read as "impossible".
        let catalog = Catalog::shipped().unwrap();
        let whisper = catalog.get("whisper").unwrap();

        for target in [Target::MacosAarch64, Target::MacosX8664] {
            match whisper.availability(target) {
                TargetState::Unsupported { reason, tracking } => {
                    assert!(reason.contains("xcframework"), "{reason}");
                    assert!(!tracking.is_empty());
                }
                other => panic!("{}: {other:?}", target.as_str()),
            }
        }
    }

    #[test]
    fn windows_and_linux_are_installable_in_the_shipped_catalogue() {
        let catalog = Catalog::shipped().unwrap();
        let whisper = catalog.get("whisper").unwrap();
        for target in [
            Target::WindowsX8664,
            Target::LinuxX8664,
            Target::LinuxAarch64,
        ] {
            assert!(
                whisper.availability(target).is_installable(),
                "{} should be installable",
                target.as_str()
            );
        }
    }

    #[test]
    fn a_wrong_schema_version_is_refused_rather_than_guessed_at() {
        let json = r#"{"schemaVersion": 999, "sidecars": []}"#;
        let err = Catalog::parse(json).unwrap_err().to_string();
        assert!(err.contains("999"), "{err}");
    }

    #[test]
    fn a_duplicate_name_is_refused() {
        // Two entries named the same would share an install directory and a PID
        // file, and one would silently win.
        let one = r#"{
            "name": "whisper",
            "description": "d",
            "source": {"repo": "r", "release": "t", "pinnedBecause": "w"},
            "portRange": {"first": 8178, "last": 8199},
            "commandFragments": ["whisper-server"],
            "targets": {}
        }"#;
        let json = format!(r#"{{"schemaVersion": {SCHEMA_VERSION}, "sidecars": [{one}, {one}]}}"#);
        let err = Catalog::parse(&json).unwrap_err().to_string();
        assert!(err.contains("twice"), "{err}");
        // And the empty-targets problem is reported in the same pass.
        assert!(err.contains("no targets"), "{err}");
    }

    #[test]
    fn every_problem_is_reported_in_one_pass() {
        let json = format!(
            r#"{{"schemaVersion": {SCHEMA_VERSION}, "sidecars": [{{
                "name": "a b",
                "description": "d",
                "source": {{"repo": "r", "release": "t", "pinnedBecause": "w"}},
                "portRange": {{"first": 0, "last": 0}},
                "commandFragments": [],
                "targets": {{}}
            }}]}}"#
        );
        let err = Catalog::parse(&json).unwrap_err().to_string();
        for expected in [
            "directory name",
            "reversed",
            "never be reaped",
            "no targets",
        ] {
            assert!(err.contains(expected), "{expected:?} missing from {err}");
        }
    }

    #[test]
    fn an_unknown_sidecar_name_is_an_error_that_names_it() {
        let catalog = Catalog::shipped().unwrap();
        let err = catalog.resolve("nonexistent").unwrap_err().to_string();
        assert!(err.contains("nonexistent"));
    }

    #[test]
    fn resolving_on_this_platform_gives_a_usable_answer_or_says_why_not() {
        // Runs on all three platforms in CI, so it asserts the shape of the
        // answer rather than a particular one. On macOS this is the honest
        // refusal path, and it must not be mistaken for a bug.
        let catalog = Catalog::shipped().unwrap();
        match catalog.resolve("whisper") {
            Ok((spec, asset)) => {
                assert_eq!(spec.name, "whisper");
                assert!(asset.executable_name().starts_with("whisper-server"));
            }
            Err(SidecarError::NotPublished { reason, .. }) => {
                assert!(!reason.is_empty(), "a refusal has to say why");
            }
            Err(other) => panic!("unexpected: {other}"),
        }
    }
}
