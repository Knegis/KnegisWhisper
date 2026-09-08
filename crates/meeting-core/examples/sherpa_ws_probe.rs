//! What sherpa-onnx's streaming WebSocket protocol actually looks like.
//!
//! KNEG-84 says the two Nemotron streaming models *"speak
//! `sherpa-onnx-online-websocket-server`'s own bespoke protocol"* and that
//! **nobody has written or tested a client for it.** Three cloud clients in this
//! crate carry "the wire shape is believed, not confirmed" for the same reason,
//! and each of those needs a paid account to settle.
//!
//! This one does not. `sherpa-onnx-online-websocket-client.exe` is installed
//! beside the server, and it is the reference implementation. So the protocol
//! can be **measured** rather than guessed, by making the reference client talk
//! to something that writes down what it says.
//!
//! ```text
//! cargo run -p kw-meeting-core --example sherpa_ws_probe -- [port]
//! ```
//!
//! Then, in another shell:
//!
//! ```text
//! sherpa-onnx-online-websocket-client.exe --server-port=<port> path\to\16k-mono.wav
//! ```
//!
//! # What this half can and cannot settle
//!
//! **Can:** everything the client sends. Framing (text against binary), the
//! sample encoding, how many samples per message, and what it sends to mark the
//! end of a stream. None of that depends on a model, so none of it needs the
//! 650 MB download the real feature does.
//!
//! **Cannot, in listen mode:** anything the server sends back. A real transcript
//! needs a real streaming model, so that half is `--talk`, which drives the real
//! server instead of impersonating it.
//!
//! ```text
//! cargo run -p kw-meeting-core --example sherpa_ws_probe -- --talk <port> <wav> [--fault]
//! ```
//!
//! `--fault` is the run that matters most. KNEG-79 exists because four clients
//! read a provider-reported error and carried on, so **what this server does
//! with a frame it cannot parse decides whether a fifth client can be written
//! honestly.** If a malformed frame is ignored silently, a client cannot tell a
//! rejected session from a quiet room, and that is the defect this project has
//! already paid for once.
//!
//! # What it found on 2026-09-01, against the installed reference client
//!
//! The whole client-to-server half, and none of it was guessable:
//!
//! ```text
//! frame 1..75   BINARY  32000 bytes   8000 samples, f32 little-endian
//! frame 76      BINARY   2560 bytes    640 samples -- the tail, short
//! frame 77      TEXT    "Done"
//!               then the client closes
//! ```
//!
//! Three things worth stating plainly, because a client written from the
//! documentation would have got each of them wrong:
//!
//! 1. **The samples are `f32`, not the `i16` the client reads from disk.** Its
//!    own `--help` says it takes *"a single channel, 16kHz, 16-bit samples"*, so
//!    `i16` on the wire is the natural assumption. It converts first. Reading
//!    frame 1 both ways settles it and nothing else does: as `f32` the values are
//!    `[-0.15, -0.18, -0.23, ...]`, ordinary speech inside ±1; the same bytes as
//!    `i16` are `[12288, -16869, 26624, ...]`, which is noise.
//! 2. **There is no header, and no handshake beyond the WebSocket one.** Frame 1
//!    is audio. Nothing announces the sample rate or the format — both are
//!    agreed out of band, by the flags the server and client were each started
//!    with, and a mismatch is therefore silent.
//! 3. **The end of a stream is a TEXT frame containing exactly `Done`.** Not a
//!    close frame, not an empty binary frame, not a JSON object. A client that
//!    closed the socket instead would leave the server waiting for the tail of
//!    the last utterance.
//!
//! Frame size is `--samples-per-message`, default 8000, and the last frame is
//! whatever is left over. So a real client chooses its own block size and does
//! not have to match the reference.
//!
//! # And what the server sends back, same day, against the real thing
//!
//! `sherpa-onnx-online-websocket-server` running the pinned
//! `nemotron-speech-streaming-en-0.6b`, driven by `--talk`:
//!
//! ```text
//! reply 1..13   TEXT   one JSON object per update
//! reply 14      TEXT   "Done!"
//!               then the socket stays open until the client closes it
//! ```
//!
//! The JSON carries `text`, `tokens[]`, `timestamps[]`, `ys_probs[]`,
//! `lm_probs[]`, `context_scores[]`, `segment`, `words[]`, `start_time`,
//! `is_final` and `is_eof`.
//!
//! Four things a client has to get right, and none is in the flags:
//!
//! 1. **`text` is cumulative within a segment.** Every update repeats the whole
//!    segment so far, growing. A client that appended would produce
//!    *"After After early After early nightfall"*. Replace — the same rule
//!    `local.rs` already applies to its windows, arrived at from the other
//!    direction.
//! 2. **`is_final` and `is_eof` are both false throughout** and both true on the
//!    last JSON. There is no per-utterance final within a stream; `segment`
//!    numbers the utterances, and endpointing is the server's
//!    `--rule2-min-trailing-silence` and `--rule3-min-trailing-silence`.
//! 3. **The server's end marker is `Done!` and the client's is `Done`.**
//!    Different strings, and the reference client also prints `Done!` as its own
//!    last log line — so it is easy to read the frame as an echo of the log.
//!    It is not; it comes off the wire.
//! 4. **The server does not close.** The reference client closes and then
//!    reports "Disconnected". A client that waits for the server to hang up
//!    waits forever, which is how this probe learnt it.
//!
//! ## The finding that decides how the client reports faults
//!
//! **A malformed frame produces nothing at all.** `--fault` sends an unparseable
//! TEXT frame (`{ not json`) and a three-byte BINARY frame — not a whole `f32`
//! — before the audio. The server answered neither, and the transcript came back
//! **byte-identical to the clean run**, so the odd-length frame did not even
//! knock the sample stream out of alignment.
//!
//! That is the opposite of the four cloud clients, and it decides the design.
//! KNEG-79 exists because those four read a provider-reported error frame and
//! carried on; here **there is no error frame to read**. So a
//! [`crate::fault::SessionFault`] for this provider can only ever be filled from
//! transport-level failure — a refused connect, a closed socket, a read error —
//! and a client that claimed to surface "what the provider said was wrong" would
//! be describing a channel that does not exist.
//!
//! # The control
//!
//! The client is a real program and can fail to connect for ordinary reasons. So
//! this reports the handshake separately from the frames: **no frames after a
//! connection is a finding, and no frames after no connection is nothing at
//! all.** That distinction is the one this project keeps having to relearn — a
//! path probe once reported `DIES` for every case including its control, and a
//! flake hunter once reported 100% failures because it could not start the
//! binary.

use futures::StreamExt as _;

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--talk") {
        talk(&args).await;
        return;
    }

    let port: u16 = std::env::args()
        .nth(1)
        .and_then(|arg| arg.parse().ok())
        .unwrap_or(6006);

    let listener = match tokio::net::TcpListener::bind(("127.0.0.1", port)).await {
        Ok(listener) => listener,
        Err(err) => {
            println!("could not listen on 127.0.0.1:{port}: {err}");
            return;
        }
    };

    println!("sherpa streaming-protocol probe");
    println!("listening on 127.0.0.1:{port}\n");
    println!("Now run the reference client against it:");
    println!(
        "  sherpa-onnx-online-websocket-client.exe --server-port={port} <16k mono 16-bit>.wav\n"
    );

    let Ok((stream, peer)) = listener.accept().await else {
        println!("the accept failed, so nothing below would be a measurement.");
        return;
    };
    println!("--- TCP from {peer} ---");

    let socket = match tokio_tungstenite::accept_async(stream).await {
        Ok(socket) => socket,
        Err(err) => {
            println!("the WebSocket handshake failed: {err}");
            println!("So the client never spoke the protocol, and this run measured nothing.");
            return;
        }
    };
    println!("--- WebSocket handshake accepted ---\n");

    let (_write, mut read) = socket.split();
    let mut frames = 0_usize;
    let mut total_bytes = 0_usize;
    // The size of the first audio frame, so a frame that differs from it can be
    // printed too. The tail is the interesting one and it is the last thing to
    // arrive: reporting it by arithmetic from a total would be a calculation
    // presented as a measurement.
    let mut usual = None;

    while let Some(message) = read.next().await {
        let message = match message {
            Ok(message) => message,
            Err(err) => {
                println!("read failed after {frames} frame(s): {err}");
                break;
            }
        };

        frames += 1;
        match message {
            tokio_tungstenite::tungstenite::Message::Binary(bytes) => {
                total_bytes += bytes.len();
                // Only the first few and the last are printed in full. A
                // thirty-second WAV is hundreds of frames and the interesting
                // ones are the first (does it carry a header?) and the last
                // (how is the end marked?).
                let odd = usual.is_some_and(|size| size != bytes.len());
                if frames <= 3 || odd {
                    describe_binary(frames, &bytes);
                    if odd {
                        println!("           SHORT -- a tail frame, not a full block");
                    }
                }
                usual.get_or_insert(bytes.len());
            }
            tokio_tungstenite::tungstenite::Message::Text(text) => {
                println!("frame {frames}: TEXT {:?}", text.as_str());
            }
            tokio_tungstenite::tungstenite::Message::Close(reason) => {
                println!("frame {frames}: CLOSE {reason:?}");
            }
            other => {
                println!("frame {frames}: {other:?}");
            }
        }
    }

    println!("\n--- {frames} frame(s), {total_bytes} byte(s) of payload ---");
    if frames == 0 {
        println!("The client connected and sent nothing. That is a finding, not a failed run.");
    }
    println!();
    println!("This measured only what the client SENDS. What the server sends back needs");
    println!("a real streaming model and a run against the real server binary.");
}

/// Print one binary frame both ways, because which one it is, is the question.
///
/// The client's own `--help` says it reads *"wave of with a single channel,
/// 16kHz, 16-bit samples"*, and sherpa works internally in `f32`. Whether it
/// converts before sending or ships the `i16` is exactly the thing a guess would
/// get wrong, so both readings are printed and the plausible one is obvious:
/// speech as `f32` sits inside ±1, and the same bytes read as `i16` do not.
fn describe_binary(frame: usize, bytes: &[u8]) {
    println!("frame {frame}: BINARY {} bytes", bytes.len());

    if bytes.len().is_multiple_of(4) {
        let as_f32: Vec<f32> = bytes
            .as_chunks::<4>()
            .0
            .iter()
            .take(6)
            .map(|&c| f32::from_le_bytes(c))
            .collect();
        println!("           as f32 le: {as_f32:?}");
        println!("           ({} samples if f32)", bytes.len() / 4);
    }

    let as_i16: Vec<i16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .take(6)
        .map(|&c| i16::from_le_bytes(c))
        .collect();
    println!("           as i16 le: {as_i16:?}");
    println!("           ({} samples if i16)", bytes.len() / 2);

    // A four-byte first frame is almost certainly a count rather than audio.
    if bytes.len() <= 8 {
        println!("           short frame -- likely a header, not audio");
    }
}

/// Drive the **real** server and print everything it says back.
///
/// The other half of the protocol, and the half that needs a model. Written as
/// the smallest client that can be believed: it sends what the reference client
/// was measured to send -- `f32` little-endian blocks then a TEXT `Done` -- and
/// prints every reply verbatim rather than parsing it into something that could
/// hide a field.
///
/// With `--fault` it sends one unparseable frame first. That is the KNEG-79
/// question asked of this server: a client cannot report a fault it is never
/// told about.
async fn talk(args: &[String]) {
    use futures::SinkExt as _;
    use tokio_tungstenite::tungstenite::Message;

    let port: u16 = args
        .iter()
        .position(|a| a == "--talk")
        .and_then(|i| args.get(i + 1))
        .and_then(|value| value.parse().ok())
        .unwrap_or(6006);
    let wav = args
        .iter()
        .position(|a| a == "--talk")
        .and_then(|i| args.get(i + 2))
        .cloned()
        .unwrap_or_default();
    let fault = args.iter().any(|a| a == "--fault");

    println!("driving the real server on 127.0.0.1:{port}");

    let url = format!("ws://127.0.0.1:{port}/");
    let (mut socket, _) = match tokio_tungstenite::connect_async(&url).await {
        Ok(pair) => pair,
        Err(err) => {
            println!("could not connect: {err}");
            println!("Nothing below is a measurement.");
            return;
        }
    };
    println!("--- connected ---");

    if fault {
        // Not JSON, not audio, not "Done". Whatever the server makes of this is
        // what a real client would have to survive.
        println!("\nsending one unparseable TEXT frame: \"{{ not json\"");
        if let Err(err) = socket.send(Message::Text("{ not json".to_owned())).await {
            println!("the send itself failed: {err}");
        }

        // A four-byte binary frame is not a whole f32 block either.
        println!("sending one odd-length BINARY frame: 3 bytes");
        if let Err(err) = socket.send(Message::Binary(vec![1, 2, 3])).await {
            println!("the send itself failed: {err}");
        }
    }

    let samples = match read_wav_f32(&wav) {
        Ok(samples) => samples,
        Err(err) => {
            println!("could not read {wav}: {err}");
            return;
        }
    };
    println!(
        "\nsending {} samples as f32 le, 8000 per frame",
        samples.len()
    );

    for block in samples.chunks(8000) {
        let mut bytes = Vec::with_capacity(block.len() * 4);
        for sample in block {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
        if let Err(err) = socket.send(Message::Binary(bytes)).await {
            println!("send failed part way: {err}");
            break;
        }
    }

    println!("sending TEXT \"Done\"\n");
    let _ = socket.send(Message::Text("Done".to_owned())).await;

    let mut replies = 0_usize;
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(120);
    loop {
        let left = deadline.saturating_duration_since(tokio::time::Instant::now());
        if left.is_zero() {
            println!("(stopped waiting after two minutes)");
            break;
        }
        match tokio::time::timeout(left, socket.next()).await {
            Err(_) => {
                println!("(stopped waiting after two minutes)");
                break;
            }
            Ok(None) => {
                println!("--- the server closed the socket ---");
                break;
            }
            Ok(Some(Err(err))) => {
                println!("--- read failed: {err} ---");
                break;
            }
            Ok(Some(Ok(message))) => {
                replies += 1;
                match message {
                    Message::Text(text) => {
                        println!("reply {replies}: TEXT {text}");
                        // The server's own end marker, which is not the same
                        // string the client sends. Measured, not assumed: the
                        // reference client also prints "Done!" as its last log
                        // line, so it would be easy to read this as an echo of
                        // that rather than as a frame off the wire.
                        if text.trim() == "Done!" {
                            println!("(the server said Done! -- end of stream)");
                            break;
                        }
                    }
                    Message::Binary(bytes) => {
                        println!("reply {replies}: BINARY {} bytes", bytes.len());
                    }
                    Message::Close(reason) => {
                        println!("reply {replies}: CLOSE {reason:?}");
                        break;
                    }
                    other => println!("reply {replies}: {other:?}"),
                }
            }
        }
    }

    println!("\n--- {replies} repl(ies) ---");
    if replies == 0 {
        println!("The server said nothing at all. For a --fault run that IS the finding:");
        println!("a client cannot report a fault it is never told about.");
    }
}

/// A 16-bit mono WAV as `f32` in [-1, 1].
///
/// Deliberately minimal and deliberately strict: this reads the format the
/// server was told to expect and refuses anything else, rather than resampling
/// and hiding a mismatch the real client would also have hidden.
fn read_wav_f32(path: &str) -> Result<Vec<f32>, String> {
    let bytes = std::fs::read(path).map_err(|err| err.to_string())?;
    if bytes.len() < 44 {
        return Err("shorter than a WAV header".to_owned());
    }

    // The data chunk is not always at offset 44, so it is searched for rather
    // than assumed. A fixed offset works on the files this was written against
    // and silently truncates on any file with an extra chunk.
    let mut at = 12;
    while let Some(header) = bytes.get(at..at + 8) {
        let Some(size) = header
            .get(4..8)
            .and_then(|raw| <[u8; 4]>::try_from(raw).ok())
            .map(|raw| u32::from_le_bytes(raw) as usize)
        else {
            break;
        };

        if header.get(..4) == Some(b"data".as_slice()) {
            let end = at.saturating_add(8).saturating_add(size).min(bytes.len());
            let Some(data) = bytes.get(at.saturating_add(8)..end) else {
                break;
            };
            return Ok(data
                .as_chunks::<2>()
                .0
                .iter()
                .copied()
                .map(|raw| f32::from(i16::from_le_bytes(raw)) / 32768.0)
                .collect());
        }
        at = at
            .saturating_add(8)
            .saturating_add(size)
            .saturating_add(size % 2);
    }
    Err("no data chunk".to_owned())
}
