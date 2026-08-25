//! Getting dictated text to the cursor.
//!
//! The strategy is always *clipboard plus a synthetic paste keystroke*, never
//! direct character injection. That is upstream's choice and it is the right
//! one: injection loses non-ASCII text, fights input-method editors, and is far
//! slower for anything longer than a few words.
//!
//! The subtle part, and the reason this is a module rather than three lines, is
//! the clipboard restore. Dictation has to borrow the clipboard, so it has to
//! give it back — but only if the user has not used it in the meantime. Upstream
//! gets this right and it is worth reproducing exactly: after pasting, re-read
//! the clipboard and restore the original **only if what is there is still what
//! we wrote**. If the user copied something during the paste window, their copy
//! wins.
//!
//! Getting that wrong is not a cosmetic bug. It silently destroys something the
//! user deliberately put on the clipboard.

use std::fmt;
use std::time::Duration;

use serde::Serialize;

use crate::PlatformError;

/// How long to wait after sending the paste keystroke before restoring.
///
/// The paste is asynchronous: the keystroke is delivered to the foreground
/// window, which reads the clipboard whenever it gets round to it. Restore too
/// early and the target application pastes the *original* clipboard content.
/// These are upstream's measured values.
#[must_use]
pub fn restore_delay() -> Duration {
    #[cfg(target_os = "macos")]
    {
        Duration::from_millis(450)
    }
    #[cfg(target_os = "windows")]
    {
        Duration::from_millis(500)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        // Upstream uses 800 ms on Linux and 1200 ms on KDE/Wayland. Until the
        // compositor detection lands, take the safer of the two.
        Duration::from_millis(1200)
    }
}

/// Read and write the system clipboard.
///
/// A trait because the restore rule above is the part worth testing, and testing
/// it against the real clipboard would be both flaky and rude to whoever is
/// running the tests.
pub trait Clipboard: Send + Sync + fmt::Debug {
    fn read_text(&self) -> Result<String, PlatformError>;
    fn write_text(&self, text: &str) -> Result<(), PlatformError>;
}

/// Deliver the platform's paste keystroke to whatever window has focus.
pub trait PasteKeystroke: Send + Sync + fmt::Debug {
    fn send(&self) -> Result<(), PlatformError>;
    /// What this actually sends, for logs and diagnostics.
    fn describe(&self) -> &'static str;
}

/// What happened to the user's original clipboard content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "state",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum ClipboardRestore {
    /// Put back as it was.
    Restored,
    /// Left alone, because the clipboard no longer held the text we pasted —
    /// the user copied something during the paste window and their copy wins.
    SkippedUserChangedIt,
    /// There was nothing on the clipboard to begin with.
    NothingToRestore,
    /// The restore was attempted and failed.
    Failed { error: String },
}

/// The outcome of an insertion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Insertion {
    /// Whether the paste keystroke was delivered. Note that this does not, and
    /// cannot, mean the target application accepted it: there is no way to
    /// confirm that from outside the application.
    pub keystroke_sent: bool,
    pub clipboard: ClipboardRestore,
    /// How the keystroke was delivered, for diagnostics.
    pub method: &'static str,
}

/// Inserts text by borrowing the clipboard.
#[derive(Debug)]
pub struct TextInserter<C, K> {
    clipboard: C,
    keystroke: K,
    delay: Duration,
}

impl<C: Clipboard, K: PasteKeystroke> TextInserter<C, K> {
    pub const fn new(clipboard: C, keystroke: K, delay: Duration) -> Self {
        Self {
            clipboard,
            keystroke,
            delay,
        }
    }

    /// Insert `text` at the cursor.
    ///
    /// Never logs `text`, and never logs the clipboard content it saved: both
    /// are user data, and one of them is whatever happened to be copied.
    pub async fn insert(&self, text: &str) -> Result<Insertion, PlatformError> {
        if text.is_empty() {
            // Nothing to paste, and no reason to disturb the clipboard.
            return Ok(Insertion {
                keystroke_sent: false,
                clipboard: ClipboardRestore::NothingToRestore,
                method: self.keystroke.describe(),
            });
        }

        // Save whatever is there. A failure to read is not fatal -- an empty or
        // image-only clipboard reads as an error on some platforms -- but it does
        // mean there is nothing we can put back.
        let original = match self.clipboard.read_text() {
            Ok(existing) if !existing.is_empty() => Some(existing),
            Ok(_) => None,
            Err(err) => {
                tracing::debug!(error = %err, "could not read the clipboard; nothing to restore");
                None
            }
        };

        self.clipboard.write_text(text)?;
        self.keystroke.send()?;

        tokio::time::sleep(self.delay).await;

        let restore = self.restore(original.as_deref(), text);

        Ok(Insertion {
            keystroke_sent: true,
            clipboard: restore,
            method: self.keystroke.describe(),
        })
    }

    /// The rule: put the original back only if the clipboard still holds what we
    /// wrote.
    fn restore(&self, original: Option<&str>, pasted: &str) -> ClipboardRestore {
        let Some(original) = original else {
            return ClipboardRestore::NothingToRestore;
        };

        match self.clipboard.read_text() {
            Ok(current) if current == pasted => match self.clipboard.write_text(original) {
                Ok(()) => ClipboardRestore::Restored,
                Err(err) => ClipboardRestore::Failed {
                    error: err.to_string(),
                },
            },
            Ok(_) => {
                // Lengths only. Logging either value would put clipboard
                // contents in a log file, which the brief forbids.
                tracing::debug!(
                    pasted_len = pasted.len(),
                    "clipboard changed during the paste window; leaving the user's content alone"
                );
                ClipboardRestore::SkippedUserChangedIt
            }
            Err(err) => {
                // Cannot tell whether it changed, so do not overwrite it. The
                // conservative choice loses a restore rather than a user's copy.
                tracing::debug!(error = %err, "could not verify the clipboard; not restoring");
                ClipboardRestore::SkippedUserChangedIt
            }
        }
    }
}

/// The real clipboard, via `arboard`.
///
/// A fresh handle per operation. `arboard` holds an OS clipboard connection
/// open, and on X11 the owning process must stay alive to serve paste requests;
/// short-lived handles keep that window small and avoid holding a selection we
/// no longer own.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClipboard;

impl Clipboard for SystemClipboard {
    fn read_text(&self) -> Result<String, PlatformError> {
        let mut clipboard =
            arboard::Clipboard::new().map_err(|e| PlatformError::Clipboard(e.to_string()))?;
        clipboard
            .get_text()
            .map_err(|e| PlatformError::Clipboard(e.to_string()))
    }

    fn write_text(&self, text: &str) -> Result<(), PlatformError> {
        let mut clipboard =
            arboard::Clipboard::new().map_err(|e| PlatformError::Clipboard(e.to_string()))?;
        clipboard
            .set_text(text.to_owned())
            .map_err(|e| PlatformError::Clipboard(e.to_string()))
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
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    /// A clipboard that can be scripted, including into failure.
    #[derive(Debug, Default)]
    struct FakeClipboard {
        content: Mutex<Option<String>>,
        /// Simulates the user copying something, once, at the given read index.
        hijack_at_read: Mutex<Option<(usize, String)>>,
        reads: AtomicUsize,
        writes: Mutex<Vec<String>>,
        read_fails: bool,
        write_fails: bool,
    }

    impl FakeClipboard {
        fn holding(text: &str) -> Self {
            Self {
                content: Mutex::new(Some(text.to_owned())),
                ..Default::default()
            }
        }

        fn empty() -> Self {
            Self::default()
        }

        /// After `index` reads, pretend the user copied `text`.
        fn hijacked_after(mut self, index: usize, text: &str) -> Self {
            self.hijack_at_read = Mutex::new(Some((index, text.to_owned())));
            self
        }

        fn writes(&self) -> Vec<String> {
            self.writes.lock().unwrap().clone()
        }
    }

    impl Clipboard for FakeClipboard {
        fn read_text(&self) -> Result<String, PlatformError> {
            if self.read_fails {
                return Err(PlatformError::Clipboard("no clipboard".to_owned()));
            }
            let n = self.reads.fetch_add(1, Ordering::SeqCst);

            if let Some((at, text)) = self.hijack_at_read.lock().unwrap().clone() {
                if n >= at {
                    return Ok(text);
                }
            }

            Ok(self.content.lock().unwrap().clone().unwrap_or_default())
        }

        fn write_text(&self, text: &str) -> Result<(), PlatformError> {
            if self.write_fails {
                return Err(PlatformError::Clipboard("read-only clipboard".to_owned()));
            }
            self.writes.lock().unwrap().push(text.to_owned());
            *self.content.lock().unwrap() = Some(text.to_owned());
            Ok(())
        }
    }

    #[derive(Debug, Default)]
    struct FakeKeystroke {
        sent: AtomicUsize,
        fails: bool,
    }

    impl PasteKeystroke for FakeKeystroke {
        fn send(&self) -> Result<(), PlatformError> {
            if self.fails {
                return Err(PlatformError::TextInsertion(
                    "no foreground window".to_owned(),
                ));
            }
            self.sent.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }

        fn describe(&self) -> &'static str {
            "fake"
        }
    }

    fn inserter<C: Clipboard>(c: C, k: FakeKeystroke) -> TextInserter<C, FakeKeystroke> {
        // Zero delay so the tests do not actually wait half a second.
        TextInserter::new(c, k, Duration::ZERO)
    }

    #[tokio::test]
    async fn pastes_the_text_and_puts_the_clipboard_back() {
        let clipboard = FakeClipboard::holding("something the user copied earlier");
        let keys = FakeKeystroke::default();
        let inserter = inserter(clipboard, keys);

        let outcome = inserter.insert("dictated sentence").await.unwrap();

        assert!(outcome.keystroke_sent);
        assert_eq!(outcome.clipboard, ClipboardRestore::Restored);
        assert_eq!(
            inserter.clipboard.writes(),
            vec!["dictated sentence", "something the user copied earlier"],
            "the text is written, then the original is put back"
        );
        assert_eq!(inserter.keystroke.sent.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn does_not_clobber_a_copy_the_user_made_during_the_paste_window() {
        // This is the assertion that matters. The user pressed Ctrl+C while the
        // paste was in flight; their content must survive.
        let clipboard = FakeClipboard::holding("original")
            // read 0 is the save; read 1 is the verify, by which point the user
            // has copied something.
            .hijacked_after(1, "what the user just copied");
        let inserter = inserter(clipboard, FakeKeystroke::default());

        let outcome = inserter.insert("dictated").await.unwrap();

        assert_eq!(outcome.clipboard, ClipboardRestore::SkippedUserChangedIt);
        assert_eq!(
            inserter.clipboard.writes(),
            vec!["dictated"],
            "only the paste was written; the original was NOT restored over the user's copy"
        );
    }

    #[tokio::test]
    async fn an_empty_clipboard_means_there_is_nothing_to_restore() {
        let inserter = inserter(FakeClipboard::empty(), FakeKeystroke::default());
        let outcome = inserter.insert("dictated").await.unwrap();

        assert_eq!(outcome.clipboard, ClipboardRestore::NothingToRestore);
        assert_eq!(inserter.clipboard.writes(), vec!["dictated"]);
    }

    #[tokio::test]
    async fn empty_text_does_not_touch_the_clipboard_at_all() {
        let inserter = inserter(FakeClipboard::holding("precious"), FakeKeystroke::default());
        let outcome = inserter.insert("").await.unwrap();

        assert!(!outcome.keystroke_sent);
        assert!(
            inserter.clipboard.writes().is_empty(),
            "nothing should have been written"
        );
        assert_eq!(inserter.keystroke.sent.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn an_unreadable_clipboard_still_allows_the_paste() {
        // An image-only clipboard reads as an error on some platforms. That must
        // not stop dictation working.
        let clipboard = FakeClipboard {
            read_fails: true,
            ..Default::default()
        };
        let inserter = inserter(clipboard, FakeKeystroke::default());

        let outcome = inserter.insert("dictated").await.unwrap();
        assert!(outcome.keystroke_sent);
        assert_eq!(outcome.clipboard, ClipboardRestore::NothingToRestore);
    }

    #[tokio::test]
    async fn a_clipboard_that_cannot_be_verified_is_left_alone() {
        // Read succeeds for the save, then starts failing. We cannot tell
        // whether the user changed it, so we must not overwrite it.
        #[derive(Debug)]
        struct FlakyClipboard {
            reads: AtomicUsize,
            writes: Mutex<Vec<String>>,
        }
        impl Clipboard for FlakyClipboard {
            fn read_text(&self) -> Result<String, PlatformError> {
                if self.reads.fetch_add(1, Ordering::SeqCst) == 0 {
                    Ok("original".to_owned())
                } else {
                    Err(PlatformError::Clipboard("clipboard busy".to_owned()))
                }
            }
            fn write_text(&self, text: &str) -> Result<(), PlatformError> {
                self.writes.lock().unwrap().push(text.to_owned());
                Ok(())
            }
        }

        let inserter = TextInserter::new(
            FlakyClipboard {
                reads: AtomicUsize::new(0),
                writes: Mutex::new(Vec::new()),
            },
            FakeKeystroke::default(),
            Duration::ZERO,
        );
        let outcome = inserter.insert("dictated").await.unwrap();

        assert_eq!(outcome.clipboard, ClipboardRestore::SkippedUserChangedIt);
        assert_eq!(
            inserter.clipboard.writes.lock().unwrap().clone(),
            vec!["dictated"]
        );
    }

    #[tokio::test]
    async fn a_failed_keystroke_is_an_error_rather_than_a_silent_success() {
        let inserter = inserter(
            FakeClipboard::holding("original"),
            FakeKeystroke {
                fails: true,
                ..Default::default()
            },
        );
        let err = inserter.insert("dictated").await.unwrap_err();
        assert!(matches!(err, PlatformError::TextInsertion(_)));
    }

    #[tokio::test]
    async fn a_clipboard_that_cannot_be_written_fails_before_any_keystroke() {
        let clipboard = FakeClipboard {
            write_fails: true,
            ..Default::default()
        };
        let inserter = inserter(clipboard, FakeKeystroke::default());

        assert!(inserter.insert("dictated").await.is_err());
        assert_eq!(
            inserter.keystroke.sent.load(Ordering::SeqCst),
            0,
            "pasting an unchanged clipboard would insert the wrong text"
        );
    }

    #[tokio::test]
    async fn a_failed_restore_is_reported_rather_than_swallowed() {
        #[derive(Debug)]
        struct WriteOnceClipboard {
            writes: Mutex<Vec<String>>,
        }
        impl Clipboard for WriteOnceClipboard {
            fn read_text(&self) -> Result<String, PlatformError> {
                let writes = self.writes.lock().unwrap();
                Ok(writes
                    .last()
                    .cloned()
                    .unwrap_or_else(|| "original".to_owned()))
            }
            fn write_text(&self, text: &str) -> Result<(), PlatformError> {
                let mut writes = self.writes.lock().unwrap();
                if writes.is_empty() {
                    writes.push(text.to_owned());
                    Ok(())
                } else {
                    Err(PlatformError::Clipboard(
                        "clipboard locked by another process".to_owned(),
                    ))
                }
            }
        }

        let inserter = TextInserter::new(
            WriteOnceClipboard {
                writes: Mutex::new(Vec::new()),
            },
            FakeKeystroke::default(),
            Duration::ZERO,
        );
        let outcome = inserter.insert("dictated").await.unwrap();
        assert!(matches!(outcome.clipboard, ClipboardRestore::Failed { .. }));
    }

    #[tokio::test]
    async fn unicode_text_survives_the_round_trip() {
        // The reason for clipboard-plus-paste rather than key injection.
        let text = "Håkan sa: 日本語 — naïve café 🙂";
        let inserter = inserter(FakeClipboard::holding("x"), FakeKeystroke::default());
        let outcome = inserter.insert(text).await.unwrap();

        assert_eq!(outcome.clipboard, ClipboardRestore::Restored);
        assert_eq!(inserter.clipboard.writes()[0], text);
    }

    #[test]
    fn the_restore_delay_is_long_enough_to_be_plausible() {
        // A restore that races the target application's clipboard read pastes
        // the wrong text, so this must not drift towards zero.
        assert!(
            restore_delay() >= Duration::from_millis(400),
            "{:?}",
            restore_delay()
        );
    }
}
