//! Application state and the operations that change it.
//!
//! Rendering lives in [`crate::ui`]; this module owns the state, the undoable
//! command history, selection, and project file handling.
mod editing;
mod files;
mod history;
mod layout;
mod playback;
mod selection;
mod shortcuts;

pub use editing::{BulkAction, EntryAction, MenuAction};
pub use files::FileAction;
pub use history::{Command, TextTarget};
pub use layout::entry_column_geometry;

use crate::{
    Lesson, audio, export_pdf, keybinds, lesson_player, parsing_review, project, recent_projects,
    search, word_index,
};
use cosmic_undo_2::Commands;

/// An entry's position as `(lesson, entry)`.
pub type Position = (usize, usize);

pub struct App {
    pub audio_editor: Option<audio::Editor>,
    pub lesson_playback: Option<lesson_player::Player>,
    pub playback_error: Option<String>,
    pub export_window: export_pdf::Window,
    pub export_settings: export_pdf::Settings,
    pub audio_gaps: project::AudioGaps,
    pub project_open: bool,
    pub recent_projects: recent_projects::RecentProjects,
    pub project_name: String,
    pub project_path: Option<std::path::PathBuf>,
    pub word_index: word_index::WordIndex,
    pub entry_row_heights: Vec<Vec<f32>>,
    pub layout_cache_width: f32,
    pub layout_cache_pixels_per_point: f32,
    pub history_tokens: Vec<u64>,
    pub history_position: usize,
    pub next_history_token: u64,
    pub saved_history_token: Option<u64>,
    #[cfg(test)]
    pub rendered_entry_count: usize,
    /// Where each rendered entry's text was drawn last frame.
    #[cfg(test)]
    pub entry_text_rects: Vec<(Position, eframe::egui::Rect)>,
    pub l1_name: String,
    pub lessons: Vec<Lesson>,
    pub lesson_recording_summaries: Option<Vec<String>>,
    pub active_lesson: usize,
    pub scroll_to_lesson: Option<usize>,
    pub scrollbar_offset: Option<f32>,
    pub focus_entry: Option<Position>,
    pub scroll_to_entry: Option<Position>,
    pub selected_entry: Option<Position>,
    pub extra_selected: std::collections::BTreeSet<Position>,
    pub selection_anchor: Option<Position>,
    pub editing_entry: Option<Position>,
    pub focus_name: Option<usize>,
    pub last_scroll_offset: f32,
    pub history: Commands<Command>,
    pub keybinds: keybinds::Registry,
    pub keybind_editor: keybinds::Editor,
    pub parsing_review: Option<parsing_review::Review>,
    pub project_details_open: bool,
    pub focus_project_name: bool,
    pub search: search::Search,
    /// Bumped whenever project content may have changed, to refresh derived views.
    pub content_generation: u64,
    /// Entry text was dragged across or double- or triple-clicked since the
    /// last press, so a settled label selection is from the transcript.
    pub transcript_text_selecting: bool,
}

impl Default for App {
    fn default() -> Self {
        Self {
            audio_editor: None,
            lesson_playback: None,
            playback_error: None,
            export_window: export_pdf::Window::default(),
            export_settings: export_pdf::Settings::default(),
            audio_gaps: project::AudioGaps::default(),
            project_open: false,
            recent_projects: recent_projects::RecentProjects::default(),
            project_name: String::new(),
            project_path: None,
            word_index: word_index::WordIndex::default(),
            entry_row_heights: Vec::new(),
            layout_cache_width: f32::NAN,
            layout_cache_pixels_per_point: 0.0,
            history_tokens: Vec::new(),
            history_position: 0,
            next_history_token: 1,
            saved_history_token: None,
            #[cfg(test)]
            rendered_entry_count: 0,
            #[cfg(test)]
            entry_text_rects: Vec::new(),
            l1_name: String::new(),
            lessons: Vec::new(),
            lesson_recording_summaries: None,
            active_lesson: 0,
            scroll_to_lesson: None,
            scrollbar_offset: None,
            focus_entry: None,
            scroll_to_entry: None,
            selected_entry: None,
            extra_selected: std::collections::BTreeSet::new(),
            selection_anchor: None,
            editing_entry: None,
            focus_name: None,
            last_scroll_offset: 0.0,
            history: Commands::new(),
            keybinds: keybinds::Registry::default(),
            keybind_editor: keybinds::Editor::default(),
            parsing_review: None,
            project_details_open: false,
            focus_project_name: false,
            search: search::Search::default(),
            content_generation: 0,
            transcript_text_selecting: false,
        }
    }
}

impl App {
    /// Drop all per-project state, keeping user settings and the details window.
    fn reset_project_state(&mut self) {
        *self = Self {
            keybinds: std::mem::take(&mut self.keybinds),
            keybind_editor: std::mem::take(&mut self.keybind_editor),
            recent_projects: std::mem::take(&mut self.recent_projects),
            project_details_open: self.project_details_open,
            next_history_token: self.next_history_token,
            search: search::Search::with_options_of(&self.search),
            ..Self::default()
        };
    }

    /// Expand and scroll to the primary selection after a structural change.
    fn reveal_selected(&mut self) {
        if let Some((lesson, index)) = self.selected_entry {
            self.active_lesson = lesson;
            self.lessons[lesson].expanded = true;
            self.scroll_to_entry = Some((lesson, index));
        }
    }
}
