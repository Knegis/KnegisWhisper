//! Install a registry-pinned model into an explicitly supplied disposable directory.
use kw_network_policy::NetworkPolicy;
use kw_provider_registry::registry;
use kw_transcription_core::sherpa_models::{install, Progress};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    let id = args
        .get(1)
        .ok_or("usage: pinned_model_install MODEL_ID DIRECTORY")?;
    let root = std::path::PathBuf::from(args.get(2).ok_or("missing directory")?);
    let model = registry()?
        .local_asr
        .sherpa
        .iter()
        .find(|m| &m.id == id)
        .ok_or("unknown model")?;
    let mut last = 0;
    let result = install(&NetworkPolicy::new()?, model, &root, &mut |p| {
        if let Progress::Downloading { written, total } = p {
            if written / 50_000_000 > last {
                last = written / 50_000_000;
                eprintln!("{written}/{total:?}");
            }
        }
    })
    .await?;
    println!(
        "{}: {} verified downloaded bytes; {}",
        result.id,
        result.bytes,
        result.dir.display()
    );
    Ok(())
}
