//! The analysis session behind the wasm interface, with JSON in and out so
//! it can be tested natively. Text offsets in results are UTF-16 code units,
//! as JavaScript strings use.
use std::ops::Range;

use loquaris_core::collo;
use loquaris_core::{
    AppliedReview, AudioGaps, EntryContent, Hit, LemmaChoice, LemmaInfo, LessonOutline,
    ProjectContent, ReviewFlag, ReviewKind, ReviewModel, SearchMode, SearchOptions, SearchResult,
    WordIndex,
};
use serde::{Deserialize, Serialize};

pub type Result<T> = std::result::Result<T, String>;

/// A word index and the requests the worker answers.
#[derive(Default)]
pub struct Session {
    index: WordIndex,
}

impl Session {
    pub fn new() -> Self {
        Self::default()
    }

    /// `{l1Name, lessons: [{id, entries: [entry]}]}`, entries as for
    /// [`set_entry`](Self::set_entry).
    pub fn set_project(&mut self, json: &str) -> Result<()> {
        let project: ProjectContent = parse(json)?;
        self.index.set_project(project);
        Ok(())
    }

    /// `[{id, entryIds}]`, lessons and their entries in script order.
    pub fn set_structure(&mut self, json: &str) -> Result<()> {
        let lessons: Vec<LessonOutline> = parse(json)?;
        self.index.set_structure(&lessons);
        Ok(())
    }

    /// `{id, participant: "teacher"|"student", language: "l1"|"latin", text,
    /// lemmas: [{token, occurrence, lemmaKey, extension}]}`.
    pub fn set_entry(&mut self, json: &str) -> Result<()> {
        let entry: EntryContent = parse(json)?;
        self.index.set_entry(entry);
        Ok(())
    }

    pub fn remove_entry(&mut self, id: &str) {
        self.index.remove_entry(id);
    }

    pub fn set_l1_name(&mut self, l1_name: &str) {
        self.index.set_l1_name(l1_name);
    }

    /// `{sameParticipantMs, differentParticipantMs}`, the playback gaps.
    pub fn set_gaps(&mut self, json: &str) -> Result<()> {
        let gaps: AudioGaps = parse(json)?;
        self.index.set_gaps(gaps);
        Ok(())
    }

    /// The spaced repetition flags in script order: `[{lemma, kind:
    /// {type: "inLesson", lessonId, entryId} | {type: "lesson", lessonId},
    /// lastHeardEntryId, lastHeardLessonId, elapsedSeconds,
    /// intervalSeconds}]`.
    pub fn spaced_repetition(&self) -> Result<String> {
        let flags = self.index.spaced_repetition();
        let dtos: Vec<_> = flags.iter().map(ReviewFlagDto::from).collect();
        to_json(&dtos)
    }

    /// Options are `{exactText?, unmarkedShort?, lemma?: {key, extension}}`.
    pub fn search(&self, query: &str, options_json: &str) -> Result<String> {
        let options: SearchOptionsDto = parse(options_json)?;
        let lemma = match &options.lemma {
            Some(lemma) => Some(
                collo_lemma(&lemma.key, lemma.extension)
                    .ok_or_else(|| format!("unknown lemma {:?}", lemma.key))?,
            ),
            None => None,
        };
        let result = self.index.search(
            query,
            &SearchOptions {
                exact_text: options.exact_text,
                unmarked_short: options.unmarked_short,
                lemma,
            },
        );
        to_json(&SearchResultDto::new(&self.index, &result))
    }

    /// `{entryId: [headword]}` for every entry with first-use lemmas.
    pub fn first_use(&self) -> Result<String> {
        let map: serde_json::Map<String, serde_json::Value> = self
            .index
            .all_first_uses()
            .map(|(id, lemmas)| {
                let headwords = lemmas
                    .into_iter()
                    .map(|lemma| loquaris_core::reader_form(lemma.headword).into())
                    .collect();
                (id.to_string(), serde_json::Value::Array(headwords))
            })
            .collect();
        to_json(&map)
    }

    /// `[lemma]` first used in the project in this lesson, in script order.
    pub fn new_in_lesson(&self, lesson_id: &str) -> Result<String> {
        let lemmas: Vec<_> = self
            .index
            .new_lemmas_in_lesson(lesson_id)
            .into_iter()
            .map(|lemma| LemmaDto::from(&self.index.lemma_info(lemma)))
            .collect();
        to_json(&lemmas)
    }

    /// The parsing review of a Latin entry, or `null`.
    pub fn review(&mut self, entry_id: &str) -> Result<String> {
        let review = self.index.review(entry_id);
        to_json(&review.map(ReviewDto::from))
    }

    /// Choices are `[readingIndex | null]`, one per review word. The result
    /// is `{text, lemmas, changed}`, or `null` if the entry's text is no
    /// longer `original`.
    pub fn apply_review(
        &mut self,
        entry_id: &str,
        original: &str,
        choices_json: &str,
    ) -> Result<String> {
        let choices: Vec<Option<usize>> = parse(choices_json)?;
        let applied = self.index.apply_review(entry_id, original, &choices);
        to_json(&applied.map(AppliedReviewDto::from))
    }

    pub fn macronize(&mut self, text: &str) -> String {
        self.index.macronize(text)
    }
}

fn parse<'a, T: Deserialize<'a>>(json: &'a str) -> Result<T> {
    serde_json::from_str(json).map_err(|error| error.to_string())
}

fn to_json(value: &impl Serialize) -> Result<String> {
    serde_json::to_string(value).map_err(|error| error.to_string())
}

fn collo_lemma(key: &str, extension: bool) -> Option<&'static collo::Lemma> {
    collo::find_lemma(key, extension)
}

/// Converts increasing byte offsets in one text to UTF-16 offsets, counting
/// only the text between successive offsets.
struct Utf16Offsets<'a> {
    text: &'a str,
    byte: usize,
    unit: usize,
}

impl<'a> Utf16Offsets<'a> {
    fn new(text: &'a str) -> Self {
        Self {
            text,
            byte: 0,
            unit: 0,
        }
    }

    fn at(&mut self, byte: usize) -> usize {
        if byte < self.byte {
            (self.byte, self.unit) = (0, 0);
        }
        self.unit += self.text[self.byte..byte].encode_utf16().count();
        self.byte = byte;
        self.unit
    }

    fn range(&mut self, range: &Range<usize>) -> (usize, usize) {
        (self.at(range.start), self.at(range.end))
    }
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct SearchOptionsDto {
    exact_text: bool,
    unmarked_short: bool,
    lemma: Option<LemmaRef>,
}

#[derive(Deserialize)]
struct LemmaRef {
    key: String,
    #[serde(default)]
    extension: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LemmaDto {
    key: &'static str,
    extension: bool,
    headword: String,
    translation: Option<&'static str>,
}

impl From<&LemmaInfo> for LemmaDto {
    fn from(info: &LemmaInfo) -> Self {
        Self {
            key: info.lemma.key,
            extension: info.lemma.extension,
            headword: info.headword.clone(),
            translation: info.translation,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ReviewFlagDto<'a> {
    lemma: LemmaDto,
    kind: ReviewKindDto<'a>,
    last_heard_entry_id: &'a str,
    last_heard_lesson_id: &'a str,
    elapsed_seconds: f64,
    interval_seconds: f64,
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum ReviewKindDto<'a> {
    #[serde(rename_all = "camelCase")]
    InLesson {
        lesson_id: &'a str,
        entry_id: &'a str,
    },
    #[serde(rename_all = "camelCase")]
    Lesson { lesson_id: &'a str },
}

impl<'a> From<&'a ReviewFlag> for ReviewFlagDto<'a> {
    fn from(flag: &'a ReviewFlag) -> Self {
        Self {
            lemma: LemmaDto::from(&flag.lemma),
            kind: match &flag.kind {
                ReviewKind::InLesson {
                    lesson_id,
                    entry_id,
                } => ReviewKindDto::InLesson {
                    lesson_id,
                    entry_id,
                },
                ReviewKind::Lesson { lesson_id } => ReviewKindDto::Lesson { lesson_id },
            },
            last_heard_entry_id: &flag.last_heard_entry_id,
            last_heard_lesson_id: &flag.last_heard_lesson_id,
            elapsed_seconds: flag.elapsed_seconds,
            interval_seconds: flag.interval_seconds,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HitDto<'a> {
    entry_id: &'a str,
    start: usize,
    end: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LessonCountDto<'a> {
    lesson_id: &'a str,
    teacher: usize,
    student: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ReadingDto<'a> {
    form: &'a str,
    morphology: Option<&'static str>,
    context_selected: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct QueryLemmaDto<'a> {
    #[serde(flatten)]
    lemma: LemmaDto,
    readings: Vec<ReadingDto<'a>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct QueryWordDto<'a> {
    text: &'a str,
    ruled_out: usize,
    lemmas: Vec<QueryLemmaDto<'a>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SearchResultDto<'a> {
    mode: &'static str,
    lemma: Option<LemmaDto>,
    hits: Vec<HitDto<'a>>,
    per_lesson: Vec<LessonCountDto<'a>>,
    lessons_with_hits: usize,
    analysis: Vec<QueryWordDto<'a>>,
}

impl<'a> SearchResultDto<'a> {
    fn new(index: &'a WordIndex, result: &'a SearchResult) -> Self {
        let lemma = match result.mode {
            SearchMode::Text => None,
            SearchMode::Lemma(lemma) => Some(LemmaDto::from(&index.lemma_info(lemma))),
        };
        Self {
            mode: if lemma.is_some() { "lemma" } else { "text" },
            lemma,
            hits: hits_dto(index, &result.hits),
            per_lesson: result
                .per_lesson
                .iter()
                .map(|lesson| LessonCountDto {
                    lesson_id: &lesson.lesson_id,
                    teacher: lesson.teacher,
                    student: lesson.student,
                })
                .collect(),
            lessons_with_hits: result.lessons_with_hits(),
            analysis: result
                .words
                .iter()
                .map(|word| QueryWordDto {
                    text: &word.text,
                    ruled_out: word.ruled_out,
                    lemmas: word
                        .lemmas
                        .iter()
                        .map(|lemma| QueryLemmaDto {
                            lemma: LemmaDto::from(&lemma.lemma),
                            readings: lemma
                                .readings
                                .iter()
                                .map(|reading| ReadingDto {
                                    form: &reading.form,
                                    morphology: reading.morphology,
                                    context_selected: reading.context_selected,
                                })
                                .collect(),
                        })
                        .collect(),
                })
                .collect(),
        }
    }
}

/// Hits with UTF-16 offsets. Hits come grouped by entry, so each entry's
/// text is looked up and scanned once.
fn hits_dto<'a>(index: &'a WordIndex, hits: &'a [Hit]) -> Vec<HitDto<'a>> {
    let mut offsets: Option<(&str, Utf16Offsets)> = None;
    hits.iter()
        .map(|hit| {
            if offsets.as_ref().is_none_or(|(id, _)| *id != &*hit.entry_id) {
                let text = index.entry_text(&hit.entry_id).unwrap_or_default();
                offsets = Some((&hit.entry_id, Utf16Offsets::new(text)));
            }
            let (_, converter) = offsets.as_mut().expect("set above");
            let (start, end) = converter.range(&hit.range);
            HitDto {
                entry_id: &hit.entry_id,
                start,
                end,
            }
        })
        .collect()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ReviewReadingDto {
    spelling: Option<String>,
    lemma: LemmaDto,
    morphology: Option<&'static str>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ReviewWordDto {
    text: String,
    start: usize,
    end: usize,
    readings: Vec<ReviewReadingDto>,
    display_order: Vec<usize>,
    context_reading: Option<usize>,
    preselected: Option<usize>,
    lemma_ambiguous: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ReviewDto {
    entry_id: String,
    original: String,
    words: Vec<ReviewWordDto>,
    direct: Option<AppliedReviewDto>,
}

impl From<ReviewModel> for ReviewDto {
    fn from(review: ReviewModel) -> Self {
        let mut offsets = Utf16Offsets::new(&review.original);
        let words = review
            .words
            .into_iter()
            .map(|word| {
                let (start, end) = offsets.range(&word.range);
                ReviewWordDto {
                    text: word.text,
                    start,
                    end,
                    readings: word
                        .readings
                        .into_iter()
                        .map(|reading| ReviewReadingDto {
                            spelling: reading.spelling,
                            lemma: LemmaDto::from(&reading.lemma),
                            morphology: reading.morphology,
                        })
                        .collect(),
                    display_order: word.display_order,
                    context_reading: word.context_reading,
                    preselected: word.preselected,
                    lemma_ambiguous: word.lemma_ambiguous,
                }
            })
            .collect();
        Self {
            entry_id: review.entry_id.to_string(),
            words,
            direct: review.direct.map(AppliedReviewDto::from),
            original: review.original,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AppliedReviewDto {
    text: String,
    lemmas: Vec<LemmaChoice>,
    changed: bool,
}

impl From<AppliedReview> for AppliedReviewDto {
    fn from(applied: AppliedReview) -> Self {
        Self {
            text: applied.text,
            lemmas: applied.lemmas,
            changed: applied.changed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    fn session() -> Session {
        let mut session = Session::new();
        let project = json!({
            "l1Name": "English",
            "lessons": [
                {"id": "a", "entries": [
                    {"id": "e1", "participant": "teacher", "language": "l1", "text": "🙂 The girl: puella"},
                    {"id": "e2", "participant": "student", "language": "latin", "text": "Puellā puellam videt.",
                     "lemmas": [], "audio": null},
                ]},
                {"id": "b", "entries": [
                    {"id": "e3", "participant": "teacher", "language": "latin", "text": "Mālum puellae dat"},
                ]},
            ],
        });
        session.set_project(&project.to_string()).unwrap();
        session
    }

    fn search(session: &Session, query: &str, options: Value) -> Value {
        serde_json::from_str(&session.search(query, &options.to_string()).unwrap()).unwrap()
    }

    #[test]
    fn hits_and_counts_use_utf16_offsets() {
        let session = session();
        let result = search(&session, "puella", json!({"exactText": true}));
        assert_eq!(result["mode"], "text");
        assert_eq!(result["lemma"], Value::Null);
        // "🙂" is two UTF-16 code units and four bytes; "ā" one and two.
        assert_eq!(
            result["hits"],
            json!([
                {"entryId": "e1", "start": 13, "end": 19},
                {"entryId": "e2", "start": 0, "end": 6},
                {"entryId": "e2", "start": 7, "end": 13},
                {"entryId": "e3", "start": 6, "end": 12},
            ])
        );
        assert_eq!(
            result["perLesson"],
            json!([
                {"lessonId": "a", "teacher": 1, "student": 2},
                {"lessonId": "b", "teacher": 1, "student": 0},
            ])
        );
        assert_eq!(result["lessonsWithHits"], 2);
        assert_eq!("🙂 The girl: puella".encode_utf16().count(), 19);
    }

    #[test]
    fn lemma_search_reports_its_lemma_and_analysis() {
        let session = session();
        let result = search(&session, "puellae", json!({}));
        assert_eq!(result["mode"], "lemma");
        assert_eq!(result["lemma"]["key"], "puella");
        assert_eq!(result["lemma"]["headword"], "puella");
        assert!(result["lemma"]["translation"].is_string());
        assert_eq!(result["hits"].as_array().unwrap().len(), 3);
        let word = &result["analysis"][0];
        assert_eq!(word["text"], "puellae");
        assert_eq!(word["ruledOut"], 0);
        let lemma = &word["lemmas"][0];
        assert_eq!(lemma["key"], "puella");
        assert!(lemma["readings"][0]["form"].is_string());
        assert!(lemma["readings"][0]["contextSelected"].is_boolean());

        // An explicit lemma is searched even with exact text on.
        let result = search(
            &session,
            "puella",
            json!({"exactText": true, "lemma": {"key": "puella", "extension": false}}),
        );
        assert_eq!(result["mode"], "lemma");
        assert!(
            session
                .search("x", &json!({"lemma": {"key": "no such lemma"}}).to_string())
                .is_err()
        );
        assert!(session.search("x", "{").is_err());
    }

    #[test]
    fn first_uses_and_new_lemmas() {
        let session = session();
        let first: Value = serde_json::from_str(&session.first_use().unwrap()).unwrap();
        assert!(first.get("e1").is_none(), "L1 entries have no lemmas");
        assert!(first["e2"].as_array().unwrap().contains(&json!("puella")));
        let new: Value = serde_json::from_str(&session.new_in_lesson("b").unwrap()).unwrap();
        let keys: Vec<_> = new
            .as_array()
            .unwrap()
            .iter()
            .map(|lemma| &lemma["key"])
            .collect();
        assert!(!keys.contains(&&json!("puella")));
        assert!(keys.contains(&&json!("do")));
        assert_eq!(session.new_in_lesson("missing").unwrap(), "[]");
    }

    #[test]
    fn spaced_repetition_uses_durations_and_gaps() {
        let mut session = Session::new();
        let filler = |id: &str, duration: Value| json!({"id": id, "participant": "student", "language": "l1", "text": "x", "duration": duration});
        let project = json!({"l1Name": "English", "lessons": [{"id": "a", "entries": [
            {"id": "e1", "participant": "teacher", "language": "latin", "text": "rosa", "duration": 20.0},
            filler("e2", json!(null)),
            filler("e3", json!(null)),
        ]}]});
        session.set_project(&project.to_string()).unwrap();
        // Gaps default to 0.1 s same, 0.5 s different: e2 starts at 20.5 s,
        // e3 at 31.0 s; nothing starts at or after 60 s, the lesson ends at
        // 41 s, so no flag.
        let flags: Value = serde_json::from_str(&session.spaced_repetition().unwrap()).unwrap();
        assert_eq!(flags, json!([]));
        session
            .set_gaps(
                &json!({"sameParticipantMs": 100, "differentParticipantMs": 20000}).to_string(),
            )
            .unwrap();
        // e2 starts at 40 s, e3 at 50.1 s, the lesson ends at 60.1 s: a lesson flag.
        let flags: Value = serde_json::from_str(&session.spaced_repetition().unwrap()).unwrap();
        assert_eq!(flags[0]["kind"], json!({"type": "lesson", "lessonId": "a"}));
        assert_eq!(flags[0]["lemma"]["headword"], "rosa");
        assert_eq!(flags[0]["lastHeardEntryId"], "e1");
        assert_eq!(flags[0]["elapsedSeconds"], 60.1);
        session
            .set_entry(&json!({"id": "e3", "participant": "student", "language": "l1", "text": "x", "duration": 30.0}).to_string())
            .unwrap();
        let flags: Value = serde_json::from_str(&session.spaced_repetition().unwrap()).unwrap();
        assert_eq!(flags[0]["kind"], json!({"type": "lesson", "lessonId": "a"}));
        session
            .set_entry(&json!({"id": "e2", "participant": "student", "language": "l1", "text": "x", "duration": 20.0}).to_string())
            .unwrap();
        let flags: Value = serde_json::from_str(&session.spaced_repetition().unwrap()).unwrap();
        assert_eq!(
            flags[0]["kind"],
            json!({"type": "inLesson", "lessonId": "a", "entryId": "e3"})
        );
        assert!(session.set_gaps("{").is_err());
    }

    #[test]
    fn structure_and_entry_updates() {
        let mut session = session();
        session
            .set_structure(
                &json!([{"id": "b", "entryIds": ["e3", "e2"]}, {"id": "a", "entryIds": ["e1"]}])
                    .to_string(),
            )
            .unwrap();
        let first: Value = serde_json::from_str(&session.first_use().unwrap()).unwrap();
        assert!(first["e3"].as_array().unwrap().contains(&json!("puella")));
        session
            .set_entry(&json!({"id": "e3", "participant": "student", "language": "latin", "text": "rosa",
                "lemmas": [{"token": "rosa", "occurrence": 0, "lemmaKey": "rosa", "extension": false}]}).to_string())
            .unwrap();
        let result = search(&session, "rosa", json!({}));
        assert_eq!(
            result["perLesson"][0],
            json!({"lessonId": "b", "teacher": 0, "student": 1})
        );
        session.remove_entry("e3");
        assert!(
            search(&session, "rosa", json!({}))["hits"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert!(session.set_entry(r#"{"id": "e9"}"#).is_err());
    }

    #[test]
    fn review_round_trip() {
        let mut session = session();
        assert_eq!(
            session.review("e1").unwrap(),
            "null",
            "L1 entries aren't reviewed"
        );
        let review: Value = serde_json::from_str(&session.review("e3").unwrap()).unwrap();
        assert_eq!(review["entryId"], "e3");
        assert_eq!(review["original"], "Mālum puellae dat");
        assert_eq!(review["direct"], Value::Null, "mālum has several lemmas");
        let words = review["words"].as_array().unwrap();
        assert_eq!(words.len(), 3);
        assert_eq!(
            (&words[1]["start"], &words[1]["end"]),
            (&json!(6), &json!(13))
        );
        assert!(words[0]["lemmaAmbiguous"].as_bool().unwrap());
        let reading = &words[0]["readings"][0];
        assert!(reading["lemma"]["headword"].is_string());
        assert!(reading.get("spelling").is_some() && reading.get("morphology").is_some());
        let choices: Vec<Value> = words
            .iter()
            .map(|word| word["preselected"].clone())
            .collect();
        let applied: Value = serde_json::from_str(
            &session
                .apply_review(
                    "e3",
                    "Mālum puellae dat",
                    &Value::Array(choices.clone()).to_string(),
                )
                .unwrap(),
        )
        .unwrap();
        assert!(applied["text"].is_string());
        assert!(applied["lemmas"][0]["lemmaKey"].is_string());
        assert!(applied["changed"].as_bool().unwrap());
        let stale = session
            .apply_review("e3", "other", &Value::Array(choices).to_string())
            .unwrap();
        assert_eq!(stale, "null");

        session
            .set_entry(&json!({"id": "e2", "participant": "student", "language": "latin", "text": "puellas"}).to_string())
            .unwrap();
        let review: Value = serde_json::from_str(&session.review("e2").unwrap()).unwrap();
        assert_eq!(
            review["direct"],
            json!({"text": "puellās", "lemmas": [], "changed": true})
        );
    }

    #[test]
    fn utf16_offsets_count_code_units_between_positions() {
        let text = "a𝔸ā b";
        let mut offsets = Utf16Offsets::new(text);
        assert_eq!(offsets.at(0), 0);
        assert_eq!(offsets.at(1), 1);
        assert_eq!(offsets.at(5), 3);
        assert_eq!(offsets.at(text.len()), 6);
        // Going back restarts the count.
        assert_eq!(offsets.at(5), 3);
        assert_eq!(Session::new().macronize("Puellas"), "Puellās");
    }
}
