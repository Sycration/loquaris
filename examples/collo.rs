//! Explore collo without launching the GUI: cargo run --example collo -- "Latin text"
use collo::{Lemmatizer, scansion::Quantity};

fn main() {
    let text = std::env::args().skip(1).collect::<Vec<_>>().join(" ");
    let text = if text.is_empty() {
        "Arma virumque cano, Troiae qui primus ab oris"
    } else {
        &text
    };
    let analyzer = Lemmatizer::new();

    println!("Text: {text}\n");
    println!("Contextual tags and possible analyses:");
    for word in analyzer.tag_text(text) {
        println!(
            "\n{} (tag: {})",
            word.text,
            word.tag.as_deref().unwrap_or("unknown")
        );
        if word.analyses.is_empty() {
            println!("  No dictionary analyses");
        }
        for (index, analysis) in word.analyses.iter().enumerate() {
            let selected = if word.selected == Some(index) {
                "*"
            } else {
                " "
            };
            println!(
                " {selected} {} — {}; {}{}{}",
                analysis.lemma.headword,
                analysis.morphology("en").unwrap_or("unknown morphology"),
                analysis
                    .lemma
                    .translation("en")
                    .unwrap_or("no English translation"),
                if analysis.suffix.is_empty() {
                    ""
                } else {
                    "; enclitic: "
                },
                analysis.suffix,
            );
        }
    }

    let scan = analyzer.scan_line(text);
    println!("\nScansion (– long, u short, ? unknown, × elided):");
    for word in &scan.words {
        let syllables = word
            .syllables
            .iter()
            .map(|syllable| {
                let quantity = if syllable.elided {
                    '×'
                } else {
                    match syllable.quantity {
                        Quantity::Long => '–',
                        Quantity::Short => 'u',
                        Quantity::Unknown => '?',
                    }
                };
                format!("{}{quantity}", syllable.text)
            })
            .collect::<Vec<_>>()
            .join(" ");
        println!("  {}: {syllables}", word.text);
    }
    println!("Candidate meters: {:?}", scan.meters);

    println!("\nOther APIs:");
    let token = analyzer.analyze_token("XIV", false);
    println!("  XIV as a Roman numeral: {:?}", token.roman_numeral);
    if let Some(analysis) = analyzer.analyze("puella", false).first() {
        let forms = analyzer.inflect(analysis.lemma);
        println!(
            "  {} has {} generated forms (first five):",
            analysis.lemma.headword,
            forms.len()
        );
        for form in forms.iter().take(5) {
            println!(
                "    {} — {}",
                form.form,
                collo::morphology(form.morphology_id, "en").unwrap_or("unknown")
            );
        }
    }
}
