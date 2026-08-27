//! Does the meeting detector recognise anything on this machine?
//!
//! ```text
//! cargo run -p kw-platform --example meeting_detect_probe
//! ```
//!
//! The name table in [`kw_platform::meeting_detect`] is written from knowledge
//! rather than ported from anything in this checkout — upstream's
//! `meetingProcessDetector.js` is not here to read. So until somebody has run
//! this with a real call in progress, every row of that table is a hypothesis.
//! This is what makes it a measurement.
//!
//! Run it three times and record what each says:
//!
//! 1. With no meeting application open. It should find nothing.
//! 2. With the client open but no call. It should find the application and say
//!    **not in a call** — that distinction is what stops a prompt appearing
//!    every time Teams sits in the tray.
//! 3. During a real call. It should find it and say **in a call**.
//!
//! If (2) and (3) look the same, the in-call process name is wrong, and the
//! candidate list printed at the end is where to look for the right one.

use kw_platform::meeting_detect::{self, Lists};

fn main() {
    println!("supported: {}", meeting_detect::supported());

    let running = meeting_detect::running();
    println!("{} processes visible\n", running.len());

    let found = meeting_detect::detect(&running, &Lists::default());
    if found.is_empty() {
        println!("no meeting application recognised.");
    } else {
        for detected in &found {
            println!(
                "{:<18} {:<20} via {:<20} {}",
                detected.provider,
                detected.name,
                detected.evidence,
                if detected.in_call {
                    "IN A CALL"
                } else {
                    "open, not in a call"
                }
            );
        }
    }

    // Everything that looks like it might be one, so a wrong table can be
    // corrected from the output rather than by guessing again. Substring
    // matching here on purpose -- `detect` is deliberately exact, and this is the
    // one place where casting a wider net is useful.
    const HINTS: [&str; 9] = [
        "zoom", "team", "webex", "cisco", "meet", "cpt", "cap", "face", "skype",
    ];
    let mut candidates: Vec<&String> = running
        .iter()
        .filter(|name| {
            let lower = name.to_lowercase();
            HINTS.iter().any(|hint| lower.contains(hint))
        })
        .collect();
    candidates.sort_unstable();
    candidates.dedup();

    println!("\nprocesses whose names look meeting-ish, matched or not:");
    if candidates.is_empty() {
        println!("  (none)");
    } else {
        for name in candidates {
            let matched = found.iter().any(|d| d.evidence.eq_ignore_ascii_case(name));
            println!("  {name}{}", if matched { "   <- matched" } else { "" });
        }
    }

    println!(
        "\nRecord the result in docs/PLATFORM_SUPPORT.md. A name in the list above that is not \
         marked `<- matched` during a real call is a name the table is missing."
    );
}
