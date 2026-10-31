//! Lesson header and footer bars.
use crate::{Lesson, model::display_name};
use eframe::egui::{self, Color32, RichText, Sense, Stroke, Vec2};

pub struct HeaderResponse {
    pub response: egui::Response,
    pub clicked: bool,
    pub play_clicked: bool,
}

/// What a lesson header shows besides the lesson itself.
pub struct HeaderOptions<'a> {
    pub width: f32,
    pub focus: bool,
    /// The copy pinned to the top of the viewport, which needs its own ids.
    pub sticky: bool,
    pub recording_summary: &'a str,
    /// `Some(true)` while playing, `Some(false)` while paused.
    pub playback: Option<bool>,
    pub play_enabled: bool,
}

#[derive(Clone, Copy)]
enum Chevron {
    Right,
    Down,
    Up,
}

fn paint_chevron(painter: &egui::Painter, center: egui::Pos2, direction: Chevron, color: Color32) {
    let points = match direction {
        Chevron::Right => [
            egui::vec2(-2.5, -5.0),
            egui::vec2(2.5, 0.0),
            egui::vec2(-2.5, 5.0),
        ],
        Chevron::Down => [
            egui::vec2(-5.0, -2.5),
            egui::vec2(0.0, 2.5),
            egui::vec2(5.0, -2.5),
        ],
        Chevron::Up => [
            egui::vec2(-5.0, 2.5),
            egui::vec2(0.0, -2.5),
            egui::vec2(5.0, 2.5),
        ],
    };
    painter.add(egui::Shape::line(
        points.into_iter().map(|offset| center + offset).collect(),
        Stroke::new(1.8, color),
    ));
}

/// Expand toggle, editable name, play button, and recording summary.
pub fn lesson_header(
    ui: &mut egui::Ui,
    lesson: &mut Lesson,
    index: usize,
    options: HeaderOptions,
) -> HeaderResponse {
    let HeaderOptions {
        width,
        focus,
        sticky,
        recording_summary,
        playback,
        play_enabled,
    } = options;
    let mut clicked = false;
    let mut play_clicked = false;
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
                    let expand = ui
                        .add(egui::Button::new("").min_size(Vec2::splat(24.0)))
                        .on_hover_text(if lesson.expanded {
                            "Collapse lesson"
                        } else {
                            "Expand lesson"
                        });
                    paint_chevron(
                        ui.painter(),
                        expand.rect.center(),
                        if lesson.expanded {
                            Chevron::Down
                        } else {
                            Chevron::Right
                        },
                        ui.visuals().widgets.inactive.fg_stroke.color,
                    );
                    if expand.clicked() {
                        lesson.expanded = !lesson.expanded;
                        clicked = true;
                    }
                    ui.add_space(8.0);
                    let status_width = ui
                        .fonts_mut(|fonts| {
                            fonts
                                .layout_no_wrap(
                                    recording_summary.to_owned(),
                                    egui::FontId::proportional(12.0),
                                    Color32::WHITE,
                                )
                                .size()
                                .x
                                + 10.0
                        })
                        .min((ui.available_width() - 40.0).max(0.0));
                    let button_width = 30.0;
                    let name_width = (ui.available_width() - status_width - button_width).max(0.0);
                    let name = ui.add_sized(
                        [name_width, 24.0],
                        egui::TextEdit::singleline(&mut lesson.name)
                            .prefix(RichText::new(format!("Lesson {}: ", index + 1)).strong())
                            .id_salt(if sticky { "sticky_name" } else { "name" })
                            .hint_text("Lesson name")
                            .desired_width(name_width)
                            .frame(egui::Frame::NONE),
                    );
                    let (icon, hover) = match playback {
                        Some(true) => ("❚❚", "Pause lesson"),
                        Some(false) => ("▶", "Resume lesson"),
                        None => ("▶", "Play lesson"),
                    };
                    play_clicked = ui
                        .add_enabled(
                            play_enabled,
                            egui::Button::new(icon).min_size(Vec2::new(button_width, 24.0)),
                        )
                        .on_hover_text(hover)
                        .clicked();
                    ui.allocate_ui_with_layout(
                        Vec2::new(status_width, 24.0),
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(recording_summary)
                                        .size(12.0)
                                        .color(Color32::WHITE),
                                )
                                .truncate(),
                            );
                        },
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
        clicked: !play_clicked && (clicked || response.clicked()),
        play_clicked,
        response,
    }
}

/// A bar closing an expanded lesson; clicking it collapses the lesson.
pub fn lesson_footer(ui: &mut egui::Ui, index: usize, name: &str, width: f32) -> egui::Response {
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
            format!("Lesson {}: {}", index + 1, display_name(name)),
            egui::TextStyle::Button.resolve(ui.style()),
            Color32::WHITE,
        );
    paint_chevron(
        painter,
        egui::pos2(rect.right() - 14.0, rect.center().y),
        Chevron::Up,
        Color32::WHITE,
    );
    response
}
