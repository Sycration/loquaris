//! Project-wide search for text or, for a single Latin word, its lemma.
use std::ops::Range;
use std::sync::Arc;

use collo::tagging::TaggedWord;
use unicode_normalization::{UnicodeNormalization, char::is_combining_mark};

use crate::language::morphology_language;
use crate::macronize::{matching_analyses, reader_form};
use crate::word_index::{Hit, LemmaInfo, SlotHit, WordIndex};

/// The search box's options. They belong to the session, not the query.
#[derive(Clone, Copy, Debug, Default)]
pub struct SearchOptions {
    /// Match the query as text even when it is a single Latin word.
    pub exact_text: bool,
    /// Read unmarked vowels as short when a reading with them short exists.
    pub unmarked_short: bool,
    /// Search every form of this lemma rather than the default mode. Picking
    /// a lemma turns exact text off, so this takes precedence over it; the
    /// interface drops the lemma when the query is edited.
    pub lemma: Option<&'static collo::Lemma>,
}

#[derive(Clone, Copy, Debug)]
pub enum SearchMode {
    /// Case- and diacritic-insensitive text matching in every entry.
    Text,
    /// Every Latin word with this lemma among its possible analyses.
    Lemma(&'static collo::Lemma),
}

impl PartialEq for SearchMode {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (SearchMode::Text, SearchMode::Text) => true,
            (SearchMode::Lemma(a), SearchMode::Lemma(b)) => std::ptr::eq(*a, *b),
            _ => false,
        }
    }
}

/// One reading of a query word.
#[derive(Clone, Debug)]
pub struct QueryReading {
    /// The form as a reader expects it.
    pub form: String,
    /// In French or Spanish when the L1 is, else English.
    pub morphology: Option<&'static str>,
    /// collo's choice from the context of the query.
    pub context_selected: bool,
}

/// A lemma of a query word and its readings with that lemma.
#[derive(Clone, Debug)]
pub struct QueryLemma {
    pub lemma: LemmaInfo,
    pub readings: Vec<QueryReading>,
}

/// collo's analysis of one word of the query.
#[derive(Clone, Debug)]
pub struct QueryWord {
    pub text: String,
    /// How many readings were excluded by its written vowel quantities (and,
    /// with unmarked-short, by its unmarked vowels).
    pub ruled_out: usize,
    /// The distinct lemmas of the remaining readings, in collo's order.
    /// Empty when the word is not recognised as Latin.
    pub lemmas: Vec<QueryLemma>,
}

/// One lesson's hits, by participant.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LessonCount {
    pub lesson_id: Arc<str>,
    pub teacher: usize,
    pub student: usize,
}

#[derive(Clone, Debug)]
pub struct SearchResult {
    pub mode: SearchMode,
    /// In script order.
    pub hits: Vec<Hit>,
    /// Every lesson in script order.
    pub per_lesson: Vec<LessonCount>,
    pub words: Vec<QueryWord>,
}

impl SearchResult {
    /// The number of lessons with at least one hit.
    pub fn lessons_with_hits(&self) -> usize {
        self.per_lesson
            .iter()
            .filter(|lesson| lesson.teacher + lesson.student > 0)
            .count()
    }
}

/// Tag the query with only the readings that fit its written quantities, so
/// the contextual selection is limited to them too. Also returns, per word,
/// how many readings were excluded.
pub fn analyze_query(query: &str, unmarked_short: bool) -> (Vec<TaggedWord>, Vec<usize>) {
    let analyzer = collo::Lemmatizer::new();
    let mut ruled_out = Vec::new();
    let words = analyzer.tag_text_with(query, |token, sentence_start| {
        let all = analyzer.analyze(token, sentence_start);
        let keep = matching_analyses(token, &all, unmarked_short);
        ruled_out.push(all.len() - keep.len());
        keep.into_iter().map(|index| all[index].clone()).collect()
    });
    (words, ruled_out)
}

/// A single word with analyses from collo is searched by lemma (the most
/// frequent of its readings); anything else as text.
pub fn default_mode(query: &str, analysis: &[TaggedWord]) -> SearchMode {
    let [word] = analysis else {
        return SearchMode::Text;
    };
    if word.text != query.trim() {
        return SearchMode::Text;
    }
    lemma_options(word)
        .into_iter()
        .max_by_key(|lemma| lemma.frequency)
        .map_or(SearchMode::Text, SearchMode::Lemma)
}

/// The distinct lemmas among a word's analyses, in collo's order.
pub fn lemma_options(word: &TaggedWord) -> Vec<&'static collo::Lemma> {
    let mut lemmas: Vec<&'static collo::Lemma> = Vec::new();
    for analysis in &word.analyses {
        if !lemmas
            .iter()
            .any(|lemma| std::ptr::eq(*lemma, analysis.lemma))
        {
            lemmas.push(analysis.lemma);
        }
    }
    lemmas
}

impl WordIndex {
    /// Search the project: hits, counts per lesson and the query's analysis.
    pub fn search(&self, query: &str, options: &SearchOptions) -> SearchResult {
        let (analysis, ruled_out) = analyze_query(query, options.unmarked_short);
        let mode = match options.lemma {
            Some(lemma) => SearchMode::Lemma(lemma),
            None if options.exact_text => SearchMode::Text,
            None => default_mode(query, &analysis),
        };
        let found = if query.trim().is_empty() {
            Vec::new()
        } else {
            match mode {
                SearchMode::Text => self.text_slot_hits(query),
                SearchMode::Lemma(lemma) => self.lemma_slot_hits(lemma.key, options.unmarked_short),
            }
        };
        let per_lesson = self
            .slot_counts(found.iter().map(|hit| hit.slot))
            .into_iter()
            .map(|(lesson_id, [teacher, student])| LessonCount {
                lesson_id,
                teacher,
                student,
            })
            .collect();
        let morphology = morphology_language(self.translation_language());
        let words = analysis
            .iter()
            .zip(ruled_out)
            .map(|(word, ruled_out)| QueryWord {
                text: word.text.clone(),
                ruled_out,
                lemmas: lemma_options(word)
                    .into_iter()
                    .map(|lemma| QueryLemma {
                        lemma: self.lemma_info(lemma),
                        readings: word
                            .analyses
                            .iter()
                            .enumerate()
                            .filter(|(_, analysis)| std::ptr::eq(analysis.lemma, lemma))
                            .map(|(index, analysis)| QueryReading {
                                form: reader_form(&format!("{}{}", analysis.form, analysis.suffix)),
                                morphology: analysis.morphology(morphology),
                                context_selected: word.selected == Some(index),
                            })
                            .collect(),
                    })
                    .collect(),
            })
            .collect();
        SearchResult {
            mode,
            hits: self.hits(&found),
            per_lesson,
            words,
        }
    }

    /// Case- and accent-insensitive, non-overlapping matches of the trimmed
    /// query in every placed entry, Latin or not, in script order.
    pub fn text_hits(&self, query: &str) -> Vec<Hit> {
        self.hits(&self.text_slot_hits(query))
    }

    fn text_slot_hits(&self, query: &str) -> Vec<SlotHit> {
        let mut needle = Folded::default();
        needle.fold(query.trim());
        let needle = needle.chars;
        if needle.is_empty() {
            return Vec::new();
        }
        // Folded ASCII is lowercase ASCII with the same offsets, so ASCII
        // entries are searched as bytes when the needle is ASCII too.
        let ascii_needle: Option<Vec<u8>> = needle
            .iter()
            .map(|&ch| u8::try_from(ch).ok().filter(u8::is_ascii))
            .collect();
        let mut hits = Vec::new();
        // One buffer for every entry, so folding doesn't allocate per entry.
        let mut haystack = Folded::default();
        for (slot, text) in self.script_texts() {
            let mut push = |range| hits.push(SlotHit { slot, range });
            if text.is_ascii() {
                if let Some(needle) = &ascii_needle {
                    find_all(text.as_bytes(), needle, u8::eq_ignore_ascii_case, |found| {
                        push(found)
                    });
                }
                continue;
            }
            haystack.fold(text);
            find_all(&haystack.chars, &needle, char::eq, |found| {
                push(haystack.ranges[found.start].start..haystack.ranges[found.end - 1].end)
            });
        }
        hits
    }
}

/// Report each non-overlapping match of `needle` in `haystack`, left to right.
fn find_all<T>(
    haystack: &[T],
    needle: &[T],
    equal: impl Fn(&T, &T) -> bool,
    mut found: impl FnMut(Range<usize>),
) {
    let mut start = 0;
    while start + needle.len() <= haystack.len() {
        let candidate = &haystack[start..start + needle.len()];
        if candidate.iter().zip(needle).all(|(a, b)| equal(a, b)) {
            found(start..start + needle.len());
            start += needle.len();
        } else {
            start += 1;
        }
    }
}

/// Text lowercased without diacritics, with each folded character's source
/// byte range.
#[derive(Default)]
struct Folded {
    chars: Vec<char>,
    ranges: Vec<Range<usize>>,
}

impl Folded {
    fn fold(&mut self, text: &str) {
        self.chars.clear();
        self.ranges.clear();
        for (start, ch) in text.char_indices() {
            let range = start..start + ch.len_utf8();
            if ch.is_ascii() {
                self.chars.push(ch.to_ascii_lowercase());
                self.ranges.push(range);
                continue;
            }
            for base in std::iter::once(ch).nfd().filter(|c| !is_combining_mark(*c)) {
                for lower in base.to_lowercase() {
                    self.chars.push(lower);
                    self.ranges.push(range.clone());
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::*;
    use crate::{Language, Participant, ProjectContent};

    fn index(project: ProjectContent) -> WordIndex {
        let mut index = WordIndex::new();
        index.set_project(project);
        index
    }

    fn lemma(key: &str) -> &'static collo::Lemma {
        collo::find_lemma(key, false).expect("a lemma")
    }

    fn text_of<'a>(index: &'a WordIndex, hit: &Hit) -> &'a str {
        &index.entry_text(&hit.entry_id).unwrap()[hit.range.clone()]
    }

    fn lemma_keys(word: &QueryWord) -> Vec<&'static str> {
        word.lemmas
            .iter()
            .map(|lemma| lemma.lemma.lemma.key)
            .collect()
    }

    #[test]
    fn searching_a_lemma_shows_its_reader_form_and_keeps_its_reading() {
        let index = WordIndex::new();
        for (key, shown) in [("regio", "regiō"), ("puella", "puella")] {
            let lemma = lemma(key);
            let headword = index.lemma_info(lemma).headword;
            assert_eq!(headword, shown);
            let options = SearchOptions {
                lemma: Some(lemma),
                ..Default::default()
            };
            let result = index.search(&headword, &options);
            assert_eq!(result.mode, SearchMode::Lemma(lemma));
            assert!(lemma_keys(&result.words[0]).contains(&key), "{key}");
        }
    }

    #[test]
    fn text_search_ignores_case_and_macrons_and_maps_byte_ranges() {
        let index = index(project(&[&[
            (
                Participant::Teacher,
                Language::Latin,
                "Puellā puellam videt.",
            ),
            (
                Participant::Student,
                Language::L1,
                "The girl sees the GIRL.",
            ),
        ]]));
        let hits = index.text_hits("puella");
        assert_eq!(hits.len(), 2);
        assert_eq!(text_of(&index, &hits[0]), "Puellā");
        assert_eq!(text_of(&index, &hits[1]), "puella");
        let hits = index.text_hits(" the girl ");
        assert_eq!(hits.len(), 2);
        assert_eq!(&*hits[1].entry_id, "0.1");
        assert_eq!(text_of(&index, &hits[1]), "the GIRL");
    }

    #[test]
    fn single_latin_word_is_searched_by_lemma_in_every_form() {
        let index = index(project(&[
            &[(Participant::Teacher, Language::Latin, "Puella cantat.")],
            &[
                (Participant::Student, Language::Latin, "Puellam video."),
                (Participant::Teacher, Language::L1, "puellam in English"),
            ],
        ]));
        let mut options = SearchOptions::default();
        let result = index.search("puellae", &options);
        let SearchMode::Lemma(lemma) = result.mode else {
            panic!("a known Latin word should be searched by lemma");
        };
        assert_eq!(lemma.key, "puella");
        assert_eq!(
            result
                .hits
                .iter()
                .map(|hit| &*hit.entry_id)
                .collect::<Vec<_>>(),
            ["0.0", "1.0"],
            "only Latin entries are lemmatized"
        );
        assert_eq!(
            result.per_lesson,
            [
                LessonCount {
                    lesson_id: "lesson 0".into(),
                    teacher: 1,
                    student: 0
                },
                LessonCount {
                    lesson_id: "lesson 1".into(),
                    teacher: 0,
                    student: 1
                }
            ]
        );
        assert_eq!(result.lessons_with_hits(), 2);

        options.exact_text = true;
        let result = index.search("puellae", &options);
        assert_eq!(result.mode, SearchMode::Text);
        assert!(result.hits.is_empty());
        assert_eq!(index.search("puella", &options).mode, SearchMode::Text);
        // Picking a lemma searches by it even though exact text was on.
        options.lemma = Some(lemma);
        assert_eq!(index.search("puella", &options).hits.len(), 2);
    }

    #[test]
    fn macrons_determine_which_lemma_a_word_is() {
        let index = index(teacher_lesson(&[("malae et mālae", Language::Latin)]));
        let check =
            |options: &mut SearchOptions, query: &str, key: &str, ruled_out, found: &[&str]| {
                options.lemma = Some(lemma(key));
                let result = index.search(query, options);
                assert_eq!(result.words.len(), 1);
                assert_eq!(result.words[0].ruled_out, ruled_out, "{query}");
                let found_text: Vec<_> =
                    result.hits.iter().map(|hit| text_of(&index, hit)).collect();
                assert_eq!(found_text, found, "{query} as {key}");
            };

        // By default only written marks count: mālae can't be mălus, but
        // unmarked malae may be either.
        let mut options = SearchOptions::default();
        assert_eq!(
            lemma_keys(&index.search("malae", &options).words[0]),
            ["mala", "malus"]
        );
        check(&mut options, "mālae", "mala", 4, &["malae", "mālae"]);
        check(&mut options, "malae", "malus", 0, &["malae"]);

        // Optionally, unmarked vowels are short when a parse with them short exists.
        let mut options = SearchOptions {
            unmarked_short: true,
            ..Default::default()
        };
        assert!(matches!(
            index.search("malae", &options).mode,
            SearchMode::Lemma(lemma) if lemma.key == "malus"
        ));
        check(&mut options, "malae", "malus", 4, &["malae"]);
        check(&mut options, "mālae", "mala", 4, &["mālae"]);

        // First-use labels follow the default rule.
        let mut first_uses: Vec<_> = index
            .first_uses("0.0")
            .into_iter()
            .map(|lemma| index.lemma_info(lemma).headword)
            .collect();
        first_uses.sort();
        assert_eq!(first_uses, ["et", "malus", "māla"]);
    }

    #[test]
    fn phrases_and_unknown_words_are_searched_as_text() {
        let index = WordIndex::new();
        let options = SearchOptions::default();
        let result = index.search("puella cantat", &options);
        assert_eq!(result.mode, SearchMode::Text);
        assert_eq!(result.words.len(), 2);
        let result = index.search("xyzzy", &options);
        assert_eq!(result.mode, SearchMode::Text);
        assert!(result.words[0].lemmas.is_empty(), "not recognised as Latin");
        let result = index.search("  ", &options);
        assert!(result.words.is_empty() && result.hits.is_empty());
    }

    #[test]
    fn results_follow_content_changes() {
        let mut index = index(teacher_lesson(&[("a cat", Language::L1)]));
        let options = SearchOptions {
            exact_text: true,
            ..Default::default()
        };
        assert_eq!(index.search("cat", &options).hits.len(), 1);
        index.set_entry(entry("0.0", Participant::Teacher, Language::L1, "a dog"));
        assert!(index.search("cat", &options).hits.is_empty());
    }

    #[test]
    fn query_analysis_lists_lemmas_with_readings_in_the_l1() {
        let mut index = WordIndex::new();
        let options = SearchOptions::default();
        let rosa_lemma = |result: &SearchResult| -> QueryLemma {
            let [word] = &result.words[..] else {
                panic!("one word");
            };
            word.lemmas
                .iter()
                .find(|lemma| lemma.lemma.lemma.key == "rosa")
                .expect("rosa among the lemmas")
                .clone()
        };
        let result = index.search("rosam", &options);
        let rosa = rosa_lemma(&result);
        assert_eq!(rosa.lemma.headword, "rosa");
        assert_eq!(rosa.lemma.translation, lemma("rosa").translation("en"));
        let [reading] = &rosa.readings[..] else {
            panic!("one reading");
        };
        assert_eq!(reading.form, "rosam");
        assert_eq!(reading.morphology, word_morphology("rosam", "en"));
        let selected = result.words[0]
            .lemmas
            .iter()
            .flat_map(|lemma| &lemma.readings)
            .filter(|reading| reading.context_selected)
            .count();
        assert_eq!(selected, 1, "one reading is collo's choice in context");

        index.set_l1_name("Français");
        let rosa = rosa_lemma(&index.search("rosam", &options));
        assert_eq!(rosa.lemma.translation, lemma("rosa").translation("fr"));
        assert_eq!(rosa.readings[0].morphology, word_morphology("rosam", "fr"));
        assert_ne!(
            word_morphology("rosam", "fr"),
            word_morphology("rosam", "en")
        );
    }

    fn word_morphology(word: &str, language: &str) -> Option<&'static str> {
        collo::analyze(word)
            .iter()
            .find(|analysis| analysis.lemma.key == "rosa")
            .and_then(|analysis| analysis.morphology(language))
    }
}
