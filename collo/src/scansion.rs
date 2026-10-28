//! Syllabification and metrical analysis of Latin verse.
use crate::Lemmatizer;

/// Metrical quantity of a syllable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quantity {
    Long,
    Short,
    Unknown,
}
/// How to place the accent when the penultimate syllable has common quantity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AmbiguousAccent {
    /// Leave the accent unspecified.
    #[default]
    Undetermined,
    /// Treat the penult as long.
    Penult,
    /// Treat the penult as short.
    Antepenult,
}
/// Options for Latin word accentuation during scansion.
#[derive(Debug, Clone, Copy, Default)]
pub struct ScansionOptions {
    pub ambiguous_accent: AmbiguousAccent,
    /// Accent the penult of words ending in a common `-ius`.
    pub illius_exception: bool,
}
/// A syllable, its current quantity, and whether it is elided in context.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Syllable {
    pub text: String,
    pub quantity: Quantity,
    pub elided: bool,
}
/// The selected quantified spelling and its syllables.
#[derive(Debug, Clone)]
pub struct ScannedWord {
    pub text: String,
    pub form: String,
    pub recognized: bool,
    pub syllables: Vec<Syllable>,
    /// Alternative syllabifications from the matching dictionary analyses.
    pub alternatives: Vec<Vec<Syllable>>,
    /// Quantified spellings corresponding to `alternatives`.
    pub alternative_forms: Vec<String>,
    /// True when alternatives disagree on the number of syllables.
    pub ambiguous_syllable_count: bool,
    /// Accent index into `accent_syllables`, if determinable.
    pub accent: Option<usize>,
    /// The best-ranked spelling in the chosen accentuation mode.
    pub accent_form: String,
    /// Syllables of `accent_form`; these may differ from the scansion reading.
    pub accent_syllables: Vec<Syllable>,
}
/// A possible dactylic meter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Meter {
    Hexameter,
    Pentameter,
}
/// One line of verse, with every compatible supported meter.
#[derive(Debug, Clone)]
pub struct ScannedLine {
    pub words: Vec<ScannedWord>,
    pub meters: Vec<Meter>,
}
fn vowel(c: char) -> bool {
    "aāăeēĕiīĭoōŏuūŭyȳўæœAEIOUYĀĂĒĔĪĬŌŎŪŬȲЎÆŒ".contains(c)
}
fn marked_vowel(c: char) -> bool {
    "āăēĕīĭōŏūŭȳўĀĂĒĔĪĬŌŎŪŬȲЎ".contains(c)
}
fn can_elide(form: &str) -> bool {
    let chars: Vec<_> = form.chars().collect();
    if chars.len() > 1
        && (form.ends_with('m') || form.ends_with("āe") || form.ends_with('\u{0306}'))
        && marked_vowel(chars[chars.len() - 2])
    {
        return true;
    }
    form != "ō" && chars.last().is_some_and(|c| marked_vowel(*c))
}
fn quantity(c: char) -> Quantity {
    if "āēīōūȳĀĒĪŌŪȲæœÆŒ".contains(c) {
        Quantity::Long
    } else if "ăĕĭŏŭўĂĔĬŎŬЎ".contains(c) {
        Quantity::Short
    } else {
        Quantity::Unknown
    }
}
fn consonantal(chars: &[char], i: usize) -> bool {
    let c = chars[i].to_ascii_lowercase();
    if c == 'u' && i > 0 && matches!(chars[i - 1], 'q' | 'Q' | 'g' | 'G') {
        return true;
    }
    (c == 'i' || c == 'u')
        && (i == 0 || vowel(chars[i - 1]))
        && chars.get(i + 1).is_some_and(|c| vowel(*c))
}
fn lengthen_before_consonant(form: &mut String) {
    let mut chars: Vec<_> = form.chars().collect();
    if chars.len() < 2 || !"bcdfgjklmnpqrstvxzBCDFGJKLMNPQRSTVXZ".contains(*chars.last().unwrap()) {
        return;
    }
    let index = chars.len() - 2;
    if index > 0
        && matches!(
            (chars[index - 1], chars[index]),
            ('ā', 'e') | ('ā', 'u') | ('ē', 'u') | ('ō', 'e')
        )
    {
        return;
    }
    chars[index] = match chars[index] {
        'a' | 'ă' => 'ā',
        'e' | 'ĕ' => 'ē',
        'i' | 'ĭ' => 'ī',
        'o' | 'ŏ' => 'ō',
        'u' | 'ŭ' => 'ū',
        'y' | 'ў' => 'ȳ',
        'A' | 'Ă' => 'Ā',
        'E' | 'Ĕ' => 'Ē',
        'I' | 'Ĭ' => 'Ī',
        'O' | 'Ŏ' => 'Ō',
        'U' | 'Ŭ' => 'Ū',
        'Y' | 'Ў' => 'Ȳ',
        c => c,
    };
    *form = chars.into_iter().collect();
}
fn attach_suffix(stem: &str, suffix: &str) -> String {
    let mut form = stem.to_owned();
    if !suffix.is_empty() {
        lengthen_before_consonant(&mut form);
        if suffix == "st" {
            form.push('s');
            lengthen_before_consonant(&mut form);
            form.push('t');
        } else {
            form.push_str(&suffix.replace("ĕst", "ēst"));
        }
    }
    form
}
fn accent_index(
    syllables: &[Syllable],
    enclitic: bool,
    word: &str,
    options: ScansionOptions,
) -> Option<usize> {
    let count = syllables.len();
    if count < 2 {
        None
    } else if enclitic {
        Some(count - 2)
    } else if count == 2 {
        Some(0)
    } else {
        match syllables[count - 2].quantity {
            Quantity::Long => Some(count - 2),
            Quantity::Short => Some(count - 3),
            Quantity::Unknown
                if options.illius_exception
                    && crate::normalize(word).to_lowercase().ends_with("ius") =>
            {
                Some(count - 2)
            }
            Quantity::Unknown => match options.ambiguous_accent {
                AmbiguousAccent::Undetermined => None,
                AmbiguousAccent::Penult => Some(count - 2),
                AmbiguousAccent::Antepenult => Some(count - 3),
            },
        }
    }
}
fn plain_spelling(form: &str) -> String {
    form.chars()
        .map(|c| match c {
            'j' | 'J' | 'v' | 'V' => c.to_string(),
            _ => crate::normalize(&c.to_string()),
        })
        .collect()
}
fn syllabify(form: &str, lexical: bool) -> Vec<Syllable> {
    if !lexical && crate::roman_numeral(form).is_some() {
        let mut syllables = Vec::new();
        let mut text = String::new();
        for c in form.chars() {
            text.push(c);
            if matches!(c, 'I' | 'U') {
                syllables.push(Syllable {
                    text: std::mem::take(&mut text),
                    quantity: Quantity::Unknown,
                    elided: false,
                });
            }
        }
        if let Some(last) = syllables.last_mut() {
            last.text.push_str(&text);
        }
        return syllables;
    }
    let chars: Vec<_> = form.chars().collect();
    let mut nuclei = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if vowel(chars[i])
            && !consonantal(&chars, i)
            && (!lexical || quantity(chars[i]) != Quantity::Unknown)
        {
            let start = i;
            let mut end = i + 1;
            let mut q = quantity(chars[i]);
            if chars.get(end) == Some(&'\u{0306}') {
                q = Quantity::Unknown;
                end += 1;
            }
            if end < chars.len() && vowel(chars[end]) {
                let pair = chars[start..=end].iter().collect::<String>().to_lowercase();
                if ["āe", "ōe", "āu", "ēu"].contains(&pair.as_str())
                    || (!lexical && ["ae", "oe", "au", "eu"].contains(&pair.as_str()))
                {
                    q = Quantity::Long;
                    end += 1;
                }
            }
            nuclei.push((start, end, q));
            i = end;
        } else {
            i += 1;
        }
    }
    if nuclei.is_empty() {
        return Vec::new();
    }
    let mut result = Vec::new();
    let mut start = 0;
    for index in 0..nuclei.len() {
        let (_, end, mut q) = nuclei[index];
        let boundary = if index + 1 < nuclei.len() {
            let next = nuclei[index + 1].0;
            let consonants: Vec<_> = chars[end..next]
                .iter()
                .copied()
                .filter(|c| !matches!(c, 'h' | 'H' | '\u{0306}'))
                .collect();
            let count: usize = consonants
                .iter()
                .map(|c| {
                    if matches!(c, 'x' | 'z' | 'X' | 'Z') {
                        2
                    } else {
                        1
                    }
                })
                .sum::<usize>()
                .saturating_sub(usize::from(
                    consonants.len() >= 2
                        && matches!(consonants[consonants.len() - 2], 'q' | 'Q')
                        && matches!(consonants[consonants.len() - 1], 'u' | 'U'),
                ));
            if count >= 2 && !lexical {
                let mute_liquid = consonants.len() == 2
                    && "bcdfgptBCDFGPT".contains(consonants[0])
                    && "lrLR".contains(consonants[1]);
                q = if mute_liquid && q != Quantity::Long {
                    Quantity::Unknown
                } else {
                    Quantity::Long
                };
            }
            if next >= 2
                && matches!(chars[next - 2], 'q' | 'Q')
                && matches!(chars[next - 1], 'u' | 'U')
            {
                next - 2
            } else if next == end {
                next
            } else {
                next.saturating_sub(1).max(end)
            }
        } else {
            chars.len()
        };
        result.push(Syllable {
            text: chars[start..boundary].iter().collect(),
            quantity: q,
            elided: false,
        });
        start = boundary;
    }
    result
}
fn fits(actual: Quantity, expected: Quantity) -> bool {
    actual == Quantity::Unknown || expected == Quantity::Unknown || actual == expected
}
fn matches_meter(words: &[ScannedWord], meter: Meter) -> bool {
    if words.is_empty() {
        return false;
    }
    let variable_feet = if meter == Meter::Hexameter { 5 } else { 2 };
    for choices in 0..(1 << variable_feet) {
        let mut pattern = Vec::new();
        for foot in 0..variable_feet {
            pattern.push(Quantity::Long);
            if choices & (1 << foot) == 0 {
                pattern.push(Quantity::Long);
            } else {
                pattern.extend([Quantity::Short, Quantity::Short]);
            }
        }
        if meter == Meter::Hexameter {
            pattern.extend([Quantity::Long, Quantity::Unknown]);
        } else {
            pattern.extend([
                Quantity::Long,
                Quantity::Long,
                Quantity::Short,
                Quantity::Short,
                Quantity::Long,
                Quantity::Short,
                Quantity::Short,
                Quantity::Unknown,
            ]);
        }
        let mut positions = vec![0];
        for word in words {
            let mut next = Vec::new();
            for &position in &positions {
                for alternative in &word.alternatives {
                    let length = alternative.iter().filter(|s| !s.elided).count();
                    if position + length <= pattern.len()
                        && alternative
                            .iter()
                            .filter(|s| !s.elided)
                            .zip(&pattern[position..])
                            .all(|(syllable, &expected)| fits(syllable.quantity, expected))
                    {
                        next.push(position + length);
                    }
                }
            }
            next.sort_unstable();
            next.dedup();
            positions = next;
            if positions.is_empty() {
                break;
            }
        }
        if positions.contains(&pattern.len()) {
            return true;
        }
    }
    false
}
impl Lemmatizer {
    /// Scan one word. The displayed spelling is frequency-weighted; alternatives
    /// are retained and disagreements are reflected in the selected syllables.
    /// Unknown words receive only position-based quantities.
    pub fn scan_word(&self, word: &str, sentence_start: bool) -> ScannedWord {
        self.scan_word_with_options(word, sentence_start, ScansionOptions::default())
    }
    /// Scan a word with explicit accentuation options.
    pub fn scan_word_with_options(
        &self,
        word: &str,
        sentence_start: bool,
        options: ScansionOptions,
    ) -> ScannedWord {
        let analyses = self.analyze(word, sentence_start);
        let mut forms = std::collections::BTreeMap::<String, u64>::new();
        for analysis in &analyses {
            *forms
                .entry(attach_suffix(&analysis.form, &analysis.suffix))
                .or_default() += crate::tagging::analysis_weight(analysis);
        }
        let best = forms
            .iter()
            .max_by(|(a, aw), (b, bw)| aw.cmp(bw).then(a.cmp(b)));
        let mut form = best
            .map(|(form, _)| form.clone())
            .unwrap_or_else(|| word.to_owned());
        if word.chars().next().is_some_and(char::is_uppercase)
            && let Some(first) = form.chars().next()
        {
            form.replace_range(
                ..first.len_utf8(),
                &first.to_uppercase().collect::<String>(),
            );
        }
        let mut alternative_forms: Vec<_> = forms.keys().cloned().collect();
        let mut alternatives: Vec<_> = alternative_forms
            .iter()
            .map(|form| syllabify(form, true))
            .collect();
        let mut syllables = syllabify(&form, best.is_some());
        if crate::roman_numeral(word).is_some() && best.is_some() {
            alternative_forms.push(word.to_owned());
            alternatives.push(syllabify(word, false));
        }
        if alternatives.is_empty() {
            alternative_forms.push(form.clone());
            alternatives.push(syllables.clone());
        }
        let ambiguous_syllable_count = alternatives
            .iter()
            .any(|candidate| candidate.len() != syllables.len());
        if !ambiguous_syllable_count {
            for (i, syllable) in syllables.iter_mut().enumerate() {
                if alternatives
                    .iter()
                    .any(|candidate| candidate[i].quantity != syllable.quantity)
                {
                    syllable.quantity = Quantity::Unknown;
                }
            }
        }
        let mut accent_weights = std::collections::BTreeMap::<
            (String, Option<usize>),
            (u64, String, Vec<Syllable>),
        >::new();
        for (candidate, weight) in &forms {
            let enclitic = analyses.iter().any(|analysis| {
                !analysis.suffix.is_empty()
                    && analysis.suffix != "st"
                    && attach_suffix(&analysis.form, &analysis.suffix) == *candidate
            });
            let candidate_syllables = syllabify(candidate, true);
            let accent = accent_index(&candidate_syllables, enclitic, word, options);
            let entry = accent_weights
                .entry((plain_spelling(candidate), accent))
                .or_insert_with(|| (0, candidate.clone(), candidate_syllables));
            entry.0 += weight;
        }
        let chosen_accent =
            accent_weights
                .into_iter()
                .max_by(|(left, left_value), (right, right_value)| {
                    left_value.0.cmp(&right_value.0).then(left.cmp(right))
                });
        let (accent, mut accent_form, mut accent_syllables) = match chosen_accent {
            Some(((_, accent), (_, form, syllables))) => (accent, form, syllables),
            None => (None, form.clone(), syllables.clone()),
        };
        if word.chars().next().is_some_and(char::is_uppercase) {
            if let Some(first) = accent_form.chars().next() {
                accent_form.replace_range(
                    ..first.len_utf8(),
                    &first.to_uppercase().collect::<String>(),
                );
            }
            if let Some(first_syllable) = accent_syllables.first_mut()
                && let Some(first) = first_syllable.text.chars().next()
            {
                first_syllable.text.replace_range(
                    ..first.len_utf8(),
                    &first.to_uppercase().collect::<String>(),
                );
            }
        }
        ScannedWord {
            text: word.to_owned(),
            form,
            recognized: best.is_some(),
            syllables,
            alternatives,
            alternative_forms,
            ambiguous_syllable_count,
            accent,
            accent_form,
            accent_syllables,
        }
    }
    /// Scan one line, applying vowel/m elision and recognizing dactylic meters.
    pub fn scan_line(&self, line: &str) -> ScannedLine {
        self.scan_line_with_options(line, ScansionOptions::default())
    }
    /// Scan a line with explicit accentuation options.
    pub fn scan_line_with_options(&self, line: &str, options: ScansionOptions) -> ScannedLine {
        let mut words = Vec::new();
        let mut start = None;
        let mut previous_end = 0;
        for (offset, ch) in line
            .char_indices()
            .chain(std::iter::once((line.len(), '\0')))
        {
            if ch.is_alphabetic()
                || (start.is_some() && matches!(ch, '\u{0304}' | '\u{0306}' | '\u{0301}'))
            {
                if start.is_none() {
                    start = Some(offset);
                }
            } else if let Some(begin) = start.take() {
                let sentence_start = words.is_empty()
                    || line[previous_end..begin].contains(['.', '?', '!', ';', ':']);
                words.push(self.scan_word_with_options(
                    &line[begin..offset],
                    sentence_start,
                    options,
                ));
                previous_end = offset;
            }
        }
        for i in 0..words.len().saturating_sub(1) {
            let next_chars: Vec<_> = words[i + 1].form.chars().collect();
            let next_is_numeral = crate::roman_numeral(&words[i + 1].text).is_some();
            let next_consonant = next_chars.first().is_some_and(|c| {
                "bcdfgjklmnpqrstvxzBCDFGJKLMNPQRSTVXZ".contains(*c)
                    || (!next_is_numeral && consonantal(&next_chars, 0))
            });
            if next_consonant {
                let word = &mut words[i];
                let is_numeral = crate::roman_numeral(&word.text).is_some();
                if !is_numeral || word.form != word.text {
                    lengthen_before_consonant(&mut word.form);
                }
                for form in &mut word.alternative_forms {
                    if !is_numeral || *form != word.text {
                        lengthen_before_consonant(form);
                    }
                }
                word.syllables = syllabify(&word.form, word.recognized);
                word.alternatives = word
                    .alternative_forms
                    .iter()
                    .map(|form| {
                        let lexical = word.recognized
                            && !(crate::roman_numeral(&word.text).is_some() && form == &word.text);
                        syllabify(form, lexical)
                    })
                    .collect();
                word.ambiguous_syllable_count = word
                    .alternatives
                    .iter()
                    .any(|alternative| alternative.len() != word.syllables.len());
            }
            let begins = next_chars.first().is_some_and(|c| {
                matches!(c, 'h' | 'H')
                    || (vowel(*c) && (next_is_numeral || !consonantal(&next_chars, 0)))
            });
            let roman_vowel = crate::roman_numeral(&words[i].text).is_some()
                && words[i].text.ends_with(['I', 'U']);
            let elidable = begins && (can_elide(&words[i].form) || roman_vowel);
            let modify = |last: &mut Syllable| {
                if elidable {
                    last.elided = true;
                }
            };
            if let Some(last) = words[i].syllables.last_mut() {
                modify(last);
            }
            for alternative in &mut words[i].alternatives {
                if let Some(last) = alternative.last_mut() {
                    modify(last);
                }
            }
            if !words[i].ambiguous_syllable_count {
                let alternatives = &words[i].alternatives;
                let mut changes = Vec::new();
                for index in 0..words[i].syllables.len() {
                    let first = &alternatives[0][index];
                    if alternatives
                        .iter()
                        .any(|candidate| candidate[index].quantity != first.quantity)
                    {
                        changes.push(index);
                    }
                }
                for index in changes {
                    words[i].syllables[index].quantity = Quantity::Unknown;
                }
            }
        }
        let mut meters = Vec::new();
        if matches_meter(&words, Meter::Hexameter) {
            meters.push(Meter::Hexameter);
        }
        if matches_meter(&words, Meter::Pentameter) {
            meters.push(Meter::Pentameter);
        }
        ScannedLine { words, meters }
    }
    /// Scan each line of a text independently.
    pub fn scan_text(&self, text: &str) -> Vec<ScannedLine> {
        text.lines().map(|line| self.scan_line(line)).collect()
    }
    /// Scan each line with explicit accentuation options.
    pub fn scan_text_with_options(&self, text: &str, options: ScansionOptions) -> Vec<ScannedLine> {
        text.lines()
            .map(|line| self.scan_line_with_options(line, options))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn word(quantities: &[Quantity]) -> ScannedWord {
        let syllables: Vec<_> = quantities
            .iter()
            .map(|&quantity| Syllable {
                text: String::new(),
                quantity,
                elided: false,
            })
            .collect();
        ScannedWord {
            text: String::new(),
            form: String::new(),
            recognized: true,
            alternatives: vec![syllables.clone()],
            alternative_forms: vec![String::new()],
            syllables,
            ambiguous_syllable_count: false,
            accent: None,
            accent_form: String::new(),
            accent_syllables: Vec::new(),
        }
    }

    #[test]
    fn meter_checks_all_syllabification_alternatives() {
        use Quantity::{Long as L, Short as S, Unknown as X};
        let hexameter = vec![word(&[L, S, S, L, L, L, S, S, L, L, L, S, S, L, X])];
        assert!(matches_meter(&hexameter, Meter::Hexameter));
        let pentameter = vec![word(&[L, S, S, L, L, L, L, S, S, L, S, S, X])];
        assert!(matches_meter(&pentameter, Meter::Pentameter));
        let mut ambiguous = vec![word(&[S])];
        ambiguous[0]
            .alternatives
            .push(hexameter[0].syllables.clone());
        assert!(matches_meter(&ambiguous, Meter::Hexameter));
        assert!(!matches_meter(&[word(&[S, S, S])], Meter::Hexameter));
    }
}
