//! The script data model shared by the editor, persistence, and exporters.
use crate::{audio, project};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Participant {
    Teacher,
    Student,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Language {
    L1,
    Latin,
}

impl Language {
    pub fn toggled(self) -> Self {
        match self {
            Language::L1 => Language::Latin,
            Language::Latin => Language::L1,
        }
    }
}

/// The part of a clip that is played and exported, in frames with `end`
/// exclusive. The whole clip is kept, so a crop can be widened again.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Trim {
    pub start: u32,
    pub end: u32,
}

/// An entry's clip together with its crop.
pub type AudioState = (Option<std::sync::Arc<[u8]>>, Option<Trim>);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub participant: Participant,
    pub language: Language,
    pub text: String,
    /// WAV bytes; shared cheaply across undo history snapshots.
    pub audio: Option<std::sync::Arc<[u8]>>,
    /// The selected part of `audio`; `None` means all of it.
    pub trim: Option<Trim>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Lesson {
    pub name: String,
    pub entries: Vec<Entry>,
    pub expanded: bool,
}

impl Lesson {
    /// An expanded lesson with no name or entries.
    pub fn empty() -> Self {
        Self {
            name: String::new(),
            entries: Vec::new(),
            expanded: true,
        }
    }

    /// The name shown in labels, substituting a placeholder when unnamed.
    pub fn display_name(&self) -> &str {
        display_name(&self.name)
    }
}

pub fn display_name(name: &str) -> &str {
    if name.is_empty() { "Untitled" } else { name }
}

/// Total lesson duration including gaps, or a notice if any clip is missing.
pub fn lesson_recording_summary(lesson: &Lesson, gaps: project::AudioGaps) -> String {
    if lesson.entries.is_empty() {
        return "Recordings incomplete".into();
    }
    let duration = lesson.entries.iter().try_fold(0.0, |sum, entry| {
        let clip = entry.audio.as_deref()?;
        Some(sum + audio::selected_duration(clip, entry.trim).ok()?)
    });
    let Some(duration) = duration else {
        return "Recordings incomplete".into();
    };
    let total = duration
        + lesson
            .entries
            .windows(2)
            .map(|pair| f64::from(gaps.between(pair[0].participant, pair[1].participant)) / 1000.0)
            .sum::<f64>();
    let tenths = (total * 10.0).round() as u64;
    let seconds = tenths / 10;
    if seconds >= 3600 {
        format!(
            "{}:{:02}:{:02}.{}",
            seconds / 3600,
            seconds / 60 % 60,
            seconds % 60,
            tenths % 10
        )
    } else {
        format!("{:02}:{:02}.{}", seconds / 60, seconds % 60, tenths % 10)
    }
}
