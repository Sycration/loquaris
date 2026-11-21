//! Rendering projects to PDF scripts and WAV audio, validating uploaded WAV
//! files, and reading and writing the desktop app's `.loqr` archives.
//!
//! Clip bytes are supplied by the caller through a loader, so this crate does
//! no storage I/O of its own.
use loquaris_core::{AudioRef, Lesson, Project};

pub mod loqr;
pub mod pdf;
pub mod wav;

/// Fetches the WAV bytes of a clip.
pub type ClipLoader<'a> = dyn FnMut(&AudioRef) -> Result<Vec<u8>, String> + 'a;

/// The lessons with the given IDs (all when `None`) in script order, each
/// with its number in the whole project, counted from one.
fn select_lessons<'a>(
    project: &'a Project,
    ids: Option<&[String]>,
) -> Result<Vec<(usize, &'a Lesson)>, String> {
    if let Some(ids) = ids
        && let Some(missing) = ids
            .iter()
            .find(|id| !project.lessons.iter().any(|lesson| lesson.id == **id))
    {
        return Err(format!("A selected lesson no longer exists ({missing})."));
    }
    Ok(project
        .lessons
        .iter()
        .enumerate()
        .filter(|(_, lesson)| ids.is_none_or(|ids| ids.contains(&lesson.id)))
        .map(|(index, lesson)| (index + 1, lesson))
        .collect())
}

#[cfg(test)]
mod tests {
    //! Builders shared by the module tests.
    use loquaris_core::{AudioGaps, Entry, Language, Lesson, Participant, PdfSettings, Project};

    pub fn project(lessons: Vec<Lesson>) -> Project {
        Project {
            name: "Course".into(),
            l1_name: "English".into(),
            gaps: AudioGaps::default(),
            pdf: PdfSettings::default(),
            lessons,
        }
    }

    pub fn lesson(name: &str, entries: Vec<Entry>) -> Lesson {
        Lesson {
            id: crate::loqr::new_id(),
            name: name.into(),
            entries,
        }
    }

    pub fn entry(participant: Participant, language: Language, text: &str) -> Entry {
        Entry {
            id: crate::loqr::new_id(),
            participant,
            language,
            text: text.into(),
            audio: None,
            trim: None,
            lemmas: Vec::new(),
        }
    }

    /// A 16-bit WAV file of `frames` frames, each sample from `sample`.
    pub fn wav(rate: u32, channels: u16, frames: u32, sample: impl Fn(u32, u16) -> i16) -> Vec<u8> {
        let mut data = std::io::Cursor::new(Vec::new());
        let spec = hound::WavSpec {
            channels,
            sample_rate: rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::new(&mut data, spec).unwrap();
        for frame in 0..frames {
            for channel in 0..channels {
                writer.write_sample(sample(frame, channel)).unwrap();
            }
        }
        writer.finalize().unwrap();
        data.into_inner()
    }

    #[test]
    fn select_lessons_filters_in_script_order() {
        let project = project(vec![
            lesson("A", vec![]),
            lesson("B", vec![]),
            lesson("C", vec![]),
        ]);
        let ids = [project.lessons[2].id.clone(), project.lessons[0].id.clone()];
        let selected = super::select_lessons(&project, Some(&ids)).unwrap();
        let names: Vec<_> = selected
            .iter()
            .map(|(number, lesson)| (*number, lesson.name.as_str()))
            .collect();
        assert_eq!(names, [(1, "A"), (3, "C")]);
        assert_eq!(super::select_lessons(&project, None).unwrap().len(), 3);
        assert!(super::select_lessons(&project, Some(&["x".into()])).is_err());
    }
}
