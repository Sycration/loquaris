//! Latin morphological analysis backed by the Collatinus lexicon.
//!
//! Data is compiled into the library at build time. Lookups preserve case by
//! default and ignore vowel quantity and the distinctions i/j and u/v.
//! Analyses are alternatives, not contextual disambiguation.
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
struct Stem {
    key: &'static str,
    lemma: usize,
    number: u8,
    text: &'static str,
}
struct Ending {
    key: &'static str,
    model: usize,
    stem: u8,
    morphology: u16,
    text: &'static str,
}
struct Irregular {
    key: &'static str,
    lemma: usize,
    morphology: u16,
    text: &'static str,
}
mod data {
    use super::{Ending, Irregular, Lemma, Stem};
    include!(concat!(env!("OUT_DIR"), "/data.rs"));
}

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
        let Some(id) = data::LEMMAS
            .iter()
            .position(|entry| std::ptr::eq(entry, lemma))
        else {
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
        for stem in data::STEMS.iter().filter(|stem| stem.lemma == id) {
            for ending in data::ENDINGS.iter().filter(|ending| {
                data::MODEL_NAMES[ending.model] == lemma.model && ending.stem == stem.number
            }) {
                if data::EXCLUSIONS
                    .binary_search(&(id, ending.morphology))
                    .is_err()
                {
                    forms.push(InflectedForm {
                        morphology_id: ending.morphology,
                        form: format!("{}{}", stem.text, ending.text),
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
        let input = original.to_lowercase();
        let v_count = input.matches('v').count();
        let ae = input
            .matches('æ')
            .count()
            .saturating_sub(usize::from(input.ends_with('æ')));
        let oe = input.matches('œ').count();
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
            let (stem, ending) = word.split_at(offset);
            let start = data::ENDINGS.partition_point(|x| x.key < ending);
            let candidates = &data::ENDINGS[start..];
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
                let start = data::STEMS.partition_point(|x| x.key < stem);
                for root in data::STEMS[start..].iter().take_while(|x| x.key == stem) {
                    let lemma = &data::LEMMAS[root.lemma];
                    if !self.include_extension && lemma.extension {
                        continue;
                    }
                    for ending in candidates.iter().take_while(|x| x.key == ending) {
                        if root.number != ending.stem
                            || data::MODEL_NAMES[ending.model] != lemma.model
                            || data::EXCLUSIONS
                                .binary_search(&(root.lemma, ending.morphology))
                                .is_ok()
                        {
                            continue;
                        }
                        let root_lower = root.text.to_lowercase();
                        let expected_v =
                            root_lower.matches('v').count() + ending.text.matches('v').count();
                        let capital_v = original.starts_with('V')
                            && root.key.starts_with('U')
                            && v_count == root_lower.matches('v').count() + 1;
                        if v_count > 0 && v_count != expected_v && !capital_v {
                            continue;
                        }
                        if ae > 0
                            && ae
                                != root_lower.matches("āe").count()
                                    + root_lower.matches("prăe").count()
                        {
                            continue;
                        }
                        if oe > 0 && oe != root_lower.matches("ōe").count() {
                            continue;
                        }
                        let form = if extra
                            && root
                                .text
                                .trim_end_matches('\u{0306}')
                                .ends_with(['i', 'ī', 'ĭ'])
                        {
                            let mut root_text = root.text.to_owned();
                            root_text.pop();
                            let ending_tail: String = ending.text.chars().skip(1).collect();
                            format!("{root_text}ī{ending_tail}")
                        } else {
                            format!("{}{}", root.text, ending.text)
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
/// All embedded dictionary entries, including the optional extension.
pub fn lemmas() -> &'static [Lemma] {
    data::LEMMAS
}
