// Do not open a console window alongside the GUI on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() -> std::process::ExitCode {
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
