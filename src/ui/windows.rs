//! Child windows of the project: parsing review, recorder, details, and export.
use crate::{
    app::{App, Command, FileAction, Origin},
    keybinds::Action,
    parsing_review,
};
use eframe::egui;

/// Repaint the main window and the child window that dispatched an action.
fn repaint_after_child_action(ctx: &egui::Context, child: &str) {
    ctx.request_repaint_of(egui::ViewportId::ROOT);
    ctx.request_repaint_of(egui::ViewportId::from_hash_of(child));
}

impl App {
    /// Show every open child window, returning a file action one requested
    /// and the window it came from.
    pub(super) fn show_windows(&mut self, ctx: &egui::Context) -> Option<(FileAction, Origin)> {
        let mut file_action = None;
        self.show_parsing_review(ctx);
        if let Some(action) = self.show_audio_editor(ctx) {
            file_action = Some((action, Origin::Recorder));
        }
        if let Some(action) = self.show_project_details(ctx) {
            file_action = Some((action, Origin::Details));
        }
        self.show_export_window(ctx);
        file_action
    }

    fn show_parsing_review(&mut self, ctx: &egui::Context) {
        if let Some(mut review) = self.parsing_review.take() {
            match review.show(ctx) {
                Some(parsing_review::Outcome::Apply(after)) => self.apply_review(&review, after),
                Some(parsing_review::Outcome::Cancel) => {}
                None => self.parsing_review = Some(review),
            }
        }
    }

    /// Show the recorder. A new clip is applied only if its entry is unchanged
    /// since the recorder opened; otherwise the recorder closes.
    fn show_audio_editor(&mut self, ctx: &egui::Context) -> Option<FileAction> {
        let mut editor = self.audio_editor.take()?;
        let outcome = editor.show(ctx, &self.l1_name, &self.keybinds);
        let mut valid = true;
        if let Some(after) = outcome.changed {
            let (lesson, index) = editor.target;
            match self
                .lessons
                .get(lesson)
                .and_then(|lesson| lesson.entries.get(index))
            {
                Some(entry)
                    if *entry == editor.original && (entry.audio.clone(), entry.trim) != after =>
                {
                    self.execute(Command::SetAudio {
                        lesson,
                        index,
                        before: (entry.audio.clone(), entry.trim),
                        after,
                    });
                    editor.original.audio = self.lessons[lesson].entries[index].audio.clone();
                    editor.original.trim = self.lessons[lesson].entries[index].trim;
                }
                Some(entry)
                    if *entry != editor.original
                        && (editor.original.audio.clone(), editor.original.trim) != after =>
                {
                    self.toasts.error(
                        "The entry changed while the audio window was open. The clip was not applied.",
                    );
                    valid = false;
                }
                Some(_) => {}
                None => valid = false,
            }
        }
        if !valid {
            return None;
        }
        if !outcome.closed {
            self.audio_editor = Some(editor);
        }
        let mut file_action = None;
        for action in outcome.actions {
            match action {
                Action::Undo => self.from_origin(Origin::Recorder, Self::undo),
                Action::Redo | Action::RedoAlternative => {
                    self.from_origin(Origin::Recorder, Self::redo)
                }
                Action::SaveProject => file_action = Some(FileAction::Save),
                Action::SaveProjectAs => file_action = Some(FileAction::SaveAs),
                _ => unreachable!("only project actions leave the recorder"),
            }
            repaint_after_child_action(ctx, "entry-audio");
        }
        file_action
    }

    fn show_export_window(&mut self, ctx: &egui::Context) {
        let export_directory = self.dialog_directory();
        let before = self.export_settings.clone();
        let recording = self
            .audio_editor
            .as_ref()
            .is_some_and(|editor| editor.is_recording());
        let actions = self.export_window.show(
            ctx,
            &self.project_name,
            &self.l1_name,
            &self.lessons,
            &mut self.export_settings,
            &self.keybinds,
            export_directory.as_deref(),
            self.project_path.as_deref(),
            self.audio_gaps,
            recording,
        );
        if let Some(message) = self.export_window.exported.take() {
            self.toasts.success(message);
        }
        if self.export_settings != before {
            self.push_history(Command::SetExportSettings {
                before,
                after: self.export_settings.clone(),
            });
            ctx.request_repaint_of(egui::ViewportId::ROOT);
        }
        for action in actions {
            match action {
                Action::Undo => self.from_origin(Origin::Export, Self::undo),
                Action::Redo | Action::RedoAlternative => {
                    self.from_origin(Origin::Export, Self::redo)
                }
                _ => unreachable!("only history actions are dispatched in the export window"),
            }
            repaint_after_child_action(ctx, "export-pdf");
        }
    }
}
