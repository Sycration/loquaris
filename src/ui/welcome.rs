//! The start screen shown when no project is open.
use super::chrome::{menu_bar_spacing, menu_item};
use crate::{
    app::{App, FileAction},
    model::display_name,
};
use eframe::egui::{self, RichText};

impl App {
    pub(super) fn welcome(&mut self, ui: &mut egui::Ui) {
        let mut file_action = self.handle_shortcuts(ui);
        self.keybind_editor.show(ui.ctx(), &mut self.keybinds);
        super::apply_dark_style(ui);
        egui::MenuBar::new().ui(ui, |ui| {
            menu_bar_spacing(ui);
            ui.menu_button("File", |ui| {
                if menu_item(ui, "New project") {
                    file_action = Some(FileAction::New);
                }
                if menu_item(ui, "Open…") {
                    file_action = Some(FileAction::Open);
                }
            });
            ui.menu_button("Edit", |ui| {
                if menu_item(ui, "Keybinds…") {
                    self.keybind_editor.open = true;
                }
            });
        });

        let mut open_recent = None;
        let mut forget_recent = None;
        egui::Frame::new().inner_margin(24.0).show(ui, |ui| {
            ui.heading("Welcome to loquaris");
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                if ui.button("New project").clicked() {
                    file_action = Some(FileAction::New);
                }
                if ui.button("Open project…").clicked() {
                    file_action = Some(FileAction::Open);
                }
            });
            ui.add_space(20.0);
            ui.label(RichText::new("Recent projects").strong());
            ui.add_space(8.0);
            if self.recent_projects.projects.is_empty() {
                ui.label("No recent projects yet.");
                return;
            }
            egui::ScrollArea::vertical()
                .id_salt("recent_projects")
                .show(ui, |ui| {
                    for project in &self.recent_projects.projects {
                        egui::Frame::group(ui.style()).show(ui, |ui| {
                            ui.set_min_width((ui.available_width() - 16.0).max(0.0));
                            ui.horizontal(|ui| {
                                if ui.button(display_name(&project.name)).clicked() {
                                    open_recent = Some(project.path.clone());
                                }
                                if ui
                                    .button("Forget")
                                    .on_hover_text(
                                        "Remove from recent projects; the file is not deleted",
                                    )
                                    .clicked()
                                {
                                    forget_recent = Some(project.path.clone());
                                }
                            });
                            ui.label(project.path.to_string_lossy());
                        });
                        ui.add_space(6.0);
                    }
                });
        });
        if let Some(path) = forget_recent {
            self.recent_projects.forget(&path);
        }
        if let Some(path) = open_recent {
            self.open_path(path, ui.ctx());
        }
        if let Some(action @ (FileAction::New | FileAction::Open)) = file_action {
            self.run_file_action(action, ui.ctx());
        }
        self.report_storage_errors();
        self.toasts.show(ui.ctx());
        if self.project_open {
            ui.ctx().request_repaint();
        }
    }
}
