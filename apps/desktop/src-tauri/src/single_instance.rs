//! One copy of the application per data folder (KNEG-564).
//!
//! Two things keep a second copy out, because neither is enough alone.
//!
//! `tauri-plugin-single-instance` is what a person meets: a second launch
//! shows the window of the copy already running, and exits. It is keyed on the
//! application's identifier. Its Windows half exits only if it finds the first
//! copy's signalling window, and a second copy that arrives after the first
//! has made its mutex and before it has made that window is let through
//! (version 2.4.5, read from its source).
//!
//! The lock here is what closes that gap, and it is keyed on what a second
//! copy actually harms: the data folder. Startup reaps model servers, marks
//! unfinished work interrupted and clears scratch files on the assumption that
//! nothing else is using them. A copy that cannot take the lock exits before
//! any of that. It shows nothing; the plugin is what brings a window forward.

use std::fs::{File, OpenOptions, TryLockError};
use std::path::Path;

/// Whether the plugin's guard is installed.
///
/// Every copy has it, except a debug build given its own profile: the plugin's
/// key is shared by a development build and the installed application, and a
/// profile moves the data folder, so that copy can do the other no harm. A
/// development build with no profile uses the real data folder and is refused
/// beside the installed application, as it should be.
pub const fn shows_the_running_copy(own_profile: bool) -> bool {
    !own_profile
}

/// The data folder's lock, held for as long as the application runs.
#[derive(Debug)]
pub struct Held(#[allow(dead_code)] File);

/// What asking for the data folder's lock found.
#[derive(Debug)]
pub enum Claim {
    /// This copy has the folder.
    Held(Held),
    /// Another copy has it.
    Taken,
    /// The lock could not be asked for. The application starts anyway: a
    /// folder that cannot hold a lock file is not a reason to refuse to run.
    Unknown(std::io::Error),
}

/// Ask for the lock on `data_dir`, without waiting.
pub fn claim(data_dir: &Path) -> Claim {
    let open = || -> std::io::Result<File> {
        std::fs::create_dir_all(data_dir)?;
        OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(data_dir.join("knegiswhisper.lock"))
    };
    let file = match open() {
        Ok(file) => file,
        Err(error) => return Claim::Unknown(error),
    };
    match file.try_lock() {
        Ok(()) => Claim::Held(Held(file)),
        Err(TryLockError::WouldBlock) => Claim::Taken,
        Err(TryLockError::Error(error)) => Claim::Unknown(error),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn only_a_copy_with_its_own_profile_goes_without_the_plugins_guard() {
        assert!(shows_the_running_copy(false));
        assert!(!shows_the_running_copy(true));
    }

    #[test]
    fn a_second_claim_on_one_data_folder_is_refused_until_the_first_ends() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("KnegisWhisper");

        let Claim::Held(first) = claim(&folder) else {
            panic!("the first copy did not get the folder");
        };
        // The defect this closes: a second copy that the plugin let through.
        assert!(matches!(claim(&folder), Claim::Taken));

        drop(first);
        assert!(matches!(claim(&folder), Claim::Held(_)));
    }

    #[test]
    fn two_data_folders_do_not_refuse_each_other() {
        let dir = tempfile::tempdir().unwrap();
        let _real = claim(&dir.path().join("real"));
        assert!(matches!(claim(&dir.path().join("profile")), Claim::Held(_)));
    }

    #[test]
    fn a_folder_that_cannot_hold_a_lock_does_not_stop_the_application() {
        let dir = tempfile::tempdir().unwrap();
        // A file where the folder would be: it cannot be created.
        let blocked = dir.path().join("blocked");
        std::fs::write(&blocked, b"x").unwrap();
        assert!(matches!(claim(&blocked), Claim::Unknown(_)));
    }
}
