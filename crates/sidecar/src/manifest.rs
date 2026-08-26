//! What a sidecar is, declaratively.
//!
//! The point of this module is that **an unpinned executable cannot be
//! represented**. [`Asset::sha256`] is not an `Option`, so there is no way to
//! describe a binary we would run without knowing what it is, and a
//! `config/sidecars.json` entry with a missing or malformed digest fails to
//! deserialize rather than defaulting to "skip verification".
//!
//! Upstream verifies its GPU packs by digest but its models by size alone. Size
//! is a typo check, not an integrity check: it tells you the transfer finished,
//! not that the bytes are the ones the publisher built. That distinction only
//! matters for something we are about to execute, so it is drawn in the type
//! system here rather than left to whoever adds the next entry.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Deserializer, Serialize};

/// A SHA-256 digest, known to be 64 lowercase hex characters.
///
/// Parsed rather than newtyped over `String` so an uppercase digest, a
/// truncated paste, or a git object id cannot sit in the config looking
/// plausible.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct Sha256Digest(String);

impl Sha256Digest {
    /// Parse a digest, rejecting anything that is not exactly 64 lowercase hex
    /// characters.
    pub fn parse(value: &str) -> Result<Self, DigestError> {
        if value.len() != 64 {
            return Err(DigestError::Length {
                found: value.len(),
                value: value.to_owned(),
            });
        }
        if let Some(bad) = value.chars().find(|c| !c.is_ascii_hexdigit()) {
            return Err(DigestError::NotHex {
                character: bad,
                value: value.to_owned(),
            });
        }
        if value.chars().any(|c| c.is_ascii_uppercase()) {
            // Rejected rather than lowercased: two spellings of one digest
            // means two ways for a comparison elsewhere to be wrong.
            return Err(DigestError::NotLowercase {
                value: value.to_owned(),
            });
        }
        Ok(Self(value.to_owned()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Sha256Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for Sha256Digest {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        Self::parse(&raw).map_err(serde::de::Error::custom)
    }
}

/// Why a digest was refused.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum DigestError {
    #[error("a sha-256 digest is 64 characters; {value:?} is {found}")]
    Length { found: usize, value: String },

    #[error("{value:?} contains {character:?}, which is not a hex digit")]
    NotHex { character: char, value: String },

    #[error("{value:?} must be lowercase, so there is only one spelling to compare")]
    NotLowercase { value: String },
}

/// The platforms a sidecar can be published for.
///
/// Deliberately a closed set rather than an arbitrary triple string: a typo in
/// a target name would otherwise read as "no asset for this platform", which is
/// indistinguishable from an asset that genuinely does not exist.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Target {
    #[serde(rename = "windows-x86_64")]
    WindowsX8664,
    #[serde(rename = "linux-x86_64")]
    LinuxX8664,
    #[serde(rename = "linux-aarch64")]
    LinuxAarch64,
    #[serde(rename = "macos-x86_64")]
    MacosX8664,
    #[serde(rename = "macos-aarch64")]
    MacosAarch64,
}

impl Target {
    /// The target this build is running on, or `None` on a platform no sidecar
    /// is published for.
    #[must_use]
    pub fn current() -> Option<Self> {
        match (std::env::consts::OS, std::env::consts::ARCH) {
            ("windows", "x86_64") => Some(Self::WindowsX8664),
            ("linux", "x86_64") => Some(Self::LinuxX8664),
            ("linux", "aarch64") => Some(Self::LinuxAarch64),
            ("macos", "x86_64") => Some(Self::MacosX8664),
            ("macos", "aarch64") => Some(Self::MacosAarch64),
            _ => None,
        }
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::WindowsX8664 => "windows-x86_64",
            Self::LinuxX8664 => "linux-x86_64",
            Self::LinuxAarch64 => "linux-aarch64",
            Self::MacosX8664 => "macos-x86_64",
            Self::MacosAarch64 => "macos-aarch64",
        }
    }

    /// Whether an executable on this target carries an `.exe` suffix.
    #[must_use]
    pub const fn is_windows(self) -> bool {
        matches!(self, Self::WindowsX8664)
    }
}

/// How a downloaded asset is packed.
///
/// Only the forms some pinned or soon-to-be-pinned asset actually uses. A
/// variant for `tar.bz2` (which sherpa-onnx uses) is deliberately absent
/// rather than present and unimplemented: an enum arm that fails at runtime is
/// a promise the type made and the code did not keep. It gets added by the
/// commit that can extract one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ArchiveKind {
    Zip,
    TarGz,
    /// A `.tar.xz` archive: decompressed with `lzma-rs` (pure Rust,
    /// decode-only) and then read as a tar with the same `tar` crate the
    /// other variants use. ffmpeg ships this shape on Linux.
    TarXz,
    /// No archive at all: the downloaded file *is* the executable, verbatim.
    /// yt-dlp ships this shape on every platform.
    ///
    /// This is the strongest row in the pinning table, not a weaker one.
    /// [`Asset::sha256`] is mandatory for every variant, but for the other
    /// three it pins a container that is then partially extracted, so it
    /// proves the container matched without, on its own, saying that
    /// extraction produced exactly what upstream built. Here there is no
    /// container: the pinned digest is the digest of the exact bytes that
    /// will be copied into place and executed.
    Bare,
}

/// Whether a sidecar can be installed on a given target at all.
///
/// `NotPublished` exists because the honest answer for macOS today is "upstream
/// ships only an xcframework, which is a library for embedding in an Xcode
/// project and not a server executable". Encoding that as a variant rather than
/// an absent map entry means the UI can say why, and `docs/PLATFORM_SUPPORT.md`
/// can cite the same fact rather than restating it by hand.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "availability",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum Availability {
    Published(Asset),
    NotPublished {
        /// Plain-language reason, shown to the user.
        reason: String,
        /// Where the work to fix it is tracked.
        tracking: String,
    },
}

/// One downloadable, digest-pinned archive.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Asset {
    /// Where to fetch it.
    pub url: String,
    pub archive: ArchiveKind,
    /// Mandatory. See the module comment.
    pub sha256: Sha256Digest,
    /// Checked before hashing, so a truncated transfer fails fast and cheaply
    /// rather than after several megabytes of SHA-256.
    pub size_bytes: u64,
    /// Path of the executable inside the archive, e.g.
    /// `Release/whisper-server.exe`.
    ///
    /// Everything in this path's parent directory is installed alongside it.
    /// That is not tidiness: `whisper-server.exe` links against `whisper.dll`,
    /// `ggml.dll`, `ggml-base.dll` and one `ggml-cpu-*.dll` per
    /// microarchitecture, all shipped as siblings, and an install of the
    /// executable on its own does not start. Upstream copies a single file out
    /// of its archive, which works only because its fork links statically.
    pub executable_in_archive: String,
}

/// Whether `name` is a single, ordinary path component: not empty, not `.`
/// or `..`, not a drive prefix, and (implied by having no separator to begin
/// with) not more than one segment.
///
/// This is the same zip-slip family `install::payload_relative` refuses
/// inside an archive, cut down to the one-component case a plain file name
/// is. Shared rather than reinvented at each call site: [`SidecarSpec::problems`]
/// uses it so a bad `executableInArchive` is caught when the manifest is
/// read, and `install::extract_bare` uses it again so a `Bare` asset that
/// somehow got past that check still cannot copy itself to `dest.join("..")`.
///
/// `Asset::executable_name` already guarantees no `/` or `\` survives into
/// its result -- whatever follows the *last* separator cannot itself contain
/// one -- so this exists only to catch the separator-free ways a name can
/// still escape: `".."` on its own, `"."`, an empty string, or (on Windows) a
/// bare drive prefix like `"C:"`.
pub(crate) fn is_plain_file_name(name: &str) -> bool {
    let mut components = Path::new(name).components();
    matches!(components.next(), Some(Component::Normal(_))) && components.next().is_none()
}

impl Asset {
    /// The executable's file name, as it will be on disk after install.
    #[must_use]
    pub fn executable_name(&self) -> &str {
        let path = self.executable_in_archive.as_str();
        match path.rfind(['/', '\\']) {
            Some(index) => path.get(index + 1..).unwrap_or(path),
            None => path,
        }
    }

    /// The archive-internal directory whose contents are installed.
    ///
    /// Empty when the executable sits at the archive root.
    #[must_use]
    pub fn payload_dir(&self) -> &str {
        let path = self.executable_in_archive.as_str();
        match path.rfind(['/', '\\']) {
            Some(index) => path.get(..index).unwrap_or(""),
            None => "",
        }
    }
}

/// An inclusive range of loopback ports a sidecar may listen on.
///
/// A range rather than a fixed port because a second copy of the app, or a
/// wedged predecessor still holding the socket, must not make the feature
/// permanently unavailable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PortRange {
    pub first: u16,
    pub last: u16,
}

impl PortRange {
    /// Every port in the range, lowest first.
    pub fn ports(self) -> impl Iterator<Item = u16> {
        self.first..=self.last
    }

    #[must_use]
    pub const fn contains(self, port: u16) -> bool {
        port >= self.first && port <= self.last
    }

    /// Whether the range is the right way round and non-empty.
    #[must_use]
    pub const fn is_valid(self) -> bool {
        self.first != 0 && self.first <= self.last
    }
}

/// How this application talks to a sidecar once it is running.
///
/// Five of the sidecars in `docs/PORTING_MATRIX.md` are long-lived loopback
/// servers; seventeen are one-shot or streaming children that speak over their
/// standard streams and never bind anything. Both are sidecars — installed,
/// pinned, digest-verified and reaped the same way — and only this differs.
///
/// A tagged enum rather than an `Option<PortRange>` deliberately. An optional
/// field is absent both when a child genuinely does not listen and when
/// somebody forgot to write the range down, and those are not the same thing.
/// Here the manifest has to say which, and `deny_unknown_fields` plus a missing
/// `transport` is a deserialization error rather than a default.
// `rename_all` on an enum renames the *variants*; the fields inside them need
// `rename_all_fields`. Leaving it off is the mistake the completion plan records
// as defect 3, and it produced exactly the same symptom here — a config saying
// `portRange` refused for not being `port_range`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum Transport {
    /// A server on loopback. Its port is allocated from this range at start-up
    /// rather than fixed, because a previous run may still be holding one.
    Loopback { port_range: PortRange },
    /// A child process driven over stdin and stdout. Binds no port.
    Stdio,
}

impl Transport {
    /// The port range, for a sidecar that serves one.
    ///
    /// `None` means this sidecar does not listen at all — not that its range is
    /// unknown. A caller that needs a port must say what it wants to happen
    /// when there is not one.
    #[must_use]
    pub const fn port_range(&self) -> Option<PortRange> {
        match self {
            Self::Loopback { port_range } => Some(*port_range),
            Self::Stdio => None,
        }
    }
}

/// Where a pinned asset came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Source {
    /// e.g. `ggml-org/whisper.cpp`.
    pub repo: String,
    /// e.g. `b4938`.
    pub release: String,
    /// Why this release and not the newest one.
    pub pinned_because: String,
}

/// A sidecar this application knows how to install and run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SidecarSpec {
    /// Stable identifier. Becomes an install directory and a PID file name, so
    /// it has to be filesystem-safe.
    pub name: String,
    /// One line describing what it is for, shown in the UI.
    pub description: String,
    /// The upstream project and release this was pinned from, for the audit
    /// trail. Not used to build the URL — the URL is written out in full, so
    /// re-pinning is a visible diff rather than a computed surprise.
    pub source: Source,
    /// Loopback server or stdio child. See [`Transport`].
    pub transport: Transport,
    /// Substrings that must appear in a running process's command before this
    /// application will signal it. See `crate::reap`.
    pub command_fragments: Vec<String>,
    /// Per-target availability. A target absent from this map is reported as
    /// unknown rather than unsupported — the difference matters, because one is
    /// a gap in the manifest and the other is a fact about the world.
    pub targets: BTreeMap<Target, Availability>,
}

impl SidecarSpec {
    /// What can be done on a target.
    #[must_use]
    pub fn availability(&self, target: Target) -> TargetState<'_> {
        match self.targets.get(&target) {
            Some(Availability::Published(asset)) => TargetState::Installable(asset),
            Some(Availability::NotPublished { reason, tracking }) => {
                TargetState::Unsupported { reason, tracking }
            }
            None => TargetState::Unknown,
        }
    }

    /// The port range this sidecar serves on, or `None` for a stdio child.
    #[must_use]
    pub const fn port_range(&self) -> Option<PortRange> {
        self.transport.port_range()
    }

    /// Where this sidecar's files live once installed.
    #[must_use]
    pub fn install_dir(&self, root: &Path) -> PathBuf {
        root.join(&self.name)
    }

    /// Everything wrong with this spec; empty when it is sound.
    ///
    /// A list rather than the first problem, because whoever is fixing a
    /// hand-edited config wants all of them at once.
    #[must_use]
    pub fn problems(&self) -> Vec<String> {
        let mut problems = Vec::new();

        if self.name.is_empty() {
            problems.push("name is empty".to_owned());
        } else if !self
            .name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            problems.push(format!(
                "name {:?} must be ASCII alphanumeric, dash, or underscore: it becomes a \
                 directory name and a PID file name",
                self.name
            ));
        }
        // Only a range that exists can be wrong. A stdio child has nothing to
        // validate here, which is the point of modelling the two separately.
        if let Some(range) = self.transport.port_range() {
            if !range.is_valid() {
                problems.push(format!(
                    "port range {}-{} is empty or reversed",
                    range.first, range.last
                ));
            }
        }
        if self.command_fragments.is_empty() {
            problems.push(format!(
                "{} declares no command fragments, so a stale process could never be \
                 identified and would never be reaped",
                self.name
            ));
        }
        if self.targets.is_empty() {
            problems.push(format!("{} has no targets at all", self.name));
        }

        for (target, availability) in &self.targets {
            let Availability::Published(asset) = availability else {
                continue;
            };
            if asset.size_bytes == 0 {
                problems.push(format!("{}/{}: size is zero", self.name, target.as_str()));
            }
            if asset.executable_in_archive.is_empty() {
                problems.push(format!(
                    "{}/{}: no executable path inside the archive",
                    self.name,
                    target.as_str()
                ));
            } else if !is_plain_file_name(asset.executable_name()) {
                // Caught here, at manifest-read time, rather than only at
                // install time: `install::extract_bare` refuses the same
                // shape, but a `Zip`/`TarGz`/`TarXz` asset never reaches that
                // check at all, so this is the only place those catch it.
                problems.push(format!(
                    "{}/{}: executable name {:?} (from executableInArchive {:?}) is not a \
                     plain file name -- copying or looking it up by that name would escape \
                     the install directory",
                    self.name,
                    target.as_str(),
                    asset.executable_name(),
                    asset.executable_in_archive,
                ));
            } else if target.is_windows() != asset.executable_name().ends_with(".exe") {
                problems.push(format!(
                    "{}/{}: executable {:?} does not match the target's convention",
                    self.name,
                    target.as_str(),
                    asset.executable_name()
                ));
            }
            if !asset.url.starts_with("https://") {
                problems.push(format!(
                    "{}/{}: {:?} is not https. An executable fetched over plaintext is chosen \
                     by whoever controls the network, not by us.",
                    self.name,
                    target.as_str(),
                    asset.url
                ));
            }
        }

        problems
    }
}

/// What this build can do with a sidecar on a particular target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetState<'a> {
    Installable(&'a Asset),
    /// No asset exists, and we know why.
    Unsupported {
        reason: &'a str,
        tracking: &'a str,
    },
    /// The manifest says nothing about this target. A gap, not a fact.
    Unknown,
}

impl<'a> TargetState<'a> {
    #[must_use]
    pub const fn asset(self) -> Option<&'a Asset> {
        match self {
            Self::Installable(asset) => Some(asset),
            _ => None,
        }
    }

    #[must_use]
    pub const fn is_installable(self) -> bool {
        matches!(self, Self::Installable(_))
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

    const REAL: &str = "c2a4b60edb11f7e11a9191ffb50929535527d4d91c9903dbe3e554583bbbc63d";

    #[test]
    fn a_real_digest_parses() {
        assert_eq!(Sha256Digest::parse(REAL).unwrap().as_str(), REAL);
    }

    #[test]
    fn a_truncated_digest_is_refused_rather_than_padded() {
        let err = Sha256Digest::parse(&REAL[..40]).unwrap_err();
        assert!(matches!(err, DigestError::Length { found: 40, .. }));
    }

    #[test]
    fn an_uppercase_digest_is_refused_rather_than_normalised() {
        // Two spellings of one digest is two ways for a comparison to be wrong.
        let err = Sha256Digest::parse(&REAL.to_uppercase()).unwrap_err();
        assert!(matches!(err, DigestError::NotLowercase { .. }));
    }

    #[test]
    fn a_git_object_id_is_not_a_sha256() {
        let err = Sha256Digest::parse("f750196f750196f750196f750196f750196f7501").unwrap_err();
        assert!(matches!(err, DigestError::Length { found: 40, .. }));
    }

    #[test]
    fn a_placeholder_digest_is_refused() {
        // The shape someone reaches for when they want to fill this in later.
        for placeholder in ["", "TODO", "0", "sha256:abc"] {
            assert!(
                Sha256Digest::parse(placeholder).is_err(),
                "{placeholder:?} must not parse"
            );
        }
    }

    #[test]
    fn an_asset_without_a_digest_does_not_deserialize() {
        // The load-bearing property of this module: there is no way to describe
        // a binary we would run without knowing what it is.
        let json = r#"{
            "url": "https://example.invalid/x.zip",
            "archive": "zip",
            "sizeBytes": 10,
            "executableInArchive": "x.exe"
        }"#;
        let err = serde_json::from_str::<Asset>(json).unwrap_err();
        assert!(
            err.to_string().contains("sha256"),
            "the error should name the missing field, got {err}"
        );
    }

    #[test]
    fn an_asset_with_an_unknown_field_does_not_deserialize() {
        // `skipVerify: true` must not be silently ignored.
        let json = format!(
            r#"{{
                "url": "https://example.invalid/x.zip",
                "archive": "zip",
                "sha256": "{REAL}",
                "sizeBytes": 10,
                "executableInArchive": "x.exe",
                "skipVerify": true
            }}"#
        );
        assert!(serde_json::from_str::<Asset>(&json).is_err());
    }

    fn asset(exe: &str) -> Asset {
        Asset {
            url: "https://example.invalid/x.zip".to_owned(),
            archive: ArchiveKind::Zip,
            sha256: Sha256Digest::parse(REAL).unwrap(),
            size_bytes: 10,
            executable_in_archive: exe.to_owned(),
        }
    }

    fn bare_asset(exe: &str) -> Asset {
        Asset {
            url: "https://example.invalid/yt-dlp".to_owned(),
            archive: ArchiveKind::Bare,
            sha256: Sha256Digest::parse(REAL).unwrap(),
            size_bytes: 10,
            executable_in_archive: exe.to_owned(),
        }
    }

    #[test]
    fn the_payload_directory_is_the_executables_parent() {
        // Real layouts from the two archives this was built against.
        let windows = asset("Release/whisper-server.exe");
        assert_eq!(windows.payload_dir(), "Release");
        assert_eq!(windows.executable_name(), "whisper-server.exe");

        let linux = asset("whisper-bin-ubuntu-x64/whisper-server");
        assert_eq!(linux.payload_dir(), "whisper-bin-ubuntu-x64");
        assert_eq!(linux.executable_name(), "whisper-server");
    }

    #[test]
    fn an_executable_at_the_archive_root_has_an_empty_payload_directory() {
        let bare = asset("whisper-server");
        assert_eq!(bare.payload_dir(), "");
        assert_eq!(bare.executable_name(), "whisper-server");
    }

    #[test]
    fn a_windows_path_separator_is_handled_too() {
        let asset = asset("Release\\whisper-server.exe");
        assert_eq!(asset.payload_dir(), "Release");
        assert_eq!(asset.executable_name(), "whisper-server.exe");
    }

    #[test]
    fn is_plain_file_name_accepts_an_ordinary_name_and_refuses_the_escaping_shapes() {
        assert!(is_plain_file_name("yt-dlp"));
        assert!(is_plain_file_name("yt-dlp.exe"));
        for unsafe_name in ["..", ".", ""] {
            assert!(
                !is_plain_file_name(unsafe_name),
                "{unsafe_name:?} must be refused"
            );
        }
    }

    #[test]
    fn a_bare_asset_with_no_directory_part_has_an_empty_payload_directory() {
        // yt-dlp's shape on every platform: there is no archive around the
        // download at all, so `executableInArchive` is just a file name with
        // no directory part -- `payload_dir` and `executable_name` have to
        // keep behaving the same way they do for an archive-root entry.
        let windows = bare_asset("yt-dlp.exe");
        assert_eq!(windows.payload_dir(), "");
        assert_eq!(windows.executable_name(), "yt-dlp.exe");

        let unix = bare_asset("yt-dlp");
        assert_eq!(unix.payload_dir(), "");
        assert_eq!(unix.executable_name(), "yt-dlp");
    }

    #[test]
    fn tar_xz_and_bare_serialize_as_kebab_case() {
        assert_eq!(
            serde_json::to_string(&ArchiveKind::TarXz).unwrap(),
            "\"tar-xz\""
        );
        assert_eq!(
            serde_json::to_string(&ArchiveKind::Bare).unwrap(),
            "\"bare\""
        );
        assert_eq!(
            serde_json::from_str::<ArchiveKind>("\"tar-xz\"").unwrap(),
            ArchiveKind::TarXz
        );
        assert_eq!(
            serde_json::from_str::<ArchiveKind>("\"bare\"").unwrap(),
            ArchiveKind::Bare
        );
    }

    fn spec() -> SidecarSpec {
        let mut targets = BTreeMap::new();
        targets.insert(
            Target::WindowsX8664,
            Availability::Published(asset("Release/whisper-server.exe")),
        );
        targets.insert(
            Target::MacosAarch64,
            Availability::NotPublished {
                reason: "upstream publishes only an xcframework".to_owned(),
                tracking: "docs/ENGINEERING_LOG.md".to_owned(),
            },
        );
        SidecarSpec {
            name: "whisper".to_owned(),
            description: "Local speech-to-text".to_owned(),
            source: Source {
                repo: "ggml-org/whisper.cpp".to_owned(),
                release: "b4938".to_owned(),
                pinned_because: "a bump must not change transcription output".to_owned(),
            },
            transport: Transport::Loopback {
                port_range: PortRange {
                    first: 8178,
                    last: 8199,
                },
            },
            command_fragments: vec!["whisper-server".to_owned()],
            targets,
        }
    }

    #[test]
    fn a_sound_spec_has_no_problems() {
        assert_eq!(spec().problems(), Vec::<String>::new());
    }

    #[test]
    fn the_three_target_states_are_distinguishable() {
        let spec = spec();
        assert!(spec.availability(Target::WindowsX8664).is_installable());

        match spec.availability(Target::MacosAarch64) {
            TargetState::Unsupported { reason, .. } => assert!(reason.contains("xcframework")),
            other => panic!("{other:?}"),
        }

        // Nothing was said about Linux, which is a gap in the manifest rather
        // than a statement that Linux cannot run it.
        assert_eq!(spec.availability(Target::LinuxX8664), TargetState::Unknown);
    }

    #[test]
    fn a_plaintext_asset_url_is_a_problem() {
        let mut spec = spec();
        let mut bad = asset("Release/whisper-server.exe");
        bad.url = "http://example.invalid/x.zip".to_owned();
        spec.targets
            .insert(Target::WindowsX8664, Availability::Published(bad));
        let problems = spec.problems();
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("not https"));
    }

    #[test]
    fn an_executable_in_archive_of_dotdot_is_a_problem() {
        // The case that motivated `is_plain_file_name`: `executable_name`
        // returns the whole string when it has no separator, so an
        // `executableInArchive` of exactly `".."` would resolve one
        // directory above wherever that name is later joined -- caught here
        // rather than only when `Bare` tries to extract it.
        let mut spec = spec();
        spec.targets
            .insert(Target::LinuxX8664, Availability::Published(asset("..")));
        let problems = spec.problems();
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(
            problems[0].contains("not a plain file name"),
            "{problems:?}"
        );
    }

    #[test]
    fn a_spec_with_no_command_fragments_is_a_problem() {
        // Without a fragment the reaper can never confirm a PID is ours, so it
        // would either never kill a stale sidecar or kill something innocent.
        let mut spec = spec();
        spec.command_fragments.clear();
        assert!(spec
            .problems()
            .iter()
            .any(|p| p.contains("never be reaped")));
    }

    #[test]
    fn a_name_that_is_not_filesystem_safe_is_a_problem() {
        let mut spec = spec();
        spec.name = "../../etc".to_owned();
        assert!(spec.problems().iter().any(|p| p.contains("directory name")));
    }

    #[test]
    fn a_reversed_port_range_is_a_problem() {
        let mut spec = spec();
        spec.transport = Transport::Loopback {
            port_range: PortRange {
                first: 8199,
                last: 8178,
            },
        };
        assert!(spec.problems().iter().any(|p| p.contains("reversed")));
    }

    #[test]
    fn a_stdio_child_has_no_port_and_that_is_not_a_problem() {
        // The AEC helper, ffmpeg and yt-dlp are driven over their standard
        // streams. Before `Transport` they could only be described by inventing
        // a port range they would never bind.
        let mut spec = spec();
        spec.transport = Transport::Stdio;

        assert_eq!(spec.port_range(), None);
        assert!(
            spec.problems().is_empty(),
            "a stdio child is a valid sidecar: {:?}",
            spec.problems()
        );
    }

    #[test]
    fn a_spec_that_does_not_say_how_it_is_spoken_to_is_refused() {
        // The reason this is an enum rather than an `Option<PortRange>`: an
        // absent optional field would silently mean "stdio", so forgetting to
        // write a server's range down would produce a sidecar that installs and
        // then has nowhere to listen.
        let json = serde_json::json!({
            "name": "whisper",
            "description": "Local speech-to-text",
            "source": {
                "repo": "ggml-org/whisper.cpp",
                "release": "b4938",
                "pinnedBecause": "a bump must not change transcription output"
            },
            "commandFragments": ["whisper-server"],
            "targets": {}
        });

        let err = serde_json::from_value::<SidecarSpec>(json).unwrap_err();
        assert!(err.to_string().contains("transport"), "{err}");
    }

    #[test]
    fn both_transports_round_trip_through_the_config_format() {
        let loopback: Transport = serde_json::from_str(
            r#"{ "kind": "loopback", "portRange": { "first": 8178, "last": 8199 } }"#,
        )
        .unwrap();
        assert_eq!(
            loopback.port_range(),
            Some(PortRange {
                first: 8178,
                last: 8199
            })
        );

        let stdio: Transport = serde_json::from_str(r#"{ "kind": "stdio" }"#).unwrap();
        assert_eq!(stdio.port_range(), None);

        // A loopback transport without a range is a deserialization error, not
        // a range of zero.
        assert!(serde_json::from_str::<Transport>(r#"{ "kind": "loopback" }"#).is_err());
    }

    #[test]
    fn an_exe_suffix_on_a_unix_target_is_a_problem() {
        let mut spec = spec();
        spec.targets.insert(
            Target::LinuxX8664,
            Availability::Published(asset("dir/whisper-server.exe")),
        );
        assert!(spec
            .problems()
            .iter()
            .any(|p| p.contains("target's convention")));
    }

    #[test]
    fn all_problems_are_reported_at_once() {
        let mut spec = spec();
        spec.name = String::new();
        spec.command_fragments.clear();
        spec.transport = Transport::Loopback {
            port_range: PortRange { first: 0, last: 0 },
        };
        assert_eq!(spec.problems().len(), 3, "{:?}", spec.problems());
    }

    #[test]
    fn the_port_range_enumerates_inclusively() {
        let range = PortRange {
            first: 8178,
            last: 8199,
        };
        let ports: Vec<u16> = range.ports().collect();
        assert_eq!(ports.len(), 22);
        assert_eq!(ports.first(), Some(&8178));
        assert_eq!(ports.last(), Some(&8199));
        assert!(range.contains(8178) && range.contains(8199));
        assert!(!range.contains(8177) && !range.contains(8200));
    }

    #[test]
    fn the_current_target_is_one_we_publish_for_or_none() {
        // Not asserting a specific value: this test runs on three platforms.
        if let Some(target) = Target::current() {
            assert!(!target.as_str().is_empty());
        }
    }
}
