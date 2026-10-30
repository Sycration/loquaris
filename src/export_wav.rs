//! Combine recorded entries into a 48 kHz, stereo, 16-bit WAV.
use std::{io::Cursor, path::Path};

use base64::{Engine, engine::general_purpose::STANDARD};

use crate::{Lesson, Participant};

const RATE: u32 = 48_000;

fn portable_stem(name: &str) -> String {
    let suggested = crate::project::suggested_file_name(name);
    if name
        .trim_matches(|c| c == ' ' || c == '.')
        .to_ascii_lowercase()
        .ends_with(".loqr")
    {
        return suggested;
    }
    suggested
        .strip_suffix(".loqr")
        .or_else(|| suggested.strip_suffix(".LOQR"))
        .unwrap_or(&suggested)
        .to_owned()
}

pub fn folder_name(project: &str) -> String {
    portable_stem(project)
}

pub fn save_lessons(
    directory: &Path,
    project: &str,
    lessons: &[Lesson],
    teacher: bool,
    student: bool,
    gaps: crate::project::AudioGaps,
) -> Result<(), String> {
    gaps.validate()?;
    if !complete(lessons) {
        return Err("Every lesson must be fully recorded before export.".into());
    }
    let mut names = std::collections::HashSet::new();
    let mut paths = Vec::new();
    for lesson in lessons {
        let name = format!(
            "{} - {}.wav",
            portable_stem(project),
            portable_stem(&lesson.name)
        );
        if !names.insert(name.to_lowercase()) {
            return Err(format!(
                "Lesson filenames conflict: {name}. Rename a lesson before exporting."
            ));
        }
        paths.push(directory.join(name));
    }
    std::fs::create_dir(directory).map_err(|e| {
        format!(
            "Could not create export folder {}: {e}",
            directory.display()
        )
    })?;
    for (lesson, path) in lessons.iter().zip(paths) {
        save(&path, std::slice::from_ref(lesson), teacher, student, gaps).map_err(|e| {
            format!(
                "{e} (Some files may already have been exported to {}.)",
                directory.display()
            )
        })?;
    }
    Ok(())
}

pub fn complete(lessons: &[Lesson]) -> bool {
    !lessons.is_empty()
        && lessons.iter().all(|lesson| {
            !lesson.entries.is_empty()
                && lesson.entries.iter().all(|entry| {
                    entry
                        .audio
                        .as_deref()
                        .is_some_and(|clip| crate::audio::clip_duration(clip).is_ok())
                })
        })
}

pub fn save(
    path: &Path,
    lessons: &[Lesson],
    teacher: bool,
    student: bool,
    gaps: crate::project::AudioGaps,
) -> Result<(), String> {
    gaps.validate()?;
    if !complete(lessons) {
        return Err("Every lesson must be fully recorded before export.".into());
    }
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    {
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: RATE,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer =
            hound::WavWriter::new(file.as_file_mut(), spec).map_err(|e| e.to_string())?;
        let mut previous = None;
        for lesson in lessons {
            for (index, entry) in lesson.entries.iter().enumerate() {
                let result = (|| -> Result<(), String> {
                    let data = STANDARD
                        .decode(entry.audio.as_ref().ok_or("Missing recording")?)
                        .map_err(|e| e.to_string())?;
                    let mut reader =
                        hound::WavReader::new(Cursor::new(data)).map_err(|e| e.to_string())?;
                    let spec = reader.spec();
                    if spec.channels == 0
                        || spec.sample_rate == 0
                        || spec.bits_per_sample == 0
                        || spec.bits_per_sample > 32
                    {
                        return Err("Invalid WAV format".into());
                    }
                    let samples: Vec<f32> = if spec.sample_format == hound::SampleFormat::Float {
                        reader
                            .samples::<f32>()
                            .collect::<Result<_, _>>()
                            .map_err(|e| e.to_string())?
                    } else {
                        let scale = (2f64).powi(i32::from(spec.bits_per_sample) - 1) as f32;
                        reader
                            .samples::<i32>()
                            .map(|s| s.map(|s| s as f32 / scale))
                            .collect::<Result<_, _>>()
                            .map_err(|e| e.to_string())?
                    };
                    let channels = usize::from(spec.channels);
                    if samples.is_empty()
                        || samples.len() % channels != 0
                        || samples.iter().any(|s| !s.is_finite())
                    {
                        return Err("Empty or invalid recording".into());
                    }
                    if let Some(participant) = previous {
                        let frames = u64::from(gaps.between(participant, entry.participant))
                            * u64::from(RATE)
                            / 1000;
                        for _ in 0..frames * 2 {
                            writer.write_sample(0i16).map_err(|e| e.to_string())?;
                        }
                    }
                    previous = Some(entry.participant);
                    let frames = samples.len() / channels;
                    let output_frames = (frames as u64 * u64::from(RATE)
                        + u64::from(spec.sample_rate) / 2)
                        / u64::from(spec.sample_rate);
                    let enabled = match entry.participant {
                        Participant::Teacher => teacher,
                        Participant::Student => student,
                    };
                    for frame in 0..output_frames {
                        let pos = frame as f64 * f64::from(spec.sample_rate) / f64::from(RATE);
                        let a = (pos as usize).min(frames - 1);
                        let b = (a + 1).min(frames - 1);
                        let fraction = (pos - a as f64) as f32;
                        for channel in 0..2 {
                            let mix = |frame: usize| {
                                if channels <= 2 {
                                    samples
                                        [frame * channels + if channels == 1 { 0 } else { channel }]
                                } else {
                                    let values = &samples[frame * channels..(frame + 1) * channels];
                                    let mut count = 0;
                                    let sum: f32 = values
                                        .iter()
                                        .skip(channel)
                                        .step_by(2)
                                        .map(|value| {
                                            count += 1;
                                            *value
                                        })
                                        .sum();
                                    sum / count as f32
                                }
                            };
                            let value = if enabled {
                                mix(a) * (1.0 - fraction) + mix(b) * fraction
                            } else {
                                0.0
                            };
                            writer
                                .write_sample((value.clamp(-1.0, 1.0) * 32767.0).round() as i16)
                                .map_err(|e| e.to_string())?;
                        }
                    }
                    Ok(())
                })();
                result
                    .map_err(|e| format!("Lesson ‘{}’, entry {}: {e}", lesson.name, index + 1))?;
            }
        }
        writer.finalize().map_err(|e| e.to_string())?;
    }
    file.persist(path)
        .map_err(|e| format!("Could not save {}: {}", path.display(), e.error))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Entry, Language};

    fn clip(rate: u32, channels: u16, value: i16) -> String {
        let mut data = Cursor::new(Vec::new());
        let spec = hound::WavSpec {
            channels,
            sample_rate: rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::new(&mut data, spec).unwrap();
        for _ in 0..rate * u32::from(channels) {
            writer.write_sample(value).unwrap();
        }
        writer.finalize().unwrap();
        STANDARD.encode(data.into_inner())
    }

    fn lessons() -> Vec<Lesson> {
        vec![
            Lesson {
                name: "One".into(),
                expanded: false,
                entries: vec![
                    Entry {
                        participant: Participant::Teacher,
                        language: Language::L1,
                        text: String::new(),
                        audio: Some(clip(8000, 1, 12000)),
                    },
                    Entry {
                        participant: Participant::Student,
                        language: Language::Latin,
                        text: String::new(),
                        audio: Some(clip(16000, 2, -12000)),
                    },
                ],
            },
            Lesson {
                name: "Two".into(),
                expanded: false,
                entries: vec![Entry {
                    participant: Participant::Teacher,
                    language: Language::Latin,
                    text: String::new(),
                    audio: Some(clip(48000, 1, 6000)),
                }],
            },
        ]
    }

    #[test]
    fn combines_lessons_and_mutes_without_changing_timing() {
        let lessons = lessons();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.wav");
        for (teacher, student) in [(true, true), (true, false), (false, true), (false, false)] {
            save(
                &path,
                &lessons,
                teacher,
                student,
                crate::project::AudioGaps::default(),
            )
            .unwrap();
            let reader = hound::WavReader::open(&path).unwrap();
            assert_eq!(reader.spec().sample_rate, RATE);
            let samples: Vec<i16> = reader.into_samples::<i16>().map(Result::unwrap).collect();
            assert_eq!(samples.len(), 8 * RATE as usize);
            assert_eq!(samples[0] != 0, teacher);
            assert!(samples[96000..144000].iter().all(|&s| s == 0));
            assert_eq!(samples[144000] != 0, student);
            assert!(samples[240000..288000].iter().all(|&s| s == 0));
            assert_eq!(samples[288000] != 0, teacher);
        }
        save(
            &path,
            &lessons[1..],
            true,
            true,
            crate::project::AudioGaps::default(),
        )
        .unwrap();
        assert_eq!(hound::WavReader::open(&path).unwrap().duration(), RATE);
    }

    #[test]
    fn incomplete_or_damaged_audio_never_overwrites_destination() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.wav");
        std::fs::write(&path, b"existing").unwrap();
        let mut lessons = lessons();
        lessons[0].entries[1].audio = None;
        assert!(!complete(&lessons));
        assert!(
            save(
                &path,
                &lessons,
                true,
                false,
                crate::project::AudioGaps::default()
            )
            .is_err()
        );
        lessons[0].entries[1].audio = Some("invalid".into());
        assert!(
            save(
                &path,
                &lessons,
                true,
                true,
                crate::project::AudioGaps::default()
            )
            .is_err()
        );
        assert_eq!(std::fs::read(&path).unwrap(), b"existing");
        lessons[0].entries.clear();
        assert!(!complete(&lessons));
    }

    #[test]
    fn exports_one_file_per_lesson_in_new_folder() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("Course");
        let lessons = lessons();
        save_lessons(
            &folder,
            "Course",
            &lessons,
            true,
            true,
            crate::project::AudioGaps::default(),
        )
        .unwrap();
        let first = hound::WavReader::open(folder.join("Course - One.wav")).unwrap();
        let second = hound::WavReader::open(folder.join("Course - Two.wav")).unwrap();
        assert_eq!(first.duration(), RATE * 2 + RATE / 2);
        assert_eq!(second.duration(), RATE);
        assert!(
            save_lessons(
                &folder,
                "Course",
                &lessons,
                true,
                true,
                crate::project::AudioGaps::default()
            )
            .is_err()
        );
        let mut duplicate = lessons;
        duplicate[1].name = "one".into();
        let other = dir.path().join("other");
        assert!(
            save_lessons(
                &other,
                "Course",
                &duplicate,
                true,
                true,
                crate::project::AudioGaps::default()
            )
            .is_err()
        );
        assert!(!other.exists());
    }

    #[test]
    fn participant_gaps_follow_settings_even_when_muted() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("gaps.wav");
        let mut lessons = lessons();
        let third = lessons[1].entries[0].clone();
        lessons[0].entries[1].participant = Participant::Teacher;
        lessons[0].entries.push(third);
        lessons[0].entries[2].participant = Participant::Student;
        let selected = &lessons[..1];
        for (gaps, same_frames, different_frames) in [
            (crate::project::AudioGaps::default(), 4800, 24000),
            (
                crate::project::AudioGaps {
                    same_participant_ms: 250,
                    different_participant_ms: 750,
                },
                12000,
                36000,
            ),
            (
                crate::project::AudioGaps {
                    same_participant_ms: 0,
                    different_participant_ms: 0,
                },
                0,
                0,
            ),
        ] {
            save(&path, selected, true, false, gaps).unwrap();
            let reader = hound::WavReader::open(&path).unwrap();
            assert_eq!(reader.duration(), RATE * 3 + same_frames + different_frames);
            let samples: Vec<i16> = reader.into_samples::<i16>().map(Result::unwrap).collect();
            let at = |frame: u32| samples[frame as usize * 2];
            if same_frames > 0 {
                assert_eq!(at(RATE), 0);
            }
            assert_ne!(at(RATE + same_frames), 0);
            if different_frames > 0 {
                assert_eq!(at(RATE * 2 + same_frames), 0);
            }
            assert_eq!(at(RATE * 2 + same_frames + different_frames), 0);
        }
    }
}
