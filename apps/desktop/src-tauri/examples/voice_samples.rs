//! Offline extraction through the packaged application's bounded worker protocol.
use knegiswhisper_lib::voice_worker::{hash_file, run_program, Request};
use std::{path::Path, time::Duration};
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let [app, library, model, wav, turns_file] = args.as_slice() else {
        return Err("usage: voice_samples APP_EXE LIBRARY MODEL WAV DIARIZER_STDOUT".into());
    };
    let turns =
        kw_meeting_core::diarization::interpret(&std::fs::read_to_string(turns_file)?, "", true)?;
    let reader = hound::WavReader::open(wav)?;
    let duration = f64::from(reader.duration()) / f64::from(reader.spec().sample_rate);
    let request = Request {
        version: 1,
        library: library.into(),
        model: model.into(),
        wav: wav.into(),
        sha256: hash_file(Path::new(wav))?,
        duration,
        turns: turns
            .into_iter()
            .map(|t| kw_notes_core::known_voices::VoiceTurn {
                start_seconds: t.start_seconds,
                end_seconds: t.end_seconds,
                speaker: t.speaker,
            })
            .collect(),
    };
    let started = std::time::Instant::now();
    let samples = run_program(
        Path::new(app),
        &request,
        Duration::from_secs_f64((30.0 + duration * 0.1).clamp(30.0, 600.0)),
    )
    .await?;
    println!(
        "{}",
        serde_json::to_string(
            &serde_json::json!({"model_id":kw_notes_core::known_voices::MODEL_ID,"format":kw_notes_core::known_voices::FORMAT,"elapsed_seconds":started.elapsed().as_secs_f64(),"samples":samples})
        )?
    );
    Ok(())
}
