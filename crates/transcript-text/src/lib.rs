//! Transcript text comparisons, without engine or database dependencies.
pub fn words_match_text<'a>(words: impl IntoIterator<Item = &'a str>, text: &str) -> bool {
    words
        .into_iter()
        .flat_map(str::chars)
        .filter(|c| !c.is_whitespace())
        .eq(text.chars().filter(|c| !c.is_whitespace()))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn compares_unicode_and_ignores_whitespace_without_ignoring_punctuation() {
        assert!(words_match_text(["Hej", "värld!"], "Hej\n värld!"));
        assert!(!words_match_text(["Hej", "värld"], "Hej värld!"));
        assert!(!words_match_text([], "speech"));
        assert!(words_match_text([], " "));
    }
}
