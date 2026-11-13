//! Secrets longer than one Windows Credential Manager entry can hold.
//!
//! Windows keeps a generic credential's blob in at most 2,560 bytes
//! (`CRED_MAX_CREDENTIAL_BLOB_SIZE`), and `keyring` stores a password there
//! as UTF-16, so one entry holds at most **1,280 UTF-16 code units**.
//! `keyring` 3.6.3 refuses anything longer (`src/windows.rs`, the
//! "password encoded as UTF-16" check). Provider API keys fit easily. An OAuth
//! grant does not: the calendar bundle holds an access token and a refresh
//! token, and a Microsoft grant measured on 2026-11-13 was refused. The first
//! real Outlook.com connect got through sign-in and then failed with "Could
//! not save or read calendar data". The in-memory test store has no size
//! limit, which is why no test caught it.
//!
//! So a secret too long for one entry is split:
//!
//! - The **parts** go into entries named `kw-part-<slot hash>-<generation>-<n>`.
//!   The slot is hashed so a part name stays inside [`KeyRef`]'s 128 characters
//!   whatever the slot's length. Every name stays in `[a-z0-9-]`, so the
//!   uninstaller's exact-pattern credential cleanup removes parts with
//!   everything else.
//! - The slot's own entry holds a **manifest**,
//!   `kw-split-v1:<count>:<generation>:<sha256>`, and nothing secret.
//!
//! A secret that fits is stored exactly as before, as one entry, so every
//! secret written by an older build still reads unchanged.
//!
//! **Writing is ordered so a failure never leaves a readable wrong secret.**
//! The generation is derived from the new secret's digest, so new parts never
//! overwrite the parts the current manifest points at. The parts are written
//! first and the manifest last, and the manifest write is the commit point.
//! A failure before it removes the new parts and leaves the previous secret in
//! place and readable. Parts the new manifest no longer names are deleted only
//! after it is written. A part that can't be deleted then is logged, and the
//! uninstaller removes it later.
//!
//! **Reading verifies the whole.** The parts are joined and checked against
//! the manifest's digest. A missing or altered part is an error, never a
//! shorter or different secret.
//!
//! **Deleting removes the parts before the manifest**, and stops if a part
//! can't be deleted. A caller that retries a failed delete (the calendar's
//! cleanup list) therefore still finds the manifest, and knows what is left.

use sha2::{Digest as _, Sha256};

use crate::{CredentialError, CredentialStore, KeyRef, Secret};

/// One Windows entry holds this many UTF-16 code units. Not used for the
/// decision below, which keeps a margin under it; kept so the tests state the
/// limit they emulate.
#[cfg(test)]
const ENTRY_UNITS: usize = 1280;

/// A secret up to this many UTF-16 code units is stored as one entry.
const SINGLE_UNITS: usize = 1200;

/// Each part holds at most this many UTF-16 code units.
const PART_UNITS: usize = 1000;

/// More parts than this is refused, rather than spread across the store.
/// 32 parts is about 32,000 characters, far beyond any grant seen.
const MAX_PARTS: usize = 32;

const MANIFEST_PREFIX: &str = "kw-split-v1:";

#[derive(Debug, PartialEq, Eq)]
struct Manifest {
    count: usize,
    generation: String,
    digest: String,
}

impl Manifest {
    fn parse(stored: &str) -> Option<Self> {
        let mut fields = stored.strip_prefix(MANIFEST_PREFIX)?.split(':');
        let count = fields
            .next()?
            .parse::<usize>()
            .ok()
            .filter(|n| (2..=MAX_PARTS).contains(n))?;
        let generation = fields.next().filter(|g| is_hex(g, 16))?.to_owned();
        let digest = fields.next().filter(|d| is_hex(d, 64))?.to_owned();
        if fields.next().is_some() {
            return None;
        }
        Some(Self {
            count,
            generation,
            digest,
        })
    }

    fn render(&self) -> String {
        format!(
            "{MANIFEST_PREFIX}{}:{}:{}",
            self.count, self.generation, self.digest
        )
    }

    fn part_keys(&self, key: &KeyRef) -> Result<Vec<KeyRef>, CredentialError> {
        (0..self.count)
            .map(|index| part_key(key, &self.generation, index))
            .collect()
    }
}

fn is_hex(value: &str, len: usize) -> bool {
    value.len() == len
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn digest_hex(value: &str) -> String {
    Sha256::digest(value.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn units(value: &str) -> usize {
    value.encode_utf16().count()
}

fn part_key(key: &KeyRef, generation: &str, index: usize) -> Result<KeyRef, CredentialError> {
    let slot_hash: String = digest_hex(key.slot()).chars().take(16).collect();
    KeyRef::new(&format!("kw-part-{slot_hash}-{generation}-{index}"))
        .ok_or(CredentialError::InvalidSlot)
}

/// Split on character boundaries, so a character outside the Basic
/// Multilingual Plane (two UTF-16 units) is never cut in half.
fn split_units(value: &str, max_units: usize) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut current_units = 0;
    for c in value.chars() {
        let n = c.len_utf16();
        if current_units + n > max_units && !current.is_empty() {
            parts.push(std::mem::take(&mut current));
            current_units = 0;
        }
        current.push(c);
        current_units += n;
    }
    if !current.is_empty() {
        parts.push(current);
    }
    parts
}

/// Delete parts this write created, except any the previous manifest also
/// names: rewriting an identical secret reuses its part names, and those
/// parts still back the manifest that remains in place.
fn discard(raw: &dyn CredentialStore, created: &[KeyRef], keep: &[KeyRef]) {
    for part in created.iter().filter(|k| !keep.contains(k)) {
        if let Err(error) = raw.delete(part) {
            tracing::warn!(slot = %part, error = %error, "could not remove a credential part after a failed write");
        }
    }
}

/// The previous manifest's parts, if the slot holds a split secret now.
fn previous_parts(raw: &dyn CredentialStore, key: &KeyRef) -> Result<Vec<KeyRef>, CredentialError> {
    match raw.get(key) {
        Ok(stored) => match Manifest::parse(stored.expose()) {
            Some(manifest) => manifest.part_keys(key),
            None => Ok(Vec::new()),
        },
        Err(CredentialError::NotFound(_)) => Ok(Vec::new()),
        Err(other) => Err(other),
    }
}

pub(crate) fn get(raw: &dyn CredentialStore, key: &KeyRef) -> Result<Secret, CredentialError> {
    let stored = raw.get(key)?;
    let Some(manifest) = Manifest::parse(stored.expose()) else {
        return Ok(stored);
    };
    let mut whole = String::new();
    for (index, part) in manifest.part_keys(key)?.iter().enumerate() {
        match raw.get(part) {
            Ok(value) => whole.push_str(value.expose()),
            Err(CredentialError::NotFound(_)) => {
                return Err(CredentialError::Backend(format!(
                    "part {index} of the credential for {key} is missing"
                )))
            }
            Err(other) => return Err(other),
        }
    }
    if digest_hex(&whole) != manifest.digest {
        return Err(CredentialError::Backend(format!(
            "the parts of the credential for {key} do not match its record"
        )));
    }
    Ok(Secret::new(whole))
}

pub(crate) fn set(
    raw: &dyn CredentialStore,
    key: &KeyRef,
    secret: &Secret,
) -> Result<(), CredentialError> {
    let previous = previous_parts(raw, key)?;
    let value = secret.expose();

    let written = if units(value) <= SINGLE_UNITS {
        raw.set(key, secret)?;
        Vec::new()
    } else {
        let chunks = split_units(value, PART_UNITS);
        if chunks.len() > MAX_PARTS {
            return Err(CredentialError::Backend(format!(
                "the credential for {key} is too long to store"
            )));
        }
        let digest = digest_hex(value);
        let manifest = Manifest {
            count: chunks.len(),
            generation: digest.chars().take(16).collect(),
            digest,
        };
        let parts = manifest.part_keys(key)?;
        for (written_so_far, (part, chunk)) in parts.iter().zip(chunks).enumerate() {
            if let Err(error) = raw.set(part, &Secret::new(chunk)) {
                discard(
                    raw,
                    parts.get(..written_so_far).unwrap_or_default(),
                    &previous,
                );
                return Err(error);
            }
        }
        if let Err(error) = raw.set(key, &Secret::new(manifest.render())) {
            discard(raw, &parts, &previous);
            return Err(error);
        }
        parts
    };

    // The new secret is in place. Parts only the previous manifest named are
    // now unreferenced; a failure here leaves an orphan, not a wrong secret.
    for part in previous.iter().filter(|k| !written.contains(k)) {
        if let Err(error) = raw.delete(part) {
            tracing::warn!(slot = %part, error = %error, "could not delete a superseded credential part");
        }
    }
    Ok(())
}

pub(crate) fn delete(raw: &dyn CredentialStore, key: &KeyRef) -> Result<(), CredentialError> {
    for part in previous_parts(raw, key)? {
        match raw.delete(&part) {
            Ok(()) | Err(CredentialError::NotFound(_)) => {}
            Err(other) => return Err(other),
        }
    }
    raw.delete(key)
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
    use crate::MemoryCredentialStore;
    use std::sync::Mutex;

    /// An in-memory store that refuses what Windows refuses, and can be told
    /// to fail one write, to stand in for a crash or a locked store.
    #[derive(Debug, Default)]
    struct WindowsLike {
        inner: MemoryCredentialStore,
        fail_write_to: Mutex<Option<String>>,
        slots: Mutex<std::collections::BTreeSet<String>>,
    }

    impl WindowsLike {
        fn fail_writes_to(&self, slot_prefix: &str) {
            *self.fail_write_to.lock().unwrap() = Some(slot_prefix.to_owned());
        }
        fn heal(&self) {
            *self.fail_write_to.lock().unwrap() = None;
        }
        fn entries(&self) -> Vec<String> {
            self.slots.lock().unwrap().iter().cloned().collect()
        }
    }

    impl CredentialStore for WindowsLike {
        fn get(&self, key: &KeyRef) -> Result<Secret, CredentialError> {
            self.inner.get(key)
        }
        fn set(&self, key: &KeyRef, secret: &Secret) -> Result<(), CredentialError> {
            if let Some(prefix) = self.fail_write_to.lock().unwrap().as_deref() {
                if key.slot().starts_with(prefix) {
                    return Err(CredentialError::Backend("simulated failure".to_owned()));
                }
            }
            if units(secret.expose()) > ENTRY_UNITS {
                return Err(CredentialError::Backend(
                    "password encoded as UTF-16 is longer than platform limit".to_owned(),
                ));
            }
            self.slots.lock().unwrap().insert(key.slot().to_owned());
            self.inner.set(key, secret)
        }
        fn delete(&self, key: &KeyRef) -> Result<(), CredentialError> {
            self.slots.lock().unwrap().remove(key.slot());
            self.inner.delete(key)
        }
    }

    fn key(slot: &str) -> KeyRef {
        KeyRef::new(slot).unwrap()
    }

    fn long(seed: char, len: usize) -> Secret {
        Secret::new(std::iter::repeat_n(seed, len).collect())
    }

    /// The shape of a calendar grant: a JSON object far over one entry.
    fn grant(len: usize) -> Secret {
        let body: String = "abcdefghijklmnopqrstuvwxyz0123456789"
            .chars()
            .cycle()
            .take(len)
            .collect();
        Secret::new(format!(
            "{{\"access_token\":\"{body}\",\"refresh_token\":\"r\"}}"
        ))
    }

    const SLOT: &str = "calendar-microsoft-0123456789abcdef";

    #[test]
    fn one_entry_refuses_a_grant_which_is_why_this_exists() {
        let raw = WindowsLike::default();
        assert!(raw.set(&key(SLOT), &grant(3000)).is_err());
    }

    #[test]
    fn a_grant_longer_than_one_entry_round_trips() {
        let raw = WindowsLike::default();
        let secret = grant(3000);
        set(&raw, &key(SLOT), &secret).unwrap();
        assert_eq!(get(&raw, &key(SLOT)).unwrap().expose(), secret.expose());
        for slot in raw.entries() {
            assert!(units(raw.inner.get(&key(&slot)).unwrap().expose()) <= ENTRY_UNITS);
        }
        assert_eq!(raw.entries().len(), 5, "the manifest and four parts");
    }

    #[test]
    fn a_secret_that_fits_is_stored_exactly_as_before() {
        let raw = WindowsLike::default();
        let secret = Secret::new("sk-short".to_owned());
        set(&raw, &key(SLOT), &secret).unwrap();
        assert_eq!(raw.inner.get(&key(SLOT)).unwrap().expose(), "sk-short");
        assert_eq!(raw.entries(), vec![SLOT.to_owned()]);
        // And one written by an older build, without this module, reads back.
        raw.set(&key("openai"), &Secret::new("sk-old".to_owned()))
            .unwrap();
        assert_eq!(get(&raw, &key("openai")).unwrap().expose(), "sk-old");
    }

    #[test]
    fn replacing_a_split_secret_removes_the_parts_it_no_longer_needs() {
        let raw = WindowsLike::default();
        set(&raw, &key(SLOT), &grant(3000)).unwrap();
        let second = grant(2500);
        set(&raw, &key(SLOT), &second).unwrap();
        assert_eq!(get(&raw, &key(SLOT)).unwrap().expose(), second.expose());
        assert_eq!(raw.entries().len(), 4, "the manifest and three parts");

        let short = Secret::new("short".to_owned());
        set(&raw, &key(SLOT), &short).unwrap();
        assert_eq!(get(&raw, &key(SLOT)).unwrap().expose(), "short");
        assert_eq!(raw.entries(), vec![SLOT.to_owned()]);
    }

    #[test]
    fn rewriting_the_same_secret_keeps_it_readable() {
        let raw = WindowsLike::default();
        let secret = grant(3000);
        set(&raw, &key(SLOT), &secret).unwrap();
        set(&raw, &key(SLOT), &secret).unwrap();
        assert_eq!(get(&raw, &key(SLOT)).unwrap().expose(), secret.expose());
        assert_eq!(raw.entries().len(), 5);
    }

    #[test]
    fn a_failed_part_write_keeps_the_previous_secret_and_leaves_no_new_parts() {
        let raw = WindowsLike::default();
        let first = grant(3000);
        set(&raw, &key(SLOT), &first).unwrap();
        let before = raw.entries();

        raw.fail_writes_to("kw-part-");
        assert!(set(&raw, &key(SLOT), &grant(2000)).is_err());
        raw.heal();
        assert_eq!(get(&raw, &key(SLOT)).unwrap().expose(), first.expose());
        assert_eq!(raw.entries(), before);
    }

    #[test]
    fn a_failed_manifest_write_keeps_the_previous_secret_and_leaves_no_new_parts() {
        let raw = WindowsLike::default();
        let first = grant(3000);
        set(&raw, &key(SLOT), &first).unwrap();
        let before = raw.entries();

        raw.fail_writes_to(SLOT);
        assert!(set(&raw, &key(SLOT), &grant(2000)).is_err());
        raw.heal();
        assert_eq!(get(&raw, &key(SLOT)).unwrap().expose(), first.expose());
        assert_eq!(raw.entries(), before);
    }

    #[test]
    fn a_missing_or_altered_part_is_an_error_never_a_different_secret() {
        let raw = WindowsLike::default();
        set(&raw, &key(SLOT), &grant(3000)).unwrap();
        let part = raw
            .entries()
            .into_iter()
            .find(|s| s.starts_with("kw-part-"))
            .unwrap();

        raw.inner
            .set(&key(&part), &Secret::new("tampered".to_owned()))
            .unwrap();
        assert!(matches!(
            get(&raw, &key(SLOT)),
            Err(CredentialError::Backend(_))
        ));

        raw.delete(&key(&part)).unwrap();
        assert!(matches!(
            get(&raw, &key(SLOT)),
            Err(CredentialError::Backend(_))
        ));
    }

    #[test]
    fn delete_removes_every_part_and_then_the_manifest() {
        let raw = WindowsLike::default();
        set(&raw, &key(SLOT), &grant(3000)).unwrap();
        delete(&raw, &key(SLOT)).unwrap();
        assert!(raw.entries().is_empty());
        assert!(matches!(
            get(&raw, &key(SLOT)),
            Err(CredentialError::NotFound(_))
        ));
        // Deleting what isn't there stays a no-op, as it was.
        delete(&raw, &key(SLOT)).unwrap();
    }

    #[test]
    fn characters_outside_the_basic_plane_are_never_cut_in_half() {
        let raw = WindowsLike::default();
        // 1,500 characters, 3,000 UTF-16 units.
        let secret = long('\u{1F600}', 1500);
        set(&raw, &key(SLOT), &secret).unwrap();
        assert_eq!(get(&raw, &key(SLOT)).unwrap().expose(), secret.expose());
    }

    #[test]
    fn a_secret_too_long_for_every_part_is_refused_and_changes_nothing() {
        let raw = WindowsLike::default();
        let first = grant(3000);
        set(&raw, &key(SLOT), &first).unwrap();
        let before = raw.entries();
        assert!(set(&raw, &key(SLOT), &long('x', PART_UNITS * MAX_PARTS + 1)).is_err());
        assert_eq!(get(&raw, &key(SLOT)).unwrap().expose(), first.expose());
        assert_eq!(raw.entries(), before);
    }

    #[test]
    fn part_names_are_valid_slots_whatever_the_slot_length() {
        let longest = "a".repeat(128);
        let part = part_key(&key(&longest), &"0".repeat(16), MAX_PARTS - 1).unwrap();
        assert!(part.slot().len() <= 128);
        assert!(part
            .slot()
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'));
    }

    #[test]
    fn only_a_well_formed_manifest_is_read_as_one() {
        let good = Manifest {
            count: 3,
            generation: "0123456789abcdef".to_owned(),
            digest: "a".repeat(64),
        };
        assert_eq!(Manifest::parse(&good.render()), Some(good));
        for stored in [
            "kw-split-v1:1:0123456789abcdef:".to_owned(),
            format!("kw-split-v1:99:0123456789abcdef:{}", "a".repeat(64)),
            format!("kw-split-v1:3:XYZ:{}", "a".repeat(64)),
            format!("kw-split-v1:3:0123456789abcdef:{}:extra", "a".repeat(64)),
            "{\"access_token\":\"kw-split-v1:3\"}".to_owned(),
        ] {
            assert_eq!(Manifest::parse(&stored), None, "{stored}");
        }
    }
}
