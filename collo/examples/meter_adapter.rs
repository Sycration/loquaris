//! Line-oriented adapter for comparing metrical classifications.
use collo::{Lemmatizer, scansion::Meter};
use std::io::{self, BufRead, Write};
fn main() -> io::Result<()> {
    let mut out = io::BufWriter::new(io::stdout().lock());
    let analyzer = Lemmatizer::new();
    for line in io::stdin().lock().lines() {
        let line = analyzer.scan_line(&line?);
        writeln!(
            out,
            "{}{}",
            if line.meters.contains(&Meter::Hexameter) {
                "H"
            } else {
                ""
            },
            if line.meters.contains(&Meter::Pentameter) {
                "P"
            } else {
                ""
            }
        )?;
    }
    Ok(())
}
