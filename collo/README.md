# collo

A Latin morphological analyzer: a Rust port of
[Collatinus](https://github.com/biblissima/collatinus). Given a Latin word, it
finds every lemma and inflection it could be. It can also tag running text,
mark vowel lengths and accents, scan verse, and inflect lemmas.

The Collatinus lexicon is compiled into the library at build time, so nothing
is read from disk at runtime.

```rust
use collo::Lemmatizer;

let analyzer = Lemmatizer::new();
for analysis in analyzer.analyze("puellae", false) {
    println!(
        "{}{} — {}: {}",
        analysis.form,
        analysis.suffix,
        analysis.lemma.headword,
        analysis.morphology("en").unwrap_or("?"),
    );
}
```

To try it from the command line, run this from the `loquaris` directory:

```sh
cargo run --release --example collo -- "Arma virumque cano"
```

## What it does

**Analysis.** `Lemmatizer::analyze(word, sentence_start)` returns every
possible reading of a word. Each reading has a lemma, a morphology ID, and the
word's spelling with vowel lengths marked. Set `sentence_start` when the word
begins a sentence, so a capitalized word can also be read as an ordinary
lowercase word.
- Lookup ignores vowel length, and treats i/j and u/v as the same letter.
- Lookup also covers common assimilations and contractions, and splits off enclitics
  such as *-que*.
- The spelling it returns may differ a little from the input, e.g. a
  contraction written out in full.
- `morphology(id, "en")` describes a morphology ID in English, French (`"fr"`)
  or Spanish (`"es"`).

**Lemmas.** Each `Lemma` has its headword, its translations, its frequency, and
the dictionary's grammatical notes. `lemmas()` lists all of them, and
`Lemmatizer::inflect` generates every form of one. An extra, less common
lexicon is available if you set `include_extension` on the `Lemmatizer`.

**Tagging.** `tag_text` selects the most likely reading of each word from its
context, using word frequencies from the LASLA corpus. `tag_text_with` does the
same with your own function supplying each word's readings.

**Scansion.** `scan_word`, `scan_line` and `scan_text` split words into
syllables and give their quantities, accents and elisions, along with the
dactylic meters a line could be.
- When there are several possible readings, all of them are kept.
- A syllable is marked unknown when the readings give it different lengths.
- `ScansionOptions` sets how an ambiguous penult is accented, and whether
  Collatinus's rule for words in *-ius* applies.

**Roman numerals.** `analyze_token` and `roman_numeral` read Roman numerals.

## Accuracy

collo returns structured data. It doesn't produce the original application's
HTML output.

Word analysis has been checked against the original C++ code on the texts in
`texts/`. Scansion, spelling variants and tagging haven't been fully checked,
especially in uncommon cases. [COMPARISON.md](COMPARISON.md) explains how to
run the comparisons.

## License

GPL, version 3 or later. collo is based on Collatinus, which is licensed under
GPL version 2 or later. Some of the bundled Collatinus data files carry LGPL
notices.
