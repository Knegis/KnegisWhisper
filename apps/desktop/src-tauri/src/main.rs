// Do not open a console window alongside the GUI on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() -> std::process::ExitCode {
    if std::env::args().nth(1).as_deref() == Some("--known-voices-worker") {
        return knegiswhisper_lib::voice_worker::worker_main();
    }
    match knegiswhisper_lib::run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(err) => {
            knegiswhisper_lib::startup_failure::report(err.as_ref());
            std::process::ExitCode::FAILURE
        }
    }
}
