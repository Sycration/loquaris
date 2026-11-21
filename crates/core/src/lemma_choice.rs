//! Keys that attach a [`LemmaChoice`](crate::LemmaChoice) to one word of an
//! entry: its normalized spelling and its occurrence among words spelled
//! alike.

/// Choices ignore quantity marks and case, so they survive macronization;
/// a reading that conflicts with written marks is never chosen.
pub fn lemma_choice_key(token: &str) -> String {
    collo::normalize(token).to_lowercase()
}

/// Number each word by how many earlier words in the text share its spelling,
/// as [`LemmaChoice::occurrence`](crate::LemmaChoice::occurrence) does.
/// Entries are short, so a list is quicker than hashing.
#[derive(Default)]
pub struct OccurrenceCounter(Vec<(String, u32)>);

impl OccurrenceCounter {
    pub fn new() -> Self {
        Self::default()
    }

    /// The word's key and occurrence number.
    pub fn next(&mut self, token: &str) -> (String, u32) {
        let key = lemma_choice_key(token);
        if let Some((_, count)) = self.0.iter_mut().find(|(seen, _)| *seen == key) {
            let occurrence = *count;
            *count += 1;
            return (key, occurrence);
        }
        self.0.push((key.clone(), 1));
        (key, 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn occurrences_count_words_spelled_alike_ignoring_case_and_marks() {
        assert_eq!(lemma_choice_key("Bēllum"), "bellum");
        let mut counter = OccurrenceCounter::new();
        let numbered: Vec<_> = ["Bellum", "pilum", "bēllum", "BELLUM"]
            .into_iter()
            .map(|word| counter.next(word))
            .collect();
        assert_eq!(
            numbered,
            [
                ("bellum".into(), 0),
                ("pilum".into(), 0),
                ("bellum".into(), 1),
                ("bellum".into(), 2),
            ]
        );
    }
}
