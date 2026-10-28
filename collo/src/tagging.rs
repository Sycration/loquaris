//! Contextual disambiguation using the bundled LASLA tag statistics.
use crate::{Analysis, Lemmatizer, data};
use std::collections::{BTreeMap, HashMap};

/// A word, its candidate analyses, and the tag selected by the sequence model.
#[derive(Debug, Clone)]
pub struct TaggedWord {
    pub text: String,
    /// Byte range in the input text.
    pub range: std::ops::Range<usize>,
    pub analyses: Vec<Analysis>,
    pub tag: Option<String>,
    /// Index of the most frequent analysis compatible with `tag`.
    pub selected: Option<usize>,
}

fn count(table: &[(&str, u64)], key: &str) -> u64 {
    table
        .binary_search_by_key(&key, |(k, _)| *k)
        .ok()
        .map_or(0, |i| table[i].1)
}
/// Occurrence count of a three-character Collatinus tag.
pub fn tag_count(tag: &str) -> u64 {
    count(data::TAG_COUNTS, tag)
}
/// Occurrence count of a space-separated three-tag sequence.
pub fn trigram_count(first: &str, second: &str, third: &str) -> u64 {
    if first.is_empty() {
        return count(data::TRIGRAM_COUNTS, &format!("{second} {third}"));
    }
    count(data::TRIGRAM_COUNTS, &format!("{first} {second} {third}"))
}
fn parts(analysis: &Analysis) -> Vec<char> {
    let details = analysis.lemma.details;
    let mut parts = Vec::new();
    for (needle, part) in [
        ("adj.", 'a'),
        ("conj", 'c'),
        ("excl.", 'e'),
        ("interj", 'i'),
        ("num.", 'm'),
        ("pron.", 'p'),
        ("prép", 'r'),
        ("adv", 'd'),
        (" nom ", 'n'),
        ("npr.", 'n'),
    ] {
        if details.contains(needle) && !parts.contains(&part) {
            parts.push(part);
        }
    }
    if parts.is_empty() && analysis.lemma.part_of_speech.is_ascii_alphabetic() {
        parts.push(analysis.lemma.part_of_speech);
    }
    parts
}
/// The possible three-character Collatinus tags for an analysis.
pub fn analysis_tags(analysis: &Analysis) -> Vec<String> {
    let description = analysis.morphology("en").unwrap_or("");
    let cases = [
        "nominative",
        "vocative",
        "accusative",
        "genitive",
        "dative",
        "ablative",
    ];
    let modes = ["indicative", "subjunctive", "imperative", "infinitive"];
    parts(analysis)
        .into_iter()
        .map(|part| {
            if part == 'n' && analysis.morphology_id == 413 {
                return "n71".to_owned();
            }
            if part == 'v' && description.contains(" -u") {
                return "v3 ".to_owned();
            }
            if part == 'v'
                && let Some(index) = modes.iter().position(|m| description.contains(m))
            {
                return format!(
                    "v{}{}",
                    index + 1,
                    if description.contains("present") {
                        '1'
                    } else {
                        ' '
                    }
                );
            }
            if let Some(index) = cases.iter().position(|c| description.contains(c)) {
                return format!(
                    "{}{}{}",
                    if part == 'v' { 'w' } else { part },
                    index + 1,
                    if description.contains("plural") { 2 } else { 1 }
                );
            }
            format!("{part}  ")
        })
        .collect()
}
pub(crate) fn fraction(tag: &str) -> u64 {
    let occurrences = tag_count(tag);
    if occurrences == 0 {
        return 1024;
    }
    let part = tag.chars().next().unwrap_or(' ');
    let total: u64 = data::TAG_COUNTS
        .iter()
        .filter(|(key, _)| key.starts_with(part))
        .map(|(_, n)| *n)
        .sum();
    let factor = match part {
        'a' | 'p' | 'w' => 341,
        'v' if tag.ends_with('1') => 512,
        'v' if tag.ends_with(' ') => 256,
        'n' => 1024,
        _ => return 1024,
    };
    (occurrences * factor / total.max(1)).max(1)
}

pub(crate) fn analysis_weight(analysis: &Analysis) -> u64 {
    let fraction = analysis_tags(analysis)
        .iter()
        .map(|tag| fraction(tag))
        .max()
        .unwrap_or(1024);
    u64::from(analysis.lemma.frequency) * fraction * u64::from(analysis.multiplicity)
}

#[derive(Clone)]
struct Path {
    score: f64,
    previous: String,
    current: String,
    choices: Vec<Option<String>>,
}
fn has_cum_enclitic(word: &str) -> bool {
    matches!(
        word,
        "mecum"
            | "tecum"
            | "secum"
            | "nobiscum"
            | "vobiscum"
            | "quibuscum"
            | "quacum"
            | "quocum"
            | "quicum"
    )
}
fn tag_sentence(words: &mut [TaggedWord]) {
    let mut paths = vec![Path {
        score: 0.0,
        previous: String::new(),
        current: "snt".into(),
        choices: Vec::new(),
    }];
    for word in words.iter() {
        let with_cum = has_cum_enclitic(&word.text);
        let mut weights: BTreeMap<String, u64> = BTreeMap::new();
        for analysis in &word.analyses {
            for tag in analysis_tags(analysis) {
                *weights.entry(tag.clone()).or_default() += u64::from(analysis.lemma.frequency)
                    * fraction(&tag)
                    * u64::from(analysis.multiplicity);
            }
        }
        if data::ABBREVIATIONS.contains(&word.text.as_str()) {
            weights.clear();
            for case in 1..=6 {
                let tag = format!("n{case}1");
                weights.insert(tag.clone(), tag_count(&tag).max(1));
            }
        }
        if with_cum && (weights.is_empty() || (weights.len() == 1 && !weights.contains_key("p61")))
        {
            weights.clear();
            weights.insert("p61".to_owned(), 1024);
        }
        if crate::roman_numeral(&word.text).is_some() {
            let key = crate::normalize(&word.text);
            if !data::LEMMAS.iter().any(|lemma| lemma.key == key) {
                *weights.entry("m  ".to_owned()).or_default() += 1024;
            }
        }
        if weights.is_empty() {
            for path in &mut paths {
                path.choices.push(None);
            }
            continue;
        }
        let total = weights.values().sum::<u64>().max(1);
        for weight in weights.values_mut() {
            *weight = (*weight * 1024 / total).max(1);
        }
        let mut best: HashMap<(String, String), Path> = HashMap::new();
        for path in &paths {
            let total: f64 = weights
                .iter()
                .map(|(tag, weight)| {
                    *weight as f64
                        * (4 * trigram_count(&path.previous, &path.current, tag) + 1) as f64
                })
                .sum();
            for (tag, weight) in &weights {
                let probability = (*weight as f64
                    * (4 * trigram_count(&path.previous, &path.current, tag) + 1) as f64
                    / total)
                    .max(f64::MIN_POSITIVE);
                let mut candidate = path.clone();
                candidate.score += probability.ln();
                candidate.previous = path.current.clone();
                candidate.current = tag.clone();
                candidate.choices.push(Some(tag.clone()));
                let suffix = word
                    .analyses
                    .first()
                    .map(|a| a.suffix.as_str())
                    .unwrap_or("");
                let enclitic = if with_cum {
                    Some("re ")
                } else {
                    match suffix {
                        "que" | "ue" | "ve" | "quĕ" | "vĕ" => Some("ce "),
                        "ne" | "nĕ" => Some("de "),
                        "st" | "t" => Some("v11"),
                        _ => None,
                    }
                };
                if let Some(enclitic) = enclitic {
                    candidate.previous = candidate.current;
                    candidate.current = enclitic.into();
                }
                let key = (candidate.previous.clone(), candidate.current.clone());
                if best.get(&key).is_none_or(|old| {
                    candidate.score > old.score
                        || (candidate.score == old.score && candidate.choices < old.choices)
                }) {
                    best.insert(key, candidate);
                }
            }
        }
        paths = best.into_values().collect();
        paths.sort_by(|a, b| {
            (&a.previous, &a.current, &a.choices).cmp(&(&b.previous, &b.current, &b.choices))
        });
    }
    if let Some(best) = paths.into_iter().max_by(|a, b| {
        a.score
            .total_cmp(&b.score)
            .then_with(|| b.choices.cmp(&a.choices))
    }) {
        for (word, tag) in words.iter_mut().zip(best.choices) {
            word.selected = tag.as_ref().and_then(|tag| {
                word.analyses
                    .iter()
                    .enumerate()
                    .filter(|(_, a)| {
                        analysis_tags(a).contains(tag)
                            || (tag == "p61" && has_cum_enclitic(&word.text))
                    })
                    .max_by_key(|(_, a)| u64::from(a.lemma.frequency) * fraction(tag))
                    .map(|(i, _)| i)
            });
            word.tag = tag;
        }
    }
}
impl Lemmatizer {
    /// Analyze and contextually tag the text, resetting at sentence punctuation.
    /// Unknown words remain in the output with no selected tag.
    pub fn tag_text(&self, text: &str) -> Vec<TaggedWord> {
        let mut words = Vec::new();
        let mut start = None;
        let mut sentence_start = 0;
        for (offset, ch) in text
            .char_indices()
            .chain(std::iter::once((text.len(), '\0')))
        {
            if ch.is_alphabetic()
                || (start.is_some() && matches!(ch, '\u{0304}' | '\u{0306}' | '\u{0301}'))
            {
                if start.is_none() {
                    start = Some(offset);
                }
                continue;
            }
            if let Some(begin) = start.take() {
                let word = &text[begin..offset];
                let analyses = self.analyze(word, words.len() == sentence_start);
                words.push(TaggedWord {
                    text: word.to_owned(),
                    range: begin..offset,
                    analyses,
                    tag: None,
                    selected: None,
                });
            }
            if matches!(ch, '.' | '!' | '?' | ';')
                && !words
                    .last()
                    .is_some_and(|w| ch == '.' && data::ABBREVIATIONS.contains(&w.text.as_str()))
            {
                tag_sentence(&mut words[sentence_start..]);
                sentence_start = words.len();
            }
        }
        tag_sentence(&mut words[sentence_start..]);
        words
    }
}
