//! Startup errors must reach the person even before a window exists.

use std::error::Error;
use std::path::Path;

/// Explain the failure and where to find its log, without repeating embedded causes.
pub fn message(error: &dyn Error, log: &Path) -> String {
    let text = error.to_string();
    let mut previous = if text.trim().is_empty() {
        "No reason was given.".to_owned()
    } else {
        text
    };
    let mut lines = vec![
        "KnegisWhisper could not start.".to_owned(),
        previous.clone(),
    ];
    let mut source = error.source();
    while let Some(cause) = source {
        let text = cause.to_string();
        if !text.trim().is_empty() && !previous.contains(&text) {
            lines.push(text.clone());
            previous = text;
        }
        source = cause.source();
    }
    lines.push(format!("The log file is at {}.", log.display()));
    lines.join("\n")
}

/// Log the failure and wait for a native Windows message box to be answered.
///
/// Other platforms print to stderr only; neither has been run, a known gap.
/// Reporting and the native box are not unit-tested; they need a manual check.
pub fn report(error: &dyn Error) {
    let message = message(error, &crate::logging::log_path());
    tracing::error!(%message, "startup failed");

    #[cfg(windows)]
    {
        use windows::core::PCWSTR;
        use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};

        let message: Vec<u16> = message.encode_utf16().chain(Some(0)).collect();
        // SAFETY: The message is null-terminated UTF-16 and stays alive for the
        // blocking call. The title is static, and no owner window is required.
        unsafe {
            MessageBoxW(
                None,
                PCWSTR(message.as_ptr()),
                windows::core::w!("KnegisWhisper"),
                MB_OK | MB_ICONERROR,
            );
        }
    }
    #[cfg(not(windows))]
    eprintln!("{message}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct Failure {
        text: &'static str,
        source: Option<Box<Failure>>,
    }

    impl std::fmt::Display for Failure {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str(self.text)
        }
    }

    impl Error for Failure {
        fn source(&self) -> Option<&(dyn Error + 'static)> {
            self.source.as_deref().map(|source| source as &dyn Error)
        }
    }

    fn failure(text: &'static str, source: Option<Failure>) -> Failure {
        Failure {
            text,
            source: source.map(Box::new),
        }
    }

    #[test]
    fn includes_error_and_log_path() {
        let message = message(
            &failure("could not open the database", None),
            Path::new("logs/app.log"),
        );
        assert!(message.contains("could not open the database"));
        assert!(message.contains("logs/app.log"));
    }

    #[test]
    fn sources_appear_in_order_on_separate_lines() {
        let error = failure(
            "could not open the database",
            Some(failure(
                "migration failed",
                Some(failure("disk full", None)),
            )),
        );
        let message = message(&error, Path::new("app.log"));
        assert_eq!(
            message.lines().collect::<Vec<_>>(),
            vec![
                "KnegisWhisper could not start.",
                "could not open the database",
                "migration failed",
                "disk full",
                "The log file is at app.log.",
            ]
        );
    }

    #[test]
    fn embedded_sources_are_not_repeated() {
        let error = failure(
            "database failed: migration failed: disk full",
            Some(failure(
                "migration failed: disk full",
                Some(failure("disk full", None)),
            )),
        );
        let message = message(&error, Path::new("app.log"));
        assert_eq!(message.matches("migration failed").count(), 1);
        assert_eq!(message.matches("disk full").count(), 1);
    }

    #[test]
    fn empty_error_has_a_readable_explanation() {
        for text in ["", "   "] {
            let message = message(&failure(text, None), Path::new("app.log"));
            assert_eq!(
                message,
                "KnegisWhisper could not start.\nNo reason was given.\nThe log file is at app.log."
            );
        }
    }
}
