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
//!    is a usable shortcut precisely because it is distinct — and it is one a user
//!    can type only because [`SIDED_MODIFIERS`] is read by `hotkey::classify`
//!    too. It was not, and the capability sat there with no way in (KNEG-35).
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

/// Every spelling that names a modifier on one particular side.
///
/// **One table, two readers, and that is the point.** [`Chord::parse`] resolves
/// these to a key, and `hotkey::classify` has to recognise exactly the same set
/// and no other. A spelling `classify` accepts that this parser does not becomes
/// a shortcut the settings screen calls hookable and `KeyHook::install` then
/// refuses; a spelling this parser accepts that `classify` does not was KNEG-35
/// — the headline capability of running a low-level hook at all, and no
/// accelerator a user could type reached it. Two lists would put both failures
/// one careless edit away.
///
/// `altgr` is in the table because Windows sends right Alt for it and a chord
/// watcher sees exactly that. It is not an endorsement of binding it — see
/// [`altgr_collision`], which is about the opposite problem.
pub const SIDED_MODIFIERS: [(&str, Key); 19] = [
    ("leftcontrol", Key::LeftControl),
    ("lcontrol", Key::LeftControl),
    ("rightcontrol", Key::RightControl),
    ("rcontrol", Key::RightControl),
    ("leftshift", Key::LeftShift),
    ("lshift", Key::LeftShift),
    ("rightshift", Key::RightShift),
    ("rshift", Key::RightShift),
    ("leftalt", Key::LeftAlt),
    ("lalt", Key::LeftAlt),
    ("rightalt", Key::RightAlt),
    ("ralt", Key::RightAlt),
    ("altgr", Key::RightAlt),
    ("leftsuper", Key::LeftSuper),
    ("lsuper", Key::LeftSuper),
    ("leftwin", Key::LeftSuper),
    ("rightsuper", Key::RightSuper),
    ("rsuper", Key::RightSuper),
    ("rightwin", Key::RightSuper),
];

/// The key `name` spells, if it names one particular side.
///
/// Case-insensitive, like every other spelling in this module.
#[must_use]
pub fn sided_modifier(name: &str) -> Option<Key> {
    let lower = name.to_lowercase();
    SIDED_MODIFIERS
        .iter()
        .find(|(spelling, _)| *spelling == lower)
        .map(|(_, key)| *key)
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

    /// The canonical sided spelling, for building an accelerator string.
    ///
    /// The inverse of the sided half of [`SIDED_MODIFIERS`]: that table maps
    /// many spellings onto one key, and this picks the one to show a person.
    #[must_use]
    pub const fn sided_spelling(self) -> Option<&'static str> {
        match self {
            Self::LeftControl => Some("LeftControl"),
            Self::RightControl => Some("RightControl"),
            Self::LeftShift => Some("LeftShift"),
            Self::RightShift => Some("RightShift"),
            Self::LeftAlt => Some("LeftAlt"),
            Self::RightAlt => Some("RightAlt"),
            Self::LeftSuper => Some("LeftSuper"),
            Self::RightSuper => Some("RightSuper"),
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

        // Read from the shared table rather than repeated here, so the set this
        // accepts and the set `hotkey::classify` accepts are the same set by
        // construction. Before the match, because a sided spelling is never also
        // one of the either-side ones below.
        if let Some(key) = sided_modifier(&lower) {
            return Ok(ChordKey::Exact(key));
        }

        Ok(match lower.as_str() {
            "control" | "ctrl" | "commandorcontrol" | "cmdorctrl" => {
                ChordKey::EitherSide("Control")
            }
            "shift" => ChordKey::EitherSide("Shift"),
            "alt" | "option" => ChordKey::EitherSide("Alt"),
            "super" | "meta" | "cmd" | "command" | "win" => ChordKey::EitherSide("Super"),

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

/* ------------------------------------------------------------------- AltGr */

/// One way a user can press AltGr, and the keys Windows produces for it.
///
/// **There is no AltGr virtual key.** Windows synthesises the combination in
/// the keyboard layout, sending **left Control and right Alt**, so nothing
/// downstream -- this hook included -- can tell an AltGr keystroke from someone
/// deliberately holding those two. That is not a gap in this code; it is what
/// arrives.
///
/// Order is part of each case rather than an accident. Rule 2 says a superset
/// does not match, so `Control+Alt` fires on a bare AltGr and does *not* fire
/// when Shift was already held -- the held set is three keys by the time the
/// chord could complete. Getting that wrong in either direction would produce a
/// warning that does not match the behaviour, which is worse than none.
const ALTGR_PRESSES: [(&str, &[Key]); 4] = [
    ("AltGr", &[Key::LeftControl, Key::RightAlt]),
    (
        "Shift held, then AltGr",
        &[Key::LeftShift, Key::LeftControl, Key::RightAlt],
    ),
    (
        "right Shift held, then AltGr",
        &[Key::RightShift, Key::LeftControl, Key::RightAlt],
    ),
    (
        "AltGr, then Shift",
        &[Key::LeftControl, Key::RightAlt, Key::LeftShift],
    ),
];

/// Which AltGr keystroke, if any, makes this chord fire.
///
/// **Decided by running the real matcher, not by a second set of rules.** The
/// question "does `Control+Alt` fire on AltGr" has an exact answer that
/// [`ChordWatcher`] already computes several hundred times a second, and a
/// separate predicate that reasoned about modifier names would be free to drift
/// away from it -- silently, because the two would only disagree in the cases
/// nobody wrote a test for. Feeding the watcher the keys Windows actually sends
/// cannot drift: if the warning and the behaviour ever disagree, it is because
/// the behaviour changed, and the same call reports it.
///
/// Returns the description of the keystroke, so the warning can say *which*
/// one rather than "some AltGr combination".
#[must_use]
pub fn altgr_collision(chord: &Chord) -> Option<&'static str> {
    for (description, sequence) in ALTGR_PRESSES {
        let mut watcher = ChordWatcher::new(vec![chord.clone()]);
        if sequence
            .iter()
            .any(|key| watcher.key_down(*key) != Fired::Nothing)
        {
            return Some(description);
        }
    }
    None
}

/// A sided spelling of `chord` that an AltGr keystroke does not complete, if
/// one exists.
///
/// **Assembled, then checked with the same matcher the warning used.** AltGr is
/// left Control and right Alt *specifically*, so naming the other side of
/// either one breaks the collision — but that is a claim about when a chord
/// fires, and this module's standing rule is that such claims are answered by
/// running [`ChordWatcher`] rather than by reasoning about names. So the
/// candidate is built and then passed back through [`altgr_collision`]; if it
/// still collides, nothing is suggested.
///
/// A remedy that does not work is worse than no remedy. That is not a general
/// sentiment: the AltGr caution named no remedy at all until KNEG-35 closed,
/// precisely because the settings field rejected every sided spelling, and
/// pointing at one would have sent the reader to a control that refused them.
///
/// Only `Control` and `Alt` are given a side. Siding `Shift` or `Super` would
/// narrow the shortcut for nothing — AltGr involves neither.
#[must_use]
pub fn sided_escape(chord: &Chord) -> Option<String> {
    let mut spelled = Vec::with_capacity(chord.keys.len());
    for key in &chord.keys {
        spelled.push(match key {
            ChordKey::EitherSide("Control") => "RightControl".to_owned(),
            ChordKey::EitherSide("Alt") => "LeftAlt".to_owned(),
            ChordKey::EitherSide(name) => (*name).to_owned(),
            ChordKey::Exact(exact) => exact.sided_spelling()?.to_owned(),
        });
    }

    let candidate = spelled.join("+");
    let parsed = Chord::parse(&candidate).ok()?;
    if altgr_collision(&parsed).is_some() {
        return None;
    }
    Some(candidate)
}

/// Ordinary keystrokes whose first two keys are modifiers, and what they do.
///
/// The third key is [`Key::Other`] with an arbitrary scan code: which key it is
/// does not matter, only that it is not a modifier, because the chord has
/// already completed by the time it arrives. That is the whole of KNEG-115.
///
/// **This list is not exhaustive and cannot be.** Every two-modifier chord is a
/// prefix of *something*; these are the ones common enough on Windows to name.
/// A chord absent from it is not thereby safe, which is why the caution built
/// from this says the general thing as well as the specific one.
const COMMON_PREFIXES: [(&str, &[Key]); 5] = [
    (
        "Ctrl+Win+Left, which switches virtual desktop",
        &[Key::LeftControl, Key::LeftSuper, Key::Other(37)],
    ),
    (
        "Ctrl+Shift+Escape, which opens Task Manager",
        &[Key::LeftControl, Key::LeftShift, Key::Other(27)],
    ),
    (
        "Ctrl+Alt+Delete",
        &[Key::LeftControl, Key::LeftAlt, Key::Other(46)],
    ),
    (
        "Win+Shift+S, which takes a screenshot",
        &[Key::LeftSuper, Key::LeftShift, Key::Other(83)],
    ),
    (
        "Alt+Shift+Tab, which switches window",
        &[Key::LeftAlt, Key::LeftShift, Key::Other(9)],
    ),
];

/// Which ordinary keystroke, if any, this chord fires on the way through.
///
/// **Decided by running the real matcher**, for the reason [`altgr_collision`]
/// is: whether `Control+Super` fires during `Ctrl+Win+Left` has an exact answer
/// that [`ChordWatcher`] already computes, and a predicate reasoning about
/// modifier names would be free to drift away from it in exactly the cases
/// nobody wrote a test for.
///
/// Returns `None` when no *listed* keystroke completes it, which is not the
/// same as safe. See [`COMMON_PREFIXES`].
#[must_use]
pub fn prefix_collision(chord: &Chord) -> Option<&'static str> {
    for (description, sequence) in COMMON_PREFIXES {
        let mut watcher = ChordWatcher::new(vec![chord.clone()]);
        if sequence
            .iter()
            .any(|key| watcher.key_down(*key) != Fired::Nothing)
        {
            return Some(description);
        }
    }
    None
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

    /* ------------------------------------------------------------- AltGr */

    /// The measured defect, KNEG-37.
    ///
    /// Injecting left Control and right Alt started a dictation on the
    /// 2026-08-26 bench run, and this is that keystroke fed to the same
    /// matcher the hook feeds.
    #[test]
    fn control_plus_alt_is_what_altgr_sends() {
        let chord = Chord::parse("Control+Alt").unwrap();
        assert_eq!(altgr_collision(&chord), Some("AltGr"));
    }

    /// The one nobody predicted, and the reason this asks the matcher instead
    /// of asking whether the accelerator names Control and Alt.
    ///
    /// AltGr's *left Control* arrives while Shift is already held, so the chord
    /// is complete before the Alt is pressed. A predicate written from the
    /// issue's description -- "does it name both Control and Alt" -- would have
    /// said `Control+Shift` was fine.
    #[test]
    fn control_plus_shift_fires_before_the_alt_even_arrives() {
        let chord = Chord::parse("Control+Shift").unwrap();
        assert_eq!(altgr_collision(&chord), Some("Shift held, then AltGr"));
    }

    /// The shipped dictation default is not affected, which is why this was
    /// filed as a warning rather than as an emergency.
    #[test]
    fn the_shipped_default_is_unaffected() {
        let chord = Chord::parse("Control+Super").unwrap();
        assert_eq!(altgr_collision(&chord), None);
    }

    /// Naming a side is the way out, and both sides matter.
    ///
    /// AltGr is *left* Control and *right* Alt specifically. Pinning either
    /// modifier to the other side avoids it entirely -- which is what makes
    /// KNEG-35 (no settings field accepts a sided spelling) the other half of
    /// this problem rather than an unrelated tidy-up.
    #[test]
    fn a_sided_chord_can_avoid_altgr_or_ask_for_it() {
        for safe in ["LeftControl+LeftAlt", "RightControl+Alt", "Control+LeftAlt"] {
            let chord = Chord::parse(safe).unwrap();
            assert_eq!(altgr_collision(&chord), None, "{safe}");
        }
        // And the user who genuinely wants AltGr as a hotkey can say so. This
        // is not a collision to warn about -- it is the feature -- but the
        // check reports it, because the keystroke is the same one and pretending
        // otherwise would need a rule about intent that nothing can know.
        let deliberate = Chord::parse("LeftControl+RightAlt").unwrap();
        assert_eq!(altgr_collision(&deliberate), Some("AltGr"));
    }

    /// A longer chord is not fired by a bare AltGr, because rule 2 holds: two
    /// keys down cannot satisfy a chord of three.
    #[test]
    fn a_three_key_chord_needs_the_third_key_from_somewhere_else() {
        let chord = Chord::parse("Control+Alt+Shift").unwrap();
        // Not the bare press -- the one where Shift was already held.
        assert_eq!(altgr_collision(&chord), Some("Shift held, then AltGr"));
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

    /* ------------------------------------ how often a prefix chord fires */

    /// Drive a watcher with a real keystroke and count the fires.
    ///
    /// `true` is a key going down, `false` up. Written out rather than
    /// abbreviated because the *order* is the whole subject: the fault
    /// KNEG-115 describes is entirely about what is held at each instant.
    fn fires(chord: &str, keystroke: &[(Key, bool)]) -> usize {
        let mut watcher = ChordWatcher::new(vec![Chord::parse(chord).expect("a valid chord")]);
        let mut count = 0;
        for (key, down) in keystroke {
            if *down {
                if watcher.key_down(*key) != Fired::Nothing {
                    count += 1;
                }
            } else {
                watcher.key_up(*key);
            }
        }
        count
    }

    /// The arrow key in `Ctrl+Win+Left`. Any non-modifier scan code will do.
    const ARROW: Key = Key::Other(37);

    /// Switching virtual desktop fires the default dictation chord once.
    ///
    /// **The measurement KNEG-115 asked for and did not have.** `Control+Super`
    /// is the shipped default and `Ctrl+Win+Left` switches desktop on Windows,
    /// so this is what a user on the default binding gets for an ordinary
    /// keystroke they did not think of as a shortcut.
    ///
    /// Once, not twice. The chord completes when Super goes down and the arrow
    /// arriving afterwards cannot un-fire it — rule 2 disqualifies a chord while
    /// an ordinary key is held, which stops a *second* fire and does nothing
    /// about the first.
    #[test]
    fn switching_virtual_desktop_fires_the_default_chord_once() {
        assert_eq!(
            fires(
                "Control+Super",
                &[
                    (Key::LeftControl, true),
                    (Key::LeftSuper, true),
                    (ARROW, true),
                    (ARROW, false),
                    (Key::LeftSuper, false),
                    (Key::LeftControl, false),
                ]
            ),
            1
        );
    }

    /// Two desktop switches are a start and a stop, and that is the cost.
    ///
    /// `dictation::on_hotkey` calls `DictationRuntime::toggle`, so an even
    /// number of fires is not harmless — the first starts recording and the
    /// second stops it, transcribes what the microphone heard in between, and
    /// puts the text at the cursor through a synthetic paste.
    ///
    /// So a user who switches desktop, works, and switches back has recorded
    /// the interval and had it typed into whatever had focus. That is a
    /// materially worse consequence than "a dictation starts on the way past",
    /// and it is why this is measured here rather than described.
    #[test]
    fn two_desktop_switches_are_a_start_and_a_stop() {
        let once = [
            (Key::LeftControl, true),
            (Key::LeftSuper, true),
            (ARROW, true),
            (ARROW, false),
            (Key::LeftSuper, false),
            (Key::LeftControl, false),
        ];
        let twice: Vec<(Key, bool)> = once.iter().chain(once.iter()).copied().collect();
        assert_eq!(fires("Control+Super", &twice), 2);
    }

    /// Tapping the arrow repeatedly without letting go of the chord fires once.
    ///
    /// Worth knowing, because it bounds the damage: cycling through four
    /// desktops with Ctrl and Win held is one toggle, not four. The re-arm on
    /// key-up is real, but the arrow going back down brings a non-modifier with
    /// it and rule 2 refuses.
    #[test]
    fn holding_the_chord_through_several_taps_still_fires_once() {
        let mut keystroke = vec![(Key::LeftControl, true), (Key::LeftSuper, true)];
        for _ in 0..4 {
            keystroke.push((ARROW, true));
            keystroke.push((ARROW, false));
        }
        keystroke.push((Key::LeftSuper, false));
        keystroke.push((Key::LeftControl, false));

        assert_eq!(fires("Control+Super", &keystroke), 1);
    }

    /// Releasing and re-pressing one modifier of the chord fires again.
    ///
    /// The other bound on the damage, in the unhelpful direction. Rule 1 re-arms
    /// on any key going up, so a user who keeps Control down and taps the
    /// Windows key twice has toggled twice.
    #[test]
    fn re_pressing_one_modifier_of_the_chord_fires_again() {
        assert_eq!(
            fires(
                "Control+Super",
                &[
                    (Key::LeftControl, true),
                    (Key::LeftSuper, true),
                    (Key::LeftSuper, false),
                    (Key::LeftSuper, true),
                    (Key::LeftControl, false),
                    (Key::LeftSuper, false),
                ]
            ),
            2
        );
    }

    /// A sided chord is not an escape from this one.
    ///
    /// It is from AltGr (KNEG-37), which is why the two are separate issues.
    /// `LeftControl+LeftSuper` is a prefix of `Ctrl+Win+Left` exactly as the
    /// unsided chord is.
    #[test]
    fn naming_a_side_does_not_help_with_a_prefix() {
        assert_eq!(
            fires(
                "LeftControl+LeftSuper",
                &[
                    (Key::LeftControl, true),
                    (Key::LeftSuper, true),
                    (ARROW, true),
                    (ARROW, false),
                    (Key::LeftSuper, false),
                    (Key::LeftControl, false),
                ]
            ),
            1
        );
    }

    /* --------------------------------------------------------- sided escapes */

    /// The escape has to be checked by the matcher, not asserted. `Control+Alt`
    /// is the chord the caution fires on, and `RightControl+LeftAlt` is what
    /// AltGr -- left Control and right Alt -- cannot complete.
    #[test]
    fn the_escape_from_altgr_is_one_the_matcher_agrees_with() {
        let colliding = Chord::parse("Control+Alt").expect("a valid chord");
        assert!(altgr_collision(&colliding).is_some(), "premise");

        // Sorted, not in the order the user typed: `Chord::parse` orders and
        // deduplicates so `Control+Super` and `Super+Control` are one chord, and
        // the suggestion is spelled from the chord rather than from the input.
        // It is a shortcut to type, not a quotation of theirs.
        let escape = sided_escape(&colliding).expect("an escape exists");
        assert_eq!(escape, "LeftAlt+RightControl");

        let parsed = Chord::parse(&escape).expect("the escape parses");
        assert!(
            altgr_collision(&parsed).is_none(),
            "{escape} still collides"
        );
    }

    /// Shift and Super are left alone: AltGr involves neither, and siding them
    /// would narrow the shortcut for nothing.
    #[test]
    fn only_the_keys_altgr_uses_are_given_a_side() {
        let chord = Chord::parse("Control+Shift+Alt").expect("a valid chord");
        assert_eq!(
            sided_escape(&chord).as_deref(),
            Some("LeftAlt+RightControl+Shift")
        );
    }

    /// Every spelling in the table resolves, and resolves to the key its name
    /// says. The classifier reads this table too, so an entry that resolved to
    /// the wrong key would misroute a shortcut rather than refuse it.
    #[test]
    fn every_sided_spelling_resolves_to_the_side_it_names() {
        for (spelling, key) in SIDED_MODIFIERS {
            assert_eq!(sided_modifier(spelling), Some(key), "{spelling}");
            assert_eq!(
                sided_modifier(&spelling.to_uppercase()),
                Some(key),
                "{spelling} is case-sensitive"
            );
        }
    }
}
