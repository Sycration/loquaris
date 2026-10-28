# collo

A Latin morphological analyzer using the bundled Collatinus lexicon. The build
script resolves inflection models and embeds sorted native Rust tables; no data
files are read at runtime.

```rust
use collo::Lemmatizer;

let analyzer = Lemmatizer::new();
for analysis in analyzer.analyze("puellae", false) {
    println!("{}: {}", analysis.lemma.headword,
        analysis.morphology("en").unwrap_or("unknown"));
}
```

Results include all matching analyses, not contextual disambiguation. Set
`include_extension` to include the optional extension lexicon. The boolean
argument to `analyze` indicates sentence-initial capitalization. Lookup ignores
vowel quantity and i/j and u/v distinctions, and tries common assimilations,
contractions, and enclitics. Original grammatical notes and translations are
available on each lemma. Reconstructed spellings may reflect an expanded or
assimilated variant rather than the exact input spelling. Use `inflect` to
generate forms for a lemma, and `analyze_token` or `roman_numeral` to interpret
Roman numerals.

`tag_text` selects contextual tags using the bundled LASLA unigram and trigram
counts. `scan_word`, `scan_line`, and `scan_text` return syllables, quantities,
accents, elisions, and candidate dactylic meters. Scansion retains alternative
syllabifications; where alternatives of the same length disagree on a quantity,
the corresponding syllable is marked unknown. `ScansionOptions` controls the
accent on an ambiguous penult and the original's optional `-ius` rule. The
best-ranked accent reading and its syllables are exposed separately from the
best-ranked scansion reading.

These structured APIs do not reproduce the original application's HTML output.
Scansion, orthographic variant handling, and contextual tagging have not been
fully validated for parity with the original, especially uncommon prosodic and
orthographic cases. The bundled Collatinus data carries LGPL notices in the data
files.

## Comparison with the original

With Qt 5 development files, `moc`, a C++ compiler, Python 3, and Cargo installed,
run `python3 tools/compare.py`. This compiles the original C++ lemmatizer and
compares it to the Rust library on the unique words in `texts/`. The script reads
the texts without modifying them, strips navigation markup and numbering, handles
their mixed encodings, and excludes Roman numerals because the two APIs expose
them differently. It writes per-word differences to
`/tmp/opencode/collo-comparison/differences.tsv`.

By default, comparison is limited to exact sets of lemma keys and morphology
IDs, with the base lexicon and sentence-initial case handling disabled. Use
`--forms` to include quantified spellings and enclitics, `--sentence-start` to
enable sentence-initial case handling for every test word, or `--extension` to
include the extension lexicon. These are word-level comparisons, not tests of
contextual tagging or scansion.

Run `python3 tools/compare_context.py` to compare contextual tags and per-word
quantity patterns on the text lines. Add `--sentences` to join wrapped lines and
compare complete sentences. Quantity comparisons exclude unknown words and
cases where the original reports an indeterminate syllable count; token-alignment
failures are reported separately. The original's HTML tag output shifts labels
when it encounters an analyzable word with no tag; those sequences are reported
and excluded rather than counted as valid reference tags. The harness also
restarts the C++ process and reports inputs that hang or crash, with a configurable
`--reference-timeout`. Run `python3 tools/compare_accents.py` to compare six
accentuation modes on an evenly spaced sample of the corpus vocabulary; pass
`--limit 0` to test all unique words. `python3 tools/compare_meters.py` compares
whole-line classifications on the poetry against the original foot search. It
separately reports source hexameter misses and source pentameters that violate
the conventional two-dactyl second half, which Rust does not classify as
pentameters. These corpus comparisons do not exhaust every possible meter.
All four scripts accept `--output` and `--limit`.
