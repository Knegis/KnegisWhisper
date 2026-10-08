//! Where a note's transcript was made, named for the note's meta row
//! (KNEG-368).
//!
//! Pure, so the names are tested here rather than read off a saved note. The
//! frontend only phrases what this says: "Transcribed on this computer" or
//! "Transcribed by {name}".

use kw_notes_core::TranscribedBy;

/// The route a transcript came back from, named as a person reads it, or
/// `None` for one nothing here names -- a note then says nothing rather than
/// a guess.
///
/// `provider` is what the route reports: `local` from the executor and the
/// whole-recording pass, a live meeting provider's id, a transcription
/// registry id, or the two self-run servers' slot names.
pub(crate) fn route(provider: &str) -> Option<TranscribedBy> {
    let (name, on_this_computer) = match provider {
        "local" | "local-streaming" => ("this computer".to_owned(), true),
        "self-hosted" => ("your self-hosted server".to_owned(), false),
        // KNEG-415's name for the custom endpoint's slot.
        "custom" => ("your own server".to_owned(), false),
        _ => {
            if let Some(live) = kw_meeting_core::capabilities(provider) {
                (live.name.to_owned(), !live.audio_leaves_device)
            } else {
                let registry = kw_provider_registry::registry().ok()?;
                let cloud = registry.transcription_provider(provider).ok()?;
                (cloud.name.clone(), false)
            }
        }
    };
    Some(TranscribedBy {
        provider: provider.to_owned(),
        name,
        on_this_computer,
        live_name: None,
    })
}

/// Saved transcript provenance with the separately selected live provider.
/// Unknown providers are not guessed; local captions need no cloud disclosure.
pub(crate) fn with_live_provider(
    provider: &str,
    live_provider: Option<&str>,
) -> Option<TranscribedBy> {
    let mut by = route(provider)?;
    if by.on_this_computer {
        by.live_name = live_provider
            .and_then(route)
            .filter(|live| !live.on_this_computer)
            .map(|live| live.name);
    }
    Some(by)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn by(provider: &str, name: &str, on_this_computer: bool) -> TranscribedBy {
        TranscribedBy {
            provider: provider.to_owned(),
            name: name.to_owned(),
            on_this_computer,
            live_name: None,
        }
    }

    #[test]
    fn each_route_is_named_and_placed() {
        for (provider, expected) in [
            ("local", Some(by("local", "this computer", true))),
            (
                "local-streaming",
                Some(by("local-streaming", "this computer", true)),
            ),
            ("openai", Some(by("openai", "OpenAI", false))),
            ("berget", Some(by("berget", "Berget AI", false))),
            (
                "deepgram-realtime",
                Some(by("deepgram-realtime", "Deepgram Realtime", false)),
            ),
            (
                "self-hosted",
                Some(by("self-hosted", "your self-hosted server", false)),
            ),
            ("custom", Some(by("custom", "your own server", false))),
            ("fixture", None),
            ("", None),
        ] {
            assert_eq!(route(provider), expected, "{provider:?}");
        }
    }
}
