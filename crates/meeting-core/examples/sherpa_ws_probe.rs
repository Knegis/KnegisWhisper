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
//! **Cannot:** anything the server sends back. A real transcript needs a real
//! streaming model. That half is a separate run against the real server, and
//! this probe deliberately does not pretend to cover it — it replies with
//! nothing and says so.
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
            .chunks_exact(4)
            .take(6)
            .filter_map(|c| c.try_into().ok().map(f32::from_le_bytes))
            .collect();
        println!("           as f32 le: {as_f32:?}");
        println!("           ({} samples if f32)", bytes.len() / 4);
    }

    let as_i16: Vec<i16> = bytes
        .chunks_exact(2)
        .take(6)
        .filter_map(|c| c.try_into().ok().map(i16::from_le_bytes))
        .collect();
    println!("           as i16 le: {as_i16:?}");
    println!("           ({} samples if i16)", bytes.len() / 2);

    // A four-byte first frame is almost certainly a count rather than audio.
    if bytes.len() <= 8 {
        println!("           short frame -- likely a header, not audio");
    }
}
