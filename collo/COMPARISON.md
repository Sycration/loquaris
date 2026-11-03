# Comparing collo with Collatinus

collo is a port, so these scripts check it against the original C++ code from
Collatinus. They compile the original code, run both programs on the unique words
or lines of the texts in `texts/`, and report where the two outputs differ.

## Requirements

- Qt 5 development files and `moc`
- A C++ compiler
- Python 3
- Cargo

## The scripts

Run them from the `collo` directory.

| Script | Compares |
|---|---|
| `tools/compare.py` | Lemmas and morphology for each word |
| `tools/compare_context.py` | Contextual tags and vowel quantities in running text |
| `tools/compare_accents.py` | Accent placement, in six accent modes |
| `tools/compare_meters.py` | Meter classifications of lines of poetry |

All four accept `--output` (where to write differences) and `--limit` (how many
words or lines to test).

### Words: `compare.py`

```sh
python3 tools/compare.py
```

This compares the exact sets of lemma keys and morphology IDs found for each word.
Differences go to `/tmp/opencode/collo-comparison/differences.tsv` by default.

- The texts are read without being changed. The script strips navigation markup
  and numbering and handles the files' mixed encodings.
- Roman numerals are left out, because the two programs report them differently.
- By default the extension lexicon and sentence-initial capitalization are off.

Options:

- `--forms` also compares the spellings with vowel quantities, and enclitics.
- `--sentence-start` treats every word as starting a sentence.
- `--extension` includes the extension lexicon.

These are word-level checks; they don't test tagging or scansion.

### Tags and quantities: `compare_context.py`

```sh
python3 tools/compare_context.py [--sentences]
```

This compares the contextual tags and each word's pattern of vowel quantities,
line by line. `--sentences` joins wrapped lines so that whole sentences are
compared instead.

- Unknown words are left out. So are words for which the original gives no
  single syllable count.
- Lines where the words can't be lined up between the two programs are reported
  separately.
- When the original finds a word it can analyze but can't tag, its HTML output
  shifts the following tags onto the wrong words. Those runs are reported and
  left out, not counted as correct.
- The script restarts the C++ process when it hangs or crashes and reports the
  input that caused it. `--reference-timeout` sets how long to wait.

### Accents: `compare_accents.py`

```sh
python3 tools/compare_accents.py [--limit 0]
```

This compares six accent modes on an evenly spaced sample of the words in the
texts. `--limit 0` tests every unique word.

### Meters: `compare_meters.py`

```sh
python3 tools/compare_meters.py
```

This compares how whole lines of poetry are classified against the original's
search for feet. Two kinds of line are reported separately:

- hexameters in the source that the original fails to recognize;
- pentameters in the source whose second half doesn't have the usual two
  dactyls. collo doesn't classify these as pentameters.

The texts don't contain every possible meter, so these checks don't cover them
all.

## Known gaps

Scansion, spelling variants and contextual tagging haven't been fully checked
against the original. The likeliest places for differences are uncommon cases of
prosody and spelling.

## Deliberate differences

collo corrects these entries of `data/lemmes.la` against Lewis & Short, so its
results differ from Collatinus on them. Their old forms could not be reached from the
real Latin spelling.

| Entry | Collatinus | collo |
|---|---|---|
| `virgulta` | `templum`, which gave `virgulum` | `castra`, as for a plural-only neuter |
| `fomenta` | A separate lemma on `templum`, which gave `fomenum` | Removed; it is the plural of `fōmentum` |
| `habiliter` | `acriter`, which gave `habiriter` | `inv`, as an uncompared adverb |
| `Lingones` | Stem `Ligōn` | Stem `Līngŏn` |
| `cito` | `adve`, whose base form is `cite` | New model `advo`, with `cito`, `citius`, `citissime` |
| `molo` | `moneo`, and a 1st conjugation `molo3` | `lego` only; L&S gives `mŏlo, ŭi, ĭtum, 3` |

The frequency of the adverb `cito` also rises from 1 to 100, so that contextual
tagging selects it over the verb `cito`. That number is an estimate, not a corpus count: it
is between `citus` (60) and `celeriter` (188), and any value from 10 up gives the
same readings in the sentences tested.

`habiliter` and `molo` also gain English glosses from Lewis & Short, and
`fōmentum` now also has the English senses of the removed `fomenta`.
