//! Latin morphological analysis backed by the Collatinus lexicon.
//!
//! Data is compiled into the library at build time. Lookups preserve case by
//! default and ignore vowel quantity and the distinctions i/j and u/v.
//! Analyses are alternatives, not contextual disambiguation.
mod hash;
mod normalize;
pub mod scansion;
pub mod tagging;
pub use normalize::normalize;

/// A dictionary entry. `key` distinguishes homonyms with a trailing digit.
#[derive(Debug)]
pub struct Lemma {
    pub key: &'static str,
    pub headword: &'static str,
    pub model: &'static str,
    /// Original dictionary grammatical notes (in the source's notation).
    pub details: &'static str,
    pub frequency: u32,
    pub extension: bool,
    /// Collatinus part-of-speech code, or NUL if not specified by the model.
    pub part_of_speech: char,
    pub translations: &'static [(&'static str, &'static str)],
}
impl Lemma {
    /// Return the translation in the requested language, if available.
    pub fn translation(&self, language: &str) -> Option<&'static str> {
        self.translations
            .iter()
            .find(|(code, _)| *code == language)
            .map(|(_, text)| *text)
    }
}
/// One possible interpretation of a word form.
#[derive(Debug, Clone)]
pub struct Analysis {
    pub lemma: &'static Lemma,
    /// Collatinus morphology identifier, in the range 1–416.
    pub morphology_id: u16,
    /// Spelling reconstructed from the underlying stem and ending.
    pub form: String,
    /// Enclitic removed during lookup, if any.
    pub suffix: String,
    pub irregular: bool,
    /// Number of identical derivations in the source lexicon. Statistical
    /// weighting uses this even though identical analyses are returned once.
    pub multiplicity: u32,
}
impl Analysis {
    /// Morphological description in English, French, Spanish, or `k9`.
    pub fn morphology(&self, language: &str) -> Option<&'static str> {
        morphology(self.morphology_id, language)
    }
}
/// Look up a morphology description by its source identifier and language.
pub fn morphology(id: u16, language: &str) -> Option<&'static str> {
    let list = match language {
        "en" => data::MORPHOLOGY_EN,
        "fr" => data::MORPHOLOGY_FR,
        "es" => data::MORPHOLOGY_ES,
        "k9" => data::MORPHOLOGY_K9,
        _ => return None,
    };
    id.checked_sub(1)
        .and_then(|i| list.get(i as usize))
        .copied()
}
/// A string in `data::TEXT`, as a byte offset and length.
#[derive(Clone, Copy)]
struct Text(u32, u16);
impl Text {
    fn get(self) -> &'static str {
        let start = self.0 as usize;
        &data::TEXT[start..start + usize::from(self.1)]
    }
}
struct Stem {
    key: Text,
    lemma: usize,
    /// Index of the lemma's model in `MODEL_NAMES`.
    model: u16,
    number: u8,
    extension: bool,
    /// Counts of `v`, `āe` plus `prăe`, and `ōe` in the lowercase text.
    v: u8,
    ae: u8,
    oe: u8,
    text: Text,
}
struct Ending {
    key: Text,
    model: u16,
    stem: u8,
    morphology: u16,
    /// Count of `v` in the text.
    v: u8,
    text: Text,
}
struct Irregular {
    key: &'static str,
    lemma: usize,
    morphology: u16,
    text: &'static str,
}
/// The contiguous run of entries whose key is `key`, located through a
/// build-time hash index of each run's first entry.
fn lookup<'a, T>(index: &[u32], entries: &'a [T], key: &str, key_of: fn(&T) -> &str) -> &'a [T] {
    let mask = index.len() - 1;
    let mut slot = hash::slot(key, index.len());
    loop {
        let position = index[slot];
        if position == hash::EMPTY {
            return &[];
        }
        let run = &entries[position as usize..];
        if key_of(&run[0]) == key {
            return &run[..run.partition_point(|entry| key_of(entry) == key)];
        }
        slot = (slot + 1) & mask;
    }
}
mod data {
    use super::{Ending, Irregular, Lemma, Stem, Text};
    include!(concat!(env!("OUT_DIR"), "/data.rs"));
}

/// Stable fingerprint for cached lexical analyses. It changes when the
/// generated lexicon, lookup implementation, or normalization rules change.
pub const ANALYSIS_CACHE_FINGERPRINT: u64 = data::ANALYSIS_CACHE_FINGERPRINT;

/// Options for analysis. The extension lexicon is disabled by default.
#[derive(Debug, Clone, Copy, Default)]
pub struct Lemmatizer {
    pub include_extension: bool,
}
impl Lemmatizer {
    pub fn new() -> Self {
        Self::default()
    }
    /// Generate the attested forms of a dictionary entry, indexed by morphology.
    pub fn inflect(&self, lemma: &'static Lemma) -> Vec<InflectedForm> {
        let Some(id) = lemma_index(lemma) else {
            return Vec::new();
        };
        if lemma.extension && !self.include_extension {
            return Vec::new();
        }
        let mut forms = Vec::new();
        for irregular in data::IRREGULARS.iter().filter(|entry| entry.lemma == id) {
            forms.push(InflectedForm {
                morphology_id: irregular.morphology,
                form: irregular.text.to_owned(),
                irregular: true,
            });
        }
        let model = data::LEMMA_MODELS[id] as usize;
        let stems = &data::STEMS_BY_LEMMA
            [data::LEMMA_STEM_STARTS[id] as usize..data::LEMMA_STEM_STARTS[id + 1] as usize];
        let endings = &data::ENDINGS_BY_MODEL[data::MODEL_ENDING_STARTS[model] as usize
            ..data::MODEL_ENDING_STARTS[model + 1] as usize];
        for stem in stems.iter().map(|&i| &data::STEMS[i as usize]) {
            for ending in endings
                .iter()
                .map(|&i| &data::ENDINGS[i as usize])
                .filter(|ending| ending.stem == stem.number)
            {
                if data::EXCLUSIONS
                    .binary_search(&(id, ending.morphology))
                    .is_err()
                {
                    forms.push(InflectedForm {
                        morphology_id: ending.morphology,
                        form: format!("{}{}", stem.text.get(), ending.text.get()),
                        irregular: false,
                    });
                }
            }
        }
        forms.sort_by(|a, b| {
            (a.morphology_id, &a.form, a.irregular).cmp(&(b.morphology_id, &b.form, b.irregular))
        });
        forms.dedup_by(|a, b| {
            a.morphology_id == b.morphology_id && a.form == b.form && a.irregular == b.irregular
        });
        forms
    }
    /// Analyze a word or recognize an uppercase Roman numeral.
    pub fn analyze_token(&self, token: &str, sentence_start: bool) -> TokenAnalysis {
        TokenAnalysis {
            analyses: self.analyze(token, sentence_start),
            roman_numeral: roman_numeral(token),
        }
    }
    /// Analyze a single word. At sentence start, an initial capital may also
    /// represent a lowercase word. Unknown words return an empty vector.
    pub fn analyze(&self, word: &str, sentence_start: bool) -> Vec<Analysis> {
        self.analyze_inner(word, sentence_start, true)
    }
    fn analyze_inner(
        &self,
        word: &str,
        sentence_start: bool,
        capital_fallback: bool,
    ) -> Vec<Analysis> {
        if word.is_empty() {
            return Vec::new();
        }
        let mut result = self.variants(word, sentence_start);
        if result.is_empty() {
            for suffix in ["que", "ne", "ue", "ve", "st"] {
                if let Some(base) = word.strip_suffix(suffix).filter(|s| !s.is_empty()) {
                    let mut found = self.analyze_inner(base, sentence_start, false);
                    if found.is_empty() && suffix == "st" {
                        found = self.analyze_inner(&format!("{base}s"), sentence_start, false);
                        for a in &mut found {
                            a.suffix.push('t');
                        }
                    } else {
                        for a in &mut found {
                            a.suffix.push_str(match suffix {
                                "que" => "quĕ",
                                "ne" => "nĕ",
                                "ue" | "ve" => "vĕ",
                                other => other,
                            });
                        }
                    }
                    if !found.is_empty() {
                        result = found;
                        break;
                    }
                }
            }
        }
        if capital_fallback
            && result.is_empty()
            && word.chars().next().is_some_and(char::is_lowercase)
        {
            let mut chars = word.chars();
            let upper: String = chars.next().unwrap().to_uppercase().chain(chars).collect();
            result = self.analyze_inner(&upper, false, false);
        }
        result.sort_by(|a, b| {
            (
                a.lemma.key,
                a.morphology_id,
                &a.form,
                &a.suffix,
                a.irregular,
            )
                .cmp(&(
                    b.lemma.key,
                    b.morphology_id,
                    &b.form,
                    &b.suffix,
                    b.irregular,
                ))
        });
        result.dedup_by(|a, b| {
            let identical = std::ptr::eq(a.lemma, b.lemma)
                && a.morphology_id == b.morphology_id
                && a.form == b.form
                && a.suffix == b.suffix
                && a.irregular == b.irregular;
            if identical {
                b.multiplicity = b.multiplicity.saturating_add(a.multiplicity);
            }
            identical
        });
        result
    }
    fn variants(&self, word: &str, sentence_start: bool) -> Vec<Analysis> {
        let mut result = self.contracted_variants(word);
        let mut transformed = None;
        for &(source, target) in data::ASSIMILATIONS {
            if let Some(rest) = word.strip_prefix(source) {
                transformed = Some((format!("{target}{rest}"), true));
                break;
            }
        }
        if transformed.is_none() {
            for &(source, target) in data::ASSIMILATIONS {
                if let Some(rest) = word.strip_prefix(target) {
                    transformed = Some((format!("{source}{rest}"), false));
                    break;
                }
            }
        }
        if let Some((transformed, assimilated)) = transformed {
            let mut matches = self.contracted_variants(&transformed);
            for analysis in &mut matches {
                for &(source, target) in data::QUANTIFIED_ASSIMILATIONS {
                    let (from, to) = if assimilated {
                        (target, source)
                    } else {
                        (source, target)
                    };
                    if let Some(rest) = analysis.form.strip_prefix(from) {
                        analysis.form = format!("{to}{rest}");
                        break;
                    }
                }
            }
            replace_lemmas(&mut result, matches);
        }
        if sentence_start && word.chars().next().is_some_and(char::is_uppercase) {
            replace_lemmas(&mut result, self.analyze(&word.to_lowercase(), false));
        }
        result
    }
    fn contracted_variants(&self, word: &str) -> Vec<Analysis> {
        let mut result = Vec::new();
        self.direct(&normalize(word), word, &mut result);
        for &(short, long) in data::CONTRACTIONS {
            if let Some(stem) = word.strip_suffix(short) {
                let replacement = if stem.contains(['v', 'V']) {
                    long.to_owned()
                } else {
                    normalize(long)
                };
                let expanded = format!("{stem}{replacement}");
                let mut matches = Vec::new();
                self.direct(&normalize(&expanded), &expanded, &mut matches);
                let difference = long.chars().count().saturating_sub(short.chars().count());
                for analysis in &mut matches {
                    let mut characters: Vec<char> = analysis.form.chars().collect();
                    let mut position = word.chars().count() - short.chars().count() + 1;
                    if expanded.chars().count() != characters.len() {
                        position += characters
                            .iter()
                            .take(position + 2)
                            .filter(|&&c| c == '\u{0306}')
                            .count();
                    }
                    if position < characters.len() {
                        characters.drain(position..(position + difference).min(characters.len()));
                        analysis.form = characters.into_iter().collect();
                    }
                }
                replace_lemmas(&mut result, matches);
            }
        }
        result
    }
    fn direct(&self, word: &str, original: &str, result: &mut Vec<Analysis>) {
        // Counts in the lowercase input; none of these letters is affected by
        // context-sensitive lowercasing.
        let (mut v_count, mut ae, mut oe) = (0, 0, 0);
        for c in original.chars() {
            match c {
                'v' | 'V' => v_count += 1,
                'æ' | 'Æ' => ae += 1,
                'œ' | 'Œ' => oe += 1,
                _ => (),
            }
        }
        let ae = ae - usize::from(original.ends_with(['æ', 'Æ']));
        let start = data::IRREGULARS.partition_point(|x| x.key < word);
        for entry in data::IRREGULARS[start..]
            .iter()
            .take_while(|x| x.key == word)
        {
            let lemma = &data::LEMMAS[entry.lemma];
            if self.include_extension || !lemma.extension {
                result.push(Analysis {
                    lemma,
                    morphology_id: entry.morphology,
                    form: entry.text.to_owned(),
                    suffix: String::new(),
                    irregular: true,
                    multiplicity: 1,
                });
            }
        }
        for (offset, _) in word
            .char_indices()
            .chain(std::iter::once((word.len(), '\0')))
        {
            if word.len() - offset > data::LONGEST_ENDING {
                continue;
            }
            let (stem, ending) = word.split_at(offset);
            let candidates = lookup(data::ENDING_INDEX, data::ENDINGS, ending, |e| e.key.get());
            if candidates.is_empty() {
                continue;
            }
            for extra in [false, true] {
                if extra
                    && (!ending.starts_with('i') || ending.starts_with("ii") || stem.ends_with('i'))
                {
                    continue;
                }
                let alternate;
                let stem = if extra {
                    alternate = format!("{stem}i");
                    alternate.as_str()
                } else {
                    stem
                };
                for root in lookup(data::STEM_INDEX, data::STEMS, stem, |s| s.key.get()) {
                    if !self.include_extension && root.extension {
                        continue;
                    }
                    // Entries sharing a key are sorted by model, then stem number.
                    let group = (root.model, root.number);
                    let first = candidates.partition_point(|e| (e.model, e.stem) < group);
                    let matching = &candidates[first..];
                    let matching =
                        &matching[..matching.partition_point(|e| (e.model, e.stem) == group)];
                    for ending in matching {
                        if data::EXCLUSIONS
                                .binary_search(&(root.lemma, ending.morphology))
                                .is_ok()
                        {
                            continue;
                        }
                        let root_v = usize::from(root.v);
                        let expected_v = root_v + usize::from(ending.v);
                        let capital_v = original.starts_with('V')
                            && root.key.get().starts_with('U')
                            && v_count == root_v + 1;
                        if v_count > 0 && v_count != expected_v && !capital_v {
                            continue;
                        }
                        if ae > 0 && ae != usize::from(root.ae) {
                            continue;
                        }
                        if oe > 0 && oe != usize::from(root.oe) {
                            continue;
                        }
                        let lemma = &data::LEMMAS[root.lemma];
                        let form = if extra
                            && root
                                .text.get()
                                .trim_end_matches('\u{0306}')
                                .ends_with(['i', 'ī', 'ĭ'])
                        {
                            let mut root_text = root.text.get().to_owned();
                            root_text.pop();
                            let ending_tail: String = ending.text.get().chars().skip(1).collect();
                            format!("{root_text}ī{ending_tail}")
                        } else {
                            format!("{}{}", root.text.get(), ending.text.get())
                        };
                        result.push(Analysis {
                            lemma,
                            morphology_id: ending.morphology,
                            form,
                            suffix: String::new(),
                            irregular: false,
                            multiplicity: 1,
                        });
                    }
                }
            }
        }
    }
}
fn replace_lemmas(result: &mut Vec<Analysis>, replacement: Vec<Analysis>) {
    if replacement.is_empty() {
        return;
    }
    result.retain(|analysis| {
        !replacement
            .iter()
            .any(|item| std::ptr::eq(analysis.lemma, item.lemma))
    });
    result.extend(replacement);
}
/// One generated form of a lemma.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InflectedForm {
    pub morphology_id: u16,
    pub form: String,
    pub irregular: bool,
}
/// Word analyses and, where applicable, a Roman numeral interpretation.
#[derive(Debug, Clone)]
pub struct TokenAnalysis {
    pub analyses: Vec<Analysis>,
    pub roman_numeral: Option<u64>,
}
/// Interpret an uppercase Roman numeral using the original analyzer's accepted
/// spelling rules. Returns `None` for non-numerals or arithmetic overflow.
pub fn roman_numeral(text: &str) -> Option<u64> {
    if text.is_empty() || text.contains("IL") || text.contains("IVI") || text.contains("IUI") {
        return None;
    }
    let values: Option<Vec<i64>> = text
        .chars()
        .map(|c| match c {
            'I' => Some(1),
            'V' | 'U' => Some(5),
            'X' => Some(10),
            'L' => Some(50),
            'C' => Some(100),
            'D' => Some(500),
            'M' => Some(1000),
            _ => None,
        })
        .collect();
    let values = values?;
    let mut total = 0i64;
    for (index, &value) in values.iter().enumerate() {
        total = total.checked_add(if values.get(index + 1).is_some_and(|next| value < *next) {
            -value
        } else {
            value
        })?;
    }
    u64::try_from(total).ok()
}
/// Analyze with the default options outside a sentence start.
pub fn analyze(word: &str) -> Vec<Analysis> {
    Lemmatizer::new().analyze(word, false)
}
/// Index of an embedded lemma, or `None` for a lemma constructed elsewhere.
pub(crate) fn lemma_index(lemma: &Lemma) -> Option<usize> {
    let size = std::mem::size_of::<Lemma>();
    let offset = (lemma as *const Lemma as usize).checked_sub(data::LEMMAS.as_ptr() as usize)?;
    let index = offset / size;
    (offset % size == 0 && index < data::LEMMAS.len()).then_some(index)
}
/// Index of the first lemma (base or extension) whose key is exactly `key`.
pub(crate) fn lemma_index_by_key(key: &str) -> Option<usize> {
    let start = data::LEMMAS_BY_KEY.partition_point(|&i| data::LEMMAS[i as usize].key < key);
    data::LEMMAS_BY_KEY
        .get(start)
        .map(|&i| i as usize)
        .filter(|&i| data::LEMMAS[i].key == key)
}
/// Look up an embedded dictionary entry by its exact key, such as `"amo"`
/// or a homonym like `"uolo2"`. This is a binary search, unlike scanning
/// [`lemmas`].
pub fn find_lemma(key: &str, extension: bool) -> Option<&'static Lemma> {
    let start = data::LEMMAS_BY_KEY.partition_point(|&i| data::LEMMAS[i as usize].key < key);
    data::LEMMAS_BY_KEY[start..]
        .iter()
        .map(|&i| &data::LEMMAS[i as usize])
        .take_while(|lemma| lemma.key == key)
        .find(|lemma| lemma.extension == extension)
}
/// All embedded dictionary entries, including the optional extension.
pub fn lemmas() -> &'static [Lemma] {
    data::LEMMAS
}
