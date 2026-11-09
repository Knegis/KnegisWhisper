// Do not open a console window alongside the GUI on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() -> std::process::ExitCode {
    if std::env::args().nth(1).as_deref() == Some("--known-voices-worker") {
        return knegiswhisper_lib::voice_worker::worker_main();
    }
    match knegiswhisper_lib::run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(err) => {
            // The GUI may never have appeared, so stderr is the only channel
            // we can rely on here.
            eprintln!("KnegisWhisper failed to start: {err:#}");
            std::process::ExitCode::FAILURE
        }
    }
}
