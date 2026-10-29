//! Application-wide shortcuts, independent of the UI that edits them.
use eframe::egui::{self, Event, InputState, Key, KeyboardShortcut, Modifiers};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Undo,
    Redo,
    RedoAlternative,
    OpenProject,
    SaveProject,
    SaveProjectAs,
    EditEntry,
    StopEditing,
    SelectPrevious,
    SelectNext,
    AddTeacherL1,
    AddTeacherLatin,
    AddStudentLatin,
    SwitchLanguage,
    MoveToStudent,
    MoveToTeacher,
    MoveEntryUp,
    MoveEntryDown,
}

struct Binding {
    action: Action,
    label: &'static str,
    shortcut: KeyboardShortcut,
}

pub struct Registry {
    bindings: Vec<Binding>,
    path: Option<PathBuf>,
    stored: BTreeMap<String, StoredShortcut>,
    pub storage_error: Option<String>,
    blocked: bool,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredShortcut {
    key: String,
    #[serde(default)]
    command: bool,
    #[serde(default)]
    ctrl: bool,
    #[serde(default)]
    alt: bool,
    #[serde(default)]
    shift: bool,
    #[serde(default)]
    mac_cmd: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Settings {
    version: u32,
    bindings: BTreeMap<String, StoredShortcut>,
}

impl Action {
    fn id(self) -> &'static str {
        match self {
            Self::Undo => "undo",
            Self::Redo => "redo",
            Self::RedoAlternative => "redo_alternative",
            Self::OpenProject => "open_project",
            Self::SaveProject => "save_project",
            Self::SaveProjectAs => "save_project_as",
            Self::EditEntry => "edit_entry",
            Self::StopEditing => "stop_editing",
            Self::SelectPrevious => "select_previous",
            Self::SelectNext => "select_next",
            Self::AddTeacherL1 => "add_teacher_l1",
            Self::AddTeacherLatin => "add_teacher_latin",
            Self::AddStudentLatin => "add_student_latin",
            Self::SwitchLanguage => "switch_language",
            Self::MoveToStudent => "move_to_student",
            Self::MoveToTeacher => "move_to_teacher",
            Self::MoveEntryUp => "move_entry_up",
            Self::MoveEntryDown => "move_entry_down",
        }
    }
}

impl StoredShortcut {
    fn from_shortcut(shortcut: KeyboardShortcut) -> Self {
        Self {
            key: shortcut.logical_key.name().to_owned(),
            command: shortcut.modifiers.command,
            ctrl: shortcut.modifiers.ctrl,
            alt: shortcut.modifiers.alt,
            shift: shortcut.modifiers.shift,
            mac_cmd: shortcut.modifiers.mac_cmd,
        }
    }
    fn shortcut(&self) -> Result<KeyboardShortcut, String> {
        let key = Key::from_name(&self.key).ok_or_else(|| format!("Unknown key: {}", self.key))?;
        Ok(KeyboardShortcut::new(
            Modifiers {
                command: self.command,
                ctrl: self.ctrl,
                alt: self.alt,
                shift: self.shift,
                mac_cmd: self.mac_cmd,
            },
            key,
        ))
    }
}

impl Default for Registry {
    fn default() -> Self {
        Self {
            bindings: vec![
                Binding {
                    action: Action::Undo,
                    label: "Undo",
                    shortcut: KeyboardShortcut::new(Modifiers::COMMAND, Key::Z),
                },
                Binding {
                    action: Action::OpenProject,
                    label: "Open project",
                    shortcut: KeyboardShortcut::new(Modifiers::COMMAND, Key::O),
                },
                Binding {
                    action: Action::SaveProject,
                    label: "Save project",
                    shortcut: KeyboardShortcut::new(Modifiers::COMMAND, Key::S),
                },
                Binding {
                    action: Action::SaveProjectAs,
                    label: "Save project as",
                    shortcut: KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, Key::S),
                },
                Binding {
                    action: Action::Redo,
                    label: "Redo",
                    shortcut: KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z),
                },
                Binding {
                    action: Action::RedoAlternative,
                    label: "Redo (alternative)",
                    shortcut: KeyboardShortcut::new(Modifiers::COMMAND, Key::Y),
                },
                Binding {
                    action: Action::EditEntry,
                    label: "Edit selected entry",
                    shortcut: KeyboardShortcut::new(Modifiers::COMMAND, Key::E),
                },
                Binding {
                    action: Action::StopEditing,
                    label: "Stop editing entry",
                    shortcut: KeyboardShortcut::new(Modifiers::NONE, Key::Escape),
                },
                Binding {
                    action: Action::SelectPrevious,
                    label: "Select previous entry",
                    shortcut: KeyboardShortcut::new(Modifiers::COMMAND, Key::ArrowUp),
                },
                Binding {
                    action: Action::SelectNext,
                    label: "Select next entry",
                    shortcut: KeyboardShortcut::new(Modifiers::COMMAND, Key::ArrowDown),
                },
                Binding {
                    action: Action::AddTeacherL1,
                    label: "Add teacher L1",
                    shortcut: KeyboardShortcut::new(Modifiers::COMMAND, Key::J),
                },
                Binding {
                    action: Action::AddTeacherLatin,
                    label: "Add teacher Latin",
                    shortcut: KeyboardShortcut::new(Modifiers::COMMAND, Key::K),
                },
                Binding {
                    action: Action::AddStudentLatin,
                    label: "Add student Latin",
                    shortcut: KeyboardShortcut::new(Modifiers::COMMAND, Key::L),
                },
                Binding {
                    action: Action::SwitchLanguage,
                    label: "Switch entry language",
                    shortcut: KeyboardShortcut::new(Modifiers::COMMAND, Key::R),
                },
                Binding {
                    action: Action::MoveToStudent,
                    label: "Move to student",
                    shortcut: KeyboardShortcut::new(Modifiers::COMMAND, Key::ArrowRight),
                },
                Binding {
                    action: Action::MoveToTeacher,
                    label: "Move to teacher",
                    shortcut: KeyboardShortcut::new(Modifiers::COMMAND, Key::ArrowLeft),
                },
                Binding {
                    action: Action::MoveEntryUp,
                    label: "Move entry up",
                    shortcut: KeyboardShortcut::new(
                        Modifiers::COMMAND | Modifiers::SHIFT,
                        Key::ArrowUp,
                    ),
                },
                Binding {
                    action: Action::MoveEntryDown,
                    label: "Move entry down",
                    shortcut: KeyboardShortcut::new(
                        Modifiers::COMMAND | Modifiers::SHIFT,
                        Key::ArrowDown,
                    ),
                },
            ],
            path: None,
            stored: BTreeMap::new(),
            storage_error: None,
            blocked: false,
        }
    }
}

impl Registry {
    pub fn load_user() -> Self {
        match directories::ProjectDirs::from("", "", "loquaris") {
            Some(dirs) => Self::load_from(&dirs.config_dir().join("keybinds.toml")),
            None => Self { storage_error: Some("Could not find a user configuration directory; keybind changes will not be saved.".into()), blocked: true, ..Self::default() },
        }
    }

    fn load_from(path: &Path) -> Self {
        let mut registry = Self {
            path: Some(path.to_owned()),
            ..Self::default()
        };
        let contents = match std::fs::read_to_string(path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return registry,
            Err(error) => {
                registry.storage_error = Some(format!(
                    "Could not read {}: {error}. Keybind changes will not be saved until the problem is resolved.",
                    path.display()
                ));
                registry.blocked = true;
                return registry;
            }
        };
        let load = (|| -> Result<Settings, String> {
            let settings: Settings = toml::from_str(&contents)
                .map_err(|error| format!("Invalid keybind settings: {error}"))?;
            if settings.version != 1 {
                return Err(format!(
                    "Unsupported keybind settings version {}.",
                    settings.version
                ));
            }
            for binding in &mut registry.bindings {
                if let Some(saved) = settings.bindings.get(binding.action.id()) {
                    let shortcut = saved.shortcut()?;
                    if !Self::valid_shortcut(binding.action, shortcut) {
                        return Err(format!("Invalid shortcut for {}.", binding.label));
                    }
                    binding.shortcut = shortcut;
                }
            }
            for (index, binding) in registry.bindings.iter().enumerate() {
                if registry.bindings[..index]
                    .iter()
                    .any(|other| Self::same_shortcut(other.shortcut, binding.shortcut))
                {
                    return Err(format!("Duplicate shortcut for {}.", binding.label));
                }
            }
            Ok(settings)
        })();
        match load {
            Ok(settings) => registry.stored = settings.bindings,
            Err(error) => {
                // Keep the original file untouched, rather than silently replacing it.
                registry.bindings = Self::default().bindings;
                registry.storage_error = Some(format!(
                    "{error} The existing file was left unchanged: {}. Keybind changes will not be saved until the file is fixed or moved.",
                    path.display()
                ));
                registry.blocked = true;
            }
        }
        registry
    }

    fn persist(&mut self) {
        if self.blocked {
            return;
        }
        let Some(path) = self.path.as_ref() else {
            return;
        };
        let save = (|| -> Result<BTreeMap<String, StoredShortcut>, String> {
            let mut bindings = self.stored.clone();
            for binding in &self.bindings {
                bindings.insert(
                    binding.action.id().to_owned(),
                    StoredShortcut::from_shortcut(binding.shortcut),
                );
            }
            let contents = toml::to_string_pretty(&Settings {
                version: 1,
                bindings: bindings.clone(),
            })
            .map_err(|error| error.to_string())?;
            let parent = path.parent().ok_or("Invalid keybind settings path")?;
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
            let mut file =
                tempfile::NamedTempFile::new_in(parent).map_err(|error| error.to_string())?;
            file.write_all(contents.as_bytes())
                .and_then(|_| file.as_file().sync_all())
                .map_err(|error| error.to_string())?;
            file.persist(path)
                .map_err(|error| error.error.to_string())?;
            Ok(bindings)
        })();
        match save {
            Ok(bindings) => {
                self.stored = bindings;
                self.storage_error = None;
            }
            Err(error) => {
                self.storage_error = Some(format!(
                    "Could not save keybinds to {}: {error}",
                    path.display()
                ))
            }
        }
    }

    fn valid_shortcut(action: Action, shortcut: KeyboardShortcut) -> bool {
        shortcut.modifiers.command
            || shortcut.modifiers.ctrl
            || shortcut.modifiers.mac_cmd
            || (action == Action::StopEditing
                && shortcut.logical_key == Key::Escape
                && shortcut.modifiers == Modifiers::NONE)
    }

    fn same_shortcut(a: KeyboardShortcut, b: KeyboardShortcut) -> bool {
        a.logical_key == b.logical_key
            && (a.modifiers.matches_exact(b.modifiers) || b.modifiers.matches_exact(a.modifiers))
    }

    /// Consume only exact matches, so Ctrl+Shift+Z cannot also invoke Ctrl+Z.
    pub fn take_actions(&self, input: &mut InputState, editing_entry: bool) -> Vec<Action> {
        let mut actions = Vec::new();
        input.events.retain(|event| {
            if let Event::Key {
                key,
                modifiers,
                pressed: true,
                ..
            } = event
            {
                if let Some(binding) = self.bindings.iter().find(|binding| {
                    binding.shortcut.logical_key == *key
                        && modifiers.matches_exact(binding.shortcut.modifiers)
                        && (binding.action != Action::StopEditing || editing_entry)
                }) {
                    actions.push(binding.action);
                    return false;
                }
                // The application owns undo history; do not let egui's independent
                // text-field undo history act on old default shortcuts after rebinding.
                if (*key == Key::Z || *key == Key::Y)
                    && modifiers.matches_logically(Modifiers::COMMAND)
                {
                    return false;
                }
            }
            true
        });
        actions
    }

    fn set(&mut self, index: usize, shortcut: KeyboardShortcut) -> Result<(), &'static str> {
        if !Self::valid_shortcut(self.bindings[index].action, shortcut) {
            return Err("Use Ctrl or Command with a key.");
        }
        if self.bindings.iter().enumerate().any(|(other, binding)| {
            other != index && Self::same_shortcut(binding.shortcut, shortcut)
        }) {
            return Err("That shortcut is already assigned.");
        }
        self.bindings[index].shortcut = shortcut;
        Ok(())
    }
}

#[derive(Default)]
pub struct Editor {
    pub open: bool,
    recording: Option<usize>,
    error: Option<&'static str>,
}

impl Editor {
    pub fn is_recording(&self) -> bool {
        self.open && self.recording.is_some()
    }

    pub fn show(&mut self, ctx: &egui::Context, registry: &mut Registry) {
        if !self.open {
            return;
        }
        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("keybind-editor"),
            egui::ViewportBuilder::default()
                .with_title("Keybinds")
                .with_inner_size([480.0, 260.0])
                .with_min_inner_size([340.0, 220.0]),
            |ui, _class| {
                if ui.input(|input| input.viewport().close_requested()) {
                    self.open = false;
                    self.recording = None;
                    return;
                }
                if let Some(index) = self.recording {
                    let key = ui.input_mut(|input| {
                        let key = input.events.iter().find_map(|event| match event {
                            Event::Key {
                                key,
                                modifiers,
                                pressed: true,
                                repeat: false,
                                ..
                            } => Some((*key, *modifiers)),
                            _ => None,
                        });
                        if key.is_some() {
                            input.events.retain(|event| {
                                !matches!(event, Event::Key { pressed: true, .. } | Event::Text(_))
                            });
                        }
                        key
                    });
                    if let Some((key, mut modifiers)) = key {
                        if key == Key::Escape {
                            self.recording = None;
                            self.error = None;
                        } else {
                            // Represent the platform's primary modifier portably.
                            if modifiers.command {
                                modifiers.ctrl = false;
                                modifiers.mac_cmd = false;
                            }
                            match registry.set(index, KeyboardShortcut::new(modifiers, key)) {
                                Ok(()) => {
                                    self.recording = None;
                                    self.error = None;
                                    registry.persist();
                                    ui.ctx().request_repaint_of(egui::ViewportId::ROOT);
                                }
                                Err(error) => self.error = Some(error),
                            }
                        }
                    }
                }
                egui::ScrollArea::vertical().show(ui, |ui| {
                    egui::Frame::new().inner_margin(12.0).show(ui, |ui| {
                        ui.heading("Keybinds");
                        ui.label("Choose a shortcut to change it. Press Escape to cancel.");
                        ui.add_space(12.0);

                        let column_width = ((ui.available_width() - 16.0) / 2.0).max(120.0);
                        egui::Grid::new("keybinds_table")
                            .num_columns(2)
                            .spacing([16.0, 6.0])
                            .striped(true)
                            .show(ui, |ui| {
                                ui.add_sized(
                                    [column_width, 24.0],
                                    egui::Label::new(egui::RichText::new("Action").strong()),
                                );
                                ui.add_sized(
                                    [column_width, 24.0],
                                    egui::Label::new(egui::RichText::new("Shortcut").strong()),
                                );
                                ui.end_row();

                                for (index, binding) in registry.bindings.iter().enumerate() {
                                    ui.add_sized(
                                        [column_width, 28.0],
                                        egui::Label::new(binding.label),
                                    );
                                    let label = if self.recording == Some(index) {
                                        "Press a shortcut…".to_owned()
                                    } else {
                                        ui.ctx().format_shortcut(&binding.shortcut)
                                    };
                                    if ui
                                        .add_sized([column_width, 28.0], egui::Button::new(label))
                                        .clicked()
                                    {
                                        self.recording = Some(index);
                                        self.error = None;
                                    }
                                    ui.end_row();
                                }
                            });

                        if let Some(error) = self.error {
                            ui.add_space(6.0);
                            ui.colored_label(egui::Color32::LIGHT_RED, error);
                        }
                        if let Some(error) = &registry.storage_error {
                            ui.add_space(6.0);
                            ui.colored_label(egui::Color32::LIGHT_RED, error);
                            if !registry.blocked && ui.button("Retry saving").clicked() {
                                registry.persist();
                                ui.ctx().request_repaint_of(egui::ViewportId::ROOT);
                            }
                        }
                        ui.add_space(12.0);
                        if ui.button("Restore defaults").clicked() {
                            registry.bindings = Registry::default().bindings;
                            registry.persist();
                            ui.ctx().request_repaint_of(egui::ViewportId::ROOT);
                            self.recording = None;
                            self.error = None;
                        }
                    });
                });
            },
        );
        if !self.open {
            self.recording = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index(registry: &Registry, action: Action) -> usize {
        registry
            .bindings
            .iter()
            .position(|binding| binding.action == action)
            .unwrap()
    }

    #[test]
    fn keybinds_round_trip_and_defaults_are_restorable() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings").join("keybinds.toml");
        let mut registry = Registry::load_from(&path);
        let undo = index(&registry, Action::Undo);
        let alternative = index(&registry, Action::RedoAlternative);
        registry
            .set(
                undo,
                KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::ALT, Key::U),
            )
            .unwrap();
        registry
            .set(
                alternative,
                KeyboardShortcut::new(Modifiers::COMMAND, Key::B),
            )
            .unwrap();
        registry.persist();
        assert!(registry.storage_error.is_none());

        let mut loaded = Registry::load_from(&path);
        assert!(loaded.storage_error.is_none());
        assert_eq!(
            loaded.bindings[index(&loaded, Action::Undo)].shortcut,
            KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::ALT, Key::U)
        );
        assert_eq!(
            loaded.bindings[index(&loaded, Action::RedoAlternative)].shortcut,
            KeyboardShortcut::new(Modifiers::COMMAND, Key::B)
        );
        assert_eq!(
            loaded.bindings[index(&loaded, Action::StopEditing)].shortcut,
            KeyboardShortcut::new(Modifiers::NONE, Key::Escape)
        );

        loaded.bindings = Registry::default().bindings;
        loaded.persist();
        let restored = Registry::load_from(&path);
        assert_eq!(
            restored.bindings[index(&restored, Action::Undo)].shortcut,
            KeyboardShortcut::new(Modifiers::COMMAND, Key::Z)
        );
        assert!(restored.storage_error.is_none());
    }

    #[test]
    fn missing_and_future_bindings_keep_defaults_and_preserve_unknown_entries() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("keybinds.toml");
        let settings = Settings {
            version: 1,
            bindings: BTreeMap::from([
                (
                    "undo".into(),
                    StoredShortcut::from_shortcut(KeyboardShortcut::new(
                        Modifiers::COMMAND,
                        Key::U,
                    )),
                ),
                (
                    "future_action".into(),
                    StoredShortcut::from_shortcut(KeyboardShortcut::new(
                        Modifiers::COMMAND,
                        Key::F,
                    )),
                ),
            ]),
        };
        std::fs::write(&path, toml::to_string(&settings).unwrap()).unwrap();
        let mut registry = Registry::load_from(&path);
        assert!(registry.storage_error.is_none());
        assert_eq!(
            registry.bindings[index(&registry, Action::Undo)]
                .shortcut
                .logical_key,
            Key::U
        );
        assert_eq!(
            registry.bindings[index(&registry, Action::SaveProject)]
                .shortcut
                .logical_key,
            Key::S
        );
        registry.persist();
        let saved: Settings = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert!(saved.bindings.contains_key("future_action"));
    }

    #[test]
    fn invalid_settings_are_reported_and_not_overwritten() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("keybinds.toml");
        for contents in [
            "invalid = [",
            "version = 9\n[bindings]\n",
            "version = 1\n[bindings.undo]\nkey = 'S'\ncommand = true\n",
        ] {
            std::fs::write(&path, contents).unwrap();
            let mut registry = Registry::load_from(&path);
            assert!(registry.storage_error.is_some());
            assert!(registry.blocked);
            registry.persist();
            assert_eq!(std::fs::read_to_string(&path).unwrap(), contents);
        }
    }

    #[test]
    fn rejects_conflicts_and_text_keys() {
        let mut registry = Registry::default();
        assert!(registry.set(0, registry.bindings[1].shortcut).is_err());
        assert!(
            registry
                .set(0, KeyboardShortcut::new(Modifiers::NONE, Key::A))
                .is_err()
        );
        assert!(
            registry
                .set(0, KeyboardShortcut::new(Modifiers::COMMAND, Key::U))
                .is_ok()
        );
        assert!(
            registry
                .set(0, KeyboardShortcut::new(Modifiers::COMMAND, Key::ArrowUp))
                .is_err()
        );
        assert!(
            registry
                .set(
                    0,
                    KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::ALT, Key::ArrowUp)
                )
                .is_ok()
        );
    }

    #[test]
    fn dispatches_exact_bindings_and_suppresses_old_text_undo() {
        let mut registry = Registry::default();
        let mut input = InputState::default();
        let event = |key, modifiers| Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        };
        input
            .events
            .push(event(Key::Z, Modifiers::COMMAND | Modifiers::SHIFT));
        assert_eq!(registry.take_actions(&mut input, false), vec![Action::Redo]);
        assert!(input.events.is_empty());

        input.events.push(event(Key::ArrowDown, Modifiers::COMMAND));
        assert_eq!(
            registry.take_actions(&mut input, false),
            vec![Action::SelectNext]
        );
        assert!(input.events.is_empty());

        for (key, modifiers, expected) in [
            (Key::O, Modifiers::COMMAND, Action::OpenProject),
            (Key::S, Modifiers::COMMAND, Action::SaveProject),
            (
                Key::S,
                Modifiers::COMMAND | Modifiers::SHIFT,
                Action::SaveProjectAs,
            ),
            (Key::R, Modifiers::COMMAND, Action::SwitchLanguage),
            (Key::ArrowRight, Modifiers::COMMAND, Action::MoveToStudent),
            (Key::ArrowLeft, Modifiers::COMMAND, Action::MoveToTeacher),
            (
                Key::ArrowUp,
                Modifiers::COMMAND | Modifiers::SHIFT,
                Action::MoveEntryUp,
            ),
            (
                Key::ArrowDown,
                Modifiers::COMMAND | Modifiers::SHIFT,
                Action::MoveEntryDown,
            ),
            (Key::ArrowUp, Modifiers::COMMAND, Action::SelectPrevious),
        ] {
            input.events.push(event(key, modifiers));
            assert_eq!(registry.take_actions(&mut input, false), vec![expected]);
        }

        registry
            .set(0, KeyboardShortcut::new(Modifiers::COMMAND, Key::U))
            .unwrap();
        input.events.push(event(Key::Z, Modifiers::COMMAND));
        input.events.push(event(Key::U, Modifiers::COMMAND));
        assert_eq!(registry.take_actions(&mut input, false), vec![Action::Undo]);
        assert!(input.events.is_empty());

        input.events.push(event(Key::Escape, Modifiers::NONE));
        assert!(registry.take_actions(&mut input, false).is_empty());
        assert_eq!(input.events.len(), 1);
        assert_eq!(
            registry.take_actions(&mut input, true),
            vec![Action::StopEditing]
        );
        assert!(input.events.is_empty());
        let stop_index = registry
            .bindings
            .iter()
            .position(|binding| binding.action == Action::StopEditing)
            .unwrap();
        registry
            .set(
                stop_index,
                KeyboardShortcut::new(Modifiers::COMMAND, Key::Q),
            )
            .unwrap();
        input.events.push(event(Key::Escape, Modifiers::NONE));
        input.events.push(event(Key::Q, Modifiers::COMMAND));
        assert_eq!(
            registry.take_actions(&mut input, true),
            vec![Action::StopEditing]
        );
        assert_eq!(input.events.len(), 1);
    }
}
