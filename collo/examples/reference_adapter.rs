//! Internal adapter used by tools/compare.py; one word per input line.
use std::io::{self, BufRead, Write};
fn main() -> io::Result<()> {
    let input = io::stdin();
    let mut output = io::BufWriter::new(io::stdout().lock());
    let args: Vec<_> = std::env::args().skip(1).collect();
    let sentence_start = args.iter().any(|arg| arg == "--sentence-start");
    let forms = args.iter().any(|arg| arg == "--forms");
    let tags = args.iter().any(|arg| arg == "--tags");
    let analyzer = collo::Lemmatizer {
        include_extension: args.iter().any(|arg| arg == "--extension"),
    };
    for word in input.lock().lines() {
        let mut analyses: Vec<String> = analyzer
            .analyze(&word?, sentence_start)
            .iter()
            .flat_map(|a| {
                let value = if forms {
                    format!(
                        "{}:{}:{}:{}",
                        a.lemma.key, a.morphology_id, a.form, a.suffix
                    )
                } else {
                    format!("{}:{}", a.lemma.key, a.morphology_id)
                };
                if tags {
                    collo::tagging::analysis_tags(a)
                        .into_iter()
                        .map(|tag| format!("{value}:{tag}"))
                        .collect::<Vec<_>>()
                } else {
                    vec![value]
                }
            })
            .collect();
        analyses.sort();
        analyses.dedup();
        writeln!(output, "{}", analyses.join("\t"))?;
    }
    Ok(())
}
