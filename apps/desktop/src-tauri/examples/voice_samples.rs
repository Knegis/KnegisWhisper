//! Offline evaluation using exactly the production extractor; never opens the app DB.
#[path = "../src/known_voices_native.rs"]
mod native;
use std::path::Path;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let [library, model, wav, turns_file] = args.as_slice() else {
        return Err("usage: voice_samples LIBRARY MODEL WAV DIARIZER_STDOUT".into());
    };
    let digest = kw_sidecar::Sha256Digest::parse(
        "357a834f702b80161e5b981182c038e18553c1f2ca752ed6cec2052365d4129b",
    )?;
    kw_sidecar::integrity::verify(Path::new(model), 29596978, &digest)?;
    let turns =
        kw_meeting_core::diarization::interpret(&std::fs::read_to_string(turns_file)?, "", true)?;
    let started = std::time::Instant::now();
    let samples = native::extract(Path::new(library), Path::new(model), Path::new(wav), &turns)?;
    println!(
        "{}",
        serde_json::to_string(
            &serde_json::json!({"model_id":kw_notes_core::known_voices::MODEL_ID,"format":kw_notes_core::known_voices::FORMAT,"elapsed_seconds":started.elapsed().as_secs_f64(),"samples":samples})
        )?
    );
    Ok(())
}
