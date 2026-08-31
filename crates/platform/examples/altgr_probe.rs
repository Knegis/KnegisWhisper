//! Does the layout in front of you have AltGr, and which hotkeys does it eat?
//!
//! ```text
//! cargo run -p kw-platform --example altgr_probe
//! ```
//!
//! Two measurements that are easy to reason about wrongly and cheap to just
//! take.
//!
//! **What the layout says.** `VkKeyScanExW` reports, per character, which
//! modifiers type it. A character needing Control *and* Alt together is an
//! AltGr character, and a layout with one has an AltGr key. Locale names are
//! not consulted: "Swedish" is not the fact, `@` needing Control and Alt is.
//!
//! **Which chords collide.** Windows has no AltGr virtual key — the layout
//! synthesises left Control plus right Alt — so a modifier-only chord of
//! `Control+Alt` matches every AltGr keystroke. The table below is produced by
//! running the *real* chord matcher over the keys a real AltGr press sends, so
//! it is the shipped behaviour rather than a description of it.
//!
//! Run it on a US layout too if you have one: the first section should say
//! `Absent`, and the second should be unchanged, because which chords collide
//! is a property of the chord and not of the layout.

use kw_platform::keyboard_layout::{self, AltGr};
use kw_platform::keys::{altgr_collision, Chord};

fn main() {
    println!("-- the active keyboard layout --\n");
    match keyboard_layout::altgr() {
        AltGr::Present { example } => {
            println!("AltGr is present: {example:?} is typed with Control and Alt together.");
            println!("A chord that matches AltGr will fire whenever you type it.");
        }
        AltGr::Absent => {
            println!("AltGr is absent: no character this layout types needs Control and Alt.");
            println!("The collisions below cannot happen while this layout is active.");
        }
        AltGr::Unknown { why } => println!("Not determined: {why}"),
    }

    println!("\n-- which chords an AltGr keystroke fires --\n");
    let candidates = [
        "Control+Alt",
        "Control+Alt+Shift",
        "Control+Super",
        "Alt+Shift",
        "Control+Shift",
        "LeftControl+LeftAlt",
        "RightControl+Alt",
        "LeftControl+RightAlt",
        "AltGr+Super",
    ];

    for accelerator in candidates {
        let verdict = match Chord::parse(accelerator) {
            Ok(chord) => match altgr_collision(&chord) {
                Some(press) => format!("fires on: {press}"),
                None => "unaffected".to_owned(),
            },
            Err(err) => format!("not a chord: {err}"),
        };
        println!("  {accelerator:<22} {verdict}");
    }

    println!(
        "\nNote what is *not* here: `LeftControl+LeftAlt` is unaffected and would be the way out,\n\
         but `hotkey::classify` cannot read a sided spelling, so no settings screen accepts it.\n\
         That is KNEG-35, and it is why the warning does not suggest one."
    );
}
