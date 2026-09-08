//! The macOS input tables a `CGEventTap` reads, kept where a test can reach
//! them.
//!
//! This module is compiled on **every** target, deliberately. Its Windows
//! counterpart — `keyhook::windows_impl::key_of` — lives inside the
//! `cfg(target_os = "windows")` module that uses it, and that is fine there
//! because `cargo run -p kw-platform --example keyhook_probe` injects real
//! keystrokes on the machine this is developed on and would catch a wrong
//! number. There is no macOS machine here and no macOS probe (KNEG-59), so the
//! same placement would put the one part of the macOS hook that *can* be
//! checked behind a `cfg` that never compiles here. The plumbing in
//! [`crate::keyhook`] is unavoidably unverifiable from this machine; these
//! tables are not, so they are not filed with it.
//!
//! **Epistemic status of what is in here: knowledge, not measurement.** The
//! `kVK_*` constants are read from Carbon's `HIToolbox/Events.h` and the mouse
//! button numbering from `CGEventTypes.h`; nothing in this project has watched
//! a real macOS key produce any of them. That is a weaker claim than the
//! Windows table can make and is why the numbers are written once, here, with a
//! test that pins each one — so the day someone runs it on a Mac, a wrong entry
//! is one edit in one place rather than a hunt through a callback.
//!
//! # The Globe key is deliberately not a modifier
//!
//! macOS reports Globe / `Fn` (`kVK_Function`, `0x3F`) through the same
//! `flagsChanged` events as the eight real modifiers, so it is the one key on
//! this platform that *looks* bindable here and is not. [`Key`] has no `Fn`
//! variant, [`crate::keys`]'s accelerator vocabulary has no spelling for it, and
//! `docs/PLATFORM_SUPPORT.md`'s Globe row says "not built" on every platform.
//! Mapping it to some spare modifier would make that row false without adding
//! the capability, so it falls through to [`Key::Other`] like any other key.
//!
//! # The difference that matters most
//!
//! On Windows a modifier press is a `WM_KEYDOWN` like any other. On macOS it is
//! **not a key event at all**: pressing Control alone emits
//! `kCGEventFlagsChanged`, and `kCGEventKeyDown` never fires for a modifier. A
//! tap that watched only key events would see nothing whatsoever for
//! `Control+Super`, which is the entire capability KNEG-62 is about. See
//! [`Transition`] for how press and release are told apart, which is the second
//! half of the same problem.

use crate::keys::{Key, MouseButton};

/// Left Control. `kVK_Control` in `HIToolbox/Events.h`.
const KVK_CONTROL: u32 = 0x3B;
/// Right Control. `kVK_RightControl`.
const KVK_RIGHT_CONTROL: u32 = 0x3E;
/// Left Shift. `kVK_Shift`.
const KVK_SHIFT: u32 = 0x38;
/// Right Shift. `kVK_RightShift`.
const KVK_RIGHT_SHIFT: u32 = 0x3C;
/// Left Option, which is Alt. `kVK_Option`.
const KVK_OPTION: u32 = 0x3A;
/// Right Option. `kVK_RightOption`.
const KVK_RIGHT_OPTION: u32 = 0x3D;
/// Left Command, which is Super. `kVK_Command`.
const KVK_COMMAND: u32 = 0x37;
/// Right Command. `kVK_RightCommand`.
const KVK_RIGHT_COMMAND: u32 = 0x36;
/// The Globe / Fn key. `kVK_Function`.
///
/// Named only so a test can assert it is *not* claimed as a modifier, which is
/// why it is `cfg(test)`: nothing in the shipped path needs the number, and the
/// decision it records is in the module doc rather than here.
#[cfg(test)]
const KVK_FUNCTION: u32 = 0x3F;

/// Which key a `CGEvent`'s `kCGKeyboardEventKeycode` names.
///
/// Sided, like the Windows table and for the same reason: a tap reports the
/// physical key, which is what lets `RightControl+Super` be told from
/// `Control+Super` at all. Anything that is not one of the eight modifiers
/// becomes [`Key::Other`], which is all [`crate::keys::ChordWatcher`]'s rule 2
/// needs — that *an* ordinary key is down, never which.
#[must_use]
pub const fn key_of(keycode: u32) -> Key {
    match keycode {
        KVK_CONTROL => Key::LeftControl,
        KVK_RIGHT_CONTROL => Key::RightControl,
        KVK_SHIFT => Key::LeftShift,
        KVK_RIGHT_SHIFT => Key::RightShift,
        KVK_OPTION => Key::LeftAlt,
        KVK_RIGHT_OPTION => Key::RightAlt,
        KVK_COMMAND => Key::LeftSuper,
        KVK_RIGHT_COMMAND => Key::RightSuper,
        other => Key::Other(other),
    }
}

/// Which side button a `kCGMouseEventButtonNumber` names, if it names one.
///
/// `CGEventTypes.h` numbers buttons from zero: 0 left, 1 right, 2 middle. So
/// the buttons a user calls 4 and 5 are 3 and 4 here. That off-by-one is the
/// macOS twin of the Windows high-word/low-word asymmetry the mouse hook
/// documents, with one difference worth keeping in view: the Windows one was
/// *measured* by `mousehook_probe`, and this one is read.
#[must_use]
pub const fn button_of(button_number: i64) -> Option<MouseButton> {
    match button_number {
        3 => Some(MouseButton::Four),
        4 => Some(MouseButton::Five),
        // A sixth button and beyond exists in the API and has never been seen
        // here. Ignored rather than folded into button 5, which would be
        // inventing hardware -- the same choice `button_of` makes on Windows.
        _ => None,
    }
}

/// Which edge a `flagsChanged` event is, for a modifier that has no key event.
///
/// **Why this is not read off the event.** `CGEventGetFlags` returns the
/// *combined* modifier state, and its public masks — `kCGEventFlagMaskControl`
/// and friends — are unsided: left and right Control set the same bit. The bits
/// that distinguish them (`NX_DEVICELCTLKEYMASK` and its seven siblings) are
/// IOKit's, not CoreGraphics', and are not part of any API this project can
/// name. So the side is taken from the keycode, which is unambiguous, and the
/// *direction* from what this tap last saw of that same key.
///
/// The cost of tracking it rather than reading it is that a missed event
/// desyncs the set, so the caller must reset on
/// `kCGEventTapDisabledByTimeout` — see [`crate::keyhook`], which does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transition {
    /// The key was not down and now is.
    Pressed,
    /// The key was down and now is not.
    Released,
}

/// Tracks which sided modifiers a tap has seen go down.
///
/// Small and separate from [`crate::keys::ChordWatcher`] on purpose: that type
/// answers "has a watched chord completed", which is a different question from
/// "is this physical key currently down", and it deliberately does not expose
/// the second. Conflating them would mean a chord watcher that had to be
/// correct about keys it is not watching.
#[derive(Debug, Default)]
pub struct FlagTracker {
    down: Vec<Key>,
}

impl FlagTracker {
    #[must_use]
    pub const fn new() -> Self {
        Self { down: Vec::new() }
    }

    /// Record a `flagsChanged` for `key` and say which edge it was.
    ///
    /// Every `flagsChanged` for a modifier is one edge or the other — the event
    /// exists because the flag set changed — so this always returns a
    /// transition rather than an `Option`.
    pub fn transition(&mut self, key: Key) -> Transition {
        if let Some(at) = self.down.iter().position(|held| *held == key) {
            self.down.swap_remove(at);
            Transition::Released
        } else {
            self.down.push(key);
            Transition::Pressed
        }
    }

    /// Forget everything, after an event this tap did not see.
    ///
    /// `kCGEventTapDisabledByTimeout` means the system stopped calling this tap
    /// because it was too slow, and re-enabling it resumes a stream with a hole
    /// in it. Keys released during the hole would still read as down, so the
    /// next press of one would be reported as a release. Clearing is the only
    /// honest recovery: it can lose a chord that was being held across the
    /// gap, which is a missed shortcut rather than a stuck one.
    pub fn clear(&mut self) {
        self.down.clear();
    }

    /// How many sided modifiers are currently believed to be down.
    #[must_use]
    pub fn held_count(&self) -> usize {
        self.down.len()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn every_sided_modifier_maps_to_its_own_key() {
        // Pinned individually rather than round-tripped through a table, so a
        // transposed pair -- the failure most likely to survive review, because
        // both numbers are present and plausible -- fails here.
        assert_eq!(key_of(0x3B), Key::LeftControl);
        assert_eq!(key_of(0x3E), Key::RightControl);
        assert_eq!(key_of(0x38), Key::LeftShift);
        assert_eq!(key_of(0x3C), Key::RightShift);
        assert_eq!(key_of(0x3A), Key::LeftAlt);
        assert_eq!(key_of(0x3D), Key::RightAlt);
        assert_eq!(key_of(0x37), Key::LeftSuper);
        assert_eq!(key_of(0x36), Key::RightSuper);
    }

    #[test]
    fn the_eight_modifiers_are_eight_distinct_keys() {
        let mut mapped: Vec<Key> = [0x3B, 0x3E, 0x38, 0x3C, 0x3A, 0x3D, 0x37, 0x36]
            .into_iter()
            .map(key_of)
            .collect();
        mapped.sort_unstable();
        mapped.dedup();
        assert_eq!(mapped.len(), 8, "two keycodes mapped to the same Key");
        assert!(
            mapped.iter().all(|key| key.is_modifier()),
            "a modifier keycode produced an ordinary key"
        );
    }

    #[test]
    fn option_is_alt_and_command_is_super() {
        // The naming trap, and the reason it is worth a test of its own: macOS
        // calls them Option and Command, this vocabulary calls them Alt and
        // Super, and a mapping that followed the *names* would swap Command
        // onto Alt -- making the shipped default `Control+Super` fire on
        // Control+Option, a chord the user never bound.
        assert_eq!(key_of(KVK_OPTION), Key::LeftAlt);
        assert_eq!(key_of(KVK_COMMAND), Key::LeftSuper);
    }

    #[test]
    fn the_globe_key_is_not_claimed_as_a_modifier() {
        assert_eq!(key_of(KVK_FUNCTION), Key::Other(KVK_FUNCTION));
        assert!(!key_of(KVK_FUNCTION).is_modifier());
    }

    #[test]
    fn an_ordinary_key_keeps_its_keycode() {
        // `kVK_ANSI_D`. Rule 2 only needs to know an ordinary key is down, but
        // the code has to survive the trip so two different ordinary keys are
        // not the same `Other`.
        assert_eq!(key_of(0x02), Key::Other(0x02));
        assert_ne!(key_of(0x02), key_of(0x03));
    }

    #[test]
    fn only_the_two_side_buttons_are_recognised() {
        assert_eq!(button_of(3), Some(MouseButton::Four));
        assert_eq!(button_of(4), Some(MouseButton::Five));
        // Left, right and middle are not shortcuts. Binding the left button
        // would make the machine unusable, which is `keys::refused_mouse_button`'s
        // subject rather than this table's.
        assert_eq!(button_of(0), None);
        assert_eq!(button_of(1), None);
        assert_eq!(button_of(2), None);
        assert_eq!(button_of(5), None);
        assert_eq!(button_of(-1), None);
    }

    #[test]
    fn a_flags_changed_pair_reads_as_a_press_then_a_release() {
        let mut tracker = FlagTracker::new();
        assert_eq!(tracker.transition(Key::LeftControl), Transition::Pressed);
        assert_eq!(tracker.transition(Key::LeftControl), Transition::Released);
        assert_eq!(tracker.held_count(), 0);
    }

    #[test]
    fn the_two_sides_of_one_modifier_are_tracked_apart() {
        // The point of taking the side from the keycode: CGEventGetFlags sets
        // the same bit for both, so a tracker keyed on the flag would call the
        // right-Control press a left-Control release.
        let mut tracker = FlagTracker::new();
        assert_eq!(tracker.transition(Key::LeftControl), Transition::Pressed);
        assert_eq!(tracker.transition(Key::RightControl), Transition::Pressed);
        assert_eq!(tracker.held_count(), 2);
        assert_eq!(tracker.transition(Key::LeftControl), Transition::Released);
        assert_eq!(tracker.transition(Key::RightControl), Transition::Released);
        assert_eq!(tracker.held_count(), 0);
    }

    #[test]
    fn a_full_chord_presses_and_releases_in_any_order() {
        let mut tracker = FlagTracker::new();
        for key in [Key::LeftControl, Key::LeftSuper] {
            assert_eq!(tracker.transition(key), Transition::Pressed);
        }
        // Released in the other order, which is what a hand actually does.
        assert_eq!(tracker.transition(Key::LeftControl), Transition::Released);
        assert_eq!(tracker.transition(Key::LeftSuper), Transition::Released);
        assert_eq!(tracker.held_count(), 0);
    }

    #[test]
    fn clearing_after_a_disabled_tap_makes_the_next_event_a_press() {
        // Without the clear, the held key survives the hole in the stream and
        // the user's next press of it reports a release -- a shortcut that
        // needs pressing twice, intermittently, which is the worst shape of
        // bug to be handed.
        let mut tracker = FlagTracker::new();
        assert_eq!(tracker.transition(Key::LeftSuper), Transition::Pressed);
        tracker.clear();
        assert_eq!(tracker.held_count(), 0);
        assert_eq!(tracker.transition(Key::LeftSuper), Transition::Pressed);
    }
}
