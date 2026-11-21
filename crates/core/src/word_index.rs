//! Compact project-wide word and lemma indexes backed by cached collo
//! analyses, keyed by entry ID.
//!
//! The index keeps each entry's content and its tokens, and the script
//! structure (lessons in order, each with its entry IDs in order) separately,
//! so the frequent structural edits (moves, inserts, deletes) only renumber
//! entries and recompute first uses, without touching text, and a content
//! edit retokenizes just that entry. Analyses are cached per distinct
//! spelling and sentence position for the life of the index.
use std::collections::{BTreeSet, HashMap};
use std::ops::Range;
use std::sync::Arc;

use serde::Deserialize;

#[path = "spaced.rs"]
pub mod spaced;

use crate::language::translation_language;
use crate::lemma_choice::lemma_choice_key;
use crate::{AudioGaps, Entry, Language, LemmaChoice, Participant, Project, macronize};

const NO_ID: u32 = u32::MAX;
const NO_POSTING: u64 = u64::MAX;
/// The rank of an entry that is not in the script structure.
const DETACHED: u32 = u32::MAX;

/// What the index needs of an entry.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryContent {
    pub id: String,
    pub participant: Participant,
    pub language: Language,
    pub text: String,
    #[serde(default)]
    pub lemmas: Vec<LemmaChoice>,
    /// The cropped clip's length in seconds; `None` without a recording.
    /// Only spaced repetition flags use it.
    #[serde(default)]
    pub duration: Option<f64>,
}

impl From<&Entry> for EntryContent {
    fn from(entry: &Entry) -> Self {
        Self {
            id: entry.id.clone(),
            participant: entry.participant,
            language: entry.language,
            text: entry.text.clone(),
            lemmas: entry.lemmas.clone(),
            duration: entry.audio.as_ref().map(|audio| audio.duration(entry.trim)),
        }
    }
}

/// A lesson with the content of its entries, in script order.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LessonContent {
    pub id: String,
    pub entries: Vec<EntryContent>,
}

/// What the index needs of a project.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectContent {
    pub l1_name: String,
    pub lessons: Vec<LessonContent>,
}

impl From<&Project> for ProjectContent {
    fn from(project: &Project) -> Self {
        Self {
            l1_name: project.l1_name.clone(),
            lessons: project
                .lessons
                .iter()
                .map(|lesson| LessonContent {
                    id: lesson.id.clone(),
                    entries: lesson.entries.iter().map(EntryContent::from).collect(),
                })
                .collect(),
        }
    }
}

/// A lesson and its entry IDs in script order.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LessonOutline {
    pub id: String,
    pub entry_ids: Vec<String>,
}

/// A span of an entry's text: a search match or a word.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hit {
    pub entry_id: Arc<str>,
    /// Byte range in the entry text.
    pub range: Range<usize>,
}

/// A lemma as the interface shows it.
#[derive(Clone, Debug)]
pub struct LemmaInfo {
    pub lemma: &'static collo::Lemma,
    /// The headword as a reader expects it (see [`macronize::reader_form`]).
    pub headword: String,
    /// The meaning in the project's L1, if collo has it in that language.
    pub translation: Option<&'static str>,
}

impl LemmaInfo {
    pub fn new(lemma: &'static collo::Lemma, translation_language: &str) -> Self {
        Self {
            lemma,
            headword: macronize::reader_form(lemma.headword),
            translation: lemma.translation(translation_language),
        }
    }
}

/// A [`Hit`] by slot, before IDs are looked up.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SlotHit {
    pub slot: u32,
    pub range: Range<usize>,
}

#[derive(Clone, Copy, Debug)]
struct IndexedToken {
    range_start: u32,
    range_end: u32,
    surface: u32,
    surface_position: u32,
    analysis: u32,
    selected_lemma: u32,
    /// `selected_lemma` was chosen by the user, so it is the only reading.
    chosen: bool,
}

struct SurfaceRecord {
    /// Keep the common single-occurrence case inline; repeated spellings use
    /// the overflow vector. Packed entry/token coordinates are unordered.
    first_occurrence: u64,
    other_occurrences: Vec<u64>,
    /// Candidate lemmas seen for this exact spelling. Per-occurrence analyses
    /// are checked when returning lemma hits, so historical candidates are safe.
    lemmas: Option<Box<[u32]>>,
}

impl Default for SurfaceRecord {
    fn default() -> Self {
        Self {
            first_occurrence: NO_POSTING,
            other_occurrences: Vec::new(),
            lemmas: None,
        }
    }
}

impl SurfaceRecord {
    fn postings(&self) -> impl Iterator<Item = u64> + '_ {
        std::iter::once(self.first_occurrence)
            .chain(self.other_occurrences.iter().copied())
            .filter(|&packed| packed != NO_POSTING)
    }
}

/// One entry's content and what is derived from it. Slots of removed entries
/// are reused.
struct EntrySlot {
    id: Arc<str>,
    participant: Participant,
    language: Language,
    text: String,
    lemmas: Vec<LemmaChoice>,
    duration: Option<f64>,
    tokens: Vec<IndexedToken>,
    /// The distinct `selected_lemma`s of the tokens, in text order.
    selected_lemmas: Box<[u32]>,
    /// The lemmas first used in the project in this entry, in text order.
    first_uses: Vec<u32>,
    /// Position in script order (ranks increase but may have gaps), or
    /// `DETACHED` while the entry is not in the structure.
    rank: u32,
    /// Index into `WordIndex::lessons`, meaningful while placed.
    lesson: u32,
}

impl EntrySlot {
    fn new(id: Arc<str>) -> Self {
        Self {
            id,
            participant: Participant::Teacher,
            language: Language::L1,
            text: String::new(),
            lemmas: Vec::new(),
            duration: None,
            tokens: Vec::new(),
            selected_lemmas: Box::default(),
            first_uses: Vec::new(),
            rank: DETACHED,
            lesson: 0,
        }
    }

    fn placed(&self) -> bool {
        self.rank != DETACHED
    }
}

struct LessonRecord {
    id: Arc<str>,
    entries: Vec<u32>,
    /// The entry IDs as last given, end to end, so an unchanged lesson is
    /// recognised without looking each ID up. Empty with `reusable` false
    /// when some were left out as listed twice.
    entry_ids: String,
    entry_id_ends: Vec<u32>,
    reusable: bool,
}

impl LessonRecord {
    /// Whether the lesson was last given exactly these entry IDs.
    fn lists(&self, entry_ids: &[String]) -> bool {
        if !self.reusable || self.entry_id_ends.len() != entry_ids.len() {
            return false;
        }
        let mut start = 0;
        entry_ids.iter().zip(&self.entry_id_ends).all(|(id, &end)| {
            let end = end as usize;
            let same = self.entry_ids.get(start..end) == Some(id.as_str());
            start = end;
            same
        })
    }
}

/// Project-wide occurrence indexes and unique-token analyses. Per-token data
/// and postings use integer IDs so large scripts do not allocate tree nodes or
/// duplicate token strings for every occurrence.
pub struct WordIndex {
    analyzer: collo::Lemmatizer,
    /// The collo language for translations, from the project's L1 name.
    translation: &'static str,

    /// By surface and sentence position.
    analysis_ids: HashMap<(u32, bool), u32>,
    analyses: Vec<Box<[collo::Analysis]>>,
    /// Lemmas of the analyses not excluded by a written macron or breve.
    analysis_lemmas: Vec<Box<[u32]>>,
    /// The subset that remains when unmarked vowels are read as short.
    analysis_short_lemmas: Vec<Box<[u32]>>,
    /// The most frequent of those lemmas, used for first-use labels.
    analysis_selected_lemma: Vec<u32>,
    /// By the address of collo's static lemma.
    lemma_ids: HashMap<usize, u32>,
    lemmas: Vec<&'static collo::Lemma>,

    surfaces: Vec<SurfaceRecord>,
    by_surface: HashMap<Box<str>, u32>,
    /// Each spelling's [`lemma_choice_key`] as an ID, computed when first
    /// needed, so numbering words for lemma choices doesn't allocate.
    surface_choice_keys: Vec<u32>,
    choice_key_ids: HashMap<String, u32>,
    choice_keys: Vec<String>,
    by_lemma: HashMap<&'static str, Vec<u32>>,
    active_surfaces: usize,

    slots: Vec<EntrySlot>,
    free_slots: Vec<u32>,
    slot_ids: HashMap<Arc<str>, u32>,
    lessons: Vec<LessonRecord>,
    lesson_ids: HashMap<Arc<str>, u32>,
    /// Placed slots in script order.
    script: Vec<u32>,
    /// Per slot, the last `set_structure` call that placed it, to find
    /// entries listed twice without visiting every slot.
    placed_in: Vec<u32>,
    structure_generation: u32,
    /// Per lemma, the slot of its first use in the script, or `NO_ID`.
    first_use: Vec<u32>,
    /// The project's participant gaps, for spaced repetition timing.
    gaps: AudioGaps,
}

impl Default for WordIndex {
    fn default() -> Self {
        Self {
            analyzer: collo::Lemmatizer::new(),
            translation: "en",
            analysis_ids: HashMap::new(),
            analyses: Vec::new(),
            analysis_lemmas: Vec::new(),
            analysis_short_lemmas: Vec::new(),
            analysis_selected_lemma: Vec::new(),
            lemma_ids: HashMap::new(),
            lemmas: Vec::new(),
            surfaces: Vec::new(),
            by_surface: HashMap::new(),
            surface_choice_keys: Vec::new(),
            choice_key_ids: HashMap::new(),
            choice_keys: Vec::new(),
            by_lemma: HashMap::new(),
            active_surfaces: 0,
            slots: Vec::new(),
            free_slots: Vec::new(),
            slot_ids: HashMap::new(),
            lessons: Vec::new(),
            lesson_ids: HashMap::new(),
            script: Vec::new(),
            placed_in: Vec::new(),
            structure_generation: 0,
            first_use: Vec::new(),
            gaps: AudioGaps::default(),
        }
    }
}

impl WordIndex {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_project(project: &Project) -> Self {
        let mut index = Self::default();
        index.set_project(ProjectContent::from(project));
        index
    }

    /// The collo language translations are shown in.
    pub fn translation_language(&self) -> &'static str {
        self.translation
    }

    /// The gaps between entries, as in lesson playback.
    pub fn set_gaps(&mut self, gaps: AudioGaps) {
        self.gaps = gaps;
    }

    pub fn set_l1_name(&mut self, l1_name: &str) {
        self.translation = translation_language(l1_name);
    }

    pub fn lemma_info(&self, lemma: &'static collo::Lemma) -> LemmaInfo {
        LemmaInfo::new(lemma, self.translation)
    }

    /// Replace the whole project. Unique-token analyses are retained.
    pub fn set_project(&mut self, project: ProjectContent) {
        self.translation = translation_language(&project.l1_name);
        self.slots.clear();
        self.free_slots.clear();
        self.slot_ids.clear();
        for surface in &mut self.surfaces {
            surface.first_occurrence = NO_POSTING;
            surface.other_occurrences.clear();
            surface.lemmas = None;
        }
        self.by_lemma.clear();
        self.active_surfaces = 0;
        self.lessons.clear();
        self.lesson_ids.clear();
        self.script.clear();
        self.placed_in.clear();
        self.first_use.fill(NO_ID);

        let mut outline = Vec::with_capacity(project.lessons.len());
        for lesson in project.lessons {
            let mut entry_ids = Vec::with_capacity(lesson.entries.len());
            for entry in lesson.entries {
                entry_ids.push(entry.id.clone());
                self.store_entry(entry);
            }
            outline.push(LessonOutline {
                id: lesson.id,
                entry_ids,
            });
        }
        self.set_structure(&outline);
    }

    /// Place entries after lessons or entries were moved, inserted or
    /// deleted. Text isn't reanalyzed. An entry ID the index doesn't know yet
    /// is placed empty until [`set_entry`](Self::set_entry) gives its
    /// content; known entries left out are kept, unplaced, until removed.
    pub fn set_structure(&mut self, lessons: &[LessonOutline]) {
        self.structure_generation = match self.structure_generation.checked_add(1) {
            Some(generation) => generation,
            None => {
                self.placed_in.fill(0);
                1
            }
        };
        let generation = self.structure_generation;
        let old_ids = std::mem::take(&mut self.lesson_ids);
        let mut old_lessons: Vec<_> = std::mem::take(&mut self.lessons)
            .into_iter()
            .map(Some)
            .collect();
        let old_script = std::mem::take(&mut self.script);
        for outline in lessons {
            if self.lesson_ids.contains_key(outline.id.as_str()) {
                continue;
            }
            let index = self.lessons.len() as u32;
            let old_index = old_ids.get(outline.id.as_str()).copied();
            // A lesson listing the same entries as before keeps its slots.
            let unchanged = old_index
                .and_then(|old| old_lessons[old as usize].take())
                .filter(|old| {
                    old.lists(&outline.entry_ids)
                        && old
                            .entries
                            .iter()
                            .all(|&slot| self.placed_in[slot as usize] != generation)
                });
            let record = match unchanged {
                Some(record) => {
                    for &slot in &record.entries {
                        self.placed_in[slot as usize] = generation;
                    }
                    if old_index != Some(index) {
                        for &slot in &record.entries {
                            self.slots[slot as usize].lesson = index;
                        }
                    }
                    record
                }
                None => self.lesson_record(outline, index, generation),
            };
            self.script.extend_from_slice(&record.entries);
            self.lesson_ids.insert(record.id.clone(), index);
            self.lessons.push(record);
        }

        // Before the first change, ranks and first uses stand. From there,
        // take the first uses back and assign them again in script order.
        // Removed entries leave gaps in the ranks, so continue from the last
        // rank kept, or start over in the unlikely case that would overflow.
        let mut start = old_script
            .iter()
            .zip(&self.script)
            .take_while(|(old, new)| old == new)
            .count();
        let mut next_rank = match start {
            0 => 0,
            _ => self.slots[self.script[start - 1] as usize].rank + 1,
        };
        if u32::try_from(self.script.len() - start)
            .ok()
            .and_then(|count| next_rank.checked_add(count))
            .is_none_or(|end| end == DETACHED)
        {
            start = 0;
            next_rank = 0;
        }
        for &slot in &old_script[start..] {
            let record = &mut self.slots[slot as usize];
            record.rank = DETACHED;
            for &lemma in &record.first_uses {
                self.first_use[lemma as usize] = NO_ID;
            }
            record.first_uses.clear();
        }
        for &slot in &self.script[start..] {
            let record = &mut self.slots[slot as usize];
            record.rank = next_rank;
            next_rank += 1;
            for &lemma in &record.selected_lemmas {
                let first = &mut self.first_use[lemma as usize];
                if *first == NO_ID {
                    *first = slot;
                    record.first_uses.push(lemma);
                }
            }
        }
    }

    /// A lesson's record with each entry ID looked up. An entry listed twice
    /// is placed once, where first listed.
    fn lesson_record(
        &mut self,
        outline: &LessonOutline,
        index: u32,
        generation: u32,
    ) -> LessonRecord {
        let mut entries = Vec::with_capacity(outline.entry_ids.len());
        let mut entry_ids = String::new();
        let mut entry_id_ends = Vec::with_capacity(outline.entry_ids.len());
        let mut reusable = true;
        for entry_id in &outline.entry_ids {
            let slot = self.slot_for(entry_id);
            if self.placed_in[slot as usize] == generation {
                reusable = false;
                continue;
            }
            self.placed_in[slot as usize] = generation;
            self.slots[slot as usize].lesson = index;
            entries.push(slot);
            entry_ids.push_str(entry_id);
            entry_id_ends.push(u32::try_from(entry_ids.len()).expect("lesson IDs exceed 4 GiB"));
        }
        if !reusable {
            entry_ids.clear();
            entry_id_ends.clear();
        }
        LessonRecord {
            id: outline.id.as_str().into(),
            entries,
            entry_ids,
            entry_id_ends,
            reusable,
        }
    }

    /// Update one entry's content. Only that entry is retokenized; a change
    /// of participant alone touches nothing else.
    pub fn set_entry(&mut self, entry: EntryContent) {
        let slot = self.slot_for(&entry.id) as usize;
        let record = &mut self.slots[slot];
        record.participant = entry.participant;
        record.duration = entry.duration;
        if record.text == entry.text
            && record.language == entry.language
            && record.lemmas == entry.lemmas
        {
            return;
        }
        let old_lemmas = std::mem::take(&mut record.selected_lemmas);
        self.unindex_tokens(slot as u32);
        let record = &mut self.slots[slot];
        record.text = entry.text;
        record.language = entry.language;
        record.lemmas = entry.lemmas;
        self.index_tokens(slot as u32);
        if self.slots[slot].placed() {
            let affected: BTreeSet<u32> = old_lemmas
                .iter()
                .chain(self.slots[slot].selected_lemmas.iter())
                .copied()
                .collect();
            for lemma in affected {
                self.refresh_first_use(lemma, slot as u32);
            }
            // Labels kept through the edit may now be in another text order.
            let record = &mut self.slots[slot];
            let selected = &record.selected_lemmas;
            record
                .first_uses
                .sort_by_key(|lemma| selected.iter().position(|item| item == lemma));
        }
    }

    /// Forget an entry and take it out of the structure.
    pub fn remove_entry(&mut self, id: &str) {
        let Some(slot) = self.slot_ids.remove(id) else {
            return;
        };
        self.unindex_tokens(slot);
        let record = std::mem::replace(&mut self.slots[slot as usize], EntrySlot::new("".into()));
        self.free_slots.push(slot);
        if !record.placed() {
            return;
        }
        // Ranks of the remaining entries keep their order, so only the first
        // uses that were in this entry move.
        let lesson = &mut self.lessons[record.lesson as usize];
        lesson.entries.retain(|&other| other != slot);
        // The lesson no longer lists what it was given.
        lesson.reusable = false;
        self.script.retain(|&other| other != slot);
        for &lemma in &record.first_uses {
            self.first_use[lemma as usize] = NO_ID;
            if let Some(first) = self.find_first_use(lemma) {
                self.first_use[lemma as usize] = first;
                self.add_first_use_label(first, lemma);
            }
        }
    }

    /// The slot holding this entry, creating an empty, unplaced one if needed.
    fn slot_for(&mut self, id: &str) -> u32 {
        if let Some(&slot) = self.slot_ids.get(id) {
            return slot;
        }
        let id: Arc<str> = id.into();
        let slot = match self.free_slots.pop() {
            Some(slot) => {
                self.slots[slot as usize] = EntrySlot::new(id.clone());
                slot
            }
            None => {
                let slot = u32::try_from(self.slots.len()).expect("too many entries");
                self.slots.push(EntrySlot::new(id.clone()));
                self.placed_in.push(0);
                slot
            }
        };
        self.slot_ids.insert(id, slot);
        slot
    }

    /// Store and tokenize an entry's content, without updating first uses.
    fn store_entry(&mut self, entry: EntryContent) {
        let slot = self.slot_for(&entry.id);
        self.unindex_tokens(slot);
        let record = &mut self.slots[slot as usize];
        record.participant = entry.participant;
        record.duration = entry.duration;
        record.language = entry.language;
        record.text = entry.text;
        record.lemmas = entry.lemmas;
        self.index_tokens(slot);
    }

    pub fn contains_entry(&self, id: &str) -> bool {
        self.slot_ids.contains_key(id)
    }

    /// The entry's text as last given to the index.
    pub fn entry_text(&self, id: &str) -> Option<&str> {
        let &slot = self.slot_ids.get(id)?;
        Some(&self.slots[slot as usize].text)
    }

    /// The index's shared copy of an entry ID.
    pub(crate) fn entry_id_arc(&self, id: &str) -> Option<Arc<str>> {
        let &slot = self.slot_ids.get(id)?;
        Some(self.slots[slot as usize].id.clone())
    }

    pub fn entry_language(&self, id: &str) -> Option<Language> {
        let &slot = self.slot_ids.get(id)?;
        Some(self.slots[slot as usize].language)
    }

    pub fn entry_lemmas(&self, id: &str) -> Option<&[LemmaChoice]> {
        let &slot = self.slot_ids.get(id)?;
        Some(&self.slots[slot as usize].lemmas)
    }

    /// Lesson IDs in script order.
    pub fn lesson_ids(&self) -> impl Iterator<Item = &Arc<str>> {
        self.lessons.iter().map(|lesson| &lesson.id)
    }

    /// The lesson's entry IDs in script order.
    pub fn lesson_entry_ids(&self, lesson_id: &str) -> Option<impl Iterator<Item = &Arc<str>>> {
        let &lesson = self.lesson_ids.get(lesson_id)?;
        Some(
            self.lessons[lesson as usize]
                .entries
                .iter()
                .map(|&slot| &self.slots[slot as usize].id),
        )
    }

    fn choice_key_id(&mut self, key: &str) -> u32 {
        if let Some(&id) = self.choice_key_ids.get(key) {
            return id;
        }
        let id = u32::try_from(self.choice_keys.len()).expect("too many unique spellings");
        self.choice_key_ids.insert(key.to_owned(), id);
        self.choice_keys.push(key.to_owned());
        id
    }

    fn surface_choice_key(&mut self, surface: u32, text: &str) -> u32 {
        let surface = surface as usize;
        if self.surface_choice_keys.len() <= surface {
            self.surface_choice_keys.resize(surface + 1, NO_ID);
        }
        if self.surface_choice_keys[surface] == NO_ID {
            self.surface_choice_keys[surface] = self.choice_key_id(&lemma_choice_key(text));
        }
        self.surface_choice_keys[surface]
    }

    /// The lemma chosen for an occurrence, if its written quantities allow it.
    fn chosen_lemma(&self, choice: &LemmaChoice, analysis: u32) -> Option<u32> {
        self.analysis_lemmas[analysis as usize]
            .iter()
            .copied()
            .find(|&id| {
                let lemma = self.lemmas[id as usize];
                lemma.key == choice.lemma_key && lemma.extension == choice.extension
            })
    }

    fn store_analysis(
        &mut self,
        lookup: (u32, bool),
        token: &str,
        analyses: Box<[collo::Analysis]>,
    ) -> u32 {
        // Every analysis is kept (macronizing uses them all); only readings
        // that fit the written quantities identify the word's lemma.
        let preferred = |unmarked_short| -> Vec<&collo::Analysis> {
            macronize::matching_analyses(token, &analyses, unmarked_short)
                .into_iter()
                .map(|index| &analyses[index])
                .collect()
        };
        let (preferred, preferred_short) = (preferred(false), preferred(true));
        let selected_lemma = preferred
            .iter()
            .max_by_key(|analysis| analysis.lemma.frequency)
            .map_or(NO_ID, |analysis| self.intern_lemma(analysis.lemma));
        let mut lemma_set = |analyses: &[&collo::Analysis]| -> Box<[u32]> {
            analyses
                .iter()
                .map(|analysis| self.intern_lemma(analysis.lemma))
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect()
        };
        let lemma_ids = lemma_set(&preferred);
        let short_lemma_ids = lemma_set(&preferred_short);
        let id = u32::try_from(self.analyses.len()).expect("too many unique project words");
        self.analysis_ids.insert(lookup, id);
        self.analyses.push(analyses);
        self.analysis_lemmas.push(lemma_ids);
        self.analysis_short_lemmas.push(short_lemma_ids);
        self.analysis_selected_lemma.push(selected_lemma);
        id
    }

    fn intern_lemma(&mut self, lemma: &'static collo::Lemma) -> u32 {
        // collo's lemmas are statics, so the address identifies one.
        let identity = std::ptr::from_ref(lemma) as usize;
        if let Some(&id) = self.lemma_ids.get(&identity) {
            return id;
        }
        let id = u32::try_from(self.lemmas.len()).expect("too many unique lemmas");
        self.lemma_ids.insert(identity, id);
        self.lemmas.push(lemma);
        self.first_use.push(NO_ID);
        id
    }

    fn analysis_id(&mut self, token: &str, sentence_start: bool, surface: u32) -> u32 {
        let lookup = (surface, sentence_start);
        if let Some(&id) = self.analysis_ids.get(&lookup) {
            return id;
        }
        let analyses = self.analyzer.analyze(token, sentence_start);
        self.store_analysis(lookup, token, analyses.into())
    }

    /// collo's analyses of a word, cached per spelling and sentence position.
    pub fn analyses(&mut self, token: &str, sentence_start: bool) -> &[collo::Analysis] {
        let surface = self.intern_surface(token);
        let id = self.analysis_id(token, sentence_start, surface) as usize;
        &self.analyses[id]
    }

    /// Tag the text in context as collo does, with cached analyses.
    pub fn tag_text(&mut self, text: &str) -> Vec<collo::tagging::TaggedWord> {
        let analyzer = self.analyzer;
        analyzer.tag_text_with(text, |token, sentence_start| {
            self.analyses(token, sentence_start).to_vec()
        })
    }

    /// [`macronize::macronize`] with cached analyses.
    pub fn macronize(&mut self, text: &str) -> String {
        let words = self.tag_text(text);
        macronize::macronize_tagged(text, &words)
    }

    fn intern_surface(&mut self, surface: &str) -> u32 {
        if let Some(&id) = self.by_surface.get(surface) {
            return id;
        }
        let id = u32::try_from(self.surfaces.len()).expect("too many unique spellings");
        self.surfaces.push(SurfaceRecord::default());
        self.by_surface.insert(surface.into(), id);
        id
    }

    /// Remove the slot's tokens from the surface postings.
    fn unindex_tokens(&mut self, slot: u32) {
        let mut old = std::mem::take(&mut self.slots[slot as usize].tokens);
        self.slots[slot as usize].selected_lemmas = Box::default();
        for token_index in 0..old.len() {
            let token = old[token_index];
            let posting = pack_occurrence(slot, token_index as u32);
            let surface = &mut self.surfaces[token.surface as usize];
            if surface.first_occurrence == posting {
                if let Some(moved) = surface.other_occurrences.pop() {
                    surface.first_occurrence = moved;
                    let (moved_slot, moved_index) = unpack_occurrence(moved);
                    if moved_slot == slot {
                        old[moved_index].surface_position = NO_ID;
                    } else {
                        self.slots[moved_slot as usize].tokens[moved_index].surface_position =
                            NO_ID;
                    }
                } else {
                    surface.first_occurrence = NO_POSTING;
                    self.active_surfaces = self.active_surfaces.saturating_sub(1);
                }
            } else {
                let position = token.surface_position as usize;
                let last = *surface
                    .other_occurrences
                    .last()
                    .expect("repeated surface posting exists");
                let removed = surface.other_occurrences.swap_remove(position);
                debug_assert_eq!(removed, posting);
                if position < surface.other_occurrences.len() {
                    let (moved_slot, moved_index) = unpack_occurrence(last);
                    if moved_slot == slot {
                        old[moved_index].surface_position = position as u32;
                    } else {
                        self.slots[moved_slot as usize].tokens[moved_index].surface_position =
                            position as u32;
                    }
                }
            }
        }
    }

    /// Tokenize the slot's text into postings. Lexical ambiguity is resolved
    /// cheaply using the highest-frequency candidate; full contextual tagging
    /// remains available through `tag_text` for explicit parsing review.
    fn index_tokens(&mut self, slot: u32) {
        let record = &mut self.slots[slot as usize];
        let text = std::mem::take(&mut record.text);
        let lemmas = std::mem::take(&mut record.lemmas);
        let language = record.language;
        let mut indexed: Vec<IndexedToken> = Vec::new();
        let mut selected_lemmas = Vec::new();
        for token in collo::tagging::tokenize(&text) {
            let token_text = &text[token.range.clone()];
            let surface_id = self.intern_surface(token_text);
            let analysis = if language == Language::Latin {
                self.analysis_id(token_text, token.sentence_start, surface_id)
            } else {
                NO_ID
            };
            // Most entries have no chosen lemmas, so don't number their words.
            let chosen = if lemmas.is_empty() || analysis == NO_ID {
                None
            } else {
                let key = self.surface_choice_key(surface_id, token_text);
                let key_text = &self.choice_keys[key as usize];
                if lemmas.iter().any(|choice| choice.token == *key_text) {
                    // Entries are short, so count earlier words of this spelling.
                    let occurrence = indexed
                        .iter()
                        .filter(|earlier| {
                            self.surface_choice_keys.get(earlier.surface as usize) == Some(&key)
                        })
                        .count() as u32;
                    lemmas
                        .iter()
                        .find(|choice| choice.occurrence == occurrence && choice.token == *key_text)
                        .and_then(|choice| self.chosen_lemma(choice, analysis))
                } else {
                    None
                }
            };
            let selected_lemma = if analysis != NO_ID {
                chosen.unwrap_or(self.analysis_selected_lemma[analysis as usize])
            } else {
                NO_ID
            };
            if selected_lemma != NO_ID && !selected_lemmas.contains(&selected_lemma) {
                selected_lemmas.push(selected_lemma);
            }

            // A chosen lemma is always among the analysis lemmas.
            let mut new_surface_lemmas = Vec::new();
            if analysis != NO_ID {
                let known = self.surfaces[surface_id as usize].lemmas.as_deref();
                for &lemma_id in &self.analysis_lemmas[analysis as usize] {
                    let already_present = known.is_some_and(|lemmas| lemmas.contains(&lemma_id));
                    if !already_present && !new_surface_lemmas.contains(&lemma_id) {
                        new_surface_lemmas.push(lemma_id);
                    }
                }
            }

            let token_index = indexed.len();
            let surface = &mut self.surfaces[surface_id as usize];
            let posting = pack_occurrence(slot, token_index as u32);
            let (surface_position, became_active) = if surface.first_occurrence == NO_POSTING {
                surface.first_occurrence = posting;
                (NO_ID, true)
            } else {
                let position = u32::try_from(surface.other_occurrences.len())
                    .expect("too many repeated words");
                surface.other_occurrences.push(posting);
                (position, false)
            };
            if !new_surface_lemmas.is_empty() {
                let mut combined = surface
                    .lemmas
                    .take()
                    .map_or_else(Vec::new, |old| old.into_vec());
                combined.extend_from_slice(&new_surface_lemmas);
                surface.lemmas = Some(combined.into_boxed_slice());
            }
            if became_active {
                self.active_surfaces += 1;
            }
            for lemma_id in new_surface_lemmas {
                let key = self.lemmas[lemma_id as usize].key;
                self.by_lemma.entry(key).or_default().push(surface_id);
            }
            indexed.push(IndexedToken {
                range_start: u32::try_from(token.range.start).expect("entry text exceeds 4 GiB"),
                range_end: u32::try_from(token.range.end).expect("entry text exceeds 4 GiB"),
                surface: surface_id,
                surface_position,
                analysis,
                selected_lemma,
                chosen: chosen.is_some(),
            });
        }
        let record = &mut self.slots[slot as usize];
        record.text = text;
        record.lemmas = lemmas;
        record.tokens = indexed;
        record.selected_lemmas = selected_lemmas.into_boxed_slice();
    }

    /// Update the lemma's first use after the placed `slot` was retokenized.
    fn refresh_first_use(&mut self, lemma: u32, slot: u32) {
        let record = &self.slots[slot as usize];
        let current_has_lemma = record.selected_lemmas.contains(&lemma);
        let rank = record.rank;
        let previous = Some(self.first_use[lemma as usize]).filter(|&first| first != NO_ID);

        if current_has_lemma {
            if previous.is_none_or(|first| rank < self.slots[first as usize].rank) {
                if let Some(first) = previous {
                    self.slots[first as usize]
                        .first_uses
                        .retain(|&item| item != lemma);
                }
                self.first_use[lemma as usize] = slot;
                self.add_first_use_label(slot, lemma);
            }
        } else if previous == Some(slot) {
            self.slots[slot as usize]
                .first_uses
                .retain(|&item| item != lemma);
            self.first_use[lemma as usize] = NO_ID;
            if let Some(first) = self.find_first_use(lemma) {
                self.first_use[lemma as usize] = first;
                self.add_first_use_label(first, lemma);
            }
        }
    }

    /// Insert a first-use label keeping the entry's labels in text order.
    fn add_first_use_label(&mut self, slot: u32, lemma: u32) {
        let record = &mut self.slots[slot as usize];
        let position_of = |lemma: u32| {
            record
                .selected_lemmas
                .iter()
                .position(|&item| item == lemma)
        };
        let position = position_of(lemma);
        let at = record
            .first_uses
            .iter()
            .position(|&other| position_of(other) > position)
            .unwrap_or(record.first_uses.len());
        record.first_uses.insert(at, lemma);
    }

    fn find_first_use(&self, lemma: u32) -> Option<u32> {
        self.script
            .iter()
            .copied()
            .find(|&slot| self.slots[slot as usize].selected_lemmas.contains(&lemma))
    }

    /// The lemmas first used in the project in this entry, in text order.
    pub fn first_uses(&self, entry_id: &str) -> Vec<&'static collo::Lemma> {
        self.slot_ids.get(entry_id).map_or_else(Vec::new, |&slot| {
            self.slots[slot as usize]
                .first_uses
                .iter()
                .map(|&lemma| self.lemmas[lemma as usize])
                .collect()
        })
    }

    /// Every placed entry with first-use lemmas, in script order.
    pub fn all_first_uses(
        &self,
    ) -> impl Iterator<Item = (&Arc<str>, Vec<&'static collo::Lemma>)> + '_ {
        self.script.iter().filter_map(|&slot| {
            let record = &self.slots[slot as usize];
            (!record.first_uses.is_empty()).then(|| {
                let lemmas = record
                    .first_uses
                    .iter()
                    .map(|&lemma| self.lemmas[lemma as usize])
                    .collect();
                (&record.id, lemmas)
            })
        })
    }

    /// Lemmas whose first use in the project is in this lesson, in script order.
    pub fn new_lemmas_in_lesson(&self, lesson_id: &str) -> Vec<&'static collo::Lemma> {
        let Some(&lesson) = self.lesson_ids.get(lesson_id) else {
            return Vec::new();
        };
        self.lessons[lesson as usize]
            .entries
            .iter()
            .flat_map(|&slot| &self.slots[slot as usize].first_uses)
            .map(|&lemma| self.lemmas[lemma as usize])
            .collect()
    }

    /// Latin words with a lemma of this key among their readings, in script
    /// order; a word whose lemma was chosen matches only that lemma. With
    /// `unmarked_short`, unmarked vowels are read as short when a reading
    /// with them short exists.
    pub fn lemma_occurrences(&self, lemma_key: &str, unmarked_short: bool) -> Vec<Hit> {
        self.hits(&self.lemma_slot_hits(lemma_key, unmarked_short))
    }

    /// [`lemma_occurrences`](Self::lemma_occurrences) by slot.
    pub(crate) fn lemma_slot_hits(&self, lemma_key: &str, unmarked_short: bool) -> Vec<SlotHit> {
        let mut found: Vec<_> = self
            .lemma_postings(lemma_key, unmarked_short)
            .into_iter()
            .map(|(slot, start, end)| (self.slots[slot as usize].rank, start, end, slot))
            .collect();
        found.sort_unstable();
        // A surface is listed once per lemma ID; the extension lexicon could
        // list one twice under a key.
        found.dedup();
        found
            .into_iter()
            .map(|(_, start, end, slot)| SlotHit {
                slot,
                range: start as usize..end as usize,
            })
            .collect()
    }

    /// Placed occurrences of the lemma as `(slot, start, end)`, unordered.
    fn lemma_postings(&self, lemma_key: &str, unmarked_short: bool) -> Vec<(u32, u32, u32)> {
        let lemma_sets = if unmarked_short {
            &self.analysis_short_lemmas
        } else {
            &self.analysis_lemmas
        };
        let Some(surfaces) = self.by_lemma.get(lemma_key) else {
            return Vec::new();
        };
        surfaces
            .iter()
            .flat_map(|&surface| self.surfaces[surface as usize].postings())
            .filter_map(|packed| {
                let (slot, token_index) = unpack_occurrence(packed);
                let record = &self.slots[slot as usize];
                if !record.placed() {
                    return None;
                }
                let token = &record.tokens[token_index];
                let has_lemma = if token.chosen {
                    self.lemmas[token.selected_lemma as usize].key == lemma_key
                } else {
                    token.analysis != NO_ID
                        && lemma_sets[token.analysis as usize]
                            .iter()
                            .any(|&lemma| self.lemmas[lemma as usize].key == lemma_key)
                };
                has_lemma.then_some((slot, token.range_start, token.range_end))
            })
            .collect()
    }

    pub(crate) fn hits(&self, found: &[SlotHit]) -> Vec<Hit> {
        found
            .iter()
            .map(|hit| Hit {
                entry_id: self.slots[hit.slot as usize].id.clone(),
                range: hit.range.clone(),
            })
            .collect()
    }

    /// Placed entries in script order as (slot, text).
    pub(crate) fn script_texts(&self) -> impl Iterator<Item = (u32, &str)> + '_ {
        self.script
            .iter()
            .map(|&slot| (slot, self.slots[slot as usize].text.as_str()))
    }

    /// Per lesson in script order, its ID and the placed hits by teacher and
    /// by student.
    pub fn counts_per_lesson(&self, hits: &[Hit]) -> Vec<(Arc<str>, [usize; 2])> {
        self.slot_counts(
            hits.iter()
                .filter_map(|hit| self.slot_ids.get(hit.entry_id.as_ref()).copied()),
        )
    }

    pub(crate) fn slot_counts(
        &self,
        slots: impl IntoIterator<Item = u32>,
    ) -> Vec<(Arc<str>, [usize; 2])> {
        let mut counts = vec![[0; 2]; self.lessons.len()];
        for slot in slots {
            let record = &self.slots[slot as usize];
            if !record.placed() {
                continue;
            }
            let side = match record.participant {
                Participant::Teacher => 0,
                Participant::Student => 1,
            };
            counts[record.lesson as usize][side] += 1;
        }
        self.lessons
            .iter()
            .zip(counts)
            .map(|(lesson, counts)| (lesson.id.clone(), counts))
            .collect()
    }

    /// Distinct spellings with at least one occurrence.
    pub fn unique_surface_count(&self) -> usize {
        self.active_surfaces
    }

    /// Occurrences of this exact spelling, placed or not.
    pub fn surface_count(&self, surface: &str) -> usize {
        self.by_surface.get(surface).map_or(0, |&id| {
            let record = &self.surfaces[id as usize];
            usize::from(record.first_occurrence != NO_POSTING) + record.other_occurrences.len()
        })
    }

    /// Number of Latin occurrences with this lemma among their candidate analyses.
    pub fn possible_lemma_count(&self, lemma_key: &str) -> usize {
        self.lemma_postings(lemma_key, false).len()
    }

    /// Number of distinct spellings analyzed so far.
    pub fn cached_analysis_count(&self) -> usize {
        self.analyses.len()
    }
}

fn pack_occurrence(slot: u32, token: u32) -> u64 {
    (u64::from(slot) << 32) | u64::from(token)
}

fn unpack_occurrence(packed: u64) -> (u32, usize) {
    ((packed >> 32) as u32, packed as u32 as usize)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::*;

    fn keys(lemmas: &[&'static collo::Lemma]) -> Vec<&'static str> {
        lemmas.iter().map(|lemma| lemma.key).collect()
    }

    fn index(project: ProjectContent) -> WordIndex {
        let mut index = WordIndex::new();
        index.set_project(project);
        index
    }

    #[test]
    fn indexes_surface_occurrences_and_possible_lemmas() {
        let index = index(teacher_lesson(&[
            ("Puella rosam amat. Puella!", Language::Latin),
            ("English word puella", Language::L1),
        ]));
        assert_eq!(index.surface_count("Puella"), 2);
        assert_eq!(index.surface_count("puella"), 1);
        assert_eq!(index.possible_lemma_count("puella"), 2);
        assert_eq!(index.possible_lemma_count("rosa"), 1);
        assert_eq!(index.unique_surface_count(), 6);
    }

    #[test]
    fn updates_only_one_entry_and_removes_stale_postings() {
        let mut index = index(teacher_lesson(&[("Puella rosam", Language::Latin)]));
        index.set_entry(entry(
            "0.0",
            Participant::Teacher,
            Language::Latin,
            "Virumque amat",
        ));
        assert_eq!(index.surface_count("Puella"), 0);
        assert_eq!(index.surface_count("rosam"), 0);
        assert_eq!(index.possible_lemma_count("puella"), 0);
        assert_eq!(index.possible_lemma_count("uir"), 1);
        assert_eq!(index.unique_surface_count(), 2);
    }

    #[test]
    fn cached_text_tagging_preserves_contextual_choices() {
        let mut index = WordIndex::new();
        let cached = index.tag_text("Puella rosam amat. Puella rosam amat.");
        let direct = collo::Lemmatizer::new().tag_text("Puella rosam amat. Puella rosam amat.");
        assert_eq!(
            cached
                .iter()
                .map(|word| word.tag.as_deref())
                .collect::<Vec<_>>(),
            direct
                .iter()
                .map(|word| word.tag.as_deref())
                .collect::<Vec<_>>()
        );
        assert_eq!(index.cached_analysis_count(), 3);
    }

    #[test]
    fn first_use_lemmas_are_marked_in_project_order_and_update_with_edits() {
        let mut index = index(teacher_lesson(&[
            ("Puella puellam videt.", Language::Latin),
            ("Puella rosam amat.", Language::Latin),
            ("rosam", Language::Latin),
        ]));
        let first = keys(&index.first_uses("0.0"));
        let second = keys(&index.first_uses("0.1"));
        let third = keys(&index.first_uses("0.2"));
        assert!(first.contains(&"puella"));
        assert!(!second.contains(&"puella"));
        assert!(second.contains(&"rosa"));
        assert!(second.contains(&"amo"));
        assert!(!third.contains(&"rosa"));

        index.set_entry(entry(
            "0.0",
            Participant::Teacher,
            Language::Latin,
            "Puer venit.",
        ));
        assert!(!keys(&index.first_uses("0.0")).contains(&"puella"));
        assert!(keys(&index.first_uses("0.1")).contains(&"puella"));
    }

    #[test]
    fn first_use_labels_are_in_text_order() {
        let mut index = index(teacher_lesson(&[("rosam puella amat", Language::Latin)]));
        assert_eq!(keys(&index.first_uses("0.0")), ["rosa", "puella", "amo"]);
        index.set_entry(entry(
            "0.0",
            Participant::Teacher,
            Language::Latin,
            "amat puella rosam",
        ));
        assert_eq!(keys(&index.first_uses("0.0")), ["amo", "puella", "rosa"]);
        assert_eq!(
            keys(&index.new_lemmas_in_lesson("lesson 0")),
            ["amo", "puella", "rosa"]
        );
    }

    #[test]
    fn structural_changes_move_first_uses_without_reanalyzing() {
        let mut index = index(project(&[
            &[(Participant::Teacher, Language::Latin, "puella cantat")],
            &[
                (Participant::Student, Language::Latin, "puellam video"),
                (Participant::Teacher, Language::Latin, "rosa"),
            ],
        ]));
        let analyzed = index.cached_analysis_count();
        assert_eq!(
            keys(&index.new_lemmas_in_lesson("lesson 0")),
            ["puella", "canto"]
        );
        assert_eq!(
            keys(&index.new_lemmas_in_lesson("lesson 1")),
            ["uideo", "rosa"]
        );

        // Move the first lesson after the second, and its entry to the end.
        index.set_structure(&outline(&[
            ("lesson 1", &["1.1", "1.0"]),
            ("lesson 0", &["0.0"]),
        ]));
        assert_eq!(index.cached_analysis_count(), analyzed);
        assert_eq!(
            keys(&index.new_lemmas_in_lesson("lesson 1")),
            ["rosa", "puella", "uideo"]
        );
        assert_eq!(keys(&index.new_lemmas_in_lesson("lesson 0")), ["canto"]);
        assert_eq!(
            index.lesson_ids().map(|id| id.as_ref()).collect::<Vec<_>>(),
            ["lesson 1", "lesson 0"]
        );
        let hits: Vec<_> = index
            .lemma_occurrences("puella", false)
            .into_iter()
            .map(|hit| hit.entry_id.to_string())
            .collect();
        assert_eq!(hits, ["1.0", "0.0"], "hits follow script order");
    }

    #[test]
    fn removing_an_entry_hands_its_first_uses_on() {
        let mut index = index(teacher_lesson(&[
            ("puella", Language::Latin),
            ("rosa", Language::Latin),
            ("puellae rosas amant", Language::Latin),
        ]));
        index.remove_entry("0.0");
        assert!(!index.contains_entry("0.0"));
        assert_eq!(keys(&index.first_uses("0.2")), ["puella", "amo"]);
        assert_eq!(index.possible_lemma_count("puella"), 1);
        assert_eq!(
            index
                .lesson_entry_ids("lesson 0")
                .unwrap()
                .map(|id| id.as_ref())
                .collect::<Vec<_>>(),
            ["0.1", "0.2"]
        );
        // The freed slot is reused by the next new entry.
        index.set_entry(entry(
            "new",
            Participant::Teacher,
            Language::Latin,
            "puella",
        ));
        index.set_structure(&outline(&[("lesson 0", &["0.1", "0.2", "new"])]));
        assert_eq!(keys(&index.first_uses("new")), Vec::<&str>::new());
        assert_eq!(index.possible_lemma_count("puella"), 2);
    }

    #[test]
    fn entries_outside_the_structure_are_not_searched() {
        let mut index = index(teacher_lesson(&[("rosa", Language::Latin)]));
        // Content can arrive before the structure that places it, and the
        // other way round.
        index.set_entry(entry(
            "early",
            Participant::Student,
            Language::Latin,
            "puella",
        ));
        assert_eq!(index.possible_lemma_count("puella"), 0);
        assert!(index.first_uses("early").is_empty());
        index.set_structure(&outline(&[("lesson 0", &["0.0", "early", "late"])]));
        assert_eq!(index.possible_lemma_count("puella"), 1);
        assert_eq!(keys(&index.first_uses("early")), ["puella"]);
        assert_eq!(index.entry_text("late"), Some(""));
        index.set_entry(entry(
            "late",
            Participant::Teacher,
            Language::Latin,
            "puella rosa amat",
        ));
        assert_eq!(keys(&index.first_uses("late")), ["amo"]);
        // Listed twice, an entry is placed once.
        index.set_structure(&outline(&[("lesson 0", &["late", "0.0", "late"])]));
        assert_eq!(index.lemma_occurrences("rosa", false).len(), 2);
        assert_eq!(keys(&index.first_uses("late")), ["puella", "rosa", "amo"]);
    }

    #[test]
    fn a_participant_change_updates_counts_only() {
        let mut index = index(teacher_lesson(&[("puella", Language::Latin)]));
        let analyzed = index.cached_analysis_count();
        let hits = index.lemma_occurrences("puella", false);
        assert_eq!(index.counts_per_lesson(&hits)[0].1, [1, 0]);
        index.set_entry(entry(
            "0.0",
            Participant::Student,
            Language::Latin,
            "puella",
        ));
        assert_eq!(index.counts_per_lesson(&hits)[0].1, [0, 1]);
        assert_eq!(index.cached_analysis_count(), analyzed);
    }

    #[test]
    fn switching_language_indexes_or_drops_lemmas() {
        let mut index = index(teacher_lesson(&[("puella", Language::L1)]));
        assert_eq!(index.possible_lemma_count("puella"), 0);
        assert!(index.first_uses("0.0").is_empty());
        index.set_entry(entry(
            "0.0",
            Participant::Teacher,
            Language::Latin,
            "puella",
        ));
        assert_eq!(index.possible_lemma_count("puella"), 1);
        assert_eq!(keys(&index.first_uses("0.0")), ["puella"]);
    }

    /// A deterministic xorshift generator.
    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }

        fn below(&mut self, n: usize) -> usize {
            (self.next() % n as u64) as usize
        }
    }

    /// Random edits applied incrementally leave the index as a full rebuild
    /// of the same project would.
    #[test]
    fn incremental_updates_match_a_rebuild() {
        const WORDS: &[&str] = &[
            "puella", "Puella", "puellae", "rosam", "amat", "et", "est", "malum", "mālum", "malae",
            "mālae", "bellum", "xyzzy", "video", "amicas", "aqua", "Rosa",
        ];
        const KEYS: &[&str] = &[
            "puella", "rosa", "amo", "sum", "malum", "malus", "mala", "bellum", "bellus", "et",
        ];
        let mut rng = Rng(0x2545_f491_4f6c_dd1d);
        let mut next_id = 0;
        let mut random_entry = |rng: &mut Rng| {
            next_id += 1;
            let words: Vec<_> = (0..1 + rng.below(5))
                .map(|_| WORDS[rng.below(WORDS.len())])
                .collect();
            let mut content = entry(
                &format!("e{next_id}"),
                if rng.below(2) == 0 {
                    Participant::Teacher
                } else {
                    Participant::Student
                },
                if rng.below(5) == 0 {
                    Language::L1
                } else {
                    Language::Latin
                },
                &words.join(" "),
            );
            if rng.below(3) == 0 {
                content.lemmas.push(LemmaChoice {
                    token: "bellum".into(),
                    occurrence: 0,
                    lemma_key: "bellus".into(),
                    extension: false,
                });
            }
            content
        };
        let mut lessons: Vec<LessonContent> = (0..4)
            .map(|lesson| LessonContent {
                id: lesson_id(lesson),
                entries: (0..5).map(|_| random_entry(&mut rng)).collect(),
            })
            .collect();
        let snapshot = |lessons: &[LessonContent]| ProjectContent {
            l1_name: "English".into(),
            lessons: lessons.to_vec(),
        };
        let structure = |lessons: &[LessonContent]| -> Vec<LessonOutline> {
            lessons
                .iter()
                .map(|lesson| LessonOutline {
                    id: lesson.id.clone(),
                    entry_ids: lesson
                        .entries
                        .iter()
                        .map(|entry| entry.id.clone())
                        .collect(),
                })
                .collect()
        };
        let mut incremental = index(snapshot(&lessons));
        let mut next_lesson = 0;
        for step in 0..400 {
            let lesson = rng.below(lessons.len());
            let count = lessons[lesson].entries.len();
            match rng.below(6) {
                0 | 1 if count > 0 => {
                    let at = rng.below(count);
                    let mut edited = random_entry(&mut rng);
                    edited.id = lessons[lesson].entries[at].id.clone();
                    lessons[lesson].entries[at] = edited.clone();
                    incremental.set_entry(edited);
                }
                2 if count > 0 => {
                    let moved = lessons[lesson].entries.remove(rng.below(count));
                    let to = rng.below(lessons.len());
                    let at = rng.below(lessons[to].entries.len() + 1);
                    lessons[to].entries.insert(at, moved);
                    incremental.set_structure(&structure(&lessons));
                }
                3 if count > 0 => {
                    let removed = lessons[lesson].entries.remove(rng.below(count));
                    incremental.remove_entry(&removed.id);
                }
                4 => {
                    let added = random_entry(&mut rng);
                    lessons[lesson]
                        .entries
                        .insert(rng.below(count + 1), added.clone());
                    // Content and structure may arrive in either order.
                    if rng.below(2) == 0 {
                        incremental.set_entry(added);
                        incremental.set_structure(&structure(&lessons));
                    } else {
                        incremental.set_structure(&structure(&lessons));
                        incremental.set_entry(added);
                    }
                }
                5 if lessons.len() > 1 && rng.below(4) == 0 => {
                    // Deleting a lesson deletes its entries.
                    for removed in lessons.remove(lesson).entries {
                        incremental.remove_entry(&removed.id);
                    }
                    incremental.set_structure(&structure(&lessons));
                }
                5 if rng.below(3) == 0 => {
                    next_lesson += 1;
                    let added = LessonContent {
                        id: format!("new lesson {next_lesson}"),
                        entries: Vec::new(),
                    };
                    lessons.insert(rng.below(lessons.len() + 1), added);
                    incremental.set_structure(&structure(&lessons));
                }
                _ => {
                    let moved = lessons.remove(lesson);
                    lessons.insert(rng.below(lessons.len() + 1), moved);
                    incremental.set_structure(&structure(&lessons));
                }
            }
            let rebuilt = index(snapshot(&lessons));
            for entry in lessons.iter().flat_map(|lesson| &lesson.entries) {
                assert_eq!(
                    keys(&incremental.first_uses(&entry.id)),
                    keys(&rebuilt.first_uses(&entry.id)),
                    "first uses of {} after step {step}",
                    entry.id
                );
            }
            for lesson in &lessons {
                assert_eq!(
                    keys(&incremental.new_lemmas_in_lesson(&lesson.id)),
                    keys(&rebuilt.new_lemmas_in_lesson(&lesson.id)),
                    "step {step}"
                );
            }
            for key in KEYS {
                for unmarked_short in [false, true] {
                    let hits = incremental.lemma_occurrences(key, unmarked_short);
                    assert_eq!(
                        hits,
                        rebuilt.lemma_occurrences(key, unmarked_short),
                        "{key} after step {step}"
                    );
                    assert_eq!(
                        incremental.counts_per_lesson(&hits),
                        rebuilt.counts_per_lesson(&hits)
                    );
                }
            }
            assert_eq!(
                incremental.text_hits("a"),
                rebuilt.text_hits("a"),
                "step {step}"
            );
            assert_eq!(
                incremental.unique_surface_count(),
                rebuilt.unique_surface_count()
            );
        }
    }
}
