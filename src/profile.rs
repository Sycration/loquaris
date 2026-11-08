//! Ignored profiling tests on a generated project of about a million Latin
//! words. Generate the fixture once, then profile:
//!
//! cargo test --release profile_ -- --ignored --nocapture --test-threads=1
//!
//! `LOQUARIS_PROFILE_DIR` overrides where the fixture is kept (default
//! `target/profile`).
use crate::{
    Entry, Language, Lesson, Participant,
    app::{App, EntryAction, TextTarget},
    parsing_review, project,
    ui::configure_fonts,
    word_index,
};
use eframe::egui::{self, Vec2};
use std::{path::PathBuf, time::Instant};
use unicode_normalization::{UnicodeNormalization, char::is_combining_mark};

const LESSONS: usize = 250;
const ENTRIES_PER_LESSON: usize = 800;

fn dir() -> PathBuf {
    std::env::var_os("LOQUARIS_PROFILE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/profile"))
}

fn fixture() -> PathBuf {
    dir().join("million.loqr")
}

fn rss() -> String {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|status| {
            status
                .lines()
                .find(|line| line.starts_with("VmRSS:"))
                .map(|line| line.trim_start_matches("VmRSS:").trim().to_owned())
        })
        .unwrap_or_default()
}

fn entry(participant: Participant, language: Language, text: String) -> Entry {
    Entry {
        participant,
        language,
        text,
        audio: None,
        trim: None,
        lemmas: Vec::new(),
    }
}

/// A deterministic xorshift generator.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

fn plain(form: &str) -> String {
    form.nfd()
        .filter(|c| !is_combining_mark(*c))
        .map(|c| match c {
            'у' => 'y',
            other => other,
        })
        .collect()
}

#[test]
#[ignore = "writes the profiling fixture"]
fn profile_generate_fixture() {
    let mut lemmas: Vec<_> = collo::lemmas()
        .iter()
        .filter(|lemma| !lemma.extension && lemma.frequency > 0)
        .collect();
    lemmas.sort_by_key(|lemma| std::cmp::Reverse(lemma.frequency));
    lemmas.truncate(4000);
    let analyzer = collo::Lemmatizer::new();
    let forms: Vec<Vec<String>> = lemmas
        .iter()
        .map(|lemma| {
            let mut forms: Vec<_> = analyzer
                .inflect(lemma)
                .into_iter()
                .map(|form| plain(&form.form).to_lowercase())
                .filter(|form| !form.is_empty() && form.chars().all(char::is_alphabetic))
                .collect();
            forms.sort();
            forms.dedup();
            forms
        })
        .filter(|forms| !forms.is_empty())
        .collect();
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    let mut latin_words = 0;
    let mut unique = std::collections::HashSet::new();
    let lessons = (0..LESSONS)
        .map(|lesson| Lesson {
            name: format!("Lesson {}", lesson + 1),
            expanded: false,
            entries: (0..ENTRIES_PER_LESSON)
                .map(|index| {
                    if index % 5 == 0 {
                        return entry(
                            Participant::Teacher,
                            Language::L1,
                            "How would you say this in Latin?".into(),
                        );
                    }
                    let count = 2 + rng.below(9);
                    let mut words = Vec::with_capacity(count);
                    for _ in 0..count {
                        // Frequent lemmas are much more likely, as in real text.
                        let lemma = (forms.len() as f64 * rng.unit().powf(3.0)) as usize;
                        let options = &forms[lemma.min(forms.len() - 1)];
                        let word = options[rng.below(options.len())].clone();
                        unique.insert(word.clone());
                        words.push(word);
                    }
                    latin_words += count;
                    let mut text = words.join(" ");
                    if let Some(first) = text.get(..1) {
                        text.replace_range(..1, &first.to_uppercase());
                    }
                    text.push(if index % 3 == 0 { '?' } else { '.' });
                    let participant = if index % 2 == 0 {
                        Participant::Teacher
                    } else {
                        Participant::Student
                    };
                    entry(participant, Language::Latin, text)
                })
                .collect(),
        })
        .collect();
    std::fs::create_dir_all(dir()).unwrap();
    project::Project::new("Profile".into(), "English".into(), lessons)
        .save(&fixture())
        .unwrap();
    eprintln!(
        "wrote {}: {latin_words} Latin words, {} unique spellings",
        fixture().display(),
        unique.len()
    );
}

fn time<T>(label: &str, action: impl FnOnce() -> T) -> T {
    let started = Instant::now();
    let result = action();
    eprintln!(
        "{label:<44} {:>10.2} ms   rss {}",
        started.elapsed().as_secs_f64() * 1000.0,
        rss()
    );
    result
}

/// Mean, median and worst time of `count` runs.
fn repeat(label: &str, count: usize, mut action: impl FnMut(usize)) {
    let mut times: Vec<f64> = (0..count)
        .map(|run| {
            let started = Instant::now();
            action(run);
            started.elapsed().as_secs_f64() * 1000.0
        })
        .collect();
    times.sort_by(f64::total_cmp);
    let mean = times.iter().sum::<f64>() / count as f64;
    eprintln!(
        "{label:<44} {mean:>10.3} ms mean, {:.3} median, {:.3} max (n={count})",
        times[count / 2],
        times[count - 1]
    );
}

fn render(app: &mut App, ctx: &egui::Context, time: f64) {
    let mut frame = eframe::Frame::_new_kittest();
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            Vec2::new(1200.0, 750.0),
        )),
        time: Some(time),
        ..Default::default()
    };
    let mut output = ctx.run_ui(input, |ui| eframe::App::ui(app, ui, &mut frame));
    output.textures_delta.clear();
}

/// Latin entries spread through the project.
fn sample_entries(app: &App, count: usize) -> Vec<(usize, usize)> {
    let mut rng = Rng(42);
    let mut positions = Vec::new();
    while positions.len() < count {
        let lesson = rng.below(app.lessons.len());
        let index = rng.below(app.lessons[lesson].entries.len());
        if app.lessons[lesson].entries[index].language == Language::Latin {
            positions.push((lesson, index));
        }
    }
    positions
}

fn profile(project: project::Project) {
    let entries: usize = project
        .lessons
        .iter()
        .map(|lesson| lesson.entries.len())
        .sum();
    let choices: usize = project
        .lessons
        .iter()
        .flat_map(|lesson| &lesson.entries)
        .map(|entry| entry.lemmas.len())
        .sum();
    eprintln!("entries {entries}, lemma choices {choices}");

    let cold = time("index, cold cache", || {
        word_index::WordIndex::from_project(&Default::default(), &project.lessons)
    });
    let cache = time("cache snapshot", || cold.persisted_cache());
    drop(cold);
    eprintln!("cached spellings {}", cache.words.len());
    let mut project = project;
    project.word_cache = cache;
    let mut app = App::default();
    time("open project (warm cache)", || {
        app.replace_project(project, None)
    });

    let ctx = egui::Context::default();
    configure_fonts(&ctx);
    for lesson in &mut app.lessons[..3] {
        lesson.expanded = true;
    }
    time("first frame", || render(&mut app, &ctx, 0.0));
    repeat("frame", 30, |run| {
        render(&mut app, &ctx, 1.0 + run as f64 / 60.0)
    });

    repeat("full index rebuild (structural edit)", 3, |_| {
        app.word_index.rebuild(&app.lessons)
    });

    let positions = sample_entries(&app, 500);
    repeat("edit one entry", positions.len(), |run| {
        let (lesson, index) = positions[run];
        let before = app.lessons[lesson].entries[index].text.clone();
        let after = format!("{before} puella");
        app.lessons[lesson].entries[index].text = after.clone();
        app.record_text(TextTarget::Entry(lesson, index), before, after);
    });
    repeat("undo an edit", 200, |_| app.undo());

    let positions = sample_entries(&app, 200);
    repeat("auto-macronize one entry", positions.len(), |run| {
        let (lesson, index) = positions[run];
        app.parsing_review = None;
        app.entry_action(lesson, index, EntryAction::AutoMacronize);
    });
    app.parsing_review = None;
    repeat("apply a review, undo, redo", positions.len(), |run| {
        let (lesson, index) = positions[run];
        let text = app.lessons[lesson].entries[index].text.clone();
        let words = app.word_index.tag_text(&collo::Lemmatizer::new(), &text);
        let review = parsing_review::Review::from_tagged_words(lesson, index, text, words);
        app.apply_review(&review, review.preview());
        app.undo();
        app.redo();
    });

    for lemma in ["sum", "et", "puella"] {
        repeat(&format!("lemma search \"{lemma}\""), 20, |_| {
            std::hint::black_box(app.word_index.lemma_occurrences(lemma, false));
        });
    }
    repeat("new lemmas in a lesson", 200, |run| {
        std::hint::black_box(app.word_index.new_lemmas_in_lesson(run % app.lessons.len()));
    });

    let out = dir().join("profile-save.loqr");
    time("save project", || {
        app.project_snapshot_for_save().save(&out).unwrap()
    });
    let reloaded = time("load saved project", || {
        project::Project::load(&out).unwrap()
    });
    drop(reloaded);
    eprintln!("final rss {}", rss());
}

#[test]
#[ignore = "profiles the generated fixture"]
fn profile_million_words() {
    let project = time("load fixture", || {
        project::Project::load(&fixture()).unwrap()
    });
    profile(project);
}

/// Choose a lemma for every word with readings from several lemmas: the
/// least frequent, so the choice differs from the default, or with
/// `most_frequent` the one the index would select anyway.
fn choose_all_lemmas(project: &mut project::Project, most_frequent: bool) {
    let analyzer = collo::Lemmatizer::new();
    let mut chosen =
        std::collections::HashMap::<(String, bool), Option<&'static collo::Lemma>>::new();
    for entry in project
        .lessons
        .iter_mut()
        .flat_map(|lesson| &mut lesson.entries)
    {
        if entry.language != Language::Latin {
            continue;
        }
        let mut occurrences = word_index::OccurrenceCounter::new();
        for token in collo::tagging::tokenize(&entry.text) {
            let text = &entry.text[token.range.clone()];
            let (key, occurrence) = occurrences.next(text);
            let lemma = *chosen
                .entry((text.to_owned(), token.sentence_start))
                .or_insert_with(|| {
                    let analyses = analyzer.analyze(text, token.sentence_start);
                    let first = analyses.first()?.lemma;
                    analyses
                        .iter()
                        .any(|analysis| analysis.lemma.key != first.key)
                        .then(|| {
                            analyses
                                .iter()
                                .map(|analysis| analysis.lemma)
                                .min_by_key(|lemma| {
                                    if most_frequent {
                                        u32::MAX - lemma.frequency
                                    } else {
                                        lemma.frequency
                                    }
                                })
                        })
                        .flatten()
                });
            if let Some(lemma) = lemma {
                entry.lemmas.push(word_index::LemmaChoice {
                    token: key,
                    occurrence,
                    lemma_key: lemma.key.to_owned(),
                    extension: lemma.extension,
                });
            }
        }
    }
}

#[test]
#[ignore = "profiles the generated fixture"]
fn profile_million_words_all_lemmas_chosen() {
    let mut project = time("load fixture", || {
        project::Project::load(&fixture()).unwrap()
    });
    time("choose a lemma for every ambiguous word", || {
        choose_all_lemmas(&mut project, false)
    });
    profile(project);
}

/// Repeated rebuilds. LOQUARIS_PROFILE_CHOICES=rare or =default chooses
/// lemmas first.
#[test]
#[ignore = "profiles the generated fixture"]
fn profile_rebuilds() {
    let mut project = project::Project::load(&fixture()).unwrap();
    match std::env::var("LOQUARIS_PROFILE_CHOICES").as_deref() {
        Ok("rare") => choose_all_lemmas(&mut project, false),
        Ok("default") => choose_all_lemmas(&mut project, true),
        _ => {}
    }
    let mut index = word_index::WordIndex::default();
    index.rebuild(&project.lessons);
    eprintln!(
        "first-use labels in the first 3 lessons: {}",
        (0..3)
            .map(|lesson| index.new_lemmas_in_lesson(lesson).len())
            .sum::<usize>()
    );
    let runs = std::env::var("LOQUARIS_PROFILE_RUNS")
        .ok()
        .and_then(|runs| runs.parse().ok())
        .unwrap_or(8);
    repeat("full index rebuild", runs, |_| {
        index.rebuild(&project.lessons)
    });
}
