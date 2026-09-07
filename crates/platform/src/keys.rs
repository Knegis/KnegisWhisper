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
//!
//! Toggle activation is a separate `Activated` event: all keys must be released
//! and any intervening key cancels that candidate for the remainder of the gesture.
//! The press/release rules above remain the hold-mode interface.
//!
//! # Two things that are not chords
//!
//! **A mouse button is not a modifier and is not a chord.** [`Chord::parse`]
//! refuses fewer than two keys and refuses anything that is not a modifier, so
//! forcing `Mouse4` through it would mean either loosening both rules or lying
//! about what a chord is. It gets [`Watched::Button`] instead, matched by
//! [`HookWatcher::button_down`] against a table that is the mouse twin of
//! [`SIDED_MODIFIERS`] — one table, read by this parser and by
//! `hotkey::classify`, for the reason that table's own comment gives.
//!
//! **A release is an event.** [`ChordWatcher::key_up`] returned `()` until
//! push-to-talk needed it: holding a chord to record and letting go to stop
//! means the release has to be reportable, so both edges now come back as
//! [`Fired`]. Nothing decides *what* a release means here — that is
//! `shortcuts::act`, and it is deliberately a different module, because when a
//! release should stop a dictation is a product question and when a chord has
//! stopped being held is not.

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

/* ------------------------------------------------------------ mouse buttons */

/// A mouse button this build is willing to watch.
///
/// Only the two side buttons. There is no `Left`, and that is a decision rather
/// than an omission — see [`refused_mouse_button`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MouseButton {
    /// Windows `XBUTTON1`, the one usually labelled "back".
    Four,
    /// Windows `XBUTTON2`, the one usually labelled "forward".
    Five,
}

impl MouseButton {
    /// The canonical spelling, for building an accelerator string.
    #[must_use]
    pub const fn spelling(self) -> &'static str {
        match self {
            Self::Four => "Mouse4",
            Self::Five => "Mouse5",
        }
    }

    /// What a browser does with this button when the hook does not consume it.
    ///
    /// Named rather than described, because the hook deliberately never
    /// swallows an event (see [`crate::keyhook`]): binding one of these means
    /// the click still reaches whatever is under the pointer, and on these two
    /// buttons that is not nothing.
    #[must_use]
    pub const fn also_does(self) -> &'static str {
        match self {
            Self::Four => "Back",
            Self::Five => "Forward",
        }
    }
}

/// Every spelling that names a watchable mouse button.
///
/// **One table, two readers**, exactly as [`SIDED_MODIFIERS`] is and for the
/// same reason: [`Watched::parse`] resolves these and `hotkey::classify` has to
/// recognise the same set and no other, or a spelling the settings screen calls
/// hookable is one `KeyHook::install` then refuses.
///
/// `xbutton1` and `xbutton2` are in the table because that is what Windows
/// calls them and somebody reading the API will type them. They are the
/// *measured* names: `cargo run -p kw-platform --example mousehook_probe`
/// reports `XBUTTON1` arriving as `mouseData` high word 1 and `XBUTTON2` as 2,
/// which is where the off-by-one in "button 4 is XBUTTON1" is easy to get
/// wrong and is not guessed here.
pub const MOUSE_BUTTONS: [(&str, MouseButton); 8] = [
    ("mouse4", MouseButton::Four),
    ("mousebutton4", MouseButton::Four),
    ("xbutton1", MouseButton::Four),
    ("mouseback", MouseButton::Four),
    ("mouse5", MouseButton::Five),
    ("mousebutton5", MouseButton::Five),
    ("xbutton2", MouseButton::Five),
    ("mouseforward", MouseButton::Five),
];

/// The mouse button `name` spells, if it names one this build will watch.
#[must_use]
pub fn mouse_button(name: &str) -> Option<MouseButton> {
    let lower = name.to_lowercase();
    MOUSE_BUTTONS
        .iter()
        .find(|(spelling, _)| *spelling == lower)
        .map(|(_, button)| *button)
}

/// Spellings that name a mouse button this build refuses, and which one.
///
/// **The refusal is the feature.** The hook does not consume the event, so
/// binding the left button would start a dictation on *every click in every
/// application* while the click still went through — an application that does
/// that is indistinguishable from a broken mouse, which is the same reasoning
/// `keyhook` gives for never swallowing a keystroke. The two side buttons are
/// the only ones with no universal meaning to destroy.
const REFUSED_MOUSE_BUTTONS: [(&str, &str); 9] = [
    ("mouse1", "the left button"),
    ("mouseleft", "the left button"),
    ("leftclick", "the left button"),
    ("mouse2", "the right button"),
    ("mouseright", "the right button"),
    ("rightclick", "the right button"),
    ("mouse3", "the middle button"),
    ("mousemiddle", "the middle button"),
    ("middleclick", "the middle button"),
];

/// Which button `name` names, if it names one this build refuses to watch.
///
/// Returns the phrase to put in the refusal, so the message says *which* button
/// rather than "that one".
#[must_use]
pub fn refused_mouse_button(name: &str) -> Option<&'static str> {
    let lower = name.to_lowercase();
    REFUSED_MOUSE_BUTTONS
        .iter()
        .find(|(spelling, _)| *spelling == lower)
        .map(|(_, what)| *what)
}

/// Whether `name` names a mouse button at all, watchable or not.
///
/// Used by `hotkey::classify` to keep a mouse spelling out of the ordinary-key
/// path: without it, `Mouse4` classifies as a perfectly good `Standard`
/// accelerator, the shortcut plugin is handed it, and the refusal arrives from
/// a parser that has never heard of a mouse.
#[must_use]
pub fn names_a_mouse_button(name: &str) -> bool {
    mouse_button(name).is_some() || refused_mouse_button(name).is_some()
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

    #[error(
        "{name:?} is a mouse button, and a mouse button is watched on its own. The system \
         shortcut register cannot take one at all, and the chord matcher only ever matches \
         modifier keys, so there is no mechanism that could hold the combination."
    )]
    MouseInAChord { name: String },

    #[error(
        "{name:?} names {what}, which this build will not watch. The hook does not swallow the \
         event, so binding it would start this action on every click in every application while \
         the click still went through. Use Mouse4 or Mouse5, the two side buttons."
    )]
    MouseButtonRefused { name: String, what: &'static str },
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

        // Before the single-character and named-key arms below, which would
        // otherwise call `Mouse4` "not a modifier" and point the user at the
        // platform's shortcut API — which cannot take a mouse button at all.
        if names_a_mouse_button(&lower) {
            return Err(ChordError::MouseInAChord {
                name: name.to_owned(),
            });
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

    /// Whether `key` is one of the keys this chord is made of.
    ///
    /// Not "is the chord satisfied" — one key, asked of the pattern rather than
    /// of what is held. It is what tells a key going up apart from an unrelated
    /// key going up while the chord is still down.
    #[must_use]
    pub fn contains(&self, key: Key) -> bool {
        self.keys.iter().any(|wanted| wanted.matches(key))
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
///
/// Two edges rather than one. `Chord(index)` used to be the only thing a
/// watcher could say, because the only question was "has it fired"; push-to-talk
/// asks the other half, and a watcher that cannot report a release cannot answer
/// it. The names are the edges and not their meanings on purpose: what a release
/// *does* is `shortcuts::act`'s decision and depends on a setting this module
/// has never heard of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fired {
    /// The thing at this index was just fully pressed.
    Pressed(usize),
    /// The thing at this index stopped being fully pressed.
    ///
    /// Reported once per press, and only for a press that was reported: a
    /// release nobody could pair with a press is not an event.
    Released(usize),
    /// An exact chord was released completely without another key intervening.
    Activated(usize),
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
    /// Which chord is currently *held down*, for the release report.
    ///
    /// **Not the same field as `armed`, and the difference is a whole key.**
    /// `armed` is cleared by *any* key going up, including an ordinary one — it
    /// has to be, or holding `Control+Super` and tapping an arrow would never
    /// re-arm. `pressed` is cleared only when a key of the chord itself goes
    /// up, because that is the moment the chord stops being held. Sharing one
    /// field would mean a push-to-talk dictation started with `Control+Super`
    /// and then interrupted by any other keystroke could never be stopped by
    /// letting go: the release would find nothing to pair with and the
    /// microphone would stay open.
    pressed: Option<usize>,
    candidate: Option<usize>,
    cancelled: BTreeSet<usize>,
    gesture: BTreeSet<Key>,
}

impl ChordWatcher {
    #[must_use]
    pub fn new(chords: Vec<Chord>) -> Self {
        Self {
            chords,
            held: BTreeSet::new(),
            armed: None,
            pressed: None,
            candidate: None,
            cancelled: BTreeSet::new(),
            gesture: BTreeSet::new(),
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
        if !self.held.insert(key) {
            return Fired::Nothing;
        }
        // A new key after an exact match contaminates this entire gesture.
        if let Some(candidate) = self.candidate.take() {
            self.cancelled.insert(candidate);
        }
        self.gesture.insert(key);

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
                self.pressed = Some(index);
                if !self.cancelled.contains(&index) && chord.satisfied_by(&self.gesture) {
                    self.candidate = Some(index);
                }
                return Fired::Pressed(index);
            }
        }

        Fired::Nothing
    }

    /// Record a key going up.
    ///
    /// Returns [`Fired::Released`] when the key that went up belonged to the
    /// chord that is currently held, and [`Fired::Nothing`] otherwise — so an
    /// ordinary key coming up in the middle of a hold is not a release, and a
    /// key coming up when nothing was pressed is not one either.
    pub fn key_up(&mut self, key: Key) -> Fired {
        self.held.remove(&key);
        let activated = if self.held.is_empty() {
            self.gesture.clear();
            self.cancelled.clear();
            self.candidate.take()
        } else {
            None
        };
        // Rule 1: releasing any key re-arms, so the next full press fires again.
        self.armed = None;

        // Only a key of the pressed chord ends the press. The `and_then` reads
        // the chord out before deciding, because a stale index would be a panic
        // in a hook callback -- undefined behaviour, per `keyhook`'s module
        // documentation -- rather than a wrong answer.
        let ends_it = self
            .pressed
            .and_then(|index| self.chords.get(index))
            .is_some_and(|chord| chord.contains(key));

        if ends_it {
            let index = self.pressed.take();
            return index.map_or(Fired::Nothing, Fired::Released);
        }

        activated.map_or(Fired::Nothing, Fired::Activated)
    }

    /// Forget everything held.
    ///
    /// Called when the hook is reinstalled or the session unlocks: a key-up that
    /// arrives while no hook is installed is a key this watcher believes is
    /// still down for ever.
    ///
    /// Deliberately silent about the press it forgets. A caller that reset
    /// because it lost track of the keyboard cannot also claim to know the key
    /// came up, and a synthesised release would stop a push-to-talk dictation
    /// on a key the user is still holding.
    pub fn reset(&mut self) {
        self.held.clear();
        self.armed = None;
        self.pressed = None;
        self.candidate = None;
        self.cancelled.clear();
        self.gesture.clear();
    }

    /// How many keys are currently held. For diagnostics.
    #[must_use]
    pub fn held_count(&self) -> usize {
        self.held.len()
    }
}

/* ------------------------------------------------------- keyboard and mouse */

/// One thing the hook is asked to watch.
///
/// A flat list of these is what [`crate::keyhook::KeyHook::install`] takes, and
/// the index into that list is what comes back when one fires. That is why the
/// two kinds share an enum rather than living in two lists: the caller has one
/// list of accelerators, in one order, and re-deriving which of two lists an
/// index belonged to is the kind of bookkeeping that goes wrong silently.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Watched {
    Chord(Chord),
    Button(MouseButton),
}

impl Watched {
    /// Parse one accelerator into whatever kind of thing it is.
    ///
    /// Mouse first, because [`Chord::parse`] would otherwise have to decide what
    /// a lone `Mouse4` is, and its honest answer would be "a chord of one",
    /// which is a refusal with the wrong reason attached.
    pub fn parse(accelerator: &str) -> Result<Self, ChordError> {
        let trimmed = accelerator.trim();

        if let Some(button) = mouse_button(trimmed) {
            return Ok(Self::Button(button));
        }
        if let Some(what) = refused_mouse_button(trimmed) {
            return Err(ChordError::MouseButtonRefused {
                name: trimmed.to_owned(),
                what,
            });
        }

        Chord::parse(trimmed).map(Self::Chord)
    }

    /// Whether this needs the mouse hook.
    #[must_use]
    pub const fn is_button(&self) -> bool {
        matches!(self, Self::Button(_))
    }
}

/// Tracks keyboard chords and mouse buttons together, over one index space.
///
/// A thin router rather than a second matcher: the chord half is
/// [`ChordWatcher`], untouched and still the thing `altgr_collision` and
/// `prefix_collision` drive, and this maps its indices back onto the caller's
/// list. Reimplementing the chord rules here to get one flat loop would have
/// meant two matchers that agree until they do not — the failure mode this
/// module's other comments keep naming.
///
/// The button half needs no rules of that kind. A button is one event with no
/// set to satisfy, so all it needs is a record of which are down, so that a
/// second down without an intervening up cannot report a second press. Whether
/// Windows ever repeats `WM_XBUTTONDOWN` for a held button is **not measured** —
/// `SendInput` cannot produce an auto-repeat to look at — so the set is there
/// because the cost is a `BTreeSet` insert and the alternative is a guess.
#[derive(Debug)]
pub struct HookWatcher {
    chords: ChordWatcher,
    /// The caller's index for each chord inside `chords`, in the same order.
    chord_index: Vec<usize>,
    /// The caller's index for each watched button.
    buttons: Vec<(usize, MouseButton)>,
    down: BTreeSet<MouseButton>,
}

impl HookWatcher {
    #[must_use]
    pub fn new(watched: Vec<Watched>) -> Self {
        let mut chords = Vec::new();
        let mut chord_index = Vec::new();
        let mut buttons = Vec::new();

        for (index, item) in watched.into_iter().enumerate() {
            match item {
                Watched::Chord(chord) => {
                    chords.push(chord);
                    chord_index.push(index);
                }
                Watched::Button(button) => buttons.push((index, button)),
            }
        }

        Self {
            chords: ChordWatcher::new(chords),
            chord_index,
            buttons,
            down: BTreeSet::new(),
        }
    }

    /// Whether anything is being watched at all.
    #[must_use]
    pub fn is_watching(&self) -> bool {
        self.chords.is_watching() || !self.buttons.is_empty()
    }

    /// Whether a mouse hook is needed.
    ///
    /// Asked rather than assumed, because `WH_MOUSE_LL` sits in the path of
    /// every mouse *move* on the machine — hundreds a second while the pointer
    /// is in use — and installing it for a configuration that watches no button
    /// is pure cost with no capability behind it.
    #[must_use]
    pub fn watches_a_button(&self) -> bool {
        !self.buttons.is_empty()
    }

    /// Translate a chord-list index into the caller's index.
    fn outward(&self, fired: Fired) -> Fired {
        let translate = |index: usize| self.chord_index.get(index).copied();
        match fired {
            Fired::Pressed(index) => translate(index).map_or(Fired::Nothing, Fired::Pressed),
            Fired::Released(index) => translate(index).map_or(Fired::Nothing, Fired::Released),
            Fired::Activated(index) => translate(index).map_or(Fired::Nothing, Fired::Activated),
            Fired::Nothing => Fired::Nothing,
        }
    }

    pub fn key_down(&mut self, key: Key) -> Fired {
        let fired = self.chords.key_down(key);
        self.outward(fired)
    }

    pub fn key_up(&mut self, key: Key) -> Fired {
        let fired = self.chords.key_up(key);
        self.outward(fired)
    }

    /// Record a mouse button going down.
    pub fn button_down(&mut self, button: MouseButton) -> Fired {
        if !self.down.insert(button) {
            return Fired::Nothing;
        }
        self.buttons
            .iter()
            .find(|(_, watched)| *watched == button)
            .map_or(Fired::Nothing, |(index, _)| Fired::Pressed(*index))
    }

    /// Record a mouse button going up.
    pub fn button_up(&mut self, button: MouseButton) -> Fired {
        if !self.down.remove(&button) {
            return Fired::Nothing;
        }
        self.buttons
            .iter()
            .find(|(_, watched)| *watched == button)
            .map_or(Fired::Nothing, |(index, _)| Fired::Released(*index))
    }

    /// Forget everything held, keyboard and mouse alike.
    pub fn reset(&mut self) {
        self.chords.reset();
        self.down.clear();
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
        assert_eq!(w.key_down(Key::LeftSuper), Fired::Pressed(0));
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
        assert_eq!(w.key_down(Key::LeftSuper), Fired::Pressed(0));

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
        assert_eq!(w.key_down(Key::LeftSuper), Fired::Pressed(0));

        w.key_up(Key::LeftSuper);
        assert_eq!(w.key_down(Key::LeftSuper), Fired::Pressed(0));
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
        assert_eq!(w.key_down(Key::LeftSuper), Fired::Pressed(0));
    }

    /// Several chords are watched at once, and the right one fires.
    #[test]
    fn the_matching_chord_is_the_one_reported() {
        let mut w = watcher(&["Control+Super", "Alt+Shift"]);

        w.key_down(Key::LeftAlt);
        assert_eq!(w.key_down(Key::RightShift), Fired::Pressed(1));

        w.key_up(Key::LeftAlt);
        w.key_up(Key::RightShift);

        w.key_down(Key::RightControl);
        assert_eq!(w.key_down(Key::RightSuper), Fired::Pressed(0));
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
        assert_eq!(w.key_down(Key::LeftSuper), Fired::Pressed(0));
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

    /* --------------------------------------------------------- mouse buttons */

    /// Every spelling in the mouse table resolves, and to the button it names.
    ///
    /// The mouse twin of the test above, and it earns its place for the same
    /// reason: `hotkey::classify` reads this table too, so an entry resolving to
    /// the wrong button would bind the wrong click rather than refuse.
    #[test]
    fn every_mouse_spelling_resolves_to_the_button_it_names() {
        for (spelling, button) in MOUSE_BUTTONS {
            assert_eq!(mouse_button(spelling), Some(button), "{spelling}");
            assert_eq!(
                mouse_button(&spelling.to_uppercase()),
                Some(button),
                "{spelling} is case-sensitive"
            );
        }
        // And the two numbers are not swapped. `xbutton1` is button *four*,
        // which is the off-by-one this whole area invites -- measured by
        // `mousehook_probe`, which reports XBUTTON1 arriving as high word 1.
        assert_eq!(mouse_button("xbutton1"), Some(MouseButton::Four));
        assert_eq!(mouse_button("xbutton2"), Some(MouseButton::Five));
    }

    /// The ordinary buttons are refused, and the refusal names them.
    #[test]
    fn the_left_right_and_middle_buttons_are_refused_by_name() {
        for (spelling, what) in [
            ("Mouse1", "the left button"),
            ("mouseleft", "the left button"),
            ("Mouse2", "the right button"),
            ("Mouse3", "the middle button"),
        ] {
            assert_eq!(refused_mouse_button(spelling), Some(what), "{spelling}");
            assert!(mouse_button(spelling).is_none(), "{spelling}");
            assert!(names_a_mouse_button(spelling), "{spelling}");
        }
        // A watchable one is not "refused", which is the other half of the
        // predicate and the half a single table could not express.
        assert_eq!(refused_mouse_button("Mouse4"), None);
        assert!(names_a_mouse_button("Mouse4"));
        assert!(!names_a_mouse_button("Control"));
    }

    /// A mouse button in a chord is refused with the reason that is true of it.
    ///
    /// Not `NotAModifier`, whose message sends the reader to the platform's own
    /// shortcut API — which cannot take a mouse button at all, so following that
    /// advice would end at a second refusal.
    #[test]
    fn a_mouse_button_inside_a_chord_is_refused_and_does_not_blame_the_register() {
        let err = Chord::parse("Control+Mouse4").expect_err("must refuse");
        assert!(matches!(err, ChordError::MouseInAChord { .. }), "{err:?}");
        assert!(!format!("{err}").contains("can be registered"), "{err}");
    }

    /// A watchable button parses to a button, not to a chord of one.
    #[test]
    fn a_lone_mouse_button_parses_as_a_button() {
        assert_eq!(
            Watched::parse("Mouse4").expect("should parse"),
            Watched::Button(MouseButton::Four)
        );
        assert!(Watched::parse("Mouse4").expect("should parse").is_button());
        assert!(!Watched::parse("Control+Super")
            .expect("should parse")
            .is_button());

        // And a refused one comes back as refused rather than as "too short",
        // which is what `Chord::parse` alone would have said about it.
        let err = Watched::parse("Mouse1").expect_err("must refuse");
        assert!(
            matches!(err, ChordError::MouseButtonRefused { .. }),
            "{err:?}"
        );
    }

    /// A button fires its own index, once down and once up.
    #[test]
    fn a_watched_button_fires_on_the_way_down_and_on_the_way_up() {
        let mut w = HookWatcher::new(vec![
            Watched::Button(MouseButton::Four),
            Watched::Button(MouseButton::Five),
        ]);
        assert_eq!(w.button_down(MouseButton::Five), Fired::Pressed(1));
        assert_eq!(w.button_up(MouseButton::Five), Fired::Released(1));
        assert_eq!(w.button_down(MouseButton::Four), Fired::Pressed(0));
        assert_eq!(w.button_up(MouseButton::Four), Fired::Released(0));
    }

    /// A second down with no up in between is not a second press.
    ///
    /// Whether Windows ever sends one is unmeasured — `SendInput` cannot
    /// produce an auto-repeat to look at — so the watcher is written not to
    /// care either way.
    #[test]
    fn holding_a_button_reports_one_press_and_one_release() {
        let mut w = HookWatcher::new(vec![Watched::Button(MouseButton::Four)]);
        assert_eq!(w.button_down(MouseButton::Four), Fired::Pressed(0));
        for _ in 0..5 {
            assert_eq!(w.button_down(MouseButton::Four), Fired::Nothing);
        }
        assert_eq!(w.button_up(MouseButton::Four), Fired::Released(0));
        // And an up with no down before it is not an event.
        assert_eq!(w.button_up(MouseButton::Four), Fired::Nothing);
    }

    /// A button nobody bound reports nothing, and does not disturb the other.
    #[test]
    fn an_unwatched_button_is_not_an_event() {
        let mut w = HookWatcher::new(vec![Watched::Button(MouseButton::Four)]);
        assert_eq!(w.button_down(MouseButton::Five), Fired::Nothing);
        assert_eq!(w.button_up(MouseButton::Five), Fired::Nothing);
        assert_eq!(w.button_down(MouseButton::Four), Fired::Pressed(0));
    }

    /// One index space over both kinds, in the order the caller gave them.
    ///
    /// The property the whole `Watched` enum exists for: `shortcuts::Hooks`
    /// routes by index into the list it passed, and a chord at position 1
    /// reporting index 0 because it is the *first chord* would fire the wrong
    /// action.
    #[test]
    fn chords_and_buttons_share_the_callers_index_space() {
        let mut w = HookWatcher::new(vec![
            Watched::Button(MouseButton::Four),
            Watched::Chord(Chord::parse("Control+Super").expect("a valid chord")),
            Watched::Button(MouseButton::Five),
            Watched::Chord(Chord::parse("Alt+Shift").expect("a valid chord")),
        ]);

        assert_eq!(w.button_down(MouseButton::Four), Fired::Pressed(0));
        assert_eq!(w.button_up(MouseButton::Four), Fired::Released(0));

        w.key_down(Key::LeftControl);
        assert_eq!(w.key_down(Key::LeftSuper), Fired::Pressed(1));
        assert_eq!(w.key_up(Key::LeftSuper), Fired::Released(1));
        w.key_up(Key::LeftControl);

        assert_eq!(w.button_down(MouseButton::Five), Fired::Pressed(2));
        assert_eq!(w.button_up(MouseButton::Five), Fired::Released(2));

        w.key_down(Key::LeftAlt);
        assert_eq!(w.key_down(Key::LeftShift), Fired::Pressed(3));
        assert_eq!(w.key_up(Key::LeftAlt), Fired::Released(3));
    }

    /// The mouse hook is installed only when something needs it.
    #[test]
    fn a_configuration_with_no_button_does_not_ask_for_a_mouse_hook() {
        let chords_only = HookWatcher::new(vec![Watched::Chord(
            Chord::parse("Control+Super").expect("a valid chord"),
        )]);
        assert!(chords_only.is_watching());
        assert!(!chords_only.watches_a_button());

        let with_button = HookWatcher::new(vec![Watched::Button(MouseButton::Four)]);
        assert!(with_button.is_watching());
        assert!(with_button.watches_a_button());

        assert!(!HookWatcher::new(Vec::new()).is_watching());
    }

    /* ------------------------------------------------------- push-to-talk */

    /// A chord reports its release, which is the whole of what push-to-talk
    /// needs from this module.
    #[test]
    fn releasing_a_key_of_a_held_chord_reports_the_release() {
        let mut w = watcher(&["Control+Super"]);
        w.key_down(Key::LeftControl);
        assert_eq!(w.key_down(Key::LeftSuper), Fired::Pressed(0));
        assert_eq!(w.key_up(Key::LeftSuper), Fired::Released(0));
        // The second key going up is not a second release: the chord stopped
        // being held once, not twice.
        assert_eq!(w.key_up(Key::LeftControl), Fired::Activated(0));
    }

    /// Either key of the chord ends the hold, not only the last one pressed.
    #[test]
    fn whichever_key_goes_up_first_is_the_release() {
        let mut w = watcher(&["Control+Super"]);
        w.key_down(Key::LeftControl);
        assert_eq!(w.key_down(Key::LeftSuper), Fired::Pressed(0));
        assert_eq!(w.key_up(Key::LeftControl), Fired::Released(0));
    }

    /// An ordinary key coming up mid-hold is not the release.
    ///
    /// **The reason `pressed` is a separate field from `armed`.** `armed` is
    /// cleared by any key going up — it has to be, or holding a chord and
    /// tapping an arrow would never re-arm — so a shared field would have
    /// reported the arrow's release as the chord's and left the real release
    /// with nothing to report. Under push-to-talk that is a microphone nobody
    /// can close: the user lets go and nothing happens.
    #[test]
    fn an_unrelated_key_coming_up_during_a_hold_is_not_the_release() {
        let mut w = watcher(&["Control+Super"]);
        w.key_down(Key::LeftControl);
        assert_eq!(w.key_down(Key::LeftSuper), Fired::Pressed(0));

        // The arrow of Ctrl+Win+Left, down and up, with the chord still held.
        w.key_down(ARROW);
        assert_eq!(w.key_up(ARROW), Fired::Nothing);

        // And letting go of the chord still ends it.
        assert_eq!(w.key_up(Key::LeftSuper), Fired::Released(0));
    }

    /// A key going up when nothing was pressed is not a release.
    #[test]
    fn a_release_with_no_press_before_it_is_not_an_event() {
        let mut w = watcher(&["Control+Super"]);
        assert_eq!(w.key_up(Key::LeftSuper), Fired::Nothing);

        // Nor is a key that is not in the chord at all, held or otherwise.
        w.key_down(Key::LeftControl);
        assert_eq!(w.key_down(Key::LeftSuper), Fired::Pressed(0));
        assert_eq!(w.key_up(Key::RightShift), Fired::Nothing);
    }

    /// Press, release, press again: two of each, in order.
    #[test]
    fn a_second_hold_reports_a_second_press_and_a_second_release() {
        let mut w = watcher(&["Control+Super"]);
        w.key_down(Key::LeftControl);
        assert_eq!(w.key_down(Key::LeftSuper), Fired::Pressed(0));
        assert_eq!(w.key_up(Key::LeftSuper), Fired::Released(0));
        assert_eq!(w.key_down(Key::LeftSuper), Fired::Pressed(0));
        assert_eq!(w.key_up(Key::LeftSuper), Fired::Released(0));
    }

    /// A reset does not synthesise the release it is throwing away.
    ///
    /// It is called when the watcher has *lost track* of the keyboard. Claiming
    /// the key came up would stop a push-to-talk dictation on a key the user is
    /// still holding, which is a worse failure than the one being recovered
    /// from.
    #[test]
    fn a_reset_does_not_invent_a_release() {
        let mut w = watcher(&["Control+Super"]);
        w.key_down(Key::LeftControl);
        assert_eq!(w.key_down(Key::LeftSuper), Fired::Pressed(0));

        w.reset();
        assert_eq!(w.held_count(), 0);

        // The key that was down before the reset comes up afterwards, and it is
        // not paired with anything -- the press it belonged to was forgotten.
        assert_eq!(w.key_up(Key::LeftSuper), Fired::Nothing);
    }

    /// The prefix keystroke KNEG-115 measures, under push-to-talk.
    ///
    /// **Why the honest consequence is what it is.** `Ctrl+Win+Left` still
    /// produces one press and one release of `Control+Super` — the mode changes
    /// nothing about *when* the chord matches. What changes is the interval
    /// between them: a toggle records from this press to the next accidental
    /// one, which may be minutes; a hold records from this press to this
    /// release, which is however long the user took to reach the arrow key.
    /// That recording is far too short to hold speech, so the gate reports
    /// `InsufficientSpeech` and nothing is transcribed or pasted.
    ///
    /// The fire counts are measured here. That a recording of that length hits
    /// the gate is read from `kw_audio_core`'s gate and `Dictation::run`, not
    /// executed — the same distinction the KNEG-115 tests above draw.
    #[test]
    fn switching_virtual_desktop_is_one_press_and_one_release() {
        let mut w = watcher(&["Control+Super"]);
        let mut edges = Vec::new();
        for (key, down) in [
            (Key::LeftControl, true),
            (Key::LeftSuper, true),
            (ARROW, true),
            (ARROW, false),
            (Key::LeftSuper, false),
            (Key::LeftControl, false),
        ] {
            let fired = if down { w.key_down(key) } else { w.key_up(key) };
            if fired != Fired::Nothing {
                edges.push(fired);
            }
        }
        assert_eq!(edges, vec![Fired::Pressed(0), Fired::Released(0)]);
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod release_activation_tests {
    use super::*;
    fn watcher(name: &str) -> ChordWatcher {
        ChordWatcher::new(vec![Chord::parse(name).unwrap()])
    }
    #[test]
    fn a_deliberately_bound_longer_modifier_chord_can_still_activate() {
        let mut w = ChordWatcher::new(vec![
            Chord::parse("Control+Super").unwrap(),
            Chord::parse("Control+Super+Shift").unwrap(),
        ]);
        w.key_down(Key::LeftControl);
        w.key_down(Key::LeftSuper);
        w.key_down(Key::LeftShift);
        w.key_up(Key::LeftShift);
        w.key_up(Key::LeftSuper);
        assert_eq!(w.key_up(Key::LeftControl), Fired::Activated(1));
    }

    #[test]
    fn exact_gestures_activate_once_on_final_release() {
        for name in ["Control+Super", "LeftControl+LeftSuper"] {
            for (a, b) in [
                (Key::LeftControl, Key::LeftSuper),
                (Key::LeftSuper, Key::LeftControl),
            ] {
                let mut w = watcher(name);
                for _ in 0..2 {
                    assert_eq!(w.key_down(a), Fired::Nothing);
                    assert_eq!(w.key_down(b), Fired::Pressed(0));
                    assert_eq!(w.key_down(b), Fired::Nothing);
                    assert_eq!(w.key_up(a), Fired::Released(0));
                    assert_eq!(w.key_up(b), Fired::Activated(0));
                    assert_eq!(w.key_up(b), Fired::Nothing);
                }
            }
        }
    }
    #[test]
    fn any_intervening_key_cancels_until_every_key_is_up() {
        for extra in [
            Key::Other(37),
            Key::Other(39),
            Key::LeftShift,
            Key::RightControl,
        ] {
            for before in [true, false] {
                let mut w = watcher("Control+Super");
                if before {
                    w.key_down(extra);
                }
                w.key_down(Key::LeftControl);
                w.key_down(Key::LeftSuper);
                if !before {
                    w.key_down(extra);
                }
                w.key_up(extra);
                w.key_up(Key::LeftSuper);
                w.key_down(Key::LeftSuper);
                w.key_up(Key::LeftSuper);
                assert_eq!(w.key_up(Key::LeftControl), Fired::Nothing);
            }
        }
    }
    #[test]
    fn reset_discards_activation_and_hold_still_releases() {
        let mut w = watcher("Control+Super");
        w.key_down(Key::LeftControl);
        assert_eq!(w.key_down(Key::LeftSuper), Fired::Pressed(0));
        w.key_down(Key::Other(37));
        assert_eq!(w.key_up(Key::LeftSuper), Fired::Released(0));
        w.reset();
        assert_eq!(w.key_up(Key::LeftControl), Fired::Nothing);
    }
}
