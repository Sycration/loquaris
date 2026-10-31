//! Checks the build-time lookup tables against the source data files.
use collo::tagging::{tag_count, trigram_count};
use std::collections::BTreeMap;

#[test]
fn tag_statistics_match_source() {
    let raw = std::fs::read_to_string("data/tags.la").unwrap();
    let mut in_trigrams = false;
    let mut unigrams = BTreeMap::<String, u64>::new();
    let mut trigrams = BTreeMap::<String, u64>::new();
    for line in raw.lines() {
        if line.starts_with("! ---") {
            in_trigrams = true;
            continue;
        }
        if line.is_empty() || line.starts_with('!') {
            continue;
        }
        let (key, count) = line.split_once(',').unwrap();
        let table = if in_trigrams {
            &mut trigrams
        } else {
            &mut unigrams
        };
        *table.entry(key.to_owned()).or_default() += count.parse::<u64>().unwrap();
    }
    for (tag, count) in &unigrams {
        assert_eq!(tag_count(tag), *count, "{tag:?}");
    }
    for (key, count) in &trigrams {
        let actual = if key.len() == 7 {
            trigram_count("", &key[0..3], &key[4..7])
        } else {
            trigram_count(&key[0..3], &key[4..7], &key[8..11])
        };
        assert_eq!(actual, *count, "{key:?}");
    }
    // Absent sequences, unknown tags, and malformed keys.
    assert_eq!(trigram_count("zzz", "n11", "v11"), 0);
    assert_eq!(trigram_count("a", "b", "c"), 0);
    assert_eq!(trigram_count("", "", ""), 0);
    assert_eq!(tag_count("zzz"), 0);
}

#[test]
fn lemma_lookup_by_key() {
    for lemma in collo::lemmas().iter().step_by(97) {
        let found = collo::find_lemma(lemma.key, lemma.extension).unwrap();
        assert_eq!(found.key, lemma.key);
        assert_eq!(found.extension, lemma.extension);
        let first = collo::lemmas()
            .iter()
            .find(|l| l.key == lemma.key && l.extension == lemma.extension)
            .unwrap();
        assert!(std::ptr::eq(found, first));
    }
    assert!(collo::find_lemma("amo", false).is_some());
    assert!(collo::find_lemma("no such lemma", false).is_none());
}
