#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod audio;
mod audio_devices;
mod export_pdf;
mod export_wav;
mod keybinds;
mod lesson_player;
mod macronize;
mod parsing_review;
mod project;
mod recent_projects;

use cosmic_undo_2::{Action, Commands};
use eframe::egui::{self, Color32, RichText, Sense, Stroke, Vec2};

const L1_COLOR: Color32 = Color32::from_rgb(24, 68, 80);
const LATIN_COLOR: Color32 = Color32::from_rgb(80, 0, 0);
const L1_ACCENT: Color32 = Color32::from_rgb(60, 184, 213);
const LATIN_ACCENT: Color32 = Color32::from_rgb(224, 70, 70);
const AUDIO_MARKER_COLOR: Color32 = Color32::from_rgb(189, 219, 63);
const AUDIO_MARKER_INSET: f32 = 8.0;
const LATIN_FONT: &str = "source-serif-4";

fn configure_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "ibm-plex-sans".into(),
        egui::FontData::from_static(include_bytes!("../assets/IBMPlexSans-Regular.ttf")).into(),
    );
    fonts.font_data.insert(
        LATIN_FONT.into(),
        egui::FontData::from_static(include_bytes!("../assets/SourceSerif4-Regular.ttf")).into(),
    );
    let proportional = fonts
        .families
        .entry(egui::FontFamily::Proportional)
        .or_default();
    proportional.insert(0, "ibm-plex-sans".into());
    let mut latin = vec![LATIN_FONT.to_owned()];
    latin.extend(proportional.iter().cloned());
    fonts
        .families
        .insert(egui::FontFamily::Name(LATIN_FONT.into()), latin);
    ctx.set_fonts(fonts);
    ctx.all_styles_mut(|style| {
        for font in style.text_styles.values_mut() {
            *font = egui::FontId::proportional(15.0);
        }
    });
}

fn entry_font(language: Language, size: f32) -> egui::FontId {
    let family = match language {
        Language::L1 => egui::FontFamily::Proportional,
        Language::Latin => egui::FontFamily::Name(LATIN_FONT.into()),
    };
    egui::FontId::new(size, family)
}

fn main() -> eframe::Result {
    eframe::run_native(
        "loquaris",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([800.0, 750.0])
                .with_min_inner_size([420.0, 350.0]),
            ..Default::default()
        },
        Box::new(|cc| {
            configure_fonts(&cc.egui_ctx);
            let mut app = App::default();
            app.keybinds = keybinds::Registry::load_user();
            app.recent_projects = recent_projects::RecentProjects::load_user();
            Ok(Box::new(app))
        }),
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Participant {
    Teacher,
    Student,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Language {
    L1,
    Latin,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Entry {
    participant: Participant,
    language: Language,
    text: String,
    audio: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Lesson {
    name: String,
    entries: Vec<Entry>,
    expanded: bool,
}

fn lesson_recording_summary(lesson: &Lesson, gaps: project::AudioGaps) -> String {
    if lesson.entries.is_empty() {
        return "Recordings incomplete".into();
    }
    let duration = lesson.entries.iter().try_fold(0.0, |sum, entry| {
        let clip = entry.audio.as_deref()?;
        Some(sum + audio::clip_duration(clip).ok()?)
    });
    let Some(duration) = duration else {
        return "Recordings incomplete".into();
    };
    let total = duration
        + lesson
            .entries
            .windows(2)
            .map(|pair| f64::from(gaps.between(pair[0].participant, pair[1].participant)) / 1000.0)
            .sum::<f64>();
    let tenths = (total * 10.0).round() as u64;
    let seconds = tenths / 10;
    if seconds >= 3600 {
        format!(
            "{}:{:02}:{:02}.{}",
            seconds / 3600,
            seconds / 60 % 60,
            seconds % 60,
            tenths % 10
        )
    } else {
        format!("{:02}:{:02}.{}", seconds / 60, seconds % 60, tenths % 10)
    }
}

#[derive(Clone)]
enum TextTarget {
    ProjectName,
    L1Name,
    LessonName(usize),
    Entry(usize, usize),
}

#[derive(Clone)]
enum Command {
    SetAudioGaps {
        before: project::AudioGaps,
        after: project::AudioGaps,
    },
    SetExportSettings {
        before: export_pdf::Settings,
        after: export_pdf::Settings,
    },
    SetAudio {
        lesson: usize,
        index: usize,
        before: Option<String>,
        after: Option<String>,
    },
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
    SetEntryKind {
        lesson: usize,
        index: usize,
        before: (Participant, Language),
        after: (Participant, Language),
    },
    MoveEntry {
        from: (usize, usize),
        to: (usize, usize),
    },
    BulkEntries {
        before: Vec<Vec<Entry>>,
        after: Vec<Vec<Entry>>,
        selected_before: Vec<(usize, usize)>,
        selected_after: Vec<(usize, usize)>,
        primary_before: Option<(usize, usize)>,
        primary_after: Option<(usize, usize)>,
    },
}

#[derive(Clone, Copy)]
enum EntryAction {
    Duplicate,
    Delete,
    MoveParticipant,
    SwitchLanguage,
    MoveUp,
    MoveDown,
    AutoMacronize,
}

#[derive(Clone, Copy)]
enum BulkAction {
    Duplicate,
    Delete,
    ToTeacher,
    ToStudent,
    ToL1,
    ToLatin,
    ToggleLanguage,
    MoveUp,
    MoveDown,
}

#[derive(Clone, Copy)]
enum MenuAction {
    Single(EntryAction),
    Bulk(BulkAction),
}

#[derive(Clone, Copy)]
enum FileAction {
    New,
    Open,
    Save,
    SaveAs,
    Close,
}

struct App {
    audio_editor: Option<audio::Editor>,
    lesson_playback: Option<lesson_player::Player>,
    playback_error: Option<String>,
    export_window: export_pdf::Window,
    export_settings: export_pdf::Settings,
    audio_gaps: project::AudioGaps,
    project_open: bool,
    recent_projects: recent_projects::RecentProjects,
    project_name: String,
    project_path: Option<std::path::PathBuf>,
    saved_project: project::Project,
    l1_name: String,
    lessons: Vec<Lesson>,
    lesson_recording_summaries: Option<Vec<String>>,
    active_lesson: usize,
    scroll_to_lesson: Option<usize>,
    scrollbar_offset: Option<f32>,
    focus_entry: Option<(usize, usize)>,
    scroll_to_entry: Option<(usize, usize)>,
    selected_entry: Option<(usize, usize)>,
    extra_selected: std::collections::BTreeSet<(usize, usize)>,
    selection_anchor: Option<(usize, usize)>,
    editing_entry: Option<(usize, usize)>,
    focus_name: Option<usize>,
    last_scroll_offset: f32,
    history: Commands<Command>,
    keybinds: keybinds::Registry,
    keybind_editor: keybinds::Editor,
    parsing_review: Option<parsing_review::Review>,
    project_details_open: bool,
    focus_project_name: bool,
}

impl Default for App {
    fn default() -> Self {
        let lessons = Vec::new();
        let saved_project = project::Project::new(String::new(), String::new(), lessons.clone());
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
            saved_project,
            l1_name: String::new(),
            lessons,
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
        }
    }
}

impl App {
    fn deselect_entries(&mut self) {
        self.set_selection(Vec::new(), None);
        self.lesson_playback = None;
    }

    fn toggle_lesson_playback(&mut self, lesson: usize) {
        if let Some(player) = &self.lesson_playback {
            if player.lesson == lesson {
                player.toggle();
                return;
            }
        }
        self.lesson_playback = None;
        self.playback_error = None;
        let start = self
            .selected_entry
            .filter(|(li, _)| *li == lesson)
            .map(|(_, index)| index)
            .unwrap_or(0);
        match lesson_player::Player::start(lesson, &self.lessons[lesson], start, self.audio_gaps) {
            Ok(player) => {
                self.set_selection(vec![(lesson, start)], Some((lesson, start)));
                self.active_lesson = lesson;
                self.lessons[lesson].expanded = true;
                self.scroll_to_entry = Some((lesson, start));
                self.editing_entry = None;
                self.lesson_playback = Some(player);
            }
            Err(error) => self.playback_error = Some(error),
        }
    }

    fn update_lesson_playback(&mut self, ctx: &egui::Context) {
        let Some(player) = &mut self.lesson_playback else {
            return;
        };
        if self.selected_entry != Some((player.lesson, player.selected)) {
            self.lesson_playback = None;
            return;
        }
        let lesson = player.lesson;
        let old = player.selected;
        let (index, finished, error) = player.update();
        if index != old {
            self.set_selection(vec![(lesson, index)], Some((lesson, index)));
            self.active_lesson = lesson;
            self.scroll_to_entry = Some((lesson, index));
            self.editing_entry = None;
        }
        if finished {
            self.lesson_playback = None;
            self.playback_error = error;
        } else if self
            .lesson_playback
            .as_ref()
            .is_some_and(|player| !player.is_paused())
        {
            ctx.request_repaint_after(std::time::Duration::from_millis(25));
        }
    }

    fn recording_summaries(&mut self) -> &[String] {
        self.lesson_recording_summaries.get_or_insert_with(|| {
            self.lessons
                .iter()
                .map(|lesson| lesson_recording_summary(lesson, self.audio_gaps))
                .collect()
        })
    }

    fn selection(&self) -> Vec<(usize, usize)> {
        let mut entries = self.extra_selected.clone();
        entries.extend(self.selected_entry);
        entries.into_iter().collect()
    }

    fn set_selection(&mut self, entries: Vec<(usize, usize)>, primary: Option<(usize, usize)>) {
        self.extra_selected = entries.into_iter().collect();
        self.selected_entry = primary
            .filter(|entry| self.extra_selected.contains(entry))
            .or_else(|| self.extra_selected.iter().next_back().copied());
        if let Some(entry) = self.selected_entry {
            self.extra_selected.remove(&entry);
        }
        self.selection_anchor = self.selected_entry;
    }

    fn select_entry(
        &mut self,
        target: (usize, usize),
        modifiers: egui::Modifiers,
        right_click: bool,
    ) {
        let mut selected = self
            .selection()
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>();
        if right_click && selected.contains(&target) {
            self.editing_entry = None;
            self.active_lesson = target.0;
            return;
        }
        if right_click {
            selected.clear();
            selected.insert(target);
            self.selection_anchor = Some(target);
        } else if modifiers.alt {
            selected.remove(&target);
            if self.selection_anchor == Some(target) {
                self.selection_anchor = selected.iter().next_back().copied();
            }
        } else if modifiers.shift {
            let anchor = self
                .selection_anchor
                .or(self.selected_entry)
                .unwrap_or(target);
            selected.clear();
            let (start, end) = if anchor <= target {
                (anchor, target)
            } else {
                (target, anchor)
            };
            for (lesson, item) in self.lessons.iter().enumerate() {
                for entry in 0..item.entries.len() {
                    if (start..=end).contains(&(lesson, entry)) {
                        selected.insert((lesson, entry));
                    }
                }
            }
            self.selection_anchor = Some(anchor);
        } else if modifiers.command || modifiers.ctrl || modifiers.mac_cmd {
            selected.insert(target);
            self.selection_anchor = Some(target);
        } else {
            selected.clear();
            selected.insert(target);
            self.selection_anchor = Some(target);
        }
        self.selected_entry = if selected.contains(&target) {
            Some(target)
        } else {
            selected.iter().next_back().copied()
        };
        if let Some(primary) = self.selected_entry {
            selected.remove(&primary);
        }
        self.extra_selected = selected;
        self.editing_entry = None;
        self.active_lesson = target.0;
    }

    fn selection_is_contiguous(&self) -> bool {
        let selection = self.selection();
        let (Some(first), Some(last)) = (selection.first(), selection.last()) else {
            return false;
        };
        self.lessons
            .iter()
            .enumerate()
            .flat_map(|(lesson, item)| (0..item.entries.len()).map(move |entry| (lesson, entry)))
            .filter(|position| position >= first && position <= last)
            .count()
            == selection.len()
    }

    fn remap_auxiliary_selection(
        &mut self,
        map: impl Fn((usize, usize)) -> Option<(usize, usize)>,
    ) {
        self.extra_selected = self
            .extra_selected
            .iter()
            .copied()
            .filter_map(&map)
            .collect();
        self.selection_anchor = self.selection_anchor.and_then(map);
    }

    fn project_snapshot(&self) -> project::Project {
        let mut project = project::Project::new(
            self.project_name.clone(),
            self.l1_name.clone(),
            self.lessons.clone(),
        );
        project.export_settings = self.export_settings.clone();
        project.audio_gaps = self.audio_gaps;
        project
    }

    fn is_dirty(&self) -> bool {
        self.project_open && self.project_snapshot() != self.saved_project
    }

    fn show_file_error(message: &str) {
        rfd::MessageDialog::new()
            .set_title("Project file error")
            .set_description(message)
            .set_level(rfd::MessageLevel::Error)
            .show();
    }

    fn save_project(&mut self, save_as: bool) -> bool {
        if !self.project_open {
            return false;
        }
        let path = if !save_as && let Some(path) = &self.project_path {
            path.clone()
        } else {
            let mut dialog = rfd::FileDialog::new().add_filter("Loquaris project", &["loqr"]);
            if let Some(directory) = self.dialog_directory() {
                dialog = dialog.set_directory(directory);
            }
            dialog = dialog.set_file_name(project::suggested_file_name(&self.project_name));
            let Some(path) = dialog.save_file() else {
                return false;
            };
            let (path, extension_added) = project::ensure_extension(&path);
            if extension_added {
                if path.exists()
                    && rfd::MessageDialog::new()
                        .set_title("Replace existing file?")
                        .set_description(format!("{} already exists. Replace it?", path.display()))
                        .set_level(rfd::MessageLevel::Warning)
                        .set_buttons(rfd::MessageButtons::YesNo)
                        .show()
                        != rfd::MessageDialogResult::Yes
                {
                    return false;
                }
            }
            path
        };
        let project = self.project_snapshot();
        match project.save(&path) {
            Ok(()) => {
                self.recent_projects.record(&path, &project.name);
                self.project_path = Some(path);
                self.saved_project = project;
                true
            }
            Err(error) => {
                Self::show_file_error(&error);
                false
            }
        }
    }

    fn confirm_discard_changes(&mut self) -> bool {
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

    fn replace_project(&mut self, project: project::Project, path: Option<std::path::PathBuf>) {
        self.lesson_playback = None;
        self.playback_error = None;
        self.audio_editor = None;
        self.export_window = export_pdf::Window::default();
        self.project_open = true;
        self.project_name = project.name.clone();
        self.l1_name = project.l1_name.clone();
        self.lessons = project.lessons.clone();
        self.lesson_recording_summaries = None;
        self.export_settings = project.export_settings.clone();
        self.audio_gaps = project.audio_gaps;
        self.saved_project = project;
        self.project_path = path;
        self.active_lesson = 0;
        self.selected_entry = None;
        self.extra_selected.clear();
        self.selection_anchor = None;
        self.editing_entry = None;
        self.focus_entry = None;
        self.focus_name = None;
        self.scroll_to_lesson = None;
        self.scroll_to_entry = None;
        self.scrollbar_offset = Some(0.0);
        self.last_scroll_offset = 0.0;
        self.history = Commands::new();
        self.parsing_review = None;
        self.focus_project_name = false;
    }

    fn open_project(&mut self, ctx: &egui::Context) {
        let mut dialog = rfd::FileDialog::new().add_filter("Loquaris projects", &["loqr"]);
        if let Some(directory) = self.dialog_directory() {
            dialog = dialog.set_directory(directory);
        }
        let Some(path) = dialog.pick_file() else {
            return;
        };
        self.open_path(path, ctx);
    }

    fn open_path(&mut self, path: std::path::PathBuf, ctx: &egui::Context) {
        if !self.confirm_discard_changes() {
            return;
        }
        match project::Project::load(&path) {
            Ok(project) => {
                self.recent_projects.record(&path, &project.name);
                self.replace_project(project, Some(path));
                ctx.memory_mut(|memory| {
                    if let Some(id) = memory.focused() {
                        memory.surrender_focus(id);
                    }
                });
            }
            Err(error) => Self::show_file_error(&error),
        }
    }

    fn dialog_directory(&self) -> Option<std::path::PathBuf> {
        self.project_path
            .as_ref()
            .and_then(|path| path.parent())
            .filter(|path| !path.as_os_str().is_empty())
            .map(std::path::Path::to_path_buf)
            .or_else(|| {
                directories::UserDirs::new()
                    .and_then(|dirs| dirs.document_dir().map(std::path::Path::to_path_buf))
            })
    }

    fn new_project(&mut self, ctx: &egui::Context) {
        if self.confirm_discard_changes() {
            self.replace_project(
                project::Project::new(
                    String::new(),
                    "English".into(),
                    vec![Lesson {
                        name: String::new(),
                        entries: Vec::new(),
                        expanded: true,
                    }],
                ),
                None,
            );
            self.project_details_open = true;
            self.focus_project_name = true;
            ctx.memory_mut(|memory| {
                if let Some(id) = memory.focused() {
                    memory.surrender_focus(id);
                }
            });
        }
    }

    fn close_project(&mut self, ctx: &egui::Context) {
        if !self.confirm_discard_changes() {
            return;
        }
        self.project_open = false;
        self.lesson_playback = None;
        self.playback_error = None;
        self.audio_editor = None;
        self.export_window = export_pdf::Window::default();
        self.export_settings = export_pdf::Settings::default();
        self.audio_gaps = project::AudioGaps::default();
        self.project_name.clear();
        self.l1_name.clear();
        self.saved_project = project::Project::new(String::new(), String::new(), Vec::new());
        self.project_path = None;
        self.lessons.clear();
        self.lesson_recording_summaries = None;
        self.active_lesson = 0;
        self.selected_entry = None;
        self.extra_selected.clear();
        self.selection_anchor = None;
        self.editing_entry = None;
        self.focus_entry = None;
        self.scroll_to_entry = None;
        self.focus_name = None;
        self.scroll_to_lesson = None;
        self.scrollbar_offset = None;
        self.last_scroll_offset = 0.0;
        self.history = Commands::new();
        self.parsing_review = None;
        self.project_details_open = false;
        self.focus_project_name = false;
        ctx.memory_mut(|memory| {
            if let Some(id) = memory.focused() {
                memory.surrender_focus(id);
            }
        });
        ctx.request_repaint();
    }

    fn add(&mut self, participant: Participant, language: Language) {
        self.lesson_playback = None;
        self.lesson_recording_summaries = None;
        let lesson_index = self
            .selected_entry
            .map_or(self.active_lesson, |(lesson, _)| lesson);
        let lesson = &mut self.lessons[lesson_index];
        lesson.expanded = true;
        let entry_index = self
            .selected_entry
            .map_or(lesson.entries.len(), |(_, entry)| entry + 1);
        let entry = Entry {
            audio: None,
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
        self.extra_selected.clear();
        self.selection_anchor = self.selected_entry;
        self.editing_entry = self.selected_entry;
        self.focus_entry = self.selected_entry;
    }

    fn new_lesson(&mut self) {
        self.lesson_playback = None;
        self.lesson_recording_summaries = None;
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
        self.extra_selected.clear();
        self.selection_anchor = None;
        self.editing_entry = None;
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

    fn duplicate_entries(&mut self, targets: &[(usize, usize)]) -> bool {
        if targets.is_empty() {
            return false;
        }
        let selected_before = self.selection();
        let chosen: std::collections::BTreeSet<_> = targets.iter().copied().collect();
        let before: Vec<Vec<Entry>> = self
            .lessons
            .iter()
            .map(|lesson| lesson.entries.clone())
            .collect();
        let mut after = Vec::with_capacity(before.len());
        let mut selected_after = Vec::new();
        let mut primary_after = None;
        for (lesson, entries) in before.iter().enumerate() {
            let mut values = Vec::new();
            for (index, entry) in entries.iter().enumerate() {
                values.push(entry.clone());
                if chosen.contains(&(lesson, index)) {
                    let position = (lesson, values.len());
                    values.push(entry.clone());
                    selected_after.push(position);
                    if self.selected_entry == Some((lesson, index)) {
                        primary_after = Some(position);
                    }
                }
            }
            after.push(values);
        }
        if selected_after.is_empty() {
            return false;
        }
        let command = Command::BulkEntries {
            before,
            after,
            selected_before,
            selected_after,
            primary_before: self.selected_entry,
            primary_after,
        };
        self.apply_command(command.clone(), true);
        self.history.push(command);
        if let Some((lesson, index)) = self.selected_entry {
            self.active_lesson = lesson;
            self.lessons[lesson].expanded = true;
            self.scroll_to_entry = Some((lesson, index));
        }
        true
    }

    fn bulk_action(&mut self, action: BulkAction) -> bool {
        let selected_before = self.selection();
        if selected_before.len() < 2 {
            return false;
        }
        if matches!(action, BulkAction::Duplicate) {
            return self.duplicate_entries(&selected_before);
        }
        if matches!(action, BulkAction::MoveUp | BulkAction::MoveDown)
            && !self.selection_is_contiguous()
        {
            return false;
        }
        let before: Vec<Vec<Entry>> = self
            .lessons
            .iter()
            .map(|lesson| lesson.entries.clone())
            .collect();
        let mut tagged: Vec<Vec<((usize, usize), Entry)>> = before
            .iter()
            .enumerate()
            .map(|(lesson, entries)| {
                entries
                    .iter()
                    .cloned()
                    .enumerate()
                    .map(|(entry, value)| ((lesson, entry), value))
                    .collect()
            })
            .collect();
        let chosen: std::collections::BTreeSet<_> = selected_before.iter().copied().collect();
        let first = selected_before[0];
        let last = *selected_before.last().unwrap();
        match action {
            BulkAction::Duplicate => unreachable!(),
            BulkAction::Delete => {
                for lesson in &mut tagged {
                    lesson.retain(|(original, _)| !chosen.contains(original));
                }
            }
            BulkAction::ToTeacher
            | BulkAction::ToStudent
            | BulkAction::ToL1
            | BulkAction::ToLatin
            | BulkAction::ToggleLanguage => {
                for lesson in &mut tagged {
                    for (original, entry) in lesson {
                        if !chosen.contains(original) {
                            continue;
                        }
                        match action {
                            BulkAction::ToTeacher if entry.participant == Participant::Student => {
                                entry.participant = Participant::Teacher
                            }
                            BulkAction::ToStudent if entry.participant == Participant::Teacher => {
                                entry.participant = Participant::Student;
                                entry.language = Language::Latin;
                            }
                            BulkAction::ToL1 if entry.participant == Participant::Teacher => {
                                entry.language = Language::L1
                            }
                            BulkAction::ToLatin if entry.participant == Participant::Teacher => {
                                entry.language = Language::Latin
                            }
                            BulkAction::ToggleLanguage
                                if entry.participant == Participant::Teacher =>
                            {
                                entry.language = match entry.language {
                                    Language::L1 => Language::Latin,
                                    Language::Latin => Language::L1,
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
            BulkAction::MoveUp => {
                if first.1 == 0 {
                    if first.0 == 0 {
                        return false;
                    }
                    let count = selected_before
                        .iter()
                        .take_while(|(lesson, _)| *lesson == first.0)
                        .count();
                    let moved: Vec<_> = tagged[first.0].drain(..count).collect();
                    tagged[first.0 - 1].extend(moved);
                } else {
                    let mut slots = vec![(first.0, first.1 - 1)];
                    slots.extend(selected_before.iter().copied());
                    let mut values: Vec<_> = slots
                        .iter()
                        .map(|&(lesson, index)| tagged[lesson][index].clone())
                        .collect();
                    values.rotate_left(1);
                    for ((lesson, index), value) in slots.into_iter().zip(values) {
                        tagged[lesson][index] = value;
                    }
                }
            }
            BulkAction::MoveDown => {
                if last.1 + 1 == tagged[last.0].len() {
                    if last.0 + 1 == tagged.len() {
                        return false;
                    }
                    let count = selected_before
                        .iter()
                        .rev()
                        .take_while(|(lesson, _)| *lesson == last.0)
                        .count();
                    let start = tagged[last.0].len() - count;
                    let moved: Vec<_> = tagged[last.0].drain(start..).collect();
                    tagged[last.0 + 1].splice(0..0, moved);
                } else {
                    let mut slots = selected_before.clone();
                    slots.push((last.0, last.1 + 1));
                    let mut values: Vec<_> = slots
                        .iter()
                        .map(|&(lesson, index)| tagged[lesson][index].clone())
                        .collect();
                    values.rotate_right(1);
                    for ((lesson, index), value) in slots.into_iter().zip(values) {
                        tagged[lesson][index] = value;
                    }
                }
            }
        }
        let mut selected_after = Vec::new();
        let mut primary_after = None;
        let mut after = Vec::new();
        for (lesson, entries) in tagged.into_iter().enumerate() {
            let mut values = Vec::new();
            for (index, (original, entry)) in entries.into_iter().enumerate() {
                if chosen.contains(&original) {
                    selected_after.push((lesson, index));
                    if self.selected_entry == Some(original) {
                        primary_after = Some((lesson, index));
                    }
                }
                values.push(entry);
            }
            after.push(values);
        }
        if before == after {
            return false;
        }
        let command = Command::BulkEntries {
            before,
            after,
            selected_before,
            selected_after,
            primary_before: self.selected_entry,
            primary_after,
        };
        self.apply_command(command.clone(), true);
        self.history.push(command);
        if let Some((lesson, index)) = self.selected_entry {
            self.active_lesson = lesson;
            self.lessons[lesson].expanded = true;
            self.scroll_to_entry = Some((lesson, index));
        }
        true
    }

    fn entry_action(&mut self, lesson: usize, index: usize, action: EntryAction) -> bool {
        if matches!(action, EntryAction::Duplicate) {
            return self.duplicate_entries(&[(lesson, index)]);
        }
        if matches!(action, EntryAction::Delete) {
            self.delete_entry(lesson, index);
            return true;
        }
        if matches!(action, EntryAction::AutoMacronize) {
            if self.selection().len() > 1 {
                return false;
            }
            let entry = &self.lessons[lesson].entries[index];
            if entry.language != Language::Latin {
                return false;
            }
            let review = parsing_review::Review::new(lesson, index, entry.text.clone());
            if let Some(after) = review.unambiguous_preview() {
                if review.original == after {
                    return false;
                }
                self.apply_review(&review, after);
            } else {
                self.parsing_review = Some(review);
            }
            return true;
        }
        let entry = &self.lessons[lesson].entries[index];
        let command = match action {
            EntryAction::MoveParticipant => {
                let after = match entry.participant {
                    Participant::Teacher => (Participant::Student, Language::Latin),
                    Participant::Student => (Participant::Teacher, Language::Latin),
                };
                Command::SetEntryKind {
                    lesson,
                    index,
                    before: (entry.participant, entry.language),
                    after,
                }
            }
            EntryAction::SwitchLanguage => {
                if entry.participant != Participant::Teacher {
                    return false;
                }
                let language = match entry.language {
                    Language::L1 => Language::Latin,
                    Language::Latin => Language::L1,
                };
                Command::SetEntryKind {
                    lesson,
                    index,
                    before: (entry.participant, entry.language),
                    after: (entry.participant, language),
                }
            }
            EntryAction::MoveUp => {
                let to = if index > 0 {
                    (lesson, index - 1)
                } else if lesson > 0 {
                    (lesson - 1, self.lessons[lesson - 1].entries.len())
                } else {
                    return false;
                };
                Command::MoveEntry {
                    from: (lesson, index),
                    to,
                }
            }
            EntryAction::MoveDown => {
                let to = if index + 1 < self.lessons[lesson].entries.len() {
                    (lesson, index + 1)
                } else if lesson + 1 < self.lessons.len() {
                    (lesson + 1, 0)
                } else {
                    return false;
                };
                Command::MoveEntry {
                    from: (lesson, index),
                    to,
                }
            }
            EntryAction::Delete | EntryAction::AutoMacronize | EntryAction::Duplicate => {
                unreachable!()
            }
        };
        self.apply_command(command.clone(), true);
        self.history.push(command);
        if let Some((lesson, index)) = self.selected_entry {
            self.active_lesson = lesson;
            self.lessons[lesson].expanded = true;
            self.scroll_to_entry = Some((lesson, index));
        }
        true
    }

    fn selected_entry_shortcut(&mut self, action: keybinds::Action) -> bool {
        if self.selection().len() > 1 {
            let bulk = match action {
                keybinds::Action::SwitchLanguage => BulkAction::ToggleLanguage,
                keybinds::Action::MoveToStudent => BulkAction::ToStudent,
                keybinds::Action::MoveToTeacher => BulkAction::ToTeacher,
                keybinds::Action::MoveEntryUp => BulkAction::MoveUp,
                keybinds::Action::MoveEntryDown => BulkAction::MoveDown,
                keybinds::Action::DuplicateEntry => BulkAction::Duplicate,
                _ => return false,
            };
            return self.bulk_action(bulk);
        }
        let Some((lesson, index)) = self.selected_entry else {
            return false;
        };
        let participant = self.lessons[lesson].entries[index].participant;
        let action = match action {
            keybinds::Action::DuplicateEntry => EntryAction::Duplicate,
            keybinds::Action::SwitchLanguage if participant == Participant::Teacher => {
                EntryAction::SwitchLanguage
            }
            keybinds::Action::MoveToStudent if participant == Participant::Teacher => {
                EntryAction::MoveParticipant
            }
            keybinds::Action::MoveToTeacher if participant == Participant::Student => {
                EntryAction::MoveParticipant
            }
            keybinds::Action::MoveEntryUp | keybinds::Action::MoveEntryDown => {
                if self
                    .lessons
                    .iter()
                    .map(|lesson| lesson.entries.len())
                    .sum::<usize>()
                    <= 1
                {
                    return false;
                }
                if action == keybinds::Action::MoveEntryUp {
                    EntryAction::MoveUp
                } else {
                    EntryAction::MoveDown
                }
            }
            _ => return false,
        };
        self.entry_action(lesson, index, action)
    }

    fn select_adjacent(&mut self, next: bool) {
        let entries: Vec<_> = self
            .lessons
            .iter()
            .enumerate()
            .flat_map(|(lesson, item)| (0..item.entries.len()).map(move |entry| (lesson, entry)))
            .collect();
        if entries.is_empty() {
            return;
        }
        let target = if let Some(selected) = self.selected_entry {
            let index = entries
                .iter()
                .position(|entry| *entry == selected)
                .unwrap_or(0);
            entries[if next {
                (index + 1).min(entries.len() - 1)
            } else {
                index.saturating_sub(1)
            }]
        } else {
            let matching = entries
                .iter()
                .copied()
                .filter(|(lesson, _)| *lesson == self.active_lesson);
            if next {
                matching.into_iter().next().unwrap_or(entries[0])
            } else {
                matching
                    .into_iter()
                    .last()
                    .unwrap_or(*entries.last().unwrap())
            }
        };
        self.selected_entry = Some(target);
        self.extra_selected.clear();
        self.selection_anchor = Some(target);
        self.editing_entry = None;
        self.active_lesson = target.0;
        self.lessons[target.0].expanded = true;
        self.focus_entry = None;
        self.scroll_to_entry = Some(target);
    }

    fn edit_selected(&mut self) {
        if let Some((lesson, entry)) = self.selected_entry {
            self.extra_selected.clear();
            self.selection_anchor = Some((lesson, entry));
            self.active_lesson = lesson;
            self.lessons[lesson].expanded = true;
            self.editing_entry = Some((lesson, entry));
            self.focus_entry = Some((lesson, entry));
        }
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

    fn apply_review(&mut self, review: &parsing_review::Review, after: String) {
        if let Some(entry) = self
            .lessons
            .get_mut(review.lesson)
            .and_then(|lesson| lesson.entries.get_mut(review.entry))
            && entry.language == Language::Latin
            && entry.text == review.original
            && entry.text != after
        {
            let before = std::mem::replace(&mut entry.text, after.clone());
            self.record_text(
                TextTarget::Entry(review.lesson, review.entry),
                before,
                after,
            );
        }
    }

    fn undo(&mut self) {
        if self
            .audio_editor
            .as_mut()
            .is_some_and(|editor| editor.block_history_while_recording())
        {
            return;
        }
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
        if self
            .audio_editor
            .as_mut()
            .is_some_and(|editor| editor.block_history_while_recording())
        {
            return;
        }
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
        if matches!(
            &command,
            Command::SetAudio { .. }
                | Command::SetAudioGaps { .. }
                | Command::SetEntryKind { .. }
                | Command::InsertLesson { .. }
                | Command::InsertEntry { .. }
                | Command::DeleteEntry { .. }
                | Command::MoveEntry { .. }
                | Command::BulkEntries { .. }
        ) {
            self.lesson_playback = None;
        }
        if matches!(
            &command,
            Command::SetAudio { .. }
                | Command::SetAudioGaps { .. }
                | Command::SetEntryKind { .. }
                | Command::InsertLesson { .. }
                | Command::InsertEntry { .. }
                | Command::DeleteEntry { .. }
                | Command::MoveEntry { .. }
                | Command::BulkEntries { .. }
        ) {
            self.lesson_recording_summaries = None;
        }
        if matches!(
            &command,
            Command::InsertLesson { .. }
                | Command::InsertEntry { .. }
                | Command::DeleteEntry { .. }
                | Command::MoveEntry { .. }
                | Command::BulkEntries { .. }
        ) {
            self.audio_editor = None;
        }
        if !matches!(&command, Command::EditText { .. }) {
            self.editing_entry = None;
        }
        match command {
            Command::SetAudioGaps { before, after } => {
                self.audio_gaps = if forward { after } else { before };
            }
            Command::SetExportSettings { before, after } => {
                self.export_settings = if forward { after } else { before };
            }
            Command::SetAudio {
                lesson,
                index,
                before,
                after,
            } => {
                self.lessons[lesson].entries[index].audio = if forward { after } else { before };
            }
            Command::EditText {
                target,
                before,
                after,
            } => {
                let value = if forward { after } else { before };
                match target {
                    TextTarget::ProjectName => self.project_name = value,
                    TextTarget::L1Name => self.l1_name = value,
                    TextTarget::LessonName(index) => self.lessons[index].name = value,
                    TextTarget::Entry(lesson, entry) => {
                        self.lessons[lesson].entries[entry].text = value
                    }
                }
            }
            Command::InsertLesson { index, lesson } => {
                self.remap_auxiliary_selection(|(lesson, entry)| {
                    if !forward && lesson == index {
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
                    }
                });
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
                self.remap_auxiliary_selection(|(selected_lesson, selected_index)| {
                    if selected_lesson != lesson {
                        return Some((selected_lesson, selected_index));
                    }
                    if !forward && selected_index == index {
                        return None;
                    }
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
                });
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
            Command::SetEntryKind {
                lesson,
                index,
                before,
                after,
            } => {
                let (participant, language) = if forward { after } else { before };
                let entry = &mut self.lessons[lesson].entries[index];
                entry.participant = participant;
                entry.language = language;
            }
            Command::MoveEntry { from, to } => {
                let (from, to) = if forward { (from, to) } else { (to, from) };
                self.remap_auxiliary_selection(|(lesson, index)| {
                    if (lesson, index) == from {
                        return Some(to);
                    }
                    let index = if lesson == from.0 && index > from.1 {
                        index - 1
                    } else {
                        index
                    };
                    let index = if lesson == to.0 && index >= to.1 {
                        index + 1
                    } else {
                        index
                    };
                    Some((lesson, index))
                });
                let entry = self.lessons[from.0].entries.remove(from.1);
                self.lessons[to.0].entries.insert(to.1, entry);
                if let Some((lesson, index)) = self.selected_entry {
                    self.selected_entry = if (lesson, index) == from {
                        Some(to)
                    } else {
                        let index = if lesson == from.0 && index > from.1 {
                            index - 1
                        } else {
                            index
                        };
                        let index = if lesson == to.0 && index >= to.1 {
                            index + 1
                        } else {
                            index
                        };
                        Some((lesson, index))
                    };
                }
            }
            Command::BulkEntries {
                before,
                after,
                selected_before,
                selected_after,
                primary_before,
                primary_after,
            } => {
                let (entries, selection, primary) = if forward {
                    (after, selected_after, primary_after)
                } else {
                    (before, selected_before, primary_before)
                };
                for (lesson, entries) in self.lessons.iter_mut().zip(entries) {
                    lesson.entries = entries;
                }
                self.set_selection(selection, primary);
            }
        }
        if let Some(editor) = &mut self.audio_editor {
            if let Some(entry) = self
                .lessons
                .get(editor.target.0)
                .and_then(|lesson| lesson.entries.get(editor.target.1))
            {
                editor.sync_entry(entry);
            }
        }
        if self.selected_entry.is_none() {
            self.selected_entry = self.extra_selected.pop_last();
        }
        if self.selection_anchor.is_none() {
            self.selection_anchor = self.selected_entry;
        }
        self.active_lesson = self.active_lesson.min(self.lessons.len().saturating_sub(1));
        self.focus_entry = None;
        self.scroll_to_entry = None;
        self.focus_name = None;
        self.scroll_to_lesson = None;
    }
}

impl App {
    fn welcome(&mut self, ui: &mut egui::Ui) {
        let mut file_action = self.handle_shortcuts(ui);
        self.keybind_editor.show(ui.ctx(), &mut self.keybinds);
        ui.visuals_mut().override_text_color = Some(Color32::WHITE);
        ui.visuals_mut().panel_fill = Color32::BLACK;
        egui::MenuBar::new().ui(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 12.0;
            ui.spacing_mut().button_padding = Vec2::new(12.0, 6.0);
            ui.spacing_mut().interact_size.y = 32.0;
            ui.menu_button("File", |ui| {
                if ui.button("New project").clicked() {
                    ui.close();
                    file_action = Some(FileAction::New);
                }
                if ui.button("Open…").clicked() {
                    ui.close();
                    file_action = Some(FileAction::Open);
                }
            });
            ui.menu_button("Edit", |ui| {
                if ui.button("Keybinds…").clicked() {
                    ui.close();
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
            if let Some(error) = &self.recent_projects.storage_error {
                ui.colored_label(Color32::LIGHT_RED, error);
            }
            if let Some(error) = &self.keybinds.storage_error {
                ui.colored_label(Color32::LIGHT_RED, error);
            }
            if self.recent_projects.projects.is_empty() {
                ui.label("No recent projects yet.");
            } else {
                egui::ScrollArea::vertical()
                    .id_salt("recent_projects")
                    .show(ui, |ui| {
                        for project in &self.recent_projects.projects {
                            egui::Frame::group(ui.style()).show(ui, |ui| {
                                ui.set_min_width((ui.available_width() - 16.0).max(0.0));
                                ui.horizontal(|ui| {
                                    let name = if project.name.is_empty() {
                                        "Untitled"
                                    } else {
                                        &project.name
                                    };
                                    if ui.button(name).clicked() {
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
            }
        });
        if let Some(path) = forget_recent {
            self.recent_projects.forget(&path);
        }
        if let Some(path) = open_recent {
            self.open_path(path, ui.ctx());
        }
        if let Some(action) = file_action {
            match action {
                FileAction::New => self.new_project(ui.ctx()),
                FileAction::Open => self.open_project(ui.ctx()),
                FileAction::Save | FileAction::SaveAs | FileAction::Close => {}
            }
        }
        if self.project_open {
            ui.ctx().request_repaint();
        }
    }

    fn show_project_details(&mut self, ctx: &egui::Context) -> Option<FileAction> {
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
                            .show(ui, |ui| {
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
                                for (label, milliseconds) in [
                                    (
                                        "Same participant gap",
                                        &mut self.audio_gaps.same_participant_ms,
                                    ),
                                    (
                                        "Different participant gap",
                                        &mut self.audio_gaps.different_participant_ms,
                                    ),
                                ] {
                                    ui.add_sized([column_width, 28.0], egui::Label::new(label));
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
                                    self.history.push(Command::SetAudioGaps {
                                        before,
                                        after: self.audio_gaps,
                                    });
                                    self.lesson_recording_summaries = None;
                                    ui.ctx().request_repaint_of(egui::ViewportId::ROOT);
                                }

                                ui.add_sized(
                                    [column_width, 28.0],
                                    egui::Label::new("Project name"),
                                );
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
                                    self.record_text(
                                        TextTarget::ProjectName,
                                        before,
                                        self.project_name.clone(),
                                    );
                                    ui.ctx().request_repaint_of(egui::ViewportId::ROOT);
                                }
                                ui.end_row();

                                ui.add_sized([column_width, 28.0], egui::Label::new("L1 language"));
                                let before = self.l1_name.clone();
                                if ui
                                    .add_sized(
                                        [column_width, 28.0],
                                        egui::TextEdit::singleline(&mut self.l1_name),
                                    )
                                    .changed()
                                {
                                    self.record_text(
                                        TextTarget::L1Name,
                                        before,
                                        self.l1_name.clone(),
                                    );
                                    ui.ctx().request_repaint_of(egui::ViewportId::ROOT);
                                }
                                ui.end_row();
                            });
                    });
                });
            },
        );
        file_action
    }

    fn handle_shortcuts(&mut self, ui: &mut egui::Ui) -> Option<FileAction> {
        let mut file_action = None;
        if !self.keybind_editor.is_recording()
            && self.parsing_review.is_none()
            && self.audio_editor.is_none()
        {
            let actions = ui.input_mut(|input| {
                self.keybinds
                    .take_actions(input, self.editing_entry.is_some())
            });
            if !actions.is_empty() {
                ui.ctx().request_repaint_of(egui::ViewportId::ROOT);
            }
            for action in actions {
                if !self.project_open && action != keybinds::Action::OpenProject {
                    continue;
                }
                match action {
                    keybinds::Action::Undo => self.undo(),
                    keybinds::Action::Redo | keybinds::Action::RedoAlternative => self.redo(),
                    keybinds::Action::OpenProject => file_action = Some(FileAction::Open),
                    keybinds::Action::SaveProject => file_action = Some(FileAction::Save),
                    keybinds::Action::SaveProjectAs => file_action = Some(FileAction::SaveAs),
                    keybinds::Action::EditEntry => self.edit_selected(),
                    keybinds::Action::StopEditing => {
                        self.editing_entry = None;
                        self.focus_entry = None;
                        ui.ctx().memory_mut(|memory| {
                            if let Some(id) = memory.focused() {
                                memory.surrender_focus(id);
                            }
                        });
                    }
                    keybinds::Action::DeselectEntries => {
                        self.deselect_entries();
                    }
                    keybinds::Action::SelectPrevious | keybinds::Action::SelectNext => {
                        self.select_adjacent(action == keybinds::Action::SelectNext);
                        ui.ctx().memory_mut(|memory| {
                            if let Some(id) = memory.focused() {
                                memory.surrender_focus(id);
                            }
                        });
                    }
                    keybinds::Action::AddTeacherL1 => self.add(Participant::Teacher, Language::L1),
                    keybinds::Action::AddTeacherLatin => {
                        self.add(Participant::Teacher, Language::Latin)
                    }
                    keybinds::Action::AddStudentLatin => {
                        self.add(Participant::Student, Language::Latin)
                    }
                    keybinds::Action::SwitchLanguage
                    | keybinds::Action::MoveToStudent
                    | keybinds::Action::MoveToTeacher
                    | keybinds::Action::MoveEntryUp
                    | keybinds::Action::MoveEntryDown
                    | keybinds::Action::DuplicateEntry => {
                        if self.selected_entry_shortcut(action) {
                            ui.ctx().memory_mut(|memory| {
                                if let Some(id) = memory.focused() {
                                    memory.surrender_focus(id);
                                }
                            });
                        }
                    }
                    keybinds::Action::RecorderPlayPause
                    | keybinds::Action::RecorderRecord
                    | keybinds::Action::RecorderStopPlayback
                    | keybinds::Action::RecorderSeekBack
                    | keybinds::Action::RecorderSeekForward
                    | keybinds::Action::RecorderSeekStart
                    | keybinds::Action::RecorderSeekEnd => {
                        unreachable!("recorder shortcuts are scoped to the recorder")
                    }
                }
            }
        }
        file_action
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if !self.project_open {
            self.welcome(ui);
            return;
        }
        self.update_lesson_playback(ui.ctx());
        let mut file_action = self.handle_shortcuts(ui);
        self.keybind_editor.show(ui.ctx(), &mut self.keybinds);
        ui.visuals_mut().override_text_color = Some(Color32::WHITE);
        ui.visuals_mut().panel_fill = Color32::BLACK;
        ui.spacing_mut().item_spacing = Vec2::new(0.0, 0.0);
        let width = ui.available_width();
        let half = width / 2.0;
        let background_area = ui.max_rect();
        let mut foreground_rects = Vec::new();

        egui::MenuBar::new().ui(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 12.0;
            ui.spacing_mut().button_padding = Vec2::new(12.0, 6.0);
            ui.spacing_mut().interact_size.y = 32.0;
            let file_menu = ui.menu_button("File", |ui| {
                if ui.button("New project").clicked() {
                    ui.close();
                    file_action = Some(FileAction::New);
                }
                if ui.button("Open…").clicked() {
                    ui.close();
                    file_action = Some(FileAction::Open);
                }
                if ui.button("Save").clicked() {
                    ui.close();
                    file_action = Some(FileAction::Save);
                }
                if ui.button("Save as…").clicked() {
                    ui.close();
                    file_action = Some(FileAction::SaveAs);
                }
                if ui.button("Export…").clicked() {
                    ui.close();
                    self.export_window.open = true;
                }
                if ui.button("Close project").clicked() {
                    ui.close();
                    file_action = Some(FileAction::Close);
                }
            });
            foreground_rects.push(file_menu.response.rect);
            let edit_menu = ui.menu_button("Edit", |ui| {
                if ui.button("Project details…").clicked() {
                    ui.close();
                    self.project_details_open = true;
                }
                if ui.button("Keybinds…").clicked() {
                    ui.close();
                    self.keybind_editor.open = true;
                }
            });
            foreground_rects.push(edit_menu.response.rect);
            if self.is_dirty() {
                foreground_rects.push(ui.label("  • Unsaved changes").rect);
            }
            if self.keybinds.storage_error.is_some() {
                foreground_rects.push(
                    ui.colored_label(Color32::LIGHT_RED, "  • Keybind settings error")
                        .on_hover_text("Open Edit → Keybinds for details.")
                        .rect,
                );
            }
            if let Some(error) = &self.recent_projects.storage_error {
                foreground_rects.push(
                    ui.colored_label(Color32::LIGHT_RED, "Recent projects error")
                        .on_hover_text(error)
                        .rect,
                );
            }
        });
        if let Some(error) = &self.playback_error {
            foreground_rects.push(ui.colored_label(Color32::LIGHT_RED, error).rect);
        }
        ui.allocate_ui_with_layout(
            Vec2::new(width, 26.0),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                let button = ui.button("+ New lesson");
                foreground_rects.push(button.rect);
                if button.clicked() {
                    self.new_lesson();
                }
                let button = ui.button("Undo");
                foreground_rects.push(button.rect);
                if button.clicked() {
                    self.undo();
                }
                let button = ui.button("Redo");
                foreground_rects.push(button.rect);
                if button.clicked() {
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
                    foreground_rects
                        .push(ui.label(RichText::new("Teacher").size(30.0).strong()).rect);
                },
            );
            ui.allocate_ui_with_layout(
                Vec2::new(half, 32.0),
                egui::Layout::right_to_left(egui::Align::Center),
                |ui| {
                    foreground_rects.push(
                        ui.label(
                            RichText::new("Student")
                                .font(entry_font(Language::Latin, 30.0))
                                .strong(),
                        )
                        .rect,
                    );
                },
            );
        });
        foreground_rects.push(ui.separator().rect);
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
        let mut pending_entry_action = None;
        let mut pending_edit = None;
        let mut pending_audio = None;
        let mut pending_playback = None;
        let mut pending_selection = None;
        let lesson_count = self.lessons.len();
        let summaries = self.recording_summaries().to_vec();
        let multiple_entries = self
            .lessons
            .iter()
            .map(|lesson| lesson.entries.len())
            .sum::<usize>()
            > 1;
        let selection = self.selection();
        let multiselect = selection.len() > 1;
        let contiguous = self.selection_is_contiguous();
        let first = selection.first().copied();
        let last = selection.last().copied();
        let can_group_up = first.is_some_and(|(lesson, index)| index > 0 || lesson > 0);
        let can_group_down = last.is_some_and(|(lesson, index)| {
            index + 1 < self.lessons[lesson].entries.len() || lesson + 1 < lesson_count
        });
        let mut has_teacher = false;
        let mut has_student = false;
        let mut has_teacher_l1 = false;
        let mut has_teacher_latin = false;
        for &(lesson, index) in &selection {
            let entry = &self.lessons[lesson].entries[index];
            match (entry.participant, entry.language) {
                (Participant::Teacher, Language::L1) => {
                    has_teacher = true;
                    has_teacher_l1 = true;
                }
                (Participant::Teacher, Language::Latin) => {
                    has_teacher = true;
                    has_teacher_latin = true;
                }
                (Participant::Student, _) => has_student = true,
            }
        }
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
                        &summaries[lesson_index],
                        self.lesson_playback
                            .as_ref()
                            .filter(|player| player.lesson == lesson_index)
                            .map(|player| !player.is_paused()),
                        self.audio_editor.is_none() && !lesson.entries.is_empty(),
                    );
                    foreground_rects.push(header.response.rect.intersect(viewport));
                    if header.play_clicked {
                        pending_playback = Some(lesson_index);
                    }
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
                        self.extra_selected.clear();
                        self.selection_anchor = None;
                        self.editing_entry = None;
                    }
                    if self.scroll_to_lesson == Some(lesson_index) {
                        header.response.scroll_to_me(Some(egui::Align::BOTTOM));
                    }
                    if lesson.expanded {
                        let entry_count = lesson.entries.len();
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
                                                                let (card_margin, card_width) =
                                                                    entry_column_geometry(
                                                                        column_width,
                                                                        entry.participant,
                                                                    );
                                                                ui.add_space(card_margin);
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
                                                                    card_width,
                                                                    focus,
                                                                    self.selected_entry
                                                                        == Some((
                                                                            lesson_index,
                                                                            entry_index,
                                                                        ))
                                                                        || self
                                                                            .extra_selected
                                                                            .contains(&(
                                                                                lesson_index,
                                                                                entry_index,
                                                                            )),
                                                                    self.editing_entry
                                                                        == Some((
                                                                            lesson_index,
                                                                            entry_index,
                                                                        )),
                                                                    EntryMenuOptions {
                                                                        show_move: multiple_entries,
                                                                        can_move_up: entry_index
                                                                            > 0
                                                                            || lesson_index > 0,
                                                                        can_move_down: entry_index
                                                                            + 1
                                                                            < entry_count
                                                                            || lesson_index + 1
                                                                                < lesson_count,
                                                                        selected: selection
                                                                            .contains(&(
                                                                                lesson_index,
                                                                                entry_index,
                                                                            )),
                                                                        multiselect,
                                                                        contiguous,
                                                                        can_group_up,
                                                                        can_group_down,
                                                                        has_teacher,
                                                                        has_student,
                                                                        has_teacher_l1,
                                                                        has_teacher_latin,
                                                                    },
                                                                );
                                                                foreground_rects.push(
                                                                    response
                                                                        .bounds
                                                                        .expand(8.0)
                                                                        .intersect(viewport),
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
                                                                if response.requests_selection(
                                                                    self.editing_entry
                                                                        == Some((
                                                                            lesson_index,
                                                                            entry_index,
                                                                        )),
                                                                ) && !focus
                                                                {
                                                                    pending_selection = Some((
                                                                        (lesson_index, entry_index),
                                                                        ui.input(|input| {
                                                                            input.modifiers
                                                                        }),
                                                                        response.right_clicked,
                                                                    ));
                                                                }
                                                                if self.editing_entry
                                                                    == Some((
                                                                        lesson_index,
                                                                        entry_index,
                                                                    ))
                                                                    && response.text.lost_focus()
                                                                    && !focus
                                                                {
                                                                    self.editing_entry = None;
                                                                }
                                                                if let Some(action) =
                                                                    response.action
                                                                {
                                                                    pending_entry_action = Some((
                                                                        lesson_index,
                                                                        entry_index,
                                                                        action,
                                                                    ));
                                                                }
                                                                if response.edit_requested {
                                                                    pending_edit = Some((
                                                                        lesson_index,
                                                                        entry_index,
                                                                    ));
                                                                }
                                                                if response.audio_requested {
                                                                    pending_audio = Some((
                                                                        lesson_index,
                                                                        entry_index,
                                                                    ));
                                                                }
                                                                if focus
                                                                    || self.scroll_to_entry
                                                                        == Some((
                                                                            lesson_index,
                                                                            entry_index,
                                                                        ))
                                                                {
                                                                    response
                                                                        .text
                                                                        .scroll_to_me(None);
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
                        let footer = lesson_footer(ui, lesson_index, &lesson.name, width);
                        foreground_rects.push(footer.rect.intersect(viewport));
                        if footer.clicked() {
                            lesson.expanded = false;
                            if self
                                .editing_entry
                                .is_some_and(|(lesson, _)| lesson == lesson_index)
                            {
                                self.editing_entry = None;
                            }
                            selected = Some(lesson_index);
                        }
                    }
                    lesson_bounds.push((lesson_index, top, ui.next_widget_position().y));
                });
                if lesson_index + 1 < lesson_count {
                    ui.add_space(12.0);
                }
            }
            self.focus_entry = None;
            self.scroll_to_entry = None;
            self.focus_name = None;
            self.scroll_to_lesson = None;
        });

        if let Some((position, modifiers, right_click)) = pending_selection {
            self.select_entry(position, modifiers, right_click);
            selected = Some(position.0);
        }
        if let Some((lesson, index)) = pending_edit {
            self.selected_entry = Some((lesson, index));
            self.edit_selected();
            selected = Some(lesson);
        }
        if let Some((lesson, index)) = pending_audio {
            self.lesson_playback = None;
            self.audio_editor = Some(audio::Editor::new(
                (lesson, index),
                self.lessons[lesson].entries[index].clone(),
            ));
        }

        if let Some((lesson, index, action)) = pending_entry_action {
            match action {
                MenuAction::Single(action) => {
                    self.entry_action(lesson, index, action);
                }
                MenuAction::Bulk(action) => {
                    self.bulk_action(action);
                }
            }
            selected = self.selected_entry.map(|(lesson, _)| lesson).or(selected);
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
                foreground_rects.push(rect);
                let mut sticky_ui = ui.new_child(egui::UiBuilder::new().max_rect(rect));
                {
                    let ui = &mut sticky_ui;
                    ui.push_id(("sticky", index), |ui| {
                        let before = self.lessons[index].name.clone();
                        let can_play =
                            self.audio_editor.is_none() && !self.lessons[index].entries.is_empty();
                        let header = lesson_header(
                            ui,
                            &mut self.lessons[index],
                            index,
                            width,
                            false,
                            true,
                            &summaries[index],
                            self.lesson_playback
                                .as_ref()
                                .filter(|player| player.lesson == index)
                                .map(|player| !player.is_paused()),
                            can_play,
                        );
                        if header.play_clicked {
                            pending_playback = Some(index);
                        }
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
                            self.extra_selected.clear();
                            self.selection_anchor = None;
                            self.editing_entry = None;
                        }
                    });
                }
            }
        }

        if let Some(index) = pending_playback {
            self.toggle_lesson_playback(index);
            ui.ctx().request_repaint();
        }

        // A single scrollbar in the gutter controls the shared chronological timeline.
        let track = egui::Rect::from_center_size(
            egui::pos2(center, viewport.center().y),
            Vec2::new(14.0, height),
        );
        foreground_rects.push(track);
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
        foreground_rects.push(ui.separator().rect);
        let target_lesson = self
            .selected_entry
            .map_or(self.active_lesson, |(lesson, _)| lesson);
        let active = &self.lessons[target_lesson];
        foreground_rects.push(
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
                .color(Color32::LIGHT_GRAY),
            )
            .rect,
        );
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.allocate_ui_with_layout(
                Vec2::new(half, 35.0),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.add_space(10.0);
                    let button = ui.add(
                        egui::Button::new(
                            RichText::new(format!("+ {}", self.l1_name))
                                .color(L1_ACCENT)
                                .strong(),
                        )
                        .fill(L1_COLOR),
                    );
                    foreground_rects.push(button.rect);
                    if button.clicked() {
                        self.add(Participant::Teacher, Language::L1);
                    }
                    ui.add_space(12.0);
                    let button = ui.add(
                        egui::Button::new(RichText::new("+ Latin").color(LATIN_ACCENT).strong())
                            .fill(LATIN_COLOR),
                    );
                    foreground_rects.push(button.rect);
                    if button.clicked() {
                        self.add(Participant::Teacher, Language::Latin);
                    }
                },
            );
            ui.allocate_ui_with_layout(
                Vec2::new(half, 35.0),
                egui::Layout::right_to_left(egui::Align::Center),
                |ui| {
                    ui.add_space(10.0);
                    let button = ui.add(
                        egui::Button::new(RichText::new("+ Latin").color(LATIN_ACCENT).strong())
                            .fill(LATIN_COLOR),
                    );
                    foreground_rects.push(button.rect);
                    if button.clicked() {
                        self.add(Participant::Student, Language::Latin);
                    }
                },
            );
        });
        if background_double_clicked(ui.ctx(), ui.layer_id(), background_area, &foreground_rects) {
            self.deselect_entries();
            self.editing_entry = None;
            self.focus_entry = None;
        }
        if let Some(mut review) = self.parsing_review.take() {
            match review.show(ui.ctx()) {
                Some(parsing_review::Outcome::Apply(after)) => self.apply_review(&review, after),
                Some(parsing_review::Outcome::Cancel) => {}
                None => self.parsing_review = Some(review),
            }
        }
        if let Some(mut editor) = self.audio_editor.take() {
            let outcome = editor.show(ui.ctx(), &self.l1_name, &self.keybinds);
            let mut valid = true;
            if let Some(after) = outcome.changed {
                let (lesson, index) = editor.target;
                if let Some(entry) = self
                    .lessons
                    .get(lesson)
                    .and_then(|lesson| lesson.entries.get(index))
                {
                    if *entry == editor.original && entry.audio != after {
                        let command = Command::SetAudio {
                            lesson,
                            index,
                            before: entry.audio.clone(),
                            after,
                        };
                        self.apply_command(command.clone(), true);
                        self.history.push(command);
                        editor.original.audio = self.lessons[lesson].entries[index].audio.clone();
                    } else if *entry != editor.original && editor.original.audio != after {
                        Self::show_file_error(
                            "The entry changed while the audio window was open. The clip was not applied.",
                        );
                        valid = false;
                    }
                } else {
                    valid = false;
                }
            }
            if !outcome.closed && valid {
                self.audio_editor = Some(editor);
            }
            if valid {
                for action in outcome.actions {
                    match action {
                        keybinds::Action::Undo => self.undo(),
                        keybinds::Action::Redo | keybinds::Action::RedoAlternative => self.redo(),
                        keybinds::Action::SaveProject => file_action = Some(FileAction::Save),
                        keybinds::Action::SaveProjectAs => file_action = Some(FileAction::SaveAs),
                        _ => unreachable!("only project actions leave the recorder"),
                    }
                    ui.ctx().request_repaint_of(egui::ViewportId::ROOT);
                    ui.ctx()
                        .request_repaint_of(egui::ViewportId::from_hash_of("entry-audio"));
                }
            }
        }
        if let Some(action) = self.show_project_details(ui.ctx()) {
            file_action = Some(action);
        }
        let export_directory = self.dialog_directory();
        let before_export = self.export_settings.clone();
        let export_actions = self.export_window.show(
            ui.ctx(),
            &self.project_name,
            &self.l1_name,
            &self.lessons,
            &mut self.export_settings,
            &self.keybinds,
            export_directory.as_deref(),
            self.project_path.as_deref(),
            self.audio_gaps,
            self.audio_editor
                .as_ref()
                .is_some_and(|editor| editor.is_recording()),
        );
        if self.export_settings != before_export {
            self.history.push(Command::SetExportSettings {
                before: before_export,
                after: self.export_settings.clone(),
            });
            ui.ctx().request_repaint_of(egui::ViewportId::ROOT);
        }
        for action in export_actions {
            match action {
                keybinds::Action::Undo => self.undo(),
                keybinds::Action::Redo | keybinds::Action::RedoAlternative => self.redo(),
                _ => unreachable!("only history actions are dispatched in the export window"),
            }
            ui.ctx().request_repaint_of(egui::ViewportId::ROOT);
            ui.ctx()
                .request_repaint_of(egui::ViewportId::from_hash_of("export-pdf"));
        }
        if let Some(action) = file_action {
            match action {
                FileAction::New => self.new_project(ui.ctx()),
                FileAction::Open => self.open_project(ui.ctx()),
                FileAction::Save => {
                    self.save_project(false);
                }
                FileAction::SaveAs => {
                    self.save_project(true);
                }
                FileAction::Close => self.close_project(ui.ctx()),
            }
        }
        if ui.input(|input| input.viewport().close_requested()) && !self.confirm_discard_changes() {
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::CancelClose);
        }
    }
}

fn background_double_clicked(
    ctx: &egui::Context,
    layer: egui::LayerId,
    viewport: egui::Rect,
    foreground: &[egui::Rect],
) -> bool {
    if egui::Popup::is_any_open(ctx) {
        return false;
    }
    let position = ctx.input(|input| {
        input
            .pointer
            .button_double_clicked(egui::PointerButton::Primary)
            .then(|| input.pointer.interact_pos())
            .flatten()
    });
    position.is_some_and(|position| {
        viewport.contains(position)
            && ctx.layer_id_at(position) == Some(layer)
            && !foreground.iter().any(|rect| rect.contains(position))
    })
}

struct HeaderResponse {
    response: egui::Response,
    clicked: bool,
    play_clicked: bool,
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

fn lesson_header(
    ui: &mut egui::Ui,
    lesson: &mut Lesson,
    index: usize,
    width: f32,
    focus: bool,
    sticky: bool,
    recording_summary: &str,
    playback: Option<bool>,
    play_enabled: bool,
) -> HeaderResponse {
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
                    play_clicked = ui
                        .add_enabled(
                            play_enabled,
                            egui::Button::new(if playback == Some(true) {
                                "❚❚"
                            } else {
                                "▶"
                            })
                            .min_size(Vec2::new(button_width, 24.0)),
                        )
                        .on_hover_text(if playback == Some(true) {
                            "Pause lesson"
                        } else if playback == Some(false) {
                            "Resume lesson"
                        } else {
                            "Play lesson"
                        })
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
    paint_chevron(
        painter,
        egui::pos2(rect.right() - 14.0, rect.center().y),
        Chevron::Up,
        Color32::WHITE,
    );
    response
}

struct CardResponse {
    text: egui::Response,
    bounds: egui::Rect,
    clicked: bool,
    right_clicked: bool,
    edit_requested: bool,
    audio_requested: bool,
    action: Option<MenuAction>,
}

impl CardResponse {
    fn requests_selection(&self, editing: bool) -> bool {
        self.clicked
            && (self.right_clicked
                || !editing
                || !(self.text.clicked() || self.text.drag_started() || self.text.dragged()))
    }
}

fn audio_marker_rect(rect: egui::Rect, entry: &Entry) -> Option<egui::Rect> {
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

fn entry_column_geometry(column_width: f32, participant: Participant) -> (f32, f32) {
    let margin = 10.0
        + if participant == Participant::Student {
            AUDIO_MARKER_INSET
        } else {
            0.0
        };
    (margin, (column_width - 20.0 - AUDIO_MARKER_INSET).max(20.0))
}

#[derive(Clone, Copy, Default)]
struct EntryMenuOptions {
    show_move: bool,
    can_move_up: bool,
    can_move_down: bool,
    selected: bool,
    multiselect: bool,
    contiguous: bool,
    can_group_up: bool,
    can_group_down: bool,
    has_teacher: bool,
    has_student: bool,
    has_teacher_l1: bool,
    has_teacher_latin: bool,
}

fn card(
    ui: &mut egui::Ui,
    entry: &mut Entry,
    l1_name: &str,
    width: f32,
    focus: bool,
    selected: bool,
    editing: bool,
    options: EntryMenuOptions,
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
                                ui.add(
                                    egui::TextEdit::multiline(&mut entry.text)
                                        .font(entry_font(entry.language, 15.0))
                                        .align(egui::Align2::LEFT_TOP)
                                        .hint_text("Enter text…")
                                        .desired_width(text_width)
                                        .desired_rows(1)
                                        .frame(egui::Frame::NONE),
                                )
                            } else {
                                // Match TextEdit's layout job, including its line height and
                                // trailing whitespace, so switching modes doesn't reflow text.
                                let font = entry_font(entry.language, 15.0);
                                let line_height = ui.fonts_mut(|fonts| fonts.row_height(&font))
                                    + ui.spacing().extra_text_line_spacing;
                                let (text, color) = if entry.text.is_empty() {
                                    ("Enter text…", ui.visuals().weak_text_color())
                                } else {
                                    (entry.text.as_str(), Color32::WHITE)
                                };
                                let mut job = egui::text::LayoutJob::simple(
                                    text.to_owned(),
                                    font,
                                    color,
                                    text_width,
                                );
                                job.keep_trailing_whitespace = true;
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
                                ui.add_space(
                                    (ui.spacing().interact_size.y - response.rect.height())
                                        .max(0.0),
                                );
                                response
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
    if let Some(rect) = audio_marker_rect(frame.response.rect, entry) {
        ui.painter().rect_filled(rect, 0.0, AUDIO_MARKER_COLOR);
    }
    let mut edit_requested = false;
    let mut audio_requested = false;
    if frame.response.container_contains_pointer() {
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
        let response = audio_overlay
            .put(audio_rect, egui::Button::new(""))
            .on_hover_text(if entry.audio.is_some() {
                "Edit attached audio"
            } else {
                "Record or import audio"
            });
        audio_requested = response.clicked();
        let center = audio_rect.center();
        let point = |x, y| center + egui::vec2(x, y);
        let color = if entry.audio.is_some() {
            accent
        } else {
            Color32::WHITE
        };
        audio_overlay.painter().add(egui::Shape::convex_polygon(
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
            audio_overlay.painter().add(egui::Shape::line(
                vec![
                    point(4.0 * scale, -5.0 * scale),
                    point(6.0 * scale, 0.0),
                    point(4.0 * scale, 5.0 * scale),
                ],
                Stroke::new(1.5, color),
            ));
        }
    }
    let mut action = None;
    let right_clicked = frame.response.container_secondary_clicked();
    if right_clicked {
        ui.ctx().memory_mut(|memory| {
            if let Some(id) = memory.focused() {
                memory.surrender_focus(id);
            }
        });
    }
    frame.response.container_context_menu(|ui| {
        let multiple = options.multiselect && options.selected;
        if multiple {
            if ui.button("Duplicate selected").clicked() {
                action = Some(MenuAction::Bulk(BulkAction::Duplicate));
                ui.close();
            }
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
                if visible && ui.button(label).clicked() {
                    action = Some(MenuAction::Bulk(choice));
                    ui.close();
                }
            }
            if options.contiguous {
                if ui
                    .add_enabled(options.can_group_up, egui::Button::new("Move up"))
                    .clicked()
                {
                    action = Some(MenuAction::Bulk(BulkAction::MoveUp));
                    ui.close();
                }
                if ui
                    .add_enabled(options.can_group_down, egui::Button::new("Move down"))
                    .clicked()
                {
                    action = Some(MenuAction::Bulk(BulkAction::MoveDown));
                    ui.close();
                }
            }
            if ui.button("Delete selected").clicked() {
                action = Some(MenuAction::Bulk(BulkAction::Delete));
                ui.close();
            }
        } else {
            if ui.button("Duplicate").clicked() {
                action = Some(MenuAction::Single(EntryAction::Duplicate));
                ui.close();
            }
            if matches!(entry.language, Language::Latin) && ui.button("Auto-macronize").clicked() {
                action = Some(MenuAction::Single(EntryAction::AutoMacronize));
                ui.close();
            }
            let participant_label = match entry.participant {
                Participant::Teacher => "Move to student →",
                Participant::Student => "← Move to teacher",
            };
            if ui.button(participant_label).clicked() {
                action = Some(MenuAction::Single(EntryAction::MoveParticipant));
                ui.close();
            }
            if entry.participant == Participant::Teacher {
                let language_label = match entry.language {
                    Language::L1 => "Switch to Latin",
                    Language::Latin => "Switch to L1",
                };
                if ui.button(language_label).clicked() {
                    action = Some(MenuAction::Single(EntryAction::SwitchLanguage));
                    ui.close();
                }
            }
            if options.show_move {
                if ui
                    .add_enabled(options.can_move_up, egui::Button::new("Move up"))
                    .clicked()
                {
                    action = Some(MenuAction::Single(EntryAction::MoveUp));
                    ui.close();
                }
                if ui
                    .add_enabled(options.can_move_down, egui::Button::new("Move down"))
                    .clicked()
                {
                    action = Some(MenuAction::Single(EntryAction::MoveDown));
                    ui.close();
                }
            }
            if ui.button("Delete").clicked() {
                action = Some(MenuAction::Single(EntryAction::Delete));
                ui.close();
            }
        }
    });
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_wav(frames: usize, rate: u32) -> String {
        use base64::Engine;
        let mut output = std::io::Cursor::new(Vec::new());
        {
            let mut writer = hound::WavWriter::new(
                &mut output,
                hound::WavSpec {
                    channels: 2,
                    sample_rate: rate,
                    bits_per_sample: 16,
                    sample_format: hound::SampleFormat::Int,
                },
            )
            .unwrap();
            for _ in 0..frames * 2 {
                writer.write_sample(0i16).unwrap();
            }
            writer.finalize().unwrap();
        }
        base64::engine::general_purpose::STANDARD.encode(output.into_inner())
    }

    #[test]
    fn lesson_recording_summary_counts_clips_and_gaps() {
        let entry = |audio| Entry {
            participant: Participant::Teacher,
            language: Language::Latin,
            text: String::new(),
            audio,
        };
        let mut lesson = Lesson {
            name: "First".into(),
            expanded: false,
            entries: Vec::new(),
        };
        assert_eq!(
            lesson_recording_summary(&lesson, project::AudioGaps::default()),
            "Recordings incomplete"
        );
        lesson.entries.push(entry(Some(test_wav(4, 4))));
        assert_eq!(
            lesson_recording_summary(&lesson, project::AudioGaps::default()),
            "00:01.0"
        );
        lesson.entries.push(entry(None));
        assert_eq!(
            lesson_recording_summary(&lesson, project::AudioGaps::default()),
            "Recordings incomplete"
        );
        lesson.entries[1].audio = Some(test_wav(8, 4));
        assert_eq!(
            lesson_recording_summary(&lesson, project::AudioGaps::default()),
            "00:03.1"
        );
        lesson.entries[1].participant = Participant::Student;
        assert_eq!(
            lesson_recording_summary(&lesson, project::AudioGaps::default()),
            "00:03.5"
        );
        assert_eq!(
            lesson_recording_summary(
                &lesson,
                project::AudioGaps {
                    same_participant_ms: 200,
                    different_participant_ms: 1200
                }
            ),
            "00:04.2"
        );
        lesson.entries[1].audio = Some("invalid".into());
        assert_eq!(
            lesson_recording_summary(&lesson, project::AudioGaps::default()),
            "Recordings incomplete"
        );
        lesson.entries = vec![entry(Some(test_wav(3600, 1)))];
        assert_eq!(
            lesson_recording_summary(&lesson, project::AudioGaps::default()),
            "1:00:00.0"
        );
    }

    #[test]
    fn cached_lesson_recording_summaries_follow_history_and_new_entries() {
        let mut app = sample_app();
        app.lessons[0].entries.truncate(1);
        assert_eq!(app.recording_summaries()[0], "Recordings incomplete");
        let command = Command::SetAudio {
            lesson: 0,
            index: 0,
            before: None,
            after: Some(test_wav(4, 4)),
        };
        app.apply_command(command.clone(), true);
        app.history.push(command);
        assert_eq!(app.recording_summaries()[0], "00:01.0");
        app.undo();
        assert_eq!(app.recording_summaries()[0], "Recordings incomplete");
        app.redo();
        assert_eq!(app.recording_summaries()[0], "00:01.0");
        app.add(Participant::Student, Language::Latin);
        assert_eq!(app.recording_summaries()[0], "Recordings incomplete");
        app.undo();
        assert_eq!(app.recording_summaries()[0], "00:01.0");
    }

    #[test]
    fn gap_settings_and_participant_changes_refresh_duration_and_undo() {
        let mut app = sample_app();
        app.lessons[0].entries.truncate(2);
        for entry in &mut app.lessons[0].entries {
            entry.participant = Participant::Teacher;
            entry.audio = Some(test_wav(4, 4));
        }
        assert_eq!(app.recording_summaries()[0], "00:02.1");
        let before = app.audio_gaps;
        let after = project::AudioGaps {
            same_participant_ms: 700,
            different_participant_ms: 1200,
        };
        let command = Command::SetAudioGaps { before, after };
        app.apply_command(command.clone(), true);
        app.history.push(command);
        assert_eq!(app.recording_summaries()[0], "00:02.7");
        assert_eq!(app.project_snapshot().audio_gaps, after);
        app.undo();
        assert_eq!(app.recording_summaries()[0], "00:02.1");
        app.redo();
        assert_eq!(app.recording_summaries()[0], "00:02.7");
        let language = app.lessons[0].entries[1].language;
        app.apply_command(
            Command::SetEntryKind {
                lesson: 0,
                index: 1,
                before: (Participant::Teacher, language),
                after: (Participant::Student, Language::Latin),
            },
            true,
        );
        assert_eq!(app.recording_summaries()[0], "00:03.2");
    }

    fn sample_app() -> App {
        let entries = [
            (Participant::Teacher, Language::L1, "Introduction"),
            (Participant::Teacher, Language::Latin, "manus"),
            (Participant::Teacher, Language::L1, "Can you say hand?"),
            (Participant::Student, Language::Latin, "manus"),
            (Participant::Teacher, Language::L1, "What about both?"),
            (Participant::Student, Language::Latin, "duae manūs"),
        ]
        .into_iter()
        .map(|(participant, language, text)| Entry {
            audio: None,
            participant,
            language,
            text: text.into(),
        })
        .collect();
        let project = project::Project::new(
            "Test".into(),
            "English".into(),
            vec![Lesson {
                name: "Body parts".into(),
                entries,
                expanded: true,
            }],
        );
        let mut app = App::default();
        app.replace_project(project, None);
        app
    }

    #[test]
    fn escape_stops_editing_then_deselects_all_entries() {
        let mut app = sample_app();
        app.set_selection(vec![(0, 1), (0, 3)], Some((0, 3)));
        app.editing_entry = Some((0, 3));
        let ctx = egui::Context::default();
        let input = || egui::RawInput {
            events: vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
            ..Default::default()
        };
        let mut output = ctx.run_ui(input(), |ui| {
            app.handle_shortcuts(ui);
        });
        output.textures_delta.clear();
        assert_eq!(app.editing_entry, None);
        assert_eq!(app.selection(), vec![(0, 1), (0, 3)]);
        let mut output = ctx.run_ui(input(), |ui| {
            app.handle_shortcuts(ui);
        });
        output.textures_delta.clear();
        assert!(app.selection().is_empty());
        assert_eq!(app.selection_anchor, None);
        assert!(!app.is_dirty());
    }

    #[test]
    fn double_click_on_background_deselects_but_not_on_entry() {
        use std::cell::Cell;
        for on_entry in [false, true] {
            let ctx = egui::Context::default();
            let mut app = sample_app();
            app.set_selection(vec![(0, 1), (0, 3)], Some((0, 3)));
            let bounds = Cell::new(egui::Rect::NOTHING);
            let mut frame = |events, time| {
                let input = egui::RawInput {
                    time: Some(time),
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(600.0, 400.0),
                    )),
                    events,
                    ..Default::default()
                };
                let mut output = ctx.run_ui(input, |ui| {
                    let viewport = ui.max_rect();
                    let response = card(
                        ui,
                        &mut app.lessons[0].entries[0],
                        "English",
                        250.0,
                        false,
                        false,
                        false,
                        EntryMenuOptions::default(),
                    );
                    bounds.set(response.bounds);
                    if background_double_clicked(
                        ui.ctx(),
                        ui.layer_id(),
                        viewport,
                        &[response.bounds.expand(8.0)],
                    ) {
                        app.deselect_entries();
                    }
                });
                output.textures_delta.clear();
            };
            frame(Vec::new(), 0.0);
            let position = if on_entry {
                bounds.get().center()
            } else {
                egui::pos2(500.0, 300.0)
            };
            let event = |pressed| egui::Event::PointerButton {
                pos: position,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            };
            frame(vec![egui::Event::PointerMoved(position), event(true)], 0.1);
            frame(vec![event(false)], 0.12);
            frame(vec![event(true)], 0.2);
            frame(vec![event(false)], 0.22);
            if on_entry {
                assert_eq!(app.selection(), vec![(0, 1), (0, 3)]);
            } else {
                assert!(app.selection().is_empty());
                assert_eq!(app.selection_anchor, None);
            }
        }
    }

    #[test]
    fn duplicate_entries_preserves_audio_and_is_undoable() {
        let mut app = sample_app();
        app.lessons[0].entries[1].audio = Some(test_wav(4, 4));
        let original = app.lessons.clone();
        app.set_selection(vec![(0, 1)], Some((0, 1)));
        assert!(app.selected_entry_shortcut(keybinds::Action::DuplicateEntry));
        assert_eq!(app.lessons[0].entries[2], original[0].entries[1]);
        assert_eq!(app.selection(), vec![(0, 2)]);
        app.undo();
        assert_eq!(app.lessons, original);
        assert_eq!(app.selection(), vec![(0, 1)]);
        app.redo();
        assert_eq!(app.lessons[0].entries[2], original[0].entries[1]);
        assert_eq!(app.selection(), vec![(0, 2)]);
    }

    #[test]
    fn duplicates_multiple_entries_in_their_original_lessons() {
        let mut app = sample_app();
        app.lessons.push(Lesson {
            name: "Second".into(),
            entries: app.lessons[0].entries[..2].to_vec(),
            expanded: true,
        });
        let original = app.lessons.clone();
        app.set_selection(vec![(0, 0), (0, 2), (1, 1)], Some((0, 2)));
        assert!(app.selected_entry_shortcut(keybinds::Action::DuplicateEntry));
        assert_eq!(app.lessons[0].entries[1], original[0].entries[0]);
        assert_eq!(app.lessons[0].entries[4], original[0].entries[2]);
        assert_eq!(app.lessons[1].entries[2], original[1].entries[1]);
        assert_eq!(app.selection(), vec![(0, 1), (0, 4), (1, 2)]);
        assert_eq!(app.selected_entry, Some((0, 4)));
        app.undo();
        assert_eq!(app.lessons, original);
        assert_eq!(app.selection(), vec![(0, 0), (0, 2), (1, 1)]);
        app.redo();
        assert_eq!(app.selection(), vec![(0, 1), (0, 4), (1, 2)]);
    }

    #[test]
    fn startup_has_no_open_or_default_project() {
        let app = App::default();
        assert!(!app.project_open);
        assert!(app.lessons.is_empty());
        assert!(!app.is_dirty());
    }

    #[test]
    fn export_settings_are_project_specific_dirty_and_undoable() {
        let mut app = sample_app();
        let before = app.export_settings.clone();
        let mut after = before.clone();
        after.paper = export_pdf::Paper::Letter;
        after.font_size = 21.0;
        after.directory = Some(std::path::PathBuf::from("/exports"));
        let command = Command::SetExportSettings {
            before: before.clone(),
            after: after.clone(),
        };
        app.apply_command(command.clone(), true);
        app.history.push(command);
        assert!(app.is_dirty());
        assert_eq!(app.project_snapshot().export_settings, after);
        app.undo();
        assert_eq!(app.export_settings, before);
        assert!(!app.is_dirty());
        app.redo();
        assert_eq!(app.export_settings, after);
        let project = app.project_snapshot();
        let mut restored = App::default();
        restored.replace_project(project, None);
        assert_eq!(restored.export_settings, after);
        restored.replace_project(sample_app().project_snapshot(), None);
        assert_eq!(restored.export_settings, export_pdf::Settings::default());
    }

    #[test]
    fn entry_audio_is_saved_moved_and_undoable() {
        let mut app = sample_app();
        let command = Command::SetAudio {
            lesson: 0,
            index: 1,
            before: None,
            after: Some("UklGRg==".into()),
        };
        app.apply_command(command.clone(), true);
        app.history.push(command);
        assert!(app.is_dirty());
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("audio.loqr");
        app.project_snapshot().save(&path).unwrap();
        assert_eq!(
            project::Project::load(&path).unwrap().lessons[0].entries[1]
                .audio
                .as_deref(),
            Some("UklGRg==")
        );
        app.undo();
        assert!(app.lessons[0].entries[1].audio.is_none());
        app.redo();
        assert_eq!(app.lessons[0].entries[1].audio.as_deref(), Some("UklGRg=="));
        app.entry_action(0, 1, EntryAction::MoveDown);
        assert_eq!(app.lessons[0].entries[2].audio.as_deref(), Some("UklGRg=="));
        app.undo();
        assert_eq!(app.lessons[0].entries[1].audio.as_deref(), Some("UklGRg=="));
    }

    #[test]
    fn audio_marker_is_only_shown_on_the_inner_side_of_attached_entries() {
        let rect = egui::Rect::from_min_max(egui::pos2(100.0, 20.0), egui::pos2(200.0, 80.0));
        let mut entry = Entry {
            participant: Participant::Teacher,
            language: Language::Latin,
            text: String::new(),
            audio: None,
        };
        assert!(audio_marker_rect(rect, &entry).is_none());
        entry.audio = Some("clip".into());
        let teacher = audio_marker_rect(rect, &entry).unwrap();
        assert!(teacher.left() > rect.right());
        assert_eq!(teacher.width(), 4.0);
        entry.participant = Participant::Student;
        let student = audio_marker_rect(rect, &entry).unwrap();
        assert!(student.right() < rect.left());
        assert_eq!(student.width(), 4.0);
    }

    #[test]
    fn entry_markers_remain_outside_the_center_scrollbar() {
        let column_width = 200.0;
        let track =
            egui::Rect::from_center_size(egui::pos2(column_width, 50.0), Vec2::new(14.0, 100.0));
        for participant in [Participant::Teacher, Participant::Student] {
            let (margin, width) = entry_column_geometry(column_width, participant);
            let column_left = if participant == Participant::Student {
                column_width
            } else {
                0.0
            };
            let rect = egui::Rect::from_min_size(
                egui::pos2(column_left + margin, 20.0),
                Vec2::new(width, 60.0),
            );
            let entry = Entry {
                participant,
                language: Language::Latin,
                text: String::new(),
                audio: Some("clip".into()),
            };
            let marker = audio_marker_rect(rect, &entry).unwrap();
            if participant == Participant::Teacher {
                assert!(marker.right() < track.left());
            } else {
                assert!(marker.left() > track.right());
            }
        }
    }

    #[test]
    fn undo_redo_refreshes_open_recorder_clip() {
        let mut app = sample_app();
        app.audio_editor = Some(audio::Editor::for_test(
            (0, 1),
            app.lessons[0].entries[1].clone(),
        ));
        let command = Command::SetAudio {
            lesson: 0,
            index: 1,
            before: None,
            after: Some("UklGRg==".into()),
        };
        app.apply_command(command.clone(), true);
        app.history.push(command);
        assert_eq!(
            app.audio_editor.as_ref().unwrap().clip_for_test(),
            Some("UklGRg==")
        );
        app.undo();
        assert_eq!(app.audio_editor.as_ref().unwrap().clip_for_test(), None);
        assert_eq!(app.audio_editor.as_ref().unwrap().original.audio, None);
        app.redo();
        assert_eq!(
            app.audio_editor.as_ref().unwrap().clip_for_test(),
            Some("UklGRg==")
        );
    }

    #[test]
    fn new_project_starts_untitled_with_details_open() {
        let mut app = App::default();
        app.new_project(&egui::Context::default());
        assert!(app.project_open);
        assert!(app.project_name.is_empty());
        assert!(app.project_details_open);
        assert!(app.focus_project_name);
        assert_eq!(app.lessons.len(), 1);
    }

    #[test]
    fn closing_a_clean_project_returns_to_welcome() {
        let mut app = sample_app();
        app.close_project(&egui::Context::default());
        assert!(!app.project_open);
        assert!(app.lessons.is_empty());
        assert!(!app.is_dirty());
        assert!(app.project_path.is_none());
    }

    #[test]
    fn successful_open_and_save_update_recent_projects() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("course.loqr");
        let source = sample_app().project_snapshot();
        source.save(&path).unwrap();

        let mut app = App::default();
        app.open_path(path.clone(), &egui::Context::default());
        assert!(app.project_open);
        assert_eq!(app.project_snapshot(), source);
        assert_eq!(app.recent_projects.projects.len(), 1);
        assert_eq!(app.recent_projects.projects[0].path, path);
        app.project_name = "Renamed".into();
        assert!(app.save_project(false));
        assert_eq!(app.recent_projects.projects.len(), 1);
        assert_eq!(app.recent_projects.projects[0].name, "Renamed");
        assert_eq!(project::Project::load(&path).unwrap().name, "Renamed");
    }

    #[test]
    fn multiselection_modifiers_and_anchor_work_across_lessons() {
        let mut app = sample_app();
        app.lessons.push(Lesson {
            name: "Next".into(),
            expanded: false,
            entries: vec![
                Entry {
                    audio: None,
                    participant: Participant::Student,
                    language: Language::Latin,
                    text: "one".into(),
                },
                Entry {
                    audio: None,
                    participant: Participant::Student,
                    language: Language::Latin,
                    text: "two".into(),
                },
            ],
        });
        app.select_entry((0, 4), egui::Modifiers::NONE, false);
        app.select_entry((1, 1), egui::Modifiers::SHIFT, false);
        assert_eq!(app.selection(), vec![(0, 4), (0, 5), (1, 0), (1, 1)]);
        assert!(app.selection_is_contiguous());
        app.select_entry((0, 5), egui::Modifiers::ALT, false);
        assert_eq!(app.selection(), vec![(0, 4), (1, 0), (1, 1)]);
        assert!(!app.selection_is_contiguous());
        app.select_entry((0, 1), egui::Modifiers::COMMAND, false);
        assert_eq!(app.selection(), vec![(0, 1), (0, 4), (1, 0), (1, 1)]);
        app.select_entry((1, 0), egui::Modifiers::NONE, true);
        assert_eq!(app.selection(), vec![(0, 1), (0, 4), (1, 0), (1, 1)]);
        app.select_entry((0, 0), egui::Modifiers::NONE, true);
        assert_eq!(app.selection(), vec![(0, 0)]);
    }

    #[test]
    fn clicking_and_dragging_in_an_editor_keeps_editing_active() {
        let ctx = egui::Context::default();
        configure_fonts(&ctx);
        let mut app = sample_app();
        app.lessons[0].entries[0].text = "A long sentence for testing cursor placement".into();
        app.selected_entry = Some((0, 0));
        app.editing_entry = Some((0, 0));
        let text_rect = std::cell::Cell::new(egui::Rect::NOTHING);
        let was_clicked = std::cell::Cell::new(false);
        let mut frame = |events: Vec<egui::Event>, focus: bool| {
            let input = egui::RawInput {
                events,
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(800.0, 600.0),
                )),
                ..Default::default()
            };
            let mut output = ctx.run_ui(input, |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    let editing = app.editing_entry == Some((0, 0));
                    let response = card(
                        ui,
                        &mut app.lessons[0].entries[0],
                        "English",
                        300.0,
                        focus,
                        true,
                        editing,
                        EntryMenuOptions::default(),
                    );
                    text_rect.set(response.text.rect);
                    was_clicked.set(response.clicked);
                    if response.requests_selection(editing) && !focus {
                        app.select_entry((0, 0), egui::Modifiers::NONE, response.right_clicked);
                    }
                });
            });
            output.textures_delta.clear();
        };
        frame(Vec::new(), true);
        let pos = text_rect.get().min + egui::vec2(15.0, 8.0);
        let pointer = |pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        frame(vec![egui::Event::PointerMoved(pos)], false);
        frame(vec![pointer(true)], false);
        frame(vec![pointer(false)], false);
        assert!(was_clicked.get());
        let end = pos + egui::vec2(60.0, 0.0);
        frame(vec![pointer(true)], false);
        frame(vec![egui::Event::PointerMoved(end)], false);
        frame(
            vec![egui::Event::PointerButton {
                pos: end,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
            false,
        );
        frame(vec![egui::Event::Text("X".into())], false);
        assert_eq!(app.editing_entry, Some((0, 0)));
        assert!(app.lessons[0].entries[0].text.contains('X'));
        assert!(!app.lessons[0].entries[0].text.ends_with('X'));
    }

    #[test]
    fn multiselect_delete_and_conversion_are_atomic_and_undoable() {
        let mut app = sample_app();
        let original = app.lessons[0].entries.clone();
        app.select_entry((0, 0), egui::Modifiers::NONE, false);
        app.select_entry((0, 3), egui::Modifiers::COMMAND, false);
        assert!(app.bulk_action(BulkAction::ToStudent));
        assert_eq!(app.lessons[0].entries[0].participant, Participant::Student);
        assert_eq!(app.lessons[0].entries[0].language, Language::Latin);
        assert_eq!(app.lessons[0].entries[3], original[3]);
        app.undo();
        assert_eq!(app.lessons[0].entries, original);
        assert_eq!(app.selection(), vec![(0, 0), (0, 3)]);
        app.redo();
        assert_eq!(app.lessons[0].entries[0].participant, Participant::Student);
        assert!(app.bulk_action(BulkAction::Delete));
        assert!(app.selection().is_empty());
        assert_eq!(app.lessons[0].entries.len(), original.len() - 2);
        app.undo();
        assert_eq!(app.selection(), vec![(0, 0), (0, 3)]);
        assert_eq!(app.lessons[0].entries.len(), original.len());
        app.redo();
        assert_eq!(app.lessons[0].entries.len(), original.len() - 2);
    }

    #[test]
    fn multiselection_tracks_single_entry_history_and_blocks_macronization() {
        let mut app = sample_app();
        app.select_entry((0, 0), egui::Modifiers::NONE, false);
        app.select_entry((0, 3), egui::Modifiers::COMMAND, false);
        assert!(!app.entry_action(0, 3, EntryAction::AutoMacronize));
        app.delete_entry(0, 1);
        assert_eq!(app.selection(), vec![(0, 0), (0, 2)]);
        app.undo();
        assert_eq!(app.selection(), vec![(0, 0), (0, 3)]);
        app.add(Participant::Teacher, Language::Latin);
        assert_eq!(app.selection(), vec![(0, 4)]);
    }

    #[test]
    fn contiguous_groups_move_across_boundaries_without_reordering() {
        let mut app = sample_app();
        for (index, entry) in app.lessons[0].entries.iter_mut().enumerate() {
            entry.text = format!("{index}");
        }
        app.lessons.push(Lesson {
            name: "Next".into(),
            expanded: true,
            entries: vec![
                Entry {
                    audio: None,
                    participant: Participant::Student,
                    language: Language::Latin,
                    text: "6".into(),
                },
                Entry {
                    audio: None,
                    participant: Participant::Student,
                    language: Language::Latin,
                    text: "7".into(),
                },
            ],
        });
        app.select_entry((0, 4), egui::Modifiers::NONE, false);
        app.select_entry((1, 0), egui::Modifiers::SHIFT, false);
        assert!(app.bulk_action(BulkAction::MoveDown));
        assert_eq!(app.lessons[0].entries[4].text, "7");
        assert_eq!(app.lessons[0].entries[5].text, "4");
        assert_eq!(
            app.lessons[1]
                .entries
                .iter()
                .map(|e| e.text.as_str())
                .collect::<Vec<_>>(),
            vec!["5", "6"]
        );
        assert_eq!(app.selection(), vec![(0, 5), (1, 0), (1, 1)]);
        app.undo();
        assert_eq!(app.selection(), vec![(0, 4), (0, 5), (1, 0)]);
        assert_eq!(app.lessons[1].entries[0].text, "6");
        app.select_entry((0, 4), egui::Modifiers::NONE, false);
        app.select_entry((0, 5), egui::Modifiers::SHIFT, false);
        assert!(app.bulk_action(BulkAction::MoveDown));
        assert_eq!(
            app.lessons[1]
                .entries
                .iter()
                .map(|e| e.text.as_str())
                .collect::<Vec<_>>(),
            vec!["4", "5", "6", "7"]
        );
        assert_eq!(app.selection(), vec![(1, 0), (1, 1)]);
        app.undo();
        assert_eq!(app.selection(), vec![(0, 4), (0, 5)]);
        app.select_entry((0, 1), egui::Modifiers::COMMAND, false);
        assert!(!app.bulk_action(BulkAction::MoveUp));
    }

    #[test]
    fn replacing_project_resets_selection_history_and_dirty_state() {
        let mut app = sample_app();
        app.add(Participant::Student, Language::Latin);
        assert!(app.is_dirty());
        let project = project::Project::new(
            "New name".into(),
            "French".into(),
            vec![Lesson {
                name: "Lesson".into(),
                expanded: false,
                entries: vec![],
            }],
        );
        app.replace_project(project.clone(), Some("example.loqr".into()));
        assert_eq!(app.project_snapshot(), project);
        assert!(!app.is_dirty());
        assert_eq!(app.selected_entry, None);
        assert_eq!(app.editing_entry, None);
        assert!(app.history.is_empty());
        assert_eq!(app.scrollbar_offset, Some(0.0));
        app.project_name = "Changed".into();
        app.record_text(TextTarget::ProjectName, "New name".into(), "Changed".into());
        assert!(app.is_dirty());
        app.undo();
        assert_eq!(app.project_name, "New name");
        assert!(!app.is_dirty());
    }

    #[test]
    fn entry_display_and_editor_have_matching_geometry() {
        fn measure(text: &str, editing: bool, language: Language) -> (egui::Pos2, f32) {
            let ctx = egui::Context::default();
            configure_fonts(&ctx);
            let mut measured = None;
            let mut entry = Entry {
                audio: None,
                participant: Participant::Teacher,
                language,
                text: text.into(),
            };
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                ui.spacing_mut().item_spacing = Vec2::ZERO;
                let start = ui.next_widget_position();
                let response = card(
                    ui,
                    &mut entry,
                    "English",
                    180.0,
                    false,
                    true,
                    editing,
                    EntryMenuOptions {
                        show_move: false,
                        can_move_up: false,
                        can_move_down: false,
                        ..Default::default()
                    },
                );
                measured = Some((
                    response.text.rect.min - start.to_vec2(),
                    ui.next_widget_position().y - start.y,
                ));
            });
            output.textures_delta.clear();
            measured.unwrap()
        }
        for text in [
            "",
            "manus",
            "This is a longer entry that wraps over several lines with some more words.",
            "first line\nsecond line",
        ] {
            for language in [Language::L1, Language::Latin] {
                let display = measure(text, false, language);
                let editor = measure(text, true, language);
                assert!(
                    (display.0.x - editor.0.x).abs() < 0.5,
                    "{text}: {display:?} vs {editor:?}"
                );
                assert!(
                    (display.0.y - editor.0.y).abs() < 0.5,
                    "{text}: {display:?} vs {editor:?}"
                );
                assert!(
                    (display.1 - editor.1).abs() < 0.5,
                    "{text}: {display:?} vs {editor:?}"
                );
            }
        }
    }

    #[test]
    fn new_entries_belong_to_the_selected_lesson() {
        let mut app = sample_app();
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
        let mut app = sample_app();
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
        let mut app = sample_app();
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
        let mut app = sample_app();
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
        let mut app = sample_app();
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

    #[test]
    fn entry_column_and_language_changes_are_undoable() {
        let mut app = sample_app();
        app.selected_entry = Some((0, 0));
        app.entry_action(0, 0, EntryAction::MoveParticipant);
        assert_eq!(app.lessons[0].entries[0].participant, Participant::Student);
        assert_eq!(app.lessons[0].entries[0].language, Language::Latin);
        app.undo();
        assert_eq!(app.lessons[0].entries[0].participant, Participant::Teacher);
        assert_eq!(app.lessons[0].entries[0].language, Language::L1);
        app.redo();
        assert_eq!(app.lessons[0].entries[0].participant, Participant::Student);
        app.entry_action(0, 0, EntryAction::MoveParticipant);
        app.entry_action(0, 0, EntryAction::SwitchLanguage);
        assert_eq!(app.lessons[0].entries[0].language, Language::L1);
        app.undo();
        assert_eq!(app.lessons[0].entries[0].language, Language::Latin);
        app.redo();
        assert_eq!(app.lessons[0].entries[0].language, Language::L1);
    }

    #[test]
    fn auto_macronizing_is_latin_only_and_undoable() {
        let mut app = sample_app();
        app.lessons[0].entries[0].text = "puella".into();
        assert!(!app.entry_action(0, 0, EntryAction::AutoMacronize));
        assert_eq!(app.lessons[0].entries[0].text, "puella");
        app.lessons[0].entries[1].text = "Puellas, puellas!".into();
        assert!(app.entry_action(0, 1, EntryAction::AutoMacronize));
        assert!(app.parsing_review.is_none());
        assert_eq!(app.lessons[0].entries[1].text, "Puellās, puellās!");
        assert!(!app.entry_action(0, 1, EntryAction::AutoMacronize));
        app.undo();
        assert_eq!(app.lessons[0].entries[1].text, "Puellas, puellas!");
        app.redo();
        assert_eq!(app.lessons[0].entries[1].text, "Puellās, puellās!");
    }

    #[test]
    fn applying_review_is_undoable_and_refuses_stale_text() {
        let mut app = sample_app();
        app.lessons[0].entries[1].text = "puellas".into();
        let review = parsing_review::Review::new(0, 1, "puellas".into());
        app.apply_review(&review, review.preview());
        assert_eq!(app.lessons[0].entries[1].text, "puellās");
        app.undo();
        assert_eq!(app.lessons[0].entries[1].text, "puellas");
        app.redo();
        assert_eq!(app.lessons[0].entries[1].text, "puellās");
        app.apply_review(&review, "other".into());
        assert_eq!(app.lessons[0].entries[1].text, "puellās");
    }

    #[test]
    fn auto_macronizing_opens_review_for_ambiguous_words() {
        let mut app = sample_app();
        app.lessons[0].entries[1].text = "malum".into();
        assert!(app.entry_action(0, 1, EntryAction::AutoMacronize));
        assert!(app.parsing_review.is_some());
        assert_eq!(app.lessons[0].entries[1].text, "malum");
    }

    #[test]
    fn entry_shortcuts_respect_selection_direction_and_undo() {
        let mut app = sample_app();
        assert!(!app.selected_entry_shortcut(keybinds::Action::MoveToStudent));
        app.selected_entry = Some((0, 0));
        assert!(!app.selected_entry_shortcut(keybinds::Action::MoveToTeacher));
        assert!(app.selected_entry_shortcut(keybinds::Action::MoveToStudent));
        assert_eq!(app.lessons[0].entries[0].participant, Participant::Student);
        assert!(!app.selected_entry_shortcut(keybinds::Action::SwitchLanguage));
        assert!(!app.selected_entry_shortcut(keybinds::Action::MoveToStudent));
        assert!(app.selected_entry_shortcut(keybinds::Action::MoveToTeacher));
        assert!(app.selected_entry_shortcut(keybinds::Action::SwitchLanguage));
        assert_eq!(app.lessons[0].entries[0].language, Language::L1);
        assert!(!app.selected_entry_shortcut(keybinds::Action::MoveEntryUp));
        assert!(app.selected_entry_shortcut(keybinds::Action::MoveEntryDown));
        assert_eq!(app.selected_entry, Some((0, 1)));
        app.undo();
        assert_eq!(app.selected_entry, Some((0, 0)));
    }

    #[test]
    fn moving_entries_crosses_lesson_boundaries_and_undoes() {
        let mut app = sample_app();
        app.lessons.push(Lesson {
            name: "Next".into(),
            entries: vec![Entry {
                audio: None,
                participant: Participant::Student,
                language: Language::Latin,
                text: "next".into(),
            }],
            expanded: false,
        });
        let last = app.lessons[0].entries.len() - 1;
        let moved = app.lessons[0].entries[last].text.clone();
        app.selected_entry = Some((0, last));
        app.entry_action(0, last, EntryAction::MoveDown);
        assert_eq!(app.lessons[1].entries[0].text, moved);
        assert_eq!(app.lessons[1].entries[1].text, "next");
        assert_eq!(app.selected_entry, Some((1, 0)));
        assert!(app.lessons[1].expanded);
        app.undo();
        assert_eq!(app.lessons[0].entries[last].text, moved);
        assert_eq!(app.selected_entry, Some((0, last)));
        app.redo();
        assert_eq!(app.lessons[1].entries[0].text, moved);
        app.entry_action(1, 0, EntryAction::MoveUp);
        assert_eq!(app.lessons[0].entries[last].text, moved);
        assert_eq!(app.selected_entry, Some((0, last)));
        app.undo();
        assert_eq!(app.selected_entry, Some((1, 0)));
        assert_eq!(app.lessons[1].entries[0].text, moved);
    }

    #[test]
    fn moving_within_a_lesson_and_into_an_empty_lesson() {
        let mut app = sample_app();
        app.lessons.push(Lesson {
            name: String::new(),
            entries: vec![],
            expanded: true,
        });
        let last = app.lessons[0].entries.len() - 1;
        app.selected_entry = Some((0, last));
        let original = app.lessons[0].entries[last].text.clone();
        app.entry_action(0, last, EntryAction::MoveUp);
        assert_eq!(app.selected_entry, Some((0, last - 1)));
        app.undo();
        assert_eq!(app.selected_entry, Some((0, last)));
        app.entry_action(0, last, EntryAction::MoveDown);
        assert_eq!(app.lessons[1].entries[0].text, original);
        app.undo();
        assert_eq!(app.lessons[0].entries[last].text, original);
        assert!(app.lessons[1].entries.is_empty());
    }

    #[test]
    fn navigation_and_editing_follow_script_order() {
        let mut app = sample_app();
        let last = app.lessons[0].entries.len() - 1;
        app.new_lesson();
        app.add(Participant::Student, Language::Latin);
        app.lessons[0].expanded = false;
        app.select_adjacent(false);
        assert_eq!(app.selected_entry, Some((0, last)));
        assert!(app.lessons[0].expanded);
        assert_eq!(app.scroll_to_entry, Some((0, last)));
        assert_eq!(app.focus_entry, None);
        assert_eq!(app.editing_entry, None);
        app.select_adjacent(true);
        assert_eq!(app.selected_entry, Some((1, 0)));
        app.edit_selected();
        assert_eq!(app.focus_entry, Some((1, 0)));
        assert_eq!(app.editing_entry, Some((1, 0)));
    }
}
