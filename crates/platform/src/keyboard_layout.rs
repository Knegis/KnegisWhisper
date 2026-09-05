//! Whether the active keyboard layout has an AltGr key.
//!
//! One question, asked for one reason. On a Nordic, German, Polish or most
//! other non-US layouts, `@ $ € { [ ] } \ |` are typed with **AltGr**, and
//! Windows produces AltGr by generating **left Control plus right Alt** — there
//! is no separate virtual key for it. A modifier-only chord of `Control+Alt`
//! therefore matches every AltGr keystroke, which is measured rather than
//! reasoned: injecting exactly that pair started a dictation on the 2026-08-26
//! bench run. See [`crate::keys::altgr_collision`], which decides the collision
//! by running the real chord matcher rather than by a second set of rules.
//!
//! This module answers the other half — whether the layout in front of the user
//! *has* an AltGr key at all, so a US-layout user is not warned about a
//! keystroke they cannot make.
//!
//! **Measured, not inferred from a locale name.** A layout is not "Swedish" to
//! this code; it either produces a character that needs Control and Alt
//! together, or it does not. `VkKeyScanExW` answers that per character with no
//! side effects, and the character it found is carried in the answer so the
//! warning can name it: *"pressing AltGr to type @"* is a sentence a user can
//! check against their own keyboard, and "your layout has AltGr" is not.

/// Whether the active layout has an AltGr key, and the evidence.
///
/// Three states rather than a boolean, for the reason
/// [`crate::window::content_protection`] has them: a platform that cannot be
/// asked must not answer "no". A layout with no AltGr and a layout nobody
/// looked at produce the same warnings if they collapse, and only one of those
/// is a reason to stay quiet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AltGr {
    /// The layout types at least one character with Control and Alt together.
    Present {
        /// One character that needs it, for a warning a user can verify.
        example: char,
    },

    /// Every character this layout can type needs something other than the
    /// Control-and-Alt state. An AltGr collision cannot happen here.
    Absent,

    /// Nobody asked, or the answer did not come back.
    Unknown {
        /// Why, in a sentence, rather than an empty `None` to guess at.
        why: String,
    },
}

impl AltGr {
    /// Whether an AltGr keystroke is possible, as far as anyone knows.
    ///
    /// `Unknown` counts as possible. The cost of warning a user whose layout
    /// turns out not to have AltGr is a sentence they can ignore; the cost of
    /// staying quiet on a layout that does have it is the defect this exists
    /// for.
    #[must_use]
    pub const fn is_possible(&self) -> bool {
        !matches!(self, Self::Absent)
    }
}

/// The `VkKeyScanEx` shift-state bits that together mean AltGr.
///
/// Bit 0 is Shift, bit 1 Control, bit 2 Alt. Both of the latter two are
/// required: a layout reporting Control alone is describing a control
/// character, not an AltGr one.
const CONTROL_AND_ALT: u8 = 0b0000_0110;

/// Characters worth asking about, most recognisable first.
///
/// Not the whole of Unicode and not meant to be — one hit is enough to answer
/// the question, and the order decides which character the warning names. `@`
/// leads because it is AltGr on Swedish, German, Norwegian, Danish, Finnish,
/// Polish and Portuguese layouts and is the one every user of those recognises
/// instantly. The list ends with the Latin-1 characters that are AltGr on
/// layouts where none of the ASCII ones are.
const CANDIDATES: [char; 14] = [
    '@', '$', '{', '}', '[', ']', '\\', '|', '~', '€', '£', '¤', 'µ', '§',
];

/// Decide from what the layout said about each character.
///
/// Pure, and the half that carries the rule: `None` means the layout cannot
/// type that character at all, and a `Some` state that is missing either bit is
/// a character typed some other way. Split out because nothing outside Windows
/// can call `VkKeyScanExW`, and a rule that only runs on one platform is a rule
/// only one platform's CI can check — this one is checked everywhere.
#[must_use]
pub fn classify(scanned: &[(char, Option<u8>)]) -> AltGr {
    for (character, state) in scanned {
        if state.is_some_and(|s| s & CONTROL_AND_ALT == CONTROL_AND_ALT) {
            return AltGr::Present {
                example: *character,
            };
        }
    }
    AltGr::Absent
}

/// Ask the active keyboard layout.
#[must_use]
pub fn altgr() -> AltGr {
    platform::altgr()
}

#[cfg(windows)]
mod platform {
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyboardLayout, VkKeyScanExW};

    use super::{classify, AltGr, CANDIDATES};

    pub fn altgr() -> AltGr {
        // The calling thread's layout, which for the settings screen is the one
        // the user is typing on. A per-window layout elsewhere is not what this
        // warning is about: the question is what happens when they press AltGr
        // while working, and the app's own layout is the same layout Windows
        // switches globally by default.
        //
        // SAFETY: `GetKeyboardLayout` takes a thread id and returns a handle; 0
        // means the calling thread and cannot fail.
        let hkl = unsafe { GetKeyboardLayout(0) };

        let scanned: Vec<(char, Option<u8>)> = CANDIDATES
            .iter()
            .map(|character| {
                let mut utf16 = [0_u16; 2];
                let encoded = character.encode_utf16(&mut utf16);
                // A character outside the BMP has no single virtual key by
                // definition. None of `CANDIDATES` is one; this is the branch
                // that says so rather than indexing and hoping.
                let Some(unit) = encoded.first().filter(|_| encoded.len() == 1) else {
                    return (*character, None);
                };
                // SAFETY: `VkKeyScanExW` reads one UTF-16 unit and a layout
                // handle and writes nothing. It returns -1 for a character the
                // layout cannot type, which is the only failure it has.
                let scan = unsafe { VkKeyScanExW(*unit, hkl) };
                if scan == -1 {
                    return (*character, None);
                }
                // High byte is the shift state, low byte the virtual key.
                #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
                let state = ((scan as u16) >> 8) as u8;
                (*character, Some(state))
            })
            .collect();

        classify(&scanned)
    }
}

#[cfg(not(windows))]
mod platform {
    use super::AltGr;

    /// Not implemented rather than answered.
    ///
    /// macOS reaches the same characters through Option, which does *not*
    /// synthesise Control, so the collision this warns about does not exist
    /// there in the same form. X11 and Wayland have a third-level shift that
    /// does behave like AltGr, and reading it means talking to the compositor.
    /// Both are real work and neither is done, so this says so instead of
    /// returning `Absent` and quietly suppressing every warning.
    pub fn altgr() -> AltGr {
        AltGr::Unknown {
            why: "this build cannot read the active keyboard layout on this platform".to_owned(),
        }
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

    #[test]
    fn a_character_needing_control_and_alt_is_altgr() {
        // What a Swedish layout says about `@`: virtual key 2, shift state 6.
        let scanned = [('@', Some(0b0000_0110))];
        assert_eq!(classify(&scanned), AltGr::Present { example: '@' });
    }

    #[test]
    fn a_us_layout_has_no_altgr() {
        // `@` is Shift+2, `$` is Shift+4, `{` is Shift+[. Nothing needs both.
        let scanned = [('@', Some(1)), ('$', Some(1)), ('{', Some(1))];
        assert_eq!(classify(&scanned), AltGr::Absent);
    }

    /// Control alone is not AltGr, and this is the case that would break a
    /// naive `state != 0` test.
    #[test]
    fn control_without_alt_is_not_altgr() {
        assert_eq!(classify(&[('@', Some(0b0000_0010))]), AltGr::Absent);
    }

    #[test]
    fn alt_without_control_is_not_altgr() {
        assert_eq!(classify(&[('@', Some(0b0000_0100))]), AltGr::Absent);
    }

    /// AltGr with Shift is still AltGr -- the Shift bit must not disqualify it.
    #[test]
    fn shift_on_top_of_altgr_still_counts() {
        assert_eq!(
            classify(&[('~', Some(0b0000_0111))]),
            AltGr::Present { example: '~' }
        );
    }

    #[test]
    fn a_character_the_layout_cannot_type_is_not_evidence_of_anything() {
        assert_eq!(classify(&[('€', None)]), AltGr::Absent);
    }

    /// The first hit wins, so the warning names the character earliest in
    /// `CANDIDATES` -- which is the most recognisable one, not an accident of
    /// iteration.
    #[test]
    fn the_example_is_the_first_candidate_that_matched() {
        let scanned = [('@', None), ('$', Some(6)), ('{', Some(6))];
        assert_eq!(classify(&scanned), AltGr::Present { example: '$' });
    }

    /// A platform nobody can ask must not read as a platform with no AltGr.
    #[test]
    fn unknown_is_treated_as_possible_and_absent_is_not() {
        assert!(AltGr::Unknown { why: String::new() }.is_possible());
        assert!(AltGr::Present { example: '@' }.is_possible());
        assert!(!AltGr::Absent.is_possible());
    }

    /// Whatever this machine says, it must say one of the three.
    #[test]
    fn the_real_layout_answers_without_panicking() {
        let answer = altgr();
        if cfg!(windows) {
            assert!(
                !matches!(answer, AltGr::Unknown { .. }),
                "Windows can be asked: {answer:?}"
            );
        } else {
            assert!(matches!(answer, AltGr::Unknown { .. }), "{answer:?}");
        }
    }
}
