//! The collaborative project document (docs/DESIGN.md, "The collaborative
//! document") read into and written from [`loquaris_core::Project`] with yrs.
//! The browser's `web/src/doc/schema.ts` is the other implementation of the
//! same encoding rules; the two must agree.
//!
//! Reading tolerates whatever a client may have written: missing fields take
//! their defaults, numbers may be integers or floats, and entries of deleted
//! lessons are skipped. Writing changes only what differs, so restoring a
//! version into a live document reaches collaborators as ordinary edits, and
//! fields this server does not know are left alone.
use std::collections::{HashMap, HashSet};

use anyhow::Context;
use loquaris_core::{
    AudioGaps, AudioRef, Entry, Language, LemmaChoice, Lesson, Paper, Participant, PdfSettings,
    Project, Trim, order,
};
use yrs::types::ToJson;
use yrs::updates::decoder::Decode;
use yrs::{
    Any, Doc, GetString, Map, MapPrelim, MapRef, Number, Out, ReadTxn, StateVector, Text,
    TextPrelim, TextRef, Transact, TransactionMut, Update, WriteTxn,
};

pub const SCHEMA: i64 = 1;

const META: &str = "meta";
const LESSONS: &str = "lessons";
const ENTRIES: &str = "entries";

/// Project settings stored in `meta`.
#[derive(Clone, Debug, PartialEq)]
pub struct Meta {
    pub name: String,
    pub l1_name: String,
    pub gaps: AudioGaps,
    pub pdf: PdfSettings,
}

/// A lesson as stored, with its fractional order key.
#[derive(Clone, Debug, PartialEq)]
pub struct LessonRow {
    pub id: String,
    pub name: String,
    pub order: String,
}

/// An entry as stored: its lesson and order key beside the content.
#[derive(Clone, Debug, PartialEq)]
pub struct EntryRow {
    pub lesson: String,
    pub order: String,
    pub entry: Entry,
}

/// The document's contents in stored form, keeping order keys, so a version
/// can be written back exactly. [`Contents::project`] turns it into script
/// order.
#[derive(Clone, Debug, PartialEq)]
pub struct Contents {
    pub meta: Meta,
    pub lessons: Vec<LessonRow>,
    pub entries: Vec<EntryRow>,
}

impl Contents {
    /// Lays out a project with fresh order keys (for new and imported
    /// projects). Lessons and entries without an ID get one.
    pub fn from_project(project: &Project) -> Self {
        let id_or_new = |id: &str| if id.is_empty() { crate::util::new_id() } else { id.to_owned() };
        let lesson_keys = order::keys_between(None, None, project.lessons.len())
            .expect("keys between the ends always exist");
        let mut lessons = Vec::new();
        let mut entries = Vec::new();
        for (lesson, key) in project.lessons.iter().zip(lesson_keys) {
            let lesson_id = id_or_new(&lesson.id);
            let entry_keys = order::keys_between(None, None, lesson.entries.len())
                .expect("keys between the ends always exist");
            for (entry, key) in lesson.entries.iter().zip(entry_keys) {
                let mut entry = entry.clone();
                entry.id = id_or_new(&entry.id);
                entries.push(EntryRow {
                    lesson: lesson_id.clone(),
                    order: key,
                    entry,
                });
            }
            lessons.push(LessonRow {
                id: lesson_id,
                name: lesson.name.clone(),
                order: key,
            });
        }
        Self {
            meta: Meta {
                name: project.name.clone(),
                l1_name: project.l1_name.clone(),
                gaps: project.gaps,
                pdf: project.pdf,
            },
            lessons,
            entries,
        }
    }

    /// Lessons and entries in script order: sorted by `(order, id)` comparing
    /// plain strings, entries of missing lessons dropped.
    pub fn project(&self) -> Project {
        let mut lessons: Vec<&LessonRow> = self.lessons.iter().collect();
        lessons.sort_by(|a, b| (&a.order, &a.id).cmp(&(&b.order, &b.id)));
        let mut by_lesson: HashMap<&str, Vec<&EntryRow>> = HashMap::new();
        for row in &self.entries {
            by_lesson.entry(&row.lesson).or_default().push(row);
        }
        let lessons = lessons
            .into_iter()
            .map(|lesson| {
                let mut rows = by_lesson.remove(lesson.id.as_str()).unwrap_or_default();
                rows.sort_by(|a, b| (&a.order, &a.entry.id).cmp(&(&b.order, &b.entry.id)));
                Lesson {
                    id: lesson.id.clone(),
                    name: lesson.name.clone(),
                    entries: rows.into_iter().map(|row| row.entry.clone()).collect(),
                }
            })
            .collect();
        Project {
            name: self.meta.name.clone(),
            l1_name: self.meta.l1_name.clone(),
            gaps: self.meta.gaps,
            pdf: self.meta.pdf,
            lessons,
        }
    }
}

/// A new document holding `project`, laid out with fresh order keys.
pub fn new_doc(project: &Project) -> Doc {
    let doc = Doc::new();
    write(&mut doc.transact_mut(), &Contents::from_project(project));
    doc
}

/// The initial document of a new project: settings at their defaults and one
/// empty lesson.
pub fn initial_project(name: &str, l1_name: &str) -> Project {
    Project {
        name: name.to_owned(),
        l1_name: l1_name.to_owned(),
        gaps: AudioGaps::default(),
        pdf: PdfSettings::default(),
        lessons: vec![Lesson {
            id: crate::util::new_id(),
            name: String::new(),
            entries: Vec::new(),
        }],
    }
}

/// The full state of a document as one Yjs update (v1 encoding).
pub fn encode_state(doc: &Doc) -> Vec<u8> {
    doc.transact().encode_state_as_update_v1(&StateVector::default())
}

/// A document from updates in v1 encoding, applied in order.
pub fn decode<'a>(updates: impl IntoIterator<Item = &'a [u8]>) -> anyhow::Result<Doc> {
    let doc = Doc::new();
    {
        let mut txn = doc.transact_mut();
        for update in updates {
            let update = Update::decode_v1(update).context("malformed document update")?;
            txn.apply_update(update).context("cannot apply document update")?;
        }
    }
    Ok(doc)
}

pub fn read_project<T: ReadTxn>(txn: &T) -> Project {
    read(txn).project()
}

/// `meta.name`, for keeping the project listing in sync.
pub fn read_name<T: ReadTxn>(txn: &T) -> String {
    txn.get_map(META)
        .and_then(|meta| string(&meta, txn, "name"))
        .unwrap_or_default()
}

pub fn write_name(txn: &mut TransactionMut, name: &str) {
    let meta = txn.get_or_insert_map(META);
    set(&meta, txn, "name", Any::from(name));
}

pub fn read<T: ReadTxn>(txn: &T) -> Contents {
    let meta = match txn.get_map(META) {
        Some(meta) => read_meta(&meta, txn),
        None => read_meta_defaults(),
    };
    let mut lessons = Vec::new();
    if let Some(map) = txn.get_map(LESSONS) {
        for (id, value) in map.iter(txn) {
            let Out::YMap(lesson) = value else { continue };
            lessons.push(LessonRow {
                id: id.to_owned(),
                name: text(&lesson, txn, "name"),
                order: string(&lesson, txn, "order").unwrap_or_default(),
            });
        }
    }
    let mut entries = Vec::new();
    if let Some(map) = txn.get_map(ENTRIES) {
        for (id, value) in map.iter(txn) {
            let Out::YMap(entry) = value else { continue };
            entries.push(read_entry(id, &entry, txn));
        }
    }
    Contents {
        meta,
        lessons,
        entries,
    }
}

fn read_meta_defaults() -> Meta {
    Meta {
        name: String::new(),
        l1_name: "English".into(),
        gaps: AudioGaps::default(),
        pdf: PdfSettings::default(),
    }
}

fn read_meta<T: ReadTxn>(meta: &MapRef, txn: &T) -> Meta {
    let defaults = read_meta_defaults();
    let gap = |key| {
        number(meta, txn, key).map(|ms| ms.round().clamp(0.0, f64::from(AudioGaps::MAX_MS)) as u32)
    };
    let paper = match string(meta, txn, "paper").as_deref() {
        Some("letter") => Paper::Letter,
        Some("legal") => Paper::Legal,
        Some("a3") => Paper::A3,
        _ => Paper::A4,
    };
    let font_size = number(meta, txn, "fontSize").map_or(defaults.pdf.font_size, |size| {
        (size as f32).clamp(PdfSettings::MIN_FONT_SIZE, PdfSettings::MAX_FONT_SIZE)
    });
    Meta {
        name: string(meta, txn, "name").unwrap_or(defaults.name),
        l1_name: string(meta, txn, "l1Name").unwrap_or(defaults.l1_name),
        gaps: AudioGaps {
            same_participant_ms: gap("gapSameMs").unwrap_or(defaults.gaps.same_participant_ms),
            different_participant_ms: gap("gapDifferentMs")
                .unwrap_or(defaults.gaps.different_participant_ms),
        },
        pdf: PdfSettings { paper, font_size },
    }
}

fn read_entry<T: ReadTxn>(id: &str, entry: &MapRef, txn: &T) -> EntryRow {
    let participant = match string(entry, txn, "participant").as_deref() {
        Some("student") => Participant::Student,
        _ => Participant::Teacher,
    };
    // Students always speak Latin, whatever a client wrote.
    let language = match string(entry, txn, "language").as_deref() {
        Some("l1") if participant == Participant::Teacher => Language::L1,
        _ => Language::Latin,
    };
    let audio = match any(entry, txn, "audio") {
        Some(Any::Map(audio)) => read_audio(&audio),
        _ => None,
    };
    let trim = match any(entry, txn, "trim") {
        Some(Any::Map(trim)) => read_trim(&trim),
        _ => None,
    };
    let mut lemmas = Vec::new();
    if let Some(Out::YMap(map)) = entry.get(txn, "lemmas") {
        for (key, value) in map.iter(txn) {
            let Out::Any(Any::Map(choice)) = value else { continue };
            let Some((token, occurrence)) = key.rsplit_once('#') else { continue };
            let (Ok(occurrence), Some(Any::String(lemma_key))) =
                (occurrence.parse(), choice.get("key"))
            else {
                continue;
            };
            lemmas.push(LemmaChoice {
                token: token.to_owned(),
                occurrence,
                lemma_key: lemma_key.to_string(),
                extension: matches!(choice.get("extension"), Some(Any::Bool(true))),
            });
        }
    }
    lemmas.sort_by(|a, b| (&a.token, a.occurrence).cmp(&(&b.token, b.occurrence)));
    EntryRow {
        lesson: string(entry, txn, "lesson").unwrap_or_default(),
        order: string(entry, txn, "order").unwrap_or_default(),
        entry: Entry {
            id: id.to_owned(),
            participant,
            language,
            text: text(entry, txn, "text"),
            audio,
            trim,
            lemmas,
        },
    }
}

fn read_audio(audio: &HashMap<String, Any>) -> Option<AudioRef> {
    let Some(Any::String(blob)) = audio.get("blob") else { return None };
    Some(AudioRef {
        blob: blob.to_string(),
        sample_rate: any_u32(audio.get("sampleRate")?)?,
        frames: any_u32(audio.get("frames")?)?,
        channels: audio.get("channels").and_then(any_u32).map_or(1, |c| c.clamp(1, 255) as u16),
    })
}

fn read_trim(trim: &HashMap<String, Any>) -> Option<Trim> {
    let start = any_u32(trim.get("start")?)?;
    let end = any_u32(trim.get("end")?)?;
    (start < end).then_some(Trim { start, end })
}

fn any_u32(value: &Any) -> Option<u32> {
    match value {
        Any::Number(n) => n.as_f64().map(|n| n.round().clamp(0.0, f64::from(u32::MAX)) as u32),
        _ => None,
    }
}

fn any<T: ReadTxn>(map: &MapRef, txn: &T, key: &str) -> Option<Any> {
    match map.get(txn, key)? {
        Out::Any(value) => Some(value),
        _ => None,
    }
}

fn string<T: ReadTxn>(map: &MapRef, txn: &T, key: &str) -> Option<String> {
    match any(map, txn, key)? {
        Any::String(value) => Some(value.to_string()),
        _ => None,
    }
}

fn number<T: ReadTxn>(map: &MapRef, txn: &T, key: &str) -> Option<f64> {
    match any(map, txn, key)? {
        Any::Number(value) => value.as_f64().filter(|n| n.is_finite()),
        _ => None,
    }
}

/// A `Y.Text` field; a plain string is accepted too.
fn text<T: ReadTxn>(map: &MapRef, txn: &T, key: &str) -> String {
    match map.get(txn, key) {
        Some(Out::YText(text)) => text.get_string(txn),
        Some(Out::Any(Any::String(value))) => value.to_string(),
        _ => String::new(),
    }
}

/// Makes the document hold `target`, changing only what differs: fields
/// with other values are set, texts get a minimal splice, objects missing
/// from `target` are deleted and new ones created. Unknown fields are kept.
pub fn write(txn: &mut TransactionMut, target: &Contents) {
    write_meta(txn, &target.meta);

    let lessons = txn.get_or_insert_map(LESSONS);
    let keep: HashSet<&str> = target.lessons.iter().map(|lesson| lesson.id.as_str()).collect();
    remove_others(&lessons, txn, &keep);
    for row in &target.lessons {
        let lesson = nested_map(&lessons, txn, &row.id);
        set_text(&lesson, txn, "name", &row.name);
        set(&lesson, txn, "order", Any::from(row.order.as_str()));
    }

    let entries = txn.get_or_insert_map(ENTRIES);
    let keep: HashSet<&str> = target.entries.iter().map(|row| row.entry.id.as_str()).collect();
    remove_others(&entries, txn, &keep);
    for row in &target.entries {
        let entry = nested_map(&entries, txn, &row.entry.id);
        write_entry(&entry, txn, row);
    }
}

fn write_meta(txn: &mut TransactionMut, meta: &Meta) {
    let map = txn.get_or_insert_map(META);
    let paper = match meta.pdf.paper {
        Paper::A4 => "a4",
        Paper::Letter => "letter",
        Paper::Legal => "legal",
        Paper::A3 => "a3",
    };
    set(&map, txn, "name", Any::from(meta.name.as_str()));
    set(&map, txn, "l1Name", Any::from(meta.l1_name.as_str()));
    set(&map, txn, "gapSameMs", Any::from(meta.gaps.same_participant_ms));
    set(&map, txn, "gapDifferentMs", Any::from(meta.gaps.different_participant_ms));
    set(&map, txn, "paper", Any::from(paper));
    set(&map, txn, "fontSize", Any::Number(Number::try_i64(f64::from(meta.pdf.font_size))));
    if !map.contains_key(txn, "schema") {
        map.insert(txn, "schema", Any::from(SCHEMA));
    }
}

fn write_entry(entry: &MapRef, txn: &mut TransactionMut, row: &EntryRow) {
    let content = &row.entry;
    let participant = match content.participant {
        Participant::Teacher => "teacher",
        Participant::Student => "student",
    };
    let language = match content.language {
        Language::L1 => "l1",
        Language::Latin => "latin",
    };
    let audio = content.audio.as_ref().map_or(Any::Null, |audio| {
        Any::from(HashMap::from([
            ("blob".to_owned(), Any::from(audio.blob.as_str())),
            ("sampleRate".to_owned(), Any::from(audio.sample_rate)),
            ("frames".to_owned(), Any::from(audio.frames)),
            ("channels".to_owned(), Any::from(audio.channels)),
        ]))
    });
    let trim = content.trim.map_or(Any::Null, |trim| {
        Any::from(HashMap::from([
            ("start".to_owned(), Any::from(trim.start)),
            ("end".to_owned(), Any::from(trim.end)),
        ]))
    });
    set(entry, txn, "lesson", Any::from(row.lesson.as_str()));
    set(entry, txn, "order", Any::from(row.order.as_str()));
    set(entry, txn, "participant", Any::from(participant));
    set(entry, txn, "language", Any::from(language));
    set_text(entry, txn, "text", &content.text);
    set(entry, txn, "audio", audio);
    set(entry, txn, "trim", trim);

    let lemmas = nested_map(entry, txn, "lemmas");
    let choices: HashMap<String, Any> = content
        .lemmas
        .iter()
        .map(|choice| {
            let value = Any::from(HashMap::from([
                ("key".to_owned(), Any::from(choice.lemma_key.as_str())),
                ("extension".to_owned(), Any::from(choice.extension)),
            ]));
            (format!("{}#{}", choice.token, choice.occurrence), value)
        })
        .collect();
    remove_others(&lemmas, txn, &choices.keys().map(String::as_str).collect());
    for (key, value) in choices {
        set(&lemmas, txn, &key, value);
    }
}

/// Sets a plain field unless it already holds an equal value.
fn set(map: &MapRef, txn: &mut TransactionMut, key: &str, value: Any) {
    if !matches!(map.get(txn, key), Some(Out::Any(current)) if current == value) {
        map.insert(txn, key, value);
    }
}

/// The nested `Y.Map` under `key`, created (or replacing a non-map) if needed.
fn nested_map(map: &MapRef, txn: &mut TransactionMut, key: &str) -> MapRef {
    match map.get(txn, key) {
        Some(Out::YMap(nested)) => nested,
        _ => map.insert(txn, key, MapPrelim::default()),
    }
}

fn remove_others(map: &MapRef, txn: &mut TransactionMut, keep: &HashSet<&str>) {
    let stale: Vec<String> = map.keys(txn).filter(|key| !keep.contains(key)).map(str::to_owned).collect();
    for key in stale {
        map.remove(txn, &key);
    }
}

/// Makes the `Y.Text` under `key` read `value` by replacing only the part
/// between the common prefix and suffix, so concurrent edits elsewhere in
/// the text survive.
fn set_text(map: &MapRef, txn: &mut TransactionMut, key: &str, value: &str) {
    let text: TextRef = match map.get(txn, key) {
        Some(Out::YText(text)) => text,
        _ => {
            map.insert(txn, key, TextPrelim::new(value));
            return;
        }
    };
    let current = text.get_string(txn);
    if current == value {
        return;
    }
    let (start, old_end, new_end) = splice(&current, value);
    // Offsets are in the document's unit, UTF-8 bytes (yrs's default), and
    // `splice` keeps them on character boundaries.
    if old_end > start {
        text.remove_range(txn, start as u32, (old_end - start) as u32);
    }
    if new_end > start {
        text.insert(txn, start as u32, &value[start..new_end]);
    }
}

/// The differing middle of `old` and `new` as byte offsets: `old[start..
/// old_end]` becomes `new[start..new_end]`.
fn splice(old: &str, new: &str) -> (usize, usize, usize) {
    let start = old
        .char_indices()
        .zip(new.chars())
        .find(|((_, a), b)| a != b)
        .map_or_else(|| old.len().min(new.len()), |((i, _), _)| i);
    // Make sure start is a boundary in both (the prefix is identical, so it is).
    let suffix = old[start..]
        .chars()
        .rev()
        .zip(new[start..].chars().rev())
        .take_while(|(a, b)| a == b)
        .map(|(a, _)| a.len_utf8())
        .sum::<usize>();
    (start, old.len() - suffix, new.len() - suffix)
}

/// The document as JSON, for debugging and tests.
pub fn to_json(doc: &Doc) -> serde_json::Value {
    let txn = doc.transact();
    let mut root = serde_json::Map::new();
    for name in [META, LESSONS, ENTRIES] {
        if let Some(map) = txn.get_map(name) {
            let value = serde_json::to_value(map.to_json(&txn)).unwrap_or_default();
            root.insert(name.to_owned(), value);
        }
    }
    serde_json::Value::Object(root)
}

#[cfg(test)]
mod tests {
    use super::*;
    use yrs::Array;

    fn entry(id: &str, text: &str) -> Entry {
        Entry {
            id: id.into(),
            participant: Participant::Teacher,
            language: Language::Latin,
            text: text.into(),
            audio: None,
            trim: None,
            lemmas: Vec::new(),
        }
    }

    fn sample() -> Project {
        Project {
            name: "Lingua Latīna".into(),
            l1_name: "English".into(),
            gaps: AudioGaps {
                same_participant_ms: 150,
                different_participant_ms: 750,
            },
            pdf: PdfSettings {
                paper: Paper::Letter,
                font_size: 12.5,
            },
            lessons: vec![
                Lesson {
                    id: "L1aaaaaaaaaaaaaa".into(),
                    name: "Prīma".into(),
                    entries: vec![
                        Entry {
                            participant: Participant::Teacher,
                            language: Language::L1,
                            audio: Some(AudioRef {
                                blob: "ab".repeat(32),
                                sample_rate: 48_000,
                                frames: 96_000,
                                channels: 2,
                            }),
                            trim: Some(Trim {
                                start: 100,
                                end: 90_000,
                            }),
                            ..entry("E1aaaaaaaaaaaaaa", "Good morning.")
                        },
                        Entry {
                            participant: Participant::Student,
                            lemmas: vec![
                                LemmaChoice {
                                    token: "bellum".into(),
                                    occurrence: 0,
                                    lemma_key: "bellum1".into(),
                                    extension: false,
                                },
                                LemmaChoice {
                                    token: "bellum".into(),
                                    occurrence: 1,
                                    lemma_key: "bellus".into(),
                                    extension: true,
                                },
                            ],
                            ..entry("E2aaaaaaaaaaaaaa", "Salvē, magister! bellum bellum")
                        },
                    ],
                },
                Lesson {
                    id: "L2aaaaaaaaaaaaaa".into(),
                    name: String::new(),
                    entries: vec![entry("E3aaaaaaaaaaaaaa", "Valē.")],
                },
            ],
        }
    }

    #[test]
    fn project_round_trips() {
        let project = sample();
        let doc = new_doc(&project);
        assert_eq!(read_project(&doc.transact()), project);
        // And through the binary encoding.
        let copy = decode([encode_state(&doc).as_slice()]).unwrap();
        assert_eq!(read_project(&copy.transact()), project);
    }

    #[test]
    fn encoding_follows_the_schema() {
        let doc = new_doc(&sample());
        let json = to_json(&doc);
        assert_eq!(json["meta"]["schema"], 1);
        assert_eq!(json["meta"]["paper"], "letter");
        assert_eq!(json["meta"]["gapSameMs"], 150);
        assert_eq!(json["meta"]["fontSize"], 12.5);
        let entry = &json["entries"]["E1aaaaaaaaaaaaaa"];
        assert_eq!(entry["lesson"], "L1aaaaaaaaaaaaaa");
        assert_eq!(entry["language"], "l1");
        assert_eq!(entry["audio"]["sampleRate"], 48_000);
        assert_eq!(entry["trim"]["end"], 90_000);
        assert_eq!(json["entries"]["E3aaaaaaaaaaaaaa"]["audio"], serde_json::Value::Null);
        let lemma = &json["entries"]["E2aaaaaaaaaaaaaa"]["lemmas"]["bellum#1"];
        assert_eq!(lemma["key"], "bellus");
        assert_eq!(lemma["extension"], true);
        // Nested types are real Y types, as the browser expects.
        let txn = doc.transact();
        let entries = txn.get_map(ENTRIES).unwrap();
        let Some(Out::YMap(e1)) = entries.get(&txn, "E1aaaaaaaaaaaaaa") else { panic!() };
        assert!(matches!(e1.get(&txn, "text"), Some(Out::YText(_))));
        assert!(matches!(e1.get(&txn, "lemmas"), Some(Out::YMap(_))));
    }

    /// A document as the browser might write it: floats for numbers, order
    /// keys out of insertion order, ties, an orphaned entry, a missing field.
    #[test]
    fn reads_tolerantly() {
        let doc = Doc::new();
        {
            let mut txn = doc.transact_mut();
            let meta = txn.get_or_insert_map(META);
            meta.insert(&mut txn, "name", "P");
            meta.insert(&mut txn, "gapSameMs", Any::Number(Number::Float(120.0)));
            meta.insert(&mut txn, "fontSize", 99.0);
            let lessons = txn.get_or_insert_map(LESSONS);
            for (id, order) in [("b", "a1"), ("a", "a1"), ("c", "a0")] {
                let lesson = lessons.insert(&mut txn, id, MapPrelim::default());
                lesson.insert(&mut txn, "name", TextPrelim::new(id));
                lesson.insert(&mut txn, "order", order);
            }
            let entries = txn.get_or_insert_map(ENTRIES);
            for (id, lesson, order) in [("e2", "a", "a0V"), ("e1", "a", "a0"), ("orphan", "gone", "a0")] {
                let entry = entries.insert(&mut txn, id, MapPrelim::default());
                entry.insert(&mut txn, "lesson", lesson);
                entry.insert(&mut txn, "order", order);
                entry.insert(&mut txn, "text", TextPrelim::new(id));
            }
            let Some(Out::YMap(e1)) = entries.get(&txn, "e1") else { panic!() };
            e1.insert(&mut txn, "participant", "student");
            e1.insert(&mut txn, "language", "l1");
            let audio = Any::from(HashMap::from([
                ("blob".to_owned(), Any::from("x")),
                ("sampleRate".to_owned(), Any::Number(Number::Float(44_100.0))),
                ("frames".to_owned(), Any::Number(Number::Float(10.0))),
            ]));
            e1.insert(&mut txn, "audio", audio);
            // Something a future client might add.
            e1.insert(&mut txn, "unknown", yrs::ArrayPrelim::default());
        }
        let project = read_project(&doc.transact());
        assert_eq!(project.gaps.same_participant_ms, 120);
        assert_eq!(project.gaps.different_participant_ms, 500);
        assert_eq!(project.pdf.font_size, PdfSettings::MAX_FONT_SIZE);
        assert_eq!(project.l1_name, "English");
        let ids: Vec<_> = project.lessons.iter().map(|l| l.id.as_str()).collect();
        assert_eq!(ids, ["c", "a", "b"]);
        let a = &project.lessons[1];
        let entry_ids: Vec<_> = a.entries.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(entry_ids, ["e1", "e2"]);
        let e1 = &a.entries[0];
        assert_eq!(e1.participant, Participant::Student);
        assert_eq!(e1.language, Language::Latin);
        assert_eq!(e1.audio.as_ref().unwrap().sample_rate, 44_100);
        assert_eq!(e1.audio.as_ref().unwrap().channels, 1);
        assert!(project.lessons.iter().all(|l| l.entries.iter().all(|e| e.id != "orphan")));

        // Writing back keeps the unknown field.
        let contents = read(&doc.transact());
        write(&mut doc.transact_mut(), &contents);
        let txn = doc.transact();
        let entries = txn.get_map(ENTRIES).unwrap();
        let Some(Out::YMap(e1)) = entries.get(&txn, "e1") else { panic!() };
        assert!(matches!(e1.get(&txn, "unknown"), Some(Out::YArray(a)) if a.len(&txn) == 0));
    }

    #[test]
    fn splices_minimally() {
        assert_eq!(splice("salve", "salvē"), (4, 5, 6));
        assert_eq!(splice("abc", "abc"), (3, 3, 3));
        assert_eq!(splice("aXc", "aYYc"), (1, 2, 3));
        assert_eq!(splice("", "new"), (0, 0, 3));
        assert_eq!(splice("aaa", "aa"), (2, 3, 2));
        assert_eq!(splice("ēē", "ē"), (2, 4, 2));
    }

    /// Writing an old state into a document that moved on produces small
    /// changes that merge with a concurrent edit elsewhere in the same text.
    #[test]
    fn writing_merges_with_concurrent_edits() {
        let project = sample();
        let server = new_doc(&project);
        let old = read(&server.transact());

        // Someone edits; then a version restore and a concurrent edit race.
        {
            let mut txn = server.transact_mut();
            let mut changed = old.clone();
            changed.entries[0].entry.text = "Good morning, class.".into();
            changed.lessons.pop();
            changed.entries.retain(|row| row.lesson != "L2aaaaaaaaaaaaaa");
            write(&mut txn, &changed);
        }
        let client = decode([encode_state(&server).as_slice()]).unwrap();
        let before_client = client.transact().state_vector();
        {
            let txn = client.transact();
            let entries = txn.get_map(ENTRIES).unwrap();
            let Some(Out::YMap(e1)) = entries.get(&txn, "E1aaaaaaaaaaaaaa") else { panic!() };
            let Some(Out::YText(text)) = e1.get(&txn, "text") else { panic!() };
            drop(txn);
            let mut txn = client.transact_mut();
            text.insert(&mut txn, 0, ">> ");
        }
        let before_server = server.transact().state_vector();
        write(&mut server.transact_mut(), &old);

        let from_server = server.transact().encode_diff_v1(&before_server);
        let from_client = client.transact().encode_diff_v1(&before_client);
        server.transact_mut().apply_update(Update::decode_v1(&from_client).unwrap()).unwrap();
        client.transact_mut().apply_update(Update::decode_v1(&from_server).unwrap()).unwrap();

        let merged = read_project(&server.transact());
        assert_eq!(merged, read_project(&client.transact()));
        assert_eq!(merged.lessons.len(), 2);
        assert_eq!(merged.lessons[0].entries[0].text, ">> Good morning.");
        assert_eq!(merged.lessons[1].entries[0].text, "Valē.");
    }

    #[test]
    fn initial_project_has_one_lesson() {
        let doc = new_doc(&initial_project("Nova", "Deutsch"));
        let json = to_json(&doc);
        assert_eq!(json["meta"]["name"], "Nova");
        assert_eq!(json["meta"]["gapDifferentMs"], 500);
        assert_eq!(json["meta"]["paper"], "a4");
        assert_eq!(json["meta"]["fontSize"], 15);
        assert_eq!(json["lessons"].as_object().unwrap().len(), 1);
        assert_eq!(read_name(&doc.transact()), "Nova");
    }
}
