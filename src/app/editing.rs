//! Script editing operations, each recorded as an undoable command.
use super::{App, Command, Position, TextTarget};
use crate::{Entry, Language, Lesson, Participant, keybinds, parsing_review};
use std::collections::BTreeSet;

#[derive(Clone, Copy)]
pub enum EntryAction {
    Duplicate,
    Delete,
    MoveParticipant,
    SwitchLanguage,
    MoveUp,
    MoveDown,
    AutoMacronize,
}

#[derive(Clone, Copy)]
pub enum BulkAction {
    Duplicate,
    Delete,
    ToTeacher,
    ToStudent,
    ToL1,
    ToLatin,
    ToggleLanguage,
    MoveUp,
    MoveDown,
}

#[derive(Clone, Copy)]
pub enum MenuAction {
    Single(EntryAction),
    Bulk(BulkAction),
}

impl BulkAction {
    /// Change one entry's participant or language. Students only speak Latin.
    fn convert(self, entry: &mut Entry) {
        let teacher = entry.participant == Participant::Teacher;
        match self {
            BulkAction::ToTeacher if !teacher => entry.participant = Participant::Teacher,
            BulkAction::ToStudent if teacher => {
                entry.participant = Participant::Student;
                entry.language = Language::Latin;
            }
            BulkAction::ToL1 if teacher => entry.language = Language::L1,
            BulkAction::ToLatin if teacher => entry.language = Language::Latin,
            BulkAction::ToggleLanguage if teacher => entry.language = entry.language.toggled(),
            _ => {}
        }
    }
}

/// Swap a run of entries with the neighbour just outside it. The run and
/// neighbour are given as `slots`; `up` rotates the neighbour to the end.
fn rotate_slots<T: Clone>(tagged: &mut [Vec<T>], slots: Vec<Position>, up: bool) {
    let mut values: Vec<_> = slots
        .iter()
        .map(|&(lesson, index)| tagged[lesson][index].clone())
        .collect();
    if up {
        values.rotate_left(1);
    } else {
        values.rotate_right(1);
    }
    for ((lesson, index), value) in slots.into_iter().zip(values) {
        tagged[lesson][index] = value;
    }
}

impl App {
    /// Insert a new entry after the selection, or at the end of the active lesson.
    pub fn add(&mut self, participant: Participant, language: Language) {
        self.lesson_playback = None;
        self.lesson_recording_summaries = None;
        let lesson_index = self.target_lesson();
        let lesson = &mut self.lessons[lesson_index];
        lesson.expanded = true;
        let entry_index = self
            .selected_entry
            .map_or(lesson.entries.len(), |(_, entry)| entry + 1);
        let entry = Entry {
            audio: None,
            trim: None,
            participant,
            language,
            text: String::new(),
        };
        lesson.entries.insert(entry_index, entry.clone());
        self.word_index.rebuild(&self.lessons);
        self.push_history(Command::InsertEntry {
            lesson: lesson_index,
            index: entry_index,
            entry,
        });
        self.active_lesson = lesson_index;
        self.select_only((lesson_index, entry_index));
        self.editing_entry = self.selected_entry;
        self.focus_entry = self.selected_entry;
    }

    /// The lesson that new entries are added to.
    pub fn target_lesson(&self) -> usize {
        self.selected_entry
            .map_or(self.active_lesson, |(lesson, _)| lesson)
    }

    pub fn new_lesson(&mut self) {
        self.lesson_playback = None;
        self.lesson_recording_summaries = None;
        let lesson = Lesson::empty();
        self.lessons.push(lesson.clone());
        self.push_history(Command::InsertLesson {
            index: self.lessons.len() - 1,
            lesson,
        });
        self.active_lesson = self.lessons.len() - 1;
        self.clear_selection();
        self.scroll_to_lesson = Some(self.active_lesson);
        self.focus_name = Some(self.active_lesson);
    }

    pub fn delete_entry(&mut self, lesson: usize, index: usize) {
        self.execute(Command::DeleteEntry {
            lesson,
            index,
            entry: self.lessons[lesson].entries[index].clone(),
        });
    }

    fn entries_snapshot(&self) -> Vec<Vec<Entry>> {
        self.lessons
            .iter()
            .map(|lesson| lesson.entries.clone())
            .collect()
    }

    /// Record a whole-script replacement and reveal the resulting selection.
    fn execute_bulk(
        &mut self,
        before: Vec<Vec<Entry>>,
        after: Vec<Vec<Entry>>,
        selected_before: Vec<Position>,
        selected_after: Vec<Position>,
        primary_after: Option<Position>,
    ) {
        self.execute(Command::BulkEntries {
            before,
            after,
            selected_before,
            selected_after,
            primary_before: self.selected_entry,
            primary_after,
        });
        self.reveal_selected();
    }

    /// Insert a copy after each target, selecting the copies.
    pub fn duplicate_entries(&mut self, targets: &[Position]) -> bool {
        if targets.is_empty() {
            return false;
        }
        let selected_before = self.selection();
        let chosen: BTreeSet<_> = targets.iter().copied().collect();
        let before = self.entries_snapshot();
        let mut after = Vec::with_capacity(before.len());
        let mut selected_after = Vec::new();
        let mut primary_after = None;
        for (lesson, entries) in before.iter().enumerate() {
            let mut values = Vec::new();
            for (index, entry) in entries.iter().enumerate() {
                values.push(entry.clone());
                if chosen.contains(&(lesson, index)) {
                    let position = (lesson, values.len());
                    values.push(entry.clone());
                    selected_after.push(position);
                    if self.selected_entry == Some((lesson, index)) {
                        primary_after = Some(position);
                    }
                }
            }
            after.push(values);
        }
        if selected_after.is_empty() {
            return false;
        }
        self.execute_bulk(
            before,
            after,
            selected_before,
            selected_after,
            primary_after,
        );
        true
    }

    /// Apply an action to every selected entry as a single undoable step.
    pub fn bulk_action(&mut self, action: BulkAction) -> bool {
        let selected_before = self.selection();
        if selected_before.len() < 2 {
            return false;
        }
        if matches!(action, BulkAction::Duplicate) {
            return self.duplicate_entries(&selected_before);
        }
        if matches!(action, BulkAction::MoveUp | BulkAction::MoveDown)
            && !self.selection_is_contiguous()
        {
            return false;
        }
        let before = self.entries_snapshot();
        // Tag each entry with its original position to track the selection.
        let mut tagged: Vec<Vec<(Position, Entry)>> = before
            .iter()
            .enumerate()
            .map(|(lesson, entries)| {
                entries
                    .iter()
                    .cloned()
                    .enumerate()
                    .map(|(entry, value)| ((lesson, entry), value))
                    .collect()
            })
            .collect();
        let chosen: BTreeSet<_> = selected_before.iter().copied().collect();
        let first = selected_before[0];
        let last = *selected_before.last().unwrap();
        match action {
            BulkAction::Duplicate => unreachable!(),
            BulkAction::Delete => {
                for lesson in &mut tagged {
                    lesson.retain(|(original, _)| !chosen.contains(original));
                }
            }
            BulkAction::ToTeacher
            | BulkAction::ToStudent
            | BulkAction::ToL1
            | BulkAction::ToLatin
            | BulkAction::ToggleLanguage => {
                for (original, entry) in tagged.iter_mut().flatten() {
                    if chosen.contains(original) {
                        action.convert(entry);
                    }
                }
            }
            BulkAction::MoveUp => {
                if first.1 == 0 {
                    // Move the selected part of the first lesson into the previous one.
                    if first.0 == 0 {
                        return false;
                    }
                    let count = selected_before
                        .iter()
                        .take_while(|(lesson, _)| *lesson == first.0)
                        .count();
                    let moved: Vec<_> = tagged[first.0].drain(..count).collect();
                    tagged[first.0 - 1].extend(moved);
                } else {
                    let mut slots = vec![(first.0, first.1 - 1)];
                    slots.extend(selected_before.iter().copied());
                    rotate_slots(&mut tagged, slots, true);
                }
            }
            BulkAction::MoveDown => {
                if last.1 + 1 == tagged[last.0].len() {
                    // Move the selected part of the last lesson into the next one.
                    if last.0 + 1 == tagged.len() {
                        return false;
                    }
                    let count = selected_before
                        .iter()
                        .rev()
                        .take_while(|(lesson, _)| *lesson == last.0)
                        .count();
                    let start = tagged[last.0].len() - count;
                    let moved: Vec<_> = tagged[last.0].drain(start..).collect();
                    tagged[last.0 + 1].splice(0..0, moved);
                } else {
                    let mut slots = selected_before.clone();
                    slots.push((last.0, last.1 + 1));
                    rotate_slots(&mut tagged, slots, false);
                }
            }
        }
        let mut selected_after = Vec::new();
        let mut primary_after = None;
        let mut after = Vec::new();
        for (lesson, entries) in tagged.into_iter().enumerate() {
            let mut values = Vec::new();
            for (index, (original, entry)) in entries.into_iter().enumerate() {
                if chosen.contains(&original) {
                    selected_after.push((lesson, index));
                    if self.selected_entry == Some(original) {
                        primary_after = Some((lesson, index));
                    }
                }
                values.push(entry);
            }
            after.push(values);
        }
        if before == after {
            return false;
        }
        self.execute_bulk(
            before,
            after,
            selected_before,
            selected_after,
            primary_after,
        );
        true
    }

    /// Apply a context-menu or shortcut action to one entry.
    pub fn entry_action(&mut self, lesson: usize, index: usize, action: EntryAction) -> bool {
        let entry = &self.lessons[lesson].entries[index];
        let command = match action {
            EntryAction::Duplicate => return self.duplicate_entries(&[(lesson, index)]),
            EntryAction::Delete => {
                self.delete_entry(lesson, index);
                return true;
            }
            EntryAction::AutoMacronize => return self.auto_macronize(lesson, index),
            EntryAction::MoveParticipant => {
                let after = match entry.participant {
                    Participant::Teacher => (Participant::Student, Language::Latin),
                    Participant::Student => (Participant::Teacher, Language::Latin),
                };
                Command::SetEntryKind {
                    lesson,
                    index,
                    before: (entry.participant, entry.language),
                    after,
                }
            }
            EntryAction::SwitchLanguage => {
                if entry.participant != Participant::Teacher {
                    return false;
                }
                Command::SetEntryKind {
                    lesson,
                    index,
                    before: (entry.participant, entry.language),
                    after: (entry.participant, entry.language.toggled()),
                }
            }
            EntryAction::MoveUp => {
                let to = if index > 0 {
                    (lesson, index - 1)
                } else if lesson > 0 {
                    (lesson - 1, self.lessons[lesson - 1].entries.len())
                } else {
                    return false;
                };
                Command::MoveEntry {
                    from: (lesson, index),
                    to,
                }
            }
            EntryAction::MoveDown => {
                let to = if index + 1 < self.lessons[lesson].entries.len() {
                    (lesson, index + 1)
                } else if lesson + 1 < self.lessons.len() {
                    (lesson + 1, 0)
                } else {
                    return false;
                };
                Command::MoveEntry {
                    from: (lesson, index),
                    to,
                }
            }
        };
        self.execute(command);
        self.reveal_selected();
        true
    }

    /// Macronize directly when unambiguous, otherwise open the review dialog.
    fn auto_macronize(&mut self, lesson: usize, index: usize) -> bool {
        if self.selection().len() > 1 {
            return false;
        }
        let entry = &self.lessons[lesson].entries[index];
        if entry.language != Language::Latin {
            return false;
        }
        let text = entry.text.clone();
        let words = self.word_index.tag_text(&collo::Lemmatizer::new(), &text);
        let review = parsing_review::Review::from_tagged_words(lesson, index, text, words);
        if let Some(after) = review.unambiguous_preview() {
            if review.original == after {
                return false;
            }
            self.apply_review(&review, after);
        } else {
            self.parsing_review = Some(review);
        }
        true
    }

    /// Run an entry shortcut on the selection; returns whether anything changed.
    pub fn selected_entry_shortcut(&mut self, action: keybinds::Action) -> bool {
        use keybinds::Action as Key;
        if self.selection().len() > 1 {
            let bulk = match action {
                Key::SwitchLanguage => BulkAction::ToggleLanguage,
                Key::MoveToStudent => BulkAction::ToStudent,
                Key::MoveToTeacher => BulkAction::ToTeacher,
                Key::MoveEntryUp => BulkAction::MoveUp,
                Key::MoveEntryDown => BulkAction::MoveDown,
                Key::DuplicateEntry => BulkAction::Duplicate,
                _ => return false,
            };
            return self.bulk_action(bulk);
        }
        let Some((lesson, index)) = self.selected_entry else {
            return false;
        };
        let teacher = self.lessons[lesson].entries[index].participant == Participant::Teacher;
        let action = match action {
            Key::DuplicateEntry => EntryAction::Duplicate,
            Key::SwitchLanguage if teacher => EntryAction::SwitchLanguage,
            Key::MoveToStudent if teacher => EntryAction::MoveParticipant,
            Key::MoveToTeacher if !teacher => EntryAction::MoveParticipant,
            Key::MoveEntryUp | Key::MoveEntryDown => {
                if self.positions().nth(1).is_none() {
                    return false;
                }
                if action == Key::MoveEntryUp {
                    EntryAction::MoveUp
                } else {
                    EntryAction::MoveDown
                }
            }
            _ => return false,
        };
        self.entry_action(lesson, index, action)
    }

    /// Apply a reviewed macronization unless the entry changed meanwhile.
    pub fn apply_review(&mut self, review: &parsing_review::Review, after: String) {
        if let Some(entry) = self
            .lessons
            .get_mut(review.lesson)
            .and_then(|lesson| lesson.entries.get_mut(review.entry))
            && entry.language == Language::Latin
            && entry.text == review.original
            && entry.text != after
        {
            let before = std::mem::replace(&mut entry.text, after.clone());
            self.record_text(
                TextTarget::Entry(review.lesson, review.entry),
                before,
                after,
            );
        }
    }
}
