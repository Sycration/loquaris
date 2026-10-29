//! Project data and TOML persistence. UI state and undo history are not stored.
use std::{io::Write, path::Path};

use serde::{Deserialize, Serialize};

use crate::{Language, Lesson, Participant};

const FORMAT_VERSION: u32 = 1;

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
    if !stem.to_ascii_lowercase().ends_with(".toml") {
        stem.push_str(".toml");
    }
    stem
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Project {
    pub format_version: u32,
    pub name: String,
    pub l1_name: String,
    pub lessons: Vec<Lesson>,
}

impl Project {
    pub fn new(name: String, l1_name: String, lessons: Vec<Lesson>) -> Self {
        Self {
            format_version: FORMAT_VERSION,
            name,
            l1_name,
            lessons,
        }
    }

    fn validate(&self) -> Result<(), String> {
        if self.format_version != FORMAT_VERSION {
            return Err(format!(
                "Unsupported project format version {}.",
                self.format_version
            ));
        }
        if self.lessons.is_empty() {
            return Err("A project must contain at least one lesson.".into());
        }
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
        let text = std::fs::read_to_string(path)
            .map_err(|error| format!("Could not read {}: {error}", path.display()))?;
        let project: Self =
            toml::from_str(&text).map_err(|error| format!("Invalid project TOML: {error}"))?;
        project.validate()?;
        Ok(project)
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        self.validate()?;
        let text = toml::to_string_pretty(self)
            .map_err(|error| format!("Could not serialize project: {error}"))?;
        let parent = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let mut file = tempfile::NamedTempFile::new_in(parent)
            .map_err(|error| format!("Could not create a temporary file: {error}"))?;
        file.write_all(text.as_bytes())
            .and_then(|_| file.as_file().sync_all())
            .map_err(|error| format!("Could not write project: {error}"))?;
        file.persist(path)
            .map_err(|error| format!("Could not save {}: {}", path.display(), error.error))?;
        Ok(())
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
                            participant: Participant::Teacher,
                            language: Language::L1,
                            text: "A \"hand\"\nwith a newline".into(),
                        },
                        Entry {
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
        assert_eq!(suggested_file_name("Via nātūrālis"), "Via nātūrālis.toml");
        assert_eq!(suggested_file_name("Latin/2026"), "Latin_2026.toml");
        assert_eq!(suggested_file_name("CON"), "_CON.toml");
        assert_eq!(suggested_file_name("Course.TOML"), "Course.TOML");
        assert_eq!(suggested_file_name("  "), "project.toml");
    }

    #[test]
    fn round_trips_a_project_through_toml_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("project.toml");
        let project = example();
        project.save(&path).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("format_version = 1"));
        assert!(text.contains("duae manūs"));
        assert_eq!(Project::load(&path).unwrap(), project);
        let mut changed = project.clone();
        changed.name = "Changed".into();
        changed.save(&path).unwrap();
        assert_eq!(Project::load(&path).unwrap(), changed);
    }

    #[test]
    fn rejects_invalid_projects_without_overwriting_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("project.toml");
        let valid = example();
        valid.save(&path).unwrap();
        let mut invalid = valid.clone();
        invalid.lessons[0].entries[1].language = Language::L1;
        assert!(invalid.save(&path).is_err());
        assert_eq!(Project::load(&path).unwrap(), valid);
        invalid = valid.clone();
        invalid.lessons.clear();
        assert!(invalid.save(&path).is_err());
        invalid = valid;
        invalid.format_version = 99;
        std::fs::write(&path, toml::to_string(&invalid).unwrap()).unwrap();
        assert!(Project::load(&path).unwrap_err().contains("version"));
        std::fs::write(&path, "not = [valid").unwrap();
        assert!(Project::load(&path).is_err());
    }
}
