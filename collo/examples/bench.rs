//! Benchmark and golden-output harness.
//!
//! `cargo run --release --example bench -- bench [iterations]` times the public
//! APIs over the bundled `texts/` corpus. `... -- dump <file>` writes every
//! result in a deterministic textual form so optimizations can be diffed for
//! exact behavioural parity.
use collo::{Lemmatizer, scansion::ScansionOptions};
use std::{
    collections::BTreeSet,
    fmt::Write as _,
    hint::black_box,
    time::{Duration, Instant},
};

fn corpus() -> Vec<String> {
    let mut paths: Vec<_> = std::fs::read_dir("texts")
        .expect("run from the crate root")
        .map(|entry| entry.unwrap().path())
        .collect();
    paths.sort();
    paths
        .into_iter()
        .map(|path| {
            let bytes = std::fs::read(path).unwrap();
            match String::from_utf8(bytes) {
                Ok(text) => text,
                Err(error) => error.into_bytes().iter().map(|&b| b as char).collect(),
            }
        })
        .collect()
}

fn words(texts: &[String]) -> Vec<String> {
    let mut set = BTreeSet::new();
    for text in texts {
        for token in collo::tagging::tokenize(text) {
            set.insert(text[token.range].to_owned());
        }
    }
    set.into_iter().collect()
}

fn time(label: &str, iterations: u32, mut f: impl FnMut()) -> Duration {
    f();
    let mut best = Duration::MAX;
    for _ in 0..iterations {
        let start = Instant::now();
        f();
        best = best.min(start.elapsed());
    }
    println!("{label:<28} {:>10.3} ms", best.as_secs_f64() * 1e3);
    best
}

fn bench(iterations: u32) {
    let texts = corpus();
    let words = words(&texts);
    let analyzer = Lemmatizer::new();
    let extended = Lemmatizer {
        include_extension: true,
    };
    println!("{} unique words, {} texts", words.len(), texts.len());
    let mut total = Duration::ZERO;
    total += time("analyze", iterations, || {
        for word in &words {
            black_box(analyzer.analyze(word, false));
        }
    });
    total += time("analyze sentence_start", iterations, || {
        for word in &words {
            black_box(analyzer.analyze(word, true));
        }
    });
    total += time("analyze extension", iterations, || {
        for word in &words {
            black_box(extended.analyze(word, false));
        }
    });
    total += time("tag_text", iterations, || {
        for text in &texts {
            black_box(analyzer.tag_text(text));
        }
    });
    total += time("scan_text", iterations, || {
        for text in &texts {
            black_box(analyzer.scan_text(text));
        }
    });
    let sample: Vec<_> = collo::lemmas().iter().step_by(400).collect();
    total += time("inflect (205 lemmas)", iterations, || {
        for lemma in &sample {
            black_box(extended.inflect(lemma));
        }
    });
    total += time("tokenize", iterations, || {
        for text in &texts {
            black_box(collo::tagging::tokenize(text));
        }
    });
    println!("{:<28} {:>10.3} ms", "total", total.as_secs_f64() * 1e3);
}

fn dump(path: &str) {
    let texts = corpus();
    let words = words(&texts);
    let mut out = String::new();
    for include_extension in [false, true] {
        let analyzer = Lemmatizer { include_extension };
        for word in &words {
            for sentence_start in [false, true] {
                let token = analyzer.analyze_token(word, sentence_start);
                write!(out, "A {include_extension} {sentence_start} {word} {:?}:", token.roman_numeral).unwrap();
                for a in &token.analyses {
                    write!(
                        out,
                        " [{} {} {} {} {} {} {:?} {:?}]",
                        a.lemma.key,
                        a.morphology_id,
                        a.form,
                        a.suffix,
                        a.irregular,
                        a.multiplicity,
                        a.morphology("en"),
                        collo::tagging::analysis_tags(a),
                    )
                    .unwrap();
                }
                out.push('\n');
            }
        }
    }
    let analyzer = Lemmatizer::new();
    for text in &texts {
        for word in analyzer.tag_text(text) {
            writeln!(
                out,
                "T {:?} {:?} {:?} {:?} {}",
                word.text,
                word.range,
                word.tag,
                word.selected,
                word.analyses.len()
            )
            .unwrap();
        }
    }
    for options in [
        ScansionOptions::default(),
        ScansionOptions {
            ambiguous_accent: collo::scansion::AmbiguousAccent::Penult,
            illius_exception: true,
        },
    ] {
        for text in &texts {
            for line in analyzer.scan_text_with_options(text, options) {
                writeln!(out, "S {:?} {:?}", line.meters, line.words).unwrap();
            }
        }
    }
    let extended = Lemmatizer {
        include_extension: true,
    };
    for lemma in collo::lemmas().iter().step_by(37) {
        writeln!(out, "I {} {:?}", lemma.key, extended.inflect(lemma)).unwrap();
        writeln!(out, "L {lemma:?}").unwrap();
    }
    for id in 0..=420 {
        for lang in ["en", "fr", "es", "k9", "xx"] {
            writeln!(out, "M {id} {lang} {:?}", collo::morphology(id, lang)).unwrap();
        }
    }
    writeln!(out, "F {}", collo::lemmas().len()).unwrap();
    std::fs::write(path, out).unwrap();
}

fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("dump") => dump(args.get(1).expect("output path")),
        _ => bench(args.get(1).and_then(|n| n.parse().ok()).unwrap_or(3)),
    }
}
