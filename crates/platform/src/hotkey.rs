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

    /// Resolve an id back to an action. Returns `None` for anything unknown —
    /// an unrecognised action id must not fall through to dictation.
    #[must_use]
    pub fn from_id(id: &str) -> Option<Self> {
        ALL_ACTIONS.into_iter().find(|a| a.id() == id)
    }
}

impl fmt::Display for HotkeyAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.id())
    }
}

/// Modifier names the cross-platform accelerator syntax accepts.
/// Recognised modifier spellings, compared case-insensitively.
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
            Self::Standard { accelerator } | Self::ModifierOnly { accelerator, .. } => accelerator,
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
            Self::ModifierOnly { accelerator, .. } => {
                if crate::keyhook::support().is_available() {
                    format!(
                        "{accelerator} uses only modifier keys, so it is handled by a keyboard \
                         hook rather than the system shortcut register. It still works, and it \
                         does not stop the same keys reaching other applications."
                    )
                } else {
                    format!(
                        "{accelerator} uses only modifier keys. That needs a low-level keyboard \
                         hook, which this platform does not have in this build, so the shortcut \
                         does nothing. Add a regular key, for example {accelerator}+Space."
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

    let mut modifiers = Vec::new();
    let mut keys = Vec::new();
    for part in parts {
        let part = part.trim();
        if MODIFIERS.iter().any(|m| m.eq_ignore_ascii_case(part)) {
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

    Accelerator::Standard {
        accelerator: trimmed.to_owned(),
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
}

impl Binding {
    /// Build from a stored settings value.
    #[must_use]
    pub fn from_stored(action: HotkeyAction, stored: &str) -> Self {
        Self {
            action,
            accelerators: parse_list(stored).iter().map(|s| classify(s)).collect(),
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

    /// Whether this action has at least one working hotkey.
    #[must_use]
    pub fn is_usable(&self) -> bool {
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

            // The two sentences differ in what they can promise. One has read
            // the layout and can name the character; the other is telling
            // somebody what would happen if their layout has AltGr, and says so
            // rather than asserting it does.
            let reason = match altgr {
                AltGr::Present { example } => format!(
                    "{accelerator} also fires when you press {press}. This layout types {example:?}                      with Control and Alt together, and Windows sends left Control and right Alt                      for AltGr -- which completes this chord, with nothing downstream able to tell                      that apart from you pressing it deliberately."
                ),
                AltGr::Unknown { why } => format!(
                    "{accelerator} also fires when you press {press}, on any layout with an AltGr                      key -- Nordic, German and most non-US layouts. Windows sends left Control and                      right Alt for AltGr, which completes this chord. Whether this layout has one                      was not determined: {why}"
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
    fn a_binding_reports_only_its_working_accelerators() {
        let b = Binding::from_stored(HotkeyAction::Dictation, "Control+Super, Alt+D, Control+");
        assert_eq!(b.registrable(), vec!["Alt+D"]);
        assert!(b.is_usable());
        assert_eq!(
            b.problems().len(),
            2,
            "the modifier-only one and the invalid one"
        );
    }

    #[test]
    fn a_binding_with_nothing_registrable_is_not_usable() {
        let b = Binding::from_stored(HotkeyAction::Dictation, "Control+Super");
        assert!(!b.is_usable());
        assert!(b.registrable().is_empty());
        assert_eq!(b.problems().len(), 1);
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
        Binding::from_stored(action, accelerator)
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

    /* --------------------------------------------------------------------- conflicts */

    #[test]
    fn two_actions_on_the_same_accelerator_is_reported() {
        let bindings = vec![
            Binding::from_stored(HotkeyAction::Dictation, "Alt+D"),
            Binding::from_stored(HotkeyAction::Translation, "Alt+D, Control+T"),
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
            .map(|(i, a)| Binding::from_stored(*a, &format!("Alt+F{}", i + 1)))
            .collect();
        assert!(conflicts(&bindings).is_empty());
    }

    #[test]
    fn an_unregistrable_accelerator_cannot_cause_a_conflict() {
        // Two actions both defaulting to Control+Super is not a conflict worth
        // reporting, because neither is registered.
        let bindings = vec![
            Binding::from_stored(HotkeyAction::Dictation, "Control+Super"),
            Binding::from_stored(HotkeyAction::Meeting, "Control+Super"),
        ];
        assert!(conflicts(&bindings).is_empty());
    }
}
