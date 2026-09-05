//! What does `sherpa-onnx-offline` actually need?
//!
//! ```text
//! cargo run -p kw-transcription-core --example sherpa_probe
//! ```
//!
//! Following the precedent of `system_audio_probe`, `keyhook_probe` and
//! `meeting_detect_probe` — every one of which changed a design once it was
//! run — this installs the real pinned `sherpa-onnx` CLI, downloads one real
//! Parakeet model archive, and prints what is actually there: the directory
//! layout inside the extracted model, `sherpa-onnx-offline --help`'s real
//! flag names, and (if the first two make an invocation obvious) a real
//! transcript of the same fixture `local_dictation.rs` uses for whisper, so
//! the two can be compared on the same input.
//!
//! `docs/BUILD_PLAN.md`'s chunk B4 explains why this could not be written
//! from the sherpa-onnx release page: an extracted model is a directory of
//! encoder/decoder/joiner/tokens files whose layout differs between the
//! Parakeet and Nemotron families, and the CLI flags follow the layout.
//! This is the thing that finds out, for real, rather than guessing.
//!
//! Downloads roughly 25 MB (the Windows CLI) plus ~630 MB (one model
//! archive) over the real network. Not `#[ignore]`d because it is not a
//! `cargo test` target at all -- examples are never picked up by `cargo
//! test --workspace`, which is the whole reason this lives here rather than
//! in a test module.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;
use std::process::Command;
use std::time::Instant;

use kw_network_policy::{NetworkPolicy, RequestPurpose};
use kw_sidecar::{ArchiveKind, Catalog};

/// The same synthesised fixture `local_dictation.rs` transcribes with
/// whisper: "the quick brown fox jumps over the lazy dog", 6.3s at 16 kHz
/// mono. Reused here so sherpa's output can be compared against a known-good
/// whisper transcript on the *same* audio rather than on nothing.
const SPEECH: &[u8] = include_bytes!("../tests/fixtures/speech-16k-mono.wav");

/// Which sherpa model to probe. Parakeet Unified EN 0.6B: English-only and
/// non-streaming, which matches the offline CLI and the English fixture —
/// the most direct comparison available. The brief calls for the Parakeet
/// models first; this is the smaller of the two (631 declared MB against
/// Parakeet TDT's 680).
const MODEL_ID: &str = "parakeet-tdt-0.6b-v3";

#[tokio::main]
async fn main() {
    println!("=== sherpa-onnx-offline probe ===\n");

    let catalog = Catalog::shipped().expect("the shipped sidecar catalogue parses");
    let (spec, asset) = match catalog.resolve("sherpa") {
        Ok(pair) => pair,
        Err(err) => {
            println!("sherpa is not installable on this platform: {err}");
            println!("This is the honest finding for this run, not a bug to route around.");
            return;
        }
    };

    let root = tempfile::tempdir().expect("a temp dir for the probe's install");
    let policy = NetworkPolicy::new().expect("network policy");

    // ---------------------------------------------------------------- 1. CLI
    println!("--- installing the sherpa-onnx sidecar ---");
    println!("source: {}", asset.url);
    let installed = kw_sidecar::install(&policy, spec, asset, root.path(), &mut |p| {
        println!("  {p:?}");
    })
    .await
    .expect("the pinned sherpa asset should install");
    println!(
        "installed {} file(s); executable at {}",
        installed.files,
        installed.executable.display()
    );

    let bin_dir = installed
        .executable
        .parent()
        .expect("the executable has a parent directory")
        .to_path_buf();
    println!("\nreal contents of the installed bin/ directory (the whole-bin/ claim, measured):");
    print_tree(&bin_dir, 0);

    // ------------------------------------------------------------- 2. --help
    println!("\n--- sherpa-onnx-offline --help ---");
    match Command::new(&installed.executable).arg("--help").output() {
        Ok(out) => {
            println!("exit: {:?}", out.status.code());
            let stdout = String::from_utf8_lossy(&out.stdout);
            let stderr = String::from_utf8_lossy(&out.stderr);
            if !stdout.trim().is_empty() {
                println!("stdout:\n{stdout}");
            }
            if !stderr.trim().is_empty() {
                println!("stderr:\n{stderr}");
            }
        }
        Err(err) => println!("could not run --help: {err}"),
    }

    // -------------------------------------------------------------- 3. model
    let registry = kw_provider_registry::registry().expect("the shipped registry parses");
    let model = registry
        .local_asr
        .sherpa
        .iter()
        .find(|m| m.id == MODEL_ID)
        .expect("the probed model id is in the registry");

    println!("\n--- downloading {} ---", model.name);
    println!("source: {}", model.download_url);
    println!("declared size: {:?} MB", model.size_mb);

    // The probe uses the same three pins the shipped path does, rather than a
    // weaker download: it is the thing that finds out what a real install
    // meets, and an unbounded probe would find out about a different code path
    // than the one users get. `MissingPin` here would mean the registry cannot
    // install this model at all, which is worth failing loudly on.
    let expected_size = model
        .expected_size_bytes
        .expect("the probed model carries a measured compressed size");
    let extracted_limit = model
        .max_extracted_size_bytes
        .expect("the probed model carries an extracted-size ceiling");
    println!("pinned size: {expected_size} bytes, extraction ceiling: {extracted_limit} bytes");

    let archive = root.path().join("model.tar.bz2");
    let mut last_reported = 0_u64;
    policy
        .download_to_file_bounded(
            RequestPurpose::UserTriggeredModelDownload,
            &model.download_url,
            &archive,
            expected_size,
            &mut |written, total| {
                if written.saturating_sub(last_reported) >= 32 * 1024 * 1024 {
                    println!(
                        "  {} MB / {:?}",
                        written / (1024 * 1024),
                        total.map(|t| t / (1024 * 1024))
                    );
                    last_reported = written;
                }
            },
        )
        .await
        .expect("the model archive should download");

    let downloaded_bytes = std::fs::metadata(&archive)
        .expect("the archive exists")
        .len();
    println!("downloaded {} bytes", downloaded_bytes);

    let models_dir = root.path().join("models");
    std::fs::create_dir_all(&models_dir).expect("create the extraction directory");

    // Empty payload_dir: unlike the sidecar binary (which strips one
    // directory to flatten sibling libraries beside the executable), the
    // *whole* internal structure of a model archive is the measurement, so
    // nothing here is stripped.
    let files = kw_sidecar::extract_with_limit(
        &archive,
        ArchiveKind::TarBz2,
        "",
        "",
        &models_dir,
        extracted_limit,
    )
    .expect("the model archive should extract as tar.bz2 within its pinned ceiling");
    println!("extracted {files} file(s)");

    let model_dir = models_dir.join(&model.extract_dir);
    println!(
        "\nreal directory layout under {} (this is the measurement):",
        model_dir.display()
    );
    print_tree(&model_dir, 0);

    // ---------------------------------------------------- 4. try a transcript
    println!("\n--- attempting a transcription ---");
    let wav_path = root.path().join("fixture.wav");
    std::fs::write(&wav_path, SPEECH).expect("write the fixture wav");

    let files_in_model = list_files(&model_dir);
    let find = |needle: &str| -> Option<&String> {
        files_in_model
            .iter()
            .find(|f| f.to_lowercase().contains(needle))
    };

    let encoder = find("encoder");
    let decoder = find("decoder");
    let joiner = find("joiner");
    let tokens = find("tokens.txt");

    println!("discovered by name:");
    println!("  encoder: {encoder:?}");
    println!("  decoder: {decoder:?}");
    println!("  joiner:  {joiner:?}");
    println!("  tokens:  {tokens:?}");

    let (Some(encoder), Some(decoder), Some(tokens)) = (encoder, decoder, tokens) else {
        println!(
            "\ncould not find an encoder/decoder/tokens triple by name. The measured layout \
             above is the finding: whatever naming convention this model actually uses, it is \
             not one of the ones this probe looked for."
        );
        return;
    };

    let mut args = vec![
        format!("--tokens={tokens}"),
        format!("--encoder={encoder}"),
        format!("--decoder={decoder}"),
    ];
    if let Some(joiner) = joiner {
        args.push(format!("--joiner={joiner}"));
    }
    args.push("--num-threads=1".to_owned());
    args.push(wav_path.display().to_string());

    println!(
        "\ninvoking:\n  {} {}",
        installed.executable.display(),
        args.join(" ")
    );

    let start = Instant::now();
    let output = Command::new(&installed.executable).args(&args).output();
    let elapsed = start.elapsed();

    match output {
        Ok(out) => {
            println!("\nexit: {:?}  (took {elapsed:?})", out.status.code());
            let stdout = String::from_utf8_lossy(&out.stdout);
            let stderr = String::from_utf8_lossy(&out.stderr);
            println!("--- stdout ---\n{stdout}");
            println!("--- stderr (tail) ---\n{}", tail(&stderr, 2000));

            println!(
                "\nwhisper's transcript of the same fixture (from local_dictation.rs) is: \
                 \"The quick brown fox jumps over the lazy dog.\""
            );
            println!(
                "\nRecord this run's real invocation, the real directory layout above, and \
                 whether the transcript is usable in docs/PLATFORM_SUPPORT.md."
            );
        }
        Err(err) => println!("could not run sherpa-onnx-offline: {err}"),
    }
}

/// Every file under `dir`, recursively, as strings usable as a CLI argument.
fn list_files(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    collect_files(dir, &mut out);
    out
}

fn collect_files(dir: &Path, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_files(&path, out);
        } else {
            out.push(path.display().to_string());
        }
    }
}

/// Print a directory tree with file sizes, so "extracts the whole bin/
/// directory" and "here is the model's real layout" are both things this
/// output actually shows rather than states.
fn print_tree(dir: &Path, depth: usize) {
    let Ok(mut entries) = std::fs::read_dir(dir).map(|e| e.flatten().collect::<Vec<_>>()) else {
        return;
    };
    entries.sort_by_key(std::fs::DirEntry::file_name);

    for entry in entries {
        let path = entry.path();
        let indent = "  ".repeat(depth);
        if path.is_dir() {
            println!(
                "{indent}{}/",
                path.file_name().unwrap_or_default().to_string_lossy()
            );
            print_tree(&path, depth + 1);
        } else {
            let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
            println!(
                "{indent}{} ({size} bytes)",
                path.file_name().unwrap_or_default().to_string_lossy()
            );
        }
    }
}

/// The last `limit` characters of `s`, on a character boundary.
fn tail(s: &str, limit: usize) -> &str {
    if s.len() <= limit {
        return s;
    }
    let start = s.len() - limit;
    let boundary = (start..s.len())
        .find(|&i| s.is_char_boundary(i))
        .unwrap_or(start);
    s.get(boundary..).unwrap_or(s)
}
