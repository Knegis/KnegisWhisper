//! One conversion for words saved by imports and meetings.
pub fn stored_words(words: &[kw_transcription_core::local::TimedWord]) -> Vec<kw_notes_core::Word> {
    words
        .iter()
        .map(|word| kw_notes_core::Word {
            text: word.text.clone(),
            start: word.start,
            end: word.end,
        })
        .collect()
}
