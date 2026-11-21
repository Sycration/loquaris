//! Parsing review: choosing a reading for each word of a Latin entry, which
//! macronizes it and records the lemma of each ambiguous word. collo selects
//! one reading from context; it does not rank the others.
use std::collections::BTreeSet;
use std::ops::Range;
use std::sync::Arc;

use collo::tagging::TaggedWord;

use crate::language::morphology_language;
use crate::lemma_choice::OccurrenceCounter;
use crate::macronize::{self, QuantityMatch};
use crate::word_index::{LemmaInfo, WordIndex};
use crate::{Language, LemmaChoice};

/// A text's words with their readings, and the reading chosen for each
/// (`None` keeps the original spelling).
#[derive(Clone, Debug)]
pub struct Review {
    pub original: String,
    words: Vec<TaggedWord>,
    choices: Vec<Option<usize>>,
}

impl Review {
    /// Review `original`, tagged with collo, starting each word at collo's
    /// context reading when its spelling can be aligned.
    pub fn new(original: String) -> Self {
        let words = collo::Lemmatizer::new().tag_text(&original);
        Self::from_tagged_words(original, words)
    }

    pub fn from_tagged_words(original: String, words: Vec<TaggedWord>) -> Self {
        let choices = words
            .iter()
            .map(|word| word.selected.filter(|&index| Self::applicable(word, index)))
            .collect();
        Self {
            original,
            words,
            choices,
        }
    }

    pub fn words(&self) -> &[TaggedWord] {
        &self.words
    }

    pub fn choices(&self) -> &[Option<usize>] {
        &self.choices
    }

    /// Choose a reading for a word, or `None` to keep its spelling. A reading
    /// that can't be applied keeps the spelling too.
    pub fn choose(&mut self, word: usize, reading: Option<usize>) {
        if let Some(choice) = self.choices.get_mut(word) {
            *choice = reading.filter(|&index| Self::applicable(&self.words[word], index));
        }
    }

    /// Whether the reading's spelling aligns with the word, so its marks can
    /// be applied safely.
    fn applicable(word: &TaggedWord, reading: usize) -> bool {
        word.analyses
            .get(reading)
            .is_some_and(|analysis| macronize::analysis_word(&word.text, analysis, true).is_some())
    }

    /// Whether the word's readings belong to more than one lemma, so the
    /// lemma must be chosen. Written quantities don't narrow them, since they
    /// may be mistaken.
    pub fn lemma_ambiguous(word: &TaggedWord) -> bool {
        let lemmas: BTreeSet<_> = word
            .analyses
            .iter()
            .map(|analysis| (analysis.lemma.key, analysis.lemma.extension))
            .collect();
        lemmas.len() > 1
    }

    /// Start from the lemmas chosen when this entry was last reviewed, keeping
    /// the contextual reading if it has the chosen lemma.
    pub fn prefer_lemmas(&mut self, chosen: &[LemmaChoice]) {
        let mut occurrences = OccurrenceCounter::new();
        for (index, word) in self.words.iter().enumerate() {
            let (key, occurrence) = occurrences.next(&word.text);
            let Some(choice) = chosen
                .iter()
                .find(|choice| choice.token == key && choice.occurrence == occurrence)
            else {
                continue;
            };
            let has_lemma = |candidate: usize| {
                let analysis = &word.analyses[candidate];
                analysis.lemma.key == choice.lemma_key
                    && analysis.lemma.extension == choice.extension
                    && macronize::quantity_match(&word.text, analysis) != QuantityMatch::Conflict
                    && macronize::analysis_word(&word.text, analysis, true).is_some()
            };
            if !self.choices[index].is_some_and(has_lemma)
                && let Some(candidate) =
                    (0..word.analyses.len()).find(|&candidate| has_lemma(candidate))
            {
                self.choices[index] = Some(candidate);
            }
        }
    }

    /// The lemmas of the chosen readings of words that need one chosen.
    pub fn lemma_choices(&self) -> Vec<LemmaChoice> {
        let mut occurrences = OccurrenceCounter::new();
        let mut choices = Vec::new();
        for (index, word) in self.words.iter().enumerate() {
            let (token, occurrence) = occurrences.next(&word.text);
            let Some(analysis) = self.choices[index].and_then(|choice| word.analyses.get(choice))
            else {
                continue;
            };
            if Self::lemma_ambiguous(word) {
                choices.push(LemmaChoice {
                    token,
                    occurrence,
                    lemma_key: analysis.lemma.key.to_owned(),
                    extension: analysis.lemma.extension,
                });
            }
        }
        choices
    }

    /// The word as its chosen reading spells it.
    pub fn word_preview(&self, index: usize) -> String {
        let word = &self.words[index];
        self.choices[index]
            .and_then(|choice| word.analyses.get(choice))
            .and_then(|analysis| macronize::analysis_word(&word.text, analysis, true))
            .unwrap_or_else(|| word.text.clone())
    }

    /// The text with every chosen reading applied.
    pub fn preview(&self) -> String {
        let mut text = String::with_capacity(self.original.len());
        let mut cursor = 0;
        for (index, word) in self.words.iter().enumerate() {
            text.push_str(&self.original[cursor..word.range.start]);
            text.push_str(&self.word_preview(index));
            cursor = word.range.end;
        }
        text.push_str(&self.original[cursor..]);
        text
    }

    /// Return a result only when every available reading of every word produces
    /// the same safe spelling and has a single lemma, even when a lemma was
    /// chosen before, so any ambiguity is reviewed. Unknown words stay unchanged.
    pub fn unambiguous_preview(&self) -> Option<String> {
        let mut text = String::with_capacity(self.original.len());
        let mut cursor = 0;
        for word in &self.words {
            text.push_str(&self.original[cursor..word.range.start]);
            if Self::lemma_ambiguous(word) {
                return None;
            }
            let mut result = None;
            for analysis in &word.analyses {
                let spelling = macronize::analysis_word(&word.text, analysis, true)?;
                if result
                    .as_ref()
                    .is_some_and(|previous| *previous != spelling)
                {
                    return None;
                }
                result = Some(spelling);
            }
            text.push_str(result.as_deref().unwrap_or(&word.text));
            cursor = word.range.end;
        }
        text.push_str(&self.original[cursor..]);
        Some(text)
    }
}

/// A reading of a word in the review.
#[derive(Clone, Debug)]
pub struct ReviewReading {
    /// The word as this reading spells it, or `None` when the reading has a
    /// different spelling and so can't be applied safely.
    pub spelling: Option<String>,
    pub lemma: LemmaInfo,
    /// In French or Spanish when the L1 is, else English.
    pub morphology: Option<&'static str>,
}

/// A word in the review.
#[derive(Clone, Debug)]
pub struct ReviewWord {
    pub text: String,
    /// Byte range in the entry text.
    pub range: Range<usize>,
    /// In collo's order, which is how choices refer to them. Empty for words
    /// without dictionary analyses, whose spelling is kept.
    pub readings: Vec<ReviewReading>,
    /// The order to list readings in: the context reading first.
    pub display_order: Vec<usize>,
    /// collo's choice from context.
    pub context_reading: Option<usize>,
    /// The reading selected when the review opens: the context reading if it
    /// can be applied, replaced by a saved lemma choice when possible.
    /// `None` keeps the original spelling.
    pub preselected: Option<usize>,
    /// The readings have more than one lemma, so a lemma choice is saved.
    pub lemma_ambiguous: bool,
}

/// Text and lemma choices to write to an entry after a review.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppliedReview {
    pub text: String,
    pub lemmas: Vec<LemmaChoice>,
    /// Whether this differs from the entry, so there is anything to apply.
    pub changed: bool,
}

/// What the parsing review dialog shows for an entry.
#[derive(Clone, Debug)]
pub struct ReviewModel {
    pub entry_id: Arc<str>,
    /// The text reviewed; applying choices requires the entry to still have it.
    pub original: String,
    pub words: Vec<ReviewWord>,
    /// When no word is ambiguous, the result to apply without asking. If its
    /// text is unchanged there is nothing to do.
    pub direct: Option<AppliedReview>,
}

impl WordIndex {
    /// The review of a Latin entry as collo reads it, preselecting the entry's
    /// saved lemma choices. `None` unless the entry is known and Latin.
    pub fn review(&mut self, entry_id: &str) -> Option<ReviewModel> {
        let (review, id) = self.start_review(entry_id)?;
        let morphology = morphology_language(self.translation_language());
        let words = review
            .words()
            .iter()
            .zip(review.choices())
            .map(|(word, &preselected)| {
                let mut display_order: Vec<_> = (0..word.analyses.len()).collect();
                if let Some(context) = word.selected
                    && context < display_order.len()
                {
                    display_order.swap(0, context);
                }
                ReviewWord {
                    text: word.text.clone(),
                    range: word.range.clone(),
                    readings: word
                        .analyses
                        .iter()
                        .map(|analysis| ReviewReading {
                            spelling: macronize::analysis_word(&word.text, analysis, true),
                            lemma: self.lemma_info(analysis.lemma),
                            morphology: analysis.morphology(morphology),
                        })
                        .collect(),
                    display_order,
                    context_reading: word.selected,
                    preselected,
                    lemma_ambiguous: Review::lemma_ambiguous(word),
                }
            })
            .collect();
        let direct = review.unambiguous_preview().map(|text| AppliedReview {
            changed: text != review.original,
            text,
            lemmas: Vec::new(),
        });
        Some(ReviewModel {
            entry_id: id,
            original: review.original,
            words,
            direct,
        })
    }

    /// Apply the reading chosen for each word (`None` keeps its spelling) to
    /// the entry reviewed with text `original`. `None` if the entry is gone,
    /// isn't Latin, or its text changed since.
    pub fn apply_review(
        &mut self,
        entry_id: &str,
        original: &str,
        choices: &[Option<usize>],
    ) -> Option<AppliedReview> {
        if self.entry_text(entry_id)? != original {
            return None;
        }
        let (mut review, _) = self.start_review(entry_id)?;
        if choices.len() != review.words().len() {
            return None;
        }
        for (word, &reading) in choices.iter().enumerate() {
            review.choose(word, reading);
        }
        let text = review.preview();
        let lemmas = review.lemma_choices();
        let changed = text != original || self.entry_lemmas(entry_id)? != lemmas;
        Some(AppliedReview {
            text,
            lemmas,
            changed,
        })
    }

    fn start_review(&mut self, entry_id: &str) -> Option<(Review, Arc<str>)> {
        if self.entry_language(entry_id)? != Language::Latin {
            return None;
        }
        let text = self.entry_text(entry_id)?.to_owned();
        let words = self.tag_text(&text);
        let mut review = Review::from_tagged_words(text, words);
        review.prefer_lemmas(self.entry_lemmas(entry_id)?);
        let id = self.entry_id_arc(entry_id)?;
        Some((review, id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_uses_choices_and_preserves_separators() {
        let mut review = Review::new("Puellas, xyzzy!\npuellas.".into());
        assert_eq!(review.preview(), "Puellās, xyzzy!\npuellās.");
        review.choose(0, None);
        assert_eq!(review.preview(), "Puellas, xyzzy!\npuellās.");
    }

    #[test]
    fn only_unambiguous_macronizations_skip_review() {
        assert_eq!(
            Review::new("puellas".into())
                .unambiguous_preview()
                .as_deref(),
            Some("puellās")
        );
        // Multiple grammatical analyses can still have the same spelling.
        assert_eq!(
            Review::new("aquis".into()).unambiguous_preview().as_deref(),
            Some("aquīs")
        );
        // amīcās is spelled alike as amīca and amīcus, but they are two lemmas.
        assert_eq!(Review::new("amicas".into()).unambiguous_preview(), None);
        assert_eq!(Review::new("malum".into()).unambiguous_preview(), None);
        // Homographs spelled alike still need a lemma chosen: bellum is
        // both "war" and the neuter of bellus, "pretty".
        let mut bellum = Review::new("bellum".into());
        assert_eq!(bellum.unambiguous_preview(), None);
        bellum.prefer_lemmas(&[LemmaChoice {
            token: "bellum".into(),
            occurrence: 0,
            lemma_key: "bellum".into(),
            extension: false,
        }]);
        // A lemma chosen before is preselected, but still reviewed.
        assert_eq!(bellum.lemma_choices()[0].lemma_key, "bellum");
        assert_eq!(bellum.unambiguous_preview(), None);
        assert_eq!(
            Review::new("xyzzy".into()).unambiguous_preview().as_deref(),
            Some("xyzzy")
        );
    }

    #[test]
    fn choosing_another_reading_can_replace_existing_macrons() {
        let mut review = Review::new("mālum".into());
        let analyses = &review.words()[0].analyses;
        let short = analyses
            .iter()
            .position(|analysis| analysis.form.contains('ă'))
            .unwrap();
        let long = analyses
            .iter()
            .position(|analysis| analysis.form.contains('ā'))
            .unwrap();
        review.choose(0, Some(short));
        assert_eq!(review.preview(), "malum");
        review.choose(0, Some(long));
        assert_eq!(review.preview(), "mālum");
        review.choose(0, None);
        assert_eq!(review.preview(), "mālum");
    }
}
