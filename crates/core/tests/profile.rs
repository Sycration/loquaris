//! Ignored profiling test on a generated project of about a million Latin
//! words, as the desktop app's profile.rs did:
//!
//! cargo test --release -p loquaris-core --test profile -- --ignored --nocapture
use std::time::Instant;

use loquaris_core::{
    EntryContent, Language, LessonContent, LessonOutline, Participant, ProjectContent,
    SearchOptions, WordIndex,
};
use unicode_normalization::{UnicodeNormalization, char::is_combining_mark};

const LESSONS: usize = 250;
const ENTRIES_PER_LESSON: usize = 800;

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

/// Lessons of Latin sentences from the 4000 most frequent lemmas, frequent
/// ones much more likely, as in real text; every fifth entry is L1.
fn generate() -> ProjectContent {
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
    let lessons = (0..LESSONS)
        .map(|lesson| LessonContent {
            id: format!("l{lesson}"),
            entries: (0..ENTRIES_PER_LESSON)
                .map(|index| {
                    let id = format!("l{lesson}e{index}");
                    if index % 5 == 0 {
                        return entry(
                            id,
                            Participant::Teacher,
                            Language::L1,
                            "How would you say this in Latin?".into(),
                        );
                    }
                    let count = 2 + rng.below(9);
                    let mut words = Vec::with_capacity(count);
                    for _ in 0..count {
                        let lemma = (forms.len() as f64 * rng.unit().powf(3.0)) as usize;
                        let options = &forms[lemma.min(forms.len() - 1)];
                        words.push(options[rng.below(options.len())].clone());
                    }
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
                    entry(id, participant, Language::Latin, text)
                })
                .collect(),
        })
        .collect();
    ProjectContent {
        l1_name: "English".into(),
        lessons,
    }
}

fn entry(id: String, participant: Participant, language: Language, text: String) -> EntryContent {
    EntryContent {
        id,
        participant,
        language,
        text,
        lemmas: Vec::new(),
        duration: None,
    }
}

fn time<T>(label: &str, action: impl FnOnce() -> T) -> T {
    let started = Instant::now();
    let result = action();
    eprintln!(
        "{label:<44} {:>10.2} ms",
        started.elapsed().as_secs_f64() * 1000.0
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

#[test]
#[ignore = "profiles a generated million-word project"]
fn profile_million_words() {
    let project = time("generate project", generate);
    let words: usize = project
        .lessons
        .iter()
        .flat_map(|lesson| &lesson.entries)
        .filter(|entry| entry.language == Language::Latin)
        .map(|entry| entry.text.split(' ').count())
        .sum();
    eprintln!("Latin words {words}");
    let mut index = WordIndex::new();
    time("set project, cold cache", || {
        index.set_project(project.clone())
    });
    eprintln!("cached spellings {}", index.cached_analysis_count());
    time("set project, warm cache", || {
        index.set_project(project.clone())
    });

    let mut outline: Vec<LessonOutline> = project
        .lessons
        .iter()
        .map(|lesson| LessonOutline {
            id: lesson.id.clone(),
            entry_ids: lesson
                .entries
                .iter()
                .map(|entry| entry.id.clone())
                .collect(),
        })
        .collect();
    let mut rng = Rng(42);
    repeat("set structure (move one entry)", 50, |_| {
        let from = rng.below(outline.len());
        let at = rng.below(outline[from].entry_ids.len());
        let moved = outline[from].entry_ids.remove(at);
        let to = rng.below(outline.len());
        let at = rng.below(outline[to].entry_ids.len() + 1);
        outline[to].entry_ids.insert(at, moved);
        index.set_structure(&outline);
    });

    let samples: Vec<_> = (0..500)
        .map(|_| {
            let lesson = &project.lessons[rng.below(LESSONS)];
            lesson.entries[1 + rng.below(ENTRIES_PER_LESSON - 1)].clone()
        })
        .filter(|entry| entry.language == Language::Latin)
        .collect();
    repeat("edit one entry", samples.len(), |run| {
        let mut edited = samples[run].clone();
        edited.text.push_str(" puella");
        index.set_entry(edited);
    });
    repeat("edit the first entry back and forth", 100, |run| {
        let mut first = project.lessons[0].entries[1].clone();
        if run % 2 == 0 {
            first.text = "Puer".into();
        }
        index.set_entry(first);
    });
    repeat("review one entry", 200, |run| {
        std::hint::black_box(index.review(&samples[run % samples.len()].id));
    });
    let options = SearchOptions::default();
    for query in ["sum", "et", "puella", "in the", "xyzzy"] {
        repeat(&format!("search \"{query}\""), 10, |_| {
            std::hint::black_box(index.search(query, &options));
        });
    }
    repeat("first uses of every entry", 20, |_| {
        std::hint::black_box(index.all_first_uses().count());
    });
    repeat("spaced repetition flags", 20, |_| {
        std::hint::black_box(index.spaced_repetition().len());
    });
    eprintln!(
        "spaced repetition flags raised {}",
        index.spaced_repetition().len()
    );
    repeat("new lemmas in a lesson", 200, |run| {
        std::hint::black_box(index.new_lemmas_in_lesson(&format!("l{}", run % LESSONS)));
    });
    repeat("remove an entry", 100, |run| {
        index.remove_entry(&samples[run % samples.len()].id);
    });
}
