//! What this machine says about system audio, and whether it really captures.
//!
//! An example rather than a test: it enumerates real devices and opens a real
//! stream, so its answer is about the machine it runs on and would be a
//! different answer in CI. That is also why it is worth having — the support
//! report is only useful if somebody has held it against real hardware, and
//! `SystemCapture` is the one part of `system_audio` that no unit test can
//! reach.
//!
//! Run with `cargo run -p kw-audio-core --example system_audio_probe`. Play
//! something first, or it will honestly report silence.
use std::time::Duration;

fn main() {
    let support = kw_audio_core::system_audio::support();
    println!("support:   {}", support.summary());
    println!("{support:#?}");

    if !support.is_available() {
        println!("\nnothing to capture from; stopping here.");
        return;
    }

    let capture = match kw_audio_core::SystemCapture::start() {
        Ok(capture) => capture,
        Err(err) => {
            println!("\ncould not start: {err}");
            return;
        }
    };
    println!("\ncapturing from {} for 2s…", capture.device());
    std::thread::sleep(Duration::from_secs(2));

    let track = capture.finish();
    let peak = track.samples.iter().fold(0.0_f32, |w, s| w.max(s.abs()));
    let seconds = if track.sample_rate == 0 || track.channels == 0 {
        0.0
    } else {
        track.samples.len() as f64 / f64::from(track.sample_rate) / f64::from(track.channels)
    };

    println!(
        "captured {} samples, {} ch at {} Hz ({seconds:.2}s), peak {peak:.4}",
        track.samples.len(),
        track.channels,
        track.sample_rate
    );

    // The distinction the mixer's report exists for: a stream that produced
    // nothing is a different fact from a stream that produced silence.
    if track.samples.is_empty() {
        println!("the stream produced NO samples — the callback never fired.");
    } else if peak == 0.0 {
        println!("the stream produced only silence — it works; nothing was playing.");
    } else {
        println!("the stream captured real audio.");
    }

    let mixed = kw_audio_core::mix(&[kw_audio_core::Track::mono(vec![0.0; 16_000]), track]);
    println!(
        "mixed with 1s of microphone silence: {} samples, {} sources, gain {:.3}",
        mixed.samples.len(),
        mixed.sources,
        mixed.gain
    );
}
