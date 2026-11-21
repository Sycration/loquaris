//! A plain snapshot of a project, read from or written to the collaborative
//! document. Lessons and entries are in script order.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Participant {
    Teacher,
    Student,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    L1,
    Latin,
}

/// An uploaded clip, stored immutably under the SHA-256 of its WAV bytes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioRef {
    /// Lowercase hex SHA-256 of the WAV file.
    pub blob: String,
    pub sample_rate: u32,
    pub frames: u32,
    pub channels: u16,
}

impl AudioRef {
    pub fn duration(&self, trim: Option<Trim>) -> f64 {
        let frames = trim.map_or(self.frames, |trim| trim.end - trim.start);
        f64::from(frames) / f64::from(self.sample_rate.max(1))
    }
}

/// The part of a clip that is played and exported, in frames with `end`
/// exclusive. The whole clip is kept, so a crop can be widened again.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Trim {
    pub start: u32,
    pub end: u32,
}

/// The lemma chosen in parsing review for one occurrence of a word in an
/// entry: the `occurrence`th (from zero) word in the entry spelled `token`.
/// Counting only words of the same spelling keeps the choice attached while
/// other words are edited, and one spelling can be different words in the
/// same entry: dum bellum geritur, pilum bellum feret miles.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LemmaChoice {
    /// collo-normalized, lowercased spelling, so choices survive
    /// macronization and case changes.
    pub token: String,
    pub occurrence: u32,
    pub lemma_key: String,
    pub extension: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub id: String,
    pub participant: Participant,
    pub language: Language,
    pub text: String,
    pub audio: Option<AudioRef>,
    /// The selected part of `audio`; `None` means all of it.
    pub trim: Option<Trim>,
    pub lemmas: Vec<LemmaChoice>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lesson {
    pub id: String,
    pub name: String,
    pub entries: Vec<Entry>,
}

impl Lesson {
    /// The name shown in labels, substituting a placeholder when unnamed.
    pub fn display_name(&self) -> &str {
        display_name(&self.name)
    }
}

pub fn display_name(name: &str) -> &str {
    if name.is_empty() { "Untitled" } else { name }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioGaps {
    pub same_participant_ms: u32,
    pub different_participant_ms: u32,
}

impl Default for AudioGaps {
    fn default() -> Self {
        Self {
            same_participant_ms: 100,
            different_participant_ms: 500,
        }
    }
}

impl AudioGaps {
    pub const MAX_MS: u32 = 60_000;

    pub fn between(self, a: Participant, b: Participant) -> u32 {
        if a == b {
            self.same_participant_ms
        } else {
            self.different_participant_ms
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Paper {
    #[default]
    A4,
    Letter,
    Legal,
    A3,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PdfSettings {
    pub paper: Paper,
    /// Body text size in points, 8–30.
    pub font_size: f32,
}

impl Default for PdfSettings {
    fn default() -> Self {
        Self {
            paper: Paper::A4,
            font_size: 15.0,
        }
    }
}

impl PdfSettings {
    pub const MIN_FONT_SIZE: f32 = 8.0;
    pub const MAX_FONT_SIZE: f32 = 30.0;
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub name: String,
    /// The learner's own language, e.g. "English".
    pub l1_name: String,
    pub gaps: AudioGaps,
    pub pdf: PdfSettings,
    pub lessons: Vec<Lesson>,
}

/// Total lesson duration including gaps, or a notice if any clip is missing.
pub fn lesson_recording_summary(lesson: &Lesson, gaps: AudioGaps) -> String {
    if lesson.entries.is_empty() {
        return "Recordings incomplete".into();
    }
    let duration = lesson.entries.iter().try_fold(0.0, |sum, entry| {
        Some(sum + entry.audio.as_ref()?.duration(entry.trim))
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
