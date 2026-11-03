//! The scrolling two-column script of lessons and entries.
//!
//! Drawing borrows the lessons mutably, so interactions are collected into
//! [`ScriptEvents`] and applied once the scroll area is done. Rows and lessons
//! outside the viewport are skipped using estimated heights.
use super::{
    FOOTER_HEIGHT, Foreground,
    card::{EntryMenuOptions, card},
    lesson::{HeaderOptions, HeaderResponse, lesson_footer, lesson_header},
};
use crate::{
    Language, Participant,
    app::{App, Command, MenuAction, Position, TextTarget, entry_column_geometry},
    audio,
    search::{self, Hit},
};
use eframe::egui::{self, Color32, Sense, Stroke, Vec2};
use std::sync::Arc;

/// Interactions from this frame's script, applied after drawing.
#[derive(Default)]
struct ScriptEvents {
    /// The lesson that should become active.
    selected_lesson: Option<usize>,
    selection: Option<(Position, egui::Modifiers, bool)>,
    edit: Option<Position>,
    audio: Option<Position>,
    entry_action: Option<(Position, MenuAction)>,
    playback: Option<usize>,
    history: Vec<Command>,
    changed_entries: Vec<(Position, String)>,
}

/// Values computed once per frame and shared by every lesson and card.
struct ScriptFrame {
    width: f32,
    viewport: egui::Rect,
    summaries: Vec<String>,
    lesson_heights: Vec<f32>,
    selection: Vec<Position>,
    /// Selection-wide menu options; per-entry fields are filled in per card.
    menu: EntryMenuOptions,
    /// Search matches, sorted in script order.
    hits: Arc<[Hit]>,
}

/// Vertical extent of each drawn lesson, used to find the one at the top.
type LessonBounds = Vec<(usize, f32, f32)>;

impl App {
    /// Draw the script and its scrollbar, returning the script viewport.
    pub(super) fn script(
        &mut self,
        ui: &mut egui::Ui,
        width: f32,
        foreground: &mut Foreground,
    ) -> egui::Rect {
        let height = (ui.available_height() - FOOTER_HEIGHT).max(40.0);
        self.ensure_entry_row_heights(ui, width);
        let viewport =
            egui::Rect::from_min_size(ui.next_widget_position(), Vec2::new(width, height));
        let center = viewport.center().x;
        ui.painter().line_segment(
            [
                egui::pos2(center, viewport.top()),
                egui::pos2(center, viewport.bottom()),
            ],
            Stroke::new(2.0, Color32::LIGHT_GRAY),
        );
        let mut scroll = egui::ScrollArea::vertical()
            .id_salt("script")
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
            .max_height(height)
            .min_scrolled_height(height)
            .auto_shrink([false, false]);
        if let Some(offset) = self.scrollbar_offset.take() {
            scroll = scroll.vertical_scroll_offset(offset);
        }
        let frame = ScriptFrame {
            width,
            viewport,
            summaries: self.recording_summaries().to_vec(),
            lesson_heights: (0..self.lessons.len())
                .map(|lesson| self.estimated_lesson_height(lesson))
                .collect(),
            selection: self.selection(),
            menu: self.selection_menu_options(),
            hits: self
                .search
                .hits(&self.lessons, &self.word_index, self.content_generation),
        };
        let mut events = ScriptEvents::default();
        let mut lesson_bounds = LessonBounds::new();
        let output = scroll.show(ui, |ui| {
            self.lessons_view(ui, &frame, &mut events, &mut lesson_bounds, foreground);
        });
        self.apply_script_events(ui.ctx(), &mut events);

        let offset = output.state.clamped_offset().y;
        let visible_lesson = lesson_bounds
            .iter()
            .find(|(_, top, bottom)| *top <= viewport.top() && *bottom > viewport.top())
            .map(|(index, _, _)| *index);
        if (offset - self.last_scroll_offset).abs() > 0.5
            && let Some(index) = visible_lesson
        {
            self.active_lesson = index;
        }
        self.last_scroll_offset = offset;
        if let Some(index) = events.selected_lesson {
            self.active_lesson = index;
        }

        if let Some(index) = visible_lesson
            && lesson_bounds[index].1 < viewport.top() - 1.0
        {
            self.sticky_header(ui, index, &frame, &mut events, foreground);
        }
        if let Some(index) = events.playback {
            self.toggle_lesson_playback(index);
            ui.ctx().request_repaint();
        }
        self.scrollbar(ui, viewport, output.content_size.y, offset, foreground);
        viewport
    }

    /// Menu options that depend only on the selection as a whole.
    fn selection_menu_options(&self) -> EntryMenuOptions {
        let selection = self.selection();
        let mut options = EntryMenuOptions {
            show_move: self.lessons.len() > 1
                || self.lessons.iter().any(|lesson| lesson.entries.len() > 1),
            multiselect: selection.len() > 1,
            contiguous: self.selection_is_contiguous(),
            can_group_up: selection
                .first()
                .is_some_and(|&(lesson, index)| index > 0 || lesson > 0),
            can_group_down: selection.last().is_some_and(|&(lesson, index)| {
                index + 1 < self.lessons[lesson].entries.len() || lesson + 1 < self.lessons.len()
            }),
            ..Default::default()
        };
        for &(lesson, index) in &selection {
            let entry = &self.lessons[lesson].entries[index];
            match (entry.participant, entry.language) {
                (Participant::Teacher, Language::L1) => {
                    options.has_teacher = true;
                    options.has_teacher_l1 = true;
                }
                (Participant::Teacher, Language::Latin) => {
                    options.has_teacher = true;
                    options.has_teacher_latin = true;
                }
                (Participant::Student, _) => options.has_student = true,
            }
        }
        options
    }

    /// Whether a lesson must be laid out even if it appears off-screen, so that
    /// pending focus and scroll requests reach their widgets.
    fn lesson_must_render(&self, lesson: usize) -> bool {
        let in_lesson = |position: Option<Position>| position.is_some_and(|(l, _)| l == lesson);
        self.focus_name == Some(lesson)
            || self.scroll_to_lesson == Some(lesson)
            || in_lesson(self.scroll_to_entry)
            || in_lesson(self.centre_entry.map(|(position, _)| position))
            || in_lesson(self.focus_entry)
            || in_lesson(self.editing_entry)
    }

    fn entry_must_render(&self, position: Position) -> bool {
        self.focus_entry == Some(position)
            || self.editing_entry == Some(position)
            || self.scroll_to_entry == Some(position)
            || self
                .centre_entry
                .is_some_and(|(target, _)| target == position)
    }

    fn lessons_view(
        &mut self,
        ui: &mut egui::Ui,
        frame: &ScriptFrame,
        events: &mut ScriptEvents,
        bounds: &mut LessonBounds,
        foreground: &mut Foreground,
    ) {
        let width = frame.width;
        ui.set_width(width);
        let lesson_count = self.lessons.len();
        for lesson in 0..lesson_count {
            let top = ui.next_widget_position().y;
            let estimate = frame.lesson_heights[lesson];
            let estimated_bounds = egui::Rect::from_min_size(
                egui::pos2(ui.min_rect().left(), top),
                Vec2::new(width, estimate),
            )
            .expand2(Vec2::new(0.0, 32.0));
            if !self.lesson_must_render(lesson) && !ui.is_rect_visible(estimated_bounds) {
                ui.allocate_space(Vec2::new(width, estimate));
                bounds.push((lesson, top, top + estimate));
            } else {
                ui.push_id(("lesson", lesson), |ui| {
                    let top = ui.next_widget_position().y;
                    self.lesson_view(ui, lesson, frame, events, foreground);
                    bounds.push((lesson, top, ui.next_widget_position().y));
                });
            }
            if lesson + 1 < lesson_count {
                ui.add_space(12.0);
            }
        }
        // These requests have now reached their widgets.
        self.focus_entry = None;
        self.scroll_to_entry = None;
        self.focus_name = None;
        // Stop centring after a while, e.g. if the entry is gone.
        self.centre_entry = self
            .centre_entry
            .and_then(|(position, frames)| Some((position, frames.checked_sub(1)?)));
        self.scroll_to_lesson = None;
    }

    /// Draw a lesson header, returning the name before any edit made in it.
    fn header_view(
        &mut self,
        ui: &mut egui::Ui,
        lesson: usize,
        frame: &ScriptFrame,
        sticky: bool,
    ) -> (HeaderResponse, Option<String>) {
        let options = HeaderOptions {
            width: frame.width,
            focus: !sticky && self.focus_name == Some(lesson),
            sticky,
            recording_summary: &frame.summaries[lesson],
            playback: self.playback_state(lesson),
            play_enabled: self.audio_editor.is_none() && !self.lessons[lesson].entries.is_empty(),
        };
        let before = self.lessons[lesson].name.clone();
        let header = lesson_header(ui, &mut self.lessons[lesson], lesson, options);
        let renamed = (self.lessons[lesson].name != before).then_some(before);
        (header, renamed)
    }

    fn lesson_view(
        &mut self,
        ui: &mut egui::Ui,
        lesson: usize,
        frame: &ScriptFrame,
        events: &mut ScriptEvents,
        foreground: &mut Foreground,
    ) {
        let width = frame.width;
        let (header, renamed) = self.header_view(ui, lesson, frame, false);
        foreground.push(header.response.rect.intersect(frame.viewport));
        if header.play_clicked {
            events.playback = Some(lesson);
        }
        if let Some(before) = renamed {
            events.history.push(Command::EditText {
                target: TextTarget::LessonName(lesson),
                before,
                after: self.lessons[lesson].name.clone(),
            });
        }
        if header.clicked {
            events.selected_lesson = Some(lesson);
            self.clear_selection();
        }
        if self.scroll_to_lesson == Some(lesson) {
            header.response.scroll_to_me(Some(egui::Align::BOTTOM));
        }
        if !self.lessons[lesson].expanded {
            return;
        }
        let body = egui::Frame::new()
            .stroke(Stroke::new(1.0, Color32::GRAY))
            .show(ui, |ui| {
                let inner_width = (width - 2.0).max(0.0);
                ui.set_width(inner_width);
                for index in 0..self.lessons[lesson].entries.len() {
                    let position = (lesson, index);
                    let row_height = self.entry_row_heights[lesson][index];
                    let row_bounds = egui::Rect::from_min_size(
                        egui::pos2(ui.min_rect().left(), ui.next_widget_position().y),
                        Vec2::new(inner_width, row_height),
                    )
                    .expand2(Vec2::new(0.0, 22.0));
                    if !self.entry_must_render(position) && !ui.is_rect_visible(row_bounds) {
                        ui.allocate_space(Vec2::new(inner_width, row_height));
                        continue;
                    }
                    #[cfg(test)]
                    {
                        self.rendered_entry_count += 1;
                    }
                    ui.push_id(index, |ui| {
                        self.entry_row(ui, position, frame, events, foreground);
                        ui.add_space(4.0);
                    });
                }
                if self.lessons[lesson].entries.is_empty() {
                    ui.add_space(20.0);
                }
            });
        if body.response.clicked() {
            events.selected_lesson = Some(lesson);
        }
        let footer = lesson_footer(ui, lesson, &self.lessons[lesson].name, width);
        foreground.push(footer.rect.intersect(frame.viewport));
        if footer.clicked() {
            self.lessons[lesson].expanded = false;
            if self.editing_entry.is_some_and(|(l, _)| l == lesson) {
                self.editing_entry = None;
            }
            events.selected_lesson = Some(lesson);
        }
    }

    /// One row: the entry's card in its participant's column, the other empty.
    fn entry_row(
        &mut self,
        ui: &mut egui::Ui,
        position: Position,
        frame: &ScriptFrame,
        events: &mut ScriptEvents,
        foreground: &mut Foreground,
    ) {
        let column_width = (frame.width - 2.0) / 2.0;
        let speaker = self.lessons[position.0].entries[position.1].participant;
        ui.horizontal_top(|ui| {
            for participant in [Participant::Teacher, Participant::Student] {
                ui.allocate_ui_with_layout(
                    Vec2::new(column_width, 0.0),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_min_width(column_width);
                        if speaker == participant {
                            ui.add_space(3.0);
                            ui.horizontal(|ui| {
                                self.entry_card(
                                    ui,
                                    position,
                                    column_width,
                                    frame,
                                    events,
                                    foreground,
                                );
                            });
                        }
                    },
                );
            }
        });
    }

    fn entry_card(
        &mut self,
        ui: &mut egui::Ui,
        position: Position,
        column_width: f32,
        frame: &ScriptFrame,
        events: &mut ScriptEvents,
        foreground: &mut Foreground,
    ) {
        let (lesson, index) = position;
        let entry_count = self.lessons[lesson].entries.len();
        let participant = self.lessons[lesson].entries[index].participant;
        let (card_margin, card_width) = entry_column_geometry(column_width, participant);
        ui.add_space(card_margin);
        let first_lemmas = self.word_index.first_use_lemmas(lesson, index);
        let focus = self.focus_entry == Some(position);
        let editing = self.editing_entry == Some(position);
        let selected =
            self.selected_entry == Some(position) || self.extra_selected.contains(&position);
        let options = EntryMenuOptions {
            can_move_up: index > 0 || lesson > 0,
            can_move_down: index + 1 < entry_count || lesson + 1 < self.lessons.len(),
            selected: frame.selection.contains(&position),
            ..frame.menu
        };
        let highlights: Vec<_> = search::entry_hits(&frame.hits, lesson, index)
            .iter()
            .map(|hit| hit.range.clone())
            .collect();
        let entry = &mut self.lessons[lesson].entries[index];
        let before = entry.text.clone();
        let response = card(
            ui,
            entry,
            &self.l1_name,
            card_width,
            focus,
            selected,
            editing,
            options,
            &first_lemmas,
            &highlights,
        );
        if entry.text != before {
            let after = entry.text.clone();
            events.changed_entries.push((position, after.clone()));
            events.history.push(Command::EditText {
                target: TextTarget::Entry(lesson, index),
                before,
                after,
            });
        }
        // Measured heights replace estimates for later frames.
        self.entry_row_heights[lesson][index] = response.bounds.height() + 7.0;
        foreground.push(response.bounds.expand(8.0).intersect(frame.viewport));
        if response.requests_selection(editing) && !focus {
            events.selection = Some((
                position,
                ui.input(|input| input.modifiers),
                response.right_clicked,
            ));
        }
        if editing && response.text.lost_focus() && !focus {
            self.editing_entry = None;
        }
        if let Some(action) = response.action {
            events.entry_action = Some((position, action));
        }
        if response.edit_requested {
            events.edit = Some(position);
        }
        if response.audio_requested {
            events.audio = Some(position);
        }
        #[cfg(test)]
        self.entry_text_rects.push((position, response.text.rect));
        if response.text_selecting {
            self.transcript_text_selecting = true;
        }
        if focus || self.scroll_to_entry == Some(position) {
            response.text.scroll_to_me(None);
        }
        if self
            .centre_entry
            .is_some_and(|(target, _)| target == position)
        {
            self.centre(ui, response.bounds, frame.viewport);
        }
    }

    /// Aim the scroll at the middle of the viewport for the entry in `bounds`,
    /// until it gets there or the user scrolls or clicks.
    fn centre(&mut self, ui: &egui::Ui, bounds: egui::Rect, viewport: egui::Rect) {
        let interrupted = ui
            .input(|input| input.smooth_scroll_delta != Vec2::ZERO || input.pointer.any_pressed());
        let offset = bounds.center().y - viewport.center().y;
        if interrupted || offset.abs() < 1.0 {
            self.centre_entry = None;
        } else {
            ui.scroll_to_rect(bounds, Some(egui::Align::Center));
            ui.ctx().request_repaint();
        }
    }

    fn apply_script_events(&mut self, ctx: &egui::Context, events: &mut ScriptEvents) {
        for command in events.history.drain(..) {
            self.push_history(command);
        }
        for ((lesson, entry), text) in events.changed_entries.drain(..) {
            if let Some(value) = self.lessons.get(lesson).and_then(|l| l.entries.get(entry)) {
                self.word_index.update_entry(
                    &collo::Lemmatizer::new(),
                    lesson,
                    entry,
                    &text,
                    value.language,
                );
            }
        }
        if let Some((position, modifiers, right_click)) = events.selection.take() {
            self.select_entry(position, modifiers, right_click);
            events.selected_lesson = Some(position.0);
        }
        if let Some(position) = events.edit.take() {
            self.selected_entry = Some(position);
            self.edit_selected();
            events.selected_lesson = Some(position.0);
        }
        if let Some((lesson, index)) = events.audio.take() {
            self.lesson_playback = None;
            self.audio_editor = Some(audio::Editor::new(
                (lesson, index),
                self.lessons[lesson].entries[index].clone(),
            ));
        }
        if let Some(((lesson, index), action)) = events.entry_action.take() {
            match action {
                MenuAction::Single(action) => {
                    self.entry_action(lesson, index, action);
                }
                MenuAction::Bulk(action) => {
                    self.bulk_action(action);
                }
            }
            events.selected_lesson = self
                .selected_entry
                .map(|(lesson, _)| lesson)
                .or(events.selected_lesson);
            super::surrender_focus(ctx);
        }
    }

    /// Repeat the header of the lesson crossing the top of the viewport.
    fn sticky_header(
        &mut self,
        ui: &mut egui::Ui,
        lesson: usize,
        frame: &ScriptFrame,
        events: &mut ScriptEvents,
        foreground: &mut Foreground,
    ) {
        let rect = egui::Rect::from_min_size(frame.viewport.min, Vec2::new(frame.width, 32.0));
        foreground.push(rect);
        let mut sticky_ui = ui.new_child(egui::UiBuilder::new().max_rect(rect));
        sticky_ui.push_id(("sticky", lesson), |ui| {
            let (header, renamed) = self.header_view(ui, lesson, frame, true);
            if header.play_clicked {
                events.playback = Some(lesson);
            }
            if let Some(before) = renamed {
                let after = self.lessons[lesson].name.clone();
                self.record_text(TextTarget::LessonName(lesson), before, after);
            }
            if header.clicked {
                self.active_lesson = lesson;
                self.clear_selection();
            }
        });
    }

    /// A single scrollbar in the centre gutter for the shared timeline.
    fn scrollbar(
        &mut self,
        ui: &mut egui::Ui,
        viewport: egui::Rect,
        content_height: f32,
        offset: f32,
        foreground: &mut Foreground,
    ) {
        let height = viewport.height();
        let track = egui::Rect::from_center_size(viewport.center(), Vec2::new(14.0, height));
        foreground.push(track);
        let content_height = content_height.max(height);
        let max_offset = (content_height - height).max(0.0);
        let handle_height = (height * height / content_height).clamp(24.0_f32.min(height), height);
        let travel = height - handle_height;
        let handle_top = track.top()
            + if max_offset > 0.0 {
                travel * offset / max_offset
            } else {
                0.0
            };
        let handle = egui::Rect::from_min_size(
            egui::pos2(track.left(), handle_top),
            Vec2::new(track.width(), handle_height),
        );
        ui.painter().rect_filled(handle, 0.0, Color32::GRAY);
        ui.painter().rect_stroke(
            handle,
            0.0,
            Stroke::new(1.0, Color32::WHITE),
            egui::StrokeKind::Inside,
        );
        let response = ui.interact(
            track,
            ui.scope_id().with("script_scrollbar"),
            Sense::click_and_drag(),
        );
        if max_offset <= 0.0 {
            return;
        }
        let target = if response.dragged() && travel > 0.0 {
            Some(offset + response.drag_delta().y * max_offset / travel)
        } else if response.clicked()
            && let Some(pos) = response.interact_pointer_pos()
        {
            Some((pos.y - track.top() - handle_height / 2.0) / travel * max_offset)
        } else {
            None
        };
        if let Some(target) = target {
            self.scrollbar_offset = Some(target.clamp(0.0, max_offset));
            ui.ctx().request_repaint();
        }
    }
}
