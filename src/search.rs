//! Project-wide search for text or, for a single Latin word, its lemma.
use crate::{Lesson, Participant, word_index::WordIndex};
use collo::tagging::TaggedWord;
use std::{ops::Range, sync::Arc};
use unicode_normalization::{UnicodeNormalization, char::is_combining_mark};

/// One matched span in an entry's text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hit {
    pub lesson: usize,
    pub entry: usize,
    /// Byte range in the entry text.
    pub range: Range<usize>,
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

/// The search query, its parse, and cached results.
pub struct Search {
    pub query: String,
    mode: SearchMode,
    analysis: Vec<TaggedWord>,
    /// Per query word, how many analyses were excluded by its written vowel quantities.
    ruled_out: Vec<usize>,
    /// Read unmarked vowels as short when a reading with them short exists.
    unmarked_short: bool,
    /// Match the query as text even when it is a single Latin word.
    exact_text: bool,
    /// Results with the content generation they were computed for.
    results: Option<(u64, Arc<[Hit]>)>,
}

impl Default for Search {
    fn default() -> Self {
        Self {
            query: String::new(),
            mode: SearchMode::Text,
            analysis: Vec::new(),
            ruled_out: Vec::new(),
            unmarked_short: false,
            exact_text: false,
            results: None,
        }
    }
}

impl Search {
    /// An empty search keeping the user's options.
    pub fn with_options_of(other: &Search) -> Self {
        Self {
            unmarked_short: other.unmarked_short,
            exact_text: other.exact_text,
            ..Self::default()
        }
    }

    pub fn unmarked_short(&self) -> bool {
        self.unmarked_short
    }

    /// Choose whether unmarked vowels count as short, re-parsing the query.
    pub fn set_unmarked_short(&mut self, unmarked_short: bool) {
        if self.unmarked_short != unmarked_short {
            self.unmarked_short = unmarked_short;
            self.query_changed();
        }
    }

    pub fn exact_text(&self) -> bool {
        self.exact_text
    }

    /// Choose whether to match the query as text rather than by lemma.
    pub fn set_exact_text(&mut self, exact_text: bool) {
        if self.exact_text != exact_text {
            self.exact_text = exact_text;
            self.results = None;
        }
    }

    /// How the query is matched: as text if that option is set, else by the
    /// chosen lemma when the query is one Latin word.
    pub fn mode(&self) -> SearchMode {
        if self.exact_text {
            SearchMode::Text
        } else {
            self.mode
        }
    }

    /// Collo's analyses of each word of the query that fit its written
    /// vowel quantities.
    pub fn analysis(&self) -> &[TaggedWord] {
        &self.analysis
    }

    /// For each query word, how many analyses were excluded by its macrons or breves.
    pub fn ruled_out(&self) -> &[usize] {
        &self.ruled_out
    }

    pub fn is_empty(&self) -> bool {
        self.query.trim().is_empty()
    }

    /// Re-parse after `query` was edited and choose the default mode.
    pub fn query_changed(&mut self) {
        let analyzer = collo::Lemmatizer::new();
        let mut ruled_out = Vec::new();
        // Tag with only the readings that fit the written quantities, so the
        // contextual selection is limited to them too.
        self.analysis = analyzer.tag_text_with(&self.query, |token, sentence_start| {
            let all = analyzer.analyze(token, sentence_start);
            let keep = crate::macronize::matching_analyses(token, &all, self.unmarked_short);
            ruled_out.push(all.len() - keep.len());
            keep.into_iter().map(|index| all[index].clone()).collect()
        });
        self.ruled_out = ruled_out;
        self.mode = default_mode(&self.query, &self.analysis);
        self.results = None;
    }

    pub fn set_query(&mut self, query: String) {
        self.query = query;
        self.query_changed();
    }

    /// Search for a lemma, showing its headword as the query.
    pub fn search_lemma(&mut self, lemma: &'static collo::Lemma) {
        self.set_query(crate::macronize::reader_form(lemma.headword));
        self.choose_lemma(lemma);
    }

    /// Search the query's word by this lemma, turning off exact text.
    pub fn choose_lemma(&mut self, lemma: &'static collo::Lemma) {
        let mode = SearchMode::Lemma(lemma);
        if self.mode != mode || self.exact_text {
            self.mode = mode;
            self.exact_text = false;
            self.results = None;
        }
    }

    /// Hits in script order, recomputed when the query, mode, or content changed.
    pub fn hits(&mut self, lessons: &[Lesson], index: &WordIndex, generation: u64) -> Arc<[Hit]> {
        if self.results.as_ref().is_none_or(|(g, _)| *g != generation) {
            let hits = if self.is_empty() {
                Vec::new()
            } else {
                match self.mode() {
                    SearchMode::Text => text_hits(lessons, &self.query),
                    SearchMode::Lemma(lemma) => lemma_hits(index, lemma, self.unmarked_short),
                }
            };
            self.results = Some((generation, hits.into()));
        }
        self.results
            .as_ref()
            .expect("results were just computed")
            .1
            .clone()
    }
}

/// The hits in one entry, from hits sorted in script order.
pub fn entry_hits(hits: &[Hit], lesson: usize, entry: usize) -> &[Hit] {
    let key = (lesson, entry);
    let start = hits.partition_point(|hit| (hit.lesson, hit.entry) < key);
    let len = hits[start..].partition_point(|hit| (hit.lesson, hit.entry) == key);
    &hits[start..start + len]
}

/// A single word with analyses from Collo is searched by lemma; anything else as text.
fn default_mode(query: &str, analysis: &[TaggedWord]) -> SearchMode {
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

/// The distinct lemmas among a word's analyses, in Collo's order.
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

fn lemma_hits(index: &WordIndex, lemma: &collo::Lemma, unmarked_short: bool) -> Vec<Hit> {
    let mut hits: Vec<_> = index
        .lemma_occurrences(lemma.key, unmarked_short)
        .unwrap_or_default()
        .into_iter()
        .map(|occurrence| Hit {
            lesson: occurrence.lesson,
            entry: occurrence.entry,
            range: occurrence.range,
        })
        .collect();
    hits.sort_by_key(|hit| (hit.lesson, hit.entry, hit.range.start));
    hits
}

/// Lowercase without diacritics, with each folded character's source byte range.
fn fold(text: &str) -> (Vec<char>, Vec<Range<usize>>) {
    let mut chars = Vec::with_capacity(text.len());
    let mut ranges = Vec::with_capacity(text.len());
    for (start, ch) in text.char_indices() {
        let range = start..start + ch.len_utf8();
        for base in std::iter::once(ch).nfd().filter(|c| !is_combining_mark(*c)) {
            for lower in base.to_lowercase() {
                chars.push(lower);
                ranges.push(range.clone());
            }
        }
    }
    (chars, ranges)
}

fn text_hits(lessons: &[Lesson], query: &str) -> Vec<Hit> {
    let (needle, _) = fold(query.trim());
    if needle.is_empty() {
        return Vec::new();
    }
    let mut hits = Vec::new();
    for (lesson, item) in lessons.iter().enumerate() {
        for (entry, value) in item.entries.iter().enumerate() {
            let (haystack, ranges) = fold(&value.text);
            let mut start = 0;
            while start + needle.len() <= haystack.len() {
                if haystack[start..start + needle.len()] == needle[..] {
                    let end = start + needle.len() - 1;
                    hits.push(Hit {
                        lesson,
                        entry,
                        range: ranges[start].start..ranges[end].end,
                    });
                    start += needle.len();
                } else {
                    start += 1;
                }
            }
        }
    }
    hits
}

/// Hits per lesson, split into `[teacher, student]`.
pub fn counts_per_lesson(lessons: &[Lesson], hits: &[Hit]) -> Vec<[usize; 2]> {
    let mut counts = vec![[0; 2]; lessons.len()];
    for hit in hits {
        let Some(entry) = lessons
            .get(hit.lesson)
            .and_then(|lesson| lesson.entries.get(hit.entry))
        else {
            continue;
        };
        let slot = match entry.participant {
            Participant::Teacher => 0,
            Participant::Student => 1,
        };
        counts[hit.lesson][slot] += 1;
    }
    counts
}

/// The Collo translation language matching the project's L1 name.
pub fn translation_language(l1_name: &str) -> &'static str {
    match l1_name.trim().to_lowercase().as_str() {
        "french" | "français" | "francais" | "fr" => "fr",
        "german" | "deutsch" | "de" => "de",
        "spanish" | "español" | "espanol" | "es" => "es",
        "italian" | "italiano" | "it" => "it",
        "portuguese" | "português" | "portugues" | "pt" => "pt",
        "catalan" | "català" | "catala" | "ca" => "ca",
        "galician" | "galego" | "gl" => "gl",
        _ => "en",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::{Entry, Language};

    fn lessons(texts: &[&[(Participant, Language, &str)]]) -> Vec<Lesson> {
        texts
            .iter()
            .map(|entries| Lesson {
                name: String::new(),
                expanded: true,
                entries: entries
                    .iter()
                    .map(|&(participant, language, text)| Entry {
                        participant,
                        language,
                        text: text.into(),
                        audio: None,
                        trim: None,
                    })
                    .collect(),
            })
            .collect()
    }

    #[test]
    fn searching_a_lemma_shows_its_reader_form_and_keeps_its_reading() {
        for (key, shown) in [("regio", "regiō"), ("puella", "puella")] {
            let lemma = collo::find_lemma(key, false).unwrap();
            let mut search = Search::default();
            search.search_lemma(lemma);
            assert_eq!(search.query, shown);
            assert_eq!(search.mode(), SearchMode::Lemma(lemma));
            assert!(
                lemma_options(&search.analysis()[0])
                    .iter()
                    .any(|option| std::ptr::eq(*option, lemma)),
                "{key}"
            );
        }
    }

    #[test]
    fn text_search_ignores_case_and_macrons_and_maps_byte_ranges() {
        let lessons = lessons(&[&[
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
        ]]);
        let hits = text_hits(&lessons, "puella");
        let text = &lessons[0].entries[0].text;
        assert_eq!(hits.len(), 2);
        assert_eq!(&text[hits[0].range.clone()], "Puellā");
        assert_eq!(&text[hits[1].range.clone()], "puella");
        let hits = text_hits(&lessons, " the girl ");
        assert_eq!(hits.len(), 2);
        assert_eq!(
            &lessons[0].entries[1].text[hits[1].range.clone()],
            "the GIRL"
        );
    }

    #[test]
    fn single_latin_word_is_searched_by_lemma_in_every_form() {
        let lessons = lessons(&[
            &[(Participant::Teacher, Language::Latin, "Puella cantat.")],
            &[
                (Participant::Student, Language::Latin, "Puellam video."),
                (Participant::Teacher, Language::L1, "puellam in English"),
            ],
        ]);
        let index = WordIndex::from_project(&Default::default(), &lessons);
        let mut search = Search::default();
        search.set_query("puellae".into());
        let SearchMode::Lemma(lemma) = search.mode() else {
            panic!("a known Latin word should be searched by lemma");
        };
        assert_eq!(lemma.key, "puella");
        let hits = search.hits(&lessons, &index, 0).to_vec();
        assert_eq!(
            hits.iter()
                .map(|hit| (hit.lesson, hit.entry))
                .collect::<Vec<_>>(),
            [(0, 0), (1, 0)],
            "only Latin entries are lemmatized"
        );
        assert_eq!(counts_per_lesson(&lessons, &hits), [[1, 0], [0, 1]]);

        search.set_exact_text(true);
        assert_eq!(search.mode(), SearchMode::Text);
        assert!(search.hits(&lessons, &index, 0).is_empty());
        // The option sticks for new queries until a lemma is chosen.
        search.set_query("puella".into());
        assert_eq!(search.mode(), SearchMode::Text);
        assert!(Search::with_options_of(&search).exact_text());
        search.choose_lemma(lemma);
        assert!(!search.exact_text());
        assert_eq!(search.hits(&lessons, &index, 0).len(), 2);
    }

    #[test]
    fn macrons_determine_which_lemma_a_word_is() {
        let lessons = lessons(&[&[(Participant::Teacher, Language::Latin, "malae et mālae")]]);
        let index = WordIndex::from_project(&Default::default(), &lessons);
        let text = &lessons[0].entries[0].text;
        let mut search = Search::default();
        let check = |search: &mut Search, query: &str, key: &str, ruled_out, found: &[&str]| {
            search.set_query(query.into());
            search.choose_lemma(
                collo::lemmas()
                    .iter()
                    .find(|lemma| lemma.key == key && !lemma.extension)
                    .expect("a lemma"),
            );
            assert_eq!(search.ruled_out(), [ruled_out], "{query}");
            let hits = search.hits(&lessons, &index, 0);
            let found_text: Vec<_> = hits.iter().map(|hit| &text[hit.range.clone()]).collect();
            assert_eq!(found_text, found, "{query} as {key}");
        };

        // By default only written marks count: mālae can't be mălus, but
        // unmarked malae may be either.
        assert!(!search.unmarked_short());
        search.set_query("malae".into());
        let options: Vec<_> = lemma_options(&search.analysis()[0])
            .iter()
            .map(|lemma| lemma.key)
            .collect();
        assert_eq!(options, ["mala", "malus"]);
        check(&mut search, "mālae", "mala", 4, &["malae", "mālae"]);
        check(&mut search, "malae", "malus", 0, &["malae"]);

        // Optionally, unmarked vowels are short when a parse with them short exists.
        search.set_unmarked_short(true);
        assert!(matches!(
            search.mode(),
            SearchMode::Lemma(lemma) if lemma.key == "malus"
        ));
        check(&mut search, "malae", "malus", 4, &["malae"]);
        check(&mut search, "mālae", "mala", 4, &["mālae"]);
        assert!(Search::with_options_of(&search).unmarked_short());

        // First-use labels follow the default rule.
        let mut first_uses = index.first_use_lemmas(0, 0);
        first_uses.sort();
        assert_eq!(first_uses, ["et", "malus", "māla"]);
    }

    #[test]
    fn phrases_and_unknown_words_are_searched_as_text() {
        let mut search = Search::default();
        search.set_query("puella cantat".into());
        assert_eq!(search.mode(), SearchMode::Text);
        assert_eq!(search.analysis().len(), 2);
        search.set_query("xyzzy".into());
        assert_eq!(search.mode(), SearchMode::Text);
    }

    #[test]
    fn entry_hits_finds_one_entrys_matches() {
        let hit = |lesson, entry, start| Hit {
            lesson,
            entry,
            range: start..start + 1,
        };
        let hits = [hit(0, 0, 0), hit(0, 2, 1), hit(0, 2, 5), hit(1, 0, 0)];
        assert_eq!(entry_hits(&hits, 0, 2), &hits[1..3]);
        assert!(entry_hits(&hits, 0, 1).is_empty());
        assert_eq!(entry_hits(&hits, 1, 0), &hits[3..]);
        assert!(entry_hits(&hits, 2, 0).is_empty());
    }

    #[test]
    fn results_refresh_when_content_changes() {
        let mut lessons = lessons(&[&[(Participant::Teacher, Language::L1, "a cat")]]);
        let index = WordIndex::default();
        let mut search = Search::default();
        search.set_exact_text(true);
        search.set_query("cat".into());
        assert_eq!(search.hits(&lessons, &index, 1).len(), 1);
        lessons[0].entries[0].text = "a dog".into();
        assert_eq!(search.hits(&lessons, &index, 1).len(), 1, "cached");
        assert!(search.hits(&lessons, &index, 2).is_empty());
    }
}
