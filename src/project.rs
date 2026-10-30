//! Versioned rkyv project persistence.
use std::{io::Write, path::Path};

use base64::{Engine, engine::general_purpose::STANDARD};

use crate::{Language, Lesson, Participant};

const MAGIC: &[u8; 8] = b"LOQUARIS";
const ARCHIVE_VERSION: u32 = 2;
const HEADER_SIZE: usize = 16;

// This schema is fixed for archive version 1. rkyv is configured for little
// endian, 32-bit pointers; future incompatible changes require a new version.
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
struct DiskProject {
    name: String,
    l1_name: String,
    lessons: Vec<DiskLesson>,
    export_paper: u8,
    export_font_size: f32,
    export_directory: Option<String>,
}

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
struct DiskProjectV2 {
    project: DiskProject,
    same_participant_ms: u32,
    different_participant_ms: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
    pub fn between(self, a: Participant, b: Participant) -> u32 {
        if a == b {
            self.same_participant_ms
        } else {
            self.different_participant_ms
        }
    }

    pub fn validate(self) -> Result<(), String> {
        if self.same_participant_ms > 60_000 || self.different_participant_ms > 60_000 {
            return Err("Audio gaps must be between 0 and 60 seconds.".into());
        }
        Ok(())
    }
}

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
struct DiskLesson {
    name: String,
    expanded: bool,
    entries: Vec<DiskEntry>,
}

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
struct DiskEntry {
    participant: u8,
    language: u8,
    text: String,
    audio: Option<Vec<u8>>,
}

/// Make a portable suggestion without changing the project name itself.
pub fn suggested_file_name(name: &str) -> String {
    let sanitized: String = name
        .chars()
        .map(|character| {
            if character.is_control() || "<>:\"/\\|?*".contains(character) {
                '_'
            } else {
                character
            }
        })
        .collect();
    let sanitized = sanitized.trim_matches(|character| character == ' ' || character == '.');
    let mut stem = if sanitized.is_empty() {
        "project".to_owned()
    } else {
        sanitized.to_owned()
    };
    let base = stem.split('.').next().unwrap_or("").to_ascii_uppercase();
    let reserved = matches!(base.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (base.len() == 4
            && (base.starts_with("COM") || base.starts_with("LPT"))
            && matches!(base.as_bytes()[3], b'1'..=b'9'));
    if reserved {
        stem.insert(0, '_');
    }
    if !stem.to_ascii_lowercase().ends_with(".loqr") {
        stem.push_str(".loqr");
    }
    stem
}

pub fn ensure_extension(path: &Path) -> (std::path::PathBuf, bool) {
    if path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("loqr"))
    {
        return (path.to_path_buf(), false);
    }
    let mut name = path.as_os_str().to_os_string();
    name.push(".loqr");
    (name.into(), true)
}

#[derive(Clone, Debug, PartialEq)]
pub struct Project {
    pub name: String,
    pub l1_name: String,
    pub lessons: Vec<Lesson>,
    pub export_settings: crate::export_pdf::Settings,
    pub audio_gaps: AudioGaps,
}

impl Project {
    pub fn new(name: String, l1_name: String, lessons: Vec<Lesson>) -> Self {
        Self {
            name,
            l1_name,
            lessons,
            export_settings: crate::export_pdf::Settings::default(),
            audio_gaps: AudioGaps::default(),
        }
    }

    fn validate(&self) -> Result<(), String> {
        if self.lessons.is_empty() {
            return Err("A project must contain at least one lesson.".into());
        }
        self.export_settings.validate()?;
        self.audio_gaps.validate()?;
        for (lesson_index, lesson) in self.lessons.iter().enumerate() {
            for (entry_index, entry) in lesson.entries.iter().enumerate() {
                if entry.participant == Participant::Student && entry.language != Language::Latin {
                    return Err(format!(
                        "Lesson {}, entry {}: student entries must be Latin.",
                        lesson_index + 1,
                        entry_index + 1
                    ));
                }
            }
        }
        Ok(())
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let bytes = std::fs::read(path)
            .map_err(|error| format!("Could not read {}: {error}", path.display()))?;
        if !bytes.starts_with(MAGIC) {
            return Err("Unrecognized or damaged project file.".into());
        }
        if bytes.len() < HEADER_SIZE {
            return Err("Truncated project header.".into());
        }
        let version = u32::from_le_bytes(bytes[8..12].try_into().unwrap());
        if version != 1 && version != ARCHIVE_VERSION {
            return Err(format!("Unsupported project archive version {version}."));
        }
        if bytes[12..HEADER_SIZE] != [0; 4] {
            return Err("Unsupported project header flags.".into());
        }
        let mut aligned = rkyv::util::AlignedVec::<16>::new();
        aligned.extend_from_slice(&bytes[HEADER_SIZE..]);
        let (disk, gaps) = if version == 1 {
            (
                rkyv::from_bytes::<DiskProject, rkyv::rancor::Error>(&aligned)
                    .map_err(|error| format!("Invalid project archive: {error}"))?,
                AudioGaps::default(),
            )
        } else {
            let disk = rkyv::from_bytes::<DiskProjectV2, rkyv::rancor::Error>(&aligned)
                .map_err(|error| format!("Invalid project archive: {error}"))?;
            (
                disk.project,
                AudioGaps {
                    same_participant_ms: disk.same_participant_ms,
                    different_participant_ms: disk.different_participant_ms,
                },
            )
        };
        let mut project = Self::from_disk(disk)?;
        project.audio_gaps = gaps;
        project.validate()?;
        Ok(project)
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        self.validate()?;
        let disk = DiskProjectV2 {
            project: self.to_disk()?,
            same_participant_ms: self.audio_gaps.same_participant_ms,
            different_participant_ms: self.audio_gaps.different_participant_ms,
        };
        let archive = rkyv::to_bytes::<rkyv::rancor::Error>(&disk)
            .map_err(|error| format!("Could not serialize project: {error}"))?;
        let parent = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let mut file = tempfile::NamedTempFile::new_in(parent)
            .map_err(|error| format!("Could not create a temporary file: {error}"))?;
        file.write_all(MAGIC)
            .and_then(|_| file.write_all(&ARCHIVE_VERSION.to_le_bytes()))
            .and_then(|_| file.write_all(&[0; 4]))
            .and_then(|_| file.write_all(&archive))
            .and_then(|_| file.as_file().sync_all())
            .map_err(|error| format!("Could not write project: {error}"))?;
        file.persist(path)
            .map_err(|error| format!("Could not save {}: {}", path.display(), error.error))?;
        Ok(())
    }

    fn to_disk(&self) -> Result<DiskProject, String> {
        let mut lessons = Vec::with_capacity(self.lessons.len());
        for (li, lesson) in self.lessons.iter().enumerate() {
            let mut entries = Vec::with_capacity(lesson.entries.len());
            for (ei, entry) in lesson.entries.iter().enumerate() {
                let audio = entry
                    .audio
                    .as_ref()
                    .map(|audio| {
                        STANDARD.decode(audio).map_err(|error| {
                            format!(
                                "Lesson {}, entry {} has invalid audio: {error}",
                                li + 1,
                                ei + 1
                            )
                        })
                    })
                    .transpose()?;
                entries.push(DiskEntry {
                    participant: match entry.participant {
                        Participant::Teacher => 0,
                        Participant::Student => 1,
                    },
                    language: match entry.language {
                        Language::L1 => 0,
                        Language::Latin => 1,
                    },
                    text: entry.text.clone(),
                    audio,
                });
            }
            lessons.push(DiskLesson {
                name: lesson.name.clone(),
                expanded: lesson.expanded,
                entries,
            });
        }
        let export_directory = self
            .export_settings
            .directory
            .as_ref()
            .map(|path| {
                path.to_str().map(str::to_owned).ok_or_else(|| {
                    "The export folder path is not valid Unicode and cannot be saved.".to_owned()
                })
            })
            .transpose()?;
        Ok(DiskProject {
            name: self.name.clone(),
            l1_name: self.l1_name.clone(),
            lessons,
            export_paper: match self.export_settings.paper {
                crate::export_pdf::Paper::A4 => 0,
                crate::export_pdf::Paper::Letter => 1,
                crate::export_pdf::Paper::Legal => 2,
                crate::export_pdf::Paper::A3 => 3,
            },
            export_font_size: self.export_settings.font_size,
            export_directory,
        })
    }

    fn from_disk(disk: DiskProject) -> Result<Self, String> {
        let paper = match disk.export_paper {
            0 => crate::export_pdf::Paper::A4,
            1 => crate::export_pdf::Paper::Letter,
            2 => crate::export_pdf::Paper::Legal,
            3 => crate::export_pdf::Paper::A3,
            _ => return Err("Invalid paper size in project archive.".into()),
        };
        let mut lessons = Vec::with_capacity(disk.lessons.len());
        for lesson in disk.lessons {
            let mut entries = Vec::with_capacity(lesson.entries.len());
            for entry in lesson.entries {
                let participant = match entry.participant {
                    0 => Participant::Teacher,
                    1 => Participant::Student,
                    _ => return Err("Invalid participant in project archive.".into()),
                };
                let language = match entry.language {
                    0 => Language::L1,
                    1 => Language::Latin,
                    _ => return Err("Invalid language in project archive.".into()),
                };
                entries.push(crate::Entry {
                    participant,
                    language,
                    text: entry.text,
                    audio: entry.audio.map(|bytes| STANDARD.encode(bytes)),
                });
            }
            lessons.push(Lesson {
                name: lesson.name,
                expanded: lesson.expanded,
                entries,
            });
        }
        let mut project = Self::new(disk.name, disk.l1_name, lessons);
        project.export_settings = crate::export_pdf::Settings {
            paper,
            font_size: disk.export_font_size,
            directory: disk.export_directory.map(std::path::PathBuf::from),
        };
        Ok(project)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Entry;

    fn example() -> Project {
        Project::new(
            "Via nātūrālis".into(),
            "English".into(),
            vec![
                Lesson {
                    name: "First".into(),
                    expanded: true,
                    entries: vec![
                        Entry {
                            audio: None,
                            participant: Participant::Teacher,
                            language: Language::L1,
                            text: "A \"hand\"\nwith a newline".into(),
                        },
                        Entry {
                            audio: None,
                            participant: Participant::Student,
                            language: Language::Latin,
                            text: "duae manūs".into(),
                        },
                    ],
                },
                Lesson {
                    name: "Second".into(),
                    expanded: false,
                    entries: vec![],
                },
            ],
        )
    }

    #[test]
    fn suggested_name_uses_project_title_and_is_portable() {
        assert_eq!(suggested_file_name("Via nātūrālis"), "Via nātūrālis.loqr");
        assert_eq!(suggested_file_name("Latin/2026"), "Latin_2026.loqr");
        assert_eq!(suggested_file_name("CON"), "_CON.loqr");
        assert_eq!(suggested_file_name("Course.LOQR"), "Course.LOQR");
        assert_eq!(suggested_file_name("  "), "project.loqr");
        assert_eq!(
            ensure_extension(Path::new("course")),
            ("course.loqr".into(), true)
        );
        assert_eq!(
            ensure_extension(Path::new("course.backup")),
            ("course.backup.loqr".into(), true)
        );
        assert_eq!(
            ensure_extension(Path::new("course.LOQR")),
            ("course.LOQR".into(), false)
        );
    }

    #[test]
    fn round_trips_an_archive_with_binary_audio() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("project.loqr");
        let mut project = example();
        let audio = vec![0x85; 8192];
        project.lessons[0].entries[0].audio = Some(STANDARD.encode(&audio));
        project.save(&path).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert!(bytes.starts_with(MAGIC));
        assert!(bytes.windows(audio.len()).any(|window| window == audio));
        assert!(bytes.len() < STANDARD.encode(&audio).len());
        assert_eq!(Project::load(&path).unwrap(), project);
        let mut changed = project.clone();
        changed.name = "Changed".into();
        changed.save(&path).unwrap();
        assert_eq!(Project::load(&path).unwrap(), changed);
    }

    #[test]
    fn export_settings_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("course.loqr");
        let mut project = example();
        project.export_settings.paper = crate::export_pdf::Paper::Legal;
        project.export_settings.font_size = 19.5;
        project.export_settings.directory = Some(dir.path().join("exports"));
        project.save(&path).unwrap();
        assert_eq!(Project::load(&path).unwrap(), project);
    }

    #[test]
    fn audio_gaps_round_trip_and_old_projects_use_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("course.loqr");
        let mut project = example();
        let disk = project.to_disk().unwrap();
        let archive = rkyv::to_bytes::<rkyv::rancor::Error>(&disk).unwrap();
        let mut bytes = MAGIC.to_vec();
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&[0; 4]);
        bytes.extend_from_slice(&archive);
        std::fs::write(&path, bytes).unwrap();
        assert_eq!(Project::load(&path).unwrap(), project);
        project.audio_gaps = AudioGaps {
            same_participant_ms: 250,
            different_participant_ms: 1200,
        };
        project.save(&path).unwrap();
        assert_eq!(Project::load(&path).unwrap(), project);
        project.audio_gaps.same_participant_ms = 60_001;
        assert!(project.save(&path).is_err());
        assert_eq!(
            Project::load(&path).unwrap().audio_gaps.same_participant_ms,
            250
        );
    }

    #[test]
    fn invalid_export_font_does_not_overwrite_project() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("course.loqr");
        let mut project = example();
        project.save(&path).unwrap();
        project.export_settings.font_size = f32::NAN;
        assert!(project.save(&path).is_err());
        assert_eq!(Project::load(&path).unwrap(), example());
    }

    #[test]
    fn rejects_invalid_projects_without_overwriting_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("project.loqr");
        let valid = example();
        valid.save(&path).unwrap();
        let mut invalid = valid.clone();
        invalid.lessons[0].entries[1].language = Language::L1;
        assert!(invalid.save(&path).is_err());
        assert_eq!(Project::load(&path).unwrap(), valid);
        invalid = valid.clone();
        invalid.lessons.clear();
        assert!(invalid.save(&path).is_err());
        let mut bytes = std::fs::read(&path).unwrap();
        bytes[8..12].copy_from_slice(&99u32.to_le_bytes());
        std::fs::write(&path, &bytes).unwrap();
        assert!(Project::load(&path).unwrap_err().contains("version"));
        bytes[8..12].copy_from_slice(&ARCHIVE_VERSION.to_le_bytes());
        bytes.truncate(HEADER_SIZE + 1);
        std::fs::write(&path, &bytes).unwrap();
        assert!(Project::load(&path).is_err());
        std::fs::write(&path, "not a project").unwrap();
        assert!(Project::load(&path).is_err());
    }

    #[test]
    fn invalid_audio_does_not_overwrite_archive() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("course.loqr");
        let valid = example();
        valid.save(&path).unwrap();
        let mut invalid = valid.clone();
        invalid.lessons[0].entries[0].audio = Some("not base64!".into());
        assert!(invalid.save(&path).unwrap_err().contains("invalid audio"));
        assert_eq!(Project::load(&path).unwrap(), valid);
    }

    #[test]
    fn rejects_invalid_archive_flags_and_semantic_values() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("invalid.loqr");
        let mut disk = example().to_disk().unwrap();
        disk.lessons[0].entries[0].participant = 9;
        let archive = rkyv::to_bytes::<rkyv::rancor::Error>(&disk).unwrap();
        let mut bytes = MAGIC.to_vec();
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&[0; 4]);
        bytes.extend_from_slice(&archive);
        std::fs::write(&path, &bytes).unwrap();
        assert!(Project::load(&path).unwrap_err().contains("participant"));
        bytes[12] = 1;
        std::fs::write(&path, &bytes).unwrap();
        assert!(Project::load(&path).unwrap_err().contains("flags"));
    }
}
