#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use cosmic_undo_2::{Action, Commands};
use eframe::egui::{self, Color32, RichText, Sense, Stroke, Vec2};

const L1_COLOR: Color32 = Color32::from_rgb(24, 68, 80);
const LATIN_COLOR: Color32 = Color32::from_rgb(80, 0, 0);
const L1_ACCENT: Color32 = Color32::from_rgb(60, 184, 213);
const LATIN_ACCENT: Color32 = Color32::from_rgb(224, 70, 70);

fn main() -> eframe::Result {
    eframe::run_native(
        "loquaris",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([800.0, 750.0])
                .with_min_inner_size([420.0, 350.0]),
            ..Default::default()
        },
        Box::new(|_cc| Ok(Box::new(App::default()))),
    )
}

#[derive(Clone, Copy, PartialEq)]
enum Participant {
    Teacher,
    Student,
}

#[derive(Clone, Copy)]
enum Language {
    L1,
    Latin,
}

#[derive(Clone)]
struct Entry {
    participant: Participant,
    language: Language,
    text: String,
}

#[derive(Clone)]
struct Lesson {
    name: String,
    entries: Vec<Entry>,
    expanded: bool,
}

#[derive(Clone)]
enum TextTarget {
    L1Name,
    LessonName(usize),
    Entry(usize, usize),
}

#[derive(Clone)]
enum Command {
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
}

struct App {
    l1_name: String,
    lessons: Vec<Lesson>,
    active_lesson: usize,
    scroll_to_lesson: Option<usize>,
    scrollbar_offset: Option<f32>,
    focus_entry: Option<(usize, usize)>,
    selected_entry: Option<(usize, usize)>,
    focus_name: Option<usize>,
    last_scroll_offset: f32,
    history: Commands<Command>,
}

impl Default for App {
    fn default() -> Self {
        let entries = [
            (
                Participant::Teacher,
                Language::L1,
                "Now we're going to talk about body parts. The Latin word for hand is",
            ),
            (Participant::Teacher, Language::Latin, "manus"),
            (
                Participant::Teacher,
                Language::L1,
                "Can you say \"hand\" in Latin to me?",
            ),
            (Participant::Student, Language::Latin, "manus"),
            (
                Participant::Teacher,
                Language::L1,
                "Perfect. Now what about both of them?",
            ),
            (Participant::Student, Language::Latin, "duae manūs"),
        ]
        .into_iter()
        .map(|(participant, language, text)| Entry {
            participant,
            language,
            text: text.to_owned(),
        })
        .collect();
        Self {
            l1_name: "English".into(),
            lessons: vec![Lesson {
                name: "Body parts".into(),
                entries,
                expanded: true,
            }],
            active_lesson: 0,
            scroll_to_lesson: None,
            scrollbar_offset: None,
            focus_entry: None,
            selected_entry: None,
            focus_name: None,
            last_scroll_offset: 0.0,
            history: Commands::new(),
        }
    }
}

impl App {
    fn add(&mut self, participant: Participant, language: Language) {
        let lesson_index = self
            .selected_entry
            .map_or(self.active_lesson, |(lesson, _)| lesson);
        let lesson = &mut self.lessons[lesson_index];
        lesson.expanded = true;
        let entry_index = self
            .selected_entry
            .map_or(lesson.entries.len(), |(_, entry)| entry + 1);
        let entry = Entry {
            participant,
            language,
            text: String::new(),
        };
        lesson.entries.insert(entry_index, entry.clone());
        self.history.push(Command::InsertEntry {
            lesson: lesson_index,
            index: entry_index,
            entry,
        });
        self.active_lesson = lesson_index;
        self.selected_entry = Some((lesson_index, entry_index));
        self.focus_entry = self.selected_entry;
    }

    fn new_lesson(&mut self) {
        let lesson = Lesson {
            name: String::new(),
            entries: Vec::new(),
            expanded: true,
        };
        self.lessons.push(lesson.clone());
        self.history.push(Command::InsertLesson {
            index: self.lessons.len() - 1,
            lesson,
        });
        self.active_lesson = self.lessons.len() - 1;
        self.selected_entry = None;
        self.scroll_to_lesson = Some(self.active_lesson);
        self.focus_name = Some(self.active_lesson);
    }

    fn delete_entry(&mut self, lesson: usize, index: usize) {
        let command = Command::DeleteEntry {
            lesson,
            index,
            entry: self.lessons[lesson].entries[index].clone(),
        };
        self.apply_command(command.clone(), true);
        self.history.push(command);
    }

    fn record_text(&mut self, target: TextTarget, before: String, after: String) {
        if before != after {
            self.history.push(Command::EditText {
                target,
                before,
                after,
            });
        }
    }

    fn undo(&mut self) {
        let actions: Vec<_> = self
            .history
            .undo()
            .map(|action| match action {
                Action::Do(command) => (command.clone(), true),
                Action::Undo(command) => (command.clone(), false),
            })
            .collect();
        for (command, forward) in actions {
            self.apply_command(command, forward);
        }
    }

    fn redo(&mut self) {
        let actions: Vec<_> = self
            .history
            .redo()
            .map(|action| match action {
                Action::Do(command) => (command.clone(), true),
                Action::Undo(command) => (command.clone(), false),
            })
            .collect();
        for (command, forward) in actions {
            self.apply_command(command, forward);
        }
    }

    fn apply_command(&mut self, command: Command, forward: bool) {
        match command {
            Command::EditText {
                target,
                before,
                after,
            } => {
                let value = if forward { after } else { before };
                match target {
                    TextTarget::L1Name => self.l1_name = value,
                    TextTarget::LessonName(index) => self.lessons[index].name = value,
                    TextTarget::Entry(lesson, entry) => {
                        self.lessons[lesson].entries[entry].text = value
                    }
                }
            }
            Command::InsertLesson { index, lesson } => {
                if forward {
                    self.lessons.insert(index, lesson);
                } else {
                    self.lessons.remove(index);
                }
                if let Some((lesson, entry)) = self.selected_entry {
                    self.selected_entry = if !forward && lesson == index {
                        None
                    } else {
                        Some((
                            if forward && lesson >= index {
                                lesson + 1
                            } else if !forward && lesson > index {
                                lesson - 1
                            } else {
                                lesson
                            },
                            entry,
                        ))
                    };
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
                if forward {
                    self.lessons[lesson].entries.insert(index, entry);
                } else {
                    self.lessons[lesson].entries.remove(index);
                }
                if let Some((selected_lesson, selected_index)) = self.selected_entry
                    && selected_lesson == lesson
                {
                    self.selected_entry = if !forward && selected_index == index {
                        None
                    } else {
                        Some((
                            lesson,
                            if forward && selected_index >= index {
                                selected_index + 1
                            } else if !forward && selected_index > index {
                                selected_index - 1
                            } else {
                                selected_index
                            },
                        ))
                    };
                }
            }
            Command::DeleteEntry {
                lesson,
                index,
                entry,
            } => {
                self.apply_command(
                    Command::InsertEntry {
                        lesson,
                        index,
                        entry,
                    },
                    !forward,
                );
                return;
            }
        }
        self.active_lesson = self.active_lesson.min(self.lessons.len().saturating_sub(1));
        self.focus_entry = None;
        self.focus_name = None;
        self.scroll_to_lesson = None;
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let (undo, redo) = ui.input_mut(|input| {
            let redo = input.consume_key(
                egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
                egui::Key::Z,
            ) || input.consume_key(egui::Modifiers::COMMAND, egui::Key::Y);
            let undo = input.consume_key(egui::Modifiers::COMMAND, egui::Key::Z);
            (undo, redo)
        });
        if undo {
            self.undo();
        }
        if redo {
            self.redo();
        }
        ui.visuals_mut().override_text_color = Some(Color32::WHITE);
        ui.visuals_mut().panel_fill = Color32::BLACK;
        ui.spacing_mut().item_spacing = Vec2::new(0.0, 0.0);
        let width = ui.available_width();
        let half = width / 2.0;

        ui.allocate_ui_with_layout(
            Vec2::new(width, 26.0),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                let before = self.l1_name.clone();
                let response = ui.add(
                    egui::TextEdit::singleline(&mut self.l1_name)
                        .prefix("L1 language: ")
                        .desired_width(200.0)
                        .frame(egui::Frame::NONE),
                );
                if response.changed() {
                    self.record_text(TextTarget::L1Name, before, self.l1_name.clone());
                }
                ui.add_space(20.0);
                if ui.button("+ New lesson").clicked() {
                    self.new_lesson();
                }
                if ui.button("Undo").clicked() {
                    self.undo();
                }
                if ui.button("Redo").clicked() {
                    self.redo();
                }
            },
        );
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            ui.allocate_ui_with_layout(
                Vec2::new(half, 32.0),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.label(RichText::new("Teacher").size(25.0).strong());
                },
            );
            ui.allocate_ui_with_layout(
                Vec2::new(half, 32.0),
                egui::Layout::right_to_left(egui::Align::Center),
                |ui| {
                    ui.label(RichText::new("Student").size(25.0).strong());
                },
            );
        });
        ui.separator();
        ui.add_space(4.0);

        let footer_height = 75.0;
        let bottom = ui.available_rect_before_wrap().bottom();
        let height = (ui.available_height() - footer_height).max(40.0);
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
        let mut lesson_bounds = Vec::new();
        let mut selected = None;
        let mut delete_entry = None;
        let output = scroll.show(ui, |ui| {
            ui.set_width(width);
            for (lesson_index, lesson) in self.lessons.iter_mut().enumerate() {
                ui.push_id(("lesson", lesson_index), |ui| {
                    let top = ui.next_widget_position().y;
                    let before_name = lesson.name.clone();
                    let header = lesson_header(
                        ui,
                        lesson,
                        lesson_index,
                        width,
                        self.focus_name == Some(lesson_index),
                        false,
                    );
                    if lesson.name != before_name {
                        self.history.push(Command::EditText {
                            target: TextTarget::LessonName(lesson_index),
                            before: before_name,
                            after: lesson.name.clone(),
                        });
                    }
                    if header.clicked {
                        selected = Some(lesson_index);
                        self.selected_entry = None;
                    }
                    if self.scroll_to_lesson == Some(lesson_index) {
                        header.response.scroll_to_me(Some(egui::Align::BOTTOM));
                    }
                    if lesson.expanded {
                        let body = egui::Frame::new()
                            .stroke(Stroke::new(1.0, Color32::GRAY))
                            .show(ui, |ui| {
                                ui.set_width((width - 2.0).max(0.0));
                                let column_width = (width - 2.0) / 2.0;
                                for (entry_index, entry) in lesson.entries.iter_mut().enumerate() {
                                    ui.push_id(entry_index, |ui| {
                                        ui.horizontal_top(|ui| {
                                            for participant in
                                                [Participant::Teacher, Participant::Student]
                                            {
                                                ui.allocate_ui_with_layout(
                                                    Vec2::new(column_width, 0.0),
                                                    egui::Layout::top_down(egui::Align::Min),
                                                    |ui| {
                                                        ui.set_min_width(column_width);
                                                        if entry.participant == participant {
                                                            ui.add_space(3.0);
                                                            ui.horizontal(|ui| {
                                                                ui.add_space(10.0);
                                                                let focus = self.focus_entry
                                                                    == Some((
                                                                        lesson_index,
                                                                        entry_index,
                                                                    ));
                                                                let before = entry.text.clone();
                                                                let response = card(
                                                                    ui,
                                                                    entry,
                                                                    &self.l1_name,
                                                                    (column_width - 20.0).max(20.0),
                                                                    focus,
                                                                    self.selected_entry
                                                                        == Some((
                                                                            lesson_index,
                                                                            entry_index,
                                                                        )),
                                                                );
                                                                if entry.text != before {
                                                                    self.history.push(
                                                                        Command::EditText {
                                                                            target:
                                                                                TextTarget::Entry(
                                                                                    lesson_index,
                                                                                    entry_index,
                                                                                ),
                                                                            before,
                                                                            after: entry
                                                                                .text
                                                                                .clone(),
                                                                        },
                                                                    );
                                                                }
                                                                if response.clicked || focus {
                                                                    selected = Some(lesson_index);
                                                                    self.selected_entry = Some((
                                                                        lesson_index,
                                                                        entry_index,
                                                                    ));
                                                                }
                                                                if response.delete {
                                                                    delete_entry = Some((
                                                                        lesson_index,
                                                                        entry_index,
                                                                    ));
                                                                }
                                                                if focus {
                                                                    response.text.scroll_to_me(
                                                                        Some(egui::Align::BOTTOM),
                                                                    );
                                                                }
                                                            });
                                                        }
                                                    },
                                                );
                                            }
                                        });
                                        ui.add_space(4.0);
                                    });
                                }
                                if lesson.entries.is_empty() {
                                    ui.add_space(20.0);
                                }
                            });
                        if body.response.clicked() {
                            selected = Some(lesson_index);
                        }
                        if lesson_footer(ui, lesson_index, &lesson.name, width).clicked() {
                            lesson.expanded = false;
                            selected = Some(lesson_index);
                        }
                    }
                    lesson_bounds.push((lesson_index, top, ui.next_widget_position().y));
                });
            }
            self.focus_entry = None;
            self.focus_name = None;
            self.scroll_to_lesson = None;
        });

        if let Some((lesson, index)) = delete_entry {
            self.delete_entry(lesson, index);
            ui.ctx().memory_mut(|memory| {
                if let Some(id) = memory.focused() {
                    memory.surrender_focus(id);
                }
            });
        }

        let offset_now = output.state.clamped_offset().y;
        let visible_lesson = lesson_bounds
            .iter()
            .find(|(_, top, bottom)| *top <= viewport.top() && *bottom > viewport.top())
            .map(|(index, _, _)| *index);
        if (offset_now - self.last_scroll_offset).abs() > 0.5
            && let Some(index) = visible_lesson
        {
            self.active_lesson = index;
        }
        self.last_scroll_offset = offset_now;
        if let Some(index) = selected {
            self.active_lesson = index;
        }

        // Repeat the header of the lesson crossing the top of the viewport.
        if let Some(index) = visible_lesson {
            let (_, top, _) = lesson_bounds[index];
            if top < viewport.top() - 1.0 {
                let rect = egui::Rect::from_min_size(viewport.min, Vec2::new(width, 32.0));
                let mut sticky_ui = ui.new_child(egui::UiBuilder::new().max_rect(rect));
                {
                    let ui = &mut sticky_ui;
                    ui.push_id(("sticky", index), |ui| {
                        let before = self.lessons[index].name.clone();
                        let header =
                            lesson_header(ui, &mut self.lessons[index], index, width, false, true);
                        if self.lessons[index].name != before {
                            self.record_text(
                                TextTarget::LessonName(index),
                                before,
                                self.lessons[index].name.clone(),
                            );
                        }
                        if header.clicked {
                            self.active_lesson = index;
                            self.selected_entry = None;
                        }
                    });
                }
            }
        }

        // A single scrollbar in the gutter controls the shared chronological timeline.
        let track = egui::Rect::from_center_size(
            egui::pos2(center, viewport.center().y),
            Vec2::new(14.0, height),
        );
        let content_height = output.content_size.y.max(height);
        let max_offset = (content_height - height).max(0.0);
        let handle_height = (height * height / content_height).clamp(24.0_f32.min(height), height);
        let travel = height - handle_height;
        let offset = output.state.clamped_offset().y;
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
        if max_offset > 0.0 {
            if response.dragged() && travel > 0.0 {
                self.scrollbar_offset = Some(
                    (offset + response.drag_delta().y * max_offset / travel).clamp(0.0, max_offset),
                );
                ui.ctx().request_repaint();
            } else if response.clicked()
                && let Some(pos) = response.interact_pointer_pos()
            {
                self.scrollbar_offset = Some(
                    ((pos.y - track.top() - handle_height / 2.0) / travel * max_offset)
                        .clamp(0.0, max_offset),
                );
                ui.ctx().request_repaint();
            }
        }

        let footer_rect = egui::Rect::from_min_max(
            egui::pos2(viewport.left(), bottom - footer_height),
            egui::pos2(viewport.right(), bottom),
        );
        let mut footer_ui = ui.new_child(egui::UiBuilder::new().max_rect(footer_rect));
        let ui = &mut footer_ui;
        ui.add_space(4.0);
        ui.separator();
        let target_lesson = self
            .selected_entry
            .map_or(self.active_lesson, |(lesson, _)| lesson);
        let active = &self.lessons[target_lesson];
        ui.label(
            RichText::new(format!(
                "Adding to lesson {}: {}",
                target_lesson + 1,
                if active.name.is_empty() {
                    "Untitled"
                } else {
                    &active.name
                }
            ))
            .small()
            .color(Color32::LIGHT_GRAY),
        );
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.allocate_ui_with_layout(
                Vec2::new(half, 35.0),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.add_space(10.0);
                    if ui
                        .add(
                            egui::Button::new(RichText::new("+ L1").color(L1_ACCENT).strong())
                                .fill(L1_COLOR),
                        )
                        .clicked()
                    {
                        self.add(Participant::Teacher, Language::L1);
                    }
                    ui.add_space(12.0);
                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new("+ Latin").color(LATIN_ACCENT).strong(),
                            )
                            .fill(LATIN_COLOR),
                        )
                        .clicked()
                    {
                        self.add(Participant::Teacher, Language::Latin);
                    }
                },
            );
            ui.allocate_ui_with_layout(
                Vec2::new(half, 35.0),
                egui::Layout::right_to_left(egui::Align::Center),
                |ui| {
                    ui.add_space(10.0);
                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new("+ Latin").color(LATIN_ACCENT).strong(),
                            )
                            .fill(LATIN_COLOR),
                        )
                        .clicked()
                    {
                        self.add(Participant::Student, Language::Latin);
                    }
                },
            );
        });
    }
}

struct HeaderResponse {
    response: egui::Response,
    clicked: bool,
}

fn lesson_header(
    ui: &mut egui::Ui,
    lesson: &mut Lesson,
    index: usize,
    width: f32,
    focus: bool,
    sticky: bool,
) -> HeaderResponse {
    let mut clicked = false;
    let response = egui::Frame::new()
        .fill(Color32::from_gray(65))
        .stroke(Stroke::new(1.0, Color32::LIGHT_GRAY))
        .inner_margin(3.0)
        .show(ui, |ui| {
            ui.set_width((width - 8.0).max(0.0));
            ui.allocate_ui_with_layout(
                Vec2::new((width - 8.0).max(0.0), 24.0),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    if ui.button(if lesson.expanded { "▼" } else { "▶" }).clicked() {
                        lesson.expanded = !lesson.expanded;
                        clicked = true;
                    }
                    ui.add_space(8.0);
                    let name = ui.add(
                        egui::TextEdit::singleline(&mut lesson.name)
                            .prefix(RichText::new(format!("Lesson {}: ", index + 1)).strong())
                            .id_salt(if sticky { "sticky_name" } else { "name" })
                            .hint_text("Lesson name")
                            .desired_width((width - 55.0).max(40.0))
                            .frame(egui::Frame::NONE),
                    );
                    if focus {
                        name.request_focus();
                    }
                    if name.clicked() || focus {
                        clicked = true;
                    }
                },
            );
        })
        .response;
    HeaderResponse {
        clicked: clicked || response.clicked(),
        response,
    }
}

fn lesson_footer(ui: &mut egui::Ui, index: usize, name: &str, width: f32) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 26.0), Sense::click());
    let painter = ui.painter();
    painter.rect_filled(rect, 0.0, Color32::from_gray(65));
    painter.rect_stroke(
        rect,
        0.0,
        Stroke::new(1.0, Color32::LIGHT_GRAY),
        egui::StrokeKind::Inside,
    );
    painter
        .with_clip_rect(egui::Rect::from_min_max(
            rect.min,
            egui::pos2((rect.right() - 28.0).max(rect.left()), rect.bottom()),
        ))
        .text(
            egui::pos2(rect.left() + 8.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            format!(
                "Lesson {}: {}",
                index + 1,
                if name.is_empty() { "Untitled" } else { name }
            ),
            egui::TextStyle::Button.resolve(ui.style()),
            Color32::WHITE,
        );
    painter.text(
        egui::pos2(rect.right() - 8.0, rect.center().y),
        egui::Align2::RIGHT_CENTER,
        "▲",
        egui::TextStyle::Button.resolve(ui.style()),
        Color32::WHITE,
    );
    response
}

struct CardResponse {
    text: egui::Response,
    clicked: bool,
    delete: bool,
}

fn card(
    ui: &mut egui::Ui,
    entry: &mut Entry,
    l1_name: &str,
    width: f32,
    focus: bool,
    selected: bool,
) -> CardResponse {
    let (background, accent, label) = match entry.language {
        Language::L1 => (L1_COLOR, L1_ACCENT, l1_name),
        Language::Latin => (LATIN_COLOR, LATIN_ACCENT, "Latin"),
    };
    let mut text_response = None;
    let frame = egui::Frame::new()
        .fill(if selected {
            background.gamma_multiply(1.35)
        } else {
            background
        })
        .stroke(if selected {
            Stroke::new(1.0, Color32::WHITE)
        } else {
            Stroke::new(1.0, Color32::DARK_GRAY)
        })
        .inner_margin(6.0)
        .show(ui, |ui| {
            ui.vertical(|ui| {
                ui.set_width((width - 12.0).max(8.0));
                let layout = if entry.participant == Participant::Teacher {
                    egui::Layout::left_to_right(egui::Align::Center)
                } else {
                    egui::Layout::right_to_left(egui::Align::Center)
                };
                ui.with_layout(layout, |ui| {
                    ui.label(RichText::new(label).color(accent).strong());
                });
                ui.add_space(6.0);
                let response = ui.add(
                    egui::TextEdit::multiline(&mut entry.text)
                        .hint_text("Enter text…")
                        .desired_width((width - 12.0).max(8.0))
                        .desired_rows(1)
                        .frame(egui::Frame::NONE),
                );
                if focus {
                    response.request_focus();
                }
                text_response = Some(response);
            });
        });
    let text = text_response.expect("card text editor is always shown");
    let mut delete = false;
    let right_clicked = frame.response.container_secondary_clicked();
    if right_clicked {
        ui.ctx().memory_mut(|memory| {
            if let Some(id) = memory.focused() {
                memory.surrender_focus(id);
            }
        });
    }
    frame.response.container_context_menu(|ui| {
        if ui.button("Delete").clicked() {
            delete = true;
            ui.close();
        }
    });
    let clicked =
        right_clicked || text.clicked() || frame.response.interact(Sense::click()).clicked();
    CardResponse {
        text,
        clicked,
        delete,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_entries_belong_to_the_selected_lesson() {
        let mut app = App::default();
        let original_count = app.lessons[0].entries.len();
        app.new_lesson();
        app.add(Participant::Student, Language::Latin);
        assert_eq!(app.lessons[0].entries.len(), original_count);
        assert_eq!(app.lessons[1].entries.len(), 1);
        assert_eq!(app.focus_entry, Some((1, 0)));

        app.active_lesson = 0;
        app.selected_entry = None;
        app.lessons[0].expanded = false;
        app.add(Participant::Teacher, Language::L1);
        assert!(app.lessons[0].expanded);
        assert_eq!(app.lessons[0].entries.len(), original_count + 1);
    }

    #[test]
    fn inserts_after_selection_and_selects_new_entry() {
        let mut app = App::default();
        app.selected_entry = Some((0, 1));
        app.add(Participant::Student, Language::Latin);
        assert_eq!(app.lessons[0].entries[1].text, "manus");
        assert!(app.lessons[0].entries[2].text.is_empty());
        assert!(app.lessons[0].entries[3].text.starts_with("Can you"));
        assert_eq!(app.selected_entry, Some((0, 2)));
        assert_eq!(app.focus_entry, Some((0, 2)));
        app.add(Participant::Teacher, Language::L1);
        assert_eq!(app.selected_entry, Some((0, 3)));
        assert!(app.lessons[0].entries[4].text.starts_with("Can you"));
    }

    #[test]
    fn undo_and_redo_text_and_structure() {
        let mut app = App::default();
        app.new_lesson();
        app.lessons[1].name = "Forum".into();
        app.record_text(TextTarget::LessonName(1), String::new(), "Forum".into());
        app.add(Participant::Student, Language::Latin);
        app.lessons[1].entries[0].text = "salve".into();
        app.record_text(TextTarget::Entry(1, 0), String::new(), "salve".into());

        app.undo();
        assert_eq!(app.lessons[1].entries[0].text, "");
        app.undo();
        assert!(app.lessons[1].entries.is_empty());
        assert_eq!(app.selected_entry, None);
        app.undo();
        assert_eq!(app.lessons[1].name, "");
        app.undo();
        assert_eq!(app.lessons.len(), 1);
        assert_eq!(app.active_lesson, 0);

        for _ in 0..4 {
            app.redo();
        }
        assert_eq!(app.lessons[1].name, "Forum");
        assert_eq!(app.lessons[1].entries[0].text, "salve");
    }

    #[test]
    fn undo_preserves_a_branch_in_history() {
        let mut app = App::default();
        app.l1_name = "French".into();
        app.record_text(TextTarget::L1Name, "English".into(), "French".into());
        app.undo();
        app.l1_name = "German".into();
        app.record_text(TextTarget::L1Name, "English".into(), "German".into());
        app.undo();
        assert_eq!(app.l1_name, "English");
        app.undo();
        assert_eq!(app.l1_name, "French");
    }

    #[test]
    fn deletion_is_undoable_and_adjusts_selection() {
        let mut app = App::default();
        app.selected_entry = Some((0, 3));
        app.delete_entry(0, 1);
        assert_eq!(app.lessons[0].entries.len(), 5);
        assert_eq!(app.selected_entry, Some((0, 2)));
        app.undo();
        assert_eq!(app.lessons[0].entries[1].text, "manus");
        assert_eq!(app.selected_entry, Some((0, 3)));
        app.redo();
        assert_eq!(app.lessons[0].entries.len(), 5);
        assert_eq!(app.selected_entry, Some((0, 2)));
        app.delete_entry(0, 2);
        assert_eq!(app.selected_entry, None);
        app.undo();
        assert_eq!(app.lessons[0].entries[2].text, "manus");
    }
}
