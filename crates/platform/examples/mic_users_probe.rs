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
//! So this probe does not wait for run 2. It runs its own control, and the
//! control is deliberately **cross-process**: it launches a second copy of
//! itself with `--hold`, which opens the microphone and waits, and then checks
//! the enumeration lists that other process.
//!
//! Seeing *itself* capture would have been the easy version and it proves less
//! than it looks. An audio session belongs to a process, and the question Task 2
//! actually asks is whether this code can see a session belonging to **somebody
//! else** — Teams. That those are the same mechanism is true and is an argument,
//! and an argument is what a measurement replaces.
//!
//! It also prints [`MicrophoneSupport`], because an empty list under `Failed` or
//! under zero endpoints is not an answer either, and those two are different
//! failures.
//!
//! # And it prints which meeting applications are open
//!
//! Added after the first real run, which reported "nothing is capturing" and a
//! passing control — and was still unreadable, because nothing in it said
//! whether Teams had been open at the time. Run 3 and "Teams was not running"
//! print the same words and mean opposite things: one is the measurement, the
//! other is no measurement at all.
//!
//! The same shape as the control, one level up. A negative is only worth what
//! the positive beside it is worth, and "Teams is open" is the positive that
//! makes "Teams is not capturing" mean something.

use kw_platform::meeting_detect;
use kw_platform::microphone::{self, MicrophoneSupport};

fn main() {
    // `--hold <seconds>`: open the microphone and wait. This is the other half
    // of the control -- the parent launches a copy of itself in this mode so it
    // has a *different* process to look for.
    let args: Vec<String> = std::env::args().collect();
    if let Some(seconds) = args
        .iter()
        .position(|a| a == "--hold")
        .and_then(|i| args.get(i + 1).and_then(|value| value.parse::<u64>().ok()))
    {
        hold(seconds);
        return;
    }

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

    // Which meeting applications are open, read off the same process table.
    // `detect` is reused rather than a name matched by hand here, so this line
    // reports what the real detector would report and not a second opinion that
    // could drift from it.
    let running: Vec<String> = names.values().cloned().collect();
    let open = meeting_detect::detect(&running, &meeting_detect::Lists::default());
    println!();
    if open.is_empty() {
        println!("no meeting application is running.");
        println!();
        println!("So an empty capture list above is the idle baseline and NOT the Teams");
        println!("comparison. Open Teams before reading it as \"Teams is not capturing\".");
    } else {
        println!("meeting applications open:");
        for app in &open {
            let capturing = found
                .processes
                .iter()
                .any(|name| name.eq_ignore_ascii_case(&app.evidence));
            println!(
                "  {} (matched {}) {}",
                app.name,
                app.evidence,
                if capturing {
                    "-- AND HOLDING THE MICROPHONE"
                } else {
                    "-- not capturing"
                }
            );
        }
    }

    control();

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
fn control() {
    println!();
    println!("--- control: a SECOND process opens the microphone ---");

    let Ok(exe) = std::env::current_exe() else {
        println!("could not find this executable, so the control did not run.");
        return;
    };

    let child = std::process::Command::new(&exe)
        .arg("--hold")
        .arg("12")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();

    let Ok(mut child) = child else {
        println!("could not start a second copy, so the control did not run.");
        return;
    };
    println!("started pid {} holding the microphone", child.id());

    // Polled rather than slept once: the session appears when the stream
    // actually starts, which is some way after the process does, and a fixed
    // sleep long enough to be reliable is longer than this needs.
    //
    // The process table is rebuilt each time. The child did not exist when the
    // reading above was taken, so reusing that snapshot would fail to resolve
    // its pid and report a false negative -- the control failing for a reason
    // that has nothing to do with what it is testing.
    let mut seen = None;
    for _ in 0..40 {
        std::thread::sleep(std::time::Duration::from_millis(100));
        let names = microphone::process_names();
        let found = microphone::capturing(&|pid| names.get(&pid).cloned());
        if let Some(other) = found
            .processes
            .iter()
            .find(|name| name.contains("mic_users_probe"))
        {
            seen = Some(other.clone());
            break;
        }
    }

    let _ = child.kill();
    let _ = child.wait();

    match seen {
        Some(name) => {
            println!("PASS: a different process appeared as {name} while its stream was open.");
            println!("The enumeration sees another process capturing, which is the property");
            println!("Teams detection needs. An empty list above is a real absence.");
        }
        None => {
            println!("FAIL: a second process held the microphone and never appeared.");
            println!("The enumeration cannot see other processes, so nothing printed above");
            println!("means anything and KNEG-94 cannot be built on it.");
        }
    }
}

/// Open the microphone and wait, so the parent has something to look for.
///
/// Deliberately silent: the parent reads nothing from it, and a child writing to
/// a shared console would interleave with the measurement.
fn hold(seconds: u64) {
    let Ok(capture) = kw_audio_core::capture::Capture::start(None) else {
        return;
    };
    std::thread::sleep(std::time::Duration::from_secs(seconds));
    drop(capture);
}
