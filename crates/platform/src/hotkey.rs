//! Hotkey slots, accelerator parsing, and honest reporting of what the
//! cross-platform shortcut API can and cannot register.
//!
//! Two things here are ports, and one is a correction.
//!
//! **Ported:** the comma-separated hotkey *list* format from upstream's
//! `hotkeyList.js`, including its genuinely subtle edge case — the comma key is
//! itself a valid hotkey (`Control+,`), so splitting on commas can tear an
//! accelerator apart. Upstream's fix, reproduced here, is that no accelerator
//! legitimately ends in `+`, so a segment ending in `+` whose successor is empty
//! gets its comma restored.
//!
//! **Ported:** the five action slots, with upstream's defaults.
//!
//! **Corrected:** upstream's default dictation hotkey is `Control+Super`, which
//! is *modifier-only*. No cross-platform global-shortcut API can register that —
//! it needs a per-platform low-level keyboard hook, which upstream provides via
//! one native listener process per watched key. Rather than register it, fail
//! silently, and leave the user pressing a dead key, [`classify`] reports
//! modifier-only accelerators as [`Accelerator::ModifierOnly`] so the UI can say
//! what is actually happening.
//!
//! **Since then:** [`crate::keyhook`] provides that hook on Windows, in-process
//! and as one hook rather than one child per key. `ModifierOnly` is still the
//! right classification — it says *which mechanism handles this* — and
//! [`Accelerator::explain`] now asks the hook whether it exists here rather than
//! asserting it does not.
//!
//! **And since that:** the same hook thread carries a `WH_MOUSE_LL` hook, so
//! there is a third mechanism and therefore a third classification —
//! [`Accelerator::MouseButton`] (KNEG-64). It is a variant rather than a
//! `ModifierOnly` with an odd spelling in it because the *sentence* differs:
//! a hooked chord takes nothing away from anything else, and a hooked mouse
//! button leaves the click going Back in every browser on the machine.
//!
//! [`HotkeyMode`] arrives with it and is orthogonal to all three: it says
//! whether an accelerator toggles or is held, and the routing that reads it is
//! `shortcuts::act` rather than anything here. Its default is deliberately the
//! shipped one — see the type's own documentation, which is where the reason
//! belongs.

use std::collections::BTreeSet;
use std::fmt;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The separator used in stored hotkey lists.
const LIST_SEPARATOR: char = ',';

/// The five things a hotkey can trigger.
///
/// Upstream names these `DICTATION_KEY`, `CHAT_AGENT_KEY`, `VOICE_AGENT_KEY`,
/// `TRANSLATION_KEY`, and `MEETING_KEY`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum HotkeyAction {
    Dictation,
    ChatAgent,
    VoiceAgent,
    Translation,
    Meeting,
}

/// Every action, so a caller cannot forget one.
pub const ALL_ACTIONS: [HotkeyAction; 5] = [
    HotkeyAction::Dictation,
    HotkeyAction::ChatAgent,
    HotkeyAction::VoiceAgent,
    HotkeyAction::Translation,
    HotkeyAction::Meeting,
];

impl HotkeyAction {
    /// The stable identifier used in settings keys and event names.
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::Dictation => "dictation",
            Self::ChatAgent => "chat-agent",
            Self::VoiceAgent => "voice-agent",
            Self::Translation => "translation",
            Self::Meeting => "meeting",
        }
    }

    /// The settings key holding this action's hotkey list.
    #[must_use]
    pub fn settings_key(self) -> String {
        format!("hotkey.{}", self.id())
    }

    /// Upstream's default, carried over so a migrated user's muscle memory works.
    ///
    /// Note that the dictation default is modifier-only; see [`classify`].
    #[must_use]
    pub const fn default_accelerator(self) -> &'static str {
        match self {
            Self::Dictation => "Control+Super",
            Self::ChatAgent => "Control+Shift+Space",
            Self::VoiceAgent => "Control+Shift+V",
            Self::Translation => "Control+Shift+T",
            Self::Meeting => "Control+Shift+M",
        }
    }

    /// The settings key holding whether this action is held or toggled.
    ///
    /// A second key rather than a second field in the accelerator string,
    /// because the two are edited by different controls and change for
    /// different reasons: a user rebinding a key is not thereby choosing
    /// push-to-talk, and a stored accelerator that has to be parsed for a mode
    /// as well is a format nobody can type.
    #[must_use]
    pub fn mode_settings_key(self) -> String {
        format!("hotkey.{}.mode", self.id())
    }

    /// Resolve an id back to an action. Returns `None` for anything unknown —
    /// an unrecognised action id must not fall through to dictation.
    #[must_use]
    pub fn from_id(id: &str) -> Option<Self> {
        ALL_ACTIONS.into_iter().find(|a| a.id() == id)
    }
}

/// Whether a hotkey toggles on each press or acts for as long as it is held.
///
/// # The default is `Toggle`, and that is deliberate, not inertia
///
/// Push-to-talk is one of the answers to **KNEG-115** — a two-modifier chord
/// fires on the way into any longer shortcut, and a chord that acts on release
/// cannot fire as a prefix in the damaging way, because the accidental press
/// and the accidental release bracket almost no audio instead of bracketing
/// everything the user did in between. That makes `Hold` a candidate for the
/// shipped default, and **shipping it as the default here would settle KNEG-115
/// in passing**, which is exactly the failure that issue is about. The mode
/// exists; the default is unchanged; the choice stays with the repo owner.
///
/// # What holding costs, said here rather than found out
///
/// A hold shorter than a syllable produces a recording shorter than a syllable.
/// The audio gate then reports `InsufficientSpeech` and nothing is transcribed
/// — which is correct behaviour and is also, under push-to-talk, a **routine**
/// outcome rather than the rare one it is under a toggle. Every mistimed tap,
/// every accidental prefix, every "I let go too early" ends that way. A user
/// who reads `InsufficientSpeech` as a fault will go looking for one.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize, TS,
)]
#[serde(rename_all = "kebab-case")]
pub enum HotkeyMode {
    /// Press to start, press again to stop. The shipped behaviour.
    #[default]
    Toggle,
    /// Recording lasts as long as the key is held. Push-to-talk.
    Hold,
}

/// Every mode, so a caller — and a settings screen — cannot forget one.
pub const ALL_MODES: [HotkeyMode; 2] = [HotkeyMode::Toggle, HotkeyMode::Hold];

impl HotkeyMode {
    /// The stable identifier stored in settings.
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::Toggle => "toggle",
            Self::Hold => "hold",
        }
    }

    /// Resolve a stored value back to a mode.
    ///
    /// `None` for anything unrecognised rather than a silent fallback: a stored
    /// value nobody wrote is a fact worth noticing, and the caller decides what
    /// to do with an unreadable setting.
    #[must_use]
    pub fn from_id(id: &str) -> Option<Self> {
        ALL_MODES.into_iter().find(|m| m.id() == id)
    }

    /// Read a stored value, falling back to the shipped default.
    ///
    /// The fallback is where an empty or corrupt setting lands, and it lands on
    /// [`HotkeyMode::Toggle`] — the behaviour that shipped — rather than on
    /// whatever was most recently added.
    #[must_use]
    pub fn from_stored(stored: &str) -> Self {
        Self::from_id(stored.trim()).unwrap_or_default()
    }
}

impl fmt::Display for HotkeyMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.id())
    }
}

impl fmt::Display for HotkeyAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.id())
    }
}

/// Modifier spellings the cross-platform accelerator syntax accepts, compared
/// case-insensitively.
///
/// **Unsided only.** The sided spellings live in
/// [`crate::keys::SIDED_MODIFIERS`] and are recognised by [`classify`] through
/// it, because a chord naming a side can never go to the system register —
/// see [`is_modifier`].
///
/// `Ctrl` earns its place by measurement, not taste: the shortcut plugin
/// registers `Ctrl+Shift+D` happily, and without this entry `Ctrl` counts as a
/// key, so the whole accelerator was rejected as "it names 2 non-modifier keys".
/// Being stricter than the platform is not caution — it refuses a shortcut that
/// works, and blames the user's spelling for it. `Win` is deliberately absent:
/// the plugin rejects that one too, so refusing it agrees with reality.
const MODIFIERS: [&str; 9] = [
    "Control",
    "Ctrl",
    "CommandOrControl",
    "CmdOrCtrl",
    "Command",
    "Super",
    "Meta",
    "Alt",
    "Shift",
];

/// Whether `part` is a modifier at all, either side or one named side.
///
/// Two sources, deliberately. The unsided spellings are the ones the shortcut
/// plugin takes, so this list is answerable to the plugin and diverges from the
/// chord parser where the plugin does (`Win` is absent because the plugin
/// rejects it). The sided ones are answerable to the chord parser instead,
/// because the register cannot express them at all and the hook is the only
/// thing that will ever see them — so they come from
/// [`crate::keys::SIDED_MODIFIERS`] rather than being spelled again here.
fn is_modifier(part: &str) -> bool {
    MODIFIERS.iter().any(|m| m.eq_ignore_ascii_case(part))
        || crate::keys::sided_modifier(part).is_some()
}

/// What an accelerator string can be registered as.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum Accelerator {
    /// Has at least one non-modifier key, so the cross-platform API can take it.
    Standard { accelerator: String },

    /// Only modifiers. Live only through a low-level keyboard hook, which exists
    /// on Windows ([`crate::keyhook`]) and nowhere else yet.
    ///
    /// Reported rather than swallowed: a hotkey the cross-platform API cannot
    /// take must not look registered. Whether the hook picks it up is a separate
    /// question, and [`Accelerator::explain`] asks rather than assuming.
    ModifierOnly {
        accelerator: String,
        modifiers: Vec<String>,
    },

    /// One mouse side button, live only through the low-level mouse hook.
    ///
    /// Its own variant rather than a `ModifierOnly` with a strange spelling in
    /// it. A mouse button is not a modifier and [`crate::keys::Chord::parse`]
    /// refuses fewer than two keys, so folding it in would mean loosening the
    /// chord rules for something that is not a chord — and the two need
    /// different sentences anyway, because what a hooked chord *also* does is
    /// nothing and what a hooked mouse button also does is go Back.
    MouseButton {
        accelerator: String,
        /// What the click still does elsewhere, because the hook does not
        /// consume it. `Back` or `Forward`.
        also_does: String,
    },

    /// Not a usable accelerator at all.
    Invalid { input: String, reason: String },
}

impl Accelerator {
    /// Whether `tauri-plugin-global-shortcut` can register this today.
    #[must_use]
    pub const fn is_registrable(&self) -> bool {
        matches!(self, Self::Standard { .. })
    }

    /// The accelerator as text, whatever the classification.
    ///
    /// `Invalid` yields the raw input rather than a canonical form: the thing
    /// worth showing beside "that is not a key" is what was actually typed.
    #[must_use]
    pub fn text(&self) -> &str {
        match self {
            Self::Standard { accelerator }
            | Self::ModifierOnly { accelerator, .. }
            | Self::MouseButton { accelerator, .. } => accelerator,
            Self::Invalid { input, .. } => input,
        }
    }

    /// A short phrase for the settings UI. Never claims a modifier-only hotkey
    /// works.
    #[must_use]
    pub fn explain(&self) -> String {
        match self {
            Self::Standard { accelerator } => format!("{accelerator} is registered"),
            // The advice depends on the platform, because the answer does. A
            // Windows build has the hook; the others do not, and telling a
            // Windows user to add a regular key would be telling them to work
            // around something that works.
            Self::ModifierOnly {
                accelerator,
                modifiers,
            } => {
                if crate::keyhook::support().is_available() {
                    format!(
                        "{accelerator} uses only modifier keys, so it is handled by a keyboard \
                         hook rather than the system shortcut register. It still works, and it \
                         does not stop the same keys reaching other applications."
                    )
                } else if modifiers.iter().any(|m| is_sided(m)) {
                    // The fallback advice below is wrong for a sided chord:
                    // adding a regular key to one makes it `Invalid`, because
                    // the register cannot see a side at all. Telling someone to
                    // do a thing the field then refuses is the defect KNEG-35
                    // was mostly about, so it is not repeated here.
                    format!(
                        "{accelerator} names one side of a key, which only a low-level keyboard \
                         hook can tell apart, and this platform does not have one in this build. \
                         There is no accelerator to fall back to here: the system shortcut \
                         register cannot see which side a modifier is on. Write the modifiers \
                         without a side to have it work anywhere."
                    )
                } else {
                    format!(
                        "{accelerator} uses only modifier keys. That needs a low-level keyboard \
                         hook, which this platform does not have in this build, so the shortcut \
                         does nothing. Add a regular key, for example {accelerator}+Space."
                    )
                }
            }
            // The sentence differs from the modifier-only one in the part that
            // matters. A hooked chord costs nothing elsewhere; a hooked mouse
            // button costs whatever that button already did, and on the two
            // side buttons that is Back and Forward in every browser on the
            // machine. Saying so here is the only place a user will read it
            // before pressing it.
            Self::MouseButton {
                accelerator,
                also_does,
            } => {
                if crate::keyhook::support().is_available() {
                    format!(
                        "{accelerator} is a mouse button, so it is watched by a low-level mouse \
                         hook — the system shortcut register cannot take a mouse button at all. \
                         It works, and the click is not swallowed: wherever the pointer is, that \
                         button still does {also_does}."
                    )
                } else {
                    format!(
                        "{accelerator} is a mouse button, which needs a low-level mouse hook. \
                         This platform does not have one in this build, so the binding does \
                         nothing. Use a keyboard shortcut here."
                    )
                }
            }
            Self::Invalid { input, reason } => {
                format!("{input:?} is not a valid shortcut: {reason}")
            }
        }
    }
}

/// Classify one accelerator string.
#[must_use]
pub fn classify(input: &str) -> Accelerator {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Accelerator::Invalid {
            input: input.to_owned(),
            reason: "it is empty".to_owned(),
        };
    }

    // A trailing `+` is how the comma key is written (`Control++` is not a
    // thing, but `Control+` followed by a comma is) -- so handle the comma key
    // explicitly before splitting on `+`.
    let parts: Vec<&str> = if trimmed.ends_with("+,") {
        let head = trimmed.trim_end_matches(",");
        let mut p: Vec<&str> = head.trim_end_matches('+').split('+').collect();
        p.push(",");
        p
    } else {
        trimmed.split('+').collect()
    };

    if parts.iter().any(|p| p.trim().is_empty()) {
        return Accelerator::Invalid {
            input: input.to_owned(),
            reason: "it has an empty segment, so a key or modifier is missing".to_owned(),
        };
    }

    // A mouse button is decided before anything else, because otherwise
    // `Mouse4` is a perfectly ordinary one-key accelerator: no modifier, one
    // key, no side named -- `Standard`. It would be handed to the shortcut
    // plugin, refused by a parser that has never heard of a mouse, and reported
    // as a spelling mistake.
    if let Some(mouse) = classify_mouse(input, &parts) {
        return mouse;
    }

    let mut modifiers = Vec::new();
    let mut keys = Vec::new();
    for part in parts {
        let part = part.trim();
        if is_modifier(part) {
            modifiers.push(part.to_owned());
        } else {
            keys.push(part.to_owned());
        }
    }

    if keys.is_empty() {
        return Accelerator::ModifierOnly {
            accelerator: trimmed.to_owned(),
            modifiers,
        };
    }

    if keys.len() > 1 {
        return Accelerator::Invalid {
            input: input.to_owned(),
            reason: format!(
                "it names {} non-modifier keys; a shortcut may have one",
                keys.len()
            ),
        };
    }

    // A side plus an ordinary key is the one combination neither mechanism can
    // take, and it must not fall through to `Standard`. The system register is
    // the only thing that handles an ordinary key and it cannot see which side a
    // modifier is on; the hook is the only thing that can see the side and
    // `keys::Chord::parse` refuses anything that is not a modifier. Reported
    // with both ways out rather than a bare refusal — unlike the AltGr caution,
    // this one has remedies that the field really accepts.
    if let Some(sided) = modifiers.iter().find(|m| is_sided(m)) {
        let key = keys.first().map_or("", String::as_str);
        return Accelerator::Invalid {
            input: input.to_owned(),
            reason: format!(
                "{sided} names one side of a key, and a shortcut naming a side has to be \
                 watched by the keyboard hook, which only ever matches modifiers. Either drop \
                 {key:?} and use modifiers alone, or write {} instead of {sided} to accept \
                 either side.",
                unsided_of(sided)
            ),
        };
    }

    Accelerator::Standard {
        accelerator: trimmed.to_owned(),
    }
}

/// The mouse verdict on an accelerator, or `None` if no part names a button.
///
/// Split out of [`classify`] because it has three answers and folding them into
/// that function's flow would have meant three more early returns in a function
/// that already had four. Reads [`crate::keys`]'s tables rather than its own, so
/// a spelling this accepts is a spelling `Watched::parse` resolves — the
/// property `SIDED_MODIFIERS` exists to hold for modifiers, applied to the mouse
/// for the same reason.
fn classify_mouse(input: &str, parts: &[&str]) -> Option<Accelerator> {
    use crate::keys::{mouse_button, names_a_mouse_button, refused_mouse_button};

    if !parts.iter().any(|p| names_a_mouse_button(p.trim())) {
        return None;
    }

    // More than one part means somebody wrote `Control+Mouse4`. Refused with
    // both facts, because the interesting one is not "we do not support it" but
    // that *no* mechanism could: the register cannot see a mouse button and the
    // hook's chord matcher only ever matches modifier keys.
    if parts.len() > 1 {
        return Some(Accelerator::Invalid {
            input: input.to_owned(),
            reason: "a mouse button is bound on its own. The system shortcut register cannot \
                     take a mouse button at all, and the keyboard hook only ever matches \
                     modifier keys, so nothing could hold the combination. Write just the \
                     button, for example Mouse4."
                .to_owned(),
        });
    }

    let only = parts.first()?.trim();

    if let Some(button) = mouse_button(only) {
        return Some(Accelerator::MouseButton {
            accelerator: only.to_owned(),
            also_does: button.also_does().to_owned(),
        });
    }

    // Left, right and middle. The refusal is the feature -- see
    // `keys::refused_mouse_button` -- so it says why rather than just no.
    refused_mouse_button(only).map(|what| Accelerator::Invalid {
        input: input.to_owned(),
        reason: format!(
            "{only} names {what}, which this build will not watch. The hook does not swallow \
             the click, so binding it would start this action on every click in every \
             application while the click still went through. Mouse4 and Mouse5, the two side \
             buttons, are the ones with no meaning to destroy."
        ),
    })
}

/// Whether this modifier spelling names one particular side.
fn is_sided(part: &str) -> bool {
    crate::keys::sided_modifier(part).is_some()
}

/// The either-side spelling to suggest in place of a sided one.
///
/// Falls back to the input rather than inventing a name: a spelling reaching
/// here is one [`crate::keys::SIDED_MODIFIERS`] resolved, so every case is
/// covered, and a wrong suggestion would be worse than an unhelpful one.
fn unsided_of(sided: &str) -> &str {
    match crate::keys::sided_modifier(sided) {
        Some(crate::keys::Key::LeftControl | crate::keys::Key::RightControl) => "Control",
        Some(crate::keys::Key::LeftShift | crate::keys::Key::RightShift) => "Shift",
        Some(crate::keys::Key::LeftAlt | crate::keys::Key::RightAlt) => "Alt",
        Some(crate::keys::Key::LeftSuper | crate::keys::Key::RightSuper) => "Super",
        Some(crate::keys::Key::Other(_)) | None => sided,
    }
}

/// Parse a stored hotkey value into a clean list.
///
/// Trimmed, de-duplicated, empties removed, order preserved. Accepts a single
/// accelerator or a comma-separated list, so a value written by an older version
/// still reads.
///
/// The comma key survives: `"Control+,,Control+Shift+P"` parses as two hotkeys,
/// not three.
#[must_use]
pub fn parse_list(value: &str) -> Vec<String> {
    let raw: Vec<&str> = value.split(LIST_SEPARATOR).collect();

    let mut seen = BTreeSet::new();
    let mut result = Vec::new();

    for (i, segment) in raw.iter().enumerate() {
        let mut hotkey = (*segment).trim().to_owned();

        // A non-final segment ending in `+` lost its comma key to the split, and
        // we know that happened because the split produced an empty successor.
        let stolen_comma = hotkey.ends_with('+')
            && i + 1 < raw.len()
            && raw.get(i + 1).is_some_and(|next| next.trim().is_empty());
        if stolen_comma {
            hotkey.push(LIST_SEPARATOR);
        }

        if hotkey.is_empty() || !seen.insert(hotkey.clone()) {
            continue;
        }
        result.push(hotkey);
    }

    result
}

/// Serialize a hotkey list back into its canonical stored form.
#[must_use]
pub fn serialize_list(hotkeys: &[String]) -> String {
    parse_list(&hotkeys.join(&LIST_SEPARATOR.to_string())).join(&LIST_SEPARATOR.to_string())
}

/// One action's configured hotkeys, classified.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
pub struct Binding {
    pub action: HotkeyAction,
    pub accelerators: Vec<Accelerator>,
    /// Whether these accelerators toggle or are held. See [`HotkeyMode`].
    pub mode: HotkeyMode,
}

impl Binding {
    /// Build from a stored settings value.
    ///
    /// The mode is a separate argument rather than being read from the same
    /// string: it lives in its own settings key, for the reasons
    /// [`HotkeyAction::mode_settings_key`] gives.
    #[must_use]
    pub fn from_stored(action: HotkeyAction, stored: &str, mode: HotkeyMode) -> Self {
        Self {
            action,
            accelerators: parse_list(stored).iter().map(|s| classify(s)).collect(),
            mode,
        }
    }

    /// Only the accelerators that can actually be registered.
    #[must_use]
    pub fn registrable(&self) -> Vec<&str> {
        self.accelerators
            .iter()
            .filter_map(|a| match a {
                Accelerator::Standard { accelerator } => Some(accelerator.as_str()),
                _ => None,
            })
            .collect()
    }

    /// Whether any of this action's accelerators is one
    /// `tauri-plugin-global-shortcut` can register today.
    ///
    /// **Not "whether this action has a working hotkey".** That is what this
    /// said until 2026-09-01, and on Windows it was false for both hooked
    /// kinds: a [`Accelerator::ModifierOnly`] chord and an
    /// [`Accelerator::MouseButton`] are unregistrable by construction and both
    /// fire through the low-level hook. [`Accelerator::explain`] says so in as
    /// many words — *"It still works"*, and *"It works, and the click is not
    /// swallowed"*. [`Accelerator::is_registrable`] had the honest wording all
    /// along; this converges on it.
    ///
    /// Whether an action ended up live is `shortcuts::RegistrationReport::is_live`,
    /// which counts `Hooked` as a success. This cannot answer that and never could.
    #[must_use]
    pub fn has_registrable(&self) -> bool {
        !self.registrable().is_empty()
    }

    /// Problems worth telling the user about.
    #[must_use]
    pub fn problems(&self) -> Vec<String> {
        self.accelerators
            .iter()
            .filter(|a| !a.is_registrable())
            .map(Accelerator::explain)
            .collect()
    }
}

/// A hotkey that works, and also does something the user did not ask for.
///
/// Deliberately not a [`Conflict`] and not a `problem`. Both of those mean the
/// hotkey will not do its job; this one will, and will *additionally* fire when
/// the user is doing something else entirely. Folding it into either would put
/// "this does not work" and "this works too often" under one word, and the
/// remedies are opposite.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Caution {
    pub action: HotkeyAction,
    pub accelerator: String,
    pub reason: String,
}

/// Every modifier-only accelerator in `bindings`, with what it also fires on.
///
/// **The measured defect (KNEG-115).** A two-modifier chord completes the
/// instant both keys are down — before the third key of a longer shortcut
/// arrives — so it goes off on the way past. `Control+Super` is the shipped
/// default and `Ctrl+Win+Left` switches virtual desktop, which means an ordinary
/// keystroke starts a dictation. Driving the shipped matcher with a real key
/// order measures it at one fire per switch, and `on_hotkey` is a **toggle**, so
/// two switches record the interval between them and paste the transcript
/// wherever the cursor is.
///
/// **This warns and changes nothing.** When a modifier-only chord should fire is
/// KNEG-115's open question, and every answer to it costs something: firing on
/// release adds the hold time to every dictation start, which is the latency
/// `ModifierOnly` exists to avoid. Choosing that in passing would be deciding
/// the behaviour of the shipped default on the way past — which is, with some
/// irony, exactly the failure being warned about. So the exposure is disclosed
/// and the decision is left where it belongs.
///
/// **Every modifier-only chord gets one**, not only the ones with a named
/// example, because the general statement is true of all of them and
/// [`crate::keys::COMMON_PREFIXES`] cannot be exhaustive. A caution that
/// appeared only for the five listed keystrokes would read as a clean bill of
/// health for the sixth.
#[must_use]
pub fn prefix_cautions(bindings: &[Binding]) -> Vec<Caution> {
    let mut found = Vec::new();
    for binding in bindings {
        for accelerator in &binding.accelerators {
            let Accelerator::ModifierOnly { accelerator, .. } = accelerator else {
                continue;
            };
            let Ok(chord) = crate::keys::Chord::parse(accelerator) else {
                continue;
            };

            let example = crate::keys::prefix_collision(&chord).map_or_else(String::new, |press| {
                format!(" On this platform that includes {press}.")
            });

            // Only for the action that is actually handled. Saying "recorded
            // and pasted" about a hotkey nothing dispatches would be inventing
            // a consequence -- see `commands::hotkeys`, which reports the other
            // four as unhandled rather than letting them look wired.
            let consequence = if binding.action == HotkeyAction::Dictation {
                " Dictation is a toggle, so two of those record everything in between and put \
                 the transcript wherever the cursor is."
            } else {
                ""
            };

            found.push(Caution {
                action: binding.action,
                accelerator: accelerator.clone(),
                reason: format!(
                    "{accelerator} fires as soon as both keys are down, so it also goes off on \
                     the way into any longer shortcut that starts with them.{example}\
                     {consequence}"
                ),
            });
        }
    }
    found
}

/// Every accelerator in `bindings` that an AltGr keystroke also fires.
///
/// **The measured defect (KNEG-37).** On a Nordic, German or most other non-US
/// layouts, `@ $ EUR { [` are typed with AltGr, and Windows has no AltGr
/// virtual key -- the layout synthesises **left Control plus right Alt**. A
/// modifier-only chord that those two keys complete therefore fires on every
/// such keystroke. Injecting exactly that pair started a dictation on the
/// 2026-08-26 bench run, and `cargo run -p kw-platform --example altgr_probe`
/// re-measures both halves on any machine.
///
/// **Which chords collide is asked of [`crate::keys::altgr_collision`]**, which
/// answers by running the real chord matcher over the keys a real AltGr press
/// sends. Reimplementing "does it name Control and Alt" here would have been
/// shorter and would have missed `Control+Shift`, which the probe reports and
/// which nobody predicted: AltGr's left Control arrives while Shift is already
/// held, so the chord is complete before the Alt is pressed.
///
/// **Two filters, and they do different jobs** -- worth stating because a
/// mutation test found the second one carrying none of the weight it looked
/// like it carried.
///
/// `Chord::parse` is what excludes a *registrable* accelerator: it refuses any
/// ordinary key, so `Control+Alt+D` never reaches the collision check. That is
/// deliberate. A chord with an ordinary key goes to the system shortcut
/// register rather than to the hook, and whether *that* also sees AltGr as
/// Control+Alt has not been measured by this project -- so it is not claimed
/// here. The question is its own issue rather than a guess on a screen.
///
/// The `ModifierOnly` match excludes an *invalid* one, which is a narrower and
/// less obvious case: `classify("Control+Alt+")` is `Invalid` (empty segment)
/// while `Chord::parse` skips the empty segment and happily returns
/// `Control+Alt`. Without the match, an accelerator that is broken and does
/// nothing at all would collect a warning about what it also does.
#[must_use]
pub fn cautions(bindings: &[Binding], altgr: &crate::keyboard_layout::AltGr) -> Vec<Caution> {
    use crate::keyboard_layout::AltGr;

    if !altgr.is_possible() {
        return Vec::new();
    }

    let mut found = Vec::new();
    for binding in bindings {
        for accelerator in &binding.accelerators {
            let Accelerator::ModifierOnly { accelerator, .. } = accelerator else {
                continue;
            };
            let Ok(chord) = crate::keys::Chord::parse(accelerator) else {
                continue;
            };
            let Some(press) = crate::keys::altgr_collision(&chord) else {
                continue;
            };

            // Suggested now that it can be typed. Until KNEG-35 closed,
            // `classify` read `RightControl` as a *key*, so the settings field
            // refused every sided spelling -- and this caution deliberately
            // named no remedy, because pointing at one the field then rejects
            // is worse than stating the problem and stopping. It accepts them
            // now. `sided_escape` builds the suggestion from the chord that
            // actually collided and re-runs the matcher over it, so the sentence
            // only appears when the escape really escapes.
            let remedy = crate::keys::sided_escape(&chord).map_or_else(String::new, |escape| {
                format!(" Naming one side avoids it entirely: {escape}.")
            });

            // The two sentences differ in what they can promise. One has read
            // the layout and can name the character; the other is telling
            // somebody what would happen if their layout has AltGr, and says so
            // rather than asserting it does.
            let reason = match altgr {
                AltGr::Present { example } => format!(
                    "{accelerator} also fires when you press {press}. This layout types \
                     {example:?} with Control and Alt together, and Windows sends left \
                     Control and right Alt for AltGr -- which completes this chord, with \
                     nothing downstream able to tell that apart from you pressing it \
                     deliberately.{remedy}"
                ),
                AltGr::Unknown { why } => format!(
                    "{accelerator} also fires when you press {press}, on any layout with \
                     an AltGr key -- Nordic, German and most non-US layouts. Windows sends \
                     left Control and right Alt for AltGr, which completes this chord. \
                     Whether this layout has one was not determined: {why}.{remedy}"
                ),
                // Filtered out above; matched rather than caught by a wildcard
                // so a fourth variant has to be thought about here.
                AltGr::Absent => continue,
            };

            found.push(Caution {
                action: binding.action,
                accelerator: accelerator.clone(),
                reason,
            });
        }
    }
    found
}

/// A conflict between two actions bound to the same accelerator.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
pub struct Conflict {
    pub accelerator: String,
    pub actions: Vec<HotkeyAction>,
}

/// Find accelerators claimed by more than one action.
///
/// Registration would otherwise fail for whichever action lost the race, which
/// is a confusing way to learn about a duplicate.
#[must_use]
pub fn conflicts(bindings: &[Binding]) -> Vec<Conflict> {
    let mut by_accelerator: std::collections::BTreeMap<String, Vec<HotkeyAction>> =
        std::collections::BTreeMap::new();

    for binding in bindings {
        for accelerator in binding.registrable() {
            by_accelerator
                .entry(accelerator.to_owned())
                .or_default()
                .push(binding.action);
        }
    }

    by_accelerator
        .into_iter()
        .filter(|(_, actions)| actions.len() > 1)
        .map(|(accelerator, actions)| Conflict {
            accelerator,
            actions,
        })
        .collect()
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

    /* -------------------------------------------------------------- the list format */

    #[test]
    fn every_classification_can_state_its_own_text() {
        assert_eq!(classify("Control+Shift+D").text(), "Control+Shift+D");
        assert_eq!(classify("Control+Super").text(), "Control+Super");
        // `Invalid` keeps what was typed rather than a canonical form: the thing
        // worth showing beside "a shortcut may have one key" is the input that
        // had two.
        assert_eq!(classify("Control+A+B").text(), "Control+A+B");
    }

    #[test]
    fn parses_a_single_accelerator() {
        assert_eq!(parse_list("Control+Shift+P"), vec!["Control+Shift+P"]);
    }

    #[test]
    fn parses_a_comma_separated_list() {
        assert_eq!(
            parse_list("Control+Shift+P, Alt+D"),
            vec!["Control+Shift+P", "Alt+D"]
        );
    }

    #[test]
    fn the_comma_key_survives_the_split() {
        // The edge case upstream documented: "Control+," is one hotkey, and
        // splitting on commas would otherwise leave a dangling "Control+".
        assert_eq!(parse_list("Control+,"), vec!["Control+,"]);
        assert_eq!(
            parse_list("Control+,,Control+Shift+P"),
            vec!["Control+,", "Control+Shift+P"]
        );
    }

    #[test]
    fn duplicates_are_removed_and_order_is_preserved() {
        assert_eq!(
            parse_list("Alt+D, Control+P, Alt+D, Control+P, Alt+X"),
            vec!["Alt+D", "Control+P", "Alt+X"]
        );
    }

    #[test]
    fn empty_and_whitespace_segments_are_dropped() {
        assert_eq!(parse_list(""), Vec::<String>::new());
        assert_eq!(parse_list("  ,, ,  "), Vec::<String>::new());
        assert_eq!(
            parse_list(" Alt+D , , Control+P "),
            vec!["Alt+D", "Control+P"]
        );
    }

    #[test]
    fn serialize_round_trips_through_parse() {
        for input in [
            "Control+Shift+P",
            "Control+,",
            "Control+,,Alt+D",
            "Alt+D,Control+P",
        ] {
            let parsed = parse_list(input);
            assert_eq!(
                parse_list(&serialize_list(&parsed)),
                parsed,
                "for {input:?}"
            );
        }
    }

    /* ---------------------------------------------------------------- classification */

    #[test]
    fn a_modifier_plus_a_key_is_registrable() {
        for input in [
            "Control+Shift+P",
            "Alt+D",
            "CommandOrControl+Space",
            "Super+F1",
            "Control+,",
        ] {
            let c = classify(input);
            assert!(
                c.is_registrable(),
                "{input} should be registrable, got {c:?}"
            );
        }
    }

    #[test]
    fn the_upstream_dictation_default_is_reported_as_modifier_only() {
        // This is the correction. Control+Super is upstream's default and no
        // cross-platform API can register it, so it must not look configured.
        let c = classify("Control+Super");
        match &c {
            Accelerator::ModifierOnly {
                accelerator,
                modifiers,
            } => {
                assert_eq!(accelerator, "Control+Super");
                assert_eq!(modifiers, &["Control".to_owned(), "Super".to_owned()]);
            }
            other => panic!("expected ModifierOnly, got {other:?}"),
        }
        assert!(!c.is_registrable());

        // The explanation names the mechanism either way, and then says
        // something true about *this* platform: on Windows the keyboard hook
        // exists and the shortcut works, elsewhere it does not and the advice is
        // to add a regular key. Asserting one wording on every platform would
        // have made this test outlive the fact it was checking.
        let explanation = c.explain();
        assert!(explanation.contains("keyboard hook"), "{explanation}");

        if crate::keyhook::support().is_available() {
            assert!(explanation.contains("still works"), "{explanation}");
            assert!(
                !explanation.contains("does nothing"),
                "a platform with the hook must not report the shortcut as dead: {explanation}"
            );
        } else {
            assert!(explanation.contains("does nothing"), "{explanation}");
        }
    }

    /* ------------------------------------------------------- sided modifiers */

    /// The whole of KNEG-35 in one assertion. `keys::Chord` has expressed these
    /// since the hook was written; `classify` read the first part as *the key*,
    /// so the first went to the system register -- the one mechanism that cannot
    /// see a side -- and the second was refused for naming two keys.
    #[test]
    fn a_sided_chord_is_hookable_rather_than_registrable_or_refused() {
        for accelerator in [
            "RightControl+Super",
            "RightControl+RightShift",
            "LeftControl+LeftAlt",
            "RightControl+Alt",
            "rcontrol+lshift",
        ] {
            match classify(accelerator) {
                Accelerator::ModifierOnly { .. } => {}
                other => panic!("{accelerator} should be hookable, got {other:?}"),
            }
        }
    }

    /// Every spelling the chord parser takes, the classifier takes. The two read
    /// one table so this cannot drift, and the test is what says so out loud --
    /// a shared constant that only one side actually consults would look
    /// identical from here.
    #[test]
    fn every_sided_spelling_the_chord_parser_takes_is_a_modifier_here() {
        for (spelling, _) in crate::keys::SIDED_MODIFIERS {
            let accelerator = format!("{spelling}+Super");
            assert!(
                crate::keys::Chord::parse(&accelerator).is_ok(),
                "{accelerator}: the chord parser refused its own table entry"
            );
            match classify(&accelerator) {
                Accelerator::ModifierOnly { .. } => {}
                other => panic!("{accelerator} should be hookable, got {other:?}"),
            }
        }
    }

    /// The combination neither mechanism can take, and the reason it must not
    /// fall through to `Standard`: the register would be handed a modifier whose
    /// side it cannot see. Before this it was `Invalid` for the wrong reason
    /// ("it names 2 non-modifier keys"), which blamed the spelling.
    #[test]
    fn a_side_plus_an_ordinary_key_is_refused_with_both_ways_out() {
        match classify("RightControl+D") {
            Accelerator::Invalid { reason, .. } => {
                assert!(reason.contains("one side"), "{reason}");
                assert!(
                    reason.contains("Control"),
                    "no unsided suggestion: {reason}"
                );
                assert!(
                    !reason.contains("non-modifier keys"),
                    "still blaming the spelling: {reason}"
                );
            }
            other => panic!("RightControl+D should be Invalid, got {other:?}"),
        }
    }

    /// A suggestion the field would refuse is worse than none, so the two the
    /// refusal offers are both checked against the classifier itself.
    #[test]
    fn both_ways_out_of_a_sided_key_combination_are_accepted() {
        assert!(matches!(
            classify("RightControl+LeftAlt"),
            Accelerator::ModifierOnly { .. }
        ));
        assert!(classify("Control+D").is_registrable());
    }

    /* -------------------------------------------------------- caution wording */

    /// The AltGr caution used to name no remedy because none could be typed.
    /// One can now, and it is built from the chord that collided rather than
    /// from a fixed example.
    #[test]
    fn the_altgr_caution_now_names_an_escape_that_works() {
        let layout = crate::keyboard_layout::AltGr::Present { example: '@' };
        let found = cautions(&[bound(HotkeyAction::Dictation, "Control+Alt")], &layout);

        assert_eq!(found.len(), 1, "{found:?}");
        let reason = &found[0].reason;
        assert!(reason.contains("Naming one side"), "{reason}");

        // The escape it names has to survive the same matcher that produced the
        // warning, and has to be something the settings field accepts.
        let escape = reason
            .rsplit_once(": ")
            .map(|(_, tail)| tail.trim_end_matches('.'))
            .unwrap_or_default();
        assert!(!escape.is_empty(), "no escape named: {reason}");
        let chord = crate::keys::Chord::parse(escape)
            .unwrap_or_else(|e| panic!("{escape} does not parse: {e}"));
        assert!(
            crate::keys::altgr_collision(&chord).is_none(),
            "{escape} still fires on AltGr"
        );
        assert!(
            matches!(classify(escape), Accelerator::ModifierOnly { .. }),
            "{escape} is not something the settings field accepts"
        );
    }

    /// A line continuation in a Rust string swallows the newline and the
    /// following indentation -- unless the backslash is lost, in which case the
    /// indentation becomes twenty spaces in the middle of a sentence on screen.
    /// Both AltGr cautions shipped that way and `cargo fmt` does not look inside
    /// a string literal, so nothing caught it. This does.
    #[test]
    fn no_caution_has_a_hole_where_a_line_continuation_should_be() {
        let layouts = [
            crate::keyboard_layout::AltGr::Present { example: '@' },
            crate::keyboard_layout::AltGr::Unknown {
                why: "the layout could not be read".to_owned(),
            },
        ];
        let bindings = [bound(HotkeyAction::Dictation, "Control+Alt")];

        let all = layouts
            .into_iter()
            .flat_map(|layout| cautions(&bindings, &layout))
            .chain(prefix_cautions(&bindings));

        for caution in all {
            assert!(
                !caution.reason.contains("  "),
                "a run of spaces in a sentence shown to a person: {}",
                caution.reason
            );
        }
    }

    /* ------------------------------------------------- prefix cautions */

    /// The shipped default is warned about, by name.
    ///
    /// `Control+Super` is what the application binds out of the box and
    /// `Ctrl+Win+Left` switches virtual desktop, so this is the sentence a user
    /// on the default sees. It has to name the keystroke: "any longer shortcut"
    /// alone is true and gives nobody anything to recognise.
    #[test]
    fn the_default_dictation_hotkey_says_what_else_fires_it() {
        let found = prefix_cautions(&[bound(HotkeyAction::Dictation, "Control+Super")]);
        assert_eq!(found.len(), 1, "{found:?}");
        let reason = &found[0].reason;
        assert!(reason.contains("Ctrl+Win+Left"), "{reason}");
        assert!(reason.contains("toggle"), "{reason}");
    }

    /// A chord with no listed keystroke is still warned about.
    ///
    /// `COMMON_PREFIXES` cannot be exhaustive — every two-modifier chord is a
    /// prefix of something. A caution that appeared only for the listed ones
    /// would read as a clean bill of health for the rest, which is the opposite
    /// of what is true.
    #[test]
    fn a_chord_with_no_named_example_is_cautioned_anyway() {
        let found = prefix_cautions(&[bound(HotkeyAction::Dictation, "Alt+Super")]);
        assert_eq!(found.len(), 1, "{found:?}");
        let reason = &found[0].reason;
        assert!(reason.contains("as soon as both keys are down"), "{reason}");
        assert!(
            !reason.contains("On this platform"),
            "no example exists, so none should be claimed: {reason}"
        );
    }

    /// An accelerator with an ordinary key in it is not affected.
    ///
    /// It goes to the system shortcut register, which arbitrates it against
    /// every other application and never reaches this matcher. Warning about it
    /// would be noise, and noise is what stops the real cautions being read.
    #[test]
    fn a_registrable_accelerator_gets_no_prefix_caution() {
        assert!(prefix_cautions(&[bound(HotkeyAction::Dictation, "Control+Shift+D")]).is_empty());
    }

    /// Only the action something dispatches is told what it costs.
    ///
    /// Four of the five actions have no handler, and `commands::hotkeys`
    /// reports them as unhandled rather than letting a bound key look wired.
    /// Telling one of those that it records and pastes would invent a
    /// consequence it does not have.
    #[test]
    fn an_unhandled_action_is_not_told_it_records_anything() {
        let found = prefix_cautions(&[bound(HotkeyAction::ChatAgent, "Control+Super")]);
        assert_eq!(found.len(), 1, "{found:?}");
        let reason = &found[0].reason;
        assert!(reason.contains("Ctrl+Win+Left"), "{reason}");
        assert!(
            !reason.contains("toggle"),
            "nothing dispatches this action, so it records nothing: {reason}"
        );
    }

    /// A sided chord is warned about too, and this is why it is worth saying.
    ///
    /// KNEG-35 made sided spellings typable and KNEG-37's AltGr caution now
    /// suggests one as a remedy. That remedy is right about AltGr and does
    /// nothing about this: `LeftControl+LeftSuper` is a prefix of
    /// `Ctrl+Win+Left` exactly as the unsided chord is. Somebody following the
    /// AltGr advice must not land here believing they are done.
    #[test]
    fn the_altgr_remedy_is_not_a_remedy_for_this() {
        let found = prefix_cautions(&[bound(HotkeyAction::Dictation, "LeftControl+LeftSuper")]);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(
            found[0].reason.contains("Ctrl+Win+Left"),
            "{}",
            found[0].reason
        );
    }

    #[test]
    fn every_action_default_is_classifiable_and_only_dictation_is_unregistrable() {
        for action in ALL_ACTIONS {
            let c = classify(action.default_accelerator());
            if action == HotkeyAction::Dictation {
                assert!(
                    !c.is_registrable(),
                    "dictation default is modifier-only upstream"
                );
            } else {
                assert!(c.is_registrable(), "{action} default {c:?} should register");
            }
        }
    }

    #[test]
    fn nonsense_accelerators_are_invalid_not_silently_accepted() {
        for (input, expect) in [
            ("", "empty"),
            ("   ", "empty"),
            ("Control+", "empty segment"),
            ("+P", "empty segment"),
            ("Control++P", "empty segment"),
            ("Control+A+B", "non-modifier keys"),
        ] {
            match classify(input) {
                Accelerator::Invalid { reason, .. } => {
                    assert!(reason.contains(expect), "{input:?}: {reason}");
                }
                other => panic!("{input:?} should be Invalid, got {other:?}"),
            }
        }
    }

    #[test]
    fn modifier_names_are_matched_case_insensitively() {
        assert!(!classify("control+super").is_registrable());
        assert!(classify("control+p").is_registrable());
    }

    /* ---------------------------------------------------------------------- bindings */

    #[test]
    fn a_binding_reports_only_its_registrable_accelerators() {
        let b = Binding::from_stored(
            HotkeyAction::Dictation,
            "Control+Super, Alt+D, Control+",
            HotkeyMode::Toggle,
        );
        assert_eq!(b.registrable(), vec!["Alt+D"]);
        assert!(b.has_registrable());
        assert_eq!(
            b.problems().len(),
            2,
            "the modifier-only one and the invalid one"
        );
    }

    #[test]
    fn a_binding_with_nothing_registrable_says_so() {
        let b = Binding::from_stored(HotkeyAction::Dictation, "Control+Super", HotkeyMode::Toggle);
        assert!(!b.has_registrable());
        assert!(b.registrable().is_empty());
        assert_eq!(b.problems().len(), 1);
    }

    /// `has_registrable` is about the register, and never about whether the
    /// hotkey works.
    ///
    /// The name it replaced — `is_usable` — claimed the second, and on Windows
    /// that was false for both hooked kinds: `Control+Super` is the shipped
    /// dictation default and it fires. This is the tripwire against the name
    /// drifting back to the claim it cannot support.
    ///
    /// Mutation that turns it red: widen [`Binding::registrable`]'s filter to
    /// accept `ModifierOnly`, and the `has_registrable` negatives fail. Delete
    /// the `Alt+D` control and a `from_stored` that parsed nothing at all would
    /// satisfy every remaining assertion — which is exactly what the control is
    /// here to stop.
    #[test]
    fn nothing_registrable_does_not_mean_nothing_works() {
        // The control. An accelerator the register does take, so the negatives
        // below are measured against a parser that is demonstrably working.
        let ordinary = Binding::from_stored(HotkeyAction::Dictation, "Alt+D", HotkeyMode::Toggle);
        assert!(ordinary.has_registrable());
        assert_eq!(ordinary.registrable(), vec!["Alt+D"]);

        for stored in ["Control+Super", "Mouse4"] {
            let b = Binding::from_stored(HotkeyAction::Dictation, stored, HotkeyMode::Toggle);
            // The premise: it parsed to one accelerator. Without this the two
            // assertions below hold for a binding with no accelerators at all.
            assert_eq!(b.accelerators.len(), 1, "{stored} did not parse to one");
            assert!(!b.has_registrable(), "{stored} is not registrable");

            // And where the hook exists it is live all the same. This is the
            // claim the old name got wrong, asserted rather than described.
            if crate::keyhook::support().is_available() {
                let problems = b.problems();
                assert_eq!(problems.len(), 1, "{stored}: {problems:?}");
                assert!(
                    !problems[0].contains("does nothing"),
                    "a platform with the hook must not call {stored} dead: {problems:?}"
                );
            }
        }
    }

    #[test]
    fn action_ids_round_trip_and_an_unknown_id_does_not_default_to_dictation() {
        for action in ALL_ACTIONS {
            assert_eq!(HotkeyAction::from_id(action.id()), Some(action));
        }
        // The important half: no silent fall-through.
        assert_eq!(HotkeyAction::from_id("dictation-agent"), None);
        assert_eq!(HotkeyAction::from_id(""), None);
        assert_eq!(HotkeyAction::from_id("Dictation"), None);
    }

    /* ----------------------------------------------------------- cautions */

    fn bound(action: HotkeyAction, accelerator: &str) -> Binding {
        Binding::from_stored(action, accelerator, HotkeyMode::Toggle)
    }

    #[test]
    fn a_layout_with_altgr_gets_a_warning_naming_the_character() {
        let layout = crate::keyboard_layout::AltGr::Present { example: '@' };
        let found = cautions(&[bound(HotkeyAction::Dictation, "Control+Alt")], &layout);

        assert_eq!(found.len(), 1);
        assert_eq!(found[0].accelerator, "Control+Alt");
        assert_eq!(found[0].action, HotkeyAction::Dictation);
        assert!(found[0].reason.contains('@'), "{}", found[0].reason);
        assert!(found[0].reason.contains("AltGr"), "{}", found[0].reason);
    }

    /// A layout that was read and has no AltGr gets nothing. This is the whole
    /// value of reading the layout rather than warning everyone: a US-layout
    /// user cannot press AltGr, so the warning would be noise.
    #[test]
    fn a_layout_without_altgr_is_left_alone() {
        let layout = crate::keyboard_layout::AltGr::Absent;
        assert!(cautions(&[bound(HotkeyAction::Dictation, "Control+Alt")], &layout).is_empty());
    }

    /// A layout nobody could read still warns, and says that it could not.
    ///
    /// Failing open here rather than closed: the cost of a warning on a layout
    /// that turns out not to have AltGr is a sentence someone ignores, and the
    /// cost of silence on a layout that does is the defect.
    #[test]
    fn an_unreadable_layout_warns_anyway_and_admits_it() {
        let layout = crate::keyboard_layout::AltGr::Unknown {
            why: "not on this platform".to_owned(),
        };
        let found = cautions(&[bound(HotkeyAction::Dictation, "Control+Alt")], &layout);
        assert_eq!(found.len(), 1);
        assert!(
            found[0].reason.contains("not on this platform"),
            "{}",
            found[0].reason
        );
        // And it must not claim to know a character it never looked one up for.
        assert!(!found[0].reason.contains('@'), "{}", found[0].reason);
    }

    /// A registrable accelerator is not examined, and that is a decision.
    ///
    /// `Control+Alt+D` goes to the system shortcut register rather than to the
    /// hook. Whether *that* also sees AltGr as Control+Alt is widely believed
    /// and has not been measured by this project, so it is not warned about --
    /// an unmeasured claim on a screen is the thing this repository keeps
    /// filing issues about. Tracked separately instead.
    ///
    /// What enforces it is `Chord::parse` refusing the ordinary key, not the
    /// `ModifierOnly` match above it. Measured, by deleting the match and
    /// finding this test still passed.
    #[test]
    fn a_standard_accelerator_is_not_claimed_about() {
        let layout = crate::keyboard_layout::AltGr::Present { example: '@' };
        assert!(cautions(&[bound(HotkeyAction::Dictation, "Control+Alt+D")], &layout).is_empty());
        assert!(crate::keys::Chord::parse("Control+Alt+D").is_err());
    }

    /// An accelerator that does nothing must not be told what else it does.
    ///
    /// This is the case the `ModifierOnly` match is actually for.
    /// `classify("Control+Alt+")` is `Invalid` -- an empty segment, so a key is
    /// missing -- while `Chord::parse` skips the empty segment and returns a
    /// perfectly good `Control+Alt`. Warning about the AltGr behaviour of a
    /// shortcut that is not registered at all would be advice about something
    /// that never happens, stacked on top of the real problem.
    #[test]
    fn an_invalid_accelerator_is_not_also_cautioned() {
        let layout = crate::keyboard_layout::AltGr::Present { example: '@' };
        let binding = bound(HotkeyAction::Dictation, "Control+Alt+");

        // The premise, asserted rather than assumed: it really is invalid, and
        // it really would parse as a chord if anything let it.
        assert!(matches!(
            binding.accelerators.first(),
            Some(Accelerator::Invalid { .. })
        ));
        assert!(crate::keys::Chord::parse("Control+Alt+").is_ok());

        assert!(cautions(&[binding], &layout).is_empty());
    }

    #[test]
    fn every_colliding_binding_is_reported_not_just_the_first() {
        let layout = crate::keyboard_layout::AltGr::Present { example: '@' };
        let found = cautions(
            &[
                bound(HotkeyAction::Dictation, "Control+Super"),
                bound(HotkeyAction::ChatAgent, "Control+Alt"),
                bound(HotkeyAction::Meeting, "Control+Shift"),
            ],
            &layout,
        );
        assert_eq!(found.len(), 2, "{found:?}");
        assert_eq!(found[0].action, HotkeyAction::ChatAgent);
        assert_eq!(found[1].action, HotkeyAction::Meeting);
    }

    #[test]
    fn settings_keys_are_namespaced_and_distinct() {
        let keys: BTreeSet<String> = ALL_ACTIONS.iter().map(|a| a.settings_key()).collect();
        assert_eq!(keys.len(), ALL_ACTIONS.len());
        assert!(keys.iter().all(|k| k.starts_with("hotkey.")));
    }

    /// The mode keys are one per action, distinct, and distinct from the
    /// accelerator keys.
    ///
    /// **Named in `config/settings-keys.json` and read by the settings gate**,
    /// which resolves `.mode_settings_key()` to a family of five and checks that
    /// this test exists. The last clause is the one that matters: `hotkey.x` and
    /// `hotkey.x.mode` are one keystroke apart, and a mode key colliding with an
    /// accelerator key would overwrite a binding rather than fail.
    #[test]
    fn mode_settings_keys_are_namespaced_and_distinct() {
        let modes: BTreeSet<String> = ALL_ACTIONS.iter().map(|a| a.mode_settings_key()).collect();
        assert_eq!(modes.len(), ALL_ACTIONS.len());
        assert!(modes.iter().all(|k| k.starts_with("hotkey.")));
        assert!(modes.iter().all(|k| k.ends_with(".mode")));

        let accelerators: BTreeSet<String> = ALL_ACTIONS.iter().map(|a| a.settings_key()).collect();
        assert!(modes.is_disjoint(&accelerators));
    }

    /// Every mode round-trips through its stored id, and nothing else resolves.
    #[test]
    fn a_mode_survives_the_trip_through_settings() {
        for mode in ALL_MODES {
            assert_eq!(HotkeyMode::from_id(mode.id()), Some(mode), "{mode}");
            assert_eq!(HotkeyMode::from_stored(mode.id()), mode, "{mode}");
            assert_eq!(HotkeyMode::from_stored(&format!("  {}  ", mode.id())), mode);
        }
        assert_eq!(HotkeyMode::from_id("push-to-talk"), None);
        assert_eq!(HotkeyMode::from_id(""), None);
    }

    /// An unreadable stored value lands on the shipped behaviour.
    ///
    /// Both halves matter. `from_id` refusing is what stops a *renderer* asking
    /// for a mode nobody wrote and being told it worked; `from_stored` falling
    /// back is what stops an empty or corrupt row turning every hotkey into
    /// something the user did not choose. They are different questions and they
    /// deliberately have different answers.
    #[test]
    fn an_unreadable_stored_mode_falls_back_to_the_shipped_one() {
        assert_eq!(HotkeyMode::from_stored(""), HotkeyMode::Toggle);
        assert_eq!(HotkeyMode::from_stored("Hold"), HotkeyMode::Toggle);
        assert_eq!(HotkeyMode::from_stored("nonsense"), HotkeyMode::Toggle);
        assert_eq!(HotkeyMode::default(), HotkeyMode::Toggle);
    }

    /// The shipped default is toggle for every action, and this test is the
    /// tripwire that says so out loud.
    ///
    /// **KNEG-115 is not settled here.** Push-to-talk is one of its candidate
    /// answers, and shipping `Hold` as the dictation default would decide that
    /// open question as a side effect of adding a mode — which is, precisely,
    /// the class of failure KNEG-115 describes. If this test ever fails, the
    /// commit that changed it is deciding something that belongs to the repo
    /// owner, and should say so.
    #[test]
    fn nothing_ships_holding_by_default() {
        for action in ALL_ACTIONS {
            let binding = Binding::from_stored(
                action,
                action.default_accelerator(),
                HotkeyMode::from_stored(""),
            );
            assert_eq!(binding.mode, HotkeyMode::Toggle, "{action}");
        }
        assert_eq!(
            HotkeyAction::Dictation.default_accelerator(),
            "Control+Super",
            "the shipped dictation accelerator changed, which KNEG-115 is also about"
        );
    }

    /* ---------------------------------------------------------- mouse buttons */

    /// A side button classifies as a mouse button, not as an ordinary key.
    ///
    /// Without the interception in `classify`, `Mouse4` is one non-modifier
    /// part with no side named — a textbook `Standard` accelerator. It would go
    /// to the shortcut plugin, be refused by a parser that has never heard of a
    /// mouse, and be reported to the user as a spelling mistake.
    #[test]
    fn a_side_button_is_its_own_classification() {
        for (spelling, also) in [
            ("Mouse4", "Back"),
            ("mouse5", "Forward"),
            ("XButton1", "Back"),
            ("xbutton2", "Forward"),
        ] {
            match classify(spelling) {
                Accelerator::MouseButton {
                    accelerator,
                    also_does,
                } => {
                    assert_eq!(accelerator, spelling);
                    assert_eq!(also_does, also, "{spelling}");
                }
                other => panic!("{spelling} classified as {other:?}"),
            }
        }
    }

    /// A mouse button is never registrable, and the explanation says why not
    /// and what it costs.
    #[test]
    fn a_mouse_button_is_hooked_rather_than_registered_and_says_what_it_also_does() {
        let mouse = classify("Mouse4");
        assert!(!mouse.is_registrable());
        assert_eq!(mouse.text(), "Mouse4");

        let explanation = mouse.explain();
        // The half that is true on every platform: the register cannot take it.
        assert!(explanation.contains("mouse button"), "{explanation}");
        // And where a hook exists, the cost of not swallowing the click. That
        // sentence is the whole reason this is a separate variant from
        // `ModifierOnly`, whose equivalent cost is nothing.
        if crate::keyhook::support().is_available() {
            assert!(explanation.contains("Back"), "{explanation}");
        } else {
            assert!(explanation.contains("does nothing"), "{explanation}");
        }
    }

    /// The ordinary buttons are refused, and the refusal explains itself.
    #[test]
    fn the_left_button_cannot_be_bound_and_the_reason_is_given() {
        for spelling in ["Mouse1", "MouseLeft", "Mouse2", "Mouse3"] {
            match classify(spelling) {
                Accelerator::Invalid { reason, .. } => {
                    assert!(reason.contains("button"), "{spelling}: {reason}");
                    // Not a bare refusal: it names the two that do work.
                    assert!(reason.contains("Mouse4"), "{spelling}: {reason}");
                }
                other => panic!("{spelling} classified as {other:?}"),
            }
        }
    }

    /// A mouse button combined with anything is refused, and says no mechanism
    /// could hold it — rather than implying a different spelling would work.
    #[test]
    fn a_mouse_button_cannot_be_combined_with_a_key_or_a_modifier() {
        for spelling in ["Control+Mouse4", "Mouse4+D", "Shift+Alt+Mouse5"] {
            match classify(spelling) {
                Accelerator::Invalid { reason, .. } => {
                    assert!(reason.contains("on its own"), "{spelling}: {reason}");
                }
                other => panic!("{spelling} classified as {other:?}"),
            }
        }
    }

    /// The classifier and the hook parser accept the same set.
    ///
    /// The mouse half of the property `SIDED_MODIFIERS` exists to hold, and the
    /// one that KNEG-35 was: a spelling one side takes and the other refuses is
    /// a shortcut the screen calls hookable and the hook then declines.
    #[test]
    fn every_spelling_the_classifier_takes_is_one_the_hook_parser_takes() {
        for (spelling, _) in crate::keys::MOUSE_BUTTONS {
            assert!(
                matches!(classify(spelling), Accelerator::MouseButton { .. }),
                "classify refuses {spelling}"
            );
            assert!(
                crate::keys::Watched::parse(spelling).is_ok(),
                "the hook parser refuses {spelling}"
            );
        }
        // And the refused ones are refused on both sides, not just one.
        for spelling in ["Mouse1", "Mouse2", "Mouse3"] {
            assert!(matches!(classify(spelling), Accelerator::Invalid { .. }));
            assert!(crate::keys::Watched::parse(spelling).is_err());
        }
    }

    /// A mouse button is not a modifier-only chord, and the two must not be
    /// confused by the caution machinery.
    ///
    /// `prefix_cautions` and `cautions` both filter on `ModifierOnly`, and a
    /// mouse button reaching either would collect a sentence about AltGr or
    /// about longer shortcuts that starts with the same keys — neither of which
    /// means anything about a mouse.
    #[test]
    fn a_mouse_binding_collects_no_chord_caution() {
        let bindings = vec![Binding::from_stored(
            HotkeyAction::Dictation,
            "Mouse4",
            HotkeyMode::Hold,
        )];
        assert!(prefix_cautions(&bindings).is_empty());
        assert!(cautions(
            &bindings,
            &crate::keyboard_layout::AltGr::Present { example: '@' }
        )
        .is_empty());
    }

    /* --------------------------------------------------------------------- conflicts */

    #[test]
    fn two_actions_on_the_same_accelerator_is_reported() {
        let bindings = vec![
            Binding::from_stored(HotkeyAction::Dictation, "Alt+D", HotkeyMode::Toggle),
            Binding::from_stored(
                HotkeyAction::Translation,
                "Alt+D, Control+T",
                HotkeyMode::Toggle,
            ),
        ];
        let found = conflicts(&bindings);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].accelerator, "Alt+D");
        assert_eq!(
            found[0].actions,
            vec![HotkeyAction::Dictation, HotkeyAction::Translation]
        );
    }

    #[test]
    fn distinct_accelerators_do_not_conflict() {
        let bindings: Vec<Binding> = ALL_ACTIONS
            .iter()
            .enumerate()
            .map(|(i, a)| Binding::from_stored(*a, &format!("Alt+F{}", i + 1), HotkeyMode::Toggle))
            .collect();
        assert!(conflicts(&bindings).is_empty());
    }

    #[test]
    fn an_unregistrable_accelerator_cannot_cause_a_conflict() {
        // Two actions both defaulting to Control+Super is not a conflict worth
        // reporting, because neither is registered.
        let bindings = vec![
            Binding::from_stored(HotkeyAction::Dictation, "Control+Super", HotkeyMode::Toggle),
            Binding::from_stored(HotkeyAction::Meeting, "Control+Super", HotkeyMode::Toggle),
        ];
        assert!(conflicts(&bindings).is_empty());
    }
}
