//! Add vowel-length marks from collo's contextually selected analyses.
use unicode_normalization::{UnicodeNormalization, char::is_combining_mark};

#[cfg(test)]
pub fn macronize(text: &str) -> String {
    let analyzer = collo::Lemmatizer::new();
    let mut result = String::with_capacity(text.len());
    let mut cursor = 0;
    for word in analyzer.tag_text(text) {
        result.push_str(&text[cursor..word.range.start]);
        let original = &text[word.range.clone()];
        if let Some(analysis) = word.selected.and_then(|index| word.analyses.get(index)) {
            result.push_str(
                &analysis_word(original, analysis, false).unwrap_or_else(|| original.to_owned()),
            );
        } else {
            result.push_str(original);
        }
        cursor = word.range.end;
    }
    result.push_str(&text[cursor..]);
    result
}

fn letters(text: &str) -> Vec<(char, Vec<char>)> {
    let mut letters: Vec<(char, Vec<char>)> = Vec::new();
    for character in text.nfd() {
        if is_combining_mark(character) {
            if let Some((_, marks)) = letters.last_mut() {
                marks.push(character);
            }
        } else {
            letters.push((character, Vec::new()));
        }
    }
    letters
}

// Collo's marked forms encode syllable weight as well as vowel quantity.
// A consonant cluster makes the syllable heavy even when its vowel is short.
// Collo does not expose whether such a vowel is also naturally long, so do not
// infer a macron there; leave any explicitly supplied macron untouched.
fn heavy_by_position(letters: &[(char, Vec<char>)], vowel: usize) -> bool {
    let mut consonants = 0;
    let mut index = vowel + 1;
    while index < letters.len() {
        let c = letters[index].0.to_ascii_lowercase();
        if "aeiouy".contains(c) {
            break;
        }
        if c == 'h' {
            index += 1;
            continue;
        }
        if !c.is_ascii_alphabetic() {
            break;
        }
        consonants += if matches!(c, 'x' | 'z') { 2 } else { 1 };
        if consonants >= 2 {
            return true;
        }
        // In qu, u represents part of the consonant rather than a vowel.
        if c == 'q'
            && letters
                .get(index + 1)
                .is_some_and(|(next, _)| next.eq_ignore_ascii_case(&'u'))
        {
            index += 1;
        }
        index += 1;
    }
    false
}

pub fn analysis_word(
    original: &str,
    analysis: &collo::Analysis,
    replace_quantities: bool,
) -> Option<String> {
    apply_macrons(
        original,
        &format!("{}{}", analysis.form, analysis.suffix),
        replace_quantities,
    )
}

fn apply_macrons(original: &str, form: &str, replace_quantities: bool) -> Option<String> {
    let mut original_letters = letters(original);
    let form_letters = letters(form);
    // Some analyses reconstruct contractions, assimilations, or ligatures. Never
    // rewrite the user's spelling or guess where a mark belongs in those cases.
    if original_letters.len() != form_letters.len()
        || original_letters
            .iter()
            .zip(&form_letters)
            .any(|((left, _), (right, _))| {
                collo::normalize(&left.to_string()).to_lowercase()
                    != collo::normalize(&right.to_string()).to_lowercase()
            })
    {
        return None;
    }
    // Collatinus marks the first vowel of these diphthongs as long. Its
    // explicit quantity marks distinguish them from vowel sequences in hiatus.
    let mut diphthong = vec![false; form_letters.len()];
    for index in 0..form_letters.len().saturating_sub(1) {
        let (first, first_marks) = &form_letters[index];
        let (second, second_marks) = &form_letters[index + 1];
        let pair = format!(
            "{}{}",
            first.to_ascii_lowercase(),
            second.to_ascii_lowercase()
        );
        if ["ae", "oe", "au", "eu"].contains(&pair.as_str())
            && first_marks.contains(&'\u{0304}')
            && !second_marks.contains(&'\u{0304}')
            && !second_marks.contains(&'\u{0306}')
            && original_letters[index].0.eq_ignore_ascii_case(first)
            && original_letters[index + 1].0.eq_ignore_ascii_case(second)
        {
            diphthong[index] = true;
            diphthong[index + 1] = true;
        }
    }

    let mut changed = false;
    let positional: Vec<_> = (0..form_letters.len())
        .map(|index| heavy_by_position(&form_letters, index))
        .collect();
    for (index, ((base, marks), (_, form_marks))) in
        original_letters.iter_mut().zip(form_letters).enumerate()
    {
        if diphthong[index] {
            let before = marks.len();
            marks.retain(|mark| *mark != '\u{0304}');
            changed |= marks.len() != before;
            continue;
        }
        if positional[index] && form_marks.contains(&'\u{0304}') {
            continue;
        }
        if replace_quantities
            && "aeiouyAEIOUY".contains(*base)
            && (form_marks.contains(&'\u{0304}') || form_marks.contains(&'\u{0306}'))
        {
            let before = marks.len();
            marks.retain(|mark| *mark != '\u{0304}' && *mark != '\u{0306}');
            changed |= before != marks.len();
        }
        if "aeiouyAEIOUY".contains(*base)
            && form_marks.contains(&'\u{0304}')
            && !marks.contains(&'\u{0304}')
            && !marks.contains(&'\u{0306}')
        {
            marks.push('\u{0304}');
            changed = true;
        }
    }
    if !changed {
        return Some(original.to_owned());
    }
    let decomposed: String = original_letters
        .into_iter()
        .flat_map(|(base, marks)| std::iter::once(base).chain(marks))
        .collect();
    Some(decomposed.nfc().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aligns_macrons_without_changing_spelling_or_case() {
        assert_eq!(
            apply_macrons("VIRUMQUE", "vīrŭmquĕ", false),
            Some("VĪRUMQUE".into())
        );
        assert_eq!(apply_macrons("Julia", "Jūlĭă", false), Some("Jūlia".into()));
        assert_eq!(apply_macrons("amasti", "ămāvĭstī", false), None);
        assert_eq!(apply_macrons("mālum", "mālŭm", false), Some("mālum".into()));
        assert_eq!(apply_macrons("mālum", "mălŭm", true), Some("malum".into()));
    }

    #[test]
    fn removes_macrons_on_marked_diphthongs_but_not_hiatus() {
        for (original, form, expected) in [
            ("puellae", "pŭēllāe", "puellae"),
            ("puēllāe", "pŭēllāe", "puēllae"),
            ("PUELLĀE", "pŭēllāe", "PUELLAE"),
            ("foedus", "fōedŭs", "foedus"),
            ("aurum", "āurŭm", "aurum"),
            ("seu", "sēu", "seu"),
            ("poeta", "pŏētă", "poēta"),
        ] {
            assert_eq!(
                apply_macrons(original, form, false).as_deref(),
                Some(expected)
            );
        }
    }

    #[test]
    fn does_not_infer_vowel_length_from_position() {
        assert_eq!(
            apply_macrons("puella", "pŭēllă", false).as_deref(),
            Some("puella")
        );
        assert_eq!(apply_macrons("dux", "dūx", false).as_deref(), Some("dux"));
        assert_eq!(
            apply_macrons("aqua", "āquă", false).as_deref(),
            Some("āqua")
        );
        // Existing marks may indicate genuinely long vowels; keep them.
        assert_eq!(
            apply_macrons("cēnsus", "cēnsŭs", true).as_deref(),
            Some("cēnsus")
        );
    }

    #[test]
    fn processes_context_but_preserves_punctuation_and_unknown_words() {
        let result = macronize("Puella, xyzzy!\npuella.");
        assert_eq!(result, "Puella, xyzzy!\npuella.");
        assert_eq!(macronize(&result), result);
    }

    #[test]
    fn macronizes_a_sentence_with_all_six_cases() {
        // The girl gives the girl's rose to me with a friend: slave, come!
        // Puella: nominative; puellae: genitive; rosam: accusative;
        // mihi: dative; amica: ablative; serve: vocative.
        let sentence = "Puella puellae rosam mihi cum amica dat: serve, veni!";
        let words = collo::Lemmatizer::new().tag_text(sentence);
        for (word, case) in [
            ("Puella", "nominative"),
            ("puellae", "genitive"),
            ("rosam", "accusative"),
            ("mihi", "dative"),
            ("amica", "ablative"),
            ("serve", "vocative"),
        ] {
            let tagged = words.iter().find(|item| item.text == word).unwrap();
            let selected = &tagged.analyses[tagged.selected.expect("a contextual analysis")];
            assert!(
                selected.morphology("en").unwrap_or("").contains(case),
                "{word}: {:?}",
                selected.morphology("en")
            );
        }
        // Current collo behavior: it selects perfect "vēnī" here even though
        // the sentence calls for the imperative "venī".
        assert_eq!(
            macronize(sentence),
            "Puella puellae rosam mihi cum amīcā dat: serve, vēnī!"
        );
        assert_eq!(macronize("puēllāe"), "puēllae");
    }

    #[test]
    #[ignore = "collo currently selects the perfect instead of the imperative for veni"]
    fn imperative_veni_should_not_lengthen_the_first_vowel() {
        assert_eq!(
            macronize("Puella puellae rosam mihi cum amica dat: serve, veni!"),
            "Puella puellae rosam mihi cum amīcā dat: serve, venī!"
        );
    }
}
