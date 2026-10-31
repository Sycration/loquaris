//! Contextual disambiguation using the bundled LASLA tag statistics.
use crate::{Analysis, Lemmatizer, data};

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

/// A three-character Collatinus tag. Tags are always ASCII.
type Tag = [u8; 3];
const NO_TAG: Tag = [0; 3];

fn tag_text(tag: &Tag) -> String {
    tag.iter().map(|&b| char::from(b)).collect()
}
fn count(table: &[(&str, u64)], key: &str) -> u64 {
    table
        .binary_search_by_key(&key, |(k, _)| *k)
        .ok()
        .map_or(0, |i| table[i].1)
}
/// Size of each dimension of the dense n-gram table: the empty tag, the
/// vocabulary, and one index for tags absent from the statistics.
fn table_size() -> usize {
    data::TAG_VOCABULARY.len() + 2
}
/// Index of a tag in the n-gram table.
fn tag_index(tag: &[u8]) -> usize {
    data::TAG_VOCABULARY
        .binary_search_by(|t| t[..].cmp(tag))
        .map_or(table_size() - 1, |i| i + 1)
}
fn ngram(first: usize, second: usize, third: usize) -> u64 {
    let size = table_size();
    let i = ((first * size + second) * size + third) * 4;
    match data::TRIGRAM_TABLE.get(i..i + 4) {
        Some(&[a, b, c, d]) => u64::from(u32::from_le_bytes([a, b, c, d])),
        _ => 0,
    }
}
/// Occurrence count of a three-character Collatinus tag.
pub fn tag_count(tag: &str) -> u64 {
    count(data::TAG_COUNTS, tag)
}
/// Occurrence count of a space-separated three-tag sequence.
pub fn trigram_count(first: &str, second: &str, third: &str) -> u64 {
    let key = if first.is_empty() {
        format!("{second} {third}")
    } else {
        format!("{first} {second} {third}")
    };
    let key = key.as_bytes();
    let known = |tag: &[u8]| Some(tag_index(tag)).filter(|&i| i < table_size() - 1);
    let indices = match key.len() {
        7 if key[3] == b' ' => (Some(0), known(&key[0..3]), known(&key[4..7])),
        11 if key[3] == b' ' && key[7] == b' ' => {
            (known(&key[0..3]), known(&key[4..7]), known(&key[8..11]))
        }
        _ => return 0,
    };
    match indices {
        (Some(a), Some(b), Some(c)) => ngram(a, b, c),
        _ => 0,
    }
}
/// Up to ten tags of one analysis, in `analysis_tags` order.
struct Tags {
    tags: [Tag; 10],
    len: usize,
}
impl Tags {
    fn as_slice(&self) -> &[Tag] {
        &self.tags[..self.len]
    }
}
fn part_mask(lemma: &crate::Lemma) -> u16 {
    if let Some(index) = crate::lemma_index(lemma) {
        return data::LEMMA_PARTS[index];
    }
    let details = lemma.details;
    let mut mask = 0;
    for (bit, needles) in [
        &["adj."][..],
        &["conj"],
        &["excl."],
        &["interj"],
        &["num."],
        &["pron."],
        &["prép"],
        &["adv"],
        &[" nom ", "npr."],
    ]
    .iter()
    .enumerate()
    {
        if needles.iter().any(|n| details.contains(n)) {
            mask |= 1 << bit;
        }
    }
    mask
}
fn tags(analysis: &Analysis) -> Tags {
    const CODES: &[u8; 9] = b"aceimprdn";
    let mut parts = [0u8; 10];
    let mut len = 0;
    let mask = part_mask(analysis.lemma);
    for (bit, &code) in CODES.iter().enumerate() {
        if mask & (1 << bit) != 0 {
            parts[len] = code;
            len += 1;
        }
    }
    if len == 0 && analysis.lemma.part_of_speech.is_ascii_alphabetic() {
        parts[0] = analysis.lemma.part_of_speech as u8;
        len = 1;
    }
    let (verb, case, plural) = analysis
        .morphology_id
        .checked_sub(1)
        .and_then(|i| data::MORPHOLOGY_TAGS.get(usize::from(i)))
        .copied()
        .unwrap_or((NO_TAG, 0, false));
    let mut result = Tags {
        tags: [NO_TAG; 10],
        len,
    };
    for (slot, &part) in result.tags.iter_mut().zip(&parts[..len]) {
        *slot = if part == b'n' && analysis.morphology_id == 413 {
            *b"n71"
        } else if part == b'v' && verb != NO_TAG {
            verb
        } else if case != 0 {
            [
                if part == b'v' { b'w' } else { part },
                b'0' + case,
                if plural { b'2' } else { b'1' },
            ]
        } else {
            [part, b' ', b' ']
        };
    }
    result
}
/// The possible three-character Collatinus tags for an analysis.
pub fn analysis_tags(analysis: &Analysis) -> Vec<String> {
    tags(analysis).as_slice().iter().map(tag_text).collect()
}
fn tag_fraction(tag: &Tag) -> u64 {
    data::TAG_VOCABULARY
        .binary_search(tag)
        .map_or(1024, |i| data::TAG_FRACTIONS[i])
}

pub(crate) fn analysis_weight(analysis: &Analysis) -> u64 {
    let fraction = tags(analysis)
        .as_slice()
        .iter()
        .map(tag_fraction)
        .max()
        .unwrap_or(1024);
    u64::from(analysis.lemma.frequency) * fraction * u64::from(analysis.multiplicity)
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
/// A surviving Viterbi state. Histories are stored as back-pointers into a
/// shared arena; `rank` orders the histories of all live paths
/// lexicographically, which is all the tie-breaking rule needs.
#[derive(Clone, Copy)]
struct Path {
    score: f64,
    previous: Tag,
    current: Tag,
    previous_index: usize,
    current_index: usize,
    node: u32,
    rank: u32,
}
const ROOT: u32 = u32::MAX;
/// Multiplicative hasher for the small integer keys of the Viterbi state map.
#[derive(Default)]
struct StateHasher(u64);
impl std::hash::Hasher for StateHasher {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.write_u64(u64::from(b));
        }
    }
    fn write_u64(&mut self, n: u64) {
        self.0 = (self.0.rotate_left(5) ^ n).wrapping_mul(0x517c_c1b7_2722_0a95);
    }
}
/// A Viterbi state, the last two tags, packed into one integer.
fn state_key(previous: Tag, current: Tag) -> u64 {
    let [a, b, c] = previous;
    let [d, e, f] = current;
    u64::from_le_bytes([a, b, c, d, e, f, 0, 0])
}
/// Per-call scratch buffers reused across words and sentences.
#[derive(Default)]
struct Scratch {
    nodes: Vec<(u32, Option<Tag>)>,
    paths: Vec<Path>,
    next: Vec<(Path, u32, Tag)>,
    best: std::collections::HashMap<u64, usize, std::hash::BuildHasherDefault<StateHasher>>,
    weights: Vec<(Tag, u64)>,
    indices: Vec<usize>,
    factors: Vec<f64>,
}
fn add_weight(weights: &mut Vec<(Tag, u64)>, tag: Tag, weight: u64) {
    match weights.binary_search_by(|(t, _)| t.cmp(&tag)) {
        Ok(i) => weights[i].1 += weight,
        Err(i) => weights.insert(i, (tag, weight)),
    }
}
fn tag_sentence(words: &mut [TaggedWord], scratch: &mut Scratch) {
    let Scratch {
        nodes,
        paths,
        next,
        best,
        weights,
        indices,
        factors,
    } = scratch;
    nodes.clear();
    paths.clear();
    paths.push(Path {
        score: 0.0,
        previous: NO_TAG,
        current: *b"snt",
        previous_index: 0,
        current_index: tag_index(b"snt"),
        node: ROOT,
        rank: 0,
    });
    for word in words.iter() {
        let with_cum = has_cum_enclitic(&word.text);
        weights.clear();
        for analysis in &word.analyses {
            for tag in tags(analysis).as_slice() {
                add_weight(
                    weights,
                    *tag,
                    u64::from(analysis.lemma.frequency)
                        * tag_fraction(tag)
                        * u64::from(analysis.multiplicity),
                );
            }
        }
        if data::ABBREVIATIONS.contains(&word.text.as_str()) {
            weights.clear();
            for case in 1..=6u8 {
                let tag = [b'n', b'0' + case, b'1'];
                let count = tag_count(std::str::from_utf8(&tag).unwrap_or_default());
                weights.push((tag, count.max(1)));
            }
        }
        if with_cum
            && (weights.is_empty() || (weights.len() == 1 && weights[0].0 != *b"p61"))
        {
            weights.clear();
            weights.push((*b"p61", 1024));
        }
        if crate::roman_numeral(&word.text).is_some()
            && crate::lemma_index_by_key(&crate::normalize(&word.text)).is_none()
        {
            add_weight(weights, *b"m  ", 1024);
        }
        if weights.is_empty() {
            for path in paths.iter_mut() {
                nodes.push((path.node, None));
                path.node = (nodes.len() - 1) as u32;
            }
            continue;
        }
        let total = weights.iter().map(|(_, w)| *w).sum::<u64>().max(1);
        for (_, weight) in weights.iter_mut() {
            *weight = (*weight * 1024 / total).max(1);
        }
        indices.clear();
        indices.extend(weights.iter().map(|(tag, _)| tag_index(tag)));
        let enclitic = if with_cum {
            Some(*b"re ")
        } else {
            match word
                .analyses
                .first()
                .map(|a| a.suffix.as_str())
                .unwrap_or("")
            {
                "que" | "ue" | "ve" | "quĕ" | "vĕ" => Some(*b"ce "),
                "ne" | "nĕ" => Some(*b"de "),
                "st" | "t" => Some(*b"v11"),
                _ => None,
            }
        };
        let enclitic = enclitic.map(|tag| (tag, tag_index(&tag)));
        next.clear();
        best.clear();
        for path in paths.iter() {
            factors.clear();
            let mut total = 0.0;
            for (&(_, weight), &index) in weights.iter().zip(indices.iter()) {
                let factor = weight as f64
                    * (4 * ngram(path.previous_index, path.current_index, index) + 1) as f64;
                total += factor;
                factors.push(factor);
            }
            for ((&(tag, _), &index), &factor) in
                weights.iter().zip(indices.iter()).zip(factors.iter())
            {
                let probability = (factor / total).max(f64::MIN_POSITIVE);
                let mut candidate = Path {
                    score: path.score + probability.ln(),
                    previous: path.current,
                    current: tag,
                    previous_index: path.current_index,
                    current_index: index,
                    node: path.node,
                    rank: 0,
                };
                if let Some((enclitic, enclitic_index)) = enclitic {
                    candidate.previous = candidate.current;
                    candidate.previous_index = candidate.current_index;
                    candidate.current = enclitic;
                    candidate.current_index = enclitic_index;
                }
                let order = (path.rank, tag);
                match best.entry(state_key(candidate.previous, candidate.current)) {
                    std::collections::hash_map::Entry::Vacant(entry) => {
                        entry.insert(next.len());
                        next.push((candidate, order.0, order.1));
                    }
                    std::collections::hash_map::Entry::Occupied(entry) => {
                        let old = &mut next[*entry.get()];
                        if candidate.score > old.0.score
                            || (candidate.score == old.0.score && order < (old.1, old.2))
                        {
                            *old = (candidate, order.0, order.1);
                        }
                    }
                }
            }
        }
        // Histories of equal length compare by parent history, then tag.
        next.sort_unstable_by_key(|&(_, rank, tag)| (rank, tag));
        paths.clear();
        for (rank, &(mut path, _, tag)) in next.iter().enumerate() {
            nodes.push((path.node, Some(tag)));
            path.node = (nodes.len() - 1) as u32;
            path.rank = rank as u32;
            paths.push(path);
        }
    }
    if let Some(best) = paths
        .iter()
        .max_by(|a, b| a.score.total_cmp(&b.score).then(b.rank.cmp(&a.rank)))
    {
        let mut choices = vec![None; words.len()];
        let mut node = best.node;
        for slot in choices.iter_mut().rev() {
            let (parent, tag) = nodes[node as usize];
            *slot = tag;
            node = parent;
        }
        for (word, tag) in words.iter_mut().zip(choices) {
            word.selected = tag.and_then(|tag| {
                let cum = tag == *b"p61" && has_cum_enclitic(&word.text);
                let fraction = tag_fraction(&tag);
                word.analyses
                    .iter()
                    .enumerate()
                    .filter(|(_, a)| cum || tags(a).as_slice().contains(&tag))
                    .max_by_key(|(_, a)| u64::from(a.lemma.frequency) * fraction)
                    .map(|(i, _)| i)
            });
            word.tag = tag.as_ref().map(tag_text);
        }
    }
}
impl Lemmatizer {
    /// Analyze and contextually tag the text, resetting at sentence punctuation.
    /// Unknown words remain in the output with no selected tag.
    pub fn tag_text(&self, text: &str) -> Vec<TaggedWord> {
        // Analyses depend only on the word and its sentence position, so
        // repeated words are analyzed once per call.
        let mut cache = std::collections::HashMap::<(&str, bool), Vec<Analysis>>::new();
        self.tag_text_with(text, |word, sentence_start| match subrange(text, word) {
            Some(range) => cache
                .entry((&text[range], sentence_start))
                .or_insert_with(|| self.analyze(word, sentence_start))
                .clone(),
            None => self.analyze(word, sentence_start),
        })
    }

    /// Analyze and tag text using caller-provided lexical analyses. This lets
    /// applications reuse analyses across repeated words while keeping the
    /// contextual sequence model local to this text.
    pub fn tag_text_with(
        &self,
        text: &str,
        mut analyze: impl FnMut(&str, bool) -> Vec<Analysis>,
    ) -> Vec<TaggedWord> {
        let mut words = Vec::new();
        let mut scratch = Scratch::default();
        let mut sentence_start = 0;
        for token in tokenize(text) {
            if token.sentence_start && words.len() > sentence_start {
                tag_sentence(&mut words[sentence_start..], &mut scratch);
                sentence_start = words.len();
            }
            let token_text = &text[token.range.clone()];
            words.push(TaggedWord {
                analyses: analyze(token_text, token.sentence_start),
                text: token_text.to_owned(),
                range: token.range,
                tag: None,
                selected: None,
            });
        }
        tag_sentence(&mut words[sentence_start..], &mut scratch);
        words
    }
}

/// The byte range of `part` if it is a subslice of `text`.
fn subrange(text: &str, part: &str) -> Option<std::ops::Range<usize>> {
    let start = (part.as_ptr() as usize).checked_sub(text.as_ptr() as usize)?;
    let range = start..start.checked_add(part.len())?;
    (text.get(range.clone()) == Some(part)).then_some(range)
}

/// A tokenized word in source text. Ranges are UTF-8 byte offsets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextToken {
    pub range: std::ops::Range<usize>,
    pub sentence_start: bool,
}

/// Split text using the same Latin-word and sentence-boundary rules as
/// contextual tagging, without performing dictionary analysis.
pub fn tokenize(text: &str) -> Vec<TextToken> {
    let mut words = Vec::new();
    let mut start = None;
    let mut next_sentence_start = true;
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
            words.push(TextToken {
                range: begin..offset,
                sentence_start: next_sentence_start,
            });
            next_sentence_start = false;
        }
        if matches!(ch, '.' | '!' | '?' | ';')
            && !words.last().is_some_and(|word: &TextToken| {
                ch == '.'
                    && data::ABBREVIATIONS
                        .iter()
                        .any(|abbreviation| *abbreviation == &text[word.range.clone()])
            })
        {
            next_sentence_start = true;
        }
    }
    words
}
