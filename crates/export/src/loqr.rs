//! The desktop app's versioned rkyv `.loqr` archives.
//!
//! An archive is a 16-byte header (`LOQUARIS`, the version as a little-endian
//! `u32`, four zero flag bytes) followed by an rkyv archive of the schema for
//! that version. The schemas below are frozen copies of the desktop app's: rkyv
//! lays out fields by declaration order and type, so they must never change.
use std::{collections::HashSet, fmt::Write as _};

use loquaris_core::{
    AudioGaps, AudioRef, Entry, Language, LemmaChoice, Lesson, Paper, Participant, PdfSettings,
    Project, Trim,
};
use sha2::{Digest, Sha256};

use crate::{
    ClipLoader,
    wav::{inspect, selected_frames},
};

const MAGIC: &[u8; 8] = b"LOQUARIS";
const ARCHIVE_VERSION: u32 = 5;
const HEADER_SIZE: usize = 16;
/// The desktop app's word cache schema. An empty cache of any fingerprint is
/// valid; the desktop app simply analyzes the words again.
const CACHE_SCHEMA_VERSION: u32 = 1;

/// A desktop project read from a `.loqr` archive. Entries' audio refers
/// to `blobs` by SHA-256.
pub struct Imported {
    pub project: Project,
    /// `(sha256 hex, WAV bytes)`, without duplicates.
    pub blobs: Vec<(String, Vec<u8>)>,
}

impl std::fmt::Debug for Imported {
    /// Blobs are shown by hash and size rather than byte by byte.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let blobs: Vec<_> = self
            .blobs
            .iter()
            .map(|(blob, bytes)| format!("{blob} ({} bytes)", bytes.len()))
            .collect();
        f.debug_struct("Imported")
            .field("project", &self.project)
            .field("blobs", &blobs)
            .finish()
    }
}

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

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
struct DiskProjectV3 {
    project: DiskProject,
    same_participant_ms: u32,
    different_participant_ms: u32,
    word_cache: PersistedCache,
}

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
struct DiskProjectV4 {
    project: DiskProject,
    same_participant_ms: u32,
    different_participant_ms: u32,
    word_cache: PersistedCache,
    /// Only the cropped entries.
    trims: Vec<DiskTrim>,
}

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
struct DiskProjectV5 {
    project: DiskProject,
    same_participant_ms: u32,
    different_participant_ms: u32,
    word_cache: PersistedCache,
    /// Only the cropped entries.
    trims: Vec<DiskTrim>,
    lemma_choices: Vec<DiskLemmaChoice>,
}

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
struct DiskLemmaChoice {
    lesson: u32,
    entry: u32,
    token: String,
    occurrence: u32,
    lemma_key: String,
    extension: bool,
}

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
struct DiskTrim {
    lesson: u32,
    entry: u32,
    start: u32,
    end: u32,
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

/// The desktop app's cache of collo analyses (`word_index::PersistedCache`).
/// It is read only to decode archives that contain it; the web app derives
/// its index from the text.
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
struct PersistedCache {
    schema_version: u32,
    fingerprint: u64,
    words: Vec<PersistedWord>,
}

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
struct PersistedWord {
    token: String,
    sentence_start: bool,
    include_extension: bool,
    analyses: Vec<PersistedAnalysis>,
}

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
struct PersistedAnalysis {
    lemma_key: String,
    extension: bool,
    morphology_id: u16,
    form: String,
    suffix: String,
    irregular: bool,
    multiplicity: u32,
}

impl PersistedCache {
    fn empty() -> Self {
        Self {
            schema_version: CACHE_SCHEMA_VERSION,
            fingerprint: 0,
            words: Vec::new(),
        }
    }
}

/// A random 16-character base-62 ID for a lesson or entry.
pub(crate) fn new_id() -> String {
    const DIGITS: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
    let mut id = String::with_capacity(16);
    let mut bytes = [0u8; 32];
    while id.len() < 16 {
        getrandom::fill(&mut bytes).expect("the system random number generator failed");
        // 248 is the largest multiple of 62 that fits a byte; rejecting the
        // rest keeps every digit equally likely.
        for &byte in bytes.iter().filter(|&&byte| byte < 248) {
            if id.len() < 16 {
                id.push(char::from(DIGITS[usize::from(byte % 62)]));
            }
        }
    }
    id
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .fold(String::with_capacity(64), |mut hex, byte| {
            let _ = write!(hex, "{byte:02x}");
            hex
        })
}

/// The parts of an archive that differ between versions, with older versions'
/// missing parts at their defaults.
struct Decoded {
    project: DiskProject,
    gaps: AudioGaps,
    trims: Vec<DiskTrim>,
    lemma_choices: Vec<DiskLemmaChoice>,
}

fn decode(bytes: &[u8]) -> Result<Decoded, String> {
    if !bytes.starts_with(MAGIC) {
        return Err("Unrecognized or damaged project file.".into());
    }
    if bytes.len() < HEADER_SIZE {
        return Err("Truncated project header.".into());
    }
    let version = u32::from_le_bytes(bytes[8..12].try_into().unwrap());
    if !(1..=ARCHIVE_VERSION).contains(&version) {
        return Err(format!("Unsupported project archive version {version}."));
    }
    if bytes[12..HEADER_SIZE] != [0; 4] {
        return Err("Unsupported project header flags.".into());
    }
    let mut aligned = rkyv::util::AlignedVec::<16>::new();
    aligned.extend_from_slice(&bytes[HEADER_SIZE..]);
    let invalid = |error: rkyv::rancor::Error| format!("Invalid project archive: {error}");
    let gaps = |same_participant_ms, different_participant_ms| AudioGaps {
        same_participant_ms,
        different_participant_ms,
    };
    Ok(match version {
        1 => Decoded {
            project: rkyv::from_bytes::<DiskProject, _>(&aligned).map_err(invalid)?,
            gaps: AudioGaps::default(),
            trims: Vec::new(),
            lemma_choices: Vec::new(),
        },
        2 => {
            let disk = rkyv::from_bytes::<DiskProjectV2, _>(&aligned).map_err(invalid)?;
            Decoded {
                project: disk.project,
                gaps: gaps(disk.same_participant_ms, disk.different_participant_ms),
                trims: Vec::new(),
                lemma_choices: Vec::new(),
            }
        }
        3 => {
            let disk = rkyv::from_bytes::<DiskProjectV3, _>(&aligned).map_err(invalid)?;
            Decoded {
                project: disk.project,
                gaps: gaps(disk.same_participant_ms, disk.different_participant_ms),
                trims: Vec::new(),
                lemma_choices: Vec::new(),
            }
        }
        4 => {
            let disk = rkyv::from_bytes::<DiskProjectV4, _>(&aligned).map_err(invalid)?;
            Decoded {
                project: disk.project,
                gaps: gaps(disk.same_participant_ms, disk.different_participant_ms),
                trims: disk.trims,
                lemma_choices: Vec::new(),
            }
        }
        5 => {
            let disk = rkyv::from_bytes::<DiskProjectV5, _>(&aligned).map_err(invalid)?;
            Decoded {
                project: disk.project,
                gaps: gaps(disk.same_participant_ms, disk.different_participant_ms),
                trims: disk.trims,
                lemma_choices: disk.lemma_choices,
            }
        }
        _ => unreachable!(),
    })
}

/// Read any archive version the desktop app wrote (1–5). New lesson and
/// entry IDs are generated.
///
/// Every clip is fully decoded. Clips are not held to the 100 MiB upload
/// limit, since a ten-minute desktop recording can exceed it. Crops are
/// clamped to their clip, as the desktop app did when playing them; a crop of
/// the whole clip, or of an entry without audio, is dropped.
pub fn read(bytes: &[u8]) -> Result<Imported, String> {
    let Decoded {
        project: disk,
        gaps,
        trims,
        lemma_choices,
    } = decode(bytes)?;
    let paper = match disk.export_paper {
        0 => Paper::A4,
        1 => Paper::Letter,
        2 => Paper::Legal,
        3 => Paper::A3,
        _ => return Err("Invalid paper size in project archive.".into()),
    };
    let pdf = PdfSettings {
        paper,
        font_size: disk.export_font_size,
    };
    if !pdf.font_size.is_finite()
        || !(PdfSettings::MIN_FONT_SIZE..=PdfSettings::MAX_FONT_SIZE).contains(&pdf.font_size)
    {
        return Err(format!(
            "PDF font size must be between {} and {} pt.",
            PdfSettings::MIN_FONT_SIZE,
            PdfSettings::MAX_FONT_SIZE
        ));
    }
    if gaps.same_participant_ms > AudioGaps::MAX_MS
        || gaps.different_participant_ms > AudioGaps::MAX_MS
    {
        return Err("Audio gaps must be between 0 and 60 seconds.".into());
    }
    if disk.lessons.is_empty() {
        return Err("A project must contain at least one lesson.".into());
    }
    let mut blobs = Vec::new();
    let mut stored = HashSet::new();
    let mut lessons = Vec::with_capacity(disk.lessons.len());
    for (lesson_index, lesson) in disk.lessons.into_iter().enumerate() {
        let mut entries = Vec::with_capacity(lesson.entries.len());
        for (entry_index, entry) in lesson.entries.into_iter().enumerate() {
            let position = format!("Lesson {}, entry {}", lesson_index + 1, entry_index + 1);
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
            if participant == Participant::Student && language != Language::Latin {
                return Err(format!("{position}: student entries must be Latin."));
            }
            let audio = match entry.audio {
                None => None,
                Some(clip) => {
                    let info = inspect(&clip).map_err(|e| format!("{position}: {e}"))?;
                    let blob = sha256_hex(&clip);
                    if stored.insert(blob.clone()) {
                        blobs.push((blob.clone(), clip));
                    }
                    Some(AudioRef {
                        blob,
                        sample_rate: info.sample_rate,
                        frames: info.frames,
                        channels: info.channels,
                    })
                }
            };
            entries.push(Entry {
                id: new_id(),
                participant,
                language,
                text: entry.text,
                audio,
                trim: None,
                lemmas: Vec::new(),
            });
        }
        lessons.push(Lesson {
            id: new_id(),
            name: lesson.name,
            entries,
        });
    }
    fn entry(lessons: &mut [Lesson], lesson: u32, entry: u32) -> Option<&mut Entry> {
        lessons
            .get_mut(lesson as usize)
            .and_then(|lesson| lesson.entries.get_mut(entry as usize))
    }
    for choice in lemma_choices {
        entry(&mut lessons, choice.lesson, choice.entry)
            .ok_or("Invalid lemma choice in project archive.")?
            .lemmas
            .push(LemmaChoice {
                token: choice.token,
                occurrence: choice.occurrence,
                lemma_key: choice.lemma_key,
                extension: choice.extension,
            });
    }
    for trim in trims {
        let entry = entry(&mut lessons, trim.lesson, trim.entry)
            .filter(|_| trim.start < trim.end)
            .ok_or("Invalid audio crop in project archive.")?;
        entry.trim = entry.audio.as_ref().and_then(|audio| {
            let selected = selected_frames(
                Some(Trim {
                    start: trim.start,
                    end: trim.end,
                }),
                audio.frames,
            );
            (selected != (0..audio.frames)).then_some(Trim {
                start: selected.start,
                end: selected.end,
            })
        });
    }
    Ok(Imported {
        project: Project {
            name: disk.name,
            l1_name: disk.l1_name,
            gaps,
            pdf,
            lessons,
        },
        blobs,
    })
}

/// Write a version 5 archive the desktop app can open.
///
/// The desktop app is stricter than the web app's document, so the archive
/// is adjusted to load there: a project without lessons gets one empty
/// lesson, student entries are written as Latin, out-of-range settings are
/// clamped (a non-finite font size becomes the default) and empty crops are
/// left out. Every lesson is expanded and the word cache is empty, which the
/// desktop app fills again.
pub fn write(project: &Project, load: &mut ClipLoader) -> Result<Vec<u8>, String> {
    let disk = to_disk(project, load)?;
    let archive = rkyv::to_bytes::<rkyv::rancor::Error>(&disk)
        .map_err(|error| format!("Could not serialize project: {error}"))?;
    Ok(with_header(ARCHIVE_VERSION, &archive))
}

fn with_header(version: u32, archive: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(HEADER_SIZE + archive.len());
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&version.to_le_bytes());
    bytes.extend_from_slice(&[0; 4]);
    bytes.extend_from_slice(archive);
    bytes
}

fn to_disk(project: &Project, load: &mut ClipLoader) -> Result<DiskProjectV5, String> {
    let mut lessons = Vec::with_capacity(project.lessons.len().max(1));
    let mut trims = Vec::new();
    let mut lemma_choices = Vec::new();
    for (lesson_index, lesson) in project.lessons.iter().enumerate() {
        let mut entries = Vec::with_capacity(lesson.entries.len());
        for (entry_index, entry) in lesson.entries.iter().enumerate() {
            let (lesson_number, entry_number) = (lesson_index as u32, entry_index as u32);
            let audio = entry
                .audio
                .as_ref()
                .map(|audio| {
                    load(audio).map_err(|e| {
                        format!(
                            "Lesson ‘{}’, entry {}: {e}",
                            lesson.display_name(),
                            entry_index + 1
                        )
                    })
                })
                .transpose()?;
            if let Some(trim) = entry
                .trim
                .filter(|trim| audio.is_some() && trim.start < trim.end)
            {
                trims.push(DiskTrim {
                    lesson: lesson_number,
                    entry: entry_number,
                    start: trim.start,
                    end: trim.end,
                });
            }
            lemma_choices.extend(entry.lemmas.iter().map(|choice| DiskLemmaChoice {
                lesson: lesson_number,
                entry: entry_number,
                token: choice.token.clone(),
                occurrence: choice.occurrence,
                lemma_key: choice.lemma_key.clone(),
                extension: choice.extension,
            }));
            let language = match (entry.participant, entry.language) {
                (Participant::Student, _) | (_, Language::Latin) => 1,
                (Participant::Teacher, Language::L1) => 0,
            };
            entries.push(DiskEntry {
                participant: match entry.participant {
                    Participant::Teacher => 0,
                    Participant::Student => 1,
                },
                language,
                text: entry.text.clone(),
                audio,
            });
        }
        lessons.push(DiskLesson {
            name: lesson.name.clone(),
            expanded: true,
            entries,
        });
    }
    if lessons.is_empty() {
        lessons.push(DiskLesson {
            name: String::new(),
            expanded: true,
            entries: Vec::new(),
        });
    }
    let font_size = if project.pdf.font_size.is_finite() {
        project
            .pdf
            .font_size
            .clamp(PdfSettings::MIN_FONT_SIZE, PdfSettings::MAX_FONT_SIZE)
    } else {
        PdfSettings::default().font_size
    };
    Ok(DiskProjectV5 {
        project: DiskProject {
            name: project.name.clone(),
            l1_name: project.l1_name.clone(),
            lessons,
            export_paper: match project.pdf.paper {
                Paper::A4 => 0,
                Paper::Letter => 1,
                Paper::Legal => 2,
                Paper::A3 => 3,
            },
            export_font_size: font_size,
            export_directory: None,
        },
        same_participant_ms: project.gaps.same_participant_ms.min(AudioGaps::MAX_MS),
        different_participant_ms: project.gaps.different_participant_ms.min(AudioGaps::MAX_MS),
        word_cache: PersistedCache::empty(),
        trims,
        lemma_choices,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::tests::{entry, lesson, project, wav};

    /// The legacy test project: two lessons, the second empty.
    fn example() -> Project {
        let mut project = project(vec![
            lesson(
                "First",
                vec![
                    entry(
                        Participant::Teacher,
                        Language::L1,
                        "A \"hand\"\nwith a newline",
                    ),
                    entry(Participant::Student, Language::Latin, "duae manūs"),
                ],
            ),
            lesson("Second", vec![]),
        ]);
        project.name = "Via nātūrālis".into();
        project
    }

    /// `example` with a clip on each entry, a crop, lemma choices and
    /// non-default settings, and a loader for its clips.
    fn full_example() -> (Project, HashMap<String, Vec<u8>>) {
        let mut project = example();
        let mut clips = HashMap::new();
        for (index, bytes) in [
            wav(8000, 1, 100, |frame, _| frame as i16),
            wav(44_100, 2, 50, |_, channel| channel as i16),
        ]
        .into_iter()
        .enumerate()
        {
            let info = inspect(&bytes).unwrap();
            let blob = sha256_hex(&bytes);
            clips.insert(blob.clone(), bytes);
            project.lessons[0].entries[index].audio = Some(AudioRef {
                blob,
                sample_rate: info.sample_rate,
                frames: info.frames,
                channels: info.channels,
            });
        }
        project.lessons[0].entries[1].trim = Some(Trim { start: 5, end: 40 });
        project.lessons[0].entries[1].lemmas = vec![
            LemmaChoice {
                token: "manus".into(),
                occurrence: 0,
                lemma_key: "manus".into(),
                extension: false,
            },
            LemmaChoice {
                token: "duae".into(),
                occurrence: 0,
                lemma_key: "duo".into(),
                extension: true,
            },
        ];
        project.gaps = AudioGaps {
            same_participant_ms: 250,
            different_participant_ms: 1200,
        };
        project.pdf = PdfSettings {
            paper: Paper::Legal,
            font_size: 19.5,
        };
        (project, clips)
    }

    fn loader(
        clips: &HashMap<String, Vec<u8>>,
    ) -> impl FnMut(&AudioRef) -> Result<Vec<u8>, String> {
        |audio: &AudioRef| {
            clips
                .get(&audio.blob)
                .cloned()
                .ok_or_else(|| "missing clip".to_owned())
        }
    }

    /// The project with IDs blanked, for comparing a project with its copy.
    fn without_ids(mut project: Project) -> Project {
        for lesson in &mut project.lessons {
            lesson.id.clear();
            for entry in &mut lesson.entries {
                entry.id.clear();
            }
        }
        project
    }

    fn archive<T>(version: u32, disk: &T) -> Vec<u8>
    where
        T: for<'a> rkyv::Serialize<
                rkyv::api::high::HighSerializer<
                    rkyv::util::AlignedVec,
                    rkyv::ser::allocator::ArenaHandle<'a>,
                    rkyv::rancor::Error,
                >,
            >,
    {
        with_header(
            version,
            &rkyv::to_bytes::<rkyv::rancor::Error>(disk).unwrap(),
        )
    }

    fn disk(project: &Project, clips: &HashMap<String, Vec<u8>>) -> DiskProjectV5 {
        to_disk(project, &mut loader(clips)).unwrap()
    }

    fn cache() -> PersistedCache {
        PersistedCache {
            schema_version: CACHE_SCHEMA_VERSION,
            fingerprint: 0x1234_5678_9abc_def0,
            words: vec![PersistedWord {
                token: "manūs".into(),
                sentence_start: false,
                include_extension: true,
                analyses: vec![PersistedAnalysis {
                    lemma_key: "manus".into(),
                    extension: false,
                    morphology_id: 7,
                    form: "manūs".into(),
                    suffix: String::new(),
                    irregular: false,
                    multiplicity: 1,
                }],
            }],
        }
    }

    #[test]
    fn ids_are_base62_and_distinct() {
        let ids: HashSet<_> = (0..1000).map(|_| new_id()).collect();
        assert_eq!(ids.len(), 1000);
        assert!(
            ids.iter()
                .all(|id| id.len() == 16 && id.bytes().all(|b| b.is_ascii_alphanumeric()))
        );
    }

    #[test]
    fn round_trips_a_project_with_audio_crops_and_lemma_choices() {
        let (project, clips) = full_example();
        let bytes = write(&project, &mut loader(&clips)).unwrap();
        assert!(bytes.starts_with(MAGIC));
        assert_eq!(bytes[8..12], ARCHIVE_VERSION.to_le_bytes());
        for clip in clips.values() {
            assert!(bytes.windows(clip.len()).any(|window| window == clip));
        }
        let imported = read(&bytes).unwrap();
        assert_eq!(
            without_ids(imported.project.clone()),
            without_ids(project.clone())
        );
        assert_ne!(imported.project.lessons[0].id, project.lessons[0].id);
        assert_eq!(imported.blobs.len(), 2);
        for (blob, bytes) in &imported.blobs {
            assert_eq!(&clips[blob], bytes);
            assert_eq!(&sha256_hex(bytes), blob);
        }
        let mut ids = HashSet::new();
        for lesson in &imported.project.lessons {
            assert!(ids.insert(&lesson.id));
            for entry in &lesson.entries {
                assert!(ids.insert(&entry.id));
            }
        }
    }

    #[test]
    fn write_adjusts_what_the_desktop_app_would_reject() {
        let mut project = project(Vec::new());
        project.pdf.font_size = f32::NAN;
        project.gaps.same_participant_ms = 90_000;
        let imported = read(&write(&project, &mut |_| unreachable!()).unwrap()).unwrap();
        assert_eq!(imported.project.lessons.len(), 1);
        assert_eq!(imported.project.pdf.font_size, 15.0);
        assert_eq!(imported.project.gaps.same_participant_ms, AudioGaps::MAX_MS);

        let (mut project, clips) = full_example();
        project.lessons[0].entries[1].language = Language::L1;
        project.lessons[0].entries[1].trim = Some(Trim { start: 9, end: 9 });
        project.lessons[0].entries[0].trim = Some(Trim { start: 1, end: 2 });
        project.lessons[0].entries[0].audio = None;
        project.pdf.font_size = 99.0;
        let imported = read(&write(&project, &mut loader(&clips)).unwrap()).unwrap();
        let entries = &imported.project.lessons[0].entries;
        assert_eq!(entries[1].language, Language::Latin);
        assert_eq!(entries[1].trim, None);
        assert_eq!(entries[0].trim, None);
        assert_eq!(imported.project.pdf.font_size, 30.0);
        assert_eq!(imported.blobs.len(), 1);

        let error = write(&project, &mut |_| Err("gone".into())).unwrap_err();
        assert_eq!(error, "Lesson ‘First’, entry 2: gone");
    }

    #[test]
    fn reads_every_archive_version() {
        let (project, clips) = full_example();
        let v5 = disk(&project, &clips);
        let v1 = || disk(&project, &clips).project;
        let defaults = {
            let mut expected = without_ids(project.clone());
            expected.gaps = AudioGaps::default();
            for entry in &mut expected.lessons[0].entries {
                entry.trim = None;
                entry.lemmas.clear();
            }
            expected
        };
        let with_gaps = {
            let mut expected = defaults.clone();
            expected.gaps = project.gaps;
            expected
        };
        let with_trims = {
            let mut expected = with_gaps.clone();
            expected.lessons[0].entries[1].trim = project.lessons[0].entries[1].trim;
            expected
        };
        let same_participant_ms = project.gaps.same_participant_ms;
        let different_participant_ms = project.gaps.different_participant_ms;
        let cases = [
            (archive(1, &v1()), defaults),
            (
                archive(
                    2,
                    &DiskProjectV2 {
                        project: v1(),
                        same_participant_ms,
                        different_participant_ms,
                    },
                ),
                with_gaps.clone(),
            ),
            (
                archive(
                    3,
                    &DiskProjectV3 {
                        project: v1(),
                        same_participant_ms,
                        different_participant_ms,
                        word_cache: cache(),
                    },
                ),
                with_gaps,
            ),
            (
                archive(
                    4,
                    &DiskProjectV4 {
                        project: v1(),
                        same_participant_ms,
                        different_participant_ms,
                        word_cache: cache(),
                        trims: disk(&project, &clips).trims,
                    },
                ),
                with_trims,
            ),
            (
                archive(
                    5,
                    &DiskProjectV5 {
                        word_cache: cache(),
                        ..v5
                    },
                ),
                without_ids(project.clone()),
            ),
        ];
        for (version, (bytes, expected)) in cases.into_iter().enumerate() {
            let imported = read(&bytes).unwrap_or_else(|e| panic!("version {}: {e}", version + 1));
            assert_eq!(
                without_ids(imported.project),
                expected,
                "version {}",
                version + 1
            );
            assert_eq!(imported.blobs.len(), 2);
        }
    }

    #[test]
    fn identical_clips_are_stored_once() {
        let (mut project, clips) = full_example();
        let first = project.lessons[0].entries[0].clone();
        project.lessons[1].entries = vec![first.clone(), first];
        let imported = read(&write(&project, &mut loader(&clips)).unwrap()).unwrap();
        assert_eq!(imported.blobs.len(), 2);
        let audio = |lesson: usize, entry: usize| {
            imported.project.lessons[lesson].entries[entry]
                .audio
                .clone()
        };
        assert_eq!(audio(1, 0), audio(0, 0));
        assert_eq!(audio(1, 1), audio(0, 0));
    }

    #[test]
    fn crops_are_clamped_to_the_clip_and_invalid_ones_are_rejected() {
        let (project, clips) = full_example();
        let with_trims = |trims: Vec<DiskTrim>| {
            archive(
                5,
                &DiskProjectV5 {
                    trims,
                    ..disk(&project, &clips)
                },
            )
        };
        let trim = |lesson, entry, start, end| DiskTrim {
            lesson,
            entry,
            start,
            end,
        };
        // The student clip has 50 frames, the teacher clip 100.
        let imported = read(&with_trims(vec![trim(0, 1, 30, 90), trim(0, 0, 0, 500)])).unwrap();
        let entries = &imported.project.lessons[0].entries;
        assert_eq!(entries[1].trim, Some(Trim { start: 30, end: 50 }));
        assert_eq!(entries[0].trim, None);
        let imported = read(&with_trims(vec![trim(0, 1, 70, 90)])).unwrap();
        assert_eq!(
            imported.project.lessons[0].entries[1].trim,
            Some(Trim { start: 49, end: 50 })
        );
        for invalid in [
            trim(0, 9, 5, 90),
            trim(0, 1, 90, 5),
            trim(0, 1, 5, 5),
            trim(3, 0, 1, 2),
        ] {
            let error = read(&with_trims(vec![invalid])).unwrap_err();
            assert!(error.contains("crop"), "{error}");
        }
        let mut disk = disk(&project, &clips);
        disk.project.lessons[0].entries[1].audio = None;
        disk.trims = vec![trim(0, 1, 5, 10)];
        let imported = read(&archive(5, &disk)).unwrap();
        assert_eq!(imported.project.lessons[0].entries[1].trim, None);
    }

    #[test]
    fn rejects_invalid_lemma_choices_and_damaged_audio() {
        let (project, clips) = full_example();
        let mut disk = disk(&project, &clips);
        disk.lemma_choices[0].entry = 5;
        assert!(
            read(&archive(5, &disk))
                .unwrap_err()
                .contains("lemma choice")
        );
        let mut disk = self::disk(&project, &clips);
        disk.project.lessons[0].entries[1].audio = Some(b"not a wav".to_vec());
        let error = read(&archive(5, &disk)).unwrap_err();
        assert!(
            error.starts_with("Lesson 1, entry 2: Invalid WAV"),
            "{error}"
        );
        disk.project.lessons[0].entries[1].audio = Some(wav(8000, 1, 0, |_, _| 0));
        assert!(read(&archive(5, &disk)).unwrap_err().contains("empty"));
    }

    #[test]
    fn rejects_invalid_headers_and_archives() {
        let (project, clips) = full_example();
        let mut bytes = write(&project, &mut loader(&clips)).unwrap();
        bytes[8..12].copy_from_slice(&99u32.to_le_bytes());
        assert!(read(&bytes).unwrap_err().contains("version"));
        bytes[8..12].copy_from_slice(&0u32.to_le_bytes());
        assert!(read(&bytes).unwrap_err().contains("version"));
        bytes[8..12].copy_from_slice(&ARCHIVE_VERSION.to_le_bytes());
        bytes[12] = 1;
        assert!(read(&bytes).unwrap_err().contains("flags"));
        bytes[12] = 0;
        // An older version's header on a newer layout.
        bytes[8..12].copy_from_slice(&4u32.to_le_bytes());
        assert!(read(&bytes).is_err());
        bytes[8..12].copy_from_slice(&ARCHIVE_VERSION.to_le_bytes());
        let length = bytes.len();
        bytes[length / 2..].fill(0xff);
        assert!(
            read(&bytes)
                .unwrap_err()
                .contains("Invalid project archive")
        );
        bytes.truncate(HEADER_SIZE + 1);
        assert!(read(&bytes).is_err());
        assert!(read(&bytes[..12]).unwrap_err().contains("Truncated"));
        assert!(read(b"not a project").unwrap_err().contains("Unrecognized"));
        assert!(read(b"").is_err());
    }

    #[test]
    fn rejects_invalid_semantic_values() {
        let example = example();
        let no_clips = HashMap::new();
        let check = |change: &dyn Fn(&mut DiskProject), message: &str| {
            let mut disk = disk(&example, &no_clips).project;
            change(&mut disk);
            let error = read(&archive(1, &disk)).unwrap_err();
            assert!(error.contains(message), "{error}");
        };
        check(
            &|disk| disk.lessons[0].entries[0].participant = 9,
            "participant",
        );
        check(&|disk| disk.lessons[0].entries[0].language = 2, "language");
        check(
            &|disk| disk.lessons[0].entries[1].language = 0,
            "Lesson 1, entry 2: student entries must be Latin.",
        );
        check(&|disk| disk.export_paper = 4, "paper");
        check(&|disk| disk.export_font_size = f32::NAN, "font size");
        check(&|disk| disk.export_font_size = 31.0, "font size");
        check(&|disk| disk.lessons.clear(), "at least one lesson");
        let disk = DiskProjectV2 {
            project: self::disk(&example, &no_clips).project,
            same_participant_ms: 60_001,
            different_participant_ms: 0,
        };
        assert!(read(&archive(2, &disk)).unwrap_err().contains("gaps"));
    }

    #[test]
    fn old_export_settings_are_dropped() {
        let mut disk = disk(&example(), &HashMap::new()).project;
        disk.export_directory = Some("/home/teacher/exports".into());
        disk.lessons[1].expanded = false;
        let imported = read(&archive(1, &disk)).unwrap();
        assert_eq!(without_ids(imported.project), without_ids(example()));
    }
}
