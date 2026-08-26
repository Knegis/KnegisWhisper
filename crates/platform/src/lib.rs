//! Operating-system integration.
//!
//! Everything in this crate is either a pure algorithm that happens to be about
//! the OS (hotkey parsing, the clipboard-restore rule) or a thin wrapper over a
//! platform API. The split is deliberate: the algorithms are tested everywhere,
//! and the wrappers are small enough to read.
//!
//! Where a platform cannot do something, this crate says so. It does not
//! substitute a weaker mechanism and report success — see
//! [`capture_exclusion`](window::exclude_from_capture) in particular, which is
//! the specific defect the upstream audit found in Pluely.

pub mod hotkey;
pub mod insertion;
pub mod keyhook;
pub mod keys;
pub mod window;

/// Failures from talking to the operating system.
#[derive(Debug, thiserror::Error)]
pub enum PlatformError {
    #[error("clipboard: {0}")]
    Clipboard(String),

    #[error("could not deliver the paste keystroke: {0}")]
    TextInsertion(String),

    #[error("window operation failed: {0}")]
    Window(String),

    /// The platform has no such facility. Distinct from a failure: nothing went
    /// wrong, the capability does not exist here.
    #[error("not supported on this platform: {0}")]
    Unsupported(String),
}

/// The paste keystroke for this platform, or `None` where synthetic paste is not
/// implemented.
///
/// Returning `None` rather than a no-op implementation is the point: a caller
/// that gets `None` must tell the user text insertion will not work, instead of
/// pasting into the void.
#[must_use]
pub fn paste_keystroke() -> Option<Box<dyn insertion::PasteKeystroke>> {
    #[cfg(target_os = "windows")]
    {
        Some(Box::new(window::WindowsPaste))
    }
    #[cfg(not(target_os = "windows"))]
    {
        // macOS needs a CGEvent ⌘V (and Accessibility permission); Linux needs
        // XTEST or an xdg-desktop-portal RemoteDesktop session. Both are real
        // work with no reference implementation to copy, and neither is
        // verifiable from this machine, so neither is pretended at.
        None
    }
}

/// Build the text inserter for this platform, if text insertion is possible.
#[must_use]
pub fn text_inserter(
) -> Option<insertion::TextInserter<insertion::SystemClipboard, Box<dyn insertion::PasteKeystroke>>>
{
    paste_keystroke().map(|keystroke| {
        insertion::TextInserter::new(
            insertion::SystemClipboard,
            keystroke,
            insertion::restore_delay(),
        )
    })
}

impl insertion::PasteKeystroke for Box<dyn insertion::PasteKeystroke> {
    fn send(&self) -> Result<(), PlatformError> {
        (**self).send()
    }

    fn describe(&self) -> &'static str {
        (**self).describe()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn text_insertion_availability_matches_the_platform_we_implemented() {
        // Windows is the platform this build targets; the others are honest
        // about being unimplemented rather than silently doing nothing.
        if cfg!(target_os = "windows") {
            assert!(paste_keystroke().is_some());
            assert!(text_inserter().is_some());
        } else {
            assert!(paste_keystroke().is_none());
            assert!(text_inserter().is_none());
        }
    }

    #[test]
    fn unsupported_is_distinguishable_from_a_failure() {
        let unsupported = PlatformError::Unsupported("wayland".to_owned());
        let failure = PlatformError::Window("E_INVALIDARG".to_owned());
        assert!(matches!(unsupported, PlatformError::Unsupported(_)));
        assert!(!matches!(failure, PlatformError::Unsupported(_)));
    }
}
