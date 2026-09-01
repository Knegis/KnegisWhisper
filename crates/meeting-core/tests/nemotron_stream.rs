//! The shipped streaming client against the real sherpa server.
//!
//! Everything in `nemotron.rs`'s own tests is a fixture taken off the wire, so
//! it proves the rules read a recorded frame correctly and nothing about whether
//! the client works. This drives
//! [`kw_meeting_core::nemotron_socket::LocalStreaming`] — the type the
//! application builds — against a real `sherpa-onnx-online-websocket-server`
//! running a real model, and checks the words come back.
//!
//! The same shape and the same reason as
//! `crates/transcription-core/tests/sherpa_dictation.rs`: `--ignored`, because
//! it needs ~630 MB of model and a binary on disk, and committed rather than run
//! once by hand, so it stays true as the pinned sherpa release moves rather than
//! being true only on the day the probe ran.
//!
//! ```text
//! KW_SHERPA_SERVER=<...>/sherpa-onnx-online-websocket-server.exe \
//! KW_SHERPA_STREAM_MODEL=<...>/sherpa-onnx-nemotron-speech-streaming-en-0.6b-...  \
//!   cargo test -p kw-meeting-core --test nemotron_stream -- --ignored --nocapture
//! ```
//!
//! # The control is the model's own transcript
//!
//! `test_wavs/trans.txt` ships with the model and is what its authors got. The
//! assertion is against that, not against a sentence chosen here — a test that
//! asserted whatever the client happened to produce would pass for a client that
//! was wrong in the same way twice.
//!
//! # What it proves, and what it does not
//!
//! It proves the client's own frames are the ones the server accepts: the `f32`
//! conversion, the frame size, the `Done` marker, the cumulative-text rule, and
//! the finish handshake against a server that never closes. **It is not a
//! meeting** — one capture rather than two, a file rather than a live device,
//! and nothing about whether words reach the screen as they are spoken.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::{Path, PathBuf};
use std::time::Duration;

use kw_meeting_core::nemotron::SAMPLE_RATE;
use kw_meeting_core::nemotron_socket::LocalStreaming;
use kw_meeting_core::{
    AudioChunk, AudioSource, MeetingProviderConfig, MeetingTranscriptionProvider,
};

/// A port well clear of every range `config/sidecars.json` allocates from, so a
/// run of this test cannot collide with a server the application started.
const PORT: u16 = 8_291;

fn from_env(name: &str) -> PathBuf {
    PathBuf::from(
        std::env::var(name)
            .unwrap_or_else(|_| panic!("{name} must point at the real thing for this test")),
    )
}

/// Find one file in `dir` whose name contains `part` and ends in `.onnx`.
///
/// The same discovery rule `kw_transcription_core::sherpa` uses, rather than
/// hard-coded names: the int8 and float builds of one model differ only in the
/// file names, and a test that only ran against one of them would be narrower
/// than the code it is checking.
fn onnx(dir: &Path, part: &str) -> PathBuf {
    std::fs::read_dir(dir)
        .expect("the model directory is readable")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| {
            path.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|name| name.contains(part) && name.ends_with(".onnx"))
        })
        .unwrap_or_else(|| panic!("no {part}*.onnx in {}", dir.display()))
}

/// 16-bit mono PCM out of a WAV, by finding the `data` chunk rather than
/// assuming it begins at offset 44.
fn wav_pcm(path: &Path) -> Vec<u8> {
    let bytes = std::fs::read(path).expect("the wav is readable");
    let mut at = 12;
    while let Some(header) = bytes.get(at..at + 8) {
        let size = header
            .get(4..8)
            .and_then(|raw| <[u8; 4]>::try_from(raw).ok())
            .map(|raw| u32::from_le_bytes(raw) as usize)
            .expect("a chunk header carries a size");
        if header.get(..4) == Some(b"data".as_slice()) {
            let end = (at + 8 + size).min(bytes.len());
            return bytes.get(at + 8..end).unwrap_or_default().to_vec();
        }
        at = at
            .saturating_add(8)
            .saturating_add(size)
            .saturating_add(size % 2);
    }
    panic!("no data chunk in {}", path.display());
}

/// The server, stopped when this is dropped.
struct Server(std::process::Child);

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

async fn start_server(executable: &Path, model: &Path) -> Server {
    let mut command = std::process::Command::new(executable);
    command
        .arg(format!("--port={PORT}"))
        .arg(format!("--tokens={}", model.join("tokens.txt").display()))
        .arg(format!("--encoder={}", onnx(model, "encoder").display()))
        .arg(format!("--decoder={}", onnx(model, "decoder").display()))
        .arg(format!("--joiner={}", onnx(model, "joiner").display()))
        .arg("--num-work-threads=5")
        // Not optional, and this test learnt it the hard way: left at its
        // default the server writes `./log.txt` into whatever the working
        // directory happens to be, which for `cargo test` is the crate root.
        // The first run of this file committed two of them. The shipped path
        // (`sherpa_stream::build_args`) has always passed this flag; the test
        // that exists to prove the shipped path works had not.
        .arg(format!(
            "--log-file={}",
            std::env::temp_dir()
                .join("kw-nemotron-stream-test.log")
                .display()
        ))
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());

    let child = Server(command.spawn().expect("the server starts"));

    // Polled, not slept: loading a 630 MB model takes as long as it takes, and a
    // fixed sleep long enough to be reliable is longer than this needs.
    let address = format!("ws://127.0.0.1:{PORT}/");
    for _ in 0..120 {
        tokio::time::sleep(Duration::from_millis(500)).await;
        if tokio_tungstenite::connect_async(address.as_str())
            .await
            .is_ok()
        {
            return child;
        }
    }
    panic!("the server never answered a handshake on {PORT}");
}

#[tokio::test]
#[ignore = "needs the sherpa server and a streaming model on disk"]
async fn the_shipped_client_transcribes_against_the_real_server() {
    let executable = from_env("KW_SHERPA_SERVER");
    let model = from_env("KW_SHERPA_STREAM_MODEL");

    let _server = start_server(&executable, &model).await;

    let mut client = LocalStreaming::new(
        kw_network_policy::NetworkPolicy::new().expect("a policy"),
        PORT,
    );

    client
        .connect(MeetingProviderConfig {
            provider: kw_meeting_core::nemotron::PROVIDER_ID.to_owned(),
            model: None,
            language: Some("en".to_owned()),
            keyterms: Vec::new(),
            key_ref: None,
            sample_rate: SAMPLE_RATE,
        })
        .await
        .expect("the session opens");

    // Fed in 4096-byte pieces, which is neither the frame size nor a divisor of
    // it. A chunk that lined up with `SAMPLES_PER_FRAME` would never exercise
    // `Pending`, and `Pending` is where the tail either survives or is lost.
    let pcm = wav_pcm(&model.join("test_wavs").join("0.wav"));
    for piece in pcm.chunks(4_096) {
        client
            .send_audio(AudioChunk {
                pcm: piece.to_vec(),
                source: AudioSource::Mic,
            })
            .await
            .expect("audio is accepted");
    }

    let transcript = client.finish().await.expect("the session finishes");

    // One event, not one per update: `text` is cumulative and the client
    // revises rather than appends. Thirteen events here would mean the rule was
    // inverted, which is exactly the failure the fixtures cannot catch on their
    // own.
    let mic: Vec<&kw_meeting_core::TranscriptEvent> = transcript
        .events
        .iter()
        .filter(|e| e.source == AudioSource::Mic)
        .collect();
    assert_eq!(mic.len(), 1, "{:#?}", transcript.events);
    assert!(mic[0].is_final, "the last update settles the event");

    // The model's own reference transcript, from `test_wavs/trans.txt`. Not a
    // sentence chosen here: a client wrong in the same way twice would pass
    // against its own output.
    let expected = "AFTER EARLY NIGHTFALL THE YELLOW LAMPS WOULD LIGHT UP HERE AND THERE THE \
                    SQUALID QUARTER OF THE BROTHELS";
    // Upper-cased, and that is a measurement rather than a convenience.
    // `trans.txt` is uppercase; the server emits properly-cased text ("After
    // early nightfall the yellow lamps..."). The first run of this test found
    // that out -- and found that this crate's own parsing fixture had been
    // copied from `trans.txt` rather than read off the wire. Comparing
    // case-insensitively keeps the model's file as the reference without
    // asserting a casing it never promised.
    assert_eq!(mic[0].text.trim().to_uppercase(), expected);

    assert_eq!(transcript.reconnects, 0);
    assert_eq!(transcript.gaps, 0);
}

#[tokio::test]
#[ignore = "needs the sherpa server and a streaming model on disk"]
async fn a_server_that_dies_mid_session_reaches_the_caller_as_an_error() {
    // The failure mode this provider is most exposed to. Its only fault channel
    // is transport-level -- the server sends no error frame, measured -- so if
    // this path is wrong a dead server produces a *quiet transcript* rather than
    // a failure, which is the shape of defect KNEG-79 was filed about.
    let executable = from_env("KW_SHERPA_SERVER");
    let model = from_env("KW_SHERPA_STREAM_MODEL");

    let server = start_server(&executable, &model).await;

    let mut client = LocalStreaming::new(
        kw_network_policy::NetworkPolicy::new().expect("a policy"),
        PORT,
    );
    client
        .connect(MeetingProviderConfig {
            provider: kw_meeting_core::nemotron::PROVIDER_ID.to_owned(),
            model: None,
            language: Some("en".to_owned()),
            keyterms: Vec::new(),
            key_ref: None,
            sample_rate: SAMPLE_RATE,
        })
        .await
        .expect("the session opens");

    let pcm = wav_pcm(&model.join("test_wavs").join("0.wav"));
    let half = pcm.len() / 2;
    for piece in pcm.get(..half).unwrap_or_default().chunks(4_096) {
        client
            .send_audio(AudioChunk {
                pcm: piece.to_vec(),
                source: AudioSource::Mic,
            })
            .await
            .expect("audio is accepted while the server is up");
    }

    drop(server);
    // The reader has to notice. Polled through `send_audio`, which is where a
    // real meeting would notice it too.
    let mut reported = None;
    for _ in 0..40 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        if let Err(err) = client
            .send_audio(AudioChunk {
                pcm: vec![0; 4_096],
                source: AudioSource::Mic,
            })
            .await
        {
            reported = Some(err);
            break;
        }
    }

    let err = reported.expect("a dead server must not keep accepting audio");
    let text = err.to_string();
    assert!(
        text.contains("local-streaming"),
        "the error names the provider: {text}"
    );
}
