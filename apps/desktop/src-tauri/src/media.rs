//! Media import: turning a local audio/video file, or a URL, into a
//! transcript. The backend half of `Notes > Upload` -- the screen itself, and
//! the claims in `config/integration-manifest.json` and
//! `config/ui-surfaces.json`, belong to a later change.
//!
//! Three entry points, one pipeline underneath -- though only two of them are
//! reachable today; see **URL IMPORT, DORMANT** below:
//!
//! * **Audio** and **video** import both hand the source file to the pinned
//!   `ffmpeg` sidecar, which strips it to 16 kHz mono WAV
//!   ([`kw_audio_core::wav::TARGET_SAMPLE_RATE`]) regardless of whether the
//!   source had a video track. There is nothing else to tell them apart, so
//!   [`MediaImportRuntime::import_local_file`] serves both.
//! * **URL** import hands the URL to the pinned `yt-dlp` sidecar, which
//!   fetches it, and then joins the same ffmpeg conversion step.
//!
//! Every import then goes through **the existing transcription route** --
//! [`kw_transcription_core::route::resolve`] against the same
//! `transcription.route` setting dictation uses, executed by the same
//! [`kw_transcription_core::Executor`] dictation uses. An import is
//! transcribed by whatever provider the user already has configured. It does
//! not get a provider of its own, and if the route cannot be resolved (no
//! provider configured, a stale setting, a missing credential) that is
//! reported honestly rather than guessed at.
//!
//! # The network boundary, precisely
//!
//! URL import is the only place in this application where a user-typed URL
//! reaches the network, and this module makes exactly one direct use of
//! [`kw_network_policy`]: [`validate_import_url`] checks the URL -- scheme,
//! the vendor blocklist, and (going further than the generic policy does) a
//! literal private or link-local host -- under
//! [`RequestPurpose::UserTriggeredMediaImport`] and no other purpose, before
//! the URL is ever handed to anything that would fetch it. That check makes
//! no network request of its own.
//!
//! **What that check does not cover: yt-dlp's own fetch.** Once a URL passes
//! validation, fetching it is yt-dlp's job, running as a separate process.
//! yt-dlp has its own HTTP stack, its own DNS resolution, its own redirect
//! handling, and its own per-site extraction logic (some of which fetches
//! several other URLs it discovers along the way) -- none of it observed,
//! validated, or routed through `kw_network_policy`. In particular:
//!
//! * A hostname that resolves to a private address only at the moment yt-dlp
//!   connects (DNS rebinding) is not caught by the up-front literal-host
//!   check, because that check runs before any resolution happens.
//! * A redirect yt-dlp follows internally is not re-validated the way
//!   [`kw_network_policy::NetworkPolicy::download_to_file`] re-validates every
//!   hop for the requests that do go through it (sidecar and model
//!   downloads). That mechanism is real and is proven, for this exact
//!   purpose, by a test in `crates/network-policy/src/lib.rs` -- but yt-dlp's
//!   fetch does not run through it, so that proof does not extend to what
//!   yt-dlp itself does with the URL.
//!
//! Bounding yt-dlp's own traffic is done with flags it enforces itself
//! ([`ytdlp_download_args`]): a filesize cap, a bounded socket timeout,
//! playlist expansion turned off, and its own update/config-file checks
//! turned off. These are best-effort limits on a process this application
//! does not otherwise control, not a claim that they close every avenue --
//! see that function's doc comment for what each one does and does not
//! guarantee.
//!
//! # URL IMPORT, DORMANT
//!
//! Everything above about yt-dlp's own fetch describes a path that is **not
//! currently reachable**. `media_import_url` was withdrawn as a Tauri command
//! and `Notes > Upload` no longer offers a URL field, because the boundary
//! this module can enforce stops at the URL as typed: the up-front check
//! cannot see where a hostname resolves at connect time, nor any redirect or
//! extractor-generated request yt-dlp makes on its own. Until each of those
//! can be checked against the network policy, the entry point stays closed
//! and `media:url-import` is `reachable: false` in
//! `config/integration-manifest.json`, which carries the same reason.
//!
//! The implementation below is kept rather than deleted, so that re-enabling
//! it is a decision about the fetch boundary rather than a rewrite. Each
//! dormant item carries `#[allow(dead_code)]` pointing back at this section;
//! its tests still run. Nothing here is dead in the sense of abandoned --
//! only unregistered.
//!
//! # What is never logged
//!
//! No full URL (a query string can carry a token) and no imported media
//! content. Log lines here carry a sidecar name, a byte count, or a bare host
//! -- never a path with a query string attached, never audio or transcript
//! bytes.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, RwLock};

use kw_network_policy::{NetworkPolicy, RequestPurpose};
use kw_security::CredentialStore;
use kw_sidecar::{Catalog, RealFs, SearchPath};
use kw_storage::Db;
use kw_transcription_core::local::WhisperServer;
use kw_transcription_core::{Executor, Transport};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter as _, Runtime, State};
use ts_rs::TS;

/// The event import and sidecar-install progress is reported on.
pub const PROGRESS_EVENT: &str = "media-import-progress";

/// How much of a failed sidecar's stderr is kept for diagnostics.
const STDERR_TAIL_BYTES: usize = 4096;

/// The two sidecars this module drives. Deliberately a closed enum rather than
/// a catalogue name typed by the frontend: a stringly-typed sidecar id would
/// let a caller ask this command to install `whisper` or `llama` too, which is
/// not what "media import" means.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum MediaSidecar {
    Ffmpeg,
    YtDlp,
}

impl MediaSidecar {
    /// The name this sidecar is registered under in `config/sidecars.json`.
    const fn catalog_name(self) -> &'static str {
        match self {
            Self::Ffmpeg => "ffmpeg",
            Self::YtDlp => "yt-dlp",
        }
    }
}

/// What can be said about one media-import sidecar without touching the
/// network.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SidecarStatus {
    pub sidecar: MediaSidecar,
    /// Reused from `local_model` rather than redeclared: the same three
    /// states (installable / not published for this platform / a gap in the
    /// manifest) apply to any sidecar, and `local_llm` already reuses it for
    /// the same reason.
    pub availability: crate::local_model::Availability,
    pub installed: bool,
}

/// Where an imported transcript came from.
///
/// Never the full URL: a query string can carry a token, and this value may
/// end up quoted back into a note. The host is enough to show the user where
/// it came from.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum MediaImportSource {
    Audio,
    Video,
    #[allow(dead_code)] // dormant: see URL IMPORT, DORMANT in the module doc.
    Url {
        host: String,
    },
}

/// A committed imported note, its recording, and the transcription result.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct MediaImportResult {
    pub note_id: i64,
    pub recording: kw_notes_core::recordings::NoteRecording,
    pub retained_bytes: u64,
    pub text: String,
    pub provider: String,
    pub model: Option<String>,
    /// The transcript divided into timed utterances, when the engine timed it.
    ///
    /// **Empty means no timings were reported**, not "the recording was one
    /// utterance". Only the local whisper server reports them, and only because
    /// this path asks (`WhisperServer::timed`); a cloud provider answers with
    /// prose, so an import routed to one still has no cues. The screen writes
    /// one segment spanning the recording in that case and says as much.
    pub segments: Vec<ImportedSegment>,
    /// Computed from the converted WAV, not asked of the source file, so it
    /// reflects what was actually transcribed.
    pub duration_ms: u64,
    pub source: MediaImportSource,
}

/// One timed utterance of an imported transcript.
///
/// A local mirror of `kw_transcription_core::local::TimedSegment` because that
/// crate has no `ts-rs` dependency and should not gain one to cross an IPC
/// boundary it knows nothing about. The field names match
/// `kw_notes_core::transcript::Segment`, which is what the screen writes into.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ImportedSegment {
    pub text: String,
    /// Seconds from the start of the recording.
    pub timestamp: f64,
    pub end_timestamp: f64,
}

impl From<&kw_transcription_core::local::TimedSegment> for ImportedSegment {
    fn from(segment: &kw_transcription_core::local::TimedSegment) -> Self {
        Self {
            text: segment.text.clone(),
            timestamp: segment.start,
            end_timestamp: segment.end,
        }
    }
}

/// Progress of installing a media-import sidecar, or of running an import
/// once both are ready.
///
/// Shares the shape of `local_model::InstallProgress` and
/// `local_llm::LocalLlmProgress` -- a staged, tagged enum reported on one
/// event -- but is a distinctly named type so it cannot collide with either
/// in the generated TypeScript. `ipc_types` asserts against exactly that
/// collision.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(
    tag = "stage",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum MediaImportProgress {
    /// Installing a sidecar: downloading the pinned archive.
    Downloading {
        sidecar: String,
        written: u64,
        total: Option<u64>,
    },
    /// Installing a sidecar: hashing the download.
    Verifying,
    /// Installing a sidecar: unpacking it.
    Extracting,
    /// Running an import: yt-dlp is fetching the URL.
    #[allow(dead_code)] // dormant: see URL IMPORT, DORMANT in the module doc.
    Fetching,
    /// The install, or the import, finished.
    Done,
}

/// Why an import, or a sidecar install, did not finish.
///
/// Kept apart deliberately: a missing sidecar, a URL the boundary refused, a
/// file ffmpeg could not read, a fetch yt-dlp could not complete, and a
/// transcription provider that failed are five different problems with five
/// different fixes, and collapsing them into one message wastes the time of
/// whoever is trying to fix it.
#[derive(Debug, thiserror::Error)]
pub enum MediaImportError {
    #[error("{0}")]
    Busy(String),
    #[error("Could not retain imported recording: {detail}")]
    Persistence { detail: String },
    #[error("{sidecar} is not available on this platform: {reason}")]
    SidecarUnavailable {
        sidecar: &'static str,
        reason: String,
    },

    #[error("{explain} Install it, then try again.")]
    SidecarNotInstalled { explain: String },

    #[error("could not install {sidecar}: {detail}")]
    SidecarInstall {
        sidecar: &'static str,
        detail: String,
    },

    #[error("that url was refused: {reason}")]
    #[allow(dead_code)] // dormant: see URL IMPORT, DORMANT in the module doc.
    UrlRefused { reason: String },

    #[error("could not read {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("ffmpeg could not convert this file: {detail}")]
    Conversion { detail: String },

    #[error("yt-dlp could not fetch that url: {detail}")]
    #[allow(dead_code)] // dormant: see URL IMPORT, DORMANT in the module doc.
    Fetch { detail: String },

    #[error("no transcription route is configured: {detail}")]
    RouteUnresolved { detail: String },

    #[error("the configured transcription provider failed: {detail}")]
    Transcription { detail: String },
}

impl MediaImportError {
    fn io(path: &Path, source: std::io::Error) -> Self {
        Self::Io {
            path: path.display().to_string(),
            source,
        }
    }
}

/* -------------------------------------------------------------- pure helpers */
//
// Everything below this line takes and returns ordinary types -- no
// `AppHandle`, no `State`. Naming a Tauri-dependent function as a value in a
// test kills the whole test binary on Windows, so the parts worth unit
// testing are kept free of Tauri entirely, the same way `meeting.rs`,
// `updater.rs` and `calendar.rs` do.

/// Validate a user-typed URL against the network boundary, under
/// [`RequestPurpose::UserTriggeredMediaImport`] and no other purpose.
///
/// Goes one step further than [`kw_network_policy::validate`] on its own:
/// that function permits any host over `https`, including a private or
/// link-local one, because most purposes have no reason to special-case a
/// host string and `https` to an internal address is nobody's SSRF vector
/// *through this function*. URL import is different -- "fetch whatever is at
/// this address" is exactly the shape yt-dlp's generic extractor takes, so a
/// private-looking host is refused outright here rather than left to the
/// process that is about to fetch it.
///
/// This is a string check on the URL as typed, made once, before anything is
/// spawned. It does not, and cannot, prove anything about where a hostname
/// resolves at the moment yt-dlp connects to it, or about any redirect yt-dlp
/// follows on its own -- see the module doc.
#[allow(dead_code)] // dormant: see URL IMPORT, DORMANT in the module doc.
fn validate_import_url(url: &str) -> Result<url::Url, MediaImportError> {
    let parsed = kw_network_policy::validate(url, RequestPurpose::UserTriggeredMediaImport)
        .map_err(|err| MediaImportError::UrlRefused {
            reason: err.to_string(),
        })?;

    let host = parsed
        .host_str()
        .ok_or_else(|| MediaImportError::UrlRefused {
            reason: "the url has no host".to_owned(),
        })?;

    if kw_network_policy::is_private_host(host) {
        return Err(MediaImportError::UrlRefused {
            reason: format!(
                "{host} looks like a private or link-local address; URL import only fetches \
                 addresses on the public internet"
            ),
        });
    }

    Ok(parsed)
}

/// The ffmpeg arguments that convert any readable audio or video file to 16
/// kHz mono WAV.
///
/// `-vn` drops any video stream. Harmless when there was never one -- a plain
/// audio file passes through untouched by it -- which is exactly why audio
/// and video import share one conversion path.
fn ffmpeg_convert_args(input: &Path, output: &Path) -> Vec<String> {
    vec![
        "-hide_banner".to_owned(),
        "-loglevel".to_owned(),
        "error".to_owned(),
        "-y".to_owned(),
        "-i".to_owned(),
        input.display().to_string(),
        "-vn".to_owned(),
        "-ac".to_owned(),
        "1".to_owned(),
        "-ar".to_owned(),
        kw_audio_core::wav::TARGET_SAMPLE_RATE.to_string(),
        "-f".to_owned(),
        "wav".to_owned(),
        output.display().to_string(),
    ]
}

/// The largest file yt-dlp will keep. Best-effort: yt-dlp can only enforce
/// this against a size the remote server declares in advance, and a server
/// that does not declare one is not capped by this flag at all.
#[allow(dead_code)] // dormant: see URL IMPORT, DORMANT in the module doc.
const YT_DLP_MAX_FILESIZE: &str = "500M";

/// How long yt-dlp will wait on a stalled connection before giving up.
#[allow(dead_code)] // dormant: see URL IMPORT, DORMANT in the module doc.
const YT_DLP_SOCKET_TIMEOUT_SECS: u32 = 30;

/// The yt-dlp arguments for fetching `url` to `output_template`.
///
/// `output_template` should end in a fixed prefix and `.%(ext)s`, so the
/// caller can find the one file this produces by prefix without trusting
/// anything yt-dlp printed. Passed as a real argv entry, not through a shell,
/// so nothing in `url` needs escaping.
///
/// What each flag does and does not guarantee:
/// * `-f bestaudio/best` prefers an audio-only stream, so there is nothing
///   for yt-dlp to merge and no second ffmpeg (yt-dlp's own, not this
///   application's pinned one) gets invoked to merge it.
/// * `--max-filesize` and `--socket-timeout` bound yt-dlp's own transfer.
///   Real, but best-effort against a server that does not declare a size
///   upfront.
/// * `--no-playlist` fetches the one item the URL names, not a whole
///   playlist it might belong to.
/// * `--no-update` and `--no-config` stop yt-dlp from checking for its own
///   updates or reading a config file this application did not write.
/// * `--restrict-filenames` keeps the produced filename plain ASCII, which
///   matters because native binaries on this machine (ffmpeg included) can
///   choke on a path with unusual characters in it.
///
/// None of these touch DNS resolution, redirect-following, or certificate
/// validation inside yt-dlp's own fetch -- see the module doc.
#[allow(dead_code)] // dormant: see URL IMPORT, DORMANT in the module doc.
fn ytdlp_download_args(url: &str, output_template: &Path) -> Vec<String> {
    vec![
        "--no-playlist".to_owned(),
        "--no-update".to_owned(),
        "--no-config".to_owned(),
        "--restrict-filenames".to_owned(),
        "-f".to_owned(),
        "bestaudio/best".to_owned(),
        "--max-filesize".to_owned(),
        YT_DLP_MAX_FILESIZE.to_owned(),
        "--socket-timeout".to_owned(),
        YT_DLP_SOCKET_TIMEOUT_SECS.to_string(),
        "-o".to_owned(),
        output_template.display().to_string(),
        url.to_owned(),
    ]
}

/// The fixed base name yt-dlp is asked to write its download as, so the
/// produced file can be found by prefix rather than by parsing anything
/// yt-dlp printed.
#[allow(dead_code)] // dormant: see URL IMPORT, DORMANT in the module doc.
const DOWNLOAD_PREFIX: &str = "downloaded";

/// The byte length of the canonical WAV header
/// ([`kw_audio_core::wav`]'s `HEADER_LEN`, not exported, so restated as the
/// fixed constant it is).
const WAV_HEADER_LEN: usize = 44;

/// How long a 16 kHz mono 16-bit WAV of `byte_len` bytes plays for.
fn wav_duration_ms(byte_len: usize) -> u64 {
    let data_len = byte_len.saturating_sub(WAV_HEADER_LEN) as u64;
    let samples = data_len / 2;
    samples.saturating_mul(1000) / u64::from(kw_audio_core::wav::TARGET_SAMPLE_RATE)
}

/// The last `max` bytes of `bytes`, without indexing directly (this workspace
/// warns on it, and `-D warnings` makes that an error).
fn tail_bytes(bytes: &[u8], max: usize) -> &[u8] {
    let start = bytes.len().saturating_sub(max);
    bytes.get(start..).unwrap_or(bytes)
}

/// Whether `path` names a file that can be imported.
///
/// `path` is the canonical path [`crate::commands::DialogGrants::authorise`]
/// returned, not a string the renderer supplied -- *whether this path may be
/// read at all* is decided in the command, before this. What is left here is
/// whether the granted path still names a file: a directory canonicalises
/// perfectly well, and a file picked and then deleted keeps its grant for the
/// rest of the session.
///
/// Extracted so this check is unit-testable on its own: nothing outside a
/// running application can construct a real `tauri::AppHandle`, so any
/// function that needs one -- as [`MediaImportRuntime::import_local_file`]
/// does, to report progress -- cannot be called from a plain unit test at
/// all, not even to reach a branch that never uses it.
fn check_importable_file(path: &Path) -> Result<PathBuf, MediaImportError> {
    if !path.is_file() {
        return Err(MediaImportError::io(
            path,
            std::io::Error::new(std::io::ErrorKind::NotFound, "not a file"),
        ));
    }
    Ok(path.to_path_buf())
}

/// yt-dlp's own markers for a download that has not finished yet. A file
/// bearing one of these must never be mistaken for the finished download,
/// even if yt-dlp's exit code were somehow misleading -- a partially fetched
/// file must not be presented as a complete import.
#[allow(dead_code)] // dormant: see URL IMPORT, DORMANT in the module doc.
const INCOMPLETE_DOWNLOAD_SUFFIXES: [&str; 2] = [".part", ".ytdl"];

/// Find the one file a sidecar left behind in `dir` whose name starts with
/// `prefix`.
///
/// Zero matches and more than one are both reported rather than guessed at:
/// zero means the process did not produce what it claimed to, and more than
/// one means this function's assumption about the output shape is wrong --
/// either way, picking one silently would risk transcribing the wrong file.
/// A file still carrying one of [`INCOMPLETE_DOWNLOAD_SUFFIXES`] is excluded
/// outright rather than counted as a match.
#[allow(dead_code)] // dormant: see URL IMPORT, DORMANT in the module doc.
fn pick_downloaded_file(dir: &Path, prefix: &str) -> Result<PathBuf, MediaImportError> {
    let mut matches: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|source| MediaImportError::io(dir, source))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| {
                    name.starts_with(prefix)
                        && !INCOMPLETE_DOWNLOAD_SUFFIXES
                            .iter()
                            .any(|suffix| name.ends_with(suffix))
                })
        })
        .collect();

    match matches.len() {
        0 => Err(MediaImportError::Fetch {
            detail: "yt-dlp finished but left no downloaded file behind".to_owned(),
        }),
        1 => Ok(matches.remove(0)),
        found => Err(MediaImportError::Fetch {
            detail: format!("yt-dlp finished but left {found} files behind, expected exactly one"),
        }),
    }
}

/// What running a one-shot sidecar process produced.
struct ProcessOutcome {
    success: bool,
    /// Bounded to [`STDERR_TAIL_BYTES`]. Never logged wholesale -- surfaced
    /// only in the error a caller sees when the process failed.
    stderr_tail: String,
}

/// Run `executable` to completion and report whether it succeeded.
///
/// No health check and no long-lived handle: unlike the loopback servers
/// `kw_sidecar::process` manages, ffmpeg and yt-dlp are one-shot children that
/// exit when they are done. Standard input is closed so neither can block
/// waiting for it; standard output is discarded (yt-dlp writes titles and
/// filenames there, never logged); standard error is kept, bounded, for when
/// something needs explaining.
async fn run_sidecar_process(
    executable: &Path,
    args: &[String],
) -> std::io::Result<ProcessOutcome> {
    let mut command = tokio::process::Command::new(executable);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .kill_on_drop(true);

    #[cfg(windows)]
    {
        // CREATE_NO_WINDOW. A console flashing up during an import would be
        // its own bug report, the same reason `kw_sidecar::process` sets it.
        command.creation_flags(0x0800_0000);
    }

    let output = command.output().await?;
    Ok(ProcessOutcome {
        success: output.status.success(),
        stderr_tail: String::from_utf8_lossy(tail_bytes(&output.stderr, STDERR_TAIL_BYTES))
            .into_owned(),
    })
}

/// A fresh, empty scratch directory for one import, under the same
/// native-binary-safe root ffmpeg's model cache uses.
fn new_scratch_dir() -> Result<PathBuf, MediaImportError> {
    let dir = kw_storage::paths::audio_temp_dir()
        .join("import")
        .join(uuid::Uuid::new_v4().to_string());
    std::fs::create_dir_all(&dir).map_err(|source| MediaImportError::io(&dir, source))?;
    Ok(dir)
}

/// Turn a [`kw_sidecar::SidecarError`] from resolving a catalogue entry into
/// the honest reason this sidecar cannot be used here -- not published for
/// this platform, or a gap in the manifest. Distinct from "not installed",
/// which means it could be used here and simply has not been downloaded yet.
fn describe_unavailable(sidecar: MediaSidecar, err: kw_sidecar::SidecarError) -> MediaImportError {
    MediaImportError::SidecarUnavailable {
        sidecar: sidecar.catalog_name(),
        reason: err.to_string(),
    }
}

/* --------------------------------------------------------------- the runtime */

/// Where a whisper server's shared handles live -- see `dictation::LocalPort`.
type LocalPort = Arc<RwLock<Option<u16>>>;

/// Everything media import needs: the sidecar catalogue, the network policy,
/// and exactly what the existing transcription route needs to run --
/// deliberately the same four things `dictation::RouteTranscriber` holds, so
/// an import is transcribed by the same route a dictation would be, not a
/// second implementation of "send this to a provider".
pub struct MediaImportRuntime {
    catalog: Catalog,
    policy: NetworkPolicy,
    credentials: Arc<dyn CredentialStore>,
    local_port: LocalPort,
    local_model: Arc<RwLock<Option<String>>>,
    /// Which sherpa model is selected. Held for the same reason `local_port` is
    /// -- an import goes through the *same* route dictation does, so it must
    /// reach the same local engine. Resolving one of them to whisper and the
    /// other to sherpa would make `Transport::Local` mean two things.
    local_sherpa: crate::local_asr::Selection,
    /// How to start the whisper server again if it has gone.
    ///
    /// Held for the same reason `local_port` is, and it matters more here than
    /// in dictation: a dictation whose server has died can be spoken again, and
    /// an imported recording generally cannot.
    local_revive: Arc<dyn crate::local_model::Revive>,
    /// Where downloaded sidecars live. Held rather than read from
    /// `kw_storage::paths` at each use, so a test can point it at an empty
    /// directory -- see [`MediaImportRuntime::rooted`].
    sidecar_root: PathBuf,
    recording_directory: PathBuf,
    db: Db,
    jobs: crate::media_job::ImportJobs,
}

impl std::fmt::Debug for MediaImportRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MediaImportRuntime").finish_non_exhaustive()
    }
}

impl MediaImportRuntime {
    /// Load the catalogue and hold the handles a transcription needs, without
    /// touching the network.
    pub fn new(
        policy: NetworkPolicy,
        credentials: Arc<dyn CredentialStore>,
        local_port: LocalPort,
        local_model: Arc<RwLock<Option<String>>>,
        local_sherpa: crate::local_asr::Selection,
        local_revive: Arc<dyn crate::local_model::Revive>,
        db: Db,
    ) -> Result<Self, String> {
        Self::rooted(
            policy,
            credentials,
            local_port,
            local_model,
            local_sherpa,
            local_revive,
            db,
            kw_storage::paths::sidecar_dir().path,
        )
    }

    /// [`Self::new`], with the directory downloaded sidecars live in supplied
    /// rather than read from the environment.
    ///
    /// Injected for the reason `binary_locator::locate` takes its search inputs
    /// as parameters: a test that reads the real install directory is a test
    /// about the machine. These two used to assert "nothing was downloaded in
    /// this test" against the real path, which held only while nobody had ever
    /// used the feature -- and stopped holding the moment a bench check on
    /// 2026-08-28 installed ffmpeg through the screen. A suite that fails
    /// because the application was used is a suite nobody can trust.
    #[allow(clippy::too_many_arguments)]
    fn rooted(
        policy: NetworkPolicy,
        credentials: Arc<dyn CredentialStore>,
        local_port: LocalPort,
        local_model: Arc<RwLock<Option<String>>>,
        local_sherpa: crate::local_asr::Selection,
        local_revive: Arc<dyn crate::local_model::Revive>,
        db: Db,
        sidecar_root: PathBuf,
    ) -> Result<Self, String> {
        let catalog = Catalog::shipped().map_err(|err| err.to_string())?;
        Ok(Self {
            catalog,
            policy,
            credentials,
            local_port,
            local_model,
            local_sherpa,
            local_revive,
            db,
            sidecar_root,
            recording_directory: kw_notes_core::recordings::recording_dir(),
            jobs: crate::media_job::ImportJobs::default(),
        })
    }

    fn search_path(&self) -> SearchPath {
        SearchPath {
            bundled: None,
            downloaded: self.sidecar_root.clone(),
        }
    }

    fn is_installed(&self, sidecar: MediaSidecar) -> bool {
        let name = sidecar.catalog_name();
        self.catalog.resolve(name).ok().is_some_and(|(_, asset)| {
            kw_sidecar::locate(name, asset, &self.search_path(), &RealFs).is_installed()
        })
    }

    /// What can be said about one sidecar without a download.
    #[must_use]
    pub fn sidecar_status(&self, sidecar: MediaSidecar) -> SidecarStatus {
        SidecarStatus {
            sidecar,
            availability: crate::local_model::Availability::of(
                &self.catalog,
                sidecar.catalog_name(),
            ),
            installed: self.is_installed(sidecar),
        }
    }

    /// Find the installed executable, or explain honestly why there is none:
    /// not published for this platform, or published but not downloaded.
    fn locate_executable(&self, sidecar: MediaSidecar) -> Result<PathBuf, MediaImportError> {
        let name = sidecar.catalog_name();
        let (_, asset) = self
            .catalog
            .resolve(name)
            .map_err(|err| describe_unavailable(sidecar, err))?;

        let installation = kw_sidecar::locate(name, asset, &self.search_path(), &RealFs);
        installation
            .executable()
            .map(Path::to_path_buf)
            .ok_or_else(|| MediaImportError::SidecarNotInstalled {
                explain: installation.explain(name),
            })
    }

    /// Download, verify and unpack a media-import sidecar, reporting progress
    /// on [`PROGRESS_EVENT`].
    pub async fn install_sidecar<R: Runtime>(
        &self,
        app: &AppHandle<R>,
        sidecar: MediaSidecar,
    ) -> Result<(), MediaImportError> {
        let name = sidecar.catalog_name();
        let (spec, asset) = self
            .catalog
            .resolve(name)
            .map_err(|err| describe_unavailable(sidecar, err))?;

        let root = kw_storage::paths::sidecar_dir().path;

        kw_sidecar::install(&self.policy, spec, asset, &root, &mut |progress| {
            emit_progress(
                app,
                match progress {
                    kw_sidecar::Progress::Downloading { written, total } => {
                        MediaImportProgress::Downloading {
                            sidecar: name.to_owned(),
                            written,
                            total,
                        }
                    }
                    kw_sidecar::Progress::Verifying => MediaImportProgress::Verifying,
                    kw_sidecar::Progress::Extracting => MediaImportProgress::Extracting,
                },
            );
        })
        .await
        .map_err(|err| MediaImportError::SidecarInstall {
            sidecar: name,
            detail: err.to_string(),
        })?;

        emit_progress(app, MediaImportProgress::Done);
        Ok(())
    }

    /// The whisper server to send an import to, starting a new one if the
    /// recorded server has gone.
    ///
    /// The same rule dictation follows, and for a stronger reason: an import is
    /// often the only copy of a recording, so failing it against a dead port
    /// spends something that cannot be spent twice.
    async fn whisper_server(&self) -> Result<Option<WhisperServer>, MediaImportError> {
        let recorded = self.local_port.read().ok().and_then(|held| *held);
        let port =
            crate::local_model::ensure_whisper(&self.policy, recorded, self.local_revive.as_ref())
                .await
                .map_err(|detail| MediaImportError::Transcription { detail })?;

        let model = self.local_model.read().ok().and_then(|held| held.clone());
        // `.timed()`: an import becomes a note that can be exported as SRT or
        // WebVTT, and without timings that export is one cue spanning the whole
        // recording. Dictation does not ask, because it wants a line of text and
        // the timings cost about 45% more wall clock -- measured, see
        // `kw_transcription_core::local`'s module doc.
        Ok(Some(
            WhisperServer::new(self.policy.clone(), port, model).timed(),
        ))
    }

    /// The local engine for an import: the same one dictation would use.
    async fn local_engine(
        &self,
    ) -> Result<Option<Box<dyn kw_transcription_core::local::LocalTranscriber>>, MediaImportError>
    {
        match crate::local_asr::resolve(&self.local_sherpa) {
            Some(Ok(offline)) => Ok(Some(Box::new(offline))),
            Some(Err(detail)) => Err(MediaImportError::RouteUnresolved { detail }),
            None => Ok(self
                .whisper_server()
                .await?
                .map(|w| Box::new(w) as Box<dyn kw_transcription_core::local::LocalTranscriber>)),
        }
    }

    /// Resolve where a transcription would go right now -- the same setting
    /// and the same resolver dictation uses. Not cached: settings can change
    /// between imports.
    async fn route_transport(&self) -> Result<Transport, MediaImportError> {
        let registry =
            kw_provider_registry::registry().map_err(|err| MediaImportError::RouteUnresolved {
                detail: err.to_string(),
            })?;

        let settings: kw_transcription_core::RouteSettings =
            kw_storage::settings::get(self.db.pool(), crate::dictation::ROUTE_SETTINGS_KEY)
                .await
                .ok()
                .flatten()
                .unwrap_or_default();

        kw_transcription_core::route::resolve(
            registry,
            &settings,
            &kw_transcription_core::RouteRequest::default(),
        )
        .map_err(|err| MediaImportError::RouteUnresolved {
            detail: err.to_string(),
        })
    }

    /// Send `wav` down the existing transcription route. Not a second path to
    /// a provider: the same [`Executor`] and the same resolved [`Transport`]
    /// a dictation would use.
    async fn prepare(&self) -> Result<PreparedImport, MediaImportError> {
        let transport = self.route_transport().await?;
        let local = if matches!(transport, Transport::Local) {
            self.local_engine().await?
        } else {
            None
        };
        Ok(PreparedImport { transport, local })
    }

    async fn transcribe(
        &self,
        wav: Vec<u8>,
        prepared: PreparedImport,
    ) -> Result<kw_transcription_core::Transcript, MediaImportError> {
        let PreparedImport { transport, local } = prepared;
        let executor = Executor::new(&self.policy, self.credentials.as_ref());
        let executor = match &local {
            Some(engine) => executor.with_local(engine.as_ref()),
            None => executor,
        };

        // Byte count only -- see `Executor::post_multipart`'s own logging,
        // which this inherits. Never the audio, never the transcript.
        tracing::info!(bytes = wav.len(), "importing media for transcription");

        executor
            .transcribe(&transport, wav)
            .await
            .map_err(|err| MediaImportError::Transcription {
                detail: err.to_string(),
            })
    }

    /// Convert `input` with ffmpeg, read the result, and transcribe it.
    ///
    /// Keep the scratch WAV through transcription so a successful import can
    /// copy it into owned storage. The caller removes scratch on either outcome.
    async fn convert_and_transcribe<R: Runtime>(
        &self,
        app: &AppHandle<R>,
        ffmpeg: &Path,
        input: &Path,
        scratch: &Path,
        lease: Option<&crate::media_job::ImportLease<'_>>,
    ) -> Result<(kw_transcription_core::Transcript, u64), MediaImportError> {
        let prepared = self.prepare().await?;
        if let Some(lease) = lease {
            let (provider, model) = prepared.identity();
            lease.route(provider, model);
            lease.stage(crate::media_job::ImportStage::Converting);
        }
        let _ = app;

        let output = scratch.join("converted.wav");
        let args = ffmpeg_convert_args(input, &output);
        let outcome = run_sidecar_process(ffmpeg, &args)
            .await
            .map_err(|source| MediaImportError::io(ffmpeg, source))?;

        if !outcome.success || !output.is_file() {
            return Err(MediaImportError::Conversion {
                detail: if outcome.stderr_tail.trim().is_empty() {
                    "ffmpeg exited without producing an output file. The source may not be a \
                     readable audio or video file."
                        .to_owned()
                } else {
                    outcome.stderr_tail
                },
            });
        }

        let wav = tokio::fs::read(&output)
            .await
            .map_err(|source| MediaImportError::io(&output, source))?;
        if wav.len() <= WAV_HEADER_LEN {
            return Err(MediaImportError::Conversion {
                detail: "the converted file has no audio in it. The source may have no audio \
                         track."
                    .to_owned(),
            });
        }
        let duration_ms = wav_duration_ms(wav.len());

        if let Some(lease) = lease {
            lease.stage(crate::media_job::ImportStage::Transcribing);
        }
        let transcript = self.transcribe(wav, prepared).await?;
        Ok((transcript, duration_ms))
    }

    /// Import an already-local audio or video file.
    ///
    /// `path` is a path a real OS dialog returned, authorised by
    /// [`crate::commands::DialogGrants`] in the command above this. Nothing
    /// here re-derives that: this function is reachable only through
    /// [`media_import_audio`] and [`media_import_video`], both of which
    /// authorise first.
    pub async fn import_local_file<R: Runtime>(
        &self,
        app: &AppHandle<R>,
        path: &Path,
        source: MediaImportSource,
    ) -> Result<MediaImportResult, MediaImportError> {
        let lease = self.jobs.begin().map_err(MediaImportError::Busy)?;
        let result = self.import_local_inner(app, path, source, &lease).await;
        lease.finish(&result.as_ref().cloned().map_err(ToString::to_string));
        result
    }

    async fn import_local_inner<R: Runtime>(
        &self,
        app: &AppHandle<R>,
        path: &Path,
        source: MediaImportSource,
        lease: &crate::media_job::ImportLease<'_>,
    ) -> Result<MediaImportResult, MediaImportError> {
        let input = check_importable_file(path)?;

        let ffmpeg = self.locate_executable(MediaSidecar::Ffmpeg)?;
        let scratch = new_scratch_dir()?;
        let _cleanup = ScratchCleanup(scratch.clone());

        let outcome = self
            .convert_and_transcribe(app, &ffmpeg, &input, &scratch, Some(lease))
            .await;
        let result = match outcome {
            Ok((transcript, _)) => {
                lease.stage(crate::media_job::ImportStage::Saving);
                save_import(
                    &self.db,
                    transcript,
                    source,
                    &input,
                    &scratch.join("converted.wav"),
                    &self.recording_directory,
                )
                .await
            }
            Err(error) => Err(error),
        };
        if let Err(error) = std::fs::remove_dir_all(&scratch) {
            tracing::warn!(%error, "could not remove media import scratch directory");
        }
        result
    }

    /// Import media fetched from a URL by the yt-dlp sidecar. See the module
    /// doc for exactly what protection applies to the fetch and what does
    /// not.
    #[allow(dead_code)] // dormant: see URL IMPORT, DORMANT in the module doc.
    pub async fn import_url<R: Runtime>(
        &self,
        app: &AppHandle<R>,
        url: &str,
    ) -> Result<MediaImportResult, MediaImportError> {
        let validated = validate_import_url(url)?;
        let host = validated.host_str().unwrap_or("").to_owned();

        let ytdlp = self.locate_executable(MediaSidecar::YtDlp)?;
        let ffmpeg = self.locate_executable(MediaSidecar::Ffmpeg)?;

        let scratch = new_scratch_dir()?;

        let outcome = self
            .fetch_and_transcribe(app, &ytdlp, &ffmpeg, url, &scratch)
            .await;
        let result = match outcome {
            Ok((transcript, _)) => {
                save_import(
                    &self.db,
                    transcript,
                    MediaImportSource::Url { host },
                    Path::new("download.wav"),
                    &scratch.join("converted.wav"),
                    &self.recording_directory,
                )
                .await
            }
            Err(error) => Err(error),
        };
        let _ = std::fs::remove_dir_all(&scratch);
        result
    }

    #[allow(dead_code)] // dormant: see URL IMPORT, DORMANT in the module doc.
    async fn fetch_and_transcribe<R: Runtime>(
        &self,
        app: &AppHandle<R>,
        ytdlp: &Path,
        ffmpeg: &Path,
        url: &str,
        scratch: &Path,
    ) -> Result<(kw_transcription_core::Transcript, u64), MediaImportError> {
        emit_progress(app, MediaImportProgress::Fetching);

        let template = scratch.join(format!("{DOWNLOAD_PREFIX}.%(ext)s"));
        let args = ytdlp_download_args(url, &template);
        let outcome = run_sidecar_process(ytdlp, &args)
            .await
            .map_err(|source| MediaImportError::io(ytdlp, source))?;

        if !outcome.success {
            return Err(MediaImportError::Fetch {
                detail: if outcome.stderr_tail.trim().is_empty() {
                    "yt-dlp exited with an error and produced no diagnostics".to_owned()
                } else {
                    outcome.stderr_tail
                },
            });
        }

        let downloaded = pick_downloaded_file(scratch, DOWNLOAD_PREFIX)?;
        self.convert_and_transcribe(app, ffmpeg, &downloaded, scratch, None)
            .await
    }
}

/// Persist only after transcription succeeds; a failed transaction owns no file.
async fn save_import(
    db: &Db,
    transcript: kw_transcription_core::Transcript,
    source: MediaImportSource,
    original: &Path,
    converted: &Path,
    directory: &Path,
) -> Result<MediaImportResult, MediaImportError> {
    let converted = converted.to_owned();
    let directory = directory.to_owned();
    let mut pending = tokio::task::spawn_blocking(move || {
        crate::recording_attach::prepare_copy(&converted, &directory)
    })
    .await
    .map_err(|e| MediaImportError::Persistence {
        detail: e.to_string(),
    })?
    .map_err(|detail| MediaImportError::Persistence { detail })?;
    let original_name = original
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let title = original
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let duration_ms = (pending.duration * 1000.0).round() as u64;
    let segments: Vec<ImportedSegment> = transcript.segments.iter().map(Into::into).collect();
    let saved_segments = if segments.is_empty() {
        vec![ImportedSegment {
            text: transcript.text.clone(),
            timestamp: 0.0,
            end_timestamp: pending.duration,
        }]
    } else {
        segments.clone()
    };
    let note = kw_notes_core::notes::NewNote {
        title,
        content: transcript.text.clone(),
        note_type: Some("import".to_owned()),
        transcript: Some(serde_json::to_string(&saved_segments).map_err(|e| {
            MediaImportError::Persistence {
                detail: e.to_string(),
            }
        })?),
        audio_duration_seconds: Some(pending.duration),
        ..Default::default()
    };
    let retained_bytes = std::fs::metadata(&pending.path)
        .map_err(|e| MediaImportError::io(&pending.path, e))?
        .len();
    let (note_id, recording) = kw_notes_core::notes::create_with_recording(
        db.pool(),
        &note,
        &pending.key,
        pending.duration,
        &original_name,
    )
    .await
    .map_err(|e| MediaImportError::Persistence {
        detail: format!("Could not save the imported note and recording: {e}"),
    })?;
    pending.committed = true;
    Ok(MediaImportResult {
        note_id,
        recording,
        retained_bytes,
        text: transcript.text,
        provider: transcript.provider,
        model: transcript.model,
        segments,
        duration_ms,
        source,
    })
}

fn emit_progress<R: Runtime>(app: &AppHandle<R>, progress: MediaImportProgress) {
    if let Err(err) = app.emit(PROGRESS_EVENT, progress) {
        tracing::debug!(error = %err, "could not report media import progress");
    }
}

/* --------------------------------------------------------------- commands */

/// Report ffmpeg's and yt-dlp's availability and install state.
#[tauri::command]
pub async fn media_import_status(
    runtime: State<'_, Arc<MediaImportRuntime>>,
) -> Result<Vec<SidecarStatus>, String> {
    Ok(vec![
        runtime.sidecar_status(MediaSidecar::Ffmpeg),
        runtime.sidecar_status(MediaSidecar::YtDlp),
    ])
}

/// Install one media-import sidecar. Explicit: nothing downloads on its own.
#[tauri::command]
pub async fn media_import_install_sidecar<R: Runtime>(
    app: AppHandle<R>,
    runtime: State<'_, Arc<MediaImportRuntime>>,
    sidecar: MediaSidecar,
) -> Result<(), String> {
    runtime
        .install_sidecar(&app, sidecar)
        .await
        .map_err(|err| err.to_string())
}

/// Import a local audio file.
///
/// **`path` is refused unless a real OS dialog returned it.** The renderer
/// supplies a plain string here, so the string alone proves nothing; the
/// canonical form of it must be one [`crate::commands::pick_import_file`]
/// recorded. Without that check a compromised renderer could name any local
/// file and, with a cloud transcription provider configured, watch its
/// contents leave the machine. See [`crate::commands::DialogGrants`] and
/// `docs/SECURITY.md`'s Webview section.
#[tauri::command]
pub async fn media_import_audio<R: Runtime>(
    app: AppHandle<R>,
    runtime: State<'_, Arc<MediaImportRuntime>>,
    grants: State<'_, crate::commands::DialogGrants>,
    path: String,
) -> Result<MediaImportResult, String> {
    let granted = grants.authorise(&path)?;
    let runtime = Arc::clone(runtime.inner());
    tokio::spawn(async move {
        runtime
            .import_local_file(&app, &granted, MediaImportSource::Audio)
            .await
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|err| err.to_string())
}

/// Import a local video file. Only the label differs from audio import --
/// ffmpeg strips any video track either way, and the same dialog grant is
/// required.
#[tauri::command]
pub async fn media_import_video<R: Runtime>(
    app: AppHandle<R>,
    runtime: State<'_, Arc<MediaImportRuntime>>,
    grants: State<'_, crate::commands::DialogGrants>,
    path: String,
) -> Result<MediaImportResult, String> {
    let granted = grants.authorise(&path)?;
    let runtime = Arc::clone(runtime.inner());
    tokio::spawn(async move {
        runtime
            .import_local_file(&app, &granted, MediaImportSource::Video)
            .await
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|err| err.to_string())
}

struct ScratchCleanup(PathBuf);
impl Drop for ScratchCleanup {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_dir_all(&self.0) {
            if error.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!(%error, "could not clean import scratch directory");
            }
        }
    }
}
struct PreparedImport {
    transport: Transport,
    local: Option<Box<dyn kw_transcription_core::local::LocalTranscriber>>,
}
impl PreparedImport {
    fn identity(&self) -> (String, Option<String>) {
        match &self.transport {
            Transport::Local => (
                "local".into(),
                self.local.as_ref().and_then(|e| e.model_name()),
            ),
            Transport::HttpBatch {
                provider, model, ..
            } => (provider.clone(), Some(model.clone())),
            Transport::ProviderClient {
                provider, model, ..
            } => (format!("{provider:?}").to_lowercase(), model.clone()),
        }
    }
}
#[tauri::command]
pub fn media_import_snapshot(
    runtime: State<'_, Arc<MediaImportRuntime>>,
) -> Option<crate::media_job::ImportSnapshot> {
    runtime.jobs.snapshot()
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

    /// A runtime whose sidecar directory is empty and belongs to this test.
    ///
    /// Returned with the `TempDir` alive, because dropping it deletes the
    /// directory and every assertion about "nothing is installed" would then be
    /// true for the wrong reason.
    async fn runtime() -> (MediaImportRuntime, tempfile::TempDir) {
        let root = tempfile::tempdir().unwrap();
        let rt = MediaImportRuntime::rooted(
            NetworkPolicy::new().unwrap(),
            Arc::new(kw_security::MemoryCredentialStore::new()),
            Arc::new(RwLock::new(None)),
            Arc::new(RwLock::new(None)),
            // No sherpa model selected: these tests are about import, not about
            // which local engine an import lands on.
            Arc::new(RwLock::new(None)),
            // Nor about reviving one. No port is recorded above, so nothing
            // here should ever ask.
            Arc::new(crate::local_model::RefusesToRevive),
            Db::open_in_memory().await.unwrap(),
            root.path().to_path_buf(),
        )
        .unwrap();
        (rt, root)
    }

    #[cfg(feature = "tauri-test")]
    #[tokio::test]
    async fn failed_import_exposes_error_and_releases_app_wide_slot() {
        let (runtime, root) = runtime().await;
        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();
        let missing = root.path().join("missing.wav");
        let lease = runtime.jobs.begin().unwrap();
        assert!(runtime
            .import_local_file(app.handle(), &missing, MediaImportSource::Audio)
            .await
            .unwrap_err()
            .to_string()
            .contains("Another media import"));
        drop(lease);
        let first_id = runtime.jobs.snapshot().unwrap().operation_id;
        assert!(runtime
            .import_local_file(app.handle(), &missing, MediaImportSource::Audio)
            .await
            .is_err());
        let failed = runtime.jobs.snapshot().unwrap();
        assert_ne!(failed.operation_id, first_id);
        assert_eq!(failed.stage, crate::media_job::ImportStage::Failed);
        assert!(failed.error.is_some());
        assert!(runtime.jobs.begin().is_ok());
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
    }

    /* ------------------------------------------------------- url validation */

    #[test]
    fn an_ordinary_https_url_is_accepted() {
        assert!(validate_import_url("https://cdn.example.com/clip.mp4").is_ok());
    }

    #[test]
    fn plaintext_is_refused() {
        let err = validate_import_url("http://example.com/clip.mp4").unwrap_err();
        assert!(matches!(err, MediaImportError::UrlRefused { .. }));
    }

    #[test]
    fn a_blocked_vendor_host_is_refused() {
        // domain-scanner:allow-next-line a negative test must name the host it asserts is blocked
        let err = validate_import_url("https://api.openwhispr.com/x.mp4").unwrap_err();
        match err {
            MediaImportError::UrlRefused { reason } => assert!(reason.contains("blocked")),
            other => panic!("{other}"),
        }
    }

    #[test]
    fn a_private_or_link_local_host_is_refused_even_over_https() {
        // The gap this function closes beyond `kw_network_policy::validate`:
        // that function alone permits any host over https, private or not.
        for url in [
            "https://169.254.169.254/latest/meta-data/",
            "https://127.0.0.1/x.mp4",
            "https://192.168.1.5/video.mp4",
            "https://localhost/x.mp4",
            "https://internal-box/x.mp4",
        ] {
            let err = validate_import_url(url).unwrap_err();
            assert!(
                matches!(err, MediaImportError::UrlRefused { .. }),
                "{url} should be refused"
            );
        }
    }

    #[test]
    fn an_ordinary_public_host_is_not_mistaken_for_a_private_one() {
        for url in [
            "https://www.youtube.com/watch?v=abc",
            "https://cdn.example.com/a/b/c.mp4",
        ] {
            assert!(validate_import_url(url).is_ok(), "{url} should be accepted");
        }
    }

    #[test]
    fn no_refusal_message_contains_a_query_string() {
        // The reason the source is reported as a host, never a full url: a
        // query string can carry a token.
        let err = validate_import_url("https://192.168.1.5/x?token=secret-value").unwrap_err();
        assert!(!err.to_string().contains("secret-value"));
    }

    /* --------------------------------------------------------------- ffmpeg */

    #[test]
    fn the_ffmpeg_conversion_targets_the_shared_sample_rate() {
        let args = ffmpeg_convert_args(Path::new("in.mp4"), Path::new("out.wav"));
        assert!(args.contains(&kw_audio_core::wav::TARGET_SAMPLE_RATE.to_string()));
        assert!(args.contains(&"-vn".to_owned()), "video is always dropped");
        assert!(args.contains(&"1".to_owned()), "mono");
        assert!(args.iter().any(|a| a == "in.mp4"));
        assert!(args.iter().any(|a| a == "out.wav"));
    }

    /* --------------------------------------------------------------- yt-dlp */

    #[test]
    fn the_ytdlp_arguments_carry_the_url_as_one_argv_entry() {
        // No shell is involved, so nothing in the url needs escaping -- this
        // is the property that makes that true: the url is the last element,
        // untouched.
        let args = ytdlp_download_args(
            "https://example.com/x?y=1&z=2",
            Path::new("/tmp/scratch/downloaded.%(ext)s"),
        );
        assert_eq!(
            args.last().map(String::as_str),
            Some("https://example.com/x?y=1&z=2")
        );
        assert!(args.contains(&"--no-playlist".to_owned()));
        assert!(args.contains(&"--max-filesize".to_owned()));
        assert!(args.contains(&YT_DLP_MAX_FILESIZE.to_owned()));
    }

    /* ------------------------------------------------------------ wav bytes */

    #[test]
    fn duration_is_computed_from_the_converted_wav_length() {
        let one_second = WAV_HEADER_LEN + (kw_audio_core::wav::TARGET_SAMPLE_RATE as usize) * 2;
        assert_eq!(wav_duration_ms(one_second), 1000);
        assert_eq!(wav_duration_ms(WAV_HEADER_LEN), 0);
        assert_eq!(wav_duration_ms(0), 0, "a header-less length must not panic");
    }

    #[test]
    fn tail_bytes_keeps_only_the_end() {
        let data = b"0123456789";
        assert_eq!(tail_bytes(data, 4), b"6789");
        assert_eq!(
            tail_bytes(data, 100),
            data,
            "shorter than the cap is untouched"
        );
        assert_eq!(tail_bytes(data, 0), b"");
    }

    /* --------------------------------------------------- picking a download */

    #[test]
    fn exactly_one_matching_file_is_found() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("downloaded.webm"), b"x").unwrap();
        std::fs::write(dir.path().join("unrelated.txt"), b"y").unwrap();

        let found = pick_downloaded_file(dir.path(), "downloaded").unwrap();
        assert_eq!(
            found.file_name().and_then(|n| n.to_str()),
            Some("downloaded.webm")
        );
    }

    #[test]
    fn no_matching_file_is_reported_rather_than_guessed_at() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("unrelated.txt"), b"y").unwrap();

        let err = pick_downloaded_file(dir.path(), "downloaded").unwrap_err();
        assert!(matches!(err, MediaImportError::Fetch { .. }));
    }

    #[test]
    fn an_in_progress_download_is_never_mistaken_for_a_finished_one() {
        // A partially fetched file must not be presented as a complete
        // import, even if yt-dlp's exit code were somehow misleading.
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("downloaded.mp4.part"), b"partial").unwrap();
        std::fs::write(dir.path().join("downloaded.f140.m4a.ytdl"), b"metadata").unwrap();

        let err = pick_downloaded_file(dir.path(), "downloaded").unwrap_err();
        assert!(matches!(err, MediaImportError::Fetch { .. }));
    }

    #[test]
    fn more_than_one_matching_file_is_reported_rather_than_the_first_one_chosen() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("downloaded.f251.webm"), b"x").unwrap();
        std::fs::write(dir.path().join("downloaded.f140.m4a"), b"y").unwrap();

        let err = pick_downloaded_file(dir.path(), "downloaded").unwrap_err();
        match err {
            MediaImportError::Fetch { detail } => assert!(detail.contains('2')),
            other => panic!("{other}"),
        }
    }

    /* ------------------------------------------------------- sidecar lookup */

    #[tokio::test]
    async fn ffmpeg_and_yt_dlp_are_both_in_the_shipped_catalogue() {
        // If Task 10's pins ever moved or were renamed, this fails here rather
        // than as a confusing "not installed" from every import.
        let (rt, _root) = runtime().await;
        for sidecar in [MediaSidecar::Ffmpeg, MediaSidecar::YtDlp] {
            let name = sidecar.catalog_name();
            match rt.catalog.resolve(name) {
                Ok(_) | Err(kw_sidecar::SidecarError::NotPublished { .. }) => {}
                Err(other) => panic!("{name}: {other}"),
            }
        }
    }

    #[tokio::test]
    async fn a_sidecar_nobody_downloaded_reports_not_installed_with_an_explanation() {
        // The honesty requirement this exists for: a missing sidecar must
        // read as "not installed, here is how to fix it", not as a raw
        // "could not start process" error.
        let (rt, _root) = runtime().await;
        match rt.locate_executable(MediaSidecar::Ffmpeg) {
            Err(MediaImportError::SidecarNotInstalled { explain }) => {
                assert!(explain.to_lowercase().contains("ffmpeg") || explain.contains("Looked in"));
            }
            // A platform ffmpeg is not published for at all is a different,
            // equally honest outcome.
            Err(MediaImportError::SidecarUnavailable { .. }) => {}
            other => panic!("expected an honest refusal, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn sidecar_status_never_claims_an_uninstalled_sidecar_is_installed() {
        let (rt, _root) = runtime().await;
        for sidecar in [MediaSidecar::Ffmpeg, MediaSidecar::YtDlp] {
            let status = rt.sidecar_status(sidecar);
            assert!(!status.installed, "nothing was downloaded in this test");
        }
    }

    /* ------------------------------------------------------ local file guard */
    //
    // `check_importable_file` and `DialogGrants::authorise` are the pure
    // extractions of the two decisions around an import. The IPC test below
    // additionally runs the exported commands through a mock Tauri webview.
    //
    // The two are deliberately separate questions. `authorise` answers "may
    // this path be read at all", and is the guard a compromised renderer meets;
    // `check_importable_file` answers "is what it names still a file", and only
    // ever sees a path that already passed the first.

    use crate::commands::DialogGrants;

    #[cfg(feature = "tauri-test")]
    fn invoke_import(
        webview: &tauri::WebviewWindow<tauri::test::MockRuntime>,
        command: &str,
        body: serde_json::Value,
    ) -> Result<serde_json::Value, serde_json::Value> {
        tauri::test::get_ipc_response(
            webview,
            tauri::webview::InvokeRequest {
                cmd: command.to_owned(),
                callback: tauri::ipc::CallbackFn(0),
                error: tauri::ipc::CallbackFn(1),
                url: "http://tauri.localhost".parse().unwrap(),
                body: tauri::ipc::InvokeBody::Json(body),
                headers: Default::default(),
                invoke_key: tauri::test::INVOKE_KEY.to_owned(),
            },
        )
        .map(|response| response.deserialize::<serde_json::Value>().unwrap())
    }

    /// The JSON body for a command, with `path` at the dotted position
    /// `PATH_TAKING_COMMANDS` records for it.
    ///
    /// Built rather than written out because the shapes differ:
    /// `import_openwhispr` wraps the path in an `ImportRequest`, the other two
    /// take the string directly. Writing them out is what made the old loop a
    /// second copy of the list.
    #[cfg(feature = "tauri-test")]
    fn body_with_path(shape: &str, path: &str) -> serde_json::Value {
        let mut value = serde_json::Value::String(path.to_owned());
        for key in shape.split('.').rev() {
            let mut map = serde_json::Map::new();
            map.insert(key.to_owned(), value);
            value = serde_json::Value::Object(map);
        }
        value
    }

    /// A file that exists, so `canonicalize` has something to resolve.
    fn touch(dir: &std::path::Path, name: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, b"not really audio, just a file").unwrap();
        path
    }

    #[test]
    fn a_directory_is_not_an_importable_file() {
        let dir = tempfile::tempdir().unwrap();
        let err = check_importable_file(dir.path()).unwrap_err();
        assert!(matches!(err, MediaImportError::Io { .. }));
    }

    #[test]
    fn a_path_that_does_not_exist_is_not_an_importable_file() {
        let err = check_importable_file(std::path::Path::new("Z:/definitely/not/a/real/path.mp3"))
            .unwrap_err();
        assert!(matches!(err, MediaImportError::Io { .. }));
    }

    #[test]
    fn an_ordinary_file_is_importable_only_when_the_dialog_returned_it() {
        let dir = tempfile::tempdir().unwrap();
        let file = touch(dir.path(), "clip.mp3");
        let grants = DialogGrants::default();

        // Before the dialog: existing, readable, ordinary -- and refused. This
        // test used to assert the first three alone, which made it a false
        // negative once the grant became the thing that decides.
        assert!(grants.authorise(file.to_str().unwrap()).is_err());

        let granted = grants.grant(&file).unwrap();
        let authorised = grants.authorise(file.to_str().unwrap()).unwrap();
        assert_eq!(authorised, granted);
        assert_eq!(check_importable_file(&authorised).unwrap(), granted);
    }

    #[test]
    fn a_path_the_dialog_never_returned_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let picked = touch(dir.path(), "picked.mp3");
        let never_picked = touch(dir.path(), "never-picked.mp3");

        let grants = DialogGrants::default();
        grants.grant(&picked).unwrap();

        // A sibling in the very directory the user did pick from is still not
        // the file they picked.
        assert!(grants.authorise(never_picked.to_str().unwrap()).is_err());
        assert!(grants.authorise(picked.to_str().unwrap()).is_ok());
    }

    #[test]
    fn a_granted_path_survives_two_imports() {
        // Retry-after-failure exists today: `NotesUpload.tsx`'s `catch` leaves
        // the picked path in state, so the same file can be tried again any
        // number of times after a failed conversion. A single-use grant would
        // break that, which is why the grant lasts the session.
        let dir = tempfile::tempdir().unwrap();
        let file = touch(dir.path(), "retry.mp3");

        let grants = DialogGrants::default();
        grants.grant(&file).unwrap();

        let first = grants.authorise(file.to_str().unwrap()).unwrap();
        let second = grants.authorise(file.to_str().unwrap()).unwrap();
        assert_eq!(first, second);
    }

    #[cfg(feature = "tauri-test")]
    #[tokio::test]
    async fn every_import_command_enforces_the_dialog_grant_at_the_ipc_boundary() {
        let dir = tempfile::tempdir().unwrap();
        let never_picked = touch(dir.path(), "credentials");
        let retry = touch(dir.path(), "retry.mp3");

        let grants = DialogGrants::default();
        grants.grant(&retry).unwrap();
        let (runtime, _sidecar_root) = runtime().await;

        let app = tauri::test::mock_builder()
            .manage(crate::state::AppState::in_memory().await.unwrap())
            .manage(std::sync::Arc::new(runtime))
            .manage(grants)
            // Still this test's own list, and it has to be: `generate_handler!`
            // needs literal paths at expansion time, so it cannot be driven from
            // a const. What ties it to the application is
            // `commands::tests::every_path_taking_command_is_registered`, and what
            // ties it to the loop below is `PATH_TAKING_COMMANDS`. An entry added
            // there and not here fails on the assertion, carrying the real error.
            .invoke_handler(tauri::generate_handler![
                media_import_audio,
                media_import_video,
                crate::commands::import_openwhispr,
            ])
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();
        let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .unwrap();

        // Driven by the shared list rather than by a copy of it. A fourth
        // path-taking command is asserted by being added to
        // `PATH_TAKING_COMMANDS`, which is the whole point (KNEG-151); the array
        // is a fixed-length const, so this loop cannot silently become empty.
        let path = never_picked.display().to_string();
        for (command, shape) in crate::commands::PATH_TAKING_COMMANDS {
            let body = body_with_path(shape, &path);
            let error = invoke_import(&webview, command, body).unwrap_err();
            let message = error.as_str().unwrap();
            assert!(
                message.contains("not chosen through the import dialog"),
                "{command} did not enforce DialogGrants: {message}"
            );
        }

        // A failed conversion leaves NotesUpload's chosen path in renderer
        // state. The command must therefore accept the same dialog grant on a
        // retry; the empty sidecar root makes both attempts fail safely after
        // authorisation, without reading or uploading anything useful.
        let retry_body = serde_json::json!({ "path": retry.display().to_string() });
        let first = invoke_import(&webview, "media_import_audio", retry_body.clone()).unwrap_err();
        let second = invoke_import(&webview, "media_import_audio", retry_body).unwrap_err();
        assert_eq!(first, second);
        assert!(
            !first
                .as_str()
                .unwrap()
                .contains("not chosen through the import dialog"),
            "the grant was consumed by the first attempt: {first}"
        );
    }

    #[test]
    fn the_oldest_grant_is_evicted_past_the_bound() {
        let dir = tempfile::tempdir().unwrap();
        let grants = DialogGrants::default();

        let oldest = touch(dir.path(), "oldest.mp3");
        grants.grant(&oldest).unwrap();

        // One more than the bound allows, counting the one above.
        for n in 0..crate::commands::MAX_DIALOG_GRANTS {
            let path = touch(dir.path(), &format!("later-{n}.mp3"));
            grants.grant(&path).unwrap();
        }

        assert!(
            grants.authorise(oldest.to_str().unwrap()).is_err(),
            "the oldest grant should have been evicted"
        );
        let newest = dir.path().join(format!(
            "later-{}.mp3",
            crate::commands::MAX_DIALOG_GRANTS - 1
        ));
        assert!(grants.authorise(newest.to_str().unwrap()).is_ok());
    }

    #[test]
    fn an_ungranted_credential_file_is_refused_rather_than_read() {
        // The compromised-renderer case, in the shape
        // `an_export_filename_survives_a_hostile_title` uses for export: the
        // caller names a file it was never given and that it has no business
        // reading. Real files in a temp directory rather than `~/.aws`, so the
        // test asserts the refusal and not merely that the path is absent.
        let dir = tempfile::tempdir().unwrap();
        let credentials = touch(dir.path(), "credentials");
        let ssh_key = touch(dir.path(), "id_rsa");
        let innocuous = touch(dir.path(), "meeting.m4a");

        let grants = DialogGrants::default();
        grants.grant(&innocuous).unwrap();

        for hostile in [&credentials, &ssh_key] {
            let err = grants.authorise(hostile.to_str().unwrap()).unwrap_err();
            assert!(
                err.contains("not chosen through the import dialog"),
                "expected a refusal naming the dialog, got {err}"
            );
        }

        // Traversal does not launder an ungranted path either. This one
        // canonicalises perfectly well -- `subdir` exists, so it resolves to
        // the very `credentials` above -- and is refused on identity, because
        // the set holds resolved paths and not prefixes.
        std::fs::create_dir(dir.path().join("subdir")).unwrap();
        let traversal = dir.path().join("subdir").join("..").join("credentials");
        assert_eq!(
            std::fs::canonicalize(&traversal).unwrap(),
            std::fs::canonicalize(&credentials).unwrap(),
            "the traversal should reach the same file, or this asserts nothing"
        );
        assert!(grants.authorise(traversal.to_str().unwrap()).is_err());
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod retained_import_tests {
    use super::*;
    fn transcript(timed: bool) -> kw_transcription_core::Transcript {
        kw_transcription_core::Transcript {
            text: "hello".to_owned(),
            provider: "fixture".to_owned(),
            model: None,
            segments: if timed {
                vec![kw_transcription_core::local::TimedSegment {
                    text: "hello".to_owned(),
                    start: 0.0,
                    end: 1.0,
                }]
            } else {
                vec![]
            },
        }
    }
    fn fixture(dir: &Path) -> PathBuf {
        let path = dir.join("original möte.wav");
        std::fs::write(
            &path,
            include_bytes!(
                "../../../../crates/transcription-core/tests/fixtures/speech-16k-mono.wav"
            ),
        )
        .unwrap();
        path
    }
    #[cfg(all(windows, feature = "tauri-test"))]
    #[tokio::test]
    #[ignore = "uses installed ffmpeg/Whisper with local fixtures; opt in explicitly"]
    async fn native_audio_video_import_pipeline() {
        let dir = tempfile::tempdir().unwrap();
        let original = fixture(dir.path());
        let original_bytes = std::fs::read(&original).unwrap();
        let policy = NetworkPolicy::new().unwrap();
        let catalog = Catalog::shipped().unwrap();
        let (spec, asset) = catalog.resolve("whisper").unwrap();
        let sidecar_root = kw_storage::paths::sidecar_dir().path;
        let installation = kw_sidecar::locate(
            "whisper",
            asset,
            &SearchPath {
                bundled: None,
                downloaded: sidecar_root.clone(),
            },
            &RealFs,
        );
        let port = kw_sidecar::port::allocate(
            kw_sidecar::PortRange {
                first: 18000,
                last: 19000,
            },
            &kw_sidecar::BindProbe,
        )
        .unwrap();
        let model = kw_storage::paths::model_cache_dir()
            .path
            .join("ggml-base.bin");
        let launch = kw_sidecar::process::Launch {
            executable: installation.executable().unwrap().to_owned(),
            args: vec![
                "--model".to_owned(),
                model.display().to_string(),
                "--host".to_owned(),
                "127.0.0.1".to_owned(),
                "--port".to_owned(),
                port.to_string(),
            ],
            port,
        };
        let pids = kw_sidecar::PidStore::new(dir.path());
        let server = kw_sidecar::process::start(
            spec,
            &launch,
            &pids,
            &kw_sidecar::HttpHealth::new(policy.clone(), "/health"),
            kw_sidecar::process::StartTimings::default(),
        )
        .await
        .unwrap();
        let recording_directory = dir.path().join("recordings");
        let video = dir.path().join("video with spaces.mkv");
        let result = async move {
            let db = Db::open_in_memory().await.unwrap();
            kw_storage::settings::set(
                db.pool(),
                crate::dictation::ROUTE_SETTINGS_KEY,
                &kw_transcription_core::RouteSettings {
                    use_local_whisper: true,
                    ..Default::default()
                },
            )
            .await
            .unwrap();
            let mut runtime = MediaImportRuntime::rooted(
                policy,
                Arc::new(kw_security::MemoryCredentialStore::new()),
                Arc::new(RwLock::new(Some(port))),
                Arc::new(RwLock::new(Some("base".to_owned()))),
                Arc::new(RwLock::new(None)),
                Arc::new(crate::local_model::RefusesToRevive),
                db.clone(),
                sidecar_root,
            )
            .unwrap();
            runtime.recording_directory = recording_directory;
            let ffmpeg = runtime.locate_executable(MediaSidecar::Ffmpeg).unwrap();
            let args = vec!["-y", "-i"]
                .into_iter()
                .map(str::to_owned)
                .chain([original.display().to_string()])
                .chain(
                    [
                        "-f",
                        "lavfi",
                        "-i",
                        "color=size=64x64:rate=1",
                        "-shortest",
                        "-c:v",
                        "ffv1",
                        "-c:a",
                        "pcm_s16le",
                    ]
                    .into_iter()
                    .map(str::to_owned),
                )
                .chain([video.display().to_string()])
                .collect::<Vec<_>>();
            assert!(run_sidecar_process(&ffmpeg, &args).await.unwrap().success);
            let app = tauri::test::mock_builder()
                .build(tauri::test::mock_context(tauri::test::noop_assets()))
                .unwrap();
            for (path, kind) in [
                (&original, MediaImportSource::Audio),
                (&video, MediaImportSource::Video),
            ] {
                let result = runtime
                    .import_local_file(app.handle(), path, kind)
                    .await
                    .unwrap();
                assert!(!result.text.is_empty());
                let snapshot = runtime.jobs.snapshot().unwrap();
                assert_eq!(snapshot.stage, crate::media_job::ImportStage::Succeeded);
                assert_eq!(snapshot.result.unwrap().note_id, result.note_id);
                assert_eq!(snapshot.provider.as_deref(), Some("local"));
                let copy = runtime
                    .recording_directory
                    .join(&result.recording.storage_key);
                assert!(copy.exists());
                let request = tauri::http::Request::builder()
                    .header("Range", "bytes=0-43")
                    .body(vec![])
                    .unwrap();
                assert_eq!(
                    crate::recordings::response_for(&request, &copy).status(),
                    206
                );
                kw_notes_core::recordings::delete_with_recording(
                    db.pool(),
                    result.note_id,
                    &runtime.recording_directory,
                )
                .await
                .unwrap();
                assert!(!copy.exists());
                assert!(path.exists());
                println!(
                    "Imported {:?}: note {}, {} bytes retained; playback range and cleanup passed",
                    result.source, result.note_id, result.retained_bytes
                );
            }
            assert_eq!(std::fs::read(original).unwrap(), original_bytes);
        };
        // Stop the owned child even when the fixture assertions fail.
        let outcome = tokio::spawn(result).await;
        server.stop(&pids).await.unwrap();
        outcome.unwrap();
    }

    #[tokio::test]
    async fn imports_commit_audio_and_note_together_and_play_through_existing_protocol() {
        for timed in [true, false] {
            let dir = tempfile::tempdir().unwrap();
            let original = fixture(dir.path());
            let recordings = dir.path().join("recordings");
            let db = Db::open_in_memory().await.unwrap();
            let result = save_import(
                &db,
                transcript(timed),
                MediaImportSource::Audio,
                &original,
                &original,
                &recordings,
            )
            .await
            .unwrap();
            assert_eq!(
                result.recording.original_filename.as_deref(),
                Some("original möte.wav")
            );
            assert_eq!(
                result.retained_bytes,
                std::fs::metadata(&original).unwrap().len()
            );
            let note = kw_notes_core::notes::get(db.pool(), result.note_id)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(note.note_type, "import");
            let segments: serde_json::Value =
                serde_json::from_str(note.transcript.as_deref().unwrap()).unwrap();
            assert_eq!(segments.as_array().unwrap().len(), 1);
            assert_eq!(result.segments.is_empty(), !timed);
            let copy = recordings.join(result.recording.storage_key);
            let request = tauri::http::Request::builder()
                .header("Range", "bytes=0-43")
                .body(vec![])
                .unwrap();
            let response = crate::recordings::response_for(&request, &copy);
            assert_eq!(response.status(), 206);
            assert_eq!(response.body().len(), 44);
            kw_notes_core::recordings::delete_with_recording(
                db.pool(),
                result.note_id,
                &recordings,
            )
            .await
            .unwrap();
            assert!(!copy.exists());
            assert!(original.exists());
        }
    }
    #[tokio::test]
    async fn database_failure_rolls_back_note_and_removes_copy() {
        let dir = tempfile::tempdir().unwrap();
        let original = fixture(dir.path());
        let recordings = dir.path().join("recordings");
        let db = Db::open_in_memory().await.unwrap();
        sqlx::query("CREATE TRIGGER refuse_import BEFORE INSERT ON note_recordings BEGIN SELECT RAISE(FAIL, 'fixture failure'); END").execute(db.pool()).await.unwrap();
        assert!(save_import(
            &db,
            transcript(false),
            MediaImportSource::Video,
            &original,
            &original,
            &recordings
        )
        .await
        .is_err());
        assert_eq!(std::fs::read_dir(&recordings).unwrap().count(), 0);
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM notes")
            .fetch_one(db.pool())
            .await
            .unwrap();
        assert_eq!(count, 0);
        assert!(original.exists());
    }
}
