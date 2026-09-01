//! Which characters in a path actually break the native ASR binaries.
//!
//! `kw_storage::paths` relocates the model cache away from the user's own
//! directory when that directory has a name the native binaries cannot cope
//! with. Until 2026-08-31 the rule was "non-ASCII **or space**", applied to the
//! binaries' own directory as well as to the files they open, and it had been
//! carried from upstream without ever being run. Three quarters of it was not
//! true.
//!
//! This is what settled it, kept so the next person does not have to take the
//! sentence on trust, and so it can be re-run on a machine this project has
//! never seen — a real non-ASCII Windows account being the obvious one. Nothing
//! here is a unit test: it needs a real `whisper-server` and a real model,
//! neither of which exists in CI.
//!
//! ```text
//! cargo run -p kw-storage --example native_path_probe
//! ```
//!
//! It finds the installed sidecars and models through `kw_storage::paths`
//! itself, copies them into directories whose names carry one awkward character
//! each, starts the server against each, and reports whether it ever answered.
//! It writes only under the system temporary directory and deletes what it
//! wrote.
//!
//! **Starting the server is the measurement, and `--help` is not.**
//! `whisper-server --help` prints its usage *before* touching the model, so a
//! probe built on it reports success for a model that cannot be loaded. The
//! first version of this file did exactly that, and said `DIES` for the control
//! as well, which is the only reason it was caught.
//!
//! # What it found on 2026-08-31, on Windows 11
//!
//! ```text
//! model file, path with a space       whisper: loads    sherpa: transcribes
//! model file, non-ASCII path          whisper: dies     sherpa: Errors in config!
//! audio input, path with a space                        sherpa: transcribes
//! audio input, non-ASCII path                           sherpa: Failed to read
//! binary's own directory, space       whisper: loads    sherpa: transcribes
//! binary's own directory, non-ASCII   whisper: loads    sherpa: transcribes
//! ```
//!
//! Both failures exit 127. `sherpa-onnx-offline` is measured by hand rather
//! than here because its model set is ~600 MB and copying it three times is not
//! something a probe should do without being asked; the commands are printed.
//! `llama-server` and `ffmpeg` were checked the same way and also run from a
//! directory named `Håkan Test`, which is why
//! [`kw_storage::paths::sidecar_dir`] stopped relocating at all.

use std::io::ErrorKind;
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

/// One awkward name, and what it is meant to test.
const NAMES: [(&str, &str); 3] = [
    ("plain", "ascii, no space -- the control"),
    ("with space", "ascii, one space"),
    ("nonascii\u{e5}", "one non-ASCII character"),
];

/// Ports to try, one per case, well away from the application's own.
const FIRST_PORT: u16 = 18400;

/// How long a load is given before it is called dead.
///
/// `ggml-tiny` loads in about a second on the machine this was written on, and
/// a load that is going to fail has already exited by then. Thirty seconds is
/// slack for a cold cache and a larger model, not a guess at the real time.
const PATIENCE: Duration = Duration::from_secs(30);

fn main() {
    let sidecars = kw_storage::paths::sidecar_dir().path;
    let models = kw_storage::paths::model_cache_dir().path;

    println!("sidecars: {}", sidecars.display());
    println!("models:   {}", models.display());
    println!();

    let scratch = std::env::temp_dir().join("kw-native-path-probe");
    let _ = std::fs::remove_dir_all(&scratch);
    if let Err(err) = std::fs::create_dir_all(&scratch) {
        println!("cannot write to {}: {err}", scratch.display());
        return;
    }

    whisper(&sidecars, &models, &scratch);
    println!();
    sherpa(&sidecars, &models);

    let _ = std::fs::remove_dir_all(&scratch);
}

/// `whisper-server`, which reads the model by path and takes audio over HTTP.
fn whisper(sidecars: &Path, models: &Path, scratch: &Path) {
    let original_dir = sidecars.join("whisper");
    let binary = original_dir.join(executable("whisper-server"));
    let model = models.join("ggml-tiny.bin");

    if !binary.is_file() || !model.is_file() {
        println!("whisper-server or ggml-tiny.bin is not installed here, skipped.");
        println!("  looked for {}", binary.display());
        println!("  and        {}", model.display());
        return;
    }

    let mut port = FIRST_PORT;

    println!("whisper-server, varying the MODEL's path:");
    for (name, what) in NAMES {
        port += 1;
        match place_file(scratch, name, &model) {
            Some(copied) => run(name, what, &binary, &copied, port),
            None => println!("  {name:<14} could not be staged"),
        }
    }

    println!("whisper-server, varying the BINARY's own directory:");
    for (name, what) in NAMES {
        port += 1;
        match place_dir(scratch, name, &original_dir) {
            Some(dir) => run(
                name,
                what,
                &dir.join(executable("whisper-server")),
                &model,
                port,
            ),
            None => println!("  {name:<14} could not be staged"),
        }
    }
}

/// The commands that measure the other binary, printed rather than run.
fn sherpa(sidecars: &Path, models: &Path) {
    let binary = sidecars
        .join("sherpa")
        .join(executable("sherpa-onnx-offline"));
    if !binary.is_file() {
        println!("sherpa-onnx-offline is not installed here, so its half is not printed.");
        return;
    }

    println!("sherpa-onnx-offline: measured by hand, because its model set is ~600 MB");
    println!("and a probe should not copy that three times without being asked.");
    println!();
    println!("  copy {}", models.join("sherpa").display());
    println!("  to a directory whose name holds a non-ASCII character, then:");
    println!();
    println!(
        "  {} --tokens=DIR/tokens.txt --encoder=DIR/encoder.onnx \\",
        binary.display()
    );
    println!("      --decoder=DIR/decoder.onnx --joiner=DIR/joiner.onnx AUDIO.wav");
    println!();
    println!("  non-ASCII in DIR      -> Errors in config!   (exit 127)");
    println!("  non-ASCII in AUDIO    -> Failed to read '...' (exit 127)");
    println!("  a space in either     -> transcribes");
}

/// Start the server against `model` and report whether it ever answered.
fn run(name: &str, what: &str, binary: &Path, model: &Path, port: u16) {
    let child = Command::new(binary)
        .arg("--model")
        .arg(model)
        .arg("--host")
        .arg("127.0.0.1")
        .arg("--port")
        .arg(port.to_string())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();

    let Ok(mut child) = child else {
        println!("  {name:<14} could not start");
        return;
    };

    let answered = wait_for_port(&mut child, port);
    let _ = child.kill();
    let output = child.wait_with_output().ok();

    if answered {
        println!("  {name:<14} LOADS   ({what})");
        return;
    }

    println!("  {name:<14} DIES    ({what})");
    if let Some(output) = output {
        let text = String::from_utf8_lossy(&output.stdout);
        let last = text.lines().rev().find(|l| !l.trim().is_empty());
        if let Some(last) = last {
            // Truncated: the interesting part is which call it stopped at, and
            // the paths in it are long enough to hide that.
            let shown: String = last.trim().chars().take(90).collect();
            println!("  {:<14} stopped after: {shown}", "");
        }
    }
}

/// Poll the port until something accepts, the child exits, or patience runs out.
///
/// Polling rather than sleeping a fixed time: a load that works answers in about
/// a second and a load that fails exits without ever binding, so both answers
/// arrive as soon as they are true.
fn wait_for_port(child: &mut Child, port: u16) -> bool {
    let address = SocketAddr::from(([127, 0, 0, 1], port));
    let deadline = std::time::Instant::now() + PATIENCE;

    while std::time::Instant::now() < deadline {
        if TcpStream::connect_timeout(&address, Duration::from_millis(200)).is_ok() {
            return true;
        }
        match child.try_wait() {
            // Gone without ever binding.
            Ok(Some(_)) => return false,
            Ok(None) => {}
            Err(err) if err.kind() == ErrorKind::InvalidInput => return false,
            Err(_) => return false,
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    false
}

/// Copy one file into `scratch/<name>/` and return where it landed.
fn place_file(scratch: &Path, name: &str, file: &Path) -> Option<PathBuf> {
    let dir = scratch.join(name);
    std::fs::create_dir_all(&dir).ok()?;
    let leaf = dir.join(file.file_name()?);
    std::fs::copy(file, &leaf).ok()?;
    Some(leaf)
}

/// Copy a whole directory into `scratch/<name>-bin/` and return it.
///
/// Files only, one level deep, which is what a sidecar directory is — and the
/// sibling DLLs are the point of this case.
fn place_dir(scratch: &Path, name: &str, from: &Path) -> Option<PathBuf> {
    let dir = scratch.join(format!("{name}-bin"));
    std::fs::create_dir_all(&dir).ok()?;
    for entry in std::fs::read_dir(from).ok()?.flatten() {
        if entry.path().is_file() {
            let _ = std::fs::copy(entry.path(), dir.join(entry.file_name()));
        }
    }
    Some(dir)
}

/// Platform's name for an executable.
fn executable(stem: &str) -> String {
    if cfg!(windows) {
        format!("{stem}.exe")
    } else {
        stem.to_owned()
    }
}
