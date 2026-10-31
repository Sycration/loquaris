//! Undoable commands and the history that records them.
use super::{App, Position};
use crate::{Entry, Language, Lesson, Participant, export_pdf, project};
use cosmic_undo_2::Action;

#[derive(Clone)]
pub enum TextTarget {
    ProjectName,
    L1Name,
    LessonName(usize),
    Entry(usize, usize),
}

#[derive(Clone)]
pub enum Command {
    SetAudioGaps {
        before: project::AudioGaps,
        after: project::AudioGaps,
    },
    SetExportSettings {
        before: export_pdf::Settings,
        after: export_pdf::Settings,
    },
    SetAudio {
        lesson: usize,
        index: usize,
        before: Option<std::sync::Arc<[u8]>>,
        after: Option<std::sync::Arc<[u8]>>,
    },
    EditText {
        target: TextTarget,
        before: String,
        after: String,
    },
    InsertLesson {
        index: usize,
        lesson: Lesson,
    },
    InsertEntry {
        lesson: usize,
        index: usize,
        entry: Entry,
    },
    DeleteEntry {
        lesson: usize,
        index: usize,
        entry: Entry,
    },
    SetEntryKind {
        lesson: usize,
        index: usize,
        before: (Participant, Language),
        after: (Participant, Language),
    },
    MoveEntry {
        from: Position,
        to: Position,
    },
    BulkEntries {
        before: Vec<Vec<Entry>>,
        after: Vec<Vec<Entry>>,
        selected_before: Vec<Position>,
        selected_after: Vec<Position>,
        primary_before: Option<Position>,
        primary_after: Option<Position>,
    },
}

impl Command {
    /// Adds, removes, or reorders entries or lessons.
    fn changes_structure(&self) -> bool {
        matches!(
            self,
            Command::InsertLesson { .. }
                | Command::InsertEntry { .. }
                | Command::DeleteEntry { .. }
                | Command::MoveEntry { .. }
                | Command::BulkEntries { .. }
        )
    }

    /// Invalidates lesson playback and recording durations.
    fn affects_recordings(&self) -> bool {
        self.changes_structure()
            || matches!(
                self,
                Command::SetAudio { .. }
                    | Command::SetAudioGaps { .. }
                    | Command::SetEntryKind { .. }
            )
    }

    /// Invalidates the word index as a whole rather than one entry.
    fn requires_index_rebuild(&self) -> bool {
        self.changes_structure() || matches!(self, Command::SetEntryKind { .. })
    }
}

/// Where `index` ends up after inserting (or removing) the item at `at`.
/// Returns `None` for the removed item itself.
fn shift_index(index: usize, at: usize, insert: bool) -> Option<usize> {
    if insert {
        Some(if index >= at { index + 1 } else { index })
    } else if index == at {
        None
    } else {
        Some(if index > at { index - 1 } else { index })
    }
}

/// Where `position` ends up after moving the entry at `from` to `to`.
fn moved_position(position: Position, from: Position, to: Position) -> Position {
    if position == from {
        return to;
    }
    let (lesson, mut index) = position;
    if lesson == from.0 && index > from.1 {
        index -= 1;
    }
    if lesson == to.0 && index >= to.1 {
        index += 1;
    }
    (lesson, index)
}

impl App {
    pub fn push_history(&mut self, command: Command) {
        self.history_tokens.truncate(self.history_position);
        self.history.push(command);
        self.history_tokens.push(self.next_history_token);
        self.next_history_token = self.next_history_token.wrapping_add(1).max(1);
        self.history_position += 1;
        self.content_generation += 1;
    }

    /// Apply a new command and record it.
    pub fn execute(&mut self, command: Command) {
        self.apply_command(command.clone(), true);
        self.push_history(command);
    }

    pub fn current_history_token(&self) -> Option<u64> {
        self.history_position
            .checked_sub(1)
            .and_then(|index| self.history_tokens.get(index).copied())
    }

    /// Record a text edit that the UI has already applied.
    pub fn record_text(&mut self, target: TextTarget, before: String, after: String) {
        if before == after {
            return;
        }
        if let TextTarget::Entry(lesson, entry) = &target
            && let Some(value) = self
                .lessons
                .get(*lesson)
                .and_then(|l| l.entries.get(*entry))
        {
            self.layout_cache_width = f32::NAN;
            self.word_index.update_entry(
                &collo::Lemmatizer::new(),
                *lesson,
                *entry,
                &value.text,
                value.language,
            );
        }
        self.push_history(Command::EditText {
            target,
            before,
            after,
        });
    }

    pub fn undo(&mut self) {
        self.step_history(false);
    }

    pub fn redo(&mut self) {
        self.step_history(true);
    }

    fn step_history(&mut self, redo: bool) {
        if self
            .audio_editor
            .as_mut()
            .is_some_and(|editor| editor.block_history_while_recording())
        {
            return;
        }
        let actions = if redo {
            self.history.redo()
        } else {
            self.history.undo()
        };
        let actions: Vec<_> = actions
            .map(|action| match action {
                Action::Do(command) => (command.clone(), true),
                Action::Undo(command) => (command.clone(), false),
            })
            .collect();
        for (command, forward) in actions {
            self.apply_command(command, forward);
        }
        self.history_position = self.history.iter_realized().count();
    }

    /// Apply `command`, or revert it when `forward` is false.
    pub fn apply_command(&mut self, command: Command, forward: bool) {
        self.content_generation += 1;
        if let Command::DeleteEntry {
            lesson,
            index,
            entry,
        } = command
        {
            let insert = Command::InsertEntry {
                lesson,
                index,
                entry,
            };
            return self.apply_command(insert, !forward);
        }
        if command.affects_recordings() {
            self.lesson_playback = None;
            self.lesson_recording_summaries = None;
        }
        if command.changes_structure() {
            self.audio_editor = None;
        }
        let rebuild_word_index = command.requires_index_rebuild();
        let update_word_index = match &command {
            Command::EditText {
                target: TextTarget::Entry(lesson, entry),
                before,
                after,
            } if before != after => Some((
                *lesson,
                *entry,
                if forward { after } else { before }.clone(),
            )),
            _ => None,
        };
        if !matches!(&command, Command::EditText { .. }) {
            self.editing_entry = None;
        }
        match command {
            Command::SetAudioGaps { before, after } => {
                self.audio_gaps = if forward { after } else { before };
            }
            Command::SetExportSettings { before, after } => {
                self.export_settings = if forward { after } else { before };
            }
            Command::SetAudio {
                lesson,
                index,
                before,
                after,
            } => {
                self.lessons[lesson].entries[index].audio = if forward { after } else { before };
            }
            Command::EditText {
                target,
                before,
                after,
            } => {
                let value = if forward { after } else { before };
                match target {
                    TextTarget::ProjectName => self.project_name = value,
                    TextTarget::L1Name => self.l1_name = value,
                    TextTarget::LessonName(index) => self.lessons[index].name = value,
                    TextTarget::Entry(lesson, entry) => {
                        self.lessons[lesson].entries[entry].text = value
                    }
                }
            }
            Command::InsertLesson { index, lesson } => {
                self.remap_selection(|(lesson, entry)| {
                    Some((shift_index(lesson, index, forward)?, entry))
                });
                if forward {
                    self.lessons.insert(index, lesson);
                } else {
                    self.lessons.remove(index);
                }
                if forward && self.active_lesson >= index {
                    self.active_lesson += 1;
                }
                if !forward && self.active_lesson > index {
                    self.active_lesson -= 1;
                }
            }
            Command::InsertEntry {
                lesson,
                index,
                entry,
            } => {
                self.remap_selection(|(selected_lesson, selected_index)| {
                    if selected_lesson != lesson {
                        return Some((selected_lesson, selected_index));
                    }
                    Some((lesson, shift_index(selected_index, index, forward)?))
                });
                if forward {
                    self.lessons[lesson].entries.insert(index, entry);
                } else {
                    self.lessons[lesson].entries.remove(index);
                }
            }
            Command::DeleteEntry { .. } => unreachable!("handled as a reverted insertion"),
            Command::SetEntryKind {
                lesson,
                index,
                before,
                after,
            } => {
                let (participant, language) = if forward { after } else { before };
                let entry = &mut self.lessons[lesson].entries[index];
                entry.participant = participant;
                entry.language = language;
            }
            Command::MoveEntry { from, to } => {
                let (from, to) = if forward { (from, to) } else { (to, from) };
                self.remap_selection(|position| Some(moved_position(position, from, to)));
                let entry = self.lessons[from.0].entries.remove(from.1);
                self.lessons[to.0].entries.insert(to.1, entry);
            }
            Command::BulkEntries {
                before,
                after,
                selected_before,
                selected_after,
                primary_before,
                primary_after,
            } => {
                let (entries, selection, primary) = if forward {
                    (after, selected_after, primary_after)
                } else {
                    (before, selected_before, primary_before)
                };
                for (lesson, entries) in self.lessons.iter_mut().zip(entries) {
                    lesson.entries = entries;
                }
                self.set_selection(selection, primary);
            }
        }
        if rebuild_word_index {
            self.word_index.rebuild(&self.lessons);
            self.layout_cache_width = f32::NAN;
        } else if let Some((lesson, entry, text)) = update_word_index
            && let Some(value) = self.lessons.get(lesson).and_then(|l| l.entries.get(entry))
        {
            self.layout_cache_width = f32::NAN;
            self.word_index.update_entry(
                &collo::Lemmatizer::new(),
                lesson,
                entry,
                &text,
                value.language,
            );
        }
        if let Some(editor) = &mut self.audio_editor
            && let Some(entry) = self
                .lessons
                .get(editor.target.0)
                .and_then(|lesson| lesson.entries.get(editor.target.1))
        {
            editor.sync_entry(entry);
        }
        if self.selected_entry.is_none() {
            self.selected_entry = self.extra_selected.pop_last();
        }
        if self.selection_anchor.is_none() {
            self.selection_anchor = self.selected_entry;
        }
        self.active_lesson = self.active_lesson.min(self.lessons.len().saturating_sub(1));
        self.focus_entry = None;
        self.scroll_to_entry = None;
        self.focus_name = None;
        self.scroll_to_lesson = None;
    }
}
