//! Print-friendly, two-column transcript export.
use std::{
    io::Write,
    path::{Path, PathBuf},
};

use eframe::egui;
use printpdf::{
    Color, LinePoint, Mm, Op, PaintMode, ParsedFont, PdfDocument, PdfFontHandle, PdfPage,
    PdfSaveOptions, Point, Polygon, PolygonRing, Pt, Rgb, TextItem, WindingOrder,
};
use unicode_normalization::UnicodeNormalization;
use unicode_segmentation::UnicodeSegmentation;

use crate::{Language, Lesson, Participant};

const SANS: &[u8] = include_bytes!("../assets/IBMPlexSans-Regular.ttf");
const LATIN: &[u8] = include_bytes!("../assets/SourceSerif4-Regular.ttf");
const MARGIN: f32 = 36.0;
const DEFAULT_FONT_SIZE: f32 = 15.0;
const MIN_FONT_SIZE: f32 = 8.0;
const MAX_FONT_SIZE: f32 = 30.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Paper {
    #[default]
    A4,
    Letter,
    Legal,
    A3,
}

impl Paper {
    fn dimensions(self) -> (f32, f32) {
        match self {
            Self::A4 => (210.0, 297.0),
            Self::Letter => (215.9, 279.4),
            Self::Legal => (215.9, 355.6),
            Self::A3 => (297.0, 420.0),
        }
    }
    fn label(self) -> &'static str {
        match self {
            Self::A4 => "A4 (210 × 297 mm)",
            Self::Letter => "Letter (8.5 × 11 in)",
            Self::Legal => "Legal (8.5 × 14 in)",
            Self::A3 => "A3 (297 × 420 mm)",
        }
    }
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub paper: Paper,
    pub font_size: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub directory: Option<PathBuf>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            paper: Paper::default(),
            font_size: DEFAULT_FONT_SIZE,
            directory: None,
        }
    }
}

impl Settings {
    pub fn validate(&self) -> Result<(), String> {
        if !self.font_size.is_finite() || !(MIN_FONT_SIZE..=MAX_FONT_SIZE).contains(&self.font_size)
        {
            return Err(format!(
                "PDF font size must be between {MIN_FONT_SIZE} and {MAX_FONT_SIZE} pt."
            ));
        }
        Ok(())
    }

    fn initial_directory<'a>(&'a self, fallback: Option<&'a Path>) -> Option<&'a Path> {
        self.directory
            .as_deref()
            .filter(|path| path.is_dir())
            .or(fallback)
    }

    fn record_export(&mut self, path: &Path) {
        self.directory = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .map(Path::to_path_buf);
    }
}

#[derive(Default)]
pub struct Window {
    pub open: bool,
    error: Option<String>,
}

impl Window {
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        name: &str,
        l1: &str,
        lessons: &[Lesson],
        settings: &mut Settings,
        keybinds: &crate::keybinds::Registry,
        directory: Option<&Path>,
        project_path: Option<&Path>,
    ) -> Vec<crate::keybinds::Action> {
        let mut actions = Vec::new();
        if !self.open {
            return actions;
        }
        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("export-pdf"),
            egui::ViewportBuilder::default()
                .with_title("Export transcript")
                .with_inner_size([440.0, 310.0])
                .with_min_inner_size([340.0, 260.0]),
            |ui, _| {
                if ui.input(|input| input.viewport().close_requested()) {
                    self.open = false;
                    return;
                }
                actions.extend(ui.input_mut(|input| keybinds.take_history_actions(input)));
                if !actions.is_empty() {
                    ui.ctx().memory_mut(|memory| {
                        if let Some(id) = memory.focused() {
                            memory.surrender_focus(id);
                        }
                    });
                    // Don't let a focused DragValue commit its stale edit buffer
                    // after the project history has been changed.
                    return;
                }
                egui::Frame::new().inner_margin(16.0).show(ui, |ui| {
                    ui.heading("Export transcript");
                    ui.add_space(12.0);
                    ui.label("Format: PDF");
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        ui.label("Paper size");
                        egui::ComboBox::from_id_salt("pdf_paper")
                            .selected_text(settings.paper.label())
                            .show_ui(ui, |ui| {
                                for paper in [Paper::A4, Paper::Letter, Paper::Legal, Paper::A3] {
                                    ui.selectable_value(&mut settings.paper, paper, paper.label());
                                }
                            });
                    });
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        ui.label("Font size");
                        ui.add(
                            egui::DragValue::new(&mut settings.font_size)
                                .range(MIN_FONT_SIZE..=MAX_FONT_SIZE)
                                .speed(0.5)
                                .suffix(" pt"),
                        );
                    });
                    ui.add_space(12.0);
                    if let Some(error) = &self.error {
                        ui.colored_label(egui::Color32::LIGHT_RED, error);
                    }
                    ui.horizontal(|ui| {
                        if ui.button("Undo").clicked() {
                            actions.push(crate::keybinds::Action::Undo);
                        }
                        if ui.button("Redo").clicked() {
                            actions.push(crate::keybinds::Action::Redo);
                        }
                    });
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button("Export…").clicked() {
                            let mut filename =
                                std::path::PathBuf::from(crate::project::suggested_file_name(name));
                            filename.set_extension("pdf");
                            let mut dialog = rfd::FileDialog::new()
                                .add_filter("PDF document", &["pdf"])
                                .set_file_name(filename.to_string_lossy());
                            if let Some(directory) = settings.initial_directory(directory) {
                                dialog = dialog.set_directory(directory);
                            }
                            if let Some(mut path) = dialog.save_file() {
                                if path.extension().is_none() {
                                    path.set_extension("pdf");
                                    if path.exists()
                                        && rfd::MessageDialog::new()
                                            .set_title("Replace existing file?")
                                            .set_description(format!(
                                                "{} already exists. Replace it?",
                                                path.display()
                                            ))
                                            .set_buttons(rfd::MessageButtons::YesNo)
                                            .show()
                                            != rfd::MessageDialogResult::Yes
                                    {
                                        return;
                                    }
                                }
                                let same_project = project_path.is_some_and(|project| {
                                    project == path
                                        || (project
                                            .canonicalize()
                                            .ok()
                                            .zip(path.canonicalize().ok())
                                            .is_some_and(|(a, b)| a == b))
                                });
                                if same_project {
                                    self.error = Some(
                                        "Choose a different file from the open project.".into(),
                                    );
                                    return;
                                }
                                match save(
                                    &path,
                                    name,
                                    l1,
                                    lessons,
                                    settings.paper,
                                    settings.font_size,
                                ) {
                                    Ok(()) => {
                                        settings.record_export(&path);
                                        self.error = None;
                                        self.open = false;
                                    }
                                    Err(error) => self.error = Some(error),
                                }
                            }
                        }
                        if ui.button("Cancel").clicked() {
                            self.open = false;
                        }
                    });
                });
                if !actions.is_empty() {
                    ui.ctx().memory_mut(|memory| {
                        if let Some(id) = memory.focused() {
                            memory.surrender_focus(id);
                        }
                    });
                }
            },
        );
        actions
    }
}

fn font(bytes: &[u8]) -> Result<ParsedFont, String> {
    ParsedFont::from_bytes(bytes, 0, &mut Vec::new())
        .ok_or_else(|| "Could not load bundled PDF font.".into())
}

fn text_width(font: &ParsedFont, text: &str, size: f32) -> f32 {
    text.chars()
        .map(|c| {
            font.lookup_glyph_index(c as u32)
                .and_then(|id| font.get_glyph_width(id))
                .unwrap_or(font.units_per_em / 2) as f32
        })
        .sum::<f32>()
        * size
        / font.units_per_em as f32
}

fn check_text(font: &ParsedFont, text: &str, field: &str) -> Result<(), String> {
    for character in text.nfc() {
        if !matches!(character, '\n' | '\r' | '\t')
            && font.lookup_glyph_index(character as u32).is_none()
        {
            return Err(format!(
                "Cannot export {field}: the bundled font does not support '{character}' (U+{:04X}).",
                character as u32
            ));
        }
    }
    Ok(())
}

/// Wrap at whitespace when possible and at grapheme boundaries for long words.
fn wrap(text: &str, font: &ParsedFont, size: f32, width: f32) -> Vec<String> {
    let text: String = text
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('\t', "    ")
        .nfc()
        .collect();
    let mut result = Vec::new();
    for paragraph in text.split('\n') {
        let mut line = String::new();
        for grapheme in paragraph.graphemes(true) {
            if !line.is_empty()
                && text_width(font, &line, size) + text_width(font, grapheme, size) > width
            {
                if let Some(split) = line.rfind(char::is_whitespace) {
                    let rest = line[split..].trim_start().to_owned();
                    result.push(line[..split].trim_end().to_owned());
                    line = rest;
                } else {
                    result.push(std::mem::take(&mut line));
                }
            }
            line.push_str(grapheme);
        }
        result.push(line);
    }
    result
}

fn color(r: f32, g: f32, b: f32) -> Color {
    Color::Rgb(Rgb::new(r, g, b, None))
}

struct Layout {
    pages: Vec<PdfPage>,
    ops: Vec<Op>,
    width: f32,
    height: f32,
    mm: (f32, f32),
    y: f32,
    sans_id: PdfFontHandle,
    latin_id: PdfFontHandle,
    sans: ParsedFont,
    latin: ParsedFont,
    project_name: String,
    scale: f32,
}

impl Layout {
    fn rect(&mut self, x: f32, y: f32, width: f32, height: f32, fill: Color) {
        let points = [
            (x, y),
            (x + width, y),
            (x + width, y + height),
            (x, y + height),
        ]
        .into_iter()
        .map(|(x, y)| LinePoint {
            p: Point {
                x: Pt(x),
                y: Pt(self.height - y),
            },
            bezier: false,
        })
        .collect();
        self.ops.push(Op::SetFillColor { col: fill });
        self.ops.push(Op::DrawPolygon {
            polygon: Polygon {
                rings: vec![PolygonRing { points }],
                mode: PaintMode::Fill,
                winding_order: WindingOrder::NonZero,
            },
        });
    }
    fn text(&mut self, x: f32, baseline: f32, text: &str, size: f32, latin: bool, fill: Color) {
        if text.is_empty() {
            return;
        }
        self.ops.extend([
            Op::StartTextSection,
            Op::SetFillColor { col: fill },
            Op::SetFont {
                font: if latin {
                    self.latin_id.clone()
                } else {
                    self.sans_id.clone()
                },
                size: Pt(size),
            },
            Op::SetTextCursor {
                pos: Point {
                    x: Pt(x),
                    y: Pt(self.height - baseline),
                },
            },
            Op::ShowText {
                items: vec![TextItem::Text(text.to_owned())],
            },
            Op::EndTextSection,
        ]);
    }
    fn new_page(&mut self) {
        let scale = self.scale;
        if !self.ops.is_empty() {
            self.pages.push(PdfPage::new(
                Mm(self.mm.0),
                Mm(self.mm.1),
                std::mem::take(&mut self.ops),
            ));
        }
        self.y = MARGIN;
        let project = self.project_name.clone();
        if !project.is_empty() {
            for line in wrap(
                &project,
                &self.sans,
                14.0 * scale,
                self.width - 2.0 * MARGIN,
            )
            .into_iter()
            .take(2)
            {
                self.text(
                    MARGIN,
                    self.y + 14.0 * scale,
                    &line,
                    14.0 * scale,
                    false,
                    color(0.0, 0.0, 0.0),
                );
                self.y += 18.0 * scale;
            }
            self.y += 4.0 * scale;
        }
        self.text(
            MARGIN,
            self.y + 18.0 * scale,
            "Teacher",
            18.0 * scale,
            false,
            color(0.0, 0.0, 0.0),
        );
        self.text(
            self.width / 2.0 + 8.0,
            self.y + 18.0 * scale,
            "Student",
            18.0 * scale,
            false,
            color(0.0, 0.0, 0.0),
        );
        self.y += 30.0 * scale;
    }
    fn lesson_header(&mut self, title: &str, continued: bool) {
        let scale = self.scale;
        let title = if continued {
            format!("{title} (continued)")
        } else {
            title.to_owned()
        };
        for line in wrap(
            &title,
            &self.sans,
            12.0 * scale,
            self.width - 2.0 * MARGIN - 16.0,
        ) {
            if self.y + 24.0 * scale > self.height - MARGIN {
                self.new_page();
            }
            self.rect(
                MARGIN,
                self.y,
                self.width - 2.0 * MARGIN,
                22.0 * scale,
                color(0.9, 0.9, 0.9),
            );
            self.text(
                MARGIN + 8.0,
                self.y + 15.0 * scale,
                &line,
                12.0 * scale,
                false,
                color(0.0, 0.0, 0.0),
            );
            self.y += 22.0 * scale;
        }
        self.y += 6.0 * scale;
    }
    fn entry(&mut self, entry: &crate::Entry, l1: &str, lesson: &str) {
        let scale = self.scale;
        let line_height = 19.0 * scale;
        let bottom_padding = 8.0 * scale;
        let latin = entry.language == Language::Latin;
        let column = (self.width - 2.0 * MARGIN) / 2.0;
        let x = MARGIN
            + if entry.participant == Participant::Student {
                column
            } else {
                0.0
            }
            + 8.0;
        let width = column - 16.0;
        let lines = wrap(
            &entry.text,
            if latin { &self.latin } else { &self.sans },
            15.0 * scale,
            width - 16.0,
        );
        let mut labels = wrap(
            if latin { "Latin" } else { l1 },
            if latin { &self.latin } else { &self.sans },
            12.0 * scale,
            width - 16.0,
        );
        if labels.len() > 3 {
            labels.truncate(3);
            labels[2].push('…');
        }
        let continued = format!(
            "{} (continued)",
            lesson.split(':').next().unwrap_or("Lesson")
        );
        let mut offset = 0;
        while offset < lines.len() {
            let header = (labels.len() as f32 * 15.0 + 14.0) * scale;
            let available = self.height - MARGIN - self.y;
            if available < header + line_height + bottom_padding {
                self.new_page();
                self.lesson_header(&continued, false);
                continue;
            }
            let count = (((available - header - bottom_padding) / line_height).floor() as usize)
                .min(lines.len() - offset)
                .max(1);
            let height = header + count as f32 * line_height + bottom_padding;
            self.rect(
                x,
                self.y,
                width,
                height,
                if latin {
                    color(1.0, 0.91, 0.91)
                } else {
                    color(0.88, 0.96, 0.98)
                },
            );
            for (i, label) in labels.iter().enumerate() {
                self.text(
                    x + 8.0,
                    self.y + (15.0 + i as f32 * 15.0) * scale,
                    label,
                    12.0 * scale,
                    latin,
                    color(0.0, 0.0, 0.0),
                );
            }
            for (i, line) in lines[offset..offset + count].iter().enumerate() {
                self.text(
                    x + 8.0,
                    self.y + header + 15.0 * scale + i as f32 * line_height,
                    line,
                    15.0 * scale,
                    latin,
                    color(0.0, 0.0, 0.0),
                );
            }
            self.y += height + 7.0 * scale;
            offset += count;
            if offset < lines.len() {
                self.new_page();
                self.lesson_header(&continued, false);
            }
        }
    }
}

fn build(
    name: &str,
    l1: &str,
    lessons: &[Lesson],
    paper: Paper,
    font_size: f32,
) -> Result<Vec<u8>, String> {
    if !font_size.is_finite() || !(MIN_FONT_SIZE..=MAX_FONT_SIZE).contains(&font_size) {
        return Err(format!(
            "Font size must be between {MIN_FONT_SIZE} and {MAX_FONT_SIZE} pt."
        ));
    }
    let sans = font(SANS)?;
    let latin = font(LATIN)?;
    check_text(&sans, name, "the project name")?;
    check_text(&sans, l1, "the L1 language name")?;
    for (index, lesson) in lessons.iter().enumerate() {
        check_text(&sans, &lesson.name, &format!("lesson {} name", index + 1))?;
        for (entry_index, entry) in lesson.entries.iter().enumerate() {
            check_text(
                if entry.language == Language::Latin {
                    &latin
                } else {
                    &sans
                },
                &entry.text,
                &format!("lesson {}, entry {}", index + 1, entry_index + 1),
            )?;
        }
    }
    let mut doc = PdfDocument::new(name);
    let sans_id = PdfFontHandle::External(doc.add_font(&sans));
    let latin_id = PdfFontHandle::External(doc.add_font(&latin));
    let mm = paper.dimensions();
    let mut layout = Layout {
        pages: Vec::new(),
        ops: Vec::new(),
        width: mm.0 * 72.0 / 25.4,
        height: mm.1 * 72.0 / 25.4,
        mm,
        y: 0.0,
        sans_id,
        latin_id,
        sans,
        latin,
        project_name: name.nfc().collect(),
        scale: font_size / DEFAULT_FONT_SIZE,
    };
    layout.new_page();
    for (index, lesson) in lessons.iter().enumerate() {
        let title: String = format!("Lesson {}: {}", index + 1, lesson.name)
            .nfc()
            .collect();
        if layout.y + 70.0 * layout.scale > layout.height - MARGIN {
            layout.new_page();
        }
        layout.lesson_header(&title, false);
        for entry in &lesson.entries {
            layout.entry(entry, l1, &title);
        }
        layout.lesson_header(&title, false);
        layout.y += 6.0 * layout.scale;
    }
    layout
        .pages
        .push(PdfPage::new(Mm(mm.0), Mm(mm.1), layout.ops));
    Ok(doc
        .with_pages(layout.pages)
        .save(&PdfSaveOptions::default(), &mut Vec::new()))
}

fn save(
    path: &Path,
    name: &str,
    l1: &str,
    lessons: &[Lesson],
    paper: Paper,
    font_size: f32,
) -> Result<(), String> {
    let bytes = build(name, l1, lessons, paper, font_size)?;
    let parent = path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut file = tempfile::NamedTempFile::new_in(parent)
        .map_err(|e| format!("Could not create PDF: {e}"))?;
    file.write_all(&bytes)
        .and_then(|_| file.as_file().sync_all())
        .map_err(|e| format!("Could not write PDF: {e}"))?;
    file.persist(path)
        .map_err(|e| format!("Could not save {}: {}", path.display(), e.error))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Entry;

    #[test]
    fn export_window_dispatches_history_shortcuts() {
        let ctx = egui::Context::default();
        let mut window = Window {
            open: true,
            ..Window::default()
        };
        let mut settings = Settings::default();
        let mut actions = Vec::new();
        let input = egui::RawInput {
            events: vec![egui::Event::Key {
                key: egui::Key::Z,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::COMMAND,
            }],
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |ui| {
            actions = window.show(
                ui.ctx(),
                "Course",
                "English",
                &[],
                &mut settings,
                &crate::keybinds::Registry::default(),
                None,
                None,
            );
        });
        output.textures_delta.clear();
        assert_eq!(actions, vec![crate::keybinds::Action::Undo]);
    }

    #[test]
    fn export_folder_prefers_existing_saved_directory_and_falls_back() {
        let root = tempfile::tempdir().unwrap();
        let fallback = root.path();
        let folder = root.path().join("exports");
        std::fs::create_dir(&folder).unwrap();
        let mut settings = Settings::default();
        assert_eq!(settings.initial_directory(Some(fallback)), Some(fallback));
        settings.record_export(&folder.join("course.pdf"));
        assert_eq!(
            settings.initial_directory(Some(fallback)),
            Some(folder.as_path())
        );
        std::fs::remove_dir(&folder).unwrap();
        assert_eq!(settings.initial_directory(Some(fallback)), Some(fallback));
        assert_eq!(settings.directory.as_deref(), Some(folder.as_path()));
    }

    #[test]
    fn wrap_preserves_macrons_breaks_and_long_words() {
        let font = font(LATIN).unwrap();
        let lines = wrap("māter\n", &font, 15.0, 30.0);
        assert_eq!(lines.last().unwrap(), "");
        assert!(lines.join("").contains("māter"));
        let word = "antidisestablishmentarianism";
        let wrapped = wrap(word, &font, 15.0, 40.0);
        assert_eq!(wrapped.concat(), word);
        assert!(
            wrapped
                .iter()
                .all(|line| text_width(&font, line, 15.0) <= 40.0)
        );
    }

    #[test]
    fn pdf_contains_collapsed_lessons_and_long_entries() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.pdf");
        let lessons = vec![Lesson {
            name: "Prīma".into(),
            expanded: false,
            entries: vec![
                Entry {
                    participant: Participant::Teacher,
                    language: Language::L1,
                    text: "Teacher introduction".into(),
                    audio: None,
                },
                Entry {
                    participant: Participant::Student,
                    language: Language::Latin,
                    text: format!("{} fīnis", "māter ".repeat(700)),
                    audio: None,
                },
            ],
        }];
        save(
            &path,
            "Latin course",
            "English",
            &lessons,
            Paper::Letter,
            DEFAULT_FONT_SIZE,
        )
        .unwrap();
        assert!(std::fs::read(&path).unwrap().starts_with(b"%PDF-"));
        if let Ok(output) = std::process::Command::new("pdftotext")
            .arg("-layout")
            .arg(&path)
            .arg("-")
            .output()
        {
            assert!(output.status.success());
            let text = String::from_utf8(output.stdout).unwrap();
            assert!(text.contains("Teacher introduction"), "{text}");
            assert!(text.contains("māter"), "{text}");
            assert!(text.contains("fīnis"), "{text}");
            assert_eq!(text.matches("māter").count(), 700);
            assert!(text.matches('\u{c}').count() > 1);
        }
        if let Ok(output) = std::process::Command::new("pdfinfo").arg(&path).output() {
            assert!(output.status.success());
            assert!(String::from_utf8_lossy(&output.stdout).contains("612 x 792 pts"));
        }
    }

    #[test]
    fn export_rejects_missing_glyphs_without_overwriting_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("existing.pdf");
        std::fs::write(&path, "existing").unwrap();
        let lesson = Lesson {
            name: "First".into(),
            expanded: true,
            entries: vec![Entry {
                participant: Participant::Teacher,
                language: Language::L1,
                text: "\u{10ffff}".into(),
                audio: None,
            }],
        };
        let error = save(
            &path,
            "Course",
            "English",
            &[lesson],
            Paper::A4,
            DEFAULT_FONT_SIZE,
        )
        .unwrap_err();
        assert!(error.contains("does not support"));
        assert_eq!(std::fs::read_to_string(path).unwrap(), "existing");
    }

    #[test]
    fn each_paper_size_produces_a_pdf() {
        for paper in [Paper::A4, Paper::Letter, Paper::Legal, Paper::A3] {
            assert!(
                build("Course", "English", &[], paper, DEFAULT_FONT_SIZE)
                    .unwrap()
                    .starts_with(b"%PDF-")
            );
        }
    }

    #[test]
    fn font_size_changes_text_geometry_and_keeps_long_entries() {
        let lessons = vec![Lesson {
            name: "First".into(),
            expanded: true,
            entries: vec![Entry {
                participant: Participant::Student,
                language: Language::Latin,
                text: format!("Probe {} fīnis", "māter ".repeat(150)),
                audio: None,
            }],
        }];
        let dir = tempfile::tempdir().unwrap();
        let mut heights = Vec::new();
        for size in [8.0, 30.0] {
            let path = dir.path().join(format!("{size}.pdf"));
            save(&path, "Course", "English", &lessons, Paper::Letter, size).unwrap();
            if let Ok(output) = std::process::Command::new("pdftotext")
                .arg("-bbox")
                .arg(&path)
                .arg("-")
                .output()
            {
                assert!(output.status.success());
                let text = String::from_utf8(output.stdout).unwrap();
                assert_eq!(text.matches(">māter</word>").count(), 150);
                assert!(text.contains(">fīnis</word>"));
                let probe = text
                    .lines()
                    .find(|line| line.contains(">Probe</word>"))
                    .unwrap();
                let attribute = |name: &str| -> f32 {
                    probe
                        .split(&format!("{name}=\""))
                        .nth(1)
                        .unwrap()
                        .split('"')
                        .next()
                        .unwrap()
                        .parse()
                        .unwrap()
                };
                heights.push(attribute("yMax") - attribute("yMin"));
            }
        }
        if heights.len() == 2 {
            assert!(heights[1] > heights[0] * 3.0);
        }
        assert!(build("Course", "English", &[], Paper::A4, f32::NAN).is_err());
        assert!(build("Course", "English", &[], Paper::A4, 31.0).is_err());
    }
}
