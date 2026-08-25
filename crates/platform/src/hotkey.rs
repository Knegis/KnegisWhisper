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

use std::collections::BTreeSet;
use std::fmt;

use serde::{Deserialize, Serialize};

/// The separator used in stored hotkey lists.
const LIST_SEPARATOR: char = ',';

/// The five things a hotkey can trigger.
///
/// Upstream names these `DICTATION_KEY`, `CHAT_AGENT_KEY`, `VOICE_AGENT_KEY`,
/// `TRANSLATION_KEY`, and `MEETING_KEY`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
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
const MODIFIERS: [&str; 8] = [
    "Control",
    "CommandOrControl",
    "CmdOrCtrl",
    "Command",
    "Super",
    "Meta",
    "Alt",
    "Shift",
];

/// What an accelerator string can be registered as.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum Accelerator {
    /// Has at least one non-modifier key, so the cross-platform API can take it.
    Standard { accelerator: String },

    /// Only modifiers. Registrable only through a low-level keyboard hook, which
    /// this application does not yet have on any platform.
    ///
    /// Reported rather than swallowed: a hotkey that cannot be registered must
    /// not look configured.
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

    /// A short phrase for the settings UI. Never claims a modifier-only hotkey
    /// works.
    #[must_use]
    pub fn explain(&self) -> String {
        match self {
            Self::Standard { accelerator } => format!("{accelerator} is registered"),
            Self::ModifierOnly { accelerator, .. } => format!(
                "{accelerator} uses only modifier keys. That needs a low-level keyboard \
                 hook, which is not implemented yet, so this shortcut does nothing. \
                 Add a regular key, for example {accelerator}+Space."
            ),
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
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

/// A conflict between two actions bound to the same accelerator.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
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

        // And the explanation says what is wrong and how to fix it, rather than
        // going quiet.
        let explanation = c.explain();
        assert!(explanation.contains("does nothing"), "{explanation}");
        assert!(explanation.contains("keyboard hook"), "{explanation}");
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
