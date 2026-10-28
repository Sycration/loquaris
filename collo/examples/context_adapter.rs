//! Line-oriented adapter for the contextual comparison script.
use collo::{Lemmatizer, scansion::Quantity};
use std::io::{self, BufRead, Write};
fn main() -> io::Result<()> {
    let input = io::stdin();
    let mut output = io::BufWriter::new(io::stdout().lock());
    let analyzer = Lemmatizer::new();
    for line in input.lock().lines() {
        let line = line?;
        let tagged = analyzer.tag_text(&line);
        let words = tagged
            .iter()
            .map(|w| w.text.as_str())
            .collect::<Vec<_>>()
            .join("|");
        let tags = tagged
            .iter()
            .map(|w| w.tag.as_deref().unwrap_or("???").to_owned())
            .collect::<Vec<_>>()
            .join("|");
        let scan = analyzer.scan_line(&line);
        let patterns = scan
            .words
            .iter()
            .map(|word| {
                word.syllables
                    .iter()
                    .map(|s| {
                        if s.elided {
                            '`'
                        } else {
                            match s.quantity {
                                Quantity::Long => '+',
                                Quantity::Short => '-',
                                Quantity::Unknown => '*',
                            }
                        }
                    })
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("|");
        writeln!(output, "{tags}\t{patterns}\t{words}")?;
    }
    Ok(())
}
