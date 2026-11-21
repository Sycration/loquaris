//! Helpers for building small projects in tests.
use crate::{EntryContent, Language, LessonContent, LessonOutline, Participant, ProjectContent};

pub type Line<'a> = (Participant, Language, &'a str);

/// The ID of the `entry`th entry of the `lesson`th lesson as built by
/// [`project`]: "1.2" is the third entry of the second lesson.
pub fn entry_id(lesson: usize, entry: usize) -> String {
    format!("{lesson}.{entry}")
}

pub fn lesson_id(lesson: usize) -> String {
    format!("lesson {lesson}")
}

pub fn entry(id: &str, participant: Participant, language: Language, text: &str) -> EntryContent {
    EntryContent {
        id: id.into(),
        participant,
        language,
        text: text.into(),
        lemmas: Vec::new(),
        duration: None,
    }
}

pub fn project(lessons: &[&[Line]]) -> ProjectContent {
    ProjectContent {
        l1_name: "English".into(),
        lessons: lessons
            .iter()
            .enumerate()
            .map(|(lesson, entries)| LessonContent {
                id: lesson_id(lesson),
                entries: entries
                    .iter()
                    .enumerate()
                    .map(|(index, &(participant, language, text))| {
                        entry(&entry_id(lesson, index), participant, language, text)
                    })
                    .collect(),
            })
            .collect(),
    }
}

/// Teacher entries in one lesson.
pub fn teacher_lesson(texts: &[(&str, Language)]) -> ProjectContent {
    let lines: Vec<Line> = texts
        .iter()
        .map(|&(text, language)| (Participant::Teacher, language, text))
        .collect();
    project(&[&lines])
}

pub fn outline(lessons: &[(&str, &[&str])]) -> Vec<LessonOutline> {
    lessons
        .iter()
        .map(|(id, entries)| LessonOutline {
            id: (*id).into(),
            entry_ids: entries.iter().map(|&entry| entry.into()).collect(),
        })
        .collect()
}
