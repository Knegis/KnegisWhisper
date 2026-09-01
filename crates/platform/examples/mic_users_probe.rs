//! Which applications are holding the microphone, right now, on this machine.
//!
//! This exists to answer one question before anything is built on top of it:
//! **can an audio-session listener tell that Teams is in a call?** Teams runs
//! the call inside `ms-teams.exe` and spawns no child process, so the process
//! table cannot — measured twice on real hardware, 2026-08-28 and 2026-08-31.
//! KNEG-94 assumes the audio layer can. That is a hypothesis, and this is how it
//! becomes a measurement.
//!
//! ```text
//! cargo run -p kw-platform --example mic_users_probe
//! ```
//!
//! # The run that matters
//!
//! Four times, and the fourth is the whole question:
//!
//! 1. **Idle.** Nothing should be capturing. If something is, find out what
//!    before reading anything else here — a headset utility that holds the
//!    device permanently would make every later answer meaningless.
//! 2. **With this application dictating.** `knegiswhisper-desktop.exe` must
//!    appear. This is the control.
//! 3. **Teams open, not in a call.** Teams should *not* appear. If it does, the
//!    signal cannot separate the two states either and KNEG-94 is dead.
//! 4. **Teams in a real call.** `ms-teams.exe` must appear. Only the repo owner
//!    can produce this one.
//!
//! # The control runs itself
//!
//! "Nothing is capturing" and "the enumeration does not work" are identical from
//! here — an empty list either way — and this project has been caught by exactly
//! that shape twice in a week: a path probe that reported `DIES` for every case
//! including its control, and a flake hunter that reported 100% failures because
//! it could not start the binary at all. **A probe that finds nothing and has
//! never found anything is not evidence of absence.**
//!
//! So this probe does not wait for run 2. It opens the microphone itself, with
//! the same `kw_audio_core::Capture` a dictation uses, and enumerates again. If
//! its own capture does not appear it says so, and every run afterwards carries
//! a control rather than depending on somebody remembering to do run 2 first.
//!
//! It also prints [`MicrophoneSupport`], because an empty list under `Failed` or
//! under zero endpoints is not an answer either, and those two are different
//! failures.

use kw_platform::microphone::{self, MicrophoneSupport};

fn main() {
    println!("microphone-activity probe\n");

    let names = microphone::process_names();
    println!("process table: {} entries", names.len());

    let found = microphone::capturing(&|pid| names.get(&pid).cloned());

    match &found.support {
        MicrophoneSupport::Available {
            mechanism,
            endpoints,
        } => {
            println!("mechanism:     {mechanism}");
            println!("endpoints:     {endpoints} active capture device(s)");
        }
        MicrophoneSupport::Failed { mechanism, reason } => {
            println!("mechanism:     {mechanism}");
            println!("FAILED:        {reason}");
        }
        MicrophoneSupport::Unsupported { reason } => {
            println!("unsupported:   {reason}");
        }
    }

    println!();
    if found.processes.is_empty() {
        if found.support.absence_is_evidence() {
            println!("nothing is capturing.");
            println!();
            println!("That is a real answer: at least one capture endpoint was examined.");
        } else {
            println!("nothing is capturing -- AND THAT IS NOT AN ANSWER.");
            println!();
            println!("Either the enumeration failed or there is no capture device to watch,");
            println!("so this run says nothing about who is or is not in a call. Fix that");
            println!("before drawing any conclusion from an empty list.");
        }
    } else {
        println!("capturing now:");
        for name in &found.processes {
            let mine = name.contains("knegiswhisper");
            println!(
                "  {name}{}",
                if mine { "   <- this application" } else { "" }
            );
        }
    }

    control(&names);

    println!();
    println!("Now run it again with Teams open and idle (it must NOT be listed above),");
    println!("and once more during a real Teams call (it must). Those two together are");
    println!("KNEG-94, and nothing else settles it.");
}

/// Open the microphone here, and check the enumeration notices.
///
/// The whole reason this probe can be believed. Without it a clean "nothing is
/// capturing" is indistinguishable from an enumeration that never worked, and
/// the second is the one somebody would act on by mistake.
fn control(names: &std::collections::HashMap<u32, String>) {
    println!();
    println!("--- control: opening the microphone from this process ---");

    let capture = match kw_audio_core::capture::Capture::start(None) {
        Ok(capture) => capture,
        Err(err) => {
            println!("could not open the microphone: {err}");
            println!("The control did not run, so the reading above is unverified.");
            return;
        }
    };

    // A capture session appears when the stream actually starts, which is not
    // the instant `start` returns. Polled rather than slept once: a fixed sleep
    // long enough to be reliable is longer than this needs on most machines.
    let mut seen = None;
    for _ in 0..25 {
        std::thread::sleep(std::time::Duration::from_millis(80));
        let found = microphone::capturing(&|pid| names.get(&pid).cloned());
        if let Some(mine) = found
            .processes
            .iter()
            .find(|name| name.contains("mic_users_probe") || name.contains("knegiswhisper"))
        {
            seen = Some(mine.clone());
            break;
        }
    }

    drop(capture);

    match seen {
        Some(name) => {
            println!("PASS: this process appeared as {name} while its stream was open.");
            println!("The enumeration sees a real capture, so an empty list above is a real");
            println!("absence rather than a broken probe.");
        }
        None => {
            println!("FAIL: this process opened the microphone and never appeared.");
            println!("The enumeration is not working and nothing printed above means");
            println!("anything. Do not read an empty list as an absence.");
        }
    }
}
