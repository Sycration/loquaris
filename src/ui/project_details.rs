//! The project details window: name, L1 language, and playback gaps.
use crate::app::{App, Command, FileAction, TextTarget};
use eframe::egui::{self, RichText};

impl App {
    pub(super) fn show_project_details(&mut self, ctx: &egui::Context) -> Option<FileAction> {
        if !self.project_details_open {
            return None;
        }
        let mut file_action = None;
        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("project-details"),
            egui::ViewportBuilder::default()
                .with_title("Project details")
                .with_inner_size([540.0, 300.0])
                .with_min_inner_size([340.0, 170.0]),
            |ui, _class| {
                if ui.input(|input| input.viewport().close_requested()) {
                    self.project_details_open = false;
                    return;
                }
                if let Some(action) = self.handle_shortcuts(ui) {
                    file_action = Some(action);
                }
                egui::Frame::new().inner_margin(12.0).show(ui, |ui| {
                    ui.heading("Project details");
                    ui.add_space(12.0);
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        let column_width = ((ui.available_width() - 16.0) / 2.0).max(120.0);
                        egui::Grid::new("project_details_table")
                            .num_columns(2)
                            .spacing([16.0, 6.0])
                            .striped(true)
                            .show(ui, |ui| self.details_table(ui, column_width));
                    });
                });
            },
        );
        file_action
    }

    fn details_table(&mut self, ui: &mut egui::Ui, column_width: f32) {
        let label = |ui: &mut egui::Ui, text: &str| {
            ui.add_sized([column_width, 28.0], egui::Label::new(text));
        };
        ui.add_sized(
            [column_width, 24.0],
            egui::Label::new(RichText::new("Detail").strong()),
        );
        ui.add_sized(
            [column_width, 24.0],
            egui::Label::new(RichText::new("Value").strong()),
        );
        ui.end_row();

        let before = self.audio_gaps;
        for (text, milliseconds) in [
            (
                "Same participant gap",
                &mut self.audio_gaps.same_participant_ms,
            ),
            (
                "Different participant gap",
                &mut self.audio_gaps.different_participant_ms,
            ),
        ] {
            label(ui, text);
            let mut seconds = f64::from(*milliseconds) / 1000.0;
            if ui
                .add(
                    egui::DragValue::new(&mut seconds)
                        .range(0.0..=60.0)
                        .speed(0.01)
                        .fixed_decimals(3)
                        .suffix(" s"),
                )
                .changed()
            {
                *milliseconds = (seconds * 1000.0).round() as u32;
            }
            ui.end_row();
        }
        if self.audio_gaps != before {
            self.lesson_playback = None;
            self.push_history(Command::SetAudioGaps {
                before,
                after: self.audio_gaps,
            });
            self.lesson_recording_summaries = None;
            ui.ctx().request_repaint_of(egui::ViewportId::ROOT);
        }

        label(ui, "Project name");
        let before = self.project_name.clone();
        let name = ui.add_sized(
            [column_width, 28.0],
            egui::TextEdit::singleline(&mut self.project_name),
        );
        if self.focus_project_name {
            name.request_focus();
            self.focus_project_name = false;
        }
        if name.changed() {
            self.record_text(TextTarget::ProjectName, before, self.project_name.clone());
            ui.ctx().request_repaint_of(egui::ViewportId::ROOT);
        }
        ui.end_row();

        label(ui, "L1 language");
        let before = self.l1_name.clone();
        if ui
            .add_sized(
                [column_width, 28.0],
                egui::TextEdit::singleline(&mut self.l1_name),
            )
            .changed()
        {
            self.record_text(TextTarget::L1Name, before, self.l1_name.clone());
            ui.ctx().request_repaint_of(egui::ViewportId::ROOT);
        }
        ui.end_row();
    }
}
