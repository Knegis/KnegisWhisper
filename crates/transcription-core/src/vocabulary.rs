//! Bounded recognition hints. Values are never included in diagnostic output.
use serde::Serialize;

#[derive(Clone, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct OmittedTerm {
    pub word: String,
    pub reason: String,
}

impl std::fmt::Debug for OmittedTerm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OmittedTerm")
            .field("word_chars", &self.word.chars().count())
            .field("reason", &self.reason)
            .finish()
    }
}

#[derive(Clone, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct VocabularyReview {
    pub supported: bool,
    pub model: Option<String>,
    pub included: Vec<String>,
    pub omitted: Vec<OmittedTerm>,
    pub limit: String,
}
impl std::fmt::Debug for VocabularyReview {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VocabularyReview")
            .field("supported", &self.supported)
            .field("included_count", &self.included.len())
            .field("omitted_count", &self.omitted.len())
            .finish()
    }
}
impl VocabularyReview {
    pub fn prompt(&self) -> Option<String> {
        (!self.included.is_empty()).then(|| self.included.join(", "))
    }
}

/// Application budgets are deliberately conservative byte limits, not token counts.
#[must_use]
pub fn prepare(provider: &str, model: Option<&str>, words: &[String]) -> VocabularyReview {
    let (supported, max_terms, max_chars, max_words, max_bytes, limit) = match provider {
        "whisper" => (
            true,
            usize::MAX,
            100,
            usize::MAX,
            200,
            "200 UTF-8 bytes (application prompt budget)",
        ),
        "assemblyai-realtime" => (
            true,
            100,
            50,
            usize::MAX,
            usize::MAX,
            "100 terms; 50 characters per term",
        ),
        "corti-realtime" => (
            true,
            1000,
            50,
            5,
            usize::MAX,
            "1,000 terms; 50 characters and 5 words per term",
        ),
        "deepgram-realtime" if model.is_none_or(|m| m == "nova-3" || m.starts_with("nova-3-")) => (
            true,
            100,
            100,
            usize::MAX,
            400,
            "100 terms; 400 UTF-8 bytes (application budget)",
        ),
        _ => (
            false,
            0,
            100,
            usize::MAX,
            0,
            "Not supported by this integration/model",
        ),
    };
    let mut result = VocabularyReview {
        supported,
        model: model.map(str::to_owned),
        included: Vec::new(),
        omitted: Vec::new(),
        limit: limit.into(),
    };
    if provider == "deepgram-realtime" && supported && !words.is_empty() {
        result.model = Some(model.unwrap_or("nova-3").into());
    }
    let mut terms: Vec<String> = words
        .iter()
        .map(|w| w.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|w| !w.is_empty())
        .collect();
    terms.sort_by_cached_key(|w| (w.to_lowercase(), w.clone()));
    terms.dedup();
    let mut bytes = 0usize;
    for word in terms {
        let cost = word.len() + if result.included.is_empty() { 0 } else { 2 };
        let reason = if !supported {
            Some("Not supported by this integration/model")
        } else if word.chars().count() > max_chars {
            Some("Term exceeds the character limit")
        } else if word.split_whitespace().count() > max_words {
            Some("Term exceeds the word limit")
        } else if result.included.len() >= max_terms {
            Some("Term count limit reached")
        } else if cost > max_bytes.saturating_sub(bytes) {
            Some("Application byte budget reached")
        } else {
            None
        };
        if let Some(reason) = reason {
            result.omitted.push(OmittedTerm {
                word,
                reason: reason.into(),
            });
        } else {
            bytes += cost;
            result.included.push(word);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preserves_spelling_and_whole_terms_with_unicode_byte_budget() {
        let terms = vec![
            "  Åsa   Karlsson ".into(),
            "Åsa Karlsson".into(),
            "a".repeat(100),
            "b".repeat(100),
            "Dyve".into(),
        ];
        let review = prepare("whisper", None, &terms);
        assert!(review.included.contains(&"Åsa Karlsson".into()));
        assert!(review.prompt().is_some_and(|p| p.len() <= 200));
        assert_eq!(review.omitted.len(), 1);
        assert!(!format!("{review:?}").contains("Karlsson"));
    }
    #[test]
    fn limits_are_provider_specific_and_unsupported_sends_nothing() {
        let terms = vec![
            "x".repeat(51),
            "one two three four five six".into(),
            "Dyve".into(),
        ];
        assert_eq!(
            prepare("assemblyai-realtime", None, &terms).included.len(),
            2
        );
        assert_eq!(prepare("corti-realtime", None, &terms).included, ["Dyve"]);
        assert!(prepare("deepgram-realtime", Some("nova-2"), &terms)
            .included
            .is_empty());
        assert!(prepare("local", None, &terms).prompt().is_none());
        assert!(prepare("whisper", None, &[]).prompt().is_none());
    }
}
