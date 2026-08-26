//! Deciding when a chord has been pressed.
//!
//! `hotkey::classify` reports `Control+Super` — upstream's default dictation
//! shortcut — as [`crate::hotkey::Accelerator::ModifierOnly`], because no
//! cross-platform global-shortcut API can register a chord with no ordinary key
//! in it. That is honest and it leaves the shortcut dead. What it needs is a
//! low-level keyboard hook, and what a low-level keyboard hook needs is this: a
//! state machine that turns a stream of key-down and key-up events into "the
//! chord fired".
//!
//! All of it is pure. The hook is in [`crate::keyhook`] and is deliberately
//! thin, because a hook callback cannot be tested and this can.
//!
//! # The four rules, and why each one exists
//!
//! 1. **Once per press.** Holding `Control+Super` produces a stream of repeated
//!    key-down messages. Firing on each would start and stop dictation dozens of
//!    times a second. The chord arms on release of any of its keys.
//! 2. **A superset does not match.** `Control+Shift+Super` is not `Control+Super`.
//!    Without this, a chord of two modifiers fires during any longer chord that
//!    contains it — including while the user is typing `Control+Shift+D` into
//!    another application.
//! 3. **Left and right are different keys.** Windows reports them separately and
//!    upstream's native listeners watched them separately. `RightControl+Super`
//!    is a usable shortcut precisely because it is distinct.
//! 4. **A modifier-only chord waits for the last key.** It fires when the final
//!    key of the set goes down, not when the first does — otherwise `Control`
//!    alone would fire `Control+Super` on its way to being pressed.

use std::collections::BTreeSet;

/// A key a low-level hook can distinguish.
///
/// Only the keys a chord can be built from. A full keyboard map would be a large
/// table nothing reads: what matters is the modifiers, side by side, plus a
/// catch-all for everything else so rule 2 can tell "an ordinary key is down".
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Key {
    LeftControl,
    RightControl,
    LeftShift,
    RightShift,
    LeftAlt,
    RightAlt,
    LeftSuper,
    RightSuper,
    /// Any key that is not a modifier, identified by its platform scan code.
    ///
    /// Kept as a number rather than named: a chord containing an ordinary key is
    /// something the cross-platform API can register, so this hook never needs
    /// to know *which* ordinary key it was — only that one is down, which is
    /// what rule 2 turns on.
    Other(u32),
}

impl Key {
    /// Whether this is a modifier.
    #[must_use]
    pub const fn is_modifier(self) -> bool {
        !matches!(self, Self::Other(_))
    }

    /// The side-independent name, for matching an accelerator string.
    ///
    /// `Control+Super` should match either Control. A chord that names a side
    /// explicitly is built from the sided variants instead.
    #[must_use]
    pub const fn unsided(self) -> Option<&'static str> {
        match self {
            Self::LeftControl | Self::RightControl => Some("Control"),
            Self::LeftShift | Self::RightShift => Some("Shift"),
            Self::LeftAlt | Self::RightAlt => Some("Alt"),
            Self::LeftSuper | Self::RightSuper => Some("Super"),
            Self::Other(_) => None,
        }
    }
}

/// One key of a chord: a specific key, or either side of a modifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ChordKey {
    /// Either side satisfies it. `Control+Super` from a settings screen.
    EitherSide(&'static str),
    /// Exactly this key. `RightAlt+Super`, which a user has to ask for.
    Exact(Key),
}

impl ChordKey {
    fn matches(self, key: Key) -> bool {
        match self {
            Self::EitherSide(name) => key.unsided() == Some(name),
            Self::Exact(wanted) => wanted == key,
        }
    }
}

/// A chord of modifiers with no ordinary key.
///
/// Only modifier-only chords, because a chord with an ordinary key in it is
/// registrable through the cross-platform API and should go there — it is the
/// operating system's job to arbitrate those, and a hook that also claimed them
/// would compete with every other application's shortcuts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chord {
    keys: Vec<ChordKey>,
}

/// Why a chord could not be built.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ChordError {
    #[error("a chord needs at least two keys; {found:?} is a single key")]
    TooShort { found: String },

    #[error(
        "{name:?} is not a modifier. A chord containing an ordinary key can be registered \
         through the platform's own shortcut API, which arbitrates it against other \
         applications; a keyboard hook would compete with them."
    )]
    NotAModifier { name: String },

    #[error("{name:?} is not a key this build recognises")]
    Unknown { name: String },
}

impl Chord {
    /// Build a chord from an accelerator string like `Control+Super`.
    ///
    /// Refuses anything containing an ordinary key. That is not a limitation
    /// being worked around — it is the boundary between this and the
    /// cross-platform shortcut API, and blurring it would mean two mechanisms
    /// claiming the same keystroke.
    pub fn parse(accelerator: &str) -> Result<Self, ChordError> {
        let mut keys = Vec::new();

        for part in accelerator.split('+') {
            let trimmed = part.trim();
            if trimmed.is_empty() {
                continue;
            }
            keys.push(Self::key_of(trimmed)?);
        }

        // Sorted and deduplicated so `Control+Super` and `Super+Control` are one
        // chord. Order of pressing is not part of the identity: nobody presses
        // two modifiers in a reproducible order.
        //
        // Before the length check, not after: `Control+Ctrl` is two spellings of
        // one key, so it is a chord of one and has to be refused as one.
        keys.sort_unstable();
        keys.dedup();

        if keys.len() < 2 {
            return Err(ChordError::TooShort {
                found: accelerator.to_owned(),
            });
        }

        Ok(Self { keys })
    }

    fn key_of(name: &str) -> Result<ChordKey, ChordError> {
        // Accepted spellings match `hotkey::MODIFIERS`, plus the sided ones a
        // hook makes possible for the first time.
        let lower = name.to_lowercase();
        Ok(match lower.as_str() {
            "control" | "ctrl" | "commandorcontrol" | "cmdorctrl" => {
                ChordKey::EitherSide("Control")
            }
            "shift" => ChordKey::EitherSide("Shift"),
            "alt" | "option" => ChordKey::EitherSide("Alt"),
            "super" | "meta" | "cmd" | "command" | "win" => ChordKey::EitherSide("Super"),

            "leftcontrol" | "lcontrol" => ChordKey::Exact(Key::LeftControl),
            "rightcontrol" | "rcontrol" => ChordKey::Exact(Key::RightControl),
            "leftshift" | "lshift" => ChordKey::Exact(Key::LeftShift),
            "rightshift" | "rshift" => ChordKey::Exact(Key::RightShift),
            "leftalt" | "lalt" => ChordKey::Exact(Key::LeftAlt),
            "rightalt" | "ralt" | "altgr" => ChordKey::Exact(Key::RightAlt),
            "leftsuper" | "lsuper" | "leftwin" => ChordKey::Exact(Key::LeftSuper),
            "rightsuper" | "rsuper" | "rightwin" => ChordKey::Exact(Key::RightSuper),

            // A single character or a named key: registrable elsewhere.
            other if other.chars().count() == 1 || is_named_key(other) => {
                return Err(ChordError::NotAModifier {
                    name: name.to_owned(),
                })
            }
            _ => {
                return Err(ChordError::Unknown {
                    name: name.to_owned(),
                })
            }
        })
    }

    /// How many keys have to be held.
    #[must_use]
    pub fn len(&self) -> usize {
        self.keys.len()
    }

    /// Always false — [`Chord::parse`] refuses fewer than two keys.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    /// Whether `held` is exactly this chord: every key satisfied, nothing extra.
    fn satisfied_by(&self, held: &BTreeSet<Key>) -> bool {
        // Rule 2: a superset does not match. Counting first is the cheap half of
        // that, and it is also what rules out `Control+Super` firing while
        // `Control+Shift+Super` is held.
        if held.len() != self.keys.len() {
            return false;
        }

        // Every chord key must be satisfied by a distinct held key. The chord is
        // deduplicated and the held set is a set, so a greedy match is exact.
        self.keys
            .iter()
            .all(|wanted| held.iter().any(|key| wanted.matches(*key)))
            && held
                .iter()
                .all(|key| self.keys.iter().any(|wanted| wanted.matches(*key)))
    }
}

/// Whether a name is a key the cross-platform API would know.
///
/// Not exhaustive and does not need to be: it exists so `Control+F5` reports "F5
/// is not a modifier" rather than "F5 is not recognised", which is the difference
/// between a useful message and a confusing one.
fn is_named_key(lower: &str) -> bool {
    const NAMED: [&str; 14] = [
        "space",
        "tab",
        "enter",
        "return",
        "escape",
        "esc",
        "backspace",
        "delete",
        "up",
        "down",
        "left",
        "right",
        "home",
        "end",
    ];
    NAMED.contains(&lower) || (lower.starts_with('f') && lower[1..].parse::<u8>().is_ok())
}

/// What a watcher decided about one event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fired {
    /// The chord at this index just fired.
    Chord(usize),
    /// Nothing.
    Nothing,
}

/// Tracks which keys are down and reports when a watched chord completes.
///
/// One watcher for all chords rather than one each: the held-key set is shared,
/// and two watchers would each have their own idea of what is pressed.
#[derive(Debug)]
pub struct ChordWatcher {
    chords: Vec<Chord>,
    held: BTreeSet<Key>,
    /// Which chord is currently satisfied and has already fired.
    ///
    /// Rule 1. Cleared when any key goes up, so a hold produces one event and a
    /// second press produces a second.
    armed: Option<usize>,
}

impl ChordWatcher {
    #[must_use]
    pub fn new(chords: Vec<Chord>) -> Self {
        Self {
            chords,
            held: BTreeSet::new(),
            armed: None,
        }
    }

    /// Whether anything is being watched. A watcher with no chords is a hook
    /// nobody needs, and the caller should not install one.
    #[must_use]
    pub fn is_watching(&self) -> bool {
        !self.chords.is_empty()
    }

    /// Record a key going down.
    pub fn key_down(&mut self, key: Key) -> Fired {
        // A repeat of a key already held changes nothing. Windows sends these
        // continuously while a key is held.
        if !self.held.insert(key) && self.armed.is_some() {
            return Fired::Nothing;
        }

        // Rule 2, the other half: an ordinary key being down disqualifies every
        // modifier-only chord. Held rather than ignored, so the set is right
        // when it comes back up.
        if self.held.iter().any(|k| !k.is_modifier()) {
            return Fired::Nothing;
        }

        // Rule 4: fires on the last key of the set, not the first.
        for (index, chord) in self.chords.iter().enumerate() {
            if chord.satisfied_by(&self.held) {
                if self.armed == Some(index) {
                    return Fired::Nothing;
                }
                self.armed = Some(index);
                return Fired::Chord(index);
            }
        }

        Fired::Nothing
    }

    /// Record a key going up.
    pub fn key_up(&mut self, key: Key) {
        self.held.remove(&key);
        // Rule 1: releasing any key re-arms, so the next full press fires again.
        self.armed = None;
    }

    /// Forget everything held.
    ///
    /// Called when the hook is reinstalled or the session unlocks: a key-up that
    /// arrives while no hook is installed is a key this watcher believes is
    /// still down for ever.
    pub fn reset(&mut self) {
        self.held.clear();
        self.armed = None;
    }

    /// How many keys are currently held. For diagnostics.
    #[must_use]
    pub fn held_count(&self) -> usize {
        self.held.len()
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;

    fn watcher(accelerators: &[&str]) -> ChordWatcher {
        ChordWatcher::new(
            accelerators
                .iter()
                .map(|a| Chord::parse(a).expect("test chord"))
                .collect(),
        )
    }

    /* -------------------------------------------------------------- parsing */

    /// The default that started all of this parses.
    #[test]
    fn upstreams_dead_default_is_a_valid_chord() {
        let chord = Chord::parse("Control+Super").expect("should parse");
        assert_eq!(chord.len(), 2);
    }

    /// Order is not part of a chord's identity.
    #[test]
    fn the_order_the_keys_are_written_in_does_not_matter() {
        assert_eq!(
            Chord::parse("Control+Super").unwrap(),
            Chord::parse("Super+Control").unwrap()
        );
    }

    /// A chord containing an ordinary key is refused, with the reason.
    ///
    /// Not a limitation: it is the boundary with the platform's own shortcut
    /// API, and a hook that claimed `Control+Shift+D` as well would compete with
    /// every other application's shortcuts.
    #[test]
    fn a_chord_with_an_ordinary_key_is_refused_and_says_where_it_belongs() {
        for accelerator in ["Control+Shift+D", "Control+F5", "Alt+Space", "Super+Enter"] {
            match Chord::parse(accelerator) {
                Err(ChordError::NotAModifier { .. }) => {}
                other => panic!("{accelerator} should be refused, got {other:?}"),
            }
        }

        // And the message points at the platform API rather than just refusing.
        let err = Chord::parse("Control+D").unwrap_err();
        assert!(format!("{err}").contains("shortcut API"), "{err}");
    }

    /// One key is not a chord.
    #[test]
    fn a_single_modifier_is_not_a_chord() {
        assert!(matches!(
            Chord::parse("Control"),
            Err(ChordError::TooShort { .. })
        ));
        // Nor is the same modifier twice, which deduplicates to one.
        assert!(matches!(
            Chord::parse("Control+Ctrl"),
            Err(ChordError::TooShort { .. })
        ));
    }

    /// Sided modifiers are their own keys, which is what a hook makes possible.
    #[test]
    fn a_side_can_be_named_explicitly() {
        let chord = Chord::parse("RightAlt+Super").expect("should parse");
        let mut held = BTreeSet::new();
        held.insert(Key::RightAlt);
        held.insert(Key::LeftSuper);
        assert!(chord.satisfied_by(&held));

        // The other Alt does not satisfy it.
        let mut wrong = BTreeSet::new();
        wrong.insert(Key::LeftAlt);
        wrong.insert(Key::LeftSuper);
        assert!(!chord.satisfied_by(&wrong));
    }

    /// An unsided chord takes either side.
    #[test]
    fn an_unsided_chord_accepts_either_side() {
        let chord = Chord::parse("Control+Shift").unwrap();
        for control in [Key::LeftControl, Key::RightControl] {
            for shift in [Key::LeftShift, Key::RightShift] {
                let held: BTreeSet<Key> = [control, shift].into_iter().collect();
                assert!(chord.satisfied_by(&held), "{control:?}+{shift:?}");
            }
        }
    }

    /* -------------------------------------------------------------- firing */

    /// The chord fires when the last of its keys goes down. Rule 4.
    #[test]
    fn a_chord_fires_on_its_last_key_and_not_its_first() {
        let mut w = watcher(&["Control+Super"]);
        assert_eq!(w.key_down(Key::LeftControl), Fired::Nothing);
        assert_eq!(w.key_down(Key::LeftSuper), Fired::Chord(0));
    }

    /// Holding it does not fire it repeatedly. Rule 1.
    ///
    /// The rule that matters most in practice: Windows sends key-down
    /// continuously while a key is held, and firing on each would start and stop
    /// dictation dozens of times a second.
    #[test]
    fn holding_a_chord_fires_once() {
        let mut w = watcher(&["Control+Super"]);
        w.key_down(Key::LeftControl);
        assert_eq!(w.key_down(Key::LeftSuper), Fired::Chord(0));

        // Twenty repeats of both keys.
        for _ in 0..20 {
            assert_eq!(w.key_down(Key::LeftControl), Fired::Nothing);
            assert_eq!(w.key_down(Key::LeftSuper), Fired::Nothing);
        }
    }

    /// Releasing and pressing again fires again.
    #[test]
    fn a_second_press_fires_again() {
        let mut w = watcher(&["Control+Super"]);
        w.key_down(Key::LeftControl);
        assert_eq!(w.key_down(Key::LeftSuper), Fired::Chord(0));

        w.key_up(Key::LeftSuper);
        assert_eq!(w.key_down(Key::LeftSuper), Fired::Chord(0));
    }

    /// A superset does not match. Rule 2.
    ///
    /// Without this, `Control+Super` fires during `Control+Shift+Super` — and,
    /// worse, during any longer chord the user is typing into another
    /// application.
    #[test]
    fn a_longer_chord_does_not_fire_a_shorter_one_inside_it() {
        let mut w = watcher(&["Control+Super"]);
        w.key_down(Key::LeftControl);
        w.key_down(Key::LeftShift);
        assert_eq!(
            w.key_down(Key::LeftSuper),
            Fired::Nothing,
            "three modifiers held must not fire a two-modifier chord"
        );
    }

    /// An ordinary key being down disqualifies every modifier-only chord.
    ///
    /// This is what stops the hook firing while somebody types `Control+Super+D`
    /// or holds Control and clicks through a document.
    #[test]
    fn an_ordinary_key_held_suppresses_the_chord() {
        let mut w = watcher(&["Control+Super"]);
        w.key_down(Key::Other(0x44)); // D
        w.key_down(Key::LeftControl);
        assert_eq!(w.key_down(Key::LeftSuper), Fired::Nothing);

        // And once it comes up, the chord works again — the ordinary key was
        // tracked, not ignored.
        w.key_up(Key::Other(0x44));
        w.key_up(Key::LeftSuper);
        assert_eq!(w.key_down(Key::LeftSuper), Fired::Chord(0));
    }

    /// Several chords are watched at once, and the right one fires.
    #[test]
    fn the_matching_chord_is_the_one_reported() {
        let mut w = watcher(&["Control+Super", "Alt+Shift"]);

        w.key_down(Key::LeftAlt);
        assert_eq!(w.key_down(Key::RightShift), Fired::Chord(1));

        w.key_up(Key::LeftAlt);
        w.key_up(Key::RightShift);

        w.key_down(Key::RightControl);
        assert_eq!(w.key_down(Key::RightSuper), Fired::Chord(0));
    }

    /// Resetting forgets keys the hook never saw released.
    ///
    /// A locked screen or a reinstalled hook swallows the key-up. Without a
    /// reset, the watcher believes a key is held for the rest of the session and
    /// every chord containing it silently stops working.
    #[test]
    fn a_reset_clears_keys_whose_release_was_never_seen() {
        let mut w = watcher(&["Control+Super"]);
        w.key_down(Key::LeftControl);
        w.key_down(Key::LeftShift);
        assert_eq!(w.held_count(), 2);

        w.reset();
        assert_eq!(w.held_count(), 0);

        // And the chord works immediately afterwards.
        w.key_down(Key::LeftControl);
        assert_eq!(w.key_down(Key::LeftSuper), Fired::Chord(0));
    }

    /// A watcher with nothing to watch says so.
    #[test]
    fn an_empty_watcher_is_not_watching() {
        assert!(!ChordWatcher::new(Vec::new()).is_watching());
        assert!(watcher(&["Control+Super"]).is_watching());
    }
}
