use crate::{
    Entry, Language, Lesson, Participant,
    app::*,
    audio, export_pdf, keybinds,
    model::lesson_recording_summary,
    parsing_review, project,
    ui::{EntryMenuOptions, audio_marker_rect, background_double_clicked, card, configure_fonts},
    word_index,
};
use eframe::egui::{self, Vec2};

fn test_wav(frames: usize, rate: u32) -> std::sync::Arc<[u8]> {
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
    output.into_inner().into()
}

#[test]
fn lesson_recording_summary_counts_clips_and_gaps() {
    let entry = |audio| Entry {
        participant: Participant::Teacher,
        language: Language::Latin,
        text: String::new(),
        audio,
        trim: None,
        lemmas: Vec::new(),
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
    lesson.entries[1].audio = Some(b"invalid".as_slice().into());
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
fn cached_lesson_recording_summaries_update_with_history_and_new_entries() {
    let mut app = sample_app();
    app.lessons[0].entries.truncate(1);
    assert_eq!(app.recording_summaries()[0], "Recordings incomplete");
    let command = Command::SetAudio {
        lesson: 0,
        index: 0,
        before: (None, None),
        after: (Some(test_wav(4, 4)), None),
    };
    app.apply_command(command.clone(), true);
    app.push_history(command);
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
    app.push_history(command);
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
        trim: None,
        participant,
        language,
        text: text.into(),
        lemmas: Vec::new(),
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
fn derived_word_cache_is_saved_without_marking_content_dirty() {
    let app = sample_app();
    assert!(!app.is_dirty());
    let snapshot = app.project_snapshot_for_save();
    assert!(!snapshot.word_cache.words.is_empty());
    assert!(!app.is_dirty());
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
                    &[],
                    &[],
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
    app.push_history(command);
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
        before: (None, None),
        after: (Some(b"RIFF".as_slice().into()), None),
    };
    app.apply_command(command.clone(), true);
    app.push_history(command);
    assert!(app.is_dirty());
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("audio.loqr");
    app.project_snapshot().save(&path).unwrap();
    assert_eq!(
        project::Project::load(&path).unwrap().lessons[0].entries[1]
            .audio
            .as_deref(),
        Some(b"RIFF".as_slice())
    );
    app.undo();
    assert!(app.lessons[0].entries[1].audio.is_none());
    app.redo();
    assert_eq!(
        app.lessons[0].entries[1].audio.as_deref(),
        Some(b"RIFF".as_slice())
    );
    app.entry_action(0, 1, EntryAction::MoveDown);
    assert_eq!(
        app.lessons[0].entries[2].audio.as_deref(),
        Some(b"RIFF".as_slice())
    );
    app.undo();
    assert_eq!(
        app.lessons[0].entries[1].audio.as_deref(),
        Some(b"RIFF".as_slice())
    );
}

#[test]
fn audio_marker_is_only_shown_on_the_inner_side_of_attached_entries() {
    let rect = egui::Rect::from_min_max(egui::pos2(100.0, 20.0), egui::pos2(200.0, 80.0));
    let mut entry = Entry {
        participant: Participant::Teacher,
        language: Language::Latin,
        text: String::new(),
        audio: None,
        trim: None,
        lemmas: Vec::new(),
    };
    assert!(audio_marker_rect(rect, &entry).is_none());
    entry.audio = Some(b"clip".as_slice().into());
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
            audio: Some(b"clip".as_slice().into()),
            trim: None,
            lemmas: Vec::new(),
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
        before: (None, None),
        after: (Some(b"RIFF".as_slice().into()), None),
    };
    app.apply_command(command.clone(), true);
    app.push_history(command);
    assert_eq!(
        app.audio_editor.as_ref().unwrap().clip_for_test(),
        Some(b"RIFF".as_slice())
    );
    app.undo();
    assert_eq!(app.audio_editor.as_ref().unwrap().clip_for_test(), None);
    assert_eq!(app.audio_editor.as_ref().unwrap().original.audio, None);
    app.redo();
    assert_eq!(
        app.audio_editor.as_ref().unwrap().clip_for_test(),
        Some(b"RIFF".as_slice())
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
                trim: None,
                participant: Participant::Student,
                language: Language::Latin,
                text: "one".into(),
                lemmas: Vec::new(),
            },
            Entry {
                audio: None,
                trim: None,
                participant: Participant::Student,
                language: Language::Latin,
                text: "two".into(),
                lemmas: Vec::new(),
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
                    &[],
                    &[],
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
                trim: None,
                participant: Participant::Student,
                language: Language::Latin,
                text: "6".into(),
                lemmas: Vec::new(),
            },
            Entry {
                audio: None,
                trim: None,
                participant: Participant::Student,
                language: Language::Latin,
                text: "7".into(),
                lemmas: Vec::new(),
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
            trim: None,
            participant: Participant::Teacher,
            language,
            text: text.into(),
            lemmas: Vec::new(),
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
                &[],
                &[],
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
    eprintln!("DBG {:?}", app.lessons[0].entries[1].text);
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
fn reviewed_lemmas_belong_to_each_occurrence_and_are_saved_and_undoable() {
    let mut app = sample_app();
    let sentence = "Dum bellum geritur, pilum bellum feret miles.";
    app.lessons[0].entries[1].text = sentence.into();
    app.lessons[0].entries[3].text = "bellum".into();
    app.word_index.rebuild(&app.lessons);
    let choice = |occurrence, lemma_key: &str| word_index::LemmaChoice {
        token: "bellum".into(),
        occurrence,
        lemma_key: lemma_key.into(),
        extension: false,
    };
    let new_keys = |app: &App| -> Vec<&str> {
        app.word_index
            .new_lemmas_in_lesson(0)
            .iter()
            .map(|lemma| lemma.key)
            .collect()
    };
    let hits_in_sentence = |app: &App, lemma_key| -> Vec<usize> {
        app.word_index
            .lemma_occurrences(lemma_key, false)
            .unwrap_or_default()
            .into_iter()
            .filter(|hit| hit.entry == 1)
            .map(|hit| hit.range.start)
            .collect()
    };
    let positions = |text: &str| (text.find("bellum").unwrap(), text.rfind("bellum").unwrap());

    // Spelled alike, so only the lemma is in question; still ask.
    assert!(app.entry_action(0, 1, EntryAction::AutoMacronize));
    assert!(app.parsing_review.take().is_some());
    let mut review = parsing_review::Review::new(0, 1, sentence.into());
    review.prefer_lemmas(&[choice(0, "bellum"), choice(1, "bellus")]);
    app.apply_review(&review, review.preview());
    let lemmas = app.lessons[0].entries[1].lemmas.clone();
    let (war, pretty) = positions(&app.lessons[0].entries[1].text);
    assert!(lemmas.contains(&choice(0, "bellum")));
    assert!(lemmas.contains(&choice(1, "bellus")));
    assert_eq!(hits_in_sentence(&app, "bellum"), [war]);
    assert_eq!(hits_in_sentence(&app, "bellus"), [pretty]);
    assert!(new_keys(&app).contains(&"bellum"));
    assert!(new_keys(&app).contains(&"bellus"));

    // Reviewing the entry again starts from its choices.
    let mut again = parsing_review::Review::new(0, 1, app.lessons[0].entries[1].text.clone());
    again.prefer_lemmas(&app.lessons[0].entries[1].lemmas);
    assert_eq!(again.lemma_choices(), lemmas);
    // Another entry's bellum is its own word, so it is asked about.
    assert!(app.entry_action(0, 3, EntryAction::AutoMacronize));
    assert!(app.parsing_review.take().is_some());

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("lemmas.loqr");
    app.project_snapshot_for_save().save(&path).unwrap();
    let loaded = project::Project::load(&path).unwrap();
    assert_eq!(loaded.lessons[0].entries[1].lemmas, lemmas);

    app.undo();
    assert_eq!(app.lessons[0].entries[1].text, sentence);
    assert!(app.lessons[0].entries[1].lemmas.is_empty());
    let (before_war, before_pretty) = positions(sentence);
    assert_eq!(
        hits_in_sentence(&app, "bellus"),
        [before_war, before_pretty]
    );
    app.redo();
    assert_eq!(app.lessons[0].entries[1].lemmas, lemmas);
    assert_eq!(hits_in_sentence(&app, "bellus"), [pretty]);
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
fn entry_shortcuts_use_selection_direction_and_undo() {
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
            trim: None,
            participant: Participant::Student,
            language: Language::Latin,
            text: "next".into(),
            lemmas: Vec::new(),
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

#[test]
#[ignore = "profile the generated large project fixture"]
fn profile_large_project_load_and_word_index() {
    fn rss() -> String {
        std::fs::read_to_string("/proc/self/status")
            .unwrap()
            .lines()
            .find(|line| line.starts_with("VmRSS:"))
            .unwrap_or("VmRSS unavailable")
            .to_owned()
    }

    let path = std::path::Path::new("/tmp/opencode/loquaris-word-index-500x1000.loqr");
    let started = std::time::Instant::now();
    let project = project::Project::load(path).unwrap();
    eprintln!(
        "load: {:.3}s; {}; lessons={} entries={} cached_words={}",
        started.elapsed().as_secs_f64(),
        rss(),
        project.lessons.len(),
        project
            .lessons
            .iter()
            .map(|lesson| lesson.entries.len())
            .sum::<usize>(),
        project.word_cache.words.len()
    );

    let started = std::time::Instant::now();
    let index = word_index::WordIndex::from_project(&project.word_cache, &project.lessons);
    eprintln!(
        "index all lessons: {:.3}s; {}",
        started.elapsed().as_secs_f64(),
        rss()
    );
    let started = std::time::Instant::now();
    let persisted = index.persisted_cache();
    eprintln!(
        "cache snapshot: {:.3}s; words={}; {}",
        started.elapsed().as_secs_f64(),
        persisted.words.len(),
        rss()
    );
    let started = std::time::Instant::now();
    drop(index);
    eprintln!(
        "drop index: {:.3}s; {}",
        started.elapsed().as_secs_f64(),
        rss()
    );

    let started = std::time::Instant::now();
    let warm_index = word_index::WordIndex::from_project(&persisted, &project.lessons);
    eprintln!(
        "warm index: {:.3}s; {}",
        started.elapsed().as_secs_f64(),
        rss()
    );

    let project::Project {
        name,
        l1_name,
        lessons,
        export_settings,
        audio_gaps,
        ..
    } = project;
    let mut app = App::default();
    app.project_open = true;
    app.project_name = name;
    app.l1_name = l1_name;
    app.lessons = lessons;
    app.word_index = warm_index;
    app.export_settings = export_settings;
    app.audio_gaps = audio_gaps;
    app.lessons[0].expanded = true;

    let ctx = egui::Context::default();
    configure_fonts(&ctx);
    let mut frame = eframe::Frame::_new_kittest();
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            Vec2::new(800.0, 750.0),
        )),
        ..Default::default()
    };
    let started = std::time::Instant::now();
    let mut output = ctx.run_ui(input, |ui| {
        eframe::App::ui(&mut app, ui, &mut frame);
    });
    output.textures_delta.clear();
    eprintln!(
        "first expanded UI frame: {:.3}s; rendered entries={}; {}",
        started.elapsed().as_secs_f64(),
        app.rendered_entry_count,
        rss()
    );
    assert!(app.rendered_entry_count > 0 && app.rendered_entry_count < 100);
    app.scrollbar_offset = Some(50_000.0);
    let input = egui::RawInput {
        time: Some(1.0),
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            Vec2::new(800.0, 750.0),
        )),
        ..Default::default()
    };
    let mut output = ctx.run_ui(input, |ui| {
        eframe::App::ui(&mut app, ui, &mut frame);
    });
    output.textures_delta.clear();
    eprintln!(
        "scrolled UI frame: rendered entries={}; {}",
        app.rendered_entry_count,
        rss()
    );
    assert!(app.rendered_entry_count > 0 && app.rendered_entry_count < 100);
}

fn render(app: &mut App, ctx: &egui::Context) {
    let mut frame = eframe::Frame::_new_kittest();
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            Vec2::new(1200.0, 750.0),
        )),
        ..Default::default()
    };
    let mut output = ctx.run_ui(input, |ui| eframe::App::ui(app, ui, &mut frame));
    output.textures_delta.clear();
}

#[test]
fn side_panel_searches_lemmas_and_updates_with_edits() {
    let mut app = sample_app();
    let ctx = egui::Context::default();
    configure_fonts(&ctx);
    render(&mut app, &ctx);
    assert!(
        app.word_index
            .new_lemmas_in_lesson(0)
            .iter()
            .any(|lemma| lemma.key == "manus")
    );

    app.search.set_query("manui".into());
    assert!(matches!(
        app.search.mode(),
        crate::search::SearchMode::Lemma(lemma) if lemma.key == "manus"
    ));
    render(&mut app, &ctx);
    let hits = app
        .search
        .hits(&app.lessons, &app.word_index, app.content_generation)
        .to_vec();
    assert_eq!(
        hits.iter().map(|hit| hit.entry).collect::<Vec<_>>(),
        [1, 3, 5],
        "every form of manus in Latin entries"
    );

    // Editing an entry refreshes the results.
    let before = app.lessons[0].entries[1].text.clone();
    app.lessons[0].entries[1].text = "pes".into();
    app.record_text(TextTarget::Entry(0, 1), before, "pes".into());
    render(&mut app, &ctx);
    let hits = app
        .search
        .hits(&app.lessons, &app.word_index, app.content_generation)
        .to_vec();
    assert_eq!(hits.iter().map(|hit| hit.entry).collect::<Vec<_>>(), [3, 5]);
    app.undo();
    let hits = app
        .search
        .hits(&app.lessons, &app.word_index, app.content_generation)
        .len();
    assert_eq!(hits, 3);

    app.reveal_entry((0, 5));
    assert_eq!(app.selection(), [(0, 5)]);
    assert!(
        app.centre_entry
            .is_some_and(|(position, _)| position == (0, 5))
    );
    render(&mut app, &ctx);
}

#[test]
fn text_selected_in_the_transcript_is_searched() {
    let mut app = sample_app();
    let ctx = egui::Context::default();
    configure_fonts(&ctx);
    render(&mut app, &ctx);

    // A settled label selection that didn't start in the transcript is ignored.
    let settle = |app: &mut App, text: &str| {
        let mut frame = eframe::Frame::_new_kittest();
        let mut output = ctx.run_ui(Default::default(), |ui| {
            eframe::App::ui(app, ui, &mut frame);
            ui.ctx()
                .send_cmd(egui::OutputCommand::TextSelectionSettled(text.into()));
        });
        output.textures_delta.clear();
    };
    app.transcript_text_selecting = false;
    settle(&mut app, "Introduction");
    render(&mut app, &ctx);
    assert!(app.search.is_empty());

    app.transcript_text_selecting = true;
    settle(&mut app, "manūs,");
    render(&mut app, &ctx);
    assert_eq!(app.search.query, "manūs");
    assert!(matches!(
        app.search.mode(),
        crate::search::SearchMode::Lemma(lemma) if lemma.key == "manus"
    ));
}

fn render_with(app: &mut App, ctx: &egui::Context, time: f64, events: Vec<egui::Event>) {
    let mut frame = eframe::Frame::_new_kittest();
    let input = egui::RawInput {
        time: Some(time),
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            Vec2::new(1200.0, 750.0),
        )),
        events,
        ..Default::default()
    };
    let mut output = ctx.run_ui(input, |ui| eframe::App::ui(app, ui, &mut frame));
    output.textures_delta.clear();
}

fn press(pos: egui::Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    }
}

#[test]
fn a_single_click_on_entry_text_does_not_search() {
    let mut app = sample_app();
    let ctx = egui::Context::default();
    configure_fonts(&ctx);
    render_with(&mut app, &ctx, 0.0, Vec::new());
    let point = entry_text_point(&app, (0, 4));
    let mut time = 0.0;
    let mut step = |app: &mut App, events| {
        time += 0.05;
        render_with(app, &ctx, time, events);
    };
    step(&mut app, vec![egui::Event::PointerMoved(point)]);
    step(&mut app, vec![press(point, true)]);
    step(&mut app, vec![press(point, false)]);
    for _ in 0..4 {
        step(&mut app, Vec::new());
    }
    assert_eq!(app.search.query, "", "a click selects the entry only");
    assert_eq!(app.selected_entry, Some((0, 4)));
}

fn entry_text_point(app: &App, position: (usize, usize)) -> egui::Pos2 {
    let (_, rect) = *app
        .entry_text_rects
        .iter()
        .find(|(rendered, _)| *rendered == position)
        .expect("entry text is rendered");
    rect.left_center() + Vec2::new(10.0, 0.0)
}

#[test]
fn dragging_or_double_clicking_entry_text_searches_the_selection() {
    let ctx = egui::Context::default();
    configure_fonts(&ctx);

    // Drag across the start of "What about both?".
    let mut app = sample_app();
    render_with(&mut app, &ctx, 0.0, Vec::new());
    let start = entry_text_point(&app, (0, 4)) - Vec2::new(9.0, 0.0);
    let end = start + Vec2::new(30.0, 0.0);
    let mut time = 0.0;
    let mut step = |app: &mut App, events| {
        time += 0.05;
        render_with(app, &ctx, time, events);
    };
    step(&mut app, vec![egui::Event::PointerMoved(start)]);
    step(&mut app, vec![press(start, true)]);
    step(
        &mut app,
        vec![egui::Event::PointerMoved(start + Vec2::new(10.0, 0.0))],
    );
    step(&mut app, vec![egui::Event::PointerMoved(end)]);
    step(&mut app, vec![press(end, false)]);
    for _ in 0..3 {
        step(&mut app, Vec::new());
    }
    assert!(!app.search.is_empty(), "a drag selection is searched");
    assert!(
        "What about both?".starts_with(&app.search.query),
        "searched {:?}",
        app.search.query
    );

    // Double-click "manus".
    let ctx = egui::Context::default();
    configure_fonts(&ctx);
    let mut app = sample_app();
    render_with(&mut app, &ctx, 0.0, Vec::new());
    let word = entry_text_point(&app, (0, 1));
    let mut time = 0.0;
    let mut step = |app: &mut App, events| {
        time += 0.05;
        render_with(app, &ctx, time, events);
    };
    step(&mut app, vec![egui::Event::PointerMoved(word)]);
    step(&mut app, vec![press(word, true)]);
    step(&mut app, vec![press(word, false)]);
    step(&mut app, vec![press(word, true)]);
    step(&mut app, vec![press(word, false)]);
    for _ in 0..3 {
        step(&mut app, Vec::new());
    }
    assert_eq!(app.search.query, "manus");
}

#[test]
fn going_to_an_entry_centres_it_in_the_script() {
    let mut app = sample_app();
    let entries = (0..60)
        .map(|n| Entry {
            audio: None,
            trim: None,
            participant: Participant::Teacher,
            language: Language::L1,
            text: format!("Line {n}"),
            lemmas: Vec::new(),
        })
        .collect();
    app.lessons[0].entries = entries;
    let ctx = egui::Context::default();
    configure_fonts(&ctx);
    // Advance time so the scroll animation finishes.
    let mut time = 0.0;
    let mut frames = |app: &mut App, count: usize| {
        for _ in 0..count {
            time += 0.1;
            let mut frame = eframe::Frame::_new_kittest();
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(1200.0, 750.0),
                )),
                time: Some(time),
                ..Default::default()
            };
            let mut output = ctx.run_ui(input, |ui| eframe::App::ui(app, ui, &mut frame));
            output.textures_delta.clear();
        }
    };
    frames(&mut app, 3);
    app.reveal_entry((0, 30));
    frames(&mut app, 20);
    let rect = app
        .entry_text_rects
        .iter()
        .find(|(position, _)| *position == (0, 30))
        .expect("the entry is drawn")
        .1;
    // Entries off screen aren't drawn, so the first drawn one isn't the first.
    assert!(app.entry_text_rects[0].0.1 > 10, "the script scrolled");
    assert!(
        (rect.center().y - 375.0).abs() < 80.0,
        "entry centred, not at an edge: {rect:?}"
    );
}

#[test]
#[ignore = "uses the generated large project fixture"]
fn going_to_an_entry_in_the_large_project() {
    let path = std::path::Path::new("/tmp/opencode/loquaris-word-index-500x1000.loqr");
    let project = project::Project::load(path).unwrap();
    let mut app = App::default();
    app.replace_project(project, None);
    for lesson in &mut app.lessons[..3] {
        lesson.expanded = true;
    }
    let ctx = egui::Context::default();
    configure_fonts(&ctx);
    let mut time = 0.0;
    let mut frames = |app: &mut App, count: usize| {
        for _ in 0..count {
            time += 1.0 / 60.0;
            let mut frame = eframe::Frame::_new_kittest();
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(1200.0, 750.0),
                )),
                time: Some(time),
                ..Default::default()
            };
            let mut output = ctx.run_ui(input, |ui| eframe::App::ui(app, ui, &mut frame));
            output.textures_delta.clear();
        }
    };
    let centre = |app: &App, target| {
        app.entry_text_rects
            .iter()
            .find(|(position, _)| *position == target)
            .map(|(_, rect)| rect.center().y)
    };
    frames(&mut app, 3);
    // Far jumps cross rows whose heights are only estimated until drawn.
    for target in [(0, 700), (2, 300), (0, 20), (1, 999), (2, 900)] {
        app.reveal_entry(target);
        let mut frames_taken = 0;
        while app.centre_entry.is_some() {
            frames(&mut app, 1);
            frames_taken += 1;
        }
        let centre = centre(&app, target).expect("the entry is on screen");
        eprintln!("{target:?}: centred at {centre} after {frames_taken} frames");
        assert!(frames_taken < 100, "centring settles");
        // The script viewport spans y = 122.5 to 675, centred at 398.75; the
        // text sits a little below the middle of its card.
        assert!(
            (centre - 399.0).abs() < 30.0,
            "{target:?} centred: {centre}"
        );
    }
}

#[test]
fn messages_appear_in_the_window_the_action_came_from() {
    use crate::{app::Origin, toasts::Level};
    let mut app = sample_app();
    let dir = tempfile::tempdir().unwrap();
    app.project_path = Some(dir.path().join("missing").join("course.loqr"));
    assert!(!app.save_project(false));
    assert_eq!(app.toasts.messages().len(), 1);
    assert_eq!(app.toasts.messages()[0].0, Level::Error);

    app.project_details_open = true;
    app.project_path = Some(dir.path().join("course.loqr"));
    assert!(app.from_origin(Origin::Details, |app| app.save_project(false)));
    assert_eq!(
        app.details_toasts.messages(),
        [(Level::Success, "Saved course.loqr", 1)]
    );
    // A message for a window that has closed goes to the main window.
    app.project_details_open = false;
    app.from_origin(Origin::Details, |app| app.messages().info("Hello"));
    assert_eq!(app.toasts.messages().last().unwrap().1, "Hello");
}

#[test]
fn storage_errors_are_reported_once() {
    let mut app = sample_app();
    app.recent_projects.storage_error = Some("Disk full".into());
    app.report_storage_errors();
    app.report_storage_errors();
    assert_eq!(
        app.toasts.messages(),
        [(crate::toasts::Level::Error, "Disk full", 1)]
    );
}
