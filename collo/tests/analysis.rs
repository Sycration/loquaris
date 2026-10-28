use collo::{Lemmatizer, analyze};

#[test]
fn matches_original_cpp_reference_forms() {
    for line in include_str!("fixtures/reference-forms.tsv").lines() {
        let (word, expected) = line.split_once('\t').expect("reference fixture format");
        let mut expected: Vec<_> = expected
            .split('\t')
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect();
        let mut actual: Vec<_> = analyze(word)
            .iter()
            .map(|a| {
                format!(
                    "{}:{}:{}:{}",
                    a.lemma.key, a.morphology_id, a.form, a.suffix
                )
            })
            .collect();
        expected.sort();
        expected.dedup();
        actual.sort();
        actual.dedup();
        assert_eq!(actual, expected, "reference mismatch for {word}");
    }
}

#[test]
fn matches_original_context_and_quantity_examples() {
    use collo::scansion::Quantity;
    let analyzer = Lemmatizer::new();
    for line in include_str!("fixtures/reference-context.tsv").lines() {
        let mut fields = line.split('\t');
        let text = fields.next().unwrap();
        let expected_tags: Vec<_> = fields
            .next()
            .unwrap()
            .split('|')
            .map(|tag| tag.replace('_', " "))
            .collect();
        let expected_patterns: Vec<_> = fields.next().unwrap().split('|').collect();
        let tags: Vec<_> = analyzer
            .tag_text(text)
            .iter()
            .map(|word| word.tag.clone().unwrap_or_else(|| "???".to_owned()))
            .collect();
        assert_eq!(tags, expected_tags, "tags for {text}");
        let scanned = analyzer.scan_line(text);
        assert_eq!(
            scanned.words.len(),
            expected_patterns.len(),
            "scan alignment for {text}"
        );
        for (word, expected) in scanned.words.iter().zip(expected_patterns) {
            if expected.starts_with(['?', '@']) {
                continue;
            }
            let actual: String = word
                .syllables
                .iter()
                .map(|syllable| {
                    if syllable.elided {
                        '`'
                    } else {
                        match syllable.quantity {
                            Quantity::Long => '+',
                            Quantity::Short => '-',
                            Quantity::Unknown => '*',
                        }
                    }
                })
                .collect();
            assert_eq!(actual, expected, "quantity for {} in {text}", word.text);
        }
    }
}

fn has(word: &str, key: &str) -> bool {
    analyze(word).iter().any(|a| a.lemma.key == key)
}

#[test]
fn regular_and_irregular_forms() {
    assert!(has("puellae", "puella"));
    assert!(has("amat", "amo"));
    assert!(has("sunt", "sum"));
    assert!(has("rosam", "rosa"));
    assert!(
        analyze("aliud")
            .iter()
            .any(|a| a.lemma.key == "alius" && a.irregular)
    );
    assert!(
        !analyze("alium")
            .iter()
            .any(|a| a.lemma.key == "alius" && a.morphology_id == 37)
    );
}

#[test]
fn morphology_and_translations_are_available() {
    let results = analyze("puellae");
    let result = results.iter().find(|a| a.lemma.key == "puella").unwrap();
    assert!(result.morphology("en").is_some());
    assert!(result.lemma.translation("en").is_some());
    assert!(collo::morphology(0, "en").is_none());
    assert!(collo::morphology(1, "unknown").is_none());
}

#[test]
fn normalization_case_and_enclitic() {
    assert_eq!(collo::normalize("Jūlius ævum"), "Iulius aeuum");
    assert!(has("puellamque", "puella"));
    assert!(
        Lemmatizer::new()
            .analyze("Puella", true)
            .iter()
            .any(|a| a.lemma.key == "puella")
    );
    assert!(analyze("").is_empty());
    assert!(analyze("zzzzzzzz").is_empty());
}

#[test]
fn roman_numerals_and_inflection() {
    assert_eq!(collo::roman_numeral("MCMXCIX"), Some(1999));
    assert_eq!(collo::roman_numeral("IV"), Some(4));
    assert_eq!(collo::roman_numeral("foo"), None);
    assert_eq!(collo::roman_numeral("IL"), None);
    let analyzer = Lemmatizer::new();
    let lemma = analyze("puella")
        .into_iter()
        .find(|a| a.lemma.key == "puella")
        .unwrap()
        .lemma;
    let forms = analyzer.inflect(lemma);
    assert!(
        forms
            .iter()
            .any(|f| f.morphology_id == 3 && collo::normalize(&f.form) == "puellam")
    );
    assert_eq!(analyzer.analyze_token("XIV", false).roman_numeral, Some(14));
}

#[test]
fn contextual_tagging_preserves_unknowns_and_ranges() {
    let analyzer = Lemmatizer::new();
    let text = "Puella rosam amat. zzzzz!";
    let words = analyzer.tag_text(text);
    assert_eq!(words.len(), 4);
    assert_eq!(words[0].text, "Puella");
    assert_eq!(&text[words[1].range.clone()], "rosam");
    assert!(words[0].tag.is_some());
    assert!(words[2].selected.is_some());
    assert!(words[3].tag.is_none());
    assert!(collo::tagging::tag_count("n11") > 0);
    assert!(collo::tagging::trigram_count("#  ", "a11", "n41") > 0);
    let simple = analyzer.tag_text("Puella rosam amat.");
    assert_eq!(
        simple
            .iter()
            .map(|word| word.tag.as_deref())
            .collect::<Vec<_>>(),
        vec![Some("n11"), Some("n31"), Some("v11")]
    );
    assert_eq!(collo::tagging::trigram_count("", "snt", "n11"), 4631);
    assert_eq!(
        analyzer.tag_text("DCCC milia.")[0].tag.as_deref(),
        Some("m  ")
    );
    assert!(
        analyze("servi")
            .iter()
            .any(|a| a.lemma.key == "seruio" && a.morphology_id == 139 && a.multiplicity == 2)
    );
}

#[test]
fn scans_syllables_elision_and_accents() {
    use collo::scansion::Quantity;
    let analyzer = Lemmatizer::new();
    let word = analyzer.scan_word("puella", false);
    assert!(word.recognized);
    assert_eq!(word.syllables.len(), 3);
    assert_eq!(word.syllables[1].quantity, Quantity::Long);
    assert_eq!(word.accent, Some(1));
    let line = analyzer.scan_line("multum ille");
    assert!(line.words[0].syllables.last().unwrap().elided);
    assert_eq!(analyzer.scan_text("arma\nvirum").len(), 2);
    let line = analyzer.scan_line("Puella rosam amat.");
    assert!(line.words[0].form.starts_with('P'));
    assert_eq!(
        line.words[0]
            .syllables
            .iter()
            .map(|s| s.quantity)
            .collect::<Vec<_>>(),
        vec![Quantity::Short, Quantity::Long, Quantity::Unknown]
    );
    assert!(line.words[1].syllables.last().unwrap().elided);
    assert_eq!(analyzer.scan_word("cui", false).syllables.len(), 1);
    assert_eq!(analyzer.scan_word("meum", false).syllables.len(), 2);
    let ambiguous = analyzer.scan_word("secernunt", false);
    assert_eq!(ambiguous.syllables[1].quantity, Quantity::Unknown);
    let enclitic = analyzer.scan_word("virumque", false);
    assert_eq!(enclitic.syllables[1].quantity, Quantity::Long);
    let enclitic = analyzer.scan_word("puellaque", false);
    assert_eq!(enclitic.accent, Some(enclitic.syllables.len() - 2));
    let contextual = analyzer.scan_line("huic totum");
    assert_eq!(
        contextual.words[0]
            .syllables
            .iter()
            .map(|s| s.quantity)
            .collect::<Vec<_>>(),
        vec![Quantity::Long, Quantity::Long]
    );
}

#[test]
fn accent_options_match_cpp_examples() {
    use collo::scansion::{AmbiguousAccent, ScansionOptions};
    let analyzer = Lemmatizer::new();
    let accent = |word, mode, exception| {
        analyzer
            .scan_word_with_options(
                word,
                false,
                ScansionOptions {
                    ambiguous_accent: mode,
                    illius_exception: exception,
                },
            )
            .accent
    };
    for word in ["puellaque", "virumque"] {
        let syllables = analyzer.scan_word(word, false).syllables.len();
        for mode in [
            AmbiguousAccent::Penult,
            AmbiguousAccent::Antepenult,
            AmbiguousAccent::Undetermined,
        ] {
            assert_eq!(accent(word, mode, false), Some(syllables - 2));
        }
    }
    for word in ["illius", "secernunt"] {
        assert_eq!(accent(word, AmbiguousAccent::Penult, false), Some(1));
        assert_eq!(accent(word, AmbiguousAccent::Antepenult, false), Some(0));
        assert_eq!(accent(word, AmbiguousAccent::Undetermined, false), None);
    }
    assert_eq!(accent("illius", AmbiguousAccent::Antepenult, true), Some(1));
    assert_eq!(
        accent("domino", AmbiguousAccent::Undetermined, false),
        Some(0)
    );
    assert_eq!(
        accent("contingere", AmbiguousAccent::Penult, false),
        Some(1)
    );
    assert_eq!(
        accent("contingere", AmbiguousAccent::Undetermined, false),
        Some(1)
    );
    // Two lexicon spellings have different syllable counts; accent ranking
    // must expose the syllables of the selected reading.
    let variant_reading = analyzer.scan_word_with_options(
        "Iesus",
        true,
        ScansionOptions {
            ambiguous_accent: AmbiguousAccent::Penult,
            illius_exception: false,
        },
    );
    assert_eq!(variant_reading.accent_syllables.len(), 2);
    assert_eq!(variant_reading.accent, Some(0));
}

#[test]
fn known_hexameter() {
    use collo::scansion::Meter;
    let line = Lemmatizer::new().scan_line("Arma virumque cano Troiae qui primus ab oris");
    assert!(line.meters.contains(&Meter::Hexameter), "{line:#?}");
    for line in [
        "conciliumque vocat divum pater atque hominum rex",
        "quae contra vetitum discordia? quis metus aut hos",
    ] {
        assert!(
            Lemmatizer::new()
                .scan_line(line)
                .meters
                .contains(&Meter::Hexameter),
            "{line}"
        );
    }
    assert!(
        !Lemmatizer::new()
            .scan_line("nec partem solido demere de die")
            .meters
            .contains(&Meter::Pentameter)
    );
}

#[test]
fn spelling_variants_and_exclusive_irregulars() {
    assert!(has("amasse", "amo"));
    assert!(has("affero", "affero") || has("affero", "adfero"));
    let alius = analyze("aliud")
        .into_iter()
        .find(|a| a.lemma.key == "alius")
        .unwrap()
        .lemma;
    let forms = Lemmatizer::new().inflect(alius);
    assert!(
        forms
            .iter()
            .any(|f| f.morphology_id == 37 && f.irregular && collo::normalize(&f.form) == "aliud")
    );
    assert!(!forms.iter().any(|f| f.morphology_id == 37 && !f.irregular));
}

#[test]
fn reference_contractions_and_suffix_inheritance() {
    let analyses = analyze("amaris");
    let morphology: Vec<_> = analyses
        .iter()
        .filter(|a| a.lemma.key == "amo")
        .map(|a| a.morphology_id)
        .collect();
    assert_eq!(morphology, vec![152, 170]);
    assert!(!analyze("quae").iter().any(|a| a.lemma.key == "quidam"));
    assert!(!analyze("quod").iter().any(|a| a.lemma.key == "quiuis"));
    assert!(analyze("flavom").is_empty());
    let amo = analyses.iter().find(|a| a.lemma.key == "amo").unwrap();
    assert_eq!(amo.form, "ămārī̆s");
    let assimilated = analyze("adflictum");
    assert!(
        assimilated
            .iter()
            .any(|a| a.lemma.key == "afflictus" && a.form == "ādflīctŭm")
    );
    let enclitic = analyze("virumque");
    assert!(
        enclitic
            .iter()
            .any(|a| a.lemma.key == "uir" && a.form == "vĭrŭm" && a.suffix == "quĕ")
    );
    let proper_name = Lemmatizer::new().analyze("Caepione", true);
    assert!(
        proper_name
            .iter()
            .any(|a| a.lemma.key == "Caepio" && a.morphology_id == 6)
    );
    assert!(
        !proper_name
            .iter()
            .any(|a| a.lemma.key == "Caepio" && matches!(a.morphology_id, 1 | 2))
    );
    assert!(
        analyze("partheniumque")
            .iter()
            .any(|a| a.lemma.key == "Parthenius")
    );
    assert!(
        analyze("Grais")
            .iter()
            .any(|a| a.lemma.key == "Graius" && a.form == "Grājīs")
    );
}
