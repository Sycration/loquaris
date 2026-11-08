//! Creating, opening, saving, and closing project files.
use super::App;
use crate::{Lesson, project, ui, word_index};
use eframe::egui;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy)]
pub enum FileAction {
    New,
    Open,
    Save,
    SaveAs,
    Close,
}

impl App {
    pub fn run_file_action(&mut self, action: FileAction, ctx: &egui::Context) {
        match action {
            FileAction::New => self.new_project(ctx),
            FileAction::Open => self.open_project(ctx),
            FileAction::Save => {
                self.save_project(false);
            }
            FileAction::SaveAs => {
                self.save_project(true);
            }
            FileAction::Close => self.close_project(ctx),
        }
    }

    pub fn project_snapshot(&self) -> project::Project {
        let mut project = project::Project::new(
            self.project_name.clone(),
            self.l1_name.clone(),
            self.lessons.clone(),
        );
        project.export_settings = self.export_settings.clone();
        project.audio_gaps = self.audio_gaps;
        project
    }

    /// The snapshot plus derived caches that are saved but don't affect dirtiness.
    pub fn project_snapshot_for_save(&self) -> project::Project {
        let mut project = self.project_snapshot();
        project.word_cache = self.word_index.persisted_cache();
        project
    }

    pub fn is_dirty(&self) -> bool {
        self.project_open && self.current_history_token() != self.saved_history_token
    }

    /// Ask for a destination, confirming before replacing a file the dialog
    /// couldn't warn about because the extension was added afterwards.
    fn choose_save_path(&self) -> Option<PathBuf> {
        let mut dialog = rfd::FileDialog::new().add_filter("Loquaris project", &["loqr"]);
        if let Some(directory) = self.dialog_directory() {
            dialog = dialog.set_directory(directory);
        }
        dialog = dialog.set_file_name(project::suggested_file_name(&self.project_name));
        let (path, extension_added) = project::ensure_extension(&dialog.save_file()?);
        if extension_added
            && path.exists()
            && rfd::MessageDialog::new()
                .set_title("Replace existing file?")
                .set_description(format!("{} already exists. Replace it?", path.display()))
                .set_level(rfd::MessageLevel::Warning)
                .set_buttons(rfd::MessageButtons::YesNo)
                .show()
                != rfd::MessageDialogResult::Yes
        {
            return None;
        }
        Some(path)
    }

    pub fn save_project(&mut self, save_as: bool) -> bool {
        if !self.project_open {
            return false;
        }
        let path = match &self.project_path {
            Some(path) if !save_as => path.clone(),
            _ => match self.choose_save_path() {
                Some(path) => path,
                None => return false,
            },
        };
        let project = self.project_snapshot_for_save();
        match project.save(&path) {
            Ok(()) => {
                self.recent_projects.record(&path, &project.name);
                let name = path
                    .file_name()
                    .unwrap_or(path.as_os_str())
                    .to_string_lossy();
                self.messages().success(format!("Saved {name}"));
                self.project_path = Some(path);
                self.saved_history_token = self.current_history_token();
                true
            }
            Err(error) => {
                self.messages().error(error);
                false
            }
        }
    }

    /// Offer to save unsaved changes; returns false if the user cancels.
    pub fn confirm_discard_changes(&mut self) -> bool {
        if !self.is_dirty() {
            return true;
        }
        match rfd::MessageDialog::new()
            .set_title("Unsaved changes")
            .set_description("Save changes to the current project first?")
            .set_level(rfd::MessageLevel::Warning)
            .set_buttons(rfd::MessageButtons::YesNoCancel)
            .show()
        {
            rfd::MessageDialogResult::Yes => self.save_project(false),
            rfd::MessageDialogResult::No => true,
            _ => false,
        }
    }

    pub fn replace_project(&mut self, project: project::Project, path: Option<PathBuf>) {
        self.reset_project_state();
        let project::Project {
            name,
            l1_name,
            lessons,
            export_settings,
            audio_gaps,
            word_cache,
        } = project;
        self.project_open = true;
        self.project_name = name;
        self.l1_name = l1_name;
        self.word_index = word_index::WordIndex::from_project(&word_cache, &lessons);
        self.lessons = lessons;
        self.export_settings = export_settings;
        self.audio_gaps = audio_gaps;
        self.project_path = path;
        self.scrollbar_offset = Some(0.0);
    }

    fn open_project(&mut self, ctx: &egui::Context) {
        let mut dialog = rfd::FileDialog::new().add_filter("Loquaris projects", &["loqr"]);
        if let Some(directory) = self.dialog_directory() {
            dialog = dialog.set_directory(directory);
        }
        if let Some(path) = dialog.pick_file() {
            self.open_path(path, ctx);
        }
    }

    pub fn open_path(&mut self, path: PathBuf, ctx: &egui::Context) {
        if !self.confirm_discard_changes() {
            return;
        }
        match project::Project::load(&path) {
            Ok(project) => {
                self.recent_projects.record(&path, &project.name);
                self.replace_project(project, Some(path));
                ui::surrender_focus(ctx);
            }
            Err(error) => self.messages().error(error),
        }
    }

    /// The project's folder, or the user's documents folder for new projects.
    pub fn dialog_directory(&self) -> Option<PathBuf> {
        self.project_path
            .as_ref()
            .and_then(|path| path.parent())
            .filter(|path| !path.as_os_str().is_empty())
            .map(Path::to_path_buf)
            .or_else(|| {
                directories::UserDirs::new()
                    .and_then(|dirs| dirs.document_dir().map(Path::to_path_buf))
            })
    }

    pub fn new_project(&mut self, ctx: &egui::Context) {
        if !self.confirm_discard_changes() {
            return;
        }
        self.replace_project(
            project::Project::new(String::new(), "English".into(), vec![Lesson::empty()]),
            None,
        );
        self.project_details_open = true;
        self.focus_project_name = true;
        ui::surrender_focus(ctx);
    }

    pub fn close_project(&mut self, ctx: &egui::Context) {
        if !self.confirm_discard_changes() {
            return;
        }
        self.reset_project_state();
        self.project_details_open = false;
        ui::surrender_focus(ctx);
        ctx.request_repaint();
    }
}
