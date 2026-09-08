//! Speaker turns from `sherpa-onnx-offline-speaker-diarization`.
//!
//! Diarization answers *how many voices, and when* — never *who*. A label here
//! is `Speaker 1`, produced by clustering voice embeddings within one
//! recording, and it carries across nothing: the same person in two meetings is
//! `Speaker 1` in one and `Speaker 3` in the other. There is no `contacts`
//! lookup in this module and there must not be one.
//!
//! ## The same shape as `kw_transcription_core::sherpa`, for the same reason
//!
//! `config/integration-manifest.json` said for months that this needed *"the
//! `ort` crate (ONNX Runtime), a `sherpa-onnx-diarize` sidecar, and
//! `silero_vad.onnx`"*. **All three are wrong.** The pinned sherpa release
//! already ships `sherpa-onnx-offline-speaker-diarization` beside the offline
//! transcription CLI this project already drives, it links ONNX Runtime itself,
//! and the pyannote segmentation model does its own voice-activity detection —
//! so no separate VAD model is involved. The manifest entry was corrected in
//! place on 2026-09-01 rather than annotated.
//!
//! So this is a pure half — [`build_args`], [`interpret`], [`assign`] — and a
//! thin `Command` half in the desktop crate, exactly as `sherpa.rs` is.
//!
//! ## Measured on 2026-09-01, not read off a release page
//!
//! Against the pinned pair (`sherpa-onnx-pyannote-segmentation-3-0` and
//! `3dspeaker-campplus-en`) and the release's own `0-four-speakers-zh.wav`.
//!
//! **Output shape.** stdout carries one `OfflineSpeakerDiarizationConfig(...)`
//! echo, then the literal line `Started`, then one line per turn:
//!
//! ```text
//! 0.318 -- 6.865 speaker_00
//! ```
//!
//! Three fields, seconds to three decimals, separated by a bare `--`. The
//! per-cent progress, the duration and the real-time factor all go to stderr
//! and never appear here.
//!
//! **A failure and a silent room produce byte-identical stdout.** Both print
//! the config echo and nothing else. The only thing separating *"this recording
//! had no speech in it"* from *"the model file was missing"* is the exit
//! status — 0 against 127 — and, on stderr, one sentence. That is the exact
//! shape of KNEG-38, so [`interpret`] takes the status and refuses to guess
//! from the text.
//!
//! Measured failures, each ending in one usable stderr line:
//!
//! | what | last stderr line |
//! | --- | --- |
//! | segmentation model absent | `Errors in config!` |
//! | WAV absent | `Failed to read ./nosuch.wav` |
//! | WAV at 48 kHz | `Expect sample rate 16000. Given: 48000` |
//!
//! **16 kHz is a refusal, not a resample.** [`REQUIRED_SAMPLE_RATE`] is what the
//! CLI enforces. A meeting recording is already written at
//! `kw_audio_core::TARGET_SAMPLE_RATE`, which is the same number, so nothing has
//! to convert — but an imported file need not be, and the refusal is worth
//! catching before spawning a process. Stereo at 16 kHz was accepted.
//!
//! **Speaker labels are cluster indices, and they are not dense from zero.** The
//! single-speaker fixture `speech-16k-mono.wav` came back as one turn labelled
//! `speaker_01` — with no `speaker_00` anywhere. A reader that turned
//! `speaker_NN` into `Speaker NN+1` would call the only person in the room
//! "Speaker 2". [`interpret`] therefore renumbers by first appearance, and
//! [`Turn::speaker`] is that renumbering rather than sherpa's index.
//!
//! **Turns overlap, and one can sit wholly inside another.** In the
//! four-speaker recording, `11.455 -- 14.037 speaker_00` and
//! `13.025 -- 13.514 speaker_01` both hold, the second nested in the first.
//! [`assign`] scores by overlapped duration for that reason, instead of taking
//! the first turn that contains a midpoint — and then has a second rule,
//! because overlapped seconds *alone* make a nested voice unreachable for every
//! span there is.
//!
//! **`--clustering.num-clusters=4` asked for four and produced three.** It is a
//! hint to the clusterer, not a promise about how many labels come back. This
//! module never passes it: a meeting does not know its own attendance, and
//! `--clustering.cluster-threshold` is the flag for that case.
//!
//! ## The control, and what it cost to run
//!
//! The first run paired the **English** embedding model with the **Chinese**
//! reference recording and returned three speakers, one of which swallowed ten
//! seconds belonging to two other people. Re-run with the language-matched
//! `3dspeaker_speech_eres2net_base_sv_zh-cn_3dspeaker_16k.onnx`, the same audio
//! through the same segmentation model returned four, with different
//! boundaries.
//!
//! So the pipeline works and **the embedding model is language-sensitive in a
//! way the segmentation model is not.** This project pins the English CAM++
//! model only. English meetings are the measured case; a meeting in any other
//! language will diarize worse and nothing in the output will say so. That is
//! recorded here, in `docs/PLATFORM_SUPPORT.md`, and on the screen that offers
//! the feature — not left for a user to infer from bad speaker turns.
//!
//! ## What is invented rather than ported
//!
//! [`assign`] is **invented**. `docs/PORTING_MATRIX.md` names upstream's
//! `diarizationPolicy.js` (63 lines) as the thing to port, and that file is not
//! in this repository — only its line count is. Writing "ported" over a rule
//! reasoned out from scratch would be the kind of claim this project's own
//! docs exist to prevent. The rule is stated in [`assign`]'s own comment and
//! stands on its tests, not on upstream's authority.

use std::path::{Path, PathBuf};

/// The only sample rate the CLI accepts. Measured: 48 kHz is refused outright
/// rather than resampled.
pub const REQUIRED_SAMPLE_RATE: u32 = 16_000;

/// The float build of the pyannote segmentation model.
///
/// The extracted archive holds **two** candidates, `model.onnx` and
/// `model.int8.onnx`, so discovery cannot be "the one `.onnx` file" the way
/// `sherpa::discover_transducer_files` is. This build names the float one
/// because that is the one every measurement above was taken through; the int8
/// build may well be fine and has not been run, which is a different sentence.
pub const SEGMENTATION_MODEL_FILE: &str = "model.onnx";

/// The line the CLI prints once its models have loaded and the run is under
/// way.
///
/// Worth naming because its *absence* is the signal: a configuration failure
/// prints the config echo and dies before this, so `Started` present with a
/// clean exit is a run that reached the audio.
const STARTED: &str = "Started";

/// The prefix of the CLI's own configuration echo, which is the first line of
/// stdout on every run including the failures.
const CONFIG_ECHO: &str = "OfflineSpeakerDiarizationConfig(";

/// The two model files a run needs, resolved to real paths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Models {
    /// `model.onnx` inside the extracted pyannote segmentation directory.
    pub segmentation: PathBuf,
    /// The CAM++ speaker embedding model, a bare `.onnx` file rather than an
    /// archive.
    pub embedding: PathBuf,
}

/// Why a pair of model paths could not be used.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LayoutError {
    #[error(
        "{dir} holds no {file}. Either the segmentation model was not fully extracted, or its \
         layout is not the one this build was measured against."
    )]
    MissingSegmentation { dir: PathBuf, file: &'static str },

    #[error(
        "{path} is not there. The speaker embedding model is a single .onnx file, downloaded \
         separately from the segmentation model."
    )]
    MissingEmbedding { path: PathBuf },
}

impl Models {
    /// Resolve the two paths, checking both exist before anything is spawned.
    ///
    /// # Errors
    ///
    /// [`LayoutError`] naming which of the two is missing. Both are checked, and
    /// the segmentation one first, so a user with neither is told about the
    /// larger download rather than being sent round twice.
    pub fn locate(segmentation_dir: &Path, embedding: &Path) -> Result<Self, LayoutError> {
        let model = segmentation_dir.join(SEGMENTATION_MODEL_FILE);
        if !model.is_file() {
            return Err(LayoutError::MissingSegmentation {
                dir: segmentation_dir.to_path_buf(),
                file: SEGMENTATION_MODEL_FILE,
            });
        }
        if !embedding.is_file() {
            return Err(LayoutError::MissingEmbedding {
                path: embedding.to_path_buf(),
            });
        }
        Ok(Self {
            segmentation: model,
            embedding: embedding.to_path_buf(),
        })
    }
}

/// The three clustering knobs, at the CLI's own defaults.
///
/// Its defaults rather than ones chosen here: they are what every measurement
/// above was taken through, and inventing different numbers would mean the
/// recorded output no longer describes what this code produces. `num_clusters`
/// is deliberately absent — see the module doc.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tuning {
    /// Smaller finds more speakers. The CLI's default is 0.5.
    pub cluster_threshold: f32,
    /// Turns shorter than this are discarded, in seconds.
    pub min_duration_on: f32,
    /// A gap shorter than this between two turns of one speaker merges them.
    pub min_duration_off: f32,
}

impl Default for Tuning {
    fn default() -> Self {
        Self {
            cluster_threshold: 0.5,
            min_duration_on: 0.3,
            min_duration_off: 0.5,
        }
    }
}

/// The command line, in the order the CLI's own usage block gives it.
///
/// Pure, and the WAV goes last as a positional argument — the CLI takes one or
/// more, and this build passes exactly one.
#[must_use]
pub fn build_args(models: &Models, wav: &Path, tuning: &Tuning) -> Vec<String> {
    vec![
        format!(
            "--segmentation.pyannote-model={}",
            models.segmentation.display()
        ),
        format!("--embedding.model={}", models.embedding.display()),
        format!(
            "--clustering.cluster-threshold={}",
            tuning.cluster_threshold
        ),
        format!("--min-duration-on={}", tuning.min_duration_on),
        format!("--min-duration-off={}", tuning.min_duration_off),
        wav.display().to_string(),
    ]
}

/// One stretch of one voice.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Turn {
    /// Seconds from the start of the recording.
    pub start_seconds: f64,
    pub end_seconds: f64,
    /// Renumbered by first appearance, so the first voice heard is always 0.
    ///
    /// **Not sherpa's `speaker_NN` index**, which is a cluster id and is not
    /// dense from zero — measured, and the reason this field exists at all.
    pub speaker: u32,
}

impl Turn {
    /// What to call this voice in a sentence a person reads.
    ///
    /// One-based, because "Speaker 0" reads as a bug to everybody who is not a
    /// programmer.
    #[must_use]
    pub fn label(&self) -> String {
        format!("Speaker {}", self.speaker.saturating_add(1))
    }

    /// How many seconds this turn shares with `from..to`.
    ///
    /// Zero when they do not touch. Used by [`assign`] rather than a
    /// containment test, because turns overlap.
    #[must_use]
    pub fn overlap(&self, from: f64, to: f64) -> f64 {
        let start = self.start_seconds.max(from);
        let end = self.end_seconds.min(to);
        (end - start).max(0.0)
    }
}

/// Why a diarization run produced no turns.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum DiarizeError {
    /// The process exited badly. `detail` is the CLI's own last sentence.
    #[error("the speaker diarization step failed: {detail}")]
    Failed { detail: String },

    /// A stdout line was neither the preamble nor a turn.
    ///
    /// Refused rather than skipped: a line this build cannot read is a line
    /// whose turn is silently missing from the transcript, and a quietly
    /// shorter answer is worse than a loud one.
    #[error("the speaker diarization step printed a line this build cannot read: {line:?}")]
    Unreadable { line: String },
}

/// Read a finished run.
///
/// Takes the exit status because **stdout alone cannot distinguish a failure
/// from a recording with no speech in it** — measured, and stated in the module
/// doc. `stderr` is read only to quote the CLI's own last sentence back to the
/// user on a failure.
///
/// # Errors
///
/// [`DiarizeError::Failed`] if the process exited badly;
/// [`DiarizeError::Unreadable`] for a stdout line that is neither the config
/// echo, `Started`, blank, nor a turn.
pub fn interpret(
    stdout: &str,
    stderr: &str,
    exited_cleanly: bool,
) -> Result<Vec<Turn>, DiarizeError> {
    if !exited_cleanly {
        return Err(DiarizeError::Failed {
            detail: last_sentence(stderr),
        });
    }

    let mut raw: Vec<(f64, f64, u32)> = Vec::new();
    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() || line == STARTED || line.starts_with(CONFIG_ECHO) {
            continue;
        }
        let parsed = parse_turn(line).ok_or_else(|| DiarizeError::Unreadable {
            line: line.to_owned(),
        })?;
        raw.push(parsed);
    }

    // Renumber by first appearance. sherpa's own indices are cluster ids: the
    // single-speaker fixture came back as `speaker_01` with no `speaker_00`,
    // which is why this is not a cast.
    let mut seen: Vec<u32> = Vec::new();
    let mut turns = Vec::with_capacity(raw.len());
    for (start_seconds, end_seconds, cluster) in raw {
        let speaker = match seen.iter().position(|c| *c == cluster) {
            Some(at) => at,
            None => {
                seen.push(cluster);
                seen.len().saturating_sub(1)
            }
        };
        turns.push(Turn {
            start_seconds,
            end_seconds,
            speaker: u32::try_from(speaker).unwrap_or(u32::MAX),
        });
    }
    Ok(turns)
}

/// `0.318 -- 6.865 speaker_00`, or nothing.
fn parse_turn(line: &str) -> Option<(f64, f64, u32)> {
    let (span, speaker) = line.rsplit_once(' ')?;
    let cluster = speaker.strip_prefix("speaker_")?.parse::<u32>().ok()?;
    let (start, end) = span.split_once(" -- ")?;
    let start = start.trim().parse::<f64>().ok()?;
    let end = end.trim().parse::<f64>().ok()?;
    if !start.is_finite() || !end.is_finite() || end < start {
        return None;
    }
    Some((start, end, cluster))
}

/// The CLI's own last word, for quoting back.
///
/// The first stderr line is an echo of the whole command line — model paths
/// included — and the progress lines are noise, so this takes the last
/// non-empty line rather than the whole stream.
fn last_sentence(stderr: &str) -> String {
    stderr
        .lines()
        .map(str::trim)
        .rfind(|line| !line.is_empty() && !line.starts_with("progress "))
        .unwrap_or("it exited without saying why")
        .to_owned()
}

/// Give each transcript span the voice that spoke most of it.
///
/// **Invented, not ported** — see the module doc. The rule, in full, and in the
/// order it is applied:
///
/// 1. Score every turn by how many seconds it shares with the span, and take
///    the highest. Overlapping speech is real (measured), so a containment test
///    or a midpoint lookup would hand a whole sentence to whoever interrupted
///    it.
/// 2. **On an equal overlap, the turn with more of *itself* inside the span
///    wins.** Without this the first rule alone can never pick an interrupter:
///    in the recorded four-speaker run, `13.025 -- 13.514 speaker_01` sits
///    entirely inside `11.455 -- 14.037 speaker_00`, so for *every* span the
///    outer turn overlaps at least as much as the inner one and the shorter
///    voice is unreachable by construction. That is not a tie worth breaking
///    arbitrarily — a span that is 92% of one turn and 17% of another belongs
///    to the first. Found by a test expecting the interrupter and getting the
///    other one; the rule was wrong, not the expectation.
/// 3. Still tied, the earlier turn wins — so the answer does not depend on the
///    order two indistinguishable turns happen to arrive in.
///
/// **No overlap at all is [`None`], never a guess.** A transcript segment that
/// diarization did not cover is a segment nobody can attribute, and labelling
/// it with the nearest voice would invent an attribution that reads exactly
/// like a measured one.
///
/// Returns one entry per span, in the same order.
#[must_use]
pub fn assign(turns: &[Turn], spans: &[(f64, f64)]) -> Vec<Option<u32>> {
    spans
        .iter()
        .map(|(from, to)| {
            let mut best: Option<(f64, f64, u32)> = None;
            for turn in turns {
                let shared = turn.overlap(*from, *to);
                if shared <= 0.0 {
                    continue;
                }
                // How much of the turn the span accounts for. A zero-length
                // turn cannot be more or less covered than any other, so it
                // scores zero here and falls through to the ordering tie-break
                // rather than dividing by nothing.
                let length = turn.end_seconds - turn.start_seconds;
                let covered = if length > 0.0 { shared / length } else { 0.0 };
                let better = match best {
                    None => true,
                    Some((most, most_covered, _)) => {
                        shared > most || (shared == most && covered > most_covered)
                    }
                };
                if better {
                    best = Some((shared, covered, turn.speaker));
                }
            }
            best.map(|(_, _, speaker)| speaker)
        })
        .collect()
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_cmp,
    // `0.318` is where the first voice starts in the recorded run, and it is
    // within clippy's tolerance of 1/pi. Rounding it to please the lint would
    // mean the fixture no longer holds what the CLI printed, which is the one
    // property these fixtures have.
    clippy::approx_constant
)]
mod tests {
    use super::*;

    /// The CLI's configuration echo, verbatim from the 2026-09-01 runs.
    ///
    /// Carried in full rather than shortened because skipping it is a thing
    /// [`interpret`] has to do, and a fixture that had been tidied would test a
    /// line no CLI ever prints.
    const CONFIG_LINE: &str = r#"OfflineSpeakerDiarizationConfig(segmentation=OfflineSpeakerSegmentationModelConfig(pyannote=OfflineSpeakerSegmentationPyannoteModelConfig(model="./sherpa-onnx-pyannote-segmentation-3-0/model.onnx"), num_threads=1, debug=False, provider="cpu"), embedding=SpeakerEmbeddingExtractorConfig(model="../campplus.onnx", num_threads=1, debug=False, provider="cpu"), clustering=FastClusteringConfig(num_clusters=4, threshold=0.5), min_duration_on=0.3, min_duration_off=0.5)"#;

    /// The pinned English CAM++ model over the release's Chinese four-speaker
    /// recording — which is to say, the mismatched pairing. Ten turns, three
    /// voices, and one of them holding ten seconds.
    fn recorded_english_model() -> String {
        [
            CONFIG_LINE,
            "Started",
            "0.318 -- 10.747 speaker_00",
            "11.455 -- 14.037 speaker_00",
            "13.025 -- 13.514 speaker_01",
            "14.037 -- 17.041 speaker_02",
            "22.137 -- 24.837 speaker_00",
            "27.638 -- 29.478 speaker_00",
            "30.001 -- 31.553 speaker_00",
            "33.680 -- 37.932 speaker_00",
            "48.040 -- 50.470 speaker_00",
            "52.546 -- 54.605 speaker_00",
        ]
        .join("\n")
    }

    /// The control: the same audio and the same segmentation model, with the
    /// language-matched Chinese embedding model. Four voices, and different
    /// boundaries.
    fn recorded_control() -> String {
        [
            CONFIG_LINE,
            "Started",
            "0.318 -- 6.865 speaker_00",
            "7.017 -- 10.747 speaker_01",
            "11.455 -- 13.632 speaker_01",
            "13.750 -- 17.041 speaker_02",
            "22.137 -- 24.837 speaker_00",
            "27.638 -- 29.478 speaker_03",
            "30.001 -- 31.553 speaker_03",
            "33.680 -- 37.932 speaker_03",
            "48.040 -- 50.470 speaker_02",
            "52.529 -- 54.605 speaker_00",
        ]
        .join("\n")
    }

    /// `crates/transcription-core/tests/fixtures/speech-16k-mono.wav` — one
    /// voice, and the reason [`Turn::speaker`] is a renumbering.
    fn recorded_one_voice() -> String {
        [CONFIG_LINE, "Started", "0.031 -- 5.549 speaker_01"].join("\n")
    }

    /// Three seconds of digital silence: a clean exit and no turns at all.
    fn recorded_silence() -> String {
        [CONFIG_LINE, "Started"].join("\n")
    }

    #[test]
    fn the_recorded_run_reads_as_ten_turns_in_three_voices() {
        let turns = interpret(&recorded_english_model(), "", true).unwrap();
        assert_eq!(turns.len(), 10);
        assert_eq!(turns[0].start_seconds, 0.318);
        assert_eq!(turns[0].end_seconds, 10.747);

        let mut voices: Vec<u32> = turns.iter().map(|t| t.speaker).collect();
        voices.sort_unstable();
        voices.dedup();
        assert_eq!(voices, vec![0, 1, 2]);
    }

    #[test]
    fn the_control_run_reads_as_four_voices() {
        // The whole point of keeping both fixtures: the pipeline is capable of
        // four, and the three above is the English model on Chinese audio
        // rather than a limit of this parser.
        let turns = interpret(&recorded_control(), "", true).unwrap();
        let mut voices: Vec<u32> = turns.iter().map(|t| t.speaker).collect();
        voices.sort_unstable();
        voices.dedup();
        assert_eq!(voices, vec![0, 1, 2, 3]);
    }

    #[test]
    fn the_only_voice_in_the_room_is_called_speaker_1() {
        // sherpa labelled it `speaker_01`. Reading that index straight through
        // would name the only person present "Speaker 2", which is the defect
        // this renumbering exists to prevent -- and the mutation that catches
        // it is deleting the renumbering loop.
        let turns = interpret(&recorded_one_voice(), "", true).unwrap();
        assert_eq!(turns.len(), 1);
        assert_eq!(turns[0].speaker, 0);
        assert_eq!(turns[0].label(), "Speaker 1");
    }

    #[test]
    fn renumbering_is_by_first_appearance_and_not_by_sorting() {
        // A recording whose first voice carries the higher cluster id. Sorting
        // the ids and using their rank would call the first voice heard
        // "Speaker 2"; ordering by appearance is what makes it "Speaker 1".
        let out = [
            CONFIG_LINE,
            "Started",
            "0.000 -- 1.000 speaker_07",
            "1.000 -- 2.000 speaker_02",
            "2.000 -- 3.000 speaker_07",
        ]
        .join("\n");
        let turns = interpret(&out, "", true).unwrap();
        assert_eq!(
            turns.iter().map(|t| t.speaker).collect::<Vec<_>>(),
            vec![0, 1, 0]
        );
    }

    #[test]
    fn a_silent_recording_is_no_turns_rather_than_a_failure() {
        assert_eq!(
            interpret(&recorded_silence(), "", true).unwrap(),
            Vec::new()
        );
    }

    #[test]
    fn stdout_alone_cannot_tell_a_failure_from_a_silent_room() {
        // The measured fact this module is built around, asserted rather than
        // only written down: the *same bytes* mean "nobody spoke" on a clean
        // exit and "the model was missing" on a bad one. If `interpret` ever
        // stops reading the status, these two lines return the same thing and
        // a failed diarization becomes a meeting where nobody spoke.
        let stdout = recorded_silence();
        assert!(interpret(&stdout, "", true).is_ok());
        assert!(interpret(&stdout, "Errors in config!", false).is_err());
    }

    #[test]
    fn a_failure_quotes_the_clis_own_last_sentence() {
        // All three measured on 2026-09-01, each the last line of a real
        // stderr.
        for (stderr, wanted) in [
            ("Errors in config!", "Errors in config!"),
            (
                "some/path.cc:ReadWave:355 ...\nFailed to read ./nosuch.wav",
                "Failed to read ./nosuch.wav",
            ),
            (
                "progress 50.00%\nprogress 100.00%\nExpect sample rate 16000. Given: 48000",
                "Expect sample rate 16000. Given: 48000",
            ),
        ] {
            match interpret("", stderr, false) {
                Err(DiarizeError::Failed { detail }) => assert_eq!(detail, wanted),
                other => panic!("{other:?}"),
            }
        }
    }

    #[test]
    fn a_failure_that_said_nothing_still_says_something() {
        match interpret("", "   \n\n", false) {
            Err(DiarizeError::Failed { detail }) => assert!(!detail.is_empty()),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn an_unreadable_line_is_refused_rather_than_skipped() {
        // Skipping it would drop one speaker turn out of the transcript with
        // nothing anywhere saying so.
        let out = [
            CONFIG_LINE,
            "Started",
            "0.318 -- 6.865 speaker_00",
            "6.900 ~~ 7.100 speaker_01",
        ]
        .join("\n");
        match interpret(&out, "", true) {
            Err(DiarizeError::Unreadable { line }) => assert_eq!(line, "6.900 ~~ 7.100 speaker_01"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_backwards_turn_is_unreadable() {
        let out = [CONFIG_LINE, "Started", "6.865 -- 0.318 speaker_00"].join("\n");
        assert!(matches!(
            interpret(&out, "", true),
            Err(DiarizeError::Unreadable { .. })
        ));
    }

    #[test]
    fn overlapping_turns_go_to_whoever_spoke_most_of_the_span() {
        // The real overlap out of the four-speaker recording: speaker_00 holds
        // 11.455--14.037 and speaker_01 interrupts for half a second inside it.
        // A midpoint lookup at 13.2 would hand the whole utterance to the
        // interrupter.
        let turns = interpret(&recorded_english_model(), "", true).unwrap();
        assert_eq!(assign(&turns, &[(11.5, 14.0)]), vec![Some(0)]);
    }

    #[test]
    fn a_nested_interrupter_is_reachable_at_all() {
        // `13.025 -- 13.514 speaker_01` lies wholly inside
        // `11.455 -- 14.037 speaker_00`, both real. On overlapped seconds alone
        // the outer turn ties or wins for *every* span there is, so the
        // interrupter could never be named -- a whole voice unreachable by
        // construction, which is the failure that produced `assign`'s second
        // rule. This span is 0.45 s of each, and 92% of the inner turn against
        // 17% of the outer.
        let turns = interpret(&recorded_english_model(), "", true).unwrap();
        assert_eq!(assign(&turns, &[(13.05, 13.50)]), vec![Some(1)]);
    }

    /// A span no voice covered comes back `None` rather than a nearest guess.
    ///
    /// 17.041 to 22.137 is a real hole in the recorded output. Naming the
    /// nearest voice would invent an attribution indistinguishable from a
    /// measured one.
    ///
    /// **`vec![None]` is also what an empty `turns` produces**, for every span
    /// there is — so without the line below this asserted a hole in nothing.
    /// The control is the turn on the far side of the hole, 22.137--24.837
    /// speaker_00: it proves the parse reached this region of the recording, so
    /// the `None` is a gap between turns rather than an absence of turns.
    ///
    /// **Mutation:** make `interpret` return `Ok(Vec::new())`. The control fails
    /// and the negative goes on passing.
    #[test]
    fn a_span_diarization_never_covered_is_none() {
        let turns = interpret(&recorded_english_model(), "", true).unwrap();
        assert_eq!(
            assign(&turns, &[(23.0, 24.0)]),
            vec![Some(0)],
            "the control: `assign` answers None for every span of an empty \
             `turns`, so the hole below needs a turn beside it to be a hole in \
             anything"
        );
        assert_eq!(assign(&turns, &[(18.0, 21.0)]), vec![None]);
    }

    #[test]
    fn a_tie_goes_to_the_earlier_turn() {
        let turns = vec![
            Turn {
                start_seconds: 0.0,
                end_seconds: 1.0,
                speaker: 0,
            },
            Turn {
                start_seconds: 1.0,
                end_seconds: 2.0,
                speaker: 1,
            },
        ];
        assert_eq!(assign(&turns, &[(0.5, 1.5)]), vec![Some(0)]);
    }

    #[test]
    fn assign_answers_once_per_span_and_in_order() {
        let turns = interpret(&recorded_control(), "", true).unwrap();
        let spans = [(0.0, 1.0), (100.0, 101.0), (28.0, 29.0)];
        assert_eq!(assign(&turns, &spans), vec![Some(0), None, Some(3)]);
    }

    #[test]
    fn build_args_names_the_flags_the_cli_accepts() {
        let models = Models {
            segmentation: PathBuf::from("/m/seg/model.onnx"),
            embedding: PathBuf::from("/m/campplus.onnx"),
        };
        let args = build_args(&models, Path::new("/rec/a.wav"), &Tuning::default());
        assert_eq!(
            args,
            vec![
                "--segmentation.pyannote-model=/m/seg/model.onnx".to_owned(),
                "--embedding.model=/m/campplus.onnx".to_owned(),
                "--clustering.cluster-threshold=0.5".to_owned(),
                "--min-duration-on=0.3".to_owned(),
                "--min-duration-off=0.5".to_owned(),
                "/rec/a.wav".to_owned(),
            ]
        );
    }

    /// `--num-clusters` is never passed.
    ///
    /// Measured: asking for four produced three, so it is a hint rather than a
    /// promise -- and a meeting does not know its own attendance in the first
    /// place. If this flag ever appears, something has decided it knows how many
    /// people are in the room.
    ///
    /// **"No argument contains num-clusters" is trivially true of no arguments
    /// at all**, so a `build_args` that returned `vec![]` — or that was renamed
    /// out from under this — would leave it green while proving nothing. The
    /// control is one flag the list must carry. `build_args_names_the_flags_the_cli_accepts`
    /// pins the whole vector by equality two tests above, but a negative that
    /// depends on a sibling for its meaning is one deletion away from meaning
    /// nothing.
    ///
    /// **Mutation:** make `build_args` return `Vec::new()`. The control fails
    /// and the negative goes on passing.
    #[test]
    fn num_clusters_is_never_passed() {
        let models = Models {
            segmentation: PathBuf::from("seg/model.onnx"),
            embedding: PathBuf::from("emb.onnx"),
        };
        let args = build_args(&models, Path::new("a.wav"), &Tuning::default());
        assert!(
            args.iter()
                .any(|a| a.starts_with("--clustering.cluster-threshold=")),
            "the control: an empty or renamed argument list makes the negative \
             below true for the wrong reason -- {args:?}"
        );
        assert!(!args.iter().any(|a| a.contains("num-clusters")), "{args:?}");
    }

    #[test]
    fn locate_names_which_model_is_missing() {
        let dir = std::env::temp_dir().join("kw-diarization-locate-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        match Models::locate(&dir, &dir.join("campplus.onnx")) {
            Err(LayoutError::MissingSegmentation { file, .. }) => {
                assert_eq!(file, SEGMENTATION_MODEL_FILE);
            }
            other => panic!("{other:?}"),
        }

        std::fs::write(dir.join(SEGMENTATION_MODEL_FILE), b"not really a model").unwrap();
        match Models::locate(&dir, &dir.join("campplus.onnx")) {
            Err(LayoutError::MissingEmbedding { path }) => {
                assert!(path.ends_with("campplus.onnx"), "{path:?}");
            }
            other => panic!("{other:?}"),
        }

        std::fs::write(dir.join("campplus.onnx"), b"nor this").unwrap();
        let models = Models::locate(&dir, &dir.join("campplus.onnx")).unwrap();
        assert!(models.segmentation.ends_with(SEGMENTATION_MODEL_FILE));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_int8_build_is_not_what_this_was_measured_through() {
        // Both files ship in the archive. Naming the float one is a choice, and
        // this is the test that makes changing it deliberate.
        assert_eq!(SEGMENTATION_MODEL_FILE, "model.onnx");
    }
}
