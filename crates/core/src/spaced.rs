//! Spaced repetition flags: Latin words a diligent learner would be due to
//! review but that the script does not bring back in time. See "Spaced
//! repetition flags" in `docs/DESIGN.md`.
//!
//! The simulated learner hears lesson *k* at day *k* and treats every use of
//! a lemma as a review on Anki's default schedule. One pass over the script's
//! lemmas, in script order, finds the lemmas whose deadline passed unheard.
use super::*;

/// First learning step: the interval after a lemma's first occurrence, and
/// after a lapse.
pub const LEARNING_STEP_1: f64 = 60.0;
/// Second learning step (seconds).
pub const LEARNING_STEP_2: f64 = 600.0;
/// The interval on graduating, after the second step (seconds).
pub const GRADUATING_INTERVAL: f64 = 86_400.0;
/// Each review after graduating multiplies the interval by this.
pub const EASE: f64 = 2.5;
/// How long an entry without a recording lasts (seconds).
pub const DEFAULT_ENTRY_SECONDS: f64 = 10.0;
/// Lesson *k* starts this many seconds after the course starts, times *k*.
pub const LESSON_SPACING: f64 = 86_400.0;
/// An occurrence earlier than this fraction of the interval is only a
/// reminder: the clock restarts but the interval stays.
pub const REVIEW_FRACTION: f64 = 0.5;
/// An occurrence later than this many intervals means the word was forgotten.
pub const LAPSE_FACTOR: f64 = 2.0;

/// Where a lemma is flagged.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReviewKind {
    /// Due inside the lesson where it was last heard: the first entry
    /// starting at or after the due time.
    InLesson { lesson_id: String, entry_id: String },
    /// Due beyond that lesson's end: flagged on the first lesson ending at or
    /// after the due time, as a whole.
    Lesson { lesson_id: String },
}

/// One word due for review that the script does not bring back in time.
#[derive(Clone, Debug)]
pub struct ReviewFlag {
    pub lemma: LemmaInfo,
    pub kind: ReviewKind,
    pub last_heard_entry_id: String,
    pub last_heard_lesson_id: String,
    /// Seconds from the last occurrence to the flagged moment (the flagged
    /// entry's start, or the flagged lesson's end).
    pub elapsed_seconds: f64,
    /// The interval that was due after the last occurrence.
    pub interval_seconds: f64,
}

/// Where the flag goes, with the time by which the word must be heard.
#[derive(Clone, Copy)]
enum Target {
    /// Index into the flat entry list.
    Entry(usize),
    Lesson(usize),
}

struct State {
    last: f64,
    last_pos: usize,
    last_lesson: usize,
    interval: f64,
    /// When the lemma is due, before relaxing it to a deadline.
    due: f64,
}

fn next_interval(interval: f64) -> f64 {
    if interval < LEARNING_STEP_2 {
        LEARNING_STEP_2
    } else if interval < GRADUATING_INTERVAL {
        GRADUATING_INTERVAL
    } else {
        interval * EASE
    }
}

impl WordIndex {
    /// The review flags for the whole script, in script order (a lesson's
    /// flags on entries first, in entry order, then its lesson flags).
    pub fn spaced_repetition(&self) -> Vec<ReviewFlag> {
        // Timeline: start time of every entry, flat in script order.
        let mut flat: Vec<u32> = Vec::with_capacity(self.script.len());
        let mut starts: Vec<f64> = Vec::with_capacity(self.script.len());
        let mut lesson_of: Vec<usize> = Vec::with_capacity(self.script.len());
        // Per lesson: its entries' range in `flat` and its end time.
        let mut ranges: Vec<Range<usize>> = Vec::with_capacity(self.lessons.len());
        let mut ends: Vec<f64> = Vec::with_capacity(self.lessons.len());
        for (k, lesson) in self.lessons.iter().enumerate() {
            let begin = flat.len();
            let day = k as f64 * LESSON_SPACING;
            let mut time = day;
            let mut end = day;
            let mut previous: Option<&EntrySlot> = None;
            for &slot in &lesson.entries {
                let record = &self.slots[slot as usize];
                if let Some(previous) = previous {
                    time = end
                        + f64::from(self.gaps.between(previous.participant, record.participant))
                            / 1000.0;
                }
                let duration = record
                    .duration
                    .filter(|d| d.is_finite())
                    .map_or(DEFAULT_ENTRY_SECONDS, |d| d.max(0.0));
                flat.push(slot);
                starts.push(time);
                lesson_of.push(k);
                end = time + duration;
                previous = Some(record);
            }
            // Keep ends non-decreasing so the first lesson ending after a
            // time can be found by bisection.
            if let Some(&before) = ends.last() {
                end = end.max(before);
            }
            ranges.push(begin..flat.len());
            ends.push(end);
        }

        // Where a lemma last heard at `last_pos` is flagged once due.
        let target_for = |last_lesson: usize, due: f64| -> Option<(Target, f64)> {
            let range = &ranges[last_lesson];
            if due <= ends[last_lesson] {
                let within = &starts[range.clone()];
                let at = within.partition_point(|&start| start < due);
                if at < within.len() {
                    let pos = range.start + at;
                    return Some((Target::Entry(pos), starts[pos]));
                }
            }
            let from = last_lesson;
            let at = from + ends[from..].partition_point(|&end| end < due);
            (at < ends.len()).then(|| (Target::Lesson(at), ends[at]))
        };

        let mut states: Vec<Option<State>> = Vec::new();
        states.resize_with(self.lemmas.len(), || None);
        let mut flags: Vec<(usize, usize, ReviewFlag)> = Vec::new();
        let flag = |lemma: u32, state: &State, flags: &mut Vec<_>| {
            let Some((target, _)) = target_for(state.last_lesson, state.due) else {
                return;
            };
            let (kind, at, sort, lesson) = match target {
                Target::Entry(pos) => {
                    let lesson = lesson_of[pos];
                    (
                        ReviewKind::InLesson {
                            lesson_id: self.lessons[lesson].id.to_string(),
                            entry_id: self.slots[flat[pos] as usize].id.to_string(),
                        },
                        starts[pos],
                        pos,
                        lesson,
                    )
                }
                Target::Lesson(lesson) => (
                    ReviewKind::Lesson {
                        lesson_id: self.lessons[lesson].id.to_string(),
                    },
                    ends[lesson],
                    usize::MAX,
                    lesson,
                ),
            };
            flags.push((
                lesson,
                sort,
                ReviewFlag {
                    lemma: self.lemma_info(self.lemmas[lemma as usize]),
                    kind,
                    last_heard_entry_id: self.slots[flat[state.last_pos] as usize].id.to_string(),
                    last_heard_lesson_id: self.lessons[state.last_lesson].id.to_string(),
                    elapsed_seconds: at - state.last,
                    interval_seconds: state.interval,
                },
            ));
        };

        for (pos, &slot) in flat.iter().enumerate() {
            let time = starts[pos];
            let lesson = lesson_of[pos];
            for &lemma in &self.slots[slot as usize].selected_lemmas {
                let entry = &mut states[lemma as usize];
                let interval = match entry {
                    None => LEARNING_STEP_1,
                    Some(state) => {
                        // The deadline is never before the due time, so only
                        // a later occurrence needs it worked out.
                        if time > state.due
                            && target_for(state.last_lesson, state.due)
                                .is_some_and(|(_, deadline)| time > deadline)
                        {
                            flag(lemma, state, &mut flags);
                        }
                        let elapsed = time - state.last;
                        if elapsed < state.interval * REVIEW_FRACTION {
                            state.interval
                        } else if elapsed > state.interval * LAPSE_FACTOR {
                            LEARNING_STEP_1
                        } else {
                            next_interval(state.interval)
                        }
                    }
                };
                *entry = Some(State {
                    last: time,
                    last_pos: pos,
                    last_lesson: lesson,
                    interval,
                    due: time + interval,
                });
            }
        }
        for (lemma, state) in states.iter().enumerate() {
            if let Some(state) = state {
                flag(lemma as u32, state, &mut flags);
            }
        }
        flags.sort_by_key(|&(lesson, sort, _)| (lesson, sort));
        flags.into_iter().map(|(_, _, flag)| flag).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::*;
    use crate::{Participant::*, ProjectContent};

    const LATIN: Language = Language::Latin;
    const L1: Language = Language::L1;
    /// Filler lines that last 10 s each and contain no Latin words.
    const FILL: Line<'static> = (Teacher, L1, "filler");

    fn lines(items: &[&str]) -> Vec<Line<'static>> {
        items
            .iter()
            .map(|&text| match text {
                "." => FILL,
                text => (
                    Teacher,
                    LATIN,
                    Box::leak(text.to_string().into_boxed_str()) as &str,
                ),
            })
            .collect()
    }

    fn index_of(lessons: &[&[&str]]) -> WordIndex {
        let built: Vec<Vec<Line>> = lessons.iter().map(|lesson| lines(lesson)).collect();
        let refs: Vec<&[Line]> = built.iter().map(Vec::as_slice).collect();
        index_with(project(&refs))
    }

    fn index_with(content: ProjectContent) -> WordIndex {
        let mut index = WordIndex::new();
        // Zero gaps keep the arithmetic simple unless a test sets them.
        index.set_gaps(AudioGaps {
            same_participant_ms: 0,
            different_participant_ms: 0,
        });
        index.set_project(content);
        index
    }

    fn headwords(flags: &[ReviewFlag]) -> Vec<String> {
        flags.iter().map(|f| f.lemma.headword.clone()).collect()
    }

    fn rows(n: usize, word: &str) -> Vec<&str> {
        let mut v = vec![word];
        v.extend(std::iter::repeat_n(".", n));
        v
    }

    #[test]
    fn first_occurrence_is_due_after_one_minute_and_flags_the_first_entry_at_or_after() {
        // rosa at 0 s, due 60 s: entry 6 starts at exactly 60 s.
        let lesson = rows(8, "rosa");
        let flags = index_of(&[&lesson]).spaced_repetition();
        assert_eq!(flags.len(), 1);
        assert_eq!(headwords(&flags), ["rosa"]);
        assert_eq!(
            flags[0].kind,
            ReviewKind::InLesson {
                lesson_id: lesson_id(0),
                entry_id: entry_id(0, 6)
            }
        );
        assert_eq!(flags[0].elapsed_seconds, 60.0);
        assert_eq!(flags[0].interval_seconds, 60.0);
        assert_eq!(flags[0].last_heard_entry_id, entry_id(0, 0));
        assert_eq!(flags[0].last_heard_lesson_id, lesson_id(0));
        assert!(flags[0].lemma.translation.unwrap().starts_with("rose"));
    }

    #[test]
    fn due_time_between_entries_flags_the_next_entry() {
        let mut lesson = vec!["."];
        lesson.push("rosa"); // starts at 10 s, due at 70 s
        lesson.extend(std::iter::repeat_n(".", 8));
        let flags = index_of(&[&lesson]).spaced_repetition();
        assert_eq!(
            flags[0].kind,
            ReviewKind::InLesson {
                lesson_id: lesson_id(0),
                entry_id: entry_id(0, 7)
            }
        );
        assert_eq!(flags[0].elapsed_seconds, 60.0);
    }

    #[test]
    fn no_flag_before_the_deadline_or_when_it_is_past_the_script_end() {
        // 5 fillers: script ends at 60 s, entry starting at/after 60 s
        // doesn't exist, but the lesson ends at 60 s, so it is a lesson flag.
        let lesson = rows(5, "rosa");
        let flags = index_of(&[&lesson]).spaced_repetition();
        assert_eq!(flags.len(), 1);
        assert_eq!(
            flags[0].kind,
            ReviewKind::Lesson {
                lesson_id: lesson_id(0)
            }
        );
        // One entry shorter: the lesson ends at 50 s, before the due time.
        let lesson = rows(4, "rosa");
        assert!(index_of(&[&lesson]).spaced_repetition().is_empty());
    }

    #[test]
    fn early_occurrence_is_a_reminder_that_keeps_the_interval() {
        // rosa at 0 and at 20 s (< 30 s): clock restarts, interval stays 60.
        // New due at 80 s: entry 8.
        let mut lesson = vec!["rosa", ".", "rosa"];
        lesson.extend(std::iter::repeat_n(".", 8));
        let flags = index_of(&[&lesson]).spaced_repetition();
        assert_eq!(flags.len(), 1);
        assert_eq!(flags[0].interval_seconds, 60.0);
        assert_eq!(flags[0].last_heard_entry_id, entry_id(0, 2));
        assert_eq!(
            flags[0].kind,
            ReviewKind::InLesson {
                lesson_id: lesson_id(0),
                entry_id: entry_id(0, 8)
            }
        );
        assert_eq!(flags[0].elapsed_seconds, 60.0);
    }

    #[test]
    fn occurrence_at_half_the_interval_is_a_review() {
        // rosa at 0 and 30 s: review, interval 600 s, due at 630 s.
        let mut lesson = vec!["rosa", ".", ".", "rosa"];
        lesson.extend(std::iter::repeat_n(".", 70));
        let flags = index_of(&[&lesson]).spaced_repetition();
        assert_eq!(flags.len(), 1);
        assert_eq!(flags[0].interval_seconds, 600.0);
        // 30 + 600 = 630 s -> entry 63
        assert_eq!(
            flags[0].kind,
            ReviewKind::InLesson {
                lesson_id: lesson_id(0),
                entry_id: entry_id(0, 63)
            }
        );
    }

    #[test]
    fn second_step_graduates_to_a_day_and_reviews_multiply_by_ease() {
        // Lesson 0: rosa at 0 s and 30 s (review: interval 600, due 630 s).
        // The due time is relaxed to the end of lesson 1, the first lesson
        // ending after it; rosa is absent there, so lesson 1 is flagged.
        let l0 = ["rosa", ".", ".", "rosa"];
        let flags = index_of(&[&l0, &["."]]).spaced_repetition();
        assert_eq!(flags.len(), 1);
        assert_eq!(
            flags[0].kind,
            ReviewKind::Lesson {
                lesson_id: lesson_id(1)
            }
        );
        assert_eq!(flags[0].interval_seconds, 600.0);
        // Heard anywhere in lesson 1 (after 86400 s > 2 x 600 s: a lapse)
        // satisfies the relaxed deadline.
        assert!(
            index_of(&[&l0, &[".", "rosa"]])
                .spaced_repetition()
                .is_empty()
        );

        // Graduation: heard at 0 s, 30 s (->600 s), and 630 s (->1 day).
        let mut l0 = vec!["rosa", ".", ".", "rosa"];
        l0.extend(std::iter::repeat_n(".", 59)); // starts at 30 s + 59*10 = 620 s
        l0.push("rosa"); // 630 s
        let l1 = ["rosa"]; // at 1 day: 86400 - 630 elapsed, within [43200, 172800]
        let l2 = ["."];
        let l3 = ["rosa"]; // day 3
        let flags = index_of(&[&l0, &l1, &l2, &l3]).spaced_repetition();
        // After the review in lesson 1 the interval is 1 day * 2.5 = 216000 s
        // = 2.5 days: due at day 3.5, relaxed to the end of lesson 3, where
        // rosa is heard (day 3 + 0 s). Heard 2 days after: elapsed 172800 s,
        // < 2*216000, at least half (108000): review, interval 540000 s.
        // Due at day 3 + 6.25 days, past the script end: no flags.
        assert!(flags.is_empty(), "{:?}", headwords(&flags));
    }

    #[test]
    fn graduation_interval_is_one_day_then_times_two_and_a_half() {
        assert_eq!(next_interval(LEARNING_STEP_1), LEARNING_STEP_2);
        assert_eq!(next_interval(LEARNING_STEP_2), GRADUATING_INTERVAL);
        assert_eq!(next_interval(GRADUATING_INTERVAL), 216_000.0);
        assert_eq!(next_interval(216_000.0), 540_000.0);
    }

    #[test]
    fn interval_of_a_day_is_seen_in_flags() {
        // 0 s -> 30 s review (600) -> 630 s review (1 day) then never again
        // heard: due at 630 s + 1 day, relaxed to the end of lesson 1.
        let mut l0 = vec!["rosa", ".", ".", "rosa"];
        l0.extend(std::iter::repeat_n(".", 59));
        l0.push("rosa");
        let flags = index_of(&[&l0, &["."], &["."]]).spaced_repetition();
        assert_eq!(flags.len(), 1);
        assert_eq!(flags[0].interval_seconds, GRADUATING_INTERVAL);
        // Due at 87030 s: lesson 1 starts at 86400 and ends at 86410, so
        // lesson 2 (172800 + 10) is the first ending at or after.
        assert_eq!(
            flags[0].kind,
            ReviewKind::Lesson {
                lesson_id: lesson_id(2)
            }
        );
    }

    #[test]
    fn day_interval_lands_in_the_next_lessons_flag_not_a_minute_of_it() {
        // Heard in lesson 0 only at 0 s (interval 60): due 60 s, in the
        // lesson (ends 60 s because 6 entries) -> lesson 0 flag when the
        // lesson ends at/after due. After a graduating review the relaxed
        // deadline is the end of lesson 1.
        let mut l0 = vec!["rosa", ".", ".", "rosa"];
        l0.extend(std::iter::repeat_n(".", 59));
        l0.push("rosa");
        let l1 = ["."];
        let l2 = ["rosa"];
        let flags = index_of(&[&l0, &l1, &l2]).spaced_repetition();
        // Due at 87030 s, lesson 1 ends at 86410 (< due), lesson 2 ends at
        // 172810 >= due: relaxed to lesson 2, where rosa is heard at its
        // start (172800 <= 172810): not flagged.
        assert!(flags.is_empty(), "{:?}", headwords(&flags));
    }

    #[test]
    fn lapse_after_more_than_twice_the_interval_restarts_at_one_minute() {
        // rosa at 0 s, again at 130 s (> 120 s): lapse, interval stays 60 s
        // (not 600). Flag for the first deadline fires at entry 6 (60 s),
        // then again after the lapse occurrence.
        let mut lesson = vec!["rosa"];
        lesson.extend(std::iter::repeat_n(".", 12));
        lesson.push("rosa"); // 130 s
        lesson.extend(std::iter::repeat_n(".", 8));
        let flags = index_of(&[&lesson]).spaced_repetition();
        assert_eq!(flags.len(), 2);
        assert_eq!(flags[0].elapsed_seconds, 60.0);
        assert_eq!(
            flags[0].kind,
            ReviewKind::InLesson {
                lesson_id: lesson_id(0),
                entry_id: entry_id(0, 6)
            }
        );
        assert_eq!(flags[1].interval_seconds, 60.0);
        assert_eq!(flags[1].last_heard_entry_id, entry_id(0, 13));
        assert_eq!(flags[1].elapsed_seconds, 60.0);
    }

    #[test]
    fn late_occurrence_within_twice_the_interval_still_counts_as_a_review() {
        // rosa at 0 and 100 s (<= 120 s): review -> 600 s; flagged once for
        // the missed first deadline only.
        let mut lesson = vec!["rosa"];
        lesson.extend(std::iter::repeat_n(".", 9));
        lesson.push("rosa");
        lesson.extend(std::iter::repeat_n(".", 5));
        let flags = index_of(&[&lesson]).spaced_repetition();
        assert_eq!(flags.len(), 1);
        assert_eq!(flags[0].last_heard_entry_id, entry_id(0, 0));
        assert_eq!(flags[0].interval_seconds, 60.0);
        // After the review at 100 s the interval is 600: due at 700 s,
        // past the script end (lesson ends at 160 s).
    }

    #[test]
    fn one_flag_until_heard_again() {
        let lesson = rows(30, "rosa");
        let flags = index_of(&[&lesson]).spaced_repetition();
        assert_eq!(flags.len(), 1);
    }

    #[test]
    fn a_word_missing_from_the_next_lesson_flags_that_lesson() {
        // Learned properly in lesson 0 (reviewed to the day interval), then
        // absent: relaxed to the end of lesson 2 (first lesson ending at or
        // after 1 day + 630 s).
        let mut l0 = vec!["rosa", ".", ".", "rosa"];
        l0.extend(std::iter::repeat_n(".", 59));
        l0.push("rosa");
        let flags = index_of(&[&l0, &["puella"], &["puella"]]).spaced_repetition();
        let rosa: Vec<_> = flags
            .iter()
            .filter(|f| f.lemma.headword == "rosa")
            .collect();
        assert_eq!(rosa.len(), 1);
        assert_eq!(
            rosa[0].kind,
            ReviewKind::Lesson {
                lesson_id: lesson_id(2)
            }
        );
        assert_eq!(rosa[0].last_heard_lesson_id, lesson_id(0));
    }

    #[test]
    fn deadline_past_the_script_end_is_not_flagged() {
        let mut l0 = vec!["rosa", ".", ".", "rosa"];
        l0.extend(std::iter::repeat_n(".", 59));
        l0.push("rosa");
        // Due at 87030 s but the script ends at day 1 + 10 s.
        assert!(index_of(&[&l0, &["."]]).spaced_repetition().is_empty());
    }

    #[test]
    fn durations_come_from_clips_and_default_to_ten_seconds() {
        let mut content = project(&[&[
            (Teacher, LATIN, "rosa"),
            (Teacher, L1, "x"),
            (Teacher, L1, "y"),
        ]]);
        // 0: 30 s clip, 1: 40 s clip, 2: no clip -> third entry starts at
        // 70 s, after rosa's due time (60 s) -> flagged on entry 2.
        content.lessons[0].entries[0].duration = Some(30.0);
        content.lessons[0].entries[1].duration = Some(40.0);
        let flags = index_with(content).spaced_repetition();
        assert_eq!(
            flags[0].kind,
            ReviewKind::InLesson {
                lesson_id: lesson_id(0),
                entry_id: entry_id(0, 2)
            }
        );
        assert_eq!(flags[0].elapsed_seconds, 70.0);

        // Without recordings the same lines are 10 s each: lesson ends at
        // 30 s, before the due time.
        let content = project(&[&[
            (Teacher, LATIN, "rosa"),
            (Teacher, L1, "x"),
            (Teacher, L1, "y"),
        ]]);
        assert!(index_with(content).spaced_repetition().is_empty());
    }

    #[test]
    fn gaps_separate_consecutive_entries_by_participant() {
        let content = project(&[&[
            (Teacher, LATIN, "rosa"),
            (Student, L1, "a"),
            (Student, L1, "b"),
            (Teacher, L1, "c"),
            (Teacher, L1, "d"),
            (Teacher, L1, "e"),
            (Teacher, L1, "f"),
        ]]);
        let mut index = index_with(content);
        // Teacher->Student 5 s, student->student 1 s, student->teacher 5 s,
        // then teacher->teacher 1 s: starts 0, 15, 26, 41, 52, 63, 74.
        // Due 60 s: entry 5 (63 s).
        index.set_gaps(AudioGaps {
            same_participant_ms: 1000,
            different_participant_ms: 5000,
        });
        let flags = index.spaced_repetition();
        assert_eq!(
            flags[0].kind,
            ReviewKind::InLesson {
                lesson_id: lesson_id(0),
                entry_id: entry_id(0, 5)
            }
        );
        assert_eq!(flags[0].elapsed_seconds, 63.0);
    }

    #[test]
    fn default_gaps_apply_until_set() {
        let content = project(&[&[(Teacher, LATIN, "rosa"), (Student, L1, "a")]]);
        let mut index = WordIndex::new();
        index.set_project(content);
        assert_eq!(index.gaps, AudioGaps::default());
    }

    #[test]
    fn l1_entries_advance_time_but_contribute_no_words() {
        // "rosa" written in an L1 entry is not a Latin word.
        let content = project(&[&[
            (Teacher, L1, "rosa"),
            (Teacher, L1, "x"),
            (Teacher, L1, "y"),
            (Teacher, L1, "z"),
            (Teacher, L1, "w"),
            (Teacher, L1, "v"),
            (Teacher, L1, "u"),
        ]]);
        assert!(index_with(content).spaced_repetition().is_empty());
        // L1 lines in the middle still advance the clock for a Latin word.
        let flags = index_of(&[&rows(7, "rosa")]).spaced_repetition();
        assert_eq!(flags.len(), 1);
    }

    #[test]
    fn student_lines_count_like_teacher_lines() {
        let content = project(&[&[
            (Student, LATIN, "rosa"),
            (Teacher, LATIN, "rosa"),
            (Teacher, L1, "a"),
            (Teacher, L1, "b"),
            (Teacher, L1, "c"),
            (Teacher, L1, "d"),
            (Teacher, L1, "e"),
            (Teacher, L1, "f"),
            (Teacher, L1, "g"),
        ]]);
        let flags = index_with(content).spaced_repetition();
        // Heard at 0 and 10 s (reminder, interval 60): due at 70 s.
        assert_eq!(flags.len(), 1);
        assert_eq!(flags[0].last_heard_entry_id, entry_id(0, 1));
    }

    #[test]
    fn chosen_lemmas_are_respected() {
        let build = |choose: bool| {
            let mut lesson = rows(8, "bellum");
            lesson.truncate(9);
            let mut content = project(&[&lines(&lesson)]);
            if choose {
                content.lessons[0].entries[0].lemmas.push(LemmaChoice {
                    token: "bellum".into(),
                    occurrence: 0,
                    lemma_key: "bellus".into(),
                    extension: false,
                });
            }
            index_with(content).spaced_repetition()
        };
        let default = build(false);
        let chosen = build(true);
        assert_eq!(default.len(), 1);
        assert_eq!(chosen.len(), 1);
        assert_ne!(default[0].lemma.lemma.key, chosen[0].lemma.lemma.key);
        assert_eq!(chosen[0].lemma.lemma.key, "bellus");
    }

    #[test]
    fn entries_outside_the_structure_are_ignored() {
        let mut index = index_of(&[&rows(8, "rosa")]);
        index.set_entry(entry("stray", Teacher, LATIN, "puella"));
        assert_eq!(headwords(&index.spaced_repetition()), ["rosa"]);
    }

    #[test]
    fn edits_update_flags_and_duration_changes_do_not_retokenize() {
        let mut index = index_of(&[&rows(5, "rosa")]);
        assert_eq!(index.spaced_repetition().len(), 1); // lesson flag
        let before = index.cached_analysis_count();
        let mut e = entry(&entry_id(0, 1), Teacher, L1, "filler");
        e.duration = Some(1.0);
        index.set_entry(e);
        // Lesson now ends at 51 s, before the due time.
        assert!(index.spaced_repetition().is_empty());
        assert_eq!(index.cached_analysis_count(), before);
    }

    #[test]
    fn flags_are_in_script_order() {
        let l0 = [
            "rosa", "puella", ".", ".", ".", ".", ".", ".", ".", ".", ".",
        ];
        let flags = index_of(&[&l0]).spaced_repetition();
        assert_eq!(headwords(&flags), ["rosa", "puella"]);
    }

    #[test]
    fn entry_content_duration_is_optional_in_json() {
        let json = r#"{"id":"a","participant":"teacher","language":"latin","text":"rosa"}"#;
        let entry: EntryContent = serde_json::from_str(json).unwrap();
        assert_eq!(entry.duration, None);
        let json =
            r#"{"id":"a","participant":"teacher","language":"latin","text":"rosa","duration":2.5}"#;
        let entry: EntryContent = serde_json::from_str(json).unwrap();
        assert_eq!(entry.duration, Some(2.5));
    }
}
