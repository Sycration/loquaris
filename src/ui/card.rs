//! An entry card: its text, hover buttons, audio marker, and context menu.
use super::{AUDIO_MARKER_COLOR, L1_ACCENT, L1_COLOR, LATIN_ACCENT, LATIN_COLOR, entry_font};
use crate::{
    Entry, Language, Participant,
    app::{BulkAction, EntryAction, MenuAction},
};
use eframe::egui::{self, Color32, RichText, Sense, Stroke, Vec2};

pub struct CardResponse {
    pub text: egui::Response,
    pub bounds: egui::Rect,
    pub clicked: bool,
    pub right_clicked: bool,
    pub edit_requested: bool,
    pub audio_requested: bool,
    pub action: Option<MenuAction>,
    /// The displayed (not edited) text is being selected: a drag started on
    /// it, or it was double- or triple-clicked to select a word or line.
    pub text_selecting: bool,
}

impl CardResponse {
    /// Whether the click should change the selection. Clicks and drags inside
    /// the text being edited only move the cursor.
    pub fn requests_selection(&self, editing: bool) -> bool {
        self.clicked
            && (self.right_clicked
                || !editing
                || !(self.text.clicked() || self.text.drag_started() || self.text.dragged()))
    }
}

/// Which context-menu items apply to an entry and the current selection.
#[derive(Clone, Copy, Default)]
pub struct EntryMenuOptions {
    pub show_move: bool,
    pub can_move_up: bool,
    pub can_move_down: bool,
    pub selected: bool,
    pub multiselect: bool,
    pub contiguous: bool,
    pub can_group_up: bool,
    pub can_group_down: bool,
    pub has_teacher: bool,
    pub has_student: bool,
    pub has_teacher_l1: bool,
    pub has_teacher_latin: bool,
}

/// The strip beside a card, on the side facing the centre, marking attached audio.
pub fn audio_marker_rect(rect: egui::Rect, entry: &Entry) -> Option<egui::Rect> {
    entry.audio.as_ref()?;
    let x = match entry.participant {
        Participant::Teacher => rect.right() + 3.0,
        Participant::Student => rect.left() - 7.0,
    };
    Some(egui::Rect::from_min_max(
        egui::pos2(x, rect.top() + 2.0),
        egui::pos2(x + 4.0, (rect.bottom() - 2.0).max(rect.top() + 2.0)),
    ))
}

#[allow(clippy::too_many_arguments)]
pub fn card(
    ui: &mut egui::Ui,
    entry: &mut Entry,
    l1_name: &str,
    width: f32,
    focus: bool,
    selected: bool,
    editing: bool,
    options: EntryMenuOptions,
    first_lemmas: &[&str],
    // Search matches to highlight, as sorted byte ranges of the text.
    highlights: &[std::ops::Range<usize>],
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
                    ui.set_max_width((width - 98.0).max(8.0));
                    ui.label(
                        RichText::new(label)
                            .font(entry_font(entry.language, 12.0))
                            .color(accent)
                            .strong(),
                    );
                });
                ui.add_space(6.0);
                let text_width = (width - 12.0).max(8.0);
                let response = ui
                    .allocate_ui_with_layout(
                        Vec2::new(text_width, ui.spacing().interact_size.y),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            if editing {
                                text_editor(ui, entry, text_width)
                            } else {
                                text_label(ui, entry, text_width, highlights)
                            }
                        },
                    )
                    .inner;
                if focus && editing {
                    response.request_focus();
                }
                text_response = Some(response);
            });
        });
    let text = text_response.expect("card text is always shown");
    let hovered = frame.response.container_contains_pointer();
    if !first_lemmas.is_empty() {
        paint_first_lemmas(
            ui,
            frame.response.rect,
            entry.participant,
            hovered,
            first_lemmas,
        );
    }
    if let Some(rect) = audio_marker_rect(frame.response.rect, entry) {
        ui.painter().rect_filled(rect, 0.0, AUDIO_MARKER_COLOR);
    }
    let mut edit_requested = false;
    let mut audio_requested = false;
    if hovered {
        let rect = egui::Rect::from_min_size(
            egui::pos2(
                frame.response.rect.right() - 43.0,
                frame.response.rect.top() + 3.0,
            ),
            Vec2::splat(40.0),
        );
        let mut overlay = ui.new_child(egui::UiBuilder::new().max_rect(rect));
        edit_requested = overlay
            .put(rect, egui::Button::new(RichText::new("✎").size(25.0)))
            .on_hover_text("Edit entry")
            .clicked();
        let audio_rect = rect.translate(egui::vec2(-40.0, 0.0));
        let mut audio_overlay = ui.new_child(egui::UiBuilder::new().max_rect(audio_rect));
        audio_requested = audio_overlay
            .put(audio_rect, egui::Button::new(""))
            .on_hover_text(if entry.audio.is_some() {
                "Edit attached audio"
            } else {
                "Record or import audio"
            })
            .clicked();
        let color = if entry.audio.is_some() {
            accent
        } else {
            Color32::WHITE
        };
        paint_speaker(audio_overlay.painter(), audio_rect.center(), color);
    }
    let right_clicked = frame.response.container_secondary_clicked();
    if right_clicked {
        super::surrender_focus(ui.ctx());
    }
    let mut action = None;
    frame.response.container_context_menu(|ui| {
        action = if options.multiselect && options.selected {
            bulk_menu(ui, options)
        } else {
            entry_menu(ui, entry, options)
        };
    });
    // A single click is not a selection, even though egui reports the
    // clicked label's whole text as one.
    let text_selecting =
        !editing && (text.drag_started() || text.double_clicked() || text.triple_clicked());
    let clicked = right_clicked
        || text.clicked()
        || text.drag_started()
        || frame.response.container_clicked();
    CardResponse {
        text,
        bounds: frame.response.rect,
        clicked,
        right_clicked,
        edit_requested,
        audio_requested,
        action,
        text_selecting,
    }
}

fn text_editor(ui: &mut egui::Ui, entry: &mut Entry, width: f32) -> egui::Response {
    ui.add(
        egui::TextEdit::multiline(&mut entry.text)
            .font(entry_font(entry.language, 15.0))
            .align(egui::Align2::LEFT_TOP)
            .hint_text("Enter text…")
            .desired_width(width)
            .desired_rows(1)
            .frame(egui::Frame::NONE),
    )
}

/// Match TextEdit's layout job, including its line height and trailing
/// whitespace, so switching between viewing and editing doesn't reflow text.
fn text_label(
    ui: &mut egui::Ui,
    entry: &Entry,
    width: f32,
    highlights: &[std::ops::Range<usize>],
) -> egui::Response {
    let font = entry_font(entry.language, 15.0);
    let line_height =
        ui.fonts_mut(|fonts| fonts.row_height(&font)) + ui.spacing().extra_text_line_spacing;
    let (text, color) = if entry.text.is_empty() {
        ("Enter text…", ui.visuals().weak_text_color())
    } else {
        (entry.text.as_str(), Color32::WHITE)
    };
    let mut job = egui::text::LayoutJob::simple(text.to_owned(), font, color, width);
    job.keep_trailing_whitespace = true;
    if !entry.text.is_empty() {
        super::highlight_matches(&mut job, highlights);
    }
    for section in &mut job.sections {
        section.format.line_height = Some(line_height);
    }
    let response = ui.add(
        egui::Label::new(job)
            .halign(egui::Align::Min)
            .selectable(true)
            .wrap()
            .sense(Sense::click_and_drag()),
    );
    ui.add_space((ui.spacing().interact_size.y - response.rect.height()).max(0.0));
    response
}

/// Lemmas first used in this entry, in the card's top corner away from the
/// hover buttons.
fn paint_first_lemmas(
    ui: &egui::Ui,
    rect: egui::Rect,
    participant: Participant,
    hovered: bool,
    lemmas: &[&str],
) {
    let (position, alignment) = match participant {
        Participant::Teacher => (
            egui::pos2(
                rect.right() - if hovered { 90.0 } else { 6.0 },
                rect.top() + 5.0,
            ),
            egui::Align2::RIGHT_TOP,
        ),
        Participant::Student => (
            egui::pos2(rect.left() + 6.0, rect.top() + 5.0),
            egui::Align2::LEFT_TOP,
        ),
    };
    ui.painter().with_clip_rect(rect.shrink(4.0)).text(
        position,
        alignment,
        lemmas.join(" · "),
        entry_font(Language::Latin, 11.0),
        Color32::YELLOW,
    );
}

fn paint_speaker(painter: &egui::Painter, center: egui::Pos2, color: Color32) {
    let point = |x, y| center + egui::vec2(x, y);
    painter.add(egui::Shape::convex_polygon(
        vec![
            point(-11.0, -4.0),
            point(-6.0, -4.0),
            point(0.0, -9.0),
            point(0.0, 9.0),
            point(-6.0, 4.0),
            point(-11.0, 4.0),
        ],
        color,
        Stroke::NONE,
    ));
    for scale in [1.0, 1.8] {
        painter.add(egui::Shape::line(
            vec![
                point(4.0 * scale, -5.0 * scale),
                point(6.0 * scale, 0.0),
                point(4.0 * scale, 5.0 * scale),
            ],
            Stroke::new(1.5, color),
        ));
    }
}

/// Show a menu button; when clicked, close the menu and return `action`.
fn item<T>(ui: &mut egui::Ui, enabled: bool, label: &str, action: T) -> Option<T> {
    let clicked = ui.add_enabled(enabled, egui::Button::new(label)).clicked();
    if clicked {
        ui.close();
    }
    clicked.then_some(action)
}

fn bulk_menu(ui: &mut egui::Ui, options: EntryMenuOptions) -> Option<MenuAction> {
    let mut action = item(ui, true, "Duplicate selected", BulkAction::Duplicate);
    for (visible, label, choice) in [
        (
            options.has_teacher,
            "Move to student →",
            BulkAction::ToStudent,
        ),
        (
            options.has_student,
            "← Move to teacher",
            BulkAction::ToTeacher,
        ),
        (
            options.has_teacher_l1,
            "Switch to Latin",
            BulkAction::ToLatin,
        ),
        (options.has_teacher_latin, "Switch to L1", BulkAction::ToL1),
    ] {
        if visible {
            action = item(ui, true, label, choice).or(action);
        }
    }
    if options.contiguous {
        action = item(ui, options.can_group_up, "Move up", BulkAction::MoveUp).or(action);
        action = item(
            ui,
            options.can_group_down,
            "Move down",
            BulkAction::MoveDown,
        )
        .or(action);
    }
    action = item(ui, true, "Delete selected", BulkAction::Delete).or(action);
    action.map(MenuAction::Bulk)
}

fn entry_menu(ui: &mut egui::Ui, entry: &Entry, options: EntryMenuOptions) -> Option<MenuAction> {
    let mut action = item(ui, true, "Duplicate", EntryAction::Duplicate);
    if entry.language == Language::Latin {
        action = item(ui, true, "Auto-macronize", EntryAction::AutoMacronize).or(action);
    }
    let participant_label = match entry.participant {
        Participant::Teacher => "Move to student →",
        Participant::Student => "← Move to teacher",
    };
    action = item(ui, true, participant_label, EntryAction::MoveParticipant).or(action);
    if entry.participant == Participant::Teacher {
        let language_label = match entry.language {
            Language::L1 => "Switch to Latin",
            Language::Latin => "Switch to L1",
        };
        action = item(ui, true, language_label, EntryAction::SwitchLanguage).or(action);
    }
    if options.show_move {
        action = item(ui, options.can_move_up, "Move up", EntryAction::MoveUp).or(action);
        action = item(
            ui,
            options.can_move_down,
            "Move down",
            EntryAction::MoveDown,
        )
        .or(action);
    }
    action = item(ui, true, "Delete", EntryAction::Delete).or(action);
    action.map(MenuAction::Single)
}
