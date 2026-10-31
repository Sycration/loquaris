//! Dispatch of main-window keyboard shortcuts.
use super::{App, FileAction};
use crate::{Language, Participant, keybinds::Action, ui};
use eframe::egui;

impl App {
    /// Run the shortcuts pressed this frame. File actions are returned so the
    /// caller can run them after the frame's other UI has been handled.
    pub fn handle_shortcuts(&mut self, ui: &mut egui::Ui) -> Option<FileAction> {
        if self.keybind_editor.is_recording()
            || self.parsing_review.is_some()
            || self.audio_editor.is_some()
        {
            return None;
        }
        let actions = ui.input_mut(|input| {
            self.keybinds
                .take_actions(input, self.editing_entry.is_some())
        });
        if !actions.is_empty() {
            ui.ctx().request_repaint_of(egui::ViewportId::ROOT);
        }
        let mut file_action = None;
        for action in actions {
            if !self.project_open && action != Action::OpenProject {
                continue;
            }
            match action {
                Action::Undo => self.undo(),
                Action::Redo | Action::RedoAlternative => self.redo(),
                Action::OpenProject => file_action = Some(FileAction::Open),
                Action::SaveProject => file_action = Some(FileAction::Save),
                Action::SaveProjectAs => file_action = Some(FileAction::SaveAs),
                Action::EditEntry => self.edit_selected(),
                Action::StopEditing => {
                    self.editing_entry = None;
                    self.focus_entry = None;
                    ui::surrender_focus(ui.ctx());
                }
                Action::DeselectEntries => self.deselect_entries(),
                Action::SelectPrevious | Action::SelectNext => {
                    self.select_adjacent(action == Action::SelectNext);
                    ui::surrender_focus(ui.ctx());
                }
                Action::AddTeacherL1 => self.add(Participant::Teacher, Language::L1),
                Action::AddTeacherLatin => self.add(Participant::Teacher, Language::Latin),
                Action::AddStudentLatin => self.add(Participant::Student, Language::Latin),
                Action::SwitchLanguage
                | Action::MoveToStudent
                | Action::MoveToTeacher
                | Action::MoveEntryUp
                | Action::MoveEntryDown
                | Action::DuplicateEntry => {
                    if self.selected_entry_shortcut(action) {
                        ui::surrender_focus(ui.ctx());
                    }
                }
                Action::RecorderPlayPause
                | Action::RecorderRecord
                | Action::RecorderStopPlayback
                | Action::RecorderSeekBack
                | Action::RecorderSeekForward
                | Action::RecorderSeekStart
                | Action::RecorderSeekEnd => {
                    unreachable!("recorder shortcuts are scoped to the recorder")
                }
            }
        }
        file_action
    }
}
