#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#[cfg(windows)]
mod windows;
#[cfg(windows)]
fn main() {
    match windows::run() {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            if !std::env::args().any(|a| a == "--quiet" || a == "--broker") {
                windows::show_error(&error.to_string());
            }
            std::process::exit(1);
        }
    }
}
#[cfg(not(windows))]
fn main() {
    eprintln!("The Windows uninstall helper is not supported on this platform.");
}
