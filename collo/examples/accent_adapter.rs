//! Line-oriented adapter for comparison with the original accentuation modes.
use collo::{
    Lemmatizer,
    scansion::{AmbiguousAccent, ScansionOptions},
};
use std::io::{self, BufRead, Write};
fn main() -> io::Result<()> {
    let mut output = io::BufWriter::new(io::stdout().lock());
    let analyzer = Lemmatizer::new();
    for word in io::stdin().lock().lines() {
        let word = word?;
        let mut accents = Vec::new();
        let mut readings = Vec::new();
        for exception in [false, true] {
            for mode in [
                AmbiguousAccent::Penult,
                AmbiguousAccent::Antepenult,
                AmbiguousAccent::Undetermined,
            ] {
                let analysis = analyzer.scan_word_with_options(
                    &word,
                    true,
                    ScansionOptions {
                        ambiguous_accent: mode,
                        illius_exception: exception,
                    },
                );
                accents.push(
                    analysis
                        .accent
                        .map(|n| n.to_string())
                        .unwrap_or_else(|| "-".to_owned()),
                );
                readings.push(
                    analysis
                        .accent_syllables
                        .iter()
                        .map(|s| s.text.as_str())
                        .collect::<Vec<_>>()
                        .join("|"),
                );
            }
        }
        writeln!(output, "{}\t{}", accents.join(","), readings.join(";"))?;
    }
    Ok(())
}
