//! Menus, the toolbar, column headings, and the add-entry bar.
use super::{Foreground, L1_ACCENT, L1_COLOR, LATIN_ACCENT, LATIN_COLOR, entry_font};
use crate::{
    Language, Participant,
    app::{App, FileAction},
};
use eframe::egui::{self, Color32, RichText, Vec2};

/// Menu-bar spacing shared by the welcome screen and the project window.
pub fn menu_bar_spacing(ui: &mut egui::Ui) {
    ui.spacing_mut().item_spacing.x = 12.0;
    ui.spacing_mut().button_padding = Vec2::new(12.0, 6.0);
    ui.spacing_mut().interact_size.y = 32.0;
}

/// A menu item that closes the menu when clicked.
pub fn menu_item(ui: &mut egui::Ui, label: &str) -> bool {
    let clicked = ui.button(label).clicked();
    if clicked {
        ui.close();
    }
    clicked
}

/// Add a button, recording its rect as foreground.
fn button(ui: &mut egui::Ui, button: egui::Button, foreground: &mut Foreground) -> bool {
    let response = ui.add(button);
    foreground.push(response.rect);
    response.clicked()
}

fn add_entry_button(
    ui: &mut egui::Ui,
    label: &str,
    language: Language,
    foreground: &mut Foreground,
) -> bool {
    let (accent, fill) = match language {
        Language::L1 => (L1_ACCENT, L1_COLOR),
        Language::Latin => (LATIN_ACCENT, LATIN_COLOR),
    };
    let text = RichText::new(format!("+ {label}")).color(accent).strong();
    button(ui, egui::Button::new(text).fill(fill), foreground)
}

impl App {
    pub(super) fn menu_bar(
        &mut self,
        ui: &mut egui::Ui,
        foreground: &mut Foreground,
    ) -> Option<FileAction> {
        let mut file_action = None;
        egui::MenuBar::new().ui(ui, |ui| {
            menu_bar_spacing(ui);
            let file_menu = ui.menu_button("File", |ui| {
                if menu_item(ui, "New project") {
                    file_action = Some(FileAction::New);
                }
                if menu_item(ui, "Open…") {
                    file_action = Some(FileAction::Open);
                }
                if menu_item(ui, "Save") {
                    file_action = Some(FileAction::Save);
                }
                if menu_item(ui, "Save as…") {
                    file_action = Some(FileAction::SaveAs);
                }
                if menu_item(ui, "Export…") {
                    self.export_window.open = true;
                }
                if menu_item(ui, "Close project") {
                    file_action = Some(FileAction::Close);
                }
            });
            foreground.push(file_menu.response.rect);
            let edit_menu = ui.menu_button("Edit", |ui| {
                if menu_item(ui, "Project details…") {
                    self.project_details_open = true;
                }
                if menu_item(ui, "Keybinds…") {
                    self.keybind_editor.open = true;
                }
            });
            foreground.push(edit_menu.response.rect);
            if self.is_dirty() {
                foreground.push(ui.label("  • Unsaved changes").rect);
            }
            if self.keybinds.storage_error.is_some() {
                foreground.push(
                    ui.colored_label(Color32::LIGHT_RED, "  • Keybind settings error")
                        .on_hover_text("Open Edit → Keybinds for details.")
                        .rect,
                );
            }
            if let Some(error) = &self.recent_projects.storage_error {
                foreground.push(
                    ui.colored_label(Color32::LIGHT_RED, "Recent projects error")
                        .on_hover_text(error)
                        .rect,
                );
            }
        });
        file_action
    }

    /// Playback errors, history buttons, and the participant column headings.
    pub(super) fn toolbar(&mut self, ui: &mut egui::Ui, width: f32, foreground: &mut Foreground) {
        if let Some(error) = &self.playback_error {
            foreground.push(ui.colored_label(Color32::LIGHT_RED, error).rect);
        }
        ui.allocate_ui_with_layout(
            Vec2::new(width, 26.0),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                if button(ui, egui::Button::new("+ New lesson"), foreground) {
                    self.new_lesson();
                }
                if button(ui, egui::Button::new("Undo"), foreground) {
                    self.undo();
                }
                if button(ui, egui::Button::new("Redo"), foreground) {
                    self.redo();
                }
            },
        );
        ui.add_space(10.0);
        let half = width / 2.0;
        ui.horizontal(|ui| {
            ui.allocate_ui_with_layout(
                Vec2::new(half, 32.0),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    foreground.push(ui.label(RichText::new("Teacher").size(30.0).strong()).rect);
                },
            );
            ui.allocate_ui_with_layout(
                Vec2::new(half, 32.0),
                egui::Layout::right_to_left(egui::Align::Center),
                |ui| {
                    let heading = RichText::new("Student")
                        .font(entry_font(Language::Latin, 30.0))
                        .strong();
                    foreground.push(ui.label(heading).rect);
                },
            );
        });
        foreground.push(ui.separator().rect);
        ui.add_space(4.0);
    }

    /// The bar under the script with one button per kind of new entry.
    pub(super) fn add_entry_bar(
        &mut self,
        ui: &mut egui::Ui,
        rect: egui::Rect,
        foreground: &mut Foreground,
    ) {
        let mut footer_ui = ui.new_child(egui::UiBuilder::new().max_rect(rect));
        let ui = &mut footer_ui;
        ui.add_space(4.0);
        foreground.push(ui.separator().rect);
        let target = self.target_lesson();
        let label = format!(
            "Adding to lesson {}: {}",
            target + 1,
            self.lessons[target].display_name()
        );
        foreground.push(
            ui.label(RichText::new(label).color(Color32::LIGHT_GRAY))
                .rect,
        );
        ui.add_space(8.0);
        let half = rect.width() / 2.0;
        ui.horizontal(|ui| {
            ui.allocate_ui_with_layout(
                Vec2::new(half, 35.0),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.add_space(10.0);
                    if add_entry_button(ui, &self.l1_name, Language::L1, foreground) {
                        self.add(Participant::Teacher, Language::L1);
                    }
                    ui.add_space(12.0);
                    if add_entry_button(ui, "Latin", Language::Latin, foreground) {
                        self.add(Participant::Teacher, Language::Latin);
                    }
                },
            );
            ui.allocate_ui_with_layout(
                Vec2::new(half, 35.0),
                egui::Layout::right_to_left(egui::Align::Center),
                |ui| {
                    ui.add_space(10.0);
                    if add_entry_button(ui, "Latin", Language::Latin, foreground) {
                        self.add(Participant::Student, Language::Latin);
                    }
                },
            );
        });
    }
}
