//! Add vowel-length marks from collo's contextually selected analyses.
use unicode_normalization::{UnicodeNormalization, char::is_combining_mark};

#[cfg(test)]
pub fn macronize(text: &str) -> String {
    let analyzer = collo::Lemmatizer::new();
    let mut cache = crate::word_index::WordIndex::default();
    let mut result = String::with_capacity(text.len());
    let mut cursor = 0;
    for word in cache.tag_text(&analyzer, text) {
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
        // Some short y's in the Collatinus data are a Cyrillic ў, which decomposes
        // into a lookalike letter that does not compare equal to a y.
        let character = match character {
            'у' => 'y',
            'У' => 'Y',
            other => other,
        };
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

/// How well an analysis matches the vowel quantities written in a word.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum QuantityMatch {
    /// A written macron or breve conflicts with the analysis.
    Conflict,
    /// Written marks match, but some unmarked vowel is long in the analysis,
    /// or the spelling can't be aligned with the analysis.
    MarksMatch,
    /// Marked vowels match and every unmarked vowel can be short.
    Exact,
}

const MACRON: char = '\u{0304}';
const BREVE: char = '\u{0306}';
/// Collatinus's marker on a u that is not a vowel of its own, as in sụādeo.
const DOT_BELOW: char = '\u{0323}';

/// Compare the quantities written in `written` with an analysis's form.
/// Diphthongs and vowels long only by position are not compared, since
/// Collo's marks there give no information about the vowel itself.
pub fn quantity_match(written: &str, analysis: &collo::Analysis) -> QuantityMatch {
    let written_letters = letters(written);
    let form_letters = letters(&format!("{}{}", analysis.form, analysis.suffix));
    let aligned = written_letters.len() == form_letters.len()
        && written_letters
            .iter()
            .zip(&form_letters)
            .all(|((left, _), (right, _))| {
                collo::normalize(&left.to_string()).to_lowercase()
                    == collo::normalize(&right.to_string()).to_lowercase()
            });
    if !aligned {
        return QuantityMatch::MarksMatch;
    }
    let mut result = QuantityMatch::Exact;
    let mut index = 0;
    while index < form_letters.len() {
        let (base, form_marks) = &form_letters[index];
        let is_vowel = "aeiouy".contains(base.to_ascii_lowercase());
        let next = form_letters.get(index + 1);
        let diphthong = next.is_some_and(|(second, second_marks)| {
            let pair = format!(
                "{}{}",
                base.to_ascii_lowercase(),
                second.to_ascii_lowercase()
            );
            ["ae", "oe", "au", "eu"].contains(&pair.as_str())
                && form_marks.contains(&MACRON)
                && !second_marks.contains(&MACRON)
                && !second_marks.contains(&BREVE)
        });
        if diphthong {
            index += 2;
            continue;
        }
        if is_vowel {
            let written_marks = &written_letters[index].1;
            // A vowel marked both ways can be either, so no mark conflicts with it.
            let anceps = form_marks.contains(&MACRON) && form_marks.contains(&BREVE);
            let form_long =
                form_marks.contains(&MACRON) && !anceps && !heavy_by_position(&form_letters, index);
            let form_short = form_marks.contains(&BREVE) && !anceps;
            if written_marks.contains(&MACRON) {
                if form_short {
                    return QuantityMatch::Conflict;
                }
            } else if written_marks.contains(&BREVE) {
                if form_long {
                    return QuantityMatch::Conflict;
                }
            } else if form_long {
                result = QuantityMatch::MarksMatch;
            }
        }
        index += 1;
    }
    result
}

/// Indices of the analyses that match the written quantities: those with no
/// conflicting macron or breve. With `unmarked_short`, unmarked vowels are also
/// read as short when an analysis with them short exists, so "malae" is mălus
/// rather than māla; otherwise no analysis is excluded because of them. If every
/// analysis conflicts with the marks, all are kept.
pub fn matching_analyses(
    written: &str,
    analyses: &[collo::Analysis],
    unmarked_short: bool,
) -> Vec<usize> {
    let matches: Vec<_> = analyses
        .iter()
        .map(|analysis| quantity_match(written, analysis))
        .collect();
    let required = if unmarked_short {
        matches
            .iter()
            .copied()
            .max()
            .unwrap_or(QuantityMatch::Conflict)
    } else {
        QuantityMatch::MarksMatch
    };
    let keep: Vec<_> = (0..analyses.len())
        .filter(|&index| matches[index] >= required)
        .collect();
    if keep.is_empty() {
        (0..analyses.len()).collect()
    } else {
        keep
    }
}

/// A Collo form or headword as a Latin reader expects to see it: macrons on
/// naturally long vowels, and no breves or marks on diphthongs and vowels long
/// only by position. Collo's own marks are for scansion.
pub fn reader_form(form: &str) -> String {
    let unmarked: String = letters(form)
        .into_iter()
        .flat_map(|(base, marks)| {
            std::iter::once(base).chain(
                marks
                    .into_iter()
                    .filter(|mark| ![MACRON, BREVE, DOT_BELOW].contains(mark)),
            )
        })
        .collect::<String>()
        .nfc()
        .collect();
    apply_macrons(&unmarked, form, false).unwrap_or(unmarked)
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
    // Some analyses contain reconstructed contractions, assimilations, or ligatures.
    // In those cases the user's spelling is unchanged and no mark is added.
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
    fn reader_forms_keep_only_the_marks_a_reader_expects() {
        for (form, expected) in [
            ("pŭēlla", "puella"),
            ("ămo", "amo"),
            ("ōdī", "ōdī"),
            ("stēlla", "stella"),
            ("āequus,āecus", "aequus,aecus"),
            ("rĕgĭō\u{306}", "regiō"),
            ("sụādeo", "suādeo"),
            ("Āeschўlus", "Aeschylus"),
            ("Prўtănis", "Prytanis"),
        ] {
            assert_eq!(reader_form(form), expected);
        }
        assert!(reader_form("Āeschўlus").is_ascii());
    }

    #[test]
    fn a_vowel_marked_both_ways_matches_either_written_mark() {
        let mut analysis = collo::analyze("regio")
            .into_iter()
            .next()
            .expect("regio is in the lexicon");
        analysis.form = "rĕgĭō\u{306}".into();
        analysis.suffix = String::new();
        for written in ["regiō", "regiŏ"] {
            assert_ne!(
                quantity_match(written, &analysis),
                QuantityMatch::Conflict,
                "{written}"
            );
        }
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
    fn does_not_derive_vowel_length_from_position() {
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
    fn written_quantities_narrow_the_readings() {
        let lemmas = |word: &str, unmarked_short| {
            let all = collo::Lemmatizer::new().analyze(word, true);
            let mut keys: Vec<_> = matching_analyses(word, &all, unmarked_short)
                .into_iter()
                .map(|index| all[index].lemma.key)
                .collect();
            keys.dedup();
            keys
        };
        // māla (cheeks) has a long a; mălus (bad) a short one.
        for unmarked_short in [false, true] {
            assert_eq!(lemmas("mālae", unmarked_short), ["mala"]);
            assert_eq!(lemmas("mălae", unmarked_short), ["malus"]);
            // A vowel long only by position (ēst) gives no information about its length.
            assert_eq!(lemmas("est", unmarked_short).len(), 2);
            // When every reading has a long unmarked vowel, none is excluded.
            assert!(lemmas("amica", unmarked_short).contains(&"amica"));
        }
        // By default, no reading is excluded because of unmarked vowels.
        assert_eq!(lemmas("malae", false), ["mala", "malus"]);
        // Optionally they are short when a reading with them short exists.
        assert_eq!(lemmas("malae", true), ["malus"]);
        assert_eq!(lemmas("Malae", true), ["malus"]);
    }

    #[test]
    fn marks_on_diphthongs_and_positional_length_are_not_compared() {
        let analysis = |word: &str, form: &str| {
            let mut analysis = collo::analyze(word)
                .into_iter()
                .next()
                .expect("a known word");
            analysis.form = form.into();
            analysis.suffix.clear();
            analysis
        };
        let puellae = analysis("puellae", "pŭēllāe");
        for written in ["puellae", "puēllae", "puellāe", "puēllāe"] {
            assert_eq!(quantity_match(written, &puellae), QuantityMatch::Exact);
        }
        assert_eq!(quantity_match("pūellae", &puellae), QuantityMatch::Conflict);
        // Spellings that can't be aligned are neither ranked first nor excluded.
        assert_eq!(
            quantity_match("amasti", &analysis("amasti", "ămāvĭstī")),
            QuantityMatch::MarksMatch
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
        // Current collo behavior: the selected reading here is the perfect "vēnī",
        // although the sentence requires the imperative "venī".
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
