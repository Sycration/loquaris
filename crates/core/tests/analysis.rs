//! The desktop app's analysis scenarios (auto-macronize, parsing review,
//! per-occurrence lemma choices, side panel search), through the public API
//! as the browser worker uses it.
use loquaris_core::{
    EntryContent, Language, LemmaChoice, LessonContent, Participant, ProjectContent, SearchMode,
    SearchOptions, WordIndex,
};

/// The desktop test suite's sample lesson; entry IDs are "e0" to "e5".
fn sample() -> WordIndex {
    let entries = [
        (Participant::Teacher, Language::L1, "Introduction"),
        (Participant::Teacher, Language::Latin, "manus"),
        (Participant::Teacher, Language::L1, "Can you say hand?"),
        (Participant::Student, Language::Latin, "manus"),
        (Participant::Teacher, Language::L1, "What about both?"),
        (Participant::Student, Language::Latin, "duae manūs"),
    ];
    let mut index = WordIndex::new();
    index.set_project(ProjectContent {
        l1_name: "English".into(),
        lessons: vec![LessonContent {
            id: "body".into(),
            entries: entries
                .into_iter()
                .enumerate()
                .map(|(index, (participant, language, text))| EntryContent {
                    id: format!("e{index}"),
                    participant,
                    language,
                    text: text.into(),
                    lemmas: Vec::new(),
                    duration: None,
                })
                .collect(),
        }],
    });
    index
}

fn set_text(index: &mut WordIndex, id: &str, text: &str) {
    set_content(index, id, text, Vec::new());
}

fn set_content(index: &mut WordIndex, id: &str, text: &str, lemmas: Vec<LemmaChoice>) {
    let (participant, language) = match id {
        "e3" | "e5" => (Participant::Student, Language::Latin),
        "e0" | "e2" | "e4" => (Participant::Teacher, Language::L1),
        _ => (Participant::Teacher, Language::Latin),
    };
    index.set_entry(EntryContent {
        id: id.into(),
        participant,
        language,
        text: text.into(),
        lemmas,
        duration: None,
    });
}

/// Apply what a review would write to the entry, as the editor does.
fn apply(index: &mut WordIndex, id: &str, text: String, lemmas: Vec<LemmaChoice>) {
    set_content(index, id, &text, lemmas);
}

#[test]
fn auto_macronizing_is_latin_only_and_direct_when_unambiguous() {
    let mut index = sample();
    set_text(&mut index, "e0", "puella");
    assert!(index.review("e0").is_none(), "L1 entries aren't reviewed");

    set_text(&mut index, "e1", "Puellas, puellas!");
    let review = index.review("e1").unwrap();
    let direct = review.direct.expect("nothing to choose");
    assert!(direct.changed);
    assert_eq!(direct.text, "Puellās, puellās!");
    assert!(direct.lemmas.is_empty());
    apply(&mut index, "e1", direct.text, direct.lemmas);

    // Macronizing again changes nothing.
    let again = index.review("e1").unwrap().direct.unwrap();
    assert!(!again.changed);
    assert_eq!(again.text, "Puellās, puellās!");
    assert_eq!(index.macronize("Puellas, puellas!"), "Puellās, puellās!");
}

#[test]
fn applying_a_review_refuses_stale_text() {
    let mut index = sample();
    set_text(&mut index, "e1", "puellas");
    let review = index.review("e1").unwrap();
    let choices: Vec<_> = review.words.iter().map(|word| word.preselected).collect();
    let applied = index
        .apply_review("e1", &review.original, &choices)
        .unwrap();
    assert!(applied.changed);
    assert_eq!(applied.text, "puellās");
    apply(&mut index, "e1", applied.text, applied.lemmas);
    assert!(
        index
            .apply_review("e1", &review.original, &choices)
            .is_none()
    );
    assert!(
        index.apply_review("e1", "puellās", &[]).is_none(),
        "one choice per word"
    );
    assert!(index.apply_review("gone", "puellās", &[None]).is_none());
}

#[test]
fn auto_macronizing_opens_review_for_ambiguous_words() {
    let mut index = sample();
    set_text(&mut index, "e1", "malum");
    let review = index.review("e1").unwrap();
    assert!(review.direct.is_none());
    let [word] = &review.words[..] else {
        panic!("one word");
    };
    assert!(word.lemma_ambiguous);
    assert_eq!(word.text, "malum");
    assert_eq!(word.range, 0..5);
    // The context reading is listed first and preselected.
    let context = word.context_reading.expect("a reading from context");
    assert_eq!(word.display_order[0], context);
    assert_eq!(word.preselected, Some(context));
    let mut order = word.display_order.clone();
    order.sort_unstable();
    assert_eq!(order, (0..word.readings.len()).collect::<Vec<_>>());
    assert!(
        word.readings
            .iter()
            .all(|reading| reading.spelling.is_some())
    );
    assert!(
        word.readings
            .iter()
            .any(|reading| reading.lemma.lemma.key == "malus")
    );
    assert!(
        word.readings
            .iter()
            .any(|reading| reading.lemma.headword == "mālum")
    );
    // Keeping the original spelling saves no choice.
    let kept = index.apply_review("e1", "malum", &[None]).unwrap();
    assert_eq!(kept.text, "malum");
    assert!(kept.lemmas.is_empty());
    assert!(!kept.changed);
}

#[test]
fn reviewed_lemmas_belong_to_each_occurrence() {
    let mut index = sample();
    let sentence = "Dum bellum geritur, pilum bellum feret miles.";
    set_text(&mut index, "e1", sentence);
    set_text(&mut index, "e3", "bellum");
    let choice = |occurrence, lemma_key: &str| LemmaChoice {
        token: "bellum".into(),
        occurrence,
        lemma_key: lemma_key.into(),
        extension: false,
    };
    let new_keys = |index: &WordIndex| -> Vec<&str> {
        index
            .new_lemmas_in_lesson("body")
            .iter()
            .map(|lemma| lemma.key)
            .collect()
    };
    let hits_in_sentence = |index: &WordIndex, lemma_key| -> Vec<usize> {
        index
            .lemma_occurrences(lemma_key, false)
            .into_iter()
            .filter(|hit| &*hit.entry_id == "e1")
            .map(|hit| hit.range.start)
            .collect()
    };
    let positions = |text: &str| (text.find("bellum").unwrap(), text.rfind("bellum").unwrap());

    // Spelled alike, so only the lemma is in question; still ask.
    let review = index.review("e1").unwrap();
    assert!(review.direct.is_none());
    let reading = |word: usize, key: &str| {
        review.words[word]
            .readings
            .iter()
            .position(|reading| reading.lemma.lemma.key == key)
    };
    let mut choices: Vec<_> = review.words.iter().map(|word| word.preselected).collect();
    choices[1] = reading(1, "bellum");
    choices[4] = reading(4, "bellus");
    let applied = index.apply_review("e1", sentence, &choices).unwrap();
    assert!(applied.lemmas.contains(&choice(0, "bellum")));
    assert!(applied.lemmas.contains(&choice(1, "bellus")));
    let lemmas = applied.lemmas.clone();
    apply(&mut index, "e1", applied.text, applied.lemmas);
    let text = index.entry_text("e1").unwrap().to_owned();
    let (war, pretty) = positions(&text);
    assert_eq!(hits_in_sentence(&index, "bellum"), [war]);
    assert_eq!(hits_in_sentence(&index, "bellus"), [pretty]);
    assert!(new_keys(&index).contains(&"bellum"));
    assert!(new_keys(&index).contains(&"bellus"));

    // Reviewing the entry again starts from its choices.
    let again = index.review("e1").unwrap();
    let preselected: Vec<_> = again.words.iter().map(|word| word.preselected).collect();
    let reapplied = index.apply_review("e1", &text, &preselected).unwrap();
    assert_eq!(reapplied.lemmas, lemmas);
    assert!(!reapplied.changed);
    // Another entry's bellum is its own word, so it is asked about.
    assert!(index.review("e3").unwrap().direct.is_none());

    // Undo: the old text without choices.
    set_text(&mut index, "e1", sentence);
    let (before_war, before_pretty) = positions(sentence);
    assert_eq!(
        hits_in_sentence(&index, "bellus"),
        [before_war, before_pretty]
    );
    // Redo.
    apply(&mut index, "e1", text, lemmas);
    assert_eq!(hits_in_sentence(&index, "bellus"), [pretty]);
}

#[test]
fn saved_choices_are_ignored_when_marks_rule_their_lemma_out() {
    let mut index = sample();
    // mālae can't be mălus, whatever was chosen.
    let chosen = LemmaChoice {
        token: "malae".into(),
        occurrence: 0,
        lemma_key: "malus".into(),
        extension: false,
    };
    set_content(&mut index, "e1", "mālae", vec![chosen.clone()]);
    let keys = |index: &WordIndex| -> Vec<&str> {
        index
            .first_uses("e1")
            .iter()
            .map(|lemma| lemma.key)
            .collect()
    };
    assert_eq!(keys(&index), ["mala"]);
    set_content(&mut index, "e1", "malae", vec![chosen]);
    assert_eq!(keys(&index), ["malus"]);
    assert!(
        index.lemma_occurrences("mala", false).is_empty(),
        "chosen lemma only"
    );
}

#[test]
fn side_panel_searches_lemmas_and_updates_with_edits() {
    let mut index = sample();
    assert!(
        index
            .new_lemmas_in_lesson("body")
            .iter()
            .any(|lemma| lemma.key == "manus")
    );
    let options = SearchOptions::default();
    let entries = |index: &WordIndex| -> Vec<String> {
        index
            .search("manui", &options)
            .hits
            .iter()
            .map(|hit| hit.entry_id.to_string())
            .collect()
    };
    assert!(matches!(
        index.search("manui", &options).mode,
        SearchMode::Lemma(lemma) if lemma.key == "manus"
    ));
    assert_eq!(
        entries(&index),
        ["e1", "e3", "e5"],
        "every form of manus in Latin entries"
    );

    // Editing an entry refreshes the results.
    set_text(&mut index, "e1", "pes");
    assert_eq!(entries(&index), ["e3", "e5"]);
    set_text(&mut index, "e1", "manus");
    assert_eq!(entries(&index).len(), 3);
}
