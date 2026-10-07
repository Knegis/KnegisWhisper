//! Find the speech first, then transcribe only that (KNEG-404).
//!
//! Long recordings -- media imports and the meeting's whole-recording pass --
//! went to `whisper-server` as one request, silence and all. Measured on
//! 2026-10-06 against `ggml-base`, on 102 s of ten sentences under pink noise:
//! rows started up to 7.0 s before their speech and ended up to 16.2 s after
//! it, the same on three runs, and 40 s of noise alone came back as invented
//! text.
//!
//! # Why not the server's own VAD
//!
//! `whisper-server` can skip silence itself (`--vad-model` at start, `vad=true`
//! per request), and its detector is accurate. Its way back to the
//! recording's clock is not. It joins the speech with 0.1 s of inserted
//! silence and spreads that 0.1 s across the whole original gap, so a row
//! boundary next to a gap can land anywhere in it: starts up to 31 s early and
//! ends up to 26 s late, measured on `b4938` and `b5130` alike. It also leaves
//! that mapping behind for the next request that does not ask for VAD. The
//! measurements are in KNEG-404.
//!
//! # What this does instead
//!
//! The design of KBLab's easytranscriber and of `WhisperX`:
//!
//! 1. `whisper-vad-speech-segments`, from the same whisper.cpp release as the
//!    server, lists where the speech is, using the Silero model the
//!    application bundles ([`SpeechFinder`]).
//! 2. [`plan`] groups that into chunks of at most [`CHUNK_SECONDS`] of speech,
//!    and starts a new chunk at any pause of [`SPLIT_GAP_SECONDS`] or more.
//!    Merging across longer pauses let whisper stretch one row over a 12.6 s
//!    gap inside a chunk.
//! 3. The language is detected once, on the first [`DETECTION_SECONDS`] of
//!    speech ([`detection_sample`]), and every chunk is sent with it. Detecting
//!    per chunk could disagree from one chunk to the next, and whisper told
//!    the wrong language tends to translate rather than transcribe.
//! 4. Each chunk is its own request. [`stitch`] moves its rows onto the
//!    recording's clock by the chunk's start, so the times are right by
//!    construction and there is no mapping step to get wrong.
//!
//! A recording with no speech in it is answered [`Inference::Empty`] without
//! asking whisper anything: no invented text, and no vocabulary prompt
//! repeated back as a transcript (the failure `kw_audio_core::vad` keeps
//! dictation out of this for).
//!
//! # Measured
//!
//! `tests/speech_first.rs`, against the real server, tool and model on
//! 2026-10-06, on 102 s of the test fixture repeated four times under a
//! noise floor. Sent whole: 6 rows for 8 sentences, the first starting 20 s
//! before its speech and one ending 15.5 s after it. Speech first: 8 rows,
//! one per sentence, every start within 0.28 s and every end within 0.34 s of
//! the speech. 40 s of the noise alone: [`Inference::Empty`].
//!
//! # What it costs
//!
//! Every chunk is a full 30-second decoder window, so speech spread thinly
//! across many short chunks costs more requests than one pass over the file
//! would: 6.56 s against 4.65 s in the measurement above. The gain is in
//! where the rows sit and in what silence produces, not in speed.
//!
//! # When it cannot run
//!
//! No tool, no model, a WAV that is not 16 kHz mono 16-bit PCM, or a tool that
//! fails: the recording goes as one request, exactly as before, and a warning
//! is logged. Not finding the speech first costs a worse transcript, never a
//! lost one.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use kw_audio_core::vad::VadConfig;

use crate::local::{Inference, TimedSegment};
use crate::scratch::{unique_wav_path, ScratchWav};

/// A pause this long or longer ends a chunk.
///
/// Within a chunk whisper places the boundaries itself, and over a short pause
/// that is accurate. Over a long one it was not: a chunk merged across a
/// 12.6 s gap came back with a row stretched across the whole gap.
pub const SPLIT_GAP_SECONDS: f64 = 2.0;

/// The most speech one chunk holds: whisper's own window.
pub const CHUNK_SECONDS: f64 = 30.0;

/// Audio kept either side of a chunk's speech, so a word the detector clipped
/// is still heard whole. What the measured prototype used.
pub const CHUNK_PAD_SECONDS: f64 = 0.2;

/// How much speech the language is detected on. Whisper decides on one
/// window, so more would not be read.
pub const DETECTION_SECONDS: f64 = 30.0;

/// The rate, channel count and sample width every chunk is sliced from.
/// What `kw_audio_core` records and media import converts to.
const RATE: u32 = 16_000;
const SAMPLE_BYTES: usize = 2;

/// A stretch of the recording, in seconds from its start.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Span {
    pub start: f64,
    pub end: f64,
}

impl Span {
    #[must_use]
    pub const fn new(start: f64, end: f64) -> Self {
        Self { start, end }
    }

    fn seconds(self) -> f64 {
        self.end - self.start
    }
}

/// Read what `whisper-vad-speech-segments` printed.
///
/// `None` unless the output is the tool's whole answer: the count line, then
/// exactly that many segments. An empty list means the tool found no speech;
/// `None` means it did not say, and the two must not be confused, because
/// the first ends a transcription with nothing and the second sends the
/// recording whole.
///
/// The tool prints centiseconds (its `--help` says so) with CRLF line ends on
/// Windows.
#[must_use]
pub fn parse_spans(stdout: &str) -> Option<Vec<Span>> {
    let mut lines = stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty());
    let count: usize = lines
        .next()?
        .strip_prefix("Detected ")?
        .strip_suffix(" speech segments:")?
        .parse()
        .ok()?;

    let mut spans = Vec::with_capacity(count);
    for line in lines {
        let (_, times) = line
            .strip_prefix("Speech segment ")?
            .split_once(": start = ")?;
        let (start, end) = times.split_once(", end = ")?;
        let start = centiseconds(start)?;
        let end = centiseconds(end)?;
        if end < start {
            return None;
        }
        spans.push(Span::new(start, end));
    }
    (spans.len() == count).then_some(spans)
}

/// Seconds from the tool's centiseconds, if they are a time at all.
fn centiseconds(value: &str) -> Option<f64> {
    let value: f64 = value.trim().parse().ok()?;
    (value.is_finite() && value >= 0.0).then_some(value / 100.0)
}

/// Group the speech into the chunks that will be transcribed.
///
/// Sorted, never overlapping, and each padded by [`CHUNK_PAD_SECONDS`] within
/// the recording. Padding never reaches past the middle of the pause to the
/// next chunk, so no audio is sent twice. A single stretch of speech longer
/// than [`CHUNK_SECONDS`] stays whole: the tool already splits speech at that
/// length, at its quietest point, and cutting it again here would cut a word.
#[must_use]
pub fn plan(spans: &[Span], duration: f64) -> Vec<Span> {
    let mut speech: Vec<Span> = spans
        .iter()
        .copied()
        .filter(|span| span.start.is_finite() && span.end.is_finite() && span.end > span.start)
        .collect();
    speech.sort_by(|a, b| a.start.total_cmp(&b.start));

    let mut chunks: Vec<Span> = Vec::new();
    for span in speech {
        match chunks.last_mut() {
            Some(chunk)
                if span.start - chunk.end < SPLIT_GAP_SECONDS
                    && span.end - chunk.start <= CHUNK_SECONDS =>
            {
                chunk.end = chunk.end.max(span.end);
            }
            _ => chunks.push(span),
        }
    }

    let mut padded = Vec::with_capacity(chunks.len());
    let mut floor = 0.0_f64;
    let mut rest = chunks.iter().peekable();
    while let Some(chunk) = rest.next() {
        let ceiling = rest
            .peek()
            .map_or(duration, |next| midpoint(chunk.end, next.start))
            .min(duration);
        let start = (chunk.start - CHUNK_PAD_SECONDS).max(floor);
        let end = (chunk.end + CHUNK_PAD_SECONDS).min(ceiling);
        if end > start {
            padded.push(Span::new(start, end));
        }
        floor = ceiling;
    }
    padded
}

/// Halfway across the pause between two chunks, where one's padding stops
/// and the next one's starts.
fn midpoint(end: f64, next_start: f64) -> f64 {
    end + (next_start - end) / 2.0
}

/// The stretches the language is detected on: the first
/// [`DETECTION_SECONDS`] of speech, in order.
#[must_use]
pub fn detection_sample(chunks: &[Span]) -> Vec<Span> {
    let mut left = DETECTION_SECONDS;
    let mut sample = Vec::new();
    for chunk in chunks {
        if left <= 0.0 {
            break;
        }
        let take = chunk.seconds().min(left);
        sample.push(Span::new(chunk.start, chunk.start + take));
        left -= take;
    }
    sample
}

/// Put the chunks' answers back together, on the recording's clock.
///
/// A chunk that heard nothing is skipped. A refusal is the answer: a
/// transcript with a missing chunk must not look complete. If any chunk
/// answered without timings, the whole answer is untimed, for the reason
/// `local::timed_segments` gives -- cues that are right for some rows and
/// absent for others are worse than none.
#[must_use]
pub fn stitch(parts: Vec<(Span, Inference)>) -> Inference {
    let mut texts = Vec::with_capacity(parts.len());
    let mut segments = Vec::new();
    let mut untimed = false;
    for (chunk, inference) in parts {
        match inference {
            Inference::Rejected { .. } => return inference,
            Inference::Empty => {}
            Inference::Text(text) => {
                untimed = true;
                texts.push(text);
            }
            Inference::Timed {
                text,
                segments: rows,
            } => {
                texts.push(text);
                segments.extend(rows.into_iter().map(|row| {
                    let start = (row.start + chunk.start).min(chunk.end);
                    TimedSegment {
                        text: row.text,
                        start,
                        end: (row.end + chunk.start).clamp(start, chunk.end),
                    }
                }));
            }
        }
    }

    if texts.is_empty() {
        return Inference::Empty;
    }
    let text = texts.join(" ");
    if untimed || segments.is_empty() {
        Inference::Text(text)
    } else {
        Inference::Timed { text, segments }
    }
}

/// The 16 kHz mono 16-bit samples inside a WAV.
#[derive(Debug, Clone, Copy)]
pub struct Pcm<'a> {
    samples: &'a [u8],
}

impl<'a> Pcm<'a> {
    /// The samples, if `wav` is 16 kHz mono 16-bit PCM.
    ///
    /// Walks the RIFF chunks rather than assuming a 44-byte header: ffmpeg,
    /// which converts every import, writes a `LIST` chunk before the data.
    #[must_use]
    pub fn parse(wav: &'a [u8]) -> Option<Self> {
        if wav.get(..4)? != b"RIFF" || wav.get(8..12)? != b"WAVE" {
            return None;
        }
        let mut at = 12_usize;
        let mut format_read = false;
        while let Some(header) = wav.get(at..at.checked_add(8)?) {
            let size = usize::try_from(read_u32(header, 4)?).ok()?;
            let body = at + 8;
            match header.get(..4)? {
                b"fmt " => {
                    let format = wav.get(body..body.checked_add(16)?)?;
                    let pcm = read_u16(format, 0)? == 1;
                    let mono = read_u16(format, 2)? == 1;
                    let rate = read_u32(format, 4)? == RATE;
                    let bits = read_u16(format, 14)? == 16;
                    if !(pcm && mono && rate && bits) {
                        return None;
                    }
                    format_read = true;
                }
                b"data" if format_read => {
                    // A streamed writer leaves the size unknown; read what is
                    // there, whole samples only.
                    let end = body.saturating_add(size).min(wav.len());
                    let whole = (end.saturating_sub(body) / SAMPLE_BYTES) * SAMPLE_BYTES;
                    return Some(Self {
                        samples: wav.get(body..body + whole)?,
                    });
                }
                b"data" => return None,
                _ => {}
            }
            // RIFF pads every odd-sized chunk to an even length.
            at = body.checked_add(size)?.checked_add(size % 2)?;
        }
        None
    }

    /// How long the recording is.
    #[must_use]
    #[allow(clippy::cast_precision_loss)]
    pub fn duration(&self) -> f64 {
        (self.samples.len() / SAMPLE_BYTES) as f64 / f64::from(RATE)
    }

    /// `span` of the recording, as a WAV of its own.
    #[must_use]
    pub fn slice(&self, span: Span) -> Vec<u8> {
        kw_audio_core::wav::wrap_pcm(self.bytes(span), RATE, 1)
    }

    /// `spans` of the recording, one after another, as a single WAV.
    #[must_use]
    pub fn join(&self, spans: &[Span]) -> Vec<u8> {
        let mut pcm = Vec::new();
        for span in spans {
            pcm.extend_from_slice(self.bytes(*span));
        }
        kw_audio_core::wav::wrap_pcm(&pcm, RATE, 1)
    }

    /// The samples of `span`, clamped to the recording.
    fn bytes(&self, span: Span) -> &'a [u8] {
        let count = self.samples.len() / SAMPLE_BYTES;
        let first = sample_at(span.start).min(count);
        let last = sample_at(span.end).clamp(first, count);
        self.samples
            .get(first * SAMPLE_BYTES..last * SAMPLE_BYTES)
            .unwrap_or_default()
    }
}

/// The sample `seconds` falls on. Negative and non-finite times are the start.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn sample_at(seconds: f64) -> usize {
    if seconds.is_finite() && seconds > 0.0 {
        (seconds * f64::from(RATE)).round() as usize
    } else {
        0
    }
}

fn read_u16(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(bytes.get(at..at + 2)?.try_into().ok()?))
}

fn read_u32(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

/// Something that can say where the speech in a recording is.
///
/// A trait so the order of requests around it can be tested without a
/// process; [`SpeechFinder`] is the one the application uses.
#[async_trait::async_trait]
pub trait FindSpeech: Send + Sync + std::fmt::Debug {
    /// The speech in `wav`, in order. Empty when there is none.
    async fn find(&self, wav: &[u8]) -> Result<Vec<Span>, String>;
}

/// `whisper-vad-speech-segments` with the bundled Silero model.
#[derive(Debug, Clone, PartialEq)]
pub struct SpeechFinder {
    tool: PathBuf,
    model: PathBuf,
    config: VadConfig,
    /// Where the recording is staged for the tool, which reads a file. The
    /// caller's ASCII-safe scratch directory, as for `SherpaOffline`: the
    /// native binaries fail on paths they cannot read in the ANSI code page.
    temp_dir: PathBuf,
}

impl SpeechFinder {
    /// `config` is clamped here, because whisper.cpp does not validate it.
    #[must_use]
    pub fn new(tool: PathBuf, model: PathBuf, config: VadConfig, temp_dir: PathBuf) -> Self {
        Self {
            tool,
            model,
            config: config.sanitised(),
            temp_dir,
        }
    }

    /// The tool's arguments for a recording staged at `wav`.
    ///
    /// Every setting is passed, because the tool's own defaults are not
    /// upstream's: 30 ms of padding where upstream has 100, and no length
    /// limit where upstream has 30 s. `-np` leaves stdout to the answer.
    #[must_use]
    pub fn args(&self, wav: &Path) -> Vec<OsString> {
        let config = self.config;
        let mut args: Vec<OsString> = vec![
            "-vm".into(),
            self.model.clone().into(),
            "-f".into(),
            wav.into(),
        ];
        for (flag, value) in [
            ("-vt", config.threshold.to_string()),
            ("-vspd", config.min_speech_duration_ms.to_string()),
            ("-vsd", config.min_silence_duration_ms.to_string()),
            ("-vmsd", config.max_speech_duration_s.to_string()),
            ("-vp", config.speech_pad_ms.to_string()),
            ("-vo", config.samples_overlap.to_string()),
        ] {
            args.push(flag.into());
            args.push(value.into());
        }
        args.push("-np".into());
        args
    }
}

/// How much of the tool's error output is worth repeating.
const STDERR_TAIL: usize = 300;

/// Read the tool's exit and output.
///
/// # Errors
///
/// When the tool exited with anything but 0, or printed something that is
/// not its answer.
pub fn interpret_output(
    code: Option<i32>,
    stdout: &str,
    stderr: &str,
) -> Result<Vec<Span>, String> {
    if code != Some(0) {
        let exit = code.map_or_else(
            || "without an exit code".to_owned(),
            |c| format!("with {c}"),
        );
        return Err(format!(
            "the speech finder exited {exit}: {}",
            tail(stderr.trim(), STDERR_TAIL)
        ));
    }
    parse_spans(stdout).ok_or_else(|| "the speech finder's answer could not be read".to_owned())
}

/// The last `limit` characters of `text`, where the error usually is.
fn tail(text: &str, limit: usize) -> &str {
    let count = text.chars().count();
    if count <= limit {
        return text;
    }
    text.char_indices()
        .nth(count - limit)
        .and_then(|(at, _)| text.get(at..))
        .unwrap_or(text)
}

#[async_trait::async_trait]
impl FindSpeech for SpeechFinder {
    async fn find(&self, wav: &[u8]) -> Result<Vec<Span>, String> {
        tokio::fs::create_dir_all(&self.temp_dir)
            .await
            .map_err(|err| err.to_string())?;
        let staged = unique_wav_path(&self.temp_dir, "speech-input");
        let _scratch = ScratchWav(staged.clone());
        tokio::fs::write(&staged, wav)
            .await
            .map_err(|err| err.to_string())?;

        let mut command = tokio::process::Command::new(&self.tool);
        command
            .args(self.args(&staged))
            .stdin(std::process::Stdio::null())
            .kill_on_drop(true);
        #[cfg(windows)]
        {
            // CREATE_NO_WINDOW, as `kw_sidecar::process` and media import set
            // it: a console flashing up during an import is its own bug.
            command.creation_flags(0x0800_0000);
        }

        let output = command
            .output()
            .await
            .map_err(|err| format!("could not run {}: {err}", self.tool.display()))?;
        interpret_output(
            output.status.code(),
            &String::from_utf8_lossy(&output.stdout),
            &String::from_utf8_lossy(&output.stderr),
        )
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation
)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    fn assert_spans(actual: &[Span], expected: &[(f64, f64)]) {
        assert_eq!(
            actual.len(),
            expected.len(),
            "{actual:?} against {expected:?}"
        );
        for (span, (start, end)) in actual.iter().zip(expected) {
            assert!(
                close(span.start, *start) && close(span.end, *end),
                "{actual:?} against {expected:?}"
            );
        }
    }

    fn spans(list: &[(f64, f64)]) -> Vec<Span> {
        list.iter().map(|(s, e)| Span::new(*s, *e)).collect()
    }

    /* -------------------------------------------- what the tool printed */

    /// Copied from the tool's own output on 2026-10-06, CRLF and all.
    const THREE: &str = "\r\nDetected 3 speech segments:\r\n\
        Speech segment 0: start = 304.00, end = 442.00\r\n\
        Speech segment 1: start = 464.00, end = 688.00\r\n\
        Speech segment 2: start = 812.00, end = 1127.00\r\n\r\n";

    #[test]
    fn the_tools_segments_are_read_in_centiseconds() {
        assert_spans(
            &parse_spans(THREE).unwrap(),
            &[(3.04, 4.42), (4.64, 6.88), (8.12, 11.27)],
        );
    }

    #[test]
    fn no_speech_is_an_empty_answer_rather_than_no_answer() {
        assert_eq!(
            parse_spans("\r\nDetected 0 speech segments:\r\n\r\n"),
            Some(Vec::new())
        );
    }

    #[test]
    fn output_without_the_count_is_no_answer() {
        // An empty list here would end an import with nothing, when the tool
        // had simply failed to say anything.
        assert_eq!(parse_spans(""), None);
        assert_eq!(
            parse_spans("Speech segment 0: start = 304.00, end = 442.00\r\n"),
            None
        );
        assert_eq!(parse_spans("error: failed to read audio data\n"), None);
    }

    #[test]
    fn a_count_that_disagrees_with_the_segments_is_no_answer() {
        let short = "Detected 2 speech segments:\nSpeech segment 0: start = 1.00, end = 2.00\n";
        assert_eq!(parse_spans(short), None);
    }

    #[test]
    fn a_segment_that_ends_before_it_starts_is_no_answer() {
        let backwards = "Detected 1 speech segments:\nSpeech segment 0: start = 9.00, end = 2.00\n";
        assert_eq!(parse_spans(backwards), None);
        let garbled = "Detected 1 speech segments:\nSpeech segment 0: start = x, end = 2.00\n";
        assert_eq!(parse_spans(garbled), None);
    }

    #[test]
    fn a_failed_run_is_an_error_carrying_what_the_tool_said() {
        let err = interpret_output(
            Some(2),
            "",
            "read_audio_data: trying to decode\nerror: failed to initialize whisper context\n",
        )
        .unwrap_err();
        assert!(
            err.contains("failed to initialize whisper context"),
            "{err}"
        );
        // Killed, or crashed without a code (a non-ASCII model path did, with
        // 127 and no output at all).
        assert!(interpret_output(None, "", "").is_err());
        assert!(interpret_output(Some(127), "", "").is_err());
    }

    #[test]
    fn a_clean_run_is_its_segments_and_an_unreadable_one_is_an_error() {
        assert_eq!(interpret_output(Some(0), THREE, "").unwrap().len(), 3);
        assert_eq!(
            interpret_output(Some(0), "Detected 0 speech segments:\n", "").unwrap(),
            Vec::new()
        );
        assert!(interpret_output(Some(0), "something else", "").is_err());
    }

    /* ------------------------------------------------------- the chunks */

    #[test]
    fn speech_with_short_pauses_is_one_chunk() {
        assert_spans(
            &plan(&spans(&[(3.04, 4.42), (4.64, 6.88), (8.12, 11.27)]), 100.0),
            &[(2.84, 11.47)],
        );
    }

    #[test]
    fn a_pause_of_two_seconds_or_more_starts_a_new_chunk() {
        assert_spans(
            &plan(&spans(&[(1.0, 2.0), (4.0, 5.0)]), 10.0),
            &[(0.8, 2.2), (3.8, 5.2)],
        );
        assert_spans(
            &plan(&spans(&[(1.0, 2.0), (3.99, 5.0)]), 10.0),
            &[(0.8, 5.2)],
        );
    }

    #[test]
    fn padding_stays_inside_the_recording() {
        assert_spans(
            &plan(&spans(&[(0.05, 1.0), (8.0, 9.98)]), 10.0),
            &[(0.0, 1.2), (7.8, 10.0)],
        );
    }

    #[test]
    fn a_long_stretch_of_speech_is_not_cut() {
        assert_spans(&plan(&spans(&[(5.0, 40.0)]), 60.0), &[(4.8, 40.2)]);
    }

    #[test]
    fn no_speech_plans_nothing() {
        assert!(plan(&[], 60.0).is_empty());
    }

    #[test]
    fn the_speech_is_planned_in_order_whatever_order_it_came_in() {
        assert_spans(
            &plan(&spans(&[(8.0, 9.0), (1.0, 2.0)]), 20.0),
            &[(0.8, 2.2), (7.8, 9.2)],
        );
    }

    #[test]
    fn chunks_hold_at_most_thirty_seconds_of_speech_and_never_share_audio() {
        // Half a second of speech every 0.6 s for a minute: no pause ever
        // ends a chunk, so only the length does, and the 0.1 s pauses are
        // too short for both neighbours' padding.
        let speech: Vec<Span> = (0..100)
            .map(|i| Span::new(f64::from(i) * 0.6 + 1.0, f64::from(i) * 0.6 + 1.5))
            .collect();
        let chunks = plan(&speech, 70.0);
        assert!(chunks.len() >= 2, "{chunks:?}");
        for pair in chunks.windows(2) {
            assert!(pair[0].end <= pair[1].start, "{chunks:?} overlap");
        }
        for chunk in &chunks {
            assert!(
                chunk.seconds() <= CHUNK_SECONDS + 2.0 * CHUNK_PAD_SECONDS + 1e-9,
                "{chunk:?} is longer than one window"
            );
        }
        for span in &speech {
            let holders = chunks
                .iter()
                .filter(|c| c.start <= span.start && span.end <= c.end)
                .count();
            assert_eq!(holders, 1, "{span:?} is in {holders} chunks of {chunks:?}");
        }
    }

    #[test]
    fn the_language_is_detected_on_the_first_thirty_seconds_of_speech() {
        assert_spans(
            &detection_sample(&spans(&[(0.0, 10.0), (20.0, 45.0), (60.0, 70.0)])),
            &[(0.0, 10.0), (20.0, 40.0)],
        );
        assert_spans(&detection_sample(&spans(&[(1.0, 3.0)])), &[(1.0, 3.0)]);
    }

    /* ------------------------------------------------- putting it back */

    fn timed(text: &str, rows: &[(&str, f64, f64)]) -> Inference {
        Inference::Timed {
            text: text.to_owned(),
            segments: rows
                .iter()
                .map(|(t, s, e)| TimedSegment {
                    text: (*t).to_owned(),
                    start: *s,
                    end: *e,
                })
                .collect(),
        }
    }

    #[test]
    fn each_chunks_rows_move_onto_the_recordings_clock() {
        let stitched = stitch(vec![
            (
                Span::new(10.0, 25.0),
                timed("Hello there", &[("Hello there", 0.3, 2.1)]),
            ),
            (
                Span::new(40.0, 50.0),
                timed(
                    "Second. Third.",
                    &[("Second.", 0.2, 1.0), ("Third.", 1.5, 3.0)],
                ),
            ),
        ]);
        assert_eq!(
            stitched,
            timed(
                "Hello there Second. Third.",
                &[
                    ("Hello there", 10.3, 12.1),
                    ("Second.", 40.2, 41.0),
                    ("Third.", 41.5, 43.0)
                ]
            )
        );
    }

    #[test]
    fn a_row_never_runs_past_its_chunk() {
        assert_eq!(
            stitch(vec![(
                Span::new(40.0, 50.0),
                timed("Long", &[("Long", 8.0, 20.0)])
            )]),
            timed("Long", &[("Long", 48.0, 50.0)])
        );
    }

    #[test]
    fn a_chunk_that_heard_nothing_is_skipped_and_all_of_them_is_nothing() {
        assert_eq!(
            stitch(vec![
                (Span::new(0.0, 5.0), Inference::Empty),
                (Span::new(9.0, 12.0), timed("Yes", &[("Yes", 0.5, 1.0)])),
            ]),
            timed("Yes", &[("Yes", 9.5, 10.0)])
        );
        assert_eq!(
            stitch(vec![(Span::new(0.0, 5.0), Inference::Empty)]),
            Inference::Empty
        );
        assert_eq!(stitch(Vec::new()), Inference::Empty);
    }

    #[test]
    fn one_untimed_chunk_makes_the_whole_answer_untimed() {
        assert_eq!(
            stitch(vec![
                (Span::new(0.0, 5.0), timed("One", &[("One", 0.5, 1.0)])),
                (Span::new(9.0, 12.0), Inference::Text("Two".to_owned())),
            ]),
            Inference::Text("One Two".to_owned())
        );
    }

    #[test]
    fn a_refused_chunk_is_the_answer() {
        let refused = Inference::Rejected {
            status: 500,
            body: "failed".to_owned(),
        };
        assert_eq!(
            stitch(vec![
                (Span::new(0.0, 5.0), timed("One", &[("One", 0.5, 1.0)])),
                (Span::new(9.0, 12.0), refused.clone()),
            ]),
            refused
        );
    }

    /* ---------------------------------------------------------- the WAV */

    /// `seconds` of 16 kHz mono, each sample its own index, so a slice can be
    /// checked sample for sample.
    fn ramp(seconds: usize) -> (Vec<u8>, Vec<u8>) {
        let mut pcm = Vec::new();
        for index in 0..seconds * 16_000 {
            pcm.extend_from_slice(&((index % 30_000) as i16).to_le_bytes());
        }
        (kw_audio_core::wav::wrap_pcm(&pcm, 16_000, 1), pcm)
    }

    #[test]
    fn a_recording_is_read_from_the_header_kw_audio_core_writes() {
        let wav = kw_audio_core::wav::encode_wav(&vec![0.0; 24_000], 16_000);
        assert!(close(Pcm::parse(&wav).unwrap().duration(), 1.5));
    }

    #[test]
    fn a_recording_is_read_past_chunks_ffmpeg_puts_before_the_data() {
        let (plain, pcm) = ramp(2);
        // RIFF header and fmt chunk from the plain file, then a LIST chunk of
        // odd length (padded to even, as RIFF requires), then the data.
        let mut wav = plain[..36].to_vec();
        wav.extend_from_slice(b"LIST");
        wav.extend_from_slice(&5_u32.to_le_bytes());
        wav.extend_from_slice(b"Lavf\0\0");
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&u32::try_from(pcm.len()).unwrap().to_le_bytes());
        wav.extend_from_slice(&pcm);
        assert!(close(Pcm::parse(&wav).unwrap().duration(), 2.0));
    }

    #[test]
    fn a_data_size_past_the_end_of_the_file_reads_what_is_there() {
        let (mut wav, _) = ramp(1);
        wav[40..44].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(close(Pcm::parse(&wav).unwrap().duration(), 1.0));
    }

    #[test]
    fn anything_but_16k_mono_16_bit_pcm_is_not_read() {
        let (_, pcm) = ramp(1);
        assert!(Pcm::parse(&kw_audio_core::wav::wrap_pcm(&pcm, 16_000, 2)).is_none());
        assert!(Pcm::parse(&kw_audio_core::wav::wrap_pcm(&pcm, 44_100, 1)).is_none());
        let mut eight_bit = kw_audio_core::wav::wrap_pcm(&pcm, 16_000, 1);
        eight_bit[34..36].copy_from_slice(&8_u16.to_le_bytes());
        assert!(Pcm::parse(&eight_bit).is_none());
        let mut float = kw_audio_core::wav::wrap_pcm(&pcm, 16_000, 1);
        float[20..22].copy_from_slice(&3_u16.to_le_bytes());
        assert!(Pcm::parse(&float).is_none());
        assert!(Pcm::parse(b"not a wav at all").is_none());
        assert!(Pcm::parse(&[]).is_none());
    }

    #[test]
    fn a_slice_is_exactly_the_samples_of_its_span() {
        let (wav, pcm) = ramp(3);
        let audio = Pcm::parse(&wav).unwrap();
        let slice = audio.slice(Span::new(0.5, 1.25));
        assert_eq!(
            slice,
            kw_audio_core::wav::wrap_pcm(&pcm[16_000..40_000], 16_000, 1)
        );
        // Past the end is clamped to it.
        let tail = audio.slice(Span::new(2.5, 9.0));
        assert_eq!(
            tail,
            kw_audio_core::wav::wrap_pcm(&pcm[80_000..], 16_000, 1)
        );
    }

    #[test]
    fn joined_spans_follow_one_another() {
        let (wav, pcm) = ramp(3);
        let joined = Pcm::parse(&wav)
            .unwrap()
            .join(&[Span::new(0.0, 0.5), Span::new(2.0, 2.5)]);
        let mut expected = pcm[..16_000].to_vec();
        expected.extend_from_slice(&pcm[64_000..80_000]);
        assert_eq!(joined, kw_audio_core::wav::wrap_pcm(&expected, 16_000, 1));
    }

    /* --------------------------------------------------------- the tool */

    #[test]
    fn the_tool_is_given_every_setting_explicitly() {
        // Explicitly, because the tool's own defaults are not upstream's:
        // 0.03 s padding and no length limit, against 0.1 s and 30 s.
        let finder = SpeechFinder::new(
            PathBuf::from("vad.exe"),
            PathBuf::from("silero.bin"),
            VadConfig::default(),
            PathBuf::from("scratch"),
        );
        let args: Vec<String> = finder
            .args(Path::new("in.wav"))
            .into_iter()
            .map(|a| a.into_string().unwrap())
            .collect();
        assert_eq!(
            args,
            [
                "-vm",
                "silero.bin",
                "-f",
                "in.wav",
                "-vt",
                "0.5",
                "-vspd",
                "250",
                "-vsd",
                "200",
                "-vmsd",
                "30",
                "-vp",
                "100",
                "-vo",
                "0.5",
                "-np"
            ]
        );
    }

    #[test]
    fn the_tools_settings_are_clamped_first() {
        let finder = SpeechFinder::new(
            PathBuf::from("vad.exe"),
            PathBuf::from("silero.bin"),
            VadConfig {
                threshold: 7.0,
                ..VadConfig::default()
            },
            PathBuf::from("scratch"),
        );
        let args = finder.args(Path::new("in.wav"));
        let at = args.iter().position(|a| a == "-vt").unwrap();
        assert_eq!(args[at + 1], "0.95");
    }
}
