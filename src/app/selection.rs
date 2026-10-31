//! Entry selection: a primary entry, extra entries, and a range anchor.
use super::{App, Position};
use eframe::egui;
use std::collections::BTreeSet;

impl App {
    /// All selected entries in script order.
    pub fn selection(&self) -> Vec<Position> {
        let mut entries = self.extra_selected.clone();
        entries.extend(self.selected_entry);
        entries.into_iter().collect()
    }

    pub fn set_selection(&mut self, entries: Vec<Position>, primary: Option<Position>) {
        self.extra_selected = entries.into_iter().collect();
        self.selected_entry = primary
            .filter(|entry| self.extra_selected.contains(entry))
            .or_else(|| self.extra_selected.iter().next_back().copied());
        if let Some(entry) = self.selected_entry {
            self.extra_selected.remove(&entry);
        }
        self.selection_anchor = self.selected_entry;
    }

    /// Select exactly one entry, making it the anchor for range selection.
    pub fn select_only(&mut self, target: Position) {
        self.selected_entry = Some(target);
        self.extra_selected.clear();
        self.selection_anchor = Some(target);
    }

    /// Clear the selection and stop editing, e.g. when a lesson header is clicked.
    pub fn clear_selection(&mut self) {
        self.selected_entry = None;
        self.extra_selected.clear();
        self.selection_anchor = None;
        self.editing_entry = None;
    }

    pub fn deselect_entries(&mut self) {
        self.set_selection(Vec::new(), None);
        self.lesson_playback = None;
    }

    /// Update the selection for a click on `target` with the given modifiers.
    pub fn select_entry(
        &mut self,
        target: Position,
        modifiers: egui::Modifiers,
        right_click: bool,
    ) {
        let mut selected = self.selection().into_iter().collect::<BTreeSet<_>>();
        if right_click && selected.contains(&target) {
            self.editing_entry = None;
            self.active_lesson = target.0;
            return;
        }
        if right_click {
            selected.clear();
            selected.insert(target);
            self.selection_anchor = Some(target);
        } else if modifiers.alt {
            selected.remove(&target);
            if self.selection_anchor == Some(target) {
                self.selection_anchor = selected.iter().next_back().copied();
            }
        } else if modifiers.shift {
            let anchor = self
                .selection_anchor
                .or(self.selected_entry)
                .unwrap_or(target);
            let range = anchor.min(target)..=anchor.max(target);
            selected = self.positions().filter(|p| range.contains(p)).collect();
            self.selection_anchor = Some(anchor);
        } else if modifiers.command || modifiers.ctrl || modifiers.mac_cmd {
            selected.insert(target);
            self.selection_anchor = Some(target);
        } else {
            selected.clear();
            selected.insert(target);
            self.selection_anchor = Some(target);
        }
        self.selected_entry = if selected.contains(&target) {
            Some(target)
        } else {
            selected.iter().next_back().copied()
        };
        if let Some(primary) = self.selected_entry {
            selected.remove(&primary);
        }
        self.extra_selected = selected;
        self.editing_entry = None;
        self.active_lesson = target.0;
    }

    /// Every entry position in script order.
    pub fn positions(&self) -> impl Iterator<Item = Position> + '_ {
        self.lessons
            .iter()
            .enumerate()
            .flat_map(|(lesson, item)| (0..item.entries.len()).map(move |entry| (lesson, entry)))
    }

    pub fn selection_is_contiguous(&self) -> bool {
        let selection = self.selection();
        let (Some(first), Some(last)) = (selection.first(), selection.last()) else {
            return false;
        };
        let linear_position = |(lesson, entry): Position| {
            self.lessons
                .iter()
                .take(lesson)
                .map(|item| item.entries.len())
                .sum::<usize>()
                + entry
        };
        linear_position(*last) - linear_position(*first) + 1 == selection.len()
    }

    /// Move every selected position through `map`, dropping those it rejects.
    pub fn remap_selection(&mut self, map: impl Fn(Position) -> Option<Position>) {
        self.extra_selected = self
            .extra_selected
            .iter()
            .copied()
            .filter_map(&map)
            .collect();
        self.selection_anchor = self.selection_anchor.and_then(&map);
        self.selected_entry = self.selected_entry.and_then(&map);
    }

    /// Select the next or previous entry in script order.
    pub fn select_adjacent(&mut self, next: bool) {
        let entries: Vec<_> = self.positions().collect();
        let (Some(&first), Some(&last)) = (entries.first(), entries.last()) else {
            return;
        };
        let target = if let Some(selected) = self.selected_entry {
            let index = entries
                .iter()
                .position(|entry| *entry == selected)
                .unwrap_or(0);
            entries[if next {
                (index + 1).min(entries.len() - 1)
            } else {
                index.saturating_sub(1)
            }]
        } else {
            let mut matching = entries
                .iter()
                .copied()
                .filter(|(lesson, _)| *lesson == self.active_lesson);
            if next {
                matching.next().unwrap_or(first)
            } else {
                matching.next_back().unwrap_or(last)
            }
        };
        self.select_only(target);
        self.editing_entry = None;
        self.active_lesson = target.0;
        self.lessons[target.0].expanded = true;
        self.focus_entry = None;
        self.scroll_to_entry = Some(target);
    }

    /// Select one entry and scroll the script to put it in the middle.
    pub fn reveal_entry(&mut self, target: Position) {
        self.select_only(target);
        self.editing_entry = None;
        self.focus_entry = None;
        self.active_lesson = target.0;
        self.lessons[target.0].expanded = true;
        self.scroll_to_entry = None;
        self.centre_entry = Some((target, 120));
    }

    /// Start editing the primary selection.
    pub fn edit_selected(&mut self) {
        if let Some(target) = self.selected_entry {
            self.select_only(target);
            self.active_lesson = target.0;
            self.lessons[target.0].expanded = true;
            self.editing_entry = Some(target);
            self.focus_entry = Some(target);
        }
    }
}
