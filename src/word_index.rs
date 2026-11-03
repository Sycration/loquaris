//! Compact project-wide word and lemma indexes backed by cached Collo analyses.
use std::collections::{BTreeSet, HashMap, HashSet};

use crate::{Language, Lesson};

const CACHE_SCHEMA_VERSION: u32 = 1;
const NO_ID: u32 = u32::MAX;
const NO_POSTING: u64 = u64::MAX;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct AnalysisKey {
    token: String,
    sentence_start: bool,
    include_extension: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct AnalysisLookupKey {
    surface: u32,
    sentence_start: bool,
    include_extension: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Occurrence {
    pub lesson: usize,
    pub entry: usize,
    pub range: std::ops::Range<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct PersistedCache {
    pub(crate) schema_version: u32,
    pub(crate) fingerprint: u64,
    pub(crate) words: Vec<PersistedWord>,
}

impl Default for PersistedCache {
    fn default() -> Self {
        Self {
            schema_version: CACHE_SCHEMA_VERSION,
            fingerprint: collo::ANALYSIS_CACHE_FINGERPRINT,
            words: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub(crate) struct PersistedWord {
    pub(crate) token: String,
    pub(crate) sentence_start: bool,
    pub(crate) include_extension: bool,
    pub(crate) analyses: Vec<PersistedAnalysis>,
}

#[derive(Clone, Debug, PartialEq, Eq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub(crate) struct PersistedAnalysis {
    pub(crate) lemma_key: String,
    pub(crate) extension: bool,
    pub(crate) morphology_id: u16,
    pub(crate) form: String,
    pub(crate) suffix: String,
    pub(crate) irregular: bool,
    pub(crate) multiplicity: u32,
}

#[derive(Clone, Copy, Debug)]
struct IndexedToken {
    range_start: u32,
    range_end: u32,
    surface: u32,
    surface_position: u32,
    analysis: u32,
    selected_lemma: u32,
}

struct SurfaceRecord {
    /// Keep the common single-occurrence case inline; repeated spellings use
    /// the overflow vector. Packed entry/token coordinates are unordered.
    first_occurrence: u64,
    other_occurrences: Vec<u64>,
    /// Candidate lemmas seen for this exact spelling. Per-occurrence analyses
    /// are checked when returning lemma hits, so historical candidates are safe.
    lemmas: Option<Box<[u32]>>,
    normalized_indexed: bool,
}

impl Default for SurfaceRecord {
    fn default() -> Self {
        Self {
            first_occurrence: NO_POSTING,
            other_occurrences: Vec::new(),
            lemmas: None,
            normalized_indexed: false,
        }
    }
}

/// Project-local occurrence indexes and unique-token analyses. Per-token data
/// and postings use integer IDs so large scripts do not allocate tree nodes or
/// duplicate token strings for every occurrence.
#[derive(Default)]
pub struct WordIndex {
    analysis_ids: HashMap<AnalysisLookupKey, u32>,
    analysis_keys: Vec<AnalysisKey>,
    analyses: Vec<std::sync::Arc<[collo::Analysis]>>,
    /// Lemmas of the analyses not excluded by a written macron or breve.
    analysis_lemmas: Vec<std::sync::Arc<[u32]>>,
    /// The subset that remains when unmarked vowels are read as short.
    analysis_short_lemmas: Vec<std::sync::Arc<[u32]>>,
    /// The most frequent of those lemmas, used for first-use labels.
    analysis_selected_lemma: Vec<u32>,
    analysis_users: Vec<usize>,
    lemma_ids: HashMap<(&'static str, bool), u32>,
    lemmas: Vec<&'static collo::Lemma>,

    entries: Vec<Vec<Vec<IndexedToken>>>,
    entry_ids: Vec<Vec<u32>>,
    entry_coordinates: Vec<(u32, u32)>,
    surfaces: Vec<SurfaceRecord>,
    by_surface: HashMap<std::sync::Arc<str>, u32>,
    by_normalized_surface: HashMap<std::sync::Arc<str>, u32>,
    normalized_surface_collisions: HashMap<std::sync::Arc<str>, Vec<u32>>,
    by_lemma: HashMap<&'static str, Vec<u32>>,

    first_use_by_lemma: HashMap<u32, (usize, usize)>,
    first_lemmas_by_entry: Vec<Vec<Vec<u32>>>,
    active_surfaces: usize,
}

impl WordIndex {
    pub fn from_project(cache: &PersistedCache, lessons: &[Lesson]) -> Self {
        let mut index = Self::default();
        index.load_cache(cache);
        index.rebuild(lessons);
        index
    }

    fn load_cache(&mut self, cache: &PersistedCache) {
        if cache.schema_version != CACHE_SCHEMA_VERSION
            || cache.fingerprint != collo::ANALYSIS_CACHE_FINGERPRINT
        {
            return;
        }
        for word in &cache.words {
            let mut analyses = Vec::with_capacity(word.analyses.len());
            let mut valid = true;
            for stored in &word.analyses {
                let Some(lemma) = collo::lemmas().iter().find(|lemma| {
                    lemma.key == stored.lemma_key && lemma.extension == stored.extension
                }) else {
                    valid = false;
                    break;
                };
                if collo::morphology(stored.morphology_id, "en").is_none()
                    || stored.multiplicity == 0
                {
                    valid = false;
                    break;
                }
                analyses.push(collo::Analysis {
                    lemma,
                    morphology_id: stored.morphology_id,
                    form: stored.form.clone(),
                    suffix: stored.suffix.clone(),
                    irregular: stored.irregular,
                    multiplicity: stored.multiplicity,
                });
            }
            if valid {
                let surface = self.intern_surface(&word.token);
                let lookup = AnalysisLookupKey {
                    surface,
                    sentence_start: word.sentence_start,
                    include_extension: word.include_extension,
                };
                let key = AnalysisKey {
                    token: word.token.clone(),
                    sentence_start: word.sentence_start,
                    include_extension: word.include_extension,
                };
                self.store_analysis(lookup, key, analyses.into());
            }
        }
    }

    fn store_analysis(
        &mut self,
        lookup: AnalysisLookupKey,
        key: AnalysisKey,
        analyses: std::sync::Arc<[collo::Analysis]>,
    ) -> u32 {
        if let Some(&id) = self.analysis_ids.get(&lookup) {
            return id;
        }
        // The cache stores every analysis (auto-macronize uses them all); only
        // readings that fit the written quantities identify the word's lemma.
        let preferred = |unmarked_short| -> Vec<&collo::Analysis> {
            crate::macronize::matching_analyses(&key.token, &analyses, unmarked_short)
                .into_iter()
                .map(|index| &analyses[index])
                .collect()
        };
        let (preferred, preferred_short) = (preferred(false), preferred(true));
        let selected_lemma = preferred
            .iter()
            .max_by_key(|analysis| analysis.lemma.frequency)
            .map_or(NO_ID, |analysis| self.intern_lemma(analysis.lemma));
        let mut lemma_set = |analyses: &[&collo::Analysis]| -> std::sync::Arc<[u32]> {
            analyses
                .iter()
                .map(|analysis| self.intern_lemma(analysis.lemma))
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>()
                .into()
        };
        let lemma_ids = lemma_set(&preferred);
        let short_lemma_ids = lemma_set(&preferred_short);
        let id = u32::try_from(self.analyses.len()).expect("too many unique project words");
        self.analysis_ids.insert(lookup, id);
        self.analysis_keys.push(key);
        self.analyses.push(analyses);
        self.analysis_lemmas.push(lemma_ids);
        self.analysis_short_lemmas.push(short_lemma_ids);
        self.analysis_selected_lemma.push(selected_lemma);
        self.analysis_users.push(0);
        id
    }

    fn intern_lemma(&mut self, lemma: &'static collo::Lemma) -> u32 {
        let identity = (lemma.key, lemma.extension);
        if let Some(&id) = self.lemma_ids.get(&identity) {
            return id;
        }
        let id = u32::try_from(self.lemmas.len()).expect("too many unique lemmas");
        self.lemma_ids.insert(identity, id);
        self.lemmas.push(lemma);
        id
    }

    fn analysis_id(
        &mut self,
        analyzer: &collo::Lemmatizer,
        token: &str,
        sentence_start: bool,
        surface: u32,
    ) -> u32 {
        let lookup = AnalysisLookupKey {
            surface,
            sentence_start,
            include_extension: analyzer.include_extension,
        };
        if let Some(&id) = self.analysis_ids.get(&lookup) {
            return id;
        }
        let key = AnalysisKey {
            token: token.to_owned(),
            sentence_start,
            include_extension: analyzer.include_extension,
        };
        let analyses = analyzer.analyze(token, sentence_start);
        self.store_analysis(lookup, key, analyses.into())
    }

    pub fn analyses(
        &mut self,
        analyzer: &collo::Lemmatizer,
        token: &str,
        sentence_start: bool,
    ) -> std::sync::Arc<[collo::Analysis]> {
        let surface = self.intern_surface(token);
        let id = self.analysis_id(analyzer, token, sentence_start, surface) as usize;
        self.analyses[id].clone()
    }

    pub fn tag_text(
        &mut self,
        analyzer: &collo::Lemmatizer,
        text: &str,
    ) -> Vec<collo::tagging::TaggedWord> {
        analyzer.tag_text_with(text, |token, sentence_start| {
            self.analyses(analyzer, token, sentence_start)
                .iter()
                .cloned()
                .collect()
        })
    }

    pub fn persisted_cache(&self) -> PersistedCache {
        let mut words: Vec<_> = self
            .analysis_keys
            .iter()
            .zip(&self.analyses)
            .zip(&self.analysis_users)
            .filter(|((_, _), users)| **users != 0)
            .map(|((key, analyses), _)| PersistedWord {
                token: key.token.clone(),
                sentence_start: key.sentence_start,
                include_extension: key.include_extension,
                analyses: analyses
                    .iter()
                    .map(|analysis| PersistedAnalysis {
                        lemma_key: analysis.lemma.key.to_owned(),
                        extension: analysis.lemma.extension,
                        morphology_id: analysis.morphology_id,
                        form: analysis.form.clone(),
                        suffix: analysis.suffix.clone(),
                        irregular: analysis.irregular,
                        multiplicity: analysis.multiplicity,
                    })
                    .collect(),
            })
            .collect();
        words.sort_by(|left, right| {
            (&left.token, left.sentence_start, left.include_extension).cmp(&(
                &right.token,
                right.sentence_start,
                right.include_extension,
            ))
        });
        PersistedCache {
            schema_version: CACHE_SCHEMA_VERSION,
            fingerprint: collo::ANALYSIS_CACHE_FINGERPRINT,
            words,
        }
    }

    /// Rebuild occurrence indexes after lesson/entry structure or language
    /// changes. Unique-token analyses are retained through the rebuild.
    pub fn rebuild(&mut self, lessons: &[Lesson]) {
        self.entries.clear();
        self.entry_ids.clear();
        self.entry_coordinates.clear();
        for surface in &mut self.surfaces {
            surface.first_occurrence = NO_POSTING;
            surface.other_occurrences.clear();
            surface.lemmas = None;
        }
        self.by_lemma.clear();
        self.first_use_by_lemma.clear();
        self.first_lemmas_by_entry.clear();
        self.active_surfaces = 0;
        self.analysis_users.fill(0);

        self.entries = lessons
            .iter()
            .map(|lesson| (0..lesson.entries.len()).map(|_| Vec::new()).collect())
            .collect();
        self.entry_ids = lessons
            .iter()
            .map(|lesson| vec![NO_ID; lesson.entries.len()])
            .collect();
        self.first_lemmas_by_entry = lessons
            .iter()
            .map(|lesson| (0..lesson.entries.len()).map(|_| Vec::new()).collect())
            .collect();

        let analyzer = collo::Lemmatizer::new();
        for (lesson_index, lesson) in lessons.iter().enumerate() {
            for (entry_index, entry) in lesson.entries.iter().enumerate() {
                self.ensure_entry_id(lesson_index, entry_index);
                self.update_entry(
                    &analyzer,
                    lesson_index,
                    entry_index,
                    &entry.text,
                    entry.language,
                );
            }
        }
    }

    fn ensure_entry_id(&mut self, lesson: usize, entry: usize) -> u32 {
        if self.entries.len() <= lesson {
            self.entries.resize_with(lesson + 1, Vec::new);
            self.entry_ids.resize_with(lesson + 1, Vec::new);
            self.first_lemmas_by_entry.resize_with(lesson + 1, Vec::new);
        }
        if self.entries[lesson].len() <= entry {
            self.entries[lesson].resize_with(entry + 1, Vec::new);
            self.entry_ids[lesson].resize(entry + 1, NO_ID);
            self.first_lemmas_by_entry[lesson].resize_with(entry + 1, Vec::new);
        }
        if self.entry_ids[lesson][entry] == NO_ID {
            let id = u32::try_from(self.entry_coordinates.len()).expect("too many entries");
            self.entry_ids[lesson][entry] = id;
            self.entry_coordinates.push((lesson as u32, entry as u32));
        }
        self.entry_ids[lesson][entry]
    }

    fn intern_surface(&mut self, surface: &str) -> u32 {
        if let Some(&id) = self.by_surface.get(surface) {
            return id;
        }
        let surface_text: std::sync::Arc<str> = surface.into();
        let id = u32::try_from(self.surfaces.len()).expect("too many unique spellings");
        self.surfaces.push(SurfaceRecord::default());
        self.by_surface.insert(surface_text, id);
        id
    }

    fn index_normalized_surface(&mut self, id: u32, surface: &str) {
        if self.surfaces[id as usize].normalized_indexed {
            return;
        }
        let normalized = collo::normalize(surface).to_lowercase();
        let normalized_text = if normalized == surface {
            self.by_surface
                .get_key_value(surface)
                .expect("surface was interned")
                .0
                .clone()
        } else if let Some((existing, _)) = self
            .by_normalized_surface
            .get_key_value(normalized.as_str())
        {
            existing.clone()
        } else {
            normalized.into()
        };
        if self
            .by_normalized_surface
            .contains_key(normalized_text.as_ref())
        {
            self.normalized_surface_collisions
                .entry(normalized_text)
                .or_default()
                .push(id);
        } else {
            self.by_normalized_surface.insert(normalized_text, id);
        }
        self.surfaces[id as usize].normalized_indexed = true;
    }

    /// Retokenize one entry. Lexical ambiguity is resolved cheaply using the
    /// highest-frequency candidate; full contextual tagging remains available
    /// through `tag_text` for explicit parsing review.
    pub fn update_entry(
        &mut self,
        analyzer: &collo::Lemmatizer,
        lesson: usize,
        entry: usize,
        text: &str,
        language: Language,
    ) {
        let entry_id = self.ensure_entry_id(lesson, entry);
        let mut old = std::mem::take(&mut self.entries[lesson][entry]);
        let mut affected_lemmas = HashSet::new();

        for token_index in 0..old.len() {
            let token = old[token_index];
            if token.selected_lemma != NO_ID {
                affected_lemmas.insert(token.selected_lemma);
            }
            if token.analysis != NO_ID {
                self.analysis_users[token.analysis as usize] =
                    self.analysis_users[token.analysis as usize].saturating_sub(1);
            }

            let posting = pack_occurrence(entry_id, token_index as u32);
            let surface = &mut self.surfaces[token.surface as usize];
            if surface.first_occurrence == posting {
                if let Some(moved) = surface.other_occurrences.pop() {
                    surface.first_occurrence = moved;
                    let moved_entry = (moved >> 32) as u32;
                    let moved_index = moved as u32 as usize;
                    if moved_entry == entry_id {
                        old[moved_index].surface_position = NO_ID;
                    } else {
                        let (moved_lesson, moved_entry_index) =
                            self.entry_coordinates[moved_entry as usize];
                        self.entries[moved_lesson as usize][moved_entry_index as usize]
                            [moved_index]
                            .surface_position = NO_ID;
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
                    let moved_entry = (last >> 32) as u32;
                    let moved_index = last as u32 as usize;
                    if moved_entry == entry_id {
                        old[moved_index].surface_position = position as u32;
                    } else {
                        let (moved_lesson, moved_entry_index) =
                            self.entry_coordinates[moved_entry as usize];
                        self.entries[moved_lesson as usize][moved_entry_index as usize]
                            [moved_index]
                            .surface_position = position as u32;
                    }
                }
            }
        }

        let mut indexed = Vec::new();
        for token in collo::tagging::tokenize(text) {
            let token_text = &text[token.range.clone()];
            let surface_id = self.intern_surface(token_text);
            if language == Language::Latin {
                self.index_normalized_surface(surface_id, token_text);
            }
            let analysis = if language == Language::Latin {
                self.analysis_id(analyzer, token_text, token.sentence_start, surface_id)
            } else {
                NO_ID
            };
            let selected_lemma = if analysis != NO_ID {
                self.analysis_users[analysis as usize] += 1;
                self.analysis_selected_lemma[analysis as usize]
            } else {
                NO_ID
            };
            if selected_lemma != NO_ID {
                affected_lemmas.insert(selected_lemma);
            }

            let mut new_surface_lemmas = Vec::new();
            if analysis != NO_ID {
                let lemma_ids = self.analysis_lemmas[analysis as usize].clone();
                for lemma_id in lemma_ids.iter().copied() {
                    let already_present = self.surfaces[surface_id as usize]
                        .lemmas
                        .as_ref()
                        .is_some_and(|lemmas| lemmas.contains(&lemma_id));
                    if !already_present {
                        new_surface_lemmas.push(lemma_id);
                    }
                }
            }

            let token_index = indexed.len();
            let surface = &mut self.surfaces[surface_id as usize];
            let posting = pack_occurrence(entry_id, token_index as u32);
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
                for lemma in new_surface_lemmas.iter().copied() {
                    if !combined.contains(&lemma) {
                        combined.push(lemma);
                    }
                }
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
            });
        }
        self.entries[lesson][entry] = indexed;
        for lemma in affected_lemmas {
            self.refresh_first_use(lemma, lesson, entry);
        }
    }

    fn refresh_first_use(&mut self, lemma: u32, lesson: usize, entry: usize) {
        let current_has_lemma = self.entries[lesson][entry]
            .iter()
            .any(|token| token.selected_lemma == lemma);
        let current = (lesson, entry);
        let previous = self.first_use_by_lemma.get(&lemma).copied();

        if current_has_lemma {
            if previous.is_none_or(|first| current < first) {
                if let Some(first) = previous {
                    self.remove_first_use_label(lemma, first);
                }
                self.first_use_by_lemma.insert(lemma, current);
                self.first_lemmas_by_entry[lesson][entry].push(lemma);
            }
        } else if previous == Some(current) {
            self.remove_first_use_label(lemma, current);
            self.first_use_by_lemma.remove(&lemma);
            if let Some(first) = self.find_first_use(lemma) {
                self.first_use_by_lemma.insert(lemma, first);
                self.first_lemmas_by_entry[first.0][first.1].push(lemma);
            }
        }
    }

    fn remove_first_use_label(&mut self, lemma: u32, position: (usize, usize)) {
        self.first_lemmas_by_entry[position.0][position.1].retain(|item| *item != lemma);
    }

    fn find_first_use(&self, lemma: u32) -> Option<(usize, usize)> {
        self.entries
            .iter()
            .enumerate()
            .find_map(|(lesson, entries)| {
                entries
                    .iter()
                    .enumerate()
                    .find(|(_, tokens)| tokens.iter().any(|token| token.selected_lemma == lemma))
                    .map(|(entry, _)| (lesson, entry))
            })
    }

    pub fn first_use_lemmas(&self, lesson: usize, entry: usize) -> Vec<String> {
        self.first_lemmas_by_entry
            .get(lesson)
            .and_then(|entries| entries.get(entry))
            .into_iter()
            .flatten()
            .map(|&lemma| crate::macronize::reader_form(self.lemmas[lemma as usize].headword))
            .collect()
    }

    /// Lemmas whose first use in the project is in `lesson`, in script order.
    pub fn new_lemmas_in_lesson(&self, lesson: usize) -> Vec<&'static collo::Lemma> {
        self.first_lemmas_by_entry
            .get(lesson)
            .into_iter()
            .flatten()
            .flatten()
            .map(|&lemma| self.lemmas[lemma as usize])
            .collect()
    }

    fn occurrence(&self, packed: u64) -> Option<Occurrence> {
        let entry_id = (packed >> 32) as usize;
        let token_index = packed as u32 as usize;
        let (lesson, entry) = *self.entry_coordinates.get(entry_id)?;
        let token = self
            .entries
            .get(lesson as usize)?
            .get(entry as usize)?
            .get(token_index)?;
        Some(Occurrence {
            lesson: lesson as usize,
            entry: entry as usize,
            range: token.range_start as usize..token.range_end as usize,
        })
    }

    fn surface_occurrences_by_id(&self, surface: u32) -> Vec<Occurrence> {
        let surface = &self.surfaces[surface as usize];
        std::iter::once(surface.first_occurrence)
            .chain(surface.other_occurrences.iter().copied())
            .filter(|&packed| packed != NO_POSTING)
            .filter_map(|packed| self.occurrence(packed))
            .collect()
    }

    #[allow(dead_code)] // Used by upcoming project-wide search features.
    pub fn surface_occurrences(&self, surface: &str) -> Option<Vec<Occurrence>> {
        let id = *self.by_surface.get(surface)?;
        Some(self.surface_occurrences_by_id(id))
    }

    #[allow(dead_code)] // Used by upcoming project-wide search features.
    /// Look up Latin surface variants normalized by Collo.
    pub fn normalized_surface_occurrences(&self, surface: &str) -> Option<Vec<Occurrence>> {
        let key = collo::normalize(surface).to_lowercase();
        let first = *self.by_normalized_surface.get(key.as_str())?;
        let collisions = self.normalized_surface_collisions.get(key.as_str());
        let mut occurrences = self.surface_occurrences_by_id(first);
        if let Some(collisions) = collisions {
            for &surface_id in collisions {
                occurrences.extend(self.surface_occurrences_by_id(surface_id));
            }
        }
        Some(occurrences)
    }

    /// Latin words with this lemma among their readings. With `unmarked_short`,
    /// unmarked vowels are read as short when a reading with them short exists.
    pub fn lemma_occurrences(
        &self,
        lemma_key: &str,
        unmarked_short: bool,
    ) -> Option<Vec<Occurrence>> {
        let lemma_sets = if unmarked_short {
            &self.analysis_short_lemmas
        } else {
            &self.analysis_lemmas
        };
        let surfaces = self.by_lemma.get(lemma_key)?;
        let occurrences = surfaces
            .iter()
            .flat_map(|&surface| {
                let surface = &self.surfaces[surface as usize];
                std::iter::once(surface.first_occurrence)
                    .chain(surface.other_occurrences.iter().copied())
                    .filter(|&packed| packed != NO_POSTING)
                    .filter_map(|packed| {
                        let occurrence = self.occurrence(packed)?;
                        let token_index = packed as u32 as usize;
                        let entry_id = (packed >> 32) as usize;
                        let (lesson, entry) = self.entry_coordinates[entry_id];
                        let token = &self.entries[lesson as usize][entry as usize][token_index];
                        let has_lemma = token.analysis != NO_ID
                            && lemma_sets[token.analysis as usize]
                                .iter()
                                .any(|&lemma| self.lemmas[lemma as usize].key == lemma_key);
                        has_lemma.then_some(occurrence)
                    })
            })
            .collect();
        Some(occurrences)
    }

    #[allow(dead_code)] // Used by upcoming project-wide word statistics.
    pub fn unique_surface_count(&self) -> usize {
        self.active_surfaces
    }

    #[allow(dead_code)] // Used by upcoming project-wide word statistics.
    pub fn surface_count(&self, surface: &str) -> usize {
        self.by_surface.get(surface).map_or(0, |&id| {
            let record = &self.surfaces[id as usize];
            usize::from(record.first_occurrence != NO_POSTING) + record.other_occurrences.len()
        })
    }

    /// Number of Latin occurrences with this lemma among their candidate analyses.
    #[allow(dead_code)] // Used by upcoming project-wide lemma statistics.
    pub fn possible_lemma_count(&self, lemma_key: &str) -> usize {
        self.lemma_occurrences(lemma_key, false)
            .map_or(0, |hits| hits.len())
    }
}

fn pack_occurrence(entry: u32, token: u32) -> u64 {
    (u64::from(entry) << 32) | u64::from(token)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Entry, Participant};

    fn lesson(texts: &[(&str, Language)]) -> Lesson {
        Lesson {
            name: "Test".into(),
            expanded: true,
            entries: texts
                .iter()
                .map(|(text, language)| Entry {
                    participant: Participant::Teacher,
                    language: *language,
                    text: (*text).into(),
                    audio: None,
                    trim: None,
                })
                .collect(),
        }
    }

    #[test]
    fn indexes_surface_occurrences_and_possible_lemmas() {
        let lessons = vec![lesson(&[
            ("Puella rosam amat. Puella!", Language::Latin),
            ("English word puella", Language::L1),
        ])];
        let mut index = WordIndex::default();
        index.rebuild(&lessons);
        assert_eq!(index.surface_count("Puella"), 2);
        assert_eq!(index.surface_count("puella"), 1);
        assert_eq!(
            index
                .normalized_surface_occurrences("PUELLĀ")
                .unwrap()
                .len(),
            2
        );
        assert_eq!(index.possible_lemma_count("puella"), 2);
        assert_eq!(index.possible_lemma_count("rosa"), 1);
        assert_eq!(index.unique_surface_count(), 6);
    }

    #[test]
    fn updates_only_one_entry_and_removes_stale_postings() {
        let lessons = vec![lesson(&[("Puella rosam", Language::Latin)])];
        let mut index = WordIndex::default();
        index.rebuild(&lessons);
        index.update_entry(
            &collo::Lemmatizer::new(),
            0,
            0,
            "Virumque amat",
            Language::Latin,
        );
        assert_eq!(index.surface_count("Puella"), 0);
        assert_eq!(index.possible_lemma_count("puella"), 0);
        assert_eq!(index.possible_lemma_count("uir"), 1);
        assert!(
            index
                .persisted_cache()
                .words
                .iter()
                .all(|word| word.token != "Puella" && word.token != "rosam")
        );
    }

    #[test]
    fn persisted_cache_round_trips_and_is_reused() {
        let lessons = vec![lesson(&[("Puella puella", Language::Latin)])];
        let mut first = WordIndex::default();
        first.rebuild(&lessons);
        let cache = first.persisted_cache();
        let restored = WordIndex::from_project(&cache, &lessons);
        assert_eq!(restored.possible_lemma_count("puella"), 2);
        assert_eq!(restored.persisted_cache(), cache);
    }

    #[test]
    fn stale_cache_fingerprint_is_ignored_and_rebuilt() {
        let lessons = vec![lesson(&[("Puella puella", Language::Latin)])];
        let mut index = WordIndex::default();
        index.rebuild(&lessons);
        let mut persisted = index.persisted_cache();
        persisted.fingerprint ^= 1;
        let restored = WordIndex::from_project(&persisted, &lessons);
        assert_eq!(restored.possible_lemma_count("puella"), 2);
        assert_eq!(
            restored.persisted_cache().fingerprint,
            collo::ANALYSIS_CACHE_FINGERPRINT
        );
    }

    #[test]
    fn cached_text_tagging_preserves_contextual_choices() {
        let analyzer = collo::Lemmatizer::new();
        let mut index = WordIndex::default();
        let cached = index.tag_text(&analyzer, "Puella rosam amat. Puella rosam amat.");
        let direct = analyzer.tag_text("Puella rosam amat. Puella rosam amat.");
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
        assert_eq!(index.analyses.len(), 3);
    }

    #[test]
    fn first_use_lemmas_are_marked_in_project_order_and_update_with_edits() {
        let lessons = vec![lesson(&[
            ("Puella puellam videt.", Language::Latin),
            ("Puella rosam amat.", Language::Latin),
            ("rosam", Language::Latin),
        ])];
        let mut index = WordIndex::default();
        index.rebuild(&lessons);
        let first = index.first_use_lemmas(0, 0);
        let second = index.first_use_lemmas(0, 1);
        let third = index.first_use_lemmas(0, 2);
        let headword = |key: &str| {
            crate::macronize::reader_form(
                collo::lemmas()
                    .iter()
                    .find(|lemma| lemma.key == key)
                    .unwrap()
                    .headword,
            )
        };
        assert!(first.contains(&headword("puella")));
        assert!(!second.contains(&headword("puella")));
        assert!(second.contains(&headword("rosa")));
        assert!(second.contains(&headword("amo")));
        assert!(!third.contains(&headword("rosa")));

        index.update_entry(
            &collo::Lemmatizer::new(),
            0,
            0,
            "Puer venit.",
            Language::Latin,
        );
        assert!(!index.first_use_lemmas(0, 0).contains(&headword("puella")));
        assert!(index.first_use_lemmas(0, 1).contains(&headword("puella")));
    }
}
